//! UNX-F50097 字体真机对照登记位（T1 档 ×2 台随闸门补测槽位）（0x43840 段）
//!
//! 锚点：UNX-F50097-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 数据段：`SEG_BASE = 0x43840`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 15`，
//! 逐项见 [`SlotError`]。
//!
//! 这张卡与 F50077（纸张）同属 T1 真机对照族，但对象换成了**字形**：T1 只上
//! 两台机器，每个字体面在每台机器上至多一个槽位；其余槽位挂在闸门之后，
//! 闸门不开就不许记证据。三态判定必须对「字形缺字」单独设卡——缺字的对照页
//! 拿到再低的 ΔE 也不能算通过，因为那一页根本没画全。
//!
//! 诚实三态：
//!
//! * `Untested`：没有证据、样本不足 `MIN_SAMPLES`、或对照页覆盖不足
//!   （`covered < required`）——三种情况都不判，既不放行也不倒扣；
//! * `Pass`：样本与覆盖都达标，平均 ΔE2000 ≤ `PASS_DELTA_E` 且字宽漂移
//!   ≤ `PASS_ADVANCE`（千分之一 em 单位）；
//! * `Fail`：样本与覆盖达标但任一度量越阈。
//!
//! 「登记不虚报」在这里是可机械复核的：`vet()` 用同一 [`judge`] 把每条证据
//! 重算一遍，任何 `Pass` 与重算不符即报 `Overclaim`；`gate_plan()` 只把
//! **未通过**的闸门位排进补测清单，已通过的不再重复占用真机时段。
//!
//! 结构清单：
//! * [`FaceCatalog`]：字体面册（upem 只认 1000/2048，重复 id 显式拒绝）；
//! * [`SlotRegistry`]：定容登记表（登记 / 开闸 / 记证据 / 复核 / 封存）；
//! * [`Slot`] / [`Verdict`] / [`Gate`] / [`Tally`]：槽位状态机与三态计数；
//! * [`EvidenceLedger`]：尝试账（attempted = accepted + rejected，恒等式可查）；
//! * [`injection_core`]：15 类坏样本注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43840 独占 0x20）。
pub const SEG_BASE: u32 = 0x43840;

const E_EMPTY: u16 = 1;
const E_DEVICE: u16 = 2;
const E_FACE: u16 = 3;
const E_DUP_SLOT: u16 = 4;
const E_CAPACITY: u16 = 5;
const E_UNKNOWN: u16 = 6;
const E_SAMPLES: u16 = 7;
const E_METRIC: u16 = 8;
const E_COVERAGE: u16 = 9;
const E_DEVICE_MISMATCH: u16 = 10;
const E_DUP_EVIDENCE: u16 = 11;
const E_NOT_OPEN: u16 = 12;
const E_SEALED: u16 = 13;
const E_GATE: u16 = 14;
const E_OVERCLAIM: u16 = 15;

/// 对照登记错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// 计划为空。
    Empty,
    /// 设备号不在 1..=2（T1 只有两台）。
    Device,
    /// 字体面 id 为 0、upem 非法或面册内重复。
    Face,
    /// 槽位号重复，或同一 (面, 设备) 已占位。
    DuplicateSlot,
    /// 登记表已满。
    Capacity,
    /// 槽位号未登记。
    Unknown,
    /// 样本数不在 1..=64。
    Samples,
    /// ΔE 或字宽漂移非有限或越界。
    Metric,
    /// 覆盖数非法（required=0、covered>required、或 required 超面字形数）。
    Coverage,
    /// 证据设备与槽位设备不一致。
    DeviceMismatch,
    /// 同一轮次证据重复登记。
    DuplicateEvidence,
    /// 闸门位尚未开闸。
    NotOpen,
    /// 登记表已封存，只读。
    Sealed,
    /// 闸门位缺少同面的常开基线位。
    Gate,
    /// 复核发现三态与证据不符（虚报）。
    Overclaim,
}

impl SlotError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SlotError::Empty => E_EMPTY,
            SlotError::Device => E_DEVICE,
            SlotError::Face => E_FACE,
            SlotError::DuplicateSlot => E_DUP_SLOT,
            SlotError::Capacity => E_CAPACITY,
            SlotError::Unknown => E_UNKNOWN,
            SlotError::Samples => E_SAMPLES,
            SlotError::Metric => E_METRIC,
            SlotError::Coverage => E_COVERAGE,
            SlotError::DeviceMismatch => E_DEVICE_MISMATCH,
            SlotError::DuplicateEvidence => E_DUP_EVIDENCE,
            SlotError::NotOpen => E_NOT_OPEN,
            SlotError::Sealed => E_SEALED,
            SlotError::Gate => E_GATE,
            SlotError::Overclaim => E_OVERCLAIM,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志与串口共用，不申请内存）。
    pub fn message(self) -> &'static str {
        match self {
            SlotError::Empty => "slot plan empty",
            SlotError::Device => "device index outside 1..=2",
            SlotError::Face => "font face id/upem invalid or duplicated",
            SlotError::DuplicateSlot => "slot id or (face,device) already registered",
            SlotError::Capacity => "slot registry full",
            SlotError::Unknown => "slot id not registered",
            SlotError::Samples => "evidence sample count outside 1..=64",
            SlotError::Metric => "metric not finite or outside legal range",
            SlotError::Coverage => "glyph coverage inconsistent with face",
            SlotError::DeviceMismatch => "evidence device differs from slot device",
            SlotError::DuplicateEvidence => "evidence round already recorded",
            SlotError::NotOpen => "gated slot is not open yet",
            SlotError::Sealed => "registry sealed (read only)",
            SlotError::Gate => "gated slot has no baseline slot on the same face",
            SlotError::Overclaim => "verdict does not match recorded evidence",
        }
    }
}

/// T1 档真机编号：只有两台进入对照，第三台及以后一律拒绝登记。
pub const T1_DEVICES: u8 = 2;
/// 本段可登记槽位上限（2 基线 + 2 闸门补测 + 余量）。
pub const MAX_SLOTS: usize = 8;
/// 面册容量（拉丁 / CJK / 符号 / 等宽 / 衬线余量）。
pub const MAX_FACES: usize = 5;
/// 判过所需最少对照页数。T1 只做小样本，低于 12 页的统计不足以判过。
pub const MIN_SAMPLES: u16 = 12;
/// 单槽最多接受的对照页数，防止「堆页」把均值压过阈值。
pub const MAX_SAMPLES: u16 = 64;
/// 判过的平均 ΔE2000 上限：字形小块在 2.5 以内目视基本无差。
pub const PASS_DELTA_E: f32 = 2.5;
/// 判过的字宽漂移上限，单位 1/1000 em（0.5% em 以内版式不会回行）。
pub const PASS_ADVANCE: f32 = 5.0;

/// 字体面类别。只用于报告分组与面册去重，不参与判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceKind {
    /// 拉丁正文面。
    Latin,
    /// CJK 面（行高与字宽差别大，单独成类方便分档看）。
    Cjk,
    /// 符号 / 图标面。
    Symbol,
    /// 等宽面（字宽漂移最敏感）。
    Mono,
    /// 衬线展示面。
    Serif,
}

impl FaceKind {
    /// 报告用短标签。
    pub fn label(self) -> &'static str {
        match self {
            FaceKind::Latin => "latin",
            FaceKind::Cjk => "cjk",
            FaceKind::Symbol => "symbol",
            FaceKind::Mono => "mono",
            FaceKind::Serif => "serif",
        }
    }
}

/// 一个参与对照的字体面。
///
/// upem 只认 1000（CFF 惯例）与 2048（TrueType 惯例）——其他值说明上游
/// 度量归一化没做，直接登记会让后续 ΔE/字宽比对口径混乱，故显式拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontFace {
    /// 面 id（段内唯一）。
    pub id: u16,
    /// 面类别。
    pub kind: FaceKind,
    /// 每 em 设计单位数：1000 或 2048。
    pub upem: u16,
    /// 面内字形总数（对照页的 required 不得超过它）。
    pub glyphs: u16,
}

/// 字体面册。插入即查重，查询线性扫描（容量 ≤5，二分无收益）。
#[derive(Clone, Debug)]
pub struct FaceCatalog {
    faces: Vec<FontFace>,
}

impl FaceCatalog {
    /// 空面册。
    pub fn new() -> FaceCatalog {
        FaceCatalog { faces: Vec::with_capacity(MAX_FACES) }
    }

    /// 登记一个字体面：id 非 0、upem 合法、容量未满、id 不重复。
    pub fn insert(&mut self, f: FontFace) -> Result<(), SlotError> {
        if f.id == 0 {
            return Err(SlotError::Face);
        }
        if f.upem != 1000 && f.upem != 2048 {
            return Err(SlotError::Face);
        }
        if f.glyphs == 0 {
            return Err(SlotError::Face);
        }
        if self.faces.len() >= MAX_FACES {
            return Err(SlotError::Capacity);
        }
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(e) = self.faces.get(i) {
                if e.id == f.id {
                    return Err(SlotError::Face);
                }
            }
            i += 1;
        }
        self.faces.push(f);
        Ok(())
    }

    /// 按 id 取面。
    pub fn get(&self, id: u16) -> Option<FontFace> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if let Some(f) = self.faces.get(i) {
                if f.id == id {
                    return Some(*f);
                }
            }
            i += 1;
        }
        None
    }

    /// 面册条目数。
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// 空册判定。
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

impl Default for FaceCatalog {
    fn default() -> FaceCatalog {
        FaceCatalog::new()
    }
}

/// 闸门属性：基线位常开，补测位必须等闸门开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 常开：T1 两台基线机，随时可记证据。
    Always,
    /// 随闸门：闸门打开后才允许补测（固件闸门 / 字体缓存闸门）。
    Gated,
}

/// 诚实三态。没有「跳过」「待定」这种模糊档，避免把未测伪装成结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 未测、样本不足或对照页缺字。
    Untested,
    /// 证据达标。
    Pass,
    /// 证据达标但不满足阈值。
    Fail,
}

/// 一轮字形对照证据。
///
/// `required` 是这一轮对照页要求的字形数，`covered` 是实际画出的字形数；
/// `drift` 是平均字宽漂移（1/1000 em）。四个量一起才能判——单看 ΔE 会把
/// 「漏字但每字都画得准」误判成通过。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evidence {
    /// 取样设备（必须与槽位设备一致）。
    pub device: u8,
    /// 有效对照页数。
    pub samples: u16,
    /// 对照页要求字形数。
    pub required: u16,
    /// 实际画出字形数。
    pub covered: u16,
    /// 平均 ΔE2000。
    pub mean_de: f32,
    /// 平均字宽漂移（1/1000 em）。
    pub drift: f32,
    /// 测量轮次（0..=255）。
    pub round: u8,
}

/// 一个对照槽位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// 槽位号（段内唯一）。
    pub id: u16,
    /// 绑定字体面 id。
    pub face: u16,
    /// 绑定设备 1..=2。
    pub device: u8,
    /// 闸门属性。
    pub gate: Gate,
    /// 是否已开闸（常开位登记即开）。
    pub open: bool,
    /// 最近一轮证据（没有就是没测）。
    pub evidence: Option<Evidence>,
    /// 判定（由证据按 [`judge`] 推出，复核会重算）。
    pub verdict: Verdict,
}

/// 三态计数。守恒：`total == pass + fail + untested`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    /// 通过槽位数。
    pub pass: usize,
    /// 失败槽位数。
    pub fail: usize,
    /// 未测 / 证据不足槽位数。
    pub untested: usize,
}

impl Tally {
    /// 三态之和。
    pub fn total(&self) -> usize {
        self.pass + self.fail + self.untested
    }
}

/// 尝试账：每次 `record` 恰好记一笔 accepted 或 rejected。
///
/// 单独记账的理由：判据要能证明「拒绝没有悄悄吞掉」——attempted 与
/// accepted + rejected 必须恒等，否则说明有路径既没成功也没计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceLedger {
    /// 进入去重/校验阶段的尝试数。
    pub attempted: u32,
    /// 成功入账数。
    pub accepted: u32,
    /// 显式拒绝数。
    pub rejected: u32,
}

impl EvidenceLedger {
    /// 空账。
    pub const fn new() -> EvidenceLedger {
        EvidenceLedger { attempted: 0, accepted: 0, rejected: 0 }
    }

    /// 成功一笔。
    pub fn note_accept(&mut self) {
        self.attempted += 1;
        self.accepted += 1;
    }

    /// 拒绝一笔。
    pub fn note_reject(&mut self) {
        self.attempted += 1;
        self.rejected += 1;
    }

    /// 恒等式：尝试 = 接受 + 拒绝。
    pub fn balanced(&self) -> bool {
        self.attempted == self.accepted + self.rejected
    }
}

fn finite(v: f32) -> bool {
    // NaN 不等于自身；±inf 在下面的区间比较里会被挡掉。
    v == v
}

/// 由证据推出三态。缺字与样本不足一律 `Untested`——这是 J1「不虚报」的核心。
pub fn judge(ev: Evidence) -> Verdict {
    if ev.required == 0 || ev.covered < ev.required {
        return Verdict::Untested;
    }
    if ev.samples < MIN_SAMPLES {
        return Verdict::Untested;
    }
    if ev.mean_de <= PASS_DELTA_E && ev.drift <= PASS_ADVANCE {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// T1 字形对照登记表。
#[derive(Clone, Debug)]
pub struct SlotRegistry {
    slots: Vec<Slot>,
    catalog: FaceCatalog,
    ledger: EvidenceLedger,
    sealed: bool,
    gates_opened: u32,
}

impl SlotRegistry {
    /// 空登记表（面册也为空，登记前必须先填面册）。
    pub fn new() -> SlotRegistry {
        SlotRegistry {
            slots: Vec::with_capacity(MAX_SLOTS),
            catalog: FaceCatalog::new(),
            ledger: EvidenceLedger::new(),
            sealed: false,
            gates_opened: 0,
        }
    }

    /// 从面册 + 槽位计划建表。空计划显式拒绝，绝不返回空表冒充成功。
    pub fn from_plan(faces: &[FontFace], plan: &[Slot]) -> Result<SlotRegistry, SlotError> {
        if plan.is_empty() {
            return Err(SlotError::Empty);
        }
        let mut r = SlotRegistry::new();
        let mut i = 0usize;
        while i < faces.len() {
            if let Some(f) = faces.get(i) {
                r.catalog.insert(*f)?;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < plan.len() {
            if let Some(s) = plan.get(j) {
                r.register(*s)?;
            }
            j += 1;
        }
        Ok(r)
    }

    /// 登记一个槽位。`open` 由闸门属性决定，调用方不能自称已开闸。
    pub fn register(&mut self, s: Slot) -> Result<(), SlotError> {
        if self.sealed {
            return Err(SlotError::Sealed);
        }
        if s.device == 0 || s.device > T1_DEVICES {
            return Err(SlotError::Device);
        }
        if self.catalog.get(s.face).is_none() {
            return Err(SlotError::Face);
        }
        if s.id == 0 {
            return Err(SlotError::DuplicateSlot);
        }
        if self.slots.len() >= MAX_SLOTS {
            return Err(SlotError::Capacity);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(e) = self.slots.get(i) {
                // 同一面在同一台机器上、同一种闸门属性只允许一个槽位：否则
                // 一台机器可以被拆成多份同质证据，凑数把均值压下来。基线与
                // 补测（Always/Gated）属于不同证据来源，允许并存。
                if e.id == s.id
                    || (e.face == s.face && e.device == s.device && e.gate == s.gate)
                {
                    return Err(SlotError::DuplicateSlot);
                }
            }
            i += 1;
        }
        let mut slot = s;
        slot.open = slot.gate == Gate::Always;
        slot.evidence = None;
        slot.verdict = Verdict::Untested;
        self.slots.push(slot);
        Ok(())
    }

    /// 打开全部闸门补测位。返回本次真正打开的个数（幂等：第二次返回 0）。
    pub fn open_gates(&mut self) -> Result<usize, SlotError> {
        if self.sealed {
            return Err(SlotError::Sealed);
        }
        let mut opened = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get_mut(i) {
                if s.gate == Gate::Gated && !s.open {
                    s.open = true;
                    opened += 1;
                }
            }
            i += 1;
        }
        if opened > 0 {
            self.gates_opened += 1;
        }
        Ok(opened)
    }

    /// 记录一轮证据。所有校验失败先记 rejected，再返回具体诊断码。
    pub fn record(&mut self, id: u16, ev: Evidence) -> Result<Verdict, SlotError> {
        match self.record_inner(id, ev) {
            Ok(v) => {
                self.ledger.note_accept();
                Ok(v)
            }
            Err(e) => {
                self.ledger.note_reject();
                Err(e)
            }
        }
    }

    fn record_inner(&mut self, id: u16, ev: Evidence) -> Result<Verdict, SlotError> {
        if self.sealed {
            return Err(SlotError::Sealed);
        }
        if !finite(ev.mean_de) || ev.mean_de < 0.0 || ev.mean_de > 100.0 {
            return Err(SlotError::Metric);
        }
        if !finite(ev.drift) || ev.drift < 0.0 || ev.drift > 100.0 {
            return Err(SlotError::Metric);
        }
        if ev.samples == 0 || ev.samples > MAX_SAMPLES {
            return Err(SlotError::Samples);
        }
        if ev.required == 0 || ev.covered > ev.required {
            return Err(SlotError::Coverage);
        }
        let mut idx = None;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let at = match idx {
            Some(v) => v,
            None => return Err(SlotError::Unknown),
        };
        // 先只读校验，再一次性写入：避免「校验失败但账已改」的中间态。
        let mut updated: Option<(Evidence, Verdict)> = None;
        if let Some(s) = self.slots.get(at) {
            let face = match self.catalog.get(s.face) {
                Some(f) => f,
                None => return Err(SlotError::Face),
            };
            if ev.required > face.glyphs {
                return Err(SlotError::Coverage);
            }
            if ev.device != s.device {
                return Err(SlotError::DeviceMismatch);
            }
            if !s.open {
                return Err(SlotError::NotOpen);
            }
            if let Some(prev) = s.evidence {
                if prev.round == ev.round {
                    return Err(SlotError::DuplicateEvidence);
                }
            }
            updated = Some((ev, judge(ev)));
        }
        match updated {
            Some((e, v)) => {
                if let Some(s) = self.slots.get_mut(at) {
                    s.evidence = Some(e);
                    s.verdict = v;
                }
                Ok(v)
            }
            None => Err(SlotError::Unknown),
        }
    }

    /// 读一个槽位的三态。
    pub fn verdict(&self, id: u16) -> Option<Verdict> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return Some(s.verdict);
                }
            }
            i += 1;
        }
        None
    }

    /// 读一个槽位的证据。
    pub fn evidence(&self, id: u16) -> Option<Evidence> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return s.evidence;
                }
            }
            i += 1;
        }
        None
    }

    /// 三态计数。
    pub fn tally(&self) -> Tally {
        let mut t = Tally { pass: 0, fail: 0, untested: 0 };
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                match s.verdict {
                    Verdict::Pass => t.pass += 1,
                    Verdict::Fail => t.fail += 1,
                    Verdict::Untested => t.untested += 1,
                }
            }
            i += 1;
        }
        t
    }

    /// 已开闸的补测清单：只列**未通过**的闸门位，按槽位号升序。
    pub fn gate_plan(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.open && s.gate == Gate::Gated && s.verdict != Verdict::Pass {
                    out.push(s.id);
                }
            }
            i += 1;
        }
        // 插入排序：定容小表，为 8 个元素引入通用排序不划算。
        let mut a = 1usize;
        while a < out.len() {
            let mut b = a;
            while b > 0 {
                let prev = match out.get(b - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let cur = match out.get(b) {
                    Some(v) => *v,
                    None => break,
                };
                if prev <= cur {
                    break;
                }
                if let Some(slot) = out.get_mut(b - 1) {
                    *slot = cur;
                }
                if let Some(slot) = out.get_mut(b) {
                    *slot = prev;
                }
                b -= 1;
            }
            a += 1;
        }
        out
    }

    /// 复核：配对完整性 + 三态与证据一致。返回通过槽位数。
    pub fn vet(&self) -> Result<usize, SlotError> {
        let mut pass = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.gate == Gate::Gated {
                    // 闸门位必须有同面的常开基线，否则补测数字没有对照物，
                    // 判过就是无源之水。
                    let mut has_base = false;
                    let mut j = 0usize;
                    while j < self.slots.len() {
                        if let Some(b) = self.slots.get(j) {
                            if b.gate == Gate::Always && b.face == s.face {
                                has_base = true;
                            }
                        }
                        j += 1;
                    }
                    if !has_base {
                        return Err(SlotError::Gate);
                    }
                }
                match s.evidence {
                    Some(ev) => {
                        let v = judge(ev);
                        if v != s.verdict {
                            return Err(SlotError::Overclaim);
                        }
                        if v == Verdict::Pass {
                            pass += 1;
                        }
                    }
                    None => {
                        if s.verdict != Verdict::Untested {
                            return Err(SlotError::Overclaim);
                        }
                    }
                }
            }
            i += 1;
        }
        Ok(pass)
    }

    /// 封存登记表：先复核，再置只读。封存后任何写入都报 `Sealed`。
    pub fn seal(&mut self) -> Result<usize, SlotError> {
        if self.sealed {
            return Err(SlotError::Sealed);
        }
        let pass = self.vet()?;
        self.sealed = true;
        Ok(pass)
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 尝试账快照。
    pub fn ledger(&self) -> EvidenceLedger {
        self.ledger
    }

    /// 闸门打开次数（用于幂等判据）。
    pub fn gates_opened(&self) -> u32 {
        self.gates_opened
    }

    /// 槽位数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 一行报告（UI / 软件写回共用，结果摘要非空）。
    pub fn report(&self) -> String {
        let t = self.tally();
        format!(
            "T1 font slots={} pass={} fail={} untested={} sealed={} gates={}",
            t.total(),
            t.pass,
            t.fail,
            t.untested,
            self.sealed as u8,
            self.gates_opened
        )
    }
}

impl Default for SlotRegistry {
    fn default() -> SlotRegistry {
        SlotRegistry::new()
    }
}

// ---------------------------------------------------------------------------
// 基线面册、基线计划与注入核
// ---------------------------------------------------------------------------

/// T1 基线面册：拉丁 / CJK / 符号三面。
pub fn baseline_faces() -> Vec<FontFace> {
    let mut v = Vec::with_capacity(3);
    v.push(FontFace { id: 1, kind: FaceKind::Latin, upem: 1000, glyphs: 230 });
    v.push(FontFace { id: 2, kind: FaceKind::Cjk, upem: 1000, glyphs: 512 });
    v.push(FontFace { id: 3, kind: FaceKind::Symbol, upem: 2048, glyphs: 64 });
    v
}

/// 面册填充（注入核专用）：五面 × 两台机器最多十个占位组合。
fn demo_faces() -> Vec<FontFace> {
    let kinds = [
        FaceKind::Latin,
        FaceKind::Cjk,
        FaceKind::Symbol,
        FaceKind::Mono,
        FaceKind::Serif,
    ];
    let mut v = Vec::with_capacity(MAX_FACES);
    let mut i = 0usize;
    while i < kinds.len() {
        if let Some(k) = kinds.get(i) {
            v.push(FontFace { id: (i + 1) as u16, kind: *k, upem: 1000, glyphs: 256 });
        }
        i += 1;
    }
    v
}

fn slot_of(id: u16, face: u16, device: u8, gate: Gate) -> Slot {
    Slot { id, face, device, gate, open: false, evidence: None, verdict: Verdict::Untested }
}

/// T1 基线计划：拉丁面两台各一张常开位，再挂两个闸门补测位（都在拉丁面上，
/// 保证闸门位有同面基线可对照）。
pub fn baseline_plan() -> Vec<Slot> {
    let mut v = Vec::with_capacity(4);
    v.push(slot_of(11, 1, 1, Gate::Always));
    v.push(slot_of(12, 1, 2, Gate::Always));
    v.push(slot_of(21, 1, 2, Gate::Gated));
    v.push(slot_of(22, 1, 1, Gate::Gated));
    v
}

fn demo_registry() -> SlotRegistry {
    match SlotRegistry::from_plan(&demo_faces(), &baseline_plan()) {
        Ok(v) => v,
        Err(_) => SlotRegistry::new(),
    }
}

/// 坏样本注入核：15 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 15usize;

    // 1 空计划
    if SlotRegistry::from_plan(&[], &[]).err() == Some(SlotError::Empty) {
        caught += 1;
    }
    // 2 设备越界（T1 只有两台）
    let mut r = SlotRegistry::new();
    let _ = r.catalog.insert(FontFace { id: 1, kind: FaceKind::Latin, upem: 1000, glyphs: 64 });
    if r.register(slot_of(1, 1, 3, Gate::Always)).err() == Some(SlotError::Device) {
        caught += 1;
    }
    // 3 字体面非法（upem 900 不在 {1000, 2048}）
    let mut bad_face = SlotRegistry::new();
    if bad_face
        .catalog
        .insert(FontFace { id: 1, kind: FaceKind::Latin, upem: 900, glyphs: 64 })
        .err()
        == Some(SlotError::Face)
    {
        caught += 1;
    }
    // 4 槽位重复（同面同机二次占位）
    let mut dup = demo_registry();
    if dup.register(slot_of(13, 1, 1, Gate::Always)).err() == Some(SlotError::DuplicateSlot) {
        caught += 1;
    }
    // 5 容量满（8 个槽位后第 9 个）
    let mut full = SlotRegistry::new();
    let faces = demo_faces();
    let mut i = 0usize;
    while i < faces.len() {
        if let Some(f) = faces.get(i).copied() {
            let _ = full.catalog.insert(f);
        }
        i += 1;
    }
    let mut dev = 1u8;
    let mut face = 1u16;
    let mut id = 1u16;
    let mut placed = 0usize;
    while placed < MAX_SLOTS {
        let _ = full.register(slot_of(id, face, dev, Gate::Always));
        id += 1;
        if dev == 1 {
            dev = 2;
        } else {
            dev = 1;
            face += 1;
        }
        placed += 1;
    }
    if full.register(slot_of(99, 1, 1, Gate::Always)).err() == Some(SlotError::Capacity) {
        caught += 1;
    }
    // 6 未知槽位
    let mut r = demo_registry();
    let ev = Evidence { device: 1, samples: 16, required: 100, covered: 100, mean_de: 1.0, drift: 1.0, round: 1 };
    if r.record(999, ev).err() == Some(SlotError::Unknown) {
        caught += 1;
    }
    // 7 样本数为零
    let mut r = demo_registry();
    let mut e0 = ev;
    e0.samples = 0;
    if r.record(11, e0).err() == Some(SlotError::Samples) {
        caught += 1;
    }
    // 8 ΔE 非有限（NaN）
    let mut r = demo_registry();
    let mut e1 = ev;
    e1.mean_de = 0.0 / 0.0;
    if r.record(11, e1).err() == Some(SlotError::Metric) {
        caught += 1;
    }
    // 9 覆盖自相矛盾（covered > required）
    let mut r = demo_registry();
    let mut e2 = ev;
    e2.covered = 101;
    if r.record(11, e2).err() == Some(SlotError::Coverage) {
        caught += 1;
    }
    // 10 证据设备与槽位不一致
    let mut r = demo_registry();
    let mut e3 = ev;
    e3.device = 2;
    if r.record(11, e3).err() == Some(SlotError::DeviceMismatch) {
        caught += 1;
    }
    // 11 同轮证据重复
    let mut r = demo_registry();
    let _ = r.record(11, ev);
    if r.record(11, ev).err() == Some(SlotError::DuplicateEvidence) {
        caught += 1;
    }
    // 12 闸门未开就补测
    let mut r = demo_registry();
    let mut e4 = ev;
    e4.device = 2;
    if r.record(21, e4).err() == Some(SlotError::NotOpen) {
        caught += 1;
    }
    // 13 封存后仍尝试登记
    let mut r = demo_registry();
    let _ = r.seal();
    if r.register(slot_of(31, 2, 1, Gate::Always)).err() == Some(SlotError::Sealed) {
        caught += 1;
    }
    // 14 闸门位缺同面基线（孤儿补测位）
    let mut orphan = SlotRegistry::new();
    let _ = orphan.catalog.insert(FontFace { id: 3, kind: FaceKind::Symbol, upem: 2048, glyphs: 32 });
    let _ = orphan.register(slot_of(41, 3, 2, Gate::Gated));
    if orphan.vet().err() == Some(SlotError::Gate) {
        caught += 1;
    }
    // 15 虚报（未测被标成 Pass）
    let mut forged = demo_registry();
    if let Some(s) = forged.slots.get_mut(0) {
        s.verdict = Verdict::Pass;
    }
    if forged.vet().err() == Some(SlotError::Overclaim) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50097 的 15 条判据（含 J1 诚实三态与注入核）。
pub fn run_unx50097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50097");

    let faces = baseline_faces();
    let plan = baseline_plan();

    // J1-1 初始三态：四个槽位全部未测，绝不能出现「默认通过」。
    let built = SlotRegistry::from_plan(&faces, &plan);
    let ok = match &built {
        Ok(r) => {
            let t = r.tally();
            t.pass == 0 && t.fail == 0 && t.untested == 4 && t.total() == 4
        }
        Err(_) => false,
    };
    s.add("50097-J1-初始未测", ok, if ok { "" } else { "默认态不是未测" });

    // J1-2 面册口径：upem 只认 1000/2048，其它值必须在登记入口就被挡。
    let mut cat = FaceCatalog::new();
    let bad_upem = cat.insert(FontFace { id: 9, kind: FaceKind::Serif, upem: 1024, glyphs: 32 });
    let good_a = cat.insert(FontFace { id: 1, kind: FaceKind::Latin, upem: 1000, glyphs: 200 });
    let good_b = cat.insert(FontFace { id: 2, kind: FaceKind::Cjk, upem: 2048, glyphs: 400 });
    let ok = bad_upem.err() == Some(SlotError::Face) && good_a.is_ok() && good_b.is_ok() && cat.len() == 2;
    let det = format!("bad={:?} len={}", bad_upem.err().map(|e| e.code()), cat.len());
    s.add_dyn("50097-J1-面册口径", ok, &det);

    let mut r = match SlotRegistry::from_plan(&faces, &plan) {
        Ok(v) => v,
        Err(_) => SlotRegistry::new(),
    };

    // J1-3 证据不足不判过：6 页 < 12 页，仍是 Untested（记了但不判）。
    let low = Evidence { device: 1, samples: 6, required: 120, covered: 120, mean_de: 0.5, drift: 0.5, round: 1 };
    let v_low = r.record(11, low);
    let ok = v_low == Ok(Verdict::Untested) && r.verdict(11) == Some(Verdict::Untested);
    let det = format!("v={:?}", v_low);
    s.add_dyn("50097-J1-样本不足不判过", ok, &det);

    // J1-4 缺字不判过：覆盖 100/120，ΔE 再低也不能算过。
    let miss = Evidence { device: 1, samples: 16, required: 120, covered: 100, mean_de: 0.2, drift: 0.2, round: 2 };
    let v_miss = r.record(11, miss);
    let ok = v_miss == Ok(Verdict::Untested) && r.verdict(11) == Some(Verdict::Untested);
    s.add("50097-J1-缺字不判过", ok, if ok { "" } else { "缺字被当成通过" });

    // J1-5 达标判过：16 页、覆盖 120/120、ΔE 1.2、漂移 1.0。
    let good = Evidence { device: 1, samples: 16, required: 120, covered: 120, mean_de: 1.2, drift: 1.0, round: 3 };
    let v_good = r.record(11, good);
    let ok = v_good == Ok(Verdict::Pass) && r.verdict(11) == Some(Verdict::Pass);
    s.add("50097-J1-达标判过", ok, if ok { "" } else { "达标未判过" });

    // J1-6 字宽漂移越阈判败：ΔE 0.9 但漂移 12‰ em > 5‰。
    let drift = Evidence { device: 2, samples: 16, required: 120, covered: 120, mean_de: 0.9, drift: 12.0, round: 1 };
    let v_bad = r.record(12, drift);
    let ok = v_bad == Ok(Verdict::Fail) && r.verdict(12) == Some(Verdict::Fail);
    let det = format!("v={:?}", v_bad);
    s.add_dyn("50097-J1-字宽越阈判败", ok, &det);

    // J1-7 不虚报：复核通过数 == Pass 槽位数，且不报 Overclaim。
    let tally = r.tally();
    let ok = match r.vet() {
        Ok(pass) => pass == tally.pass && tally.pass == 1 && tally.fail == 1 && tally.untested == 2,
        Err(_) => false,
    };
    let det = format!("pass={} fail={} untested={}", tally.pass, tally.fail, tally.untested);
    s.add_dyn("50097-J1-登记不虚报", ok, &det);

    // J1-8 闸门补测清单：开闸后只列未通过的闸门位，升序且不含已通过项。
    let before = r.gate_plan();
    let opened = r.open_gates();
    let plan_now = r.gate_plan();
    let asc = plan_now.len() == 2
        && plan_now.first() == Some(&21)
        && plan_now.get(1) == Some(&22);
    let ok = before.is_empty() && opened == Ok(2) && asc;
    let det = format!("before={} after={} opened={:?}", before.len(), plan_now.len(), opened);
    s.add_dyn("50097-J1-闸门补测清单", ok, &det);

    // 补测达标后该位出列，清单只剩 22（已通过不重复打扰真机）。
    let fix = Evidence { device: 2, samples: 16, required: 120, covered: 120, mean_de: 2.0, drift: 2.0, round: 1 };
    let _ = r.record(21, fix);
    let plan2 = r.gate_plan();
    let ok = plan2.len() == 1 && plan2.first() == Some(&22);
    s.add("50097-J1-通过即出列", ok, if ok { "" } else { "已通过仍在补测清单" });

    // J1-9 开闸幂等：第二次开闸打开 0 个，打开计数只增一次。
    let again = r.open_gates();
    let ok = again == Ok(0) && r.gates_opened() == 1;
    let det = format!("again={:?} gates={}", again, r.gates_opened());
    s.add_dyn("50097-J1-开闸幂等", ok, &det);

    // J1-10 证据账守恒：attempted == accepted + rejected。
    let led = r.ledger();
    let ok = led.balanced() && led.attempted == led.accepted + led.rejected && led.accepted == 5;
    let det = format!("a={} ok={} rej={}", led.attempted, led.accepted, led.rejected);
    s.add_dyn("50097-J1-证据账守恒", ok, &det);

    // J1-11 同轮重复登记被拒，且三态不变（防同一批数据报两次）。
    let dup = Evidence { device: 2, samples: 16, required: 120, covered: 120, mean_de: 2.0, drift: 2.0, round: 1 };
    let before_v = r.verdict(21);
    let got = r.record(21, dup);
    let ok = got.err() == Some(SlotError::DuplicateEvidence) && r.verdict(21) == before_v;
    s.add("50097-J1-同轮去重", ok, if ok { "" } else { "同轮证据被重复接受" });

    // J1-12 配对完整性：闸门位缺同面基线 → Gate。
    let mut orphan = SlotRegistry::new();
    let _ = orphan.catalog.insert(FontFace { id: 2, kind: FaceKind::Cjk, upem: 1000, glyphs: 64 });
    let _ = orphan.register(slot_of(31, 2, 2, Gate::Gated));
    let ok = orphan.vet().err() == Some(SlotError::Gate);
    s.add("50097-J1-配对校验", ok, if ok { "" } else { "孤儿闸门位通过复核" });

    // J1-13 虚报必抓：内部把未测槽位标成 Pass，vet 必须报 Overclaim。
    let mut forged = match SlotRegistry::from_plan(&faces, &plan) {
        Ok(v) => v,
        Err(_) => SlotRegistry::new(),
    };
    if let Some(x) = forged.slots.get_mut(0) {
        x.verdict = Verdict::Pass;
    }
    let ok = forged.vet().err() == Some(SlotError::Overclaim);
    s.add("50097-J1-虚报必抓", ok, if ok { "" } else { "伪造 Pass 未被抓" });

    // J1-14 封存只读：封存成功、再次封存与写入都报 Sealed，已记证据仍可读。
    let sealed_pass = r.seal();
    let again = r.seal();
    let write = r.record(22, fix);
    let reg = r.register(slot_of(51, 3, 1, Gate::Always));
    let ok = sealed_pass == Ok(2)
        && again.err() == Some(SlotError::Sealed)
        && write.err() == Some(SlotError::Sealed)
        && reg.err() == Some(SlotError::Sealed)
        && r.verdict(11) == Some(Verdict::Pass)
        && r.report().contains("T1 font");
    let det = format!("seal={:?} again={:?}", sealed_pass, again.err().map(|e| e.code()));
    s.add_dyn("50097-J1-封存只读", ok, &det);

    // 注入核：15 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50097-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50097_checks_green() {
        let set = run_unx50097_checks();
        assert!(set.all_passed(), "UNX-F50097 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50097_missing_glyphs_never_pass() {
        let ev = Evidence { device: 1, samples: 32, required: 200, covered: 199, mean_de: 0.0, drift: 0.0, round: 1 };
        assert_eq!(judge(ev), Verdict::Untested);
    }
}
