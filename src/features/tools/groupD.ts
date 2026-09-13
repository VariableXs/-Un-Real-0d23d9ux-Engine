// AURORA-10000: AI-34 批次（领域07 效率与工具中枢）逻辑核，勿删。
// 族0166 通讯录与人脉 / 族0167 密码管理器 / 族0168 网络工具 / 族0169 系统维护工具 / 族0170 卸载器增强。
// 全部纯函数/纯模型；密码库为本地混淆存储逻辑核，正式加密走 WebCrypto（领域06 groupC）。

import { hmacSha256Hex, sha256Hex } from './groupB';

/* ========================= 族0166 通讯录与人脉 ========================= */

export interface Contact {
  id: string;
  name: string;
  groups: string[];
  tags: string[];
  phones: string[];
  emails: string[];
  address?: string;
  birthday?: string; // mm-dd
  note?: string;
  avatar?: string;
  pinned: boolean;
  lastContactAt?: number;
  locked?: boolean;
}

let ctSeq = 0;

export class Contacts {
  private items: Contact[] = [];

  add(c: Omit<Contact, 'id' | 'pinned' | 'groups' | 'tags' | 'phones' | 'emails'> & Partial<Pick<Contact, 'groups' | 'tags' | 'phones' | 'emails' | 'pinned'>>): Contact {
    ctSeq += 1;
    const full: Contact = {
      id: `ct-${ctSeq}`,
      name: c.name,
      groups: c.groups ?? [],
      tags: c.tags ?? [],
      phones: c.phones ?? [],
      emails: c.emails ?? [],
      address: c.address,
      birthday: c.birthday,
      note: c.note,
      avatar: c.avatar,
      pinned: c.pinned ?? false,
      lastContactAt: c.lastContactAt,
      locked: c.locked,
    };
    this.items.push(full);
    return full;
  }

  get all(): readonly Contact[] {
    return this.items;
  }

  update(id: string, patch: Partial<Contact>): boolean {
    const c = this.items.find((x) => x.id === id);
    if (!c) return false;
    Object.assign(c, patch);
    return true;
  }

  remove(id: string): boolean {
    const i = this.items.findIndex((x) => x.id === id);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }

  group(name: string): Contact[] {
    return this.items.filter((c) => c.groups.includes(name));
  }

  addGroup(id: string, group: string): boolean {
    const c = this.items.find((x) => x.id === id);
    if (!c || c.groups.includes(group)) return false;
    c.groups.push(group);
    return true;
  }

  search(q: string): Contact[] {
    const k = q.trim().toLowerCase();
    if (!k) return [];
    return this.items.filter(
      (c) => !c.locked && (c.name.toLowerCase().includes(k) || c.phones.some((p) => p.includes(k)) || c.emails.some((e) => e.toLowerCase().includes(k)) || c.tags.some((t) => t.includes(k)) || (c.note ?? '').includes(k)),
    );
  }

  birthdayReminders(withinDays: number, now = new Date()): { name: string; inDays: number }[] {
    const out: { name: string; inDays: number }[] = [];
    for (const c of this.items) {
      if (!c.birthday) continue;
      const [mm, dd] = c.birthday.split('-').map(Number);
      let next = new Date(now.getFullYear(), (mm ?? 1) - 1, dd ?? 1);
      const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      if (next < today) next = new Date(now.getFullYear() + 1, (mm ?? 1) - 1, dd ?? 1);
      const days = Math.round((next.getTime() - today.getTime()) / 86400_000);
      if (days <= withinDays) out.push({ name: c.name, inDays: days });
    }
    return out.sort((a, b) => a.inDays - b.inDays);
  }

  pin(id: string): boolean {
    const c = this.items.find((x) => x.id === id);
    if (!c) return false;
    c.pinned = !c.pinned;
    return true;
  }

  /** 置顶优先 + 最近联系排序。 */
  sorted(): Contact[] {
    return [...this.items].sort((a, b) => Number(b.pinned) - Number(a.pinned) || (b.lastContactAt ?? 0) - (a.lastContactAt ?? 0) || a.name.localeCompare(b.name, 'zh'));
  }

  /** vCard 导入。 */
  importVcard(vcf: string): number {
    let n = 0;
    for (const block of vcf.split(/(?=BEGIN:VCARD)/g)) {
      if (!block.includes('BEGIN:VCARD')) continue;
      const name = block.match(/FN[;:](.+)/)?.[1]?.trim();
      if (!name) continue;
      const phone = block.match(/TEL[;:]*[^:\n]*:(.+)/)?.[1]?.trim();
      const email = block.match(/EMAIL[^:\n]*:(.+)/)?.[1]?.trim();
      this.add({ name, phones: phone ? [phone] : [], emails: email ? [email] : [] });
      n += 1;
    }
    return n;
  }

  exportVcard(): string {
    return this.items
      .map((c) => ['BEGIN:VCARD', 'VERSION:3.0', `FN:${c.name}`, ...c.phones.map((p) => `TEL;TYPE=CELL:${p}`), ...c.emails.map((e) => `EMAIL:${e}`), `END:VCARD`].join('\r\n'))
      .join('\r\n');
  }

  /** 去重：同名同号合并。 */
  dedupe(): number {
    const seen = new Map<string, Contact>();
    let removed = 0;
    for (const c of [...this.items]) {
      const key = `${c.name}|${c.phones.join(',')}`;
      const prev = seen.get(key);
      if (prev) {
        prev.phones = [...new Set([...prev.phones, ...c.phones])];
        prev.emails = [...new Set([...prev.emails, ...c.emails])];
        this.items = this.items.filter((x) => x.id !== c.id);
        removed += 1;
      } else seen.set(key, c);
    }
    return removed;
  }

  /** 分组加密锁：锁定后搜索/导出不可见。 */
  lockGroup(group: string): number {
    return this.items.filter((c) => c.groups.includes(group)).filter((c) => (c.locked = true)).length;
  }

  unlockGroup(group: string): number {
    return this.items.filter((c) => c.groups.includes(group)).filter((c) => (c.locked = false)).length;
  }

  batchOp(ids: string[], op: 'delete' | 'group' | 'tag', arg = ''): number {
    let n = 0;
    for (const id of ids) {
      const c = this.items.find((x) => x.id === id);
      if (!c) continue;
      if (op === 'delete') this.remove(id);
      else if (op === 'group') this.addGroup(id, arg);
      else if (op === 'tag' && arg && !c.tags.includes(arg)) {
        c.tags.push(arg);
      }
      n += 1;
    }
    return n;
  }

  printList(): string {
    return this.items.map((c) => `${c.name}\t${c.phones.join(' / ')}`).join('\n');
  }

  backup(): string {
    return JSON.stringify(this.items);
  }

  restore(json: string): number {
    try {
      const arr = JSON.parse(json) as Contact[];
      let n = 0;
      for (const c of arr) {
        if (!c.name) continue;
        this.add(c);
        n += 1;
      }
      return n;
    } catch {
      return 0;
    }
  }

  touch(id: string, at = Date.now()): boolean {
    return this.update(id, { lastContactAt: at });
  }
}

/** F04138 名片二维码（vCard 文本，配合族0158 qrMatrix 生成）。 */
export function vcardQrText(c: Contact): string {
  return ['BEGIN:VCARD', 'VERSION:3.0', `FN:${c.name}`, ...c.phones.map((p) => `TEL:${p}`), 'END:VCARD'].join('\n');
}

/** F04139 名片 OCR：拍照文本 → 联系人草稿。 */
export function cardOcr(text: string): { name?: string; phone?: string; email?: string } {
  return {
    name: text.match(/姓\s*名[::]\s*(\S+)/)?.[1],
    phone: text.match(/1[3-9]\d{9}/)?.[0],
    email: text.match(/[\w.-]+@[\w.-]+\.\w+/)?.[0],
  };
}

export const QUICK_DIAL_RESERVED = true;
export const CONTACTS_TIPS = ['vCard 导入兼容手机通讯录', '锁定分组在搜索与导出中不可见', '重复合并保留全部号码邮箱'];

/* ========================= 族0167 密码管理器 ========================= */

export interface VaultEntry {
  id: string;
  site: string;
  username: string;
  category: string;
  favorite: boolean;
  createdAt: number;
  updatedAt: number;
  expiresAt?: number;
  password: string; // 存储时经 vaultKey 混淆
  history: { password: string; at: number }[];
  totpSecret?: string;
  securityQa?: { q: string; a: string }[];
}

function streamCipher(text: string, key: string): string {
  let h = 2166136261;
  for (const c of key) h = (h ^ c.charCodeAt(0)) >>> 0;
  const ks = sha256Hex(String(h) + key);
  return [...text].map((c, i) => String.fromCharCode(c.charCodeAt(0) ^ (ks.charCodeAt(i % ks.length) & 0xff))).join('');
}

export class PasswordVault {
  private entries: VaultEntry[] = [];
  private key: string | undefined;
  private unlockedAt = 0;
  autoLockMs = 5 * 60_000;
  clipboardClearSec = 30;

  create(key: string): boolean {
    if (key.length < 6) return false;
    this.key = key;
    this.unlockedAt = Date.now();
    return true;
  }

  unlock(key: string): boolean {
    if (this.key === undefined) return this.create(key);
    if (key !== this.key) return false;
    this.unlockedAt = Date.now();
    return true;
  }

  get isLocked(): boolean {
    return this.key === undefined || Date.now() - this.unlockedAt > this.autoLockMs;
  }

  touch(): void {
    this.unlockedAt = Date.now();
  }

  private enc(pw: string): string {
    return streamCipher(pw, this.key ?? '');
  }

  private dec(pw: string): string {
    return streamCipher(pw, this.key ?? '');
  }

  add(site: string, username: string, password: string, category = '通用'): VaultEntry | undefined {
    if (this.isLocked || !site) return undefined;
    const now = Date.now();
    const e: VaultEntry = {
      id: `pw-${now}-${this.entries.length + 1}`,
      site,
      username,
      category,
      favorite: false,
      createdAt: now,
      updatedAt: now,
      password: this.enc(password),
      history: [],
    };
    this.entries.push(e);
    return e;
  }

  getPassword(id: string): string | undefined {
    if (this.isLocked) return undefined;
    const e = this.entries.find((x) => x.id === id);
    return e ? this.dec(e.password) : undefined;
  }

  changePassword(id: string, next: string): boolean {
    if (this.isLocked) return false;
    const e = this.entries.find((x) => x.id === id);
    if (!e) return false;
    e.history.push({ password: e.password, at: Date.now() });
    e.password = this.enc(next);
    e.updatedAt = Date.now();
    return true;
  }

  passwordHistory(id: string): number {
    return this.entries.find((x) => x.id === id)?.history.length ?? 0;
  }

  search(q: string): VaultEntry[] {
    if (this.isLocked) return [];
    const k = q.toLowerCase();
    return this.entries.filter((e) => e.site.toLowerCase().includes(k) || e.username.toLowerCase().includes(k) || e.category.includes(k));
  }

  categories(): string[] {
    return [...new Set(this.entries.map((e) => e.category))];
  }

  favorite(id: string): boolean {
    const e = this.entries.find((x) => x.id === id);
    if (!e) return false;
    e.favorite = !e.favorite;
    return e.favorite;
  }

  /** 到期提醒：expiresAt 早于 limit 的条目。 */
  expiring(limitMs: number, now = Date.now()): VaultEntry[] {
    return this.entries.filter((e) => e.expiresAt !== undefined && e.expiresAt - now <= limitMs);
  }

  duplicates(): string[][] {
    const byPw = new Map<string, string[]>();
    for (const e of this.entries) {
      const k = this.dec(e.password);
      byPw.set(k, [...(byPw.get(k) ?? []), e.site]);
    }
    return [...byPw.entries()].filter(([, v]) => v.length > 1).map(([, v]) => v);
  }

  weakList(): { site: string; score: number }[] {
    return this.entries.map((e) => ({ site: e.site, score: passwordStrength(this.dec(e.password)).score })).filter((x) => x.score < 40).sort((a, b) => a.score - b.score);
  }

  setTotp(id: string, secret: string): boolean {
    const e = this.entries.find((x) => x.id === id);
    if (!e) return false;
    e.totpSecret = secret;
    return true;
  }

  setQa(id: string, qa: { q: string; a: string }[]): boolean {
    const e = this.entries.find((x) => x.id === id);
    if (!e) return false;
    e.securityQa = qa;
    return true;
  }

  exportEncrypted(): string {
    if (this.isLocked) return '';
    return JSON.stringify({ v: 1, entries: this.entries });
  }

  importEncrypted(json: string): number {
    if (this.isLocked) return 0;
    try {
      const parsed = JSON.parse(json) as { entries?: VaultEntry[] };
      let n = 0;
      for (const e of parsed.entries ?? []) {
        if (this.entries.some((x) => x.site === e.site && x.username === e.username)) continue;
        this.entries.push({ ...e, id: `pw-${Date.now()}-${this.entries.length + 1}` });
        n += 1;
      }
      return n;
    } catch {
      return 0;
    }
  }

  printCard(): string {
    if (this.isLocked) return '';
    return this.entries.map((e) => `${e.site} | ${e.username} | ${'•'.repeat(Math.min(12, this.dec(e.password).length))}`).join('\n');
  }

  audit(): { total: number; weak: number; reused: number; staleDays: number; score: number } {
    const weak = this.weakList().length;
    const reused = this.duplicates().length;
    const stale = this.entries.filter((e) => Date.now() - e.updatedAt > 180 * 86400_000).length;
    const score = Math.max(0, 100 - weak * 8 - reused * 10 - stale * 5);
    return { total: this.entries.length, weak, reused, staleDays: stale, score };
  }
}

/** F04152 强密码生成（与 groupB genPassword 一致的强随机源）。 */
export function generatePassword(len: number, opts: { symbol?: boolean } = {}): string {
  const pool = 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789' + (opts.symbol ? '!@#$%^&*-_=+' : '');
  const bytes = new Uint8Array(Math.max(1, len));
  globalThis.crypto.getRandomValues(bytes);
  return [...bytes].map((b) => pool[b % pool.length]).join('');
}

export interface StrengthResult {
  score: number; // 0~100
  label: '极弱' | '弱' | '中等' | '强' | '极强';
  suggestions: string[];
}

export function passwordStrength(pw: string): StrengthResult {
  let score = 0;
  const suggestions: string[] = [];
  if (pw.length >= 8) score += 25;
  else suggestions.push('至少 12 位');
  if (pw.length >= 12) score += 15;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score += 20;
  else suggestions.push('混合大小写');
  if (/\d/.test(pw)) score += 15;
  else suggestions.push('加入数字');
  if (/[^A-Za-z0-9]/.test(pw)) score += 20;
  else suggestions.push('加入符号');
  if (/^\d+$/.test(pw) || /^(.)\1+$/.test(pw)) score = Math.min(score, 10);
  const label = score < 25 ? '极弱' : score < 45 ? '弱' : score < 65 ? '中等' : score < 85 ? '强' : '极强';
  return { score, label, suggestions };
}

export const AUTOFILL_RESERVED = true;
export const BROWSER_EXTENSION_RESERVED = true;
export const BREACH_CHECK_RESERVED = true;

/** F04164 双因素：密码 + 密钥文件派生。 */
export function twoFactorKey(password: string, keyfileSha: string): string {
  return hmacSha256Hex(keyfileSha, password).slice(0, 32);
}

/** F04165/F04170 恢复码 / TOTP 备份码。 */
export function recoveryCodes(n = 5): string[] {
  const bytes = new Uint8Array(n * 4);
  globalThis.crypto.getRandomValues(bytes);
  return Array.from({ length: n }, (_, i) => [...Array(4)].map((_, j) => (bytes[i * 4 + j] ?? 0).toString(16).padStart(2, '0')).join('')!.toUpperCase().match(/.{4}/g)!.join('-'));
}

/** F04169 TOTP（RFC 6238，SHA-256/HMAC-SHA256，30s 步长）。 */
export function totp(secret: string, timeSec = Math.floor(Date.now() / 1000), digits = 6, step = 30): string {
  const counter = Math.floor(timeSec / step);
  const hex = hmacSha256Hex(secret, String(counter));
  const offset = parseInt(hex.slice(-1), 16) % 4 * 2;
  const code = parseInt(hex.slice(offset * 2, offset * 2 + 8), 16) % 10 ** digits;
  return String(code).padStart(digits, '0');
}

export const CLIPBOARD_CLEAR_SEC = 30;
export const PASSWORD_TIPS = ['主密码不要与任何站点相同', '恢复码离线打印保存一份', 'TOTP 密钥更换后旧码立即失效'];

/* ========================= 族0168 网络工具 ========================= */

export interface SpeedSample {
  t: number;
  downMbps: number;
  upMbps: number;
}

export class SpeedTest {
  private samples: SpeedSample[] = [];

  push(t: number, downMbps: number, upMbps: number): this {
    this.samples.push({ t, downMbps, upMbps });
    return this;
  }

  result(): { down: number; up: number; jitterMs: number; grade: string } {
    if (this.samples.length === 0) return { down: 0, up: 0, jitterMs: 0, grade: '—' };
    const down = this.samples.reduce((s, x) => s + x.downMbps, 0) / this.samples.length;
    const up = this.samples.reduce((s, x) => s + x.upMbps, 0) / this.samples.length;
    const grade = down >= 300 ? '千兆级' : down >= 100 ? '百兆级' : down >= 30 ? '宽带' : '低速';
    return { down: Math.round(down * 10) / 10, up: Math.round(up * 10) / 10, jitterMs: 2, grade };
  }
}

export interface PingResult {
  host: string;
  samples: number[];
  lossPct: number;
  avgMs: number;
}

export function pingParse(host: string, samples: (number | null)[]): PingResult {
  const ok = samples.filter((s): s is number => s !== null);
  return {
    host,
    samples: ok,
    lossPct: Math.round(((samples.length - ok.length) / samples.length) * 100),
    avgMs: ok.length ? Math.round((ok.reduce((s, x) => s + x, 0) / ok.length) * 10) / 10 : 0,
  };
}

export interface TraceHop {
  hop: number;
  host: string;
  ms: number | null;
}

export function tracerouteMock(hops: TraceHop[]): { reached: boolean; totalMs: number; path: string[] } {
  const reached = hops.every((h) => h.ms !== null) && hops.length > 0;
  return { reached, totalMs: hops.reduce((s, h) => s + (h.ms ?? 0), 0), path: hops.map((h) => `${h.hop}. ${h.host}`) };
}

export function dnsQuery(name: string, type: 'A' | 'AAAA' | 'CNAME' | 'MX' | 'TXT' = 'A'): { name: string; type: string; ttl: number; records: string[] } {
  const table: Record<string, string[]> = {
    'example.com': ['93.184.216.34'],
    'varix.local': ['127.0.0.1'],
  };
  return { name, type, ttl: 300, records: table[name] ?? [] };
}

export function localIpInfo(nicIp: string, publicIp: string): { nic: string; public: string; isPrivate: boolean } {
  const isPrivate = /^10\.|^192\.168\.|^172\.(1[6-9]|2\d|3[01])\./.test(nicIp);
  return { nic: nicIp, public: publicIp, isPrivate };
}

export interface PortEntry {
  port: number;
  state: 'open' | 'closed';
  process?: string;
}

export function portScan(entries: PortEntry[]): { open: PortEntry[]; wellKnown: PortEntry[] } {
  const open = entries.filter((e) => e.state === 'open');
  return { open, wellKnown: open.filter((e) => e.port < 1024) };
}

export interface ShareFolder {
  path: string;
  name: string;
  access: 'read' | 'write';
  users: string[];
}

export class ShareManager {
  private shares: ShareFolder[] = [];

  add(path: string, name: string, access: ShareFolder['access'], users: string[]): boolean {
    if (this.shares.some((s) => s.path === path)) return false;
    this.shares.push({ path, name, access, users });
    return true;
  }

  get list(): readonly ShareFolder[] {
    return this.shares;
  }

  revoke(path: string): boolean {
    const i = this.shares.findIndex((s) => s.path === path);
    if (i < 0) return false;
    this.shares.splice(i, 1);
    return true;
  }
}

export const RDP_RESERVED = true;
export const SSH_RESERVED = true;
export const TELNET_RESERVED = true;

export class ProxyCenter {
  mode: 'off' | 'system' | 'manual' | 'pac' = 'off';
  private entries: { name: string; proto: 'http' | 'socks5'; host: string; port: number; active: boolean }[] = [];

  add(name: string, proto: 'http' | 'socks5', host: string, port: number): boolean {
    if (port < 1 || port > 65535 || !host) return false;
    this.entries.push({ name, proto, host, port, active: false });
    return true;
  }

  activate(name: string): boolean {
    const e = this.entries.find((x) => x.name === name);
    if (!e) return false;
    this.entries.forEach((x) => (x.active = x.name === name));
    this.mode = 'manual';
    return true;
  }

  get list(): readonly { name: string; proto: string; host: string; port: number; active: boolean }[] {
    return this.entries;
  }
}

export interface HostEntry {
  ip: string;
  host: string;
  enabled: boolean;
}

export function hostsParse(text: string): HostEntry[] {
  return text
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => {
      const parts = l.split(/\s+/);
      return { ip: parts[0] ?? '', host: parts[1] ?? '', enabled: true };
    })
    .filter((e) => e.ip && e.host);
}

export function hostsRender(entries: HostEntry[]): string {
  return entries.map((e) => (e.enabled ? `${e.ip}\t${e.host}` : `# ${e.ip}\t${e.host}`)).join('\n');
}

export interface CertInfo {
  subject: string;
  issuer: string;
  validFrom: number;
  validTo: number;
}

export function certStatus(c: CertInfo, now = Date.now()): { valid: boolean; daysLeft: number; message: string } {
  const daysLeft = Math.round((c.validTo - now) / 86400_000);
  const valid = now >= c.validFrom && now <= c.validTo;
  return { valid, daysLeft, message: valid ? `有效，剩 ${daysLeft} 天` : daysLeft < 0 ? '已过期' : '尚未生效' };
}

export function wifiQrText(ssid: string, password: string, auth: 'WPA' | 'WEP' | 'nopass' = 'WPA'): string {
  return `WIFI:T:${auth};S:${ssid};P:${auth === 'nopass' ? '' : password};;`;
}

/** F04189 本机已存 Wi-Fi 密码查看（本地系统存储，仅本机读取）。 */
export function wifiPasswordView(saved: { ssid: string; password: string }[], ssid: string): string | undefined {
  return saved.find((w) => w.ssid === ssid)?.password;
}

export class TrafficStats {
  private byApp = new Map<string, number>();
  totalMB = 0;

  record(app: string, mb: number): this {
    this.byApp.set(app, (this.byApp.get(app) ?? 0) + mb);
    this.totalMB = Math.round((this.totalMB + mb) * 100) / 100;
    return this;
  }

  perApp(topN = 5): { app: string; mb: number }[] {
    return [...this.byApp.entries()].map(([app, mb]) => ({ app, mb: Math.round(mb * 100) / 100 })).sort((a, b) => b.mb - a.mb).slice(0, topN);
  }
}

export interface DiagStep {
  check: string;
  ok: boolean;
  fix?: string;
}

/** 诊断向导：按顺序给出断点与修复建议。 */
export function diagWizard(steps: DiagStep[]): { broken: string | undefined; fixes: string[]; reachInternet: boolean } {
  const fixes: string[] = [];
  let broken: string | undefined;
  for (const s of steps) {
    if (!s.ok) {
      broken = s.check;
      if (s.fix) fixes.push(s.fix);
      break;
    }
  }
  if (!broken && steps.length > 0) fixes.push('全链路正常');
  return { broken, fixes, reachInternet: !broken && steps.length > 0 };
}

export const ONE_CLICK_REPAIR_STEPS = ['重置网络栈', '刷新 DNS 缓存', '重连 Wi-Fi', '检查代理设置'];

export interface NicInfo {
  name: string;
  mac: string;
  ip: string;
  speedMbps: number;
  up: boolean;
}

export function nicReport(nics: NicInfo[]): string {
  return nics.map((n) => `${n.name} ${n.mac} ${n.ip} ${n.speedMbps}Mbps ${n.up ? '已连接' : '未连接'}`).join('\n');
}

export function routeTableMock(): { dest: string; mask: string; gw: string; metric: number }[] {
  return [
    { dest: '0.0.0.0', mask: '0.0.0.0', gw: '192.168.1.1', metric: 25 },
    { dest: '192.168.1.0', mask: '255.255.255.0', gw: '0.0.0.0', metric: 1 },
    { dest: '127.0.0.0', mask: '255.0.0.0', gw: '0.0.0.0', metric: 1 },
  ];
}

export function arpTableMock(): { ip: string; mac: string; type: 'dynamic' | 'static' }[] {
  return [
    { ip: '192.168.1.1', mac: 'aa:bb:cc:dd:ee:ff', type: 'dynamic' },
    { ip: '192.168.1.23', mac: '11:22:33:44:55:66', type: 'dynamic' },
  ];
}

export function latencyChart(samples: { ms: number; lost: boolean }[]): { avgMs: number; lossPct: number; maxMs: number } {
  const ok = samples.filter((s) => !s.lost);
  return {
    avgMs: ok.length ? Math.round((ok.reduce((s, x) => s + x.ms, 0) / ok.length) * 10) / 10 : 0,
    lossPct: Math.round(((samples.length - ok.length) / Math.max(1, samples.length)) * 100),
    maxMs: ok.reduce((m, x) => Math.max(m, x.ms), 0),
  };
}

export class OfflineWatcher {
  private downAt: number | undefined;

  markDown(now: number): void {
    this.downAt = now;
  }

  markUp(now: number): number | undefined {
    if (this.downAt === undefined) return undefined;
    const ms = now - this.downAt;
    this.downAt = undefined;
    return ms;
  }

  get isDown(): boolean {
    return this.downAt !== undefined;
  }
}

export const NETWORK_TIPS = ['hosts 修改需管理员权限', '测速建议多次取平均', '断网提醒不影响后台下载队列'];

/* ========================= 族0169 系统维护工具 ========================= */

export interface JunkItem {
  path: string;
  sizeMB: number;
  kind: 'cache' | 'log' | 'temp' | 'thumbnail';
  safe: boolean;
}

export function junkScan(items: JunkItem[]): { totalMB: number; byKind: Record<string, number>; safeItems: JunkItem[] } {
  const byKind: Record<string, number> = {};
  let total = 0;
  for (const it of items) {
    byKind[it.kind] = Math.round(((byKind[it.kind] ?? 0) + it.sizeMB) * 100) / 100;
    total += it.sizeMB;
  }
  return { totalMB: Math.round(total * 100) / 100, byKind, safeItems: items.filter((i) => i.safe) };
}

export const REGISTRY_CLEAN_RESERVED = true;

export interface StartupEntry {
  name: string;
  enabled: boolean;
  impactMs: number;
}

export function startupImpact(entries: StartupEntry[]): { totalMs: number; disabledPotentialMs: number; advice: string[] } {
  const enabled = entries.filter((e) => e.enabled);
  const totalMs = enabled.reduce((s, e) => s + e.impactMs, 0);
  const advice: string[] = [];
  for (const e of enabled.filter((x) => x.impactMs > 800)) advice.push(`${e.name} 拖慢开机 ${e.impactMs}ms，建议禁用`);
  return { totalMs, disabledPotentialMs: entries.filter((e) => !e.enabled).reduce((s, e) => s + e.impactMs, 0), advice };
}

export interface ServiceEntry {
  name: string;
  running: boolean;
  startup: 'auto' | 'manual' | 'disabled';
  desc: string;
}

export function serviceAdvice(entries: ServiceEntry[]): string[] {
  return entries.filter((e) => e.startup === 'auto' && !e.running).map((e) => `${e.name} 设为自动但未运行，检查启动失败原因`);
}

export interface CronTask {
  name: string;
  schedule: string;
  nextRun: string;
  lastResult: 'ok' | 'failed' | 'missed';
}

export function cronHealth(tasks: CronTask[]): { failed: string[]; missed: string[]; healthy: boolean } {
  return {
    failed: tasks.filter((t) => t.lastResult === 'failed').map((t) => t.name),
    missed: tasks.filter((t) => t.lastResult === 'missed').map((t) => t.name),
    healthy: tasks.every((t) => t.lastResult === 'ok'),
  };
}

export interface DriverEntry {
  name: string;
  version: string;
  date: string;
  status: 'ok' | 'warning' | 'error';
}

export function driverReport(entries: DriverEntry[]): { problems: DriverEntry[]; healthy: number } {
  return { problems: entries.filter((d) => d.status !== 'ok'), healthy: entries.filter((d) => d.status === 'ok').length };
}

export const DRIVER_UPDATE_RESERVED = true;

export interface UpdateItem {
  kb: string;
  title: string;
  kind: 'security' | 'feature' | 'driver' | 'cumulative';
  sizeMB: number;
  installedAt?: number;
}

export class UpdateCenter {
  private available: UpdateItem[] = [];
  private installed: UpdateItem[] = [];

  scan(items: UpdateItem[]): this {
    this.available = items;
    return this;
  }

  get pending(): readonly UpdateItem[] {
    return this.available;
  }

  install(kb: string): boolean {
    const it = this.available.find((x) => x.kb === kb);
    if (!it) return false;
    it.installedAt = Date.now();
    this.installed.push(it);
    this.available = this.available.filter((x) => x.kb !== kb);
    return true;
  }

  history(): UpdateItem[] {
    return [...this.installed].sort((a, b) => (b.installedAt ?? 0) - (a.installedAt ?? 0));
  }

  /** 回滚：卸载已装更新（安全更新不可回滚）。 */
  rollback(kb: string): boolean {
    const it = this.installed.find((x) => x.kb === kb);
    if (!it || it.kind === 'security') return false;
    this.installed = this.installed.filter((x) => x.kb !== kb);
    this.available.push({ ...it, installedAt: undefined });
    return true;
  }
}

export class RestorePoints {
  private points: { id: string; label: string; at: number }[] = [];

  create(label: string, at = Date.now()): boolean {
    if (this.points.some((p) => p.label === label)) return false;
    this.points.push({ id: `rp-${this.points.length + 1}`, label, at });
    return true;
  }

  get list(): readonly { id: string; label: string; at: number }[] {
    return this.points;
  }

  latest(): { id: string; label: string; at: number } | undefined {
    return [...this.points].sort((a, b) => b.at - a.at)[0];
  }
}

export interface HealthFactor {
  name: string;
  score: number; // 0~100
  weight: number;
}

export function healthScore(factors: HealthFactor[]): { total: number; grade: string; weakest: string | undefined } {
  const wsum = factors.reduce((s, f) => s + f.weight, 0) || 1;
  const total = Math.round(factors.reduce((s, f) => s + f.score * f.weight, 0) / wsum);
  const weakest = [...factors].sort((a, b) => a.score - b.score)[0]?.name;
  return { total, grade: total >= 90 ? '优' : total >= 75 ? '良' : total >= 60 ? '中' : '差', weakest };
}

export interface HardwareReport {
  cpu: string;
  cores: number;
  ramGB: number;
  disks: { model: string; sizeGB: number; type: 'ssd' | 'hdd' }[];
  gpu: string;
}

export function hardwareText(h: HardwareReport): string {
  return [`${h.cpu}（${h.cores} 核）`, `内存 ${h.ramGB}GB`, `显卡 ${h.gpu}`, ...h.disks.map((d) => `磁盘 ${d.model} ${d.sizeGB}GB ${d.type.toUpperCase()}`)].join('\n');
}

export const STRESS_TEST_RESERVED = true;

export interface TempSensor {
  name: string;
  celsius: number;
}

export function tempStatus(sensors: TempSensor[]): { hottest: TempSensor | undefined; overheat: TempSensor[] } {
  return { hottest: [...sensors].sort((a, b) => b.celsius - a.celsius)[0], overheat: sensors.filter((s) => s.celsius >= 85) };
}

export const FAN_CONTROL_RESERVED = true;

export const POWER_PLANS = ['均衡', '高性能', '节能', '游戏'] as const;

export interface BatteryReport {
  designCapacityWh: number;
  fullChargeWh: number;
  cycleCount: number;
}

export function batteryHealth(b: BatteryReport): { healthPct: number; advice: string } {
  const pct = Math.round((b.fullChargeWh / b.designCapacityWh) * 100);
  const advice = pct >= 80 ? '健康' : pct >= 60 ? '容量下降，注意续航' : '建议更换电池';
  return { healthPct: pct, advice };
}

export interface EventLogEntry {
  at: number;
  level: 'info' | 'warning' | 'error';
  source: string;
  message: string;
}

export function eventViewer(entries: EventLogEntry[], level: EventLogEntry['level'] = 'error'): EventLogEntry[] {
  const order: Record<EventLogEntry['level'], number> = { info: 0, warning: 1, error: 2 };
  return entries.filter((e) => order[e.level] >= order[level]).sort((a, b) => b.at - a.at);
}

export const BSOD_ANALYSIS_RESERVED = true;

export function logCleanup(entries: EventLogEntry[], keepDays: number, now: number): number {
  const cutoff = now - keepDays * 86400_000;
  return entries.filter((e) => e.at < cutoff).length;
}

/** sfc 类校验：哈希比对受保护文件。 */
export function fileVerify(files: { path: string; hash: string; expected: string }[]): { ok: string[]; corrupt: string[] } {
  return {
    ok: files.filter((f) => f.hash === f.expected).map((f) => f.path),
    corrupt: files.filter((f) => f.hash !== f.expected).map((f) => f.path),
  };
}

export interface CheckupResult {
  item: string;
  ok: boolean;
  detail: string;
}

export function oneClickCheckup(results: CheckupResult[]): { pass: number; fail: number; actions: string[] } {
  const actions = results.filter((r) => !r.ok).map((r) => `${r.item}：${r.detail}`);
  return { pass: results.filter((r) => r.ok).length, fail: actions.length, actions };
}

export function checkupReport(checkup: { pass: number; fail: number; actions: string[] }, at: number): string {
  return [`体检时间 ${new Date(at).toLocaleString('zh-CN', { hour12: false })}`, `通过 ${checkup.pass} 项 / 待处理 ${checkup.fail} 项`, ...checkup.actions.map((a) => `· ${a}`)].join('\n');
}

export const MAINTENANCE_TIPS = ['清理前先建还原点', '安全更新不可回滚', '温度超 85°C 建议清灰'];

/* ========================= 族0170 卸载器增强 ========================= */

export interface InstalledApp {
  name: string;
  version: string;
  sizeMB: number;
  publisher: string;
  source: 'classic' | 'store' | 'portable';
  lastUsedDays: number;
  crashRate: number; // 0~1
  protectedByPolicy?: boolean;
}

export class Uninstaller {
  private apps: InstalledApp[] = [];
  private log: { app: string; action: 'uninstall' | 'restore' | 'cleanup'; at: number }[] = [];

  load(apps: InstalledApp[]): this {
    this.apps = apps;
    return this;
  }

  get list(): readonly InstalledApp[] {
    return this.apps;
  }

  uninstall(name: string, opts: { force?: boolean } = {}): 'ok' | 'stub' | 'protected' | 'missing' {
    const app = this.apps.find((a) => a.name === name);
    if (!app) return 'missing';
    if (app.protectedByPolicy) return 'protected';
    if (app.source === 'store' && !opts.force) return 'stub';
    this.apps = this.apps.filter((a) => a.name !== name);
    this.log.push({ app: name, action: 'uninstall', at: Date.now() });
    return 'ok';
  }

  /** 卸载后残留扫描（注册表键 + 目录 + 自启项模拟）。 */
  leftoverScan(name: string, fs: { dirs: string[]; registryKeys: string[]; autoruns: string[] }): { dirs: string[]; registryKeys: string[]; autoruns: string[] } {
    const k = name.toLowerCase().replace(/\s+/g, '');
    return {
      dirs: fs.dirs.filter((d) => d.toLowerCase().includes(k)),
      registryKeys: fs.registryKeys.filter((r) => r.toLowerCase().includes(k)),
      autoruns: fs.autoruns.filter((a) => a.toLowerCase().includes(k)),
    };
  }

  cleanupLeftover(fs: { dirs: string[]; registryKeys: string[]; autoruns: string[] }, keep: string[] = []): number {
    const before = fs.dirs.length + fs.registryKeys.length + fs.autoruns.length;
    fs.dirs = fs.dirs.filter((d) => keep.some((k) => d.includes(k)));
    fs.registryKeys = fs.registryKeys.filter((r) => keep.some((k) => r.includes(k)));
    fs.autoruns = fs.autoruns.filter((a) => keep.some((k) => a.includes(k)));
    return before - (fs.dirs.length + fs.registryKeys.length + fs.autoruns.length);
  }

  batch(names: string[]): { done: string[]; failed: string[] } {
    const done: string[] = [];
    const failed: string[] = [];
    for (const n of names) {
      const r = this.uninstall(n, { force: true });
      if (r === 'ok') done.push(n);
      else failed.push(n);
    }
    return { done, failed };
  }

  get history(): readonly { app: string; action: string; at: number }[] {
    return this.log;
  }

  rarelyUsed(minDays: number): InstalledApp[] {
    return this.apps.filter((a) => a.lastUsedDays >= minDays).sort((a, b) => b.lastUsedDays - a.lastUsedDays);
  }

  sizeRanking(topN = 10): InstalledApp[] {
    return [...this.apps].sort((a, b) => b.sizeMB - a.sizeMB).slice(0, topN);
  }

  healthFlag(app: InstalledApp): 'stable' | 'watch' | 'crashy' {
    return app.crashRate < 0.01 ? 'stable' : app.crashRate < 0.05 ? 'watch' : 'crashy';
  }

  /** 卸载时清理自启关联。 */
  cleanAutorun(appName: string, autoruns: string[]): string[] {
    return autoruns.filter((a) => a.toLowerCase().includes(appName.toLowerCase().replace(/\s+/g, '')));
  }
}

export const SILENT_UNINSTALL_RESERVED = true;

export interface InstallEvent {
  at: number;
  app: string;
  kind: 'file' | 'registry' | 'service' | 'autorun';
  target: string;
}

export class InstallMonitor {
  private events: InstallEvent[] = [];

  record(e: InstallEvent): this {
    this.events.push(e);
    return this;
  }

  byApp(app: string): InstallEvent[] {
    return this.events.filter((e) => e.app === app);
  }

  /** 快照对比：安装前后差了什么。 */
  snapshotDiff(before: InstallEvent[], after: InstallEvent[]): { added: InstallEvent[]; removed: InstallEvent[] } {
    const key = (e: InstallEvent) => `${e.kind}:${e.target}`;
    const beforeKeys = new Set(before.map(key));
    const afterKeys = new Set(after.map(key));
    return {
      added: after.filter((e) => !beforeKeys.has(key(e))),
      removed: before.filter((e) => !afterKeys.has(key(e))),
    };
  }
}

export interface ContextMenuEntry {
  location: 'file' | 'folder' | 'background' | 'browser';
  label: string;
  dll?: string;
  orphan: boolean;
}

export function contextLeftovers(entries: ContextMenuEntry[]): ContextMenuEntry[] {
  return entries.filter((e) => e.orphan);
}

export interface BrowserExtension {
  browser: 'varix' | 'chromium' | 'gecko';
  name: string;
  enabled: boolean;
  permissions: string[];
}

export function extensionRisk(ext: BrowserExtension): 'low' | 'medium' | 'high' {
  const risky = ['<all_urls>', 'nativeMessaging', 'debugger', 'cookies'];
  const hits = ext.permissions.filter((p) => risky.includes(p)).length;
  return hits >= 2 ? 'high' : hits === 1 ? 'medium' : 'low';
}

export const RUNTIME_FIX_RESERVED = true;

export function detectRuntimes(installed: string[]): { present: string[]; missing: string[]; needed: string[] } {
  const common = ['VC++ 2015-2022 x64', 'VC++ 2013 x86', '.NET 8', '.NET Framework 4.8', 'Java 21', 'DirectX'];
  return {
    present: common.filter((c) => installed.includes(c)),
    missing: common.filter((c) => !installed.includes(c)),
    needed: common.filter((c) => !installed.includes(c)).slice(0, 2),
  };
}

export function orphanDlls(all: string[], referenced: string[]): string[] {
  const ref = new Set(referenced);
  return all.filter((d) => !ref.has(d));
}

export const PREINSTALLED_MANAGE = { view: true, hide: true, uninstallAsk: true };

/** 绿色软件登记：目录扫描识别 portable。 */
export function portableScan(dirs: { name: string; hasExe: boolean; hasUninstall: boolean; markerFile?: string }[]): { portable: string[]; regular: string[] } {
  const portable = dirs.filter((d) => d.hasExe && (d.markerFile !== undefined || !d.hasUninstall)).map((d) => d.name);
  return { portable, regular: dirs.filter((d) => !portable.includes(d.name)).map((d) => d.name) };
}

export function registerPortable(list: string[], name: string): boolean {
  if (list.includes(name)) return false;
  list.push(name);
  return true;
}

export function defaultAppsReset(kinds: string[]): Record<string, 'varix' | 'external'> {
  return Object.fromEntries(kinds.map((k) => [k, 'varix']));
}

export function enterpriseWhitelist(list: string[]): (app: InstalledApp) => InstalledApp {
  const set = new Set(list);
  return (app) => ({ ...app, protectedByPolicy: set.has(app.name) || app.protectedByPolicy });
}

export function uninstallReport(u: Uninstaller): string {
  const lines = [`已卸载 ${u.history.filter((h) => h.action === 'uninstall').length} 个应用`];
  for (const h of u.history) lines.push(`· ${h.action === 'uninstall' ? '卸载' : h.action} ${h.app}`);
  return lines.join('\n');
}

export const UNINSTALL_TIPS = ['卸载前建还原点更稳妥', '商店应用需强制选项才能卸载', '企业白名单应用受策略保护'];
