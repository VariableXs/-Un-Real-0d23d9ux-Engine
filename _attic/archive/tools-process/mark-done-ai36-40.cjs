// AURORA-10000: AI-36~AI-40 批次文档状态更新脚本（一次性，运行后可删）。
// 更新分工完成图 AI-36~40 区块 + 全景图 F04376~F05000 状态行，root 与 docs/ 副本 md5 对齐。
const fs = require('fs');

const LUODIAN =
  'src/features/hardware/：hwModel.ts（共享模型 + CapabilityRegistry 能力位注册表 + Switch + TutorialCenter）+ ' +
  'groupA~E 五文件 25 族逻辑核（AI-36 显示/音频/电源/外设/存储 → groupA；AI-37 固件/输入联动/传感/互联/虚拟化 → groupB；' +
  'AI-38 诊断/更新/灾备/安全硬件/性能调校 → groupC；AI-39 触笔/影像/色准/空间音频/扫描 → groupD；' +
  'AI-40 笔记本场景/DIY/平板/IoT/可靠性 → groupE）+ checks.ts 625 项逐条断言注册表（runDomain08Checks，' +
  '70 个「位/预留」按 §15 口径 = 接口冻结 + 开关存在，26 篇族教学交付）+ __tests__/hardware.test.ts 16 例全绿；' +
  '提交号见 git log（feat(hardware): AI-36~AI-40）';

// ---- 1) 分工完成图 ----
for (const p of ['AURORA-10000-AI分工完成图.md']) {
  let t = fs.readFileSync(p, 'utf8');
  const before = t;

  // 1a. 总状态行
  t = t.replace(
    '> **当前状态：实施进行中，累计 3750/10000——AI-01~AI-15（W1+W2）与 AI-26~AI-30（领域06 文件与数据能力，F03126~F03750，625 项）均已 ✅；其余批次 ⬜。**',
    '> **当前状态：实施进行中，累计 4375/10000——AI-01~AI-15（W1+W2）、AI-26~AI-30（领域06 文件与数据能力，F03126~F03750，625 项）与 AI-36~AI-40（领域08 系统集成与硬件，F04376~F05000，625 项）均已 ✅；其余批次 ⬜。**'
  );
  // 1b. W3 行与合计行
  t = t.replace('| W3 | AI-26~AI-40 | F03126~F05000 | 🔶 625/1875（领域06 全部完成） |', '| W3 | AI-26~AI-40 | F03126~F05000 | 🔶 1250/1875（领域06、领域08 全部完成） |');
  t = t.replace('| 合计 | 80 | F00001~F10000 | 🔶 3750/10000 |', '| 合计 | 80 | F00001~F10000 | 🔶 4375/10000 |');
  // 1c. AI-36~40 区块头与族行（族行严格限定 0176~0200，避免误伤 AI-35 的族0170~0175）
  t = t.replace(/^### (AI-3[6-9]|AI-40) (.+· W3）)⬜$/gm, '### $1 $2✅');
  t = t.replace(/^- (族0(17[6-9]|1[89][0-9]|200)) .+25 项 ⬜ 0\/25$/gm, (m) => m.replace('⬜ 0/25', '✅ 25/25'));
  // 1d. 落点记录（仅限 AI-36 区块头到 W4 批次标题之间）
  const start = t.indexOf('### AI-36 ');
  const end = t.indexOf('## W4 批次');
  if (start < 0 || end < 0 || end <= start) throw new Error('block bounds not found');
  const seg = t.slice(start, end);
  const segNew = seg.split('- 落点记录：（实施会话完成后填写实际改动文件与提交号）').join('- 落点记录：' + LUODIAN);
  t = t.slice(0, start) + segNew + t.slice(end);

  if (t === before) throw new Error('no change applied to ' + p);
  fs.writeFileSync(p, t);
  console.log('updated', p);
}

// ---- 2) 全景图 F04376~F05000 状态行 ----
for (const p of ['AURORA-10000-功能全景图.md']) {
  const lines = fs.readFileSync(p, 'utf8').split('\n');
  let n = 0;
  const out = lines.map((line) => {
    const m = line.match(/^- F(\d{5}) /);
    if (m) {
      const id = parseInt(m[1], 10);
      if (id >= 4376 && id <= 5000 && !line.endsWith('✅')) {
        n++;
        return line + ' ✅';
      }
    }
    return line;
  });
  if (n !== 625) throw new Error('expected 625 panorama lines marked, got ' + n);
  fs.writeFileSync(p, out.join('\n'));
  console.log('panorama marked', n, 'lines');
}

// ---- 3) 同步 docs/ 副本 ----
fs.copyFileSync('AURORA-10000-AI分工完成图.md', 'docs/AURORA-10000-AI分工完成图.md');
fs.copyFileSync('AURORA-10000-功能全景图.md', 'docs/AURORA-10000-功能全景图.md');
console.log('docs/ copies synced');
