/**
 * AI-07 · V-44 纯文本净化粘贴：
 * - 剥离富文本格式（颜色/字体/链接残留全去），含隐藏字符（零宽、双向标记、
 *   NBSP、BOM、软回车统一规范化）
 * - 让位检测：目标应用自带 Ctrl+Shift+V 时不拦截（内置常见占用表）
 * - 剪贴板源为图片/文件时不介入（如实透传）——由调用方判定源类型后调用
 * 红线：不做自定义粘贴模板；不改写剪贴板历史。
 */

/** 已知自带「无格式粘贴」的应用（进程名小写）——让位透传。 */
export const NATIVE_PURE_PASTE_APPS: ReadonlySet<string> = new Set([
  "code", // VS Code
  "devenv", // Visual Studio
  "chrome",
  "msedge",
  "firefox",
  "notepad++",
  "idea64", // JetBrains
  "webstorm64",
  "pycharm64",
  "sublime_text",
  "typora",
  "obsidian",
]);

/** 目标应用是否让位（自带 Ctrl+Shift+V）。 */
export function shouldYieldToApp(processName: string): boolean {
  return NATIVE_PURE_PASTE_APPS.has(processName.toLowerCase().replace(/\.exe$/, ""));
}

/** 隐藏/格式残留字符规范化：零宽、双向控制、NBSP、BOM、行分隔符。 */
export function normalizeHiddenChars(text: string): string {
  return text
    .replace(/[\u200B\u200C\u200D\uFEFF\u2060]/g, "") // 零宽类 + BOM + 词连接
    .replace(/[\u202A-\u202E\u2066-\u2069]/g, "") // 双向控制（RTL 覆写攻击面）
    .replace(/\u00A0/g, " ") // NBSP → 空格
    .replace(/\u2028\u2029/g, "\n") // 行/段分隔符 → 换行
    .replace(/\r\n?/g, "\n") // CRLF/CR 统一
    .replace(/[\t\x0B\f]/g, "  ") // 制表类 → 两空格（保守，去控制字符）
    .replace(/[\x00-\x08\x0E-\x1F\x7F]/g, ""); // 其余 C0 控制字符（保留 \n）
}

/** HTML → 纯文本（块级元素换行、<br> 换行、实体解码、去标签）。 */
export function htmlToPlainText(html: string): string {
  let s = html;
  // 块级结束标签 → 换行
  s = s.replace(/<\/(p|div|li|tr|h[1-6]|blockquote|pre)>/gi, "\n");
  s = s.replace(/<br\s*\/?>/gi, "\n");
  s = s.replace(/<li[^>]*>/gi, "• ");
  // 去其余标签（script/style 内容整体移除）
  s = s.replace(/<(script|style)[\s\S]*?<\/\1>/gi, "");
  s = s.replace(/<[^>]+>/g, "");
  // 常见 HTML 实体
  const entities: Record<string, string> = {
    amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'",
    nbsp: " ", mdash: "—", ndash: "–", hellip: "…", copy: "©",
  };
  s = s.replace(/&(amp|lt|gt|quot|#39|apos|nbsp|mdash|ndash|hellip|copy);/g, (m) => entities[m.slice(1, -1)] ?? m);
  s = s.replace(/&#(\d+);/g, (_, d: string) => {
    const code = Number(d);
    return code > 0 && code < 0x10ffff ? String.fromCodePoint(code) : "";
  });
  return s;
}

/** 净化粘贴主入口：任意源（HTML 片段或纯文本）→ 干净纯文本。 */
export function scrubToPlainText(source: string): string {
  const looksHtml = /<\/?[a-z][\s\S]*>/i.test(source);
  const raw = looksHtml ? htmlToPlainText(source) : source;
  return normalizeHiddenChars(raw)
    .split("\n")
    .map((l) => l.replace(/[ \t]+$/g, "")) // 行尾空白
    .join("\n")
    .replace(/\n{3,}/g, "\n\n") // 三连以上空行压缩
    .trim();
}

/** 剪贴板源类型（图片/文件不介入，如实透传）。 */
export type ClipboardSourceKind = "text" | "html" | "image" | "file";

/** V-44 是否介入该源（仅文本/HTML）。 */
export function shouldScrub(kind: ClipboardSourceKind): boolean {
  return kind === "text" || kind === "html";
}
