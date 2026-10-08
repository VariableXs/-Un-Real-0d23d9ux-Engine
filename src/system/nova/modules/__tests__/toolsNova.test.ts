import { describe, expect, it } from "vitest";
import {
  BLINK_ICON_MS,
  CROSS_MAP,
  DAY_MIN,
  EYE_RESET_MS,
  EXTEND_MAX,
  EXTEND_MS,
  GRID_MIN,
  HABIT_IDEAL_DEF,
  HABIT_TOL,
  MEETING_LEAD_MIN,
  MICRO_MAX,
  PIN_MAX_LEN,
  QUADRANT_TOOLS,
  ROUNDS_PER_STAMP,
  SCRATCH_TTL_MS,
  SHUTTLE_ARM_H,
  SHUTTLE_TTL_MS,
  SWAP_ANIM_MS,
  TABLE_TTL_MS,
  TOOLS_NOVA_FEATURES,
  UNDO_MAX,
  VOICE_MAX_MS,
  VOICE_MIN_MS,
  adviceRules,
  blockAt,
  blocksConflict,
  carryTopN,
  convertUnit,
  dayHourKey,
  dayKeyOf,
  debriefDue,
  debriefSummary,
  deviationBand,
  evalSnippet,
  evictShuttle,
  eyeInit,
  eyeTick,
  fmtHM,
  formatNum,
  insertBlock,
  isNumericSelection,
  meetingLights,
  meetingReady,
  meetingReport,
  microPush,
  microRemove,
  microToggle,
  nextBlockStart,
  nextQuadrant,
  pinClampText,
  pushUndo,
  ratioPct,
  removeBlock,
  replaceQuadrant,
  scratchExtend,
  scratchState,
  shuttleAnchorKeys,
  shuttleLookup,
  snapDown,
  snapUp,
  stampDots,
  swapPanes,
  tableExpired,
  toTimeBlocks,
  toolForFile,
  toolsNovaDomain,
  undoneCount,
  shiftBlock,
  voiceVerdict,
} from "../toolsNova";

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------
describe("toolsNova manifest", () => {
  it("W-077…W-089 共 13 项，编号连续无缺", () => {
    expect(TOOLS_NOVA_FEATURES).toHaveLength(13);
    const ids = TOOLS_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(77);
    expect(ids[ids.length - 1]).toBe(89);
  });

  it("每项必含中英标题/描述，域标识 S7/AI-07", () => {
    for (const f of TOOLS_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(toolsNovaDomain.id).toBe("S7");
    expect(toolsNovaDomain.route).toBe("AI-07");
  });

  it("默认档与 S0 注册表口径一致（W-086/W-089 opt-in 默认关）", () => {
    const defOf = (id: string): boolean => TOOLS_NOVA_FEATURES.find((f) => f.id === id)?.defaultOn ?? true;
    for (const f of TOOLS_NOVA_FEATURES) {
      expect(f.defaultOn).toBe(f.id === "W-086" || f.id === "W-089" ? false : true);
    }
    expect(defOf("W-086")).toBe(false);
    expect(defOf("W-089")).toBe(false);
  });

  it("overlay 工具窗与注册表对齐（timebox/table/habit/shuttle/scratch）", () => {
    const overlayOf = (id: string): string | undefined =>
      TOOLS_NOVA_FEATURES.find((f) => f.id === id)?.overlay;
    expect(overlayOf("W-077")).toBe("nova-timebox");
    expect(overlayOf("W-080")).toBe("nova-table");
    expect(overlayOf("W-082")).toBe("nova-habit");
    expect(overlayOf("W-085")).toBe("nova-shuttle");
    expect(overlayOf("W-089")).toBe("nova-scratch");
    expect(TOOLS_NOVA_FEATURES.filter((f) => f.overlay).length).toBe(5);
  });

  it("参数卡与注册表对齐（W-079 everyMin / W-081 hour+minute）", () => {
    const paramsOf = (id: string) => TOOLS_NOVA_FEATURES.find((f) => f.id === id)?.params;
    expect(paramsOf("W-079")).toEqual([
      { key: "everyMin", labelKey: "novaP_everyMin", type: "slider", default: 45, min: 20, max: 90, step: 5 },
    ]);
    expect(paramsOf("W-081")).toEqual([
      { key: "hour", labelKey: "novaP_hour", type: "slider", default: 21, min: 19, max: 23, step: 1 },
      { key: "minute", labelKey: "novaP_minute", type: "slider", default: 30, min: 0, max: 59, step: 15 },
    ]);
  });
});

// ---------------------------------------------------------------------------
// W-077 时间块雕塑家
// ---------------------------------------------------------------------------
describe("W-077 time boxing", () => {
  it("15min 网格吸附（下取/上取/封顶）", () => {
    expect(GRID_MIN).toBe(15);
    expect(snapDown(17)).toBe(15);
    expect(snapUp(17)).toBe(30);
    expect(snapDown(-3)).toBe(0);
    expect(snapUp(DAY_MIN + 40)).toBe(DAY_MIN);
  });

  it("冲突判定为半开区间（首尾相接不冲突）", () => {
    const a = { start: 540, end: 600, label: "a" };
    const b = { start: 600, end: 660, label: "b" };
    expect(blocksConflict(a, b)).toBe(false);
    expect(blocksConflict(a, { ...b, start: 555 })).toBe(true);
  });

  it("插入修剪重叠（新块获胜，切掉既有块重叠段，结果有序）", () => {
    const blocks = [
      { start: 540, end: 630, label: "旧" }, // 9:00-10:30
    ];
    const out = insertBlock(blocks, { start: 585, end: 675, label: "新" }); // 9:45-11:15
    expect(out.map((b) => [b.start, b.end, b.label])).toEqual([
      [540, 585, "旧"],
      [585, 675, "新"],
    ]);
  });

  it("整块被覆盖则移除；无重叠原样保留", () => {
    const blocks = [
      { start: 540, end: 555, label: "被吞" },
      { start: 600, end: 615, label: "无叠" },
    ];
    const out = insertBlock(blocks, { start: 540, end: 600, label: "新" });
    expect(out.map((b) => b.label)).toEqual(["新", "无叠"]);
  });

  it("非 15 网格输入被吸附对齐（起点下取、终点上取）", () => {
    const out = insertBlock([], { start: 547, end: 592, label: "x" });
    expect(out[0]).toEqual({ start: 540, end: 600, label: "x" });
  });

  it("步进改期：吸附 15min 并钳制在一天内", () => {
    const b = { start: 1425, end: 1440, label: "尾" };
    expect(shiftBlock(b, 30)).toEqual({ start: 1425, end: 1440, label: "尾" });
    const b2 = { start: 600, end: 630, label: "中" };
    expect(shiftBlock(b2, -GRID_MIN).start).toBe(585);
    expect(shiftBlock(b2, 17).start).toBe(615);
  });

  it("blockAt 命中当前块；nextBlockStart 进行中优先", () => {
    const blocks = [
      { start: 540, end: 600, label: "甲" },
      { start: 660, end: 720, label: "乙" },
    ];
    expect(blockAt(blocks, 570)?.label).toBe("甲");
    expect(blockAt(blocks, 601)).toBeNull();
    const info = nextBlockStart(blocks, 570);
    expect(info).toEqual({ block: blocks[0], state: "active", inMin: 0 });
    const soon = nextBlockStart(blocks, 610);
    expect(soon?.state).toBe("soon");
    expect(soon?.inMin).toBe(50);
    expect(nextBlockStart(blocks, 800)).toBeNull();
  });

  it("removeBlock 按起点移除", () => {
    const blocks = [
      { start: 540, end: 555, label: "a" },
      { start: 600, end: 615, label: "b" },
    ];
    expect(removeBlock(blocks, 540)).toHaveLength(1);
  });

  it("fmtHM 补零与回绕", () => {
    expect(fmtHM(510)).toBe("08:30");
    expect(fmtHM(0)).toBe("00:00");
    expect(fmtHM(1440)).toBe("00:00");
    expect(fmtHM(1441 + 510)).toBe("08:31");
  });
});

// ---------------------------------------------------------------------------
// W-078 微清单
// ---------------------------------------------------------------------------
describe("W-078 micro checklist", () => {
  it("上限 3 条（FIFO）", () => {
    expect(MICRO_MAX).toBe(3);
    let list: ReturnType<typeof microPush>["list"] = [];
    for (const t of ["一", "二", "三"]) {
      list = microPush(list, t, `id-${t}`, 1000).list;
    }
    expect(list.map((x) => x.text)).toEqual(["一", "二", "三"]);
  });

  it("第 4 条加入 → 最旧自动归档", () => {
    const list = [
      { id: "1", text: "一", done: false, at: 1 },
      { id: "2", text: "二", done: false, at: 2 },
      { id: "3", text: "三", done: false, at: 3 },
    ];
    const r = microPush(list, "四", "4", 4);
    expect(r.archived?.text).toBe("一");
    expect(r.list.map((x) => x.text)).toEqual(["二", "三", "四"]);
  });

  it("空文本不入列", () => {
    const r = microPush([], "   ", "x", 1);
    expect(r.list).toHaveLength(0);
    expect(r.archived).toBeNull();
  });

  it("勾选/移除/未完成计数", () => {
    const list = [
      { id: "1", text: "一", done: false, at: 1 },
      { id: "2", text: "二", done: true, at: 2 },
    ];
    expect(microToggle(list, "1")[0]!.done).toBe(true);
    expect(microRemove(list, "1")).toHaveLength(1);
    expect(undoneCount(list)).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// W-079 屏幕时辰簿
// ---------------------------------------------------------------------------
describe("W-079 eye book", () => {
  it("活跃累计到点 → blink + 进入下一轮", () => {
    let st = eyeInit();
    let blinked = false;
    const everyMin = 45;
    for (let s = 0; s < 45 * 60; s++) {
      const r = eyeTick(st, true, (s + 1) * 1000, everyMin);
      st = r.state;
      if (r.blink) {
        blinked = true;
        break;
      }
    }
    expect(blinked).toBe(true);
    expect(st.rounds).toBe(1);
    expect(st.accMs).toBe(0);
  });

  it("≥5min 休憩重置计程", () => {
    expect(EYE_RESET_MS).toBe(5 * 60_000);
    let st = eyeInit();
    st = eyeTick(st, true, 1000, 45).state;
    st = eyeTick(st, true, 2000, 45).state; // 累计 2s
    // 中断 5min 后重新活跃
    st = eyeTick(st, true, 2000 + EYE_RESET_MS + 1000, 45).state;
    expect(st.accMs).toBe(1000);
  });

  it("非活跃 tick 不累计", () => {
    const st = eyeInit();
    const r = eyeTick(st, false, 60_000, 45);
    expect(r.state.accMs).toBe(0);
    expect(r.blink).toBe(false);
  });

  it("everyMin 参数生效（20min 档提前触发）", () => {
    let st = eyeInit();
    let blinked = false;
    for (let s = 0; s < 20 * 60 + 5; s++) {
      const r = eyeTick(st, true, (s + 1) * 1000, 20);
      st = r.state;
      if (r.blink) {
        blinked = true;
        break;
      }
    }
    expect(blinked).toBe(true);
  });

  it("盖章视图：4 轮一章，进度点 4 槽", () => {
    expect(ROUNDS_PER_STAMP).toBe(4);
    expect(stampDots(0)).toEqual({ dots: "○○○○", chapters: 0 });
    expect(stampDots(2)).toEqual({ dots: "●●○○", chapters: 0 });
    expect(stampDots(4)).toEqual({ dots: "○○○○", chapters: 1 });
    expect(stampDots(10)).toEqual({ dots: "●●○○", chapters: 2 });
    expect(BLINK_ICON_MS).toBe(20_000);
  });
});

// ---------------------------------------------------------------------------
// W-080 多工具横桌
// ---------------------------------------------------------------------------
describe("W-080 tool table", () => {
  it("四象限内置工具齐", () => {
    expect([...QUADRANT_TOOLS]).toEqual(["calc", "convert", "note", "picker"]);
  });

  it("Tab 循环聚焦", () => {
    expect(nextQuadrant(0)).toBe(1);
    expect(nextQuadrant(3)).toBe(0);
    expect(nextQuadrant(4)).toBe(1);
  });

  it("换工具位：目标位已有该工具 → 互换（无重复）", () => {
    const next = replaceQuadrant([...QUADRANT_TOOLS], 0, "picker");
    expect(next).toEqual(["picker", "convert", "note", "calc"]);
  });

  it("换工具位：同位同名 → 不变", () => {
    expect(replaceQuadrant([...QUADRANT_TOOLS], 1, "convert")).toEqual([...QUADRANT_TOOLS]);
  });

  it("Esc 收起状态保留 10 分钟", () => {
    expect(TABLE_TTL_MS).toBe(10 * 60_000);
    expect(tableExpired(1000, 1000 + TABLE_TTL_MS)).toBe(false);
    expect(tableExpired(1000, 1000 + TABLE_TTL_MS + 1)).toBe(true);
    expect(tableExpired(0, 999_999)).toBe(false); // 从未收起
  });
});

// ---------------------------------------------------------------------------
// W-081 今日收官仪式
// ---------------------------------------------------------------------------
describe("W-081 daily debrief", () => {
  it("到点触发且当日仅一次", () => {
    expect(debriefDue(21 * 60 + 29, 21, 30, null, "2026-09-10")).toBe(false);
    expect(debriefDue(21 * 60 + 30, 21, 30, null, "2026-09-10")).toBe(true);
    expect(debriefDue(23 * 60 + 59, 21, 30, "2026-09-10", "2026-09-10")).toBe(false);
    expect(debriefDue(23 * 60 + 59, 21, 30, "2026-09-09", "2026-09-10")).toBe(true);
  });

  it("未竟提名至多 3 条且去空", () => {
    expect(carryTopN(["a", "b", "c", "d", "e"])).toEqual(["a", "b", "c"]);
    expect(carryTopN([" ", "", "x"])).toEqual(["x"]);
  });

  it("提名转明日时间块（30min 连续排布，自 9:00 起）", () => {
    const blocks = toTimeBlocks(["甲", "乙"]);
    expect(blocks).toEqual([
      { start: 540, end: 570, label: "甲" },
      { start: 570, end: 600, label: "乙" },
    ]);
    // 一天排满即止
    const many = toTimeBlocks(Array.from({ length: 40 }, (_, i) => `t${i}`));
    expect(many[many.length - 1]!.end).toBeLessThanOrEqual(DAY_MIN);
  });

  it("收官摘要同源账本", () => {
    expect(debriefSummary({ done: 2, closed: 5, typed: 300 })).toEqual([
      "已完成 2 项",
      "关闭窗口 5 个",
      "打字 300 字",
    ]);
  });
});

// ---------------------------------------------------------------------------
// W-082 习惯配比镜
// ---------------------------------------------------------------------------
describe("W-082 habit ratio", () => {
  it("占比计算（双零如实 null）", () => {
    expect(ratioPct(0, 0)).toBeNull();
    expect(ratioPct(40, 60)).toBe(40);
    expect(ratioPct(1, 2)).toBe(33.3);
  });

  it("偏差色带（±15 容差）", () => {
    expect(HABIT_TOL).toBe(15);
    expect(HABIT_IDEAL_DEF).toBe(40);
    expect(deviationBand(null)).toBe("none");
    expect(deviationBand(40)).toBe("ok");
    expect(deviationBand(25)).toBe("ok");
    expect(deviationBand(24)).toBe("low");
    expect(deviationBand(55)).toBe("ok");
    expect(deviationBand(56)).toBe("high");
  });

  it("样本不足如实不给建议", () => {
    expect(adviceRules({ clicks: 10, wheel: 0, keys: 20, mouseSwitch: 9, keySwitch: 1 })).toEqual([]);
  });

  it("切换 80%+ 走鼠标 → Alt+Tab 建议（可解释）", () => {
    const c = { clicks: 300, wheel: 0, keys: 300, mouseSwitch: 17, keySwitch: 3 };
    const rules = adviceRules(c);
    expect(rules[0]!.id).toBe("switch");
    expect(rules[0]!.text).toContain("85%");
    expect(rules[0]!.text).toContain("Alt+Tab");
  });

  it("点击偏重 / 滚轮负载 / 全键盘三条规则", () => {
    const clickHeavy = adviceRules({ clicks: 700, wheel: 0, keys: 300, mouseSwitch: 0, keySwitch: 0 });
    expect(clickHeavy.map((r) => r.id)).toContain("click-heavy");
    const wheelHeavy = adviceRules({ clicks: 100, wheel: 900, keys: 300, mouseSwitch: 0, keySwitch: 0 });
    expect(wheelHeavy.map((r) => r.id)).toContain("wheel");
    const keyHeavy = adviceRules({ clicks: 5, wheel: 0, keys: 800, mouseSwitch: 0, keySwitch: 0 });
    expect(keyHeavy.map((r) => r.id)).toContain("key-heavy");
  });
});

// ---------------------------------------------------------------------------
// W-083 跨工具超拖
// ---------------------------------------------------------------------------
describe("W-083 cross drop", () => {
  it("8 类内置语义映射", () => {
    expect(Object.keys(CROSS_MAP)).toHaveLength(8);
    expect(toolForFile("photo.png")?.tool).toBe("picker");
    expect(toolForFile("song.mp3")?.tool).toBe("wave");
    expect(toolForFile("notes.txt")?.tool).toBe("wordcount");
    expect(toolForFile("data.csv")?.tool).toBe("tablesum");
    expect(toolForFile("clip.mp4")?.tool).toBe("duration");
    expect(toolForFile("bundle.zip")?.tool).toBe("inspect");
    expect(toolForFile("main.ts")?.tool).toBe("loc");
    expect(toolForFile("paper.pdf")?.tool).toBe("wordcount");
  });

  it("未映射 / 无扩展名 → null（拒收）", () => {
    expect(toolForFile("virus.exe")).toBeNull();
    expect(toolForFile("noext")).toBeNull();
    expect(toolForFile("dir.")).toBeNull();
  });

  it("路径取 basename", () => {
    expect(toolForFile("C:\\tmp\\a\\img.jpg")?.name).toBe("img.jpg");
    expect(toolForFile("/tmp/a/img.jpeg")?.ext).toBe("jpeg");
  });
});

// ---------------------------------------------------------------------------
// W-084 会议快车道
// ---------------------------------------------------------------------------
describe("W-084 meeting lane", () => {
  it("四项状态灯顺序固定", () => {
    const lights = meetingLights({ mic: true, dnd: false, awake: null, cam: true });
    expect(lights.map((l) => l.id)).toEqual(["mic", "dnd", "awake", "cam"]);
    expect(lights.map((l) => l.ok)).toEqual([true, false, null, true]);
  });

  it("四灯全绿才就绪；UNKNOWN 不算就绪", () => {
    expect(meetingReady(meetingLights({ mic: true, dnd: true, awake: true, cam: true }))).toBe(true);
    expect(meetingReady(meetingLights({ mic: true, dnd: true, awake: null, cam: true }))).toBe(false);
    expect(meetingReady(meetingLights({ mic: true, dnd: true, awake: false, cam: true }))).toBe(false);
  });

  it("退出汇报（分钟取整，至少 1 分钟）", () => {
    expect(meetingReport(30_000)).toBe("会议态 1 分钟 · 已退出");
    expect(meetingReport(10 * 60_000)).toBe("会议态 10 分钟 · 已退出");
    expect(MEETING_LEAD_MIN).toBe(5);
  });
});

// ---------------------------------------------------------------------------
// W-085 时间梭笔
// ---------------------------------------------------------------------------
describe("W-085 timeline shuttle", () => {
  it("小时键 = 日键-时（本地时区）", () => {
    const t = new Date(2026, 8, 10, 14, 30).getTime();
    expect(dayHourKey(t)).toBe("2026-09-10-14");
    expect(dayKeyOf(t)).toBe("2026-09-10");
  });

  it("72h 滚动淘汰", () => {
    expect(SHUTTLE_TTL_MS).toBe(72 * 3_600_000);
    const now = 1_000_000_000_000;
    const map = {
      fresh: { ts: now - 1000, app: "a" },
      stale: { ts: now - SHUTTLE_TTL_MS - 1000, app: "b" },
    };
    const out = evictShuttle(map, now);
    expect(Object.keys(out)).toEqual(["fresh"]);
  });

  it("对照臂：昨日同时刻 ±1h，由近及远", () => {
    expect(SHUTTLE_ARM_H).toBe(1);
    const now = new Date(2026, 8, 10, 14, 0).getTime();
    expect(shuttleAnchorKeys(now)).toEqual([
      "2026-09-09-15",
      "2026-09-09-14",
      "2026-09-09-13",
    ]);
  });

  it("缺小时如实缺位（null）", () => {
    const now = new Date(2026, 8, 10, 14, 0).getTime();
    const map = { "2026-09-09-14": { ts: now - 24 * 3_600_000, app: "编辑器" } };
    const rows = shuttleLookup(map, now);
    expect(rows).toHaveLength(3);
    expect(rows[0]!.sample).toBeNull();
    expect(rows[1]!.sample?.app).toBe("编辑器");
    expect(rows[2]!.sample).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-086 语音图钉
// ---------------------------------------------------------------------------
describe("W-086 voice pin", () => {
  it("引擎未装 → offline（调用方整项隐藏）", () => {
    expect(voiceVerdict(3000, false)).toEqual({ ok: false, reason: "offline" });
  });

  it("过短 / 正常 / 超长判定", () => {
    expect(VOICE_MIN_MS).toBe(400);
    expect(VOICE_MAX_MS).toBe(10_000);
    expect(voiceVerdict(399, true).reason).toBe("too-short");
    expect(voiceVerdict(3000, true)).toEqual({ ok: true, reason: "ok" });
    expect(voiceVerdict(10_600, true).reason).toBe("too-long");
  });

  it("图钉文本：压空白 + 截断省略", () => {
    expect(pinClampText("  一句   灵感 \n")).toBe("一句 灵感");
    const long = "字".repeat(PIN_MAX_LEN + 10);
    const clamped = pinClampText(long);
    expect(clamped.length).toBe(PIN_MAX_LEN);
    expect(clamped.endsWith("…")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-087 算术选中
// ---------------------------------------------------------------------------
describe("W-087 snippet math", () => {
  it("四则与左结合", () => {
    expect(evalSnippet("1+2*3")).toEqual({ ok: true, value: 7 });
    expect(evalSnippet("10-4-3")).toEqual({ ok: true, value: 3 });
    expect(evalSnippet("2*3+4")).toEqual({ ok: true, value: 10 });
  });

  it("幂右结合 + 括号 + 一元负号", () => {
    expect(evalSnippet("2^3^2")).toEqual({ ok: true, value: 512 });
    expect(evalSnippet("(1+2)*3")).toEqual({ ok: true, value: 9 });
    expect(evalSnippet("-3+5")).toEqual({ ok: true, value: 2 });
    expect(evalSnippet("2*-3")).toEqual({ ok: true, value: -6 });
  });

  it("取模 / 除零 / 非法", () => {
    expect(evalSnippet("10%3")).toEqual({ ok: true, value: 1 });
    expect(evalSnippet("1/0").ok).toBe(false);
    expect(evalSnippet("5%0").ok).toBe(false);
    expect(evalSnippet("1+").ok).toBe(false);
    expect(evalSnippet("(1+2").ok).toBe(false);
    expect(evalSnippet("abc").ok).toBe(false);
    expect(evalSnippet("1&2").ok).toBe(false);
  });

  it("unicode 运算符与千分位逗号", () => {
    expect(evalSnippet("6×7")).toEqual({ ok: true, value: 42 });
    expect(evalSnippet("9÷3")).toEqual({ ok: true, value: 3 });
    expect(evalSnippet("10−4")).toEqual({ ok: true, value: 6 });
    expect(evalSnippet("1,000+1")).toEqual({ ok: true, value: 1001 });
  });

  it("选区可算性：需数字且需运算符", () => {
    expect(isNumericSelection("1+2*3")).toBe(true);
    expect(isNumericSelection(" (1,000 − 4) ")).toBe(true);
    expect(isNumericSelection("123")).toBe(false);
    expect(isNumericSelection("hello+1")).toBe(false);
    expect(isNumericSelection("")).toBe(false);
  });

  it("结果格式化消浮点噪声", () => {
    expect(formatNum(0.1 + 0.2)).toBe("0.3");
    expect(formatNum(42)).toBe("42");
    expect(formatNum(1 / 0)).toBe("∞");
  });

  it("撤销栈上限 5（先进后出）", () => {
    expect(UNDO_MAX).toBe(5);
    let stack: ReturnType<typeof pushUndo> = [];
    for (let i = 0; i < 7; i++) {
      stack = pushUndo(stack, { el: null as unknown as HTMLElement, prev: String(i) });
    }
    expect(stack).toHaveLength(5);
    expect(stack[stack.length - 1]!.prev).toBe("6");
  });
});

// ---------------------------------------------------------------------------
// W-088 双栏交换座
// ---------------------------------------------------------------------------
describe("W-088 swap panes", () => {
  it("全状态互换（路径/滚动/选中，选中深拷贝）", () => {
    expect(SWAP_ANIM_MS).toBe(200);
    const a = { path: "C:/left", scroll: 12, selected: ["a.txt"] };
    const b = { path: "C:/right", scroll: 40, selected: ["b.txt", "c.txt"] };
    const [na, nb] = swapPanes(a, b);
    expect(na).toEqual({ path: "C:/right", scroll: 40, selected: ["b.txt", "c.txt"] });
    expect(nb).toEqual({ path: "C:/left", scroll: 12, selected: ["a.txt"] });
    // 深拷贝：改新选中态不影响旧对象
    na.selected.push("x");
    expect(b.selected).toEqual(["b.txt", "c.txt"]);
  });
});

// ---------------------------------------------------------------------------
// W-089 氛围备忘板
// ---------------------------------------------------------------------------
describe("W-089 ambient scratch", () => {
  it("15min 寿命与最后 60s 微提示", () => {
    expect(SCRATCH_TTL_MS).toBe(15 * 60_000);
    const start = 1000;
    const s1 = scratchState(start, SCRATCH_TTL_MS, 0, start + 14 * 60_000);
    expect(s1.hint).toBe(false);
    expect(s1.burned).toBe(false);
    const s2 = scratchState(start, SCRATCH_TTL_MS, 0, start + 14 * 60_000 + 30_000);
    expect(s2.hint).toBe(true);
  });

  it("寿命归零 → 焚毁", () => {
    const start = 1000;
    expect(scratchState(start, SCRATCH_TTL_MS, 0, start + SCRATCH_TTL_MS).burned).toBe(true);
  });

  it("延寿 +5min 仅一次", () => {
    expect(EXTEND_MS).toBe(5 * 60_000);
    expect(EXTEND_MAX).toBe(1);
    const first = scratchExtend(0);
    expect(first).toEqual({ ok: true, count: 1 });
    // 延寿后寿命 = 15 + 5
    const start = 0;
    const st = scratchState(start, SCRATCH_TTL_MS, 1, start + 16 * 60_000);
    expect(st.burned).toBe(false);
    expect(st.hint).toBe(false); // 距焚毁还有 4min
    const nearEnd = scratchState(start, SCRATCH_TTL_MS, 1, start + 19.5 * 60_000);
    expect(nearEnd.burned).toBe(false);
    expect(nearEnd.hint).toBe(true); // 延寿后最后 60s 内仍微提示
    const second = scratchExtend(1);
    expect(second).toEqual({ ok: false, count: 1 });
  });
});

// ---------------------------------------------------------------------------
// W-080 换算象限（内置极小集）
// ---------------------------------------------------------------------------
describe("W-080 convert unit", () => {
  it("长度/重量/温度三对换算", () => {
    expect(Math.round(convertUnit(1, "m2ft") * 100) / 100).toBeCloseTo(3.28);
    expect(Math.round(convertUnit(3.28084, "ft2m") * 1000) / 1000).toBeCloseTo(1);
    expect(Math.round(convertUnit(1, "kg2lb") * 1000) / 1000).toBeCloseTo(2.205);
    expect(convertUnit(0, "c2f")).toBe(32);
    expect(convertUnit(100, "c2f")).toBe(212);
    expect(convertUnit(32, "f2c")).toBe(0);
    expect(convertUnit(5, "unknown")).toBe(5);
  });
});

// ---------------------------------------------------------------------------
// 回归防护：导出面无幽灵符号（测试里曾出现误加的未用导入）
// ---------------------------------------------------------------------------
describe("module hygiene", () => {
  it("工具位长度恒为 4", () => {
    expect(QUADRANT_TOOLS.length).toBe(4);
  });
});
