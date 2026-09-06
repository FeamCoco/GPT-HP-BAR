// 皮肤 ⑩ 像素 Pixel（还原 showcase 方案⑩：8-bit 心形 + 方块血槽，on/off 由 JS 生成）
window.SKINS = window.SKINS || {};

window.SKINS['pixel'] = {
  name: '⑩ 像素 Pixel',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [316, 150], compact: null },

  html: `
    <div class="pixel sk-pixel">
      <div class="p-title">CODEX-HP <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span><span class="p-player">PLAYER 1</span></div>
      <div class="p-hearts"><span>♥</span><span>♥</span><span>♥</span><span>♥</span><span>♥</span></div>
      <div class="p-blocks"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
      <div class="p-sub">
        <span class="p-cdw">RESET <b class="p-cd">--</b></span>
        <span class="p-wkw">WK <b class="p-wk">--</b>%</span>
        <span>HP <b class="p-hp">--</b>%</span>
      </div>
    </div>`,

  mini: `
    <div class="sk-pixel-mini"><i></i><i></i><i></i><i></i><span class="m-num">--%</span></div>`,

  // 5 颗心各代表 20%：i < ceil(剩余/20) 点亮；剩余 ≤20% 时最后一颗亮心闪烁
  setHearts(el, rem) {
    const lit = rem == null ? 0 : Math.ceil(rem / 20);
    const low = rem != null && rem <= 20;
    el.querySelectorAll('.p-hearts span').forEach((h, i) => {
      h.classList.toggle('on', i < lit);
      h.classList.toggle('blink', low && i === lit - 1);
    });
  },

  // 10 格方块血槽（与 showcase 相同取整规则）
  setBlocks(el, rem) {
    const lit = rem == null ? 0 : Math.round(rem / 10);
    el.querySelectorAll('.p-blocks i').forEach((b, i) => b.classList.toggle('on', i < lit));
  },

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    const remw = ok && u.secondary ? HP.rem(u.secondary.used_percent) : null;
    HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('nodata', !ok);

    this.setHearts(el, rem5);
    this.setBlocks(el, rem5);

    q('.p-cd').textContent = HP.dur(ok && u.primary ? u.primary.resets_in_seconds : null);
    q('.p-wk').textContent = remw == null ? '--' : Math.round(remw);
    q('.p-hp').textContent = rem5 == null ? '--' : Math.round(rem5);

    const show = (sel, on) => { const n = q(sel); if (n) n.style.display = on ? '' : 'none'; };
    show('.p-cdw', s.show_countdown !== false);
    show('.p-wkw', s.show_week !== false);
  },

  updateMini(el, u, s) {
    const rem = u && u.ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    const lit = rem == null ? 0 : Math.ceil(rem / 25); // 4 格，每格 25%
    el.querySelectorAll('i').forEach((b, i) => b.classList.toggle('on', i < lit));
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
