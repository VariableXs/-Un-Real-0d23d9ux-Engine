/**
 * AI-06 输入手感组 — 设置页「输入手感」标签（U-58/U-59、V-61…V-70 面板）。
 *
 * 红线：
 * - V-61「同步到系统」必须显式确认（askConfirm），写后提供一键回滚；
 * - 全部开关默认关闭或等于现状；
 * - 不越权：环境内参数即时生效并明确标注生效范围（不影响其他应用）。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  KEYBOARD_COVERAGE,
  POINTER_STATES,
  REMAP_FROM_KEYS,
  REMAP_TO_KEYS,
  TYPING_SOUNDS,
  validateCustomCursorPack,
  validateRemaps,
  isTestSquareDoubleHit,
  type InputFeelSettings,
  type KeyRemapEntry,
  type PointerSchemeId,
  type PointerState,
} from "../../lib/inputFeel";
import { SHORTCUT_ACTIONS, prettyAccel } from "../../lib/shortcuts";
import "../../styles/input-feel.css";

export function InputFeelTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t, lang } = useI18n();
  const ife = props.settings.inputFeel;
  const set = (patch: Partial<InputFeelSettings>): void =>
    props.onPatch({ inputFeel: { ...ife, ...patch } });

  // ---- V-61：系统当前参数（只读展示 + 写回/回滚基准） ----
  const [sysParams, setSysParams] = useState<{ speed: number; doubleClickMs: number; wheelLines: number; swapButtons: boolean } | null>(null);
  useEffect(() => {
    void ipc.mouseParamsGet().then(setSysParams).catch(() => setSysParams(null));
  }, []);
  const [synced, setSynced] = useState(false);

  // ---- V-61：双击测试方块 ----
  const [sqColor, setSqColor] = useState(false);
  const firstHit = useRef<{ x: number; y: number; t: number } | null>(null);
  const onSqClick = (e: React.MouseEvent): void => {
    const second = { x: e.clientX, y: e.clientY, t: Date.now() };
    if (isTestSquareDoubleHit(firstHit.current, second, ife.mouse.doubleClickMs)) {
      setSqColor((v) => !v); // 双击变色 = 速度合适
      firstHit.current = null;
    } else {
      firstHit.current = second;
    }
  };

  const syncToSystem = () =>
    void (async () => {
      const ok = await askConfirm({ title: t("ifSyncTitle"), body: t("ifSyncBody") });
      if (!ok) return;
      try {
        await ipc.mouseParamsWrite(ife.mouse);
        setSynced(true);
        setSysParams(await ipc.mouseParamsGet());
        pushToast("success", t("ifSyncOk"));
      } catch (e) {
        pushToast("error", t("ifSyncTitle"), errMessage(e).message);
      }
    })();

  const rollbackSystem = () =>
    void (async () => {
      try {
        const done = await ipc.mouseParamsRollback();
        if (done) {
          setSynced(false);
          setSysParams(await ipc.mouseParamsGet());
          pushToast("success", t("ifRollbackOk"));
        } else {
          pushToast("error", t("ifRollbackNone"));
        }
      } catch (e) {
        pushToast("error", t("ifRollbackOk"), errMessage(e).message);
      }
    })();

  // ---- V-62：自定义指针包 ----
  const pickCursor = (st: PointerState) =>
    void (async () => {
      const p = await openFileDialog({
        multiple: false,
        filters: [{ name: "Cursor", extensions: ["cur", "ani"] }],
      });
      if (typeof p !== "string") return;
      set({ customCursors: { ...ife.customCursors, [st]: p } });
    })();
  const cursorPackErrors = validateCustomCursorPack(ife.pointerScheme === "custom" ? ife.customCursors : {});

  // ---- V-66：重映射编辑 ----
  const remapErrors = validateRemaps(ife.keyRemaps);
  const updateRemap = (i: number, patch: Partial<KeyRemapEntry>): void => {
    const next = ife.keyRemaps.map((r, idx) => (idx === i ? { ...r, ...patch } : r));
    set({ keyRemaps: next });
  };

  // ---- V-68：试听（合成 keydown 事件走运行时同一条音效链路） ----
  const previewSound = useCallback(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
  }, []);

  const schemeOptions: { id: PointerSchemeId; label: string }[] = [
    { id: "system", label: t("ifSchemeSystem") },
    { id: "large", label: t("ifSchemeLarge") },
    { id: "high-contrast", label: t("ifSchemeHC") },
    { id: "custom", label: t("ifSchemeCustom") },
  ];

  return (
    <div className="if-tab">
      {/* ---------------- V-61 鼠标手感面板 ---------------- */}
      <section>
        <h3>{t("ifMouseTitle")}</h3>
        <p className="dim small">{t("ifMouseHint")}</p>
        <div className="row gap8 wrap">
          <label className="if-field">
            <span>{t("ifMouseSpeed")}（{ife.mouse.speed}/20）</span>
            <input
              type="range"
              min={1}
              max={20}
              value={ife.mouse.speed}
              onChange={(e) => set({ mouse: { ...ife.mouse, speed: Number(e.target.value) } })}
            />
          </label>
          <label className="if-field">
            <span>{t("ifDoubleClick")}（{ife.mouse.doubleClickMs}ms）</span>
            <input
              type="range"
              min={200}
              max={900}
              step={10}
              value={ife.mouse.doubleClickMs}
              onChange={(e) => set({ mouse: { ...ife.mouse, doubleClickMs: Number(e.target.value) } })}
            />
          </label>
          <label className="if-field">
            <span>{t("ifWheelLines")}（{ife.mouse.wheelLines}）</span>
            <input
              type="range"
              min={1}
              max={10}
              value={ife.mouse.wheelLines}
              onChange={(e) => set({ mouse: { ...ife.mouse, wheelLines: Number(e.target.value) } })}
            />
          </label>
        </div>
        <div className="row gap8 wrap" style={{ marginTop: 8 }}>
          <label className="row gap8">
            <input
              type="checkbox"
              checked={ife.mouse.swapButtons}
              onChange={(e) => set({ mouse: { ...ife.mouse, swapButtons: e.target.checked } })}
            />
            <span>{t("ifSwapButtons")}</span>
          </label>
          <button
            type="button"
            className={`if-test-sq${sqColor ? " hit" : ""}`}
            onClick={onSqClick}
            title={t("ifTestSqHint")}
          >
            {t("ifTestSq")}
          </button>
        </div>
        {sysParams && (
          <p className="dim small">
            {t("ifSysNow")}: {t("ifMouseSpeed")} {sysParams.speed} · {t("ifDoubleClick")} {sysParams.doubleClickMs}ms ·{" "}
            {t("ifWheelLines")} {sysParams.wheelLines} · {t("ifSwapButtons")} {sysParams.swapButtons ? "On" : "Off"}
          </p>
        )}
        <div className="row gap8 wrap">
          <button type="button" className="btn primary" onClick={syncToSystem}>
            {t("ifSyncBtn")}
          </button>
          {synced && (
            <button type="button" className="btn" onClick={rollbackSystem}>
              {t("ifRollbackBtn")}
            </button>
          )}
          <span className="dim small">{t("ifSyncScope")}</span>
        </div>
      </section>

      {/* ---------------- V-62 指针方案管理 ---------------- */}
      <section>
        <h3>{t("ifSchemeTitle")}</h3>
        <div className="row gap8 wrap">
          {schemeOptions.map((o) => (
            <label key={o.id} className="row gap8">
              <input
                type="radio"
                name="if-pointer-scheme"
                checked={ife.pointerScheme === o.id}
                onChange={() => set({ pointerScheme: o.id })}
              />
              <span>{o.label}</span>
            </label>
          ))}
        </div>
        {ife.pointerScheme === "custom" && (
          <div className="if-cursor-grid">
            {POINTER_STATES.map((st) => (
              <div key={st} className="row gap8 if-cursor-row">
                <span className="small" style={{ minWidth: 120 }}>{t(`ifCursor_${st}`)}</span>
                <input className="text-input flex-1" readOnly value={ife.customCursors[st] ?? ""} placeholder=".cur / .ani" />
                <button type="button" className="btn ghost tiny" onClick={() => pickCursor(st)}>
                  {t("chooseFile")}
                </button>
              </div>
            ))}
            {cursorPackErrors.length > 0 && (
              <p className="if-err small">
                {t("ifCursorMissing")}: {cursorPackErrors.map((st) => t(`ifCursor_${st}`)).join("、")}
              </p>
            )}
          </div>
        )}
      </section>

      {/* ---------------- V-63/V-64 轨迹与涟漪 ---------------- */}
      <section>
        <h3>{t("ifFeedbackTitle")}</h3>
        <div className="row gap8 wrap">
          <label className="row gap8">
            <input type="checkbox" checked={ife.trailEnabled} onChange={(e) => set({ trailEnabled: e.target.checked })} />
            <span>{t("ifTrail")}</span>
          </label>
          {ife.trailEnabled && (
            <select
              className="if-select"
              value={ife.trailLevel}
              onChange={(e) => set({ trailLevel: Number(e.target.value) as 1 | 2 | 3 })}
            >
              <option value={1}>{t("ifTrailL1")}</option>
              <option value={2}>{t("ifTrailL2")}</option>
              <option value={3}>{t("ifTrailL3")}</option>
            </select>
          )}
          <label className="row gap8">
            <input type="checkbox" checked={ife.rippleEnabled} onChange={(e) => set({ rippleEnabled: e.target.checked })} />
            <span>{t("ifRipple")}</span>
          </label>
          <span className="dim small">{t("ifFeedbackHint")}</span>
        </div>
      </section>

      {/* ---------------- V-65 大写锁定提示 ---------------- */}
      <section>
        <h3>{t("ifCapsTitle")}</h3>
        <div className="row gap8 wrap">
          <label className="row gap8">
            <input type="checkbox" checked={ife.capsLockHint} onChange={(e) => set({ capsLockHint: e.target.checked })} />
            <span>{t("ifCapsToggle")}</span>
          </label>
          <span className="dim small">{t("ifCapsHint")}</span>
        </div>
      </section>

      {/* ---------------- V-66 按键重映射 ---------------- */}
      <section>
        <h3>{t("ifRemapTitle")}</h3>
        <p className="dim small">{t("ifRemapHint")}</p>
        {ife.keyRemaps.length === 0 && <p className="dim small">{t("ifRemapEmpty")}</p>}
        {ife.keyRemaps.map((r, i) => (
          <div key={i} className="row gap8 if-remap-row">
            <select className="if-select" value={r.from} onChange={(e) => updateRemap(i, { from: e.target.value })}>
              {REMAP_FROM_KEYS.map((k) => (
                <option key={k} value={k}>{k}</option>
              ))}
            </select>
            <span className="dim">→</span>
            <select className="if-select" value={r.to} onChange={(e) => updateRemap(i, { to: e.target.value })}>
              {REMAP_TO_KEYS.map((k) => (
                <option key={k} value={k}>{k}</option>
              ))}
            </select>
            <button
              type="button"
              className="btn ghost tiny"
              onClick={() => set({ keyRemaps: ife.keyRemaps.filter((_, idx) => idx !== i) })}
            >
              ✕
            </button>
            {remapErrors.some((e) => e.index === i) && <span className="if-err small">{t("ifRemapBad")}</span>}
          </div>
        ))}
        <button
          type="button"
          className="btn ghost"
          disabled={remapErrors.length > 0}
          onClick={() => set({ keyRemaps: [...ife.keyRemaps, { from: "CapsLock", to: "ControlLeft" }] })}
        >
          + {t("ifRemapAdd")}
        </button>
      </section>

      {/* ---------------- V-67 自然滚动 / V-68 打字音效 ---------------- */}
      <section>
        <h3>{t("ifScrollSoundTitle")}</h3>
        <div className="row gap8 wrap">
          <label className="row gap8">
            <input type="checkbox" checked={ife.naturalScroll} onChange={(e) => set({ naturalScroll: e.target.checked })} />
            <span>{t("ifNaturalScroll")}</span>
          </label>
          <span className="dim small">{t("ifNaturalScrollHint")}</span>
        </div>
        <div className="row gap8 wrap" style={{ marginTop: 6 }}>
          <label className="if-field">
            <span>{t("ifTypingSound")}</span>
            <select
              className="if-select"
              value={ife.typingSound}
              onChange={(e) => set({ typingSound: e.target.value as InputFeelSettings["typingSound"] })}
            >
              {TYPING_SOUNDS.map((s) => (
                <option key={s} value={s}>{t(`ifSound_${s}`)}</option>
              ))}
            </select>
          </label>
          {ife.typingSound !== "off" && (
            <button type="button" className="btn ghost" onClick={previewSound}>
              {t("ifSoundPreview")}
            </button>
          )}
          <span className="dim small">{t("ifSoundHint")}</span>
        </div>
      </section>

      {/* ---------------- V-69 指针精确模式 / V-70 拖拽阈值 ---------------- */}
      <section>
        <h3>{t("ifPrecisionTitle")}</h3>
        <div className="row gap8 wrap">
          <label className="row gap8">
            <input type="checkbox" checked={ife.precisionEnabled} onChange={(e) => set({ precisionEnabled: e.target.checked })} />
            <span>{t("ifPrecisionToggle")}</span>
          </label>
          {ife.precisionEnabled && (
            <>
              <select
                className="if-select"
                value={ife.precisionModifier}
                onChange={(e) => set({ precisionModifier: e.target.value as InputFeelSettings["precisionModifier"] })}
              >
                <option value="alt">Alt</option>
                <option value="ctrl">Ctrl</option>
                <option value="shift">Shift</option>
              </select>
              <label className="if-field">
                <span>{t("ifPrecisionRatio")}（{Math.round(ife.precisionRatio * 100)}%）</span>
                <input
                  type="range"
                  min={20}
                  max={60}
                  step={5}
                  value={Math.round(ife.precisionRatio * 100)}
                  onChange={(e) => set({ precisionRatio: Number(e.target.value) / 100 })}
                />
              </label>
              <label className="row gap8">
                <input type="checkbox" checked={ife.precisionCross} onChange={(e) => set({ precisionCross: e.target.checked })} />
                <span>{t("ifPrecisionCross")}</span>
              </label>
            </>
          )}
        </div>
        <div className="row gap8 wrap" style={{ marginTop: 8 }}>
          <label className="if-field">
            <span>{t("ifDragThreshold")}（{ife.dragThreshold}px）</span>
            <input
              type="range"
              min={2}
              max={10}
              value={ife.dragThreshold}
              onChange={(e) => set({ dragThreshold: Number(e.target.value) })}
            />
          </label>
          <span className="dim small">{t("ifDragThresholdHint")}</span>
        </div>
      </section>

      {/* ---------------- U-59 触控基础 ---------------- */}
      <section>
        <h3>{t("ifTouchTitle")}</h3>
        <div className="row gap8 wrap">
          <select
            className="if-select"
            value={ife.touchMode}
            onChange={(e) => set({ touchMode: e.target.value as InputFeelSettings["touchMode"] })}
          >
            <option value="auto">{t("ifTouchAuto")}</option>
            <option value="on">{t("ifTouchOn")}</option>
            <option value="off">{t("ifTouchOff")}</option>
          </select>
          <span className="dim small">{t("ifTouchHint")}</span>
        </div>
      </section>

      {/* ---------------- U-58 键盘全景：覆盖审计表 ---------------- */}
      <section>
        <h3>{t("ifCoverageTitle")}</h3>
        <p className="dim small">{t("ifCoverageHint")}</p>
        <div className="if-coverage">
          <table className="if-coverage-table">
            <tbody>
              {KEYBOARD_COVERAGE.map((r) => {
                const action = SHORTCUT_ACTIONS.find((a) => a.accel === r.path);
                const accel = action ? props.settings.shortcutBinds[action.id] ?? action.accel : r.path;
                return (
                  <tr key={r.opKey} className={r.covered ? "" : "bad"}>
                    <td>{t(r.opKey)}</td>
                    <td>
                      <kbd>{prettyAccel(accel, lang)}</kbd>
                    </td>
                    <td>{r.covered ? "✓" : t("ifCoverageTodo")}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
