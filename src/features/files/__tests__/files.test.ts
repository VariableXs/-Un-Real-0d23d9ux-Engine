// AURORA-10000: AI-26~AI-30 批次（领域06 文件与数据能力）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain06Checks } from '../checks';
import { Vfs } from '../fsModel';
import { NameIndex, searchFiles, findDuplicates, batchRename, pasteWithConflict } from '../groupA';
import { treemap, RecycleBin, VersionStore, twoWaySync, blockDiff } from '../groupB';
import { encryptFile, decryptFile } from '../groupC';
import { crc32, textDiff, ChangeMonitor } from '../groupD';
import { virtualWindow, ThumbCache, fsck, coalesceEvents } from '../groupE';

describe('AURORA-10000 领域06 全量自检（F03126~F03750）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain06Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('fsModel 基础', () => {
  it('Vfs 去重与级联删除', () => {
    const v = Vfs.withRoot();
    v.add({ path: '/a', kind: 'dir' });
    v.add({ path: '/a/x', kind: 'file', size: 1 });
    expect(v.add({ path: '/a', kind: 'dir' })).toBe(false);
    expect(v.remove('/a')).toBe(2);
    expect(v.has('/a/x')).toBe(false);
  });
  it('孤立路径拒绝新增', () => {
    const v = Vfs.withRoot();
    expect(v.add({ path: '/no/parent', kind: 'file' })).toBe(false);
  });
});

describe('AI-26 逻辑核抽查', () => {
  it('搜索索引增量与重复文件', () => {
    const v = Vfs.demo();
    const idx = new NameIndex();
    const all = v.all().map((n) => ({ path: n.path, size: n.size }));
    expect(idx.updateAll(all)).toBe(all.length);
    expect(idx.updateAll(all)).toBe(0);
    v.update('/docs/a.txt', { size: 999 });
    expect(idx.updateAll([{ path: '/docs/a.txt', size: 999 }])).toBe(1);
  });
  it('内容搜索与隐私开关', () => {
    const v = Vfs.demo();
    expect(searchFiles(v, { query: 'aurora' }, { searchContent: true }).map((n) => n.path)).toContain('/docs/a.txt');
    expect(searchFiles(v, { query: 'aurora' }, { searchContent: false })).toHaveLength(0);
  });
  it('重复文件按哈希分组', () => {
    const v = Vfs.withRoot();
    v.add({ path: '/a', kind: 'file', hash: 'h1' });
    v.add({ path: '/b', kind: 'file', hash: 'h1' });
    v.add({ path: '/c', kind: 'file', hash: 'h2' });
    expect(findDuplicates(v).length).toBe(1);
    expect(findDuplicates(v)[0]!.length).toBe(2);
  });
  it('批量重命名与粘贴冲突', () => {
    expect(batchRename(['a.txt', 'b.md'], { prefix: 'P', pad: 2, seqOnly: true })).toEqual(['P01.txt', 'P02.md']);
    const v = Vfs.withRoot();
    v.add({ path: '/s', kind: 'dir' });
    v.add({ path: '/d', kind: 'dir' });
    v.add({ path: '/s/x', kind: 'file', size: 1 });
    v.add({ path: '/d/x', kind: 'file', size: 2 });
    expect(pasteWithConflict(v, '/s', '/d', 'skip').skipped).toBe(1);
    expect(pasteWithConflict(v, '/s', '/d', 'rename').renamed).toBe(1);
    expect(pasteWithConflict(v, '/s', '/d', 'overwrite').moved).toBe(1);
  });
});

describe('AI-27 逻辑核抽查', () => {
  it('treemap 覆盖整幅', () => {
    const rects = treemap([{ name: 'a', size: 3 }, { name: 'b', size: 1 }], 0, 0, 100, 100);
    expect(rects.reduce((s, r) => s + r.w * r.h, 0)).toBeCloseTo(10000);
  });
  it('回收站删除-还原-永久', () => {
    const v = Vfs.withRoot();
    v.add({ path: '/f', kind: 'file', size: 5, content: 'x' });
    const bin = new RecycleBin();
    expect(bin.delete(v, '/f')?.path).toBe('/f');
    expect(v.has('/f')).toBe(false);
    expect(bin.restore(v, ['/f'])).toBe(1);
    expect(v.has('/f')).toBe(true);
  });
  it('版本快照去重与回滚', () => {
    const v = Vfs.withRoot();
    v.add({ path: '/f', kind: 'file', size: 1, content: 'v2' });
    const vs = new VersionStore();
    expect(vs.snapshot('/f', 'v1', 1)).toBe(true);
    expect(vs.snapshot('/f', 'v1', 2)).toBe(false);
    expect(vs.restore(v, '/f', 1)).toBe(true);
    expect(v.get('/f')!.content).toBe('v1');
  });
  it('双向同步与块级增量', () => {
    const v = Vfs.withRoot();
    v.add({ path: '/a', kind: 'dir' });
    v.add({ path: '/b', kind: 'dir' });
    v.add({ path: '/a/x', kind: 'file', size: 1 });
    v.add({ path: '/b/y', kind: 'file', size: 2 });
    const s = twoWaySync(v, '/a', '/b');
    expect(s.copyToB).toEqual(['/a/x']);
    expect(s.copyToA).toEqual(['/b/y']);
    expect(blockDiff('aaaa', 'aaba', 2).changedBlocks).toBe(1);
  });
});

describe('AI-28 逻辑核抽查', () => {
  it('AES-256-GCM 加解密回环', async () => {
    const ct = await encryptFile('机密内容 secret', 'pass-1234');
    expect(ct).not.toContain('机密');
    await expect(decryptFile(ct, 'wrong')).rejects.toThrow();
    expect(await decryptFile(ct, 'pass-1234')).toBe('机密内容 secret');
  });
  it('CRC32 标准向量', () => {
    expect(crc32('abc')).toBe('352441c2');
  });
  it('文本 diff 与变更监视', () => {
    expect(textDiff('a\nb\nc', 'a\nx\nc')).toEqual({ added: 1, removed: 1, same: 2 });
    const v = Vfs.withRoot();
    v.add({ path: '/f', kind: 'file', content: 'v1' });
    const m = new ChangeMonitor();
    m.seed(v);
    v.update('/f', { content: 'v2' });
    expect(m.scan(v)).toEqual([{ path: '/f', change: 'mod' }]);
    expect(m.scan(v)).toHaveLength(0);
  });
});

describe('AI-30 逻辑核抽查', () => {
  it('虚拟化窗口与 LRU', () => {
    expect(virtualWindow(0, 320, 32, 20)).toEqual({ start: 0, end: 20 });
    expect(virtualWindow(5000, 320, 32, 500)).toEqual({ start: 151, end: 171 });
    const c = new ThumbCache(2);
    c.set('a', '1');
    c.set('b', '2');
    c.get('a');
    c.set('c', '3');
    expect(c.get('a')).toBe('1');
    expect(c.get('b')).toBeUndefined();
  });
  it('内核事件合并与 fsck', () => {
    expect(coalesceEvents([{ path: '/a', op: 'w' }, { path: '/a', op: 'w' }, { path: '/b', op: 'r' }])).toEqual([
      { path: '/a', ops: ['w', 'w'] },
      { path: '/b', ops: ['r'] },
    ]);
    expect(fsck(Vfs.demo()).ok).toBe(true);
  });
});
