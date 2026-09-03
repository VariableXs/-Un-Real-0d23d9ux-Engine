/**
 * Markdown-lite converter for MINDMAP NODE paste (distinct from the document
 * editor's markdown.ts): converts pasted plain-text Markdown into the node
 * content model — ``` fences become embedded .mm-code segments (with language
 * for highlighting), headings clamp to h1..h4, links render as underlined
 * text (hrefs are never persisted inside nodes).
 *
 * Everything is escaped first; the output only ever contains tags the node
 * sanitizer allows, so it can pass straight through it.
 */

const ESC: Record<string, string> = {
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;",
};

function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ESC[c] ?? c);
}

/** Heuristic: does pasted plain text carry Markdown structure worth keeping? */
export function looksLikeMarkdown(text: string): boolean {
  if (text.includes("```")) return true;
  return /^#{1,6}\s+\S/m.test(text)
    || /\*\*[^*\n]+\*\*/.test(text)
    || /(?<![*\w])\*(?!\s)[^*\n]+?(?<!\s)\*(?!\*)/.test(text)
    || /~~[^~\n]+~~/.test(text)
    || /`[^`\n]+`/.test(text)
    || /^[-*+]\s+\S/m.test(text)
    || /^\d+\.\s+\S/m.test(text)
    || /^>\s?\S/m.test(text)
    || /^(?:---|\*\*\*|___)\s*$/m.test(text);
}

/** Inline spans: `code`, **bold**, *italic*, ~~strike~~, [text](url). */
function inlineMd(src: string): string {
  let out = "";
  let rest = src;
  const patterns: Array<[RegExp, (m: RegExpMatchArray) => string]> = [
    [/^`([^`]+)`/, (m) => `<code>${m[1]}</code>`],
    [/^\*\*([^*]+)\*\*/, (m) => `<strong>${m[1]}</strong>`],
    [/^__(.+?)__/, (m) => `<strong>${m[1]}</strong>`],
    [/^~~([^~]+)~~/, (m) => `<s>${m[1]}</s>`],
    [/^\*([^*\n]+)\*/, (m) => `<em>${m[1]}</em>`],
    [/^_([^_\n]+)_/, (m) => `<em>${m[1]}</em>`],
    [/^\[([^\]]+)\]\([^)\s]+\)/, (m) => `<u>${m[1]}</u>`],
  ];
  while (rest.length > 0) {
    if (rest.startsWith("\\") && rest.length > 1) {
      out += esc(rest[1]!);
      rest = rest.slice(2);
      continue;
    }
    let matched = false;
    for (const [re, fn] of patterns) {
      const m = rest.match(re);
      if (m) {
        out += fn(m);
        rest = rest.slice(m[0].length);
        matched = true;
        break;
      }
    }
    if (!matched) {
      out += esc(rest[0]!);
      rest = rest.slice(1);
    }
  }
  return out;
}

function listBlocks(lines: string[], ordered: boolean): string {
  const items: string[] = [];
  let i = 0;
  const re = ordered ? /^\s*\d+\.\s+(.*)$/ : /^\s*[-*+]\s+(.*)$/;
  while (i < lines.length) {
    const m = lines[i]!.match(re);
    if (!m) break;
    items.push(`<li>${inlineMd(m[1]!)}</li>`);
    i++;
  }
  const tag = ordered ? "ol" : "ul";
  return `<${tag}>${items.join("")}</${tag}>`;
}

/** Convert Markdown text to a safe HTML fragment for node content. */
export function mdToNodeHtml(src: string): string {
  const out: string[] = [];
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  let para: string[] = [];
  const flushPara = (): void => {
    if (para.length > 0) {
      out.push(`<div>${para.map(inlineMd).join("<br>")}</div>`);
      para = [];
    }
  };
  while (i < lines.length) {
    const line = lines[i]!;
    // fenced code block → embedded code segment
    const fence = line.match(/^```\s*([A-Za-z0-9+#._-]*)\s*$/);
    if (fence) {
      flushPara();
      const lang = fence[1] ?? "";
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i]!)) {
        body.push(lines[i]!);
        i++;
      }
      i++; // skip closing fence (or EOF)
      out.push(`<pre class="mm-code" data-lang="${esc(lang)}">${esc(body.join("\n"))}</pre>`);
      continue;
    }
    // heading (nodes are compact → clamp to h1..h4)
    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      flushPara();
      const level = Math.min(h[1]!.length, 4);
      out.push(`<h${level}>${inlineMd(h[2]!.trim())}</h${level}>`);
      i++;
      continue;
    }
    // horizontal rule
    if (/^(?:---|\*\*\*|___)\s*$/.test(line)) {
      flushPara();
      out.push("<hr>");
      i++;
      continue;
    }
    // blockquote (consecutive lines merged)
    if (/^>\s?/.test(line)) {
      flushPara();
      const body: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i]!)) {
        body.push(lines[i]!.replace(/^>\s?/, ""));
        i++;
      }
      out.push(`<blockquote>${inlineMd(body.join(" "))}</blockquote>`);
      continue;
    }
    // lists
    if (/^\s*[-*+]\s+\S/.test(line)) {
      flushPara();
      const body: string[] = [];
      while (i < lines.length && /^\s*[-*+]\s+/.test(lines[i]!)) {
        body.push(lines[i]!);
        i++;
      }
      out.push(listBlocks(body, false));
      continue;
    }
    if (/^\s*\d+\.\s+\S/.test(line)) {
      flushPara();
      const body: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i]!)) {
        body.push(lines[i]!);
        i++;
      }
      out.push(listBlocks(body, true));
      continue;
    }
    // blank line → paragraph break
    if (line.trim() === "") {
      flushPara();
      i++;
      continue;
    }
    para.push(line);
    i++;
  }
  flushPara();
  return out.join("");
}
