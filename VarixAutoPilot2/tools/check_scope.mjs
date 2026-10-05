// ★ 真机验证：loop_ui 的 12 个回调能否被内联 onclick 解析到 ★
//
// 要验证的核心事实：HTML 的 onclick="enqueueOne()" 里，
// 裸标识符 enqueueOne 会去**全局作用域链**找——
// 而不是 window.VAPUI.enqueueOne。
//
// 做法：加载真页面 → 逐个查 typeof → 再真实点一次按钮看效果。

import { chromium } from 'playwright-core';

const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});
const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
const errs = [];
p.on('pageerror', (e) => errs.push(String(e)));
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2200);

// ① 12 个回调在裸全局 vs window.VAPUI 上分别是什么
const r = await p.evaluate(() => {
  const names = ['pickConv', 'enqueueOne', 'renderQueue', 'refreshQueue',
    'clearQueue', 'switchLoopTab', 'loopTexts', 'startLoop', 'stopLoop',
    'readMd', 'probeSkills', 'appendSkill',
    // app.js 那几个（作为对照：它们是能用的）
    'doPreview', 'doSend', 'render', 'tick'];
  const bare = {};
  const viaVapui = {};
  const viaWin = {};
  for (const n of names) {
    // eslint-disable-next-line no-new-func
    bare[n] = (() => { try { return typeof eval(n); } catch (e) { return 'throw'; } })();
    viaWin[n] = typeof window[n];
    viaVapui[n] = window.VAPUI ? typeof window.VAPUI[n] : '(无VAPUI)';
  }
  return { bare, viaWin, viaVapui, VAPUI存在: !!window.VAPUI };
});

console.log('=== 裸全局（onclick 实际查找的位置）===');
for (const [k, v] of Object.entries(r.bare)) {
  const mark = v === 'function' ? 'OK  ' : '★缺★';
  console.log(`  ${mark} ${k.padEnd(16)} = ${v}`);
}
console.log('\n=== window.X（直接属性访问）===');
for (const [k, v] of Object.entries(r.viaWin)) {
  const mark = v === 'function' ? 'OK  ' : '★缺★';
  console.log(`  ${mark} ${k.padEnd(16)} = ${v}`);
}
console.log('\nwindow.VAPUI 存在:', r.VAPUI存在);

// ② 真点一次「加入待发」，看 onclick 是否真的绑上了
const clicked = await p.evaluate(() => {
  const btn = Array.from(document.querySelectorAll('button'))
    .find((x) => (x.innerText || '').trim() === '加入待发');
  if (!btn) return { found: false };
  const attr = btn.getAttribute('onclick');
  // 直接调 onclick 属性里写的那个裸名字，看能不能解析
  let resolved = 'throw';
  try { resolved = typeof eval(attr.replace(/\(\)$/, '')); } catch (e) { resolved = 'throw'; }
  return { found: true, onclick属性: attr, 该名字解析为: resolved };
});
console.log('\n=== onclick 属性解析 ===');
console.log(' ', JSON.stringify(clicked, null, 1).replace(/\n/g, '\n  '));

console.log('\n页面错误:', errs.length ? errs.join(' | ') : '(无)');
await b.close();
