/**
 * N-10 锁屏与仪式屏（Lock Screen，功能全景 L871-889）。
 *
 * - 全屏 overlay：CSS 星空背景（本车道用纯 CSS 星点，零依赖零 GPU 负担；
 *   复用 WebGL 星空引擎属可选优化，见桌面壁纸工坊）；
 * - 大时钟（字号档 S/M/L，localStorage variable:lockscreen:clock-size）；
 * - 通知聚合：只显示「来源 + 数量」，绝不显示标题/正文（L878 隐私红线，
 *   aggregateByKind 输入类型层面即剔除内容字段）；
 * - 双重角色：真锁（口令 SHA-256 本地哈希 + 8 位恢复码仅显示一次 + 错 5 次指数退避）
 *   与仪式锁（任意键/点击退出，可显示专注倒计时）；
 * - 锁定/解锁动效走 CSS（prefers-reduced-motion 时关闭）；
 * - 诚实边界：应用层锁屏，不替代 Windows 锁屏；Win+L 全局热键属后端
 *   kbdhook 领地，本车道不做 —— 入口为菜单/快捷面板事件 ai04:open-feature。
 *
 * 自挂载：模块加载即监听 `ai04:open-feature` {feature:"lockscreen"}，
 * 主控集成阶段 `void import("system/lockscreen/LockScreen")` 一次即激活。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useNotifications } from "../../state/notifyStore";
import { mountOnEvent, installCloseHandler, dispatchClose } from "../wallpaper/mount";
import { lockT } from "./labels";
import {
  aggregateByKind,
  backoffMs,
  clockSizeClass,
  loadFails,
  loadMode,
  LS_CLOCK_SIZE,
  LS_FOCUS_UNTIL,
  LS_HASH,
  LS_FAILS,
  LS_RECOVERY_HASH,
  randomRecoveryCode,
  lsGet,
  lsSet,
  sha256Hex,
  verifySecret,
  type LockMode,
} from "./lock";

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useEffect(() => {
    if (typeof matchMedia === "undefined") return;
    const mq = matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (): void => setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

function fmtClock(d: Date): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function fmtDate(d: Date): string {
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

function fmtRemaining(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

const KIND_LABEL: Record<string, "notifPrivacy" | "notifHardware" | "notifSystem"> = {
  privacy: "notifPrivacy",
  hardware: "notifHardware",
  system: "notifSystem",
};

type RealStage = "verify" | "setup" | "recovery";

function RealLock(props: { onUnlock: () => void; reduced: boolean }): React.ReactElement {
  const t = useMemo(() => lockT(), []);
  const [stage, setStage] = useState<RealStage>(() => (lsGet(LS_HASH) ? "verify" : "setup"));
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [msg, setMsg] = useState("");
  const [fails, setFails] = useState(() => loadFails());
  const [waitUntil, setWaitUntil] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const [recoveryCode, setRecoveryCode] = useState("");
  const [copied, setCopied] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    inputRef.current?.focus();
  }, [stage]);

  const remaining = Math.max(0, waitUntil - now);
  const locked = remaining > 0;

  const submit = async (): Promise<void> => {
    if (locked) return;
    const hash = lsGet(LS_HASH);
    if (stage === "verify" && hash) {
      // 恢复码路径：匹配恢复码哈希 → 重置口令（验收④）
      const recHash = lsGet(LS_RECOVERY_HASH);
      if (recHash && (await verifySecret(pw, recHash))) {
        try { localStorage.removeItem(LS_HASH); localStorage.removeItem(LS_RECOVERY_HASH); } catch { /* 忽略 */ }
        lsSet(LS_FAILS, "0");
        setFails(0);
        setPw("");
        setPw2("");
        setStage("setup");
        setMsg(t("pwReset"));
        return;
      }
      if (await verifySecret(pw, hash)) {
        lsSet(LS_FAILS, "0");
        props.onUnlock();
        return;
      }
      const next = fails + 1;
      setFails(next);
      lsSet(LS_FAILS, String(next));
      const wait = backoffMs(next);
      if (wait > 0) {
        setWaitUntil(Date.now() + wait);
        setMsg(t("tooMany"));
      } else {
        setMsg(t("wrongPw"));
      }
      setPw("");
      return;
    }
    if (stage === "setup") {
      if (pw.length < 4) {
        setMsg(t("setupPw"));
        return;
      }
      if (pw !== pw2) {
        setMsg(t("setupPwMismatch"));
        return;
      }
      lsSet(LS_HASH, await sha256Hex(pw));
      const code = randomRecoveryCode();
      lsSet(LS_RECOVERY_HASH, await sha256Hex(code));
      lsSet(LS_FAILS, "0");
      setRecoveryCode(code); // 仅显示一次（L886）
      setStage("recovery");
      return;
    }
  };

  if (stage === "recovery") {
    return (
      <div className={`lockscr-panel ${props.reduced ? "no-anim" : ""}`}>
        <h3>{t("recoveryShow")}</h3>
        <div className="lockscr-recovery">
          <code>{recoveryCode}</code>
          <button
            onClick={() => {
              void navigator.clipboard?.writeText(recoveryCode).catch(() => {});
              setCopied(true);
            }}
          >
            {copied ? t("recoveryCopied") : "⧉"}
          </button>
        </div>
        <button className="lockscr-primary" onClick={() => { setStage("verify"); setPw(""); setPw2(""); setMsg(""); }}>
          {t("recoveryDone")}
        </button>
      </div>
    );
  }

  return (
    <div className={`lockscr-panel ${props.reduced ? "no-anim" : ""}`} data-testid="lockscreen">
      <h3>{stage === "setup" ? t("setupTitle") : t("realHint")}</h3>
      <input
        ref={inputRef}
        className="lockscr-input"
        type="password"
        value={pw}
        disabled={locked}
        placeholder={stage === "setup" ? t("setupPw") : ""}
        onChange={(e) => setPw(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void submit();
          e.stopPropagation(); // 真锁：按键不触发外层任意键退出
        }}
      />
      {stage === "setup" && (
        <input
          className="lockscr-input"
          type="password"
          value={pw2}
          placeholder={t("setupPw2")}
          onChange={(e) => setPw2(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
            e.stopPropagation();
          }}
        />
      )}
      {locked && <p className="lockscr-warn">{`${t("tooMany")} ${fmtRemaining(remaining)}`}</p>}
      {msg && !locked && <p className="lockscr-warn">{msg}</p>}
      <button className="lockscr-primary" disabled={locked} onClick={() => void submit()}>
        {stage === "setup" ? t("setupSave") : t("unlock")}
      </button>
      {stage === "verify" && !locked && (
        <button className="lockscr-link" onClick={() => { setMsg(""); setPw(""); }}>
          {t("recoveryEntry")}
        </button>
      )}
      {fails > 0 && <p className="lockscr-fails">✕ {fails}</p>}
    </div>
  );
}

export function LockScreen(): React.ReactElement {
  const t = useMemo(() => lockT(), []);
  const reduced = useReducedMotion();
  const mode: LockMode = useMemo(() => loadMode(), []);
  const items = useNotifications();
  const sources = useMemo(() => aggregateByKind(items), [items]);
  const [now, setNow] = useState(() => new Date());
  const [clockSize, setClockSize] = useState(() => clockSizeClass());
  const focusUntil = Number(lsGet(LS_FOCUS_UNTIL)) || 0;

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  // 仪式锁：任意键 / 点击退出（真锁不注册本监听）
  useEffect(() => {
    if (mode !== "ritual") return;
    const exit = (): void => dispatchClose("lockscreen");
    window.addEventListener("keydown", exit);
    window.addEventListener("pointerdown", exit);
    return () => {
      window.removeEventListener("keydown", exit);
      window.removeEventListener("pointerdown", exit);
    };
  }, [mode]);

  const cycleClockSize = (): void => {
    const order = ["s", "m", "l"];
    const cur = lsGet(LS_CLOCK_SIZE) ?? "m";
    const next = order[(order.indexOf(cur) + 1) % order.length] ?? "m";
    lsSet(LS_CLOCK_SIZE, next);
    setClockSize(clockSizeClass());
  };

  return createPortal(
    <div className={`lockscr-root ${reduced ? "no-anim" : ""}`} role="dialog" aria-label="lock screen">
      {/* CSS 星空（注：非 WebGL 引擎，锁屏低功耗优先；桌面壁纸引擎见 WallpaperLayer） */}
      <div className="lockscr-stars" aria-hidden />
      <div className="lockscr-core">
        <div className={`lockscr-clock ${clockSize}`}>
          <span>{fmtClock(now)}</span>
          <em>{fmtDate(now)}</em>
        </div>

        {mode === "ritual" ? (
          <div className="lockscr-panel">
            <p className="lockscr-hint">{t("ritualHint")}</p>
            {focusUntil > now.getTime() && (
              <p className="lockscr-focus">
                {t("ritualFocus")} <strong>{fmtRemaining(focusUntil - now.getTime())}</strong>
              </p>
            )}
          </div>
        ) : (
          <RealLock onUnlock={() => dispatchClose("lockscreen")} reduced={reduced} />
        )}

        {sources.length > 0 && (
          <div className="lockscr-notifs" aria-label={t("notifTitle")}>
            <span className="lockscr-notifs-title">{t("notifTitle")}</span>
            {/* 红线：只渲染来源与数量，内容（title/body）在此组件中不可达 */}
            {sources.map((s) => (
              <span key={s.kind} className="lockscr-notif-chip">
                {t(KIND_LABEL[s.kind] ?? "notifSystem")} · {s.count}
              </span>
            ))}
          </div>
        )}
      </div>

      <footer className="lockscr-footer">
        {mode === "ritual" && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              lsSet(LS_FOCUS_UNTIL, String(Date.now() + 25 * 60_000));
            }}
          >
            +25:00
          </button>
        )}
        <button onClick={(e) => { e.stopPropagation(); cycleClockSize(); }}>{t("clockSize")}</button>
        <span>{t("boundary")}</span>
      </footer>
    </div>,
    document.body,
  );
}

/* 模块加载即监听（主控集成阶段动态 import 本模块即激活；不改 DesktopShell）。 */
if (typeof window !== "undefined") {
  mountOnEvent(window, "lockscreen", async () => ({ Overlay: LockScreen }));
  installCloseHandler(window, "lockscreen");
}