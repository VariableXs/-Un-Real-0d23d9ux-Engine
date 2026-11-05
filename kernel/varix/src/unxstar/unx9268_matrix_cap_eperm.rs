//! UNX-F9268 能力位 CAP_* 与 EPERM 判定矩阵 · 0x7B080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（plan.json id=9268 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9268-J1 guard::CAP_* 能力位逐位
//!   注入判据过，缺 CAP 全 EPERM 逐格检出，能力位与 uid 判序格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 能力位按 Linux 原生编号建模（CHOWN=0 … SYSLOG=34），掩码一律走
//!      checked_shl，31/63 位不溢出；未命名的位不许被顺手置上。
//!   ② 权限集分 permitted/effective 两栏：生效判定只看 effective，继承时
//!      effective 不得凭空放大（子集关系由位运算直接保证）。
//!   ③ guard::CAP_* 逐位注入：全量能力集去掉任一位后，require 该位必须
//!      EPERM，其余位仍 Ok——逐位可观测，缺一位就是一位。
//!   ④ 能力位与 uid 的判序是本模块的核心：CAP_FOWNER 先于「是否属主」，
//!      CAP_DAC_OVERRIDE 先于「模式位是否放行」，CAP_SETUID 先于「目标
//!      uid 是否等于自己」。有能力的路径不许被 uid 关提前截断。
//!   ⑤ 构建器状态机 Empty→Partial→Full→Sealed 单向：未满不许封账，封账
//!      后 raise/drop 全拒并计非法；容量恒为 22 个命名位。
//!   ⑥ 注入账守恒：22 位 × 10 轮 = 220 次，漏检恒 0；双跑指纹一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9268 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B080;

/// 命名能力位数。
pub const CAP_COUNT: usize = 22;
/// 单能力位注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 特权不足。
pub const EPERM: i32 = 1;
/// 参数非法（未满封账 / 未命名位号越界）。
pub const EINVAL: i32 = 22;
/// 权限不足。
pub const EACCES: i32 = 13;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9268 段内诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapCode {
    /// 缺能力未回 EPERM
    CapMissing,
    /// 能力位编号/掩码不对
    CapBitWrong,
    /// 能力位串到邻位
    CapLeak,
    /// 能力位与 uid 判序倒置
    OrderBroken,
    /// DAC_OVERRIDE 未抢先
    DacOverrideLost,
    /// DAC_READ_SEARCH 越权到写
    ReadSearchLeak,
    /// SETUID 语义错
    SetuidLeak,
    /// 31/63 位掩码溢出
    BitOverflow,
    /// 容量越界
    Capacity,
    /// 继承丢位或放大
    InheritLost,
    /// 构建器状态机倒置
    StageOrder,
    /// 注入漏检
    InjectionMiss,
    /// 注入账不守恒
    LedgerBroken,
    /// 双跑不一致
    NotReproducible,
    /// 文案为空
    TextEmpty,
    /// 段位越界
    SegmentViolation,
}

impl CapCode {
    /// 码值。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            CapCode::CapMissing => "缺能力未拒：effective 缺该位必须 EPERM；请检查 has_eff 路径",
            CapCode::CapBitWrong => "能力位号错：必须与 Linux 原生编号对齐；请复核位号表",
            CapCode::CapLeak => "能力位串位：置一位不得点亮邻位；请用单一位掩码",
            CapCode::OrderBroken => "判序倒置：能力位先于 uid/模式位；请把 CAP 检查前移",
            CapCode::DacOverrideLost => "DAC_OVERRIDE 未抢先：有该能力时必须越过模式位拒绝",
            CapCode::ReadSearchLeak => "READ_SEARCH 越权：该能力只覆盖读与目录搜索，不得放行写",
            CapCode::SetuidLeak => "SETUID 语义错：有该能力可切任意 uid，无能力只能保持自身 uid",
            CapCode::BitOverflow => "掩码溢出：1<<31 / 1<<63 必须走 checked_shl，不得回绕",
            CapCode::Capacity => "容量越界：命名能力位恒为 22，未命名位必须拒绝",
            CapCode::InheritLost => "继承丢位或放大：子集 permitted & bounding，effective 不得超 permitted",
            CapCode::StageOrder => "构建器次序倒置：Empty→Partial→Full→Sealed，未满不许封账",
            CapCode::InjectionMiss => "注入漏检：去位后 require 未回 EPERM；请检查记账",
            CapCode::LedgerBroken => "注入账不守恒：注入 = 检出 + 漏检；请检查记账点",
            CapCode::NotReproducible => "双跑不一致：同能力集两次指纹不同；请排查隐藏状态",
            CapCode::TextEmpty => "文案为空：诊断码必须带非空建议",
            CapCode::SegmentViolation => "段位越界：SEG_BASE=0x7B080 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [CapCode; 16] = [
        CapCode::CapMissing, CapCode::CapBitWrong, CapCode::CapLeak,
        CapCode::OrderBroken, CapCode::DacOverrideLost, CapCode::ReadSearchLeak,
        CapCode::SetuidLeak, CapCode::BitOverflow, CapCode::Capacity,
        CapCode::InheritLost, CapCode::StageOrder, CapCode::InjectionMiss,
        CapCode::LedgerBroken, CapCode::NotReproducible, CapCode::TextEmpty,
        CapCode::SegmentViolation,
    ];
}

// ── 能力位 ────────────────────────────────────────────────────────────
/// 22 个命名能力位（Linux 编号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    /// CAP_CHOWN=0
    Chown,
    /// CAP_DAC_OVERRIDE=1
    DacOverride,
    /// CAP_DAC_READ_SEARCH=2
    DacReadSearch,
    /// CAP_FOWNER=3
    Fowner,
    /// CAP_FSETID=4
    Fsetid,
    /// CAP_KILL=5
    Kill,
    /// CAP_SETGID=6
    Setgid,
    /// CAP_SETUID=7
    Setuid,
    /// CAP_SETPCAP=8
    Setpcap,
    /// CAP_NET_BIND_SERVICE=10
    NetBindService,
    /// CAP_NET_RAW=13
    NetRaw,
    /// CAP_SYS_CHROOT=18
    SysChroot,
    /// CAP_SYS_PTRACE=19
    SysPtrace,
    /// CAP_SYS_ADMIN=21
    SysAdmin,
    /// CAP_SYS_BOOT=22
    SysBoot,
    /// CAP_SYS_NICE=23
    SysNice,
    /// CAP_SYS_RESOURCE=24
    SysResource,
    /// CAP_SYS_TIME=25
    SysTime,
    /// CAP_MKNOD=27
    Mknod,
    /// CAP_AUDIT_WRITE=29
    AuditWrite,
    /// CAP_SETFCAP=31
    Setfcap,
    /// CAP_SYSLOG=34
    Syslog,
}

impl Cap {
    /// 闭集全量（顺序即逐位注入序）。
    pub const ALL: [Cap; CAP_COUNT] = [
        Cap::Chown, Cap::DacOverride, Cap::DacReadSearch, Cap::Fowner,
        Cap::Fsetid, Cap::Kill, Cap::Setgid, Cap::Setuid, Cap::Setpcap,
        Cap::NetBindService, Cap::NetRaw, Cap::SysChroot, Cap::SysPtrace,
        Cap::SysAdmin, Cap::SysBoot, Cap::SysNice, Cap::SysResource,
        Cap::SysTime, Cap::Mknod, Cap::AuditWrite, Cap::Setfcap, Cap::Syslog,
    ];

    /// Linux 能力位号。
    pub fn bit(self) -> u32 {
        match self {
            Cap::Chown => 0,
            Cap::DacOverride => 1,
            Cap::DacReadSearch => 2,
            Cap::Fowner => 3,
            Cap::Fsetid => 4,
            Cap::Kill => 5,
            Cap::Setgid => 6,
            Cap::Setuid => 7,
            Cap::Setpcap => 8,
            Cap::NetBindService => 10,
            Cap::NetRaw => 13,
            Cap::SysChroot => 18,
            Cap::SysPtrace => 19,
            Cap::SysAdmin => 21,
            Cap::SysBoot => 22,
            Cap::SysNice => 23,
            Cap::SysResource => 24,
            Cap::SysTime => 25,
            Cap::Mknod => 27,
            Cap::AuditWrite => 29,
            Cap::Setfcap => 31,
            Cap::Syslog => 34,
        }
    }

    /// 单一位掩码（checked_shl，无回绕）。
    pub fn mask(self) -> u64 {
        1u64.checked_shl(self.bit()).unwrap_or(0)
    }

    /// 能力名。
    pub fn name(self) -> &'static str {
        match self {
            Cap::Chown => "CAP_CHOWN",
            Cap::DacOverride => "CAP_DAC_OVERRIDE",
            Cap::DacReadSearch => "CAP_DAC_READ_SEARCH",
            Cap::Fowner => "CAP_FOWNER",
            Cap::Fsetid => "CAP_FSETID",
            Cap::Kill => "CAP_KILL",
            Cap::Setgid => "CAP_SETGID",
            Cap::Setuid => "CAP_SETUID",
            Cap::Setpcap => "CAP_SETPCAP",
            Cap::NetBindService => "CAP_NET_BIND_SERVICE",
            Cap::NetRaw => "CAP_NET_RAW",
            Cap::SysChroot => "CAP_SYS_CHROOT",
            Cap::SysPtrace => "CAP_SYS_PTRACE",
            Cap::SysAdmin => "CAP_SYS_ADMIN",
            Cap::SysBoot => "CAP_SYS_BOOT",
            Cap::SysNice => "CAP_SYS_NICE",
            Cap::SysResource => "CAP_SYS_RESOURCE",
            Cap::SysTime => "CAP_SYS_TIME",
            Cap::Mknod => "CAP_MKNOD",
            Cap::AuditWrite => "CAP_AUDIT_WRITE",
            Cap::Setfcap => "CAP_SETFCAP",
            Cap::Syslog => "CAP_SYSLOG",
        }
    }
}

/// 命名能力位全集掩码。
pub fn named_mask() -> u64 {
    let mut m = 0u64;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() { m |= cap.mask(); }
        i += 1;
    }
    m
}

// ── 能力集（permitted / effective 两栏） ──────────────────────────────
/// 能力集：permitted 是上限，effective 才参与判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapSet {
    /// 允许集
    pub permitted: u64,
    /// 生效集
    pub effective: u64,
}

impl CapSet {
    /// 空集。
    pub const fn empty() -> CapSet { CapSet { permitted: 0, effective: 0 } }
    /// 全量（22 命名位）。
    pub fn full() -> CapSet {
        let m = named_mask();
        CapSet { permitted: m, effective: m }
    }
    /// effective 是否含该位。
    pub fn has_eff(&self, cap: Cap) -> bool { self.effective & cap.mask() != 0 }
    /// permitted 是否含该位。
    pub fn has_permitted(&self, cap: Cap) -> bool { self.permitted & cap.mask() != 0 }
    /// 置一位（两栏同时置）。
    pub fn raise(&mut self, cap: Cap) -> bool {
        let before = self.effective;
        self.permitted |= cap.mask();
        self.effective |= cap.mask();
        self.effective != before
    }
    /// 清一位（两栏同时清）。
    pub fn drop(&mut self, cap: Cap) {
        self.permitted &= !cap.mask();
        self.effective &= !cap.mask();
    }
    /// 去掉某位的副本。
    pub fn without(&self, cap: Cap) -> CapSet {
        CapSet {
            permitted: self.permitted & !cap.mask(),
            effective: self.effective & !cap.mask(),
        }
    }
    /// effective 置位数。
    pub fn eff_count(&self) -> u32 { self.effective.count_ones() }
    /// permitted 置位数。
    pub fn perm_count(&self) -> u32 { self.permitted.count_ones() }
    /// 子集关系：effective ⊆ permitted。
    pub fn coherent(&self) -> bool { self.effective & !self.permitted == 0 }

    /// 按裸位号置位：>63 EINVAL，未命名位 EPERM（不静默忽略）。
    pub fn raise_raw(&mut self, bit: u32) -> Result<(), i32> {
        if bit > 63 { return Err(EINVAL); }
        let mask = 1u64.checked_shl(bit).unwrap_or(0);
        if mask == 0 || mask & named_mask() == 0 { return Err(EPERM); }
        self.permitted |= mask;
        self.effective |= mask;
        Ok(())
    }
}

/// exec 继承：permitted 取父/文件并集后与 bounding 相与，effective 不得放大。
pub fn exec_inherit(parent: &CapSet, bounding: u64, file_permitted: u64) -> CapSet {
    let permitted = (parent.permitted | file_permitted) & bounding;
    let effective = permitted & parent.effective;
    CapSet { permitted, effective }
}

// ── guard::CAP_* 判定 ─────────────────────────────────────────────────
/// 能力门：缺位即 EPERM。
pub fn guard_require(caps: &CapSet, cap: Cap) -> Result<(), i32> {
    if caps.has_eff(cap) { Ok(()) } else { Err(EPERM) }
}

/// chmod 判序：CAP_FOWNER 先于「是否属主」。
pub fn judge_chmod(file_uid: u32, cred_uid: u32, caps: &CapSet) -> Result<(), i32> {
    if caps.has_eff(Cap::Fowner) { return Ok(()); }
    if file_uid == cred_uid { return Ok(()); }
    Err(EPERM)
}

/// DAC 请求方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DacWant {
    /// 读
    Read,
    /// 写
    Write,
    /// 目录搜索
    Search,
}

/// DAC 判序：OVERRIDE 全放，READ_SEARCH 只管读/搜索，最后才看模式位。
pub fn judge_dac(mode_grants: bool, want: DacWant, caps: &CapSet) -> Result<(), i32> {
    if caps.has_eff(Cap::DacOverride) { return Ok(()); }
    if want != DacWant::Write && caps.has_eff(Cap::DacReadSearch) { return Ok(()); }
    if mode_grants { Ok(()) } else { Err(EACCES) }
}

/// setuid 判序：CAP_SETUID 任意目标，否则只许保持自身 uid。
pub fn judge_setuid(target: u32, cred_uid: u32, caps: &CapSet) -> Result<(), i32> {
    if caps.has_eff(Cap::Setuid) { return Ok(()); }
    if target == cred_uid { return Ok(()); }
    Err(EPERM)
}

// ── 构建器状态机 ──────────────────────────────────────────────────────
/// 构建阶段（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapStage {
    /// 空集
    Empty,
    /// 部分置位
    Partial,
    /// 22 位齐
    Full,
    /// 已封账
    Sealed,
}

/// 能力集构建器：未满不许封账，封账后不可增删。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapBuilder {
    set: CapSet,
    stage: CapStage,
    illegal: u32,
}

impl CapBuilder {
    /// 新建空构建器。
    pub const fn new() -> CapBuilder {
        CapBuilder { set: CapSet::empty(), stage: CapStage::Empty, illegal: 0 }
    }
    /// 阶段。
    pub fn stage(&self) -> CapStage { self.stage }
    /// 非法迁移数。
    pub fn illegal(&self) -> u32 { self.illegal }
    /// 当前集合。
    pub fn set(&self) -> CapSet { self.set }

    /// 置一位：Sealed 拒绝。
    pub fn raise(&mut self, cap: Cap) -> Result<u32, i32> {
        if self.stage == CapStage::Sealed {
            self.illegal = self.illegal.saturating_add(1);
            return Err(EPERM);
        }
        self.set.raise(cap);
        let n = self.set.eff_count();
        self.stage = if n == 0 {
            CapStage::Empty
        } else if n as usize >= CAP_COUNT {
            CapStage::Full
        } else {
            CapStage::Partial
        };
        Ok(n)
    }

    /// 清一位：Sealed 拒绝，其余允许。
    pub fn drop_cap(&mut self, cap: Cap) -> Result<u32, i32> {
        if self.stage == CapStage::Sealed {
            self.illegal = self.illegal.saturating_add(1);
            return Err(EPERM);
        }
        self.set.drop(cap);
        let n = self.set.eff_count();
        self.stage = if n == 0 {
            CapStage::Empty
        } else if n as usize >= CAP_COUNT {
            CapStage::Full
        } else {
            CapStage::Partial
        };
        Ok(n)
    }

    /// 封账：只有 Full 可封。
    pub fn seal(&mut self) -> Result<u64, i32> {
        if self.stage != CapStage::Full {
            self.illegal = self.illegal.saturating_add(1);
            return Err(EINVAL);
        }
        self.stage = CapStage::Sealed;
        Ok(self.set.effective)
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 注入总数
    pub injected: u32,
    /// 检出数
    pub detected: u32,
    /// 漏检数
    pub missed: u32,
}

impl Ledger {
    /// 零账。
    pub const fn zero() -> Ledger { Ledger { injected: 0, detected: 0, missed: 0 } }
    /// 记一次。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.missed = self.missed.saturating_add(1); }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.missed) == self.injected
    }
}

/// 22 位逐位注入：去一位必须 EPERM，全量同轮必须 Ok；各 10 轮。
pub fn run_cap_matrix() -> (Ledger, [u32; CAP_COUNT]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; CAP_COUNT];
    let full = CapSet::full();
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() {
            let missing = full.without(cap);
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = guard_require(&missing, cap) == Err(EPERM)
                    && guard_require(&full, cap) == Ok(());
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per.get_mut(i) { *slot = slot.saturating_add(1); }
                }
                round += 1;
            }
        }
        i += 1;
    }
    (ledger, per)
}

// ── 对账与指纹 ────────────────────────────────────────────────────────
/// 能力表：(名字, Linux 位号)。
pub const CAP_TABLE: [(&str, u32); CAP_COUNT] = [
    ("CAP_CHOWN", 0), ("CAP_DAC_OVERRIDE", 1), ("CAP_DAC_READ_SEARCH", 2),
    ("CAP_FOWNER", 3), ("CAP_FSETID", 4), ("CAP_KILL", 5), ("CAP_SETGID", 6),
    ("CAP_SETUID", 7), ("CAP_SETPCAP", 8), ("CAP_NET_BIND_SERVICE", 10),
    ("CAP_NET_RAW", 13), ("CAP_SYS_CHROOT", 18), ("CAP_SYS_PTRACE", 19),
    ("CAP_SYS_ADMIN", 21), ("CAP_SYS_BOOT", 22), ("CAP_SYS_NICE", 23),
    ("CAP_SYS_RESOURCE", 24), ("CAP_SYS_TIME", 25), ("CAP_MKNOD", 27),
    ("CAP_AUDIT_WRITE", 29), ("CAP_SETFCAP", 31), ("CAP_SYSLOG", 34),
];

/// 对账：22 行名字与位号；返回（通过数，总数）。
pub fn reconcile_caps() -> (u32, u32) {
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let (Some(cap), Some(row)) = (Cap::ALL.get(i).copied(), CAP_TABLE.get(i)) {
            if cap.name() == row.0 && cap.bit() == row.1 && cap.mask() != 0 { 
                ok = ok.saturating_add(1);
            }
        }
        i += 1;
    }
    (ok, CAP_TABLE.len() as u32)
}

/// 确定性指纹：22 位单点集 + 缺位集的 require 结果折叠。
pub fn cap_fingerprint() -> u32 {
    let full = CapSet::full();
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() {
            let single = {
                let mut s = CapSet::empty();
                s.raise(cap);
                s
            };
            let miss = full.without(cap);
            let single_ok = guard_require(&single, cap) == Ok(())
                && single.eff_count() == 1
                && single.coherent();
            let miss_ok = guard_require(&miss, cap) == Err(EPERM);
            h ^= ((single.effective ^ (cap.bit() as u64)) & 0xFF) as u32
                ^ if single_ok && miss_ok { 1 } else { 0 };
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9268 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx9268_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9268");

    let seg_ok = SEG_BASE == 0x7B080 && SEG_BASE % 0x40 == 0
        && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0;
    s.add("段位守卫：0x7B080 且 0x40 对齐落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址须独占且对齐；越界即整册拒绝注册");

    let full = CapSet::full();
    let mut all_ok = true;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() {
            if guard_require(&full, cap) != Ok(()) { all_ok = false; }
        }
        i += 1;
    }
    s.add("正常流：全量能力集对 22 位逐位放行", all_ok,
          "effective 全置时每个 guard 都必须 Ok");

    let mut single_ok = true;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() {
            let mut one = CapSet::empty();
            let changed = one.raise(cap);
            if !changed || one.eff_count() != 1 || !one.has_eff(cap) || !one.coherent() {
                single_ok = false;
            }
            let mut j = 0usize;
            while j < Cap::ALL.len() {
                if let (Some(other), true) = (Cap::ALL.get(j).copied(), j != i) {
                    if one.has_eff(other) { single_ok = false; }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("逐位可观测：单点集只点亮本位（22/22）", single_ok,
          "一位一份掩码，邻位被点亮即 CapLeak");

    let (ledger, per) = run_cap_matrix();
    let mut per_ok = true;
    let mut i = 0usize;
    while i < per.len() {
        if per.get(i) != Some(&INJECT_ROUNDS) { per_ok = false; }
        i += 1;
    }
    s.add("注入必抓：22 位去位各 10/10 回 EPERM", per_ok,
          "220 轮注入必须 220 检出，缺一位就是一位");

    s.add("守恒：注入 = 检出 + 漏检，漏检恒为 0",
          ledger.conserved() && ledger.missed == 0
              && ledger.injected == (CAP_COUNT as u32) * INJECT_ROUNDS,
          "220 次注入必须 220 检出 0 漏检");

    let fowner_only = {
        let mut c = CapSet::empty();
        c.raise(Cap::Fowner);
        c
    };
    let none = CapSet::empty();
    let order_ok = judge_chmod(7, 1000, &fowner_only) == Ok(())
        && judge_chmod(7, 1000, &none) == Err(EPERM)
        && judge_chmod(1000, 1000, &none) == Ok(());
    s.add("能力位先于 uid：FOWNER 可改他人文件，无能力则只许属主",
          order_ok,
          "有能力的路径不许被 uid 关提前截断");

    let dac_override = {
        let mut c = CapSet::empty();
        c.raise(Cap::DacOverride);
        c
    };
    let dac_read = {
        let mut c = CapSet::empty();
        c.raise(Cap::DacReadSearch);
        c
    };
    let dac_ok = judge_dac(false, DacWant::Write, &dac_override) == Ok(())
        && judge_dac(false, DacWant::Read, &dac_read) == Ok(())
        && judge_dac(false, DacWant::Search, &dac_read) == Ok(())
        && judge_dac(false, DacWant::Write, &dac_read) == Err(EACCES)
        && judge_dac(true, DacWant::Write, &none) == Ok(())
        && judge_dac(false, DacWant::Write, &none) == Err(EACCES);
    s.add("DAC 判序：OVERRIDE 越过模式位，READ_SEARCH 只管读/搜索",
          dac_ok,
          "能力位先于 EACCES，READ_SEARCH 不得放行写");

    let setuid_cap = {
        let mut c = CapSet::empty();
        c.raise(Cap::Setuid);
        c
    };
    let setuid_ok = judge_setuid(0, 1000, &setuid_cap) == Ok(())
        && judge_setuid(1000, 1000, &none) == Ok(())
        && judge_setuid(0, 1000, &none) == Err(EPERM);
    s.add("SETUID 判序：有 CAP 可切任意 uid，无 CAP 只许保持自身",
          setuid_ok,
          "能力位先于「目标 uid 是否等于自己」");

    let mut raw = CapSet::empty();
    let bit31 = {
        let mut c = CapSet::empty();
        c.raise(Cap::Setfcap);
        c.effective
    };
    let bit_ok = bit31 == 0x8000_0000
        && raw.raise_raw(63) == Err(EPERM)
        && raw.raise_raw(64) == Err(EINVAL)
        && raw.raise_raw(Cap::Syslog.bit()) == Ok(())
        && raw.has_eff(Cap::Syslog);
    s.add("位边界：31 位不溢出、63 位属未命名拒绝、64 位 EINVAL",
          bit_ok,
          "掩码一律 checked_shl，未命名位不许顺手置上");

    let mut builder = CapBuilder::new();
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(cap) = Cap::ALL.get(i).copied() {
            let _ = builder.raise(cap);
        }
        i += 1;
    }
    let full_stage = builder.stage() == CapStage::Full && builder.set().eff_count() == 22;
    let sealed = builder.seal() == Ok(named_mask());
    let post = builder.raise(Cap::Chown) == Err(EPERM)
        && builder.drop_cap(Cap::Chown) == Err(EPERM)
        && builder.seal() == Err(EINVAL);
    s.add("构建器状态机：Empty→Partial→Full→Sealed，封账后增删全拒",
          full_stage && sealed && post && builder.illegal() == 3,
          "未满不许封账，封账后 raise/drop/seal 各记一次非法");

    let mut early = CapBuilder::new();
    let _ = early.raise(Cap::Chown);
    let early_seal = early.seal() == Err(EINVAL) && early.stage() == CapStage::Partial;
    let mut capset = CapSet::empty();
    let capacity_ok = named_mask().count_ones() as usize == CAP_COUNT
        && capset.raise_raw(60) == Err(EPERM)
        && capset.raise_raw(63) == Err(EPERM)
        && capset.perm_count() == 0;
    s.add("容量：命名位恒 22，未命名位（60/63）一律拒绝且不改集",
          early_seal && capacity_ok,
          "22 位是硬容量，未命名位不许挤进来");

    let parent = CapSet::full();
    let bound = named_mask() & !Cap::Chown.mask();
    let child = exec_inherit(&parent, bound, 0);
    let inherit_ok = !child.has_eff(Cap::Chown)
        && child.has_eff(Cap::Kill)
        && child.effective & !child.permitted == 0
        && child.eff_count() == (CAP_COUNT as u32 - 1);
    let no_eff = CapSet { permitted: named_mask(), effective: 0 };
    let child2 = exec_inherit(&no_eff, named_mask(), 0);
    let clear_ok = child2.perm_count() == CAP_COUNT as u32 && child2.eff_count() == 0;
    s.add("继承：bounding 裁 permitted，effective 不得放大，全清仍保留 permitted",
          inherit_ok && clear_ok,
          "子集关系由位运算保证，有效集只减不增");

    let f1 = cap_fingerprint();
    let f2 = cap_fingerprint();
    s.add("双跑一致：22 位指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同集合两次必须同指纹");

    let (rok, rtotal) = reconcile_caps();
    s.add("对账：22 行能力名与 Linux 位号全中", rok == rtotal && rtotal == 22,
          "位号错一位，整张能力矩阵就错一位");

    let mut text_ok = true;
    let mut ti = 0usize;
    while ti < CapCode::ALL.len() {
        if let Some(code) = CapCode::ALL.get(ti).copied() {
            if code.detail().is_empty() { text_ok = false; }
        }
        ti += 1;
    }
    s.add("文案闭集：16 个诊断码均带非空中文建议", text_ok,
          "红项必须能直接读到处置建议，不许返回空串");

    let mut uniq = true;
    let mut i = 0usize;
    while i < CapCode::ALL.len() {
        let mut j = i + 1;
        while j < CapCode::ALL.len() {
            if let (Some(a), Some(b)) = (CapCode::ALL.get(i), CapCode::ALL.get(j)) {
                if a.val() == b.val() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：16 个码值两两唯一", uniq,
          "段内闭集码重复会让红项无法定位");

    let ledger_text = format!("注入 {} / 检出 {} / 漏检 {} / 命名位 {}",
                              ledger.injected, ledger.detected, ledger.missed,
                              named_mask().count_ones());
    s.add_dyn("账目快照", ledger.conserved() && ledger.missed == 0, &ledger_text);
    let fp_text = format!("cap 指纹 0x{:08X} 对账 {}/{}", f1, rok, rtotal);
    s.add_dyn("指纹快照", f1 == f2 && rok == rtotal, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_anchored() {
        assert_eq!(SEG_BASE, 0x7B080);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn single_bit_masks() {
        let mut seen = 0u64;
        let mut i = 0usize;
        while i < Cap::ALL.len() {
            if let Some(cap) = Cap::ALL.get(i).copied() {
                let m = cap.mask();
                assert_ne!(m, 0);
                assert_eq!(m & seen, 0);
                seen |= m;
            }
            i += 1;
        }
        assert_eq!(seen, named_mask());
        assert_eq!(Cap::Setfcap.mask(), 0x8000_0000);
    }

    #[test]
    fn require_needs_effective() {
        let mut c = CapSet { permitted: named_mask(), effective: 0 };
        assert_eq!(guard_require(&c, Cap::Chown), Err(EPERM));
        let _ = c.raise(Cap::Chown);
        assert_eq!(guard_require(&c, Cap::Chown), Ok(()));
        c.drop(Cap::Chown);
        assert_eq!(guard_require(&c, Cap::Chown), Err(EPERM));
    }

    #[test]
    fn fowner_precedes_uid() {
        let mut c = CapSet::empty();
        c.raise(Cap::Fowner);
        assert_eq!(judge_chmod(9, 1000, &c), Ok(()));
        assert_eq!(judge_chmod(9, 1000, &CapSet::empty()), Err(EPERM));
        assert_eq!(judge_chmod(1000, 1000, &CapSet::empty()), Ok(()));
    }

    #[test]
    fn dac_read_search_is_read_only() {
        let mut c = CapSet::empty();
        c.raise(Cap::DacReadSearch);
        assert_eq!(judge_dac(false, DacWant::Read, &c), Ok(()));
        assert_eq!(judge_dac(false, DacWant::Search, &c), Ok(()));
        assert_eq!(judge_dac(false, DacWant::Write, &c), Err(EACCES));
    }

    #[test]
    fn setuid_cap_semantics() {
        let mut c = CapSet::empty();
        c.raise(Cap::Setuid);
        assert_eq!(judge_setuid(0, 1000, &c), Ok(()));
        assert_eq!(judge_setuid(0, 1000, &CapSet::empty()), Err(EPERM));
        assert_eq!(judge_setuid(1000, 1000, &CapSet::empty()), Ok(()));
    }

    #[test]
    fn raw_bits_are_bounded() {
        let mut c = CapSet::empty();
        assert_eq!(c.raise_raw(64), Err(EINVAL));
        assert_eq!(c.raise_raw(63), Err(EPERM));
        assert_eq!(c.raise_raw(Cap::Syslog.bit()), Ok(()));
        assert_eq!(c.perm_count(), 1);
    }

    #[test]
    fn builder_is_monotonic() {
        let mut b = CapBuilder::new();
        assert_eq!(b.seal(), Err(EINVAL));
        let mut i = 0usize;
        while i < Cap::ALL.len() {
            if let Some(cap) = Cap::ALL.get(i).copied() {
                assert!(b.raise(cap).is_ok());
            }
            i += 1;
        }
        assert_eq!(b.stage(), CapStage::Full);
        assert_eq!(b.seal(), Ok(named_mask()));
        assert_eq!(b.raise(Cap::Chown), Err(EPERM));
        assert_eq!(b.illegal(), 2);
    }

    #[test]
    fn inheritance_stays_subset() {
        let parent = CapSet::full();
        let bound = named_mask() & !Cap::Chown.mask();
        let child = exec_inherit(&parent, bound, 0);
        assert!(!child.has_eff(Cap::Chown));
        assert_eq!(child.eff_count(), CAP_COUNT as u32 - 1);
        assert!(child.coherent());
    }

    #[test]
    fn matrix_and_reconcile() {
        let (ledger, per) = run_cap_matrix();
        assert!(ledger.conserved());
        assert_eq!(ledger.injected, 220);
        assert_eq!(per, [INJECT_ROUNDS; CAP_COUNT]);
        let (ok, total) = reconcile_caps();
        assert_eq!(ok, total);
        assert_eq!(total, 22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9268_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
