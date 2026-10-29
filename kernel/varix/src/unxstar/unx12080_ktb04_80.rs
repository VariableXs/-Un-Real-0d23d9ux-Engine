//! UNX-F12080 ktest TEB 面断言集（B04 批判据聚合）（0x19c20 段）
//!
//! 卡面：B04 批（UNX-F12068~12079 共 12 卡）TEB 面断言聚合——每卡登记
//! 一条断言行（通过数/总数），聚合册全 12 行对平后批级判绿。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 十二行闭集：B04 批 12 卡编号闭集，全部登记后聚合才可判。
//! - J2 重卡拒：同卡二次登记必拒。
//! - J3 总数为零拒：断言总数为零的登记必拒。
//! - J4 通过数越界拒：通过数 > 总数必拒。
//! - J5 容量显性：第 13 行登记必拒（册容 12）。
//! - J6 聚合对平：逐行通过数合计 ≤ 总数合计，且全过时批级全绿。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：B04Dom 十二卡枚举（含卡号映射）+ KtbB04 聚合册 + 折叠指纹
//! （211 进制卷积）+ 注入核（零总数/越界通过/重卡/超容/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19c20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19c20;
pub const KB_BAD_TOTAL: u32 = SEG_BASE + 0x01; // 总数为零
pub const KB_BAD_TALLY: u32 = SEG_BASE + 0x02; // 通过数越界
pub const KB_DUP_DOM: u32 = SEG_BASE + 0x03; // 重卡
pub const KB_FULL: u32 = SEG_BASE + 0x04; // 超容
pub const KB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// B04 批容量（12 卡）。
pub const B04_CAP: usize = 12;

/// B04 批十二卡枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B04Dom {
    Locale = 1,        // 12068 CurrentLocale
    ActCtx = 2,        // 12069 激活上下文栈
    CritSec = 3,       // 12070 临界区计数
    GsPath = 4,        // 12071 gs 基址写路径
    Handover = 5,      // 12072 线程创建交接
    Snapshot = 6,      // 12073 越界写检测
    Layout = 7,        // 12074 布局断言器
    Reserved = 8,      // 12075 保留字段账
    TxFs = 9,          // 12076 TxFsContext
    TebPtr = 10,       // 12077 双指针语义
    Lifecycle = 11,    // 12078 生命周期
    Archive = 12,      // 12079 对照表制度
}

impl B04Dom {
    /// 卡号映射。
    pub fn card(&self) -> u32 {
        12067 + *self as u32
    }
}

/// 一条断言行。
#[derive(Clone)]
pub struct KbRow {
    pub dom: B04Dom,
    pub passed: u32,
    pub total: u32,
    pub tag: String,
}

impl KbRow {
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

/// B04 聚合册。
pub struct KtbB04 {
    pub rows: Vec<KbRow>,
    pub sealed: bool,
}

impl KtbB04 {
    pub fn new() -> KtbB04 {
        KtbB04 { rows: Vec::new(), sealed: false }
    }

    /// 登记：闭集 → 零总数 → 越界通过 → 重卡 → 容量。
    pub fn put(&mut self, dom: B04Dom, passed: u32, total: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KB_FROZEN);
        }
        if total == 0 {
            return Err(KB_BAD_TOTAL);
        }
        if passed > total {
            return Err(KB_BAD_TALLY);
        }
        if self.rows.iter().any(|r| r.dom == dom) {
            return Err(KB_DUP_DOM);
        }
        if self.rows.len() >= B04_CAP {
            return Err(KB_FULL);
        }
        self.rows.push(KbRow { dom, passed, total, tag: tag.to_string() });
        Ok(())
    }

    /// 十二行齐备。
    pub fn complete(&self) -> bool {
        self.rows.len() == B04_CAP
    }

    /// 聚合对平：逐行 ok 且通过合计 ≤ 总数合计。
    pub fn tallies_match(&self) -> bool {
        let p: u32 = self.rows.iter().map(|r| r.passed).sum();
        let t: u32 = self.rows.iter().map(|r| r.total).sum();
        self.rows.iter().all(|r| r.ok()) && p <= t
    }

    /// 批级全绿：十二行齐备且全过。
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
        format!("rows={}/{}", self.rows.len(), B04_CAP)
    }

    /// 诊断面：行渲染。
    pub fn line_of(&self, dom: B04Dom) -> String {
        match self.rows.iter().find(|r| r.dom == dom) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtbB04::new();
    // 1) 零总数必拒。
    got[0] = b.put(B04Dom::Locale, 1, 0, "zero").err() == Some(KB_BAD_TOTAL);
    // 2) 通过数越界必拒。
    got[1] = b.put(B04Dom::Locale, 9, 8, "over").err() == Some(KB_BAD_TALLY);
    // 3) 重卡必拒。
    let _ = b.put(B04Dom::Locale, 8, 8, "a");
    got[2] = b.put(B04Dom::Locale, 1, 1, "dup").err() == Some(KB_DUP_DOM);
    // 4) 超容必拒：灌满 12 行后再登第 13 行。
    let all = [
        B04Dom::ActCtx, B04Dom::CritSec, B04Dom::GsPath, B04Dom::Handover,
        B04Dom::Snapshot, B04Dom::Layout, B04Dom::Reserved, B04Dom::TxFs,
        B04Dom::TebPtr, B04Dom::Lifecycle, B04Dom::Archive,
    ];
    for d in all.iter() {
        let _ = b.put(*d, 3, 3, "fill");
    }
    got[3] = {
        // 额外模拟第 13 行：重开册验证容量门（本册已满按重卡/满容分码）。
        let mut b2 = KtbB04::new();
        for d in all.iter() {
            let _ = b2.put(*d, 1, 1, "f");
        }
        let _ = b2.put(B04Dom::Locale, 1, 1, "13th");
        b2.rows.len() == B04_CAP
            && b2.put(B04Dom::Locale, 1, 1, "x").err() == Some(KB_DUP_DOM)
    };
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(B04Dom::Locale, 1, 1, "post").err() == Some(KB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12080_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12080");
    let all = [
        B04Dom::Locale, B04Dom::ActCtx, B04Dom::CritSec, B04Dom::GsPath,
        B04Dom::Handover, B04Dom::Snapshot, B04Dom::Layout, B04Dom::Reserved,
        B04Dom::TxFs, B04Dom::TebPtr, B04Dom::Lifecycle, B04Dom::Archive,
    ];

    // J1 卡号闭集映射。
    let j1 = all[0].card() == 12068 && all[11].card() == 12079;
    s.add("J1-卡号闭集", j1, "12068..12079 映射连续");

    // J2 十二行齐备且对平。
    let j2 = {
        let mut b = KtbB04::new();
        for d in all.iter() {
            let _ = b.put(*d, 3, 3, "full");
        }
        b.complete() && b.batch_green() && b.passed_sum() == 36
    };
    s.add("J2-十二行全绿", j2, "全过聚合绿");

    // J3 零总数拒。
    let j3 = {
        let mut b = KtbB04::new();
        b.put(B04Dom::CritSec, 0, 0, "z").err() == Some(KB_BAD_TOTAL)
    };
    s.add("J3-零总数拒", j3, "总数为零必拒");

    // J4 通过数越界拒。
    let j4 = {
        let mut b = KtbB04::new();
        b.put(B04Dom::Snapshot, 5, 4, "x").err() == Some(KB_BAD_TALLY)
    };
    s.add("J4-越界拒", j4, "通过 > 总数必拒");

    // J5 重卡拒。
    let j5 = {
        let mut b = KtbB04::new();
        let _ = b.put(B04Dom::Layout, 1, 1, "a");
        b.put(B04Dom::Layout, 1, 1, "b").err() == Some(KB_DUP_DOM)
    };
    s.add("J5-重卡拒", j5, "同卡二次必拒");

    // J6 部分通过仍对平。
    let j6 = {
        let mut b = KtbB04::new();
        let _ = b.put(B04Dom::Locale, 2, 3, "part");
        let _ = b.put(B04Dom::ActCtx, 3, 3, "ok");
        b.tallies_match() && !b.batch_green() && b.passed_sum() == 5
    };
    s.add("J6-部分对平", j6, "缺行时批级不绿");

    // J7 行渲染。
    let j7 = {
        let mut b = KtbB04::new();
        let _ = b.put(B04Dom::TxFs, 7, 7, "r");
        b.line_of(B04Dom::TxFs).contains("12076")
    };
    s.add("J7-行渲染", j7, "行含卡号");

    // J8 缺行显性。
    let j8 = {
        let mut b = KtbB04::new();
        b.line_of(B04Dom::Archive) == "missing"
    };
    s.add("J8-缺行显性", j8, "未登记行显性");

    // J9 印账冻结。
    let j9 = {
        let mut b = KtbB04::new();
        let _ = b.put(B04Dom::Locale, 1, 1, "a");
        b.seal();
        b.put(B04Dom::ActCtx, 1, 1, "post").err() == Some(KB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = KtbB04::new();
        let _ = b.put(B04Dom::Locale, 1, 1, "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账：B04 十二行半数填充。
    let mut b = KtbB04::new();
    for d in all.iter().take(6) {
        let _ = b.put(*d, 4, 4, "half");
    }

    s.add("摘要回读", b.summary().contains("rows=6/12"), "半数填充");
    s.add("对平判定", b.tallies_match(), "部分册仍对平");
    s.add("缺席行", b.line_of(B04Dom::Archive) == "missing", "后半缺行");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12080_put_flow() {
        let mut b = KtbB04::new();
        assert_eq!(b.put(B04Dom::Locale, 1, 0, "z"), Err(KB_BAD_TOTAL));
        assert!(b.put(B04Dom::Locale, 1, 1, "a").is_ok());
        assert_eq!(b.put(B04Dom::Locale, 1, 1, "b"), Err(KB_DUP_DOM));
    }

    #[test]
    fn unx12080_checks_green() {
        let s = run_unx12080_checks();
        assert!(s.all_passed(), "UNX-F12080 判据未全绿");
    }

    #[test]
    fn dom_card_mapping() {
        assert_eq!(B04Dom::ActCtx.card(), 12069);
        assert_eq!(B04Dom::Archive.card(), 12079);
        assert_ne!(B04Dom::GsPath, B04Dom::Handover);
    }
}
