/**
 * AI-07 · V-41 内联计算（搜索框/运行框/命令面板输入算式即时显示结果卡）：
 * - 复用 F-2.1 计算器引擎（src/lib/calc.ts，零依赖本地递归下降求值）
 * - 输入含字母路径/命令/URL 时自动让位（不误判为算式）
 * - Enter 复制结果；方向键可选历史（历史管道由面板侧接 privacy 三态）
 * 红线：不做变量/公式记忆、不做汇率（需网络）。
 */

import { calcEval, fmtResult } from "../calc";

export interface InlineCalcResult {
  expr: string;
  value: number;
  /** 展示格式（整数直出 / 12 位有效）。 */
  formatted: string;
}

/** 让位判定：这些输入不是算式（V-41 验收：路径/命令零误判）。 */
export function looksLikeNonMath(input: string): boolean {
  const s = input.trim();
  if (!s) return true;
  // 盘符路径 / UNC / URL / shell: / ms-settings: / 环境变量
  if (/^[a-zA-Z]:[\\/]/.test(s)) return true;
  if (/^\\\\/.test(s)) return true;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) return true;
  if (/^(shell|ms-settings|ms-):/i.test(s)) return true;
  if (/%[A-Za-z_]+%/.test(s)) return true;
  // 单词命令（含字母且不是已知常量/函数的混合式）
  if (/^[a-z]+$/i.test(s) && !/^(pi|e|sqrt|abs|sin|cos|tan|ln|log)$/i.test(s)) return true;
  return false;
}

/**
 * 内联计算入口：是算式则求值返回结果卡数据，否则返回 null（调用方让位给
 * 搜索/命令/路径通道）。
 */
export function inlineCalc(input: string): InlineCalcResult | null {
  if (looksLikeNonMath(input)) return null;
  // 至少含一个数字与一个运算符才算算式意图
  if (!/\d/.test(input) || !/[+\-*/^%!()]|sqrt|abs|sin|cos|tan|ln|log/i.test(input)) return null;
  try {
    const value = calcEval(input);
    if (!Number.isFinite(value) && !Number.isNaN(value)) return null; // ±∞ 如实不出卡
    if (Number.isNaN(value)) return null;
    return { expr: input.trim(), value, formatted: fmtResult(value) };
  } catch {
    return null; // 非法算式如实让位（不弹错）
  }
}
