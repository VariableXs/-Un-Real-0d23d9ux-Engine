/**
 * UNREAL-X-15000 · AI-34 兼容工程（领域09 · 族0331~0340 · X08251~X08500）门禁用例。
 * V 线五族 125 项（compat/ai34Checks.ts）+ C 线四族 + K 线族0339 合计 250 项分段核验。
 */
import { describe, expect, it } from 'vitest';
import { runAi34Checks, checkF0333, checkF0334, checkF0335, checkF0337, checkF0338 } from '../ai34Checks';
import { CoexistMatrix, EnterpriseEnv, CjkCompat, WebCompat, FormatBridge, explainError } from '../ai34Models';

const families: [string, () => ReturnType<typeof checkF0333>][] = [
  ['族0333 多系统共存', checkF0333],
  ['族0334 企业环境', checkF0334],
  ['族0335 中文深度兼容', checkF0335],
  ['族0337 Web 兼容', checkF0337],
  ['族0338 格式兼容', checkF0338],
];

describe('AI-34 兼容工程 · V 线（族0333~0340 V 段）', () => {
  it('各族恰 25 项', () => {
    for (const [name, f] of families) expect(f(), name).toHaveLength(25);
  });

  it('V 线聚合 125 项全绿', () => {
    const { entries, failed } = runAi34Checks();
    expect(entries).toHaveLength(125);
    expect(failed).toEqual([]);
  });

  it('V 线 ID 区间唯一且不越界', () => {
    const { entries } = runAi34Checks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(125);
    for (const id of ids) {
      const inV = (id >= 8301 && id <= 8375) || (id >= 8401 && id <= 8450);
      expect(inV, String(id)).toBe(true);
    }
  });
});

describe('AI-34 逻辑核抽查', () => {
  it('共存菜单与风险分级', () => {
    const m = new CoexistMatrix();
    m.add('VARIX', 0);
    m.add('win98', 9, false);
    expect(m.menu()[0]).toBe('VARIX');
    expect(m.risk()).toBe(1);
  });

  it('企业强制策略不可覆盖', () => {
    const e = new EnterpriseEnv();
    e.push('k', 'domain', true);
    e.push('k', 'local', false);
    expect(e.effective('k')).toBe('domain');
    expect(e.deployQuota()).toBe(20);
  });

  it('中文路径长度与全角', () => {
    const c = new CjkCompat();
    expect(c.pathLen('C:/中文/文件')).toBe(12);
    expect(c.toFullWidth('a')).toBe('ａ');
    expect(c.fontFallback('楷体').join()).toBe('楷体,黑体,system');
  });

  it('Web 引擎降级与格式嗅探', () => {
    const w = new WebCompat('off');
    expect(w.pickEngine('Modern')).toBe('static');
    expect(w.renderPath(true, true)).toBe('canvas2d');
    const f = new FormatBridge();
    expect(f.sniff('{"a":1}')).toBe('json');
    expect(f.fromCsv('a,"b,c"').join('|')).toBe('a|b,c');
    expect(f.toCsv(['x', 'y,z'])).toBe('x,"y,z"');
  });

  it('错误码叙事完备', () => {
    for (const code of ['E3401', 'E3402', 'E3403', 'E3404', 'E3405', 'E9999']) {
      expect(explainError(code).text.length).toBeGreaterThan(0);
      expect(explainError(code).next.length).toBeGreaterThan(0);
    }
  });
});
