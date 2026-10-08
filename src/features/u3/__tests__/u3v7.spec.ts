/**
 * U3-v7 引擎群测试（AI-U3 · 批次七）。
 *
 * 四引擎（deskmenu/copyqueue/clockpanel/walkrehearse）+ 自检聚合 +
 * 破坏注入 + 端到端装配链。测试是判据的执行器（不计功能行数）。
 */

import { describe, expect, it } from "vitest";
import {
  // deskmenu
  fullTextReachable, splitNameExt, initialSelection, firstIllegalChar,
  renameCommit, renameIdle, renameEnter, renameSubmit, renameEscape,
  propsGeneral, applyAttribute, folderStatsStepper, iconContextMenu, desktopContextMenu,
  ILLEGAL_NAME_CHARS, CLUSTER_BYTES, type ItemFacts,
  // copyqueue
  newJob, jobRow, jobProgress, jobPause, jobResume, jobCancel, jobDone, jobStateText,
  undoBannerAssemble, undoBannerTick, undoBannerExtend, undoBannerRestore,
  preflightGate, SPEED_SAMPLE_WINDOW,
  // clockpanel
  buildMonthGrid, panelInit, panelToggle, panelShiftMonth, panelSelect, panelTitle,
  holidayOf, holidayCoverage, HOLIDAYS_2026, HOLIDAY_TABLE,
  GRID_ROWS, GRID_COLS,
  // walkrehearse
  rehearsalScript, pendingLedger, recordWalk, blockedWorkorder, fiveCheck,
  MANUAL_WALK_QUERIES, DPI_SCALE_TIERS, FIVE_CHECK_NAMES,
  // 聚合自检 + 词典账本（v7 扩容回归）
  deskmenuSelfCheck, copyqueueSelfCheck, clockpanelSelfCheck, walkrehearseSelfCheck,
  v7EnginesSelfCheck, V7_ENGINE_SELFCHECKS, u3EnginesSelfCheck,
  U3_DICT_ENTRIES, walkDictEntries, dictWalkVerdict,
} from "../engines";
import { anchorRuntime } from "../anchor";

/* ------------------------------ deskmenu（F502/F260/F264） ------------------------------ */

describe("U3-v7 deskmenu", () => {
  const full = "年度总结报告-特别长的名字版本.docx";

  it("F502 三路全文可达（破坏注入：缺一路显性红）", () => {
    const ok = fullTextReachable(full, { tooltip: () => full, rename: () => full, properties: () => full });
    expect(ok).toEqual({ ok: true, missing: [] });
    const bad = fullTextReachable(full, { tooltip: () => full, rename: () => full.slice(0, 5) });
    expect(bad.ok).toBe(false);
    expect(bad.missing).toEqual(["rename", "properties"]);
  });

  it("F260 扩展名隔离与选区", () => {
    expect(splitNameExt("报告.docx")).toEqual({ base: "报告", ext: ".docx" });
    expect(splitNameExt(".hidden")).toEqual({ base: ".hidden", ext: "" });
    expect(splitNameExt("无扩展名")).toEqual({ base: "无扩展名", ext: "" });
    expect(splitNameExt("a.b.c")).toEqual({ base: "a.b", ext: ".c" });
    const sel = initialSelection("总结 v2.final.docx");
    expect(sel).toEqual({ start: 0, end: "总结 v2.final".length });
  });

  it("F260 非法字符 9 个全测——即时抖动拒绝不等到提交", () => {
    expect(ILLEGAL_NAME_CHARS).toHaveLength(9);
    for (const c of ILLEGAL_NAME_CHARS) {
      const v = renameCommit(`x${c}y`, [], true);
      expect(v.ok).toBe(false);
      expect(v.shake).toBe(true);
      expect(v.inlineError).toContain(c);
    }
    expect(firstIllegalChar("干净名字")).toBeNull();
  });

  it("F260 失败行内红字不弹窗：重名/权限/空名", () => {
    const dup = renameCommit("Report.TXT", ["report.txt"], true);
    expect(dup.failKind).toBe("duplicate");
    expect(dup.shake).toBe(false);
    const perm = renameCommit("x", [], false);
    expect(perm.failKind).toBe("permission");
    const empty = renameCommit("   ", [], true);
    expect(empty.failKind).toBe("empty");
    expect(renameCommit("正常名字", [], true).ok).toBe(true);
  });

  it("F260 状态机：三入口进编辑 → Esc 还原 → Enter 提交/失败留态", () => {
    let rt = renameIdle();
    expect(rt.phase).toBe("idle");
    for (const via of ["f2", "name-click", "slow-dbl"] as const) {
      rt = renameEnter(renameIdle(), "目标.txt", via);
      expect(rt.via).toBe(via);
      expect(rt.phase).toBe("editing");
    }
    const esc = renameEscape(rt);
    expect(esc.phase).toBe("idle");
    expect(esc.target).toBe("目标.txt");
    rt = renameEnter(renameIdle(), "旧名.txt", "f2");
    rt.draft = "新名.txt";
    expect(renameSubmit(rt, [], true).verdict.ok).toBe(true);
    const bad = renameSubmit({ ...rt, draft: "坏:名.txt" }, [], true);
    expect(bad.verdict.ok).toBe(false);
    expect(bad.rt.phase).toBe("editing");
  });

  it("F264 常规页字段 + 占用按簇取整 + 受保护拒绝取消只读", () => {
    const facts: ItemFacts = {
      name: "data.bin", kind: "symlink", openWith: "终端", location: "C:\\links",
      sizeBytes: 100, createdAt: 1, modifiedAt: 2, accessedAt: 3,
      readOnly: false, hidden: true, protectedReason: null, contains: null,
    };
    const g = propsGeneral(facts);
    expect(g.type).toBe("符号链接");
    expect(g.sizeOnDisk).toContain(`${CLUSTER_BYTES}`);
    expect(g.hidden).toBe(true);
    const flipped = applyAttribute(facts, "hidden", false);
    expect(flipped.hidden).toBe(false);
    const guarded: ItemFacts = { ...facts, readOnly: true, protectedReason: "策略锁定" };
    expect(applyAttribute(guarded, "readOnly", false).readOnly).toBe(true);
  });

  it("F264 文件夹统计异步步进（进度单调、结果只在末次）", () => {
    const step = folderStatsStepper([{ sizeBytes: 5 }, { sizeBytes: 6 }, { sizeBytes: 7 }, { sizeBytes: 8 }], 3);
    const p1 = step();
    expect(p1.finished).toBe(false);
    expect(p1.stats).toBeNull();
    const p2 = step();
    expect(p2.finished).toBe(true);
    expect(p2.stats).toEqual({ items: 4, sizeBytes: 26 });
    expect(p2.progress).toBe(1);
  });

  it("F264 菜单装配：四项+子菜单+受保护禁用+空白菜单", () => {
    const facts: ItemFacts = {
      name: "a.txt", kind: "file", openWith: "编辑器", location: "D:\\",
      sizeBytes: 10, createdAt: 0, modifiedAt: 0, accessedAt: 0,
      readOnly: false, hidden: false, protectedReason: null, contains: null,
    };
    const menu = iconContextMenu(facts, ["编辑器", "浏览器", "查看器"]);
    expect(menu.map((m) => m.id)).toEqual(["open", "open-with", "-", "rename", "properties"]);
    const ow = menu.find((m) => m.id === "open-with")!;
    expect(ow.submenu!.map((s) => s.label)).toEqual(["编辑器", "浏览器", "查看器"]);
    const locked = iconContextMenu({ ...facts, protectedReason: "受保护" }, []);
    expect(locked.find((m) => m.id === "rename")!.disabled).toBe(true);
    expect(locked.find((m) => m.id === "open-with")!.disabled).toBe(true);
    expect(desktopContextMenu(true).some((m) => m.id === "refresh")).toBe(true);
  });
});

/* ------------------------------ copyqueue（F531/F524/F529/F530） ------------------------------ */

describe("U3-v7 copyqueue", () => {
  it("F531 四信息：样本不足显性「计算中」、样本足够出速度与剩余", () => {
    let j = newJob("j1", "D:", "src", "dst", 10_000);
    let row = jobRow(j);
    expect(row.speedBps).toBeNull();
    expect(row.etaSec).toBeNull();
    expect(row.state).toBe("queued");
    j = jobProgress(j, 0, 1_000);
    j = jobProgress(j, 2_000, 5_000);
    row = jobRow(j);
    expect(row.speedBps).toBe(2_000);
    expect(row.etaSec).toBeCloseTo(2.5, 9);
    expect(row.progressPct).toBeCloseTo(50, 9);
    expect(row.label).toBe("src → dst");
  });

  it("F531 样本滑动窗口与生命周期五态", () => {
    let j = newJob("j2", "C:", "a", "b", 1e9);
    for (let t = 0; t < SPEED_SAMPLE_WINDOW + 10; t++) j = jobProgress(j, t * 100, t * 1000);
    expect(j.samples.length).toBe(SPEED_SAMPLE_WINDOW);
    const paused = jobPause({ ...j, state: "running" });
    expect(paused.state).toBe("paused");
    expect(jobResume(paused).state).toBe("queued");
    expect(jobCancel(jobDone(newJob("x", "C:", "a", "b", 1))).state).toBe("done");
    expect(jobStateText("running")).toBe("复制中");
  });

  it("F524 后悔窗通知条全生命周期", () => {
    const open = undoBannerAssemble(["1", "2"], 1000);
    expect(open.banner.text).toContain("2 项");
    expect(open.banner.remainingMs).toBe(5000);
    const mid = undoBannerTick(open.rt, 3500);
    expect(mid.banner.remainingMs).toBe(2500);
    const ex = undoBannerExtend(mid.rt);
    expect(ex.banner.text).toContain("10 秒");
    const rst = undoBannerRestore(ex.rt);
    expect(rst.restored).toEqual(["1", "2"]);
    expect(rst.banner.visible).toBe(false);
    const timeout = undoBannerTick(undoBannerAssemble(["x"], 0).rt, 6000);
    expect(timeout.banner.visible).toBe(false);
    expect(undoBannerRestore(timeout.rt).restored).toBeNull();
  });

  it("F529 预检闸三选出路（破坏注入：不足必拦）", () => {
    const short = { totalBytes: 0, perTargetFree: { "E:": 1.5 * 1024 ** 3 }, perTargetNeed: { "E:": 3 * 1024 ** 3 } };
    expect(preflightGate(short, "cancel").stage).toBe("blocked");
    expect(preflightGate(short, "change-target").stage).toBe("blocked");
    const force = preflightGate(short, "proceed");
    expect(force.stage).toBe("passed");
    expect(force.message).toContain("仍要复制");
    expect(preflightGate(null, "cancel").stage).toBe("passed");
  });
});

/* ------------------------------ clockpanel（F549/F560） ------------------------------ */

describe("U3-v7 clockpanel", () => {
  const today: readonly [number, number, number] = [2026, 9, 27];

  it("F549 网格结构：42 格满格、周一起始、前后月补位", () => {
    const grid = buildMonthGrid(2026, 9, today);
    expect(grid.length).toBe(GRID_ROWS * GRID_COLS);
    expect(grid.filter((c) => c.inMonth)).toHaveLength(30);
    expect(grid[0]!.inMonth).toBe(false);
    expect(grid[0]!.m).toBe(8);
    const lastCell = grid[grid.length - 1]!;
    expect(lastCell.inMonth).toBe(false);
    expect(lastCell.m).toBe(10);
  });

  it("F549 农历逐日：初一显月名、中秋锚十五、范围外诚实 null", () => {
    const grid = buildMonthGrid(2026, 9, today);
    expect(grid.find((c) => c.m === 9 && c.d === 11)!.lunarText).toBe("八月");
    expect(grid.find((c) => c.m === 9 && c.d === 25)!.lunarText).toBe("十五");
    const far = buildMonthGrid(2033, 5, today);
    expect(far.some((c) => c.lunarText === null)).toBe(true);
  });

  it("F560 节假日：红标在册、网格挂接、两源互证、覆盖诚实账", () => {
    expect(HOLIDAYS_2026).toHaveLength(7);
    expect(HOLIDAY_TABLE).toHaveLength(8);
    expect(holidayOf(2026, 9, 25)!.name).toBe("中秋");
    expect(holidayOf(2026, 3, 15)).toBeNull();
    const grid = buildMonthGrid(2026, 10, today);
    expect(grid.find((c) => c.m === 10 && c.d === 1)!.holiday!.name).toBe("国庆");
    const cov = holidayCoverage(2027);
    expect(cov.covered).toBe(1);
    expect(cov.pendingNote).toContain("F122");
  });

  it("F549 两层分工状态机：点开归今天、翻页跨年、点灰显格翻月、再点关闭", () => {
    let ps = panelInit(today);
    expect(ps.open).toBe(false);
    ps = panelToggle(ps, today);
    expect(ps.open).toBe(true);
    expect(ps.selected).toEqual([2026, 9, 27]);
    ps = panelShiftMonth(ps, -9);
    expect(ps.viewYear).toBe(2025);
    expect(ps.viewMonth).toBe(12);
    ps = panelShiftMonth(ps, 2);
    expect(ps.viewMonth).toBe(2);
    const grid = buildMonthGrid(ps.viewYear, ps.viewMonth, today);
    ps = panelSelect(ps, grid[0]!);
    expect(ps.viewMonth).toBe(1);
    expect(panelTitle(ps)).toContain("2026 年 1 月");
    ps = panelToggle(ps, today);
    expect(ps.open).toBe(false);
  });
});

/* ------------------------------ walkrehearse（十二查 manual-walk 预演） ------------------------------ */

describe("U3-v7 walkrehearse", () => {
  it("八条 manual-walk 查项脚本全在册且步骤非空", () => {
    expect(MANUAL_WALK_QUERIES).toEqual([4, 5, 6, 7, 8, 10, 11, 12]);
    for (const q of MANUAL_WALK_QUERIES) {
      const script = rehearsalScript(q, "sysdev");
      expect(script.length).toBeGreaterThan(0);
      expect(script.every((s) => s.action.length > 0 && s.expect.length > 0)).toBe(true);
    }
    expect(DPI_SCALE_TIERS).toEqual([100, 125, 150, 200]);
  });

  it("登记表：pending 显性、4K 按档拆槽、执行入账与防洗账", () => {
    const ledger = pendingLedger(["a", "b"]);
    expect(ledger).toHaveLength(2 * (7 + 4));
    expect(ledger.every((r) => r.result === "pending" && r.executedAt === null)).toBe(true);
    const r1 = recordWalk(ledger, 4, "a", 125, "fail", "125% 档焦点环看不清");
    expect(r1.changed).toBe(true);
    const rec = r1.ledger.find((r) => r.queryNo === 4 && r.domain === "a" && r.dpiPct === 125)!;
    expect(rec.result).toBe("fail");
    expect(rec.executedAt).not.toBeNull();
    const r2 = recordWalk(r1.ledger, 4, "a", 125, "pass", "改判尝试");
    expect(r2.changed).toBe(false);
  });

  it("工单导出三件套 + 收工五勾机检", () => {
    const wo = blockedWorkorder(pendingLedger(["x"]));
    expect(wo.total).toBe(11);
    expect(wo.items.every((b) => b.need.length > 0 && b.who.length > 0)).toBe(true);
    expect(wo.text).toContain("谁能解");
    const all = { categoryPage: true, searchReachable: true, inlineAdjustable: true, descSentences: true, pathChainRegistered: true };
    expect(fiveCheck(all)).toEqual({ pass: true, missing: [] });
    const half = { ...all, categoryPage: false, descSentences: false };
    expect(fiveCheck(half).missing).toEqual(["分类页", "说明句"]);
    expect(FIVE_CHECK_NAMES).toHaveLength(5);
  });
});

/* ------------------------------ 聚合与回归 ------------------------------ */

describe("U3-v7 聚合自检与全链回归", () => {
  it("四引擎自检全绿（红项点名断言）", () => {
    for (const engine of [deskmenuSelfCheck, copyqueueSelfCheck, clockpanelSelfCheck, walkrehearseSelfCheck]) {
      const red = engine().filter((c) => !c.pass);
      expect(red).toEqual([]);
    }
  });

  it("v7 注册表与 v7 总自检全绿", () => {
    expect(V7_ENGINE_SELFCHECKS).toHaveLength(4);
    const run = v7EnginesSelfCheck();
    expect(run.filter((c) => !c.pass)).toEqual([]);
    expect(run.every((c) => c.name.startsWith("["))).toBe(true);
  });

  it("全引擎群（v4+v5+v6+v7）总自检全绿", () => {
    expect(u3EnginesSelfCheck().filter((c) => !c.pass)).toEqual([]);
  });

  it("词典账本 v7 扩容回归：13 条全绿（四个新浮层四出路齐全）", () => {
    expect(U3_DICT_ENTRIES).toHaveLength(13);
    const verdicts = dictWalkVerdict(walkDictEntries(), U3_DICT_ENTRIES);
    expect(verdicts.filter((v) => !v.pass)).toEqual([]);
    const newIds = ["u3-desk-rename", "u3-props-dialog", "u3-clock-panel", "u3-copy-queue"];
    expect(newIds.every((id) => U3_DICT_ENTRIES.some((e) => e.id === id))).toBe(true);
  });

  it("F550 锚点域表十三域且 anchorRuntime 全绿（v7-engines 并入）", () => {
    const rt = anchorRuntime();
    expect(rt.allGreen).toBe(true);
    expect(rt.failed).toBe(0);
  });
});
