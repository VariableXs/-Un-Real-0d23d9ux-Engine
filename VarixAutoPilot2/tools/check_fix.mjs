// ★ 验证本轮三项修复 ★
import { chromium } from 'playwright-core';
const b = await chromium.launch({ headless: true, executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe' });
const p = await b.newPage({ viewport: { width: 1518, height: 947 } });
const errs = [];
p.on('pageerror', (e) => errs.push(String(e)));
await p.goto('http://127.0.0.1:8791/index.html');
await p.waitForTimeout(2200);

const d = () => p.evaluate(() => ({
  o_dry: document.getElementById('o-dry')?.checked,
  o_new: document.getElementById('o-new')?.checked,
  o_free: document.getElementById('o-free')?.checked,
  srcnote: (document.getElementById('src-note')?.textContent || '').slice(0, 50),
  drynote可见: !document.getElementById('dry-note')?.hidden,
}));

console.log('=== 修复 2：默认不勾干跑/不勾新对话 ===');
let s = await d();
console.log('  只填不发 =', s.o_dry, s.o_dry ? '★仍勾着★' : 'OK 不勾');
console.log('  先开新对话 =', s.o_new, s.o_new ? '★仍勾着★' : 'OK 不勾');
console.log('  干跑条可见 =', s.drynote可见, s.drynote可见 ? '★不该可见★' : 'OK 隐藏');

console.log('\n=== 修复 1：内容来源指示器常驻 ===');
console.log('  src-note =', JSON.stringify(s.srcnote));

console.log('\n=== 修复 1b：勾了空自由文本 ⇒ 指示器立刻报警 ===');
await p.click('#o-free');
await p.waitForTimeout(400);
s = await d();
console.log('  src-note =', JSON.stringify(s.srcnote));
console.log('  ' + (s.srcnote.includes('空的') ? '★ 正确报警 ★' : '未报警'));

console.log('\n=== 修复 1c：填上内容后指示器变正常 ===');
await p.fill('#free-text', '测试内容 ABC');
await p.waitForTimeout(400);
s = await d();
console.log('  src-note =', JSON.stringify(s.srcnote));

console.log('\n=== 勾干跑 ⇒ 醒目条出现 ===');
await p.click('#o-dry');
await p.waitForTimeout(400);
s = await d();
console.log('  干跑条可见 =', s.drynote可见, s.drynote可见 ? '★ 正确出现 ★' : '未出现');

console.log('\n=== 全空状态点「加入待发」⇒ 失败但提示醒目 ===');
await p.click('#o-free'); await p.waitForTimeout(200);
await p.fill('#free-text', ''); await p.waitForTimeout(300);
await p.click('button:has-text("加入待发")');
await p.waitForTimeout(1000);
const q = await p.evaluate(() => ({
  队列: document.querySelectorAll('#qlist .qi').length,
  hint: (document.getElementById('hint')?.textContent || '').slice(0, 50),
  srcnote: (document.getElementById('src-note')?.textContent || '').slice(0, 46),
}));
console.log('  队列 =', q.队列, '（0 是对的，本来就该失败）');
console.log('  hint =', q.hint);
console.log('  src-note =', q.srcnote);
console.log('  ' + (q.srcnote.includes('空的') ? '★ 界面已明确告知原因 ★' : '★ 界面无提示 ★'));

console.log('\n=== 有内容时点「加入待发」⇒ 应成功 ===');
await p.fill('#free-text', '测试内容 ABC');
await p.waitForTimeout(300);
await p.click('button:has-text("加入待发")');
await p.waitForTimeout(1200);
const ok = await p.evaluate(() => ({
  队列: document.querySelectorAll('#qlist .qi').length,
  qsum: document.getElementById('q-sum')?.textContent || '',
}));
console.log('  队列 =', ok.队列, '|', ok.qsum);
console.log('  ' + (ok.队列 > 0 ? '★ 入队成功 ★' : '★ 仍失败 ★'));

console.log('\n页面错误:', errs.length ? errs.join(' | ') : '(无)');
await b.close();
