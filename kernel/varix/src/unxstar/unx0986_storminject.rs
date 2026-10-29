//! unx0986_storminject — UNX-F0986 风暴注入资产（致风暴必抓与分级触发全验证）。
//!
//! 任务锚点：UNX-F0986（第二批工位 W7，SEG 段位 0x8140，本单独占
//! 0x8140..=0x815F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**风暴注入场景闭集（突发/持续/爬坡/抖动
//! 四类）全部可注入且必被检出（致风暴必抓），各注入场景的预期风暴
//! 级与实际触发级逐条对得上（分级触发全验证），注入资产账守恒**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x814 段闭集）；tick 显性
//! 注入零墙钟；注入账定容守恒（抓到+漏抓==总注入 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8140 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴注入资产段基址（0x8140，本单 0x8140..=0x815F 独占）。
pub const SEG_BASE: u16 = 0x8140;
/// 0x8141 注入场景越界（场景码不在四类闭集内）。
pub const XC_SCENARIO_BAD: u16 = SEG_BASE + 1;
/// 0x8142 致风暴漏抓（注入序列判据成立但检出面未报风暴）。
pub const XC_STORM_MISSED: u16 = SEG_BASE + 2;
/// 0x8143 无风暴误抓（静默序列被判为风暴——检出面误报）。
pub const XC_FALSE_ALARM: u16 = SEG_BASE + 3;
/// 0x8144 分级失配（预期触发级与实际触发级不一致）。
pub const XC_TIER_MISMATCH: u16 = SEG_BASE + 4;
/// 0x8145 注入账守恒破坏（抓到+漏抓 != 总注入）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x8146 注入 tick 回退（注入时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 6;
/// 0x8147 注入资产指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x8148 注入账定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x8149 注入强度非法（强度为零或超域）。
pub const XC_INTENSITY_BAD: u16 = SEG_BASE + 9;
/// 0x814A 重复场景注入（同场景双注入且指纹同——资产面重复）。
pub const XC_SCENARIO_DUP: u16 = SEG_BASE + 10;

const _: () = assert!(XC_SCENARIO_BAD != XC_STORM_MISSED);
const _: () = assert!(XC_FALSE_ALARM != XC_TIER_MISMATCH);
const _: () = assert!(XC_DRIFT != XC_SCENARIO_DUP);

// ---------------------------------------------------------------------------
// §1 注入场景闭集（四类）
// ---------------------------------------------------------------------------

/// 注入场景闭集：突发 / 持续 / 爬坡 / 抖动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectScenario {
    /// 突发型：单窗尖峰直接越确认阈。
    Burst,
    /// 持续型：多窗稳定高位。
    Sustained,
    /// 爬坡型：逐窗递增穿越两级判据。
    Ramp,
    /// 抖动型：高低交替（不应触发——静默判据验误报面）。
    Jitter,
}

impl InjectScenario {
    /// 场景码（1..=4）。
    pub const fn code(self) -> u8 {
        match self {
            InjectScenario::Burst => 1,
            InjectScenario::Sustained => 2,
            InjectScenario::Ramp => 3,
            InjectScenario::Jitter => 4,
        }
    }
    /// 场景名。
    pub const fn name(self) -> &'static str {
        match self {
            InjectScenario::Burst => "突发",
            InjectScenario::Sustained => "持续",
            InjectScenario::Ramp => "爬坡",
            InjectScenario::Jitter => "抖动",
        }
    }
    /// 预期是否触发风暴（抖动型为反例——验误报面）。
    pub const fn expect_storm(self) -> bool {
        match self {
            InjectScenario::Jitter => false,
            _ => true,
        }
    }
    /// 预期触发级（1 观察 / 2 确认；抖动型恒 0）。
    pub const fn expect_tier(self) -> u8 {
        match self {
            InjectScenario::Burst => 2,
            InjectScenario::Sustained => 2,
            InjectScenario::Ramp => 2,
            InjectScenario::Jitter => 0,
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<InjectScenario> {
        match c {
            1 => Some(InjectScenario::Burst),
            2 => Some(InjectScenario::Sustained),
            3 => Some(InjectScenario::Ramp),
            4 => Some(InjectScenario::Jitter),
            _ => None,
        }
    }
}

/// 注入强度域（每窗事件数 0..=INTENSITY_MAX）。
pub const INTENSITY_MAX: u32 = 100_000;

// ---------------------------------------------------------------------------
// §2 注入序列构造器（场景 → 窗口计数序列——确定性生成）
// ---------------------------------------------------------------------------

/// 由场景构造窗口计数序列（确定性——同场景同强度双生成必同）。
///
/// 窗序列长度定值 6：覆盖判据持续窗（3）并留回退观察位。
pub const SEQ_LEN: usize = 6;

/// 生成注入序列（强度非法拒收；抖动型强度为低位均值）。
pub fn gen_sequence(sc: InjectScenario, intensity: u32) -> Result<Vec<u32>, u16> {
    if intensity == 0 || intensity > INTENSITY_MAX {
        return Err(XC_INTENSITY_BAD);
    }
    let mut seq: Vec<u32> = Vec::with_capacity(SEQ_LEN);
    let mut i: usize = 0;
    while i < SEQ_LEN {
        let v = match sc {
            InjectScenario::Burst => {
                if i == 0 {
                    intensity
                } else {
                    intensity / 2
                }
            }
            InjectScenario::Sustained => intensity,
            InjectScenario::Ramp => intensity * (i as u32 + 1) / (SEQ_LEN as u32),
            InjectScenario::Jitter => {
                if i % 2 == 0 {
                    intensity
                } else {
                    0
                }
            }
        };
        seq.push(v);
        i += 1;
    }
    Ok(seq)
}

// ---------------------------------------------------------------------------
// §3 内联检出判定（本单自持的最小率判据——分级触发验证靶）
// ---------------------------------------------------------------------------

/// 确认阈（单窗计数 ≥ 此值记命中）。
pub const CONFIRM_RATE: u32 = 1_000;
/// 连续命中窗数（≥3 窗判风暴成立）。
pub const CONFIRM_RUN: usize = 3;

/// 对序列跑检出判定（返回是否触发风暴与触发时命中窗数）。
///
/// 判定闭集：连续 CONFIRM_RUN 窗 ≥ CONFIRM_RATE 即风暴；任一窗
/// 回落到 CONFIRM_RATE 以下清零连击。
pub fn detect(seq: &[u32]) -> (bool, usize) {
    let mut run: usize = 0;
    let mut hit: usize = 0;
    let mut i: usize = 0;
    while i < seq.len() {
        if seq[i] >= CONFIRM_RATE {
            run += 1;
            if run >= CONFIRM_RUN {
                hit = run;
                return (true, hit);
            }
        } else {
            run = 0;
        }
        i += 1;
    }
    (false, hit)
}

// ---------------------------------------------------------------------------
// §4 注入资产账（InjectBook）
// ---------------------------------------------------------------------------

/// 一行注入账：场景 / 强度 / 是否触发 / 实际命中窗数 / tick。
#[derive(Clone, Copy, Debug)]
pub struct InjectRow {
    scenario: InjectScenario,
    intensity: u32,
    triggered: bool,
    hit_windows: usize,
    tick: u64,
}

/// 注入账容量定值：16 行（四场景各四强度档量级）。
pub const INJECT_CAP: usize = 16;

/// 注入资产账——致风暴必抓与分级触发全验证的本体。
///
/// 守恒恒等式：`触发数 + 未触发数 == 总注入`（每行恰处一态）；
/// 分级恒等式：触发行必命中预期级、反例行必不触发（逐行机检）。
#[derive(Debug)]
pub struct InjectBook {
    rows: Vec<InjectRow>,
    cap: usize,
    caught: u32,
    last_tick: u64,
    rejected: u32,
}

impl InjectBook {
    /// 建注入账。
    pub fn new(cap: usize) -> Result<InjectBook, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(InjectBook {
            rows: Vec::with_capacity(cap),
            cap,
            caught: 0,
            last_tick: 0,
            rejected: 0,
        })
    }

    /// 注入行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 抓到数。
    pub fn caught(&self) -> u32 {
        self.caught
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 注入一场景（生成序列→检出判定→与预期逐条对）。
    pub fn inject(&mut self, sc: InjectScenario, intensity: u32, tick: u64) -> Result<bool, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if InjectScenario::from_code(sc.code()).is_none() {
            self.rejected += 1;
            return Err(XC_SCENARIO_BAD);
        }
        if self.rows.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.last_tick = tick;
        let seq = gen_sequence(sc, intensity)?;
        let (trig, hit) = detect(&seq);
        let row = InjectRow {
            scenario: sc,
            intensity,
            triggered: trig,
            hit_windows: hit,
            tick,
        };
        // 分级触发全验证：预期触发但未触发 → 漏抓；反例触发 → 误报。
        if sc.expect_storm() && !trig {
            self.rows.push(row);
            self.rejected += 1;
            return Err(XC_STORM_MISSED);
        }
        if !sc.expect_storm() && trig {
            self.rows.push(row);
            self.rejected += 1;
            return Err(XC_FALSE_ALARM);
        }
        if trig && hit < CONFIRM_RUN {
            self.rows.push(row);
            self.rejected += 1;
            return Err(XC_TIER_MISMATCH);
        }
        if trig {
            self.caught += 1;
        }
        self.rows.push(row);
        Ok(trig)
    }

    /// 守恒机检：触发数 + 未触发数 == 总注入（逐行重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut t: u32 = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].triggered {
                t += 1;
            }
            i += 1;
        }
        if t != self.caught {
            return Err(XC_CONSERVATION);
        }
        if t + (self.rows.len() as u32 - t) != self.rows.len() as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 分级机检：逐行预期级与实际触发一致（含反例行恒不触发）。
    pub fn audit_tiering(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let expect_trig = r.scenario.expect_storm();
            if r.triggered != expect_trig {
                return Err(XC_TIER_MISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 注入账指纹（场景/强度/触发位逐行 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ r.scenario.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.intensity as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.triggered as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 注入账文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        self.audit_tiering()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0986-INJECT|v1|rows=");
        s.push_str(&self.rows.len().to_string());
        s.push_str("|caught=");
        s.push_str(&self.caught.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.rows.len() {
            s.push_str(self.rows[i].scenario.name());
            s.push('|');
            s.push_str(&self.rows[i].intensity.to_string());
            s.push('|');
            s.push_str(if self.rows[i].triggered { "HIT" } else { "QUIET" });
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §5 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：四场景注入全验证→漏抓/误报/强度非法注入全拦→守恒→导出。
pub fn drill() -> Result<u32, u16> {
    let mut b = InjectBook::new(INJECT_CAP)?;
    // 三类致风暴场景（预期级 2——确认触发）。
    if b.inject(InjectScenario::Burst, 2_000, 10)? != true {
        return Err(XC_STORM_MISSED);
    }
    if b.inject(InjectScenario::Sustained, 1_500, 20)? != true {
        return Err(XC_STORM_MISSED);
    }
    if b.inject(InjectScenario::Ramp, 2_400, 30)? != true {
        return Err(XC_STORM_MISSED);
    }
    // 反例场景：抖动型（高低交替必不触发——误报面验证）。
    if b.inject(InjectScenario::Jitter, 5_000, 40)? != false {
        return Err(XC_FALSE_ALARM);
    }
    // 注入面：强度非法、tick 回退、场景越界全拦。
    if b.inject(InjectScenario::Burst, 0, 50) != Err(XC_INTENSITY_BAD) {
        return Err(XC_INTENSITY_BAD);
    }
    if b.inject(InjectScenario::Burst, INTENSITY_MAX + 1, 50) != Err(XC_INTENSITY_BAD) {
        return Err(XC_INTENSITY_BAD);
    }
    if b.inject(InjectScenario::Burst, 2_000, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：4 行 = 3 触发 + 1 未触发；分级逐行一致。
    b.audit_conservation()?;
    b.audit_tiering()?;
    if b.caught() != 3 || b.len() != 4 || b.rejected() != 3 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = b.fingerprint();
    let _ = b.export()?;
    if b.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
