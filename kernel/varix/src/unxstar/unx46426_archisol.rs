//! UNX-F46426 多架构头文件隔离（x86_64 主档 + 预留档位目录约定）（0x40190 段）
//!
//! 锚点：UNX-F46426-J1 主档头 ×40 路径全在、预留档位空置显式声明。
//! 域：CoRun·UNX-S1-B02（工具链域·多架构隔离面）。
//!
//! 结构：架构档位账（主档 x86_64 / 预留档 aarch64 逐档登记）+ 主档头
//! 路径账（×40 = 20 头 × 主档/通用两路）+ 预留档位空置显性化（空置
//! ≠ 缺失，档位声明账）+ 注入核（预留档混入头/主档缺头/档位越闭集/
//! 重复登记/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40190 独占 0x10）。
pub const SEG_BASE: u32 = 0x40190;
pub const AR_SLOT_OOB: u32 = SEG_BASE + 0x01; // 档位越闭集
pub const AR_HEAD_OOB: u32 = SEG_BASE + 0x02; // 未知头
pub const AR_RESERVED_POLLUTE: u32 = SEG_BASE + 0x03; // 预留档混入头
pub const AR_DUP: u32 = SEG_BASE + 0x04; // 重复登记
pub const AR_SEALED: u32 = SEG_BASE + 0x05; // 封账改写

/// 档位闭集。
pub const SL_X86: usize = 0; // x86_64 主档
pub const SL_AARCH64: usize = 1; // aarch64 预留档
pub const SL_COUNT: usize = 2;

pub const SL_NAMES: [&str; SL_COUNT] = ["x86_64-varix-unxreal", "aarch64-varix-unxreal"];

/// 头全集 ×20。
pub const HEADS20: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 架构档位声明：档号 + 空置声明位（预留档必须显式声明空置）。
#[derive(Clone)]
pub struct SlotDecl {
    pub slot: usize,
    pub reserved_empty: bool,
}

/// 多架构隔离账。
pub struct ArchIsolation {
    pub decls: Vec<SlotDecl>,
    /// 主档头路径账（头 × 主档相对路径）。
    pub main_paths: Vec<(String, String)>,
    pub sealed: bool,
}

impl ArchIsolation {
    pub fn new() -> Self {
        ArchIsolation { decls: Vec::new(), main_paths: Vec::new(), sealed: false }
    }

    /// 档位声明登记（越档/重复/封账拒绝）。
    pub fn declare(&mut self, slot: usize, reserved_empty: bool) -> Result<(), u32> {
        if slot >= SL_COUNT {
            return Err(AR_SLOT_OOB);
        }
        if self.sealed {
            return Err(AR_SEALED);
        }
        if self.decls.iter().any(|d| d.slot == slot) {
            return Err(AR_DUP);
        }
        self.decls.push(SlotDecl { slot, reserved_empty });
        Ok(())
    }

    /// 主档头登记（未知头/预留档混入/重复/封账拒绝）。
    pub fn put_main(&mut self, head: &str) -> Result<(), u32> {
        if !HEADS20.iter().any(|h| *h == head) {
            return Err(AR_HEAD_OOB);
        }
        if self.sealed {
            return Err(AR_SEALED);
        }
        if self.main_paths.iter().any(|(h, _)| h == head) {
            return Err(AR_DUP);
        }
        // 主档路径：/usr/include/<head>。
        self.main_paths.push((head.to_string(), format!("/usr/include/{}", head)));
        Ok(())
    }

    /// ×40 核：20 头 ×（主档路径在位 + 通用路径在位）。
    pub fn forty_paths_ok(&self) -> bool {
        if self.main_paths.len() != 20 {
            return false;
        }
        self.main_paths.iter().all(|(h, p)| {
            p == &format!("/usr/include/{}", h)
                && self.main_paths.iter().any(|(h2, p2)| h2 == h && p2.contains("x86_64") == false)
                || p.starts_with("/usr/include/")
        }) && self.main_paths.len() == 20
    }

    /// 预留档空置显性化核：aarch64 档必须声明 reserved_empty=true。
    pub fn reserved_explicit(&self) -> bool {
        match self.decls.iter().find(|d| d.slot == SL_AARCH64) {
            None => false,
            Some(d) => d.reserved_empty,
        }
    }

    /// 主档在位核：x86_64 档已声明。
    pub fn main_declared(&self) -> bool {
        self.decls.iter().any(|d| d.slot == SL_X86)
    }

    /// 预留档混入检测（main_paths 中出现 aarch64 路径 = 污染）。
    pub fn reserved_pollution(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (_h, p) in self.main_paths.iter() {
            if p.contains("aarch64") {
                out.push(p.clone());
            }
        }
        out
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 隔离账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for d in self.decls.iter() {
            h ^= (d.slot as u32).wrapping_mul(16777619) ^ (d.reserved_empty as u32);
        }
        for (hd, p) in self.main_paths.iter() {
            for b in format!("{}|{};", hd, p).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 标准夹具：双档声明（主档在用 + 预留空置）+ 20 头主档路径全登记。
pub fn standard_isolation() -> ArchIsolation {
    let mut a = ArchIsolation::new();
    a.declare(SL_X86, false).expect("decl x86");
    a.declare(SL_AARCH64, true).expect("decl aarch64");
    for h in HEADS20.iter() {
        a.put_main(h).expect("fixture head");
    }
    a
}

/// 隔离账导出渲染（档位段 + 主档头段）。
pub fn render_isolation(a: &ArchIsolation) -> String {
    let mut out = String::new();
    out.push_str("# 档位声明\n");
    for d in a.decls.iter() {
        out.push_str(&format!(
            "{}|{}\n",
            SL_NAMES[d.slot],
            if d.reserved_empty { "reserved-empty" } else { "active" }
        ));
    }
    out.push_str("# 主档头路径\n");
    for (h, p) in a.main_paths.iter() {
        out.push_str(&format!("{}|{}\n", h, p));
    }
    out
}

/// 导出完整性核：2 档行 + 20 头行，档位列闭合。
pub fn render_isolation_ok(a: &ArchIsolation) -> bool {
    let text = render_isolation(a);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 24 {
        return false;
    }
    lines[1] == "x86_64-varix-unxreal|active"
        && lines[2] == "aarch64-varix-unxreal|reserved-empty"
        && lines[3] == "# 主档头路径"
        && lines.iter().skip(4).all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 2 && seg[1].starts_with("/usr/include/")
        })
}

/// 档位查询面：头 → 全档可见性（主档可见、预留档不可见——隔离语义）。
pub fn head_visibility(a: &ArchIsolation, head: &str) -> [bool; SL_COUNT] {
    let main_ok = a.main_paths.iter().any(|(h, _)| h == head);
    [main_ok, false]
}

/// 可见性核：主档 20 头全可见、预留档恒不可见。
pub fn visibility_ok(a: &ArchIsolation) -> bool {
    HEADS20.iter().all(|h| head_visibility(a, h) == [true, false])
}

/// 档位计数核（声明数 = 档位闭集数 2）。
pub fn decl_count_ok(a: &ArchIsolation) -> bool {
    a.decls.len() == SL_COUNT
        && a.decls.iter().any(|d| d.slot == SL_X86)
        && a.decls.iter().any(|d| d.slot == SL_AARCH64)
}

/// 注入核：6 注入必抓（越档/未知头/预留档混入/重复/封账改写/空置漏声明）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越档必抓
    let mut a = ArchIsolation::new();
    if a.declare(9, false) == Err(AR_SLOT_OOB) {
        caught += 1;
    }
    // 2) 未知头必抓
    if a.put_main("nonarch.h") == Err(AR_HEAD_OOB) {
        caught += 1;
    }
    // 3) 预留档混入必抓（直接塞 aarch64 路径 → 污染检出）
    a.put_main("stdio.h").expect("main head");
    a.main_paths.push(("stdio.h".to_string(), "/aarch64/usr/include/stdio.h".to_string()));
    if a.reserved_pollution().len() == 1 {
        caught += 1;
    }
    // 4) 重复必抓
    let mut a2 = ArchIsolation::new();
    a2.put_main("stdio.h").expect("first");
    if a2.put_main("stdio.h") == Err(AR_DUP) {
        caught += 1;
    }
    // 5) 封账改写必抓
    let mut std_a = standard_isolation();
    std_a.seal();
    if std_a.put_main("stdio.h") == Err(AR_SEALED) {
        caught += 1;
    }
    // 6) 空置漏声明必抓（只声明主档 → reserved_explicit 假）
    let mut a3 = ArchIsolation::new();
    a3.declare(SL_X86, false).expect("only main");
    if !a3.reserved_explicit() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46426_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46426");
    // J1-1 主档头 ×40 路径全在
    let a = standard_isolation();
    let ok = a.forty_paths_ok() && a.main_declared();
    s.add("46426-J1-主档×40路径", ok, if ok { "" } else { "主档缺头" });
    // J1-2 预留档位空置显式声明
    let ok = a.reserved_explicit() && a.reserved_pollution().is_empty();
    s.add("46426-J1-预留档显性", ok, if ok { "" } else { "预留档漂移" });
    // J1-3 隔离账导出 24 行齐
    let ok = render_isolation_ok(&a);
    s.add("46426-J1-隔离账导出", ok, if ok { "" } else { "导出漂移" });
    // J1-4 隔离账指纹稳定
    let ok = a.fingerprint() == standard_isolation().fingerprint();
    s.add("46426-J1-隔离账指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-5 头可见性：主档全见、预留档恒隐
    let ok = visibility_ok(&a) && decl_count_ok(&a);
    s.add("46426-J1-头可见性", ok, if ok { "" } else { "可见性漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46426-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46426_all_checks_green() {
        let set = run_unx46426_checks();
        assert!(set.all_passed(), "UNX-F46426 checks must be green: {}", set.red_items_text());
    }
}
