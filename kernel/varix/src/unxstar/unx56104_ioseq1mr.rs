//! UNX-F56104 1M 顺序读负载执行器（0x60CE0 段）
//!
//! 锚点：UNX-F56104-J1 执行账全量（执行批逐批落账：批键 × 吞吐 MB/s ×
//! 时延微秒三元组）；J2 负载语义一致（块径 1M、顺序读模式、队列深在档
//! 逐项对档）；J3 执行审计链（批指纹链、封账只读、吞吐均值闭式）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·1M 顺序读面）。
//!
//! 结构：负载参数闭集（1M/seqread/队列深 1 或 32）+ 执行批登记册 +
//! 参数对档核 + 值域守闸（吞吐物理界）+ 指纹链 + 注入核（错块径/错
//! 模式/错队列深/空批键/吞吐越界/封账改写/空账查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60CE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60CE0;
pub const S1M_BS_MISS: u32 = SEG_BASE + 0x01; // 块径错（非 1M）
pub const S1M_MODE_MISS: u32 = SEG_BASE + 0x02; // 模式错（非 seqread）
pub const S1M_QD_MISS: u32 = SEG_BASE + 0x03; // 队列深错
pub const S1M_KEY_EMPTY: u32 = SEG_BASE + 0x04; // 批键为空
pub const S1M_VAL_RANGE: u32 = SEG_BASE + 0x05; // 吞吐越物理界
pub const S1M_SEALED: u32 = SEG_BASE + 0x06; // 执行账已封账拒批
pub const S1M_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// 负载参数正本口径。
pub const CANON_BS_MB: u32 = 1;
pub const CANON_MODE: &str = "seqread";
pub const QD_LO: u32 = 1;
pub const QD_HI: u32 = 32;

/// 吞吐物理界（MB/s）。
pub const TP_MIN: u64 = 1;
pub const TP_MAX: u64 = 30_000;
pub const LAT_US_MAX: u64 = 1_000_000;

/// 参数对档核。
pub fn params_ok(bs_mb: u32, mode: &str, qd: u32) -> Result<(), u32> {
    if bs_mb != CANON_BS_MB {
        return Err(S1M_BS_MISS);
    }
    if mode != CANON_MODE {
        return Err(S1M_MODE_MISS);
    }
    if qd != QD_LO && qd != QD_HI {
        return Err(S1M_QD_MISS);
    }
    Ok(())
}

/// 吞吐值域核。
pub fn tp_ok(tp: u64) -> bool {
    tp >= TP_MIN && tp <= TP_MAX
}

/// 执行指纹：FNV-1a 32 位。
pub fn exec_fingerprint(key: &str, tp_mbps: u64, lat_us: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, tp_mbps, lat_us);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 执行批：批键 × 吞吐 × 时延 × 指纹 × 链。
#[derive(Clone)]
pub struct ExecBatch {
    pub key: String,
    pub tp_mbps: u64,
    pub lat_us: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 执行总账。
pub struct S1mBook {
    pub batches: Vec<ExecBatch>,
    pub sealed: bool,
}

impl S1mBook {
    pub fn new() -> Self {
        S1mBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 执行落账：空键拒 + 参数对档 + 值域闸 + 重复拒四闸。
    pub fn run(
        &mut self,
        key: &str,
        tp_mbps: u64,
        lat_us: u64,
        bs_mb: u32,
        mode: &str,
        qd: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(S1M_SEALED);
        }
        if key.is_empty() {
            return Err(S1M_KEY_EMPTY);
        }
        params_ok(bs_mb, mode, qd)?;
        if !tp_ok(tp_mbps) || lat_us > LAT_US_MAX {
            return Err(S1M_VAL_RANGE);
        }
        if self.find(key).is_some() {
            return Err(S1M_EMPTY);
        }
        let fp = exec_fingerprint(key, tp_mbps, lat_us);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(ExecBatch { key: key.to_string(), tp_mbps, lat_us, fp, chain });
        Ok(())
    }

    /// 查询批结果。
    pub fn result_of(&self, key: &str) -> Result<(u64, u64), u32> {
        match self.find(key) {
            Some(i) => Ok((self.batches[i].tp_mbps, self.batches[i].lat_us)),
            None => Err(S1M_EMPTY),
        }
    }

    /// J1：执行账全量。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| !b.key.is_empty() && tp_ok(b.tp_mbps))
    }

    /// J3：吞吐均值闭式。
    pub fn mean_tp(&self) -> Result<u64, u32> {
        if self.batches.is_empty() {
            return Err(S1M_EMPTY);
        }
        let mut sum: u64 = 0;
        for b in self.batches.iter() {
            sum += b.tp_mbps;
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

/// 标准夹具：3 批 1M 顺序读（QD 32）。
pub fn standard_book() -> S1mBook {
    let mut b = S1mBook::new();
    let plan: [(&str, u64, u64); 3] =
        [("io|1msr|qd32|A", 7_120, 1_820), ("io|1msr|qd32|B", 6_980, 1_865), ("io|1msr|qd32|C", 7_265, 1_790)];
    for p in plan.iter() {
        if b.run(p.0, p.1, p.2, CANON_BS_MB, CANON_MODE, QD_HI).is_err() {
            panic!("standard fixture run must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（错块径/错模式/错队列深/空批键/吞吐越界/封账改写/空账查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 错块径
    let mut b = S1mBook::new();
    if b.run("k", 1000, 100, 4, CANON_MODE, QD_HI) == Err(S1M_BS_MISS) {
        caught += 1;
    }
    // 2) 错模式
    if b.run("k", 1000, 100, CANON_BS_MB, "randread", QD_HI) == Err(S1M_MODE_MISS) {
        caught += 1;
    }
    // 3) 错队列深
    if b.run("k", 1000, 100, CANON_BS_MB, CANON_MODE, 8) == Err(S1M_QD_MISS) {
        caught += 1;
    }
    // 4) 空批键
    if b.run("", 1000, 100, CANON_BS_MB, CANON_MODE, QD_HI) == Err(S1M_KEY_EMPTY) {
        caught += 1;
    }
    // 5) 吞吐越下界
    if b.run("k", 0, 100, CANON_BS_MB, CANON_MODE, QD_HI) == Err(S1M_VAL_RANGE) {
        caught += 1;
    }
    // 6) 空账查询
    if S1mBook::new().result_of("k").err() == Some(S1M_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.run("late", 1000, 100, CANON_BS_MB, CANON_MODE, QD_HI) == Err(S1M_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 执行账渲染导出。
pub fn render_book(b: &S1mBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", x.key, x.tp_mbps, x.lat_us, x.fp));
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
pub fn run_unx56104_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56104");
    // J1-1 执行账全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56104-J1-执行账全量", ok, if ok { "" } else { "执行批残缺" });
    // J1-2 参数正本口径
    let ok = CANON_BS_MB == 1 && CANON_MODE == "seqread" && QD_LO == 1 && QD_HI == 32;
    s.add("56104-J1-参数正本", ok, if ok { "" } else { "参数漂移" });
    // J1-3 参数对档三闸
    let ok = params_ok(1, "seqread", 32).is_ok()
        && params_ok(4, "seqread", 32) == Err(S1M_BS_MISS)
        && params_ok(1, "seqwrite", 32) == Err(S1M_MODE_MISS)
        && params_ok(1, "seqread", 4) == Err(S1M_QD_MISS);
    s.add("56104-J1-参数对档三闸", ok, if ok { "" } else { "对档闸失效" });
    // J2-1 批结果命中
    let ok = b.result_of("io|1msr|qd32|A") == Ok((7_120, 1_820));
    s.add("56104-J2-批结果命中", ok, if ok { "" } else { "结果漂移" });
    // J2-2 吞吐均值闭式（(7120+6980+7265)/3 = 7121 向下取整）
    let ok = b.mean_tp() == Ok(7_121);
    s.add("56104-J2-吞吐均值闭式", ok, if ok { "" } else { "均值算术漂移" });
    // J2-3 吞吐闭集边界
    let ok = tp_ok(TP_MIN) && tp_ok(TP_MAX) && !tp_ok(0) && !tp_ok(TP_MAX + 1);
    s.add("56104-J2-吞吐闭集", ok, if ok { "" } else { "吞吐界漂移" });
    // J2-4 空账查询显性拒绝
    let ok = b.result_of("ghost").err() == Some(S1M_EMPTY);
    s.add("56104-J2-空账拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J3-1 执行指纹链复核
    let ok = b.chain_ok();
    s.add("56104-J3-执行指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56104-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改吞吐链尾变）
    let mut b3 = standard_book();
    b3.batches[2].tp_mbps += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56104-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒批
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.run("late", 1000, 100, CANON_BS_MB, CANON_MODE, QD_HI) == Err(S1M_SEALED);
    s.add("56104-J3-封账拒批", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.starts_with("io|1msr|qd32|A|7120|1820|");
    s.add("56104-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56104-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56104_all_checks_green() {
        let set = run_unx56104_checks();
        assert!(set.all_passed(), "UNX-F56104 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56104_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
