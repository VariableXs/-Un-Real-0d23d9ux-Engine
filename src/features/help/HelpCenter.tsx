/**
 * AI-17 · Z-70 帮助中心（Help Center）运行时
 * F1 = 当前界面的上下文帮助；面板内全文搜索 + 主题浏览；Esc 关闭。
 * 由 VisionRuntime 挂载（仅桌面窗口）。
 */
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { searchHelp, topicForContext, defaultTopic, HELP_TOPICS, type HelpTopic } from "./helpTopics";
import { resolveIcon } from "../../lib/iconRegistry";

export function HelpCenter(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [contextTopic, setContextTopic] = useState<HelpTopic | null>(null);
  const Close = resolveIcon("close");
  const Search = resolveIcon("search");

  // Esc 关闭（浮层栈顶消费）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  // F1 上下文帮助：按 data-help-context 命中主题直接打开
  useEffect(() => {
    const onF1 = (e: KeyboardEvent): void => {
      if (e.key !== "F1") return;
      e.preventDefault();
      const active = document.activeElement as HTMLElement | null;
      const ctxEl = active?.closest("[data-help-context]") as HTMLElement | null;
      const ctx = ctxEl?.dataset.helpContext ?? document.body.dataset.helpContext ?? "";
      setContextTopic(topicForContext(ctx));
      setQuery("");
      setOpen((v) => !v);
    };
    window.addEventListener("keydown", onF1);
    return () => window.removeEventListener("keydown", onF1);
  }, []);

  const results = useMemo(() => (query.trim() ? searchHelp(query) : []), [query]);
  const locale = lang === "en" ? "en" : "zh";
  const shown: HelpTopic | null = query.trim()
    ? (results[0]?.topic ?? null)
    : (contextTopic ?? defaultTopic(locale));

  if (!open) return null;

  return (
    <div className="help-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) setOpen(false); }}>
      <div className="help-panel" role="dialog" aria-label={t("helpCenterTitle")}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Search size={18} />
          <input
            className="help-search"
            style={{ flex: 1 }}
            placeholder={t("helpSearchPlaceholder")}
            value={query}
            autoFocus
            onChange={(e) => setQuery(e.target.value)}
          />
          <button type="button" className="mi-ctl" onClick={() => setOpen(false)} aria-label={t("close")}>
            <Close size={16} />
          </button>
        </div>
        <div style={{ overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
          {shown ? (
            <div className="help-topic">
              <div className="help-topic-title">{shown[locale].title}</div>
              <div className="help-topic-body">{shown[locale].body}</div>
            </div>
          ) : (
            <div className="help-topic">
              <div className="help-topic-body">{t("helpNoResults")}</div>
            </div>
          )}
          {!query.trim() && contextTopic === null ? (
            <>
              <div className="dim" style={{ fontSize: "var(--fs-12)" }}>{t("helpBrowseHint")}</div>
              {HELP_TOPICS.map((topic) => (
                <div key={topic.id} className="help-topic" onClick={() => setContextTopic(topic)}>
                  <div className="help-topic-title">{topic[locale].title}</div>
                </div>
              ))}
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
