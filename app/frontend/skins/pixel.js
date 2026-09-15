// 皮肤 ⑩ 像素 Pixel（还原 showcase 方案⑩：8-bit 心形 + 方块血槽，on/off 由 JS 生成）
window.SKINS = window.SKINS || {};

window.SKINS['pixel'] = {
  name: '⑩ 像素 Pixel',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [316, 150], compact: [316, 98] },

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

    // 8-bit 的语言是"一格一格跳"，不是平滑跑：心与方块由阶梯缓动驱动，
    // 逐颗 / 逐格点亮熄灭（硬切、无亚像素位移 —— 像素做平滑运动就不是像素了）。
    // 数字同步走阶梯，并闪两下（blip）代替位移：老主机的计数器语言
    const hpEl = q('.p-hp');
    HP.tween(el, 'px5', rem5, (v) => {
      this.setHearts(el, v);
      this.setBlocks(el, v);
      hpEl.textContent = v == null ? '--' : Math.round(v);
    }, { ease: HP.easeStep(5), start: () => { HP.fx(hpEl, 'blip'); HP.fx(q('.p-blocks'), 'blip'); } });

    const cdVal = HP.durMaybe(ok && u.primary ? u.primary.resets_in_seconds : null);
    q('.p-cd').textContent = cdVal ?? '--';
    HP.tween(el, 'pxw', s.show_week === false ? null : remw, (v) => {
      q('.p-wk').textContent = v == null ? '--' : Math.round(v);
    }, { ease: HP.easeStep(4) });

    const show = (sel, on) => { const n = q(sel); if (n) n.style.display = on ? '' : 'none'; };
    show('.p-cdw', s.show_countdown !== false && cdVal != null);
    show('.p-wkw', s.show_week !== false);
  },

  updateMini(el, u, s) {
    const rem = u && u.ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    const cells = el.querySelectorAll('i'), n = el.querySelector('.m-num');
    HP.tween(el, 'm', rem, (v) => {
      const lit = v == null ? 0 : Math.ceil(v / 25);   // 4 格，每格 25%
      cells.forEach((b, i) => b.classList.toggle('on', i < lit));
      n.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { ease: HP.easeStep(4), start: () => HP.fx(n, 'blip') });
  },
};
