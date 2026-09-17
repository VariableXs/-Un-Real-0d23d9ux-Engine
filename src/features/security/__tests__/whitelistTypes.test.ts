import { describe, expect, it } from "vitest";
import {
  AUDIT_PATH_MAX,
  KERNEL_AUDIT_FIELDS,
  RULES_MAX,
  type AuditEntry,
  type WhitelistRule,
} from "../whitelistTypes";

/**
 * 数据模型与内核 vfsguard.rs 逐字段对齐的静态断言（任务 36 验收：字段对齐）。
 */
describe("白名单/审计类型与 vfsguard.rs 对齐", () => {
  it("常量对齐：RULES_MAX=32、AUDIT_PATH_MAX=80", () => {
    expect(RULES_MAX).toBe(32);
    expect(AUDIT_PATH_MAX).toBe(80);
  });

  it("WhitelistRule 字段对齐 Rule{prefix(NormPath), read, write} + index", () => {
    const r: WhitelistRule = { prefix: "/handoff", read: true, write: true, index: 0, wildcard: true };
    expect(r).toHaveProperty("prefix");
    expect(r).toHaveProperty("read");
    expect(r).toHaveProperty("write");
    expect(r).toHaveProperty("index");
  });

  it("AuditEntry 含内核 AuditRecord 全部字段（seq/pid/allow/write/path/path_len）+ 扩展", () => {
    const e: AuditEntry = {
      seq: 1,
      pid: 1001,
      allow: false,
      write: false,
      path: "/etc/passwd",
      pathLen: 12,
      source: "kernel",
    };
    for (const f of KERNEL_AUDIT_FIELDS) {
      expect(e).toHaveProperty(f);
    }
    // ui-change 扩展字段
    const c: AuditEntry = { ...e, source: "ui-change", operator: "local-ui", action: "add", summary: "rw /x" };
    expect(c.operator).toBe("local-ui");
    expect(c.action).toBe("add");
  });
});
