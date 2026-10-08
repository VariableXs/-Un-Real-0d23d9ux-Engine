import { describe, expect, it } from "vitest";
import { hasPermission, validateManifest, type UxpackManifest } from "../uxpack";

/**
 * X-2/X-3：.uxpack.json Schema v1 与权限模型前端口径测试。
 * Rust 侧 shell/extensions.rs 同规则单测（permission_exact_and_scope 等），
 * 两侧口径保持一致：同 root 作用域声明覆盖同 root 请求。
 */

const base: UxpackManifest = {
  id: "clock-widget",
  name: "Clock Widget",
  version: "1.0.0",
  type: "web",
  permissions: ["widget", "notify", "storage:read:kv", "storage:write:kv"],
  entry: "main.html",
};

describe("uxpack validateManifest", () => {
  it("合法清单通过", () => {
    expect(validateManifest(base)).toEqual([]);
  });

  it("非法 id / type / 权限被拒", () => {
    expect(validateManifest({ ...base, id: "Bad_ID" }).length).toBe(1);
    expect(
      validateManifest({ ...base, type: "alien" as UxpackManifest["type"] }).length
    ).toBe(1);
    expect(
      validateManifest({ ...base, permissions: ["root:all"] }).length
    ).toBe(1);
    expect(validateManifest({ ...base, entry: "" }).length).toBe(1);
  });

  it("三种扩展类型均合法", () => {
    for (const t of ["web", "plugin", "external"] as const) {
      expect(validateManifest({ ...base, type: t })).toEqual([]);
    }
  });
});

describe("uxpack hasPermission（与 Rust 侧一致）", () => {
  it("精确命中", () => {
    expect(hasPermission(base.permissions, "notify")).toBe(true);
    expect(hasPermission(base.permissions, "widget")).toBe(true);
  });

  it("作用域声明覆盖同 root 请求", () => {
    expect(hasPermission(base.permissions, "storage:read")).toBe(true);
    expect(hasPermission(base.permissions, "storage:write")).toBe(true);
  });

  it("越权被拒（X-2 验收：越权 API 调用被拒）", () => {
    expect(hasPermission(base.permissions, "aihub:invoke")).toBe(false);
    expect(hasPermission(base.permissions, "vault:get:secrets")).toBe(false);
    expect(hasPermission(base.permissions, "storage:read")).toBe(true);
    expect(hasPermission(["storage"], "storage:read")).toBe(false); // 精确粒度：必须带作用域
  });
});
