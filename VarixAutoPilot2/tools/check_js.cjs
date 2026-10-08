// app.js 死代码扫描。
//
// ★★ 为什么这里没有括号配平检查 ★★
// 第一版自己写了个扫描器，结果**三次误报**，都不是代码的错：
//   误报一：正则字面量里的引号。`.replace(/[&<>"']/g, ...)` 中的 `'`
//           被当成"单引号字符串开始"，一路错到文件尾。
//   误报二：模板串里的 `${}`。`` `{{${key}}}` `` 中第一个 `{{` 是字面量、
//           第二个才是插值，朴素扫描必混淆。
//   误报三：strCh 声明在模块级而赋值在函数内 ⇒ 改的是另一个绑定。
//
// 期间用 `node --check src/app.js` 判定：**退出码 0，代码完全正确**。
//
// **结论：语法正确性交给权威工具 `node --check`（它用 V8 真正的解析器）。**
// 自己的检查器只做它擅长且不会误报的事：扫描"定义了却从不调用"的死函数。
//
// ★ 教训 ★：**校验器自己也必须被校验。谎报通过的校验器比没有更危险**——
//   它会让人以为验过了。本项目这次连续误报三次就是证明。

const fs = require('fs');
const path = process.argv[2] || 'src/app.js';
const src = fs.readFileSync(path, 'utf8');

/** 扫死函数：只有定义、全文无调用。保守策略，宁可漏报不谎报。 */
function findDead(s) {
  const names = new Set();
  // function name(  /  async function name(
  for (const m of s.matchAll(/(?:^|\n)(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(/g)) names.add(m[1]);
  // const name = (...) =>  /  const name = async (...) =>
  // ★ 名字必须以字母/下划线开头 ★
  // 早先用 [A-Za-z_$] 会把 `const $ = (id) => ...` 也算进去，
  // 而调用形态是 $('id') —— 正则 \b 在 `$` 前不成立 ⇒ 误判"死代码"。
  // 查 `node --check` 是通过的这个事实，才确认是检查器的错。
  for (const m of s.matchAll(/(?:^|\n)const\s+([A-Za-z_][\w$]*)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>/g)) names.add(m[1]);
  // const name = function(
  for (const m of s.matchAll(/(?:^|\n)const\s+([A-Za-z_$][\w$]*)\s*=\s*function\b/g)) names.add(m[1]);

  const dead = [];
  // ★ HTML 里的 onclick="fn()" 也是调用点 ★
  // 漏了它会把所有按钮回调误判成死函数（本轮 8 个全是误报）。
  const fsx = require('fs');
  const pthx = require('path');
  const dir = pthx.dirname(path);
  let inHtml = new Set();
  try {
    const html = fsx
      .readdirSync(dir)
      .filter((f) => f.endsWith('.html'))
      .map((f) => fsx.readFileSync(pthx.join(dir, f), 'utf8'))
      .join('\n');
    inHtml = new Set(
      [...html.matchAll(/onclick="?\s*([A-Za-z_$][\w$]*)\s*\(/g)].map((m) => m[1])
    );
  } catch {
    /* 读不到 HTML 就退化成只看 JS 内部（可能误报，但不会漏报真死代码） */
  }
  for (const name of names) {
    if (inHtml.has(name)) continue;          // 被 onclick 调用 ⇒ 活的
    const esc = name.replace(/\$/g, '\\$');
    // 全文出现次数 <= 1 ⇒ 只有定义，没人调用。
    // （名字若出现在字符串/注释里会多算 1 → 保守不报）
    const hits = (s.match(new RegExp(`\\b${esc}\\b`, 'g')) || []).length;
    if (hits <= 1) dead.push(name);
  }
  return dead;
}

const dead = findDead(src);
console.log(`文件: ${path}  ${src.split('\n').length} 行`);
console.log(`语法: 用 \`node --check ${path}\` 判定（V8 解析器，权威）`);
console.log(dead.length ? '★ 疑似死函数: ' + dead.join(', ') : '死函数: 无');
process.exit(dead.length ? 1 : 0);
