/**
 * AI-U1（F401-F450）前端功能面单测——判据的执行器。
 * 覆盖：九大逻辑模块核心判据 + u1store 底座行为 + ledger 检查项对账。
 * 参数值与 kernel/varix/src/uni1/ v1 语义核同源（一处一事实）。
 */

import { describe, expect, it, beforeEach } from "vitest";
import { U1_DEFAULTS, u1Store, U1_SECTIONS, U1_LS_KEY } from "../u1store";
import {
  TASKMGR_ENTRIES, LOCK_BUDGET_MS, WINX_ITEMS, WINX_OPEN_BUDGET_MS, START_OPEN_BUDGET_MS,
  altF4Resolve, lockLatencyVerdict, mediaPauseResume, refreshWithinBudget, reconcileReadings,
  winIResolve, winXLetterJump, winKeyToggle, typingDuringOpen,
} from "../hotkeys";
import {
  CLOCK_CTX_ITEMS, ESC_TIERS, ESC_BUDGET_MS, SLIDER_REGISTRY, REPEAT_FIRST_DELAY_MS, REPEAT_STEP_MS,
  DROPDOWN_TOGGLE_BUDGET_MS, dialogEscKind, defaultButton, dropdownFollow, escPeelOne,
  escStackInvariant, flipMenu, jumpLetter, keyboardMenuAnchor, repeatDue, sliderStep, taskbarMenuItemCountOk, recentJumps,
} from "../menus";
import {
  HELP_ANCHORS, THIS_PC_LIST, TRASH_DROP_RADIUS_PX, WIN_E_COLD_BUDGET_MS,
  aggregateProps, arrangeMutex, autoArrange, backspaceResolve, deleteRouteNormalizes,
  enterResolve, helpAnchorsValid, inTrashDropZone, insertIntoSequence, nextFolderName,
  trashState, type DeskIcon,
} from "../explorerkeys";
import {
  CAM_PREVIEW_BUDGET_MS, IME_CLICK_BUDGET_MS, METER_LATENCY_BUDGET_MS, VOLUME_APPLY_BUDGET_MS,
  batteryChargeMinutes, batteryRemainMinutes, camParamApply, clampVolume, imeBadge, imeCycle,
  imeThreeWaySync, micAdvice, micLevel, volumeFlyoutGeometry,
} from "../flyouts";
import {
  ICON_MODES, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, TOP_EDGE_RETRACT_MS, anchorZoom,
  contentUnderMouse, fullscreenExitKey, iconModeStep, pageDown, pageReady, pageUp,
  relativeRow, topEdgeTick, type ListNavState, type ZoomState,
} from "../viewfx";
import {
  FORMAT_CANCEL_WINDOW_MS, FOREGROUND_IO_BUDGET_MS, changeLetter,
  formatCancellable, formatNeedsTypeConfirm, formatWarning, fsAllowed, isoMount,
  isoWriteBlocked, letterHolder, looksLikeIso, vaultBadge, vaultEnableReady, vaultUnlock, type VaultState,
} from "../disksuite";
import {
  BT_RECONNECT_BUDGET_MS, DRIVER_SOURCE_LABELS, RESTORE_BUDGET_MS, RESTORE_CAP,
  btConfirm, btFailureCause, printerFailureGuidance, printerFinish, printerInstall,
  printerSearch, restoreDefaultName, restoreDelete, restoreRotate, scheduleLabel, taskDue,
  type BtDevice, type RestorePoint,
} from "../wizards";
import {
  BLACKOUT_BUDGET_MS, CONFIRM_WINDOW_MS, DEFAULT_SCHEME, NUMBER_OVERLAY_MS, SNAP_THRESHOLD_PX,
  canSelect, gamingPick, modeLabel, resetScheme, scaleSuggestion, setPrimary, snapDrag,
  validateCustomWav, windowRehome, type ModeCombo, type ScreenBlock,
} from "../displayav";
import {
  CASCADE_STEP_MS, FILTER_MIN_HOLD_MS, READONLY_SAVE_OPTIONS, SHAKE_AMPLITUDE_MIN_PX,
  SHAKE_CROSSES_REQUIRED, SHAKE_WINDOW_MS, STICKY_TOGGLE_PRESSES, a11yIndicator, cascadeSteps,
  PRINT_QUICK_ITEMS, filterAutoRepeat, filterPress, printScreenResolve, printPreviewPages, saveKeyResolve,
  cutSemantics, shakeDetect, stickyConfirm, stickyEquivalent, stickyKey, stickyShiftPress,
  stickyTap, type FilterState, type StickyState, type TracePoint,
} from "../docops";
import { U1_LEDGER, ledgerTotals, noTruncation, rangeComplete, AGGREGATOR_CAPACITY } from "../ledger";

/* ------------------------------- u1store 底座 ------------------------------- */

describe("u1store", () => {
  beforeEach(() => {
    localStorage.clear();
    // 重置内部 sections（读空根）。
    u1Store.importAll(JSON.stringify({ format: "u1-config", version: 1, sections: {} }));
  });

  it("defaults = 主册判据默认（抽样硬锚）", () => {
    expect((U1_DEFAULTS.startMenu as { openBudgetMs: number }).openBudgetMs).toBe(150);
    expect((U1_DEFAULTS.viewFx as { zoomStep: number }).zoomStep).toBe(250);
    expect((U1_DEFAULTS.diskTools as { isoCap: number }).isoCap).toBe(4);
    expect((U1_DEFAULTS.a11yKeys as { stickyEnabled: boolean }).stickyEnabled).toBe(false);
    expect((U1_DEFAULTS.a11yKeys as { filterMinHoldMs: number }).filterMinHoldMs).toBe(50);
  });

  it("set/subscribe 广播 + 落盘", () => {
    let fired = 0;
    const off = u1Store.subscribe(() => (fired += 1));
    u1Store.set("escStack", { budgetMs: 80 });
    expect(fired).toBe(1);
    expect(u1Store.get<{ budgetMs: number }>("escStack").budgetMs).toBe(80);
    expect(JSON.parse(localStorage.getItem(U1_LS_KEY)!).sections.escStack.budgetMs).toBe(80);
    off();
  });

  it("undoSection 一键还原（栈深 3）", () => {
    u1Store.set("escStack", { budgetMs: 60 });
    u1Store.set("escStack", { budgetMs: 70 });
    expect(u1Store.undoSection("escStack")).toBe(true);
    expect(u1Store.get<{ budgetMs: number }>("escStack").budgetMs).toBe(60);
  });

  it("importAll 校验失败拒载（F305 同源）", () => {
    expect(u1Store.importAll("{broken")).toBe(false);
    expect(u1Store.importAll(JSON.stringify({ format: "x", version: 9, sections: {} }))).toBe(false);
    expect(u1Store.importAll(JSON.stringify({ format: "u1-config", version: 1, sections: { evil: {} } }))).toBe(false);
  });

  it("十五节全登记（范围完整性）", () => {
    expect(U1_SECTIONS.length).toBe(15);
  });
});

/* ------------------------------- hotkeys（F402-F408/F416） ------------------------------- */

describe("hotkeys 系统快捷键", () => {
  it("F402 三入口 + 刷新预算 + 三处对账", () => {
    expect(TASKMGR_ENTRIES.length).toBe(3);
    expect(refreshWithinBudget(1000)).toBe(true);
    expect(refreshWithinBudget(500)).toBe(false);
    expect(reconcileReadings(100, 101, 99)).toBe(true);
    expect(reconcileReadings(100, 110)).toBe(false);
  });

  it("F403 锁定预算与媒体暂停续播", () => {
    expect(LOCK_BUDGET_MS).toBe(300);
    expect(lockLatencyVerdict(250)).toBe("ok");
    expect(lockLatencyVerdict(400)).toBe("over");
    expect(mediaPauseResume(true, true)).toBe("pause");
    expect(mediaPauseResume(false, false)).toBe("resume");
    expect(mediaPauseResume(false, true)).toBe("keep");
  });

  it("F405 焦点三场景判定", () => {
    expect(altF4Resolve("desktop", false, true)).toBe("power-menu");
    expect(altF4Resolve("window", true, true)).toBe("triple-ask");
    expect(altF4Resolve("window", false, true)).toBe("close-window");
    expect(altF4Resolve("modal", false, false)).toBe("close-window");
  });

  it("F407 三场景行为", () => {
    expect(winIResolve("closed")).toBe("open-focus-page");
    expect(winIResolve("open")).toBe("focus-window");
    expect(winIResolve("open-on-search")).toBe("focus-search-keep");
  });

  it("F408 九项对照表 + 首字母唯一命中", () => {
    expect(WINX_ITEMS.length).toBe(9);
    expect(WINX_OPEN_BUDGET_MS).toBe(1000);
    expect(winXLetterJump("t")?.page).toBe("taskmgr");
    expect(winXLetterJump("z")).toBeNull();
    // 九项字母唯一（首字母快捷不歧义）。
    const letters = WINX_ITEMS.map((i) => i.letter);
    expect(new Set(letters).size).toBe(9);
  });

  it("F416 开合时序与连按去抖", () => {
    expect(START_OPEN_BUDGET_MS).toBe(150);
    let s = winKeyToggle({ open: false, pressedAtMs: null }, 0);
    expect(s.open).toBe(true);
    s = winKeyToggle(s, 100); // 开着再按 → 关
    expect(s.open).toBe(false);
    const r = winKeyToggle({ open: true, pressedAtMs: null }, 500);
    expect(r.open).toBe(false);
    expect(typingDuringOpen(false)).toBe(true);   // 非 IME 组合期直通
    expect(typingDuringOpen(true)).toBe(false);   // IME 组合期让路（吞键=0）
  });
});

/* ------------------------------- menus（F419-F424/F433-F436） ------------------------------- */

describe("menus 菜单与对话框", () => {
  it("F419 菜单项数审计与最近 3 条", () => {
    expect(taskbarMenuItemCountOk(8)).toBe(true);
    expect(taskbarMenuItemCountOk(9)).toBe(false);
    expect(recentJumps(["a", "b", "c", "d", "e"])).toEqual(["a", "b", "c"]);
  });

  it("F420 时钟右键三菜单项", () => {
    expect(CLOCK_CTX_ITEMS.length).toBe(3);
  });

  it("F424 四层语义逐层剥离 + 桌面态无副作用 + 不变量", () => {
    expect(ESC_TIERS).toEqual(["popup", "panel", "flyout", "modal"]);
    expect(ESC_BUDGET_MS).toBe(100);
    const stack = ["popup", "panel", "modal"] as const;
    expect(escPeelOne([...stack])).toBe("modal"); // 一次只剥最上层
    expect(escPeelOne([])).toBeNull();            // 桌面态无副作用
    expect(escStackInvariant([...stack])).toBe(true);
    expect(escStackInvariant(["modal", "popup"])).toBe(false); // 更浮的 tier 在底 = 破坏
  });

  it("F433 呼出位置与翻转边界", () => {
    expect(keyboardMenuAnchor(null, { w: 1920, h: 1080 })).toEqual({ x: 960, y: 540 });
    const a = keyboardMenuAnchor({ x: 100, y: 100, w: 200, h: 40 }, { w: 1920, h: 1080 });
    expect(a).toEqual({ x: 200, y: 120 });
    const f = flipMenu({ x: 1900, y: 1000 }, { w: 200, h: 300 }, { w: 1920, h: 1080 });
    expect(f.x).toBe(1712);
    expect(f.y).toBe(772);
  });

  it("F434 默认按钮与删除类 Esc=取消", () => {
    expect(defaultButton([{ label: "好", kind: "normal" }, { label: "确定", kind: "primary" }])).toBe(1);
    expect(defaultButton([{ label: "好", kind: "normal", isDefault: true }, { label: "确定", kind: "primary" }])).toBe(0);
    expect(dialogEscKind(true)).toBe("cancel");
  });

  it("F435 跳选循环与滚动跟随", () => {
    expect(DROPDOWN_TOGGLE_BUDGET_MS).toBe(100);
    const opts = ["甲", "U乙", "丙", "U丁"];
    expect(jumpLetter(opts, 0, "u")).toBe(1);
    expect(jumpLetter(opts, 1, "u")).toBe(3);
    expect(jumpLetter(opts, 3, "u")).toBe(1);  // 循环回环
    expect(jumpLetter([], 0, "u")).toBeNull(); // 空列表安全
    expect(dropdownFollow(5, 0, 5)).toBe(1);
    expect(dropdownFollow(3, 0, 5)).toBe(0);
    expect(dropdownFollow(1, 3, 5)).toBe(1);
  });

  it("F436 滑杆步进表 + 亮度下限 10 + 连发节奏", () => {
    expect(SLIDER_REGISTRY.length).toBe(3);
    const bright = SLIDER_REGISTRY[1];
    expect(sliderStep(bright, 50, -10)).toBe(10);  // F239 同源
    expect(sliderStep(SLIDER_REGISTRY[0], 1, -1)).toBe(0);
    expect(sliderStep(SLIDER_REGISTRY[2], 150, 2)).toBe(200);
    expect(repeatDue(399)).toBe(0);
    expect(repeatDue(400)).toBe(1);
    expect(repeatDue(880)).toBe(9);
    expect(REPEAT_FIRST_DELAY_MS).toBe(400);
    expect(REPEAT_STEP_MS).toBe(60);
  });
});

/* ------------------------------- explorerkeys（F401/F404/F409-F415/F431） ------------------------------- */

const ICONS: DeskIcon[] = [
  { id: 1, name: "报告.docx", sizeKb: 120, kind: "doc", dateMs: 300 },
  { id: 2, name: "照片.png", sizeKb: 40, kind: "img", dateMs: 100 },
  { id: 3, name: "工具.lnk", sizeKb: 5, kind: "app", dateMs: 200 },
];

describe("explorerkeys 资源管理器", () => {
  it("F401 四序排列 + 插入补位 + 模式互斥", () => {
    expect(autoArrange(ICONS, "name").map((i) => i.id)).toEqual([1, 3, 2]);
    expect(autoArrange(ICONS, "size").map((i) => i.id)).toEqual([3, 2, 1]);
    expect(autoArrange(ICONS, "date").map((i) => i.id)).toEqual([2, 3, 1]);
    expect(autoArrange(ICONS, "type").map((i) => i.id)).toEqual([3, 1, 2]); // app < doc < img
    const seq = autoArrange(ICONS, "name");
    expect(insertIntoSequence(seq, { id: 9, name: "阿饼.txt", sizeKb: 1, kind: "doc", dateMs: 50 }, "name")).toBe(0);
    expect(arrangeMutex(true)).toEqual({ auto: true, free: false });
  });

  it("F404 此机页六区 + 冷启动预算", () => {
    expect(THIS_PC_LIST.length).toBe(6);
    expect(WIN_E_COLD_BUDGET_MS).toBe(1500);
  });

  it("F409 锚点全覆盖 + 死锚审计", () => {
    const existing = new Set(Object.values(HELP_ANCHORS));
    expect(helpAnchorsValid(existing)).toBe(true);
    expect(helpAnchorsValid(new Set(["help/appearance"]))).toBe(false);
  });

  it("F410 四键行为矩阵", () => {
    expect(enterResolve("file", false, true)).toBe("preview");
    expect(enterResolve("file", true, true)).toBe("open-app");
    expect(enterResolve("dir", false, true)).toBe("open-tab");
    expect(enterResolve("dir", true, true)).toBe("open-window");
  });

  it("F411 两键分岔与根目录边界", () => {
    expect(backspaceResolve(false, false)).toBe("parent");
    expect(backspaceResolve(true, false)).toBe("history-back");
    expect(backspaceResolve(false, true)).toBe("noop");
  });

  it("F412 合计计算", () => {
    expect(aggregateProps([{ sizeKb: 100 }, { sizeKb: 250 }])).toEqual({ count: 2, totalKb: 350 });
  });

  it("F414 三路同归 + 24px 判定区", () => {
    expect(TRASH_DROP_RADIUS_PX).toBe(24);
    expect(deleteRouteNormalizes("drag")).toBe("confirm-delete");
    expect(deleteRouteNormalizes("del-key")).toBe("confirm-delete");
    expect(inTrashDropZone({ x: 10, y: 10 }, { x: 10, y: 10 })).toBe(true);
    expect(inTrashDropZone({ x: 30, y: 10 }, { x: 10, y: 10 })).toBe(true);  // 20px < 24px 半径内
    expect(inTrashDropZone({ x: 40, y: 10 }, { x: 10, y: 10 })).toBe(false); // 30px > 24px 半径外
  });

  it("F415 两态判定", () => {
    expect(trashState(0, 0)).toBe("empty");
    expect(trashState(5, 99)).toBe("partial");
    expect(trashState(5, 100)).toBe("full");
  });

  it("F431 重名递增", () => {
    expect(nextFolderName(new Set())).toBe("新建文件夹");
    expect(nextFolderName(new Set(["新建文件夹"]))).toBe("新建文件夹(2)");
    expect(nextFolderName(new Set(["新建文件夹", "新建文件夹(2)"]))).toBe("新建文件夹(3)");
  });
});

/* ------------------------------- flyouts（F421-F423/F448/F449） ------------------------------- */

describe("flyouts 指示器与浮层", () => {
  it("F421 徽标三态 + 循环 + 三处同步", () => {
    expect(imeBadge("zh-pinyin", "zh-shuangpin")).toBe("zh");
    expect(imeBadge("zh-shuangpin", "zh-shuangpin")).toBe("zh-shuangpin");
    expect(imeBadge("en-us", null)).toBe("en");
    expect(imeCycle(["a", "b", "c"], 2)).toBe(0);
    expect(imeCycle([], 0)).toBe(0);
    expect(imeThreeWaySync("zh", "zh", "zh")).toBe(true);
    expect(imeThreeWaySync("zh", "zh", "en")).toBe(false);
    expect(IME_CLICK_BUDGET_MS).toBe(100);
  });

  it("F422 浮层几何（图标上方居中 + 边缘钳制）+ 音量步进", () => {
    expect(VOLUME_APPLY_BUDGET_MS).toBe(50);
    const g = volumeFlyoutGeometry({ x: 960, w: 40 }, { w: 280, h: 96 }, { w: 1920, h: 1080 });
    expect(g.x).toBe(840);
    expect(g.y).toBe(1080 - 8 - 96);
    const edge = volumeFlyoutGeometry({ x: 2, w: 40 }, { w: 280, h: 96 }, { w: 1920, h: 1080 });
    expect(edge.x).toBe(8); // 左缘钳制
    expect(clampVolume(41)).toBe(42);
    expect(clampVolume(-5)).toBe(0);
    expect(clampVolume(150)).toBe(100);
  });

  it("F423 续航 ±15% 与充电预计", () => {
    const est = batteryRemainMinutes(50, 10);
    expect(est.minutes).toBe(300);
    expect(est.tolerancePct).toBe(15);
    expect(batteryChargeMinutes(20, 40)).toBe(120);
  });

  it("F448 电平映射与分档建议", () => {
    expect(METER_LATENCY_BUDGET_MS).toBe(100);
    expect(micLevel(0)).toBe(0);
    expect(micLevel(32767)).toBe(1000);
    expect(micLevel(16384)).toBe(500);
    expect(micAdvice(100, 50)).toContain("近一点");
    expect(micAdvice(980, 80)).toContain("降到 60");
    expect(micAdvice(980, 30)).toContain("远一点");
    expect(micAdvice(500, 50)).toContain("正常");
  });

  it("F449 参数两路诚实标注", () => {
    expect(CAM_PREVIEW_BUDGET_MS).toBe(200);
    const supported = { brightness: true, contrast: true, saturation: false };
    expect(camParamApply(supported, "brightness", 70)).toEqual({ ok: true, value: 70 });
    expect(camParamApply(supported, "saturation", 80)).toEqual({ ok: false, note: "该摄像头不支持此调节——参数仅作展示" });
  });
});

/* ------------------------------- viewfx（F429/F430/F432） ------------------------------- */

describe("viewfx 视图", () => {
  it("F429 顶缘滑出与退出双键", () => {
    expect(topEdgeTick(0, 400, "hidden", 0)).toBe("shown");
    expect(topEdgeTick(0, 100, "hidden", 0)).toBe("peeking");
    expect(topEdgeTick(500, 400, "shown", TOP_EDGE_RETRACT_MS)).toBe("hidden");
    expect(fullscreenExitKey("F11")).toBe(true);
    expect(fullscreenExitKey("Escape")).toBe(true);
    expect(fullscreenExitKey("a")).toBe(false);
  });

  it("F430 五档 + 锚点不动点 + 边界贴边", () => {
    expect(ICON_MODES.length).toBe(5);
    expect(iconModeStep(2, 1)).toBe(1);
    expect(iconModeStep(0, 1)).toBeNull();
    expect(iconModeStep(4, -1)).toBeNull();

    // 锚点不动点（内核修复版数学：content = (mouse-offset)*1000/permille）。
    let z: ZoomState = { permille: 1000, offset: { x: 100, y: 50 }, bounceHints: 0 };
    const m = { x: 300, y: 250 };
    const before = contentUnderMouse(z, m);
    z = anchorZoom(z, m, -1); // 缩小 0.75x
    const after = contentUnderMouse(z, m);
    expect(before).toEqual(after);       // 不变量成立
    expect(z.permille).toBe(750);

    // 边界：越界请求末段半步贴边 + 微弹账。
    let b: ZoomState = { permille: 1000, offset: { x: 0, y: 0 }, bounceHints: 0 };
    for (let i = 0; i < 40; i++) b = anchorZoom(b, { x: 0, y: 0 }, 1);
    expect(b.permille).toBe(ZOOM_MAX);
    expect(b.bounceHints).toBeGreaterThanOrEqual(1);
    let s: ZoomState = { permille: 1000, offset: { x: 0, y: 0 }, bounceHints: 0 };
    for (let i = 0; i < 10; i++) s = anchorZoom(s, { x: 0, y: 0 }, -1);
    expect(s.permille).toBe(ZOOM_MIN);   // min 真正可达（贴边语义）
    expect(ZOOM_STEP).toBe(250);
  });

  it("F432 相对位置保持（选中项随页同步移动）", () => {
    const s: ListNavState = { count: 1000, pageRows: 40, selected: 82, scrollTop: 80 };
    expect(relativeRow(s)).toBe(2);
    expect(pageDown(s)).toBe(true);
    expect(s.scrollTop).toBe(120);
    expect(s.selected).toBe(122);
    expect(relativeRow(s)).toBe(2);      // 翻页后仍第 3 行
    const bottom: ListNavState = { count: 45, pageRows: 40, selected: 44, scrollTop: 5 };
    expect(pageDown(bottom)).toBe(false); // 两端到界才无动作
    const top: ListNavState = { count: 100, pageRows: 40, selected: 0, scrollTop: 0 };
    expect(pageUp(top)).toBe(false);
    expect(pageReady(10000, 40, 249)).toBe(true);
    expect(pageReady(45, 40, 1)).toBe(false);
  });
});

/* ------------------------------- disksuite（F437-F440） ------------------------------- */

describe("disksuite 磁盘工具", () => {
  it("F437 警示带 + 二次确认 + 兼容表 + 取消窗口", () => {
    expect(formatWarning("system")).toContain("无法启动");
    expect(formatWarning("shared-s")).toContain("S: 共享卷");
    expect(formatNeedsTypeConfirm("system")).toBe(true);
    expect(formatNeedsTypeConfirm("normal")).toBe(false);
    expect(fsAllowed("NTFS", "system")).toBe(true);
    expect(fsAllowed("FAT32", "system")).toBe(false);
    expect(formatCancellable(FORMAT_CANCEL_WINDOW_MS)).toBe(true);
    expect(formatCancellable(FORMAT_CANCEL_WINDOW_MS + 1)).toBe(false);
  });

  it("F438 冲突拦截 + lnk 自动修复 + 留痕", () => {
    const drives = [
      { id: 1, letter: "C", label: "系统", mountDir: null },
      { id: 2, letter: "D", label: "数据", mountDir: null },
    ];
    const lnks = [{ path: "a.lnk", target: "D:\\tools\\app.exe" }];
    expect(letterHolder(drives, "D")).toBe(2);
    const bad = changeLetter(drives, lnks, 1, "D");
    expect(bad.ok).toBe(false);
    expect(bad.holder).toBe(2);      // 确认前拦截
    const good = changeLetter(drives, lnks, 2, "E");
    expect(good.ok).toBe(true);
    expect(good.fixed).toBe(1);
    expect(lnks[0]!.target).toBe("E:\\tools\\app.exe"); // 前后有效性对比
    expect(good.events.some((e) => e.includes("已跟随改符"))).toBe(true);
  });

  it("F439 强制导出跳不过 + 统一错误文案", () => {
    const base: VaultState = { passwordSet: true, keyGenerated: true, keyExported: false, progressPermille: 0, sessionUnlocked: false, failedAttempts: 0 };
    expect(vaultEnableReady(base)).toBe(false);         // 未导出：拒
    expect(vaultEnableReady({ ...base, keyExported: true })).toBe(true);
    const done: VaultState = { ...base, keyExported: true, progressPermille: 1000 };
    expect(vaultUnlock(done, 1, 1)).toEqual({ ok: true, msg: "已解锁" });
    expect(vaultUnlock(done, 9, 1)).toEqual({ ok: false, msg: "密码不正确" }); // 统一文案
    expect(vaultBadge({ ...base, progressPermille: 500 })).toBe("unlocking");
    expect(vaultBadge({ ...base, sessionUnlocked: true })).toBe("unlocked");
    expect(FOREGROUND_IO_BUDGET_MS).toBe(2);
  });

  it("F440 非 ISO 拒 + 上限 + 只读", () => {
    const sig = new Uint8Array([0x43, 0x44, 0x30, 0x30, 0x31]); // CD001
    expect(looksLikeIso(sig)).toBe(true);
    expect(looksLikeIso(new Uint8Array([1, 2, 3]))).toBe(false);
    const mounted: { source: string; letter: string; files: string[] }[] = [];
    const r1 = isoMount(mounted, "a.iso", sig);
    expect(r1.ok).toBe(true);
    expect(isoMount(mounted, "b.iso", new Uint8Array([9]))).toEqual({ ok: false, reason: expect.stringContaining("不是 ISO") });
    const full: { source: string; letter: string; files: string[] }[] = [
      { source: "1", letter: "E", files: [] }, { source: "2", letter: "F", files: [] },
      { source: "3", letter: "G", files: [] }, { source: "4", letter: "H", files: [] },
    ];
    expect(isoMount(full, "x.iso", sig).ok).toBe(false); // 上限 4
    expect(isoWriteBlocked(full, "E")).toContain("只读");
    expect(isoWriteBlocked(full, "C")).toBeNull();
  });
});

/* ------------------------------- wizards（F441-F444） ------------------------------- */

describe("wizards 向导族", () => {
  it("F441 频率全型人话 + 触发判定矩阵", () => {
    expect(scheduleLabel({ kind: "daily", hour: 3, minute: 30 })).toBe("每天 03:30");
    expect(scheduleLabel({ kind: "weekly", weekday: 1, hour: 9, minute: 0 })).toBe("每周一 09:00");
    const ctx = { nowMin: 540, weekday: 1, dayOfMonth: 1, idleMs: 16 * 60000, onPower: true };
    expect(taskDue({ kind: "weekly", weekday: 1, hour: 9, minute: 0 }, { kind: "always" }, ctx)).toBe(true);
    expect(taskDue({ kind: "daily", hour: 3, minute: 30 }, { kind: "always" }, ctx)).toBe(false);
    expect(taskDue({ kind: "daily", hour: 9, minute: 0 }, { kind: "idle", minutes: 15 }, ctx)).toBe(true);
    expect(taskDue({ kind: "daily", hour: 9, minute: 0 }, { kind: "idle", minutes: 15 }, { ...ctx, idleMs: 5 * 60000 })).toBe(false);
    expect(taskDue({ kind: "daily", hour: 9, minute: 0 }, { kind: "on-power" }, { ...ctx, onPower: false })).toBe(false);
  });

  it("F442 默认名 + 轮替 + 删除保护", () => {
    expect(RESTORE_BUDGET_MS).toBe(30000);
    expect(RESTORE_CAP).toBe(10);
    const name = restoreDefaultName("manual", new Date("2026-09-26T12:34:56").getTime());
    expect(name).toContain("手动-");
    let points: RestorePoint[] = Array.from({ length: RESTORE_CAP + 3 }, (_, i) => ({ name: `p${i}`, kind: "auto" as const, atMs: (i + 1) * 1000, sizeKb: 1 }));
    points = restoreRotate(points);
    expect(points.length).toBe(RESTORE_CAP);
    expect(points[0]!.atMs).toBe(13000);   // 最新在前
    expect(points[points.length - 1]!.atMs).toBe(4000); // 最旧被轮走
    expect(restoreDelete(points, 1, false).ok).toBe(false);
    expect(restoreDelete(points, 0, true).ok).toBe(false); // 最近一个永留
    expect(restoreDelete(points, 1, true).ok).toBe(true);
  });

  it("F443 确认码不跳过 + 归因映射", () => {
    expect(BT_RECONNECT_BUDGET_MS).toBe(3000);
    const d: BtDevice = { id: 1, name: "耳机", rssi: 80, state: "awaiting-confirm", expectCode: 482913, customName: null };
    expect(btConfirm(d, 111)).toBe(false);
    expect(d.state).toBe("failed");
    d.state = "awaiting-confirm";
    d.expectCode = 482913;
    expect(btConfirm(d, 482913)).toBe(true);
    expect(d.state).toBe("paired");
    const [w, n] = btFailureCause(2);
    expect(w).toBe("确认码不匹配");
    expect(n.length).toBeGreaterThan(4);
    expect(btFailureCause(99)[0]).toBe("配对失败");
  });

  it("F444 三态诚实 + 队列衔接 + 测试页 + 搜索", () => {
    const catalog = [
      { model: "StarJet 2000", source: "builtin" as const },
      { model: "StarJet 3000 Pro", source: "vendor" as const },
      { model: "AncientDot 90", source: "manual" as const },
    ];
    expect(printerSearch(catalog, "StarJet").length).toBe(2);
    const printers: { model: string; source: "builtin" | "vendor" | "manual"; state: "installing" | "ready" | "failed"; queueRegistered: boolean }[] = [];
    expect(printerInstall(catalog, "AncientDot 90", printers)).toBe(false); // manual 不假装装好
    expect(printerInstall(catalog, "StarJet 2000", printers)).toBe(true);
    expect(printerFinish(printers, "StarJet 2000")).toBe(true);
    expect(printers[0]!.queueRegistered).toBe(true);  // F289 衔接
    expect(printerFailureGuidance("manual")).toContain("厂商");
    expect(DRIVER_SOURCE_LABELS.manual).toBe("驱动：需手动安装");
    expect(PRINT_QUICK_ITEMS).toBeDefined();
    expect(printPreviewPages(5, 2)).toBe(3);
  });
});

/* ------------------------------- displayav（F445-F447） ------------------------------- */

describe("displayav 显示与声音", () => {
  const screens: ScreenBlock[] = [
    { id: 1, x: 0, y: 0, w: 1920, h: 1080, primary: true },
    { id: 2, x: 1950, y: 100, w: 1280, h: 720, primary: false },
  ];

  it("F445 吸附 ±8px + 主屏互斥 + 回流钳制", () => {
    expect(SNAP_THRESHOLD_PX).toBe(8);
    expect(NUMBER_OVERLAY_MS).toBe(3000);
    const s = snapDrag(screens, 2, 1925, 100);
    expect(s).toEqual({ x: 1920, y: 100 });       // 贴齐右缘
    const far = snapDrag(screens, 2, 1929, 0);
    expect(far).toEqual({ x: 1929, y: 0 });       // 阈值外不吸
    expect(setPrimary(screens, 2)).toBe(true);
    expect(screens[0]!.primary).toBe(false);
    expect(screens[1]!.primary).toBe(true);
    const home = windowRehome(screens, 1, { x: 0.5, y: 0.5 });
    expect(home).toEqual({ x: 1950 + 960, y: 100 + 540 }); // 映射到存活屏（x=1950 起）
    expect(windowRehome(screens, 99, { x: 0.5, y: 0.5 })).toBeNull();
  });

  it("F446 EDID 真实列表 + 游戏向 + 缩放建议", () => {
    const edid: ModeCombo[] = [
      { width: 3840, height: 2160, refreshCentiHz: 6000 },
      { width: 2560, height: 1440, refreshCentiHz: 16500 },
      { width: 1920, height: 1080, refreshCentiHz: 6000 },
    ];
    expect(canSelect(edid, edid[1]!)).toBe(true);
    expect(canSelect(edid, { width: 3840, height: 2160, refreshCentiHz: 24000 })).toBe(false);
    expect(gamingPick(edid)?.refreshCentiHz).toBe(16500);
    expect(modeLabel(edid[0]!)).toBe("3840×2160 @ 60Hz");
    expect(scaleSuggestion(edid[0]!)).toBe(150);
    expect(scaleSuggestion(edid[1]!)).toBe(125);
    expect(scaleSuggestion(edid[2]!)).toBe(100);
    expect(CONFIRM_WINDOW_MS).toBe(15000);
    expect(BLACKOUT_BUDGET_MS).toBe(2000);
  });

  it("F447 WAV 校验 + 默认方案 + 恢复默认", () => {
    const wav = new Uint8Array([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x41, 0x56, 0x45]);
    expect(validateCustomWav(wav, 44100 * 2 * 3 + 44).ok).toBe(true);   // 恰 3 秒
    expect(validateCustomWav(wav, 44100 * 2 * 3 + 44 + 89).ok).toBe(false); // 超 3 秒
    expect(validateCustomWav(new Uint8Array([1]), 100).ok).toBe(false);
    const bindings = new Map<string, string>(DEFAULT_SCHEME.map(([e, s]) => [e, s]));
    bindings.set("notify", "我的-水晶");
    resetScheme(bindings);
    expect(bindings.get("notify")).toBe("默认-叮咚");
  });
});

/* ------------------------------- docops（F413/F425-F428/F450） ------------------------------- */

describe("docops 文档与无障碍", () => {
  it("F413 三键位语义", () => {
    expect(printScreenResolve(false, false)).toBe("fullscreen");
    expect(printScreenResolve(false, true)).toBe("window");
    expect(printScreenResolve(true, false)).toBe("region");
  });

  it("F425 晃动判定 + 误触 20 次 0 触发 + 收缩阶梯", () => {
    expect(SHAKE_WINDOW_MS).toBe(150);
    expect(SHAKE_CROSSES_REQUIRED).toBe(3);
    expect(SHAKE_AMPLITUDE_MIN_PX).toBe(40);
    const shake: TracePoint[] = [
      { tMs: 0, x: 1000 }, { tMs: 30, x: 1060 }, { tMs: 60, x: 940 },
      { tMs: 90, x: 1060 }, { tMs: 120, x: 940 }, { tMs: 145, x: 1000 },
    ];
    expect(shakeDetect(shake)).toBe(true);
    let rejected = 0;
    for (let i = 0; i < 20; i++) {
      const pts: TracePoint[] = i % 4 === 0
        ? [{ tMs: 0, x: 1000 }, { tMs: 40, x: 1010 }, { tMs: 80, x: 990 }]
        : i % 4 === 1
          ? [{ tMs: 0, x: 1000 }, { tMs: 50, x: 1100 }, { tMs: 100, x: 900 }]
          : i % 4 === 2
            ? [{ tMs: 0, x: 1000 }, { tMs: 100, x: 1100 }, { tMs: 200, x: 900 }, { tMs: 300, x: 1100 }, { tMs: 400, x: 900 }]
            : [{ tMs: 0, x: 1000 }, { tMs: 60, x: 1200 }, { tMs: 120, x: 1400 }];
      if (!shakeDetect(pts)) rejected += 1;
    }
    expect(rejected).toBe(20);
    expect(CASCADE_STEP_MS).toBe(40);
    expect(cascadeSteps(3)).toEqual([0, 40, 80]);
  });

  it("F426 三键矩阵 + 只读三问", () => {
    expect(saveKeyResolve(true, false, "s")).toBe("save");
    expect(saveKeyResolve(true, true, "s")).toBe("save-as");
    expect(saveKeyResolve(false, false, "F12")).toBe("save-as");
    expect(saveKeyResolve(false, false, "a")).toBe("noop");
    expect(READONLY_SAVE_OPTIONS[2]).toBe("取消"); // 取消永远安全出路
  });

  it("F427 剪切延迟执行（文件域）", () => {
    expect(cutSemantics("files")).toBe("deferred");
    expect(cutSemantics("text")).toBe("immediate");
  });

  it("F450 粘滞键全状态机 + 五组合键等效 + 筛选键", () => {
    let s: StickyState = { enabled: false, pendingConfirm: false, neverRemind: false, latched: [], shiftPresses: 0, enableNotices: 0 };
    for (let i = 0; i < STICKY_TOGGLE_PRESSES - 1; i++) s = stickyShiftPress(s);
    expect(s.pendingConfirm).toBe(false);
    s = stickyShiftPress(s);
    expect(s.pendingConfirm).toBe(true);
    s = stickyConfirm(s);
    expect(s.enabled && s.enableNotices === 1).toBe(true);

    // 五组合键逐键等效。
    const combos: [Array<"Ctrl" | "Alt" | "Shift" | "Win">, string][] = [
      [["Ctrl"], "c"], [["Ctrl", "Shift"], "n"], [["Alt"], "f4"],
      [["Ctrl", "Alt"], "d"], [["Ctrl", "Shift", "Alt"], "s"],
    ];
    for (const [direct, key] of combos) {
      let t: StickyState = { ...s, latched: [] };
      for (const m of direct) t = stickyTap(t, m);
      const { combo, key: got, next } = stickyKey(t, key);
      expect(stickyEquivalent(direct, combo, key, got)).toBe(true);
      expect(next.latched.length).toBe(0); // 一次合成后回弹
    }

    // 锁存取消。
    let t2: StickyState = { ...s, latched: [] };
    t2 = stickyTap(t2, "Ctrl");
    t2 = stickyTap(t2, "Ctrl");
    expect(t2.latched.length).toBe(0);

    // 筛选键。
    const f: FilterState = { enabled: true, minHoldMs: FILTER_MIN_HOLD_MS, ignoredTaps: 0, suppressedRepeats: 0 };
    expect(filterPress(f, 20)).toBe(false);
    expect(filterPress(f, 50)).toBe(true);
    expect(filterAutoRepeat(f)).toBe(false);
    expect(a11yIndicator(true, false)).toBe(true);
    expect(a11yIndicator(false, false)).toBe(false);
  });
});

/* ------------------------------- ledger 检查项对账 ------------------------------- */

describe("ledger 检查项对账（与 tally.rs 同数）", () => {
  it("51 块零截断 + F401-F450 范围完整", () => {
    expect(U1_LEDGER.length).toBe(51);
    expect(AGGREGATOR_CAPACITY).toBe(64);
    expect(noTruncation()).toBe(true);
    expect(rangeComplete()).toBe(true);
  });

  it("汇总 = 机数基准（tally.rs TOTAL 行）", () => {
    const t = ledgerTotals();
    expect(t.blocks).toBe(51);
    expect(t.items).toBe(50);
    expect(t.checks).toBe(492);   // 2026-09-26 tally 机数
    expect(t.unitTests).toBe(114); // 各文件 #[test] 机数（主 crate 115 = 114 + 域聚合 1）
  });

  it("数字全部为正（无占位）", () => {
    for (const r of U1_LEDGER) {
      expect(r.checks).toBeGreaterThan(0);
      expect(r.module).toMatch(/\.rs$/);
      expect(r.name.length).toBeGreaterThan(2);
    }
  });
});
