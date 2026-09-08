import { beforeEach, describe, expect, it, vi } from "vitest";
import { BUILTIN_TEMPLATES, TEMPLATES_LS_KEY, TEMPLATE_MAX_BYTES } from "../templates";

/**
 * V-09 回归：模板中心纯逻辑 ——
 * - 内置模板恒在（不可删改）；用户模板 localStorage 持久化（测试环境装 Map 垫片）；
 * - 上限（50 个）/体积（256KB）/非法扩展名如实拒绝，绝不静默截断。
 */

function shimStorage(): void {
  const m = new Map<string, string>();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
    clear: () => void m.clear(),
  };
}

beforeEach(() => {
  vi.resetModules();
  shimStorage();
});

describe("templates（V-09 模板中心）", () => {
  it("内置模板恒在且不可删除/重命名", async () => {
    const t = await import("../templates");
    expect(t.listTemplates().slice(0, 2)).toEqual([...t.BUILTIN_TEMPLATES]);
    t.removeTemplate("builtin-txt");
    t.renameTemplate("builtin-txt", "x");
    expect(t.listTemplates().map((x) => x.id)).toContain("builtin-txt");
  });

  it("新增/重命名/删除 用户模板", async () => {
    const t = await import("../templates");
    const r = t.addTemplate("PS1 脚本", ".ps1", "param()\n");
    expect(r.result).toBe("ok");
    expect(r.template?.ext).toBe("ps1"); // 扩展名去点 + 小写
    const id = r.template!.id;
    t.renameTemplate(id, "脚本");
    expect(t.listTemplates().find((x) => x.id === id)?.name).toBe("脚本");
    t.removeTemplate(id);
    expect(t.listTemplates().find((x) => x.id === id)).toBeUndefined();
  });

  it("非法扩展名 / 超体积 / 空 name 如实拒绝", async () => {
    const t = await import("../templates");
    expect(t.addTemplate("", "txt", "").result).toBe("invalid");
    expect(t.addTemplate("x", "toolongext", "").result).toBe("invalid");
    expect(t.addTemplate("x", "txt", "a".repeat(TEMPLATE_MAX_BYTES + 1)).result).toBe("size");
  });

  it("数量上限 50 如实拒绝", async () => {
    const t = await import("../templates");
    for (let i = 0; i < 50; i++) {
      expect(t.addTemplate(`t${i}`, "txt", "").result).toBe("ok");
    }
    expect(t.addTemplate("overflow", "txt", "").result).toBe("limit");
    expect(t.listTemplates().filter((x) => !x.id.startsWith("builtin-"))).toHaveLength(50);
  });

  it("templateFileName 序号后缀 & 损坏存储兜底", async () => {
    const t = await import("../templates");
    const tpl = { id: "x", name: "笔记", ext: "md", content: "" };
    expect(t.templateFileName(tpl, 0)).toBe("笔记.md");
    expect(t.templateFileName(tpl, 2)).toBe("笔记 (3).md");
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: () => "{corrupted",
      setItem: () => {},
      removeItem: () => {},
      clear: () => {},
    };
    expect(t.listTemplates()).toEqual([...BUILTIN_TEMPLATES]);
    expect(localStorage.getItem(TEMPLATES_LS_KEY)).toBe("{corrupted");
  });
});
