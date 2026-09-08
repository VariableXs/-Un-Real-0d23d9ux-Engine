/**
 * AI-07 · V-48 运行框自动补全：
 * - 补全源：历史命令 + 可执行名（PATH + App Paths，启动后台扫描缓存）+ shell: 系统目录
 * - 前缀匹配（不做模糊——与 V-39 职责分开）；↑↓ 选择、Tab/→ 确认（UI 层接键）
 * - 分类图标区分（history/program/shell）；补全可整体关闭
 * 红线：不做自定义别名（宏引擎领地）。
 */

export type CompletionKind = "history" | "program" | "shell";

export interface CompletionItem {
  /** 补全文本（完整输入值）。 */
  value: string;
  kind: CompletionKind;
  /** 显示用标签（如程序友好名）。 */
  label: string;
}

export interface CompletionSources {
  history: string[];
  /** 可执行名（不含 .exe 后缀亦可；前缀匹配）。 */
  programs: string[];
  /** shell: 系统目录名（startup / windows / downloads…）。 */
  shellDirs: string[];
}

/** 内置 shell: 目录表（Windows 常用，本地常量）。 */
export const SHELL_DIRS: string[] = [
  "startup", "common startup", "programs", "common programs", "appdata",
  "local appdata", "windows", "system", "system32", "downloads", "desktop",
  "documents", "pictures", "music", "videos", "fonts", "printers", "recyclebinfolder",
];

export function defaultSources(history: string[]): CompletionSources {
  return { history, programs: [], shellDirs: SHELL_DIRS };
}

/** 前缀补全查询（大小写不敏感；历史 > 程序 > shell 排序，各按字典序）。 */
export function complete(input: string, src: CompletionSources, limit = 8): CompletionItem[] {
  const q = input.trim().toLowerCase();
  if (!q) return [];
  const out: CompletionItem[] = [];
  if (src.history.length > 0) {
    for (const h of src.history) {
      if (h.toLowerCase().startsWith(q)) out.push({ value: h, kind: "history", label: h });
    }
  }
  for (const p of src.programs) {
    if (p.toLowerCase().startsWith(q)) out.push({ value: p, kind: "program", label: p });
  }
  // shell: 前缀（输入 "shell:" 或 "shell:s" 均可）
  const shellPrefix = q.startsWith("shell:") ? q.slice(6) : q === "shell" ? "" : null;
  if (shellPrefix !== null) {
    for (const d of src.shellDirs) {
      if (d.startsWith(shellPrefix)) out.push({ value: `shell:${d}`, kind: "shell", label: d });
    }
  }
  // 去重 + 截断
  const seen = new Set<string>();
  return out
    .filter((x) => {
      const k = x.kind + ":" + x.value;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .slice(0, limit);
}
