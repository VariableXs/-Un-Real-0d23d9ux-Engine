import { describe, expect, it } from "vitest";
import { auditToCsv, auditToJson, csvEscapeField, toCsv } from "../csvExport";
import type { AuditEntry } from "../whitelistTypes";

describe("CSV 字段转义（RFC 4180）", () => {
  it("普通字段不加引号", () => {
    expect(csvEscapeField("hello")).toBe("hello");
    expect(csvEscapeField("123")).toBe("123");
  });

  it("含逗号/双引号/换行/回车 → 整体引号包裹，内部双引号翻倍", () => {
    expect(csvEscapeField("a,b")).toBe('"a,b"');
    expect(csvEscapeField('say "hi"')).toBe('"say ""hi"""');
    expect(csvEscapeField("line1\r\nline2")).toBe('"line1\r\nline2"');
  });
});

describe("审计 CSV 导出（理由码/摘要含逗号必须正确引用）", () => {
  const rows: AuditEntry[] = [
    {
      seq: 1,
      pid: 1001,
      allow: false,
      write: false,
      path: "/etc/passwd",
      pathLen: 12,
      source: "kernel",
    },
    {
      seq: 2,
      pid: 0,
      allow: true,
      write: true,
      path: "rw /handoff",
      pathLen: 12,
      source: "ui-change",
      operator: "local-ui",
      action: "add",
      summary: "rw /handoff, note with, comma",
    },
  ];

  it("含逗号的 summary 字段被引号正确转义", () => {
    const csv = auditToCsv(rows);
    const lines = csv.trim().split("\r\n");
    expect(lines[0]).toContain("summary");
    // 第二条的 summary 含逗号 → 整字段引号包裹
    const dataLine = lines[lines.length - 1];
    expect(dataLine).toContain('"rw /handoff, note with, comma"');
  });

  it("表头 + 每行均含分隔字段；行数以 CRLF 分段", () => {
    const csv = auditToCsv(rows);
    const lines = csv.split("\r\n").filter((l) => l.length > 0);
    expect(lines[0]!.split(",")[0]).toBe("seq");
    expect(lines).toHaveLength(3); // 表头 + 2 行
  });

  it("空表导出仍含表头", () => {
    const csv = auditToCsv([]);
    expect(csv.startsWith("seq,")).toBe(true);
  });

  it("JSON 导出可被解析且字段完整", () => {
    const json = auditToJson(rows);
    const parsed = JSON.parse(json) as AuditEntry[];
    expect(parsed).toHaveLength(2);
    expect(parsed[1]!.summary).toBe("rw /handoff, note with, comma");
  });

  it("toCsv 通用表支持自定义列与转义", () => {
    const out = toCsv(
      [{ name: 'a,"b', n: 1 }],
      [
        { header: "name", get: (r) => r.name },
        { header: "n", get: (r) => r.n },
      ],
    );
    expect(out).toContain('"a,""b"');
  });
});
