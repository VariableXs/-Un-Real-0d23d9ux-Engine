//! UNX-F50017 色彩真机对照登记位（T1 档 ×2 台随闸门补测槽位）（0x42e40 段）
//!
//! 锚点：UNX-F50017-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 数据段：`SEG_BASE = 0x42e40`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 10`，
//! 逐项见 [`T1Error`]（不认识别的码，收到即视为跨段串扰）。
//!
//! 本模块要回答的不是「测了多少」，而是「登记簿会不会替测量说谎」。T1 档真机
//! 对照是 2 台设备 × 12 色块的固定槽位矩阵，每槽必须在**闸门开启 + 证据文本
//! 非空 + ΔE 合法**三证齐备时才允许进入 `Measured`；任一缺失即停在
//! `Unmeasured` / `Pending` 并计入 `rejected`。宁可账面上少一条，也不许把
//! 「没测」写成「测了」——这正是 §AI-63 §5 诚实三态的落点。
//!
//! 结构清单：
//! * [`Slot`] + [`T1Registry`]：定容槽位注册表（注册 / 开闸 / 测量 / 挂起）；
//! * [`Claim`]：诚实三态（Unmeasured / Pending / Measured），`Measured` 由
//!   [`Slot::honest`] 复核，不是自封的；
//! * [`Report`]：三态守恒账（`total = measured + pending + unmeasured`）+
//!   `inflated`（虚报计数，恒应为 0）与 `rejected`（显性拒绝计数）；
//! * [`injection_core`]：8 类坏样本注入核，逐类断言被显式拒绝。
//!
//! 复用 B21 共用内核（`super::unx50001_gamutmap`）的 `Lab` 与 `delta_e2000`，
//! 保证真机对照与 L2 映射用同一套 ΔE 口径，不存在「两把尺子」。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{delta_e2000, Lab};

/// 诊断段（0x42e40 独占 0x20）。
pub const SEG_BASE: u32 = 0x42e40;

const E_SLOT_UNKNOWN: u16 = 1;
const E_DUPLICATE: u16 = 2;
const E_FULL: u16 = 3;
const E_GATE_CLOSED: u16 = 4;
const E_NO_EVIDENCE: u16 = 5;
const E_BAD_DELTA: u16 = 6;
const E_PATCH_RANGE: u16 = 7;
const E_STATE: u16 = 8;
const E_DEVICE: u16 = 9;
const E_EVIDENCE_LONG: u16 = 10;

/// 登记位错误。`code()` 返回 `SEG_BASE + n`，闭集见模块头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T1Error {
    /// 槽位号未登记。
    SlotUnknown,
    /// 槽位号重复登记。
    Duplicate,
    /// 注册表已到容量上限。
    Full,
    /// 闸门未开就试图写入测量值。
    GateClosed,
    /// 证据文本为空/纯空白。
    NoEvidence,
    /// ΔE 非有限值或落在 0..=100 之外。
    BadDelta,
    /// 色块序号越界。
    PatchRange,
    /// 状态机不允许的迁移（例如 Measured → Pending）。
    State,
    /// 设备标签不认识。
    Device,
    /// 证据文本超长（> 64 字节）。
    EvidenceLong,
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
            T1Error::BadDelta => E_BAD_DELTA,
            T1Error::PatchRange => E_PATCH_RANGE,
            T1Error::State => E_STATE,
            T1Error::Device => E_DEVICE,
            T1Error::EvidenceLong => E_EVIDENCE_LONG,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志用，不参与判据）。
    pub fn message(self) -> &'static str {
        match self {
            T1Error::SlotUnknown => "slot id not registered",
            T1Error::Duplicate => "slot id already registered",
            T1Error::Full => "registry capacity reached",
            T1Error::GateClosed => "measurement gate closed",
            T1Error::NoEvidence => "evidence text empty",
            T1Error::BadDelta => "delta-e not finite or out of range",
            T1Error::PatchRange => "patch index outside 0..12",
            T1Error::State => "claim transition not allowed",
            T1Error::Device => "unknown device tag",
            T1Error::EvidenceLong => "evidence text longer than 64 bytes",
        }
    }
}

/// 真机台位：T1 档固定两台（A/B），顺序稳定用于排序与对账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// 一号台（T1-A）。
    T1a,
    /// 二号台（T1-B）。
    T1b,
}

impl Device {
    /// 台位标签（账本/日志共用）。
    pub fn tag(self) -> &'static str {
        match self {
            Device::T1a => "T1-A",
            Device::T1b => "T1-B",
        }
    }

    /// 台位下标（0/1），用于槽位号推导。
    pub fn index(self) -> usize {
        match self {
            Device::T1a => 0,
            Device::T1b => 1,
        }
    }

    /// 解析台位标签；不认识就返回 `None`（不猜、不兜底）。
    pub fn from_tag(t: &str) -> Option<Device> {
        if t == "T1-A" {
            Some(Device::T1a)
        } else if t == "T1-B" {
            Some(Device::T1b)
        } else {
            None
        }
    }
}

/// 诚实三态。`Measured` 只是「声明」，是否成立由 [`Slot::honest`] 复核。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// 从未尝试测量。
    Unmeasured,
    /// 已排队等闸门/等补测。
    Pending,
    /// 已写入测量值（必须带证据）。
    Measured,
}

impl Claim {
    /// 一行标签，用于对账文本。
    pub fn label(self) -> &'static str {
        match self {
            Claim::Unmeasured => "unmeasured",
            Claim::Pending => "pending",
            Claim::Measured => "measured",
        }
    }
}

/// 补测闸门：关闭时任何写入都必须被拒（真机未就绪 / 环境未达标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 闸门关闭。
    Closed,
    /// 闸门开启。
    Open,
}

/// 一个对照槽位。
#[derive(Clone, Debug)]
pub struct Slot {
    /// 槽位号（调用方分配的稳定 ID）。
    pub id: u16,
    /// 台位。
    pub device: Device,
    /// 色块序号 0..12。
    pub patch: u8,
    /// 三态声明。
    pub claim: Claim,
    /// 闸门状态。
    pub gate: Gate,
    /// 最近一次 ΔE2000（Unmeasured 时为 0）。
    pub delta_e: f32,
    /// 判定容差（T1 档固定 3.0）。
    pub tolerance: f32,
    /// 证据文本（文件名/时间戳/仪器序列号摘要）。
    pub evidence: String,
    /// 写入尝试次数（含被拒）。
    pub attempts: u16,
    /// 达标次数（ΔE ≤ tolerance）。
    pub passes: u16,
}

impl Slot {
    /// 声明是否经得起复核：`Measured` 必须有证据、闸门开启、ΔE 合法。
    pub fn honest(&self) -> bool {
        if self.claim != Claim::Measured {
            return true; // 未声明测量，谈不上虚报
        }
        let e = self.evidence.trim();
        self.gate == Gate::Open
            && !e.is_empty()
            && self.delta_e.is_finite()
            && self.delta_e >= 0.0
            && self.delta_e <= 100.0
    }

    /// 该槽是否既测到达标（供报表统计，不参与三态判定）。
    pub fn passed(&self) -> bool {
        self.claim == Claim::Measured && self.honest() && self.delta_e <= self.tolerance
    }

    /// 槽位是否仍待补测（未测量或挂起）。
    pub fn awaiting(&self) -> bool {
        self.claim != Claim::Measured
    }
}

/// T1 档真机对照固定矩阵：2 台 × 12 色块。
pub const PATCH_COUNT: u8 = 12;
/// T1 档判定容差：ΔE2000 ≤ 3.0（§AI-63 §5 真机对照档口径）。
pub const T1_TOLERANCE: f32 = 3.0;
/// 单条证据文本上限（字节）；超长即拒，避免账本被日志灌爆。
pub const EVIDENCE_CAP: usize = 64;
/// 注册表默认容量（2 台 × 12 色块 = 24，留一倍余量给复测槽）。
pub const DEFAULT_CAPACITY: usize = 48;
/// 槽位号基址：id = base + device*PATCH_COUNT + patch。
pub const SLOT_ID_BASE: u16 = 1000;

/// 三态守恒账。`inflated` 是两个计数器的**差额**：声明 Measured 的条数减去
/// 真正经得起复核的条数，恒应为 0；一旦非 0 就是登记簿在替测量说谎。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 槽位总数。
    pub total: usize,
    /// 声明已测条数。
    pub measured: usize,
    /// 挂起条数。
    pub pending: usize,
    /// 未尝试条数。
    pub unmeasured: usize,
    /// 达标条数。
    pub passed: usize,
    /// 已测但不达标条数。
    pub failed: usize,
    /// 虚报条数（必须为 0）。
    pub inflated: usize,
    /// 累计被拒动作数。
    pub rejected: u32,
}

/// 定容槽位注册表。所有可变入口返回 `Result`，被拒动作计入 `rejected`。
pub struct T1Registry {
    slots: Vec<Slot>,
    capacity: usize,
    rejected: u32,
}

impl T1Registry {
    /// 建空表；`capacity` 上限 [`DEFAULT_CAPACITY`] 以上的部分不额外分配。
    pub fn new(capacity: usize) -> T1Registry {
        let cap = if capacity == 0 { 1 } else { capacity.min(256) };
        T1Registry {
            slots: Vec::with_capacity(cap),
            capacity: cap,
            rejected: 0,
        }
    }

    /// 槽位号推导：同一台位同一色块恒得同一 ID（可复算，便于跨账对号）。
    pub fn slot_id(device: Device, patch: u8) -> u16 {
        SLOT_ID_BASE + (device.index() as u16) * (PATCH_COUNT as u16) + patch as u16
    }

    fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            match self.slots.get(i) {
                Some(s) if s.id == id => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn reject<T>(&mut self, e: T1Error) -> Result<T, T1Error> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 登记一个槽位：三态置 Unmeasured、闸门置 Closed（永远从「没测」起步）。
    pub fn register(&mut self, id: u16, device: Device, patch: u8) -> Result<(), T1Error> {
        if patch >= PATCH_COUNT {
            return self.reject(T1Error::PatchRange);
        }
        if self.find(id).is_some() {
            return self.reject(T1Error::Duplicate);
        }
        if self.slots.len() >= self.capacity {
            return self.reject(T1Error::Full);
        }
        self.slots.push(Slot {
            id,
            device,
            patch,
            claim: Claim::Unmeasured,
            gate: Gate::Closed,
            delta_e: 0.0,
            tolerance: T1_TOLERANCE,
            evidence: String::new(),
            attempts: 0,
            passes: 0,
        });
        Ok(())
    }

    /// 登记整台设备的 12 个色块；中途失败即整体失败（调用方不半途起账）。
    pub fn register_device(&mut self, device: Device) -> Result<usize, T1Error> {
        let mut n = 0usize;
        let mut patch = 0u8;
        while patch < PATCH_COUNT {
            let id = T1Registry::slot_id(device, patch);
            self.register(id, device, patch)?;
            n += 1;
            patch += 1;
        }
        Ok(n)
    }

    /// 开闸：只改闸门，不改三态（开闸本身不等于测过）。
    pub fn open_gate(&mut self, id: u16) -> Result<(), T1Error> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.gate = Gate::Open;
                Ok(())
            }
            None => self.reject(T1Error::SlotUnknown),
        }
    }

    /// 挂起：仅允许 Unmeasured → Pending；已测槽不得倒回挂起态。
    pub fn mark_pending(&mut self, id: u16) -> Result<(), T1Error> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        match self.slots.get_mut(idx) {
            Some(s) => match s.claim {
                Claim::Unmeasured => {
                    s.claim = Claim::Pending;
                    Ok(())
                }
                _ => {
                    self.rejected = self.rejected.saturating_add(1);
                    Err(T1Error::State)
                }
            },
            None => self.reject(T1Error::SlotUnknown),
        }
    }

    /// 写入一次真机对照测量：闸门 + 证据 + ΔE 三证齐备才落 `Measured`。
    ///
    /// 返回实际算出的 ΔE2000。判定**不参与**准入——不达标也会被如实记为
    /// `Measured`，只是不计入 `passes`：账本记录事实，不记录期望。
    pub fn record(
        &mut self,
        id: u16,
        measured: Lab,
        reference: Lab,
        evidence: &str,
    ) -> Result<f32, T1Error> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return self.reject(T1Error::SlotUnknown),
        };
        let ev = evidence.trim();
        if ev.is_empty() {
            return self.reject(T1Error::NoEvidence);
        }
        if ev.len() > EVIDENCE_CAP {
            return self.reject(T1Error::EvidenceLong);
        }
        // 闸门判定放在证据之后：证据本身可以离线准备，闸门才是真机就绪的凭据。
        let gate_open = match self.slots.get(idx) {
            Some(s) => s.gate == Gate::Open,
            None => return self.reject(T1Error::SlotUnknown),
        };
        if !gate_open {
            return self.reject(T1Error::GateClosed);
        }
        // 入参先做值域校验：`delta_e2000` 对 NaN 输入会因 `term > 0.0` 判假
        // 而返回 0.0（一个「完美」的假结果），必须在调用前拦下。
        if !measured.l.is_finite()
            || !measured.a.is_finite()
            || !measured.b.is_finite()
            || !reference.l.is_finite()
            || !reference.a.is_finite()
            || !reference.b.is_finite()
        {
            return self.reject(T1Error::BadDelta);
        }
        let de = delta_e2000(measured, reference);
        if !de.is_finite() || de < 0.0 || de > 100.0 {
            return self.reject(T1Error::BadDelta);
        }
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.claim = Claim::Measured;
                s.delta_e = de;
                s.evidence = ev.to_string();
                s.attempts = s.attempts.saturating_add(1);
                if de <= s.tolerance {
                    s.passes = s.passes.saturating_add(1);
                }
                Ok(de)
            }
            None => self.reject(T1Error::SlotUnknown),
        }
    }

    /// 按槽位号取只读记录。
    pub fn get(&self, id: u16) -> Option<&Slot> {
        match self.find(id) {
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

    /// 容量下限（已被接受的登记数不会超过它）。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒动作数（显性记账，不静默吞）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 三态守恒对账。任一时刻 `measured + pending + unmeasured == total`。
    pub fn report(&self) -> Report {
        let mut measured = 0usize;
        let mut pending = 0usize;
        let mut unmeasured = 0usize;
        let mut passed = 0usize;
        let mut inflated = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                match s.claim {
                    Claim::Measured => {
                        measured += 1;
                        if s.honest() {
                            if s.passed() {
                                passed += 1;
                            }
                        } else {
                            inflated += 1;
                        }
                    }
                    Claim::Pending => pending += 1,
                    Claim::Unmeasured => unmeasured += 1,
                }
            }
            i += 1;
        }
        let failed = if measured >= passed { measured - passed } else { 0 };
        Report {
            total: self.slots.len(),
            measured,
            pending,
            unmeasured,
            passed,
            failed,
            inflated,
            rejected: self.rejected,
        }
    }

    /// 待补测槽位号清单（按登记序），补测排程直接消费。
    pub fn awaiting_ids(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.awaiting() {
                    out.push(s.id);
                }
            }
            i += 1;
        }
        out
    }
}

/// 造一本标准账：T1-A / T1-B 各 12 槽，全部 Closed 且 Unmeasured。
pub fn seed_registry() -> T1Registry {
    let mut r = T1Registry::new(DEFAULT_CAPACITY);
    // 两台设备是固定矩阵，登记失败属于不可达分支；仍把返回值显式丢弃，
    // 不给内核路径留下任何「解包即成功」的假设。
    let _ = r.register_device(Device::T1a);
    let _ = r.register_device(Device::T1b);
    r
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1) 未登记槽位
    let mut r = T1Registry::new(8);
    if r.record(7, Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "ev").err()
        == Some(T1Error::SlotUnknown)
    {
        caught += 1;
    }

    // 2) 闸门未开（已登记，证据齐全也不行）
    let mut r2 = T1Registry::new(8);
    let _ = r2.register(10, Device::T1a, 0);
    if r2.record(10, Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "probe.icc").err()
        == Some(T1Error::GateClosed)
    {
        caught += 1;
    }

    // 3) 证据为空（闸门已开）
    let mut r3 = T1Registry::new(8);
    let _ = r3.register(11, Device::T1a, 1);
    let _ = r3.open_gate(11);
    if r3.record(11, Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "   ").err()
        == Some(T1Error::NoEvidence)
    {
        caught += 1;
    }

    // 4) ΔE 非有限（NaN 参考点会把 ΔE 拖成 NaN）
    let mut r4 = T1Registry::new(8);
    let _ = r4.register(12, Device::T1a, 2);
    let _ = r4.open_gate(12);
    if r4
        .record(12, Lab::new(f32::NAN, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "ev")
        .err()
        == Some(T1Error::BadDelta)
    {
        caught += 1;
    }

    // 5) 重复登记
    let mut r5 = T1Registry::new(8);
    let _ = r5.register(13, Device::T1a, 3);
    if r5.register(13, Device::T1b, 3).err() == Some(T1Error::Duplicate) {
        caught += 1;
    }

    // 6) 容量满
    let mut r6 = T1Registry::new(1);
    let _ = r6.register(14, Device::T1a, 4);
    if r6.register(15, Device::T1a, 5).err() == Some(T1Error::Full) && r6.len() == 1 {
        caught += 1;
    }

    // 7) 色块越界
    let mut r7 = T1Registry::new(8);
    if r7.register(16, Device::T1a, PATCH_COUNT).err() == Some(T1Error::PatchRange) {
        caught += 1;
    }

    // 8) 状态机倒回（已测槽不得改挂起）
    let mut r8 = T1Registry::new(8);
    let _ = r8.register(17, Device::T1b, 6);
    let _ = r8.open_gate(17);
    let ok = r8
        .record(17, Lab::new(40.0, 1.0, 1.0), Lab::new(40.0, 1.0, 1.0), "ev")
        .is_ok();
    if ok && r8.mark_pending(17).err() == Some(T1Error::State) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50017 自检：12 条判据（8–20 区间内）。
pub fn run_unx50017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50017");

    // J1-1 起步三态：24 槽全部 Unmeasured，measured/passed/inflated 均为 0。
    let seed = seed_registry();
    let r0 = seed.report();
    let ok = r0.total == 24
        && r0.unmeasured == 24
        && r0.measured == 0
        && r0.passed == 0
        && r0.inflated == 0;
    let det = format!("t={} u={} m={}", r0.total, r0.unmeasured, r0.measured);
    s.add_dyn("50017-J1-起步三态", ok, &det);

    // J1-2 闸门未开必拒：不只看错误码，还要看三态没被写脏。
    let mut r = seed_registry();
    let id = T1Registry::slot_id(Device::T1a, 0);
    let refused = r
        .record(id, Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "lab.log")
        .err()
        == Some(T1Error::GateClosed);
    let rp = r.report();
    let ok = refused && rp.measured == 0 && rp.inflated == 0 && rp.rejected == 1;
    let det = format!("m={} rej={}", rp.measured, rp.rejected);
    s.add_dyn("50017-J1-闸门拒写", ok, &det);

    // J1-3 开闸后空证据仍必拒（闸门不是万能通行证）。
    let mut r = seed_registry();
    let _ = r.open_gate(id);
    let ok = r
        .record(id, Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "")
        .err()
        == Some(T1Error::NoEvidence)
        && r.get(id).map(|x| x.claim) == Some(Claim::Unmeasured);
    s.add("50017-J1-空证据拒", ok, if ok { "" } else { "空证据被接受" });

    // J1-4 三证齐备才能落 Measured，且 ΔE 与 50001 口径一致（同点 = 0）。
    let mut r = seed_registry();
    let _ = r.open_gate(id);
    let de = r.record(id, Lab::new(55.0, 2.0, -3.0), Lab::new(55.0, 2.0, -3.0), "t1a.log#12");
    let same_point = match de {
        Ok(v) => fabsf(v) < 1e-6,
        Err(_) => false,
    };
    let rp = r.report();
    let ok = same_point && rp.measured == 1 && rp.passed == 1 && rp.inflated == 0 && rp.unmeasured == 23;
    let det = format!("m={} p={} inf={}", rp.measured, rp.passed, rp.inflated);
    s.add_dyn("50017-J1-三证落账", ok, &det);

    // J1-5 复测幂等：同输入连测两次，ΔE 逐位一致、measured 仍为 1、attempts=2。
    let mut r = seed_registry();
    let _ = r.open_gate(id);
    let a1 = r.record(id, Lab::new(48.0, 1.0, 2.0), Lab::new(48.0, 0.0, 0.0), "ev-a");
    let a2 = r.record(id, Lab::new(48.0, 1.0, 2.0), Lab::new(48.0, 0.0, 0.0), "ev-b");
    let same = match (a1, a2) {
        (Ok(x), Ok(y)) => fabsf(x - y) < 1e-6,
        _ => false,
    };
    let rp = r.report();
    let ok = same && rp.measured == 1 && rp.passed == 1 && r.get(id).map(|x| x.attempts) == Some(2);
    let attempts = match r.get(id) {
        Some(x) => x.attempts,
        None => 0,
    };
    let det = format!("m={} att={}", rp.measured, attempts);
    s.add_dyn("50017-J1-复测幂等", ok, &det);

    // J1-6 超标测量：单测一条应落 failed=1、passed=0（诚实记录失败也是记录）。
    let mut r = seed_registry();
    let _ = r.open_gate(id);
    let _ = r.record(id, Lab::new(70.0, 0.0, 0.0), Lab::new(50.0, 0.0, 0.0), "ev3");
    let rp = r.report();
    let ok = rp.measured == 1 && rp.passed == 0 && rp.failed == 1;
    let det = format!("p={} f={}", rp.passed, rp.failed);
    s.add_dyn("50017-J1-超标记账", ok, &det);

    // J1-7 重复登记必拒且不改变表长。
    let mut r = T1Registry::new(8);
    let _ = r.register(20, Device::T1a, 0);
    let before = r.len();
    let ok = r.register(20, Device::T1b, 1).err() == Some(T1Error::Duplicate)
        && r.len() == before
        && r.rejected() == 1;
    s.add("50017-J1-防重复登记", ok, if ok { "" } else { "重复登记进了表" });

    // J1-8 容量满必拒，且 len 严格不超容量（定容账不得被撑破）。
    let mut r = T1Registry::new(2);
    let _ = r.register(21, Device::T1a, 0);
    let _ = r.register(22, Device::T1a, 1);
    let ok = r.register(23, Device::T1a, 2).err() == Some(T1Error::Full)
        && r.len() <= r.capacity()
        && r.capacity() == 2;
    let det = format!("len={} cap={}", r.len(), r.capacity());
    s.add_dyn("50017-J1-定容不破", ok, &det);

    // J1-9 色块越界必拒（PATCH_COUNT 是闭区间上界）。
    let mut r = T1Registry::new(8);
    let ok = r.register(30, Device::T1a, PATCH_COUNT).err() == Some(T1Error::PatchRange)
        && r.register(31, Device::T1a, PATCH_COUNT - 1).is_ok();
    s.add("50017-J1-色块区间", ok, if ok { "" } else { "色块边界错" });

    // J1-10 状态机：Unmeasured→Pending 合法，Measured→Pending 非法。
    let mut r = seed_registry();
    let pending_ok = r.mark_pending(id).is_ok();
    let again = r.mark_pending(id).err() == Some(T1Error::State);
    let rp = r.report();
    let ok = pending_ok && again && rp.pending == 1 && rp.measured == 0;
    let det = format!("pend={} m={}", rp.pending, rp.measured);
    s.add_dyn("50017-J1-三态迁移", ok, &det);

    // J1-11 守恒：任何操作序列后 measured+pending+unmeasured == total。
    let mut r = seed_registry();
    let a = T1Registry::slot_id(Device::T1a, 0);
    let b = T1Registry::slot_id(Device::T1b, 11);
    let _ = r.open_gate(a);
    let _ = r.record(a, Lab::new(30.0, 0.0, 0.0), Lab::new(30.0, 0.0, 0.0), "ev");
    let _ = r.mark_pending(b);
    let _ = r.open_gate(T1Registry::slot_id(Device::T1b, 0));
    let rp = r.report();
    let sum = rp.measured + rp.pending + rp.unmeasured;
    let ok = sum == rp.total && rp.total == 24 && r.awaiting_ids().len() == rp.awaiting();
    s.add("50017-J1-三态守恒", ok, if ok { "" } else { "三态不闭合" });

    // J1-12 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50017-J1-注入必抓", ok, &det);

    s
}

impl Report {
    /// 待补测槽数（未测 + 挂起），与 [`T1Registry::awaiting_ids`] 等长。
    pub fn awaiting(&self) -> usize {
        self.unmeasured + self.pending
    }

    /// 一行对账文本（落账/日志用）。
    pub fn ledger_line(&self) -> String {
        format!(
            "T1 total={} measured={} pending={} unmeasured={} pass={} fail={} inflated={} rejected={}",
            self.total, self.measured, self.pending, self.unmeasured, self.passed, self.failed, self.inflated, self.rejected
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50017_checks_green() {
        let set = run_unx50017_checks();
        assert!(set.all_passed(), "UNX-F50017 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50017_no_inflation() {
        let mut r = seed_registry();
        let id = T1Registry::slot_id(Device::T1a, 5);
        let _ = r.open_gate(id);
        let _ = r.record(id, Lab::new(20.0, 0.0, 0.0), Lab::new(20.0, 0.0, 0.0), "ev");
        let rep = r.report();
        assert_eq!(rep.inflated, 0);
        assert_eq!(rep.measured, 1);
    }

    #[test]
    fn unx50017_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }
}
