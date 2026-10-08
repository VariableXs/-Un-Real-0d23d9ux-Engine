// ★ 多尺寸下验证「每个按钮都可达」★
//
// 起因：Variable 反馈「所有按钮点了没反应」。
// 根因候选：按钮被推出可视区（布局缺陷）—— 但**不能靠看图判断**，
// 必须量 getBoundingClientRect 是否落在视口内。
// 本脚本在用户实际用过的几个窗口尺寸下逐个量。

import { chromium } from 'playwright-core';

const SIZES = [
  [1818, 1102],   // 之前实测的最大尺寸
  [1454, 882],    // ★ Variable 实际缩小到的尺寸（截图证实）
  [1400, 860],    // 新的 minWidth/minHeight
  [1280, 800],
  [1100, 700],    // 旧的 minWidth/minHeight
];

const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});

let bad = 0;
for (const [w, h] of SIZES) {
  const p = await b.newPage({ viewport: { width: w, height: h } });
  await p.goto('http://127.0.0.1:8791/index.html');
  await p.waitForTimeout(1500);

  const r = await p.evaluate(() => {
    // ★ 判据辅助：初始是否在视口内
    const inView = (r) =>
      r.right > 0 && r.left < window.innerWidth &&
      r.bottom > 0 && r.top < window.innerHeight;
    const out = {
      视口: window.innerWidth + 'x' + window.innerHeight,
      越界按钮: [],
      可见按钮: 0,
      按钮总数: 0,
      队列面板: '',
      右栏宽: 0,
    };
    // 主区两栏实际宽度（诊断右栏被压成多窄）
    const cols = document.querySelectorAll('main > .col');
    if (cols[1]) out.右栏宽 = Math.round(cols[1].getBoundingClientRect().width);

    const q = document.getElementById('qlist');
    if (q) {
      const qr = q.getBoundingClientRect();
      out.队列面板 = Math.round(qr.top) + '..' + Math.round(qr.bottom)
        + (qr.width > 0 ? ' 宽' + Math.round(qr.width) : ' 宽0');
    }

    for (const btn of document.querySelectorAll('button')) {
      const r = btn.getBoundingClientRect();
      if (r.width === 0 && r.height === 0) continue;   // 隐藏的
      out.按钮总数++;
      // ★ 判据：「初始在视口内」或「所在栏能滚到它」都算可达 ★
      // 早先只判初始可见，内容一多就报「全部越界」——
      // 那是**检查器的错**，会让人去改本来正确的布局。
      let reachable = inView(r);
      if (!reachable) {
        const col = btn.closest('.col');
        if (col) {
          const canScroll = col.scrollHeight > col.clientHeight + 2
            && getComputedStyle(col).overflowY !== 'visible';
          if (canScroll) {
            const cr = col.getBoundingClientRect();
            // 滚到底之后它能不能进入视口
            const need = Math.max(0, r.bottom - (cr.bottom - 4));
            reachable = need <= col.scrollHeight - col.clientHeight + 12;
          }
        }
      }
      if (reachable) {
        out.可见按钮++;
      } else {
        out.越界按钮.push(
          (btn.innerText || btn.title || '?').trim().slice(0, 12) +
          '@' + Math.round(r.left) + ',' + Math.round(r.top)
        );
      }
    }
    return out;
  });

  const ok = r.越界按钮.length === 0;
  if (!ok) bad++;
  console.log((ok ? 'OK   ' : '★越界') + ' ' + r.视口 +
    '  右栏宽=' + r.右栏宽 +
    '  按钮 ' + r.可见按钮 + '/' + r.按钮总数 +
    '  队列[' + r.队列面板 + ']');
  if (!ok) console.log('        越界: ' + r.越界按钮.join(' | '));
  await p.close();
}

await b.close();
console.log(bad === 0 ? '\n全部尺寸：所有按钮都在可视区内' : '\n★ ' + bad + ' 个尺寸有按钮越界');
process.exit(bad === 0 ? 0 : 1);
