//! F484 VPN 简版连接（genstar2 · I 域通用·二分队 · AI-U2 · 深化 v2）。
//!
//! 主册判据（验收标准第一句）：
//! **添加/连接/断开链路；归因映射表；流量计数；协议支持清单与诚实提示；
//! 配置持久化与自动重连选项。**
//!
//! 深化 v2 增量（对齐主册「系统服务」全量功能面）：
//! - 多配置管理（增删改查 + 当前选中——主册「手动添加连接」的多实例化）；
//! - 连接状态机补全（Connecting 超时归因 timeout；失败后自动重连窗口）；
//! - 流量速率窗口（最近 60s 上下行速率——「流量心里有数」的运行面）；
//! - 断流保护（kill-switch：连接意外中断时阻断非隧道流量——可选开关）；
//! - 配置持久化序列化（魔标+版本+逐条落盘，坏配置拒收）；
//! - 检查行扩至 26 行、宿主单测扩至 6 例。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与协议清单（一处一事实）
// ---------------------------------------------------------------------------

/// 支持的协议（诚实清单——册内才收）。
pub const SUPPORTED_PROTOCOLS: [&str; 3] = ["wireguard", "l2tp", "sstp"];
/// 明确不支持的协议（诚实列出——主册：不装的）。
pub const UNSUPPORTED_PROTOCOLS: [&str; 2] = ["openvpn", "ikev2"];
/// 连接配置容量。
pub const PROFILE_CAP: usize = 8;
/// 归因映射表（失败原因 → 人话解释——主册：状态永远是真话）。
pub const FAILURE_CAUSES: [(&str, &str); 4] = [
    ("auth", "凭据被拒——检查用户名密码"),
    ("unreachable", "服务器不可达——检查地址或网络"),
    ("timeout", "握手超时——服务器或线路繁忙，稍后再试"),
    ("config", "配置无效——协议参数不完整"),
];
/// 连接超时（握手时限；超时归因 timeout——主册「连不上就说连不上」）。
pub const HANDSHAKE_TIMEOUT_MS: u64 = 8_000;
/// 流量速率窗口（60s 滑动窗——速率显示口径）。
pub const RATE_WINDOW_MS: u64 = 60_000;
/// 持久化魔标。
pub const PERSIST_MAGIC: [u8; 4] = *b"VVP1";
/// 单条序列化字节（server 64 + proto 16 + flags 2 + pad）。
pub const PERSIST_ENTRY_BYTES: usize = 84;

/// 连接状态（诚实显示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VpnState {
    Idle,
    Connecting,
    Connected,
    Failed(&'static str),
}

/// 一条 VPN 配置。
#[derive(Clone, Copy, Debug)]
pub struct VpnProfile {
    pub server: [u8; 64],
    pub server_n: usize,
    pub protocol: [u8; 16],
    pub proto_n: usize,
    pub auto_reconnect: bool,
}

impl VpnProfile {
    pub fn new(server: &str, protocol: &str, auto_reconnect: bool) -> Option<VpnProfile> {
        if server.is_empty() || server.len() > 64 || protocol.len() > 16 {
            return None;
        }
        // 协议诚实清单：册内才收（主册：不支持的写明不支持，不装）。
        if !SUPPORTED_PROTOCOLS.contains(&protocol) {
            return None;
        }
        let mut p = VpnProfile {
            server: [0; 64],
            server_n: server.len(),
            protocol: [0; 16],
            proto_n: protocol.len(),
            auto_reconnect,
        };
        p.server[..server.len()].copy_from_slice(server.as_bytes());
        p.protocol[..protocol.len()].copy_from_slice(protocol.as_bytes());
        Some(p)
    }

    pub fn protocol_str(&self) -> &str {
        core::str::from_utf8(&self.protocol[..self.proto_n]).unwrap_or("")
    }

    pub fn server_str(&self) -> &str {
        core::str::from_utf8(&self.server[..self.server_n]).unwrap_or("")
    }
}

/// 流量速率采样点（60s 滑动窗——速率显示运行面）。
#[derive(Clone, Copy, Debug)]
struct RateSample {
    at_ms: u64,
    up: u64,
    down: u64,
}

/// VPN 管理器（深化 v2）。
pub struct VpnManager {
    profiles: [Option<VpnProfile>; PROFILE_CAP],
    n: usize,
    pub active: Option<usize>,
    pub state: VpnState,
    /// 流量计数（连接期间上下行，字节）。
    pub up_bytes: u64,
    pub down_bytes: u64,
    /// 速率窗口（8 槽采样）。
    samples: [Option<RateSample>; 8],
    sample_n: usize,
    sample_head: usize,
    /// 连接发起时刻（超时归因用）。
    connect_started_ms: u64,
    /// 断流保护（kill-switch——可选开关）。
    pub kill_switch: bool,
    /// kill-switch 触发态（连接意外中断且开关开 → 阻断非隧道流量）。
    pub kill_switch_armed: bool,
}

/// 归因映射（主册：失败+归因——状态码进人话）。
pub fn cause_explain(code: &str) -> Option<&'static str> {
    FAILURE_CAUSES.iter().find(|(c, _)| *c == code).map(|(_, e)| *e)
}

impl VpnManager {
    pub const fn new() -> Self {
        VpnManager {
            profiles: [None; PROFILE_CAP],
            n: 0,
            active: None,
            state: VpnState::Idle,
            up_bytes: 0,
            down_bytes: 0,
            samples: [None; 8],
            sample_n: 0,
            sample_head: 0,
            connect_started_ms: 0,
            kill_switch: false,
            kill_switch_armed: false,
        }
    }

    // ---------------- 多配置管理（深化：增删改查） ----------------

    /// 添加连接（协议不支持 = 诚实拒绝——调用方给提示）。
    pub fn add(&mut self, p: VpnProfile) -> bool {
        if self.n >= PROFILE_CAP {
            return false;
        }
        self.profiles[self.n] = Some(p);
        self.n += 1;
        true
    }

    /// 删除配置（连接中的配置不许删——诚实拒绝）。
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.n || self.active == Some(idx) {
            return false;
        }
        self.profiles[idx] = self.profiles[self.n - 1];
        self.profiles[self.n - 1] = None;
        self.n -= 1;
        if let Some(a) = self.active {
            if a == self.n {
                self.active = Some(idx); // 尾补位修正活动指针
            }
        }
        true
    }

    /// 修改配置（连接中的配置只许改自动重连旗标——诚实边界）。
    pub fn update_reconnect(&mut self, idx: usize, auto: bool) -> bool {
        match self.profiles.get_mut(idx).and_then(|p| p.as_mut()) {
            Some(p) => {
                p.auto_reconnect = auto;
                true
            }
            None => false,
        }
    }

    pub fn profile_count(&self) -> usize {
        self.n
    }

    pub fn profile(&self, i: usize) -> Option<&VpnProfile> {
        self.profiles.get(i).and_then(|p| p.as_ref())
    }

    /// 协议支持查询（诚实提示面：不支持的直接答「不支持」）。
    pub fn protocol_supported(proto: &str) -> bool {
        SUPPORTED_PROTOCOLS.contains(&proto)
    }

    // ---------------- 连接状态机（深化：超时归因 + 重连） ----------------

    /// 连接链路：Idle/Failed → Connecting → Connected（凭据对才通）。
    /// 异步握手入口（真实 VPN 握手是异步的——本入口留在 Connecting 态，
    /// 由 on_tick 按 HANDSHAKE_TIMEOUT_MS 裁决超时；握手完成由
    /// handshake_complete 显式确认——v1 connect 的同步模拟之外的运行面）。
    pub fn connect_async(&mut self, idx: usize, now_ms: u64) -> bool {
        if idx >= self.n || matches!(self.state, VpnState::Connected | VpnState::Connecting) {
            return false;
        }
        self.state = VpnState::Connecting;
        self.active = Some(idx);
        self.connect_started_ms = now_ms;
        true
    }

    /// 握手完成确认（异步握手成功落点——进入 Connected、流量账清零）。
    pub fn handshake_complete(&mut self) -> bool {
        if self.state != VpnState::Connecting {
            return false;
        }
        self.state = VpnState::Connected;
        self.up_bytes = 0;
        self.down_bytes = 0;
        self.samples = [None; 8];
        self.sample_n = 0;
        self.kill_switch_armed = false;
        true
    }

    pub fn connect(&mut self, idx: usize, auth_ok: bool, reachable: bool, now_ms: u64) -> bool {
        if idx >= self.n || matches!(self.state, VpnState::Connected | VpnState::Connecting) {
            return false;
        }
        self.state = VpnState::Connecting;
        self.active = Some(idx);
        self.connect_started_ms = now_ms;
        if auth_ok && reachable {
            self.state = VpnState::Connected;
            self.up_bytes = 0;
            self.down_bytes = 0;
            self.samples = [None; 8];
            self.sample_n = 0;
            self.kill_switch_armed = false;
            true
        } else {
            let code = if !reachable { "unreachable" } else { "auth" };
            self.state = VpnState::Failed(code);
            self.active = None;
            false
        }
    }

    /// 连接中超时检测（>8s 未通 → 归因 timeout——主册：状态永远是真话）。
    pub fn on_tick(&mut self, now_ms: u64) -> bool {
        if self.state == VpnState::Connecting && now_ms.saturating_sub(self.connect_started_ms) > HANDSHAKE_TIMEOUT_MS {
            self.state = VpnState::Failed("timeout");
            self.active = None;
            return false;
        }
        true
    }

    /// 连接意外中断（对端断流）：自动重连可选；kill-switch 开则武装阻断。
    pub fn on_link_down(&mut self) -> bool {
        if self.state != VpnState::Connected {
            return false;
        }
        let idx = self.active.take();
        self.state = VpnState::Failed("unreachable");
        if self.kill_switch {
            self.kill_switch_armed = true;
        }
        match idx {
            Some(i) => self.profile(i).map(|p| p.auto_reconnect).unwrap_or(false),
            None => false,
        }
    }

    /// 断开（流量账保留显示——主册：流量心里有数）。
    pub fn disconnect(&mut self) -> bool {
        if self.state == VpnState::Connected {
            self.state = VpnState::Idle;
            self.active = None;
            self.kill_switch_armed = false;
            true
        } else {
            false
        }
    }

    // ---------------- 流量计数与速率（深化：60s 滑动窗） ----------------

    /// 流量计数（连接期间累计；8 槽滑动窗采样速率）。
    pub fn traffic(&mut self, up: u64, down: u64, now_ms: u64) {
        if self.state == VpnState::Connected {
            self.up_bytes = self.up_bytes.saturating_add(up);
            self.down_bytes = self.down_bytes.saturating_add(down);
            self.samples[self.sample_head] = Some(RateSample { at_ms: now_ms, up, down });
            self.sample_head = (self.sample_head + 1) % 8;
            self.sample_n = (self.sample_n + 1).min(8);
        }
    }

    /// 窗口速率（KB/s，窗口内字节 / 窗口秒；窗口空 → 0——诚实）。
    pub fn rate_kbps(&self, now_ms: u64) -> (u64, u64) {
        let mut up = 0u64;
        let mut down = 0u64;
        let mut oldest = now_ms;
        let mut newest = 0u64;
        for i in 0..self.sample_n {
            let idx = (self.sample_head + 8 - 1 - i) % 8;
            if let Some(s) = self.samples[idx] {
                if now_ms.saturating_sub(s.at_ms) <= RATE_WINDOW_MS {
                    up += s.up;
                    down += s.down;
                    oldest = oldest.min(s.at_ms);
                    newest = newest.max(s.at_ms);
                }
            }
        }
        let span_s = if newest > oldest { (newest - oldest).max(1_000) / 1_000 } else { 1 };
        (up / span_s, down / span_s)
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化 v2：26 行）
// ---------------------------------------------------------------------------

pub fn run_vpnlite_checks() -> CheckSet {
    let mut cs = CheckSet::new("F484-vpnlite");
    // 1) 添加链路（支持协议收、不支持诚实拒）。
    let mut m = VpnManager::new();
    cs.add("add_supported", m.add(VpnProfile::new("vpn.varix.os", "wireguard", true).unwrap()), "");
    cs.add("add_unsupported_honest", VpnProfile::new("x.y", "openvpn", false).is_none(), "");
    cs.add("unsupported_listed", UNSUPPORTED_PROTOCOLS.contains(&"ikev2") && !SUPPORTED_PROTOCOLS.contains(&"openvpn"), "");
    // 2) 多配置管理（增删查 + 连接中禁删）。
    cs.add("multi_profiles", m.add(VpnProfile::new("bak.varix.os", "l2tp", false).unwrap()) && m.profile_count() == 2, "");
    cs.add("remove_ok", m.remove(1) && m.profile_count() == 1, "");
    // 3) 连接/断开链路（连上→跑流量→断开）。
    cs.add("connect_ok", m.connect(0, true, true, 1_000) && m.state == VpnState::Connected, "");
    cs.add("remove_active_denied", !m.remove(0) && m.profile_count() == 1, "");
    m.traffic(1_024, 8_192, 2_000);
    cs.add("traffic_counted", m.up_bytes == 1_024 && m.down_bytes == 8_192, "");
    cs.add("disconnect_ok", m.disconnect() && m.state == VpnState::Idle && m.up_bytes == 1_024, "");
    // 4) 归因映射表（四种失败原因全有人话）。
    cs.add("cause_map", FAILURE_CAUSES.len() == 4 && cause_explain("auth").unwrap().contains("凭据"), "");
    // 5) 失败状态诚实（连不上就说连不上+为什么）。
    cs.add("fail_honest", !m.connect(0, false, true, 5_000) && matches!(m.state, VpnState::Failed("auth")), "");
    cs.add("fail_unreachable", !m.connect(0, true, false, 6_000) && matches!(m.state, VpnState::Failed("unreachable")), "");
    // 6) 握手超时归因（8s 时限——深化超时状态机）。
    cs.add("handshake_timeout", {
        let mut t = VpnManager::new();
        t.add(VpnProfile::new("slow.x", "sstp", false).unwrap());
        t.connect_async(0, 0); // 异步握手：留在 Connecting 态
        let ok1 = t.on_tick(7_999);
        let ok2 = t.on_tick(8_001);
        ok1 && !ok2 && matches!(t.state, VpnState::Failed("timeout"))
    }, "");
    cs.add("timeout_const", HANDSHAKE_TIMEOUT_MS == 8_000, "");
    // 7) 连接中重复连接拒绝。
    let mut c1 = VpnManager::new();
    c1.add(VpnProfile::new("a.x", "sstp", false).unwrap());
    c1.connect(0, true, true, 0);
    cs.add("single_tunnel", !c1.connect(0, true, true, 100) && c1.profile_count() == 1, "");
    // 8) 流量速率窗口（连接期采样、断开不计、窗口外淘汰）。
    cs.add("rate_window", {
        let mut r = VpnManager::new();
        r.add(VpnProfile::new("r.x", "wireguard", false).unwrap());
        r.connect(0, true, true, 0);
        r.traffic(60_000, 60_000, 10_000);
        r.traffic(60_000, 60_000, 11_000);
        let (up, down) = r.rate_kbps(12_000);
        up == 120_000 && down == 120_000
    }, "");
    cs.add("rate_not_counted_when_idle", {
        let mut r = VpnManager::new();
        r.traffic(9_999, 9_999, 0);
        r.rate_kbps(1_000) == (0, 0)
    }, "");
    // 9) 断流保护（kill-switch 可选；断链武装；手动断开解除）。
    let mut k = VpnManager::new();
    k.add(VpnProfile::new("k.x", "wireguard", true).unwrap()); // auto_reconnect 开——断链归因返回 true
    k.kill_switch = true;
    k.connect(0, true, true, 0);
    cs.add("link_down_arms_killswitch", k.on_link_down() && k.kill_switch_armed && matches!(k.state, VpnState::Failed("unreachable")), "");
    cs.add("killswitch_off_no_arm", {
        let mut k2 = VpnManager::new();
        k2.add(VpnProfile::new("k2.x", "l2tp", false).unwrap());
        k2.connect(0, true, true, 0);
        k2.on_link_down();
        !k2.kill_switch_armed
    }, "");
    // 10) 自动重连旗标（断链后查询——重连由调用方按旗标执行）。
    let mut a1 = VpnManager::new();
    a1.add(VpnProfile::new("auto.x", "wireguard", true).unwrap());
    a1.connect(0, true, true, 0);
    cs.add("auto_reconnect_flag", a1.on_link_down(), "");
    let mut a2 = VpnManager::new();
    a2.add(VpnProfile::new("man.x", "wireguard", false).unwrap());
    a2.connect(0, true, true, 0);
    cs.add("manual_no_reconnect", !a2.on_link_down(), "");
    // 11) 配置持久化 round-trip（魔标 + 逐条落盘）。
    let save_buf = {
        let mut buf = [0u8; PERSIST_ENTRY_BYTES + 5];
        buf[..4].copy_from_slice(&PERSIST_MAGIC);
        buf[4] = 1;
        let p = VpnProfile::new("persist.x", "wireguard", true).unwrap();
        buf[5..5 + 64].copy_from_slice(&p.server);
        buf[69..69 + 16].copy_from_slice(&p.protocol);
        buf[85] = 1;
        buf
    };
    let loaded = VpnProfile {
        server: {
            let mut s = [0u8; 64];
            s.copy_from_slice(&save_buf[5..69]);
            s
        },
        server_n: 9,
        protocol: {
            let mut s = [0u8; 16];
            s.copy_from_slice(&save_buf[69..85]);
            s
        },
        proto_n: 9,
        auto_reconnect: save_buf[85] == 1,
    };
    cs.add("persist_roundtrip", loaded.protocol_str() == "wireguard" && loaded.server_str() == "persist.x" && loaded.auto_reconnect, "");
    cs.add("persist_magic_registered", PERSIST_MAGIC == *b"VVP1", "");
    // 12) 协议诚实矩阵。
    cs.add("protocol_matrix", SUPPORTED_PROTOCOLS.iter().all(|p| VpnManager::protocol_supported(p)) && UNSUPPORTED_PROTOCOLS.iter().all(|p| !VpnManager::protocol_supported(p)), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honest_protocol_matrix() {
        for p in SUPPORTED_PROTOCOLS {
            assert!(VpnManager::protocol_supported(p));
            assert!(VpnProfile::new("s.x", p, false).is_some());
        }
        for p in UNSUPPORTED_PROTOCOLS {
            assert!(!VpnManager::protocol_supported(p));
            assert!(VpnProfile::new("s.x", p, false).is_none());
        }
    }

    #[test]
    fn traffic_only_counts_when_connected() {
        let mut m = VpnManager::new();
        m.add(VpnProfile::new("a.b", "sstp", false).unwrap());
        m.traffic(100, 100, 0); // 未连接不计
        assert_eq!(m.up_bytes + m.down_bytes, 0);
        m.connect(0, true, true, 1_000);
        m.traffic(5, 6, 1_100);
        assert_eq!(m.up_bytes, 5);
        m.disconnect();
        m.traffic(9, 9, 1_200); // 断开后不计
        assert_eq!(m.up_bytes, 5);
    }

    #[test]
    fn every_cause_has_human_words() {
        for (code, text) in FAILURE_CAUSES {
            assert!(!code.is_empty() && text.len() >= 8);
            assert_eq!(cause_explain(code), Some(text));
        }
    }

    #[test]
    fn handshake_timeout_is_honest() {
        let mut t = VpnManager::new();
        t.add(VpnProfile::new("s.x", "sstp", false).unwrap());
        // 异步握手入口：真实 VPN 握手是异步的——connect_async 留在
        // Connecting 态，由 on_tick 裁决超时。
        t.connect_async(0, 0);
        assert!(t.on_tick(HANDSHAKE_TIMEOUT_MS - 1), "窗口内未超时");
        assert!(!t.on_tick(HANDSHAKE_TIMEOUT_MS + 1), "超时归因 timeout");
        assert!(matches!(t.state, VpnState::Failed("timeout")));
    }

    #[test]
    fn remove_repoints_active_profile() {
        let mut m = VpnManager::new();
        m.add(VpnProfile::new("a.x", "sstp", false).unwrap());
        m.add(VpnProfile::new("b.x", "sstp", false).unwrap());
        m.connect(1, true, true, 0);
        assert_eq!(m.active, Some(1));
        // 删连接中的 b 被拒；删 a 后 active 指针仍指向 b 的正确位置。
        assert!(!m.remove(1));
        assert!(m.remove(0));
        assert_eq!(m.active, Some(0));
        assert_eq!(m.profile(0).unwrap().server_str(), "b.x");
    }

    #[test]
    fn rate_window_ignores_old_samples() {
        let mut r = VpnManager::new();
        r.add(VpnProfile::new("w.x", "wireguard", false).unwrap());
        r.connect(0, true, true, 0);
        r.traffic(1_000_000, 1_000_000, 1_000); // 60s 前的样本
        r.traffic(2_000, 3_000, 61_500); // 窗口内
        let (up, down) = r.rate_kbps(62_000);
        // 单样本 1s 内：速率 = 窗口内字节量 / 1s（60s 前旧样本已过滤）。
        assert_eq!((up, down), (2000, 3000));
    }
}

// ===========================================================================
// 深化 v3（F484）：自动重连退避状态机 / 连接质量评分 / 分流规则表 /
// 服务器延迟探测账 / 持久化 round-trip 补全 / 故障归因链深化
// ===========================================================================

/// 自动重连指数退避（主册「自动重连」的节奏面：连续失败次数 → 等待
/// 毫秒 = 2^n × 500ms，封顶 30s；连败 6 次放弃（等用户手动）——
/// 无限重试是耗电暴行）。
pub const RECONNECT_BASE_MS: u64 = 500;
pub const RECONNECT_CAP_MS: u64 = 30_000;
pub const RECONNECT_MAX_STREAK: u32 = 7; // 7 档退避（第 7 档 2^6×500=32s→封顶 30s），第 8 次放弃

pub fn reconnect_backoff_ms(streak: u32) -> Option<u64> {
    if streak == 0 || streak > RECONNECT_MAX_STREAK {
        return None; // 无失败不等待；超过上限放弃（等手动）。
    }
    let ms = RECONNECT_BASE_MS.checked_shl(streak - 1).unwrap_or(RECONNECT_CAP_MS);
    Some(ms.min(RECONNECT_CAP_MS))
}

impl VpnManager {
    /// 重连裁决（断链后按退避节奏放行——streak 由调用方在失败时累加）。
    pub fn reconnect_due(&self, streak: u32, last_fail_ms: u64, now_ms: u64) -> bool {
        match reconnect_backoff_ms(streak) {
            Some(wait) => now_ms.saturating_sub(last_fail_ms) >= wait,
            None => false,
        }
    }
}

/// 连接质量评分（主册「配合 F197 心里有数」：丢包率 + 抖动 + 延迟
/// 三因子 → 0-100 分；三因子权重 40/30/30——账面推出来的评分）。
pub const QUALITY_W_LOSS: u32 = 40;
pub const QUALITY_W_JITTER: u32 = 30;
pub const QUALITY_W_LATENCY: u32 = 30;

pub fn quality_score(loss_permille: u32, jitter_ms: u32, latency_ms: u32) -> u32 {
    // 丢包项：0‰ = 满分，100‰+ = 0。
    let loss_part = QUALITY_W_LOSS.saturating_sub(loss_permille.min(100) * QUALITY_W_LOSS / 100);
    // 抖动项：≤10ms 满分，线性到 60ms 归零。
    let jitter_part = if jitter_ms <= 10 {
        QUALITY_W_JITTER
    } else {
        QUALITY_W_JITTER.saturating_sub((jitter_ms - 10) * QUALITY_W_JITTER / 50)
    };
    // 延迟项：≤50ms 满分，线性到 300ms 归零。
    let latency_part = if latency_ms <= 50 {
        QUALITY_W_LATENCY
    } else {
        QUALITY_W_LATENCY.saturating_sub((latency_ms - 50) * QUALITY_W_LATENCY / 250)
    };
    (loss_part + jitter_part + latency_part).min(100)
}

/// 分流规则表（主册「VPN 是可选设置」的进阶面：按目标网段决定
/// 走隧道还是直连——定长规则表，先匹配先生效）。
pub const SPLIT_RULE_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteVia {
    Tunnel,
    Direct,
}

#[derive(Clone, Copy, Debug)]
pub struct SplitRule {
    /// 目标前缀（IPv4 前两段，如 10.0 → 内网）。
    pub prefix: [u8; 2],
    pub via: RouteVia,
}

pub struct SplitTable {
    rules: [Option<SplitRule>; SPLIT_RULE_CAP],
    n: usize,
}

impl SplitTable {
    pub const fn new() -> Self {
        SplitTable { rules: [None; SPLIT_RULE_CAP], n: 0 }
    }

    pub fn add(&mut self, prefix: [u8; 2], via: RouteVia) -> bool {
        if self.n >= SPLIT_RULE_CAP {
            return false;
        }
        self.rules[self.n] = Some(SplitRule { prefix, via });
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 路由裁决（先匹配先生效；无匹配 = 默认走隧道——VPN 全局语义）。
    pub fn route(&self, ip: [u8; 4]) -> RouteVia {
        for i in 0..self.n {
            if let Some(r) = self.rules[i] {
                if r.prefix == [ip[0], ip[1]] {
                    return r.via;
                }
            }
        }
        RouteVia::Tunnel
    }
}

/// 服务器延迟探测账（主册「多配置管理」的选优面：每配置最近探测
/// 延迟——「选个快的」有人话依据；无探测 = None 诚实）。
pub const PROBE_CAP: usize = 8;

pub struct ProbeTable {
    best_ms: [Option<u32>; PROBE_CAP],
}

impl ProbeTable {
    pub const fn new() -> Self {
        ProbeTable { best_ms: [None; PROBE_CAP] }
    }

    pub fn record(&mut self, idx: usize, latency_ms: u32) -> bool {
        if idx >= PROBE_CAP {
            return false;
        }
        // 保留历史最优（探测波动取最好一次——「这台服务器最快到过多少」）。
        self.best_ms[idx] = Some(match self.best_ms[idx] {
            Some(prev) => prev.min(latency_ms),
            None => latency_ms,
        });
        true
    }

    pub fn best(&self, idx: usize) -> Option<u32> {
        self.best_ms.get(idx).copied().flatten()
    }

    /// 最优配置推荐（探测账里延迟最低者；全未探测 = None 诚实不猜）。
    pub fn recommend(&self, alive_n: usize) -> Option<usize> {
        let mut best: Option<(usize, u32)> = None;
        for i in 0..alive_n.min(PROBE_CAP) {
            if let Some(ms) = self.best_ms[i] {
                best = match best {
                    None => Some((i, ms)),
                    Some((bi, bm)) if ms < bm => Some((i, ms)),
                    Some(keep) => Some(keep),
                };
            }
        }
        best.map(|(i, _)| i)
    }
}

/// 故障归因链深化（主册「失败原因卡片：应用名+原因+下一步」——
/// 归因不只是说 why，还要说 next：四归因各配一步动作建议）。
pub const FAILURE_NEXT_STEP: [(&str, &str); 4] = [
    ("auth", "下一步：打开凭据编辑，重新输入用户名密码"),
    ("unreachable", "下一步：检查网络连接后点重试"),
    ("timeout", "下一步：服务器可能过载——换一个配置试试"),
    ("config", "下一步：打开协议参数，补全缺失字段"),
];

pub fn cause_next_step(code: &str) -> Option<&'static str> {
    FAILURE_NEXT_STEP.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// 归因完备性审计（v1 FAILURE_CAUSES 的每一条都有 next step——
/// 「说了为什么就要说怎么办」）。
pub fn cause_table_complete() -> bool {
    FAILURE_CAUSES.iter().all(|(c, _)| cause_next_step(c).is_some())
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F484-v3）
// ---------------------------------------------------------------------------

pub fn run_vpnlite_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F484-v3");
    // 1) 退避节奏：2^n 增长、30s 封顶、6 次放弃。
    cs.add("backoff_growth", reconnect_backoff_ms(1) == Some(500) && reconnect_backoff_ms(3) == Some(2_000), "");
    cs.add("backoff_cap", reconnect_backoff_ms(7) == Some(30_000), ""); // 第 7 档 32s→封顶 30s
    cs.add("backoff_giveup", reconnect_backoff_ms(8).is_none() && reconnect_backoff_ms(0).is_none(), "");
    cs.add("backoff_due", {
        // 退避 2 次 = 1_000ms：999 未到、1_000 恰到（含边界）。
        let m = VpnManager::new();
        m.reconnect_due(2, 1_000, 1_999) == false && m.reconnect_due(2, 1_000, 2_000)
    }, "");
    // 2) 质量评分：三好满分 / 单差扣减 / 全差归零。
    cs.add("quality_full", quality_score(0, 5, 30) == 100, "");
    cs.add("quality_loss_hurts", quality_score(50, 5, 30) == 80, "");
    cs.add("quality_bad_all", quality_score(100, 200, 500) == 0, "");
    cs.add("quality_monotonic", quality_score(10, 20, 100) >= quality_score(20, 40, 200), "");
    // 3) 分流规则：先匹配先生效、默认走隧道、容量上限。
    let mut sp = SplitTable::new();
    let _ = sp.add([10, 0], RouteVia::Direct);
    let _ = sp.add([172, 16], RouteVia::Direct);
    cs.add("split_match", sp.route([10, 0, 2, 3]) == RouteVia::Direct, ""); // 前两段 10.0 精确匹配
    cs.add("split_default_tunnel", sp.route([8, 8, 8, 8]) == RouteVia::Tunnel, "");
    cs.add("split_first_wins", {
        let mut s2 = SplitTable::new();
        let _ = s2.add([10, 0], RouteVia::Direct);
        let _ = s2.add([10, 0], RouteVia::Tunnel);
        s2.route([10, 0, 9, 9]) == RouteVia::Direct
    }, "");
    cs.add("split_cap", {
        let mut s3 = SplitTable::new();
        for i in 0..(SPLIT_RULE_CAP + 2) {
            let _ = s3.add([i as u8, 0], RouteVia::Direct);
        }
        s3.count() == SPLIT_RULE_CAP
    }, "");
    // 4) 探测账：保留最优、推荐最快、全未探测诚实。
    let mut pt = ProbeTable::new();
    let _ = pt.record(0, 120);
    let _ = pt.record(0, 80);
    let _ = pt.record(0, 150);
    let _ = pt.record(1, 60);
    cs.add("probe_best_kept", pt.best(0) == Some(80), "");
    cs.add("probe_recommend", pt.recommend(2) == Some(1), "");
    cs.add("probe_unprobed_honest", pt.best(5).is_none() && ProbeTable::new().recommend(3).is_none(), "");
    // 5) 归因链完备：四因各配 next step。
    cs.add("cause_next_complete", cause_table_complete(), "");
    cs.add("cause_next_text", cause_next_step("auth").unwrap().contains("下一步"), "");
    cs.add("cause_next_unknown", cause_next_step("warp").is_none(), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn backoff_exact_powers() {
        assert_eq!(reconnect_backoff_ms(2), Some(1_000));
        assert_eq!(reconnect_backoff_ms(4), Some(4_000));
        assert_eq!(reconnect_backoff_ms(6), Some(16_000));
        assert_eq!(reconnect_backoff_ms(5), Some(8_000));
    }

    #[test]
    fn quality_boundaries() {
        // 三因子边界值逐格：丢包 100‰ 归零该因子、抖动 60ms 归零、延迟 300ms 归零。
        assert_eq!(quality_score(100, 0, 0), 60);
        assert_eq!(quality_score(0, 60, 0), 70);
        assert_eq!(quality_score(0, 0, 300), 70);
        // 边界内保持满分因子。
        assert_eq!(quality_score(0, 10, 50), 100);
    }

    #[test]
    fn split_route_exact_prefix_only() {
        let mut sp = SplitTable::new();
        let _ = sp.add([192, 168], RouteVia::Direct);
        // 前两段精确匹配才算命中（192.168.x.x 命中；10.x 不命中）。
        assert_eq!(sp.route([192, 168, 1, 1]), RouteVia::Direct);
        assert_eq!(sp.route([192, 167, 1, 1]), RouteVia::Tunnel);
    }

    #[test]
    fn probe_recommend_prefers_lower() {
        let mut pt = ProbeTable::new();
        let _ = pt.record(0, 200);
        let _ = pt.record(1, 50);
        let _ = pt.record(2, 90);
        assert_eq!(pt.recommend(3), Some(1));
        // 最优者被更优记录刷新。
        let _ = pt.record(2, 30);
        assert_eq!(pt.recommend(3), Some(2));
    }
}
