/**
 * 车道 E overlay 挂载工具（N-08/N-09/N-12 共用）：
 * 监听 window CustomEvent "ai04:open-feature"（事件契约见 docs/AI04-实施计划.md），
 * detail.feature 命中时动态 import 对应工坊并 createRoot 渲染；工坊组件内部
 * 再 createPortal(document.body) 成 overlay。单例：已打开时忽略重复事件。
 */
import type { ComponentType } from "react";
import { createRoot, type Root } from "react-dom/client";

export interface FeatureOverlayProps {
  onClose: () => void;
}

const open = new Map<string, { root: Root; el: HTMLElement }>();

export function mountFeatureOnEvent(
  feature: string,
  load: () => Promise<{ default: ComponentType<FeatureOverlayProps> }>,
): void {
  if (typeof window === "undefined") return;
  window.addEventListener("ai04:open-feature", (ev: Event) => {
    const detail = (ev as CustomEvent<{ feature?: string }>).detail;
    if (!detail || detail.feature !== feature || open.has(feature)) return;
    void load()
      .then((mod) => {
        if (open.has(feature)) return;
        const Comp = mod.default;
        const el = document.createElement("div");
        el.dataset.laneEOverlay = feature;
        document.body.appendChild(el);
        const root = createRoot(el);
        const onClose = (): void => {
          root.unmount();
          el.remove();
          open.delete(feature);
        };
        root.render(<Comp onClose={onClose} />);
        open.set(feature, { root, el });
      })
      .catch((e) => console.error(`[lane-e] mount ${feature} failed`, e));
  });
}