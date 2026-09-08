/**
 * AI-16 通知运行时（挂载于 DesktopShell，零 UI）：
 * - Z-49：接住后端 "reminder://due"（系统时钟锚定），推入通知中心（reminder 类豁免勿扰）；
 * - Z-44：每分钟按 settings 勿扰日程刷新 schedDnd（手动 ∪ 日程 = 生效勿扰；
 *   日程结束自动触发回放，见 notifyStore.setSchedDnd）；
 * - Z-47：启动时按保留策略执行一次存档清理（retention 0 = 永久保留，跳过）。
 */
import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { loadSettings } from "../../lib/settings";
import { isDndActive } from "../../lib/dndSchedule";
import { pushNotify, setSchedDnd } from "../../state/notifyStore";
import { ipc } from "../../lib/ipc";

/** 提醒到期事件载荷（soundnotify.rs spawn_reminder_runtime）。 */
interface ReminderDuePayload {
  id: string;
  text: string;
  category: string;
  repeat: string;
  dueAt: number;
}

export function NotifyRuntime(): null {
  const { t } = useI18n();
  useEffect(() => {
    const unlistenP = listen<ReminderDuePayload>("reminder://due", (ev) => {
      const p = ev.payload;
      // reminder 类在 pushNotify 内豁免勿扰（Z-44/Z-49 联动：约定必达）
      pushNotify("reminder", p.text, "", [
        { label: t("snRemindAck"), type: "dismiss" },
      ], { source: "reminder", exempt: true });
    });

    let stopped = false;
    let unlisten: UnlistenFn | null = null;
    void unlistenP.then((f) => {
      if (stopped) f();
      else unlisten = f;
    }).catch(() => {});

    // 每分钟：日程勿扰刷新 + 保留策略到期清理（幂等）
    async function tick(): Promise<void> {
      try {
        const s = await loadSettings();
        setSchedDnd(
          isDndActive(false, {
            enabled: s.dndScheduleEnabled,
            start: s.dndScheduleStart,
            end: s.dndScheduleEnd,
          }),
        );
        if (s.notifyRetentionDays > 0) {
          await ipc.notifyArchiveCleanup(s.notifyRetentionDays).catch(() => {});
        }
      } catch {
        /* 设置读取失败：保持现状 */
      }
    }
    void tick();
    const timer = window.setInterval(() => void tick(), 60_000);

    return () => {
      stopped = true;
      unlisten?.();
      void unlistenP.then((f) => f()).catch(() => {});
      window.clearInterval(timer);
    };
  }, [t]);

  return null;
}
