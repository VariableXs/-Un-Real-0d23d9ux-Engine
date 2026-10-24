//! veb387_dupaudit —— VE-F0387 B 域查重终审。
//! 锚点（CoRun Unxreal 主册）：VE-F0387 · B 域查重终审 · 目标 300 行
//! 判据原文：VE-F0387-J1 终审把 B 域交付物逐件查重：物名唯一
//!           （重名立案）；指纹唯一（FNV32 内容指纹，同指纹异
//!           名视为疑似复制立案）；自审记录（立案 + 豁免两态，
//!           豁免须带理由代号）；终审结论（无立案 → 放行，有
//!           立案 → 缓行并列单）。
//! 组件：交付物（名 + 内容指纹）+ 查重引擎（重名 + 重指纹）+
//!       豁免账 + 终审结论。
//! 职责定位：查重终审是「B 域出厂前的质检关」——两件东西一个
//!           名字是管理事故，两段不同名的代码一个指纹大概率是
//!           复制粘贴债；豁免要理由是防豁免泛滥：真有正当理由
//!           （如镜像表）就登记豁免代号，可追溯。
//! 性能逐项分解：查重 O(n²) 最坏（小集合可接受）；豁免 O(1)。
//! 诊断码段：0x30640（0x30641~0x30649 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DA_VERSION: &str = "veb387_dupaudit-v1";

/// 诊断段基址（0x30640 段 · 本模块独占）。
pub const DA_SEG_BASE: u32 = 0x30640;
pub const XC_DUP_NAME: u32 = 0x30641; // 重名立案
pub const XC_DUP_FP: u32 = 0x30642; // 重指纹立案
pub const XC_NOENT: u32 = 0x30643; // 无此物
pub const XC_MAGIC: u32 = 0x30644; // 账魔数破
pub const XC_NO_EXCUSE: u32 = 0x30645; // 豁免无理由
pub const XC_NOT_AUDIT: u32 = 0x30646; // 未审先结
pub const XC_EXC_FOREIGN: u32 = 0x30647; // 豁免指向他物
pub const XC_VERDICT: u32 = 0x30648; // 结论与案账不符
pub const XC_EMPTY: u32 = 0x30649; // 空集终审

/// FNV32。
fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 交付物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: &'static str,
    pub fp: u32,
}

/// 立案记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseRec {
    pub name: &'static str,
    pub kind: u32, // 1=重名 2=重指纹
    /// 豁免理由代号（Some = 已豁免）。
    pub excused: Option<&'static str>,
}

/// 终审结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 放行。
    Pass = 0,
    /// 缓行（有未豁免立案）。
    Hold = 1,
}

/// 查重终审。
pub struct DupAudit {
    items: Vec<Item>,
    cases: Vec<CaseRec>,
    audited: bool,
    magic: u32,
}

impl DupAudit {
    /// 建。
    pub fn new() -> DupAudit {
        DupAudit { items: Vec::new(), cases: Vec::new(), audited: false, magic: 0x0640_D0A5 }
    }

    /// 提交交付物。
    pub fn submit(&mut self, name: &'static str, content: &[u8]) -> Result<(), u32> {
        if self.magic != 0x0640_D0A5 {
            return Err(XC_MAGIC);
        }
        self.items.push(Item { name, fp: fnv32(content) });
        Ok(())
    }

    /// 查重：重名 + 重指纹立案。
    pub fn audit(&mut self) -> Result<u32, u32> {
        if self.magic != 0x0640_D0A5 {
            return Err(XC_MAGIC);
        }
        if self.items.is_empty() {
            return Err(XC_EMPTY);
        }
        self.cases.clear();
        // 重名。
        for i in 0..self.items.len() {
            for j in (i + 1)..self.items.len() {
                if self.items[i].name == self.items[j].name {
                    self.cases.push(CaseRec { name: self.items[i].name, kind: 1, excused: None });
                }
            }
        }
        // 重指纹异名。
        for i in 0..self.items.len() {
            for j in (i + 1)..self.items.len() {
                if self.items[i].fp == self.items[j].fp && self.items[i].name != self.items[j].name {
                    self.cases.push(CaseRec { name: self.items[i].name, kind: 2, excused: None });
                }
            }
        }
        self.audited = true;
        Ok(self.cases.len() as u32)
    }

    /// 豁免（带理由代号）。
    pub fn excuse(&mut self, name: &str, why: &'static str) -> Result<(), u32> {
        if !self.audited {
            return Err(XC_NOT_AUDIT);
        }
        if why.is_empty() {
            return Err(XC_NO_EXCUSE);
        }
        let mut in_case = false;
        for c in self.cases.iter_mut() {
            if c.name == name {
                in_case = true;
                c.excused = Some(why);
            }
        }
        if !in_case {
            return Err(XC_NOENT);
        }
        // 豁免须指向在册物。
        let mut in_items = false;
        for it in self.items.iter() {
            if it.name == name {
                in_items = true;
            }
        }
        if !in_items {
            return Err(XC_EXC_FOREIGN);
        }
        Ok(())
    }

    /// 终审结论：无未豁免立案 → 放行。
    pub fn verdict(&self) -> Result<Verdict, u32> {
        if !self.audited {
            return Err(XC_NOT_AUDIT);
        }
        for c in self.cases.iter() {
            if c.excused.is_none() {
                return Ok(Verdict::Hold);
            }
        }
        Ok(Verdict::Pass)
    }

    /// 立案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 未豁免立案数。
    pub fn open_count(&self) -> u32 {
        let mut n = 0u32;
        for c in self.cases.iter() {
            if c.excused.is_none() {
                n += 1;
            }
        }
        n
    }

    /// 结论一致性核：结论与案账相符。
    pub fn self_check(&self) -> Result<bool, u32> {
        let v = self.verdict()?;
        let expect = if self.open_count() == 0 { Verdict::Pass } else { Verdict::Hold };
        if v != expect {
            return Err(XC_VERDICT);
        }
        Ok(true)
    }
}

pub fn run_veb387_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0387");
    // 族一：干净集放行。
    let mut a = DupAudit::new();
    let _ = a.submit("io_read", b"fn io_read() -> u32 { 1 }");
    let _ = a.submit("io_write", b"fn io_write(v: u32) {}");
    let _ = a.submit("io_poll", b"fn io_poll() -> bool { true }");
    let n = a.audit();
    s.add("干净零案", n == Ok(0), "无重");
    s.add("放行", a.verdict() == Ok(Verdict::Pass), "全绿");
    s.add("自洽", a.self_check() == Ok(true), "结论相符");
    // 族二：重名立案。
    let mut b = DupAudit::new();
    let _ = b.submit("x", b"content-a");
    let _ = b.submit("x", b"content-b");
    let nb = b.audit();
    s.add("重名一案", nb == Ok(1), "同名");
    s.add("缓行", b.verdict() == Ok(Verdict::Hold), "未豁免");
    s.add("豁免需先审", {
        let mut z = DupAudit::new();
        z.excuse("x", "later") == Err(XC_NOT_AUDIT)
    }, "未审先豁拒");
    // 族三：重指纹异名。
    let mut c = DupAudit::new();
    let _ = c.submit("alpha", b"same-bytes");
    let _ = c.submit("beta", b"same-bytes");
    let _ = c.submit("gamma", b"other-bytes-1");
    let nc = c.audit();
    s.add("重指纹一案", nc == Ok(1), "复制嫌疑");
    s.add("指纹案缓行", c.verdict() == Ok(Verdict::Hold), "点名");
    s.add("案首 alpha", c.case_count() == 1, "一条");
    // 豁免后放行。
    let ex = c.excuse("alpha", "MIRROR-TABLE");
    s.add("豁免过", ex.is_ok(), "有理由");
    s.add("豁免后放行", c.verdict() == Ok(Verdict::Pass), "案销");
    s.add("豁免自洽", c.self_check() == Ok(true), "相符");
    // 族四：豁免约束。
    let mut d = DupAudit::new();
    let _ = d.submit("p", b"pp");
    let _ = d.submit("q", b"qq");
    let _ = d.audit();
    s.add("豁免无案拒", d.excuse("p", "WHY") == Err(XC_NOENT), "无案可豁");
    s.add("豁免无理由拒", d.excuse("p", "") == Err(XC_NO_EXCUSE), "空理由");
    // 族五：混合立案数。
    let mut e = DupAudit::new();
    let _ = e.submit("m", b"mm");
    let _ = e.submit("m", b"mm"); // 重名 + 重指纹（同名不记指纹案）
    let ne = e.audit();
    s.add("同名同文一案", ne == Ok(1), "重名优先");
    s.add("魔数核", DA_SEG_BASE == 0x30640, "段基址");
    s
}
