import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import {
  SLEEP_EXEMPT_REMAINING_BYTES,
  TRAY_STEP_PCT,
  auditBoundary,
  closureLog,
  completionNotice,
  logClosure,
  sleepExempt,
  trayProgress,
  verifyFailedPlan,
} from "../f357-downloadClosure";

describe("F357 下载收口体验", () => {
  it("托盘微进度：整数步进、超界钳制、非法输入显式无效", () => {
    const r = trayProgress({ id: "a", fileName: "a", totalBytes: 200, doneBytes: 103 });
    expect(r.valid).toBe(true);
    expect(r.stepped % TRAY_STEP_PCT).toBe(0);
    expect(trayProgress({ id: "a", fileName: "a", totalBytes: 0, doneBytes: 0 }).valid).toBe(false);
    expect(trayProgress({ id: "a", fileName: "a", totalBytes: 100, doneBytes: 999 }).stepped).toBeLessThanOrEqual(100);
  });

  it("睡眠豁免：大余量豁免、小余量不豁免、零余量不豁免（判据三态）", () => {
    const big = [{ id: "a", fileName: "a", totalBytes: SLEEP_EXEMPT_REMAINING_BYTES + 1024, doneBytes: 0 }];
    expect(sleepExempt(big).exempt).toBe(true);
    const small = [{ id: "a", fileName: "a", totalBytes: 1024, doneBytes: 512 }];
    expect(sleepExempt(small).exempt).toBe(false);
    const done = [{ id: "a", fileName: "a", totalBytes: 100, doneBytes: 100 }];
    expect(sleepExempt(done).exempt).toBe(false);
    expect(sleepExempt([]).exempt).toBe(false);
  });

  it("完成通知：双钮链路 + 文件名大小齐全", () => {
    const n = completionNotice("d1", "setup.exe", 1536 * 1024);
    expect(n.actions.map((a) => a.id)).toEqual(["open", "reveal"]);
    expect(n.title).toContain("setup.exe");
    expect(n.body).toContain("1.5 MB");
  });

  it("校验失败 → 重下路径（三要素文案，不静默）", () => {
    const p = verifyFailedPlan("a.zip");
    expect(p.action).toBe("redownload");
    expect(p.message).toContain("完整性校验未通过");
    expect(p.message).toContain("建议");
  });

  it("系统不越界审计：任一面造 UI 即 violated", () => {
    const clean = auditBoundary({});
    expect(clean.every((r) => !r.violated)).toBe(true);
    const dirty = auditBoundary({ hasDownloadListUi: true });
    expect(dirty.find((r) => r.violated)!.facet).toBe("下载列表 UI");
  });

  it("收口动作留痕：滚动上限 100 条、只记动作", () => {
    __clearMem();
    const s = memStore();
    for (let i = 0; i < 105; i++) logClosure({ at: i, noticeId: `n${i}`, action: i % 2 ? "open" : "dismiss" }, s);
    const log = closureLog(s);
    expect(log).toHaveLength(100);
    expect(log[0]!.noticeId).toBe("n5");
    expect(log.every((e) => ["open", "reveal", "dismiss"].includes(e.action))).toBe(true);
  });
});
