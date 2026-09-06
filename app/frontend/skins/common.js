// 皮肤公共辅助：所有皮肤共享的数据换算与状态色
window.HP = {
  // 已用% -> 剩余%
  rem(used) { return used == null ? null : Math.max(0, 100 - used); },
  // 秒 -> "Xh Ym"
  dur(sec) {
    if (sec == null || sec < 0) return '--';
    const m = Math.max(0, Math.round(sec / 60));
    return Math.floor(m / 60) + 'h ' + String(m % 60).padStart(2, '0') + 'm';
  },
  // credits 对象 -> 显示文本
  credits(c) {
    if (!c) return null;
    if (c.unlimited === true) return HP.t('unlimited');
    const bal = c.balance ?? c.credits_balance;
    if (bal == null) return null;
    const n = Number(bal);
    return isNaN(n) ? String(bal) : '$' + n.toFixed(2);
  },
  // 剩余% + accent 设置 -> [色A, 色B, danger]
  tone(rem, accent) {
    if (accent && accent !== 'auto') {
      const fixed = { green: ['#6ee7b7', '#10b981'], amber: ['#fde68a', '#f59e0b'], cyan: ['#7dd3fc', '#0ea5e9'] }[accent];
      if (fixed) return [fixed[0], fixed[1], false];
    }
    if (rem == null) return ['#6ee7b7', '#10b981', false];
    if (rem <= 20) return ['#fca5a5', '#ef4444', true];
    if (rem <= 50) return ['#fde68a', '#f59e0b', false];
    return ['#6ee7b7', '#10b981', false];
  },
  // 把状态色写到元素 CSS 变量上（皮肤 CSS 用 var(--tone-a)/var(--tone-b) 取色）
  applyTone(el, rem, accent) {
    const [a, b, danger] = this.tone(rem, accent);
    el.style.setProperty('--tone-a', a);
    el.style.setProperty('--tone-b', b);
    return danger;
  },
  // 邮箱脱敏
  mail(email) {
    if (!email) return '';
    return email.length > 18 ? email.slice(0, 17) + '…' : email;
  },
};

// 界面语言胶水（字典与实现在 i18n.js，最先加载）
HP.lang = window.I18N_LANG || 'zh';
HP.setLang = (l) => { HP.lang = l === 'en' ? 'en' : 'zh'; window.I18N_LANG = HP.lang; };
HP.t = (k) => window.i18nT(k);
HP.i = (s) => window.i18nFill(s);

// 皮肤注册表：各皮肤文件向这里注册
window.SKINS = window.SKINS || {};
