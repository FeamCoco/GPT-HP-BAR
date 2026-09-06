// 皮肤 ② 琉璃 Glass —— 毛玻璃胶囊单行（剩余% + 重置倒计时 + 周窗口），闪电颜色=状态色 + 任务栏迷你形态
window.SKINS = window.SKINS || {};

window.SKINS['glass'] = {
  name: '② 琉璃 Glass',
  // 逻辑尺寸（乘以字号缩放后由外壳调 set_window_size）
  sizes: { full: [376, 80], compact: null },

  html: `
    <div class="sk-glass">
      <svg class="bolt" viewBox="0 0 24 24"><path d="M13 2 3 14h7l-1 8 10-12h-7l1-8z"/></svg>
      <span class="num n5">--%</span>
      <span class="sep s1"></span>
      <span class="cd">@@cd_pre@@<b class="cdv">--</b>@@cd_post@@</span>
      <span class="sep s2"></span>
      <span class="week">@@week_w@@ <b class="nw">--%</b></span>
      <span class="hp-chrome-slot" style="display:inline-flex;margin-left:auto"></span>
    </div>`,

  mini: `
    <div class="sk-glass-mini">
      <span class="m-dot"></span><span class="m-num">--%</span>
    </div>`,

  update(el, u, s) {
    const q = (c) => el.querySelector(c);
    const ok = !!u.ok;
    const rem5 = ok ? HP.rem(u.primary.used_percent) : null;
    const remw = ok ? HP.rem(u.secondary.used_percent) : null;
    const danger = HP.applyTone(el, rem5 ?? remw, s.accent);
    el.classList.toggle('danger', danger && rem5 != null && rem5 <= 20);
    el.classList.toggle('nodata', !ok);

    q('.n5').textContent = rem5 == null ? '--%' : Math.round(rem5) + '%';
    q('.cdv').textContent = HP.dur(ok ? u.primary.resets_in_seconds : null);
    if (s.show_week !== false) {
      q('.nw').textContent = remw == null ? '--%' : Math.round(remw) + '%';
    }

    // 开关：倒计时 / 周；分隔线随两侧内容显隐
    const cdOn = s.show_countdown !== false;
    const wkOn = s.show_week !== false;
    q('.cd').style.display = cdOn ? '' : 'none';
    q('.week').style.display = wkOn ? '' : 'none';
    q('.s2').style.display = (cdOn && wkOn) ? '' : 'none';
    q('.s1').style.display = (cdOn || wkOn) ? '' : 'none';
    // 无邮箱元素（设计如此），s.show_email 忽略
  },

  updateMini(el, u, s) {
    const ok = !!u.ok;
    const rem = ok ? HP.rem(u.primary.used_percent) : null;
    HP.applyTone(el, rem, s.accent);
    el.classList.toggle('nodata', rem == null);
    el.querySelector('.m-num').textContent = rem == null ? '--%' : Math.round(rem) + '%';
  },
};
