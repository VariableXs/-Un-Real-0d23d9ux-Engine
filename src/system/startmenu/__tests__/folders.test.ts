import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * 化境 V-13 回归：固定文件夹 ——
 * - Folder {id,name,items[]} 存 localStorage（variable:start:folders:v1）；
 * - 上限 24 项：mergeFolderItems 超限 ok:false、folderAddItem 满 → "full"（绝不静默丢弃）；
 * - 拖出清空 → 自动解散（不留空壳）；重命名 trim、空名忽略；
 * - 测试环境无 localStorage：装 Map 垫片 + resetModules 后动态导入。
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

const ids = (n: number): string[] => Array.from({ length: n }, (_, i) => `app-${i + 1}`);

describe("grid id 映射（V-13 fd- 前缀）", () => {
  it("folderGridId / isFolderGridId / folderIdOfGrid 往返一致", async () => {
    const f = await import("../folders");
    const gid = f.folderGridId("f1");
    expect(gid).toBe("fd-f1");
    expect(f.isFolderGridId(gid)).toBe(true);
    expect(f.folderIdOfGrid(gid)).toBe("f1");
    expect(f.isFolderGridId("app-write")).toBe(false);
    expect(f.folderIdOfGrid("app-write")).toBeNull();
  });
});

describe("createFolder（V-13 新建）", () => {
  it("新建去重成员并持久化到 localStorage", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("工具", ["app-a", "app-b", "app-a"]);
    expect(folder.items).toEqual(["app-a", "app-b"]);
    expect(f.getFolders()).toHaveLength(1);
    expect(f.getFolders()[0]?.name).toBe("工具");
    // 持久化：重载模块后仍在
    vi.resetModules();
    const f2 = await import("../folders");
    expect(f2.getFolders()[0]?.items).toEqual(["app-a", "app-b"]);
  });
});

describe("folderAddItem（V-13 上限 24）", () => {
  it("正常追加 → ok；重复 → duplicate；不存在 → missing", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("夹", ["app-a"]);
    expect(f.folderAddItem(folder.id, "app-b")).toBe("ok");
    expect(f.folderAddItem(folder.id, "app-b")).toBe("duplicate");
    expect(f.folderAddItem("no-such", "app-c")).toBe("missing");
    expect(f.getFolders()[0]?.items).toEqual(["app-a", "app-b"]);
  });

  it("满 24 项后再加 → full（如实拦截，不静默丢弃）", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("满", ids(24));
    expect(folder.items).toHaveLength(f.FOLDER_MAX_ITEMS);
    expect(f.folderAddItem(folder.id, "app-overflow")).toBe("full");
    expect(f.getFolders()[0]?.items).toHaveLength(24);
    expect(f.getFolders()[0]?.items).not.toContain("app-overflow");
  });
});

describe("folderRemoveItem / disbandFolder（V-13 拖出与解散）", () => {
  it("移出单个成员，其余保留", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("夹", ["app-a", "app-b", "app-c"]);
    f.folderRemoveItem(folder.id, "app-b");
    expect(f.getFolders()[0]?.items).toEqual(["app-a", "app-c"]);
  });

  it("清空即自动解散（不留空壳）", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("夹", ["app-a"]);
    f.folderRemoveItem(folder.id, "app-a");
    expect(f.getFolders()).toHaveLength(0);
  });

  it("disbandFolder 返回成员并移除文件夹；未知 id → 空数组", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("夹", ["tp-1", "sys-2"]);
    expect(f.disbandFolder(folder.id)).toEqual(["tp-1", "sys-2"]);
    expect(f.getFolders()).toHaveLength(0);
    expect(f.disbandFolder("ghost")).toEqual([]);
  });
});

describe("renameFolder（V-13 重命名）", () => {
  it("trim 后生效；空白名忽略", async () => {
    const f = await import("../folders");
    const folder = f.createFolder("旧名", ["app-a"]);
    f.renameFolder(folder.id, "  新名  ");
    expect(f.getFolders()[0]?.name).toBe("新名");
    f.renameFolder(folder.id, "   ");
    expect(f.getFolders()[0]?.name).toBe("新名");
  });
});

describe("mergeFolderItems（V-13 合并判定，纯函数）", () => {
  it("去重且保持先序（a 在前）", async () => {
    const f = await import("../folders");
    const r = f.mergeFolderItems(["app-a", "app-b"], ["app-b", "app-c"]);
    expect(r.ok).toBe(true);
    expect(r.items).toEqual(["app-a", "app-b", "app-c"]);
  });

  it("合并后 24 项 → ok；25 项 → ok:false", async () => {
    const f = await import("../folders");
    expect(f.mergeFolderItems(ids(20), ids(24)).ok).toBe(true); // 去重后 24
    const over = f.mergeFolderItems(ids(24), ["tp-extra"]);
    expect(over.items).toHaveLength(25);
    expect(over.ok).toBe(false);
  });

  it("空对空 / 上限常量为 24", async () => {
    const f = await import("../folders");
    expect(f.mergeFolderItems([], [])).toEqual({ items: [], ok: true });
    expect(f.FOLDER_MAX_ITEMS).toBe(24);
  });
});