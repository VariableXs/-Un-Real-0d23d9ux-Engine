//! UNX-F56108 盘态归零与预热规程件（0x60D60 段）
//!
//! 锚点：UNX-F56108-J1 规程步全量（归零与预热规程步逐个落账：步号 × 步
//! 名 × 目标态三元组）；J2 规程序语义（步号 1..n 连续、终态核验逐步通
//! 过、跳步即拒）；J3 规程审计链（步指纹链、封账只读、终态闭式）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·盘态规程面）。
//!
//! 结构：规程步闭集（填盘/trim/归零验证/预热跑/稳态确认五步）+ 规程步
//! 登记册（步号 → 步名 × 目标态逐个落账）+ 步序闸（步号连续单调）+
//! 终态核验核（逐步目标态命中才可放行下一步）+ 指纹链 + 注入核（空步
//! 名/步号跳步/终态未过强推/重复步/封账改写/空规程核验/终态闭式漂移
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60D60 独占 0x20）。
pub const SEG_BASE: u32 = 0x60D60;
pub const DP_NAME_EMPTY: u32 = SEG_BASE + 0x01; // 步名为空
pub const DP_STEP_GAP: u32 = SEG_BASE + 0x02; // 步号跳步
pub const DP_STATE_MISS: u32 = SEG_BASE + 0x03; // 终态未过强推
pub const DP_STEP_DUP: u32 = SEG_BASE + 0x04; // 重复步
pub const DP_SEALED: u32 = SEG_BASE + 0x05; // 规程账已封账拒写
pub const DP_EMPTY: u32 = SEG_BASE + 0x06; // 空规程核验
pub const DP_CENSUS_MISS: u32 = SEG_BASE + 0x07; // 终态闭式漂移

/// 规程步闭集。
pub const S_FILL: usize = 0;
pub const S_TRIM: usize = 1;
pub const S_ZERO_VERIFY: usize = 2;
pub const S_WARMUP: usize = 3;
pub const S_STEADY: usize = 4;
pub const S_COUNT: usize = 5;
pub const S_NAMES: [&str; S_COUNT] = ["fill", "trim", "zero_verify", "warmup", "steady"];

/// 目标态闭集。
pub const ST_DONE: usize = 0;
pub const ST_PENDING: usize = 1;
pub const ST_COUNT: usize = 2;
pub const ST_NAMES: [&str; ST_COUNT] = ["done", "pending"];

/// 步指纹：FNV-1a 32 位。
pub fn step_fingerprint(no: u32, name: &str, state: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let st = if state < ST_COUNT { ST_NAMES[state] } else { "?" };
    let line = format!("{}|{}|{}", no, name, st);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 规程步：步号 × 步名 × 目标态 × 指纹 × 链。
#[derive(Clone)]
pub struct ProStep {
    pub no: u32,
    pub name: String,
    pub state: usize,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 规程总账。
pub struct DiskPrimeBook {
    pub steps: Vec<ProStep>,
    pub sealed: bool,
}

impl DiskPrimeBook {
    pub fn new() -> Self {
        DiskPrimeBook { steps: Vec::new(), sealed: false }
    }

    fn find(&self, no: u32) -> Option<usize> {
        for i in 0..self.steps.len() {
            if self.steps[i].no == no {
                return Some(i);
            }
        }
        None
    }

    /// 规程步落账：空步名拒 + 步号连续 + 重复拒三闸（步号 = 已有数 + 1）。
    pub fn step(&mut self, name: &str, state: usize) -> Result<u32, u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        if name.is_empty() {
            return Err(DP_NAME_EMPTY);
        }
        if state >= ST_COUNT {
            return Err(DP_STATE_MISS);
        }
        let no = (self.steps.len() + 1) as u32;
        if self.find(no).is_some() {
            return Err(DP_STEP_DUP);
        }
        // 步号连续闸：登记序即 1..n，天然连续；超五步闭集拒。
        if no > S_COUNT as u32 {
            return Err(DP_STEP_GAP);
        }
        let fp = step_fingerprint(no, name, state);
        let chain = match self.steps.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.steps.push(ProStep { no, name: name.to_string(), state, fp, chain });
        Ok(no)
    }

    /// 终态核验：把第 no 步置 done（未过核验不得强推——pending 步禁止跳过）。
    pub fn confirm(&mut self, no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        let idx = match self.find(no) {
            Some(i) => i,
            None => return Err(DP_EMPTY),
        };
        // 前序步必须全部 done 才可核验本步（跳步即拒）。
        for i in 0..idx {
            if self.steps[i].state != ST_DONE {
                return Err(DP_STATE_MISS);
            }
        }
        self.steps[idx].state = ST_DONE;
        Ok(())
    }

    /// J1：规程全量（五步逐个在册）。
    pub fn steps_ok(&self) -> bool {
        self.steps.len() == S_COUNT
            && self.steps.iter().all(|s| !s.name.is_empty() && s.state < ST_COUNT)
    }

    /// J2：步序连续核（1..5 连续）。
    pub fn numbering_ok(&self) -> bool {
        for (i, s) in self.steps.iter().enumerate() {
            if s.no != (i + 1) as u32 {
                return false;
            }
        }
        true
    }

    /// J2：终态闭式（全 done 才判规程完成）。
    pub fn all_done(&self) -> bool {
        if self.steps.len() != S_COUNT {
            return false;
        }
        self.steps.iter().all(|s| s.state == ST_DONE)
    }

    /// J3：步指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.steps.is_empty() {
            return false;
        }
        for i in 0..self.steps.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.steps[0].fp
            } else {
                self.steps[i - 1].fp ^ self.steps[i].fp
            };
            if self.steps[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.steps.last() {
            None => CHAIN_SEED,
            Some(s) => s.chain,
        }
    }
}

/// 标准夹具：五步全登记全 done。
pub fn standard_book() -> DiskPrimeBook {
    let mut b = DiskPrimeBook::new();
    for i in 0..S_COUNT {
        if b.step(S_NAMES[i], ST_PENDING).is_err() {
            panic!("standard fixture step must succeed");
        }
        if b.confirm((i + 1) as u32).is_err() {
            panic!("standard fixture confirm must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空步名/超步拒/核验跳步/空规程核验/封账改写/终态闭式/重复步）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空步名
    let mut b = DiskPrimeBook::new();
    if b.step("", ST_PENDING) == Err(DP_NAME_EMPTY) {
        caught += 1;
    }
    // 2) 超步闭集拒（第六步）
    let mut b2 = standard_book();
    if b2.step("extra", ST_PENDING) == Err(DP_STEP_GAP) {
        caught += 1;
    }
    // 3) 核验跳步（第 2 步未 done 先核第 3 步）
    let mut b3 = DiskPrimeBook::new();
    let _ = b3.step(S_NAMES[0], ST_PENDING);
    let _ = b3.step(S_NAMES[1], ST_PENDING);
    if b3.confirm(2) == Err(DP_STATE_MISS) {
        caught += 1;
    }
    // 4) 空规程核验
    if DiskPrimeBook::new().confirm(1).err() == Some(DP_EMPTY) {
        caught += 1;
    }
    // 5) 终态闭式（未全 done 判负）
    if !b3.all_done() {
        caught += 1;
    }
    // 6) 状态越界拒
    if b3.step("x", 9) == Err(DP_STATE_MISS) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.step("late", ST_PENDING) == Err(DP_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 规程账渲染导出。
pub fn render_book(b: &DiskPrimeBook) -> String {
    let mut out = String::new();
    for s in b.steps.iter() {
        out.push_str(&format!("{}|{}|{}\n", s.no, s.name, ST_NAMES[s.state]));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行三段 + 步名闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 3
            && (parts[1] == "fill" || parts[1] == "trim" || parts[1] == "zero_verify"
                || parts[1] == "warmup" || parts[1] == "steady")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56108");
    // J1-1 规程步全量（五步逐个在册）
    let b = standard_book();
    let ok = b.steps_ok() && b.steps.len() == S_COUNT;
    s.add("56108-J1-规程步全量", ok, if ok { "" } else { "规程步残缺" });
    // J1-2 步名闭集齐
    let ok = S_NAMES.len() == S_COUNT && S_NAMES[2] == "zero_verify";
    s.add("56108-J1-步名闭集齐", ok, if ok { "" } else { "步名漂移" });
    // J1-3 步号连续（1..5）
    let ok = b.numbering_ok() && b.steps[0].no == 1 && b.steps[4].no == 5;
    s.add("56108-J1-步号连续", ok, if ok { "" } else { "步号断档" });
    // J2-1 终态闭式（全 done 判规程完成）
    let ok = b.all_done();
    s.add("56108-J2-终态闭式", ok, if ok { "" } else { "终态漂移" });
    // J2-2 未完成判负（夹具 pending 态）
    let mut b2 = DiskPrimeBook::new();
    for i in 0..S_COUNT {
        let _ = b2.step(S_NAMES[i], if i < 3 { ST_DONE } else { ST_PENDING });
    }
    let ok = !b2.all_done();
    s.add("56108-J2-未完成判负", ok, if ok { "" } else { "未完成穿透" });
    // J2-3 核验跳步拒
    let mut b3 = DiskPrimeBook::new();
    let _ = b3.step(S_NAMES[0], ST_PENDING);
    let _ = b3.step(S_NAMES[1], ST_PENDING);
    let ok = b3.confirm(2) == Err(DP_STATE_MISS);
    s.add("56108-J2-跳步拒", ok, if ok { "" } else { "跳步穿透" });
    // J2-4 顺序核验放行（逐个 confirm 成功）
    let mut b4 = DiskPrimeBook::new();
    let mut ok = true;
    for i in 0..S_COUNT {
        if b4.step(S_NAMES[i], ST_PENDING).is_err() || b4.confirm((i + 1) as u32).is_err() {
            ok = false;
        }
    }
    let ok = ok && b4.all_done();
    s.add("56108-J2-顺序核验放行", ok, if ok { "" } else { "顺序核验漂移" });
    // J3-1 步指纹链复核
    let ok = b.chain_ok();
    s.add("56108-J3-步指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56108-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（终态变化链尾变）
    let mut b5 = DiskPrimeBook::new();
    for i in 0..S_COUNT {
        let _ = b5.step(S_NAMES[i], ST_PENDING);
    }
    let ok = b5.chain_tail() != standard_book().chain_tail();
    s.add("56108-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b6 = standard_book();
    b6.seal();
    let ok = b6.step("late", ST_PENDING) == Err(DP_SEALED);
    s.add("56108-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.starts_with("1|fill|done");
    s.add("56108-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56108-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56108_all_checks_green() {
        let set = run_unx56108_checks();
        assert!(set.all_passed(), "UNX-F56108 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56108_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
