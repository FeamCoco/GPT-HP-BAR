# GPT-HP-BAR

把 Codex 剩余额度做成一条血条，挂在你的 Windows 桌面和任务栏上。

**简体中文** | [English](README_EN.md)

> 实时显示 Codex（CLI 与 ChatGPT 桌面版共用同一账号池）的 5h 窗口 / 本周窗口 / credits 余量。
> 桌面悬浮窗 + 任务栏挂件双形态，10 套皮肤即点即换，纯本地运行。

<p>
  <img src="docs/images/main.png" width="246" alt="桌面悬浮窗" />
  &nbsp;
  <img src="docs/images/mini.png" width="170" alt="任务栏挂件" />
</p>

## 特性

- **双形态展示** —— 置顶无边框悬浮窗（可拖拽、完整/紧凑两档，切换时保持当前位置并带过渡动画），以及常驻系统任务栏的迷你挂件
- **10 套皮肤即点即换** —— 设置面板内用真实样式实时预览（无需逐个切换试穿），点击卡片立即生效
- **任务栏智能避让** —— Win10 / Win11 通吃：自动避开托盘区、开始按钮与已固定图标群（通过 UI Automation 读取任务栏真实渲染位置，旧版窗口结构在新版 Win11 上拿不到准确坐标）；与 TrafficMonitor 等第三方任务栏挂件共存；图标群随应用启停伸缩时每 2 秒自动跟随；与图标群/托盘保持安全净空并为图标群右侧、托盘左侧预留"生长余量"（新开应用/新通知图标不会压住挂件）；位置冲突时自动滑向最近空闲区，落位前还会对占位做一次交叉校验
- **外观自定义** —— 字号缩放 80–150%、窗口透明度、主题色（跟随额度三档 / 固定绿·琥珀·青）、显示内容四开关（本周 / credits / 倒计时 / 邮箱），改动即时生效
- **托盘集成** —— 托盘图标按剩余额度三档变色（绿 >50% / 黄 >20% / 红 ≤20%），菜单直达显示/设置/刷新/退出
- **开机自启** —— 用户级注册表（HKCU），无需管理员权限

## 截图

设置面板内置全部皮肤的实时预览（上：悬浮窗完整形态，下：青色条 = 任务栏挂件形态）：

![设置面板 · 皮肤预览网格](docs/images/settings.png)

## 数据源与账号

数据获取按以下优先级自动回退：

1. **Codex CLI 自带 app-server**（JSON-RPC：`account/read`、`account/rateLimits/read`）——官方进程，token 续期由 Codex CLI 自己处理
2. **直连** `chatgpt.com/backend-api/wham/usage`（Bearer token 读取自 `~/.codex/auth.json`）
3. **中转 / 第三方 provider**——探测 `config.toml` 中 `model_provider` 指向的 `base_url` 提供的 wham/usage 兼容接口

- 运行过 `codex login` 即可显示真实额度；都没有时显示"无数据源"
- 字段级兼容 `rate_limit` / `rate_limits` 两种拼写及 `additional_rate_limits[]`
- 排查数据源：`gpt-hp-bar.exe --probe`（打印探测结果，不输出任何密钥）

**隐私**：全部逻辑在本机运行，token 只在本机内存中用于查询，无遥测、不上传任何数据。

## 下载与运行

从 [Releases](../../releases) 下载单文件 `gpt-hp-bar.exe`（免安装）。要求 Windows 10/11 + WebView2 Runtime（Win11 自带）。

从源码构建：

```bash
git clone https://github.com/FeamCoco/GPT-HP-BAR.git
cd GPT-HP-BAR/app
cargo build --release
./target/release/gpt-hp-bar.exe          # 悬浮窗 + 托盘
./target/release/gpt-hp-bar.exe --probe          # 数据源诊断
./target/release/gpt-hp-bar.exe --probe-taskbar # 任务栏占位/空闲区间 + 挂件预测落位/净空诊断
./target/release/gpt-hp-bar.exe --probe-vd      # 虚拟桌面归属检测诊断
```

> `--probe-*` 会先声明进程 DPI 感知，因此输出的坐标就是实机物理坐标，可直接和截图对齐核对。

工具链：Rust stable。官方 MSVC 路线装 [VS Build Tools](https://visualstudio.microsoft.com/zh-hans/downloads/) 即可；
没有管理员权限时可用便携版 [MinGW-w64](https://winlibs.com/)——把 `app/.cargo/config.example.toml` 复制为同目录 `config.toml` 并填入你的 gcc 路径。

## 10 套皮肤

| # | 名称 | 风格 |
|---|---|---|
| ① | 血条 Hero | 游戏血条：分段血槽、三档变色、告急闪烁 |
| ② | 琉璃 Glass | 毛玻璃胶囊：极简单行 |
| ③ | 终端 Term | htop 风：等宽字体 + █░ 进度块 |
| ④ | 简报 Card | 信息卡：双窗口 + credits · **默认皮肤** |
| ⑤ | 磁贴 Tile | Fluent 磁贴：大数字 + 白色进度线 |
| ⑥ | 电池 Battery | 电量隐喻：电池图标即额度 |
| ⑦ | 表盘 Gauge | 半圆仪表 + 弹性指针 |
| ⑧ | 霓虹 Neon | 渐变扫光血槽 + 发光大数字 |
| ⑨ | 液体 Liquid | 液面高度即余量，双层波浪 |
| ⑩ | 像素 Pixel | 8-bit：5 心制 + 方块血槽 |

皮肤 = `app/frontend/skins/<id>.css`（样式，作用域 `.sk-<id>`）+ `<id>.js`（注册到 `window.SKINS`，含完整形态与任务栏 mini 形态），新增皮肤只需两个文件。

## 配置

- 设置文件：`%APPDATA%\GPT-HP-BAR\settings.json`（设置面板改动即存盘并实时应用）
- 账号数据：`CODEX_HOME`（默认 `~/.codex`）
- 打开设置：悬浮窗右上角齿轮，或托盘图标右键菜单 →"设置…"

## 目录结构

```
app/
  src/main.rs        # 窗口/托盘/轮询/任务栏定位（Win32 + UIA 避让算法）
  src/datasource.rs  # 数据源多级回退
  src/settings.rs    # 设置持久化与开机自启
  frontend/          # 悬浮窗 / 任务栏挂件 / 设置面板（含 skins/ 10 套皮肤）
docs/                # 需求与技术方案（数据源调研、任务栏避让算法设计）
```

## Roadmap

- [ ] 低额提醒（阈值通知）
- [ ] 用量趋势 sparkline
- [ ] 副屏任务栏挂件
- [x] 多语言界面（v0.4：中英切换，设置面板可选）

## License

[MIT](LICENSE) © 2026 FeamCoco
