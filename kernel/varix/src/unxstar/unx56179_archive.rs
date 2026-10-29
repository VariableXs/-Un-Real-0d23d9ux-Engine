//! UNX-F56179 syscall 深化批存证归档件（0x61660 段）
//!
//! 锚点：UNX-F56179-J1 存证归档账（syscall 深化批存证逐件归档：存证
//! 键 × 类别 × 归档指纹 × 归档态四元组）；J2 归档闸（类别闭集 + 重复
//! 键拒 + 空键拒 + 归档后只读四闸）；J3 归档印（全部存证齐档可印）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·存证归档面）。
//!
//! 结构：存证类别闭集（聚合账/归因剖析/转单账/复测账/脚本冻结/审计
//! 快照六类）+ 归档器（键唯一索引）+ 归档账 + 归档印 + 注入核（重复
//! 键/未知类/空键/印后档/缺件印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61660 独占 0x20）。
pub const SEG_BASE: u32 = 0x61660;
pub const AR_DUP_KEY: u32 = SEG_BASE + 0x01; // 重复存证键
pub const AR_UNK_CAT: u32 = SEG_BASE + 0x02; // 未知类别
pub const AR_EMPTY_KEY: u32 = SEG_BASE + 0x03; // 空键
pub const AR_SEALED: u32 = SEG_BASE + 0x04; // 印后档
pub const AR_MISS_SEAL: u32 = SEG_BASE + 0x05; // 缺件印

/// 存证类别闭集（六类）。
pub const C_AGG: usize = 0;
pub const C_PROFILE: usize = 1;
pub const C_TICKET: usize = 2;
pub const C_REPLAY: usize = 3;
pub const C_SCRIPT: usize = 4;
pub const C_SNAP: usize = 5;
pub const CAT_COUNT: usize = 6;

/// 类别名渲染。
pub const CAT_NAMES: [&str; CAT_COUNT] =
    ["agg", "profile", "ticket", "replay", "script", "snap"];

/// 归档指纹：键 × 类折叠。
pub fn archive_fp(key: &str, cat: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h = h.wrapping_mul(0x0100_0193) ^ cat as u32;
    h
}

/// 单件存证。
#[derive(Clone)]
pub struct ArchiveEntry {
    pub key: String,
    pub cat: usize,
    pub fp: u32,
    pub state: &'static str,
}

impl ArchiveEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{:08x}|{}",
            self.key, CAT_NAMES[self.cat], self.fp, self.state
        )
    }
}

/// 归档册。
pub struct ArchiveBook {
    entries: Vec<ArchiveEntry>,
    expected: [bool; CAT_COUNT], // 每类至少一件的预期
    sealed: bool,
}

impl ArchiveBook {
    pub fn new() -> Self {
        ArchiveBook {
            entries: Vec::new(),
            expected: [false; CAT_COUNT],
            sealed: false,
        }
    }

    /// 设预期：类别必须归档。
    pub fn expect(&mut self, cat: usize) -> Result<(), u32> {
        if cat >= CAT_COUNT {
            return Err(AR_UNK_CAT);
        }
        self.expected[cat] = true;
        Ok(())
    }

    /// 归档一件：空键拒 + 类闭集 + 重复拒 + 印后拒四闸。
    pub fn archive(&mut self, key: &str, cat: usize) -> Result<u32, u32> {
        if self.sealed {
            return Err(AR_SEALED);
        }
        if key.is_empty() {
            return Err(AR_EMPTY_KEY);
        }
        if cat >= CAT_COUNT {
            return Err(AR_UNK_CAT);
        }
        if self.entries.iter().any(|e| e.key == key) {
            return Err(AR_DUP_KEY);
        }
        let fp = archive_fp(key, cat);
        self.entries.push(ArchiveEntry {
            key: key.to_string(),
            cat,
            fp,
            state: "ARCHIVED",
        });
        Ok(fp)
    }

    pub fn entry(&self, key: &str) -> Option<&ArchiveEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 预期覆盖核：所有 expect 的类别至少一件。
    pub fn coverage_ok(&self) -> bool {
        (0..CAT_COUNT).all(|c| {
            !self.expected[c] || self.entries.iter().any(|e| e.cat == c)
        })
    }

    /// 类别分布。
    pub fn cat_dist(&self) -> [usize; CAT_COUNT] {
        let mut d = [0; CAT_COUNT];
        for e in &self.entries {
            d[e.cat] += 1;
        }
        d
    }

    /// 归档印：预期全覆盖才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.coverage_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("ARCHIVE-SEAL\n");
        }
        out
    }
}

impl Default for ArchiveBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：六类各归一件。
fn standard_book() -> ArchiveBook {
    let mut b = ArchiveBook::new();
    for c in 0..CAT_COUNT {
        let _ = b.expect(c);
    }
    let keys = ["agg-w26", "prof-entry", "tick-p1", "replay-x1", "script-v2", "snap-f3"];
    for (c, k) in keys.iter().enumerate() {
        let _ = b.archive(k, c);
    }
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：重复键
    let mut b = standard_book();
    if b.archive("agg-w26", C_PROFILE) == Err(AR_DUP_KEY) {
        caught += 1;
    }
    // 注 2：未知类
    let mut b = ArchiveBook::new();
    if b.archive("x", 9) == Err(AR_UNK_CAT) {
        caught += 1;
    }
    // 注 3：空键
    let mut b = ArchiveBook::new();
    if b.archive("", C_AGG) == Err(AR_EMPTY_KEY) {
        caught += 1;
    }
    // 注 4：印后档
    let mut b = standard_book();
    let _ = b.seal();
    if b.archive("y", C_AGG) == Err(AR_SEALED) {
        caught += 1;
    }
    // 注 5：缺件印
    let mut b = ArchiveBook::new();
    let _ = b.expect(C_AGG);
    let _ = b.expect(C_PROFILE);
    let _ = b.archive("a", C_AGG);
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 归档账；J2 归档闸；J3 归档印。
pub fn run_unx56179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56179");
    // J1-1 六件全归档
    let b = standard_book();
    let ok = b.len() == 6 && b.coverage_ok();
    s.add("56179-J1-六件全归档", ok, if ok { "" } else { "归档账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("agg-w26|agg|") && text.contains("snap-f3|snap|") && text.contains("ARCHIVED");
    s.add("56179-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 类别分布（各一）
    let ok = standard_book().cat_dist() == [1, 1, 1, 1, 1, 1];
    s.add("56179-J2-类别分布", ok, if ok { "" } else { "分布漂移" });
    // J2-2 归档指纹确定
    let ok = archive_fp("k", C_AGG) == archive_fp("k", C_AGG);
    s.add("56179-J2-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J2-3 类别入签
    let ok = archive_fp("k", C_AGG) != archive_fp("k", C_PROFILE);
    s.add("56179-J2-类别入签", ok, if ok { "" } else { "类别穿透" });
    // J2-4 键入签
    let ok = archive_fp("k1", C_AGG) != archive_fp("k2", C_AGG);
    s.add("56179-J2-键入签", ok, if ok { "" } else { "键穿透" });
    // J2-5 类别名闭集
    let ok = CAT_NAMES[C_AGG] == "agg" && CAT_NAMES[C_SNAP] == "snap";
    s.add("56179-J2-名闭集", ok, if ok { "" } else { "名漂移" });
    // J3-1 归档印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56179-J3-归档印", ok, if ok { "" } else { "归档印失败" });
    // J3-2 印后档拒
    let ok = b.archive("y", C_AGG) == Err(AR_SEALED);
    s.add("56179-J3-印后档拒", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("ARCHIVE-SEAL");
    s.add("56179-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 无预期空册可印（预期空集）
    let mut b = ArchiveBook::new();
    let ok = b.seal() && b.sealed();
    s.add("56179-J3-空预期印", ok, if ok { "" } else { "边界漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56179-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56179_all_checks_green() {
        let set = run_unx56179_checks();
        assert!(set.all_passed(), "UNX-F56179 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56179_fp_stable() {
        assert_eq!(archive_fp("k", C_AGG), archive_fp("k", C_AGG));
    }
}
