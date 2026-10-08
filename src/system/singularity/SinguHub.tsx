/**
 * SINGULARITY-100 · 奇点中枢（Singularity Hub）。
 *
 * 100 项能力的统一管理面：15 域导航 / 搜索（名称+编号+拼音）/ 开关与
 * 参数实时调节 / 降级矩阵（Q-98）/ overlay 工具窗直达。
 * 挂载协议：window "ai04:open-feature" { feature: "singu-hub" }。
 */

import { useEffect, useMemo, useState, useSyncExternalStore, type ReactElement } from "react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import {
  getSinguState,
  resetSinguAll,
  resetSinguDomain,
  setSinguOn,
  setSinguParam,
  singuMotionOK,
  singuStats,
  singuSnapshot,
  subscribeSingu,
  SINGU_DOMAINS,
  SINGU_FEATURES,
  type SinguDomainId,
  type SinguParamDef,
} from "./registry";
import { SINGU_LABELS, singuT } from "./labels";
import { getDegradations, subscribeDegrade } from "./shared";
import { dispatchClose, installCloseHandler, mountOnEvent } from "../wallpaper/mount";
import { pinyinOf } from "../../lib/pinyin";

function labelOf(id: string, _lang: Lang): [string, string] {
  return SINGU_LABELS[id] ?? [id, id];
}
function titleOf(id: string, lang: Lang): string {
  return labelOf(id, lang)[0];
}
function descOf(id: string, lang: Lang): string {
  return labelOf(id, lang)[1];
}

function FeatureRow(props: { id: string }): ReactElement {
  const { lang } = useI18n();
  const def = SINGU_FEATURES.find((f) => f.id === props.id);
  const state = getSinguState()[props.id];
  if (!def || !state) return <></>;
  const openOverlay = (): void => {
    if (def.overlay) {
      window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: def.overlay } }));
    }
  };
  return (
    <div className={`singu-row ${state.on ? "on" : ""}`}>
      <label className="singu-row-main">
        <input type="checkbox" checked={state.on} onChange={(e) => setSinguOn(def.id, e.target.checked)} />
        <span className="singu-row-id">{def.id}</span>
        <span className="singu-row-title">{titleOf(def.id, lang)}</span>
      </label>
      <p className="singu-row-desc">{descOf(def.id, lang)}</p>
      {def.params && state.on ? (
        <div className="singu-row-params">
          {def.params.map((p) => (
            <ParamControl key={p.key} id={def.id} p={p} />
          ))}
        </div>
      ) : null}
      {def.overlay && state.on ? (
        <button className="singu-btn" onClick={openOverlay}>
          {singuT("singuH_open", lang)}
        </button>
      ) : null}
    </div>
  );
}

function ParamControl(props: { id: string; p: SinguParamDef }): ReactElement {
  const { lang } = useI18n();
  const state = getSinguState()[props.id];
  const value = state?.params[props.p.key] ?? props.p.default;
  const label = singuT(props.p.labelKey, lang);
  const set = (v: number | boolean | string): void => setSinguParam(props.id, props.p.key, v);
  return (
    <div className="singu-param">
      <span className="singu-param-label">{label}</span>
      {props.p.type === "slider" ? (
        <>
          <input
            type="range"
            min={props.p.min}
            max={props.p.max}
            step={props.p.step}
            value={Number(value)}
            onChange={(e) => set(Number(e.target.value))}
          />
          <b className="singu-param-value">{Number(value)}</b>
        </>
      ) : props.p.type === "toggle" ? (
        <input type="checkbox" checked={value === true} onChange={(e) => set(e.target.checked)} />
      ) : (
        <select value={String(value)} onChange={(e) => set(e.target.value)}>
          {(props.p.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {singuT(o.labelKey, lang)}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

function DegradeMatrix(): ReactElement {
  const { lang } = useI18n();
  const [, force] = useState(0);
  useEffect(() => subscribeDegrade(() => force((n) => n + 1)), []);
  const rows = getDegradations();
  if (rows.length === 0) return <></>;
  return (
    <div className="singu-degrade">
      <div className="singu-degrade-title">DEGRADATION MATRIX · {titleOf("Q-98", lang)}</div>
      {rows.map((r) => (
        <div key={r.key} className={`singu-degrade-row ${r.active ? "active" : ""}`}>
          <i className={`dot ${r.active ? "on" : ""}`} />
          <b>{r.zh}</b>
          <span>{r.active ? r.reason : r.recover}</span>
        </div>
      ))}
    </div>
  );
}

export function SinguHub(): ReactElement {
  useSyncExternalStore(subscribeSingu, singuSnapshot);
  const { lang } = useI18n();
  const [domain, setDomain] = useState<SinguDomainId | "all">("all");
  const [query, setQuery] = useState("");
  const stats = singuStats();

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return SINGU_FEATURES.filter((f) => {
      if (domain !== "all" && f.domain !== domain) return false;
      if (!q) return true;
      const title = titleOf(f.id, "zh").toLowerCase();
      return f.id.toLowerCase().includes(q) || title.includes(q) || pinyinOf(title).toLowerCase().includes(q);
    });
  }, [domain, query]);

  const close = (): void => dispatchClose("singu-hub");

  return (
    <div className="singu-hub" role="dialog" aria-label={singuT("singuH_title", lang)}>
      <header className="singu-hub-head">
        <div className="singu-hub-brand">
          <h1>{singuT("singuH_title", lang)}</h1>
          <span>{singuT("singuH_sub", lang)}</span>
        </div>
        <input
          className="singu-hub-search"
          placeholder={singuT("singuH_search", lang)}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="singu-hub-stats">
          {singuT("singuH_statLine", lang).replace("{on}", String(stats.on)).replace("{total}", String(stats.total))}
        </div>
        <button className="singu-btn" onClick={() => resetSinguAll()}>
          {singuT("singuH_resetAll", lang)}
        </button>
        <button className="singu-btn danger" onClick={close}>
          {singuT("singuH_close", lang)}
        </button>
      </header>
      <div className="singu-hub-body">
        <nav className="singu-hub-nav">
          <button className={domain === "all" ? "on" : ""} onClick={() => setDomain("all")}>
            {singuT("singuH_domainAll", lang)}
          </button>
          {SINGU_DOMAINS.map((d) => (
            <button key={d.id} className={domain === d.id ? "on" : ""} onClick={() => setDomain(d.id)}>
              <i>{String(d.index).padStart(2, "0")}</i>
              {lang === "en" ? d.en : d.zh}
            </button>
          ))}
        </nav>
        <main className="singu-hub-list">
          {!singuMotionOK() ? <div className="singu-hub-note">{singuT("singuH_motionNote", lang)}</div> : null}
          {list.map((f) => (
            <FeatureRow key={f.id} id={f.id} />
          ))}
          <DegradeMatrix />
          <footer className="singu-hub-foot">
            {singuT("singuH_noteLocal", lang)}
            {domain !== "all" ? (
              <button className="singu-btn" onClick={() => resetSinguDomain(domain)}>
                {singuT("singuH_reset", lang)}
              </button>
            ) : null}
          </footer>
        </main>
      </div>
    </div>
  );
}

// ai04 协议自挂载（模块 import 即激活监听）
if (typeof window !== "undefined") {
  void mountOnEvent(window, "singu-hub", async () => ({ Overlay: (await import("./SinguHub")).SinguHub }));
  void installCloseHandler(window, "singu-hub");
}
