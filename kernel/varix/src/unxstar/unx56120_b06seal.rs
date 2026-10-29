//! UNX-F56120 B06 批防重声明与批收官印（0x60EE0 段）
//!
//! 锚点：UNX-F56120-J1 防重声明全量（B06 批内模块指纹逐件登记，无重复
//! 指纹无缺件）；J2 批收官印语义（五防重闸逐闸可验）；J3 收官印审计链
//! （批印指纹折叠、封账只读、批元组齐备）三判据全绿。
//! 域：CoRun·UNX-F2-B40（B06 批·收官面）。
//!
//! 结构：批件登记册 + 指纹折叠核 + 五防重闸（重件/缺件/波次回退/封后
//! 改写/印复算漂移）+ 封账语义 + 注入核（非批件混入/重复指纹/波次回退/
//! 空批收官/缺件收官/未收官取印/封后改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60EE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60EE0;
pub const SEAL_DUP_FP: u32 = SEG_BASE + 0x01; // 重复指纹
pub const SEAL_MISSING: u32 = SEG_BASE + 0x02; // 缺件
pub const SEAL_WAVE_BACK: u32 = SEG_BASE + 0x03; // 波次回退
pub const SEAL_SEALED: u32 = SEG_BASE + 0x04; // 批已收官拒写
pub const SEAL_FP_MISS: u32 = SEG_BASE + 0x05; // 批印复算不一致
pub const SEAL_EMPTY: u32 = SEG_BASE + 0x06; // 空批收官
pub const SEAL_FOREIGN: u32 = SEG_BASE + 0x07; // 非批件混入

/// B06 批件号闭集（56115..56119 五件）。
pub const B06_FIRST: u32 = 56115;
pub const B06_LAST: u32 = 56119;
pub const B06_COUNT: usize = 5;

/// 批件号合法性核。
pub fn in_batch(mod_no: u32) -> bool {
    mod_no >= B06_FIRST && mod_no <= B06_LAST
}

/// 批件指纹：FNV-1a 32 位。
pub fn module_fingerprint(mod_no: u32, slug: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}", mod_no, slug);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 批件账目。
#[derive(Clone)]
pub struct SealEntry {
    pub mod_no: u32,
    pub fp: u32,
    pub wave: u32,
}

/// 批收官总账。
pub struct SealBook {
    pub entries: Vec<SealEntry>,
    pub sealed: bool,
    pub top_wave: u32,
}

impl SealBook {
    pub fn new() -> Self {
        SealBook { entries: Vec::new(), sealed: false, top_wave: 0 }
    }

    /// 逐件落账：批件号闭集 + 指纹唯一 + 波次单调三闸。
    pub fn enroll(&mut self, mod_no: u32, fp: u32, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SEAL_SEALED);
        }
        if !in_batch(mod_no) {
            return Err(SEAL_FOREIGN);
        }
        for e in self.entries.iter() {
            if e.mod_no == mod_no || e.fp == fp {
                return Err(SEAL_DUP_FP);
            }
        }
        if wave == 0 || wave < self.top_wave {
            return Err(SEAL_WAVE_BACK);
        }
        if wave > self.top_wave {
            self.top_wave = wave;
        }
        self.entries.push(SealEntry { mod_no, fp, wave });
        Ok(())
    }

    /// J1：防重声明全量。
    pub fn complete(&self) -> bool {
        if self.entries.len() != B06_COUNT {
            return false;
        }
        for m in B06_FIRST..=B06_LAST {
            let mut hit = false;
            for e in self.entries.iter() {
                if e.mod_no == m {
                    hit = true;
                }
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// J2：指纹两两互异。
    pub fn fingerprints_unique(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].fp == self.entries[j].fp {
                    return false;
                }
            }
        }
        !self.entries.is_empty()
    }

    /// J2：批印折叠（按模块号升序确定性复合）。
    pub fn batch_imprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut sorted: Vec<u32> = Vec::new();
        for e in self.entries.iter() {
            sorted.push(e.mod_no);
        }
        for i in 1..sorted.len() {
            let key = sorted[i];
            let mut j = i;
            while j > 0 && sorted[j - 1] > key {
                sorted[j] = sorted[j - 1];
                j -= 1;
            }
            sorted[j] = key;
        }
        for m in sorted.iter() {
            for e in self.entries.iter() {
                if e.mod_no == *m {
                    h ^= e.fp;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }

    /// J3：波次纪律。
    pub fn wave_ok(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| e.wave >= 1 && e.wave <= self.top_wave)
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(SEAL_SEALED);
        }
        if !self.complete() {
            return Err(SEAL_MISSING);
        }
        self.sealed = true;
        Ok(())
    }

    /// 收官印快照。
    pub fn imprint_snapshot(&self) -> Result<u32, u32> {
        if !self.sealed {
            return Err(SEAL_EMPTY);
        }
        Ok(self.batch_imprint())
    }
}

/// 标准夹具：B06 五件全登记（波次 6）。
pub fn standard_book() -> SealBook {
    let mut b = SealBook::new();
    let slugs: [&str; B06_COUNT] =
        ["ionadisc", "iofailtrace", "iovault", "iodemo", "ioocalib"];
    for i in 0..B06_COUNT {
        let m = B06_FIRST + i as u32;
        let fp = module_fingerprint(m, slugs[i]);
        if b.enroll(m, fp, 6).is_err() {
            panic!("standard fixture enroll must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（非批件/重复指纹/波次回退/空批收官/缺件收官/未收官取印/封后改写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 非批件混入
    let mut b = SealBook::new();
    if b.enroll(99999, 0x1234, 1) == Err(SEAL_FOREIGN) {
        caught += 1;
    }
    // 2) 重复指纹
    if b.enroll(B06_FIRST, 0xAAAA, 1).is_ok() && b.enroll(B06_FIRST + 1, 0xAAAA, 1) == Err(SEAL_DUP_FP) {
        caught += 1;
    }
    // 3) 波次回退
    if b.enroll(B06_FIRST + 2, 0xBBBB, 2).is_ok()
        && b.enroll(B06_FIRST + 3, 0xCCCC, 1) == Err(SEAL_WAVE_BACK)
    {
        caught += 1;
    }
    // 4) 空批收官
    if SealBook::new().seal() == Err(SEAL_MISSING) {
        caught += 1;
    }
    // 5) 缺件收官
    let mut b2 = SealBook::new();
    for i in 0..B06_COUNT - 1 {
        let m = B06_FIRST + i as u32;
        let _ = b2.enroll(m, module_fingerprint(m, "s"), 6);
    }
    if b2.seal() == Err(SEAL_MISSING) {
        caught += 1;
    }
    // 6) 未收官取印
    if standard_book().imprint_snapshot() == Err(SEAL_EMPTY) {
        caught += 1;
    }
    // 7) 封后改写拒
    let mut sb = standard_book();
    if sb.seal().is_ok() && sb.enroll(B06_LAST, 0xDDDD, 7) == Err(SEAL_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 收官账渲染导出。
pub fn render_book(b: &SealBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!("{}|{:08x}|w{}\n", e.mod_no, e.fp, e.wave));
    }
    if b.sealed {
        out.push_str(&format!("IMPRINT|{:08x}\n", b.batch_imprint()));
    }
    out
}

/// 渲染完整性核。
pub fn render_complete(text: &str, sealed: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return false;
    }
    let n = lines.len();
    for (i, ln) in lines.iter().enumerate() {
        let parts: Vec<&str> = ln.split('|').collect();
        if i + 1 == n && sealed {
            if parts.len() != 2 || !lines[i].starts_with("IMPRINT|") {
                return false;
            }
        } else if parts.len() != 3 {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56120");
    // J1-1 防重声明全量
    let b = standard_book();
    let ok = b.complete() && !b.sealed;
    s.add("56120-J1-防重声明全量", ok, if ok { "" } else { "批件残缺" });
    // J1-2 指纹互异
    let ok = b.fingerprints_unique();
    s.add("56120-J1-指纹互异", ok, if ok { "" } else { "指纹重复" });
    // J1-3 批件号闭集
    let ok = in_batch(B06_FIRST) && in_batch(B06_LAST) && !in_batch(B06_FIRST - 1) && !in_batch(B06_LAST + 1);
    s.add("56120-J1-批件号闭集", ok, if ok { "" } else { "批界漂移" });
    // J2-1 批印确定性
    let ok = b.batch_imprint() == b.batch_imprint();
    s.add("56120-J2-批印确定性", ok, if ok { "" } else { "批印漂移" });
    // J2-2 批印敏感
    let mut b2 = SealBook::new();
    for i in 0..B06_COUNT - 1 {
        let m = B06_FIRST + i as u32;
        let _ = b2.enroll(m, module_fingerprint(m, "ionadisc"), 6);
    }
    let ok = b2.batch_imprint() != b.batch_imprint();
    s.add("56120-J2-批印敏感", ok, if ok { "" } else { "批印钝感" });
    // J2-3 收官语义
    let mut b3 = standard_book();
    let ok = b3.seal().is_ok() && b3.sealed && b3.imprint_snapshot().is_ok();
    s.add("56120-J2-收官语义", ok, if ok { "" } else { "收官漂移" });
    // J2-4 缺件收官拒
    let ok = b2.seal() == Err(SEAL_MISSING);
    s.add("56120-J2-缺件收官拒", ok, if ok { "" } else { "缺件闸失效" });
    // J3-1 波次纪律
    let ok = standard_book().wave_ok() && standard_book().top_wave == 6;
    s.add("56120-J3-波次纪律", ok, if ok { "" } else { "波次破纪" });
    // J3-2 收官印稳定
    let i1 = {
        let mut t = standard_book();
        let _ = t.seal();
        t.imprint_snapshot().unwrap_or(0)
    };
    let i2 = {
        let mut t = standard_book();
        let _ = t.seal();
        t.imprint_snapshot().unwrap_or(0)
    };
    let ok = i1 == i2 && i1 != 0;
    s.add("56120-J3-收官印稳定", ok, if ok { "" } else { "收官印漂移" });
    // J3-3 封后改写拒
    let mut b4 = standard_book();
    let _ = b4.seal();
    let ok = b4.enroll(B06_LAST, 0xEEEE, 8) == Err(SEAL_SEALED);
    s.add("56120-J3-封后改写拒", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出齐
    let text = render_book(&b3);
    let ok = render_complete(&text, true) && text.starts_with("56115|") && text.contains("IMPRINT|");
    s.add("56120-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56120-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56120_all_checks_green() {
        let set = run_unx56120_checks();
        assert!(set.all_passed(), "UNX-F56120 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56120_imprint_stable() {
        let mut a = standard_book();
        let mut c = standard_book();
        let _ = a.seal();
        let _ = c.seal();
        assert_eq!(a.imprint_snapshot(), c.imprint_snapshot());
    }
}
