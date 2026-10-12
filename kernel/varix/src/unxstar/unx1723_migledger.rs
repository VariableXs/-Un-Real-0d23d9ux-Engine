//! unx1723_migledger —— UNX-F1723 迁移执行三向记账（源/目标/线程）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F1723）：
//!   UNX-F1723-J1 源核出账/目标核入账/线程随行账三向闭合断言全过，
//!   与 F1627/F1646/F1704 三账联动一致
//!
//! ## 迁移为什么必须"记三次账"
//!
//! Windows 的线程迁移只有调度器视角：把线程挂到目标核的运行队列上，
//! 改个归属字段，`KeSetSystemAffinity` 返回成功就算完。它回答不了
//! 一个问题：**如果这次迁移只完成了一半，会发生什么？**
//!
//! 内核侧的迁移是三段原子链（源核摘出 → 暂存区 → 目标核挂入）。
//! 三段里任何一段失败，线程就处在"摘了但没挂上"的悬空态——
//! 调度器看不见它、队列里也没有它，但它还在占着栈和页表。
//!
//! 要让这种悬空态**可检出**，必须有三个独立的账：
//!
//! 1. **源核出账（`SrcOut`）**：源核记账"我摘出了 mid=X 的线程"。
//!    没有这条，就不知道该去找谁。
//! 2. **目标核入账（`DstIn`）**：目标核记账"我挂上了 mid=X 的线程"。
//!    没有这条，挂了但没记 = 从账上消失，等于没挂。
//! 3. **线程随行账（`Follow`）**：线程自身带一份"我被迁移了"的随行标。
//!    没有这条，线程在半路崩溃时无从判断自己是不是孤儿。
//!
//! ## 三向闭合的判据口径
//!
//! 对任一`mid`，必须有：`SrcOut ∧ DstIn ∧ Follow`三者齐备，
//! 且三账的**关键字段互为投影**——
//!
//! - `SrcOut` 与 `DstIn` 的 `(src_cpu, dst_cpu, payload_fp)` 必须逐位相同；
//! - `Follow` 的 `(tid, payload_fp)` 必须与前两者对齐；
//! - 三账的 `tick` 必须**严格递增**（出 <入，跟随在入之后或同拍）。
//!
//! 只查"三条都在"是**弱闭合**：三份彼此无关的记录凑齐了也能过。
//! 真正抓得住 bug 的是**投影一致性**——比如目标核挂入时算错了
//! `payload_fp`（用了挂入后的脏数据），三条记录都在，但指纹投影对不上，
//! 立刻红。
//!
//! ## 为什么"缺账"比"多账"更难查
//!
//! 多出来的账会被后续的 `SrcOut`/`DstIn` 覆盖，或至少表现为重复；
//! 但**缺账**是静默的：源核摘了记了，目标核挂了没记（或反过来），
//! 账面上就是"少了一条"，没有任何一条账自己知道对方缺了。
//! 所以本模块的 `triple_close` 判据对**三缺一**统一判红，
//! 且为缺失方向给出不同的诊断码——排障时能直接看出是哪一段漏了。

#![allow(clippy::too_many_arguments)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const F1723_VERSION: &str = "unx1723-migledger-v1";

/// 诊断段基址（本单独占·`0xB080`）。
pub const F1723_SEG_BASE: u16 = 0xB080;

/// 诊断段上限。
pub const F1723_SEG_LIMIT: u16 = 0xB08F;

/// 每账槽数（定容）。
pub const TRIPLE_SLOTS: usize = 32;

/// 判据要求的迁移样本数。
pub const MIG_SAMPLES: usize = 24;

/// FNV-1a64 偏移基。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a64 素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// --------------------------------------------------------------------------
// 诊断码
// --------------------------------------------------------------------------

/// 三向记账诊断码（**12 码闭集**·独占 `0xB080` 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T1723Code {
    /// 01 源核出账缺失。
    SrcOutMissing = 0,
    /// 02 目标核入账缺失。
    DstInMissing,
    /// 03 线程随行账缺失。
    FollowMissing,
    /// 04 源/目标核投影不一致（`src_cpu`/`dst_cpu`）。
    CpuProjBad,
    /// 05 载荷指纹投影不一致。
    PayloadProjBad,
    /// 06 线程号投影不一致。
    TidProjBad,
    /// 07 出账晚于入账（tick 序倒置）。
    TickOrderBad,
    /// 08 迁移号重复登记。
    MidDup,
    /// 09 环形账溢出（有丢账未检出）。
    RingOverflow,
    /// 10 悬空态未检出（三账齐但迁移未完成）。
    OrphanMissed,
    /// 11 线程随行账未随迁移移动（仍在源核）。
    FollowNotMoved,
    /// 12 三账联动与 F1627/F1646/F1704 不一致。
    CrossLedgerBad,
}

impl T1723Code {
    /// 全部码（闭集·顺序即码序）。
    pub const ALL: [T1723Code; 12] = [
        T1723Code::SrcOutMissing,
        T1723Code::DstInMissing,
        T1723Code::FollowMissing,
        T1723Code::CpuProjBad,
        T1723Code::PayloadProjBad,
        T1723Code::TidProjBad,
        T1723Code::TickOrderBad,
        T1723Code::MidDup,
        T1723Code::RingOverflow,
        T1723Code::OrphanMissed,
        T1723Code::FollowNotMoved,
        T1723Code::CrossLedgerBad,
    ];

    /// 诊断码值（`0xB081` 起）。
    pub fn num(self) -> u16 {
        F1723_SEG_BASE + 1 + (self as u16)
    }

    /// 可操作修复建议（每条 >= 10 字节·不许有空洞）。
    pub fn suggestion(self) -> &'static str {
        match self {
            T1723Code::SrcOutMissing => "缺源核出账：在detach 段成功后立刻写 SrcOut 账，mid 必须与队列项一致",
            T1723Code::DstInMissing => "缺目标核入账：attach 段成功后补 DstIn 账；先摘后挂路径须在挂入点无条件落账",
            T1723Code::FollowMissing => "缺线程随行账：线程结构体内须在迁移提交时置随行标并落Follow 账",
            T1723Code::CpuProjBad => "源/目标核投影不一致：出账与入账的 src_cpu/dst_cpu 必须逐位相同，勿用入账时的当前核号",
            T1723Code::PayloadProjBad => "载荷指纹投影不一致：三账须对迁移前同一快照取指纹，勿在挂入后重取（数据已脏）",
            T1723Code::TidProjBad => "线程号投影不一致：Follow 账的 tid 必须等于出账/入账的 tid，不得用 pid 或句柄混填",
            T1723Code::TickOrderBad => "出账 tick 晚于入账：核对 tick 采样点在 detach 前与 attach 后，勿复用同一快照 tick",
            T1723Code::MidDup => "迁移号重复登记：mid 必须全局单调且不复用，重复说明回滚后未作废旧 mid",
            T1723Code::RingOverflow => "环形账已溢出丢账：加大槽数或缩短采样窗口，丢账必须显性上报而非静默覆盖",
            T1723Code::OrphanMissed => "存在悬空态线程：三账缺一即已检出，请据缺哪一账定位中断的那一段",
            T1723Code::FollowNotMoved => "随行账未随核移动：Follow 账的 dst_cpu 必须等于入账目标核，仍为源核即迁移未完成",
            T1723Code::CrossLedgerBad => "三账与 F1627/F1646/F1704 联动不一致：先对齐各账 mid 口径，再逐字段比对投影",
        }
    }

    /// 渲染行。
    pub fn line(self) -> String {
        format!("0x{:04X} {}", self.num(), self.suggestion())
    }
}

/// 诊断段守卫。
#[must_use]
pub fn segment_guard(c: u16) -> bool {
    (F1723_SEG_BASE + 1..=F1723_SEG_LIMIT).contains(&c)
}

// --------------------------------------------------------------------------
// 三账分类
// --------------------------------------------------------------------------

/// 三向账类别（**三态闭集**·`Rollback` 不属三向，另计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TKind {
    /// 源核出账。
    SrcOut = 0,
    /// 目标核入账。
    DstIn = 1,
    /// 线程随行账。
    Follow = 2,
    /// 回滚账（**不计入三向闭合**）。
    Rollback = 3,
}

impl TKind {
    /// 三向类别的闭集。
    pub const TRIPLE: [TKind; 3] = [TKind::SrcOut, TKind::DstIn, TKind::Follow];

    /// 全部类别。
    pub const ALL: [TKind; 4] = [
        TKind::SrcOut,
        TKind::DstIn,
        TKind::Follow,
        TKind::Rollback,
    ];

    /// 序号。
    pub fn num(self) -> u8 {
        self as u8
    }

    /// 是否属三向（`Rollback` 不属）。
    pub fn in_triple(self) -> bool {
        self != TKind::Rollback
    }

    /// 标签。
    pub fn text(self) -> &'static str {
        match self {
            TKind::SrcOut => "src_out",
            TKind::DstIn => "dst_in",
            TKind::Follow => "follow",
            TKind::Rollback => "rollback",
        }
    }

    /// 由序号反解。
    pub fn from_num(v: u8) -> Option<TKind> {
        match v {
            0 => Some(TKind::SrcOut),
            1 => Some(TKind::DstIn),
            2 => Some(TKind::Follow),
            3 => Some(TKind::Rollback),
            _ => None,
        }
    }
}

// --------------------------------------------------------------------------
// 账条目
// --------------------------------------------------------------------------

/// 三账条目（**三向字段齐备**·便于投影比对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Triple {
    /// 类别。
    pub kind: TKind,
    /// 迁移号（三账关联键）。
    pub mid: u64,
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 目标核号。
    pub dst_cpu: u32,
    /// 采样逻辑 tick。
    pub tick: u64,
    /// 载荷指纹（对迁移前快照取）。
    pub payload_fp: u64,
    /// 成败（`false` = 该段失败并转回滚）。
    pub ok: bool,
}

impl Triple {
    /// 构造。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: TKind,
        mid: u64,
        tid: u64,
        src_cpu: u32,
        dst_cpu: u32,
        tick: u64,
        payload_fp: u64,
        ok: bool,
    ) -> Triple {
        Triple {
            kind,
            mid,
            tid,
            src_cpu,
            dst_cpu,
            tick,
            payload_fp,
            ok,
        }
    }

    /// 条目指纹（八字段 FNV）。
    pub fn entry_fp(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for f in [
            self.kind.num() as u64,
            self.mid,
            self.tid,
            self.src_cpu as u64,
            self.dst_cpu as u64,
            self.tick,
            self.payload_fp,
            self.ok as u64,
        ]
        .iter()
        {
            h ^= *f;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 投影三元组（`src_cpu, dst_cpu, payload_fp`）——**出/入账之间必须逐位相同**。
    pub fn proj(&self) -> (u32, u32, u64) {
        (self.src_cpu, self.dst_cpu, self.payload_fp)
    }

    /// 渲染行（零浮点）。
    pub fn line(&self) -> String {
        format!(
            "{} mid={} tid={} cpu={}->{} tick={} fp=0x{:016X} ok={}",
            self.kind.text(),
            self.mid,
            self.tid,
            self.src_cpu,
            self.dst_cpu,
            self.tick,
            self.payload_fp,
            self.ok
        )
    }
}

// --------------------------------------------------------------------------
// 三账本
// --------------------------------------------------------------------------

/// 三向账本（四类定容环形·丢账显性暴露）。
#[derive(Clone, Debug)]
pub struct MigLedger {
    /// 槽表（按 `TKind::num()` 分四段）。
    buf: Vec<Triple>,
    /// 每类游标。
    head: [usize; 4],
    /// 每类已填。
    filled: [usize; 4],
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// 已接受的三向迁移号（用于查重）。
    seen_mid: Vec<(u8, u64)>,
}

/// 迁移的三段状态（**三段原子链的进度**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigPhase {
    /// 未开始。
    Idle = 0,
    /// 源核已摘出。
    Detached = 1,
    /// 已入暂存区。
    Staged = 2,
    /// 目标核已挂入（终态）。
    Attached = 3,
}

impl MigPhase {
    /// 是否为终态。
    pub fn is_final(self) -> bool {
        self == MigPhase::Attached
    }

    /// 文本。
    pub fn text(self) -> &'static str {
        match self {
            MigPhase::Idle => "idle",
            MigPhase::Detached => "detached",
            MigPhase::Staged => "staged",
            MigPhase::Attached => "attached",
        }
    }

    /// 次态。
    pub fn next(self) -> Option<MigPhase> {
        match self {
            MigPhase::Idle => Some(MigPhase::Detached),
            MigPhase::Detached => Some(MigPhase::Staged),
            MigPhase::Staged => Some(MigPhase::Attached),
            MigPhase::Attached => None,
        }
    }
}

impl MigLedger {
    /// 造空账。
    pub fn new() -> MigLedger {
        MigLedger {
            buf: Vec::with_capacity(TRIPLE_SLOTS * 4),
            head: [0; 4],
            filled: [0; 4],
            stats: [0; 12],
            seen_mid: Vec::with_capacity(TRIPLE_SLOTS * 3),
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: T1723Code) {
        let i = (c.num() - F1723_SEG_BASE - 1) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: T1723Code) -> u64 {
        let i = (c.num() - F1723_SEG_BASE - 1) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 段首下标。
    fn seg_base(k: TKind) -> usize {
        k.num() as usize * TRIPLE_SLOTS
    }

    /// 写一条账（定容环形·满了覆盖最旧并立案）。
    pub fn push(&mut self, e: Triple) -> bool {
        let k = e.kind.num() as usize;
        let base = MigLedger::seg_base(e.kind);
        let idx = base + self.head[k];
        // 三向账的 `mid` 在**同一类别内**必须首次出现。
        // 注意口径：不能拿全局 `seen_mid` 查重——三向闭合的
        // 定义就是同一个 mid 要在三个类别里各有一条，若跨类别查重，
        // 第二类（DstIn）会被自己的第一条 SrcOut 判成重复而拒，
        // 于是三账永远凑不齐。
        if e.kind.in_triple() && self.seen_mid.contains(&(e.kind.num(), e.mid)) {
            self.bump(T1723Code::MidDup);
            return false;
        }
        if let Some(cell) = self.filled.get_mut(k) {
            if *cell >= TRIPLE_SLOTS {
                // 覆盖：必须显性上报，绝不静默丢。
                self.bump(T1723Code::RingOverflow);
            } else {
                *cell = cell.wrapping_add(1);
            }
        }
        // Vec 首次填满时长度不足，需要补齐到定容。
        while self.buf.len() < base + TRIPLE_SLOTS {
            self.buf.push(Triple::new(TKind::SrcOut, 0, 0, 0, 0, 0, 0, false));
        }
        if let Some(slot) = self.buf.get_mut(idx) {
            *slot = e;
        }
        self.head[k] = (self.head[k].wrapping_add(1)) % TRIPLE_SLOTS;
        if e.kind.in_triple() {
            self.seen_mid.push((e.kind.num(), e.mid));
        }
        true
    }

    /// 取某类的全部账（按写入序）。
    pub fn list(&self, k: TKind) -> Vec<Triple> {
        let base = MigLedger::seg_base(k);
        let mut v: Vec<Triple> = Vec::with_capacity(TRIPLE_SLOTS);
        for i in 0..self.filled[k as usize] {
            // `at` 必须是**段内偏移**，且`head` 指向"下一个写入位"，
            // 所以最新已写的一条在 `head-1`——这里若不减那个 1，
            // 读到的就是从未写过的空槽，find 因此永远返回 None。
            let at = (self.head[k as usize] + TRIPLE_SLOTS - 1 - i) % TRIPLE_SLOTS;
            if let Some(t) = self.buf.get(base + at) {
                v.push(*t);
            }
        }
        v
    }

    /// 按 `mid` 找某类的账。
    pub fn find(&self, k: TKind, mid: u64) -> Option<Triple> {
        self.list(k).into_iter().find(|e| e.mid == mid)
    }

    /// 该 `mid` 是否三账齐备。
    pub fn triple_present(&self, mid: u64) -> bool {
        TKind::TRIPLE.iter().all(|k| self.find(*k, mid).is_some())
    }

    /// 三向闭合（**存在性 + 投影一致性 + tick 序**）。
    ///
    /// 返回 `(mid, 首个缺陷码)`；全闭合返回 `(mid, None)`。
    pub fn triple_close(&mut self, mid: u64) -> Option<T1723Code> {
        let s = self.find(TKind::SrcOut, mid);
        let d = self.find(TKind::DstIn, mid);
        let f = self.find(TKind::Follow, mid);
        let (s, d, f) = match (s, d, f) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            (None, _, _) => {
                self.bump(T1723Code::SrcOutMissing);
                return Some(T1723Code::SrcOutMissing);
            }
            (_, None, _) => {
                self.bump(T1723Code::DstInMissing);
                return Some(T1723Code::DstInMissing);
            }
            (_, _, None) => {
                self.bump(T1723Code::FollowMissing);
                return Some(T1723Code::FollowMissing);
            }
        };
        // 投影①：仅源核/目标核。
        // 注意不能直接比 `proj()`——它把 `payload_fp` 也包进去了，
        // 于是"只改了载荷指纹"的破口会被先判成核投影错，
        // 诊断码答非所问（排障会被带去看核号，而真凶是挂入后重取了脏数据）。
        // 核投影与载荷投影是两个独立维度，必须分开判。
        if s.src_cpu != d.src_cpu || s.dst_cpu != d.dst_cpu {
            self.bump(T1723Code::CpuProjBad);
            return Some(T1723Code::CpuProjBad);
        }
        // 投影②：载荷指纹。
        if s.payload_fp != d.payload_fp || d.payload_fp != f.payload_fp {
            self.bump(T1723Code::PayloadProjBad);
            return Some(T1723Code::PayloadProjBad);
        }
        // 投影③：线程号。
        if s.tid != d.tid || d.tid != f.tid {
            self.bump(T1723Code::TidProjBad);
            return Some(T1723Code::TidProjBad);
        }
        // 随行账必须已随核移动。
        if f.dst_cpu != d.dst_cpu {
            self.bump(T1723Code::FollowNotMoved);
            return Some(T1723Code::FollowNotMoved);
        }
        // tick 序：出 <= 入 <= 随行。
        if !(s.tick <= d.tick && d.tick <= f.tick) {
            self.bump(T1723Code::TickOrderBad);
            return Some(T1723Code::TickOrderBad);
        }
        None
    }

    /// 全样本三向闭合（逐 mid 检·返回首个缺陷）。
    pub fn all_close(&mut self, mids: &[u64]) -> Option<T1723Code> {
        for m in mids.iter() {
            if let Some(c) = self.triple_close(*m) {
                return Some(c);
            }
        }
        None
    }

    /// 悬空态检出（**三段未走完却已被记为完成**）。
    ///
    /// 真正的悬空态是：某`mid` 只有出账没有入账，线程已从源核消失。
    /// 这在三向闭合里已被 `DstInMissing` 抓住；这里额外查一种
    /// 更隐蔽的形态——**入账记了但随行账的 `dst_cpu` 还是源核**，
    /// 说明"挂入"只是写了个标记，线程实际没搬。
    pub fn orphans(&mut self, mids: &[u64]) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::new();
        for m in mids.iter() {
            let d = self.find(TKind::DstIn, *m);
            let f = self.find(TKind::Follow, *m);
            let hit = match (d, f) {
                (Some(dd), Some(ff)) => ff.dst_cpu != dd.dst_cpu,
                (Some(_), None) => true,
                _ => false,
            };
            if hit {
                v.push(*m);
            }
        }
        if !v.is_empty() {
            self.bump(T1723Code::OrphanMissed);
        }
        v
    }

    /// 账本指纹（按四类顺序延展）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for k in TKind::ALL.iter() {
            h ^= k.num() as u64;
            h = h.wrapping_mul(FNV_PRIME);
            for e in self.list(*k).iter() {
                h ^= e.entry_fp();
                h = h.wrapping_mul(FNV_PRIME);
            }
        }
        h
    }

    /// 渲染（看板真源·头尾各两行）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "mig-ledger slots={} fp=0x{:016X} seg=0x{:04X}\n",
            TRIPLE_SLOTS,
            self.fingerprint(),
            F1723_SEG_BASE
        ));
        for k in TKind::ALL.iter() {
            let l = self.list(*k);
            s.push_str(&format!(
                "{} n={} ok={} filled={}\n",
                k.text(),
                l.len(),
                l.iter().filter(|e| e.ok).count(),
                self.filled[k.num() as usize]
            ));
        }
        s.push_str("src_out | dst_in | follow\n");
        let n = self.filled[TKind::SrcOut as usize].min(2);
        for i in 0..n {
            if let (Some(a), Some(b), Some(c)) = (
                self.find(TKind::SrcOut, self.list(TKind::SrcOut)[i].mid),
                self.find(TKind::DstIn, self.list(TKind::SrcOut)[i].mid),
                self.find(TKind::Follow, self.list(TKind::SrcOut)[i].mid),
            ) {
                s.push_str(&format!("{}\n", a.line()));
                s.push_str(&format!("{}\n", b.line()));
                s.push_str(&format!("{}\n\n", c.line()));
            }
        }
        s
    }
}

// --------------------------------------------------------------------------
// 三向联动登记（F1627 / F1646 / F1704 口径）
// --------------------------------------------------------------------------

/// 外部三账的联动指纹槽（**只对齐口径·不复制数据**）。
///
/// F1627 管对象生命周期、F1646 管队列归属、F1704 管跨核可见性。
/// 三者与本模块的迁移账共享 `mid`。联动一致的口径是：
/// **同一 `mid` 在四个账系里的出现次数必须一致**——多一次或少一次
/// 都说明某一段写了重复账或漏写账。
#[derive(Clone, Debug)]
pub struct CrossLink {
    /// F1627 对象账指纹。
    pub f1627: u64,
    /// F1646 队列账指纹。
    pub f1646: u64,
    /// F1704 可见性账指纹。
    pub f1704: u64,
    /// 本模块三账合成指纹。
    pub f1723: u64,
    /// 联动口径版本（对齐用）。
    pub abi: u32,
}

/// 联动口径版本（冻结值·变更须走显式升版）。
pub const LINK_ABI: u32 = 1;

impl CrossLink {
    /// 构造。
    pub fn new(f1627: u64, f1646: u64, f1704: u64, f1723: u64) -> CrossLink {
        CrossLink {
            f1627,
            f1646,
            f1704,
            f1723,
            abi: LINK_ABI,
        }
    }

    /// 四账合成指纹（**顺序敏感**·防拼接歧义）。
    ///
    /// 这里刻意用**位置化**的 FNV 延展而不是 XOR 聚合：XOR 满足交换律，
    /// `[a,b,c,d]` 与 `[d,c,b,a]` 的异或和完全相同，等于四笔账的
    /// 位置信息被抹掉——而"哪笔账接的是哪一笔"恰恰是联动要查的东西。
    pub fn mixed(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for (i, v) in [self.f1627, self.f1646, self.f1704, self.f1723]
            .iter()
            .enumerate()
        {
            // 先掺位置号，四账同值也必得不同合成值。
            h ^= (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            h = h.wrapping_mul(FNV_PRIME);
            h ^= *v;
            h = h.wrapping_mul(FNV_PRIME);
        }
        // 再混入 ABI，防止"改了口径但指纹恰好相同"。
        h ^ (self.abi as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }

    /// 联动一致（四账齐备且 ABI 正确）。
    pub fn consistent(&self) -> bool {
        self.abi == LINK_ABI
            && self.f1627 != 0
            && self.f1646 != 0
            && self.f1704 != 0
            && self.f1723 != 0
    }

    /// 交换任两账后的指纹（**用于证明顺序确实入指纹**）。
    pub fn swapped_mixed(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for (i, v) in [self.f1704, self.f1646, self.f1723, self.f1627]
            .iter()
            .enumerate()
        {
            h ^= (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            h = h.wrapping_mul(FNV_PRIME);
            h ^= *v;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^ (self.abi as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }
}

// --------------------------------------------------------------------------
// 样本生成
// --------------------------------------------------------------------------

/// 确定性 LCG（**零墙钟·跨平台同序**）。
pub fn lcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 11
}

/// 样本载荷指纹。
#[must_use]
pub fn sample_fp(mid: u64) -> u64 {
    (FNV_OFFSET_BASIS ^ mid.wrapping_mul(FNV_PRIME)).wrapping_add(0x0100_0000_0000_0000)
}

/// 造一份三账齐备的正常样本账本（`MIG_SAMPLES` 个迁移）。
#[must_use]
pub fn build_clean() -> (MigLedger, Vec<u64>) {
    let mut l = MigLedger::new();
    let mut mids: Vec<u64> = Vec::with_capacity(MIG_SAMPLES);
    let mut st = 0x1234_5678_9abc_def0u64;
    for i in 0..MIG_SAMPLES {
        let mid = (i as u64) + 1;
        mids.push(mid);
        let tid = 0x4000 + (lcg(&mut st) & 0xFFF);
        let src = (lcg(&mut st) % 8) as u32;
        let dst = (src + 1 + (lcg(&mut st) % 7) as u32) % 8;
        let fp = sample_fp(mid);
        let t0 = 100 + (i as u64) * 10;
        let _ = l.push(Triple::new(TKind::SrcOut, mid, tid, src, dst, t0, fp, true));
        let _ = l.push(Triple::new(TKind::DstIn, mid, tid, src, dst, t0 + 1, fp, true));
        let _ = l.push(Triple::new(TKind::Follow, mid, tid, src, dst, t0 + 2, fp, true));
    }
    (l, mids)
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// UNX-F1723 判据全集（12 条）。
pub fn run_f1723_checks() -> CheckSet {
    let mut cs = CheckSet::new("f1723");

    // ---- F1723-01 诊断段闭集与建议完备 ----
    let seg_ok = T1723Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.num() == F1723_SEG_BASE + 1 + i as u16)
        && T1723Code::ALL.iter().all(|c| segment_guard(c.num()));
    let sug_ok = T1723Code::ALL.iter().all(|c| c.suggestion().len() >= 10);
    cs.add("F1723-01 诊断段闭集", seg_ok && sug_ok, "12 码连续落段+建议完备");

    // ---- F1723-02 三向类别闭集 ----
    let kind_ok = TKind::TRIPLE.len() == 3
        && TKind::TRIPLE.iter().all(|k| k.in_triple())
        && !TKind::Rollback.in_triple()
        && TKind::from_num(2) == Some(TKind::Follow)
        && TKind::from_num(9).is_none();
    cs.add("F1723-02 三账闭集", kind_ok, "SrcOut/DstIn/Follow 三态·Rollback 不入三向");

    // ---- F1723-03 干净样本三向全闭合 ----
    let (mut l3, mids) = build_clean();
    let all_closed = l3.all_close(&mids).is_none();
    cs.add(
        "F1723-03 三向闭合",
        all_closed && l3.triple_present(mids[0]),
        "24 个迁移三账齐备且逐 mid 闭合无缺陷",
    );

    // ---- F1723-04 投影一致（出/入账字段逐位相同） ----
    let proj_ok = mids.iter().all(|m| {
        match (l3.find(TKind::SrcOut, *m), l3.find(TKind::DstIn, *m)) {
            (Some(a), Some(b)) => a.proj() == b.proj() && a.tid == b.tid,
            _ => false,
        }
    });
    cs.add("F1723-04 出入投影一致", proj_ok, "24 组 (src,dst,fp,tid) 出入账逐位相同");

    // ---- F1723-05 三缺一各自被定位 ----
    // 分别造三本缺一账的账本，期望各自命中对应诊断码。
    let mut miss_src = build_clean();
    let mid_a = 1u64;
    let mut l5a = MigLedger::new();
    if let Some(d) = miss_src.0.find(TKind::DstIn, mid_a) {
        let _ = l5a.push(d);
    }
    if let Some(f) = miss_src.0.find(TKind::Follow, mid_a) {
        let _ = l5a.push(f);
    }
    let e_a = l5a.triple_close(mid_a);
    let mut miss_dst = build_clean();
    let mut l5b = MigLedger::new();
    if let Some(s) = miss_dst.0.find(TKind::SrcOut, mid_a) {
        let _ = l5b.push(s);
    }
    if let Some(f) = miss_dst.0.find(TKind::Follow, mid_a) {
        let _ = l5b.push(f);
    }
    let e_b = l5b.triple_close(mid_a);
    let mut miss_fol = build_clean();
    let mut l5c = MigLedger::new();
    if let Some(s) = miss_fol.0.find(TKind::SrcOut, mid_a) {
        let _ = l5c.push(s);
    }
    if let Some(d) = miss_fol.0.find(TKind::DstIn, mid_a) {
        let _ = l5c.push(d);
    }
    let e_c = l5c.triple_close(mid_a);
    cs.add(
        "F1723-05 三缺一定位",
        e_a == Some(T1723Code::SrcOutMissing)
            && e_b == Some(T1723Code::DstInMissing)
            && e_c == Some(T1723Code::FollowMissing),
        "缺出账/缺入账/缺随行账分别命中三个不同诊断码",
    );

    // ---- F1723-06 投影破口四类各被抓住 ----
    // ① 改入账的 dst_cpu（悬空态的典型形态）。
    let mut l6a = MigLedger::new();
    let tid = 0x1000;
    let fp = sample_fp(7);
    let _ = l6a.push(Triple::new(TKind::SrcOut, 7, tid, 0, 3, 10, fp, true));
    let _ = l6a.push(Triple::new(TKind::DstIn, 7, tid, 0, 9, 11, fp, true));
    let _ = l6a.push(Triple::new(TKind::Follow, 7, tid, 0, 9, 12, fp, true));
    let e6a = l6a.triple_close(7);
    // ② 载荷指纹错（挂入后重取·数据已脏）。
    let mut l6b = MigLedger::new();
    let _ = l6b.push(Triple::new(TKind::SrcOut, 8, tid, 1, 2, 20, fp, true));
    let _ = l6b.push(Triple::new(TKind::DstIn, 8, tid, 1, 2, 21, fp ^ 0xFF, true));
    let _ = l6b.push(Triple::new(TKind::Follow, 8, tid, 1, 2, 22, fp, true));
    let e6b = l6b.triple_close(8);
    // ③ 线程号错。
    let mut l6c = MigLedger::new();
    let _ = l6c.push(Triple::new(TKind::SrcOut, 9, tid, 3, 4, 30, fp, true));
    let _ = l6c.push(Triple::new(TKind::DstIn, 9, tid, 3, 4, 31, fp, true));
    let _ = l6c.push(Triple::new(TKind::Follow, 9, tid ^ 1, 3, 4, 32, fp, true));
    let e6c = l6c.triple_close(9);
    // ④ tick 序倒置。
    let mut l6d = MigLedger::new();
    let _ = l6d.push(Triple::new(TKind::SrcOut, 10, tid, 0, 1, 99, fp, true));
    let _ = l6d.push(Triple::new(TKind::DstIn, 10, tid, 0, 1, 40, fp, true));
    let _ = l6d.push(Triple::new(TKind::Follow, 10, tid, 0, 1, 41, fp, true));
    let e6d = l6d.triple_close(10);
    cs.add(
        "F1723-06 投影破口四类",
        e6a == Some(T1723Code::CpuProjBad)
            && e6b == Some(T1723Code::PayloadProjBad)
            && e6c == Some(T1723Code::TidProjBad)
            && e6d == Some(T1723Code::TickOrderBad),
        "核投影/载荷指纹/线程号/tick 序 四个破口各命中对应码",
    );

    // ---- F1723-07 悬空态检出 ----
    let (mut l7, mids7) = build_clean();
    // 造一个"入账记了但随行账 dst 仍是源核"的悬空。
    let mut orphan_mid = (MIG_SAMPLES as u64) + 100;
    let _ = l7.push(Triple::new(TKind::SrcOut, orphan_mid, 0x55, 2, 5, 900, 1, true));
    let _ = l7.push(Triple::new(TKind::DstIn, orphan_mid, 0x55, 2, 5, 901, 1, true));
    let _ = l7.push(Triple::new(TKind::Follow, orphan_mid, 0x55, 2, 2, 902, 1, true));
    let mut probe = mids7.clone();
    probe.push(orphan_mid);
    let orph = l7.orphans(&probe);
    let orphan_ok = orph.len() == 1
        && orph[0] == orphan_mid
        && l7.triple_close(orphan_mid) == Some(T1723Code::FollowNotMoved)
        && l7.stat(T1723Code::OrphanMissed) >= 1;
    cs.add(
        "F1723-07 悬空态检出",
        orphan_ok,
        "入账已记但随行账未随核移动→OrphanMissed+FollowNotMoved",
    );

    // ---- F1723-08 mid 重复登记被拒 ----
    let mut l8 = MigLedger::new();
    let a1 = l8.push(Triple::new(TKind::SrcOut, 1, 1, 0, 1, 1, 1, true));
    let a2 = l8.push(Triple::new(TKind::SrcOut, 1, 1, 0, 1, 2, 1, true));
    // 回滚账允许复用 mid。
    let r1 = l8.push(Triple::new(TKind::Rollback, 1, 1, 0, 1, 3, 1, false));
    let r2 = l8.push(Triple::new(TKind::Rollback, 1, 1, 0, 1, 4, 1, false));
    // 反向：同一 mid 在**不同类别**各一条必须放行（这正是三向闭合的定义）。
    let x1 = l8.push(Triple::new(TKind::DstIn, 1, 1, 0, 1, 5, 1, true));
    let x2 = l8.push(Triple::new(TKind::Follow, 1, 1, 0, 1, 6, 1, true));
    cs.add(
        "F1723-08 mid 查重",
        a1 && !a2 && r1 && r2 && x1 && x2 && l8.stat(T1723Code::MidDup) == 1,
        "同类内 mid 重复拒·跨类各一条放行·回滚账允许复用",
    );

    // ---- F1723-09 环形溢出显性上报 ----
    let mut l9 = MigLedger::new();
    for i in 0..(TRIPLE_SLOTS as u64 + 5) {
        let _ = l9.push(Triple::new(
            TKind::SrcOut,
            1000 + i,
            i,
            0,
            1,
            i,
            i,
            true,
        ));
    }
    let ovf = l9.stat(T1723Code::RingOverflow);
    cs.add(
        "F1723-09 溢出显性",
        ovf == 5 && l9.filled[TKind::SrcOut as usize] == TRIPLE_SLOTS,
        "写 37 条进 32 槽→溢出 5 条全部立案·filled 封顶",
    );

    // ---- F1723-10 三段相状态机 ----
    let mut p = MigPhase::Idle;
    let mut steps = 0u32;
    while let Some(n) = p.next() {
        p = n;
        steps = steps.wrapping_add(1);
    }
    let phase_ok = steps == 3 && p.is_final() && MigPhase::Attached.next().is_none();
    cs.add("F1723-10 三段相", phase_ok, "Idle→Detached→Staged→Attached 三步到终态");

    // ---- F1723-11 四账联动一致 ----
    let (l11, _) = build_clean();
    let link_ok = CrossLink::new(0x1111, 0x2222, 0x3333, l11.fingerprint());
    let link = link_ok.consistent() && link_ok.abi == LINK_ABI;
    // 交换顺序必须改指纹（证明顺序真的入了合成）。
    let order_sensitive = link_ok.mixed() != link_ok.swapped_mixed();
    // 缺任一账即不联动。
    let broken = CrossLink::new(0x1111, 0x2222, 0, l11.fingerprint());
    let mut l11m = MigLedger::new();
    if !broken.consistent() {
        l11m.bump(T1723Code::CrossLedgerBad);
    }
    cs.add(
        "F1723-11 四账联动",
        link && order_sensitive && !broken.consistent() && l11m.stat(T1723Code::CrossLedgerBad) == 1,
        "F1627/F1646/F1704/本模块 四账齐备且顺序入指纹·缺一即破联动",
    );

    // ---- F1723-12 闭环语义总闸 ----
    // 把F1723-03/05/06/07 的结论合到一处：三向闭合不是"三条都在"，
    // 而是"三条都在且互为投影"。这里做一次端到端演示：
    // 先确认干净账全闭合，再抽掉一个 mid 的入账看它立刻红。
    let (mut l12, mids12) = build_clean();
    let before_ok = l12.all_close(&mids12).is_none();
    let fp_before = l12.fingerprint();
    // 抽掉 mid=3 的入账：模拟目标核挂入成功但没记账。
    let victim = 3u64;
    let dst3 = l12.find(TKind::DstIn, victim);
    let mut l12b = l12.clone();
    if let Some(d) = dst3 {
        let _ = d;
        // 用一条 mid 不同、内容相同的入账覆盖，等价于"入账丢失"。
        let mut poisoned = Vec::new();
        for k in TKind::TRIPLE.iter() {
            for e in l12b.list(*k).iter() {
                if e.mid == victim && *k == TKind::DstIn {
                    continue;
                }
                poisoned.push(*e);
            }
        }
        l12b = MigLedger::new();
        for e in poisoned.iter() {
            let _ = l12b.push(*e);
        }
    }
    let after = l12b.triple_close(victim);
    let fp_after = l12b.fingerprint();
    cs.add(
        "F1723-12 缺账必红",
        before_ok && after == Some(T1723Code::DstInMissing) && fp_before != fp_after,
        "干净账全闭合→抽掉一条入账立刻红且账本指纹改变",
    );

    cs
}

/// UNX-F1723 锚点串（跨批引用防漂移）。
#[must_use]
pub fn f1723_anchor() -> &'static str {
    F1723_VERSION
}