import { useEffect, useReducer, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { BootAnim, BootPacing, PerfMode } from "../../lib/settings";
import { ceremonyReducer, INITIAL_CEREMONY, pacingTimings, SKIP_THRESHOLD } from "./ceremony";
import { BootWordmark } from "./BootWordmark";
import { CapsuleBar } from "./CapsuleBar";
import { FileTicker } from "./FileTicker";
import { BootAtmosphere } from "./BootAtmosphere";
import "../../styles/boot.css";

/**
 * 启动仪式（真实事件驱动）。
 *
 * 硬性规则（docs/ARCHITECTURE_V2.md §5）：
 * - 进度 100% 来自后端 `boot://event` 真实事件，无预设时间线；
 *   VARIABLE 八字母描边按每字母 12.5% 区间映射真实进度，rAF 只做视觉平滑。
 * - 阶段机（entering → streaming → readyHold → exiting）为纯 reducer（ceremony.ts），
 *   定时器全部由 pacingTimings(bootPacing) 驱动；instant 节奏 = 直通（同 bootAnim=none）。
 * - Esc/空格：进度 <30% 拒绝跳过（如实提示）；≥30% 跳过 UI，后台加载继续。
 *
 * 对外契约（App.tsx 依赖，勿改）：props { onDone, onExitStart?, onStats? }；
 * 导出 BootStats / LoadEventPayload；onExitStart 先于视觉退出、onDone 在其后。
 */

export interface BootStats {
  folders: number;
  records: number;
  mindmaps: number;
  nodes: number;
  edges: number;
  mediaFiles: number;
  attachments: number;
  workspaceFiles: number;
  workspaceFolders: number;
  workspaceBytes: number;
  mediaDirFiles: number;
  mediaDirBytes: number;
  backups: number;
  schemaVersion: number;
  version: string;
  portable: boolean;
  dataDir: string;
}

export interface LoadEventPayload {
  seq: number;
  progress: number;
  currentTask: string;
  filePath: string | null;
  fileCount: number | null;
  totalCount: number | null;
  icon: string;
  level: number;
  elapsedMs: number;
  timestamp: number;
  stats: BootStats | null;
}

const STALL_MS = 500;
const SLOW_TOTAL_MS = 5000;

function fmtBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${v} B` : `${v.toFixed(1)} ${units[u]}`;
}

export function BootScreen(props: {
  onDone: () => void;
  /** 退出编排开始（字母起飞前一刻）：宿主此刻挂载桌面 shell（beneath），设置加载同步启动。 */
  onExitStart?: () => void;
  /** ready 摘要（真实统计）上抛，供桌面"磁盘同步通知"使用。 */
  onStats?: (stats: BootStats) => void;
}): React.ReactElement {
  const [ceremony, dispatch] = useReducer(ceremonyReducer, INITIAL_CEREMONY);
  const [shown, setShown] = useState(0); // 平滑显示值（收敛于真实进度）
  const [current, setCurrent] = useState<{ task: string; filePath: string | null; level: number }>({
    task: "",
    filePath: null,
    level: 0,
  });
  const [stats, setStats] = useState<BootStats | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [fileCount, setFileCount] = useState<number | null>(null);
  const [totalCount, setTotalCount] = useState<number | null>(null);
  const [stalled, setStalled] = useState(false);
  const [denied, setDenied] = useState(false);
  const [exit, setExit] = useState<BootAnim | null>(null);
  // 氛围层性能口径：设置加载前用系统级 reduce-motion 预判，加载后跟随设置
  const [atmo, setAtmo] = useState<{
    reduceMotion: boolean;
    safeMode: boolean;
    perfMode: PerfMode;
  }>(() => ({
    reduceMotion:
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    safeMode: false,
    perfMode: "balanced",
  }));

  const progressRef = useRef(0); // 真实进度（单调不减）
  const lastSeqRef = useRef(0);
  const lastEventAtRef = useRef(Date.now());
  const finishedRef = useRef(false);
  const animRef = useRef<BootAnim>("full");
  const pacingRef = useRef<BootPacing>("cinematic");
  // U-05 启动交响：三层音景句柄 + 30%/80% 节拍脉冲各一次的触发标记
  const symphonyRef = useRef<import("../../lib/sounds").BootSymphony | null>(null);
  const pulsed30Ref = useRef(false);
  const pulsed80Ref = useRef(false);
  const onDoneRef = useRef(props.onDone);
  onDoneRef.current = props.onDone;
  const onExitStartRef = useRef(props.onExitStart);
  onExitStartRef.current = props.onExitStart;
  const onStatsRef = useRef(props.onStats);
  onStatsRef.current = props.onStats;

  /** 退出编排开始：full=实心化→光扫→落向任务栏；simple=640ms 交叉淡出；none/instant=直通。 */
  const finish = (): void => {
    if (finishedRef.current) return;
    finishedRef.current = true;
    // M-60 测试钩子：boot 完成事件 = e2e 就绪哨兵（监听 "variable:boot-ready"）
    window.dispatchEvent(new CustomEvent("variable:boot-ready", { detail: { anim: animRef.current } }));
    const a = animRef.current;
    setExit(a);
    if (a === "none" || pacingRef.current === "instant") {
      onExitStartRef.current?.();
      onDoneRef.current();
      dispatch({ type: "EXIT_DONE" });
      return;
    }
    if (a === "simple") {
      onExitStartRef.current?.();
      window.setTimeout(() => {
        onDoneRef.current();
        dispatch({ type: "EXIT_DONE" });
      }, 640);
      return;
    }
    // full：实心化 → 柔光横扫 → 字标+胶囊三段沉降落向任务栏 → 桌面就绪
    window.setTimeout(() => onExitStartRef.current?.(), 480);
    window.setTimeout(() => {
      onDoneRef.current();
      dispatch({ type: "EXIT_DONE" });
    }, 1880);
  };

  const apply = (ev: LoadEventPayload): void => {
    if (ev.seq <= lastSeqRef.current) return; // 回放去重
    lastSeqRef.current = ev.seq;
    lastEventAtRef.current = Date.now();
    progressRef.current = Math.max(progressRef.current, ev.progress);
    // U-05 启动交响：30% / 80% 真实进度跨越各一次节拍脉冲（不伪造时间线）
    if (symphonyRef.current) {
      if (!pulsed30Ref.current && progressRef.current >= 0.3) {
        pulsed30Ref.current = true;
        symphonyRef.current.pulse();
      }
      if (!pulsed80Ref.current && progressRef.current >= 0.8) {
        pulsed80Ref.current = true;
        symphonyRef.current.pulse();
      }
    }
    dispatch({ type: "PROGRESS", progress: ev.progress });
    setElapsed(ev.elapsedMs);
    if (ev.fileCount != null) setFileCount(ev.fileCount);
    if (ev.totalCount != null) setTotalCount(ev.totalCount);
    setCurrent({ task: ev.currentTask, filePath: ev.filePath, level: ev.level });
    if (ev.stats) {
      setStats(ev.stats);
      onStatsRef.current?.(ev.stats);
      symphonyRef.current?.ready(); // U-05：就绪双音（chime 触发点即 ready 事件到达点）
      dispatch({ type: "READY" }); // readyHold 停留时长由 pacingTimings 驱动（见下方 effect）
    }
  };

  useEffect(() => {
    let un: UnlistenFn | undefined;
    let cancelled = false;
    let enterTimer = 0;
    (async () => {
      // 启动动画/节奏/声音设置：与 boot_replay 同期轻量读取（DB 单一事实源），失败回退 full/cinematic。
      try {
        const s = await (await import("../../lib/settings")).loadSettings();
        if (!cancelled) {
          if (s.bootAnim === "simple" || s.bootAnim === "none") animRef.current = s.bootAnim;
          pacingRef.current = s.bootPacing;
          setAtmo({ reduceMotion: s.reduceMotion, safeMode: s.safeMode, perfMode: s.perfMode });
          // U-05 启动交响：mode 三档（full 三层 / mute 静音 / chime-only 仅就绪音）。
          try {
            symphonyRef.current = (await import("../../lib/sounds")).startBootSymphony({
              volume: s.soundVolume,
              muted: s.soundMuted,
              mode: s.bootSoundMode,
              theme: s.soundTheme,
              nightDamp: s.soundNightDamp,
            });
          } catch {
            /* 声音是增益不是依赖：失败静默 */
          }
        }
      } catch {
        /* 浏览器 dev 模式或读取失败 → full / cinematic */
      }
      // 入场编排时长（entering → streaming/readyHold）
      if (!cancelled) {
        enterTimer = window.setTimeout(() => dispatch({ type: "ENTER_DONE" }), pacingTimings(pacingRef.current).enter);
      }
      // 先拉取错过的真实事件（webview 挂载晚于后端启动时），再挂实时监听。
      try {
        const replay = await invoke<LoadEventPayload[]>("boot_replay");
        if (cancelled) return;
        for (const ev of replay) apply(ev);
      } catch {
        /* 浏览器 dev 模式无后端，正常 */
      }
      try {
        un = await listen<LoadEventPayload>("boot://event", (e) => apply(e.payload));
      } catch {
        /* 同上 */
      }
    })();
    return () => {
      cancelled = true;
      window.clearTimeout(enterTimer);
      symphonyRef.current?.stop(); // U-05：卸载即停（低频铺底节点不悬挂）
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // readyHold：真实摘要停留（cinematic 1200ms / brisk 400ms / instant 0）→ 退出编排
  useEffect(() => {
    if (ceremony.phase !== "readyHold" || finishedRef.current) return;
    const t = window.setTimeout(() => finish(), pacingTimings(pacingRef.current).readyHold);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ceremony.phase]);

  // rAF 平滑：显示值向真实进度缓动；真实进度停滞时显示值同步停滞。
  useEffect(() => {
    let raf = 0;
    const tick = (): void => {
      setShown((s) => {
        const target = progressRef.current;
        const d = target - s;
        if (d <= 0) return s;
        return d < 0.0008 ? target : s + d * 0.12;
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  // 停滞检测（单任务 >500ms 显示 micro spinner）：真实测量。
  useEffect(() => {
    const id = window.setInterval(() => {
      setStalled(Date.now() - lastEventAtRef.current > STALL_MS);
    }, 250);
    return () => window.clearInterval(id);
  }, []);

  // 跳过机制：Esc / 空格；进度 <30% 拒绝并如实提示。
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" && e.key !== " ") return;
      if (progressRef.current < SKIP_THRESHOLD) {
        setDenied(true);
        window.setTimeout(() => setDenied(false), 2000);
      } else {
        dispatch({ type: "SKIP" }); // ≥30%：跳过 UI，后台加载继续
        finish();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 移除 desktop.html 静态 splash（本组件接管启动画面）。
  useEffect(() => {
    const splash = document.getElementById("boot-splash");
    splash?.classList.add("done");
    const t = window.setTimeout(() => splash?.remove(), 450);
    return () => window.clearTimeout(t);
  }, []);

  const totalSlow = elapsed > SLOW_TOTAL_MS && !stats;

  return (
    <div
      className="boot-screen"
      data-exit={exit ?? undefined}
      data-phase={ceremony.phase}
      role="status"
      aria-live="polite"
    >
      <BootAtmosphere
        progress={shown}
        reduceMotion={atmo.reduceMotion}
        safeMode={atmo.safeMode}
        perfMode={atmo.perfMode}
      />
      <div className="boot-stage">
        <div className="boot-wordmark-wrap">
          <BootWordmark progress={shown} />
        </div>

        <div className="boot-capsule-wrap">
          <CapsuleBar progress={shown} stalled={stalled} />
        </div>

        <FileTicker
          task={current.task}
          filePath={current.filePath}
          level={current.level}
          stalled={stalled}
          fileCount={fileCount}
          totalCount={totalCount}
          elapsedMs={elapsed}
        />

        {totalSlow && (
          <div className="boot-hint">Taking longer than usual — the bar reflects real loading</div>
        )}

        {stats && (
          <div className="boot-summary">
            <span>
              {stats.records} records
            </span>
            <span>
              {stats.mindmaps} mindmaps
            </span>
            <span>
              {stats.mediaDirFiles} media files
            </span>
            <span>
              {fmtBytes(stats.workspaceBytes)} workspace
            </span>
            <span>
              {stats.nodes} nodes
            </span>
            <span>
              {stats.attachments} attachments
            </span>
          </div>
        )}
      </div>

      <div className="boot-skip">Esc / Space to skip · unlocks at 30%</div>
      {denied && (
        <div className="boot-denied">System is still loading critical data. Please wait.</div>
      )}
    </div>
  );
}
