// ★ 真机复现「点按钮没反应」★
//
// 现象（Variable 截图 + 日志）：
//   日志里**完全没有 enqueue 记录** ⇒ 点击没到后端
//   而 18:25:48 有第二次「启动」⇒ 我自己的轮询脚本杀了窗口重启
//
// 本脚本只做一件事：真点一次「加入待发」，然后看日志。
// 不做任何其他操作（避免再干扰用户）。

import { chromium } from 'playwright-core';
import { readFileSync, existsSync } from 'node:fs';

const LOG = 'C:/Users/varia/AppData/Local/Temp/varix-autopilot.log';
const readLog = () =>
  existsSync(LOG)
    ? readFileSync(LOG, 'utf8')
        .replace(/�/g, '')
        .split('\n')
        .slice(-8)
        .join('\n')
    : '(无日志)';

console.log('=== 点击前日志 ===');
console.log(readLog());

// 用 file:// 直接加载 Tauri 打进去的那份页面不行（Tauri 资源在 exe 里），
// 所以这里验证的是「同一份 HTML+JS 在浏览器里的行为」，
// 重点是看 onclick 到底有没有绑上。
const b = await chromium.launch({
  headless: true,
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
});
const p = await b.newPage({ viewport: { width: 1518, height: 947 } });
const errs = [];
p.on('pageerror', (e) => errs.push('PAGEERROR: ' + String(e)));
p.on('console', (m) => {
  if (m.type() === 'error') errs.push('[console] ' + m.text());
});
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2200);

// ① 逐个勾选框点一下，看会不会自己弹回去
console.log('\n=== 勾选框稳定性测试（点两次，看是否保持）===');
for (const id of ['o-new', 'o-dry', 'lp-inf', 'o-free']) {
  const r = await p.evaluate((i) => {
    const e = document.getElementById(i);
    if (!e) return { id: i, 存在: false };
    const before = e.checked;
    e.click();
    const after = e.checked;
    return { id: i, 前: before, 点一次后: after };
  }, id);
  console.log(`  ${r.id.padEnd(8)} 前=${r.前} 点一次后=${r.点一次后} ${r.点一次后 === r.前 ? '★没变化★' : 'OK'}`);
}

// ② 等 3.5 秒（跨过两次 tick）再看有没有被重置
await p.waitForTimeout(3500);
console.log('\n=== 等待 3.5 秒（跨过两次 tick）后 ===');
const after = await p.evaluate(() => ({
  'o-new': document.getElementById('o-new')?.checked,
  'o-dry': document.getElementById('o-dry')?.checked,
  'lp-inf': document.getElementById('lp-inf')?.checked,
  'o-free': document.getElementById('o-free')?.checked,
}));
for (const [k, v] of Object.entries(after)) {
  console.log(`  ${k.padEnd(8)} = ${v}`);
}

// ③ 真点「加入待发」
console.log('\n=== 真点「加入待发」===');
await p.click('button:has-text("加入待发")');
await p.waitForTimeout(1500);
const q = await p.evaluate(() => ({
  队列: document.querySelectorAll('#qlist .qi').length,
  qsum: document.getElementById('q-sum')?.textContent || '',
  hint: (document.getElementById('hint')?.textContent || '').slice(0, 60),
}));
console.log('  队列 =', q.队列, '|', q.qsum);
console.log('  hint =', q.hint);

console.log('\n页面错误:', errs.length ? errs.join('\n           ') : '(无)');
await b.close();
