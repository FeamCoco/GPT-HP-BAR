// 设置窗口：读取/保存设置（改动即保存并实时应用到悬浮窗）
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);
const FIELDS = ["skin", "font_scale", "opacity", "accent", "show_week", "show_credits", "show_countdown", "show_email", "poll_secs", "autostart", "mini_enabled", "mini_pos"];

let loading = true;

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
    const t = new Date();
    $("#saved").textContent = "已保存 " + t.toLocaleTimeString("zh-CN", { hour12: false });
  } catch (e) {
    $("#saved").textContent = "保存失败：" + e;
  }
}

function showUsage(u) {
  const el = $("#src_info");
  if (u && u.ok) {
    el.innerHTML = `<b style="color:#6ee7b7">已连接</b> · ${u.source} · 5h 剩余 ${Math.round(100 - (u.primary.used_percent ?? 0))}%` +
      (u.email ? ` · ${u.email}` : "") + (u.plan ? ` · ${String(u.plan).toUpperCase()}` : "");
  } else {
    el.innerHTML = `<b style="color:#f87171">无数据源</b> · 当前未检测到 ChatGPT 登录态或兼容的中转接口`;
  }
}

// ===== 皮肤预览网格：各皮肤真实模板+CSS 渲染，点卡片即换肤 =====
const MOCK_USAGE = {
  ok: true, source: "ChatGPT 官方", plan: "plus", email: "codex@example.com",
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
      `<div class="pv-stage"><div class="pv-full">${sk.html}</div></div>` +
      `<div class="pv-ministage"><div class="pv-mini">${sk.mini}</div></div>` +
      `<div class="pv-name">${sk.name}</div>`;
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
  $("#src_info").textContent = "刷新中…";
  invoke("refresh_now").then(showUsage).catch((e) => { $("#src_info").textContent = "刷新失败：" + e; });
});

$("#reset").addEventListener("click", async () => {
  const def = { skin: "card", font_scale: 100, opacity: 96, accent: "auto", show_week: true, show_credits: true, show_countdown: true, show_email: true, poll_secs: 60, autostart: false };
  fill(def);
  await persist();
});

listen("usage", (e) => showUsage(e.payload));

(async () => {
  fill(await invoke("get_settings"));
  loading = false;
  invoke("refresh_now").then(showUsage).catch(() => {});
})();
