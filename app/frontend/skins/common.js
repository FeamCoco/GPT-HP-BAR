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
  // 秒 -> "Xh Ym"；无有效数据返回 null（皮肤据此隐藏倒计时片段，而不是显示 "--"）
  durMaybe(sec) {
    return sec == null || sec < 0 ? null : HP.dur(sec);
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
  // 主题色表（设计稿 design/theme-v2.css §3）：6 个同亮度档位的色相，各含两档
  //   ok  = 充足（>50%）  warn = 偏低（21–50%）
  //   告急（≤20%）不在这里 —— 它是强制的告警红，见 tone()
  // 键名 green 是旧设置值的别名（保留以兼容老配置文件）
  ACCENTS: {
    mint:   { ok: ['#6ee7b7', '#10b981'], warn: ['#fde68a', '#f59e0b'] },
    cyan:   { ok: ['#7dd3fc', '#0ea5e9'], warn: ['#fde68a', '#f59e0b'] },
    sky:    { ok: ['#93c5fd', '#3b82f6'], warn: ['#fde68a', '#f59e0b'] },
    violet: { ok: ['#c4b5fd', '#8b5cf6'], warn: ['#fde68a', '#f59e0b'] },
    amber:  { ok: ['#fde68a', '#f59e0b'], warn: ['#fbbf24', '#d97706'] },
    rose:   { ok: ['#fda4af', '#f43f5e'], warn: ['#fde68a', '#f59e0b'] },
    green:  { ok: ['#6ee7b7', '#10b981'], warn: ['#fde68a', '#f59e0b'] }, // 旧值别名 = mint
  },
  // 剩余% + accent 设置 -> [色A, 色B, danger]
  tone(rem, accent) {
    // 告急优先：无论选了什么主题色，剩余 ≤20% 一律切告警红 ——
    // 额度告急是安全信息，不该被偏好色盖住。
    // （旧实现里只要 accent != auto 就直接返回固定色且 danger 恒为 false，
    //   选了"固定青"之后额度剩 8% 也是青的，这里一并修掉。）
    if (rem != null && rem <= 20) return ['#fca5a5', '#ef4444', true];
    const fixed = accent && accent !== 'auto' ? HP.ACCENTS[accent] : null;
    if (fixed) {
      const c = rem != null && rem <= 50 ? fixed.warn : fixed.ok;
      return [c[0], c[1], false];
    }
    if (rem != null && rem <= 50) return ['#fde68a', '#f59e0b', false];
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
