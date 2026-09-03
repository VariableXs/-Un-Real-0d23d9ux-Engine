/**
 * Minimal offline Markdown → HTML converter for opening local .md files as
 * documents. Supports the common subset the editor round-trips: headings,
 * bold/italic/strikethrough/inline code, fenced code blocks, lists, quotes,
 * links, images and horizontal rules. All output goes through the editor's
 * sanitizer before rendering.
 */

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

function inline(md: string): string {
  let s = escapeHtml(md);
  // images before links: ![alt](src)
  s = s.replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, '<img src="$2" alt="$1">');
  // links: [text](url)
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>');
  // inline code first so its content isn't further styled
  s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
  s = s.replace(/\*\*\*([^*]+)\*\*\*/g, "<strong><em>$1</em></strong>");
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  s = s.replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>");
  s = s.replace(/~~([^~]+)~~/g, "<s>$1</s>");
  // autolink bare URLs
  s = s.replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, '$1<a href="$2">$2</a>');
  return s;
}

export function markdownToHtml(mdText: string): string {
  const lines = mdText.replace(/\r\n?/g, "\n").split("\n");
  const out: string[] = [];
  let inCode = false;
  let listType: "ul" | "ol" | null = null;
  let inQuote = false;

  const closeList = (): void => {
    if (listType) { out.push(`</${listType}>`); listType = null; }
  };
  const closeQuote = (): void => {
    if (inQuote) { out.push("</blockquote>"); inQuote = false; }
  };

  for (const raw of lines) {
    const line = raw.trimEnd();

    if (/^```/.test(line.trim())) {
      closeList(); closeQuote();
      out.push(inCode ? "</pre>" : "<pre><code>");
      inCode = !inCode;
      continue;
    }
    if (inCode) { out.push(escapeHtml(raw)); continue; }

    if (line.trim() === "") { closeList(); closeQuote(); continue; }

    const h = line.match(/^(#{1,3})\s+(.*)$/);
    if (h) {
      closeList(); closeQuote();
      const lvl = h[1]!.length;
      out.push(`<h${lvl}>${inline(h[2]!)}</h${lvl}>`);
      continue;
    }

    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      closeList(); closeQuote();
      out.push("<hr>");
      continue;
    }

    const q = line.match(/^>\s?(.*)$/);
    if (q) {
      closeList();
      if (!inQuote) { out.push("<blockquote>"); inQuote = true; }
      out.push(`<p>${inline(q[1]!)}</p>`);
      continue;
    }
    closeQuote();

    const ul = line.match(/^\s*[-*+]\s+(.*)$/);
    if (ul) {
      if (listType !== "ul") { closeList(); out.push("<ul>"); listType = "ul"; }
      out.push(`<li>${inline(ul[1]!)}</li>`);
      continue;
    }
    const ol = line.match(/^\s*\d+[.)]\s+(.*)$/);
    if (ol) {
      if (listType !== "ol") { closeList(); out.push("<ol>"); listType = "ol"; }
      out.push(`<li>${inline(ol[1]!)}</li>`);
      continue;
    }

    closeList();
    out.push(`<p>${inline(line)}</p>`);
  }
  if (inCode) out.push("</pre>");
  closeList(); closeQuote();
  return out.join("\n");
}
