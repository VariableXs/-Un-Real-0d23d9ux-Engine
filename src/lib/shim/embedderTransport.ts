/**
 * Servo 嵌入方传输适配（任务 27 前端侧缺口件；实机渲染待 Servo 用户态进程就位）。
 *
 * 契约：Servo 嵌入层（内核用户态）在全局注入 `__VARIX_SHIM__` 桥：
 *   `globalThis.__VARIX_SHIM__(cmd: string, args: object) => Promise<unknown>`
 * 返回值即垫片协议传输形态（载荷直达 | __shim_error | __shim_missing），
 * 解码统一由 shimInvoke 完成——本模块只负责"找到桥并接上"，零协议逻辑。
 * Windows 侧不走此文件（Tauri 传输由 ipc.ts 承担）。
 */
import { installShimTransport } from "./shimInvoke";

/** 嵌入方注入桥的全局名（VARIX 嵌入层契约，Servo 侧注册时使用同一名字）。 */
export const VARIX_SHIM_GLOBAL = "__VARIX_SHIM__";

type EmbedderBridge = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

/** 读取嵌入方桥；未注入返回 null（不猜测、不伪造）。 */
export function findEmbedderBridge(): EmbedderBridge | null {
  const g = globalThis as Record<string, unknown>;
  const candidate = g[VARIX_SHIM_GLOBAL];
  if (typeof candidate === "function") return candidate as EmbedderBridge;
  return null;
}

/**
 * 若嵌入方桥存在则安装垫片传输层；返回是否安装成功。
 * 桥不存在（如 Windows 侧/早于嵌入注入）→ 返回 false，调用方继续走既有 Tauri 传输。
 */
export function installEmbedderTransportIfPresent(): boolean {
  const bridge = findEmbedderBridge();
  if (!bridge) return false;
  installShimTransport(async (cmd, args) => bridge(cmd, args));
  return true;
}
