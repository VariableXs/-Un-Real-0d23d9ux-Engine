/**
 * NOVA-200 · S0 地基 · 新星中枢 NovaHub（AI-01 路代建，十六路共享）。
 *
 * 200 项能力的统一管理面（实施总步骤 §0.5）：十六域导航 / 搜索（编号+名称+
 * 拼音）/ 开关与参数实时调节 / overlay 工具窗直达 / 域与全局重置。
 * - 状态即 registry 真源（novaSnapshot + useSyncExternalStore）；
 * - 挂载协议：window "ai04:open-feature" { feature: "nova-hub" }（自挂载，
 *   与奇点中枢 singu-hub 同构并行、互不干扰）；
 * - Esc 关闭；打开/关闭均走 wallpaper/mount 既有 overlay 基础设施（零重复）。
 */

import { useEffect, useMemo, useState, useSyncExternalStore, type ReactElement } from "react";
import { useI18n } from "../../i18n";
import {
  NOVA_DOMAINS,
  NOVA_FEATURES,
  novaSnapshot,
  novaStats,
  novaMotionOK,
  resetNovaAll,
  resetNovaDomain,
  setNovaOn,
  setNovaParam,
  subscribeNova,
  type NovaDomainId,
  type NovaParamDef,
} from "./registry";
import { NOVA_LABELS, novaT } from "./labels";
import type { Lang } from "../../i18n/dictionaries";
import { dispatchClose, installCloseHandler, mountOnEvent } from "../wallpaper/mount";
import { pinyinOf } from "../../lib/pinyin";
import { refreshNovaDegrade } from "./NovaRuntime";

export const NOVA_HUB_FEATURE = "nova-hub";

/** 标签双语取值（en → [en, zh] 倒序，使 [0] 恒为当前语言）。 */
function labelOf(id: string, lang: Lang): [string, string] {
  const e = NOVA_LABELS[id];
  if (!e) return [id, id];
  return lang === "en" ? [e[1], e[0]] : e;
}
function titleOf(id: string, lang: Lang): string {
  return labelOf(id, lang)[0];
}
function descOf(id: string, lang: Lang): string {
  return labelOf(id, lang)[1];
}

function ParamControl(props: { id: string; p: NovaParamDef }): ReactElement {
  const { lang } = useI18n();
  const snap = useSyncExternalStore(subscribeNova, novaSnapshot, novaSnapshot);
  const state = snap[props.id];
  const value = state?.params[props.p.key] ?? props.p.default;
  const set = (v: number | boolean | string): void => setNovaParam(props.id, props.p.key, v);
  return (
    <div className="nova-param">
      <span className="nova-param-label">{novaT(props.p.labelKey, lang)}</span>
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
          <b className="nova-param-value">{Number(value)}</b>
        </>
      ) : props.p.type === "toggle" ? (
        <input type="checkbox" checked={value === true} onChange={(e) => set(e.target.checked)} />
      ) : (
        <select value={String(value)} onChange={(e) => set(e.target.value)}>
          {(props.p.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {novaT(o.labelKey, lang)}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

function FeatureRow(props: { id: string }): ReactElement {
  const { lang } = useI18n();
  const snap = novaSnapshot();
  const def = NOVA_FEATURES.find((f) => f.id === props.id);
  const state = snap[props.id];
  if (!def || !state) return <></>;
  const openOverlay = (): void => {
    if (def.overlay) {
      window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: def.overlay } }));
    }
  };
  return (
    <div className={`nova-row ${state.on ? "on" : ""}`}>
      <label className="nova-row-main">
        <input type="checkbox" checked={state.on} onChange={(e) => setNovaOn(def.id, e.target.checked)} />
        <span className="nova-row-id">{def.id}</span>
        <span className="nova-row-title">{titleOf(def.id, lang)}</span>
      </label>
      <p className="nova-row-desc">{descOf(def.id, lang)}</p>
      <p className="nova-row-changelog">
        <b>{novaT("novaH_changelog", lang)}</b> {def.changelog}
      </p>
      {def.params && state.on ? (
        <div className="nova-row-params">
          {def.params.map((p) => (
            <ParamControl key={p.key} id={def.id} p={p} />
          ))}
        </div>
      ) : null}
      {def.overlay && state.on ? (
        <button className="nova-btn" onClick={openOverlay}>
          {novaT("novaH_open", lang)}
        </button>
      ) : null}
    </div>
  );
}

export function NovaHub(): ReactElement {
  useSyncExternalStore(subscribeNova, novaSnapshot);
  const { lang } = useI18n();
  const [domain, setDomain] = useState<NovaDomainId | "all">("all");
  const [query, setQuery] = useState("");
  const stats = novaStats();

  // 打开即复读降级设置（safeMode / static 数据集刷新）
  useEffect(() => {
    refreshNovaDegrade();
  }, []);

  // Esc 关闭
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") dispatchClose(NOVA_HUB_FEATURE);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return NOVA_FEATURES.filter((f) => {
      if (domain !== "all" && f.domain !== domain) return false;
      if (!q) return true;
      const title = titleOf(f.id, "zh").toLowerCase();
      return (
        f.id.toLowerCase().includes(q) ||
        title.includes(q) ||
        pinyinOf(title).toLowerCase().includes(q) ||
        f.changelog.includes(q)
      );
    });
  }, [domain, query]);

  const close = (): void => dispatchClose(NOVA_HUB_FEATURE);

  return (
    <div className="nova-hub" role="dialog" aria-label={novaT("novaH_title", lang)}>
      <header className="nova-hub-head">
        <div className="nova-hub-brand">
          <h1>{novaT("novaH_title", lang)}</h1>
          <span>{novaT("novaH_sub", lang)}</span>
        </div>
        <input
          className="nova-hub-search"
          placeholder={novaT("novaH_search", lang)}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="nova-hub-stats">
          {novaT("novaH_statLine", lang).replace("{on}", String(stats.on)).replace("{total}", String(stats.total))}
        </div>
        <button className="nova-btn" onClick={() => resetNovaAll()}>
          {novaT("novaH_resetAll", lang)}
        </button>
        <button className="nova-btn danger" onClick={close}>
          {novaT("novaH_close", lang)}
        </button>
      </header>
      <div className="nova-hub-body">
        <nav className="nova-hub-nav">
          <button className={domain === "all" ? "on" : ""} onClick={() => setDomain("all")}>
            {novaT("novaH_domainAll", lang)}
          </button>
          {NOVA_DOMAINS.map((d) => (
            <button key={d.id} className={domain === d.id ? "on" : ""} onClick={() => setDomain(d.id)}>
              <i>{String(d.index).padStart(2, "0")}</i>
              {lang === "en" ? d.en : d.zh}
            </button>
          ))}
        </nav>
        <main className="nova-hub-list">
          {!novaMotionOK() ? <div className="nova-hub-note">{novaT("novaH_motionNote", lang)}</div> : null}
          {list.map((f) => (
            <FeatureRow key={f.id} id={f.id} />
          ))}
          <footer className="nova-hub-foot">
            {novaT("novaH_noteLocal", lang)}
            {domain !== "all" ? (
              <button className="nova-btn" onClick={() => resetNovaDomain(domain)}>
                {novaT("novaH_reset", lang)}
              </button>
            ) : null}
          </footer>
        </main>
      </div>
    </div>
  );
}

// ai04 协议自挂载（模块 import 即激活监听；与 SinguHub 同构）
if (typeof window !== "undefined") {
  void mountOnEvent(window, NOVA_HUB_FEATURE, async () => ({ Overlay: (await import("./NovaHub")).NovaHub }));
  void installCloseHandler(window, NOVA_HUB_FEATURE);
}
