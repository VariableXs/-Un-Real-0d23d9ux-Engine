/**
 * HTML sanitizer for pasted/imported content.
 * Allowlist-based; strips scripts, remote resources, event handlers and
 * dangerous URLs so only safe local content reaches the DOM.
 */
const ALLOWED_TAGS: ReadonlySet<string> = new Set([
  "p", "br", "hr", "h1", "h2", "h3", "strong", "b", "em", "i", "u", "s", "del",
  "ul", "ol", "li", "blockquote", "code", "pre", "span", "img", "video",
]);

const VOID_TAGS: ReadonlySet<string> = new Set(["br", "hr", "img"]);

const DANGEROUS_CONTAINERS: ReadonlySet<string> = new Set([
  "script", "style", "iframe", "object", "embed", "noscript", "template", "svg", "math",
]);

const ALLOWED_ATTRS = new Map<string, ReadonlySet<string>>([
  ["img", new Set(["src", "alt"])],
  ["video", new Set(["src", "controls"])],
]);

export function isSafeMediaSrc(src: string): boolean {
  const s = src.trim().toLowerCase();
  if (s.startsWith("data:image/")) return true;
  if (s.startsWith("asset://") || s.startsWith("http://asset.localhost/") || s.startsWith("https://asset.localhost/")) return true;
  if (/^[a-z]:[\\/]/.test(s)) return true; // absolute local windows path
  if (s.startsWith("/")) return true; // unix-style local abs path
  return false;
}

export function sanitizeHtml(input: string): string {
  let out = "";
  let i = 0;
  const len = input.length;
  // When inside a dropped container (script/style/…), skip raw content.
  let skipUntil: string | null = null;
  while (i < len) {
    if (skipUntil !== null) {
      const closeIdx = input.toLowerCase().indexOf(`</${skipUntil}`, i);
      if (closeIdx === -1) break;
      const afterClose = input.indexOf(">", closeIdx);
      if (afterClose === -1) break;
      i = afterClose + 1;
      skipUntil = null;
      continue;
    }
    const lt = input.indexOf("<", i);
    if (lt === -1) {
      out += input.slice(i);
      break;
    }
    out += input.slice(i, lt);
    const gt = input.indexOf(">", lt);
    if (gt === -1) break;
    const rawTag = input.slice(lt + 1, gt);
    i = gt + 1;

    const closing = rawTag.startsWith("/");
    const nameMatch = rawTag.match(/^\/?\s*([a-zA-Z0-9]+)/);
    const name = nameMatch?.[1]?.toLowerCase() ?? "";
    if (!name) continue;
    if (closing) {
      if (!VOID_TAGS.has(name) && ALLOWED_TAGS.has(name)) out += `</${name}>`;
      continue;
    }
    if (!ALLOWED_TAGS.has(name)) {
      // Drop dangerous containers WITH their content; void-ish unknowns are just removed.
      if (DANGEROUS_CONTAINERS.has(name)) skipUntil = name;
      continue;
    }
    const attrStr = rawTag.slice(name.length);
    const kept: string[] = [];
    const attrRe = /([a-zA-Z-]+)\s*=\s*"([^"]*)"|([a-zA-Z-]+)\s*=\s*'([^']*)'/g;
    let m: RegExpExecArray | null;
    while ((m = attrRe.exec(attrStr)) !== null) {
      const an = (m[1] ?? m[3] ?? "").toLowerCase();
      const av = m[2] ?? m[4] ?? "";
      if (an.startsWith("on")) continue;
      if (an === "style" && (name === "span" || name === "p" || name === "video" || name === "img")) {
        const safeStyle = sanitizeStyle(av);
        if (safeStyle) kept.push(`style="${safeStyle}"`);
        continue;
      }
      const allowed = ALLOWED_ATTRS.get(name);
      if (allowed?.has(an)) {
        if (an === "src") {
          if (!isSafeMediaSrc(av)) continue;
          kept.push(`${an}="${escapeAttr(av)}"`);
        } else {
          kept.push(`${an}="${escapeAttr(av)}"`);
        }
      }
    }
    const attrs = kept.length > 0 ? ` ${kept.join(" ")}` : "";
    out += VOID_TAGS.has(name) ? `<${name}${attrs}>` : `<${name}${attrs}>`;
  }
  return out;
}

function sanitizeStyle(style: string): string {
  const parts = style.split(";").map((p) => p.trim()).filter(Boolean);
  const ok: string[] = [];
  for (const p of parts) {
    const idx = p.indexOf(":");
    if (idx <= 0) continue;
    const propL = p.slice(0, idx).trim().toLowerCase();
    const val = p.slice(idx + 1).trim();
    if (["color", "font-size", "text-align"].includes(propL) && !/url|expression|import/i.test(val)) {
      ok.push(`${propL}: ${val}`);
    }
  }
  return ok.join("; ");
}

function escapeAttr(s: string): string {
  return s.replace(/"/g, "&quot;").replace(/</g, "&lt;");
}
