import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { useStore, type Store } from "../../lib/store";
import { pushToast } from "../../state/uiStore";
import { saveSetting, type Settings } from "../../lib/settings";
import { vwmStore, snapVwmRect, type VwmWin } from "./vwm";
import {
  listSnaps,
  snapshotNow,
  clearTimeline,
  planRestore,
  restoreTargets,
  takeSnap,
  type TimelineSnap,
} from "./timeline";
import { healthCheck } from "./rescue";
import { loadScenes, saveScene, deleteScene, applyScene, type Scene } from "./scenes";
import { activateStage } from "./StageRail";
import { loadRules, saveRule, ruleLogs, RULE_TEMPLATES, loadRules as rulesOf } from "./rules";
import { loadStages } from "./stages";
import { snap2Enabled, setSnap2Enabled } from "./snap2";
import { colorBandEnabled, setColorBandEnabled, loadBandMap, saveBandMap, BAND_LABEL, type BandColor } from "./colorBand";

/**
 * 窗口编排中心（AI-02 窗口编排组 · Ctrl+Alt+O 呼出）：
 * - N-01 时间机器：定格此刻 / 快照列表 / 一键恢复（缺失窗口如实报告）/ 清空；
 * - V-22 失联救援：手动体检 + 一键拉回；
 * - N-05 场景：从当前布局新建场景 / 应用（未保存守卫 + 全量回滚）/ 删除；
 * - N-03 规则：模板一键启用 / 启用位开关 / 裁决日志；
 * - 编排设置：U-14 吸附 2.0 开关、V-29 色带开关与按应用配色。
 */

type Tab = "timeline" | "scenes" | "rules";

/** N-01：应用快照（精确恢复仅重摆位置；缺失窗口如实报告）。 */
export function applyTimelineSnap(snap: TimelineSnap, wins: VwmWin[]): { restored: number; missing: number } {
  const plan = planRestore(snap, wins);
  const targets = restoreTargets(plan);
  let maxZ = vwmStore.getState().topZ;
  vwmStore.setState((st) => ({
    wins: st.wins.map((w) => {
      const tg = targets[w.id];
      if (!tg) return w;
      maxZ = Math.max(maxZ, tg.z);
      return { ...w, x: tg.x ?? w.x, y: tg.y ?? w.y, w: tg.w ?? w.w, h: tg.h ?? w.h, state: "normal" };
    }),
    topZ: maxZ,
  }));
  return { restored: plan.exact.length, missing: plan.missing.length };
}

export function WindowOrchestrator(props: { settings: Settings; onClose: () => void }): React.ReactElement {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>("timeline");
  const wins = useStore(vwmStore, (s) => s.wins);
  const [version, setVersion] = useState(0);
  void version;
  useEffect(() => vwmStore.subscribe(() => setVersion((v) => v + 1)), []);

  const snaps = listSnaps();
  const scenes = loadScenes();
  const rules = loadRules();
  const logs = ruleLogs().slice(-8).reverse();

  // ---------- N-01 ----------
  const doSnapNow = (): void => {
    snapshotNow(wins);
    setVersion((v) => v + 1);
    pushToast("info", t("orchSnapped"));
  };
  const doRestore = (snap: TimelineSnap): void => {
    const r = applyTimelineSnap(snap, wins);
    pushToast(
      r.missing > 0 ? "info" : "success",
      t("orchRestoredTitle"),
      `${t("orchRestoredBody")} ${r.restored} · ${t("orchMissingBody")} ${r.missing}`,
    );
  };

  // ---------- V-22 ----------
  const doRescue = (): void => {
    const st = vwmStore.getState();
    const rep = healthCheck(st.wins, st.workArea);
    if (rep.lost.length === 0) {
      pushToast("info", t("orchRescue"), t("orchRescueNone"));
      return;
    }
    vwmStore.setState((cur) => ({
      wins: cur.wins.map((w) => (rep.moved[w.id] ? { ...w, ...rep.moved[w.id] } : w)),
    }));
    pushToast("ok", t("orchRescue"), `${t("orchRescuedN")} ${rep.lost.length}`);
  };

  // ---------- N-05 ----------
  const sceneFromCurrent = (): void => {
    const s: Scene = {
      id: `scene-${Date.now().toString(36)}`,
      name: `${t("sceneDefaultName")} ${scenes.length + 1}`,
      visual: {},
      orchestration: { snapshotRef: takeSnap(wins) },
      behavior: {},
      triggers: [{ type: "manual" }],
    };
    saveScene(s);
    setVersion((v) => v + 1);
  };
  const applySceneById = (sc: Scene): void => {
    const st = vwmStore.getState();
    const res = applyScene(sc, {
      workArea: st.workArea,
      titles: st.wins.map((w) => w.app),
      applySnapshot: (snap) => {
        applyTimelineSnap(snap, st.wins);
      },
      applyStage: (stageId) => {
        activateStage(stageId);
      },
      applySnap: (app, rect) => {
        const top = st.wins
          .filter((w) => w.app === app)
          .sort((a, b) => b.z - a.z)[0];
        if (top) snapVwmRect(top.id, rect);
      },
      applyVisual: (v) => {
        // 视觉包：持久化生效（theme/wallpaperMode/fontSize/taskbarPos 与设置中心同字段）
        if (v.theme !== undefined) void saveSetting("theme", v.theme);
        if (v.wallpaperMode !== undefined) void saveSetting("wallpaperMode", v.wallpaperMode);
        if (v.fontSize !== undefined) void saveSetting("fontSize", v.fontSize);
        if (v.taskbarPos !== undefined) void saveSetting("taskbarPos", v.taskbarPos);
      },
      applyBehavior: (_b) => {
        // 行为包（勿扰/声音）：勿扰属 AI-16 通知域领地，跨组协作接入前为空操作（诚实降级）
      },
    });
    if (!res.ok && res.guard) {
      pushToast("info", t("sceneGuardTitle"), t("scGuardBody"));
    } else if (!res.ok) {
      pushToast("error", t("scFailTitle"), res.error ?? "");
    } else {
      pushToast("success", t("scAppliedTitle"), sc.name);
    }
  };

  // ---------- N-03 ----------
  const addTemplate = (idx: number): void => {
    const tpl = RULE_TEMPLATES[idx];
    if (!tpl) return;
    saveRule({ ...tpl.rule, id: `rule-${Date.now().toString(36)}-${idx}` });
    setVersion((v) => v + 1);
  };
  const toggleRule = (id: string): void => {
    const r = rules.find((x) => x.id === id);
    if (!r) return;
    saveRule({ ...r, enabled: !r.enabled });
    setVersion((v) => v + 1);
  };

  // ---------- 编排设置 ----------
  const [snap2, setSnap2] = useState(snap2Enabled());
  const [bandOn, setBandOn] = useState(colorBandEnabled());
  const [bandMap, setBandMap] = useState(loadBandMap());
  const appKeys = [...new Set(wins.map((w) => w.app as string))];

  return (
    <div className="vwm-orch-backdrop" onPointerDown={props.onClose} data-testid="window-orchestrator">
      <div
        className="vwm-orch"
        role="dialog"
        aria-label={t("orchTitle")}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <div className="vwm-orch-head">
          <strong>{t("orchTitle")}</strong>
          <div className="vwm-orch-tabs" role="tablist">
            {(["timeline", "scenes", "rules"] as const).map((k) => (
              <button
                key={k}
                type="button"
                role="tab"
                aria-selected={tab === k}
                className={`vwm-orch-tab${tab === k ? " on" : ""}`}
                onClick={() => setTab(k)}
              >
                {t(k === "timeline" ? "orchTabTimeline" : k === "scenes" ? "orchTabScenes" : "orchTabRules")}
              </button>
            ))}
          </div>
          <button type="button" className="vwm-orch-close" aria-label={t("close")} onClick={props.onClose}>
            ×
          </button>
        </div>

        <div className="vwm-orch-body">
          {tab === "timeline" && (
            <>
              <div className="vwm-orch-row">
                <button type="button" className="btn" onClick={doSnapNow}>
                  {t("orchSnapNow")}
                </button>
                <button type="button" className="btn" onClick={doRescue}>
                  {t("orchRescue")}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    const n = clearTimeline();
                    setVersion((v) => v + 1);
                    pushToast("info", t("orchCleared"), `${n}`);
                  }}
                >
                  {t("orchClear")}
                </button>
              </div>
              {snaps.length === 0 && <div className="vwm-orch-empty">{t("orchEmpty")}</div>}
              <ul className="vwm-orch-list">
                {snaps.slice(0, 30).map((s) => (
                  <li key={s.ts}>
                    <span className="vwm-orch-name">{s.name ?? new Date(s.ts).toLocaleString()}</span>
                    <span className="vwm-orch-meta">{s.vwm.length}</span>
                    <button type="button" className="btn tiny" onClick={() => doRestore(s)}>
                      {t("orchRestore")}
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}

          {tab === "scenes" && (
            <>
              <div className="vwm-orch-row">
                <button type="button" className="btn" onClick={sceneFromCurrent}>
                  {t("sceneFromCurrent")}
                </button>
              </div>
              {scenes.length === 0 && <div className="vwm-orch-empty">{t("scEmpty")}</div>}
              <ul className="vwm-orch-list">
                {scenes.map((s) => (
                  <li key={s.id}>
                    <span className="vwm-orch-name">{s.name}</span>
                    <span className="vwm-orch-meta">
                      {s.orchestration.snapshotRef ? "N-01" : ""}
                      {s.orchestration.stageRef ? " N-02" : ""}
                      {s.orchestration.snaps?.length ? ` ×${s.orchestration.snaps.length}` : ""}
                    </span>
                    <button type="button" className="btn tiny" onClick={() => applySceneById(s)}>
                      {t("scApply")}
                    </button>
                    <button
                      type="button"
                      className="btn tiny"
                      onClick={() => {
                        deleteScene(s.id);
                        setVersion((v) => v + 1);
                      }}
                    >
                      {t("scDelete")}
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}

          {tab === "rules" && (
            <>
              <div className="vwm-orch-row">
                <select
                  aria-label={t("rulesTemplates")}
                  defaultValue=""
                  onChange={(e) => {
                    const i = Number(e.target.value);
                    if (!Number.isNaN(i)) addTemplate(i);
                    e.currentTarget.value = "";
                  }}
                >
                  <option value="" disabled>
                    {t("rulesTemplates")}
                  </option>
                  {RULE_TEMPLATES.map((tp, i) => (
                    <option key={tp.name} value={i}>
                      {tp.name}
                    </option>
                  ))}
                </select>
              </div>
              {rules.length === 0 && <div className="vwm-orch-empty">{t("rulesEmpty")}</div>}
              <ul className="vwm-orch-list">
                {rules.map((r) => (
                  <li key={r.id}>
                    <span className="vwm-orch-name">{r.name}</span>
                    <span className="vwm-orch-meta">{r.trigger.app}</span>
                    <button type="button" className="btn tiny" onClick={() => toggleRule(r.id)}>
                      {r.enabled ? t("rulesEnabled") : t("rulesDisabled")}
                    </button>
                  </li>
                ))}
              </ul>
              {logs.length > 0 && (
                <>
                  <div className="vwm-orch-cap">{t("rulesLogs")}</div>
                  <ul className="vwm-orch-logs">
                    {logs.map((l) => (
                      <li key={`${l.ts}-${l.text}`}>{l.text}</li>
                    ))}
                  </ul>
                </>
              )}
            </>
          )}

          {/* 编排设置（U-14 / V-29） */}
          <div className="vwm-orch-cap">{t("orchSettings")}</div>
          <label className="vwm-orch-check">
            <input
              type="checkbox"
              checked={snap2}
              onChange={(e) => {
                setSnap2Enabled(e.target.checked);
                setSnap2(e.target.checked);
              }}
            />
            {t("orchSnap2Toggle")}
          </label>
          <label className="vwm-orch-check">
            <input
              type="checkbox"
              checked={bandOn}
              onChange={(e) => {
                setColorBandEnabled(e.target.checked);
                setBandOn(e.target.checked);
                setVersion((v) => v + 1);
              }}
            />
            {t("orchBandToggle")}
          </label>
          {bandOn && (
            <div className="vwm-orch-bands">
              {appKeys.map((app) => (
                <label key={app} className="vwm-orch-band-row">
                  <span>{app}</span>
                  <select
                    value={bandMap[app] ?? ""}
                    onChange={(e) => {
                      const next = { ...bandMap };
                      const v = e.target.value as BandColor | "";
                      if (v === "") delete next[app];
                      else next[app] = v;
                      saveBandMap(next);
                      setBandMap(next);
                      setVersion((x) => x + 1);
                    }}
                  >
                    <option value="">{t("orchBandNone")}</option>
                    {(Object.keys(BAND_LABEL) as BandColor[]).map((c) => (
                      <option key={c} value={c}>
                        {BAND_LABEL[c]}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// 舞台组数量（场景页展示引用用；保留 Store 类型引用防未用告警）
export type _Store = Store<unknown>;
export function _stageCount(): number {
  return loadStages().length;
}
