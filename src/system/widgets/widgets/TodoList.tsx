/** N-09 待办：本地 localStorage 清单（第三方 widget 可经 todo:read 权限只读）。 */
import { useState } from "react";
import { Check, Plus, X } from "lucide-react";
import { LABELS, useLaneLang } from "../labels";
import { loadTodos, saveTodos, type WgtTodoItem } from "./common";

export default function TodoList({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [items, setItems] = useState<WgtTodoItem[]>(loadTodos);
  const [draft, setDraft] = useState("");
  void paused; // 待办无轮询；gate 仅影响刷新型组件

  const mutate = (next: WgtTodoItem[]): void => {
    setItems(next);
    saveTodos(next);
  };

  return (
    <div className="wgt-todo">
      <form
        className="wgt-todo-form"
        onSubmit={(e) => {
          e.preventDefault();
          const text = draft.trim();
          if (!text) return;
          mutate([...items, { id: Date.now(), text, done: false }]);
          setDraft("");
        }}
      >
        <input className="wgt-input" value={draft} placeholder={t.addTodo} onChange={(e) => setDraft(e.target.value)} />
        <button type="submit" className="wgt-add" aria-label="add"><Plus size={12} /></button>
      </form>
      <div className="wgt-todo-list">
        {items.length === 0 && <div className="wgt-dim">{t.noTodos}</div>}
        {items.slice(-6).map((it) => (
          <div key={it.id} className={`wgt-todo-row ${it.done ? "done" : ""}`}>
            <button
              type="button"
              className={`wgt-check ${it.done ? "on" : ""}`}
              aria-label="toggle"
              onClick={() => mutate(items.map((x) => (x.id === it.id ? { ...x, done: !x.done } : x)))}
            >
              {it.done && <Check size={10} />}
            </button>
            <span className="wgt-todo-text">{it.text}</span>
            <button type="button" className="wgt-x" aria-label="remove" onClick={() => mutate(items.filter((x) => x.id !== it.id))}>
              <X size={11} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}