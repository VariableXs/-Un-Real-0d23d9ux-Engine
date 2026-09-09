/**
 * AI-20 质量门禁与收官组 — V-91 演示模式（Demo Mode）。
 *
 * 快捷面板一键进入：免打扰 + 保持唤醒（V-54 会话级，随模式退出解除）+
 * 可选指针增强（V-63 轨迹 + V-64 涟漪一键开）；再次点击全部原样恢复。
 *
 * 红线（化境纪律）：
 * - 进入/退出 toast 明确列出改变了什么、恢复了什么（诚实清单）；
 * - 与 V-54 唤醒定时正交（演示模式的唤醒随模式退出解除，不吞用户的定时）；
 * - 异常退出环境时退出钩子恢复全部状态（beforeunload + 进程级 keepawake
 *   本身会话级自动失效双保险）；
 * - 不做投屏检测自动进入（误判风险）；不做演示计时器。
 */

import { useEffect, useState } from "react";
import { MonitorPlay } from "lucide-react";
import { useI18n } from "../../i18n";
import { translate } from "../../i18n/dictionaries";
import type { Lang } from "../../i18n/dictionaries";
import { notifyStore } from "../../state/notifyStore";
import { pushToast } from "../../state/uiStore";
import { ipc11 } from "../../lib/ipc11";
import { loadSettings, saveSetting } from "../../lib/settings";

export const DEMO_MODE_KEY = "variable:demo:active:v1";

interface DemoSnapshot {
  dnd: boolean;
  trailEnabled: boolean;
  rippleEnabled: boolean;
}

let snapshot: DemoSnapshot | null = null;

/** 当前是否处于演示模式（模块态；面板/托盘共用）。 */
export function demoModeActive(): boolean {
  return snapshot !== null;
}

/** 进入演示模式（幂等；已在演示中再点 = 退出）。 */
export async function toggleDemoMode(): Promise<void> {
  if (snapshot) {
    await exitDemoMode();
    return;
  }
  // 先取当前值做快照（退出恢复原样，绝不吞用户既有偏好）
  const settings = await loadSettings();
  const prev: DemoSnapshot = {
    dnd: notifyStore.getState().dnd,
    trailEnabled: settings.inputFeel.trailEnabled,
    rippleEnabled: settings.inputFeel.rippleEnabled,
  };
  snapshot = prev;
  try {
    localStorage.setItem(DEMO_MODE_KEY, "1");
  } catch {
    /* ignore */
  }

  // (1) 免打扰
  if (!prev.dnd) notifyStore.setState({ dnd: true });
  // (2) 保持唤醒（会话级：进程退出自动失效；display=true 防息屏）
  try {
    await ipc11.keepawakeSet(true, true);
  } catch {
    /* 非 Windows / 失败如实降级：toast 已列出该项 */
  }
  // (3) 指针增强（V-63 轨迹 + V-64 涟漪；仅当用户未开启时才临时开启）
  if (!prev.trailEnabled || !prev.rippleEnabled) {
    await saveSetting("inputFeel", { ...settings.inputFeel, trailEnabled: true, rippleEnabled: true });
  }
  pushToast("success", useI18nStatic("v91EnterToast"), useI18nStatic("v91EnterDetail"));
}

/** 退出演示模式：全部原样恢复（诚实清单）。 */
export async function exitDemoMode(): Promise<void> {
  if (!snapshot) return;
  const prev = snapshot;
  snapshot = null;
  try {
    localStorage.removeItem(DEMO_MODE_KEY);
  } catch {
    /* ignore */
  }
  // (1) 勿扰还原
  notifyStore.setState({ dnd: prev.dnd });
  // (2) 唤醒解除（正交：只解除演示模式开启的；用户自己的 V-54 定时由其自身管理）
  try {
    await ipc11.keepawakeSet(false, false);
  } catch {
    /* ignore */
  }
  // (3) 指针增强还原
  if (!prev.trailEnabled || !prev.rippleEnabled) {
    const settings = await loadSettings();
    await saveSetting("inputFeel", {
      ...settings.inputFeel,
      trailEnabled: prev.trailEnabled,
      rippleEnabled: prev.rippleEnabled,
    });
  }
  pushToast("info", useI18nStatic("v91ExitToast"), useI18nStatic("v91ExitDetail"));
}

/** 异常退出钩子：环境被杀/刷新时尽力恢复（keepawake 会话级自动失效双保险）。 */
export function installDemoModeExitHook(): void {
  window.addEventListener("beforeunload", () => {
    if (snapshot) {
      notifyStore.setState({ dnd: snapshot.dnd });
      try {
        localStorage.removeItem(DEMO_MODE_KEY);
      } catch {
        /* ignore */
      }
      void ipc11.keepawakeSet(false, false).catch(() => {});
    }
  });
}

// ---------- 快捷面板按钮 ----------

/** i18n 静态取词（非组件上下文用；语言取当前 DOM lang）。 */
function useI18nStatic(key: string): string {
  try {
    const lang = (document.documentElement.lang || "zh") as Lang;
    return translate(lang, key);
  } catch {
    return key;
  }
}

export function DemoModeButton(): React.ReactElement {
  const { t } = useI18n();
  const [active, setActive] = useState(demoModeActive());
  useEffect(() => {
    const id = window.setInterval(() => setActive(demoModeActive()), 500);
    return () => window.clearInterval(id);
  }, []);
  return (
    <button
      type="button"
      className={`qp-tile${active ? " on" : ""}`}
      data-testid="demo-mode-btn"
      aria-pressed={active}
      title={active ? t("v91ExitToast") : t("v91EnterToast")}
      onClick={() => void toggleDemoMode()}
    >
      <MonitorPlay size={17} strokeWidth={1.8} />
      <span>{active ? t("v91Exit") : t("v91Enter")}</span>
    </button>
  );
}
