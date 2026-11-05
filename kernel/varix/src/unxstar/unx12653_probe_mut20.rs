//! UNX-F12654 探测面注入联测（探测条件突变 20 例） · 0x80D40段
//!
//! 判据原文（派工卡 12653 逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12654-J1 20 例突变联测全过（版本/偏移/类号突变），
//!      检出断言零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 探测条件域模型：条件五元 =（主版本/次版本/构建号/字段偏移/信息类号）；
//!    参照剖面冻结（Win11 锚 10.0.22000 + 八类标准偏移表），探测条件必须
//!    与参照剖面完全对平，任何一路突变都不得蒙混过关；
//! 2. 突变三族闭集：版本突变族（主版本改写/次版本改写/构建回退/构建
//!    越界）、偏移突变族（偏移+1/偏移-1/偏移清零/偏移越界/偏移错类）、
//!    类号突变族（类号+1/类号-1/类号高位/类号越闭集/类号保留位），
//!    三族合计 20 例，例例手写不复用；
//! 3. 检出口单一权威：cond_detect() 对条件五元做三闸判定（版本闸/
//!    偏移闸/类号闸），突变例必须被至少一闸抓住；抓不住即静默，
//!    静默即判据失效；
//! 4. 检出断言零静默：20 例联测逐例记账（检出/逃脱），全过要求
//!    检出=20、逃脱=0、总=20 三数齐等；逃脱路必须给出可定位描述；
//! 5. 双跑一致：突变表为冻结常量面，两轮联测逐例同判；
//! 6. 状态机 待联测 → 联测中 → 联测封账 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12653 − 12600)）。
pub const SEG_BASE: u32 = 0x80D40;

// ── 联测诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutDiag {
    /// 版本闸未抓（版本突变蒙混）。
    EVerMiss,
    /// 偏移闸未抓（偏移突变蒙混）。
    EOffMiss,
    /// 类号闸未抓（类号突变蒙混）。
    EClassMiss,
    /// 突变例越三族闭集。
    ECaseRange,
    /// 参照剖面越剖面板（剖面账被改）。
    EProfileBad,
    /// 检出数不足（20 例未全过）。
    ENotAllCaught,
    /// 逃脱数非零（静默放行）。
    EEscape,
    /// 联测例号越序（记账乱序）。
    ECaseOrder,
    /// 未联测就试图封账。
    ENotRun,
    /// 封账后重跑被拒。
    ESealed,
    /// 状态机倒退。
    ERewind,
    /// 记账等式破坏（检出+逃脱≠总数）。
    EEqBroken,
}

impl MutDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MutDiag::EVerMiss => 0x01,
                MutDiag::EOffMiss => 0x02,
                MutDiag::EClassMiss => 0x03,
                MutDiag::ECaseRange => 0x04,
                MutDiag::EProfileBad => 0x05,
                MutDiag::ENotAllCaught => 0x06,
                MutDiag::EEscape => 0x07,
                MutDiag::ECaseOrder => 0x08,
                MutDiag::ENotRun => 0x09,
                MutDiag::ESealed => 0x0a,
                MutDiag::ERewind => 0x0b,
                MutDiag::EEqBroken => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MutDiag::EVerMiss => "版本闸未抓：版本突变蒙混过关，先核版本三元的闸口径再入账",
            MutDiag::EOffMiss => "偏移闸未抓：偏移突变蒙混过关，先核偏移与标准偏移表的对平口",
            MutDiag::EClassMiss => "类号闸未抓：类号突变蒙混过关，先核类号闭集闸的收口",
            MutDiag::ECaseRange => "突变例越三族闭集：只认版本/偏移/类号三族 20 例，越例先对突变表",
            MutDiag::EProfileBad => "参照剖面越剖面板：剖面账被改，剖面常量面只读，回滚后再联测",
            MutDiag::ENotAllCaught => "检出数不足：20 例未全过，按逃脱路描述回修对应闸口",
            MutDiag::EEscape => "逃脱数非零：静默放行即判据失效，逃脱路逐条补闸",
            MutDiag::ECaseOrder => "联测例号越序：记账按 0..20 逐例，跳号先回补",
            MutDiag::ENotRun => "未联测禁止封账：先跑满 20 例再走封账口",
            MutDiag::ESealed => "联测已封账：封账后重跑拒绝，改动走下一轮联测滚动",
            MutDiag::ERewind => "状态机倒退：待联测→联测中→封账只进不退，倒退即账本可疑",
            MutDiag::EEqBroken => "记账等式破坏：检出+逃脱≠总数，记账面先复核再入账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const MUT_ALL: &[MutDiag] = &[
    MutDiag::EVerMiss,
    MutDiag::EOffMiss,
    MutDiag::EClassMiss,
    MutDiag::ECaseRange,
    MutDiag::EProfileBad,
    MutDiag::ENotAllCaught,
    MutDiag::EEscape,
    MutDiag::ECaseOrder,
    MutDiag::ENotRun,
    MutDiag::ESealed,
    MutDiag::ERewind,
    MutDiag::EEqBroken,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in MUT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80D41..0x80D4C 闭集内"
}

// ── 探测条件与参照剖面 ───────────────────────────────────────────
/// 探测条件五元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeCond {
    /// 主版本。
    pub ver_major: u32,
    /// 次版本。
    pub ver_minor: u32,
    /// 构建号。
    pub ver_build: u32,
    /// 字段偏移。
    pub offset: u32,
    /// 信息类号。
    pub class: u32,
}

/// 参照剖面：版本锚 + 八类标准偏移表（冻结，只读）。
/// 版本锚取本批探测面约定档（10.0.22000）；偏移表为剖面约定值
/// （每类八字对齐递增，冻结不许重排）。
pub const REF_VER: (u32, u32, u32) = (10, 0, 22000);

/// 八类标准偏移表（类号 0..8，偏移冻结）。
pub const REF_OFFSETS: &[u32] = &[0x00, 0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38];

/// 参照剖面类闭集容量。
pub const CLASS_CAP: usize = 8;

/// 联测例总数（判据 20 例）。
pub const CASE_CAP: usize = 20;

/// 标准偏移查表（越类 None）。
pub fn ref_offset_of(class: u32) -> Option<u32> {
    REF_OFFSETS.get(class as usize).copied()
}

/// 参照剖面自检（剖面账未被改）。
pub fn profile_sound() -> bool {
    // 偏移表八字且逐行按 8 递增。
    if REF_OFFSETS.len() != CLASS_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < REF_OFFSETS.len() {
        let v = match REF_OFFSETS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v != (i * 8) as u32 {
            return false;
        }
        i += 1;
    }
    REF_VER.0 == 10 && REF_VER.1 == 0 && REF_VER.2 == 22000
}

// ── 检出口：三闸判定 ─────────────────────────────────────────────
/// 检出口：对条件五元做三闸判定。返回 Err(诊断码) 表示检出突变，
/// Ok(()) 表示条件与参照剖面完全对平。
pub fn cond_detect(c: &ProbeCond) -> Result<(), u32> {
    // 版本闸：三元必须与参照锚逐元相等。
    if c.ver_major != REF_VER.0 || c.ver_minor != REF_VER.1 || c.ver_build != REF_VER.2 {
        return Err(MutDiag::EVerMiss.val());
    }
    // 类号闸：类号必须落闭集。
    if c.class as usize >= CLASS_CAP {
        return Err(MutDiag::EClassMiss.val());
    }
    // 偏移闸：偏移必须等于该类标准偏移。
    match ref_offset_of(c.class) {
        Some(off) => {
            if c.offset != off {
                return Err(MutDiag::EOffMiss.val());
            }
        }
        None => return Err(MutDiag::EClassMiss.val()),
    }
    Ok(())
}

// ── 突变三族闭集（20 例手写，例号 0..20） ────────────────────────
/// 突变例：例号 + 族名 + 例名 + 突变语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutCase {
    /// 例号（0..20）。
    pub no: u32,
    /// 族名（版本/偏移/类号）。
    pub family: &'static str,
    /// 例名。
    pub name: &'static str,
    /// 期望抓闸（哪个闸必须抓到）。
    pub expect: MutDiag,
}

/// 突变表（20 例冻结，例序不许重排）。
pub const MUT_CASES: &[MutCase] = &[
    // ── 版本突变族（0..7） ──
    MutCase { no: 0, family: "版本", name: "主版本改10→9", expect: MutDiag::EVerMiss },
    MutCase { no: 1, family: "版本", name: "主版本改10→6", expect: MutDiag::EVerMiss },
    MutCase { no: 2, family: "版本", name: "次版本改0→1", expect: MutDiag::EVerMiss },
    MutCase { no: 3, family: "版本", name: "构建回退22000→19041", expect: MutDiag::EVerMiss },
    MutCase { no: 4, family: "版本", name: "构建回退22000→7601", expect: MutDiag::EVerMiss },
    MutCase { no: 5, family: "版本", name: "构建越界22000→99999", expect: MutDiag::EVerMiss },
    MutCase { no: 6, family: "版本", name: "构建清零", expect: MutDiag::EVerMiss },
    MutCase { no: 7, family: "版本", name: "版本三元全改", expect: MutDiag::EVerMiss },
    // ── 偏移突变族（8..13） ──
    MutCase { no: 8, family: "偏移", name: "偏移+1", expect: MutDiag::EOffMiss },
    MutCase { no: 9, family: "偏移", name: "偏移-1", expect: MutDiag::EOffMiss },
    MutCase { no: 10, family: "偏移", name: "偏移清零错类", expect: MutDiag::EOffMiss },
    MutCase { no: 11, family: "偏移", name: "偏移越界0x8000", expect: MutDiag::EOffMiss },
    MutCase { no: 12, family: "偏移", name: "偏移挪到邻类", expect: MutDiag::EOffMiss },
    MutCase { no: 13, family: "偏移", name: "偏移翻倍", expect: MutDiag::EOffMiss },
    // ── 类号突变族（14..19） ──
    MutCase { no: 14, family: "类号", name: "类号+1越闭集", expect: MutDiag::EClassMiss },
    MutCase { no: 15, family: "类号", name: "类号高位0x8000", expect: MutDiag::EClassMiss },
    MutCase { no: 16, family: "类号", name: "类号越闭集9999", expect: MutDiag::EClassMiss },
    MutCase { no: 17, family: "类号", name: "类号回绕0xFFFFFFFF", expect: MutDiag::EClassMiss },
    MutCase { no: 18, family: "类号", name: "类号改写换语义", expect: MutDiag::EOffMiss },
    MutCase { no: 19, family: "类号", name: "类号挪到末类", expect: MutDiag::EOffMiss },
];

/// 突变例越闭集守卫：例号必须落在 0..20。
pub fn case_in_table(no: u32) -> bool {
    MUT_CASES.iter().any(|c| c.no == no)
}

/// 按突变例号构造突变条件（例语义逐例手写，不参数化生成）。
pub fn mutated_cond(no: u32) -> Option<ProbeCond> {
    let base = ProbeCond {
        ver_major: REF_VER.0,
        ver_minor: REF_VER.1,
        ver_build: REF_VER.2,
        offset: 0x18,
        class: 3,
    };
    let c = match no {
        0 => ProbeCond { ver_major: 9, ..base },
        1 => ProbeCond { ver_major: 6, ..base },
        2 => ProbeCond { ver_minor: 1, ..base },
        3 => ProbeCond { ver_build: 19041, ..base },
        4 => ProbeCond { ver_build: 7601, ..base },
        5 => ProbeCond { ver_build: 99999, ..base },
        6 => ProbeCond { ver_build: 0, ..base },
        7 => ProbeCond { ver_major: 6, ver_minor: 3, ver_build: 9600, ..base },
        8 => ProbeCond { offset: 0x19, ..base },
        9 => ProbeCond { offset: 0x17, ..base },
        10 => ProbeCond { offset: 0x00, ..base },
        11 => ProbeCond { offset: 0x8000, ..base },
        12 => ProbeCond { offset: 0x20, ..base },
        13 => ProbeCond { offset: 0x30, ..base },
        14 => ProbeCond { class: 8, ..base },
        15 => ProbeCond { class: 0x8000, ..base },
        16 => ProbeCond { class: 9999, ..base },
        17 => ProbeCond { class: 0xFFFF_FFFF, ..base },
        18 => ProbeCond { class: 4, ..base },
        19 => ProbeCond { class: 7, ..base },
        _ => return None,
    };
    Some(c)
}

/// 参照条件（对平基准，未突变）。
pub fn reference_cond() -> ProbeCond {
    ProbeCond {
        ver_major: REF_VER.0,
        ver_minor: REF_VER.1,
        ver_build: REF_VER.2,
        offset: 0x18,
        class: 3,
    }
}

// ── 联测账数据模型 ───────────────────────────────────────────────
/// 联测记账一行：例号 / 例名 / 判定（检出/逃脱）。
#[derive(Clone, Debug)]
pub struct CaseRecord {
    /// 例号。
    pub no: u32,
    /// 例名。
    pub name: String,
    /// 是否检出。
    pub caught: bool,
    /// 实际抓闸码（检出时记录）。
    pub diag: u32,
}

/// 联测账状态机：待联测 → 联测中 → 联测封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepState {
    /// 待联测。
    Idle,
    /// 联测中：记账落行。
    Running,
    /// 联测封账：只读。
    Sealed,
}

impl SweepState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            SweepState::Idle => 0,
            SweepState::Running => 1,
            SweepState::Sealed => 2,
        }
    }
}

/// 注入联测账。
#[derive(Clone, Debug)]
pub struct SweepLedger {
    /// 记账行（按例号序）。
    records: Vec<CaseRecord>,
    /// 账本状态。
    state: SweepState,
}

impl SweepLedger {
    /// 开一本新联测账。
    pub fn new() -> SweepLedger {
        SweepLedger {
            records: Vec::new(),
            state: SweepState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SweepState {
        self.state
    }

    /// 记账行数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn record_at(&self, idx: usize) -> Option<&CaseRecord> {
        self.records.get(idx)
    }

    /// 开跑（Idle→Running，重复开跑拒绝）。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SweepState::Running.rank() {
            return Err(MutDiag::ERewind.val());
        }
        self.state = SweepState::Running;
        Ok(())
    }

    /// 逐例记账（例号序闸 + 记账等式闸）。
    pub fn record(&mut self, no: u32, caught: bool, diag: u32) -> Result<(), u32> {
        if self.state.rank() < SweepState::Running.rank() {
            return Err(MutDiag::ENotRun.val());
        }
        if self.state.rank() >= SweepState::Sealed.rank() {
            return Err(MutDiag::ESealed.val());
        }
        // 例号序闸。
        let expect = self.records.len() as u32;
        if no != expect {
            return Err(MutDiag::ECaseOrder.val());
        }
        // 例闭集闸。
        if no as usize >= CASE_CAP {
            return Err(MutDiag::ECaseRange.val());
        }
        let name = match MUT_CASES.get(no as usize) {
            Some(c) => c.name.to_string(),
            None => String::new(),
        };
        self.records.push(CaseRecord { no, name, caught, diag });
        Ok(())
    }

    /// 检出数。
    pub fn caught_count(&self) -> usize {
        let mut n = 0usize;
        for r in self.records.iter() {
            if r.caught {
                n += 1;
            }
        }
        n
    }

    /// 逃脱数。
    pub fn escape_count(&self) -> usize {
        self.records.len().saturating_sub(self.caught_count())
    }

    /// 记账等式：检出+逃脱 == 总数。
    pub fn equation_ok(&self) -> bool {
        self.caught_count() + self.escape_count() == self.records.len()
    }

    /// 首个逃脱路描述（无逃脱返回空串）。
    pub fn first_escape(&self) -> String {
        for r in self.records.iter() {
            if !r.caught {
                return format!("例{}({}) 逃脱 diag=0x{:X}", r.no, r.name, r.diag);
            }
        }
        String::new()
    }

    /// 封账：20 例齐 + 全检出 + 等式成立才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SweepState::Sealed.rank() {
            return Err(MutDiag::ERewind.val());
        }
        if self.state.rank() < SweepState::Running.rank() {
            return Err(MutDiag::ENotRun.val());
        }
        if self.records.len() != CASE_CAP {
            return Err(MutDiag::ENotAllCaught.val());
        }
        if !self.equation_ok() {
            return Err(MutDiag::EEqBroken.val());
        }
        if self.escape_count() != 0 {
            return Err(MutDiag::EEscape.val());
        }
        self.state = SweepState::Sealed;
        Ok(())
    }
}

// ── 全量联测（20 例跑满） ────────────────────────────────────────
/// 跑满 20 例联测：逐例构造突变条件 → 检出口判定 → 记账。
/// 返回（联测账, 首个逃脱路描述）。
pub fn run_full_sweep() -> (SweepLedger, String) {
    let mut l = SweepLedger::new();
    let _ = l.start();
    let mut i = 0u32;
    while i < CASE_CAP as u32 {
        // 突变例必须存在且必须被抓。
        let (caught, diag) = match mutated_cond(i) {
            Some(c) => match cond_detect(&c) {
                Err(d) => (true, d),
                Ok(()) => (false, 0),
            },
            None => (false, MutDiag::ECaseRange.val()),
        };
        // 期望抓闸核对：突变例的期望闸必须与实际抓闸一致（严检）。
        let expect_ok = match MUT_CASES.get(i as usize) {
            Some(mc) => !caught || diag == mc.expect.val(),
            None => false,
        };
        let verdict = caught && expect_ok;
        match l.record(i, verdict, diag) {
            Ok(()) => i += 1,
            Err(e) => {
                // 记账闸出错即账本坏，直接终止（断言会红）。
                let _ = e;
                break;
            }
        }
    }
    let esc = l.first_escape();
    (l, esc)
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 20 例突变联测全过（判据 J1 的"全过"直读口）。
fn sweep_all_caught() -> bool {
    let (l, esc) = run_full_sweep();
    if l.len() != CASE_CAP {
        return false;
    }
    if l.caught_count() != CASE_CAP {
        return false;
    }
    if l.escape_count() != 0 {
        return false;
    }
    if !esc.is_empty() {
        return false;
    }
    // 封账必须成功。
    let mut l2 = l;
    if l2.seal().is_err() {
        return false;
    }
    true
}

/// 检出断言零静默（检出记账逐例复核，静默即失效）。
fn no_silence_ok() -> bool {
    let (l, _esc) = run_full_sweep();
    if l.records.len() != CASE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < l.records.len() {
        match l.record_at(i) {
            Some(r) => {
                if !r.caught {
                    return false;
                }
                if r.no != i as u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 参照条件对平（未突变条件必须全闸通过——反向护栏）。
fn reference_ok() -> bool {
    let c = reference_cond();
    if cond_detect(&c).is_err() {
        return false;
    }
    // 参照剖面自检。
    if !profile_sound() {
        return false;
    }
    true
}

/// 双跑一致（冻结突变表下两轮联测逐例同判）。
fn dual_run_ok() -> bool {
    let (a, ea) = run_full_sweep();
    let (b, eb) = run_full_sweep();
    if a.len() != b.len() || a.caught_count() != b.caught_count() {
        return false;
    }
    if ea != eb {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.record_at(i), b.record_at(i)) {
            (Some(x), Some(y)) => {
                if x.caught != y.caught || x.diag != y.diag {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 边界：例号越序/越闭集/未跑记账/封后重跑 四点全拒。
fn boundary_ok() -> bool {
    let mut l = SweepLedger::new();
    // 未跑记账被拒。
    if l.record(0, true, 0).is_ok() {
        return false;
    }
    let _ = l.start();
    // 例号越序被拒。
    if l.record(1, true, 0).is_ok() {
        return false;
    }
    // 正常记第一例后例闭集越界被拒。
    let _ = l.record(0, true, MUT_CASES.get(0).map(|c| c.expect.val()).unwrap_or(0));
    if l.record(20, true, 0).is_ok() {
        return false;
    }
    // 重复开跑被拒。
    if l.start().is_ok() {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12653u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let (mut l, _esc) = run_full_sweep();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != SweepState::Sealed {
        return false;
    }
    match l.record_at(0) {
        Some(r) => {
            if r.name.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12654 20 例突变联测自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12653_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12653-probe-mut20");
    let (ledger, escape) = run_full_sweep();

    s.add_dyn(
        "F12654-01 突变表二十例齐",
        MUT_CASES.len() == CASE_CAP,
        &format!("cases={}", MUT_CASES.len()),
    );

    s.add_dyn(
        "F12654-02 20 例突变联测全过",
        sweep_all_caught(),
        &format!("caught={}/{} esc={}", ledger.caught_count(), ledger.len(), escape),
    );

    s.add_dyn(
        "F12654-03 检出断言零静默",
        no_silence_ok(),
        "逐例复核检出记账，零静默放行",
    );

    s.add_dyn(
        "F12654-04 参照条件对平",
        reference_ok(),
        "未突变条件全闸通过（反向护栏）",
    );

    s.add_dyn(
        "F12654-05 双跑一致",
        dual_run_ok(),
        "冻结突变表下两轮联测逐例同判",
    );

    s.add_dyn(
        "F12654-06 边界记账四闸",
        boundary_ok(),
        "未跑记账/例号越序/越闭集/重复开跑 全拒",
    );

    s.add_dyn(
        "F12654-07 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12654-08 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 突变三族闭集：版本 8 例 / 偏移 6 例 / 类号 6 例。
    let mut ver_n = 0usize;
    let mut off_n = 0usize;
    let mut cls_n = 0usize;
    for c in MUT_CASES.iter() {
        if c.family == "版本" {
            ver_n += 1;
        } else if c.family == "偏移" {
            off_n += 1;
        } else if c.family == "类号" {
            cls_n += 1;
        }
    }
    s.add_dyn(
        "F12654-09 突变三族闭集",
        ver_n == 8 && off_n == 6 && cls_n == 6,
        &format!("版本{} 偏移{} 类号{}", ver_n, off_n, cls_n),
    );

    // 突变表例号连续守卫（0..20 各一）。
    let mut seq_ok = true;
    for i in 0..CASE_CAP {
        match MUT_CASES.get(i) {
            Some(c) => {
                if c.no != i as u32 || !case_in_table(c.no) {
                    seq_ok = false;
                }
            }
            None => seq_ok = false,
        }
    }
    s.add_dyn(
        "F12654-10 突变表例号连续",
        seq_ok,
        "例号 0..20 各一按序",
    );

    // 记账等式：检出+逃脱=总数。
    s.add_dyn(
        "F12654-11 记账等式",
        ledger.equation_ok(),
        &format!("{}+{}={}", ledger.caught_count(), ledger.escape_count(), ledger.len()),
    );

    // 期望抓闸与实际抓闸逐例一致（严检直读）。
    let mut strict_ok = true;
    for i in 0..ledger.len() {
        match (ledger.record_at(i), MUT_CASES.get(i)) {
            (Some(r), Some(c)) => {
                if r.caught && r.diag != c.expect.val() {
                    strict_ok = false;
                }
            }
            _ => strict_ok = false,
        }
    }
    s.add_dyn(
        "F12654-12 期望抓闸逐例一致",
        strict_ok,
        "每例实际抓闸 = 期望抓闸",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80D40);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(MUT_ALL.len(), 12);
        let mut i = 0usize;
        while i < MUT_ALL.len() {
            let vi = MUT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < MUT_ALL.len() {
                let vj = MUT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn all_twenty_cases_caught() {
        // 注入必抓：20 例突变全数检出，无静默。
        let (l, esc) = run_full_sweep();
        assert_eq!(l.len(), 20);
        assert_eq!(l.caught_count(), 20, "逃脱路：{}", esc);
        assert!(esc.is_empty());
        assert!(l.equation_ok());
    }

    #[test]
    fn every_case_hits_expected_gate() {
        // 严检：每例实际抓闸必须等于期望抓闸。
        let mut i = 0u32;
        while i < CASE_CAP as u32 {
            let opt = mutated_cond(i);
            assert!(opt.is_some(), "例 {} 缺突变构造", i);
            let c = match opt {
                Some(c) => c,
                None => reference_cond(),
            };
            let res = cond_detect(&c);
            assert!(res.is_err(), "例 {} 未检出", i);
            let d = match res {
                Err(d) => d,
                Ok(()) => 0,
            };
            let mc = match MUT_CASES.get(i as usize) {
                Some(mc) => mc,
                None => {
                    assert!(false, "例 {} 缺表行", i);
                    break;
                }
            };
            assert_eq!(d, mc.expect.val(), "例 {} 抓闸不符", i);
            i += 1;
        }
    }

    #[test]
    fn reference_cond_passes_all_gates() {
        // 反向护栏：未突变参照条件必须全闸通过。
        assert!(cond_detect(&reference_cond()).is_ok());
        assert!(profile_sound());
    }

    #[test]
    fn family_census_is_closed() {
        // 三族闭集：版本 8 + 偏移 6 + 类号 6 = 20。
        let mut ver = 0usize;
        let mut off = 0usize;
        let mut cls = 0usize;
        for c in MUT_CASES.iter() {
            match c.family {
                "版本" => ver += 1,
                "偏移" => off += 1,
                "类号" => cls += 1,
                _ => assert!(false, "越族名"),
            }
        }
        assert_eq!(ver, 8);
        assert_eq!(off, 6);
        assert_eq!(cls, 6);
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：冻结突变表下两轮联测逐例同判。
        let (a, ea) = run_full_sweep();
        let (b, eb) = run_full_sweep();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.caught_count(), b.caught_count());
        assert_eq!(ea, eb);
        let mut i = 0usize;
        while i < a.len() {
            match (a.record_at(i), b.record_at(i)) {
                (Some(x), Some(y)) => {
                    assert_eq!(x.caught, y.caught);
                    assert_eq!(x.diag, y.diag);
                }
                (None, None) => {}
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
    }

    #[test]
    fn ledger_gates_reject() {
        // 未跑记账被拒。
        let mut l = SweepLedger::new();
        assert_eq!(l.record(0, true, 0), Err(MutDiag::ENotRun.val()));
        // 开跑后例号越序被拒。
        let _ = l.start();
        assert_eq!(l.record(1, true, 0), Err(MutDiag::ECaseOrder.val()));
        // 正常记一例后越序被拒（越序闸先于越闭集闸）。
        let _ = l.record(0, true, 0);
        assert_eq!(l.record(20, true, 0), Err(MutDiag::ECaseOrder.val()));
        // 重复开跑被拒。
        assert_eq!(l.start(), Err(MutDiag::ERewind.val()));
    }

    #[test]
    fn seal_requires_all_caught() {
        // 边界：20 例未齐禁止封账；有逃脱禁止封账。
        let mut half = SweepLedger::new();
        let _ = half.start();
        let _ = half.record(0, true, 0);
        assert_eq!(half.seal(), Err(MutDiag::ENotAllCaught.val()));
        // 有逃脱账本封账被拒。
        let mut esc_l = SweepLedger::new();
        let _ = esc_l.start();
        let _ = esc_l.record(0, false, 0);
        assert_eq!(esc_l.seal(), Err(MutDiag::ENotAllCaught.val()));
        // 全检出账本封账成功，重封倒退拒绝。
        let (mut full, esc) = run_full_sweep();
        assert!(esc.is_empty());
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(MutDiag::ERewind.val()));
    }

    #[test]
    fn mutation_table_is_frozen() {
        // 突变表冻结：例号连续、族名非空、例名非空。
        assert_eq!(MUT_CASES.len(), CASE_CAP);
        let mut i = 0usize;
        while i < MUT_CASES.len() {
            match MUT_CASES.get(i) {
                Some(c) => {
                    assert_eq!(c.no, i as u32, "例 {} 号漂移", i);
                    assert!(!c.family.is_empty());
                    assert!(!c.name.is_empty());
                    assert!(case_in_table(c.no));
                }
                None => assert!(false, "突变表缺行"),
            }
            i += 1;
        }
        assert!(!case_in_table(20));
        assert!(!case_in_table(999));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Idle→Running→Sealed。
        let (mut l, _esc) = run_full_sweep();
        assert!(SweepState::Idle.rank() < SweepState::Running.rank());
        assert!(SweepState::Running.rank() < SweepState::Sealed.rank());
        assert_eq!(l.state(), SweepState::Running);
        let _ = l.seal();
        assert_eq!(l.state(), SweepState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12653_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
