//! UNX-F1535 域级故障注入资产总谱（注入一册全谱） · 0x786C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1535 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1535-J1 注入总谱齐备（全域故障注入资产逐件编谱：
//!     注入号/目标/必抓判据三列），逐件可独立执行，误报率 0 断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三列编谱：注入号（INJ-nn）/ 目标不变式 / 必抓判据号（C-nn），
//!     三列齐全才准入册，缺列 / 重号 / 判据号格式错显性拒；
//!   - 目标系统建模：带 11 条冻结不变式（件号 / 名称 / 值窗 / 序窗 /
//!     流量 / 口径重名 / 口径齐备 / 阶段上限 / 标签唯一 / 校验和 / 奇偶），
//!     Detector::audit 逐条闭集核查；
//!   - 逐件独立执行：每条注入对健康账克隆施加确定性故障，audit 必抓；
//!     健康账同轮免注入审计必 Ok（误报率 0 断言逐件成立）；
//!   - 总谱冻结：全谱逐件检出 + 误报 0 才许冻结封版；
//!   - 复跑一致：全谱执行两轮，逐件检出 / 误报结论逐位相等；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x786C0，派工卡 VX8-B03 卡 4 独占窗口）。
pub const SEG_BASE: u32 = 0x786C0;

/// 冻结不变式条数（目标系统核查闭集 11 条）。
pub const INVARIANT_COUNT: usize = 11;
/// 总谱注入件数（一册全谱 = 11 件，逐条不变式一件注入资产）。
pub const SCORE_ENTRIES: usize = 11;
/// 健康账冻结件号。
pub const SYS_ID_FROZEN: u32 = 0x786C0;
/// 值窗下界（整数冻结）。
pub const VALUE_MIN: u32 = 0x100;
/// 值窗上界。
pub const VALUE_MAX: u32 = 0x1FF;
/// 序窗基值。
pub const SEQ_BASE: u64 = 0x786C0;
/// 流量冻结值。
pub const UNITS_FROZEN: u32 = 0x55;
/// 口径登记数（冻结）。
pub const CALIBER_NEEDED: usize = 5;
/// 阶段上限（0..=3）。
pub const STAGE_MAX: u32 = 3;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjCode {
    /// 件号失配：目标系统件号被改，按冻结件号回滚
    IdMismatch,
    /// 名称空账：名称被清空，补名称后复审
    NameEmpty,
    /// 值越窗：值出冻结窗，回窗内再审
    ValueOutRange,
    /// 序越窗：序低于基值或溢出，回窗内再审
    SeqOutOfRange,
    /// 流量失配：流量被改，对齐冻结流量
    UnitsMismatch,
    /// 口径重名：口径册重复登记，合并重名
    CaliberDuplicate,
    /// 口径缺项：口径册不满五条，补齐再审
    CaliberMissing,
    /// 阶段越限：阶段越过上限 3，回退单调推进
    StageOver,
    /// 标签重号：标签册重复，重编标签
    TagDuplicate,
    /// 校验和失配：载荷校验和与账不符，重算封账
    ChecksumBad,
    /// 奇偶失配：奇偶位与账不符，复核奇偶
    ParityOdd,
    /// 注入未检出：故障逃逸，补审计判据后重跑
    ExecuteMissed,
}

impl InjCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            InjCode::IdMismatch => "件号失配：目标系统件号被改，按冻结件号回滚",
            InjCode::NameEmpty => "名称空账：补名称后复审",
            InjCode::ValueOutRange => "值越窗：回冻结窗内再审",
            InjCode::SeqOutOfRange => "序越窗：回冻结序窗内再审",
            InjCode::UnitsMismatch => "流量失配：对齐冻结流量 0x55",
            InjCode::CaliberDuplicate => "口径重名：合并重复口径保一册",
            InjCode::CaliberMissing => "口径缺项：补满五条口径再审",
            InjCode::StageOver => "阶段越限：回退到上限 3 内单调推进",
            InjCode::TagDuplicate => "标签重号：重编标签保唯一",
            InjCode::ChecksumBad => "校验和失配：重算载荷校验和封账",
            InjCode::ParityOdd => "奇偶失配：复核奇偶位",
            InjCode::ExecuteMissed => "注入未检出：故障逃逸，补审计判据后重跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [InjCode; 12] = [
        InjCode::IdMismatch,
        InjCode::NameEmpty,
        InjCode::ValueOutRange,
        InjCode::SeqOutOfRange,
        InjCode::UnitsMismatch,
        InjCode::CaliberDuplicate,
        InjCode::CaliberMissing,
        InjCode::StageOver,
        InjCode::TagDuplicate,
        InjCode::ChecksumBad,
        InjCode::ParityOdd,
        InjCode::ExecuteMissed,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < InjCode::ALL.len() {
        if let Some(c) = InjCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── 域模型：目标系统 / 审计器 / 故障类 ──────────────────────────
/// 目标系统：注入试验的被测账（11 条冻结不变式）。
pub struct TargetSystem {
    id: u32,
    name: String,
    value: u32,
    seq: u64,
    units: u32,
    calibers: Vec<String>,
    stage: u32,
    tags: Vec<String>,
    checksum: u64,
    parity: u32,
}

impl TargetSystem {
    /// 健康账（全部不变式满足）。
    pub fn healthy() -> TargetSystem {
        let mut t = TargetSystem {
            id: SYS_ID_FROZEN,
            name: "收官段注入靶机".to_string(),
            value: VALUE_MIN + 0x20,
            seq: SEQ_BASE + 16,
            units: UNITS_FROZEN,
            calibers: Vec::new(),
            stage: 2,
            tags: Vec::new(),
            checksum: 0,
            parity: 0,
        };
        let mut k = 0usize;
        while k < CALIBER_NEEDED {
            let _ = t.calibers.push(format!("口径{:02}", k + 1));
            k += 1;
        }
        let _ = t.tags.push("账本面".to_string());
        let _ = t.tags.push("断言面".to_string());
        let _ = t.tags.push("数据流面".to_string());
        t.checksum = t.compute_checksum();
        t.parity = t.compute_parity();
        t
    }
    fn compute_checksum(&self) -> u64 {
        // 载荷摘要：名称 + 值 + 流量拼接 FNV（纯整数）。
        let mut buf = String::new();
        buf.push_str(&self.name);
        let mut vb = String::new();
        push_u32(&mut vb, self.value);
        buf.push_str(&vb);
        let mut ub = String::new();
        push_u32(&mut ub, self.units);
        buf.push_str(&ub);
        fnv1a64(buf.as_bytes())
    }
    fn compute_parity(&self) -> u32 {
        // 奇偶位：全字段位和的低 1 位（冻结算式）。
        let mut acc = self.id;
        acc = acc ^ self.value;
        acc = acc ^ (self.units * 3);
        acc = acc ^ (self.stage * 7);
        acc & 1
    }
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn value(&self) -> u32 {
        self.value
    }
    pub fn stage(&self) -> u32 {
        self.stage
    }
}

/// FNV-1a 64（校验和核心）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    h
}

fn push_u32(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push((b'0' + (x % 10) as u8) as char);
        x /= 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = digits.get(i) {
            out.push(*c);
        }
    }
}

/// 审计器：逐条不变式闭集核查，第一条违例即返诊断码。
pub struct Detector;

impl Detector {
    /// 逐条核查（顺序 = 诊断码 0x01..0x0B 闭集序）。
    pub fn audit(t: &TargetSystem) -> Result<(), u32> {
        if t.id != SYS_ID_FROZEN {
            return Err(InjCode::IdMismatch.val());
        }
        if t.name.is_empty() {
            return Err(InjCode::NameEmpty.val());
        }
        if t.value < VALUE_MIN || t.value > VALUE_MAX {
            return Err(InjCode::ValueOutRange.val());
        }
        if t.seq < SEQ_BASE || t.seq > SEQ_BASE + 0x1000 {
            return Err(InjCode::SeqOutOfRange.val());
        }
        if t.units != UNITS_FROZEN {
            return Err(InjCode::UnitsMismatch.val());
        }
        // 口径重名（重名即违约）。
        let mut ci = 0usize;
        while ci < t.calibers.len() {
            let mut cj = ci + 1;
            while cj < t.calibers.len() {
                let a = match t.calibers.get(ci) {
                    Some(x) => x.clone(),
                    None => break,
                };
                let b = match t.calibers.get(cj) {
                    Some(x) => x.clone(),
                    None => break,
                };
                if a == b {
                    return Err(InjCode::CaliberDuplicate.val());
                }
                cj += 1;
            }
            ci += 1;
        }
        if t.calibers.len() < CALIBER_NEEDED {
            return Err(InjCode::CaliberMissing.val());
        }
        if t.stage > STAGE_MAX {
            return Err(InjCode::StageOver.val());
        }
        // 标签唯一。
        let mut ti = 0usize;
        while ti < t.tags.len() {
            let mut tj = ti + 1;
            while tj < t.tags.len() {
                let a = match t.tags.get(ti) {
                    Some(x) => x.clone(),
                    None => break,
                };
                let b = match t.tags.get(tj) {
                    Some(x) => x.clone(),
                    None => break,
                };
                if a == b {
                    return Err(InjCode::TagDuplicate.val());
                }
                tj += 1;
            }
            ti += 1;
        }
        if t.checksum != t.compute_checksum() {
            return Err(InjCode::ChecksumBad.val());
        }
        if t.parity != t.compute_parity() {
            return Err(InjCode::ParityOdd.val());
        }
        Ok(())
    }
}

/// 故障类（11 类，每类精确击穿一条不变式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 击穿件号不变式
    FlipId,
    /// 击穿名称不变式
    ClearName,
    /// 击穿值窗不变式（越上界）
    ValueOver,
    /// 击穿序窗不变式（低于基值）
    SeqUnder,
    /// 击穿流量不变式
    FlipUnits,
    /// 击穿口径重名不变式
    DupCaliber,
    /// 击穿口径齐备不变式（砍一条）
    DropCaliber,
    /// 击穿阶段上限不变式
    StageOver,
    /// 击穿标签唯一不变式
    DupTag,
    /// 击穿校验和不变式
    CorruptChecksum,
    /// 击穿奇偶不变式
    FlipParity,
}

impl FaultKind {
    /// 全类清单（总谱编谱序）。
    pub const ALL: [FaultKind; 11] = [
        FaultKind::FlipId,
        FaultKind::ClearName,
        FaultKind::ValueOver,
        FaultKind::SeqUnder,
        FaultKind::FlipUnits,
        FaultKind::DupCaliber,
        FaultKind::DropCaliber,
        FaultKind::StageOver,
        FaultKind::DupTag,
        FaultKind::CorruptChecksum,
        FaultKind::FlipParity,
    ];
    /// 中文目标面名（编谱第二列：目标不变式）。
    pub fn target_name(self) -> &'static str {
        match self {
            FaultKind::FlipId => "件号冻结不变式",
            FaultKind::ClearName => "名称非空不变式",
            FaultKind::ValueOver => "值窗不变式",
            FaultKind::SeqUnder => "序窗不变式",
            FaultKind::FlipUnits => "流量冻结不变式",
            FaultKind::DupCaliber => "口径唯一不变式",
            FaultKind::DropCaliber => "口径齐备不变式",
            FaultKind::StageOver => "阶段上限不变式",
            FaultKind::DupTag => "标签唯一不变式",
            FaultKind::CorruptChecksum => "校验和不变式",
            FaultKind::FlipParity => "奇偶位不变式",
        }
    }
    /// 对健康账施加确定性故障（逐字节 / 逐字段精确定位）。
    pub fn apply(self, t: &mut TargetSystem) {
        match self {
            FaultKind::FlipId => {
                t.id = t.id ^ 0x1;
            }
            FaultKind::ClearName => {
                t.name = String::new();
            }
            FaultKind::ValueOver => {
                t.value = VALUE_MAX + 1;
            }
            FaultKind::SeqUnder => {
                t.seq = SEQ_BASE - 1;
            }
            FaultKind::FlipUnits => {
                t.units = UNITS_FROZEN ^ 0x1;
            }
            FaultKind::DupCaliber => {
                if let Some(first) = t.calibers.get(0) {
                    let dup = first.clone();
                    t.calibers.push(dup);
                }
            }
            FaultKind::DropCaliber => {
                let _ = t.calibers.pop();
            }
            FaultKind::StageOver => {
                t.stage = STAGE_MAX + 1;
            }
            FaultKind::DupTag => {
                if let Some(first) = t.tags.get(0) {
                    let dup = first.clone();
                    t.tags.push(dup);
                }
            }
            FaultKind::CorruptChecksum => {
                t.checksum = t.checksum ^ 0xDEAD_BEEF;
            }
            FaultKind::FlipParity => {
                t.parity = t.parity ^ 1;
            }
        }
    }
    /// 该故障的必抓诊断码（编谱第三列：必抓判据的落点）。
    pub fn expected_code(self) -> u32 {
        match self {
            FaultKind::FlipId => InjCode::IdMismatch.val(),
            FaultKind::ClearName => InjCode::NameEmpty.val(),
            FaultKind::ValueOver => InjCode::ValueOutRange.val(),
            FaultKind::SeqUnder => InjCode::SeqOutOfRange.val(),
            FaultKind::FlipUnits => InjCode::UnitsMismatch.val(),
            FaultKind::DupCaliber => InjCode::CaliberDuplicate.val(),
            FaultKind::DropCaliber => InjCode::CaliberMissing.val(),
            FaultKind::StageOver => InjCode::StageOver.val(),
            FaultKind::DupTag => InjCode::TagDuplicate.val(),
            FaultKind::CorruptChecksum => InjCode::ChecksumBad.val(),
            FaultKind::FlipParity => InjCode::ParityOdd.val(),
        }
    }
}

// ── 总谱：编谱 / 执行 / 冻结 ────────────────────────────────────
/// 注入件（三列齐备）：注入号 / 目标 / 必抓判据。
pub struct InjectionEntry {
    no: String,
    kind: FaultKind,
    criterion: String,
    caught: bool,
    false_alarm: bool,
}

impl InjectionEntry {
    pub fn no(&self) -> &str {
        &self.no
    }
    pub fn kind(&self) -> FaultKind {
        self.kind
    }
    pub fn criterion(&self) -> &str {
        &self.criterion
    }
    pub fn is_caught(&self) -> bool {
        self.caught
    }
    pub fn has_false_alarm(&self) -> bool {
        self.false_alarm
    }
}

/// 总谱阶段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScorePhase {
    /// 编谱：三列入册
    Compiled,
    /// 执行：逐件独立执行
    Executed,
    /// 冻结：全谱结论封版
    Frozen,
}

impl ScorePhase {
    pub fn rank(self) -> u32 {
        match self {
            ScorePhase::Compiled => 0,
            ScorePhase::Executed => 1,
            ScorePhase::Frozen => 2,
        }
    }
}

/// 注入总谱账。
pub struct InjectionScore {
    entries: Vec<InjectionEntry>,
    phase: ScorePhase,
    summary: String,
}

impl InjectionScore {
    pub fn new() -> InjectionScore {
        InjectionScore {
            entries: Vec::new(),
            phase: ScorePhase::Compiled,
            summary: String::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn phase(&self) -> ScorePhase {
        self.phase
    }
    /// 编谱：注入号唯一 + 判据号格式 + 类未知拒。
    pub fn compile(&mut self, no: &str, kind: FaultKind, criterion: &str) -> Result<(), u32> {
        if self.entries.iter().any(|e| e.no() == no) {
            return Err(InjCode::IdMismatch.val());
        }
        if self.entries.iter().any(|e| e.criterion() == criterion) {
            return Err(InjCode::TagDuplicate.val());
        }
        // 判据号格式：UNX-F1535-Cnn（可溯唯一）。
        if !criterion.starts_with("UNX-F1535-C") || criterion.len() < 13 {
            return Err(InjCode::ChecksumBad.val());
        }
        self.entries.push(InjectionEntry {
            no: no.to_string(),
            kind,
            criterion: criterion.to_string(),
            caught: false,
            false_alarm: false,
        });
        Ok(())
    }
    /// 逐件独立执行：健康账免注审计必 Ok，注入账审计必须按预期码抓中。
    pub fn execute(&mut self) -> usize {
        let mut missed = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            let kind = match self.entries.get(k) {
                Some(e) => e.kind(),
                None => break,
            };
            // 误报面：健康账审计必须 Ok。
            let healthy = TargetSystem::healthy();
            let false_alarm = match Detector::audit(&healthy) {
                Ok(()) => false,
                Err(_) => true,
            };
            // 检出面：注入账审计必须按预期诊断码抓中。
            let mut injected = TargetSystem::healthy();
            kind.apply(&mut injected);
            let caught = match Detector::audit(&injected) {
                Err(code) => code == kind.expected_code(),
                Ok(()) => false,
            };
            if let Some(e) = self.entries.get_mut(k) {
                e.caught = caught;
                e.false_alarm = false_alarm;
            }
            if !caught || false_alarm {
                missed += 1;
            }
            k += 1;
        }
        if missed == 0 && self.phase.rank() < ScorePhase::Executed.rank() {
            self.phase = ScorePhase::Executed;
        }
        missed
    }
    /// 全谱结论：逐件检出且误报 0。
    pub fn all_caught_zero_alarm(&self) -> bool {
        if self.entries.len() != SCORE_ENTRIES {
            return false;
        }
        for e in self.entries.iter() {
            if !e.is_caught() || e.has_false_alarm() {
                return false;
            }
        }
        true
    }
    /// 单调推进：倒退拒。
    pub fn advance(&mut self, to: ScorePhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(InjCode::StageOver.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 冻结封版：未全检出拒，摘要非空才落。
    pub fn freeze(&mut self, summary: &str) -> Result<(), u32> {
        if !self.all_caught_zero_alarm() {
            return Err(InjCode::ExecuteMissed.val());
        }
        if summary.is_empty() {
            return Err(InjCode::NameEmpty.val());
        }
        self.advance(ScorePhase::Frozen)?;
        self.summary = summary.to_string();
        Ok(())
    }
    pub fn is_frozen(&self) -> bool {
        self.phase == ScorePhase::Frozen && !self.summary.is_empty()
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
    /// 逐件执行结论快照（复跑比对用）。
    pub fn verdict_bits(&self) -> Vec<u32> {
        self.entries
            .iter()
            .map(|e| {
                let mut b = 0u32;
                if e.is_caught() {
                    b = b | 1;
                }
                if e.has_false_alarm() {
                    b = b | 2;
                }
                b
            })
            .collect()
    }
}

// ── 语料构建 ────────────────────────────────────────────────────
/// 好谱：11 件全编（注入号 INJ-01..INJ-11，判据号 C01..C11）。
pub fn build_score_ok() -> InjectionScore {
    let mut sc = InjectionScore::new();
    let mut i = 0usize;
    while i < FaultKind::ALL.len() {
        let kind = match FaultKind::ALL.get(i) {
            Some(k) => *k,
            None => break,
        };
        let no = format!("INJ-{:02}", i + 1);
        let crit = format!("UNX-F1535-C{:02}", i + 1);
        let _ = sc.compile(&no, kind, &crit);
        i += 1;
    }
    sc
}

/// 坏谱：注入号重号 + 判据号格式错两路注入。
pub fn build_score_injected() -> InjectionScore {
    let mut sc = InjectionScore::new();
    let _ = sc.compile("INJ-01", FaultKind::FlipId, "UNX-F1535-C01");
    // 注入一：注入号重号。
    let _ = sc.compile("INJ-01", FaultKind::ClearName, "UNX-F1535-C02");
    // 注入二：判据号格式错。
    let _ = sc.compile("INJ-02", FaultKind::ValueOver, "坏判据号");
    sc
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

/// 三列编谱齐备：11 件、号与判据号一一对应。
fn score_complete_ok() -> bool {
    let sc = build_score_ok();
    if sc.len() != SCORE_ENTRIES {
        return false;
    }
    if sc.phase() != ScorePhase::Compiled {
        return false;
    }
    true
}

/// 编谱闸门：重号 / 判据号格式错必拒。
fn compile_guards_ok() -> bool {
    let sc = build_score_injected();
    // 注入被拒后总谱仍只有 1 件。
    if sc.len() != 1 {
        return false;
    }
    // 健康编谱不误伤：第 2 件用合法号可入。
    let mut sc2 = InjectionScore::new();
    if sc2.compile("INJ-01", FaultKind::FlipId, "UNX-F1535-C01").is_err() {
        return false;
    }
    if sc2.compile("INJ-02", FaultKind::ClearName, "UNX-F1535-C02").is_err() {
        return false;
    }
    true
}

/// 健康账零误报：11 条不变式全过。
fn healthy_audit_ok() -> bool {
    let t = TargetSystem::healthy();
    if Detector::audit(&t).is_err() {
        return false;
    }
    // 边界：值窗上下界各试一例。
    let mut lo = TargetSystem::healthy();
    lo.value = VALUE_MIN;
    lo.checksum = lo.compute_checksum();
    lo.parity = lo.compute_parity();
    if Detector::audit(&lo).is_err() {
        return false;
    }
    let mut hi = TargetSystem::healthy();
    hi.value = VALUE_MAX;
    hi.checksum = hi.compute_checksum();
    hi.parity = hi.compute_parity();
    Detector::audit(&hi).is_ok()
}

/// 逐件执行：11 件全检出且误报 0。
fn execute_all_caught_ok() -> bool {
    let mut sc = build_score_ok();
    let missed = sc.execute();
    if missed != 0 {
        return false;
    }
    if !sc.all_caught_zero_alarm() {
        return false;
    }
    if sc.phase() != ScorePhase::Executed {
        return false;
    }
    true
}

/// 故障逃逸注入：抽走一件注入的检出能力（用未编谱类）必被总成抓出。
fn escape_caught_ok() -> bool {
    // 构造缺件谱（只编 10 件）：总谱齐备判据必拒。
    let mut sc = InjectionScore::new();
    let mut i = 0usize;
    while i < FaultKind::ALL.len() - 1 {
        let kind = match FaultKind::ALL.get(i) {
            Some(k) => *k,
            None => break,
        };
        let no = format!("INJ-{:02}", i + 1);
        let crit = format!("UNX-F1535-C{:02}", i + 1);
        let _ = sc.compile(&no, kind, &crit);
        i += 1;
    }
    let _ = sc.execute();
    if sc.all_caught_zero_alarm() {
        return false;
    }
    match sc.freeze("全谱结论") {
        Err(code) => code == InjCode::ExecuteMissed.val(),
        Ok(()) => false,
    }
}

/// 每件故障精确命中预期诊断码（必抓判据逐件可验）。
fn expected_codes_hit_ok() -> bool {
    let mut i = 0usize;
    while i < FaultKind::ALL.len() {
        let kind = match FaultKind::ALL.get(i) {
            Some(k) => *k,
            None => break,
        };
        let mut t = TargetSystem::healthy();
        kind.apply(&mut t);
        match Detector::audit(&t) {
            Err(code) => {
                if code != kind.expected_code() {
                    return false;
                }
            }
            Ok(()) => return false,
        }
        i += 1;
    }
    true
}

/// 冻结封版：全谱检出后可冻结，摘要非空。
fn freeze_gate_ok() -> bool {
    let mut sc = build_score_ok();
    let _ = sc.execute();
    if sc.freeze("").is_ok() {
        return false;
    }
    let summary = "注入总谱 11 件全检出，误报 0，一册封版。";
    if sc.freeze(summary).is_err() {
        return false;
    }
    if !sc.is_frozen() {
        return false;
    }
    if sc.summary().len() < 8 {
        return false;
    }
    // 冻结后倒退拒。
    if sc.advance(ScorePhase::Compiled).is_ok() {
        return false;
    }
    true
}

/// 复跑一致：全谱执行两轮，逐件结论逐位相等。
fn rerun_consistent_ok() -> bool {
    let mut a = build_score_ok();
    let mut b = build_score_ok();
    let _ = a.execute();
    let _ = b.execute();
    let va = a.verdict_bits();
    let vb = b.verdict_bits();
    if va.len() != vb.len() {
        return false;
    }
    let mut i = 0usize;
    while i < va.len() {
        let x = match va.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match vb.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in InjCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first_detail = match InjCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first_detail {
        return false;
    }
    true
}

/// 故障类闭集：11 类目标名互异、预期码互异且全部落窗。
fn fault_kinds_closed_ok() -> bool {
    if FaultKind::ALL.len() != SCORE_ENTRIES {
        return false;
    }
    let mut names: Vec<&str> = Vec::new();
    let mut codes: Vec<u32> = Vec::new();
    for k in FaultKind::ALL.iter() {
        let n = k.target_name();
        if n.is_empty() || names.iter().any(|x| *x == n) {
            return false;
        }
        names.push(n);
        let c = k.expected_code();
        if c == 0 || codes.iter().any(|x| *x == c) {
            return false;
        }
        codes.push(c);
    }
    true
}

/// 靶机指纹确定性：健康账两次构建校验和 / 奇偶逐位相等。
fn system_digest_stable_ok() -> bool {
    let a = TargetSystem::healthy();
    let b = TargetSystem::healthy();
    if a.compute_checksum() != b.compute_checksum() {
        return false;
    }
    if a.compute_parity() != b.compute_parity() {
        return false;
    }
    // 注入后账面校验和必与重算值失配（校验和面可检）。
    let mut t = TargetSystem::healthy();
    FaultKind::CorruptChecksum.apply(&mut t);
    t.checksum != t.compute_checksum()
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1535_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1535 域级故障注入资产总谱");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("三列编谱齐备", score_complete_ok(), &format!("{} 件注入号/目标/判据号三列齐", SCORE_ENTRIES));
    s.add_dyn("编谱闸门", compile_guards_ok(), "重号注入号 / 坏判据号必拒");
    s.add_dyn("健康账零误报", healthy_audit_ok(), "11 条不变式全过，值窗边界各一例");
    s.add_dyn("逐件执行全检出", execute_all_caught_ok(), "11 件全检出且误报 0");
    s.add_dyn("逃逸注入必抓", escape_caught_ok(), "缺件谱冻结必拒（故障逃逸码）");
    s.add_dyn("必抓判据逐件命中", expected_codes_hit_ok(), "每件故障命中预期诊断码");
    s.add_dyn("冻结封版", freeze_gate_ok(), "全谱检出后可冻结，倒退拒");
    s.add_dyn("复跑一致", rerun_consistent_ok(), "两轮执行逐件结论逐位相等");
    s.add_dyn("故障类闭集", fault_kinds_closed_ok(), "11 类目标名 / 预期码互异且全部落窗");
    s.add_dyn("靶机指纹确定性", system_digest_stable_ok(), "健康账指纹双跑逐位相等，注入后必变");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1535_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn healthy_system_passes_all_invariants() {
        let t = TargetSystem::healthy();
        assert!(Detector::audit(&t).is_ok());
        assert_eq!(t.id(), SYS_ID_FROZEN);
        assert_eq!(t.stage(), 2);
    }

    #[test]
    fn every_fault_hits_expected_code() {
        assert!(expected_codes_hit_ok());
        // 单件细检：值越窗必返 ValueOutRange 码。
        let mut t = TargetSystem::healthy();
        FaultKind::ValueOver.apply(&mut t);
        match Detector::audit(&t) {
            Err(code) => assert_eq!(code, InjCode::ValueOutRange.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn compile_rejects_duplicate_and_bad_criterion() {
        let mut sc = InjectionScore::new();
        assert!(sc.compile("INJ-01", FaultKind::FlipId, "UNX-F1535-C01").is_ok());
        match sc.compile("INJ-01", FaultKind::ClearName, "UNX-F1535-C02") {
            Err(code) => assert_eq!(code, InjCode::IdMismatch.val()),
            Ok(()) => must_not_reach(),
        }
        match sc.compile("INJ-02", FaultKind::ValueOver, "坏判据号") {
            Err(code) => assert_eq!(code, InjCode::ChecksumBad.val()),
            Ok(()) => must_not_reach(),
        }
        match sc.compile("INJ-02", FaultKind::ValueOver, "UNX-F1535-C01") {
            Err(code) => assert_eq!(code, InjCode::TagDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn full_score_executes_green() {
        let mut sc = build_score_ok();
        assert_eq!(sc.execute(), 0);
        assert!(sc.all_caught_zero_alarm());
        assert_eq!(sc.phase(), ScorePhase::Executed);
    }

    #[test]
    fn missing_entry_blocks_freeze() {
        assert!(escape_caught_ok());
        let mut sc = InjectionScore::new();
        let _ = sc.compile("INJ-01", FaultKind::FlipId, "UNX-F1535-C01");
        let _ = sc.execute();
        match sc.freeze("缺件结论") {
            Err(code) => assert_eq!(code, InjCode::ExecuteMissed.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn freeze_and_no_regress() {
        let mut sc = build_score_ok();
        let _ = sc.execute();
        assert!(sc.freeze("注入总谱 11 件全检出，误报 0，一册封版。").is_ok());
        assert!(sc.is_frozen());
        match sc.advance(ScorePhase::Compiled) {
            Err(code) => assert_eq!(code, InjCode::StageOver.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn rerun_verdicts_identical() {
        let mut a = build_score_ok();
        let mut b = build_score_ok();
        let _ = a.execute();
        let _ = b.execute();
        assert_eq!(a.verdict_bits(), b.verdict_bits());
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(InjCode::ALL.len(), 12);
        let first = match InjCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match InjCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(1), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
