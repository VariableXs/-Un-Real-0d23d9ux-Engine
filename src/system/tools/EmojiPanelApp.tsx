import { useCallback, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { pushToast } from "../../state/uiStore";
import {
  EMOJI_CATEGORIES,
  SYMBOL_GROUPS,
  readRecent,
  searchEmoji,
  writeRecent,
  type EmojiItem,
} from "./emojiData";
import "../../styles/ai08-emoji.css";

/**
 * Z-24 字符与 Emoji 面板（VWM 虚拟窗口应用，AI-08 基础工具组）：
 * - 中英双语搜索 / 九大分类侧栏 / emoji 网格 / 最近使用（上限 24）
 * - 常用符号区：数学 / 货币 / 箭头 / 制表 四组子标签
 * - 点击复制到剪贴板 + toast 反馈 + 写入最近使用
 * - 全键盘可达：网格内方向键移动、Enter 复制
 * 红线：只做字符复制，不解析剪贴板、不监听输入法、不做联网扩充。
 */

/** 本地三语词条（键与 dictionaries.ts 计划键一致，缺失时兜底显示）。 */
const EM_LABELS: Record<string, Record<Lang, string>> = {
  emTitle: { zh: "字符与 Emoji", "zh-TW": "字元與 Emoji", en: "Emoji & Symbols" },
  emSearch: { zh: "搜索 emoji（中 / 英）", "zh-TW": "搜尋 emoji（中 / 英）", en: "Search emoji (zh / en)" },
  emRecent: { zh: "最近使用", "zh-TW": "最近使用", en: "Recent" },
  emCopied: { zh: "已复制", "zh-TW": "已複製", en: "Copied" },
  emCopyFail: { zh: "复制失败", "zh-TW": "複製失敗", en: "Copy failed" },
  emNoResult: { zh: "无匹配结果", "zh-TW": "無匹配結果", en: "No matches" },
  emSymbols: { zh: "常用符号", "zh-TW": "常用符號", en: "Symbols" },
  emSearchResult: { zh: "搜索结果", "zh-TW": "搜尋結果", en: "Results" },
  emKbdHint: { zh: "方向键移动 · Enter 复制", "zh-TW": "方向鍵移動 · Enter 複製", en: "Arrows to move · Enter to copy" },
};

/** 分类网格固定 10 列（键盘纵向步进与 CSS grid 保持一致）。 */
const GRID_COLS = 10;
/** 最近使用行固定 12 列。 */
const RECENT_COLS = 12;

export function EmojiPanelApp(_props: { winId: string }): React.ReactElement {
  const { lang, t } = useI18n();
  const tt = useCallback((key: string) => EM_LABELS[key]?.[lang] ?? t(key), [lang, t]);

  const [query, setQuery] = useState("");
  const [activeCat, setActiveCat] = useState<string>(EMOJI_CATEGORIES[0]!.id);
  const [activeSym, setActiveSym] = useState<(typeof SYMBOL_GROUPS)[number]["id"]>("math");
  const [recent, setRecent] = useState<string[]>(() => readRecent());

  // ch → 条目反查（最近使用区 title 提示用）
  const chToItem = useMemo(() => {
    const m = new Map<string, EmojiItem>();
    for (const c of EMOJI_CATEGORIES) for (const it of c.items) m.set(it.ch, it);
    for (const g of SYMBOL_GROUPS) for (const it of g.items) m.set(it.ch, it);
    return m;
  }, []);

  const searching = query.trim().length > 0;
  const hits = useMemo(() => (searching ? searchEmoji(query, lang) : []), [query, lang, searching]);

  const pick = useCallback(
    (ch: string, label: string): void => {
      void navigator.clipboard
        .writeText(ch)
        .then(() => {
          setRecent(writeRecent(ch));
          pushToast("success", tt("emTitle"), `${ch} ${label} · ${tt("emCopied")}`);
        })
        .catch(() => pushToast("error", tt("emTitle"), tt("emCopyFail")));
    },
    [tt],
  );

  /** 网格键盘导航：方向键在按钮间移动（Enter 走 button 原生 click）。 */
  const onGridKey = (
    e: React.KeyboardEvent<HTMLDivElement>,
    total: number,
    cols: number,
  ): void => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) return;
    const btn = e.target as HTMLElement;
    const idxRaw = btn.dataset?.idx;
    if (idxRaw === undefined) return;
    const idx = Number(idxRaw);
    if (Number.isNaN(idx)) return;
    e.preventDefault();
    let next = idx;
    if (e.key === "ArrowLeft") next = idx - 1;
    else if (e.key === "ArrowRight") next = idx + 1;
    else if (e.key === "ArrowDown") next = idx + cols;
    else next = idx - cols;
    if (next < 0 || next >= total) return;
    e.currentTarget.querySelector<HTMLButtonElement>(`button[data-idx="${next}"]`)?.focus();
  };

  const renderItem = (item: EmojiItem, i: number, isSymbol: boolean): React.ReactElement => (
    <button
      key={`${item.ch}-${i}`}
      type="button"
      className={`em-item${isSymbol ? " is-symbol" : ""}`}
      data-idx={i}
      title={`${item.zh} · ${item.en}`}
      aria-label={`${item.zh} ${item.en}`}
      onClick={() => pick(item.ch, item.zh)}
    >
      {item.ch}
    </button>
  );

  const cat = EMOJI_CATEGORIES.find((c) => c.id === activeCat) ?? EMOJI_CATEGORIES[0]!;
  const symGroup = SYMBOL_GROUPS.find((g) => g.id === activeSym) ?? SYMBOL_GROUPS[0]!;

  return (
    <div className="em-app" role="application" aria-label={tt("emTitle")}>
      <div className="em-bar">
        <input
          className="text-input em-search"
          type="text"
          value={query}
          placeholder={tt("emSearch")}
          onChange={(e) => setQuery(e.target.value)}
          aria-label={tt("emSearch")}
          autoFocus
        />
      </div>

      <div className="em-body">
        <nav className="em-cats" aria-label={tt("emTitle")}>
          {EMOJI_CATEGORIES.map((c) => (
            <button
              key={c.id}
              type="button"
              className={`em-cat${!searching && activeCat === c.id ? " active" : ""}`}
              onClick={() => {
                setActiveCat(c.id);
                setQuery("");
              }}
            >
              <span className="em-cat-icon">{c.icon}</span>
              <span className="em-cat-label">{lang === "en" ? c.en : c.zh}</span>
            </button>
          ))}
          <div className="em-cats-divider" />
          <button
            type="button"
            className={`em-cat${!searching && activeCat === "symbols" ? " active" : ""}`}
            onClick={() => {
              setActiveCat("symbols");
              setQuery("");
            }}
          >
            <span className="em-cat-icon">∑</span>
            <span className="em-cat-label">{tt("emSymbols")}</span>
          </button>
        </nav>

        <div className="em-main">
          {searching ? (
            <div className="em-section">
              <div className="em-section-title">
                {tt("emSearchResult")} · {hits.length}
              </div>
              {hits.length === 0 ? (
                <p className="em-empty">{tt("emNoResult")}</p>
              ) : (
                <div
                  className="em-grid"
                  role="listbox"
                  aria-label={tt("emSearchResult")}
                  onKeyDown={(e) => onGridKey(e, hits.length, GRID_COLS)}
                >
                  {hits.map((item, i) => renderItem(item, i, item.ch.length === 1 && item.ch.charCodeAt(0) < 0x3000))}
                </div>
              )}
            </div>
          ) : activeCat === "symbols" ? (
            <div className="em-section">
              <div className="em-sym-tabs" role="tablist">
                {SYMBOL_GROUPS.map((g) => (
                  <button
                    key={g.id}
                    type="button"
                    role="tab"
                    aria-selected={activeSym === g.id}
                    className={`em-sym-tab${activeSym === g.id ? " active" : ""}`}
                    onClick={() => setActiveSym(g.id)}
                  >
                    {lang === "en" ? g.en : g.zh}
                  </button>
                ))}
              </div>
              <div
                className="em-grid"
                role="listbox"
                aria-label={lang === "en" ? symGroup.en : symGroup.zh}
                onKeyDown={(e) => onGridKey(e, symGroup.items.length, GRID_COLS)}
              >
                {symGroup.items.map((item, i) => renderItem(item, i, true))}
              </div>
            </div>
          ) : (
            <>
              {recent.length > 0 && (
                <div className="em-section">
                  <div className="em-section-title">{tt("emRecent")}</div>
                  <div
                    className="em-grid em-recent-grid"
                    role="listbox"
                    aria-label={tt("emRecent")}
                    onKeyDown={(e) => onGridKey(e, recent.length, RECENT_COLS)}
                  >
                    {recent.map((ch, i) => {
                      const item = chToItem.get(ch);
                      return (
                        <button
                          key={`${ch}-${i}`}
                          type="button"
                          className="em-item"
                          data-idx={i}
                          title={item ? `${item.zh} · ${item.en}` : ch}
                          aria-label={item ? item.zh : ch}
                          onClick={() => pick(ch, item?.zh ?? "")}
                        >
                          {ch}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
              <div className="em-section">
                <div className="em-section-title">{lang === "en" ? cat.en : cat.zh}</div>
                <div
                  className="em-grid"
                  role="listbox"
                  aria-label={lang === "en" ? cat.en : cat.zh}
                  onKeyDown={(e) => onGridKey(e, cat.items.length, GRID_COLS)}
                >
                  {cat.items.map((item, i) => renderItem(item, i, false))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      <div className="em-kbd-hint">{tt("emKbdHint")}</div>
    </div>
  );
}
