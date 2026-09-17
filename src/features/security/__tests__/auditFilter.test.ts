import { describe, expect, it } from "vitest";
import { auditActionKind, type AuditEntry } from "../whitelistTypes";
import { filterAudit, sortBySeqDesc } from "../auditFilter";

function mk(partial: Partial<AuditEntry> & { seq?: number }): AuditEntry {
  return {
    pid: 0,
    allow: true,
    write: false,
    path: "",
    pathLen: 0,
    source: "kernel",
    ...partial,
    seq: partial.seq ?? 0,
  };
}

describe("审计过滤逻辑（按进程 / 按路径 / 来源）", () => {
  const entries: AuditEntry[] = [
    mk({ seq: 1, pid: 1001, path: "/handoff/queue.json", source: "kernel" }),
    mk({ seq: 2, pid: 1001, path: "/pub/report.txt", source: "kernel" }),
    mk({ seq: 3, pid: 2002, path: "/etc/passwd", source: "kernel" }),
    mk({ seq: 4, pid: 0, path: "rw /handoff", source: "ui-change", operator: "local-ui", action: "add" }),
  ];

  it("按进程号过滤（PID）", () => {
    expect(filterAudit(entries, { pid: 1001 }).map((e) => e.seq)).toEqual([1, 2]);
    expect(filterAudit(entries, { pid: 2002 }).map((e) => e.seq)).toEqual([3]);
  });

  it("按路径子串过滤（大小写不敏感）", () => {
    expect(filterAudit(entries, { pathSubstr: "HANDOFF" }).map((e) => e.seq)).toEqual([1, 4]);
    expect(filterAudit(entries, { pathSubstr: "/pub" }).map((e) => e.seq)).toEqual([2]);
  });

  it("按来源过滤（kernel / ui-change）", () => {
    expect(filterAudit(entries, { source: "ui-change" }).map((e) => e.seq)).toEqual([4]);
    expect(filterAudit(entries, { source: "kernel" }).map((e) => e.seq)).toEqual([1, 2, 3]);
  });

  it("组合过滤（PID + 路径）取交集", () => {
    expect(filterAudit(entries, { pid: 1001, pathSubstr: "report" }).map((e) => e.seq)).toEqual([2]);
  });

  it("空过滤条件返回全部且不修改原数组", () => {
    const r = filterAudit(entries, {});
    expect(r).toHaveLength(4);
    expect(entries).toHaveLength(4);
  });

  it("按 seq 倒序用于时间线（最新在前）", () => {
    expect(sortBySeqDesc(entries).map((e) => e.seq)).toEqual([4, 3, 2, 1]);
  });
});

describe("审计动作分类（auditActionKind）", () => {
  it("内核访问记录按 allow/write 推导", () => {
    expect(auditActionKind(mk({ allow: true, write: false }))).toBe("allow-read");
    expect(auditActionKind(mk({ allow: true, write: true }))).toBe("allow-write");
    expect(auditActionKind(mk({ allow: false, write: false }))).toBe("deny-read");
    expect(auditActionKind(mk({ allow: false, write: true }))).toBe("deny-write");
  });
  it("ui-change 按 action 推导", () => {
    expect(auditActionKind(mk({ source: "ui-change", action: "add" }))).toBe("add");
    expect(auditActionKind(mk({ source: "ui-change", action: "remove" }))).toBe("remove");
  });
});
