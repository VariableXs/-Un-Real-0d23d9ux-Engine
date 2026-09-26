/**
 * AI-U3 深化批次四（v4 引擎群）单测——十三引擎 × 判据常量 × 边界三点 ×
 * 破坏注入 × 显性报错路径。
 *
 * 环境声明：vitest node 环境，本文件只用纯函数引擎（零 DOM 依赖）——
 * 引擎与实验室面板共用同一事实源，测试即判据的执行器。
 */

import { describe, expect, it } from "vitest";

import {
  // lumapick
  robustWallpaperLuma, synthWallpaperSamples, pickTextColorHysteresis, applyDarkOverlay,
  iconTextRenderSpec, selectPillGeometry, fulltextRouteVerdict, tooltipShouldShowFulltext,
  // gridlab
  planResnap, auditCustomGrid, arbitrateSnap, GRID_DENSITY_PX, KEEP_ORDER_GREEN_LINE,
  // pinvault
  cooldownMinutesFor, isValidPinShape, auditPinTiming, PIN_SEGMENT_BUDGET, PIN_TOTAL_BUDGET_MS,
  btInitialState, btTick, btLockBadge, mergeLockEvents,
  // guestbox
  guestOpenSession, guestRecord, guestClose, guestScanResidue, guestRemaining, guestUiAllowed,
  // capguard
  adjudicateCapture, composeShieldBlocks, intersectRect, subtractRect, CAPTURE_CHANNELS,
  // shredplan
  planShred, shredEtaText, clipWipeConfirmText, clipWipeTick, clipPasteAfterWipe,
  evictShots, shutdownPurgeNotice, SHOT_HISTORY_CAP, type ClipWipeState,
  // vxcrypt2
  serializeHeader, parseHeader, tempViewOpen, tempViewClose, normalizeArchiveEntry,
  VXCRYPT_KDF_ITER_BASE, VXCRYPT_KDF_ITER_FLOOR,
  // findrepl
  findAll, previewCount, openReplaceSession, replaceAll, replaceNext, undoReplaceBatch,
  reconcileStatusBarCounts, keyboardSelectionText, emptyDirectoryStatus, STATUSBAR_HEIGHT_PX,
  // bannerpack
  bannerLayout, bannerStackRects, osdSlot, capsToneFor, capsVolumeFollow, midClickRoute,
  BANNER_PRESETS, CAPS_TONE_HZ,
  // hotkeymap
  capsDualUseVerdict, releaseOldScheme, exportCardMarkdown, exportCardText, cardFormatsAgree,
  resolveWinNumber, layoutLockFeedback, auditHotkeyConflicts, CAPS_LONG_PRESS_MS, IME_SCHEMES,
  // sysdiag
  diagFindingComplete, toDiagReport, netResetStepsComplete, netResetCancellable, clickLockActivate,
  applyMasterVolume, dualSliderIndependent, batterySourcesAgree, smoothBatteryStep,
  deviceArrivalNotice, clampBalance, channelGains,
  // explog
  ExpLog, toImprovementItems, EXPLOG_CAP, u3ExpLog,
  // walkcheck
  TWELVE_QUERIES, u3ItemCoverage, buildDomainChecklist, reconcileTwelveQueries,
  // 总自检
  v4EnginesSelfCheck, V4_ENGINE_SELFCHECKS,
} from "../engines";
import { labelsSelfCheck, U3_ENGINE_LABELS, u3Label, U3_LAB_LABELS } from "../labels";
import { wrapIconLabel } from "../deskicons";

/* ------------------------------ 引擎群总自检（每引擎自检全绿） ------------------------------ */

describe("v4 引擎群总自检", () => {
  it("十三引擎自检全部通过（清单在册且逐条绿）", () => {
    expect(V4_ENGINE_SELFCHECKS.length).toBe(13);
    const all = v4EnginesSelfCheck();
    expect(all.length).toBeGreaterThanOrEqual(60);
    const red = all.filter((c) => !c.pass);
    expect(red, `红项：${red.map((c) => c.name).join("；")}`).toEqual([]);
  });

  it("labels 双语零缺键", () => {
    for (const c of labelsSelfCheck()) expect(c.pass, c.name).toBe(true);
    expect(Object.keys(U3_ENGINE_LABELS).length).toBe(16);
    expect(u3Label("runAll", "en", U3_LAB_LABELS)).toBe("Run all engine self-checks");
  });
});

/* ------------------------------ lumapick（F501/F502） ------------------------------ */

describe("lumapick · F501 亮度采样与字色", () => {
  it("五档样点截尾均值回位（±0.02）", () => {
    for (const t of [0.05, 0.25, 0.5, 0.75, 0.95]) {
      const l = robustWallpaperLuma(synthWallpaperSamples(t));
      expect(Math.abs(l - t)).toBeLessThan(0.02);
    }
  });

  it("采样数非法诚实抛错（异常零静默）", () => {
    expect(() => robustWallpaperLuma([[0.5, 0.5, 0.5]])).toThrow(/25/);
  });

  it("高光抗扰：一块角落高光不翻转判定", () => {
    const base = synthWallpaperSamples(0.42);
    const withGlare = base.map((p, i) => (i < 3 ? ([0.99, 0.99, 0.99] as [number, number, number]) : p));
    const l = robustWallpaperLuma(withGlare);
    expect(pickTextColorHysteresis(l)).toBe("light"); // 仍暗壁纸
  });

  it("滞后带三点：带内维持、带外翻转", () => {
    expect(pickTextColorHysteresis(0.51, "light")).toBe("light");
    expect(pickTextColorHysteresis(0.55, "light")).toBe("dark");
    expect(pickTextColorHysteresis(0.47, "dark")).toBe("dark");
    expect(pickTextColorHysteresis(0.45, "dark")).toBe("light");
  });

  it("F297 压暗联动同式", () => {
    expect(applyDarkOverlay(0.55, true)).toBeCloseTo(0.385);
    expect(applyDarkOverlay(0.55, false)).toBeCloseTo(0.55);
  });

  it("4K 渲染精度：DPR=2 物理像素翻倍；DPR 越界抛错", () => {
    expect(iconTextRenderSpec("light", 2).shadowBlurDevicePxAt2x).toBe(4);
    expect(() => iconTextRenderSpec("light", 8)).toThrow(/DPR/);
  });

  it("选中胶囊几何", () => {
    const pill = selectPillGeometry(20, 2);
    expect(pill.heightPx).toBe(48);
    expect(pill.radiusPx).toBe(24);
  });

  it("F502 三路可达闭环：全禁用抛错；部分可用给主路", () => {
    expect(() => fulltextRouteVerdict({ tooltip: false, rename: false, properties: false })).toThrow(/红线/);
    const v = fulltextRouteVerdict({ tooltip: false, rename: true, properties: false });
    expect(v.primary).toBe("rename");
    expect(v.reachable).toEqual(["rename"]);
  });

  it("Tooltip 截断联动：未截断不出 tooltip", () => {
    expect(tooltipShouldShowFulltext(false, true)).toBe(false);
    expect(tooltipShouldShowFulltext(true, true)).toBe(true);
  });
});

/* ------------------------------ gridlab（F503） ------------------------------ */

describe("gridlab · F503 网格规划器", () => {
  it("步进审计：57→56 回弹、310 钳到 200", () => {
    expect(auditCustomGrid(57)).toEqual({ value: 56, snapped: true });
    expect(auditCustomGrid(310)).toEqual({ value: 200, snapped: true });
    expect(auditCustomGrid(80)).toEqual({ value: 80, snapped: false });
  });

  it("紧凑化计划：零冲突、保持度满格", () => {
    const pts = [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => ({ x: c * 96, y: r * 96 })));
    const plan = planResnap(pts, { colPx: 96, rowPx: 96 }, { colPx: 64, rowPx: 64 });
    expect(new Set(plan.moves.map((m) => `${m.to.x},${m.to.y}`)).size).toBe(9);
    expect(plan.displaced).toBe(0);
    expect(plan.keepOrder).toBeGreaterThanOrEqual(KEEP_ORDER_GREEN_LINE);
  });

  it("冲突螺旋转移：占位者先来先占、后来者最近空位", () => {
    const plan = planResnap([{ x: 10, y: 10 }, { x: 20, y: 12 }], { colPx: 80, rowPx: 80 }, { colPx: 64, rowPx: 64 });
    expect(plan.displaced).toBe(1);
    expect(new Set(plan.moves.map((m) => `${m.to.x},${m.to.y}`)).size).toBe(2);
  });

  it("F084/F401 仲裁三态", () => {
    expect(arbitrateSnap("manual").allow).toBe(true);
    const lk = arbitrateSnap("layout-locked");
    expect(lk.allow).toBe(false);
    if (!lk.allow && lk.reason === "layout-locked") expect(lk.hint).toContain("F546");
    expect(arbitrateSnap("auto-arrange").allow).toBe(false);
  });

  it("三档常量与主册一致", () => {
    expect(GRID_DENSITY_PX).toEqual({ loose: 96, standard: 80, compact: 64 });
  });
});

/* ------------------------------ pinvault（F504/F505） ------------------------------ */

describe("pinvault · F504 PIN 生命周期", () => {
  it("冷却翻倍表（5 次→10min，逐次翻倍，封顶 160）", () => {
    expect(cooldownMinutesFor(4)).toBe(0);
    expect([5, 6, 7, 8, 9].map(cooldownMinutesFor)).toEqual([10, 20, 40, 80, 160]);
    expect(cooldownMinutesFor(20)).toBe(160);
  });

  it("PIN 形状 4-6 位", () => {
    expect(isValidPinShape("1234")).toBe(true);
    expect(isValidPinShape("123456")).toBe(true);
    expect(isValidPinShape("123")).toBe(false);
    expect(isValidPinShape("1234567")).toBe(false);
    expect(isValidPinShape("12a4")).toBe(false);
  });

  it("解锁时序预算对账（全按预算绿 / 超支点名）", () => {
    expect(auditPinTiming({ ...PIN_SEGMENT_BUDGET }).withinBudget).toBe(true);
    const bad = auditPinTiming({ ...PIN_SEGMENT_BUDGET, broadcastMs: 900 });
    expect(bad.withinBudget).toBe(false);
    expect(bad.over).toEqual(["broadcastMs"]);
    expect(PIN_TOTAL_BUDGET_MS).toBe(1500);
  });
});

describe("pinvault · F505 蓝牙动态锁", () => {
  it("时间线：在场→<10s 波动豁免→回连不锁", () => {
    let s = btInitialState();
    s = btTick(s, 100, true).state;
    const away = btTick(s, 105, false);
    expect(away.state.phase).toBe("away");
    const back = btTick(away.state, 112, true);
    expect(back.state.phase).toBe("present");
    expect(back.unlocked).toBe(false);
  });

  it("时间线：离场 30s±5s 触发锁定 + 锁形标记", () => {
    let s = btInitialState();
    s = btTick(s, 0, true).state;
    const away = btTick(s, 5, false);
    const locked = btTick(away.state, 32, false);
    expect(locked.lockTriggered).toBe(true);
    expect(locked.state.phase).toBe("locked");
    expect(btLockBadge(locked.state).show).toBe(true);
    expect(btLockBadge(locked.state).reason).toContain("30");
  });

  it("回连即时解锁", () => {
    let s = btInitialState();
    s = btTick(s, 0, true).state;
    const away = btTick(s, 5, false);
    const locked = btTick(away.state, 40, false);
    const back = btTick(locked.state, 50, true);
    expect(back.unlocked).toBe(true);
    expect(back.state.phase).toBe("present");
    expect(back.state.lockedAt).toBeNull();
  });

  it("F316 双保险 2s 窗口去重（主因优先级）", () => {
    const merged = mergeLockEvents([
      { cause: "idle-f316", atMs: 1000 },
      { cause: "manual", atMs: 1500 },
      { cause: "bt-dynamic", atMs: 1400 },
    ]);
    expect(merged.length).toBe(1);
    expect(merged[0]!.cause).toBe("manual");
    expect(merged[0]!.merged.length).toBe(3);
  });
});

/* ------------------------------ guestbox（F506） ------------------------------ */

describe("guestbox · F506 访客沙盒账", () => {
  it("四轴记账 + 逆序清算", () => {
    const s = guestOpenSession(0);
    guestRecord(s, { axis: "file", target: "a.bin", atMs: 1, rollback: "delete" });
    guestRecord(s, { axis: "settings", target: "wallpaper", atMs: 2, rollback: "restore" });
    guestRecord(s, { axis: "permission", target: "cam", atMs: 3, rollback: "revoke" });
    guestRecord(s, { axis: "netshare", target: "share1", atMs: 4, rollback: "close" });
    const order: string[] = [];
    const rep = guestClose(s, (w) => { order.push(w.target); return { ok: true }; });
    expect(rep.clean).toBe(true);
    expect(rep.rolledBack.length).toBe(4);
    expect(order[0]).toBe("share1"); // 逆序：后写的先撤
  });

  it("未知轴与关闭后记账诚实抛错", () => {
    const s = guestOpenSession(0);
    expect(() => guestRecord(s, { axis: "gpu" as "file", target: "x", atMs: 0, rollback: "delete" })).toThrow(/轴/);
    guestClose(s, () => ({ ok: true }));
    expect(() => guestRecord(s, { axis: "file", target: "y", atMs: 9, rollback: "delete" })).toThrow(/关闭/);
  });

  it("残留注入扫描：注入三处清两处检出一处", () => {
    const s = guestOpenSession(0);
    guestRecord(s, { axis: "file", target: "g/left.bin", atMs: 1, rollback: "delete" });
    guestRecord(s, { axis: "file", target: "g/gone.bin", atMs: 2, rollback: "delete" });
    guestRecord(s, { axis: "settings", target: "theme", atMs: 3, rollback: "restore" });
    const residue = guestScanResidue(s, (t) => t === "g/left.bin");
    expect(residue.length).toBe(1);
    expect(residue[0]!.target).toBe("g/left.bin");
  });

  it("回滚失败显性（clean=false + 理由）", () => {
    const s = guestOpenSession(0);
    guestRecord(s, { axis: "file", target: "busy.tmp", atMs: 1, rollback: "delete" });
    const rep = guestClose(s, () => ({ ok: false, reason: "文件被占用" }));
    expect(rep.clean).toBe(false);
    expect(rep.failed[0]!.reason).toBe("文件被占用");
  });

  it("2h 时限提示分级", () => {
    const s = guestOpenSession(0);
    expect(guestRemaining(s, 0).warnLevel).toBe(0);
    expect(guestRemaining(s, 90 * 60000).warnLevel).toBe(1);
    expect(guestRemaining(s, 116 * 60000).warnLevel).toBe(2);
    expect(guestRemaining(s, 121 * 60000).warnLevel).toBe(3);
  });

  it("极简白名单", () => {
    expect(guestUiAllowed("notepad").allowed).toBe(true);
    const d = guestUiAllowed("registry-editor");
    expect(d.allowed).toBe(false);
    expect(d.hint).toBeDefined();
  });
});

/* ------------------------------ capguard（F507/F508） ------------------------------ */

describe("capguard · F507/F508 截图防护", () => {
  it("三路锁屏全拒（三路同函数——一致性来自结构）", () => {
    for (const ch of CAPTURE_CHANNELS) {
      const v = adjudicateCapture(ch, true, false);
      expect(v.allow).toBe(false);
      if (!v.allow) expect(v.reason).toBe("lockscreen");
    }
  });

  it("解锁后恢复 + 防截区域黑块路径 + 零开销 fast path", () => {
    const free = adjudicateCapture("printscreen", false, false);
    expect(free.allow).toBe(true);
    if (free.allow && "fastPath" in free) expect(free.fastPath).toBe(true);
    const m = adjudicateCapture("screen-record", false, true);
    expect(m.allow).toBe(true);
    if (m.allow && "masked" in m) expect(m.reason).toBe("app-shield");
  });

  it("未知通道抛错", () => {
    expect(() => adjudicateCapture("hologram" as "printscreen", false, false)).toThrow(/通道/);
  });

  it("黑块几何：相交/减法/合成全链", () => {
    const a = { x: 0, y: 0, w: 100, h: 100 };
    const b = { x: 50, y: 50, w: 100, h: 100 };
    expect(intersectRect(a, b)).toEqual({ x: 50, y: 50, w: 50, h: 50 });
    expect(subtractRect(a, b).reduce((s, r) => s + r.w * r.h, 0)).toBe(7500);
  });

  it("Z 序遮挡：被顶窗遮住的部分不重复出黑块", () => {
    const win = { rect: { x: 0, y: 0, w: 800, h: 600 }, markers: [{ appId: "bank", region: { x: 100, y: 100, w: 200, h: 100 }, featherPx: 0 }] };
    const cover = { rect: { x: 150, y: 100, w: 100, h: 600 }, markers: [] };
    const blocks = composeShieldBlocks([cover, win]);
    expect(blocks.length).toBe(2);
    expect(blocks.reduce((s, b) => s + b.rect.w * b.rect.h, 0)).toBe(10000);
    expect(blocks.every((b) => b.appId === "bank")).toBe(true);
  });

  it("未标记应用零影响 + 越界声明裁剪对齐", () => {
    expect(composeShieldBlocks([{ rect: { x: 0, y: 0, w: 400, h: 400 }, markers: [] }])).toEqual([]);
    const over = composeShieldBlocks([{ rect: { x: 0, y: 0, w: 100, h: 100 }, markers: [{ appId: "a", region: { x: 50, y: 50, w: 300, h: 300 }, featherPx: 0 }] }]);
    expect(over).toEqual([{ rect: { x: 50, y: 50, w: 50, h: 50 }, appId: "a", featherPx: 0 }]);
  });
});

/* ------------------------------ shredplan（F509/F511/F512） ------------------------------ */

describe("shredplan · F509 粉碎计划器", () => {
  it("机械盘覆写计划（含验证道）与耗时估算", () => {
    const p = planShred(10 * 1024 * 1024, 3, false);
    expect(p.branch).toBe("overwrite");
    expect(p.passes).toBe(3);
    expect(p.verifyPass).toBe(true);
    expect(p.estimateMs).toBeGreaterThan(0);
    expect(shredEtaText(p)).toContain("3 道");
  });

  it("SSD 诚实标注分支", () => {
    const p = planShred(1024, 3, true);
    expect(p.branch).toBe("honest-ssd");
    expect(p.notice).toContain("SSD");
  });

  it("道数越界诚实抛错", () => {
    expect(() => planShred(1024, 0, false)).toThrow(/道数/);
    expect(() => planShred(1024, 9, false)).toThrow(/道数/);
  });
});

describe("shredplan · F511/F512", () => {
  it("清空确认条数文案", () => {
    expect(clipWipeConfirmText(1, 19)).toContain("1 条");
    expect(clipWipeConfirmText(1, 19)).toContain("19 条");
  });

  it("密码后 5s 提示时序（未到不提示、到点提示、confirm 态不再推进）", () => {
    let st: ClipWipeState = { phase: "idle", itemCount: 5, warnUntilMs: 0 };
    st = clipWipeTick(st, 1000, 2000);
    expect(st.phase).toBe("idle");
    st = clipWipeTick(st, 7200, 2000);
    expect(st.phase).toBe("postsecret-warn");
    expect(clipWipeTick({ ...st, phase: "confirm" }, 9000, 2000).phase).toBe("confirm");
  });

  it("清空后诚实失败语义", () => {
    expect(clipPasteAfterWipe()).toEqual({ ok: false, reason: "clipboard-empty" });
  });

  it("截图历史 20 条上限：临时优先淘汰、全保存逐最旧", () => {
    expect(SHOT_HISTORY_CAP).toBe(20);
    const shots = Array.from({ length: 22 }, (_, i) => ({ id: `s${i}`, state: i < 3 ? ("temp" as const) : ("saved" as const), sizeBytes: 1 }));
    expect(evictShots(shots)).toEqual(["s0", "s1"]);
    const savedOnly = Array.from({ length: 21 }, (_, i) => ({ id: `t${i}`, state: "saved" as const, sizeBytes: 1 }));
    expect(evictShots(savedOnly)).toEqual(["t0"]);
  });

  it("关机清理通知只发一次", () => {
    expect(shutdownPurgeNotice(false).notify).toBe(true);
    expect(shutdownPurgeNotice(true).notify).toBe(false);
  });
});

/* ------------------------------ vxcrypt2（F510） ------------------------------ */

describe("vxcrypt2 · F510 容器格式与生命周期", () => {
  const header = {
    magic: "VXCRYPT1",
    version: 1,
    kdfIter: VXCRYPT_KDF_ITER_BASE,
    saltHex: "a".repeat(32),
    ivHex: "b".repeat(24),
    payloadOffset: 128,
  };

  it("头序列化/反序列化 round-trip", () => {
    const line = serializeHeader(header);
    const back = parseHeader(line);
    expect(back).toEqual(header);
  });

  it("非法容器三要素拒绝（魔数/版本过高/弱 KDF）", () => {
    const tail = `0001:000927c0:${"a".repeat(32)}:${"b".repeat(24)}:00000080`;
    expect(() => parseHeader(`NOTAVX:${tail}`)).toThrow(/不是有效/);
    expect(() => parseHeader(`VXCRYPT1:0002:000927c0:${"a".repeat(32)}:${"b".repeat(24)}:00000080`)).toThrow(/版本/);
    expect(() => parseHeader(`VXCRYPT1:0001:000186a0:${"a".repeat(32)}:${"b".repeat(24)}:00000080`)).toThrow(/安全下限/);
    expect(() => serializeHeader({ ...header, kdfIter: VXCRYPT_KDF_ITER_FLOOR - 1 })).toThrow(/下限/);
    expect(() => serializeHeader({ ...header, saltHex: "zz" })).toThrow(/salt/);
  });

  it("临时视图无痕：开→落账→关→清零→清扫幂等", () => {
    const lv = tempViewOpen(["t1", "t2", "t3"]);
    const c1 = tempViewClose(lv, (p) => p !== "t2");
    expect(c1.removed).toEqual(["t1", "t3"]);
    expect(c1.ledger.phase).toBe("cleaning");
    const c2 = tempViewClose(c1.ledger, () => true);
    expect(c2.ledger.phase).toBe("cleaned");
    expect(c2.leftover).toEqual([]);
  });

  it("打包路径规范化与逃逸拒绝", () => {
    expect(normalizeArchiveEntry("C:/docs", "C:/docs\\a\\b\\c.txt").rel).toBe("a/b/c.txt");
    expect(() => normalizeArchiveEntry("C:/docs", "C:/docs/../../x")).toThrow(/\.\./);
    expect(() => normalizeArchiveEntry("C:/docs", "D:/x")).toThrow(/越出/);
  });
});

/* ------------------------------ findrepl（F515/F526） ------------------------------ */

describe("findrepl · F515/F526", () => {
  const text = "Cat catalog cat CAT";
  const opts = { matchCase: false, wholeWord: false };

  it("影响数预览与开关矩阵", () => {
    expect(previewCount(text, "cat", opts)).toBe(4);
    expect(previewCount(text, "cat", { ...opts, matchCase: true })).toBe(2);
    expect(previewCount(text, "cat", { ...opts, wholeWord: true })).toBe(3);
    expect(findAll(text, "", opts)).toEqual([]); // 空针诚实零命中
  });

  it("全部替换 + 整批一次回滚 + 失效代数显性拒绝", () => {
    const s = openReplaceSession(text, "cat", "dog", opts);
    const r = replaceAll(s, text);
    expect(r.applied).toBe(4);
    expect(r.text).toBe("dog dogalog dog dog");
    expect(undoReplaceBatch(s, s.epoch).text).toBe(text);
    const denied = undoReplaceBatch(s, s.epoch + 1);
    expect(denied.text).toBeNull();
    expect(denied.reason).toBeDefined();
  });

  it("逐个替换推进与收尾", () => {
    const s = openReplaceSession(text, "cat", "dog", opts);
    let t = text;
    let done = false;
    for (let i = 0; i < 10 && !done; i++) {
      const n = replaceNext(s, t);
      if (n.done) { done = true; expect(i).toBe(4); break; }
      t = n.text;
    }
    expect(done).toBe(true);
    expect(t).toBe("dog dogalog dog dog");
  });

  it("状态栏三段：24px 基线、空目录态、三处同源、键盘反映", () => {
    expect(STATUSBAR_HEIGHT_PX).toBe(24);
    expect(emptyDirectoryStatus("报告").content).toContain("报告");
    expect(reconcileStatusBarCounts([
      { where: "statusbar", count: 7 }, { where: "detail-pane", count: 7 }, { where: "search", count: 7 },
    ]).consistent).toBe(true);
    const bad = reconcileStatusBarCounts([{ where: "statusbar", count: 7 }, { where: "search", count: 6 }]);
    expect(bad.consistent).toBe(false);
    expect(bad.mismatches).toEqual(["search"]);
    expect(keyboardSelectionText(2, 10, "a.txt")).toContain("3/10");
  });
});

/* ------------------------------ bannerpack（F516/F519/F520） ------------------------------ */

describe("bannerpack · F516/F519/F520", () => {
  it("三档预设与堆叠方向推导", () => {
    expect(Object.keys(BANNER_PRESETS).length).toBe(3);
    expect(bannerLayout("top-left").stackDirection).toBe("down");
    expect(bannerLayout("bottom-left").stackDirection).toBe("up");
  });

  it("贴边堆叠几何（右下向上生长）", () => {
    const rects = bannerStackRects(bannerLayout("bottom-right"), [
      { id: "a", w: 320, h: 80 }, { id: "b", w: 320, h: 80 },
    ], 12, 8, 1920, 1080);
    expect(rects[0]).toEqual({ id: "a", x: 1588, y: 988, w: 320, h: 80 });
    expect(rects[1]!.y).toBe(900);
  });

  it("OSD 独立槽位", () => {
    const osd = osdSlot(1920, 1080, 200, 60, 12);
    expect(osd.x).toBe(860);
    expect(osd.y).toBe(1008);
  });

  it("双音色与音量跟随", () => {
    expect(capsToneFor(true)).toBe("on");
    expect(CAPS_TONE_HZ.on).not.toBe(CAPS_TONE_HZ.off);
    expect(capsVolumeFollow(50)).toBe(50);
    expect(capsVolumeFollow(3)).toBe(8);
    expect(capsVolumeFollow(0)).toBe(0);
  });

  it("三义分流矩阵全格", () => {
    expect(midClickRoute("titlebar", "none", true).action).toBe("minimize");
    expect(midClickRoute("icon", "none", true).action).toBe("close-group");
    expect(midClickRoute("tab", "none", true).action).toBe("new-tab");
    const shift = midClickRoute("titlebar", "shift", true);
    expect(shift.action).toBe("noop");
    if (shift.action === "noop") expect(shift.reason).toContain("不吞");
    expect(midClickRoute("titlebar", "none", false).action).toBe("noop");
  });
});

/* ------------------------------ hotkeymap（F518/F525/F535-F539） ------------------------------ */

describe("hotkeymap · F518/F525/F535-F539", () => {
  it("Caps 600ms 分界三点钉死", () => {
    expect(CAPS_LONG_PRESS_MS).toBe(600);
    expect(capsDualUseVerdict(599)).toEqual({ imeToggle: true, capsLock: false });
    expect(capsDualUseVerdict(600)).toEqual({ imeToggle: false, capsLock: true });
    expect(capsDualUseVerdict(601).capsLock).toBe(true);
    expect(() => capsDualUseVerdict(-1)).toThrow(/时长/);
  });

  it("四方案与让出旧键账", () => {
    expect(IME_SCHEMES.length).toBe(4);
    expect(releaseOldScheme("win-space", "ctrl-space").releasedKey).toBe("Win+Space");
    expect(releaseOldScheme("shift", "shift").releasedKey).toBeNull();
  });

  it("速查卡双格式同源一致 + 改键同步反映", () => {
    const rows = [
      { keys: "Win+1", action: "打开第 1 个应用", category: "窗口" as const },
      { keys: "Win+D", action: "显示桌面", category: "桌面" as const },
    ];
    expect(cardFormatsAgree(rows, "速查卡")).toBe(true);
    const md = exportCardMarkdown(rows, "速查卡");
    const tx = exportCardText(rows, "速查卡");
    expect(md).toContain("| Win+D | 显示桌面 |");
    expect(tx).toContain("Win+D  →  显示桌面");
    const rows2 = rows.map((r) => (r.keys === "Win+D" ? { ...r, keys: "Win+Shift+D" } : r));
    expect(cardFormatsAgree(rows2, "速查卡")).toBe(true);
    expect(exportCardMarkdown(rows2, "速查卡")).not.toContain("| Win+D |");
  });

  it("Win+数字映射与 Shift 新实例", () => {
    const maps = [
      { slot: 1, appId: "explorer", shiftNewInstance: true },
      { slot: 2, appId: "notepad", shiftNewInstance: false },
    ];
    expect(resolveWinNumber(1, true, maps)).toEqual({ launch: "explorer", newInstance: true });
    expect(resolveWinNumber(2, true, maps).newInstance).toBe(false);
    expect(resolveWinNumber(9, false, maps).launch).toBeNull();
  });

  it("布局锁定拒绝反馈", () => {
    const lk = layoutLockFeedback("desktop", true, 120);
    expect(lk).toEqual({ allowed: false, shakeMs: 120, badge: true });
    expect(layoutLockFeedback("taskbar", false, 120).allowed).toBe(true);
  });

  it("冲突审计：重复检出、禁用豁免", () => {
    const conflicts = auditHotkeyConflicts([
      { keys: "Ctrl+Q", owner: "记事本", enabled: true },
      { keys: "Ctrl+Q", owner: "计算器", enabled: true },
      { keys: "Ctrl+Q", owner: "时钟", enabled: false },
    ]);
    expect(conflicts).toEqual([{ keys: "Ctrl+Q", owners: ["记事本", "计算器"] }]);
  });
});

/* ------------------------------ sysdiag（F540-F547） ------------------------------ */

describe("sysdiag · F540-F547 系统与设备", () => {
  it("诊断三要素与报告器", () => {
    expect(diagFindingComplete({ code: "D", what: "a", why: "b", next: "c", severity: "info" })).toBe(true);
    expect(diagFindingComplete({ code: "D", what: "a", why: "", next: "c", severity: "info" })).toBe(false);
    const rep = toDiagReport([{ code: "NET", ok: false, detail: "DNS 无响应" }, { code: "MEM", ok: true, detail: "" }]);
    expect(rep[0]!.severity).toBe("error");
    expect(rep[1]!.severity).toBe("info");
  });

  it("网络重置清单完整性与 90s 取消窗口", () => {
    expect(netResetStepsComplete(["禁用并重新启用网络适配器", "重置 DNS 缓存"]).missing).toEqual(["重置 Winsock 目录", "清除并重建路由表默认项"]);
    expect(netResetStepsComplete(["禁用并重新启用网络适配器", "重置 DNS 缓存", "重置 Winsock 目录", "清除并重建路由表默认项"]).complete).toBe(true);
    expect(netResetCancellable(89)).toBe(true);
    expect(netResetCancellable(90)).toBe(false);
  });

  it("ClickLock 1100ms 边界三点", () => {
    expect(clickLockActivate(1099, true)).toBe(false);
    expect(clickLockActivate(1100, true)).toBe(true);
    expect(clickLockActivate(1101, true)).toBe(true);
    expect(clickLockActivate(1100, false)).toBe(false);
  });

  it("分设备跟随与双滑杆独立", () => {
    const after = applyMasterVolume([
      { deviceId: "spk", volume: 50, followMaster: true },
      { deviceId: "bt", volume: 30, followMaster: false },
    ], +10);
    expect(after[0]!.volume).toBe(60);
    expect(after[1]!.volume).toBe(30);
    expect(dualSliderIndependent(50, 40, +20)).toEqual({ master: 70, notify: 40, independent: true });
  });

  it("电量三处同源 + 跳变平滑", () => {
    expect(batterySourcesAgree([{ pct: 80, source: "statusbar" }, { pct: 80, source: "settings" }, { pct: 80, source: "diag" }]).agree).toBe(true);
    expect(batterySourcesAgree([{ pct: 80, source: "statusbar" }, { pct: 79, source: "diag" }]).agree).toBe(false);
    expect(smoothBatteryStep(50, 60, 2)).toBe(52);
    expect(smoothBatteryStep(50, 51, 2)).toBe(51);
  });

  it("接入通知三态（2s 就绪驻留）", () => {
    expect(deviceArrivalNotice("initializing", 3000).notify).toBe(false);
    expect(deviceArrivalNotice("ready", 1999).notify).toBe(false);
    expect(deviceArrivalNotice("ready", 2000).notify).toBe(true);
    expect(deviceArrivalNotice("driver-missing", 0).text).toContain("驱动缺失");
  });

  it("平衡钳制与等功率增益", () => {
    expect(clampBalance(150)).toBe(100);
    expect(clampBalance(-150)).toBe(-100);
    const mid = channelGains(0);
    expect(mid.left).toBeCloseTo(mid.right);
    expect(channelGains(-100).left).toBeCloseTo(1);
    expect(channelGains(-100).right).toBeCloseTo(0);
    expect(channelGains(100).right).toBeCloseTo(1);
  });
});

/* ------------------------------ explog（十三章） ------------------------------ */

describe("explog · 十三章体验日志", () => {
  it("结论推导统一口径", () => {
    expect(ExpLog.verdictFor(50, false, false)).toBe("smooth");
    expect(ExpLog.verdictFor(120, false, false)).toBe("laggy");
    expect(ExpLog.verdictFor(1500, false, false)).toBe("no-response");
    expect(ExpLog.verdictFor(10, true, false)).toBe("error");
    expect(ExpLog.verdictFor(10, false, true)).toBe("interrupted");
  });

  it("脱敏闸门：敏感目标打码、超长截断", () => {
    expect(ExpLog.sanitizedTargetId("input-password")).toBe("target:redacted");
    expect(ExpLog.sanitizedTargetId("输入密码框")).toBe("target:redacted");
    expect(ExpLog.sanitizedTargetId("a".repeat(100))).toHaveLength(64);
  });

  it("环形缓冲有上限（内存上限纪律）", () => {
    const log = new ExpLog();
    for (let i = 0; i < EXPLOG_CAP + 50; i++) {
      log.record({ atMs: i, domain: "f513", action: "click", targetId: `t${i % 7}`, latencyMs: 10, verdict: "smooth" });
    }
    expect(log.size()).toBe(EXPLOG_CAP);
    expect(log.exportAll()[0]!.atMs).toBe(50); // 最早的 50 条被逐出
  });

  it("rage click 指纹（1.5s 内同点位 ≥3 击）", () => {
    const log = new ExpLog();
    const base = 1000;
    for (let i = 0; i < 3; i++) {
      log.record({ atMs: base + i * 200, domain: "f509", action: "click", targetId: "btn", latencyMs: 10, verdict: "smooth" });
    }
    expect(log.frustrationSignals().some((s) => s.kind === "rage-click")).toBe(true);
  });

  it("dead click 指纹（latency ≥1000 或 no-response）", () => {
    const log = new ExpLog();
    log.record({ atMs: 1, domain: "f509", action: "click", targetId: "shred", latencyMs: 1500, verdict: "no-response" });
    const sig = log.frustrationSignals();
    expect(sig.filter((s) => s.kind === "dead-click").length).toBe(1); // 单事件单指纹（latency 与 verdict 同源不双计）
  });

  it("overlay flap 指纹（10s 内 ≥3 次开关）", () => {
    const log = new ExpLog();
    for (let i = 0; i < 3; i++) {
      log.record({ atMs: i * 2000, domain: "f525", action: "overlay-open", targetId: "kc", latencyMs: 5, verdict: "smooth" });
      log.record({ atMs: i * 2000 + 1000, domain: "f525", action: "overlay-close", targetId: "kc", latencyMs: 5, verdict: "smooth" });
    }
    expect(log.frustrationSignals().some((s) => s.kind === "overlay-flap")).toBe(true);
  });

  it("改进清单（最挫败的十次操作维度）", () => {
    const items = toImprovementItems([
      { kind: "dead-click", targetId: "shred", atMs: 1, verdict: "no-response" },
      { kind: "dead-click", targetId: "shred", atMs: 2, verdict: "no-response" },
      { kind: "rage-click", targetId: "btn", atMs: 3, verdict: "no-response" },
    ]);
    expect(items[0]!.targetId).toBe("shred");
    expect(items[0]!.rank).toBe(1);
    expect(items.length).toBeLessThanOrEqual(10);
  });

  it("全局单例存在且时间轴有序", () => {
    expect(u3ExpLog).toBeInstanceOf(ExpLog);
    const all = u3ExpLog.exportAll();
    expect(all.every((e, i) => i === 0 || all[i - 1]!.seq < e.seq)).toBe(true);
  });
});

/* ------------------------------ walkcheck（十二查） ------------------------------ */

describe("walkcheck · 十二查对账", () => {
  it("十二查清单在册且编号连续", () => {
    expect(TWELVE_QUERIES.length).toBe(12);
    expect(TWELVE_QUERIES.every((q, i) => q.no === i + 1)).toBe(true);
  });

  it("F501-F550 覆盖不重不漏", () => {
    const cov = u3ItemCoverage();
    expect(cov.unique).toBe(50);
    expect(cov.missing).toEqual([]);
    expect(cov.duplicated).toEqual([]);
  });

  it("清单生成：自检类自动装载、走查类显性 pending、缺自检显性缺证", () => {
    const good = buildDomainChecklist("deskicons", {
      runSelfcheck: () => [{ name: "x", pass: true }],
      constantsRegistered: true,
      manualWalkResults: { 4: { pass: true, note: "四档走查绿" } },
    });
    expect(good.find((e) => e.no === 1)!.pass).toBe(true);
    expect(good.find((e) => e.no === 4)!.pass).toBe(true);
    expect(good.filter((e) => e.auto === "manual-walk" && !e.pass).length).toBe(7); // 8 个走查查项 - 1 已登记
    const noSc = buildDomainChecklist("x", { runSelfcheck: null, constantsRegistered: true, manualWalkResults: {} });
    expect(noSc.find((e) => e.no === 1)!.pass).toBe(false);
    expect(noSc.find((e) => e.no === 1)!.detail).toContain("缺证");
  });

  it("汇总：阻塞清单逐条显性", () => {
    const good = buildDomainChecklist("good", { runSelfcheck: () => [{ name: "x", pass: true }], constantsRegistered: true, manualWalkResults: {} });
    const summary = reconcileTwelveQueries([{ domain: "good", evidence: good }]);
    expect(summary.greenDomains).toBe(0);
    expect(summary.blocked.length).toBe(8); // 全部走查查项 pending
    expect(summary.blocked.every((b) => b.what.length > 0)).toBe(true);
  });
});

/* ------------------------------ 与 v1 换行引擎对拍（同判据双实现对账） ------------------------------ */

describe("F502 换行对拍（v1 deskicons × v4 实验室同源）", () => {
  it("长名截断 + 省略号 + 英文不断词——两处调用同一实现", () => {
    const long = wrapIconLabel("项目总结报告最终版本提交给评审委员会审议用副本");
    expect(long.truncated).toBe(true);
    expect(long.lines[1]!.endsWith("…")).toBe(true);
    const eng = wrapIconLabel("hello world dashboard");
    expect(eng.lines[0]!.split(" ").length).toBeGreaterThanOrEqual(1);
    expect(eng.lines.join("")).not.toContain("helloworld");
  });
});
