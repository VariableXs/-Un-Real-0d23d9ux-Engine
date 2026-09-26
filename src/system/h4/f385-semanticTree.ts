/**
 * F385 无障碍语义树（H 域 · AI-H4）：
 * 全部界面控件暴露语义信息（角色/名称/状态/层级——自研无障碍树，接口对齐开放标准
 * F141 纪律）：每个按钮有名字（图标按钮必须有 ToolTip 即名字源 F205 联动）、每个状态
 * 可读（开关读「开/关」而非视觉）、弹窗打开即播报标题。
 * 判据（主册 F385）：语义树覆盖率（全系统控件扫描，无名可交互元素=0）；角色/状态
 * 完整性抽查 50 控件；弹窗播报；第三方读屏接口文档公开（F135 文档站）。
 * 依赖锚点：F135 开发者文档站 / F141 无障碍开放标准 / F205 Tooltip。
 */

/** 语义角色（对齐开放标准角色词表的 VARIX 子集）。 */
export const ROLES = ["button", "checkbox", "switch", "textbox", "list", "listitem", "dialog", "menu", "menuitem", "tab", "tree", "treeitem", "progressbar", "slider"] as const;
export type AriaRole = (typeof ROLES)[number];

/** 需要名字的角色（判据「无名可交互元素=0」——全部可交互角色）。 */
const INTERACTIVE_ROLES: ReadonlySet<AriaRole> = new Set(["button", "checkbox", "switch", "textbox", "menuitem", "tab", "treeitem", "slider"]);

export interface SemanticNode {
  id: string;
  role: AriaRole;
  /** 名称源：文本内容 / aria-label / ToolTip（F205 联动）。 */
  name: string | null;
  nameSource: "text" | "label" | "tooltip" | null;
  /** 状态的可读描述（判据「开关读开/关而非视觉」）。 */
  state: string | null;
  /** 层级路径（父 id 链）。 */
  parentIds: string[];
}

export interface SemanticTree {
  nodes: Map<string, SemanticNode>;
}

export function createTree(nodes: SemanticNode[]): SemanticTree {
  return { nodes: new Map(nodes.map((n) => [n.id, n])) };
}

/** 覆盖率审计（判据）：全部可交互元素必须有名字；无名=0 才合格。 */
export function auditNameCoverage(tree: SemanticTree): { pass: boolean; unnamed: string[] } {
  const unnamed: string[] = [];
  for (const n of tree.nodes.values()) {
    if (INTERACTIVE_ROLES.has(n.role) && (!n.name || !n.name.trim())) unnamed.push(n.id);
  }
  return { pass: unnamed.length === 0, unnamed };
}

/** 角色/状态完整性抽查（判据「抽查 50 控件」）：角色在词表内、状态可读。 */
export function auditRoleState(tree: SemanticTree, sample = 50): { sampled: number; badRoles: string[]; unreadableStates: string[]; pass: boolean } {
  const list = [...tree.nodes.values()].slice(0, sample);
  const badRoles = list.filter((n) => !ROLES.includes(n.role)).map((n) => n.id);
  const stateRoles: ReadonlySet<AriaRole> = new Set(["checkbox", "switch", "progressbar", "slider", "treeitem"]);
  const unreadableStates = list.filter((n) => stateRoles.has(n.role) && (!n.state || !n.state.trim())).map((n) => n.id);
  return { sampled: list.length, badRoles, unreadableStates, pass: badRoles.length === 0 && unreadableStates.length === 0 };
}

/** 开关状态可读化（判据示例）：布尔 → 「开」/「关」（不靠视觉/颜色）。 */
export function readableSwitchState(on: boolean): string {
  return on ? "开" : "关";
}

/** 进度可读化：67% → 「进度 67%」。 */
export function readableProgress(pct: number): string {
  const p = Math.min(100, Math.max(0, Math.round(pct)));
  return `进度 ${p}%`;
}

export interface Announcement {
  text: string;
  /** polite=等待空闲播报（弹窗标题）；assertive=立即（危险确认）。 */
  priority: "polite" | "assertive";
}

/** 弹窗打开即播报（判据）：标题 + polite 优先级。 */
export function announceDialog(title: string): Announcement {
  return { text: `对话框：${title}`, priority: "polite" };
}

/** 层级朗读：路径链 → 「位于 X 内的 Y」。 */
export function speakHierarchy(tree: SemanticTree, id: string): string {
  const node = tree.nodes.get(id);
  if (!node) return "";
  if (node.parentIds.length === 0) return node.name ?? node.role;
  const chain = node.parentIds.map((p) => tree.nodes.get(p)?.name ?? p);
  return `位于 ${chain.join(" 内的 ")} 的 ${node.name ?? node.role}`;
}

/** 第三方读屏接口文档（判据「文档公开」）：接口描述导出（F135 文档站数据源）。 */
export function readerInterfaceDoc(): { version: string; standard: string; endpoints: string[] } {
  return {
    version: "1.0",
    standard: "F141 开放标准对齐（WAI-ARIA 词表子集）",
    endpoints: ["getTree(): SemanticNode[]", "subscribe(cb): unsubscribe", "announce(a: Announcement): void"],
  };
}
