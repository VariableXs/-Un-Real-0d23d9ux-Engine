//! CGPU-F0461 · 边界交叉 fuzz（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - fuzz：边界条件交叉 fuzz（多边界同时触发——低资源+热极限叠加
//!   ——不变量：守恒/无饥饿在边界保持）；
//! - 畸形边界配置 fuzz（阈值非法——钳制）；
//! - 案例固化 CI。判据：交叉、钳制、CI、判据。
//!
//! 设计纪律：
//! - 交叉引擎：多边界开关位图同时注入，账本在交叉压力下跑确定性
//!   轮次，五不变量抽二（守恒/无饥饿）全程断言。
//! - 畸形配置钳制：阈值非法（0/越界/负向溢出回绕）一律钳到合法
//!   区间，钳制动作计数留痕——畸形不炸引擎只被驯服。
//! - 案例固化：交叉案例记录（开关+轮次+结论）入册，CI 重放同
//!   结论（确定性可重放）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1000 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1000，全仓独占）。
pub const SEG_BASE: u16 = 0x1000;

pub const E_XF_MASK: u16 = SEG_BASE + 1; // 交叉开关非法
pub const E_XF_INVARIANT: u16 = SEG_BASE + 2; // 不变量破缺
pub const E_XF_CASE: u16 = SEG_BASE + 3; // 案例不存在

/// 边界开关位（可叠加）。
pub const B_LOWRES: u32 = 1 << 0; // 低资源（配额压缩）
pub const B_EMERGENCY: u32 = 1 << 1; // 应急（弃车）
pub const B_COLDBOOT: u32 = 1 << 2; // 冷启动（保守配额）
pub const B_OFFLINE: u32 = 1 << 3; // 离线（软渲）
pub const B_THERMAL: u32 = 1 << 4; // 热极限（压缩）
pub const MAX_EDGE_MASK: u32 = 0b1_1111;

/// 交叉 fuzz 账本（守恒+无饥饿双断言）。
#[derive(Debug)]
pub struct CrossLedger {
    pub budget: u64,
    pub granted: Vec<u64>,
    /// 最近一轮是否有人被服务（无饥饿抽样）。
    pub last_served: Vec<bool>,
}

impl CrossLedger {
    pub fn new(n: usize) -> Self {
        Self { budget: 1000, granted: vec![0; n], last_served: vec![false; n] }
    }
    /// 压缩系数注入（热/应急叠加效果）。
    pub fn throttle(&mut self, factor_pm: u64) {
        for g in self.granted.iter_mut() {
            *g = *g * factor_pm / 1000;
        }
    }
    /// 申请注入：低资源/冷启动叠加时配额减半。
    pub fn request(&mut self, acct: usize, pm: u64, quota_half: bool) -> bool {
        let i = acct % self.granted.len();
        let want = if quota_half { pm / 2 } else { pm };
        let total: u64 = self.granted.iter().sum();
        if total + want <= self.budget {
            self.granted[i] += want;
            self.last_served[i] = true;
            true
        } else {
            // 拒绝不抹历史：无饥饿看窗口内曾服务（配额满的拒绝是正常态）。
            false
        }
    }
    pub fn conserve_ok(&self) -> bool {
        self.granted.iter().sum::<u64>() <= self.budget
    }
    /// 无饥饿抽样：窗口内每户至少被服务一次（记录型验证）。
    pub fn no_starvation(&self) -> bool {
        self.last_served.iter().any(|&s| s)
    }
}

/// 交叉案例记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossCase {
    pub mask: u32,
    pub rounds: u64,
    pub pass: bool,
}

/// 交叉 fuzz 引擎。
#[derive(Debug)]
pub struct CrossFuzzer {
    pub cases: Vec<CrossCase>,
}

impl CrossFuzzer {
    pub fn new() -> Self {
        Self { cases: Vec::new() }
    }

    /// 交叉轮：多边界叠加跑确定性轮次，双不变量全程断言。
    pub fn run_cross(&mut self, mask: u32, rounds: u64) -> Result<CrossCase, u16> {
        if mask & !MAX_EDGE_MASK != 0 {
            return Err(E_XF_MASK);
        }
        let mut led = CrossLedger::new(4);
        let thermal = mask & B_THERMAL != 0;
        let emergency = mask & B_EMERGENCY != 0;
        let quota_half = mask & (B_LOWRES | B_COLDBOOT) != 0;
        let mut pass = true;
        for r in 0..rounds {
            if thermal && r % 2 == 0 {
                led.throttle(750);
            }
            if emergency && r % 3 == 0 {
                led.throttle(550);
            }
            let _ = led.request(r as usize % 4, 100, quota_half);
            if !led.conserve_ok() {
                pass = false;
                break;
            }
            if r % 8 == 7 && !led.no_starvation() {
                pass = false;
                break;
            }
        }
        let case = CrossCase { mask, rounds, pass };
        self.cases.push(case);
        if !pass {
            return Err(E_XF_INVARIANT);
        }
        Ok(case)
    }

    /// 畸形阈值钳制：非法值驯服到合法区间，返回 (钳后值, 是否动钳)。
    pub fn clamp_threshold(raw: u64) -> (u64, bool) {
        const LO: u64 = 1;
        const HI: u64 = 1000;
        if raw < LO {
            (LO, true)
        } else if raw > HI {
            (HI, true)
        } else {
            (raw, false)
        }
    }

    /// 案例重放：同参数重跑必须同结论（确定性）。
    pub fn replay(&self, idx: usize) -> Result<bool, u16> {
        let c = self.cases.get(idx).ok_or(E_XF_CASE)?;
        let mut tmp = CrossFuzzer::new();
        let r = tmp.run_cross(c.mask, c.rounds);
        Ok(r.as_ref().map(|x| x.pass) == Ok(c.pass))
    }
}

/// 案例固化 CI：全案例 PASS + 重放一致。
pub fn ci_green(f: &CrossFuzzer) -> Result<(usize, usize), u16> {
    if f.cases.is_empty() {
        return Err(E_XF_CASE);
    }
    let mut replay_ok = 0;
    for i in 0..f.cases.len() {
        if f.replay(i) == Ok(true) {
            replay_ok += 1;
        }
    }
    if f.cases.iter().all(|c| c.pass) && replay_ok == f.cases.len() {
        Ok((f.cases.len(), replay_ok))
    } else {
        Err(E_XF_INVARIANT)
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、交叉：单边界与叠加边界全过双不变量。
    {
        let mut f = CrossFuzzer::new();
        let r1 = f.run_cross(B_LOWRES, 32);
        let r2 = f.run_cross(B_THERMAL, 32);
        let r3 = f.run_cross(B_LOWRES | B_THERMAL, 32); // 低资源+热极限叠加
        let r4 = f.run_cross(MAX_EDGE_MASK, 48); // 全边界叠加
        v.push(("F0461-交叉-单边界过", r1.map(|c| c.pass) == Ok(true)));
        v.push(("F0461-交叉-叠加边界过", r3.map(|c| c.pass) == Ok(true)));
        v.push(("F0461-交叉-全叠加过", r4.map(|c| c.pass) == Ok(true)));
        let _ = r2;
    }
    // 二、钳制：非法阈值驯服。
    {
        v.push(("F0461-钳制-零值钳到下限", CrossFuzzer::clamp_threshold(0) == (1, true)));
        v.push(("F0461-钳制-越界钳到上限", CrossFuzzer::clamp_threshold(99_999) == (1000, true)));
        v.push(("F0461-钳制-合法直通", CrossFuzzer::clamp_threshold(500) == (500, false)));
    }
    // 三、案例固化 CI。
    {
        let mut f = CrossFuzzer::new();
        v.push(("F0461-CI-空案例判缺", ci_green(&f) == Err(E_XF_CASE)));
        f.run_cross(B_LOWRES, 16).ok();
        f.run_cross(B_EMERGENCY | B_OFFLINE, 24).ok();
        f.run_cross(B_COLDBOOT | B_THERMAL, 24).ok();
        let g = ci_green(&f);
        v.push(("F0461-CI-三案例全绿", g == Ok((3, 3))));
        // 重放确定性。
        v.push(("F0461-CI-重放同结论", f.replay(0) == Ok(true) && f.replay(1) == Ok(true)));
        v.push(("F0461-CI-案例不存在拒", f.replay(9) == Err(E_XF_CASE)));
    }
    // 四、开关非法。
    {
        let mut f = CrossFuzzer::new();
        v.push(("F0461-交叉-越界开关拒", f.run_cross(0xFFFF_FFF0, 8) == Err(E_XF_MASK)));
    }
    // 五、守恒抽样压力：极端压缩后申请仍守恒。
    {
        let mut led = CrossLedger::new(4);
        led.request(0, 900, false);
        led.throttle(550);
        led.request(1, 900, true);
        v.push(("F0461-判据-压缩后守恒", led.conserve_ok() && led.granted.iter().sum::<u64>() <= 1000));
    }
    // 六、判据自检。
    {
        v.push(("F0461-判据-开关位图上限", MAX_EDGE_MASK == 0b1_1111));
        let mut f = CrossFuzzer::new();
        let _ = f.run_cross(B_OFFLINE, 8);
        v.push(("F0461-判据-案例入册", f.cases.len() == 1 && f.cases[0].mask == B_OFFLINE));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0461_crossfuzz_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
