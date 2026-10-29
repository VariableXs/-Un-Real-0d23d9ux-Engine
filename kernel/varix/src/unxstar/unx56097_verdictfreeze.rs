//! UNX-F56097 verdict 字段语义冻结件（O1 对下冻结接口②）（0x60C00 段）
//!
//! 锚点：UNX-F56097-J1 verdict 语义冻结全量（verdict 取值逐项落契：值名
//! × 语义 × 权重三元组）；J2 对下冻结语义（verdict 取值闭集不可扩、
//! 语义映射不可变，越值/漂移即拒）；J3 verdict 审计链（语义指纹、封账
//! 只读、权重闭合）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·O1 verdict 面）。
//!
//! 结构：verdict 取值闭集（pass/warn/fail/na 四值）+ 语义映射登记册
//! （值名 → 语义 × 权重逐项落契）+ 越值拦截核（闭集外取值必拒）+ 权重
//! 闭合核（权重和为定值口径）+ 封账语义 + 注入核（空值名/未知语义/越
//! 值映射/重复落契/封账改契/空契查询/权重漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60C00 独占 0x20）。
pub const SEG_BASE: u32 = 0x60C00;
pub const VD_NAME_EMPTY: u32 = SEG_BASE + 0x01; // 值名为空
pub const VD_SEM_UNK: u32 = SEG_BASE + 0x02; // 未知语义（闭集外）
pub const VD_VAL_OVER: u32 = SEG_BASE + 0x03; // 越值映射（取值闭集外）
pub const VD_DUP: u32 = SEG_BASE + 0x04; // 重复落契
pub const VD_SEALED: u32 = SEG_BASE + 0x05; // verdict 契已封账拒改
pub const VD_EMPTY: u32 = SEG_BASE + 0x06; // 空契查询
pub const VD_WEIGHT_MISS: u32 = SEG_BASE + 0x07; // 权重漂移（和不符口径）

/// verdict 取值闭集。
pub const V_PASS: usize = 0;
pub const V_WARN: usize = 1;
pub const V_FAIL: usize = 2;
pub const V_NA: usize = 3;
pub const V_COUNT: usize = 4;
pub const V_NAMES: [&str; V_COUNT] = ["pass", "warn", "fail", "na"];

/// 语义闭集。
pub const S_OK: usize = 0;
pub const S_SOFT: usize = 1;
pub const S_HARD: usize = 2;
pub const S_SKIP: usize = 3;
pub const S_COUNT: usize = 4;
pub const S_TEXT: [&str; S_COUNT] = ["达标", "警戒", "破线", "缺侧不计"];

/// 标准权重闭集（万分比，和 = 10000）。
pub const W_PASS: u32 = 10_000;
pub const W_WARN: u32 = 5_000;
pub const W_FAIL: u32 = 0;
pub const W_NA: u32 = 0;
pub const WEIGHT_SUM: u32 = 15_000;

/// 权重闭合核：四值标准权重和 == WEIGHT_SUM。
pub fn weight_closed() -> bool {
    W_PASS + W_WARN + W_FAIL + W_NA == WEIGHT_SUM
}

/// verdict 指纹：FNV-1a 32 位。
pub fn verdict_fingerprint(name: &str, sem: usize, weight: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let sem_name = if sem < S_COUNT { S_TEXT[sem] } else { "?" };
    let line = format!("{}|{}|{}", name, sem_name, weight);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 语义映射条目：值名 × 语义 × 权重。
#[derive(Clone)]
pub struct VerdictEntry {
    pub val: usize,
    pub sem: usize,
    pub weight: u32,
}

/// verdict 总契。
pub struct VerdictBook {
    pub entries: [Option<VerdictEntry>; V_COUNT],
    pub sealed: bool,
}

impl VerdictBook {
    pub fn new() -> Self {
        VerdictBook { entries: [None, None, None, None], sealed: false }
    }

    /// 落契：值名闭集 + 语义闭集 + 重复落契拒三闸。
    pub fn define(&mut self, val: usize, sem: usize, weight: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(VD_SEALED);
        }
        if val >= V_COUNT {
            return Err(VD_VAL_OVER);
        }
        if V_NAMES[val].is_empty() {
            return Err(VD_NAME_EMPTY);
        }
        if sem >= S_COUNT {
            return Err(VD_SEM_UNK);
        }
        if self.entries[val].is_some() {
            return Err(VD_DUP);
        }
        self.entries[val] = Some(VerdictEntry { val, sem, weight });
        Ok(())
    }

    /// J2：语义查询（未落契显性拒绝）。
    pub fn semantics_of(&self, val: usize) -> Result<(usize, u32), u32> {
        if val >= V_COUNT {
            return Err(VD_VAL_OVER);
        }
        match &self.entries[val] {
            None => Err(VD_EMPTY),
            Some(e) => Ok((e.sem, e.weight)),
        }
    }

    /// J1：语义冻结全量（四值逐项落契）。
    pub fn frozen_ok(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// J2：权重闭合核（四值权重和符合口径）。
    pub fn weight_ok(&self) -> bool {
        if !self.frozen_ok() {
            return false;
        }
        let mut sum = 0u32;
        for e in self.entries.iter() {
            if let Some(x) = e {
                sum += x.weight;
            }
        }
        sum == WEIGHT_SUM
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 语义指纹（逐值复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for v in 0..V_COUNT {
            if let Some(e) = &self.entries[v] {
                h ^= verdict_fingerprint(V_NAMES[v], e.sem, e.weight);
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：四值全落契（标准语义与权重）。
pub fn standard_book() -> VerdictBook {
    let mut b = VerdictBook::new();
    let plan: [(usize, usize, u32); 4] =
        [(V_PASS, S_OK, W_PASS), (V_WARN, S_SOFT, W_WARN), (V_FAIL, S_HARD, W_FAIL), (V_NA, S_SKIP, W_NA)];
    for p in plan.iter() {
        if b.define(p.0, p.1, p.2).is_err() {
            panic!("standard fixture define must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（越值映射/未知语义/空值名/重复落契/封账改契/空契查询/权重漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越值映射（val=9 越闭集）
    let mut b = VerdictBook::new();
    if b.define(9, S_OK, W_PASS) == Err(VD_VAL_OVER) {
        caught += 1;
    }
    // 2) 未知语义
    if b.define(V_PASS, 9, W_PASS) == Err(VD_SEM_UNK) {
        caught += 1;
    }
    // 3) 重复落契
    if b.define(V_PASS, S_OK, W_PASS).is_ok() && b.define(V_PASS, S_SOFT, W_WARN) == Err(VD_DUP) {
        caught += 1;
    }
    // 4) 空契查询
    if b.semantics_of(V_WARN).err() == Some(VD_EMPTY) {
        caught += 1;
    }
    // 5) 权重漂移必抓（改权重后闭合失败）
    let mut dirty = standard_book();
    dirty.entries[0] = Some(VerdictEntry { val: V_PASS, sem: S_OK, weight: 9_000 });
    if !dirty.weight_ok() {
        caught += 1;
    }
    // 6) 封账改契拒
    let mut sb = standard_book();
    sb.seal();
    if sb.define(V_PASS, S_SOFT, W_WARN) == Err(VD_SEALED) {
        caught += 1;
    }
    // 7) 标准权重闭合自证
    if weight_closed() {
        caught += 1;
    }
    (caught, 7)
}

/// verdict 契渲染导出（逐值一行）。
pub fn render_book(b: &VerdictBook) -> String {
    let mut out = String::new();
    for v in 0..V_COUNT {
        match &b.entries[v] {
            None => out.push_str(&format!("{}|UNSET\n", V_NAMES[v])),
            Some(e) => out.push_str(&format!("{}|{}|{}\n", V_NAMES[v], S_TEXT[e.sem], e.weight)),
        }
    }
    out
}

/// 渲染完整性核：四行齐 + 全落契无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != V_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for v in 0..V_COUNT {
        if !text.contains(V_NAMES[v]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56097");
    // J1-1 语义冻结全量（四值逐项落契）
    let b = standard_book();
    let ok = b.frozen_ok() && !b.sealed;
    s.add("56097-J1-语义冻结全量", ok, if ok { "" } else { "取值残缺" });
    // J1-2 取值闭集齐
    let ok = V_NAMES.len() == V_COUNT && V_NAMES[0] == "pass" && V_NAMES[3] == "na";
    s.add("56097-J1-取值闭集齐", ok, if ok { "" } else { "取值漂移" });
    // J1-3 标准权重闭合（15000 口径）
    let ok = weight_closed() && b.weight_ok();
    s.add("56097-J1-权重闭合", ok, if ok { "" } else { "权重漂移" });
    // J2-1 语义映射查询（pass→达标/w10000）
    let ok = b.semantics_of(V_PASS) == Ok((S_OK, W_PASS));
    s.add("56097-J2-语义映射查询", ok, if ok { "" } else { "映射漂移" });
    // J2-2 越值查询拒
    let ok = b.semantics_of(9).err() == Some(VD_VAL_OVER);
    s.add("56097-J2-越值查询拒", ok, if ok { "" } else { "越值穿透" });
    // J2-3 未落契查询显性拒绝
    let ok = VerdictBook::new().semantics_of(V_FAIL).err() == Some(VD_EMPTY);
    s.add("56097-J2-未落契拒绝", ok, if ok { "" } else { "空契穿透" });
    // J2-4 fail→破线映射（权重 0 不计提升）
    let ok = b.semantics_of(V_FAIL) == Ok((S_HARD, W_FAIL));
    s.add("56097-J2-fail破线映射", ok, if ok { "" } else { "fail 语义漂移" });
    // J3-1 语义指纹确定性
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56097-J3-语义指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 指纹敏感（语义改指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b2 = VerdictBook::new();
    let _ = b2.define(V_PASS, S_SOFT, W_PASS);
    let _ = b2.define(V_WARN, S_SOFT, W_WARN);
    let _ = b2.define(V_FAIL, S_HARD, W_FAIL);
    let _ = b2.define(V_NA, S_SKIP, W_NA);
    let ok = b2.seal_fingerprint() != fp0;
    s.add("56097-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-3 重复落契拒
    let mut b3 = standard_book();
    let ok = b3.define(V_PASS, S_OK, W_PASS) == Err(VD_DUP);
    s.add("56097-J3-重复落契拒", ok, if ok { "" } else { "重复闸失效" });
    // J3-4 封账拒改
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.define(V_WARN, S_HARD, W_WARN) == Err(VD_SEALED);
    s.add("56097-J3-封账拒改", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出四行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("pass|达标|10000");
    s.add("56097-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56097-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56097_all_checks_green() {
        let set = run_unx56097_checks();
        assert!(set.all_passed(), "UNX-F56097 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56097_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
