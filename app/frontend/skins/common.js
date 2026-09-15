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
  // ---- 数值过渡引擎（v0.6）------------------------------------------------
  // 背景：进度变化原来是直接赋值 —— CSS 里那几条 `transition:width .5s` 只在
  // "变化量够大"时看得见；一次轮询（60s）剩余额度通常只动 0~1%，在 120px 宽的
  // 条上不到 1.5px，而数字连这点过渡都没有。观感上就是"数据啪一下换了"。
  // 现在拆成两层：
  //   ① 数值本身由 rAF 补间（条宽 / 液面高度 / 弧长 / 指针角度 / 数字共用同一个
  //      时钟与曲线，不会出现"数字先到位、条还在爬"）；
  //   ② 每次变化再叠一层**该皮肤自己的**一次性动效（HP.FX），让 1% 这种小变化
  //      也有"刚刚更新过"的反馈 —— 这是这套引擎真正的价值所在。
  // 三条纪律：
  //   · 首屏不补间：挂载时直接把值画上去，避免开机看到数字从 0 冲到 87%；
  //   · 无数据（null）不补间：直接落到占位状态，不让 "--%" 参与插值；
  //   · 中途换目标从"当前插值"续跑，而不是跳回上一个目标（连续轮询不打架）。
  MS: 620,          // 数值补间时长；所有皮肤、所有几何量都吃这一个数
  _tw: new WeakMap(), // 每个元素一份补间状态：{ key: { v, raf } }，v 是当前显示值

  // 缓动：三条曲线覆盖全部皮肤（与设计稿 §5.2 的"起步快、收尾缓"一致）
  easeOut: (p) => 1 - Math.pow(1 - p, 3),          // 通用：数字 / 细条
  easeSoft: (p) => 1 - Math.pow(1 - p, 4),         // 更缓：血槽 / 液面（体量感）
  easeBack: (p) => { const c = 1.22, q = p - 1;    // 轻微过冲：指针 / 磁贴回弹
    return 1 + (c + 1) * q * q * q + c * q * q; },
  // 阶梯缓动：把"平滑跑"变成"一格一格跳"（终端 / 像素这类离散语言用）
  easeStep: (n) => (p) => Math.min(1, Math.floor(p * n + 1e-4) / Math.max(1, n - 1)),

  // 系统"减少动态效果"：信息性动效不能删（否则用户就读不到"刚刚变了"），
  // 折中为压缩到 1/3 时长；纯装饰的 HP.fx 在这种情况下直接不播。
  reduced() {
    try { return window.matchMedia('(prefers-reduced-motion: reduce)').matches; }
    catch (e) { return false; }
  },
  ms(base) { return this.reduced() ? Math.max(1, Math.round(base / 3)) : base; },

  // 一次性动效签名表 —— 每套皮肤挑一个，动词必须来自它自己的视觉语言：
  //   pop    仪表/血条族的"机械弹跳"，数字顶一下再落回
  //   slide  琉璃的透感：新值像浮上来一样
  //   jitter CRT / 霓虹刷新时的高压不稳，横向抖 2px 就定住
  //   lift   Fluent 磁贴的"抬起-回弹"
  //   charge 电池隐喻：不位移，只把亮度顶一下（位移会像"接触不良"）
  //   bob    液体：数字随液面下沉再浮起（阻尼感）
  //   blip   8-bit 计数器：闪两下，不做亚像素位移
  //   tick   简报卡：最克制的一档，只有一次极短的明度下探
  FX: {
    pop:    { dur: 480, ease: 'cubic-bezier(.34,1.4,.64,1)',
              kf: [{ transform: 'scale(1.09)' }, { transform: 'scale(1)' }] },
    slide:  { dur: 460, ease: 'cubic-bezier(.22,1,.36,1)',
              kf: [{ transform: 'translateY(5px)', opacity: '0.35' },
                   { transform: 'none', opacity: '1' }] },
    jitter: { dur: 380, ease: 'linear',
              kf: [{ transform: 'translateX(-2px)' }, { transform: 'translateX(2px)' },
                   { transform: 'translateX(-1px)' }, { transform: 'none' }] },
    lift:   { dur: 520, ease: 'cubic-bezier(.34,1.4,.64,1)',
              kf: [{ transform: 'translateY(-3px)' }, { transform: 'none' }] },
    charge: { dur: 620, ease: 'ease-out',
              kf: [{ filter: 'brightness(1.6)' }, { filter: 'brightness(1)' }] },
    bob:    { dur: 620, ease: 'cubic-bezier(.22,1,.36,1)',
              kf: [{ transform: 'translateY(4px) scale(.97)' },
                   { transform: 'translateY(-1px) scale(1.01)' }, { transform: 'none' }] },
    blip:   { dur: 300, ease: 'linear',
              kf: [{ opacity: '0.25' }, { opacity: '1' },
                   { opacity: '0.45' }, { opacity: '1' }] },
    tick:   { dur: 420, ease: 'ease-out',
              kf: [{ opacity: '0.45' }, { opacity: '1' }] },
  },

  /// 数值补间：把 el 上 key 对应的显示值从"当前值"平滑推到 to。
  /// draw(v) 拿到的是**中间浮点值**（null = 无数据），由皮肤决定怎么画：
  /// 条宽直接除、数字取 Math.round、离散量自己量化（心/格/方块）。
  /// opt: { ms, ease, start }  —— start() 在"真正开始补间"时回调一次，
  ///        用来触发该皮肤的一次性动效（值没变就不播，避免无谓的闪动）。
  tween(el, key, to, draw, opt) {
    if (!el || typeof draw !== 'function') return;
    let bag = this._tw.get(el);
    if (!bag) { bag = Object.create(null); this._tw.set(el, bag); }
    let st = bag[key];
    if (!st) { st = bag[key] = { v: null, raf: 0 }; }
    if (st.raf) { cancelAnimationFrame(st.raf); st.raf = 0; }

    const o = opt || {};
    if (to == null) {                       // 无数据：直接落占位态，不插值
      st.v = null; draw(null); return;
    }
    const num = Number(to);
    if (!isFinite(num)) { st.v = null; draw(null); return; }

    if (st.v == null) { st.v = num; draw(num); return; }   // 首屏：直接就位
    const from = st.v;
    if (Math.abs(num - from) < 0.05) { st.v = num; draw(num); return; }

    const dur = this.ms(o.ms || this.MS);
    const ease = o.ease || this.easeOut;
    if (dur <= 1 || document.hidden) { st.v = num; draw(num); return; }
    if (o.start) o.start();

    const t0 = (window.performance || Date).now();
    const step = (now) => {
      let p = (now - t0) / dur;
      if (p > 1) p = 1;
      st.v = from + (num - from) * ease(p);
      draw(st.v);
      if (p < 1) st.raf = requestAnimationFrame(step);
      else { st.raf = 0; st.v = num; draw(num); }
    };
    st.raf = requestAnimationFrame(step);
  },

  /// 一次性动效：用 Web Animations API 播放，而不是加 CSS 类重播 keyframes。
  /// 原因是级联冲突 —— 主数字常同时挂 `.danger-flash`（额度告急的呼吸光，
  /// 带 !important 的 animation），同一个元素只能生效一条 animation 声明，
  /// 走类名就会让"告急时恰好没有变化动效"，而告急恰恰是最该被看见的时刻。
  /// WAAPI 的动画与 CSS animation 在不同属性上可以共存（这条只动
  /// transform/opacity/filter，不与告警的 text-shadow 抢），也不会被
  /// app.css 里 reduce-motion 的 `transition/animation-duration:.001ms!important` 误杀。
  fx(el, name) {
    if (!el || !el.animate || this.reduced()) return;
    const f = this.FX[name];
    if (!f) return;
    try {
      el.animate(f.kf, { duration: this.ms(f.dur), easing: f.ease || 'ease-out' });
    } catch (e) { /* 动效失败绝不影响数值本身 */ }
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
