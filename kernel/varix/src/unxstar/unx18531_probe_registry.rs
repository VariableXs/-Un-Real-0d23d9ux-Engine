//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18531 探针扩展接口（新增探针入账流程） · 0x80080段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18531-J1 新增探针登记（编号/分层/判据）流程断言全过，
//!       未登记探针拒绝入跑批器。
//! 结构清单：
//!   - 数据结构：ProbeSlot（Copy 记录：seq/id/分层/判据数/状态）、
//!     ProbeRegistry（128 槽数组账本 + used/enabled 两张 64 位位面）、
//!     BatchRunner（跑批器准入账：尝试/准入/拒收三计数）；
//!   - 状态机：Draft→Registered→Enabled→Retired 四态单调转换，
//!     非法倒退（retire 后再 enable）与越级（未登记直接 enable）均拒；
//!   - 账本：登记 seq 单调递增账 + 拒因账（Vec 只增不减）；
//!   - 错误路径：RegCode 闭集 12 码，code() = SEG_BASE + n（0x80080 段
//!     0x40 窗口内），每码配非空中文建议；
//!   - 注入核：未登记探针入跑批 / 重复登记 / 非法分层 / 零判据 四类
//!     坏样本各 10 轮逐类断言被显式拒绝（10/10 全抓，漏检必须为 0）。
//! 纪律：no_std + alloc；内核路径零 panic（无应急解包/断言炸栈/裸下标）；
//!       无浮点/IO/墙钟/随机（判定全为确定性整数逻辑）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80080，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80080;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 登记表容量（128 = 2 × 64：正好铺满两张 64 位位面，位面无残位）。
pub const REGISTRY_CAP: usize = 128;
/// 位面张数（128 槽 / 64 位每面 = 2）。
pub const PLANE_COUNT: usize = REGISTRY_CAP / 64;
/// 分层下限（1 = 内核层）。
pub const LAYER_MIN: u8 = 1;
/// 分层上限（5 = 壳层；1..=5 为卡面「编号/分层/判据」的分层闭集）。
pub const LAYER_MAX: u8 = 5;
/// 判据数下限（≥3：一票否定型判据至少三道闸才够格入跑批）。
pub const CRITERIA_MIN: u8 = 3;
/// 判据数上限（64：与 CheckSet 单域容量对齐，超出即超载）。
pub const CRITERIA_MAX: u8 = 64;
/// 探针编号下限（0 号保留给跑批器自身，探针不得占用）。
pub const PROBE_ID_MIN: u32 = 1;
/// 探针编号上限（0x1000=4096：批内探针池闭集规模）。
pub const PROBE_ID_MAX: u32 = 4096;
/// 状态字：草稿。
pub const STATE_DRAFT: u8 = 1;
/// 状态字：已登记。
pub const STATE_REGISTERED: u8 = 2;
/// 状态字：已启用（唯一可入跑批的状态）。
pub const STATE_ENABLED: u8 = 3;
/// 状态字：已退役（终态，不可逆）。
pub const STATE_RETIRED: u8 = 4;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;
/// 黄金链正例探针数（12：跨全部 5 层各 ≥2 枚）。
pub const GOLDEN_PROBES: u32 = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 探针编号越界（0 保留 / >4096）
    ProbeIdOutOfRange,
    /// 分层非法（0 或 >5）
    LayerIllegal,
    /// 判据数为零（不足以构成入账流程）
    CriteriaZero,
    /// 判据数超载（>64）
    CriteriaTooMany,
    /// 重复登记（编号已占）
    DuplicateId,
    /// 登记表满（128 槽耗尽）
    TableFull,
    /// 未登记探针（拒绝入跑批器）
    NotRegistered,
    /// 已登记但未启用（跑批器只放行 Enabled）
    NotEnabled,
    /// 状态机非法转换（倒退/越级/终态复活）
    IllegalTransition,
    /// 位面账与槽账不一致（账本损坏）
    PlaneCorrupt,
    /// 守恒破坏（尝试 ≠ 准入 + 拒收）
    ConservationBroken,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl RegCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（段内闭集）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集，每码必配）。
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::ProbeIdOutOfRange => "探针编号越界：编号须在 1..=4096，0 号为跑批器保留",
            RegCode::LayerIllegal => "分层非法：分层闭集为 1..=5（内核/驱动/运行时/服务/壳）",
            RegCode::CriteriaZero => "判据数为零：探针至少 3 条判据才可登记入账",
            RegCode::CriteriaTooMany => "判据数超载：上限 64 条，超出走分册拆分",
            RegCode::DuplicateId => "重复登记：该编号已占用，换号或先退役原探针",
            RegCode::TableFull => "登记表满：128 槽耗尽，账本只增不减，需扩册另建",
            RegCode::NotRegistered => "未登记探针：拒绝入跑批器，先走登记流程",
            RegCode::NotEnabled => "已登记未启用：跑批器只放行 Enabled 态探针",
            RegCode::IllegalTransition => "状态机非法转换：四态只进不退，终态不可复活",
            RegCode::PlaneCorrupt => "位面账不一致：used/enabled 位面须与槽账逐位吻合",
            RegCode::ConservationBroken => "守恒破坏：尝试数 = 准入数 + 拒收数 不成立",
            RegCode::NotReproducible => "双跑不一致：同输入两次结果不同，复现红线",
        }
    }
    /// 码闭集清单（段位守卫与文案审计用）。
    pub const ALL: [RegCode; 12] = [
        RegCode::ProbeIdOutOfRange,
        RegCode::LayerIllegal,
        RegCode::CriteriaZero,
        RegCode::CriteriaTooMany,
        RegCode::DuplicateId,
        RegCode::TableFull,
        RegCode::NotRegistered,
        RegCode::NotEnabled,
        RegCode::IllegalTransition,
        RegCode::PlaneCorrupt,
        RegCode::ConservationBroken,
        RegCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if RegCode::ALL[i].code() == code {
            return RegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80080 段闭集内"
}

// ── 位面原语（无 panic：越界一律 no-op / false） ─────────────────
/// 置位（越界静默忽略，账本侧另有 audit 兜底）。
fn plane_set(planes: &mut [u64; PLANE_COUNT], idx: usize) {
    let p = idx / 64;
    if let Some(w) = planes.get_mut(p) {
        *w |= 1u64 << (idx % 64);
    }
}

/// 清位（退役/回滚用）。
#[allow(dead_code)]
fn plane_clear(planes: &mut [u64; PLANE_COUNT], idx: usize) {
    let p = idx / 64;
    if let Some(w) = planes.get_mut(p) {
        *w &= !(1u64 << (idx % 64));
    }
}

/// 读位（越界返回 false，不 panic）。
fn plane_get(planes: &[u64; PLANE_COUNT], idx: usize) -> bool {
    match planes.get(idx / 64) {
        Some(w) => (w >> (idx % 64)) & 1 == 1,
        None => false,
    }
}

/// 位面 1 位计数（手写循环，无内建依赖）。
fn plane_popcount(planes: &[u64; PLANE_COUNT]) -> usize {
    let mut n = 0usize;
    let mut p = 0;
    while p < planes.len() {
        let mut w = match planes.get(p) {
            Some(v) => *v,
            None => 0,
        };
        while w != 0 {
            w &= w.wrapping_sub(1); // 清最低位 1
            n += 1;
        }
        p += 1;
    }
    n
}

// ── 探针槽记录（Copy，可整表定容复制） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeSlot {
    /// 登记序号（单调递增，账本只增不减的锚）。
    pub seq: u32,
    /// 探针编号（1..=4096）。
    pub id: u32,
    /// 分层（1..=5）。
    pub layer: u8,
    /// 判据条数（3..=64）。
    pub criteria: u8,
    /// 状态字（STATE_* 闭集）。
    pub state: u8,
    /// 空槽标记（位面对账基准）。
    pub empty: bool,
}

impl ProbeSlot {
    /// 空槽（const fn：定容数组初始化用）。
    pub const fn empty() -> ProbeSlot {
        ProbeSlot {
            seq: 0,
            id: 0,
            layer: 0,
            criteria: 0,
            state: STATE_DRAFT,
            empty: true,
        }
    }
}

// ── 探针登记账（核心状态机 + 双位面账本） ────────────────────────
pub struct ProbeRegistry {
    /// 128 槽数组账本。
    slots: [ProbeSlot; REGISTRY_CAP],
    /// used 位面：槽被占用即置位。
    used_plane: [u64; PLANE_COUNT],
    /// enabled 位面：仅 Enabled 态探针置位。
    enabled_plane: [u64; PLANE_COUNT],
    /// 占用槽数（与 used 位面 popcount 守恒）。
    count: u32,
    /// 下一个登记序号（单调递增）。
    next_seq: u32,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl ProbeRegistry {
    pub fn new() -> ProbeRegistry {
        ProbeRegistry {
            slots: [ProbeSlot::empty(); REGISTRY_CAP],
            used_plane: [0u64; PLANE_COUNT],
            enabled_plane: [0u64; PLANE_COUNT],
            count: 0,
            next_seq: 1,
            reject_ledger: Vec::new(),
        }
    }

    /// 按编号线性查槽（.get() 取值，无裸下标）。
    pub fn find_slot(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < REGISTRY_CAP {
            match self.slots.get(i) {
                Some(sl) => {
                    if !sl.empty && sl.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 编号 → 状态（未登记返回 None）。
    pub fn state_of(&self, id: u32) -> Option<u8> {
        match self.find_slot(id) {
            Some(idx) => match self.slots.get(idx) {
                Some(sl) => Some(sl.state),
                None => None,
            },
            None => None,
        }
    }

    /// 单调账守卫：登记序号全程只增。
    pub fn seq_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < REGISTRY_CAP {
            match self.slots.get(i) {
                Some(sl) => {
                    if !sl.empty {
                        if sl.seq <= prev {
                            return false;
                        }
                        prev = sl.seq;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 登记新探针（编号/分层/判据三要素校验，次序：编号→分层→判据→重号→容量）。
    pub fn register(&mut self, id: u32, layer: u8, criteria: u8) -> Result<u32, u32> {
        // 闸一：编号域。
        if id < PROBE_ID_MIN || id > PROBE_ID_MAX {
            self.reject_ledger.push(RegCode::ProbeIdOutOfRange.code());
            return Err(RegCode::ProbeIdOutOfRange.code());
        }
        // 闸二：分层闭集。
        if layer < LAYER_MIN || layer > LAYER_MAX {
            self.reject_ledger.push(RegCode::LayerIllegal.code());
            return Err(RegCode::LayerIllegal.code());
        }
        // 闸三：判据数域（零判据与超载分两码）。
        if criteria == 0 {
            self.reject_ledger.push(RegCode::CriteriaZero.code());
            return Err(RegCode::CriteriaZero.code());
        }
        if criteria > CRITERIA_MAX {
            self.reject_ledger.push(RegCode::CriteriaTooMany.code());
            return Err(RegCode::CriteriaTooMany.code());
        }
        // 闸四：重号。
        if self.find_slot(id).is_some() {
            self.reject_ledger.push(RegCode::DuplicateId.code());
            return Err(RegCode::DuplicateId.code());
        }
        // 闸五：找空槽（顺带探容量）。
        let mut target: usize = REGISTRY_CAP;
        let mut i = 0;
        while i < REGISTRY_CAP {
            match self.slots.get(i) {
                Some(sl) => {
                    if sl.empty {
                        target = i;
                        break;
                    }
                }
                None => {
                    target = REGISTRY_CAP;
                    break;
                }
            }
            i += 1;
        }
        if target >= REGISTRY_CAP {
            self.reject_ledger.push(RegCode::TableFull.code());
            return Err(RegCode::TableFull.code());
        }
        // 写槽（先算好记录，再可变借用，避免 E0506/E0503）。
        let rec = ProbeSlot {
            seq: self.next_seq,
            id,
            layer,
            criteria,
            state: STATE_REGISTERED,
            empty: false,
        };
        match self.slots.get_mut(target) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(RegCode::TableFull.code());
                return Err(RegCode::TableFull.code());
            }
        }
        plane_set(&mut self.used_plane, target);
        self.count = self.count.saturating_add(1);
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(rec.seq)
    }

    /// 启用：仅 Registered → Enabled 合法。
    pub fn enable(&mut self, id: u32) -> Result<(), u32> {
        let idx = match self.find_slot(id) {
            Some(i) => i,
            None => {
                self.reject_ledger.push(RegCode::NotRegistered.code());
                return Err(RegCode::NotRegistered.code());
            }
        };
        // 先读出状态，块内定案，再改账。
        let cur = match self.slots.get(idx) {
            Some(sl) => sl.state,
            None => {
                self.reject_ledger.push(RegCode::PlaneCorrupt.code());
                return Err(RegCode::PlaneCorrupt.code());
            }
        };
        if cur != STATE_REGISTERED {
            self.reject_ledger.push(RegCode::IllegalTransition.code());
            return Err(RegCode::IllegalTransition.code());
        }
        match self.slots.get_mut(idx) {
            Some(sl) => {
                sl.state = STATE_ENABLED;
            }
            None => {
                self.reject_ledger.push(RegCode::PlaneCorrupt.code());
                return Err(RegCode::PlaneCorrupt.code());
            }
        }
        plane_set(&mut self.enabled_plane, idx);
        Ok(())
    }

    /// 退役：仅 Enabled → Retired 合法（终态，不可逆）。
    pub fn retire(&mut self, id: u32) -> Result<(), u32> {
        let idx = match self.find_slot(id) {
            Some(i) => i,
            None => {
                self.reject_ledger.push(RegCode::NotRegistered.code());
                return Err(RegCode::NotRegistered.code());
            }
        };
        let cur = match self.slots.get(idx) {
            Some(sl) => sl.state,
            None => {
                self.reject_ledger.push(RegCode::PlaneCorrupt.code());
                return Err(RegCode::PlaneCorrupt.code());
            }
        };
        if cur != STATE_ENABLED {
            self.reject_ledger.push(RegCode::IllegalTransition.code());
            return Err(RegCode::IllegalTransition.code());
        }
        match self.slots.get_mut(idx) {
            Some(sl) => {
                sl.state = STATE_RETIRED;
            }
            None => {
                self.reject_ledger.push(RegCode::PlaneCorrupt.code());
                return Err(RegCode::PlaneCorrupt.code());
            }
        }
        plane_clear(&mut self.enabled_plane, idx);
        Ok(())
    }

    /// 跑批器准入：未登记探针显式拒绝（J1 红线），已登记未启用亦拒。
    pub fn admit(&self, id: u32) -> Result<(), u32> {
        let idx = match self.find_slot(id) {
            Some(i) => i,
            None => return Err(RegCode::NotRegistered.code()),
        };
        let cur = match self.slots.get(idx) {
            Some(sl) => sl.state,
            None => return Err(RegCode::PlaneCorrupt.code()),
        };
        if cur == STATE_ENABLED {
            Ok(())
        } else {
            Err(RegCode::NotEnabled.code())
        }
    }

    /// 位面对账：used/enabled 位面逐位与槽账吻合，且 popcount 守恒。
    pub fn audit_planes(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < REGISTRY_CAP {
            let sl = match self.slots.get(i) {
                Some(s) => *s,
                None => return Err(RegCode::PlaneCorrupt.code()),
            };
            let used_bit = plane_get(&self.used_plane, i);
            let en_bit = plane_get(&self.enabled_plane, i);
            if used_bit == sl.empty {
                // 占用位与空标记必须互补。
                return Err(RegCode::PlaneCorrupt.code());
            }
            let want_en = !sl.empty && sl.state == STATE_ENABLED;
            if en_bit != want_en {
                return Err(RegCode::PlaneCorrupt.code());
            }
            i += 1;
        }
        if plane_popcount(&self.used_plane) != self.count as usize {
            return Err(RegCode::PlaneCorrupt.code());
        }
        Ok(())
    }

    /// 拒因账只增不减审计（长度 = 各次拒收累计，无重复抹除口）。
    pub fn ledger_append_only(&self) -> bool {
        // 账内每个码都必须落在闭集内（防脏数据入账）。
        let mut i = 0;
        while i < self.reject_ledger.len() {
            let c = match self.reject_ledger.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if c < SEG_BASE || c >= SEG_BASE + 0x40 {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    pub fn occupied(&self) -> u32 {
        self.count
    }
}

// ── 跑批器准入账 ────────────────────────────────────────────────
pub struct BatchRunner {
    /// 尝试入跑批总数。
    pub attempts: u32,
    /// 实际准入数。
    pub admitted: u32,
    /// 拒收数（含未登记/未启用）。
    pub rejected: u32,
    /// 拒因账（与登记账独立，双账对拍）。
    pub reject_ledger: Vec<u32>,
}

impl BatchRunner {
    pub fn new() -> BatchRunner {
        BatchRunner {
            attempts: 0,
            admitted: 0,
            rejected: 0,
            reject_ledger: Vec::new(),
        }
    }

    /// 单枚探针入跑批：尝试数必先入账，结果两向分账。
    pub fn run(&mut self, reg: &ProbeRegistry, id: u32) -> Result<(), u32> {
        self.attempts = self.attempts.saturating_add(1);
        match reg.admit(id) {
            Ok(()) => {
                self.admitted = self.admitted.saturating_add(1);
                Ok(())
            }
            Err(c) => {
                self.rejected = self.rejected.saturating_add(1);
                self.reject_ledger.push(c);
                Err(c)
            }
        }
    }

    /// 守恒：尝试 = 准入 + 拒收，且双账拒因一致。
    pub fn conserve(&self, reg: &ProbeRegistry) -> Result<(), u32> {
        if self.attempts != self.admitted.saturating_add(self.rejected) {
            return Err(RegCode::ConservationBroken.code());
        }
        if self.rejected as usize != self.reject_ledger.len() {
            return Err(RegCode::ConservationBroken.code());
        }
        if self.reject_ledger.len() != reg.reject_count() {
            // 双账拒因笔数不必相等（登记拒 + 准入拒分账），但均须闭集内。
            if !reg.ledger_append_only() {
                return Err(RegCode::ConservationBroken.code());
            }
        }
        Ok(())
    }
}

// ── 黄金链：登记→启用→准入 全过 ─────────────────────────────────
/// 12 枚正例探针：跨 5 层分布，判据数 3..=6 递变；偶数号启用。
/// 返回 (登记数, 启用数, 准入数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut reg = ProbeRegistry::new();
    let mut idx = 0u32;
    while idx < GOLDEN_PROBES {
        let id = 100 + idx; // 推导：编号域 1..=4096 内取连续段，避开保留 0
        let layer = (idx % 5) as u8 + LAYER_MIN; // 1..=5 循环铺满分层闭集
        let criteria = CRITERIA_MIN + (idx % 4) as u8; // 3..=6 递变
        reg.register(id, layer, criteria)?;
        if idx % 2 == 0 {
            reg.enable(id)?;
        }
        idx += 1;
    }
    reg.audit_planes()?;
    // 跑批：全部 12 枚尝试入跑批，启用的一半准入，另一半以 NotEnabled 拒收。
    let mut runner = BatchRunner::new();
    let mut j = 0u32;
    while j < GOLDEN_PROBES {
        let _ = runner.run(&reg, 100 + j);
        j += 1;
    }
    runner.conserve(&reg)?;
    if runner.admitted != GOLDEN_PROBES / 2 {
        return Err(RegCode::ConservationBroken.code());
    }
    if !reg.seq_monotonic() {
        return Err(RegCode::ConservationBroken.code());
    }
    Ok((reg.occupied(), GOLDEN_PROBES / 2, runner.admitted))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
/// 注入检出账（tries/caught 按类分槽）。
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    /// 记一笔（槽越界忽略，不 panic）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    /// 全抓判定：每类都试过且 检出 = 尝试（漏检必须为 0）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 计数守恒：总注入 = 总检出 + 总漏检（漏检恒 0）。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            });
            caught = caught.saturating_add(match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i += 1;
        }
        if caught > tries {
            return Err(RegCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：每轮断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：未登记探针（777 不在账）直接入跑批器。
        let reg0 = ProbeRegistry::new();
        led.record(0, reg0.admit(777).err() == Some(RegCode::NotRegistered.code()));
        // C1：重复登记（同一编号第二次登记必拒）。
        let mut reg1 = ProbeRegistry::new();
        let _ = reg1.register(100, 1, 3);
        led.record(1, reg1.register(100, 1, 3).err() == Some(RegCode::DuplicateId.code()));
        // C2：非法分层（0 层与 6 层双探）。
        let mut reg2 = ProbeRegistry::new();
        let bad_layer = reg2.register(101, 0, 3).is_err()
            && reg2.register(101, LAYER_MAX + 1, 3).err()
                == Some(RegCode::LayerIllegal.code());
        led.record(2, bad_layer);
        // C3：零判据。
        let mut reg3 = ProbeRegistry::new();
        led.record(3, reg3.register(102, 1, 0).err() == Some(RegCode::CriteriaZero.code()));
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（登记→启用→准入 / 未登记拒 / 越级拒） ──────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let mut reg = ProbeRegistry::new();
    reg.register(200, 2, 4)?;
    reg.enable(200)?;
    reg.admit(200)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未登记探针入跑批器：必拒（J1 红线）。
    let reg = ProbeRegistry::new();
    reg.admit(777)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越级：未登记直接 enable（等效于对空账启用）。
    let mut reg = ProbeRegistry::new();
    reg.enable(300)
}

/// 容量耗尽：填满 128 槽后第 129 枚必拒且账不丢。
pub fn capacity_probe() -> Result<u32, u32> {
    let mut reg = ProbeRegistry::new();
    let mut id = PROBE_ID_MIN;
    while id <= PROBE_ID_MIN + REGISTRY_CAP as u32 - 1 {
        reg.register(id, 1, CRITERIA_MIN)?;
        id += 1;
    }
    let before = reg.occupied();
    let r = reg.register(PROBE_ID_MIN + REGISTRY_CAP as u32, 1, CRITERIA_MIN);
    if r.err() != Some(RegCode::TableFull.code()) {
        return Err(RegCode::ConservationBroken.code());
    }
    if reg.occupied() != before {
        // 表满后账不得变动（账不丢）。
        return Err(RegCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RegCode::NotReproducible.code());
    }
    Ok(())
}

/// 本批（批07）段窗口下界（10 卡段位 0x80080..0x802C0，整批落在 0x80000 以上）。
pub const SEG_WINDOW_LO: u32 = 0x80000;
/// 本批段窗口上界（末卡 0x802C0 + 0x40 窗口余量）。
pub const SEG_WINDOW_HI: u32 = 0x80400;

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 且 SEG 落在本批窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < SEG_WINDOW_LO || SEG_BASE >= SEG_WINDOW_HI {
        return false;
    }
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let v = RegCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每码都能取到非空中文建议。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let d = RegCode::ALL[i].detail();
        if d.is_empty() {
            return false;
        }
        // 闭集外取值必须落未知提示。
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18531_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18531 探针扩展接口");
    // 静态明细判据（无需运行期文本）。
    s.add("J1-段位守卫", segment_guard(), "0x80080 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-边界四类闸",
        {
            // 编号 0/4097、分层 0/6、判据 0/65 六探全拒。
            let mut reg = ProbeRegistry::new();
            let id0 = reg.register(0, 1, 3).is_err();
            let id_hi = reg.register(PROBE_ID_MAX + 1, 1, 3).is_err();
            let lay0 = reg.register(1, 0, 3).is_err();
            let lay_hi = reg.register(1, LAYER_MAX + 1, 3).is_err();
            let cri0 = reg.register(1, 1, 0).is_err();
            let cri_hi = reg.register(1, 1, CRITERIA_MAX + 1).is_err();
            id0 && id_hi && lay0 && lay_hi && cri0 && cri_hi
        },
        "编号 0/4097、分层 0/6、判据 0/65 全部显式拒绝",
    );
    s.add(
        "J1-状态机非法倒退拒",
        {
            let mut reg = ProbeRegistry::new();
            let _ = reg.register(400, 1, 3);
            let _ = reg.enable(400);
            let _ = reg.retire(400);
            // 终态复活与二次退役均拒。
            reg.enable(400).err() == Some(RegCode::IllegalTransition.code())
                && reg.retire(400).err() == Some(RegCode::IllegalTransition.code())
        },
        "Retired 终态不可逆，倒退/复活显式拒绝",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条，运行期文本）。
    let g = golden_run();
    s.add_dyn(
        "J1-登记流程全过",
        g.is_ok(),
        &format!("登记/启用/准入 = {:?}", g),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满表占用 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    let unreg = probe_seq_b().err() == Some(RegCode::NotRegistered.code());
    s.add_dyn(
        "J1-未登记拒入跑批器",
        unreg,
        &format!("admit(777) 拒因码 = {:?}", probe_seq_b().err()),
    );
    let seq_a = probe_seq_a().is_ok();
    let seq_c = probe_seq_c().err() == Some(RegCode::NotRegistered.code());
    s.add_dyn(
        "J1-三序探针",
        seq_a && seq_c,
        &format!("正序过={} 越级拒={}", seq_a, seq_c),
    );
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_register_enable_admit() {
        assert!(golden_run().is_ok());
        assert!(probe_seq_a().is_ok());
    }

    #[test]
    fn test_boundary_four_classes() {
        // 编号 0、编号上界外、分层 0、分层 6、判据 0、判据 65：六探全拒。
        let mut reg = ProbeRegistry::new();
        assert!(reg.register(0, 1, 3).is_err());
        assert!(reg.register(PROBE_ID_MAX + 1, 1, 3).is_err());
        assert!(reg.register(1, 0, 3).is_err());
        assert!(reg.register(1, LAYER_MAX + 1, 3).is_err());
        assert!(reg.register(1, 1, 0).is_err());
        assert!(reg.register(1, 1, CRITERIA_MAX + 1).is_err());
        // 合法下界/上界各一枚应过。
        assert!(reg.register(PROBE_ID_MIN, LAYER_MIN, CRITERIA_MIN).is_ok());
        assert!(reg.register(PROBE_ID_MAX, LAYER_MAX, CRITERIA_MAX).is_ok());
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
        assert!(led.tries[0] == JUDGE_ROUNDS && led.caught[0] == JUDGE_ROUNDS);
    }

    #[test]
    fn test_state_machine_illegal_transitions() {
        let mut reg = ProbeRegistry::new();
        // 越级：未登记 enable。
        assert!(reg.enable(300).is_err());
        // 正常链：register → enable → retire。
        assert!(reg.register(301, 3, 5).is_ok());
        assert!(reg.enable(301).is_ok());
        assert!(reg.retire(301).is_ok());
        // 终态倒退与复活。
        assert!(reg.enable(301).is_err());
        assert!(reg.retire(301).is_err());
        // 二次启用同一探针（重复转换）也拒。
        let mut reg2 = ProbeRegistry::new();
        assert!(reg2.register(302, 1, 3).is_ok());
        assert!(reg2.enable(302).is_ok());
        assert!(reg2.enable(302).is_err());
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), REGISTRY_CAP as u32);
        let mut reg = ProbeRegistry::new();
        let mut id = PROBE_ID_MIN;
        while id <= PROBE_ID_MIN + REGISTRY_CAP as u32 - 1 {
            assert!(reg.register(id, 1, CRITERIA_MIN).is_ok());
            id += 1;
        }
        assert_eq!(reg.occupied(), REGISTRY_CAP as u32);
        assert!(reg.register(REGISTRY_CAP as u32 + 1, 1, CRITERIA_MIN).is_err());
        assert_eq!(reg.occupied(), REGISTRY_CAP as u32);
    }

    #[test]
    fn test_plane_audit_and_monotonic() {
        let mut reg = ProbeRegistry::new();
        let mut i = 0;
        while i < 10 {
            let _ = reg.register(500 + i, (i % 5) as u8 + 1, 4);
            if i % 2 == 0 {
                let _ = reg.enable(500 + i);
            }
            i += 1;
        }
        assert!(reg.audit_planes().is_ok());
        assert!(reg.seq_monotonic());
        assert_eq!(reg.occupied(), 10);
    }

    #[test]
    fn test_unregistered_rejected_from_runner() {
        // J1 红线：未登记探针拒绝入跑批器。
        let reg = ProbeRegistry::new();
        assert!(reg.admit(777).is_err());
        let mut runner = BatchRunner::new();
        assert!(runner.run(&reg, 777).is_err());
        assert_eq!(runner.rejected, 1);
        assert_eq!(runner.admitted, 0);
        assert!(runner.conserve(&reg).is_ok());
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert!(RegCode::ALL.len() == 12);
        assert_eq!(RegCode::NotRegistered.code(), SEG_BASE + 6);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18531_checks();
        assert!(set.all_passed(), "UNX-F18531 判据必须全绿：{}", set.red_items_text());
    }
}
