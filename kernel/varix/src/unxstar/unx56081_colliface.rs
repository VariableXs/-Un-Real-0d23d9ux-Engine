//! UNX-F56081 采集器统一接口契约件（collect 函数族）（0x60A00 段）
//!
//! 锚点：UNX-F56081-J1 接口契约全量（collect 函数族五签名逐项落契：签名
//! 名 × 入参序 × 出参口径）；J2 契约守闸一致（调用侧入参序与出参口径
//! 逐项对契，错序/错径即拒）；J3 契约审计链（契约指纹、封账只读、签名
//! 闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·接口契约面）。
//!
//! 结构：collect 函数族签名闭集（采样/换算/聚合/存证/心跳五签名）+
//! 契约登记册（签名名 → 入参序 × 出参口径逐项落契）+ 调用守闸核（入参
//! 序对契 + 出参口径对契双闸）+ 契约指纹链 + 封账语义 + 注入核（未知
//! 签名/错序调用/错径出参/重复落契/封账改写/空契调用必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60A00 独占 0x20）。
pub const SEG_BASE: u32 = 0x60A00;
pub const IF_UNK_SIG: u32 = SEG_BASE + 0x01; // 未知签名（五闭集外）
pub const IF_ARG_ORDER: u32 = SEG_BASE + 0x02; // 入参序错（与契约不符）
pub const IF_RET_PATH: u32 = SEG_BASE + 0x03; // 出参口径错（与契约不符）
pub const IF_SIG_DUP: u32 = SEG_BASE + 0x04; // 同签名重复落契
pub const IF_SEALED: u32 = SEG_BASE + 0x05; // 契约册已封账拒写
pub const IF_EMPTY: u32 = SEG_BASE + 0x06; // 空契调用
pub const IF_ARG_BAD: u32 = SEG_BASE + 0x07; // 入参值非法（负值/越界）

/// collect 函数族签名闭集。
pub const F_SAMPLE: usize = 0;
pub const F_CONVERT: usize = 1;
pub const F_AGGREGATE: usize = 2;
pub const F_EVIDENCE: usize = 3;
pub const F_HEARTBEAT: usize = 4;
pub const F_COUNT: usize = 5;
pub const F_NAMES: [&str; F_COUNT] = ["collect_sample", "collect_convert", "collect_aggregate", "collect_evidence", "collect_heartbeat"];

/// 契约账目：签名 × 入参序 × 出参口径 三元组。
#[derive(Clone)]
pub struct IfContract {
    pub func: usize,
    pub arg_order: [usize; 2], // 入参序（两参签名；单参签名第二位填 0）
    pub ret_path: usize,
}

/// 契约总账。
pub struct IfBook {
    pub contracts: [Option<IfContract>; F_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl IfBook {
    pub fn new() -> Self {
        IfBook { contracts: [None, None, None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 落契：签名闭集 + 入参序合法 + 口径合法 + 重复落契拒四闸。
    pub fn define(&mut self, func: usize, arg_order: [usize; 2], ret_path: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(IF_SEALED);
        }
        if func >= F_COUNT {
            return Err(IF_UNK_SIG);
        }
        if self.contracts[func].is_some() {
            return Err(IF_SIG_DUP);
        }
        if arg_order[0] >= 4 || arg_order[1] >= 4 || ret_path >= 4 {
            return Err(IF_ARG_ORDER);
        }
        self.audit.push(format!(
            "define|{}|{},{}|{}",
            F_NAMES[func], arg_order[0], arg_order[1], ret_path
        ));
        self.contracts[func] = Some(IfContract { func, arg_order, ret_path });
        Ok(())
    }

    /// J2：调用守闸——入参序与出参口径逐项对契。
    pub fn invoke_check(&self, func: usize, given_order: [usize; 2], given_ret: usize) -> Result<(), u32> {
        if func >= F_COUNT {
            return Err(IF_UNK_SIG);
        }
        match &self.contracts[func] {
            None => Err(IF_EMPTY),
            Some(c) => {
                if c.arg_order != given_order {
                    return Err(IF_ARG_ORDER);
                }
                if c.ret_path != given_ret {
                    return Err(IF_RET_PATH);
                }
                Ok(())
            }
        }
    }

    /// J2：入参值守闸（非负 + 越界拒绝；0..4 为合法序位）。
    pub fn arg_value_ok(v: usize) -> bool {
        v < 4
    }

    /// J1：契约全量（五签名逐项落契）。
    pub fn contracts_ok(&self) -> bool {
        self.contracts.iter().all(|c| c.is_some())
    }

    /// J3：审计链完整（链长 = 签名数）。
    pub fn audit_ok(&self) -> bool {
        let mut n = 0usize;
        for c in self.contracts.iter() {
            if c.is_some() {
                n += 1;
            }
        }
        self.audit.len() == n && self.audit.iter().all(|a| a.starts_with("define|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐契复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for f in 0..F_COUNT {
            match &self.contracts[f] {
                None => {}
                Some(c) => {
                    let line = format!(
                        "{}|{},{}|{}",
                        F_NAMES[c.func], c.arg_order[0], c.arg_order[1], c.ret_path
                    );
                    for b in line.as_bytes() {
                        h ^= *b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                }
            }
        }
        h
    }
}

/// 标准夹具：五签名全落契（标准序 [0,1] 口径 2）。
pub fn standard_book() -> IfBook {
    let mut b = IfBook::new();
    for f in 0..F_COUNT {
        if b.define(f, [0, 1], 2).is_err() {
            panic!("standard fixture define must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知签名/错序调用/错径调用/重复落契/封账改写/空契调用/坏入参位）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知签名
    let mut b = IfBook::new();
    if b.define(9, [0, 1], 2) == Err(IF_UNK_SIG) {
        caught += 1;
    }
    // 2) 错序落契（序位越界）
    if b.define(F_SAMPLE, [0, 9], 2) == Err(IF_ARG_ORDER) {
        caught += 1;
    }
    // 3) 空契调用
    if b.invoke_check(F_SAMPLE, [0, 1], 2) == Err(IF_EMPTY) {
        caught += 1;
    }
    // 4) 重复落契
    if b.define(F_SAMPLE, [0, 1], 2).is_ok() && b.define(F_SAMPLE, [1, 0], 3) == Err(IF_SIG_DUP) {
        caught += 1;
    }
    // 5) 错序调用
    let sb = standard_book();
    if sb.invoke_check(F_SAMPLE, [1, 0], 2) == Err(IF_ARG_ORDER) {
        caught += 1;
    }
    // 6) 错径调用
    if sb.invoke_check(F_SAMPLE, [0, 1], 3) == Err(IF_RET_PATH) {
        caught += 1;
    }
    // 7) 封账后落契拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.define(F_HEARTBEAT, [0, 1], 2) == Err(IF_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 契约册渲染导出（逐契一行，未落契行 UNSET）。
pub fn render_book(b: &IfBook) -> String {
    let mut out = String::new();
    for f in 0..F_COUNT {
        match &b.contracts[f] {
            None => out.push_str(&format!("{}|UNSET\n", F_NAMES[f])),
            Some(c) => out.push_str(&format!(
                "{}|{},{}|{}\n",
                F_NAMES[c.func], c.arg_order[0], c.arg_order[1], c.ret_path
            )),
        }
    }
    out
}

/// 渲染完整性核：五行齐 + 全落契无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != F_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for f in 0..F_COUNT {
        if !text.contains(F_NAMES[f]) {
            return false;
        }
    }
    true
}

/// 契约指纹独立核（单签名指纹可复算）。
pub fn sig_fingerprint(func: usize, order: [usize; 2], ret: usize) -> Result<u32, u32> {
    if func >= F_COUNT {
        return Err(IF_UNK_SIG);
    }
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{},{}|{}", F_NAMES[func], order[0], order[1], ret);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    Ok(h)
}

/// 判据入口（CheckSet）。
pub fn run_unx56081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56081");
    // J1-1 契约全量（五签名逐项落契）
    let b = standard_book();
    let ok = b.contracts_ok() && !b.sealed;
    s.add("56081-J1-契约全量", ok, if ok { "" } else { "签名残缺" });
    // J1-2 签名闭集齐
    let ok = F_NAMES.len() == F_COUNT && F_NAMES[0] == "collect_sample";
    s.add("56081-J1-签名闭集齐", ok, if ok { "" } else { "签名漂移" });
    // J1-3 入参序位闭集
    let ok = IfBook::arg_value_ok(0) && IfBook::arg_value_ok(3) && !IfBook::arg_value_ok(4);
    s.add("56081-J1-序位闭集", ok, if ok { "" } else { "序位漂移" });
    // J2-1 对契调用放行
    let ok = b.invoke_check(F_SAMPLE, [0, 1], 2).is_ok();
    s.add("56081-J2-对契调用放行", ok, if ok { "" } else { "对契误拒" });
    // J2-2 错序调用拒
    let ok = b.invoke_check(F_AGGREGATE, [1, 0], 2) == Err(IF_ARG_ORDER);
    s.add("56081-J2-错序调用拒", ok, if ok { "" } else { "序闸失效" });
    // J2-3 错径调用拒
    let ok = b.invoke_check(F_CONVERT, [0, 1], 0) == Err(IF_RET_PATH);
    s.add("56081-J2-错径调用拒", ok, if ok { "" } else { "径闸失效" });
    // J2-4 空契调用拒
    let ok = IfBook::new().invoke_check(F_HEARTBEAT, [0, 1], 2) == Err(IF_EMPTY);
    s.add("56081-J2-空契调用拒", ok, if ok { "" } else { "空契闸失效" });
    // J3-1 审计链完整（链长 5）
    let ok = b.audit_ok() && b.audit.len() == F_COUNT;
    s.add("56081-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56081-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 契约指纹敏感（改口径指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b2 = IfBook::new();
    for f in 0..F_COUNT {
        let _ = b2.define(f, [0, 1], if f == F_SAMPLE { 3 } else { 2 });
    }
    let ok = b2.seal_fingerprint() != fp0;
    s.add("56081-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 签名指纹独立可复算
    let ok = sig_fingerprint(F_SAMPLE, [0, 1], 2).is_ok()
        && sig_fingerprint(F_SAMPLE, [0, 1], 2) != sig_fingerprint(F_SAMPLE, [0, 1], 3);
    s.add("56081-J3-签名指纹复算", ok, if ok { "" } else { "签名指纹漂移" });
    // J2-5 渲染导出五行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("collect_heartbeat|0,1|2");
    s.add("56081-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 封账后落契拒
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.define(F_SAMPLE, [0, 1], 2) == Err(IF_SEALED);
    s.add("56081-J1-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56081-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56081_all_checks_green() {
        let set = run_unx56081_checks();
        assert!(set.all_passed(), "UNX-F56081 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56081_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
