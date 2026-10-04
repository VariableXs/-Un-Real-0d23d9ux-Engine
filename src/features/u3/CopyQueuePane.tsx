/**
 * U3-v7 CopyQueuePane 活体件（AI-U3 · 批次七 · copyqueue 引擎演示面）。
 *
 * 一个真实跑动的复制队列：发起三批复制（C 盘两批同盘串行 + D 盘一批），
 * 每批进度/速度/剩余时间实时跳动，暂停/取消/「先传这批」插队全部接引擎；
 * 空间预检拦截演示（F529：不过闸不出进度框）；清空回收站后悔窗通知条
 * （F524：5s 驻留/倒计时/延寿/撤销）。
 *
 * 词典纪律（十章）：队列面板浮层出路四条齐全——dictwalk 账本
 * u3-copy-queue 条目。调度核心复用 copyops（同盘串行/异盘并行判据同源）。
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  newJob, jobRow, jobProgress, jobPause, jobResume, jobCancel, jobDone, jobStateText,
  scheduleQueue, jumpQueue, undoBannerAssemble, undoBannerTick, undoBannerExtend,
  undoBannerRestore, preflightGate, type CopyJob, type UndoBanner,
} from "./labapi";

/** 演示节拍（进度 tick 间隔——缩比时间，判据语义不变）。 */
const TICK_MS = 250;
/** 演示速率（字节/秒·缩比）。 */
const DEMO_RATE = 64_000;

export function U3CopyQueueSection(): React.ReactElement {
  const [jobs, setJobs] = useState<ReadonlyArray<CopyJob>>([]);
  const [queueOpen, setQueueOpen] = useState(true);
  const [msg, setMsg] = useState("发起复制 → 队列真跑（同盘串行/异盘并行）；预检不足直接拦下");
  const [banner, setBanner] = useState<UndoBanner | null>(null);
  const [binRt, setBinRt] = useState<ReturnType<typeof undoBannerAssemble>["rt"] | null>(null);
  const nextId = useRef(1);
  const timer = useRef<number | null>(null);

  /** 调度推进（引擎 scheduleQueue 判据活跑：同盘串行、异盘并行、优先级）。 */
  const tick = useCallback(() => {
    setJobs((prev) => {
      const now = Date.now();
      const running = prev.filter((j) => j.state === "running").map((j) => j.volume);
      const picks = scheduleQueue(prev, new Set(running), 2);
      const pickIds = new Set(picks.map((p) => p.id));
      return prev.map((j) => {
        if (j.state === "running") {
          const done = Math.min(j.bytesTotal, j.bytesDone + DEMO_RATE * TICK_MS / 1000);
          const advanced = jobProgress(j, now, done);
          return done >= j.bytesTotal ? jobDone(advanced) : advanced;
        }
        if (pickIds.has(j.id)) return { ...j, state: "running" as const };
        return j;
      });
    });
  }, []);

  useEffect(() => {
    timer.current = window.setInterval(tick, TICK_MS);
    return () => { if (timer.current !== null) window.clearInterval(timer.current); };
  }, [tick]);

  /** 发起一批复制（F529 预检前置闸：不过闸不出进度作业）。 */
  const launch = useCallback((volume: string, targetFreeGB: number) => {
    const id = `job-${nextId.current++}`;
    const need = 3 * 1024 ** 3;
    const gate = preflightGate(
      { totalBytes: need, perTargetFree: { [volume]: targetFreeGB * 1024 ** 3 }, perTargetNeed: { [volume]: need } },
      "cancel",
    );
    if (gate.stage === "blocked") {
      setMsg(`预检拦截：${gate.message}——不出进度作业（三选出路：仍要复制/换目标/取消）`);
      return;
    }
    setJobs((prev) => [...prev, newJob(id, volume, `源·${volume} 素材包`, `目标·${volume} 备份`, need, prev.length + 5)]);
    setMsg(`预检过闸 → 「${id}」入队（${volume}）`);
  }, []);

  const undoBinOpen_ = useCallback(() => {
    const { rt, banner: b } = undoBannerAssemble(["旧截图 1", "旧截图 2", "废弃草稿"], Date.now());
    setBinRt(rt);
    setBanner(b);
    setMsg("回收站已清空——后悔窗开（5 秒驻留）");
  }, []);

  /** 通知条倒计时 tick。 */
  useEffect(() => {
    if (!binRt || !banner?.visible) return;
    const t = window.setInterval(() => {
      setBinRt((cur) => {
        if (!cur) return cur;
        const { rt, banner: b } = undoBannerTick(cur, Date.now());
        setBanner(b);
        return rt;
      });
    }, 200);
    return () => window.clearInterval(t);
  }, [binRt, banner?.visible]);

  const rows = jobs.map((j) => ({ j, row: jobRow(j) }));
  const runningCount = jobs.filter((j) => j.state === "running").length;

  return (
    <SectionCard title="复制队列装配区" f="F531/F524/F529/F530·v7">
      <div className="u3-lab-intro">
        复制链装配活体：F531 队列视图四信息（进度/速度/剩余/状态）+ 同盘串行异盘并行 + 插队 + 独立暂停取消、F529 预检前置闸、F524 后悔窗通知条、F530 大文件自动校验标记。
      </div>

      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F531</span>发起复制</div>
          <div className="u3-desc">C 盘两批同盘串行排队；D 盘一批异盘并行；空间不足那路演示 F529 拦截</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn" onClick={() => launch("C:", 99)}>复制 → C:（足量）</button>
          <button type="button" className="j1x-btn" onClick={() => launch("D:", 99)}>复制 → D:（足量）</button>
          <button type="button" className="j1x-btn" onClick={() => launch("E:", 2)}>复制 → E:（只剩 2GB→拦截）</button>
          <button type="button" className="j1x-btn" onClick={undoBinOpen_}>清空回收站 3 项</button>
        </div>
      </div>

      {/* F524 后悔窗通知条 */}
      {banner?.visible && (
        <div className="u3-undo-banner" role="status">
          <span>{banner.text || "已清空回收站——撤销"}</span>
          <span className="u3-undo-timer">{(banner.remainingMs / 1000).toFixed(1)}s</span>
          {binRt && (
            <>
              <button
                type="button" className="j1x-btn"
                disabled={!banner.canExtend}
                onClick={() => {
                  const { rt, banner: b } = undoBannerExtend(binRt);
                  setBinRt(rt);
                  setBanner(b);
                  setMsg(b.text || "延寿");
                }}
              >延寿 +10s</button>
              <button
                type="button" className="j1x-btn"
                onClick={() => {
                  const { restored, banner: b } = undoBannerRestore(binRt);
                  setBinRt(null);
                  setBanner(b);
                  setMsg(restored ? `已还原 ${restored.length} 项` : "窗口已关——诚实不可恢复");
                }}
              >撤销</button>
            </>
          )}
        </div>
      )}

      {/* 队列视图（浮层出路四条齐全——u3-copy-queue 账本条目） */}
      {queueOpen && rows.length > 0 && (
        <div className="u3-queue" data-u3-float="queue" role="table" aria-label="复制队列">
          <div className="u3-queue-head">
            <span>复制队列（运行 {runningCount} · 共 {jobs.length}）</span>
            <button type="button" className="j1x-btn" onClick={() => setQueueOpen(false)}>收起</button>
          </div>
          {rows.map(({ j, row }) => (
            <div key={j.id} className={`u3-queue-row is-${j.state}`}>
              <span className="u3-queue-label">{row.label}</span>
              <span className="u3-queue-bar" aria-hidden>
                <span className="u3-queue-fill" style={{ width: `${row.progressPct}%` }} />
              </span>
              <span className="u3-queue-pct">{row.progressPct.toFixed(0)}%</span>
              <span className="u3-queue-meta">
                {row.speedBps !== null ? `${(row.speedBps / 1024).toFixed(0)} KB/s` : "计算中"}
                {" · "}
                {row.etaSec !== null ? `剩 ${row.etaSec.toFixed(0)}s` : "剩余计算中"}
              </span>
              <span className={`u3-badge ${j.state === "done" ? "ok" : j.state === "canceled" ? "warn" : ""}`}>
                {jobStateText(j.state)}
              </span>
              {j.verify && j.state === "running" && <span className="u3-badge">校验中（&gt;1GB 自动）</span>}
              <span className="u3-queue-ops">
                <button type="button" className="j1x-btn" disabled={j.state !== "running"}
                  onClick={() => setJobs((p) => p.map((x) => (x.id === j.id ? jobPause(x) : x)))}>暂停</button>
                <button type="button" className="j1x-btn" disabled={j.state !== "paused"}
                  onClick={() => setJobs((p) => p.map((x) => (x.id === j.id ? jobResume(x) : x)))}>恢复</button>
                <button type="button" className="j1x-btn" disabled={j.state === "done" || j.state === "canceled"}
                  onClick={() => setJobs((p) => p.map((x) => (x.id === j.id ? jobCancel(x) : x)))}>取消</button>
                <button type="button" className="j1x-btn" disabled={j.state !== "queued"}
                  onClick={() => setJobs((p) => jumpQueue(p, j.id))}>先传这批</button>
              </span>
            </div>
          ))}
        </div>
      )}
      {jobs.length > 0 && !queueOpen && (
        <div className="u3-row">
          <div className="u3-ctl" style={{ gridColumn: "1 / span 3" }}>
            <button type="button" className="j1x-btn" onClick={() => setQueueOpen(true)}>展开队列（{jobs.length} 批）</button>
          </div>
        </div>
      )}

      <div className="u3-row">
        <div className="u3-ctl" style={{ gridColumn: "1 / span 3" }}>
          <span className="u3-stat" role="status">{msg}</span>
        </div>
      </div>
    </SectionCard>
  );
}
