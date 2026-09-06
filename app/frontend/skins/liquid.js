// 皮肤 ⑨ 液体 Liquid（还原 showcase 方案⑨：液面高度=剩余%，双层波浪流动，数字浮层）
window.SKINS = window.SKINS || {};

window.SKINS['liquid'] = {
  name: '⑨ 液体 Liquid',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [240, 220], compact: null },

  html: `
    <div class="liquid sk-liquid">
      <span class="hp-chrome-slot"></span>
      <div class="l-body">
        <div class="l-water">
          <svg class="l-wave l-wa" viewBox="0 0 120 20" preserveAspectRatio="none"><path d="M0 10 Q 15 0 30 10 T 60 10 T 90 10 T 120 10 V20 H0 Z"/></svg>
          <svg class="l-wave l-wb" viewBox="0 0 120 20" preserveAspectRatio="none"><path d="M0 10 Q 15 2 30 10 T 60 10 T 90 10 T 120 10 V20 H0 Z"/></svg>
        </div>
        <div class="l-num">--%</div>
      </div>
      <div class="l-cap"><b class="l-rem">--</b> 剩余<span class="l-cdseg"> · 重置 <b class="l-cd">--</b></span></div>
    </div>`,

  mini: `
    <div class="sk-liquid-mini"><span class="m-cup"><i class="m-fill"></i><em class="m-bub"></em></span><span class="m-num">--%</span></div>`,

  // 窗口剩余时长 ≈ window_minutes × 剩余%，满杯时正好 "5h"（与 showcase 文案一致）
  remainTxt(mins, rem) {
    if (rem == null) return '--';
    if (mins == null || mins <= 0) return Math.round(rem) + '%';
    const m = Math.max(0, Math.round((mins * rem) / 100));
    const h = Math.floor(m / 60), mm = m % 60;
    return h > 0 ? (mm ? h + 'h ' + mm + 'm' : h + 'h') : mm + 'm';
  },

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem5, s.accent); // 液体/波浪随三档色变化，≤20% 自动泛红
    el.classList.toggle('nodata', !ok);

    const water = q('.l-water');
    if (rem5 == null || rem5 <= 0) water.style.display = 'none';
    else { water.style.display = ''; water.style.height = rem5 + '%'; }

    q('.l-num').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    q('.l-rem').textContent = this.remainTxt(ok && u.primary ? u.primary.window_minutes : null, rem5);
    q('.l-cd').textContent = HP.dur(ok && u.primary ? u.primary.resets_in_seconds : null);
    q('.l-cdseg').style.display = s.show_countdown === false ? 'none' : '';
  },

  updateMini(el, u, s) {
    const rem = u && u.ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.querySelector('.m-fill').style.width = (rem == null ? 0 : rem) + '%';
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
