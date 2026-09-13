// AURORA-10000: AI-71~AI-75 批次文档标记脚本（领域15 F08751~F09375），勿删。
// 用法：node tools/mark-ai71-75.cjs （幂等，可重复执行）
const fs = require('fs');

const copies = ['', 'docs/'];

function markPanorama(text) {
  let n = 0;
  const out = text.split('\n').map((line) => {
    const cr = line.endsWith('\r');
    const body = cr ? line.slice(0, -1) : line;
    const m = body.match(/^- F(\d{5}) /);
    if (m) {
      const id = parseInt(m[1], 10);
      if (id >= 8751 && id <= 9375 && !body.endsWith('✅')) {
        n++;
        return `${body} ✅${cr ? '\r' : ''}`;
      }
    }
    return line;
  });
  return { text: out.join('\n'), n };
}

function markAssignments(text) {
  let changed = 0;
  const lines = text.split('\n');
  let inW7 = false;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const cr = raw.endsWith('\r');
    const L = cr ? raw.slice(0, -1) : raw;
    if (L.startsWith('## W7 批次')) { inW7 = true; continue; }
    if (inW7 && L.startsWith('## W8 批次')) { inW7 = false; continue; }
    if (!inW7) continue;
    let rep = null;
    if (/^### AI-7[1-5] .*· W7）⬜$/.test(L)) rep = L.replace('）⬜', '）✅');
    else if (/^- 族0(35[1-9]|36[0-9]|37[0-5]) .*25 项 ⬜ 0\/25$/.test(L)) rep = L.replace('25 项 ⬜ 0/25', '25 项 ✅ 25/25');
    else if (L === '落点记录：（实施会话完成后填写实际改动文件与提交号）') {
      rep = '落点记录：src/features/uikit/{groupA,groupB,groupC,groupD,groupE,checks}.ts（族0351~0375 逻辑核 + 625 项逐条断言注册表 runDomain15Checks）+ __tests__/uikit.test.ts（vitest 11 例全绿）；typecheck 本范围 0 错误；ID 校验 unique=10000 无缺重；提交号见 git log（uikit(ai-71~ai-75)）。';
    }
    if (rep !== null) { lines[i] = `${rep}${cr ? '\r' : ''}`; changed++; }
  }
  return { text: lines.join('\n'), changed };
}

// 分工图头部状态行与总览表（基于当前 7375 基线 → 8000）
function markHeader(text) {
  const before = text;
  let t = text;
  t = t.replace('累计 7375/10000——AI-01~AI-15（W1+W2）', '累计 8000/10000——AI-01~AI-15（W1+W2）');
  t = t.replace('、AI-76~AI-79（领域16 工程质量·性能与收官前四组，F09376~F09875，500 项）均已 ✅',
    '、AI-71~AI-75（领域15 UI 设计与优化，F08751~F09375，625 项）、AI-76~AI-79（领域16 工程质量·性能与收官前四组，F09376~F09875，500 项）均已 ✅');
  t = t.replace('| W7 | AI-71~AI-77 | F08751~F09625 | 🔶 250/875（AI-76~AI-77 区块 ✅） |', '| W7 | AI-71~AI-77 | F08751~F09625 | ✅ 875/875（领域15 全部完成 + AI-76~77） |');
  t = t.replace('| 合计 | 80 | F00001~F10000 | 🔶 7375/10000 |', '| 合计 | 80 | F00001~F10000 | 🔶 8000/10000 |');
  return { text: t, headerChanged: t !== before };
}

for (const dir of copies) {
  const pano = `${dir}AURORA-10000-功能全景图.md`;
  const assign = `${dir}AURORA-10000-AI分工完成图.md`;
  const r1 = markPanorama(fs.readFileSync(pano, 'utf8'));
  fs.writeFileSync(pano, r1.text);
  const a = fs.readFileSync(assign, 'utf8');
  const r2 = markAssignments(a);
  const r3 = markHeader(r2.text);
  fs.writeFileSync(assign, r3.text);
  console.log(`${dir || './'}: panorama +${r1.n}, assignment ${r2.changed} lines, header=${r3.headerChanged}`);
}
