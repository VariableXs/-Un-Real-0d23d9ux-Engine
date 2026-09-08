/**
 * AI-19 无障碍与本地化组 — 设置页「无障碍与本地化」标签
 * （U-40 无障碍 2.0 / U-41 多语言中心 2.0 / M-73…M-78 面板）。
 *
 * 红线（承 ASCENT U-40/U-41 / SUMMIT 域 U 口径）：
 * - 全部开关默认关闭或等于现状；
 * - 讲述人/屏幕阅读器本体是系统职责，环境只做联动降级与如实提示；
 * - 工作台是字典的安全壳（运行时覆盖，源 dictionaries.ts 结构不动）；
 * - 本页所有图标按钮均带 aria-label（M-76 独善其身）。
 */
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { dictionaries, type Lang } from "../../i18n/dictionaries";
import {
  S2T_LEXICON_MAX,
  s2tLexiconConflict,
  s2t,
} from "../../i18n/s2t";
import { ipc } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import {
  formatBytesLocale,
  formatDateTimeLocale,
  formatNumberLocale,
  formatTimeLocale,
  systemLocale,
  type ByteUnitMode,
} from "../../lib/localeFormat";
import type { CvdKind } from "../../lib/a11y";

interface A11yProbeState {
  stickyKeys: boolean;
  filterKeys: boolean;
  highContrast: boolean;
  narratorRunning: boolean;
}

export function A11yTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t, lang } = useI18n();
  const s = props.settings;
  const set = <K extends keyof Settings>(k: K, v: Settings[K]): void => props.onPatch({ [k]: v } as Partial<Settings>);

  // ---- M-73 探针（手动刷新 + 面板打开即取一次）----
  const [probe, setProbe] = useState<A11yProbeState | null>(null);
  const refreshProbe = (): void => {
    void ipc.a11yProbe().then(setProbe).catch(() => setProbe(null));
  };
  useEffect(refreshProbe, []);

  // ---- M-77 词表编辑态 ----
  const [lexKey, setLexKey] = useState("");
  const [lexVal, setLexVal] = useState("");
  const [lexConflict, setLexConflict] = useState<string | null>(null);
  const lexCount = Object.keys(s.s2tLexicon).length;

  const addLexicon = (): void => {
    const k = lexKey.trim();
    const v = lexVal.trim();
    if (!k || !v || lexCount >= S2T_LEXICON_MAX) return;
    const conflict = s2tLexiconConflict(k, v, s.s2tLexicon);
    setLexConflict(conflict ? t("a19LexConflict", { existing: conflict }) : null);
    set("s2tLexicon", { ...s.s2tLexicon, [k]: v });
    setLexKey("");
    setLexVal("");
  };

  // ---- U-41 工作台 ----
  const [wbQuery, setWbQuery] = useState("");
  const [wbEditKey, setWbEditKey] = useState<string | null>(null);
  const [wbEditZh, setWbEditZh] = useState("");
  const [wbEditEn, setWbEditEn] = useState("");
  const [csvText, setCsvText] = useState("");

  const wbRows = useMemo(() => {
    const q = wbQuery.trim().toLowerCase();
    const keys = Object.keys(dictionaries.zh);
    const filtered = q ? keys.filter((k) => k.toLowerCase().includes(q) || dictionaries.zh[k].toLowerCase().includes(q) || (dictionaries.en[k] ?? "").toLowerCase().includes(q)) : keys;
    return filtered.slice(0, 200).map((k) => ({
      key: k,
      zh: dictionaries.zh[k] ?? "",
      en: dictionaries.en[k] ?? "",
      zhOverridden: !!s.i18nOverrides.zh?.[k],
      enOverridden: !!s.i18nOverrides.en?.[k],
      missingEn: !dictionaries.en[k],
    }));
  }, [wbQuery, s.i18nOverrides]);

  const startEdit = (key: string): void => {
    setWbEditKey(key);
    setWbEditZh(s.i18nOverrides.zh?.[key] ?? dictionaries.zh[key] ?? "");
    setWbEditEn(s.i18nOverrides.en?.[key] ?? dictionaries.en[key] ?? "");
  };

  const saveEdit = (): void => {
    if (!wbEditKey) return;
    const next: Record<string, Record<string, string>> = {
      zh: { ...(s.i18nOverrides.zh ?? {}) },
      en: { ...(s.i18nOverrides.en ?? {}) },
      ...(s.i18nOverrides["zh-TW"] ? { "zh-TW": { ...s.i18nOverrides["zh-TW"] } } : {}),
    };
    if (wbEditZh.trim() && wbEditZh !== dictionaries.zh[wbEditKey]) next.zh[wbEditKey] = wbEditZh.trim();
    else delete next.zh[wbEditKey];
    if (wbEditEn.trim() && wbEditEn !== dictionaries.en[wbEditKey]) next.en[wbEditKey] = wbEditEn.trim();
    else delete next.en[wbEditKey];
    set("i18nOverrides", next);
    setWbEditKey(null);
  };

  const exportCsv = (): void => {
    const rows = Object.keys(dictionaries.zh).map((k) => `${escapeCsv(k)},${escapeCsv(dictionaries.zh[k] ?? "")},${escapeCsv(dictionaries.en[k] ?? "")}`);
    setCsvText(`key,zh,en\n${rows.join("\n")}`);
  };

  const importCsv = (): void => {
    const next: Record<string, Record<string, string>> = {
      zh: { ...(s.i18nOverrides.zh ?? {}) },
      en: { ...(s.i18nOverrides.en ?? {}) },
      ...(s.i18nOverrides["zh-TW"] ? { "zh-TW": { ...s.i18nOverrides["zh-TW"] } } : {}),
    };
    let count = 0;
    for (const line of csvText.split(/\r?\n/).slice(1)) {
      if (!line.trim()) continue;
      const cells = parseCsvLine(line);
      if (cells.length < 2) continue;
      const [k, zh, en] = cells;
      if (!k) continue;
      if (zh && zh !== dictionaries.zh[k]) next.zh[k] = zh;
      if (en && en !== dictionaries.en[k]) next.en[k] = en;
      count++;
    }
    set("i18nOverrides", next);
    setCsvText("");
  };

  const overrideCount = Object.keys(s.i18nOverrides.zh ?? {}).length + Object.keys(s.i18nOverrides.en ?? {}).length;

  // ---- M-78 预览 ----
  const locale = systemLocale();
  const now = Date.now();

  const statusText = (on: boolean | null): string =>
    on === null ? t("a19Unknown") : on ? t("a19On") : t("a19Off");

  return (
    <div className="a19-tab">
      <p className="a19-desc">{t("a19Desc")}</p>

      {/* M-73 系统辅助功能桥 */}
      <section className="a19-group">
        <h4>{t("a19BridgeTitle")}</h4>
        <p className="a19-hint">{t("a19BridgeHint")}</p>
        <div className="a19-status" role="status" aria-live="polite">
          <span>{t("a19Sticky")}: {statusText(probe?.stickyKeys ?? null)}</span>
          <span>{t("a19Filter")}: {statusText(probe?.filterKeys ?? null)}</span>
          <span>{t("a19Narrator")}: {statusText(probe?.narratorRunning ?? null)}</span>
          <span>{t("a19Hc")}: {statusText(probe?.highContrast ?? null)}</span>
        </div>
        <p className="a19-hint">{t("a19NarratorNote")}</p>
        <div className="a19-row">
          <span />
          <button type="button" className="a19-btn" onClick={refreshProbe} aria-label={t("a19ProbeRefresh")}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
              <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6" />
            </svg>
            {t("a19ProbeRefresh")}
          </button>
        </div>
      </section>

      {/* M-74 HC 系统跟随 */}
      <section className="a19-group">
        <h4>{t("a19HcFollowTitle")}</h4>
        <label className="a19-row a19-check">
          <input
            type="checkbox"
            checked={s.themeFollowHc}
            onChange={(e) => set("themeFollowHc", e.target.checked)}
          />
          <span>
            <span className="a19-label">{t("a19HcFollow")}</span>
            <span className="a19-hint">{t("a19HcFollowHint")}</span>
          </span>
        </label>
      </section>

      {/* M-75 动效时长缩放 */}
      <section className="a19-group">
        <h4>{t("a19MotionTitle")}</h4>
        <div className="a19-row">
          <span>
            <span className="a19-label">{t("a19MotionScale")}</span>
            <span className="a19-hint">{t("a19MotionHint")}</span>
          </span>
          <select
            value={String(s.motionScale)}
            onChange={(e) => set("motionScale", e.target.value === "0.5" ? 0.5 : 1)}
            aria-label={t("a19MotionScale")}
          >
            <option value="1">{t("a19MotionNormal")}</option>
            <option value="0.5">{t("a19MotionHalf")}</option>
          </select>
        </div>
      </section>

      {/* U-40 色觉模拟器 + 焦点跟随提示 */}
      <section className="a19-group">
        <h4>{t("a19CvdTitle")}</h4>
        <div className="a19-row">
          <span>
            <span className="a19-label">{t("a19CvdLabel")}</span>
            <span className="a19-hint">{t("a19CvdHint")}</span>
          </span>
          <select
            value={s.cvdSim}
            onChange={(e) => set("cvdSim", e.target.value as CvdKind)}
            aria-label={t("a19CvdLabel")}
          >
            <option value="off">{t("a19CvdOff")}</option>
            <option value="protanopia">{t("a19CvdProtan")}</option>
            <option value="deuteranopia">{t("a19CvdDeuteran")}</option>
            <option value="tritanopia">{t("a19CvdTritan")}</option>
            <option value="achromatopsia">{t("a19CvdAchro")}</option>
          </select>
        </div>
        <label className="a19-row a19-check">
          <input
            type="checkbox"
            checked={s.focusAnnounce}
            onChange={(e) => set("focusAnnounce", e.target.checked)}
          />
          <span>
            <span className="a19-label">{t("a19FocusToggle")}</span>
            <span className="a19-hint">{t("a19FocusHint")}</span>
          </span>
        </label>
      </section>

      {/* M-77 简繁用户词表 */}
      <section className="a19-group">
        <h4>{t("a19LexTitle")}</h4>
        <p className="a19-hint">{t("a19LexHint", { max: String(S2T_LEXICON_MAX), count: String(lexCount) })}</p>
        <div className="a19-lex-add">
          <input
            type="text"
            value={lexKey}
            placeholder={t("a19LexKey")}
            onChange={(e) => setLexKey(e.target.value)}
            aria-label={t("a19LexKey")}
          />
          <span aria-hidden="true">→</span>
          <input
            type="text"
            value={lexVal}
            placeholder={t("a19LexValue")}
            onChange={(e) => setLexVal(e.target.value)}
            aria-label={t("a19LexValue")}
          />
          <button type="button" className="a19-btn" onClick={addLexicon} disabled={!lexKey.trim() || !lexVal.trim()}>
            {t("a19LexAdd")}
          </button>
        </div>
        {lexKey.trim() && (
          <p className="a19-hint">{t("a19LexPreview", { out: s2t(lexKey) })}</p>
        )}
        {lexConflict && <p className="a19-warn" role="alert">{lexConflict}</p>}
        {lexCount > 0 && (
          <ul className="a19-lex-list">
            {Object.entries(s.s2tLexicon).slice(0, 50).map(([k, v]) => (
              <li key={k}>
                <span className="a19-lex-pair">{k} → {v}</span>
                <button
                  type="button"
                  className="a19-icon-btn"
                  onClick={() => {
                    const next = { ...s.s2tLexicon };
                    delete next[k];
                    set("s2tLexicon", next);
                  }}
                  aria-label={t("a19LexDelete", { key: k })}
                >
                  ×
                </button>
              </li>
            ))}
            {lexCount > 50 && <li className="a19-hint">{t("a19LexMore", { count: String(lexCount - 50) })}</li>}
          </ul>
        )}
        <div className="a19-csv">
          <textarea
            value={csvText}
            onChange={(e) => setCsvText(e.target.value)}
            placeholder={t("a19LexCsvPlaceholder")}
            rows={3}
            aria-label={t("a19LexCsvPlaceholder")}
          />
          <div className="a19-csv-actions">
            <button type="button" className="a19-btn" onClick={() => setCsvText(Object.entries(s.s2tLexicon).map(([k, v]) => `${escapeCsv(k)},${escapeCsv(v)}`).join("\n"))}>
              {t("a19LexExport")}
            </button>
            <button type="button" className="a19-btn" onClick={() => { importLexiconCsv(csvText, set, s); setCsvText(""); }}>
              {t("a19LexImport")}
            </button>
            {lexCount > 0 && (
              <button type="button" className="a19-btn" onClick={() => set("s2tLexicon", {})}>
                {t("a19LexClear")}
              </button>
            )}
          </div>
        </div>
      </section>

      {/* U-41 多语言中心 2.0：工作台 + 伪本地化 + RTL 试点 */}
      <section className="a19-group">
        <h4>{t("a19I18nTitle")}</h4>
        <p className="a19-hint">{t("a19I18nHint")}</p>
        <label className="a19-row a19-check">
          <input type="checkbox" checked={s.pseudoLocale} onChange={(e) => set("pseudoLocale", e.target.checked)} />
          <span>
            <span className="a19-label">{t("a19I18nPseudo")}</span>
            <span className="a19-hint">{t("a19I18nPseudoHint")}</span>
          </span>
        </label>
        <label className="a19-row a19-check">
          <input type="checkbox" checked={s.rtlPilot} onChange={(e) => set("rtlPilot", e.target.checked)} />
          <span>
            <span className="a19-label">{t("a19I18nRtl")}</span>
            <span className="a19-hint">{t("a19I18nRtlHint")}</span>
          </span>
        </label>

        <input
          type="text"
          className="a19-query"
          value={wbQuery}
          placeholder={t("a19I18nSearch")}
          onChange={(e) => setWbQuery(e.target.value)}
          aria-label={t("a19I18nSearch")}
        />
        <p className="a19-hint">{t("a19I18nOverrideCount", { count: String(overrideCount) })}</p>
        <div className="a19-wb-list">
          {wbRows.map((r) => (
            <div key={r.key} className={`a19-wb-row${r.missingEn ? " missing" : ""}`}>
              <span className="a19-wb-key">{r.key}</span>
              <span className="a19-wb-zh">{r.zhOverridden ? "● " : ""}{r.zh}</span>
              <span className="a19-wb-en">{r.enOverridden ? "● " : ""}{r.missingEn ? `⚠ ${t("a19I18nMissing")}` : r.en}</span>
              <button type="button" className="a19-icon-btn" onClick={() => startEdit(r.key)} aria-label={t("a19I18nEdit", { key: r.key })}>
                ✎
              </button>
            </div>
          ))}
        </div>
        {wbEditKey && (
          <div className="a19-wb-edit">
            <p className="a19-wb-key">{wbEditKey}</p>
            <label>
              <span className="a19-hint">zh</span>
              <input type="text" value={wbEditZh} onChange={(e) => setWbEditZh(e.target.value)} aria-label={`zh: ${wbEditKey}`} />
            </label>
            <label>
              <span className="a19-hint">en</span>
              <input type="text" value={wbEditEn} onChange={(e) => setWbEditEn(e.target.value)} aria-label={`en: ${wbEditKey}`} />
            </label>
            <div className="a19-csv-actions">
              <button type="button" className="a19-btn" onClick={saveEdit}>{t("a19I18nSave")}</button>
              <button type="button" className="a19-btn" onClick={() => setWbEditKey(null)}>{t("a19I18nCancel")}</button>
            </div>
          </div>
        )}
        <div className="a19-csv">
          <textarea
            value={csvText}
            onChange={(e) => setCsvText(e.target.value)}
            placeholder={t("a19I18nCsvPlaceholder")}
            rows={3}
            aria-label={t("a19I18nCsvPlaceholder")}
          />
          <div className="a19-csv-actions">
            <button type="button" className="a19-btn" onClick={exportCsv}>{t("a19I18nCsvExport")}</button>
            <button type="button" className="a19-btn" onClick={importCsv}>{t("a19I18nCsvImport")}</button>
            {overrideCount > 0 && (
              <button type="button" className="a19-btn" onClick={() => set("i18nOverrides", {})}>
                {t("a19I18nReset")}
              </button>
            )}
          </div>
        </div>
      </section>

      {/* M-78 区域格式跟随 */}
      <section className="a19-group">
        <h4>{t("a19LocaleTitle")}</h4>
        <label className="a19-row a19-check">
          <input type="checkbox" checked={s.localeFormatFollow} onChange={(e) => set("localeFormatFollow", e.target.checked)} />
          <span>
            <span className="a19-label">{t("a19LocaleFollow")}</span>
            <span className="a19-hint">{t("a19LocaleFollowHint")}</span>
          </span>
        </label>
        <div className="a19-row">
          <span>
            <span className="a19-label">{t("a19ByteUnit")}</span>
            <span className="a19-hint">{t("a19ByteUnitHint")}</span>
          </span>
          <select
            value={s.byteUnit}
            onChange={(e) => set("byteUnit", e.target.value as ByteUnitMode)}
            aria-label={t("a19ByteUnit")}
          >
            <option value="auto">{t("a19ByteAuto")}</option>
            <option value="binary">{t("a19ByteBinary")}</option>
            <option value="decimal">{t("a19ByteDecimal")}</option>
          </select>
        </div>
        <p className="a19-hint">
          {t("a19LocalePreview")}: {locale} · {formatDateTimeLocale(now, locale)} · {formatNumberLocale(1234567.89, locale)} · {formatBytesLocale(1536 * 1024 * 1024, s.byteUnit, locale)}
          {s.localeFormatFollow ? "" : ` · ${formatTimeLocale(now, locale)}`}
        </p>
      </section>

      <p className="a19-hint">{lang !== "en" ? "AI-19 · 无障碍与本地化组（U-40/U-41、M-73…M-78）" : "AI-19 · Accessibility & Localization (U-40/U-41, M-73…M-78)"}</p>
    </div>
  );
}

/** CSV 行解析（最小 RFC：引号转义双引号）。 */
function parseCsvLine(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (inQuotes) {
      if (c === '"') {
        if (line[i + 1] === '"') {
          cur += '"';
          i++;
        } else inQuotes = false;
      } else cur += c;
    } else if (c === '"') inQuotes = true;
    else if (c === ",") {
      cells.push(cur);
      cur = "";
    } else cur += c;
  }
  cells.push(cur);
  return cells;
}

function escapeCsv(s: string): string {
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** M-77 词表 CSV 导入（key,value 每行一条）。 */
function importLexiconCsv(
  csv: string,
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void,
  s: Settings,
): void {
  const next: Record<string, string> = { ...s.s2tLexicon };
  for (const line of csv.split(/\r?\n/)) {
    if (!line.trim() || line.startsWith("key,")) continue;
    const cells = parseCsvLine(line);
    if (cells.length >= 2 && cells[0].trim() && cells[1].trim() && Object.keys(next).length < S2T_LEXICON_MAX) {
      next[cells[0].trim()] = cells[1].trim();
    }
  }
  set("s2tLexicon", next);
}

// Lang 类型守卫（导出供测试复用口径）。
export type { Lang };
