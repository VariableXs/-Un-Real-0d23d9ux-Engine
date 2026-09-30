import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { accumulateWritten, auditWriteAccounting, columnsFor, healthLevel, noticeFor, unknownHealthMessage, writtenTotal, type MediaHealth } from "../f395-usbHealth";

const OK: MediaHealth = { mediaId: "usb0", label: "系统 U 盘", lifePctRemaining: 85, writtenTb: 12.3, temperatureC: 33, remappedBlocks: 0 };
const WARN: MediaHealth = { ...OK, mediaId: "usb1", label: "老 U 盘", lifePctRemaining: 15 };
const CRIT: MediaHealth = { ...OK, mediaId: "usb2", label: "濒危盘", lifePctRemaining: 8 };
const DARK: MediaHealth = { mediaId: "usb3", label: "哑盘", lifePctRemaining: null, writtenTb: null, temperatureC: null, remappedBlocks: null };

describe("F395 U 盘健康监控", () => {
  it("读得到/读不到两分支（判据）：哑盘 = unknown 且诚实文案、不编数", () => {
    expect(healthLevel(OK)).toBe("ok");
    expect(healthLevel(DARK)).toBe("unknown");
    expect(unknownHealthMessage(DARK)).toContain("不提供健康数据");
    expect(unknownHealthMessage(OK)).toBe("");
    expect(noticeFor(DARK)).toBeNull(); // 读不到不发假警报
  });

  it("两阈值分级：20 黄 10 红（判据）", () => {
    expect(healthLevel(WARN)).toBe("warn");
    expect(healthLevel(CRIT)).toBe("critical");
    expect(healthLevel({ ...OK, lifePctRemaining: 20 })).toBe("ok"); // <20 才黄（边界口径）
    expect(healthLevel({ ...OK, lifePctRemaining: 10 })).toBe("warn");
    expect(healthLevel({ ...OK, lifePctRemaining: 9 })).toBe("critical");
  });

  it("阈值提示与出路链接：warn→迁移指南、critical→备份向导直达（判据）", () => {
    expect(noticeFor(WARN)!.action).toEqual({ label: "查看迁移指南", target: "migration-guide" });
    expect(noticeFor(CRIT)!.action).toEqual({ label: "打开备份向导", target: "backup-wizard" });
    expect(noticeFor(CRIT)!.message).toContain("立即备份");
    expect(noticeFor(OK)).toBeNull();
  });

  it("写入量累计对账（判据）：页面值 vs IO 计数 ≤2%", () => {
    expect(auditWriteAccounting(12.3, 12.3).pass).toBe(true);
    expect(auditWriteAccounting(12.5, 12.3).pass).toBe(true);
    expect(auditWriteAccounting(14, 12.3).pass).toBe(false);
    expect(auditWriteAccounting(0, 0).pass).toBe(true);
  });

  it("多介质分列（判据 S: 共享卷）：各列独立判级", () => {
    const cols = columnsFor([OK, WARN, CRIT, DARK]);
    expect(cols.map((c) => c.level)).toEqual(["ok", "warn", "critical", "unknown"]);
    expect(new Set(cols.map((c) => c.mediaId)).size).toBe(4);
  });

  it("写入量累计账持久化：只增不减、非法增量被拒", () => {
    __clearMem();
    const s = memStore();
    expect(accumulateWritten("usb0", 1.5, s)).toEqual({ ok: true, total: 1.5 });
    accumulateWritten("usb0", 0.2, s);
    expect(writtenTotal("usb0", s)).toBeCloseTo(1.7);
    accumulateWritten("usb0", -5, s); // 非法增量不落账
    expect(writtenTotal("usb0", s)).toBeCloseTo(1.7);
    expect(writtenTotal("ghost", s)).toBe(0);
  });
});
