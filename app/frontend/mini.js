// 任务栏挂件：挂载当前皮肤的 mini 形态，尺寸变化通知 Rust 重新定位
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let usage = null;
let settings = null;
let cur = null;

function mount() {
  const id = settings && window.SKINS[settings.skin] ? settings.skin : 'card';
  cur = window.SKINS[id];

  let link = document.getElementById('skin-css');
  if (!link) {
    link = document.createElement('link');
    link.id = 'skin-css';
    link.rel = 'stylesheet';
    // 样式表加载完成后再测量尺寸，避免拿到的是无样式裸宽度
    link.onload = () => syncSize();
    document.head.appendChild(link);
  }
  link.href = 'skins/' + id + '.css';

  const root = document.getElementById('root');
  root.innerHTML = cur.mini;
  syncSize();
  setTimeout(syncSize, 150); // 兜底：样式/字体就绪后再校一次
}

function syncSize() {
  const el = document.querySelector('#root > *');
  if (!el) return;
  if (usage && cur) cur.updateMini(el, usage, settings || {});
  const w = el.offsetWidth + 8;
  const h = el.offsetHeight + 8;
  invoke('set_window_size', { w, h }).catch(() => {});
}

listen('usage', (e) => {
  usage = e.payload;
  const el = document.querySelector('#root > *');
  if (cur && el) cur.updateMini(el, usage, settings || {});
});

listen('settings', (e) => { settings = e.payload; mount(); });

invoke('get_settings').then((s) => { settings = s; mount(); }).catch(() => mount());
