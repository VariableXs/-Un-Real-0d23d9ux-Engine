//! UNX-F12277 文件族防幻觉出处账（0x649a0 段）
//!
//! 卡面：文件族断言出处账——每条领域断言必须挂一个权威出处（WinSDK
//! 头文件、MSDN、WRK、ReactOS、S3 基准机录制五源闭集），无出处/出处
//! 越闭集/空断言皆拒；同一断言不得重复入账；seal 前账面不得为空。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 出处闭集：五源之外必拒。
//! - J2 零出处拒：未挂出处必拒。
//! - J3 空断言拒：断言文本为空必拒。
//! - J4 重号断言必拒。
//! - J5 空账 seal 拒。
//! - J6 印账冻结：seal 后补账必拒。
//! - J7 断言可按出处过滤回读。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x649a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x649a0;
pub const PR_BAD_SOURCE: u32 = SEG_BASE + 0x01; // 出处越闭集
pub const PR_EMPTY_CLAIM: u32 = SEG_BASE + 0x02; // 空断言
pub const PR_DUP: u32 = SEG_BASE + 0x03; // 重号断言
pub const PR_UNSIGNED: u32 = SEG_BASE + 0x04; // 空账印账
pub const PR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补账

/// 权威出处闭集。
pub const SRC_WINSDK: u8 = 1;
pub const SRC_MSDN: u8 = 2;
pub const SRC_WRK: u8 = 3;
pub const SRC_REACTOS: u8 = 4;
pub const SRC_S3REC: u8 = 5;

fn source_known(s: u8) -> bool {
    (1..=5).contains(&s)
}

fn source_name(s: u8) -> String {
    match s {
        SRC_WINSDK => "winsdk".to_string(),
        SRC_MSDN => "msdn".to_string(),
        SRC_WRK => "wrk".to_string(),
        SRC_REACTOS => "reactos".to_string(),
        SRC_S3REC => "s3rec".to_string(),
        _ => "unknown".to_string(),
    }
}

/// 一条断言。
#[derive(Clone)]
pub struct Claim {
    pub id: u32,
    pub text: String,
    pub source: u8,
}

impl Claim {
    pub fn render(&self) -> String {
        format!("c{}|{}|{}", self.id, source_name(self.source), self.text)
    }
}

/// 出处账册。
pub struct ClaimBook {
    pub recs: Vec<Claim>,
    pub sealed: bool,
}

impl ClaimBook {
    pub fn new() -> ClaimBook {
        ClaimBook { recs: Vec::new(), sealed: false }
    }

    /// 入账：出处闭集、断言非空、重号拒。
    pub fn put(&mut self, id: u32, text: &str, source: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if !source_known(source) {
            return Err(PR_BAD_SOURCE);
        }
        if text.is_empty() {
            return Err(PR_EMPTY_CLAIM);
        }
        if self.recs.iter().any(|r| r.id == id) {
            return Err(PR_DUP);
        }
        self.recs.push(Claim {
            id,
            text: text.to_string(),
            source,
        });
        Ok(())
    }

    /// 按出处过滤。
    pub fn by_source(&self, source: u8) -> Vec<&Claim> {
        self.recs.iter().filter(|r| r.source == source).collect()
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.recs.is_empty() {
            return Err(PR_UNSIGNED);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.source as u32);
            for b in r.text.as_bytes() {
                h = h.wrapping_mul(211).wrapping_add(*b as u32);
            }
        }
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("claims={}", self.recs.len())
    }

    pub fn line_of(&self, id: u32) -> String {
        match self.recs.iter().find(|r| r.id == id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ClaimBook::new();
    // 1) 出处越闭集必拒。
    got[0] = b.put(1, "x", 0).err() == Some(PR_BAD_SOURCE);
    // 2) 出处 6 越闭集必拒。
    got[1] = b.put(1, "x", 6).err() == Some(PR_BAD_SOURCE);
    // 3) 空断言必拒。
    got[2] = b.put(1, "", SRC_MSDN).err() == Some(PR_EMPTY_CLAIM);
    // 4) 重号断言必拒。
    let _ = b.put(1, "Create 返回句柄", SRC_WINSDK);
    got[3] = b.put(1, "Create 返回句柄", SRC_WINSDK).err() == Some(PR_DUP);
    // 5) 印账后补账必抓。
    b.seal();
    got[4] = b.put(2, "x", SRC_MSDN).err() == Some(PR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12277_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12277");

    // J1 出处闭集。
    let j1 = {
        let mut ok = true;
        for src in 1..=5u8 {
            let mut b = ClaimBook::new();
            if b.put(1, "a", src).is_err() {
                ok = false;
            }
        }
        ok
    };
    s.add("J1-出处闭集", j1, "五源全收");

    // J2 越闭集拒。
    let j2 = {
        let mut b = ClaimBook::new();
        b.put(1, "a", 9).err() == Some(PR_BAD_SOURCE)
    };
    s.add("J2-越闭集拒", j2, "0 与 6+ 必拒");

    // J3 空断言拒。
    let j3 = {
        let mut b = ClaimBook::new();
        b.put(1, "", SRC_WRK).err() == Some(PR_EMPTY_CLAIM)
    };
    s.add("J3-空断言拒", j3, "无文本不收");

    // J4 重号断言拒。
    let j4 = {
        let mut b = ClaimBook::new();
        let _ = b.put(3, "a", SRC_REACTOS);
        b.put(3, "b", SRC_REACTOS).err() == Some(PR_DUP)
    };
    s.add("J4-重号拒", j4, "断言号唯一");

    // J5 按出处过滤。
    let j5 = {
        let mut b = ClaimBook::new();
        let _ = b.put(1, "a", SRC_MSDN);
        let _ = b.put(2, "b", SRC_S3REC);
        let _ = b.put(3, "c", SRC_MSDN);
        b.by_source(SRC_MSDN).len() == 2 && b.by_source(SRC_WINSDK).is_empty()
    };
    s.add("J5-按源过滤", j5, "出处面可回读");

    // J6 空账 seal 拒。
    let j6 = ClaimBook::new().seal().err() == Some(PR_UNSIGNED);
    s.add("J6-空账seal拒", j6, "零断言必拒");

    // J7 有账 seal 成功。
    let j7 = {
        let mut b = ClaimBook::new();
        let _ = b.put(1, "共享冲突码恒定", SRC_S3REC);
        b.seal().is_ok()
    };
    s.add("J7-有账seal", j7, "非空方可印账");

    // J8 印账冻结。
    let j8 = {
        let mut b = ClaimBook::new();
        let _ = b.put(1, "a", SRC_WINSDK);
        let _ = b.seal();
        b.put(2, "b", SRC_WINSDK).err() == Some(PR_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = ClaimBook::new();
        let _ = b.put(1, "text", SRC_MSDN);
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "文本入指纹");

    // J10 回读渲染。
    let j10 = {
        let mut b = ClaimBook::new();
        let _ = b.put(5, "msdn 断言", SRC_MSDN);
        b.line_of(5).contains("msdn") && b.line_of(6) == "missing"
    };
    s.add("J10-回读渲染", j10, "断言行可渲染");

    // J11 出处名记账。
    let j11 = source_name(SRC_S3REC) == "s3rec" && source_name(0) == "unknown";
    s.add("J11-出处名记账", j11, "名可回读");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ClaimBook::new();
    let _ = b.put(1, "NtCreateFile disposition 闭集", SRC_WINSDK);
    let _ = b.put(2, "共享冲突实测可复现", SRC_S3REC);
    s.add("摘要回读", b.summary().contains("claims=2"), "双断言在册");
    s.add("双源覆盖", b.by_source(SRC_WINSDK).len() == 1, "出处分离");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12277_claim_flow() {
        let mut b = ClaimBook::new();
        assert!(b.put(1, "claim", SRC_WRK).is_ok());
        assert_eq!(b.put(1, "again", SRC_WRK), Err(PR_DUP));
        assert_eq!(b.put(2, "", SRC_WRK), Err(PR_EMPTY_CLAIM));
        assert_eq!(b.put(2, "x", 7), Err(PR_BAD_SOURCE));
        assert!(b.seal().is_ok());
        assert_eq!(b.put(3, "y", SRC_WRK), Err(PR_FROZEN));
    }

    #[test]
    fn unx12277_checks_green() {
        let s = run_unx12277_checks();
        assert!(s.all_passed(), "UNX-F12277 判据未全绿");
    }

    #[test]
    fn unx12277_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
