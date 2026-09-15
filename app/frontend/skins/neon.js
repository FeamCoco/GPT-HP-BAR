// 皮肤 ⑧ 霓虹 Neon（还原 showcase 方案⑧：扫光血槽 + 渐变发光大数字 + 扫描线）
window.SKINS = window.SKINS || {};

window.SKINS['neon'] = {
  name: '⑧ 霓虹 Neon',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [320, 140], compact: [320, 92] },

  html: `
    <div class="neon sk-neon">
      <div class="n-top">
        <span class="n-lab">CODEX // HP</span>
        <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
        <span class="n-num">--%</span>
      </div>
      <div class="n-track"><i class="n-fill"></i></div>
      <div class="n-info"><span class="n-mail hp-mail"></span><span class="n-cr">CREDITS <b class="n-crv">--</b></span></div>
      <div class="n-sub">
        <span class="n-cdw">RESET <b class="n-cd">--</b></span>
        <span class="n-wkw">WEEK <b class="n-wk">--%</b></span>
        <span>PLAN <b class="n-plan">--</b></span>
      </div>
    </div>`,

  mini: `
    <div class="sk-neon-mini"><span class="m-lab">HP</span><span class="m-num">--%</span></div>`,

  // accent=auto 时保持品牌青紫渐变（HP.applyTone 的三档绿被覆盖）；
  // 低额度(≤20%)时 applyTone 给出红色变量且返回 danger=true，边框/辉光随之转红
  tone(el, rem, s) {
    const danger = HP.applyTone(el, rem, s.accent);
    if ((!s.accent || s.accent === 'auto') && !danger) {
      el.style.setProperty('--tone-a', '#22d3ee');
      el.style.setProperty('--tone-b', '#a78bfa');
    }
    return danger;
  },

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    const remw = ok && u.secondary ? HP.rem(u.secondary.used_percent) : null;
    const danger = this.tone(el, rem5 ?? remw, s);
    el.classList.toggle('danger', danger);
    el.classList.toggle('nodata', !ok);

    // 显示开关
    const show = (sel, on) => { const n = q(sel); if (n) n.style.display = on ? '' : 'none'; };
    show('.n-info', s.show_email !== false || s.show_credits !== false);
    show('.n-mail', s.show_email !== false);
    show('.n-cr', s.show_credits !== false);
    const cdVal = HP.durMaybe(ok && u.primary ? u.primary.resets_in_seconds : null);
    show('.n-cdw', s.show_countdown !== false && cdVal != null);
    show('.n-wkw', s.show_week !== false);

    // 赛博语言：血槽扫光条在变化瞬间亮一下（charge，只动亮度不动位置），
    // 大数字横向抖一下（jitter）—— CRT 刷新时高压不稳的那两帧，
    // 与"永远在跑的扫光"形成"常态流动 + 事件闪烁"两层节奏
    const num = q('.n-num'), nf = q('.n-fill');
    HP.tween(el, 'n5', rem5, (v) => {
      num.textContent = v == null ? '--%' : Math.round(v) + '%';
      nf.style.width = (v == null ? 0 : v) + '%';
    }, { start: () => { HP.fx(num, 'jitter'); HP.fx(nf, 'charge'); } });
    q('.n-cd').textContent = cdVal ?? '--';
    HP.tween(el, 'nw', s.show_week === false ? null : remw, (v) => {
      q('.n-wk').textContent = v == null ? '--%' : Math.round(v) + '%';
    });
    q('.n-plan').textContent = ok ? (u.plan || '--').toUpperCase() : '--';
    q('.n-crv').textContent = HP.credits(ok ? u.credits : null) || '--';
    const m = q('.n-mail');
    m.textContent = HP.mail(ok ? u.email : '');
    m.title = (ok && u.email) || '';
  },

  updateMini(el, u, s) {
    const rem = u && u.ok && u.primary ? HP.rem(u.primary.used_percent) : null;
    this.tone(el, rem, s);
    const n = el.querySelector('.m-num');
    HP.tween(el, 'm', rem, (v) => {
      n.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { start: () => HP.fx(n, 'jitter') });
  },
};
