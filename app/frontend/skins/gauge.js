// 皮肤 ⑦ 表盘 Gauge —— 半圆仪表：弧线 dasharray=剩余/2（pathLength=100），指针角度=剩余%*1.8-90
window.SKINS = window.SKINS || {};

window.SKINS['gauge'] = {
  name: '⑦ 表盘 Gauge',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [228, 220], compact: [228, 170] },

  html: `
    <div class="sk-gauge">
      <div class="top">
        <span class="hp-mail"></span>
        <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
      </div>
      <svg class="gauge-svg" viewBox="0 0 168 92">
        <path class="g-track" d="M16 80 A68 68 0 0 1 152 80" pathLength="100"/>
        <path class="g-arc" d="M16 80 A68 68 0 0 1 152 80" pathLength="100"/>
        <line class="g-tick" x1="84" y1="6" x2="84" y2="14"/>
        <line class="g-tick" x1="22" y1="42" x2="29" y2="46"/>
        <line class="g-tick" x1="146" y1="42" x2="139" y2="46"/>
        <line class="g-needle" x1="84" y1="80" x2="84" y2="22"/>
        <circle class="g-hub" cx="84" cy="80" r="4.5"/>
      </svg>
      <div class="num">--%</div>
      <div class="cap">@@gauge_cap@@</div>
      <div class="sub"><span class="cdw">@@reset@@ <b class="cdv">--</b></span><span class="wk">@@week_w@@ <b class="nw">--%</b></span></div>
    </div>`,

  mini: `
    <div class="sk-gauge-mini">
      <svg class="m-gauge" viewBox="0 0 22 13">
        <path class="m-track" d="M2 11 A9 9 0 0 1 20 11" pathLength="100"/>
        <path class="m-arc" d="M2 11 A9 9 0 0 1 20 11" pathLength="100"/>
      </svg><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('nodata', !ok);

    // 弧线（重）与指针（轻）都吃同一个 620ms 时钟，但曲线不同：
    // 弧线用 easeOut 稳稳扫过去，指针用 easeBack 带一点过冲 —— 表盘的"回针弹簧感"
    // 就在这一点上。半圆弧只覆盖 pathLength 的一半：弧长 = 剩余%/2（0~50 / 100）。
    const arc = q('.g-arc'), num = q('.num');
    HP.tween(el, 'g5', rem5, (v) => {
      arc.setAttribute('stroke-dasharray', (v == null ? 0 : v / 2) + ' 100');
      num.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { start: () => HP.fx(num, 'pop') });
    // 指针：0% -> -90°（左端），100% -> +90°（右端）
    HP.tween(el, 'gneedle', rem5, (v) => {
      q('.g-needle').style.transform = 'rotate(' + (v == null ? -90 : v * 1.8 - 90) + 'deg)';
    }, { ease: HP.easeBack });
    const cdVal = HP.durMaybe(ok ? u.primary.resets_in_seconds : null);
    q('.cdv').textContent = cdVal ?? '--';

    const cdOn = s.show_countdown !== false && cdVal != null, wkOn = s.show_week !== false;
    q('.cdw').style.display = cdOn ? '' : 'none';
    HP.tween(el, 'gw', wkOn ? remw : null, (v) => {
      q('.nw').textContent = v == null ? '--%' : Math.round(v) + '%';
    });
    q('.wk').style.display = wkOn ? '' : 'none';
    q('.sub').style.display = (cdOn || wkOn) ? '' : 'none';
    // show_credits：表盘无 credits 行，忽略

    const m = q('.hp-mail'); const txt = HP.mail(u.email);
    m.textContent = txt; m.title = u.email || '';
    m.style.display = (s.show_email === false || !txt) ? 'none' : '';
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    const arc = el.querySelector('.m-arc'), n = el.querySelector('.m-num');
    HP.tween(el, 'm', rem, (v) => {
      arc.setAttribute('stroke-dasharray', (v == null ? 0 : v / 2) + ' 100');
      n.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { start: () => HP.fx(n, 'charge') });
  },
};
