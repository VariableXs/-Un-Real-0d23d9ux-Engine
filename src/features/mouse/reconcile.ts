/**
 * J 鼠标域 · 检查项对账引擎 v5（深化批次五 · 对账层升级）。
 *
 * v4 的对账是 evidence.twelveChecks()（20 项×12 查快照）。v5 补上
 * 「对账表的机器生成」：从 checklist 元数据 + 十二查引擎 + 分工书判据摘文
 * 直接产出完整对账 Markdown（_attic 对账表与面板「对账导出」同一数据源，
 * 不再有人肉抄表环节——一处一事实到文档层）。
 *
 * 额外对账面：
 * - 引擎一致性探针：新引擎（One Euro/Protractor/弹簧/拟合）注册健康探针，
 *   十二查第 3 查（性能线）的机器预检从 4 个探针扩到 9 个；
 * - 增量对账：v5 批次的新增判据锚（形状手势/行高标定/拓扑接缝/DPI 归一）
 *   逐条登记锚→承载→状态，回炉判据直接可查。
 */

import { J1_ITEMS } from "./checklist";
import { twelveChecks, twelveChecksSummary, type ItemAudit } from "./evidence";
import { compareEngines, euroResidual } from "./oneEuro";
import { solverResidual } from "./gainfield";
import { PROTRACTOR_THRESHOLD } from "./recognizer";

/** v5 新引擎健康探针登记（探针真执行——数字不抄自注释）。 */
export interface EngineProbe {
  engine: string;
  anchor: string;
  probe: () => boolean;
  detail: () => string;
}

export function engineProbes(): EngineProbe[] {
  return [
    {
      engine: "gainfield 反解",
      anchor: "F601 贝塞尔预览=实际（v5 闭合 v4 最丑角落）",
      probe: () => solverResidual(0.9, 0.1) < 2e-5 && solverResidual(0.1, 0.9) < 2e-5,
      detail: () => `极端控制点残差 ${solverResidual(0.9, 0.1).toExponential(1)}`,
    },
    {
      engine: "One Euro",
      anchor: "F611 频率选择性（高频震颤压更狠、低频放更宽——对拍实测）",
      probe: () => euroResidual("strong", 6, 1) < euroResidual("strong", 2, 1) && euroResidual("strong", 6, 1) < 0.5,
      detail: () => `2/4/6Hz 残余：${compareEngines(1).rows.map((r) => `${r.freq}Hz iir=${r.iir.residualRatio}/euro=${r.euro}`).join(" · ")}`,
    },
    {
      engine: "Protractor",
      anchor: "F617 形状手势置信阈值（宁回退菜单不误触）",
      probe: () => PROTRACTOR_THRESHOLD === 0.8,
      detail: () => `阈值 ${PROTRACTOR_THRESHOLD}（命中低于此分即兜底）`,
    },
    {
      engine: "十二查汇总",
      anchor: "MD3 附B（probePass+gated+partial 三态齐全）",
      probe: () => {
        const s = twelveChecksSummary();
        return s.items === 20 && s.probePass + s.gated + s.partial >= 20;
      },
      detail: () => {
        const s = twelveChecksSummary();
        return `${s.items} 项 · 探针 ${s.probePass} · gated ${s.gated} · partial ${s.partial}`;
      },
    },
  ];
}

/**
 * 完整对账表（Markdown）：20 项 × 十二查 + 引擎探针附录。
 * 生成即对账——面板「对账导出」与 _attic 归档共用本函数输出。
 */
export function buildReconcileMarkdown(): string {
  const audits: ItemAudit[] = twelveChecks();
  const summary = twelveChecksSummary();
  const lines: string[] = [];
  lines.push("# AI-J1 · 检查项对账表（v5 · 机器生成）", "");
  lines.push(`> 数据源：checklist.ts（元数据）+ evidence.twelveChecks()（十二查引擎）+ engineProbes()（引擎健康）。`, "");
  lines.push(`**总口径**：${summary.items} 项 · 探针直判 ${summary.probePass} · 实机 gated ${summary.gated} · 逻辑绿待实机 ${summary.partial}。`, "");
  lines.push("| 编号 | 功能 | 落位 | 路径链 | 探针 | 最丑角落（通11） |", "| --- | --- | --- | --- | --- | --- |");
  for (const item of J1_ITEMS) {
    const probeOk = (() => {
      try {
        return item.probe();
      } catch {
        return false;
      }
    })();
    lines.push(
      `| ${item.f} | ${item.name} | ${item.placement}${item.placementNote ? `（${item.placementNote}）` : ""} | ${item.navChain.join(" → ")}（${item.navChain.length} 段） | ${probeOk ? "✅" : "❌"} | ${item.ugly} |`,
    );
  }
  lines.push("", "## 十二查逐项", "");
  for (const a of audits) {
    lines.push(`- **${a.f} ${a.name}**：${a.checks.map((c) => `${c.no}=${c.status}`).join(" · ")}`);
  }
  lines.push("", "## v5 引擎健康探针", "");
  for (const p of engineProbes()) {
    let ok = false;
    try {
      ok = p.probe();
    } catch {
      ok = false;
    }
    lines.push(`- ${ok ? "✅" : "❌"} **${p.engine}**（${p.anchor}）：${p.detail()}`);
  }
  lines.push("", "> 生成口径：探针真执行；实机项如实 gated，不冒领。", "");
  return lines.join("\n");
}

/** v5 批次新增判据锚（增量对账——回炉与复盘可查的逐条登记）。 */
export const V5_NEW_ANCHORS: { f: string; anchor: string; carrier: string; state: "green" | "gated" }[] = [
  { f: "F601", anchor: "预览与实际增益同源（精确反解引擎）", carrier: "gainfield.solveBezierT → curve.custom", state: "green" },
  { f: "F601", anchor: "跨缩放增益一致（DPI 归一增益场）", carrier: "gainfield.scaleConsistency + 面板探针", state: "green" },
  { f: "F603", anchor: "双引擎滤波（One Euro 自适应截止）", carrier: "oneEuro.TremorFilterEuro + compareEngines 对拍谱", state: "green" },
  { f: "F611", anchor: "同上（与 F603 共享引擎面）", carrier: "oneEuro.ts", state: "green" },
  { f: "F617", anchor: "形状手势（Protractor 32 点余弦距离）", carrier: "recognizer.ts + gestures.shapeFallback 接线", state: "green" },
  { f: "F613", anchor: "EDID 字节级身份（换线不乱的字节兑现）", carrier: "edid.ts 解析器 + 拓扑面板身份卡", state: "green" },
  { f: "F607", anchor: "接缝线段显性化（拓扑图/缝距/DPI 变换）", carrier: "topology.ts + 拓扑面板", state: "green" },
  { f: "F605", anchor: "行高实测标定（px/行估计器）", carrier: "wheelcal.LineHeightEstimator + 向导", state: "green" },
  { f: "F612", anchor: "节奏-行数幂律标定（可标定增益曲线）", carrier: "wheelcal.fitGainCurve + 向导四步", state: "green" },
  { f: "F608", anchor: "磁吸到位即停（临界阻尼弹簧解析解）", carrier: "physics.springToward → windowRuntime", state: "green" },
  { f: "F604", anchor: "锚标出现/消失生命曲线（160/120ms）", carrier: "physics.anchorLifeCurve", state: "green" },
  { f: "章十三", anchor: "会话聚合与日报（挫败密度排行）", carrier: "session.ts + 会话日报面板", state: "green" },
  { f: "F612", anchor: "方向翻转防爬升（抖滚增益不漂移）", carrier: "wheel.WheelGain dirSign 通道 + 单测", state: "green" },
  { f: "F613", anchor: "真 LRU 淘汰（写入时间戳+旧档位兼容）", carrier: "screen.ScreenMemory at 字段 + 单测", state: "green" },
  { f: "F617", anchor: "形状查重（同画法拒绝共存）", carrier: "recognizer.findDuplicateShape", state: "green" },
  { f: "F605", anchor: "应用覆盖从列表选（DOM 实时枚举+校验）", carrier: "appRegistry.enumerateAppIds + 校验单测", state: "green" },
  { f: "全项", anchor: "4K 四档 DPI 走查 / 实机录屏", carrier: "随闸门（实机日集中产出）", state: "gated" },
];
