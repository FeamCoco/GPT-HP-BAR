# 动效层 v0.6 · 交付说明（给开发团队）

> 状态：**代码已在当前工作区，待编译 + 实机验收 + 提交**。
> 交互侧自检：`cd tools && npm install && npm run verify`（下面有预期输出）。

---

## 1. 为什么要做（先看根因，别只看改动）

用户反馈"进度条和数字变了是直接改，没有任何动画"。排查结论：**不是没写 transition**。

皮肤 CSS 里本来就有 `transition:width .5s`，但

- 一次轮询（默认 60s）剩余额度通常只动 **0~1%**，在 120px 宽的条上 **不到 1.5px** —— 过渡根本看不出来；
- 数字是 `textContent` 直写，连这点过渡都没有。

所以**只加长过渡时长/幅度是无效的**：方案里必须包含一层"事件级"反馈，让 1% 这种小变化也能被读到。

顺带一个反直觉的点：进度几何量**不能**靠 CSS 过渡。JS 逐帧写 `width` 时，CSS 过渡会在每帧重新起跑，
变成指数衰减长尾，与数字的补间不同钟（表现为"数字先到位、条还在后面爬"）。本次已把 7 个皮肤
CSS 里的进度几何过渡**移除**，改由 JS 独占。

## 2. 方案：两层

**① 数值补间 `HP.tween`（rAF 驱动，620ms 统一时钟）**
条宽 / 液面高度 / 弧长 / 指针角度 / 数字**共用同一个时钟与曲线**，同起同落。
三条纪律（都已实现，勿改）：

- 首屏不补间 —— 挂载时直接就位，避免开机看到数字从 0 冲到 87%；
- `null`（无数据）不补间 —— 直接落占位态，不让 `--%` 参与插值；
- 中途换目标从**当前插值**续跑 —— 连续轮询/连续点击不打架。

**② 每套皮肤专属的一次性动效 `HP.fx`（8 个签名）**
每次**真实变化**播一次。1% 的小变化能被读到，靠的就是这一层。
`reduce-motion`（系统"减少动态效果"）策略：**信息性补间压缩到 1/3 时长而不是删除**（删了用户就读不到"刚刚变了"），
纯装饰的 `HP.fx` 直接不播。

## 3. 变更清单

### 本次新增

| 文件 | 说明 |
|---|---|
| `tools/anim-verify.cjs` | 动效自动化验证（100 项断言，可挂 CI） |
| `tools/anim-preview/{page.src.html,build.cjs}` | 动效预览页生成器（设计侧目视验收用） |
| `tools/package.json` | 工具依赖（linkedom），与 Tauri 应用无关 |
| `docs/MOTION-HANDOFF.md` | 本文件 |

### 本次修改

| 文件 | 改了什么 |
|---|---|
| `app/frontend/skins/common.js` | +121 行：动效引擎 `HP.tween` / `HP.fx` / 缓动曲线 / 8 个签名 |
| `app/frontend/skins/{hero,glass,term,card,tile,battery,gauge,neon,liquid,pixel}.js` | 十套皮肤的 `update` / `updateMini` 全部改走 `HP.tween`，并各接一个动效签名 |
| `app/frontend/skins/{card,hero,tile,battery,gauge,neon,liquid}.css` | **移除**进度几何量的 `transition`（width / height / stroke-dasharray / transform），交还给 JS |
| `app/frontend/app.css` | §0 token 块加 `font-variant-numeric:tabular-nums`（补间时数字宽度稳定） |
| `app/frontend/mini.css` | `.m-num{min-width:4ch;text-align:right}`（挂件窗口只在挂载时量一次尺寸，固定栏位避免内容被挤出边界） |

> ⚠ **工作区里还有一批不属于本次的未提交改动**：`app/src/main.rs`(+517)、`app/frontend/app.js`(+147)、
> `app/frontend/settings.js`、`app/src/settings.rs`、`app/frontend/skins/{glass,term,pixel}.css`、
> 以及未跟踪的 `docs/NEXT-PLAN-0.5.md`。提交时请按上下文拆分，别和本次动效混在一个 commit 里。

## 4. 动效规格（设计侧参数集中在这里）

| 皮肤 | 进度几何 | 数字 |
|---|---|---|
| ① 血条 Hero | 血槽 easeSoft | **pop** 机械弹跳 |
| ② 琉璃 Glass | 无条，数值靠数字承载 | **slide** 上浮 + 闪电 pop |
| ③ 终端 Term | 10 格 █░ 阶梯逐格跳 | **jitter** 抖动 + 阶梯跳数 |
| ④ 简报 Card | easeOut | **tick** 压一次明度（最克制） |
| ⑤ 磁贴 Tile | easeBack 回弹 | **lift** 抬起 |
| ⑥ 电池 Battery | easeOut | **charge** 亮度脉冲（不做位移） |
| ⑦ 表盘 Gauge | 弧 easeOut + 指针 easeBack 过冲 | **pop** |
| ⑧ 霓虹 Neon | 扫光条 easeOut + 亮度闪 | **jitter** 抖动 |
| ⑨ 液体 Liquid | 液面 easeSoft | **bob** 随液面晃动 |
| ⑩ 像素 Pixel | 心/方块阶梯硬切 | **blip** 闪两下 |

参数：补间 620ms（`HP.MS`）｜曲线 `easeOut` / `easeSoft` / `easeBack` / `easeStep(n)`｜
签名时长 300~620ms（见 `HP.FX`）。

## 5. 接入契约（以后新增皮肤照这个写）

```js
// update(el, u, s)
HP.tween(el, '主值的 key', rem5, (v) => {
  bar.style.width = (v == null ? 0 : v) + '%';            // 几何
  num.textContent = v == null ? '--%' : Math.round(v) + '%'; // 数字
}, { ease: HP.easeSoft, start: () => HP.fx(num, 'pop') });  // start 只在"真的变了"时回调
```

四条硬规则（都有具体理由，别绕开）：

1. **不要在皮肤 CSS 里给进度几何写 `transition`** —— 会和 JS 补间叠成两层缓动（见 §1）。
2. **`HP.fx` 用 WAAPI，不要改成"加类名重播 keyframes"** —— 主数字常同时挂 `.danger-flash`
   （带 `!important` 的 animation），同一个元素只能生效一条 `animation` 声明，走类名会导致
   **"额度告急时恰好没有变化动效"**，而告急最该被看见。WAAPI 只要属性不冲突就能与 CSS animation 共存。
3. **`draw(v)` 拿到的是中间浮点值**，几何量直接用，文本自己 `Math.round`，离散量（心/格/方块）自己量化。
4. **挂件形态（`updateMini`）不要用位移类动效** —— 挂件窗口只有内容 +8px，位移会裁切；
   用 `charge` / `tick` / `jitter` / `blip` 这类不位移的。

## 6. 验收

### 6.1 自动（可直接挂 CI）

```bash
cd tools && npm install && npm run verify
```

预期（实测输出）：

```
用例：20 个（10 套皮肤 × 主形态 + 挂件形态） · 断言：100
结果：通过 100 · 失败 0
动效签名覆盖：8/8  （全部触发）
  触发次数：pop×9  slide×3  jitter×12  lift×3  charge×15  bob×3  blip×9  tick×15
```

断言内容：每套皮肤的主/挂件形态都验证了「出现过中间帧（确实在补间）」「终值精确（与直接赋值一致）」
「中途换目标后收敛」「无数据往返后就位」「起止状态不同」。退出码 1 = 有失败项。

### 6.2 目视（设计侧对照用）

```bash
cd tools && npm run preview     # 生成 design/anim-preview.html
```

自包含单文件，双击即可用浏览器打开（`design/` 按仓库约定不入库）。十套并排 + 滑杆 / ±1% / 大跳变 /
告警 / 无数据 / 自动轮播；顶部有自检条（引擎是否就绪 / 挂载几套 / 渲染次数 / 最近一次变化 / 捕获到的错误），
"没动效"时它会直接说出原因。

### 6.3 必须实机确认的三项（自动化覆盖不到）

1. **补间流畅度** —— 620ms / 60fps 观感，以及任务栏挂件上是否过动；
2. **挂件宽度不跳** —— 数字在 `100% ↔ 8% ↔ --%` 之间变化时，挂件窗口不应左右晃；
3. **Windows「减少动态效果」开启时的降级** —— 应保留短暂补间、不播装饰动效。

### 6.4 构建

```bash
cd app && export CARGO_TARGET_DIR=E:/project/GPT-HP-BAR/tmp/tgt \
        && export PATH="/c/Users/pp/w64dev/mingw64/bin:$PATH" \
        && cargo build --release
```

⚠ **`CARGO_TARGET_DIR` 必须显式指定**：真实增量缓存目录是 `tmp/tgt`，它不在 Cargo.toml、
`.cargo/config.toml` 或环境变量里。不指定就会往 `app/target` 全量重编所有依赖（两边的指纹不通用）。

## 7. 已知阻塞 / 未做

- **本机（WorkBuddy 会话内）跑不了 cargo 构建**：沙箱下报
  `error writing dependencies to .../deps/*.d: 拒绝访问 (os error 5)`，`dangerouslyDisableSandbox`
  也一样；免沙箱的长任务会被中途终止。目录本身可写（PowerShell 写同一个目录正常），所以不是权限问题。
  **这一步需要人工在终端跑**（命令见 §6.4）。
- **版本号未改**。要发版才动，共 4 处：`app/Cargo.toml`、`app/tauri.conf.json`、
  `Cargo.lock` 里 `name = "gpt-hp-bar"`、`frontend/settings.html` 的 `<span class="ver">` 兜底值。
- 工作区混有非本次的改动（见 §3 的警示）。

## 8. 建议的提交拆分

```
feat: 进度条与数字的变化动效（十套皮肤各一套语言）
  1. 根因：CSS 过渡对 1% 的变化不可见（120px 条上 <1.5px），数字完全没有过渡。
  2. 两层动效：HP.tween 统一 620ms 时钟驱动几何+数字；HP.fx 每套皮肤一个一次性签名。
  3. 移除 7 个皮肤 CSS 的进度几何 transition（与 JS 补间叠成两层缓动）。
  4. 验证：tools/anim-verify.cjs 100 项断言全通过，8/8 动效签名均触发。

chore: 版本号 0.4.7      # 仅发版时
```

## 9. 待决 / 可选后话

- **参数集中**：620ms、四条曲线、8 个签名的时长目前散在 `common.js` 的 `HP.MS` / `HP.FX` 里，
  皮肤 CSS 里还有各自的紧凑形态过渡。若以后要"一键调快/调慢动效"，建议抽成一组 token。
- **`HP.fx` 的签名表是 JS 对象而不是 CSS `@keyframes`**：为了让每套皮肤的动效语言集中在一处可读、
  且不受级联影响（见 §5 规则 2）。代价是设计师改不了 CSS 就得改 JS —— 如果设计侧要自己调，可以在设置面板里加一个"动效强度"档位。
- **挂件数字栏位 `min-width:4ch`**：为了保证宽度不跳，代价是 2 位数字左侧会留约 6px 空档。
  如果视觉上不能接受，替代方案是让挂件窗口的尺寸在数值稳定后重新量一次（会牺牲一点稳定性）。
- 单元测试没有覆盖**真实 WebView2 的 WAAPI 行为**（`el.animate` 在验证脚本里是打桩的）。
  如果后续要重做签名表，建议在真机上过一遍 §6.3。
