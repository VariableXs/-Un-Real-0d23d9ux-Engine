/**
 * UNREAL-X AI-01：BootchainHealthTab.tsx — 启动链健康设置页（族0001~0010 · X00001~X00250）。
 * 10 个族各一个开关/档位选择器，值持久化到 settings.bootchain（键值白名单钳制在 settings.ts）。
 * 样式全部走既有 class + tokens（var(--sp-*)），不写裸值。
 */
import type { Settings } from "../../lib/settings";
import { BOOTCHAIN_KEYS, BOOTCHAIN_VALUE_SETS } from "../../lib/settings";
import { runAi01Checks } from "../uikit/checks";
import * as N from "../oobe/bootFailNarrative";
import * as P from "../oobe/bootPacing";

const FAMILY_META: Record<string, { title: string; desc: string }> = {
  audit: { title: "族0001 冷启动链路体检", desc: "off=现状；deep 起启用预算判定，custom 自定义阈值。" },
  health: { title: "族0002 Bootchain 健康度", desc: "阶段加权评分与五档健康档位（优/良/中/差/危）。" },
  secureboot: { title: "族0008 安全启动仪式", desc: "信任链逐环校验：off=只度量不阻断，locked=断裂即进恢复环境。" },
  persona: { title: "族0010 固件风格定制", desc: "固件皮（配色/徽标/进度样式/静默），默认关 = 与现状一致。" },
  repair: { title: "族0003 启动修复工坊", desc: "选默认修复策略；重试次数钳制 0~5。" },
  repairRetries: { title: "族0003 修复重试上限", desc: "每次失败自动重试的次数上限（0~5）。" },
  recovery: { title: "族0004 恢复环境重生", desc: "快照登记与一键续作；失败自动转恢复环境。" },
  recoveryDegrade: { title: "族0004 资源降级档", desc: "0 全量 / 1 精简 / 2 最小（工具按档裁剪）。" },
  logTheater: { title: "族0005 启动日志剧场化", desc: "启动日志按阶段成幕演出；timeline = 默认。", },
  failNarrative: { title: "族0006 引导失败叙事", desc: "错误码 → 人话叙事 + 下一步建议，禁裸报错。" },
  pacing: { title: "族0007 启动配速学", desc: "五档节奏：稳妥/均衡/疾速/竞速/静默。" },
  multiBoot: { title: "族0009 多系统选择剧场", desc: "启动项列表、默认项与上次选择记忆。" },
};

const DEGRADE_OPTIONS = [
  { v: "0", label: "0 全量" },
  { v: "1", label: "1 精简" },
  { v: "2", label: "2 最小" },
];

export function BootchainHealthTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const sel = props.settings.bootchain ?? {};
  const set = (key: string, value: string): void => {
    props.onPatch({ bootchain: { ...sel, [key]: value } });
  };

  // 族0006 叙事抽样（示例码 → 人话）。
  const sampleNarrative = N.narrate("BC-003");
  // 族0007 配速摘要。
  const pacingCaption = new P.PacingMixer(sel.pacing || "balanced").caption();
  // 自检按钮：跑 AI-01 断言组。
  const selfCheck = (): string => {
    const { entries, failed } = runAi01Checks();
    return failed.length === 0
      ? `自检通过：${entries.length} 项全绿`
      : `自检失败：${failed.map((f) => f.id).join("、")}`;
  };

  return (
    <div className="tab-body">
      <p className="dim small">
        启动可靠与恢复（X00001~X00250）：全部默认「关闭/现状」，选择后下次启动生效。
      </p>
      {BOOTCHAIN_KEYS.map((key) => {
        const meta = FAMILY_META[key];
        const options = BOOTCHAIN_VALUE_SETS[key];
        const current = sel[key] ?? "";
        const isSwitch = options.join() === "off,on";
        const label = (v: string): string => {
          if (isSwitch) return v === "on" ? "开" : "关（默认）";
          if (key === "audit") {
            return { off: "关闭（默认）", standard: "标准（现状）", deep: "深度", forensic: "取证", custom: "自定义阈值" }[v] ?? v;
          }
          if (key === "logTheater") {
            return { off: "关闭（默认）", minimal: "极简", timeline: "时间线", acts: "分幕", spotlight: "聚光灯", cinematic: "影院" }[v] ?? v;
          }
          if (key === "secureboot") {
            return { off: "关闭（默认）", audit: "审计", relaxed: "宽松", strict: "严格", locked: "锁定" }[v] ?? v;
          }
          if (key === "recoveryDegrade") return v;
          if (key === "pacing") return P.findPacing(v).name;
          if (key === "repair") return v === "off" ? "关闭（默认）" : v;
          return v;
        };
        return (
          <div key={key} className="field">
            <span className="field-label">
              {meta?.title ?? key}
              <span className="dim small"> · {meta?.desc ?? ""}</span>
            </span>
            <select
              value={current}
              onChange={(e) => set(key, e.target.value)}
              aria-label={`选择${meta?.title ?? key}档位`}
              style={{ width: "100%" }}
            >
              {!current && key !== "recoveryDegrade" ? <option value="">关闭（默认）</option> : undefined}
              {key === "recoveryDegrade"
                ? DEGRADE_OPTIONS.map((d) => (
                    <option key={d.v} value={d.v}>{d.label}</option>
                  ))
                  : options.map((v) => (
                    <option key={v} value={v}>{label(v)}</option>
                  ))}
            </select>
            {key === "pacing" && current ? <span className="dim small">{pacingCaption}</span> : undefined}
            {key === "repairRetries" ? (
              <span className="dim small">建议 2：失败先续作，两次不过再降级重试。</span>
            ) : undefined}
            {key === "failNarrative" ? (
              <span className="dim small">示例——{sampleNarrative}</span>
            ) : undefined}
          </div>
        );
      })}
      <div className="field">
        <span className="field-label">
          族0002 自检
          <span className="dim small"> · 运行 AI-01 断言组（只增不删）</span>
        </span>
        <button type="button" onClick={() => window.alert(selfCheck())}>
          运行自检
        </button>
      </div>
    </div>
  );
}
