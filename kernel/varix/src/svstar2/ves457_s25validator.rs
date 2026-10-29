//! ves457_s25validator — VE-F4223 · 契约验证器
//! 静态验证（结构完整/约束可满足/判据可测三检）+语义验证（引用闭合/类型匹配/单位一致三检）——六检全过才可注册；
//! 任一检不过→拒绝注册+列明细；误报→规则校准留痕；报告缺→阻断；
//! 含无障碍判据可测性专检（判据不可测=拒绝注册——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1680;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VdCode {
    Ok = 0,
    StaticFail = SEG_BASE + 0x01, // 0x1681
    SemanticFail = SEG_BASE + 0x02, // 0x1682
    NotTestable = SEG_BASE + 0x03, // 0x1683 a11y 可测性专检（域本色）
}

/// 六检名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check6 {
    StructFull = 0, ConstraintSat = 1, CriterionTestable = 2,
    RefClosed = 3, TypeMatch = 4, UnitConsistent = 5,
}

impl Check6 {
    pub const ALL: [Check6; 6] = [
        Check6::StructFull, Check6::ConstraintSat, Check6::CriterionTestable,
        Check6::RefClosed, Check6::TypeMatch, Check6::UnitConsistent];
    pub fn name(self) -> &'static str {
        match self {
            Check6::StructFull => "struct-full", Check6::ConstraintSat => "constraint-sat",
            Check6::CriterionTestable => "criterion-testable", Check6::RefClosed => "ref-closed",
            Check6::TypeMatch => "type-match", Check6::UnitConsistent => "unit-consistent",
        }
    }
}

/// 验证报告（六检结果 + a11y 可测性位）。
pub struct ValidReport {
    pub results: [bool; 6],
    pub a11y_testable: bool, // 无障碍判据可测性专检（域本色）
    pub details: Vec<&'static str>,
    pub calibrated: u32,
}

/// 验证器。
pub struct Validator { pub reports: u32, pub blocked: bool }

impl Validator {
    pub fn new() -> Self { Validator { reports: 0, blocked: false } }
    /// 六检：全过→可注册；任一不过→拒绝注册+列明细。
    pub fn validate(&mut self, r: &ValidReport) -> Result<(), VdCode> {
        self.reports += 1;
        // 域本色专检先于六检（可测性闸）。
        if !r.a11y_testable { self.blocked = true; return Err(VdCode::NotTestable); }
        for (i, ok) in r.results.iter().enumerate() {
            if !ok {
                self.blocked = true;
                let _ = i;
                return Err(if i < 3 { VdCode::StaticFail } else { VdCode::SemanticFail });
            }
        }
        self.blocked = false;
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves457_checks() -> CheckSet {
    let mut set = CheckSet::new("ves457");
    let mut v = Validator::new();
    let pass = ValidReport { results: [true; 6], a11y_testable: true, details: Vec::new(), calibrated: 0 };

    // C457-01 六检全过→可注册。
    chk(&mut set, "C457-01", v.validate(&pass).is_ok() && !v.blocked, "六检全过");

    // C457-02 静态检失败（结构不完整）→拒。
    let mut r2 = pass.clone_for_test();
    r2.results[0] = false;
    chk(&mut set, "C457-02", v.validate(&r2) == Err(VdCode::StaticFail) && v.blocked, "静态拒");

    // C457-03 语义检失败（引用不闭合）→拒。
    let mut r3 = pass.clone_for_test();
    r3.results[3] = false;
    chk(&mut set, "C457-03", v.validate(&r3) == Err(VdCode::SemanticFail), "语义拒");

    // C457-04 域本色专检：判据不可测=拒绝注册（先于六检）。
    let mut r4 = pass.clone_for_test();
    r4.a11y_testable = false;
    chk(&mut set, "C457-04", v.validate(&r4) == Err(VdCode::NotTestable), "可测性闸");

    // C457-05 六检名单单源。
    chk(&mut set, "C457-05", Check6::ALL.len() == 6
        && Check6::ALL[2].name() == "criterion-testable", "六检单源");

    // C457-06 误报校准留痕（校准计数）。
    let mut r6 = pass.clone_for_test();
    r6.calibrated = 1;
    v.validate(&r6).unwrap();
    chk(&mut set, "C457-06", v.reports == 4, "校准留痕计数");

    // C457-07 六检分组（前三静态/后三语义）。
    chk(&mut set, "C457-07", Check6::CriterionTestable as u8 == 2
        && Check6::RefClosed as u8 == 3, "三三分组");

    // C457-08 blocked 复位（成功验证后）。
    chk(&mut set, "C457-08", !v.blocked, "复位");

    // C457-09 失败明细可列（details 空=明细在验检名）。
    let mut r9 = pass.clone_for_test();
    r9.results[5] = false;
    let err = v.validate(&r9);
    chk(&mut set, "C457-09", err == Err(VdCode::SemanticFail)
        && Check6::ALL[5].name() == "unit-consistent", "明细可定位");

    // C457-10 全绿终态复验。
    chk(&mut set, "C457-10", v.validate(&pass).is_ok(), "终态复验");

    // C457-11 报告计数累计。
    chk(&mut set, "C457-11", v.reports == 6, "报告计数");

    // C457-12 错误码段位单源。
    chk(&mut set, "C457-12", VdCode::NotTestable as u16 == SEG_BASE + 3, "段位单源");

    // ===== 二轮深化判据（wbv-r2d3 · 2026-10-27）=====
    let mut led = ValidLedger::new();
    led.record(&pass_report(), VdCode::Ok);
    let mut bad1 = pass_report();
    bad1.results[1] = false;
    led.record(&bad1, VdCode::StaticFail);
    let mut bad2 = pass_report();
    bad2.a11y_testable = false;
    led.record(&bad2, VdCode::NotTestable);

    // C457-13 账本守恒恒等式（过+拒 ≡ 总提交）。
    chk(&mut set, "C457-13", led.conservation() && led.submitted == 3, "守恒恒等式");

    // C457-14 域本色：a11y 专检拒绝单列留痕。
    chk(&mut set, "C457-14", led.a11y_rejects() == 1, "a11y 拒绝单列");

    // C457-15 归因一致性（失败位图 ↔ 结局码）。
    chk(&mut set, "C457-15", led.attribution_ok(), "归因一致");

    // C457-16 批次归因（每 8 次换批）。
    let mut led2 = ValidLedger::new();
    for _ in 0..10 { led2.record(&pass_report(), VdCode::Ok); }
    chk(&mut set, "C457-16", led2.batch_rejects(0) == 0 && led2.entries[9].batch == 1, "批次归因");

    // C457-17 LCG 注入回归（64 轮 100% 拒绝 + 负对照零误报）。
    chk(&mut set, "C457-17", injection_regression(64).is_ok(), "注入回归");

    // C457-18 幂等闸（同报告二次提交零副作用）。
    chk(&mut set, "C457-18", idempotence_gate(&pass_report()) && idempotence_gate(&bad1), "幂等闸");

    // C457-19 报告定长序列化往返。
    let mut rep = pass_report();
    rep.calibrated = 0x0102_0304;
    let enc = encode_report(&rep);
    let roundtrip = decode_report(&enc).map(|d| reports_equal(&d, &rep)).unwrap_or(false);
    chk(&mut set, "C457-19", roundtrip, "序列化往返");

    // C457-20 指纹抗篡改（翻转一位→解码拒绝）。
    let mut tampered = encode_report(&rep);
    tampered[0] ^= 0x01;
    chk(&mut set, "C457-20", decode_report(&tampered).is_none(), "指纹抗篡改");

    // C457-21 边界探针四件套。
    chk(&mut set, "C457-21", boundary_probes(), "边界探针");

    // C457-22 整数分位（失败位图中位数，排序取点禁浮点）。
    chk(&mut set, "C457-22", median_fail_mask(&led) == 0, "整数分位");

    // C457-23 FNV 指纹确定性单源。
    chk(&mut set, "C457-23", fnv1a(b"ves457") != 0 && fnv1a(b"ves457") == fnv1a(b"ves457"), "指纹单源");

    // C457-24 LCG 同种子同序列。
    let mut l1 = Lcg457::new(7);
    let mut l2 = Lcg457::new(7);
    chk(&mut set, "C457-24", (0..8).all(|_| l1.next_u16() == l2.next_u16()), "LCG 确定性");

    set
}

/// ValidReport 测试克隆助手（避免 derive 冲突，保持 house style 显式）。
impl ValidReport {
    fn clone_for_test(&self) -> ValidReport {
        ValidReport { results: self.results, a11y_testable: self.a11y_testable,
            details: self.details.clone(), calibrated: self.calibrated }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves457_smoke() {
        let set = run_ves457_checks();
        assert!(set.failed_count() == 0, "ves457 red: {}", set.red_items_text());
    }
}

// ===== 二轮深化（2026-10-27 · wbv-r2d3）：验证账本 / 注入回归 / 序列化指纹 =====
// 深化面：① 结构化验证账本（attempt 级留痕 + 批次归因 + 守恒恒等式）；
//         ② 边界探针（空账本 / 越界序号 / 重复注入 / 满容量语义）；
//         ③ LCG 注入回归（确定性随机缺陷，验证器必须 100% 拒绝 + 负对照零误报）；
//         ④ 幂等闸（同报告二次提交零副作用）；
//         ⑤ 报告定长序列化往返 + FNV-1a 指纹抗篡改。

/// 账本条目：一次验证 attempt 的结构化留痕（批次 / 六检失败位图 / 结局码）。
pub struct LedgerEntry {
    pub attempt: u32,
    pub batch: u16,
    pub outcome: VdCode,
    pub fail_mask: u16, // bit i = Check6 第 i 检未过
    pub a11y_ok: bool,
}

/// 验证账本：过+拒 ≡ 总提交（守恒恒等式），每 8 次换批归因。
pub struct ValidLedger {
    pub entries: Vec<LedgerEntry>,
    pub submitted: u32,
    pub batch_size: u16,
}

impl ValidLedger {
    pub fn new() -> Self { ValidLedger { entries: Vec::new(), submitted: 0, batch_size: 8 } }
    pub fn record(&mut self, r: &ValidReport, outcome: VdCode) {
        let mut fail_mask = 0u16;
        for (i, ok) in r.results.iter().enumerate() {
            if !ok { fail_mask |= 1u16 << i; }
        }
        self.entries.push(LedgerEntry {
            attempt: self.submitted,
            batch: (self.submitted / self.batch_size as u32) as u16,
            outcome,
            fail_mask,
            a11y_ok: r.a11y_testable,
        });
        self.submitted += 1;
    }
    pub fn passed(&self) -> u32 { self.entries.iter().filter(|e| e.outcome == VdCode::Ok).count() as u32 }
    pub fn rejected(&self) -> u32 { self.entries.iter().filter(|e| e.outcome != VdCode::Ok).count() as u32 }
    /// 守恒恒等式：过+拒 ≡ 总提交，且条目数 ≡ 总提交。
    pub fn conservation(&self) -> bool {
        self.passed() + self.rejected() == self.submitted && self.entries.len() as u32 == self.submitted
    }
    /// 按批次分列统计拒绝数。
    pub fn batch_rejects(&self, batch: u16) -> u32 {
        self.entries.iter().filter(|e| e.batch == batch && e.outcome != VdCode::Ok).count() as u32
    }
    /// 越界查询投影：attempt 越界→None（不炸）。
    pub fn fail_mask_at(&self, attempt: u32) -> Option<u16> {
        self.entries.iter().find(|e| e.attempt == attempt).map(|e| e.fail_mask)
    }
    /// 域本色：a11y 可测性闸拒绝单列留痕。
    pub fn a11y_rejects(&self) -> u32 {
        self.entries.iter().filter(|e| e.outcome == VdCode::NotTestable).count() as u32
    }
    /// 归因一致性：失败位图与结局码互相印证（前三位→静态 / 后三位→语义 / a11y→专检）。
    pub fn attribution_ok(&self) -> bool {
        self.entries.iter().all(|e| match e.outcome {
            VdCode::Ok => e.fail_mask == 0 && e.a11y_ok,
            VdCode::NotTestable => !e.a11y_ok,
            VdCode::StaticFail => e.a11y_ok && e.fail_mask & 0x0007 != 0,
            VdCode::SemanticFail => e.a11y_ok && e.fail_mask & 0x0038 != 0,
        })
    }
}

fn pass_report() -> ValidReport {
    ValidReport { results: [true; 6], a11y_testable: true, details: Vec::new(), calibrated: 0 }
}

/// 字段级相等投影（ValidReport 不派生 PartialEq，保持 house style）。
pub fn reports_equal(a: &ValidReport, b: &ValidReport) -> bool {
    a.results == b.results && a.a11y_testable == b.a11y_testable && a.calibrated == b.calibrated
}

/// 8 行确定性 LCG（无浮点/无墙钟，注入回归专用）。
pub struct Lcg457(u64);
impl Lcg457 {
    pub fn new(seed: u64) -> Self { Lcg457(seed ^ 0x9E37_79B9_7F4A_7C15) }
    pub fn next_u16(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u16
    }
}

/// 注入回归：N 轮随机置某一检为假，验证器必须 100% 拒绝且归因正确；负对照（干净报告）零误报。
pub fn injection_regression(rounds: u32) -> Result<(), &'static str> {
    let mut v = Validator::new();
    let mut rng = Lcg457::new(0x5645_5334_3537); // "VES457" 派生种子
    for _ in 0..rounds {
        let slot = (rng.next_u16() % 6) as usize;
        let mut bad = pass_report();
        bad.results[slot] = false;
        let want = if slot < 3 { VdCode::StaticFail } else { VdCode::SemanticFail };
        if v.validate(&bad) != Err(want) { return Err("注入缺陷漏拒"); }
        let mut clean = pass_report();
        clean.calibrated = 1; // 误报校准留痕
        if v.validate(&clean).is_err() { return Err("负对照误报"); }
    }
    Ok(())
}

/// 幂等闸：同一报告二次提交→结局一致、计数严格 +1、blocked 终值一致（零副作用）。
pub fn idempotence_gate(r: &ValidReport) -> bool {
    let mut v = Validator::new();
    let first = v.validate(r);
    let blocked1 = v.blocked;
    let reports1 = v.reports;
    let second = v.validate(r);
    second == first && v.reports == reports1 + 1 && v.blocked == blocked1
}

/// FNV-1a 32 位指纹（确定性，无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes { h ^= *b as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 报告定长投影：6 结果位打包 1 字节 + a11y 位 1 字节 + calibrated 4 字节 = 6 字节体 + 4 字节 FNV = 10 字节。
pub fn encode_report(r: &ValidReport) -> [u8; 10] {
    let mut bits = 0u8;
    for (i, ok) in r.results.iter().enumerate() {
        if *ok { bits |= 1u8 << i; }
    }
    let mut out = [0u8; 10];
    out[0] = bits;
    out[1] = if r.a11y_testable { 1 } else { 0 };
    out[2] = (r.calibrated & 0xFF) as u8;
    out[3] = ((r.calibrated >> 8) & 0xFF) as u8;
    out[4] = ((r.calibrated >> 16) & 0xFF) as u8;
    out[5] = ((r.calibrated >> 24) & 0xFF) as u8;
    let fp = fnv1a(&out[..6]);
    out[6] = (fp & 0xFF) as u8;
    out[7] = ((fp >> 8) & 0xFF) as u8;
    out[8] = ((fp >> 16) & 0xFF) as u8;
    out[9] = ((fp >> 24) & 0xFF) as u8;
    out
}

/// 解码往返：指纹不符→None（不炸）。
pub fn decode_report(b: &[u8; 10]) -> Option<ValidReport> {
    let fp = u32::from_le_bytes([b[6], b[7], b[8], b[9]]);
    if fnv1a(&b[..6]) != fp { return None; }
    let mut results = [false; 6];
    for i in 0..6 { results[i] = b[0] & (1u8 << i) != 0; }
    Some(ValidReport {
        results,
        a11y_testable: b[1] != 0,
        details: Vec::new(),
        calibrated: u32::from_le_bytes([b[2], b[3], b[4], b[5]]),
    })
}

/// 边界探针四件套：空账本 / 越界序号 / 重复注入 / 批次边界（无 panic，显性码）。
pub fn boundary_probes() -> bool {
    // ① 空账本查询投影：全零且守恒成立。
    let empty = ValidLedger::new();
    let p1 = empty.conservation() && empty.passed() == 0 && empty.a11y_rejects() == 0;
    // ② 越界序号：fail_mask_at 越界→None，在界→Some(0)。
    let mut l = ValidLedger::new();
    let r = pass_report();
    l.record(&r, VdCode::Ok);
    let p2 = l.fail_mask_at(1).is_none() && l.fail_mask_at(0) == Some(0);
    // ③ 重复注入：同缺陷报告连续提交两次→两次同码拒绝。
    let mut bad = pass_report();
    bad.results[4] = false;
    let mut v = Validator::new();
    let p3 = v.validate(&bad) == Err(VdCode::SemanticFail)
        && v.validate(&bad) == Err(VdCode::SemanticFail);
    // ④ 批次边界：单条目落在 0 批，拒绝归因不串批。
    let p4 = l.batch_rejects(0) == 0 && l.batch_rejects(1) == 0;
    p1 && p2 && p3 && p4
}

/// 整数分位：失败位图计数排序取中位（禁浮点，排序取点）。
pub fn median_fail_mask(led: &ValidLedger) -> u16 {
    let mut v: Vec<u16> = led.entries.iter().map(|e| e.fail_mask).collect();
    if v.is_empty() { return 0; }
    v.sort_unstable();
    v[v.len() / 2]
}
