/**
 * S2.04 · KV 存储宿主安装器（AI-3 · 总案阶段3步骤4：vwmStore 持久化迁移）。
 *
 * 总案原文：「几何持久化从 localStorage 切到 KV 服务，前端代码零改动（在垫片内做
 * localStorage→KV 透明桥）」。本模块就是"垫片内"的那个安装点：
 *   - 内核侧（嵌入 realm）：能力位 kvStore=true → 经 kvBridge 建立镜像桥，
 *     并把它安装为 globalThis.localStorage —— 之后 vwm.ts 等既有代码对裸
 *     `localStorage` 的全部读写**零改动**落内核 KV；
 *   - Windows 侧：能力位不可用（未握手/无 kvStore）→ 不安装，原生 localStorage
 *     原样保留（零行为变化，这就是迁移的回退路径）；
 *   - 宿主 realm 的 localStorage 不可替换（真浏览器 [LegacyUnforgeable]）→
 *     **如实返回 native-unforgeable**，绝不伪造成功；此时嵌入层应改走
 *     Servo 存储后端接线（内核侧由嵌入层持有本模块返回的 bridge 注入）。
 *
 * 嵌入层契约（与 embedderTransport.ts 的 __VARIX_SHIM__ 同族）：
 *   内核嵌入方在应用 bundle 求值前调用 installKvStorageHost({ origin })；
 *   origin → ns 规则与 kvBridge 一致（≤16B，确定性，见 originToNamespace）。
 *
 * 安装语义：只覆盖"缺席或可配置"的 localStorage 属性描述符；存在且不可配置
 * （真实浏览器语义）时不强行覆盖、不静默吞掉——返回原因由调用方决策。
 */
import { initKvStorage, type KvBridge } from "./kvBridge";
import { shimCapability } from "./shimInvoke";

/** 未安装原因（调用方据此走各自的回退面，绝不静默）。 */
export type StorageSkipReason =
  /** 能力位缺失（Windows 侧/未握手）——保持原生 localStorage，行为等价。 */
  | "kv-unavailable"
  /** KV 预热失败（内核 KV 通道异常）——调用方决定回退原生或重试。 */
  | "kv-init-failed"
  /** 宿主 realm 的 localStorage 不可替换——bridge 仍在返回值里，供嵌入层另行注入。 */
  | "native-unforgeable";

export type StorageInstallResult =
  | { installed: true; storage: KvBridge; namespace: string }
  | { installed: false; reason: StorageSkipReason; storage: KvBridge | null };

export interface KvStorageHostOptions {
  /** 前端 origin（→ 内核命名空间，≤16B）。 */
  origin: string;
  /** 垫片 invoke（默认 shimInvoke；测试注入桩）。 */
  invoke?: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  /** 能力位查询（默认 shimCapability("kvStore")；测试注入）。 */
  kvAvailable?: () => boolean;
  /** 异步持久化失败回调（透传 kvBridge，绝不静默丢写）。 */
  onError?: (key: string, err: unknown) => void;
}

/**
 * 安装 KV 存储宿主：建桥 → 顶替全局 localStorage（仅当缺席或可配置）。
 * 永不抛异常——任何失败都归约为 installed:false + 原因，调用方面决定回退。
 */
export async function installKvStorageHost(opts: KvStorageHostOptions): Promise<StorageInstallResult> {
  const available = opts.kvAvailable ?? (() => shimCapability("kvStore"));
  if (!available()) {
    return { installed: false, reason: "kv-unavailable", storage: null };
  }

  let bridge: KvBridge;
  try {
    bridge = await initKvStorage({
      origin: opts.origin,
      invoke: opts.invoke,
      // 安装器只在能力位确认后才走到这里；桥内恒走 KV 路径。
      kvAvailable: () => true,
      nativeFallback: null,
      onError: opts.onError,
    });
  } catch {
    return { installed: false, reason: "kv-init-failed", storage: null };
  }

  const g = globalThis as Record<string, unknown>;
  const desc = Object.getOwnPropertyDescriptor(g, "localStorage");
  if (desc && !desc.configurable) {
    // 真实浏览器语义（[LegacyUnforgeable]）：如实报告，不伪造安装成功。
    return { installed: false, reason: "native-unforgeable", storage: bridge };
  }
  try {
    Object.defineProperty(g, "localStorage", {
      value: bridge,
      writable: true,
      configurable: true,
      enumerable: true,
    });
  } catch {
    return { installed: false, reason: "native-unforgeable", storage: bridge };
  }
  return { installed: true, storage: bridge, namespace: bridge.namespace };
}

/** 仅供测试：卸载安装的全局存储（属性不可删时静默忽略——测试 realm 均可删）。 */
export function uninstallKvStorageHostForTest(): void {
  const g = globalThis as Record<string, unknown>;
  const desc = Object.getOwnPropertyDescriptor(g, "localStorage");
  if (desc?.configurable) delete g.localStorage;
}
