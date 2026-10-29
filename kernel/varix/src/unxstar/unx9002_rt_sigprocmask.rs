//! UNX-F9002 rt_sigprocmask 掩码语义（0x62000 段）
//!
//! 锚点：UNX-F9002-J1 掩码账（三态 how 操作全落账 + oldset 链逐笔回读 +
//! 渲染导出面）；J2 掩码语义（SIG_BLOCK 并 / SIG_UNBLOCK 差 / SIG_SETMASK
//! 直设三态运算 + 信号号 1~64 到位 0~63 的映射 + popcount 位守恒）；J3 封
//! 账印（掩码指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11（syscall 信号面）。
//!
//! 结构：64 位掩码账本 + 三态 how 分派核（非法 how 拒 + 信号号越界拒）+
//! oldset 回读链 + 掩码指纹 + 注入核（非法 how / 信号越界 / 印后改掩码 /
//! 空账封印 / 掩码链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62000 独占 0x20）。
pub const SEG_BASE: u32 = 0x62000;
pub const SM_BAD_HOW: u32 = SEG_BASE + 0x01; // 非法 how（非三态之一）
pub const SM_SIG_OOB: u32 = SEG_BASE + 0x02; // 信号号越界（0 或 >64）
pub const SM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改掩码
pub const SM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const SM_CHAIN: u32 = SEG_BASE + 0x05; // 掩码链断裂（oldset 回读不守恒）

/// POSIX rt_sigprocmask 三态 how。
pub const SIG_BLOCK: u32 = 0; // 并入阻塞集
pub const SIG_UNBLOCK: u32 = 1; // 从阻塞集剔除
pub const SIG_SETMASK: u32 = 2; // 直设阻塞集
pub const NSIG_MAX: u8 = 64; // 信号号上界（1..=64 → 位 0..=63）

/// how 名渲染。
pub fn how_name(how: u32) -> &'static str {
    match how {
        SIG_BLOCK => "BLOCK",
        SIG_UNBLOCK => "UNBLOCK",
        SIG_SETMASK => "SETMASK",
        _ => "BADHOW",
    }
}

/// 信号号列表折叠为掩码位：信号 n → 位 n-1。
pub fn sig_bits(sigs: &[u8]) -> u64 {
    sigs.iter().fold(0u64, |acc, &s| acc | (1u64 << (s - 1)))
}

/// 三态掩码运算核。
pub fn apply_mask(old: u64, how: u32, sigs: &[u8]) -> u64 {
    let bits = sig_bits(sigs);
    match how {
        SIG_BLOCK => old | bits,
        SIG_UNBLOCK => old & !bits,
        SIG_SETMASK => bits,
        _ => old,
    }
}

/// 单笔掩码操作账。
#[derive(Clone)]
pub struct MaskOp {
    pub how: u32,
    pub sigs: Vec<u8>,
    pub old_mask: u64,
    pub new_mask: u64,
}

impl MaskOp {
    fn render_line(&self) -> String {
        let sigs_s = self
            .sigs
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}|{}|old{}|new{}",
            how_name(self.how),
            sigs_s,
            self.old_mask,
            self.new_mask
        )
    }
}

/// rt_sigprocmask 掩码账本。
pub struct SigmaskLedger {
    ops: Vec<MaskOp>,
    mask: u64,
    sealed: bool,
}

impl SigmaskLedger {
    pub fn new() -> Self {
        SigmaskLedger {
            ops: Vec::new(),
            mask: 0,
            sealed: false,
        }
    }

    /// 应用一次 rt_sigprocmask：三态分派 + 越界拒 + 印后拒。
    /// Ok(old) 即 oldset 回读。
    pub fn apply(&mut self, how: u32, sigs: &[u8]) -> Result<u64, u32> {
        if self.sealed {
            return Err(SM_SEALED);
        }
        if how > SIG_SETMASK {
            return Err(SM_BAD_HOW);
        }
        for &s in sigs {
            if s == 0 || s > NSIG_MAX {
                return Err(SM_SIG_OOB);
            }
        }
        let old = self.mask;
        let new = apply_mask(old, how, sigs);
        self.ops.push(MaskOp {
            how,
            sigs: sigs.to_vec(),
            old_mask: old,
            new_mask: new,
        });
        self.mask = new;
        Ok(old)
    }

    /// 掩码链守恒核：首笔 old = 0，逐笔 old = 前笔 new，
    /// 每笔 new 可由三态核重算复现，末笔 new = 当前掩码。
    pub fn chain_ok(&self) -> bool {
        let mut expect: u64 = 0;
        for op in &self.ops {
            if op.old_mask != expect {
                return false;
            }
            if op.new_mask != apply_mask(op.old_mask, op.how, &op.sigs) {
                return false;
            }
            expect = op.new_mask;
        }
        self.mask == expect
    }

    /// 信号 n 当前是否被阻塞。
    pub fn is_blocked(&self, sig: u8) -> bool {
        if sig == 0 || sig > NSIG_MAX {
            return false;
        }
        (self.mask >> (sig - 1)) & 1 == 1
    }

    /// 阻塞位计数（popcount 守恒面）。
    pub fn blocked_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// oldset 逐笔回读序列。
    pub fn oldsets(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.old_mask).collect()
    }

    /// 掩码指纹：掩码 + 各笔操作折叠。
    pub fn sm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.mask as u32);
        h = h.wrapping_mul(0x0100_0193) ^ ((self.mask >> 32) as u32);
        for op in &self.ops {
            h = h.wrapping_mul(0x0100_0193) ^ op.how;
            h = h.wrapping_mul(0x0100_0193) ^ (op.old_mask as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.new_mask as u32);
            h ^= ((op.old_mask >> 32) as u32) ^ ((op.new_mask >> 32) as u32);
            for &s in &op.sigs {
                h = h.wrapping_mul(0x0100_0193) ^ (s as u32);
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印：空账拒 + 重封拒。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.ops.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for op in &self.ops {
            out.push_str(&op.render_line());
            out.push('\n');
        }
        out.push_str(&format!("SIGMASK-MASK|{}\n", self.mask));
        out.push_str(&format!("SIGMASK-POPC|{}\n", self.mask.count_ones()));
        if self.sealed {
            out.push_str("SIGMASK-SEAL\n");
        }
        out
    }
}

impl Default for SigmaskLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三态四连操作（BLOCK[2,5]→BLOCK[10]→UNBLOCK[2]→SETMASK[1,3]）。
/// 手验：0→18→530→528→5。
fn standard_ledger() -> SigmaskLedger {
    let mut l = SigmaskLedger::new();
    let _ = l.apply(SIG_BLOCK, &[2, 5]); // old 0   → new 18
    let _ = l.apply(SIG_BLOCK, &[10]); // old 18  → new 530
    let _ = l.apply(SIG_UNBLOCK, &[2]); // old 530 → new 528
    let _ = l.apply(SIG_SETMASK, &[1, 3]); // old 528 → new 5
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 how（3 非三态之一）
    let mut l = SigmaskLedger::new();
    if l.apply(3, &[1]).err() == Some(SM_BAD_HOW) {
        caught += 1;
    }
    // 注 2：信号号越界（0 与 65 双向）
    let mut l = SigmaskLedger::new();
    if l.apply(SIG_BLOCK, &[0]).err() == Some(SM_SIG_OOB)
        && l.apply(SIG_BLOCK, &[65]).err() == Some(SM_SIG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改掩码
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.apply(SIG_BLOCK, &[7]).err() == Some(SM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SigmaskLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：掩码链断裂（篡改中段 oldset）
    let mut l = standard_ledger();
    l.ops[1].old_mask += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 掩码账；J2 掩码语义；J3 封账印。
pub fn run_unx9002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9002");
    let p = standard_ledger();
    // J1-1 三态四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9002-J1-操作全落账", ok, if ok { "" } else { "掩码账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 18, 530, 528];
    s.add("9002-J1-oldset回读", ok, if ok { "" } else { "oldset 链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("BLOCK|2,5|old0|new18")
        && text.contains("BLOCK|10|old18|new530")
        && text.contains("UNBLOCK|2|old530|new528")
        && text.contains("SETMASK|1,3|old528|new5")
        && text.contains("SIGMASK-MASK|5")
        && text.contains("SIGMASK-POPC|2");
    s.add("9002-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终掩码 = 5（信号 1、3）
    let ok = p.mask == 5;
    s.add("9002-J2-终掩码", ok, if ok { "" } else { "终掩码漂移" });
    // J2-2 SIG_BLOCK 并运算（0 | 18 = 18）
    let ok = p.ops[0].how == SIG_BLOCK
        && p.ops[0].old_mask == 0
        && p.ops[0].new_mask == 18
        && p.ops[0].new_mask == p.ops[0].old_mask | sig_bits(&[2, 5]);
    s.add("9002-J2-BLOCK并运算", ok, if ok { "" } else { "并运算漂移" });
    // J2-3 SIG_UNBLOCK 差运算（530 & !2 = 528）
    let ok = p.ops[2].how == SIG_UNBLOCK
        && p.ops[2].old_mask == 530
        && p.ops[2].new_mask == 528
        && p.ops[2].new_mask == p.ops[2].old_mask & !sig_bits(&[2]);
    s.add("9002-J2-UNBLOCK差运算", ok, if ok { "" } else { "差运算漂移" });
    // J2-4 SIG_SETMASK 直设（528 → 5）
    let ok = p.ops[3].how == SIG_SETMASK
        && p.ops[3].old_mask == 528
        && p.ops[3].new_mask == 5
        && p.ops[3].new_mask == sig_bits(&[1, 3]);
    s.add("9002-J2-SETMASK直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 信号号→位映射（n → 位 n-1）
    let ok = p.is_blocked(1) && p.is_blocked(3) && !p.is_blocked(2) && !p.is_blocked(10);
    s.add("9002-J2-信号位映射", ok, if ok { "" } else { "位映射漂移" });
    // J2-6 popcount 位守恒
    let ok = p.blocked_count() == 2 && p.chain_ok();
    s.add("9002-J2-popcount守恒", ok, if ok { "" } else { "位守恒破坏" });
    // J3-1 掩码指纹确定
    let ok = standard_ledger().sm_fp() == standard_ledger().sm_fp();
    s.add("9002-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ledger();
    let fp0 = p2.sm_fp();
    p2.ops[0].new_mask += 1;
    let ok = p2.sm_fp() != fp0;
    s.add("9002-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ledger();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("SIGMASK-SEAL");
    s.add("9002-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(SIG_SETMASK, &[0]).err() == Some(SM_SEALED);
    s.add("9002-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9002-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9002_all_checks_green() {
        let set = run_unx9002_checks();
        assert!(set.all_passed(), "UNX-F9002 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9002_mask_stable() {
        let l = standard_ledger();
        assert!(l.chain_ok());
        assert_eq!(l.mask, 5);
        assert_eq!(l.blocked_count(), 2);
    }
}
