//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18421 · 谎报矩阵总纲（三元组裁决表冻结） · 0x7E500 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18421-J1 (进程,API,维度)→值裁决表冻结 v1，键唯一性断言
//!       全过，重复键注入必红
//! 本模块结构清单：
//!   1. 冻结常量层：矩阵容量/键域闭集/判据轮数（推导注释写死）；
//!   2. 诊断码闭集 VmCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 键模型 LieKey：三元组（进程键号,API 族,查询维度），域守卫；
//!   4. 值模型 VerValue：四段版本打包 + 谎报类别标签；
//!   5. 三元组裁决表 VerdictMatrix：数组账本 + 冻结状态机
//!      （Editable→Frozen 单向，冻结后拒写）+ 键唯一性线性扫；
//!   6. 注入核：重复键/空键段/维度越界/API 越界/冻结后写入五类坏
//!      样本，逐类 10/10 断言被显式拒绝；
//!   7. 黄金全链 golden_run：16 正例入表→冻结→全部可查→注入全抓
//!      →双跑逐字段一致；
//!   8. run_unx18421_checks：14 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E500，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E500;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 裁决表容量（写死 16）：本批 10 卡按 8 个主档进程键 × 2 个备份槽
/// 规划，8+2×4=16 恰为 2 的幂，便于指纹对折；扩容须换版 v2。
pub const MATRIX_CAP: usize = 16;
/// API 族域上界（冻结 v1：12 族，与 unx18425 B01 入口清单勾稽）。
pub const API_FAMILY_MAX: u8 = 12;
/// 查询维度域上界（冻结 v1：6 维，与 unx18426 六维枚举勾稽）。
pub const QUERY_DIM_MAX: u8 = 6;
/// 进程键号 0 保留（0 号保留给"矩阵自身"，不得作为业务键段）。
pub const PROC_KEY_RESERVED: u32 = 0;
/// 判据轮数（写死 10：判据原文"注入必红"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 表版本号（冻结 v1）。
pub const MATRIX_VER_V1: u32 = 1;
/// 指纹折叠位宽（写死 12：16 槽 × 四段值域用 12 位折叠可分性实测够用）。
pub const FOLD_BITS: u32 = 12;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmCode {
    /// 键段非法：进程键号撞保留字
    KeyProcReserved,
    /// 键段非法：API 族越出 12 族闭集
    KeyApiOutOfRange,
    /// 键段非法：查询维度越出 6 维闭集
    KeyDimOutOfRange,
    /// 重复键：三元组已在表
    DupKey,
    /// 表满：16 槽用尽
    TableFull,
    /// 表冻结：v1 冻结后拒绝写入
    TableFrozen,
    /// 重复冻结：冻结状态机只能单向走一次
    DupFreeze,
    /// 查无此键：三元组不在表
    NoEntry,
    /// 键唯一性不变量被破坏（自检发现）
    UniquenessBroken,
    /// 表计数守恒破坏：入账数≠成功+拒绝
    ConservationBroken,
    /// 指纹不符：双跑或快照对比失败
    DigestMismatch,
    /// 冻结时点表空：不许冻结空表
    FreezeEmpty,
}

impl VmCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VmCode::KeyProcReserved => "键段非法：进程键号 0 为矩阵保留字",
            VmCode::KeyApiOutOfRange => "键段非法：API 族越出 12 族闭集",
            VmCode::KeyDimOutOfRange => "键段非法：查询维度越出 6 维闭集",
            VmCode::DupKey => "重复键：三元组已登记，v1 表键唯一",
            VmCode::TableFull => "表满：16 槽用尽，须升版扩容",
            VmCode::TableFrozen => "表冻结：v1 冻结后拒绝一切写入",
            VmCode::DupFreeze => "重复冻结：冻结单向，二次冻结拒",
            VmCode::NoEntry => "查无此键：三元组不在 v1 表",
            VmCode::UniquenessBroken => "键唯一性不变量破坏：自检必红",
            VmCode::ConservationBroken => "表计数守恒破坏：入账≠成功+拒绝",
            VmCode::DigestMismatch => "指纹不符：双跑或快照对比失败",
            VmCode::FreezeEmpty => "冻结时点表空：空表不许冻结",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [VmCode; 12] = [
        VmCode::KeyProcReserved,
        VmCode::KeyApiOutOfRange,
        VmCode::KeyDimOutOfRange,
        VmCode::DupKey,
        VmCode::TableFull,
        VmCode::TableFrozen,
        VmCode::DupFreeze,
        VmCode::NoEntry,
        VmCode::UniquenessBroken,
        VmCode::ConservationBroken,
        VmCode::DigestMismatch,
        VmCode::FreezeEmpty,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < VmCode::ALL.len() {
        if let Some(c) = VmCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E500 段闭集内"
}

// ── 键模型 ─────────────────────────────────────────────────────────
/// 三元组键（进程键号, API 族, 查询维度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LieKey {
    /// 进程键号（由 unx18424 双因子绑定派生，非 0）。
    pub proc_key: u32,
    /// API 族号（0..12，闭集见 unx18425）。
    pub api_family: u8,
    /// 查询维度号（0..6，闭集见 unx18426）。
    pub query_dim: u8,
}

impl LieKey {
    /// 域守卫：三段全部落闭集才算合法键。
    pub fn legal(&self) -> bool {
        self.proc_key != PROC_KEY_RESERVED
            && self.proc_key <= 0x00FF_FFFF
            && self.api_family < API_FAMILY_MAX
            && self.query_dim < QUERY_DIM_MAX
    }
    /// 键指纹：三段折叠成 24 位（进程段低 16 位 ×API×维度），仅用于
    /// 快速预筛，唯一性仍以全等比较为准（指纹可碰撞，判定不可）。
    pub fn fingerprint(&self) -> u32 {
        ((self.proc_key & 0xFFFF) << 8) | ((self.api_family as u32) << 4) | (self.query_dim as u32)
    }
}

/// 构造键（ Result 化，不在构造层埋 panic）。
pub fn make_key(proc_key: u32, api_family: u8, query_dim: u8) -> Result<LieKey, u32> {
    let k = LieKey { proc_key, api_family, query_dim };
    if k.legal() {
        Ok(k)
    } else if proc_key == PROC_KEY_RESERVED {
        Err(VmCode::KeyProcReserved.code())
    } else if api_family >= API_FAMILY_MAX {
        Err(VmCode::KeyApiOutOfRange.code())
    } else {
        Err(VmCode::KeyDimOutOfRange.code())
    }
}

// ── 值模型 ─────────────────────────────────────────────────────────
/// 裁决值：四段版本打包 + 谎报类别标签。
/// 打包布局（写死）：[63:48] major | [47:32] minor | [31:16] build | [15:0] suite。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerValue {
    /// 打包版本值。
    pub packed: u64,
    /// 谎报类别：0=如实 1=降版兼容 2=升版伪装 3=族级替身。
    pub lie_class: u8,
}

impl VerValue {
    /// 打包构造（四段各 16 位域守卫）。
    pub fn pack(major: u16, minor: u16, build: u16, suite: u16, lie_class: u8) -> VerValue {
        let packed = ((major as u64) << 48)
            | ((minor as u64) << 32)
            | ((build as u64) << 16)
            | (suite as u64);
        VerValue { packed, lie_class: lie_class & 0x03 }
    }
    /// 拆包（返回四段，元组而非越界下标）。
    pub fn unpack(&self) -> (u16, u16, u16, u16) {
        (
            ((self.packed >> 48) & 0xFFFF) as u16,
            ((self.packed >> 32) & 0xFFFF) as u16,
            ((self.packed >> 16) & 0xFFFF) as u16,
            (self.packed & 0xFFFF) as u16,
        )
    }
    /// 类别合法（2 位域闭集内）。
    pub fn class_legal(&self) -> bool {
        self.lie_class <= 3
    }
}

// ── 表条目 ─────────────────────────────────────────────────────────
/// 单条裁决（键+值+登记序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerdictEntry {
    pub key: LieKey,
    pub value: VerValue,
    /// 登记序号（单调递增，注入审计用）。
    pub seq: u32,
}

// ── 三元组裁决表 ───────────────────────────────────────────────────
/// 表状态机：Editable（可写）→ Frozen（冻结 v1，只读）。单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixPhase {
    Editable,
    Frozen,
}

/// 三元组裁决表 v1（数组账本 + 键唯一性 + 冻结闸）。
pub struct VerdictMatrix {
    /// 账本主体（Vec 承载，容量 MATRIX_CAP 硬闸）。
    slots: Vec<VerdictEntry>,
    phase: MatrixPhase,
    /// 冻结版本号（冻结时写死 MATRIX_VER_V1）。
    frozen_ver: u32,
    /// 入账审计：尝试/成功/拒绝（守恒不变量）。
    pub tried: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 冻结时点快照指纹（FNV-1a 折叠，见 digest()）。
    frozen_digest: u64,
}

impl VerdictMatrix {
    /// 新表：Editable 相，空账本，审计归零。
    pub fn new() -> VerdictMatrix {
        VerdictMatrix {
            slots: Vec::new(),
            phase: MatrixPhase::Editable,
            frozen_ver: 0,
            tried: 0,
            accepted: 0,
            rejected: 0,
            frozen_digest: 0,
        }
    }

    /// 当前相（只读探视）。
    pub fn phase(&self) -> MatrixPhase {
        self.phase
    }

    /// 已用槽位（恒 ≤ MATRIX_CAP）。
    pub fn used(&self) -> usize {
        self.slots.len()
    }

    /// 键查表：线性全等扫（16 槽线性扫为冻结 v1 的可审计代价）。
    pub fn lookup(&self, key: &LieKey) -> Result<VerValue, u32> {
        if !key.legal() {
            return Err(VmCode::KeyProcReserved.code());
        }
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = self.slots.get(i) {
                if e.key == *key {
                    return Ok(e.value);
                }
            }
            i += 1;
        }
        Err(VmCode::NoEntry.code())
    }

    /// 重复键预检：返回 true 表示键已存在（插入路径复用）。
    fn key_exists(&self, key: &LieKey) -> bool {
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = self.slots.get(i) {
                if e.key == *key {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 登记一条裁决：五闸依次——键域/冻结/容量/重复键/入账守恒。
    /// 闸序写死：键域闸最先（坏键不污染审计），冻结闸其次，
    /// 容量闸再次，重复键闸最后（重复必红是 J1 核心）。
    pub fn insert(&mut self, key: LieKey, value: VerValue) -> Result<(), u32> {
        self.tried = self.tried.saturating_add(1);
        if !key.legal() {
            self.rejected = self.rejected.saturating_add(1);
            if key.proc_key == PROC_KEY_RESERVED {
                return Err(VmCode::KeyProcReserved.code());
            }
            if key.api_family >= API_FAMILY_MAX {
                return Err(VmCode::KeyApiOutOfRange.code());
            }
            return Err(VmCode::KeyDimOutOfRange.code());
        }
        if self.phase == MatrixPhase::Frozen {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VmCode::TableFrozen.code());
        }
        if self.slots.len() >= MATRIX_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VmCode::TableFull.code());
        }
        if self.key_exists(&key) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VmCode::DupKey.code());
        }
        let seq = self.accepted;
        self.slots.push(VerdictEntry { key, value, seq });
        self.accepted = self.accepted.saturating_add(1);
        Ok(())
    }

    /// 冻结：单向状态转换，空表拒冻结，二次冻结拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.phase == MatrixPhase::Frozen {
            return Err(VmCode::DupFreeze.code());
        }
        if self.slots.is_empty() {
            return Err(VmCode::FreezeEmpty.code());
        }
        self.frozen_digest = self.digest();
        self.frozen_ver = MATRIX_VER_V1;
        self.phase = MatrixPhase::Frozen;
        Ok(())
    }

    /// 冻结版本号（未冻结为 0）。
    pub fn ver(&self) -> u32 {
        self.frozen_ver
    }

    /// 冻结时点快照指纹（未冻结为 0）。
    pub fn frozen_digest(&self) -> u64 {
        self.frozen_digest
    }

    /// 全表 FNV-1a 指纹（键指纹+值打包+序号逐条折叠；确定性，无墙钟）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = self.slots.get(i) {
                let fp = e.key.fingerprint() as u64;
                h ^= fp;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
                h ^= e.value.packed;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
                h ^= e.seq as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            i += 1;
        }
        h
    }

    /// 键唯一性自检：两两全等比较，任何相等即不变量破坏。
    pub fn uniqueness_ok(&self) -> bool {
        let n = self.slots.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                let (a, b) = match (self.slots.get(i), self.slots.get(j)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return false,
                };
                if a.key == b.key {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 计数守恒：尝试 = 成功 + 拒绝。
    pub fn conserve_ok(&self) -> bool {
        self.tried == self.accepted.saturating_add(self.rejected)
    }

    /// 导出键列表（只读副本，供对账）。
    pub fn keys(&self) -> Vec<LieKey> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = self.slots.get(i) {
                v.push(e.key);
            }
            i += 1;
        }
        v
    }
}

// ── 正例语料：16 条主档裁决（确定性派生，冻结 v1 内容） ────────────
/// 主档进程键号（8 个：1..=8，由卡 A03 规划，0 保留）。
pub const MAIN_PROC_KEYS: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
/// 语料 API 族基线（写死：0=GetVersion 族，3=RtlGetVersion 族，5=VerifyVersion 族）。
pub const CORPUS_API_BASE: [u8; 3] = [0, 3, 5];
/// 语料维度基线（写死：0=主版本，1=次版本，2=Build）。
pub const CORPUS_DIM_BASE: [u8; 3] = [0, 1, 2];

/// 第 idx 条正例键（idx<16；键空间 8 进程 × 2 维度基线，确定性）。
pub fn corpus_key(idx: usize) -> Result<LieKey, u32> {
    let proc = MAIN_PROC_KEYS.get(idx % 8).copied().unwrap_or(1);
    let api = CORPUS_API_BASE.get(idx % 3).copied().unwrap_or(0);
    // 维度：偶数条落 0（主版本），奇数条落 2（Build）——避开同键重复。
    let dim: u8 = if idx % 2 == 0 { CORPUS_DIM_BASE.get(0).copied().unwrap_or(0) } else { CORPUS_DIM_BASE.get(2).copied().unwrap_or(2) };
    make_key(proc, api, dim)
}

/// 第 idx 条正例值（确定性：major=10，minor=idx%2，build=10240+idx*16）。
pub fn corpus_value(idx: usize) -> VerValue {
    let minor = (idx % 2) as u16;
    let build = 10240u16.saturating_add((idx as u16).wrapping_mul(16));
    // 类别：偶数条如实(0)，奇数条降版兼容(1)——覆盖两个类别域。
    let class = if idx % 2 == 0 { 0 } else { 1 };
    VerValue::pack(10, minor, build, 0, class)
}

// ── 注入核：五类坏样本 ─────────────────────────────────────────────
/// 坏样本类别（J1"重复键注入必红"+ 键域闸四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：重复键（先登记合法键再重放）。
    DupKeyInject,
    /// 类1：进程键号撞保留字 0。
    ReservedProcInject,
    /// 类2：查询维度越界（≥6）。
    DimOverflowInject,
    /// 类3：API 族越界（≥12）。
    ApiOverflowInject,
    /// 类4：冻结后写入。
    FrozenWriteInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::DupKeyInject => 0,
            BadClass::ReservedProcInject => 1,
            BadClass::DimOverflowInject => 2,
            BadClass::ApiOverflowInject => 3,
            BadClass::FrozenWriteInject => 4,
        }
    }
}

pub const BAD_CLASSES: usize = 5;

/// 注入检出账（tries/caught 平行账，守恒可复算）。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注：slot 越出类域静默忽略（账本自身不许被注入打穿）。
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
    /// 全槽全抓判定：每槽至少一试且 caught==tries。
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

/// 注入一轮（对全新表做五类注入各一次，返回逐类是否被拒）。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：重复键——合法键登记两次，第二次必 DupKey。
    let mut m = VerdictMatrix::new();
    if let Ok(k) = corpus_key(0) {
        let first = m.insert(k, corpus_value(0));
        let second = m.insert(k, corpus_value(1));
        if let Some(h) = hits.get_mut(BadClass::DupKeyInject.slot()) {
        *h = first.is_ok() && second == Err(VmCode::DupKey.code());
    }
    }
    // 类1：保留字进程键。
    if let Some(h) = hits.get_mut(BadClass::ReservedProcInject.slot()) {
        *h = make_key(PROC_KEY_RESERVED, 0, 0).is_err() && VerdictMatrix::new() .insert(LieKey { proc_key: 0, api_family: 0, query_dim: 0 }, corpus_value(0)) == Err(VmCode::KeyProcReserved.code());
    }
    // 类2：维度越界。
    if let Some(h) = hits.get_mut(BadClass::DimOverflowInject.slot()) {
        *h = VerdictMatrix::new() .insert(LieKey { proc_key: 1, api_family: 0, query_dim: QUERY_DIM_MAX }, corpus_value(0)) == Err(VmCode::KeyDimOutOfRange.code());
    }
    // 类3：API 族越界。
    if let Some(h) = hits.get_mut(BadClass::ApiOverflowInject.slot()) {
        *h = VerdictMatrix::new() .insert(LieKey { proc_key: 1, api_family: API_FAMILY_MAX, query_dim: 0 }, corpus_value(0)) == Err(VmCode::KeyApiOutOfRange.code());
    }
    // 类4：冻结后写入。
    let mut f = VerdictMatrix::new();
    if let Ok(k) = corpus_key(0) {
        let _ = f.insert(k, corpus_value(0));
        let fr = f.freeze();
        let post = if let Ok(k2) = corpus_key(1) {
            f.insert(k2, corpus_value(1))
        } else {
            Ok(())
        };
        if let Some(h) = hits.get_mut(BadClass::FrozenWriteInject.slot()) {
        *h = fr.is_ok() && post == Err(VmCode::TableFrozen.code());
    }
    }
    hits
}

/// 注入 rounds 轮（判据 10/10 语义：每类 rounds 全抓）。
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
/// 黄金全链：16 正例入表→唯一性→冻结→全查→冻结拒写→守恒→指纹。
/// 返回（accepted, rejected, frozen_ver, digest）。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut m = VerdictMatrix::new();
    let mut idx = 0;
    while idx < MATRIX_CAP {
        let k = corpus_key(idx)?;
        let v = corpus_value(idx);
        m.insert(k, v)?;
        idx += 1;
    }
    if !m.uniqueness_ok() {
        return Err(VmCode::UniquenessBroken.code());
    }
    if !m.conserve_ok() || m.accepted != MATRIX_CAP as u32 {
        return Err(VmCode::ConservationBroken.code());
    }
    m.freeze()?;
    // 冻结后 16 条全查命中且值逐位一致。
    let mut i = 0;
    while i < MATRIX_CAP {
        let k = corpus_key(i)?;
        let v = m.lookup(&k)?;
        if v != corpus_value(i) {
            return Err(VmCode::DigestMismatch.code());
        }
        i += 1;
    }
    // 冻结后写入必拒（TableFrozen）。
    let k = corpus_key(0)?;
    if m.insert(k, corpus_value(0)) != Err(VmCode::TableFrozen.code()) {
        return Err(VmCode::TableFrozen.code());
    }
    // 表满后再入必拒（先解一张新表验证容量闸）。
    let mut full = VerdictMatrix::new();
    let mut j = 0;
    while j < MATRIX_CAP {
        let kk = corpus_key(j)?;
        let _ = full.insert(kk, corpus_value(j));
        j += 1;
    }
    // 16 槽满后换键再入 → TableFull。
    if let Ok(extra) = make_key(9, 0, 0) {
        if full.insert(extra, corpus_value(0)) != Err(VmCode::TableFull.code()) {
            return Err(VmCode::TableFull.code());
        }
    }
    let d = m.digest();
    Ok((m.accepted, m.rejected, m.ver(), d))
}

/// 双跑确定性：两次黄金全链逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VmCode::DigestMismatch.code());
    }
    Ok(())
}

// ── 探针三序（快/中/慢三条最小路径） ───────────────────────────────
/// 探针A：单条正例登记后立即命中。
pub fn probe_a() -> Result<(), u32> {
    let mut m = VerdictMatrix::new();
    let k = corpus_key(0)?;
    m.insert(k, corpus_value(0))?;
    let v = m.lookup(&k)?;
    if v == corpus_value(0) {
        Ok(())
    } else {
        Err(VmCode::DigestMismatch.code())
    }
}

/// 探针B：查无此键返回 NoEntry（不 panic、不误报命中）。
pub fn probe_b() -> Result<(), u32> {
    let m = VerdictMatrix::new();
    let k = make_key(7, 5, 3)?;
    match m.lookup(&k) {
        Err(c) if c == VmCode::NoEntry.code() => Ok(()),
        _ => Err(VmCode::NoEntry.code()),
    }
}

/// 探针C：空表拒冻结（FreezeEmpty 闸）。
pub fn probe_c() -> Result<(), u32> {
    let mut m = VerdictMatrix::new();
    match m.freeze() {
        Err(c) if c == VmCode::FreezeEmpty.code() => Ok(()),
        _ => Err(VmCode::FreezeEmpty.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < VmCode::ALL.len() {
        if let Some(c) = VmCode::ALL.get(i) {
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
/// run_unx18421_checks：J1 判据 14 条（断言性质，动态明细 ≤8）。
pub fn run_unx18421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18421 谎报矩阵总纲（三元组裁决表冻结）");
    // J1-1 冻结 v1：表容量/版本/键域三常量与判据原文一致。
    s.add(
        "J1-冻结v1常量",
        MATRIX_CAP == 16 && MATRIX_VER_V1 == 1 && API_FAMILY_MAX == 12 && QUERY_DIM_MAX == 6,
        "容量16 版本1 API族12 维度6",
    );
    // J1-2 黄金全链：16 正例入表→冻结→全查。
    let g = golden_run();
    let g_ok = match &g {
        Ok((acc, _, ver, _)) => *acc == MATRIX_CAP as u32 && *ver == MATRIX_VER_V1,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链16正例", g_ok, &format!("{:?}", g));
    // J1-3 键唯一性断言全过（正例表两两比较）。
    let mut m = VerdictMatrix::new();
    let mut i = 0;
    while i < MATRIX_CAP {
        if let (Ok(k), ok) = (corpus_key(i), i < MATRIX_CAP) {
            let _ = ok;
            let _ = m.insert(k, corpus_value(i));
        }
        i += 1;
    }
    s.add("J1-键唯一性全过", m.uniqueness_ok(), "16键两两全等互异");
    // J1-4 重复键注入必红：10 轮注入账全槽全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-重复键注入必红10/10",
        led.all_caught() && led.missed() == 0,
        &format!("tries={:?} caught={:?}", led.tries, led.caught),
    );
    // J1-5 注入计数守恒：漏检=0 且 tries=10×5。
    let total_tries = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入计数守恒",
        led.missed() == 0 && total_tries == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入50",
    );
    // J1-6 冻结闸：冻结后写入必拒（TableFrozen）。
    let mut fm = VerdictMatrix::new();
    if let Ok(k) = corpus_key(0) {
        let _ = fm.insert(k, corpus_value(0));
    }
    let frozen_ok = match (fm.freeze(), corpus_key(1)) {
        (Ok(()), Ok(k2)) => fm.insert(k2, corpus_value(1)) == Err(VmCode::TableFrozen.code()),
        _ => false,
    };
    s.add("J1-冻结后拒写", frozen_ok, "Frozen→insert 必 TableFrozen");
    // J1-7 容量闸：16 槽满后第 17 入必 TableFull。
    let mut full = VerdictMatrix::new();
    let mut j = 0;
    while j < MATRIX_CAP {
        if let Ok(k) = corpus_key(j) {
            let _ = full.insert(k, corpus_value(j));
        }
        j += 1;
    }
    let full_ok = match make_key(9, 0, 0) {
        Ok(k) => full.insert(k, corpus_value(0)) == Err(VmCode::TableFull.code()),
        Err(_) => false,
    };
    s.add("J1-容量闸16槽", full_ok, "表满必 TableFull 且账不丢");
    // J1-8 键域闸三段：保留字/维度越界/API 越界逐闸拒。
    let gate = make_key(0, 0, 0).is_err()
        && make_key(1, API_FAMILY_MAX, 0).is_err()
        && make_key(1, 0, QUERY_DIM_MAX).is_err();
    s.add("J1-键域三闸", gate, "保留字/API越界/维度越界全拒");
    // J1-9 守恒不变量：尝试=成功+拒绝。
    s.add("J1-计数守恒", m.conserve_ok(), "tried=accepted+rejected");
    // J1-10 探针三序：命中/查无/空表拒冻结。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "命中/NoEntry/FreezeEmpty");
    // J1-11 双跑一致：黄金全链两次逐字段相等。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-12 指纹稳定：同表两次 digest 相等，异表不等。
    let mut m2 = VerdictMatrix::new();
    if let Ok(k) = corpus_key(0) {
        let _ = m2.insert(k, corpus_value(0));
    }
    let dg_ok = m.digest() == m.digest() && m.digest() != m2.digest();
    s.add("J2-指纹稳定", dg_ok, "FNV 折叠确定性且可区分");
    // J1-13 值打包往返：四段 pack→unpack 往返无损。
    let vv = VerValue::pack(10, 0, 10240, 256, 1);
    s.add(
        "J2-值打包往返",
        vv.unpack() == (10, 0, 10240, 256) && vv.class_legal(),
        "pack/unpack 逐段还原",
    );
    // J1-14 段位守卫：闭集落 0x7E500 窗口。
    s.add("J3-段位守卫", segment_guard(), "0x7E500 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18421_checks();
        assert!(set.all_passed(), "UNX-F18421 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_corpus_insert_and_lookup() {
        let mut m = VerdictMatrix::new();
        for i in 0..MATRIX_CAP {
            let k = corpus_key(i).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 0 });
            assert!(m.insert(k, corpus_value(i)).is_ok());
        }
        for i in 0..MATRIX_CAP {
            let k = corpus_key(i).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 0 });
            assert_eq!(m.lookup(&k), Ok(corpus_value(i)));
        }
    }

    #[test]
    fn t_duplicate_key_rejected() {
        let mut m = VerdictMatrix::new();
        let k = corpus_key(0).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 0 });
        assert!(m.insert(k, corpus_value(0)).is_ok());
        assert_eq!(m.insert(k, corpus_value(1)), Err(VmCode::DupKey.code()));
    }

    #[test]
    fn t_key_domain_gates() {
        assert_eq!(make_key(0, 0, 0), Err(VmCode::KeyProcReserved.code()));
        assert_eq!(make_key(1, API_FAMILY_MAX, 0), Err(VmCode::KeyApiOutOfRange.code()));
        assert_eq!(make_key(1, 0, QUERY_DIM_MAX), Err(VmCode::KeyDimOutOfRange.code()));
        assert!(make_key(1, 11, 5).is_ok());
    }

    #[test]
    fn t_freeze_and_frozen_write() {
        let mut m = VerdictMatrix::new();
        let k0 = corpus_key(0).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 0 });
        let _ = m.insert(k0, corpus_value(0));
        assert!(m.freeze().is_ok());
        assert_eq!(m.freeze(), Err(VmCode::DupFreeze.code()));
        let k1 = corpus_key(1).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 2 });
        assert_eq!(m.insert(k1, corpus_value(1)), Err(VmCode::TableFrozen.code()));
        assert_eq!(m.ver(), MATRIX_VER_V1);
    }

    #[test]
    fn t_capacity_full() {
        let mut m = VerdictMatrix::new();
        for i in 0..MATRIX_CAP {
            let k = corpus_key(i).unwrap_or(LieKey { proc_key: 1, api_family: 0, query_dim: 0 });
            assert!(m.insert(k, corpus_value(i)).is_ok());
        }
        let k = make_key(9, 0, 0).unwrap_or(LieKey { proc_key: 9, api_family: 0, query_dim: 0 });
        assert_eq!(m.insert(k, corpus_value(0)), Err(VmCode::TableFull.code()));
        assert!(m.conserve_ok());
    }

    #[test]
    fn t_inject_ledger_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..BAD_CLASSES {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
            assert_eq!(led.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probe_three() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
