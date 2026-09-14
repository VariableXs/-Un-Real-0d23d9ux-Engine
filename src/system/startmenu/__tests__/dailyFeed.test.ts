import { describe, expect, it } from "vitest";
import {
  dailyQuiz,
  dailyTip,
  dayLabel,
  daySerial,
  modIndex,
  pickTrending,
  weekdayLabel,
} from "../dailyFeed";

/**
 * 开始菜单「今日面板」内容源的确定性契约：
 * 同一天内恒定（不随机）、跨天轮换、下标合法、文案格式稳定。
 */
describe("dailyFeed", () => {
  it("daySerial：同一本地日历日恒定，跨日 +1", () => {
    const a = new Date(2026, 8, 14, 0, 0, 1);
    const b = new Date(2026, 8, 14, 23, 59, 59);
    const c = new Date(2026, 8, 15, 0, 0, 0);
    expect(daySerial(a)).toBe(daySerial(b));
    expect(daySerial(c) - daySerial(a)).toBe(1);
  });

  it("modIndex：负数也落回 [0, n)", () => {
    expect(modIndex(-1, 5)).toBe(4);
    expect(modIndex(7, 5)).toBe(2);
    expect(modIndex(3, 0)).toBe(0);
  });

  it("dailyQuiz：同一天多次调用恒定，且答案下标与选项数量自洽", () => {
    const d = new Date(2026, 8, 14, 10, 0, 0);
    const q1 = dailyQuiz(d);
    const q2 = dailyQuiz(d);
    expect(q1).toBe(q2);
    expect(q1.options.length).toBeGreaterThanOrEqual(2);
    expect(q1.answer).toBeGreaterThanOrEqual(0);
    expect(q1.answer).toBeLessThan(q1.options.length);
    expect(q1.q.length).toBeGreaterThan(0);
    expect(q1.note.length).toBeGreaterThan(0);
  });

  it("dailyQuiz / dailyTip：跨天轮换会取到题库里的其他条目", () => {
    const seenQ = new Set<string>();
    const seenT = new Set<string>();
    for (let i = 0; i < 40; i++) {
      const d = new Date(2026, 8, 1 + i);
      seenQ.add(dailyQuiz(d).q);
      seenT.add(dailyTip(d).title);
    }
    expect(seenQ.size).toBeGreaterThan(1);
    expect(seenT.size).toBeGreaterThan(1);
  });

  it("dayLabel / weekdayLabel：中英两语格式稳定", () => {
    const d = new Date(2026, 8, 14);
    expect(dayLabel(d, "zh")).toBe("9月14日");
    expect(dayLabel(d, "en")).toBe("Sep 14");
    expect(weekdayLabel(d, "zh")).toBe("周一");
    expect(weekdayLabel(d, "en")).toBe("Mon");
  });

  it("pickTrending：去重、取 n 个、候选不足时全取", () => {
    expect(pickTrending(["a", "b", "a"], new Date(2026, 8, 14), 4)).toEqual(["a", "b"]);
    const four = pickTrending(["a", "b", "c", "d", "e", "f"], new Date(2026, 8, 14), 4);
    expect(four).toHaveLength(4);
    expect(new Set(four).size).toBe(4);
    // 轮换起点随日期变化，但同一天内恒定
    const same = pickTrending(["a", "b", "c", "d", "e", "f"], new Date(2026, 8, 14, 22), 4);
    expect(same).toEqual(four);
  });
});
