/**
 * S2.04 · vwm 布局持久化迁移验收（前端零改动 · KV 后端 · AI-3）。
 *
 * 走 vwm.ts **真实代码路径**：persistGeom/loadGeomMap 对裸 `localStorage` 的读写
 * 一行未改，全局 localStorage 由 KV 桥顶替（内核嵌入 realm 的行为模型，见
 * lib/shim/storageHost.ts 契约）。每轮「冷启动」= 模块注册表清空 + 全新镜像桥
 * （同一内核存储）→ 开窗必须逐字节还原上一轮 settleVwmWin 的几何。
 * 数据每轮变化（防镜像偶然命中），终态断言几何 JSON 真实落内核 KV。
 */
import { afterAll, describe, expect, it, vi } from "vitest";
import { initKvStorage, type KvBridge } from "../../../lib/shim/kvBridge";

/** 桩内核 KV（与 kvBridge.spec 同口径：ns 隔离 + kv_* 命令面）。 */
function makeKernelKv() {
  const store = new Map<string, Map<string, string>>();
  const invoke = async (cmd: string, args: Record<string, unknown> = {}): Promise<unknown> => {
    const ns = String(args.ns);
    if (!store.has(ns)) store.set(ns, new Map());
    const bucket = store.get(ns)!;
    switch (cmd) {
      case "kv_keys":
        return [...bucket.keys()];
      case "kv_get":
        return bucket.get(String(args.key)) ?? null;
      case "kv_set":
        bucket.set(String(args.key), String(args.value));
        return undefined;
      case "kv_remove":
        bucket.delete(String(args.key));
        return undefined;
      default:
        throw new Error(`未知命令 ${cmd}`);
    }
  };
  return { invoke, store };
}

const GEOM_KEY = "variable:vwm:geom:v2";
const NS = "varix-desktop";
const WA = { x: 0, y: 48, w: 2400, h: 1300 };

/**
 * 第 round 轮的特征几何（每轮不同 → 还原必须来自持久化而非巧合）。
 * 全部落在 vwm 全局最小尺寸（MIN_W=820/MIN_H=540）之上——真实用户路径
 * 的窗口几何恒 ≥ 该下限（openVwmInstance 二次钳制保证），还原才可能逐字节一致。
 */
function rectOf(round: number) {
  return { x: 60 + round * 13, y: 100 + round * 17, w: 900 + round * 8, h: 600 + round * 6 };
}

afterAll(() => {
  // 卸载安装的全局存储；vitest 文件级隔离兜底其余文件。
  delete (globalThis as { localStorage?: unknown }).localStorage;
});

describe("S2.04 vwm 布局重启还原 ×10（localStorage→KV 透明桥）", () => {
  it(
    "每轮冷启动后 openVwmApp 逐字节还原上轮 settleVwmWin 的几何",
    async () => {
    const kernel = makeKernelKv();
    let lastRect = rectOf(1);
    let bridge: KvBridge | null = null;

    for (let round = 1; round <= 10; round++) {
      // —— 冷启动：模块注册表清空 + 全新镜像桥（同一内核存储）——
      vi.resetModules();
      bridge = await initKvStorage({ origin: NS, invoke: kernel.invoke });
      (globalThis as { localStorage?: unknown }).localStorage = bridge;
      const vwm = await import("../vwm");
      vwm.setVwmWorkArea(WA);

      vwm.openVwmApp("calc");
      const win = vwm.vwmStore.getState().wins[0];
      expect(win, `round ${round} 开窗`).toBeTruthy();

      if (round > 1) {
        // 重启还原：窗口必须落在上一轮 settle 的几何（单实例首开 → 级联偏移 0）。
        expect([win!.x, win!.y, win!.w, win!.h], `round ${round} 还原`).toEqual([
          lastRect.x,
          lastRect.y,
          lastRect.w,
          lastRect.h,
        ]);
      }
      // 本轮用户会话：拖到新几何并 settle（拖拽/缩放结束的持久化点）。
      const rect = rectOf(round);
      vwm.moveVwmWin(win!.id, rect.x, rect.y);
      vwm.resizeVwmWin(win!.id, rect);
      vwm.settleVwmWin(win!.id);
      lastRect = rect;
      // 退出前强同步（flush 对应内核世界退出/切换前的落盘动作）。
      await bridge.flush();
    }

    // 终态：几何 JSON 真实落在内核 KV（不是仅镜像内存）。
    const raw = kernel.store.get(NS)!.get(GEOM_KEY);
    expect(raw, "GEOM_KEY 落内核 KV").toBeTruthy();
    const geom = JSON.parse(raw!) as Record<string, { x: number; y: number; w: number; h: number }>;
    expect(geom.calc).toEqual(lastRect);
    },
    // ×10 轮「模块注册表重置 + 依赖图重导入 + 新建镜像桥」在全力并行下显著慢于单跑。
    30_000,
  );
});
