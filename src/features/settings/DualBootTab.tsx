/**
 * 阶段 7（任务 57）DualBootTab.tsx — 双域设置四组 UI：
 * ① 引导行为（默认项/倒计时/显隐）② 软件通道规则 ③ 共享白名单（入口复用白名单 Tab）
 * ④ 资源档位（办公/均衡/游戏 → ramcache 尺寸与配额联动，任务 53）。
 * 全部走 settings 总线（onPatch → saveSetting → settings://changed 双向同步沿用）。
 */
import type { BootDefaultOs, ChannelPolicy, ResProfile, Settings } from "../../lib/settings";
import { useI18n } from "../../i18n";

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
