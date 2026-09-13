// AURORA-10000: AI-41~AI-45 批次（领域09 兼容性防线）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain09Checks } from '../checks';
import { detectAntiCheat, embedVerdict, cefChannelFor, fitEmbedRect } from '../groupA';
import { HookAuditor, routeSelection, btCodecNegotiate } from '../groupB';
import { runCheckup, compatScore, BootMenu } from '../groupC';
import { CompatTelemetry, RegistryVirtualization, mergeGate, desensitize } from '../groupD';
import { PermissionCenter, ResourceLedger, SchedulerPanel, certify } from '../groupE';

describe('AURORA-10000 领域09 全量自检（F05001~F05625）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain09Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-41 逻辑核抽查', () => {
  it('嵌入裁决与反作弊检测', () => {
    const w = { id: 1, className: 'Chrome_WidgetWin_1', title: 'x', processName: 'steam.exe' };
    expect(embedVerdict(w, new Set(), new Set(['steam.exe']), new Map()) === 'embed').toBe(true);
    expect(detectAntiCheat(['BEService.exe', 'GameMon.exe'])).toEqual(['BattlEye', 'nProtect']);
  });
  it('CEF 通道与 DPI 重嵌', () => {
    expect(cefChannelFor({ name: 'steam.exe' })).toBe('dedicated');
    expect(fitEmbedRect({ w: 800, h: 600 }, 192, 96)).toEqual({ w: 1600, h: 1200 });
  });
});

describe('AI-42 逻辑核抽查', () => {
  it('钩子审计与路由选择', () => {
    const a = new HookAuditor();
    a.addWhitelist('im.dll');
    a.register({ name: 'im', kind: 'keyboard', module: 'im.dll' });
    a.register({ name: 'evil', kind: 'keyboard', module: 'evil.dll' });
    expect(a.untrusted('keyboard').map((h) => h.module)).toEqual(['evil.dll']);
    expect(routeSelection([{ iface: 'eth0', metric: 5, dest: '192.168.0.0/16' }, { iface: 'eth1', metric: 1, dest: '0.0.0.0/0' }], '192.168.1.5')).toBe('eth0');
  });
  it('蓝牙编解码回退', () => {
    expect(btCodecNegotiate(['SBC', 'aptX'], 'LDAC')).toBe('aptX');
    expect(btCodecNegotiate(['SBC'], 'LDAC')).toBe('SBC');
  });
});

describe('AI-43 逻辑核抽查', () => {
  it('体检评分与启动菜单', () => {
    const items = runCheckup({ brokenExt: [], driverClash: ['a.sys'], missingRuntime: [] });
    expect(compatScore(items)).toBe(80);
    const menu = new BootMenu();
    menu.add({ id: 'w', label: 'VarixOS', kind: 'windows', default: true, timeoutSec: 5 });
    menu.add({ id: 'l', label: 'Linux', kind: 'linux', default: false, timeoutSec: 5 });
    expect(menu.setDefault('l')).toBe(true);
    expect(menu.defaultEntry?.id).toBe('l');
  });
});

describe('AI-44 逻辑核抽查', () => {
  it('崩溃聚类与注册表虚拟化', () => {
    const t = new CompatTelemetry();
    t.record({ time: 1, kind: 'crash', app: 'a', detail: 'x' });
    t.record({ time: 2, kind: 'crash', app: 'b', detail: 'x' });
    t.record({ time: 3, kind: 'yield', app: 'a' });
    expect(t.crashClusters()).toHaveLength(1);
    const reg = new RegistryVirtualization();
    expect(reg.write('HKLM', 'SOFTWARE\\v\\k', '1').redirected).toBe(true);
    expect(reg.write('HKCU', 'Software\\v\\k', '1').redirected).toBe(false);
  });
  it('脱敏与合并门禁', () => {
    expect(desensitize('C:\\secret.txt')).toBe('<drive>\\<redacted>');
    expect(mergeGate(false).allowMerge).toBe(false);
    expect(mergeGate(true).reason).toContain('全绿');
  });
});

describe('AI-45 逻辑核抽查', () => {
  it('权限中心与资源账本', () => {
    const p = new PermissionCenter();
    expect(p.grant('app', 'camera')).toBe(true);
    expect(p.grant('app', 'camera')).toBe(false);
    expect(p.revoke('app', 'camera')).toBe(true);
    const l = new ResourceLedger();
    l.add('cpu', 'a', 10);
    l.add('cpu', 'b', 30);
    expect(l.top('cpu', 1)).toEqual([['b', 30]]);
  });
  it('调度错峰与认证', () => {
    const s = new SchedulerPanel();
    s.add({ name: 'a', kind: 'backup', atHour: 3, estMin: 40, canDefer: true });
    s.add({ name: 'b', kind: 'index', atHour: 3, estMin: 40, canDefer: true });
    expect(new Set(s.stagger().map((t) => t.atHour)).size).toBe(2);
    expect(certify('x', [{ id: 'a', weight: 1, pass: true }]).level).toBe('ready');
  });
});
