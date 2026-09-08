/** AI-10 DataVault 面板共享的纯格式化函数（无副作用，可单测）。 */

export function fmtSize(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0 B";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function fmtTime(ms: number): string {
  if (!ms) return "—";
  const d = new Date(ms);
  const p = (x: number): string => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function fmtDur(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60);
  return `${h}h${min % 60}m`;
}

/** 拖拽/输入框常见分隔（换行 / 分号 / 逗号）→ 路径数组；去空白、去重。 */
export function splitPaths(raw: string): string[] {
  return [...new Set(
    raw
      .split(/[\n;]+/)
      .map((s) => s.trim())
      .filter(Boolean),
  )];
}
