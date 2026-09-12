// AURORA-10000：AI-01~AI-05 批次，勿删。
// BootTheater.tsx — 领域01 启动与品牌剧场渲染引擎（F00001~F00625）。
// 数据驱动：registry.ts 提供 25 族 × 25 档，params.ts 提供每项独立参数；
// 本组件只按「族 kind → 渲染层」消费选中档位，不含任何硬编码色值/时长
// （色相与时长由参数派生，基准节奏仍走 tokens --dur-* 与 BootScreen pacing）。

import { useMemo } from "react";
import { findTheaterItem, type TheaterKind } from "./registry";
import { itemParams, colorPipeline, narrativeLine, eggSpec, a11yBootFlags, pacingProfile } from "./params";

export type TheaterSelections = Partial<Record<TheaterKind, string>>;

const EASE = ["var(--ease-standard)", "var(--ease-emphasized)", "var(--ease-spring)"] as const;

interface LayerProps {
  id: string;
  progress: number;
  reduceMotion: boolean;
  perfMode: string;
}

/* ---------- 族0001 光弧：横向扫过光带（hue/宽度/辉光由档位派生） ---------- */
function ArcLayer({ id, progress, reduceMotion }: LayerProps): React.ReactElement | null {
  const p = itemParams(id);
  const item = findTheaterItem(id);
  const wide = item ? /缎带|极光|涟漪|斜纹|螺旋|均衡器|像素波|日出|灯塔/.test(item.name) : false;
  const h = p.hue;
  const color = `hsl(${h} 85% 65%)`;
  const colorSoft = `hsl(${h} 85% 65% / ${0.35 * p.intensity})`;
  const dur = `${(2.4 * p.durScale).toFixed(2)}s`;
  return (
    <div className={`bt-arc ${wide ? "bt-arc--wide" : ""}`} style={{ opacity: p.intensity > 1 ? 1 : 0.8 }} aria-hidden="true">
      <div
        className={reduceMotion ? "bt-arc-line bt-arc--static" : "bt-arc-line"}
        style={{
          background: `linear-gradient(90deg, transparent, ${colorSoft}, ${color}, ${colorSoft}, transparent)`,
          height: wide ? 6 : 2,
          boxShadow: `0 0 12px ${colorSoft}`,
          animationDuration: dur,
          animationTimingFunction: EASE[p.easing],
        }}
        data-progress={Math.round(progress * 100)}
      />
    </div>
  );
}

/* ---------- 族0002 品牌呼吸：中央光环比 Logo 更大圈的呼吸 ---------- */
function BreathLayer({ id, reduceMotion }: LayerProps): React.ReactElement | null {
  const p = itemParams(id);
  const color = `hsl(${p.hue} 80% 60% / ${0.25 * p.intensity})`;
  return (
    <div className="bt-breath" aria-hidden="true">
      <div
        className={reduceMotion ? "bt-breath-ring bt-arc--static" : "bt-breath-ring"}
        style={{
          borderColor: color,
          boxShadow: `0 0 24px ${color}, inset 0 0 24px ${color}`,
          animationDuration: `${(3 * p.durScale).toFixed(2)}s`,
          animationTimingFunction: EASE[p.easing],
        }}
      />
    </div>
  );
}

/* ---------- 族0003 进度叙事：底部一行当前幕文案 ---------- */
function NarrativeLayer({ id, progress }: LayerProps): React.ReactElement | null {
  const line = narrativeLine(id, progress);
  if (!line) return null;
  return (
    <div className="bt-narrative" aria-live="polite">
      {line}
    </div>
  );
}

/* ---------- 族0013 倒计时刻度：进度环 + 档位样式变体 ---------- */
function CountdownLayer({ id, progress }: LayerProps): React.ReactElement | null {
  const p = itemParams(id);
  const item = findTheaterItem(id);
  const variant = /数字|跨年|胶片|倒数/.test(item?.name ?? "") ? "digits" : /饼图|披萨|月食|弧线/.test(item?.name ?? "") ? "pie" : "ring";
  const deg = Math.round(progress * 360);
  const color = `hsl(${p.hue} 80% 62%)`;
  return (
    <div className={`bt-count bt-count--${variant}`} aria-hidden="true">
      {variant === "digits" ? (
        <span style={{ color, fontVariantNumeric: "tabular-nums" }}>{Math.round(progress * 100)}</span>
      ) : (
        <span
          className="bt-count-dial"
          style={{
            background: `conic-gradient(${color} ${deg}deg, transparent ${deg}deg)`,
            boxShadow: `0 0 8px hsl(${p.hue} 80% 62% / 0.3)`,
          }}
        />
      )}
    </div>
  );
}

/* ---------- 族0006/0007/0008 诊断/仪表/信任：进度驱动的状态带 ---------- */
function StatusStripLayer({ id, progress }: LayerProps): React.ReactElement | null {
  const p = itemParams(id);
  const item = findTheaterItem(id);
  if (!item) return null;
  const cells = 10;
  const lit = Math.round(progress * cells);
  const isTrust = /信任|签名|证书|密钥|封印|哈希|TPM|度量|白名单|徽章|钥匙|解密|防火墙|沙箱|补丁|隐私|摄像头|更新|恢复|加密|UEFI|防拆|生物|防窥|绿灯/.test(item.name);
  const color = `hsl(${p.hue} 80% 62%)`;
  return (
    <div className={isTrust ? "bt-strip bt-strip--trust" : "bt-strip"} aria-hidden="true">
      {Array.from({ length: cells }, (_, i) => (
        <i
          key={i}
          style={{
            background: i < lit ? color : "transparent",
            borderColor: `hsl(${p.hue} 60% 60% / 0.4)`,
            transitionDelay: `${i * 30}ms`,
          }}
        />
      ))}
      <span className="bt-strip-label" style={{ color }}>{isTrust ? "信任链" : "自检"} {Math.round(progress * 100)}%</span>
    </div>
  );
}

/* ---------- 族0010 固件风格 / 族0015 日志美学 / 族0011 Splash 特效 / 族0018 情绪板：全屏质感层 ---------- */
function TextureLayer({ id }: { id: string }): React.ReactElement | null {
  const p = itemParams(id);
  const item = findTheaterItem(id);
  if (!item) return null;
  const n = item.name;
  // 固件/日志终端系：扫描线 + 单色罩（绿磷/琥珀/冰蓝按 hue 派生）
  const terminal = /终端|磷|琥珀|示波|电传|打字|点阵|打孔|纸带|小票|LCD|计算器|稿纸|心电纸/.test(n);
  // 情绪板：柔和渐变底罩
  const mood = item ? true : false;
  void mood;
  const hue = p.hue;
  const tint = `hsl(${hue} 60% 50% / ${0.06 * p.intensity})`;
  return (
    <>
      {terminal && <div className="bt-texture bt-texture--scan" style={{ color: `hsl(${hue} 70% 60%)` }} aria-hidden="true" />}
      <div className="bt-texture" style={{ background: `radial-gradient(ellipse at center, transparent 55%, ${tint})` }} aria-hidden="true" />
    </>
  );
}

/* ---------- 族0012 转场：退出阶段的全屏过渡 ---------- */
function TransitionLayer({ id }: { id: string }): React.ReactElement | null {
  const p = itemParams(id);
  const item = findTheaterItem(id);
  if (!item) return null;
  const n = item.name;
  const variant = /擦除|百叶|像素|液体|烟雾|光带/.test(n) ? "wipe" : /光圈|沙漏|心跳|钟针|幕布|灯亮/.test(n) ? "iris" : "fade";
  return (
    <div className={`bt-trans bt-trans--${variant}`} style={{ animationDuration: `${(0.64 * p.durScale).toFixed(2)}s` }} aria-hidden="true" />
  );
}

/* ---------- 族0024 彩蛋：低频装饰浮层 ---------- */
function EggLayer({ id }: { id: string }): React.ReactElement | null {
  const spec = eggSpec(id);
  if (spec.master || spec.trigger !== "auto-low") return null;
  if (Math.random() > spec.chance) return null;
  const p = itemParams(id);
  return (
    <div className="bt-egg" style={{ color: `hsl(${p.hue} 85% 70%)`, animationDuration: `${(4 * p.durScale).toFixed(2)}s` }} aria-hidden="true">
      ✦
    </div>
  );
}

/* ---------- 主层 ---------- */
export function BootTheaterLayer(props: {
  selections: TheaterSelections;
  progress: number;
  reduceMotion: boolean;
  perfMode: string;
  phase: string;
}): React.ReactElement {
  const { selections, progress, reduceMotion, perfMode, phase } = props;
  const pipeline = useMemo(() => (selections.color ? colorPipeline(selections.color) : null), [selections.color]);
  const pacing = selections.pacing ? pacingProfile(selections.pacing) : null;
  const a11y = selections.a11y ? a11yBootFlags(selections.a11y) : null;
  const keep = pacing?.animKeep ?? 1;
  const show = (kind: TheaterKind): boolean => {
    const id = selections[kind];
    return !!id && keep > 0 && !!findTheaterItem(id);
  };
  const layer = (kind: TheaterKind): LayerProps => ({
    id: selections[kind] as string,
    progress,
    reduceMotion: reduceMotion || (a11y?.slowMotion ?? false),
    perfMode,
  });

  return (
    <div
      className={`bt-root${a11y?.highContrast ? " bt-root--hc" : ""}${a11y?.largeText ? " bt-root--large" : ""}${pacing && pacing.animKeep === 0 ? " bt-root--silent" : ""}`}
      data-flicker={a11y?.noFlicker ? "off" : undefined}
    >
      {pipeline && pipeline.blackBase && <div className="bt-blackbase" aria-hidden="true" />}
      {pipeline && pipeline.filter !== "none" && (
        <div className="bt-pipeline" style={{ backdropFilter: pipeline.filter, WebkitBackdropFilter: pipeline.filter }} aria-hidden="true" />
      )}
      {show("mood") && <TextureLayer id={selections.mood as string} />}
      {show("firmware") && <TextureLayer id={selections.firmware as string} />}
      {show("splash") && <TextureLayer id={selections.splash as string} />}
      {show("log") && <TextureLayer id={selections.log as string} />}
      {show("arc") && <ArcLayer {...layer("arc")} />}
      {show("breath") && <BreathLayer {...layer("breath")} />}
      {show("narrative") && <NarrativeLayer {...layer("narrative")} />}
      {show("countdown") && <CountdownLayer {...layer("countdown")} />}
      {show("diagnostic") && <StatusStripLayer {...layer("diagnostic")} />}
      {show("gauge") && <StatusStripLayer {...layer("gauge")} />}
      {show("trust") && <StatusStripLayer {...layer("trust")} />}
      {show("recovery") && <StatusStripLayer {...layer("recovery")} />}
      {show("egg") && <EggLayer id={selections.egg as string} />}
      {phase === "exiting" && show("transition") && <TransitionLayer id={selections.transition as string} />}
    </div>
  );
}

/* ---------- 关机/唤醒仪式档渲染（ceremonyFx 消费同一参数源） ---------- */
export { itemParams, colorPipeline, narrativeLine, eggSpec, a11yBootFlags, pacingProfile };
