import { beforeEach, describe, expect, it } from "vitest";
import {
  TransitionDriver, diffTokenTables, suggestContrastFix, auditTableContrast,
  scanHardcodedColors, scanProjectHardcodes, deriveTableFromVthemeColors,
  exportVthemeColors, transitionFrame, TRANSITION_TOTAL_MS, SWAP_POINT,
} from "../theme-engine";
import {
  scheduleDownloads, completeDownload, inIdleWindow, WallpaperCache, planPreload,
  pickForMonitors, orientationTransform, parseExifOrientation, DEFAULT_QUEUE_CONFIG, MAX_ATTEMPTS,
  type DownloadTask,
} from "../wallpaper-engine";
import {
  IconCache, cacheKey, iconInvalidationBus, auditSvgAsset, deepValidatePack,
  migratePackCoverage,
} from "../icon-engine";
import { defaultTokenTable, type TokenTable } from "../tokens";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("theme-engine · 过渡编排（F153 深化）", () => {
  it("双层交叉全程：旧 1→0、新 0→1（亮度守恒无白屏）", () => {
    expect(TRANSITION_TOTAL_MS).toBe(300);
    expect(SWAP_POINT).toBe(0.5); // 中段原子替换点钉死（一处一事实）
    const t0 = transitionFrame(0);
    expect(t0.oldOpacity).toBe(1);
    const tMid = transitionFrame(150);
    expect(tMid.oldOpacity).toBeCloseTo(0.5);
    expect(tMid.newOpacity).toBeCloseTo(0.5);
    expect(tMid.oldOpacity + tMid.newOpacity).toBeCloseTo(1);
    const t1 = transitionFrame(300);
    expect(t1.newOpacity).toBe(1);
  });

  it("中段原子替换：首次越过 50% 触发恰好一次（跨帧跳变安全）", () => {
    const d = new TransitionDriver();
    d.start();
    const f1 = d.tick(100);
    expect(f1.doSwap).toBe(false);
    // 跳过精确中点：直接到 160ms（>150ms）——首次越过即触发。
    const f2 = d.tick(160);
    expect(f2.doSwap).toBe(true);
    const f3 = d.tick(200);
    expect(f3.doSwap).toBe(false);
    const f4 = d.tick(300);
    expect(f4.phase).toBe("done");
    expect(d.isDone).toBe(true);
  });
});

describe("theme-engine · 令牌 diff 与对比度自修（F151/F161 深化）", () => {
  it("diff 逐令牌定位（颜色/圆角/动效分kind）", () => {
    const a = defaultTokenTable();
    const b: TokenTable = JSON.parse(JSON.stringify(a));
    b.colors["--p-accent"] = "#ff8800";
    b.radius.window = 24;
    b.motion.enter.duration = "panel";
    const d = diffTokenTables(a, b);
    expect(d.some((e) => e.kind === "color" && e.key === "--p-accent")).toBe(true);
    expect(d.some((e) => e.kind === "radius" && e.key === "radius.window")).toBe(true);
    expect(d.some((e) => e.kind === "motion" && e.key === "motion.enter")).toBe(true);
  });

  it("对比度自修：不达标色给出达标建议且保持色相（只调明度）", () => {
    // 浅底 + 浅字 → 建议压暗。
    const fix = suggestContrastFix("#cccccc", "#f0f0f0");
    expect(fix).not.toBeNull();
    expect(fix!.ratioAfter).toBeGreaterThanOrEqual(4.5);
    // 达标组合 → null（无需修）。
    expect(suggestContrastFix("#222222", "#f0f0f0")).toBeNull();
  });

  it("全表审计：出厂主题零红牌（不达标注定可查）", () => {
    const fixes = auditTableContrast(defaultTokenTable());
    // 出厂主题必须自证清白（B-1104 联动）。
    expect(fixes).toHaveLength(0);
  });
});

describe("theme-engine · 硬编码扫描器（B-1104 执法）", () => {
  it("命中出生主题物理值并给出令牌建议；clean 文件零误报", () => {
    const src = 'const a = "#6e7fd4"; // 硬编码\nconst ok = "var(--p-accent)";';
    const hits = scanHardcodedColors("demo.ts", src);
    expect(hits).toHaveLength(1);
    expect(hits[0]?.line).toBe(1);
    expect(hits[0]?.suggestion).toBe("var(--p-accent)");
    const clean = scanHardcodedColors("clean.ts", 'const x = "#abcdef";');
    expect(clean).toHaveLength(0);
  });

  it("批量扫描汇总（filesClean/filesTotal）", () => {
    const r = scanProjectHardcodes({ a: 'x("#6e7fd4")', b: "clean" });
    expect(r.filesTotal).toBe(2);
    expect(r.filesClean).toBe(1);
    expect(r.hits).toHaveLength(1);
  });

  it("vtheme 双向桥：派生 + 反导出 round-trip", () => {
    const d = defaultTokenTable();
    const vcolors = exportVthemeColors(d);
    const { table, unmapped } = deriveTableFromVthemeColors(vcolors);
    expect(unmapped).toHaveLength(0);
    expect(table.colors["--p-accent"]).toBe(d.colors["--p-accent"]);
  });
});

describe("wallpaper-engine · 下载队列与缓存（F154 深化）", () => {
  it("空闲窗口内并发调度；窗口外不启动", () => {
    const tasks = [
      { wallpaperId: "a", url: "u", maxBytes: 100, checksum: "x", state: "queued" as const, attempts: 0, bytes: 0 },
      { wallpaperId: "b", url: "u", maxBytes: 100, checksum: "x", state: "queued" as const, attempts: 0, bytes: 0 },
      { wallpaperId: "c", url: "u", maxBytes: 100, checksum: "x", state: "queued" as const, attempts: 0, bytes: 0 },
    ];
    const inWindow = new Date("2026-09-26T03:00:00").getTime();
    const scheduled = scheduleDownloads(tasks, DEFAULT_QUEUE_CONFIG, inWindow);
    expect(scheduled.filter((t) => t.state === "downloading")).toHaveLength(2); // 并发 2
    const offWindow = new Date("2026-09-26T12:00:00").getTime();
    expect(scheduleDownloads(tasks, DEFAULT_QUEUE_CONFIG, offWindow).every((t) => t.state === "queued")).toBe(true);
    expect(inIdleWindow(inWindow, DEFAULT_QUEUE_CONFIG.idleWindow)).toBe(true);
  });

  it("完成判定：超限/校验失败 → 重试计数；三次失败静默回退", () => {
    let t: DownloadTask = { wallpaperId: "a", url: "u", maxBytes: 100, checksum: "good", state: "downloading", attempts: 0, bytes: 0 };
    t = completeDownload(t, 50, "good");
    expect(t.state).toBe("done");
    t = completeDownload({ ...t, state: "downloading" }, 99999, "good");
    expect(t.state).toBe("queued");
    expect(t.attempts).toBe(1);
    for (let i = 0; i < MAX_ATTEMPTS; i++) {
      t = completeDownload({ ...t, state: "downloading" }, 10, "bad");
    }
    expect(t.state).toBe("failed");
    expect(t.reason).toContain("回退");
  });

  it("缓存 LRU：容量上限逐出最旧；pin 豁免", () => {
    const cache = new WallpaperCache(300);
    cache.touch("w1", 100, 1);
    cache.touch("w2", 100, 2);
    cache.touch("w3", 100, 3);
    cache.pin("w3", true);
    const evict = cache.planEviction(100);
    expect(evict).toContain("w1");
    expect(evict).not.toContain("w3");
    cache.applyEviction(evict);
    expect(cache.stats().count).toBe(2);
  });

  it("预载计划：空闲窗口交集 + 顺延到窗口起点", () => {
    // 03:00 换壁纸，窗口 02:00-05:00 → 02:30 预载。
    const rotateAt = new Date("2026-09-27T03:00:00").getTime();
    const now = new Date("2026-09-26T12:00:00").getTime();
    const plan = planPreload(rotateAt, DEFAULT_QUEUE_CONFIG.idleWindow, now, "w-next");
    expect(plan?.preloadAt).toBe(new Date("2026-09-27T02:30:00").getTime());
  });

  it("多屏池：主屏跟随全局；副屏独立抽取", () => {
    const pools = [
      { monitorId: "main", poolIds: ["a", "b"], followGlobal: true },
      { monitorId: "side", poolIds: ["c", "d"], followGlobal: false },
    ];
    const r = pickForMonitors(pools, "a", { side: () => 0 });
    expect(r["main"]).toBe("a");
    expect(r["side"]).toBe("c");
  });

  it("EXIF：魔数嗅探 + 方向 6 交换宽高", () => {
    expect(orientationTransform(6).swapWH).toBe(true);
    expect(orientationTransform(1).rotateDeg).toBe(0);
    // 构造 JPEG + APP1 + Orientation=6。
    const buf = new ArrayBuffer(64);
    const v = new DataView(buf);
    v.setUint16(0, 0xffd8);
    v.setUint16(2, 0xffe1);
    v.setUint16(4, 32); // APP1 长度
    const enc = new TextEncoder();
    enc.encodeInto("Exif\0\0", new Uint8Array(buf, 6, 6));
    v.setUint16(12, 0x4949); // TIFF "II"（小端）
    v.setUint16(14, 42);
    v.setUint32(16, 8, true); // IFD0 相对偏移（小端存储——与 "II" 一致）→ ifd0 = 12+8 = 20
    v.setUint16(20, 1, true); // entry count（小端）
    v.setUint16(22, 0x0112, true); // Orientation tag
    v.setUint16(24, 3, true); // type SHORT
    v.setUint32(26, 1, true); // count
    v.setUint16(30, 6, true); // value = 6
    expect(parseExifOrientation(buf)).toBe(6);
    expect(parseExifOrientation(new ArrayBuffer(2))).toBe(1);
  });
});

describe("icon-engine · 缓存/总线/深校验/迁移（F155 深化）", () => {
  it("缓存键含包版本（换包不误命中旧包缓存）", () => {
    const cache = new IconCache(10);
    const k1 = { packVersion: "1.0", category: "system.folder", size: "24" as const };
    cache.put(k1, "data:old");
    expect(cacheKey(k1)).toBe("1.0:system.folder:24"); // 键序钉死——失效按版本前缀依赖此格式
    expect(cache.get(k1)).toBe("data:old");
    expect(cache.get({ ...k1, packVersion: "2.0" })).toBeNull();
  });

  it("LRU 容量逐出 + 整包失效（按版本前缀）", () => {
    const cache = new IconCache(2);
    cache.put({ packVersion: "1.0", category: "a", size: "24" }, "1");
    cache.put({ packVersion: "1.0", category: "b", size: "24" }, "2");
    cache.put({ packVersion: "1.0", category: "c", size: "24" }, "3"); // 逐出 a
    expect(cache.get({ packVersion: "1.0", category: "a", size: "24" })).toBeNull();
    expect(cache.get({ packVersion: "1.0", category: "c", size: "24" })).toBe("3");
    cache.put({ packVersion: "2.0", category: "a", size: "24" }, "v2");
    expect(cache.invalidatePack("2.0")).toBe(1);
    expect(cache.get({ packVersion: "2.0", category: "a", size: "24" })).toBeNull();
  });

  it("失效广播：订阅方收到事件（订阅制不轮询）", () => {
    const seen: string[] = [];
    const off = iconInvalidationBus.subscribe((e) => seen.push(e.reason));
    iconInvalidationBus.broadcast({ reason: "switch", packId: "p", packVersion: "1", at: Date.now() });
    off();
    iconInvalidationBus.broadcast({ reason: "rollback", packId: null, packVersion: null, at: Date.now() });
    expect(seen).toEqual(["switch"]);
  });

  it("SVG 安全审计：脚本注入向量全拒", () => {
    expect(auditSvgAsset('<svg><script>alert(1)</script></svg>').ok).toBe(false);
    expect(auditSvgAsset('<svg onload="x()"></svg>').ok).toBe(false);
    expect(auditSvgAsset('<a href="javascript:x()">y</a>').ok).toBe(false);
    expect(auditSvgAsset('<svg><path d="M0 0"/></svg>').ok).toBe(true);
  });

  it("整包深校验：缺资产 warning、坏 SVG error、规范外类别 error", () => {
    const pack = {
      id: "p", name: "p", version: "1", checksum: "c",
      coverage: { "system.folder": "folder.svg", "system.start": "missing.svg", "bogus.cat": "x.svg" },
    };
    const issues = deepValidatePack(pack, (ref) => (ref === "folder.svg" ? "<svg/>" : ref === "x.svg" ? "<svg><script/></svg>" : null));
    expect(issues.some((i) => i.category === "system.start" && i.level === "warning")).toBe(true);
    expect(issues.some((i) => i.category === "bogus.cat" && i.level === "error")).toBe(true);
    expect(issues.some((i) => i.category === "system.folder" && i.level === "error")).toBe(false); // 合法资产零误报
  });

  it("覆盖表版本迁移：改名映射 + 冲突保留新键", () => {
    const r = migratePackCoverage({ "system.computer": "old.svg" });
    expect(r.migrated["system.this-pc"]).toBe("old.svg");
    expect(r.migrated["system.computer"]).toBeUndefined();
    expect(r.renamed).toHaveLength(1);
    const r2 = migratePackCoverage({ "system.computer": "a", "system.this-pc": "b" });
    expect(r2.migrated["system.this-pc"]).toBe("b");
    expect(r2.renamed).toHaveLength(0);
  });
});
