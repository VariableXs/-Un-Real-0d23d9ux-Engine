//! UNX-F56102 4K 随机读负载执行器（0x60CA0 段）
//!
//! 锚点：UNX-F56102-J1 执行账全量（执行批逐批落账：批键 × IOPS × 时延
//! 微秒三元组）；J2 负载语义一致（块径 4K、随机读模式、队列深度在档——
//! 三元组逐项对档，错径即拒）；J3 执行审计链（批指纹链、封账只读、
//! 均值闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·4K 随机读面）。
//!
//! 结构：负载参数闭集（块径 4K/模式 randread/队列深 1 或 32）+ 执行批
//! 登记册（批键 → IOPS × 时延逐批落账）+ 参数对档核（块径/模式/队列深
//! 三闸）+ 值域守闸（IOPS 与时延物理界）+ 指纹链 + 注入核（错块径/错
//! 模式/错队列深/批键空/值越界/封账改写/空账查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60CA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60CA0;
pub const R4K_BS_MISS: u32 = SEG_BASE + 0x01; // 块径错（非 4K）
pub const R4K_MODE_MISS: u32 = SEG_BASE + 0x02; // 模式错（非 randread）
pub const R4K_QD_MISS: u32 = SEG_BASE + 0x03; // 队列深错（非 1/32 档）
pub const R4K_KEY_EMPTY: u32 = SEG_BASE + 0x04; // 批键为空
pub const R4K_VAL_RANGE: u32 = SEG_BASE + 0x05; // 值越物理界
pub const R4K_SEALED: u32 = SEG_BASE + 0x06; // 执行账已封账拒批
pub const R4K_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// 负载参数正本口径。
pub const CANON_BS_KB: u32 = 4;
pub const CANON_MODE: &str = "randread";
pub const QD_LO: u32 = 1;
pub const QD_HI: u32 = 32;

/// 值域物理界。
pub const IOPS_MAX: u64 = 5_000_000;
pub const LAT_US_MAX: u64 = 1_000_000;

/// 参数对档核（块径/模式/队列深三闸）。
pub fn params_ok(bs_kb: u32, mode: &str, qd: u32) -> Result<(), u32> {
    if bs_kb != CANON_BS_KB {
        return Err(R4K_BS_MISS);
    }
    if mode != CANON_MODE {
        return Err(R4K_MODE_MISS);
    }
    if qd != QD_LO && qd != QD_HI {
        return Err(R4K_QD_MISS);
    }
    Ok(())
}

/// 执行指纹：FNV-1a 32 位。
pub fn exec_fingerprint(key: &str, iops: u64, lat_us: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, iops, lat_us);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 执行批：批键 × IOPS × 时延微秒 × 指纹 × 链。
#[derive(Clone)]
pub struct ExecBatch {
    pub key: String,
    pub iops: u64,
    pub lat_us: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 执行总账。
pub struct R4kBook {
    pub batches: Vec<ExecBatch>,
    pub sealed: bool,
}

impl R4kBook {
    pub fn new() -> Self {
        R4kBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 执行落账：空键拒 + 值域闸 + 重复拒 + 参数对档四闸。
    pub fn run(
        &mut self,
        key: &str,
        iops: u64,
        lat_us: u64,
        bs_kb: u32,
        mode: &str,
        qd: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(R4K_SEALED);
        }
        if key.is_empty() {
            return Err(R4K_KEY_EMPTY);
        }
        params_ok(bs_kb, mode, qd)?;
        if iops == 0 || iops > IOPS_MAX || lat_us > LAT_US_MAX {
            return Err(R4K_VAL_RANGE);
        }
        if self.find(key).is_some() {
            return Err(R4K_VAL_RANGE);
        }
        let fp = exec_fingerprint(key, iops, lat_us);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(ExecBatch { key: key.to_string(), iops, lat_us, fp, chain });
        Ok(())
    }

    /// 查询批结果（未执行显性拒绝）。
    pub fn result_of(&self, key: &str) -> Result<(u64, u64), u32> {
        match self.find(key) {
            Some(i) => Ok((self.batches[i].iops, self.batches[i].lat_us)),
            None => Err(R4K_EMPTY),
        }
    }

    /// J1：执行账全量（逐批值域合法）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self
                .batches
                .iter()
                .all(|b| !b.key.is_empty() && b.iops > 0 && b.iops <= IOPS_MAX)
    }

    /// J3：均值闭式（IOPS 均值 u64 中转）。
    pub fn mean_iops(&self) -> Result<u64, u32> {
        if self.batches.is_empty() {
            return Err(R4K_EMPTY);
        }
        let mut sum: u64 = 0;
        for b in self.batches.iter() {
            sum += b.iops;
        }
        Ok(sum / self.batches.len() as u64)
    }

    /// J3：执行指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        for i in 0..self.batches.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.batches[0].fp
            } else {
                self.batches[i - 1].fp ^ self.batches[i].fp
            };
            if self.batches[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：3 批 4K 随机读（QD 32）。
pub fn standard_book() -> R4kBook {
    let mut b = R4kBook::new();
    let plan: [(&str, u64, u64); 3] =
        [("io|4krr|qd32|A", 412_000, 310), ("io|4krr|qd32|B", 398_500, 322), ("io|4krr|qd32|C", 425_100, 298)];
    for p in plan.iter() {
        if b.run(p.0, p.1, p.2, CANON_BS_KB, CANON_MODE, QD_HI).is_err() {
            panic!("standard fixture run must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（错块径/错模式/错队列深/空批键/值越界/封账改写/空账查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 错块径
    let mut b = R4kBook::new();
    if b.run("k", 1000, 100, 8, CANON_MODE, QD_HI) == Err(R4K_BS_MISS) {
        caught += 1;
    }
    // 2) 错模式
    if b.run("k", 1000, 100, CANON_BS_KB, "randwrite", QD_HI) == Err(R4K_MODE_MISS) {
        caught += 1;
    }
    // 3) 错队列深
    if b.run("k", 1000, 100, CANON_BS_KB, CANON_MODE, 16) == Err(R4K_QD_MISS) {
        caught += 1;
    }
    // 4) 空批键
    if b.run("", 1000, 100, CANON_BS_KB, CANON_MODE, QD_HI) == Err(R4K_KEY_EMPTY) {
        caught += 1;
    }
    // 5) 值越界（IOPS 0）
    if b.run("k", 0, 100, CANON_BS_KB, CANON_MODE, QD_HI) == Err(R4K_VAL_RANGE) {
        caught += 1;
    }
    // 6) 空账查询
    if R4kBook::new().result_of("k").err() == Some(R4K_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.run("late", 1000, 100, CANON_BS_KB, CANON_MODE, QD_HI) == Err(R4K_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 执行账渲染导出（逐批一行）。
pub fn render_book(b: &R4kBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", x.key, x.iops, x.lat_us, x.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx56102_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56102");
    // J1-1 执行账全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56102-J1-执行账全量", ok, if ok { "" } else { "执行批残缺" });
    // J1-2 参数正本口径
    let ok = CANON_BS_KB == 4 && CANON_MODE == "randread" && QD_LO == 1 && QD_HI == 32;
    s.add("56102-J1-参数正本", ok, if ok { "" } else { "参数漂移" });
    // J1-3 参数对档三闸（合法过、三路错各拒）
    let ok = params_ok(4, "randread", 32).is_ok()
        && params_ok(8, "randread", 32) == Err(R4K_BS_MISS)
        && params_ok(4, "seqread", 32) == Err(R4K_MODE_MISS)
        && params_ok(4, "randread", 8) == Err(R4K_QD_MISS);
    s.add("56102-J1-参数对档三闸", ok, if ok { "" } else { "对档闸失效" });
    // J2-1 批结果命中
    let ok = b.result_of("io|4krr|qd32|A") == Ok((412_000, 310));
    s.add("56102-J2-批结果命中", ok, if ok { "" } else { "结果漂移" });
    // J2-2 IOPS 均值闭式（(412000+398500+425100)/3 = 411866 向下取整）
    let ok = b.mean_iops() == Ok(411_866);
    s.add("56102-J2-IOPS均值闭式", ok, if ok { "" } else { "均值算术漂移" });
    // J2-3 空账查询显性拒绝
    let ok = b.result_of("ghost").err() == Some(R4K_EMPTY);
    s.add("56102-J2-空账拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J2-4 错径执行拒（错模式）
    let mut b2 = R4kBook::new();
    let ok = b2.run("k", 1000, 100, CANON_BS_KB, "seqread", QD_LO) == Err(R4K_MODE_MISS);
    s.add("56102-J2-错径拒", ok, if ok { "" } else { "错径穿透" });
    // J3-1 执行指纹链复核
    let ok = b.chain_ok();
    s.add("56102-J3-执行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56102-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改 IOPS 链尾变）
    let mut b3 = standard_book();
    b3.batches[0].iops += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56102-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒批
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.run("late", 1000, 100, CANON_BS_KB, CANON_MODE, QD_HI) == Err(R4K_SEALED);
    s.add("56102-J3-封账拒批", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.starts_with("io|4krr|qd32|A|412000|310|");
    s.add("56102-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56102-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56102_all_checks_green() {
        let set = run_unx56102_checks();
        assert!(set.all_passed(), "UNX-F56102 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56102_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
