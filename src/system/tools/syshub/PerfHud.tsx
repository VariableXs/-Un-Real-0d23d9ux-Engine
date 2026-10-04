import { useEffect, useState } from "react";
import { Activity, X } from "lucide-react";
import { useI18n } from "../../../i18n";
import { ipc } from "../../../lib/ipc";
import { ipc11 } from "../../../lib/ipc11";
import { isTauriRuntime } from "../../../entries/runtime";
import type { PerfMode } from "./perfmode";
import { loadPerfMode } from "./perfmode";

/**
 * AI-11 N-19 性能 HUD 悬浮窗（桌面层右上角小浮条）。
 * 开关持久化 localStorage（variable:ai11:hud）；内容 = 系统运行时长 +
 * 电池/电源 + 性能模式标签（30s 轮询 sys_uptime，轻量）。
 * 数据全部只读；失败如实隐藏对应行。
 */

const HUD_KEY = "variable:ai11:hud";

export function hudEnabled(): boolean {
  try {
    return localStorage.getItem(HUD_KEY) === "1";
  } catch {
    return false;
  }
}

export function setHudEnabled(on: boolean): void {
  try {
    localStorage.setItem(HUD_KEY, on ? "1" : "0");
  } catch {
    /* storage blocked */
  }
}

/** 供设置页/系统中枢切换（窗口内 React 状态由 key 重挂载同步）。 */
export function PerfHud(): React.ReactElement | null {
  const { t } = useI18n();
  const [on, setOn] = useState(() => hudEnabled());
  const [tick, setTick] = useState(0);
  const [sysSecs, setSysSecs] = useState<number | null>(null);
  const [battery, setBattery] = useState<string | null>(null);
  const [mode] = useState<PerfMode>(() => loadPerfMode());

  // 跨组件同步：localStorage 变更即重读（storage 事件仅跨页签，这里用自定义事件兜底）
  useEffect(() => {
    const h = (): void => setOn(hudEnabled());
    window.addEventListener("variable:ai11:hud-changed", h);
    return () => window.removeEventListener("variable:ai11:hud-changed", h);
  }, []);

  useEffect(() => {
    if (!on || !isTauriRuntime()) return;
    const pull = (): void => {
      ipc11
        .sysUptime()
        .then((u) => {
          setSysSecs(u.sysSecs);
          setTick((v) => v + 1);
        })
        .catch(() => setSysSecs(null));
      ipc
        .batteryGet()
        .then((b) => setBattery(b.hasBattery ? `${b.percent ?? "?"}%${b.acOnline ? "⚡" : ""}` : null))
        .catch(() => setBattery(null));
    };
    pull();
    const id = window.setInterval(pull, 30_000);
    return () => window.clearInterval(id);
  }, [on]);

  if (!on) return null;

  const d = sysSecs !== null ? Math.floor(sysSecs / 86400) : null;
  const h = sysSecs !== null ? Math.floor((sysSecs % 86400) / 3600) : null;

  return (
    <div className="ai11-perfhud" data-tick={tick} title={t("toolSyshub")}>
      <Activity size={13} />
      <span>
        {t("shSysUptime")} {d !== null && h !== null ? (d > 0 ? `${d}d ${h}h` : `${h}h`) : "—"}
      </span>
      {battery && <span>· {battery}</span>}
      <span className="dim">· {mode}</span>
      <button
        type="button"
        className="ai11-perfhud-x"
        aria-label={t("close")}
        onClick={() => {
          setHudEnabled(false);
          setOn(false);
          window.dispatchEvent(new Event("variable:ai11:hud-changed"));
        }}
      >
        <X size={11} />
      </button>
    </div>
  );
}
