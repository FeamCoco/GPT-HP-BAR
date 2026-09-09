// 皮肤 ③ 终端 Term —— htop 式等宽：JS 生成 10 格 █░ 进度、双行双窗口、credits 行可关 + 任务栏迷你形态
window.SKINS = window.SKINS || {};

window.SKINS['term'] = {
  name: '③ 终端 Term',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [340, 150], compact: null },

  html: `
    <div class="sk-term">
      <div class="title">GPT-HP-BAR <span class="sub">v0.1 · codex@<span class="plan">--</span></span><span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span></div>
      <div class="line"><span class="k">5h</span><span class="blocks b5"><span class="off">░░░░░░░░░░</span></span><span class="pct n5">--%</span></div>
      <div class="line wk-row"><span class="k">week</span><span class="blocks bwk"><span class="off">░░░░░░░░░░</span></span><span class="pct nw">--%</span></div>
      <div class="line dim"><span class="err">@@no_source@@ · </span><span class="cdw">reset <b class="cdv">--</b></span><span class="sep1"> · </span><span class="cr-txt">credits <b class="nc">--</b></span><span class="cursor">▌</span></div>
    </div>`,

  mini: `
    <div class="sk-term-mini"><span class="br">[</span><span class="m-num">--%</span><span class="br">]</span></div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    const danger = HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('danger', danger && rem5 != null && rem5 <= 20);
    el.classList.toggle('nodata', !ok);

    // 10 格 █░ 进度块：filled = round(剩余%/10)
    const blocks = (rem) => {
      const filled = rem == null ? 0 : Math.max(0, Math.min(10, Math.round(rem / 10)));
      return '<span class="on">' + '█'.repeat(filled) + '</span><span class="off">' + '░'.repeat(10 - filled) + '</span>';
    };
    q('.b5').innerHTML = blocks(rem5);
    q('.n5').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    if (s.show_week !== false) {
      q('.bwk').innerHTML = blocks(remw);
      q('.nw').textContent = remw == null ? '--%' : Math.round(remw) + '%';
    }

    // 标题行 plan / 尾行 开关（reset · credits · 无数据源），分隔符随两侧显隐
    q('.plan').textContent = ok ? String(u.plan || '--').toLowerCase() : '--';
    q('.err').style.display = ok ? 'none' : '';
    q('.wk-row').style.display = s.show_week === false ? 'none' : '';
    const cdVal = HP.durMaybe(ok ? u.primary.resets_in_seconds : null);
    const cdOn = s.show_countdown !== false && cdVal != null;
    const crOn = s.show_credits !== false;
    q('.cdw').style.display = cdOn ? '' : 'none';
    q('.cr-txt').style.display = crOn ? '' : 'none';
    q('.sep1').style.display = (cdOn && crOn) ? '' : 'none';
    q('.cdv').textContent = cdVal ?? '--';
    const cr = HP.credits(u.credits);
    q('.nc').textContent = cr || '--';
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.classList.toggle('nodata', rem == null);
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
