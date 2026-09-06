// 设置窗口：读取/保存设置（改动即保存并实时应用到悬浮窗）
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);
const T = (k) => window.i18nT(k);
const FIELDS = ["lang", "skin", "font_scale", "opacity", "accent", "show_week", "show_credits", "show_countdown", "show_email", "poll_secs", "autostart", "mini_enabled", "mini_pos"];

let loading = true;
let curLang = null;
let lastUsage = null;

// 语言变化后重译静态界面 + 按当前语言重建皮肤预览
function applyLangUI() {
  window.i18nApply(document);
  document.title = T('settings_title');
  document.documentElement.lang = HP.lang === 'en' ? 'en' : 'zh-CN';
  document.querySelectorAll(".sk-card-pv").forEach((card) => {
    const sk = window.SKINS[card.dataset.skin];
    if (!sk) return;
    card.querySelector(".pv-full").innerHTML = HP.i(sk.html);
    card.querySelector(".pv-mini").innerHTML = HP.i(sk.mini);
    card.querySelector(".pv-name").textContent = T('skin_' + card.dataset.skin);
  });
  if (lastUsage) showUsage(lastUsage); // 数据源状态行按新语言立即重译
  refreshPreviews();
  layoutPreviews();
}

function fill(s) {
  for (const k of FIELDS) {
    const el = document.getElementById(k);
    if (!el || s[k] === undefined) continue;
    if (el.type === "checkbox") el.checked = !!s[k];
    else el.value = s[k];
  }
  $("#font_scale_v").textContent = s.font_scale + "%";
  $("#opacity_v").textContent = s.opacity + "%";
  markSel();
}

function collect() {
  const s = {};
  for (const k of FIELDS) {
    const el = document.getElementById(k);
    if (el.type === "checkbox") s[k] = el.checked;
    else if (el.type === "range") s[k] = parseInt(el.value, 10);
    else if (k === "poll_secs") s[k] = parseInt(el.value, 10);
    else s[k] = el.value;
  }
  return s;
}

async function persist() {
  if (loading) return;
  try {
    const saved = await invoke("save_settings", { s: collect() });
    fill(saved);
    HP.setLang(saved.lang);
    if (saved.lang !== curLang) { curLang = saved.lang; applyLangUI(); }
    const t = new Date();
    $("#saved").textContent = T('saved_at') + ' ' + t.toLocaleTimeString(HP.lang === 'en' ? 'en-US' : 'zh-CN', { hour12: false });
  } catch (e) {
    $("#saved").textContent = T('save_failed') + e;
  }
}

function showUsage(u) {
  lastUsage = u;
  const el = $("#src_info");
  if (u && u.ok) {
    el.innerHTML = `<b style="color:#6ee7b7">${T('connected')}</b> · ${u.source} · ${T('conn_5h_left')} ${Math.round(100 - (u.primary.used_percent ?? 0))}%` +
      (u.email ? ` · ${u.email}` : "") + (u.plan ? ` · ${String(u.plan).toUpperCase()}` : "");
  } else {
    el.innerHTML = `<b style="color:#f87171">${T('src_none')}</b> · ${T('src_none_detail')}`;
  }
}

// ===== 皮肤预览网格：各皮肤真实模板+CSS 渲染，点卡片即换肤 =====
const MOCK_USAGE = {
  ok: true, plan: "plus", email: "codex@example.com",
  get source() { return T("mock_source"); }, // 语言切换后预览数据源名同步重译
  credits: { unlimited: false, balance: 12.5 },
  primary: { used_percent: 34, resets_in_seconds: 2 * 3600 + 51 * 60 },
  secondary: { used_percent: 61, resets_in_seconds: 5 * 3600 + 12 * 60 },
};
const PV_ORDER = ["hero", "glass", "term", "card", "tile", "battery", "gauge", "neon", "liquid", "pixel"];

function markSel() {
  const cur = $("#skin") && $("#skin").value;
  if (!cur) return;
  document.querySelectorAll(".sk-card-pv").forEach((c) =>
    c.classList.toggle("sel", c.dataset.skin === cur));
}

function refreshPreviews() {
  if (!window.SKINS) return;
  const s = collect();
  document.querySelectorAll(".sk-card-pv").forEach((card) => {
    const sk = window.SKINS[card.dataset.skin];
    if (!sk) return;
    try { sk.update && sk.update(card.querySelector(".pv-full").firstElementChild, MOCK_USAGE, s); } catch (e) {}
    try { sk.updateMini && sk.updateMini(card.querySelector(".pv-mini").firstElementChild, MOCK_USAGE, s); } catch (e) {}
  });
}

// 按各皮肤声明的逻辑尺寸缩放到固定舞台内（窗口缩放时重算）
function layoutPreviews() {
  document.querySelectorAll(".sk-card-pv").forEach((card) => {
    const sk = window.SKINS[card.dataset.skin];
    if (!sk) return;
    const stage = card.querySelector(".pv-stage");
    const full = card.querySelector(".pv-full");
    const [w, h] = sk.sizes.full;
    const s = Math.min((stage.clientWidth - 14) / w, (stage.clientHeight - 8) / h);
    full.style.transform = `scale(${s})`;
    full.style.left = (stage.clientWidth - w * s) / 2 + "px";
    full.style.top = (stage.clientHeight - h * s) / 2 + "px";
    const ms = card.querySelector(".pv-ministage");
    const mini = card.querySelector(".pv-mini");
    const mw = mini.firstElementChild.offsetWidth, mh = mini.firstElementChild.offsetHeight;
    const sm = Math.min((ms.clientWidth - 18) / mw, (ms.clientHeight - 6) / mh, 1);
    mini.style.transform = `scale(${sm})`;
    mini.style.left = (ms.clientWidth - mw * sm) / 2 + "px";
    mini.style.top = (ms.clientHeight - mh * sm) / 2 + "px";
  });
}

function buildSkinGrid() {
  const grid = $("#skin_grid");
  if (!grid) return;
  for (const id of PV_ORDER) {
    const sk = window.SKINS[id];
    if (!sk) continue;
    const card = document.createElement("div");
    card.className = "sk-card-pv";
    card.dataset.skin = id;
    card.innerHTML =
      `<div class="pv-stage"><div class="pv-full">${HP.i(sk.html)}</div></div>` +
      `<div class="pv-ministage"><div class="pv-mini">${HP.i(sk.mini)}</div></div>` +
      `<div class="pv-name">${T('skin_' + id)}</div>`;
    card.addEventListener("click", () => {
      if ($("#skin").value === id) return;
      $("#skin").value = id;
      markSel();
      persist();
    });
    grid.appendChild(card);
  }
  refreshPreviews();
  layoutPreviews();
  setTimeout(layoutPreviews, 300); // 字体加载完成后校一次尺寸
  window.addEventListener("resize", layoutPreviews);
}

buildSkinGrid();

for (const k of FIELDS) {
  const el = document.getElementById(k);
  if (!el) continue;
  el.addEventListener("change", () => { persist(); refreshPreviews(); });
  if (el.type === "range") {
    el.addEventListener("input", () => {
      $("#" + k + "_v").textContent = el.value + "%";
    });
  }
}

$("#rescan").addEventListener("click", () => {
  $("#src_info").textContent = T('refreshing');
  invoke("refresh_now").then(showUsage).catch((e) => { $("#src_info").textContent = T('refresh_failed') + e; });
});

$("#reset").addEventListener("click", async () => {
  const def = { skin: "card", font_scale: 100, opacity: 96, accent: "auto", show_week: true, show_credits: true, show_countdown: true, show_email: true, poll_secs: 60, autostart: false };
  fill(def);
  await persist();
});

listen("usage", (e) => showUsage(e.payload));

$("#src_info").textContent = T('src_reading');

(async () => {
  const s = await invoke("get_settings");
  HP.setLang(s.lang);
  curLang = s.lang;
  applyLangUI();
  fill(s);
  loading = false;
  invoke("refresh_now").then(showUsage).catch(() => {});
})();
