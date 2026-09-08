import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ALARM_MAX,
  CITY_ZONES,
  clockUid,
  createStopwatch,
  createTimer,
  defaultClockHubData,
  formatOffset,
  minuteKey,
  nextAlarmAt,
  alarmShouldFireNow,
  parseData,
  sanitizeAlarm,
  sanitizeAlarms,
  serializeData,
  stopwatchElapsed,
  stopwatchLap,
  stopwatchReset,
  stopwatchToggle,
  timerJustDone,
  timerMarkDone,
  timerRemainMs,
  timerReset,
  timerToggle,
  zoneOffsetMinutes,
  type Alarm,
} from "../clockhub";

/** Z-22 时钟中心：闹钟序列化/校验、锚定计时无漂移、内置城市时区表。 */
afterEach(() => {
  vi.useRealTimers();
});

const alarm = (over: Partial<Alarm> = {}): Alarm => ({
  id: "a1",
  hour: 8,
  minute: 30,
  days: [1, 2, 3, 4, 5],
  enabled: true,
  label: "起床",
  ...over,
});

describe("Z-22 闹钟：序列化往返与非法输入过滤", () => {
  it("合法闹钟序列化 → 解析 往返不变", () => {
    const alarms: Alarm[] = [
      alarm(),
      alarm({ id: "a2", hour: 0, minute: 59, days: [], enabled: false, label: "" }),
      alarm({ id: "a3", hour: 23, minute: 0, days: [0, 6], label: "周末" }),
    ];
    const json = serializeData({ ...defaultClockHubData(), alarms });
    expect(parseData(json).alarms).toEqual(alarms);
  });

  it("非法字段被过滤/修复：小时/分钟域外整条丢弃，days 去重排序，label 截断", () => {
    expect(sanitizeAlarm({ id: "x", hour: 24, minute: 0, days: [], enabled: true, label: "" })).toBeNull();
    expect(sanitizeAlarm({ id: "x", hour: 0, minute: -1, days: [], enabled: true, label: "" })).toBeNull();
    expect(sanitizeAlarm({ id: "", hour: 0, minute: 0, days: [], enabled: true, label: "" })).toBeNull();
    expect(sanitizeAlarm("nonsense")).toBeNull();
    expect(sanitizeAlarm(null)).toBeNull();

    // days 带非法星期（7/-1）与重复 → 只留合法并排序去重
    const fixed = sanitizeAlarm({ id: "x", hour: 7, minute: 5, days: [3, 7, 1, 1, -2, 0], enabled: "yes", label: 42 });
    expect(fixed).toEqual({ id: "x", hour: 7, minute: 5, days: [0, 1, 3], enabled: false, label: "" });

    // label 超长截断
    const long = sanitizeAlarm({ id: "x", hour: 1, minute: 2, days: [], enabled: true, label: "啊".repeat(99) });
    expect(long?.label.length).toBe(40);
  });

  it("批量清洗：非数组/坏 JSON → 空表；混入非法条目只留合法的；条数封顶 ALARM_MAX", () => {
    expect(sanitizeAlarms(undefined)).toEqual([]);
    expect(sanitizeAlarms("no")).toEqual([]);
    const mixed = [
      alarm({ id: "ok1" }),
      { id: "bad", hour: 25, minute: 0, days: [], enabled: true, label: "" },
      alarm({ id: "ok2", days: [2, 2, 9] }),
      "junk",
    ];
    expect(sanitizeAlarms(mixed)).toEqual([alarm({ id: "ok1" }), alarm({ id: "ok2", days: [2] })]);

    const many = Array.from({ length: ALARM_MAX + 5 }, (_, i) => alarm({ id: `a${i}` }));
    expect(sanitizeAlarms(many).length).toBe(ALARM_MAX);
  });

  it("parseData：JSON 损坏 / 城市全非法 → 兜底默认，绝不抛", () => {
    expect(parseData("not json")).toEqual(defaultClockHubData());
    expect(parseData(null)).toEqual(defaultClockHubData());
    const d = parseData(JSON.stringify({ version: 1, cities: ["nowhere"], alarms: [{ id: "z", hour: 9, minute: 9, days: [], enabled: true, label: "" }] }));
    expect(d.cities).toEqual(defaultClockHubData().cities);
    expect(d.alarms.length).toBe(1);
  });

  it("下次响铃：工作日闹钟跳过周末；单次闹钟今天未过点取今天", () => {
    // 2026-01-14 是周三 10:00（本地）
    const wed = new Date(2026, 0, 14, 10, 0, 0);
    const wd = alarm({ days: [1, 2, 3, 4, 5] });
    // 周三 10:00 已过 08:30 → 下一个工作日周四 08:30
    expect(nextAlarmAt(wd, wed)).toBe(new Date(2026, 0, 15, 8, 30).getTime());
    // 周五 10:00 → 跳过周末 → 下周一
    const fri = new Date(2026, 0, 16, 10, 0, 0);
    expect(nextAlarmAt(wd, fri)).toBe(new Date(2026, 0, 19, 8, 30).getTime());
    // 单次：今天 07:00 未到 08:30 → 今天
    const once = alarm({ days: [] });
    expect(nextAlarmAt(once, new Date(2026, 0, 14, 7, 0, 0))).toBe(new Date(2026, 0, 14, 8, 30).getTime());
    // 单次：今天已过 → 明天
    expect(nextAlarmAt(once, wed)).toBe(new Date(2026, 0, 15, 8, 30).getTime());
  });

  it("分钟匹配与分钟键：同一分钟内判定稳定", () => {
    const at = new Date(2026, 0, 14, 8, 30, 20); // 周三
    expect(alarmShouldFireNow(alarm(), at)).toBe(true);
    expect(alarmShouldFireNow(alarm({ enabled: false }), at)).toBe(false);
    expect(alarmShouldFireNow(alarm({ days: [3] }), at)).toBe(true);
    expect(alarmShouldFireNow(alarm({ days: [4] }), at)).toBe(false);
    expect(alarmShouldFireNow(alarm(), new Date(2026, 0, 14, 8, 31, 0))).toBe(false);
    expect(minuteKey(at)).toBe(minuteKey(new Date(2026, 0, 14, 8, 30, 59)));
    expect(minuteKey(at)).not.toBe(minuteKey(new Date(2026, 0, 14, 8, 31, 0)));
  });
});

describe("Z-22 秒表/计时器：系统时钟锚定无漂移", () => {
  it("推进 3 小时读数精确；重复采样不累加（fake 时间推进）", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 14, 9, 0, 0));
    const sw = createStopwatch("s1", "A");
    vi.setSystemTime(new Date(2026, 0, 14, 12, 0, 0)); // +3h（真实墙钟推进，非 tick 累加）
    expect(stopwatchElapsed(sw)).toBe(3 * 3_600_000);
    for (let i = 0; i < 100; i++) expect(stopwatchElapsed(sw)).toBe(3 * 3_600_000); // 读多少次都不漂
  });

  it("暂停期间不走表；恢复后总时长 = 两段之和，精确到毫秒", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 14, 9, 0, 0));
    let sw = createStopwatch("s1", "A");
    vi.setSystemTime(new Date(2026, 0, 14, 9, 0, 10)); // +10s
    sw = stopwatchToggle(sw); // 暂停（结转 10s）
    vi.setSystemTime(new Date(2026, 0, 14, 20, 0, 0)); // 暂停 11 小时
    expect(stopwatchElapsed(sw)).toBe(10_000);
    sw = stopwatchToggle(sw); // 恢复（锚点重设）
    vi.setSystemTime(new Date(2026, 0, 14, 20, 0, 25)); // 再走 25s
    expect(stopwatchElapsed(sw)).toBe(35_000);
    expect(stopwatchElapsed(sw)).toBe(35_000);
  });

  it("计次记录锚定读数；重置归零且停走", () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    let sw = createStopwatch("s1", "A");
    vi.setSystemTime(5_000);
    const l1 = stopwatchLap(sw);
    sw = l1.sw;
    vi.setSystemTime(8_000);
    const l2 = stopwatchLap(sw);
    sw = l2.sw;
    expect(l1.lapMs).toBe(5_000);
    expect(l2.lapMs).toBe(8_000);
    expect(sw.laps).toEqual([5_000, 8_000]);
    const reset = stopwatchReset(sw);
    expect(stopwatchElapsed(reset, 100_000)).toBe(0);
  });

  it("计时器：到点判定精确、暂停不流失、恢复重锚、只报一次", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 14, 9, 0, 0));
    let tm = createTimer("t1", "T", 90_000);
    vi.setSystemTime(new Date(2026, 0, 14, 9, 1, 0)); // +60s
    expect(timerRemainMs(tm)).toBe(30_000);
    expect(timerJustDone(tm)).toBe(false);
    vi.setSystemTime(new Date(2026, 0, 14, 9, 1, 30)); // +90s → 到点
    expect(timerRemainMs(tm)).toBe(0);
    expect(timerJustDone(tm)).toBe(true);
    tm = timerMarkDone(tm);
    expect(timerJustDone(tm)).toBe(false); // 标记后不再重复报
    expect(timerRemainMs(tm)).toBe(0);

    // 暂停锚定：剩 30s 时暂停，等 10 小时后恢复仍剩 30s
    let t2 = createTimer("t2", "T2", 90_000, new Date(2026, 0, 14, 9, 0, 0).getTime());
    vi.setSystemTime(new Date(2026, 0, 14, 9, 1, 0)); // 剩 30s
    t2 = timerToggle(t2); // 暂停
    vi.setSystemTime(new Date(2026, 0, 14, 19, 0, 0)); // +10h（墙钟照走）
    expect(timerRemainMs(t2)).toBe(30_000);
    t2 = timerToggle(t2); // 恢复：endAt = now + 30s
    expect(timerRemainMs(t2)).toBe(30_000);
    vi.setSystemTime(new Date(2026, 0, 14, 19, 0, 29));
    expect(timerJustDone(t2)).toBe(false);
    vi.setSystemTime(new Date(2026, 0, 14, 19, 0, 30));
    expect(timerJustDone(t2)).toBe(true);

    // 重置：用初始时长重新锚定
    const t3 = timerReset(timerMarkDone(t2), new Date(2026, 0, 14, 20, 0, 0).getTime());
    expect(t3.running).toBe(true);
    expect(timerRemainMs(t3, new Date(2026, 0, 14, 20, 0, 0).getTime())).toBe(90_000);
  });

  it("clockUid 会话内唯一", () => {
    const set = new Set(Array.from({ length: 200 }, () => clockUid("sw")));
    expect(set.size).toBe(200);
  });
});

describe("Z-22 内置城市时区表", () => {
  it("20 个城市，id 与 IANA 名均唯一", () => {
    expect(CITY_ZONES.length).toBe(20);
    expect(new Set(CITY_ZONES.map((c) => c.id)).size).toBe(20);
    expect(new Set(CITY_ZONES.map((c) => c.iana)).size).toBeLessThanOrEqual(20); // 北京/上海共用 Asia/Shanghai 属预期
  });

  it("所有 IANA 时区名合法（Intl 可解析，偏移非 null）", () => {
    for (const c of CITY_ZONES) {
      expect(zoneOffsetMinutes(c.iana, new Date(2026, 0, 15)), c.iana).not.toBeNull();
    }
  });

  it("覆盖 ≥6 个不同 UTC 偏移（冬/夏令时两个采样点都满足）", () => {
    for (const at of [new Date(2026, 0, 15), new Date(2026, 6, 15)]) {
      const offs = new Set(CITY_ZONES.map((c) => zoneOffsetMinutes(c.iana, at)).filter((o): o is number => o !== null));
      expect(offs.size, at.toISOString()).toBeGreaterThanOrEqual(6);
    }
  });

  it("偏移格式化：整点/半小时/负偏移", () => {
    expect(formatOffset(480)).toBe("GMT+8");
    expect(formatOffset(330)).toBe("GMT+5:30");
    expect(formatOffset(-180)).toBe("GMT-3");
    expect(formatOffset(0)).toBe("GMT+0");
    expect(formatOffset(null)).toBe("GMT?");
  });
});
