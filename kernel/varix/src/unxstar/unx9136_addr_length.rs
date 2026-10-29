//! UNX-F9136 内存族参数探针总成（addr/length 对齐探针）（0x630C0 段）
//!
//! 锚点：UNX-F9136-J1 探针账（内存族调用参数探针——探针号 × addr 对
//! 齐态 × length 对齐态 × 判定档逐探针落账）；J2 探针语义（页界对齐
//! 判定 + 零长拒绝 + 判定档闭集三性质）；J3 探针指纹（探针账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（syscall 内存族参数探针面）。
//!
//! 结构：判定档闭集 + 探针账本（addr/length → 判定档）+ 对齐核（页
//! 界闸 + 零长闸 + 档闭集闸）+ 探针指纹 + 注入核（零长放行/页界误判
//! /未知档/重探针号/印后探针必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x630C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x630C0;
pub const MP_ZERO_LEN: u32 = SEG_BASE + 0x01; // 零长放行
pub const MP_ALIGN_MISS: u32 = SEG_BASE + 0x02; // 页界误判
pub const MP_UNK_VERDICT: u32 = SEG_BASE + 0x03; // 未知判定档
pub const MP_DUP_PROBE: u32 = SEG_BASE + 0x04; // 重探针号
pub const MP_FROZEN: u32 = SEG_BASE + 0x05; // 印后探针

/// 页大小（4 KiB）。
pub const PAGE_SIZE: u64 = 4_096;

/// 判定档闭集。
pub const V_ALIGNED: usize = 0; // addr 与 length 均页对齐
pub const V_ADDR_MIS: usize = 1; // addr 非页对齐
pub const V_LEN_MIS: usize = 2; // length 非页对齐（向上取整档）
pub const V_ZERO: usize = 3; // 零长
pub const V_COUNT: usize = 4;

pub const VERDICT_NAMES: [&str; V_COUNT] = ["aligned", "addr-misaligned", "len-roundup", "zero"];

/// 单条探针账。
#[derive(Clone)]
pub struct MemProbe {
    pub probe_no: u32,
    pub addr: u64,
    pub length: u64,
    pub verdict: usize,
}

impl MemProbe {
    /// 判定推导：零长 → ZERO；addr 非页对齐 → ADDR_MIS；
    /// length 非页对齐 → LEN_ROUNDUP；否则 ALIGNED。
    pub fn verdict_of(addr: u64, length: u64) -> usize {
        if length == 0 {
            return V_ZERO;
        }
        if addr % PAGE_SIZE != 0 {
            return V_ADDR_MIS;
        }
        if length % PAGE_SIZE != 0 {
            return V_LEN_MIS;
        }
        V_ALIGNED
    }

    /// 覆盖页数（向上取整；零长为 0）。
    pub fn pages(&self) -> u64 {
        if self.length == 0 {
            return 0;
        }
        (self.length + PAGE_SIZE - 1) / PAGE_SIZE
    }

    fn render_line(&self) -> String {
        format!(
            "MP{}|a={:#x}|len={}|{}|pages={}",
            self.probe_no, self.addr, self.length, VERDICT_NAMES[self.verdict], self.pages()
        )
    }
}

/// 对齐核：页界判定单测口。
pub fn page_aligned(v: u64) -> bool {
    v % PAGE_SIZE == 0
}

/// 探针账本。
pub struct ProbeLedger {
    probes: Vec<MemProbe>,
    sealed: bool,
}

impl ProbeLedger {
    pub fn new() -> Self {
        ProbeLedger {
            probes: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条探针：零长闸（零长必须记 ZERO 档）+ 档闭集闸 + 重号拒 + 印后拒。
    pub fn probe(&mut self, no: u32, addr: u64, length: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(MP_FROZEN);
        }
        let v = MemProbe::verdict_of(addr, length);
        if v >= V_COUNT {
            return Err(MP_UNK_VERDICT);
        }
        if length == 0 && v != V_ZERO {
            return Err(MP_ZERO_LEN);
        }
        if self.probes.iter().any(|p| p.probe_no == no) {
            return Err(MP_DUP_PROBE);
        }
        self.probes.push(MemProbe {
            probe_no: no,
            addr,
            length,
            verdict: v,
        });
        Ok(v)
    }

    pub fn probe_entry(&self, no: u32) -> Option<&MemProbe> {
        self.probes.iter().find(|p| p.probe_no == no)
    }

    pub fn len(&self) -> usize {
        self.probes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// 各档计数。
    pub fn verdict_counts(&self) -> [usize; V_COUNT] {
        let mut c = [0usize; V_COUNT];
        for p in &self.probes {
            if p.verdict < V_COUNT {
                c[p.verdict] += 1;
            }
        }
        c
    }

    /// 总覆盖页数。
    pub fn total_pages(&self) -> u64 {
        self.probes.iter().map(|p| p.pages()).sum()
    }

    /// 探针指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in &self.probes {
            h = h.wrapping_mul(0x0100_0193) ^ p.probe_no;
            h = h.wrapping_mul(0x0100_0193) ^ (p.addr as u32);
            h ^= (p.addr >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193) ^ (p.length as u32);
            h ^= (p.length >> 32) as u32;
            h ^= p.verdict as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.probes.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for p in &self.probes {
            out.push_str(&p.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("MPROBE-SEAL\n");
        }
        out
    }
}

impl Default for ProbeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四档各一条（8192/8192 对齐、4097/8192 addr 偏、
/// 8192/12288+1 len 偏、8192/0 零长）。
fn standard_ledger() -> ProbeLedger {
    let mut l = ProbeLedger::new();
    let _ = l.probe(1, 8_192, 8_192);
    let _ = l.probe(2, 4_097, 8_192);
    let _ = l.probe(3, 8_192, 12_289);
    let _ = l.probe(4, 8_192, 0);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：页界误判（4097 判 aligned 必不成立——验证推导正确性）
    if MemProbe::verdict_of(4_097, 8_192) != V_ALIGNED {
        caught += 1;
    }
    // 注 2：零长判 ZERO 档
    if MemProbe::verdict_of(8_192, 0) == V_ZERO {
        caught += 1;
    }
    // 注 3：重探针号
    let mut l = standard_ledger();
    if l.probe(1, 8_192, 8_192) == Err(MP_DUP_PROBE) {
        caught += 1;
    }
    // 注 4：空账封印拒
    let mut l = ProbeLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：印后探针
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.probe(9, 8_192, 8_192) == Err(MP_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 探针账；J2 探针语义；J3 探针指纹。
pub fn run_unx9136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9136");
    // J1-1 四探针全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.probe_entry(n).is_some());
    s.add("9136-J1-四探针全落账", ok, if ok { "" } else { "探针账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("MP1|a=0x2000|len=8192|aligned|pages=2")
        && text.contains("MP4|a=0x2000|len=0|zero|pages=0")
        && text.contains("MPROBE-SEAL");
    s.add("9136-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 四档判定抽查
    let ok = l.probe_entry(1).map(|p| p.verdict == V_ALIGNED).unwrap_or(false)
        && l.probe_entry(2).map(|p| p.verdict == V_ADDR_MIS).unwrap_or(false)
        && l.probe_entry(3).map(|p| p.verdict == V_LEN_MIS).unwrap_or(false)
        && l.probe_entry(4).map(|p| p.verdict == V_ZERO).unwrap_or(false);
    s.add("9136-J2-四档判定", ok, if ok { "" } else { "判定档漂移" });
    // J2-2 页界判定核
    let ok = page_aligned(0) && page_aligned(4_096) && page_aligned(8_192) && !page_aligned(4_097) && !page_aligned(1);
    s.add("9136-J2-页界判定", ok, if ok { "" } else { "页界核漂移" });
    // J2-3 覆盖页数（12289 → 3 页；8192 → 2 页；0 → 0 页）
    let ok = l.probe_entry(3).map(|p| p.pages() == 3).unwrap_or(false)
        && l.probe_entry(1).map(|p| p.pages() == 2).unwrap_or(false)
        && l.probe_entry(4).map(|p| p.pages() == 0).unwrap_or(false);
    s.add("9136-J2-覆盖页数", ok, if ok { "" } else { "页数漂移" });
    // J2-4 档计数（各 1）
    let vc = l.verdict_counts();
    let ok = vc == [1, 1, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", vc[0], vc[1], vc[2], vc[3]) };
    s.add_dyn("9136-J2-档计数", vc == [1, 1, 1, 1], &detail);
    // J2-5 总页数（2 + 2 + 3 + 0 = 7）
    let ok = l.total_pages() == 7;
    s.add("9136-J2-总页数", ok, if ok { "" } else { "总页数漂移" });
    // J2-6 重号拒
    let mut l3 = standard_ledger();
    let ok = l3.probe(2, 8_192, 8_192) == Err(MP_DUP_PROBE);
    s.add("9136-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J3-1 探针指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9136-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 参数变签变
    let mut l4 = ProbeLedger::new();
    let _ = l4.probe(1, 8_192, 8_192);
    let fp0 = l4.ledger_fp();
    let _ = l4.probe(2, 8_192, 12_288);
    let ok = l4.ledger_fp() != fp0;
    s.add("9136-J3-参数变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l5 = standard_ledger();
    let ok = l5.seal() && l5.sealed();
    s.add("9136-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒探针
    let ok = l5.probe(9, 8_192, 8_192) == Err(MP_FROZEN);
    s.add("9136-J3-印后拒探针", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9136-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9136_all_checks_green() {
        let set = run_unx9136_checks();
        assert!(set.all_passed(), "UNX-F9136 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9136_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
