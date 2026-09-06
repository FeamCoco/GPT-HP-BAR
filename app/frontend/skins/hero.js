// 皮肤 ① 血条 Hero —— 分段血槽：剩余%=100-used，10 段刻度遮罩，低额呼吸闪烁 + 任务栏迷你形态
window.SKINS = window.SKINS || {};

window.SKINS['hero'] = {
  name: '① 血条 Hero',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [320, 120], compact: null },

  html: `
    <div class="sk-hero">
      <div class="head">
        <span class="lab">CODEX · HP</span>
        <span class="hp-mail"></span>
        <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
      </div>
      <div class="num n5">--%</div>
      <div class="track"><i class="b5"></i><span class="ticks"></span></div>
      <div class="sub">
        <span class="win">5h 窗口<span class="cdw"> · 重置 <b class="cdv">--</b></span></span>
        <span class="nodata">无数据源</span>
        <span class="wk">周剩余 <b class="nw">--%</b></span>
      </div>
    </div>`,

  mini: `
    <div class="sk-hero-mini">
      <div class="m-track"><i class="m-fill"></i><span class="m-ticks"></span></div><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    const danger = HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('danger', danger && rem5 != null && rem5 <= 20);
    el.classList.toggle('nodata', !ok);

    q('.b5').style.width = (rem5 == null ? 0 : rem5) + '%';
    q('.n5').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    q('.n5').classList.toggle('danger-flash', rem5 != null && rem5 <= 20);
    q('.cdv').textContent = HP.dur(ok ? u.primary.resets_in_seconds : null);
    if (s.show_week !== false) {
      q('.nw').textContent = remw == null ? '--%' : Math.round(remw) + '%';
    }

    // 副行：有数据 -> 5h 窗口/重置/周剩余；无数据 -> 无数据源
    q('.win').style.display = ok ? '' : 'none';
    q('.nodata').style.display = ok ? 'none' : '';
    q('.wk').style.display = (ok && s.show_week !== false) ? '' : 'none';
    q('.cdw').style.display = s.show_countdown === false ? 'none' : '';

    const m = q('.hp-mail'); const txt = HP.mail(u.email);
    m.textContent = txt; m.title = u.email || '';
    m.style.display = (s.show_email === false || !txt) ? 'none' : '';
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.classList.toggle('nodata', rem == null);
    el.querySelector('.m-fill').style.width = (rem == null ? 0 : rem) + '%';
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
