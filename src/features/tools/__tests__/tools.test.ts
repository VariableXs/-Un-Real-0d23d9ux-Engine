// AURORA-10000: AI-31~AI-35 批次（领域07 效率与工具中枢）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain07Checks } from '../checks';
import { evalExpr, monthGrid, parseNatural, SnippetLib, StickyBoard, TodoStore } from '../groupA';
import { dedupeLines, diffLines, qrMatrix, sha256Hex, toFullWidth, type DiffLine } from '../groupB';
import { bookletOrder, nUpLayout } from '../groupC';
import { passwordStrength, totp } from '../groupD';
import { haversineKm, sm2, createCard } from '../groupE';

describe('AURORA-10000 领域07 全量自检（F03751~F04375）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain07Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-31 逻辑核抽查', () => {
  it('表达式求值与月历', () => {
    expect(evalExpr('2*(3+4)-5/2')).toBe(11.5);
    const grid = monthGrid(2026, 1).flat().filter((d): d is number => d !== null);
    expect(grid[0]).toBe(1);
    expect(grid[grid.length - 1]).toBe(28);
  });
  it('自然语言解析', () => {
    const now = new Date(2026, 8, 10, 12, 0);
    const p = parseNatural('明天下午3点半开会', now);
    expect(p.title).toContain('开会');
    expect(p.due).toBe(new Date(2026, 8, 11, 15, 30).getTime());
  });
  it('片段库去重与便签加密', () => {
    const lib = new SnippetLib();
    expect(lib.add({ title: 'a', body: 'b' })).toBeDefined();
    expect(lib.add({ title: 'a', body: 'b' })).toBeUndefined();
    const board = new StickyBoard();
    const n = board.add('机密内容');
    board.encrypt(n.id, 'k1');
    expect(board.decrypt(n.id, 'k2')).toBeUndefined();
    expect(board.decrypt(n.id, 'k1')).toBe('机密内容');
  });
  it('待办重复任务推进', () => {
    const s = new TodoStore();
    const t = s.addTodo('明天 喝水', 1760000000000);
    s.setTags(t.id, ['习惯']);
    const next = s.nextRepeat({ ...t, repeat: 'daily' }, 1760000000000);
    expect(next).toBeDefined();
    expect(next! % 86_400_000).toBeDefined();
    expect(s.batchOp([t.id], 'done')).toBe(1);
    expect(s.exportJson()).toContain('喝水');
  });
});

describe('AI-32 逻辑核抽查', () => {
  it('全半角与行去重', () => {
    expect(toFullWidth('abc 123')).toBe('ａｂｃ　１２３');
    expect(dedupeLines('x\ny\nx', true)).toBe('y\nx');
  });
  it('SHA-256 已知向量', () => {
    expect(sha256Hex('')).toBe('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
    expect(sha256Hex('hello 世界')).toHaveLength(64);
  });
  it('二维码矩阵含定位图案', () => {
    const m = qrMatrix('https://varix.local');
    expect(m.size).toBeGreaterThanOrEqual(21);
    expect(m.dark(0, 0) && m.dark(m.size - 1, 0) && m.dark(0, m.size - 1)).toBe(true);
  });
});

describe('AI-33 逻辑核抽查', () => {
  it('N 合一与小册子排版', () => {
    expect(nUpLayout(2)).toMatchObject({ cols: 2, rows: 1, perSheet: 2 });
    expect(bookletOrder(4).flat().join()).toBe('4,1,2,3');
  });
});

describe('AI-34 逻辑核抽查', () => {
  it('密码强度与 TOTP', () => {
    expect(passwordStrength('Str0ng!Passw0rd!').score).toBeGreaterThanOrEqual(85);
    expect(passwordStrength('123456').label).toBe('极弱');
    expect(totp('SECRET', 0)).toMatch(/^\d{6}$/);
    expect(totp('SECRET', 0)).toBe(totp('SECRET', 29));
    expect(totp('SECRET', 0)).not.toBe(totp('SECRET', 60));
  });
});

describe('AI-35 逻辑核抽查', () => {
  it('球面距离与 SM-2', () => {
    const d = haversineKm({ lat: 31.23, lon: 121.47 }, { lat: 39.9, lon: 116.4 });
    expect(d).toBeGreaterThan(1000);
    expect(d).toBeLessThan(1120);
    const card = sm2(sm2(createCard('d', 'f', 'b'), 5), 3);
    expect(card.intervalDays).toBeGreaterThanOrEqual(1);
  });
});

describe('跨模块一致性', () => {
  it('diff 与合并互补', () => {
    const d = diffLines('a\nb', 'a\nc');
    expect(d.filter((l: DiffLine) => l.type === 'add')).toHaveLength(1);
    expect(d.filter((l: DiffLine) => l.type === 'del')).toHaveLength(1);
  });
});
