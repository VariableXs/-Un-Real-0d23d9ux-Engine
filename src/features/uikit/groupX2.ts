// UNREAL-X AI-02：族0011~0020「电源状态剧场」自检断言组（X00251~X00500 代表性断言），勿删。
// 每族 ≥5 条可运行断言：覆盖开关存在 / 功能逻辑 / 边界钳制三类口径。
// 纯逻辑断言，全部走 src/features/oobe/、src/features/settings/、src/lib/settings.ts 的真实实现。

import * as T from "../oobe/powerTheater";
import * as H from "../settings/hiberArchive";
import * as E from "../settings/powerEvents";
import * as G from "../settings/powerGauge";
import * as W from "../settings/warmupPlan";
import * as Q from "../settings/bootQuiet";
import * as K from "../settings/powerEggs";
import * as A from "../ambience/powerX2";
import { POWER_KEYS, POWER_VALUE_SETS } from "../../lib/settings";
import type { PowerKey } from "../../lib/settings";

export type { PowerKey };

/** 与 checks.ts 的 CheckEntry 同构（独立声明，避免循环依赖）。 */
export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/* -------- 族0011 关机重启仪式 2.0（档位 + 编排 + 钳制） -------- */
export function checkX0011(): CheckEntry[] {
  const r = new T.CeremonyRunner("restart", "cinematic");
  r.start();
  r.tick();
  const mid = r.stage;
  r.pause();
  const resumed = r.resume();
  while (r.tick() !== "done") { /* 推进到完成 */ }
  const bad = new T.CeremonyRunner("shutdown", "nope");
  return [
    { id: "X00251", name: "仪式档位 5 档", check: () => T.CEREMONY_PROFILES.length === 5 && T.DEFAULT_CEREMONY_ID === "balanced" },
    { id: "X00252", name: "非法档回默认", check: () => bad.profileId === "balanced" && bad.clamped === 1 },
    { id: "X00253", name: "阶段编排到 done", check: () => mid > 0 && resumed && r.phase === "done" && T.ceremonyPercent(r) === 100 },
    { id: "X00254", name: "快照导出导入往返", check: () => { const s = r.snapshot(); const q = new T.CeremonyRunner(); return q.restore(s) && q.stage === r.stage && q.profileId === "cinematic"; } },
    { id: "X00255", name: "坏快照钳制不崩溃", check: () => { const q = new T.CeremonyRunner(); return !q.restore("{bad") && q.clamped === 1; } },
    { id: "X00256", name: "叙事码有下一步", check: () => T.CEREMONY_NARRATIVES.length >= 5 && T.findNarrative("PW-402").next.length > 0 && T.findNarrative("PW-999").code === "PW-000" },
    { id: "X00259", name: "低电量降级 1 阶/tick", check: () => { const q = new T.CeremonyRunner("shutdown", "cinematic", true); q.start(); q.tick(); return q.stage === 1; } },
    { id: "X00260", name: "取消可续作且净身", check: () => { const q = new T.CeremonyRunner(); q.start(); q.tick(); q.cancel(); const ok = q.phase === "canceled" && q.resume(); q.reset(); return ok && q.stage === 0 && q.logs.length === 0 && q.phase === "idle"; } },
  ];
}

/* -------- 族0012 睡眠唤醒剧场 2.0（档位 + 面纱 + 钳制） -------- */
export function checkX0012(): CheckEntry[] {
  const w = new T.WakeTheater("sunrise");
  w.wake();
  w.tick();
  const mid = w.stage;
  w.pause();
  const resumed = w.resume();
  while (w.tick() !== "awake") { /* 推进到醒来 */ }
  return [
    { id: "X00276", name: "唤醒档位 5 档", check: () => T.WAKE_PROFILES.length === 5 && T.DEFAULT_WAKE_ID === "balancedWake" },
    { id: "X00277", name: "非法档回默认", check: () => new T.WakeTheater("nope").profileId === "balancedWake" },
    { id: "X00278", name: "阶段推进到 awake", check: () => mid > 0 && resumed && w.phase === "awake" && T.wakePercent(w) === 100 },
    { id: "X00279", name: "瞬醒单阶段", check: () => { const q = new T.WakeTheater("blink"); q.wake(); return q.tick() === "awake" && q.stage === 1; } },
    { id: "X00286", name: "reduce-motion 压面纱", check: () => w.veilAlpha(true) === 1 && w.veilAlpha(false) > 0.2 },
    { id: "X00281", name: "取消回睡眠不崩溃", check: () => { const q = new T.WakeTheater(); q.wake(); q.tick(); q.cancel(); return q.phase === "sleeping" && q.stage === 0; } },
    { id: "X00285", name: "净身复位", check: () => { w.reset(); return w.phase === "sleeping" && w.stage === 0 && w.clamped === 0; } },
  ];
}

/* -------- 族0013 休眠档案（快照/导出/恢复） -------- */
export function checkX0013(): CheckEntry[] {
  const h = new H.HiberArchive();
  h.snapshot("a", [{ id: "files", focus: 9 }], { preset: "flow", columns: 2 });
  h.snapshot("b", [{ id: "x", focus: 99 }], {}, false);
  const before = h.snapshots.length;
  const exported = h.export();
  const fresh = new H.HiberArchive();
  return [
    { id: "X00301", name: "档案开关存在", check: () => POWER_VALUE_SETS.hiber.join() === "off,on" },
    { id: "X00302", name: "环形容量 3", check: () => { h.snapshot("c", [], {}); return before === 2 && h.snapshots.length === H.HIBER_SNAPSHOTS && h.snapshots[0]!.label === "c"; } },
    { id: "X00303", name: "应用数钳制 0~8", check: () => { const q = new H.HiberArchive(); q.snapshot("x", Array.from({ length: 12 }, (_, i) => ({ id: `a${i}`, focus: 5 }))); return q.snapshots[0]!.apps.length === H.HIBER_APP_MAX && q.clamped === 1; } },
    { id: "X00306", name: "焦点越界钳回默认", check: () => h.snapshots.find((s) => s.label === "b")!.apps[0]!.focus === 9 },
    { id: "X00308", name: "半成品续作", check: () => h.resume() && h.snapshots.every((s) => s.complete) && h.restorePoint() !== null && new H.HiberArchive().resume() === false },
    { id: "X00304", name: "导出导入三通道", check: () => { const ok = fresh.import(exported); return ok && fresh.snapshots.length === before && fresh.snapshots.find((s) => s.label === "a")!.complete && !fresh.snapshots.find((s) => s.label === "b")!.complete; } },
    { id: "X00307", name: "坏载荷拒收不崩溃", check: () => { const q = new H.HiberArchive(); return !q.import("{bad") && !q.import(JSON.stringify({ fmt: "vx-hiber/9", snaps: [] })) && q.snapshots.length === 0; } },
  ];
}

/* -------- 族0016 电源事件（事件流 + 订阅） -------- */
export function checkX0016(): CheckEntry[] {
  const s = new E.PowerEventStream();
  let seen = 0;
  const unsub = s.subscribe(() => { seen += 1; });
  s.record("wake", 1, "ok");
  s.record("shutdown", 2);
  s.record("bogus-type", 3);
  const n = s.events.length;
  const truncated = (() => { const q = new E.PowerEventStream(); const e = q.record("sleep", 4, "x".repeat(200)); return e ? e.detail.length : 0; })();
  return [
    { id: "X00351", name: "事件开关存在", check: () => POWER_VALUE_SETS.events.join() === "off,on" },
    { id: "X00352", name: "事件类型白名单", check: () => E.POWER_EVENT_TYPES.length === 8 && E.isPowerEventType("lid") && !E.isPowerEventType("boom") },
    { id: "X00353", name: "订阅回调与退订", check: () => seen === 2 && (unsub(), s.record("wake", 9), seen === 2 && s.subscriberCount() === 0) },
    { id: "X00354", name: "非法类型记钳制", check: () => s.clamped === 1 && n === 2 },
    { id: "X00355", name: "详情截断 120", check: () => truncated === 120 },
    { id: "X00356", name: "过滤查询与 lastOf", check: () => s.query(["wake"]).every((e) => e.type === "wake") && s.query().length >= 2 && s.lastOf("shutdown") !== null && s.lastOf("plugged") === null },
    { id: "X00357", name: "事件行可读", check: () => E.PowerEventStream.line({ type: "wake", stamp: 12, detail: "d" }).includes("唤醒") },
  ];
}

/* -------- 族0017 性能仪表 2.0（采样 + 健康度） -------- */
export function checkX0017(): CheckEntry[] {
  const g = new G.PowerGauge();
  g.sample(1, { battery: 64, cpu: 42, thermal: 55, runtime: 60 });
  g.sample(2, { battery: 150, cpu: -3, thermal: 91, runtime: 8 });
  return [
    { id: "X00376", name: "仪表开关存在", check: () => POWER_VALUE_SETS.gauge.join() === "off,on" },
    { id: "X00377", name: "四通道预算表", check: () => G.GAUGE_CHANNELS.length === 4 && G.GAUGE_BUDGETS.cpu.bad === 90 },
    { id: "X00378", name: "越界钳制 0~100", check: () => g.samples[0]!.values.battery === 100 && g.samples[0]!.values.cpu === 0 },
    { id: "X00379", name: "均值计算", check: () => g.avg("battery") === 82 && g.avg("cpu") === 21 },
    { id: "X00380", name: "健康度三档", check: () => g.health("thermal") === "warn" && g.health("cpu") === "ok" && g.health("runtime") === "warn" && g.degraded() },
    { id: "X00381", name: "摘要行含标记", check: () => g.caption().includes("!") },
    { id: "X00382", name: "环形窗口 16", check: () => { for (let i = 0; i < 20; i += 1) g.sample(i, { battery: 50 }); return g.samples.length === G.GAUGE_CAPACITY && g.samples[0]!.stamp === 19; } },
  ];
}

/* -------- 族0018 预热编排（队列 + tick） -------- */
export function checkX0018(): CheckEntry[] {
  const p = new W.WarmupPlanner();
  p.enqueue({ id: "low", type: "index-scan", priority: 9, budgetMs: 100 });
  p.enqueue({ id: "high", type: "network", priority: 1, budgetMs: 99000 });
  p.begin();
  const first = p.tick();
  const order = p.tick();
  const held = new W.WarmupPlanner(true);
  held.enqueue({ id: "a", type: "notify-sync", priority: 5, budgetMs: 50 });
  held.begin();
  held.tick();
  return [
    { id: "X00401", name: "预热开关存在", check: () => POWER_VALUE_SETS.warmup.join() === "off,on" },
    { id: "X00402", name: "按优先级出队", check: () => first === "running" && order === "done" && p.doneIds[0] === "high" && p.doneIds.length === 2 },
    { id: "X00403", name: "预算钳制 0~5000", check: () => { const q = new W.WarmupPlanner(); q.enqueue({ id: "x", type: "network", priority: 1, budgetMs: 99000 }); return q.queue[0]!.budgetMs === W.WARMUP_BUDGET_MAX; } },
    { id: "X00404", name: "非法类型拒收", check: () => !p.enqueue({ id: "z", type: "boom" as never, priority: 1, budgetMs: 1 }) && p.clamped === 1 },
    { id: "X00405", name: "低电量挂起与恢复", check: () => held.phase === "held" && held.release() && (held.phase as W.WarmupPhase) === "running" },
    { id: "X00406", name: "取消与净身", check: () => held.cancel("a") && (held.phase as W.WarmupPhase) === "done" && (held.reset(), held.queue.length === 0 && (held.phase as W.WarmupPhase) === "idle") },
  ];
}

/* -------- 族0019 启动降噪（档位 + 窗口） -------- */
export function checkX0019(): CheckEntry[] {
  const q = new Q.BootQuiet("hushed", 60);
  const inWin = q.decision("notify");
  for (let i = 0; i < 61; i += 1) q.tick();
  const outWin = q.decision("notify");
  const bad = new Q.BootQuiet("boom", 9999);
  return [
    { id: "X00426", name: "降噪四档存在", check: () => Q.QUIET_TIERS.length === 4 && Q.DEFAULT_QUIET_ID === "off" && Q.QUIET_TIERS[0]!.level === 0 },
    { id: "X00427", name: "窗口钳制 0~300", check: () => Q.clampQuietWindow(9999) === 300 && Q.clampQuietWindow(-5) === 0 && Q.clampQuietWindow(NaN) === Q.QUIET_WINDOW_DEFAULT },
    { id: "X00428", name: "档位抑制决策", check: () => inWin.suppressed && inWin.reason.includes("静谧") && !q.decision("motion").suppressed },
    { id: "X00429", name: "窗口外放行", check: () => !outWin.suppressed && !q.inWindow() },
    { id: "X00430", name: "非法档回 off", check: () => bad.tierId === "off" && bad.windowSec === 300 && bad.clamped === 1 },
    { id: "X00431", name: "全静档三路抑制", check: () => { const m = new Q.BootQuiet("mute", 30); return ["notify", "motion", "sound"].every((k) => m.decision(k as "notify").suppressed); } },
  ];
}

/* -------- 族0020 彩蛋层（可关闭 + 触发） -------- */
export function checkX0020(): CheckEntry[] {
  const k = new K.EggKeeper();
  k.celebrate("shutdown");
  const off = k.lineFor("shutdown");
  const wasDisabled = !k.enabled;
  k.enabled = true;
  const on = k.lineFor("shutdown");
  k.celebrate("shutdown"); k.celebrate("shutdown"); k.celebrate("shutdown"); k.celebrate("shutdown");
  k.dismiss("night");
  const fifth = k.lineFor("shutdown");
  return [
    { id: "X00451", name: "彩蛋默认关", check: () => wasDisabled && off === "" && K.POWER_EGGS.length >= 5 },
    { id: "X00452", name: "开启后按阈值触发", check: () => on.includes("明天见") && fifth.includes("告别与重逢") },
    { id: "X00453", name: "场合过滤", check: () => k.lineFor("wake") === "" },
    { id: "X00454", name: "屏蔽与恢复", check: () => fifth.includes("告别与重逢") && (k.dismiss("count"), k.lineFor("shutdown") === "" && k.restore("night") && k.dismiss("nope") === false && k.clamped === 1) },
    { id: "X00455", name: "计数钳制 99", check: () => { for (let i = 0; i < 200; i += 1) k.celebrate("wake"); return k.wakes === K.EGG_TRIGGER_MAX; } },
    { id: "X00456", name: "净身回默认", check: () => { k.reset(); return !k.enabled && k.shutdowns === 0 && k.dismissed.size === 0; } },
    { id: "X00457", name: "设置键白名单闭合", check: () => POWER_KEYS.length === 8 && (Object.keys(POWER_VALUE_SETS) as PowerKey[]).every((key) => POWER_VALUE_SETS[key].length > 0) },
  ];
}

/* -------- 氛围侧：环境光档（族0011/0012 落点） -------- */
export function checkX0020b(): CheckEntry[] {
  return [
    { id: "X00261", name: "氛围五档存在", check: () => A.AMBIENT_LEVELS.join() === "0,1,2,3,4" },
    { id: "X00262", name: "档位映射 token", check: () => A.ambientToken(0) === "--aurora-power-veil-l0" && A.ambientToken(9) === "--aurora-power-veil-l4" && A.ambientToken(NaN) === "--aurora-power-veil-l0" },
    { id: "X00263", name: "reduce-motion 压平", check: () => A.ambientFlat(2, true) === 0 && A.ambientFlat(3, true) === 4 && A.ambientFlat(2, false) === 2 },
    { id: "X00264", name: "关机/唤醒序列长度", check: () => A.SHUTDOWN_AMBIENT.length === 8 && A.WAKE_AMBIENT.length === 5 && A.SHUTDOWN_AMBIENT[7] === 4 && A.WAKE_AMBIENT[4] === 0 },
    { id: "X00265", name: "氛围摘要可读", check: () => A.ambientCaption("wake").includes("唤醒") },
  ];
}
