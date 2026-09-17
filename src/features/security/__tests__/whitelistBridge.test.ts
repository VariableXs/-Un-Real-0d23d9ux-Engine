import { afterEach, describe, expect, it, vi } from "vitest";
import { type ShimTransport } from "../../../lib/shim/shimInvoke";
import {
  VfsGuardBridge,
  type StorageLike,
} from "../whitelistBridge";
import type { AuditEntry, WhitelistRule } from "../whitelistTypes";

function memoryStorage(): StorageLike {
  const m = new Map<string, string>();
  return {
    getItem: (k) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k, v) => void m.set(k, v),
    removeItem: (k) => void m.delete(k),
  };
}

/** 真实通道桩：维护内核侧规则表与审计账本，模拟 vfs_whitelist_* / vfs_audit_list。 */
function makeKernelBridge() {
  let rules: WhitelistRule[] = [];
  const audit: AuditEntry[] = [];
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  const invoke: ShimTransport = vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "vfs_whitelist_list":
        return rules.map((r) => ({ ...r })) as unknown;
      case "vfs_whitelist_add": {
        const idx = rules.length;
        const r: WhitelistRule = {
          prefix: String(args.prefix),
          read: Boolean(args.read),
          write: Boolean(args.write),
          index: idx,
          wildcard: false,
        };
        rules = [...rules, r];
        return { ...r } as unknown;
      }
      case "vfs_whitelist_remove": {
        const i = Number(args.index);
        rules = rules.filter((r) => r.index !== i).map((r, k) => ({ ...r, index: k }));
        return undefined as unknown;
      }
      case "vfs_audit_list":
        return audit.map((a) => ({ ...a })) as unknown;
      default:
        throw Object.assign(new Error("MISSING"), { __shim_missing: cmd });
    }
  });
  // 内核账本可由测试预置访问审计（source=kernel）
  const seedAudit = (e: AuditEntry) => {
    audit.push(e);
  };
  return { invoke, calls, seedAudit };
}

/** 永不可用通道：能力位真但命令全 MISSING。 */
const missingInvoke: ShimTransport = vi.fn(async (cmd: string) => {
  throw Object.assign(new Error("MISSING"), { __shim_missing: cmd });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("白名单规则 CRUD 状态机（本地预览态）", () => {
  it("增 → 规范化前缀、分配 index；通配 /* 收敛为前缀", async () => {
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    expect(b.getMode()).toBe("local");

    const r1 = await b.addRule({ prefix: "/handoff", read: true, write: true });
    expect(r1.ok).toBe(true);
    const r2 = await b.addRule({ prefix: "/pub/*", read: true, write: false });
    expect(r2.ok).toBe(true);

    const rules = b.getRules();
    expect(rules).toHaveLength(2);
    expect(rules[0]).toMatchObject({ prefix: "/handoff", read: true, write: true, index: 0 });
    expect(rules[1]).toMatchObject({ prefix: "/pub", read: true, write: false, wildcard: true, index: 1 });
  });

  it("非法前缀被拒：非绝对路径 / 含 .. 或反斜杠", async () => {
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    expect((await b.addRule({ prefix: "handoff", read: true, write: false })).ok).toBe(false);
    expect((await b.addRule({ prefix: "/a/../b", read: true, write: false })).ok).toBe(false);
    expect((await b.addRule({ prefix: "/a\\b", read: true, write: false })).ok).toBe(false);
    expect(b.getRules()).toHaveLength(0);
  });

  it("规则满容（32）拒绝后续新增", async () => {
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    for (let i = 0; i < 32; i++) {
      const res = await b.addRule({ prefix: `/d${i}`, read: true, write: false });
      expect(res.ok).toBe(true);
    }
    const over = await b.addRule({ prefix: "/overflow", read: true, write: false });
    expect(over.ok).toBe(false);
    if (!over.ok) expect(over.code).toBe("RULES_FULL");
    expect(b.getRules()).toHaveLength(32);
  });

  it("删除按 index 定位并回缩序号；增删均留痕审计", async () => {
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    await b.addRule({ prefix: "/a", read: true, write: false });
    await b.addRule({ prefix: "/b", read: true, write: false });
    const rem = await b.removeRule(0);
    expect(rem.ok).toBe(true);
    expect(b.getRules().map((r) => r.prefix)).toEqual(["/b"]);

    const audit = b.getAudit();
    // 两条 ui-change：add /a、add /b、remove /a → 共 3 条
    expect(audit.filter((e) => e.source === "ui-change")).toHaveLength(3);
    expect(audit[audit.length - 1]!.action).toBe("remove");
  });

  it("变更审计自身留痕：operator=local-ui、与内核记录同 schema 同列表", async () => {
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    await b.addRule({ prefix: "/x", read: true, write: true });
    const entry = b.getAudit().find((e) => e.source === "ui-change");
    expect(entry).toBeDefined();
    expect(entry?.operator).toBe("local-ui");
    expect(entry?.action).toBe("add");
    expect(entry?.summary).toBe("rw /x");
  });
});

describe("垫片降级路径（任务 36：命令 MISSING → 本地镜像 + 角标）", () => {
  it("能力位满足但命令 MISSING → 降级本地预览，isLocalPreview=true", async () => {
    const b = new VfsGuardBridge({
      available: () => true,
      invoke: missingInvoke,
      storage: memoryStorage(),
    });
    const mode = await b.init();
    expect(mode).toBe("local");
    expect(b.isLocalPreview()).toBe(true);
  });

  it("本地镜像经 localStorage 持久化：重建桥后规则与审计仍在", async () => {
    const storage = memoryStorage();
    const b1 = new VfsGuardBridge({ available: () => false, storage });
    await b1.init();
    await b1.addRule({ prefix: "/persist", read: true, write: false });

    const b2 = new VfsGuardBridge({ available: () => false, storage });
    await b2.init();
    expect(b2.getRules().map((r) => r.prefix)).toEqual(["/persist"]);
    expect(b2.getAudit().some((e) => e.source === "ui-change")).toBe(true);
  });

  it("规则变更后 emit settings://changed 事件（经垫片事件通道）", async () => {
    const spy = vi.spyOn(await import("../../../lib/shim/shimInvoke"), "dispatchShimEvent");
    const b = new VfsGuardBridge({ available: () => false, storage: memoryStorage() });
    await b.init();
    await b.addRule({ prefix: "/emit", read: true, write: false });
    expect(spy).toHaveBeenCalledWith("settings://changed", expect.objectContaining({ area: "vfsguard" }));
  });
});

describe("真实内核通道同步（命令可用时走内核，失败回落本地）", () => {
  it("init 拉取内核规则与审计 → 模式 kernel；add 下发 vfs_whitelist_add 并回读", async () => {
    const kernel = makeKernelBridge();
    const b = new VfsGuardBridge({ available: () => true, invoke: kernel.invoke, storage: memoryStorage() });
    await b.init();
    expect(b.getMode()).toBe("kernel");

    const res = await b.addRule({ prefix: "/kernel", read: true, write: true });
    expect(res.ok).toBe(true);
    expect(kernel.calls.some((c) => c.cmd === "vfs_whitelist_add" && c.args.prefix === "/kernel")).toBe(true);
    expect(b.getRules().map((r) => r.prefix)).toContain("/kernel");
  });

  it("内核 add 抛 MISSING → 回落本地预览并继续生效（绝不静默假装成功）", async () => {
    const b = new VfsGuardBridge({
      available: () => true,
      invoke: missingInvoke,
      storage: memoryStorage(),
    });
    await b.init();
    expect(b.getMode()).toBe("local");
    const res = await b.addRule({ prefix: "/fallback", read: true, write: false });
    expect(res.ok).toBe(true);
    expect(b.getRules().map((r) => r.prefix)).toContain("/fallback");
    expect(b.isLocalPreview()).toBe(true);
  });

  it("内核预置访问审计与 ui-change 变更审计同列表、按 seq 时间线排序", async () => {
    const kernel = makeKernelBridge();
    kernel.seedAudit({
      seq: 5,
      pid: 1001,
      allow: false,
      write: false,
      path: "/etc/passwd",
      pathLen: 12,
      source: "kernel",
    });
    const b = new VfsGuardBridge({ available: () => true, invoke: kernel.invoke, storage: memoryStorage() });
    await b.init();
    await b.addRule({ prefix: "/x", read: true, write: false });
    const all = b.getAudit();
    expect(all.some((e) => e.source === "kernel" && e.pid === 1001)).toBe(true);
    expect(all.some((e) => e.source === "ui-change")).toBe(true);
    // 时间线：seq 升序
    for (let i = 1; i < all.length; i++) expect(all[i]!.seq).toBeGreaterThanOrEqual(all[i - 1]!.seq);
  });
});
