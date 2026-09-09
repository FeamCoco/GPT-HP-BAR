// 皮肤 ⑤ 磁贴 Tile —— 78px Fluent 磁贴：状态色渐变底 + 白色大数字 + 底部白色进度线（宽度=剩余%）
window.SKINS = window.SKINS || {};

window.SKINS['tile'] = {
  name: '⑤ 磁贴 Tile',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [128, 150], compact: null },

  html: `
    <div class="sk-tile">
      <div class="chrome-row">
        <span class="hp-mail"></span>
        <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
      </div>
      <div class="tile">
        <div class="lab">CODEX</div>
        <span class="num"><span class="nv">--</span><small>%</small></span>
        <div class="track"><i class="fill"></i></div>
        <div class="sub">@@reset@@ <span class="cdv">--</span></div>
      </div>
    </div>`,

  mini: `
    <div class="sk-tile-mini">
      <span class="m-tile"><i class="m-track"></i><i class="m-fill"></i></span><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.classList.toggle('nodata', !ok);

    q('.nv').textContent = rem == null ? '--' : String(Math.round(rem));
    q('.fill').style.width = (rem == null ? 0 : rem) + '%';
    const cdVal = HP.durMaybe(ok ? u.primary.resets_in_seconds : null);
    q('.cdv').textContent = cdVal ?? '--';
    // 倒计时行可关（磁贴 sub 行仅承载重置倒计时），无数据时同样隐藏
    q('.sub').style.display = (s.show_countdown === false || cdVal == null) ? 'none' : '';
    // show_week / show_credits：磁贴无对应行，忽略

    const m = q('.hp-mail'); const txt = HP.mail(u.email);
    m.textContent = txt; m.title = u.email || '';
    m.style.display = (s.show_email === false || !txt) ? 'none' : '';
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.querySelector('.m-fill').style.width = (rem == null ? 0 : rem) + '%';
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
