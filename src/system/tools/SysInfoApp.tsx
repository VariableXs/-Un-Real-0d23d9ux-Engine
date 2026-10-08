import { useCallback, useEffect, useState } from "react";
import { ClipboardCopy, RefreshCw } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { formatBytes } from "../../lib/format";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import type { Shell } from "../../lib/ipc";
import "../../styles/ai08-sysinfo.css";

/**
 * Z-27 系统信息面板（VWM 虚拟窗口应用，只读）：
 * - 四个数据源全部走既有 IPC：sysSelfInfo（版本/运行档/运行时长/数据目录占用/OS）、
 *   sysBrief（CPU/内存/开机时长）、sysenvOverview（显示器/时区/电源计划/用户）、
 *   sysDisks（磁盘容量）；每项标注数据来源与刷新时间
 * - 「刷新」按钮显式拉取（默认不自动轮询）；失败保留上次数据并如实标注错误
 * - 「复制诊断摘要」：版本/运行档/运行时长/OS/CPU/内存/GPU/磁盘/数据目录占用
 *   拼接多行文本 → navigator.clipboard + toast
 * - GPU 走 WebGL 渲染器信息（本地读取，零网络）；读不到如实标「未获取」
 * 红线：只读面板，无任何写操作；不自动联网；不做性能监控图表（任务管理器领地）。
 */

/** 单个数据区块状态：data 保留上次成功值；err 非空 = 最近一次拉取失败。 */
interface Sec<T> {
  data: T | null;
  err: string | null;
  at: number | null;
}

/** GPU 名称（WebGL UNMASKED_RENDERER，本地读取；失败返回 null）。 */
function readGpu(): string | null {
  try {
    const cv = document.createElement("canvas");
    const gl = cv.getContext("webgl");
    if (!gl) return null;
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    if (!ext) return null;
    const s = gl.getParameter(ext.UNMASKED_RENDERER_WEBGL);
    return typeof s === "string" && s ? s : null;
  } catch {
    return null;
  }
}

function fmtClock(ts: number): string {
  const d = new Date(ts);
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

export function SysInfoApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [self, setSelf] = useState<Sec<Shell.SysSelfInfo>>({ data: null, err: null, at: null });
  const [brief, setBrief] = useState<Sec<Shell.SysBrief>>({ data: null, err: null, at: null });
  const [env, setEnv] = useState<Sec<Shell.SysEnvOverview>>({ data: null, err: null, at: null });
  const [disks, setDisks] = useState<Sec<Shell.SysDisk[]>>({ data: null, err: null, at: null });
  const [gpu, setGpu] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 显式刷新（挂载时拉一次；之后仅手动触发，默认不自动轮询）
  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    setBusy(true);
    const now = Date.now();
    ipc
      .sysSelfInfo()
      .then((data) => setSelf({ data, err: null, at: now }))
      .catch((e: unknown) => setSelf((p) => ({ data: p.data, err: errMessage(e).message, at: now })));
    ipc
      .sysBrief()
      .then((data) => setBrief({ data, err: null, at: now }))
      .catch((e: unknown) => setBrief((p) => ({ data: p.data, err: errMessage(e).message, at: now })));
    ipc
      .sysenvOverview()
      .then((data) => setEnv({ data, err: null, at: now }))
      .catch((e: unknown) => setEnv((p) => ({ data: p.data, err: errMessage(e).message, at: now })));
    ipc
      .sysDisks()
      .then((data) => setDisks({ data, err: null, at: now }))
      .catch((e: unknown) => setDisks((p) => ({ data: p.data, err: errMessage(e).message, at: now })));
    setGpu(readGpu());
    // 四路独立落定后统一复位（不阻塞任何一路失败）
    window.setTimeout(() => setBusy(false), 200);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const fmtUptime = (secs: number): string => {
    const d = Math.floor(secs / 86400);
    const h = Math.floor((secs % 86400) / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const parts: string[] = [];
    if (d > 0) parts.push(`${d}${t("siUptimeDay")}`);
    if (h > 0) parts.push(`${h}${t("siUptimeHour")}`);
    parts.push(`${m}${t("siUptimeMin")}`);
    return parts.join(" ");
  };

  const memPct = brief.data && brief.data.memTotal > 0 ? Math.round((brief.data.memUsed / brief.data.memTotal) * 100) : null;

  /** 诊断摘要（版本/运行档/运行时长/OS/CPU/内存/GPU/磁盘/数据目录占用）。 */
  const buildSummary = (): string => {
    const na = t("siNA");
    const lines: string[] = [t("siSummaryHead")];
    lines.push(`${t("siVersion")}: ${self.data?.version ?? na}`);
    lines.push(`${t("siRuntimeMode")}: ${self.data?.runtimeMode ?? brief.data?.runtimeMode ?? na}`);
    lines.push(`${t("siUptime")}: ${self.data ? fmtUptime(self.data.uptimeSecs) : na}`);
    lines.push(`OS: ${self.data?.osVersion ?? na}`);
    lines.push(`${t("siCpu")}: ${brief.data ? `${brief.data.cpu}%` : na}`);
    lines.push(
      `${t("siMem")}: ${
        brief.data && brief.data.memTotal > 0
          ? `${formatBytes(brief.data.memUsed)} / ${formatBytes(brief.data.memTotal)}（${memPct ?? "?"}%）`
          : na
      }`,
    );
    lines.push(
      `${t("siGpu")}: ${gpu ? `${gpu}（WebGL）` : na}`,
    );
    lines.push(
      `${t("siDisks")}: ${
        disks.data && disks.data.length > 0
          ? disks.data
              .map((d) => {
                const pct = d.total > 0 ? Math.round(((d.total - d.free) / d.total) * 100) : 0;
                return `${d.letter} ${pct}%（${formatBytes(d.total - d.free)}/${formatBytes(d.total)}）`;
              })
              .join("；")
          : na
      }`,
    );
    lines.push(
      `${t("siDataDirBytes")}: ${
        self.data ? `${self.data.dataDir}（${formatBytes(self.data.dataDirBytes)}${self.data.dataDirCapped ? t("siDataDirCapped") : ""}）` : na
      }`,
    );
    return lines.join("\n");
  };

  const copySummary = (): void => {
    void navigator.clipboard
      .writeText(buildSummary())
      .then(() => pushToast("success", t("siTitle"), t("siCopied")))
      .catch(() => pushToast("error", t("siTitle"), t("siCopyFail")));
  };

  // ---------- 区块渲染 ----------

  const secHead = (title: string, src: string, at: number | null): React.ReactElement => (
    <div className="si-sec-head">
      <span className="si-sec-title">{title}</span>
      <span className="si-src dim small">
        {t("siSource")}: {src}
        {at !== null ? ` · ${t("siRefreshed")} ${fmtClock(at)}` : ""}
      </span>
    </div>
  );

  const secErr = (err: string | null): React.ReactElement | null =>
    err ? <p className="si-err small">{t("siLoadFail", { msg: err })}</p> : null;

  const row = (label: string, value: React.ReactNode): React.ReactElement => (
    <div className="si-row">
      <span className="si-row-label dim small">{label}</span>
      <span className="si-row-value small">{value}</span>
    </div>
  );

  if (!isTauriRuntime()) {
    return (
      <div className="si-app" aria-label={t("siTitle")}>
        <p className="dim small si-empty">{t("siNoTauri")}</p>
      </div>
    );
  }

  return (
    <div className="si-app" aria-label={t("siTitle")}>
      <div className="si-toolbar">
        <span className="si-title">{t("siTitle")}</span>
        <span className="flex-1" />
        <button type="button" className="btn ghost tiny" disabled={busy} onClick={refresh} aria-label={t("siRefresh")}>
          <RefreshCw size={11} className={busy ? "si-spinning" : undefined} /> {t("siRefresh")}
        </button>
        <button type="button" className="btn primary tiny" onClick={copySummary}>
          <ClipboardCopy size={11} /> {t("siCopySummary")}
        </button>
      </div>

      <div className="si-body">
        <section className="si-sec">
          {secHead(t("siSecSelf"), "sys_self_info", self.at)}
          {secErr(self.err)}
          {row(t("siVersion"), self.data?.version ?? t("siNA"))}
          {row(t("siRuntimeMode"), self.data?.runtimeMode ?? t("siNA"))}
          {row(t("siUptime"), self.data ? fmtUptime(self.data.uptimeSecs) : t("siNA"))}
          {row("OS", self.data?.osVersion ?? t("siNA"))}
          {row(
            t("siDataDirBytes"),
            self.data ? (
              <>
                {formatBytes(self.data.dataDirBytes)}
                {self.data.dataDirCapped ? t("siDataDirCapped") : ""} · <span className="dim">{self.data.dataDir}</span>
              </>
            ) : (
              t("siNA")
            ),
          )}
        </section>

        <section className="si-sec">
          {secHead(t("siSecBrief"), "sys_brief", brief.at)}
          {secErr(brief.err)}
          {row(t("siCpu"), brief.data ? `${brief.data.cpu}%` : t("siNA"))}
          {row(
            t("siMem"),
            brief.data && brief.data.memTotal > 0 ? (
              <>
                {formatBytes(brief.data.memUsed)} / {formatBytes(brief.data.memTotal)}（{memPct ?? "?"}%）
                <span className="si-mem-bar">
                  <span className="si-mem-fill" style={{ width: `${memPct ?? 0}%` }} />
                </span>
              </>
            ) : (
              t("siNA")
            ),
          )}
          {row(t("siGpu"), gpu ? `${gpu} · WebGL` : t("siNA"))}
        </section>

        <section className="si-sec">
          {secHead(t("siSecEnv"), "sysenv_overview", env.at)}
          {secErr(env.err)}
          {row(
            t("siDisplays"),
            env.data && env.data.displays.length > 0 ? (
              env.data.displays.map((d) => (
                <span key={d.device} className="si-display">
                  {d.current ? `${d.current.width}×${d.current.height}@${d.current.hz}Hz` : t("siNA")}
                  {d.primary ? ` · ${t("siPrimary")}` : ""}
                  {` · ${d.name}`}
                </span>
              ))
            ) : (
              t("siNA")
            ),
          )}
          {row(
            t("siTimezone"),
            env.data
              ? `${env.data.timezone}（UTC${
                  env.data.utc_offset_minutes >= 0 ? "+" : "-"
                }${String(Math.floor(Math.abs(env.data.utc_offset_minutes) / 60)).padStart(2, "0")}:${String(
                  Math.abs(env.data.utc_offset_minutes) % 60,
                ).padStart(2, "0")}）`
              : t("siNA"),
          )}
          {row(t("siPower"), env.data ? `${env.data.power_scheme_name}（${env.data.power_scheme}）` : t("siNA"))}
          {row(t("siUser"), env.data?.username ?? t("siNA"))}
          {row(
            t("siVmMode"),
            env.data ? `${env.data.vm ? t("siVmOn") : t("siVmOff")}${env.data.vm_reason ? ` · ${env.data.vm_reason}` : ""}` : t("siNA"),
          )}
        </section>

        <section className="si-sec">
          {secHead(t("siSecDisks"), "sys_disks", disks.at)}
          {secErr(disks.err)}
          {disks.data && disks.data.length > 0 ? (
            <div className="si-disks">
              {disks.data.map((d) => {
                const used = d.total - d.free;
                const pct = d.total > 0 ? Math.round((used / d.total) * 100) : 0;
                return (
                  <div key={d.letter} className="si-disk">
                    <div className="si-disk-head small">
                      <span className="si-disk-letter">{d.letter}</span>
                      <span className="dim">{formatBytes(used)} / {formatBytes(d.total)}</span>
                      <span className="flex-1" />
                      <span className="dim">{pct}%</span>
                    </div>
                    <div className="si-disk-bar">
                      <div className="si-disk-fill" style={{ width: `${pct}%` }} />
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            row(t("siDisks"), t("siNA"))
          )}
        </section>
      </div>
    </div>
  );
}
