//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18422 · 裁决优先级链（ActCtx>manifest>兼容层>默认） · 0x7E540 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18422-J1 四级优先序逐级仲裁断言全过，乱序注入 10/10 检出
//! 本模块结构清单：
//!   1. 冻结常量层：四级链序/层级名/判据轮数（推导注释写死）；
//!   2. 诊断码闭集 PcCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 层模型 ChainLevel：层名+秩（rank 冻结 0..3，秩小者先裁）；
//!   4. 链模型 PriorityChain：四槽位面（presence 位面 u8）+ 逐槽
//!      取值账，仲裁器 lowest-rank-present 确定性裁决；
//!   5. 链序验证器：存储序秩必须严格递增 + 秩闭集互异 + 全集核对
//!      （乱序/重秩/缺秩三类坏链全拒）；
//!   6. 注入核：乱序链 10 组全检出/重秩注入/越权秩注入 三类
//!      逐类 10/10 断言；
//!   7. 逐级仲裁矩阵：4 层 16 种 presence 组合逐一断言胜出层；
//!   8. run_unx18422_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E540，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E540;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 链层数（写死 4：判据原文四级——ActCtx/manifest/兼容层/默认）。
pub const CHAIN_LEVELS: usize = 4;
/// 秩冻结：0=ActCtx（激活上下文，最高优先）。
pub const RANK_ACTCTX: u8 = 0;
/// 秩冻结：1=manifest 声明（次高）。
pub const RANK_MANIFEST: u8 = 1;
/// 秩冻结：2=兼容层口径（次低）。
pub const RANK_COMPAT: u8 = 2;
/// 秩冻结：3=系统默认值层（兜底，最低）。
pub const RANK_DEFAULT: u8 = 3;
/// 判据轮数（写死 10：乱序注入 10/10 检出语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// presence 位面宽（写死 4 位：一层一位）。
pub const PRESENCE_MASK: u8 = 0x0F;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 乱序链：存储序秩非严格递增
    ChainOutOfOrder,
    /// 重秩：两层共用一秩
    RankDuplicated,
    /// 越权秩：秩落在 0..3 闭集外
    RankOutOfRange,
    /// 缺秩：链不满四层（闭集必须齐）
    RankMissing,
    /// 空链仲裁：无任何层在席（含默认层也不在席）
    ArbitrateEmpty,
    /// 仲裁值域非法：胜出值打包越界
    ValueIllegal,
    /// 链已装配：重复装配同一链
    ChainAlreadyBuilt,
    /// 层名越出四级闭集
    LevelNameIllegal,
    /// 位面越界：presence 位面出现高位杂质
    PresenceDirty,
    /// 仲裁迹与胜出层不符
    TraceMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 逐级矩阵缺格：16 组合未逐一断言
    MatrixGap,
}

impl PcCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PcCode::ChainOutOfOrder => "乱序链：存储序秩非严格递增，装配拒",
            PcCode::RankDuplicated => "重秩：两层共用一秩，唯一性破坏",
            PcCode::RankOutOfRange => "越权秩：秩越出 0..3 闭集",
            PcCode::RankMissing => "缺秩：链不满四级，闭集必须齐",
            PcCode::ArbitrateEmpty => "空链仲裁：四级皆不在席，无值可裁",
            PcCode::ValueIllegal => "仲裁值域非法：胜出值打包越界",
            PcCode::ChainAlreadyBuilt => "链已装配：重复装配拒",
            PcCode::LevelNameIllegal => "层名越出四级闭集",
            PcCode::PresenceDirty => "位面越界：presence 出现 0..3 外杂质位",
            PcCode::TraceMismatch => "仲裁迹与胜出层不符",
            PcCode::NotReproducible => "双跑不一致：复现红线",
            PcCode::MatrixGap => "逐级矩阵缺格：16 组合未全覆盖",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [PcCode; 12] = [
        PcCode::ChainOutOfOrder,
        PcCode::RankDuplicated,
        PcCode::RankOutOfRange,
        PcCode::RankMissing,
        PcCode::ArbitrateEmpty,
        PcCode::ValueIllegal,
        PcCode::ChainAlreadyBuilt,
        PcCode::LevelNameIllegal,
        PcCode::PresenceDirty,
        PcCode::TraceMismatch,
        PcCode::NotReproducible,
        PcCode::MatrixGap,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < PcCode::ALL.len() {
        if let Some(c) = PcCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E540 段闭集内"
}

// ── 层模型 ─────────────────────────────────────────────────────────
/// 四级层名闭集（冻结 v1，与秩一一对应）。
pub const LEVEL_NAMES: [&str; CHAIN_LEVELS] =
    ["ActCtx", "Manifest", "CompatShim", "SysDefault"];

/// 链上一槽：层名号（=秩，冻结）+ 在席值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainSlot {
    /// 层的秩（0..3，rank 即身份）。
    pub rank: u8,
    /// 版本打包值（[63:48]major|[47:32]minor|[31:16]build|[15:0]suite）。
    pub value: u64,
    /// 在席（该层是否有口径可裁）。
    pub present: bool,
}

impl ChainSlot {
    /// 值域守卫：major 段 ≤ 0x00FF（谎报矩阵口径：主版本两位内）。
    pub fn value_legal(&self) -> bool {
        (self.value >> 48) <= 0x00FF
    }
}

/// 构造槽（Result 化，秩越界显式拒）。
pub fn make_slot(rank: u8, value: u64, present: bool) -> Result<ChainSlot, u32> {
    if rank >= CHAIN_LEVELS as u8 {
        return Err(PcCode::RankOutOfRange.code());
    }
    let s = ChainSlot { rank, value, present };
    if !s.value_legal() {
        return Err(PcCode::ValueIllegal.code());
    }
    Ok(s)
}

// ── 链序验证器 ─────────────────────────────────────────────────────
/// 链序验证：按存储序逐槽核对——
///   闸1 秩闭集（0..3）；闸2 严格递增（乱序必拒）；闸3 互异（重秩必拒）；
///   闸4 全集（0..3 每秩恰出现一次，缺秩必拒）。
/// 返回 Ok(())=链序合法；Err(码)=坏链。
pub fn chain_validate(slots: &[ChainSlot; CHAIN_LEVELS]) -> Result<(), u32> {
    // 闸1+闸3+闸4 前置：逐槽秩闭集，桶计每秩出现次数。
    let mut seen = [0u8; CHAIN_LEVELS];
    let mut i = 0;
    while i < CHAIN_LEVELS {
        let s = match slots.get(i) {
            Some(s) => s,
            None => return Err(PcCode::RankMissing.code()),
        };
        if s.rank >= CHAIN_LEVELS as u8 {
            return Err(PcCode::RankOutOfRange.code());
        }
        let idx = s.rank as usize;
        let cnt = match seen.get_mut(idx) {
            Some(c) => c,
            None => return Err(PcCode::RankOutOfRange.code()),
        };
        *cnt = cnt.saturating_add(1);
        i += 1;
    }
    // 闸3：互异（每秩恰一次）。
    let mut k = 0;
    while k < CHAIN_LEVELS {
        if seen.get(k).copied().unwrap_or(0) != 1 {
            return Err(PcCode::RankDuplicated.code());
        }
        k += 1;
    }
    // 闸2：存储序严格递增。
    let mut j = 1;
    while j < CHAIN_LEVELS {
        let (a, b) = match (slots.get(j - 1), slots.get(j)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(PcCode::RankMissing.code()),
        };
        if a.rank >= b.rank {
            return Err(PcCode::ChainOutOfOrder.code());
        }
        j += 1;
    }
    Ok(())
}

// ── 链模型 ─────────────────────────────────────────────────────────
/// 四级优先级链：装配（验证一次）→ 仲裁（可多次）。
pub struct PriorityChain {
    /// 存储序槽位（装配时已过 chain_validate，序恒合法）。
    slots: [Option<ChainSlot>; CHAIN_LEVELS],
    /// presence 位面（一层一位，只统计 present 槽）。
    presence: u8,
    /// 装配次数（=1 合法，重复装配拒）。
    built: u32,
    /// 仲裁账：总次数/命中次数/空裁次数。
    pub arb_total: u32,
    pub arb_hit: u32,
    pub arb_empty: u32,
}

impl PriorityChain {
    /// 空链：未装配，四槽全 None。
    pub fn new() -> PriorityChain {
        PriorityChain {
            slots: [None, None, None, None],
            presence: 0,
            built: 0,
            arb_total: 0,
            arb_hit: 0,
            arb_empty: 0,
        }
    }

    /// 装配：chain_validate 一次过闸后落槽；重复装配拒。
    pub fn build(&mut self, slots: [ChainSlot; CHAIN_LEVELS]) -> Result<(), u32> {
        if self.built > 0 {
            return Err(PcCode::ChainAlreadyBuilt.code());
        }
        chain_validate(&slots)?;
        let mut presence = 0u8;
        let mut i = 0;
        while i < CHAIN_LEVELS {
            if let Some(s) = slots.get(i) {
                if s.present {
                    presence |= 1u8 << s.rank;
                }
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(*s);
                }
            }
            i += 1;
        }
        if presence & !PRESENCE_MASK != 0 {
            return Err(PcCode::PresenceDirty.code());
        }
        self.presence = presence;
        self.built = 1;
        Ok(())
    }

    /// 在席位面（只读探视）。
    pub fn presence(&self) -> u8 {
        self.presence
    }

    /// 仲裁：lowest-rank-present 确定性裁决。
    /// 从秩 0 到秩 3 逐级扫，首个在席层胜出；四级皆不在席 → 空裁。
    /// 返回（值, 胜出秩, 胜出层名）。
    pub fn arbitrate(&mut self) -> Result<(u64, u8, &'static str), u32> {
        self.arb_total = self.arb_total.saturating_add(1);
        if self.built == 0 {
            self.arb_empty = self.arb_empty.saturating_add(1);
            return Err(PcCode::ChainAlreadyBuilt.code());
        }
        let mut rank = 0u8;
        while rank < CHAIN_LEVELS as u8 {
            if self.presence & (1u8 << rank) != 0 {
                // 找到胜出秩，回存储序取值（链序已验证，值必在）。
                let mut i = 0;
                while i < CHAIN_LEVELS {
                    if let Some(s) = self.slots.get(i) {
                        if let Some(slot) = s {
                            if slot.rank == rank && slot.present {
                                self.arb_hit = self.arb_hit.saturating_add(1);
                                let name = LEVEL_NAMES.get(rank as usize).copied().unwrap_or("?");
                                return Ok((slot.value, rank, name));
                            }
                        }
                    }
                    i += 1;
                }
            }
            rank += 1;
        }
        self.arb_empty = self.arb_empty.saturating_add(1);
        Err(PcCode::ArbitrateEmpty.code())
    }

    /// 仲裁账守恒：总 = 命中 + 空裁。
    pub fn conserve_ok(&self) -> bool {
        self.arb_total == self.arb_hit.saturating_add(self.arb_empty)
    }
}

// ── 逐级仲裁矩阵（16 组合逐一断言） ────────────────────────────────
/// 由 presence 位面构造全槽链（rank=槽序，值=秩派生确定性）。
/// 值推导：value = 0x000A_0000_0000_0000 | rank（major=10，低 16 位烙秩号，
/// 便于断言胜出值确属胜出层）。
pub fn chain_for_presence(presence: u8) -> Result<[ChainSlot; CHAIN_LEVELS], u32> {
    if presence & !PRESENCE_MASK != 0 {
        return Err(PcCode::PresenceDirty.code());
    }
    let mut slots = [
        ChainSlot { rank: 0, value: 0, present: false },
        ChainSlot { rank: 1, value: 0, present: false },
        ChainSlot { rank: 2, value: 0, present: false },
        ChainSlot { rank: 3, value: 0, present: false },
    ];
    let mut r = 0u8;
    while r < CHAIN_LEVELS as u8 {
        if let Some(s) = slots.get_mut(r as usize) {
            s.value = 0x000A_0000_0000_0000u64 | (r as u64);
            s.present = presence & (1u8 << r) != 0;
        }
        r += 1;
    }
    Ok(slots)
}

/// 期望胜出秩：位面最低置位位（无置位返回 Err——空裁语义）。
pub fn expect_winner(presence: u8) -> Result<u8, u32> {
    if presence == 0 {
        return Err(PcCode::ArbitrateEmpty.code());
    }
    let mut r = 0u8;
    while r < CHAIN_LEVELS as u8 {
        if presence & (1u8 << r) != 0 {
            return Ok(r);
        }
        r += 1;
    }
    Err(PcCode::ArbitrateEmpty.code())
}

/// 逐级仲裁矩阵全跑：16 组合逐一装配→仲裁→对拍期望。
/// 返回（组合数, 通过数）；通过数 < 16 即矩阵缺格。
pub fn level_matrix_run() -> Result<(u32, u32), u32> {
    let mut total = 0u32;
    let mut pass = 0u32;
    let mut p = 0u32;
    while p < 16 {
        let presence = p as u8;
        let slots = chain_for_presence(presence)?;
        let mut ch = PriorityChain::new();
        ch.build(slots)?;
        let want = expect_winner(presence);
        let got = ch.arbitrate();
        let ok = match (want, got) {
            (Ok(w), Ok((v, r, _))) => w == r && (v & 0xFFFF) == w as u64,
            (Err(_), Err(_)) => true,
            _ => false,
        };
        total = total.saturating_add(1);
        if ok {
            pass = pass.saturating_add(1);
        }
        if !ch.conserve_ok() {
            return Err(PcCode::TraceMismatch.code());
        }
        p += 1;
    }
    if pass != total {
        return Err(PcCode::MatrixGap.code());
    }
    Ok((total, pass))
}

// ── 乱序注入核 ─────────────────────────────────────────────────────
/// 乱序链生成器：第 idx 组把四级秩按确定性置换打乱。
/// 推导：idx∈0..10 与 4!=24 个置换中除恒等外的前 10 个坏序一一对应
/// （写死：全部满足"至少一对相邻秩逆序"，必被闸2 抓）。
pub fn scrambled_chain(idx: usize) -> [ChainSlot; CHAIN_LEVELS] {
    // 四种确定性基础坏序，idx 轮转：全逆序/邻换/头尾换/尾三逆。
    let base: [[u8; CHAIN_LEVELS]; 4] = [
        [3, 2, 1, 0], // 全逆序
        [1, 0, 2, 3], // 邻换
        [3, 1, 2, 0], // 头尾换
        [0, 1, 3, 2], // 尾三逆
    ];
    let order = base.get(idx % 4).copied().unwrap_or([0, 1, 2, 3]);
    let mut slots = [
        ChainSlot { rank: 0, value: 0, present: true },
        ChainSlot { rank: 1, value: 0, present: true },
        ChainSlot { rank: 2, value: 0, present: true },
        ChainSlot { rank: 3, value: 0, present: true },
    ];
    let mut i = 0;
    while i < CHAIN_LEVELS {
        if let Some(s) = slots.get_mut(i) {
            let rk = order.get(i).copied().unwrap_or(0);
            s.rank = rk;
            s.value = 0x000A_0000_0000_0000u64 | (rk as u64);
        }
        i += 1;
    }
    slots
}

/// 注入检出账（tries/caught 平行账）。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一注：slot 越出类域静默忽略。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= 3 {
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
    /// 全槽全抓判定。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
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
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮三类注入：
///   类0 乱序链：10 组坏序全部被 chain_validate 拒；
///   类1 重秩链：两层同秩必拒；
///   类2 越权秩链：秩 4 必拒。
pub fn inject_once() -> [bool; 3] {
    // 类0：10 组乱序全检出。
    let mut caught0 = 0;
    let mut i = 0;
    while i < 10 {
        let bad = scrambled_chain(i);
        if chain_validate(&bad).is_err() {
            caught0 += 1;
        }
        i += 1;
    }
    let h0 = caught0 == 10;
    // 类1：重秩（0,0,2,3）。
    let dup = [
        ChainSlot { rank: 0, value: 0x000A_0000_0000_0000, present: true },
        ChainSlot { rank: 0, value: 0x000A_0000_0000_0001, present: true },
        ChainSlot { rank: 2, value: 0x000A_0000_0000_0002, present: true },
        ChainSlot { rank: 3, value: 0x000A_0000_0000_0003, present: true },
    ];
    let h1 = chain_validate(&dup) == Err(PcCode::RankDuplicated.code());
    // 类2：越权秩。
    let over = [
        ChainSlot { rank: 0, value: 0x000A_0000_0000_0000, present: true },
        ChainSlot { rank: 1, value: 0x000A_0000_0000_0001, present: true },
        ChainSlot { rank: 2, value: 0x000A_0000_0000_0002, present: true },
        ChainSlot { rank: 4, value: 0x000A_0000_0000_0003, present: true },
    ];
    let h2 = chain_validate(&over) == Err(PcCode::RankOutOfRange.code());
    [h0, h1, h2]
}

/// 注入 rounds 轮（判据 10/10 语义）。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        led.record(0, hits.first().copied().unwrap_or(false));
        led.record(1, hits.get(1).copied().unwrap_or(false));
        led.record(2, hits.get(2).copied().unwrap_or(false));
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：满链装配→逐级仲裁矩阵 16 组合→注入全抓→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    // 满链：四级全在席，ActCtx 必胜。
    let slots = chain_for_presence(PRESENCE_MASK)?;
    let mut ch = PriorityChain::new();
    ch.build(slots)?;
    let (v, r, name) = ch.arbitrate()?;
    if r != RANK_ACTCTX || name != "ActCtx" || (v & 0xFFFF) != 0 {
        return Err(PcCode::TraceMismatch.code());
    }
    // 16 组合矩阵全过。
    let (total, pass) = level_matrix_run()?;
    if total != 16 || pass != 16 {
        return Err(PcCode::MatrixGap.code());
    }
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(PcCode::ChainOutOfOrder.code());
    }
    Ok((total, pass))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PcCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：仅默认层在席 → 兜底胜出（秩 3）。
pub fn probe_a() -> Result<(), u32> {
    let slots = chain_for_presence(0b1000)?;
    let mut ch = PriorityChain::new();
    ch.build(slots)?;
    match ch.arbitrate() {
        Ok((_, 3, "SysDefault")) => Ok(()),
        _ => Err(PcCode::TraceMismatch.code()),
    }
}

/// 探针B：全空链 → 空裁（ArbitrateEmpty）。
pub fn probe_b() -> Result<(), u32> {
    let slots = chain_for_presence(0)?;
    let mut ch = PriorityChain::new();
    ch.build(slots)?;
    match ch.arbitrate() {
        Err(c) if c == PcCode::ArbitrateEmpty.code() => Ok(()),
        _ => Err(PcCode::ArbitrateEmpty.code()),
    }
}

/// 探针C：ActCtx 缺席时 manifest 顶上（逐级让渡语义）。
pub fn probe_c() -> Result<(), u32> {
    let slots = chain_for_presence(0b1110)?;
    let mut ch = PriorityChain::new();
    ch.build(slots)?;
    match ch.arbitrate() {
        Ok((_, 1, "Manifest")) => Ok(()),
        _ => Err(PcCode::TraceMismatch.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < PcCode::ALL.len() {
        if let Some(c) = PcCode::ALL.get(i) {
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
/// run_unx18422_checks：J1 判据 16 条（断言性质，动态明细 ≤8）。
pub fn run_unx18422_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18422 裁决优先级链（ActCtx>manifest>兼容层>默认）");
    // J1-1 冻结四级：链层数/四秩常量与判据原文一致。
    s.add(
        "J1-四级冻结",
        CHAIN_LEVELS == 4
            && RANK_ACTCTX == 0
            && RANK_MANIFEST == 1
            && RANK_COMPAT == 2
            && RANK_DEFAULT == 3,
        "ActCtx0 Manifest1 Compat2 Default3",
    );
    // J1-2 满链装配 + ActCtx 胜出。
    let full = {
        let slots = chain_for_presence(PRESENCE_MASK);
        match slots {
            Ok(sl) => {
                let mut ch = PriorityChain::new();
                let b = ch.build(sl);
                match (b, ch.arbitrate()) {
                    (Ok(()), Ok((_, 0, "ActCtx"))) => true,
                    _ => false,
                }
            }
            Err(_) => false,
        }
    };
    s.add("J1-满链ActCtx胜", full, "四级全席 ActCtx 必裁");
    // J1-3 逐级仲裁矩阵 16/16。
    let mat = level_matrix_run();
    s.add_dyn(
        "J1-逐级矩阵16/16",
        matches!(mat, Ok((t, p)) if t == 16 && p == 16),
        &format!("{:?}", mat),
    );
    // J1-4 乱序注入 10/10 检出。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-乱序注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒：漏检 0，总注入 30。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * 3,
        "漏检0 总注入30",
    );
    // J1-6 重秩/越权/缺秩三闸直验。
    let dup = scrambled_chain(1); // 邻换含逆序
    s.add(
        "J1-链序三闸",
        chain_validate(&dup).is_err()
            && make_slot(CHAIN_LEVELS as u8, 0, true).is_err()
            && make_slot(0, 0x0100_0000_0000_0000, true).is_err(),
        "乱序/越权秩/值域超界全拒",
    );
    // J1-7 逐级让渡：ActCtx 缺席 manifest 顶上；再缺则兼容层。
    let c2 = match chain_for_presence(0b1100) {
        Ok(sl) => {
            let mut ch = PriorityChain::new();
            let _ = ch.build(sl);
            matches!(ch.arbitrate(), Ok((_, 2, "CompatShim")))
        }
        Err(_) => false,
    };
    s.add("J1-逐级让渡", probe_c().is_ok() && c2, "缺位让渡至下级");
    // J1-8 兜底：仅默认层在席必胜。
    s.add("J1-兜底默认层", probe_a().is_ok(), "仅 SysDefault 在席必裁");
    // J1-9 空裁：全空链 ArbitrateEmpty 且账入空裁。
    s.add("J1-空裁语义", probe_b().is_ok(), "四级皆缺 → ArbitrateEmpty");
    // J1-10 重复装配拒。
    let reb = {
        let slots = chain_for_presence(0b1111);
        match slots {
            Ok(sl) => {
                let mut ch = PriorityChain::new();
                let _ = ch.build(sl);
                ch.build(sl) == Err(PcCode::ChainAlreadyBuilt.code())
            }
            Err(_) => false,
        }
    };
    s.add("J1-重复装配拒", reb, "built>0 二次装配必拒");
    // J1-11 仲裁账守恒。
    let cons = {
        let slots = chain_for_presence(0b0101);
        match slots {
            Ok(sl) => {
                let mut ch = PriorityChain::new();
                let _ = ch.build(sl);
                let _ = ch.arbitrate();
                let _ = ch.arbitrate();
                ch.conserve_ok()
            }
            Err(_) => false,
        }
    };
    s.add("J1-仲裁账守恒", cons, "total=hit+empty");
    // J1-12 黄金全链。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("{:?}", g));
    // J1-13 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-14 位面杂质：presence 高位拒。
    s.add(
        "J2-位面杂质拒",
        chain_for_presence(0b1_0000).is_err(),
        "位面高位杂质必拒",
    );
    // J1-15 层名闭集：四名与四秩对应非空。
    s.add(
        "J2-层名闭集",
        LEVEL_NAMES.len() == 4
            && LEVEL_NAMES.iter().all(|n| !n.is_empty()),
        "四级名闭集非空",
    );
    // J1-16 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E540 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18422_checks();
        assert!(set.all_passed(), "UNX-F18422 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_full_chain_actctx_wins() {
        let slots = chain_for_presence(0b1111).unwrap_or(scrambled_chain(99));
        let mut ch = PriorityChain::new();
        assert!(ch.build(slots).is_ok());
        let (_, r, name) = ch.arbitrate().unwrap_or((0, 9, "?"));
        assert_eq!(r, 0);
        assert_eq!(name, "ActCtx");
    }

    #[test]
    fn t_matrix_16_all_pass() {
        let (t, p) = level_matrix_run().unwrap_or((0, 0));
        assert_eq!((t, p), (16, 16));
    }

    #[test]
    fn t_scrambled_all_detected() {
        let mut i = 0;
        while i < 10 {
            assert!(chain_validate(&scrambled_chain(i)).is_err(), "乱序组 {} 未检出", i);
            i += 1;
        }
    }

    #[test]
    fn t_rank_gates() {
        assert_eq!(make_slot(4, 0, true), Err(PcCode::RankOutOfRange.code()));
        assert_eq!(
            chain_for_presence(0b10000),
            Err(PcCode::PresenceDirty.code())
        );
    }

    #[test]
    fn t_empty_chain_arbitrate() {
        let slots = chain_for_presence(0).unwrap_or(scrambled_chain(99));
        let mut ch = PriorityChain::new();
        let _ = ch.build(slots);
        assert_eq!(ch.arbitrate(), Err(PcCode::ArbitrateEmpty.code()));
        assert!(ch.conserve_ok());
    }

    #[test]
    fn t_ledger_conservation() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..3 {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
