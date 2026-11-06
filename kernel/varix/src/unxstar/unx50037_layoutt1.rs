//! UNX-F50037 布局真机对照登记位（T1 档 ×2 台随闸门补测槽位）（0x430c0 段）
//!
//! 锚点：UNX-F50037-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 数据段：`SEG_BASE = 0x430c0`，诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 10`，
//! 逐项见 [`T1Error`]；本模块只认这十个码，收到别的码一律视为跨段串扰。
//!
//! 版面真机对照的诱人错误是「把排了版当成对过版」：N-up / 折手 / 拼接
//! 这些用例只要代码能算出来，就很容易在账面上被写成「已验证」。这里的
//! 纪律正好相反——一个槽位要从 `Unmeasured` 走到 `Measured`，必须在
//! **闸门开启、证据文本非空、偏差量在合法区间**三条同时成立时才允许，
//! 而且 `Measured` 只是声明，是否成立由 [`Slot::honest`] 逐条复核。
//! 报表里的 `inflated` 是「声明已测」与「经得起复核」两个计数的差额，
//! 恒应为 0；非 0 就说明登记簿在替测量说谎（§AI-63 §5 诚实三态）。
//!
//! 结构清单：
//! * [`Device`] / [`CaseKind`]：2 台 × 6 类版面用例的固定矩阵；
//! * [`Slot`] + [`Claim`] + [`Gate`]：单槽状态机（未测 / 挂起 / 已测）；
//! * [`T1Registry`]：定容登记表（登记 / 开闸 / 测量 / 挂起 / 报表）；
//! * [`Report`]：三态守恒账 + `inflated` + `rejected`；
//! * [`injection_core`]：7 类坏样本注入核，逐类断言被显式拒绝。
//!
//! 偏差量用「百分之一毫米」整数记账，而不是浮点：真机量测报告本身就是
//! 0.01 mm 分辨率，整数让账本可逐位复算，也把 NaN/Inf 这类脏值挡在类型外。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x430c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x430c0;

const E_SLOT_UNKNOWN: u16 = 1;
const E_DUPLICATE: u16 = 2;
const E_FULL: u16 = 3;
const E_GATE_CLOSED: u16 = 4;
const E_NO_EVIDENCE: u16 = 5;
const E_BAD_MISALIGN: u16 = 6;
const E_STATE: u16 = 7;
const E_LONG_EVIDENCE: u16 = 8;
const E_BAD_SLOT_ID: u16 = 9;
const E_EMPTY_TEXT: u16 = 10;

/// 登记位错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T1Error {
    /// 槽位号未登记。
    SlotUnknown,
    /// 同一 (设备, 用例) 重复登记。
    Duplicate,
    /// 登记表容量已满。
    Full,
    /// 闸门未开就写测量值。
    GateClosed,
    /// 证据文本为空或全空白。
    NoEvidence,
    /// 偏差量越出 0..=500（即 0.00..=5.00 mm）。
    BadMisalign,
    /// 状态机不允许的迁移。
    State,
    /// 证据文本超长（> 64 字节）。
    LongEvidence,
    /// 槽位号不在本模块的号段内。
    BadSlotId,
    /// 待写入的文本为空（设备标签 / 证据共用的空串判定）。
    EmptyText,
}

impl T1Error {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            T1Error::SlotUnknown => E_SLOT_UNKNOWN,
            T1Error::Duplicate => E_DUPLICATE,
            T1Error::Full => E_FULL,
            T1Error::GateClosed => E_GATE_CLOSED,
            T1Error::NoEvidence => E_NO_EVIDENCE,
            T1Error::BadMisalign => E_BAD_MISALIGN,
            T1Error::State => E_STATE,
            T1Error::LongEvidence => E_LONG_EVIDENCE,
            T1Error::BadSlotId => E_BAD_SLOT_ID,
            T1Error::EmptyText => E_EMPTY_TEXT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志用，不参与判据）。
    pub fn message(self) -> &'static str {
        match self {
            T1Error::SlotUnknown => "slot not registered",
            T1Error::Duplicate => "slot already registered",
            T1Error::Full => "registry full",
            T1Error::GateClosed => "measurement gate closed",
            T1Error::NoEvidence => "evidence text empty",
            T1Error::BadMisalign => "misalignment outside 0..=500 (0.01mm)",
            T1Error::State => "claim transition not allowed",
            T1Error::LongEvidence => "evidence text longer than 64 bytes",
            T1Error::BadSlotId => "slot id outside this module's range",
            T1Error::EmptyText => "text argument empty",
        }
    }
}

/// T1 档真机台位：固定两台，次序稳定（台账顺序、覆盖矩阵都由它定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// 一号台（T1-A）。
    T1a,
    /// 二号台（T1-B）。
    T1b,
}

impl Device {
    /// 台位下标 0/1。
    pub fn index(self) -> usize {
        match self {
            Device::T1a => 0,
            Device::T1b => 1,
        }
    }

    /// 台位标签。
    pub fn tag(self) -> &'static str {
        match self {
            Device::T1a => "LAY-A",
            Device::T1b => "LAY-B",
        }
    }

    /// 解析台位标签；不认识返回 `None`（不猜、不兜底）。
    pub fn from_tag(t: &str) -> Option<Device> {
        if t == "LAY-A" {
            Some(Device::T1a)
        } else if t == "LAY-B" {
            Some(Device::T1b)
        } else {
            None
        }
    }
}

/// 版面用例类别：本批 20 张卡里可上真机的六类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 2-up 纵向对开。
    Nup2,
    /// 4-up 田字。
    Nup4,
    /// 小册子折手。
    Booklet,
    /// 2×2 海报拼接。
    Poster4,
    /// 双面长边翻页。
    Duplex,
    /// 180° 旋转装订口。
    Rotate180,
}

impl CaseKind {
    /// 六类用例的固定顺序（也用于覆盖矩阵列号）。
    pub const ALL: [CaseKind; 6] = [
        CaseKind::Nup2,
        CaseKind::Nup4,
        CaseKind::Booklet,
        CaseKind::Poster4,
        CaseKind::Duplex,
        CaseKind::Rotate180,
    ];

    /// 列下标 0..6。
    pub fn index(self) -> usize {
        match self {
            CaseKind::Nup2 => 0,
            CaseKind::Nup4 => 1,
            CaseKind::Booklet => 2,
            CaseKind::Poster4 => 3,
            CaseKind::Duplex => 4,
            CaseKind::Rotate180 => 5,
        }
    }

    /// 用例标签（台账一行一条用）。
    pub fn tag(self) -> &'static str {
        match self {
            CaseKind::Nup2 => "nup2",
            CaseKind::Nup4 => "nup4",
            CaseKind::Booklet => "booklet",
            CaseKind::Poster4 => "poster4",
            CaseKind::Duplex => "duplex",
            CaseKind::Rotate180 => "rotate180",
        }
    }

    /// 解析用例标签。
    pub fn from_tag(t: &str) -> Option<CaseKind> {
        if t == "nup2" {
            Some(CaseKind::Nup2)
        } else if t == "nup4" {
            Some(CaseKind::Nup4)
        } else if t == "booklet" {
            Some(CaseKind::Booklet)
        } else if t == "poster4" {
            Some(CaseKind::Poster4)
        } else if t == "duplex" {
            Some(CaseKind::Duplex)
        } else if t == "rotate180" {
            Some(CaseKind::Rotate180)
        } else {
            None
        }
    }
}

/// 诚实三态。`Measured` 只是声明，成立与否由 [`Slot::honest`] 复核。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// 从未尝试。
    Unmeasured,
    /// 已排队补测（等闸门 / 等设备）。
    Pending,
    /// 已写入测量值（必须带证据）。
    Measured,
}

impl Claim {
    /// 一行标签。
    pub fn label(self) -> &'static str {
        match self {
            Claim::Unmeasured => "unmeasured",
            Claim::Pending => "pending",
            Claim::Measured => "measured",
        }
    }
}

/// 补测闸门：关闭时任何写入都必须被拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 关闭。
    Closed,
    /// 开启。
    Open,
}

/// 一个对照槽位。
#[derive(Clone, Debug)]
pub struct Slot {
    /// 槽位号 = SLOT_BASE + device*6 + case。
    pub id: u16,
    /// 台位。
    pub device: Device,
    /// 用例类别。
    pub case: CaseKind,
    /// 三态声明。
    pub claim: Claim,
    /// 闸门状态。
    pub gate: Gate,
    /// 最近一次版面偏差（0.01 mm）；未测时为 0。
    pub misalign: u16,
    /// 证据文本（工单号 / 扫描件名 / 仪器序列号摘要）。
    pub evidence: String,
    /// 写入尝试次数（含被拒，只增不减）。
    pub attempts: u16,
    /// 达标次数（honest 且 misalign ≤ T1_TOLERANCE）。
    pub passes: u16,
}

impl Slot {
    /// 声明是否经得起复核：未声明 `Measured` 谈不上虚报；
    /// 声明了就必须「闸门开着 + 证据非空 + 偏差合法」三证齐备。
    pub fn honest(&self) -> bool {
        if self.claim != Claim::Measured {
            return true;
        }
        !self.evidence.trim().is_empty() && self.gate == Gate::Open && self.misalign <= MISALIGN_MAX
    }

    /// 是否既测到达标（只用于统计，不参与三态判定）。
    pub fn passed(&self) -> bool {
        self.claim == Claim::Measured && self.honest() && self.misalign <= T1_TOLERANCE
    }

    /// 是否仍待补测。
    pub fn awaiting(&self) -> bool {
        self.claim != Claim::Measured
    }

    /// 台账一行。
    pub fn ledger_line(&self) -> String {
        format!(
            "{} {} {} {} misalign={} ev=\"{}\" try={} pass={}",
            self.id,
            self.device.tag(),
            self.case.tag(),
            self.claim.label(),
            self.misalign,
            self.evidence,
            self.attempts,
            self.passes
        )
    }
}

/// 用例类别数（覆盖矩阵列数）。
pub const CASE_COUNT: usize = 6;
/// 台位数。
pub const DEVICE_COUNT: usize = 2;
/// 固定矩阵槽位数：2 台 × 6 类 = 12。
pub const SLOT_COUNT: usize = DEVICE_COUNT * CASE_COUNT;
/// 登记表容量：矩阵 12 格 + 12 个复测槽。
pub const DEFAULT_CAPACITY: usize = 24;
/// T1 判定容差：2.00 mm（版面套准在真机上的可见阈值）。
pub const T1_TOLERANCE: u16 = 200;
/// 偏差合法上限：5.00 mm；超过即视为量测错误而不是版面偏差。
pub const MISALIGN_MAX: u16 = 500;
/// 证据文本字节上限。
pub const EVIDENCE_CAP: usize = 64;
/// 槽位号基址（号段 0x0C00..=0x0C17）。
pub const SLOT_BASE: u16 = 0x0C00;

/// 由 (台位, 用例) 推导稳定槽位号。
pub fn slot_id(device: Device, case: CaseKind) -> u16 {
    SLOT_BASE + (device.index() * CASE_COUNT + case.index()) as u16
}

/// 解析台位标签：空串落到 `EmptyText`，不认识落到 `SlotUnknown`
/// （两类失败分开记，日志里才分得清「漏填」和「填错」）。
pub fn parse_device(tag: &str) -> Result<Device, T1Error> {
    if tag.trim().is_empty() {
        return Err(T1Error::EmptyText);
    }
    match Device::from_tag(tag) {
        Some(d) => Ok(d),
        None => Err(T1Error::SlotUnknown),
    }
}

/// 三态守恒账。`inflated` = 声明已测 − 经得起复核，恒应为 0。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 槽位总数。
    pub total: usize,
    /// 声明已测。
    pub measured: usize,
    /// 挂起。
    pub pending: usize,
    /// 未尝试。
    pub unmeasured: usize,
    /// 其中达标。
    pub passed: usize,
    /// 其中不达标。
    pub failed: usize,
    /// 虚报（必须为 0）。
    pub inflated: usize,
    /// 累计被拒动作数。
    pub rejected: u32,
    /// 已开闸槽位数。
    pub gates_open: usize,
}

impl Report {
    /// 三态守恒是否成立（total 恒等于三者之和，与 inflated 无关）。
    pub fn conserved(&self) -> bool {
        self.total == self.measured + self.pending + self.unmeasured
    }

    /// 一行对账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "total={} m={} p={} u={} pass={} fail={} infl={} rej={}",
            self.total, self.measured, self.pending, self.unmeasured, self.passed, self.failed,
            self.inflated, self.rejected
        )
    }
}

/// 定容槽位登记表。所有写入口返回 `Result`，被拒动作计入 `rejected`。
pub struct T1Registry {
    slots: Vec<Slot>,
    capacity: usize,
    rejected: u32,
}

impl T1Registry {
    /// 建空表（容量固定，避免运行期扩容带来的顺序不稳定）。
    pub fn new(capacity: usize) -> T1Registry {
        let cap = if capacity == 0 { 1 } else { capacity };
        T1Registry {
            slots: Vec::with_capacity(cap),
            capacity: cap,
            rejected: 0,
        }
    }

    /// 默认容量表。
    pub fn with_default_capacity() -> T1Registry {
        T1Registry::new(DEFAULT_CAPACITY)
    }

    fn reject<T>(&mut self, e: T1Error) -> Result<T, T1Error> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    fn index_of(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按槽位号取只读视图。
    pub fn slot(&self, id: u16) -> Option<&Slot> {
        match self.index_of(id) {
            Some(i) => self.slots.get(i),
            None => None,
        }
    }

    /// 槽位数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 登记一个 (台位, 用例) 槽位；号由矩阵位置推导，重复即拒。
    pub fn register(&mut self, device: Device, case: CaseKind) -> Result<u16, T1Error> {
        let id = slot_id(device, case);
        if self.index_of(id).is_some() {
            return self.reject(T1Error::Duplicate);
        }
        if self.slots.len() >= self.capacity {
            return self.reject(T1Error::Full);
        }
        self.slots.push(Slot {
            id,
            device,
            case,
            claim: Claim::Unmeasured,
            gate: Gate::Closed,
            misalign: 0,
            evidence: String::new(),
            attempts: 0,
            passes: 0,
        });
        Ok(id)
    }

    /// 在表中查 (台位, 用例) 的槽位号。
    pub fn find(&self, device: Device, case: CaseKind) -> Option<u16> {
        let id = slot_id(device, case);
        match self.index_of(id) {
            Some(_) => Some(id),
            None => None,
        }
    }

    fn mutate(&mut self, id: u16, f: impl FnOnce(&mut Slot)) -> Result<(), T1Error> {
        match self.index_of(id) {
            Some(i) => match self.slots.get_mut(i) {
                Some(s) => {
                    f(s);
                    Ok(())
                }
                None => self.reject(T1Error::SlotUnknown),
            },
            None => self.reject(T1Error::SlotUnknown),
        }
    }

    /// 开闸：只有开闸的槽位才允许写入测量。
    pub fn open_gate(&mut self, id: u16) -> Result<(), T1Error> {
        if id < SLOT_BASE || id >= SLOT_BASE + SLOT_COUNT as u16 {
            return self.reject(T1Error::BadSlotId);
        }
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        let measured = match self.slots.get(idx) {
            Some(s) => s.claim == Claim::Measured,
            None => false,
        };
        if measured {
            // 已测槽位重开闸等于允许改写历史，直接拒。
            return self.reject(T1Error::State);
        }
        self.mutate(id, |s| {
            s.gate = Gate::Open;
        })
    }

    /// 关闸：闸门随时可关（环境失效），但已测结果不被抹掉。
    pub fn close_gate(&mut self, id: u16) -> Result<(), T1Error> {
        self.mutate(id, |s| {
            s.gate = Gate::Closed;
        })
    }

    /// 挂起补测：只允许从 `Unmeasured` 迁到 `Pending`；
    /// `Pending` 再挂起是幂等的，`Measured` 降级是禁的（不许抹掉已测记录）。
    pub fn suspend(&mut self, id: u16) -> Result<(), T1Error> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        let claim = match self.slots.get(idx) {
            Some(s) => s.claim,
            None => return self.reject(T1Error::SlotUnknown),
        };
        match claim {
            Claim::Unmeasured => {}
            Claim::Pending => return Ok(()),
            Claim::Measured => return self.reject(T1Error::State),
        }
        self.mutate(id, |s| {
            s.claim = Claim::Pending;
        })
    }

    /// 写入测量：闸门开启 + 证据非空且不超长 + 偏差在合法区间，四条缺一不可。
    pub fn measure(&mut self, id: u16, misalign: u16, evidence: &str) -> Result<(), T1Error> {
        let trimmed = evidence.trim();
        if trimmed.is_empty() {
            return self.reject(T1Error::NoEvidence);
        }
        if evidence.as_bytes().len() > EVIDENCE_CAP {
            return self.reject(T1Error::LongEvidence);
        }
        if misalign > MISALIGN_MAX {
            return self.reject(T1Error::BadMisalign);
        }
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        let gate_open = match self.slots.get(idx) {
            Some(s) => s.gate == Gate::Open,
            None => false,
        };
        if !gate_open {
            return self.reject(T1Error::GateClosed);
        }
        let pass = misalign <= T1_TOLERANCE;
        self.mutate(id, |s| {
            s.misalign = misalign;
            s.evidence = trimmed.to_string();
            s.claim = Claim::Measured;
            s.attempts = s.attempts.saturating_add(1);
            if pass {
                s.passes = s.passes.saturating_add(1);
            }
        })
    }

    /// 全表统计：三态计数、达标/不达标、虚报差额。
    pub fn report(&self) -> Report {
        let mut measured = 0usize;
        let mut pending = 0usize;
        let mut unmeasured = 0usize;
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut inflated = 0usize;
        let mut gates_open = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                match s.claim {
                    Claim::Measured => measured += 1,
                    Claim::Pending => pending += 1,
                    Claim::Unmeasured => unmeasured += 1,
                }
                if s.claim == Claim::Measured && !s.honest() {
                    inflated += 1;
                }
                if s.passed() {
                    passed += 1;
                } else if s.claim == Claim::Measured {
                    failed += 1;
                }
                if s.gate == Gate::Open {
                    gates_open += 1;
                }
            }
            i += 1;
        }
        Report {
            total: self.slots.len(),
            measured,
            pending,
            unmeasured,
            passed,
            failed,
            inflated,
            rejected: self.rejected,
            gates_open,
        }
    }

    /// 覆盖矩阵：`matrix[device][case]` = 该格是否「已测且经得起复核」。
    pub fn coverage(&self) -> [[bool; CASE_COUNT]; DEVICE_COUNT] {
        let mut m = [[false; CASE_COUNT]; DEVICE_COUNT];
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                let d = s.device.index();
                let c = s.case.index();
                if s.claim == Claim::Measured && s.honest() {
                    if let Some(row) = m.get_mut(d) {
                        if let Some(cell) = row.get_mut(c) {
                            *cell = true;
                        }
                    }
                }
            }
            i += 1;
        }
        m
    }

    /// 覆盖矩阵已填格数（0..=12）。
    pub fn covered_cells(&self) -> usize {
        let m = self.coverage();
        let mut n = 0usize;
        let mut d = 0usize;
        while d < DEVICE_COUNT {
            if let Some(row) = m.get(d) {
                let mut c = 0usize;
                while c < CASE_COUNT {
                    if let Some(v) = row.get(c) {
                        if *v {
                            n += 1;
                        }
                    }
                    c += 1;
                }
            }
            d += 1;
        }
        n
    }

    /// 全表台账文本（逐槽一行，按登记序）。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                out.push_str(&s.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 按固定矩阵登记 12 个槽位（不测量）。
pub fn build_matrix() -> T1Registry {
    let mut r = T1Registry::with_default_capacity();
    let devices = [Device::T1a, Device::T1b];
    let mut d = 0usize;
    while d < devices.len() {
        if let Some(dev) = devices.get(d) {
            let mut c = 0usize;
            while c < CaseKind::ALL.len() {
                if let Some(case) = CaseKind::ALL.get(c) {
                    let _ = r.register(*dev, *case);
                }
                c += 1;
            }
        }
        d += 1;
    }
    r
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 未登记的槽位号：凭空写测量。
    let mut r = build_matrix();
    if r.measure(0x0CFF, 10, "scan-a").err() == Some(T1Error::SlotUnknown) {
        caught += 1;
    }

    // 2) 重复登记同一 (台位, 用例)。
    if r.register(Device::T1a, CaseKind::Nup4).err() == Some(T1Error::Duplicate) {
        caught += 1;
    }

    // 3) 闸门关闭就写测量。
    let id = slot_id(Device::T1a, CaseKind::Nup2);
    if r.measure(id, 10, "scan-b").err() == Some(T1Error::GateClosed) {
        caught += 1;
    }

    // 4) 证据为空（纯空白也算空）。
    let _ = r.open_gate(id);
    let no_ev = r.measure(id, 10, "   ").err() == Some(T1Error::NoEvidence);
    if no_ev {
        caught += 1;
    }

    // 5) 偏差越界（501 = 5.01 mm）。
    let bad_mm = r.measure(id, MISALIGN_MAX + 1, "scan-c").err() == Some(T1Error::BadMisalign);
    if bad_mm {
        caught += 1;
    }

    // 6) 证据超长（65 字节）。
    let long = "e".repeat(EVIDENCE_CAP + 1);
    let long_rej = r.measure(id, 10, &long).err() == Some(T1Error::LongEvidence);
    if long_rej {
        caught += 1;
    }

    // 7) 已测槽位挂起（Measured → Pending 禁）。
    let _ = r.measure(id, 200, "scan-d");
    if r.suspend(id).err() == Some(T1Error::State) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50037 自检：17 条判据。
pub fn run_unx50037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50037");

    // J1-1 固定矩阵登记：12 槽、卡号连续、每格恰好一个槽。
    let base = build_matrix();
    let mut contiguous = base.len() == SLOT_COUNT;
    let mut i = 0usize;
    while i < base.len() {
        let want = SLOT_BASE + i as u16;
        match base.slots.get(i) {
            Some(sl) => {
                if sl.id != want {
                    contiguous = false;
                }
            }
            None => contiguous = false,
        }
        i += 1;
    }
    let det = format!("slots={} covered={}", base.len(), base.covered_cells());
    s.add_dyn("50037-J1-矩阵登记", contiguous, &det);

    // J1-2 初始三态：12 条全部 Unmeasured，覆盖 0，虚报 0。
    let r0 = base.report();
    let ok = r0.unmeasured == SLOT_COUNT && r0.measured == 0 && r0.inflated == 0 && base.covered_cells() == 0;
    let det = r0.ledger_line();
    s.add_dyn("50037-J1-初始不虚报", ok, &det);

    // J1-3 闸门关闭写测量必拒，且 rejected 递增。
    let mut r = build_matrix();
    let id = match r.find(Device::T1a, CaseKind::Nup2) {
        Some(v) => v,
        None => 0,
    };
    let before = r.rejected();
    let closed = r.measure(id, 10, "scan-01").err() == Some(T1Error::GateClosed);
    let attempts_after = match r.slot(id) {
        Some(sl) => sl.attempts,
        None => 9,
    };
    let ok = closed && r.rejected() == before + 1 && attempts_after == 0;
    s.add("50037-J1-闸门挡写", ok, if ok { "" } else { "闸门关闭仍写入了测量" });

    // J1-4 空证据必拒（开闸也不许写）。
    let _ = r.open_gate(id);
    let ok = r.measure(id, 10, "\t \n").err() == Some(T1Error::NoEvidence);
    s.add("50037-J1-空证据拒", ok, if ok { "" } else { "空证据被接受" });

    // J1-5 偏差越界必拒（500 收、501 拒）。
    let ok_edge = r.measure(id, MISALIGN_MAX, "scan-02").is_ok();
    let ok_over = r.measure(id, MISALIGN_MAX + 1, "scan-03").err() == Some(T1Error::BadMisalign);
    let ok = ok_edge && ok_over;
    s.add("50037-J1-偏差边界", ok, if ok { "" } else { "偏差边界判定错" });

    // J1-6 合法测量：三证齐备才进 Measured，且 honest 成立。
    let mut r = build_matrix();
    let id = match r.find(Device::T1b, CaseKind::Booklet) {
        Some(v) => v,
        None => 0,
    };
    let gated = r.open_gate(id).is_ok();
    let measured = r.measure(id, 180, "ticket-7731 fold").is_ok();
    let ok = match r.slot(id) {
        Some(sl) => {
            gated && measured && sl.claim == Claim::Measured && sl.honest() && sl.passed() && sl.attempts == 1
        }
        None => false,
    };
    s.add("50037-J1-三证入测", ok, if ok { "" } else { "合法测量未入账" });

    // J1-7 不达标也诚实：偏差 250 > 容差 200，Measured + honest + !passed。
    let id2 = match r.find(Device::T1a, CaseKind::Poster4) {
        Some(v) => v,
        None => 0,
    };
    let _ = r.open_gate(id2);
    let _ = r.measure(id2, 250, "ticket-7732 poster");
    let ok = match r.slot(id2) {
        Some(sl) => sl.claim == Claim::Measured && sl.honest() && !sl.passed(),
        None => false,
    };
    let rep = r.report();
    let ok = ok && rep.passed == 1 && rep.failed == 1 && rep.inflated == 0;
    let det = rep.ledger_line();
    s.add_dyn("50037-J1-达标不达标分账", ok, &det);

    // J1-8 三态守恒：total = measured + pending + unmeasured。
    let id3 = match r.find(Device::T1b, CaseKind::Nup4) {
        Some(v) => v,
        None => 0,
    };
    let _ = r.suspend(id3);
    let rep = r.report();
    let ok = rep.conserved() && rep.total == SLOT_COUNT && rep.pending == 1;
    let det = rep.ledger_line();
    s.add_dyn("50037-J1-三态守恒", ok, &det);

    // J1-9 状态机：Measured 不许降级挂起；Pending 再挂起幂等。
    let measured_id = match r.find(Device::T1b, CaseKind::Booklet) {
        Some(v) => v,
        None => 0,
    };
    let down = r.suspend(measured_id).err() == Some(T1Error::State);
    let idem = r.suspend(id3).is_ok() && r.slot(id3).map(|x| x.claim) == Some(Claim::Pending);
    let ok = down && idem && r.report().conserved();
    s.add("50037-J1-状态机", ok, if ok { "" } else { "三态迁移越权" });

    // J1-10 已测槽位重开闸必拒（不许掩盖历史）。
    let reopen = r.open_gate(measured_id).err() == Some(T1Error::State);
    let ok = reopen && r.slot(measured_id).map(|x| x.gate) == Some(Gate::Open);
    s.add("50037-J1-重开闸拒", ok, if ok { "" } else { "已测槽位被重开闸" });

    // J1-11 证据超长必拒，长度上限内共 64 字节，65 字节拒。
    let mut r2 = build_matrix();
    let id = match r2.find(Device::T1a, CaseKind::Duplex) {
        Some(v) => v,
        None => 0,
    };
    let _ = r2.open_gate(id);
    let ok64 = r2.measure(id, 100, &"a".repeat(EVIDENCE_CAP)).is_ok();
    let id2 = match r2.find(Device::T1a, CaseKind::Rotate180) {
        Some(v) => v,
        None => 0,
    };
    let _ = r2.open_gate(id2);
    let bad65 = r2.measure(id2, 100, &"a".repeat(EVIDENCE_CAP + 1)).err() == Some(T1Error::LongEvidence);
    let ok = ok64 && bad65;
    s.add("50037-J1-证据长度", ok, if ok { "" } else { "证据长度上下界错" });

    // J1-12 覆盖矩阵：只统计 honest 的已测格；2 台 × 6 列，行列各不超界。
    let m = r2.coverage();
    let mut rows_ok = true;
    let mut d = 0usize;
    while d < DEVICE_COUNT {
        let mut c = 0usize;
        while c < CASE_COUNT {
            if m.get(d).and_then(|row| row.get(c)).is_none() {
                rows_ok = false;
            }
            c += 1;
        }
        d += 1;
    }
    let ok = rows_ok && r2.covered_cells() == 1 && r2.report().measured == 1;
    let det = format!("covered={} measured={}", r2.covered_cells(), r2.report().measured);
    s.add_dyn("50037-J1-覆盖矩阵", ok, &det);

    // J1-13 未登记槽位与越界号段两类都拒。
    let unknown = r2.measure(0x0CFF, 1, "x").err() == Some(T1Error::SlotUnknown);
    let bad_seg = r2.open_gate(0x4400).err() == Some(T1Error::BadSlotId);
    let ok = unknown && bad_seg;
    s.add("50037-J1-未知槽拒", ok, if ok { "" } else { "未知槽/越段未被拒" });

    // J1-14 容量防线：容量 6 的新表登记第 7 个槽位即 `Full`，已有 6 条不受影响。
    let mut rc = T1Registry::new(6);
    let mut registered = 0usize;
    let mut c = 0usize;
    while c < CASE_COUNT {
        if let Some(case) = CaseKind::ALL.get(c) {
            if rc.register(Device::T1a, *case).is_ok() {
                registered += 1;
            }
        }
        c += 1;
    }
    let overflow = rc.register(Device::T1b, CaseKind::Nup2).err() == Some(T1Error::Full);
    let ok = registered == 6 && rc.len() == 6 && overflow && rc.rejected() == 1;
    let det = format!("len={} rej={}", rc.len(), rc.rejected());
    s.add_dyn("50037-J1-容量防线", ok, &det);

    // J1-15 标签解析：往返一致；空串与错标签分别落到不同诊断码。
    let rt = parse_device("LAY-B").ok() == Some(Device::T1b)
        && parse_device("LAY-A").ok() == Some(Device::T1a);
    let empty = parse_device("  ").err() == Some(T1Error::EmptyText);
    let typo = parse_device("LAY-C").err() == Some(T1Error::SlotUnknown);
    let ok = rt
        && empty
        && typo
        && Device::T1a.tag() == "LAY-A"
        && CaseKind::from_tag("booklet") == Some(CaseKind::Booklet)
        && CaseKind::from_tag("nup9").is_none();
    s.add("50037-J1-标签解析", ok, if ok { "" } else { "标签往返/坏标签判定错" });

    // J1-16 确定性：同一操作序列两遍得到逐字段一致的报表与台账。
    let a = scenario_report();
    let b = scenario_report();
    let ok = a == b && a.conserved() && a.inflated == 0 && a.measured == 3;
    let det = a.ledger_line();
    s.add_dyn("50037-J1-序列确定", ok, &det);

    // J1-17 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50037-J1-注入必抓", ok, &det);

    s
}

/// 固定操作序列（确定性判据用）：2 台各测若干，其余保持待测。
fn scenario_report() -> Report {
    let mut r = build_matrix();
    let plan = [
        (Device::T1a, CaseKind::Nup2, 90u16, "t-1"),
        (Device::T1a, CaseKind::Booklet, 260u16, "t-2"),
        (Device::T1b, CaseKind::Poster4, 0u16, "t-3"),
    ];
    let mut i = 0usize;
    while i < plan.len() {
        if let Some((dev, case, mm, ev)) = plan.get(i) {
            if let Some(id) = r.find(*dev, *case) {
                let _ = r.open_gate(id);
                let _ = r.measure(id, *mm, *ev);
            }
        }
        i += 1;
    }
    r.report()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50037_checks_green() {
        let set = run_unx50037_checks();
        assert!(set.all_passed(), "UNX-F50037 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50037_honest_three_states() {
        let mut r = build_matrix();
        let id = match r.find(Device::T1a, CaseKind::Nup2) {
            Some(v) => v,
            None => 0,
        };
        // 未开闸 → 写不进 → 仍是 Unmeasured，报表不虚报。
        let _ = r.measure(id, 10, "ev");
        let rep = r.report();
        assert_eq!(rep.measured, 0);
        assert_eq!(rep.unmeasured, SLOT_COUNT);
        assert_eq!(rep.inflated, 0);
        assert!(rep.conserved());
    }

    #[test]
    fn unx50037_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
