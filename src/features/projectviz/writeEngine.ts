/**
 * 第三章 · 续写写入引擎的本地文件安全层。
 *  - 3.3 备份机制：写入前把原文件存到 <同目录>/.backups/<名字>.<时间戳>.bak
 *  - 4.6 撤销：会话内编辑历史栈，Ctrl+Z 回滚上一次写入（持久化历史为路线图）
 *  - 3.2.1 第一层语法守卫：括号/引号配平检查（词法降噪后统计，复用 parsers 的降噪器）
 * 实际文件读写走 ipc.saveTextFile / ipc.readTextFile，全程本地。
 */

import type { LangId } from "./types";
import { stripNoise, noiseSpec } from "./parsers";

export interface EditRecord {
  absPath: string;
  relPath: string;
  before: string;
  after: string;
  backupPath: string;
  at: number;
  /** 用户原话（大白话意图）。 */
  utterance: string;
}

/** 3.3 备份路径：<目录>/.backups/<名字>.YYYYMMDD-HHMMSS.mmm.bak */
export function backupPath(absPath: string, now = Date.now()): string {
  const sep = absPath.includes("\\") ? "\\" : "/";
  const idx = absPath.lastIndexOf(sep);
  const dir = idx > 0 ? absPath.slice(0, idx) : ".";
  const name = idx > 0 ? absPath.slice(idx + 1) : absPath;
  const d = new Date(now);
  const p2 = (n: number): string => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}${p2(d.getMonth() + 1)}${p2(d.getDate())}-${p2(d.getHours())}${p2(d.getMinutes())}${p2(d.getSeconds())}-${String(d.getMilliseconds()).padStart(3, "0")}`;
  return `${dir}${sep}.backups${sep}${name}.${stamp}.bak`;
}

/**
 * 3.2.1 第一层语法守卫：降噪（去注释/字符串）后检查三类括号是否配平。
 * 不试图替代真 AST —— 括号不配平直接拒绝写入，并给出人话原因。
 */
export function checkBalanced(content: string, lang: LangId): { ok: boolean; detail: string } {
  const { bare } = stripNoise(content, noiseSpec(lang));
  const stack: string[] = [];
  const pairs: Record<string, string> = { ")": "(", "]": "[", "}": "{" };
  for (const ch of bare) {
    if (ch === "(" || ch === "[" || ch === "{") stack.push(ch);
    else if (ch === ")" || ch === "]" || ch === "}") {
      const open = stack.pop();
      if (open !== pairs[ch]) {
        return { ok: false, detail: "括号不成对" };
      }
    }
  }
  if (stack.length > 0) {
    return { ok: false, detail: `有 ${stack.length} 个括号没有闭合` };
  }
  return { ok: true, detail: "" };
}

/** 4.6 会话内撤销栈（线程无关，UI 持有）。 */
export class EditHistory {
  private stack: EditRecord[] = [];

  push(rec: EditRecord): void {
    this.stack.push(rec);
    if (this.stack.length > 50) this.stack.shift();
  }

  /** 弹出上一条记录（用于 Ctrl+Z 回滚真实文件内容）。 */
  pop(): EditRecord | null {
    return this.stack.pop() ?? null;
  }

  /** 该文件最近一次被这样改动过（7.1 编辑历史，会话内视图）。 */
  tailFor(absPath: string): EditRecord | null {
    for (let i = this.stack.length - 1; i >= 0; i--) {
      const r = this.stack[i]!;
      if (r.absPath === absPath) return r;
    }
    return null;
  }

  get size(): number {
    return this.stack.length;
  }
}
