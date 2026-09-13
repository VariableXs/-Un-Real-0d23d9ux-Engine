// AURORA-10000: AI-51~AI-55 批次（领域11 开放生态）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain11Checks } from '../checks';
import { parseManifest, PermissionDialog, runFlow, parseDeepLink, storeSearch } from '../groupA';
import { ApiAuth, RateLimiter } from '../groupB';
import { FeedbackBoard, nps } from '../groupC';
import { ProtocolRegistry, ModelManager } from '../groupD';
import { resolveSyncConflict, OfflineQueue } from '../groupE';

describe('AURORA-10000 领域11 全量自检（F06251~F06875）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain11Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-51 插件与自动化核', () => {
  it('清单解析与权限', () => {
    const m = parseManifest(JSON.stringify({ id: 'abc', name: 'A', apiVersion: 3, permissions: ['files'], entry: 'm.js' }));
    expect((m as { id?: string }).id).toBe('abc');
    const dlg = new PermissionDialog();
    expect(dlg.approve('x', 'files', 1)).toBe(false);
    dlg.request('x', 'files', 1);
    expect(dlg.approve('x', 'files', 2)).toBe(true);
  });
  it('流程引擎分支与循环', () => {
    const r = runFlow([
      { op: 'set-var', name: 'v', varName: 'n', value: '2' },
      { op: 'if', name: 'x', condition: (v) => v['n'] === '2', branch: { then: [{ op: 'action', name: 'yes' }], else: [{ op: 'action', name: 'no' }] } },
      { op: 'loop', name: 'l', maxIterations: 2, body: [{ op: 'action', name: 'tick' }] },
    ]);
    expect(r.error).toBeNull();
    expect(r.executed).toContain('yes');
    expect(r.executed.filter((s) => s === 'tick')).toHaveLength(2);
  });
  it('深链解析', () => {
    expect(parseDeepLink('varix://a/b?k=1')).toEqual({ target: 'a', action: 'b', params: { k: '1' } });
    expect(parseDeepLink('nope://x')).toEqual({ error: 'bad-scheme' });
  });
});

describe('AI-52 API 与社区核', () => {
  it('令牌与限流', () => {
    const auth = new ApiAuth();
    const t = auth.issue(['w'], 1);
    expect(auth.check(t.token, 'w')).toBe(true);
    auth.revoke(t.token);
    expect(auth.check(t.token, 'w')).toBe(false);
    const rl = new RateLimiter(2, 0);
    expect(rl.allow('a', 1)).toBe(true);
    expect(rl.allow('a', 1)).toBe(true);
    expect(rl.allow('a', 1)).toBe(false);
  });
  it('商店纠错搜索', () => {
    const ls = [{ id: 'a', name: 'Code', category: 'x', sizeMb: 1, version: '1', versionHistory: [], permissions: [], privacyLabel: 'local-only' as const, minOs: 'v', rating: 5, downloads: 1, releasedAt: 0, free: true }];
    expect(storeSearch(ls, 'Codf')).toHaveLength(1);
  });
});

describe('AI-53 反馈与联盟核', () => {
  it('反馈去重与状态机', () => {
    const b = new FeedbackBoard();
    const f1 = b.submit({ category: 'bug', title: 'T', body: 'a', screenshot: false, logsAttached: false, anonymous: false, createdAt: 1, stack: 's' });
    const f2 = b.submit({ category: 'bug', title: 'T', body: 'b', screenshot: false, logsAttached: false, anonymous: false, createdAt: 2, stack: 's' });
    expect(f2.deduped).toBe(true);
    expect(b.transition(f1.item.id, 'fixed')).toBe(false);
  });
  it('NPS', () => {
    expect(nps([10, 9, 9, 6, 6])).toBe(20);
  });
});

describe('AI-54 协议与 AI 核', () => {
  it('协议注册表', () => {
    const reg = new ProtocolRegistry([
      { number: 1, name: 'p1', version: 1, status: 'stable', impl: 'desktop' },
      { number: 1, name: 'dup', version: 1, status: 'stable', impl: 'desktop' },
    ]);
    expect(reg.all()).toHaveLength(1);
  });
  it('模型管理器只收 GGUF 且默认沙箱', () => {
    const mm = new ModelManager();
    expect(mm.register({ id: 'm', format: 'gguf', sizeMb: 1, params: '', quant: '', granted: false, sandboxed: false })).toBe(true);
    expect(mm.list()[0]!.sandboxed).toBe(true);
  });
});

describe('AI-55 自托管核', () => {
  it('同步冲突三路合并', () => {
    const local = { mtime: 1, content: 'B2' };
    const remote = { mtime: 2, content: 'R' };
    const base = { content: 'B' };
    expect(resolveSyncConflict(local, remote, base, 'local-wins')).toEqual({ content: 'B2', conflict: true });
    expect(resolveSyncConflict({ mtime: 1, content: 'B' }, remote, base, 'local-wins')).toEqual({ content: 'R', conflict: false });
  });
  it('离线队列', () => {
    const q = new OfflineQueue();
    q.enqueue('op', 'p');
    expect(q.pending()).toBe(1);
    expect(q.drain()).toHaveLength(1);
    expect(q.pending()).toBe(0);
  });
});
