//! UNX-F21588 · 域收官宣示（此后零新条目入域账）
//!
//! 诊断段 0x91039~0x91040（F2-B40 域收官段 8/100）。
//!
//! 判据原文（施工书 UNX-F21588-J1）：宣示判据：F21600 后任何新增尝试被校验器拒绝（写锁生效）。
//!
//! 设计要点（为什么本单只有两个码却仍能定位到具体违规）：
//! 派工单给本单的码段只有两位（0x91039 成功 / 0x91040 拒绝），而拒绝成因有七种。
//! 把七种成因摊成七个码必然越界占用相邻单的码段，日志对账时会与别单撞键；
//! 只报一个裸 0x91040 又等于没报。折中是：**码回答「锁是否生效」，成因枚举
//! 回答「哪一道闸拦的」**，两者同装进 `Fault`。判据同时断言码与成因，定位
//! 粒度比七个平级码更细，且成因可以继续增长而码段不膨胀。
//!
//! 三态闸门 Open → Sealed → Frozen 是单向的：宣示不可撤销，冻结只在宣示之上
//! 再压一层。因此 `freeze` **不推进 epoch**——epoch 是「宣示」的单调序号
//! （代替墙钟），冻结不是一次新宣示，推进它会让下游误判域账重开过。
//!
//! 为什么新增 / 修改 / 删除三个入口共用一个守卫：写锁的价值在于「没有旁路」。
//! 三处各写一遍判断，早晚有一处漏判；统一走 `guard` 后，判据只需证明
//! 「三条路径返回同一个码」，而这条性质是结构保证而非逐条自觉。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 域账编号上界（施工书 UNX-F21600 终条）。
pub const ID_CEILING: u32 = 21_600;
/// 拒绝成因槽位数（`LockMeter` 分桶直方图定容长度，与枚举项数一致）。
pub const REASON_SLOTS: usize = 7;
/// 写入口种数（新增 / 修改 / 删除）。
pub const WRITE_OPS: usize = 3;

/// 收官诊断码（本单码段仅两位）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体会整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SolemnCode {
    /// 0x91039 宣示成立（写锁未触发）。
    SolemnOk = 0x91039,
    /// 0x91040 写入被校验器拒绝（成因见 `Fault::reason`）。
    WriteRefused = 0x91040,
}

impl SolemnCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            SolemnCode::SolemnOk => "域账写入被校验器放行",
            SolemnCode::WriteRefused => "域账写入被校验器拒绝",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            SolemnCode::SolemnOk => "闸门处于 Open 且条目满足编号约束",
            SolemnCode::WriteRefused => "闸门已封或条目越界，写锁生效",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SolemnCode::SolemnOk => "继续后续条目写入",
            SolemnCode::WriteRefused => "查看成因码，改走只读复核接口",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断成 0x1039）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 拒绝成因（七元闭集，定位粒度在此而不在码上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// 闸门非 Open，写入被锁拦下。
    LockHeld,
    /// 已冻结再冻结。
    FrozenAgain,
    /// 未宣示就冻结。
    NotSealed,
    /// 编号越过 F21600 上界。
    IdBeyondCeiling,
    /// 编号为 0 / 不递增 / 目标条目不存在。
    IdUnusable,
    /// 空账宣示。
    EmptyAccount,
    /// 末条不是域终条，宣示单不成立。
    TailMismatch,
}

impl Refusal {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            Refusal::LockHeld => "写锁生效，写入被拒",
            Refusal::FrozenAgain => "域账已冻结，重复冻结被拒",
            Refusal::NotSealed => "域账未宣示，冻结被拒",
            Refusal::IdBeyondCeiling => "条目编号越过 F21600 上界",
            Refusal::IdUnusable => "条目编号不可用于本次写入",
            Refusal::EmptyAccount => "空账不构成宣示",
            Refusal::TailMismatch => "末条非 F21600，宣示单不成立",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            Refusal::LockHeld => "宣示后闸门单向关闭，不再接受任何写入口",
            Refusal::FrozenAgain => "冻结是终态，不存在二次冻结的语义",
            Refusal::NotSealed => "冻结的对象必须是已宣示的域账",
            Refusal::IdBeyondCeiling => "收官域只到 F21600，越界条目属于下一域",
            Refusal::IdUnusable => "编号必须从 1 起严格递增，且目标条目须在账内",
            Refusal::EmptyAccount => "宣示需要账面上确有条目可封",
            Refusal::TailMismatch => "宣示单的末条必须与域终条逐字相符",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            Refusal::LockHeld => "改走只读复核，或另开新域账",
            Refusal::FrozenAgain => "冻结无需重复执行，直接读回执",
            Refusal::NotSealed => "先完成宣示再冻结",
            Refusal::IdBeyondCeiling => "核对编号分配，F21600 之后的条目单独立账",
            Refusal::IdUnusable => "重排编号：从 1 起连续、不重号、不倒退",
            Refusal::EmptyAccount => "先写入条目再宣示",
            Refusal::TailMismatch => "补齐至 F21600 或修正被截断的末条",
        }
    }
    /// 直方图槽位（与 `REASON_SLOTS` 对齐）。
    pub const fn slot(self) -> usize {
        self as usize
    }
    /// 是否属于「写入口」被拒（区别于宣示/冻结等闸门操作被拒）。
    pub const fn is_write_refusal(self) -> bool {
        matches!(self, Refusal::LockHeld | Refusal::IdBeyondCeiling | Refusal::IdUnusable)
    }
}

/// 拒绝记录（码 + 成因，缺一不可）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 成因。
    pub reason: Refusal,
}

impl Fault {
    const fn refused(reason: Refusal) -> Fault {
        Fault {
            code: SolemnCode::WriteRefused.code(),
            reason,
        }
    }
    /// 码是否确为拒绝码。
    pub const fn is_refusal(self) -> bool {
        self.code == SolemnCode::WriteRefused as u32
    }
    /// 三要素拼成一行（运维面板直读，不经格式化器）。
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(96);
        out.push_str("refused/");
        out.push_str(self.reason.what());
        out.push_str(" | ");
        out.push_str(self.reason.why());
        out.push_str(" | ");
        out.push_str(self.reason.advice());
        out
    }
}/// 写入口种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOp {
    /// 新增条目。
    Add = 0,
    /// 修改条目。
    Amend = 1,
    /// 删除条目。
    Retract = 2,
}

impl WriteOp {
    /// 名称（报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            WriteOp::Add => "add",
            WriteOp::Amend => "amend",
            WriteOp::Retract => "retract",
        }
    }
    /// 计数器槽位。
    pub const fn slot(self) -> usize {
        self as usize
    }
}

/// 闸门三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 可写。
    Open,
    /// 已宣示，写锁生效。
    Sealed,
    /// 已冻结，写锁 + 终态。
    Frozen,
}

impl GateState {
    /// 名称（报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            GateState::Open => "open",
            GateState::Sealed => "sealed",
            GateState::Frozen => "frozen",
        }
    }
    /// 是否接受写入。
    ///
    /// 只有 Open 可写：Sealed 与 Frozen 都必须拦，且拦下来的码必须相同——
    /// 否则下游会误以为「冻结比宣示更宽松」而去试探。
    pub const fn is_writable(self) -> bool {
        matches!(self, GateState::Open)
    }
}

/// 锁计数表（拒绝次数可观测的载体）。
///
/// 三个计数面分开记：`attempts` 只数写入口尝试（用于「尝试 = 成功 + 被拒」
/// 的守恒式），`refusals` 数所有闸门拒绝（含宣示/冻结被拒），`by_reason` /
/// `by_op` 用于把总数拆回成因与入口。合成一个计数器就再也分不清「谁在撞锁」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockMeter {
    attempts: u32,
    refusals: u32,
    by_reason: [u32; REASON_SLOTS],
    by_op: [u32; WRITE_OPS],
}

impl LockMeter {
    /// 全新计数表。
    pub const fn new() -> LockMeter {
        LockMeter {
            attempts: 0,
            refusals: 0,
            by_reason: [0; REASON_SLOTS],
            by_op: [0; WRITE_OPS],
        }
    }
    /// 写入口尝试总数。
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }
    /// 闸门拒绝总数（写入口 + 宣示/冻结）。
    pub const fn refusals(&self) -> u32 {
        self.refusals
    }
    /// 某一成因的拒绝次数（越界槽位返回 0，不 panic）。
    pub fn refusals_of(&self, reason: Refusal) -> u32 {
        match self.by_reason.get(reason.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 某一入口的拒绝次数。
    pub fn op_hits(&self, op: WriteOp) -> u32 {
        match self.by_op.get(op.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 记一次写尝试（成功与失败都算尝试，拒绝另在 `refuse` 里落桶）。
    fn attempt(&mut self, _op: WriteOp) {
        self.attempts = self.attempts.saturating_add(1);
    }
    /// 记一次拒绝（`op` 为 None 表示不是写入口触发的）。
    fn refuse(&mut self, reason: Refusal, op: Option<WriteOp>) {
        self.refusals = self.refusals.saturating_add(1);
        if let Some(v) = self.by_reason.get_mut(reason.slot()) {
            *v = v.saturating_add(1);
        }
        if let Some(op) = op {
            if let Some(v) = self.by_op.get_mut(op.slot()) {
                *v = v.saturating_add(1);
            }
        }
    }
    /// 写入口被拒总数（守恒式右端）。
    pub fn write_refusals(&self) -> u32 {
        // 先按成因求和，再扣掉非写入口的拒绝（宣示/冻结），得到纯写入口拒绝数。
        let sum = self.by_reason.iter().fold(0u32, |acc, v| acc.saturating_add(*v));
        let gate = self
            .refusals_of(Refusal::FrozenAgain)
            .saturating_add(self.refusals_of(Refusal::NotSealed))
            .saturating_add(self.refusals_of(Refusal::EmptyAccount))
            .saturating_add(self.refusals_of(Refusal::TailMismatch));
        sum.saturating_sub(gate)
    }
}

/// 域账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 条目编号（1..=21600，严格递增）。
    pub id: u32,
    /// 条目内容指纹（二级指纹，写入后仍可改，用于测试「可写期可改」）。
    pub tag: u32,
}

impl Entry {
    /// 构造条目。
    pub const fn new(id: u32, tag: u32) -> Entry {
        Entry { id, tag }
    }
}

/// 宣示单（外部复核凭据）。
///
/// `epoch` 是单调序号而非墙钟：内核无墙钟且判据要可重放，时间戳一旦入
/// 回执就再也无法逐位比对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SolemnReceipt {
    /// 宣示序号（从 1 起，单调）。
    pub epoch: u16,
    /// 批号。
    pub batch: u16,
    /// 封印条目数。
    pub entries: u32,
    /// 末条编号。
    pub tail: u32,
    /// 账链指纹（FNV-1a 64）。
    pub fingerprint: u64,
}

impl SolemnReceipt {
    /// 回执自洽（五项交叉）。
    pub const fn is_complete(&self) -> bool {
        self.epoch > 0 && self.batch > 0 && self.entries > 0
            && self.tail == ID_CEILING && self.fingerprint != 0
    }
}

/// 域账本（三态闸门 + 单一守卫）。
pub struct DomainAccount {
    entries: Vec<Entry>,
    gate: GateState,
    meter: LockMeter,
    epoch: u16,
    seal_rounds: u16,
    receipt: Option<SolemnReceipt>,
}

impl DomainAccount {
    /// 新建空账（Open）。
    pub const fn new() -> DomainAccount {
        DomainAccount {
            entries: Vec::new(),
            gate: GateState::Open,
            meter: LockMeter::new(),
            epoch: 0,
            seal_rounds: 0,
            receipt: None,
        }
    }

    /// 当前闸门。
    pub const fn gate(&self) -> GateState {
        self.gate
    }
    /// 宣示序号。
    pub const fn epoch(&self) -> u16 {
        self.epoch
    }
    /// 成功宣示次数（幂等观测点：宣示成功后恒为 1）。
    pub const fn seal_rounds(&self) -> u16 {
        self.seal_rounds
    }
    /// 当前回执。
    pub const fn receipt(&self) -> Option<SolemnReceipt> {
        self.receipt
    }
    /// 计数表快照（`LockMeter` 是 Copy，取快照不影响账本）。
    pub const fn meter(&self) -> LockMeter {
        self.meter
    }
    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    /// 末条编号（空账返回 0）。
    pub fn last_id(&self) -> u32 {
        match self.entries.last() {
            Some(e) => e.id,
            None => 0,
        }
    }

    /// 统一守卫：三条写入口的唯一闸口。
    ///
    /// 顺序上先记账再判锁：被拒的尝试也必须留下计数，否则「被拒次数」恒为 0，
    /// 写锁就变成不可观测的静默丢弃。
    fn guard(&mut self, op: WriteOp) -> Result<(), Fault> {
        self.meter.attempt(op);
        if !self.gate.is_writable() {
            self.meter.refuse(Refusal::LockHeld, Some(op));
            return Err(Fault::refused(Refusal::LockHeld));
        }
        Ok(())
    }

    /// 新增条目。
    ///
    /// 编号三道闸：非零 → 不越界 → 严格递增。顺序固定：越界是「永不可用」，
    /// 不递增是「此刻不可用」，前者更根本，先报。
    pub fn append(&mut self, entry: Entry) -> Result<(), Fault> {
        self.guard(WriteOp::Add)?;
        if entry.id > ID_CEILING {
            self.meter.refuse(Refusal::IdBeyondCeiling, Some(WriteOp::Add));
            return Err(Fault::refused(Refusal::IdBeyondCeiling));
        }
        if entry.id == 0 || entry.id <= self.last_id() {
            self.meter.refuse(Refusal::IdUnusable, Some(WriteOp::Add));
            return Err(Fault::refused(Refusal::IdUnusable));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 修改条目内容（编号不可改，只改 tag）。
    pub fn amend(&mut self, id: u32, tag: u32) -> Result<(), Fault> {
        self.guard(WriteOp::Amend)?;
        let mut hit = false;
        for slot in self.entries.iter_mut() {
            if slot.id == id {
                slot.tag = tag;
                hit = true;
                break;
            }
        }
        if !hit {
            self.meter.refuse(Refusal::IdUnusable, Some(WriteOp::Amend));
            return Err(Fault::refused(Refusal::IdUnusable));
        }
        Ok(())
    }

    /// 删除条目。
    pub fn retract(&mut self, id: u32) -> Result<(), Fault> {
        self.guard(WriteOp::Retract)?;
        let pos = self.entries.iter().position(|e| e.id == id);
        match pos {
            Some(p) => {
                self.entries.remove(p);
                Ok(())
            }
            None => {
                self.meter.refuse(Refusal::IdUnusable, Some(WriteOp::Retract));
                Err(Fault::refused(Refusal::IdUnusable))
            }
        }
    }

    /// 账链指纹：逐条把 (id, tag) 喂进 FNV-1a 64。
    ///
    /// 逐字段喂而非喂结构体内存：结构体内存含 padding，未初始化字节会让指纹
    /// 随编译器版本漂移，跨版本复核就永远对不上。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for e in self.entries.iter() {
            h = fnv_u64(h, e.id as u64);
            h = fnv_u64(h, e.tag as u64);
        }
        h
    }

    /// 宣示（域收官）。
    ///
    /// 幂等：已宣示（含已冻结）时**原样返回同一回执**且不推进 epoch、不增
    /// `seal_rounds`。只读性来自「回执一旦发出即不可变」——否则下游拿到的
    /// 两份回执会描述同一本账的不同版本，复核无从对账。
    pub fn solemnize(&mut self, batch: u16) -> Result<SolemnReceipt, Fault> {
        if let Some(r) = self.receipt {
            return Ok(r);
        }
        if self.entries.is_empty() {
            self.meter.refuse(Refusal::EmptyAccount, None);
            return Err(Fault::refused(Refusal::EmptyAccount));
        }
        let tail = self.last_id();
        if tail != ID_CEILING {
            self.meter.refuse(Refusal::TailMismatch, None);
            return Err(Fault::refused(Refusal::TailMismatch));
        }
        let receipt = SolemnReceipt {
            epoch: self.epoch.saturating_add(1),
            batch,
            entries: self.entries.len() as u32,
            tail,
            fingerprint: self.fingerprint(),
        };
        self.gate = GateState::Sealed;
        self.epoch = receipt.epoch;
        self.seal_rounds = self.seal_rounds.saturating_add(1);
        self.receipt = Some(receipt);
        Ok(receipt)
    }

    /// 冻结（终态）。
    ///
    /// 冻结不改回执、不推进 epoch：它只是把「不可写」从可逆（理论上可复核
    /// 重开）变成不可逆，宣示内容本身没有变化。
    pub fn freeze(&mut self) -> Result<SolemnReceipt, Fault> {
        match self.gate {
            GateState::Open => {
                self.meter.refuse(Refusal::NotSealed, None);
                Err(Fault::refused(Refusal::NotSealed))
            }
            GateState::Frozen => {
                self.meter.refuse(Refusal::FrozenAgain, None);
                Err(Fault::refused(Refusal::FrozenAgain))
            }
            GateState::Sealed => match self.receipt {
                Some(r) => {
                    self.gate = GateState::Frozen;
                    Ok(r)
                }
                // Sealed 而回执缺失在结构上不可达；仍显性上报而非原谅。
                None => {
                    self.meter.refuse(Refusal::NotSealed, None);
                    Err(Fault::refused(Refusal::NotSealed))
                }
            },
        }
    }

    /// 宣示文本（可归档，字段顺序固定便于 diff）。
    pub fn declaration(&self) -> String {
        let mut out = String::with_capacity(120);
        out.push_str("unx-f21588 gate=");
        out.push_str(self.gate.label());
        out.push_str(" epoch=");
        out.push_str(&self.epoch.to_string());
        out.push_str(" rounds=");
        out.push_str(&self.seal_rounds.to_string());
        out.push_str(" entries=");
        out.push_str(&self.entries.len().to_string());
        out.push_str(" tail=");
        out.push_str(&self.last_id().to_string());
        out.push_str(" refusals=");
        out.push_str(&self.meter.refusals().to_string());
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint()));
        out
    }
}

/// FNV-1a 64 单步（质数与偏移基写死，来源：FNV 规范 1a 变体）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，免 `format!` 的格式化开销）。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    for shift in (0..64u32).step_by(4).rev() {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
    }
    s
}

/// 装条目至 `upto`（含）。
///
/// 走真实 `append` 而非直接 push 内部 Vec：夹具若绕过守卫，测出来的「写锁
/// 生效」只是在测一个从未被走到的分支。
pub fn filled_account(upto: u32) -> Result<DomainAccount, Fault> {
    let mut acc = DomainAccount::new();
    let mut id = 1u32;
    while id <= upto {
        // tag 用乘法散列拉开差异，使「改 tag 指纹必变」这条判据有意义。
        acc.append(Entry::new(id, id.wrapping_mul(2_654_435_761)))?;
        id += 1;
    }
    Ok(acc)
}

/// 标准满账（1..=21600）。
pub fn standard_account() -> Result<DomainAccount, Fault> {
    filled_account(ID_CEILING)
}

/// 判据入口（UNX-F21588-J1 及其配套十四条）。
///
/// 判据常量独立写死：`21_600` / `21_601` / `0x91040` 在断言里直接以字面量
/// 出现，避免「被测值自己证自己」。
pub fn run_unx21588_closesolemn_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21588");
    let fixture = standard_account();

    // 1) 契约与夹具先行：码值 / 三要素 / 拒绝文本 / 满账可构造。
    cs.add(
        "f21588.contract_and_fixture",
        fixture.is_ok()
            && SolemnCode::SolemnOk.code() == 0x91039
            && SolemnCode::WriteRefused.code() == 0x91040
            && !SolemnCode::SolemnOk.advice().is_empty()
            && !SolemnCode::WriteRefused.advice().is_empty()
            && Fault::refused(Refusal::LockHeld).text().contains("写锁"),
        "满账构造失败，或码值/三要素/拒绝文本不达标",
    );

    // 2) 宣示成立：回执五项交叉 + 闸门落 Sealed。
    match standard_account() {
        Ok(mut acc) => {
            let ok = match acc.solemnize(40) {
                Ok(r) => {
                    r.epoch == 1
                        && r.batch == 40
                        && r.entries == 21_600
                        && r.tail == 21_600
                        && r.fingerprint != 0
                        && r.is_complete()
                        && acc.gate() == GateState::Sealed
                        && acc.seal_rounds() == 1
                }
                Err(_) => false,
            };
            cs.add("f21588.solemn_ok", ok, "回执不自洽或闸门未落 Sealed");
        }
        Err(_) => cs.add("f21588.solemn_ok", false, "夹具不可用"),
    }

    // 3) 重复宣示幂等：回执逐位相同、epoch 与轮数都不变。
    if let Ok(mut acc) = standard_account() {
        let same = match (acc.solemnize(40), acc.solemnize(40)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        let ok = same && acc.epoch() == 1 && acc.seal_rounds() == 1;
        cs.add("f21588.reseal_idempotent", ok, "重复宣示未返回同一回执或 epoch/轮数被推进");
    } else {
        cs.add("f21588.reseal_idempotent", false, "夹具不可用");
    }

    // 4) 宣示后新增必拒（码 + 拒绝标志 + 账未被写脏）。
    if let Ok(mut acc) = standard_account() {
        let _ = acc.solemnize(40);
        let err = acc.append(Entry::new(21_601, 7));
        let code_ok = err.as_ref().err().map(|f| f.code) == Some(0x91040);
        let flag_ok = err.as_ref().err().map(|f| f.is_refusal()) == Some(true);
        let ok = code_ok && flag_ok && acc.entry_count() == 21_600;
        cs.add("f21588.add_locked", ok, "宣示后新增未被拒、码不符或账被写脏");
    } else {
        cs.add("f21588.add_locked", false, "夹具不可用");
    }

    // 5) 三条写入口返回同一拒绝码，且计数按入口分桶各记一次。
    if let Ok(mut acc) = standard_account() {
        let _ = acc.solemnize(40);
        let a = acc.append(Entry::new(21_601, 1)).err().map(|f| f.code);
        let b = acc.amend(1, 9).err().map(|f| f.code);
        let c = acc.retract(1).err().map(|f| f.code);
        let m = acc.meter();
        let uniform = a == Some(0x91040) && b == Some(0x91040) && c == Some(0x91040);
        let bucketed = m.refusals() == 3
            && m.op_hits(WriteOp::Add) == 1
            && m.op_hits(WriteOp::Amend) == 1
            && m.op_hits(WriteOp::Retract) == 1
            && m.refusals_of(Refusal::LockHeld) == 3;
        let labels = WriteOp::Add.label() == "add" && WriteOp::Amend.label() == "amend";
        let ok = uniform
            && bucketed
            && labels
            && WriteOp::Retract.label() == "retract"
            && Refusal::LockHeld.is_write_refusal();
        cs.add("f21588.all_writers_same_code", ok, "三入口码不统一或计数未按入口分桶");
    } else {
        cs.add("f21588.all_writers_same_code", false, "夹具不可用");
    }

    // 6) 上界边界：21600 收、21601 拒（严格贴边各验一次）。
    if let Ok(mut acc) = filled_account(21_599) {
        let ok = acc.append(Entry::new(21_600, 5)).is_ok();
        let bad = acc.append(Entry::new(21_601, 6)).err().map(|f| f.reason);
        cs.add("f21588.ceiling_edge", ok && bad == Some(Refusal::IdBeyondCeiling), "21600 应可写、21601 应因越界被拒");
    } else {
        cs.add("f21588.ceiling_edge", false, "半满夹具不可用");
    }

    // 7) 编号 0 边界（空账上也必须拒）。
    {
        let mut acc = DomainAccount::new();
        let bad = acc.append(Entry::new(0, 1)).err().map(|f| f.reason);
        cs.add("f21588.id_zero_refused", bad == Some(Refusal::IdUnusable), "编号 0 未被判为不可用");
    }

    // 8) 重号与倒退（两个反向样本，同一个成因）。
    if let Ok(mut acc) = filled_account(100) {
        let dup = acc.append(Entry::new(100, 1)).err().map(|f| f.reason);
        let back = acc.append(Entry::new(50, 1)).err().map(|f| f.reason);
        let bad = dup == Some(Refusal::IdUnusable) && back == Some(Refusal::IdUnusable);
        cs.add("f21588.id_order_refused", bad, "重号或倒退未被拒");
    } else {
        cs.add("f21588.id_order_refused", false, "夹具不可用");
    }

    // 9) 末条不是 21600 的账不得宣示。
    if let Ok(mut acc) = filled_account(21_599) {
        let bad = acc.solemnize(40).err().map(|f| f.reason);
        cs.add("f21588.tail_mismatch", bad == Some(Refusal::TailMismatch), "末条 21599 的账宣示未被拒");
    } else {
        cs.add("f21588.tail_mismatch", false, "半满夹具不可用");
    }

    // 10) 空账不得宣示（空集合边界）。
    {
        let mut acc = DomainAccount::new();
        let bad = acc.solemnize(1).err().map(|f| f.reason);
        cs.add("f21588.empty_account", bad == Some(Refusal::EmptyAccount), "空账宣示未被拒");
    }

    // 11) 冻结：闸门落 Frozen、回执与 epoch 不被改写。
    if let Ok(mut acc) = standard_account() {
        let preserved = match (acc.solemnize(40), acc.freeze()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        let after = acc.append(Entry::new(21_601, 2)).err().map(|f| f.code);
        let ok = preserved && acc.gate() == GateState::Frozen && acc.epoch() == 1;
        cs.add("f21588.freeze_after_seal", ok && after == Some(0x91040), "冻结改写了回执/epoch，或冻结后仍可写");
    } else {
        cs.add("f21588.freeze_after_seal", false, "夹具不可用");
    }

    // 12) 二次冻结必拒，且拒绝计数只加一次。
    if let Ok(mut acc) = standard_account() {
        let _ = acc.solemnize(40);
        let _ = acc.freeze();
        let before = acc.meter().refusals();
        let second = acc.freeze();
        let ok = second.err().map(|f| f.reason) == Some(Refusal::FrozenAgain);
        cs.add("f21588.refreeze_refused", ok && acc.meter().refusals() == before + 1, "二次冻结未被拒或计数不符");
    } else {
        cs.add("f21588.refreeze_refused", false, "夹具不可用");
    }

    // 13) 未宣示就冻结必拒。
    {
        let mut acc = DomainAccount::new();
        let bad = acc.freeze().err().map(|f| f.reason);
        cs.add("f21588.freeze_before_seal", bad == Some(Refusal::NotSealed), "Open 态冻结未被拒");
    }

    // 14) 守恒式：本段写尝试数 == 成功写入数 + 写入口被拒数。
    //
    // 取差值而不是绝对值：夹具装配本身也走写入口，绝对值里混着 200 次
    // 装配尝试，判据就会被夹具规模绑架（改夹具即红，属于判据写错）。
    if let Ok(mut acc) = filled_account(200) {
        let base = acc.meter().attempts();
        let _ = acc.append(Entry::new(201, 1));
        let _ = acc.append(Entry::new(202, 2));
        let _ = acc.amend(1, 5);
        let _ = acc.retract(2);
        let _ = acc.append(Entry::new(0, 0));
        let _ = acc.append(Entry::new(203, 3));
        let m = acc.meter();
        let ok = m.attempts().saturating_sub(base) == 6
            && acc.entry_count() == 202
            && m.write_refusals() == 1
            && m.refusals_of(Refusal::IdUnusable) == 1;
        cs.add("f21588.meter_conserved", ok, "尝试/成功/被拒三者不守恒");
    } else {
        cs.add("f21588.meter_conserved", false, "夹具不可用");
    }

    // 15) 确定性：两次独立构造的指纹与宣示文本逐位相同；改 tag 后指纹必变。
    match (standard_account(), standard_account()) {
        (Ok(a), Ok(b)) => {
            let (fa, fb) = (a.fingerprint(), b.fingerprint());
            let tampered = match filled_account(21_600) {
                Ok(mut t) => {
                    let _ = t.amend(21_600, 99);
                    t.fingerprint()
                }
                Err(_) => 0,
            };
            let ok = fa != 0 && fa == fb && a.declaration() == b.declaration();
            cs.add("f21588.deterministic", ok && tampered != fa, "两次构造不一致或篡改后指纹未变");
        }
        _ => cs.add("f21588.deterministic", false, "夹具不可用"),
    }

    cs
}
