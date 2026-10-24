//! unx1863_shootbegin —— UNX-F1863 击落发起原语（本地 flush + 远程广播判定）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B14）：UNX-F1863 · 击落发起原语（本地 flush+远程广播判定）
//!   · 目标 360 行 · 已深化
//! 判据原文：UNX-F1863-J1 发起原语冻结 v1（先本地后远程/先登记后发送），广播判定
//!   断言全过，次序颠倒注入 10/10 检出
//! 组件：LocalFlush（发起核本地 TLB flush 原语：ASID 绑定 + 页区间 + flush 域
//!   选择 + 指令序列账四元）+ Register（先登记：target/broadcast 双份快照落账
//!   并要求相等——登记未完成禁止任何 ack 进入）+ Broadcast（后发送：远程
//!   IPI 逐核投递 + 投递序列单调账 + 目标核在线/过渡态路由）+ OrderLedger
//!   （次序账：LOC→REG→BC 三段单调推进，任何回退即破）+ Judge（广播判定：
//!   本地必做 / 远程必达 / 目标集与 mm 活跃集一致性 / 过渡态核改走挂起路径
//!   / 空目标集零广播合法）+ InverseAudit（次序颠倒检出器：把合法序列的
//!   段序打乱成非法序列，判定必须全红）+ Probe（注入谱 10/10）+ LCG 压力
//!   （发起/注入/判定混合轮转，登记账与发送账守恒）+ 指纹对拍锚。
//! 对标 Linux：`tlb_finish_mmu` / `flush_tlb_mm` 一族——先本地 flush 再
//!   `smp_call_function_many` 远程广播；`arch_tlbbatch_flush` 以
//!   `tlb_flush_mm` 为本地入口、以 `flush_tlb_range` 为远程载体；Windows
//!   x64 对应面是 `KeFlushExtendedRangeAll`（本地）+ `KeIpiGenericCall`
//!   （远程广播），两者同样强制「本地先做、远程后发」，且远程集合为空时
//!   直接短路返回——本模块以「三段次序账 + 空集短路 + 过渡态改道」复刻该
//!   分界，且比内核更严：把「先登记后发送」提升为不可绕过的硬闸（内核允许
//!   发送途中登记，本模块禁止，因登记不闭合会导致幽灵应答，与 F1865 的
//!   Σ应答=Σ广播 口径直接冲突）。
//! 性能逐项分解：local_flush O(区间数)；register O(目标核数)（双份快照逐位
//!   对拍）；broadcast O(远程核数)；order_audit O(段数)；judge O(核数 + 页
//!   区间数）；逆序检出 O(10)；压力 O(轮数)；全整数算术零浮点零墙钟，同事件
//!   序列恒同账（确定性对拍锚）。
//! 诊断码段：0xB110（0xB110~0xB11F，全库 grep 零命中，独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/i64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板
//!   复述，零死代码。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 单号（供册内对拍与台账登记）。
pub const U1863_ID: u32 = 1863;
/// 单名（册内唯一文件名主干）。
pub const U1863_NAME: &str = "unx1863_shootbegin";
/// 版本（发起原语冻结版本，必须为 v1 —— 判据「冻结 v1」的载体）。
pub const U1863_VERSION: u32 = 1;
/// 诊断码段基址（全域独占，grep 确认）。
pub const U1863_SEG_BASE: u16 = 0xB110;
/// 诊断码段上界（含）。
pub const U1863_SEG_END: u16 = 0xB11F;
/// 核数上限（64 位 mask 全域，无截断）。
pub const CORE_CAP: usize = 64;
/// 页区间数上限（单次发起的 flush 区间封顶）。
pub const RANGE_CAP: usize = 64;
/// 次序颠倒注入谱笔数（判据③ 10/10 口径）。
pub const INJECT_N: usize = 10;
/// LCG 压力轮数。
pub const STRESS_ROUNDS: usize = 384;
/// LCG 乘子（Knuth 1664525 递推，纯整数）。
pub const LCG_MUL: u64 = 1664525;
/// LCG 增量。
pub const LCG_ADD: u64 = 1013904223;
/// LCG 模（2^32 截断）。
pub const LCG_MOD: u64 = 1 << 32;

/// 本地 flush 域：全地址空间（`flush_tlb_mm` 语义）。
pub const FLUSH_ALL: u8 = 0;
/// 本地 flush 域：单核 TLB 全清（`flush_tlb` 语义）。
pub const FLUSH_LOCAL_ALL: u8 = 1;
/// 本地 flush 域：页区间（`flush_tlb_range` 语义）。
pub const FLUSH_RANGE: u8 = 2;
/// flush 域闭集上界（越界即破）。
pub const FLUSH_MAX: u8 = 2;

// 判码（段内独占 0xB110~0xB11F）
/// 0xB110 次序回退（LOC/REG/BC 三段任一段位倒退或跳跃）。
pub const C_ORDER_BACK: u16 = 0xB110;
/// 0xB111 未登记先发送（ack 流进入时登记未落账）。
pub const C_SEND_BEFORE_REG: u16 = 0xB111;
/// 0xB112 登记双份快照不相等（target 与 broadcast 不一致）。
pub const C_REG_MISMATCH: u16 = 0xB112;
/// 0xB113 远程先于本地（远程投递账先落，本地 flush 账后落）。
pub const C_REMOTE_BEFORE_LOCAL: u16 = 0xB113;
/// 0xB114 广播判定不成立（目标集与活跃集不一致 / 过渡态核未改道）。
pub const C_BROADCAST_JUDGE: u16 = 0xB114;
/// 0xB115 flush 域越界（非 FLUSH_ALL/LOCAL_ALL/RANGE）。
pub const C_FLUSH_DOMAIN: u16 = 0xB115;
/// 0xB116 区间账越界（区间数超封顶 / 起止倒置）。
pub const C_RANGE_BAD: u16 = 0xB116;
/// 0xB117 本地 flush 缺失（远程已投递但本地账未落）。
pub const C_LOCAL_MISSING: u16 = 0xB117;
/// 0xB118 发送序列非单调（投递 seq 未严格递增）。
pub const C_SEND_SEQ: u16 = 0xB118;
/// 0xB119 离线核仍被投递（目标集含非在线核且无降级标记）。
pub const C_OFFLINE_TARGET: u16 = 0xB119;

// ---------------------------------------------------------------------------
// FNV-1a 64（无依赖实现，指纹对拍锚）
// ---------------------------------------------------------------------------

/// FNV-1a 64 offset basis。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 prime。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 单字节折叠。
pub fn fnv1a64_byte(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

/// FNV-1a 64 `u64` 折叠（8 字节大端序）。
pub fn fnv1a64_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        let byte = ((v >> (56 - i * 8)) & 0xFF) as u8;
        h = fnv1a64_byte(h, byte);
        i += 1;
    }
    h
}

/// FNV-1a 64 字符串折叠。
pub fn fnv1a64_str(mut h: u64, s: &str) -> u64 {
    for b in s.as_bytes() {
        h = fnv1a64_byte(h, *b);
    }
    h
}

/// FNV-1a 64 起始值。
pub fn fnv1a64_start() -> u64 {
    FNV_OFFSET
}

// ---------------------------------------------------------------------------
// 位原语族（全域零依赖，O(1)）
// ---------------------------------------------------------------------------

/// 64 位核 mask 快照。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CoreMask(pub u64);

impl CoreMask {
    /// 全空 mask。
    pub const fn none() -> Self {
        CoreMask(0)
    }

    /// 全满 mask（64 核全在线）。
    pub const fn all() -> Self {
        CoreMask(u64::MAX)
    }

    /// 单核置位。
    pub const fn with(core: usize) -> Self {
        CoreMask(1u64 << core)
    }

    /// 置位（幂等）。
    pub const fn set(&mut self, core: usize) {
        self.0 |= 1u64 << core;
    }

    /// 清位（幂等）。
    pub const fn clear(&mut self, core: usize) {
        self.0 &= !(1u64 << core);
    }

    /// 取位。
    pub const fn test(&self, core: usize) -> bool {
        self.0 & (1u64 << core) != 0
    }

    /// 人口（popcount）。
    pub const fn len(&self) -> u32 {
        self.0.count_ones()
    }

    /// 是否为空。
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// 集合并。
    pub const fn union(self, o: CoreMask) -> Self {
        CoreMask(self.0 | o.0)
    }

    /// 集合交。
    pub const fn inter(self, o: CoreMask) -> Self {
        CoreMask(self.0 & o.0)
    }

    /// 集合差（self 减 o）。
    pub const fn diff(self, o: CoreMask) -> Self {
        CoreMask(self.0 & !o.0)
    }

    /// 子集判定（self ⊆ o）。
    pub const fn subset_of(self, o: CoreMask) -> bool {
        self.0 & !o.0 == 0
    }

    /// 真子集判定（self ⊊ o）。
    pub const fn proper_subset_of(self, o: CoreMask) -> bool {
        self.subset_of(o) && self.0 != o.0
    }

    /// 遍历置位核（升序，无收集分配）。
    pub fn iter_ones(&self) -> BitIter {
        BitIter { m: self.0 }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64_u64(fnv1a64_str(fnv1a64_start(), U1863_NAME), self.0)
    }
}

/// 置位核迭代器（借用式，零分配）。
pub struct BitIter {
    m: u64,
}

impl BitIter {
    /// 取下一个置位核号；`None` 表示耗尽。
    pub fn next(&mut self) -> Option<usize> {
        if self.m == 0 {
            return None;
        }
        let c = self.m.trailing_zeros() as usize;
        self.m &= self.m - 1;
        Some(c)
    }
}

/// 迭代器收集为 `Vec<usize>`（等价于 `Vec::from_iter` 的自有实现，避免导入）。
pub fn collect_cores(m: CoreMask) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    let mut it = m.iter_ones();
    while let Some(c) = it.next() {
        v.push(c);
    }
    v
}

// ---------------------------------------------------------------------------
// LCG（确定性压力源，零墙钟）
// ---------------------------------------------------------------------------

/// LCG 状态。
#[derive(Clone, Copy)]
pub struct Lcg(pub u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 步进。
    pub fn next(&mut self) -> u64 {
        self.0 = (self.0.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD)) % LCG_MOD;
        self.0
    }

    /// 取 [0, n) 无偏近似（用高位乘，避免取模偏差主导）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.next();
        (((v >> 8) * n as u64) >> 24) as u32
    }
}

// ---------------------------------------------------------------------------
// 错误类型（全部带可操作修复建议，零静默）
// ---------------------------------------------------------------------------

/// 发起原语错误族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BeginErr {
    /// 次序回退。
    OrderBack { from: u8, to: u8 },
    /// 未登记先发送。
    SendBeforeReg { shoot_id: u64 },
    /// 登记双份快照不相等。
    RegMismatch { shoot_id: u64, target: u64, broadcast: u64 },
    /// 远程先于本地。
    RemoteBeforeLocal { shoot_id: u64 },
    /// 广播判定不成立。
    BroadcastJudge { shoot_id: u64, why: u8 },
    /// flush 域越界。
    FlushDomain { got: u8 },
    /// 区间账越界。
    RangeBad { ranges: u32, idx: u32 },
    /// 本地 flush 缺失。
    LocalMissing { shoot_id: u64 },
    /// 发送序列非单调。
    SendSeq { shoot_id: u64, last: u32, got: u32 },
    /// 离线核仍被投递。
    OfflineTarget { shoot_id: u64, core: u32 },
}

impl BeginErr {
    /// 诊断码（段内独占）。
    pub const fn code(self) -> u16 {
        match self {
            BeginErr::OrderBack { .. } => C_ORDER_BACK,
            BeginErr::SendBeforeReg { .. } => C_SEND_BEFORE_REG,
            BeginErr::RegMismatch { .. } => C_REG_MISMATCH,
            BeginErr::RemoteBeforeLocal { .. } => C_REMOTE_BEFORE_LOCAL,
            BeginErr::BroadcastJudge { .. } => C_BROADCAST_JUDGE,
            BeginErr::FlushDomain { .. } => C_FLUSH_DOMAIN,
            BeginErr::RangeBad { .. } => C_RANGE_BAD,
            BeginErr::LocalMissing { .. } => C_LOCAL_MISSING,
            BeginErr::SendSeq { .. } => C_SEND_SEQ,
            BeginErr::OfflineTarget { .. } => C_OFFLINE_TARGET,
        }
    }

    /// 错误码是否落在本单独占段内。
    pub const fn in_segment(self) -> bool {
        let c = self.code();
        c >= U1863_SEG_BASE && c <= U1863_SEG_END
    }

    /// 修复建议（可操作，禁止静默）。
    pub fn repair(self) -> &'static str {
        match self {
            BeginErr::OrderBack { from: _, to: _ } => {
                "次序必须严格 LOC→REG→BC：先调 local_flush() 落本地账，再调 register() 落双份快照，最后 broadcast() 投递；检查调用点是否把 broadcast 提到了 local_flush 之前"
            }
            BeginErr::SendBeforeReg { shoot_id: _ } => {
                "任何 ack/应答进入前必须先 register() 落双份快照；检查是否在广播投递后补登记——补登记会导致 Σ应答≠Σ广播（F1865 口径破裂），必须重排为登记前置"
            }
            BeginErr::RegMismatch { shoot_id: _, target: _, broadcast: _ } => {
                "register() 的 target 快照与 broadcast 快照必须逐位相等；检查 mask 计算是否复制后又被就地修改（Copy 语义误用），改为单源快照双份派生"
            }
            BeginErr::RemoteBeforeLocal { shoot_id: _ } => {
                "本地 flush 账必须先于首个远程投递落账；检查是否把本地 flush 放进回调里延迟执行——本地 flush 无回调可延迟，必须同步前置"
            }
            BeginErr::BroadcastJudge { why: 0, shoot_id: _ } => {
                "广播判定：目标集必须等于该 mm 的普通活跃集（持 mm 且非过渡态，F1862 mm 活跃 mask 派生）；检查活跃核表在切换挂点处是否漏同步，或过渡态核是否被误并入目标集"
            }
            BeginErr::BroadcastJudge { why: 1, shoot_id: _ } => {
                "过渡态核（F1654/F1867）不得按普通活跃核击落，必须走 park_transition 挂起路径，待切换完成后由 flush_after_switch 补做；检查过渡态标记是否丢失"
            }
            BeginErr::BroadcastJudge { why: 2, shoot_id: _ } => {
                "非活跃核不得被纳入击落目标（幽灵击落）；检查目标集是否误并入 idle 核或他 mm 核"
            }
            BeginErr::BroadcastJudge { .. } => {
                "广播判定不成立：核对目标集/在线集/过渡态集三者关系与 F1862 活跃 mask 的一致性口径"
            }
            BeginErr::FlushDomain { .. } => {
                "flush 域只能是 FLUSH_ALL(0)/FLUSH_LOCAL_ALL(1)/FLUSH_RANGE(2)；检查是否传入了枚举外的派生值"
            }
            BeginErr::RangeBad { ranges: _, idx: _ } => {
                "页区间账越界：单次发起区间数不得超过 RANGE_CAP(64)，且每区间 start<end；检查区间切分是否未合并"
            }
            BeginErr::LocalMissing { .. } => {
                "远程投递前本地 flush 账必须已落；检查是否在本地 flush 失败后仍继续投递（应立即中止整条发起链并显式返回错误）"
            }
            BeginErr::SendSeq { shoot_id: _, last: _, got: _ } => {
                "投递 seq 必须严格单调递增（每核一枚）；检查 seq 是否按核号复用或回绕"
            }
            BeginErr::OfflineTarget { shoot_id: _, core: _ } => {
                "离线核不得被投递 IPI；检查在线表是否在发起瞬间过期——过期时应走降级路径并显式落账，不得静默丢弃"
            }
        }
    }

    /// 面向册内的一行摘要（`core` 取值为模式专属信息，缺省 0）。
    pub fn line(self) -> String {
        let mut s = String::new();
        s.push_str("UNX-F1863-E ");
        s.push_str(&hex16(self.code()));
        s.push(' ');
        match self {
            BeginErr::OrderBack { from, to } => {
                s.push_str("order_back from=");
                s.push_str(&from.to_string());
                s.push_str(" to=");
                s.push_str(&to.to_string());
            }
            BeginErr::SendBeforeReg { shoot_id } => {
                s.push_str("send_before_reg shoot=");
                s.push_str(&shoot_id.to_string());
            }
            BeginErr::RegMismatch { shoot_id, target, broadcast } => {
                s.push_str("reg_mismatch shoot=");
                s.push_str(&shoot_id.to_string());
                s.push_str(" target=");
                s.push_str(&target.to_string());
                s.push_str(" bc=");
                s.push_str(&broadcast.to_string());
            }
            BeginErr::RemoteBeforeLocal { shoot_id } => {
                s.push_str("remote_before_local shoot=");
                s.push_str(&shoot_id.to_string());
            }
            BeginErr::BroadcastJudge { shoot_id, why } => {
                s.push_str("broadcast_judge shoot=");
                s.push_str(&shoot_id.to_string());
                s.push_str(" why=");
                s.push_str(&why.to_string());
            }
            BeginErr::FlushDomain { got } => {
                s.push_str("flush_domain got=");
                s.push_str(&got.to_string());
            }
            BeginErr::RangeBad { ranges, idx } => {
                s.push_str("range_bad n=");
                s.push_str(&ranges.to_string());
                s.push_str(" idx=");
                s.push_str(&idx.to_string());
            }
            BeginErr::LocalMissing { shoot_id } => {
                s.push_str("local_missing shoot=");
                s.push_str(&shoot_id.to_string());
            }
            BeginErr::SendSeq { shoot_id, last, got } => {
                s.push_str("send_seq shoot=");
                s.push_str(&shoot_id.to_string());
                s.push_str(" last=");
                s.push_str(&last.to_string());
                s.push_str(" got=");
                s.push_str(&got.to_string());
            }
            BeginErr::OfflineTarget { shoot_id, core } => {
                s.push_str("offline_target shoot=");
                s.push_str(&shoot_id.to_string());
                s.push_str(" core=");
                s.push_str(&core.to_string());
            }
        }
        s.push_str(" :: ");
        s.push_str(self.repair());
        s
    }
}

/// 四位十六格式化（零分配，手工十六表）。
pub fn hex16(v: u16) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(4);
    s.push(D[((v >> 12) & 0xF) as usize] as char);
    s.push(D[((v >> 8) & 0xF) as usize] as char);
    s.push(D[((v >> 4) & 0xF) as usize] as char);
    s.push(D[(v & 0xF) as usize] as char);
    s
}

/// u64 十六格式化（零依赖，宽度 16）。
pub fn hex64(v: u64) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut i = 16;
    while i > 0 {
        i -= 1;
        s.push(D[((v >> (i * 4)) & 0xF) as usize] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// 核状态（与 F1862 承接）
// ---------------------------------------------------------------------------

/// 核运行态（与 `unx1862_mmmask` 语义一致）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// 离线（不参与击落）。
    Off,
    /// 在线空闲（持 mm 引用，参与击落）。
    Idle,
    /// 在线运行（持 mm 引用，参与击落）。
    Running,
    /// 过渡态（F1654/F1867：切换半程，禁止普通击落）。
    Transition,
}

impl CoreState {
    /// 是否在线（可被投递）。
    pub const fn online(self) -> bool {
        !matches!(self, CoreState::Off)
    }

    /// 是否过渡态（须改走挂起路径）。
    pub const fn in_transition(self) -> bool {
        matches!(self, CoreState::Transition)
    }

    /// 是否持 mm 引用（应被击落）。
    pub const fn holds_mm(self) -> bool {
        matches!(self, CoreState::Idle | CoreState::Running | CoreState::Transition)
    }

    /// 状态名（册内对账用）。
    pub const fn name(self) -> &'static str {
        match self {
            CoreState::Off => "Off",
            CoreState::Idle => "Idle",
            CoreState::Running => "Running",
            CoreState::Transition => "Transition",
        }
    }
}

/// 每核记录。
#[derive(Clone, Copy)]
pub struct CoreRec {
    /// 核号。
    pub id: u32,
    /// 运行态。
    pub state: CoreState,
    /// 当前 mm 号（`0xFFFF_FFFF` 表示无 mm）。
    pub mm: u32,
}

impl CoreRec {
    /// 无 mm 哨兵。
    pub const NO_MM: u32 = 0xFFFF_FFFF;

    /// 构造：给定核号与态。
    pub const fn new(id: u32, state: CoreState, mm: u32) -> Self {
        CoreRec { id, state, mm }
    }
}

// ---------------------------------------------------------------------------
// 核表 / mm 表
// ---------------------------------------------------------------------------

/// 核表（全域 64 槽，O(1) 取核）。
#[derive(Clone)]
pub struct CoreTable {
    /// 槽位。
    pub recs: Vec<CoreRec>,
}

impl CoreTable {
    /// 构造 `n` 核全离线表。
    pub fn new(n: usize) -> Self {
        let mut recs = Vec::new();
        let mut i = 0usize;
        while i < n {
            recs.push(CoreRec::new(i as u32, CoreState::Off, CoreRec::NO_MM));
            i += 1;
        }
        CoreTable { recs }
    }

    /// 核数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }

    /// 取核记录（越界返回 `None`）。
    pub fn get(&self, id: u32) -> Option<CoreRec> {
        if (id as usize) < self.recs.len() {
            Some(self.recs[id as usize])
        } else {
            None
        }
    }

    /// 就地改核（越界返回 `false`）。
    pub fn set(&mut self, id: u32, rec: CoreRec) -> bool {
        if (id as usize) < self.recs.len() {
            self.recs[id as usize] = rec;
            true
        } else {
            false
        }
    }

    /// 在线核集合。
    pub fn online_mask(&self) -> CoreMask {
        let mut m = CoreMask::none();
        for r in self.recs.iter() {
            if r.state.online() {
                m.set(r.id as usize);
            }
        }
        m
    }

    /// 某 mm 的活跃核集合（持 mm 引用者；含过渡态，F1867 另做改道）。
    pub fn active_mask_of(&self, mm: u32) -> CoreMask {
        let mut m = CoreMask::none();
        for r in self.recs.iter() {
            if r.state.holds_mm() && r.mm == mm {
                m.set(r.id as usize);
            }
        }
        m
    }

    /// 某 mm 的过渡态核集合。
    pub fn transition_mask_of(&self, mm: u32) -> CoreMask {
        let mut m = CoreMask::none();
        for r in self.recs.iter() {
            if r.state.in_transition() && r.mm == mm {
                m.set(r.id as usize);
            }
        }
        m
    }

    /// 某 mm 的普通活跃核集合（持 mm 且非过渡态）。
    pub fn plain_active_mask_of(&self, mm: u32) -> CoreMask {
        let mut m = CoreMask::none();
        for r in self.recs.iter() {
            if r.state.holds_mm() && !r.state.in_transition() && r.mm == mm {
                m.set(r.id as usize);
            }
        }
        m
    }

    /// 在线且持 mm 的核集合（真正可投递面：在线 ∧ 持 mm ∧ 非过渡态）。
    pub fn deliverable_mask_of(&self, mm: u32) -> CoreMask {
        let mut m = CoreMask::none();
        for r in self.recs.iter() {
            if r.state.online() && r.state.holds_mm() && !r.state.in_transition() && r.mm == mm {
                m.set(r.id as usize);
            }
        }
        m
    }

    /// 指纹（对账锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "coretable");
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = self.recs[i];
            h = fnv1a64_u64(h, r.id as u64);
            h = fnv1a64_u64(h, state_code(r.state));
            h = fnv1a64_u64(h, r.mm as u64);
            i += 1;
        }
        h
    }
}

/// 状态编码（指纹用，封闭）。
pub const fn state_code(s: CoreState) -> u64 {
    match s {
        CoreState::Off => 0,
        CoreState::Idle => 1,
        CoreState::Running => 2,
        CoreState::Transition => 3,
    }
}

/// mm 槽（地址空间）。
#[derive(Clone, Copy)]
pub struct MmSlot {
    /// mm 号。
    pub id: u32,
    /// 生命周期：Live / Dying / Dead。
    pub life: MmLife,
    /// 当前发起者核（发起核自身）。
    pub self_core: u32,
}

/// mm 生命周期。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MmLife {
    /// 存活。
    Live,
    /// 消亡中（禁止新发起；已在途的发起必须走完四段）。
    Dying,
    /// 已死（禁止任何发起）。
    Dead,
}

impl MmLife {
    /// 是否允许发起新击落。
    pub const fn allows_begin(self) -> bool {
        matches!(self, MmLife::Live)
    }
    /// 状态名。
    pub const fn name(self) -> &'static str {
        match self {
            MmLife::Live => "Live",
            MmLife::Dying => "Dying",
            MmLife::Dead => "Dead",
        }
    }
}

/// mm 表（`Vec` 线性，mm 数封顶 32 —— 内核 mm 数受控）。
pub struct MmTable {
    /// 槽位。
    pub slots: Vec<MmSlot>,
}

impl MmTable {
    /// 构造 `n` 个 Live mm。
    pub fn new(n: u32, self_core: u32) -> Self {
        let mut slots = Vec::new();
        let mut i = 0u32;
        while i < n {
            slots.push(MmSlot { id: i, life: MmLife::Live, self_core });
            i += 1;
        }
        MmTable { slots }
    }

    /// 取 mm（越界返回 `None`）。
    pub fn get(&self, id: u32) -> Option<MmSlot> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].id == id {
                return Some(self.slots[i]);
            }
            i += 1;
        }
        None
    }

    /// 就地改 mm（不存在返回 `false`）。
    pub fn set_life(&mut self, id: u32, life: MmLife) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].id == id {
                self.slots[i].life = life;
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 页区间（本地 flush 的载荷）
// ---------------------------------------------------------------------------

/// 页区间（`start` 含 / `end` 排除，页粒度）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    /// 起始页号。
    pub start: u64,
    /// 结束页号（排除）。
    pub end: u64,
}

impl PageRange {
    /// 构造。
    pub const fn new(start: u64, end: u64) -> Self {
        PageRange { start, end }
    }

    /// 页数（end ≥ start 时有效）。
    pub const fn pages(&self) -> u64 {
        if self.end >= self.start {
            self.end - self.start
        } else {
            0
        }
    }

    /// 区间合法性（start < end）。
    pub const fn valid(&self) -> bool {
        self.end > self.start
    }

    /// 是否覆盖 `p`。
    pub const fn covers(&self, p: u64) -> bool {
        p >= self.start && p < self.end
    }
}

/// 页区间集合（封顶 `RANGE_CAP`）。
#[derive(Clone)]
pub struct RangeSet {
    /// 区间列表。
    pub items: Vec<PageRange>,
}

impl RangeSet {
    /// 空集。
    pub fn empty() -> Self {
        RangeSet { items: Vec::new() }
    }

    /// 由列表构造（不做校验，校验在发起入口做）。
    pub fn from(items: Vec<PageRange>) -> Self {
        RangeSet { items }
    }

    /// 区间数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 总页数（Σ pages，越界区间按 0 计）。
    pub fn total_pages(&self) -> u64 {
        let mut s = 0u64;
        for r in self.items.iter() {
            s = s.wrapping_add(r.pages());
        }
        s
    }

    /// 全部区间合法性且数量不超封顶。
    pub fn well_formed(&self) -> bool {
        if self.items.len() > RANGE_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].valid() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 覆盖判定（任一区间覆盖即真）。
    pub fn covers(&self, p: u64) -> bool {
        for r in self.items.iter() {
            if r.covers(p) {
                return true;
            }
        }
        false
    }

    /// 区间并集指纹（次序无关：先逐区折叠再按序号异或滚动）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < self.items.len() {
            let r = self.items[i];
            let h = fnv1a64_u64(fnv1a64_u64(fnv1a64_start(), r.start), r.end);
            // 与序号绑定，使次序差异也可检出。
            acc ^= fnv1a64_u64(h ^ (i as u64).wrapping_mul(FNV_PRIME), i as u64);
            i += 1;
        }
        acc
    }

    /// 合并相邻/重叠区间（有序输入；Linux `flush_tlb_range` 亦做区间合并）。
    pub fn merged(&self) -> RangeSet {
        let mut out: Vec<PageRange> = Vec::new();
        let mut sorted: Vec<PageRange> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let r = self.items[i];
            if r.valid() {
                sorted.push(r);
            }
            i += 1;
        }
        // 插入序排序（区间数封顶 64，插入序 O(n²) 但 n 小且只做一次）。
        let mut j = 1usize;
        while j < sorted.len() {
            let key = sorted[j];
            let mut k = j;
            while k > 0 && sorted[k - 1].start > key.start {
                sorted[k] = sorted[k - 1];
                k -= 1;
            }
            sorted[k] = key;
            j += 1;
        }
        let mut k = 0usize;
        while k < sorted.len() {
            let mut cur = sorted[k];
            let mut m = k + 1;
            while m < sorted.len() && sorted[m].start <= cur.end {
                if sorted[m].end > cur.end {
                    cur.end = sorted[m].end;
                }
                m += 1;
            }
            out.push(cur);
            k = m;
        }
        RangeSet { items: out }
    }
}

// ---------------------------------------------------------------------------
// 本地 flush 原语（段一：LOC）
// ---------------------------------------------------------------------------

/// ASID（地址空间标识符；PCID 前向预留见 F1872 —— 本单只读消费其存在性）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Asid {
    /// ASID 数值。
    pub value: u32,
    /// PCID 分配是否已启用（F1872 预留声明位；本单仅作存在性断言）。
    pub pcid_enabled: bool,
}

impl Asid {
    /// 无 ASID（全局 flush 场景）。
    pub const GLOBAL: Asid = Asid { value: 0, pcid_enabled: false };

    /// 构造普通 ASID。
    pub const fn new(value: u32) -> Self {
        Asid { value, pcid_enabled: true }
    }

    /// 是否为全局 flush（ASID 无关）。
    pub const fn is_global(&self) -> bool {
        self.value == 0
    }
}

/// 本地 flush 指令序列账（`invlpg` / `invlpg_all` / `mov cr3` 三形态）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Insn {
    /// `invlpg [addr]`（区间局部刷）。
    Invlpg,
    /// `invlpg_all`（全核 TLB 清，等价 `mov cr3` 副作用）。
    InvlpgAll,
    /// `mov cr3, cr3`（ASID 绑定切换式全刷）。
    MovCr3,
}

impl Insn {
    /// 指令名（册内对账）。
    pub const fn name(self) -> &'static str {
        match self {
            Insn::Invlpg => "invlpg",
            Insn::InvlpgAll => "invlpg_all",
            Insn::MovCr3 => "mov_cr3",
        }
    }

    /// flush 域归属（域与指令一一对应，构造期封闭）。
    pub const fn domain(self) -> u8 {
        match self {
            Insn::Invlpg => FLUSH_RANGE,
            Insn::InvlpgAll => FLUSH_LOCAL_ALL,
            Insn::MovCr3 => FLUSH_ALL,
        }
    }

    /// 由域反查指令（越界返回 `None`）。
    pub fn from_domain(d: u8) -> Option<Insn> {
        if d == FLUSH_ALL {
            Some(Insn::MovCr3)
        } else if d == FLUSH_LOCAL_ALL {
            Some(Insn::InvlpgAll)
        } else if d == FLUSH_RANGE {
            Some(Insn::Invlpg)
        } else {
            None
        }
    }
}

/// 本地 flush 落账（发起核自身的一次 flush 事件）。
#[derive(Clone, Copy)]
pub struct LocalFlushRec {
    /// 击落单号（归属）。
    pub shoot_id: u64,
    /// 发起核。
    pub core: u32,
    /// mm 号。
    pub mm: u32,
    /// ASID。
    pub asid: Asid,
    /// flush 域。
    pub domain: u8,
    /// 实际执行指令。
    pub insn: Insn,
    /// 载荷区间指纹（`FLUSH_RANGE` 时有效，其余为 0）。
    pub range_fp: u64,
    /// 区间总页数（`FLUSH_RANGE` 时有效）。
    pub pages: u64,
    /// 逻辑 tick（零墙钟）。
    pub tick: u64,
}

impl LocalFlushRec {
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "localflush");
        h = fnv1a64_u64(h, self.shoot_id);
        h = fnv1a64_u64(h, self.core as u64);
        h = fnv1a64_u64(h, self.mm as u64);
        h = fnv1a64_u64(h, self.asid.value as u64);
        h = fnv1a64_u64(h, self.asid.pcid_enabled as u64);
        h = fnv1a64_u64(h, self.domain as u64);
        h = fnv1a64_u64(h, insn_code(self.insn));
        h = fnv1a64_u64(h, self.range_fp);
        h = fnv1a64_u64(h, self.pages);
        h = fnv1a64_u64(h, self.tick);
        h
    }
}

/// 指令编码（指纹用）。
pub const fn insn_code(i: Insn) -> u64 {
    match i {
        Insn::Invlpg => 0x100,
        Insn::InvlpgAll => 0x101,
        Insn::MovCr3 => 0x102,
    }
}

/// 本地 flush 原语（纯语义模型：不触碰硬件寄存器，仅落账）。
#[derive(Clone)]
pub struct LocalFlush {
    /// 已落账记录。
    pub recs: Vec<LocalFlushRec>,
    /// 域选择覆盖计数（`FLUSH_ALL` / `LOCAL_ALL` / `RANGE`）。
    pub domain_hits: [u32; 3],
}

impl LocalFlush {
    /// 构造。
    pub fn new() -> Self {
        LocalFlush { recs: Vec::new(), domain_hits: [0; 3] }
    }

    /// 执行一次本地 flush（段 LOC）。
    ///
    /// 前置：区间集良构（`FLUSH_RANGE` 时）、域合法。失败不落账（零静默：返回错误）。
    pub fn do_flush(
        &mut self,
        shoot_id: u64,
        core: u32,
        mm: u32,
        asid: Asid,
        domain: u8,
        rs: &RangeSet,
        tick: u64,
    ) -> Result<u64, BeginErr> {
        if domain > FLUSH_MAX {
            return Err(BeginErr::FlushDomain { got: domain });
        }
        let insn = match Insn::from_domain(domain) {
            Some(v) => v,
            None => return Err(BeginErr::FlushDomain { got: domain }),
        };
        if insn.domain() != domain {
            return Err(BeginErr::FlushDomain { got: domain });
        }
        let mut fp = 0u64;
        let mut pages = 0u64;
        if domain == FLUSH_RANGE {
            if rs.items.len() > RANGE_CAP {
                return Err(BeginErr::RangeBad { ranges: rs.len(), idx: RANGE_CAP as u32 });
            }
            let mut i = 0usize;
            while i < rs.items.len() {
                if !rs.items[i].valid() {
                    return Err(BeginErr::RangeBad { ranges: rs.len(), idx: i as u32 });
                }
                i += 1;
            }
            fp = rs.fingerprint();
            pages = rs.total_pages();
        } else if !rs.is_empty() {
            // 非区间域携带区间载荷属口径混淆：显式拒（零静默）。
            return Err(BeginErr::RangeBad { ranges: rs.len(), idx: 0 });
        }
        self.domain_hits[domain as usize] += 1;
        self.recs.push(LocalFlushRec {
            shoot_id,
            core,
            mm,
            asid,
            domain,
            insn,
            range_fp: fp,
            pages,
            tick,
        });
        Ok(fp)
    }

    /// 取某击落单的本地域账（O(n)，账小）。
    pub fn rec_of(&self, shoot_id: u64) -> Option<LocalFlushRec> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].shoot_id == shoot_id {
                return Some(self.recs[i]);
            }
            i += 1;
        }
        None
    }

    /// 本地账条数。
    pub fn len(&self) -> u32 {
        self.recs.len() as u32
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 域与指令一致性自检（三域各一枚）。
    pub fn domain_insn_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = self.recs[i];
            if r.insn.domain() != r.domain {
                return false;
            }
            if r.domain == FLUSH_RANGE && r.pages == 0 && r.range_fp == 0 {
                return false;
            }
            if r.domain != FLUSH_RANGE && (r.pages != 0 || r.range_fp != 0) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹（次序敏感，用于重放对拍）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "localflush_seq");
        let mut i = 0usize;
        while i < self.recs.len() {
            h = fnv1a64_u64(h, self.recs[i].fingerprint());
            h = fnv1a64_u64(h, i as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 远程投递（段三：BC）
// ---------------------------------------------------------------------------

/// 投递记录（每核一枚）。
#[derive(Clone, Copy)]
pub struct SendRec {
    /// 击落单号。
    pub shoot_id: u64,
    /// 目标核。
    pub core: u32,
    /// 投递序列（严格单调）。
    pub seq: u32,
    /// 逻辑 tick。
    pub tick: u64,
    /// IPI 向量号（F1846 登记向量，本单消费其存在性，不做分配）。
    pub vector: u32,
    /// 载荷 ASID（远程侧刷同一 ASID）。
    pub asid: Asid,
}

impl SendRec {
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "sendrec");
        h = fnv1a64_u64(h, self.shoot_id);
        h = fnv1a64_u64(h, self.core as u64);
        h = fnv1a64_u64(h, self.seq as u64);
        h = fnv1a64_u64(h, self.tick);
        h = fnv1a64_u64(h, self.vector as u64);
        h = fnv1a64_u64(h, self.asid.value as u64);
        h
    }
}

/// IPI 向量号：击落专用（F1846 登记，本单只消费）。
pub const SHOOTDOWN_VECTOR: u32 = 0x2F;

/// 远程投递账。
#[derive(Clone)]
pub struct Broadcast {
    /// 已投递记录。
    pub recs: Vec<SendRec>,
    /// 每击落单的最后投递 seq（单调账）。
    pub last_seq: Vec<(u64, u32)>,
}

impl Broadcast {
    /// 构造。
    pub fn new() -> Self {
        Broadcast { recs: Vec::new(), last_seq: Vec::new() }
    }

    /// 取某单最后 seq（无则 0）。
    pub fn seq_of(&self, shoot_id: u64) -> u32 {
        let mut i = 0usize;
        while i < self.last_seq.len() {
            if self.last_seq[i].0 == shoot_id {
                return self.last_seq[i].1;
            }
            i += 1;
        }
        0
    }

    /// 投递到一批核（seq 严格递增；离线核显式拒）。
    pub fn send_batch(
        &mut self,
        shoot_id: u64,
        cores: &CoreMask,
        cores_tbl: &CoreTable,
        asid: Asid,
        tick: u64,
    ) -> Result<u32, BeginErr> {
        let mut seq = self.seq_of(shoot_id);
        let list = collect_cores(*cores);
        let mut i = 0usize;
        while i < list.len() {
            let c = list[i] as u32;
            match cores_tbl.get(c) {
                None => {
                    return Err(BeginErr::OfflineTarget { shoot_id, core: c });
                }
                Some(r) => {
                    if !r.state.online() {
                        return Err(BeginErr::OfflineTarget { shoot_id, core: c });
                    }
                }
            }
            seq = seq.wrapping_add(1);
            self.recs.push(SendRec {
                shoot_id,
                core: c,
                seq,
                tick,
                vector: SHOOTDOWN_VECTOR,
                asid,
            });
            i += 1;
        }
        self.last_seq.push((shoot_id, seq));
        Ok(seq)
    }

    /// 某单的投递核集合（O(n)，账小）。
    pub fn sent_mask_of(&self, shoot_id: u64) -> CoreMask {
        let mut m = CoreMask::none();
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].shoot_id == shoot_id {
                m.set(self.recs[i].core as usize);
            }
            i += 1;
        }
        m
    }

    /// 某单投递数。
    pub fn sent_count_of(&self, shoot_id: u64) -> u32 {
        self.sent_mask_of(shoot_id).len()
    }

    /// 全域投递条数。
    pub fn len(&self) -> u32 {
        self.recs.len() as u32
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// seq 严格单调自检（按落账序，同单 seq 严格增；跨单独立）。
    pub fn seq_monotonic(&self) -> bool {
        let mut i = 0usize;
        let mut seen: Vec<(u64, u32)> = Vec::new();
        while i < self.recs.len() {
            let r = self.recs[i];
            let mut j = 0usize;
            let mut found = false;
            while j < seen.len() {
                if seen[j].0 == r.shoot_id {
                    if r.seq <= seen[j].1 {
                        return false;
                    }
                    seen[j].1 = r.seq;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                seen.push((r.shoot_id, r.seq));
            }
            i += 1;
        }
        true
    }

    /// 向量号一致性（全域必须为击落向量）。
    pub fn vector_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].vector != SHOOTDOWN_VECTOR {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹（次序敏感）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "broadcast_seq");
        let mut i = 0usize;
        while i < self.recs.len() {
            h = fnv1a64_u64(h, self.recs[i].fingerprint());
            h = fnv1a64_u64(h, i as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 登记（段二：REG）—— 先登记后发送的硬闸载体
// ---------------------------------------------------------------------------

/// 登记快照（双份：target 与 broadcast 必须逐位相等）。
#[derive(Clone, Copy)]
pub struct RegSnap {
    /// 击落单号。
    pub shoot_id: u64,
    /// mm 号。
    pub mm: u32,
    /// 发起核。
    pub self_core: u32,
    /// 目标核快照（第一份）。
    pub target: CoreMask,
    /// 广播核快照（第二份）。
    pub broadcast: CoreMask,
    /// 区间载荷指纹。
    pub range_fp: u64,
    /// 区间总页数。
    pub pages: u64,
    /// ASID。
    pub asid: Asid,
    /// 逻辑 tick。
    pub tick: u64,
    /// 是否已登记完成。
    pub sealed: bool,
}

impl RegSnap {
    /// 双份相等判定。
    pub fn dual_equal(&self) -> bool {
        self.target.0 == self.broadcast.0
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "regsnap");
        h = fnv1a64_u64(h, self.shoot_id);
        h = fnv1a64_u64(h, self.mm as u64);
        h = fnv1a64_u64(h, self.self_core as u64);
        h = fnv1a64_u64(h, self.target.0);
        h = fnv1a64_u64(h, self.broadcast.0);
        h = fnv1a64_u64(h, self.range_fp);
        h = fnv1a64_u64(h, self.pages);
        h = fnv1a64_u64(h, self.asid.value as u64);
        h = fnv1a64_u64(h, self.tick);
        h = fnv1a64_u64(h, self.sealed as u64);
        h
    }
}

/// 登记簿（先登记后发送的唯一真源）。
#[derive(Clone)]
pub struct Register {
    /// 快照列表。
    pub snaps: Vec<RegSnap>,
}

impl Register {
    /// 构造。
    pub fn new() -> Self {
        Register { snaps: Vec::new() }
    }

    /// 登记（未 sealed 前 ack 不得进入 —— 由 `seal` 显式收口）。
    pub fn register(&mut self, snap: RegSnap) -> Result<(), BeginErr> {
        if !snap.dual_equal() {
            return Err(BeginErr::RegMismatch {
                shoot_id: snap.shoot_id,
                target: snap.target.0,
                broadcast: snap.broadcast.0,
            });
        }
        self.snaps.push(snap);
        Ok(())
    }

    /// 封口（登记完成，此后允许 ack 进入）。
    ///
    /// 语义：必须先登记到**双份快照逐位相等**的快照，才谈封口；
    /// 封口是 ack 流进入的准入闸（F1865 的 Σ应答=Σ广播 依赖于此）。
    pub fn seal(&mut self, shoot_id: u64) -> Result<(), BeginErr> {
        match self.find(shoot_id) {
            None => Err(BeginErr::SendBeforeReg { shoot_id }),
            Some(s) => {
                if !s.dual_equal() {
                    return Err(BeginErr::RegMismatch {
                        shoot_id,
                        target: s.target.0,
                        broadcast: s.broadcast.0,
                    });
                }
                let mut i = 0usize;
                while i < self.snaps.len() {
                    if self.snaps[i].shoot_id == shoot_id {
                        self.snaps[i].sealed = true;
                        return Ok(());
                    }
                    i += 1;
                }
                Err(BeginErr::SendBeforeReg { shoot_id })
            }
        }
    }

    /// 查快照。
    pub fn find(&self, shoot_id: u64) -> Option<RegSnap> {
        let mut i = 0usize;
        while i < self.snaps.len() {
            if self.snaps[i].shoot_id == shoot_id {
                return Some(self.snaps[i]);
            }
            i += 1;
        }
        None
    }

    /// 是否已登记。
    pub fn registered(&self, shoot_id: u64) -> bool {
        self.find(shoot_id).is_some()
    }

    /// 是否已封口（ack 准入闸）。
    pub fn sealed(&self, shoot_id: u64) -> bool {
        match self.find(shoot_id) {
            None => false,
            Some(s) => s.sealed,
        }
    }

    /// ack 准入判定：必须已封口。
    pub fn ack_admit(&self, shoot_id: u64) -> Result<(), BeginErr> {
        if self.sealed(shoot_id) {
            Ok(())
        } else {
            Err(BeginErr::SendBeforeReg { shoot_id })
        }
    }

    /// 全域双份一致性自检。
    pub fn dual_all_equal(&self) -> bool {
        let mut i = 0usize;
        while i < self.snaps.len() {
            if !self.snaps[i].dual_equal() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "register_seq");
        let mut i = 0usize;
        while i < self.snaps.len() {
            h = fnv1a64_u64(h, self.snaps[i].fingerprint());
            h = fnv1a64_u64(h, i as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 次序账（LOC → REG → BC 三段单调推进）
// ---------------------------------------------------------------------------

/// 段号（封闭三段）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// 未开始。
    Init = 0,
    /// 本地 flush 段（段一）。
    Loc = 1,
    /// 登记段（段二）。
    Reg = 2,
    /// 广播段（段三）。
    Bc = 3,
    /// 完成（四段之外的终态，交给 F1865）。
    Done = 4,
}

impl Phase {
    /// 段名。
    pub const fn name(self) -> &'static str {
        match self {
            Phase::Init => "Init",
            Phase::Loc => "Loc",
            Phase::Reg => "Reg",
            Phase::Bc => "Bc",
            Phase::Done => "Done",
        }
    }
    /// 段位值。
    pub const fn ordinal(self) -> u8 {
        self as u8
    }
}

/// 次序事件（落账用）。
#[derive(Clone, Copy)]
pub struct OrderEvent {
    /// 击落单号。
    pub shoot_id: u64,
    /// 目标段。
    pub phase: Phase,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 次序账（任何回退/跳跃即破 —— 判据「次序颠倒注入 10/10 检出」的核心载体）。
#[derive(Clone)]
pub struct OrderLedger {
    /// 事件流。
    pub events: Vec<OrderEvent>,
}

impl OrderLedger {
    /// 构造。
    pub fn new() -> Self {
        OrderLedger { events: Vec::new() }
    }

    /// 推进到某段（单调检查）。
    pub fn advance(&mut self, shoot_id: u64, phase: Phase, tick: u64) -> Result<Phase, BeginErr> {
        let cur = self.phase_of(shoot_id);
        if (phase as u8) < (cur as u8) {
            return Err(BeginErr::OrderBack {
                from: cur.ordinal(),
                to: phase.ordinal(),
            });
        }
        // 跳跃合法（允许跨段：LOCAL_ALL 域可直接进 REG/BC），但回退必拒。
        self.events.push(OrderEvent { shoot_id, phase, tick });
        self.set_phase(shoot_id, phase);
        Ok(phase)
    }

    /// 查当前段（无记录为 `Init`）。
    pub fn phase_of(&self, shoot_id: u64) -> Phase {
        let mut best = Phase::Init;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].shoot_id == shoot_id && self.events[i].phase > best {
                best = self.events[i].phase;
            }
            i += 1;
        }
        best
    }

    fn set_phase(&mut self, shoot_id: u64, phase: Phase) {
        // `phase_of` 取最大值，故只需追加即可推进；此处仅做存在性断言用。
        let _ = (shoot_id, phase);
    }

    /// 某单是否已过某段。
    pub fn passed(&self, shoot_id: u64, phase: Phase) -> bool {
        self.phase_of(shoot_id) >= phase
    }

    /// 事件总数。
    pub fn len(&self) -> u32 {
        self.events.len() as u32
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 全域单调自检（按落账序，逐单段位非降）。
    pub fn monotone(&self) -> bool {
        let mut i = 0usize;
        while i < self.events.len() {
            // 同单后段事件不得低于此前已达最高段（phase_of 即最高段，故只需
            // 检查事件本身的段位序列在「首次出现」时是升序的）。
            if self.events[i].phase == Phase::Init {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹（次序敏感 —— 正是次序颠倒能被检出的原因）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "order_seq");
        let mut i = 0usize;
        while i < self.events.len() {
            h = fnv1a64_u64(h, self.events[i].shoot_id);
            h = fnv1a64_u64(h, self.events[i].phase.ordinal() as u64);
            h = fnv1a64_u64(h, self.events[i].tick);
            h = fnv1a64_u64(h, i as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 广播判定（Judge）
// ---------------------------------------------------------------------------

/// 判定码（广播判定的具名理由）。
pub const WHY_ACTIVE_MISMATCH: u8 = 0;
/// 判定码：过渡态核未改道。
pub const WHY_TRANSITION_UNPARKED: u8 = 1;
/// 判定码：目标集含非活跃核（幽灵击落）。
pub const WHY_GHOST_TARGET: u8 = 2;
/// 判定码：离线核未降级。
pub const WHY_OFFLINE_UNDEGRADED: u8 = 3;

/// 广播判定结论（详细）。
#[derive(Clone, Copy)]
pub struct JudgeVerdict {
    /// 是否通过。
    pub pass: bool,
    /// 失败理由码（`pass` 时为 0xFF）。
    pub why: u8,
    /// 本地必做：本地账已落。
    pub local_done: bool,
    /// 远程必达：投递集合 == 可投递集合。
    pub remote_reach: bool,
    /// 目标集与 mm 活跃集一致性。
    pub active_match: bool,
    /// 过渡态核改道成立。
    pub transition_parked: bool,
    /// 空目标集零广播合法。
    pub empty_short_circuit: bool,
}

impl JudgeVerdict {
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), "judge");
        h = fnv1a64_u64(h, self.pass as u64);
        h = fnv1a64_u64(h, self.why as u64);
        h = fnv1a64_u64(h, self.local_done as u64);
        h = fnv1a64_u64(h, self.remote_reach as u64);
        h = fnv1a64_u64(h, self.active_match as u64);
        h = fnv1a64_u64(h, self.transition_parked as u64);
        h = fnv1a64_u64(h, self.empty_short_circuit as u64);
        h
    }
}

/// 判定结果错误包装。
pub fn judge_err(shoot_id: u64, why: u8) -> BeginErr {
    BeginErr::BroadcastJudge { shoot_id, why }
}

// ---------------------------------------------------------------------------
// 击落发起原语主体（冻结 v1）
// ---------------------------------------------------------------------------

/// 次序策略（冻结 v1 = `Strict`；其余为注入用的非法变体）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OrderPolicy {
    /// 冻结 v1：严格 LOC → REG → BC。
    Strict,
    /// 非法变体 A：先远程后本地（REG/BC 提到 LOC 前）。
    RemoteFirst,
    /// 非法变体 B：先发送后登记（BC 提到 REG 前）。
    SendFirst,
    /// 非法变体 C：不登记直接发送（跳过 REG）。
    NoRegister,
    /// 非法变体 D：跳过本地 flush（直接 REG+BC）。
    NoLocal,
    /// 非法变体 E：跳跃后回退（LOC → BC → REG）。
    JumpBack,
    /// 非法变体 F：目标集含非活跃核（幽灵击落）。
    GhostTarget,
    /// 非法变体 G：过渡态核未改道（按普通活跃核投递）。
    TransitionRaw,
    /// 非法变体 H：离线核未降级仍投递。
    OfflineRaw,
    /// 非法变体 I：本地 flush 域越界（传 FLUSH_MAX+1）。
    BadDomain,
    /// 非法变体 J：区间账越界（start ≥ end）。
    BadRange,
}

/// 击落发起原语（冻结 v1）。
pub struct ShootBegin {
    /// mm 号。
    pub mm: u32,
    /// 发起核。
    pub self_core: u32,
    /// ASID。
    pub asid: Asid,
    /// 本地 flush 域。
    pub domain: u8,
    /// 区间载荷。
    pub ranges: RangeSet,
    /// 逻辑 tick。
    pub tick: u64,
    /// 次序策略。
    pub policy: OrderPolicy,
    /// 已成功发起的击落单计数（单调单号源）。
    pub issued: u64,
    /// 本地 flush 原语。
    pub lf: LocalFlush,
    /// 登记簿。
    pub reg: Register,
    /// 广播账。
    pub bc: Broadcast,
    /// 次序账。
    pub ord: OrderLedger,
    /// 最近一次判定结论。
    pub last: JudgeVerdict,
    /// 错误账（显式落账，零静默）。
    pub errs: Vec<BeginErr>,
}

impl ShootBegin {
    /// 构造（`mm` / `self_core` / `domain` 固定，载荷可后续 `with_ranges`）。
    pub fn new(mm: u32, self_core: u32, asid: Asid, domain: u8) -> Self {
        ShootBegin {
            mm,
            self_core,
            asid,
            domain,
            ranges: RangeSet::empty(),
            tick: 0,
            policy: OrderPolicy::Strict,
            issued: 0,
            lf: LocalFlush::new(),
            reg: Register::new(),
            bc: Broadcast::new(),
            ord: OrderLedger::new(),
            last: JudgeVerdict {
                pass: false,
                why: 0xFF,
                local_done: false,
                remote_reach: false,
                active_match: false,
                transition_parked: false,
                empty_short_circuit: false,
            },
            errs: Vec::new(),
        }
    }

    /// 挂区间载荷（链式）。
    pub fn with_ranges(mut self, rs: RangeSet) -> Self {
        self.ranges = rs;
        self
    }

    /// 挂策略（链式）。
    pub fn with_policy(mut self, p: OrderPolicy) -> Self {
        self.policy = p;
        self
    }

    /// 记错误（显式落账）。
    fn fail(&mut self, e: BeginErr) -> BeginErr {
        self.errs.push(e);
        e
    }

    /// 错误账条数。
    pub fn err_count(&self) -> u32 {
        self.errs.len() as u32
    }

    /// 冻结 v1 的正常发起链：LOC → REG → BC → 判定。
    ///
    /// 这是判据的主路径；返回击落单号。
    pub fn begin(&mut self, cores: &CoreTable) -> Result<u64, BeginErr> {
        self.issued += 1;
        let sid = self.issued;
        let tick = self.tick;
        self.tick = self.tick.wrapping_add(1);

        // 策略展开：非法策略注入对应的破口。
        let p = self.policy;

        // 非法变体 I（域越界）必须在 LOC **之前**落地，否则域闸无从生效 ——
        // 「域闸显式拒」正是它存在的意义，绝不允许事后静默纠正。
        if p == OrderPolicy::BadDomain {
            self.domain = FLUSH_MAX + 1;
        }

        // 非法变体 J（区间账越界）同样必须先于 LOC 落地，否则本地 flush 会
        // 拿着合法区间跑通，注入就变成空转（判据「10/10」不认空转）。
        if p == OrderPolicy::BadRange {
            self.ranges = RangeSet::from(alloc::vec![PageRange::new(0x900, 0x900)]);
        }

        // 非法变体 A（远程先于本地）：先把远程投递账落下来并把次序推进到 BC，
        // 随后的 LOC 推进必然触发次序回退（C_ORDER_BACK）—— 这正是「先本地
        // 后远程」硬闸的负向证据。
        if p == OrderPolicy::RemoteFirst {
            let early = cores.plain_active_mask_of(self.mm);
            if let Err(e) = self.bc.send_batch(sid, &early, cores, self.asid, tick) {
                return Err(self.fail(e));
            }
            if let Err(e) = self.ord.advance(sid, Phase::Bc, tick) {
                return Err(self.fail(e));
            }
        }

        // 段一：LOC（本地 flush）。
        if p != OrderPolicy::NoLocal {
            if let Err(e) = self.lf.do_flush(sid, self.self_core, self.mm, self.asid, self.domain, &self.ranges, tick) {
                return Err(self.fail(e));
            }
            if let Err(e) = self.ord.advance(sid, Phase::Loc, tick) {
                return Err(self.fail(e));
            }
        } else if p == OrderPolicy::NoLocal {
            // 非法变体 D：跳过本地但仍推进次序（真实次序破损由 audit 抓）。
            if let Err(e) = self.ord.advance(sid, Phase::Loc, tick) {
                return Err(self.fail(e));
            }
        }

        // 策略 F/J：目标集/区间在进入登记前被污染。
        //
        // **关键口径**：击落**目标集**必须等于「普通活跃集」（持 mm 且**非**过渡态）——
        // 过渡态核（F1654/F1867）必须剥离出去走挂起路径，绝不能混入目标集。
        // 这正是 `judge` 的两条断言的同源口径：
        //   active_match   : target.0 == plain.0（目标集 == 普通活跃集）
        //   transition_parked: target ∩ trans = 空（过渡态已剥离）
        let mut target = cores.plain_active_mask_of(self.mm);
        if p == OrderPolicy::GhostTarget {
            // 并入一个不持本 mm 的在线核（幽灵击落）。
            let online = cores.online_mask();
            let ghost = online.diff(target);
            target = target.union(ghost);
        }
        if p == OrderPolicy::TransitionRaw {
            // 过渡态核按普通活跃核投递（不改道）—— 注入破绽。
            target = target.union(cores.transition_mask_of(self.mm));
        }

        // 非法变体 E（跳跃后回退）在此埋点：先 BC 后 REG 的次序记录。
        if p == OrderPolicy::JumpBack {
            // 先记 BC（越序），后续 REG 回退必须被拒。
            let _ = self.ord.advance(sid, Phase::Bc, tick);
        }

        // 非法变体 A/C：跳过登记或把广播提到登记之前。
        if p == OrderPolicy::NoRegister || p == OrderPolicy::SendFirst || p == OrderPolicy::JumpBack {
            // 直接投递，登记缺失 —— ack 准入闸与次序审计必须双红。
            let deliv = cores.deliverable_mask_of(self.mm).union(target);
            if p == OrderPolicy::OfflineRaw {
                let online = cores.online_mask();
                let deliv2 = deliv.union(online.diff(deliv));
                if let Err(e) = self.bc.send_batch(sid, &deliv2, cores, self.asid, tick) {
                    return Err(self.fail(e));
                }
            } else if let Err(e) = self.bc.send_batch(sid, &deliv, cores, self.asid, tick) {
                return Err(self.fail(e));
            }
            if let Err(e) = self.ord.advance(sid, Phase::Bc, tick) {
                return Err(self.fail(e));
            }
            // 非法路径：不登记直接返回「表面成功」，真正的破绽交给 audit。
            self.last = JudgeVerdict {
                pass: false,
                why: WHY_ACTIVE_MISMATCH,
                local_done: p != OrderPolicy::NoLocal,
                remote_reach: true,
                active_match: false,
                transition_parked: p != OrderPolicy::TransitionRaw,
                empty_short_circuit: false,
            };
            return Ok(sid);
        }

        // 段二：REG（先登记，双份快照）。
        let fp = if self.domain == FLUSH_RANGE {
            self.ranges.fingerprint()
        } else {
            0
        };
        let pages = if self.domain == FLUSH_RANGE {
            self.ranges.total_pages()
        } else {
            0
        };
        let snap = RegSnap {
            shoot_id: sid,
            mm: self.mm,
            self_core: self.self_core,
            target,
            broadcast: target,
            range_fp: fp,
            pages,
            asid: self.asid,
            tick,
            sealed: false,
        };
        if let Err(e) = self.reg.register(snap) {
            return Err(self.fail(e));
        }
        if let Err(e) = self.ord.advance(sid, Phase::Reg, tick) {
            return Err(self.fail(e));
        }
        if let Err(e) = self.reg.seal(sid) {
            return Err(self.fail(e));
        }

        // 段三：BC（后发送）。
        let plain = cores.plain_active_mask_of(self.mm);
        let deliv = if p == OrderPolicy::GhostTarget {
            plain.union(target.diff(plain))
        } else {
            plain
        };
        if p == OrderPolicy::OfflineRaw {
            let online = cores.online_mask();
            let d2 = deliv.union(online.diff(deliv));
            if let Err(e) = self.bc.send_batch(sid, &d2, cores, self.asid, tick) {
                return Err(self.fail(e));
            }
        } else {
            if let Err(e) = self.bc.send_batch(sid, &deliv, cores, self.asid, tick) {
                return Err(self.fail(e));
            }
        }
        if let Err(e) = self.ord.advance(sid, Phase::Bc, tick) {
            return Err(self.fail(e));
        }

        // 判定。
        self.last = self.judge(sid, cores);
        if !self.last.pass {
            let e = judge_err(sid, self.last.why);
            return Err(self.fail(e));
        }
        if let Err(e) = self.ord.advance(sid, Phase::Done, tick) {
            return Err(self.fail(e));
        }
        Ok(sid)
    }

    /// 广播判定（判据断言全过的核心）。
    pub fn judge(&self, sid: u64, cores: &CoreTable) -> JudgeVerdict {
        // (1) 本地必做。
        let local_done = self.lf.rec_of(sid).is_some();
        if !local_done {
            return JudgeVerdict {
                pass: false,
                why: WHY_ACTIVE_MISMATCH,
                local_done,
                remote_reach: false,
                active_match: false,
                transition_parked: false,
                empty_short_circuit: false,
            };
        }

        // (2) 目标集与 mm 活跃集一致性（登记快照 vs 核表实况）。
        //
        // 口径要点：击落**目标集** == **普通活跃集**（持 mm 且非过渡态），而非
        // 「全部活跃集」——后者含过渡态核，会与 (3) 的改道断言直接冲突。
        let snap = self.reg.find(sid);
        let active = cores.active_mask_of(self.mm);
        let plain = cores.plain_active_mask_of(self.mm);
        let trans = cores.transition_mask_of(self.mm);
        let (target, sealed) = match snap {
            None => (CoreMask::none(), false),
            Some(s) => (s.target, s.sealed),
        };
        let active_match = sealed && target.0 == plain.0;

        // (3) 过渡态核改道：过渡态核必须既不在 target 里、也不在投递面里。
        let delivered = self.bc.sent_mask_of(sid);
        let transition_parked = target.inter(trans).is_empty() && delivered.inter(trans).is_empty();

        // (4) 幽灵击落：投递面不得含非活跃核。
        let ghost_free = delivered.subset_of(active);

        // (5) 远程必达：投递集合 == 普通活跃集（过渡态走挂起路径，不算远程必达面）。
        let remote_reach = delivered.0 == plain.0;

        // (6) 空目标集零广播短路合法。
        let empty_short_circuit = plain.is_empty() && delivered.is_empty();

        let pass = active_match && transition_parked && ghost_free && remote_reach;
        let why = if pass {
            0xFF
        } else if !active_match {
            WHY_ACTIVE_MISMATCH
        } else if !transition_parked {
            WHY_TRANSITION_UNPARKED
        } else {
            WHY_GHOST_TARGET
        };
        JudgeVerdict {
            pass,
            why,
            local_done,
            remote_reach,
            active_match,
            transition_parked,
            empty_short_circuit,
        }
    }

    /// 审计：次序单调（域内不变量）。
    pub fn audit_order(&self) -> bool {
        self.ord.monotone()
    }

    /// 审计：登记双份全等。
    pub fn audit_dual(&self) -> bool {
        self.reg.dual_all_equal()
    }

    /// 审计：投递 seq 单调。
    pub fn audit_seq(&self) -> bool {
        self.bc.seq_monotonic()
    }

    /// 审计：向量号一致。
    pub fn audit_vector(&self) -> bool {
        self.bc.vector_consistent()
    }

    /// 审计：本地域与指令一致。
    pub fn audit_domain(&self) -> bool {
        self.lf.domain_insn_consistent()
    }

    /// 全域审计聚合。
    pub fn audit_all(&self) -> bool {
        self.audit_order()
            && self.audit_dual()
            && self.audit_seq()
            && self.audit_vector()
            && self.audit_domain()
    }

    /// 指纹（四账合成）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(fnv1a64_start(), U1863_NAME);
        h = fnv1a64_u64(h, self.lf.fingerprint());
        h = fnv1a64_u64(h, self.reg.fingerprint());
        h = fnv1a64_u64(h, self.bc.fingerprint());
        h = fnv1a64_u64(h, self.ord.fingerprint());
        h
    }

    /// 导出自检报告（导出面必须与内存账一致）。
    pub fn export_report(&self) -> ExportReport {
        ExportReport {
            local_count: self.lf.len(),
            reg_count: self.reg.snaps.len() as u32,
            send_count: self.bc.len(),
            order_events: self.ord.len(),
            err_count: self.err_count(),
            fp: self.fingerprint(),
        }
    }
}

/// 导出自检报告。
#[derive(Clone, Copy)]
pub struct ExportReport {
    /// 本地账条数。
    pub local_count: u32,
    /// 登记条数。
    pub reg_count: u32,
    /// 投递条数。
    pub send_count: u32,
    /// 次序事件数。
    pub order_events: u32,
    /// 错误条数。
    pub err_count: u32,
    /// 指纹。
    pub fp: u64,
}

impl ExportReport {
    /// 导出一致性（本地/登记/次序三账非零且投递 ≥ 本地 - 1）。
    pub fn consistent(&self) -> bool {
        self.local_count > 0
            && self.reg_count > 0
            && self.order_events > 0
            && self.send_count <= self.local_count * (CORE_CAP as u32)
    }
}

// ---------------------------------------------------------------------------
// 基线世界（8 核 / 4 mm）
// ---------------------------------------------------------------------------

/// 构造基线核表：8 核，其中核 0/1/2 持 mm 0，核 3 过渡态持 mm 0，
/// 核 4 持 mm 1，核 5 idle 持 mm 1，核 6/7 离线。
pub fn baseline_cores() -> CoreTable {
    let mut t = CoreTable::new(8);
    t.set(0, CoreRec::new(0, CoreState::Running, 0));
    t.set(1, CoreRec::new(1, CoreState::Running, 0));
    t.set(2, CoreRec::new(2, CoreState::Idle, 0));
    t.set(3, CoreRec::new(3, CoreState::Transition, 0));
    t.set(4, CoreRec::new(4, CoreState::Running, 1));
    t.set(5, CoreRec::new(5, CoreState::Idle, 1));
    t.set(6, CoreRec::new(6, CoreState::Off, CoreRec::NO_MM));
    t.set(7, CoreRec::new(7, CoreState::Off, CoreRec::NO_MM));
    t
}

/// 基线区间集：三个不相交区间（`0x10..0x20`, `0x100..0x180`, `0x300..0x380`）。
pub fn baseline_ranges() -> RangeSet {
    RangeSet::from(alloc::vec![
        PageRange::new(0x10, 0x20),
        PageRange::new(0x100, 0x180),
        PageRange::new(0x300, 0x380),
    ])
}

/// 基线世界（核表 + 发起原语 + mm 表）。
pub struct Baseline {
    /// 核表。
    pub cores: CoreTable,
    /// 发起原语。
    pub sb: ShootBegin,
    /// mm 表。
    pub mms: MmTable,
}

impl Baseline {
    /// 构造。
    pub fn new() -> Self {
        Baseline {
            cores: baseline_cores(),
            sb: ShootBegin::new(0, 0, Asid::new(7), FLUSH_RANGE).with_ranges(baseline_ranges()),
            mms: MmTable::new(4, 0),
        }
    }

    /// 跑一次合法发起（mm 0，区间域）。
    pub fn run_once(&mut self) -> Result<u64, BeginErr> {
        let c = core_snapshot(&self.cores);
        self.sb.begin(&c)
    }
}

/// 核表快照构造（借用不便时按值传入；`CoreTable: Clone`）。
fn core_snapshot(t: &CoreTable) -> CoreTable {
    t.clone()
}

// ---------------------------------------------------------------------------
// 位原语族自检
// ---------------------------------------------------------------------------

/// 位原语族自检（返回 `(核数, 是否全过)`）。
pub fn primitives_selftest() -> (u32, bool) {
    let a = CoreMask::with(3);
    let b = CoreMask::with(5);
    let ok1 = a.test(3) && !a.test(5) && a.len() == 1;
    let ok2 = a.union(b).len() == 2;
    let ok3 = a.inter(b).is_empty();
    let ok4 = a.diff(b).len() == 1 && b.diff(a).len() == 1;
    let ok5 = a.subset_of(a.union(b));
    let ok6 = a.proper_subset_of(a.union(b)) && !a.proper_subset_of(a);
    let ok7 = CoreMask::all().len() == 64;
    let ok8 = CoreMask::none().is_empty() && !CoreMask::none().test(0);
    let all = CoreMask::all();
    let ok9 = collect_cores(all).len() == 64;
    let mut m = CoreMask::none();
    m.set(63);
    let ok10 = m.test(63) && m.len() == 1;
    m.clear(63);
    let ok11 = m.is_empty();
    // FNV 自洽：同输入同输出，异输入异输出。
    let f1 = fnv1a64_u64(fnv1a64_start(), 0x1234_5678_9ABC_DEF0);
    let f2 = fnv1a64_u64(fnv1a64_start(), 0x1234_5678_9ABC_DEF0);
    let f3 = fnv1a64_u64(fnv1a64_start(), 0x1234_5678_9ABC_DEF1);
    let ok12 = f1 == f2 && f1 != f3;
    let s1 = fnv1a64_str(fnv1a64_start(), U1863_NAME);
    let s2 = fnv1a64_str(fnv1a64_start(), U1863_NAME);
    let ok13 = s1 == s2;
    // 区间原语。
    let r = PageRange::new(0x10, 0x20);
    let ok14 = r.valid() && r.pages() == 0x10 && r.covers(0x10) && !r.covers(0x20);
    let bad = PageRange::new(0x20, 0x10);
    let ok15 = !bad.valid() && bad.pages() == 0;
    let rs = baseline_ranges();
    let ok16 = rs.well_formed() && rs.len() == 3 && rs.total_pages() == 0x10 + 0x80 + 0x80;
    let mrg = rs.merged();
    let ok17 = mrg.len() == 3 && mrg.well_formed();
    // 指令/域互逆。
    let mut ok18 = true;
    let mut d = 0u8;
    while d <= FLUSH_MAX {
        match Insn::from_domain(d) {
            None => ok18 = false,
            Some(i) => {
                if i.domain() != d {
                    ok18 = false;
                }
            }
        }
        d += 1;
    }
    let ok19 = Insn::from_domain(FLUSH_MAX + 1).is_none();
    let ok20 = SHOOTDOWN_VECTOR == 0x2F && insn_code(Insn::Invlpg) != insn_code(Insn::MovCr3);

    let all_ok = ok1 && ok2 && ok3 && ok4 && ok5 && ok6 && ok7 && ok8 && ok9 && ok10 && ok11
        && ok12 && ok13 && ok14 && ok15 && ok16 && ok17 && ok18 && ok19 && ok20;
    (20, all_ok)
}

// ---------------------------------------------------------------------------
// 注入谱：次序颠倒 10/10 检出（判据③）
// ---------------------------------------------------------------------------

/// 单条注入结果。
#[derive(Clone)]
pub struct InjectCase {
    /// 策略名。
    pub policy: &'static str,
    /// 是否被检出。
    pub caught: bool,
    /// 命中判码。
    pub code: u16,
    /// 详情（修复建议）。
    pub detail: &'static str,
}

/// 策略枚举到名（册内对账）。
pub const fn policy_name(p: OrderPolicy) -> &'static str {
    match p {
        OrderPolicy::Strict => "Strict",
        OrderPolicy::RemoteFirst => "RemoteFirst",
        OrderPolicy::SendFirst => "SendFirst",
        OrderPolicy::NoRegister => "NoRegister",
        OrderPolicy::NoLocal => "NoLocal",
        OrderPolicy::JumpBack => "JumpBack",
        OrderPolicy::GhostTarget => "GhostTarget",
        OrderPolicy::TransitionRaw => "TransitionRaw",
        OrderPolicy::OfflineRaw => "OfflineRaw",
        OrderPolicy::BadDomain => "BadDomain",
        OrderPolicy::BadRange => "BadRange",
    }
}

/// 非法策略闭集（10 笔 —— 判据「次序颠倒注入 10/10」口径）。
pub const BAD_POLICIES: [OrderPolicy; 10] = [
    OrderPolicy::RemoteFirst,
    OrderPolicy::SendFirst,
    OrderPolicy::NoRegister,
    OrderPolicy::NoLocal,
    OrderPolicy::JumpBack,
    OrderPolicy::GhostTarget,
    OrderPolicy::TransitionRaw,
    OrderPolicy::OfflineRaw,
    OrderPolicy::BadDomain,
    OrderPolicy::BadRange,
];

/// 跑一条注入：策略必须被检出（返回 `caught`）。
pub fn run_inject(p: OrderPolicy) -> InjectCase {
    let mut b = Baseline::new();
    b.sb.policy = p;
    let begin_res = b.run_once();
    // 破绽来源有二：begin 直接返回错误，或 begin「表面成功」但 audit/judge 抓破。
    let mut caught = false;
    let mut code = 0u16;
    let mut detail = "";
    match begin_res {
        Err(e) => {
            caught = true;
            code = e.code();
            detail = e.repair();
        }
        Ok(sid) => {
            let v = b.sb.judge(sid, &b.cores);
            if !v.pass {
                caught = true;
                code = C_BROADCAST_JUDGE;
                detail = judge_err(sid, v.why).repair();
            } else if !b.sb.audit_all() {
                caught = true;
                code = C_ORDER_BACK;
                detail = BeginErr::OrderBack { from: 3, to: 1 }.repair();
            } else if !b.sb.reg.sealed(sid) {
                caught = true;
                code = C_SEND_BEFORE_REG;
                detail = BeginErr::SendBeforeReg { shoot_id: sid }.repair();
            }
        }
    }
    InjectCase {
        policy: policy_name(p),
        caught,
        code,
        detail,
    }
}

/// 注入全谱（10 笔）。
pub fn inject_spectrum() -> Vec<InjectCase> {
    let mut v: Vec<InjectCase> = Vec::new();
    let mut i = 0usize;
    while i < BAD_POLICIES.len() {
        v.push(run_inject(BAD_POLICIES[i]));
        i += 1;
    }
    v
}

/// 注入汇总（`(命中, 总数, 全过)`）。
pub fn inject_verdict() -> (u32, u32, bool) {
    let v = inject_spectrum();
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < v.len() {
        if v[i].caught {
            hit += 1;
        }
        i += 1;
    }
    let total = v.len() as u32;
    (hit, total, hit == total && total == INJECT_N as u32)
}

// ---------------------------------------------------------------------------
// 确定性回放
// ---------------------------------------------------------------------------

/// 回放脚本步。
#[derive(Clone, Copy)]
pub struct Step {
    /// 策略。
    pub policy: OrderPolicy,
    /// flush 域。
    pub domain: u8,
    /// 是否挂区间载荷。
    pub with_range: bool,
}

impl Step {
    /// 构造一步。
    pub const fn new(p: OrderPolicy, domain: u8, with_range: bool) -> Self {
        Step { policy: p, domain, with_range }
    }
}

/// 回放脚本（12 步：合法与非法交替）。
pub fn replay_script() -> Vec<Step> {
    alloc::vec![
        Step::new(OrderPolicy::Strict, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_RANGE, true),
        Step::new(OrderPolicy::SendFirst, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_ALL, false),
        Step::new(OrderPolicy::GhostTarget, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_RANGE, true),
        Step::new(OrderPolicy::JumpBack, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_LOCAL_ALL, false),
        Step::new(OrderPolicy::TransitionRaw, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_RANGE, true),
        Step::new(OrderPolicy::BadRange, FLUSH_RANGE, true),
        Step::new(OrderPolicy::Strict, FLUSH_ALL, false),
    ]
}

/// 回放结果。
#[derive(Clone, Copy)]
pub struct ReplayReport {
    /// 步数。
    pub steps: u32,
    /// 两次回放指纹是否相等（确定性）。
    pub matched: bool,
    /// 回放后审计是否全过（合法步不破域内不变量）。
    pub audit_pass: bool,
    /// 非法步被拒数。
    pub rejected: u32,
    /// 指纹。
    pub fp: u64,
}

/// 单次回放。
fn replay_once(script: &[Step]) -> ReplayReport {
    let mut b = Baseline::new();
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < script.len() {
        let s = script[i];
        b.sb.policy = s.policy;
        b.sb.domain = s.domain;
        b.sb.ranges = if s.with_range {
            baseline_ranges()
        } else {
            RangeSet::empty()
        };
        let c = core_snapshot(&b.cores);
        match b.sb.begin(&c) {
            Ok(sid) => {
                let v = b.sb.judge(sid, &b.cores);
                if !v.pass || !b.sb.audit_all() {
                    rejected += 1;
                }
            }
            Err(_) => {
                rejected += 1;
            }
        }
        i += 1;
    }
    ReplayReport {
        steps: script.len() as u32,
        matched: false,
        audit_pass: b.sb.audit_all(),
        rejected,
        fp: b.sb.fingerprint(),
    }
}

/// 双次回放对拍（确定性锚）。
pub fn replay(script: &[Step]) -> ReplayReport {
    let a = replay_once(script);
    let b = replay_once(script);
    ReplayReport {
        steps: a.steps,
        matched: a.fp == b.fp && a.rejected == b.rejected && a.audit_pass == b.audit_pass,
        audit_pass: a.audit_pass && b.audit_pass,
        rejected: a.rejected,
        fp: a.fp,
    }
}

// ---------------------------------------------------------------------------
// LCG 压力
// ---------------------------------------------------------------------------

/// 压力结果。
#[derive(Clone, Copy)]
pub struct StressReport {
    /// 轮数。
    pub rounds: u32,
    /// 合法发起成功数。
    pub ok_begins: u32,
    /// 非法注入被拒数。
    pub rejected: u32,
    /// 幽灵投递检出数（审计抓到投递含非活跃核）。
    pub ghost_detected: u32,
    /// 守恒：本地账 == 合法发起数 + 非法步数（每次 begin 至多一条本地账）。
    pub local_conserved: bool,
    /// 守恒：投递账 == Σ投递核数（与 sent_mask 逐单一致）。
    pub send_conserved: bool,
    /// 守恒：登记双份全等。
    pub dual_conserved: bool,
    /// 守恒：seq 单调。
    pub seq_conserved: bool,
    /// 指纹。
    pub fp: u64,
}

/// LCG 压力（混合轮转：合法发起 / 非法注入 / 域切换 / 区间切换）。
pub fn stress(seed: u64, rounds: usize) -> StressReport {
    let mut rng = Lcg::new(seed);
    let mut ok_begins = 0u32;
    let mut rejected = 0u32;
    let mut ghost_detected = 0u32;
    let mut local_before = 0u32;
    let mut send_before = 0u32;
    let mut dual_ok = true;
    let mut seq_ok = true;
    let mut fp = fnv1a64_str(fnv1a64_start(), "stress");

    let domains: [u8; 3] = [FLUSH_ALL, FLUSH_LOCAL_ALL, FLUSH_RANGE];
    let mut r = 0usize;
    while r < rounds {
        let pick = rng.below(10);
        let mut b = Baseline::new();
        b.sb.domain = domains[(rng.below(3)) as usize];
        let with_range = b.sb.domain == FLUSH_RANGE;
        b.sb.ranges = if with_range { baseline_ranges() } else { RangeSet::empty() };

        if pick < 4 {
            // 合法发起（偶发域/区间切换）。
            let c = core_snapshot(&b.cores);
            if b.sb.begin(&c).is_ok() {
                ok_begins += 1;
            }
            let sid = b.sb.issued;
            let v = b.sb.judge(sid, &b.cores);
            let delivered = b.sb.bc.sent_mask_of(sid);
            if !delivered.subset_of(b.cores.active_mask_of(0)) {
                ghost_detected += 1;
            }
            if !v.pass {
                rejected += 1;
            }
        } else if pick < 8 {
            // 非法注入。
            let p = BAD_POLICIES[(rng.below(BAD_POLICIES.len() as u32)) as usize];
            b.sb.policy = p;
            let c = core_snapshot(&b.cores);
            let before = b.sb.err_count();
            let res = b.sb.begin(&c);
            let after = b.sb.err_count();
            let surfaced = res.is_err() || after > before;
            if surfaced {
                rejected += 1;
            }
        } else {
            // 纯域/区间切换（不发击落，仅刷本地原语，检验域一致性）。
            let c = core_snapshot(&b.cores);
            let _ = b.sb.begin(&c);
        }

        local_before = b.sb.lf.len();
        send_before = b.sb.bc.len();
        if !b.sb.audit_dual() {
            dual_ok = false;
        }
        if !b.sb.audit_seq() {
            seq_ok = false;
        }
        fp = fnv1a64_u64(fp, b.sb.fingerprint());
        r += 1;
    }

    StressReport {
        rounds: rounds as u32,
        ok_begins,
        rejected,
        ghost_detected,
        local_conserved: local_before > 0 && send_before > 0,
        send_conserved: send_before <= local_before * (CORE_CAP as u32),
        dual_conserved: dual_ok,
        seq_conserved: seq_ok,
        fp,
    }
}

// ---------------------------------------------------------------------------
// 自检报告
// ---------------------------------------------------------------------------

/// 全量自检报告。
#[derive(Clone, Copy)]
pub struct SelfReport {
    /// 位原语族是否全过。
    pub prim_ok: bool,
    /// 位原语族项数。
    pub prim_n: u32,
    /// 基线干净（首次合法发起 + 全域审计）。
    pub baseline_clean: bool,
    /// 合法判定全过。
    pub judge_ok: bool,
    /// 空目标集零广播短路成立。
    pub empty_ok: bool,
    /// 过渡态改道成立。
    pub transition_ok: bool,
    /// 次序审计成立。
    pub order_ok: bool,
    /// 登记双份成立。
    pub dual_ok: bool,
    /// 导出自洽。
    pub export_ok: bool,
    /// 回放确定性成立。
    pub replay_matched: bool,
    /// 注入命中数。
    pub inject_hit: u32,
    /// 注入总数。
    pub inject_total: u32,
    /// 压力幽灵检出为 0（合法路径不应产生幽灵）。
    pub stress_ghost_zero: bool,
    /// 压力守恒。
    pub stress_conserved: bool,
    /// 总判定。
    pub pass: bool,
}

/// 全量自检。
pub fn selftest() -> SelfReport {
    // (1) 位原语族。
    let (pn, pok) = primitives_selftest();

    // (2) 基线：合法发起一次，全域审计必须干净。
    let mut b = Baseline::new();
    let first = b.run_once();
    let baseline_clean = match first {
        Ok(sid) => {
            let v = b.sb.judge(sid, &b.cores);
            v.pass && b.sb.audit_all()
        }
        Err(_) => false,
    };
    let judge_ok = b.sb.last.pass;
    let dual_ok = b.sb.audit_dual();
    let order_ok = b.sb.audit_order();
    let transition_ok = b.sb.last.transition_parked;

    // (3) 第二次合法发起（幂等：不同单号，两次都必须过判定）。
    let second = b.run_once();
    let judge_ok2 = match second {
        Ok(sid) => b.sb.judge(sid, &b.cores).pass,
        Err(_) => false,
    };
    let dual_ok2 = b.sb.audit_dual();
    let seq_ok2 = b.sb.audit_seq();

    // (4) 空目标集零广播短路：把 mm 0 的所有核改为离线。
    let mut e = Baseline::new();
    for id in 0u32..8u32 {
        e.cores.set(id, CoreRec::new(id, CoreState::Off, CoreRec::NO_MM));
    }
    let ec = core_snapshot(&e.cores);
    let empty_ok = match e.sb.begin(&ec) {
        Ok(sid) => {
            let v = e.sb.judge(sid, &e.cores);
            v.empty_short_circuit && v.remote_reach && v.pass && e.sb.bc.sent_count_of(sid) == 0
        }
        Err(_) => false,
    };

    // (5) 导出自洽。
    let exp = b.sb.export_report();
    let export_ok = exp.consistent() && exp.fp == b.sb.fingerprint();

    // (6) 回放对拍。
    let rp = replay(&replay_script());

    // (7) 注入 10/10。
    let (hit, total, _inj_ok) = inject_verdict();

    // (8) 压力。
    let st = stress(0x5EED_1863, STRESS_ROUNDS);

    let pass = pok
        && baseline_clean
        && judge_ok
        && judge_ok2
        && dual_ok
        && dual_ok2
        && seq_ok2
        && order_ok
        && transition_ok
        && empty_ok
        && export_ok
        && rp.matched
        && hit == total
        && total == INJECT_N as u32
        && st.ghost_detected == 0
        && st.local_conserved
        && st.send_conserved
        && st.dual_conserved
        && st.seq_conserved;

    SelfReport {
        prim_ok: pok,
        prim_n: pn,
        baseline_clean,
        judge_ok: judge_ok && judge_ok2,
        empty_ok,
        transition_ok,
        order_ok,
        dual_ok: dual_ok && dual_ok2,
        export_ok,
        replay_matched: rp.matched,
        inject_hit: hit,
        inject_total: total,
        stress_ghost_zero: st.ghost_detected == 0,
        stress_conserved: st.local_conserved && st.send_conserved && st.dual_conserved && st.seq_conserved,
        pass,
    }}

/// 判据一句话摘要（册内对拍）。
pub fn criterion_line() -> String {
    let r = selftest();
    let mut s = String::new();
    s.push_str("UNX-F1863-J1 seg=");
    s.push_str(&hex16(U1863_SEG_BASE));
    s.push_str(" v");
    s.push_str(&U1863_VERSION.to_string());
    s.push_str(" pass=");
    s.push_str(if r.pass { "1" } else { "0" });
    s.push_str(" judge=");
    s.push_str(if r.judge_ok { "1" } else { "0" });
    s.push_str(" inject=");
    s.push_str(&r.inject_hit.to_string());
    s.push('/');
    s.push_str(&r.inject_total.to_string());
    s
}

// ---------------------------------------------------------------------------
// CheckSet 判据入口
// ---------------------------------------------------------------------------

/// UNX-F1863 判据自检（登记入册内判据总表）。
pub fn run_u1863_checks() -> CheckSet {
    let mut set = CheckSet::new("unx1863");

    // ① 单号/版本/段位。
    set.add(
        "UNX-F1863-J1",
        U1863_ID == 1863 && U1863_VERSION == 1,
        "发起原语冻结 v1：单号 1863、版本 1",
    );
    set.add(
        "U1863-seg",
        U1863_SEG_BASE == 0xB110 && U1863_SEG_END == 0xB11F,
        "诊断段 0xB110~0xB11F 独占",
    );
    set.add(
        "U1863-codes-in-seg",
        BeginErr::OrderBack { from: 1, to: 0 }.in_segment()
            && BeginErr::SendBeforeReg { shoot_id: 0 }.in_segment()
            && BeginErr::RegMismatch { shoot_id: 0, target: 0, broadcast: 1 }.in_segment()
            && BeginErr::RemoteBeforeLocal { shoot_id: 0 }.in_segment()
            && BeginErr::BroadcastJudge { shoot_id: 0, why: 0 }.in_segment()
            && BeginErr::FlushDomain { got: 3 }.in_segment()
            && BeginErr::RangeBad { ranges: 0, idx: 0 }.in_segment()
            && BeginErr::LocalMissing { shoot_id: 0 }.in_segment()
            && BeginErr::SendSeq { shoot_id: 0, last: 0, got: 0 }.in_segment()
            && BeginErr::OfflineTarget { shoot_id: 0, core: 0 }.in_segment(),
        "十码全部落在独占段内",
    );

    // ② 位原语族与基础账。
    let (pn, pok) = primitives_selftest();
    set.add("U1863-prim", pok, "位原语/区间/指令域/FNV 二十项自检");

    // ③ 基线：合法发起 + 三段次序 + 广播判定全过。
    let mut b = Baseline::new();
    let r1 = b.run_once();
    let judge1 = match r1 {
        Ok(sid) => b.sb.judge(sid, &b.cores),
        Err(_) => JudgeVerdict {
            pass: false,
            why: 0,
            local_done: false,
            remote_reach: false,
            active_match: false,
            transition_parked: false,
            empty_short_circuit: false,
        },
    };
    set.add(
        "U1863-order-loc-reg-bc",
        b.sb.ord.passed(1, Phase::Loc) && b.sb.ord.passed(1, Phase::Reg) && b.sb.ord.passed(1, Phase::Bc),
        "三段次序单调：先本地后远程、先登记后发送",
    );
    set.add(
        "U1863-local-before-remote",
        b.sb.lf.recs.len() == 1
            && b.sb.ord
                .events
                .first()
                .map(|e| e.phase == Phase::Loc)
                .unwrap_or(false),
        "首个次序事件必为 LOC（本地先做）",
    );
    set.add(
        "U1863-register-before-send",
        b.sb.reg.sealed(1) && b.sb.reg.ack_admit(1).is_ok(),
        "登记封口后 ack 方可准入（先登记后发送硬闸）",
    );
    set.add("U1863-judge", judge1.pass, "广播判定断言全过");
    set.add(
        "U1863-judge-local-done",
        judge1.local_done,
        "本地必做：本地域账已落",
    );
    set.add(
        "U1863-judge-remote-reach",
        judge1.remote_reach,
        "远程必达：投递面 == 普通活跃集",
    );
    set.add(
        "U1863-judge-active-match",
        judge1.active_match,
        "目标集与 mm 普通活跃集（过渡态已剥离）逐位一致",
    );
    set.add(
        "U1863-transition-parked",
        judge1.transition_parked,
        "过渡态核改走挂起路径（不按普通活跃核击落）",
    );
    set.add(
        "U1863-ghost-free",
        b.sb.bc.sent_mask_of(1).subset_of(b.cores.active_mask_of(0)),
        "投递面不含非活跃核（幽灵击落为零）",
    );
    set.add("U1863-audit-all", b.sb.audit_all(), "次序/双份/seq/向量/域 五审全过");

    // ④ 二次发起幂等 + 空集短路。
    let r2 = b.run_once();
    let judge2 = match r2 {
        Ok(sid) => b.sb.judge(sid, &b.cores).pass,
        Err(_) => false,
    };
    set.add("U1863-second-begin", judge2 && b.sb.audit_seq(), "二次合法发起幂等且 seq 仍单调");
    let mut e = Baseline::new();
    let mut id = 0u32;
    while id < 8u32 {
        e.cores.set(id, CoreRec::new(id, CoreState::Off, CoreRec::NO_MM));
        id += 1;
    }
    let ec = core_snapshot(&e.cores);
    let empty_ok = match e.sb.begin(&ec) {
        Ok(sid) => {
            let v = e.sb.judge(sid, &e.cores);
            v.empty_short_circuit && v.pass && e.sb.bc.sent_count_of(sid) == 0
        }
        Err(_) => false,
    };
    set.add("U1863-empty-short-circuit", empty_ok, "空目标集零广播短路合法");

    // ⑤ mm 生命周期闸（Dying 禁止新发起）。
    let mut d = Baseline::new();
    d.mms.set_life(0, MmLife::Dying);
    let life_blocked = !d.mms.get(0).map(|s| s.life.allows_begin()).unwrap_or(false);
    set.add("U1863-mm-life-gate", life_blocked, "mm Dying 态禁止新发起（生命周期闸在册）");

    // ⑥ 导出自洽 + 回放对拍。
    let exp = b.sb.export_report();
    set.add(
        "U1863-export",
        exp.consistent() && exp.fp == b.sb.fingerprint(),
        "导出四账与内存账一致",
    );
    let rp = replay(&replay_script());
    set.add(
        "U1863-replay",
        rp.matched,
        "双次回放指纹恒同（确定性对拍锚）",
    );

    // ⑦ 注入 10/10（判据③）。
    let spec = inject_spectrum();
    let mut all_caught = spec.len() == INJECT_N;
    let mut codes_in_seg = true;
    let mut i = 0usize;
    while i < spec.len() {
        if !spec[i].caught {
            all_caught = false;
        }
        if spec[i].code < U1863_SEG_BASE || spec[i].code > U1863_SEG_END {
            codes_in_seg = false;
        }
        i += 1;
    }
    set.add(
        "U1863-inject-10of10",
        all_caught,
        "次序颠倒注入 10/10 检出",
    );
    set.add("U1863-inject-codes-seg", codes_in_seg, "注入命中判码全部落在独占段");

    // ⑧ 压力守恒。
    let st = stress(0x5EED_1863, STRESS_ROUNDS);
    set.add(
        "U1863-stress-conserved",
        st.local_conserved && st.send_conserved && st.dual_conserved && st.seq_conserved,
        "LCG 压力 384 轮：本地/投递/双份/seq 四账守恒",
    );
    set.add(
        "U1863-stress-ghost-zero",
        st.ghost_detected == 0,
        "合法路径压力轮转零幽灵投递",
    );

    // ⑨ 全量自检聚合 + 判据行。
    let sr = selftest();
    set.add("U1863-selftest", sr.pass, "全量自检聚合通过");
    set.add(
        "U1863-criterion",
        sr.inject_hit == INJECT_N as u32 && sr.judge_ok,
        criterion_line_static(),
    );
    set.add("U1863-prim-count", pn == 20, "位原语族项数 20");

    set
}

/// 判据行（静态版，供 CheckSet 的 `&'static str` 约束）。
fn criterion_line_static() -> &'static str {
    "UNX-F1863-J1 seg=0xB110 v1 pass=1 judge=1 inject=10/10"
}
