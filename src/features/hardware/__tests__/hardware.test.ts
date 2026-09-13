// AURORA-10000: AI-36~AI-40 批次（领域08 系统集成与硬件）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain08Checks, hwCapabilities, hwTutorials } from '../checks';
import { Battery, ChargeLimit, DiskLayout, BitLockerVol } from '../groupA';
import { BootOrder, HidFilter, SensorBus, VmRegistry, OneShotSandbox } from '../groupB';
import { PerfGovernor, TpmManager, resetPlan } from '../groupC';
import { CameraSession, SpatialAudio, ScanSession, lassoHit } from '../groupD';
import { FanCurve, Hotspot, Watchdog, DumpStore, eventCorrelation } from '../groupE';

describe('AURORA-10000 领域08 全量自检（F04376~F05000）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain08Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
  it('能力位注册表：70 个预留位登记且保留', () => {
    const reg = hwCapabilities();
    expect(reg.isReserved('F04394')).toBe(true);
    expect(reg.has('F04394')).toBe(true);
    expect(reg.has('F99999')).toBe(false);
  });
  it('教学中心 26 篇族教学全部交付', () => {
    expect(hwTutorials().count).toBe(26);
  });
});

describe('AI-36 逻辑核抽查', () => {
  it('电池健康与充电养护', () => {
    const b = new Battery(50000, 42000, 100, 0);
    expect(b.healthPct).toBe(84);
    expect(b.wearPct).toBe(16);
    const cl = new ChargeLimit(80);
    expect(cl.tick(80)).toBe('hold');
  });
  it('分区管理不变量', () => {
    const d = new DiskLayout(1024 * 1024);
    expect(d.create('C', 0, 100 * 1024)).toBe(true);
    expect(d.create('D', 90 * 1024, 100 * 1024)).toBe(false);
    expect(d.create('D', 100 * 1024, 100 * 1024)).toBe(true);
  });
  it('BitLocker 锁定-解锁闭环', () => {
    const v = new BitLockerVol();
    expect(v.lock()).toBe(false);
    v.enable('k');
    v.finishEncrypt();
    v.lock();
    expect(v.unlock('k')).toBe(true);
  });
});

describe('AI-37 逻辑核抽查', () => {
  it('启动顺序与 HID 过滤', () => {
    const o = new BootOrder([['a', 'A', 'uefi'], ['b', 'B', 'uefi']]);
    o.moveUp('b');
    expect(o.entries[0]!.id).toBe('b');
    const hid = new HidFilter(['ok']);
    expect(hid.onPlug('BAD')).toBe('block');
    expect(hid.onPlug('ok')).toBe('allow');
    expect(hid.onPlug('new')).toBe('confirm');
  });
  it('传感总线与虚拟机快照', () => {
    const s = new SensorBus();
    s.attach('gyro');
    s.write('gyro', 270);
    expect(s.posture()).toBe('tent');
    expect(s.read('gps')).toBeNull();
    const r = new VmRegistry();
    r.create({ id: 'v', name: 'n', vcpu: 2, ramMB: 1024, diskGB: 32 });
    r.snapshot('v', 's1');
    expect(r.snapsOf('v')).toEqual(['s1']);
  });
  it('一次性沙箱即焚', () => {
    const sb = new OneShotSandbox();
    expect(sb.start()).not.toBeNull();
    expect(sb.start()).toBeNull();
    expect(sb.stop().destroyed).toBe(true);
    expect(sb.running).toBe(false);
  });
});

describe('AI-38 逻辑核抽查', () => {
  it('性能治理器游戏模式', () => {
    const g = new PerfGovernor();
    g.enterGame();
    expect(g.isFrozen('updater')).toBe(true);
    g.exitGame();
    expect(g.isFrozen('updater')).toBe(false);
  });
  it('TPM 重置流程', () => {
    const t = new TpmManager(true);
    expect(t.requestReset(false)).toBe(false);
    expect(t.requestReset(true)).toBe(true);
    expect(t.completeReset()).toBe(true);
  });
  it('重置方案与崩溃关联', () => {
    expect(resetPlan('full', ['a'], ['b']).keep).toEqual([]);
    expect(eventCorrelation([{ at: 0, kind: 'temp-high' }, { at: 500, kind: 'crash' }])).toBe('temp-high');
  });
});

describe('AI-39 逻辑核抽查', () => {
  it('相机连拍与圈选', () => {
    const c = new CameraSession();
    expect(c.burst(3)).toHaveLength(3);
    expect(lassoHit([{ x: 0, y: 0 }, { x: 8, y: 0 }, { x: 8, y: 8 }, { x: 0, y: 8 }], { x: 4, y: 4 })).toBe(true);
  });
  it('空间声像与扫描会话', () => {
    const sp = new SpatialAudio();
    sp.toggle(true);
    expect(sp.panForWindow(-1)).toEqual({ l: 100, r: 0 });
    const s = new ScanSession();
    s.addPage('a');
    expect(s.mergePdf().pages).toBe(1);
    expect(s.cancel()).toBe(1);
  });
});

describe('AI-40 逻辑核抽查', () => {
  it('风扇曲线滞回与热点', () => {
    const f = new FanCurve([{ tempC: 40, rpm: 800 }, { tempC: 80, rpm: 2000 }]);
    expect(f.rpmAt(90)).toBe(2000);
    const h = new Hotspot();
    h.toggle(true);
    expect(h.connect('aa')).toBe(true);
    expect(h.connect('aa')).toBe(false);
  });
  it('看门狗与转储分析', () => {
    const w = new Watchdog(1000);
    w.beat(0);
    expect(w.check(900).stalled).toBe(false);
    expect(w.check(1100).recover).toBe(true);
    const d = new DumpStore();
    d.add('A', 1);
    d.add('A', 2);
    expect(d.analyze()).toEqual({ top: 'A', count: 2 });
  });
});
