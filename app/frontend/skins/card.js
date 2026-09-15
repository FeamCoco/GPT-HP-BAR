// 皮肤 ④ 简报 Card（范例实现——其余皮肤照此契约）
window.SKINS = window.SKINS || {};

window.SKINS['card'] = {
  name: '④ 简报 Card',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [280, 212], compact: [248, 104] },

  html: `
    <div class="card sk-card">
      <div class="head">
        <span class="title">Codex<em class="hp-plan">--</em></span>
        <span class="mail hp-mail"></span>
        <span class="hp-chrome-slot" style="display:flex;margin-left:auto"></span>
      </div>
      <div class="row"><span class="k">@@win5h@@</span><div class="bar"><i class="b5"></i></div><b class="n5">--%</b></div>
      <div class="row wk-row extra"><span class="k">@@week_label@@</span><div class="bar"><i class="bwk wk"></i></div><b class="nw">--%</b></div>
      <div class="row cr cr-row extra"><span class="k">credits</span><div class="sp"></div><b class="nc">--</b></div>
      <div class="foot extra"><span class="dot"></span><span class="src"></span><span class="cd">@@reset@@ <b class="cdv">--</b></span></div>
    </div>`,

  mini: `
    <div class="sk-card-mini">
      <div class="m-track"><i class="m-fill"></i></div><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    const danger = HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('danger', danger);

    const show = (sel, on) => { const n = q(sel); if (n) n.style.display = on ? '' : 'none'; };
    show('.wk-row', s.show_week !== false);
    show('.cr-row', s.show_credits !== false);
    show('.foot', s.show_countdown !== false || true); // foot 还承载状态点，仅关倒计时
    const cdVal = HP.durMaybe(ok ? u.primary.resets_in_seconds : null);
    const cd = q('.cd');
    if (cd) cd.style.display = (s.show_countdown === false || cdVal == null) ? 'none' : '';

    // 简报卡是十套里最克制的一档（业务阅读场景，动效不该抢戏）：
    // 条与数字共用补间，数字只叠一次极短的明度下探（tick），既提示"刷新过"
    // 又不会让人在读数字时觉得字在跳
    const b5 = q('.b5'), n5 = q('.n5');
    HP.tween(el, 'c5', rem5, (v) => {
      b5.style.width = (v == null ? 0 : v) + '%';
      n5.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { start: () => HP.fx(n5, 'tick') });
    n5.classList.toggle('danger-flash', rem5 != null && rem5 <= 20);
    HP.tween(el, 'cw', s.show_week === false ? null : remw, (v) => {
      q('.bwk').style.width = (v == null ? 0 : v) + '%';
      q('.nw').textContent = v == null ? '--%' : Math.round(v) + '%';
    });
    q('.hp-plan').textContent = (u.plan || '--').toUpperCase();
    const m = q('.hp-mail'); m.textContent = HP.mail(u.email); m.title = u.email || '';
    if (s.show_email === false) m.style.display = 'none'; else m.style.display = '';
    const cr = HP.credits(u.credits); q('.nc').textContent = cr || '--';
    q('.cdv').textContent = cdVal ?? '--';

    const dot = q('.dot'), src = q('.src');
    if (ok) { dot.className = 'dot ok'; src.textContent = u.source; src.title = ''; }
    else { dot.className = 'dot bad'; src.textContent = HP.t('no_source'); src.title = u.error || ''; }
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    const f = el.querySelector('.m-fill'), n = el.querySelector('.m-num');
    HP.tween(el, 'm', rem, (v) => {
      f.style.width = (v == null ? 0 : v) + '%';
      n.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { start: () => HP.fx(n, 'tick') });
  },
};
