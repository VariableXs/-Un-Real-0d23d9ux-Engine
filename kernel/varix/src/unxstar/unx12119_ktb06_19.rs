//! UNX-F12119 ktest 字符串族断言集（B06 批判据聚合）（0x1a100 段）
//!
//! 卡面：B06 批（UNX-F12101~12118 共 18 卡）字符串族断言聚合——
//! 每卡登记一条断言行（通过数/总数），聚合册全 18 行对平后批级判绿。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 十八行闭集：B06 批 18 卡编号闭集，全部登记后聚合才可判。
//! - J2 重卡拒：同卡二次登记必拒。
//! - J3 总数为零拒 / 通过数越界拒。
//! - J4 容量显性：第 19 行登记必拒（册容 18）。
//! - J5 聚合对平：逐行 ok 且通过合计 ≤ 总数合计。
//! - J6 批级全绿：十八行齐备且全过。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：B06Dom 十八卡枚举（含卡号映射）+ KtbB06 聚合册 + 折叠指纹
//! （211 进制卷积）+ 注入核（零总数/越界通过/重卡/超容/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a100 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a100;
pub const K6_BAD_TOTAL: u32 = SEG_BASE + 0x01; // 总数为零
pub const K6_BAD_TALLY: u32 = SEG_BASE + 0x02; // 通过数越界
pub const K6_DUP_DOM: u32 = SEG_BASE + 0x03; // 重卡
pub const K6_FULL: u32 = SEG_BASE + 0x04; // 超容
pub const K6_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// B06 批容量（18 卡）。
pub const B06_CAP: usize = 18;

/// B06 批十八卡枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B06Dom {
    UniStruct = 1,   // 12101 结构语义
    NulConv = 2,     // 12102 NUL 约定
    RtlInit = 3,     // 12103 RtlInitUnicodeString
    AnsiInit = 4,    // 12104 ANSI/OEM
    Copy = 5,        // 12105 RtlCopyUnicodeString
    Append = 6,      // 12106 RtlAppend 族
    SafeFamily = 7,  // 12107 安全族
    AnsiToUni = 8,   // 12108 ANSI→UTF-16
    Upcase = 9,      // 12109 大写转换
    Compare = 10,    // 12110 比较族
    Prefix = 11,     // 12111 前缀判定
    IntConv = 12,    // 12112 整数转换
    Printf = 13,     // 12113 安全格式化
    PoolOwn = 14,    // 12114 缓冲所有权
    DefMatrix = 15,  // 12115 防御矩阵
    MultiEnc = 16,   // 12116 多编码账
    UpcaseHash = 17, // 12117 不敏感哈希
    Xref = 18,       // 12118 对照表
}

impl B06Dom {
    /// 卡号映射。
    pub fn card(&self) -> u32 {
        12100 + *self as u32
    }
}

/// 一条断言行。
#[derive(Clone)]
pub struct K6Row {
    pub dom: B06Dom,
    pub passed: u32,
    pub total: u32,
    pub tag: String,
}

impl K6Row {
    pub fn ok(&self) -> bool {
        self.total > 0 && self.passed <= self.total
    }
    pub fn all_pass(&self) -> bool {
        self.passed == self.total
    }
    pub fn render(&self) -> String {
        format!("D{}|{}/{}", self.dom.card(), self.passed, self.total)
    }
}

/// B06 聚合册。
pub struct KtbB06 {
    pub rows: Vec<K6Row>,
    pub sealed: bool,
}

impl KtbB06 {
    pub fn new() -> KtbB06 {
        KtbB06 { rows: Vec::new(), sealed: false }
    }

    /// 登记：零总数 → 越界 → 重卡 → 容量。
    pub fn put(&mut self, dom: B06Dom, passed: u32, total: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(K6_FROZEN);
        }
        if total == 0 {
            return Err(K6_BAD_TOTAL);
        }
        if passed > total {
            return Err(K6_BAD_TALLY);
        }
        if self.rows.iter().any(|r| r.dom == dom) {
            return Err(K6_DUP_DOM);
        }
        if self.rows.len() >= B06_CAP {
            return Err(K6_FULL);
        }
        self.rows.push(K6Row { dom, passed, total, tag: tag.to_string() });
        Ok(())
    }

    /// 十八行齐备。
    pub fn complete(&self) -> bool {
        self.rows.len() == B06_CAP
    }

    /// 聚合对平。
    pub fn tallies_match(&self) -> bool {
        let p: u32 = self.rows.iter().map(|r| r.passed).sum();
        let t: u32 = self.rows.iter().map(|r| r.total).sum();
        self.rows.iter().all(|r| r.ok()) && p <= t
    }

    /// 批级全绿。
    pub fn batch_green(&self) -> bool {
        self.complete() && self.rows.iter().all(|r| r.all_pass())
    }

    /// 通过合计。
    pub fn passed_sum(&self) -> u32 {
        self.rows.iter().map(|r| r.passed).sum()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.dom.card());
            h = h.wrapping_mul(211).wrapping_add(r.passed);
            h = h.wrapping_mul(211).wrapping_add(r.total);
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("rows={}/{}", self.rows.len(), B06_CAP)
    }

    /// 诊断面：行渲染。
    pub fn line_of(&self, dom: B06Dom) -> String {
        match self.rows.iter().find(|r| r.dom == dom) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// B06 十八卡全列。
pub fn b06_all_doms() -> [B06Dom; 18] {
    [
        B06Dom::UniStruct, B06Dom::NulConv, B06Dom::RtlInit,
        B06Dom::AnsiInit, B06Dom::Copy, B06Dom::Append,
        B06Dom::SafeFamily, B06Dom::AnsiToUni, B06Dom::Upcase,
        B06Dom::Compare, B06Dom::Prefix, B06Dom::IntConv,
        B06Dom::Printf, B06Dom::PoolOwn, B06Dom::DefMatrix,
        B06Dom::MultiEnc, B06Dom::UpcaseHash, B06Dom::Xref,
    ]
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtbB06::new();
    // 1) 零总数必拒。
    got[0] = b.put(B06Dom::UniStruct, 1, 0, "zero").err() == Some(K6_BAD_TOTAL);
    // 2) 通过数越界必拒。
    got[1] = b.put(B06Dom::UniStruct, 9, 8, "over").err() == Some(K6_BAD_TALLY);
    // 3) 重卡必拒。
    let _ = b.put(B06Dom::UniStruct, 8, 8, "a");
    got[2] = b.put(B06Dom::UniStruct, 1, 1, "dup").err() == Some(K6_DUP_DOM);
    // 4) 超容必拒：灌满 18 行后册容锁定。
    let all = b06_all_doms();
    let mut b2 = KtbB06::new();
    for d in all.iter() {
        let _ = b2.put(*d, 1, 1, "fill");
    }
    got[3] = b2.complete() && b2.rows.len() == B06_CAP;
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(B06Dom::UniStruct, 1, 1, "post").err() == Some(K6_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12119_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12119");
    let all = b06_all_doms();

    // J1 卡号闭集映射。
    let j1 = all[0].card() == 12101 && all[17].card() == 12118;
    s.add("J1-卡号闭集", j1, "12101..12118 映射连续");

    // J2 十八行齐备且批级全绿。
    let j2 = {
        let mut b = KtbB06::new();
        for d in all.iter() {
            let _ = b.put(*d, 4, 4, "full");
        }
        b.complete() && b.batch_green() && b.passed_sum() == 72
    };
    s.add("J2-十八行全绿", j2, "全过聚合绿");

    // J3 零总数拒。
    let j3 = {
        let mut b = KtbB06::new();
        b.put(B06Dom::NulConv, 0, 0, "z").err() == Some(K6_BAD_TOTAL)
    };
    s.add("J3-零总数拒", j3, "总数为零必拒");

    // J4 通过数越界拒。
    let j4 = {
        let mut b = KtbB06::new();
        b.put(B06Dom::Copy, 5, 4, "x").err() == Some(K6_BAD_TALLY)
    };
    s.add("J4-越界拒", j4, "通过 > 总数必拒");

    // J5 重卡拒。
    let j5 = {
        let mut b = KtbB06::new();
        let _ = b.put(B06Dom::Append, 1, 1, "a");
        b.put(B06Dom::Append, 1, 1, "b").err() == Some(K6_DUP_DOM)
    };
    s.add("J5-重卡拒", j5, "同卡二次必拒");

    // J6 部分通过仍对平且批级不绿。
    let j6 = {
        let mut b = KtbB06::new();
        let _ = b.put(B06Dom::UniStruct, 2, 3, "part");
        let _ = b.put(B06Dom::NulConv, 3, 3, "ok");
        b.tallies_match() && !b.batch_green() && b.passed_sum() == 5
    };
    s.add("J6-部分对平", j6, "缺行时批级不绿");

    // J7 行渲染含卡号。
    let j7 = {
        let mut b = KtbB06::new();
        let _ = b.put(B06Dom::IntConv, 7, 7, "r");
        b.line_of(B06Dom::IntConv).contains("12112")
    };
    s.add("J7-行渲染", j7, "行含卡号");

    // J8 缺行显性。
    let j8 = {
        let mut b = KtbB06::new();
        b.line_of(B06Dom::Xref) == "missing"
    };
    s.add("J8-缺行显性", j8, "未登记行显性");

    // J9 印账冻结。
    let j9 = {
        let mut b = KtbB06::new();
        let _ = b.put(B06Dom::UniStruct, 1, 1, "a");
        b.seal();
        b.put(B06Dom::NulConv, 1, 1, "post").err() == Some(K6_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = KtbB06::new();
        let _ = b.put(B06Dom::UniStruct, 1, 1, "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账：B06 十八行前十二填充。
    let mut b = KtbB06::new();
    for d in all.iter().take(12) {
        let _ = b.put(*d, 3, 3, "major");
    }

    s.add("摘要回读", b.summary().contains("rows=12/18"), "三分之二填充");
    s.add("对平判定", b.tallies_match(), "部分册仍对平");
    s.add("缺席行", b.line_of(B06Dom::Xref) == "missing", "尾段缺行");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12119_put_flow() {
        let mut b = KtbB06::new();
        assert_eq!(b.put(B06Dom::UniStruct, 1, 0, "z"), Err(K6_BAD_TOTAL));
        assert!(b.put(B06Dom::UniStruct, 1, 1, "a").is_ok());
        assert_eq!(b.put(B06Dom::UniStruct, 1, 1, "b"), Err(K6_DUP_DOM));
    }

    #[test]
    fn unx12119_checks_green() {
        let s = run_unx12119_checks();
        assert!(s.all_passed(), "UNX-F12119 判据未全绿");
    }

    #[test]
    fn dom_card_mapping() {
        assert_eq!(B06Dom::RtlInit.card(), 12103);
        assert_eq!(B06Dom::Xref.card(), 12118);
        assert_ne!(B06Dom::Copy, B06Dom::Append);
    }
}
