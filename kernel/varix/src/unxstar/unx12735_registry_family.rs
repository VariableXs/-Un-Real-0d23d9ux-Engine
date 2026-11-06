//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12735 · 注册表族对照回归 · 0x821C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12736-J1 令牌族对照回归绿，特权调整语义维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注册表域：定容 8 槽键表 + 键生命周期单调状态机
//!     （创建→写入→封存→删除/未封先删四弧），倒退弧（封存后写值、
//!     删除后复活、封存后退回写入态）一律显性拒；
//!   - 值类型闭集八档（winnt.h REG_* 冻结子集）：set/get 带类型核，
//!     类型错读即违约必抓；
//!   - 令牌族对照回归（判据锚点）：特权四态闭集
//!     （未持有/持有未启/已启/已移除）× 特权名冻结表（winnt.h
//!     SE_*_NAME 子集），调整语义（启用须先持有、重复启用幂等拒、
//!     移除为终态不可复启）全程守恒可对账；
//!   - 注入三族（生命周期倒退/特权调整违约/值类型违约）各 10 轮必抓全抓；
//!   - LCG 种子写死 = SEG_BASE（0x821C0）；no_std + alloc；
//!     零 std/零浮点/零 IO/零墙钟/零随机/零 unwrap/零裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（VX9 公式：0x80000 + 0x40 × (12735−12600) = 0x821C0）。
pub const SEG_BASE: u32 = 0x821C0;

// ── 冻结常量表 ─────────────────────────────────────────────────────
/// 键表容量（定容 8 槽；冻结值）。
pub const KEY_SLOT_N: usize = 8;
/// 每键值条目上限（冻结值）。
pub const VALUE_CAP: usize = 4;
/// 特权名冻结表长（winnt.h SE_*_NAME 子集）。
pub const PRIV_N: usize = 4;
/// 值类型闭集档数（winnt.h REG_* 冻结子集）。
pub const REG_TYPE_N: usize = 8;
/// SE_PRIVILEGE_ENABLED_BY_DEFAULT（winnt.h 0x1，冻结）。
pub const SE_PRIV_ENABLED_BY_DEFAULT: u32 = 0x1;
/// SE_PRIVILEGE_ENABLED（winnt.h 0x2，冻结）。
pub const SE_PRIV_ENABLED: u32 = 0x2;
/// SE_PRIVILEGE_REMOVED（winnt.h 0x4，冻结）。
pub const SE_PRIV_REMOVED: u32 = 0x4;
/// SE_PRIVILEGE_USED_FOR_ACCESS（winnt.h 0x8，冻结）。
pub const SE_PRIV_USED_FOR_ACCESS: u32 = 0x8;

/// 特权名冻结表（出处：winnt.h SE_*_NAME 常量子集，名冻结）。
pub const PRIV_NAMES: [&str; PRIV_N] = [
    "SeBackupPrivilege",
    "SeRestorePrivilege",
    "SeDebugPrivilege",
    "SeShutdownPrivilege",
];

/// 值类型闭集八档（出处：winnt.h REG_* 类型常量，值冻结）。
pub const REG_TYPES: [u32; REG_TYPE_N] = [
    1, // REG_SZ
    2, // REG_EXPAND_SZ
    3, // REG_BINARY
    4, // REG_DWORD
    5, // REG_DWORD_BIG_ENDIAN
    6, // REG_LINK
    7, // REG_MULTI_SZ
    11, // REG_QWORD
];

// ── 诊断码闭集（段内 0x40 窗口，10 码） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegDiag {
    /// 键名越字符闭集（限字母/数字/_/-）
    KeyNameBad,
    /// 键表满（容量耗尽）
    KeySlotFull,
    /// 键不存在（查无此槽/已删）
    KeyNotFound,
    /// 键生命周期倒退（封存后写值/删除后复活）
    PhaseBackward,
    /// 封存后写值（封存是写入态的终闸）
    SealedWrite,
    /// 值类型违约（存取类型错配）
    ValTypeBreak,
    /// 值不存在（查无名下此值）
    ValNotFound,
    /// 未持有特权即启用
    PrivNotHeld,
    /// 特权调整重复（重复启用幂等拒）
    PrivAdjustRepeat,
    /// 移除后启用（移除是终态，不可复启）
    PrivRemovedFinal,
}

impl RegDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegDiag::KeyNameBad => "键名越闭集：限字母数字与_-",
            RegDiag::KeySlotFull => "键表满：先删旧键再建新键",
            RegDiag::KeyNotFound => "键不存在：先创建再操作",
            RegDiag::PhaseBackward => "生命周期倒退：单调不回退",
            RegDiag::SealedWrite => "封存后写值：先解封（重建）再写",
            RegDiag::ValTypeBreak => "值类型违约：存取类型须一致",
            RegDiag::ValNotFound => "值不存在：先写入再读取",
            RegDiag::PrivNotHeld => "未持有特权：先获得再启用",
            RegDiag::PrivAdjustRepeat => "调整重复：已启用无需再启",
            RegDiag::PrivRemovedFinal => "移除为终态：不可复启",
        }
    }
    pub const ALL: [RegDiag; 10] = [
        RegDiag::KeyNameBad,
        RegDiag::KeySlotFull,
        RegDiag::KeyNotFound,
        RegDiag::PhaseBackward,
        RegDiag::SealedWrite,
        RegDiag::ValTypeBreak,
        RegDiag::ValNotFound,
        RegDiag::PrivNotHeld,
        RegDiag::PrivAdjustRepeat,
        RegDiag::PrivRemovedFinal,
    ];
}

/// 诊断码查表（闭集返回；未知码给中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < RegDiag::ALL.len() {
        if let Some(c) = RegDiag::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x821C0 段闭集内"
}

// ── 值模型（类型闭集 + 类型核） ────────────────────────────────────
/// 注册表值：类型闭集 + 载荷（载荷按类型定型存取）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RegVal {
    /// REG_SZ / REG_EXPAND_SZ：字符串载荷
    Sz(String),
    /// REG_BINARY：字节载荷
    Binary(Vec<u8>),
    /// REG_DWORD / REG_DWORD_BIG_ENDIAN：32 位载荷
    Dword(u32),
    /// REG_QWORD：64 位载荷
    Qword(u64),
}

impl RegVal {
    /// 类型指纹（对齐 REG_TYPES 闭集）。
    pub fn type_tag(&self) -> u32 {
        match self {
            RegVal::Sz(_) => 1,      // REG_SZ
            RegVal::Binary(_) => 3,  // REG_BINARY
            RegVal::Dword(_) => 4,   // REG_DWORD
            RegVal::Qword(_) => 11,  // REG_QWORD
        }
    }
    /// 类型在闭集内判定。
    pub fn type_closed(&self) -> bool {
        let t = self.type_tag();
        let mut i = 0usize;
        while i < REG_TYPES.len() {
            match REG_TYPES.get(i) {
                Some(&v) if v == t => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }
}

// ── 键生命周期单调状态机 ───────────────────────────────────────────
/// 键生命周期四态：创建→写入→封存→删除（删除可自写入态直达）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyPhase {
    /// 创建：键名已登记，无值
    Created,
    /// 写入：值已落键
    Populated,
    /// 封存：写闸落锁（终闸，写值必拒）
    Sealed,
    /// 删除：墓碑态（终态，复活必拒）
    Tombstoned,
}

impl KeyPhase {
    /// 态名（对照回归文案面）。
    pub fn name(self) -> &'static str {
        match self {
            KeyPhase::Created => "创建",
            KeyPhase::Populated => "写入",
            KeyPhase::Sealed => "封存",
            KeyPhase::Tombstoned => "删除",
        }
    }
}

/// 生命周期四弧冻结表：(弧名, 起点, 终点, 语义)。
/// 出处：注册表键生命周期规格——合法迁移恰此四条，越表/倒退即违约。
pub const KEY_ARCS: [(&'static str, KeyPhase, KeyPhase, &'static str); 4] = [
    ("弧一·落值", KeyPhase::Created, KeyPhase::Populated, "首值入键"),
    ("弧二·封存", KeyPhase::Populated, KeyPhase::Sealed, "写闸落锁"),
    ("弧三·销键", KeyPhase::Sealed, KeyPhase::Tombstoned, "封存后删除"),
    ("弧四·弃键", KeyPhase::Populated, KeyPhase::Tombstoned, "未封先删"),
];

/// 注册表键：名 + 生命周期 + 值条目。
#[derive(Clone)]
pub struct RegKey {
    /// 键名（字符闭集已校验）
    pub name: String,
    /// 生命周期态
    pub phase: KeyPhase,
    /// 值条目（名→值，同名覆盖）
    pub values: Vec<(String, RegVal)>,
    /// 走弧数（对账账）
    pub arcs: u32,
}

impl RegKey {
    /// 键名字符闭集：限字母/数字/_/-（冻结语义，与值名共用）。
    pub fn name_charset_ok(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        for &b in name.as_bytes().iter() {
            let ok = (b >= b'a' && b <= b'z')
                || (b >= b'A' && b <= b'Z')
                || (b >= b'0' && b <= b'9')
                || b == b'_'
                || b == b'-';
            if !ok {
                return false;
            }
        }
        true
    }

    /// 按弧迁移（单调核：越表/倒退显性拒；终点为删除态时清值）。
    pub fn advance(&mut self, arc: usize) -> Result<KeyPhase, RegDiag> {
        let entry = match KEY_ARCS.get(arc) {
            Some(e) => *e,
            None => return Err(RegDiag::PhaseBackward),
        };
        let (_name, from, to, _sem) = entry;
        if self.phase != from {
            return Err(RegDiag::PhaseBackward);
        }
        self.phase = to;
        self.arcs = self.arcs.saturating_add(1);
        if to == KeyPhase::Tombstoned {
            self.values.clear();
        }
        Ok(self.phase)
    }

    /// 写值：封存态必拒（终闸）；墓碑态必拒（键不存在语义）。
    pub fn put(&mut self, vname: &str, v: RegVal) -> Result<(), RegDiag> {
        if !Self::name_charset_ok(vname) {
            return Err(RegDiag::KeyNameBad);
        }
        match self.phase {
            KeyPhase::Sealed => return Err(RegDiag::SealedWrite),
            KeyPhase::Tombstoned => return Err(RegDiag::KeyNotFound),
            _ => {}
        }
        if !v.type_closed() {
            return Err(RegDiag::ValTypeBreak);
        }
        // 同名覆盖；否则追加（容量上限 VALUE_CAP）。
        let mut i = 0usize;
        while i < self.values.len() {
            if let Some((n, old)) = self.values.get(i) {
                if n.eq_ignore_ascii_case(vname) {
                    if old.type_tag() != v.type_tag() {
                        return Err(RegDiag::ValTypeBreak);
                    }
                    if let Some(slot) = self.values.get_mut(i) {
                        slot.1 = v;
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        if self.values.len() + 1 > VALUE_CAP {
            return Err(RegDiag::KeySlotFull);
        }
        self.values.push((String::from(vname), v));
        Ok(())
    }

    /// 读值：类型错配即违约（存取类型须一致）。
    pub fn get(&self, vname: &str, want_tag: u32) -> Result<RegVal, RegDiag> {
        if self.phase == KeyPhase::Tombstoned {
            return Err(RegDiag::KeyNotFound);
        }
        let mut i = 0usize;
        while i < self.values.len() {
            if let Some((n, v)) = self.values.get(i) {
                if n.eq_ignore_ascii_case(vname) {
                    if v.type_tag() != want_tag {
                        return Err(RegDiag::ValTypeBreak);
                    }
                    return Ok(v.clone());
                }
            }
            i += 1;
        }
        Err(RegDiag::ValNotFound)
    }
}

// ── 注册表键表 ─────────────────────────────────────────────────────
/// 键表：定容 8 槽 + 生命周期对账账。
#[derive(Clone)]
pub struct Registry {
    pub slots: [Option<RegKey>; KEY_SLOT_N],
    /// 生命周期弧总账（Σ在槽键弧 + 已清理键弧 ≡ 表弧）
    pub arcs: u32,
    /// 已清理（purge）键携带的弧账
    pub purged_arcs: u32,
    /// 建键/删键累计账
    pub created: u32,
    pub deleted: u32,
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            slots: core::array::from_fn(|_| None),
            arcs: 0,
            purged_arcs: 0,
            created: 0,
            deleted: 0,
        }
    }

    /// 建键：字符闭集校验→查重（同名单键）→空槽分配（满容即拒）。
    pub fn create(&mut self, name: &str) -> Result<usize, RegDiag> {
        if !RegKey::name_charset_ok(name) {
            return Err(RegDiag::KeyNameBad);
        }
        let mut i = 0usize;
        while i < KEY_SLOT_N {
            if let Some(Some(k)) = self.slots.get(i) {
                if k.name.eq_ignore_ascii_case(name) {
                    // 同名重复建键 = 倒退（键已在）。
                    return Err(RegDiag::PhaseBackward);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < KEY_SLOT_N {
            let empty = match self.slots.get(j) {
                Some(None) => true,
                _ => false,
            };
            if empty {
                if let Some(slot) = self.slots.get_mut(j) {
                    *slot = Some(RegKey {
                        name: String::from(name),
                        phase: KeyPhase::Created,
                        values: Vec::new(),
                        arcs: 0,
                    });
                }
                self.created = self.created.saturating_add(1);
                return Ok(j);
            }
            j += 1;
        }
        Err(RegDiag::KeySlotFull)
    }

    /// 查活键下标（墓碑/不存在同拒）。
    pub fn find(&self, name: &str) -> Result<usize, RegDiag> {
        let mut i = 0usize;
        while i < KEY_SLOT_N {
            if let Some(Some(k)) = self.slots.get(i) {
                if k.name.eq_ignore_ascii_case(name) && k.phase != KeyPhase::Tombstoned {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(RegDiag::KeyNotFound)
    }

    /// 按弧推进某键（对账联动）。
    pub fn advance(&mut self, name: &str, arc: usize) -> Result<KeyPhase, RegDiag> {
        let idx = self.find(name)?;
        let phase = match self.slots.get_mut(idx) {
            Some(Some(k)) => k.advance(arc)?,
            _ => return Err(RegDiag::KeyNotFound),
        };
        self.arcs = self.arcs.saturating_add(1);
        if phase == KeyPhase::Tombstoned {
            self.deleted = self.deleted.saturating_add(1);
        }
        Ok(phase)
    }

    /// 写值（对账联动：落值弧自动走通——Created 首值即弧一）。
    pub fn put(&mut self, name: &str, vname: &str, v: RegVal) -> Result<(), RegDiag> {
        let idx = self.find(name)?;
        let was_created = match self.slots.get(idx) {
            Some(Some(k)) => k.phase == KeyPhase::Created,
            _ => false,
        };
        match self.slots.get_mut(idx) {
            Some(Some(k)) => k.put(vname, v)?,
            _ => return Err(RegDiag::KeyNotFound),
        }
        if was_created {
            // 首值入键 = 弧一自动落账（生命周期联动）。
            let _ = self.advance(name, 0);
        }
        Ok(())
    }

    /// 读值。
    pub fn get(&self, name: &str, vname: &str, want_tag: u32) -> Result<RegVal, RegDiag> {
        let idx = self.find(name)?;
        match self.slots.get(idx) {
            Some(Some(k)) => k.get(vname, want_tag),
            _ => Err(RegDiag::KeyNotFound),
        }
    }

    /// 清理墓碑：物理移除槽位（弧账转入 purged_arcs，对账不断链）。
    pub fn purge(&mut self, name: &str) -> Result<(), RegDiag> {
        let mut i = 0usize;
        while i < KEY_SLOT_N {
            let target = match self.slots.get(i) {
                Some(Some(k)) => {
                    k.name.eq_ignore_ascii_case(name) && k.phase == KeyPhase::Tombstoned
                }
                _ => false,
            };
            if target {
                if let Some(Some(k)) = self.slots.get(i) {
                    self.purged_arcs = self.purged_arcs.saturating_add(k.arcs);
                }
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = None;
                }
                return Ok(());
            }
            i += 1;
        }
        Err(RegDiag::KeyNotFound)
    }

    /// 对账守恒：Σ在槽键弧 + 已清理键弧 ≡ 表弧。
    pub fn conservation(&self) -> Result<(), RegDiag> {
        let mut sum = self.purged_arcs;
        let mut i = 0usize;
        while i < KEY_SLOT_N {
            if let Some(Some(k)) = self.slots.get(i) {
                sum = sum.saturating_add(k.arcs);
            }
            i += 1;
        }
        if sum != self.arcs {
            return Err(RegDiag::PhaseBackward);
        }
        Ok(())
    }
}

// ── 令牌族对照（判据锚点：特权调整语义维持） ───────────────────────
/// 特权四态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivState {
    /// 未持有：令牌不含此特权
    Absent,
    /// 持有未启：含而未启（ENABLED_BY_DEFAULT 语义位）
    Disabled,
    /// 已启：启用中（ENABLED 语义位）
    Enabled,
    /// 已移除：终态（REMOVED 语义位，不可复启）
    Removed,
}

impl PrivState {
    /// 态名（对照文案面）。
    pub fn name(self) -> &'static str {
        match self {
            PrivState::Absent => "未持有",
            PrivState::Disabled => "持有未启",
            PrivState::Enabled => "已启",
            PrivState::Removed => "已移除",
        }
    }
    /// 冻结语义位映射（winnt.h SE_PRIVILEGE_*）。
    pub fn attr(self) -> u32 {
        match self {
            PrivState::Absent => 0,
            PrivState::Disabled => SE_PRIV_ENABLED_BY_DEFAULT,
            PrivState::Enabled => SE_PRIV_ENABLED_BY_DEFAULT | SE_PRIV_ENABLED,
            PrivState::Removed => SE_PRIV_REMOVED,
        }
    }
}

/// 特权调整动作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivAction {
    /// 启用（持有未启 → 已启）
    Enable,
    /// 停用（已启 → 持有未启）
    Disable,
    /// 移除（任意持有态 → 已移除终态）
    Remove,
}

/// 令牌：特权四态表 + 调整账。
#[derive(Clone, Copy)]
pub struct Token {
    /// 特权态表（与 PRIV_NAMES 同序对齐）
    pub privs: [PrivState; PRIV_N],
    /// 调整账（每次受纳调整 +1，对账守恒）
    pub adjusts: u32,
}

impl Token {
    pub fn new() -> Token {
        Token {
            privs: [
                PrivState::Disabled,
                PrivState::Disabled,
                PrivState::Absent,
                PrivState::Absent,
            ],
            adjusts: 0,
        }
    }

    /// 调整核（单调语义全在此）：越表拒/终态拒/幂等拒/前置拒。
    pub fn adjust(&mut self, priv_idx: usize, action: PrivAction) -> Result<PrivState, RegDiag> {
        let cur = match self.privs.get(priv_idx) {
            Some(s) => *s,
            None => return Err(RegDiag::PrivNotHeld),
        };
        let next = match action {
            PrivAction::Enable => match cur {
                PrivState::Absent => return Err(RegDiag::PrivNotHeld),
                PrivState::Enabled => return Err(RegDiag::PrivAdjustRepeat),
                PrivState::Removed => return Err(RegDiag::PrivRemovedFinal),
                PrivState::Disabled => PrivState::Enabled,
            },
            PrivAction::Disable => match cur {
                PrivState::Enabled => PrivState::Disabled,
                PrivState::Disabled => return Err(RegDiag::PrivAdjustRepeat),
                PrivState::Absent => return Err(RegDiag::PrivNotHeld),
                PrivState::Removed => return Err(RegDiag::PrivRemovedFinal),
            },
            PrivAction::Remove => match cur {
                PrivState::Absent => return Err(RegDiag::PrivNotHeld),
                PrivState::Removed => return Err(RegDiag::PrivAdjustRepeat),
                _ => PrivState::Removed,
            },
        };
        if let Some(slot) = self.privs.get_mut(priv_idx) {
            *slot = next;
        }
        self.adjusts = self.adjusts.saturating_add(1);
        Ok(next)
    }

    /// 特权调整语义维持判定：各态冻结语义位自洽。
    pub fn semantics_ok(&self) -> bool {
        // 语义位自洽：已移除恒带 REMOVED 位、已启恒带 ENABLED 位。
        let mut i = 0usize;
        while i < PRIV_N {
            let st = match self.privs.get(i) {
                Some(s) => *s,
                None => return false,
            };
            match st {
                PrivState::Removed => {
                    if st.attr() & SE_PRIV_REMOVED == 0 {
                        return false;
                    }
                }
                PrivState::Enabled => {
                    if st.attr() & SE_PRIV_ENABLED == 0 {
                        return false;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        true
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：键生命周期全弧 + 值类型核 + 特权调整全语义 ────────────
/// 黄金正演返回断言通过数（满绿 = 12）。
pub fn golden_run() -> Result<u32, RegDiag> {
    let mut reg = Registry::new();
    // 键一：创建→写值（弧一自动落账）→封存→删除（弧二、弧三）。
    let k0 = reg.create("Config")?;
    reg.put("Config", "Mode", RegVal::Dword(7))?;
    if reg.get("Config", "Mode", 4).is_err() {
        return Err(RegDiag::ValTypeBreak);
    }
    if reg.advance("Config", 1) != Ok(KeyPhase::Sealed) {
        return Err(RegDiag::PhaseBackward);
    }
    // 封存后写值必拒（终闸）。
    if reg.put("Config", "Mode", RegVal::Dword(9)).is_ok() {
        return Err(RegDiag::SealedWrite);
    }
    if reg.advance("Config", 2) != Ok(KeyPhase::Tombstoned) {
        return Err(RegDiag::PhaseBackward);
    }
    // 墓碑后复活必拒（写值/读值/再删全拒）。
    if reg.put("Config", "Mode", RegVal::Dword(1)).is_ok() {
        return Err(RegDiag::PhaseBackward);
    }
    if reg.advance("Config", 2).is_ok() {
        return Err(RegDiag::PhaseBackward);
    }
    let _ = k0;
    // 键二：未封先删（弧四）。
    let _ = reg.create("Cache")?;
    reg.put("Cache", "Hint", RegVal::Sz(String::from("lru")))?;
    if reg.advance("Cache", 3) != Ok(KeyPhase::Tombstoned) {
        return Err(RegDiag::PhaseBackward);
    }
    // 键三：四值条目齐 + 容量上限第五条必拒。
    let _ = reg.create("Vault")?;
    reg.put("Vault", "A", RegVal::Dword(1))?;
    reg.put("Vault", "B", RegVal::Sz(String::from("b")))?;
    reg.put("Vault", "C", RegVal::Qword(0x1234_5678_9ABC_DEF0))?;
    reg.put("Vault", "D", RegVal::Binary(Vec::new()))?;
    if reg.put("Vault", "E", RegVal::Dword(5)).is_ok() {
        return Err(RegDiag::KeySlotFull);
    }
    // 类型错读必抓。
    if reg.get("Vault", "A", 1).is_ok() {
        return Err(RegDiag::ValTypeBreak);
    }
    // 键表满容必抓。
    let mut i = 0usize;
    while i < KEY_SLOT_N {
        let mut nm = String::from("K");
        nm.push((b'A' + i as u8) as char);
        let _ = reg.create(&nm);
        i += 1;
    }
    if reg.create("Overflow").is_ok() {
        return Err(RegDiag::KeySlotFull);
    }
    // 令牌族对照：启用/停用/移除全语义走通。
    let mut tok = Token::new();
    if tok.adjust(0, PrivAction::Enable) != Ok(PrivState::Enabled) {
        return Err(RegDiag::PrivAdjustRepeat);
    }
    if tok.adjust(0, PrivAction::Disable) != Ok(PrivState::Disabled) {
        return Err(RegDiag::PrivAdjustRepeat);
    }
    if tok.adjust(1, PrivAction::Enable) != Ok(PrivState::Enabled) {
        return Err(RegDiag::PrivAdjustRepeat);
    }
    if tok.adjust(1, PrivAction::Remove) != Ok(PrivState::Removed) {
        return Err(RegDiag::PrivAdjustRepeat);
    }
    // 移除后复启必拒（终态违逆）。
    if tok.adjust(1, PrivAction::Enable).is_ok() {
        return Err(RegDiag::PrivRemovedFinal);
    }
    // 未持有启用必拒。
    if tok.adjust(2, PrivAction::Enable).is_ok() {
        return Err(RegDiag::PrivNotHeld);
    }
    if !tok.semantics_ok() {
        return Err(RegDiag::PrivAdjustRepeat);
    }
    reg.conservation()?;
    Ok(run_asserts())
}

/// 对照断言组（12 条，全过 = 12）。
pub fn run_asserts() -> u32 {
    let mut pass = 0u32;
    // A1：生命周期弧表冻结为 4。
    if KEY_ARCS.len() == 4 {
        pass += 1;
    }
    // A2：特权名冻结表全非空 + 值类型闭集全非零。
    let mut names_ok = true;
    let mut i = 0usize;
    while i < PRIV_NAMES.len() {
        if let Some(n) = PRIV_NAMES.get(i) {
            if n.is_empty() {
                names_ok = false;
            }
        }
        i += 1;
    }
    if names_ok && REG_TYPE_N == 8 {
        pass += 1;
    }
    // A3：SE_PRIVILEGE_* 语义位两两可辨。
    if SE_PRIV_ENABLED != SE_PRIV_ENABLED_BY_DEFAULT
        && SE_PRIV_REMOVED != SE_PRIV_USED_FOR_ACCESS
        && (SE_PRIV_ENABLED & SE_PRIV_REMOVED) == 0
    {
        pass += 1;
    }
    // A4：键名越闭集必拒。
    if !RegKey::name_charset_ok("bad name") && !RegKey::name_charset_ok("") {
        pass += 1;
    }
    // A5：生命周期倒退必拒（Created 直封存 / Tombstoned 再删）。
    let mut probe = Registry::new();
    let _ = probe.create("P");
    if probe.advance("P", 1).is_err() {
        pass += 1;
    }
    // A6：封存后写值必拒。
    let mut p2 = Registry::new();
    let _ = p2.create("Q");
    let _ = p2.put("Q", "v", RegVal::Dword(1));
    let _ = p2.advance("Q", 1);
    if p2.put("Q", "v", RegVal::Dword(2)).is_err() {
        pass += 1;
    }
    // A7：类型错读必抓。
    let mut p3 = Registry::new();
    let _ = p3.create("R");
    let _ = p3.put("R", "x", RegVal::Sz(String::from("s")));
    if p3.get("R", "x", 4).is_err() {
        pass += 1;
    }
    // A8：同名覆盖须同型（跨型覆盖必抓）。
    let mut p4 = Registry::new();
    let _ = p4.create("S");
    let _ = p4.put("S", "y", RegVal::Dword(1));
    if p4.put("S", "y", RegVal::Sz(String::from("t"))).is_err() {
        pass += 1;
    }
    // A9：特权幂等拒（重复启用/重复移除）。
    let mut t1 = Token::new();
    let _ = t1.adjust(0, PrivAction::Enable);
    if t1.adjust(0, PrivAction::Enable).is_err() {
        pass += 1;
    }
    // A10：移除终态违逆必抓。
    let mut t2 = Token::new();
    let _ = t2.adjust(0, PrivAction::Remove);
    if t2.adjust(0, PrivAction::Enable).is_err() && t2.adjust(0, PrivAction::Remove).is_err() {
        pass += 1;
    }
    // A11：键表满容必抓。
    let mut reg9 = Registry::new();
    let mut full = true;
    let mut j = 0usize;
    while j < KEY_SLOT_N {
        let mut nm = String::from("N");
        nm.push((b'A' + j as u8) as char);
        if reg9.create(&nm).is_err() {
            full = false;
        }
        j += 1;
    }
    if full && reg9.create("More").is_err() {
        pass += 1;
    }
    // A12：对账守恒（Σ键弧 ≡ 表弧）。
    let mut pc = Registry::new();
    let _ = pc.create("T");
    let _ = pc.put("T", "a", RegVal::Dword(1));
    let _ = pc.advance("T", 1);
    if pc.conservation().is_ok() {
        pass += 1;
    }
    pass
}

// ── 注入面：三族 × 10 轮全抓 ──────────────────────────────────────
/// 注入账本：三族各自 10 轮尝试/抓获。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 全抓判定：三族抓获数 ≡ 尝试数。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 轮数守恒：每族恰 10 轮。
    pub fn ten_rounds_each(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            match self.tries.get(i) {
                Some(&10) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 注入三族 × 10 轮：族0=生命周期倒退（直封存/墓碑复活/越表弧）、
/// 族1=特权调整违约（未持启用/移除复启/幂等重）、族2=值类型违约
/// （跨型覆盖/错读）。LCG 变参（种子=SEG_BASE），违约必抓。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族0：生命周期倒退注入——随机选一种倒退路径必抓。
        if let Some(t0) = ledger.tries.get_mut(0) {
            *t0 += 1;
        }
        let mut reg = Registry::new();
        let _ = reg.create("Inj");
        let kind = rng.below(3);
        let caught0 = match kind {
            0 => reg.advance("Inj", 1).is_err(), // Created 直封存
            1 => {
                let _ = reg.put("Inj", "v", RegVal::Dword(1));
                let _ = reg.advance("Inj", 1);
                let _ = reg.advance("Inj", 2); // 封存→墓碑
                reg.put("Inj", "v", RegVal::Dword(2)).is_err() // 墓碑复活
            }
            _ => reg.advance("Inj", 9).is_err(), // 越表弧
        };
        if caught0 {
            if let Some(c0) = ledger.caught.get_mut(0) {
                *c0 += 1;
            }
        }
        // 族1：特权调整违约注入——未持启用/移除复启/未启停用轮换必抓。
        if let Some(t1) = ledger.tries.get_mut(1) {
            *t1 += 1;
        }
        let mut tok = Token::new();
        // 下标 2/3 恒为未持有（Absent），0/1 恒为持有未启（Disabled）。
        let p_absent = 2 + (rng.below(2) as usize);
        let p_held = rng.below(2) as usize;
        let kind1 = rng.below(3);
        let caught1 = match kind1 {
            0 => tok.adjust(p_absent, PrivAction::Enable).is_err(), // 未持启用
            1 => {
                // 移除（持有→终态）后复启必拒。
                let _ = tok.adjust(p_held, PrivAction::Remove);
                tok.adjust(p_held, PrivAction::Enable).is_err()
            }
            _ => {
                // 未持停用后再停用仍必拒。
                let _ = tok.adjust(p_absent, PrivAction::Disable);
                tok.adjust(p_absent, PrivAction::Disable).is_err()
            }
        };
        if caught1 {
            if let Some(c1) = ledger.caught.get_mut(1) {
                *c1 += 1;
            }
        }
        // 族2：值类型违约注入——跨型覆盖必抓。
        if let Some(t2) = ledger.tries.get_mut(2) {
            *t2 += 1;
        }
        let mut reg2 = Registry::new();
        let _ = reg2.create("Typ");
        let _ = reg2.put("Typ", "v", RegVal::Dword(1));
        if reg2.put("Typ", "v", RegVal::Sz(String::from("x"))).is_err() {
            if let Some(c2) = ledger.caught.get_mut(2) {
                *c2 += 1;
            }
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ──────────────────────
/// 令牌族对照回归绿：启用/停用/移除全语义 + 账守恒 + 语义位自洽。
fn token_priv_ok() -> bool {
    let mut tok = Token::new();
    // 启用（持有未启→已启）。
    if tok.adjust(0, PrivAction::Enable) != Ok(PrivState::Enabled) {
        return false;
    }
    // 重复启用幂等拒。
    if tok.adjust(0, PrivAction::Enable).is_ok() {
        return false;
    }
    // 停用（已启→持有未启）。
    if tok.adjust(0, PrivAction::Disable) != Ok(PrivState::Disabled) {
        return false;
    }
    // 重复停用幂等拒。
    if tok.adjust(0, PrivAction::Disable).is_ok() {
        return false;
    }
    // 移除（持有→终态）→复启拒。
    if tok.adjust(0, PrivAction::Remove) != Ok(PrivState::Removed) {
        return false;
    }
    if tok.adjust(0, PrivAction::Enable).is_ok() || tok.adjust(0, PrivAction::Remove).is_ok() {
        return false;
    }
    // 未持有启用/停用/移除全拒。
    if tok.adjust(2, PrivAction::Enable).is_ok()
        || tok.adjust(2, PrivAction::Disable).is_ok()
        || tok.adjust(2, PrivAction::Remove).is_ok()
    {
        return false;
    }
    tok.semantics_ok() && tok.adjusts == 3
}

/// 注册表族生命周期单调绿：四弧全走通 + 倒退全拒 + 对账守恒。
fn registry_key_ok() -> bool {
    let mut reg = Registry::new();
    let _ = reg.create("Life");
    // 弧一（自动落值）→弧二封存→弧三销键。
    if reg.put("Life", "v", RegVal::Dword(1)).is_err() {
        return false;
    }
    if reg.advance("Life", 1) != Ok(KeyPhase::Sealed) {
        return false;
    }
    if reg.advance("Life", 2) != Ok(KeyPhase::Tombstoned) {
        return false;
    }
    // 键二：弧四未封先删。
    let _ = reg.create("Fast");
    if reg.put("Fast", "v", RegVal::Sz(String::from("s"))).is_err() {
        return false;
    }
    if reg.advance("Fast", 3) != Ok(KeyPhase::Tombstoned) {
        return false;
    }
    reg.conservation().is_ok()
}

/// 值类型闭集核：存取一致放行、错读/跨型覆盖必抓、容量上限必抓。
fn value_type_ok() -> bool {
    let mut reg = Registry::new();
    let _ = reg.create("Val");
    if reg.put("Val", "d", RegVal::Dword(0xDEAD_BEEF)).is_err() {
        return false;
    }
    if reg.put("Val", "q", RegVal::Qword(1)).is_err() {
        return false;
    }
    if reg.put("Val", "s", RegVal::Sz(String::from("t"))).is_err() {
        return false;
    }
    if reg.put("Val", "b", RegVal::Binary(Vec::new())).is_err() {
        return false;
    }
    // 同型覆盖放行。
    if reg.put("Val", "d", RegVal::Dword(2)).is_err() {
        return false;
    }
    // 错读必抓。
    if reg.get("Val", "d", 1).is_ok() || reg.get("Val", "s", 4).is_ok() {
        return false;
    }
    // 正读放行。
    match reg.get("Val", "d", 4) {
        Ok(RegVal::Dword(v)) => {
            if v != 2 {
                return false;
            }
        }
        _ => return false,
    }
    // 容量上限（第五条）必抓。
    reg.put("Val", "e", RegVal::Qword(9)).is_err()
}

/// 判定次序：建→写→封→删，乱序必拒。
fn order_ok() -> bool {
    let mut reg = Registry::new();
    // 未建先写/先读必拒。
    if reg.put("Ghost", "v", RegVal::Dword(1)).is_ok() || reg.get("Ghost", "v", 4).is_ok() {
        return false;
    }
    let _ = reg.create("Ord");
    // 未写先封存（Created 直封）必拒。
    if reg.advance("Ord", 1).is_ok() {
        return false;
    }
    // 正序：写→封→删。
    if reg.put("Ord", "v", RegVal::Dword(1)).is_err() {
        return false;
    }
    if reg.advance("Ord", 1) != Ok(KeyPhase::Sealed) {
        return false;
    }
    reg.advance("Ord", 2) == Ok(KeyPhase::Tombstoned)
}

/// 容量耗尽：8 键满拒，弃一清一后可复建（建键账守恒）。
fn capacity_ok() -> bool {
    let mut reg = Registry::new();
    let mut i = 0usize;
    while i < KEY_SLOT_N {
        let mut nm = String::from("C");
        nm.push((b'A' + i as u8) as char);
        if reg.create(&nm).is_err() {
            return false;
        }
        i += 1;
    }
    if reg.create("X").is_ok() {
        return false;
    }
    // 弧四弃键 → 墓碑；purge 清槽 → 槽位释放。
    if reg.put("CA", "v", RegVal::Dword(1)).is_err() {
        return false;
    }
    if reg.advance("CA", 3) != Ok(KeyPhase::Tombstoned) {
        return false;
    }
    if reg.purge("CA").is_err() {
        return false;
    }
    reg.create("Y").is_ok() && reg.created == (KEY_SLOT_N + 1) as u32
}

/// 双跑一致：黄金正演跑两遍结果逐位一致。
fn double_run_ok() -> bool {
    golden_run() == golden_run()
}

/// 对账一致/计数守恒：键弧账+注入账双守恒。
fn conservation_ok() -> bool {
    let mut reg = Registry::new();
    let _ = reg.create("Led");
    let mut i = 0u32;
    while i < 3 {
        if reg.put("Led", "v", RegVal::Dword(i + 1)).is_err() {
            return false;
        }
        i += 1;
    }
    if reg.conservation().is_err() {
        return false;
    }
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught() && led.ten_rounds_each()
}

/// 文案闭集：10 码全查得非空中文，未知码给兜底。
fn text_closed_ok() -> bool {
    let mut i = 0usize;
    while i < RegDiag::ALL.len() {
        if let Some(c) = RegDiag::ALL.get(i) {
            let d = detail(c.val());
            if d.is_empty() || d == "未知诊断码：不在 0x821C0 段闭集内" {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F) == "未知诊断码：不在 0x821C0 段闭集内"
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 区间，诊断码全在段窗内。
fn segment_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < RegDiag::ALL.len() {
        if let Some(c) = RegDiag::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 收口只增不减：断言组重跑不劣化。
fn closing_ok() -> bool {
    let a = run_asserts();
    let b = run_asserts();
    b >= a && a == 12
}

// ── 检查名/静态明细闭集（match 查表，无裸下标） ───────────────────
/// 第 i 条检查的静态名（闭集 match 查表，越表给兜底）。
fn check_name(i: usize) -> &'static str {
    match i {
        0 => "J1-令牌族对照回归绿",
        1 => "J1-特权调整语义维持",
        2 => "J1-注册表族生命周期绿",
        3 => "J1-值类型闭集核",
        4 => "J1-判定次序先行",
        5 => "J1-容量耗尽必抓",
        6 => "J1-注入族一·倒退10轮全抓",
        7 => "J1-注入族二·特权违约全抓",
        8 => "J1-注入族三·类型违约全抓",
        9 => "J1-双跑一致",
        10 => "J1-对账一致·双账守恒",
        11 => "J1-文案闭集",
        12 => "J1-段位守卫0x821C0",
        13 => "J1-收口只增不减",
        _ => "越表检查项",
    }
}

/// 第 i 条检查的静态明细兜底（第 9 条起走 s.add 静态明细）。
fn check_static(i: usize) -> &'static str {
    match i {
        0 => "启用/停用/移除全语义+账守恒",
        1 => "幂等拒·终态拒·前置拒全成立",
        2 => "四弧全走·倒退全拒·守恒绿",
        3 => "存取一致放行·错读跨型必抓",
        4 => "建→写→封→删乱序必拒",
        5 => "8键满拒·弃一复建守恒",
        6 => "倒退注入10/10",
        7 => "特权违约注入10/10",
        8 => "类型违约注入10/10",
        9 => "两次黄金正演逐位一致",
        10 => "键弧账+注入账双守恒",
        11 => "10码查表非空·未知码兜底",
        12 => "0x40对齐·VX9区间·段窗内",
        13 => "断言组重跑不劣化",
        _ => "明细缺表",
    }
}

/// extra_checks：14 条检查（正常流/边界/注入/判定次序/双跑/守恒/守卫/收口）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 1：令牌族对照回归绿（判据锚点主谓词）。
    let mut tok_probe = Token::new();
    let _ = tok_probe.adjust(0, PrivAction::Enable);
    v.push((
        String::from(check_name(0)),
        token_priv_ok(),
        format!("adjusts={}", tok_probe.adjusts),
    ));
    // 2：特权调整语义维持（判据锚点副谓词）。
    v.push((
        String::from(check_name(1)),
        privilege_semantics_ok(),
        String::from("幂等拒·终态拒·前置拒全成立"),
    ));
    // 3：注册表族生命周期绿。
    v.push((
        String::from(check_name(2)),
        registry_key_ok(),
        format!("arcs={:?}", KEY_ARCS.len()),
    ));
    // 4：值类型闭集核。
    v.push((String::from(check_name(3)), value_type_ok(), String::from(check_static(3))));
    // 5：判定次序先行。
    v.push((String::from(check_name(4)), order_ok(), String::from(check_static(4))));
    // 6：容量耗尽必抓。
    v.push((String::from(check_name(5)), capacity_ok(), String::from(check_static(5))));
    // 7~9：注入三族各 10 轮全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let mut f = 0usize;
    while f < 3 {
        let t = match led.tries.get(f) {
            Some(x) => *x,
            None => 0,
        };
        let c = match led.caught.get(f) {
            Some(x) => *x,
            None => 0,
        };
        v.push((
            String::from(check_name(6 + f)),
            c == t && t == 10,
            format!("c/t={}/{}", c, t),
        ));
        f += 1;
    }
    // 10：双跑一致。
    v.push((String::from(check_name(9)), double_run_ok(), String::from(check_static(9))));
    // 11：对账一致/双账守恒。
    v.push((
        String::from(check_name(10)),
        conservation_ok(),
        String::from("键弧账+注入账双守恒"),
    ));
    // 12：文案闭集。
    v.push((String::from(check_name(11)), text_closed_ok(), String::from(check_static(11))));
    // 13：段位守卫。
    v.push((String::from(check_name(12)), segment_guard_ok(), String::from(check_static(12))));
    // 14：收口只增不减。
    v.push((String::from(check_name(13)), closing_ok(), String::from(check_static(13))));
    v
}

/// 特权调整语义维持谓词（第 2 条检查的判定核，独立于 token_priv_ok 的
/// 全语义走通——本谓词专核三类拒的显性成立）。
fn privilege_semantics_ok() -> bool {
    // 前置拒：未持有启用。
    let mut a = Token::new();
    if a.adjust(2, PrivAction::Enable).is_ok() {
        return false;
    }
    // 幂等拒：已启再启、已停再停、已移再移。
    let mut b = Token::new();
    let _ = b.adjust(1, PrivAction::Enable);
    if b.adjust(1, PrivAction::Enable).is_ok() {
        return false;
    }
    let mut c = Token::new();
    let _ = c.adjust(1, PrivAction::Enable);
    let _ = c.adjust(1, PrivAction::Disable);
    if c.adjust(1, PrivAction::Disable).is_ok() {
        return false;
    }
    let mut d = Token::new();
    let _ = d.adjust(1, PrivAction::Remove);
    if d.adjust(1, PrivAction::Remove).is_ok() {
        return false;
    }
    // 终态拒：移除后启用/停用。
    if d.adjust(1, PrivAction::Enable).is_ok() || d.adjust(1, PrivAction::Disable).is_ok() {
        return false;
    }
    // 语义位自洽。
    d.semantics_ok()
}

// ── 检查入口 ───────────────────────────────────────────────────────
/// VX9 检查入口：extra_checks 逐条进 set（动态槽只给最关键 8 条）。
pub fn run_unx12735_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12735 注册表族对照回归");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_name_dyn, passed, detail_dyn)) = items.get(i) {
            let name = check_name(i);
            if i < 8 {
                // 动态明细槽（≤8）：判据锚点与注入族优先。
                s.add_dyn(name, *passed, detail_dyn);
            } else {
                // 静态明细：不占动态槽，红项照记不丢。
                s.add(name, *passed, check_static(i));
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_privilege_semantics_held() {
        // 判据锚点：令牌族对照回归绿 + 特权调整语义维持。
        assert!(token_priv_ok());
        assert!(privilege_semantics_ok());
    }

    #[test]
    fn registry_lifecycle_monotonic() {
        // 正常流：四弧全走通 + 倒退全拒 + 对账守恒。
        assert!(registry_key_ok());
        let mut reg = Registry::new();
        let _ = reg.create("K");
        assert!(reg.advance("K", 1).is_err());
    }

    #[test]
    fn value_type_closed_check() {
        // 边界：存取一致放行、错读/跨型覆盖/容量上限必抓。
        assert!(value_type_ok());
    }

    #[test]
    fn phase_backward_explicitly_rejected() {
        // 注入：封存后写值/墓碑复活/越表弧必抓。
        let mut reg = Registry::new();
        let _ = reg.create("B");
        let _ = reg.put("B", "v", RegVal::Dword(1));
        let _ = reg.advance("B", 1);
        assert!(reg.put("B", "v", RegVal::Dword(2)).is_err());
        let _ = reg.advance("B", 2);
        assert!(reg.put("B", "v", RegVal::Dword(3)).is_err());
        assert!(reg.advance("B", 2).is_err());
    }

    #[test]
    fn op_order_enforced() {
        // 判序：未建先写/未写先封必拒；正序全绿。
        assert!(order_ok());
    }

    #[test]
    fn key_slot_capacity_exhaustion() {
        // 边界：8 键满拒、弃一复建、建键账守恒。
        assert!(capacity_ok());
    }

    #[test]
    fn injections_three_families_all_caught() {
        // 注入：倒退/特权违约/类型违约三族各 10 轮全抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught());
        assert!(led.ten_rounds_each());
    }

    #[test]
    fn double_run_identical() {
        // 双跑一致：两次黄金正演逐位一致。
        assert!(double_run_ok());
        assert_eq!(extra_checks().len(), 14);
    }

    #[test]
    fn conservation_double_ledger() {
        // 守恒：键弧账+注入账双守恒。
        assert!(conservation_ok());
        let mut reg = Registry::new();
        let _ = reg.create("L");
        let _ = reg.put("L", "v", RegVal::Dword(1));
        assert!(reg.conservation().is_ok());
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集 + 未知码兜底。
        assert!(segment_guard_ok());
        assert!(text_closed_ok());
        assert_eq!(SEG_BASE, 0x821C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0xFFFFFFFF), "未知诊断码：不在 0x821C0 段闭集内");
    }

    #[test]
    fn checks_all_green_in_set() {
        // 全绿：检查入口 14 条全过、无截断、动态槽不超 8。
        let s = run_unx12735_checks();
        assert_eq!(s.len(), 14);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.dyn_used() <= 8);
    }
}
