/**
 * H4 运行时（AI-H4 深化批次 v2 · 桌面窗口接线薄壳）。
 *
 * 生效面声明（诚实边界）：
 * - 开机序列：挂载即执行一次（StrictMode 双挂载由 bootOnce 去重守卫兜住）——
 *   F371 徽标浮层按 BADGE_TIMING 真实时序（300/3000/500ms）淡入停留淡出；
 *   F399 彩蛋①命中时以 toast 呈现（彩蛋是情感不是门——只播动画不改状态）。
 * - 色彩滤镜（F387）：documentElement 单点应用（`--h4-filter` 唯一滤镜位，
 *   auditSingleFilterPoint 的落地面）；变更广播 H4_FILTER_EVENT 驱动即时刷新。
 * - 阅读模式（F386）：变量落 `--h4-reading-*`，文本区域按 CSS 消费；
 *   每应用记忆由引擎持久化，本壳只负责把「当前应用」的态翻译成变量。
 * - 不复制引擎状态、不二次存储；逻辑全部在 h4ui.ts（可测层）。
 */

import React, { useEffect, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import * as grayscale from "../../system/h4/f387-grayscaleMode";
import * as reading from "../../system/h4/f386-readingMode";
import { BADGE_TIMING } from "../../system/h4/f371-bootBadge";
import { defaultStore } from "../../system/h4/internal/store";
import {
  H4_FILTER_EVENT,
  H4_READING_EVENT,
  applyFilterPlan,
  applyReadingVars,
  badgePhaseClass,
  filterPlan,
  readingVarsFor,
  runBootPlan,
  type BootOnceState,
} from "./h4ui";

const BADGE_TOTAL_MS = BADGE_TIMING.fadeInMs + BADGE_TIMING.holdMs + BADGE_TIMING.fadeOutMs;
const BOOT_MS_DEFAULT = 3200; // 桌面壳挂载时刻≈启动链尾段——实测链接入前先按 B-2x 基线记账

export function H4Runtime(): React.ReactElement | null {
  const [badge, setBadge] = useState<{ text: string; shownAt: number } | null>(null);
  const bootOnce = useRef<BootOnceState>({ done: false });
  const badgeTimer = useRef<number>(0);
  const phaseTimer = useRef<number>(0);

  useEffect(() => {
    /* ---------- 开机序列（一次守卫） ---------- */
    const r = runBootPlan(bootOnce.current, defaultStore(), BOOT_MS_DEFAULT);
    bootOnce.current = r.state;
    const plan = r.plan;
    if (plan.badgeVm) {
      const shownAt = Date.now();
      setBadge({ text: plan.badgeVm.text, shownAt });
      phaseTimer.current = window.setInterval(() => {
        setBadge((b) => (b === null ? null : { ...b }));
      }, 100);
      badgeTimer.current = window.setTimeout(() => {
        setBadge(null);
        window.clearInterval(phaseTimer.current);
      }, BADGE_TOTAL_MS);
    }
    if (plan.eggPlay && plan.eggVariant) {
      pushToast("info", "✦ 星徽粒子变奏", `第 100 次开机纪念——彩蛋「${plan.eggVariant}」为你而放（一生一次）。`);
    }

    /* ---------- 滤镜单点应用（挂载即同步 + 广播跟随） ---------- */
    const applyFilter = (): void => {
      applyFilterPlan(document.documentElement, filterPlan(grayscale.activeFilter()));
    };
    applyFilter();

    /* ---------- 阅读模式变量（当前焦点应用；无焦点应用 = 桌面壳自身） ---------- */
    const applyReading = (): void => {
      const appId = (document.activeElement?.closest("[data-app-id]") as HTMLElement | null)?.dataset.appId ?? "desktop";
      const mode = reading.appMode(appId);
      if (!mode.on) {
        applyReadingVars(document.documentElement, null);
        return;
      }
      const style = reading.withSerif(reading.defaultStyle(), mode.serif);
      applyReadingVars(document.documentElement, readingVarsFor(style, 16, document.body.innerText || ""));
    };
    applyReading();

    const onFilterEvent = (): void => applyFilter();
    const onReadingEvent = (): void => applyReading();
    window.addEventListener(H4_FILTER_EVENT, onFilterEvent);
    window.addEventListener(H4_READING_EVENT, onReadingEvent);
    window.addEventListener("focusin", applyReading);
    return () => {
      window.clearTimeout(badgeTimer.current);
      window.clearInterval(phaseTimer.current);
      window.removeEventListener(H4_FILTER_EVENT, onFilterEvent);
      window.removeEventListener(H4_READING_EVENT, onReadingEvent);
      window.removeEventListener("focusin", applyReading);
      // 卸载不清滤镜/阅读变量：滤镜位是全局单点，跟随引擎态而非本组件生命周期
    };
  }, []);

  if (badge === null) return null;
  const phase = badgePhaseClass(Date.now() - badge.shownAt);
  return (
    <div aria-hidden className={`h4-boot-badge ${phase}`} role="status">
      <svg width={14} height={14} viewBox="0 0 24 24" className="h4-boot-badge-star" aria-hidden>
        <path d="M12 2l2.6 6.9L22 9.3l-5.4 4.8L18.2 22 12 17.6 5.8 22l1.6-7.9L2 9.3l7.4-.4z" fill="currentColor" />
      </svg>
      {badge.text}
    </div>
  );
}
