// AURORA-10000: AI-54 批次（领域11 开放生态 · 族0266~0270 · F06626~F06750），勿删。
// 内核开放（文档/接口冻结，桌面侧消费） / 桌面协议 / AI 生态位 / 内容格式开放 / 开放治理。

/* ===================== 族0266 内核开放（归属内核：接口冻结 + 文档） ===================== */

export interface KernelDoc {
  id: string;
  title: string;
  domain: string;
  frozen: boolean;
}

export const KERNEL_OPEN_DOCS: KernelDoc[] = [
  { id: 'KD-01', title: '内核架构总览', domain: 'arch', frozen: true },
  { id: 'KD-02', title: '20 域模块图', domain: 'arch', frozen: true },
  { id: 'KD-03', title: '系统调用参考', domain: 'syscall', frozen: true },
  { id: 'KD-04', title: '驱动开发指南（DDK）', domain: 'ddk', frozen: true },
  { id: 'KD-05', title: '示例驱动：串口回显', domain: 'ddk', frozen: true },
  { id: 'KD-06', title: '调试符号发布说明', domain: 'debug', frozen: true },
  { id: 'KD-07', title: '调试协议（qmon）', domain: 'debug', frozen: true },
  { id: 'KD-08', title: 'QEMU 调试指南', domain: 'debug', frozen: true },
  { id: 'KD-09', title: '观测位（eBPF 类预留）', domain: 'observe', frozen: true },
  { id: 'KD-10', title: '追踪点清单', domain: 'observe', frozen: true },
  { id: 'KD-11', title: '性能计数器文档', domain: 'perf', frozen: true },
  { id: 'KD-12', title: 'Limine 引导协议', domain: 'boot', frozen: true },
  { id: 'KD-13', title: '内存布局图', domain: 'mm', frozen: true },
  { id: 'KD-14', title: '调度器设计', domain: 'sched', frozen: true },
  { id: 'KD-15', title: 'VFS 设计', domain: 'vfs', frozen: true },
  { id: 'KD-16', title: '安全模型', domain: 'sec', frozen: true },
  { id: 'KD-17', title: '内核贡献指南', domain: 'governance', frozen: true },
  { id: 'KD-18', title: '内核 RFC 流程', domain: 'governance', frozen: true },
  { id: 'KD-19', title: '行为准则', domain: 'governance', frozen: true },
  { id: 'KD-20', title: '内核路线图', domain: 'governance', frozen: true },
];

export interface KernelTracepoint {
  name: string;
  domain: string;
  args: string[];
}

export const KERNEL_TRACEPOINTS: KernelTracepoint[] = [
  { name: 'sched:switch', domain: 'sched', args: ['prev_pid', 'next_pid'] },
  { name: 'mm:alloc', domain: 'mm', args: ['addr', 'size'] },
  { name: 'vfs:lookup', domain: 'vfs', args: ['path', 'ret'] },
  { name: 'ipc:send', domain: 'ipc', args: ['ch', 'len'] },
  { name: 'syscall:enter', domain: 'syscall', args: ['num', 'a0', 'a1'] },
];

export interface KernelPerfCounter {
  name: string;
  unit: string;
  domain: string;
}

export const KERNEL_PERF_COUNTERS: KernelPerfCounter[] = [
  { name: 'ctx_switches', unit: 'count', domain: 'sched' },
  { name: 'page_faults', unit: 'count', domain: 'mm' },
  { name: 'irq_rate', unit: 'hz', domain: 'arch' },
  { name: 'ipc_latency', unit: 'us', domain: 'ipc' },
];

export function kernelDocIndex(domain?: string): KernelDoc[] {
  return domain ? KERNEL_OPEN_DOCS.filter((d) => d.domain === domain) : [...KERNEL_OPEN_DOCS];
}

export function docsFrozen(docs: KernelDoc[]): boolean {
  return docs.length > 0 && docs.every((d) => d.frozen);
}

/* ===================== 族0267 桌面协议 ===================== */

export interface DesktopProtocol {
  number: number; // 协议注册号
  name: string;
  version: number;
  status: 'stable' | 'experimental' | 'deprecated';
  impl: 'desktop' | 'desktop+kernel';
}

export const DESKTOP_PROTOCOLS: DesktopProtocol[] = [
  { number: 1, name: 'varix compositor', version: 2, status: 'stable', impl: 'desktop+kernel' },
  { number: 2, name: 'varix embed', version: 1, status: 'stable', impl: 'desktop+kernel' },
  { number: 3, name: 'varix theme', version: 1, status: 'stable', impl: 'desktop' },
  { number: 4, name: 'varix widget', version: 1, status: 'stable', impl: 'desktop' },
  { number: 5, name: 'varix plugin', version: 3, status: 'stable', impl: 'desktop' },
  { number: 6, name: 'varix notify', version: 1, status: 'stable', impl: 'desktop' },
  { number: 7, name: 'varix clipboard', version: 1, status: 'stable', impl: 'desktop+kernel' },
  { number: 8, name: 'varix drag-drop', version: 1, status: 'stable', impl: 'desktop' },
  { number: 9, name: 'varix screenshot', version: 1, status: 'stable', impl: 'desktop+kernel' },
  { number: 10, name: 'varix screencast', version: 1, status: 'experimental', impl: 'desktop+kernel' },
  { number: 11, name: 'varix controlled-inject', version: 1, status: 'experimental', impl: 'desktop' },
  { number: 12, name: 'varix automation', version: 1, status: 'stable', impl: 'desktop' },
  { number: 13, name: 'varix wallpaper', version: 1, status: 'stable', impl: 'desktop' },
  { number: 14, name: 'varix sound-theme', version: 1, status: 'stable', impl: 'desktop' },
  { number: 15, name: 'varix icon-pack', version: 1, status: 'stable', impl: 'desktop' },
  { number: 16, name: 'varix cursor-pack', version: 1, status: 'stable', impl: 'desktop' },
];

/** 协议注册表：编号唯一，可按号查询。 */
export class ProtocolRegistry {
  private byNumber = new Map<number, DesktopProtocol>();
  private byName = new Map<string, DesktopProtocol>();

  constructor(protos: DesktopProtocol[]) {
    for (const p of protos) {
      if (this.byNumber.has(p.number) || this.byName.has(p.name)) continue; // 去重
      this.byNumber.set(p.number, p);
      this.byName.set(p.name, p);
    }
  }

  byNum(n: number): DesktopProtocol | undefined {
    return this.byNumber.get(n);
  }

  all(): DesktopProtocol[] {
    return [...this.byNumber.values()].sort((a, b) => a.number - b.number);
  }

  /** 弃用：stable → deprecated，并给出替代协议。 */
  deprecate(n: number, replacement: number): DesktopProtocol | undefined {
    const p = this.byNumber.get(n);
    if (!p || !this.byNumber.has(replacement) || replacement === n) return undefined;
    p.status = 'deprecated';
    return p;
  }

  /** 兼容测试：客户端版本 ≤ 协议版本 即兼容。 */
  compatTest(protoName: string, clientVersion: number): boolean {
    const p = this.byName.get(protoName);
    if (!p || p.status === 'deprecated') return false;
    return clientVersion <= p.version;
  }
}

export function protocolRfcIndex(): Array<{ id: string; protocol: string; state: 'draft' | 'accepted' }> {
  return [
    { id: 'proto-rfc-001', protocol: 'varix screencast v2', state: 'draft' },
    { id: 'proto-rfc-002', protocol: 'varix embed v2 受控嵌入', state: 'accepted' },
  ];
}

/* ===================== 族0268 AI 生态位 ===================== */

export interface LocalModel {
  id: string;
  format: 'gguf';
  sizeMb: number;
  params: string;
  quant: string;
  granted: boolean;
  sandboxed: boolean;
}

export class ModelManager {
  private models = new Map<string, LocalModel>();
  private downloads = new Map<string, { total: number; done: number }>();

  register(m: LocalModel): boolean {
    if (m.format !== 'gguf') return false; // 只收 GGUF
    if (this.models.has(m.id)) return false;
    this.models.set(m.id, { ...m, sandboxed: true }); // 默认沙箱
    return true;
  }

  startDownload(id: string, totalMb: number): boolean {
    if (this.downloads.has(id)) return false;
    this.downloads.set(id, { total: totalMb, done: 0 });
    return true;
  }

  tick(id: string, mb: number): 'downloading' | 'done' | 'none' {
    const d = this.downloads.get(id);
    if (!d) return 'none';
    d.done = Math.min(d.total, d.done + mb);
    return d.done >= d.total ? 'done' : 'downloading';
  }

  grant(id: string, granted: boolean): boolean {
    const m = this.models.get(id);
    if (!m) return false;
    m.granted = granted;
    return true;
  }

  list(): LocalModel[] {
    return [...this.models.values()];
  }
}

/** 算力预算：NPU/GPU/CPU 时间片守护。 */
export class ComputeBudget {
  constructor(private budgetMsPerMin: number) {}
  private used = 0;
  private windowStart = 0;

  consume(ms: number, now: number): boolean {
    if (now - this.windowStart >= 60_000) {
      this.windowStart = now;
      this.used = 0;
    }
    if (this.used + ms > this.budgetMsPerMin) return false;
    this.used += ms;
    return true;
  }

  usage(): number {
    return this.budgetMsPerMin === 0 ? 0 : Math.round((this.used / this.budgetMsPerMin) * 100);
  }
}

export interface AiActionLog {
  at: number;
  model: string;
  action: string;
  localOnly: boolean;
  bytesOut: number;
}

/** 全本地承诺：任何出站字节都视为违规。 */
export function auditAiPrivacy(logs: AiActionLog[]): AiActionLog[] {
  return logs.filter((l) => !l.localOnly || l.bytesOut > 0);
}

/** 自然语言命令解析（规则级）：动词 + 目标。 */
export function parseNlCommand(text: string): { verb: string; target: string } | null {
  const t = text.trim().toLowerCase();
  const verbs = ['打开', '关闭', '搜索', '整理', '提醒', '摘要', '翻译', '清空'];
  for (const v of verbs) {
    if (t.startsWith(v)) {
      const target = text.trim().slice(v.length).trim();
      return target ? { verb: v, target } : null;
    }
  }
  return null;
}

/* ===================== 族0269 内容格式开放 ===================== */

export interface ContentFormat {
  id: string;
  app: 'write' | 'mind' | 'todo' | 'calendar' | 'note' | 'wallpaper' | 'theme' | 'widget' | 'plugin' | 'config' | 'snapshot' | 'backup' | 'log';
  extension: string;
  version: number;
  openSpec: boolean;
  schema: Record<string, string>;
}

export const CONTENT_FORMATS: ContentFormat[] = [
  { id: 'fmt-note', app: 'write', extension: '.vnote', version: 1, openSpec: true, schema: { title: 'string', body: 'markdown', tags: 'string[]' } },
  { id: 'fmt-mind', app: 'mind', extension: '.vmind', version: 1, openSpec: true, schema: { root: 'node' } },
  { id: 'fmt-todo', app: 'todo', extension: '.vtodo', version: 1, openSpec: true, schema: { items: 'todo[]' } },
  { id: 'fmt-cal', app: 'calendar', extension: '.ics', version: 1, openSpec: true, schema: { events: 'vevent[]' } },
  { id: 'fmt-sticky', app: 'note', extension: '.vsticky', version: 1, openSpec: true, schema: { text: 'string', color: 'string' } },
  { id: 'fmt-wall', app: 'wallpaper', extension: '.vwall', version: 1, openSpec: true, schema: { image: 'path', meta: 'json' } },
  { id: 'fmt-theme', app: 'theme', extension: '.vtheme', version: 1, openSpec: true, schema: { tokens: 'map' } },
  { id: 'fmt-widget', app: 'widget', extension: '.vwidget', version: 1, openSpec: true, schema: { manifest: 'json' } },
  { id: 'fmt-plugin', app: 'plugin', extension: '.vplugin', version: 1, openSpec: true, schema: { manifest: 'json' } },
  { id: 'fmt-config', app: 'config', extension: '.toml', version: 1, openSpec: true, schema: { section: 'map' } },
  { id: 'fmt-snap', app: 'snapshot', extension: '.vsnap', version: 1, openSpec: true, schema: { blocks: 'block[]' } },
  { id: 'fmt-backup', app: 'backup', extension: '.vbk', version: 1, openSpec: true, schema: { manifest: 'json', payload: 'bytes' } },
  { id: 'fmt-log', app: 'log', extension: '.vlog', version: 1, openSpec: true, schema: { lines: 'line[]' } },
];

export function formatVersioned(f: ContentFormat, dataVersion: number): 'ok' | 'too-new' | 'too-old' {
  if (dataVersion > f.version) return 'too-new';
  if (dataVersion < 1) return 'too-old';
  return 'ok';
}

export function formatCompatSuite(fmts: ContentFormat[]): { pass: boolean; failing: string[] } {
  const failing = fmts.filter((f) => !f.openSpec || !f.extension.startsWith('.') || Object.keys(f.schema).length === 0).map((f) => f.id);
  return { pass: failing.length === 0, failing };
}

/** 导入器开放：第三方注册转换器。 */
export class ImporterRegistry {
  private importers = new Map<string, (raw: string) => unknown>();

  register(fromExt: string, fn: (raw: string) => unknown): boolean {
    if (this.importers.has(fromExt)) return false;
    this.importers.set(fromExt, fn);
    return true;
  }

  convert(fromExt: string, raw: string): unknown | undefined {
    return this.importers.get(fromExt)?.(raw);
  }

  list(): string[] {
    return [...this.importers.keys()].sort();
  }
}

/* ===================== 族0270 开放治理 ===================== */

export interface RfcEntry {
  id: string;
  title: string;
  state: 'draft' | 'review' | 'accepted' | 'rejected' | 'withdrawn';
  links: string[];
}

export class RfcIndex {
  private entries = new Map<string, RfcEntry>();

  add(e: RfcEntry): boolean {
    if (this.entries.has(e.id)) return false;
    this.entries.set(e.id, e);
    return true;
  }

  transition(id: string, to: RfcEntry['state']): boolean {
    const e = this.entries.get(id);
    if (!e) return false;
    const legal: Record<RfcEntry['state'], RfcEntry['state'][]> = {
      draft: ['review', 'withdrawn'],
      review: ['accepted', 'rejected', 'withdrawn'],
      accepted: [],
      rejected: [],
      withdrawn: [],
    };
    if (!legal[e.state].includes(to)) return false;
    e.state = to;
    return true;
  }

  list(state?: RfcEntry['state']): RfcEntry[] {
    const all = [...this.entries.values()].sort((a, b) => a.id.localeCompare(b.id));
    return state ? all.filter((e) => e.state === state) : all;
  }
}

export interface AdrRecord {
  id: string;
  decision: string;
  context: string;
  consequences: string;
  supersedes?: string;
}

export interface WorkingGroup {
  name: string;
  scope: string;
  members: string[];
  minutes: string[];
}

export function groupMeetingMinutes(g: WorkingGroup, minute: string): WorkingGroup {
  return { ...g, minutes: [...g.minutes, minute] };
}

export interface RoadmapVote {
  feature: string;
  votes: number;
}

export function voteRoadmap(items: RoadmapVote[], feature: string): RoadmapVote[] {
  return items.map((i) => (i.feature === feature ? { ...i, votes: i.votes + 1 } : i));
}

export function roadmapTop(items: RoadmapVote[], n = 5): RoadmapVote[] {
  return [...items].sort((a, b) => b.votes - a.votes).slice(0, n);
}

export const GOVERNANCE_CHARTER_SECTIONS = [
  '使命与价值', '社区角色', '决策机制', '行为准则执行', '贡献者权利', '资源与资金透明', '商标与品牌', '修订程序',
] as const;

export interface CoCIncident {
  id: string;
  severity: 1 | 2 | 3;
  resolved: boolean;
  actionTaken?: string;
}

export function cocEnforcement(incidents: CoCIncident[]): { unresolved: number; overdue: CoCIncident[] } {
  return { unresolved: incidents.filter((i) => !i.resolved).length, overdue: incidents.filter((i) => !i.resolved && i.severity === 1) };
}

export function contributorHonor(contribs: Array<{ name: string; commits: number; reviews: number }>, n = 5): string[] {
  return [...contribs]
    .sort((a, b) => b.commits * 2 + b.reviews - (a.commits * 2 + a.reviews))
    .slice(0, n)
    .map((c) => c.name);
}
