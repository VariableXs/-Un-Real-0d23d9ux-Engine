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

/** 读标记里的持久快照（被强杀后自愈用）；损坏/旧格式一律视为无快照。 */
function readPersistedSnapshot(): DemoSnapshot | null {
  try {
    const raw = localStorage.getItem(DEMO_MODE_KEY);
    if (!raw) return null;
    const s = JSON.parse(raw) as Partial<DemoSnapshot>;
    if (typeof s.dnd !== "boolean" || typeof s.trailEnabled !== "boolean" || typeof s.rippleEnabled !== "boolean") {
      return null;
    }
    return s as DemoSnapshot;
  } catch {
    return null;
  }
}

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
    localStorage.setItem(DEMO_MODE_KEY, JSON.stringify(prev));
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

/** 按快照尽力恢复（beforeunload / 启动自愈共用；异步链路 fire-and-forget）。 */
async function restoreFromSnapshot(prev: DemoSnapshot): Promise<void> {
  notifyStore.setState({ dnd: prev.dnd });
  try {
    await ipc11.keepawakeSet(false, false);
  } catch {
    /* keepawake 会话级，进程退出自动失效 */
  }
  const settings = await loadSettings();
  await saveSetting("inputFeel", {
    ...settings.inputFeel,
    trailEnabled: prev.trailEnabled,
    rippleEnabled: prev.rippleEnabled,
  });
}

/** 异常退出钩子：环境被杀/刷新时尽力恢复全部状态（keepawake 会话级自动失效双保险；
 *  saveSetting 若未及落盘，标记留存 → 下次启动 recoverDemoModeOnBoot 兜底）。
 *  幂等：重复安装 no-op（挂载点可能反复挂载/卸载）。 */
let exitHookInstalled = false;
export function installDemoModeExitHook(): void {
  if (exitHookInstalled) return;
  exitHookInstalled = true;
  window.addEventListener("beforeunload", () => {
    if (!snapshot) return;
    const prev = snapshot;
    snapshot = null;
    try {
      localStorage.removeItem(DEMO_MODE_KEY);
    } catch {
      /* ignore */
    }
    void restoreFromSnapshot(prev).catch(() => {});
  });
}

/** 启动自愈：上次会话处于演示模式且未能正常退出（进程被杀）时，
 *  从持久标记恢复用户原状并清除标记（幂等；无标记 no-op）。
 *  红线（化境 V-91）：进程被杀场景状态零残留。 */
export async function recoverDemoModeOnBoot(): Promise<void> {
  if (snapshot) return; // 本会话已在演示中（正常重载场景由 beforeunload 处理）
  const prev = readPersistedSnapshot();
  if (!prev) {
    try {
      localStorage.removeItem(DEMO_MODE_KEY); // 损坏数据如实清除
    } catch {
      /* ignore */
    }
    return;
  }
  try {
    localStorage.removeItem(DEMO_MODE_KEY);
  } catch {
    /* ignore */
  }
  await restoreFromSnapshot(prev).catch(() => {});
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
