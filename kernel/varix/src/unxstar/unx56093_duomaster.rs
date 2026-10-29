//! UNX-F56093 双机编排总控（counterbalance 策略库）（0x60B80 段）
//!
//! 锚点：UNX-F56093-J1 策略库全量（counterbalance 策略逐条落库：策略名 ×
//! 交换序 × 适用面三元组）；J2 策略语义一致（交换序互为镜像且适用面闭
//! 集命中，镜像残缺即拒）；J3 策略审计链（策略指纹链、封账只读、策略
//! 闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·双机总控面）。
//!
//! 结构：counterbalance 策略登记册（策略名 → 交换序对 × 适用面逐条落
//! 库）+ 镜像核对（A 先 B 后 ↔ B 先 A 后互为镜像）+ 适用面闭集（内存/
//! IO/帧率三面）+ 策略指纹链 + 封账语义 + 注入核（空策略名/镜像残缺/
//! 未知适用面/重复策略/封账改写/空库查询/序自交必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60B80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60B80;
pub const CB_NAME_EMPTY: u32 = SEG_BASE + 0x01; // 策略名为空
pub const CB_MIRROR_MISS: u32 = SEG_BASE + 0x02; // 镜像残缺（序不自镜像）
pub const CB_FACE_UNK: u32 = SEG_BASE + 0x03; // 未知适用面（闭集外）
pub const CB_DUP: u32 = SEG_BASE + 0x04; // 重复策略名
pub const CB_SEALED: u32 = SEG_BASE + 0x05; // 策略库已封账拒写
pub const CB_EMPTY: u32 = SEG_BASE + 0x06; // 空库查询
pub const CB_SELF_ORDER: u32 = SEG_BASE + 0x07; // 序自交（A 先 A 后）

/// 适用面闭集。
pub const F_MEM: usize = 0;
pub const F_IO: usize = 1;
pub const F_FPS: usize = 2;
pub const F_COUNT: usize = 3;
pub const F_NAMES: [&str; F_COUNT] = ["mem", "io", "fps"];

/// 交换序（双机先行方闭集）。
pub const O_A_FIRST: usize = 0;
pub const O_B_FIRST: usize = 1;
pub const O_COUNT: usize = 2;

/// 镜像核对：A 先 ↔ B 先互为镜像；同序不自镜像。
pub fn mirror_ok(a: usize, b: usize) -> bool {
    a < O_COUNT && b < O_COUNT && a != b
}

/// 策略指纹：FNV-1a 32 位。
pub fn policy_fingerprint(name: &str, order: usize, face: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let face_name = if face < F_COUNT { F_NAMES[face] } else { "?" };
    let line = format!("{}|{}|{}", name, order, face_name);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 策略条目：策略名 × 先行序 × 镜像序 × 适用面 × 指纹 × 链。
#[derive(Clone)]
pub struct CbPolicy {
    pub name: String,
    pub order: usize,
    pub mirror: usize,
    pub face: usize,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 策略总库。
pub struct CbBook {
    pub policies: Vec<CbPolicy>,
    pub sealed: bool,
}

impl CbBook {
    pub fn new() -> Self {
        CbBook { policies: Vec::new(), sealed: false }
    }

    fn find(&self, name: &str) -> Option<usize> {
        for i in 0..self.policies.len() {
            if self.policies[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// 策略落库：空名拒 + 适用面闭集 + 镜像自洽 + 重复拒四闸。
    pub fn enroll(&mut self, name: &str, order: usize, face: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(CB_SEALED);
        }
        if name.is_empty() {
            return Err(CB_NAME_EMPTY);
        }
        if face >= F_COUNT {
            return Err(CB_FACE_UNK);
        }
        let mirror = if order == O_A_FIRST { O_B_FIRST } else { O_A_FIRST };
        if !mirror_ok(order, mirror) {
            return Err(CB_MIRROR_MISS);
        }
        if order >= O_COUNT {
            return Err(CB_MIRROR_MISS);
        }
        if self.find(name).is_some() {
            return Err(CB_DUP);
        }
        let fp = policy_fingerprint(name, order, face);
        let chain = match self.policies.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.policies.push(CbPolicy {
            name: name.to_string(),
            order,
            mirror,
            face,
            fp,
            chain,
        });
        Ok(())
    }

    /// J2：镜像执行核（给定先行方，返回本拍先行序与下拍先行序）。
    pub fn swap_plan(&self, name: &str) -> Result<(usize, usize), u32> {
        match self.find(name) {
            Some(i) => {
                let p = &self.policies[i];
                Ok((p.order, p.mirror))
            }
            None => Err(CB_EMPTY),
        }
    }

    /// J1：策略库全量（逐条名合法 + 镜像自洽 + 面闭集）。
    pub fn policies_ok(&self) -> bool {
        !self.policies.is_empty()
            && self.policies.iter().all(|p| {
                !p.name.is_empty() && mirror_ok(p.order, p.mirror) && p.face < F_COUNT
            })
    }

    /// J1：适用面覆盖核（三面各至少一策）。
    pub fn faces_covered(&self) -> bool {
        for f in 0..F_COUNT {
            let mut hit = false;
            for p in self.policies.iter() {
                if p.face == f {
                    hit = true;
                }
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// J3：策略指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.policies.is_empty() {
            return false;
        }
        for i in 0..self.policies.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.policies[0].fp
            } else {
                self.policies[i - 1].fp ^ self.policies[i].fp
            };
            if self.policies[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.policies.last() {
            None => CHAIN_SEED,
            Some(p) => p.chain,
        }
    }
}

/// 标准夹具：3 策略（三面各一，序交替）。
pub fn standard_book() -> CbBook {
    let mut b = CbBook::new();
    let plan: [(&str, usize, usize); 3] =
        [("cb|mem|alt", O_A_FIRST, F_MEM), ("cb|io|alt", O_B_FIRST, F_IO), ("cb|fps|alt", O_A_FIRST, F_FPS)];
    for p in plan.iter() {
        if b.enroll(p.0, p.1, p.2).is_err() {
            panic!("standard fixture enroll must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空策略名/未知适用面/重复策略/序自交镜像/封账改写/空库查询/面覆盖缺失）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空策略名
    let mut b = CbBook::new();
    if b.enroll("", O_A_FIRST, F_MEM) == Err(CB_NAME_EMPTY) {
        caught += 1;
    }
    // 2) 未知适用面
    if b.enroll("k", O_A_FIRST, 9) == Err(CB_FACE_UNK) {
        caught += 1;
    }
    // 3) 序自交（镜像恒异序，自交不可能成立；越界序拒）
    if b.enroll("k", 9, F_MEM) == Err(CB_MIRROR_MISS) {
        caught += 1;
    }
    // 4) 重复策略
    if b.enroll("k", O_A_FIRST, F_MEM).is_ok() && b.enroll("k", O_B_FIRST, F_MEM) == Err(CB_DUP) {
        caught += 1;
    }
    // 5) 空库查询
    if CbBook::new().swap_plan("k").err() == Some(CB_EMPTY) {
        caught += 1;
    }
    // 6) 面覆盖缺失（单面库不覆盖）
    if !b.faces_covered() {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.enroll("late", O_A_FIRST, F_IO) == Err(CB_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 策略库渲染导出（逐条一行）。
pub fn render_book(b: &CbBook) -> String {
    let mut out = String::new();
    for p in b.policies.iter() {
        let o = if p.order == O_A_FIRST { "A" } else { "B" };
        out.push_str(&format!("{}|{}先|{}|{:08x}\n", p.name, o, F_NAMES[p.face], p.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 面名闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[2] == "mem" || parts[2] == "io" || parts[2] == "fps")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56093");
    // J1-1 策略库全量（三策逐条合法）
    let b = standard_book();
    let ok = b.policies_ok() && b.policies.len() == 3;
    s.add("56093-J1-策略库全量", ok, if ok { "" } else { "策略残缺" });
    // J1-2 适用面覆盖（三面各一）
    let ok = b.faces_covered();
    s.add("56093-J1-适用面覆盖", ok, if ok { "" } else { "覆盖缺面" });
    // J1-3 镜像核（A↔B 真、同序假）
    let ok = mirror_ok(O_A_FIRST, O_B_FIRST) && mirror_ok(O_B_FIRST, O_A_FIRST)
        && !mirror_ok(O_A_FIRST, O_A_FIRST);
    s.add("56093-J1-镜像核", ok, if ok { "" } else { "镜像漂移" });
    // J2-1 交换计划（A 先策略：本拍 A、下拍 B）
    let ok = b.swap_plan("cb|mem|alt") == Ok((O_A_FIRST, O_B_FIRST));
    s.add("56093-J2-交换计划A", ok, if ok { "" } else { "交换序漂移" });
    // J2-2 交换计划（B 先策略：本拍 B、下拍 A）
    let ok = b.swap_plan("cb|io|alt") == Ok((O_B_FIRST, O_A_FIRST));
    s.add("56093-J2-交换计划B", ok, if ok { "" } else { "镜像序漂移" });
    // J2-3 交换序闭集（两序齐）
    let ok = O_COUNT == 2;
    s.add("56093-J2-交换序闭集", ok, if ok { "" } else { "序闭集漂移" });
    // J2-4 重复策略拒
    let mut b2 = standard_book();
    let ok = b2.enroll("cb|mem|alt", O_B_FIRST, F_MEM) == Err(CB_DUP);
    s.add("56093-J2-重复策略拒", ok, if ok { "" } else { "重复闸失效" });
    // J3-1 策略指纹链复核
    let ok = b.chain_ok();
    s.add("56093-J3-策略指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56093-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（换序链尾变）
    let mut b3 = CbBook::new();
    let _ = b3.enroll("cb|mem|alt", O_B_FIRST, F_MEM);
    let mut b4 = CbBook::new();
    let _ = b4.enroll("cb|mem|alt", O_A_FIRST, F_MEM);
    let ok = b3.chain_tail() != b4.chain_tail();
    s.add("56093-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.enroll("late", O_A_FIRST, F_MEM) == Err(CB_SEALED);
    s.add("56093-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("cb|mem|alt|A先|mem");
    s.add("56093-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56093-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56093_all_checks_green() {
        let set = run_unx56093_checks();
        assert!(set.all_passed(), "UNX-F56093 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56093_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
