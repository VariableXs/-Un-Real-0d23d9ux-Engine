/**
 * AI-17 · U-56 错误叙事 2.0（Error Narratives）
 * 错误四段式：发生了什么（人话）/ 影响 / 你现在能做什么 / 技术详情折叠区。
 * 错误词典：后端错误码 → zh/en 叙事映射；未知码走通用模板并标记「未收录」。
 * 重试策略：可重试错误自带退避重试（1s/3s/9s 三次），进度内联上报，不弹新窗。
 */

export interface ErrorNarrative {
  /** 发生了什么（人话） */
  what: string;
  whatEn: string;
  /** 影响 */
  impact: string;
  impactEn: string;
  /** 你现在能做什么 */
  actions: string[];
  actionsEn: string[];
  /** 是否可自动重试 */
  retryable: boolean;
}

export interface NarratedError {
  code: string;
  narrative: ErrorNarrative;
  /** 未收录标记（走通用模板） */
  unknown: boolean;
  /** 技术详情（原始错误串/码/时间，供诊断导出） */
  detail: { raw: string; at: number };
}

interface NarrativeSpec {
  what: string;
  whatEn: string;
  impact: string;
  impactEn: string;
  actions: string[];
  actionsEn: string[];
  retryable: boolean;
}

/** 错误词典：占用/权限/磁盘满/路径超长/网络/缺失/损坏/超时/只读/未授权 */
const DICT: Record<string, NarrativeSpec> = {
  file_in_use: {
    what: "文件正被另一个程序使用",
    whatEn: "The file is in use by another program",
    impact: "本次操作未能完成，文件本身没有变化",
    impactEn: "The operation was not completed; the file is unchanged",
    actions: ["关闭正在使用该文件的程序后重试", "换一个目标路径", "查看详情"],
    actionsEn: ["Close the program using it, then retry", "Choose another destination", "View details"],
    retryable: true,
  },
  access_denied: {
    what: "没有权限访问这个位置",
    whatEn: "You don't have permission to access this location",
    impact: "操作被系统拒绝",
    impactEn: "The operation was blocked by the system",
    actions: ["以管理员身份重试", "换一个你有权限的路径", "查看详情"],
    actionsEn: ["Retry as administrator", "Pick a path you can access", "View details"],
    retryable: true,
  },
  disk_full: {
    what: "目标磁盘空间不足",
    whatEn: "Not enough space on the destination disk",
    impact: "写入中途停止，可能留下部分文件",
    impactEn: "Writing stopped midway; partial files may remain",
    actions: ["清理磁盘后重试", "换一个空间足够的盘", "查看详情"],
    actionsEn: ["Free up space and retry", "Choose another drive", "View details"],
    retryable: true,
  },
  path_too_long: {
    what: "路径太长了",
    whatEn: "The path is too long",
    impact: "Windows 无法创建这个路径",
    impactEn: "Windows cannot create this path",
    actions: ["缩短文件夹层级或文件名", "把目标移到更浅的目录", "查看详情"],
    actionsEn: ["Shorten folder names or depth", "Move target to a shallower folder", "View details"],
    retryable: false,
  },
  network_unreachable: {
    what: "暂时连不上网络",
    whatEn: "The network is unreachable right now",
    impact: "需要联网的操作已暂停",
    impactEn: "Online operations are paused",
    actions: ["检查网络后重试", "稍后再试", "查看详情"],
    actionsEn: ["Check your connection and retry", "Try again later", "View details"],
    retryable: true,
  },
  not_found: {
    what: "找不到这个文件或位置",
    whatEn: "This file or location cannot be found",
    impact: "它可能已被移动、重命名或删除",
    impactEn: "It may have been moved, renamed, or deleted",
    actions: ["刷新后重试", "重新选择目标", "查看详情"],
    actionsEn: ["Refresh and retry", "Re-select the target", "View details"],
    retryable: false,
  },
  corrupted: {
    what: "数据看起来已损坏",
    whatEn: "The data appears to be corrupted",
    impact: "无法安全读取完整内容",
    impactEn: "The full content cannot be read safely",
    actions: ["从备份恢复", "重新下载或重新生成", "查看详情"],
    actionsEn: ["Restore from a backup", "Re-download or regenerate", "View details"],
    retryable: false,
  },
  timeout: {
    what: "操作等待超时",
    whatEn: "The operation timed out",
    impact: "系统在等待响应时放弃了",
    impactEn: "The system gave up waiting for a response",
    actions: ["重试一次", "减小操作规模（如分批复制）", "查看详情"],
    actionsEn: ["Retry once", "Reduce the batch size", "View details"],
    retryable: true,
  },
  read_only: {
    what: "这个位置是只读的",
    whatEn: "This location is read-only",
    impact: "写入被拒绝",
    impactEn: "Writing was refused",
    actions: ["去掉只读属性后重试", "换一个可写的位置", "查看详情"],
    actionsEn: ["Clear read-only and retry", "Choose a writable location", "View details"],
    retryable: false,
  },
  unauthorized: {
    what: "你没有执行这个操作的授权",
    whatEn: "You are not authorized to do this",
    impact: "操作被安全策略拦截",
    impactEn: "Blocked by security policy",
    actions: ["联系管理员授权", "改用允许的方式", "查看详情"],
    actionsEn: ["Ask an administrator for access", "Use an allowed method", "View details"],
    retryable: false,
  },
};

const GENERIC: NarrativeSpec = {
  what: "发生了一个问题",
  whatEn: "Something went wrong",
  impact: "本次操作未能完成",
  impactEn: "The operation was not completed",
  actions: ["重试", "查看详情"],
  actionsEn: ["Retry", "View details"],
  retryable: true,
};

/** 后端错误串 → 词典码（宽松匹配：包含关键词即可命中）。 */
export function classifyError(raw: string): string {
  const s = raw.toLowerCase();
  if (/(in use|being used|占用|使用|access.*denied|拒绝|denied|permission|权限)/.test(s)) {
    return /(denied|permission|权限|拒绝)/.test(s) ? "access_denied" : "file_in_use";
  }
  if (/(disk|space|full|空间|磁盘)/.test(s)) return "disk_full";
  if (/(too long|路径|path.*long|过长)/.test(s)) return "path_too_long";
  if (/(network|unreachable|网络|offline|断网)/.test(s)) return "network_unreachable";
  if (/(not found|no such|cannot find|can't find|找不到|不存在)/.test(s)) return "not_found";
  if (/(corrupt|损坏|损坏的)/.test(s)) return "corrupted";
  if (/(timeout|timed out|超时)/.test(s)) return "timeout";
  if (/(read.?only|只读)/.test(s)) return "read_only";
  if (/(unauthorized|forbidden|未授权|401|403)/.test(s)) return "unauthorized";
  return "unknown";
}

/** 把原始错误串叙述化（lang 决定中文/英文叙事）。 */
export function narrateError(raw: string, lang: "zh" | "en" = "zh"): NarratedError {
  const code = classifyError(raw);
  const spec = DICT[code] ?? GENERIC;
  return {
    code,
    unknown: !(code in DICT),
    narrative: {
      what: lang === "en" ? spec.whatEn : spec.what,
      whatEn: spec.whatEn,
      impact: lang === "en" ? spec.impactEn : spec.impact,
      impactEn: spec.impactEn,
      actions: lang === "en" ? spec.actionsEn : spec.actions,
      actionsEn: spec.actionsEn,
      retryable: spec.retryable,
    },
    detail: { raw, at: Date.now() },
  };
}

/**
 * 退避重试：1s → 3s → 9s 三次（共 4 次尝试）。
 * onProgress(attempt, totalAttempts, nextDelayMs) 内联上报进度。
 * 不可重试错误（isRetryable 返回 false）立即抛出。
 */
export async function backoffRetry<T>(
  fn: (attempt: number) => Promise<T>,
  opts?: {
    delays?: number[];
    shouldRetry?: (err: unknown, attempt: number) => boolean;
    onProgress?: (attempt: number, total: number, nextDelayMs: number) => void;
    sleep?: (ms: number) => Promise<void>;
  },
): Promise<T> {
  const delays = opts?.delays ?? [1000, 3000, 9000];
  const total = delays.length + 1;
  const sleep = opts?.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  let lastErr: unknown;
  for (let attempt = 1; attempt <= total; attempt++) {
    try {
      return await fn(attempt);
    } catch (err) {
      lastErr = err;
      if (attempt === total) break;
      if (opts?.shouldRetry && !opts.shouldRetry(err, attempt)) break;
      const delay = delays[attempt - 1] ?? 1000;
      opts?.onProgress?.(attempt, total, delay);
      await sleep(delay);
    }
  }
  throw lastErr;
}
