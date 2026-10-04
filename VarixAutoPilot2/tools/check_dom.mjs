// DOM 结构自检：确保 buildConvCard 建出的节点真的挂上了内容容器。
//
// ★ 为什么需要它 ★
// 本轮踩过一次：重写增量更新时，创建了 s1/s2 两个 span 却忘了
// `meta.append(s1, s2)`，于是卡片只有标题、「模型/目录」两行整个消失。
// **静默无错**——不抛异常、不打日志、界面只是"少了点东西"。
// 这类"创建了却没挂上"的错误，静态检查抓不到，只有实测能抓。
// 但实测抓到了就说明闸门该常驻。
//
// 做法：在浏览器里跑 buildConvCard 造一张卡，然后断言关键子节点存在。
// 依赖 Playwright（仓库根已有 playwright-core + 系统 Chrome）。

import { chromium } from 'playwright-core';

const URL = process.argv[2] || 'http://127.0.0.1:8791/index.html';
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

const b = await chromium.launch({ headless: true, executablePath: CHROME });
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
p.on('pageerror', (e) => errors.push(String(e)));
p.on('console', (m) => {
  // ★ 过滤 favicon 404 ★：浏览器总会自动请求它，那是开发服务器的
  // 正常噪声，不该让结构自检失败。真实错误仍然会报出来。
  if (m.type() !== 'error') return;
  if (/favicon/i.test(m.text())) return;
  // 「Failed to load resource: 404」在 Playwright 里只给文字、不给URL，
  // 定位不到是哪个资源。而实测把 response 监听挂上后**没有**任何 4xx ——
  // 说明它就是浏览器自动要的 favicon（发生在response 事件之前/之外）。
  // 判据：只忽略这一条形态的资源加载失败；其他一律报。
  if (/Failed to load resource/.test(m.text())) return;
  errors.push('console: ' + m.text());
});

await p.goto(URL);
await p.waitForTimeout(1500);

const r = await p.evaluate(() => {
  // 取一张真实卡片（mock 模式下会有 7 张）
  const card = document.querySelector('.conv');
  if (!card) return { 错: '没有任何 .conv 卡片（mock 未渲染？）' };
  const meta = card.querySelector('.meta');
  const t = card.querySelector('.t');
  const txt = card.querySelector('.t .txt');
  const rt = card.querySelector('.rt');
  // ★ hidden 属性是否真被 CSS 尊重★
  // 坑：`.tag{display:inline-block}` 会覆盖 UA 的 [hidden]{display:none}，
  // 于是 hidden=true 也照样显示（实测 7 张卡片全亮「当前」）。
  const allTags = Array.from(document.querySelectorAll('.tag'));
  const selCards = document.querySelectorAll('.conv.sel').length;
  return {
    有meta: !!meta,
    meta子节点数: meta ? meta.children.length : -1,
    meta文本长度: meta ? (meta.textContent || '').trim().length : -1,
    meta前60字: meta ? (meta.textContent || '').trim().slice(0, 60) : '',
    有rt: !!rt,
    有txt: !!txt,
    txt宽度: txt ? txt.getBoundingClientRect().width : -1,
    t宽度: t ? t.getBoundingClientRect().width : -1,
    标题文本: txt ? (txt.textContent || '').slice(0, 30) : '',
    // 可见的「当前」标签数（应等于选中卡片数，且只有 1）
    可见当前标签: allTags.filter(
      (x) => x.textContent === '当前' && x.getBoundingClientRect().width > 0
    ).length,
    选中卡片数: selCards,
  };
});

const fails = [];
if (r.错) fails.push(r.错);
if (!r.有meta) fails.push('卡片缺少 .meta 容器');
if (r.meta子节点数 < 2) fails.push(`.meta 只有 ${r.meta子节点数} 个子节点（应>=2：模型行 + 目录行）——★ 忘了 meta.append(s1,s2)？`);
if (r.meta文本长度 <= 0) fails.push('.meta 内容为空（模型/目录两行没渲染出来）');
if (!r.有rt) fails.push('卡片缺少 .rt（相对时间）');
if (!r.有txt) fails.push('卡片缺少 .t .txt（★ 标题文本需独立盒子，否则省略号不生效）');
// ★ hidden 是否真被尊重★
if (r.选中卡片数 > 0 && r.可见当前标签 > r.选中卡片数) {
  fails.push(`可见「当前」标签 ${r.可见当前标签} 个，但只有 ${r.选中卡片数} 张卡片被选中`
    + ' —— ★ `hidden` 属性被 CSS display 覆盖了？需要 `[hidden]{display:none!important}`');
}
if (r.选中卡片数 > 1) {
  fails.push(`有 ${r.选中卡片数} 张卡片同时被标为当前（应恰好 1 张）`);
}
// .txt 应「至少占满 .t 减去标签的宽度」——有「当前」标签时必然窄一点，
// 不能简单判相等。判据：.txt 宽度应 > .t 宽度的 60%，
// 太小说明 flex:1 没生效（会退回按内容收缩 ⇒ 省略号不触发）。
const ratio = r.t宽度 > 0 ? r.txt宽度 / r.t宽度 : 1;
if (r.txt宽度 > 0 && r.t宽度 > 0 && ratio < 0.6) {
  fails.push('.txt 只占 .t 的 ' + Math.round(ratio * 100) + '%（'
    + r.txt宽度 + '/' + r.t宽度 + '）—— flex:1 未生效，省略号不会触发');
}
if (errors.length) fails.push('页面报错: ' + errors.join(' | '));

await b.close();

console.log(JSON.stringify(r, null, 1));
console.log('');
if (fails.length) {
  console.log('★ DOM 结构自检失败:');
  fails.forEach((x) => console.log('   - ' + x));
  process.exit(1);
}
console.log('DOM 结构自检: 全部通过');
