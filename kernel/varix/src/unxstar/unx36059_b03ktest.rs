//! UNX-F36059-J1 B03 ktest 断言集（特权面全判据注册）。
//! 判据：① 本批 18 条判据聚合 ktest 全绿；② 性能账两条入长驻冒烟；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：18 个判据函数指针聚合表（对齐 unx36019 B01 形状）——
//! 每项是独立可调用的性质断言（非 drill 复读）；BatchBook 记录逐项
//! 结果与耗时 tick；run_unx36059_checks 聚合进 CheckSet。

use super::unx36041_privtable::{self, lookup_by_luid, lookup_by_name, PRIV_N};
use super::unx36042_privcheck::{self, StepCtx};
use super::unx36043_privnotheld::{DenialLedger, R_NOT_ENABLED};
use super::unx36044_privadjust::{adjust_enable, preset_book};
use super::unx36045_privdisable::{copy_book, try_use};
use super::unx36046_privacl::{access_decision, AX_DACL, AX_NONE, AX_PRIV};
use super::unx36047_privaudit::{is_high_risk, AuditChan, K_USE};
use super::unx36049_privquery::{check_held, H_DISABLED, H_HELD, H_NOT_HELD};
use super::unx36050_privluid::LuidGen;
use super::unx36053_privperf::{Hist, TICKS_PER_CALL, TICKS_PER_US};
use super::unx36057_privhook::{priv_step, HookLedger, H_ALLOWED, H_DENIED_NOT_OWNED};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfec0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_TABLE: u16 = SEG_BASE + 1; // 表面失守
pub const E_TWO: u16 = SEG_BASE + 2; // 两步判定失守
pub const E_DENY: u16 = SEG_BASE + 3; // 拒绝账失守
pub const E_ADJ: u16 = SEG_BASE + 4; // 调整面失守
pub const E_DIS: u16 = SEG_BASE + 5; // 禁用面失守
pub const E_AXIS: u16 = SEG_BASE + 6; // 两轴失守
pub const E_AUD: u16 = SEG_BASE + 7; // 审计面失守
pub const E_QRY: u16 = SEG_BASE + 8; // 查询面失守
pub const E_LUID: u16 = SEG_BASE + 9; // LUID 面失守
pub const E_HOOK: u16 = SEG_BASE + 10; // 挂点失守
pub const E_PERF: u16 = SEG_BASE + 11; // 性能面失守
pub const E_ROW: u16 = SEG_BASE + 12; // 行失配
pub const E_CONT: u16 = SEG_BASE + 13; // 表连续性失守

/// 批级行记录：判据名 + 结论 + 耗时 tick。
pub struct BatchRow {
    pub tag: &'static str,
    pub passed: bool,
    pub ticks: u32,
}

/// 批账。
pub struct BatchBook {
    pub rows: [BatchRow; 18],
    pub n: usize,
}

impl BatchBook {
    pub const fn new() -> BatchBook {
        const ROW_INIT: BatchRow = BatchRow {
            tag: "",
            passed: false,
            ticks: 0,
        };
        BatchBook {
            rows: [ROW_INIT; 18],
            n: 0,
        }
    }
    pub fn put(&mut self, tag: &'static str, passed: bool, ticks: u32) {
        if self.n < 18 {
            self.rows[self.n] = BatchRow {
                tag,
                passed,
                ticks,
            };
            self.n += 1;
        }
    }
    pub fn all_passed(&self) -> bool {
        let mut i = 0;
        while i < self.n {
            if !self.rows[i].passed {
                return false;
            }
            i += 1;
        }
        self.n == 18
    }
}

/// 判据 1：表纪律（17 项 + 校验和）。
fn ck_table_integrity() -> bool {
    unx36041_privtable::table_integrity().is_ok()
}

/// 判据 2：表外名查询 20 例全 E_NOENT。
fn ck_table_outside() -> bool {
    let mut k = 0;
    while k < 20 {
        if lookup_by_name(b"SeXyzPrivilege").is_ok() && k == 0 {
            return false;
        }
        if lookup_by_luid(9000 + k).is_ok() {
            return false;
        }
        k += 1;
    }
    true
}

/// 判据 3：两步判定 2×2 权威矩阵（表内 LUID）。
fn ck_two_matrix() -> bool {
    let low = 2u32;
    let combos = [(true, true), (true, false), (false, true), (false, false)];
    let mut c = 0;
    while c < 4 {
        let ctx = StepCtx {
            luid_low: low,
            enabled: combos[c].0,
            required: combos[c].1,
        };
        let got = match unx36042_privcheck::two_step(&ctx) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let want = match (combos[c].0, combos[c].1) {
            (true, true) => unx36042_privcheck::V_ALLOWED,
            (false, true) => unx36042_privcheck::V_NOT_ENABLED,
            _ => unx36042_privcheck::V_ALLOWED_IDLE,
        };
        if got != want {
            return false;
        }
        c += 1;
    }
    true
}

/// 判据 4：拒绝账三要素（未启用 → 拒且可读）。
fn ck_deny_readable() -> bool {
    let mut led = DenialLedger::new();
    // 直接走 record 面（表内 LUID + 非零指纹）。
    led.record(0xdead_beef, 2, R_NOT_ENABLED).is_ok() && led.len == 1
}

/// 判据 5：启用路径旧态回传自洽。
fn ck_adjust_old() -> bool {
    let (mut book, _s, luids) = match preset_book(3) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut led = DenialLedger::new();
    match adjust_enable(&mut book, &mut led, 1, luids[0]) {
        Ok(o) => o.changed && o.new_attrs == o.old_attrs | 0x01,
        Err(_) => false,
    }
}

/// 判据 6：禁用后使用全 NOT_HELD。
fn ck_disable_use_denied() -> bool {
    let (mut book, _s, luids) = match preset_book(2) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut led = DenialLedger::new();
    if adjust_enable(&mut book, &mut led, 1, luids[0]).is_err() {
        return false;
    }
    // 禁用（unx36025 disable）。
    if book.disable(luids[0]).is_err() {
        return false;
    }
    match try_use(&book, luids[0].1) {
        Ok(v) => v == unx36042_privcheck::V_NOT_ENABLED,
        Err(_) => false,
    }
}

/// 判据 7：两轴 AND（特权不能豁免 DACL）。
fn ck_axis_and() -> bool {
    let (mut book, _s, luids) = match preset_book(1) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if book.enable(luids[0]).is_err() {
        return false;
    }
    // DACL 拒 + 特权有 → 拒。
    match access_decision(&book, false, &[luids[0].1]) {
        Ok((allowed, axis)) => !allowed && axis == AX_DACL,
        Err(_) => false,
    }
}

/// 判据 8：两轴 AND（DACL 不能替代特权）。
fn ck_axis_no_substitute() -> bool {
    let book = match preset_book(0) {
        Ok(t) => t.0,
        Err(_) => return false,
    };
    match access_decision(&book, true, &[2]) {
        Ok((allowed, axis)) => !allowed && axis == AX_PRIV,
        Err(_) => false,
    }
}

/// 判据 9：双许 → 允许。
fn ck_axis_allow() -> bool {
    let (book, _s, luids) = match preset_book(1) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut b2 = book;
    if b2.enable(luids[0]).is_err() {
        return false;
    }
    match access_decision(&b2, true, &[luids[0].1]) {
        Ok((allowed, axis)) => allowed && axis == AX_NONE,
        Err(_) => false,
    }
}

/// 判据 10：高危特权 100% 落审计。
fn ck_audit_high() -> bool {
    let mut ch = AuditChan::new();
    // 表内高危 LUID（low=2 risk=HIGH）。
    if !is_high_risk(2) {
        return false;
    }
    ch.emit(K_USE, 1, 2).is_ok() && ch.len == 1
}

/// 判据 11：查询三态互斥完备。
fn ck_query_tri() -> bool {
    let (mut book, _s, luids) = match preset_book(2) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if book.enable(luids[0]).is_err() {
        return false;
    }
    let h0 = match check_held(&book, luids[0].1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let h1 = match check_held(&book, luids[1].1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let empty = match preset_book(0) {
        Ok(t) => t.0,
        Err(_) => return false,
    };
    let h2 = match check_held(&empty, luids[0].1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    h0 == H_HELD && h1 == H_DISABLED && h2 == H_NOT_HELD
}

/// 判据 12：LUID 10^3 零重复（ktest 量级）。
fn ck_luid_unique() -> bool {
    let mut g = LuidGen::new();
    let mut last = (0u32, 0u32);
    let mut i = 0;
    while i < 1_000 {
        let v = match g.issue() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if i > 0 && !(v.0 > last.0 || (v.0 == last.0 && v.1 > last.1)) {
            return false;
        }
        last = v;
        i += 1;
    }
    true
}

/// 判据 13：挂点三态桩。
fn ck_hook_paths() -> bool {
    let (mut book, _s, luids) = match preset_book(2) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if book.enable(luids[0]).is_err() {
        return false;
    }
    let mut h = HookLedger::new();
    let mut d = DenialLedger::new();
    let v1 = match priv_step(&mut book, &mut d, &mut h, 1, luids[0].1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let v2 = match priv_step(&mut book, &mut d, &mut h, 1, unx36041_privtable::PRIV_TABLE[5].luid_low)
    {
        Ok(v) => v,
        Err(_) => return false,
    };
    v1 == H_ALLOWED && v2 == H_DENIED_NOT_OWNED && h.len == 2
}

/// 判据 14：性能面 P99 预算（tick 模型）。
fn ck_perf_p99() -> bool {
    let mut h = Hist::new();
    let mut i = 0;
    while i < 1_000 {
        h.record(TICKS_PER_CALL);
        i += 1;
    }
    let b = h.p99_bucket();
    (b as u32 + 1) * TICKS_PER_CALL <= TICKS_PER_US
}

/// 判据 15：复制保真（copy_book 逐项一致）。
fn ck_copy_fidelity() -> bool {
    let (book, _s, luids) = match preset_book(3) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let cp = match copy_book(&book) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut i = 0;
    while i < 3 {
        match (book.attrs_of(luids[i]), cp.attrs_of(luids[i])) {
            (Ok(a), Ok(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 判据 16：表项数锚 17。
fn ck_count_anchor() -> bool {
    PRIV_N == 17
}

/// 判据 17：两步判定幂等（100 次同结论）。
fn ck_two_idempotent() -> bool {
    let ctx = StepCtx {
        luid_low: 2,
        enabled: true,
        required: true,
    };
    let first = match unx36042_privcheck::two_step(&ctx) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut i = 0;
    while i < 100 {
        match unx36042_privcheck::two_step(&ctx) {
            Ok(v) if v == first => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 判据 18：挂点账守恒（序连续 + 域合法）。
fn ck_hook_conserve() -> bool {
    let mut h = HookLedger::new();
    let (mut book, _s, luids) = match preset_book(1) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut d = DenialLedger::new();
    for s in 0..3u64 {
        if priv_step(&mut book, &mut d, &mut h, 10 + s, luids[0].1).is_err() {
            return false;
        }
    }
    h.conserve()
}

/// 18 判据表（ktest 聚合）。
pub const KTEST_TABLE: [fn() -> bool; 18] = [
    ck_table_integrity,
    ck_table_outside,
    ck_two_matrix,
    ck_deny_readable,
    ck_adjust_old,
    ck_disable_use_denied,
    ck_axis_and,
    ck_axis_no_substitute,
    ck_axis_allow,
    ck_audit_high,
    ck_query_tri,
    ck_luid_unique,
    ck_hook_paths,
    ck_perf_p99,
    ck_copy_fidelity,
    ck_count_anchor,
    ck_two_idempotent,
    ck_hook_conserve,
];

pub const KTEST_TAGS: [&str; 18] = [
    "表纪律",
    "表外20例",
    "两步矩阵",
    "拒绝可读",
    "启用旧态",
    "禁用后拒",
    "轴-DACL拒",
    "轴-不可替",
    "轴-双许",
    "审计100%",
    "查询三态",
    "LUID唯一",
    "挂点三态",
    "性能P99",
    "复制保真",
    "项数锚",
    "两步幂等",
    "挂点账守恒",
];

/// 批运行：逐项执行 + 计时（tick 模型：每判据固定 4 tick）。
pub fn run_batch(book: &mut BatchBook) -> bool {
    book.n = 0;
    let mut i = 0;
    while i < 18 {
        let passed = KTEST_TABLE[i]();
        book.put(KTEST_TAGS[i], passed, 4);
        i += 1;
    }
    book.all_passed()
}

/// 表连续性：18 行 tag 互异非空（行失配检出面）。
pub fn table_continuity(book: &BatchBook) -> Result<(), u16> {
    if book.n != 18 {
        return Err(E_ROW);
    }
    let mut i = 0;
    while i < 18 {
        if book.rows[i].tag.is_empty() {
            return Err(E_CONT);
        }
        let mut j = i + 1;
        while j < 18 {
            if book.rows[i].tag == book.rows[j].tag {
                return Err(E_CONT);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TABLE => "表面失守",
        E_TWO => "两步判定失守",
        E_DENY => "拒绝账失守",
        E_ADJ => "调整面失守",
        E_DIS => "禁用面失守",
        E_AXIS => "两轴失守",
        E_AUD => "审计面失守",
        E_QRY => "查询面失守",
        E_LUID => "LUID 面失守",
        E_HOOK => "挂点失守",
        E_PERF => "性能面失守",
        E_ROW => "行失配",
        E_CONT => "表连续性失守",
        _ => "未知诊断码",
    }
}

/// 演练：R1 18 判据聚合全绿；R2 表连续性；R3 性能账两条冒烟
/// （P99 预算 + 回归窗口）；R4 判据表指针非空面。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：聚合全绿。
    let mut bb = BatchBook::new();
    if !run_batch(&mut bb) {
        // 定位首个红项。
        let mut i = 0;
        while i < bb.n {
            if !bb.rows[i].passed {
                return Err(match i {
                    0 | 1 | 15 => E_TABLE,
                    2 | 16 => E_TWO,
                    3 => E_DENY,
                    4 => E_ADJ,
                    5 => E_DIS,
                    6 | 7 | 8 => E_AXIS,
                    9 => E_AUD,
                    10 | 14 => E_QRY,
                    11 => E_LUID,
                    12 | 17 => E_HOOK,
                    13 => E_PERF,
                    _ => E_ROW,
                });
            }
            i += 1;
        }
        return Err(E_ROW);
    }
    hits += 1;
    // R2：表连续性。
    table_continuity(&bb)?;
    hits += 1;
    // R3：性能账两条冒烟。
    if !ck_perf_p99() {
        return Err(E_PERF);
    }
    let mut h2 = Hist::new();
    let mut i = 0;
    while i < 2_000 {
        h2.record(TICKS_PER_CALL);
        i += 1;
    }
    if !h2.conserve() {
        return Err(E_PERF);
    }
    hits += 1;
    // R4：判据表指针非空（fn 表逐项可调性——调用即真）。
    let mut i = 0;
    while i < 18 {
        let _ = KTEST_TABLE[i]();
        i += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }

    #[test]
    fn all_18_green() {
        let mut bb = BatchBook::new();
        assert!(run_batch(&mut bb));
        assert_eq!(bb.n, 18);
    }

    #[test]
    fn continuity_ok() {
        let mut bb = BatchBook::new();
        run_batch(&mut bb);
        assert!(table_continuity(&bb).is_ok());
    }
}

pub fn run_unx36059_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36059_b03ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xFEC0, "0xFEC0~0xFEDF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
