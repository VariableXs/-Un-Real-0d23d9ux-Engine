/**
 * AURORA-10000 · AI-11~AI-15 车道 · 领域03「桌面设计·桌面与图标」共享类型。
 *
 * 交付口径（全景图 §参数族说明）：凡「×25 型/档/预设」标注的族，每一行都是
 * 独立可交付单元（独立参数档、资产或开关），验收同族标准。
 * - kind "preset"   ：族内单选的参数档（同族同时只生效一档）；
 * - kind "switch"   ：独立开关/能力项（各自启停，互不影响）；
 * - kind "reserved" ：预留位（接口冻结 + 文档 + 开关存在，不虚标完成，
 *                     见实施总步骤图 §15.6 预留位验收口径）。
 * 一切颜色与动效参数最终汇入 runtime.ts 写入 :root 的 --w2-* 令牌，
 * 禁止在本车道任何组件硬编码裸值绕过令牌。
 */

export type Lang = "zh" | "en";

export interface Label {
  zh: string;
  en: string;
}

export type EntryKind = "preset" | "switch" | "reserved";

export interface DesignEntry {
  /** 全景图唯一编号，如 F01251。 */
  id: string;
  /** 稳定 slug（localStorage 持久化用，不随文案改）。 */
  key: string;
  /** 所属族 id，如 f0051。 */
  family: string;
  kind: EntryKind;
  label: Label;
  /** 启用/选中后写入 :root 的 CSS 变量（--w2-* 或既有语义令牌）。 */
  vars?: Record<string, string>;
  /** 运行时消费的行为参数（纯数据，不接触 DOM）。 */
  params?: Record<string, unknown>;
  /** 诚实边界 / 预留位说明。 */
  note?: Label;
}

export interface DesignFamily {
  /** 族 id，如 f0051。 */
  id: string;
  /** 归属 AI 编号。 */
  ai: string;
  /** 族编号（如 "0051"）。 */
  no: string;
  title: Label;
  /** 全景图功能号区间 [起, 止]。 */
  range: [number, number];
  entries: DesignEntry[];
}

/** 构造入口行的小工具（收口 id 格式校验）。 */
export function entry(
  id: number,
  family: string,
  kind: EntryKind,
  key: string,
  zh: string,
  en: string,
  extra?: Omit<Partial<DesignEntry>, "id" | "family" | "kind" | "key" | "label">,
): DesignEntry {
  const text = String(id);
  if (!/^F\d{5}$/.test("F" + text.padStart(5, "0"))) {
    throw new Error(`bad feature id: ${id}`);
  }
  return {
    id: "F" + text.padStart(5, "0"),
    key,
    family,
    kind,
    label: { zh, en },
    ...extra,
  };
}

export function family(
  id: string,
  no: string,
  ai: string,
  zh: string,
  en: string,
  range: [number, number],
  entries: DesignEntry[],
): DesignFamily {
  return {
    id,
    no,
    ai,
    title: { zh, en },
    range,
    entries,
  };
}
