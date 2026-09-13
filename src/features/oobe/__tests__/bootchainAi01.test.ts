// UNREAL-X AI-01：src/features/oobe 六大模块逻辑核测试，勿删。
import { describe, expect, it } from 'vitest';
import { RepairWorkshop, REPAIR_STRATEGIES, findStrategy, repairPercent } from '../repairWorkshop';
import { RecoveryReborn, RECOVERY_TOOLS, snapshotCaption } from '../recoveryReborn';
import { buildActs, parseBootLog, resolveTheaterStyle, spotlight } from '../bootLogTheater';
import { FAIL_NARRATIVES, findNarrative, narrate, shouldEscalate } from '../bootFailNarrative';
import { PACING_PROFILES, PacingMixer, clampCountdown, clampMotion, findPacing } from '../bootPacing';
import { MultiBootTheater, BOOT_ENTRY_MAX } from '../multiBootTheater';

describe('族0003 启动修复工坊', () => {
  it('端到端推进到 done 且 100%', () => {
    const w = new RepairWorkshop('rebuild-bcd');
    w.start();
    while (w.tick() !== 'done') { /* 推进 */ }
    expect(repairPercent(w)).toBe(100);
    expect(w.logs.length).toBe(findStrategy('rebuild-bcd').steps);
  });
  it('失败给出错误码并支持续作', () => {
    const w = new RepairWorkshop('verify-loader');
    w.start();
    expect(w.tick(2)).toBe('failed');
    expect(w.logs.some((l) => l.code === 'BC-501')).toBe(true);
    expect(w.resume()).toBe(true);
    while (w.tick() !== 'done') { /* 续作 */ }
    expect(repairPercent(w)).toBe(100);
  });
  it('低功耗档每 tick 1 步', () => {
    const w = new RepairWorkshop('repair-entry', 2, true);
    w.start();
    w.tick();
    expect(w.step).toBe(1);
  });
  it('未知策略与重试钳制', () => {
    expect(findStrategy('nope').id).toBe('repair-entry');
    expect(new RepairWorkshop('x', 9).maxRetries).toBe(5);
    expect(new RepairWorkshop('x', -1).maxRetries).toBe(0);
    expect(REPAIR_STRATEGIES.length).toBe(5);
  });
});

describe('族0004 恢复环境重生', () => {
  it('半成品续作闭环', () => {
    const r = new RecoveryReborn();
    r.snapshot('升级前', 2, 128, true);
    r.snapshot('升级中', 3, 64, false);
    const p = r.resumePoint();
    expect(p?.offset).toBe(64);
    expect(r.resume()).toBe(true);
    expect(r.resumePoint()).toBeNull();
  });
  it('容量 4 环形覆盖', () => {
    const r = new RecoveryReborn();
    for (let i = 0; i < 6; i += 1) r.snapshot(`s${i}`, 1, i, true);
    expect(r.snapshots.length).toBe(4);
    expect(r.snapshots[0]!.label).toBe('s2');
  });
  it('降级档裁剪工具', () => {
    const r = new RecoveryReborn();
    expect(r.availableTools().length).toBe(RECOVERY_TOOLS.length);
    r.setDegrade(1);
    expect(r.availableTools().some((t) => t.id === 'wipe')).toBe(false);
    r.setDegrade(9);
    expect(r.availableTools().length).toBe(1);
  });
  it('净身与叙事联动', () => {
    const r = new RecoveryReborn();
    r.snapshot('x', 1, 1, false);
    r.wipe();
    expect(r.resumePoint()).toBeNull();
    expect(r.fail('BC-003')).toBe('内核摘要不符');
  });
  it('摘要行格式', () => {
    const r = new RecoveryReborn();
    const s = r.snapshot('首启', 0, 8, true);
    expect(snapshotCaption(s)).toContain('完整');
  });
});

describe('族0005 启动日志剧场化', () => {
  const raw = '0 firmware i 上电\n40 limine i 菜单\n90 limine w 慢盘\n120 kernel e 缺补丁\n130 init i 挂载\n';
  it('解析五段式', () => {
    const es = parseBootLog(raw);
    expect(es.length).toBe(5);
    expect(es[1]!.level).toBe('info');
    expect(es[2]!.level).toBe('warn');
  });
  it('畸形行降级', () => {
    const es = parseBootLog('垃圾行\n\n');
    expect(es.length).toBe(1);
    expect(es[0]!.level).toBe('unknown');
  });
  it('分幕与占比', () => {
    const acts = buildActs(parseBootLog(raw));
    expect(acts.length).toBe(4);
    expect(acts[1]!.durationMs).toBe(50);
    const total = acts.reduce((a, x) => a + x.share, 0);
    expect(total).toBeCloseTo(1, 9);
  });
  it('样式解析与聚光灯', () => {
    expect(resolveTheaterStyle('cinematic')).toBe('cinematic');
    expect(resolveTheaterStyle('zzz')).toBe('timeline');
    expect(spotlight(parseBootLog(raw)).length).toBe(2);
  });
});

describe('族0006 引导失败叙事', () => {
  it('映射表完整', () => {
    expect(FAIL_NARRATIVES.length).toBeGreaterThanOrEqual(5);
    for (const n of FAIL_NARRATIVES) {
      expect(n.nextSteps.length).toBeGreaterThan(0);
      expect(n.title).not.toBe('');
    }
  });
  it('未知码兜底不裸报错', () => {
    const n = findNarrative('BC-777');
    expect(n.nextSteps.length).toBeGreaterThan(0);
    expect(narrate('bc-001')).toContain('BC-001');
    expect(shouldEscalate('BC-004')).toBe(true);
    expect(shouldEscalate('BC-001')).toBe(false);
  });
});

describe('族0007 启动配速学', () => {
  it('五档矩阵', () => {
    expect(PACING_PROFILES.length).toBe(5);
    expect(findPacing('warp').id).toBe('balanced');
  });
  it('钳制边界', () => {
    expect(clampCountdown(31)).toBe(30);
    expect(clampCountdown(-1)).toBe(0);
    expect(clampCountdown(Number.NaN)).toBe(3);
    expect(clampMotion(5)).toBe(3);
    expect(clampMotion(3, true)).toBe(1);
  });
  it('切档重置 + 摘要', () => {
    const m = new PacingMixer('sprint');
    expect(m.countdownSec).toBe(0);
    m.switchTo('steady');
    expect(m.countdownSec).toBe(8);
    m.setCountdown(99);
    expect(m.caption()).toContain('30s');
  });
});

describe('族0009 多系统选择剧场', () => {
  it('登记与选择记忆', () => {
    const t = new MultiBootTheater();
    t.add('varix', 'Variable', 'variable');
    t.add('win', 'Windows', 'windows');
    t.setDefault('win');
    expect(t.select('varix', 10)?.id).toBe('varix');
    expect(t.remembered).toBe('varix');
    expect(t.renderList()[0]!.id).toBe('varix');
    expect(t.resolveTarget()).toBe('win');
  });
  it('不可用项与容量钳制', () => {
    const t = new MultiBootTheater();
    t.add('rec', '恢复', 'recovery', false);
    expect(t.select('rec', 1)).toBeNull();
    expect(t.add('rec', '重复', 'linux')).toBe(false);
    for (let i = 0; i < BOOT_ENTRY_MAX; i += 1) t.add(`e${i}`, `E${i}`, 'linux', true);
    expect(t.add('overflow', '溢出', 'linux')).toBe(false);
  });
  it('删除联动与净身', () => {
    const t = new MultiBootTheater();
    t.add('a', 'A', 'variable');
    t.add('b', 'B', 'variable');
    t.setDefault('a');
    t.select('a', 1);
    t.remove('a');
    expect(t.defaultId).toBeNull();
    expect(t.remembered).toBeNull();
    t.reset();
    expect(t.entries.length).toBe(0);
  });
});
