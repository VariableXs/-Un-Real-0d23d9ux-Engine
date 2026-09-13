/**
 * UNREAL-X-15000 · AI-35 兼容深化与收官（领域09 · 族0341~0350 · X08501~X08750）门禁用例。
 * V 线五族 125 项（compat/ai35Checks.ts）。
 */
import { describe, expect, it } from 'vitest';
import { runAi35Checks, checkF0344, checkF0345, checkF0346, checkF0348, checkF0350 } from '../ai35Checks';
import { CompatDocLibrary, CommunityFeedback, CertSuite, CompatA11y, CompatFinale, FINALE_CHECKLIST, explainError } from '../ai35Models';

const families: [string, () => ReturnType<typeof checkF0344>][] = [
  ['族0344 兼容文档库', checkF0344],
  ['族0345 社区反馈', checkF0345],
  ['族0346 兼容认证', checkF0346],
  ['族0348 兼容无障碍', checkF0348],
  ['族0350 兼容收官', checkF0350],
];

describe('AI-35 兼容深化与收官 · V 线（族0344~0350 V 段）', () => {
  it('各族恰 25 项', () => {
    for (const [name, f] of families) expect(f(), name).toHaveLength(25);
  });

  it('V 线聚合 125 项全绿', () => {
    const { entries, failed } = runAi35Checks();
    expect(entries).toHaveLength(125);
    expect(failed).toEqual([]);
  });

  it('V 线 ID 区间唯一且不越界', () => {
    const { entries } = runAi35Checks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(125);
    for (const id of ids) {
      const inV = (id >= 8576 && id <= 8650) || (id >= 8676 && id <= 8700) || (id >= 8726 && id <= 8750);
      expect(inV, String(id)).toBe(true);
    }
  });
});

describe('AI-35 逻辑核抽查', () => {
  it('文档库版本只升不降与前缀检索', () => {
    const lib = new CompatDocLibrary();
    lib.put('a', 2, 'new');
    lib.put('a', 1, 'old');
    expect(lib.get('a')?.ver).toBe(2);
    lib.put('b/1', 1, '');
    lib.put('b/2', 1, '');
    expect(lib.search('b/').join()).toBe('b/1,b/2');
  });

  it('反馈热度榜与去重', () => {
    const fb = new CommunityFeedback();
    fb.submit({ title: 'a', reproSteps: 2, votes: 1 });
    fb.submit({ title: 'a', reproSteps: 2, votes: 4 });
    fb.submit({ title: 'b', reproSteps: 3, votes: 9 });
    fb.dedup();
    expect(fb.count).toBe(2);
    expect(fb.top(1)[0]?.title).toBe('b');
  });

  it('认证评级矩阵', () => {
    const mk = (n: number, passEvery: number) => {
      const q = new CertSuite();
      for (let i = 0; i < n; i++) q.record(passEvery === 0 || i % passEvery !== 0);
      return q.verdict();
    };
    expect(mk(20, 0)).toBe('gold');
    expect(mk(10, 0)).toBe('silver');
    expect(mk(10, 10)).toBe('pass');
    expect(mk(10, 2)).toBe('fail');
  });

  it('无障碍对比与读屏回退', () => {
    const a = new CompatA11y();
    expect(a.contrast(0, 255)).toBe(21);
    expect(a.aaOk(4.4, false)).toBe(false);
    expect(a.ariaLabel(undefined, '回退')).toBe('回退');
    expect(new CompatA11y('strict').isHc()).toBe(true);
  });

  it('收官门禁与核对单', () => {
    const f = new CompatFinale();
    for (const item of FINALE_CHECKLIST) f.check(item);
    expect(f.ready()).toBe(true);
    expect(f.banner()).toContain('750/750');
    expect(new CompatFinale().check('不存在项')).toBe(false);
  });

  it('错误码叙事完备', () => {
    for (const code of ['E3501', 'E3502', 'E3503', 'E3504', 'E3505', 'E0000']) {
      expect(explainError(code).text.length).toBeGreaterThan(0);
      expect(explainError(code).next.length).toBeGreaterThan(0);
    }
  });
});
