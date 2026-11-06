//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18423 · 矩阵条目依据登记（manifest/标志/锚基线三源） · 0x7E580 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18423-J1 每条目依据字段非空断言 100/100 样例，无依据
//!       默认值注入必红
//! 本模块结构清单：
//!   1. 冻结常量层：三源闭集/样本数 100/依据引用格式闸（推导注释）；
//!   2. 诊断码闭集 BrCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 源模型 BasisSource：Manifest=0/Flag=1/AnchorBaseline=2，
//!      三源各自引用文本格式闸（后缀/前缀/长度）；
//!   4. 依据记录 BasisRecord：条目号+源+引用文本+状态机
//!      （Unregistered→Registered→Verified 单向）；
//!   5. 依据登记册 BasisRegistry：数组账本（条目号唯一）+ 全册核验；
//!   6. 注入核：空依据默认值注入/坏引用文本注入/源越界注入/重号
//!      注入 四类逐类 10/10 断言被显式拒绝；
//!   7. 百样语料：100 条确定性样例（manifest 40/标志 30/锚基线 30，
//!      按覆盖面分配），逐条登记→核验→100/100 非空断言；
//!   8. run_unx18423_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E580，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E580;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 源闭集大小（写死 3：判据原文三源——manifest/标志/锚基线）。
pub const BASIS_SOURCES: usize = 3;
/// 源号冻结：0=manifest 依据（清单文件背书）。
pub const SRC_MANIFEST: u8 = 0;
/// 源号冻结：1=标志依据（开关/策略背书）。
pub const SRC_FLAG: u8 = 1;
/// 源号冻结：2=锚基线依据（锚点实测基线背书）。
pub const SRC_ANCHOR: u8 = 2;
/// 样本总数（写死 100：判据原文 100/100 语义）。
pub const SAMPLES: usize = 100;
/// manifest 引用后缀闸（写死 9 字节：".manifest"）。
pub const MANIFEST_SUFFIX: &str = ".manifest";
/// 标志引用前缀闸（写死 5 字节："flag:"）。
pub const FLAG_PREFIX: &str = "flag:";
/// 锚基线引用前缀闸（写死 5 字节："anch-"）。
pub const ANCHOR_PREFIX: &str = "anch-";
/// 引用文本最短长（写死 8：前/后缀 + 至少 3 字符主体，防空壳）。
pub const REF_LEN_MIN: usize = 8;
/// 引用文本最长长（写死 64：登记册定宽预算，超长拒）。
pub const REF_LEN_MAX: usize = 64;
/// 判据轮数（写死 10：注入必红 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 三源样本分配（写死 40/30/30：manifest 覆盖面最广故占大头，
/// 40+30+30=100 与 SAMPLES 勾稽）。
pub const ALLOC_MANIFEST: usize = 40;
pub const ALLOC_FLAG: usize = 30;
pub const ALLOC_ANCHOR: usize = 30;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrCode {
    /// 无依据默认值：值入册而依据字段为空
    NoBasisDefault,
    /// 源越界：源号落 0..2 闭集外
    SourceOutOfRange,
    /// 引用为空：依据字段空串
    RefEmpty,
    /// 引用过短：低于 8 字节下限
    RefTooShort,
    /// 引用过长：超过 64 字节上限
    RefTooLong,
    /// 引用格式坏：不合该源前/后缀闸
    RefFormatBad,
    /// 条目重号：同条目号重复登记
    EntryDuplicated,
    /// 条目越界：条目号落 0..99 之外
    EntryOutOfRange,
    /// 核验未过：登记后未走 Verified 即入册使用
    NotVerified,
    /// 状态机倒退：Verified 后试图重登记
    StateRegression,
    /// 百样核验缺格：100 样本未全覆盖
    SampleGap,
    /// 双跑不一致
    NotReproducible,
}

impl BrCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BrCode::NoBasisDefault => "无依据默认值：依据字段为空必红",
            BrCode::SourceOutOfRange => "源越界：源号须落 manifest/标志/锚基线闭集",
            BrCode::RefEmpty => "引用为空：依据字段不得为空串",
            BrCode::RefTooShort => "引用过短：低于 8 字节下限",
            BrCode::RefTooLong => "引用过长：超过 64 字节上限",
            BrCode::RefFormatBad => "引用格式坏：不合该源前/后缀闸",
            BrCode::EntryDuplicated => "条目重号：条目号在册唯一",
            BrCode::EntryOutOfRange => "条目越界：条目号落 0..99 之外",
            BrCode::NotVerified => "核验未过：未 Verified 不得入册使用",
            BrCode::StateRegression => "状态机倒退：Verified 后重登记拒",
            BrCode::SampleGap => "百样核验缺格：100 样本未全覆盖",
            BrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [BrCode; 12] = [
        BrCode::NoBasisDefault,
        BrCode::SourceOutOfRange,
        BrCode::RefEmpty,
        BrCode::RefTooShort,
        BrCode::RefTooLong,
        BrCode::RefFormatBad,
        BrCode::EntryDuplicated,
        BrCode::EntryOutOfRange,
        BrCode::NotVerified,
        BrCode::StateRegression,
        BrCode::SampleGap,
        BrCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BrCode::ALL.len() {
        if let Some(c) = BrCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E580 段闭集内"
}

// ── 引用文本格式闸 ─────────────────────────────────────────────────
/// 后缀匹配（字节级，无越界：短于后缀即 false）。
fn ends_with(text: &str, suffix: &str) -> bool {
    let t = text.as_bytes();
    let s = suffix.as_bytes();
    if t.len() < s.len() {
        return false;
    }
    let off = t.len() - s.len();
    let mut i = 0;
    while i < s.len() {
        if t.get(off + i).copied().unwrap_or(0) != s.get(i).copied().unwrap_or(1) {
            return false;
        }
        i += 1;
    }
    true
}

/// 前缀匹配（字节级）。
fn starts_with(text: &str, prefix: &str) -> bool {
    let t = text.as_bytes();
    let p = prefix.as_bytes();
    if t.len() < p.len() {
        return false;
    }
    let mut i = 0;
    while i < p.len() {
        if t.get(i).copied().unwrap_or(0) != p.get(i).copied().unwrap_or(1) {
            return false;
        }
        i += 1;
    }
    true
}

/// 单源格式闸：manifest 需 ".manifest" 后缀；标志需 "flag:" 前缀；
/// 锚基线需 "anch-" 前缀。三源共用长度闸 [8,64]。
pub fn ref_format_ok(source: u8, text: &str) -> Result<(), u32> {
    if text.is_empty() {
        return Err(BrCode::RefEmpty.code());
    }
    if text.len() < REF_LEN_MIN {
        return Err(BrCode::RefTooShort.code());
    }
    if text.len() > REF_LEN_MAX {
        return Err(BrCode::RefTooLong.code());
    }
    match source {
        SRC_MANIFEST if ends_with(text, MANIFEST_SUFFIX) => Ok(()),
        SRC_FLAG if starts_with(text, FLAG_PREFIX) => Ok(()),
        SRC_ANCHOR if starts_with(text, ANCHOR_PREFIX) => Ok(()),
        SRC_MANIFEST | SRC_FLAG | SRC_ANCHOR => Err(BrCode::RefFormatBad.code()),
        _ => Err(BrCode::SourceOutOfRange.code()),
    }
}

// ── 依据记录状态机 ─────────────────────────────────────────────────
/// 依据状态：Unregistered→Registered→Verified。单向，倒退拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasisPhase {
    Unregistered,
    Registered,
    Verified,
}

/// 单条依据记录。
#[derive(Clone, Debug)]
pub struct BasisRecord {
    /// 矩阵条目号（0..99，闭集）。
    pub entry_id: u16,
    /// 依据源（0/1/2 闭集）。
    pub source: u8,
    /// 依据引用文本（非空，格式过闸）。
    pub reference: String,
    /// 状态机当前相。
    pub phase: BasisPhase,
}

impl BasisRecord {
    /// 依据字段非空判定（J1 核心语义：非空=有源+有非空引用+已核验）。
    pub fn basis_present(&self) -> bool {
        self.phase == BasisPhase::Verified
            && !self.reference.is_empty()
            && self.source < BASIS_SOURCES as u8
    }
}

/// 构造记录：四闸依次——条目号闭集/源闭集/引用非空/引用格式。
pub fn make_record(entry_id: u16, source: u8, reference: &str) -> Result<BasisRecord, u32> {
    if entry_id as usize >= SAMPLES {
        return Err(BrCode::EntryOutOfRange.code());
    }
    if source >= BASIS_SOURCES as u8 {
        return Err(BrCode::SourceOutOfRange.code());
    }
    ref_format_ok(source, reference)?;
    Ok(BasisRecord {
        entry_id,
        source,
        reference: reference.to_string(),
        phase: BasisPhase::Registered,
    })
}

// ── 依据登记册 ─────────────────────────────────────────────────────
/// 依据登记册：条目号唯一账本 + 全册核验。
pub struct BasisRegistry {
    records: Vec<BasisRecord>,
    /// 登记审计：尝试/成功/拒绝（守恒不变量）。
    pub tried: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 拒因账（逐次留码，审计可复算）。
    pub reject_codes: Vec<u32>,
}

impl BasisRegistry {
    /// 新册。
    pub fn new() -> BasisRegistry {
        BasisRegistry {
            records: Vec::new(),
            tried: 0,
            accepted: 0,
            rejected: 0,
            reject_codes: Vec::new(),
        }
    }

    /// 条目号在册判定。
    pub fn has_entry(&self, entry_id: u16) -> bool {
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.entry_id == entry_id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 按条目号查记录（只读副本）。
    pub fn find(&self, entry_id: u16) -> Option<BasisRecord> {
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.entry_id == entry_id {
                    return Some(r.clone());
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条依据：条目重号/越界/格式坏显式拒，拒因留账。
    pub fn register(&mut self, rec: BasisRecord) -> Result<(), u32> {
        self.tried = self.tried.saturating_add(1);
        let err = if rec.entry_id as usize >= SAMPLES {
            Some(BrCode::EntryOutOfRange.code())
        } else if self.has_entry(rec.entry_id) {
            Some(BrCode::EntryDuplicated.code())
        } else if ref_format_ok(rec.source, &rec.reference).is_err() {
            Some(BrCode::RefFormatBad.code())
        } else if rec.phase != BasisPhase::Registered {
            // 只有 Registered 相可入册；Unregistered 是空壳，Verified 是
            // 绕过核验的倒退注入。
            Some(BrCode::StateRegression.code())
        } else {
            None
        };
        match err {
            Some(c) => {
                self.rejected = self.rejected.saturating_add(1);
                self.reject_codes.push(c);
                Err(c)
            }
            None => {
                self.records.push(rec);
                self.accepted = self.accepted.saturating_add(1);
                Ok(())
            }
        }
    }

    /// 核验：Registered→Verified（引用格式复核一次，防登记后改写）。
    pub fn verify(&mut self, entry_id: u16) -> Result<(), u32> {
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get_mut(i) {
                if r.entry_id == entry_id {
                    if r.phase == BasisPhase::Verified {
                        return Err(BrCode::StateRegression.code());
                    }
                    ref_format_ok(r.source, &r.reference)?;
                    r.phase = BasisPhase::Verified;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(BrCode::NotVerified.code())
    }

    /// 全册核验：在册记录逐条 basis_present，条目覆盖数返回。
    pub fn audit(&self) -> Result<usize, u32> {
        let mut covered = 0usize;
        let mut i = 0;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(BrCode::NotVerified.code()),
            };
            if !r.basis_present() {
                return Err(BrCode::NoBasisDefault.code());
            }
            covered = covered.saturating_add(1);
            i += 1;
        }
        Ok(covered)
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 计数守恒：尝试=成功+拒绝。
    pub fn conserve_ok(&self) -> bool {
        self.tried == self.accepted.saturating_add(self.rejected)
    }
}

// ── 百样语料（100 条确定性） ────────────────────────────────────────
/// 第 idx 样例的源分配（0..40 manifest / 40..70 标志 / 70..100 锚基线）。
pub fn sample_source(idx: usize) -> u8 {
    if idx < ALLOC_MANIFEST {
        SRC_MANIFEST
    } else if idx < ALLOC_MANIFEST + ALLOC_FLAG {
        SRC_FLAG
    } else {
        SRC_ANCHOR
    }
}

/// 第 idx 样例的引用文本（确定性派生：条目号烙进文本，格式过闸）。
pub fn sample_reference(idx: usize) -> String {
    let src = sample_source(idx);
    match src {
        s if s == SRC_MANIFEST => format!("app{}-lie{}.manifest", idx % 16, idx),
        s if s == SRC_FLAG => format!("flag:lie{}mtx{}", idx % 8, idx),
        _ => format!("anch-{:04}-{}", idx, idx % 7),
    }
}

/// 第 idx 样例记录（直接可登记）。
pub fn sample_record(idx: usize) -> Result<BasisRecord, u32> {
    make_record(idx as u16, sample_source(idx), &sample_reference(idx))
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：无依据默认值（空引用）注入。
    NoBasisInject,
    /// 类1：坏引用文本（格式不合该源闸）注入。
    RefFormatInject,
    /// 类2：源越界注入。
    SourceOverflowInject,
    /// 类3：条目重号注入。
    DupEntryInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::NoBasisInject => 0,
            BadClass::RefFormatInject => 1,
            BadClass::SourceOverflowInject => 2,
            BadClass::DupEntryInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注：slot 越界静默忽略。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓判定。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数（必须为 0）。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入（对全新册执行，逐类断言显式拒）。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：无依据默认值——空引用必须 RefEmpty；空引用记录不得通过
    // basis_present（"无依据默认值注入必红"的直验）。
    let empty = make_record(0, SRC_MANIFEST, "");
    let empty2 = BasisRecord {
        entry_id: 0,
        source: SRC_FLAG,
        reference: String::new(),
        phase: BasisPhase::Verified,
    };
    if let Some(h) = hits.get_mut(BadClass::NoBasisInject.slot()) {
        *h = empty.is_err() && empty2.reference.is_empty() && !empty2.basis_present();
    }
    // 类1：坏引用文本——manifest 源给非 .manifest 后缀。
    let badref = make_record(0, SRC_MANIFEST, "notamanifest.txt");
    if let Some(h) = hits.get_mut(BadClass::RefFormatInject.slot()) {
        *h = badref.err() == Some(BrCode::RefFormatBad.code());
    }
    // 类2：源越界。
    let badsrc = make_record(0, BASIS_SOURCES as u8, "flag:whatever9");
    if let Some(h) = hits.get_mut(BadClass::SourceOverflowInject.slot()) {
        *h = badsrc.err() == Some(BrCode::SourceOutOfRange.code());
    }
    // 类3：条目重号——同号登记两次，第二次必拒。
    let mut reg = BasisRegistry::new();
    let r0 = sample_record(0);
    let dup_ok = match r0 {
        Ok(r) => {
            let first = reg.register(r);
            let second = match sample_record(0) {
                Ok(r2) => reg.register(r2),
                Err(_) => Ok(()),
            };
            first.is_ok() && second.err() == Some(BrCode::EntryDuplicated.code())
        }
        Err(_) => false,
    };
    if let Some(h) = hits.get_mut(BadClass::DupEntryInject.slot()) {
        *h = dup_ok;
    }
    hits
}

/// 注入 rounds 轮（判据 10/10 语义）。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：100 样本登记→核验→全册审计 100/100→注入全抓→守恒。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut reg = BasisRegistry::new();
    let mut idx = 0;
    while idx < SAMPLES {
        let rec = sample_record(idx)?;
        reg.register(rec)?;
        reg.verify(idx as u16)?;
        idx += 1;
    }
    let covered = reg.audit()?;
    if covered != SAMPLES {
        return Err(BrCode::SampleGap.code());
    }
    if !reg.conserve_ok() || reg.accepted != SAMPLES as u32 {
        return Err(BrCode::SampleGap.code());
    }
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(BrCode::NoBasisDefault.code());
    }
    Ok((covered, reg.accepted, reg.rejected))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BrCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：单样本登记→核验→basis_present 为真。
pub fn probe_a() -> Result<(), u32> {
    let mut reg = BasisRegistry::new();
    let rec = sample_record(3)?;
    reg.register(rec)?;
    reg.verify(3)?;
    match reg.find(3) {
        Some(r) if r.basis_present() => Ok(()),
        _ => Err(BrCode::NotVerified.code()),
    }
}

/// 探针B：登记后未核验即审计 → NotVerified 拒。
pub fn probe_b() -> Result<(), u32> {
    let mut reg = BasisRegistry::new();
    let rec = sample_record(5)?;
    reg.register(rec)?;
    match reg.audit() {
        Err(c) if c == BrCode::NoBasisDefault.code() => Ok(()),
        _ => Err(BrCode::NotVerified.code()),
    }
}

/// 探针C：条目号越界（100）显式拒。
pub fn probe_c() -> Result<(), u32> {
    let r = make_record(SAMPLES as u16, SRC_ANCHOR, "anch-9999-0");
    match r {
        Err(c) if c == BrCode::EntryOutOfRange.code() => Ok(()),
        _ => Err(BrCode::EntryOutOfRange.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < BrCode::ALL.len() {
        if let Some(c) = BrCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18423_checks：J1 判据 16 条（断言性质，动态明细 ≤8）。
pub fn run_unx18423_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18423 矩阵条目依据登记（三源）");
    // J1-1 三源闭集与分配常量勾稽。
    s.add(
        "J1-三源闭集",
        BASIS_SOURCES == 3
            && SRC_MANIFEST == 0
            && SRC_FLAG == 1
            && SRC_ANCHOR == 2
            && ALLOC_MANIFEST + ALLOC_FLAG + ALLOC_ANCHOR == SAMPLES,
        "manifest40 标志30 锚30 合100",
    );
    // J1-2 百样登记→核验→审计 100/100。
    let g = golden_run();
    s.add_dyn(
        "J1-百样100/100",
        matches!(&g, Ok((c, _, _)) if *c == SAMPLES),
        &format!("{:?}", g),
    );
    // J1-3 每条目依据字段非空（逐样直验，100 条逐一）。
    let all_present = {
        let mut ok = true;
        let mut i = 0;
        while i < SAMPLES {
            match sample_record(i) {
                Ok(mut r) => {
                    r.phase = BasisPhase::Verified;
                    if !r.basis_present() || r.reference.is_empty() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        ok
    };
    s.add("J1-依据非空100条", all_present, "100 样例逐条非空");
    // J1-4 无依据默认值注入必红：10 轮注入账全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-无依据注入必红10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒：漏检 0，总注入 40。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-6 引用格式三源闸逐源直验。
    let fmt_ok = ref_format_ok(SRC_MANIFEST, "a\\b lie0.manifest").is_ok()
        && ref_format_ok(SRC_FLAG, "flag:lie0mtx0").is_ok()
        && ref_format_ok(SRC_ANCHOR, "anch-0003-3").is_ok()
        && ref_format_ok(SRC_MANIFEST, "badref.txt").is_err()
        && ref_format_ok(SRC_FLAG, "nosuchprefix0").is_err();
    s.add("J1-引用格式闸", fmt_ok, "三源前/后缀闸逐源过");
    // J1-7 引用长度闸：下限 8 / 上限 64 两侧。
    let too_short = "flag:a1";
    let too_long = {
        let mut t = String::from("flag:");
        let mut k = 0;
        while k < REF_LEN_MAX {
            t.push('x');
            k += 1;
        }
        t
    };
    s.add(
        "J1-引用长度闸",
        ref_format_ok(SRC_FLAG, too_short).is_err()
            && ref_format_ok(SRC_FLAG, &too_long).is_err(),
        "短于8/长于64两侧拒",
    );
    // J1-8 条目重号拒（含拒因留账）。
    let dup_ledger = {
        let mut reg = BasisRegistry::new();
        if let Ok(r) = sample_record(1) {
            let _ = reg.register(r);
        }
        if let Ok(r2) = sample_record(1) {
            let _ = reg.register(r2);
        }
        reg.conserve_ok()
            && reg.rejected == 1
            && reg
                .reject_codes
                .first()
                .copied()
                .unwrap_or(0)
                == BrCode::EntryDuplicated.code()
    };
    s.add("J1-重号拒因留账", dup_ledger, "DupKey 拒因入账可复算");
    // J1-9 状态机：未核验不可用，Verified 后重核验拒（倒退）。
    let sm = {
        let mut reg = BasisRegistry::new();
        let a = match sample_record(2) {
            Ok(r) => reg.register(r).is_ok() && reg.audit().is_err(),
            Err(_) => false,
        };
        let b = reg.verify(2).is_ok() && reg.verify(2).is_err();
        a && b
    };
    s.add("J1-状态机单向", sm, "Registered→Verified 单向倒退拒");
    // J1-10 条目号越界拒。
    s.add("J1-条目号闭集", probe_c().is_ok(), "条目号 100 越界拒");
    // J1-11 探针三序。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "核验过/未核验红/越界拒");
    // J1-12 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-13 样例文本确定性：同号两次派生逐字节相等。
    s.add(
        "J2-样例确定性",
        sample_reference(7) == sample_reference(7) && sample_reference(7).len() >= REF_LEN_MIN,
        "同号派生逐字节相等",
    );
    // J1-14 源分配边界：39/40/69/70/99 五点落在各自源。
    let boundary = sample_source(39) == SRC_MANIFEST
        && sample_source(40) == SRC_FLAG
        && sample_source(69) == SRC_FLAG
        && sample_source(70) == SRC_ANCHOR
        && sample_source(99) == SRC_ANCHOR;
    s.add("J2-分配边界", boundary, "40/70 两界五点对源");
    // J1-15 文案闭集：12 码逐码非空。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < BrCode::ALL.len() {
            if let Some(c) = BrCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-16 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E580 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18423_checks();
        assert!(set.all_passed(), "UNX-F18423 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_100_samples_register_verify() {
        let mut reg = BasisRegistry::new();
        for i in 0..SAMPLES {
            let rec = sample_record(i).unwrap_or_else(|_| BasisRecord {
                entry_id: 0,
                source: 0,
                reference: "x.manifest".to_string(),
                phase: BasisPhase::Registered,
            });
            assert!(reg.register(rec).is_ok(), "样例 {} 登记失败", i);
            assert!(reg.verify(i as u16).is_ok(), "样例 {} 核验失败", i);
        }
        assert_eq!(reg.len(), SAMPLES);
        assert_eq!(reg.audit().unwrap_or(0), SAMPLES);
    }

    #[test]
    fn t_no_basis_injected_rejected() {
        assert_eq!(make_record(0, SRC_MANIFEST, "").err(), Some(BrCode::RefEmpty.code()));
        let fake = BasisRecord {
            entry_id: 1,
            source: SRC_FLAG,
            reference: String::new(),
            phase: BasisPhase::Verified,
        };
        assert!(!fake.basis_present());
    }

    #[test]
    fn t_format_gates_per_source() {
        assert!(ref_format_ok(SRC_MANIFEST, "lie0.manifest").is_ok());
        assert!(ref_format_ok(SRC_FLAG, "flag:lie0").is_ok());
        assert!(ref_format_ok(SRC_ANCHOR, "anch-0001-1").is_ok());
        assert_eq!(
            ref_format_ok(SRC_MANIFEST, "flag:lie0"),
            Err(BrCode::RefFormatBad.code())
        );
        assert_eq!(
            ref_format_ok(BASIS_SOURCES as u8, "flag:lie0"),
            Err(BrCode::SourceOutOfRange.code())
        );
    }

    #[test]
    fn t_duplicate_entry() {
        let mut reg = BasisRegistry::new();
        let r = sample_record(4).unwrap_or_else(|_| BasisRecord {
            entry_id: 4,
            source: 1,
            reference: "flag:dup4444".to_string(),
            phase: BasisPhase::Registered,
        });
        assert!(reg.register(r).is_ok());
        let r2 = sample_record(4).unwrap_or_else(|_| BasisRecord {
            entry_id: 4,
            source: 1,
            reference: "flag:dup4444".to_string(),
            phase: BasisPhase::Registered,
        });
        assert_eq!(reg.register(r2), Err(BrCode::EntryDuplicated.code()));
        assert!(reg.conserve_ok());
    }

    #[test]
    fn t_state_machine_single_direction() {
        let mut reg = BasisRegistry::new();
        let r = sample_record(9).unwrap_or_else(|_| BasisRecord {
            entry_id: 9,
            source: 2,
            reference: "anch-0009-2".to_string(),
            phase: BasisPhase::Registered,
        });
        let _ = reg.register(r);
        assert!(reg.verify(9).is_ok());
        assert_eq!(reg.verify(9), Err(BrCode::StateRegression.code()));
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..BAD_CLASSES {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
