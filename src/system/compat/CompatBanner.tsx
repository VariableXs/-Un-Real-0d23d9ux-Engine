import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../../lib/ipc";
import type { CompatStatus } from "../../lib/ipc";

/**
 * CEF 应用（Wallpaper Engine / Steam）兼容横幅（悬浮在桌面顶部，桌面环境独有）。
 * - 后端在启动/运行期检测 wallpaper64/wallpaperservice/steam/steamwebhelper 进程
 *   （Steam 与 WE 同为 Chromium 系：GPU 竞争与 z-order 抖动同源）；
 * - 命中时自动进入兼容态（alwaysOnTop=false + 壁纸降载），相关进程全部退出
 *   约 30s 后自动恢复；
 * - R7 实机需求（用户硬约束）：横幅与提示 toast 永久静默 —— 兼容缓解全自动
 *   在后端完成，不再以任何形式打扰桌面。组件保留挂载（事件订阅维持状态流），
 *   但恒不渲染。
 */
export function CompatBanner(): React.ReactElement | null {
  const [st, setSt] = useState<CompatStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    // 初次查询（仅维持状态流，不渲染）
    ipc
      .compatCheck()
      .then((s) => {
        if (!cancelled) setSt(s);
      })
      .catch(() => {});

    // 后端 watcher 推送（R7：不再弹横幅，也不再 pushToast）
    const un = listen<CompatStatus>("compat://cef-apps", (e) => {
      setSt(e.payload);
    });
    return () => {
      cancelled = true;
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  // R7：恒不渲染（st 仅用于保持订阅语义，避免未使用告警）
  void st;
  return null;
}
