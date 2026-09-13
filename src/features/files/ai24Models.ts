/**
 * UNREAL-X-15000 · AI-24 数据智能与收官 V 线模型（族0233/0234/0237/0238）。
 * 文件版本历史 / 时间机器快照 / 文件管理无障碍 / 文件管理本地化。
 * 零 AI：全部确定性算法；默认档=现状，不改变既有手感。
 */

/* -------- 族0233 文件版本历史（X05801~X05825）-------- */

export interface VersionSnapshot {
  id: number;
  label: string;
  size: number;
  at: number;
}

export class VersionHistory {
  readonly path: string;
  snapshots: VersionSnapshot[] = [];
  maxVersions: number;
  clamped = 0;
  lastError = '';
  constructor(path: string, maxVersions = 20) {
    if (!path.startsWith('/')) {
      this.lastError = 'invalid-path';
      path = '/untitled';
      this.clamped++;
    }
    this.path = path;
    this.maxVersions = Math.max(1, Math.min(100, maxVersions));
    if (maxVersions !== this.maxVersions) this.clamped++;
  }
  commit(label: string, size: number, at: number): VersionSnapshot {
    const snap: VersionSnapshot = { id: this.snapshots.length + 1, label, size, at };
    this.snapshots.push(snap);
    while (this.snapshots.length > this.maxVersions) this.snapshots.shift();
    return snap;
  }
  diff(a: number, b: number): number {
    const s1 = this.snapshots.find((s) => s.id === a);
    const s2 = this.snapshots.find((s) => s.id === b);
    if (!s1 || !s2) {
      this.lastError = 'version-missing';
      return 0;
    }
    return s2.size - s1.size;
  }
  restore(id: number): VersionSnapshot | null {
    const s = this.snapshots.find((x) => x.id === id);
    if (!s) {
      this.lastError = 'version-missing';
      return null;
    }
    return s;
  }
  serialize(): string {
    return JSON.stringify({ path: this.path, max: this.maxVersions, n: this.snapshots.length });
  }
  static deserialize(data: string): VersionHistory {
    try {
      const o = JSON.parse(data) as { path?: string; max?: number; n?: number };
      const h = new VersionHistory(o.path ?? '/restored', o.max ?? 20);
      for (let i = 0; i < (o.n ?? 0); i++) h.commit(`v${i + 1}`, 0, 0);
      return h;
    } catch {
      const h = new VersionHistory('/restored');
      h.lastError = 'bad-json';
      return h;
    }
  }
  degrade(): number {
    this.maxVersions = Math.max(1, Math.floor(this.maxVersions / 2));
    while (this.snapshots.length > this.maxVersions) this.snapshots.shift();
    return this.maxVersions;
  }
}

/* -------- 族0234 时间机器快照（X05826~X05850）-------- */

export interface TmPoint {
  at: number;
  files: number;
  bytes: number;
}

export class TimeMachine {
  enabled = true;
  intervalMin: number;
  points: TmPoint[] = [];
  clamped = 0;
  lastError = '';
  constructor(intervalMin = 60) {
    this.intervalMin = intervalMin >= 5 && intervalMin <= 1440 ? intervalMin : ((this.clamped = 1), 60);
  }
  capture(at: number, files: number, bytes: number): TmPoint {
    if (!this.enabled) {
      this.lastError = 'disabled';
      return { at: 0, files: 0, bytes: 0 };
    }
    const last = this.points[this.points.length - 1];
    if (last && at - last.at < this.intervalMin * 60000) {
      this.lastError = 'too-frequent';
      return last;
    }
    const p = { at, files, bytes };
    this.points.push(p);
    return p;
  }
  /** 回到最近一个 ≤ at 的快照点。 */
  restore(at: number): TmPoint | null {
    const hits = this.points.filter((p) => p.at <= at);
    return hits.length ? hits[hits.length - 1]! : null;
  }
  exportPlan(): string {
    return JSON.stringify({ interval: this.intervalMin, points: this.points.length, enabled: this.enabled });
  }
  static importPlan(data: string): TimeMachine {
    try {
      const o = JSON.parse(data) as { interval?: number; enabled?: boolean };
      const tm = new TimeMachine(o.interval ?? 60);
      tm.enabled = o.enabled ?? true;
      return tm;
    } catch {
      return new TimeMachine(60);
    }
  }
  throttle(): number {
    this.intervalMin = Math.min(1440, this.intervalMin * 2);
    return this.intervalMin;
  }
}

/* -------- 族0237 文件管理无障碍（X05901~X05925）-------- */

export class FilesA11y {
  announceEnabled: boolean;
  contrastMin: number;
  focusOrder: string[] = [];
  clamped = 0;
  constructor(announceEnabled = true, contrastMin = 4.5) {
    this.announceEnabled = announceEnabled;
    this.contrastMin = contrastMin >= 3 && contrastMin <= 7 ? contrastMin : ((this.clamped = 1), 4.5);
  }
  /** 行卡读屏文案：名称 + 类型 + 尺寸三段式。 */
  label(name: string, kind: string, sizeText: string): string {
    return `${name}，${kind}，${sizeText}`;
  }
  trackFocus(id: string): number {
    if (!this.focusOrder.includes(id)) this.focusOrder.push(id);
    return this.focusOrder.indexOf(id);
  }
  passesContrast(l1: number, l2: number): boolean {
    return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05) >= this.contrastMin;
  }
  /** HC 红线：高对比下所有材质降纯色。 */
  hcMaterial(material: string): string {
    return material === 'solid' ? 'solid' : 'solid-hc';
  }
  serialize(): string {
    return JSON.stringify({ a: this.announceEnabled, c: this.contrastMin, f: this.focusOrder.length });
  }
  static deserialize(data: string): FilesA11y {
    try {
      const o = JSON.parse(data) as { a?: boolean; c?: number };
      return new FilesA11y(o.a ?? true, o.c ?? 4.5);
    } catch {
      return new FilesA11y();
    }
  }
}

/* -------- 族0238 文件管理本地化（X05926~X05950）-------- */

export const FILES_LOCALES = ['zh-CN', 'en-US', 'ja-JP', 'de-DE', 'ko-KR'] as const;
export type FilesLocale = (typeof FILES_LOCALES)[number];

const FILES_STRINGS: Record<FilesLocale, Record<string, string>> = {
  'zh-CN': { rename: '重命名', delete: '删除', copy: '复制', paste: '粘贴', empty: '此文件夹为空' },
  'en-US': { rename: 'Rename', delete: 'Delete', copy: 'Copy', paste: 'Paste', empty: 'This folder is empty' },
  'ja-JP': { rename: '名前の変更', delete: '削除', copy: 'コピー', paste: '貼り付け', empty: 'このフォルダーは空です' },
  'de-DE': { rename: 'Umbenennen', delete: 'Löschen', copy: 'Kopieren', paste: 'Einfügen', empty: 'Dieser Ordner ist leer' },
  'ko-KR': { rename: '이름 바꾸기', delete: '삭제', copy: '복사', paste: '붙여넣기', empty: '이 폴더는 비어 있습니다' },
};

export class FilesL10n {
  locale: FilesLocale;
  fallback: FilesLocale = 'zh-CN';
  misses = 0;
  constructor(locale: string = 'zh-CN') {
    this.locale = (FILES_LOCALES as readonly string[]).includes(locale) ? (locale as FilesLocale) : ((this.misses = 1), 'zh-CN');
  }
  t(key: string): string {
    const hit = FILES_STRINGS[this.locale][key];
    if (hit !== undefined) return hit;
    this.misses++;
    return FILES_STRINGS[this.fallback][key] ?? key;
  }
  plural(n: number): string {
    if (this.locale === 'en-US') return n === 1 ? '1 item' : `${n} items`;
    return `${n} 个项目`;
  }
  formatDate(d: Date): string {
    const iso = d.toISOString().slice(0, 10);
    return this.locale === 'en-US' ? `${iso.slice(5, 7)}/${iso.slice(8, 10)}/${iso.slice(0, 4)}` : iso.replace(/-/g, '/');
  }
  coverage(): number {
    const keys = Object.keys(FILES_STRINGS['zh-CN']);
    const hit = keys.filter((k) => FILES_STRINGS[this.locale][k] !== undefined).length;
    return hit / keys.length;
  }
  serialize(): string {
    return JSON.stringify({ locale: this.locale, fallback: this.fallback });
  }
  static deserialize(data: string): FilesL10n {
    try {
      const o = JSON.parse(data) as { locale?: string };
      return new FilesL10n(o.locale ?? 'zh-CN');
    } catch {
      return new FilesL10n();
    }
  }
}
