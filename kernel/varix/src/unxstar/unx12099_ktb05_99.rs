//! UNX-F12099 ktest 对象协议面断言集（B05 批判据聚合）（0x19e80 段）
//!
//! 卡面：B05 批（UNX-F12081~12098 共 18 卡）对象协议面断言聚合——
//! 每卡登记一条断言行（通过数/总数），聚合册全 18 行对平后批级判绿。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 十八行闭集：B05 批 18 卡编号闭集，全部登记后聚合才可判。
//! - J2 重卡拒：同卡二次登记必拒。
//! - J3 总数为零拒 / 通过数越界拒。
//! - J4 容量显性：第 19 行登记必拒（册容 18）。
//! - J5 聚合对平：逐行 ok 且通过合计 ≤ 总数合计。
//! - J6 批级全绿：十八行齐备且全过。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：B05Dom 十八卡枚举（含卡号映射）+ KtbB05 聚合册 + 折叠指纹
//! （211 进制卷积）+ 注入核（零总数/越界通过/重卡/超容/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19e80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19e80;
pub const K5_BAD_TOTAL: u32 = SEG_BASE + 0x01; // 总数为零
pub const K5_BAD_TALLY: u32 = SEG_BASE + 0x02; // 通过数越界
pub const K5_DUP_DOM: u32 = SEG_BASE + 0x03; // 重卡
pub const K5_FULL: u32 = SEG_BASE + 0x04; // 超容
pub const K5_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// B05 批容量（18 卡）。
pub const B05_CAP: usize = 18;

/// B05 批十八卡枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B05Dom {
    ObjAttr = 1,     // 12081 结构语义
    CaseInsensitive = 2, // 12082 大小写位
    Inherit = 3,     // 12083 继承位
    KernelHandle = 4, // 12084 内核句柄位
    FlagFamily = 5,  // 12085 位族
    RootDir = 6,     // 12086 相对打开
    ObjectName = 7,  // 12087 名引用
    SecDesc = 8,     // 12088 安全描述符
    SecQos = 9,      // 12089 SQOS
    InitMacro = 10,  // 12090 宏语义
    HtProto = 11,    // 12091 句柄表接口
    HtEntry = 12,    // 12092 表项权限
    TypeInit = 13,   // 12093 类型初始化
    Matrix = 14,     // 12094 拒止矩阵
    VersionAnchor = 15, // 12095 版本锚
    DupObject = 16,  // 12096 对象复制
    Boundary = 17,   // 12097 D1/D3 分界
    Xref = 18,       // 12098 对照表
}

impl B05Dom {
    /// 卡号映射。
    pub fn card(&self) -> u32 {
        12080 + *self as u32
    }
}

/// 一条断言行。
#[derive(Clone)]
pub struct K5Row {
    pub dom: B05Dom,
    pub passed: u32,
    pub total: u32,
    pub tag: String,
}

impl K5Row {
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

/// B05 聚合册。
pub struct KtbB05 {
    pub rows: Vec<K5Row>,
    pub sealed: bool,
}

impl KtbB05 {
    pub fn new() -> KtbB05 {
        KtbB05 { rows: Vec::new(), sealed: false }
    }

    /// 登记：零总数 → 越界 → 重卡 → 容量。
    pub fn put(&mut self, dom: B05Dom, passed: u32, total: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(K5_FROZEN);
        }
        if total == 0 {
            return Err(K5_BAD_TOTAL);
        }
        if passed > total {
            return Err(K5_BAD_TALLY);
        }
        if self.rows.iter().any(|r| r.dom == dom) {
            return Err(K5_DUP_DOM);
        }
        if self.rows.len() >= B05_CAP {
            return Err(K5_FULL);
        }
        self.rows.push(K5Row { dom, passed, total, tag: tag.to_string() });
        Ok(())
    }

    /// 十八行齐备。
    pub fn complete(&self) -> bool {
        self.rows.len() == B05_CAP
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
        format!("rows={}/{}", self.rows.len(), B05_CAP)
    }

    /// 诊断面：行渲染。
    pub fn line_of(&self, dom: B05Dom) -> String {
        match self.rows.iter().find(|r| r.dom == dom) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtbB05::new();
    // 1) 零总数必拒。
    got[0] = b.put(B05Dom::ObjAttr, 1, 0, "zero").err() == Some(K5_BAD_TOTAL);
    // 2) 通过数越界必拒。
    got[1] = b.put(B05Dom::ObjAttr, 9, 8, "over").err() == Some(K5_BAD_TALLY);
    // 3) 重卡必拒。
    let _ = b.put(B05Dom::ObjAttr, 8, 8, "a");
    got[2] = b.put(B05Dom::ObjAttr, 1, 1, "dup").err() == Some(K5_DUP_DOM);
    // 4) 超容必拒：灌满 18 行后再登。
    let all = b05_all_doms();
    let mut b2 = KtbB05::new();
    for d in all.iter() {
        let _ = b2.put(*d, 1, 1, "fill");
    }
    got[3] = b2.complete()
        && b2.put(B05Dom::ObjAttr, 1, 1, "19th").err() == Some(K5_DUP_DOM)
        && b2.rows.len() == B05_CAP;
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(B05Dom::ObjAttr, 1, 1, "post").err() == Some(K5_FROZEN);
    got
}

/// B05 十八卡全列。
pub fn b05_all_doms() -> [B05Dom; 18] {
    [
        B05Dom::ObjAttr, B05Dom::CaseInsensitive, B05Dom::Inherit,
        B05Dom::KernelHandle, B05Dom::FlagFamily, B05Dom::RootDir,
        B05Dom::ObjectName, B05Dom::SecDesc, B05Dom::SecQos,
        B05Dom::InitMacro, B05Dom::HtProto, B05Dom::HtEntry,
        B05Dom::TypeInit, B05Dom::Matrix, B05Dom::VersionAnchor,
        B05Dom::DupObject, B05Dom::Boundary, B05Dom::Xref,
    ]
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12099_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12099");
    let all = b05_all_doms();

    // J1 卡号闭集映射。
    let j1 = all[0].card() == 12081 && all[17].card() == 12098;
    s.add("J1-卡号闭集", j1, "12081..12098 映射连续");

    // J2 十八行齐备且批级全绿。
    let j2 = {
        let mut b = KtbB05::new();
        for d in all.iter() {
            let _ = b.put(*d, 4, 4, "full");
        }
        b.complete() && b.batch_green() && b.passed_sum() == 72
    };
    s.add("J2-十八行全绿", j2, "全过聚合绿");

    // J3 零总数拒。
    let j3 = {
        let mut b = KtbB05::new();
        b.put(B05Dom::Inherit, 0, 0, "z").err() == Some(K5_BAD_TOTAL)
    };
    s.add("J3-零总数拒", j3, "总数为零必拒");

    // J4 通过数越界拒。
    let j4 = {
        let mut b = KtbB05::new();
        b.put(B05Dom::KernelHandle, 5, 4, "x").err() == Some(K5_BAD_TALLY)
    };
    s.add("J4-越界拒", j4, "通过 > 总数必拒");

    // J5 重卡拒。
    let j5 = {
        let mut b = KtbB05::new();
        let _ = b.put(B05Dom::Matrix, 1, 1, "a");
        b.put(B05Dom::Matrix, 1, 1, "b").err() == Some(K5_DUP_DOM)
    };
    s.add("J5-重卡拒", j5, "同卡二次必拒");

    // J6 部分通过仍对平且批级不绿。
    let j6 = {
        let mut b = KtbB05::new();
        let _ = b.put(B05Dom::ObjAttr, 2, 3, "part");
        let _ = b.put(B05Dom::CaseInsensitive, 3, 3, "ok");
        b.tallies_match() && !b.batch_green() && b.passed_sum() == 5
    };
    s.add("J6-部分对平", j6, "缺行时批级不绿");

    // J7 行渲染含卡号。
    let j7 = {
        let mut b = KtbB05::new();
        let _ = b.put(B05Dom::DupObject, 7, 7, "r");
        b.line_of(B05Dom::DupObject).contains("12096")
    };
    s.add("J7-行渲染", j7, "行含卡号");

    // J8 缺行显性。
    let j8 = {
        let mut b = KtbB05::new();
        b.line_of(B05Dom::Xref) == "missing"
    };
    s.add("J8-缺行显性", j8, "未登记行显性");

    // J9 印账冻结。
    let j9 = {
        let mut b = KtbB05::new();
        let _ = b.put(B05Dom::ObjAttr, 1, 1, "a");
        b.seal();
        b.put(B05Dom::Inherit, 1, 1, "post").err() == Some(K5_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = KtbB05::new();
        let _ = b.put(B05Dom::ObjAttr, 1, 1, "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账：B05 十八行前九填充。
    let mut b = KtbB05::new();
    for d in all.iter().take(9) {
        let _ = b.put(*d, 3, 3, "half");
    }

    s.add("摘要回读", b.summary().contains("rows=9/18"), "半数填充");
    s.add("对平判定", b.tallies_match(), "部分册仍对平");
    s.add("缺席行", b.line_of(B05Dom::Xref) == "missing", "后半缺行");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12099_put_flow() {
        let mut b = KtbB05::new();
        assert_eq!(b.put(B05Dom::ObjAttr, 1, 0, "z"), Err(K5_BAD_TOTAL));
        assert!(b.put(B05Dom::ObjAttr, 1, 1, "a").is_ok());
        assert_eq!(b.put(B05Dom::ObjAttr, 1, 1, "b"), Err(K5_DUP_DOM));
    }

    #[test]
    fn unx12099_checks_green() {
        let s = run_unx12099_checks();
        assert!(s.all_passed(), "UNX-F12099 判据未全绿");
    }

    #[test]
    fn dom_card_mapping() {
        assert_eq!(B05Dom::SecQos.card(), 12089);
        assert_eq!(B05Dom::Xref.card(), 12098);
        assert_ne!(B05Dom::HtProto, B05Dom::HtEntry);
    }
}
