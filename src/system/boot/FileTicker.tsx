import { useEffect, useRef, useState } from "react";

/**
 * U-04 文件走马灯（真实事件流，无预设时间线）。
 *
 * - 单行当前文件：等宽字体，长路径「保文件名截头」（tail-truncate），
 *   title 悬浮给完整路径；换行过渡 旧-上淡出 / 新-下淡入 120ms（仅 transform+opacity）。
 * - 下方三列真实统计：速度（累计 files/elapsed）· 剩余项目 · 剩余时间
 *   （3s 滑动窗口速度均值 × 剩余数）。LoadEventPayload 无剩余字节 —— 只如实展示
 *   可推导项，绝不编造字节数。
 * - 左侧 12px 内联 SVG 状态图标：弧=加载（旋转）/ 三角=警告 / 叉=错误，无 emoji。
 * - 停滞 >500ms（上层 STALL_MS 判定）显示微型 spinner。
 */

/** 3s 滑动窗口：速度样本环（t=后端 elapsedMs，单调）。 */
const WINDOW_MS = 3000;
const ROW_SWAP_MS = 130; // 120ms 过渡 + 10ms 余量后移除旧行

interface TickerItem {
  key: number;
  task: string;
  filePath: string | null;
}

/** 长路径保文件名截头：显示尾部（文件名永远可见），前置省略号。 */
function tailTruncate(p: string, max = 56): string {
  if (p.length <= max) return p;
  return `…${p.slice(p.length - (max - 1))}`;
}

function ArcIcon(): React.ReactElement {
  // 旋转弧：r=4.5，周长≈28.3，dash 21.2（≈270° 开口）
  return (
    <svg viewBox="0 0 12 12" className="boot-arc-spin" aria-hidden="true">
      <circle cx="6" cy="6" r="4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeDasharray="21.2 7.1" strokeLinecap="round" />
    </svg>
  );
}

function WarnIcon(): React.ReactElement {
  return (
    <svg viewBox="0 0 12 12" aria-hidden="true">
      <path d="M6 1.6 L11 10.4 H1 Z" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
    </svg>
  );
}

function ErrorIcon(): React.ReactElement {
  return (
    <svg viewBox="0 0 12 12" aria-hidden="true">
      <path d="M2.6 2.6 L9.4 9.4 M9.4 2.6 L2.6 9.4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export function FileTicker(props: {
  /** 当前真实任务名（无文件路径时直接展示）。 */
  task: string;
  /** 当前真实文件完整路径（null = 本事件无文件）。 */
  filePath: string | null;
  /** 0=info 1=warn 2=error（来自后端事件）。 */
  level: number;
  /** 上层 STALL_MS(500) 判定的停滞标志。 */
  stalled: boolean;
  fileCount: number | null;
  totalCount: number | null;
  elapsedMs: number;
  zh: boolean;
}): React.ReactElement {
  const { task, filePath, level, stalled, fileCount, totalCount, elapsedMs, zh } = props;

  // ---- 换行过渡（旧-上淡出 / 新-下淡入） ----
  const prevRef = useRef<TickerItem>({ key: 0, task, filePath });
  const [item, setItem] = useState<TickerItem>(prevRef.current);
  const [leaving, setLeaving] = useState<TickerItem | null>(null);
  useEffect(() => {
    const prev = prevRef.current;
    if (prev.task === task && prev.filePath === filePath) return;
    const next: TickerItem = { key: prev.key + 1, task, filePath };
    prevRef.current = next;
    setLeaving(prev);
    setItem(next);
    const t = window.setTimeout(() => setLeaving(null), ROW_SWAP_MS);
    return () => window.clearTimeout(t);
  }, [task, filePath]);

  // ---- 3s 滑动窗口速度（(t, fileCount) 样本环） ----
  const samplesRef = useRef<{ t: number; c: number }[]>([]);
  const [windowSpeed, setWindowSpeed] = useState<number | null>(null);
  useEffect(() => {
    if (fileCount == null || elapsedMs <= 0) return;
    const buf = samplesRef.current;
    const last = buf[buf.length - 1];
    if (last && last.c === fileCount && last.t === elapsedMs) return; // 同一样本
    buf.push({ t: elapsedMs, c: fileCount });
    while (buf.length > 2 && elapsedMs - (buf[0]?.t ?? elapsedMs) > WINDOW_MS) buf.shift();
    const first = buf[0];
    const newest = buf[buf.length - 1];
    if (first && newest && newest.t > first.t) {
      setWindowSpeed((newest.c - first.c) / ((newest.t - first.t) / 1000));
    }
  }, [fileCount, elapsedMs]);

  // ---- 三列统计（全部真实推导） ----
  const speed = fileCount != null && elapsedMs > 0 ? fileCount / (elapsedMs / 1000) : null;
  const remainingItems = fileCount != null && totalCount != null ? Math.max(0, totalCount - fileCount) : null;
  const etaSec =
    windowSpeed != null && windowSpeed > 0 && remainingItems != null ? remainingItems / windowSpeed : null;

  const L = (zh: boolean, a: string, b: string): string => (zh ? a : b);
  const fmtEta = (n: number): string => (zh ? `大约 ${Math.ceil(n)} 秒` : `~${Math.ceil(n)}s`);

  const renderRow = (it: TickerItem, cls: string): React.ReactElement => (
    <div className={`boot-ticker-row ${cls}`} data-level={level} key={it.key}>
      <span className="boot-ticker-icon">
        {level === 1 ? <WarnIcon /> : level === 2 ? <ErrorIcon /> : <ArcIcon />}
      </span>
      <span className="boot-ticker-name" title={it.filePath ?? undefined}>
        {it.filePath ? tailTruncate(it.filePath) : it.task}
      </span>
      {stalled && (
        <span className="boot-ticker-stallspin" aria-label={L(zh, "加载停滞中", "stalled")} role="img">
          <svg viewBox="0 0 8 8" aria-hidden="true">
            <circle cx="4" cy="4" r="2.8" fill="none" stroke="currentColor" strokeWidth="1.2" strokeDasharray="13.2 4.4" strokeLinecap="round" />
          </svg>
        </span>
      )}
    </div>
  );

  return (
    <div className="boot-ticker">
      <div className="boot-ticker-rowbox" aria-live="polite">
        {leaving ? renderRow(leaving, "is-leaving") : null}
        {renderRow(item, "is-current")}
      </div>
      <div className="boot-ticker-stats">
        <span className="boot-ticker-stat">
          {L(zh, "速度", "Speed")}:{" "}
          <b>{speed != null ? `${speed.toFixed(1)} files/s` : "—"}</b>
        </span>
        <span className="boot-ticker-stat">
          {L(zh, "剩余项目", "Remaining")}:{" "}
          <b>{remainingItems != null ? String(remainingItems) : "—"}</b>
        </span>
        <span className="boot-ticker-stat">
          {L(zh, "剩余时间", "ETA")}:{" "}
          <b>{etaSec != null ? fmtEta(etaSec) : "—"}</b>
        </span>
      </div>
    </div>
  );
}
