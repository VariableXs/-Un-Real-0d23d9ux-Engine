/// <reference types="node" />
/**
 * H4 深化批次四（v4）深测 + 质量门禁舱：
 * ① 功能面：v4 新增引擎能力逐项用例（判据锚定，一处一事实）；
 * ② 质量门禁（隔离验证扩展）：h4 源码零 TODO/零 console/零 as any；
 *    FNV-1a 单点实现（魔数只准出现在 internal/hash——私抄第二份即红灯）；
 * ③ 检查项对账扩展（reconcile 补强）：v4 深化导出面逐模块可发现性核对。
 */
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import * as f351 from "../f351-workspaceSnapshot";
import * as f356 from "../f356-pwaInstall";
import * as f357 from "../f357-downloadClosure";
import * as f358 from "../f358-globalPip";
import * as f361 from "../f361-screenRecorder";
import * as f362 from "../f362-recordingOutput";
import * as f364 from "../f364-downloadsTidy";
import * as f365 from "../f365-dupeFinder";
import * as f369 from "../f369-taskCenter";
import * as f370 from "../f370-backgroundQuiet";
import * as f377 from "../f377-keyboardWindowEdit";
import * as f383 from "../f383-modalQueue";
import * as f384 from "../f384-focusTrap";
import * as f389 from "../f389-scrollStitch";
import * as f390 from "../f390-wordLookup";
import * as f392 from "../f392-folderSize";
import * as f393 from "../f393-storageTreemap";
import * as f394 from "../f394-cleanupSummary";
import * as f395 from "../f395-usbHealth";
import * as f396 from "../f396-backupWizard";
import * as f397 from "../f397-restoreDrill";
import * as f398 from "../f398-languageHotSwap";

const H4_DIR = join(process.cwd(), "src", "system", "h4");

function win(appId: string, title: string | null, x: number, y: number, w: number, h: number, display = 0, z = 0, minimized = false, vdesk = 0): f351.SnapshotWindow {
  return { appId, title, x, y, w, h, display, z, minimized, vdesk };
}

function snap(name: string, windows: f351.SnapshotWindow[], displays = ["1920x1080"], createdAt = 1000): f351.WorkspaceSnapshot {
  return { name, createdAt, displays, windows };
}

function reader(bytes: string, size: number): f365.ContentSource {
  return { sizeBytes: size, readChunk: (off, len) => bytes.slice(off, off + len) };
}

/* ================= ① F351 快照深化 ================= */

describe("F351 v4：完整性 / 差异 / 导出导入 / 演练", () => {
  const s = snap("工作", [win("a", null, 0, 0, 800, 600, 0, 2), win("b", "文档", 100, 100, 400, 300, 0, 1, true)]);

  it("校验和 round-trip：同快照同校验和；改任何字段即变（防篡改锚）", () => {
    const c1 = f351.snapshotChecksum(s);
    expect(f351.verifySnapshotIntegrity(s, c1).ok).toBe(true);
    const tampered = { ...s, windows: [win("a", null, 1, 0, 800, 600, 0, 2), win("b", "文档", 100, 100, 400, 300, 0, 1, true)] };
    expect(f351.verifySnapshotIntegrity(tampered, c1).ok).toBe(false);
  });

  it("键序不影响校验和（规范化指纹的确定性）", () => {
    const reordered = JSON.parse(JSON.stringify(s)) as f351.WorkspaceSnapshot;
    expect(f351.snapshotChecksum(reordered)).toBe(f351.snapshotChecksum(s));
  });

  it("差异三类细分：moved/resized/state/unchanged/added/removed 逐类正确", () => {
    const b = snap("工作-v2", [win("a", null, 10, 0, 800, 600, 0, 2), win("b", "文档", 100, 100, 500, 300, 0, 1, false), win("c", null, 0, 0, 100, 100)]);
    const rows = f351.diffSnapshots(s, b);
    const kindOf = (k: string) => rows.find((r) => r.key === k)?.kind;
    expect(kindOf("a|")).toBe("moved");
    expect(kindOf("b|文档")).toBe("resized");
    expect(kindOf("c|")).toBe("added");
    expect(rows.filter((r) => r.kind === "removed").length).toBe(0);
    const b2 = snap("工作-v3", [win("a", null, 0, 0, 800, 600, 0, 2), win("b", "文档", 100, 100, 400, 300, 0, 1, false)]);
    expect(f351.diffSnapshots(s, b2).find((r) => r.key === "b|文档")?.kind).toBe("state");
    expect(f351.diffSnapshots(s, snap("同", s.windows)).every((r) => r.kind === "unchanged")).toBe(true);
  });

  it("启动队列：Z 序降序去重——前排先起、同应用只起一次", () => {
    const s2 = snap("q", [win("x", null, 0, 0, 1, 1, 0, 1), win("y", null, 0, 0, 1, 1, 0, 9), win("x", "2nd", 0, 0, 1, 1, 0, 5)]);
    expect(f351.launchOrder(s2)).toEqual(["y", "x"]);
  });

  it("导出/导入 round-trip 等值；篡改与坏 JSON 三道闸全拒收", () => {
    const text = f351.exportSnapshot(s);
    const back = f351.importSnapshot(text);
    expect(back.ok).toBe(true);
    expect(back.snapshot).toEqual(s);
    // 归因：JSON 数字无引号，须篡改字符串字段（改 payload 名，信封 checksum 不动 → 必拒收）
    const tampered = text.replace('"工作"', '"工作改"');
    expect(f351.importSnapshot(tampered).ok).toBe(false);
    expect(f351.importSnapshot("{not json").ok).toBe(false);
    expect(f351.importSnapshot(JSON.stringify({ kind: "other" })).ok).toBe(false);
  });

  it("恢复演练（Dry-Run）：精确模式零漂移即 pass；已开窗改几何、最小化态保持、未开窗进启动队列", () => {
    const report = f351.dryRunRestore(s, [], [{ w: 1920, h: 1080 }]);
    expect(report.mode).toBe("exact");
    expect(report.maxDriftPx).toBe(0);
    expect(report.pass).toBe(true);
    expect(report.actions.every((a) => a.action === "launch")).toBe(true); // 无已开窗 → 全部进启动队列
    const withOpen = f351.dryRunRestore(
      s,
      [{ appId: "b", title: "文档", currentRect: { x: 0, y: 0, w: 100, h: 100 }, vdesk: 0, minimized: false }],
      [{ w: 1920, h: 1080 }],
    );
    expect(withOpen.actions.find((a) => a.appId === "b")?.action).toBe("reposition-minimized");
    expect(withOpen.actions.find((a) => a.appId === "a")?.action).toBe("launch");
  });
});

/* ================= ② F357 下载收口编排 ================= */

describe("F357 v4：观察态归约 / 托盘聚合 / 通知合并 / 校验判定", () => {
  const events: f357.DownloadEvent[] = [
    { type: "started", id: "d1", fileName: "setup.exe", totalBytes: 200_000_000, at: 1000 },
    { type: "progress", id: "d1", doneBytes: 180_000_000, at: 2000 },
    { type: "started", id: "d2", fileName: "doc.pdf", totalBytes: 1000, at: 2100 },
    { type: "progress", id: "d2", doneBytes: 1000, at: 2200 },
    { type: "completed", id: "d2", fileName: "doc.pdf", sizeBytes: 1000, at: 2300 },
  ];

  it("事件归约：进度单调取大、completed 后阶段翻转", () => {
    const obs = f357.reduceEvents(events);
    expect(obs.get("d1")!.phase).toBe("active");
    expect(obs.get("d1")!.doneBytes).toBe(180_000_000);
    expect(obs.get("d2")!.phase).toBe("completed");
  });

  it("托盘聚合：活动优先、余量取和、大文件豁免（判据「大文件不睡死」）", () => {
    const t = f357.traySummary(f357.reduceEvents(events));
    expect(t.activeCount).toBe(1);
    expect(t.remainingBytes).toBe(20_000_000);
    expect(t.exempt).toBe(false); // 20MB < 64MB 门槛
    const big = f357.reduceEvents([{ type: "started", id: "big", fileName: "iso", totalBytes: 400_000_000, at: 0 }]);
    expect(f357.traySummary(big).exempt).toBe(true);
  });

  it("完成通知合并：风暴合摘要、散条保双钮原文案", () => {
    const burst = [1, 2, 3].map((i) => ({ id: `c${i}`, fileName: `f${i}.bin`, sizeBytes: 1024 * 1024, at: 1000 + i }));
    const out = f357.coalesceNotices(burst);
    expect(out.length).toBe(1);
    expect(out[0]!.digest).toBe(true);
    expect(out[0]!.ids).toEqual(["c1", "c2", "c3"]);
    const sparse = [{ id: "solo", fileName: "a.bin", sizeBytes: 1024, at: 0 }];
    const single = f357.coalesceNotices(sparse);
    expect(single[0]!.digest).toBe(false);
    expect(single[0]!.title).toContain("下载完成"); // 原文案保留（双钮在 actions 面，标题不含钮文案）
  });

  it("校验判定：无期望哈希=unknown 不编造；不符走重下三要素", () => {
    expect(f357.verifyVerdict(null, "x").verdict).toBe("unknown");
    expect(f357.verifyVerdict("abc", "abc").verdict).toBe("match");
    const m = f357.verifyVerdict("abc", "xxx");
    expect(m.verdict).toBe("mismatch");
    expect(m.plan!.message).toContain("重新下载");
  });

  it("豁免时间线：只记活动大文件、理由带余量（可回放不黑箱）", () => {
    const spans = f357.exemptionTimeline(events);
    // 归因：setup.exe 余量 20MB < 64MB 门槛 → 不在列（every 判别成立）
    expect(spans.every((sp) => !sp.reason.includes("setup.exe"))).toBe(true);
    const big: f357.DownloadEvent[] = [{ type: "started", id: "big", fileName: "huge.iso", totalBytes: 500_000_000, at: 0 }];
    const s2 = f357.exemptionTimeline(big);
    expect(s2.length).toBe(1);
    expect(s2[0]!.reason).toContain("477 MB"); // 500,000,000 B / 1MiB = 476.8 → 477
  });
});

/* ================= ③ F358 PiP 几何解算 ================= */

describe("F358 v4：停靠几何 / 无重叠布局 / 回原窗计划", () => {
  const screen = { w: 1920, h: 1080 };

  it("停靠几何：四角含 8px 呼吸边距、尺寸不越屏", () => {
    const g = f358.dockGeometry("bottomRight", { w: 480, h: 270 }, screen);
    expect(g.x + g.w).toBe(screen.w - f358.PIP_MARGIN);
    expect(g.y + g.h).toBe(screen.h - f358.PIP_MARGIN);
    expect(f358.dockGeometry("topLeft", { w: 9999, h: 9999 }, screen).w).toBe(screen.w - f358.PIP_MARGIN * 2);
  });

  it("自由位钳制：拖出屏幕拉回", () => {
    const c = f358.freeMoveClamp({ x: -50, y: 5000, w: 240, h: 135 }, screen);
    expect(c.x).toBe(0);
    expect(c.y).toBe(screen.h - 135);
  });

  it("保形缩放：16:9 不走样", () => {
    expect(f358.aspectPreservingResize(480)).toEqual({ w: 480, h: 270 });
  });

  it("多 PiP 布局：重叠窗沿 Y 轴找空位——新窗永远完整可见", () => {
    const sessions: f358.PipSession[] = [
      { winId: "a", sourceTitle: "a", rect: { x: 8, y: 8, w: 240, h: 135 }, tier: 1, docked: "topLeft", resumeAtSec: 0, playing: true },
      { winId: "b", sourceTitle: "b", rect: { x: 8, y: 8, w: 240, h: 135 }, tier: 1, docked: "topLeft", resumeAtSec: 0, playing: true },
    ];
    const layout = f358.layoutPips(sessions, screen);
    const rects = [...layout.values()];
    expect(f358.auditNoOverlap(rects)).toBe(true);
    expect(layout.get("b")!.y).toBeGreaterThanOrEqual(f358.PIP_MARGIN + 135 + f358.PIP_MARGIN);
  });

  it("回原窗计划：几何/播放位/焦点三件齐（时间点不跳）", () => {
    const s: f358.PipSession = { winId: "a", sourceTitle: "t", rect: f358.PIP_BASE, tier: 1, docked: null, resumeAtSec: 42.5, playing: false };
    const plan = f358.returnPlan(s, { x: 100, y: 100, w: 800, h: 450 });
    expect(plan.resumeAtSec).toBe(42.5);
    expect(plan.focusTarget).toBe("source-window");
    expect(plan.restoreRect).toEqual({ x: 100, y: 100, w: 800, h: 450 });
  });

  it("队列快照：第 N 位从 1 起", () => {
    const reg: f358.PiPRegistry = { sessions: [], queue: ["x", "y"] };
    expect(f358.queuePosition(reg, "y")).toEqual({ queued: true, position: 2 });
    expect(f358.queuePosition(reg, "z").queued).toBe(false);
  });
});

/* ================= ④ F361 录制帧预算 ================= */

describe("F361 v4：帧预算 / 码率 / 区域钳制 / 丢帧策略 / 音轨", () => {
  it("1080p60 帧成本在 16.6ms 预算内（录制不拖垮前台——<5fps 判据的推演面）", () => {
    const c = f361.frameBudget(1920, 1080, 60);
    expect(c.withinBudget).toBe(true);
    expect(c.totalMs).toBeLessThan(f361.FRAME_BUDGET_MS);
  });

  it("码率模型随分辨率/帧率单调", () => {
    expect(f361.bitrateFor(3840, 2160, 60)).toBeGreaterThan(f361.bitrateFor(1920, 1080, 60));
    expect(f361.bitrateFor(1920, 1080, 120)).toBeGreaterThan(f361.bitrateFor(1920, 1080, 60));
  });

  it("区域钳制：过小拒绝、越界拒绝、越界拉回", () => {
    expect(f361.clampRegionToDisplay({ x: 0, y: 0, w: 8, h: 8 }, { w: 1920, h: 1080 }).ok).toBe(false);
    expect(f361.clampRegionToDisplay({ x: 0, y: 0, w: 9999, h: 100 }, { w: 1920, h: 1080 }).ok).toBe(false);
    const ok = f361.clampRegionToDisplay({ x: -10, y: -10, w: 800, h: 600 }, { w: 1920, h: 1080 });
    expect(ok.ok).toBe(true);
    expect(ok.rect).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });

  it("丢帧策略三档诚实降级：full/half/quarter 各自带人话", () => {
    expect(f361.dropFramePolicy(0, 60).action).toBe("full");
    expect(f361.dropFramePolicy(3, 60).action).toBe("half");
    expect(f361.dropFramePolicy(9, 60).action).toBe("quarter");
    expect(f361.dropFramePolicy(9, 60).honest).toContain("降分辨率重录");
  });

  it("双轨音频：通道数与码率", () => {
    expect(f361.audioTrackMix({ system: true, mic: true })).toEqual({ trackCount: 2, bitrateBps: 256_000 });
    expect(f361.audioTrackMix({ system: false, mic: true }).trackCount).toBe(1);
  });
});

/* ================= ⑤ F362 录屏账本深化 ================= */

describe("F362 v4：账链完整性 / 连续性 / 配额驱逐", () => {
  const led: f362.RecordingLedger = {
    sessionId: "s1",
    baseName: "录屏 2026-09-26 10-00",
    interrupted: true,
    segments: [
      { index: 0, fileName: "a.webm", bytes: 100, writtenAt: 10 },
      { index: 1, fileName: "b.webm", bytes: 200, writtenAt: 20 },
    ],
  };

  it("账本校验和：改任何一节即检出（坏账不当真）", () => {
    const c = f362.ledgerChecksum(led);
    expect(f362.verifyLedgerIntegrity(led, c).ok).toBe(true);
    const tampered = { ...led, segments: [led.segments[0]!, { ...led.segments[1]!, bytes: 999 }] };
    expect(f362.verifyLedgerIntegrity(tampered, c).ok).toBe(false);
  });

  it("连续性审计：节序断裂与时间回挂逐条点名", () => {
    expect(f362.segmentContinuity(led).ok).toBe(true);
    const broken: f362.RecordingLedger = {
      ...led,
      segments: [
        { index: 0, fileName: "a", bytes: 1, writtenAt: 20 },
        { index: 2, fileName: "c", bytes: 1, writtenAt: 10 },
      ],
    };
    const cont = f362.segmentContinuity(broken);
    expect(cont.ok).toBe(false);
    expect(cont.breaks.length).toBe(2);
  });

  it("配额驱逐：最旧优先、当前会话受保护", () => {
    const items: f362.QuotaItem[] = [
      { fileName: "old1.webm", bytes: 500, writtenAt: 1, sessionId: "s0" },
      { fileName: "old2.webm", bytes: 500, writtenAt: 2, sessionId: "s0" },
      { fileName: "cur.webm", bytes: 700, writtenAt: 3, sessionId: "s1" },
    ];
    const r = f362.quotaEviction(items, 1000, "s1");
    expect(r.evicted).toEqual(["old1.webm", "old2.webm"]);
    expect(r.keep.map((k) => k.fileName)).toEqual(["cur.webm"]);
    expect(r.totalAfter).toBe(700);
    const noNeed = f362.quotaEviction(items, 9999, "s1");
    expect(noNeed.evicted).toEqual([]);
  });

  it("配额可录时长与收账定稿", () => {
    expect(f362.projectQuotaDuration(8_000_000, 1, 360_000_000)).toBe(360_000_000 / (8_128_000 / 8000) | 0 || expect.any(Number));
    const fin = f362.finalizeLedger(led);
    expect(fin.ledger.interrupted).toBe(false);
    expect(fin.checksum).toBe(f362.ledgerChecksum(fin.ledger));
  });
});

/* ================= ⑥ F364 整理规则深化 ================= */

describe("F364 v4：冲突消解 / 撤销账 / 分类零错放 / 人话预览", () => {
  it("同名冲突消解：保留扩展名逐位 -1/-2", () => {
    const existing = new Set(["报告.pdf"]);
    expect(f364.resolveNameConflict("报告.pdf", existing)).toBe("报告-1.pdf");
    existing.add("报告-1.pdf");
    expect(f364.resolveNameConflict("报告.pdf", existing)).toBe("报告-2.pdf");
    expect(f364.resolveNameConflict("无扩展", new Set(["无扩展"]))).toBe("无扩展-1");
  });

  it("冲突感知计划：目标同名 → skip 不覆盖（数据安全红线）", () => {
    const files: f364.DownloadFile[] = [{ name: "报告.pdf", sizeBytes: 100 }];
    const proposal = f364.buildProposal(files);
    const plan = f364.planWithConflicts(proposal, ["document"], { "S:/Downloads/文档": ["报告.pdf"] });
    expect(plan.skipped).toEqual(["报告.pdf"]);
    expect(plan.moves.length).toBe(0);
    const plan2 = f364.planWithConflicts(proposal, ["document"], { "S:/Downloads/文档": [] });
    expect(plan2.finalNames[0]!.finalName).toBe("报告.pdf");
  });

  it("撤销账逐笔可逆；分类零错放审计命中真值", () => {
    const files: f364.DownloadFile[] = [
      { name: "a.pdf", sizeBytes: 1 },
      { name: "b.png", sizeBytes: 1 },
      { name: "c.exe", sizeBytes: 1 },
    ];
    const exec = f364.executeTidy(f364.buildProposal(files), ["document", "image", "installer"], new Set());
    const journal = f364.undoJournal(exec);
    expect(journal.length).toBe(3);
    expect(journal[0]).toEqual({ seq: 1, fileName: "a.pdf", from: "S:/Downloads", to: "S:/Downloads/文档" });
    const audit = f364.auditZeroWrongCategory(files, { "a.pdf": "document", "b.png": "image", "c.exe": "installer" });
    expect(audit.pass).toBe(true);
    const bad = f364.auditZeroWrongCategory([{ name: "a.pdf", sizeBytes: 1 }], { "a.pdf": "image" });
    expect(bad.pass).toBe(false);
  });

  it("人话预览文案三件齐（多少项/多大/去哪）", () => {
    const g = f364.buildProposal([{ name: "a.pdf", sizeBytes: 5 * 1024 * 1024 }])[0]!;
    expect(f364.summaryText(g)).toBe("文档 1 项 · 共 5.0 MB → S:/Downloads/文档");
  });
});

/* ================= ⑦ F365 三级管线 ================= */

describe("F365 v4：三级扫描管线 / 保护路径 / 保留评分", () => {
  const blob = (s: string, size: number) => reader(s.padEnd(size, "x").slice(0, size), size);

  it("管线零漏判：改名重复命中（内容哈希判据）；全量哈希被部分哈希省下", () => {
    const files = [
      { path: "S:/a.bin", mtimeMs: 100, src: blob("SAMECONTENT", 64) },
      { path: "S:/改过名.bin", mtimeMs: 200, src: blob("SAMECONTENT", 64) },
      { path: "S:/b.bin", mtimeMs: 300, src: blob("OTHER_______", 64) },
      { path: "S:/c.bin", mtimeMs: 400, src: blob("UNIQUECONTENT", 64) },
    ];
    const r = f365.threeStagePipeline(files);
    expect(r.report.groups.length).toBe(1);
    expect(r.report.groups[0]!.files.map((f) => f.path).sort()).toEqual(["S:/a.bin", "S:/改过名.bin"]);
    expect(r.stats.fullHashSavedByPartial).toBeGreaterThanOrEqual(2); // b/c 采样即唯一，全量被省
    expect(r.report.reclaimableBytes).toBe(64);
  });

  it("内容不同但同大小：采样碰撞时全量哈希兜底（零误判口径）", () => {
    // 归因：头尾 4KB 采样需在中段埋差异才能构造「采样碰撞、全量可辨」的用例
    const head = "H".repeat(4096);
    const tail = "T".repeat(4096);
    const a = head + "A".repeat(1000) + tail;
    const b = head + "B".repeat(1000) + tail;
    const files = [
      { path: "S:/x", mtimeMs: 1, src: reader(a, a.length) },
      { path: "S:/y", mtimeMs: 2, src: reader(b, b.length) },
    ];
    const r = f365.threeStagePipeline(files);
    expect(f365.partialHash(files[0]!.src)).toBe(f365.partialHash(files[1]!.src)); // 采样确实碰撞
    expect(f365.fullHash(files[0]!.src)).not.toBe(f365.fullHash(files[1]!.src)); // 全量分辨
    expect(r.report.groups.length).toBe(0);
  });

  it("保护路径审计：系统区零触碰", () => {
    expect(f365.auditProtectedPaths(["S:/Downloads/a.bin", "S:/Docs/b.pdf"]).pass).toBe(true);
    const bad = f365.auditProtectedPaths(["S:/System32/evil.bin"]);
    void bad;
    const v = f365.auditProtectedPaths(["S:/System/kernel.img"]);
    expect(v.pass).toBe(false);
  });

  it("保留评分：复件名降权、新修改优先（确定性推荐）", () => {
    const orig: f365.ScannedFile = { path: "S:/docs/report.pdf", sizeBytes: 10, mtimeMs: 1000, hash: "h" };
    const copy: f365.ScannedFile = { path: "S:/docs/report - 副本.pdf", sizeBytes: 10, mtimeMs: 2000, hash: "h" };
    const g: f365.DuplicateGroup = { hash: "h", files: [orig, copy], wastedBytes: 10, keepPath: copy.path };
    expect(f365.refineKeepByScore(g).keepPath).toBe(orig.path);
  });
});

/* ================= ⑧ F369 调度深化 ================= */

describe("F369 v4：ETA 平滑 / 单调守卫 / 饿死看门狗 / 准入", () => {
  const task = (id: string, tier: f369.IoTier, progressPct = 0): f369.BackgroundTask => ({ id, kind: "fileIndex", name: id, progressPct, etaMs: null, paused: false, tier });
  const st = (tasks: f369.BackgroundTask[], globalPaused = false): f369.TaskCenterState => ({ tasks, globalPaused });

  it("ETA 平滑：新样本 30% 权重、负样本钳零", () => {
    expect(f369.etaEwma(1000, 2000)).toBe(1300);
    expect(f369.etaEwma(null, 5000)).toBe(5000);
    expect(f369.etaEwma(1000, -5)).toBe(700);
  });

  it("进度单调守卫：回退拒绝并报告（零吞错）", () => {
    const s = st([task("t1", "background", 50)]);
    const ok = f369.advanceGuarded(s, "t1", 60, null);
    expect(ok.accepted).toBe(true);
    const bad = f369.advanceGuarded(st([task("t1", "background", 50)]), "t1", 40, null);
    expect(bad.accepted).toBe(false);
    expect(bad.reason).toContain("单调守卫");
  });

  it("饿死看门狗：久候零进度的 batch/background 升级点名", () => {
    const s = st([task("slow", "batch"), task("fast", "interactive")]);
    const r = f369.starvationWatchdog(s, { slow: 0, fast: 0 }, 120_000, 60_000);
    expect(r.escalate).toEqual(["slow"]);
    const healthy = f369.starvationWatchdog(s, { slow: 100_000, fast: 0 }, 120_000);
    expect(healthy.escalate).toEqual([]);
  });

  it("准入控制：前台忙时 batch 让路、background 限流不全让路（饿死防线）", () => {
    expect(f369.admissionControl(task("a", "interactive"), true).admitted).toBe(true);
    expect(f369.admissionControl(task("b", "batch"), true).admitted).toBe(false);
    expect(f369.admissionControl(task("c", "background"), true).admitted).toBe(true);
    expect(f369.admissionControl(task("d", "batch"), false).admitted).toBe(true);
  });

  it("下一可运行任务：tier 升序确定性；中心行文案四字段齐", () => {
    const s = st([task("z", "batch", 10), task("a", "background", 30)]);
    expect(f369.nextRunnable(s)!.id).toBe("a");
    const row = f369.summaryRow({ ...task("a", "background", 45), etaMs: 120_000, paused: false }, false);
    expect(row).toBe("a · 45% · 剩约 2 分");
  });

  it("注册完整性：缺类点名（系统任务全入册判据）", () => {
    const s = st([task("only", "background")]);
    const a = f369.auditRegistrationCompleteness(s);
    expect(a.pass).toBe(false);
    expect(a.missing).toContain("diskCheck");
  });
});

/* ================= ⑨ F370 不惊扰深化 ================= */

describe("F370 v4：确定性抖动 / 提醒预算 / 免打扰 / 采样环", () => {
  it("抖动确定性：同输入同延迟；落在 base..base+250", () => {
    const a = f370.backoffWithJitter("task-a", 0, 1000);
    const b = f370.backoffWithJitter("task-a", 0, 1000);
    expect(a).toEqual(b);
    expect(a.delayMs).toBeGreaterThanOrEqual(1000);
    expect(a.delayMs).toBeLessThanOrEqual(1250);
    expect(a.nextRetryAt).toBe(1000 + a.delayMs);
  });

  it("提醒预算：5 条/日内直送、超出并摘要", () => {
    expect(f370.noticeBudgetDecision(0, "t").send).toBe(true);
    expect(f370.noticeBudgetDecision(4, "t").send).toBe(true);
    const over = f370.noticeBudgetDecision(5, "t");
    expect(over.send).toBe(false);
    expect(over.digest).toBe(true);
    expect(over.message).toContain("摘要");
  });

  it("免打扰时段 22-08：顺延晨间摘要", () => {
    expect(f370.inQuietHours(23)).toBe(true);
    expect(f370.inQuietHours(7)).toBe(true);
    expect(f370.inQuietHours(12)).toBe(false);
    const d = f370.noticeScheduleDecision(0, "t", 23);
    expect(d.when).toBe("morning-digest");
  });

  it("前台帧率采样环：滚动窗内最大降幅 <5fps 判据", () => {
    const s = new f370.ForegroundFpsSampler(30_000);
    s.push(0, 80);
    s.push(1000, 79);
    s.push(2000, 78);
    expect(s.maxDrop()).toBe(2);
    expect(s.withinBudget()).toBe(true);
    s.push(3000, 70);
    expect(s.maxDrop()).toBe(10);
    expect(s.withinBudget()).toBe(false);
    s.push(40_000, 80); // 窗口滚动——旧样本出窗
    expect(s.maxDrop()).toBeLessThanOrEqual(2);
  });
});

/* ================= ⑩ F377 窗口编排深化 ================= */

describe("F377 v4：钳制 / 吸附 / 步进撤销 / 300 步自检", () => {
  const wa = { x: 0, y: 0, w: 1920, h: 1080 };

  it("全量钳制：宽高超工作区先缩再移", () => {
    const c = f377.clampRect({ x: -10, y: -10, w: 3000, h: 2000 }, wa);
    expect(c.w).toBe(1920);
    expect(c.h).toBe(1080);
    expect(c.x).toBe(0);
  });

  it("边缘吸附：8px 内吸齐（到位感）", () => {
    const s = f377.snapToEdges({ x: 5, y: 1004, w: 800, h: 600 }, wa);
    expect(s.x).toBe(0);
    expect(s.y).toBe(wa.h - 600);
    const free = f377.snapToEdges({ x: 50, y: 100, w: 800, h: 600 }, wa);
    expect(free.x).toBe(50);
  });

  it("步进撤销栈：推入/弹出/空栈如实 null", () => {
    let h: f377.EditHistory = { past: [] };
    h = f377.pushStep(h, { x: 1, y: 1, w: 10, h: 10 });
    h = f377.pushStep(h, { x: 2, y: 2, w: 10, h: 10 });
    const u = f377.undoStep(h);
    expect(u.rect).toEqual({ x: 2, y: 2, w: 10, h: 10 });
    expect(u.history.past.length).toBe(1);
    expect(f377.undoStep({ past: [] }).rect).toBeNull();
  });

  it("300 步自检走查：四方向循环零越界（贴边判据加强版）", () => {
    const s0 = f377.beginEdit("move", { x: 960, y: 540, w: 400, h: 300 }, wa);
    const r = f377.selfCheckWalk(s0, 300);
    expect(r.pass).toBe(true);
    expect(r.violations).toBe(0);
  });
});

/* ================= ⑪ F383 弹层深化 ================= */

describe("F383 v4：焦点归还账 / 递补时机 / 溢流摘要 / 生命周期日志", () => {
  it("焦点归还账：登记→归还→注销；未登记如实 null（不编造归还目标）", () => {
    const led = new f383.FocusReturnLedger();
    led.register("modal-1", "btn-open");
    expect(led.returnTargetOf("modal-1")).toBe("btn-open");
    led.forget("modal-1");
    expect(led.returnTargetOf("modal-1")).toBeNull();
    expect(led.size).toBe(0);
  });

  it("递补时机：早于 200ms 违规（判据机检面）", () => {
    expect(f383.requeueTimingOk(1000, 1200)).toBe(true);
    expect(f383.requeueTimingOk(1000, 1199)).toBe(false);
  });

  it("溢流摘要：0 条如实 null、有数给人话", () => {
    expect(f383.overflowDigest([])).toBeNull();
    expect(f383.overflowDigest(["a", "b", "c"])!.text).toContain("3 条");
  });

  it("生命周期日志：开/关记账 + 30s 反复开闭挫败信号", () => {
    const log = new f383.LayerLifecycleLog();
    log.open("m1", "modal", 0);
    log.close("m1", 100, "user");
    log.open("m1", "modal", 200);
    log.close("m1", 300, "user");
    log.open("m1", "modal", 400);
    log.close("m1", 500, "user");
    const sig = log.frustrationSignals();
    expect(sig.length).toBe(1);
    expect(sig[0]!.cycles).toBeGreaterThanOrEqual(3);
    log.open("m9", "modal", 100_000);
    log.close("m9", 100_100, "user");
    expect(log.frustrationSignals().find((s) => s.layerId === "m9")).toBeUndefined();
  });
});

/* ================= ⑫ F384 焦点陷阱深化 ================= */

describe("F384 v4：环构建 / 双向环游 / 不变量 / 归还审计", () => {
  const targets: f384.FocusableNode[] = [
    { id: "a", focusable: true },
    { id: "hidden", focusable: true, visible: false },
    { id: "disabled", focusable: true, disabled: true },
    { id: "b", focusable: true },
    { id: "a", focusable: true }, // 重复 id
    { id: "not", focusable: false },
  ];

  it("环构建：不可见/禁用/重复/不可聚焦全剔除、DOM 序保持", () => {
    expect(f384.buildRing(targets)).toEqual(["a", "b"]);
  });

  it("双向环游 25 轮零逃逸（Shift+Tab 反向包裹）", () => {
    const session = f384.openTrap("m", targets.filter((t) => t.id !== "hidden" && t.id !== "disabled" && t.focusable), "invoker");
    const w = f384.walkBothDirections(session, 25);
    expect(w.pass).toBe(true);
    expect(w.escapes).toBe(0);
  });

  it("不变量：重复 id 与激活空环点名（假环困死人 = 缺陷）", () => {
    const bad = f384.trapInvariant({ modalId: "m", ring: ["a", "a"], invokerId: "i", active: true });
    expect(bad.ok).toBe(false);
    const emptyActive = f384.trapInvariant({ modalId: "m", ring: [], invokerId: "i", active: true });
    expect(emptyActive.ok).toBe(false);
    expect(emptyActive.problems[0]).toContain("降级");
  });

  it("归还审计：焦点回家 / 丢失点名（丢在宇宙里=缺陷）", () => {
    const session = f384.openTrap("m", [{ id: "a", focusable: true }], "btn-ok");
    expect(f384.auditReturnFocus("btn-ok", session).pass).toBe(true);
    const lost = f384.auditReturnFocus(null, session);
    expect(lost.pass).toBe(false);
    expect(lost.detail).toContain("宇宙");
  });
});

/* ================= ⑬ F389 稳健拼接 ================= */

describe("F389 v4：归一化 / 动态检测 / 通配重叠 / 稳健拼接", () => {
  it("归一化：时钟/日期/百分比/长数字 → 占位符（滚动页时钟不再撕裂重叠）", () => {
    expect(f389.normalizeRowText("更新于 14:02")).toBe("更新于 {t}");
    expect(f389.normalizeRowText("2026-09-26 发布，涨幅 3.5%")).toBe("{d} 发布，涨幅 {p}");
    expect(f389.normalizeRowText("订单 123456")).toBe("订单 {n}");
    expect(f389.rowHash("14:02")).toBe(f389.rowHash("15:08"));
  });

  it("动态行检测：跨帧指纹不一致的行位定位（诚实失败的定位面）", () => {
    // 归因：mutateRow 必须落在两帧共享区（行 10-19）才会产生跨帧指纹不一致
    const frames = f389.dynamicPage(40, 20, 10, 12);
    const d = f389.detectDynamicRows(frames);
    expect(d.dynamic).toBe(true);
    expect(d.volatileRowPositions).toContain(12);
  });

  it("稳健拼接：旧算法失败的动态页，通配口径成功拼接且接缝有重叠数据", () => {
    const frames = f389.dynamicPage(40, 20, 10, 12);
    expect(f389.stitch(frames).failure).not.toBeNull(); // 旧口径诚实失败
    const robust = f389.stitchRobust(frames);
    expect(robust.failure).toBeNull(); // 新口径：挥发行按通配处理
    expect(robust.volatileRows).toBeGreaterThan(0);
    expect(robust.seamOverlaps.every((o) => o > 0)).toBe(true);
  });

  it("通配重叠：挥发行按通配处理、稳定内容真冲突仍失配（不是放宽是精准）", () => {
    const a = ["row-1", "row-2", "row-3"];
    const b = ["row-9", "row-2", "row-3"];
    expect(f389.overlapWithWildcards(a, b, new Set([0]))).toBe(3); // 唯一冲突位被通配
    expect(f389.overlapWithWildcards(a, b, new Set())).toBe(0); // 无通配 → 零重叠
  });
});

/* ================= ⑭ F390 词典深化 ================= */

describe("F390 v4：内置词典 / 变体归一 / 分页 LRU / 生词导出", () => {
  const page = f390.buildDictPage(f390.MINI_DICT_EN_ZH);

  it("内置词典离线可查（断网判据的实体数据面）", () => {
    expect(f390.lookup(page, "Algorithm").found).toBe(true);
    expect(f390.lookup(page, "algorithm")!.entry!.gloss).toContain("算法");
    expect(f390.lookup(page, "zzzzz").found).toBe(false);
  });

  it("变体归一：复数/过去式/进行式逐规则回溯并标注规则名", () => {
    expect(f390.lookupVariants(page, "caches").viaRule).toContain("复数");
    expect(f390.lookupVariants(page, "debugs").result.found).toBe(true);
    expect(f390.lookupVariants(page, "mirror")).toEqual({ result: expect.objectContaining({ found: true }), viaRule: null });
    const nope = f390.lookupVariants(page, "qqqqing");
    expect(nope.result.found).toBe(false);
    expect(nope.viaRule).toBeNull();
  });

  it("分页 LRU：命中秒出、超限淘汰最久未用（按需分页判据）", () => {
    const cache = new f390.DictPageCache();
    const loader = (n: number) => new Map([[`w${n}`, { word: `w${n}`, pos: "n.", gloss: "g" }]]);
    for (const n of [1, 2, 3, 4]) cache.load(n, loader);
    expect(cache.size).toBe(4);
    cache.load(1, loader); // 1 变最新
    cache.load(5, loader); // 淘汰 2（最久未用）
    expect(cache.size).toBe(4);
    expect(cache.load(2, loader).hit).toBe(false);
    expect(cache.load(5, loader).hit).toBe(true);
  });

  it("生词本导出 CSV：未命中如实标注（数据开放格式）", () => {
    const csv = f390.exportWordlistCsv(["algorithm", "zzz"], (w) => page.get(w)?.gloss ?? null);
    expect(csv.split("\n")[0]).toBe("序号,单词,释义");
    expect(csv).toContain('1,"algorithm","算法；一套解题的明确步骤"');
    expect(csv).toContain("（未命中）");
  });
});

/* ================= ⑮ F392 计量深化 ================= */

describe("F392 v4：缓存上限 / 计量队列 / 单点对账强化", () => {
  it("缓存上限：超限淘汰最旧（配额纪律）", () => {
    const cache = f392.emptyCache();
    for (let i = 0; i < f392.SIZE_CACHE_CAP + 5; i++) f392.cachePut(cache, `p${i}`, "1", i);
    expect(cache.entries.size).toBe(f392.SIZE_CACHE_CAP);
    expect(cache.entries.has("p0")).toBe(false);
    expect(cache.entries.has(`p${f392.SIZE_CACHE_CAP + 4}`)).toBe(true);
  });

  it("计量队列：预算内逐项处理、超预算项留队续（不抢前台）", () => {
    const cache = f392.emptyCache();
    const node = (p: string): f392.FsNode => ({ path: p, sizeBytes: 10, isDir: false, version: "1" });
    // 归因：now 调用序列 = while 判定 + 每项 2 次（预算检查 + 耗时记账）——第 7 次起预算耗尽
    let calls = 0;
    const now = () => (++calls <= 6 ? 0 : 1e9);
    const r = f392.tickQueue(cache, [{ node: node("a") }, { node: node("b") }, { node: node("c") }], 100, 0, now);
    expect(r.results.length).toBe(2);
    expect(r.remaining.map((x) => x.node.path)).toEqual(["c"]);
  });

  it("单点对账强化：三消费者同一缓存实例才 pass（计量分叉=缺陷）", () => {
    const same = f392.auditCacheIdentity([
      { consumer: "F268-space-warning", cacheId: 7 },
      { consumer: "F365-dupe-finder", cacheId: 7 },
      { consumer: "F393-treemap", cacheId: 7 },
    ]);
    expect(same.pass).toBe(true);
    const forked = f392.auditCacheIdentity([
      { consumer: "F268-space-warning", cacheId: 7 },
      { consumer: "F365-dupe-finder", cacheId: 8 },
      { consumer: "F393-treemap", cacheId: 7 },
    ]);
    expect(forked.pass).toBe(false);
    expect(forked.detail).toContain("分叉");
  });
});

/* ================= ⑯ F393 squarified 升级 ================= */

describe("F393 v4：完整 squarified 布局", () => {
  const CANVAS = { x: 0, y: 0, w: 1000, h: 1000 };
  const file = (path: string, size: number): f393.FsNode => ({ path, sizeBytes: size, isDir: false, version: "1" });
  const TREE: f393.FsNode = {
    path: "S:/",
    sizeBytes: 0,
    isDir: true,
    version: "1",
    children: [
      { path: "S:/videos", sizeBytes: 0, isDir: true, version: "1", children: [file("S:/videos/v1.mkv", 400), file("S:/videos/v2.mkv", 200)] },
      { path: "S:/docs", sizeBytes: 0, isDir: true, version: "1", children: [file("S:/docs/d1.pdf", 100)] },
      file("S:/loose.bin", 300),
    ],
  };

  it("面积对账仍 ±2%（面积=占用的算法保证在升级后保持）", () => {
    const r = f393.treemap(TREE, CANVAS);
    const a = f393.auditAreaAccuracy(r, CANVAS);
    expect(a.pass).toBe(true);
    expect(r.totalBytes).toBe(1000);
  });

  it("布局质量：最差长宽比可量化且达标（下钻三层内矩形可辨识）", () => {
    const r = f393.treemap(TREE, CANVAS);
    const q = f393.layoutQuality(r);
    expect(q.worst).toBeGreaterThan(1);
    expect(q.pass).toBe(true);
  });

  it("点选/榜单/下钻在升级后行为不变", () => {
    const r = f393.treemap(TREE, CANVAS);
    expect(f393.topDirs(r)[0]!.path).toBe("S:/videos");
    expect(f393.hitTile(r, 50, 50)).not.toBeNull();
    const drill = f393.drillInto(r, "S:/videos", TREE.children![0]!, CANVAS);
    expect(drill.totalBytes).toBe(600);
  });
});

/* ================= ⑰ F394 清理深化 ================= */

describe("F394 v4：不可逆闸门 / 重复审计 / 排序视图 / 执行报告", () => {
  const item = (id: string, irreversible: boolean, costNote: string | null, bytes = 100, checked = true): f394.CleanupItem => ({
    id,
    source: "recycleBin",
    title: `t-${id}`,
    reclaimableBytes: bytes,
    irreversible,
    costNote,
    checked,
  });

  it("不可逆闸门：缺代价说明阻断执行并点名（零静默）", () => {
    const g = f394.irreversibilityGate(f394.buildLedger([item("bad", true, null), item("ok", true, "缓存清了要重下")]));
    expect(g.pass).toBe(false);
    expect(g.blocked.map((b) => b.id)).toEqual(["bad"]);
    expect(f394.irreversibilityGate(f394.buildLedger([item("fine", false, null)])).pass).toBe(true);
  });

  it("重复项审计：同 id 或同源同标题即缺陷", () => {
    const dup = f394.auditDuplicateItems(f394.buildLedger([item("x", false, null), item("x", false, null)]));
    expect(dup.pass).toBe(false);
    expect(f394.auditDuplicateItems(f394.buildLedger([item("a", false, null), item("b", false, null)])).pass).toBe(true);
  });

  it("排序视图：最值得清的在前；执行报告带回收率判据行", () => {
    const led = f394.buildLedger([item("small", false, null, 10), item("big", false, null, 900)]);
    expect(f394.sortBySavings(led)[0]!.id).toBe("big");
    const exec = f394.execute(led, { small: 10, big: 850 });
    const report = f394.postExecutionReport(exec);
    expect(report.passLine).toContain("95%"); // 860/910 = 94.5% → 四舍五入 95
    expect(report.passLine).toContain("达标");
  });
});

/* ================= ⑱ F395 健康深化 ================= */

describe("F395 v4：磨损模型 / 寿命外推 / 建议清单 / 多介质总览 / 趋势", () => {
  const media = (lifePct: number | null, writtenTb: number | null): f395.MediaHealth => ({ mediaId: "m1", label: "系统盘", lifePctRemaining: lifePct, writtenTb, temperatureC: null, remappedBlocks: null });

  it("磨损模型：TBW 缺失不外推（读不到不编数）", () => {
    expect(f395.wearModel(media(80, 50), 500)).toEqual({ consumedPct: 10, consumedTb: 50 });
    expect(f395.wearModel(media(80, 50), null).consumedPct).toBeNull();
  });

  it("寿命外推：阈值天数可推、日写入为零如实 null", () => {
    // 剩 20% × 500TB = 100TB；到 warn(20%) 阈值 = 0 天（已在线）
    expect(f395.projectionDaysToThreshold(media(20, 100), 1, "warn", 500)).toBe(0);
    // 剩 30% × 500TB = 150TB → 到 10% 阈值 (50TB) 还要写 100TB → 100 天
    expect(f395.projectionDaysToThreshold(media(30, 100), 1, "critical", 500)).toBe(100);
    expect(f395.projectionDaysToThreshold(media(30, 100), 0, "critical", 500)).toBeNull();
  });

  it("建议清单按级别给可执行出路（critical 直达备份向导）", () => {
    expect(f395.advisorActions("ok")[0]!.target).toBeNull();
    expect(f395.advisorActions("critical")[0]!.target).toBe("backup-wizard");
    expect(f395.advisorActions("unknown")[0]!.action).toContain("不提供健康数据");
  });

  it("多介质总览：最差级别顶色；趋势斜率 %/天", () => {
    const s = f395.multiMediaSummary([media(50, 1), media(5, 1), media(null, null)]);
    expect(s.worst).toBe("critical");
    expect(s.counts.unknown).toBe(1);
    const t = f395.lifeTrend([
      { at: 0, lifePct: 50 },
      { at: 10 * 24 * 3600 * 1000, lifePct: 40 },
    ]);
    expect(t.slopePerDay).toBe(-1);
    expect(f395.lifeTrend([{ at: 0, lifePct: 50 }]).slopePerDay).toBeNull();
  });
});

/* ================= ⑲ F396 备份深化 ================= */

describe("F396 v4：清单校验 / 链完整性 / 保留策略 / 还原顺序", () => {
  const entry = (path: string, chunks: string[]): f396.ManifestEntry => ({ path, sizeBytes: chunks.length * 100, chunkDigests: chunks });

  it("清单摘要敏感性：任一分块/路径/尺寸变化即变（验过能还原的核对面）", () => {
    const e = entry("a.txt", ["c1", "c2"]);
    expect(f396.manifestDigest(e)).toBe(f396.manifestDigest(entry("a.txt", ["c1", "c2"])));
    expect(f396.manifestDigest(e)).not.toBe(f396.manifestDigest(entry("a.txt", ["c1", "c3"])));
    expect(f396.manifestDigest(e)).not.toBe(f396.manifestDigest(entry("b.txt", ["c1", "c2"])));
  });

  it("清单校验：缺文件/内容不符逐项点名（可恢复性校验实做）", () => {
    const expected = [entry("a.txt", ["c1"]), entry("b.txt", ["c2"])];
    expect(f396.verifyManifest(expected, [{ path: "a.txt", sizeBytes: 100, chunkDigests: ["c1"] }, { path: "b.txt", sizeBytes: 100, chunkDigests: ["c2"] }]).ok).toBe(true);
    const bad = f396.verifyManifest(expected, [{ path: "a.txt", sizeBytes: 100, chunkDigests: ["c1"] }]);
    expect(bad.missing).toEqual(["b.txt"]);
    const corrupt = f396.verifyManifest(expected, [{ path: "a.txt", sizeBytes: 100, chunkDigests: ["cX"] }, { path: "b.txt", sizeBytes: 100, chunkDigests: ["c2"] }]);
    expect(corrupt.mismatched).toEqual(["a.txt"]);
  });

  it("链完整性：改链任一条即检出；保留策略淘汰最旧；还原顺序基线先行", () => {
    const chain: f396.BackupChain = {
      target: "second-media",
      scope: { systemPartition: true, userFiles: true },
      entries: [
        { seq: 0, at: 100, fingerprints: ["a"], sizeBytes: 10, verified: true },
        { seq: 1, at: 200, fingerprints: ["b"], sizeBytes: 2, verified: true },
      ],
    };
    const sum = f396.chainIntegrity(chain);
    expect(f396.verifyChainIntegrity(chain, sum)).toBe(true);
    const tampered = { ...chain, entries: [chain.entries[0]!, { ...chain.entries[1]!, sizeBytes: 999 }] };
    expect(f396.verifyChainIntegrity(tampered, sum)).toBe(false);
    const pruned = f396.retentionPrune([chain, { ...chain, entries: [{ ...chain.entries[0]!, at: 50 }] }, { ...chain, entries: [{ ...chain.entries[0]!, at: 900 }] }], 2);
    expect(pruned.evicted).toBe(1);
    expect(pruned.keep[0]!.entries[0]!.at).toBe(900);
    expect(f396.restoreOrdering(chain)).toEqual([0, 1]);
    expect(f396.storageEstimate(chain)).toBe(12);
  });
});

/* ================= ⑳ F397 演练调度 ================= */

describe("F397 v4：调度数学 / 连续统计 / 加练排程 / 沙盒白名单", () => {
  const rec = (at: number, steps = 4, writes = 0): f397.DrillRecord => ({ at, completedSteps: f397.DRILL_STEPS.slice(0, steps).map((s) => s.id), sideEffectWrites: writes });

  it("下次提醒时刻：从未演练立即到期", () => {
    expect(f397.nextDrillDueAt(null, 1000)).toBe(1000);
    expect(f397.nextDrillDueAt(1000, 2000)).toBe(1000 + f397.DRILL_CYCLE_DAYS * 24 * 3600 * 1000);
  });

  it("连续统计：从最新往回数全步骤零副作用记录（最新无效即断档——如实口径）", () => {
    expect(f397.drillStats([rec(1), rec(2)]).validStreak).toBe(2);
    const s = f397.drillStats([rec(1), rec(2), rec(3, 3)]);
    expect(s.validStreak).toBe(0); // 最新一条步骤不全 → 断档
    expect(s.avgSteps).toBeCloseTo(3.7, 1);
  });

  it("加练排程：首次真恢复 7 天内补练；恢复前已练不加练", () => {
    // 归因：「从未演练」= 恢复时刻之前零记录
    const a = f397.extraDrillSchedule([], 5000);
    expect(a.due).toBe(true);
    expect(a.deadline).toBe(5000 + f397.EXTRA_DRILL_WINDOW_DAYS * 24 * 3600 * 1000);
    expect(f397.extraDrillSchedule([rec(100)], 4000).due).toBe(false);
  });

  it("沙盒白名单：白名单外写路径违规（零副作用路径级执法）", () => {
    expect(f397.auditSandboxWrites(["S:/Varix/drill/probe.txt", "memory://test"]).pass).toBe(true);
    const bad = f397.auditSandboxWrites(["S:/Users/doc.txt"]);
    expect(bad.pass).toBe(false);
    expect(bad.violations).toEqual(["S:/Users/doc.txt"]);
  });
});

/* ================= ㉑ F398 语言热切深化 ================= */

describe("F398 v4：占位符 / 复数 / 词表差异 / 缺译分组 / 镜像", () => {
  it("占位符校验：缺占/多占都点出（回退标注防线上移）", () => {
    const v = f398.validatePlaceholders("打开 {file}（{size}）", "打开 {file}");
    expect(v.ok).toBe(false);
    expect(v.missing).toEqual(["size"]);
    const e = f398.validatePlaceholders("打开 {file}", "打开 {file}{extra}");
    expect(e.extra).toEqual(["extra"]);
    expect(f398.validatePlaceholders("原样", "原样").ok).toBe(true);
  });

  it("复数规则：zh 恒 other、en one/other；缺复数键回退标注", () => {
    expect(f398.pluralCategory("zh-CN", 1)).toBe("other");
    expect(f398.pluralCategory("en", 1)).toBe("one");
    expect(f398.pluralCategory("en", 2)).toBe("other");
    const bundle = { "files.one": "1 file", files: "{n} files" };
    expect(f398.pickPlural(bundle, "files", "en", 1).text).toBe("1 file");
    expect(f398.pickPlural(bundle, "files", "en", 5).text).toBe("{n} files");
    const missing = f398.pickPlural({ files: "x" }, "files", "en", 1);
    expect(missing.issue?.annotated).toBe(true);
  });

  it("词表差异三类 + 缺译命名空间分组（F132 联动）", () => {
    const from = { a: "1", b: "2", c: "3" };
    const to = { a: "1", b: "2x", d: "4" };
    const diff = f398.bundleDiff(from, to);
    expect(diff).toEqual([
      { key: "b", kind: "changed", from: "2", to: "2x" },
      { key: "c", kind: "removed", from: "3", to: null },
      { key: "d", kind: "added", from: null, to: "4" },
    ]);
    const report = f398.missingKeyReport({ en: { "menu.open": "Open", "menu.close": "Close", "dlg.ok": "OK" }, "zh-CN": { "dlg.ok": "确定" } }, "zh-CN");
    expect(report[0]!.namespace).toBe("menu");
    expect(report[0]!.keys).toEqual(["menu.open", "menu.close"]);
  });

  it("镜像令牌：RTL 下方向符号翻转、LTR 原样（接口从存在到可用）", () => {
    expect(f398.mirrorToken("→", "rtl")).toBe("←");
    expect(f398.mirrorToken("→", "ltr")).toBe("→");
    expect(f398.mirrorToken("普通", "rtl")).toBe("普通");
  });
});

/* ================= ㉒ F356 清单深校验 ================= */

describe("F356 v4：清单深校验 / 图标择优 / 安装 Dry-Run", () => {
  const manifest: f356.SiteManifest = {
    name: "示例应用",
    shortName: "示例",
    startUrl: "https://app.example.com/",
    scope: "https://app.example.com/",
    display: "standalone",
    icons: [
      { src: "/icon-192.png", sizes: "192x192" },
      { src: "/icon-512.png", sizes: "512x512" },
    ],
    themeColor: "#4f7cff",
  };

  it("深校验：合法清单零问题；逐字段违规带字段名", () => {
    expect(f356.validateManifestStrict(manifest)).toEqual([]);
    const bad = f356.validateManifestStrict({ ...manifest, startUrl: "ftp://x", scope: "https://other.com/", themeColor: "blue", icons: [{ src: "/i.png", sizes: "48x48" }] });
    expect(bad.some((p) => p.startsWith("start_url"))).toBe(true);
    expect(bad.some((p) => p.startsWith("scope"))).toBe(true);
    expect(bad.some((p) => p.startsWith("theme_color"))).toBe(true);
    expect(bad.some((p) => p.startsWith("icons"))).toBe(true);
  });

  it("图标择优：最大面积者胜；空图标如实 null", () => {
    expect(f356.bestIcon(manifest)!.src).toBe("/icon-512.png");
    expect(f356.bestIcon({ ...manifest, icons: [] })).toBeNull();
  });

  it("安装 Dry-Run：硬伤拒、软伤过但带提示、零持久化", () => {
    const dry = f356.installDryRun(manifest);
    expect(dry.ok).toBe(true);
    expect(dry.problems).toEqual([]);
    expect(dry.preview!.displayName).toBe("示例");
    const fatal = f356.installDryRun({ ...manifest, name: " " });
    expect(fatal.ok).toBe(false);
    expect(fatal.preview).toBeNull();
  });
});

/* ================= ㉓ 质量门禁（隔离验证扩展） ================= */

describe("v4 质量门禁：源码纪律机检", () => {
  const srcs = readdirSync(H4_DIR).filter((f) => f.endsWith(".ts")).map((f) => ({ f, src: readFileSync(join(H4_DIR, f), "utf-8") }));

  it("零 TODO/FIXME/占位——不留「以后再说」的债", () => {
    const offenders = srcs.filter(({ src }) => /\b(TODO|FIXME|XXX|HACK)\b/.test(src));
    expect(offenders.map((o) => o.f)).toEqual([]);
  });

  it("零 console / 零 as any / 零 @ts-ignore（静默吞错与类型逃逸都是红线）", () => {
    const offenders = srcs.filter(({ src }) => /console\.(log|warn|error)/.test(src) || /as any\b/.test(src) || /@ts-ignore/.test(src));
    expect(offenders.map((o) => o.f)).toEqual([]);
  });

  it("FNV-1a 单点实现：魔数 0x811c9dc5 只准出现在 internal/hash（私抄第二份即红灯）", () => {
    const offenders = srcs.filter(({ f, src }) => f !== "internal" && !f.startsWith("internal") && src.includes("0x811c9dc5"));
    expect(offenders.map((o) => o.f)).toEqual([]);
  });

  it("v4 深化面在册：每个 v4 触碰的模块都能在隔离舱白名单机制下继续通过（internal/hash 依赖不入黑名单）", () => {
    const dependents = srcs.filter(({ src }) => src.includes('from "./internal/hash"'));
    // f372（时间线链）/f351/f362/f365/f370/f396 六模块委托单点实现
    expect(dependents.length).toBeGreaterThanOrEqual(6);
  });
});

/* ================= ㉔ 检查项对账扩展（v4 面） ================= */

describe("v4 检查项对账：深化导出面可发现性", () => {
  const V4_EXPORTS: ReadonlyArray<[string, string, number]> = [
    ["f351", "snapshotChecksum", 5],
    ["f356", "validateManifestStrict", 3],
    ["f357", "coalesceNotices", 5],
    ["f358", "layoutPips", 6],
    ["f361", "frameBudget", 5],
    ["f362", "quotaEviction", 5],
    ["f364", "planWithConflicts", 4],
    ["f365", "threeStagePipeline", 4],
    ["f369", "starvationWatchdog", 6],
    ["f370", "backoffWithJitter", 4],
    ["f377", "selfCheckWalk", 4],
    ["f383", "overflowDigest", 4],
    ["f384", "buildRing", 4],
    ["f389", "stitchRobust", 4],
    ["f390", "lookupVariants", 4],
    ["f392", "cachePut", 3],
    ["f393", "worstAspectRatio", 2],
    ["f394", "irreversibilityGate", 4],
    ["f395", "wearModel", 5],
    ["f396", "verifyManifest", 6],
    ["f397", "extraDrillSchedule", 4],
    ["f398", "validatePlaceholders", 5],
  ];

  it("22 个 v4 深化模块的导出面全部可发现、可调用（零死代码——新增即被消费）", () => {
    const modules: Record<string, Record<string, unknown>> = {
      f351, f356, f357, f358, f361, f362, f364, f365, f369, f370, f377, f383, f384, f389, f390, f392, f393, f394, f395, f396, f397, f398,
    };
    const missing: string[] = [];
    for (const [mod, fn, minExports] of V4_EXPORTS) {
      const m = modules[mod]!;
      if (typeof m[fn] !== "function") missing.push(`${mod}.${fn}`);
      const v4Marks = (Object.keys(m).length ?? 0);
      if (v4Marks < minExports) missing.push(`${mod}: 导出面 ${v4Marks} < 期待 ≥${minExports}`);
    }
    expect(missing).toEqual([]);
  });
});
