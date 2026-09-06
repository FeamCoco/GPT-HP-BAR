// 皮肤 ⑥ 电池 Battery —— 额度=电量：电池图标填充条宽度=剩余%，tone 渐变（90deg），低电量闪烁
window.SKINS = window.SKINS || {};

window.SKINS['battery'] = {
  name: '⑥ 电池 Battery',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [244, 130], compact: null },

  html: `
    <div class="sk-battery">
      <div class="top">
        <span class="hp-mail"></span>
        <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
      </div>
      <div class="main">
        <div class="battery"><i class="fill"></i></div>
        <div><span class="num">--%</span><div class="lab">Codex 剩余额度</div></div>
      </div>
      <div class="sub">
        <span><span class="win">5h 窗口</span><span class="cdw"> · 重置 <b class="cdv">--</b></span></span>
        <span class="wk">周 <b class="nw">--%</b></span>
      </div>
    </div>`,

  mini: `
    <div class="sk-battery-mini">
      <span class="m-batt"><i class="m-fill"></i></span><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    const danger = HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('danger', danger && rem5 != null && rem5 <= 20);
    el.classList.toggle('nodata', !ok);

    q('.fill').style.width = (rem5 == null ? 0 : rem5) + '%';
    q('.num').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    q('.cdv').textContent = HP.dur(ok ? u.primary.resets_in_seconds : null);
    q('.cdw').style.display = s.show_countdown === false ? 'none' : '';
    if (s.show_week !== false) q('.nw').textContent = remw == null ? '--%' : Math.round(remw) + '%';
    q('.wk').style.display = s.show_week === false ? 'none' : '';
    // show_credits：电池无 credits 行，忽略

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
