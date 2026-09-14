// AURORA-10000: AI-66~AI-70 收官脚本——全景图/分工完成图状态更新 + 四处副本 md5 对齐，勿删。
const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');
const write = (p, s) => fs.writeFileSync(path.join(root, p), s);

function markPanorama(text) {
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const m = /^- (F08(1[2-9]\d|2\d\d|3\d\d|4\d\d|5\d\d|6\d\d|7[0-4]\d|750)) /.exec(lines[i]);
    if (!m) continue;
    const n = parseInt(m[1].slice(1), 10);
    if (n < 8126 || n > 8750) continue;
    if (!lines[i].endsWith('✅')) lines[i] = lines[i] + ' ✅';
  }
  return lines.join('\n');
}

function updateAssignment(text) {
  let t = text;
  // AI-66~AI-70 区块标题与族行：⬜ 0/25 → ✅ 25/25
  for (let id = 66; id <= 70; id++) {
    t = t.replace(new RegExp(`(### AI-${id} [^\\n]*· W6）)⬜`, 'u'), '$1✅');
  }
  t = t.replace(/(25 项) ⬜ 0\/25/g, '$1 ✅ 25/25');
  const landing = '- 落点记录：src/features/a11y-l10n/{types,groupA~E,checkA~E,checks}.ts（领域14 无障碍与本地化 25 族逻辑核 + 625 项逐条断言注册表，runDomain14Checks；「位/预留」按 §15 口径 = 接口冻结 + 开关存在）+ __tests__/a11yL10n.test.ts 16 例全绿；提交号见 git log（feat(a11y-l10n): AI-66~AI-70）。';
  const landingTail = '- 自检：构建/审计/视觉/性能/文档五门禁 + 本组 125 项逐条核对（本批次已跑：tsc 本范围 0 错 / vitest 2332 绿（含领域14 625 项断言全绿））';
  const placeholder = /- 落点记录：（实施会话完成后填写实际改动文件与提交号）\r?\n- 自检：构建\/审计\/视觉\/性能\/文档五门禁 \+ 本组 125 项逐条核对/g;
  t = t.replace(placeholder, landing + '\n' + landingTail);
  // 总览行
  t = t.replace('| W6 | AI-61~AI-70 | F07501~F08750 | ⬜ 0/1250 |', '| W6 | AI-61~AI-70 | F07501~F08750 | 🔶 625/1250（AI-66~AI-70 区块 ✅） |');
  t = t.replace('| 合计 | 80 | F00001~F10000 | 🔶 6250/10000 |', '| 合计 | 80 | F00001~F10000 | 🔶 6875/10000 |');
  // 头部状态
  t = t.replace(
    '**当前状态：实施进行中，累计 6250/10000——',
    '**当前状态：实施进行中，累计 6875/10000——AI-66~AI-70（领域14 无障碍与本地化，F08126~F08750，625 项）已 ✅；'
  );
  return t;
}

for (const p of ['docs/AURORA-10000-功能全景图.md', 'AURORA-10000-功能全景图.md']) {
  const before = read(p);
  const after = markPanorama(before);
  write(p, after);
  console.log(p, 'panorama-marked:', after !== before);
}

for (const p of ['docs/AURORA-10000-AI分工完成图.md', 'AURORA-10000-AI分工完成图.md']) {
  const before = read(p);
  const after = updateAssignment(before);
  write(p, after);
  console.log(p, 'assignment-updated:', after !== before);
}

// md5 对齐校验
const crypto = require('crypto');
for (const name of ['AURORA-10000-功能全景图.md', 'AURORA-10000-AI分工完成图.md', 'AURORA-10000-实施总步骤图.md']) {
  const a = crypto.createHash('md5').update(read(path.join('docs', name))).digest('hex');
  const b = crypto.createHash('md5').update(read(name)).digest('hex');
  if (a !== b) { fs.copyFileSync(path.join(root, 'docs', name), path.join(root, name)); console.log(name, 'synced root<-docs'); }
  console.log(name, 'md5-match:', crypto.createHash('md5').update(read(path.join('docs', name))).digest('hex') === crypto.createHash('md5').update(read(name)).digest('hex'));
}
