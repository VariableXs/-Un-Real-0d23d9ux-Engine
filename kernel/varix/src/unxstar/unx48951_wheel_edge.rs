//! UNX-F48951 平滑滚动分账登记与滚轮异常账 · 0x712C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F48951-J1 分账登记成文零越权实装；
//!   ③ UNX-F48952-J1 异常注入 ×10^2 全防御入账。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：登记声明校验 → 零实装校验 → delta 分类 → 异常入账 →
//!           计数守恒 → 账本复读。
//! 先证「没做」再证「做了」：平滑滚动的语义属于应用层，本域只登记
//! 消费分界。若登记里出现任何步进数，就等于越权实装；因此零实装必须
//! 先被断言，异常账才有资格谈「入账」。
//! 负 delta 不是异常：它是反向滚动，必须计入方向份额而不是防御计数。
//! 把负值当异常会让「异常全防御」的判据变成永远为真——这是本模块
//! 特意避开的自欺。
//! 关键差异：本模块只回答「平滑滚动的分界登记是否诚实、异常是否全部
//!           入账」，不做平滑算法（应用层）、不做轴锁定（B08-DISPATCH）、
//!           不做 fuzz 面（B08-BRIDGE/QUALITY）。谁在本域写插值步进，
//!           谁就越过了 §AI-62 §1 语义边界。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;
// p_meta 仅测试用（细格边界复算），移入 tests 以免非测试构建告警。

/// 诊断段基址（0x712C0，B08 族第五槽）。
pub const SEG_BASE: u32 = 0x712C0;

/// 异常注入轮数（卡面判据 ×10^2 的载体）。
pub const ANOMALY_ROUNDS: u32 = 100;
/// 平滑滚动消费分界：实装方必须是应用层。
pub const SMOOTH_OWNER: &str = "应用层（滚轮语义消费方）";
/// 平滑滚动在本域的步进数：必须恒为 0。
pub const SMOOTH_STEPS: u32 = 0;
/// 登记文本字段数。
pub const REG_FIELDS: usize = 3;
/// 异常分类数。
pub const ANOMALY_KINDS: usize = 4;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeCode {
    /// 登记非法（字段缺失或声明不完整）
    RegBad,
    /// 越权实装（本域出现平滑步进）
    OverreachBad,
    /// 所有者非法（实装方不是应用层）
    OwnerBad,
    /// 未知异常类别
    KindBad,
    /// 溢出异常（delta 越界）
    OverflowBad,
    /// 空异常（零 delta）
    ZeroBad,
    /// 次序非法
    OrderBad,
    /// 账不平（异常计数与总轮数不符）
    LedgerBad,
    /// 渲染非法
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
}

impl HeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HeCode::RegBad => "登记非法：分界声明字段缺失或不完整",
            HeCode::OverreachBad => "越权实装：本域出现非零平滑步进",
            HeCode::OwnerBad => "所有者非法：平滑滚动实装方不是应用层",
            HeCode::KindBad => "未知异常类别：超出闭集",
            HeCode::OverflowBad => "溢出异常：delta 绝对值超界",
            HeCode::ZeroBad => "空异常：零 delta 不得进入滚轮语义",
            HeCode::OrderBad => "次序非法：异常账计数次序断裂",
            HeCode::LedgerBad => "账不平：异常计数与总轮数不符",
            HeCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HeCode::ReplayBad => "复算不一致：同一注入重放不同",
            HeCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            HeCode::SegmentBad => "段位越界：诊断码逃出 0x712C0 窗口",
        }
    }

    pub const ALL: [HeCode; 12] = [
        HeCode::RegBad,
        HeCode::OverreachBad,
        HeCode::OwnerBad,
        HeCode::KindBad,
        HeCode::OverflowBad,
        HeCode::ZeroBad,
        HeCode::OrderBad,
        HeCode::LedgerBad,
        HeCode::RenderBad,
        HeCode::ReplayBad,
        HeCode::CoverageBad,
        HeCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HeCode::ALL.len() {
        if HeCode::ALL[i].val() == code {
            return HeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 平滑滚动分账登记（只登记，不实装） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SmoothRegistry {
    pub declared: bool,
    pub owner_is_app: bool,
    pub steps: u32,
    pub applied: bool,
    pub digest: u64,
}

impl SmoothRegistry {
    pub fn new() -> SmoothRegistry {
        SmoothRegistry {
            declared: true,
            owner_is_app: true,
            steps: SMOOTH_STEPS,
            applied: false,
            digest: p_core::FNV_OFF,
        }
    }

    /// 登记审计：三项声明必须同时成立，且步进恒为零。
    pub fn audit(&self) -> Result<(), u32> {
        if !self.declared {
            return Err(HeCode::RegBad.val());
        }
        if !self.owner_is_app {
            return Err(HeCode::OwnerBad.val());
        }
        if self.steps != 0 {
            return Err(HeCode::OverreachBad.val());
        }
        if self.applied {
            return Err(HeCode::OverreachBad.val());
        }
        Ok(())
    }

    pub fn declare(&mut self, owner_is_app: bool, steps: u32) -> Result<(), u32> {
        if steps != 0 {
            return Err(HeCode::OverreachBad.val());
        }
        if !owner_is_app {
            return Err(HeCode::OwnerBad.val());
        }
        self.declared = true;
        self.owner_is_app = owner_is_app;
        self.steps = steps;
        self.applied = false;
        self.digest = p_core::fold_u32(self.digest, 1);
        Ok(())
    }

    /// 登记文本：`owner=…|steps=0|applied=0`。
    pub fn text(&self) -> String {
        format!(
            "owner={}|steps={}|applied={}",
            SMOOTH_OWNER,
            self.steps,
            if self.applied { 1 } else { 0 }
        )
    }
}

/// 解析登记文本，拒绝任何非零步进或非零 applied。
pub fn audit_registry(text: &str) -> Result<(u32, u32), u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != REG_FIELDS {
        return Err(HeCode::RegBad.val());
    }
    let owner = match parts.first() {
        Some(v) => *v,
        None => return Err(HeCode::RegBad.val()),
    };
    if !owner.starts_with("owner=") || !owner.contains("应用层") {
        return Err(HeCode::OwnerBad.val());
    }
    let steps_part = match parts.get(1) {
        Some(v) => *v,
        None => return Err(HeCode::RegBad.val()),
    };
    let steps = parse_u32(steps_part.strip_prefix("steps=").unwrap_or(""))?;
    let applied_part = match parts.get(2) {
        Some(v) => *v,
        None => return Err(HeCode::RegBad.val()),
    };
    let applied = parse_u32(applied_part.strip_prefix("applied=").unwrap_or(""))?;
    if steps != 0 || applied != 0 {
        return Err(HeCode::OverreachBad.val());
    }
    Ok((steps, applied))
}

// ── delta 异常分类 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeltaKind {
    /// 正常正向
    Forward,
    /// 正常反向（负 delta 合法）
    Backward,
    /// 零 delta
    Zero,
    /// 越界
    Overflow,
}

impl DeltaKind {
    pub fn index(self) -> u8 {
        match self {
            DeltaKind::Forward => 0,
            DeltaKind::Backward => 1,
            DeltaKind::Zero => 2,
            DeltaKind::Overflow => 3,
        }
    }

    pub fn from_index(i: u8) -> Result<DeltaKind, u32> {
        match i {
            0 => Ok(DeltaKind::Forward),
            1 => Ok(DeltaKind::Backward),
            2 => Ok(DeltaKind::Zero),
            3 => Ok(DeltaKind::Overflow),
            _ => Err(HeCode::KindBad.val()),
        }
    }
}

pub fn classify(delta: i32) -> DeltaKind {
    if delta == 0 {
        DeltaKind::Zero
    } else if (delta as i64).unsigned_abs() > p_core::DELTA_MAX as u64 {
        DeltaKind::Overflow
    } else if delta < 0 {
        DeltaKind::Backward
    } else {
        DeltaKind::Forward
    }
}

/// 异常账：四类计数 + 拒绝计数，拒绝必须显式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnomalyLedger {
    pub counts: [u32; ANOMALY_KINDS],
    pub admits: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl AnomalyLedger {
    pub fn new() -> AnomalyLedger {
        AnomalyLedger {
            counts: [0; ANOMALY_KINDS],
            admits: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 负 delta 是合法方向，计入 Backward 且返回 Ok；
    /// 零与越界必须返回错误码，并计入 rejects。
    pub fn admit(&mut self, delta: i32) -> Result<DeltaKind, u32> {
        let kind = classify(delta);
        let idx = kind.index() as usize;
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.digest = p_core::fold_i32(self.digest, delta);
        match kind {
            DeltaKind::Zero => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, HeCode::ZeroBad.val());
                Err(HeCode::ZeroBad.val())
            }
            DeltaKind::Overflow => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, HeCode::OverflowBad.val());
                Err(HeCode::OverflowBad.val())
            }
            DeltaKind::Forward | DeltaKind::Backward => {
                self.admits = self.admits.saturating_add(1);
                Ok(kind)
            }
        }
    }

    /// 细格异常同样要入账：越界细格与零细格必须拒绝。
    pub fn admit_fine(&mut self, fine: i32) -> Result<DeltaKind, u32> {
        self.admit(fine)
    }

    pub fn audit(&self, rounds: u32) -> Result<(), u32> {
        if rounds == 0 {
            return Err(HeCode::CoverageBad.val());
        }
        let sum = self.admits.saturating_add(self.rejects);
        if sum != rounds {
            return Err(HeCode::LedgerBad.val());
        }
        let mut total = 0u32;
        let mut i = 0usize;
        while i < ANOMALY_KINDS {
            total = total.saturating_add(*self.counts.get(i).unwrap_or(&0));
            i += 1;
        }
        if total != rounds {
            return Err(HeCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 异常注入 ×10^2：每轮 8 个样本覆盖四类，拒绝数与轮数精确对账。
pub fn anomaly_rounds(rounds: u32) -> Result<AnomalyLedger, u32> {
    if rounds == 0 {
        return Err(HeCode::CoverageBad.val());
    }
    let mut led = AnomalyLedger::new();
    let mut seen = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let samples = [
            120,
            -120,
            0,
            p_core::DELTA_MAX + 1,
            1,
            -1,
            0,
            -p_core::DELTA_MAX - 1,
        ];
        let mut k = 0usize;
        while k < samples.len() {
            if let Some(d) = samples.get(k) {
                let _ = led.admit(*d);
            }
            seen = seen.saturating_add(1);
            k += 1;
        }
        i = i.saturating_add(1);
    }
    led.audit(seen)?;
    Ok(led)
}

/// fuzz：随机符号与量级，只允许零/越界被拒绝，负值必须放行。
pub fn fuzz_anomaly_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(HeCode::CoverageBad.val());
    }
    let mut led = AnomalyLedger::new();
    let mut rng = p_core::Lcg::new(0x8A11_0052);
    let mut i = 0u32;
    while i < rounds {
        let d = match rng.below(6) {
            0 => 0,
            1 => p_core::DELTA_MAX.saturating_add(1),
            2 => -p_core::DELTA_MAX.saturating_sub(1),
            _ => rng.signed(10_000),
        };
        let kind = classify(d);
        let res = led.admit(d);
        let expect_ok = kind == DeltaKind::Forward || kind == DeltaKind::Backward;
        if res.is_ok() != expect_ok {
            return Err(HeCode::KindBad.val());
        }
        i = i.saturating_add(1);
    }
    led.audit(rounds)?;
    Ok(led.digest)
}

// ── 账本渲染（异常账） ────────────────────────────────────────────
pub fn render_anomaly(l: &AnomalyLedger) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        l.counts[0], l.counts[1], l.counts[2], l.counts[3], l.rejects, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HeCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HeCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HeCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HeCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HeCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HeCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HeCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HeCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<(u32, u32), u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 6 {
        return Err(HeCode::RenderBad.val());
    }
    let mut counts = [0u32; ANOMALY_KINDS];
    let mut i = 0usize;
    while i < ANOMALY_KINDS {
        let raw = match parts.get(i) {
            Some(v) => *v,
            None => return Err(HeCode::RenderBad.val()),
        };
        let v = parse_u32(raw)?;
        if let Some(c) = counts.get_mut(i) {
            *c = v;
        }
        i += 1;
    }
    let rejects = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(HeCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(HeCode::RenderBad.val()),
    })?;
    let zero = *counts.get(2).unwrap_or(&0);
    let overflow = *counts.get(3).unwrap_or(&0);
    if rejects != zero.saturating_add(overflow) {
        return Err(HeCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(HeCode::RenderBad.val());
    }
    let total = counts[0]
        .saturating_add(counts[1])
        .saturating_add(zero)
        .saturating_add(overflow);
    Ok((total, rejects))
}

pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let led = anomaly_rounds(ANOMALY_ROUNDS)?;
    let text = render_anomaly(&led);
    let (total, rejects) = audit_render(&text)?;
    if total != led.admits + led.rejects || rejects != led.rejects {
        return Err(HeCode::ReplayBad.val());
    }
    Ok((total, rejects, led.digest))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(HeCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 非零步进登记必须炸 OverreachBad。
        let mut r = SmoothRegistry::new();
        if r.declare(true, 1).err() == Some(HeCode::OverreachBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 所有者不是应用层必须炸 OwnerBad。
        let mut s = SmoothRegistry::new();
        if s.declare(false, 0).err() == Some(HeCode::OwnerBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 零 delta 必须炸 ZeroBad。
        let mut led = AnomalyLedger::new();
        if led.admit(0).err() == Some(HeCode::ZeroBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let mut keep = AnomalyLedger::new();
        let _ = keep.admit(120);
        let text = render_anomaly(&keep).replacen("0x", "0y", 1);
        if audit_render(&text).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HeCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let r = SmoothRegistry::new();
    r.audit()?;
    let text = r.text();
    let (steps, applied) = audit_registry(&text)?;
    if steps != 0 || applied != 0 {
        return Err(HeCode::OverreachBad.val());
    }
    if SMOOTH_STEPS != 0 {
        return Err(HeCode::OverreachBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let led = anomaly_rounds(ANOMALY_ROUNDS)?;
    if led.counts[0] == 0 || led.counts[1] == 0 || led.counts[2] == 0 || led.counts[3] == 0 {
        return Err(HeCode::CoverageBad.val());
    }
    if led.rejects != led.counts[2].saturating_add(led.counts[3]) {
        return Err(HeCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = AnomalyLedger::new();
    if led.admit(-120) != Ok(DeltaKind::Backward) {
        return Err(HeCode::KindBad.val());
    }
    if led.admit(-p_core::DELTA_MAX) != Ok(DeltaKind::Backward) {
        return Err(HeCode::KindBad.val());
    }
    if led.admits != 2 || led.rejects != 0 {
        return Err(HeCode::LedgerBad.val());
    }
    if classify(-1) != DeltaKind::Backward || classify(1) != DeltaKind::Forward {
        return Err(HeCode::KindBad.val());
    }
    if !segment_guard() {
        return Err(HeCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = anomaly_rounds(50)?;
    let b = anomaly_rounds(50)?;
    if a.digest != b.digest || a.counts != b.counts {
        return Err(HeCode::ReplayBad.val());
    }
    if fuzz_anomaly_rounds(300)? != fuzz_anomaly_rounds(300)? {
        return Err(HeCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HeCode::ALL.len() {
        let v = HeCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let r = SmoothRegistry::new();
    out.push((
        "F48951 分账登记成文",
        r.audit().is_ok() && r.text().contains("steps=0"),
        r.text(),
    ));

    let audit = audit_registry(&r.text());
    out.push((
        "F48951 零越权实装",
        audit == Ok((0, 0)) && SMOOTH_STEPS == 0,
        match &audit {
            Ok((s, a)) => format!("steps={} applied={}", s, a),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut bad = SmoothRegistry::new();
    let over = bad.declare(true, 3);
    let mut wrong_owner = SmoothRegistry::new();
    let owner = wrong_owner.declare(false, 0);
    // 步进越权与所有者越权合并：同属登记拒绝，任一漏判即整条红。
    out.push((
        "F48951 步进/所有者越权被拒",
        over == Err(HeCode::OverreachBad.val())
            && owner == Err(HeCode::OwnerBad.val()),
        format!("steps={:?} owner={:?}", over, owner),
    ));

    let led = anomaly_rounds(ANOMALY_ROUNDS);
    out.push((
        "F48952 异常×10^2全入账",
        matches!(&led, Ok(l) if l.audit(ANOMALY_ROUNDS * 8).is_ok()),
        match &led {
            Ok(l) => format!(
                "fwd={} bwd={} zero={} ovf={} rejects={}",
                l.counts[0], l.counts[1], l.counts[2], l.counts[3], l.rejects
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut neg = AnomalyLedger::new();
    let neg_ok = neg.admit(-7) == Ok(DeltaKind::Backward) && neg.rejects == 0;
    let mut ovf = AnomalyLedger::new();
    let ovf_res = ovf.admit(p_core::DELTA_MAX + 1);
    // 负值放行与溢出拒绝合并：同一分类器的两侧边界，必须同时成立。
    out.push((
        "F48952 负值放行/溢出拒绝",
        neg_ok && ovf_res == Err(HeCode::OverflowBad.val()) && ovf.rejects == 1,
        format!("neg_rejects={} ovf={:?} ovf_rejects={}", neg.rejects, ovf_res, ovf.rejects),
    ));

    let fuzz = fuzz_anomaly_rounds(2_000);
    out.push((
        "F48952 fuzz×10^3分类",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let render = golden_run();
    out.push((
        "F48952 异常账可解析",
        render.is_ok(),
        match &render {
            Ok((t, r, d)) => format!("total={} rejects={} d=0x{:X}", t, r, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48951 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", HeCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48951_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48951 平滑滚动登记与滚轮异常账");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::unx48943_wheel_meta as p_meta;

    #[test]
    fn registry_declares_zero_steps() {
        let r = SmoothRegistry::new();
        assert_eq!(r.steps, 0);
        assert!(!r.applied);
        assert!(r.audit().is_ok());
        assert_eq!(audit_registry(&r.text()), Ok((0, 0)));
    }

    #[test]
    fn registry_rejects_overreach() {
        let mut r = SmoothRegistry::new();
        assert_eq!(r.declare(true, 1), Err(HeCode::OverreachBad.val()));
        r.steps = 1;
        assert_eq!(r.audit(), Err(HeCode::OverreachBad.val()));
    }

    #[test]
    fn registry_rejects_wrong_owner() {
        let mut r = SmoothRegistry::new();
        assert_eq!(r.declare(false, 0), Err(HeCode::OwnerBad.val()));
        let mut t = SmoothRegistry::new();
        t.owner_is_app = false;
        assert_eq!(t.audit(), Err(HeCode::OwnerBad.val()));
    }

    #[test]
    fn registry_text_tamper_is_rejected() {
        let r = SmoothRegistry::new();
        let bad = r.text().replacen("steps=0", "steps=5", 1);
        assert_eq!(audit_registry(&bad), Err(HeCode::OverreachBad.val()));
        let bad2 = r.text().replacen("applied=0", "applied=1", 1);
        assert_eq!(audit_registry(&bad2), Err(HeCode::OverreachBad.val()));
    }

    #[test]
    fn classify_covers_four_kinds() {
        assert_eq!(classify(1), DeltaKind::Forward);
        assert_eq!(classify(-1), DeltaKind::Backward);
        assert_eq!(classify(0), DeltaKind::Zero);
        assert_eq!(classify(p_core::DELTA_MAX + 1), DeltaKind::Overflow);
    }

    #[test]
    fn negative_is_admitted() {
        let mut l = AnomalyLedger::new();
        assert_eq!(l.admit(-120), Ok(DeltaKind::Backward));
        assert_eq!(l.admits, 1);
        assert_eq!(l.rejects, 0);
        assert_eq!(l.counts[1], 1);
    }

    #[test]
    fn zero_is_rejected() {
        let mut l = AnomalyLedger::new();
        assert_eq!(l.admit(0), Err(HeCode::ZeroBad.val()));
        assert_eq!(l.rejects, 1);
        assert_eq!(l.counts[2], 1);
    }

    #[test]
    fn overflow_is_rejected() {
        let mut l = AnomalyLedger::new();
        assert_eq!(l.admit(p_core::DELTA_MAX + 1), Err(HeCode::OverflowBad.val()));
        assert_eq!(l.rejects, 1);
        assert_eq!(l.counts[3], 1);
    }

    #[test]
    fn anomaly_rounds_balance() {
        match anomaly_rounds(ANOMALY_ROUNDS) {
            Ok(l) => {
                assert!(l.audit(ANOMALY_ROUNDS * 8).is_ok());
                // 每轮 8 样本：2×零 + 2×越界被拒，4 个合法值入账。
                assert_eq!(l.rejects, ANOMALY_ROUNDS * 4);
                assert_eq!(l.admits, ANOMALY_ROUNDS * 4);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fine_admit_uses_same_bounds() {
        let mut l = AnomalyLedger::new();
        assert_eq!(l.admit_fine(p_meta::FINE_DELTA), Ok(DeltaKind::Forward));
        assert_eq!(l.admit_fine(0), Err(HeCode::ZeroBad.val()));
    }

    #[test]
    fn fuzz_is_deterministic() {
        assert_eq!(fuzz_anomaly_rounds(300), fuzz_anomaly_rounds(300));
        assert!(fuzz_anomaly_rounds(0).is_err());
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let mut l = AnomalyLedger::new();
        let _ = l.admit(120);
        let _ = l.admit(0);
        let text = render_anomaly(&l);
        assert_eq!(audit_render(&text), Ok((2, 1)));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48951_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
