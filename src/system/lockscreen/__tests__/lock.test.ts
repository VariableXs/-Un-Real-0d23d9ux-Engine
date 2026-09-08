import { describe, expect, it } from "vitest";
import {
  aggregateByKind,
  backoffMs,
  randomRecoveryCode,
  sha256Hex,
  verifySecret,
  BACKOFF_MAX_MS,
  FAIL_THRESHOLD,
} from "../lock";

describe("N-10 backoffMs 口令错误指数退避", () => {
  it("第 5 次失败起 30s 翻倍；之前为 0", () => {
    expect(backoffMs(0)).toBe(0);
    expect(backoffMs(4)).toBe(0);
    expect(backoffMs(FAIL_THRESHOLD)).toBe(30_000);
    expect(backoffMs(6)).toBe(60_000);
    expect(backoffMs(7)).toBe(120_000);
  });

  it("封顶 30 分钟；非法输入安全为 0", () => {
    expect(backoffMs(30)).toBe(BACKOFF_MAX_MS);
    expect(backoffMs(999)).toBe(BACKOFF_MAX_MS);
    expect(backoffMs(-1)).toBe(0);
    expect(backoffMs(NaN)).toBe(0);
  });

  it("可注入 base/max（可测性）", () => {
    expect(backoffMs(5, 1000, 60_000)).toBe(1000);
    expect(backoffMs(8, 1000, 3000)).toBe(3000);
  });
});

describe("N-10 口令哈希与恢复码", () => {
  it("sha256Hex 输出 64 位 hex 且确定性一致", async () => {
    const a = await sha256Hex("passphrase");
    const b = await sha256Hex("passphrase");
    expect(a).toBe(b);
    expect(a).toMatch(/^[0-9a-f]{64}$/);
  });

  it("verifySecret：正确通过 / 错误拒绝", async () => {
    const hash = await sha256Hex("s3cret");
    expect(await verifySecret("s3cret", hash)).toBe(true);
    expect(await verifySecret("wrong", hash)).toBe(false);
  });

  it("恢复码 8 位数字；随机源可注入", () => {
    let i = 0;
    const seq = [0.1, 0.99, 0.5, 0.05, 0.75, 0.33, 0.62, 0.47];
    const code = randomRecoveryCode(() => seq[i++ % seq.length]);
    expect(code).toMatch(/^\d{8}$/);
    expect(code).toBe("19507364"); // floor(x*10) 逐位
  });
});

describe("N-10 aggregateByKind 通知聚合（隐私红线）", () => {
  it("只按来源聚合未读数量", () => {
    const groups = aggregateByKind([
      { kind: "privacy", read: false },
      { kind: "privacy", read: false },
      { kind: "hardware", read: false },
      { kind: "hardware", read: true },
      { kind: "system", read: false },
    ]);
    expect(groups).toEqual([
      { kind: "privacy", count: 2 },
      { kind: "hardware", count: 1 },
      { kind: "system", count: 1 },
    ]);
  });

  it("空/全已读 → 空数组", () => {
    expect(aggregateByKind([])).toEqual([]);
    expect(aggregateByKind([{ kind: "system", read: true }])).toEqual([]);
  });

  it("输出类型只含 kind+count，无任何内容字段（编译期+运行期双保险）", () => {
    const groups = aggregateByKind([{ kind: "privacy", read: false }]);
    for (const g of groups) {
      expect(Object.keys(g).sort()).toEqual(["count", "kind"]);
      expect("body" in g).toBe(false);
      expect("title" in g).toBe(false);
    }
  });
});