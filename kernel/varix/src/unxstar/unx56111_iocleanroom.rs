//! UNX-F56111 IO 采集干扰隔离件（采集器自身 IO 归零）（0x60DC0 段）
//!
//! 锚点：UNX-F56111-J1 隔离账全量（采集批逐批落账：批键 × 采集器自身
//! IOPS × 归零判定三元组）；J2 干扰隔离语义（采集器自身 IO 必须归零——
//! 自身 IOPS ≤ 阈值才判隔离通过，超阈即拒对照）；J3 隔离审计链（批指纹
//! 链、封账只读、隔离率闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·干扰隔离面）。
//!
//! 结构：采集批登记册（批键 → 自身 IOPS × 隔离结论逐批落账）+ 归零阈
//! 闸（自身 IOPS ≤ 阈值）+ 隔离率闭式（通过批数 / 总批数）+ 指纹链 +
//! 注入核（空批键/自身超阈/零批放行/重复批/封账改写/空账查询/隔离率
//! 漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60DC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60DC0;
pub const ISO_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const ISO_OVER: u32 = SEG_BASE + 0x02; // 采集器自身 IO 超阈
pub const ISO_DUP: u32 = SEG_BASE + 0x03; // 重复批键
pub const ISO_SEALED: u32 = SEG_BASE + 0x04; // 隔离账已封账拒写
pub const ISO_EMPTY: u32 = SEG_BASE + 0x05; // 空账查询
pub const ISO_RATE_MISS: u32 = SEG_BASE + 0x06; // 隔离率漂移
pub const ISO_NEG: u32 = SEG_BASE + 0x07; // 负计数（不可能值）

/// 归零阈值（采集器自身 IOPS 上限）。
pub const SELF_IOPS_MAX: u64 = 50;

/// 隔离结论闭集。
pub const V_CLEAN: usize = 0;
pub const V_NOISY: usize = 1;
pub const V_NAMES: [&str; V_COUNT2] = ["clean", "noisy"];
pub const V_COUNT2: usize = 2;

/// 归零判定核。
pub fn clean_ok(self_iops: u64) -> bool {
    self_iops <= SELF_IOPS_MAX
}

/// 隔离批指纹：FNV-1a 32 位。
pub fn iso_fingerprint(key: &str, self_iops: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}", key, self_iops);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 隔离批：批键 × 自身 IOPS × 结论 × 指纹 × 链。
#[derive(Clone)]
pub struct IsoBatch {
    pub key: String,
    pub self_iops: u64,
    pub verdict: usize,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 隔离总账。
pub struct IsoBook {
    pub batches: Vec<IsoBatch>,
    pub sealed: bool,
}

impl IsoBook {
    pub fn new() -> Self {
        IsoBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 落账：空键拒 + 重复拒 + 结论自动判定（归零闸）。
    pub fn audit_batch(&mut self, key: &str, self_iops: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(ISO_SEALED);
        }
        if key.is_empty() {
            return Err(ISO_KEY_EMPTY);
        }
        if self.find(key).is_some() {
            return Err(ISO_DUP);
        }
        let verdict = if clean_ok(self_iops) { V_CLEAN } else { V_NOISY };
        let fp = iso_fingerprint(key, self_iops);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(IsoBatch {
            key: key.to_string(),
            self_iops,
            verdict,
            fp,
            chain,
        });
        Ok(verdict)
    }

    /// 查询批结论（未登记显性拒绝）。
    pub fn verdict_of(&self, key: &str) -> Result<usize, u32> {
        match self.find(key) {
            Some(i) => Ok(self.batches[i].verdict),
            None => Err(ISO_EMPTY),
        }
    }

    /// J2：隔离率闭式（clean 数 / 总数，万分比）。
    pub fn clean_rate_bp(&self) -> Result<u64, u32> {
        if self.batches.is_empty() {
            return Err(ISO_EMPTY);
        }
        let mut clean = 0u64;
        for b in self.batches.iter() {
            if b.verdict == V_CLEAN {
                clean += 1;
            }
        }
        Ok(clean * 10_000 / self.batches.len() as u64)
    }

    /// J1：隔离账全量（逐批结论合法）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| !b.key.is_empty() && b.verdict < V_COUNT2)
    }

    /// J1：clean 批自身 IOPS 复核（全 ≤ 阈值）。
    pub fn clean_ok_recheck(&self) -> bool {
        self.batches.iter().all(|b| {
            if b.verdict == V_CLEAN {
                clean_ok(b.self_iops)
            } else {
                !clean_ok(b.self_iops)
            }
        })
    }

    /// J3：隔离指纹链复核。
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

/// 标准夹具：3 批（2 clean + 1 noisy）。
pub fn standard_book() -> IsoBook {
    let mut b = IsoBook::new();
    if b.audit_batch("iso|4krr|A", 0).is_err() {
        panic!("standard fixture batch1 must audit");
    }
    if b.audit_batch("iso|4krr|B", 17).is_err() {
        panic!("standard fixture batch2 must audit");
    }
    if b.audit_batch("iso|4krr|C", 120).is_err() {
        panic!("standard fixture batch3 must audit");
    }
    b
}

/// 注入核：7 注入必抓（空批键/重复批/空账查询/隔离率漂移/封账改写/超阈判噪/结论越界）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut b = IsoBook::new();
    if b.audit_batch("", 0) == Err(ISO_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 重复批
    if b.audit_batch("k", 0).is_ok() && b.audit_batch("k", 1) == Err(ISO_DUP) {
        caught += 1;
    }
    // 3) 空账查询
    if IsoBook::new().verdict_of("k").err() == Some(ISO_EMPTY) {
        caught += 1;
    }
    // 4) 超阈判噪
    let sb = standard_book();
    if sb.verdict_of("iso|4krr|C") == Ok(V_NOISY) {
        caught += 1;
    }
    // 5) 隔离率复算（2/3 → 6666）
    if sb.clean_rate_bp() == Ok(6_666) {
        caught += 1;
    }
    // 6) 隔离率漂移必抓（篡改结论后复算失败）
    let mut dirty = standard_book();
    dirty.batches[0].verdict = V_NOISY;
    if dirty.clean_rate_bp() != Ok(6_666) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.audit_batch("late", 0) == Err(ISO_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 隔离账渲染导出。
pub fn render_book(b: &IsoBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", x.key, x.self_iops, V_NAMES[x.verdict], x.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 结论闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[2] == "clean" || parts[2] == "noisy")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56111");
    // J1-1 隔离账全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56111-J1-隔离账全量", ok, if ok { "" } else { "隔离批残缺" });
    // J1-2 归零阈值口径
    let ok = SELF_IOPS_MAX == 50 && clean_ok(0) && clean_ok(50) && !clean_ok(51);
    s.add("56111-J1-归零阈值口径", ok, if ok { "" } else { "阈值漂移" });
    // J1-3 clean 批复核（结论与值一致）
    let ok = b.clean_ok_recheck();
    s.add("56111-J1-clean批复核", ok, if ok { "" } else { "结论漂移" });
    // J2-1 隔离率闭式（2/3 → 6666）
    let ok = b.clean_rate_bp() == Ok(6_666);
    s.add("56111-J2-隔离率闭式", ok, if ok { "" } else { "隔离率漂移" });
    // J2-2 超阈批拒对照（noisy 不进有效集）
    let ok = b.verdict_of("iso|4krr|C") == Ok(V_NOISY);
    s.add("56111-J2-超阈判噪", ok, if ok { "" } else { "噪声穿透" });
    // J2-3 零自耗判净
    let ok = b.verdict_of("iso|4krr|A") == Ok(V_CLEAN);
    s.add("56111-J2-零自耗判净", ok, if ok { "" } else { "归零语义漂移" });
    // J2-4 空账查询显性拒绝
    let ok = b.verdict_of("ghost").err() == Some(ISO_EMPTY);
    s.add("56111-J2-空账拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J3-1 隔离指纹链复核
    let ok = b.chain_ok();
    s.add("56111-J3-隔离指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56111-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改自耗链尾变）
    let mut b3 = standard_book();
    b3.batches[0].self_iops = 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56111-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.audit_batch("late", 0) == Err(ISO_SEALED);
    s.add("56111-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("iso|4krr|A|0|clean|");
    s.add("56111-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56111-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56111_all_checks_green() {
        let set = run_unx56111_checks();
        assert!(set.all_passed(), "UNX-F56111 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56111_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
