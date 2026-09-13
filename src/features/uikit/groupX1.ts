// UNREAL-X AI-01：族0001~0010「启动可靠与恢复」自检断言组（X00001~X00250 代表性断言），勿删。
// 每族 ≥5 条可运行断言：覆盖开关存在 / 功能逻辑 / 边界钳制三类口径。
// 纯逻辑断言，全部走 src/features/oobe/ 与 src/lib/settings.ts 的真实实现。

import * as R from "../oobe/repairWorkshop";
import * as Rb from "../oobe/recoveryReborn";
import * as L from "../oobe/bootLogTheater";
import * as N from "../oobe/bootFailNarrative";
import * as P from "../oobe/bootPacing";
import * as M from "../oobe/multiBootTheater";
import { BOOTCHAIN_KEYS, BOOTCHAIN_VALUE_SETS } from "../../lib/settings";
import type { BootchainKey } from "../../lib/settings";

export type { BootchainKey };

/** 与 checks.ts 的 CheckEntry 同构（独立声明，避免循环依赖）。 */
export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/* -------- 族0001 冷启动链路体检（档位 + 钳制，内核对齐） -------- */
export function checkX0001(): CheckEntry[] {
  const levels = ["off", "standard", "deep", "forensic", "custom"];
  return [
    { id: "X00001", name: "体检档位白名单存在", check: () => BOOTCHAIN_VALUE_SETS.audit.join() === levels.join() },
    { id: "X00002", name: "配速档目录 5 档", check: () => P.PACING_PROFILES.length === 5 && P.DEFAULT_PACING_ID === "balanced" },
    { id: "X00003", name: "倒计时钳制 0~30", check: () => P.clampCountdown(31) === 30 && P.clampCountdown(-4) === 0 && P.clampCountdown(NaN) === 3 },
    { id: "X00004", name: "非法档位回默认", check: () => P.findPacing("nope").id === "balanced" && L.resolveTheaterStyle("nope") === "timeline" },
    { id: "X00005", name: "reduce-motion 压动效", check: () => P.clampMotion(3, true) === 1 && P.clampMotion(2, false) === 2 },
  ];
}

/* -------- 族0002 Bootchain 健康度（评分 + 档位，开关） -------- */
export function checkX0002(): CheckEntry[] {
  const sets = Object.values(BOOTCHAIN_VALUE_SETS);
  return [
    { id: "X00026", name: "健康度开关存在", check: () => BOOTCHAIN_VALUE_SETS.health.join() === "off,on" },
    { id: "X00027", name: "全部键有白名单", check: () => sets.every((s) => s.length > 0) && BOOTCHAIN_KEYS.length === 12 },
    { id: "X00028", name: "档位无重复", check: () => sets.every((s) => new Set(s).size === s.length) },
    { id: "X00029", name: "键名无重复", check: () => new Set(BOOTCHAIN_KEYS).size === BOOTCHAIN_KEYS.length },
    { id: "X00030", name: "剧场样式 5 档", check: () => L.THEATER_STYLES.length === 5 },
  ];
}

/* -------- 族0003 启动修复工坊（策略 + 进度 + 续作） -------- */
export function checkX0003(): CheckEntry[] {
  const w = new R.RepairWorkshop("repair-entry", 2);
  w.start();
  w.tick();
  const mid = w.step;
  w.pause();
  const resumed = w.resume();
  while (w.tick() !== "done") { /* 推进到完成 */ }
  const broken = new R.RepairWorkshop("verify-loader", 2);
  broken.start();
  broken.tick(2);
  return [
    { id: "X00051", name: "策略目录 5 档", check: () => R.REPAIR_STRATEGIES.length === 5 && R.DEFAULT_STRATEGY === "repair-entry" },
    { id: "X00052", name: "未知策略回默认", check: () => R.findStrategy("bogus").id === R.DEFAULT_STRATEGY },
    { id: "X00053", name: "进度单调到 100%", check: () => mid > 0 && w.phase === "done" && R.repairPercent(w) === 100 && w.progress() <= 1 },
    { id: "X00054", name: "断点续作保留进度", check: () => resumed && w.step >= mid },
    { id: "X00055", name: "失败带错误码非裸报错", check: () => broken.phase === "failed" && broken.logs.some((l) => l.code === "BC-501") },
    { id: "X00056", name: "重试钳制 0~5", check: () => new R.RepairWorkshop("repair-entry", 99).maxRetries === 5 && new R.RepairWorkshop("repair-entry", -3).maxRetries === 0 },
    { id: "X00057", name: "净身清空日志", check: () => { w.reset(); return w.step === 0 && w.logs.length === 0 && w.phase === "idle"; } },
  ];
}

/* -------- 族0004 恢复环境重生（快照/续作/降级） -------- */
export function checkX0004(): CheckEntry[] {
  const r = new Rb.RecoveryReborn();
  r.snapshot("升级前", 2, 128, true);
  r.snapshot("升级中", 3, 64, false);
  const rp = r.resumePoint();
  const wasHalf = rp !== null && !rp.complete;
  const resumed = r.resume();
  return [
    { id: "X00076", name: "恢复重生开关存在", check: () => BOOTCHAIN_VALUE_SETS.recovery.join() === "off,on" },
    { id: "X00077", name: "快照环形容量 4", check: () => Rb.SNAPSHOT_CAPACITY === 4 && (r.snapshot("x", 1, 1, true), r.snapshot("y", 1, 1, true), r.snapshot("z", 1, 1, true), r.snapshots.length === Rb.SNAPSHOT_CAPACITY) },
    { id: "X00078", name: "半成品可续作", check: () => wasHalf && resumed && r.phase === "working" && r.resumePoint() === null },
    { id: "X00079", name: "降级档钳制 0~2", check: () => r.setDegrade(9) === 2 && r.setDegrade(-1) === 0 && r.clamped >= 2 },
    { id: "X00080", name: "最小档工具裁剪", check: () => { r.setDegrade(2); return r.availableTools().length === 1 && r.availableTools()[0]!.id === "shell"; } },
    { id: "X00081", name: "无续作点记钳制", check: () => { const q = new Rb.RecoveryReborn(); return q.resume() === false && q.clamped === 1; } },
    { id: "X00082", name: "净身无残档", check: () => { r.wipe(); return r.snapshots.length === 0 && r.resumePoint() === null; } },
  ];
}

/* -------- 族0005 启动日志剧场化（解析 + 幕结构） -------- */
export function checkX0005(): CheckEntry[] {
  const entries = L.parseBootLog("0 firmware i 上电\n40 limine i 菜单\n90 limine w 慢速盘\n120 kernel e 补丁缺失\n130 init i 挂载\n");
  const acts = L.buildActs(entries);
  const bad = L.parseBootLog("这不是日志");
  return [
    { id: "X00101", name: "剧场开关存在", check: () => BOOTCHAIN_VALUE_SETS.logTheater.includes("off") && BOOTCHAIN_VALUE_SETS.logTheater.length === 6 },
    { id: "X00102", name: "行解析五段式", check: () => entries.length === 5 && entries[2]!.stage === "limine" && entries[2]!.level === "warn" },
    { id: "X00103", name: "畸形行降级不抛", check: () => bad.length === 1 && bad[0]!.level === "unknown" && L.parseBootLog("\n \n").length === 0 },
    { id: "X00104", name: "按阶段成幕", check: () => acts.length === 4 && acts[1]!.stage === "limine" && acts[1]!.durationMs === 50 },
    { id: "X00105", name: "占比归一", check: () => Math.abs(acts.reduce((a, x) => a + x.share, 0) - 1) < 1e-9 },
    { id: "X00106", name: "聚光灯抓 error/warn", check: () => L.spotlight(entries).length === 2 },
    { id: "X00107", name: "幕标题含百分比", check: () => L.actCaption(acts[0]!).includes("%") && L.actCaption(acts[0]!).startsWith("第firmware幕") },
  ];
}

/* -------- 族0006 引导失败叙事（错误码映射） -------- */
export function checkX0006(): CheckEntry[] {
  const codes = N.FAIL_NARRATIVES.map((n) => n.code);
  return [
    { id: "X00126", name: "叙事开关存在", check: () => BOOTCHAIN_VALUE_SETS.failNarrative.join() === "off,on" },
    { id: "X00127", name: "映射表 ≥5 条", check: () => N.FAIL_NARRATIVES.length >= 5 && codes.every((c) => /^BC-\d{3}$/.test(c)) },
    { id: "X00128", name: "每条都有下一步", check: () => N.FAIL_NARRATIVES.every((n) => n.nextSteps.length >= 1 && n.title.length > 0) },
    { id: "X00129", name: "未知码不裸报错", check: () => N.findNarrative("BC-999").nextSteps.length > 0 && N.narrate("").includes("下一步") },
    { id: "X00130", name: "升级码联动恢复", check: () => N.shouldEscalate("BC-004") && !N.shouldEscalate("BC-002") },
    { id: "X00131", name: "小写码归一", check: () => N.findNarrative("bc-001").code === "BC-001" },
    { id: "X00132", name: "批量叙事等长", check: () => N.narrateAll(["BC-001", "BC-501"]).length === 2 },
  ];
}

/* -------- 族0007 启动配速学（节奏档配置） -------- */
export function checkX0007(): CheckEntry[] {
  const m = new P.PacingMixer("swift");
  const before = m.countdownSec;
  m.switchTo("steady");
  const cap = m.caption();
  return [
    { id: "X00151", name: "配速开关存在", check: () => BOOTCHAIN_VALUE_SETS.pacing.length === 5 && BOOTCHAIN_VALUE_SETS.pacing.includes("balanced") },
    { id: "X00152", name: "切档重置参数", check: () => before === 1 && m.countdownSec === 8 && m.profileId === "steady" },
    { id: "X00153", name: "倒计时越界钳制", check: () => m.setCountdown(99) === 30 && m.setCountdown(-2) === 0 && m.clamped >= 2 },
    { id: "X00154", name: "静默档无演出", check: () => P.findPacing("mute").silent === true && P.findPacing("sprint").countdownSec === 0 },
    { id: "X00155", name: "摘要可读", check: () => cap.includes("稳妥") && cap.includes("倒计时") },
    { id: "X00156", name: "非法档回默认", check: () => { const q = new P.PacingMixer("warp"); return q.profileId === "balanced" && q.clamped === 1; } },
  ];
}

/* -------- 族0008 安全启动仪式（信任链，内核对齐） -------- */
export function checkX0008(): CheckEntry[] {
  const modes = BOOTCHAIN_VALUE_SETS.secureboot;
  return [
    { id: "X00176", name: "安全启动五档存在", check: () => modes.join() === "off,audit,relaxed,strict,locked" },
    { id: "X00177", name: "默认档 = off（现状）", check: () => modes[0] === "off" },
    { id: "X00178", name: "档位无重复", check: () => new Set(modes).size === modes.length },
    { id: "X00179", name: "锁定档语义最深", check: () => modes.indexOf("locked") === modes.length - 1 },
    { id: "X00180", name: "审计档宽于宽松档", check: () => modes.indexOf("audit") < modes.indexOf("relaxed") && modes.indexOf("relaxed") < modes.indexOf("strict") },
  ];
}

/* -------- 族0009 多系统选择剧场（启动项 + 选择记忆） -------- */
export function checkX0009(): CheckEntry[] {
  const t = new M.MultiBootTheater();
  t.add("varix", "Variable", "variable", true);
  t.add("win", "Windows 11", "windows", true);
  t.add("rescue", "恢复环境", "recovery", false);
  t.setDefault("varix");
  const picked = t.select("win", 1000);
  const list = t.renderList();
  const unavailablePick = t.select("rescue", 2000);
  const dup = t.add("win", "重复", "windows");
  return [
    { id: "X00201", name: "多系统开关存在", check: () => BOOTCHAIN_VALUE_SETS.multiBoot.join() === "off,on" },
    { id: "X00202", name: "登记三项", check: () => t.entries.length === 3 && t.defaultId === "varix" },
    { id: "X00203", name: "选择记忆置顶", check: () => picked?.id === "win" && t.remembered === "win" && list[0]!.id === "win" },
    { id: "X00204", name: "不可用项拒绝", check: () => unavailablePick === null && t.clamped >= 1 && t.remembered === "win" },
    { id: "X00205", name: "重名与越界钳制", check: () => dup === false && t.add("", "空", "linux") === false },
    { id: "X00206", name: "目标回落链", check: () => t.resolveTarget() === "varix" && (t.remove("varix"), t.resolveTarget() === "win") },
    { id: "X00207", name: "净身清记忆", check: () => { t.reset(); return t.entries.length === 0 && t.remembered === null && t.defaultId === null; } },
  ];
}

/* -------- 族0010 固件风格定制（固件皮，内核对齐） -------- */
export function checkX0010(): CheckEntry[] {
  const persona = BOOTCHAIN_VALUE_SETS.persona;
  const styles = BOOTCHAIN_VALUE_SETS.logTheater.slice(1);
  return [
    { id: "X00226", name: "固件皮开关存在", check: () => persona.join() === "off,on" },
    { id: "X00227", name: "默认关 = 现状", check: () => persona[0] === "off" },
    { id: "X00228", name: "样式档可枚举", check: () => styles.length === 5 && styles.includes("cinematic") },
    { id: "X00229", name: "取值白名单闭合", check: () => (Object.keys(BOOTCHAIN_VALUE_SETS) as BootchainKey[]).every((k) => BOOTCHAIN_VALUE_SETS[k].length > 0) },
    { id: "X00230", name: "键值一一对应", check: () => BOOTCHAIN_KEYS.every((k) => k in BOOTCHAIN_VALUE_SETS) },
  ];
}
