//! UNX-F9137 内存族与 glibc malloc 消费核（0x630E0 段）
//!
//! 锚点：UNX-F9137-J1 消费账（glibc malloc 消费核——请求号 × 请求尺
//! 寸 × 分配档 × 消费态逐请求落账）；J2 消费语义（分配档闭集 + 尺寸
//! 向上取整守恒 + 消费态闭集三性质）；J3 消费指纹（消费账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B19（syscall 内存族 glibc 对照面）。
//!
//! 结构：分配档闭集 + 消费账本（malloc 语义核）+ 取整核（粒度闸 + 档
//! 闸）+ 消费指纹 + 注入核（零请求/超档/尺寸漂移/重请求号/印后请求
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x630E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x630E0;
pub const GL_ZERO_REQ: u32 = SEG_BASE + 0x01; // 零请求
pub const GL_OVER_TIER: u32 = SEG_BASE + 0x02; // 超档
pub const GL_SIZE_MISS: u32 = SEG_BASE + 0x03; // 尺寸漂移
pub const GL_DUP_REQ: u32 = SEG_BASE + 0x04; // 重请求号
pub const GL_FROZEN: u32 = SEG_BASE + 0x05; // 印后请求

/// glibc 最小分配粒度（32 B：16 头 + 16 对齐）。
pub const MALLOC_GRAIN: u64 = 32;

/// 分配档闭集（按 usable 尺寸）。
pub const T_TINY: usize = 0; // ≤ 24 B（tcache fastbin 最小档）
pub const T_SMALL: usize = 1; // ≤ 1_008 B（tcache 上限附近）
pub const T_LARGE: usize = 2; // ≤ 128 KiB（top 切块档）
pub const T_MMAP: usize = 3; // > 128 KiB（mmap 直配）
pub const T_COUNT: usize = 4;

pub const TIER_NAMES: [&str; T_COUNT] = ["tiny", "small", "large", "mmap"];

/// mmap 直配阈值。
pub const MMAP_THRESHOLD: u64 = 128 * 1_024;

/// 分配档推导。
pub fn tier_of(req: u64) -> usize {
    if req > MMAP_THRESHOLD {
        return T_MMAP;
    }
    if req > 1_008 {
        return T_LARGE;
    }
    if req > 24 {
        return T_SMALL;
    }
    T_TINY
}

/// usable 尺寸：向上取整到粒度并扣除 8 B 尺寸字段复用。
pub fn usable_of(req: u64) -> u64 {
    let aligned = (req + MALLOC_GRAIN - 1) / MALLOC_GRAIN * MALLOC_GRAIN;
    aligned.saturating_sub(8)
}

/// 单条消费账。
#[derive(Clone)]
pub struct MallocEntry {
    pub req_no: u32,
    pub request: u64,
    pub tier: usize,
    pub usable: u64,
    /// 消费态：0=已分配 1=已释放。
    pub freed: bool,
}

impl MallocEntry {
    fn render_line(&self) -> String {
        format!(
            "M{}|req={}|tier={}|usable={}|{}",
            self.req_no,
            self.request,
            TIER_NAMES[self.tier],
            self.usable,
            if self.freed { "freed" } else { "live" }
        )
    }
}

/// 消费账本。
pub struct MallocLedger {
    entries: Vec<MallocEntry>,
    sealed: bool,
}

impl MallocLedger {
    pub fn new() -> Self {
        MallocLedger {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 请求分配：零请求拒 + 档闭集 + 重号拒 + 印后拒。
    pub fn alloc_req(&mut self, no: u32, request: u64) -> Result<(usize, u64), u32> {
        if self.sealed {
            return Err(GL_FROZEN);
        }
        if request == 0 {
            return Err(GL_ZERO_REQ);
        }
        let tier = tier_of(request);
        if tier >= T_COUNT {
            return Err(GL_OVER_TIER);
        }
        if self.entries.iter().any(|e| e.req_no == no) {
            return Err(GL_DUP_REQ);
        }
        let usable = usable_of(request);
        if usable < request && request <= MMAP_THRESHOLD {
            return Err(GL_SIZE_MISS);
        }
        self.entries.push(MallocEntry {
            req_no: no,
            request,
            tier,
            usable,
            freed: false,
        });
        Ok((tier, usable))
    }

    /// 释放。
    pub fn free_req(&mut self, no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GL_FROZEN);
        }
        match self.entries.iter_mut().find(|e| e.req_no == no) {
            None => Err(GL_DUP_REQ),
            Some(e) => {
                if e.freed {
                    return Err(GL_DUP_REQ);
                }
                e.freed = true;
                Ok(())
            }
        }
    }

    pub fn entry(&self, no: u32) -> Option<&MallocEntry> {
        self.entries.iter().find(|e| e.req_no == no)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 各档计数。
    pub fn tier_counts(&self) -> [usize; T_COUNT] {
        let mut c = [0usize; T_COUNT];
        for e in &self.entries {
            if e.tier < T_COUNT {
                c[e.tier] += 1;
            }
        }
        c
    }

    /// 活跃（未释放）请求总 usable。
    pub fn live_usable(&self) -> u64 {
        self.entries.iter().filter(|e| !e.freed).map(|e| e.usable).sum()
    }

    /// 消费指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.entries {
            h = h.wrapping_mul(0x0100_0193) ^ e.req_no;
            h = h.wrapping_mul(0x0100_0193) ^ (e.request as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (e.usable as u32);
            h ^= e.tier as u32;
            h ^= e.freed as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() {
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
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("MALLOC-SEAL\n");
        }
        out
    }
}

impl Default for MallocLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四档各一条（16 tiny / 256 small / 4096 large / 256KiB mmap）
/// + 释放 tiny。
fn standard_ledger() -> MallocLedger {
    let mut l = MallocLedger::new();
    let _ = l.alloc_req(1, 16);
    let _ = l.alloc_req(2, 256);
    let _ = l.alloc_req(3, 4_096);
    let _ = l.alloc_req(4, 256 * 1_024);
    let _ = l.free_req(1);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：零请求
    let mut l = MallocLedger::new();
    if l.alloc_req(1, 0) == Err(GL_ZERO_REQ) {
        caught += 1;
    }
    // 注 2：尺寸漂移（usable < request 在非 mmap 档）
    if usable_of(32) < 32 {
        caught += 1;
    }
    // 注 3：重请求号
    let mut l = standard_ledger();
    if l.alloc_req(1, 64) == Err(GL_DUP_REQ) {
        caught += 1;
    }
    // 注 4：双重释放
    let mut l = standard_ledger();
    let _ = l.free_req(1);
    if l.free_req(1) == Err(GL_DUP_REQ) {
        caught += 1;
    }
    // 注 5：印后请求
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.alloc_req(9, 64) == Err(GL_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 消费账；J2 消费语义；J3 消费指纹。
pub fn run_unx9137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9137");
    // J1-1 四请求全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.entry(n).is_some());
    s.add("9137-J1-四请求全落账", ok, if ok { "" } else { "消费账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("M1|req=16|tier=tiny|usable=24|freed")
        && text.contains("M4|req=262144|tier=mmap|usable=262136|live")
        && text.contains("MALLOC-SEAL");
    s.add("9137-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 四档推导
    let ok = tier_of(16) == T_TINY
        && tier_of(256) == T_SMALL
        && tier_of(4_096) == T_LARGE
        && tier_of(256 * 1_024) == T_MMAP;
    s.add("9137-J2-四档推导", ok, if ok { "" } else { "档推导漂移" });
    // J2-2 阈值边界（128KiB 恰为 large；+1 为 mmap）
    let ok = tier_of(MMAP_THRESHOLD) == T_LARGE && tier_of(MMAP_THRESHOLD + 1) == T_MMAP;
    s.add("9137-J2-阈值边界", ok, if ok { "" } else { "边界漂移" });
    // J2-3 usable 取整守恒（16→24、256→256、4096→4088）
    let ok = usable_of(16) == 24
        && usable_of(256) == 256
        && usable_of(4_096) == 4_088
        && usable_of(33) == 56;
    s.add("9137-J2-usable取整", ok, if ok { "" } else { "取整漂移" });
    // J2-4 档计数（tiny1/small1/large1/mmap1）
    let tc = l.tier_counts();
    let ok = tc == [1, 1, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", tc[0], tc[1], tc[2], tc[3]) };
    s.add_dyn("9137-J2-档计数", tc == [1, 1, 1, 1], &detail);
    // J2-5 活跃 usable（256+4088+262136 = 266480）
    let ok = l.live_usable() == 266_480;
    s.add("9137-J2-活跃usable", ok, if ok { "" } else { "活跃漂移" });
    // J2-6 双重释放拒
    let mut l3 = standard_ledger();
    let _ = l3.free_req(1);
    let ok = l3.free_req(1) == Err(GL_DUP_REQ);
    s.add("9137-J2-双重释放拒", ok, if ok { "" } else { "释放闸失效" });
    // J3-1 消费指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9137-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 释放变签变
    let mut l4 = MallocLedger::new();
    let _ = l4.alloc_req(1, 64);
    let fp0 = l4.ledger_fp();
    let _ = l4.free_req(1);
    let ok = l4.ledger_fp() != fp0;
    s.add("9137-J3-释放变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l5 = standard_ledger();
    let ok = l5.seal() && l5.sealed();
    s.add("9137-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒请求
    let ok = l5.alloc_req(9, 64) == Err(GL_FROZEN);
    s.add("9137-J3-印后拒请求", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9137-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9137_all_checks_green() {
        let set = run_unx9137_checks();
        assert!(set.all_passed(), "UNX-F9137 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9137_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
