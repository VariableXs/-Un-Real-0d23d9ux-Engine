import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";

/**
 * F-2.2 便签（VWM 虚拟窗口应用）：
 * - 多便签、6 色标记、随手编辑（所见即所得，无保存按钮 —— 改动即冲刷）
 * - 数据落容器 data/tools/notes.json（经 tool_data_read/write）
 * - 开机自动恢复：内容全部持久化，窗口关闭/重开即还原
 */

interface Note {
  id: string;
  color: number;
  text: string;
  updated: number;
}

const COLORS = ["#fde68a", "#bbf7d0", "#bfdbfe", "#fbcfe8", "#ddd6fe", "#fecaca"];
const DEBOUNCE_MS = 500;

export function NotesApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const raw = await ipc.toolDataRead("notes");
        if (!alive) return;
        const list: Note[] = raw ? (JSON.parse(raw) as Note[]) : [];
        setNotes(list);
        if (list.length > 0) setSelected(list[0]!.id);
        else if (raw !== null) setNotes([]);
      } catch {
        setNotes([]);
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const flush = useCallback((list: Note[]) => {
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void ipc.toolDataWrite("notes", JSON.stringify(list)).catch(() => {
        /* 落盘失败不打断编辑；下次改动重试 */
      });
    }, DEBOUNCE_MS);
  }, []);

  const mutate = useCallback(
    (fn: (list: Note[]) => Note[]) => {
      setNotes((cur) => {
        if (!cur) return cur;
        const next = fn(cur);
        flush(next);
        return next;
      });
    },
    [flush],
  );

  const addNote = () => {
    const n: Note = {
      id: `n${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`,
      color: Math.floor(Math.random() * COLORS.length),
      text: "",
      updated: Date.now(),
    };
    mutate((l) => [n, ...l]);
    setSelected(n.id);
  };

  const removeNote = (id: string) => {
    mutate((l) => l.filter((x) => x.id !== id));
    if (selected === id) setSelected(null);
  };

  const updateNote = (id: string, patch: Partial<Note>) => {
    mutate((l) => l.map((x) => (x.id === id ? { ...x, ...patch, updated: Date.now() } : x)));
  };

  if (notes === null) {
    return <div className="notes-app"><p className="dim small">…</p></div>;
  }

  const sel = notes.find((n) => n.id === selected) ?? notes[0] ?? null;

  return (
    <div className="notes-app">
      <div className="notes-toolbar">
        <button type="button" className="btn ghost tiny" onClick={addNote} aria-label={t("notesAdd")}>
          <Plus size={13} /> {t("notesAdd")}
        </button>
        {sel && (
          <div className="row gap4" role="radiogroup" aria-label={t("notesColor")}>
            {COLORS.map((c, i) => (
              <button
                key={c}
                type="button"
                className="notes-swatch"
                style={{ background: c, outline: sel.color === i ? "2px solid var(--accent, #7aa2f7)" : "none" }}
                aria-pressed={sel.color === i}
                aria-label={`${t("notesColor")} ${i + 1}`}
                onClick={() => updateNote(sel.id, { color: i })}
              />
            ))}
          </div>
        )}
        {sel && (
          <button type="button" className="icon-btn tiny danger-hover" onClick={() => removeNote(sel.id)} aria-label={t("notesDelete")}>
            <Trash2 size={13} />
          </button>
        )}
      </div>
      {notes.length === 0 ? (
        <p className="dim small" style={{ padding: 12 }}>{t("notesEmpty")}</p>
      ) : (
        <div className="notes-body">
          <ul className="notes-list" role="listbox" aria-label={t("notesList")}>
            {notes.map((n) => (
              <li key={n.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={sel?.id === n.id}
                  className={`notes-item${sel?.id === n.id ? " active" : ""}`}
                  onClick={() => setSelected(n.id)}
                >
                  <span className="notes-dot" style={{ background: COLORS[n.color] }} />
                  <span className="ellipsis small">{n.text.split("\n")[0] || t("notesUntitled")}</span>
                </button>
              </li>
            ))}
          </ul>
          {sel && (
            <textarea
              className="notes-editor"
              value={sel.text}
              onChange={(e) => updateNote(sel.id, { text: e.target.value })}
              aria-label={t("notesEditor")}
              placeholder={t("notesPlaceholder")}
            />
          )}
        </div>
      )}
    </div>
  );
}
