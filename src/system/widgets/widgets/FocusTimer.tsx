/**
 * N-09 倒计时专注：分钟输入 + 到点 WebAudio 短铃。
 * 诚实边界：宿主 settings.soundMuted / soundVolume 经 loadSettings 读取（异步一次），
 * 读不到时按静音处理（声音是增益不是依赖）；倒计时基于时间戳，gate 暂停刷新后
 * 恢复仍准确（不追帧）。
 */
import { useEffect, useRef, useState } from "react";
import { LABELS, useLaneLang } from "../labels";
import { loadSettings } from "../../../lib/settings";

type Phase = "idle" | "running" | "done";

function chime(): void {
  try {
    const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = 880;
    gain.gain.setValueAtTime(0.12, ctx.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.9);
    osc.connect(gain).connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + 0.9);
    setTimeout(() => void ctx.close().catch(() => {}), 1100);
  } catch {
    /* 无音频设备：静默 */
  }
}

export default function FocusTimer({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [minutes, setMinutes] = useState(25);
  const [phase, setPhase] = useState<Phase>("idle");
  const [left, setLeft] = useState(0);
  const endAtRef = useRef(0);
  const mutedRef = useRef(true);

  useEffect(() => {
    void loadSettings()
      .then((s) => {
        mutedRef.current = s.soundMuted;
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (phase !== "running") return;
    const id = window.setInterval(() => {
      if (paused) return; // 低电/全屏暂停刷新，时间戳恢复后仍准确
      const remain = Math.max(0, Math.round((endAtRef.current - Date.now()) / 1000));
      setLeft(remain);
      if (remain === 0) {
        setPhase("done");
        if (!mutedRef.current) chime();
      }
    }, 500);
    return () => window.clearInterval(id);
  }, [phase, paused]);

  const mm = String(Math.floor(left / 60)).padStart(2, "0");
  const ss = String(left % 60).padStart(2, "0");

  return (
    <div className="wgt-focus">
      <div className={`wgt-focus-time ${phase === "done" ? "done" : ""}`}>{phase === "idle" ? `${minutes}:00` : `${mm}:${ss}`}</div>
      {phase === "done" && <div className="wgt-focus-done">{t.timeUp}</div>}
      <div className="wgt-focus-row">
        {phase === "running" ? (
          <button type="button" className="wgt-btn" onClick={() => { setPhase("idle"); setLeft(0); }}>{t.reset}</button>
        ) : (
          <>
            <input
              type="number"
              min={1}
              max={180}
              className="wgt-input num"
              value={minutes}
              aria-label={t.minutes}
              disabled={phase === "done"}
              onChange={(e) => setMinutes(Math.min(180, Math.max(1, Number(e.target.value) || 1)))}
            />
            <button
              type="button"
              className="wgt-btn"
              onClick={() => {
                endAtRef.current = Date.now() + minutes * 60_000;
                setLeft(minutes * 60);
                setPhase("running");
              }}
            >
              {t.start}
            </button>
          </>
        )}
      </div>
      <div className="wgt-dim tiny">{t.mutedNote}</div>
    </div>
  );
}