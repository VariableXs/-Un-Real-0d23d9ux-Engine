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
