import { useCallback, useEffect, useRef, useState } from "react";
import { Pin, PinOff, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * F-2.5 剪贴板历史（VWM 虚拟窗口应用）：
 * - 文本 / 图片 / 文件（路径文本）三类，容量上限 500 条
 * - 置顶钉选；跨重启保留（DPAPI 加密落容器 data/tools/clipboard.json）
 * - 轮询系统剪贴板（1.5s；仅在窗口可见时抓取，避免无谓功耗）
 * - 呼出快捷键 ctrl+alt+v（winman "clipboardHistory"，Win+V 被系统占用）
 */

const CAP = 500;
const POLL_MS = 1500;

export interface ClipItem {
  id: string;
  kind: "text" | "image" | "file";
  /** text/file：文本内容；image：dataURL（PNG/BMP）。 */
  data: string;
  /** 预览摘要（图片用尺寸描述）。 */
  preview: string;
  ts: number;
  pinned: boolean;
}

function looksLikePath(s: string): boolean {
  return /^[a-zA-Z]:\\[^"<>|*\r\n]+$/.test(s.trim()) && s.length < 600;
}

export function ClipboardHistoryApp(): React.ReactElement {
  const { t } = useI18n();
  const [items, setItems] = useState<ClipItem[] | null>(null);
  const [filter, setFilter] = useState<"all" | "text" | "image" | "file">("all");
  const lastRef = useRef<string>("");

  useEffect(() => {
    void (async () => {
      try {
        const raw = await ipc.toolSecureRead("clipboard");
        if (raw) {
          const parsed = JSON.parse(raw) as ClipItem[];
          setItems(Array.isArray(parsed) ? parsed : []);
          const first = parsed.find((x) => x.pinned) ?? parsed[0];
          if (first) lastRef.current = first.data.slice(0, 64);
        } else {
          setItems([]);
        }
      } catch {
        // 解密失败（换机/换用户）→ 如实从空开始
        setItems([]);
        pushToast("info", t("clipTitle"), t("clipLoadFail"));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const flush = useCallback(
    (list: ClipItem[]) => {
      void ipc.toolSecureWrite("clipboard", JSON.stringify(list)).catch(() => {});
    },
    [],
  );

  const addItems = useCallback(
    (next: ClipItem[]) => {
      if (next.length === 0) return;
      setItems((cur) => {
        if (!cur) return cur;
        const merged = [...next, ...cur]
          .filter((x, i, arr) => arr.findIndex((y) => y.data === x.data) === i)
          .sort((a, b) => (a.pinned === b.pinned ? b.ts - a.ts : a.pinned ? -1 : 1))
          .slice(0, CAP);
        flush(merged);
        return merged;
      });
    },
    [flush],
  );

  // 轮询系统剪贴板
  useEffect(() => {
    if (items === null) return;
    const id = window.setInterval(() => {
      if (document.hidden) return;
      void (async () => {
        try {
          const read = navigator.clipboard;
          if (!read || !read.read) return;
          const perms = await navigator.permissions
            .query({ name: "clipboard-read" as PermissionName })
            .then((p) => p.state)
            .catch(() => "prompt");
          if (perms === "denied") return;
          const contents = await read.read().catch(() => null);
          if (!contents) return;
          const fresh: ClipItem[] = [];
          for (const item of contents) {
            const textType = item.types.find((ty) => ty === "text/plain");
            const imgType = item.types.find((ty) => ty.startsWith("image/"));
            if (imgType) {
              const blob = await item.getType(imgType);
              if (blob.size > 2_000_000) continue; // 超大图不入历史（容量保护）
              const b64 = await new Promise<string>((res) => {
                const fr = new FileReader();
                fr.onload = () => res(String(fr.result));
                fr.readAsDataURL(blob);
              });
              if (b64.slice(0, 64) !== lastRef.current) {
                lastRef.current = b64.slice(0, 64);
                fresh.push({
                  id: `c${Date.now().toString(36)}`,
                  kind: "image",
                  data: b64,
                  preview: `${blob.type} · ${Math.round(blob.size / 1024)} KB`,
                  ts: Date.now(),
                  pinned: false,
                });
              }
            } else if (textType) {
              const text = await item.getType(textType).then((b) => b.text());
              if (!text || text.slice(0, 64) === lastRef.current) continue;
              lastRef.current = text.slice(0, 64);
              fresh.push({
                id: `c${Date.now().toString(36)}`,
                kind: looksLikePath(text) ? "file" : "text",
                data: text,
                preview: text,
                ts: Date.now(),
                pinned: false,
              });
            }
          }
          addItems(fresh);
        } catch {
          /* 权限/格式不支持 → 静默跳过，如实不抓 */
        }
      })();
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [items === null, addItems]);

  const copyBack = async (x: ClipItem) => {
    try {
      if (x.kind === "image") {
        const blob = await (await fetch(x.data)).blob();
        await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })]);
      } else {
        await navigator.clipboard.writeText(x.data);
      }
      pushToast("success", t("clipTitle"), t("clipCopied"));
    } catch {
      pushToast("error", t("clipTitle"), t("clipCopyFail"));
    }
  };

  const mutate = (id: string, fn: (x: ClipItem) => ClipItem) => {
    setItems((cur) => {
      if (!cur) return cur;
      const next = cur
        .map((x) => (x.id === id ? fn(x) : x))
        .sort((a, b) => (a.pinned === b.pinned ? b.ts - a.ts : a.pinned ? -1 : 1));
      flush(next);
      return next;
    });
  };

  const remove = (id: string) => {
    setItems((cur) => {
      if (!cur) return cur;
      const next = cur.filter((x) => x.id !== id);
      flush(next);
      return next;
    });
  };

  const clearUnpinned = () => {
    setItems((cur) => {
      if (!cur) return cur;
      const next = cur.filter((x) => x.pinned);
      flush(next);
      return next;
    });
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
        <button type="button" className="btn ghost tiny" onClick={clearUnpinned}>{t("clipClear")}</button>
      </div>
      {shown.length === 0 ? (
        <p className="dim small" style={{ padding: 12 }}>{t("clipEmpty")}</p>
      ) : (
        <ul className="clip-list">
          {shown.map((x) => (
            <li key={x.id} className="clip-item">
              {x.kind === "image" ? (
                <img src={x.data} alt={x.preview} className="clip-thumb" />
              ) : (
                <span className={`clip-kind clip-kind-${x.kind}`}>{t(`clipKind_${x.kind}`)}</span>
              )}
              <button type="button" className="clip-body ellipsis small" title={x.preview} onClick={() => void copyBack(x)}>
                {x.kind === "image" ? x.preview : x.data.replace(/\s+/g, " ")}
              </button>
              <span className="dim small">{new Date(x.ts).toLocaleTimeString()}</span>
              <button
                type="button"
                className="icon-btn tiny"
                aria-label={x.pinned ? t("clipUnpin") : t("clipPin")}
                onClick={() => mutate(x.id, (v) => ({ ...v, pinned: !v.pinned }))}
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
