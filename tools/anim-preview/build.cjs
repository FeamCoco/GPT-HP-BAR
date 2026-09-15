#!/usr/bin/env node
/* ============================================================================
 * 生成动效预览页（自包含单文件）
 *
 * 把 page.src.html 里两个标记替换成**真实的运行时前端**（app.css + 十套
 * skin css + i18n/common + 十套 skin js），产出一个不依赖任何外部文件的 HTML。
 *
 * 为什么要内联：
 *   1) 预览面板/在线预览是从 /static-html/<hash>/ 下提供服务的，模板里写
 *      `../app/frontend/...` 这种相对路径会全部 404 —— CSS/JS 一个都加载不上，
 *      页面看起来就是"什么都没有，更不会有动效"（踩过一次）；
 *   2) 内联后这个文件可以双击用浏览器打开，也能直接发给别人看动效。
 *
 * 用法：
 *   node tools/anim-preview/build.cjs               # 输出到 design/anim-preview.html
 *   node tools/anim-preview/build.cjs <输出路径>     # 自定义输出
 * 注意：前端改了（配色 / 皮肤 / 动效参数）要重新跑一次，否则预览是旧的。
 *      预览页顶部自检条会显示内联进来的动效签名单数量，可用来对照是否过期。
 * ========================================================================== */
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..', '..');
const FRONT = path.join(ROOT, 'app', 'frontend');
const SKINS = ['hero', 'glass', 'term', 'card', 'tile', 'battery', 'gauge', 'neon', 'liquid', 'pixel'];

const read = (p) => fs.readFileSync(path.join(FRONT, p), 'utf8');
// 内联进 <script> 时必须挡住 "</script"，否则 HTML 解析器会提前收尾
const safeJs = (s) => s.replace(/<\/script/gi, '<\\/script');

const skinCss = [read('app.css')]
  .concat(SKINS.map((s) => read('skins/' + s + '.css')))
  .join('\n\n');
const runtimeJs = [read('i18n.js'), read('skins/common.js')]
  .concat(SKINS.map((s) => read('skins/' + s + '.js')))
  .join('\n;\n');

const srcPath = path.join(__dirname, 'page.src.html');
const src = fs.readFileSync(srcPath, 'utf8');
for (const marker of ['@@SKIN_CSS@@', '@@RUNTIME_JS@@']) {
  if (src.indexOf(marker) < 0) throw new Error('模板 ' + srcPath + ' 里找不到标记 ' + marker);
}

const out = src
  .replace('/* @@SKIN_CSS@@ */', () => skinCss)      // 用函数形式，避免 $& 之类被当成替换占位符
  .replace('/* @@RUNTIME_JS@@ */', () => safeJs(runtimeJs));

const target = process.argv[2] || path.join(ROOT, 'design', 'anim-preview.html');
fs.mkdirSync(path.dirname(target), { recursive: true });
fs.writeFileSync(target, out, 'utf8');
console.log('已生成 ' + target + ' （' + Math.round(out.length / 1024) + ' KB，'
  + SKINS.length + ' 套皮肤 + ' + (SKINS.length + 2) + ' 个运行时脚本，全部内联）');
