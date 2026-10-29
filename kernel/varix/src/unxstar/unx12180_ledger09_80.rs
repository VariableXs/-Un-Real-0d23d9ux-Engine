//! UNX-F12180 — B09 批域内集成账（0x1a8a0 段）
//!
//! 锚点：UNX-F12180-J1 B09 集成账 10/10（采集→校验→恢复三段闭环
//! 全链模拟：上下文采集过校验器后入恢复账、校验不过不入账），域
//! 账收官。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系·收官集成）。
//! 结构：CpChain（采集→校验→恢复三段闭环账）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a8a0;
pub const IC_GHOST: u32 = SEG_BASE + 0x01; // 恢复未入账上下文
pub const IC_ORDER: u32 = SEG_BASE + 0x02; // 校验不过即恢复
pub const IC_TWICE: u32 = SEG_BASE + 0x03; // 重复闭环
pub const IC_SEALED: u32 = SEG_BASE + 0x04; // 印后记账
pub const IC_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕

/// 闭环记录。
#[derive(Clone, PartialEq)]
pub struct CpChain {
    pub rip: u64,
    pub rsp: u64,
    pub flags: u32,
    pub passed: bool,
    pub restored: bool,
    pub note: String,
}

impl CpChain {
    /// 闭环合法：未过校验不得恢复。
    pub fn ok(&self) -> bool {
        if !self.passed && self.restored {
            return false;
        }
        self.rip != 0 && self.rsp % 8 == 0 && self.flags != 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!(
            "rip={:#x} f={:#x} {}",
            self.rip,
            self.flags,
            if self.restored { "R" } else if self.passed { "P" } else { "X" }
        )
    }
}

/// 闭环账。
pub struct ChainBook {
    pub chains: Vec<CpChain>,
    next_seq: u32,
    sealed: bool,
}

impl ChainBook {
    pub fn new() -> ChainBook {
        ChainBook { chains: Vec::new(), next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一次闭环（采集锚非法/空留痕/重复闭环必拒）。
    pub fn record(
        &mut self,
        rip: u64,
        rsp: u64,
        flags: u32,
        passed: bool,
        restore: bool,
        note: &str,
    ) -> Result<u32, u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        if note.is_empty() {
            return Err(IC_EMPTY);
        }
        if flags == 0 {
            return Err(IC_ORDER);
        }
        let c = CpChain { rip, rsp, flags, passed, restored: restore, note: note.to_string() };
        if !c.ok() {
            return Err(if !passed && restore { IC_ORDER } else { IC_ORDER });
        }
        if self.chains.iter().any(|x| x.rip == rip && x.rsp == rsp) {
            return Err(IC_TWICE);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.chains.push(c);
        Ok(seq)
    }

    /// 已恢复子集。
    pub fn restored_set(&self) -> Vec<&CpChain> {
        self.chains.iter().filter(|c| c.restored).collect()
    }

    /// 已拒绝子集（未过校验）。
    pub fn rejected_set(&self) -> Vec<&CpChain> {
        self.chains.iter().filter(|c| !c.passed).collect()
    }

    /// 全链自洽。
    pub fn all_ok(&self) -> bool {
        self.chains.iter().all(|c| c.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a8a0;
        for c in &self.chains {
            h = h.wrapping_mul(211).wrapping_add(c.rip as u32);
            h = h.wrapping_mul(211).wrapping_add(c.flags);
            h = h.wrapping_mul(211).wrapping_add(c.restored as u32);
            for by in c.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "chains={} rest={} rej={} ok={}",
            self.chains.len(),
            self.restored_set().len(),
            self.rejected_set().len(),
            self.all_ok()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ChainBook::new();
    // 1) 校验不过即恢复必拒。
    got[0] = b.record(0x1000, 0x2000, 0x1001, false, true, "bad").err() == Some(IC_ORDER);
    // 2) 零 flags 必拒。
    got[1] = b.record(0x1000, 0x2000, 0, true, false, "z").err() == Some(IC_ORDER);
    // 3) 非对齐 RSP 必拒。
    got[2] = b.record(0x1000, 0x2004, 0x1001, true, false, "m").err() == Some(IC_ORDER);
    // 4) 空留痕必拒。
    got[3] = b.record(0x1000, 0x2000, 0x1001, true, false, "").err() == Some(IC_EMPTY);
    // 5) 印后记账必拒。
    b.seal();
    got[4] = b.record(0x1000, 0x2000, 0x1001, true, false, "post").err() == Some(IC_SEALED);
    got
}

/// 判据入口：UNX-F12180
pub fn run_unx12180_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12180");
    let mut b = ChainBook::new();

    // J1 十点闭环（8 过+恢复、2 过未恢复）。
    let mut ok1 = true;
    for i in 0..10u64 {
        let rip = 0x1000 + i * 0x40;
        let rsp = 0x8000 + i * 0x20;
        if b.record(rip, rsp, 0x100F, true, i < 8, "cp").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_chain10", ok1 && b.chains.len() == 10, "十点闭环全过");

    // J2 恢复子集 8。
    s.add(
        "J2_restored8",
        b.restored_set().len() == 8,
        "恢复子集 8",
    );

    // J3 拒绝子集 0。
    s.add(
        "J3_rejected0",
        b.rejected_set().is_empty(),
        "拒绝子集空",
    );

    // J4 校验不过即恢复拒。
    let mut b4 = ChainBook::new();
    s.add(
        "J4_fail_restore",
        b4.record(0x1000, 0x2000, 0x1001, false, true, "bad").err() == Some(IC_ORDER),
        "未过校验恢复拒 ORDER",
    );

    // J5 校验不过不恢复合法。
    let r5 = b4.record(0x1000, 0x2000, 0x1001, false, false, "rej");
    s.add(
        "J5_fail_record",
        r5.is_ok() && b4.rejected_set().len() == 1,
        "未过校验仅记账合法",
    );

    // J6 闭环渲染。
    let r = b.chains[0].render();
    s.add(
        "J6_render",
        r.contains("rip=0x1000") && r.ends_with("R"),
        "渲染含 rip/flags/态",
    );

    // J7 闭环自洽核。
    s.add(
        "J7_all_ok",
        b.all_ok(),
        "全链过自洽核",
    );

    // J8 重复闭环拒。
    s.add(
        "J8_twice",
        b.record(0x1000, 0x8000, 0x100F, true, false, "dup").err() == Some(IC_TWICE),
        "同锚重复闭环拒 TWICE",
    );

    // J9 零 flags 拒。
    s.add(
        "J9_zero_flags",
        b.record(0x2000, 0x8100, 0, true, false, "z").err() == Some(IC_ORDER),
        "零 flags 拒 ORDER",
    );

    // J10 非对齐 RSP 拒。
    s.add(
        "J10_misalign",
        b.record(0x2000, 0x8104, 0x1001, true, false, "m").err() == Some(IC_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "闭环账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.record(0x3000, 0x9000, 0x1001, true, false, "p").err() == Some(IC_SEALED),
        "印账后记账拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("chains=10") && sm.contains("rest=8") && sm.contains("ok=true"),
        "摘要含闭环/恢复/自洽位",
    );

    // J15 空留痕拒。
    let mut b15 = ChainBook::new();
    s.add(
        "J15_empty_note",
        b15.record(0x1000, 0x2000, 0x1001, true, false, "").err() == Some(IC_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J16 混合闭环（过/拒/恢复三分）。
    let mut b16 = ChainBook::new();
    let ok16 = b16.record(0x1000, 0x2000, 0x1001, true, true, "a").is_ok()
        && b16.record(0x1010, 0x2010, 0x1001, true, false, "b").is_ok()
        && b16.record(0x1020, 0x2020, 0x1001, false, false, "c").is_ok();
    s.add(
        "J16_mixed",
        ok16 && b16.restored_set().len() == 1 && b16.rejected_set().len() == 1,
        "三分闭环 R/P/X 各一",
    );

    // J17 渲染三态。
    let r17: Vec<String> = b16.chains.iter().map(|c| c.render()).collect();
    s.add(
        "J17_render3",
        r17[0].ends_with('R') && r17[1].ends_with('P') && r17[2].ends_with('X'),
        "三态渲染 R/P/X",
    );

    // J18 空账指纹稳定。
    let b18 = ChainBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空闭环账指纹稳定");

    // J19 序号连续。
    let n19 = b.chains.len() as u32;
    s.add(
        "J19_seq",
        b.next_seq == n19 + 1,
        "next_seq = 链长 + 1",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12180_chain() {
        let mut b = ChainBook::new();
        assert!(b.record(0x1000, 0x2000, 0x1001, true, true, "a").is_ok());
        assert_eq!(b.record(0x1000, 0x2000, 0x1001, false, true, "bad"), Err(IC_ORDER));
    }

    #[test]
    fn unx12180_checks_green() {
        let s = run_unx12180_checks();
        assert!(s.all_passed(), "UNX-F12180 判据未全绿");
    }

    #[test]
    fn unx12180_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
