//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18427 · 系统默认值层（锚基线解耦插槽） · 0x7E680 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18427-J1 基线插槽空载可运行断言全过（机制与基线解耦），
//!       ADR-UNX-008 填充接口联签（AI-93 备案）
//! 本模块结构清单：
//!   1. 冻结常量层：插槽布局（6 维槽 + 2 备用槽）/ADR-UNX-008 联签
//!      授权码/AI-93 备案号（推导注释写死）；
//!   2. 诊断码闭集 DlCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 插槽模型 DefSlot：状态机 Empty→Filled→Sealed 单向 + 值域
//!      闸 + 联签闸；
//!   4. 默认值层 DefaultLayer：八槽账本 + 空载运行承诺（全空槽时
//!      查询/审计/守恒照常走，不 panic 不误答）；
//!   5. ADR-UNX-008 填充接口 fill_slot：联签（授权码全等）+
//!      槽号闭集 + 值域闸 + 状态闸四闸依次；
//!   6. 解耦断言器：同一组机制操作（查/填/封/审）在空载态与满载态
//!      各跑一遍，两态机账行为逐项一致（值不同、账行为同）；
//!   7. 注入核：封槽改写/无联签填充/槽号越界/值域污染 四类逐类
//!      10/10 断言被显式拒绝；
//!   8. run_unx18427_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E680，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E680;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 插槽总数（写死 8：6 维槽对齐 unx18426 六维枚举 + 2 备用槽
/// 预留 ADR-UNX-008 后续填充面；2 的幂便于位面记账）。
pub const SLOT_COUNT: usize = 8;
/// 业务维槽数（写死 6：0..5 与六维一一对应，6/7 为备用槽）。
pub const DIM_SLOTS: usize = 6;
/// ADR-UNX-008 联签授权码（写死 0x0080_0093：ADR-UNX-008 编号
/// 折叠 + AI-93 备案号 93 恒联——联签语义：授权码逐字节全等才放行）。
pub const ADR008_TOKEN: u32 = 0x0080_0093;
/// 槽值域闭集（各槽共用：0..=0xFFFF，与四段版本打包口径一致）。
pub const SLOT_VALUE_MAX: u32 = 0xFFFF;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 空载承诺哨兵值（写死 0：空槽查询返回值恒为哨兵且带 NotBound 标）。
pub const UNBOUND_SENTINEL: u32 = 0;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlCode {
    /// 槽号越界：落 0..7 闭集外
    SlotOutOfRange,
    /// 无联签：授权码不符（非 ADR-UNX-008 联签）
    TokenMismatch,
    /// 值域污染：槽值超 0xFFFF 闭集
    ValueOutOfRange,
    /// 封槽改写：Sealed 槽拒绝再填
    SealedReject,
    /// 空槽回滚：Empty 槽无值可回滚
    UnfillEmpty,
    /// 封槽回滚：Sealed 槽不可回滚（封印单向）
    UnfillSealed,
    /// 空槽答值：空载态下试图取得业务值（应得 NotBound）
    UnboundAnswer,
    /// 空槽封印：Empty 槽不许直接封（先填后封）
    SealEmpty,
    /// 重复填充：Filled 槽未回滚即再填
    RefillRejected,
    /// 解耦破坏：空载/满载两态机账行为不一致
    DecouplingBroken,
    /// 层账守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl DlCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DlCode::SlotOutOfRange => "槽号越界：须落 0..7 闭集",
            DlCode::TokenMismatch => "无联签：授权码不符 ADR-UNX-008 联签",
            DlCode::ValueOutOfRange => "值域污染：槽值超 0xFFFF 闭集",
            DlCode::SealedReject => "封槽改写：Sealed 槽拒绝再填",
            DlCode::UnfillEmpty => "空槽回滚：Empty 槽无值可回滚",
            DlCode::UnfillSealed => "封槽回滚：封印单向不可退",
            DlCode::UnboundAnswer => "空槽答值：空载态须答 NotBound 非业务值",
            DlCode::SealEmpty => "空槽封印：须先填后封",
            DlCode::RefillRejected => "重复填充：Filled 槽须先回滚再填",
            DlCode::DecouplingBroken => "解耦破坏：两态机账行为不一致",
            DlCode::ConservationBroken => "层账守恒破坏",
            DlCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [DlCode; 12] = [
        DlCode::SlotOutOfRange,
        DlCode::TokenMismatch,
        DlCode::ValueOutOfRange,
        DlCode::SealedReject,
        DlCode::UnfillEmpty,
        DlCode::UnfillSealed,
        DlCode::UnboundAnswer,
        DlCode::SealEmpty,
        DlCode::RefillRejected,
        DlCode::DecouplingBroken,
        DlCode::ConservationBroken,
        DlCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < DlCode::ALL.len() {
        if let Some(c) = DlCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E680 段闭集内"
}

// ── 插槽模型 ───────────────────────────────────────────────────────
/// 插槽状态机：Empty→Filled→Sealed。单向，倒退拒（回滚仅 Filled 可走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotPhase {
    Empty,
    Filled,
    Sealed,
}

/// 单插槽。
#[derive(Clone, Copy, Debug)]
pub struct DefSlot {
    /// 状态机当前相。
    pub phase: SlotPhase,
    /// 槽值（Filled/Sealed 相有效；Empty 相恒为哨兵 0）。
    pub value: u32,
    /// 填充联签号（Empty 相为 0；Filled/Sealed 记 ADR008_TOKEN）。
    pub token: u32,
    /// 本槽机制操作计数（填/滚/封各计入，解耦对账用）。
    pub ops: u32,
}

impl DefSlot {
    /// 新空槽。
    pub const fn empty() -> DefSlot {
        DefSlot { phase: SlotPhase::Empty, value: UNBOUND_SENTINEL, token: 0, ops: 0 }
    }
    /// 槽是否可答业务值（Filled/Sealed 且有联签）。
    pub fn bound(&self) -> bool {
        self.phase != SlotPhase::Empty && self.token == ADR008_TOKEN
    }
}

// ── 默认值层 ───────────────────────────────────────────────────────
/// 八槽默认值层（锚基线解耦：层机制不依赖槽内有值）。
pub struct DefaultLayer {
    slots: [DefSlot; SLOT_COUNT],
    /// 层审计：尝试/成功/拒绝（守恒不变量）。
    pub tried: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 拒因账。
    pub reject_codes: Vec<u32>,
}

impl DefaultLayer {
    /// 新层：八槽全空（空载态——机制照常可运行）。
    pub fn new() -> DefaultLayer {
        DefaultLayer {
            slots: [
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
                DefSlot::empty(),
            ],
            tried: 0,
            accepted: 0,
            rejected: 0,
            reject_codes: Vec::new(),
        }
    }

    /// 槽只读探视（槽号越界返回 None，不 panic）。
    pub fn slot(&self, slot_id: u8) -> Option<DefSlot> {
        let idx = slot_id as usize;
        if idx >= SLOT_COUNT {
            return None;
        }
        self.slots.get(idx).copied()
    }

    /// 空载查询：Empty 槽返回 Err(UnboundAnswer)（不答业务值、
    /// 不 panic、不误答哨兵为真值）——"空载可运行"的查询侧语义。
    pub fn query(&self, slot_id: u8) -> Result<u32, u32> {
        let s = match self.slot(slot_id) {
            Some(s) => s,
            None => return Err(DlCode::SlotOutOfRange.code()),
        };
        if s.bound() {
            Ok(s.value)
        } else {
            Err(DlCode::UnboundAnswer.code())
        }
    }

    /// ADR-UNX-008 填充接口：联签→槽号→值域→状态 四闸依次。
    /// 联签闸最先（无联签不得触碰槽账）。
    pub fn fill_slot(&mut self, slot_id: u8, value: u32, token: u32) -> Result<(), u32> {
        self.tried = self.tried.saturating_add(1);
        // 闸1 联签。
        if token != ADR008_TOKEN {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(DlCode::TokenMismatch.code());
            return Err(DlCode::TokenMismatch.code());
        }
        // 闸2 槽号。
        let idx = slot_id as usize;
        if idx >= SLOT_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(DlCode::SlotOutOfRange.code());
            return Err(DlCode::SlotOutOfRange.code());
        }
        // 闸3 值域。
        if value > SLOT_VALUE_MAX {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(DlCode::ValueOutOfRange.code());
            return Err(DlCode::ValueOutOfRange.code());
        }
        // 闸4 状态：Empty 可填，Filled 拒重填，Sealed 拒改写。
        let s = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(DlCode::SlotOutOfRange.code()),
        };
        s.ops = s.ops.saturating_add(1);
        match s.phase {
            SlotPhase::Sealed => {
                self.rejected = self.rejected.saturating_add(1);
                self.reject_codes.push(DlCode::SealedReject.code());
                Err(DlCode::SealedReject.code())
            }
            SlotPhase::Filled => {
                self.rejected = self.rejected.saturating_add(1);
                self.reject_codes.push(DlCode::RefillRejected.code());
                Err(DlCode::RefillRejected.code())
            }
            SlotPhase::Empty => {
                s.phase = SlotPhase::Filled;
                s.value = value;
                s.token = token;
                self.accepted = self.accepted.saturating_add(1);
                Ok(())
            }
        }
    }

    /// 回滚：Filled→Empty（Sealed 不可退，Empty 无可退）。
    pub fn unfill_slot(&mut self, slot_id: u8) -> Result<(), u32> {
        let idx = slot_id as usize;
        if idx >= SLOT_COUNT {
            return Err(DlCode::SlotOutOfRange.code());
        }
        let s = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(DlCode::SlotOutOfRange.code()),
        };
        s.ops = s.ops.saturating_add(1);
        match s.phase {
            SlotPhase::Empty => Err(DlCode::UnfillEmpty.code()),
            SlotPhase::Sealed => Err(DlCode::UnfillSealed.code()),
            SlotPhase::Filled => {
                s.phase = SlotPhase::Empty;
                s.value = UNBOUND_SENTINEL;
                s.token = 0;
                Ok(())
            }
        }
    }

    /// 封印：Filled→Sealed（Empty 拒封；Sealed 重复封拒）。
    pub fn seal_slot(&mut self, slot_id: u8) -> Result<(), u32> {
        let idx = slot_id as usize;
        if idx >= SLOT_COUNT {
            return Err(DlCode::SlotOutOfRange.code());
        }
        let s = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(DlCode::SlotOutOfRange.code()),
        };
        s.ops = s.ops.saturating_add(1);
        match s.phase {
            SlotPhase::Empty => Err(DlCode::SealEmpty.code()),
            SlotPhase::Filled => {
                s.phase = SlotPhase::Sealed;
                Ok(())
            }
            SlotPhase::Sealed => Err(DlCode::SealedReject.code()),
        }
    }

    /// 已绑槽数。
    pub fn bound_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < SLOT_COUNT {
            if let Some(s) = self.slots.get(i) {
                if s.bound() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 层账守恒。
    pub fn conserve_ok(&self) -> bool {
        self.tried == self.accepted.saturating_add(self.rejected)
    }

    /// 拒因留账核对：账上是否存在某码。
    pub fn rejected_with(&self, code: u32) -> bool {
        let mut i = 0;
        while i < self.reject_codes.len() {
            if let Some(c) = self.reject_codes.get(i) {
                if *c == code {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

// ── 解耦断言器 ─────────────────────────────────────────────────────
/// 机制操作签名（空载/满载两态各跑同一序列，全部作用在备用槽 7——
/// 两态下槽 7 恒为 Empty，保证序列同构可比）：
///   步1 查越界槽 9（槽号闸）/ 步2 联签填 7 / 步3 回滚 7 /
///   步4 空槽封印 7（状态闸）/ 步5 无联签填 7（联签闸）。
/// 解耦语义：两态的"结果码序列"逐项一致（机制与基线无关）。
fn mech_signature(layer: &mut DefaultLayer) -> [u32; 5] {
    // 步1：查越界槽 9。
    let s0 = match layer.query(9) {
        Ok(_) => 0,
        Err(c) => c,
    };
    // 步2：联签填备用槽 7。
    let s1 = match layer.fill_slot(7, 100, ADR008_TOKEN) {
        Ok(()) => 0,
        Err(c) => c,
    };
    // 步3：回滚槽 7。
    let s2 = match layer.unfill_slot(7) {
        Ok(()) => 0,
        Err(c) => c,
    };
    // 步4：空槽封印（必拒）。
    let s3 = match layer.seal_slot(7) {
        Ok(()) => 0,
        Err(c) => c,
    };
    // 步5：无联签填充（必拒）。
    let s4 = match layer.fill_slot(7, 100, ADR008_TOKEN ^ 0xFFFF) {
        Ok(()) => 0,
        Err(c) => c,
    };
    [s0, s1, s2, s3, s4]
}

/// 解耦断言：空载态与满载态各跑机制签名序列，逐项比对——
///   ① 机制签名两态逐项一致（机制与基线解耦）；
///   ② 值语义两态确不同（槽 0：空载 NotBound，满载业务值）——
///     "值随基线变、机制不随基线变"是解耦的完整语义。
pub fn decoupling_check() -> Result<(), u32> {
    // 空载态。
    let mut empty_layer = DefaultLayer::new();
    let sig_empty = mech_signature(&mut empty_layer);
    // 满载态：联签填 6 个业务槽（不封，槽 7 保持空同构）。
    let mut full_layer = DefaultLayer::new();
    let mut d = 0;
    while d < DIM_SLOTS {
        full_layer.fill_slot(d as u8, 1000 + d as u32, ADR008_TOKEN)?;
        d += 1;
    }
    let sig_full = mech_signature(&mut full_layer);
    // ① 机制签名逐项比对。
    let mut i = 0;
    while i < 5 {
        if sig_empty.get(i).copied().unwrap_or(1) != sig_full.get(i).copied().unwrap_or(2) {
            return Err(DlCode::DecouplingBroken.code());
        }
        i += 1;
    }
    // ② 值语义两态不同（同槽 0：空载拒/满载答 1000）。
    if empty_layer.query(0) != Err(DlCode::UnboundAnswer.code())
        || full_layer.query(0) != Ok(1000)
    {
        return Err(DlCode::DecouplingBroken.code());
    }
    // 两态守恒均过。
    if !empty_layer.conserve_ok() || !full_layer.conserve_ok() {
        return Err(DlCode::ConservationBroken.code());
    }
    Ok(())
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：封槽改写注入（Sealed 后再填）。
    SealedWriteInject,
    /// 类1：无联签填充注入。
    NoTokenInject,
    /// 类2：槽号越界注入。
    SlotOverflowInject,
    /// 类3：值域污染注入。
    ValuePolluteInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::SealedWriteInject => 0,
            BadClass::NoTokenInject => 1,
            BadClass::SlotOverflowInject => 2,
            BadClass::ValuePolluteInject => 3,
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
    /// 记一注。
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
    /// 全槽全抓。
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
    /// 漏检总数。
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

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：封槽改写——填→封→再填必 SealedReject。
    let mut l0 = DefaultLayer::new();
    let f0 = l0.fill_slot(0, 5, ADR008_TOKEN);
    let s0 = l0.seal_slot(0);
    let w0 = l0.fill_slot(0, 6, ADR008_TOKEN);
    if let Some(h) = hits.get_mut(BadClass::SealedWriteInject.slot()) {
        *h = f0.is_ok() && s0.is_ok() && w0 == Err(DlCode::SealedReject.code());
    }
    // 类1：无联签——错误授权码必 TokenMismatch 且槽保持 Empty。
    let mut l1 = DefaultLayer::new();
    let r1 = l1.fill_slot(1, 5, ADR008_TOKEN ^ 0x1);
    let still_empty = match l1.slot(1) {
        Some(s) => s.phase == SlotPhase::Empty,
        None => false,
    };
    if let Some(h) = hits.get_mut(BadClass::NoTokenInject.slot()) {
        *h = r1 == Err(DlCode::TokenMismatch.code()) && still_empty;
    }
    // 类2：槽号越界（8 号）。
    let mut l2 = DefaultLayer::new();
    if let Some(h) = hits.get_mut(BadClass::SlotOverflowInject.slot()) {
        *h = l2.fill_slot(SLOT_COUNT as u8, 5, ADR008_TOKEN) == Err(DlCode::SlotOutOfRange.code()) && l2.query(SLOT_COUNT as u8) == Err(DlCode::SlotOutOfRange.code());
    }
    // 类3：值域污染（0x10000）。
    let mut l3 = DefaultLayer::new();
    if let Some(h) = hits.get_mut(BadClass::ValuePolluteInject.slot()) {
        *h = l3.fill_slot(2, SLOT_VALUE_MAX + 1, ADR008_TOKEN) == Err(DlCode::ValueOutOfRange.code());
    }
    hits
}

/// 注入 rounds 轮。
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
/// 黄金全链：空载可运行→六槽联签填充→逐槽查询→封印→改写拒→
/// 解耦断言→注入全抓→守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut layer = DefaultLayer::new();
    // 空载态：六业务槽查询全走 NotBound（机制活着，不 panic）。
    let mut d = 0;
    while d < DIM_SLOTS {
        if layer.query(d as u8) != Err(DlCode::UnboundAnswer.code()) {
            return Err(DlCode::UnboundAnswer.code());
        }
        d += 1;
    }
    // 联签填充六业务槽（ADR-UNX-008 接口）。
    let mut e = 0;
    while e < DIM_SLOTS {
        // 值推导：1062 + 槽号×16（确定性，落 0xFFFF 域内）。
        layer.fill_slot(e as u8, 1062 + (e as u32) * 16, ADR008_TOKEN)?;
        e += 1;
    }
    if layer.bound_count() != DIM_SLOTS as u32 {
        return Err(DlCode::RefillRejected.code());
    }
    // 逐槽查询命中。
    let mut q = 0;
    while q < DIM_SLOTS {
        let want = 1062 + (q as u32) * 16;
        if layer.query(q as u8)? != want {
            return Err(DlCode::UnboundAnswer.code());
        }
        q += 1;
    }
    // 封印六槽。
    let mut z = 0;
    while z < DIM_SLOTS {
        layer.seal_slot(z as u8)?;
        z += 1;
    }
    // 封槽改写必拒。
    if layer.fill_slot(0, 7, ADR008_TOKEN) != Err(DlCode::SealedReject.code()) {
        return Err(DlCode::SealedReject.code());
    }
    // 解耦断言。
    decoupling_check()?;
    // 注入全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(DlCode::ConservationBroken.code());
    }
    Ok((layer.bound_count(), layer.rejected))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DlCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：空载层五步机制签名全符合承诺（空载可运行直验）。
pub fn probe_a() -> Result<(), u32> {
    let mut layer = DefaultLayer::new();
    let sig = mech_signature(&mut layer);
    // 承诺序列：槽号闸拒/填过/滚过/空槽封印拒/无联签拒。
    let expect = [
        DlCode::SlotOutOfRange.code(),
        0,
        0,
        DlCode::SealEmpty.code(),
        DlCode::TokenMismatch.code(),
    ];
    let mut i = 0;
    while i < 5 {
        if sig.get(i).copied().unwrap_or(1) != expect.get(i).copied().unwrap_or(2) {
            return Err(DlCode::DecouplingBroken.code());
        }
        i += 1;
    }
    Ok(())
}

/// 探针B：封印后回滚必拒（封印单向直验）。
pub fn probe_b() -> Result<(), u32> {
    let mut layer = DefaultLayer::new();
    layer.fill_slot(3, 42, ADR008_TOKEN)?;
    layer.seal_slot(3)?;
    if layer.unfill_slot(3) == Err(DlCode::UnfillSealed.code()) {
        Ok(())
    } else {
        Err(DlCode::UnfillSealed.code())
    }
}

/// 探针C：拒因留账——TokenMismatch 注入后账上可复算。
pub fn probe_c() -> Result<(), u32> {
    let mut layer = DefaultLayer::new();
    let _ = layer.fill_slot(1, 5, 0xDEAD);
    if layer.rejected_with(DlCode::TokenMismatch.code()) && layer.conserve_ok() {
        Ok(())
    } else {
        Err(DlCode::TokenMismatch.code())
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < DlCode::ALL.len() {
        if let Some(c) = DlCode::ALL.get(i) {
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
/// run_unx18427_checks：J1 判据 16 条（断言性质，动态明细 ≤8）。
pub fn run_unx18427_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18427 系统默认值层（锚基线解耦插槽）");
    // J1-1 冻结常量：8 槽/6 业务槽/联签码 0x00800093。
    s.add(
        "J1-插槽布局冻结",
        SLOT_COUNT == 8 && DIM_SLOTS == 6 && ADR008_TOKEN == 0x0080_0093,
        "8槽=6维+2备 联签0x00800093",
    );
    // J1-2 空载可运行：空载层六槽查询全走 NotBound 且机制活着。
    let empty_alive = {
        let mut l = DefaultLayer::new();
        let mut ok = true;
        let mut d = 0;
        while d < DIM_SLOTS {
            if l.query(d as u8) != Err(DlCode::UnboundAnswer.code()) {
                ok = false;
            }
            d += 1;
        }
        ok && l.conserve_ok() && l.bound_count() == 0
    };
    s.add("J1-空载可运行", empty_alive, "全空槽不 panic 不误答");
    // J1-3 解耦断言：空载/满载两态机制签名逐项一致。
    s.add("J1-机制基线解耦", decoupling_check().is_ok(), "两态机账行为逐项一致");
    // J1-4 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        matches!(&g, Ok((b, _)) if *b == DIM_SLOTS as u32),
        &format!("{:?}", g),
    );
    // J1-5 ADR-UNX-008 填充接口：联签过闸可填可查。
    let fill_ok = {
        let mut l = DefaultLayer::new();
        let f = l.fill_slot(0, 10240, ADR008_TOKEN);
        let q = l.query(0);
        f.is_ok() && q == Ok(10240)
    };
    s.add("J1-联签填充接口", fill_ok, "ADR008_TOKEN 过闸填查一致");
    // J1-6 无联签注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-7 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-8 状态机单向：Filled→Sealed 后回滚拒；Empty 回滚拒。
    let sm = {
        let mut l = DefaultLayer::new();
        let a = l.unfill_slot(0) == Err(DlCode::UnfillEmpty.code());
        let _ = l.fill_slot(0, 1, ADR008_TOKEN);
        let b = l.unfill_slot(0).is_ok();
        let _ = l.fill_slot(0, 2, ADR008_TOKEN);
        let _ = l.seal_slot(0);
        let c = l.unfill_slot(0) == Err(DlCode::UnfillSealed.code());
        a && b && c
    };
    s.add("J1-状态机单向", sm, "Empty回滚拒 Filled可滚 Sealed封死");
    // J1-9 重复填充拒：Filled 未滚再填必拒。
    let refill = {
        let mut l = DefaultLayer::new();
        let _ = l.fill_slot(1, 1, ADR008_TOKEN);
        l.fill_slot(1, 2, ADR008_TOKEN) == Err(DlCode::RefillRejected.code())
    };
    s.add("J1-重复填充拒", refill, "Filled 槽未滚再填拒");
    // J1-10 空槽封印拒：先填后封才合法。
    s.add(
        "J1-空槽封印拒",
        {
            let mut l = DefaultLayer::new();
            l.seal_slot(5) == Err(DlCode::SealEmpty.code())
        },
        "Empty 槽直封拒",
    );
    // J1-11 值域边界：0 过/0xFFFF 过/0x10000 拒。
    let mut lval = DefaultLayer::new();
    let v0 = lval.fill_slot(0, 0, ADR008_TOKEN);
    let _ = lval.unfill_slot(0);
    let v1 = lval.fill_slot(0, SLOT_VALUE_MAX, ADR008_TOKEN);
    let _ = lval.unfill_slot(0);
    let v2 = lval.fill_slot(0, SLOT_VALUE_MAX + 1, ADR008_TOKEN);
    s.add(
        "J1-值域三点",
        v0.is_ok() && v1.is_ok() && v2.is_err(),
        "0/上界过 上界+1拒",
    );
    // J1-12 拒因留账（探针C 直验）。
    s.add("J1-拒因留账", probe_c().is_ok(), "TokenMismatch 入账可复算");
    // J1-13 探针三序。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "空载签名/封印单向/留账");
    // J1-14 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-15 文案闭集。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DlCode::ALL.len() {
            if let Some(c) = DlCode::ALL.get(i) {
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
    s.add("J3-段位守卫", segment_guard(), "0x7E680 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18427_checks();
        assert!(set.all_passed(), "UNX-F18427 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_empty_layer_runs() {
        let l = DefaultLayer::new();
        assert_eq!(l.bound_count(), 0);
        for d in 0..DIM_SLOTS as u8 {
            assert_eq!(l.query(d), Err(DlCode::UnboundAnswer.code()));
        }
        assert!(l.conserve_ok());
    }

    #[test]
    fn t_fill_seal_write_reject() {
        let mut l = DefaultLayer::new();
        assert!(l.fill_slot(0, 10, ADR008_TOKEN).is_ok());
        assert!(l.seal_slot(0).is_ok());
        assert_eq!(l.fill_slot(0, 11, ADR008_TOKEN), Err(DlCode::SealedReject.code()));
        assert_eq!(l.query(0), Ok(10));
    }

    #[test]
    fn t_token_gate() {
        let mut l = DefaultLayer::new();
        assert_eq!(l.fill_slot(0, 10, 0x0), Err(DlCode::TokenMismatch.code()));
        assert_eq!(l.fill_slot(0, 10, 0xFFFF_FFFF), Err(DlCode::TokenMismatch.code()));
        assert!(l.fill_slot(0, 10, ADR008_TOKEN).is_ok());
    }

    #[test]
    fn t_slot_range_gate() {
        let mut l = DefaultLayer::new();
        assert_eq!(
            l.fill_slot(SLOT_COUNT as u8, 1, ADR008_TOKEN),
            Err(DlCode::SlotOutOfRange.code())
        );
        assert_eq!(l.query(SLOT_COUNT as u8), Err(DlCode::SlotOutOfRange.code()));
        assert!(l.slot(SLOT_COUNT as u8).is_none());
    }

    #[test]
    fn t_state_machine_roundtrip() {
        let mut l = DefaultLayer::new();
        assert_eq!(l.unfill_slot(2), Err(DlCode::UnfillEmpty.code()));
        assert!(l.fill_slot(2, 77, ADR008_TOKEN).is_ok());
        assert!(l.unfill_slot(2).is_ok());
        assert_eq!(l.query(2), Err(DlCode::UnboundAnswer.code()));
        assert!(l.fill_slot(2, 78, ADR008_TOKEN).is_ok());
        assert!(l.seal_slot(2).is_ok());
        assert_eq!(l.unfill_slot(2), Err(DlCode::UnfillSealed.code()));
    }

    #[test]
    fn t_decoupling() {
        assert!(decoupling_check().is_ok());
        assert!(probe_a().is_ok());
    }

    #[test]
    fn t_inject_ledger_and_repro() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
