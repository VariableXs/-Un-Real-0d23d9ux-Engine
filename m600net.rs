//! m600net — VARIX-M600 AI-17 网络与互联域 (F401~F425)
//!
//! 网络栈精修/Wi-Fi 漫游大师/网络诊断室/带宽公平调度/流量可视化/
//! 防火墙策展/VPN 融合层/代理自动发现/DNS 策略中枢/离线优先架构/
//! 局域网发现/附近共享/手机伴侣/剪贴板跨设备/网络时间诚实化/
//! 证书管家/网络沙盒/热点礼仪/弱网优雅模式/网络回归金样/
//! 带宽预算/连接状态叙述者/网络故障剧本/IPv6 全栈/网络年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F401 — 网络栈精修：帧 MTU 与校验和
// ===========================================================================

pub const ETH_MTU: u16 = 1500;
pub const IPV6_MIN_MTU: u16 = 1280;

/// 帧可发：v4 上限 1500；v6 路径按发现下限 1280 封顶。
pub fn frame_sendable(len: u16, is_v6: bool) -> bool {
    if is_v6 {
        len <= IPV6_MIN_MTU
    } else {
        len <= ETH_MTU
    }
}

/// Internet 校验和（16 位反码和，整数实现）。
pub fn inet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// ===========================================================================
// F402 — Wi-Fi 漫游大师：信号迟滞切换
// ===========================================================================

pub const ROAM_HYSTERESIS_PERMILLE: u16 = 100; // 新 AP 需强 100‰ 才切换

/// 漫游裁决：当前信号 permille，候选需超出迟滞带才切换，防抖动。
pub fn roam_should_switch(current_permille: u16, candidate_permille: u16) -> bool {
    candidate_permille >= 1000.min(current_permille + ROAM_HYSTERESIS_PERMILLE)
}

// ===========================================================================
// F403 — 网络诊断室：ping/trace 判定
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ProbeResult {
    pub sent: u8,
    pub received: u8,
    pub rtt_max_ms: u32,
}

impl ProbeResult {
    /// 丢包率 permille（整数）。
    pub fn loss_permille(&self) -> u16 {
        if self.sent == 0 {
            return 1000;
        }
        ((self.sent - self.received) as u16 * 1000) / self.sent as u16
    }
    pub fn healthy(&self) -> bool {
        self.received > 0 && self.loss_permille() <= 200 && self.rtt_max_ms <= 500
    }
}

// ===========================================================================
// F404 — 带宽公平调度：按权重分配（permille）
// ===========================================================================

/// 公平份额：总带宽按权重比例切，返回 permille（和恰为 1000）。
/// weights: 裸切片，out 与 weights 等长。
pub fn fair_share_permille(weights: &[u32], out: &mut [u16]) -> bool {
    let total: u32 = weights.iter().sum();
    if total == 0 || out.len() < weights.len() {
        return false;
    }
    let mut allocated: u32 = 0;
    for i in 0..weights.len() {
        let share = weights[i] * 1000 / total;
        out[i] = share as u16;
        allocated += share;
    }
    // 余数补给首个权重最大者，保证总和恰为 1000。
    let rest = 1000 - allocated;
    if rest > 0 {
        let mut best = 0usize;
        for i in 0..weights.len() {
            if weights[i] > weights[best] {
                best = i;
            }
        }
        out[best] += rest as u16;
    }
    true
}

// ===========================================================================
// F405 — 流量可视化：按应用聚合
// ===========================================================================

#[derive(Clone, Copy)]
pub struct AppTraffic {
    pub app_id: &'static str,
    pub kib: u32,
}

/// 头部占用者：返回占用最大的下标（并列取先）。
pub fn top_talker(traffic: &[AppTraffic]) -> usize {
    let mut best = 0usize;
    for i in 0..traffic.len() {
        if traffic[i].kib > traffic[best].kib {
            best = i;
        }
    }
    best
}

// ===========================================================================
// F406 — 防火墙策展：默认拒绝 + 白名单
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FirewallAction {
    Allow,
    Deny,
}

#[derive(Clone, Copy)]
pub struct FwRule {
    pub port: u16,
    pub action: FirewallAction,
}

/// 默认拒绝防火墙：命中白名单端口放行，其余拒绝。
pub const FW_DEFAULT: FirewallAction = FirewallAction::Deny;

pub fn firewall_decide(rules: &[FwRule], port: u16) -> FirewallAction {
    for r in rules {
        if r.port == port {
            return r.action;
        }
    }
    FW_DEFAULT
}

// ===========================================================================
// F407 — VPN 融合层：隧道优先级
// ===========================================================================

#[derive(Clone, Copy)]
pub struct VpnTunnel {
    pub id: u8,
    pub metric: u16, // 越小越优先
    pub up: bool,
}

/// 选活隧道中 metric 最小者；全挂则 None（用 u8::MAX 表示无）。
pub fn vpn_select(tunnels: &[VpnTunnel]) -> Option<u8> {
    let mut best: Option<u8> = None;
    let mut best_metric = u16::MAX;
    for t in tunnels {
        if t.up && t.metric < best_metric {
            best_metric = t.metric;
            best = Some(t.id);
        }
    }
    best
}

// ===========================================================================
// F408 — 代理自动发现：PAC 结果裁决
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProxyDecision {
    Direct,
    ViaProxy,
}

/// 目标主机命中例外表（以 "." 开头为后缀匹配）→ 直连。
pub fn proxy_decide(exceptions: &[&str], host: &str) -> ProxyDecision {
    let hb = host.as_bytes();
    for ex in exceptions {
        if let Some(suffix) = ex.strip_prefix('.') {
            let sb = suffix.as_bytes();
            if hb.len() > sb.len() && hb.ends_with(sb) && hb[hb.len() - sb.len() - 1] == b'.' {
                return ProxyDecision::Direct;
            }
        } else if *ex == host {
            return ProxyDecision::Direct;
        }
    }
    ProxyDecision::ViaProxy
}

// ===========================================================================
// F409 — DNS 策略中枢：解析路径选择
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DnsPath {
    Cache,
    Plain,
    Encrypted,
}

pub const DNS_CACHE_TTL_S: u32 = 300;

/// 敏感域名走加密，命中缓存走缓存，其余普通。
pub fn dns_pick_path(cache_fresh_s: u32, sensitive: bool) -> DnsPath {
    if sensitive {
        DnsPath::Encrypted
    } else if cache_fresh_s < DNS_CACHE_TTL_S {
        DnsPath::Cache
    } else {
        DnsPath::Plain
    }
}

// ===========================================================================
// F410 — 离线优先架构：本地队列先落地
// ===========================================================================

pub const OFFLINE_QUEUE_MAX: usize = 64;

#[derive(Clone, Copy)]
pub struct OfflineQueue {
    len: usize,
    flushed: usize,
}

impl OfflineQueue {
    pub const fn new() -> OfflineQueue {
        OfflineQueue { len: 0, flushed: 0 }
    }
    /// 入队：满则拒（返回 false），数据不丢先落本地。
    pub fn enqueue(&mut self) -> bool {
        if self.len >= OFFLINE_QUEUE_MAX {
            return false;
        }
        self.len += 1;
        true
    }
    /// 联网后按序冲刷，返回冲刷条数。
    pub fn flush(&mut self, budget: usize) -> usize {
        let n = if budget < self.len { budget } else { self.len };
        self.len -= n;
        self.flushed += n;
        n
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn total_flushed(&self) -> usize {
        self.flushed
    }
}

// ===========================================================================
// F411 — 局域网发现：通告 TTL 衰减
// ===========================================================================

/// 每过 refresh_interval 通告 TTL 减半（整数），≤ 0 判过期。
pub fn announce_ttl_after(halvings: u32, initial_s: u32) -> u32 {
    let mut ttl = initial_s;
    let mut i = 0;
    while i < halvings && ttl > 1 {
        ttl /= 2;
        i += 1;
    }
    if halvings > 0 && ttl <= 1 {
        0
    } else {
        ttl
    }
}

// ===========================================================================
// F412 — 附近共享：配对握手四步
// ===========================================================================

pub const SHARE_HANDSHAKE_STEPS: [&str; 4] =
    ["advertise", "request", "consent", "transfer"];

/// 握手状态推进合法性：必须按序。
pub fn share_step_ok(from: u8, to: u8) -> bool {
    to == from + 1 && (to as usize) < SHARE_HANDSHAKE_STEPS.len()
}

// ===========================================================================
// F413 — 手机伴侣：消息镜像配额
// ===========================================================================

pub const PHONE_MIRROR_MAX_MSGS: usize = 500;

/// 镜像配额：未读 permille 超 800 触发摘要折叠。
pub fn phone_mirror_fold(unread: usize, total: usize) -> bool {
    if total == 0 {
        return false;
    }
    unread * 1000 / total > 800 && total > PHONE_MIRROR_MAX_MSGS / 10
}

// ===========================================================================
// F414 — 剪贴板跨设备：敏感内容不出网
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ClipItem {
    pub size_bytes: u32,
    pub marked_sensitive: bool,
}

/// 同步裁决：敏感或超大（> 1 MiB）不跨设备。
pub fn clip_syncable(c: ClipItem) -> bool {
    !c.marked_sensitive && c.size_bytes <= 1024 * 1024
}

// ===========================================================================
// F415 — 网络时间诚实化：偏移报告
// ===========================================================================

pub const NTP_DRIFT_WARN_MS: i32 = 2000;

/// 偏移绝对值超阈值需告警（有符号处理）。
pub fn ntp_drift_warn(offset_ms: i32) -> bool {
    let abs = if offset_ms < 0 { -offset_ms } else { offset_ms };
    abs > NTP_DRIFT_WARN_MS
}

// ===========================================================================
// F416 — 证书管家：有效期窗口
// ===========================================================================

pub const CERT_RENEW_WINDOW_DAYS: u32 = 14;

/// 距过期天数 ≤ 窗口即需续期。
pub fn cert_needs_renew(days_left: u32) -> bool {
    days_left <= CERT_RENEW_WINDOW_DAYS
}

// ===========================================================================
// F417 — 网络沙盒：应用网络白名单端口
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NetSandbox {
    pub allowed_ports: [u16; 4],
    pub used: usize,
}

impl NetSandbox {
    pub const fn new() -> NetSandbox {
        NetSandbox { allowed_ports: [0; 4], used: 0 }
    }
    pub fn allow(&mut self, port: u16) -> bool {
        if self.used >= 4 {
            return false;
        }
        self.allowed_ports[self.used] = port;
        self.used += 1;
        true
    }
    pub fn can_connect(&self, port: u16) -> bool {
        self.allowed_ports[..self.used].contains(&port)
    }
}

// ===========================================================================
// F418 — 热点礼仪：共享限额
// ===========================================================================

pub const HOTSPOT_TETHER_BUDGET_MIB: u32 = 2048;

/// 热点共享：累计流量超预算即提示降速。
pub fn hotspot_over_budget(used_mib: u32) -> bool {
    used_mib > HOTSPOT_TETHER_BUDGET_MIB
}

// ===========================================================================
// F419 — 弱网优雅模式：信号差时降级
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeakNetTier {
    Full,
    Text,
    Offline,
}

/// 信号 permille：<300 离线兜底，<600 纯文本。
pub fn weak_net_tier(signal_permille: u16) -> WeakNetTier {
    if signal_permille >= 600 {
        WeakNetTier::Full
    } else if signal_permille >= 300 {
        WeakNetTier::Text
    } else {
        WeakNetTier::Offline
    }
}

// ===========================================================================
// F420 — 网络回归金样：固定输入期望输出
// ===========================================================================

#[derive(Clone, Copy)]
pub struct GoldenCase {
    pub name: &'static str,
    pub input_a: u32,
    pub input_b: u32,
    pub expect: u32,
}

/// 金样回归：公平分配结果总和必须恰为 1000‰。
pub fn golden_total_permille(weights: &[u32]) -> u32 {
    let mut out = [0u16; 8];
    if !fair_share_permille(weights, &mut out[..weights.len()]) {
        return 0;
    }
    out[..weights.len()].iter().map(|&v| v as u32).sum()
}

// ===========================================================================
// F421 — 带宽预算：月度配额
// ===========================================================================

pub const MONTHLY_BUDGET_MIB: u32 = 50 * 1024;

/// 用量 permille 与超限判定（一次计算，中间量存变量）。
pub fn budget_status(used_mib: u32) -> (u16, bool) {
    let permille = if used_mib >= MONTHLY_BUDGET_MIB {
        1000
    } else {
        (used_mib * 1000 / MONTHLY_BUDGET_MIB) as u16
    };
    let over = used_mib > MONTHLY_BUDGET_MIB;
    (permille, over)
}

// ===========================================================================
// F422 — 连接状态叙述者：人话状态
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Down,
    Captive,
    Limited,
    Online,
}

pub fn link_narration(s: LinkState) -> &'static str {
    match s {
        LinkState::Down => "未连接",
        LinkState::Captive => "需要登录认证",
        LinkState::Limited => "已连接但受限",
        LinkState::Online => "在线",
    }
}

// ===========================================================================
// F423 — 网络故障剧本：症状→动作
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetSymptom {
    NoDhcp,
    DnsFail,
    SlowThroughput,
    FrequentDrops,
}

/// 剧本第一步：按症状给出处置动作编号。
pub fn playbook_first_step(s: NetSymptom) -> u8 {
    match s {
        NetSymptom::NoDhcp => 1,          // 检查链路/DHCP
        NetSymptom::DnsFail => 2,         // 换 DNS 探测
        NetSymptom::SlowThroughput => 3,  // 带宽诊断
        NetSymptom::FrequentDrops => 4,   // 漫游/驱动排查
    }
}

// ===========================================================================
// F424 — IPv6 全栈：地址压缩判定
// ===========================================================================

/// IPv6 地址段（8 组 u16）是否全零段可压缩（存在连续 ≥1 组全零）。
pub fn ipv6_compressible(groups: &[u16; 8]) -> bool {
    let mut run = 0usize;
    let mut best = 0usize;
    for g in groups {
        if *g == 0 {
            run += 1;
            if run > best {
                best = run;
            }
        } else {
            run = 0;
        }
    }
    best >= 1
}

/// EUI-64 接口标识 U/L 位翻转（第 7 位）。
pub fn ipv6_eui64_flipped(mac_hi: u16, _mac_lo: u32) -> u16 {
    mac_hi ^ 0x0200
}

// ===========================================================================
// F425 — 网络年报：年度统计板块
// ===========================================================================

pub const NET_REPORT_SECTIONS: [&str; 4] = ["traffic", "uptime", "incidents", "top-apps"];

/// 年报就绪：板块齐 + 可用率 permille ≥ 990。
pub fn net_report_ready(sections: &[&str], uptime_permille: u16) -> bool {
    NET_REPORT_SECTIONS.iter().all(|s| sections.contains(s)) && uptime_permille >= 990
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600net_checks() -> CheckSet {
    let mut set = CheckSet::new("m600net");

    // F401 网络栈
    set.add("F401 mtu v4", frame_sendable(1500, false) && !frame_sendable(1501, false), "1500 cap");
    set.add("F401 mtu v6", frame_sendable(1280, true) && !frame_sendable(1281, true), "v6 cap 1280");
    let sum = inet_checksum(&[0x01, 0x02, 0x03, 0x04]);
    set.add("F401 checksum", inet_checksum(&[]) == 0xFFFF && sum != 0, "ones-complement");

    // F402 漫游
    set.add(
        "F402 roam hysteresis",
        roam_should_switch(500, 610) && !roam_should_switch(500, 590) && roam_should_switch(900, 1000),
        "100‰ band",
    );

    // F403 诊断室
    let good = ProbeResult { sent: 10, received: 9, rtt_max_ms: 120 };
    let bad = ProbeResult { sent: 10, received: 2, rtt_max_ms: 900 };
    set.add("F403 probe healthy", good.healthy() && !bad.healthy(), "loss+rtt");
    set.add("F403 loss calc", good.loss_permille() == 100 && bad.loss_permille() == 800, "permille");

    // F404 公平调度
    let mut shares = [0u16; 3];
    let ok = fair_share_permille(&[1, 1, 2], &mut shares);
    set.add("F404 fair share", ok && shares[0] == 250 && shares[1] == 250 && shares[2] == 500, "25/25/50");
    let mut shares2 = [0u16; 3];
    fair_share_permille(&[1, 1, 1], &mut shares2);
    set.add(
        "F404 remainder",
        shares2[0] == 334 && shares2[1] == 333 && shares2[2] == 333,
        "remainder to first",
    );

    // F405 流量可视化
    let traffic = [
        AppTraffic { app_id: "sync", kib: 900 },
        AppTraffic { app_id: "video", kib: 4000 },
        AppTraffic { app_id: "mail", kib: 12 },
    ];
    set.add("F405 top talker", top_talker(&traffic) == 1 && traffic[1].app_id == "video", "aggregated");

    // F406 防火墙
    let rules = [
        FwRule { port: 80, action: FirewallAction::Allow },
        FwRule { port: 23, action: FirewallAction::Deny },
    ];
    set.add(
        "F406 firewall",
        firewall_decide(&rules, 80) == FirewallAction::Allow
            && firewall_decide(&rules, 443) == FirewallAction::Deny
            && firewall_decide(&rules, 23) == FirewallAction::Deny,
        "default deny",
    );

    // F407 VPN
    let tunnels = [
        VpnTunnel { id: 1, metric: 30, up: true },
        VpnTunnel { id: 2, metric: 10, up: true },
        VpnTunnel { id: 3, metric: 5, up: false },
    ];
    set.add(
        "F407 vpn select",
        vpn_select(&tunnels) == Some(2)
            && vpn_select(&[VpnTunnel { id: 9, metric: 1, up: false }]).is_none(),
        "lowest live metric",
    );

    // F408 代理发现
    set.add(
        "F408 proxy pac",
        proxy_decide(&[".internal", "localhost"], "db.internal") == ProxyDecision::Direct
            && proxy_decide(&[".internal"], "example.com") == ProxyDecision::ViaProxy
            && proxy_decide(&["localhost"], "localhost") == ProxyDecision::Direct,
        "suffix exceptions",
    );

    // F409 DNS 策略
    set.add(
        "F409 dns path",
        dns_pick_path(100, false) == DnsPath::Cache
            && dns_pick_path(600, false) == DnsPath::Plain
            && dns_pick_path(0, true) == DnsPath::Encrypted,
        "cache/plain/enc",
    );

    // F410 离线优先
    let mut q = OfflineQueue::new();
    let mut enq_ok = true;
    for _ in 0..OFFLINE_QUEUE_MAX {
        enq_ok = enq_ok && q.enqueue();
    }
    let full_reject = !q.enqueue();
    let flushed = q.flush(10);
    set.add(
        "F410 offline queue",
        enq_ok && full_reject && flushed == 10 && q.len() == OFFLINE_QUEUE_MAX - 10,
        "local-first",
    );

    // F411 局域网发现
    set.add(
        "F411 announce ttl",
        announce_ttl_after(1, 64) == 32 && announce_ttl_after(2, 64) == 16 && announce_ttl_after(9, 3) == 0,
        "halve decay",
    );

    // F412 附近共享
    set.add(
        "F412 handshake",
        share_step_ok(0, 1) && share_step_ok(2, 3) && !share_step_ok(0, 2) && !share_step_ok(3, 0),
        "ordered 4 steps",
    );

    // F413 手机伴侣
    set.add(
        "F413 phone mirror",
        phone_mirror_fold(81, 100) && !phone_mirror_fold(50, 100) && !phone_mirror_fold(0, 0),
        "fold at 80%+",
    );

    // F414 剪贴板
    set.add(
        "F414 clip sync",
        clip_syncable(ClipItem { size_bytes: 1024, marked_sensitive: false })
            && !clip_syncable(ClipItem { size_bytes: 64, marked_sensitive: true })
            && !clip_syncable(ClipItem { size_bytes: 2 * 1024 * 1024, marked_sensitive: false }),
        "sensitive/size gate",
    );

    // F415 时间诚实化
    set.add(
        "F415 ntp drift",
        ntp_drift_warn(2500) && ntp_drift_warn(-3000) && !ntp_drift_warn(1500) && !ntp_drift_warn(-1999),
        "±2s warn",
    );

    // F416 证书管家
    set.add(
        "F416 cert renew",
        cert_needs_renew(14) && cert_needs_renew(3) && !cert_needs_renew(15),
        "14d window",
    );

    // F417 网络沙盒
    let mut sb = NetSandbox::new();
    sb.allow(443);
    sb.allow(80);
    set.add(
        "F417 net sandbox",
        sb.can_connect(443) && !sb.can_connect(22),
        "port whitelist",
    );

    // F418 热点礼仪
    set.add(
        "F418 hotspot",
        hotspot_over_budget(2049) && !hotspot_over_budget(2048),
        "2GiB budget",
    );

    // F419 弱网模式
    set.add(
        "F419 weak net tier",
        weak_net_tier(900) == WeakNetTier::Full
            && weak_net_tier(450) == WeakNetTier::Text
            && weak_net_tier(100) == WeakNetTier::Offline,
        "auto degrade",
    );

    // F420 金样回归
    set.add(
        "F420 golden cases",
        golden_total_permille(&[1, 1, 2]) == 1000
            && golden_total_permille(&[1, 1, 1]) == 1000
            && golden_total_permille(&[7]) == 1000
            && golden_total_permille(&[0, 0]) == 0,
        "sums == 1000‰",
    );

    // F421 带宽预算
    let (pm, over) = budget_status(MONTHLY_BUDGET_MIB / 2);
    let (pm_full, over_full) = budget_status(MONTHLY_BUDGET_MIB + 1);
    set.add("F421 budget half", pm == 500 && !over, "50%");
    set.add("F421 budget over", pm_full == 1000 && over_full, "clamped+over");

    // F422 状态叙述
    set.add(
        "F422 link narration",
        link_narration(LinkState::Captive) == "需要登录认证"
            && link_narration(LinkState::Online) == "在线"
            && link_narration(LinkState::Limited) != link_narration(LinkState::Online),
        "human words",
    );

    // F423 故障剧本
    set.add(
        "F423 playbook",
        playbook_first_step(NetSymptom::NoDhcp) == 1
            && playbook_first_step(NetSymptom::DnsFail) == 2
            && playbook_first_step(NetSymptom::SlowThroughput) == 3
            && playbook_first_step(NetSymptom::FrequentDrops) == 4,
        "symptom routing",
    );

    // F424 IPv6
    let v6a = [0x2001, 0x0DB8, 0x0000, 0x0000, 0x0000, 0x0000, 0x0001, 0x0002];
    let v6b = [0x2001, 0x0DB8, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006];
    set.add(
        "F424 ipv6 compress",
        ipv6_compressible(&v6a) && !ipv6_compressible(&v6b),
        "zero run",
    );
    set.add("F424 eui64 ul-bit", ipv6_eui64_flipped(0x00AA, 0) == 0x02AA, "flip 7th bit");

    // F425 网络年报
    set.add(
        "F425 net report",
        net_report_ready(&NET_REPORT_SECTIONS, 995)
            && !net_report_ready(&NET_REPORT_SECTIONS, 989)
            && !net_report_ready(&["traffic"], 999),
        "sections+uptime",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f401_checksum_bytes() {
        // 单字节尾数也要进和（高位对齐）。
        let sum = inet_checksum(&[0xFF]);
        assert_eq!(sum, !(0xFF00u16));
        assert_eq!(inet_checksum(&[0x00, 0x00]), 0xFFFF);
    }

    #[test]
    fn f402_roam_boundary() {
        assert!(roam_should_switch(0, 100));
        assert!(!roam_should_switch(0, 99));
        assert!(!roam_should_switch(1000, 999)); // 已满格无处可升
    }

    #[test]
    fn f404_fair_share_zero_weights() {
        let mut out = [0u16; 2];
        assert!(!fair_share_permille(&[0, 0], &mut out));
        let mut out2 = [0u16; 1];
        assert!(!fair_share_permille(&[1, 2], &mut out2)); // out 太短
    }

    #[test]
    fn f407_vpn_all_down() {
        let ts = [
            VpnTunnel { id: 1, metric: 1, up: false },
            VpnTunnel { id: 2, metric: 2, up: false },
        ];
        assert!(vpn_select(&ts).is_none());
    }

    #[test]
    fn f419_tier_boundaries() {
        assert_eq!(weak_net_tier(600), WeakNetTier::Full);
        assert_eq!(weak_net_tier(599), WeakNetTier::Text);
        assert_eq!(weak_net_tier(299), WeakNetTier::Offline);
    }

    #[test]
    fn f425_domain_selfcheck_all_pass() {
        let set = run_m600net_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
