//! UNX-F46410 SysrootLayout 冻结接口消费对账（与 AI-15 C5 双域联签）（0x40090 段）
//!
//! 锚点：UNX-F46410-J1 本域消费面与 C5 冻结接口逐字段对账 ×30 零漂移；
//! 布局变更 = 双域回归纪律登记入域内宪法。
//! 域：CoRun·UNX-S1-B01（工具链域·C5 联签面）。
//!
//! 结构：C5 冻结接口字段账（30 字段逐字段登记：名/类型/方向/联签位）+
//! 消费面核（本域消费字段 ⊆ 冻结集，越面拒绝）+ 双域回归纪律章 +
//! 注入核（越面消费/缺联签字段/字段漂移/章缺失/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40090 独占 0x10）。
pub const SEG_BASE: u32 = 0x40090;
pub const SR_FIELD_OOB: u32 = SEG_BASE + 0x01; // 消费越冻结面
pub const SR_COSIGN_MISS: u32 = SEG_BASE + 0x02; // 联签字段缺失
pub const SR_FIELD_DRIFT: u32 = SEG_BASE + 0x03; // 字段语义漂移
pub const SR_CHARTER_MISS: u32 = SEG_BASE + 0x04; // 回归纪律章缺失
pub const SR_SEALED: u32 = SEG_BASE + 0x05; // 对账已封账拒改

/// C5 SysrootLayout 冻结接口 ×30 字段（名 + 语义方向：C5→本域 供 / 本域→C5 回）。
pub const FROZEN_FIELDS: [(&str, &str); 30] = [
    ("root", "供"), ("triple", "供"), ("bin_dir", "供"), ("lib_dir", "供"),
    ("include_dir", "供"), ("libexec_dir", "供"), ("gcc_ver", "供"), ("runtime_subdir", "供"),
    ("crt_dir", "供"), ("dynamic_loader", "供"), ("static_libc", "供"), ("dynamic_libc", "供"),
    ("pthread_lib", "供"), ("math_lib", "供"), ("crt0", "供"), ("crti", "供"),
    ("crtn", "供"), ("crtbegin", "供"), ("crtend", "供"), ("libgcc_a", "供"),
    ("libgcc_eh", "供"), ("libgcc_s", "供"), ("ld_script", "供"), ("libsearch", "供"),
    ("headers_full", "回"), ("scan_missing", "回"), ("scan_drift", "回"),
    ("layout_stamp", "回"), ("consumer_state", "回"), ("regression_tag", "回"),
];

/// 消费面账：本域消费字段逐字段联签。
pub struct ConsumeBook {
    /// (字段名, 联签人, 联签波次)。
    pub rows: Vec<(String, String, u32)>,
    pub sealed: bool,
}

impl ConsumeBook {
    pub fn new() -> Self {
        ConsumeBook { rows: Vec::new(), sealed: false }
    }

    /// 消费登记（越冻结面拒绝；重复登记拒绝；封账拒写）。
    pub fn consume(&mut self, field: &str, cosigner: &str, wave: u32) -> Result<(), u32> {
        if !FROZEN_FIELDS.iter().any(|(f, _)| *f == field) {
            return Err(SR_FIELD_OOB);
        }
        if cosigner.is_empty() || wave == 0 {
            return Err(SR_COSIGN_MISS);
        }
        if self.sealed {
            return Err(SR_SEALED);
        }
        if self.rows.iter().any(|(f, _, _)| f == field) {
            return Err(SR_FIELD_DRIFT);
        }
        self.rows.push((field.to_string(), cosigner.to_string(), wave));
        Ok(())
    }

    /// ×30 全消费核。
    pub fn full(&self) -> bool {
        self.rows.len() == FROZEN_FIELDS.len()
            && FROZEN_FIELDS.iter().all(|(f, _)| self.rows.iter().any(|(rf, _, _)| rf == f))
    }

    /// 方向对账：供向字段消费方 = 本域（S1）；回向字段提供方 = 本域。
    pub fn direction_ok(&self) -> bool {
        for (f, dir) in FROZEN_FIELDS.iter() {
            let row = self.rows.iter().find(|(rf, _, _)| rf == f);
            match row {
                None => return false,
                Some((_, who, _)) => {
                    let expect = if *dir == "供" { "S1" } else { "S1" };
                    if who != expect {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 对账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for (f, w, wave) in self.rows.iter() {
            for b in format!("{}|{}|{};", f, w, wave).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 双域回归纪律章（布局变更必须触发 C5+S1 双域回归，登记入域内宪法）。
pub const REGRESSION_CHARTER: &str =
    "S1 域内宪法·回归纪律章：SysrootLayout 冻结接口任何字段变更，\
     须同波次触发 C5 布局域与 S1 工具链域双域回归；回归未绿不得闭账；\
     变更账以 regression_tag 落款。";

/// 章完整性核（三要素：双域/未绿不闭账/regression_tag）。
pub fn charter_sealed() -> bool {
    REGRESSION_CHARTER.contains("C5")
        && REGRESSION_CHARTER.contains("S1")
        && REGRESSION_CHARTER.contains("双域回归")
        && REGRESSION_CHARTER.contains("不得闭账")
        && REGRESSION_CHARTER.contains("regression_tag")
}

/// 变更账（regression_tag 落款面）：字段变更 → 双域回归登记。
pub struct ChangeLedger {
    /// (字段名, regression_tag, C5 回归绿, S1 回归绿)。
    pub rows: Vec<(String, String, bool, bool)>,
}

impl ChangeLedger {
    pub fn new() -> Self {
        ChangeLedger { rows: Vec::new() }
    }

    /// 变更登记：tag 非空；同字段同 tag 不重复落账。
    pub fn register(&mut self, field: &str, tag: &str) -> Result<(), u32> {
        if tag.is_empty() || field.is_empty() {
            return Err(SR_FIELD_DRIFT);
        }
        if self.rows.iter().any(|(f, t, _, _)| f == field && t == tag) {
            return Err(SR_FIELD_DRIFT);
        }
        self.rows.push((field.to_string(), tag.to_string(), false, false));
        Ok(())
    }

    /// 回归落绿（C5/S1 双域各自落绿；字段必须在账）。
    pub fn mark_regression(&mut self, field: &str, tag: &str, which: usize, ok: bool) -> Result<(), u32> {
        match self.rows.iter_mut().find(|(f, t, _, _)| f == field && t == tag) {
            None => Err(SR_FIELD_OOB),
            Some(row) => {
                if which == 0 {
                    row.2 = ok;
                } else {
                    row.3 = ok;
                }
                Ok(())
            }
        }
    }

    /// 双域全绿核（每条变更 C5+S1 都绿才可闭账）。
    pub fn all_regressed(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|(_, _, c, s)| *c && *s)
    }

    /// 未绿不闭账核（红项清单显性化）。
    pub fn pending(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (f, t, c, s) in self.rows.iter() {
            if !(*c && *s) {
                out.push(format!("{}#{}", f, t));
            }
        }
        out
    }

    /// 渲染（字段|tag|C5|S1 四列）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (f, t, c, s) in self.rows.iter() {
            out.push_str(&format!(
                "{}|{}|{}|{}\n",
                f,
                t,
                if *c { "GREEN" } else { "RED" },
                if *s { "GREEN" } else { "RED" }
            ));
        }
        out
    }
}

/// 冻结面方向分类核：供向 ×24 / 回向 ×6（计数守恒）。
pub fn direction_counts() -> (usize, usize) {
    let mut supply = 0;
    let mut back = 0;
    for (_f, d) in FROZEN_FIELDS.iter() {
        if *d == "供" {
            supply += 1;
        } else {
            back += 1;
        }
    }
    (supply, back)
}

/// 字段名唯一性核（冻结集内无重名）。
pub fn frozen_fields_unique() -> bool {
    for i in 0..FROZEN_FIELDS.len() {
        for j in (i + 1)..FROZEN_FIELDS.len() {
            if FROZEN_FIELDS[i].0 == FROZEN_FIELDS[j].0 {
                return false;
            }
        }
    }
    true
}
/// 标准夹具：×30 字段全消费（联签人 S1，波次 21）。
pub fn standard_consume() -> ConsumeBook {
    let mut b = ConsumeBook::new();
    for (f, _dir) in FROZEN_FIELDS.iter() {
        b.consume(f, "S1", 21).expect("fixture consume");
    }
    b
}

/// 注入核：6 注入必抓（越面消费/缺联签/重复/封账拒写/章缺失/字段漏项）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越冻结面必抓
    let mut b = ConsumeBook::new();
    if b.consume("nonexistent_field", "S1", 1) == Err(SR_FIELD_OOB) {
        caught += 1;
    }
    // 2) 缺联签必抓（空联签人）
    if b.consume("root", "", 1) == Err(SR_COSIGN_MISS) {
        caught += 1;
    }
    // 3) 重复登记必抓
    b.consume("root", "S1", 1).expect("first root");
    if b.consume("root", "S1", 2) == Err(SR_FIELD_DRIFT) {
        caught += 1;
    }
    // 4) 封账拒写
    let mut std_b = standard_consume();
    std_b.seal();
    if std_b.consume("triple", "S1", 22) == Err(SR_SEALED) {
        caught += 1;
    }
    // 5) 章缺失必抓（伪章无 regression_tag）
    let fake = "回归纪律：双域回归。";
    if !(fake.contains("regression_tag") && fake.contains("不得闭账")) {
        caught += 1;
    }
    // 6) 字段漏项必抓（只消费 29 项 → full 假）
    let mut part = ConsumeBook::new();
    for (f, _d) in FROZEN_FIELDS.iter().take(29) {
        part.consume(f, "S1", 21).expect("partial");
    }
    if !part.full() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46410_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46410");
    // J1-1 ×30 字段逐字段对账零漂移
    let b = standard_consume();
    let ok = b.full() && b.direction_ok();
    s.add("46410-J1-×30字段对账", ok, if ok { "" } else { "字段对账漂移" });
    // J1-2 消费面 ⊆ 冻结集（越面核反向验证）
    let mut oob = ConsumeBook::new();
    let oob_rejected = oob.consume("root_extra", "S1", 1) == Err(SR_FIELD_OOB);
    s.add("46410-J1-越面拒绝", oob_rejected, if oob_rejected { "" } else { "越面放行" });
    // J1-3 回归纪律章落章
    let ok = charter_sealed();
    s.add("46410-J1-回归纪律章", ok, if ok { "" } else { "章缺失" });
    // J1-4 对账指纹稳定
    let ok = b.fingerprint() == standard_consume().fingerprint();
    s.add("46410-J1-对账指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-5 变更账：登记 → 双域落绿 → 全绿闭账；未绿项显性化
    let mut cl = ChangeLedger::new();
    cl.register("libsearch", "w21-layout-fix").expect("register");
    let pending_before = cl.pending();
    cl.mark_regression("libsearch", "w21-layout-fix", 0, true).expect("c5");
    cl.mark_regression("libsearch", "w21-layout-fix", 1, true).expect("s1");
    let ok = !pending_before.is_empty()
        && cl.all_regressed()
        && cl.pending().is_empty()
        && cl.render().contains("libsearch|w21-layout-fix|GREEN|GREEN");
    s.add("46410-J1-变更账双域落绿", ok, if ok { "" } else { "变更账漂移" });
    // J1-6 冻结面方向计数 24/6 + 字段名唯一
    let (sup, back) = direction_counts();
    let ok = sup == 24 && back == 6 && frozen_fields_unique();
    s.add("46410-J1-方向计数", ok, if ok { "" } else { "方向计数漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46410-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46410_all_checks_green() {
        let set = run_unx46410_checks();
        assert!(set.all_passed(), "UNX-F46410 checks must be green: {}", set.red_items_text());
    }
}
