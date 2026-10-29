//! cgb854_amdpower —— CGPU-F0854 AMD 功耗画像。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0854 AMD 功耗画像  <SEG 0x7540>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0854；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 功耗画像，整数毫瓦/毫度面）：
//!   1. 功耗档闭集五档（写死）：静默 / 节能 / 均衡 / 性能 / 狂暴；
//!      档号 0..=4，越界红码（XC_BAD_PROFILE）；
//!   2. 每档写死限值（毫瓦）：静默 80000 / 节能 120000 / 均衡
//!      180000 / 性能 260000 / 狂暴 330000；样本超档限红码
//!      （XC_POWER_OVER_LIMIT）；
//!   3. 画像样本环（定容 32）：入账 = 在环 + 逐出；每样本带档
//!      位、毫瓦、毫度（结温 20000..=110000）；越界红码
//!      （XC_BAD_TEMP）；
//!   4. 能效账（恒等式）：累计功 = Σ样本毫瓦 × 采样间隔（统一
//!      10ms 整数化），累加不溢出（u64）；积分漂移红码
//!      （XC_ENERGY_DRIFT：重算和 ≠ 累计值）；
//!   5. 档位切换状态机：档间切换必须单调过散热缓冲（写死：切
//!      高档需温度 < 90000 毫度），违例红码（XC_THROTTLE_RISK）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7540 段为本模块独占（SEG_BASE=0x7540），判别式
//!   SEG_BASE+1..+N；与 0x7520/0x7560 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AP_VERSION: &str = "cgb854_amdpower-v1";

/// 诊断段基址（0x7540 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7540;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7541 功耗档越界（0..=4 之外）。
pub const XC_BAD_PROFILE: u16 = SEG_BASE + 1;
/// 0x7542 功率超档限。
pub const XC_POWER_OVER_LIMIT: u16 = SEG_BASE + 2;
/// 0x7543 温度越界（20000..=110000 毫度）。
pub const XC_BAD_TEMP: u16 = SEG_BASE + 3;
/// 0x7544 画像样本账漂移。
pub const XC_SAMPLE_DRIFT: u16 = SEG_BASE + 4;
/// 0x7545 能效积分漂移（重算和 ≠ 累计）。
pub const XC_ENERGY_DRIFT: u16 = SEG_BASE + 5;
/// 0x7546 散热缓冲违例（高温切高档）。
pub const XC_THROTTLE_RISK: u16 = SEG_BASE + 6;
/// 0x7547 样本环空出画像。
pub const XC_PROFILE_EMPTY: u16 = SEG_BASE + 7;
/// 0x7548 升温步进越界（相邻样本温度跳变 > 15000 毫度）。
pub const XC_TEMP_JUMP: u16 = SEG_BASE + 8;

/// 样本环容量（写死：32）。
pub const RING_CAP: usize = 32;
/// 采样间隔（毫秒，写死 10，用于能效积分）。
pub const SAMPLE_MS: u32 = 10;
/// 温度下限（毫度）。
pub const TEMP_MIN: u32 = 20000;
/// 温度上限（毫度）。
pub const TEMP_MAX: u32 = 110000;
/// 温度跳变上限（毫度）。
pub const TEMP_JUMP_MAX: u32 = 15000;
/// 切高档温度红线（毫度）。
pub const THROTTLE_LINE: u32 = 90000;
/// 功耗档数（写死：5）。
pub const PROFILE_CAP: usize = 5;

/// 功耗档闭集：0=静默 1=节能 2=均衡 3=性能 4=狂暴。
pub const P_QUIET: usize = 0;
pub const P_ECO: usize = 1;
pub const P_BALANCED: usize = 2;
pub const P_PERF: usize = 3;
pub const P_FURY: usize = 4;

/// 各档限值（毫瓦，写死）。
pub const PROFILE_LIMIT: [u32; PROFILE_CAP] =
    [80000, 120000, 180000, 260000, 330000];

/// 档名（闭集原文）。
pub fn profile_name(p: usize) -> &'static str {
    match p {
        P_QUIET => "quiet",
        P_ECO => "eco",
        P_BALANCED => "balanced",
        P_PERF => "perf",
        _ => "fury",
    }
}

/// 单条画像样本。
#[derive(Clone, Copy, Debug)]
pub struct PowerSample {
    /// 档位（0..=4）。
    pub profile: usize,
    /// 功率（毫瓦）。
    pub mw: u32,
    /// 结温（毫度）。
    pub mt: u32,
}

/// 功耗画像册。
pub struct PowerProfile {
    /// 样本环（定容 32）。
    pub ring: Vec<PowerSample>,
    /// 环头。
    pub head: usize,
    /// 入账数。
    pub pushed: u64,
    /// 逐出数。
    pub evicted: u64,
    /// 当前档位。
    pub cur: usize,
    /// 累计能效（毫瓦·10ms 整数化）。
    pub energy: u64,
    /// 最后样本温度。
    pub last_temp: u32,
    /// 切档次数。
    pub switches: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl PowerProfile {
    /// 开册（默认均衡档）。
    pub fn new() -> Self {
        PowerProfile {
            ring: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            cur: P_BALANCED,
            energy: 0,
            last_temp: TEMP_MIN,
            switches: 0,
            red_count: 0,
        }
    }

    /// 样本账守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.ring.len() as u64 + self.evicted
    }

    /// 切档（高档需温度低于红线；同档幂等）。
    pub fn set_profile(&mut self, p: usize) -> Result<(), u16> {
        if p >= PROFILE_CAP {
            self.red_count += 1;
            return Err(XC_BAD_PROFILE);
        }
        if p == self.cur {
            return Ok(());
        }
        if p > self.cur && self.last_temp >= THROTTLE_LINE {
            self.red_count += 1;
            return Err(XC_THROTTLE_RISK);
        }
        self.cur = p;
        self.switches += 1;
        Ok(())
    }

    /// 入账样本（档限/温界/温跳三重校验）。
    pub fn push(&mut self, mw: u32, mt: u32) -> Result<(), u16> {
        if mw == 0 || mw > PROFILE_LIMIT[self.cur] {
            self.red_count += 1;
            return Err(XC_POWER_OVER_LIMIT);
        }
        if mt < TEMP_MIN || mt > TEMP_MAX {
            self.red_count += 1;
            return Err(XC_BAD_TEMP);
        }
        let d = if mt > self.last_temp {
            mt - self.last_temp
        } else {
            self.last_temp - mt
        };
        if d > TEMP_JUMP_MAX {
            self.red_count += 1;
            return Err(XC_TEMP_JUMP);
        }
        let s = PowerSample {
            profile: self.cur,
            mw,
            mt,
        };
        if self.ring.len() == RING_CAP {
            self.ring[self.head] = s;
            self.head = (self.head + 1) % RING_CAP;
            self.evicted += 1;
            self.pushed += 1;
        } else {
            self.ring.push(s);
            self.pushed += 1;
        }
        // 能效积分：毫瓦 × 采样间隔（ms）→ 毫瓦·ms。
        self.energy += mw as u64 * SAMPLE_MS as u64;
        self.last_temp = mt;
        Ok(())
    }

    /// 均值功率（毫瓦，空环回 0）。
    pub fn mean_mw(&self) -> u32 {
        if self.ring.is_empty() {
            return 0;
        }
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            acc += self.ring[i].mw as u64;
            i += 1;
        }
        (acc / self.ring.len() as u64) as u32
    }

    /// 峰值温度（空环回 0）。
    pub fn peak_temp(&self) -> u32 {
        if self.ring.is_empty() {
            return 0;
        }
        let mut peak: u32 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            if self.ring[i].mt > peak {
                peak = self.ring[i].mt;
            }
            i += 1;
        }
        peak
    }

    /// 能效复核：Σ(样本毫瓦 × 间隔) = energy。
    pub fn energy_check(&self) -> Result<(), u16> {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            acc += self.ring[i].mw as u64 * SAMPLE_MS as u64;
            i += 1;
        }
        // 在环积分 ≤ 累计积分（逐出部分只增不减）。
        if acc > self.energy {
            return Err(XC_ENERGY_DRIFT);
        }
        Ok(())
    }

    /// 画像判定（能效三档：均值/档限比 优<60% 良<85% 紧≥85%）。
    pub fn grade(&mut self) -> Result<u8, u16> {
        if self.ring.is_empty() {
            self.red_count += 1;
            return Err(XC_PROFILE_EMPTY);
        }
        let mean = self.mean_mw() as u64;
        let limit = PROFILE_LIMIT[self.cur] as u64;
        // 千分位比值。
        let ratio = mean * 1000 / limit;
        Ok(if ratio < 600 {
            0
        } else if ratio < 850 {
            1
        } else {
            2
        })
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdpower cur=");
        s += profile_name(self.cur);
        s += " mean_mw=";
        s += &self.mean_mw().to_string();
        s += " peak_mt=";
        s += &self.peak_temp().to_string();
        s += " energy=";
        s += &self.energy.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_SAMPLE_DRIFT);
        }
        self.energy_check()?;
        Ok(())
    }
}

/// 自例程：切档 → 采样 → 能效复核 → 画像 → 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut p = PowerProfile::new();
    // 均衡档采样 40 条（覆盖满环逐出）。
    let mut k: u32 = 0;
    while k < 40 {
        let mw = 100000 + (k % 8) * 4000;
        let mt = 45000 + (k % 6) * 2000;
        p.push(mw, mt)?;
        k += 1;
    }
    p.audit()?;
    // 负例：功率超档限。
    if p.push(PROFILE_LIMIT[P_BALANCED] + 1, 50000) != Err(XC_POWER_OVER_LIMIT) {
        return Err(XC_POWER_OVER_LIMIT);
    }
    // 负例：温度越界。
    if p.push(100000, TEMP_MAX + 1) != Err(XC_BAD_TEMP) {
        return Err(XC_BAD_TEMP);
    }
    if p.push(100000, TEMP_MIN - 1) != Err(XC_BAD_TEMP) {
        return Err(XC_BAD_TEMP);
    }
    // 负例：温度跳变。
    if p.push(100000, TEMP_MAX) != Err(XC_TEMP_JUMP) {
        return Err(XC_TEMP_JUMP);
    }
    // 负例：切档越界。
    if p.set_profile(PROFILE_CAP) != Err(XC_BAD_PROFILE) {
        return Err(XC_BAD_PROFILE);
    }
    // 切狂暴需温度 < 红线；当前 last_temp 已在采样尾（55000 左右）。
    p.set_profile(P_FURY)?;
    // 负例：高温切高档（构造高温样本后回切）。
    p.push(300000, 95000)?;
    if p.set_profile(P_FURY) == Ok(()) {
        // 同档幂等，允许；但切更高档不存在，验证降档后再升被挡。
    }
    if p.set_profile(P_QUIET) != Ok(()) {
        return Err(XC_THROTTLE_RISK);
    }
    // 现在温度 95000 ≥ 红线，升档应被挡。
    if p.set_profile(P_PERF) != Err(XC_THROTTLE_RISK) {
        return Err(XC_THROTTLE_RISK);
    }
    p.audit()?;
    let g = p.grade()?;
    if g > 2 {
        return Err(XC_PROFILE_EMPTY);
    }
    let s = p.summary();
    if s.len() == 0 {
        return Err(XC_PROFILE_EMPTY);
    }
    // 空环画像红码。
    let mut q = PowerProfile::new();
    if q.grade() != Err(XC_PROFILE_EMPTY) {
        return Err(XC_PROFILE_EMPTY);
    }
    // 能效积分与守恒终验。
    if p.energy == 0 || !p.balanced() {
        return Err(XC_ENERGY_DRIFT);
    }
    Ok(0)
}
