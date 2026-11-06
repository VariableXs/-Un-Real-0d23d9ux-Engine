//! UNX-F50077 纸张真机对照登记位（T1 档 ×2 台随闸门补测槽位）（0x435c0 段）
//!
//! 锚点：UNX-F50077-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 数据段：`SEG_BASE = 0x435c0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 14`，
//! 逐项见 [`SlotError`]。
//!
//! 这张卡不产出「对照通过」的漂亮数字，而是产出一张**不许虚报**的登记表：
//! T1 档只在两台真机上取样，其余对照位挂在闸门之后，闸门不开就不许记证据。
//! 三态纪律（诚实三态）：
//!
//! * `Untested`：没有证据，**或**证据样本数不足 `MIN_SAMPLES`——不足即不判，
//!   既不冒充通过也不倒扣成失败；
//! * `Pass`：样本数达标且平均 ΔE ≤ `PASS_DELTA_E`；
//! * `Fail`：样本数达标但平均 ΔE 超阈值。
//!
//! 因此「登记不虚报」可以机械复核：`vet()` 用同一判据把每条证据重算一遍，
//! 任何 `Pass` 若与重算结果不符就报 `Overclaim`；`gate_plan()` 只把**未通过**
//! 的闸门位排进补测清单，已通过的不重复打扰真机。
//!
//! 结构清单：
//! * [`SlotRegistry`]：定容登记表（注册 / 开闸 / 记证据 / 复核 / 封存）；
//! * [`EvidenceLedger`]：尝试账（attempted = accepted + rejected，恒等式可查）；
//! * [`Slot`] / [`Verdict`] / [`Gate`]：槽位状态机与三态判定；
//! * [`injection_core`]：8 类坏样本注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x435c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x435c0;

const E_EMPTY: u16 = 1;
const E_DEVICE: u16 = 2;
const E_PAPER: u16 = 3;
const E_DUP_SLOT: u16 = 4;
const E_CAPACITY: u16 = 5;
const E_UNKNOWN: u16 = 6;
const E_SAMPLES: u16 = 7;
const E_METRIC: u16 = 8;
const E_DEVICE_MISMATCH: u16 = 9;
const E_DUP_EVIDENCE: u16 = 10;
const E_NOT_OPEN: u16 = 11;
const E_SEALED: u16 = 12;
const E_GATE: u16 = 13;
const E_OVERCLAIM: u16 = 14;

/// 对照登记错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// 计划为空。
    Empty,
    /// 设备号不在 1..=2。
    Device,
    /// 纸张号不在 1..=3。
    Paper,
    /// 槽位号重复。
    DuplicateSlot,
    /// 登记表已满。
    Capacity,
    /// 槽位号未登记。
    Unknown,
    /// 样本数不在 1..=64。
    Samples,
    /// 平均 ΔE 非有限或不在 0..=100。
    Metric,
    /// 证据设备与槽位设备不一致。
    DeviceMismatch,
    /// 同一轮次证据重复登记。
    DuplicateEvidence,
    /// 闸门位尚未开闸。
    NotOpen,
    /// 登记表已封存，只读。
    Sealed,
    /// 闸门位缺少同纸基线位。
    Gate,
    /// 复核发现三态与实际证据不符（虚报）。
    Overclaim,
}

impl SlotError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SlotError::Empty => E_EMPTY,
            SlotError::Device => E_DEVICE,
            SlotError::Paper => E_PAPER,
            SlotError::DuplicateSlot => E_DUP_SLOT,
            SlotError::Capacity => E_CAPACITY,
            SlotError::Unknown => E_UNKNOWN,
            SlotError::Samples => E_SAMPLES,
            SlotError::Metric => E_METRIC,
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
            SlotError::Paper => "paper id outside 1..=3",
            SlotError::DuplicateSlot => "slot id already registered",
            SlotError::Capacity => "slot registry full",
            SlotError::Unknown => "slot id not registered",
            SlotError::Samples => "evidence sample count outside 1..=64",
            SlotError::Metric => "mean delta-e not finite or outside 0..=100",
            SlotError::DeviceMismatch => "evidence device differs from slot device",
            SlotError::DuplicateEvidence => "evidence round already recorded",
            SlotError::NotOpen => "gated slot is not open yet",
            SlotError::Sealed => "registry sealed (read only)",
            SlotError::Gate => "gated slot has no baseline slot on the same paper",
            SlotError::Overclaim => "verdict does not match recorded evidence",
        }
    }
}

/// T1 档真机编号：只有两台进入对照，第三台及以后一律拒绝登记。
pub const T1_DEVICES: u8 = 2;
/// 本段可登记槽位上限（2 基线 + 2 闸门补测 + 余量）。
pub const MAX_SLOTS: usize = 8;
/// 判过所需最少样本数。T1 只做小样本，低于 8 张的统计不足以判过。
pub const MIN_SAMPLES: u16 = 8;
/// 单槽最多接受的样本数，防止「堆样本」把均值压过阈值。
pub const MAX_SAMPLES: u16 = 64;
/// 判过的平均 ΔE 上限（纸张真机对照用 ΔE2000，3.0 为目视可辨边界）。
pub const PASS_DELTA_E: f32 = 3.0;

/// 闸门属性：基线位常开，补测位必须等闸门开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 常开：T1 两台基线机，随时可记证据。
    Always,
    /// 随闸门：闸门打开后才允许补测（例如固件闸门 / 纸路闸门）。
    Gated,
}

/// 诚实三态。没有「跳过」「待定」这种模糊档，避免把未测伪装成结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 未测或证据不足。
    Untested,
    /// 证据达标。
    Pass,
    /// 证据达标但不满足阈值。
    Fail,
}

/// 一轮真机证据。`round` 是测量轮次，同轮重复登记视为虚报。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evidence {
    /// 取样设备（必须与槽位设备一致）。
    pub device: u8,
    /// 有效样本数。
    pub samples: u16,
    /// 平均 ΔE2000。
    pub mean_de: f32,
    /// 测量轮次（0..=255）。
    pub round: u8,
}

/// 一个对照槽位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// 槽位号（段内唯一）。
    pub id: u16,
    /// 纸张号 1..=3。
    pub paper: u8,
    /// 绑定设备 1..=2。
    pub device: u8,
    /// 闸门属性。
    pub gate: Gate,
    /// 是否已开闸（常开位登记即开）。
    pub open: bool,
    /// 最近一轮证据（没有就是没测）。
    pub evidence: Option<Evidence>,
    /// 判定（由证据按 `judge` 推出，复核会重算）。
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
/// 为什么单独记账：判据要能证明「拒绝没有悄悄吞掉」——attempted 与
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

/// 由证据推出三态。样本不足一律 `Untested`——这是 J1「不虚报」的核心。
pub fn judge(ev: Evidence) -> Verdict {
    if ev.samples < MIN_SAMPLES {
        return Verdict::Untested;
    }
    if ev.mean_de <= PASS_DELTA_E {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

fn finite(v: f32) -> bool {
    // NaN 不等于自身；无穷会在下面的区间比较里被挡掉（inf > 100）。
    v == v
}

/// T1 对照登记表（定容，零堆增长之外的 Vec 预留）。
#[derive(Clone, Debug)]
pub struct SlotRegistry {
    slots: Vec<Slot>,
    ledger: EvidenceLedger,
    sealed: bool,
    gates_opened: u32,
}

impl SlotRegistry {
    /// 空登记表。
    pub fn new() -> SlotRegistry {
        SlotRegistry {
            slots: Vec::with_capacity(MAX_SLOTS),
            ledger: EvidenceLedger::new(),
            sealed: false,
            gates_opened: 0,
        }
    }

    /// 从计划位列表建表。空计划显式拒绝，绝不返回空表冒充成功。
    pub fn from_plan(plan: &[Slot]) -> Result<SlotRegistry, SlotError> {
        if plan.is_empty() {
            return Err(SlotError::Empty);
        }
        let mut r = SlotRegistry::new();
        let mut i = 0usize;
        while i < plan.len() {
            if let Some(s) = plan.get(i) {
                r.register(*s)?;
            }
            i += 1;
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
        if s.paper == 0 || s.paper > 3 {
            return Err(SlotError::Paper);
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
                if e.id == s.id {
                    return Err(SlotError::DuplicateSlot);
                }
            }
            i += 1;
        }
        let mut slot = s;
        // 常开位登记即开；闸门位强制关闭，只能由 open_gates 打开。
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

    /// 记录一轮证据。所有校验失败都先记 rejected，再返回具体诊断码。
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
        if ev.samples == 0 || ev.samples > MAX_SAMPLES {
            return Err(SlotError::Samples);
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
        let mut updated: Option<(Evidence, Verdict)> = None;
        if let Some(s) = self.slots.get(at) {
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
        // 插入排序：定容小表，避免为 8 个元素引入额外依赖。
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
                    let mut has_base = false;
                    let mut j = 0usize;
                    while j < self.slots.len() {
                        if let Some(b) = self.slots.get(j) {
                            if b.gate == Gate::Always && b.paper == s.paper {
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

    /// 一行报告（UI / 软件写回共用，结果摘要非空）。
    pub fn report(&self) -> String {
        let t = self.tally();
        format!(
            "T1 slots={} pass={} fail={} untested={} sealed={} gates={}",
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
// 基线计划与注入核
// ---------------------------------------------------------------------------

/// T1 基线计划：两台真机各登记一张 A4 常开位，另挂两个闸门补测位。
pub fn baseline_plan() -> Vec<Slot> {
    let mut v = Vec::with_capacity(4);
    v.push(Slot { id: 11, paper: 1, device: 1, gate: Gate::Always, open: false, evidence: None, verdict: Verdict::Untested });
    v.push(Slot { id: 12, paper: 1, device: 2, gate: Gate::Always, open: false, evidence: None, verdict: Verdict::Untested });
    v.push(Slot { id: 21, paper: 1, device: 2, gate: Gate::Gated, open: false, evidence: None, verdict: Verdict::Untested });
    v.push(Slot { id: 22, paper: 1, device: 1, gate: Gate::Gated, open: false, evidence: None, verdict: Verdict::Untested });
    v
}

fn slot(id: u16, paper: u8, device: u8, gate: Gate) -> Slot {
    Slot { id, paper, device, gate, open: false, evidence: None, verdict: Verdict::Untested }
}

/// 坏样本注入核：8 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1 空计划
    if SlotRegistry::from_plan(&[]).err() == Some(SlotError::Empty) {
        caught += 1;
    }
    // 2 设备越界（T1 只有两台）
    let mut r = SlotRegistry::new();
    if r.register(slot(1, 1, 3, Gate::Always)).err() == Some(SlotError::Device) {
        caught += 1;
    }
    // 3 槽位重复
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(1, 1, 1, Gate::Always));
    if r.register(slot(1, 1, 2, Gate::Always)).err() == Some(SlotError::DuplicateSlot) {
        caught += 1;
    }
    // 4 证据设备与槽位不一致
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(1, 1, 1, Gate::Always));
    let ev = Evidence { device: 2, samples: 12, mean_de: 1.0, round: 1 };
    if r.record(1, ev).err() == Some(SlotError::DeviceMismatch) {
        caught += 1;
    }
    // 5 样本数为零
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(1, 1, 1, Gate::Always));
    let ev = Evidence { device: 1, samples: 0, mean_de: 1.0, round: 1 };
    if r.record(1, ev).err() == Some(SlotError::Samples) {
        caught += 1;
    }
    // 6 平均 ΔE 非有限（NaN）
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(1, 1, 1, Gate::Always));
    let ev = Evidence { device: 1, samples: 12, mean_de: 0.0 / 0.0, round: 1 };
    if r.record(1, ev).err() == Some(SlotError::Metric) {
        caught += 1;
    }
    // 7 闸门未开就补测
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(21, 1, 2, Gate::Gated));
    let ev = Evidence { device: 2, samples: 12, mean_de: 1.0, round: 1 };
    if r.record(21, ev).err() == Some(SlotError::NotOpen) {
        caught += 1;
    }
    // 8 封存后仍尝试登记
    let mut r = SlotRegistry::new();
    let _ = r.register(slot(1, 1, 1, Gate::Always));
    let _ = r.seal();
    if r.register(slot(2, 1, 2, Gate::Always)).err() == Some(SlotError::Sealed) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50077 的 14 条判据（含 J1 诚实三态与注入核）。
pub fn run_unx50077_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50077");

    // J1-1 初始三态：四个槽位全部未测，绝不能出现「默认通过」。
    let built = SlotRegistry::from_plan(&baseline_plan());
    let ok = match &built {
        Ok(r) => {
            let t = r.tally();
            t.pass == 0 && t.fail == 0 && t.untested == 4 && t.total() == 4
        }
        Err(_) => false,
    };
    s.add("50077-J1-初始未测", ok, if ok { "" } else { "默认态不是未测" });

    let mut r = match SlotRegistry::from_plan(&baseline_plan()) {
        Ok(v) => v,
        Err(_) => SlotRegistry::new(),
    };

    // J1-2 证据不足不判过（三态诚实：4 张 < 8 张，仍是 Untested）。
    let low = Evidence { device: 1, samples: 4, mean_de: 0.8, round: 1 };
    let v_low = r.record(11, low);
    let ok = v_low == Ok(Verdict::Untested) && r.verdict(11) == Some(Verdict::Untested);
    s.add("50077-J1-样本不足不判过", ok, if ok { "" } else { "小样本被判过" });

    // J1-3 达标判过：12 张、均值 1.4 ≤ 3.0。
    let good = Evidence { device: 1, samples: 12, mean_de: 1.4, round: 2 };
    let v_good = r.record(11, good);
    let ok = v_good == Ok(Verdict::Pass) && r.verdict(11) == Some(Verdict::Pass);
    s.add("50077-J1-达标判过", ok, if ok { "" } else { "达标未判过" });

    // J1-4 越阈判败：12 张、均值 7.5 > 3.0。
    let bad = Evidence { device: 2, samples: 12, mean_de: 7.5, round: 1 };
    let v_bad = r.record(12, bad);
    let ok = v_bad == Ok(Verdict::Fail) && r.verdict(12) == Some(Verdict::Fail);
    s.add("50077-J1-越阈判败", ok, if ok { "" } else { "越阈未判败" });

    // J1-5 不虚报：复核通过数必须等于 Pass 槽位数，且 vet 不报 Overclaim。
    let tally = r.tally();
    let ok = match r.vet() {
        Ok(pass) => pass == tally.pass && tally.pass == 1,
        Err(_) => false,
    };
    let det = format!("pass={} fail={} untested={}", tally.pass, tally.fail, tally.untested);
    s.add_dyn("50077-J1-登记不虚报", ok, &det);

    // J1-6 闸门补测清单：开闸后只列未通过的闸门位，升序且不含已通过项。
    let before = r.gate_plan();
    let opened = r.open_gates();
    let plan = r.gate_plan();
    let asc = plan.len() == 2 && plan.first() == Some(&21) && plan.get(1) == Some(&22);
    let ok = before.is_empty() && opened == Ok(2) && asc;
    let det = format!("before={} after={} opened={:?}", before.len(), plan.len(), opened);
    s.add_dyn("50077-J1-闸门补测清单", ok, &det);

    // 闸门位 21 补测达标后，清单只剩 22（已通过不重复打扰真机）。
    let fix = Evidence { device: 2, samples: 16, mean_de: 2.0, round: 1 };
    let _ = r.record(21, fix);
    let plan2 = r.gate_plan();
    let ok = plan2.len() == 1 && plan2.first() == Some(&22);
    s.add("50077-J1-通过即出列", ok, if ok { "" } else { "已通过仍在补测清单" });

    // J1-7 开闸幂等：第二次开闸打开 0 个，打开计数只增一次。
    let again = r.open_gates();
    let ok = again == Ok(0) && r.gates_opened() == 1;
    let det = format!("again={:?} gates={}", again, r.gates_opened());
    s.add_dyn("50077-J1-开闸幂等", ok, &det);

    // J1-8 证据账守恒：attempted == accepted + rejected。
    let led = r.ledger();
    let ok = led.balanced() && led.attempted == led.accepted + led.rejected && led.attempted >= 4;
    let det = format!("a={} ok={} rej={}", led.attempted, led.accepted, led.rejected);
    s.add_dyn("50077-J1-证据账守恒", ok, &det);

    // J1-9 同轮重复登记被拒，且三态不变（防同一批数据报两次）。
    let dup = Evidence { device: 2, samples: 16, mean_de: 2.0, round: 1 };
    let before = r.verdict(21);
    let got = r.record(21, dup);
    let ok = got.err() == Some(SlotError::DuplicateEvidence) && r.verdict(21) == before;
    s.add("50077-J1-同轮去重", ok, if ok { "" } else { "同轮证据被重复接受" });

    // J1-10 配对完整性：闸门位缺同纸基线 → Gate。
    let mut orphan = SlotRegistry::new();
    let _ = orphan.register(slot(31, 3, 2, Gate::Gated));
    let ok = orphan.vet().err() == Some(SlotError::Gate);
    s.add("50077-J1-配对校验", ok, if ok { "" } else { "孤儿闸门位通过复核" });

    // J1-11 虚报必抓：内部把未测槽位标成 Pass，vet 必须报 Overclaim。
    let mut forged = SlotRegistry::new();
    let _ = forged.register(slot(41, 1, 1, Gate::Always));
    if let Some(x) = forged.slots.get_mut(0) {
        x.verdict = Verdict::Pass;
    }
    let ok = forged.vet().err() == Some(SlotError::Overclaim);
    s.add("50077-J1-虚报必抓", ok, if ok { "" } else { "伪造 Pass 未被抓" });

    // J1-12 封存只读：封存后写入一律 Sealed，已记证据仍可读。
    let sealed_pass = r.seal();
    let write = r.record(22, Evidence { device: 1, samples: 12, mean_de: 1.0, round: 1 });
    let ok = sealed_pass == Ok(2)
        && r.is_sealed()
        && write.err() == Some(SlotError::Sealed)
        && r.verdict(11) == Some(Verdict::Pass);
    s.add("50077-J1-封存只读", ok, if ok { "" } else { "封存后仍可写" });

    // J1-13 容量边界：第 9 个槽位必须被 Capacity 拒绝。
    let mut cap = SlotRegistry::new();
    let mut i = 1u16;
    while i <= MAX_SLOTS as u16 {
        let _ = cap.register(slot(i, 1, 1, Gate::Always));
        i += 1;
    }
    let over = cap.register(slot(99, 1, 2, Gate::Always));
    let ok = over.err() == Some(SlotError::Capacity) && cap.tally().total() == MAX_SLOTS;
    s.add("50077-J1-容量边界", ok, if ok { "" } else { "容量未封顶" });

    // J1-14 确定性：同一计划两条路径的判定与报告逐字一致。
    let mut a = SlotRegistry::from_plan(&baseline_plan());
    let mut b = SlotRegistry::from_plan(&baseline_plan());
    let same = match (&mut a, &mut b) {
        (Ok(x), Ok(y)) => {
            let e = Evidence { device: 1, samples: 12, mean_de: 1.4, round: 2 };
            let _ = x.record(11, e);
            let _ = y.record(11, e);
            x.report() == y.report() && x.verdict(11) == y.verdict(11)
        }
        _ => false,
    };
    s.add("50077-J1-判定确定", same, if same { "" } else { "同输入判定漂移" });

    // 注入核：8 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50077-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50077_checks_green() {
        let set = run_unx50077_checks();
        assert!(set.all_passed(), "UNX-F50077 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50077_small_sample_stays_untested() {
        let ev = Evidence { device: 1, samples: 7, mean_de: 0.1, round: 1 };
        assert_eq!(judge(ev), Verdict::Untested);
    }

    #[test]
    fn unx50077_codes_in_segment() {
        assert_eq!(SlotError::Empty.code(), SEG_BASE + 1);
        assert_eq!(SlotError::Overclaim.code(), SEG_BASE + 14);
    }
}
