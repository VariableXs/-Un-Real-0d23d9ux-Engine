// AURORA-10000: AI-61~AI-65 批次（领域13 声音与通知）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain13Checks } from '../checks';
import { SystemSoundLibrary, SoundPackManager, SoundLevelSystem, AmbienceMixer, SoundAccessibility } from '../groupA';
import { NotificationIntelligence, NotificationTemplateRegistry, PopupEtiquette, DndSystem, AudioDebugConsole } from '../groupB';
import { NotificationWorkflow, MediaControlHub, AlarmSystem, TimerStopwatch } from '../groupC';
import { ContentProtector, maskSensitive, parseReminderText, ReminderSystem, SoundFavorites } from '../groupD';
import { NotifyPerformanceGuard } from '../groupE';

describe('AURORA-10000 领域13 全量自检（F07501~F08125）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain13Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-61 声音设计核', () => {
  it('系统声音库与静默原则', () => {
    const lib = new SystemSoundLibrary();
    lib.setMasterMuted(true);
    expect(lib.play('notify', 1)).toBe(false);
    lib.setMasterMuted(false);
    expect(lib.play('notify', 2)).toBe(true);
    expect(lib.playHistory().length).toBe(2);
  });
  it('声包定时换包与混搭', () => {
    const mgr = new SoundPackManager();
    mgr.addSchedule({ fromHour: 22, toHour: 6, packId: 'zen' });
    expect(mgr.packForHour(23)).toBe('zen');
    expect(mgr.packForHour(12)).toBe('default');
  });
  it('分级疲劳保护与场景静默', () => {
    const sys = new SoundLevelSystem();
    let audible = 0;
    for (let i = 0; i < 10; i++) if (sys.underFatigueLimit(1000 + i)) audible++;
    expect(audible).toBe(6);
    expect(sys.mutedByContext('notify', { meeting: true })).toBe(true);
    expect(sys.mutedByContext('alarm', { meeting: true })).toBe(false);
  });
  it('声景助眠渐弱与声音历史', () => {
    const m = new AmbienceMixer();
    m.setMasterVolume(1);
    m.startSleepTimer(45 * 60_000, 0, true);
    expect(m.currentVolume(0)).toBe(1);
    expect(m.currentVolume(45 * 60_000)).toBe(0);
    const ax = new SoundAccessibility({ envCaptions: true });
    expect(ax.captionFor('doorbell', 1)).toBe('门铃响');
  });
});

describe('AI-62 智能通知核', () => {
  it('价值评分与广告拦截', () => {
    const ni = new NotificationIntelligence();
    const base = { id: 'n', app: 'mail', title: 't', body: 'b', category: 'notify' as const, at: 0 };
    expect(ni.isBlocked({ ...base, category: 'ad' })).toBe(true);
    expect(ni.route({ ...base, category: 'alarm' }).score).toBe(1);
  });
  it('模板 A/B 与弹出礼仪堆叠', () => {
    const reg = new NotificationTemplateRegistry();
    reg.startAb('k', 'minimal', 'card');
    reg.recordAb('k', 'a', true);
    reg.recordAb('k', 'b', false);
    expect(reg.abWinner('k')).toBe('a');
    const et = new PopupEtiquette();
    et.push('1'); et.push('2'); et.push('3');
    expect(et.push('4')).toBe('1');
    expect(!et.canPopup({ typing: true }, 'high')).toBe(true);
  });
  it('勿扰重呼放行与错过摘要', () => {
    const dnd = new DndSystem();
    dnd.enable('manual', 0);
    const n = { id: 'm', app: 'a', title: 't', at: 0 };
    expect(dnd.receive(n, 1)).toBe(false);
    dnd.shouldPassThrough('a:t', 2);
    dnd.shouldPassThrough('a:t', 3);
    expect(dnd.receive(n, 4)).toBe(true);
    dnd.disable(5);
    expect(dnd.sessionsCount()).toBe(1);
  });
  it('调试控制台路径与响度', () => {
    const dbg = new AudioDebugConsole();
    const r = dbg.probePath([{ device: 'spk', muted: true, volume: 1 }]);
    expect(r.blockers).toContain('spk:muted');
    expect(dbg.normalizeAdvice(-16)).toContain('达标');
  });
});

describe('AI-63 工作流与闹钟核', () => {
  it('通知条件弹与稍后队列', () => {
    const wf = new NotificationWorkflow();
    wf.addLater({ id: '1', title: 'x', app: 'a', at: 0 });
    expect(wf.processQueue()).toHaveLength(1);
  });
  it('媒体独占仲裁与拔耳机暂停', () => {
    const hub = new MediaControlHub();
    hub.register({ id: 'a', app: 'a', title: 'A', artist: '', positionSec: 0, durationSec: 10, playing: true });
    hub.register({ id: 'b', app: 'b', title: 'B', artist: '', positionSec: 0, durationSec: 10, playing: true });
    hub.activate('a');
    expect(hub.arbitrate()!.id).toBe('a');
    expect(hub.sourcesAll().filter((s) => s.playing)).toHaveLength(1);
    expect(hub.onHeadphone(false)).toBe('pause');
  });
  it('闹钟贪睡上限与算术关闭', () => {
    const sys = new AlarmSystem();
    sys.add({ id: 'a', hour: 7, minute: 0, label: 'x', enabled: true, repeatDays: [], ringtone: 'r', volume: 1, rampMinutes: 1, snoozeMinutes: 5, snoozeLimit: 1, dismissMode: 'math', bypassDnd: false });
    expect(sys.snooze('a', 0)).toBe(true);
    expect(sys.snooze('a', 1)).toBe(false);
    const ch = sys.mathChallenge(3);
    expect(ch.q).toContain('×');
  });
  it('番茄相位与间歇训练', () => {
    const ts = new TimerStopwatch();
    expect(ts.pomodoroPhase(0).phase).toBe('work');
    expect(ts.pomodoroPhase(27).phase).toBe('short-break');
    expect(ts.intervalPhase(35, 30, 15)).toBe('rest');
  });
});

describe('AI-64 隐私与工程核', () => {
  it('内容保护锁屏与脱敏', () => {
    const c = new ContentProtector();
    expect(c.lockscreenView('mail', '秘密')).toBe('mail');
    expect(maskSensitive('密码: 123').includes('***')).toBe(true);
  });
  it('提醒自然语言解析', () => {
    const now = new Date(2026, 8, 13, 10, 0, 0).getTime();
    expect(parseReminderText('20分钟后', now).atMs).toBe(now + 20 * 60_000);
    const r = new ReminderSystem();
    expect(r.addFromText('x', '明早8点', now)?.atMs).toBeGreaterThan(now);
  });
  it('收藏随机与轮换选取', () => {
    const fav = new SoundFavorites();
    for (const id of ['a', 'b', 'c']) {
      expect(fav.add({ id, name: id, source: 'builtin', durationMs: 100, fadeInMs: 0, fadeOutMs: 0, loop: false })).toBe(true);
    }
    fav.setRotation(['a', 'b', 'c']);
    expect(fav.pick('random', 7)).toBe('b');
    expect(fav.pick('rotate')).toBe('a');
  });
});

describe('AI-65 收官性能核', () => {
  it('虚拟化与预算', () => {
    const g = new NotifyPerformanceGuard();
    expect(g.withinRenderBudget(10)).toBe(true);
    expect(g.renderedRows(10000, 500000, 560)).toBeLessThan(200);
    g.buildIndex(Array.from({ length: 1000 }, (_, i) => `n${i}`));
    expect(g.indexOf('n999')).toBe(999);
  });
});
