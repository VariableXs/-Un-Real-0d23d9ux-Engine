/**
 * NOVA-200 · S11 生态基因路（AI-11）单测 —— ecoNova（W-127…W-138）。
 *
 * 覆盖：manifest 契约（12 项连续/双语/默认档与 S0 一致）+ 十二项纯逻辑 +
 * 行为层非 DOM 安全性。验收口径：
 * - W-127 基因关闭 → 明确拒绝（GENE_OFF），未知基因 UNKNOWN_GENE；
 * - W-129 起草离线、知情投递、拒绝不残留；
 * - W-130 依赖冲突三解法影响面；
 * - W-131 24h 滚动、四维评分、中位对比；
 * - W-132 ≤2MB/60s + FNV-1a 校验和 + 续传 missing；
 * - W-133 500ms 刷新、超载红灯；
 * - W-134 未知权限如实警示；
 * - W-135 逐 token diff 与勾选移植；
 * - W-136 双货架独立排序 + 体检分过滤；
 * - W-137 封顶录制、逐条回放、销毁；
 * - W-138 零统计简短版讣告。
 */

import { describe, expect, it } from "vitest";
import {
  CAR_WARM_PCT,
  DRAFT_MAX,
  ECO_NOVA_FEATURES,
  GENE_CATALOG,
  GENE_DEFAULT,
  GENE_OFF,
  GENE_UNKNOWN,
  HYBRID_NAME,
  LH_WINDOW_MS,
  ORGAN_ZH,
  PERM_LEXICON,
  POST_MAX,
  POST_TEXT_MAX,
  RELAY_BUDGET_MS,
  RELAY_CHUNK_BYTES,
  RELAY_MAX_BYTES,
  RADAR_AXES,
  RADAR_AXIS_DEF,
  SCRIPT_CAP,
  SHELF_QUALITY_GATE,
  TRAIN_REFRESH_MS,
  activateEcoNova,
  applySelected,
  carLoadPct,
  carStatus,
  clamp,
  curateShelf,
  decodePerm,
  decodeWall,
  deactivateEcoNova,
  draftPost,
  ecoNovaDomain,
  effectiveGene,
  evictLH,
  farewellFor,
  filterByScore,
  findConflicts,
  flagOn,
  fnv1a,
  geneVerdict,
  isEcoNovaActive,
  median,
  medianCompare,
  monthDelta,
  monthKeyOf,
  normalizeAxis,
  obituaryOf,
  organOf,
  packRelay,
  parseRange,
  postManifest,
  pushDraft,
  radarEmpty,
  radarRead,
  ratingText,
  reassemble,
  recordAction,
  relayPlan,
  replayAt,
  riskColor,
  sealDispatch,
  semverCmp,
  semverInRange,
  servedDays,
  scriptOf,
  scriptSummary,
  scoreLighthouse,
  setGene,
  threeRemedies,
  tokenDiff,
  trainReport,
} from "../ecoNova";
import type { PostDraft } from "../ecoNova";

const NOW = Date.UTC(2026, 8, 10, 12, 0, 0); // 2026-09-10 12:00 UTC
const DAY = 86_400_000;

// ---------------------------------------------------------------------------
// manifest：12 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------

describe("ecoNova manifest", () => {
  it("W-127…W-138 共 12 项，编号连续无缺", () => {
    expect(ECO_NOVA_FEATURES).toHaveLength(12);
    const ids = ECO_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(127);
    expect(ids[ids.length - 1]).toBe(138);
  });

  it("每项必含中英标题/描述，域标识 S11/AI-11", () => {
    for (const f of ECO_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(ecoNovaDomain.id).toBe("S11");
    expect(ecoNovaDomain.route).toBe("AI-11");
  });

  it("默认档与 S0 注册表口径一致（仅 W-132 opt-in 默认关）", () => {
    for (const f of ECO_NOVA_FEATURES) {
      expect(f.defaultOn).toBe(f.id === "W-132" ? false : true);
    }
  });

  it("overlay 与 S0 注册表一致（nova-genes / nova-radar / nova-lighthouse）", () => {
    const ov = Object.fromEntries(ECO_NOVA_FEATURES.map((f) => [f.id, f.overlay]));
    expect(ov["W-127"]).toBe("nova-genes");
    expect(ov["W-128"]).toBe("nova-radar");
    expect(ov["W-131"]).toBe("nova-lighthouse");
    expect(Object.values(ov).filter(Boolean)).toHaveLength(3);
  });

  it("flagOn 只读消费 S0 注册表（未知 id 如实 false）", () => {
    expect(flagOn("W-999")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-127 插件基因编辑
// ---------------------------------------------------------------------------

describe("W-127 插件基因编辑", () => {
  it("基因目录 6 项、键唯一、默认态克制（自启默认关）", () => {
    expect(GENE_CATALOG).toHaveLength(6);
    expect(new Set(GENE_CATALOG.map((g) => g.key)).size).toBe(6);
    expect(GENE_DEFAULT.autostart).toBe(false);
    expect(GENE_DEFAULT.net).toBe(true);
    expect(GENE_DEFAULT.fs).toBe(true);
    expect(GENE_DEFAULT.notify).toBe(true);
    expect(GENE_DEFAULT.hotkey).toBe(true);
    expect(GENE_DEFAULT.clipboard).toBe(true);
  });

  it("生效基因优先级：覆盖 > 声明 > 目录默认", () => {
    expect(effectiveGene(false, true, "net")).toBe(true);
    expect(effectiveGene(true, false, "net")).toBe(false);
    expect(effectiveGene(false, undefined, "net")).toBe(false);
    expect(effectiveGene(undefined, undefined, "autostart")).toBe(false);
    expect(effectiveGene(undefined, undefined, "net")).toBe(true);
  });

  it("已关基因调用得到明确拒绝（GENE_OFF，非静默失败）", () => {
    const v = geneVerdict("p1", "net", false, true);
    expect(v.ok).toBe(false);
    expect(v.code).toBe(GENE_OFF);
    expect(v.message).toContain("GENE_OFF");
    expect(v.message).toContain("p1");
  });

  it("开启基因裁决 OK；未知基因如实 UNKNOWN_GENE", () => {
    expect(geneVerdict("p1", "fs", undefined, true).code).toBe("OK");
    const u = geneVerdict("p1", "teleport", undefined, undefined);
    expect(u.ok).toBe(false);
    expect(u.code).toBe(GENE_UNKNOWN);
  });

  it("声明关闭即拒绝（无覆盖时声明生效）", () => {
    const v = geneVerdict("p2", "clipboard", undefined, false);
    expect(v.code).toBe(GENE_OFF);
  });

  it("setGene 纯函数：新增/更新且不可变；同值幂等返回原引用", () => {
    const base = { p1: { net: true } };
    const next = setGene(base, "p1", "net", false);
    expect(next.p1?.net).toBe(false);
    expect(base.p1?.net).toBe(true);
    const added = setGene(base, "p9", "fs", false);
    expect(added.p9?.fs).toBe(false);
    expect(setGene(base, "p1", "net", true)).toBe(base);
  });
});

// ---------------------------------------------------------------------------
// W-128 生态雷达图
// ---------------------------------------------------------------------------

describe("W-128 生态雷达图", () => {
  it("六轴定义与 cap 满分归一", () => {
    expect(RADAR_AXES).toHaveLength(6);
    expect(normalizeAxis("security", 100)).toBe(100);
    expect(normalizeAxis("plugins", 20)).toBe(50);
    expect(normalizeAxis("themes", 999)).toBe(100);
    expect(normalizeAxis("activity", -5)).toBe(0);
  });

  it("月键格式 YYYY-MM", () => {
    expect(monthKeyOf(Date.UTC(2026, 0, 3))).toBe("2026-01");
    expect(monthKeyOf(NOW)).toBe("2026-09");
  });

  it("radarRead 同月均值归一、缺轴如实 null、月偏移取上月", () => {
    const samples = [
      { axis: "plugins" as const, value: 10, ts: NOW - DAY },
      { axis: "plugins" as const, value: 30, ts: NOW - 2 * DAY },
      { axis: "security" as const, value: 100, ts: NOW - DAY },
      { axis: "plugins" as const, value: 40, ts: Date.UTC(2026, 7, 15) },
    ];
    const cur = radarRead(samples, NOW, 0);
    const plugins = cur.find((x) => x.axis === "plugins")!;
    expect(plugins.value).toBe(normalizeAxis("plugins", 20)); // (10+30)/2
    expect(cur.find((x) => x.axis === "security")!.value).toBe(100);
    expect(cur.find((x) => x.axis === "themes")!.value).toBeNull();
    const prev = radarRead(samples, NOW, -1);
    expect(prev.find((x) => x.axis === "plugins")!.value).toBe(normalizeAxis("plugins", 40));
  });

  it("monthDelta 差值与 null 跳过", () => {
    const cur = [
      { axis: "plugins" as const, value: 50 },
      { axis: "themes" as const, value: null },
    ];
    const prev = [
      { axis: "plugins" as const, value: 40 },
      { axis: "themes" as const, value: 10 },
    ];
    const d = monthDelta(cur, prev);
    expect(d.find((x) => x.axis === "plugins")!.delta).toBe(10);
    expect(d.find((x) => x.axis === "themes")!.delta).toBeNull();
  });

  it("无样本如实空态", () => {
    expect(radarEmpty([])).toBe(true);
    expect(radarEmpty([{ axis: "plugins", value: 1, ts: NOW }])).toBe(false);
  });

  it("RADAR_AXIS_DEF 六轴中文名齐备", () => {
    for (const a of RADAR_AXES) expect(RADAR_AXIS_DEF[a].zh.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// W-129 社区声邮
// ---------------------------------------------------------------------------

describe("W-129 社区声邮", () => {
  it("本地起草：裁剪、字数、语音来源、id 唯一", () => {
    const a = draftPost("text", "  你好生态  ", NOW)!;
    expect(a.text).toBe("你好生态");
    expect(a.chars).toBe(4);
    const b = draftPost("voice", "你好生态", NOW + 1)!;
    expect(b.kind).toBe("voice");
    expect(b.id).not.toBe(a.id);
  });

  it("空稿如实拒绝；超长封顶 POST_TEXT_MAX", () => {
    expect(draftPost("text", "   ", NOW)).toBeNull();
    const long = draftPost("text", "字".repeat(POST_TEXT_MAX + 500), NOW)!;
    expect(long.chars).toBe(POST_TEXT_MAX);
  });

  it("草稿封顶 DRAFT_MAX 滚动淘汰最旧", () => {
    let ds: PostDraft[] = [];
    for (let i = 0; i < DRAFT_MAX + 5; i++) {
      const d = draftPost("text", `t${i}`, NOW + i)!;
      ds = pushDraft(ds, d);
    }
    expect(ds).toHaveLength(DRAFT_MAX);
    expect(ds[0]!.text).toBe("t5");
    expect(ds[ds.length - 1]!.text).toBe(`t${DRAFT_MAX + 4}`);
  });

  it("投递前清单：完整内容（类型/字数/首行预览）知情", () => {
    const ds = [draftPost("text", "一句话反馈", NOW)!, draftPost("voice", "口".repeat(30), NOW + 1)!];
    const m = postManifest(ds);
    expect(m).toHaveLength(2);
    expect(m[0]).toContain("文字");
    expect(m[0]).toContain("5 字");
    expect(m[1]).toContain("语音");
    expect(m[1]).toContain("…");
  });

  it("知情投递：confirm=false 拒绝不残留草稿外副本；confirm=true 至多 POST_MAX 封", () => {
    const ds = Array.from({ length: POST_MAX + 3 }, (_, i) => draftPost("text", `t${i}`, NOW + i)!);
    const refuse = sealDispatch(ds, false);
    expect(refuse.posted).toHaveLength(0);
    expect(refuse.kept).toBe(ds);
    const ok = sealDispatch(ds, true);
    expect(ok.posted).toHaveLength(POST_MAX);
    expect(ok.kept).toHaveLength(3);
    expect(ok.posted[0]!.text).toBe("t0");
  });
});

// ---------------------------------------------------------------------------
// W-130 依赖树医生
// ---------------------------------------------------------------------------

describe("W-130 依赖树医生", () => {
  it("区间解析：*/精确/脱字/下界", () => {
    expect(parseRange("*")).toEqual({ min: [0, 0, 0], maxExcl: null });
    expect(parseRange("1.2.3")).toEqual({ min: [1, 2, 3], maxExcl: [1, 2, 4] });
    expect(parseRange("^1.2.3")).toEqual({ min: [1, 2, 3], maxExcl: [2, 0, 0] });
    expect(parseRange(">=1.0.0")).toEqual({ min: [1, 0, 0], maxExcl: null });
  });

  it("semverInRange 矩阵与排序", () => {
    expect(semverInRange("1.5.0", "^1.2.3")).toBe(true);
    expect(semverInRange("2.0.0", "^1.2.3")).toBe(false);
    expect(semverInRange("1.2.3", "1.2.3")).toBe(true);
    expect(semverInRange("1.2.4", "1.2.3")).toBe(false);
    expect(semverInRange("0.9.0", ">=1.0.0")).toBe(false);
    expect(semverCmp([1, 2, 3], [1, 2, 4])).toBeLessThan(0);
    expect(semverCmp([2, 0, 0], [1, 9, 9])).toBeGreaterThan(0);
  });

  it("A 要 v1 / B 要 v2 的冲突被检出（红结）", () => {
    const nodes = [
      { id: "A", version: "1.0.0", deps: { shared: "^1.0.0" } },
      { id: "B", version: "1.0.0", deps: { shared: "^2.0.0" } },
    ];
    const c = findConflicts(nodes);
    expect(c).toHaveLength(1);
    expect(c[0]!.dep).toBe("shared");
    expect(c[0]!.demandors.map((d) => d.id).sort()).toEqual(["A", "B"]);
  });

  it("区间可交叠或单声明方 → 无冲突（入口隐藏）", () => {
    expect(
      findConflicts([
        { id: "A", version: "1.0.0", deps: { shared: "^1.0.0" } },
        { id: "B", version: "1.0.0", deps: { shared: ">=1.2.0" } },
      ]),
    ).toHaveLength(0);
    expect(findConflicts([{ id: "A", version: "1.0.0", deps: { x: "*" } }])).toHaveLength(0);
    expect(findConflicts([])).toHaveLength(0);
  });

  it("三解法预估：升A/降B/双版本，影响面与成本", () => {
    const c = {
      dep: "shared",
      demandors: [
        { id: "A", range: "^1.0.0" },
        { id: "B", range: "^2.0.0" },
        { id: "C", range: ">=1.5.0" },
      ],
    };
    const remedies = threeRemedies(c);
    const up = remedies[0]!;
    const down = remedies[1]!;
    const dual = remedies[2]!;
    expect(up.kind).toBe("upgrade");
    expect(up.target).toBe("2.0.0");
    expect(up.affected).toContain("A"); // ^1 不容 2.0.0
    expect(up.affected).not.toContain("C"); // >=1.5 容 2.0.0
    expect(down.kind).toBe("downgrade");
    expect(down.target).toBe("1.0.0");
    expect(down.affected).toContain("B");
    expect(down.affected).not.toContain("A");
    expect(dual.kind).toBe("dual");
    expect(dual.affected).toEqual([]);
    expect(dual.cost).toBe(3);
    expect(dual.target).toContain("1.0.0 + 2.0.0");
  });
});

// ---------------------------------------------------------------------------
// W-131 本地灯塔
// ---------------------------------------------------------------------------

describe("W-131 本地灯塔", () => {
  const s = (pid: string, cpuPct: number, memMb: number, bootMs: number, eventsPerMin: number, ts = NOW): LHSampleLike => ({
    pluginId: pid,
    ts,
    cpuPct,
    memMb,
    bootMs,
    eventsPerMin,
  });

  it("24h 滚动窗口淘汰", () => {
    const arr = [s("a", 1, 1, 1, 1, NOW - LH_WINDOW_MS), s("a", 2, 2, 2, 2, NOW - LH_WINDOW_MS - 1), s("a", 3, 3, 3, 3, NOW)];
    const kept = evictLH(arr, NOW);
    expect(kept).toHaveLength(2);
    expect(kept.every((x) => NOW - x.ts <= LH_WINDOW_MS)).toBe(true);
  });

  it("四维评分 0–100 且成本越低分越高；overall 四维均值", () => {
    const lite = scoreLighthouse([s("lite", 2, 20, 100, 5)])!["lite"]!;
    const heavy = scoreLighthouse([s("heavy", 20, 200, 900, 40)])!["heavy"]!;
    expect(lite.cpu).toBeGreaterThan(heavy.cpu);
    expect(lite.mem).toBeGreaterThan(heavy.mem);
    expect(lite.boot).toBeGreaterThan(heavy.boot);
    expect(lite.events).toBeGreaterThan(heavy.events);
    expect(lite.overall).toBe(Math.round((lite.cpu + lite.mem + lite.boot + lite.events) / 4));
    for (const v of [lite.cpu, lite.mem, lite.boot, lite.events, heavy.cpu]) {
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(100);
    }
  });

  it("评分越界钳制（超重插件不为负）", () => {
    const worst = scoreLighthouse([s("x", 100, 1000, 5000, 100)])!["x"]!;
    expect(worst.cpu).toBe(0);
    expect(worst.mem).toBe(0);
    expect(worst.boot).toBe(0);
    expect(worst.events).toBe(0);
  });

  it("无样本如实 null（不编造跑分）", () => {
    expect(scoreLighthouse([])).toBeNull();
  });

  it("median 奇偶与空集", () => {
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 2, 3])).toBe(Math.round(2.5));
    expect(median([])).toBe(0);
  });

  it("中位对比条：delta = 我 − 生态中位（评分制）", () => {
    const scores = scoreLighthouse([s("a", 5, 40, 200, 10), s("b", 15, 120, 600, 30)])!;
    const cmpA = medianCompare(scores, "a");
    expect(cmpA).toHaveLength(4);
    const cpu = cmpA.find((c) => c.dim === "cpu")!;
    expect(cpu.pid).toBe("a");
    expect(cpu.mine).toBe(80); // 100 − 5×4
    expect(cpu.med).toBe(60); // median(80,40)
    expect(cpu.delta).toBe(20);
    const cmpB = medianCompare(scores, "b");
    expect(cmpB.find((c) => c.dim === "cpu")!.mine).toBe(40);
    expect(cmpB.find((c) => c.dim === "cpu")!.delta).toBe(-20);
  });
});

// ---------------------------------------------------------------------------
// W-132 二维码驿传
// ---------------------------------------------------------------------------

describe("W-132 二维码驿传", () => {
  it("FNV-1a 32bit：空向量与确定性", () => {
    expect(fnv1a(new Uint8Array(0))).toBe("811c9dc5");
    const a = fnv1a(new TextEncoder().encode("nova"));
    expect(a).toBe(fnv1a(new TextEncoder().encode("nova")));
    expect(a).toHaveLength(8);
    expect(a).not.toBe(fnv1a(new TextEncoder().encode("novb")));
  });

  it("分片打包：count/逐片体量/全包校验和", () => {
    const data = new TextEncoder().encode("x".repeat(RELAY_CHUNK_BYTES * 2 + 5));
    const p = packRelay(data);
    expect(p.count).toBe(3);
    expect(p.chunks).toHaveLength(3);
    expect(p.chunks[0]!.body).toHaveLength(RELAY_CHUNK_BYTES);
    expect(p.chunks[2]!.body).toHaveLength(5);
    expect(p.checksum).toBe(fnv1a(data));
  });

  it("发车预检：2MB 可行且 60s 预算内；超 2MB 如实拒绝", () => {
    const ok = relayPlan(RELAY_MAX_BYTES);
    expect(ok.ok).toBe(true);
    expect(ok.count).toBe(Math.ceil(RELAY_MAX_BYTES / RELAY_CHUNK_BYTES));
    expect(ok.perChunkMs).toBeGreaterThanOrEqual(1);
    expect(ok.perChunkMs * ok.count).toBeLessThanOrEqual(RELAY_BUDGET_MS);
    const over = relayPlan(RELAY_MAX_BYTES + 1);
    expect(over.ok).toBe(false);
    expect(over.reason).toContain("OVER BUDGET");
  });

  it("重组闭环：分片齐 → 校验和通过 → 原文还原", () => {
    const data = new TextEncoder().encode("驿传数据 relay-payload");
    const p = packRelay(data);
    const r = reassemble(p, p.chunks);
    expect(r.ok).toBe(true);
    expect(r.missing).toEqual([]);
    expect(new TextDecoder().decode(r.data!)).toBe("驿传数据 relay-payload");
  });

  it("缺片如实列出 missing（续传依据）；校验和不符 → 拒收", () => {
    const data = new TextEncoder().encode("y".repeat(RELAY_CHUNK_BYTES * 3));
    const p = packRelay(data);
    const miss = reassemble(p, [p.chunks[0]!, p.chunks[2]!]);
    expect(miss.ok).toBe(false);
    expect(miss.missing).toEqual([1]);
    const bad: typeof p.chunks = p.chunks.map((c, i) => (i === 1 ? { ...c, body: new TextEncoder().encode("z".repeat(RELAY_CHUNK_BYTES)) } : c));
    const r = reassemble(p, bad);
    expect(r.ok).toBe(false);
    expect(r.checksumOk).toBe(false);
    expect(r.data).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-133 限流列车
// ---------------------------------------------------------------------------

describe("W-133 限流列车", () => {
  it("车厢状态：空载/正常/黄灯 ≥85%/超载红灯", () => {
    expect(CAR_WARM_PCT).toBe(85);
    expect(TRAIN_REFRESH_MS).toBe(500);
    expect(carStatus({ id: "a", used: 0, quota: 100 })).toBe("idle");
    expect(carStatus({ id: "a", used: 10, quota: 0 })).toBe("idle");
    expect(carStatus({ id: "a", used: 50, quota: 100 })).toBe("ok");
    expect(carStatus({ id: "a", used: 85, quota: 100 })).toBe("warm");
    expect(carStatus({ id: "a", used: 101, quota: 100 })).toBe("overload");
    expect(carLoadPct({ id: "a", used: 30, quota: 100 })).toBe(30);
    expect(carLoadPct({ id: "a", used: 1, quota: 0 })).toBe(0);
  });

  it("时刻表：载客率降序 + 红灯/黄灯计数", () => {
    const rep = trainReport([
      { id: "low", used: 10, quota: 100 },
      { id: "hot", used: 120, quota: 100 },
      { id: "warm", used: 90, quota: 100 },
      { id: "zero", used: 0, quota: 100 },
    ]);
    expect(rep.cars.map((c) => c.id)).toEqual(["hot", "warm", "low", "zero"]);
    expect(rep.overloaded).toBe(1);
    expect(rep.warm).toBe(1);
    expect(rep.cars.find((c) => c.id === "hot")!.status).toBe("overload");
  });
});

// ---------------------------------------------------------------------------
// W-134 权限语言墙
// ---------------------------------------------------------------------------

describe("W-134 权限语言墙", () => {
  it("已知权限逐条白话 + 风险分级", () => {
    const d = decodePerm("fs.delete");
    expect(d.plainZh).toContain("删除");
    expect(d.risk).toBe("high");
    expect(d.warn).toBe(false);
    expect(decodePerm("notify").risk).toBe("low");
    expect(decodePerm("clipboard.read").risk).toBe("high");
    expect(decodePerm("net.send").risk).toBe("mid");
  });

  it("通配权限剥壳（fs.read~/* → 按声明范围）", () => {
    const d = decodePerm("fs.read~/*");
    expect(d.risk).toBe("mid");
    expect(d.warn).toBe(false);
    expect(d.plainZh).toContain("范围");
  });

  it("未知权限如实显示原文 + 警示（NOT IN LEXICON）", () => {
    const d = decodePerm("teleport.now");
    expect(d.warn).toBe(true);
    expect(d.risk).toBe("unknown");
    expect(d.plainZh).toContain("NOT IN LEXICON");
    expect(d.plainZh).toContain("teleport.now");
  });

  it("翻译库覆盖生态规范全集动词；解码墙与色标", () => {
    expect(Object.keys(PERM_LEXICON).length).toBeGreaterThanOrEqual(15);
    const wall = decodeWall(["fs.read~/*", "shell.exec", "unknown.verb"]);
    expect(wall).toHaveLength(3);
    expect(riskColor("low")).toContain("risk-low");
    expect(riskColor("unknown")).toContain("risk-unknown");
    expect(decodeWall(["fs.read", "shell.exec"]).filter((x) => x.risk === "high")).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// W-135 主题换器官预览
// ---------------------------------------------------------------------------

describe("W-135 主题换器官预览", () => {
  it("token 键 → 器官归类", () => {
    expect(organOf("--accent-color")).toBe("color");
    expect(organOf("--radius-lg")).toBe("radius");
    expect(organOf("--font-weight-body")).toBe("weight");
    expect(organOf("--grid-gap")).toBe("spacing");
    expect(organOf("--font-ui")).toBe("font");
    expect(organOf("--misc")).toBe("other");
    expect(ORGAN_ZH.color).toBe("颜色");
  });

  it("逐 token diff：只列变更、键名字典序、增删如实", () => {
    const diff = tokenDiff({ a: "1", b: "2", c: "3" }, { a: "1", b: "9", d: "4" });
    expect(diff.map((d) => d.key)).toEqual(["b", "c", "d"]);
    expect(diff.find((d) => d.key === "c")!.from).toBe("3");
    expect(diff.find((d) => d.key === "c")!.to).toBe("∅");
    expect(diff.find((d) => d.key === "d")!.from).toBe("∅");
  });

  it("勾选移植：只换所选、未选保持本色、非变更键不掺入", () => {
    const base = { color: "red", radius: "8px", weight: "400" };
    const next = { color: "blue", radius: "12px", weight: "600" };
    const merged = applySelected(base, next, ["color"]);
    expect(merged).toEqual({ color: "blue", radius: "8px", weight: "400" });
    expect(applySelected(base, next, ["color", "weight"]).weight).toBe("600");
    expect(applySelected(base, next, [])).toEqual(base);
  });

  it("混成主题命名固定", () => {
    expect(HYBRID_NAME).toBe("我的混成主题");
  });
});

// ---------------------------------------------------------------------------
// W-136 市场策展货架
// ---------------------------------------------------------------------------

describe("W-136 市场策展货架", () => {
  const items = [
    { id: "o1", name: "官方甲", official: true, votes: 5, quality: 90 },
    { id: "o2", name: "官方乙", official: true, votes: 99, quality: 70 }, // 未过门槛
    { id: "c1", name: "社区甲", official: false, votes: 40, quality: 60 },
    { id: "c2", name: "社区乙", official: false, votes: 80, quality: 50 },
  ];

  it("双货架独立排序：官方按门槛分、社区按投票", () => {
    expect(SHELF_QUALITY_GATE).toBe(80);
    const { official, community } = curateShelf(items);
    expect(official.map((i) => i.id)).toEqual(["o1"]);
    expect(community.map((i) => i.id)).toEqual(["c2", "c1"]);
  });

  it("体检分过滤真实联动：无跑分如实滤除", () => {
    const scored = [
      { id: "c1", name: "甲", official: false, votes: 1, quality: 50, score: 88 },
      { id: "c2", name: "乙", official: false, votes: 2, quality: 50, score: 40 },
      { id: "c3", name: "丙", official: false, votes: 3, quality: 50, score: null },
    ];
    expect(filterByScore(scored, 0)).toHaveLength(3);
    const f = filterByScore(scored, 80);
    expect(f.map((i) => i.id)).toEqual(["c1"]);
  });
});

// ---------------------------------------------------------------------------
// W-137 行为剧作 replay
// ---------------------------------------------------------------------------

describe("W-137 行为剧作 replay", () => {
  it("录制封顶 SCRIPT_CAP 滚动淘汰最旧（含被拦截尝试）", () => {
    let s = scriptOf("sb-1", NOW);
    for (let i = 0; i < SCRIPT_CAP + 10; i++) {
      s = recordAction(s, { t: NOW + i, kind: i % 3 === 0 ? "blocked" : "net", detail: `a${i}`, blocked: i % 3 === 0 });
    }
    expect(s.actions).toHaveLength(SCRIPT_CAP);
    expect(s.actions[0]!.detail).toBe("a10");
    expect(s.actions.some((a) => a.blocked && a.detail === "a12")).toBe(true);
  });

  it("剧本摘要：总数/拦截数/分动作计数", () => {
    let s = scriptOf("sb-2", NOW);
    s = recordAction(s, { t: NOW, kind: "net", detail: "GET api" });
    s = recordAction(s, { t: NOW + 1, kind: "blocked", detail: "读隐私目录", blocked: true });
    s = recordAction(s, { t: NOW + 2, kind: "fs", detail: "写缓存" });
    const sum = scriptSummary(s);
    expect(sum.total).toBe(3);
    expect(sum.blocked).toBe(1);
    expect(sum.kinds.net).toBe(1);
    expect(sum.kinds.fs).toBe(1);
    expect(sum.kinds.blocked).toBe(1);
  });

  it("逐条暂停审阅：prev/cur/next/done，负索引如实钳制", () => {
    let s = scriptOf("sb-3", NOW);
    s = recordAction(s, { t: NOW, kind: "ui", detail: "一" });
    s = recordAction(s, { t: NOW + 1, kind: "ui", detail: "二" });
    const step1 = replayAt(s, 0);
    expect(step1.cur!.detail).toBe("一");
    expect(step1.prev).toBeNull();
    expect(step1.next!.detail).toBe("二");
    expect(step1.done).toBe(false);
    const stepEnd = replayAt(s, 2);
    expect(stepEnd.done).toBe(true);
    expect(replayAt(s, -0.5).index).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-138 插件讣告
// ---------------------------------------------------------------------------

describe("W-138 插件讣告", () => {
  it("服役天数向下取整；在役（uninstalledAt=0）按当前时刻", () => {
    expect(servedDays({ pluginId: "p", installedAt: NOW - 3 * DAY, uninstalledAt: NOW, runs: 1, rating: null }, NOW)).toBe(3);
    expect(servedDays({ pluginId: "p", installedAt: NOW - 3 * DAY + 1000, uninstalledAt: 0, runs: 1, rating: null }, NOW)).toBe(2);
    expect(servedDays({ pluginId: "p", installedAt: NOW + DAY, uninstalledAt: 0, runs: 1, rating: null }, NOW)).toBe(0);
  });

  it("评分星数与未评分如实 UNRATED", () => {
    expect(ratingText(null)).toBe("UNRATED");
    expect(ratingText(4.6)).toBe("★★★★★ (5/5)");
    expect(ratingText(3.2)).toBe("★★★☆☆ (3/5)");
  });

  it("告别文案随真实数据分档", () => {
    expect(farewellFor(0, null)).toContain("未及留下足迹");
    expect(farewellFor(150, 4.8)).toContain("老伙计");
    expect(farewellFor(20, 4)).toContain("功成身退");
    expect(farewellFor(2, 3)).toContain("再会");
  });

  it("零统计新装插件 → 简短版讣告；有统计 → 全量", () => {
    const brief = obituaryOf({ pluginId: "new", installedAt: NOW - DAY, uninstalledAt: NOW, runs: 0, rating: null }, NOW);
    expect(brief.brief).toBe(true);
    expect(brief.runs).toBe(0);
    const full = obituaryOf({ pluginId: "old", installedAt: NOW - 100 * DAY, uninstalledAt: NOW, runs: 120, rating: 4.7 }, NOW);
    expect(full.brief).toBe(false);
    expect(full.servedDays).toBe(100);
    expect(full.ratingText).toContain("★");
  });
});

// ---------------------------------------------------------------------------
// 通用与行为层安全（node 环境 no-op 语义）
// ---------------------------------------------------------------------------

describe("通用与行为层", () => {
  it("clamp 边界", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
  });

  it("激活/卸载在非 DOM 环境安全 no-op（幂等）", () => {
    expect(() => activateEcoNova()).not.toThrow();
    expect(isEcoNovaActive()).toBe(false);
    expect(() => deactivateEcoNova()).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// 局部类型（测试内轻量；与 ecoNova.LHSample 结构同源）
// ---------------------------------------------------------------------------

interface LHSampleLike {
  pluginId: string;
  ts: number;
  cpuPct: number;
  memMb: number;
  bootMs: number;
  eventsPerMin: number;
}
