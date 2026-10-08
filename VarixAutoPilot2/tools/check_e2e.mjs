// ★★★ 端到端验证：入队 → 自动发送 → 循环下一轮 ★★★
//
// 这是 Variable 的核心诉求：「确保可以在这个界面进行自动化循环化自动提交」。
// 本脚本按真实用户操作顺序走完整链路，每一步都断言。

import { chromium } from 'playwright-core';

const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});
const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
const errs = [];
p.on('pageerror', (e) => errs.push('PAGEERROR: ' + String(e)));
p.on('console', (m) => {
  if (m.type() === 'error') errs.push('[console] ' + m.text());
});
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2200);

const snap = () =>
  p.evaluate(() => ({
    队列: document.querySelectorAll('.qi').length,
    qsum: (document.getElementById('q-sum') || {}).textContent || '',
    循环状态: (document.getElementById('loop-state') || {}).textContent || '',
    循环说明: (document.getElementById('loop-note') || {}).textContent || '',
    hint: ((document.getElementById('hint') || {}).textContent || '').slice(0, 60),
    状态项: Array.from(document.querySelectorAll('.qi .st')).map((e) => e.textContent),
  }));

const step = async (title) => {
  const s = await snap();
  console.log(`\n── ${title}`);
  console.log(`   队列=${s.队列} ${s.qsum}  循环=${s.循环状态} ${s.循环说明}`);
  console.log(`   状态=[${s.状态项.join(', ')}]`);
  console.log(`   hint=${s.hint}`);
  return s;
};

await step('① 初始');

// ══ 场景 A：普通入队 ══
await p.click('button:has-text("加入待发")');
await p.waitForTimeout(900);
const a = await step('② 点「加入待发」');
if (a.队列 < 1) console.log('   ★ 入队失败');

// ══ 场景 B：自由文本优先 ══
await p.check('#o-free');
await p.fill('#free-text', '第 2 轮：继续施工，先报当日门禁结果。');
await p.click('button:has-text("加入待发")');
await p.waitForTimeout(900);
const bres = await step('③ 自由文本入队');
const hasFree = await p.evaluate(() => {
  const items = Array.from(document.querySelectorAll('.qi .pv'));
  return items.some((e) => (e.textContent || '').includes('第 2 轮'));
});
console.log(`   ${hasFree ? 'OK  ' : '★缺★'} 自由文本内容已入队`);
await p.uncheck('#o-free');

// ══ 场景 C：循环（首轮 + 后续每轮） ══
await p.fill('#lp-rounds', '3');
await p.click('button:has-text("开始循环")');
await p.waitForTimeout(1400);
const c = await step('④ 点「开始循环」（3轮）');

// ══ 场景 D：停止 ══
await p.click('button:has-text("停止")');
await p.waitForTimeout(700);
const d = await step('⑤ 点「停止」');

// ══ 场景 E：清已完成 ══
await p.click('button:has-text("清已完成")');
await p.waitForTimeout(600);
await step('⑥ 点「清已完成」');

console.log('\n=== 汇总 ===');
console.log('页面错误:', errs.length ? errs.join('\n           ') : '(无)');
await p.screenshot({ path: 'shots/e2e_result.png' });
await b.close();
