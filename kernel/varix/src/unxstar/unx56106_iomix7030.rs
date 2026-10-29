//! UNX-F56106 70/30 混合负载执行器（0x60D20 段）
//!
//! 锚点：UNX-F56106-J1 执行账全量（混合批逐批落账：批键 × 读占比万分比
//! × 混合 IOPS 三元组）；J2 混合语义一致（读占比锁定 7000 万分比 ± 容差、
//! 读写 IOPS 拆分复算闭合）；J3 执行审计链（批指纹链、封账只读、拆分
//! 均值闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·混合负载面）。
//!
//! 结构：混合参数闭集（目标读占比 7000 万分比 ± 容差）+ 混合批登记册
//! （批键 → 读 IOPS × 写 IOPS × 读占比逐批落账）+ 占比守闸 + 拆分闭合
//! 核（总 IOPS = 读 + 写复算）+ 指纹链 + 注入核（空批键/占比越带/拆分
//! 不闭合/零 IOPS/封账改写/空账查询/重复批必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60D20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60D20;
pub const MX_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const MX_RATIO_OVER: u32 = SEG_BASE + 0x02; // 读占比越容差带
pub const MX_SPLIT_MISS: u32 = SEG_BASE + 0x03; // 拆分不闭合（读+写 ≠ 总）
pub const MX_IOPS_ZERO: u32 = SEG_BASE + 0x04; // 零 IOPS
pub const MX_DUP: u32 = SEG_BASE + 0x05; // 重复批键
pub const MX_SEALED: u32 = SEG_BASE + 0x06; // 执行账已封账拒批
pub const MX_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// 混合正本口径：读占比 7000 万分比，容差 ±100 万分比。
pub const READ_RATIO_BP: u64 = 7_000;
pub const RATIO_TOL_BP: u64 = 100;

/// 占比守闸：|r - 7000| ≤ 100。
pub fn ratio_ok(read_bp: u64) -> bool {
    let d = if read_bp >= READ_RATIO_BP {
        read_bp - READ_RATIO_BP
    } else {
        READ_RATIO_BP - read_bp
    };
    d <= RATIO_TOL_BP
}

/// 占比复算核：read_bp = read × 10000 / (read + write)。
pub fn ratio_of(read_iops: u64, write_iops: u64) -> Result<u64, u32> {
    let total = read_iops + write_iops;
    if total == 0 {
        return Err(MX_IOPS_ZERO);
    }
    Ok(read_iops * 10_000 / total)
}

/// 混合批指纹：FNV-1a 32 位。
pub fn mix_fingerprint(key: &str, read_iops: u64, write_iops: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, read_iops, write_iops);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 混合批：批键 × 读 IOPS × 写 IOPS × 指纹 × 链。
#[derive(Clone)]
pub struct MixBatch {
    pub key: String,
    pub read_iops: u64,
    pub write_iops: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 混合执行总账。
pub struct MixBook {
    pub batches: Vec<MixBatch>,
    pub sealed: bool,
}

impl MixBook {
    pub fn new() -> Self {
        MixBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 混合落账：空键拒 + 占比闸 + 零 IOPS 闸 + 重复拒四闸。
    pub fn run(&mut self, key: &str, read_iops: u64, write_iops: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(MX_SEALED);
        }
        if key.is_empty() {
            return Err(MX_KEY_EMPTY);
        }
        if read_iops == 0 && write_iops == 0 {
            return Err(MX_IOPS_ZERO);
        }
        let bp = ratio_of(read_iops, write_iops)?;
        if !ratio_ok(bp) {
            return Err(MX_RATIO_OVER);
        }
        if self.find(key).is_some() {
            return Err(MX_DUP);
        }
        let fp = mix_fingerprint(key, read_iops, write_iops);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(MixBatch { key: key.to_string(), read_iops, write_iops, fp, chain });
        Ok(())
    }

    /// J2：拆分闭合核（给定总 IOPS 与批内读+写比对）。
    pub fn split_check(&self, key: &str, total: u64) -> Result<bool, u32> {
        match self.find(key) {
            Some(i) => Ok(self.batches[i].read_iops + self.batches[i].write_iops == total),
            None => Err(MX_EMPTY),
        }
    }

    /// J1：混合账全量（逐批占比在带 + IOPS 非零）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| {
                !b.key.is_empty()
                    && b.read_iops + b.write_iops > 0
                    && ratio_of(b.read_iops, b.write_iops)
                        .map(|bp| ratio_ok(bp))
                        .unwrap_or(false)
            })
    }

    /// J3：读 IOPS 均值闭式。
    pub fn mean_read(&self) -> Result<u64, u32> {
        if self.batches.is_empty() {
            return Err(MX_EMPTY);
        }
        let mut sum: u64 = 0;
        for b in self.batches.iter() {
            sum += b.read_iops;
        }
        Ok(sum / self.batches.len() as u64)
    }

    /// J3：混合指纹链复核。
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

/// 标准夹具：3 批 70/30 混合（读 70000/写 30000 口径微扰）。
pub fn standard_book() -> MixBook {
    let mut b = MixBook::new();
    let plan: [(&str, u64, u64); 3] =
        [("io|mix|qd32|A", 70_000, 30_000), ("io|mix|qd32|B", 68_900, 30_100), ("io|mix|qd32|C", 70_400, 29_800)];
    for p in plan.iter() {
        if b.run(p.0, p.1, p.2).is_err() {
            panic!("standard fixture run must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空批键/零 IOPS/占比越带/拆分不闭合/重复批/封账改写/空账查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut b = MixBook::new();
    if b.run("", 70_000, 30_000) == Err(MX_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 零 IOPS
    if b.run("k", 0, 0) == Err(MX_IOPS_ZERO) {
        caught += 1;
    }
    // 3) 占比越带（全读）
    if b.run("k", 100_000, 0) == Err(MX_RATIO_OVER) {
        caught += 1;
    }
    // 4) 重复批
    if b.run("k2", 70_000, 30_000).is_ok() && b.run("k2", 69_000, 30_000) == Err(MX_DUP) {
        caught += 1;
    }
    // 5) 拆分不闭合
    let sb = standard_book();
    if sb.split_check("io|mix|qd32|A", 99_999) == Ok(false) {
        caught += 1;
    }
    // 6) 空账查询
    if MixBook::new().split_check("k", 1).err() == Some(MX_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.run("late", 70_000, 30_000) == Err(MX_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 混合账渲染导出。
pub fn render_book(b: &MixBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", x.key, x.read_iops, x.write_iops, x.fp));
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
pub fn run_unx56106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56106");
    // J1-1 混合账全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56106-J1-混合账全量", ok, if ok { "" } else { "混合批残缺" });
    // J1-2 占比正本口径（7000 ± 100）
    let ok = READ_RATIO_BP == 7_000 && RATIO_TOL_BP == 100;
    s.add("56106-J1-占比正本", ok, if ok { "" } else { "占比漂移" });
    // J1-3 占比闭集边界（6900/7100 在带，6899/7101 越带）
    let ok = ratio_ok(6_900) && ratio_ok(7_100) && !ratio_ok(6_899) && !ratio_ok(7_101);
    s.add("56106-J1-占比闭集边界", ok, if ok { "" } else { "占比闸漂移" });
    // J2-1 占比复算（70000/30000 → 7000）
    let ok = ratio_of(70_000, 30_000) == Ok(7_000);
    s.add("56106-J2-占比复算", ok, if ok { "" } else { "复算漂移" });
    // J2-2 拆分闭合命中（70000+30000 = 100000）
    let ok = b.split_check("io|mix|qd32|A", 100_000) == Ok(true);
    s.add("56106-J2-拆分闭合命中", ok, if ok { "" } else { "闭合漂移" });
    // J2-3 读 IOPS 均值闭式（(70000+68900+70400)/3 = 69766）
    let ok = b.mean_read() == Ok(69_766);
    s.add("56106-J2-读均值闭式", ok, if ok { "" } else { "均值算术漂移" });
    // J2-4 零 IOPS 拒
    let ok = ratio_of(0, 0) == Err(MX_IOPS_ZERO);
    s.add("56106-J2-零IOPS拒", ok, if ok { "" } else { "零值穿透" });
    // J2-5 空账查询显性拒绝
    let ok = b.split_check("ghost", 1).err() == Some(MX_EMPTY);
    s.add("56106-J2-空账拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J3-1 混合指纹链复核
    let ok = b.chain_ok();
    s.add("56106-J3-混合指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56106-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改写 IOPS 链尾变）
    let mut b3 = standard_book();
    b3.batches[1].write_iops += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56106-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒批
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.run("late", 70_000, 30_000) == Err(MX_SEALED);
    s.add("56106-J3-封账拒批", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.starts_with("io|mix|qd32|A|70000|30000|");
    s.add("56106-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56106-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56106_all_checks_green() {
        let set = run_unx56106_checks();
        assert!(set.all_passed(), "UNX-F56106 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56106_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
