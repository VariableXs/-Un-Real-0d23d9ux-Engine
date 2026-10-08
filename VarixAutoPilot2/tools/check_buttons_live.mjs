// ★ 逐个真点按钮，验证哪些真的生效 ★
//
// 目的：搞清 app.js 的 doPreview / doSend 到底能不能用。
// 检查器显示它们**不在**裸全局也不在 window 上，
// 但实测「预览」按钮点下去时间戳会变——矛盾必须查清。

import { chromium } from 'playwright-core';

const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});
const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
const errs = [];
p.on('pageerror', (e) => errs.push('PAGEERROR: ' + String(e)));
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2200);

// 先看 onclick 属性的实际内容（可能与HTML 源码不同）
const attrs = await p.evaluate(() => {
  const out = {};
  for (const b of document.querySelectorAll('button[onclick]')) {
    out[(b.innerText || '').trim().slice(0, 10)] = b.getAttribute('onclick');
  }
  return out;
});
console.log('=== 按钮的 onclick 属性实际值 ===');
for (const [k, v] of Object.entries(attrs)) console.log(`  ${k.padEnd(10)} → ${v}`);

const labels = ['预览', '刷新', '首轮', '后续每轮', '读取', '探测可用技能'];
for (const label of labels) {
  const before = await p.evaluate(() => ({
    hint: (document.getElementById('hint') || {}).textContent?.slice(0, 30) || '',
    loopState: (document.getElementById('loop-state') || {}).textContent || '',
  }));
  let ok = true;
  try {
    await p.click(`button:has-text("${label}")`, { timeout: 3000 });
  } catch (e) {
    ok = false;
    console.log(`点「${label}」→ ★点击失败★ ${String(e).slice(0, 60)}`);
  }
  if (!ok) continue;
  await p.waitForTimeout(900);
  const after = await p.evaluate(() => ({
    hint: (document.getElementById('hint') || {}).textContent?.slice(0, 44) || '',
    loopState: (document.getElementById('loop-state') || {}).textContent || '',
    queue: document.querySelectorAll('.qi').length,
  }));
  const changed = after.hint !== before.hint;
  console.log(
    `点「${label}」→ ${changed ? '有反应' : '无变化'} hint="${after.hint}" ` +
    `循环=${after.loopState} 队列=${after.queue}`
  );
}
console.log('\n错误:', errs.length ? errs.join(' | ') : '(无)');
await b.close();
