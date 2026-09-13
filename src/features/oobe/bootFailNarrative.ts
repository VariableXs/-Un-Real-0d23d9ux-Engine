/**
 * UNREAL-X AI-01 · 族0006 引导失败叙事（X00126~X00150）。
 *
 * 错误码 → 叙事 + 下一步建议映射表：禁裸报错，每一种失败都有
 * 人话标题、成因解释与可执行的下一步。未知错误码回落到通用叙事
 * （同样带下一步建议），绝不返回空/裸消息。
 */

export interface FailNarrative {
  code: string;
  title: string;
  cause: string;
  /** 下一步建议（≥1 条，可执行）。 */
  nextSteps: string[];
  /** 是否建议自动进入恢复环境（联动族0004）。 */
  escalate: boolean;
}

/** 错误码映射表（BC-xxx 引导链错误码体系）。 */
export const FAIL_NARRATIVES: readonly FailNarrative[] = [
  {
    code: "BC-001",
    title: "引导器找不到",
    cause: "启动介质上的引导器文件丢失或被移动。",
    nextSteps: ["打开启动修复工坊，运行「引导器校验」", "若校验失败，运行「启动配置重建」"],
    escalate: false,
  },
  {
    code: "BC-002",
    title: "启动盘响应超时",
    cause: "启动设备在预算时间内没有应答，可能是接触不良或盘体老化。",
    nextSteps: ["重新插拔启动介质后再试", "在恢复环境中查看盘体健康度"],
    escalate: false,
  },
  {
    code: "BC-003",
    title: "内核摘要不符",
    cause: "内核文件与登记的摘要不一致，可能是更新中断。",
    nextSteps: ["在启动修复工坊运行「快照回滚」回到上一个可用内核", "不要反复重启，避免覆盖回滚点"],
    escalate: true,
  },
  {
    code: "BC-004",
    title: "安全启动拦截",
    cause: "信任链中有环节签名未通过，被当前安全启动档位拦截。",
    nextSteps: ["在安全启动仪式面板查看信任链哪一环断裂", "确认该环节为受信版本后，可临时切到审计档重启验证"],
    escalate: true,
  },
  {
    code: "BC-005",
    title: "恢复内存盘损坏",
    cause: "恢复环境所用的内存盘镜像读取失败。",
    nextSteps: ["在恢复环境重生面板执行「重建快照」", "重建失败时使用出厂净身（会保留个人数据档）"],
    escalate: true,
  },
  {
    code: "BC-501",
    title: "修复步骤中断",
    cause: "修复工坊在执行中被打断，进度已记录，可续作。",
    nextSteps: ["回到修复工坊点击「续作」，从断点继续", "若连续两次失败，改用降级（低功耗）模式重试"],
    escalate: false,
  },
];

/** 通用兜底叙事：未知码也绝不裸报错。 */
export const FALLBACK_NARRATIVE: FailNarrative = {
  code: "BC-???",
  title: "启动遇到未知状况",
  cause: "出现了未登记的失败码，已记录现场。",
  nextSteps: ["打开启动日志剧场查看失败前的最后几条日志", "把失败码反馈给启动修复工坊重试一次"],
  escalate: false,
};

export function findNarrative(code: string): FailNarrative {
  const c = code.trim().toUpperCase();
  return FAIL_NARRATIVES.find((n) => n.code === c) ?? { ...FALLBACK_NARRATIVE, code: c || FALLBACK_NARRATIVE.code };
}

/** 叙事渲染文本（UI 单段展示用）。 */
export function narrate(code: string): string {
  const n = findNarrative(code);
  return `${n.code} ${n.title}：${n.cause}下一步——${n.nextSteps.join("；")}。`;
}

/** 是否需要自动转恢复环境。 */
export function shouldEscalate(code: string): boolean {
  return findNarrative(code).escalate;
}

/** 批量叙事：一批错误码 → 逐条渲染文本（剧场字幕）。 */
export function narrateAll(codes: string[]): string[] {
  return codes.map(narrate);
}
