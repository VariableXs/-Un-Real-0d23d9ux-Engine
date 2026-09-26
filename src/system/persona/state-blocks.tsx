/**
 * 通用十二查 8/9/11 · 状态块渲染层（message-catalog → 二十页挂载件）。
 *
 * 主册判据延伸：
 * - 「空态是设计资源」「错误三要素」「破坏性二次确认」——目录有了、
 *   渲染件在此：二十页统一用这三个块，不再各写各的（十章一致性）；
 * - 「取消永远是安全出路」：确认块取消钮固定在安全侧（布局契约写入组件）；
 * - 「引导只出现一次、可跳过、可找回」：首次提示走 first-run.ts 状态。
 */
import { Card, Notice, PButton, Row } from "./ui";
import { pageMessages } from "./message-catalog";

export const STATE_PAGES = [
  "tokens", "preview", "autodark", "exceptions", "wallpaper", "icons", "pointer", "sound",
  "startmenu", "font", "motion", "archive", "widgets", "lock", "boot", "ime",
  "ctxmenu", "taskbar", "shortcuts", "verdict",
] as const;

export type StatePage = (typeof STATE_PAGES)[number];

function isStatePage(p: string): p is StatePage {
  return (STATE_PAGES as readonly string[]).includes(p);
}

/** 空态块：标题 + 引导 + 入口动作（空态是设计资源不是空白）。 */
export function EmptyStateBlock(props: { page: string; onAction?: () => void; lang?: "zh" | "en" }): React.ReactNode {
  const m = pageMessages(props.page, props.lang ?? "zh");
  if (!m) return <Notice tone="warn">{`页面 ${props.page} 未登记状态目录——这本身是缺陷（十二查 13）`}</Notice>;
  return (
    <Card title={m.empty.title}>
      <Row label={m.empty.guide} sub="">
        <PButton kind="primary" onClick={props.onAction ?? (() => undefined)}>{m.empty.action}</PButton>
      </Row>
    </Card>
  );
}

/** 错误三要素块：发生了什么/为什么/下一步 + 技术详情折叠位。 */
export function ErrorTriadBlock(props: { page: string; technical?: string | null; onRetry?: () => void; lang?: "zh" | "en" }): React.ReactNode {
  const m = pageMessages(props.page, props.lang ?? "zh");
  if (!m) return null;
  return (
    <Notice tone="danger">
      {`${m.error.what}｜${m.error.why}｜${m.error.next}`}
      {props.technical ? <div style={{ fontSize: 10, opacity: 0.6, marginTop: 4 }}>{`详情（默认折叠）：${props.technical}`}</div> : null}
      {props.onRetry ? <div style={{ marginTop: 6 }}><PButton onClick={props.onRetry}>{m.error.next.split("，")[0]}</PButton></div> : null}
    </Notice>
  );
}

/** 确认对话模型：布局契约的数据面（取消固定安全侧——十章词典的机械形态）。 */
export interface ConfirmDialogModel {
  title: string;
  rows: Array<{ label: string; text: string }>;
  /** 按钮序：取消永远在确认左/安全位（交互词典规则 D-CONFIRM-01）。 */
  buttons: Array<{ id: "cancel" | "confirm"; text: string; kind: "ghost" | "danger"; position: "safe-side" | "action-side" }>;
}

export function confirmDialogModel(page: string, lang: "zh" | "en" = "zh"): ConfirmDialogModel | null {
  const m = pageMessages(page, lang);
  if (!m) return null;
  return {
    title: m.confirm.what,
    rows: [
      { label: lang === "zh" ? "后果" : "Consequence", text: m.confirm.consequence },
    ],
    buttons: [
      { id: "cancel", text: m.confirm.cancel, kind: "ghost", position: "safe-side" },
      { id: "confirm", text: m.confirm.confirm, kind: "danger", position: "action-side" },
    ],
  };
}

/** 二十页目录覆盖审计（挂载完整性机检——漏页即红）。 */
export function auditStateCoverage(): { registered: StatePage[]; missingCatalog: string[]; total: number } {
  const missingCatalog = STATE_PAGES.filter((p) => !isStatePage(p) || !pageMessages(p, "zh") || !pageMessages(p, "en"));
  return { registered: [...STATE_PAGES], missingCatalog: [...missingCatalog], total: STATE_PAGES.length };
}
