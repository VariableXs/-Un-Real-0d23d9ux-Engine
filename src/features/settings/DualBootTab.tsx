/**
 * 阶段 7（任务 57）DualBootTab.tsx — 双域设置四组 UI：
 * ① 引导行为（默认项/倒计时/显隐）② 软件通道规则 ③ 共享白名单（入口复用白名单 Tab）
 * ④ 资源档位（办公/均衡/游戏 → ramcache 尺寸与配额联动，任务 53）。
 * 全部走 settings 总线（onPatch → saveSetting → settings://changed 双向同步沿用）。
 *
 * 双域 ③-a 追加：③ 段接上**跨域文件级同步**的实时状态（后端 `shell::filesync`）。
 * 这一段是「与 Windows 共享的内容能在 Variable 里显现、且实时更新」的唯一界面入口，
 * 所以三件事必须做实：接没接上共享盘要说清、变动要实时刷新、没接上不许假装正常。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { BootDefaultOs, ChannelPolicy, ResProfile, Settings } from "../../lib/settings";
import type { Shell } from "../../lib/ipc";
import { ipc } from "../../lib/ipc";
import {
  FALLBACK_POLL_MS,
  SYNC_EVENT,
  changeLine,
  idleHint,
  mergeDiff,
  statusHint,
  statusTitle,
  type RecentChange,
} from "../../lib/filesyncView";
import { useI18n } from "../../i18n";
import {
  autostartDisabled,
  chainHint,
  handoffDisabled,
  sharedLabel,
} from "../../lib/handoffView";

const BOOT_OS_LABELS: Record<BootDefaultOs, string> = {
  varix: "VARIX（Variable 桌面）",
  windows: "Windows",
};

const TIMEOUT_OPTIONS: Array<{ v: number; label: string }> = [
  { v: 3, label: "3 秒" },
  { v: 5, label: "5 秒（默认）" },
  { v: 10, label: "10 秒" },
  { v: 30, label: "30 秒" },
];

const CHANNEL_LABELS: Record<ChannelPolicy, string> = {
  auto: "自动（按软件登记通道）",
  "wine-first": "Wine 优先",
  "engine-first": "引擎优先",
  "native-only": "仅原生（禁用兼容通道）",
};

const RES_PROFILE_LABELS: Record<ResProfile, { label: string; desc: string }> = {
  office: { label: "办公", desc: "省内存：ramcache 256 MiB · 引擎配额 2 核 · 画质 24fps/4 Mbps" },
  balanced: { label: "均衡", desc: "默认：ramcache 1 GiB · 引擎配额自动 · 画质 30fps/8 Mbps" },
  gaming: { label: "游戏", desc: "性能：ramcache 4 GiB · 引擎配额自动+ · 画质 60fps/20 Mbps" },
};

/** 资源档位 → ramcache 尺寸（MiB）/引擎配额修正（任务 52/53 联动口径，开放性：脚本侧同源）。 */
export function resProfileQuota(p: ResProfile): { ramcacheMiB: number; coresDelta: number } {
  if (p === "office") return { ramcacheMiB: 256, coresDelta: -2 };
  if (p === "gaming") return { ramcacheMiB: 4096, coresDelta: 2 };
  return { ramcacheMiB: 1024, coresDelta: 0 };
}

/**
 * 跨域文件级同步面板（双域 ③-a 的界面入口）。
 *
 * 数据流：后端 `filesync://changed` 事件（1 秒轮询推）为主 → 本组件另挂一条
 * 4 秒兜底轮询。为什么两条都要：事件通道正常时零额外开销且延迟最低；但事件
 * 监听在窗口刚起、或后端 watcher 线程异常退出时会静默失效，界面就会「永远
 * 不刷新」——这是最难被发现的坏体验，必须有兜底。
 *
 * 空态是设计资源不是空白：「没接共享盘」是要说清楚的状态（要用户去插 U 盘），
 * 绝不能显示成「已同步 0 个文件」那种看起来一切正常的样子。
 */
function FileSyncCard(): React.ReactElement {
  const [status, setStatus] = useState<Shell.SyncStatus | null>(null);
  const [recent, setRecent] = useState<RecentChange[]>([]);
  const [recentTotal, setRecentTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  /** 组件卸载后禁止 setState（Tauri 事件回调是异步的，容易落在卸载之后）。 */
  const alive = useRef(true);

  /** 吸收一次后端返回的 (status, diff)。 */
  const absorb = useCallback((st: Shell.SyncStatus, diff: Shell.SyncDiff) => {
    if (!alive.current) return;
    setStatus(st);
    const { rows, total } = mergeDiff(diff);
    if (total === 0) return; // 空回包不清空历史行：用户刚看到的变动不该被无变动轮次抹掉
    setRecentTotal(total);
    setRecent(rows);
  }, []);

  /** 主动拉一次（首屏 + 兜底轮询 + 用户点刷新）。 */
  const pull = useCallback(async () => {
    try {
      const [st, diff] = await ipc.filesyncPoll();
      absorb(st, diff);
      if (alive.current) setErr(null);
    } catch (e) {
      // 静默失败 = 用户看着「一切正常」其实已经断了。错误必须显性化。
      if (alive.current) setErr(String(e));
    }
  }, [absorb]);

  useEffect(() => {
    alive.current = true;
    void pull();

    // 主路：后端事件。dynamic import 与全仓既有范式一致（不把 Tauri API 拉进首屏包）。
    let un: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(({ listen }) => {
      if (!alive.current) return;
      void listen<[Shell.SyncStatus, Shell.SyncDiff]>(SYNC_EVENT, (ev) => {
        const [st, diff] = ev.payload;
        absorb(st, diff);
      }).then((f) => {
        // 竞态：listen 解析出来时组件可能已卸载，此时必须立刻解绑。
        if (alive.current) un = f;
        else f();
      });
    });

    // 兜底：事件断供时界面仍能自愈。
    const tick = window.setInterval(() => void pull(), FALLBACK_POLL_MS);
    return () => {
      alive.current = false;
      un?.();
      window.clearInterval(tick);
    };
  }, [absorb, pull]);

  const reveal = useCallback(async () => {
    setBusy(true);
    try {
      await ipc.filesyncReveal();
      setErr(null);
    } catch (e) {
      setErr(String(e));
    } finally {
      if (alive.current) setBusy(false);
    }
  }, []);

  // 首帧尚未拿到状态：显示「正在读取」而不是伪造一个「未接盘」——避免闪一下错状态。
  if (!status) {
    return (
      <div className="field">
        <span className="dim small">正在读取共享盘状态…</span>
      </div>
    );
  }

  return (
    <div className="field" style={{ display: "block" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <span
          aria-hidden
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            display: "inline-block",
            background: status.available ? "#3fb950" : "#8b949e",
          }}
        />
        <strong>{statusTitle(status)}</strong>
        <span className="dim small"> · {statusHint(status)}</span>
      </div>

      {status.available ? (
        <div className="dim small" style={{ marginTop: 4, wordBreak: "break-all" }}>
          同步根：{status.root}
        </div>
      ) : null}

      {status.available && recent.length > 0 ? (
        <div style={{ marginTop: 8 }}>
          <div className="dim small">
            最近变动（本轮 {recentTotal} 项
            {recentTotal > recent.length ? `，只显示最新 ${recent.length} 项` : ""}）
          </div>
          <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
            {recent.map((r, i) => (
              <li
                key={`${r.kind}-${r.entry.rel}-${i}`}
                className="small"
                style={{ color: r.kind === "removed" ? "#d29922" : undefined, wordBreak: "break-all" }}
              >
                {changeLine(r)}
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {status.available && recent.length === 0 ? (
        <div className="dim small" style={{ marginTop: 6 }}>
          {idleHint()}
        </div>
      ) : null}

      <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center" }}>
        <button type="button" className="btn" onClick={() => void reveal()} disabled={busy || !status.available}>
          {busy ? "打开中…" : "打开共享盘"}
        </button>
        <button type="button" className="btn" onClick={() => void pull()}>
          立即刷新
        </button>
      </div>

      {err ? (
        <div className="small" role="alert" style={{ color: "#f85149", marginTop: 6 }}>
          同步状态读取失败：{err}
        </div>
      ) : null}
    </div>
  );
}

/**
 * 需求 2 卡片：内核 → Variable 的交接。
 *
 * 这一张卡是「A 卡（VARIX + VARIABLE）加载完能不能真的进到 Variable 桌面」
 * 的**唯一界面入口**，所以必须把两个半段都摆出来：
 *   ① 内核侧交接开关（写 SHARED 卷的 boot-select.json，内核读同一份文件）；
 *   ② Windows 侧开机自启（HKCU Run）——少了这一段，交接过去只会看到普通
 *      Windows 桌面，而不是 Variable。
 * 并且如实交代「中间会有一次重启」——那是 UEFI 的硬约束（内核早期已交还
 * 引导服务，无法跳转到 Windows Boot Manager），不是实现偷懒。
 */
function HandoffCard(): React.ReactElement {
  const [cfg, setCfg] = useState<Shell.BootCfgView | null>(null);
  const [auto, setAuto] = useState<{ on: boolean; command: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [note, setNote] = useState("");

  const reload = useCallback(async (): Promise<void> => {
    try {
      // 自启状态读不到时不该拖垮整卡（例如非 Windows 宿主）——分别兜住。
      const c = await ipc.dualbootStatus();
      setCfg(c);
      setErr("");
      try {
        setAuto(await ipc.autostartGet());
      } catch {
        setAuto(null);
      }
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const toggleHandoff = async (on: boolean): Promise<void> => {
    setBusy(true);
    setNote("");
    try {
      setCfg(await ipc.dualbootSetHandoff(on));
      setErr("");
      setNote(on ? "已开启：内核加载完会自动交接（可在内核侧确认）" : "已关闭：内核会停在自绘 ushell");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleAuto = async (on: boolean): Promise<void> => {
    setBusy(true);
    setNote("");
    try {
      setAuto(await ipc.autostartSet(on));
      setErr("");
      setNote(on ? "已设为开机自启：Windows 起来后自动全屏进入 Variable" : "已关闭开机自启");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const hint = chainHint(cfg, auto);
  return (
    <div className="field" style={{ display: "block" }}>
      <span className="field-label">
        内核 → Variable 交接
        <span className="dim small">
          · 内核里跑不了 Tauri（需要 Windows + WebView2），所以 A 卡加载完会
          把引导权交回 Windows，由那边的 Variable 自启全屏
        </span>
      </span>

      {err ? (
        <p className="small" role="alert" style={{ margin: "6px 0 0", opacity: 0.9 }}>
          {err}
        </p>
      ) : null}

      <label
        className="field"
        style={{ display: "flex", gap: 8, alignItems: "flex-start", marginTop: 6 }}
      >
        <input
          type="checkbox"
          checked={cfg?.handoff ?? true}
          disabled={handoffDisabled(cfg, busy)}
          onChange={(e) => void toggleHandoff(e.target.checked)}
          aria-label="内核加载完交接给 Windows 上的 Variable"
        />
        <span>
          <strong>内核加载完自动交接</strong>
          <span className="dim small">
            {" "}
            · 开：看到 VARIX 加载后自动进 Variable 桌面（中间有一次重启，这是
            UEFI 硬约束）；关：停在 VARIX 自绘 ushell
          </span>
        </span>
      </label>

      <label
        className="field"
        style={{ display: "flex", gap: 8, alignItems: "flex-start" }}
      >
        <input
          type="checkbox"
          checked={auto?.on ?? false}
          disabled={autostartDisabled(auto, busy)}
          onChange={(e) => void toggleAuto(e.target.checked)}
          aria-label="Windows 开机自动进入 Variable"
        />
        <span>
          <strong>Windows 开机自动进入 Variable</strong>
          <span className="dim small">
            {" "}
            · 交接的后半段：没有它，交接过去只会看到普通 Windows 桌面
            {auto?.on && auto.command ? `（当前：${auto.command}）` : ""}
          </span>
        </span>
      </label>

      {hint ? (
        <p className="dim small" role="status" style={{ margin: "6px 0 0" }}>
          {hint}
        </p>
      ) : null}

      <p className="dim small" style={{ margin: "6px 0 0" }}>
        {sharedLabel(cfg)}
      </p>

      {cfg?.sharedRoot ? (
        <p className="small" style={{ margin: "6px 0 0" }}>
          <button type="button" className="btn" onClick={() => void ipc.filesyncReveal()}>
            打开共享盘
          </button>
          <button
            type="button"
            className="btn"
            style={{ marginLeft: 8 }}
            onClick={() => void reload()}
          >
            重新读取
          </button>
          {note ? <span className="dim small"> · {note}</span> : null}
        </p>
      ) : null}
    </div>
  );
}

export function DualBootTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const s = props.settings;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]): void =>
    props.onPatch({ [key]: value } as Partial<Settings>);

  return (
    <div className="tab-body">
      <p className="dim small">
        双域系统（VARIX + Windows 引擎）：引导行为与通道规则保存后经 settings://changed
        双向同步，便携脚本与引导页下次生效。
      </p>

      {/* ① 引导行为 */}
      <h3 className="w11-sec-title">引导行为</h3>
      <div className="field">
        <span className="field-label">
          倒计时默认进入
          <span className="dim small"> · 引导页倒计时结束后自动进入的系统</span>
        </span>
        <select
          value={s.bootDefaultOs}
          onChange={(e) => set("bootDefaultOs", e.target.value as BootDefaultOs)}
          aria-label="倒计时默认进入的系统"
          style={{ width: "100%" }}
        >
          {(Object.keys(BOOT_OS_LABELS) as BootDefaultOs[]).map((k) => (
            <option key={k} value={k}>{BOOT_OS_LABELS[k]}</option>
          ))}
        </select>
      </div>
      <div className="field">
        <span className="field-label">
          倒计时秒数
          <span className="dim small"> · 按 Esc 可跳过倒计时</span>
        </span>
        <select
          value={s.bootTimeoutSec}
          onChange={(e) => set("bootTimeoutSec", Number(e.target.value))}
          aria-label="引导页倒计时秒数"
          style={{ width: "100%" }}
        >
          {TIMEOUT_OPTIONS.map((o) => (
            <option key={o.v} value={o.v}>{o.label}</option>
          ))}
        </select>
      </div>
      <label className="field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input
          type="checkbox"
          checked={s.bootMenuVisible}
          onChange={(e) => set("bootMenuVisible", e.target.checked)}
        />
        <span>
          显示引导菜单
          <span className="dim small"> · 关闭后倒计时直进默认系统（Esc 仍可呼出菜单）</span>
        </span>
      </label>

      {/* ①-b 需求 2：内核 → Variable 交接（A 卡的落点） */}
      <h3 className="w11-sec-title">内核 → Variable 交接</h3>
      <HandoffCard />

      {/* ② 软件通道规则 */}
      <h3 className="w11-sec-title">软件通道规则</h3>
      <div className="field">
        <span className="field-label">
          全局通道优先序
          <span className="dim small"> · 逐软件覆盖在适配看板（SHARED apps.json）里登记</span>
        </span>
        <select
          value={s.channelPolicy}
          onChange={(e) => set("channelPolicy", e.target.value as ChannelPolicy)}
          aria-label="软件通道优先序"
          style={{ width: "100%" }}
        >
          {(Object.keys(CHANNEL_LABELS) as ChannelPolicy[]).map((k) => (
            <option key={k} value={k}>{CHANNEL_LABELS[k]}</option>
          ))}
        </select>
      </div>

      {/* ③ 共享白名单（入口） */}
      <h3 className="w11-sec-title">共享白名单</h3>
      <div className="field">
        <span className="field-label">
          SHARED 分区白名单与审计
          <span className="dim small">
            · 管理见「共享白名单」页（{t("wlTitle")}），审计查看见「白名单审计」页
          </span>
        </span>
      </div>
      <FileSyncCard />

      {/* ④ 资源档位 */}
      <h3 className="w11-sec-title">资源档位</h3>
      {(Object.keys(RES_PROFILE_LABELS) as ResProfile[]).map((k) => {
        const meta = RES_PROFILE_LABELS[k];
        const quota = resProfileQuota(k);
        return (
          <label key={k} className="field" style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
            <input
              type="radio"
              name="res-profile"
              checked={s.resProfile === k}
              onChange={() => set("resProfile", k)}
              style={{ marginTop: 4 }}
            />
            <span>
              <strong>{meta.label}</strong>
              <span className="dim small">
                {" "}
                · {meta.desc}（脚本口径：ramcache {quota.ramcacheMiB} MiB）
              </span>
            </span>
          </label>
        );
      })}
    </div>
  );
}
