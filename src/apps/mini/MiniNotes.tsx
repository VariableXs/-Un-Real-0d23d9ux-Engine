import { useEffect, useRef, useState } from "react";
import { Pin } from "lucide-react";
import { useI18n } from "../../i18n";
import { pinMiniApp } from "../../system/vwm/miniframe";

/**
 * U-18 迷你应用：便签。
 * - 多色（4 色）：便签底色随选色变化（mini-note-c0..c3）
 * - 置顶（会话内）：调用迷你窗口框架的 pinMiniApp 把本窗口抬到迷你层
 *   最上（会话态，不持久化）
 * - 内容持久化：localStorage 键 variable:mini:notes:v1，每次 Esc/失焦即落盘；
 *   设计取舍：Esc 落盘不阻断冒泡——迷你窗口框架的「Esc 关闭」契约继续生效，
 *   便签在关窗路上先保存（卸载兜底再存一次，双保险）
 * - 「mini-notes」为 registry.ts 中内置注册 id（两侧约定一致）
 */

const NOTES_KEY = "variable:mini:notes:v1";
const NOTES_ID = "mini-notes";
const COLOR_COUNT = 4;

interface NotesData {
  text: string;
  color: number;
}

function loadNotes(): NotesData {
  try {
    const raw = JSON.parse(localStorage.getItem(NOTES_KEY) ?? "null") as NotesData | null;
    if (raw && typeof raw === "object" && typeof raw.text === "string" && typeof raw.color === "number") {
      return { text: raw.text, color: Math.min(COLOR_COUNT - 1, Math.max(0, Math.floor(raw.color))) };
    }
  } catch {
    /* 损坏数据 → 空白便签 */
  }
  return { text: "", color: 0 };
}

export function MiniNotes(): React.ReactElement {
  const { t } = useI18n();
  const [data, setData] = useState<NotesData>(loadNotes);
  const [pinned, setPinned] = useState(false);
  // 卸载兜底落盘用最新值（不随渲染重建）
  const dataRef = useRef(data);
  dataRef.current = data;

  const persist = (): void => {
    try {
      localStorage.setItem(NOTES_KEY, JSON.stringify(dataRef.current));
    } catch {
      /* storage 满/被禁 → 本次不落盘 */
    }
  };

  useEffect(() => persist, []);

  const togglePin = (): void => {
    const next = !pinned;
    setPinned(next);
    pinMiniApp(NOTES_ID, next);
  };

  return (
    <div className={`mini-note mini-note-c${data.color}`}>
      <div className="mini-note-bar">
        <div className="mini-note-swatches" role="group" aria-label={t("miniNotesColor")}>
          {Array.from({ length: COLOR_COUNT }, (_, i) => (
            <button
              key={i}
              type="button"
              className={`mini-note-sw mini-note-sw${i}${data.color === i ? " on" : ""}`}
              aria-label={`${t("miniNotesColor")} ${i + 1}`}
              onClick={() => setData((d) => ({ ...d, color: i }))}
            />
          ))}
        </div>
        <button
          type="button"
          className={`mini-note-pin${pinned ? " on" : ""}`}
          onClick={togglePin}
          aria-label={pinned ? t("miniNotesPinned") : t("miniNotesPin")}
          title={pinned ? t("miniNotesPinned") : t("miniNotesPin")}
        >
          <Pin size={13} />
        </button>
      </div>
      <textarea
        className="mini-note-text"
        value={data.text}
        placeholder={t("miniNotesPlaceholder")}
        aria-label={t("miniNotesTitle")}
        spellCheck={false}
        onChange={(e) => setData((d) => ({ ...d, text: e.target.value }))}
        onBlur={persist}
        onKeyDown={(e) => {
          if (e.key === "Escape") persist(); // 落盘后继续冒泡：框架层 Esc 关窗
        }}
      />
    </div>
  );
}
