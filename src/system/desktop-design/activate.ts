/**
 * AURORA-10000 · AI-11~AI-15 车道 · 副作用激活入口。
 * DesktopShell 仅需 +1 行：`import "../desktop-design/activate";`
 * （AURORA-10000：AI-11~AI-15 批次，勿删）
 *
 * 职责：
 * 1. 令牌运行时（状态 → :root 变量重放）；
 * 2. 设计中心 / 剧场 / 仪式卡三个 overlay 自挂载监听；
 * 3. 健康 / 仪式 / 节令 / 光标四类 runner。
 * 全部本地副作用，零出站；reduced-motion 下装饰层自动停。
 */
import { applyDesignState } from "./runtime";
import { designStore } from "./state";
import { installDesignOverlays } from "./mounts";
import { startAllRunners } from "./runners";

export function activateDesktopDesign(): void {
  if (typeof window === "undefined") return;
  applyDesignState();
  designStore.subscribe(() => applyDesignState());
  installDesignOverlays(window);
  startAllRunners();
  window.dispatchEvent(new CustomEvent("aurora-w2:ready"));
}

activateDesktopDesign();
