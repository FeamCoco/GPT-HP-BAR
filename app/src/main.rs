//! GPT-HP-BAR —— Codex 剩余额度悬浮窗 + 托盘
//!
//! 架构：Rust 轮询数据源（datasource.rs）→ emit "usage" 事件 → WebView 前端渲染皮肤；
//! 设置持久化（settings.rs）→ emit "settings" → 前端应用字体/透明度/显示项；
//! 托盘图标按剩余额度动态重绘；`--probe` 走调试台打印数据源探测结果。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod datasource;
mod settings;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[cfg(windows)]
use windows::core::{BOOL, w};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, RECT, RPC_E_CHANGED_MODE, S_FALSE, S_OK};
#[cfg(windows)]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
#[cfg(windows)]
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Descendants};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetWindowPos, GWL_EXSTYLE,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_TOPMOST,
};

/// 托盘句柄，轮询线程里按额度重绘图标
struct TrayState(Mutex<Option<tauri::tray::TrayIcon>>);

/// 当前设置（含轮询间隔的原子读取）
struct AppState {
    settings: Mutex<settings::Settings>,
    poll_secs: Arc<AtomicU32>,
}

fn main() {
    if std::env::args().any(|a| a == "--probe") {
        datasource::probe();
        return;
    }

    let init_settings = settings::load();
    let init_poll = Arc::new(AtomicU32::new(init_settings.poll_secs.clamp(10, 600)));

    tauri::Builder::default()
        .manage(TrayState(Mutex::new(None)))
        .manage(AppState {
            settings: Mutex::new(init_settings),
            poll_secs: init_poll.clone(),
        })
        .setup(move |app| {
            // 悬浮窗默认停靠屏幕右上
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(Some(mon)) = win.current_monitor() {
                    let ms = mon.size();
                    let ws = win.outer_size().unwrap_or_default();
                    let x = ms.width as i32 - ws.width as i32 - 24;
                    let y = 24;
                    let _ = win.set_position(tauri::PhysicalPosition::new(x.max(8), y));
                }
            }

            // 托盘菜单
            let show = MenuItem::with_id(app, "show", "显示 / 隐藏悬浮窗", true, None::<&str>)?;
            let st = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &st, &refresh, &quit])?;

            let tray = tauri::tray::TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("GPT-HP-BAR")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                    "settings" => open_settings_window(app),                    "refresh" => {
                        let u = datasource::fetch_all();
                        let _ = app.emit("usage", &u);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            *app.state::<TrayState>().0.lock().unwrap() = Some(tray);

            // 设置窗口：关闭时隐藏而非销毁，再次打开直接 show
            if let Some(sw) = app.get_webview_window("settings") {
                let hidden = sw.clone();
                sw.on_window_event(move |e| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                });
            }

            // 轮询线程：间隔由设置驱动（save_settings 实时更新原子值）
            let handle = app.handle().clone();
            let poll = init_poll.clone();
            thread::spawn(move || loop {
                let u = datasource::fetch_all();
                let remaining = if u.ok {
                    u.primary.used_percent.or(u.secondary.used_percent).map(|used| (100.0 - used).clamp(0.0, 100.0))
                } else {
                    None
                };
                update_tray(&handle, remaining);
                let _ = handle.emit("usage", &u);
                #[cfg(windows)]
                {
                    let st = handle.state::<AppState>();
                    position_mini(&handle, &st);
                }
                let secs = poll.load(Ordering::Relaxed).max(10) as u32;
                for i in 0..secs {
                    thread::sleep(Duration::from_secs(1));
                    // 任务栏图标群随应用启停实时伸缩，最长 600s 的数据轮询跟不上，
                    // 每 2s 重定位一次（占位没变时 SetWindowPos 幂等，无开销感知）
                    #[cfg(windows)]
                    if i % 2 == 1 {
                        let st = handle.state::<AppState>();
                        position_mini(&handle, &st);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![refresh_now, set_window_size, open_settings, get_settings, save_settings])
        .run(tauri::generate_context!())
        .expect("error while running gpt-hp-bar");
}

#[tauri::command]
fn refresh_now() -> datasource::Usage {
    datasource::fetch_all()
}

/// 前端（主窗口/任务栏挂件）尺寸变化时同步；挂件随后按设置重新锚定任务栏
#[tauri::command]
fn set_window_size(window: tauri::WebviewWindow, w: f64, h: f64, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    window.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    if window.label() == "mini" {
        position_mini(&app, &state);
    } else {
        // 主窗口保持右上停靠
        if let Ok(Some(mon)) = window.current_monitor() {
            let ms = mon.size();
            let ws = window.outer_size().unwrap_or_default();
            let x = ms.width as i32 - ws.width as i32 - 24;
            let _ = window.set_position(tauri::PhysicalPosition::new(x.max(8), 24));
        }
    }
    Ok(())
}

/// Shell_TrayWnd 矩形 + 需要避让的占位区间：
/// 系统子窗口（托盘区/开始按钮兜底）+ 任务栏带内的第三方置顶挂件（如 TrafficMonitor）
#[cfg(windows)]
struct EnumCtx {
    tray: HWND,
    band: RECT,
    pid: u32,
    occ: Vec<RECT>,
}

/// 系统结构子窗口类名（不计入占位，否则 XAML 桥会吞掉整个任务栏）。
/// 注意：MSTaskSwWClass/ReBarWindow32 的 rect **要计入占用**，但只能当兜底——
/// Win11 新版任务栏图标群是 XAML 画的，旧窗口 rect 是过期值（实测 26200：
/// 窗口 rect 1060-1368，真实图标群 1105-1545），精确避让必须走 UIA（见 uia_band_occupancy）。
#[cfg(windows)]
const SYS_CHILD_CLASSES: &[&str] = &[
    "TrayNotifyWnd",
    "Start",
    "Windows.UI.Composition.DesktopWindowContentBridge",
    "XamlExplorerHostIslandWindow",
    "Windows.UI.Input.InputSite.WindowClass",
    "TrayDummySearchControl",
];

#[cfg(windows)]
unsafe extern "system" fn enum_taskbar_widget(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    if hwnd == ctx.tray {
        return BOOL(1); // 任务栏自身不算占位
    }
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.pid {
        return BOOL(1); // 跳过自己
    }
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return BOOL(1);
    }
    let b = &ctx.band;
    // 必须整体落在任务栏带内（最大化普通窗口不在此列——它们在任务栏之下）
    if r.left < b.left - 2 || r.right > b.right + 2 || r.top < b.top - 2 || r.bottom > b.bottom + 2 {
        return BOOL(1);
    }
    if r.right - r.left < 8 || r.bottom - r.top < 8 {
        return BOOL(1);
    }
    // 仅置顶层（第三方挂件都是 topmost）
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOPMOST.0 == 0 {
        return BOOL(1);
    }
    ctx.occ.push(r);
    BOOL(1)
}

/// 枚举任务栏子窗口：第三方挂件（如 TrafficMonitor）是 SetParent 进去的子窗口，
/// EnumWindows 枚举不到，必须走 EnumChildWindows；系统结构类名排除
#[cfg(windows)]
unsafe extern "system" fn enum_tray_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut buf = [0u16; 256];
    let n = GetClassNameW(hwnd, &mut buf);
    let cls = String::from_utf16_lossy(&buf[..n as usize]);
    if SYS_CHILD_CLASSES.iter().any(|s| cls.eq_ignore_ascii_case(s)) {
        return BOOL(1);
    }
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return BOOL(1);
    }
    if r.right - r.left < 8 || r.bottom - r.top < 8 {
        return BOOL(1);
    }
    // GetWindowRect 对子窗口返回的同样是屏幕坐标（无需父客户区换算）
    ctx.occ.push(r);
    BOOL(1)
}

/// UIA 读取任务栏真实可见内容矩形（XAML 元素）。
/// Win11 新版任务栏整个是 XAML 渲染：Start/任务视图/居中图标群都没有对应的 Win32 子窗口
/// （MSTaskSwWClass 的 rect 是过期的），只有 UIA 元素树能拿到真实矩形；
/// 失败时返回空 vec，上层退回窗口结构结果。
#[cfg(windows)]
fn uia_band_occupancy(tray: HWND, band: RECT) -> Vec<RECT> {
    let mut out = Vec::new();
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let inited = hr == S_OK || hr == S_FALSE; // CHANGED_MODE 时不能配对 CoUninitialize
        if inited || hr == RPC_E_CHANGED_MODE {
            let ua: windows::core::Result<IUIAutomation> =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER);
            if let Ok(ua) = ua {
                if let Ok(root) = ua.ElementFromHandle(tray) {
                    if let Ok(cond) = ua.CreateTrueCondition() {
                        if let Ok(arr) = root.FindAll(TreeScope_Descendants, &cond) {
                            let n = arr.Length().unwrap_or(0);
                            let bw = (band.right - band.left).max(1);
                            for i in 0..n {
                                let Ok(el) = arr.GetElement(i) else { continue };
                                let Ok(r) = el.CurrentBoundingRectangle() else { continue };
                                if r.right - r.left < 8 || r.bottom - r.top < 8 {
                                    continue;
                                }
                                // 超出任务栏带的（弹出层/遮罩）不算
                                if r.left < band.left - 2
                                    || r.right > band.right + 2
                                    || r.top < band.top - 2
                                    || r.bottom > band.bottom + 2
                                {
                                    continue;
                                }
                                // 占 ≥90% 带宽的是 XAML 容器（TaskbarFrame/InputSite），不算
                                if (r.right - r.left) * 10 >= bw * 9 {
                                    continue;
                                }
                                out.push(r);
                            }
                        }
                    }
                }
            }
            if inited {
                CoUninitialize();
            }
        }
    }
    out
}

/// Shell_TrayWnd 矩形 + 需要避让的占位区间
#[cfg(windows)]
fn taskbar_band() -> Option<(RECT, Vec<RECT>)> {
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut tr = RECT::default();
        GetWindowRect(tray, &mut tr).ok()?;
        let mut occ = Vec::new();
        let mut has_notify = false;
        let mut has_start = false;
        for cls in ["TrayNotifyWnd", "Start"] {
            let cls_w = windows::core::HSTRING::from(cls);
            if let Ok(h) = FindWindowExW(Some(tray), None, &cls_w, None) {
                let mut r = RECT::default();
                if GetWindowRect(h, &mut r).is_ok() {
                    if cls == "TrayNotifyWnd" { has_notify = true; }
                    if cls == "Start" { has_start = true; }
                    occ.push(r);
                }
            }
        }
        // 新版 Win11 任务栏是纯 XAML，TrayNotifyWnd/Start 可能不存在 → 保守宽度兜底
        if !has_notify {
            occ.push(RECT { left: tr.right - 280, top: tr.top, right: tr.right, bottom: tr.bottom });
        }
        if !has_start {
            occ.push(RECT { left: tr.left, top: tr.top, right: tr.left + 56, bottom: tr.bottom });
        }
        // 子窗口（TrafficMonitor 等第三方挂件）+ 带内第三方顶层窗
        let mut ctx = EnumCtx { tray, band: tr, pid: std::process::id(), occ };
        let _ = EnumWindows(Some(enum_taskbar_widget), LPARAM(&mut ctx as *mut EnumCtx as isize));
        let _ = EnumChildWindows(
            Some(tray),
            Some(enum_tray_child),
            LPARAM(&mut ctx as *mut EnumCtx as isize),
        );
        // UIA 补充真实 XAML 内容（Win11 居中图标群等）——比窗口结构精确
        ctx.occ.extend(uia_band_occupancy(tray, tr));
        Some((tr, ctx.occ))
    }
}

/// 任务栏挂件定位：顶层置顶窗浮在任务栏带上（v1 简化版，视觉等同嵌入），
/// 在空闲区间里选离锚点最近的位置，避开托盘区/开始按钮
#[cfg(windows)]
fn position_mini(app: &AppHandle, state: &State<AppState>) {
    let s = state.settings.lock().unwrap().clone();
    let Some(win) = app.get_webview_window("mini") else { return };
    if !s.mini_enabled {
        let _ = win.hide();
        return;
    }
    let Some((band, occ)) = taskbar_band() else { return };

    let sz = win.outer_size().unwrap_or_default();
    let (mw, mh) = (sz.width as i32, sz.height as i32);
    if mw <= 0 || mh <= 0 { return; }

    let m = 4i32;
    let mut ints: Vec<(i32, i32)> = occ.iter().map(|r| (r.left - m, r.right + m)).collect();
    ints.sort();
    let mut gaps: Vec<(i32, i32)> = Vec::new();
    let mut cur = band.left + m;
    for (a, b) in ints {
        if a - cur > 0 { gaps.push((cur, a)); }
        cur = cur.max(b);
    }
    if band.right - m - cur > 0 { gaps.push((cur, band.right - m)); }
    if gaps.is_empty() { gaps.push((band.left + m, band.right - m)); }

    let a_right = gaps.last().unwrap().1 - mw;
    let a_left = band.left + m;
    let a_center = (band.left + band.right) / 2 - mw / 2;
    let ax = match s.mini_pos.as_str() { "left" => a_left, "center" => a_center, _ => a_right };

    let mut best: Option<i32> = None;
    let mut bs = i32::MAX;
    for (gl, gr) in &gaps {
        if gr - gl >= mw {
            for x in [*gl, gr - mw, gl + (gr - gl - mw) / 2] {
                let sc = (x - ax).abs();
                // 同分取更靠右的：居中锚点两侧等距时优先贴图标群右侧，视觉上更"居中"
                if sc < bs || (sc == bs && best.map_or(false, |b| x > b)) {
                    bs = sc;
                    best = Some(x);
                }
            }
        }
    }
    let x = match best {
        Some(x) => x,
        None => {
            // 没有任何空隙放得下：退而求其次放最大空隙中间（宁可压内容也别贴死左边缘）
            let (gl, gr) = gaps
                .iter()
                .copied()
                .max_by_key(|(gl, gr)| gr - gl)
                .unwrap_or((band.left + m, band.right - m));
            gl + ((gr - gl - mw) / 2).max(0)
        }
    };
    let y = band.top + ((band.bottom - band.top) - mh) / 2;

    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let _ = SetWindowPos(HWND(hwnd.0), None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    let _ = win.show();
}

#[cfg(not(windows))]
fn position_mini(app: &AppHandle, _state: &State<AppState>) {
    if let Some(win) = app.get_webview_window("mini") {
        let _ = win.hide();
    }
}

/// 打开（或聚焦）设置窗口
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    open_settings_window(&app);
    Ok(())
}

fn open_settings_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    // 兜底：静态声明缺失时动态创建
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("GPT-HP-BAR 设置")
        .inner_size(760.0, 560.0)
        .min_inner_size(680.0, 500.0)
        .resizable(true)
        .build();
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> settings::Settings {
    let mut s = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
    // 自启以注册表实际状态为准（防止注册表被外部改动后 UI 显示失真）
    s.autostart = settings::read_autostart();
    s
}

/// 保存设置：写盘 + 自启注册表 + 更新轮询间隔 + 推送给悬浮窗实时应用
#[tauri::command]
fn save_settings(app: AppHandle, s: settings::Settings, state: State<AppState>) -> Result<settings::Settings, String> {
    let mut s = s;
    s.font_scale = s.font_scale.clamp(80, 150);
    s.opacity = s.opacity.clamp(40, 100);
    s.poll_secs = s.poll_secs.clamp(10, 600);
    if !["auto", "green", "amber", "cyan"].contains(&s.accent.as_str()) {
        s.accent = "auto".into();
    }

    settings::apply_autostart(s.autostart)?;
    settings::save(&s)?;

    state.poll_secs.store(s.poll_secs, Ordering::Relaxed);
    *state.settings.lock().map_err(|e| e.to_string())? = s.clone();

    if let Some(w) = app.get_webview_window("main") {
        let _ = w.emit("settings", &s);
    }
    // 任务栏挂件随设置显隐/换位
    #[cfg(windows)]
    position_mini(&app, &state);
    Ok(s)
}

/// 32×32 RGBA 托盘图标：深色圆角底 + 血条，颜色随剩余额度三档变化
fn draw_tray(remaining: Option<f64>) -> tauri::image::Image<'static> {
    const S: usize = 32;
    let mut px = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) * 4;
            let dx = (x as i32 - 16).abs();
            let dy = (y as i32 - 16).abs();
            if dx <= 12 && dy <= 12 {
                px[i] = 16;
                px[i + 1] = 20;
                px[i + 2] = 28;
                px[i + 3] = 240;
            }
        }
    }
    let (x0, x1, y0, y1) = (5usize, 27usize, 13usize, 19usize);
    let color: (u8, u8, u8) = match remaining {
        None => (90, 98, 115),                              // 无数据
        Some(r) if r > 50.0 => (110, 231, 183),             // 绿
        Some(r) if r > 20.0 => (253, 230, 138),             // 黄
        Some(_) => (252, 165, 165),                         // 红
    };
    let span = (x1 - x0) as f64;
    let fill_end = x0 + match remaining {
        Some(r) => (span * (r / 100.0)).round() as usize,
        None => 0,
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * S + x) * 4;
            let (r, g, b) = if x < fill_end { color } else { (62, 70, 88) };
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = 255;
        }
    }
    tauri::image::Image::new_owned(px, S as u32, S as u32)
}

fn update_tray(app: &AppHandle, remaining: Option<f64>) {
    let state = app.state::<TrayState>();
    let tray = match state.0.lock() {
        Ok(g) => g.clone(),
        Err(_) => None,
    };
    if let Some(t) = tray {
        let _ = t.set_icon(Some(draw_tray(remaining)));
    }
}
