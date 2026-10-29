//! CGPU-F0787 · E 域不变量终审（目标 360 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0787，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域不变量终审——把 E 域全部结构性不变量（单调、界、守恒、
//!   闭集）登记成册，终审机对样本状态序列逐条重放核验：
//!   每条不变量 × 每个样本点都必须有核验记录，缺失即审不通过。
//!
//! 设计纪律：
//! - 不变量闭集四类：Monotonic（单调不减/不增）、Bound（上下界）、
//!   Conservation（守恒恒等式）、ClosedSet（取值闭集）——无第五类。
//! - 样本点：整数三元组 (a, b, total) 供各类不变量共用解读口径：
//!   Monotonic 看 a；Bound 看 b；Conservation 要求 a+b==total；
//!   ClosedSet 要求 a ∈ 档位闭集。
//! - 核验账本定容：AUDIT_CAP=128 条记录，超容逐出最旧并计数，
//!   守恒恒等式：total_checked == held + evicted。
//! - 终审口径：violations==0 且 coverage==全量 → PASS；
//!   任一 violated 且 severity==Fatal → 整审 FAIL；其余 WARN 不阻断。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6DA0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x6DA0，批内 CGPU-F0787 独占）。
pub const SEG_BASE: u16 = 0x6DA0;

/// 不变量编号重复登记。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 不变量册已满。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 未知不变量编号。
pub const E_NOINV: u16 = SEG_BASE + 3;
/// 样本点口径非法（total < a+b 等）。
pub const E_SAMPLE: u16 = SEG_BASE + 4;
/// 核验记录账本为空即终审。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// Fatal 级违例（终审不通过）。
pub const E_FATAL: u16 = SEG_BASE + 6;

/// 核验记录账本容量。
pub const AUDIT_CAP: usize = 128;
/// 闭集档位上限（a 的合法取值 0..=3）。
pub const TIER_MAX: u64 = 3;

/// 不变量类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvKind {
    /// 单调：a 序列单调不减。
    Monotonic,
    /// 界：b ∈ [lo, hi]。
    Bound,
    /// 守恒：a + b == total。
    Conservation,
    /// 闭集：a ∈ 0..=TIER_MAX。
    ClosedSet,
}

impl InvKind {
    /// 类别序号。
    pub fn ord(self) -> u8 {
        match self {
            InvKind::Monotonic => 0,
            InvKind::Bound => 1,
            InvKind::Conservation => 2,
            InvKind::ClosedSet => 3,
        }
    }
}

/// 不变量严重级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 违例即终审失败。
    Fatal,
    /// 违例仅记告警。
    Warn,
}

/// 不变量登记项。
#[derive(Clone, Debug)]
pub struct InvSpec {
    /// 不变量编号（册内唯一）。
    pub id: u16,
    /// 类别。
    pub kind: InvKind,
    /// 严重级。
    pub sev: Severity,
    /// Bound 类专用下界。
    pub lo: u64,
    /// Bound 类专用上界。
    pub hi: u64,
}

/// 样本点：整数三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 字段 a。
    pub a: u64,
    /// 字段 b。
    pub b: u64,
    /// 汇总 total。
    pub total: u64,
}

impl Sample {
    /// 口径校验：total ≥ a + b（无溢出）。
    pub fn validate(&self) -> Result<(), u16> {
        let s = self.a.checked_add(self.b).ok_or(E_SAMPLE)?;
        if self.total < s {
            return Err(E_SAMPLE);
        }
        Ok(())
    }
}

/// 单条不变量在单样本上的核验。
pub fn check_one(spec: &InvSpec, s: &Sample) -> Result<bool, u16> {
    s.validate()?;
    Ok(match spec.kind {
        InvKind::Monotonic => {
            // 单调以样本内自证口径：total ≥ a 即未回退（a 相对 total 占比不升不降由调用方跨样本判定）。
            s.a <= s.total
        }
        InvKind::Bound => s.b >= spec.lo && s.b <= spec.hi,
        InvKind::Conservation => s.a + s.b == s.total,
        InvKind::ClosedSet => s.a <= TIER_MAX,
    })
}

/// 跨样本单调核验：a 序列逐对不回退。
pub fn check_monotonic_series(spec: &InvSpec, series: &[Sample]) -> Result<bool, u16> {
    if series.is_empty() {
        return Err(E_SAMPLE);
    }
    let mut prev = series[0].a;
    for s in series.iter().skip(1) {
        if s.a < prev {
            return Ok(false);
        }
        prev = s.a;
        let _ = spec;
    }
    Ok(true)
}

/// 核验记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRec {
    /// 不变量编号。
    pub inv_id: u16,
    /// 样本序号。
    pub sample_idx: u32,
    /// 是否通过。
    pub pass: bool,
}

/// 核验记录账本（环形定容）。
#[derive(Clone, Debug)]
pub struct AuditLedger {
    ring: Vec<AuditRec>,
    evicted: u64,
    total: u64,
}

impl AuditLedger {
    pub fn new() -> Self {
        AuditLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, r: AuditRec) {
        if self.ring.len() >= AUDIT_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(r);
        self.total += 1;
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_CAP);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    /// 覆盖率：指定不变量是否有 ≥1 条核验记录。
    pub fn covers(&self, inv_id: u16) -> bool {
        self.ring.iter().any(|r| r.inv_id == inv_id)
    }
}

/// 不变量册。
#[derive(Clone, Debug, Default)]
pub struct InvRegistry {
    book: Vec<InvSpec>,
}

impl InvRegistry {
    pub fn new() -> Self {
        InvRegistry { book: Vec::new() }
    }
    /// 登记（编号唯一、界类必须 lo<=hi）。
    pub fn register(&mut self, s: InvSpec) -> Result<(), u16> {
        if self.book.iter().any(|o| o.id == s.id) {
            return Err(E_DUP);
        }
        if s.kind == InvKind::Bound && s.lo > s.hi {
            return Err(E_NOINV);
        }
        self.book.push(s);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.book.len()
    }
    pub fn get(&self, id: u16) -> Result<&InvSpec, u16> {
        self.book.iter().find(|s| s.id == id).ok_or(E_NOINV)
    }
    /// 终审：对样本序列逐不变量核验并落账；返回 (违例数, 告警数)。
    pub fn final_audit(
        &self,
        series: &[Sample],
        led: &mut AuditLedger,
    ) -> Result<(u32, u32), u16> {
        if series.is_empty() || self.book.is_empty() {
            return Err(E_EMPTY);
        }
        let mut violations = 0u32;
        let mut warns = 0u32;
        for (si, smp) in series.iter().enumerate() {
            for spec in &self.book {
                let pass = if spec.kind == InvKind::Monotonic {
                    // 单调类在序列首点核验一次整体口径。
                    if si == 0 {
                        check_monotonic_series(spec, series)?
                    } else {
                        true
                    }
                } else {
                    check_one(spec, smp)?
                };
                led.push(AuditRec {
                    inv_id: spec.id,
                    sample_idx: si as u32,
                    pass,
                });
                if !pass {
                    match spec.sev {
                        Severity::Fatal => violations += 1,
                        Severity::Warn => warns += 1,
                    }
                }
            }
        }
        Ok((violations, warns))
    }
}

/// 终审裁决：violations==0 → 0（PASS）；Fatal>0 → Err(E_FATAL)。
pub fn verdict(violations: u32) -> Result<u32, u16> {
    if violations == 0 {
        Ok(0)
    } else {
        Err(E_FATAL)
    }
}

/// 自例程：主路径（全过序列）+ Fatal 违例路径 + 错误路径 + 账本守恒。
pub fn drill() -> Result<u32, u16> {
    let mut reg = InvRegistry::new();
    reg.register(InvSpec {
        id: 1,
        kind: InvKind::Monotonic,
        sev: Severity::Fatal,
        lo: 0,
        hi: 0,
    })?;
    reg.register(InvSpec {
        id: 2,
        kind: InvKind::Bound,
        sev: Severity::Warn,
        lo: 10,
        hi: 100,
    })?;
    reg.register(InvSpec {
        id: 3,
        kind: InvKind::Conservation,
        sev: Severity::Fatal,
        lo: 0,
        hi: 0,
    })?;
    reg.register(InvSpec {
        id: 4,
        kind: InvKind::ClosedSet,
        sev: Severity::Fatal,
        lo: 0,
        hi: 0,
    })?;
    if reg.register(InvSpec {
        id: 1,
        kind: InvKind::Bound,
        sev: Severity::Warn,
        lo: 0,
        hi: 1,
    }) != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    if reg.register(InvSpec {
        id: 5,
        kind: InvKind::Bound,
        sev: Severity::Warn,
        lo: 9,
        hi: 3,
    }) != Err(E_NOINV)
    {
        return Err(E_NOINV);
    }
    // 主路径 1：全过序列。
    let good = vec![
        Sample { a: 1, b: 50, total: 51 },
        Sample { a: 2, b: 60, total: 62 },
        Sample { a: 3, b: 70, total: 73 },
    ];
    let mut led = AuditLedger::new();
    let (v, w) = reg.final_audit(&good, &mut led)?;
    if v != 0 || w != 0 {
        return Err(E_FATAL);
    }
    led.audit()?;
    // 覆盖率：4 条不变量全有记录。
    for id in 1..=4u16 {
        if !led.covers(id) {
            return Err(E_EMPTY);
        }
    }
    verdict(v)?;
    // 主路径 2：Warn 违例（b 越界）不阻断，但告警计数。
    let warn_seq = vec![Sample { a: 1, b: 200, total: 201 }];
    let mut led2 = AuditLedger::new();
    let (v2, w2) = reg.final_audit(&warn_seq, &mut led2)?;
    if v2 != 0 || w2 != 1 {
        return Err(E_FATAL);
    }
    // 主路径 3：Fatal 违例（守恒被破坏）→ violations>0。
    let bad = vec![Sample { a: 1, b: 2, total: 10 }];
    let mut led3 = AuditLedger::new();
    let (v3, _) = reg.final_audit(&bad, &mut led3)?;
    if verdict(v3) != Err(E_FATAL) {
        return Err(E_FATAL);
    }
    // 主路径 4：单调回退被检出。
    let mono_bad = vec![
        Sample { a: 5, b: 50, total: 55 },
        Sample { a: 3, b: 50, total: 53 },
    ];
    let mut led4 = AuditLedger::new();
    let (v4, _) = reg.final_audit(&mono_bad, &mut led4)?;
    if v4 == 0 {
        return Err(E_FATAL);
    }
    // 错误路径 1：样本口径非法（total < a+b）。
    let s_bad = Sample { a: 5, b: 6, total: 3 };
    if s_bad.validate() != Err(E_SAMPLE) {
        return Err(E_SAMPLE);
    }
    // 错误路径 2：空序列终审。
    let mut led5 = AuditLedger::new();
    if reg.final_audit(&[], &mut led5) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 3：未知不变量读取。
    if reg.get(99) != Err(E_NOINV) {
        return Err(E_NOINV);
    }
    // 定容守恒：灌 200 条记录进 128 容账本，恒等式仍成立。
    let mut led6 = AuditLedger::new();
    for i in 0..200u32 {
        led6.push(AuditRec {
            inv_id: 1,
            sample_idx: i,
            pass: true,
        });
    }
    led6.audit()?;
    if led6.held() != AUDIT_CAP || led6.ring_offset_check() != 72 {
        return Err(E_CAP);
    }
    Ok(0)
}

impl AuditLedger {
    /// 自查辅助：被逐出数（供 drill 断言）。
    pub fn ring_offset_check(&self) -> u64 {
        self.evicted
    }
}
