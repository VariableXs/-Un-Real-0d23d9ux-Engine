//! UNX-F0249 早传第八步：日志-指纹关联键（0x3BE0 段）
//!
//! 锚点：UNX-F0249-J1 引导指纹/序号/日志缓冲指针三键入
//! Manifest，全链日志可关联。
//! 域：CoRun·UNX-A1-B13（日志关联键面）。
//!
//! 结构：三键模型（引导指纹 / 日志序号 / 日志缓冲指针）+ 键链
//! （同指纹序号单调递增，跨步可关联）+ 关联核（指纹一致 + 序号
//! 链单调 + 指针区段有效）+ 注入核（指纹断裂/序号倒退/指针越界
//! /封账拒写必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3BE0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3BE0;
pub const LK_FP_BREAK: u32 = SEG_BASE + 0x01; // 指纹断裂（同链不同指纹）
pub const LK_SEQ_BACK: u32 = SEG_BASE + 0x02; // 序号倒退
pub const LK_PTR_OOB: u32 = SEG_BASE + 0x03; // 指针越出缓冲区段
pub const LK_SEALED: u32 = SEG_BASE + 0x04; // 关联键已封账拒写
pub const LK_EMPTY: u32 = SEG_BASE + 0x05; // 空键（指纹为零）

/// 日志缓冲区段（物理边界，指针必须落在此段内）。
pub const LOG_BUF_BASE: u64 = 0x0010_0000;
pub const LOG_BUF_SIZE: u64 = 0x0004_0000;

/// 日志关联键：指纹 + 序号 + 缓冲内指针。
#[derive(Clone)]
pub struct LogKey {
    pub boot_fp: u64,
    pub seq: u32,
    pub buf_off: u64,
}

impl LogKey {
    /// 指针落段核。
    pub fn ptr_in_range(&self) -> bool {
        self.buf_off >= LOG_BUF_BASE && self.buf_off < LOG_BUF_BASE + LOG_BUF_SIZE
    }

    /// 非空核（指纹为零 = 空键，拒入链）。
    pub fn nonempty(&self) -> bool {
        self.boot_fp != 0
    }
}

/// 关联链（同引导指纹下序号单调递增）。
pub struct KeyChain {
    pub boot_fp: u64,
    pub keys: Vec<LogKey>,
    pub sealed: bool,
}

impl KeyChain {
    /// 以引导指纹开链。
    pub fn new(boot_fp: u64) -> Self {
        KeyChain { boot_fp, keys: Vec::new(), sealed: false }
    }

    /// 入链（指纹一致 + 非空 + 指针落段 + 序号严格递增）。
    pub fn append(&mut self, k: LogKey) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if !k.nonempty() {
            return Err(LK_EMPTY);
        }
        if k.boot_fp != self.boot_fp {
            return Err(LK_FP_BREAK);
        }
        if !k.ptr_in_range() {
            return Err(LK_PTR_OOB);
        }
        if let Some(last) = self.keys.last() {
            if k.seq <= last.seq {
                return Err(LK_SEQ_BACK);
            }
        }
        self.keys.push(k);
        Ok(())
    }

    /// 全链可关联核：全部条目指纹一致、序号严格递增、指针落段。
    pub fn linkable(&self) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        let mut prev: Option<u32> = None;
        for k in &self.keys {
            if k.boot_fp != self.boot_fp || !k.ptr_in_range() || !k.nonempty() {
                return false;
            }
            if let Some(p) = prev {
                if k.seq <= p {
                    return false;
                }
            }
            prev = Some(k.seq);
        }
        true
    }

    /// 按序号查键（二分语义：序号链单调，线性扫描等价）。
    pub fn find(&self, seq: u32) -> Option<&LogKey> {
        self.keys.iter().find(|k| k.seq == seq)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Manifest 渲染（指纹行 + 每键一行）。
    pub fn render(&self) -> String {
        let mut out = format!("关联链 指纹={:016x} 键数={}\n", self.boot_fp, self.keys.len());
        for k in &self.keys {
            let line = format!(
                "  键 seq={} off=0x{:06x}\n",
                k.seq, k.buf_off
            );
            out.push_str(&line);
        }
        out
    }
}

/// 引导指纹派生（FNV-1a：种子 + 时间基准 + 内存面摘要合成）。
pub fn derive_boot_fp(seed: u64, tsc_base: u64, mem_digest: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in [seed, tsc_base, mem_digest] {
        let mut x = v;
        let mut i = 0;
        while i < 8 {
            h ^= (x & 0xff) as u64;
            h = h.wrapping_mul(0x100000001b3);
            x >>= 8;
            i += 1;
        }
    }
    h
}

/// 标准夹具：指纹派生 + 5 键入链（覆盖首键/末键/边界指针）。
pub fn standard_chain() -> KeyChain {
    let fp = derive_boot_fp(1, 0x1000, 0xdead);
    let mut ch = KeyChain::new(fp);
    let _ = ch.append(LogKey { boot_fp: fp, seq: 0, buf_off: LOG_BUF_BASE });
    let _ = ch.append(LogKey { boot_fp: fp, seq: 1, buf_off: LOG_BUF_BASE + 0x40 });
    let _ = ch.append(LogKey { boot_fp: fp, seq: 2, buf_off: LOG_BUF_BASE + 0x80 });
    let _ = ch.append(LogKey { boot_fp: fp, seq: 3, buf_off: LOG_BUF_BASE + 0x100 });
    let _ = ch.append(LogKey {
        boot_fp: fp,
        seq: 4,
        buf_off: LOG_BUF_BASE + LOG_BUF_SIZE - 1, // 段尾边界
    });
    ch
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let fp = derive_boot_fp(1, 0x1000, 0xdead);
    // 1) 指纹断裂（混入他链指纹）
    let mut ch = KeyChain::new(fp);
    let _ = ch.append(LogKey { boot_fp: fp, seq: 0, buf_off: LOG_BUF_BASE });
    if ch.append(LogKey { boot_fp: fp ^ 1, seq: 1, buf_off: LOG_BUF_BASE }) == Err(LK_FP_BREAK) {
        caught += 1;
    }
    // 2) 序号倒退
    let mut ch2 = KeyChain::new(fp);
    let _ = ch2.append(LogKey { boot_fp: fp, seq: 5, buf_off: LOG_BUF_BASE });
    if ch2.append(LogKey { boot_fp: fp, seq: 5, buf_off: LOG_BUF_BASE + 0x40 }) == Err(LK_SEQ_BACK)
    {
        caught += 1;
    }
    // 3) 指针越下界
    let mut ch3 = KeyChain::new(fp);
    if ch3.append(LogKey { boot_fp: fp, seq: 0, buf_off: LOG_BUF_BASE - 1 }) == Err(LK_PTR_OOB) {
        caught += 1;
    }
    // 4) 指针越上界
    if ch3.append(LogKey {
        boot_fp: fp,
        seq: 1,
        buf_off: LOG_BUF_BASE + LOG_BUF_SIZE,
    }) == Err(LK_PTR_OOB)
    {
        caught += 1;
    }
    // 5) 空键拒
    if ch3.append(LogKey { boot_fp: 0, seq: 2, buf_off: LOG_BUF_BASE }) == Err(LK_EMPTY) {
        caught += 1;
    }
    // 6) 封账拒写
    ch3.seal();
    if ch3.append(LogKey { boot_fp: fp, seq: 9, buf_off: LOG_BUF_BASE }) == Err(LK_SEALED) {
        caught += 1;
    }
    // 7) 漂移核：篡改已入链条目指针后 linkable 转红（双向：干净链绿）
    let mut ch4 = standard_chain();
    if ch4.linkable() {
        caught += 1;
    }
    ch4.keys[2].buf_off = 0xffff_ffff;
    if !ch4.linkable() {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx249_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0249");
    // J1-1 三键入 Manifest（链齐备 + 渲染非空）
    let ch = standard_chain();
    let ok = ch.linkable() && !ch.render().is_empty() && ch.keys.len() == 5;
    s.add("B13-15-三键入链", ok, if ok { "" } else { "三键缺一或链破" });
    // J1-2 全链可关联（指纹一致 + 序号单调 + 指针落段）
    let ok = standard_chain().linkable();
    s.add("B13-16-全链可关联", ok, if ok { "" } else { "关联断裂" });
    // J1-3 按序号检索（首/中/尾三命中）
    let ch = standard_chain();
    let ok = ch.find(0).is_some() && ch.find(3).is_some() && ch.find(4).is_some()
        && ch.find(9).is_none();
    s.add("B13-17-序号检索", ok, if ok { "" } else { "检索失准" });
    // J1-4 指纹派生稳定（同源同指纹，异源异指纹）
    let a = derive_boot_fp(1, 0x1000, 0xdead);
    let b = derive_boot_fp(1, 0x1000, 0xdead);
    let c = derive_boot_fp(2, 0x1000, 0xdead);
    let ok = a == b && a != c;
    s.add("B13-18-指纹派生稳定", ok, if ok { "" } else { "派生漂移" });
    // J1-5 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-19-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx249_all_checks_green() {
        let set = run_unx249_checks();
        assert!(set.all_passed(), "UNX-F0249 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx249_ptr_boundary_exact() {
        let fp = 1u64;
        let mut ch = KeyChain::new(fp);
        assert!(ch
            .append(LogKey { boot_fp: fp, seq: 0, buf_off: LOG_BUF_BASE + LOG_BUF_SIZE - 1 })
            .is_ok());
    }
}
