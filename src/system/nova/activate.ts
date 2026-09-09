/**
 * NOVA-200 · S0 地基 · activate.ts（副作用激活入口，AI-01 路代建）。
 *
 * DesktopShell 仅需 +1 行：`import "./system/nova/activate"`。
 * - nova-hub 中枢监听由 NovaHub.tsx 自挂载（ai04:open-feature 协议，与 SinguHub
 *   同构；动态 import 触发，不占启动包体）；
 * - 本模块只负责运行时的按需启动：任一功能开启 → 动态 import NovaRuntime；
 *   全部关闭 → 运行时保持轻订阅待命（零常驻开销的诚实口径）。
 */

import { novaStats, subscribeNova } from "./registry";

let booted = false;

/** 幂等激活（模块加载即执行一次；导出供测试/热重载显式调用）。 */
export function activateNovaFoundation(): void {
  if (booted || typeof window === "undefined") return;
  booted = true;
  let runtimeBooted = false;
  const bootRuntime = (): void => {
    if (runtimeBooted) return;
    runtimeBooted = true;
    void import("./NovaRuntime")
      .then((m) => m.startNovaRuntime())
      .catch(() => {
        /* 运行时加载失败：诚实跳过（hub 仍可开关 registry 状态） */
        runtimeBooted = false;
      });
  };
  if (novaStats().on > 0) bootRuntime();
  subscribeNova(() => {
    if (novaStats().on > 0) bootRuntime();
  });
}

activateNovaFoundation();
