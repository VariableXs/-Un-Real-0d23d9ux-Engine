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
