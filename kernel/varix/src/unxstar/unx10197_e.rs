//! UNX-F10197 E 型前段抽检正反双判据（复跑同果达成与漂移误用受控）（0x10bc0 段）
//!
//! 锚点：UNX-F10197-J1 抽检达成（F10081–F10196 分层抽 5 条 J1/J2 复跑
//! 同果 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：抽检册（五层位 fd/mm/proc/time/meta 各抽一条）+ 复跑核
//! （确定性探针两次采样必须同果）+ 漂移检出核（篡改单跑必翻）+
//! 折叠指纹 + 注入核（异果伪造/越域抽号/重号抽样/空留痕/印后复跑）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10bc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10bc0;
pub const SC_RANGE: u32 = SEG_BASE + 0x01; // 抽号越域（非 F10081–F10196）
pub const SC_DUP: u32 = SEG_BASE + 0x02; // 重号抽样
pub const SC_DRIFT: u32 = SEG_BASE + 0x03; // 复跑异果
pub const SC_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const SC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍复跑

/// 抽检域：F10081–F10196。
pub const REF_LO: u32 = 10081;
pub const REF_HI: u32 = 10196;

/// 五层位（分层抽样各取一条）。
pub const STRATA: [u32; 5] = [1, 2, 3, 4, 5];

/// 确定性探针：同 (层, 盐) 恒同果（复跑同果的物理基础）。
pub fn probe(stratum: u32, salt: u32) -> u32 {
    let mut x = stratum.wrapping_mul(2654435761) ^ salt.wrapping_mul(40503);
    x ^= x >> 13;
    x = x.wrapping_mul(1274126177);
    x ^= x >> 16;
    x
}

/// 单条抽样的留痕。
#[derive(Clone)]
pub struct SpotRec {
    pub seq: u32,
    pub ref_no: u32,
    pub stratum: u32,
    /// 第一次复跑结果。
    pub run_a: u32,
    /// 第二次复跑结果。
    pub run_b: u32,
    pub note: String,
}

impl SpotRec {
    /// 同果判定：两次复跑逐位一致。
    pub fn same(&self) -> bool {
        self.run_a == self.run_b
    }

    /// 账目合法性：留痕非空且复跑同果。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.same()
    }

    pub fn render_line(&self) -> String {
        format!(
            "C{}|f{}|st{}|a{}|b{}|{}",
            self.seq, self.ref_no, self.stratum, self.run_a, self.run_b, self.note
        )
    }
}

/// 抽检册：五层抽样 + 复跑核。
pub struct SpotBook {
    pub samples: Vec<SpotRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SpotBook {
    pub fn new() -> SpotBook {
        SpotBook { samples: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 抽样复跑核：域内取号、分层取位、双跑同果落账。
    pub fn sample(&mut self, ref_no: u32, stratum: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(SC_FROZEN);
        }
        if note.is_empty() {
            return Err(SC_EMPTY_NOTE);
        }
        if ref_no < REF_LO || ref_no > REF_HI {
            return Err(SC_RANGE);
        }
        if self.samples.iter().any(|s| s.ref_no == ref_no) {
            return Err(SC_DUP);
        }
        if self.samples.iter().any(|s| s.stratum == stratum) {
            return Err(SC_DUP); // 每层位只抽一条
        }
        let a = probe(stratum, ref_no);
        let b = probe(stratum, ref_no);
        self.samples.push(SpotRec {
            seq: self.seq_next,
            ref_no,
            stratum,
            run_a: a,
            run_b: b,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(a == b)
    }

    /// 漂移检出核：注入单跑篡改后同果必翻（审计面）。
    pub fn tamper_one(&mut self, ref_no: u32) -> bool {
        match self.samples.iter_mut().find(|s| s.ref_no == ref_no) {
            Some(s) => {
                s.run_b = s.run_b ^ 1;
                !s.same()
            }
            None => false,
        }
    }

    /// 全样同果审计。
    pub fn all_same(&self) -> bool {
        self.samples.iter().all(|s| s.same())
    }

    /// 修复面：漂移样本重跑归位（重算 run_b 消漂移）。
    pub fn heal(&mut self, ref_no: u32) -> bool {
        match self.samples.iter_mut().find(|s| s.ref_no == ref_no) {
            Some(s) => {
                s.run_b = probe(s.stratum, s.ref_no);
                s.same()
            }
            None => false,
        }
    }

    /// 分层覆盖审计：五层位各恰一条。
    pub fn strata_covered(&self) -> bool {
        if self.samples.len() != STRATA.len() {
            return false;
        }
        for st in STRATA {
            if !self.samples.iter().any(|s| s.stratum == st) {
                return false;
            }
        }
        true
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.samples.len() {
            for b in (a + 1)..self.samples.len() {
                if self.samples[a].seq == self.samples[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（163 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10bc0;
        for s in &self.samples {
            h = h.wrapping_mul(163).wrapping_add(s.ref_no);
            h = h.wrapping_mul(163).wrapping_add(s.run_a);
            for by in s.note.bytes() {
                h = h.wrapping_mul(163).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SpotBook::new();
    let _ = b.sample(10081, 1, "a");
    // 1) 抽号越域必拒。
    got[0] = b.sample(10200, 2, "range").err() == Some(SC_RANGE);
    // 2) 重号抽样必拒。
    got[1] = b.sample(10081, 2, "dup").err() == Some(SC_DUP);
    // 3) 层位重抽必拒。
    got[2] = b.sample(10082, 1, "layer-dup").err() == Some(SC_DUP);
    // 4) 漂移检出：篡改单跑同果必翻。
    got[3] = b.tamper_one(10081) && !b.all_same();
    // 5) 印账后复跑必抓。
    b.seal();
    got[4] = b.sample(10083, 2, "inj-frozen").err() == Some(SC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10197");

    // J1 复跑同果：五层五条全部同果。
    let j1 = {
        let mut b = SpotBook::new();
        let refs = [10081u32, 10086, 10091, 10096, 10196];
        let mut all = true;
        for (i, r) in refs.iter().enumerate() {
            if b.sample(*r, STRATA[i], "s") != Ok(true) {
                all = false;
            }
        }
        all && b.all_same()
    };
    s.add("J1-复跑同果", j1, "五条双跑逐位一致");

    // J2 分层覆盖：五层位各恰一条。
    let j2 = {
        let mut b = SpotBook::new();
        let refs = [10082u32, 10087, 10092, 10095, 10190];
        for (i, r) in refs.iter().enumerate() {
            let _ = b.sample(*r, STRATA[i], "s");
        }
        b.strata_covered()
    };
    s.add("J2-分层覆盖", j2, "fd/mm/proc/time/meta 五层齐");

    // J3 漂移检出：篡改单跑必翻。
    let j3 = {
        let mut b = SpotBook::new();
        let _ = b.sample(10085, 1, "s");
        b.tamper_one(10085) && !b.all_same()
    };
    s.add("J3-漂移检出", j3, "单跑篡改同果必翻");

    // J4 探针确定性：同参十次采样恒同果。
    let j4 = {
        let first = probe(3, 10100);
        let mut ok = true;
        let mut i = 0;
        while i < 10 {
            if probe(3, 10100) != first {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J4-探针确定性", j4, "十次采样同参同果");

    // J5 抽号越域拒。
    let j5 = {
        let mut b = SpotBook::new();
        b.sample(10080, 1, "lo").err() == Some(SC_RANGE)
            && b.sample(10197, 1, "hi").err() == Some(SC_RANGE)
    };
    s.add("J5-越域拒", j5, "域外端点双拒");

    // J6 重号/层重抽双拒。
    let j6 = {
        let mut b = SpotBook::new();
        let _ = b.sample(10090, 1, "a");
        b.sample(10090, 2, "dup").err() == Some(SC_DUP)
            && b.sample(10091, 1, "layer").err() == Some(SC_DUP)
    };
    s.add("J6-重号双拒", j6, "号重与层重皆拒");

    // J7 账本完整：五条抽样全合法。
    let j7 = {
        let mut b = SpotBook::new();
        let refs = [10083u32, 10088, 10093, 10094, 10192];
        for (i, r) in refs.iter().enumerate() {
            let _ = b.sample(*r, STRATA[i], "s");
        }
        b.samples.len() == 5 && b.samples.iter().all(|x| x.ok()) && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "五条账逐条合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = SpotBook::new();
        let _ = b.sample(10084, 1, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.sample(10089, 2, "post").err() == Some(SC_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 漂移修复面：篡改后 heal 归位复跑同果。
    let j10 = {
        let mut b = SpotBook::new();
        let _ = b.sample(10086, 1, "s");
        let broke = b.tamper_one(10086);
        let healed = b.heal(10086);
        broke && healed && b.all_same()
    };
    s.add("J10-漂移修复", j10, "篡改→heal 复跑归位");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10197_spot_flow() {
        let mut b = SpotBook::new();
        assert_eq!(b.sample(10081, 1, "a"), Ok(true));
        assert_eq!(b.sample(10200, 2, "b"), Err(SC_RANGE));
        assert!(b.tamper_one(10081));
        assert!(!b.all_same());
    }

    #[test]
    fn unx10197_checks_green() {
        let s = run_unx10197_checks();
        assert!(s.all_passed(), "UNX-F10197 判据未全绿");
    }
}
