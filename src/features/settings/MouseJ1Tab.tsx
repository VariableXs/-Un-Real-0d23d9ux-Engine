/**
 * J 鼠标域 · AI-J1 — 设置中心「鼠标」标签页（F601-F620 全量面板）。
 *
 * 面板纪律：
 * - 名称 + 一句话说明 + 调节控件三件套齐全（F474 同源）；
 * - 默认档 = 主册判据默认（描边开、磁吸关、手势关、增益开……）；
 * - 全部改动即时生效（j1Store 订阅制广播）+ 可回退（undoSection 栈深 3）；
 * - 能力检测显隐（F606：无倾斜轮硬件时该节显示能力说明而非空开关）；
 * - 高密度纵深排布：五个分组区，二十项各就各位，无空话。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { j1Store, J1_DEFAULTS, type J1Section } from "../mouse/j1store";
import { CURVE_LIBRARY, SLOW_TUNE_KEYS, SLOW_TUNE_RATIOS, gainTable20, slowTuneRegistryRow, type CurveConfig, type CurveId, type SlowTuneKey } from "../mouse/curve";
import { TREMOR_LEVELS, tremorSpectrum } from "../mouse/filters";
import { WHEEL_MODES, notchLines, PASSTHROUGH_TYPE_SEMANTICS, type WheelNotchConfig, type WheelMode } from "../mouse/wheel";
import { SEAM_GUARD_PRESET, cornerExempt } from "../mouse/screen";
import { MAGNET_RADII } from "../mouse/magnet";
import { autoscrollVelocity, AUTOSCROLL_PRESET, DRAG_BAND_PX, edgeScrollSpeed } from "../mouse/autoscroll";
import { HOVER_DELAY_STEPS, LONG_PRESS_SCALES, LONG_PRESS_LABELS, longPressMs, longPressDefaultAudit } from "../mouse/hoverTiming";
import { DEVICE_PROFILE_CAP, validateDeviceProfile, type DeviceProfile } from "../mouse/profiles";
import { FIVE_BUTTON_MAP, SIDE_ACTIONS, sideGesturePriorityMatrix } from "../mouse/sideButtons";
import { BUILTIN_GESTURES } from "../mouse/gestures";
import { OVERLAY_WALKTHROUGH_BACKGROUNDS, composeOverlay, invertColor } from "../mouse/overlay";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import {
  SectionCard,
  WheelOverridesEditor,
  AppProfilePanel,
  SideKeyAppPanel,
  GesturePad,
  ScreenMemoryPanel,
  LongPressRegistryPanel,
  CurvePlayground,
  Dir16Grid,
  TremorAutoTune,
  TelemetryPanel,
  PackPanel,
  DevicePackPanel,
  EvidencePanel,
} from "./MouseJ1Panels";
import "../../styles/mouse-j1.css";

type Cfg = Record<string, unknown>;

function useSection<T extends Cfg>(section: J1Section): [T, (patch: Partial<T>) => void] {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const value = { ...(J1_DEFAULTS[section] as T), ...(j1Store.get(section) as unknown as T) };
  const set = (patch: Partial<T>): void => {
    try {
      j1Store.set(section, patch as Cfg);
    } catch (e) {
      pushToast("error", "配置落盘失败", String(e));
    }
  };
  return [value, set];
}

/** 分组容器（纵深层级：组标题 + F 号锚 + 说明 + 内容）。 */
function Group(props: { title: string; f: string; desc: string; children: React.ReactNode }): React.ReactElement {
  return (
    <section className="j1-group">
      <header className="j1-group-head">
        <h4>{props.title}</h4>
        <span className="j1-fnum">{props.f}</span>
        <p>{props.desc}</p>
      </header>
      <div className="j1-group-body">{props.children}</div>
    </section>
  );
}

/** 三件套行：名称 / 一句话说明 / 调节控件。 */
function Row(props: { label: string; hint: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="j1-row">
      <div className="j1-row-text">
        <span className="j1-row-label">{props.label}</span>
        <span className="j1-row-hint">{props.hint}</span>
      </div>
      <div className="j1-row-ctl">{props.children}</div>
    </div>
  );
}

function Toggle(props: { on: boolean; onChange: (v: boolean) => void; label: string }): React.ReactElement {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.on}
      aria-label={props.label}
      className={props.on ? "j1-switch j1-switch--on" : "j1-switch"}
      onClick={() => props.onChange(!props.on)}
    >
      <span className="j1-switch-knob" />
    </button>
  );
}

/** 贝塞尔编辑器（F601：两控制点拖拽即时预览 + 20 点增益对拍表）。 */
function BezierEditor(props: { cfg: CurveConfig; onChange: (p: Partial<CurveConfig>) => void }): React.ReactElement {
  const boxRef = useRef<HTMLDivElement>(null);
  const dragging = useRef<1 | 2 | null>(null);
  const W = 220;
  const H = 140;

  const pt = (cx: number, cy: number): { x: number; y: number } => ({ x: 12 + cx * (W - 24), y: H - 12 - cy * (H - 24) });
  const onPointerDown = (which: 1 | 2) => (e: React.PointerEvent) => {
    dragging.current = which;
    (e.target as Element).setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragging.current) return;
    const r = boxRef.current?.getBoundingClientRect();
    if (!r) return;
    const cx = Math.max(0, Math.min(1, (e.clientX - r.left - 12) / (W - 24)));
    const cy = Math.max(0, Math.min(1, (H - 12 - (e.clientY - r.top)) / (H - 24)));
    props.onChange(dragging.current === 1 ? { cp1x: cx, cp1y: cy } : { cp2x: cx, cp2y: cy });
  };
  const stop = (): void => {
    dragging.current = null;
  };

  const path = useMemo(() => {
    const pts: string[] = [];
    for (let i = 0; i <= 40; i++) {
      const t = i / 40;
      const u = 1 - t;
      const x = 3 * u * u * t * props.cfg.cp1x + 3 * u * t * t * props.cfg.cp2x + t * t * t;
      const y = 3 * u * u * t * props.cfg.cp1y + 3 * u * t * t * props.cfg.cp2y + t * t * t;
      const p = pt(x, y);
      pts.push(`${p.x.toFixed(1)},${p.y.toFixed(1)}`);
    }
    return pts.join(" ");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.cfg.cp1x, props.cfg.cp1y, props.cfg.cp2x, props.cfg.cp2y]);

  const table = gainTable20(props.cfg.id, props.cfg);
  const p1 = pt(props.cfg.cp1x, props.cfg.cp1y);
  const p2 = pt(props.cfg.cp2x, props.cfg.cp2y);

  return (
    <div className="j1-bezier">
      <div
        ref={boxRef}
        className="j1-bezier-canvas"
        style={{ width: W, height: H }}
        onPointerMove={onPointerMove}
        onPointerUp={stop}
        role="application"
        aria-label="贝塞尔曲线编辑器：拖拽两个控制点，预览即时跟随"
      >
        <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
          <line x1={12} y1={H - 12} x2={W - 12} y2={12} className="j1-bezier-diag" />
          <polyline points={`12,${H - 12} ${p1.x},${p1.y}`} className="j1-bezier-arm" />
          <polyline points={`${W - 12},12 ${p2.x},${p2.y}`} className="j1-bezier-arm" />
          <polyline points={path} className="j1-bezier-curve" />
          <circle cx={p1.x} cy={p1.y} r={7} className="j1-bezier-handle" onPointerDown={onPointerDown(1)} aria-label="控制点 1" />
          <circle cx={p2.x} cy={p2.y} r={7} className="j1-bezier-handle" onPointerDown={onPointerDown(2)} aria-label="控制点 2" />
        </svg>
      </div>
      <table className="j1-gain-table" aria-label="增益对拍表（输入-输出位移 20 点采样）">
        <thead>
          <tr>
            <th>输入 px</th>
            <th>输出 px</th>
            <th>增益</th>
          </tr>
        </thead>
        <tbody>
          {table.map((r) => (
            <tr key={r.inPx}>
              <td>{r.inPx}</td>
              <td>{r.outPx}</td>
              <td>{(r.outPx / r.inPx).toFixed(2)}×</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function MouseJ1Tab(): React.ReactElement {
  const [curve, setCurve] = useSection<CurveConfig & Cfg>("curve");
  const [slow, setSlow] = useSection<{ enabled: boolean; ratio: number; key: SlowTuneKey }>("slowTune");
  const [lift, setLift] = useSection<{ enabled: boolean }>("liftFilter");
  const [auto, setAuto] = useSection<{ enabled: boolean; deadZonePx: number; maxPx: number }>("autoscroll");
  const [wheel, setWheel] = useSection<WheelNotchConfig & Cfg>("wheelNotch");
  const [tilt, setTilt] = useSection<{ enabled: boolean; colsPerNotch: number; repeatDelayMs: number; repeatRateMs: number; hasTilt: boolean }>("tiltWheel");
  const [seam, setSeam] = useSection<{ enabled: boolean }>("seamGuard");
  const [magnet, setMagnet] = useSection<{ enabled: boolean; radiusPx: number }>("magnet");
  const [drag, setDrag] = useSection<{ enabled: boolean; bandPx: number }>("dragScroll");
  const [hover, setHover] = useSection<{ menuDelayMs: number; tooltipDelayMs: number }>("hoverTiming");
  const [tremor, setTremor] = useSection<{ level: "off" | "light" | "strong" }>("tremor");
  const [gain, setGain] = useSection<{ enabled: boolean; minLines: number; maxLines: number }>("wheelGain");
  const [memory, setMemory] = useSection<{ enabled: boolean }>("screenMemory");
  const [devices, setDevices] = useSection<{ profiles: DeviceProfile[]; notifyOnClone: boolean }>("devices");
  const [side, setSide] = useSection<{ global: Record<string, { kind: string; action?: string; keys?: string; appId?: string }>; apps: Record<string, Record<string, unknown>> }>("sideButtons");
  const [gest, setGest] = useSection<{ enabled: boolean; trailFadeMs: number; custom: Record<string, unknown> }>("gestures");
  const [pass, setPass] = useSection<{ enabled: boolean; exemptTypes: string[] }>("passthrough");
  const [lp, setLp] = useSection<{ scale: number; registry: Record<string, number> }>("longPress");
  const [ov, setOv] = useSection<{ outline: boolean; shadow: boolean; ring: boolean }>("overlay");
  const [hasTiltHw] = useState<boolean>(() => {
    // 能力检测：PointerEvent with tiltX 支持探测（无硬件面板隐藏开关、只留说明）。
    try {
      const ev = new PointerEvent("pointermove");
      return "tiltX" in ev && (ev as PointerEvent & { tiltX?: number }).tiltX !== undefined;
    } catch {
      return false;
    }
  });

  const audit = longPressDefaultAudit({ scale: lp.scale, registry: lp.registry });
  const seamFull = { ...SEAM_GUARD_PRESET, enabled: seam.enabled };
  const spectrum = (lv: "light" | "strong") =>
    ([2, 4, 6] as const).map((hz) => ({ hz, ...tremorSpectrum(lv, hz, 1.2) }));

  const undo = (s: J1Section, name: string): void => {
    if (j1Store.undoSection(s)) pushToast("success", `已还原「${name}」上一态`);
    else pushToast("info", "没有可回退的历史");
  };

  const resetAll = (): void => {
    void (async () => {
      const ok = await askConfirm({
        title: "恢复鼠标域默认",
        body: "将把 J 鼠标域（F601-F620）全部 20 项恢复为系统默认档。此操作可撤销（每节保留最近 3 次快照）。继续吗？",
      });
      if (!ok) return;
      for (const s of Object.keys(J1_DEFAULTS) as J1Section[]) j1Store.set(s, { ...(J1_DEFAULTS[s] as Cfg) });
      pushToast("success", "鼠标域已恢复默认");
    })();
  };

  const removeDevice = (key: string): void => {
    void (async () => {
      const p = devices.profiles.find((d) => d.deviceKey === key);
      const ok = await askConfirm({ title: "删除设备档案", body: `删除「${p?.name ?? key}」的鼠标档案？删除后该设备下次插入将按新设备重新建档。` });
      if (!ok) return;
      setDevices({ profiles: devices.profiles.filter((d) => d.deviceKey !== key) });
    })();
  };

  const errs = devices.profiles.flatMap((p) => validateDeviceProfile(p).map((e) => `${p.name}: ${e}`));

  return (
    <div className="j1-tab">
      <p className="j1-desc">
        鼠标与指针的手感、自定义与可见性（J 域一分队 · F601-F620）。全部改动即时生效，每节可一键还原上一态；基线（双击速度/指针方案）在「输入手感」页。
      </p>

      <div className="j1-toolbar">
        <button type="button" onClick={resetAll}>恢复域默认</button>
        <span className="j1-toolbar-note">档案 {devices.profiles.length}/{DEVICE_PROFILE_CAP} · 旋钮 {LONG_PRESS_LABELS[String(lp.scale)] ?? "标准 1x"}</span>
      </div>

      <Group title="指针速度曲线谱" f="F601" desc="加速不是开关二选一而是曲线族；「线性 1:1」与输入手感的关加速档是同一事实源。">
        <Row label="曲线族" hint="换曲线即时生效、试错零成本。">
          <select value={curve.id} onChange={(e) => setCurve({ id: e.target.value as CurveId })} aria-label="速度曲线">
            {CURVE_LIBRARY.map((c) => (
              <option key={c.id} value={c.id}>{c.name}</option>
            ))}
          </select>
        </Row>
        <Row label="全局灵敏度" hint="对拍表按灵敏度 1.0 出表。">
          <input
            type="range" min={0.2} max={3} step={0.05} value={curve.sens}
            onChange={(e) => setCurve({ sens: Number(e.target.value) })}
            aria-label="全局灵敏度"
          />
          <span className="j1-num">{curve.sens.toFixed(2)}×</span>
        </Row>
        {curve.id === "custom" && (
          <Row label="自定义曲线" hint="拖拽两个控制点，曲线即时跟随；右侧为 20 点增益对拍表。">
            <BezierEditor cfg={curve} onChange={(p) => setCurve(p)} />
          </Row>
        )}
        <Row label="曲线说明" hint={CURVE_LIBRARY.find((c) => c.id === curve.id)?.desc ?? ""}>
          <button type="button" onClick={() => undo("curve", "速度曲线")}>还原上一态</button>
        </Row>
        <SectionCard title="示例区与人群预设" f="F601">
          <CurvePlayground
            cfg={curve}
            onPreset={(p) => {
              setCurve({ id: p.curve, sens: p.sens });
              pushToast("success", `已切换预设（曲线 ${p.curve} · ${p.sens}×）`);
            }}
          />
        </SectionCard>
      </Group>

      <Group title="慢速微调" f="F602" desc="按住修饰键指针立刻「听话变慢」——精确落点的确定性优先；修饰键占用已登记进快捷键冲突审计。">
        <Row label="启用慢速微调" hint="默认开：不碰修饰键的人完全无感。">
          <Toggle on={slow.enabled} onChange={(v) => setSlow({ enabled: v })} label="启用慢速微调" />
        </Row>
        <Row label="修饰键" hint={slowTuneRegistryRow({ ...slow, enabled: slow.enabled }).conflictsWith.join("、") + "——冲突由审计表裁决。"}>
          <select value={slow.key} onChange={(e) => setSlow({ key: e.target.value as SlowTuneKey })} aria-label="修饰键">
            {SLOW_TUNE_KEYS.map((k) => (
              <option key={k.id} value={k.id}>{k.name}</option>
            ))}
          </select>
        </Row>
        <Row label="降速档" hint="步进式降档（非平滑减速）：5% / 10% / 20% 三选，1px 步进可达。">
          <select value={String(slow.ratio)} onChange={(e) => setSlow({ ratio: Number(e.target.value) })} aria-label="降速档">
            {SLOW_TUNE_RATIOS.map((r) => (
              <option key={r} value={String(r)}>{Math.round(r * 100)}%</option>
            ))}
          </select>
        </Row>
      </Group>

      <Group title="滤波与手抖" f="F603 / F611" desc="抬笔滤波吃掉「松手前抖一下」（管线零延迟）；手抖过滤是无障碍件，默认关，只压高频不压意图。">
        <Row label="抬笔滤波" hint="按键抬起后 8ms 窗口内末位移按 50% 折算——快速移动完全无感。">
          <Toggle on={lift.enabled} onChange={(v) => setLift({ enabled: v })} label="抬笔滤波" />
        </Row>
        <Row label="手抖过滤" hint="帕金森/震颤用户「停得住」；快速大幅度移动零衰减直通。">
          <select value={tremor.level} onChange={(e) => setTremor({ level: e.target.value as "off" | "light" | "strong" })} aria-label="手抖过滤档">
            <option value="off">关（默认）</option>
            <option value="light">轻（吃 0.5px 内抖动）</option>
            <option value="strong">强（吃 2px 内抖动）</option>
          </select>
        </Row>
        {tremor.level !== "off" && (
          <Row label="过滤效果谱" hint={`2/4/6Hz 震颤注入的残余幅度比（0=全滤除）；${TREMOR_LEVELS[tremor.level].name}档当前样本：`}>
            <table className="j1-gain-table j1-gain-table--mini">
              <thead>
                <tr><th>频段</th><th>残余比</th></tr>
              </thead>
              <tbody>
                {spectrum(tremor.level).map((r) => (
                  <tr key={r.hz}><td>{r.hz}Hz</td><td>{Math.round(r.residualRatio * 100)}%</td></tr>
                ))}
              </tbody>
            </table>
          </Row>
        )}
        <SectionCard title="自动调谐（无障碍引导）" f="F611">
          <TremorAutoTune />
        </SectionCard>
      </Group>

      <Group title="滚轮手感" f="F605 / F606 / F612 / F618" desc="逐档与平滑两派手感各有主场；应用覆盖优先于全局；穿透让阅读一路到底。">
        <Row label="全局刻度档" hint="「按应用默认」= 文档/代码逐档、浏览器/长列表平滑。">
          <select value={wheel.mode} onChange={(e) => setWheel({ mode: e.target.value as WheelMode })} aria-label="全局刻度档">
            {WHEEL_MODES.map((m) => (
              <option key={m.id} value={m.id}>{m.name}</option>
            ))}
          </select>
        </Row>
        <Row label="每格行数" hint={`逐档基准 ${notchLines(wheel.linesPerNotch)} 行/格（Windows 同款 3 行）。`}>
          <input type="range" min={1} max={12} value={wheel.linesPerNotch} onChange={(e) => setWheel({ linesPerNotch: Number(e.target.value) })} aria-label="每格行数" />
          <span className="j1-num">{wheel.linesPerNotch} 行</span>
        </Row>
        <Row label="自适应增益" hint="轻滚逐行精读、快滚一撸到底（最高 12 行/格）；逐档应用自动豁免。">
          <Toggle on={gain.enabled} onChange={(v) => setGain({ enabled: v })} label="自适应增益" />
        </Row>
        <Row label="滚轮穿透" hint={`悬停装饰浮层时滚轮作用其下容器（阅读多数派）；豁免类型：${pass.exemptTypes.join(" / ")}。`}>
          <Toggle on={pass.enabled} onChange={(v) => setPass({ enabled: v })} label="滚轮穿透" />
        </Row>
        {hasTiltHw && (
          <Row label="倾斜滚轮" hint="左/右倾斜映射水平滚动，一次 3 列（与垂直对称）；Shift+滚轮为等效入口。">
            <Toggle on={tilt.enabled} onChange={(v) => setTilt({ enabled: v })} label="倾斜滚轮" />
          </Row>
        )}
        {!hasTiltHw && (
          <Row label="倾斜滚轮" hint="当前环境未检测到倾斜轮硬件；Shift+滚轮横向滚动始终可用。">
            <span className="j1-readonly">按能力自动隐藏</span>
          </Row>
        )}
        <Row label="穿透类型语义表" hint="白名单按控件类型登记（审计口径）：">
          <table className="j1-gain-table j1-gain-table--mini">
            <tbody>
              {PASSTHROUGH_TYPE_SEMANTICS.map((t) => (
                <tr key={t.type}><td>{t.type}</td><td>{t.meaning}</td></tr>
              ))}
            </tbody>
          </table>
        </Row>
        <SectionCard title="应用覆盖编辑器" f="F605">
          <WheelOverridesEditor />
        </SectionCard>
      </Group>

      <Group title="自动滚动" f="F604 / F609" desc="中键锚点滚长文档；拖文件到容器边缘它自己开始滚——油门在指针深入量。">
        <Row label="中键自动滚动" hint="按一下、推一推、点一下退出（Windows 肌肉记忆）；默认开。">
          <Toggle on={auto.enabled} onChange={(v) => setAuto({ enabled: v })} label="中键自动滚动" />
        </Row>
        <Row label="锚点速度" hint={`死区 ${auto.deadZonePx}px 起步、${auto.maxPx}px 封顶（16 方位采样对拍 Windows）。`}>
          <input type="range" min={40} max={120} value={auto.maxPx} onChange={(e) => setAuto({ maxPx: Number(e.target.value) })} aria-label="锚点封顶速度" />
          <span className="j1-num">{auto.maxPx}px</span>
        </Row>
        <Row label="拖拽边缘自动滚" hint={`边缘 ${drag.bandPx || DRAG_BAND_PX}px 触发带、深入三档（${edgeScrollSpeed(4)}/${edgeScrollSpeed(16)}/${edgeScrollSpeed(22)}px/帧）；仅对声明容器生效。`}>
          <Toggle on={drag.enabled} onChange={(v) => setDrag({ enabled: v })} label="拖拽边缘自动滚" />
        </Row>
        <Row label="16 方位映射抽查" hint="锚点偏移 → 方向档（0=右，顺时针）：">
          <span className="j1-readonly">
            {[0, 45, 90, 135, 180, 225, 270, 315].map((deg) => {
              const r = Math.cos((deg * Math.PI) / 180);
              const s = Math.sin((deg * Math.PI) / 180);
              return ` ${deg}°→${autoscrollVelocity(r * 40, s * 40, { ...AUTOSCROLL_PRESET, enabled: true }).dirIndex}`;
            })}
          </span>
        </Row>
        <SectionCard title="16 方位可视化与速度语义" f="F604">
          <Dir16Grid />
          <p className="j1x-hint">16 格 = 锚点四周方位档；数字即 quantizeDirection 输出——锚点滚动方向由所在格决定，速度随离锚距离线性（{auto.deadZonePx}px 起步、{auto.maxPx}px 封顶）。</p>
        </SectionCard>
      </Group>

      <Group title="跨屏与落点" f="F607 / F613" desc="接缝护边 4px/200ms 防勾绊、四角 8px 秒达；每块屏记住指针最后落点（EDID 指纹为键，换线不乱）。">
        <Row label="接缝护边" hint={`穿越需在接缝 ${seamFull.edgePx}px 内停留 ${seamFull.dwellMs}ms；四角 ${seamFull.cornerPx}px 豁免（热角秒达）。`}>
          <Toggle on={seam.enabled} onChange={(v) => setSeam({ enabled: v })} label="接缝护边" />
        </Row>
        <Row label="跨屏落点记忆" hint="锁屏唤醒/KVM 切回时指针回到离开的地方；单屏环境自然休眠。">
          <Toggle on={memory.enabled} onChange={(v) => setMemory({ enabled: v })} label="跨屏落点记忆" />
        </Row>
        <Row label="角落豁免自检" hint="四角坐标（0,0 / 右上 / 左下 / 右下）应全部豁免：">
          <span className="j1-readonly">
            {JSON.stringify(cornerExempt([{ id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "t", scale: 1 }], 4, 4, 8))}
            {" / "}
            {JSON.stringify(cornerExempt([{ id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "t", scale: 1 }], 1916, 1076, 8))}
          </span>
        </Row>
        <SectionCard title="跨屏记忆点管理" f="F613">
          <ScreenMemoryPanel />
        </SectionCard>
      </Group>

      <Group title="磁吸与悬停" f="F608 / F610" desc="磁吸是「帮助对准」不是「抢走控制权」（默认关、判定零偏移）；悬停节奏两把旋钮，点击展开永远即时。">
        <Row label="指针磁吸对齐" hint="接近小目标 12px 内视觉微移对齐；大目标（≥24px）不吸；关闭后零干预。">
          <Toggle on={magnet.enabled} onChange={(v) => setMagnet({ enabled: v })} label="指针磁吸对齐" />
        </Row>
        <Row label="磁吸半径" hint="三档：8 / 12 / 16px。">
          <select value={String(magnet.radiusPx)} onChange={(e) => setMagnet({ radiusPx: Number(e.target.value) })} aria-label="磁吸半径">
            {MAGNET_RADII.map((r) => (
              <option key={r} value={String(r)}>{r}px</option>
            ))}
          </select>
        </Row>
        <Row label="菜单子级展开延迟" hint="急性子调快、手抖党调慢；默认 400ms（Windows 同款）。">
          <select value={String(hover.menuDelayMs)} onChange={(e) => setHover({ menuDelayMs: Number(e.target.value) })} aria-label="菜单展开延迟">
            {HOVER_DELAY_STEPS.map((v) => (
              <option key={v} value={String(v)}>{v}ms</option>
            ))}
          </select>
        </Row>
        <Row label="Tooltip 出现延迟" hint="独立旋钮（基线 500ms）；跟随/翻转行为不受档位影响。">
          <select value={String(hover.tooltipDelayMs)} onChange={(e) => setHover({ tooltipDelayMs: Number(e.target.value) })} aria-label="Tooltip 延迟">
            {HOVER_DELAY_STEPS.map((v) => (
              <option key={v} value={String(v)}>{v}ms</option>
            ))}
          </select>
        </Row>
      </Group>

      <Group title="设备与应用档案" f="F614 / F616" desc="每只鼠标各记一套手感（上限 10 台、超出淘汰最久未用）；同一只鼠标在不同应用里各有性格（增量切换防跳变）。">
        {errs.length > 0 && (
          <p className="j1-warning" role="alert">档案校验发现问题：{errs.join("；")}</p>
        )}
        {devices.profiles.length === 0 && (
          <Row label="还没有设备档案" hint="插入鼠标后自动建档（首插克隆当前默认 + 气泡提示），或从下方手动添加。">
            <button
              type="button"
              onClick={() =>
                setDevices({
                  profiles: [
                    ...devices.profiles,
                    { id: `dev-manual-${Date.now()}`, name: "手动档案", deviceKey: `manual:${devices.profiles.length + 1}`, params: { sens: 1, curve: "classic", wheelMode: "per-app" }, createdAt: Date.now(), lastUsedAt: Date.now() },
                  ],
                })
              }
            >
              添加手动档案
            </button>
          </Row>
        )}
        {devices.profiles.map((p) => (
          <Row key={p.id} label={p.name} hint={`识别键 ${p.deviceKey} · 曲线 ${p.params.curve} · 灵敏度 ${p.params.sens}× · 滚轮 ${p.params.wheelMode}`}>
            <span className="j1-row-actions">
              <input
                type="range" min={0.2} max={3} step={0.05} value={p.params.sens}
                aria-label={`${p.name} 灵敏度`}
                onChange={(e) =>
                  setDevices({
                    profiles: devices.profiles.map((d) => (d.id === p.id ? { ...d, params: { ...d.params, sens: Number(e.target.value) } } : d)),
                  })
                }
              />
              <button type="button" onClick={() => removeDevice(p.deviceKey)}>删除</button>
            </span>
          </Row>
        ))}
        <Row label="首插气泡提示" hint="新设备建档时提示「已为此设备建档」——不静默改手感。">
          <Toggle on={devices.notifyOnClone} onChange={(v) => setDevices({ notifyOnClone: v })} label="首插气泡提示" />
        </Row>
        <SectionCard title="设备档案导入导出" f="F614">
          <DevicePackPanel />
        </SectionCard>
        <SectionCard title="应用级档案（正交第二维）" f="F616">
          <AppProfilePanel />
        </SectionCard>
      </Group>

      <Group title="侧键与手势" f="F615 / F617" desc="侧键全局默认后退/前进、应用可覆盖；右键手势默认关——没画完就是右键菜单，菜单永远兜底。">
        {FIVE_BUTTON_MAP.map((b) => (
          <Row key={b.button} label={b.name} hint={b.remappable ? "映射目标：系统动作 / 快捷键 / 启动应用。" : "主键不可重映射（系统语义）。"}>
            {b.remappable ? (
              <select
                value={side.global[String(b.button)]?.kind === "action" ? side.global[String(b.button)]?.action : "custom"}
                aria-label={`${b.name} 映射`}
                onChange={(e) => {
                  const v = e.target.value;
                  setSide({
                    global: {
                      ...side.global,
                      [String(b.button)]: v.startsWith("custom:") ? { kind: "shortcut", keys: v.slice(7) } : { kind: "action", action: v },
                    },
                  });
                }}
              >
                {SIDE_ACTIONS.map((a) => (
                  <option key={a.id} value={a.id}>{a.name}</option>
                ))}
                <option value="custom:Ctrl+Shift+V">快捷键（示例）</option>
              </select>
            ) : (
              <span className="j1-readonly">固定</span>
            )}
          </Row>
        ))}
        <Row label="右键手势层" hint="按住右键画轨迹触发动作；墨迹 120ms 淡出；无轨迹松开=右键菜单（零误伤）。">
          <Toggle on={gest.enabled} onChange={(v) => setGest({ enabled: v })} label="右键手势层" />
        </Row>
        {gest.enabled && (
          <Row label="内置手势库" hint="八方向编码（容错两向偏差）——识别率判据 ≥95%（200 例样本）：">
            <table className="j1-gain-table j1-gain-table--mini">
              <tbody>
                {BUILTIN_GESTURES.map((g) => (
                  <tr key={g.id}>
                    <td>{g.name}</td>
                    <td>{g.dirs.join("→")}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Row>
        )}
        <Row label="优先级矩阵" hint={sideGesturePriorityMatrix(Object.keys(side.global).length > 0, gest.enabled)}>
          <span />
        </Row>
        <SectionCard title="自定义手势录制台" f="F617">
          <GesturePad />
        </SectionCard>
        <SectionCard title="侧键应用覆盖与冲突审计" f="F615">
          <SideKeyAppPanel />
        </SectionCard>
      </Group>

      <Group title="长按与衬底" f="F619 / F620" desc="全系统长按统一旋钮（藏在进阶位——普通用户不该被问「长按多长」）；指针衬底让复杂壁纸上永远找得到箭头。">
        <Row label="长按时长档" hint="触屏菜单 500ms / 磁贴 500ms / ClickLock 1100ms 统一跟随缩放；默认 1x = 现行值。">
          <select value={String(lp.scale)} onChange={(e) => setLp({ scale: Number(e.target.value) })} aria-label="长按时长档">
            {LONG_PRESS_SCALES.map((s) => (
              <option key={s} value={String(s)}>{LONG_PRESS_LABELS[String(s)]}</option>
            ))}
          </select>
        </Row>
        <Row label="缩放对账" hint="1x 档各功能值 = 现行基线（对账脚本同源）：">
          <table className="j1-gain-table j1-gain-table--mini">
            <tbody>
              {audit.map((a) => (
                <tr key={a.feature}>
                  <td>{a.feature}</td>
                  <td>{longPressMs(a.base, lp.scale)}ms</td>
                  <td className={a.ok ? "" : "j1-bad"}>{a.ok ? "✓" : "偏差"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Row>
        <Row label="反色描边" hint="1px，自动取指针色相反色——默认开（可见性底线）。">
          <Toggle on={ov.outline} onChange={(v) => setOv({ outline: v })} label="反色描边" />
        </Row>
        <Row label="柔投影" hint="4px 偏移 30% 透明——默认关（干净优先）。">
          <Toggle on={ov.shadow} onChange={(v) => setOv({ shadow: v })} label="柔投影" />
        </Row>
        <Row label="高对比衬圈" hint="3px 圆衬——默认关。">
          <Toggle on={ov.ring} onChange={(v) => setOv({ ring: v })} label="高对比衬圈" />
        </Row>
        <Row label="衬底预览" hint="三底色走查（深/浅/花）——描边色即反色结果：">
          <span className="j1-swatch-row">
            {OVERLAY_WALKTHROUGH_BACKGROUNDS.map((bg) => (
              <span
                key={bg.id}
                className="j1-swatch"
                title={`${bg.name} · 描边 ${composeOverlay(ov).active ? invertColor("#ffffff") : "关"}`}
                style={{ background: bg.color, boxShadow: composeOverlay(ov).boxShadow || undefined, filter: composeOverlay(ov).cssFilter || undefined }}
              >
                <svg width={20} height={20} viewBox="0 0 32 32"><path d="M2 2 L2 24.5 L8.2 19.6 L12.2 28.6 L15.8 27 L11.9 18.3 L19.4 17.6 Z" fill="#fff" /></svg>
              </span>
            ))}
          </span>
        </Row>
        <SectionCard title="长按功能登记表" f="F619">
          <LongPressRegistryPanel />
        </SectionCard>
      </Group>

      <Group title="遥测与证据" f="十三/十三·补 + MD3 附B" desc="体验日志还原每一次操作（狂点/死点自动标记，隐私红线：不记内容只记行为）；证据包把全部对拍表收敛成一份可归档 JSON。">
        <SectionCard title="体验日志" f="十三">
          <TelemetryPanel />
        </SectionCard>
        <SectionCard title="判据证据包" f="附B">
          <EvidencePanel />
        </SectionCard>
        <SectionCard title="鼠标档案打包（vxtheme 对接）" f="F623">
          <PackPanel />
        </SectionCard>
      </Group>
    </div>
  );
}
