// 皮肤 ① 血条 Hero —— 分段血槽：剩余%=100-used，10 段刻度遮罩，低额呼吸闪烁 + 任务栏迷你形态
window.SKINS = window.SKINS || {};

window.SKINS['hero'] = {
  name: '① 血条 Hero',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [320, 140], compact: [320, 112] },

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
        <span class="win">@@win5h@@<span class="cdw"> · @@reset@@ <b class="cdv">--</b></span></span>
        <span class="nodata">@@no_source@@</span>
        <span class="wk">@@week_left@@ <b class="nw">--%</b></span>
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

    // 血槽与 30px 大数字吃同一个补间值 —— 两者同起同落到，不会"数字先到、条还在爬"。
    // 血槽用 easeSoft（体量感，起步不冲），数字额外叠一记"机械弹跳"：
    // 一次轮询往往只动 1%，条宽那点位移根本看不出来，这记弹跳才是"刚刚刷新了"的信号。
    const b5 = q('.b5'), n5 = q('.n5');
    HP.tween(el, 'hp5', rem5, (v) => {
      b5.style.width = (v == null ? 0 : v) + '%';
      n5.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { ease: HP.easeSoft, start: () => HP.fx(n5, 'pop') });
    n5.classList.toggle('danger-flash', rem5 != null && rem5 <= 20);

    const cdVal = HP.durMaybe(ok ? u.primary.resets_in_seconds : null);
    q('.cdv').textContent = cdVal ?? '--';
    // 周窗口：关掉时传 null（顺带把补间状态复位，重新打开时不会从旧值补一段）
    HP.tween(el, 'hpw', s.show_week === false ? null : remw, (v) => {
      q('.nw').textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { ease: HP.easeSoft });

    // 副行：有数据 -> 5h 窗口/重置/周剩余；无数据 -> 无数据源
    q('.win').style.display = ok ? '' : 'none';
    q('.nodata').style.display = ok ? 'none' : '';
    q('.wk').style.display = (ok && s.show_week !== false) ? '' : 'none';
    q('.cdw').style.display = (s.show_countdown === false || cdVal == null) ? 'none' : '';

    const m = q('.hp-mail'); const txt = HP.mail(u.email);
    m.textContent = txt; m.title = u.email || '';
    m.style.display = (s.show_email === false || !txt) ? 'none' : '';
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.classList.toggle('nodata', rem == null);
    const f = el.querySelector('.m-fill'), n = el.querySelector('.m-num');
    // 挂件窗口只有内容 + 8px，动效一律不位移，只用亮度脉冲（见 common.js HP.FX.charge）
    HP.tween(el, 'm', rem, (v) => {
      f.style.width = (v == null ? 0 : v) + '%';
      n.textContent = v == null ? '--%' : Math.round(v) + '%';
    }, { ease: HP.easeSoft, start: () => HP.fx(n, 'charge') });
  },
};
