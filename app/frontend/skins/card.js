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
      <div class="row"><span class="k">5h 窗口</span><div class="bar"><i class="b5"></i></div><b class="n5">--%</b></div>
      <div class="row wk-row extra"><span class="k">本周</span><div class="bar"><i class="bwk wk"></i></div><b class="nw">--%</b></div>
      <div class="row cr cr-row extra"><span class="k">credits</span><div class="sp"></div><b class="nc">--</b></div>
      <div class="foot extra"><span class="dot"></span><span class="src"></span><span class="cd">重置 <b class="cdv">--</b></span></div>
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
    const cd = q('.cd'); if (cd) cd.style.display = s.show_countdown === false ? 'none' : '';

    q('.b5').style.width = (rem5 == null ? 0 : rem5) + '%';
    q('.n5').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    q('.n5').classList.toggle('danger-flash', rem5 != null && rem5 <= 20);
    if (s.show_week !== false) {
      q('.bwk').style.width = (remw == null ? 0 : remw) + '%';
      q('.nw').textContent = remw == null ? '--%' : Math.round(remw) + '%';
    }
    q('.hp-plan').textContent = (u.plan || '--').toUpperCase();
    const m = q('.hp-mail'); m.textContent = HP.mail(u.email); m.title = u.email || '';
    if (s.show_email === false) m.style.display = 'none'; else m.style.display = '';
    const cr = HP.credits(u.credits); q('.nc').textContent = cr || '--';
    q('.cdv').textContent = HP.dur(ok ? u.primary.resets_in_seconds : null);

    const dot = q('.dot'), src = q('.src');
    if (ok) { dot.className = 'dot ok'; src.textContent = u.source; src.title = ''; }
    else { dot.className = 'dot bad'; src.textContent = '无数据源'; src.title = u.error || ''; }
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.querySelector('.m-fill').style.width = (rem == null ? 0 : rem) + '%';
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
