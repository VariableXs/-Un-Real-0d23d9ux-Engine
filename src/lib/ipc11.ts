/**
 * AI-11 系统集成与硬件组 IPC（U-43..U-48 / N-19..N-25 / V-51..V-60）。
 *
 * 独立文件避免与其他 AI 会话并发编辑 ipc.ts 冲突；invoke 与 ipc.ts 同模式
 * （lazy import，vitest 纯逻辑测试不加载 @tauri-apps/api）。
 * 红线：本文件只封装 shell::sysprobe / shell::winpower 的已注册命令；
 * 写操作（proxy/gamma/优先级/电源计划）一律由 UI 显式确认后调用。
 */

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const mod = await import("@tauri-apps/api/core");
  return mod.invoke<T>(cmd, args);
}

// ---------- DTO（与 Rust serde rename_all = "camelCase" 对齐） ----------

export interface MonitorDto {
  device: string;
  x: number;
  y: number;
  w: number;
  h: number;
  primary: boolean;
}

export interface PortRow {
  proto: "tcp" | "udp" | string;
  local: string;
  remote: string;
  state: string;
  pid: number;
}

export interface EventRow {
  log: string;
  time: string;
  level: string;
  provider: string;
  id: number;
  message: string;
}

export interface FileHit {
  path: string;
  size: number;
  mtime: number;
}

export interface UptimeDto {
  sysSecs: number;
  envSecs: number;
}

export interface StartupProc {
  pid: number;
  name: string;
  bootOffsetMs: number;
  path: string;
}

export interface CheckItem {
  id: string;
  name: string;
  status: "ok" | "warn" | "fail" | string;
  detail: string;
}

export interface PowerLossReport {
  dirty: boolean;
  checks: CheckItem[];
  fixed: string[];
}

export interface PeriphProbe {
  audioDevices: string[];
  wlan: string;
  wlanNetworks: string[];
  btRadios: string[];
  inputDevices: string[];
}

export interface KeepAwakeState {
  on: boolean;
  display: boolean;
}

export interface PowerScheme {
  guid: string;
  name: string;
  active: boolean;
}

export interface BatteryHealth {
  designMwh: number | null;
  fullMwh: number | null;
  cycleCount: number | null;
  wearPct: number | null;
}

export interface ProxyState {
  enabled: boolean;
  server: string;
  overrideList: string;
  autoConfigUrl: string;
}

export interface PingResult {
  host: string;
  avgMs: number | null;
  lostPct: number | null;
  rawExcerpt: string;
}

// ---------- 命令封装 ----------

export const ipc11 = {
  // sysprobe（只读探针 + 白名单修复）
  monitorList: () => invoke<MonitorDto[]>("monitor_list"),
  portTable: () => invoke<PortRow[]>("port_table"),
  eventlogRecent: (log: string, count: number) =>
    invoke<EventRow[]>("eventlog_recent", { log, count }),
  bigfileScan: (root: string, minMb: number, days: number) =>
    invoke<FileHit[]>("bigfile_scan", { root, minMb, days }),
  sysUptime: () => invoke<UptimeDto>("sys_uptime"),
  startupProcs: () => invoke<StartupProc[]>("startup_procs"),
  selfhealChecks: () => invoke<CheckItem[]>("selfheal_checks"),
  healRun: (id: string) => invoke<string>("heal_run", { id }),
  pwrlossCheck: () => invoke<PowerLossReport>("pwrloss_check"),
  predwarm: (path: string) => invoke<boolean>("predwarm", { path }),
  periphProbe: () => invoke<PeriphProbe>("periph_probe"),

  // winpower（电源/色彩/优先级/代理/测速）
  keepawakeSet: (on: boolean, display: boolean) =>
    invoke<KeepAwakeState>("keepawake_set", { on, display }),
  keepawakeGet: () => invoke<KeepAwakeState>("keepawake_get"),
  powerSchemesList: () => invoke<PowerScheme[]>("power_schemes_list"),
  powerSchemeSet: (guid: string) => invoke<void>("power_scheme_set", { guid }),
  batteryHealth: () => invoke<BatteryHealth>("battery_health"),
  gammaSet: (kelvin: number) => invoke<void>("gamma_set", { kelvin }),
  gammaRestore: () => invoke<void>("gamma_restore"),
  procPrioritySet: (pid: number, klass: string) =>
    invoke<void>("proc_priority_set", { pid, class: klass }),
  proxyGet: () => invoke<ProxyState>("proxy_get"),
  proxySet: (enabled: boolean, server: string) =>
    invoke<ProxyState>("proxy_set", { enabled, server }),
  netPing: (host: string) => invoke<PingResult>("net_ping", { host }),
};
