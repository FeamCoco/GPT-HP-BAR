// 模型用量面板：数据由 Rust stats 线程持续 emit "stats"（活跃时 2s 一次），
// 打开时先 invoke('get_stats') 打底。本窗口不含任何轮询逻辑。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);
const T = (k) => window.i18nT(k);

let tab = 'today';
let last = null;

/* ---- 数字格式：1_234_567 -> 1.23M，12345 -> 12.3k，<1000 原样 ---- */
function fmtTok(n) {
  if (n == null || isNaN(n)) return '--';
  if (n >= 1e6) return (n / 1e6).toFixed(2) + 'M';
  if (n >= 1e4) return (n / 1e3).toFixed(1) + 'k';
  if (n >= 1e3) return (n / 1e3).toFixed(2) + 'k';
  return String(n);
}

/* 秒 -> "45 s / 12 min / 3 h"（休眠态的"最近活动"） */
function fmtAge(sec) {
  if (sec == null) return '';
  if (sec < 60) return sec + ' s';
  if (sec < 3600) return Math.round(sec / 60) + ' min';
  return Math.round(sec / 3600) + ' h';
}

function modelName(m) {
  return m === 'unknown' ? T('model_unknown') : m;
}

function renderSpeed(st) {
  const card = $('#speed_card'), icon = $('#spd_icon'), val = $('#spd_val'), name = $('#spd_name');
  if (st.active) {
    card.classList.add('on');
    if (st.speed_tok_s == null) {
      // 活跃但窗口内采样不足（工具调用长间隙）：诚实显示"生成中"，不硬编一个 0
      icon.textContent = '⏳';
      val.textContent = '…';
      name.textContent = T('st_running');
    } else {
      const tier = window.speedTier(st.speed_tok_s);
      const n = Math.min(999, Math.round(st.speed_tok_s));
      icon.textContent = tier.icon;
      val.textContent = String(n);
      name.textContent = tier.name + ' · ' + T('st_active');
    }
  } else {
    card.classList.remove('on');
    icon.textContent = '💤';
    val.textContent = '--';
    name.textContent = T('st_idle') +
      (st.last_event_age_secs != null ? ' · ' + T('st_last_activity') + ' ' + fmtAge(st.last_event_age_secs) : '');
  }
  $('#sum_today').textContent = fmtTok(st.today_total);
  $('#sum_week').textContent = fmtTok(st.week_total);
}

function renderTable(st) {
  const rows = tab === 'today' ? st.today : st.week;
  const tb = $('#tbl');
  if (!rows.length) {
    tb.innerHTML = `<tr><td class="none" colspan="5">${T('st_empty')}</td></tr>`;
    return;
  }
  const max = rows[0].total_tokens || 1;
  tb.innerHTML = rows.map((m) => {
    const share = Math.max(2, Math.round((m.total_tokens / max) * 100));
    const name = modelName(m.model);
    return `<tr>
      <td><div class="m-name" title="${name}">${name}</div>
          <div class="m-bar"><i style="width:${share}%"></i></div></td>
      <td class="num">${fmtTok(m.input_tokens)}${m.cached_input_tokens > 0
        ? `<span class="sub">(缓存 ${fmtTok(m.cached_input_tokens)})</span>` : ''}</td>
      <td class="num">${fmtTok(m.output_tokens)}</td>
      <td class="num tot">${fmtTok(m.total_tokens)}</td>
      <td class="num">${m.responses || ''}</td>
    </tr>`;
  }).join('');
}

function renderHours(st) {
  const box = $('#hours');
  const max = Math.max(...st.hours, 1);
  const nowH = new Date().getHours();
  const any = st.hours.some((v) => v > 0);
  box.innerHTML = st.hours.map((v, h) => {
    const cls = v === 0 ? 'col zero' : (h === nowH ? 'col now' : 'col');
    const hgt = any && v > 0 ? Math.max(6, Math.round((v / max) * 100)) : 4;
    return `<div class="${cls}" title="${String(h).padStart(2, '0')}:00 · ${v} tok">
      <i style="height:${any ? hgt : 100}%"></i></div>`;
  }).join('');
}

function renderMeta(st) {
  const t = new Date(st.scanned_at * 1000);
  const hh = String(t.getHours()).padStart(2, '0') + ':' + String(t.getMinutes()).padStart(2, '0');
  $('#meta').textContent = `${st.day} · ${T('st_files')} ${st.files} · ${T('st_scanned')} ${hh}`;
}

function renderAll(st) {
  const has = !!(st && st.ok);
  $('#empty').style.display = has ? 'none' : '';
  $('#sec_hours').style.display = has ? '' : 'none';
  $('#sec_tbl').style.display = has ? '' : 'none';
  $('#speed_card').style.display = has ? '' : 'none';
  if (!has) return;
  last = st;
  renderMeta(st);
  renderSpeed(st);
  renderHours(st);
  renderTable(st);
}

document.querySelectorAll('.tab').forEach((b) => {
  b.addEventListener('click', () => {
    tab = b.dataset.tab;
    document.querySelectorAll('.tab').forEach((x) => x.classList.toggle('on', x === b));
    if (last) renderTable(last);
  });
});

listen('stats', (e) => renderAll(e.payload));

// 界面语言跟随设置（与 settings.js 同款：读设置 -> 设字典 -> 重译静态文案）
(async () => {
  try {
    const s = await invoke('get_settings');
    window.I18N_LANG = s.lang === 'en' ? 'en' : 'zh';
  } catch (e) { /* 取不到设置就按默认 zh */ }
  window.i18nApply(document);
  document.title = window.I18N_LANG === 'en' ? 'GPT-HP-BAR Model Usage' : 'GPT-HP-BAR 模型用量';
  renderAll(await invoke('get_stats').catch(() => null));
})();
