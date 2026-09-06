// 外壳：皮肤加载 / 数据分发 / 尺寸同步 / 拖拽 / 设置应用
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);

let usage = null;
let compact = false;
let appSettings = {
  skin: 'card', font_scale: 100, opacity: 96, accent: 'auto',
  show_week: true, show_credits: true, show_countdown: true, show_email: true, poll_secs: 60,
};

const skinId = () => (window.SKINS[appSettings.skin] ? appSettings.skin : 'card');
const zoom = () => (appSettings.font_scale || 100) / 100;

const GEAR_SVG = `<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
const CHEV_SVG = `<svg viewBox="0 0 24 24"><path d="m6 15 6-6 6 6"/></svg>`;

async function mountSkin() {
  const sk = window.SKINS[skinId()];
  if (!sk) return;

  let link = document.getElementById('skin-css');
  if (!link) { link = document.createElement('link'); link.id = 'skin-css'; link.rel = 'stylesheet'; document.head.appendChild(link); }
  link.href = 'skins/' + skinId() + '.css';

  const root = document.getElementById('root');
  root.innerHTML = sk.html;
  root.style.zoom = zoom();
  root.style.opacity = (appSettings.opacity ?? 96) / 100;

  // 外壳按钮注入皮肤的 chrome 槽位（没有槽位就绝对定位右上）
  const chrome = document.createElement('span');
  chrome.className = 'hp-chrome';
  chrome.innerHTML = `<button id="gear" class="tgl" title="设置">${GEAR_SVG}</button>` +
    (sk.sizes.compact ? `<button id="tgl" class="tgl" title="切换 完整 / 紧凑 形态">${CHEV_SVG}</button>` : '');
  const slot = root.querySelector('.hp-chrome-slot');
  if (slot) { slot.style.display = 'inline-flex'; slot.appendChild(chrome); }
  else {
    chrome.style.position = 'absolute'; chrome.style.right = '8px'; chrome.style.top = '8px'; chrome.style.zIndex = 9;
    root.appendChild(chrome);
  }
  $('#gear').addEventListener('click', () => invoke('open_settings').catch(() => {}));
  if ($('#tgl')) $('#tgl').addEventListener('click', async () => {
    compact = !compact;
    document.body.classList.toggle('compact', compact);
    await applySize();
  });

  // 拖拽：整卡按下即系统级拖拽（按钮除外）
  root.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    if (e.target.closest('.tgl')) return;
    try { window.__TAURI__.window.getCurrentWindow().startDragging(); } catch (err) { /* ignore */ }
  });

  if (usage) sk.update(root.firstElementChild, usage, appSettings);
  await applySize();
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
}

function applySettings(s) {
  const prevSkin = appSettings.skin;
  appSettings = s;
  if (s.skin !== prevSkin || !document.querySelector('#root > *')) { mountSkin(); return; }
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
