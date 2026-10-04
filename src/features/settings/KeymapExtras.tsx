/**
 * AI-05 键位纪律组 — 快捷键设置页扩展（自包含组件，挂在 shortcuts tab 尾部）。
 *
 * 覆盖：Z-14 方案选择器 / M-28 使用榜（本地只读） / M-30 侧键编程 /
 * M-31 启动槽可视化分配 / M-33 按键回显开关 / M-35 任务栏滚轮音量 /
 * M-36 键位变更预览 / V-94 键位体检医生。
 *
 * 红线：M-31 只写 settings.launchApps 映射（launch 槽的应用归属），
 * 不触碰 shortcutBinds 键位表本身；拖放 payload 仅携带 appKey。
 */

import { useMemo, useState } from "react";
import type { Settings } from "../../lib/settings";
import { SHORTCUT_ACTIONS, effectiveBinds, prettyAccel, normalizeAccel } from "../../lib/shortcuts";
import { KEYMAP_PROFILES, applyProfile, validateProfilePatch } from "../../lib/keymap/profiles";
import { previewChanges, suggestFreeSlots } from "../../lib/keymap/preview";
import { runKeymapDoctor } from "../../lib/keymap/doctor";
import { topActions } from "../../lib/keymap/telemetry";
import { useI18n } from "../../i18n";
import { useUi, pushToast } from "../../state/uiStore";

const LAUNCH_IDS = SHORTCUT_ACTIONS.filter((a) => a.group === "launch").map((a) => a.id);

function Field({ label, children }: { label: string; children: React.ReactNode }): JSX.Element {
  return (
    <div style={{ margin: "8px 0" }}>
      <div className="dim small" style={{ marginBottom: 4 }}>{label}</div>
      {children}
    </div>
  );
}

export function KeymapExtras(props: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
  binds: Record<string, string>;
  applyBinds: (next: Record<string, string>) => void;
}): JSX.Element {
  const { t, lang } = useI18n();
  const s = props.settings;
  const uiTab = useUi((st) => st.settingsTab);
  const [doctor, setDoctor] = useState<ReturnType<typeof runKeymapDoctor> | null>(null);
  const zh = lang !== "en";

  // ---- M-36 变更预览（基于当前草稿 binds 与默认表 diff） ----
  const preview = useMemo(() => previewChanges(props.binds), [props.binds]);

  const fullBinds = useMemo(
    () => effectiveBinds(props.binds).filter((b) => b.accel !== ""),
    [props.binds],
  );
  const freeSlots = useMemo(() => suggestFreeSlots(fullBinds.map((b) => b.accel)), [fullBinds]);

  // ---- M-28 使用榜（本地统计，只记 action id + 次数） ----
  const stats = topActions(s.keyStats ?? {}, 5);

  const labelOf = (actionId: string): string => {
    const a = SHORTCUT_ACTIONS.find((x) => x.id === actionId);
    if (!a) return actionId;
    return a.labelKey === "scActLaunchN"
      ? t("scActLaunchN", { n: a.id.replace("launch", "") })
      : t(a.labelKey);
  };

  return (
    <div style={{ marginTop: 14 }}>
      <hr />
      {/* ---- Z-14 键位方案管理 ---- */}
      <Field label={zh ? "键位方案（Z-14）" : "Keymap profile (Z-14)"}>
        <select
          value={s.keymapProfile}
          onChange={(e) => {
            const p = KEYMAP_PROFILES.find((x) => x.id === e.target.value);
            if (!p) return;
            const check = validateProfilePatch(p.patch);
            if (!check.ok) {
              pushToast("error", t("scTitle"), [...check.errors, ...check.conflictList].join("; "));
              return;
            }
            props.applyBinds(applyProfile(s.shortcutBinds ?? {}, p));
            props.onChange({ keymapProfile: p.id });
            pushToast("success", t("scTitle"), p.id);
          }}
        >
          {KEYMAP_PROFILES.map((p) => (
            <option key={p.id} value={p.id}>
              {zh ? { default: "默认方案", leftHand: "左手方案", minimal: "极简方案" }[p.id] ?? p.id : { default: "Default", leftHand: "Left hand", minimal: "Minimal" }[p.id] ?? p.id}
            </option>
          ))}
        </select>
      </Field>
      <p className="dim small">{zh ? "方案以补丁方式覆盖默认表；往返切换后注册表终态一致（Z-14 门禁）。" : "Profiles patch the default table; round-trip switching converges (Z-14 gate)."}</p>

      {/* ---- M-36 键位变更预览 ---- */}
      <Field label={zh ? "变更预览（M-36，保存前 diff）" : "Change preview (M-36, pre-save diff)"}>
        {preview.changes.length === 0 ? (
          <span className="dim small">{zh ? "无待保存变更。" : "No pending changes."}</span>
        ) : (
          <div className="sc-list" style={{ maxHeight: 160, overflow: "auto" }}>
            {preview.changes.map((c) => (
              <div key={c.action} className={`sc-row${c.reservedHit || preview.conflicts.includes(c.to) ? " bad" : ""}`}>
                <span className="sc-label">{labelOf(c.action)}</span>
                <span className="sc-input" style={{ border: "none", background: "transparent" }}>
                  {c.from === "" ? "—" : prettyAccel(c.from, lang)} → {c.to === "" ? (zh ? "禁用" : "disabled") : prettyAccel(c.to, lang)}
                  {c.reservedHit && <span className="sc-err">{zh ? "系统保留键（风险高）" : "system-reserved (high risk)"}</span>}
                </span>
              </div>
            ))}
            {preview.displaced.map((d) => (
              <div key={`${d.action}:${d.accel}`} className="sc-row bad">
                <span className="sc-label">{labelOf(d.action)}</span>
                <span className="sc-err">{zh ? `被 ${labelOf(d.by)} 挤占（${d.accel}）` : `displaced by ${labelOf(d.by)} (${d.accel})`}</span>
              </div>
            ))}
          </div>
        )}
        {freeSlots.length > 0 && (
          <p className="dim small">{zh ? "空闲槽建议：" : "Free slots: "}{freeSlots.map((c) => prettyAccel(c, lang)).join(" · ")}</p>
        )}
      </Field>

      {/* ---- M-31 启动槽可视化分配 ---- */}
      <Field label={zh ? "启动槽分配（M-31，拖应用图标到槽位）" : "Launch slots (M-31, drop app icons)"}>
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          {LAUNCH_IDS.map((id, i) => {
            const app = (s.launchApps ?? {})[id] ?? "";
            return (
              <div
                key={id}
                onDragOver={(e) => e.preventDefault()}
                onDrop={(e) => {
                  e.preventDefault();
                  const appKey = e.dataTransfer.getData("text/appkey") || e.dataTransfer.getData("text/plain");
                  if (!appKey) return;
                  if (appKey === app) return;
                  props.onChange({ launchApps: { ...(s.launchApps ?? {}), [id]: appKey } });
                  pushToast("success", prettyAccel(`ctrl+alt+${i + 1}`, lang), appKey);
                }}
                style={{
                  width: 88, height: 60, borderRadius: 8, display: "flex", flexDirection: "column",
                  alignItems: "center", justifyContent: "center", gap: 2, padding: 4,
                  border: app ? "1px solid var(--accent, #4a7dff)" : "1px dashed var(--bd, #666)",
                  fontSize: 11, textAlign: "center", wordBreak: "break-all", cursor: "default",
                }}
                title={prettyAccel(`ctrl+alt+${i + 1}`, lang)}
              >
                <span style={{ opacity: 0.6 }}>{prettyAccel(`ctrl+alt+${i + 1}`, lang)}</span>
                <span>{app || (zh ? "空槽" : "empty")}</span>
                {app && (
                  <button
                    type="button"
                    className="btn ghost"
                    style={{ padding: "0 6px", height: 18, fontSize: 10 }}
                    onClick={() => {
                      const next = { ...(s.launchApps ?? {}) };
                      delete next[id];
                      props.onChange({ launchApps: next });
                    }}
                  >
                    {zh ? "清空" : "clear"}
                  </button>
                )}
              </div>
            );
          })}
        </div>
        <p className="dim small">{zh ? "应用被卸载后槽位自动清空并提示（后端联动）；同应用可占多槽。" : "Slots clear automatically when the app is removed; same app may occupy multiple slots."}</p>
      </Field>

      {/* ---- M-30 鼠标侧键可编程 ---- */}
      <Field label={zh ? "鼠标侧键编程（M-30）" : "Mouse XButton programming (M-30)"}>
        <div className="row gap8 wrap">
          <label className="dim small">
            XButton1
            <select
              value={s.xBinds.xbutton1}
              onChange={(e) => props.onChange({ xBinds: { ...s.xBinds, xbutton1: e.target.value } })}
              style={{ marginLeft: 6 }}
            >
              <option value="back">{zh ? "后退（默认）" : "Back (default)"}</option>
              <option value="forward">{zh ? "前进" : "Forward"}</option>
              <option value="explorer">{zh ? "资源管理器" : "Explorer"}</option>
              <option value="clipboardHistory">{zh ? "剪贴板历史" : "Clipboard"}</option>
            </select>
          </label>
          <label className="dim small">
            XButton2
            <select
              value={s.xBinds.xbutton2}
              onChange={(e) => props.onChange({ xBinds: { ...s.xBinds, xbutton2: e.target.value } })}
              style={{ marginLeft: 6 }}
            >
              <option value="forward">{zh ? "前进（默认）" : "Forward (default)"}</option>
              <option value="back">{zh ? "后退" : "Back"}</option>
              <option value="explorer">{zh ? "资源管理器" : "Explorer"}</option>
              <option value="notifyCenter">{zh ? "通知中心" : "Notifications"}</option>
            </select>
          </label>
        </div>
        <p className="dim small">{zh ? "全屏应用内自动让位回系统语义；映射经动作分发表复用（kbdhook 轮询路径对接中）。" : "Fullscreen apps fall back to system semantics; actions reuse the dispatch table."}</p>
      </Field>

      {/* ---- M-33 按键回显 ---- */}
      <Field label={zh ? "按键回显（M-33，只回显功能组合，永不显示打字内容）" : "Key cast (M-33, function combos only — typed text never shown)"}>
        <div className="row gap8 wrap">
          <label className="row gap8" style={{ alignItems: "center" }}>
            <input type="checkbox" checked={s.keycast} onChange={(e) => props.onChange({ keycast: e.target.checked })} />
            <span>{zh ? "开启" : "Enabled"}</span>
          </label>
          <select
            value={s.keycastCorner}
            onChange={(e) => props.onChange({ keycastCorner: e.target.value as Settings["keycastCorner"] })}
            disabled={!s.keycast}
          >
            <option value="tl">{zh ? "左上角" : "Top left"}</option>
            <option value="tr">{zh ? "右上角" : "Top right"}</option>
            <option value="bl">{zh ? "左下角" : "Bottom left"}</option>
            <option value="br">{zh ? "右下角" : "Bottom right"}</option>
          </select>
        </div>
      </Field>

      {/* ---- M-35 任务栏滚轮音量 ---- */}
      <Field label={zh ? "任务栏滚轮调音量（M-35）" : "Taskbar wheel volume (M-35)"}>
        <label className="row gap8" style={{ alignItems: "center" }}>
          <input type="checkbox" checked={s.wheelVolume} onChange={(e) => props.onChange({ wheelVolume: e.target.checked })} />
          <span className="dim small">{zh ? "开启后在任务栏滚轮 = 音量步进（Shift=横滚 / Ctrl=缩放语义见 docs/INTERACTION.md）" : "Wheel on taskbar adjusts volume; Shift=horizontal / Ctrl=zoom per docs/INTERACTION.md"}</span>
        </label>
      </Field>

      {/* ---- Z-13 命令提示条开关 ---- */}
      <Field label={zh ? "命令提示条（Z-13）" : "Command hint bar (Z-13)"}>
        <label className="row gap8" style={{ alignItems: "center" }}>
          <input type="checkbox" checked={s.commandHintBar} onChange={(e) => props.onChange({ commandHintBar: e.target.checked })} />
          <span className="dim small">{zh ? "底部实时提示当前可用键位（容器高度 > 600px 才显示）" : "Bottom bar shows active keys (only when viewport height > 600px)"}</span>
        </label>
      </Field>

      {/* ---- M-28 键位使用统计（本地只读榜） ---- */}
      <Field label={zh ? "键位使用榜（M-28，本地 · 只记动作与次数）" : "Key usage (M-28, local · action id + count only)"}>
        {stats.length === 0 ? (
          <span className="dim small">{zh ? "暂无数据。" : "No data yet."}</span>
        ) : (
          <div className="sc-list">
            {stats.map((st) => (
              <div key={st.id} className="sc-row">
                <span className="sc-label">{labelOf(st.id)}</span>
                <span className="dim small">×{st.count}</span>
              </div>
            ))}
          </div>
        )}
      </Field>

      {/* ---- V-94 键位体检医生 ---- */}
      <Field label={zh ? "键位体检医生（V-94）" : "Keymap doctor (V-94)"}>
        <button type="button" className="btn" onClick={() => {
          const report = runKeymapDoctor({ overrides: s.shortcutBinds ?? {} });
          setDoctor(report);
        }}>
          {zh ? "运行体检" : "Run check-up"}
        </button>
        {doctor && (
          <div className="sc-list" style={{ marginTop: 8 }}>
            <div className="sc-row">
              <span className={doctor.healthy ? "sc-label" : "sc-err"}>
                {doctor.healthy ? (zh ? "✓ 键位健康" : "✓ Keymap healthy") : (zh ? "✗ 存在 error 级问题" : "✗ errors found")}
              </span>
            </div>
            {doctor.findings.map((f, i) => (
              <div key={i} className="sc-row">
                <span className={f.severity === "error" ? "sc-err" : "dim small"}>[{f.code}] {f.message}</span>
              </div>
            ))}
            {doctor.freeSlots.length > 0 && (
              <p className="dim small">{zh ? "空闲槽：" : "Free slots: "}{doctor.freeSlots.map((c) => prettyAccel(c, lang)).join(" · ")}</p>
            )}
          </div>
        )}
      </Field>
      {/* uiTab 引用避免未使用告警（tab 感知留待键盘走查） */}
      <span hidden>{uiTab}</span>
    </div>
  );
}

export function isValidAccel(a: string): boolean {
  return normalizeAccel(a) !== null;
}
