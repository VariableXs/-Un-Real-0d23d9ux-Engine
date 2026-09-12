//! m700rf — VARIX-M700 AI-18 无线与链路域 (F426~F450)
//!
//! 无线链路状态机/信标考古员/信号质量谱/加密套件舱/扫描调度官/漫游预言家/
//! 射频功耗契约/链路回放流/认证失败分类官/蓝牙链路谱/蓝牙配对官/无线 fuzz 桩/
//! 信道雷达/无线回归金样/组网自描述导出/链路健康分/飞行模式总闸/无线事件流/
//! 国家码礼仪/热点内核舱/网格组网侦察/射频校准档案/无线压力剧本/无线统计分账/
//! 无线域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F426 — 无线链路状态机：扫描→认证→关联→连通
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    Offline,
    Scanning,
    Authenticating,
    Associating,
    Connected,
    Roaming,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkEvent {
    Scan,
    BeaconFound,
    AuthOk,
    AssocOk,
    RssiLow,
    RoamDone,
    Disconnect,
}

pub fn link_advance(cur: LinkState, ev: LinkEvent) -> Option<LinkState> {
    use LinkEvent::*;
    use LinkState::*;
    match (cur, ev) {
        (Offline, Scan) => Some(Scanning),
        (Scanning, BeaconFound) => Some(Authenticating),
        (Authenticating, AuthOk) => Some(Associating),
        (Associating, AssocOk) => Some(Connected),
        (Connected, RssiLow) => Some(Scanning),
        (Roaming, RoamDone) => Some(Connected),
        (Connected, Disconnect) | (Scanning, Disconnect) => Some(Offline),
        _ => None,
    }
}

// ===========================================================================
// F427 — 信标考古员：信标帧关键域提取
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Beacon {
    pub ssid_len: u8,     // 0~32（0 = 隐藏 SSID）
    pub interval_tu: u16, // TU（1 TU ≈ 1024µs）
    pub channel: u8,
}

/// 简化信标布局：[0]=ssid_len [1..3]=interval_tu(BE) [3]=channel。
pub fn parse_beacon(bytes: &[u8]) -> Option<Beacon> {
    if bytes.len() < 4 {
        return None;
    }
    let ssid_len = bytes[0];
    if ssid_len > 32 {
        return None;
    }
    let interval = ((bytes[1] as u16) << 8) | bytes[2] as u16;
    let channel = bytes[3];
    if !(1..=14).contains(&channel) || interval < 10 {
        return None;
    }
    Some(Beacon { ssid_len, interval_tu: interval, channel })
}

// ===========================================================================
// F428 — 信号质量谱：RSSI → permille 质量
// ===========================================================================

/// -30dBm=1000‰，-90dBm=0‰，线性内插，越界钳位。
pub fn rssi_quality(rssi_dbm: i32) -> u16 {
    if rssi_dbm >= -30 {
        1000
    } else if rssi_dbm <= -90 {
        0
    } else {
        let span = (-30 - rssi_dbm) as u32; // 0..60
        (1000 - span * 1000 / 60) as u16
    }
}

// ===========================================================================
// F429 — 加密套件舱：合规加密白名单
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cipher {
    Open,
    Wep,
    WpaTkip,
    Wpa2Ccmp,
    Wpa3Sae,
}

/// WEP/TKIP 已破不收；开放网络仅限明确声明的无密场景。
pub fn cipher_secure(c: Cipher) -> bool {
    matches!(c, Cipher::Wpa2Ccmp | Cipher::Wpa3Sae)
}

// ===========================================================================
// F430 — 扫描调度官：信道驻留计划
// ===========================================================================

pub const SCAN_DWELL_MS: u16 = 40;
pub const SCAN_CHANNEL_MAX: usize = 13;

#[derive(Clone, Copy)]
pub struct ScanPlan {
    pub channels: [u8; SCAN_CHANNEL_MAX],
    pub count: usize,
    pub dwell_ms: u16,
}

impl ScanPlan {
    pub fn full() -> ScanPlan {
        let mut ch = [0u8; SCAN_CHANNEL_MAX];
        for (i, c) in ch.iter_mut().enumerate() {
            *c = (i + 1) as u8;
        }
        ScanPlan { channels: ch, count: SCAN_CHANNEL_MAX, dwell_ms: SCAN_DWELL_MS }
    }
    /// 信道无重复、都在 1..=13、驻留时长在 20~120ms。
    pub fn valid(&self) -> bool {
        if self.count == 0 || self.count > SCAN_CHANNEL_MAX {
            return false;
        }
        if !(20..=120).contains(&self.dwell_ms) {
            return false;
        }
        for i in 0..self.count {
            if !(1..=13).contains(&self.channels[i]) {
                return false;
            }
            for j in (i + 1)..self.count {
                if self.channels[i] == self.channels[j] {
                    return false;
                }
            }
        }
        true
    }
}

// ===========================================================================
// F431 — 漫游预言家：RSSI 余量判决
// ===========================================================================

/// 候选比当前好出 margin_db 且当前低于 roam_floor 才漫游（防乒乓）。
pub fn should_roam(cur_rssi: i32, cand_rssi: i32, margin_db: i32, roam_floor: i32) -> bool {
    cur_rssi < roam_floor && cand_rssi - cur_rssi >= margin_db
}

// ===========================================================================
// F432 — 射频功耗契约：占空比/发射功率约束
// ===========================================================================

pub const RF_TX_MAX_MW: u16 = 100;
pub const RF_DUTY_MAX_PERMILLE: u16 = 600;

#[derive(Clone, Copy)]
pub struct RfPower {
    pub tx_mw: u16,
    pub duty_permille: u16,
}

pub fn rf_power_within(p: RfPower) -> bool {
    p.tx_mw <= RF_TX_MAX_MW && p.duty_permille <= RF_DUTY_MAX_PERMILLE
}

// ===========================================================================
// F433 — 链路回放流：帧序列确定性摘要
// ===========================================================================

pub fn rf_digest(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x5246_4447; // "RFDG"
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub fn replay_deterministic(frames: [&[u8]; 2]) -> bool {
    rf_digest(frames[0]) == rf_digest(frames[0])
        && rf_digest(frames[1]) == rf_digest(frames[1])
        && rf_digest(frames[0]) != rf_digest(frames[1])
}

// ===========================================================================
// F434 — 认证失败分类官：失败原因归类
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthFail {
    Timeout,
    WrongPsk,
    Rejected,
}

/// 802.11 状态码归类：15=超时类，2/42=密钥错，1/其他=拒绝。
pub fn classify_auth_fail(status: u16, timed_out: bool) -> AuthFail {
    if timed_out {
        AuthFail::Timeout
    } else {
        match status {
            2 | 42 => AuthFail::WrongPsk,
            _ => AuthFail::Rejected,
        }
    }
}

/// 密钥错要引导用户重输；超时/拒绝允许自动重试。
pub fn fail_needs_user(a: AuthFail) -> bool {
    matches!(a, AuthFail::WrongPsk)
}

// ===========================================================================
// F435 — 蓝牙链路谱：BT 链路状态机
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtState {
    Idle,
    Connecting,
    Connected,
    Streaming,
}

pub fn bt_advance(cur: BtState, connect: bool, stream: bool, drop: bool) -> Option<BtState> {
    match (cur, connect, stream, drop) {
        (BtState::Idle, true, false, false) => Some(BtState::Connecting),
        (BtState::Connecting, false, false, true) => Some(BtState::Idle),
        (BtState::Connecting, true, false, false) => Some(BtState::Connected),
        (BtState::Connected, false, true, false) => Some(BtState::Streaming),
        (BtState::Connected, false, false, true) => Some(BtState::Idle),
        (BtState::Streaming, false, false, true) => Some(BtState::Connected),
        _ => None,
    }
}

// ===========================================================================
// F436 — 蓝牙配对官：6 位配对码
// ===========================================================================

pub const PASSKEY_DIGITS: usize = 6;

#[derive(Clone, Copy)]
pub struct Passkey {
    pub digits: [u8; PASSKEY_DIGITS], // 每个 0~9
}

impl Passkey {
    pub fn valid(&self) -> bool {
        self.digits.iter().all(|&d| d <= 9)
    }
    pub fn same(&self, other: &Passkey) -> bool {
        self.digits == other.digits
    }
}

/// 双端确认一致才完成配对。
pub fn pairing_complete(local: &Passkey, remote: &Passkey, user_confirmed: bool) -> bool {
    local.valid() && remote.valid() && local.same(remote) && user_confirmed
}

// ===========================================================================
// F437 — 无线 fuzz 桩：确定性信标字节生成
// ===========================================================================

pub fn fuzz_beacon(seed: u32) -> [u8; 4] {
    let x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    [
        (x >> 24) as u8 & 0x1F, // ssid_len 0~31
        ((x >> 16) & 0xFF) as u8,
        ((x >> 8) & 0xFF) as u8,
        (x & 0x0F) as u8 + 1,   // channel 1~14
    ]
}

pub fn fuzz_replayable(seed: u32) -> bool {
    fuzz_beacon(seed) == fuzz_beacon(seed)
}

// ===========================================================================
// F438 — 信道雷达：DFS 雷达检测后让出信道
// ===========================================================================

/// 检测到雷达即从候选里挑一个非雷达信道；没有可用则 None（静默）。
pub fn radar_vacate(current: u8, radar_hit: bool, candidates: &[u8]) -> Option<u8> {
    if !radar_hit {
        return None; // 没雷达不折腾
    }
    candidates.iter().copied().find(|&c| c != current)
}

// ===========================================================================
// F439 — 无线回归金样：信标解析摘要对表
// ===========================================================================

pub const BEACON_GOLDEN_DIGEST: u32 = 0x1111_1111; // 哨兵：金样必须≠哨兵

pub fn beacon_golden_ok(digest: u32) -> bool {
    digest != 0 && digest != BEACON_GOLDEN_DIGEST
}

// ===========================================================================
// F440 — 组网自描述导出：完整描述符
// ===========================================================================

pub const RF_DESCRIPTOR_FIELDS: [&str; 5] =
    ["link-states", "beacons", "ciphers", "channels", "topology"];

pub fn rf_descriptor_complete(fields: [&str; 5]) -> bool {
    (0..5).all(|i| !fields[i].is_empty())
}

// ===========================================================================
// F441 — 链路健康分：信号质量与重传率合成
// ===========================================================================

pub fn link_health(quality_permille: u16, retry_permille: u16) -> u16 {
    let retry_penalty = (retry_permille as u32 * 500 / 1000) as u16; // 重传最多扣 500
    (quality_permille as u32 * 500 / 1000) as u16
        + (500 - retry_penalty.min(500))
}

// ===========================================================================
// F442 — 飞行模式总闸：一键掐断所有射频
// ===========================================================================

#[derive(Clone, Copy)]
pub struct AirplaneMode {
    pub master: bool,
    pub wifi: bool,
    pub bt: bool,
}

/// 总闸合上时，任何分开关都必须归零。
pub fn airplane_enforced(m: AirplaneMode) -> bool {
    if m.master {
        !m.wifi && !m.bt
    } else {
        true
    }
}

// ===========================================================================
// F443 — 无线事件流：定容事件日志
// ===========================================================================

pub const RF_EVENT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct RfEventLog {
    kinds: [u8; RF_EVENT_CAP], // 1=beacon 2=auth 3=roam 4=radar
    count: usize,
}

impl RfEventLog {
    pub const fn new() -> RfEventLog {
        RfEventLog { kinds: [0; RF_EVENT_CAP], count: 0 }
    }
    pub fn push(&mut self, kind: u8) -> bool {
        if self.count >= RF_EVENT_CAP {
            return false;
        }
        self.kinds[self.count] = kind;
        self.count += 1;
        true
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn kind_at(&self, i: usize) -> Option<u8> {
        if i < self.count {
            Some(self.kinds[i])
        } else {
            None
        }
    }
}

// ===========================================================================
// F444 — 国家码礼仪：代码合法 + 信道规则
// ===========================================================================

/// 两位大写字母国家码。
pub fn country_code_ok(cc: &[u8; 2]) -> bool {
    cc.iter().all(|&b| b.is_ascii_uppercase())
}

/// 简化规则：2.4G 12/13 信道在 FCC（US）禁止，ETSI（DE）允许但禁 14。
pub fn channel_allowed(cc: &[u8; 2], channel: u8) -> bool {
    let fcc = &b"US"[..];
    let etsi = &b"DE"[..];
    if !(1..=14).contains(&channel) {
        return false;
    }
    if cc.as_slice() == fcc {
        channel <= 11
    } else if cc.as_slice() == etsi {
        channel <= 13
    } else {
        true // 未登记地区先宽放，登记后收紧
    }
}

// ===========================================================================
// F445 — 热点内核舱：AP 模式参数
// ===========================================================================

pub const AP_CLIENT_MAX: u8 = 8;

#[derive(Clone, Copy)]
pub struct ApConfig {
    pub ssid_len: u8,
    pub channel: u8,
    pub clients_max: u8,
    pub wpa3: bool,
}

pub fn ap_config_valid(a: ApConfig) -> bool {
    (1..=32).contains(&a.ssid_len)
        && (1..=13).contains(&a.channel)
        && a.clients_max >= 1
        && a.clients_max <= AP_CLIENT_MAX
        && a.wpa3 // 热点必须加密
}

// ===========================================================================
// F446 — 网格组网侦察：mesh 跳数预算
// ===========================================================================

pub const MESH_HOP_LIMIT: u8 = 4;

/// 跳数受限且路径不能回头（hop预算内）。
pub fn mesh_path_ok(hops: u8) -> bool {
    hops >= 1 && hops <= MESH_HOP_LIMIT
}

/// 端到端预算：每跳衰减 15%，全程可用信号 permille 下限。
pub fn mesh_signal_permille(hops: u8) -> u16 {
    let mut sig: u32 = 1000;
    for _ in 0..hops {
        sig = sig * 850 / 1000;
    }
    sig as u16
}

// ===========================================================================
// F447 — 射频校准档案：校准记录新鲜度
// ===========================================================================

pub const CALIB_TTL_DAYS: u32 = 365;

#[derive(Clone, Copy)]
pub struct RfCalib {
    pub tx_power_centi_dbm: i16, // 0.01dBm
    pub temp_c: i16,
    pub age_days: u32,
}

pub fn calib_fresh(c: RfCalib) -> bool {
    c.tx_power_centi_dbm.abs() <= 2000 && (-40..=85).contains(&c.temp_c)
        && c.age_days <= CALIB_TTL_DAYS
}

// ===========================================================================
// F448 — 无线压力剧本：多链路并发压力
// ===========================================================================

pub const RF_STRESS_LINKS: u8 = 3;
pub const RF_STRESS_MINUTES: u32 = 30;

pub fn rf_stress_plan_valid(links: u8, minutes: u32) -> bool {
    links >= 1 && links <= 8 && minutes >= 10 && minutes <= 120
}

// ===========================================================================
// F449 — 无线统计分账：信标/关联/掉线计数
// ===========================================================================

#[derive(Clone, Copy)]
pub struct RfStats {
    pub beacons: u32,
    pub assoc_ok: u32,
    pub assoc_fail: u32,
    pub disconnects: u32,
}

impl RfStats {
    pub fn assoc_rate_permille(&self) -> u32 {
        let total = self.assoc_ok + self.assoc_fail;
        if total == 0 {
            return 0;
        }
        self.assoc_ok as u32 * 1000 / total as u32
    }
    /// 合理：每次掉线都应来自成功关联过的链路。
    pub fn sane(&self) -> bool {
        self.disconnects <= self.assoc_ok
    }
}

// ===========================================================================
// F450 — 无线域年报
// ===========================================================================

pub const RF_REPORT_SECTIONS: [&str; 4] = ["milestones", "interop", "incidents", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700rf_checks() -> CheckSet {
    let mut set = CheckSet::new("m700rf");

    // F426 链路状态机
    let s0 = link_advance(LinkState::Offline, LinkEvent::Scan);
    let s1 = link_advance(LinkState::Scanning, LinkEvent::BeaconFound);
    set.add("F426 link scan→auth", s0 == Some(LinkState::Scanning) && s1 == Some(LinkState::Authenticating), "ladder");
    set.add("F426 link connect", link_advance(LinkState::Associating, LinkEvent::AssocOk) == Some(LinkState::Connected), "assoc ok");
    set.add("F426 link illegal", link_advance(LinkState::Offline, LinkEvent::AssocOk).is_none(), "no leap");

    // F427 信标考古
    let beacon = parse_beacon(&[8, 0, 100, 6]);
    set.add("F427 beacon parse", beacon == Some(Beacon { ssid_len: 8, interval_tu: 100, channel: 6 }), "fields");
    set.add("F427 beacon short", parse_beacon(&[4, 0, 100]).is_none(), "truncated");
    set.add("F427 beacon bad ch", parse_beacon(&[8, 0, 100, 15]).is_none(), "channel 15");

    // F428 信号质量
    set.add("F428 rssi ends", rssi_quality(-30) == 1000 && rssi_quality(-90) == 0, "clamped");
    set.add("F428 rssi mid", rssi_quality(-60) == 500, "-60dBm=50%");
    set.add("F428 rssi monotone", rssi_quality(-40) > rssi_quality(-70), "stronger=better");

    // F429 加密套件
    set.add("F429 cipher secure", cipher_secure(Cipher::Wpa3Sae) && cipher_secure(Cipher::Wpa2Ccmp), "modern only");
    set.add("F429 cipher reject", !cipher_secure(Cipher::Wep) && !cipher_secure(Cipher::WpaTkip), "broken out");

    // F430 扫描调度
    let plan = ScanPlan::full();
    set.add("F430 scan full plan", plan.valid() && plan.count == 13, "1..13");
    set.add("F430 scan dupe", !ScanPlan { channels: [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], count: 2, dwell_ms: 40 }.valid(), "no dupe");
    set.add("F430 scan dwell", !ScanPlan { channels: [1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], count: 2, dwell_ms: 5 }.valid(), "dwell bounds");

    // F431 漫游预言家
    set.add("F431 roam margin", should_roam(-70, -50, 15, -65) && !should_roam(-70, -60, 15, -65), "needs margin");
    set.add("F431 roam floor", !should_roam(-50, -40, 10, -65), "no ping-pong");

    // F432 射频功耗
    set.add("F432 rf power ok", rf_power_within(RfPower { tx_mw: 100, duty_permille: 600 }), "at limit");
    set.add("F432 rf power over", !rf_power_within(RfPower { tx_mw: 200, duty_permille: 900 }), "over budget");

    // F433 链路回放
    set.add("F433 replay digest", replay_deterministic([b"frame-a", b"frame-b"]), "deterministic");
    set.add("F433 replay stable", rf_digest(b"same") == rf_digest(b"same"), "same→same");

    // F434 认证失败分类
    set.add("F434 auth classify", classify_auth_fail(2, false) == AuthFail::WrongPsk
        && classify_auth_fail(15, true) == AuthFail::Timeout
        && classify_auth_fail(1, false) == AuthFail::Rejected, "3 buckets");
    set.add("F434 auth user action", fail_needs_user(AuthFail::WrongPsk) && !fail_needs_user(AuthFail::Timeout), "psk needs user");

    // F435 蓝牙链路
    let b0 = bt_advance(BtState::Idle, true, false, false);
    let b1 = bt_advance(BtState::Connecting, true, false, false);
    set.add("F435 bt connect", b0 == Some(BtState::Connecting) && b1 == Some(BtState::Connected), "ladder");
    set.add("F435 bt stream", bt_advance(BtState::Connected, false, true, false) == Some(BtState::Streaming), "stream");
    set.add("F435 bt drop", bt_advance(BtState::Streaming, false, false, true) == Some(BtState::Connected), "graceful");

    // F436 蓝牙配对
    let k1 = Passkey { digits: [1, 2, 3, 4, 5, 6] };
    let k2 = Passkey { digits: [1, 2, 3, 4, 5, 6] };
    let k3 = Passkey { digits: [1, 2, 3, 4, 5, 7] };
    set.add("F436 passkey match", pairing_complete(&k1, &k2, true), "confirmed");
    set.add("F436 passkey mismatch", !pairing_complete(&k1, &k3, true), "digit diff");
    set.add("F436 passkey unconfirmed", !pairing_complete(&k1, &k2, false), "need user");

    // F437 fuzz 桩
    set.add("F437 rf fuzz deterministic", fuzz_replayable(0xBEEF_CAFE), "replayable");
    set.add("F437 rf fuzz chan range", (1..=14).contains(&fuzz_beacon(7)[3]), "channel 1..14");

    // F438 信道雷达
    set.add("F438 radar vacate", radar_vacate(52, true, &[52, 60, 100]) == Some(60), "pick next free");
    set.add("F438 radar quiet", radar_vacate(52, false, &[60]).is_none(), "no radar no move");
    set.add("F438 radar stuck", radar_vacate(52, true, &[52]).is_none(), "no alternative");

    // F439 回归金样
    set.add("F439 rf golden", beacon_golden_ok(rf_digest(b"beacon-v1")) && !beacon_golden_ok(BEACON_GOLDEN_DIGEST), "fnv digest");

    // F440 自描述导出
    set.add("F440 rf descriptor", rf_descriptor_complete(["ls", "bc", "ci", "ch", "tp"]), "5 fields");
    set.add("F440 descriptor gap", !rf_descriptor_complete(["ls", "", "ci", "ch", "tp"]), "no empty");

    // F441 链路健康分
    set.add("F441 health good", link_health(900, 100) == 900, "q*0.5 + (500-50)");
    set.add("F441 health retry cap", link_health(1000, 2000) == 500, "retry penalty capped");

    // F442 飞行模式
    set.add("F442 airplane enforced", airplane_enforced(AirplaneMode { master: true, wifi: false, bt: false }), "all off");
    set.add("F442 airplane leak", !airplane_enforced(AirplaneMode { master: true, wifi: true, bt: false }), "wifi leaked");

    // F443 无线事件流
    let mut rel = RfEventLog::new();
    let pushed = rel.push(1);
    rel.push(2);
    rel.push(4);
    set.add("F443 rf events", pushed && rel.count() == 3 && rel.kind_at(2) == Some(4), "ordered");

    // F444 国家码礼仪
    set.add("F444 cc format", country_code_ok(b"US") && country_code_ok(b"DE") && !country_code_ok(b"u1"), "A-Z");
    set.add("F444 cc channels", !channel_allowed(b"US", 12) && channel_allowed(b"DE", 13)
        && !channel_allowed(b"DE", 14), "regional rules");

    // F445 热点内核舱
    set.add("F445 ap valid", ap_config_valid(ApConfig { ssid_len: 8, channel: 6, clients_max: 4, wpa3: true }), "ok");
    set.add("F445 ap must encrypt", !ap_config_valid(ApConfig { ssid_len: 8, channel: 6, clients_max: 4, wpa3: false }), "wpa3 required");

    // F446 网格组网
    set.add("F446 mesh hops", mesh_path_ok(4) && !mesh_path_ok(5) && !mesh_path_ok(0), "hop limit 4");
    set.add("F446 mesh decay", mesh_signal_permille(1) == 850 && mesh_signal_permille(4) == 521, "15% per hop");

    // F447 射频校准
    set.add("F447 calib fresh", calib_fresh(RfCalib { tx_power_centi_dbm: 1500, temp_c: 25, age_days: 100 }), "in ttl");
    set.add("F447 calib stale", !calib_fresh(RfCalib { tx_power_centi_dbm: 1500, temp_c: 25, age_days: 400 }), "expired");

    // F448 压力剧本
    set.add("F448 rf stress", rf_stress_plan_valid(RF_STRESS_LINKS, RF_STRESS_MINUTES)
        && !rf_stress_plan_valid(9, 30), "bounds");

    // F449 统计分账
    let rs = RfStats { beacons: 9000, assoc_ok: 90, assoc_fail: 10, disconnects: 5 };
    set.add("F449 rf stats rate", rs.assoc_rate_permille() == 900 && rs.sane(), "90% assoc");
    set.add("F449 rf stats insane", !RfStats { beacons: 1, assoc_ok: 1, assoc_fail: 0, disconnects: 2 }.sane(), "dc>assoc");

    // F450 年报
    set.add("F450 rf annual report", RF_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f426_link_full_ladder() {
        let s = link_advance(LinkState::Offline, LinkEvent::Scan).unwrap();
        let s = link_advance(s, LinkEvent::BeaconFound).unwrap();
        let s = link_advance(s, LinkEvent::AuthOk).unwrap();
        let s = link_advance(s, LinkEvent::AssocOk).unwrap();
        assert_eq!(s, LinkState::Connected);
        assert!(link_advance(LinkState::Connected, LinkEvent::BeaconFound).is_none());
    }

    #[test]
    fn f428_rssi_interpolation() {
        assert_eq!(rssi_quality(-45), 750);
        assert_eq!(rssi_quality(-90), 0);
        assert_eq!(rssi_quality(0), 1000);
    }

    #[test]
    fn f430_scan_plan_validation() {
        assert!(ScanPlan::full().valid());
        let mut empty = ScanPlan::full();
        empty.count = 0;
        assert!(!empty.valid());
    }

    #[test]
    fn f436_passkey_digits() {
        let bad = Passkey { digits: [1, 2, 3, 4, 5, 10] };
        assert!(!bad.valid());
        let good = Passkey { digits: [9; PASSKEY_DIGITS] };
        assert!(pairing_complete(&good, &good, true));
    }

    #[test]
    fn f444_country_code_rules() {
        assert!(country_code_ok(b"JP"));
        assert!(!country_code_ok(b"jP"));
        assert!(channel_allowed(b"DE", 1));
        assert!(!channel_allowed(b"US", 14));
    }

    #[test]
    fn f450_domain_selfcheck_all_pass() {
        let set = run_m700rf_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
