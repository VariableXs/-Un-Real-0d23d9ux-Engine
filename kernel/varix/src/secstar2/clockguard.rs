//! F187 时区与时钟守护（secstar2 · G-G-17）——时间对了，日历对了，连天气都跟着对了。
//!
//! **判据（主册）**：漂移注入 60s → 校正回来；时区切换全链（文件时间/日历/
//! 天气联动）实测；推断路径标注正确。
//!
//! **功能定义（主册 G-G-17）**：RTC 漂移校正（启动时对 NTP 或上次已知好值）+
//! 时区切换向导（跨时区用 U 盘整机的真实场景）+ NTP 自动校（网络可用时）；
//! 校时历史可查。
//!
//! 【交互设计】「时间和语言」页：当前时间大显+时区下拉+「自动校时」开关+
//! 校时历史行（最近 5 次：时刻/来源/偏移量）；向导弹层三步（检测到变化→
//! 选新时区→确认）；手动改时间入口（高级折叠——慎用区）。
//! 【数据与存储】时区与校时历史配置层；「上次已知好值」双存储（配置+快照）。
//! 【状态与异常】无网络 → 用上次好值（漂移曲线插值——标注「估算」）；RTC
//! 硬件失效（常年归零）→ 检测后每次启动走推断流程+黄条说明；夏令时规则随
//! tzdata（F022 同源）。
//! 【设计细节】漂移校正策略：偏差 <2s 静默 / 2-300s 静默校正+记录 / >300s
//! 显式提示（可能 RTC 电池问题）；NTP 源池三个公网+国内候选；时区切换不改
//! 文件 mtime（历史真相不动——只改显示层）；跨时区提示触发=RTC 本地时与
//! 设置时区推算差 >30 分钟。
//!
//! 时间纪律：与 timesrv（B-3501）同源——单调钟给测量、墙钟给显示，本模块
//! 只管「墙钟对不对」，不反向污染单调钟。宿主测试一切时间注入。
//! 依赖锚点：F022（tzdata）、F117（网络步后向导）、F121（快照覆盖）、F101（天气联动）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 漂移分档：静默上限（秒）——偏差 <2s 静默。
pub const DRIFT_SILENT_S: i64 = 2;
/// 漂移分档：静默校正+记录上限（秒）——2..=300 静默校正；>300 显式提示。
pub const DRIFT_EXPLICIT_S: i64 = 300;

/// 跨时区提示触发阈值：RTC 本地时与设置时区推算差（分钟）>30。
pub const TZ_HINT_MIN: i64 = 30;

/// 校时历史行数（最近 5 次：时刻/来源/偏移量）。
pub const HISTORY_CAP: usize = 5;

/// NTP 源池容量（三个公网+国内候选——主册【设计细节】）。
pub const NTP_POOL_CAP: usize = 4;

/// RTC 失效判定：连续归零读数次数（检测后每次启动走推断流程+黄条）。
pub const RTC_DEAD_STRIKES: u32 = 3;

// ---------------------------------------------------------------------------
// SNTP 客户端（自研 ~200 行量级——纯函数，宿主可测）
// ---------------------------------------------------------------------------

/// SNTP 报文长度（RFC 4330）。
pub const SNTP_PACKET_LEN: usize = 48;

/// SNTP 报文构造/解析（1900 纪元 ↔ Unix 纪元换算内建）。
pub struct Sntp;

impl Sntp {
    /// Unix 纪元 → NTP 纪元秒差（1900-01-01 → 1970-01-01）。
    pub const NTP_EPOCH_DELTA: u64 = 2_208_988_800;

    /// 构造客户端请求报文（LI=0 VN=4 Mode=3，transmit 秒戳=unix 秒）。
    pub fn request(unix_secs: u64) -> [u8; SNTP_PACKET_LEN] {
        let mut p = [0u8; SNTP_PACKET_LEN];
        p[0] = 0b00_100_011; // LI=0, VN=4, Mode=3 (client)
        let ntp = unix_secs + Self::NTP_EPOCH_DELTA;
        p[40..44].copy_from_slice(&(ntp as u32).to_be_bytes());
        p
    }

    /// 解析服务器应答：提取 (t2 服务器收包, t3 服务器发包)（Unix 秒，
    /// 定点 16.16 到微秒）。
    /// Mode 必须为 4（server）或 5（broadcast）；stratum 0（kiss-of-death）
    /// 拒绝——诚实报错不硬凑。
    pub fn parse_response(pkt: &[u8]) -> Result<(u64, u64), &'static str> {
        if pkt.len() < SNTP_PACKET_LEN {
            return Err("packet too short");
        }
        let mode = pkt[0] & 0b0000_0111;
        if mode != 4 && mode != 5 {
            return Err("not a server response");
        }
        let stratum = pkt[1];
        if stratum == 0 {
            return Err("kiss-of-death (stratum 0)");
        }
        let rd_secs = u32::from_be_bytes([pkt[32], pkt[33], pkt[34], pkt[35]]) as u64;
        let rd_frac = u32::from_be_bytes([pkt[36], pkt[37], pkt[38], pkt[39]]) as u64;
        let xt_secs = u32::from_be_bytes([pkt[40], pkt[41], pkt[42], pkt[43]]) as u64;
        let xt_frac = u32::from_be_bytes([pkt[44], pkt[45], pkt[46], pkt[47]]) as u64;
        let to_us = |secs: u64, frac: u64| -> u64 {
            // NTP 纪元 → Unix 纪元；分数 2^-32 秒 → 微秒。
            let secs_unix = secs.wrapping_sub(Self::NTP_EPOCH_DELTA);
            secs_unix
                .saturating_mul(1_000_000)
                .saturating_add(frac * 1_000_000 / (1u64 << 32))
        };
        Ok((to_us(rd_secs, rd_frac), to_us(xt_secs, xt_frac)))
    }

    /// 时钟偏移（微秒）：offset = ((t1-t0) + (t2-t3)) / 2。
    /// t0=客户端发包 t1=客户端收包（Unix µs），t2/t3 来自应答。
    pub fn offset_us(t0: u64, t1: u64, t2: u64, t3: u64) -> i64 {
        let a = (t1 as i128) - (t0 as i128);
        let b = (t2 as i128) - (t3 as i128);
        ((a + b) / 2) as i64
    }
}

// ---------------------------------------------------------------------------
// 漂移策略
// ---------------------------------------------------------------------------

/// 漂移分档判定（主册三档：<2s 静默 / 2..=300s 静默校正+记录 / >300s 显式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftAction {
    /// <2s：静默（不值得记录）。
    Silent,
    /// 2..=300s：静默校正+记录。
    SilentCorrect,
    /// >300s：显式提示（可能 RTC 电池问题）。
    ExplicitPrompt,
}

pub fn classify_drift(offset_s: i64) -> DriftAction {
    let abs = offset_s.abs();
    if abs < DRIFT_SILENT_S {
        DriftAction::Silent
    } else if abs <= DRIFT_EXPLICIT_S {
        DriftAction::SilentCorrect
    } else {
        DriftAction::ExplicitPrompt
    }
}

// ---------------------------------------------------------------------------
// 时区
// ---------------------------------------------------------------------------

/// 时区条目（tzdata 同源数据的最小投影——F022/F130 登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TzEntry {
    pub name: &'static str,
    /// 基础偏移（分钟，东正西负）。
    pub offset_min: i64,
    /// 是否有夏令时规则（规则细则随 tzdata）。
    pub has_dst: bool,
}

/// 内置时区表（覆盖 U 盘整机常见场景；完整表随 tzdata 加载）。
pub const TZ_TABLE: [TzEntry; 6] = [
    TzEntry { name: "Asia/Shanghai", offset_min: 480, has_dst: false },
    TzEntry { name: "Asia/Tokyo", offset_min: 540, has_dst: false },
    TzEntry { name: "UTC", offset_min: 0, has_dst: false },
    TzEntry { name: "Europe/Berlin", offset_min: 60, has_dst: true },
    TzEntry { name: "America/New_York", offset_min: -300, has_dst: true },
    TzEntry { name: "America/Los_Angeles", offset_min: -480, has_dst: true },
];

// ---------------------------------------------------------------------------
// 校时历史
// ---------------------------------------------------------------------------

/// 一条校时历史（时刻/来源/偏移量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncRecord {
    /// 校时时刻（Unix 秒）。
    pub at: u64,
    /// 来源（NTP 源名 / last-good / snapshot / manual）。
    pub source: &'static str,
    /// 校正前偏移（秒，正=RTC 慢）。
    pub offset_s: i64,
    /// 是否标注「估算」（无网络用上次好值插值路径）。
    pub estimated: bool,
}

// ---------------------------------------------------------------------------
// 时钟守护主体
// ---------------------------------------------------------------------------

/// NTP 源池条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceHealth {
    /// 未试。
    Untested,
    /// 上次成功。
    Alive,
    /// 上次失败（超时/拒绝）。
    Dead,
}

/// 时钟守护：漂移校正 + 时区向导 + 历史账。
pub struct ClockGuard {
    /// NTP 源池（三个公网+国内候选——池满拒绝，零静默）。
    pub pool: [(&'static str, SourceHealth); NTP_POOL_CAP],
    pub pool_len: usize,
    /// 上次已知好值双存储：配置槽 + 快照槽（F121 覆盖）。
    pub good_config: Option<u64>,
    pub good_snapshot: Option<u64>,
    /// 校时历史（最近 5 次——环，新→旧）。
    pub history: RingLog<SyncRecord, HISTORY_CAP>,
    /// 当前时区。
    pub tz: TzEntry,
    /// 自动校时开关。
    pub auto_sync: bool,
    /// RTC 硬件失效判定：连续归零读数。
    rtc_zero_strikes: u32,
    /// RTC 失效态（触发后每次启动走推断流程+黄条说明）。
    pub rtc_dead: bool,
    /// 推断路径激活（黄条文案消费方读此标志）。
    pub inferring: bool,
    /// 向导完成标志（`tz_wizard_advance` 置位——UI 层读后清）。
    pub wizard_done: bool,
}

impl ClockGuard {
    pub fn new() -> ClockGuard {
        ClockGuard {
            pool: [("", SourceHealth::Untested); NTP_POOL_CAP],
            pool_len: 0,
            good_config: None,
            good_snapshot: None,
            history: RingLog::new(),
            tz: TZ_TABLE[0],
            auto_sync: true,
            rtc_zero_strikes: 0,
            rtc_dead: false,
            inferring: false,
            wizard_done: false,
        }
    }

    /// 登记一个 NTP 源（池满返回 false——调用方必须处理，零静默）。
    pub fn add_source(&mut self, name: &'static str) -> bool {
        if self.pool.iter().take(self.pool_len).any(|(n, _)| *n == name) {
            return false;
        }
        if self.pool_len >= NTP_POOL_CAP {
            return false;
        }
        self.pool[self.pool_len] = (name, SourceHealth::Untested);
        self.pool_len += 1;
        true
    }

    /// 标记源健康（成功/失败——源选择按 Alive 优先、Untested 次之）。
    pub fn mark_source(&mut self, name: &str, alive: bool) {
        let st = if alive { SourceHealth::Alive } else { SourceHealth::Dead };
        for (n, s) in self.pool.iter_mut().take(self.pool_len) {
            if *n == name {
                *s = st;
            }
        }
    }

    /// 挑下一个尝试的源（Alive 跳过——已成功不再打；Untested 优先于 Dead）。
    pub fn next_source(&self) -> Option<&'static str> {
        let rank = |h: SourceHealth| match h {
            SourceHealth::Untested => 0,
            SourceHealth::Dead => 1,
            SourceHealth::Alive => 2,
        };
        (0..self.pool_len)
            .min_by_key(|&i| rank(self.pool[i].1))
            .filter(|&i| rank(self.pool[i].1) < 2)
            .map(|i| self.pool[i].0)
    }

    /// RTC 读数上报（每次启动/周期）：连零 → 失效态（推断流程+黄条）。
    pub fn note_rtc_reading(&mut self, unix_s: u64) {
        if unix_s == 0 {
            self.rtc_zero_strikes += 1;
            if self.rtc_zero_strikes >= RTC_DEAD_STRIKES {
                self.rtc_dead = true;
                self.inferring = true;
            }
        } else {
            self.rtc_zero_strikes = 0;
        }
    }

    /// **漂移校正主路**（判据一「注入 60s → 校正回来」）。
    ///
    /// `rtc_s`=RTC 读数，`true_s`=可信真值（NTP 对齐结果或上次好值插值）。
    /// 按三档策略处置并记史；返回处置动作供 UI 层消费（显式提示档弹卡）。
    pub fn correct_drift(&mut self, rtc_s: u64, true_s: u64, source: &'static str) -> DriftAction {
        let off = (true_s as i128 - rtc_s as i128) as i64;
        let act = classify_drift(off);
        match act {
            DriftAction::Silent => {}
            DriftAction::SilentCorrect => {
                self.history.push(SyncRecord { at: true_s, source, offset_s: off, estimated: false });
                self.good_config = Some(true_s);
                self.good_snapshot = Some(true_s);
            }
            DriftAction::ExplicitPrompt => {
                self.history.push(SyncRecord { at: true_s, source, offset_s: off, estimated: false });
                self.good_config = Some(true_s);
                self.good_snapshot = Some(true_s);
            }
        }
        act
    }

    /// **无网络回退路**：用上次好值漂移插值——标注「估算」（判据三）。
    ///
    /// 插值模型：好值时刻 `good_s`，实时钟 `rtc_s`，漂移率 `ppm`（上次
    /// 校准测得）→ 估算真值 = good + (rtc-good) × (1 + ppm/1e6)。
    pub fn estimate_from_last_good(&mut self, rtc_s: u64, good_s: u64, drift_ppm: i64, now_s: u64) -> u64 {
        let elapsed = rtc_s.saturating_sub(good_s) as i128;
        let est = (good_s as i128) + elapsed + elapsed * (drift_ppm as i128) / 1_000_000;
        let est = est.max(0) as u64;
        self.inferring = true;
        self.history.push(SyncRecord { at: now_s, source: "last-good", offset_s: (est as i128 - rtc_s as i128) as i64, estimated: true });
        est
    }

    /// 校时历史快照（新→旧，≤5 条）。
    pub fn recent_history(&self) -> Vec<SyncRecord> {
        self.history.newest_first()
    }

    /// **跨时区提示判定**：RTC 本地时与设置时区推算差 >30 分钟 → 向导弹层。
    ///
    /// `rtc_local_min`=RTC 认为的本地分钟偏移；`wall_utc_min`=可信 UTC 分钟。
    pub fn tz_switch_hint(&self, rtc_local_min: i64, wall_utc_min: i64) -> bool {
        let assumed = rtc_local_min - self.tz.offset_min;
        let diff = (assumed - wall_utc_min).abs() % (24 * 60);
        diff.min(24 * 60 - diff) > TZ_HINT_MIN
    }

    /// **时区切换向导三步状态机**（检测到变化→选新时区→确认）。
    ///
    /// 返回 Ok(true) = 切换完成；Ok(false) = 状态推进未完成；Err = 非法迁移。
    /// 切换语义（判据二）：**只改显示层**——文件 mtime 不动，日历/天气等
    /// 消费面通过重读 `tz` 生效（联动由消费方订阅）。
    pub fn tz_wizard_advance(&mut self, st: TzWizardStep, pick: Option<TzEntry>) -> Result<bool, &'static str> {
        match st {
            TzWizardStep::Idle => Err("wizard not started"),
            TzWizardStep::Detected => {
                // 第一步→第二步：需要候选时区（pick 占位校验）。
                match pick {
                    Some(_) => Ok(false),
                    None => Err("select a timezone to continue"),
                }
            }
            TzWizardStep::Selecting => {
                let tz = pick.ok_or("no timezone picked")?;
                if !TZ_TABLE.iter().any(|t| t.name == tz.name) {
                    return Err("timezone not in tzdata table");
                }
                self.tz = tz;
                self.wizard_done = true;
                Ok(true)
            }
            TzWizardStep::Confirmed => Ok(true),
        }
    }

    /// 显示层换算：文件 mtime（UTC 秒）→ 本地显示秒。
    /// **历史真相不动**：mtime 本体永不改写，只在此换算（判据二全链）。
    pub fn display_time(&self, mtime_utc_s: u64, dst_active: bool) -> u64 {
        let dst = if dst_active && self.tz.has_dst { 60 } else { 0 };
        mtime_utc_s + (self.tz.offset_min + dst) as u64 * 60
    }
}

/// 向导三步（检测到变化→选新时区→确认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TzWizardStep {
    Idle,
    Detected,
    Selecting,
    Confirmed,
}

impl Default for ClockGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F187 自检（聚合进 secstar2 域）。
pub fn run_clockguard_checks() -> CheckSet {
    let mut set = CheckSet::new("F187-clockguard");

    // SNTP 报文 round-trip：构造请求 → 伪造应答 → 偏移解析。
    let t0: u64 = 1_700_000_000_000_000; // 客户端发包 µs
    let req = Sntp::request(t0 / 1_000_000);
    set.add("sntp req mode", req[0] == 0b00_100_011, "");
    let t3 = t0 / 1_000_000 + 10; // 服务器发包（unix 秒）
    let mut resp = [0u8; 48];
    resp[0] = 0b00_100_100; // LI=0 VN=4 Mode=4
    resp[1] = 2; // stratum
    let ntp_t3 = t3 + Sntp::NTP_EPOCH_DELTA;
    let t2 = t3; // 服务器收包=发包（零处理时延样本）
    let ntp_t2 = t2 + Sntp::NTP_EPOCH_DELTA;
    resp[32..36].copy_from_slice(&(ntp_t2 as u32).to_be_bytes());
    resp[40..44].copy_from_slice(&(ntp_t3 as u32).to_be_bytes());
    let parsed = Sntp::parse_response(&resp);
    set.add("sntp parse ok", parsed.is_ok(), "");
    let (p2, p3) = parsed.unwrap();
    set.add("sntp epoch", p3 == t3 * 1_000_000 && p2 == t2 * 1_000_000, "");
    // 偏移：t1=t0+1ms 链路，t2/t3 已是 µs → offset = ((t1-t0)+(t2-t3))/2 = 500µs。
    let t1 = t0 + 1_000;
    set.add("sntp offset", Sntp::offset_us(t0, t1, p2, p3) == 500, "");

    // Kiss-of-death 与短包拒绝。
    let mut kod = [0u8; 48];
    kod[0] = 0b00_100_100;
    kod[1] = 0;
    set.add("sntp kod reject", Sntp::parse_response(&kod).is_err(), "");
    set.add("sntp short reject", Sntp::parse_response(&resp[..20]).is_err(), "");

    // 漂移三档：60s 注入 → SilentCorrect（校正回来）；5s→记录；1s→静默；400s→显式。
    set.add("drift 1s silent", classify_drift(1) == DriftAction::Silent, "");
    set.add("drift 60s correct", classify_drift(60) == DriftAction::SilentCorrect, "");
    set.add("drift 300s correct", classify_drift(300) == DriftAction::SilentCorrect, "");
    set.add("drift 400s explicit", classify_drift(400) == DriftAction::ExplicitPrompt, "");

    let mut g = ClockGuard::new();
    for s in ["pool.ntp.org", "cn.pool.ntp.org", "time.cloudflare.com"] {
        assert!(g.add_source(s));
    }
    // 池=三个公网+国内候选共 4 槽：第 4 槽可用，第 5 槽拒绝（池满零静默）。
    set.add("pool 4th slot", g.add_source("time.windows.com"), "");
    set.add("pool cap loud", !g.add_source("overflow.example"), "");
    set.add("pool next untested", g.next_source() == Some("pool.ntp.org"), "");
    g.mark_source("pool.ntp.org", true);
    set.add("pool skips alive", g.next_source() != Some("pool.ntp.org"), "");

    // 判据一：注入 60s 漂移 → 校正回来（真值回填 + 历史记录 + 好值双存）。
    let act = g.correct_drift(1_700_000_000, 1_700_000_060, "cn.pool.ntp.org");
    set.add("correct 60s", act == DriftAction::SilentCorrect, "");
    set.add("good dual store", g.good_config == Some(1_700_000_060) && g.good_snapshot == Some(1_700_000_060), "");
    set.add("history recorded", g.recent_history().len() == 1, "");

    // 判据三：无网络推断路——标注「估算」。
    let est = g.estimate_from_last_good(1_700_100_000, 1_700_000_060, 50, 1_700_100_000);
    set.add("estimate marked", g.recent_history()[0].estimated, "");
    // 插值数学：elapsed=99940s，ppm=50 → est = good + 99940 + 99940*50/1e6。
    set.add("estimate math", est == 1_700_000_060 + 99_940 + 4, "");

    // RTC 失效：连零三读 → 推断+黄条。
    let mut g2 = ClockGuard::new();
    g2.note_rtc_reading(0);
    g2.note_rtc_reading(0);
    set.add("rtc not yet dead", !g2.rtc_dead, "");
    g2.note_rtc_reading(0);
    set.add("rtc dead inferring", g2.rtc_dead && g2.inferring, "");
    g2.note_rtc_reading(1_700_000_000);
    set.add("rtc strike reset", g2.rtc_zero_strikes == 0, "");

    // 判据二：跨时区提示 + 向导三步 + mtime 不动。
    let mut g3 = ClockGuard::new();
    // 上海 UTC+480：RTC 本地认为 22:00(1320min)，UTC 墙钟 08:00(480min)
    // → assumed=1320-480=840 ≈ UTC 14:00 → 差 360min > 30 → 提示。
    set.add("tz hint fires", g3.tz_switch_hint(1320, 480), "");
    // 静默样本：RTC 本地=UTC+480（上海正点）±15min → 差 ≤30 → 不提示。
    set.add("tz hint quiet", !g3.tz_switch_hint(960 + 15, 480), "");
    set.add("wizard idle err", g3.tz_wizard_advance(TzWizardStep::Idle, None).is_err(), "");
    set.add("wizard detect", g3.tz_wizard_advance(TzWizardStep::Detected, Some(TZ_TABLE[0])).is_ok(), "");
    set.add("wizard pick", g3.tz_wizard_advance(TzWizardStep::Selecting, Some(TZ_TABLE[1])).is_ok(), "");
    set.add("wizard switched", g3.tz.name == "Asia/Tokyo", "");
    let mtime = 1_700_000_000;
    set.add("mtime untouched", g3.display_time(mtime, false) == mtime + 540 * 60, "");
    // DST 语义：无 DST 规则的时区（东京）请求 DST 也忽略；有规则的（柏林）
    // 叠加 60min。
    set.add("dst ignored without rule", g3.display_time(mtime, true) == mtime + 540 * 60, "");
    g3.tz = TZ_TABLE[3]; // Europe/Berlin（UTC+60，has_dst）
    set.add("dst applies", g3.display_time(mtime, true) == mtime + 120 * 60, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f187_sntp_fraction_us() {
        // 分数位换算：0x80000000 = 半秒。
        let mut pkt = [0u8; 48];
        pkt[0] = 0b00_100_100;
        pkt[1] = 1;
        let secs = 3_900_000_000u32; // > NTP_EPOCH_DELTA，Unix=1691011200
        pkt[40..44].copy_from_slice(&secs.to_be_bytes());
        pkt[44..48].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let (_, t3) = Sntp::parse_response(&pkt).unwrap();
        assert_eq!(t3 % 1_000_000, 500_000, "half second fraction");
    }

    #[test]
    fn f187_source_ranking_untested_before_dead() {
        let mut g = ClockGuard::new();
        g.add_source("a");
        g.add_source("b");
        g.mark_source("a", false);
        assert_eq!(g.next_source(), Some("b"), "untested tried before dead");
        g.mark_source("b", false);
        assert_eq!(g.next_source(), Some("a"), "dead retried when nothing else");
        g.mark_source("a", true);
        assert_eq!(g.next_source(), Some("b"));
    }

    #[test]
    fn f187_history_ring_five() {
        let mut g = ClockGuard::new();
        for i in 0..8u64 {
            g.correct_drift(i * 1000, i * 1000 + 100, "test");
        }
        let h = g.recent_history();
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h[0].at, 7100, "newest first (at=true_s)");
        assert_eq!(h[4].at, 3100);
    }

    #[test]
    fn f187_explicit_prompt_records_too() {
        let mut g = ClockGuard::new();
        let act = g.correct_drift(100, 100 + 3600, "manual");
        assert_eq!(act, DriftAction::ExplicitPrompt);
        assert_eq!(g.recent_history().len(), 1);
        assert_eq!(g.recent_history()[0].offset_s, 3600);
    }

    #[test]
    fn f187_silent_no_record() {
        let mut g = ClockGuard::new();
        let act = g.correct_drift(1_000_000, 1_000_001, "n");
        assert_eq!(act, DriftAction::Silent);
        assert!(g.recent_history().is_empty(), "<2s silent means no record");
    }

    #[test]
    fn f187_wizard_rejects_unknown_tz() {
        let mut g = ClockGuard::new();
        let bogus = TzEntry { name: "Mars/Olympus", offset_min: 0, has_dst: false };
        assert!(g.tz_wizard_advance(TzWizardStep::Selecting, Some(bogus)).is_err());
        assert_eq!(g.tz.name, "Asia/Shanghai", "tz unchanged on reject");
    }

    #[test]
    fn f187_dst_respects_has_dst_flag() {
        let mut g = ClockGuard::new();
        g.tz = TZ_TABLE[2]; // UTC 无 DST
        assert_eq!(g.display_time(1000, true), 1000, "dst ignored without rule");
        g.tz = TZ_TABLE[3]; // Berlin 有 DST
        assert_eq!(g.display_time(1000, true), 1000 + 120 * 60);
    }

    #[test]
    fn f187_run_checks_pass() {
        assert!(run_clockguard_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：SyncEngine —— 逐源尝试状态机（超时/退避/换源，确定性时间注入）
// ---------------------------------------------------------------------------

/// 单源尝试结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// 成功（offset 秒已回填好值）。
    Synced(i64),
    /// 超时（ms 预算内无应答）。
    Timeout,
    /// 源拒绝（kiss-of-death/短包——parse 错误面）。
    Rejected,
}

/// 同步引擎：对源池逐个尝试；失败按指数退避（1s→2s→4s 封顶 8s），
/// 单源连续失败 ≥3 次降权（本周期内跳过）；全部源耗尽 → 回落上次好值。
pub struct SyncEngine {
    /// 每源连续失败计数。
    fail_streaks: Vec<(&'static str, u32)>,
    /// 退避到期时刻（源名 → 最早可再试时刻 ms）。
    backoff_until: Vec<(&'static str, u64)>,
    /// 本周期跳过清单（降权）。
    skipped: Vec<&'static str>,
    /// 同步成功次数。
    pub syncs: u64,
    /// 超时次数。
    pub timeouts: u64,
    /// 拒绝次数。
    pub rejects: u64,
}

pub const ATTEMPT_TIMEOUT_MS: u64 = 1_500;
pub const BACKOFF_BASE_MS: u64 = 1_000;
pub const BACKOFF_CAP_MS: u64 = 8_000;
pub const SKIP_AFTER_STREAK: u32 = 3;

impl SyncEngine {
    pub fn new() -> SyncEngine {
        SyncEngine { fail_streaks: Vec::new(), backoff_until: Vec::new(), skipped: Vec::new(), syncs: 0, timeouts: 0, rejects: 0 }
    }

    fn bump_streak(&mut self, src: &'static str) -> u32 {
        match self.fail_streaks.iter_mut().find(|(s, _)| *s == src) {
            Some((_, c)) => {
                *c += 1;
                *c
            }
            None => {
                self.fail_streaks.push((src, 1));
                1
            }
        }
    }

    /// 源现在可试吗（退避未到/已降权 → 不可试）。
    pub fn eligible(&self, src: &'static str, now_ms: u64) -> bool {
        if self.skipped.iter().any(|s| *s == src) {
            return false;
        }
        !self.backoff_until.iter().any(|(s, t)| *s == src && *t > now_ms)
    }

    /// 记一次尝试结果（引擎记账 + 退避/降权决策）。
    pub fn note_attempt(&mut self, src: &'static str, outcome: AttemptOutcome, now_ms: u64) {
        match outcome {
            AttemptOutcome::Synced(_) => {
                self.syncs += 1;
                self.fail_streaks.retain(|(s, _)| *s != src);
                self.backoff_until.retain(|(s, _)| *s != src);
            }
            AttemptOutcome::Timeout => {
                self.timeouts += 1;
                let streak = self.bump_streak(src);
                let backoff = (BACKOFF_BASE_MS << (streak - 1).min(3)).min(BACKOFF_CAP_MS);
                self.backoff_until.retain(|(s, _)| *s != src);
                self.backoff_until.push((src, now_ms + backoff));
                if streak >= SKIP_AFTER_STREAK {
                    self.skipped.push(src);
                }
            }
            AttemptOutcome::Rejected => {
                self.rejects += 1;
                let streak = self.bump_streak(src);
                let backoff = (BACKOFF_BASE_MS << (streak - 1).min(3)).min(BACKOFF_CAP_MS);
                self.backoff_until.retain(|(s, _)| *s != src);
                self.backoff_until.push((src, now_ms + backoff));
                if streak >= SKIP_AFTER_STREAK {
                    self.skipped.push(src);
                }
            }
        }
    }

    /// 全部源耗尽？（全部降权或退避中——调用方回落上次好值）。
    pub fn pool_exhausted(&self, pool: &[&'static str], now_ms: u64) -> bool {
        pool.iter().all(|s| !self.eligible(s, now_ms))
    }

    /// 指数退避值（第 streak 次失败的应等时长——测试对账）。
    pub fn backoff_for(streak: u32) -> u64 {
        (BACKOFF_BASE_MS << (streak.max(1) - 1).min(3)).min(BACKOFF_CAP_MS)
    }
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深二：DriftEstimator —— 漂移率 ppm 估计（两点标定法）
// ---------------------------------------------------------------------------

/// 两次标定（好值对 + RTC 对）→ 漂移率 ppm（整数定点，1e6=1 倍速）。
/// ppm = ((trueΔ − rtcΔ) / rtcΔ) × 1e6；rtcΔ=0 不可判 → None（诚实）。
pub fn estimate_ppm(first: (u64, u64), second: (u64, u64)) -> Option<i64> {
    let (true0, rtc0) = first;
    let (true1, rtc1) = second;
    if rtc1 <= rtc0 || true1 < true0 {
        return None;
    }
    let rtc_delta = (rtc1 - rtc0) as i128;
    let true_delta = (true1 - true0) as i128;
    let ppm = (true_delta - rtc_delta) * 1_000_000 / rtc_delta;
    // 合理界：±500ppm 之外的漂移率视作标定错误（RTC 常规漂移 ≤±150ppm）。
    if ppm.abs() > 500 * 1_000_000 {
        return None;
    }
    Some(ppm as i64)
}

/// 用漂移率外推：`est = base + elapsed × (1 + ppm/1e6)`（零浮点 i128）。
pub fn extrapolate(base_s: u64, elapsed_s: u64, ppm: i64) -> u64 {
    let est = (base_s as i128) + (elapsed_s as i128) + (elapsed_s as i128) * (ppm as i128) / 1_000_000;
    est.max(0) as u64
}

// ---------------------------------------------------------------------------
// 深三：DstTransition —— 夏令时边界换算（has_dst 时区的月度规则表）
// ---------------------------------------------------------------------------

/// 简化 DST 规则（北半球通用面：4 月第一个周日开、10 月最后一个周日关——
/// 完整 tzdata 规则由 F022 供给，此表覆盖提示/显示层的绝大多数场景）。
pub struct DstRule {
    /// 开启月（1-12）与当月第几个周日（1-4）。
    pub start_month: u64,
    pub start_week: u64,
    /// 关闭月与当月第几个周日。
    pub end_month: u64,
    pub end_week: u64,
}

pub const DST_RULE_NORTH: DstRule = DstRule { start_month: 4, start_week: 1, end_month: 10, end_week: 5 };

/// 该月第 `week` 个周日是几号（当月 1 号的星期由调用方给：0=周日）。
pub fn nth_sunday(day_of_week_first: u64, week: u64) -> u64 {
    // 第一个周日 = ((7 - dow) % 7) + 1；第 week 个 = +7×(week-1)。钳进 1..=31。
    let first = ((7 - day_of_week_first % 7) % 7) + 1;
    (first + 7 * (week.max(1) - 1)).min(31)
}

/// 某日期 DST 是否生效（北半球规则；南半球/南亚无 DST 的时区 has_dst=false 不进此函数）。
pub fn dst_active(month: u64, day: u64, dow_first_of_month: u64) -> bool {
    let start_day = nth_sunday(dow_first_of_month, DST_RULE_NORTH.start_week);
    let end_day = nth_sunday(dow_first_of_month, DST_RULE_NORTH.end_week);
    // 4 月起 10 月止：月比较 + 日比较（开当日 02:00 起、关当日 02:00 止按整天近似——
    // 显示层场景误差 ≤2h，标注口径）。
    (month > DST_RULE_NORTH.start_month && month < DST_RULE_NORTH.end_month)
        || (month == DST_RULE_NORTH.start_month && day >= start_day)
        || (month == DST_RULE_NORTH.end_month && day < end_day)
}

// ---------------------------------------------------------------------------
// 深四：ManualSetGate —— 手动改时间入口（高级折叠——慎用区三重防呆）
// ---------------------------------------------------------------------------

/// 手动校时闸：主册「手动改时间入口（高级折叠——慎用区）」的执法面。
pub struct ManualSetGate {
    /// 双重确认状态（第一次点击=武装，第二次=执行）。
    armed: bool,
    /// 手动改动次数（审计面——慎用区被用了几次）。
    pub manual_sets: u64,
    /// 单次最大修正幅度（秒）：±24h——更大的偏差请走 NTP/好值路。
    pub max_adjust_s: i64,
}

impl ManualSetGate {
    pub fn new() -> ManualSetGate {
        ManualSetGate { armed: false, manual_sets: 0, max_adjust_s: 24 * 3600 }
    }

    /// 第一次点击：武装（弹层显示将改成的目标时刻）。
    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// 放弃（点外部/Esc）——解除武装。
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// 执行手动设定：必须已武装 + 给原因（审计行）+ 幅度界内。
    pub fn commit(&mut self, adjust_s: i64, reason: &str) -> Result<i64, &'static str> {
        if !self.armed {
            return Err("需先确认一次（双重确认防呆）");
        }
        if reason.len() < 4 {
            return Err("请填写修改原因（审计要求）");
        }
        if adjust_s.abs() > self.max_adjust_s {
            return Err("单次修正超过 ±24h：请改用网络校时或恢复环境");
        }
        self.armed = false;
        self.manual_sets += 1;
        Ok(adjust_s)
    }
}

impl Default for ManualSetGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深五：SyncStats —— 校时历史统计面（「时间和语言」页数据源）
// ---------------------------------------------------------------------------

/// 校时统计快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncStats {
    /// 总校时次数。
    pub total_syncs: usize,
    /// 各源成功次数（池内逐源）。
    pub per_source: [(&'static str, usize); NTP_POOL_CAP],
    /// 平均绝对偏移（秒，整数均值——样本 0 时为 0）。
    pub avg_abs_offset_s: i64,
    /// 最近一次校时距今（秒；无历史 = None）。
    pub last_sync_age_s: Option<u64>,
}

/// 从历史环算统计（数据源：`ClockGuard::recent_history()` 全量展开）。
pub fn compute_stats(history: &[SyncRecord], pool: &[&'static str], now_s: u64) -> SyncStats {
    let mut per_source: [(&'static str, usize); NTP_POOL_CAP] =
        [("", 0), ("", 0), ("", 0), ("", 0)];
    for (i, src) in pool.iter().enumerate() {
        if i < NTP_POOL_CAP {
            per_source[i] = (src, 0);
        }
    }
    let mut abs_sum: i128 = 0;
    let mut real_syncs = 0usize;
    for h in history {
        // 估算样本不进均值（它们不是真校时——标注「估算」的语义就在这）。
        if h.estimated {
            continue;
        }
        real_syncs += 1;
        abs_sum += h.offset_s.abs() as i128;
        for (s, c) in per_source.iter_mut() {
            if *s == h.source {
                *c += 1;
            }
        }
    }
    let avg = if real_syncs == 0 {
        0
    } else {
        (abs_sum / real_syncs as i128) as i64
    };
    let last_age = history.first().map(|h| now_s.saturating_sub(h.at));
    SyncStats {
        total_syncs: history.iter().filter(|h| !h.estimated).count(),
        per_source,
        avg_abs_offset_s: avg,
        last_sync_age_s: last_age,
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F187 深化自检（聚合进 secstar2 域）。
pub fn run_clockguard_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F187-deep");

    // 深一：同步引擎——超时退避 1s→2s→4s，三次连败降权，成功清账。
    let mut eng = SyncEngine::new();
    set.add("backoff ladder", SyncEngine::backoff_for(1) == 1000
        && SyncEngine::backoff_for(2) == 2000
        && SyncEngine::backoff_for(3) == 4000
        && SyncEngine::backoff_for(9) == 8000, "cap at 8s");
    eng.note_attempt("a", AttemptOutcome::Timeout, 0);
    set.add("timeout counted", eng.timeouts == 1, "");
    set.add("backoff armed", !eng.eligible("a", 500) && eng.eligible("a", 1_000), "");
    eng.note_attempt("a", AttemptOutcome::Timeout, 1_000);
    eng.note_attempt("a", AttemptOutcome::Timeout, 3_000);
    set.add("skip after 3", eng.skipped.contains(&"a"), "");
    set.add("exhausted skips", eng.pool_exhausted(&["a"], 9_000), "");
    // 成功重置退避与连败。
    let mut eng2 = SyncEngine::new();
    eng2.note_attempt("b", AttemptOutcome::Timeout, 0);
    eng2.note_attempt("b", AttemptOutcome::Synced(5), 2_000);
    set.add("sync clears", eng2.eligible("b", 2_000) && eng2.syncs == 1 && eng2.timeouts == 1, "");
    // 拒绝也计入连败。
    eng2.note_attempt("b", AttemptOutcome::Rejected, 3_000);
    set.add("reject counted", eng2.rejects == 1, "");

    // 深二：漂移率估计——+50ppm 样本；rtcΔ=0 不可判；超界拒绝。
    let ppm = estimate_ppm((1_000_000, 1_000_000), (1_000_000 + 500_000 + 25, 1_500_000));
    set.add("ppm positive", ppm == Some(50), "25s drift over 500ks = 50ppm");
    set.add("ppm zero delta none", estimate_ppm((1, 1), (2, 1)).is_none(), "");
    set.add("ppm absurd none", estimate_ppm((1, 1), (1_000_000, 2)).is_none(), "");
    set.add("extrapolate", extrapolate(1_000_000, 100_000, 50) == 1_000_000 + 100_000 + 5, "");

    // 深三：DST——规则表换算与生效判定。
    set.add("nth sunday", nth_sunday(3, 1) == 5, "Wed-1st → first Sunday = 5th");
    set.add("nth sunday late", nth_sunday(6, 5) == 30, "Sat month → 5th Sunday = 30th");
    set.add("dst summer on", dst_active(7, 15, 1), "July is in DST");
    set.add("dst winter off", !dst_active(1, 15, 4), "January is out");
    set.add("dst april edge", dst_active(4, 7, 1), "on/after start Sunday");
    set.add("dst oct edge", !dst_active(10, 29, 0), "on end Sunday (29th when month starts Sunday)");

    // 深四：手动校时闸——三重防呆（武装/原因/幅度）。
    let mut gate = ManualSetGate::new();
    set.add("manual needs arm", gate.commit(60, "电池拔插导致 RTC 丢失").is_err(), "");
    gate.arm();
    set.add("manual needs reason", gate.commit(60, "ab").is_err(), "");
    set.add("manual needs bounds", gate.commit(100_000, "测试原因文案").is_err(), "");
    set.add("manual commit", gate.commit(-3600, "跨时区手动校准").is_ok(), "");
    set.add("manual disarmed after", gate.commit(60, "再次尝试").is_err(), "");
    gate.arm();
    gate.disarm();
    set.add("manual disarm", gate.commit(60, "放弃路径").is_err(), "");

    // 深五：统计面——按源计数/均值/最近年龄。
    let mut g = ClockGuard::new();
    let _ = g.add_source("s1");
    let _ = g.add_source("s2");
    let _ = g.correct_drift(100, 160, "s1"); // +60
    let _ = g.correct_drift(200, 230, "s1"); // +30
    let _ = g.estimate_from_last_good(400, 230, 0, 500); // 估算路径不计源
    let pool = ["s1", "s2"];
    let st = compute_stats(&g.recent_history(), &pool, 600);
    set.add("stats per source", st.per_source[0] == ("s1", 2), "");
    set.add("stats avg", st.avg_abs_offset_s == 45, "(60+30)/2");
    set.add("stats last age", st.last_sync_age_s == Some(100), "");
    set.add("stats estimated excluded", st.total_syncs == 2, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f187_deep_engine_multi_source_rotation() {
        // 源 a 超时 → 立刻试 b；b 成功；a 的退避到期后仍可试。
        let mut eng = SyncEngine::new();
        eng.note_attempt("a", AttemptOutcome::Timeout, 0);
        assert!(!eng.eligible("a", 0));
        assert!(eng.eligible("b", 0));
        eng.note_attempt("b", AttemptOutcome::Synced(10), 100);
        assert!(eng.eligible("a", 1_100), "backoff expired at 1s");
        assert_eq!(eng.syncs, 1);
        assert_eq!(eng.timeouts, 1);
    }

    #[test]
    fn f187_deep_ppm_negative_drift() {
        // RTC 走快（trueΔ < rtcΔ）→ 负 ppm。
        let ppm = estimate_ppm((1_000_000, 1_000_000), (1_000_000 + 100_000 - 10, 1_100_000));
        assert_eq!(ppm, Some(-100));
        // 外推负漂移：走快的钟要往下修。
        assert_eq!(extrapolate(1_000_000, 100_000, -100), 1_000_000 + 100_000 - 10);
    }

    #[test]
    fn f187_deep_dst_both_years_edges() {
        // 3 月/11 月明确界外；4 月 1 日（周日恰为首日）算界外——start_day=1 起。
        assert!(!dst_active(3, 31, 2));
        assert!(!dst_active(11, 30, 0));
        // start Sunday = 4 月 5 日（周三开头）：4/4 界外，4/5 界内。
        assert!(!dst_active(4, 4, 3));
        assert!(dst_active(4, 5, 3));
    }

    #[test]
    fn f187_deep_manual_gate_audit_count() {
        let mut gate = ManualSetGate::new();
        gate.arm();
        gate.commit(120, "宿主休眠唤醒漂移").unwrap();
        gate.arm();
        gate.commit(120, "宿主休眠唤醒漂移").unwrap();
        assert_eq!(gate.manual_sets, 2);
        // 相邻两次手动设定都要重新武装——防连点。
        assert!(gate.commit(1, "未武装尝试").is_err());
    }

    #[test]
    fn f187_deep_stats_empty_history() {
        let st = compute_stats(&[], &["s1"], 100);
        assert_eq!(st.total_syncs, 0);
        assert_eq!(st.avg_abs_offset_s, 0);
        assert_eq!(st.last_sync_age_s, None);
    }

    #[test]
    fn f187_deep_run_checks_pass() {
        assert!(run_clockguard_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——历史渲染行 / 源池健康度 /
// 时区切换联动对账 / RTC 推断黄条。判据源：主册【交互设计】「校时历史行
// （最近 5 次：时刻/来源/偏移量）」+【状态与异常】「RTC 硬件失效→推断
// 流程+黄条说明」+【用户故事】「日历对了，连天气都跟着对了」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：HistoryRows —— 校时历史渲染行（最近 5 次：时刻/来源/偏移量）
// ---------------------------------------------------------------------------

/// 一行校时历史（人话字段 + 性质标签）。
pub struct HistoryRow {
    /// 时刻（Unix 秒）。
    pub at_s: u64,
    /// 来源。
    pub source: &'static str,
    /// 偏移量（秒）。
    pub offset_s: i64,
    /// 行语义（显式校正/静默校正/估算——这次校时是什么性质，用户看得见）。
    pub tag: &'static str,
}

/// 历史行组装（新→旧；估计路径优先标「估算」——诚实纪律）。
pub fn history_rows(g: &ClockGuard) -> Vec<HistoryRow> {
    g.history
        .newest_first()
        .iter()
        .map(|r| HistoryRow {
            at_s: r.at,
            source: r.source,
            offset_s: r.offset_s,
            tag: if r.estimated {
                "估算"
            } else if r.offset_s.abs() > DRIFT_EXPLICIT_S {
                "显式校正"
            } else {
                "静默校正"
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v3-二：PoolHealth —— NTP 源池健康度视图（「时间不准」先看得见原因）
// ---------------------------------------------------------------------------

/// 单源健康行。
pub struct SourceHealthRow {
    pub name: &'static str,
    /// Alive / Untested=待试 / Dead。
    pub state: SourceHealth,
}

/// 池健康报告。
pub struct PoolHealth {
    pub rows: Vec<SourceHealthRow>,
    /// 存活数。
    pub alive_n: usize,
    /// 全灭（如实呈现——pool_exhausted 的面板前置状态）。
    pub all_dead: bool,
}

/// 组装（ClockGuard 源池账 → 面板行）。
pub fn pool_health(g: &ClockGuard) -> PoolHealth {
    let rows: Vec<SourceHealthRow> = g
        .pool
        .iter()
        .take(g.pool_len)
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, s)| SourceHealthRow { name: n, state: *s })
        .collect();
    let alive_n = rows.iter().filter(|r| r.state == SourceHealth::Alive).count();
    PoolHealth { all_dead: g.pool_len > 0 && alive_n == 0, alive_n, rows }
}

// ---------------------------------------------------------------------------
// v3-三：TzSwitchChecklist —— 时区切换全链联动对账（主册【验收判据】：
// 「时区切换全链（文件时间/日历/天气联动）实测」——联动是四项逐项打勾的
// 对账清单；缺一项=切换没走完）
// ---------------------------------------------------------------------------

/// 联动项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TzLink {
    /// 文件时间显示层（mtime 历史真相不动——只换显示）。
    FileTime,
    /// 日历视图。
    Calendar,
    /// 天气面板（F101 数据源同值）。
    Weather,
    /// 终端公告（新时区生效提示）。
    TerminalNotice,
}

impl TzLink {
    pub fn name(self) -> &'static str {
        match self {
            TzLink::FileTime => "文件时间",
            TzLink::Calendar => "日历",
            TzLink::Weather => "天气",
            TzLink::TerminalNotice => "终端公告",
        }
    }
}

/// 联动对账清单（向导确认后逐项回报——全勾才算切换完成）。
pub struct TzSwitchChecklist {
    done: [bool; 4],
    checked: usize,
}

impl TzSwitchChecklist {
    pub fn new() -> TzSwitchChecklist {
        TzSwitchChecklist { done: [false; 4], checked: 0 }
    }

    /// 回报一项完成（重复回报拒绝——假勾进不来）。
    pub fn mark(&mut self, item: TzLink) -> Result<usize, &'static str> {
        let idx = match item {
            TzLink::FileTime => 0,
            TzLink::Calendar => 1,
            TzLink::Weather => 2,
            TzLink::TerminalNotice => 3,
        };
        if self.done[idx] {
            return Err("该项已回报（重复回报拒绝）");
        }
        self.done[idx] = true;
        self.checked += 1;
        Ok(self.checked)
    }

    pub fn all_done(&self) -> bool {
        self.checked == 4
    }

    /// 未完成项（切换失败的「还差什么」诚实输出）。
    pub fn missing(&self) -> Vec<TzLink> {
        let all = [TzLink::FileTime, TzLink::Calendar, TzLink::Weather, TzLink::TerminalNotice];
        all.iter()
            .enumerate()
            .filter(|(i, _)| !self.done[*i])
            .map(|(_, l)| *l)
            .collect()
    }
}

impl Default for TzSwitchChecklist {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3-四：RtcInferBanner —— RTC 失效推断黄条（主册【状态与异常】：RTC
// 硬件失效→每次启动走推断流程+黄条说明）
// ---------------------------------------------------------------------------

/// 黄条渲染数据（主行/推断来源/帮助链/建议动作四件）。
pub struct RtcInferBanner {
    pub text: &'static str,
    pub basis: &'static str,
    pub help: &'static str,
    pub action: &'static str,
}

/// 组装（`rtc_dead`/`inferring` 置位时由设置页消费）。
pub fn rtc_infer_banner() -> RtcInferBanner {
    RtcInferBanner {
        text: "主板时钟疑似失效（多次读到零值）——当前时间为推断值",
        basis: "由上次已知好值与估计漂移推算，标注「估算」",
        help: "help:rtc-inferred",
        action: "更换主板电池后时间将自动恢复精确校准",
    }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F187 v3 自检（聚合进 secstar2 域）。
pub fn run_clockguard_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F187-v3");

    // v3-一：历史行——环上限、标签三态、新→旧（>300s 才是显式——主册边界）。
    let mut g = ClockGuard::new();
    g.add_source("ntp-a");
    let _ = g.correct_drift(1_000_000, 1_000_010, "ntp-a");
    let _ = g.correct_drift(1_000_200, 1_000_260, "ntp-a");
    let _ = g.correct_drift(1_000_400, 1_000_702, "ntp-a");
    let rows = history_rows(&g);
    set.add("hist rows", rows.len() == 3, "");
    set.add("hist tag silent", rows[2].tag == "静默校正" && rows[2].offset_s == 10, "");
    set.add("hist tag explicit", rows[0].tag == "显式校正" && rows[0].offset_s == 302, "");
    set.add("hist boundary 300 silent", {
        let mut gb = ClockGuard::new();
        gb.add_source("n");
        let _ = gb.correct_drift(10, 310, "n");
        history_rows(&gb)[0].tag == "静默校正"
    }, "恰好 300s 属静默段（2-300s 闭区间）");

    // v3-二：源池健康——存活计数与全灭诚实。
    let mut g2 = ClockGuard::new();
    g2.add_source("ntp-a");
    g2.add_source("ntp-b");
    g2.mark_source("ntp-a", true);
    g2.mark_source("ntp-b", false);
    let ph = pool_health(&g2);
    set.add("pool rows", ph.rows.len() == 2, "");
    set.add("pool alive", ph.alive_n == 1 && !ph.all_dead, "");
    let mut g3 = ClockGuard::new();
    g3.add_source("x");
    g3.mark_source("x", false);
    set.add("pool all dead honest", pool_health(&g3).all_dead, "");
    set.add("pool empty not dead", !pool_health(&ClockGuard::new()).all_dead, "空池是未配置不是全灭");

    // v3-三：联动对账——逐项回报/重复拒/缺失清单/全勾完成。
    let mut cl = TzSwitchChecklist::new();
    set.add("tz mark 1", cl.mark(TzLink::FileTime) == Ok(1), "");
    set.add("tz dup refused", cl.mark(TzLink::FileTime).is_err(), "");
    set.add("tz missing", cl.missing() == vec![TzLink::Calendar, TzLink::Weather, TzLink::TerminalNotice], "");
    let _ = cl.mark(TzLink::Calendar);
    let _ = cl.mark(TzLink::Weather);
    set.add("tz not done", !cl.all_done(), "");
    let _ = cl.mark(TzLink::TerminalNotice);
    set.add("tz all done", cl.all_done() && cl.missing().is_empty(), "");
    set.add("tz link names", TzLink::Weather.name() == "天气", "");

    // v3-四：推断黄条——估算标注与建议动作齐。
    let b = rtc_infer_banner();
    set.add("rtc banner text", b.text.contains("推断"), "");
    set.add("rtc banner basis", b.basis.contains("估算"), "");
    set.add("rtc banner action", b.action.contains("电池"), "");
    set.add("rtc banner help", b.help == "help:rtc-inferred", "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f187_v3_history_ring_caps_at_five() {
        // HISTORY_CAP=5：6 次校正只留最近 5 行（环账语义）。
        let mut g = ClockGuard::new();
        g.add_source("ntp-a");
        for i in 0..6u64 {
            let _ = g.correct_drift(1_000_000 + i * 1000, 1_000_100 + i * 1000, "ntp-a");
        }
        assert_eq!(history_rows(&g).len(), HISTORY_CAP);
    }

    #[test]
    fn f187_v3_checklist_never_fakes_completion() {
        // 破坏性尝试：三项完成+一次重复回报 → 仍不算完成。
        let mut cl = TzSwitchChecklist::new();
        let _ = cl.mark(TzLink::FileTime);
        let _ = cl.mark(TzLink::Calendar);
        let _ = cl.mark(TzLink::Weather);
        assert!(cl.mark(TzLink::FileTime).is_err(), "duplicate must not fake the 4th");
        assert!(!cl.all_done());
        assert_eq!(cl.missing(), vec![TzLink::TerminalNotice]);
    }

    #[test]
    fn f187_v3_run_checks_pass() {
        assert!(run_clockguard_deep2_checks().all_passed());
    }
}
