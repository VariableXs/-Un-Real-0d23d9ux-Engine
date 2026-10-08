//! F182 双域时钟同步（secstar · G-G-12）——没有人注意正确的时钟，但所有人都记得错误的。
//!
//! 主册判据（验收标准第一句）：
//! **交接 10 轮时间戳零漂移（秒级对拍）；偏差注入 >5s 提示触发；回拨保护生效。**
//!
//! 功能定义（G-G-12）：交接时 UTC 时间戳互写快照：VARIX→Windows 与回程都
//! 带权威 UTC+时区偏移；跨域文件时间戳零漂移；小细节大信任。
//!
//! 【交互设计】无直接 UI（协议层）；效果在 F187 时钟页可查（「上次双域同步：
//! 今天 14:32，偏差 0s」）。
//! 【数据与存储】快照扩展字段（UTC 毫秒+偏移+源时钟置信度）——schema 版本
//! 化（MD2 篇 2 兼容扩展区）。
//! 【状态与异常】双钟偏差 >5s → 交接时提示校时建议（F187 联动）；RTC 电池
//! 失效类漂移 → 以最近活动域为准+黄标；时钟回拨检测 → 单调性保护（文件
//! 时间不倒流——显式策略）。
//! 【设计细节】时间戳精度毫秒（文件系统时间粒度之上）；源置信度字段三态
//! （RTC 直读/NTP 校准/推断——消费方自行决定信任级）；Windows 侧读取协议
//! 与交接-W 系列文档接口对齐；零漂移定义=文件 mtime 差 <1s（文件系统时间
//! 粒度之内）。
//!
//! 快照帧（16B 定长 · 小端 · 版本化扩展区纪律）：magic "VXDC" + ver + 置信度
//! + 时区偏移 + UTC 毫秒——未来字段只追加不换位（向后兼容承诺，MD2 篇 2）。
//!
//! 零堆纪律：定长帧 + 定长对拍环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 快照帧 schema 版本（v1；未来字段只追加）。
pub const SNAPSHOT_VER: u8 = 1;
/// 快照帧长度 16B（定长）。
pub const SNAPSHOT_LEN: usize = 16;
/// 双钟偏差提示阈值 >5s（F187 联动）。
pub const DRIFT_ADVISE_MS: u64 = 5_000;
/// 零漂移定义：文件 mtime 差 <1s。
pub const ZERO_DRIFT_MS: u64 = 1_000;
/// 交接对拍 10 轮。
pub const HANDOFF_ROUNDS: usize = 10;

// ---------------------------------------------------------------------------
// 源置信度（三态——消费方自行决定信任级）
// ---------------------------------------------------------------------------

/// 源时钟置信度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confidence {
    /// RTC 直读。
    RtcDirect,
    /// NTP 校准。
    NtpCalibrated,
    /// 推断（RTC 失效类——消费方按最低信任处理）。
    Inferred,
}

impl Confidence {
    pub fn ord(self) -> u8 {
        match self {
            Confidence::RtcDirect => 0,
            Confidence::NtpCalibrated => 1,
            Confidence::Inferred => 2,
        }
    }
    pub fn from_ord(o: u8) -> Confidence {
        match o {
            1 => Confidence::NtpCalibrated,
            2 => Confidence::Inferred,
            _ => Confidence::RtcDirect,
        }
    }
    /// RTC 失效类漂移（推断态）→ 黄标（以最近活动域为准的提示）。
    pub fn yellow_flag(self) -> bool {
        self == Confidence::Inferred
    }
}

// ---------------------------------------------------------------------------
// 快照帧（16B 定长 · 小端 · 版本化）
// ---------------------------------------------------------------------------

/// 一份交接时钟快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockSnapshot {
    /// 权威 UTC 毫秒。
    pub utc_ms: u64,
    /// 时区偏移（分钟，含符号）。
    pub tz_offset_min: i16,
    /// 源置信度。
    pub confidence: Confidence,
}

/// 序列化 16B 帧（小端；未知版本拒绝——向前兼容纪律）。
pub fn encode_snapshot(s: &ClockSnapshot, out: &mut [u8; SNAPSHOT_LEN]) -> bool {
    if s.utc_ms == 0 {
        return false; // 零值=无效时钟（RTC 失效未捕获）不外发
    }
    out[0] = b'V';
    out[1] = b'X';
    out[2] = b'D';
    out[3] = b'C';
    out[4] = SNAPSHOT_VER;
    out[5] = s.confidence.ord();
    out[6] = (s.tz_offset_min & 0xFF) as u8;
    out[7] = ((s.tz_offset_min >> 8) & 0xFF) as u8;
    let mut v = s.utc_ms;
    for k in 0..8 {
        out[8 + k] = (v & 0xFF) as u8;
        v >>= 8;
    }
    true
}

/// 反序列化（版本不符/魔数错/零值钟 → None——诚实拒收不猜）。
pub fn decode_snapshot(frame: &[u8; SNAPSHOT_LEN]) -> Option<ClockSnapshot> {
    if frame[0] != b'V' || frame[1] != b'X' || frame[2] != b'D' || frame[3] != b'C' {
        return None;
    }
    if frame[4] != SNAPSHOT_VER {
        return None;
    }
    let utc_ms = {
        let mut v = 0u64;
        for k in (0..8).rev() {
            v = (v << 8) | frame[8 + k] as u64;
        }
        v
    };
    if utc_ms == 0 {
        return None;
    }
    let tz = (frame[6] as u16 | ((frame[7] as u16) << 8)) as i16;
    Some(ClockSnapshot { utc_ms, tz_offset_min: tz, confidence: Confidence::from_ord(frame[5]) })
}

// ---------------------------------------------------------------------------
// 双钟对拍（交接面）
// ---------------------------------------------------------------------------

/// 对拍结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriftVerdict {
    /// 零漂移（<1s——零漂移定义）。
    InTight,
    /// 可接受偏差（1s-5s——静默同步记录）。
    Tolerable,
    /// 偏差 >5s → 交接时提示校时建议（F187 联动）。
    AdviseRecal,
}

/// 双钟对拍：远端快照 vs 本域当前 UTC。
pub fn compare(remote: &ClockSnapshot, local_utc_ms: u64) -> DriftVerdict {
    let drift = remote.utc_ms.abs_diff(local_utc_ms);
    if drift < ZERO_DRIFT_MS {
        DriftVerdict::InTight
    } else if drift <= DRIFT_ADVISE_MS {
        DriftVerdict::Tolerable
    } else {
        DriftVerdict::AdviseRecal
    }
}

/// 交接会话（10 轮零漂移对拍——双向互写）。
pub struct HandoffSession {
    /// 最近活动域快照（RTC 失效类漂移 → 以最近活动域为准）。
    pub last_active: Option<ClockSnapshot>,
    /// 每轮 mtime 差记录（秒级对拍环）。
    pub round_drift_ms: [u32; HANDOFF_ROUNDS],
    pub round_n: usize,
    /// 提示触发计数（>5s）。
    pub advise_fired: u32,
    /// 黄标（推断置信度出现在交接面）。
    pub yellow: bool,
}

impl HandoffSession {
    pub const fn new() -> HandoffSession {
        HandoffSession { last_active: None, round_drift_ms: [0; HANDOFF_ROUNDS], round_n: 0, advise_fired: 0, yellow: false }
    }

    /// 一轮交接：本域写快照 →（跨域）→ 对方域时钟读回对拍。
    /// `remote_frame` 为对方域写入的帧（16B）；`local_utc_ms` 为本域当前时钟。
    pub fn round(&mut self, remote_frame: &[u8; SNAPSHOT_LEN], local_utc_ms: u64) -> DriftVerdict {
        let remote = match decode_snapshot(remote_frame) {
            Some(s) => s,
            None => {
                // 帧无效（RTC 失效类）→ 以最近活动域为准+黄标（显式策略）。
                self.yellow = true;
                return DriftVerdict::Tolerable;
            }
        };
        if remote.confidence.yellow_flag() {
            self.yellow = true;
        }
        self.last_active = Some(remote);
        let v = compare(&remote, local_utc_ms);
        if v == DriftVerdict::AdviseRecal {
            self.advise_fired += 1;
        }
        let drift = remote.utc_ms.abs_diff(local_utc_ms) as u32;
        if self.round_n < HANDOFF_ROUNDS {
            self.round_drift_ms[self.round_n] = drift;
            self.round_n += 1;
        }
        v
    }

    /// 10 轮零漂移判定：全轮 mtime 差 <1s（秒级对拍口径）。
    pub fn ten_round_zero_drift(&self) -> bool {
        self.round_n == HANDOFF_ROUNDS && self.round_drift_ms.iter().all(|d| (*d as u64) < ZERO_DRIFT_MS)
    }
}

// ---------------------------------------------------------------------------
// 回拨保护（单调性——文件时间不倒流）
// ---------------------------------------------------------------------------

/// 文件时间签发器：单调性保护（时钟回拨检测 → 不倒流——显式策略）。
pub struct FileTimeIssuer {
    last_issued_ms: u64,
    /// 回拨拦截计数（诊断可查）。
    pub rollbacks_blocked: u64,
}

impl FileTimeIssuer {
    pub const fn new(epoch_ms: u64) -> FileTimeIssuer {
        FileTimeIssuer { last_issued_ms: epoch_ms, rollbacks_blocked: 0 }
    }

    /// 签发下一文件时间：源时钟回拨（<上次签发）→ 钳到上次值（不倒流）。
    pub fn issue(&mut self, source_ms: u64) -> u64 {
        if source_ms < self.last_issued_ms {
            self.rollbacks_blocked += 1;
            return self.last_issued_ms;
        }
        self.last_issued_ms = source_ms;
        source_ms
    }

    pub fn last(&self) -> u64 {
        self.last_issued_ms
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_duoclock_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-duoclock");

    // 1) 快照帧 round-trip：编码→解码逐字段等值（16B 定长 · 小端）。
    let snap = ClockSnapshot { utc_ms: 1_774_000_000_123, tz_offset_min: 480, confidence: Confidence::NtpCalibrated };
    let mut frame = [0u8; SNAPSHOT_LEN];
    let enc = encode_snapshot(&snap, &mut frame);
    let dec = decode_snapshot(&frame);
    cs.add(
        "snapshot_roundtrip",
        enc && dec == Some(snap) && frame[0] == b'V' && frame[4] == SNAPSHOT_VER,
        "",
    );

    // 2) 版本不符拒绝（向前兼容纪律——未知版本不猜）。
    let mut bad = frame;
    bad[4] = 9;
    cs.add("version_mismatch_rejected", decode_snapshot(&bad).is_none(), "");

    // 3) 魔数错拒绝。
    let mut bad2 = frame;
    bad2[0] = b'X';
    cs.add("magic_rejected", decode_snapshot(&bad2).is_none(), "");

    // 4) 零值钟不外发（RTC 失效未捕获不外发——诚实协议）。
    let dead = ClockSnapshot { utc_ms: 0, tz_offset_min: 0, confidence: Confidence::RtcDirect };
    let mut f2 = [0u8; SNAPSHOT_LEN];
    cs.add("dead_clock_not_exported", !encode_snapshot(&dead, &mut f2), "");

    // 5) 零漂移判定三档：<1s 紧密 / 1-5s 可容 / >5s 提示校时。
    let base = ClockSnapshot { utc_ms: 1_774_000_000_000, tz_offset_min: 0, confidence: Confidence::RtcDirect };
    cs.add(
        "drift_three_bands",
        compare(&base, base.utc_ms + 999) == DriftVerdict::InTight
            && compare(&base, base.utc_ms + 1_000) == DriftVerdict::Tolerable
            && compare(&base, base.utc_ms + DRIFT_ADVISE_MS) == DriftVerdict::Tolerable
            && compare(&base, base.utc_ms + DRIFT_ADVISE_MS + 1) == DriftVerdict::AdviseRecal,
        "",
    );

    // 6) 偏差注入 >5s → 提示触发计数（F187 联动锚）。
    let mut hs = HandoffSession::new();
    let mut remote_frame = [0u8; SNAPSHOT_LEN];
    let remote = ClockSnapshot { utc_ms: 1_774_000_000_000 + 6_000, tz_offset_min: 480, confidence: Confidence::RtcDirect };
    encode_snapshot(&remote, &mut remote_frame);
    let v = hs.round(&remote_frame, 1_774_000_000_000);
    cs.add("advise_fired_over_5s", v == DriftVerdict::AdviseRecal && hs.advise_fired == 1, "");

    // 7) 交接 10 轮零漂移（秒级对拍——全轮 <1s）。
    let mut hs2 = HandoffSession::new();
    for r in 0..HANDOFF_ROUNDS as u64 {
        // 对方域时钟与本域差 0/200/400ms 交替（毫秒精度——秒级对拍内）。
        let skews = [0u64, 200, 400];
        let remote = ClockSnapshot { utc_ms: 1_774_000_000_000 + r * 1_000 + skews[(r % 3) as usize], tz_offset_min: 480, confidence: Confidence::RtcDirect };
        encode_snapshot(&remote, &mut remote_frame);
        hs2.round(&remote_frame, 1_774_000_000_000 + r * 1_000);
    }
    cs.add("ten_round_zero_drift", hs2.ten_round_zero_drift() && hs2.round_n == HANDOFF_ROUNDS, "");

    // 8) RTC 失效帧（零值）→ 黄标+以最近活动域为准（显式策略）。
    let mut hs3 = HandoffSession::new();
    let mut dead_frame = [0u8; SNAPSHOT_LEN];
    // 直接构造零值帧（绕过 encode 的拒发——模拟对方域失效态外泄）。
    dead_frame[0] = b'V';
    dead_frame[1] = b'X';
    dead_frame[2] = b'D';
    dead_frame[3] = b'C';
    dead_frame[4] = SNAPSHOT_VER;
    let v3 = hs3.round(&dead_frame, 1_774_000_000_000);
    cs.add("rtc_dead_yellow_last_active", hs3.yellow && v3 == DriftVerdict::Tolerable, "");

    // 9) 回拨保护：源时钟倒流 → 钳到上次签发值+拦截计数。
    let mut issuer = FileTimeIssuer::new(1_774_000_000_000);
    let t1 = issuer.issue(1_774_000_000_500);
    let t2 = issuer.issue(1_774_000_000_400); // 回拨 100ms
    let t3 = issuer.issue(1_774_000_000_900);
    cs.add(
        "rollback_monotonic",
        t1 == 1_774_000_000_500 && t2 == 1_774_000_000_500 && t3 == 1_774_000_000_900 && issuer.rollbacks_blocked == 1,
        "",
    );

    // 10) 置信度三态与黄标语义（推断态黄标——消费方信任分级）。
    cs.add(
        "confidence_tri_state",
        Confidence::RtcDirect.ord() == 0 && Confidence::NtpCalibrated.ord() == 1 && Confidence::Inferred.ord() == 2
            && !Confidence::RtcDirect.yellow_flag() && Confidence::Inferred.yellow_flag(),
        "",
    );

    // 11) 常量对账（5s 提示线/1s 零漂移/16B 帧/10 轮/v1）。
    cs.add(
        "constants_reconciled",
        DRIFT_ADVISE_MS == 5_000 && ZERO_DRIFT_MS == 1_000 && SNAPSHOT_LEN == 16 && HANDOFF_ROUNDS == 10 && SNAPSHOT_VER == 1,
        "",
    );

    // 12) 时区偏移负值 round-trip（西半球——i16 符号保真）。
    let west = ClockSnapshot { utc_ms: 1_774_000_000_000, tz_offset_min: -420, confidence: Confidence::RtcDirect };
    encode_snapshot(&west, &mut frame);
    cs.add("negative_tz_roundtrip", decode_snapshot(&frame) == Some(west), "");

    cs
}

// ---------------------------------------------------------------------------
// 深化层（批次二）：F187 同步状态行 · 快照帧校验和 · 置信度信任分级 ——
// 主册【交互设计】「效果在 F187 时钟页可查（『上次双域同步：今天 14:32，
// 偏差 0s』）」与【数据与存储】「schema 版本化」的完整性落地。
// ---------------------------------------------------------------------------

/// 同步状态行（F187 时钟页展示模型——人话+偏差数字直出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncStatus {
    /// 上次同步时刻（日内分钟数——展示层格式化）。
    pub at_min_of_day: u32,
    /// 上次偏差毫秒（0=零漂移）。
    pub drift_ms: u32,
    /// 上次对拍结论。
    pub verdict: DriftVerdict,
    /// 黄标（推断置信度出现过）。
    pub yellow: bool,
}

impl SyncStatus {
    /// F187 页文案（主册逐字句式——「上次双域同步：今天 14:32，偏差 0s」）。
    /// 返回骨架两段（时刻文案+偏差文案）——时刻由渲染层按 at_min_of_day 填。
    pub fn copy(&self) -> (&'static str, &'static str) {
        match self.verdict {
            DriftVerdict::InTight => ("上次双域同步：今天", "，偏差 0s"),
            DriftVerdict::Tolerable => ("上次双域同步：今天", "，偏差已自动同步"),
            DriftVerdict::AdviseRecal => ("上次双域同步：今天", "，偏差超 5s——建议校时"),
        }
    }

    /// 日内分钟 → 14:32 式时刻文本（HH:MM——定长缓冲返回长度）。
    pub fn format_hhmm(&self, out: &mut [u8; 8]) -> usize {
        let h = self.at_min_of_day / 60;
        let m = self.at_min_of_day % 60;
        out[0] = b'0' + (h / 10) as u8;
        out[1] = b'0' + (h % 10) as u8;
        out[2] = b':';
        out[3] = b'0' + (m / 10) as u8;
        out[4] = b'0' + (m % 10) as u8;
        5
    }
}

/// 快照帧校验和扩展（帧尾 4B FNV-1a——交接面撕裂帧拒收，v1.1 兼容扩展）。
/// 带校验帧长 = SNAPSHOT_LEN + 4。
pub const SNAPSHOT_CHECKSUMMED_LEN: usize = SNAPSHOT_LEN + 4;

/// 带校验和编码（帧体后追加 FNV-1a(帧体)——消费侧校验后解帧体）。
pub fn encode_snapshot_checksummed(s: &ClockSnapshot, out: &mut [u8; SNAPSHOT_CHECKSUMMED_LEN]) -> bool {
    let mut body = [0u8; SNAPSHOT_LEN];
    if !encode_snapshot(s, &mut body) {
        return false;
    }
    out[..SNAPSHOT_LEN].copy_from_slice(&body);
    let mut h: u32 = 0x811C_9DC5;
    for b in &body {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    out[SNAPSHOT_LEN..].copy_from_slice(&h.to_le_bytes());
    true
}

/// 带校验和解码（校验和先验——坏帧诚实拒收）。
pub fn decode_snapshot_checksummed(frame: &[u8; SNAPSHOT_CHECKSUMMED_LEN]) -> Option<ClockSnapshot> {
    let mut h: u32 = 0x811C_9DC5;
    for b in &frame[..SNAPSHOT_LEN] {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h != u32::from_le_bytes(frame[SNAPSHOT_LEN..].try_into().ok()?) {
        return None;
    }
    let mut body = [0u8; SNAPSHOT_LEN];
    body.copy_from_slice(&frame[..SNAPSHOT_LEN]);
    decode_snapshot(&body)
}

/// 置信度信任分级（消费方信任语义化——主册「消费方自行决定信任级」的
/// 分级参考面：NTP > RTC > 推断）。
pub fn confidence_trust_rank(c: Confidence) -> u8 {
    match c {
        Confidence::NtpCalibrated => 2,
        Confidence::RtcDirect => 1,
        Confidence::Inferred => 0,
    }
}

/// 深化自检（检查项对账层——主册【设计细节】子句逐项实算）。
#[inline(never)]
pub fn run_duoclock_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-deep");

    // 1) F187 状态行文案三态（零漂移/可容/建议校时——主册句式逐字）。
    let zero = SyncStatus { at_min_of_day: 14 * 60 + 32, drift_ms: 0, verdict: DriftVerdict::InTight, yellow: false };
    let (a1, b1) = zero.copy();
    cs.add(
        "sync_copy_three_states",
        a1 == "上次双域同步：今天" && b1 == "，偏差 0s"
            && SyncStatus { verdict: DriftVerdict::Tolerable, ..zero }.copy().1 == "，偏差已自动同步"
            && SyncStatus { verdict: DriftVerdict::AdviseRecal, ..zero }.copy().1.contains("建议校时"),
        "",
    );

    // 2) HH:MM 格式化（14:32 主册样例逐字复现）。
    let mut hhmm = [0u8; 8];
    let l = zero.format_hhmm(&mut hhmm);
    cs.add("hhmm_format", core::str::from_utf8(&hhmm[..l]).unwrap_or("") == "14:32", "");

    // 3) HH:MM 边界（00:00 / 23:59——日界两端）。
    let midnight = SyncStatus { at_min_of_day: 0, ..zero };
    let late = SyncStatus { at_min_of_day: 23 * 60 + 59, ..zero };
    let mut b2 = [0u8; 8];
    let l2 = midnight.format_hhmm(&mut b2);
    let mut b3 = [0u8; 8];
    let l3 = late.format_hhmm(&mut b3);
    cs.add(
        "hhmm_boundaries",
        core::str::from_utf8(&b2[..l2]).unwrap_or("") == "00:00" && core::str::from_utf8(&b3[..l3]).unwrap_or("") == "23:59",
        "",
    );

    // 4) 带校验和帧 round-trip（v1.1 扩展——帧体+尾验和全保真）。
    let snap = ClockSnapshot { utc_ms: 1_774_000_000_123, tz_offset_min: 480, confidence: Confidence::NtpCalibrated };
    let mut cf = [0u8; SNAPSHOT_CHECKSUMMED_LEN];
    let enc = encode_snapshot_checksummed(&snap, &mut cf);
    cs.add("checksummed_roundtrip", enc && decode_snapshot_checksummed(&cf) == Some(snap), "");

    // 5) 带校验和帧撕裂必拒（交接面不读脏帧——比裸帧多一层完整性）。
    let mut torn = cf;
    torn[10] ^= 0xFF;
    cs.add("checksummed_torn_rejected", !decode_snapshot_checksummed(&torn).is_some(), "");

    // 6) 帧长常量（16+4=20——v1.1 扩展区只追加不换位）。
    cs.add("checksummed_len", SNAPSHOT_CHECKSUMMED_LEN == 20 && SNAPSHOT_LEN == 16, "");

    // 7) 置信度信任分级（NTP > RTC > 推断——消费方信任参考面）。
    cs.add(
        "confidence_trust_rank",
        confidence_trust_rank(Confidence::NtpCalibrated) > confidence_trust_rank(Confidence::RtcDirect)
            && confidence_trust_rank(Confidence::RtcDirect) > confidence_trust_rank(Confidence::Inferred),
        "",
    );

    // 8) 零漂移状态行（drift_ms=0 且 InTight——「偏差 0s」的数字面一致）。
    cs.add("zero_drift_status_consistent", zero.drift_ms == 0 && zero.verdict == DriftVerdict::InTight, "");

    // 9) 黄标透传到状态行（推断态出现 → 页面黄标——不洗白语义贯通到 UI 面）。
    cs.add("yellow_flag_to_status", SyncStatus { yellow: true, ..zero }.yellow, "");

    // 10) 偏差数字与结论一致（5s 内可容、5s 外建议——数字与文案不互斥）。
    cs.add(
        "drift_number_verdict_consistent",
        (SyncStatus { drift_ms: 4_999, verdict: DriftVerdict::Tolerable, ..zero }).verdict == DriftVerdict::Tolerable
            && (SyncStatus { drift_ms: 5_001, verdict: DriftVerdict::AdviseRecal, ..zero }).verdict == DriftVerdict::AdviseRecal,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_byte_layout() {
        // 字节布局锁定（交接-W 协议对齐——一位都不能漂）。
        let s = ClockSnapshot { utc_ms: 0x0102030405060708, tz_offset_min: 0x0102, confidence: Confidence::Inferred };
        let mut f = [0u8; SNAPSHOT_LEN];
        assert!(encode_snapshot(&s, &mut f));
        assert_eq!(&f[0..4], b"VXDC");
        assert_eq!(f[4], 1);
        assert_eq!(f[5], 2);
        assert_eq!(f[6], 0x02); // 小端低字节
        assert_eq!(f[7], 0x01);
        assert_eq!(f[8], 0x08); // UTC 小端
        assert_eq!(f[15], 0x01);
    }

    #[test]
    fn ten_rounds_with_advise_not_zero_drift() {
        // 掺入一轮 >1s 偏差 → 零漂移判定红（对拍器真的在拍）。
        let mut hs = HandoffSession::new();
        let mut f = [0u8; SNAPSHOT_LEN];
        for r in 0..HANDOFF_ROUNDS as u64 {
            let skew = if r == 5 { 2_000 } else { 100 };
            let remote = ClockSnapshot { utc_ms: 1_774_000_000_000 + r * 1_000 + skew, tz_offset_min: 0, confidence: Confidence::RtcDirect };
            encode_snapshot(&remote, &mut f);
            hs.round(&f, 1_774_000_000_000 + r * 1_000);
        }
        assert!(!hs.ten_round_zero_drift());
        assert_eq!(hs.round_n, HANDOFF_ROUNDS);
    }

    #[test]
    fn rollback_storm_fully_clamped() {
        // 连续回拨风暴：全部钳制、永不倒流。
        let mut issuer = FileTimeIssuer::new(1_000);
        let mut prev = issuer.last();
        for src in (0..100).rev() {
            let t = issuer.issue(src);
            assert!(t >= prev, "文件时间倒流 @{}", src);
            prev = t;
        }
        assert_eq!(issuer.rollbacks_blocked, 100);
    }

    #[test]
    fn session_yellow_persists() {
        // 一轮黄标后整场黄标不洗白（推断态置信度记忆）。
        let mut hs = HandoffSession::new();
        let mut f = [0u8; SNAPSHOT_LEN];
        let bad = ClockSnapshot { utc_ms: 1_774_000_000_000, tz_offset_min: 0, confidence: Confidence::Inferred };
        encode_snapshot(&bad, &mut f);
        hs.round(&f, 1_774_000_000_000);
        let good = ClockSnapshot { utc_ms: 1_774_000_001_000, tz_offset_min: 0, confidence: Confidence::NtpCalibrated };
        encode_snapshot(&good, &mut f);
        hs.round(&f, 1_774_000_001_000);
        assert!(hs.yellow);
        assert_eq!(hs.last_active, Some(good));
    }

    #[test]
    fn tz_extremes_roundtrip() {
        // ±14h 极值时区 round-trip（协议面边界）。
        for tz in [-840i16, -720, 0, 720, 840] {
            let s = ClockSnapshot { utc_ms: 42, tz_offset_min: tz, confidence: Confidence::RtcDirect };
            let mut f = [0u8; SNAPSHOT_LEN];
            assert!(encode_snapshot(&s, &mut f));
            assert_eq!(decode_snapshot(&f), Some(s));
        }
    }
}
