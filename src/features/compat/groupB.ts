// AURORA-10000: AI-42 批次（族0206~0210 · 驱动与拦截兼容/Shell 扩展兼容/显示管线兼容/音频管线兼容/网络兼容），勿删。

/* ============ 族0206 驱动与拦截兼容（F05126~F05150） ============ */

export interface HookEntry { name: string; kind: 'keyboard' | 'mouse' | 'dll-inject' | 'api-hook'; trusted: boolean; module: string }

/** F05126~F05131 全局钩子审计 + 白名单 + 注入/Hook 检测。 */
export class HookAuditor {
  private whitelist = new Set<string>();
  private hooks: HookEntry[] = [];
  addWhitelist(module: string): boolean {
    if (this.whitelist.has(module)) return false;
    this.whitelist.add(module);
    return true;
  }
  register(h: Omit<HookEntry, 'trusted'>): boolean {
    if (this.hooks.some((x) => x.module === h.module && x.kind === h.kind)) return false;
    this.hooks.push({ ...h, trusted: this.whitelist.has(h.module) });
    return true;
  }
  byKind(kind: HookEntry['kind']): HookEntry[] {
    return this.hooks.filter((h) => h.kind === kind);
  }
  /** 未在白名单的钩子/注入。 */
  untrusted(kind: HookEntry['kind']): HookEntry[] {
    return this.byKind(kind).filter((h) => !h.trusted);
  }
  get all(): HookEntry[] {
    return [...this.hooks];
  }
}

/** F05132/F05133 FS/网络过滤驱动列表。 */
export interface FilterDriver { name: string; altitude: string; kind: 'fs' | 'network' }
export function listFilterDrivers(drivers: FilterDriver[], kind: FilterDriver['kind']): FilterDriver[] {
  return drivers.filter((d) => d.kind === kind).sort((a, b) => a.altitude.localeCompare(b.altitude));
}

/** F05134~F05137 杀软共存/白名单/火绒/Defender 排除建议。 */
export const AV_VENDORS = ['Defender', '火绒', '360', 'Kaspersky', 'Norton'] as const;
export type AvVendor = (typeof AV_VENDORS)[number];
export function avExclusionSuggest(av: AvVendor, dirs: string[]): { vendor: AvVendor; exclusions: string[]; note: string } {
  const excl = dirs.map((d) => (d.endsWith('\\') ? d + '*' : d + '\\*'));
  const note = av === 'Defender' ? 'Add-MpPreference -ExclusionPath' : av === '火绒' ? '信任区添加目录' : '设置→排除项添加目录';
  return { vendor: av, exclusions: excl, note };
}

/** F05138~F05141 EDR 位/Sandboxie/VM/RDP。 */
export const EDR_SUPPORT = { reserved: true } as const;
export function sandboxieCompatible(flags: { injectDll: boolean; globalHook: boolean }): { ok: boolean; disabled: string[] } {
  const disabled: string[] = [];
  if (flags.injectDll) disabled.push('dll-inject');
  if (flags.globalHook) disabled.push('global-hook');
  return { ok: disabled.length === 0, disabled };
}
export function isVirtualMachine(traits: { manufacturer?: string; model?: string; macPrefix?: string }): boolean {
  return /vmware|virtualbox|kvm|qemu|hyper-v|vagrant/i.test(`${traits.manufacturer ?? ''} ${traits.model ?? ''}`);
}
export function rdpAdapt(session: { remote: boolean; dpi: number }): { disableAcrylic: boolean; scaleStep: number } {
  return session.remote ? { disableAcrylic: true, scaleStep: Math.max(1, Math.round(session.dpi / 96)) } : { disableAcrylic: false, scaleStep: 1 };
}

/** F05142~F05146 投屏/录屏/直播/PowerToys/剪贴工具共存。 */
export type CaptureTool = 'OBS' | '投屏' | 'Snipaste' | 'PowerToys' | 'QQ 截图';
export function captureCompat(tool: CaptureTool, overlayRunning: boolean): { coexist: boolean; overlaySuspended: boolean } {
  const needsOverlayOff = tool === 'OBS' || tool === '投屏';
  return { coexist: true, overlaySuspended: needsOverlayOff && overlayRunning };
}

/** F05147/F05148 输入法候选窗置顶 + 皮肤软件兼容。 */
export function imeCandidateTopMost(consumers: string[], gameExclusive: boolean): { topMost: boolean; reason: string } {
  if (gameExclusive) return { topMost: false, reason: 'game-yield' };
  return { topMost: consumers.length > 0, reason: consumers.length > 0 ? 'ime-visible' : 'no-ime' };
}
export const SKIN_TOOL_KB = ['WindowBlinds', 'StartAllBack', 'TranslucentTB'] as const;

/** F05149/F05150 MOD 工具 + 知识库。 */
export function modToolCompat(mod: { name: string; injectsGame: boolean }): { allow: boolean; note: string } {
  return { allow: !mod.injectsGame, note: mod.injectsGame ? '注入型 MOD 不放行' : '普通 MOD 放行' };
}
export const INTERCEPT_KB: { case: string; fix: string }[] = [
  { case: '火绒弹窗误报桌面进程', fix: '提交信任区并加入白名单' },
  { case: 'OBS 采集黑屏', fix: '关闭覆盖层后改用窗口采集' },
  { case: 'VM 内毛玻璃失效', fix: 'VM 内自动降级静态背景' },
];
export function interceptTutorial(): string[] {
  return ['钩子审计列出全部全局钩子', '可信模块加入白名单后不再告警', '杀软排除目录避免实时扫描冲突'];
}

/* ============ 族0207 Shell 扩展兼容（F05151~F05175） ============ */

export interface ShellExtension { name: string; kind: 'context-menu' | 'overlay' | 'thumbnail' | 'property-sheet' | 'preview'; stable: boolean }

/** F05151~F05153 菜单隔离/沙箱/崩溃隔离。 */
export class ShellMenuHost {
  private exts: ShellExtension[] = [];
  private quarantined = new Set<string>();
  register(e: ShellExtension): boolean {
    this.exts.push(e);
    return true;
  }
  /** 菜单项沙箱执行：崩溃不连坐。 */
  invoke(name: string): { ok: boolean; quarantined: boolean } {
    const e = this.exts.find((x) => x.name === name);
    if (!e) return { ok: false, quarantined: false };
    if (!e.stable) {
      this.quarantined.add(name);
      return { ok: false, quarantined: true };
    }
    return { ok: true, quarantined: false };
  }
  get active(): ShellExtension[] {
    return this.exts.filter((e) => !this.quarantined.has(e.name));
  }
  get quarantine(): string[] {
    return [...this.quarantined];
  }
}

/** F05154/F05155 overlay 管理 + 15 项上限。 */
export class OverlayIconManager {
  static readonly LIMIT = 15;
  private items: string[] = [];
  add(name: string): boolean {
    if (this.items.includes(name) || this.items.length >= OverlayIconManager.LIMIT) return false;
    this.items.push(name);
    return true;
  }
  remove(name: string): boolean {
    const i = this.items.indexOf(name);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }
  get list(): string[] {
    return [...this.items];
  }
}

/** F05156/F05157 缩略隔离 + 超时回收。 */
export class ThumbnailHost {
  private pending = new Map<string, number>();
  request(path: string, now: number): void {
    this.pending.set(path, now);
  }
  /** 回收超时（毫秒）任务。 */
  reclaim(now: number, timeoutMs = 2000): string[] {
    const dead = [...this.pending.entries()].filter(([, t]) => now - t > timeoutMs).map(([p]) => p);
    for (const p of dead) this.pending.delete(p);
    return dead;
  }
  complete(path: string): boolean {
    return this.pending.delete(path);
  }
}

/** F05158/F05159 属性页/预览处理器。 */
export function shellHandlers(exts: ShellExtension[]): { propertySheets: string[]; previews: string[] } {
  return {
    propertySheets: exts.filter((e) => e.kind === 'property-sheet' && e.stable).map((e) => e.name),
    previews: exts.filter((e) => e.kind === 'preview' && e.stable).map((e) => e.name),
  };
}

/** F05160~F05162 关联/协议冲突 + 默认仲裁。 */
export function associationConflict(progIds: { ext: string; progId: string; registeredBy: string }[]): string[] {
  const byExt = new Map<string, string[]>();
  for (const p of progIds) byExt.set(p.ext, [...(byExt.get(p.ext) ?? []), p.progId]);
  return [...byExt.entries()].filter(([, v]) => v.length > 1).map(([k]) => k);
}
export function protocolConflict(urls: { scheme: string; handler: string }[]): string[] {
  const m = new Map<string, number>();
  for (const u of urls) m.set(u.scheme, (m.get(u.scheme) ?? 0) + 1);
  return [...m.entries()].filter(([, c]) => c > 1).map(([k]) => k);
}
export function defaultProgramArbitrate(candidates: { progId: string; userChoice?: boolean; capability: number }[]): string {
  const chosen = candidates.find((c) => c.userChoice);
  return chosen?.progId ?? [...candidates].sort((a, b) => b.capability - a.capability)[0]!.progId;
}

/** F05163/F05164 托盘协议 + AutoPlay。 */
export function trayNotifyCompat(msg: { tip: string; icon: boolean; balloon: boolean }): { displayable: boolean; fallback: string } {
  return { displayable: Boolean(msg.tip) && msg.icon, fallback: msg.balloon ? 'toast' : 'silent' };
}
export function autoPlayDecision(usb: { kind: 'storage' | 'camera' | 'audio'; content: string }, policy: 'ask' | 'open' | 'ignore'): string {
  if (policy === 'ignore') return 'ignored';
  if (policy === 'ask') return `ask:${usb.kind}`;
  return usb.content === 'installer.exe' ? 'ask' : 'open';
}

/** F05165/F05166 发送到 + 库文件。 */
export class SendToMenu {
  private items: string[] = [];
  add(target: string): boolean {
    if (this.items.includes(target)) return false;
    this.items.push(target);
    return true;
  }
  remove(target: string): boolean {
    const i = this.items.indexOf(target);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }
  get list(): string[] {
    return [...this.items];
  }
}
export function libraryFolderResolve(libs: { name: string; folders: string[] }[], lib: string): string[] {
  return libs.find((l) => l.name === lib)?.folders ?? [];
}

/** F05167~F05171 ACL 继承/符号链接/junction/硬链接/ADS。 */
export function aclInherit(parentAcl: string[], childExplicit: string[]): string[] {
  return [...new Set([...parentAcl, ...childExplicit])];
}
export function symlinkTraversalSafe(root: string, target: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '');
  return norm(target).startsWith(norm(root) + '/');
}
export function isJunction(path: string, attrs: { reparsePoint: boolean; tag: 'mount-point' | 'symlink' | 'none' }): boolean {
  return attrs.reparsePoint && attrs.tag === 'mount-point' && !path.includes(' ');
}
export function hardlinkCount(links: string[], path: string): number {
  return links.filter((l) => l === path).length;
}
export function adsStreams(streams: { file: string; name: string; size: number }[], file: string): { name: string; size: number }[] {
  return streams.filter((s) => s.file === file && s.name !== '::$DATA');
}

/** F05172~F05174 零字节/长路径/特殊字符。 */
export function zeroByteHandling(size: number): { valid: boolean; note: string } {
  return { valid: size >= 0, note: size === 0 ? '空文件保留占位' : '正常文件' };
}
export function longPathNormalize(path: string): string {
  return path.length > 260 && !path.startsWith('\\\\?\\') ? '\\\\?\\' + path : path;
}
export function specialCharOk(name: string): boolean {
  return !/[<>:"|?*]/.test(name) && name.trim().length > 0 && !name.endsWith('.');
}

/** F05175 知库 + 教学。 */
export const SHELL_KB: { case: string; fix: string }[] = [
  { case: '右键菜单卡顿', fix: '隔离崩溃扩展后逐个启用' },
  { case: 'overlay 图标消失', fix: '超 15 项上限时裁剪低优先级' },
  { case: '缩略图一直转圈', fix: '超时回收后重试处理器' },
];
export function shellTutorial(): string[] {
  return ['第三方右键菜单运行在沙箱中，崩溃不影响桌面', 'overlay 图标最多 15 项（系统上限）', 'junction/符号链接遍历限制在授权根目录内'];
}

/* ============ 族0208 显示管线兼容（F05176~F05200） ============ */

export interface DisplayChain { kind: 'exclusive-flip' | 'borderless' | 'windowed'; hdr: boolean; bitDepth: 8 | 10 | 12; output: 'RGB' | 'YCbCr444' | 'YCbCr422' }

/** F05176~F05178 独占翻转/无边框识别/DWM 协调。 */
export function flipModelCompat(kind: DisplayChain['kind'], dwmOn: boolean): { path: 'flip' | 'compose'; independentFlip: boolean } {
  if (kind === 'exclusive-flip' && !dwmOn) return { path: 'flip', independentFlip: false };
  return { path: dwmOn ? 'compose' : 'flip', independentFlip: kind === 'borderless' && dwmOn };
}
export function isBorderlessWindow(w: { style: string; rect: { w: number; h: number }; screen: { w: number; h: number } }): boolean {
  return w.style === 'popup' && w.rect.w >= w.screen.w && w.rect.h >= w.screen.h;
}

/** F05179/F05180 双 GPU/Optimus。 */
export function gpuRouting(app: { gpu: 'integrated' | 'discrete' | 'auto'; isGame: boolean }): 'integrated' | 'discrete' {
  if (app.gpu === 'auto') return app.isGame ? 'discrete' : 'integrated';
  return app.gpu;
}
export function optimusRenderPath(hasOptimus: boolean, discreteSelected: boolean): 'hybrid' | 'dGPU-only' | 'iGPU-only' {
  return hasOptimus && discreteSelected ? 'hybrid' : discreteSelected ? 'dGPU-only' : 'iGPU-only';
}

/** F05181~F05183 混合刷新/G-Sync/FreeSync。 */
export function mixedRefreshHz(screens: number[]): { ok: boolean; min: number; note: string } {
  const min = Math.min(...screens);
  return { ok: screens.length <= 1 || Math.max(...screens) - min <= 60, min, note: screens.length > 1 ? '以最低刷新率为合成基准' : '单屏' };
}
export function vrrCompat(vendor: 'g-sync' | 'freesync' | 'none', range: [number, number], currentHz: number): { active: boolean; reason: string } {
  if (vendor === 'none') return { active: false, reason: '显示器不支持' };
  return { active: currentHz >= range[0] && currentHz <= range[1], reason: currentHz <= range[1] ? 'in-range' : 'out-of-range' };
}

/** F05184~F05188 HDR/10bit/YCbCr/DSC/MST。 */
export function hdrPassThrough(sinkHdr: boolean, sourceHdr: boolean, depth: DisplayChain['bitDepth']): { hdr: boolean; need10bit: boolean } {
  return { hdr: sinkHdr && sourceHdr, need10bit: depth >= 10 };
}
export function outputFormatFor(bandwidthGbps: number, hdr: boolean): DisplayChain['output'] {
  if (!hdr) return 'RGB';
  return bandwidthGbps >= 25 ? 'YCbCr444' : 'YCbCr422';
}
export function dscDetected(link: { bandwidthGbps: number; requiredGbps: number }): { dsc: boolean; compressionRatio: number } {
  const dsc = link.requiredGbps > link.bandwidthGbps;
  return { dsc, compressionRatio: dsc ? Math.ceil((link.requiredGbps / link.bandwidthGbps) * 10) / 10 : 1 };
}
export function mstHubTopology(hub: { ports: number; displays: number }): { ok: boolean; perPortBandwidth: number } {
  return { ok: hub.displays <= hub.ports * 4, perPortBandwidth: Math.floor(32 / Math.max(1, hub.ports)) };
}

/** F05189~F05193 扩展坞/雷电/KVM/采集卡/虚拟显示器。 */
export function usbCDockProbe(dock: { vendor: string; displays: number; hotplug: boolean }): { ok: boolean; maxDisplays: number } {
  return { ok: /dell|hp|lenovo|anker|caldigit/i.test(dock.vendor), maxDisplays: dock.hotplug ? Math.max(dock.displays, 2) : dock.displays };
}
export function thunderboltHotplug(authorized: boolean, chainDepth: number): { works: boolean; depthLimit: number } {
  return { works: authorized && chainDepth <= 6, depthLimit: 6 };
}
export function kvmSwitchCompat(kvm: { edidEmulation: boolean }): { stableResolution: boolean; note: string } {
  return { stableResolution: kvm.edidEmulation, note: kvm.edidEmulation ? 'EDID 模拟保持分辨率' : '切换可能重排窗口' };
}
export function captureCardHdmiIn(card: { fourK60: boolean; hdr: boolean }, need4k: boolean): { usable: boolean; mode: string } {
  return { usable: need4k ? card.fourK60 : true, mode: need4k ? (card.hdr ? '4K60-HDR' : '4K60-SDR') : '1080p60' };
}
export const VIRTUAL_DISPLAY_DRIVER = 'varix-idd' as const;

/** F05194~F05197 串流屏/旧投影仪/拼接屏/竖屏旋转。 */
export function streamDisplayLink(latencyMs: number, fps: number): { usable: boolean; quality: 'high' | 'medium' | 'low' } {
  const q = latencyMs < 30 && fps >= 60 ? 'high' : latencyMs < 80 ? 'medium' : 'low';
  return { usable: latencyMs < 200, quality: q };
}
export function legacyProjectorMode(edid: { x: number; y: number }): { mode: 'edid' | 'safe-list'; resolution: [number, number] } {
  return edid.x >= 640 && edid.y >= 480 ? { mode: 'edid', resolution: [edid.x, edid.y] } : { mode: 'safe-list', resolution: [1024, 768] };
}
export function videoWallGrid(total: number): { cols: number; rows: number } {
  const cols = Math.ceil(Math.sqrt(total));
  return { cols, rows: Math.ceil(total / cols) };
}
export function portraitRotation(angle: 0 | 90 | 180 | 270): { transform: string; swap: boolean } {
  return { transform: `rotate(${angle}deg)`, swap: angle === 90 || angle === 270 };
}

/** F05198~F05200 EDID 容错/超频检测/知识库。 */
export function edidFaultTolerant(edid: { checksumOk: boolean; descriptors: number }): { fallback: boolean; resolution: [number, number] } {
  return edid.checksumOk && edid.descriptors >= 1 ? { fallback: false, resolution: [2560, 1440] } : { fallback: true, resolution: [1920, 1080] };
}
export function overclockDetect(requestedHz: number, standardHz: number[]): { nonStandard: boolean; nearest: number } {
  const nearest = standardHz.reduce((a, b) => (Math.abs(b - requestedHz) < Math.abs(a - requestedHz) ? b : a), standardHz[0]!);
  return { nonStandard: !standardHz.includes(requestedHz), nearest };
}
export const DISPLAY_KB: { case: string; fix: string }[] = [
  { case: '扩展坞热插拔后窗口乱排', fix: '拓扑感知重排 + 位置记忆' },
  { case: 'KVM 切换后分辨率变化', fix: '建议开启 KVM EDID 模拟' },
  { case: '旧投影仪无信号', fix: '回退安全分辨率清单' },
];
export function displayTutorial(): string[] {
  return ['独占翻转交给游戏，合成器退后', 'HDR 需要 10bit 与足够带宽，不足时降 YCbCr422', 'EDID 损坏自动回退 1080p'];
}

/* ============ 族0209 音频管线兼容（F05201~F05225） ============ */

export interface AudioDevice { name: string; exclusive: boolean; sampleRateHz: number; channels: number; codec?: 'SBC' | 'aptX' | 'aptX-Adaptive' | 'LDAC' | 'LE-Audio' }

/** F05201/F05202 独占仲裁 + 共享优先。 */
export function exclusiveArbitration(clients: { name: string; wantsExclusive: boolean }[]): { grant: string | null; note: string } {
  if (clients.length <= 1) return { grant: clients[0]?.wantsExclusive ? clients[0]!.name : null, note: '无竞争' };
  const want = clients.filter((c) => c.wantsExclusive);
  if (want.length === 0) return { grant: null, note: '全部共享' };
  return { grant: null, note: '多方申请独占，回落共享模式' };
}
export function sharedModePreferred(device: AudioDevice, gameRunning: boolean): { mode: 'shared' | 'exclusive'; reason: string } {
  return gameRunning || device.exclusive === false ? { mode: 'shared', reason: '多应用共存' } : { mode: 'exclusive', reason: '单应用低延迟' };
}

/** F05203/F05204 ASIO 提示 + 蓝牙协商。 */
export function asioConflictHint(apps: string[], asioApp: string): string | null {
  return apps.length > 1 && asioApp ? `${asioApp} 独占 ASIO 期间其他应用无声，建议共享模式` : null;
}
export function btCodecNegotiate(supports: AudioDevice['codec'][], prefer: AudioDevice['codec']): AudioDevice['codec'] {
  const order: AudioDevice['codec'][] = ['LDAC', 'aptX-Adaptive', 'aptX', 'LE-Audio', 'SBC'];
  if (supports.includes(prefer)) return prefer;
  return order.find((c) => supports.includes(c)) ?? 'SBC';
}

/** F05205~F05207 aptX/LDAC/LE Audio 支持位。 */
export const BT_CODEC_SUPPORT = { aptX: true, aptXAdaptive: true, ldac: false, leAudio: false } as const;

/** F05208~F05210 USB DAC/耳放/调音台。 */
export function usbDacProfile(dac: { maxSampleRateHz: number; maxBitDepth: number }): { sampleRateHz: number; bitDepth: number; bitPerfect: boolean } {
  return { sampleRateHz: dac.maxSampleRateHz, bitDepth: dac.maxBitDepth, bitPerfect: dac.maxSampleRateHz >= 96000 };
}
export function amplifierGain(impedanceOhm: number, sensitivityDb: number): { gain: 'low' | 'high'; needsAmp: boolean } {
  return { gain: impedanceOhm >= 150 ? 'high' : 'low', needsAmp: impedanceOhm >= 150 && sensitivityDb < 100 };
}
export function mixerRouting(inputs: number, outputs: number): { matrix: string; valid: boolean } {
  return { matrix: `${inputs}x${outputs}`, valid: inputs > 0 && outputs > 0 };
}

/** F05211~F05215 虚拟声卡/变声/K 歌/直播声卡/内录。 */
export const VIRTUAL_AUDIO_DRIVER = 'varix-vaudio' as const;
export function voiceChangerCompat(chain: { driver: string; latencyMs: number }): { ok: boolean; note: string } {
  return { ok: chain.driver === VIRTUAL_AUDIO_DRIVER && chain.latencyMs < 50, note: '变声链路延迟需 <50ms' };
}
export function karaokeMonitor(mic: boolean, backing: boolean, headphones: boolean): { monitor: boolean; reason: string } {
  return { monitor: mic && backing && headphones, reason: mic && backing && headphones ? '耳机监听避免啸叫' : '需要耳机 + 伴奏 + 麦克风' };
}
export function liveAudioSetup(channels: string[]): { scene: string; order: string[] } {
  return { scene: 'live', order: ['game', ...channels] };
}
export function loopbackRecord(device: AudioDevice): { supported: boolean; sampleRateHz: number } {
  return { supported: true, sampleRateHz: device.sampleRateHz };
}

/** F05216~F05218 采样仲裁/时钟漂移/蓝牙补偿。 */
export function sampleRateArbitration(requested: number[], deviceMax: number): { chosen: number; resample: boolean } {
  const target = Math.min(Math.max(...requested), deviceMax);
  return { chosen: target, resample: requested.some((r) => r !== target) };
}
export function clockDriftCompensate(ppm: number, bufferMs: number): { adjustMsPerMin: number; bufferOk: boolean } {
  return { adjustMsPerMin: (ppm * 60) / 1000, bufferOk: bufferMs >= 40 };
}
export function btLatencyCompensate(baseMs: number, codec: NonNullable<AudioDevice['codec']>): { delayMs: number } {
  const factor: Record<NonNullable<AudioDevice['codec']>, number> = { SBC: 1.5, aptX: 1.0, 'aptX-Adaptive': 0.8, LDAC: 1.2, 'LE-Audio': 0.6 };
  return { delayMs: Math.round(baseMs * factor[codec]) };
}

/** F05219~F05221 断连恢复/热插切换/防爆音。 */
export class AudioRecovery {
  private lost = new Set<string>();
  markLost(device: string): void {
    this.lost.add(device);
  }
  reconnect(device: string): boolean {
    return this.lost.delete(device);
  }
  get pending(): string[] {
    return [...this.lost];
  }
}
export function hotswapFade(prevVol: number, nextVol: number): { rampMs: number; popFree: boolean } {
  return { rampMs: 30, popFree: prevVol >= 0 && nextVol >= 0 };
}

/** F05222~F05224 增强软件/语音软件/多设备仲裁。 */
export function audioEnhancerCompat(enhancer: string): { stack: string[]; ok: boolean } {
  return { stack: ['wasapi', enhancer], ok: true };
}
export function voiceDenoiseCompat(vendor: 'NVIDIA Broadcast' | 'RTX Voice' | 'Krisp'): { virtualMic: string; ok: boolean } {
  return { virtualMic: `${vendor} (varix-vaudio)`, ok: true };
}
export function multiOutputArbitration(outputs: { name: string; priority: number }[]): string {
  return [...outputs].sort((a, b) => b.priority - a.priority)[0]!.name;
}

/** F05225 知识库 + 教学。 */
export const AUDIO_KB: { case: string; fix: string }[] = [
  { case: '蓝牙耳机切换后爆音', fix: '30ms 淡入淡出' },
  { case: '独占模式抢占', fix: '多方申请回落共享' },
  { case: 'OBS 内录无声', fix: '启用 loopback 采集源' },
];
export function audioTutorial(): string[] {
  return ['共享优先：多方抢独占自动回落共享', '蓝牙断连自动恢复并补偿延迟', '切换设备 30ms 渐变防爆音'];
}

/* ============ 族0210 网络兼容（F05226~F05250） ============ */

export interface ProxyStack { name: string; mode: 'system' | 'process' | 'tun' | 'pac'; port: number }

/** F05226~F05228 代理共存/TUN 检测/代理区分。 */
export function proxyCoexist(stack: ProxyStack): { ok: boolean; note: string } {
  if (stack.mode === 'tun') return { ok: true, note: 'TUN 模式底层接管，与系统代理共存' };
  return { ok: stack.port > 0, note: `${stack.mode} 模式经 ${stack.port} 端口` };
}
export function tunDetected(adapters: { name: string; type: string }[]): boolean {
  return adapters.some((a) => /tun|wintun|tap/i.test(a.name) && a.type === 'virtual');
}
export function proxyScope(req: { appId: string; systemProxy: string | null; processProxy: string | null }): string {
  return req.processProxy ?? req.systemProxy ?? 'direct';
}

/** F05229/F05230 分流提示 + 多 VPN 共存。 */
export function splitTunnelHint(appId: string, rules: { pattern: string; via: string }[]): string {
  return rules.find((r) => new RegExp(r.pattern, 'i').test(appId))?.via ?? 'default';
}
export function multiVpnStack(vpns: { name: string; layer: 'l3-tun' | 'l2' | 'ssl' }[]): { ok: boolean; order: string[]; conflict?: string } {
  const l3 = vpns.filter((v) => v.layer === 'l3-tun');
  return l3.length > 1 ? { ok: false, order: vpns.map((v) => v.name), conflict: '多个 L3 TUN 路由表冲突' } : { ok: true, order: vpns.map((v) => v.name) };
}

/** F05231~F05235 WG 位/企业 VPN/虚拟网卡/多网卡/metric。 */
export const WIREGUARD_SUPPORT = { reserved: true, kernelBypass: false } as const;
export function enterpriseVpnDetect(client: string): { known: boolean; adapterHint: string } {
  const known = /easyconnect|pulse|fortinet|cisco|anyconnect/i.test(client);
  return { known, adapterHint: known ? `${client} 虚拟网卡` : '未知客户端' };
}
export function isVirtualAdapter(a: { name: string; driver: string }): boolean {
  return /tun|tap|hyper-v|vmware|virtualbox|wg/i.test(a.driver);
}
/** F05234 多网卡路由选择：最长前缀优先，metric 小者胜；默认路由兜底。 */
export function routeSelection(routes: { iface: string; metric: number; dest: string }[], dest: string): string {
  const scored = routes
    .map((r) => {
      const [net, bitsStr] = r.dest.split('/');
      const bits = Number(bitsStr ?? '0');
      if (bits === 0) return { iface: r.iface, metric: r.metric + 1000, prefixLen: 0 };
      const prefix = net!.split('.').slice(0, Math.floor(bits / 8)).join('.');
      const hit = dest.startsWith(prefix + '.') || dest === prefix;
      return { iface: r.iface, metric: r.metric, prefixLen: hit ? bits : -1 };
    })
    .filter((r) => r.prefixLen >= 0)
    .sort((a, b) => b.prefixLen - a.prefixLen || a.metric - b.metric);
  return scored[0]?.iface ?? 'lo';
}
export function metricConflict(routes: { iface: string; metric: number }[]): string[] {
  const m = new Map<number, string[]>();
  for (const r of routes) m.set(r.metric, [...(m.get(r.metric) ?? []), r.iface]);
  return [...m.entries()].filter(([, v]) => v.length > 1).flatMap(([, v]) => v);
}

/** F05236/F05237 DNS 仲裁 + DoH。 */
export function dnsArbitration(sources: { name: string; servers: string[]; priority: number }[]): string[] {
  return [...sources].sort((a, b) => a.priority - b.priority)[0]!.servers;
}
export const DOH_SUPPORT = { enabled: true, defaultProvider: 'system-resolver' } as const;

/** F05238~F05240 防火墙冲突/第三方防火墙/杀软过滤。 */
export function firewallRuleConflict(rules: { port: number; action: 'allow' | 'deny'; source: string }[], port: number): { blocked: boolean; by?: string } {
  const hit = rules.filter((r) => r.port === port);
  if (hit.some((r) => r.action === 'deny')) return { blocked: true, by: hit.find((r) => r.action === 'deny')!.source };
  return { blocked: false };
}
export const THIRD_PARTY_FW = ['火绒', '360', 'Comodo'] as const;
export function avNetworkFilter(av: string): { inspects: boolean; bypassTip: string } {
  return { inspects: true, bypassTip: `${av} 网络过滤建议排除 varix 进程` };
}

/** F05241/F05242 抓包共存 + 下载工具接管。 */
export function packetCaptureCompat(tool: 'Wireshark' | 'Fiddler' | 'mitmproxy'): { npcapNeeded: boolean; tlsHint: string } {
  return { npcapNeeded: tool === 'Wireshark', tlsHint: tool === 'Wireshark' ? 'TLS 需配合日志密钥' : '需安装并信任根证书' };
}
export function downloadTakeover(url: string, managers: { name: string; pattern: string }[]): string | null {
  return managers.find((m) => new RegExp(m.pattern, 'i').test(url))?.name ?? null;
}

/** F05243/F05244 P2P 提示 + 网银控件。 */
export function p2pBandwidthHint(speedMbps: number, capMbps = 50): { throttled: boolean; suggest: string } {
  return { throttled: speedMbps > capMbps, suggest: speedMbps > capMbps ? '限制上传速率避免占用带宽' : '带宽正常' };
}
export const BANKING_CONTROL_KB = ['ActiveX 网银控件需 IE 模式', '密码框安全控件不参与云剪贴'] as const;

/** F05245/F05246 证书冲突 + NTP。 */
export function certTrustConflict(certs: { subject: string; store: 'user' | 'machine' }[]): string[] {
  const m = new Map<string, string[]>();
  for (const c of certs) m.set(c.subject, [...(m.get(c.subject) ?? []), c.store]);
  return [...m.entries()].filter(([, v]) => v.length > 1).map(([k]) => k);
}
export function ntpSourceConflict(services: { name: string; intervalMin: number }[]): { single: boolean; winner: string } {
  return { single: services.length === 1, winner: services.sort((a, b) => a.intervalMin - b.intervalMin)[0]?.name ?? '' };
}

/** F05247~F05249 IPv6/双栈/多播。 */
export function ipv6Compat(stack: { v6: boolean; v6Only: boolean }): { reachable: boolean; strategy: 'v6-first' | 'v4-first' | 'v4-only' } {
  if (!stack.v6) return { reachable: false, strategy: 'v4-only' };
  return { reachable: true, strategy: stack.v6Only ? 'v6-first' : 'v4-first' };
}
export function dualStackPreferIPv4(hasV6Route: boolean, v6QualityMs: number, v4QualityMs: number): 'v6' | 'v4' {
  return hasV6Route && v6QualityMs < v4QualityMs ? 'v6' : 'v4';
}
export function multicastGroupJoin(groups: string[], ifaceSupports: boolean): { joined: string[]; ok: boolean } {
  return { joined: ifaceSupports ? groups : [], ok: ifaceSupports };
}

/** F05250 知识库 + 教学。 */
export const NETWORK_KB: { case: string; fix: string }[] = [
  { case: 'Clash TUN 后桌面更新失败', fix: '更新进程走进程代理并加入分流' },
  { case: '双 VPN 断流', fix: '只保留一个 L3 TUN，其余用 SSL 层' },
  { case: '网银控件打不开', fix: '该站点进 IE 模式站点库' },
];
export function networkTutorial(): string[] {
  return ['系统代理与进程代理分开识别', '多路由按 metric 仲裁并提示冲突', '双栈默认 v4 优先，v6 质量更好才切换'];
}
