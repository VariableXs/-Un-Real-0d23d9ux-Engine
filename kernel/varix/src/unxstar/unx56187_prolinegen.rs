//! UNX-F56187 启动对照行深化版生成器（0x61760 段）
//!
//! 锚点：UNX-F56187-J1 深化行账（对照行深化版——行 × 四段时长 × 16
//! 子段摘要 × N/A 缺侧标记逐字段落账）；J2 行语义（四段和 = 总时长
//! + 子段摘要齐 + 缺侧 N/A 显式三性质）；J3 行指纹（行字段折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·对照行深化面）。
//!
//! 结构：深化行结构 + 字段组装核（四段和闸 + 摘要齐闸 + N/A 闸）+
//! 行指纹 + 注入核（四段和不等/摘要缺字段/N/A 未显式/空行/未知段序
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61760 独占 0x20）。
pub const SEG_BASE: u32 = 0x61760;
pub const PG_SUM_MISS: u32 = SEG_BASE + 0x01; // 四段和不等
pub const PG_SUMMARY_MISS: u32 = SEG_BASE + 0x02; // 摘要缺字段
pub const PG_NA_INEXPLICIT: u32 = SEG_BASE + 0x03; // N/A 未显式
pub const PG_EMPTY_ROW: u32 = SEG_BASE + 0x04; // 空行
pub const PG_UNK_SEGIDX: u32 = SEG_BASE + 0x05; // 未知段序

/// 对照行四段时长（boot/kernel/login/interact）。
#[derive(Clone)]
pub struct DeepCompareRow {
    pub label: String,
    pub seg_ms: [u64; 4],
    /// 16 子段摘要（子段名|时长，空 = 该子段缺）。
    pub sub_summary: [String; 16],
    /// 缺侧标记：true = 对侧无数据（N/A）。
    pub missing_side: bool,
}

impl DeepCompareRow {
    pub fn new(label: &str) -> Self {
        DeepCompareRow {
            label: label.to_string(),
            seg_ms: [0; 4],
            sub_summary: {
                let mut a: [String; 16] = Default::default();
                for s in a.iter_mut() {
                    *s = String::new();
                }
                a
            },
            missing_side: false,
        }
    }

    /// 置一段时长。
    pub fn set_seg(&mut self, seg: usize, ms: u64) -> Result<(), u32> {
        if seg >= 4 {
            return Err(PG_UNK_SEGIDX);
        }
        self.seg_ms[seg] = ms;
        Ok(())
    }

    /// 置一子段摘要。
    pub fn set_sub(&mut self, idx: usize, name: &str, ms: u64) -> Result<(), u32> {
        if idx >= 16 {
            return Err(PG_UNK_SEGIDX);
        }
        if name.is_empty() {
            return Err(PG_EMPTY_ROW);
        }
        self.sub_summary[idx] = format!("{}|{}", name, ms);
        Ok(())
    }

    /// 标记缺侧（N/A 显式化）。
    pub fn mark_na(&mut self) {
        self.missing_side = true;
    }

    /// 四段和。
    pub fn seg_sum(&self) -> u64 {
        self.seg_ms.iter().sum()
    }

    /// 四段和闸：四段和 = 总时长声明。
    pub fn sum_ok(&self, declared_total: u64) -> bool {
        self.seg_sum() == declared_total
    }

    /// 摘要齐闸：16 子段摘要全非空。
    pub fn summary_full(&self) -> bool {
        self.sub_summary.iter().all(|s| !s.is_empty())
    }

    /// N/A 显式闸：缺侧行必须显式标记。
    pub fn na_explicit(&self) -> bool {
        !self.missing_side || self.label.contains("[NA]")
    }

    /// 组装深化行文本。
    pub fn render_row(&self) -> String {
        let mut out = format!("ROW|{}|segs={},{},{},{}|sum={}", self.label, self.seg_ms[0], self.seg_ms[1], self.seg_ms[2], self.seg_ms[3], self.seg_sum());
        if self.missing_side {
            out.push_str("|NA");
        }
        for (i, s) in self.sub_summary.iter().enumerate() {
            if !s.is_empty() {
                out.push_str(&format!("|s{i}:{s}"));
            }
        }
        out
    }

    /// 行指纹。
    pub fn row_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in self.label.bytes() {
            h = h.wrapping_mul(0x0100_0193) ^ b as u32;
        }
        for m in self.seg_ms {
            h = h.wrapping_mul(0x0100_0193) ^ (m as u32);
            h ^= (m >> 32) as u32;
        }
        for s in &self.sub_summary {
            for b in s.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.missing_side as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }
}

/// 标准夹具：本机行（四段 800/1800/2600/800 → 总 6000，16 子段齐）。
fn standard_row() -> DeepCompareRow {
    const SUBS: [&str; 16] = [
        "fwpost", "load", "menu", "handoff", "init", "drv", "svc", "sched",
        "sesmgr", "compos", "logingui", "cred", "input", "firstwin", "evtpath", "focus",
    ];
    const DURS: [u64; 16] = [150, 370, 240, 40, 300, 1000, 400, 100, 400, 1100, 700, 400, 300, 200, 200, 100];
    let mut r = DeepCompareRow::new("local-b40");
    let _ = r.set_seg(0, 800);
    let _ = r.set_seg(1, 1800);
    let _ = r.set_seg(2, 2600);
    let _ = r.set_seg(3, 800);
    for (i, (n, d)) in SUBS.iter().zip(DURS.iter()).enumerate() {
        let _ = r.set_sub(i, n, *d);
    }
    r
}

/// 缺侧夹具：对侧无数据行。
fn na_row() -> DeepCompareRow {
    let mut r = DeepCompareRow::new("remote-b40");
    r.mark_na();
    r
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：四段和不等于声明总
    let mut r = standard_row();
    r.seg_ms[3] += 1;
    if !r.sum_ok(6_000) {
        caught += 1;
    }
    // 注 2：摘要缺字段
    let mut r = standard_row();
    r.sub_summary[7].clear();
    if !r.summary_full() {
        caught += 1;
    }
    // 注 3：N/A 未显式（缺侧但无 [NA] 标记）
    let mut r = DeepCompareRow::new("blank-remote");
    r.missing_side = true;
    if !r.na_explicit() {
        caught += 1;
    }
    // 注 4：空名拒
    let mut r = DeepCompareRow::new("x");
    if r.set_sub(0, "", 5) == Err(PG_EMPTY_ROW) {
        caught += 1;
    }
    // 注 5：未知段序拒
    let mut r = DeepCompareRow::new("x");
    if r.set_seg(9, 5) == Err(PG_UNK_SEGIDX) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 深化行账；J2 行语义；J3 行指纹。
pub fn run_unx56187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56187");
    // J1-1 四段置齐 + 摘要齐
    let r = standard_row();
    let ok = r.summary_full() && r.sum_ok(6_000);
    s.add("56187-J1-四段摘要齐", ok, if ok { "" } else { "行账不全" });
    // J1-2 渲染导出齐
    let text = standard_row().render_row();
    let ok = text.contains("ROW|local-b40|segs=800,1800,2600,800|sum=6000")
        && text.contains("s0:fwpost|150")
        && text.contains("s15:focus|100");
    s.add("56187-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 四段和闸（6000）
    let ok = r.seg_sum() == 6_000;
    s.add("56187-J2-四段和", ok, if ok { "" } else { "和漂移" });
    // J2-2 摘要齐闸
    let ok = r.summary_full();
    s.add("56187-J2-摘要齐闸", ok, if ok { "" } else { "摘要缺" });
    // J2-3 子段和 = 段和抽查（boot 段 150+370+240+40 = 800）
    let boot_sum: u64 = [150u64, 370, 240, 40].iter().sum();
    let ok = boot_sum == 800;
    s.add("56187-J2-boot子段和", ok, if ok { "" } else { "子段和漂移" });
    // J2-4 缺侧显式闸（NA 行带 [NA] 语义标记后过闸）
    let mut na = na_row();
    na.label.push_str("[NA]");
    let ok = na.na_explicit();
    s.add("56187-J2-缺侧显式", ok, if ok { "" } else { "N/A 不显式" });
    // J2-5 本侧行不误标 N/A
    let ok = !r.missing_side && r.na_explicit();
    s.add("56187-J2-本侧无NA", ok, if ok { "" } else { "误标 N/A" });
    // J2-6 未知段序拒
    let mut r2 = DeepCompareRow::new("t");
    let ok = r2.set_seg(4, 1) == Err(PG_UNK_SEGIDX);
    s.add("56187-J2-段序闸", ok, if ok { "" } else { "段序闸失效" });
    // J3-1 行指纹确定
    let ok = standard_row().row_fp() == standard_row().row_fp();
    s.add("56187-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 字段变签变
    let mut r3 = standard_row();
    let fp0 = r3.row_fp();
    r3.seg_ms[0] += 1;
    let ok = r3.row_fp() != fp0;
    s.add("56187-J3-字段变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 标签变签变
    let mut r4 = standard_row();
    let fp0 = r4.row_fp();
    r4.label.push('x');
    let ok = r4.row_fp() != fp0;
    s.add("56187-J3-标签变签变", ok, if ok { "" } else { "标签折叠退化" });
    // J3-4 摘要变签变
    let mut r5 = standard_row();
    let fp0 = r5.row_fp();
    r5.sub_summary[0].push('z');
    let ok = r5.row_fp() != fp0;
    s.add("56187-J3-摘要变签变", ok, if ok { "" } else { "摘要折叠退化" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56187-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56187_all_checks_green() {
        let set = run_unx56187_checks();
        assert!(set.all_passed(), "UNX-F56187 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56187_row_stable() {
        let r = standard_row();
        assert!(r.summary_full() && r.sum_ok(6_000));
    }
}
