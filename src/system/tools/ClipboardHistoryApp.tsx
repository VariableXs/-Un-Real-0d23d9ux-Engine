import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Pin, PinOff, Settings2, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { Shell } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * AI-07 · N-15 剪贴板历史中心（前端）：
 * - 后端 Win32 序列号看护线程录制（文本/文件/图片 DIB），DPAPI 加密落盘；
 * - 敏感应用名单期间零记录；「关闭即焚」模式下卸载面板即焚毁（含落盘）；
 * - 新条目经 `cliphist://changed` 事件推送（前端不再轮询 navigator.clipboard）；
 * - 写回（回贴）走 `cliphist_write_back`，直接 SetClipboardData。
 */

export function ClipboardHistoryApp(): React.ReactElement {
  const { t } = useI18n();
  const [items, setItems] = useState<Shell.ClipEntry[] | null>(null);
  const [filter, setFilter] = useState<"all" | "text" | "image" | "file">("all");
  const [showSettings, setShowSettings] = useState(false);
  const [enabled, setEnabled] = useState(true);
  const [sensitive, setSensitive] = useState("");
  const [burn, setBurn] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setItems(await ipc.cliphistList());
    } catch {
      setItems([]);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      await refresh();
      try {
        const cfg = await ipc.cliphistConfigGet();
        setEnabled(cfg.enabled);
        setSensitive(cfg.sensitiveApps.join(", "));
        setBurn(cfg.burnOnClose);
      } catch {
        /* 非 Tauri 环境：保持默认 */
      }
    })();
  }, [refresh]);

  // 后端事件驱动：新条目 / 超大图拒存
  useEffect(() => {
    let un1: (() => void) | undefined;
    let un2: (() => void) | undefined;
    void (async () => {
      try {
        un1 = await listen("cliphist://changed", () => void refresh());
        un2 = await listen("cliphist://rejected-image", () =>
          pushToast("info", t("clipTitle"), t("clipImageRejected")),
        );
      } catch {
        /* 非 Tauri 环境无事件 */
      }
    })();
    return () => {
      un1?.();
      un2?.();
    };
  }, [refresh, t]);

  // 关闭即焚：面板卸载时焚毁
  useEffect(
    () => () => {
      void ipc.cliphistBurn().catch(() => {});
    },
    [],
  );

  const saveConfig = (next: Partial<Shell.ClipConfig>) => {
    const cfg: Shell.ClipConfig = {
      enabled,
      sensitiveApps: sensitive
        .split(",")
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean),
      burnOnClose: burn,
      ...next,
    };
    setEnabled(cfg.enabled);
    setBurn(cfg.burnOnClose);
    void ipc.cliphistConfigSet(cfg).catch(() => {});
  };

  const copyBack = async (x: Shell.ClipEntry) => {
    try {
      const ok = await ipc.cliphistWriteBack(x.id);
      if (ok) {
        pushToast("success", t("clipTitle"), t("clipCopied"));
      } else {
        // 后端写回不可用（非 Windows）→ 浏览器通道兜底（仅文本）
        if (x.kind !== "image") {
          await navigator.clipboard.writeText(x.data);
          pushToast("success", t("clipTitle"), t("clipCopied"));
        } else {
          pushToast("error", t("clipTitle"), t("clipCopyFail"));
        }
      }
    } catch {
      pushToast("error", t("clipTitle"), t("clipCopyFail"));
    }
  };

  const pin = (id: string, pinned: boolean) => {
    setItems((cur) => (cur ? cur.map((x) => (x.id === id ? { ...x, pinned } : x)) : cur));
    void ipc.cliphistPin(id, pinned).catch(() => {});
  };

  const remove = (id: string) => {
    setItems((cur) => (cur ? cur.filter((x) => x.id !== id) : cur));
    void ipc.cliphistRemove(id).catch(() => {});
  };

  const clearUnpinned = () => {
    setItems((cur) => (cur ? cur.filter((x) => x.pinned) : cur));
    void ipc.cliphistClear(true).catch(() => {});
  };

  if (items === null) return <div className="clip-app"><p className="dim small">…</p></div>;

  const shown = items.filter((x) => filter === "all" || x.kind === filter);

  return (
    <div className="clip-app">
      <div className="notes-toolbar" role="radiogroup" aria-label={t("clipFilter")}>
        {(["all", "text", "image", "file"] as const).map((f) => (
          <button key={f} type="button" className={`btn ghost tiny${filter === f ? " active" : ""}`} aria-pressed={filter === f} onClick={() => setFilter(f)}>
            {t(`clipKind_${f}`)}
          </button>
        ))}
        <span className="flex-1" />
        <button
          type="button"
          className="icon-btn tiny"
          aria-pressed={showSettings}
          aria-label={t("clipSettings")}
          onClick={() => setShowSettings((v) => !v)}
        >
          <Settings2 size={12} />
        </button>
        <button type="button" className="btn ghost tiny" onClick={clearUnpinned}>{t("clipClear")}</button>
      </div>

      {showSettings && (
        <div className="clip-settings" style={{ padding: 12, borderBottom: "1px solid var(--line)" }}>
          <label className="flex align-center gap8 small">
            <input type="checkbox" checked={enabled} onChange={(e) => saveConfig({ enabled: e.target.checked })} />
            {t("clipRecordOn")}
          </label>
          <label className="flex align-center gap8 small" style={{ marginTop: 8 }}>
            <input type="checkbox" checked={burn} onChange={(e) => saveConfig({ burnOnClose: e.target.checked })} />
            {t("clipBurn")}
          </label>
          <label className="small" style={{ marginTop: 8, display: "block" }}>
            {t("clipSensitive")}
            <input
              type="text"
              className="input tiny"
              style={{ marginTop: 4 }}
              value={sensitive}
              placeholder={t("clipSensitiveHint")}
              onChange={(e) => setSensitive(e.target.value)}
              onBlur={() => saveConfig({})}
            />
          </label>
        </div>
      )}

      {shown.length === 0 ? (
        <p className="dim small" style={{ padding: 12 }}>{t("clipEmpty")}</p>
      ) : (
        <ul className="clip-list">
          {shown.map((x) => (
            <li key={x.id} className="clip-item">
              {x.kind === "image" ? (
                <span className="clip-kind clip-kind-image">{t("clipKind_image")}</span>
              ) : (
                <span className={`clip-kind clip-kind-${x.kind}`}>{t(`clipKind_${x.kind}`)}</span>
              )}
              <button type="button" className="clip-body ellipsis small" title={x.preview} onClick={() => void copyBack(x)}>
                {x.preview.replace(/\s+/g, " ")}
              </button>
              <span className="dim small">{new Date(x.ts).toLocaleTimeString()}</span>
              <button
                type="button"
                className="icon-btn tiny"
                aria-label={x.pinned ? t("clipUnpin") : t("clipPin")}
                onClick={() => pin(x.id, !x.pinned)}
              >
                {x.pinned ? <PinOff size={12} /> : <Pin size={12} />}
              </button>
              <button type="button" className="icon-btn tiny danger-hover" aria-label={t("notesDelete")} onClick={() => remove(x.id)}>
                <Trash2 size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
