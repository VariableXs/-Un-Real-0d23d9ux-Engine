import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import { askConfirm } from "../../components/Modal";

/**
 * AI-01 Z-42 桌面切换预览（Desktop Switcher +）：
 * - 右缘热区（宽度 settings.desktopHotzone，0 = 关闭）或 Ctrl+Alt+G 呼出
 * - 预览卡横排：环境名 + 首个应用图标位（简渲染，不做实时缩略图——克制）
 * - 170ms 横移动效（对齐既有动效令牌）；全部行为可关闭
 * - 切换前确认当前环境未保存工作（与 EnvsTab 同一口径：envSwitch 自带快照）
 */

interface EnvCard {
  id: string;
  name: string;
  active: boolean;
}

export function DesktopSwitcher(props: { open: boolean; onClose: () => void; settings: Settings }): React.ReactElement | null {
  const { t } = useI18n();
  const [envs, setEnvs] = useState<EnvCard[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!props.open || !isTauriRuntime()) return;
    let alive = true;
    ipc
      .envList()
      .then((list: unknown) => {
        if (!alive || !Array.isArray(list)) return;
        setEnvs(
          (list as Array<{ id: string; name?: string; active?: boolean }>).map((e) => ({
            id: e.id,
            name: e.name ?? e.id,
            active: e.active === true,
          })),
        );
      })
      .catch(() => setEnvs([]));
    return () => {
      alive = false;
    };
  }, [props.open]);

  useEffect(() => {
    if (!props.open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.preventDefault();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props]);

  if (!props.open) return null;

  const switchTo = (id: string): void => {
    if (busy) return;
    void (async () => {
      const target = envs.find((e) => e.id === id);
      if (target?.active) {
        props.onClose();
        return;
      }
      const ok = await askConfirm({ title: t("wfSwitchTitle"), body: t("wfSwitchBody"), danger: false });
      if (!ok) return;
      setBusy(true);
      try {
        await ipc.envSwitch(id);
        props.onClose();
      } catch (e) {
        pushToast("error", t("wfSwitchTitle"), errMessage(e).message ?? String(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div className="wf-switcher-overlay" role="dialog" aria-label={t("wfSwitcherTitle")} onClick={props.onClose}>
      <div className="wf-switcher-panel" onClick={(e) => e.stopPropagation()}>
        <div className="wf-switcher-cap">{t("wfSwitcherTitle")}</div>
        <div className="wf-switcher-row">
          {envs.length === 0 && <div className="wf-drawer-empty">{t("wfSwitcherEmpty")}</div>}
          {envs.map((e, i) => (
            <button
              key={e.id}
              type="button"
              className={`wf-switcher-card${e.active ? " active" : ""}`}
              style={{ animationDelay: `${i * 20}ms` }}
              onClick={() => switchTo(e.id)}
              disabled={busy}
            >
              <span className="wf-switcher-thumb" aria-hidden>
                <span className="vwm-app-dot" />
              </span>
              <span className="wf-switcher-name">{e.name}</span>
              {e.active && <span className="wf-switcher-active-tag">{t("wfSwitcherActive")}</span>}
            </button>
          ))}
        </div>
        <div className="wf-switcher-hint">{t("wfSwitcherHint")}</div>
      </div>
    </div>
  );
}

/** Z-42：右缘热区（hover 达标回调）。宽度 0 = 关闭。 */
export function DesktopHotzone(props: { width: number; onTrigger: () => void }): React.ReactElement | null {
  const [timer, setTimer] = useState<number | null>(null);
  if (props.width <= 0) return null;
  return (
    <div
      className="wf-hotzone"
      style={{ width: props.width }}
      onMouseEnter={() => {
        const id = window.setTimeout(props.onTrigger, 400);
        setTimer(id);
      }}
      onMouseLeave={() => {
        if (timer !== null) window.clearTimeout(timer);
        setTimer(null);
      }}
    />
  );
}
