# GPT-HP-BAR 下一步改进规划（v0.5 候选）

> 基线：v0.4.6（`f22a8d2` 皮肤 v2 落地 + `2bb3d22` 版本号）
> 本机实测环境：2560×1440 / 100% DPI / Win11 任务栏 0,1392–2560,1440
> 本文只做规划，尚未改动任何代码。

---

## 0. 结论速览

| 编号 | 事项 | 类型 | 用户可感知 | 依赖 |
|---|---|---|---|---|
| **P0-A** | 收起成小条：给 9 套皮肤补「紧凑形态」 | 功能缺口 | 高（当前诉求） | 无 |
| **P0-B** | 点任务栏/程序图标后挂件被遮 2s | 真 bug | 高（当前诉求） | 无 |
| **P0-C** | 主题色白名单过期，4 种颜色选完被静默改回 | 真 bug | 中 | 无 |
| **P0-D** | 挂件保活被数据轮询阻塞（最长 16s+） | 真 bug | 高（放大 P0-B） | 与 P0-B 同一处改动 |
| **P1** | cloaked 统一判定 / 全屏与自动隐藏任务栏 | 健壮性 | 中 | P0-B |
| **P1** | 前端心跳 + WebView2 卡死自愈 | 健壮性 | 高（另一条"消失"路径） | 无 |
| **P1** | 单实例守卫 / 托盘菜单动态态 / 设置校验补全 | 健壮性 | 中 | 无 |
| **P2** | 低额提醒、用量 sparkline、副屏挂件（README Roadmap） | 新功能 | 中 | sparkline 需先做历史存储 |
| **P2** | 数据源未登录引导 / 自动更新 / 竖排任务栏 | 体验 | 中 | 无 |

---

## 1. P0-A｜「无法隐藏悬浮窗」= 只有 card 能收起

### 现状（已核实）

- 悬浮窗自身**没有隐藏按钮**，隐藏的唯一入口是托盘右键菜单「显示 / 隐藏悬浮窗」（`app/src/main.rs:177-187`）。
- 悬浮窗上的 chrome 只有两个按钮：齿轮（设置）+ 折叠箭头（收起）。而折叠箭头**只有 card 皮肤会渲染**：

  `app/frontend/app.js:42`
  ```js
  chrome.innerHTML = `<button id="gear" ...>` +
    (sk.sizes.compact ? `<button id="tgl" ...>` : '');
  ```

- 10 套皮肤的尺寸声明里，**只有 card 有 compact**：

  | 皮肤 | sizes |
  |---|---|
  | card | `full: [280,212]`、`compact: [248,104]` |
  | 其余 9 套（battery/gauge/glass/hero/liquid/neon/pixel/term/tile） | `compact: null` |

  → 换到非 card 皮肤后，箭头消失，窗口永远展开占屏。这就是「无法隐藏」。

- card 的收起实现可作为模板：`app/frontend/skins/card.css:45-53`（`.extra{max-height:0}` 收缩 + 收窄 padding + 隐藏邮箱），配合 `app.css:40` 的箭头翻转、`app.js:69` 的尺寸切换、`app.js:50-54` 的点击逻辑。

### 两个可选方案

**方案 A —— 每套皮肤做各自的精简形态（体验最好，工作量最大）**
- 需要为 9 套皮肤各定一个紧凑尺寸 + 一段 `body.compact .sk-x{...}` 规则。
- 要点（沿用 card 的经验）：
  - 只保留「身份标题 + 5h 主进度」两行，week/credits/邮箱/副标题全部收起；
  - **不要用 `display:none`**，用 `max-height`/`opacity` 过渡，否则高度突变会让窗口补间跳一下（card 已踩过）；
  - 皮肤若在根元素上写了 `width/transition`（card 就是这样），往末尾追加规则时要**把原属性一起带上**，否则会覆盖掉过渡（09-11 笔记里记过这个坑）；
  - 竖长皮肤（gauge 228×220、liquid 240×220）收成横条时布局差异大，需要单独设计，不能机械缩宽。

**方案 B —— 收起后统一渲染该皮肤的 `mini` 形态（推荐先做）**
- 每套皮肤**已经有一个 `mini` 模板 + `updateMini()`**（本来就是给任务栏挂件用的，见 `app/frontend/skins/*.js` 与 `mini.js`）。
- 收起时把 `root.innerHTML = HP.i(cur.mini)` 而不是 `HP.i(sk.html)`，配合一个略大于挂件的尺寸即可。
- 优点：**一次改动 10 套皮肤全部拥有收起形态**，且「收起态」与「任务栏挂件」视觉一致，天然符合产品语义；不需要逐皮肤调 CSS。
- 代价：收起后丢掉该皮肤的辨识度（都是小条），且需要新增"compact 态 + 挂件态"两套渲染分支与状态管理。
- 落地建议：`sk.sizes.compact` 从"是否渲染箭头"的开关，改为「compact 尺寸」；箭头渲染条件改为「compact 存在 **或** 该皮肤有 mini 模板」，这样方案 B 能立刻覆盖 10/10。

### 顺带一起做的（同属"别让悬浮窗碍事"）

1. **真正的隐藏按钮**：chrome 增加一个「—」按钮 → `invoke('hide_main')` → `win.hide()`。与"收起"是两个不同诉求（收起=变小条常驻，隐藏=完全消失），建议同时提供。
2. **全局热键**：`Ctrl+Alt+H` 切换显示/隐藏（`tauri-plugin-global-shortcut`）。隐藏后不依赖托盘也能叫回来。
3. **记住隐藏状态**：`settings.json` 增加 `main_hidden`，重启后保持一致（适合"只想要任务栏挂件"的用户）。
4. **托盘菜单文案跟随状态**：当前固定写「显示 / 隐藏悬浮窗」（`main.rs:71`），改成按 `is_visible()` 动态显示「显示悬浮窗」/「隐藏悬浮窗」，并加勾选态。
5. **全部皮肤都要有箭头**：ARIA/title 走 `HP.t('ttl_toggle')`（i18n 键已存在）。

---

## 2. P0-B｜点任务栏/程序图标后挂件被遮 2s

### 已确认现象

> 用户描述：点击任务栏或者其他程序 logo 会消失 2s 又回来。

**这就是 z-order 被抢 + 重试周期 2s 的典型征象，不是落位算错。**

### 根因链

1. 挂件不是任务栏子窗口，而是**同层 topmost 的浮动窗**（`main.rs:880-888` 用 `SetWindowPos(..., HWND_TOPMOST, ..., SWP_SHOWWINDOW)`）。这是刻意设计（代码注释写明"v1 简化版，视觉等同嵌入"）。
2. 任务栏 `Shell_TrayWnd` **同样 topmost**。点击任务栏/程序图标时 shell 会把任务栏提到 topmost 组最前 → 挂件被整块盖住（挂件完全落在 band 内，所以是"整条看不见"）。
3. 唯一把挂件拉回来的机制是轮询重新置顶，而周期就是那个 2s：

   `main.rs:246-256`
   ```rust
   for i in 0..secs {
       thread::sleep(Duration::from_secs(1));
       // 每 2s 重定位一次（占位没变时 SetWindowPos 幂等）
       if i % 2 == 1 { position_mini(&handle, &st); }
   }
   ```
4. 而 `position_mini` 里 `SetWindowPos(HWND_TOPMOST)` 会插到 topmost 组最前 → 2s 后恢复。**2s 延迟 = 实测现象，完全吻合。**

### 修复方案

**B-1（主方案）拆出独立的「置顶保活」线程**

- 新增一条与数据轮询无关的线程，周期 **200~300ms**：
  1. 读挂件 HWND 与 `GetWindowRect`；
  2. `WindowFromPoint(挂件中心)` 判定是否被遮挡（返回的不是挂件 hwnd → 被盖）；
  3. 仅在被遮挡时 `SetWindowPos(HWND_TOPMOST, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`。
- 效果：感知延迟从 2000ms 降到 ≤300ms，基本看不出闪。
- 只在被遮挡时才调用，空闲时零开销（不重建窗口、不重绘）。
- 注意：`WindowFromPoint` 会跳过 `WS_EX_TRANSPARENT` 窗口，任务栏/普通应用都不带该样式，可用。若担心误判，可退化为"每 300ms 无条件重置顶"（幂等、开销极小），只是会一直压在别人的 topmost 窗口之上。

**B-2（可选，根治级）把挂件变成任务栏子窗口**

- `SetParent(挂件HWND, Shell_TrayWnd)`：子窗口随父窗口的 z-order 走，任务栏被激活时挂件跟着上来，**从根上不存在"被任务栏遮住"**。
- 本机证据：`--probe-taskbar` 显示任务栏带内确实存在第三方窗口（`#32770`，2055..2272，TrafficMonitor），说明这条路径在 Win11 上是活的。
- 风险：Win11 XAML 任务栏对 `SetParent` 的兼容性未知（可能被重建、可能失去透明、可能被 XAML 层盖住）；子窗口会随任务栏自动隐藏一起消失（这其实是我们想要的行为，见 P1）；需要实机验证，作为 B-1 的后续增强而非首选项。

**B-3 与 P0-D 合并**：见下节。

---

## 3. P0-D｜挂件保活被数据轮询阻塞（放大 P0-B）

### 现状

`main.rs:212-257` 的轮询线程按「落位 → 取数 → 落位 → 每秒 tick 里每 2s 落位」串行执行。而 `datasource::fetch_all()` 是**同步阻塞**的，最坏路径很长：

- `rest` 超时 8s（`datasource.rs:133`）
- → 失败后 `app-server` 再等 8s（`datasource.rs:257` 的 deadline）
- → 再走 relay

`fetch_all()` 三段是顺序回退（`datasource.rs:309-353`），**单次最坏可达 16s+**。这段时间里：
- `position_mini` 完全不被调用 → 被遮住就一直被遮；
- 用户感知就是"消失很久才回来"。

### 修复

1. **把「落位/保活」和「数据轮询」彻底解耦成两条线程**（B-1 的保活线程天然承担这件事）。
2. `fetch_all()` 里三段回退加**总预算**（例如整体 3s），或改成并行发起，避免单源超时拖死整轮。
3. 给 `datasource` 加一层"上次成功结果"缓存：重启应用后先渲染缓存，别让挂件空着等十几秒。

### 附带发现的同类问题

- `main.rs:218` 在 `fetch_all()` **之前**就调用 `position_mini`，注释说"此时前端多半已调用 set_window_size，尺寸已就绪"——但首次启动时前端未必就绪，会先用估算尺寸落位一次再纠正（有 `mini_size` 兜底逻辑，`main.rs:844-857`）。解耦后这条顺序依赖消失。

---

## 4. P0-C｜主题色白名单过期（已确认的真 bug）

### 问题

`app/src/main.rs:973`：
```rust
if !["auto", "green", "amber", "cyan"].contains(&s.accent.as_str()) {
    s.accent = "auto".into();   // 静默改回"跟随三档"
}
```

而皮肤 v2 的主题色已经是 6 色：

`app/frontend/skins/common.js:28-36`
```js
ACCENTS: { mint, cyan, sky, violet, amber, rose, green(=mint 别名) }
```

设置面板下拉也给的是 7 项（`settings.html:45-53`：auto/mint/cyan/sky/violet/amber/rose）。

**后果**：用户选「固定薄荷 / 固定天蓝 / 固定靛紫 / 固定玫红」→ 保存 → 被静默改回 `auto`，界面无任何提示。
更讽刺的是 `settings.js:38` 做了 `green -> mint` 的旧值迁移，迁移完 `mint` 立刻又被 `save_settings` 打回 `auto`，**迁移逻辑自己把自己废掉**。

### 修复

1. 白名单改成"以 `HP.ACCENTS` 的键集合为单一真源"。Rust 侧没有前端常量，最小改法是 Rust 里放一份与 `common.js` 对齐的 `const ACCENTS: &[&str] = &["auto","mint","cyan","sky","violet","amber","rose","green"];`，并在注释里写明"改 `common.js` 的 ACCENTS 时必须同步这里"。
2. 更彻底的防呆：**由前端下发已校验的 accent 集合**，或 Rust 侧只做「非空字符串」校验、把合法性判断交给前端（前端本来就有兜底 `HP.ACCENTS[accent]` 查表）。
3. 顺手修过期注释：`app/src/settings.rs:12` 仍写 `accent: auto | green | amber | cyan`。
4. `save_settings` 目前**未校验** `skin` 与 `mini_pos`（非法值会落盘，靠前端各自兜底）。一并补上。
5. 版本号目前要人肉同步 4 处（`Cargo.toml` / `tauri.conf.json` / `Cargo.lock` / `settings.html` 的 `.ver` 兜底）。建议加一个 `get_version` 命令返回 `env!("CARGO_PKG_VERSION")`，`settings.html` 只读运行时版本、去掉兜底硬编码。

---

## 5. P1｜健壮性（另一条"消失"路径 + 其它）

### 5.1 cloaked 统一判定，覆盖全屏与自动隐藏任务栏

`ensure_on_current_desktop`（`main.rs:904-926`）只处理**虚拟桌面**导致的 cloaked，用 `IsWindowOnCurrentVirtualDesktop` 判断。但 Windows 在**全屏应用**运行时也会 cloak 顶层窗口——这条没覆盖，表现同样是"`IsWindowVisible` 为真、`show()` 无效、就是看不见"。

- 统一改用 **`DwmGetWindowAttribute(DWMWA_CLOAKED)`**：它同时覆盖"虚拟桌面 cloaked"和"全屏 cloaked"，是判断"名义可见但实际不可见"的权威来源。
- **反向 bug**：本机 probe 显示 band 恒为 `0,1392–2560,1440`，即使任务栏被全屏应用隐藏，`GetWindowRect(Shell_TrayWnd)` 依然返回"桌面上那一条"。于是全屏时挂件会**浮在全屏应用上**。需要加规则：
  - `IsWindowVisible(Shell_TrayWnd)` 为 false，或 band 与当前显示器工作区不重叠 → **主动隐藏挂件**，任务栏回来后再显示。
- 任务栏自动隐藏时同理（band 贴边收成一条），应跟随隐藏或退让。

### 5.2 前端心跳 + WebView2 卡死自愈

挂件是 `transparent: true` 的窗口。若 WebView2 渲染进程被终止（内存压力/崩溃），窗口会变成**全透明**——看起来就是"血条消失"，而 2s 重置顶完全救不回来，只能重启应用。

- 方案：前端在每次 `render`/`updateMini` 后 `invoke('alive_ping', { label })`；Rust 保活线程记录各窗口最后 ack 时间戳，超过阈值（如 3 个轮询周期或 30s）→ `win.eval("location.reload()")`，必要时重建 webview。
- 好处：同时能覆盖"数据长期不刷新"和"渲染进程假死"两类问题。
- 诊断价值：ack 时间戳一并写进 `mini-diag.log`。

### 5.3 单实例守卫

目前没有 `tauri-plugin-single-instance`（`app/Cargo.toml` 依赖里无此插件）。双击两次会起**两个实例**：两个托盘图标、两组窗口互相抢任务栏位置（`enum_taskbar_widget` 只跳过自己 pid，另一个实例会被当成"第三方占位"），现场会非常混乱。

### 5.4 数据源「未登录」引导

本机实测 `ok=false`（无 OAuth 登录态、relay 404、app-server 无结果），皮肤只显示"无数据源"、倒计时隐藏。建议：
- 区分"未登录 Codex / 未配置中转 / 网络失败"三种状态，给出各自的下一步操作提示；
- 设置面板提供一键重试 + 「打开 Codex 登录说明」链接；
- 顺便把 `read_auth`/`read_config` 的路径与判定结果暴露到探针输出里（`--probe` 已有基础）。

### 5.5 托盘菜单

- 「显示/隐藏悬浮窗」文案与勾选态跟随实际可见性；
- 增加「任务栏挂件」开关（等价于设置里的 `mini_enabled`，但不用开设置窗）；
- 增加「刷新」的视觉反馈（当前点完毫无动静）。

---

## 6. P2｜功能（README Roadmap + 交互增强）

按 README Roadmap 现有条目，加上本轮暴露出来的诉求：

| 功能 | 说明 | 前置 |
|---|---|---|
| **低额提醒** | 剩余 ≤20% / ≤10% 时托盘气泡通知（可选声音）；只在跨阈值时提醒一次，避免每轮都弹 | 无 |
| **用量趋势 sparkline** | 目前**没有任何历史存储**，需要先加本地历史（滚动窗口 + 落盘节流 + 上限裁剪），再谈画图 | 历史存储 |
| **副屏任务栏挂件** | 现在只找 `Shell_TrayWnd`，未处理 `Shell_SecondaryTrayWnd`；副屏挂件需要按显示器分别落位 | 多显示器改造 |
| **自动更新** | `tauri-plugin-updater` + 关于页；发版流程已跑通（release.yml → zip） | 签名/更新端点 |
| **点击穿透** | `WS_EX_TRANSPARENT` + 按键修饰（按住 Alt 才响应鼠标），悬浮窗彻底不挡操作 | 无 |
| **自绘右键菜单** | 现在悬浮窗/挂件全局屏蔽 `contextmenu`（`app.js:9-10`），可改成自绘菜单提供 刷新/收起/隐藏/设置/退出 | 无 |
| **竖排任务栏** | 算法假定横向带、y 取带内居中；Win10 可设左右任务栏 | 无 |

---

## 7. 验证计划

沙箱限制（既有结论）：**应用前端 JS 不执行**（窗口恒为 `tauri.conf.json` 初始尺寸）、GUI 进程无法跨工具调用存活 → **前端与落位改动无法 live 验证**，只能靠探针 + 实机截图交叉核对，最终由用户实机确认。

因此本轮要把可验证的部分做实：

1. **纯函数单测**：`plan_mini` / `rect_free` / `merge_x_intervals` 与新增的"遮挡判定"逻辑全部是纯函数，补 `#[cfg(test)]`，本机可直接 `cargo test`（不需要 GUI）。
2. **`--probe-taskbar` 回归**：本机当前基线已记录，可直接对比：
   ```
   band = (0,1392)-(2560,1440)
   effective occ: 1015..1545（图标群） / 2055..2560（TrafficMonitor 2055..2272 + 托盘 2270..2560）
   pos=right  -> x=1883 span=1883..2009 free=true clearance=46
   pos=center -> x=875  span=875..1001  free=true clearance=14
   pos=left   -> x=14   span=14..140    free=true clearance=875
   uia_count=25, scale=1, pad=14
   ```
   落位逻辑改动后这三个数字必须保持或更优。
3. **新增 `--probe-mini`**：给定挂件 hwnd，打印 `IsWindowVisible` / `DWMWA_CLOAKED` / `GetWindowLongPtr(GWL_EXSTYLE)` 是否含 `WS_EX_TOPMOST` / `WindowFromPoint(中心)` 返回值 / 最后 ack 时间戳。异地机器上看不到画面时，这个命令 + `mini-diag.log` 是唯一的一手证据。
4. **`mini-diag.log` 扩展**：现有字段（pos/t/x/mw/pad/scale/uia_count/occ/band/gaps）基础上加 `cloaked`、`topmost`、`occluded_by`、`ack_age`，让"消失"能事后归因到具体分支。
5. **实机验收清单**（交给用户，逐条打勾）：
   - 10 套皮肤都能收起成小条（并确认收起时窗口尺寸与内容不裁剪）；
   - 反复点击任务栏 / 点击程序图标 / 打开开始菜单，挂件不出现可见的消失；
   - 切换虚拟桌面、开全屏视频、切回桌面：挂件状态合理（该藏则藏、该回则回）；
   - 主题色 6 色逐一选择 → 保存 → 重开设置，值不被改回 `auto`。

---

## 8. 建议的落地节奏

- **第 1 批（v0.4.7，纯修复，风险最低）**：P0-C（主题色白名单 + 设置校验 + 注释/版本号卫生）、P0-D 的解耦（保活线程独立 + `fetch_all` 预算）。这两条改动小、可单测、直接消除用户可见的静默错误。
- **第 2 批（v0.5.0，体验）**：P0-B（置顶保活 + 遮挡检测，含 B-2 的 `SetParent` 实验开关）、P0-A（收起形态，推荐先走方案 B 覆盖 10/10，再按皮肤打磨方案 A）、P1.5.1（cloaked 统一判定 + 全屏/自动隐藏任务栏规则）。
- **第 3 批（v0.5.x）**：P1.5.2 心跳自愈、P1.5.3 单实例、P1.5.4 数据源引导、P1.5.5 托盘菜单态。
- **第 4 批（v0.6）**：P2 功能，按「低额提醒 → 历史存储 + sparkline → 副屏挂件 → 自动更新」的顺序，sparkline 必须等历史存储先落地。

---

## 9. 需要你拍板的两个设计选择

1. **收起形态走哪条路？** 方案 B（统一复用各皮肤 `mini` 形态，改动小、立刻 10/10，但收起后皮肤辨识度丢失）还是方案 A（每套皮肤各自精简，体验最好，9 套皮肤逐一设计）？建议 **B 先上、A 逐步替换**。
2. **挂件被遮走哪条路？** B-1（独立保活线程 + 遮挡检测，改动小、可回退）还是直接试 B-2（`SetParent` 到任务栏，根治但 Win11 兼容性需实机验证）？建议 **B-1 先上，B-2 作为实验开关**。

---

## 10. 落地记录（2026-09-12，第一批已实施）

**用户拍板：① 收起形态按逐皮肤做（方案 A）；② 挂件直接上 SetParent（B-2）。** 以下改动已完成并通过 `cargo check`（尚未发版）：

### P0-C accent 白名单（已修）
- `main.rs`：白名单改为 `ACCENT_VALUES = [auto, mint, cyan, sky, violet, amber, rose, green]`，与 `common.js HP.ACCENTS` 对齐；新增 `SKIN_IDS` / `MINI_POS_VALUES` 校验；`settings.rs` 注释同步。

### P0-B/D 挂件嵌入 + 保活（已实施）
- 新设置 `mini_embed`（默认开，设置面板"任务栏"节可选）。
- `embed_mini()`：WS_CHILD + `SetParent(Shell_TrayWnd)`；`unembed_mini()` 反向恢复。**注意 `SetParent` 旧父为 NULL 时 windows crate 误报 Err，一律用 `GetParent` 复核。**
- `position_mini` 分两路：嵌入模式坐标转任务栏客户区、不调 tao 的 `show()`（它会按 WS_POPUP 重写样式把嵌入拆掉）；浮动模式先摘出嵌入态再置顶。
- **独立保活线程（250ms，与数据轮询解耦）**：嵌入模式校验父窗口（explorer 重启自动重挂）→ 子窗口置顶（压回任务栏内部 XAML 宿主）→ 可见性 → 位置漂移；浮动模式 `WindowFromPoint` + `GA_ROOT` 命中测试，被遮挡才重置顶（WebView2 内层子 HWND 必须取根再比对）。
- 自愈：挂件 HWND 失效时 `try_rebuild_mini()` 重建同名窗口（10s 节流）；状态变化写 `mini-diag.log` 的 `watch` 行。
- `ensure_on_current_desktop` 跳过已嵌入的挂件（跟随任务栏桌面）。
- **已知风险（实机要验）**：explorer 重建任务栏时嵌入子窗口可能被连带销毁（Win32 跨进程父子销毁语义）——有重建自愈 + `mini_embed=false` 双保险；若实机复现，备选方案是保留 WS_POPUP 只 SetParent 不换样式。

### P0-A 逐皮肤紧凑形态（已实施）
- 9 套皮肤 `sizes.compact` + 各自 CSS（`display:none!important` 压过 JS 写入的 inline style）。card 原有不动。
- **v2 起每套皮肤都渲染 2 个 chrome 按钮（46px）**：neon `n-top` 让位 26→51px、pixel `p-title` 26→52px（完整/紧凑同值）。
- 紧凑尺寸（含 chrome 行高 20px 的修正；窗口=内容+16，与 card 先例一致）：
  hero [320,112] / glass [206,60] / term [340,94] / tile [128,120] / battery [244,92] / gauge [228,170] / neon [320,92] / pixel [316,98] / liquid [240,128]。
- **验证限制**：沙箱连无头 Chrome/Edge 都无法启动（`--dump-dom` 零输出），尺寸只能按 CSS 算术推定；`tmp/harness/skin-probe.html` 验证台已备好，实机上打开即可逐皮肤核对（对比 `content=..x.. need=..x.. FIT/OVERFLOW`）。**需用户实机验收：10 套皮肤收起无裁剪 + 点任务栏挂件不再消失 + explorer 重启后挂件自愈。**

### 17:45 挂起事故与线程纪律修复（同日追加）

**现象**：用户点悬浮窗"收起"，应用挂起（事件查看器 **Event 1002 Application Hang @ 17:47:07**）。`mini-diag.log` 显示 17:45:14 挂件以 34px（皮肤 CSS 未加载的裸宽度）定位后，样式化的二次测量再未出现 → **主线程在挂件挂载后约 150ms 内已挂死**，"点收起没反应"是结果不是原因。

**根因**：第一版把 `SetParent/SetWindowPos/ShowWindow` 放在了轮询线程和保活线程里直接调。这些 Win32 调用会**同步 SendMessage 到窗口属主线程**（挂件属主是主线程），而启动期主线程正在初始化 WebView2 不泵消息 → 后台线程卡死在 SendMessage，跨进程 SetParent 挂接输入队列后把主线程一起拖死。

**修复（线程纪律）**：一切窗口样式/父子/位置/显隐变更只在主线程执行：
- `position_mini` = 计算（任意线程）+ `apply_mini_layout`（仅主线程，`run_on_main_thread` 派发）；
- 保活线程退化为 250ms 心跳，检查与变更全部在主线程 `mini_keepalive_tick` 内完成；
- `set_window_size` 挂件分支的 `set_size`、轮询线程的 `ensure_on_current_desktop` 一并派发；
- 后台线程只保留内核侧无副作用读取（GetParent/GetTopWindow/GetWindowRect 等）。
沙箱无法复现（GUI 内前端 JS 不执行），已通过 `cargo check` + release 构建，**待实机回归**：启动即点收起 / 点任务栏 / explorer 重启三条路径。

---

## 11. 回退：跨进程嵌入被移除（2026-09-12 第二轮）

**结论：B-2（`SetParent` 到任务栏）这条路走不通，已整条删除，不要再试。**

- 第一节的"线程纪律修复"（把窗口变更挪到主线程）**没有解决问题**，只是把"后台线程卡死"
  换成了"主线程卡死"：跨进程 `SetParent` 会让挂件成为 explorer 的子窗口，之后
  `SetParent/ShowWindow/SetWindowPos` 都要同步 SendMessage 到 explorer 的属主线程，
  子窗口的输入队列也挂到 explorer 上 —— explorer 一忙，本进程主线程整个卡住
  （Event 1002 Application Hang）。用户表现就是"点收起没反应、整个应用挂住"。
- 一手证据：用户机器 `%APPDATA%/GPT-HP-BAR/mini-diag.log` 里 17:45:14、18:37:49 各只有**一行落位记录**，
  之后 250ms 一次的 `watch` 行一行都没有 → 主线程在挂件挂载后立刻死掉。
  且用户 settings.json 没有 `mini_embed` 字段，`#[serde(default)]` 默认取 `true` → 新装/老配置都会走嵌入。
- **现状**：`mini_embed` 设置、设置面板复选框、i18n 文案、`embed_mini/unembed_mini`、`LAST_MINI_POS`
  全部删除；挂件回到浮动 topmost。**P0-B 的"被任务栏盖住 2s"由保活 tick 解决**：
  `WindowFromPoint(挂件中心)` + `GetAncestor(GA_ROOT)` 命中测试，**只在真被遮挡时**重置顶，
  600ms 限频（原来是无条件每 250ms 置顶，等于持续抢 z-order）。感知延迟 2s → ≤600ms。
- `main.rs` 与项目记忆里都留了"禁止复活"的说明，避免下一轮又走回头路。

### 同期修复：收起动画的位置抖动（三处时钟不一致）

- 窗口补间（Rust 逐帧 `set_size`）与内容折叠（皮肤 CSS）是两个独立时钟；`body` 又是 flex 居中，
  内容一旦提前到位，就会被还在变形的窗口推着上下漂移 —— 就是"内容跳一下再慢慢回位"。
  用户当前皮肤 pixel 实测约 26px 的瞬时跳变（compact 用 `display:none` 硬切，高度瞬间到位）。
- 三个时钟现在统一为 **240ms + easeOutCubic**：
  1. `main.rs` `RESIZE_MS = 240`，`Instant` 计时（不再"固定 sleep 14ms × 16 帧"被拉伸），
     12ms 调度粒度，收缩向上取整 / 放大向下取整（窗口矩形全程不小于内容），同帧内改尺寸+位置；
  2. `app.js` `RESIZE_MS` / `EASE_OUT_CUBIC` / `ANIM_TRANS`（注释里写明必须与 Rust 一致）；
  3. 皮肤 CSS 的几何过渡：`240ms cubic-bezier(.215,.61,.355,1)`。
- `app.js` 新增 `toggleCompact()`：把"`display` 切换"改写成按**实测尺寸**的
  max-height/max-width/margin/padding/border 过渡（inline `display` 带 `!important`，
  否则压不过皮肤紧凑规则里的 `display:none!important`），结束后撤掉 inline 交还 CSS；
  异常自动退化为原来的直接切换，功能不受影响。
- 遗留：glass/liquid 等声明的窗口高度与"内容 + 16"约定不齐 → 收起时卡片有 ≤10px 的**平滑**位移
  （不是跳变）。要彻底消除需要改成"窗口尺寸由前端实测内容算出"，属于后续重构。

