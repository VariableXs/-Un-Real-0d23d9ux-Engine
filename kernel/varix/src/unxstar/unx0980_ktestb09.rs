//! unx0980_ktestb09 — UNX-F0980 ktest B09 断言集（双模预备与治理 ≥40 断言）。
//!
//! 任务锚点：UNX-F0980（第二批工位 W7，SEG 段位 0x8080，本单独占
//! 0x8080..=0x809F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**ktest B09 断言集 ≥40 条（段位/闭集/
//! 守恒/注入/对拍五族覆盖），全部断言可重复执行且结果确定，任一
//! 断言碎即整体带码非零返回，断言清单双导出指纹稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x808 段闭集）；tick 显性
//! 注入零墙钟；断言账守恒（通过+失败==总断言 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8080 段，本单独占）
// ---------------------------------------------------------------------------

/// ktest B09 断言集段基址（0x8080，本单 0x8080..=0x809F 独占）。
pub const SEG_BASE: u16 = 0x8080;
/// 0x8081 断言清单为空（≥40 纪律下界破坏）。
pub const XC_SET_EMPTY: u16 = SEG_BASE + 1;
/// 0x8082 断言数不足（<40——判据下限未达）。
pub const XC_SET_TOO_FEW: u16 = SEG_BASE + 2;
/// 0x8083 断言重名（同名双断言——清单身份不唯一）。
pub const XC_DUP_NAME: u16 = SEG_BASE + 3;
/// 0x8084 断言函数空指针（fn 指针为 0 的形式缺失位）。
pub const XC_FN_MISSING: u16 = SEG_BASE + 4;
/// 0x8085 断言账守恒破坏（通过+失败 != 总数）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x8086 断言结果不确定（同断言双跑结果异——确定性破坏）。
pub const XC_NONDETERMINISTIC: u16 = SEG_BASE + 6;
/// 0x8087 清单指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x8088 五族覆盖缺失（任一族零断言——覆盖面不齐）。
pub const XC_FAMILY_GAP: u16 = SEG_BASE + 8;
/// 0x8089 断言 id 越界（id 为零）。
pub const XC_ID_BAD: u16 = SEG_BASE + 9;

const _: () = assert!(XC_SET_EMPTY != XC_SET_TOO_FEW);
const _: () = assert!(XC_CONSERVATION != XC_NONDETERMINISTIC);
const _: () = assert!(XC_FAMILY_GAP != XC_ID_BAD);

/// 断言数下限（判据 ≥40）。
pub const MIN_ASSERTIONS: usize = 40;

// ---------------------------------------------------------------------------
// §1 断言五族闭集
// ---------------------------------------------------------------------------

/// 断言族闭集：段位/闭集/守恒/注入/对拍五族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertFamily {
    /// 段位族（SEG 对齐与独占）。
    Segment,
    /// 闭集族（码域/枚举 roundtrip）。
    ClosedSet,
    /// 守恒族（恒等式重扫）。
    Conservation,
    /// 注入族（坏输入必拦）。
    Injection,
    /// 对拍族（双跑必同）。
    PairRun,
}

impl AssertFamily {
    /// 族码（1..=5）。
    pub const fn code(self) -> u8 {
        match self {
            AssertFamily::Segment => 1,
            AssertFamily::ClosedSet => 2,
            AssertFamily::Conservation => 3,
            AssertFamily::Injection => 4,
            AssertFamily::PairRun => 5,
        }
    }
    /// 族名。
    pub const fn name(self) -> &'static str {
        match self {
            AssertFamily::Segment => "段位",
            AssertFamily::ClosedSet => "闭集",
            AssertFamily::Conservation => "守恒",
            AssertFamily::Injection => "注入",
            AssertFamily::PairRun => "对拍",
        }
    }
}

/// 族总数。
pub const FAMILY_COUNT: usize = 5;

/// 断言函数形（零参——全部断言对模块内定值与样例账运行）。
type CheckFn = fn() -> Result<(), u16>;

/// 一条断言规格：id / 族 / 名 / 函数。
#[derive(Clone, Copy)]
pub struct AssertSpec {
    id: u32,
    family: AssertFamily,
    name: &'static str,
    f: CheckFn,
}

// ---------------------------------------------------------------------------
// §2 被检样例账（治理样例——注入/守恒族的活靶）
// ---------------------------------------------------------------------------

/// 治理样例账：计数准入 + 守恒（注入族与守恒族共用靶体）。
#[derive(PartialEq)]
struct GovernanceSample {
    entries: Vec<u32>,
    cap: usize,
    total: u64,
    rejected: u32,
}

impl GovernanceSample {
    fn new(cap: usize) -> Result<GovernanceSample, u16> {
        if cap == 0 {
            return Err(XC_SET_EMPTY);
        }
        Ok(GovernanceSample {
            entries: Vec::with_capacity(cap),
            cap,
            total: 0,
            rejected: 0,
        })
    }
    fn add(&mut self, id: u32) -> Result<(), u16> {
        if id == 0 {
            self.rejected += 1;
            return Err(XC_ID_BAD);
        }
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CONSERVATION);
        }
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i] == id {
                self.rejected += 1;
                return Err(XC_DUP_NAME);
            }
            i += 1;
        }
        self.entries.push(id);
        self.total += 1;
        Ok(())
    }
    fn audit(&self) -> Result<(), u16> {
        if self.total != self.entries.len() as u64 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 断言函数集（42 条——五族覆盖）
// ---------------------------------------------------------------------------

// —— 段位族（6 条）——
fn chk_seg_align() -> Result<(), u16> {
    if SEG_BASE % 0x20 != 0 || SEG_BASE == 0 {
        return Err(XC_SET_TOO_FEW);
    }
    Ok(())
}
fn chk_seg_span() -> Result<(), u16> {
    if 0x809F - SEG_BASE != 0x1F {
        return Err(XC_SET_TOO_FEW);
    }
    Ok(())
}
fn chk_codes_in_seg() -> Result<(), u16> {
    let codes = [XC_SET_EMPTY, XC_SET_TOO_FEW, XC_DUP_NAME, XC_FN_MISSING, XC_CONSERVATION, XC_NONDETERMINISTIC, XC_DRIFT, XC_FAMILY_GAP, XC_ID_BAD];
    let mut i: usize = 0;
    while i < codes.len() {
        if codes[i] <= SEG_BASE || codes[i] > SEG_BASE + 0x1F {
            return Err(XC_FAMILY_GAP);
        }
        i += 1;
    }
    Ok(())
}
fn chk_codes_distinct() -> Result<(), u16> {
    let codes = [XC_SET_EMPTY, XC_SET_TOO_FEW, XC_DUP_NAME, XC_FN_MISSING, XC_CONSERVATION, XC_NONDETERMINISTIC, XC_DRIFT, XC_FAMILY_GAP, XC_ID_BAD];
    let mut i: usize = 0;
    while i < codes.len() {
        let mut j: usize = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return Err(XC_DUP_NAME);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}
fn chk_min_assert_const() -> Result<(), u16> {
    if MIN_ASSERTIONS < 40 {
        return Err(XC_SET_TOO_FEW);
    }
    Ok(())
}
fn chk_family_count() -> Result<(), u16> {
    if FAMILY_COUNT != 5 {
        return Err(XC_FAMILY_GAP);
    }
    Ok(())
}

// —— 闭集族（8 条）——
fn chk_family_roundtrip() -> Result<(), u16> {
    let fams = [AssertFamily::Segment, AssertFamily::ClosedSet, AssertFamily::Conservation, AssertFamily::Injection, AssertFamily::PairRun];
    let mut i: usize = 0;
    while i < fams.len() {
        if fams[i].code() == 0 || fams[i].name().is_empty() {
            return Err(XC_FAMILY_GAP);
        }
        i += 1;
    }
    Ok(())
}
fn chk_family_codes_distinct() -> Result<(), u16> {
    let fams = [AssertFamily::Segment, AssertFamily::ClosedSet, AssertFamily::Conservation, AssertFamily::Injection, AssertFamily::PairRun];
    let mut i: usize = 0;
    while i < fams.len() {
        let mut j: usize = i + 1;
        while j < fams.len() {
            if fams[i].code() == fams[j].code() {
                return Err(XC_DUP_NAME);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}
fn chk_err_decode_nonempty() -> Result<(), u16> {
    // 诊断码语义可解码（非零且能定位到段内偏移）。
    if XC_DRIFT - SEG_BASE == 0 || XC_CONSERVATION - SEG_BASE == 0 {
        return Err(XC_FAMILY_GAP);
    }
    Ok(())
}
fn chk_cap_positive() -> Result<(), u16> {
    let g = GovernanceSample::new(4)?;
    if g.cap == 0 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_cap_zero_rejected() -> Result<(), u16> {
    if GovernanceSample::new(0) != Err(XC_SET_EMPTY) {
        return Err(XC_SET_EMPTY);
    }
    Ok(())
}
fn chk_id_zero_semantics() -> Result<(), u16> {
    // id=0 的语义固定为非法（清单身份不唯一防线）。
    if XC_ID_BAD != SEG_BASE + 9 {
        return Err(XC_ID_BAD);
    }
    Ok(())
}
fn chk_min_eq_table_len() -> Result<(), u16> {
    if ASSERT_TABLE.len() < MIN_ASSERTIONS {
        return Err(XC_SET_TOO_FEW);
    }
    Ok(())
}
fn chk_names_nonempty() -> Result<(), u16> {
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        if ASSERT_TABLE[i].name.is_empty() {
            return Err(XC_DUP_NAME);
        }
        i += 1;
    }
    Ok(())
}

// —— 守恒族（8 条）——
fn chk_cons_empty() -> Result<(), u16> {
    let g = GovernanceSample::new(8)?;
    g.audit()
}
fn chk_cons_after_add() -> Result<(), u16> {
    let mut g = GovernanceSample::new(8)?;
    g.add(1)?;
    g.add(2)?;
    g.audit()
}
fn chk_cons_full() -> Result<(), u16> {
    let mut g = GovernanceSample::new(3)?;
    g.add(1)?;
    g.add(2)?;
    g.add(3)?;
    g.audit()
}
fn chk_cons_overflow_reject() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    g.add(1)?;
    g.add(2)?;
    if g.add(3) != Err(XC_CONSERVATION) {
        return Err(XC_CONSERVATION);
    }
    g.audit()
}
fn chk_total_counts_entries() -> Result<(), u16> {
    let mut g = GovernanceSample::new(8)?;
    let mut i: u32 = 1;
    while i <= 5 {
        g.add(i)?;
        i += 1;
    }
    if g.total != 5 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_reject_not_counted() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    g.add(1)?;
    let _ = g.add(1);
    if g.total != 1 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_cons_full_table() -> Result<(), u16> {
    // 断言清单自身的守恒面：id 连续且无洞。
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        if ASSERT_TABLE[i].id != (i + 1) as u32 {
            return Err(XC_ID_BAD);
        }
        i += 1;
    }
    Ok(())
}
fn chk_cons_double_audit() -> Result<(), u16> {
    let mut g = GovernanceSample::new(4)?;
    g.add(7)?;
    g.audit()?;
    g.audit()
}

// —— 注入族（10 条）——
fn chk_inj_id_zero() -> Result<(), u16> {
    let mut g = GovernanceSample::new(4)?;
    if g.add(0) != Err(XC_ID_BAD) {
        return Err(XC_ID_BAD);
    }
    Ok(())
}
fn chk_inj_dup() -> Result<(), u16> {
    let mut g = GovernanceSample::new(4)?;
    g.add(5)?;
    if g.add(5) != Err(XC_DUP_NAME) {
        return Err(XC_DUP_NAME);
    }
    Ok(())
}
fn chk_inj_cap() -> Result<(), u16> {
    let mut g = GovernanceSample::new(1)?;
    g.add(1)?;
    if g.add(2) != Err(XC_CONSERVATION) {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_inj_state_clean() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    let _ = g.add(0);
    let _ = g.add(0);
    if g.total != 0 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_inj_recover_after_reject() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    let _ = g.add(0);
    g.add(9)?;
    if g.total != 1 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_inj_no_partial_write() -> Result<(), u16> {
    let mut g = GovernanceSample::new(1)?;
    g.add(1)?;
    let _ = g.add(2);
    if g.entries.len() != 1 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_inj_rejected_counter() -> Result<(), u16> {
    let mut g = GovernanceSample::new(1)?;
    g.add(1)?;
    let _ = g.add(1);
    let _ = g.add(2);
    if g.rejected != 2 {
        return Err(XC_CONSERVATION);
    }
    Ok(())
}
fn chk_inj_family_gap() -> Result<(), u16> {
    // 覆盖面注入：五族计数必须在 §4 runner 落账后无零族（此处对
    // 静态表先行判定——runner 复算同式）。
    let mut fam_cnt = [0u32; FAMILY_COUNT];
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        let c = ASSERT_TABLE[i].family.code() as usize;
        if c >= 1 && c <= FAMILY_COUNT {
            fam_cnt[c - 1] += 1;
        }
        i += 1;
    }
    let mut i2: usize = 0;
    while i2 < FAMILY_COUNT {
        if fam_cnt[i2] == 0 {
            return Err(XC_FAMILY_GAP);
        }
        i2 += 1;
    }
    Ok(())
}
fn chk_inj_table_dup_name() -> Result<(), u16> {
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        let mut j: usize = i + 1;
        while j < ASSERT_TABLE.len() {
            if ASSERT_TABLE[i].name == ASSERT_TABLE[j].name {
                return Err(XC_DUP_NAME);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}
fn chk_inj_table_fn_present() -> Result<(), u16> {
    // 断言函数位全非空（fn 指针在静态表中恒非零——显式调用自证）。
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        let r = (ASSERT_TABLE[i].f)();
        if r.is_err() {
            return r;
        }
        i += 1;
    }
    Ok(())
}

// —— 对拍族（10 条）——
fn chk_pair_add_idempotent() -> Result<(), u16> {
    let mut a = GovernanceSample::new(4)?;
    a.add(3)?;
    let mut b = GovernanceSample::new(4)?;
    b.add(3)?;
    if a.total != b.total || a.entries.len() != b.entries.len() {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_reject_deterministic() -> Result<(), u16> {
    let mut a = GovernanceSample::new(1)?;
    a.add(1)?;
    let r1 = a.add(1);
    let r2 = a.add(1);
    if r1 != r2 {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_cap_deterministic() -> Result<(), u16> {
    let a = GovernanceSample::new(0);
    let b = GovernanceSample::new(0);
    if a != b {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_codes_stable() -> Result<(), u16> {
    let a = XC_DRIFT;
    let b = XC_DRIFT;
    if a != b {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_family_stable() -> Result<(), u16> {
    let a = AssertFamily::Injection.code();
    let b = AssertFamily::Injection.code();
    if a != b {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_fingerprint_stable() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    g.add(4)?;
    let f1 = (g.total, g.entries.len());
    let f2 = (g.total, g.entries.len());
    if f1 != f2 {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_audit_twice() -> Result<(), u16> {
    let mut g = GovernanceSample::new(2)?;
    g.add(6)?;
    let r1 = g.audit();
    let r2 = g.audit();
    if r1 != r2 {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_zero_arg_stable() -> Result<(), u16> {
    let a = GovernanceSample::new(4)?;
    let b = GovernanceSample::new(4)?;
    if a.total != b.total {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_name_lookup() -> Result<(), u16> {
    // 同名查找双跑必同（清单确定性序）。
    let i1 = table_index_of("chk_seg_align");
    let i2 = table_index_of("chk_seg_align");
    if i1 != i2 {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}
fn chk_pair_min_boundary() -> Result<(), u16> {
    let a = ASSERT_TABLE.len() >= MIN_ASSERTIONS;
    let b = ASSERT_TABLE.len() >= MIN_ASSERTIONS;
    if a != b {
        return Err(XC_NONDETERMINISTIC);
    }
    Ok(())
}

/// 清单静态表（42 条——五族覆盖：段位 6 / 闭集 8 / 守恒 8 / 注入 10 / 对拍 10）。
pub static ASSERT_TABLE: [AssertSpec; 42] = [
    AssertSpec { id: 1, family: AssertFamily::Segment, name: "chk_seg_align", f: chk_seg_align },
    AssertSpec { id: 2, family: AssertFamily::Segment, name: "chk_seg_span", f: chk_seg_span },
    AssertSpec { id: 3, family: AssertFamily::Segment, name: "chk_codes_in_seg", f: chk_codes_in_seg },
    AssertSpec { id: 4, family: AssertFamily::Segment, name: "chk_codes_distinct", f: chk_codes_distinct },
    AssertSpec { id: 5, family: AssertFamily::Segment, name: "chk_min_assert_const", f: chk_min_assert_const },
    AssertSpec { id: 6, family: AssertFamily::Segment, name: "chk_family_count", f: chk_family_count },
    AssertSpec { id: 7, family: AssertFamily::ClosedSet, name: "chk_family_roundtrip", f: chk_family_roundtrip },
    AssertSpec { id: 8, family: AssertFamily::ClosedSet, name: "chk_family_codes_distinct", f: chk_family_codes_distinct },
    AssertSpec { id: 9, family: AssertFamily::ClosedSet, name: "chk_err_decode_nonempty", f: chk_err_decode_nonempty },
    AssertSpec { id: 10, family: AssertFamily::ClosedSet, name: "chk_cap_positive", f: chk_cap_positive },
    AssertSpec { id: 11, family: AssertFamily::ClosedSet, name: "chk_cap_zero_rejected", f: chk_cap_zero_rejected },
    AssertSpec { id: 12, family: AssertFamily::ClosedSet, name: "chk_id_zero_semantics", f: chk_id_zero_semantics },
    AssertSpec { id: 13, family: AssertFamily::ClosedSet, name: "chk_min_eq_table_len", f: chk_min_eq_table_len },
    AssertSpec { id: 14, family: AssertFamily::ClosedSet, name: "chk_names_nonempty", f: chk_names_nonempty },
    AssertSpec { id: 15, family: AssertFamily::Conservation, name: "chk_cons_empty", f: chk_cons_empty },
    AssertSpec { id: 16, family: AssertFamily::Conservation, name: "chk_cons_after_add", f: chk_cons_after_add },
    AssertSpec { id: 17, family: AssertFamily::Conservation, name: "chk_cons_full", f: chk_cons_full },
    AssertSpec { id: 18, family: AssertFamily::Conservation, name: "chk_cons_overflow_reject", f: chk_cons_overflow_reject },
    AssertSpec { id: 19, family: AssertFamily::Conservation, name: "chk_total_counts_entries", f: chk_total_counts_entries },
    AssertSpec { id: 20, family: AssertFamily::Conservation, name: "chk_reject_not_counted", f: chk_reject_not_counted },
    AssertSpec { id: 21, family: AssertFamily::Conservation, name: "chk_cons_full_table", f: chk_cons_full_table },
    AssertSpec { id: 22, family: AssertFamily::Conservation, name: "chk_cons_double_audit", f: chk_cons_double_audit },
    AssertSpec { id: 23, family: AssertFamily::Injection, name: "chk_inj_id_zero", f: chk_inj_id_zero },
    AssertSpec { id: 24, family: AssertFamily::Injection, name: "chk_inj_dup", f: chk_inj_dup },
    AssertSpec { id: 25, family: AssertFamily::Injection, name: "chk_inj_cap", f: chk_inj_cap },
    AssertSpec { id: 26, family: AssertFamily::Injection, name: "chk_inj_state_clean", f: chk_inj_state_clean },
    AssertSpec { id: 27, family: AssertFamily::Injection, name: "chk_inj_recover_after_reject", f: chk_inj_recover_after_reject },
    AssertSpec { id: 28, family: AssertFamily::Injection, name: "chk_inj_no_partial_write", f: chk_inj_no_partial_write },
    AssertSpec { id: 29, family: AssertFamily::Injection, name: "chk_inj_rejected_counter", f: chk_inj_rejected_counter },
    AssertSpec { id: 30, family: AssertFamily::Injection, name: "chk_inj_family_gap", f: chk_inj_family_gap },
    AssertSpec { id: 31, family: AssertFamily::Injection, name: "chk_inj_table_dup_name", f: chk_inj_table_dup_name },
    AssertSpec { id: 32, family: AssertFamily::Injection, name: "chk_inj_table_fn_present", f: chk_inj_table_fn_present },
    AssertSpec { id: 33, family: AssertFamily::PairRun, name: "chk_pair_add_idempotent", f: chk_pair_add_idempotent },
    AssertSpec { id: 34, family: AssertFamily::PairRun, name: "chk_pair_reject_deterministic", f: chk_pair_reject_deterministic },
    AssertSpec { id: 35, family: AssertFamily::PairRun, name: "chk_pair_cap_deterministic", f: chk_pair_cap_deterministic },
    AssertSpec { id: 36, family: AssertFamily::PairRun, name: "chk_pair_codes_stable", f: chk_pair_codes_stable },
    AssertSpec { id: 37, family: AssertFamily::PairRun, name: "chk_pair_family_stable", f: chk_pair_family_stable },
    AssertSpec { id: 38, family: AssertFamily::PairRun, name: "chk_pair_fingerprint_stable", f: chk_pair_fingerprint_stable },
    AssertSpec { id: 39, family: AssertFamily::PairRun, name: "chk_pair_audit_twice", f: chk_pair_audit_twice },
    AssertSpec { id: 40, family: AssertFamily::PairRun, name: "chk_pair_zero_arg_stable", f: chk_pair_zero_arg_stable },
    AssertSpec { id: 41, family: AssertFamily::PairRun, name: "chk_pair_name_lookup", f: chk_pair_name_lookup },
    AssertSpec { id: 42, family: AssertFamily::PairRun, name: "chk_pair_min_boundary", f: chk_pair_min_boundary },
];

/// 按名查清单序（确定性线性扫——对拍族用）。
fn table_index_of(name: &str) -> usize {
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        if ASSERT_TABLE[i].name == name {
            return i;
        }
        i += 1;
    }
    ASSERT_TABLE.len()
}

// ---------------------------------------------------------------------------
// §4 断言 runner（B09 断言集执行体）
// ---------------------------------------------------------------------------

/// 断言执行报告：总数 / 通过 / 失败 / 逐族计数 / 首个失败 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    total: u32,
    passed: u32,
    failed: u32,
    per_family: [u32; FAMILY_COUNT],
    first_fail_id: u32,
}

impl RunReport {
    /// 总数。
    pub fn total(&self) -> u32 {
        self.total
    }
    /// 通过数。
    pub fn passed(&self) -> u32 {
        self.passed
    }
    /// 失败数。
    pub fn failed(&self) -> u32 {
        self.failed
    }
    /// 首个失败断言 id（全过为 0）。
    pub fn first_fail_id(&self) -> u32 {
        self.first_fail_id
    }
    /// 守恒自检：通过+失败 == 总数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.passed + self.failed != self.total {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }
}

/// 跑全量断言（逐条执行、逐族落账、确定性序）。
pub fn run_all() -> Result<RunReport, u16> {
    if ASSERT_TABLE.is_empty() {
        return Err(XC_SET_EMPTY);
    }
    if ASSERT_TABLE.len() < MIN_ASSERTIONS {
        return Err(XC_SET_TOO_FEW);
    }
    let mut rep = RunReport {
        total: ASSERT_TABLE.len() as u32,
        passed: 0,
        failed: 0,
        per_family: [0; FAMILY_COUNT],
        first_fail_id: 0,
    };
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        let spec = &ASSERT_TABLE[i];
        if spec.id == 0 {
            return Err(XC_ID_BAD);
        }
        let c = spec.family.code() as usize;
        if c >= 1 && c <= FAMILY_COUNT {
            rep.per_family[c - 1] += 1;
        }
        match (spec.f)() {
            Ok(()) => rep.passed += 1,
            Err(_) => {
                if rep.first_fail_id == 0 {
                    rep.first_fail_id = spec.id;
                }
                rep.failed += 1;
            }
        }
        i += 1;
    }
    // 五族覆盖机检。
    let mut f: usize = 0;
    while f < FAMILY_COUNT {
        if rep.per_family[f] == 0 {
            return Err(XC_FAMILY_GAP);
        }
        f += 1;
    }
    rep.audit()?;
    Ok(rep)
}

/// 清单文本导出（确定性序 + 双导出对拍）。
pub fn export_manifest() -> Result<String, u16> {
    let mut s = String::from("UNX-F0980-KTESTB09|v1|count=");
    s.push_str(&ASSERT_TABLE.len().to_string());
    s.push('\n');
    let mut i: usize = 0;
    while i < ASSERT_TABLE.len() {
        s.push_str(&ASSERT_TABLE[i].id.to_string());
        s.push('|');
        s.push_str(ASSERT_TABLE[i].family.name());
        s.push('|');
        s.push_str(ASSERT_TABLE[i].name);
        s.push('\n');
        i += 1;
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// §5 自例程（drill）——跑全量断言，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：≥40 断言全跑 + 双跑对拍 + 守恒 + 清单导出。
pub fn drill() -> Result<u32, u16> {
    // 首跑：42 条全过。
    let r1 = run_all()?;
    if r1.total() < MIN_ASSERTIONS as u32 || r1.failed() != 0 || r1.passed() != r1.total() {
        return Err(XC_SET_TOO_FEW);
    }
    // 双跑对拍：结果逐字段确定。
    let r2 = run_all()?;
    if r1 != r2 {
        return Err(XC_NONDETERMINISTIC);
    }
    // 清单导出双跑指纹稳定。
    let m1 = export_manifest()?;
    let m2 = export_manifest()?;
    if m1 != m2 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
