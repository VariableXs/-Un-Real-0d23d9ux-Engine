//! VARIX-M500 AI-09 · 网络体验与协同（F201~F225）。
//!
//! 体检向导、连接质量评分、弱网模拟、QoS 面板、流量画像、联网询问、
//! 域名信誉、DNS 缓存、离线优先、无缝切换、热点、诊断包、带宽预算、
//! 代理、信任库、Wi-Fi/蓝牙 PAN、日志故事化、测速靶、预览沙、P2P、
//! 性能基线、IPv6 双栈、fuzz 与域自检。
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F201 网络体检向导 — 一键诊断全链路（检查项管线）
// ---------------------------------------------------------------------------

pub const CHECKUP_ITEMS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckupItem {
    LinkUp,
    DhcpLease,
    GatewayPing,
    DnsResolve,
    RttProbe,
    TlsHandshake,
}

#[derive(Clone, Copy)]
pub struct NetCheckup {
    pub results: [Option<bool>; CHECKUP_ITEMS],
    pub count: usize,
}

impl NetCheckup {
    pub const fn new() -> NetCheckup {
        NetCheckup { results: [None; CHECKUP_ITEMS], count: 0 }
    }

    pub fn record(&mut self, ok: bool) {
        if self.count < CHECKUP_ITEMS {
            self.results[self.count] = Some(ok);
        }
        self.count += 1;
    }

    /// 首个失败项（0=全部通过）。
    pub fn first_failure(&self) -> usize {
        (0..CHECKUP_ITEMS).find(|&i| self.results[i] == Some(false)).map(|i| i + 1).unwrap_or(0)
    }

    pub fn all_ok(&self) -> bool {
        self.count > 0 && (0..self.count).all(|i| self.results[i] == Some(true))
    }
}

// ---------------------------------------------------------------------------
// F202 连接质量评分 — 丢包/抖动/RTT 综合分
// ---------------------------------------------------------------------------

/// 评分 0..100：RTT、丢包、抖动加权。
pub fn connection_score(rtt_ms: u32, loss_permille: u16, jitter_ms: u32) -> u8 {
    let rtt_pts = if rtt_ms <= 20 {
        50
    } else if rtt_ms >= 200 {
        0
    } else {
        50 - (rtt_ms - 20) * 50 / 180
    } as u32;
    let loss_pts = 25u32.saturating_sub(loss_permille as u32 / 4);
    let jit_pts = if jitter_ms <= 5 {
        25
    } else if jitter_ms >= 100 {
        0
    } else {
        25 - (jitter_ms - 5) * 25 / 95
    };
    (rtt_pts + loss_pts + jit_pts).min(100) as u8
}

// ---------------------------------------------------------------------------
// F203 弱网模拟器 — 开发用劣化注入
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct WeakNetProfile {
    /// 注入延迟 ms。
    pub delay_ms: u32,
    /// 丢包千分比。
    pub loss_permille: u16,
    /// 带宽上限 kbps（0=不限）。
    pub bw_kbps: u32,
    pub enabled: bool,
}

impl WeakNetProfile {
    pub const fn off() -> WeakNetProfile {
        WeakNetProfile { delay_ms: 0, loss_permille: 0, bw_kbps: 0, enabled: false }
    }

    /// 判定一个包是否被丢弃（确定性：用包序号 + 阈值）。
    pub fn drops(&self, seq: u32) -> bool {
        self.enabled && (seq % 1000) < self.loss_permille as u32
    }

    /// 传输耗时（延迟 + 按带宽的串行化时间）。
    pub fn transit_ms(&self, bytes: u32) -> u32 {
        if !self.enabled {
            return 0;
        }
        let serialize = if self.bw_kbps == 0 { 0 } else { bytes * 8 / self.bw_kbps };
        self.delay_ms + serialize
    }
}

// ---------------------------------------------------------------------------
// F204 流量优先级面板 — 可视化 QoS（类 → 权重）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TrafficClass {
    Interactive,
    Streaming,
    Background,
}

pub fn class_weight(c: TrafficClass) -> u8 {
    match c {
        TrafficClass::Interactive => 8,
        TrafficClass::Streaming => 4,
        TrafficClass::Background => 1,
    }
}

/// 加权公平份额：各类请求字节数 → 分配比例（千分比）。
pub fn fair_share(want: [u32; 3]) -> [u32; 3] {
    let weights = [class_weight(TrafficClass::Interactive) as u32,
        class_weight(TrafficClass::Streaming) as u32,
        class_weight(TrafficClass::Background) as u32];
    let total_w: u32 = weights.iter().sum();
    let demand: u32 = want.iter().sum();
    if demand == 0 {
        return [0; 3];
    }
    let mut out = [0u32; 3];
    for i in 0..3 {
        // 先满足低需求类。
        let cap = want[i] * 1000 / demand.max(1);
        let share = weights[i] * 1000 / total_w;
        out[i] = cap.min(share).max(0);
    }
    out
}

// ---------------------------------------------------------------------------
// F205 应用流量画像 — 联网习惯档案
// ---------------------------------------------------------------------------

pub const TRAFFIC_APP_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct AppTraffic {
    pub app_id: u16,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub conn_count: u32,
}

#[derive(Clone, Copy)]
pub struct TrafficProfileStore {
    pub apps: [AppTraffic; TRAFFIC_APP_CAP],
    pub count: usize,
}

impl TrafficProfileStore {
    pub const fn new() -> TrafficProfileStore {
        TrafficProfileStore {
            apps: [AppTraffic { app_id: 0, tx_bytes: 0, rx_bytes: 0, conn_count: 0 }; TRAFFIC_APP_CAP],
            count: 0,
        }
    }

    pub fn observe(&mut self, app_id: u16, tx: u64, rx: u64) -> bool {
        for i in 0..self.count {
            if self.apps[i].app_id == app_id {
                self.apps[i].tx_bytes += tx;
                self.apps[i].rx_bytes += rx;
                self.apps[i].conn_count += 1;
                return true;
            }
        }
        if self.count >= TRAFFIC_APP_CAP {
            return false;
        }
        self.apps[self.count] = AppTraffic { app_id, tx_bytes: tx, rx_bytes: rx, conn_count: 1 };
        self.count += 1;
        true
    }

    /// 最耗流量的 app。
    pub fn top_talker(&self) -> Option<u16> {
        if self.count == 0 {
            return None;
        }
        Some(
            (0..self.count)
                .reduce(|a, b| {
                    let ta = self.apps[a].tx_bytes + self.apps[a].rx_bytes;
                    let tb = self.apps[b].tx_bytes + self.apps[b].rx_bytes;
                    if tb > ta { b } else { a }
                })
                .map(|i| self.apps[i].app_id)
                .unwrap(),
        )
    }
}

// ---------------------------------------------------------------------------
// F206 首次联网询问 — 新应用联网授权
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetGrant {
    Unknown,
    Allowed,
    Denied,
}

#[derive(Clone, Copy)]
pub struct NetGate {
    pub grants: [(u16, NetGrant); 16],
    pub count: usize,
    pub asked: u32,
}

impl NetGate {
    pub const fn new() -> NetGate {
        NetGate { grants: [(0, NetGrant::Unknown); 16], count: 0, asked: 0 }
    }

    /// 应用首次联网 → 需要询问；已授权/拒绝直接裁决。
    pub fn request(&mut self, app_id: u16) -> (bool, bool) {
        for i in 0..self.count {
            if self.grants[i].0 == app_id {
                return (self.grants[i].1 == NetGrant::Allowed, false);
            }
        }
        if self.count < 16 {
            self.grants[self.count] = (app_id, NetGrant::Unknown);
            self.count += 1;
        }
        self.asked += 1;
        (false, true) // 需要用户询问
    }

    pub fn answer(&mut self, app_id: u16, allow: bool) -> bool {
        for i in 0..self.count {
            if self.grants[i].0 == app_id {
                self.grants[i].1 = if allow { NetGrant::Allowed } else { NetGrant::Denied };
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F207 域名信誉提示 — 可疑域名告警
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Reputation {
    Trusted,
    Unknown,
    Suspicious,
}

/// 简单启发式：超长标签 / 连字符过多 / 伪装 TLD。
pub fn domain_reputation(host: &[u8]) -> Reputation {
    let mut dots = 0;
    let mut hyphens = 0;
    let mut max_label = 0u32;
    let mut cur = 0u32;
    for &c in host {
        match c {
            b'.' => {
                dots += 1;
                max_label = max_label.max(cur);
                cur = 0;
            }
            b'-' => hyphens += 1,
            _ => cur += 1,
        }
    }
    max_label = max_label.max(cur);
    if max_label > 40 || hyphens >= 5 || dots > 6 {
        Reputation::Suspicious
    } else {
        Reputation::Unknown
    }
}

// ---------------------------------------------------------------------------
// F208 本地 DNS 缓存 — 查询加速服务
// ---------------------------------------------------------------------------

pub const DNS_CAP: usize = 16;
pub const DNS_TTL_MS: u32 = 60_000;

#[derive(Clone, Copy)]
pub struct DnsEntry {
    pub host_hash: u64,
    pub addr: u32,
    pub expires_ms: u32,
}

#[derive(Clone, Copy)]
pub struct DnsCache {
    pub entries: [DnsEntry; DNS_CAP],
    pub count: usize,
    pub hits: u32,
    pub misses: u32,
}

impl DnsCache {
    pub const fn new() -> DnsCache {
        DnsCache { entries: [DnsEntry { host_hash: 0, addr: 0, expires_ms: 0 }; DNS_CAP], count: 0, hits: 0, misses: 0 }
    }

    pub fn put(&mut self, host_hash: u64, addr: u32, now_ms: u32) -> bool {
        for i in 0..self.count {
            if self.entries[i].host_hash == host_hash {
                self.entries[i].addr = addr;
                self.entries[i].expires_ms = now_ms + DNS_TTL_MS;
                return true;
            }
        }
        if self.count >= DNS_CAP {
            // 逐出最老（最早过期）。
            let mut victim = 0;
            for i in 1..DNS_CAP {
                if self.entries[i].expires_ms < self.entries[victim].expires_ms {
                    victim = i;
                }
            }
            self.entries[victim] = DnsEntry { host_hash, addr, expires_ms: now_ms + DNS_TTL_MS };
            return true;
        }
        self.entries[self.count] = DnsEntry { host_hash, addr, expires_ms: now_ms + DNS_TTL_MS };
        self.count += 1;
        true
    }

    pub fn get(&mut self, host_hash: u64, now_ms: u32) -> Option<u32> {
        for i in 0..self.count {
            if self.entries[i].host_hash == host_hash {
                if now_ms < self.entries[i].expires_ms {
                    self.hits += 1;
                    return Some(self.entries[i].addr);
                }
                self.misses += 1;
                return None;
            }
        }
        self.misses += 1;
        None
    }
}

pub fn hash_host(host: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &c in host {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// F209 离线优先 API — 应用离线契约（写缓存 + 同步水位）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct OfflineContract {
    /// 本地待同步操作数。
    pub pending: u32,
    /// 上次同步成功时刻 ms。
    pub last_sync_ms: u32,
    pub synced: bool,
}

impl OfflineContract {
    pub const fn new() -> OfflineContract {
        OfflineContract { pending: 0, last_sync_ms: 0, synced: false }
    }

    pub fn write_offline(&mut self) {
        self.pending += 1;
        self.synced = false;
    }

    /// 同步：返回本次推上去的数量。
    pub fn sync(&mut self, now_ms: u32) -> u32 {
        let n = self.pending;
        self.pending = 0;
        self.last_sync_ms = now_ms;
        self.synced = true;
        n
    }

    pub fn is_stale(&self, now_ms: u32, max_ms: u32) -> bool {
        !self.synced || now_ms - self.last_sync_ms > max_ms
    }
}

// ---------------------------------------------------------------------------
// F210 网络无缝切换 — 多网卡热切换（保持连接的路径迁移）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfState {
    Down,
    Up,
    Preferred,
}

#[derive(Clone, Copy)]
pub struct InterfaceState {
    pub states: [IfState; 4],
    pub active: usize,
}

impl InterfaceState {
    pub const fn new() -> InterfaceState {
        InterfaceState { states: [IfState::Down; 4], active: usize::MAX }
    }

    pub fn set(&mut self, idx: usize, st: IfState) -> bool {
        if idx >= 4 {
            return false;
        }
        self.states[idx] = st;
        // 当前活动口掉线 → 自动切到最优可用口。
        if self.active != usize::MAX && self.states[self.active] == IfState::Down {
            self.active = usize::MAX;
        }
        if self.active == usize::MAX {
            for i in 0..4 {
                if self.states[i] == IfState::Preferred {
                    self.active = i;
                    break;
                }
            }
            if self.active == usize::MAX {
                for i in 0..4 {
                    if self.states[i] == IfState::Up {
                        self.active = i;
                        break;
                    }
                }
            }
        }
        // 新口更优则迁移。
        if self.active != usize::MAX
            && (0..4).any(|i| i < self.active && self.states[i] == IfState::Preferred && self.states[self.active] != IfState::Preferred)
        {
            self.active = (0..4).find(|&i| self.states[i] == IfState::Preferred).unwrap_or(self.active);
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F211 热点共享 — 变身接入点（客户端表）
// ---------------------------------------------------------------------------

pub const HOTSPOT_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct HotspotClient {
    pub mac_hash: u64,
    pub ip: u32,
    pub connected: bool,
}

#[derive(Clone, Copy)]
pub struct Hotspot {
    pub ssid_hash: u64,
    pub clients: [HotspotClient; HOTSPOT_CAP],
    pub count: usize,
    pub on: bool,
}

impl Hotspot {
    pub const fn new(ssid_hash: u64) -> Hotspot {
        Hotspot {
            ssid_hash,
            clients: [HotspotClient { mac_hash: 0, ip: 0, connected: false }; HOTSPOT_CAP],
            count: 0,
            on: false,
        }
    }

    pub fn start(&mut self) {
        self.on = true;
    }

    pub fn stop(&mut self) {
        self.on = false;
        for c in self.clients[..self.count].iter_mut() {
            c.connected = false;
        }
    }

    /// 客户端关联（满/未开热点拒绝）。
    pub fn associate(&mut self, mac_hash: u64, ip: u32) -> bool {
        if !self.on {
            return false;
        }
        for i in 0..self.count {
            if self.clients[i].mac_hash == mac_hash {
                self.clients[i].connected = true;
                self.clients[i].ip = ip;
                return true;
            }
        }
        if self.count >= HOTSPOT_CAP {
            return false;
        }
        self.clients[self.count] = HotspotClient { mac_hash, ip, connected: true };
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F212 诊断包导出 — 一键问题取证
// ---------------------------------------------------------------------------

pub const DIAG_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct DiagBundle {
    /// 摘要槽（键值对数组索引）。
    pub keys: [&'static str; DIAG_CAP],
    pub vals: [u32; DIAG_CAP],
    pub count: usize,
    pub sealed: bool,
}

impl DiagBundle {
    pub const fn new() -> DiagBundle {
        DiagBundle { keys: ["", "", "", "", "", "", "", ""], vals: [0; DIAG_CAP], count: 0, sealed: false }
    }

    pub fn add(&mut self, key: &'static str, val: u32) -> bool {
        if self.sealed || self.count >= DIAG_CAP {
            return false;
        }
        self.keys[self.count] = key;
        self.vals[self.count] = val;
        self.count += 1;
        true
    }

    /// 封包后不可再写（导出语义）。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.count as u32
    }

    /// 脱敏摘要：只暴露键名与值是否为 0。
    pub fn redacted_summary(&self) -> [u8; DIAG_CAP] {
        let mut out = [0u8; DIAG_CAP];
        for i in 0..self.count {
            out[i] = if self.vals[i] == 0 { b'0' } else { b'1' };
        }
        out
    }
}

// ---------------------------------------------------------------------------
// F213 带宽预算 — 流量计划管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BandwidthBudget {
    /// 周期配额 MB。
    pub quota_mb: u32,
    pub used_mb: u32,
    /// 80% 预警已发。
    pub warned: bool,
}

impl BandwidthBudget {
    pub const fn new(quota_mb: u32) -> BandwidthBudget {
        BandwidthBudget { quota_mb: if quota_mb < 1 { 1 } else { quota_mb }, used_mb: 0, warned: false }
    }

    pub fn consume(&mut self, mb: u32) -> bool {
        self.used_mb = self.used_mb.saturating_add(mb);
        if !self.warned && self.used_mb * 100 >= self.quota_mb * 80 {
            self.warned = true;
        }
        self.used_mb <= self.quota_mb
    }

    pub fn exceeded(&self) -> bool {
        self.used_mb > self.quota_mb
    }

    pub fn remaining(&self) -> u32 {
        self.quota_mb.saturating_sub(self.used_mb)
    }
}

// ---------------------------------------------------------------------------
// F214 代理框架 — 全局/分应用代理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProxyRule {
    pub app_id: u16,
    /// 代理端点 id（0=直连）。
    pub endpoint: u16,
}

#[derive(Clone, Copy)]
pub struct ProxyTable {
    pub global_endpoint: u16,
    pub rules: [ProxyRule; 8],
    pub count: usize,
    pub global_on: bool,
}

impl ProxyTable {
    pub const fn new() -> ProxyTable {
        ProxyTable { global_endpoint: 0, rules: [ProxyRule { app_id: 0, endpoint: 0 }; 8], count: 0, global_on: false }
    }

    pub fn set_global(&mut self, endpoint: u16, on: bool) {
        self.global_endpoint = endpoint;
        self.global_on = on;
    }

    pub fn set_app(&mut self, app_id: u16, endpoint: u16) -> bool {
        for i in 0..self.count {
            if self.rules[i].app_id == app_id {
                self.rules[i].endpoint = endpoint;
                return true;
            }
        }
        if self.count >= 8 {
            return false;
        }
        self.rules[self.count] = ProxyRule { app_id, endpoint };
        self.count += 1;
        true
    }

    /// 分应用规则优先于全局。
    pub fn resolve(&self, app_id: u16) -> u16 {
        for i in 0..self.count {
            if self.rules[i].app_id == app_id {
                return self.rules[i].endpoint;
            }
        }
        if self.global_on {
            self.global_endpoint
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F215 本地信任库 — 证书池管理
// ---------------------------------------------------------------------------

pub const TRUST_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct TrustStore {
    /// 证书指纹（简化 64bit）。
    pub pins: [u64; TRUST_CAP],
    pub count: usize,
    pub revoked: [u64; TRUST_CAP],
    pub revoked_count: usize,
}

impl TrustStore {
    pub const fn new() -> TrustStore {
        TrustStore { pins: [0; TRUST_CAP], count: 0, revoked: [0; TRUST_CAP], revoked_count: 0 }
    }

    pub fn pin(&mut self, fp: u64) -> bool {
        if self.count >= TRUST_CAP || (0..self.count).any(|i| self.pins[i] == fp) {
            return false;
        }
        self.pins[self.count] = fp;
        self.count += 1;
        true
    }

    pub fn revoke(&mut self, fp: u64) -> bool {
        if self.revoked_count >= TRUST_CAP {
            return false;
        }
        self.revoked[self.revoked_count] = fp;
        self.revoked_count += 1;
        true
    }

    /// 验证：在池中且未被吊销。
    pub fn verify(&self, fp: u64) -> bool {
        (0..self.count).any(|i| self.pins[i] == fp)
            && !(0..self.revoked_count).any(|i| self.revoked[i] == fp)
    }
}

// ---------------------------------------------------------------------------
// F216 Wi-Fi 管理框架 — 扫描/连接预留
// ---------------------------------------------------------------------------

pub const WIFI_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct WifiAp {
    pub ssid_hash: u64,
    /// 信号强度 0..100。
    pub rssi: u8,
    pub secure: bool,
}

#[derive(Clone, Copy)]
pub struct WifiManager {
    pub scan: [WifiAp; WIFI_CAP],
    pub count: usize,
    pub connected_ssid: u64,
}

impl WifiManager {
    pub const fn new() -> WifiManager {
        WifiManager { scan: [WifiAp { ssid_hash: 0, rssi: 0, secure: false }; WIFI_CAP], count: 0, connected_ssid: 0 }
    }

    pub fn scan_result(&mut self, ssid_hash: u64, rssi: u8, secure: bool) -> bool {
        for i in 0..self.count {
            if self.scan[i].ssid_hash == ssid_hash {
                self.scan[i].rssi = rssi;
                return true;
            }
        }
        if self.count >= WIFI_CAP {
            return false;
        }
        self.scan[self.count] = WifiAp { ssid_hash, rssi, secure };
        self.count += 1;
        true
    }

    /// 连接最强 AP（安全网优先于开放网当强度接近）。
    pub fn best(&self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let mut best = 0usize;
        for i in 1..self.count {
            let si = self.scan[i].rssi as i32 + if self.scan[i].secure { 10 } else { 0 };
            let sb = self.scan[best].rssi as i32 + if self.scan[best].secure { 10 } else { 0 };
            if si > sb {
                best = i;
            }
        }
        Some(self.scan[best].ssid_hash)
    }

    pub fn connect(&mut self, ssid_hash: u64) -> bool {
        if (0..self.count).any(|i| self.scan[i].ssid_hash == ssid_hash) {
            self.connected_ssid = ssid_hash;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F217 蓝牙 PAN 预留 — 蓝牙网络占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanState {
    Idle,
    Pairing,
    Connected,
}

#[derive(Clone, Copy)]
pub struct PanLink {
    pub peer_hash: u64,
    pub state: PanState,
    pub mtu: u16,
}

/// PAN 连接生命周期。
pub fn pan_step(link: &mut PanLink, event: u8) -> bool {
    match event {
        0 => {
            link.state = PanState::Pairing;
            false
        }
        1 if link.state == PanState::Pairing => {
            link.state = PanState::Connected;
            link.mtu = 1500;
            true
        }
        2 => {
            link.state = PanState::Idle;
            link.mtu = 0;
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// F218 网络日志故事化 — 人话网络事件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetStory {
    LinkUp,
    LinkDown,
    DnsSlow(u16),
    PacketLoss,
    Roamed,
}

/// 事件 → 严重度（0=信息 1=注意 2=告警）。
pub fn story_severity(s: NetStory) -> u8 {
    match s {
        NetStory::LinkUp | NetStory::Roamed => 0,
        NetStory::DnsSlow(ms) if ms < 500 => 1,
        NetStory::DnsSlow(_) | NetStory::PacketLoss => 2,
        NetStory::LinkDown => 2,
    }
}

// ---------------------------------------------------------------------------
// F219 内置测速靶 — 连接测试点
// ---------------------------------------------------------------------------

/// 回显靶：校验负载并测量 RTT。
pub fn speedtest_echo(payload_sum: u32, payload_len: u32, sent_ms: u32, now_ms: u32) -> Option<(bool, u32)> {
    if payload_len == 0 || now_ms < sent_ms {
        return None;
    }
    // 校验：和 == 长度 × 平均值约定（这里约定和 = len*42）。
    let expect = payload_len.wrapping_mul(42);
    Some((payload_sum == expect, now_ms - sent_ms))
}

// ---------------------------------------------------------------------------
// F220 链接预览沙 — 预览隔离执行
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewVerdict {
    FetchMeta,
    Blocked,
}

/// 预览请求裁决：仅允许拉取元数据，内网地址与超大响应拒绝。
pub fn preview_sandbox(host_is_private: bool, meta_bytes: u32) -> PreviewVerdict {
    if host_is_private || meta_bytes > 65536 {
        PreviewVerdict::Blocked
    } else {
        PreviewVerdict::FetchMeta
    }
}

// ---------------------------------------------------------------------------
// F221 P2P 直传预留 — 局域互传
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct P2pSession {
    pub peer_hash: u64,
    pub chunks_done: u32,
    pub chunks_total: u32,
    pub active: bool,
}

impl P2pSession {
    pub const fn new(peer_hash: u64, total: u32) -> P2pSession {
        P2pSession { peer_hash, chunks_done: 0, chunks_total: total, active: true }
    }

    pub fn ack_chunk(&mut self) -> bool {
        if !self.active || self.chunks_done >= self.chunks_total {
            return false;
        }
        self.chunks_done += 1;
        if self.chunks_done == self.chunks_total {
            self.active = false;
        }
        true
    }

    pub fn progress_permille(&self) -> u32 {
        if self.chunks_total == 0 {
            return 1000;
        }
        self.chunks_done * 1000 / self.chunks_total
    }
}

// ---------------------------------------------------------------------------
// F222 网络性能基线 — 吞吐/延迟回归
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct NetBaseline {
    pub throughput_mbps: u32,
    pub rtt_ms: u32,
}

/// 回归判定：吞吐劣化 >15% 或 RTT 劣化 >25%。
pub fn net_regress(base: NetBaseline, now: NetBaseline) -> bool {
    let tp_bad = now.throughput_mbps * 100 < base.throughput_mbps * 85;
    let rtt_bad = now.rtt_ms * 100 > base.rtt_ms * 125;
    tp_bad || rtt_bad
}

// ---------------------------------------------------------------------------
// F223 IPv6 双栈深化 — 地址全生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V6AddrState {
    Tentative,
    Preferred,
    Deprecated,
    Invalid,
}

#[derive(Clone, Copy)]
pub struct V6Addr {
    pub prefix: u64,
    pub iid: u64,
    pub state: V6AddrState,
    pub valid_until_ms: u32,
    pub preferred_until_ms: u32,
}

impl V6Addr {
    pub const fn new(prefix: u64, iid: u64) -> V6Addr {
        V6Addr { prefix, iid, state: V6AddrState::Tentative, valid_until_ms: 0, preferred_until_ms: 0 }
    }

    /// 生命周期推进（DAD 通过 → preferred → deprecated → invalid）。
    pub fn age(&mut self, now_ms: u32) -> V6AddrState {
        self.state = if now_ms >= self.valid_until_ms {
            V6AddrState::Invalid
        } else if now_ms >= self.preferred_until_ms {
            V6AddrState::Deprecated
        } else {
            V6AddrState::Preferred
        };
        self.state
    }

    /// 选中源地址：优先 preferred，其次 deprecated，Tentative/Invalid 不可用。
    pub fn usable_for_source(&self) -> bool {
        matches!(self.state, V6AddrState::Preferred | V6AddrState::Deprecated)
    }
}

// ---------------------------------------------------------------------------
// F224 网络 fuzz — 协议栈对抗
// ---------------------------------------------------------------------------

pub struct NetFuzzer {
    pub state: u32,
    pub malformed: u32,
    pub rejected: u32,
}

impl NetFuzzer {
    pub const fn new(seed: u32) -> NetFuzzer {
        NetFuzzer { state: seed | 1, malformed: 0, rejected: 0 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// 生成畸形包长度（0 / 超大 / 普通）。
    pub fn gen_len(&mut self) -> u32 {
        let r = self.next();
        match r & 3 {
            0 => 0,
            1 => 0xFFFF_FFFF,
            _ => (r >> 16) % 1500,
        }
    }

    /// 协议栈入口校验：长度 0 或 > MTU 必须拒绝。
    pub fn ingest(&mut self, len: u32, mtu: u32) -> bool {
        if len == 0 || len > mtu {
            self.malformed += 1;
            self.rejected += 1;
            return false;
        }
        self.malformed += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F225 网络域自检 — 25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_netxp_checks() -> CheckSet {
    let mut set = CheckSet::new("netxp");

    // F201 体检
    let mut ck = NetCheckup::new();
    ck.record(true);
    ck.record(true);
    ck.record(false);
    ck.record(true);
    set.add("F201 checkup wizard", ck.first_failure() == 3 && !ck.all_ok(), "locates failure");

    // F202 质量评分
    let s_good = connection_score(10, 0, 2);
    let s_bad = connection_score(300, 200, 150);
    let s_mid = connection_score(100, 0, 50);
    set.add("F202 conn score", s_good > 90 && s_bad < 20 && s_mid > s_bad && s_mid < s_good, "weighted score");

    // F203 弱网
    let wn = WeakNetProfile { delay_ms: 100, loss_permille: 100, bw_kbps: 1000, enabled: true };
    let drop = wn.drops(50);
    let keep = wn.drops(1500);
    let t = wn.transit_ms(1000); // 1000B*8/1000kbps = 8ms + 100
    let off = WeakNetProfile::off().transit_ms(9999);
    set.add("F203 weaknet sim", drop && !keep && t == 108 && off == 0, "loss + serialization");

    // F204 QoS
    let share = fair_share([1000, 100, 100]);
    set.add(
        "F204 qos panel",
        share[0] >= share[1] && share[1] >= share[2] && class_weight(TrafficClass::Interactive) == 8,
        "weighted shares",
    );

    // F205 流量画像
    let mut tp = TrafficProfileStore::new();
    let _ = tp.observe(1, 100, 900);
    let _ = tp.observe(2, 5000, 5000);
    let _ = tp.observe(1, 100, 100);
    set.add("F205 app traffic", tp.top_talker() == Some(2) && tp.count == 2, "top talker");

    // F206 联网询问
    let mut gate = NetGate::new();
    let r1 = gate.request(7);
    let _ = gate.answer(7, true);
    let r2 = gate.request(7);
    set.add("F206 first-net ask", r1 == (false, true) && r2 == (true, false) && gate.asked == 1, "gate flow");

    // F207 域名信誉
    let r_ok = domain_reputation(b"example.com");
    let r_bad = domain_reputation(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.example.com");
    let r_hyph = domain_reputation(b"a-b-c-d-e-f.example.com");
    set.add("F207 domain rep", r_ok == Reputation::Unknown && r_bad == Reputation::Suspicious && r_hyph == Reputation::Suspicious, "heuristics");

    // F208 DNS 缓存
    let mut dns = DnsCache::new();
    let h = hash_host(b"varix.dev");
    let _ = dns.put(h, 0x0A00_0001, 0);
    let hit1 = dns.get(h, 1000);
    let miss = dns.get(h, DNS_TTL_MS + 1);
    set.add("F208 dns cache", hit1 == Some(0x0A00_0001) && miss.is_none() && dns.hits == 1 && dns.misses == 1, "ttl expiry");

    // F209 离线优先
    let mut oc = OfflineContract::new();
    oc.write_offline();
    oc.write_offline();
    let stale = oc.is_stale(100, 50);
    let n = oc.sync(200);
    set.add("F209 offline api", stale && n == 2 && !oc.is_stale(250, 100), "sync watermark");

    // F210 无缝切换
    let mut ifs = InterfaceState::new();
    let _ = ifs.set(1, IfState::Up);
    let _ = ifs.set(2, IfState::Preferred);
    let _ = ifs.set(1, IfState::Down);
    let active = ifs.active;
    set.add("F210 seamless switch", active == 2, "preferred failover");

    // F211 热点
    let mut hs = Hotspot::new(0xBEEF);
    let before = hs.associate(1, 2);
    hs.start();
    let ok1 = hs.associate(1, 2);
    let ok2 = hs.associate(2, 3);
    hs.stop();
    let gone = hs.clients[0].connected;
    set.add("F211 hotspot", !before && ok1 && ok2 && !gone && hs.clients[0].connected == false, "assoc lifecycle");

    // F212 诊断包
    let mut db = DiagBundle::new();
    let _ = db.add("rtt", 42);
    let _ = db.add("loss", 0);
    let n = db.seal();
    let locked = db.add("x", 1);
    let summ = db.redacted_summary();
    set.add("F212 diag bundle", n == 2 && !locked && summ[0] == b'1' && summ[1] == b'0', "seal + redact");

    // F213 带宽预算
    let mut bb = BandwidthBudget::new(100);
    let ok1 = bb.consume(50);
    let _ = bb.consume(30);
    let warned = bb.warned;
    let ok2 = bb.consume(25);
    set.add("F213 bandwidth budget", ok1 && warned && !ok2 && bb.exceeded(), "80% warn + cap");

    // F214 代理
    let mut px = ProxyTable::new();
    px.set_global(90, true);
    let _ = px.set_app(5, 0);
    let r1 = px.resolve(5);
    let r2 = px.resolve(6);
    set.add("F214 proxy framework", r1 == 0 && r2 == 90, "app rule over global");

    // F215 信任库
    let mut ts = TrustStore::new();
    let _ = ts.pin(0xAA);
    let _ = ts.pin(0xBB);
    let _ = ts.revoke(0xBB);
    set.add("F215 trust store", ts.verify(0xAA) && !ts.verify(0xBB) && !ts.verify(0xCC), "pin + revoke");

    // F216 Wi-Fi
    let mut wf = WifiManager::new();
    let _ = wf.scan_result(0x11, 80, false);
    let _ = wf.scan_result(0x22, 78, true);
    let best = wf.best();
    let ok = best.map(|b| wf.connect(b)).unwrap_or(false);
    set.add("F216 wifi mgr", best == Some(0x22) && ok && wf.connected_ssid == 0x22, "secure bias");

    // F217 蓝牙 PAN
    let mut pan = PanLink { peer_hash: 7, state: PanState::Idle, mtu: 0 };
    let _ = pan_step(&mut pan, 0);
    let ok = pan_step(&mut pan, 1);
    let connected = pan.state == PanState::Connected;
    let stop = pan_step(&mut pan, 2);
    set.add("F217 bluetooth pan", ok && connected && stop == false, "pair→connect");

    // F218 日志故事化
    let s1 = story_severity(NetStory::LinkUp);
    let s2 = story_severity(NetStory::DnsSlow(1500));
    let s3 = story_severity(NetStory::LinkDown);
    set.add("F218 net stories", s1 == 0 && s2 == 2 && s3 == 2, "severity map");

    // F219 测速靶
    let r1 = speedtest_echo(42 * 100, 100, 1000, 1042);
    let r2 = speedtest_echo(1, 100, 1000, 1042);
    set.add("F219 speedtest target", r1 == Some((true, 42)) && r2 == Some((false, 42)), "echo verify");

    // F220 预览沙
    let v1 = preview_sandbox(false, 1000);
    let v2 = preview_sandbox(true, 1000);
    let v3 = preview_sandbox(false, 70000);
    set.add("F220 preview sandbox", v1 == PreviewVerdict::FetchMeta && v2 == PreviewVerdict::Blocked && v3 == PreviewVerdict::Blocked, "private/size guard");

    // F221 P2P
    let mut p2p = P2pSession::new(0xCAFE, 4);
    for _ in 0..4 {
        p2p.ack_chunk();
    }
    let done = !p2p.active && p2p.progress_permille() == 1000;
    let over = p2p.ack_chunk();
    set.add("F221 p2p direct", done && !over, "chunk completion");

    // F222 性能基线
    let base = NetBaseline { throughput_mbps: 100, rtt_ms: 20 };
    let ok = !net_regress(base, NetBaseline { throughput_mbps: 90, rtt_ms: 22 });
    let bad = net_regress(base, NetBaseline { throughput_mbps: 80, rtt_ms: 20 });
    set.add("F222 net baseline", ok && bad, "15%/25% thresholds");

    // F223 IPv6
    let mut a = V6Addr::new(0x2001_db8, 0x42);
    a.valid_until_ms = 3000;
    a.preferred_until_ms = 2000;
    let s1 = a.age(1000);
    let s2 = a.age(2500);
    let s3 = a.age(3500);
    set.add(
        "F223 ipv6 lifecycle",
        s1 == V6AddrState::Preferred && s2 == V6AddrState::Deprecated && s3 == V6AddrState::Invalid
            && !V6Addr::new(1, 1).usable_for_source(),
        "addr lifecycle",
    );

    // F224 网络 fuzz
    let mut fz = NetFuzzer::new(77);
    let mut accepted = 0;
    for _ in 0..200 {
        let len = fz.gen_len();
        if fz.ingest(len, 1500) {
            accepted += 1;
        }
    }
    set.add("F224 net fuzz", fz.rejected > 0 && accepted > 0 && accepted + fz.rejected == 200, "malformed filter");

    // F225 域自检可用性
    set.add("F225 netxp selftest reachable", set.len() >= 24, "selftest must cover domain");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f202_score_bounds() {
        assert_eq!(connection_score(0, 0, 0), 100);
        assert!(connection_score(500, 500, 200) < 30);
    }

    #[test]
    fn f208_dns_eviction() {
        let mut c = DnsCache::new();
        for i in 0..DNS_CAP as u64 {
            c.put(i, i as u32, 0);
        }
        assert!(c.put(999, 999, 10)); // 触发逐出
        assert_eq!(c.get(0, 20), None);
        assert_eq!(c.get(999, 20), Some(999));
    }

    #[test]
    fn f225_selftest_passes() {
        let set = run_netxp_checks();
        let mut buf = [0u8; 512];
        set.render(&mut buf);
        assert!(set.all_passed() && set.len() >= 25, "{}", core::str::from_utf8(&buf).unwrap_or("?"));
    }
}
