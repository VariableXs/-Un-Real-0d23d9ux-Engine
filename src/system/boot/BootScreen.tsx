import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { ipc } from "../../lib/ipc";
import type { BootAnim } from "../../lib/settings";
import "../../styles/boot.css";

/**
 * 启动仪式（真实事件驱动）。
 *
 * 硬性规则（docs/ARCHITECTURE_V2.md §5）：
 * - 进度条 100% 来自后端 `boot://event` 真实进度，无预设时间线。
 * - 日志 100% 是真实任务与真实文件名，无硬编码文案。
 * - VARIABLE 八个字母的 SVG 描边按每字母 12.5% 区间映射真实进度，
 *   rAF 平滑插值只做视觉平滑；进度停滞时描边同步停滞。
 * - Esc/空格：进度 <30% 拒绝跳过；≥30% 跳过 UI，后台加载继续。
 *
 * 批次A 阶段4/5：ready（真实摘要）之后的过渡编排由 `bootAnim` 设置控制——
 * full=字母落位任务栏 / simple=快速淡出 / none=直接进入。均为纯视觉过渡，
 * 不伪造任何加载进度。
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

const LETTERS = ["V", "A", "R", "I", "A", "B", "L", "E"];
/** 校准值：84px 字形轮廓近似长度，仅影响描边视觉，不影响进度真实性。 */
const STROKE_DASH = 340;
const MAX_LINES = 9;
const STALL_MS = 500;
const SLOW_TOTAL_MS = 5000;

interface Line {
  id: number;
  icon: string;
  text: string;
  path: string | null;
  level: number;
}

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
  /** 退出编排开始（字母起飞前一刻）：宿主此刻挂载桌面 shell（ beneath ），设置加载同步启动。 */
  onExitStart?: () => void;
  /** ready 摘要（真实统计）上抛，供桌面"磁盘同步通知"使用。 */
  onStats?: (stats: BootStats) => void;
}): React.ReactElement {
  const [shown, setShown] = useState(0); // 平滑显示值（收敛于真实进度）
  const [lines, setLines] = useState<Line[]>([]);
  const [stats, setStats] = useState<BootStats | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [fileCount, setFileCount] = useState<number | null>(null);
  const [memoryMb, setMemoryMb] = useState<number | null>(null);
  const [stalled, setStalled] = useState(false);
  const [denied, setDenied] = useState(false);
  const [exit, setExit] = useState<BootAnim | null>(null);

  const progressRef = useRef(0); // 真实进度（单调不减）
  const lastSeqRef = useRef(0);
  const lastEventAtRef = useRef(Date.now());
  const finishedRef = useRef(false);
  const animRef = useRef<BootAnim>("full");
  const onDoneRef = useRef(props.onDone);
  onDoneRef.current = props.onDone;
  const onExitStartRef = useRef(props.onExitStart);
  onExitStartRef.current = props.onExitStart;
  const onStatsRef = useRef(props.onStats);
  onStatsRef.current = props.onStats;
  const zh = typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("zh");

  const finish = (): void => {
    if (finishedRef.current) return;
    finishedRef.current = true;
    const a = animRef.current;
    setExit(a);
    if (a === "none") {
      onExitStartRef.current?.();
      onDoneRef.current();
      return;
    }
    if (a === "simple") {
      onExitStartRef.current?.();
      window.setTimeout(() => onDoneRef.current(), 640);
      return;
    }
    // full：字母实心化 → UI 淡出 → 字母缩小落向任务栏 → 桌面就绪
    window.setTimeout(() => onExitStartRef.current?.(), 480);
    window.setTimeout(() => onDoneRef.current(), 1880);
  };

  const apply = (ev: LoadEventPayload): void => {
    if (ev.seq <= lastSeqRef.current) return; // 回放去重
    lastSeqRef.current = ev.seq;
    lastEventAtRef.current = Date.now();
    progressRef.current = Math.max(progressRef.current, ev.progress);
    setElapsed(ev.elapsedMs);
    if (ev.fileCount != null) setFileCount(ev.fileCount);
    setLines((prev) => {
      const next: Line = { id: ev.seq, icon: ev.icon, text: ev.currentTask, path: ev.filePath, level: ev.level };
      const merged = [...prev, next];
      return merged.length > MAX_LINES ? merged.slice(merged.length - MAX_LINES) : merged;
    });
    if (ev.stats) {
      setStats(ev.stats);
      onStatsRef.current?.(ev.stats);
      // ready 后短暂停留，让用户看到真实摘要，再过渡到桌面
      window.setTimeout(finish, 800);
    }
  };

  useEffect(() => {
    let un: UnlistenFn | undefined;
    let cancelled = false;
    (async () => {
      // 启动动画设置：与 boot_replay 同期轻量读取（DB 单一事实源），失败回退 full。
      try {
        const raw = await ipc.getSettings();
        if (!cancelled) {
          const v = raw["bootAnim"];
          if (v === "simple" || v === "none") animRef.current = v;
        }
      } catch {
        /* 浏览器 dev 模式或读取失败 → full */
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
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

  // 停滞检测（单任务 >500ms 显示 spinner）+ 耗时 + 内存：全部真实测量。
  useEffect(() => {
    const id = window.setInterval(() => {
      setStalled(Date.now() - lastEventAtRef.current > STALL_MS);
      const mem = (performance as { memory?: { usedJSHeapSize: number } }).memory;
      if (mem) setMemoryMb(mem.usedJSHeapSize / 1048576);
    }, 250);
    return () => window.clearInterval(id);
  }, []);

  // 跳过机制：Esc / 空格；进度 <30% 拒绝。
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" && e.key !== " ") return;
      if (progressRef.current < 0.3) {
        setDenied(true);
        window.setTimeout(() => setDenied(false), 2000);
      } else {
        finish();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 移除 index.html 静态 splash（本组件接管启动画面）。
  useEffect(() => {
    const splash = document.getElementById("boot-splash");
    splash?.classList.add("done");
    const t = window.setTimeout(() => splash?.remove(), 450);
    return () => window.clearTimeout(t);
  }, []);

  const totalSlow = elapsed > SLOW_TOTAL_MS && !stats;
  const speed = fileCount != null && elapsed > 0 ? fileCount / (elapsed / 1000) : null;
  const letterProgress = (i: number): number => Math.min(1, Math.max(0, shown * 8 - i));

  return (
    <div
      className="boot-screen"
      data-exit={exit ?? undefined}
      role="status"
      aria-live="polite"
    >
      <div className="boot-stage">
        <div className="boot-letters" aria-label="Variable">
          {LETTERS.map((L, i) => {
            const p = letterProgress(i);
            return (
              <svg key={i} className="boot-letter" viewBox="0 0 100 120" aria-hidden>
                <text x="50" y="62" textAnchor="middle" dominantBaseline="central" className="boot-letter-fill">
                  {L}
                </text>
                <text
                  x="50"
                  y="62"
                  textAnchor="middle"
                  dominantBaseline="central"
                  className="boot-letter-stroke"
                  strokeDasharray={STROKE_DASH}
                  strokeDashoffset={STROKE_DASH * (1 - p)}
                  style={{ opacity: p > 0 ? 1 : 0 }}
                >
                  {L}
                </text>
              </svg>
            );
          })}
        </div>

        <div className="boot-bar">
          <div className="boot-bar-fill" style={{ width: `${(shown * 100).toFixed(2)}%` }} />
        </div>
        <div className="boot-progress-row">
          <span>{zh ? "真实加载进度（无预设动画）" : "Real loading progress (no scripted timeline)"}</span>
          <span className="boot-percent">{(shown * 100).toFixed(1)}%</span>
        </div>

        <div className="boot-log">
          {lines.map((ln, idx) => {
            const latest = idx === lines.length - 1;
            const cls = `boot-line${ln.level === 1 ? " is-warn" : ln.level === 2 ? " is-error" : ""}${latest ? " boot-line-latest" : ""}`;
            return (
              <div key={ln.id}>
                <div className={cls}>
                  {latest && stalled && !stats ? <span className="boot-spinner" /> : null}
                  {ln.icon} {ln.text}
                </div>
                {ln.path && latest ? <div className="boot-line-path">└─ {ln.path}</div> : null}
              </div>
            );
          })}
        </div>

        <div className="boot-stats-row">
          {fileCount != null && (
            <span>
              ✓ {zh ? "已加载" : "Loaded"} {fileCount}
              {stats ? ` / ${stats.workspaceFiles}` : ""} {zh ? "个文件" : "files"}
            </span>
          )}
          <span>⏱ {(elapsed / 1000).toFixed(1)}s</span>
          {speed != null && <span>📊 {speed.toFixed(1)} files/s</span>}
          {memoryMb != null && <span>💾 {memoryMb.toFixed(0)}MB</span>}
        </div>

        {totalSlow && (
          <div className="boot-hint">
            {zh ? "耗时比平时长，请稍候…（进度条反映真实加载状态）" : "This is taking longer than usual. Please wait…"}
          </div>
        )}

        {stats && (
          <div className="boot-summary">
            📊 {stats.records} {zh ? "条记录" : "records"} · {stats.mindmaps} {zh ? "张导图" : "mindmaps"} ·{" "}
            {stats.mediaDirFiles} {zh ? "个媒体文件" : "media files"} · {fmtBytes(stats.workspaceBytes)}{" "}
            {zh ? "工作区" : "workspace"}
          </div>
        )}
      </div>

      <div className="boot-skip">{zh ? "Esc / 空格跳过（进度 ≥30% 后可用）" : "Esc / Space to skip (after 30%)"}</div>
      {denied && (
        <div className="boot-denied">
          {zh ? "系统仍在初始化关键数据，请稍候" : "System is still loading critical data. Please wait."}
        </div>
      )}
    </div>
  );
}
