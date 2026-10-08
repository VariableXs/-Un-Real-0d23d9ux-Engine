/**
 * AI-07 · V-46 全局速记：
 * - ctrl+alt+n 呼出速记浮窗 → 输入 → Esc 即落盘（当日笔记文件 速记/YYYY-MM-DD.md）
 * - 文件格式 Markdown、时间戳分节；每次 Esc 落盘（断电不丢）
 * - 与 V-47 隐私联动：速记可在设置中完全关闭（enabled=false 时热键不注册）
 * 红线：不做富文本、不做搜索、不做自动整理。
 */

/** 当日速记文件名（相对速记根目录）：速记/2026-09-08.md。 */
export function quickNoteFilename(d: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `速记/${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}.md`;
}

/** 速记条目 → Markdown 分节（时间戳标题 + 原文，多行缩进保持）。 */
export function quickNoteEntry(text: string, at: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  const ts = `${p(at.getHours())}:${p(at.getMinutes())}:${p(at.getSeconds())}`;
  const body = text.trim();
  return `\n## ${ts}\n\n${body}\n`;
}

/** 追加写入（已有文件则拼接；原子性由容器写管线保证）。 */
export function appendQuickNote(existing: string, text: string, at: Date = new Date()): string {
  const entry = quickNoteEntry(text, at);
  if (!existing.trim()) {
    return `# 速记 ${quickNoteFilename(at).slice(3)}\n${entry}`;
  }
  return existing + entry;
}
