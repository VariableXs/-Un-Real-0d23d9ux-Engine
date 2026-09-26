/**
 * 通用十二查 8/9 · 二十页状态挂载表（state-blocks → 页面渲染的布线真源）。
 *
 * 主册判据延伸：
 * - 「空态是设计资源」「错误三要素」最后一步：**每页在什么条件下显示什么
 *   状态块**——布线表是数据（不是散在各页的 if）——可审计可机检；
 * - 挂载机检：二十页必须每页有空态触发条件，漏页 = 布线缺陷。
 */

import { STATE_PAGES, type StatePage } from "./state-blocks";

export interface StateWiring {
  page: StatePage;
  /** 空态触发条件（页面数据为空的判定名——与各引擎的空判据对拍）。 */
  emptyWhen: string;
  /** 错误态触发条件（保存失败/加载失败的判定名）。 */
  errorWhen: string;
  /** 确认对话挂载点（破坏性操作的触发动作名）。 */
  confirmTriggers: string[];
}

/** 二十页布线表（一处一事实——页面层按此表渲染，词典按此表机检）。 */
export const STATE_WIRING: StateWiring[] = [
  { page: "tokens", emptyWhen: "tokens=默认表且无历史", errorWhen: "令牌写入失败", confirmTriggers: ["恢复出厂配色"] },
  { page: "preview", emptyWhen: "差异会话未启动", errorWhen: "预览渲染失败", confirmTriggers: ["放弃未应用更改"] },
  { page: "autodark", emptyWhen: "自动切换未启用", errorWhen: "日落时刻获取失败", confirmTriggers: ["今晚跳过切换"] },
  { page: "exceptions", emptyWhen: "例外列表为空", errorWhen: "例外添加失败", confirmTriggers: ["移除例外"] },
  { page: "wallpaper", emptyWhen: "图片池为空", errorWhen: "预载失败", confirmTriggers: ["清空图片池"] },
  { page: "icons", emptyWhen: "无第三方图标包", errorWhen: "热更换失败", confirmTriggers: ["卸载并还原"] },
  { page: "pointer", emptyWhen: "全部指针为默认", errorWhen: "方案保存失败", confirmTriggers: ["还原默认指针"] },
  { page: "sound", emptyWhen: "全部音量未调节", errorWhen: "试听播放失败", confirmTriggers: ["全部静音"] },
  { page: "startmenu", emptyWhen: "无自定义预设", errorWhen: "预设切换失败", confirmTriggers: ["删除预设"] },
  { page: "font", emptyWhen: "未选择待检字体", errorWhen: "字体扫描超时", confirmTriggers: ["强行应用缺字字体"] },
  { page: "motion", emptyWhen: "动效强度未调节", errorWhen: "帧时采样失败", confirmTriggers: ["关闭动效"] },
  { page: "archive", emptyWhen: "从未导出档案", errorWhen: "档案导入失败", confirmTriggers: ["导入并覆盖"] },
  { page: "widgets", emptyWhen: "桌面无组件", errorWhen: "组件数据更新失败", confirmTriggers: ["移除组件"] },
  { page: "lock", emptyWhen: "锁屏为默认样式", errorWhen: "锁屏预览失败", confirmTriggers: ["只保留计数"] },
  { page: "boot", emptyWhen: "开机动画为出厂", errorWhen: "烘帧预算超限", confirmTriggers: ["还原出厂方案"] },
  { page: "ime", emptyWhen: "输入法皮肤未定制", errorWhen: "皮肤包校验失败", confirmTriggers: ["重置皮肤"] },
  { page: "ctxmenu", emptyWhen: "右键菜单为默认结构", errorWhen: "隐藏锁定项被拒", confirmTriggers: ["恢复默认排序"] },
  { page: "taskbar", emptyWhen: "任务栏为默认配置", errorWhen: "几何计算失败", confirmTriggers: ["开启自动隐藏"] },
  { page: "shortcuts", emptyWhen: "无自定义快捷键", errorWhen: "重录冲突", confirmTriggers: ["恢复全部默认"] },
  { page: "verdict", emptyWhen: "域总检未执行", errorWhen: "域总检中断", confirmTriggers: ["导出证据包"] },
];

/** 布线机检：二十页全覆盖、空/错条件非空、确认触发至少一条（破坏性操作都有确认）。 */
export function auditStateWiring(): { ok: boolean; issues: string[]; wired: number } {
  const issues: string[] = [];
  const wiredPages = new Set(STATE_WIRING.map((w) => w.page));
  for (const p of STATE_PAGES) {
    if (!wiredPages.has(p)) issues.push(`${p}: 未布线状态块`);
  }
  for (const w of STATE_WIRING) {
    if (!w.emptyWhen.trim()) issues.push(`${w.page}: emptyWhen 为空`);
    if (!w.errorWhen.trim()) issues.push(`${w.page}: errorWhen 为空`);
    if (w.confirmTriggers.length === 0) issues.push(`${w.page}: 无确认触发点（破坏性操作没有确认=红线）`);
  }
  return { ok: issues.length === 0, issues, wired: STATE_WIRING.length };
}

/** 页面状态解析（页面层消费的入口：给当前判定结果 → 该显示哪个块）。 */
export type PagePhase = "normal" | "empty" | "error";

export function resolvePhase(_w: StateWiring, conditions: { isEmpty: boolean; isError: boolean }): PagePhase {
  if (conditions.isError) return "error";
  if (conditions.isEmpty) return "empty";
  return "normal";
}
