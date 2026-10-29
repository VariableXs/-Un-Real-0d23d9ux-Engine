//! UNX-F46423 头文件集版本一致性（头内容与 glibc 钉版版本对应表）（0x40160 段）
//!
//! 锚点：UNX-F46423-J1 头-版本对应表 ×40 项与 C5 钉版账零漂移。
//! 域：CoRun·UNX-S1-B02（工具链域·头版本面）。
//!
//! 结构：头-版本对应账（40 项：头 × 引入 glibc 版本 × 特性宏 × 钉版
//! 一致位）+ 钉版联动核（C5 钉版版本为正本，对应表逐项对齐）+ 版本比较
//! 器（三段数字序）+ 注入核（越版本/未知头/重复/封账改写/漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40160 独占 0x10）。
pub const SEG_BASE: u32 = 0x40160;
pub const HV_VER_BAD: u32 = SEG_BASE + 0x01; // 版本格式非法
pub const HV_HEAD_OOB: u32 = SEG_BASE + 0x02; // 未知头
pub const HV_DUP: u32 = SEG_BASE + 0x03; // 重复项
pub const HV_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const HV_DRIFT: u32 = SEG_BASE + 0x05; // 与钉版账漂移

/// C5 钉版 glibc 版本（正本）。
pub const PINNED_GLIBC: &str = "2.38.1";

/// ×40 对应表的 20 头 × 2 记录（引入版 + 特性宏验证版）。
pub const HEAD_UNIVERSE: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 版本格式核（主.次.修订 三段数字）。
pub fn ver_ok(v: &str) -> bool {
    let p: Vec<&str> = v.split('.').collect();
    p.len() == 3 && p.iter().all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// 版本比较器：-1/0/1（三段逐段数字比较）。
pub fn ver_cmp(a: &str, b: &str) -> Result<i32, u32> {
    if !ver_ok(a) || !ver_ok(b) {
        return Err(HV_VER_BAD);
    }
    let pa: Vec<u32> = a.split('.').map(|s| s.parse::<u32>().unwrap_or(0)).collect();
    let pb: Vec<u32> = b.split('.').map(|s| s.parse::<u32>().unwrap_or(0)).collect();
    for i in 0..3 {
        if pa[i] < pb[i] {
            return Ok(-1);
        }
        if pa[i] > pb[i] {
            return Ok(1);
        }
    }
    Ok(0)
}

/// 头-版本对应项。
#[derive(Clone)]
pub struct HeadVer {
    pub head: String,
    pub introduced: String,
    /// 特性验证宏（如 __USE_XOPEN2K8）。
    pub feature_macro: String,
    /// 记录类别：0=引入版，1=宏验证版（每头两条）。
    pub kind: u8,
}

/// 头-版本对应账（×40 项）。
pub struct HeadVerBook {
    pub rows: Vec<HeadVer>,
    pub sealed: bool,
}

impl HeadVerBook {
    pub fn new() -> Self {
        HeadVerBook { rows: Vec::new(), sealed: false }
    }

    /// 登记项（未知头/坏版本/空宏/重复/封账全拒）。
    pub fn put(&mut self, head: &str, introduced: &str, macro_name: &str, kind: u8) -> Result<(), u32> {
        if !HEAD_UNIVERSE.iter().any(|h| *h == head) {
            return Err(HV_HEAD_OOB);
        }
        if !ver_ok(introduced) {
            return Err(HV_VER_BAD);
        }
        if macro_name.is_empty() {
            return Err(HV_VER_BAD);
        }
        if kind > 1 {
            return Err(HV_HEAD_OOB);
        }
        if self.sealed {
            return Err(HV_SEALED);
        }
        if self
            .rows
            .iter()
            .any(|r| r.head == head && r.kind == kind)
        {
            return Err(HV_DUP);
        }
        self.rows.push(HeadVer {
            head: head.to_string(),
            introduced: introduced.to_string(),
            feature_macro: macro_name.to_string(),
            kind,
        });
        Ok(())
    }

    /// ×40 全登记核（20 头 × 2 类）。
    pub fn full40(&self) -> bool {
        self.rows.len() == 40
            && HEAD_UNIVERSE.iter().all(|h| {
                self.rows.iter().any(|r| r.head == *h && r.kind == 0)
                    && self.rows.iter().any(|r| r.head == *h && r.kind == 1)
            })
    }

    /// J1 零漂移核：每项引入版 ≤ 钉版版（头必须先于钉版存在）。
    pub fn align_pinned(&self) -> bool {
        self.rows
            .iter()
            .all(|r| ver_cmp(&r.introduced, PINNED_GLIBC) == Ok(-1) || ver_cmp(&r.introduced, PINNED_GLIBC) == Ok(0))
    }

    /// 漂移清单（引入版 > 钉版版的项）。
    pub fn drifts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in self.rows.iter() {
            if ver_cmp(&r.introduced, PINNED_GLIBC) == Ok(1) {
                out.push(format!("{}@{}", r.head, r.introduced));
            }
        }
        out
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in self.rows.iter() {
            for b in format!("{}|{}|{}|{};", r.head, r.introduced, r.feature_macro, r.kind).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 特性宏闭集核：宏名以 __VARIX_ 或 __USE_ 打头（命名宪法）。
pub fn macro_name_ok(name: &str) -> bool {
    name.starts_with("__VARIX_") || name.starts_with("__USE_")
}

/// 对应表导出渲染（头|引入版|宏|类 四列 ×40 行）。
pub fn render_table(b: &HeadVerBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}\n",
            r.head,
            r.introduced,
            r.feature_macro,
            if r.kind == 0 { "intro" } else { "macro" }
        ));
    }
    out
}

/// 渲染完整性核（40 行 × 四段，类别列闭合）。
pub fn render_table_ok(b: &HeadVerBook) -> bool {
    let text = render_table(b);
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == 40
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 4 && (seg[3] == "intro" || seg[3] == "macro") && ver_ok(seg[1])
        })
}

/// 版本提升审计：对应表版本分布直方图（轮次 → 头数）。
pub fn version_histogram(b: &HeadVerBook) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for r in b.rows.iter() {
        match out.iter_mut().find(|(v, _)| *v == r.introduced) {
            Some((_v, c)) => *c += 1,
            None => out.push((r.introduced.clone(), 1)),
        }
    }
    out.sort_by(|a, b2| ver_cmp(&a.0, &b2.0).unwrap_or(0).cmp(&0));
    out
}

/// 版本提升审计核：全部版本 ≤ 钉版且直方图计数守恒（Σ = 40）。
pub fn histogram_ok(b: &HeadVerBook) -> bool {
    let hist = version_histogram(b);
    hist.iter().map(|(_v, c)| *c).sum::<usize>() == 40
        && hist.iter().all(|(v, _c)| ver_cmp(v, PINNED_GLIBC) != Ok(1))
}

/// 版本提升账：钉版版本升级事件（旧版 → 新版，对应表全量复核标记）。
pub struct UpgradeLedger {
    pub rows: Vec<(String, String, bool)>, // (旧版, 新版, 对应表复核完成)
}

impl UpgradeLedger {
    pub fn new() -> Self {
        UpgradeLedger { rows: Vec::new() }
    }

    /// 升级登记（新 > 旧才可登记；重复升级拒绝）。
    pub fn register(&mut self, old: &str, new: &str) -> Result<(), u32> {
        if ver_ok(old) && ver_ok(new) && ver_cmp(old, new) == Ok(-1)
            && !self.rows.iter().any(|(o, n, _)| o == old && n == new)
        {
            self.rows.push((old.to_string(), new.to_string(), false));
            Ok(())
        } else {
            Err(HV_VER_BAD)
        }
    }

    /// 复核落账。
    pub fn mark_reviewed(&mut self, old: &str, new: &str) -> Result<(), u32> {
        match self.rows.iter_mut().find(|(o, n, _)| o == old && n == new) {
            None => Err(HV_HEAD_OOB),
            Some(r) => {
                r.2 = true;
                Ok(())
            }
        }
    }

    /// 全复核核。
    pub fn all_reviewed(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|(_o, _n, r)| *r)
    }
}
/// 标准夹具：×40 项（20 头 × 引入/宏验证，版本族 2.26~2.38 全 ≤ 钉版）。
pub fn standard_book() -> HeadVerBook {
    let mut b = HeadVerBook::new();
    let vers = [
        "2.26.0", "2.27.0", "2.28.0", "2.29.0", "2.30.0",
        "2.31.0", "2.31.0", "2.32.0", "2.32.0", "2.33.0",
        "2.33.0", "2.34.0", "2.34.0", "2.35.0", "2.35.0",
        "2.36.0", "2.36.0", "2.37.0", "2.37.0", "2.38.0",
    ];
    for (i, h) in HEAD_UNIVERSE.iter().enumerate() {
        b.put(h, vers[i], &format!("__VARIX_HEAD_{}", i), 0).expect("fixture kind0");
        b.put(h, vers[i], &format!("__VARIX_MACRO_{}", i), 1).expect("fixture kind1");
    }
    b
}

/// 注入核：6 注入必抓（坏版本/未知头/重复/封账改写/漂移检出/版本比较错）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 坏版本必抓
    let mut b = HeadVerBook::new();
    if b.put("stdio.h", "2.38", "M", 0) == Err(HV_VER_BAD) {
        caught += 1;
    }
    // 2) 未知头必抓
    if b.put("nonstd.h", "2.38.0", "M", 0) == Err(HV_HEAD_OOB) {
        caught += 1;
    }
    // 3) 重复必抓
    b.put("stdio.h", "2.26.0", "M0", 0).expect("first");
    if b.put("stdio.h", "2.27.0", "M1", 0) == Err(HV_DUP) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_b = standard_book();
    std_b.seal();
    if std_b.put("stdio.h", "2.26.0", "MX", 0) == Err(HV_SEALED) {
        caught += 1;
    }
    // 5) 漂移检出必抓（引入版 > 钉版）
    let mut b2 = standard_book();
    b2.rows[0].introduced = "2.39.0".to_string();
    if b2.drifts().len() == 1 && !b2.align_pinned() {
        caught += 1;
    }
    // 6) 版本比较器正确性（2.38.0 < 2.38.1 < 2.39.0）
    if ver_cmp("2.38.0", "2.38.1") == Ok(-1)
        && ver_cmp("2.38.1", "2.39.0") == Ok(-1)
        && ver_cmp("2.38.1", "2.38.1") == Ok(0)
    {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46423_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46423");
    // J1-1 ×40 项全登记
    let b = standard_book();
    let ok = b.full40();
    s.add("46423-J1-×40对应表", ok, if ok { "" } else { "对应表不全" });
    // J1-2 与 C5 钉版零漂移
    let ok = b.align_pinned() && b.drifts().is_empty();
    s.add("46423-J1-钉版零漂移", ok, if ok { "" } else { "版本漂移" });
    // J1-3 版本比较器 ×3 断言
    let ok = ver_cmp(PINNED_GLIBC, "2.38.0") == Ok(1)
        && ver_cmp(PINNED_GLIBC, "2.38.1") == Ok(0)
        && ver_cmp("2.26.0", PINNED_GLIBC) == Ok(-1);
    s.add("46423-J1-版本比较器", ok, if ok { "" } else { "比较器漂移" });
    // J1-4 账指纹稳定
    let ok = b.fingerprint() == standard_book().fingerprint();
    s.add("46423-J1-账指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-5 特性宏命名宪法 + 对应表导出 40 行齐
    let ok = b.rows.iter().all(|r| macro_name_ok(&r.feature_macro))
        && render_table_ok(&b);
    s.add("46423-J1-宏命名与导出", ok, if ok { "" } else { "导出漂移" });
    // J1-6 版本直方图计数守恒
    let ok = histogram_ok(&b);
    s.add("46423-J1-版本直方图", ok, if ok { "" } else { "直方图漂移" });
    // J1-7 版本提升账：2.37→2.38 登记 + 复核全落
    let mut ul = UpgradeLedger::new();
    let r1 = ul.register("2.37.0", "2.38.1");
    ul.mark_reviewed("2.37.0", "2.38.1").expect("review");
    let ok = r1.is_ok()
        && ul.all_reviewed()
        && ul.register("2.39.0", "2.38.0") == Err(HV_VER_BAD);
    s.add("46423-J1-版本提升账", ok, if ok { "" } else { "提升账漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46423-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46423_all_checks_green() {
        let set = run_unx46423_checks();
        assert!(set.all_passed(), "UNX-F46423 checks must be green: {}", set.red_items_text());
    }
}
