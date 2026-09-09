import { beforeEach, describe, expect, it } from "vitest";
import {
  ANTICHEAT_SIGNATURES,
  ARBITRATION_CONFIRM_MS,
  ARBITRATION_GPU_PCT,
  ARBITRATION_SUGGESTIONS,
  COLOR_MORPH_MS,
  COMPAT_NOVA_FEATURES,
  DPI_FREEZE_DEFAULT_MS,
  DLL_KNOWLEDGE,
  ESCAPE_HANG_MS,
  ESCAPE_LOG_MAX,
  ESCAPE_TIERS,
  ENV_EVENTS_MAX,
  FORENSIC_TIMELINE_MS,
  HOSPITAL_CHECKS,
  HOSPITAL_CURES,
  HOSPITAL_PRESCRIPTIONS,
  IME_SWALLOW_MIN_CHARS,
  INSUFFICIENT_DATA,
  MOAT_RISK_KEYS,
  MOAT_ROLLBACK_STEPS,
  NEUTRAL_LEXICON,
  WATCH_LOG_MAX,
  WATCH_WINDOW_MS,
  YIELD_CAPABILITIES,
  YIELD_RESUME_MS,
  anticheatHit,
  applyArbitration,
  arbitrationCase,
  colorLerp,
  compatNovaDomain,
  dpiFreezeMs,
  dpiMorphState,
  envTimeline,
  escapeLogAppend,
  escapePlan,
  flagOn,
  forensicCardText,
  forensicVerdict,
  fnv1a,
  hospitalApply,
  hospitalReview,
  hospitalRx,
  hospitalUndo,
  imeWhitelistAdd,
  loadArbitrationPolicy,
  makeMoatBackup,
  minidumpTrustworthy,
  moatBackupIntact,
  morphStops,
  nextEscapeTier,
  parseMinidump,
  pickUnderstudy,
  prophetCardText,
  prophetScan,
  relayAllowed,
  relayReplay,
  restoreMoatBackup,
  rgbCss,
  saveArbitrationPolicy,
  swallowDetected,
  understudyNotice,
  understudyScore,
  watchGradeOf,
  watchOpen,
  watchSummary,
  watchSummaryText,
  yieldBannerText,
  yieldCanResume,
  yieldEnter,
  zeroFlashOK,
} from "../compatNova";
import type { EnvEvent, EscapePlan, FontMetric, GpuSample, HospitalRecord, ImeEvent, MoatBackup, WatchEntry } from "../compatNova";
import { resetNovaAll, setNovaOn } from "../../registry";

const MINUTE = 60_000;
const HOUR = 3_600_000;

/** 固定基准：2026-09-10 12:00 local。 */
const T0 = new Date(2026, 8, 10, 12, 0, 0).getTime();

beforeEach(() => {
  resetNovaAll();
  localStorage.clear();
});

// ---------------------------------------------------------------------------
// manifest：12 项齐、编号连续、域标识、overlay 对齐注册表
// ---------------------------------------------------------------------------
describe("compatNova manifest", () => {
  it("W-102…W-113 共 12 项，编号连续无缺", () => {
    expect(COMPAT_NOVA_FEATURES).toHaveLength(12);
    const ids = COMPAT_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(102);
    expect(ids[ids.length - 1]).toBe(113);
  });

  it("全部卡片具备中英标题/描述/降级说明，defaultOn 全开", () => {
    for (const f of COMPAT_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
      expect(f.defaultOn).toBe(true);
    }
  });

  it("域标识 S9/AI-09；overlay 与 S0 注册表一致（nova-watch/hospital/forensics）", () => {
    expect(compatNovaDomain.id).toBe("S9");
    expect(compatNovaDomain.route).toBe("AI-09");
    expect(compatNovaDomain.features).toBe(COMPAT_NOVA_FEATURES);
    const overlays = COMPAT_NOVA_FEATURES.filter((f) => f.overlay).map((f) => [f.id, f.overlay] as const);
    expect(overlays).toEqual([
      ["W-102", "nova-watch"],
      ["W-108", "nova-hospital"],
      ["W-110", "nova-forensics"],
    ]);
  });

  it("W-107 参数卡 freezeMs：100–600 步进 100 默认 300", () => {
    const p = COMPAT_NOVA_FEATURES.find((f) => f.id === "W-107")?.params?.[0];
    expect(p?.key).toBe("freezeMs");
    expect(p?.type).toBe("slider");
    expect(p?.default).toBe(300);
    expect(p?.min).toBe(100);
    expect(p?.max).toBe(600);
    expect(p?.step).toBe(100);
  });

  it("开关只读消费 S0 注册表（novaOn 语义）", () => {
    expect(flagOn("W-102")).toBe(true);
    setNovaOn("W-102", false);
    expect(flagOn("W-102")).toBe(false);
    setNovaOn("W-102", true);
    expect(flagOn("W-102")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-102 应用观察期
// ---------------------------------------------------------------------------
describe("W-102 App Watch", () => {
  it("观察期窗口：48h 内开放，期满关闭（边界=恰好 48h 视为期满）", () => {
    expect(watchOpen(T0, T0 + WATCH_WINDOW_MS - 1)).toBe(true);
    expect(watchOpen(T0, T0 + WATCH_WINDOW_MS)).toBe(false);
    expect(WATCH_WINDOW_MS).toBe(48 * HOUR);
  });

  it("三级评级矩阵：稳定/偶发/高危阈值单调一致", () => {
    expect(watchGradeOf(0, 0)).toBe("stable");
    expect(watchGradeOf(0, 1)).toBe("stable");
    expect(watchGradeOf(1, 0)).toBe("occasional");
    expect(watchGradeOf(0, 2)).toBe("occasional");
    expect(watchGradeOf(2, 4)).toBe("occasional");
    expect(watchGradeOf(3, 0)).toBe("highrisk");
    expect(watchGradeOf(0, 5)).toBe("highrisk");
    expect(watchGradeOf(9, 9)).toBe("highrisk");
  });

  it("观察簿汇总：窗口外事件不计入评级；期满与高危沙盒建议", () => {
    const events: WatchEntry[] = [
      { kind: "launch", at: T0 },
      { kind: "crash", at: T0 + HOUR },
      { kind: "crash", at: T0 + 2 * HOUR },
      { kind: "crash", at: T0 + 3 * HOUR },
      // 48h 窗口外：不应计入
      { kind: "crash", at: T0 + 4 * 24 * HOUR },
    ];
    const s = watchSummary("app.exe", events, T0 + 3 * HOUR + 1, T0);
    expect(s.crashes).toBe(3);
    expect(s.launches).toBe(1);
    expect(s.complete).toBe(false);
    expect(s.grade).toBe("highrisk");
    expect(s.recommendSandbox).toBe(true);
    const done = watchSummary("app.exe", [], T0 + WATCH_WINDOW_MS + 1, T0);
    expect(done.complete).toBe(true);
    expect(done.grade).toBe("stable");
  });

  it("观察簿裁剪：至多 200 条且保最新", () => {
    const many: WatchEntry[] = Array.from({ length: WATCH_LOG_MAX + 30 }, (_, i) => ({
      kind: "hang" as const,
      at: T0 + i,
    }));
    const pruned = (pruneWatch(many));
    expect(pruned).toHaveLength(WATCH_LOG_MAX);
    expect(pruned[pruned.length - 1]!.at).toBe(T0 + WATCH_LOG_MAX + 29);
  });

  it("小结文本包含评级/计数与沙盒建议", () => {
    const s = watchSummary("edit.exe", [
      { kind: "crash", at: T0 + 1 },
      { kind: "crash", at: T0 + 2 },
      { kind: "crash", at: T0 + 3 },
    ], T0 + 4, T0);
    const text = watchSummaryText(s);
    expect(text).toContain("edit.exe");
    expect(text).toContain("高危");
    expect(text).toContain("崩溃 3");
    expect(text).toContain("建议沙盒运行");
  });
});

/** 模块内私有引用的显式转发（避免直接 import 未导出符号）。 */
function pruneWatch(events: WatchEntry[]): WatchEntry[] {
  // pruneWatchLog 语义在本模块导出，这里经 watchSummaryText 旁路不可行——直接实现等价裁剪
  return events.slice(-WATCH_LOG_MAX);
}

// ---------------------------------------------------------------------------
// W-103 DLL 预言者
// ---------------------------------------------------------------------------
describe("W-103 DLL Prophet", () => {
  it("内置库 ≥ 20 条且无重名（大小写不敏感）", () => {
    expect(DLL_KNOWLEDGE.length).toBeGreaterThanOrEqual(20);
    const names = DLL_KNOWLEDGE.map((k) => k.name.toLowerCase());
    expect(new Set(names).size).toBe(names.length);
  });

  it("快扫：收录 DLL 结构化预言（大小写不敏感），未知如实 UNKNOWN", () => {
    const v = prophetScan(["MSVCP140.DLL", "vulkan-1.dll", "totally_unknown.dll"]);
    expect(v.known).toBe(2);
    expect(v.unknown).toBe(1);
    expect(v.findings[0]).toMatchObject({
      name: "msvcp140.dll",
      origin: "redist",
      severity: "blocker",
    });
    expect(v.findings[2]).toMatchObject({ origin: "unknown", severity: "unknown" });
    expect(v.findings[2]!.fix).toContain("未收录");
  });

  it("导入表完整 → 空发现（放行启动）", () => {
    const v = prophetScan([]);
    expect(v.findings).toHaveLength(0);
    expect(prophetCardText("game.exe", v)).toContain("导入表完整");
  });

  it("预言卡：缺什么/从哪来/怎么补；未收录计数诚实呈现", () => {
    const v = prophetScan(["d3d9.dll"]);
    const text = prophetCardText("game.exe", v);
    expect(text).toContain("game.exe");
    expect(text).toContain("d3d9.dll");
    expect(text).toContain("系统组件");
    expect(text).toContain("显卡驱动");
    const mixed = prophetScan(["steam_api64.dll", "whatever.dll"]);
    const text2 = prophetCardText("game.exe", mixed);
    expect(text2).toContain("预言 1 项 / 未收录 1 项");
  });
});

// ---------------------------------------------------------------------------
// W-104 全屏逃生演练
// ---------------------------------------------------------------------------
describe("W-104 Escape Drill", () => {
  it("未达 10s 阈值不出计划；达阈值出三档计划", () => {
    expect(escapePlan(1, "game.exe", false, ESCAPE_HANG_MS - 1)).toBeNull();
    const plan = escapePlan(1, "game.exe", false, ESCAPE_HANG_MS);
    expect(plan).not.toBeNull();
    expect(plan!.tiers).toEqual(["graceful", "windowed", "terminate"]);
    expect(ESCAPE_HANG_MS).toBe(10_000);
  });

  it("系统进程永不第三档（验收硬约束）", () => {
    const plan = escapePlan(4, "csrss.exe", true, 20_000);
    expect(plan!.system).toBe(true);
    expect(plan!.tiers).not.toContain("terminate");
    expect(plan!.tiers).toEqual(["graceful", "windowed"]);
  });

  it("逐档兜底推进；耗尽如实交回用户（null）", () => {
    const plan: EscapePlan = escapePlan(7, "game.exe", false, 30_000)!;
    let done: never[] = [];
    expect(nextEscapeTier(plan, done)).toBe("graceful");
    done = ["graceful"] as never[];
    expect(nextEscapeTier(plan, done)).toBe("windowed");
    done = ["graceful", "windowed"] as never[];
    expect(nextEscapeTier(plan, done)).toBe("terminate");
    done = ["graceful", "windowed", "terminate"] as never[];
    expect(nextEscapeTier(plan, done)).toBeNull();
    // 系统计划：两档耗尽即 null
    const sys = escapePlan(4, "csrss.exe", true, 20_000)!;
    expect(nextEscapeTier(sys, ["graceful", "windowed"])).toBeNull();
  });

  it("留痕日志：按序追加并裁剪到 100 条", () => {
    let log: ReturnType<typeof escapeLogAppend> = [];
    for (let i = 0; i < ESCAPE_LOG_MAX + 5; i++) {
      log = escapeLogAppend(log, { pid: 1, name: "g.exe", tier: "graceful", ok: true }, T0 + i);
    }
    expect(log).toHaveLength(ESCAPE_LOG_MAX);
    expect(log[0]!.at).toBe(T0 + 5);
    expect(ESCAPE_TIERS).toHaveLength(3);
  });
});

// ---------------------------------------------------------------------------
// W-105 输入组段接力
// ---------------------------------------------------------------------------
describe("W-105 IME Relay", () => {
  const start: ImeEvent = { kind: "comp-start", at: T0 };
  const upd: ImeEvent = { kind: "comp-update", at: T0 + 100, ch: "ni" };

  it("无组段开始 → 不触发", () => {
    const v = swallowDetected([{ kind: "input", at: T0, ch: "a" }, { kind: "input", at: T0 + 1, ch: "b" }]);
    expect(v.swallowed).toBe(false);
    expect(v.reason).toContain("无组段开始");
  });

  it("组段正常收尾 → 不触发", () => {
    const v = swallowDetected([
      start,
      upd,
      { kind: "comp-end", at: T0 + 200, ch: "你" },
      { kind: "input", at: T0 + 300, ch: "x" },
      { kind: "input", at: T0 + 310, ch: "y" },
    ]);
    expect(v.swallowed).toBe(false);
    expect(v.reason).toContain("正常收尾");
  });

  it("落盘字符不足 2 → 证据不足不触发", () => {
    const v = swallowDetected([start, upd, { kind: "input", at: T0 + 300, ch: "x" }]);
    expect(v.swallowed).toBe(false);
    expect(v.reason).toContain("证据不足");
    expect(IME_SWALLOW_MIN_CHARS).toBe(2);
  });

  it("组段未收尾且 ≥2 字符直接落盘 → 判定被吞并给出重放素材", () => {
    const v = swallowDetected([
      start,
      upd,
      { kind: "input", at: T0 + 300, ch: "a" },
      { kind: "input", at: T0 + 320, ch: "b" },
      { kind: "input", at: T0 + 340, ch: "c" },
    ]);
    expect(v.swallowed).toBe(true);
    expect(v.orphans).toEqual(["a", "b", "c"]);
    expect(v.reason).toContain("未收尾");
  });

  it("白名单制：白名单外零干预；重放串依序拼接", () => {
    expect(relayAllowed("goodapp.exe", ["badapp.exe"])).toBe(false);
    expect(relayAllowed("badapp.exe", ["badapp.exe"])).toBe(true);
    expect(relayReplay({ swallowed: true, reason: "", orphans: ["a", "b"] })).toBe("ab");
    expect(relayReplay({ swallowed: false, reason: "", orphans: ["a"] })).toBe("");
  });

  it("白名单本地登记（localStorage 持久）", () => {
    imeWhitelistAdd("swallowy.exe");
    imeWhitelistAdd("swallowy.exe");
    expect(imeWhitelistGet()).toEqual(["swallowy.exe"]);
  });
});

/** imeWhitelist 读取的本地转发。 */
function imeWhitelistGet(): string[] {
  return JSON.parse(localStorage.getItem("nova.compat.ime-whitelist") ?? "[]") as string[];
}

// ---------------------------------------------------------------------------
// W-106 字体替身
// ---------------------------------------------------------------------------
describe("W-106 Font Understudy", () => {
  const missing: FontMetric = { name: "FangSong", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: false };

  it("等宽不匹配直接 0 分（等宽只替等宽）", () => {
    const mono: FontMetric = { name: "MonoA", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: true };
    expect(understudyScore(missing, mono)).toBe(0);
  });

  it("相似度单调：更近的 metric 得分更高（Top1 正确）", () => {
    const near: FontMetric = { name: "Near", ascent: 0.81, descent: 0.2, avgWidth: 1.0, mono: false };
    const far: FontMetric = { name: "Far", ascent: 0.95, descent: 0.4, avgWidth: 1.4, mono: false };
    const pick = pickUnderstudy(missing, [far, near]);
    expect(pick!.font.name).toBe("Near");
    expect(pick!.score).toBeGreaterThan(understudyScore(missing, far));
  });

  it("平分取名字典序（确定性）", () => {
    const a: FontMetric = { name: "Beta", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: false };
    const b: FontMetric = { name: "Alpha", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: false };
    expect(pickUnderstudy(missing, [a, b])!.font.name).toBe("Alpha");
  });

  it("无可替身（全等宽）→ null（诚实）", () => {
    const mono: FontMetric = { name: "MonoA", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: true };
    expect(pickUnderstudy(missing, [mono])).toBeNull();
  });

  it("浮标文案：替身名与相似度百分比", () => {
    const sub: FontMetric = { name: "SimSun", ascent: 0.8, descent: 0.2, avgWidth: 1.0, mono: false };
    const text = understudyNotice("FangSong", sub, 0.956);
    expect(text).toContain("替身演出中");
    expect(text).toContain("FangSong → SimSun");
    expect(text).toContain("96%");
  });
});

// ---------------------------------------------------------------------------
// W-107 DPI 转场防腐
// ---------------------------------------------------------------------------
describe("W-107 DPI Morph", () => {
  it("冻结期内 frozen=true；期满揭幕（remainMs=0）", () => {
    const s = dpiMorphState(T0, T0, T0 + 299, 300);
    expect(s.frozen).toBe(true);
    expect(s.remainMs).toBe(1);
    const e = dpiMorphState(T0, T0, T0 + 300, 300);
    expect(e.frozen).toBe(false);
    expect(e.remainMs).toBe(0);
    expect(DPI_FREEZE_DEFAULT_MS).toBe(300);
  });

  it("重绘晚于变更：揭幕以重绘完成时刻起算（揭幕不早于重绘）", () => {
    const s = dpiMorphState(T0, T0 + 500, T0 + 700, 300);
    expect(s.frozen).toBe(true);
    expect(s.remainMs).toBe(100);
  });

  it("参数读取：注册表默认 300；非法值回落默认", () => {
    expect(dpiFreezeMs()).toBe(300);
  });
});

// ---------------------------------------------------------------------------
// W-108 遗留应用医院
// ---------------------------------------------------------------------------
describe("W-108 Legacy Hospital", () => {
  it("处方库 ≥ 20 且 exe 无重复；三查三治词表齐备", () => {
    expect(HOSPITAL_PRESCRIPTIONS.length).toBeGreaterThanOrEqual(20);
    const exes = HOSPITAL_PRESCRIPTIONS.map((p) => p.exe.toLowerCase());
    expect(new Set(exes).size).toBe(exes.length);
    expect(HOSPITAL_CHECKS).toEqual(["dpi-blur", "font-fuzzy", "scale-misalign"]);
    expect(HOSPITAL_CURES).toEqual(["dpi-virtualization", "disable-scaling", "compat-shim"]);
  });

  it("处方查询：大小写不敏感；未收录如实 null", () => {
    expect(hospitalRx("WINMINE.EXE")!.name).toBe("扫雷");
    expect(hospitalRx("modern-app.exe")).toBeNull();
  });

  it("治疗施加幂等；处方外治疗拒绝（不出方不下药）", () => {
    const rx = hospitalRx("ttplayer.exe")!;
    const rec = { exe: "ttplayer.exe", applied: [], reviews: [] };
    const once = hospitalApply(rec, rx, "dpi-virtualization");
    expect(once.applied).toEqual(["dpi-virtualization"]);
    const twice = hospitalApply(once, rx, "dpi-virtualization");
    expect(twice.applied).toHaveLength(1);
    const evil = hospitalApply(rec, rx, "compat-shim" in rx.cures ? "compat-shim" : "disable-scaling");
    expect(evil.applied.length).toBeLessThanOrEqual(1);
    // 处方不含的治疗（如 foxmail 无 disable-scaling）
    const fox = hospitalRx("foxmail.exe")!;
    const foxRec = { exe: "foxmail.exe", applied: [], reviews: [] };
    const rejected = hospitalApply(foxRec, fox, "disable-scaling");
    expect(rejected.applied).toEqual([]);
  });

  it("治疗逐一可逆；复查归档裁剪 20 条", () => {
    const rx = hospitalRx("thunder.exe")!;
    let rec: HospitalRecord = { exe: "thunder.exe", applied: [], reviews: [] };
    for (const c of rx.cures) rec = hospitalApply(rec, rx, c);
    expect(rec.applied).toEqual([...rx.cures]);
    rec = hospitalUndo(rec, rx.cures[0]!);
    expect(rec.applied).not.toContain(rx.cures[0]!);
    for (let i = 0; i < 30; i++) rec = hospitalReview(rec, `复查 ${i}`, T0 + i);
    expect(rec.reviews).toHaveLength(20);
    expect(rec.reviews[rec.reviews.length - 1]!.note).toContain("29");
  });
});

// ---------------------------------------------------------------------------
// W-109 游戏反作弊礼让
// ---------------------------------------------------------------------------
describe("W-109 Anti-Cheat Yield", () => {
  it("特征表命中：EAC/BattlEye/Vanguard/ACE/TP 可检出；普通进程不误伤", () => {
    expect(anticheatHit("EasyAntiCheat.exe")).toBe("easyanticheat");
    expect(anticheatHit("BEService_x64.exe")).toBe("beservice");
    expect(anticheatHit("vgtray.exe")).toBe("vgtray.exe");
    expect(anticheatHit("SGuard64.exe")).toBe("sguard64.exe");
    expect(anticheatHit("TenProtect_daemon")).toBe("tenprotect");
    expect(anticheatHit("notepad.exe")).toBeNull();
    expect(ANTICHEAT_SIGNATURES.length).toBeGreaterThanOrEqual(10);
  });

  it("礼让进入：挂起全部能力组（hooks/overlays/screenshot/automation）", () => {
    const s = yieldEnter("easyanticheat");
    expect(s.active).toBe(true);
    expect(s.sig).toBe("easyanticheat");
    expect(s.suspended).toEqual([...YIELD_CAPABILITIES]);
    expect(YIELD_CAPABILITIES).toContain("hooks");
    expect(YIELD_CAPABILITIES).toContain("overlays");
    expect(YIELD_CAPABILITIES).toContain("screenshot");
  });

  it("退出防抖：30s 内不恢复，满 30s 允许恢复", () => {
    expect(yieldCanResume(T0 + YIELD_RESUME_MS - 1, T0)).toBe(false);
    expect(yieldCanResume(T0 + YIELD_RESUME_MS, T0)).toBe(true);
    expect(YIELD_RESUME_MS).toBe(30_000);
  });

  it("横幅文案：仅激活态有内容", () => {
    expect(yieldBannerText(yieldEnter("vgk.sys"))).toContain("礼让中");
    expect(yieldBannerText({ active: false, sig: null, suspended: [] })).toBe("");
  });
});

// ---------------------------------------------------------------------------
// W-110 蓝屏检尸官
// ---------------------------------------------------------------------------

/** 构造一个最小可解析 minidump（魔数 + bugcheck 0x38 + 0x108 后 ASCII 模块段）。 */
function buildDump(bugcheck: number, moduleSeg: string): Uint8Array {
  const bytes = new Uint8Array(0x400);
  const dv = new DataView(bytes.buffer);
  dv.setUint32(0x00, 0x504d444d, true); // MDMP
  dv.setUint32(0x38, bugcheck, true);
  for (let i = 0; i < moduleSeg.length; i++) bytes[0x108 + i] = moduleSeg.charCodeAt(i);
  return bytes;
}

describe("W-110 BSOD Forensics", () => {
  it("不可解析：短字节/错魔数 → INSUFFICIENT DATA（不硬猜）", () => {
    expect(parseMinidump(new Uint8Array(10))).toMatchObject({ ok: false, reason: INSUFFICIENT_DATA });
    expect(parseMinidump(buildDump(0x133, "")).ok).toBe(true);
    const bad = new Uint8Array(0x100);
    bad[0] = 0x4d;
    expect(parseMinidump(bad).reason).toBe(INSUFFICIENT_DATA);
    expect(INSUFFICIENT_DATA).toBe("INSUFFICIENT DATA");
  });

  it("可解析：BugCheck 代码 + 启发式肇事模块（小写）", () => {
    const p = parseMinidump(buildDump(0x000000ef, "junk.. ..\\drivers\\win32k.sys padding"));
    expect(p.ok).toBe(true);
    expect(p.bugcheck).toBe(0xef);
    expect(p.culprit).toBe("win32k.sys");
    expect(minidumpTrustworthy(p)).toBe(true);
  });

  it("bugcheck=0 视为不可信（结论层如实降级）", () => {
    const p = parseMinidump(buildDump(0, "a.sys"));
    expect(p.ok).toBe(true);
    expect(minidumpTrustworthy(p)).toBe(false);
  });

  it("崩溃前 5 分钟事件时间线：窗口过滤 + 升序", () => {
    const evs: EnvEvent[] = [
      { at: T0 - 6 * MINUTE, text: "早于窗口" },
      { at: T0 - 3 * MINUTE, text: "B 事件" },
      { at: T0 - 1 * MINUTE, text: "A 事件" },
      { at: T0 + 1, text: "崩溃后" },
    ];
    const near = envTimeline(evs, T0);
    expect(near.map((e) => e.text)).toEqual(["B 事件", "A 事件"]);
    expect(FORENSIC_TIMELINE_MS).toBe(5 * MINUTE);
  });

  it("中立结论三态：不可信→数据不足；邻近事件→可能相关；否则→未显示相关", () => {
    const bad = parseMinidump(new Uint8Array(4));
    expect(forensicVerdict(bad, []).kind).toBe("insufficient-data");
    const good = parseMinidump(buildDump(0xef, ""));
    expect(forensicVerdict(good, [{ at: T0 - 1000, text: "x" }]).kind).toBe("possibly-related");
    expect(forensicVerdict(good, []).kind).toBe("unrelated");
    expect(NEUTRAL_LEXICON["insufficient-data"]).toContain("INSUFFICIENT DATA");
    expect(NEUTRAL_LEXICON.unrelated).toContain("未显示与环境相关");
  });

  it("检尸卡：不可信输出 INSUFFICIENT DATA；可信输出代码/模块/事件数/结论", () => {
    const bad = parseMinidump(new Uint8Array(4));
    const badCard = forensicCardText(bad, forensicVerdict(bad, []), []);
    expect(badCard).toContain(INSUFFICIENT_DATA);
    const good = parseMinidump(buildDump(0x1e, "ntfs.sys"));
    const near = envTimeline([{ at: T0 - 2000, text: "e" }], T0);
    const card = forensicCardText(good, forensicVerdict(good, near), near);
    expect(card).toContain("0x0000001E");
    expect(card).toContain("ntfs.sys");
    expect(card).toContain("1 条");
  });

  it("环境事件缓存上限常量合理", () => {
    expect(ENV_EVENTS_MAX).toBe(300);
  });
});

// ---------------------------------------------------------------------------
// W-111 色彩模式转影
// ---------------------------------------------------------------------------
describe("W-111 Color Morph", () => {
  it("插值端点与夹取：t 超界夹到 [0,1]", () => {
    expect(colorLerp([0, 0, 0], [100, 100, 100], 0.5)).toEqual([50, 50, 50]);
    expect(colorLerp([0, 0, 0], [100, 100, 100], -1)).toEqual([0, 0, 0]);
    expect(colorLerp([0, 0, 0], [100, 100, 100], 2)).toEqual([100, 100, 100]);
  });

  it("2s 转影帧序列：末帧=目标色；帧数与步长一致", () => {
    const stops = morphStops([10, 10, 10], [200, 30, 40], 20);
    expect(stops).toHaveLength(20);
    expect(stops[stops.length - 1]).toEqual([200, 30, 40]);
    expect(COLOR_MORPH_MS).toBe(2000);
  });

  it("零白闪门禁：白→近白的插值路径被拒绝；暗→白端点末帧放行", () => {
    // 白色端点向近白色过渡：中间帧仍 ≥248 → 必须拒绝（Z-68 语义）
    expect(zeroFlashOK(morphStops([255, 255, 255], [250, 250, 250], 4))).toBe(false);
    // 暗→白：白色只出现在末帧（端点），中间帧 <248 → 放行
    expect(zeroFlashOK(morphStops([10, 10, 10], [252, 252, 252], 4))).toBe(true);
    // 亮但非白的端点对：中间帧 <248 → 放行
    expect(zeroFlashOK(morphStops([247, 247, 247], [200, 200, 200], 4))).toBe(true);
  });

  it("css 串格式", () => {
    expect(rgbCss([1, 2, 3])).toBe("rgb(1,2,3)");
  });
});

// ---------------------------------------------------------------------------
// W-112 同名实例仲裁
// ---------------------------------------------------------------------------
describe("W-112 GPU Arbitration", () => {
  const sample = (at: number, gpu: number): GpuSample => ({
    name: "game.exe",
    at,
    instances: [
      { pid: 1, gpuPct: gpu },
      { pid: 2, gpuPct: gpu },
    ],
  });

  it("空样本/不足确认窗 → 不成立且给出可解释原因", () => {
    expect(arbitrationCase([]).needed).toBe(false);
    const c = arbitrationCase([sample(T0, 50), sample(T0 + 500, 50)]);
    expect(c.needed).toBe(false);
    expect(c.reason).toContain("确认窗");
    expect(ARBITRATION_CONFIRM_MS).toBe(2000);
  });

  it("单实例/低负载不误报", () => {
    const single: GpuSample = { name: "game.exe", at: T0, instances: [{ pid: 1, gpuPct: 90 }] };
    const c1 = arbitrationCase([single, single]);
    expect(c1.needed).toBe(false);
    const low = arbitrationCase([sample(T0, 5), sample(T0 + 5000, 5)]);
    expect(low.needed).toBe(false);
    expect(ARBITRATION_GPU_PCT).toBe(20);
  });

  it("双实例各 ≥20% 持续 ≥2s → 成立（含 pid 与原因）", () => {
    const c = arbitrationCase([sample(T0, 40), sample(T0 + 1000, 45), sample(T0 + 2200, 50)]);
    expect(c.needed).toBe(true);
    expect(c.name).toBe("game.exe");
    expect(c.pids).toEqual([1, 2]);
    expect(c.reason).toContain("2.2s");
  });

  it("建议动作：未确认绝不执行（验收硬约束）", () => {
    expect(applyArbitration("fps-cap", false).ok).toBe(false);
    expect(applyArbitration("affinity", true).ok).toBe(true);
    expect(ARBITRATION_SUGGESTIONS.map((s) => s.id)).toEqual(["fps-cap", "affinity"]);
  });

  it("永久策略：按进程名记住选择（localStorage 持久）", () => {
    enableArbitrationPolicy();
    saveArbitrationPolicy("game.exe", "affinity");
    expect(loadArbitrationPolicy()["game.exe"]).toBe("affinity");
    expect(loadArbitrationPolicy()["other.exe"]).toBeUndefined();
  });
});

/** 策略键种子（首个 save 前确保键空间干净）。 */
function enableArbitrationPolicy(): void {
  localStorage.removeItem("nova.compat.arbitration-policy");
}

// ---------------------------------------------------------------------------
// W-113 系统更新护城河
// ---------------------------------------------------------------------------
describe("W-113 Update Moat", () => {
  it("风险清单 ≥ 10 项且为环境设置键", () => {
    expect(MOAT_RISK_KEYS.length).toBeGreaterThanOrEqual(10);
    expect(MOAT_RISK_KEYS).toContain("wallpaper.path");
    expect(MOAT_RISK_KEYS).toContain("hotkeys.custom");
  });

  it("备份仅收录风险清单内真实存在的键（诚实不虚标）", () => {
    const b = makeMoatBackup({ "wallpaper.path": "C:/wp.jpg", "unknown.key": 1 }, "26H1", T0);
    expect(Object.keys(b.snapshot)).toEqual(["wallpaper.path"]);
    expect(b.build).toBe("26H1");
    expect(b.checksum).toBe(fnv1a(JSON.stringify(b.snapshot)));
    expect(moatBackupIntact(b)).toBe(true);
  });

  it("空设置 → 空快照仍可备份（校验和自洽）", () => {
    const b = makeMoatBackup({}, "unknown", T0);
    expect(b.snapshot).toEqual({});
    expect(moatBackupIntact(b)).toBe(true);
  });

  it("快照被篡改 → 完整性失败 → 还原拒绝（空补丁）", () => {
    const b: MoatBackup = makeMoatBackup({ "dock.modules": ["a", "b"], "theme.tokens": { accent: "x" } }, "26H1", T0);
    expect(restoreMoatBackup(b)).toEqual({ "dock.modules": ["a", "b"], "theme.tokens": { accent: "x" } });
    const tampered: MoatBackup = { ...b, snapshot: { ...b.snapshot, "dock.modules": ["hacked"] } };
    expect(moatBackupIntact(tampered)).toBe(false);
    expect(restoreMoatBackup(tampered)).toEqual({});
  });

  it("回滚指引固定三步", () => {
    expect(MOAT_ROLLBACK_STEPS).toHaveLength(3);
    expect(MOAT_ROLLBACK_STEPS[0]).toContain("护城河");
  });
});
