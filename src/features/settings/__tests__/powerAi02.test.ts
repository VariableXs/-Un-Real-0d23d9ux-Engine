// UNREAL-X AI-02：电源状态剧场六大逻辑模块核测试（族0011/0012/0013/0016/0017/0018/0019/0020），勿删。
import { describe, expect, it } from 'vitest';
import {
  CEREMONY_NARRATIVES, CEREMONY_PROFILES, CeremonyRunner, DEFAULT_CEREMONY_ID,
  ceremonyPercent, findCeremony, findNarrative,
} from '../../oobe/powerTheater';
import {
  DEFAULT_WAKE_ID, WAKE_PROFILES, WakeTheater, findWake, wakePercent,
} from '../../oobe/powerTheater';
import { HiberArchive, HIBER_APP_MAX, HIBER_FORMAT, HIBER_SNAPSHOTS } from '../hiberArchive';
import { PowerEventStream, POWER_EVENT_CAPACITY, POWER_EVENT_TYPES, PowerEventStream as Stream } from '../powerEvents';
import { GAUGE_BUDGETS, GAUGE_CAPACITY, PowerGauge } from '../powerGauge';
import { WARMUP_BUDGET_MAX, WarmupPlanner } from '../warmupPlan';
import { BootQuiet, QUIET_TIERS, clampQuietWindow, findQuiet } from '../bootQuiet';
import { EGG_TRIGGER_MAX, EggKeeper, POWER_EGGS } from '../powerEggs';

describe('族0011 关机重启仪式 2.0', () => {
  it('端到端推进到 done 且 100%', () => {
    const r = new CeremonyRunner('restart', 'farewell');
    r.start();
    while (r.tick() !== 'done') { /* 推进 */ }
    expect(ceremonyPercent(r)).toBe(100);
  });
  it('暂停后续作保留进度', () => {
    const r = new CeremonyRunner('shutdown', 'cinematic');
    r.start();
    r.tick();
    const mid = r.stage;
    r.pause();
    expect(r.resume()).toBe(true);
    while (r.tick() !== 'done') { /* 推进 */ }
    expect(r.stage).toBeGreaterThanOrEqual(mid);
  });
  it('失败给出叙事码 PW-402', () => {
    const r = new CeremonyRunner('shutdown', 'brisk');
    r.start();
    expect(r.tick(1)).toBe('failed');
    expect(r.logs[0]!.code).toBe('PW-402');
    expect(findNarrative('PW-402').next.length).toBeGreaterThan(0);
  });
  it('档位与非法档钳制', () => {
    expect(CEREMONY_PROFILES.length).toBe(5);
    expect(DEFAULT_CEREMONY_ID).toBe('balanced');
    expect(findCeremony('nope').id).toBe('balanced');
    expect(new CeremonyRunner('shutdown', 'warp').clamped).toBe(1);
  });
  it('快照往返与坏载荷钳制', () => {
    const r = new CeremonyRunner('shutdown', 'instant');
    r.start();
    r.tick();
    const q = new CeremonyRunner();
    expect(q.restore(r.snapshot())).toBe(true);
    expect(q.stage).toBe(r.stage);
    const bad = new CeremonyRunner();
    expect(bad.restore('not json')).toBe(false);
    expect(bad.clamped).toBe(1);
  });
  it('低电量每 tick 1 阶', () => {
    const r = new CeremonyRunner('shutdown', 'balanced', true);
    r.start();
    r.tick();
    expect(r.stage).toBe(1);
  });
  it('叙事目录 ≥5 条', () => {
    expect(CEREMONY_NARRATIVES.length).toBeGreaterThanOrEqual(5);
  });
});

describe('族0012 睡眠唤醒剧场 2.0', () => {
  it('唤醒端到端到 awake', () => {
    const w = new WakeTheater('dawn');
    w.wake();
    while (w.tick() !== 'awake') { /* 推进 */ }
    expect(wakePercent(w)).toBe(100);
  });
  it('取消回睡眠', () => {
    const w = new WakeTheater('balancedWake');
    w.wake();
    w.tick();
    w.cancel();
    expect(w.phase).toBe('sleeping');
    expect(w.stage).toBe(0);
  });
  it('档位与非法档钳制', () => {
    expect(WAKE_PROFILES.length).toBe(5);
    expect(DEFAULT_WAKE_ID).toBe('balancedWake');
    expect(findWake('nope').id).toBe('balancedWake');
  });
  it('面纱透明度随进度变化且 reduce-motion 恒 1', () => {
    const w = new WakeTheater('sunrise');
    w.wake();
    w.tick();
    const a = w.veilAlpha(false);
    expect(a).toBeGreaterThan(0.2);
    expect(a).toBeLessThan(1);
    expect(w.veilAlpha(true)).toBe(1);
  });
  it('瞬醒一拍即醒', () => {
    const w = new WakeTheater('blink');
    w.wake();
    expect(w.tick()).toBe('awake');
  });
});

describe('族0013 休眠档案', () => {
  it('登记/恢复闭环', () => {
    const h = new HiberArchive();
    h.snapshot('离机前', [{ id: 'files', focus: 9 }], { preset: 'flow', columns: 3 });
    expect(h.caption()).toContain('离机前');
    expect(h.restorePoint()!.apps[0]!.id).toBe('files');
  });
  it('环形容量 3', () => {
    const h = new HiberArchive();
    for (let i = 0; i < 5; i += 1) h.snapshot(`s${i}`, []);
    expect(h.snapshots.length).toBe(HIBER_SNAPSHOTS);
    expect(h.snapshots[0]!.label).toBe('s4');
  });
  it('应用数与焦点钳制', () => {
    const h = new HiberArchive();
    h.snapshot('x', Array.from({ length: 12 }, (_, i) => ({ id: `a${i}`, focus: 42 })));
    expect(h.snapshots[0]!.apps.length).toBe(HIBER_APP_MAX);
    expect(h.snapshots[0]!.apps[0]!.focus).toBe(9);
    expect(h.clamped).toBe(1);
  });
  it('导出/导入/坏载荷三通道', () => {
    const h = new HiberArchive();
    h.snapshot('a', [{ id: 'x', focus: 1 }]);
    const fresh = new HiberArchive();
    expect(fresh.import(h.export())).toBe(true);
    expect(fresh.snapshots[0]!.label).toBe('a');
    expect(fresh.import('{bad')).toBe(false);
    expect(fresh.import(JSON.stringify({ fmt: 'vx-hiber/0', snaps: [] }))).toBe(false);
    expect(HIBER_FORMAT).toBe('vx-hiber/1');
  });
  it('半成品续作与净身', () => {
    const h = new HiberArchive();
    expect(h.resume()).toBe(false);
    h.snapshot('half', [], {}, false);
    expect(h.resume()).toBe(true);
    h.wipe();
    expect(h.snapshots.length).toBe(0);
  });
});

describe('族0016 电源事件', () => {
  it('记录/订阅/退订闭环', () => {
    const s = new Stream();
    let seen = 0;
    const unsub = s.subscribe(() => { seen += 1; });
    s.record('wake', 1);
    s.record('sleep', 2);
    expect(seen).toBe(2);
    unsub();
    s.record('wake', 3);
    expect(seen).toBe(2);
    expect(s.subscriberCount()).toBe(0);
  });
  it('非法类型记钳制', () => {
    const s = new PowerEventStream();
    expect(s.record('boom', 1)).toBeNull();
    expect(s.clamped).toBe(1);
  });
  it('环形容量 32 与详情截断', () => {
    const s = new PowerEventStream();
    for (let i = 0; i < 40; i += 1) s.record('wake', i, 'd'.repeat(200));
    expect(s.events.length).toBe(POWER_EVENT_CAPACITY);
    expect(s.events[0]!.detail.length).toBe(120);
    expect(s.events[0]!.stamp).toBe(39);
  });
  it('类型白名单 8 类', () => {
    expect(POWER_EVENT_TYPES.length).toBe(8);
  });
  it('lastOf 与过滤查询', () => {
    const s = new PowerEventStream();
    s.record('sleep', 1);
    s.record('wake', 2);
    expect(s.lastOf('sleep')!.stamp).toBe(1);
    expect(s.query(['wake']).length).toBe(1);
  });
});

describe('族0017 性能仪表 2.0', () => {
  it('采样/均值/健康度', () => {
    const g = new PowerGauge();
    g.sample(1, { battery: 60, cpu: 50, thermal: 50, runtime: 60 });
    g.sample(2, { battery: 150, cpu: -3, thermal: 95, runtime: 8 });
    expect(g.avg('battery')).toBe(80);
    expect(g.health('battery')).toBe('ok');
    expect(g.health('thermal')).toBe('warn');
    expect(g.health('runtime')).toBe('warn');
    expect(g.health('cpu')).toBe('ok');
    expect(g.degraded()).toBe(true);
  });
  it('越界钳制与环形窗口', () => {
    const g = new PowerGauge();
    for (let i = 0; i < 20; i += 1) g.sample(i, { battery: 300, cpu: -9 });
    expect(g.samples.length).toBe(GAUGE_CAPACITY);
    expect(g.samples[0]!.values.battery).toBe(100);
    expect(g.samples[0]!.values.cpu).toBe(0);
  });
  it('预算表与摘要', () => {
    expect(GAUGE_BUDGETS.thermal.bad).toBe(88);
    expect(new PowerGauge().caption()).toContain('电量');
  });
});

describe('族0018 预热编排', () => {
  it('优先级出队到 done', () => {
    const p = new WarmupPlanner();
    p.enqueue({ id: 'b', type: 'index-scan', priority: 9, budgetMs: 10 });
    p.enqueue({ id: 'a', type: 'network', priority: 1, budgetMs: 10 });
    p.begin();
    while (p.tick() !== 'done') { /* 执行 */ }
    expect(p.doneIds[0]).toBe('a');
    expect(p.doneIds.length).toBe(2);
  });
  it('预算与类型钳制', () => {
    const p = new WarmupPlanner();
    expect(p.enqueue({ id: 'x', type: 'network', priority: 1, budgetMs: 99999 })).toBe(true);
    expect(p.queue[0]!.budgetMs).toBe(WARMUP_BUDGET_MAX);
    expect(p.enqueue({ id: 'y', type: 'boom' as never, priority: 1, budgetMs: 1 })).toBe(false);
    expect(p.clamped).toBe(1);
  });
  it('低电量挂起与恢复', () => {
    const p = new WarmupPlanner(true);
    p.enqueue({ id: 'a', type: 'notify-sync', priority: 5, budgetMs: 50 });
    p.begin();
    expect(p.tick()).toBe('held');
    expect(p.release()).toBe(true);
    expect(p.tick()).toBe('held'); // 电量未回升，继续挂起（守护不失效）
    p.lowBattery = false;
    expect(p.release()).toBe(true);
    expect(p.tick()).toBe('done');
  });
  it('取消与净身', () => {
    const p = new WarmupPlanner();
    p.enqueue({ id: 'a', type: 'network', priority: 1, budgetMs: 5 });
    expect(p.cancel('a')).toBe(true);
    expect(p.cancel('zzz')).toBe(false);
    p.reset();
    expect(p.queue.length).toBe(0);
  });
});

describe('族0019 启动降噪', () => {
  it('窗口内抑制与窗口外放行', () => {
    const q = new BootQuiet('hushed', 10);
    expect(q.decision('notify').suppressed).toBe(true);
    for (let i = 0; i < 11; i += 1) q.tick();
    expect(q.decision('notify').suppressed).toBe(false);
  });
  it('档位与窗口钳制', () => {
    expect(QUIET_TIERS.length).toBe(4);
    expect(findQuiet('nope').id).toBe('off');
    expect(clampQuietWindow(1000)).toBe(300);
    expect(clampQuietWindow(-1)).toBe(0);
  });
  it('off 档 = 现状全放行', () => {
    const q = new BootQuiet('off', 60);
    expect(q.decision('notify').suppressed).toBe(false);
    expect(q.decision('sound').suppressed).toBe(false);
  });
});

describe('族0020 彩蛋层', () => {
  it('默认关 = 不触发', () => {
    const k = new EggKeeper();
    k.celebrate('shutdown');
    expect(k.lineFor('shutdown')).toBe('');
  });
  it('开启后按阈值触发并可屏蔽', () => {
    const k = new EggKeeper();
    k.enabled = true;
    k.celebrate('wake');
    expect(k.lineFor('wake')).toContain('晨');
    expect(k.dismiss('dawn')).toBe(true);
    expect(k.lineFor('wake')).toBe('');
    expect(k.restore('dawn')).toBe(true);
    expect(k.dismiss('ghost')).toBe(false);
  });
  it('计数钳制 99 与净身', () => {
    const k = new EggKeeper();
    for (let i = 0; i < 200; i += 1) k.celebrate('shutdown');
    expect(k.shutdowns).toBe(EGG_TRIGGER_MAX);
    k.reset();
    expect(k.shutdowns).toBe(0);
    expect(k.enabled).toBe(false);
  });
  it('彩蛋目录 ≥5 枚', () => {
    expect(POWER_EGGS.length).toBeGreaterThanOrEqual(5);
  });
});
