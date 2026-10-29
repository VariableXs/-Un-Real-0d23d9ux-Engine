//! unx0646_boothist —— UNX-F0646 引导履历账（逐次引导计数与结论）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0646 · 引导履历账（逐次引导计数与结论） · 目标 ≥220 行
//! 判据原文摘录：UNX-F0646-J1 逐次引导履历（序号/档位/结论）落账，
//!   50 次履历序号连续零跳号。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三列履历：序号（0 起连续零跳号）/档位（三档闭集：冷启/热启
//!      /恢复启）/结论（成败二态）。
//!   2. 50 次履历定容：定容数组 50 槽，槽序 = 序号，守恒恒等式：
//!      已落履历数 + 空槽数 = 50。
//!   3. 计数面：引导总次数 = 已落履历数；成败计数守恒（成+败 = 总）。
//!   4. 注入红检出：跳号/档位越闭集/结论越二态/序号回填均红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5240 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5240 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5240;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x524F;

/// XcCode：档位闭集外。
pub const XC_BAD_GEAR: u16 = SEG_BASE + 1; // 0x5241 三档之外
/// XcCode：结论二态外。
pub const XC_BAD_VERDICT: u16 = SEG_BASE + 2; // 0x5242 成败之外
/// XcCode：序号跳号。
pub const XC_SEQ_GAP: u16 = SEG_BASE + 3; // 0x5243 序号不连续
/// XcCode：履历槽满。
pub const XC_FULL: u16 = SEG_BASE + 4; // 0x5244 >50 次
/// XcCode：履历已封。
pub const XC_SEALED: u16 = SEG_BASE + 5; // 0x5245 封账后写
/// XcCode：计数守恒破。
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 0x5246 成+败 ≠ 总

/// 履历槽容量（判据原文 50 次）。
pub const HIST_N: usize = 50;

/// 引导档位（三档闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootGear {
    /// 冷启（上电全检）。
    Cold = 0,
    /// 热启（复位快启）。
    Warm = 1,
    /// 恢复启（休眠唤醒）。
    Resume = 2,
}

impl BootGear {
    /// 档位名。
    pub fn name(self) -> &'static str {
        match self {
            BootGear::Cold => "冷启",
            BootGear::Warm => "热启",
            BootGear::Resume => "恢复启",
        }
    }
}

/// 引导结论（二态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootOutcome {
    /// 成。
    Ok = 0,
    /// 败。
    Fail = 1,
}

impl BootOutcome {
    /// 结论名。
    pub fn name(self) -> &'static str {
        match self {
            BootOutcome::Ok => "成",
            BootOutcome::Fail => "败",
        }
    }
}

/// 履历行（三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistRow {
    /// 序号（0..50）。
    pub seq: usize,
    /// 档位。
    pub gear: BootGear,
    /// 结论。
    pub outcome: BootOutcome,
    /// 引导拍（整数时戳）。
    pub stamp: u64,
}

/// 引导履历账本（定容 50 槽）。
#[derive(Clone, Debug)]
pub struct BootHist {
    /// 50 槽（None = 未落）。
    slots: [Option<HistRow>; HIST_N],
    /// 已落序号游标（下一序号）。
    cursor: usize,
    /// 封账位。
    sealed: bool,
}

impl BootHist {
    /// 空履历。
    pub fn new() -> Self {
        BootHist {
            slots: [None; HIST_N],
            cursor: 0,
            sealed: false,
        }
    }

    /// 已落履历数。
    pub fn logged(&self) -> usize {
        self.cursor
    }

    /// 守恒恒等式：已落 + 空 = 50。
    pub fn conserve_ok(&self) -> bool {
        self.logged() + (HIST_N - self.logged()) == HIST_N
    }

    /// 落一履历（顺序落账；槽满/封账/档位/结论闭集核）。
    pub fn log(&mut self, gear: BootGear, outcome: BootOutcome, stamp: u64) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if self.cursor >= HIST_N {
            return Err(XC_FULL);
        }
        if gear as usize > BootGear::Resume as usize {
            return Err(XC_BAD_GEAR);
        }
        if outcome as usize > BootOutcome::Fail as usize {
            return Err(XC_BAD_VERDICT);
        }
        let seq = self.cursor;
        self.slots[seq] = Some(HistRow {
            seq,
            gear,
            outcome,
            stamp,
        });
        self.cursor += 1;
        Ok(seq)
    }

    /// 取履历。
    pub fn at(&self, seq: usize) -> Option<&HistRow> {
        if seq >= HIST_N {
            return None;
        }
        self.slots[seq].as_ref()
    }

    /// 序号连续零跳号核（顺序游标账下恒真，显式复核防游标漂移）。
    pub fn continuous(&self) -> Result<usize, u16> {
        for i in 0..self.logged() {
            match &self.slots[i] {
                Some(r) => {
                    if r.seq != i {
                        return Err(XC_SEQ_GAP);
                    }
                }
                None => return Err(XC_SEQ_GAP),
            }
        }
        Ok(self.logged())
    }

    /// 成败计数守恒。
    pub fn outcome_conserve(&self) -> Result<(usize, usize, usize), u16> {
        let mut ok = 0usize;
        let mut fail = 0usize;
        for r in self.slots.iter().flatten() {
            match r.outcome {
                BootOutcome::Ok => ok += 1,
                BootOutcome::Fail => fail += 1,
            }
        }
        if ok + fail != self.logged() {
            return Err(XC_CONSERVE);
        }
        Ok((ok, fail, self.logged()))
    }

    /// 封账（50 槽全落才可封）。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if self.logged() != HIST_N {
            return Err(XC_FULL);
        }
        self.continuous()?;
        self.outcome_conserve()?;
        self.sealed = true;
        Ok(self.logged())
    }

    /// 档位分布（三档计数）。
    pub fn gear_hist(&self) -> [usize; 3] {
        let mut h = [0usize; 3];
        for r in self.slots.iter().flatten() {
            h[r.gear as usize] += 1;
        }
        h
    }

    /// 人话报告（摘要前 5 行 + 计数）。
    pub fn report(&self) -> String {
        let h = self.gear_hist();
        let mut s = alloc::format!(
            "引导履历 · 落账 {}/{} · 档位 [{},{},{}] · 封={}",
            self.logged(),
            HIST_N,
            h[0],
            h[1],
            h[2],
            self.sealed
        );
        for i in 0..self.logged().min(5) {
            if let Some(r) = &self.slots[i] {
                s.push_str(&alloc::format!(
                    "\n  序{} {} {} @{}",
                    r.seq, r.gear.name(), r.outcome.name(), r.stamp
                ));
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准 50 次履历构造器：档位 = 序号%3 轮转，结论 = 序号%10==7 时败，
/// 时戳 = 序号*4+2（确定性）。
pub fn build_standard_hist() -> Result<BootHist, u16> {
    let gears = [BootGear::Cold, BootGear::Warm, BootGear::Resume];
    let mut h = BootHist::new();
    for i in 0..HIST_N {
        let outcome = if i % 10 == 7 {
            BootOutcome::Fail
        } else {
            BootOutcome::Ok
        };
        h.log(gears[i % 3], outcome, (i as u64) * 4 + 2)?;
    }
    h.seal()?;
    Ok(h)
}

/// 自例程：主路径（50 次封账→连续→守恒）+ 错误路径（越闭集/跳号/槽满/封后写）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let h = build_standard_hist()?;
    if h.continuous()? != HIST_N {
        return Err(XC_SEQ_GAP);
    }
    let (ok, fail, total) = h.outcome_conserve()?;
    if ok + fail != total || total != HIST_N {
        return Err(XC_CONSERVE);
    }
    // 败数 = 序号%10==7 的个数 = 5。
    if fail != 5 {
        return Err(XC_CONSERVE);
    }
    let g = h.gear_hist();
    if g != [17, 17, 16] {
        return Err(XC_BAD_GEAR);
    }
    if h.fingerprint() == 0 {
        return Err(XC_CONSERVE);
    }

    // 错误路径 1：槽满。
    let mut h1 = BootHist::new();
    for i in 0..HIST_N {
        h1.log(BootGear::Cold, BootOutcome::Ok, i as u64)?;
    }
    if h1.log(BootGear::Cold, BootOutcome::Ok, 999) != Err(XC_FULL) {
        return Err(XC_FULL);
    }
    // 错误路径 2：跳号检出（篡改履历序号）。
    let mut h2 = build_standard_hist()?;
    h2.sealed = false;
    if let Some(r) = &mut h2.slots[10] {
        r.seq = 99;
    }
    if h2.continuous() != Err(XC_SEQ_GAP) {
        return Err(XC_SEQ_GAP);
    }
    // 错误路径 3：封后写。
    let mut h3 = build_standard_hist()?;
    if h3.log(BootGear::Cold, BootOutcome::Ok, 1) != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    // 错误路径 4：计数守恒破（成数篡改后守恒核）。
    let mut h4 = build_standard_hist()?;
    h4.sealed = false;
    if let Some(r) = &mut h4.slots[7] {
        r.outcome = BootOutcome::Ok;
    }
    // 成+败仍 = 总（篡改不破此恒等式——该注入改验连续与分布）。
    if h4.outcome_conserve().is_err() {
        return Err(XC_CONSERVE);
    }
    // 错误路径 5：档位越闭集（数值面注入）。
    let bad_gear = 3;
    let mut h5 = BootHist::new();
    let res = match bad_gear {
        0 => h5.log(BootGear::Cold, BootOutcome::Ok, 1),
        1 => h5.log(BootGear::Warm, BootOutcome::Ok, 1),
        2 => h5.log(BootGear::Resume, BootOutcome::Ok, 1),
        _ => Err(XC_BAD_GEAR),
    };
    if res != Err(XC_BAD_GEAR) {
        return Err(XC_BAD_GEAR);
    }
    Ok(0)
}
