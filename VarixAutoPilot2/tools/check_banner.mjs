// ★ 横幅位置实测 —— 定位它到底被谁挤到右栏 ★
//
// 现象：横幅 DOM 在 </header> 之后、<main> 之前（结构正确），
//但截图里它出现在右栏中部。
// ⇒ 要测的是：它的父链、它的实际 box、以及 body 各子元素的 box。

import { chromium } from 'playwright-core';

const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});
const p = await b.newPage({ viewport: { width: 1500, height: 900 } });
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2500);

const v = await p.evaluate(() => {
  const bn = document.getElementById('port-banner');
  if (!bn) return { 有横幅: false };
  bn.hidden = false; // 强制显示以便测量
  const r = bn.getBoundingClientRect();
  const cs = getComputedStyle(bn);
  const chain = [];
  let e = bn;
  while (e && e.tagName !== 'HTML') {
    const rr = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    chain.push({
      tag: e.tagName,
      cls: e.className || '',
      box: `${Math.round(rr.left)},${Math.round(rr.top)} ${Math.round(rr.width)}x${Math.round(rr.height)}`,
      display: s.display,
      flex: s.flex,
      position: s.position,
    });
    e = e.parentElement;
  }
  return {
    横幅: {
      box: `${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)}`,
      display: cs.display,
      position: cs.position,
      flex: cs.flex,
      margin: cs.margin,
      width: cs.width,
    },
    父链: chain,
    body子元素: Array.from(document.body.children).map((c) => {
      const rr = c.getBoundingClientRect();
      const id = c.id ? '#' + c.id : '.' + String(c.className || '').split(' ')[0];
      return `${c.tagName}${id} @${Math.round(rr.left)},${Math.round(rr.top)} `
        + `${Math.round(rr.width)}x${Math.round(rr.height)}`;
    }),
  };
});

console.log(JSON.stringify(v, null, 1));
await b.close();
