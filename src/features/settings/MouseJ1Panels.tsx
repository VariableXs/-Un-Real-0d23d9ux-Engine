/**
 * J 鼠标域 · AI-J1 设置面板·重型子面板集（MouseJ1Tab 的纵深件）。
 *
 * 职责：把 20 项中「需要编辑器/画布/清单」的六类面拆到独立组件——
 * 应用覆盖编辑器（F605/F615/F616）、手势录制试笔台（F617）、跨屏记忆点表
 * （F613）、长按登记表（F619）、遥测与证据包（体验章十三 + MD3 附录 B）、
 * 档案打包（F623 对接 AI-J2）。全部走 j1Store 订阅制 + pushToast 显性反馈。
 */

import { useEffect, useRef, useState } from "react";
import { j1Store, J1_DEFAULTS } from "../mouse/j1store";
import { WHEEL_MODES, type WheelMode } from "../mouse/wheel";
import { DEVICE_PROFILE_CAP, exportDeviceProfiles, importDeviceProfiles, upsertAppProfile, removeAppProfile, trackCurrentApp, type AppProfile, type DeviceProfile } from "../mouse/profiles";
import { clearAppOverride, sideButtonConflicts, sideButtonRegistryRows, type SideButtonsConfig, type SideTarget } from "../mouse/sideButtons";
import { GestureRecorder, exportCustomGestures, importCustomGestures, CUSTOM_GESTURE_CAP } from "../mouse/gestureRecorder";
import { gestureLibrary, type GestureLibraryConfig } from "../mouse/gestures";
import { SENS_PRESETS, simulateTrace, curveSmoothness, type CurveConfig } from "../mouse/curve";
import { autoTuneTremor } from "../mouse/filters";
import { LONG_PRESS_BASE, longPressMs } from "../mouse/hoverTiming";
import { j1Telemetry } from "../mouse/telemetry";
import { buildEvidencePack, auditEvidence } from "../mouse/evidence";
import { exportPack, importPack, validatePack, packSummary, type MousePack } from "../mouse/pack";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { autoscrollVelocity, AUTOSCROLL_PRESET } from "../mouse/autoscroll";
import { activeRuntimeSnapshot } from "../mouse/windowRuntime";
import { actionHandlerSnapshot, J1_ACTION_REGISTRY, findActionMeta } from "../mouse/actions";

/* ------------------------------- 通用小件 ------------------------------- */

export function SectionCard(props: { title: string; f: string; children: React.ReactNode; defaultOpen?: boolean }): React.ReactElement {
  const [open, setOpen] = useState(props.defaultOpen ?? false);
  return (
    <div className="j1x-card">
      <button type="button" className="j1x-card-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="j1x-card-title">{props.title}</span>
        <span className="j1-fnum">{props.f}</span>
        <span className="j1x-chevron" aria-hidden>{open ? "▾" : "▸"}</span>
      </button>
      {open && <div className="j1x-card-body">{props.children}</div>}
    </div>
  );
}

function MiniButton(props: { onClick: () => void; children: React.ReactNode; tone?: "default" | "danger" }): React.ReactElement {
  return (
    <button type="button" className={props.tone === "danger" ? "j1x-btn j1x-btn--danger" : "j1x-btn"} onClick={props.onClick}>
      {props.children}
    </button>
  );
}

function JsonIO(props: { label: string; onExport: () => string; onImport: (json: string) => void; exportName: string }): React.ReactElement {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  return (
    <div className="j1x-jsonio">
      <div className="j1x-jsonio-row">
        <MiniButton
          onClick={() => {
            const json = props.onExport();
            void navigator.clipboard
              ?.writeText(json)
              .then(() => pushToast("success", `${props.exportName} 已复制到剪贴板`))
              .catch(() => {
                setText(json);
                setOpen(true);
                pushToast("info", "剪贴板不可用——内容已放入下方文本框，请手动复制");
              });
          }}
        >
          导出{props.label}
        </MiniButton>
        <MiniButton onClick={() => setOpen(!open)}>{open ? "收起导入" : "导入…"}</MiniButton>
      </div>
      {open && (
        <>
          <textarea
            className="j1x-textarea"
            rows={5}
            placeholder={`粘贴${props.label} JSON…`}
            value={text}
            onChange={(e) => setText(e.target.value)}
            aria-label={`导入${props.label}`}
          />
          <MiniButton
            tone="danger"
            onClick={() => {
              try {
                props.onImport(text);
                setText("");
                setOpen(false);
              } catch (e) {
                pushToast("error", `导入失败`, String(e));
              }
            }}
          >
            确认导入
          </MiniButton>
        </>
      )}
    </div>
  );
}

/* ------------------------------- F605 应用覆盖编辑器 ------------------------------- */

export function WheelOverridesEditor(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const s = j1Store.get("wheelNotch");
  const overrides = (s.overrides as Record<string, WheelMode>) ?? {};
  const [appId, setAppId] = useState("");
  const [mode, setMode] = useState<WheelMode>("notch");

  const add = (): void => {
    if (!appId.trim()) {
      pushToast("info", "先填应用标识（窗口内 data-app-id，如 app-write）");
      return;
    }
    j1Store.set("wheelNotch", { overrides: { ...overrides, [appId.trim()]: mode } });
    setAppId("");
    pushToast("success", `已为「${appId.trim()}」设置覆盖：${WHEEL_MODES.find((m) => m.id === mode)?.name}`);
  };
  const remove = (id: string): void => {
    const next = { ...overrides };
    delete next[id];
    j1Store.set("wheelNotch", { overrides: next });
  };

  return (
    <div className="j1x-stack">
      <div className="j1x-inline">
        <input className="j1x-input" placeholder="应用标识（data-app-id）" value={appId} onChange={(e) => setAppId(e.target.value)} aria-label="应用标识" />
        <select value={mode} onChange={(e) => setMode(e.target.value as WheelMode)} aria-label="覆盖档">
          {WHEEL_MODES.map((m) => (
            <option key={m.id} value={m.id}>{m.name}</option>
          ))}
        </select>
        <MiniButton onClick={add}>添加覆盖</MiniButton>
      </div>
      {Object.keys(overrides).length === 0 && <p className="j1x-empty">还没有应用覆盖——覆盖后全局改档不影响该应用（覆盖优先级如实呈现）。</p>}
      {Object.entries(overrides).map(([id, m]) => (
        <div key={id} className="j1x-rowline">
          <span className="j1x-mono">{id}</span>
          <span className="j1x-badge">{WHEEL_MODES.find((x) => x.id === m)?.name ?? m}</span>
          <MiniButton tone="danger" onClick={() => remove(id)}>移除</MiniButton>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------- F616 应用档案管理 ------------------------------- */

export function AppProfilePanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const s = j1Store.get("appProfiles");
  const profiles = (s.profiles as Record<string, AppProfile>) ?? {};
  const current = (s.currentApp as string) ?? "";
  const [appId, setAppName] = useState("");
  const [sens, setSens] = useState(0.8);

  return (
    <div className="j1x-stack">
      <p className="j1x-hint">前台获焦即挂载（&lt;100ms、增量切换防跳变）；设备档案与应用档案正交——两维各管各的。</p>
      <div className="j1x-inline">
        <input className="j1x-input" placeholder="应用标识（如 app-write）" value={appId} onChange={(e) => setAppName(e.target.value)} aria-label="应用标识" />
        <label className="j1x-inline">
          灵敏度
          <input type="range" min={0.2} max={3} step={0.05} value={sens} onChange={(e) => setSens(Number(e.target.value))} aria-label="档案灵敏度" />
          <span className="j1-num">{sens.toFixed(2)}×</span>
        </label>
        <MiniButton
          onClick={() => {
            if (!appId.trim()) {
              pushToast("info", "先填应用标识");
              return;
            }
            upsertAppProfile({ appId: appId.trim(), appName: appId.trim(), source: "manual", overrides: { sens } });
            setAppName("");
            pushToast("success", "应用档案已保存（手配路）");
          }}
        >
          建档
        </MiniButton>
      </div>
      {Object.keys(profiles).length === 0 && <p className="j1x-empty">还没有应用档案——默认单档案走天下（不想分层的人零干预）。</p>}
      {Object.values(profiles).map((p) => (
        <div key={p.appId} className="j1x-rowline">
          <span className="j1x-mono">{p.appId}</span>
          <span className="j1x-badge">{p.source === "declared" ? "应用声明" : "手动"}</span>
          {p.overrides.sens !== undefined && <span className="j1-num">{p.overrides.sens}×</span>}
          {current === p.appId && <span className="j1x-badge j1x-badge--on">前台</span>}
          <MiniButton
            onClick={() => {
              const ap = trackCurrentApp(p.appId);
              pushToast("success", ap ? `已挂载「${p.appId}」档案` : "该应用无档案（默认态）");
            }}
          >
            试挂载
          </MiniButton>
          <MiniButton
            tone="danger"
            onClick={() => {
              removeAppProfile(p.appId);
              pushToast("success", `已清除「${p.appId}」覆盖——回退全局/设备默认`);
            }}
          >
            清除
          </MiniButton>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------- F615 应用覆盖与冲突审计 ------------------------------- */

export function SideKeyAppPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const s = j1Store.get("sideButtons") as unknown as SideButtonsConfig;
  const apps = s.apps ?? {};
  const [appId, setAppId] = useState("");

  return (
    <div className="j1x-stack">
      <div className="j1x-inline">
        <input className="j1x-input" placeholder="应用标识（如 app-code）" value={appId} onChange={(e) => setAppId(e.target.value)} aria-label="应用标识" />
        {[3, 4].map((b) => (
          <select
            key={b}
            aria-label={`侧键 ${b} 覆盖`}
            value={(apps[appId]?.[String(b)] as { action?: string } | undefined)?.action ?? ""}
            onChange={(e) => {
              if (!appId.trim()) {
                pushToast("info", "先填应用标识");
                return;
              }
              const t: SideTarget = { kind: "action", action: e.target.value };
              j1Store.set("sideButtons", { apps: { ...apps, [appId.trim()]: { ...(apps[appId.trim()] ?? {}), [String(b)]: t } } });
              pushToast("success", `侧键 ${b === 3 ? "后退" : "前进"} 已覆盖到「${appId.trim()}」`);
            }}
          >
            <option value="">（无覆盖）</option>
          </select>
        ))}
      </div>
      <p className="j1x-hint">清除某应用的覆盖后该键回全局默认（一键回全局判据）。</p>
      {Object.entries(apps).map(([id, mapping]) => (
        <div key={id} className="j1x-rowline">
          <span className="j1x-mono">{id}</span>
          {Object.entries(mapping as Record<string, SideTarget>).map(([b, t]) => (
            <span key={b} className="j1x-badge">
              侧键{b === "3" ? "1" : "2"}→{t.kind === "action" ? (t as { action: string }).action : t.kind}
            </span>
          ))}
          <MiniButton tone="danger" onClick={() => { clearAppOverride(id, 3); clearAppOverride(id, 4); pushToast("success", `已清除「${id}」全部侧键覆盖`); }}>
            清除
          </MiniButton>
        </div>
      ))}
      <details className="j1x-details">
        <summary>F244 注册行（冲突审计同源）</summary>
        <pre className="j1x-pre">{sideButtonRegistryRows(s, Object.keys(apps)[0] ?? null).map((r) => `${r.key} · ${r.scope} · ${r.action}`).join("\n") || "（无映射）"}</pre>
        {sideButtonConflicts(s, []).length > 0 && (
          <p className="j1-warning" role="alert">检测到快捷键冲突：{sideButtonConflicts(s, []).map((c) => `${c.key}↔${c.conflictWith}`).join("；")}</p>
        )}
      </details>
    </div>
  );
}

/* ------------------------------- F617 手势录制试笔台 ------------------------------- */

export function GesturePad(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const recRef = useRef(new GestureRecorder());
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rec = recRef.current;
  const gcfg = { ...(J1_DEFAULTS.gestures as unknown as GestureLibraryConfig), ...j1Store.get("gestures") } as GestureLibraryConfig;
  const [name, setName] = useState("");
  const drawing = useRef(false);

  const redraw = (): void => {
    const cv = canvasRef.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, cv.width, cv.height);
    ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--vx-accent") || "#4f7cff";
    ctx.lineWidth = 2.5;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    const trail = rec.trailCopy;
    if (trail.length > 1) {
      ctx.beginPath();
      ctx.moveTo(trail[0]!.x, trail[0]!.y);
      for (const p of trail.slice(1)) ctx.lineTo(p.x, p.y);
      ctx.stroke();
    }
  };

  useEffect(() => {
    const up = (): void => {
      if (!drawing.current) return;
      drawing.current = false;
      const ok = rec.finish();
      if (!ok && rec.lastError) pushToast("error", "录制无效", rec.lastError);
      tick((v) => v + 1);
      redraw();
    };
    window.addEventListener("pointerup", up);
    return () => window.removeEventListener("pointerup", up);
  }, [rec]);

  const lib = gestureLibrary(gcfg);

  return (
    <div className="j1x-stack">
      <canvas
        ref={canvasRef}
        width={320}
        height={140}
        className="j1x-canvas"
        aria-label="手势录制试笔台：按住左键画轨迹，松手完成录制"
        onPointerDown={(e) => {
          drawing.current = true;
          rec.begin();
          const r = e.currentTarget.getBoundingClientRect();
          rec.feed(e.clientX - r.left, e.clientY - r.top);
          redraw();
        }}
        onPointerMove={(e) => {
          if (!drawing.current) return;
          const r = e.currentTarget.getBoundingClientRect();
          rec.feed(e.clientX - r.left, e.clientY - r.top);
          redraw();
        }}
      />
      {rec.state === "review" && rec.result && (
        <div className="j1x-stack">
          <p className="j1x-hint">轨迹编码：{rec.result.dirs.join("→")}（{rec.result.rawPoints} 个原始点）</p>
          {rec.duplicateOf(lib) && <p className="j1-warning" role="alert">与「{rec.duplicateOf(lib)?.name}」重复——换一个画法（重复拒绝显性提示）</p>}
          <div className="j1x-inline">
            <input className="j1x-input" placeholder="手势名称（如 关闭右侧标签）" value={name} onChange={(e) => setName(e.target.value)} aria-label="手势名称" />
            <MiniButton
              onClick={() => {
                const dup = rec.duplicateOf(lib);
                if (dup) {
                  pushToast("error", "轨迹重复", `与「${dup.name}」相同或近似`);
                  return;
                }
                const custom = { ...(gcfg.custom ?? {}) };
                const r = rec.commit(name, `custom.${name || "gesture"}`, custom);
                if (r.ok) {
                  j1Store.set("gestures", { custom });
                  setName("");
                  pushToast("success", "手势已入库");
                } else pushToast("error", "入库失败", r.error ?? "");
                tick((v) => v + 1);
              }}
            >
              入库
            </MiniButton>
            <MiniButton onClick={() => { rec.discard(); tick((v) => v + 1); redraw(); }}>丢弃</MiniButton>
          </div>
        </div>
      )}
      <div className="j1x-stack">
        <p className="j1x-hint">自定义库（{Object.keys(gcfg.custom ?? {}).length}/{CUSTOM_GESTURE_CAP}）——内置 12 条不占额度：</p>
        {Object.entries(gcfg.custom ?? {}).map(([id, g]) => (
          <div key={id} className="j1x-rowline">
            <span>{g.name}</span>
            <span className="j1x-mono">{g.dirs.join("→")}</span>
            <MiniButton
              tone="danger"
              onClick={() => {
                void (async () => {
                  const ok = await askConfirm({ title: "删除自定义手势", body: `删除「${g.name}」？` });
                  if (!ok) return;
                  const next = { ...(gcfg.custom ?? {}) };
                  delete next[id];
                  j1Store.set("gestures", { custom: next });
                })();
              }}
            >
              删除
            </MiniButton>
          </div>
        ))}
        <JsonIO
          label="自定义手势库"
          exportName="手势库"
          onExport={() => exportCustomGestures(gcfg.custom ?? {})}
          onImport={(json) => {
            const r = importCustomGestures(json);
            if (!r.ok) throw new Error(r.errors.join("；"));
            j1Store.set("gestures", { custom: r.data });
            pushToast("success", `导入 ${Object.keys(r.data).length} 条自定义手势`);
          }}
        />
      </div>
    </div>
  );
}

/* ------------------------------- F613 记忆点表 ------------------------------- */

export function ScreenMemoryPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const points = (j1Store.get("screenMemory").points as Record<string, { x: number; y: number }>) ?? {};
  return (
    <div className="j1x-stack">
      {Object.keys(points).length === 0 && <p className="j1x-empty">暂无记忆点——多屏环境下指针停留会自动记录（按 EDID 指纹，换线不乱）。</p>}
      {Object.entries(points).map(([edid, p]) => (
        <div key={edid} className="j1x-rowline">
          <span className="j1x-mono">{edid}</span>
          <span className="j1-num">({p.x}, {p.y})</span>
          <MiniButton tone="danger" onClick={() => { const next = { ...points }; delete next[edid]; j1Store.set("screenMemory", { points: next }); }}>遗忘</MiniButton>
        </div>
      ))}
      {Object.keys(points).length > 0 && (
        <MiniButton tone="danger" onClick={() => { j1Store.set("screenMemory", { points: {} }); pushToast("success", "已清空全部跨屏记忆点"); }}>清空全部</MiniButton>
      )}
    </div>
  );
}

/* ------------------------------- F619 登记表 ------------------------------- */

export function LongPressRegistryPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const registry = { ...LONG_PRESS_BASE, ...((j1Store.get("longPress").registry as Record<string, number>) ?? {}) };
  const scale = (j1Store.get("longPress").scale as number) ?? 1.0;
  const [feat, setFeat] = useState("");
  const [ms, setMs] = useState(500);

  return (
    <div className="j1x-stack">
      <p className="j1x-hint">新增长按功能必须在此登记基线时长（登记纪律）——未登记的功能调用即报错（异常显性化）。</p>
      {Object.entries(registry).map(([f, base]) => (
        <div key={f} className="j1x-rowline">
          <span className="j1x-mono">{f}</span>
          <span>基线 {base}ms</span>
          <span className="j1x-badge">当前 {longPressMs(base, scale)}ms</span>
        </div>
      ))}
      <div className="j1x-inline">
        <input className="j1x-input" placeholder="功能键（如 myFeature）" value={feat} onChange={(e) => setFeat(e.target.value)} aria-label="功能键" />
        <input className="j1x-input" type="number" min={100} max={5000} step={10} value={ms} onChange={(e) => setMs(Number(e.target.value))} aria-label="基线毫秒" />
        <MiniButton
          onClick={() => {
            if (!feat.trim()) {
              pushToast("info", "先填功能键");
              return;
            }
            j1Store.set("longPress", { registry: { ...registry, [feat.trim()]: ms } });
            setFeat("");
            pushToast("success", `「${feat.trim()}」已登记进长按旋钮`);
          }}
        >
          登记
        </MiniButton>
      </div>
    </div>
  );
}

/* ------------------------------- F601 示例区 + 预设 ------------------------------- */

export function CurvePlayground(props: { cfg: CurveConfig; onPreset: (p: { curve: CurveConfig["id"]; sens: number }) => void }): React.ReactElement {
  const trace = simulateTrace(props.cfg.id, props.cfg);
  const smooth = curveSmoothness(props.cfg.id, props.cfg);
  const maxOut = Math.max(...trace.map((t) => t.outPx), 160);
  return (
    <div className="j1x-stack">
      <div className="j1x-chips" role="radiogroup" aria-label="灵敏度预设">
        {SENS_PRESETS.map((p) => (
          <button key={p.id} type="button" className="j1x-chip" title={p.desc} onClick={() => props.onPreset({ curve: p.curve, sens: p.sens })}>
            {p.name}
          </button>
        ))}
      </div>
      <svg width={320} height={120} className="j1x-canvas" role="img" aria-label="示例区输入-输出轨迹预览">
        <line x1={0} y1={118} x2={320} y2={118} className="j1x-axis" />
        <line x1={0} y1={118} x2={320} y2={0} className="j1x-axis j1x-axis--diag" />
        <polyline
          points={trace.map((t) => `${(t.inPx / 160) * 318 + 1},${118 - (t.outPx / maxOut) * 114}`).join(" ")}
          className="j1x-trace"
        />
      </svg>
      <p className="j1x-hint">示例区轨迹（匀速扫动 4→160px）：平滑度 {smooth}px/步 {smooth < 0.15 ? "✓ 无跳变" : "⚠ 建议缓和控制点"}</p>
    </div>
  );
}

/* ------------------------------- F604 16 方位可视化 ------------------------------- */

export function Dir16Grid(): React.ReactElement {
  const auto = { ...AUTOSCROLL_PRESET, enabled: true };
  const cells = Array.from({ length: 16 }, (_, i) => {
    const deg = i * 22.5;
    const rad = (deg * Math.PI) / 180;
    const v = autoscrollVelocity(Math.cos(rad) * 40, Math.sin(rad) * 40, auto);
    return { i, deg, active: v.dirIndex === i && (v.vx !== 0 || v.vy !== 0) };
  });
  return (
    <div className="j1x-dirgrid" role="img" aria-label="16 方位映射可视化">
      {cells.map((c) => (
        <span key={c.i} className={c.active ? "j1x-dircell j1x-dircell--on" : "j1x-dircell"} title={`${c.deg}° → 档 ${c.i}`}>
          {c.i}
        </span>
      ))}
    </div>
  );
}

/* ------------------------------- F611 自动调谐 ------------------------------- */

export function TremorAutoTune(): React.ReactElement {
  const samplesRef = useRef<{ dx: number; dy: number }[]>([]);
  const [result, setResult] = useState<{ recommended: string; jitterRatio: number } | null>(null);
  const last = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const onMove = (e: PointerEvent): void => {
      if (samplesRef.current.length >= 120) return;
      const l = last.current;
      if (l) samplesRef.current.push({ dx: e.clientX - l.x, dy: e.clientY - l.y });
      last.current = { x: e.clientX, y: e.clientY };
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  return (
    <div className="j1x-stack">
      <p className="j1x-hint">在屏幕上正常移动指针几秒，点击「分析」——按你的抖动能量推荐档位（建议非强制，一键采纳）。</p>
      <div className="j1x-inline">
        <MiniButton
          onClick={() => {
            const r = autoTuneTremor(samplesRef.current);
            if (r.jitterRatio === 0 && samplesRef.current.length < 20) {
              pushToast("info", "样本不足——再移动几秒指针");
              return;
            }
            setResult({ recommended: r.recommended, jitterRatio: r.jitterRatio });
            j1Store.set("tremor", { level: r.recommended });
            pushToast("success", `已按分析结果设为「${r.recommended === "off" ? "关" : r.recommended === "light" ? "轻" : "强"}」档（抖动占比 ${Math.round(r.jitterRatio * 100)}%）`);
          }}
        >
          分析并采纳
        </MiniButton>
        {result && <span className="j1x-hint">建议档：{result.recommended} · 抖动占比 {Math.round(result.jitterRatio * 100)}%</span>}
      </div>
    </div>
  );
}

/* ------------------------------- 遥测面板 ------------------------------- */

export function TelemetryPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const worst = j1Telemetry.worstTen();
  return (
    <div className="j1x-stack">
      <div className="j1x-inline">
        <span className="j1x-hint">事件 {j1Telemetry.size} 条 · 挫败信号 {j1Telemetry.frustrationCount} 条</span>
        <MiniButton onClick={() => { void navigator.clipboard?.writeText(j1Telemetry.exportTimeline()).then(() => pushToast("success", "体验时间轴已复制")); }}>导出时间轴</MiniButton>
        <MiniButton
          tone="danger"
          onClick={() => {
            void (async () => {
              const ok = await askConfirm({ title: "清空体验日志", body: "清空全部交互事件与挫败信号？此操作不可撤销。" });
              if (!ok) return;
              j1Telemetry.clear();
              tick((v) => v + 1);
            })();
          }}
        >
          清空
        </MiniButton>
      </div>
      {worst.length === 0 && <p className="j1x-empty">暂无挫败信号——狂点/死点/手势放弃会被自动标记（不用等投诉）。</p>}
      {worst.map((f, i) => (
        <div key={`${f.at}-${i}`} className="j1x-rowline">
          <span className="j1x-badge j1x-badge--warn">{f.kind}</span>
          <span className="j1x-hint">{f.detail}</span>
          <span className="j1x-hint">{new Date(f.at).toLocaleTimeString()}</span>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------- v3 运行时接线审计 ------------------------------- */

/**
 * 运行时接线审计（v3）：诚实呈现「哪些窗口挂着内核、动作路由表里有什么、
 * F609 声明容器有几处、F610 变量通道现在是什么值」——接线状态一目了然，
 * 不让「已实现」停留在代码注释里。
 */
export function WiringPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const runtimes = activeRuntimeSnapshot();
  const handlers = actionHandlerSnapshot();
  const containers = typeof document !== "undefined" ? document.querySelectorAll(`[data-autoscroll]`).length : 0;
  let menuVar = "（不可用）";
  let hoverOverride = "（未覆盖——基线档）";
  if (typeof document !== "undefined") {
    const rootStyle = getComputedStyle(document.documentElement);
    menuVar = rootStyle.getPropertyValue("--vx-menu-delay").trim() || "（未设）";
    const hd = rootStyle.getPropertyValue("--hover-delay").trim();
    // tokens.css 基线 400ms；被 F610 改写时显示覆盖值。
    hoverOverride = hd && hd !== "400ms" ? hd : "（未覆盖——基线档）";
  }
  const registered = new Set(J1_ACTION_REGISTRY.map((a) => a.action));

  return (
    <div className="j1x-stack">
      <p className="j1x-hint">
        接线快照（只读诊断）：运行时按窗口挂载——桌面窗全量层（副本/锚标/墨迹），其余窗口 headless（滚轮/侧键/手势/自动滚真实生效、零渲染层）。
      </p>
      <div className="j1x-rowline"><span className="j1x-badge">本窗挂载</span><span className="j1x-hint">{runtimes.map((r) => `${r.entry}${r.replica ? "（全量层）" : "（headless）"}`).join("、") || "（未挂载——本窗不在 J1 覆盖清单）"}</span></div>
      <div className="j1x-rowline"><span className="j1x-badge">F609 声明容器</span><span className="j1x-hint">{containers} 处（data-autoscroll：设置弹窗滚动体 / explorer 文件列表——拖拽到边缘自动滚的接入面）</span></div>
      <div className="j1x-rowline"><span className="j1x-badge">F610 变量通道</span><span className="j1x-hint">--vx-menu-delay {menuVar} · --hover-delay {hoverOverride}（tooltip 旋钮偏离 500ms 基线时接管全局令牌）</span></div>
      <div className="j1x-rowline"><span className="j1x-badge">动作路由表</span><span className="j1x-hint">登记 {handlers.length} 条处理器（内置 {handlers.filter((h) => registered.has(h.action)).length} + 扩展 {handlers.filter((h) => !registered.has(h.action)).length}）</span></div>
      <details className="j1x-details">
        <summary>内置动作登记表（12 手势 + 侧键系统动作）</summary>
        <div className="j1x-stack">
          {J1_ACTION_REGISTRY.map((a) => {
            const wired = handlers.some((h) => h.action === a.action);
            return (
              <div key={`${a.via}-${a.action}`} className="j1x-rowline">
                <span className={wired ? "j1x-badge j1x-badge--on" : "j1x-badge"}>{wired ? "已接线" : "待消费者"}</span>
                <span className="j1x-mono">{a.action}</span>
                <span className="j1x-hint">{findActionMeta(a.action)?.name ?? a.name}</span>
              </div>
            );
          })}
        </div>
      </details>
    </div>
  );
}

/* ------------------------------- F623 档案打包 ------------------------------- */

export function PackPanel(): React.ReactElement {
  const [summary, setSummary] = useState<string | null>(null);
  return (
    <div className="j1x-stack">
      <p className="j1x-hint">J1 全配置 + 设备档案 + 自定义手势一包带走（F623 vxtheme 打包对接——AI-J2 消费同一格式）。</p>
      <JsonIO
        label="鼠标档案包"
        exportName="档案包"
        onExport={() => {
          const pack = exportPack();
          setSummary(packSummary(pack));
          return JSON.stringify(pack, null, 2);
        }}
        onImport={(json) => {
          const parsed = JSON.parse(json) as MousePack;
          const v = validatePack(parsed);
          if (!v.ok) throw new Error(v.errors.join("；"));
          const r = importPack(parsed);
          pushToast("success", `档案包已导入${r.evicted.length > 0 ? `（淘汰最久未用：${r.evicted.join("、")}）` : ""}`);
        }}
      />
      {summary && <p className="j1x-hint">{summary}</p>}
    </div>
  );
}

/* ------------------------------- F614 设备档案管理 ------------------------------- */

export function DevicePackPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const profiles = (j1Store.get("devices").profiles as DeviceProfile[]) ?? [];
  return (
    <div className="j1x-stack">
      <p className="j1x-hint">档案 {profiles.length}/{DEVICE_PROFILE_CAP} 台——超出淘汰最久未用（F543 同纪律）。</p>
      <JsonIO
        label="设备档案"
        exportName="设备档案"
        onExport={() => JSON.stringify(exportDeviceProfiles({ profiles, notifyOnClone: true }), null, 2)}
        onImport={(json) => {
          const r = importDeviceProfiles(JSON.parse(json));
          if (r.accepted.length > 0) {
            const keys = new Set(profiles.map((p) => p.deviceKey));
            const merged = [...profiles, ...r.accepted.filter((p) => !keys.has(p.deviceKey))];
            j1Store.set("devices", { profiles: merged });
          }
          pushToast(
            r.rejected.length > 0 ? "info" : "success",
            `导入完成：${r.accepted.length} 台收录${r.rejected.length > 0 ? `，${r.rejected.length} 台被拒（${r.rejected.map((x) => `${x.name}: ${x.errors[0]}`).join("；")}）` : ""}`,
          );
        }}
      />
    </div>
  );
}

/* ------------------------------- 证据包 ------------------------------- */

export function EvidencePanel(): React.ReactElement {
  const [audit, setAudit] = useState<{ ok: boolean; failures: string[] } | null>(null);
  return (
    <div className="j1x-stack">
      <p className="j1x-hint">证据包 = F601 对拍表 + F603 自检 + F604 方位表 + F605 档位矩阵 + F607 护边矩阵 + F611 谱表 + F619 对账 + F620 走查样本（数据、复现命令、日期三件齐）。</p>
      <div className="j1x-inline">
        <MiniButton
          onClick={() => {
            const pack = buildEvidencePack();
            const a = auditEvidence(pack);
            setAudit(a);
            void navigator.clipboard
              ?.writeText(JSON.stringify(pack, null, 2))
              .then(() => pushToast(a.ok ? "success" : "info", a.ok ? "证据包已复制（内建自检全绿）" : "证据包已复制（自检有失败项，见下方）"))
              .catch(() => pushToast("error", "剪贴板不可用", "请在控制台调用 buildEvidencePack() 导出"));
          }}
        >
          生成并复制证据包
        </MiniButton>
      </div>
      {audit && !audit.ok && (
        <p className="j1-warning" role="alert">自检失败项：{audit.failures.join("；")}</p>
      )}
    </div>
  );
}
