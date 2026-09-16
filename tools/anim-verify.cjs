#!/usr/bin/env node
/* ============================================================================
 * 前端动效 / 十套皮肤的自动化验证（v0.6 动效层）
 *
 * 为什么需要它：这套动效只能在 DOM 里跑，但 CI 和开发机常常起不了浏览器 GUI。
 * 这里用 linkedom 搭一个最小 DOM + **手动 rAF/setTimeout 时钟**，直接加载
 * app/frontend 下真实的运行时文件，逐套皮肤断言：
 *   ① 变化确实在"补间"：出现过既不是起点、也不是终点的中间帧；
 *   ② 终值精确：补间跑完后与"直接赋目标值"的状态逐字节一致；
 *   ③ 不抛异常，含两条边界路径（中途换目标 / 无数据往返）；
 *   ④ 无 5h 额度（Pro 5x/20x 只有周额度）：主行换成周剩余、周行不再重复、
 *      从 Plus 形态切过来要收敛到同一状态，挂件同样读周剩余。
 * 另外统计 8 个一次性动效签名各自被触发多少次 —— 这是"每套皮肤都接上了"的证据。
 *
 * 用法：
 *   cd tools && npm install && npm run verify
 *   或：node tools/anim-verify.cjs
 * 退出码：0 = 全通过，1 = 有失败项（可直接挂 CI）
 * ========================================================================== */
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const FE = path.join(__dirname, '..', 'app', 'frontend');
const SKINS = ['hero', 'glass', 'term', 'card', 'tile', 'battery', 'gauge', 'neon', 'liquid', 'pixel'];

let parseHTML;
try {
  ({ parseHTML } = require('linkedom'));
} catch (e) {
  console.error('缺少依赖 linkedom。请先执行：\n  cd tools && npm install');
  process.exit(2);
}

/* ---------- 手动时钟：rAF / setTimeout 只在我们推进时才执行 ---------- */
let now = 0;
let seq = 0;
const rafs = new Map();
const timers = new Map();
const sandbox = { console };
sandbox.requestAnimationFrame = (fn) => { const id = ++seq; rafs.set(id, fn); return id; };
sandbox.cancelAnimationFrame = (id) => rafs.delete(id);
sandbox.setTimeout = (fn, ms) => { const id = ++seq; timers.set(id, { due: now + (ms || 0), fn }); return id; };
sandbox.clearTimeout = (id) => timers.delete(id);
sandbox.performance = { now: () => now };
sandbox.matchMedia = () => ({ matches: false });
sandbox.addEventListener = () => { };        // vm 全局对象上没有，浏览器里有

const { window, document } = parseHTML(
  '<!doctype html><html><head></head><body><div id="root"></div></body></html>');
document.hidden = false;
sandbox.document = document;

const context = vm.createContext(sandbox);
sandbox.window = sandbox;        // window === globalThis，否则皮肤里的裸 HP / SKINS 解析不到

/* linkedom 没有 Web Animations API：打桩并计数（HP.fx 是纯装饰，不参与数值断言） */
(function patchAnimate() {
  const stub = function () { return { cancel() { }, finish() { } }; };
  let p = Object.getPrototypeOf(document.createElement('div'));
  while (p && p !== Object.prototype) {
    try { Object.defineProperty(p, 'animate', { value: stub, writable: true, configurable: true }); } catch (e) { }
    p = Object.getPrototypeOf(p);
  }
})();

/* ---------- 加载真实的运行时前端 ---------- */
const files = ['i18n.js', 'skins/common.js'].concat(SKINS.map((s) => 'skins/' + s + '.js'));
for (const f of files) {
  vm.runInContext(fs.readFileSync(path.join(FE, f), 'utf8'), context, { filename: f });
}

const HP = sandbox.HP;
const SKINSREG = sandbox.SKINS;
HP.setLang('zh');

/* 统计各动效签名的实际调用 —— 顺手证明"每套皮肤都接上了自己的动作" */
const fxUse = Object.create(null);
const rawFx = HP.fx.bind(HP);
HP.fx = (el, name) => { fxUse[name] = (fxUse[name] || 0) + 1; return rawFx(el, name); };

const SETTINGS = { accent: 'auto', lang: 'zh', show_week: true, show_credits: true,
  show_countdown: true, show_email: true };

/* 与 Rust 侧 usage 同构：rem% -> used_percent = 100 - rem */
function payload(rem) {
  if (rem == null) return { ok: false, source: 'none' };
  const u = (r) => ({ used_percent: 100 - r });
  return {
    ok: true, source: 'verify', plan: 'plus', email: 'a@b.c', credits: { unlimited: false, balance: 1.25 },
    primary: Object.assign(u(rem), { window_minutes: 300, resets_in_seconds: 7200 }),
    secondary: Object.assign(u(rem), { window_minutes: 10080, resets_in_seconds: 99999 }),
  };
}

/* 状态指纹：把所有"会被动画改掉"的东西拍成一行，用来判定"变没变 / 到没到位" */
function fingerprint(el) {
  const parts = [];
  for (const n of el.querySelectorAll('*')) {
    const st = n.style || {};
    if (st.width && st.width.indexOf('%') >= 0) parts.push('w' + st.width);
    if (st.height && st.height.indexOf('%') >= 0) parts.push('h' + st.height);
    if (st.transform && st.transform.indexOf('rotate') === 0) parts.push('r' + st.transform.replace(/[^0-9.\-]/g, ''));
    if (n.getAttribute) { const d = n.getAttribute('stroke-dasharray'); if (d) parts.push('d' + d); }
    if (n.children.length === 0) { const t = (n.textContent || '').trim(); if (t) parts.push('t' + t); }
    if (n.classList && (n.classList.contains('on') || n.classList.contains('off'))) parts.push('c' + (n.classList.contains('on') ? 1 : 0));
  }
  return parts.join('|');
}

function mount(id, kind) {
  const stage = document.createElement('div');
  document.body.appendChild(stage);
  const sk = SKINSREG[id];
  stage.innerHTML = sandbox.i18nFill(kind === 'mini' ? sk.mini : sk.html);
  return { sk, kind, el: stage.firstElementChild };
}

function frames(n, ms) {
  for (let i = 0; i < n; i++) {
    now += (ms || 40);
    for (const [id, t] of [...timers]) { if (t.due <= now) { timers.delete(id); try { t.fn(); } catch (e) { } } }
    const due = [...rafs.values()];
    rafs.clear();
    for (const fn of due) fn(now);
  }
}

let pass = 0;
const fails = [];
const check = (cond, label) => { if (cond) pass++; else fails.push(label); };

function run(m, rem) {
  try {
    if (m.kind === 'mini') m.sk.updateMini(m.el, payload(rem), SETTINGS);
    else m.sk.update(m.el, payload(rem), SETTINGS);
  } catch (e) {
    fails.push(m.sk.name + ' 抛异常：' + e.message);
  }
}

/* 每个用例 5 项断言；10 套 × 主形态 + 挂件形态 = 20 个用例 = 100 项 */
function runCase(id, kind) {
  const tag = id + (kind === 'mini' ? ' · 挂件' : '');
  const A = mount(id, kind); run(A, 80);
  const sA = fingerprint(A.el);                       // 起点
  const B = mount(id, kind); run(B, 30);
  const sB = fingerprint(B.el);                       // 终点（直接赋值做基准）
  check(sA !== sB, tag + '：起止状态不同（测试前提）');

  run(A, 30);                                         // 真实路径：同一实例补间过去
  const seen = new Set([sA]);
  for (let i = 0; i < 20; i++) { frames(1); seen.add(fingerprint(A.el)); }

  check(fingerprint(A.el) === sB, tag + '：补间终值与直接赋值一致');
  check([...seen].filter((s) => s !== sA && s !== sB).length > 0, tag + '：出现过中间帧（确实在补间）');

  // 边界一：连续变化 / 中途换目标，必须从当前插值续跑并收敛到新终值
  run(A, 55); frames(3); run(A, 90); frames(30);
  const C = mount(id, kind); run(C, 90);
  check(fingerprint(A.el) === fingerprint(C.el), tag + '：中途换目标后收敛到新终值');

  // 边界二：无数据 ↔ 有数据，不能补间出奇怪的中间态，也不能抛
  run(A, null); run(A, 42);
  const D = mount(id, kind); run(D, 42);
  check(fingerprint(A.el) === fingerprint(D.el), tag + '：从无数据恢复后直接就位');
  return { id, kind, seg: sA.split('|').length };
}

const rows = [];
for (const id of SKINS) {
  if (!SKINSREG[id]) { fails.push('缺少皮肤 ' + id); continue; }
  rows.push(runCase(id, 'main'));
  rows.push(runCase(id, 'mini'));
}

/* ---------- 无 5h 额度：Pro 5x / 20x 只有周额度 ------------------------------
 * 真实形态：Rust 侧 normalize_windows 已经把那个周窗口放进 primary、secondary
 * 置空、has_5h=false（详见 datasource.rs 的 infer_has_5h）。前端要做到：
 *   ① 主行换成周剩余，不再顶着"5h 窗口"的名头显示周数字；
 *   ② 「周」那一行不再重复出现；
 *   ③ 从 Plus 形态（两行都在）切过来时收敛到与"直接挂载 Pro"完全一致的状态 ——
 *      这条最要紧：账号换档时残留一行空数字比一开始就少一行更糟。
 * 行显隐在皮肤里是写 inline style.display，所以这里直读它，不看计算样式。 */
const PRO_PAYLOAD = (rem) => ({
  ok: true, source: 'verify', plan: 'pro', email: 'a@b.c', credits: { unlimited: false, balance: 1.25 },
  has_5h: false,
  primary: { used_percent: 100 - rem, window_minutes: 10080, kind: 'weekly', resets_in_seconds: 99999 },
  secondary: { used_percent: null, window_minutes: null, resets_in_seconds: null, kind: null },
});

// 每套皮肤：主数字 / 周行 / 主行标签 的选择器，以及无 5h 时该看到的标签文案
const NO_5H = {
  hero:    { main: '.n5',    weekRow: '.wk',     label: '.wl',      labelText: '本周' },
  glass:   { main: '.n5',    weekRow: '.week',   label: null,       labelText: null },
  term:    { main: '.n5',    weekRow: '.wk-row', label: '.line .k', labelText: '周' },
  card:    { main: '.n5',    weekRow: '.wk-row', label: '.row .k',  labelText: '本周' },
  tile:    { main: '.nv',    weekRow: null,      label: null,       labelText: null },
  battery: { main: '.num',   weekRow: '.wk',     label: '.wl',      labelText: '本周' },
  gauge:   { main: '.num',   weekRow: '.wk',     label: '.cap',     labelText: 'CODEX 周剩余' },
  neon:    { main: '.n-num', weekRow: '.n-wkw',  label: null,       labelText: null },
  liquid:  { main: '.l-num', weekRow: null,      label: null,       labelText: null },
  pixel:   { main: '.p-hp',  weekRow: '.p-wkw',  label: null,       labelText: null },
};

let cases5h = 0;
function runCaseNo5h(id) {
  const C = NO_5H[id];
  const tag = id + ' · 无5h额度';
  const txt = (el, sel) => { const n = sel && el.querySelector(sel); return n ? (n.textContent || '').trim() : null; };
  const disp = (el, sel) => { const n = sel && el.querySelector(sel); return n ? (n.style.display || '') : '(缺元素)'; };

  const A = mount(id, 'main');
  run(A, 87);                                   // 先渲染成 Plus 形态（5h 主行 + 周行都在）
  frames(2);
  try { A.sk.update(A.el, PRO_PAYLOAD(61), SETTINGS); } catch (e) { fails.push(tag + ' 抛异常：' + e.message); }
  frames(30);
  cases5h++;

  check(String(txt(A.el, C.main)).indexOf('61') >= 0,
    tag + '：主行显示周剩余（' + C.main + ' = ' + txt(A.el, C.main) + '）');
  if (C.weekRow) check(disp(A.el, C.weekRow) === 'none', tag + '：周行不再重复出现（' + C.weekRow + '）');
  if (C.label) check(txt(A.el, C.label) === C.labelText,
    tag + '：主行标签改为「' + C.labelText + '」（实际 ' + txt(A.el, C.label) + '）');

  const B = mount(id, 'main');
  try { B.sk.update(B.el, PRO_PAYLOAD(61), SETTINGS); } catch (e) { fails.push(tag + ' 抛异常：' + e.message); }
  frames(30);
  check(fingerprint(A.el) === fingerprint(B.el), tag + '：从 Plus 切过来与直接挂载结果一致');

  const M = mount(id, 'mini');
  try { M.sk.updateMini(M.el, PRO_PAYLOAD(61), SETTINGS); } catch (e) { fails.push(tag + ' 抛异常：' + e.message); }
  frames(30);
  check(String(txt(M.el, '.m-num')).indexOf('61') >= 0, tag + '：挂件同样显示周剩余');
}
for (const id of SKINS) if (SKINSREG[id]) runCaseNo5h(id);

const sigs = Object.keys(HP.FX);
const usedSigs = Object.keys(fxUse);

const L = [];
L.push('用例：' + rows.length + ' 个（10 套皮肤 × 主形态 + 挂件形态） · 断言：' + (pass + fails.length));
L.push('      + 无 5h 额度（Pro 周额度）' + cases5h + ' 个用例');
L.push('结果：通过 ' + pass + ' · 失败 ' + fails.length);
L.push('');
L.push('皮肤            主形态状态段数   挂件状态段数');
for (const r of rows) {
  if (r.kind !== 'main') continue;
  const mini = rows.find((x) => x.id === r.id && x.kind === 'mini');
  L.push('  ' + r.id.padEnd(12) + String(r.seg).padStart(8) + String(mini ? mini.seg : '-').padStart(16));
}
L.push('');
L.push('动效签名覆盖：' + usedSigs.length + '/' + sigs.length +
  (sigs.filter((s) => !fxUse[s]).length ? '  未触发：' + sigs.filter((s) => !fxUse[s]).join(',') : '  （全部触发）'));
L.push('  触发次数：' + sigs.map((s) => s + '×' + (fxUse[s] || 0)).join('  '));
if (fails.length) { L.push(''); L.push('失败项：'); fails.forEach((f) => L.push('  ✗ ' + f)); }

console.log(L.join('\n'));
process.exit(fails.length ? 1 : 0);
