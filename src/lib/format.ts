export function stripHtmlToText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.querySelectorAll("script,style").forEach((n) => n.remove());
  return (doc.body.textContent ?? "").replace(/\u00a0/g, " ");
}

/** CJK chars count individually; latin words as words. */
export function countWords(text: string): number {
  if (!text.trim()) return 0;
  let cjk = 0;
  for (const ch of text) {
    const c = ch.codePointAt(0) ?? 0;
    if (
      (c >= 0x4e00 && c <= 0x9fff) ||
      (c >= 0x3400 && c <= 0x4dbf) ||
      (c >= 0xf900 && c <= 0xfaff) ||
      c >= 0x20000
    ) {
      cjk++;
    }
  }
  const latin = text
    .replace(/[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff\u{20000}-\u{2ffff}]/gu, " ")
    .split(/\s+/)
    .filter((w) => /[a-zA-Z0-9]/.test(w)).length;
  return cjk + latin;
}

export function readingMinutes(words: number): number {
  return Math.max(1, Math.round(words / 220));
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function uid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}

export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}
