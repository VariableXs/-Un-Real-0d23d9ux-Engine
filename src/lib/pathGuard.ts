/**
 * Front-end path guards. The backend re-validates everything; these checks give
 * instant feedback and prevent obvious traversal / bad-name submissions.
 */
const ILLEGAL_NAME_CHARS = /[\\/:*?"<>|\0]/;

export function isPlainFileName(name: string): boolean {
  return name.length > 0 && name.length <= 180 && !ILLEGAL_NAME_CHARS.test(name) && name !== "." && name !== "..";
}

export interface FolderNameCheck {
  ok: boolean;
  reason?: "empty" | "too-long" | "illegal";
}

export function validateItemName(raw: string): FolderNameCheck {
  const name = raw.trim();
  if (!name) return { ok: false, reason: "empty" };
  if (name.length > 120) return { ok: false, reason: "too-long" };
  if (/[\\/]/.test(name) || name.includes("\0")) return { ok: false, reason: "illegal" };
  return { ok: true };
}
