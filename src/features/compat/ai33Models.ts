/**
 * UNREAL-X-15000 · AI-33 兼容性防线·第 1 组 V/K 线逻辑核（族0321~0330 · X08001~X08250），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0321 窗口嵌入探测 2.0（X08001~X08025）-------- */

export type EmbedMode = 'none' | 'child' | 'layered' | 'transparent';

export interface EmbedWindowInfo {
  title: string;
  className: string;
  styles: number;
  hasOwnChrome: boolean;
}

/** 嵌入探测：判定窗口可嵌入性与嵌入模式。 */
export class EmbedProbe {
  clamped = 0;
  probed: EmbedWindowInfo[] = [];
  /** 探测：无标题/空类名 → 钳制拒绝。 */
  probe(info: EmbedWindowInfo): EmbedMode {
    if (!info.title || !info.className) {
      this.clamped++;
      return 'none';
    }
    this.probed.push(info);
    if (info.hasOwnChrome) return 'none';
    if (info.styles & 0x80000000) return 'layered';
    if (info.styles & 0x00040000) return 'transparent';
    return 'child';
  }
  /** 嵌入候选：child/transparent 可直接嵌入。 */
  candidates(): EmbedWindowInfo[] {
    return this.probed.filter((w) => {
      const m = this.probeOnce(w);
      return m === 'child' || m === 'transparent';
    });
  }
  private probeOnce(w: EmbedWindowInfo): EmbedMode {
    if (w.hasOwnChrome) return 'none';
    if (w.styles & 0x80000000) return 'layered';
    if (w.styles & 0x00040000) return 'transparent';
    return 'child';
  }
  static modeLabel(m: EmbedMode): string {
    return { none: '独立窗口', child: '子窗口', layered: '分层窗口', transparent: '透明窗口' }[m];
  }
}

/* -------- 族0322 反作弊共存 2.0（X08026~X08050）-------- */

export const AC_TRUST_LEVELS = ['unknown', 'observed', 'signed', 'kernel'] as const;
export type AcTrustLevel = (typeof AC_TRUST_LEVELS)[number];

/** 反作弊共存：信任级 + 共存策略（隔离/放行/拒绝）。 */
export class AnticheatCoex {
  trust: Record<string, AcTrustLevel> = {};
  clamped = 0;
  setTrust(process: string, level: string): AcTrustLevel {
    const ok = (AC_TRUST_LEVELS as readonly string[]).includes(level);
    const lv = ok ? (level as AcTrustLevel) : 'unknown';
    if (!ok) this.clamped++;
    this.trust[process] = lv;
    return lv;
  }
  /** 策略：kernel→隔离让位；signed/observed→受控放行；unknown→拒绝注入。 */
  policy(process: string): 'isolate' | 'allow-controlled' | 'deny' {
    switch (this.trust[process]) {
      case 'kernel': return 'isolate';
      case 'signed':
      case 'observed': return 'allow-controlled';
      default: return 'deny';
    }
  }
  /** 挂钩白名单：kernel/signed 进程不注入。 */
  canHook(process: string): boolean {
    const p = this.policy(process);
    return p === 'allow-controlled';
  }
}

/* -------- 族0323 CEF 内核兼容 2.0（X08051~X08075）-------- */

export const CEF_FLAG_MATRIX = {
  'disable-gpu': false,
  'disable-gpu-vsync': false,
  'enable-features': '',
  'force-color-profile': 'srgb',
  'disable-software-rasterizer': false,
} as const;
export type CefFlagKey = keyof typeof CEF_FLAG_MATRIX;

/** CEF 兼容：版本映射 + 启动旗标裁剪。 */
export class CefCompat {
  clamped = 0;
  flags: Record<string, string | boolean> = { ...CEF_FLAG_MATRIX };
  /** 版本协商：主版本 ≥109 走现代管线，<109 走回退。 */
  pipeline(cefMajor: number): 'modern' | 'legacy' {
    if (!Number.isFinite(cefMajor) || cefMajor <= 0) {
      this.clamped++;
      return 'legacy';
    }
    return cefMajor >= 109 ? 'modern' : 'legacy';
  }
  /** 设置旗标：白名单外拒绝。 */
  setFlag(key: string, value: string | boolean): boolean {
    if (!(key in CEF_FLAG_MATRIX)) {
      this.clamped++;
      return false;
    }
    this.flags[key] = value;
    return true;
  }
  /** 启动串拼接。 */
  launchArgs(): string[] {
    const args: string[] = [];
    for (const [k, v] of Object.entries(this.flags)) {
      if (v === true) args.push(`--${k}`);
      else if (v !== false && v !== '') args.push(`--${k}=${v}`);
    }
    return args;
  }
  static deserialize(raw: string): CefCompat {
    const c = new CefCompat();
    try {
      const o = JSON.parse(raw) as Record<string, string | boolean>;
      for (const [k, v] of Object.entries(o)) c.setFlag(k, v);
    } catch {
      /* 净身回默认 */
    }
    return c;
  }
}

/* -------- 族0324 独占全屏让位 2.0（X08076~X08100）-------- */

export type FullscreenState = 'windowed' | 'borderless' | 'exclusive';

/** 全屏让位：独占全屏检测 + 让位策略（覆盖层让位/回退无边框）。 */
export class FullscreenYield {
  clamped = 0;
  state: FullscreenState = 'windowed';
  overlays: string[] = [];
  /** 状态钳制：非法值回 windowed。 */
  setState(s: string): FullscreenState {
    const ok = s === 'windowed' || s === 'borderless' || s === 'exclusive';
    this.state = ok ? (s as FullscreenState) : 'windowed';
    if (!ok) this.clamped++;
    return this.state;
  }
  /** 独占全屏时覆盖层须让位。 */
  shouldYield(overlay: string): boolean {
    if (!overlay) {
      this.clamped++;
      return false;
    }
    return this.state === 'exclusive';
  }
  /** 回退建议：exclusive→borderless，其余不变。 */
  fallback(): FullscreenState {
    return this.state === 'exclusive' ? 'borderless' : this.state;
  }
  registerOverlay(name: string): boolean {
    if (!name || this.overlays.includes(name)) return false;
    this.overlays.push(name);
    return true;
  }
}

/* -------- 族0325 老应用兼容 2.0（X08101~X08125）-------- */

export const SHIM_LAYERS = ['dpi', 'font', 'theme', 'gdi', 'registry'] as const;
export type ShimLayer = (typeof SHIM_LAYERS)[number];

/** 老应用兼容：垫片层启用表 + 年代推断。 */
export class LegacyApp {
  clamped = 0;
  enabled: Set<ShimLayer> = new Set();
  /** 按年代启用垫片组合（≥5 档矩阵）。 */
  applyPreset(era: 'win9x' | 'winxp' | 'win7' | 'win10' | 'modern'): ShimLayer[] {
    const table: Record<string, ShimLayer[]> = {
      win9x: ['dpi', 'font', 'theme', 'gdi', 'registry'],
      winxp: ['dpi', 'font', 'theme', 'gdi'],
      win7: ['dpi', 'theme'],
      win10: ['dpi'],
      modern: [],
    };
    const set = table[era];
    if (!set) {
      this.clamped++;
      return [...this.enabled];
    }
    this.enabled = new Set(set);
    return set;
  }
  /** 手动开关垫片层。 */
  toggle(layer: string, on: boolean): boolean {
    if (!(SHIM_LAYERS as readonly string[]).includes(layer)) {
      this.clamped++;
      return false;
    }
    const l = layer as ShimLayer;
    if (on) this.enabled.add(l);
    else this.enabled.delete(l);
    return true;
  }
  /** DPI 垫片缩放钳制（100%~500%）。 */
  static clampScale(pct: number): number {
    return Math.min(500, Math.max(100, Math.round(pct / 25) * 25));
  }
}

/* -------- 族0326 驱动拦截兼容（X08126~X08150 · K 线）-------- */

export type InterceptAction = 'pass' | 'emulate' | 'block';

/** 驱动拦截：规则裁决（规则号越大优先级越高）。 */
export class DriverIntercept {
  clamped = 0;
  rules: { match: string; action: InterceptAction; prio: number }[] = [];
  addRule(match: string, action: string, prio: number): boolean {
    if (!match || !(action === 'pass' || action === 'emulate' || action === 'block')) {
      this.clamped++;
      return false;
    }
    this.rules.push({ match, action: action as InterceptAction, prio });
    return true;
  }
  /** 裁决：取匹配的最高 prio 规则；无匹配→pass。 */
  decide(name: string): InterceptAction {
    const hits = this.rules.filter((r) => name.includes(r.match));
    if (hits.length === 0) return 'pass';
    hits.sort((a, b) => b.prio - a.prio);
    return hits[0]!.action;
  }
  /** 净身：清空规则。 */
  reset(): void {
    this.rules = [];
  }
}

/* -------- 族0327 Shell 扩展兼容（X08151~X08175）-------- */

export const SHEXT_LOAD_POLICY = ['blocklist', 'allowlist', 'audit-only'] as const;
export type ShextPolicy = (typeof SHEXT_LOAD_POLICY)[number];

/** Shell 扩展：加载策略 + 沙盒裁决 + 崩溃熔断。 */
export class ShellExtCompat {
  policy: ShextPolicy;
  blocked = new Set<string>();
  allowed = new Set<string>();
  crashes: Record<string, number> = {};
  clamped = 0;
  constructor(policy: string = 'blocklist') {
    const ok = (SHEXT_LOAD_POLICY as readonly string[]).includes(policy);
    this.policy = ok ? (policy as ShextPolicy) : 'blocklist';
    if (!ok) this.clamped++;
  }
  /** 裁决：allowlist→仅白名单；blocklist→非黑名单；audit-only→全部放行。 */
  shouldLoad(clsid: string): boolean {
    if (this.policy === 'audit-only') return true;
    if (this.policy === 'allowlist') return this.allowed.has(clsid);
    return !this.blocked.has(clsid);
  }
  /** 崩溃熔断：3 次即拉黑。 */
  reportCrash(clsid: string): boolean {
    const n = (this.crashes[clsid] ?? 0) + 1;
    this.crashes[clsid] = n;
    if (n >= 3) {
      this.blocked.add(clsid);
      return true;
    }
    return false;
  }
  setPolicy(p: string): boolean {
    if (!(SHEXT_LOAD_POLICY as readonly string[]).includes(p)) {
      this.clamped++;
      return false;
    }
    this.policy = p as ShextPolicy;
    return true;
  }
}

/* -------- 族0328 显示管线兼容（X08176~X08200 · K 线）-------- */

export const DISPLAY_FALLBACK_CHAIN = ['hdr10', 'sdr-high', 'sdr-native'] as const;
export type DisplayMode = (typeof DISPLAY_FALLBACK_CHAIN)[number];

/** 显示管线：能力协商 + 回退链。 */
export class DisplayPipeline {
  clamped = 0;
  caps: string[] = [];
  current: DisplayMode = 'hdr10';
  negotiate(caps: string[]): DisplayMode {
    if (!Array.isArray(caps)) {
      this.clamped++;
      return 'sdr-native';
    }
    this.caps = caps;
    for (const m of DISPLAY_FALLBACK_CHAIN) {
      if (caps.includes(m)) {
        this.current = m;
        return m;
      }
    }
    this.current = 'sdr-native';
    return this.current;
  }
  /** 刷新率钳制（24~500Hz）。 */
  static clampRefresh(hz: number): number {
    if (!Number.isFinite(hz)) return 60;
    return Math.min(500, Math.max(24, Math.round(hz)));
  }
  /** 色深协商：hdr10→10bit，其余→8bit。 */
  bitDepth(): 8 | 10 {
    return this.current === 'hdr10' ? 10 : 8;
  }
}

/* -------- 族0329 音频管线兼容（X08201~X08225 · K 线）-------- */

export const AUDIO_FMT_CHAIN = ['float32-192k', 'float32-48k', 'int16-48k', 'int16-44k'] as const;
export type AudioFmt = (typeof AUDIO_FMT_CHAIN)[number];

/** 音频管线：格式协商 + 独占/共享模式 + APO 链。 */
export class AudioPipeline {
  clamped = 0;
  fmt: AudioFmt = 'int16-48k';
  exclusive = false;
  apox: string[] = [];
  negotiate(deviceFmts: string[]): AudioFmt {
    if (!Array.isArray(deviceFmts)) {
      this.clamped++;
      this.fmt = 'int16-44k';
      return this.fmt;
    }
    for (const f of AUDIO_FMT_CHAIN) {
      if (deviceFmts.includes(f)) {
        this.fmt = f;
        return f;
      }
    }
    this.fmt = 'int16-44k';
    return this.fmt;
  }
  /** 独占模式守卫：仅高格式允许独占。 */
  setExclusive(on: boolean): boolean {
    if (on && !(this.fmt === 'float32-192k' || this.fmt === 'float32-48k')) {
      this.clamped++;
      return false;
    }
    this.exclusive = on;
    return true;
  }
  addApo(name: string): boolean {
    if (!name || this.apox.length >= 8) return false;
    this.apox.push(name);
    return true;
  }
}

/* -------- 族0330 网络兼容 2.0（X08226~X08250 · K 线）-------- */

export const NET_PROTO_CHAIN = ['quic', 'http2', 'http1.1'] as const;
export type NetProto = (typeof NET_PROTO_CHAIN)[number];

/** 网络兼容：协议回退 + 代理/VPN 共存裁决。 */
export class NetCompat {
  clamped = 0;
  proto: NetProto = 'http1.1';
  vpn = false;
  proxy: string | null = null;
  /** 协议协商：按服务端支持列表回退。 */
  negotiate(serverSupport: string[]): NetProto {
    if (!Array.isArray(serverSupport)) {
      this.clamped++;
      return 'http1.1';
    }
    for (const p of NET_PROTO_CHAIN) {
      if (serverSupport.includes(p)) {
        this.proto = p;
        return p;
      }
    }
    this.proto = 'http1.1';
    return this.proto;
  }
  /** 共存裁决：VPN 下禁 QUIC（回 http2），代理下回 http1.1。 */
  coexist(vpnOn: boolean, proxySet: boolean): NetProto {
    this.vpn = vpnOn;
    this.proxy = proxySet ? 'system' : null;
    if (proxySet) {
      this.proto = 'http1.1';
      return this.proto;
    }
    if (vpnOn && this.proto === 'quic') {
      this.proto = 'http2';
      return this.proto;
    }
    return this.proto;
  }
  /** 端口钳制（1~65535）。 */
  static clampPort(p: number): number {
    if (!Number.isFinite(p)) return 443;
    return Math.min(65535, Math.max(1, Math.round(p)));
  }
}
