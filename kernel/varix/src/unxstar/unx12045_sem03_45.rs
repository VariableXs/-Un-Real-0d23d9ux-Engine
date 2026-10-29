//! UNX-F12045 ApiSetMap 指针语义与 API Set 解析预告（0x197C0 段）
//!
//! 判据原文：ApiSetMap 解析面——契约名（api-ms-win-* 前缀）按映射册
//! 归约到宿主名；非契约名直通不解析；册内查无契约即解析失败（红账）；
//! 空契约名与坏前缀即拒；解析逐笔可对账。
//!
//! 结构：映射册（ApiSetBook：契约 → 宿主）+ 归约核（四码守卫）+
//! 解析账（AsRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12045_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x197C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x197C0;
pub const AS_BAD_CONTRACT: u32 = SEG_BASE + 0x01; // 空契约/坏前缀
pub const AS_NOT_IN_MAP: u32 = SEG_BASE + 0x02; // 册内查无契约
pub const AS_DUP: u32 = SEG_BASE + 0x03; // 重复登记
pub const AS_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造解析账
pub const AS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 契约名前缀钉值。
pub const CONTRACT_PREFIX: &str = "api-ms-win-";

/// 契约名判定：非空且带前缀。
pub fn is_contract(name: &str) -> bool {
    name.starts_with(CONTRACT_PREFIX) && name.len() > CONTRACT_PREFIX.len()
}

/// 一笔解析的账目。
#[derive(Clone)]
pub struct AsRec {
    pub seq: u32,
    pub contract: String,
    /// 归约结果（直通名即原名）。
    pub resolved: String,
    pub hit: bool,
    pub note: String,
}

impl AsRec {
    pub fn ok(&self) -> bool {
        // 红账（查无契约，hit=false）允许 resolved 为空。
        if !self.note.is_empty() && self.seq > 0 && !self.hit {
            return true;
        }
        !self.note.is_empty() && self.seq > 0 && !self.resolved.is_empty()
    }
}

/// 映射册模型。
pub struct ApiSetBook {
    pub pairs: Vec<(String, String)>,
    pub ledger: Vec<AsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ApiSetBook {
    pub fn new() -> ApiSetBook {
        ApiSetBook { pairs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：契约名合法 + 唯一 + 宿主非空。
    pub fn enlist(&mut self, contract: &str, host: &str, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AS_FROZEN);
        }
        if note.is_empty() {
            return Err(AS_BAD_REC);
        }
        if !is_contract(contract) || host.is_empty() {
            return Err(AS_BAD_CONTRACT);
        }
        if self.pairs.iter().any(|p| p.0 == contract) {
            return Err(AS_DUP);
        }
        self.pairs.push((contract.to_string(), host.to_string()));
        Ok(())
    }

    /// 归约核：非契约名直通；契约名查册；查无即红账。
    pub fn resolve(&mut self, name: &str, note: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(AS_FROZEN);
        }
        if note.is_empty() {
            return Err(AS_BAD_REC);
        }
        if !is_contract(name) {
            // 直通名入账（hit=false 语义不适用，用 hit=true + 同名）。
            let r = AsRec {
                seq: self.seq_next,
                contract: name.to_string(),
                resolved: name.to_string(),
                hit: true,
                note: note.to_string(),
            };
            if !r.ok() {
                return Err(AS_BAD_REC);
            }
            self.seq_next += 1;
            self.ledger.push(r);
            return Ok(name.to_string());
        }
        match self.pairs.iter().find(|p| p.0 == name) {
            Some((_, host)) => {
                let r = AsRec {
                    seq: self.seq_next,
                    contract: name.to_string(),
                    resolved: host.clone(),
                    hit: true,
                    note: note.to_string(),
                };
                if !r.ok() {
                    return Err(AS_BAD_REC);
                }
                self.seq_next += 1;
                self.ledger.push(r);
                Ok(host.clone())
            }
            None => {
                let r = AsRec {
                    seq: self.seq_next,
                    contract: name.to_string(),
                    resolved: String::new(),
                    hit: false,
                    note: note.to_string(),
                };
                if !r.ok() {
                    return Err(AS_BAD_REC);
                }
                self.seq_next += 1;
                self.ledger.push(r);
                Err(AS_NOT_IN_MAP)
            }
        }
    }

    /// 册数。
    pub fn count(&self) -> usize {
        self.pairs.len()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for p in &self.pairs {
            for by in p.0.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ApiSetBook::new();
    let _ = b.enlist(
        "api-ms-win-core-file-l1-1-0",
        "kernel32",
        "ok",
    );
    // 1) 空契约名 → AS_BAD_CONTRACT。
    got[0] = b.enlist("", "host", "e").err() == Some(AS_BAD_CONTRACT);
    // 2) 坏前缀（非契约名冒充契约）→ AS_BAD_CONTRACT。
    got[1] = b.enlist("ntdll", "host", "p").err() == Some(AS_BAD_CONTRACT);
    // 3) 契约查无 → AS_NOT_IN_MAP。
    got[2] = b
        .resolve("api-ms-win-core-ghost-l1-1-0", "g")
        .err() == Some(AS_NOT_IN_MAP);
    // 4) 空留痕 → AS_BAD_REC。
    got[3] = b
        .resolve("api-ms-win-core-file-l1-1-0", "")
        .err() == Some(AS_BAD_REC);
    // 5) 印账后动作 → AS_FROZEN。
    b.seal();
    got[4] = b
        .resolve("api-ms-win-core-file-l1-1-0", "post")
        .err() == Some(AS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12045");

    // J1 正路径：契约归约到宿主。
    let j1 = {
        let mut b = ApiSetBook::new();
        let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a");
        b.resolve("api-ms-win-core-file-l1-1-0", "r") == Ok("kernel32".to_string())
    };
    s.add("J1-契约归约", j1, "file→kernel32");

    // J2 非契约名直通。
    let j2 = {
        let mut b = ApiSetBook::new();
        b.resolve("ntdll", "pass") == Ok("ntdll".to_string())
    };
    s.add("J2-直通名", j2, "ntdll 原样");

    // J3 前缀判定。
    let j3 = is_contract("api-ms-win-core-x-l1-1-0")
        && !is_contract("ntdll")
        && !is_contract("api-ms-win-")
        && !is_contract("");
    s.add("J3-前缀判定", j3, "四面对账");

    // J4 查无契约红账。
    let j4 = {
        let mut b = ApiSetBook::new();
        b.resolve("api-ms-win-core-ghost-l1-1-0", "g").err() == Some(AS_NOT_IN_MAP)
    };
    s.add("J4-查无拒", j4, "解析失败入账");

    // J5 重复登记拒。
    let j5 = {
        let mut b = ApiSetBook::new();
        let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a");
        b.enlist("api-ms-win-core-file-l1-1-0", "other", "b") == Err(AS_DUP)
    };
    s.add("J5-重复拒", j5, "契约唯一");

    // J6 空宿主拒。
    let j6 = {
        let mut b = ApiSetBook::new();
        b.enlist("api-ms-win-core-file-l1-1-0", "", "h") == Err(AS_BAD_CONTRACT)
    };
    s.add("J6-空宿主拒", j6, "宿主必填");

    // J7 账目对账：直通与归约同账。
    let j7 = {
        let mut b = ApiSetBook::new();
        let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a");
        let _ = b.resolve("api-ms-win-core-file-l1-1-0", "r1");
        let _ = b.resolve("ntdll", "r2");
        b.ledger.len() == 2 && b.ledger_ok()
    };
    s.add("J7-解析对账", j7, "两笔留痕");

    // J8 指纹区分度与幂等。
    let j8 = {
        let mut a = ApiSetBook::new();
        let _ = a.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = ApiSetBook::new();
        let _ = c.enlist("api-ms-win-core-path-l1-1-0", "kernel32", "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J8-指纹性质", j8, "幂等且异册异纹");

    // J9 印账冻结。
    let j9 = {
        let mut b = ApiSetBook::new();
        let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a");
        b.seal();
        b.resolve("api-ms-win-core-file-l1-1-0", "post").err() == Some(AS_FROZEN)
            && b.enlist("api-ms-win-core-path-l1-1-0", "x", "post").err() == Some(AS_FROZEN)
    };
    s.add("J9-印账冻结", j9, "双面操作印后全拒");

    // J10 册数与命中清点。
    let j10 = {
        let mut b = ApiSetBook::new();
        let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a");
        let _ = b.enlist("api-ms-win-core-path-l1-1-0", "msvcrt", "b");
        b.count() == 2
    };
    s.add("J10-册数清点", j10, "双契约在册");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：双契约解析面。
    let mut b = ApiSetBook::new();
    let _ = b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "sum-1");
    let _ = b.enlist("api-ms-win-core-path-l1-1-0", "msvcrt", "sum-2");
    let _ = b.resolve("api-ms-win-core-file-l1-1-0", "sum-r1");
    let _ = b.resolve("api-ms-win-core-path-l1-1-0", "sum-r2");
    s.add("样例双归约", b.ledger.len() == 2, "两笔解析");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12045_resolve_flow() {
        let mut b = ApiSetBook::new();
        assert!(b.enlist("api-ms-win-core-file-l1-1-0", "kernel32", "a").is_ok());
        assert_eq!(b.resolve("api-ms-win-core-file-l1-1-0", "r"), Ok("kernel32".to_string()));
        assert_eq!(b.resolve("ntdll", "p"), Ok("ntdll".to_string()));
        assert!(b.resolve("api-ms-win-core-ghost-l1-1-0", "g").is_err());
    }

    #[test]
    fn unx12045_checks_green() {
        let s = run_unx12045_checks();
        assert!(s.all_passed(), "UNX-F12045 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12045_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
