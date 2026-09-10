//! GPT-HP-BAR —— Codex 剩余额度悬浮窗 + 托盘
//!
//! 架构：Rust 轮询数据源（datasource.rs）→ emit "usage" 事件 → WebView 前端渲染皮肤；
//! 设置持久化（settings.rs）→ emit "settings" → 前端应用字体/透明度/显示项；
//! 托盘图标按剩余额度动态重绘；`--probe` 走调试台打印数据源探测结果。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod datasource;
mod settings;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};

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
use windows::Win32::UI::HiDpi::GetDpiForWindow;
#[cfg(windows)]
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_SHOWWINDOW, WS_EX_TOPMOST,
};

/// 诊断子命令入口：探针进程声明 DPI 感知。
/// 否则（未声明 DPI 感知时）GetWindowRect 返回被 Windows 虚拟化的逻辑坐标
/// （3840 物理屏上得到 2560），与实机截图对不上，没法交叉核对。
#[cfg(windows)]
fn probe_dpi_aware() {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware();
    }
}

/// 托盘句柄，轮询线程里按额度重绘图标
struct TrayState(Mutex<Option<tauri::tray::TrayIcon>>);

/// 当前设置（含轮询间隔的原子读取）
struct AppState {
    settings: Mutex<settings::Settings>,
    poll_secs: Arc<AtomicU32>,
    /// 任务栏挂件的前端实测逻辑尺寸（set_window_size 记录）。
    /// 定位时优先用它：刚改完尺寸时 `outer_size()` 可能还是旧值，
    /// 按旧宽度算出的 x 会让挂件压进图标区。
    mini_size: Arc<Mutex<(f64, f64)>>,
}

/// 托盘菜单文案：[显示/隐藏, 设置, 立即刷新, 退出]
fn tray_labels(lang: &str) -> [&'static str; 4] {
    if lang == "en" {
        ["Show / Hide overlay", "Settings…", "Refresh now", "Quit"]
    } else {
        ["显示 / 隐藏悬浮窗", "设置…", "立即刷新", "退出"]
    }
}

fn settings_title(lang: &str) -> &'static str {
    if lang == "en" {
        "GPT-HP-BAR Settings"
    } else {
        "GPT-HP-BAR 设置"
    }
}

fn tray_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let [l_show, l_set, l_refresh, l_quit] = tray_labels(lang);
    let show = MenuItem::with_id(app, "show", l_show, true, None::<&str>)?;
    let st = MenuItem::with_id(app, "settings", l_set, true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", l_refresh, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l_quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show, &st, &refresh, &quit])
}

/// 界面语言切换的桌面侧应用：重建托盘菜单 + 更新设置窗口标题
fn apply_lang(app: &AppHandle, lang: &str) {
    let tray = app.state::<TrayState>().0.lock().map(|g| g.clone()).unwrap_or(None);
    if let Some(t) = tray {
        if let Ok(menu) = tray_menu(app, lang) {
            let _ = t.set_menu(Some(menu));
        }
    }
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_title(settings_title(lang));
    }
}

fn main() {
    if std::env::args().any(|a| a == "--probe") {
        datasource::probe();
        return;
    }
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--probe-taskbar") {
        probe_dpi_aware();
        taskbar_probe();
        return;
    }
    // --probe-vd [hwnd]：打印窗口句柄及其是否在当前虚拟桌面（缺省取前台窗口），
    // 用于验证虚拟桌面切换检测（与 ensure_on_current_desktop 同一 API）
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("--probe-vd") {
        probe_dpi_aware();
        let arg = std::env::args().nth(2).and_then(|a| a.parse::<isize>().ok());
        let hwnd = arg.map_or_else(
            || unsafe { GetForegroundWindow() },
            |a| HWND(a as *mut core::ffi::c_void),
        );
        let mgr: windows::core::Result<IVirtualDesktopManager> = unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let ok = hr == S_OK || hr == S_FALSE || hr == RPC_E_CHANGED_MODE;
            if !ok {
                println!("CoInitializeEx failed: {hr:?}");
                return;
            }
            // 探针一次性进程：不配对 CoUninitialize，接口指针用完即随进程回收
            CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER)
        };
        match mgr {
            Ok(m) => match unsafe { m.IsWindowOnCurrentVirtualDesktop(hwnd) } {
                Ok(on) => println!("hwnd {hwnd:?} on_current={}", on.as_bool()),
                Err(e) => println!("hwnd {hwnd:?} IsWindowOnCurrentVirtualDesktop err: {e}"),
            },
            Err(e) => println!("CoCreateInstance VirtualDesktopManager err: {e}"),
        }
        return;
    }

    let init_settings = settings::load();
    let init_poll = Arc::new(AtomicU32::new(init_settings.poll_secs.clamp(10, 600)));
    let init_lang = init_settings.lang.clone();

    tauri::Builder::default()
        .manage(TrayState(Mutex::new(None)))
        .manage(AppState {
            settings: Mutex::new(init_settings),
            poll_secs: init_poll.clone(),
            mini_size: Arc::new(Mutex::new((0.0, 0.0))),
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

            // 托盘菜单（文案随界面语言；设置里切换语言后由 apply_lang 重建）
            let menu = tray_menu(app.handle(), &init_lang)?;

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
                // 先落位任务栏挂件再取数：数据源首查可能耗时 10s+（app-server 超时回退），
                // 不能让挂件干等；此时前端多半已调用 set_window_size，尺寸已就绪
                #[cfg(windows)]
                {
                    let st = handle.state::<AppState>();
                    position_mini(&handle, &st);
                }
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
                    // 虚拟桌面切换后，可见的悬浮类窗口会被 cloaked 在旧桌面上
                    // （WS_VISIBLE 仍在，show() 是 no-op）——重挂到当前桌面；
                    // 用户主动隐藏的（is_visible=false）保持隐藏
                    for label in ["main", "mini"] {
                        if let Some(w) = handle.get_webview_window(label) {
                            if let Ok(h) = w.hwnd() {
                                let h = HWND(h.0);
                                if unsafe { IsWindowVisible(h).as_bool() } {
                                    ensure_on_current_desktop(h);
                                }
                            }
                        }
                    }
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

/// 主窗口尺寸过渡的世代号：连续切换时旧的补间线程看到世代变化立即退出
static RESIZE_GEN: AtomicU64 = AtomicU64::new(0);

/// 主窗口尺寸过渡动画：约 220ms / easeOutCubic（与 card.css 的
/// cubic-bezier(.33,1,.68,1) 对齐，窗口与内容同步变形）。
///
/// 位置策略（修"切换简约/完整版把窗口弹回右上角"）：
///   - 贴屏幕左/右边（阈值 72 逻辑 px）→ 固定该边，展开/收起时向屏幕内侧生长；
///   - 其余情况 → 固定左上角，保持用户拖到的位置；
///   - 最后夹取进当前显示器工作区，避免尺寸变化后越界。
fn animate_main_resize(app: &AppHandle, win: tauri::WebviewWindow, w: f64, h: f64) {
    let my_gen = RESIZE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    thread::spawn(move || {
        let scale = win.scale_factor().unwrap_or(1.0);
        let cur = win.inner_size().unwrap_or_default();
        let pos = win.outer_position().unwrap_or_default();
        let (sw, sh) = (cur.width as f64, cur.height as f64);
        let (px, py) = (pos.x as f64, pos.y as f64);
        let (tw, th) = (w * scale, h * scale);
        // 尺寸不可测 / 变化可忽略：直接落定，不做补间
        if sw < 1.0 || sh < 1.0 || ((sw - tw).abs() < 1.5 && (sh - th).abs() < 1.5) {
            let _ = app.run_on_main_thread(move || {
                let _ = win.set_size(LogicalSize::new(w, h));
            });
            return;
        }

        // 目标位置：固定锚点 + 工作区夹取
        let (mut tx, mut ty) = (px, py);
        if let Ok(Some(mon)) = win.current_monitor() {
            let mp = mon.position();
            let ms = mon.size();
            let (ml, mt) = (mp.x as f64, mp.y as f64);
            let (mr, mb) = (ml + ms.width as f64, mt + ms.height as f64);
            let snap = 72.0 * scale;
            if (mr - (px + sw)).abs() <= snap {
                tx = mr - tw; // 贴右停靠：右边缘不动
            } else if (px - ml).abs() <= snap {
                tx = ml; // 贴左停靠：左边缘不动
            }
            tx = tx.clamp(ml, (mr - tw).max(ml));
            ty = ty.clamp(mt, (mb - th).max(mt));
        }

        const STEPS: u32 = 16;
        for i in 1..=STEPS {
            // 有新的切换请求：本次补间作废（避免两个线程互相拉扯）
            if RESIZE_GEN.load(Ordering::SeqCst) != my_gen {
                return;
            }
            let p = i as f64 / STEPS as f64;
            let e = 1.0 - (1.0 - p).powi(3); // easeOutCubic
            let cw = (sw + (tw - sw) * e).round().max(1.0) as u32;
            let ch = (sh + (th - sh) * e).round().max(1.0) as u32;
            let cx = (px + (tx - px) * e).round() as i32;
            let cy = (py + (ty - py) * e).round() as i32;
            let w2 = win.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = w2.set_size(PhysicalSize::new(cw, ch));
                let _ = w2.set_position(tauri::PhysicalPosition::new(cx, cy));
            });
            if i < STEPS {
                thread::sleep(Duration::from_millis(14));
            }
        }
    });
}

/// 前端（主窗口/任务栏挂件）尺寸变化时同步；挂件随后按设置重新锚定任务栏
#[tauri::command]
fn set_window_size(window: tauri::WebviewWindow, w: f64, h: f64, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    if window.label() == "mini" {
        // 先记录前端实测尺寸（定位用它，避免 outer_size 滞后）
        if let Ok(mut g) = state.mini_size.lock() {
            *g = (w, h);
        }
        window.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        position_mini(&app, &state);
    } else {
        // 主窗口：平滑过渡到新尺寸，并保持当前位置（不再强制拉回右上角）
        animate_main_resize(&app, window, w, h);
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
    let mut out: Vec<RECT> = Vec::new();
    // 带类名的中间结果：用于事后剔除"不可见宿主窗口"（见下方 retains）
    let mut tmp: Vec<(String, RECT)> = Vec::new();
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
                                let cls = el
                                    .CurrentClassName()
                                    .map(|b| b.to_string())
                                    .unwrap_or_default();
                                tmp.push((cls, r));
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
    // 只要 UIA 已经拿到真正的任务栏内容（图标按钮/托盘按钮），就可以把"不可见宿主窗口"
    // 从占位里去掉——`Windows.UI.Input.InputSite.WindowClass` 这类输入宿主横跨大半个
    // 任务栏却不显示任何东西，留着会把空白区误判成占用，害得挂件无处可落。
    // 反过来，若没拿到任何按钮（UIA 部分失效），就保守地全部保留。
    let has_content = tmp.iter().any(|(c, _)| {
        c.contains("TaskListButton") || c.contains("ToggleButton") || c.contains("SystemTray")
    });
    tmp.retain(|(c, _)| {
        !(has_content && SYS_CHILD_CLASSES.iter().any(|s| c.eq_ignore_ascii_case(s)))
    });
    out.extend(tmp.into_iter().map(|(_, r)| r));
    out
}

/// 任务栏带的测量结果
#[cfg(windows)]
struct BandInfo {
    /// Shell_TrayWnd 矩形（物理像素）
    band: RECT,
    /// 需要避让的占位矩形（物理像素，未加间隙；各数据源取并集）
    occ: Vec<RECT>,
    /// 物理/逻辑缩放（96 DPI = 1.0）
    scale: f64,
    /// UIA 拿到的带内元素数；为 0 说明 UIA 探测失败（COM 忙/超时），
    /// 此时占位是不完整的，选位要额外保守
    uia_count: usize,
}

/// Shell_TrayWnd 矩形 + 需要避让的占位区间（系统子窗口 / 第三方挂件 / UIA 元素取并集）
#[cfg(windows)]
fn taskbar_band() -> Option<BandInfo> {
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut tr = RECT::default();
        GetWindowRect(tray, &mut tr).ok()?;
        // DPI 缩放（96=100%）：兜底预留宽度按逻辑像素换算
        let scale = (GetDpiForWindow(tray) as f64 / 96.0).max(1.0);
        let mut occ = Vec::new();
        for cls in ["TrayNotifyWnd", "Start"] {
            let cls_w = windows::core::HSTRING::from(cls);
            if let Ok(h) = FindWindowExW(Some(tray), None, &cls_w, None) {
                let mut r = RECT::default();
                if GetWindowRect(h, &mut r).is_ok() {
                    occ.push(r);
                }
            }
        }
        // 左右结构保护区：新版 Win11 任务栏是纯 XAML，TrayNotifyWnd/Start 的 rect
        // 是过期值（可能过窄）或不存在，UIA 也可能临时拿不到 —— 无论探测结果如何
        // 都按 DPI 缩放预留托盘/开始按钮的逻辑宽度兜底（与实际 rect 并集取更靠左者）
        occ.push(RECT { left: tr.right - (280.0 * scale).round() as i32, top: tr.top, right: tr.right, bottom: tr.bottom });
        occ.push(RECT { left: tr.left, top: tr.top, right: tr.left + (56.0 * scale).round() as i32, bottom: tr.bottom });
        // 子窗口（TrafficMonitor 等第三方挂件）+ 带内第三方顶层窗
        let mut ctx = EnumCtx { tray, band: tr, pid: std::process::id(), occ };
        let _ = EnumWindows(Some(enum_taskbar_widget), LPARAM(&mut ctx as *mut EnumCtx as isize));
        let _ = EnumChildWindows(
            Some(tray),
            Some(enum_tray_child),
            LPARAM(&mut ctx as *mut EnumCtx as isize),
        );
        // UIA 补充真实 XAML 内容（Win11 居中图标群等）——比窗口结构精确
        let uia = uia_band_occupancy(tray, tr);
        let uia_count = uia.len();
        ctx.occ.extend(uia);
        Some(BandInfo { band: tr, occ: ctx.occ, scale, uia_count })
    }
}

/// 挂件与任何占位矩形之间必须保留的间隙（逻辑像素）。
/// 原来是 4——而窗口四周还有 4px 透明留白，视觉间隙直接为 0，看起来就是
/// "贴着/压着"系统图标；放大到 14 后即使占位数据有轻微滞后也不会视觉重叠。
#[cfg(windows)]
const PAD_LOGICAL: f64 = 14.0;
/// 中央图标群右侧预留的生长余量（逻辑像素）：新开/关闭应用会在图标群右侧增删图标，
/// 而重定位最快也要 2s，不预留就会被刚冒出来的图标压住
#[cfg(windows)]
const ICON_GROW_LOGICAL: f64 = 44.0;
/// 托盘左侧预留的生长余量（逻辑像素）：新通知图标会插到托盘左侧、把托盘整体推左
#[cfg(windows)]
const TRAY_GROW_LOGICAL: f64 = 32.0;
/// UIA 探测失败（带内元素数为 0）时追加的保守余量（逻辑像素）
#[cfg(windows)]
const UIA_FAIL_PAD_LOGICAL: f64 = 24.0;

/// 占位矩形 -> 互不相交的 x 区间（缝隙 ≤2px 视为同一块，
/// 避免图标之间 1~2px 的缝被误判成可落位的空隙）
#[cfg(windows)]
fn merge_x_intervals(rects: &[RECT]) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = rects
        .iter()
        .filter(|r| r.right > r.left)
        .map(|r| (r.left, r.right))
        .collect();
    v.sort();
    let mut out: Vec<(i32, i32)> = Vec::new();
    for (a, b) in v.drain(..) {
        match out.last_mut() {
            Some(last) if a <= last.1 + 2 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// 选位计算（纯函数：position_mini 与 --probe-taskbar 共用，便于校验）
/// 返回 (x, y, 收缩后的空闲区间, pad)
#[cfg(windows)]
fn plan_mini(info: &BandInfo, mw: i32, mh: i32, pos: &str) -> (i32, i32, Vec<(i32, i32)>, i32) {
    let band = info.band;
    let scale = info.scale;
    let mut pad = (PAD_LOGICAL * scale).round() as i32;
    if info.uia_count == 0 {
        // UIA 拿不到（COM 忙/超时）：占位只剩窗口结构，而 Win11 上窗口 rect 可能是
        // 过期值（图标群/托盘真实范围更大）——再追加一层保守余量
        pad += (UIA_FAIL_PAD_LOGICAL * scale).round() as i32;
    }
    let grow = (ICON_GROW_LOGICAL * scale).round() as i32;
    let tgrow = (TRAY_GROW_LOGICAL * scale).round() as i32;

    // 占位区间：跨任务栏中心的那块 = 中央图标群（右侧留生长余量）；
    // 贴住任务栏右端的那块 = 托盘/系统区（左侧留生长余量）
    let mut blocks = merge_x_intervals(&info.occ);
    let center = (band.left + band.right) / 2;
    for (a, b) in blocks.iter_mut() {
        if *a <= center && center <= *b {
            *b = (*b + grow).min(band.right);
        }
        if *b >= band.right - 2 {
            *a = (*a - tgrow).max(band.left);
        }
    }
    blocks.sort();
    let mut merged: Vec<(i32, i32)> = Vec::new();
    for (a, b) in blocks {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 2 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }

    // 再按 pad 收缩出真正可用的空隙
    let mut gaps: Vec<(i32, i32)> = Vec::new();
    let mut cur = band.left + pad;
    for (a, b) in merged.iter().copied() {
        let (a, b) = (a - pad, b + pad);
        if a - cur > 0 {
            gaps.push((cur, a));
        }
        cur = cur.max(b);
    }
    if band.right - pad - cur > 0 {
        gaps.push((cur, band.right - pad));
    }
    if gaps.is_empty() {
        gaps.push((band.left + pad, band.right - pad));
    }

    let a_left = gaps.first().map(|g| g.0).unwrap_or(band.left + pad);
    let a_right = gaps.last().map(|g| g.1 - mw).unwrap_or(band.right - pad - mw);
    let a_center = (band.left + band.right) / 2 - mw / 2;
    let ax = match pos {
        "left" => a_left,
        "center" => a_center,
        _ => a_right,
    };

    let mut best: Option<i32> = None;
    let mut bs = i32::MAX;
    for (gl, gr) in gaps.iter().copied() {
        if gr - gl < mw {
            continue;
        }
        for x in [gl, gr - mw, gl + (gr - gl - mw) / 2] {
            let sc = (x - ax).abs();
            // 同分取更靠右的：居中锚点两侧等距时优先贴图标群右侧，视觉上更"居中"
            if sc < bs || (sc == bs && best.map_or(false, |b| x > b)) {
                bs = sc;
                best = Some(x);
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
                .unwrap_or((band.left + pad, band.right - pad));
            gl + ((gr - gl - mw) / 2).max(0)
        }
    };
    let y = band.top + ((band.bottom - band.top) - mh) / 2;
    (x, y, gaps, pad)
}

/// 交叉校验：挂件框不得与任何**原始**占位矩形相交
/// （生长余量只是额外保守量，不参与校验，否则会被自己加的余量误判）
#[cfg(windows)]
fn rect_free(x: i32, mw: i32, occ: &[RECT]) -> bool {
    occ.iter().all(|r| x + mw <= r.left || x >= r.right)
}

/// 任务栏挂件定位：顶层置顶窗浮在任务栏带上（v1 简化版，视觉等同嵌入），
/// 在空闲区间里选离锚点最近的位置，避开图标群/托盘/开始按钮/第三方挂件。
/// 落位前对占位做一次交叉校验，校验不过再退到最大空隙中间。
#[cfg(windows)]
fn position_mini(app: &AppHandle, state: &State<AppState>) {
    let s = state.settings.lock().unwrap().clone();
    let Some(win) = app.get_webview_window("mini") else { return };
    if !s.mini_enabled {
        let _ = win.hide();
        return;
    }
    let Some(info) = taskbar_band() else { return };

    // 注意：不能用 outer_size()==0 作为放弃条件——窗口从未 show 过时 tao 侧
    // 尺寸可能尚未回填（前端 set_window_size 未到达），直接 abort 会造成
    // "永不显示 -> 尺寸永缺" 的死锁。尺寸优先级：前端实测（最新）> outer_size
    // （刚改尺寸时可能还是旧值）> 经验估算；三者取大者，宁可多留空间。
    let scale = info.scale;
    let rec = state.mini_size.lock().map(|g| *g).unwrap_or((0.0, 0.0));
    let sz = win.outer_size().unwrap_or_default();
    let (mut mw, mut mh) = if rec.0 > 1.0 && rec.1 > 1.0 {
        ((rec.0 * scale).round() as i32, (rec.1 * scale).round() as i32)
    } else {
        (sz.width as i32, sz.height as i32)
    };
    if mw <= 0 || mh <= 0 {
        // 保守估计（逻辑 px * 缩放），只影响首次落位，之后会被真实尺寸替代
        mw = (160.0 * scale).round() as i32;
        mh = (48.0 * scale).round() as i32;
    }
    mw = mw.max(sz.width as i32);
    mh = mh.max(sz.height as i32);

    let (mut x, y, gaps, pad) = plan_mini(&info, mw, mh, s.mini_pos.as_str());
    if !rect_free(x, mw, &info.occ) {
        // 兜底：从最大空隙中间取一个不压占位的位置
        let (gl, gr) = gaps
            .iter()
            .copied()
            .max_by_key(|(gl, gr)| gr - gl)
            .unwrap_or((info.band.left + pad, info.band.right - pad));
        let cand = gl + ((gr - gl - mw) / 2).max(0);
        if rect_free(cand, mw, &info.occ) {
            x = cand;
        }
        // 两次校验都不过：仍按计划位置落位——宁可位置略有偏差，也不能让挂件消失
    }

    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            // HWND_TOPMOST：挂件必须稳定压在任务栏（同为 topmost）之上，否则会被
            // 任务栏整体遮挡（IsWindowVisible 仍为 true 但完全不可见）；
            // SWP_SHOWWINDOW 原子显示，show() 仅作 Tauri 侧状态兜底
            let _ = SetWindowPos(
                HWND(hwnd.0),
                Some(HWND_TOPMOST),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
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

/// 虚拟桌面修复：切换桌面后窗口只是被 shell "cloaked" 在旧桌面上（WS_VISIBLE 仍在，
/// 所以 IsWindowVisible 为真、show() 是 no-op）。隐藏再显示会让 shell 把窗口重新
/// 归入当前桌面；用 SW_SHOWNA 避免抢焦点。
#[cfg(windows)]
fn ensure_on_current_desktop(hwnd: HWND) {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let inited = hr == S_OK || hr == S_FALSE; // CHANGED_MODE 时不能配对 CoUninitialize
        if inited || hr == RPC_E_CHANGED_MODE {
            let mgr: windows::core::Result<IVirtualDesktopManager> =
                CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER);
            if let Ok(mgr) = mgr {
                // 对未归位/隐藏窗口会返回 Err——视为"状态未知"，不动
                if let Ok(on_cur) = mgr.IsWindowOnCurrentVirtualDesktop(hwnd) {
                    if !on_cur.as_bool() {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        let _ = ShowWindow(hwnd, SW_SHOWNA);
                    }
                }
            }
            if inited {
                CoUninitialize();
            }
        }
    }
}

/// 打开（或聚焦）设置窗口
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    open_settings_window(&app);
    Ok(())
}

fn open_settings_window(app: &AppHandle) {
    let lang = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|g| g.lang.clone())
        .unwrap_or_else(|_| "zh".into());
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_title(settings_title(&lang));
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    // 兜底：静态声明缺失时动态创建
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title(settings_title(&lang))
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
    if !["zh", "en"].contains(&s.lang.as_str()) {
        s.lang = settings::default_lang();
    }
    // 语言是否变化：决定是否重建托盘菜单/窗口标题
    let lang_changed = state.settings.lock().map(|p| p.lang != s.lang).unwrap_or(false);

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
    // 界面语言切换：托盘菜单与设置窗口标题即时重译（悬浮窗由前端响应 settings 事件重挂载）
    if lang_changed {
        apply_lang(&app, &s.lang);
    }
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

/// 诊断：打印任务栏带、各来源占位矩形（系统子窗口/第三方顶层窗/UIA 元素）与空闲区间，
/// 用于排查挂件与托盘图标重叠（--probe-taskbar）
#[cfg(windows)]
fn taskbar_probe() {
    unsafe {
        let tray = match FindWindowW(w!("Shell_TrayWnd"), None) {
            Ok(h) => h,
            Err(_) => {
                println!("Shell_TrayWnd not found");
                return;
            }
        };
        let mut tr = RECT::default();
        if GetWindowRect(tray, &mut tr).is_err() {
            println!("GetWindowRect failed");
            return;
        }
        println!(
            "band = ({},{})-({},{})  w={} h={}",
            tr.left,
            tr.top,
            tr.right,
            tr.bottom,
            tr.right - tr.left,
            tr.bottom - tr.top
        );
        for cls in ["TrayNotifyWnd", "Start", "MSTaskSwWClass", "ReBarWindow32"] {
            let cls_w = windows::core::HSTRING::from(cls);
            match FindWindowExW(Some(tray), None, &cls_w, None) {
                Ok(h) => {
                    let mut r = RECT::default();
                    if GetWindowRect(h, &mut r).is_ok() {
                        println!("child {cls}: ({},{})-({},{})", r.left, r.top, r.right, r.bottom);
                    }
                }
                Err(_) => println!("child {cls}: <absent>"),
            }
        }

        let mut ctx = EnumCtx { tray, band: tr, pid: std::process::id(), occ: Vec::new() };
        let _ = EnumWindows(Some(enum_taskbar_widget), LPARAM(&mut ctx as *mut EnumCtx as isize));
        println!("-- third-party topmost ({}):", ctx.occ.len());
        for r in &ctx.occ {
            println!("   ({},{})-({},{})", r.left, r.top, r.right, r.bottom);
        }
        let base = ctx.occ.len();
        let _ = EnumChildWindows(Some(tray), Some(enum_tray_child), LPARAM(&mut ctx as *mut EnumCtx as isize));
        println!("-- tray children (+{}):", ctx.occ.len() - base);
        for r in &ctx.occ[base..] {
            println!("   ({},{})-({},{})", r.left, r.top, r.right, r.bottom);
        }
        let base = ctx.occ.len();
        ctx.occ.extend(uia_band_occupancy(tray, tr));
        println!("-- UIA (+{}):", ctx.occ.len() - base);
        dump_uia_names(tray, tr);

        // 生产路径（taskbar_band + plan_mini）的落位与净空
        let Some(info) = taskbar_band() else { return };
        println!(
            "\n== plan (scale={}, uia_count={}, occ={}) ==",
            info.scale,
            info.uia_count,
            info.occ.len()
        );
        for pos in ["right", "center", "left"] {
            // card 皮肤挂件实测约 126x44 逻辑 px（含窗口 8px 透明留白）
            let mw = (126.0 * info.scale).round() as i32;
            let mh = (44.0 * info.scale).round() as i32;
            let (x, y, gaps, pad) = plan_mini(&info, mw, mh, pos);
            let mut clearance = i32::MAX;
            for r in &info.occ {
                let d = if r.right <= x {
                    x - r.right
                } else if r.left >= x + mw {
                    r.left - (x + mw)
                } else {
                    0
                };
                clearance = clearance.min(d);
            }
            println!(
                "pos={pos:<6} pad={pad} -> x={x} span={x}..{} y={y} free={} clearance={clearance}",
                x + mw,
                rect_free(x, mw, &info.occ)
            );
            println!("   gaps: {gaps:?}");
        }
    }
}

/// UIA 元素名 + 矩形逐条打印（探针专用）
#[cfg(windows)]
fn dump_uia_names(tray: HWND, band: RECT) {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let inited = hr == S_OK || hr == S_FALSE;
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
                                let name = el
                                    .CurrentName()
                                    .map(|b| b.to_string())
                                    .unwrap_or_default();
                                // 类名/AutomationId 用于识别匿名元素（排查占位来源）
                                let cls = el
                                    .CurrentClassName()
                                    .map(|b| b.to_string())
                                    .unwrap_or_default();
                                let aid = el
                                    .CurrentAutomationId()
                                    .map(|b| b.to_string())
                                    .unwrap_or_default();
                                let Ok(r) = el.CurrentBoundingRectangle() else { continue };
                                let full = (r.right - r.left) * 10 >= bw * 9;
                                println!(
                                    "   [{}] ({},{})-({},{}) cls=[{}] aid=[{}] \"{}\"",
                                    if full { "container" } else { "elem" },
                                    r.left, r.top, r.right, r.bottom,
                                    cls, aid, name
                                );
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
}
