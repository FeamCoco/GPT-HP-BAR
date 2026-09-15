// 外壳：皮肤加载 / 数据分发 / 尺寸同步 / 拖拽 / 设置应用
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);

// 心跳：渲染进程每次 render 后 + 每 5s 上报一次。Rust 保活线程据此判断 WebView2
// 渲染进程是否假死（transparent 窗口的渲染进程一旦被终止，窗口会变成全透明"消失"），
// 超时则 reload 本窗口前端自愈。上报失败静默忽略，绝不影响渲染。
function alivePing() { invoke('alive_ping', { label: 'main' }).catch(() => {}); }

// 悬浮窗是常驻小挂件，不需要 WebView2 的默认右键菜单（刷新/另存为/检查…）。
// 捕获阶段拦一层，避免皮肤内部代码在冒泡阶段吃掉事件后菜单照样弹出。
document.addEventListener('contextmenu', (e) => { e.preventDefault(); }, true);
window.addEventListener('contextmenu', (e) => e.preventDefault());

let usage = null;
let compact = false;
let appSettings = {
  skin: 'card', font_scale: 100, opacity: 96, accent: 'auto', lang: 'zh',
  show_week: true, show_credits: true, show_countdown: true, show_email: true, poll_secs: 60,
};

const skinId = () => (window.SKINS[appSettings.skin] ? appSettings.skin : 'card');
const zoom = () => (appSettings.font_scale || 100) / 100;

const GEAR_SVG = `<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
const CHEV_SVG = `<svg viewBox="0 0 24 24"><path d="m6 15 6-6 6 6"/></svg>`;

/* ---------- 收起 / 展开：内容折叠与窗口补间共用一个时钟 ----------
   收起时有两个独立时钟在跑：窗口矩形由 Rust 的 animate_main_resize 逐帧改，
   内容由 CSS 过渡折叠。卡片在 body 里是 flex 居中，所以窗口边界与内容边界
   只要错开一点，卡片就会被"还在变形的窗口"推着走 —— 收起动画里的位置抖动
   就是这么来的。三条纪律保证两者同相：
     1. 时长/曲线完全一致：都是 240ms 的 cubic-bezier(.215,.61,.355,1)
        （Rust 侧不再用 1-(1-p)³ 近似，改为求同一条贝塞尔曲线）；
     2. 起点同帧：尺寸请求押到 requestAnimationFrame 里发，见 applySizeNextFrame；
     3. 皮肤里用 display 硬切的行，改写为按实测尺寸做的 max-height/盒模型过渡。
   任何一步失败都退化为原来的直接切换。 */
const RESIZE_MS = 240;                                   // ← 必须与 main.rs 的 RESIZE_MS 一致
const EASE_OUT_CUBIC = 'cubic-bezier(.215,.61,.355,1)';   // ← 必须与 main.rs 的 EASE_X1..Y2 一致
const BOX_PROPS = [
  'margin-top', 'margin-bottom', 'margin-left', 'margin-right',
  'padding-top', 'padding-bottom', 'padding-left', 'padding-right',
  'border-top-width', 'border-bottom-width',
];
const ANIM_PROPS = ['display', 'transition', 'width', 'height', 'opacity', 'overflow',
  'max-width', 'max-height', ...BOX_PROPS];
// width/height 必须在列表里：紧凑态会改根元素宽高的皮肤（card 264→232、glass 376→206）
// 走的是 sizeOnly 分支，只设 inline 宽高。少了这两项，根宽会在切换瞬间跳到位，
// 而窗口还要 240ms 才收到位 —— 卡片边缘与窗口边缘不同步，就是"抖"的来源。
const ANIM_TRANS = ['width', 'height', 'max-width', 'max-height', 'margin', 'padding', 'border-width']
  .map((p) => `${p} ${RESIZE_MS}ms ${EASE_OUT_CUBIC}`)
  .concat([`opacity ${Math.round(RESIZE_MS * 0.7)}ms ease`]).join(',');

let collapseRun = null;              // { timer, items, snaps }
let sizeRaf = 0;                     // 本帧已排过的尺寸请求（合并同帧的多次切换）

const snapInline = (el) => ANIM_PROPS.map((p) =>
  [p, el.style.getPropertyValue(p), el.style.getPropertyPriority(p)]);

function restoreInline(el, snap) {
  for (const [p, v, pr] of snap) {
    if (v) el.style.setProperty(p, v, pr); else el.style.removeProperty(p);
  }
}

/// 把尺寸请求押到**下一帧**再发（同帧多次切换合并成一次）。
///
/// 这一步是"让不动的边真的不动"的关键：CSS 过渡的 t0 是浏览器提交这一帧样式的
/// 瞬间，而 Rust 侧补间以收到 IPC 的时刻为 t0。若在 flip() 里同步下发（那时
/// 起点样式还没提交），窗口会抢跑整整一帧 —— 缓动起步最快，一帧就能拉开十几 px，
/// 之后内容一路追赶，看起来就是四周都在抖。押到下一帧，两个时钟同帧起跑。
function applySizeNextFrame() {
  if (sizeRaf) return;
  sizeRaf = requestAnimationFrame(() => { sizeRaf = 0; applySize(); });
}

/// 撤掉本次动画加的所有 inline。连续点收起/展开时也必须先收尾上一次，
/// 否则下一次会把"动画中间态"当成起点来量。
function endCollapseRun() {
  if (!collapseRun) return;
  clearTimeout(collapseRun.timer);
  const run = collapseRun;
  collapseRun = null;
  for (const { el } of run.items) {
    const s = run.snaps.get(el);
    if (s) restoreInline(el, s);
  }
}

/// 收起 / 展开切换（tgl 按钮的唯一入口）
function toggleCompact() {
  endCollapseRun();
  const rootEl = document.querySelector('#root > *');
  const flip = () => {
    compact = !compact;
    document.body.classList.toggle('compact', compact);
    applySizeNextFrame();            // 与 CSS 过渡同帧起跑，见上面的说明
  };
  if (!rootEl) { flip(); return; }

  const all = [rootEl, ...rootEl.querySelectorAll('*')];
  const geo = (el) => {
    const cs = getComputedStyle(el);
    return {
      hidden: cs.display === 'none',
      display: cs.display,
      w: el.offsetWidth,
      h: el.offsetHeight,
      box: BOX_PROPS.map((p) => cs.getPropertyValue(p)),
    };
  };
  const from = all.map(geo);
  const snaps = new Map();
  const items = [];
  try {
    flip();                        // 布局立刻切到目标形态，下面再用过渡把它"演"回来
    const to = all.map(geo);

    // 根元素自身宽高也归同一个时钟（card / glass 的紧凑态会改根宽）
    if (from[0].w !== to[0].w || from[0].h !== to[0].h) {
      snaps.set(rootEl, snapInline(rootEl));
      items.push({ el: rootEl, from: from[0], to: to[0], sizeOnly: true });
    }
    // display 发生切换的行 → 用实测尺寸做折叠/展开过渡
    all.forEach((el, i) => {
      if (from[i].hidden === to[i].hidden) return;
      snaps.set(el, snapInline(el));
      items.push({ el, from: from[i], to: to[i] });
    });
    if (!items.length) return;

    for (const { el, from: f, to: a, sizeOnly } of items) {
      el.style.transition = ANIM_TRANS;
      el.style.overflow = 'hidden';
      if (sizeOnly) {                        // 可见元素：只过渡宽高，摆回原尺寸当起点
        el.style.width = f.w + 'px';
        el.style.height = f.h + 'px';
        continue;
      }
      // 动画期间必须可见：展开时它是 display:none，取可见那一侧的 display 值，
      // 并用 important 压过皮肤紧凑规则里的 display:none
      el.style.setProperty('display', f.hidden ? a.display : f.display, 'important');
      el.style.opacity = f.hidden ? '0' : '1';
      el.style.maxWidth = (f.hidden ? 0 : f.w) + 'px';
      el.style.maxHeight = (f.hidden ? 0 : f.h) + 'px';
      BOX_PROPS.forEach((p, k) => el.style.setProperty(p, f.hidden ? '0px' : f.box[k]));
    }
    void rootEl.offsetWidth;                 // 提交起点状态，过渡才有得插值

    for (const { el, to: a, sizeOnly } of items) {
      if (sizeOnly) {
        el.style.width = a.w + 'px';
        el.style.height = a.h + 'px';
        continue;
      }
      el.style.opacity = a.hidden ? '0' : '1';
      el.style.maxWidth = (a.hidden ? 0 : a.w) + 'px';
      el.style.maxHeight = (a.hidden ? 0 : a.h) + 'px';
      BOX_PROPS.forEach((p, k) => el.style.setProperty(p, a.hidden ? '0px' : a.box[k]));
    }
    // 收尾：撤掉 inline，交还给皮肤的紧凑规则（此刻两者已一致，不会跳）
    collapseRun = { items, snaps, timer: setTimeout(endCollapseRun, RESIZE_MS + 60) };
  } catch (err) {
    collapseRun = null;                      // 动画失败不影响收起本身
  }
}


async function mountSkin() {
  const sk = window.SKINS[skinId()];
  if (!sk) return;

  let link = document.getElementById('skin-css');
  if (!link) { link = document.createElement('link'); link.id = 'skin-css'; link.rel = 'stylesheet'; document.head.appendChild(link); }
  link.href = 'skins/' + skinId() + '.css';

  const root = document.getElementById('root');
  root.innerHTML = HP.i(sk.html);
  root.style.zoom = zoom();
  root.style.opacity = (appSettings.opacity ?? 96) / 100;

  // 外壳按钮注入皮肤的 chrome 槽位（没有槽位就绝对定位右上）
  const chrome = document.createElement('span');
  chrome.className = 'hp-chrome';
  chrome.innerHTML = `<button id="gear" class="tgl" title="${HP.t('ttl_settings')}">${GEAR_SVG}</button>` +
    (sk.sizes.compact ? `<button id="tgl" class="tgl" title="${HP.t('ttl_toggle')}">${CHEV_SVG}</button>` : '');
  const slot = root.querySelector('.hp-chrome-slot');
  if (slot) { slot.style.display = 'inline-flex'; slot.appendChild(chrome); }
  else {
    chrome.style.position = 'absolute'; chrome.style.right = '8px'; chrome.style.top = '8px'; chrome.style.zIndex = 9;
    root.appendChild(chrome);
  }
  $('#gear').addEventListener('click', () => invoke('open_settings').catch(() => {}));
  if ($('#tgl')) $('#tgl').addEventListener('click', () => toggleCompact());

  // 拖拽：整卡按下即系统级拖拽（按钮除外）
  root.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    if (e.target.closest('.tgl')) return;
    try { window.__TAURI__.window.getCurrentWindow().startDragging(); } catch (err) { /* ignore */ }
  });

  if (usage) sk.update(root.firstElementChild, usage, appSettings);
  await applySize();
  alivePing();
}

function applySize() {
  const sk = window.SKINS[skinId()];
  const use = (compact && sk.sizes.compact) ? sk.sizes.compact : sk.sizes.full;
  return invoke('set_window_size', { w: use[0] * zoom(), h: use[1] * zoom() }).catch(() => {});
}

function render(u) {
  usage = u;
  const sk = window.SKINS[skinId()];
  const rootEl = document.querySelector('#root > *');
  if (sk && rootEl) sk.update(rootEl, u, appSettings);
  alivePing();
}

function applySettings(s) {
  const prevSkin = appSettings.skin;
  const prevLang = appSettings.lang;
  appSettings = s;
  HP.setLang(s.lang);
  // 换肤或切语言都要重新挂载（模板令牌在挂载时按当前语言替换）
  if (s.skin !== prevSkin || s.lang !== prevLang || !document.querySelector('#root > *')) { mountSkin(); return; }
  const root = document.getElementById('root');
  root.style.zoom = zoom();
  root.style.opacity = (s.opacity ?? 96) / 100;
  applySize();
  render(usage || { ok: false, source: 'none' });
}

listen('usage', (e) => render(e.payload));
listen('settings', (e) => applySettings(e.payload));
invoke('refresh_now').then(render).catch(() => render({ ok: false, source: 'none' }));
invoke('get_settings').then(applySettings).catch(() => {});
setInterval(alivePing, 5000);
