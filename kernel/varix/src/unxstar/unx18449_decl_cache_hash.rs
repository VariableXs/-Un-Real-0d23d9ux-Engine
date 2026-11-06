//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18449 · 声明缓存与失效联动 · 0x7EC00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18449-J1 声明缓存随文件变更失效断言全过（哈希比对），
//!       脏缓存注入检出
//! 本模块结构清单：
//!   1. 冻结常量层：缓存槽数 8 / 哈希口径 FNV-1a 32 / 双账互证口径
//!      （内容哈希账 × 守卫哈希账——守卫 = FNV(内容哈希‖世代位面)，
//!      篡改内容哈希而不动守卫即检出）；
//!   2. 诊断码闭集 CcCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构 DeclCache：8 槽定长缓存，槽项（path_id/内容哈希/
//!      守卫哈希/世代位面/逻辑时钟戳），LRU 语义（满覆最旧戳）；
//!   4. 失效联动：validate(path_id, 新内容) → 哈希比对 → 命中（缓存
//!      有效）或失配（失效联动：槽清零 + 失效事件计数）；
//!   5. 脏缓存注入核 4 类：内容变更未失效（陈旧命中必须失配）/
//!      内容哈希篡改（守卫互证检出）/ 未知 path_id 查询（干净未命中
//!      不崩）/ 槽满覆盖（最旧被逐，最新存活）；
//!   6. 边界：空表查询 / 重复装载同内容幂等（哈希相等不失效）/
//!      位面随失效清零 / 双跑一致；
//!   7. run_unx18449_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EC00，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EC00;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 缓存槽数（写死 8：单进程声明缓存定长）。
pub const SLOT_N: usize = 8;
/// 哈希偏移基（FNV-1a 32，写死）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// 哈希素数（FNV-1a 32，写死）。
pub const FNV_PRIME: u32 = 0x0100_0193;
/// 逻辑时钟上限（写死 1<<30：戳环绕前的单调域）。
pub const STAMP_MAX: u32 = 1 << 30;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;
/// 世代位面宽（写死 5：5 规范世代，衔接 F18442/F18443 位面口径）。
pub const MASK_BITS: u32 = 5;

/// FNV-1a 32（字节串）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 守卫哈希：FNV(内容哈希 4B 小端 ‖ 位面 1B)。
/// 为什么双账：内容哈希单账可被整体篡改（脏缓存注入面），守卫账
/// 捆绑位面一起哈希，改内容哈希不改守卫即失配检出。
pub fn guard_hash(content_hash: u32, mask: u8) -> u32 {
    let mut buf = [0u8; 5];
    buf[0] = (content_hash & 0xFF) as u8;
    buf[1] = ((content_hash >> 8) & 0xFF) as u8;
    buf[2] = ((content_hash >> 16) & 0xFF) as u8;
    buf[3] = ((content_hash >> 24) & 0xFF) as u8;
    buf[4] = mask;
    fnv1a(&buf)
}

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CcCode {
    /// 陈旧命中：内容已变而缓存未失效
    StaleHit,
    /// 守卫失配：内容哈希被篡改
    GuardMismatch,
    /// 未知路径：干净未命中（不崩）
    PathMiss,
    /// 槽满覆盖：最旧被逐
    SlotEvicted,
    /// 位面越界：位面超出 5 位闭集
    MaskDirty,
    /// 守恒破坏：槽占用数 ≠ 有效项计数
    LedgerBroken,
    /// 哈希重算失配
    HashMismatch,
    /// 失效事件账破坏
    InvalidateBroken,
    /// 戳越界：逻辑时钟溢出
    StampOverflow,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
    /// 幂等破坏：同内容重复装载产生失效
    NotIdempotent,
}

impl CcCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CcCode::StaleHit => "陈旧命中：内容已变缓存未失效",
            CcCode::GuardMismatch => "守卫失配：内容哈希被篡改",
            CcCode::PathMiss => "未知路径：干净未命中",
            CcCode::SlotEvicted => "槽满覆盖：最旧被逐",
            CcCode::MaskDirty => "位面越界：超 5 位闭集",
            CcCode::LedgerBroken => "槽占用守恒破坏",
            CcCode::HashMismatch => "哈希重算失配",
            CcCode::InvalidateBroken => "失效事件账破坏",
            CcCode::StampOverflow => "逻辑时钟溢出",
            CcCode::InjectMissed => "注入未检出",
            CcCode::NotReproducible => "双跑不一致：复现红线",
            CcCode::NotIdempotent => "幂等破坏：同内容重复装载误失效",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [CcCode; 12] = [
        CcCode::StaleHit,
        CcCode::GuardMismatch,
        CcCode::PathMiss,
        CcCode::SlotEvicted,
        CcCode::MaskDirty,
        CcCode::LedgerBroken,
        CcCode::HashMismatch,
        CcCode::InvalidateBroken,
        CcCode::StampOverflow,
        CcCode::InjectMissed,
        CcCode::NotReproducible,
        CcCode::NotIdempotent,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CcCode::ALL.len() {
        if CcCode::ALL[i].code() == code {
            return CcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EC00 段闭集内"
}

// ── 缓存项与缓存表 ────────────────────────────────────────────────
/// 单槽项（used=false 表示空槽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheSlot {
    /// 占用位。
    pub used: bool,
    /// 路径 id（仓内编号，非真实文件路径——内核口径零字符串键）。
    pub path_id: u32,
    /// 内容哈希（FNV-1a of 文件字节）。
    pub content_hash: u32,
    /// 守卫哈希（捆绑位面）。
    pub guard: u32,
    /// 世代位面（5 位闭集）。
    pub mask: u8,
    /// 逻辑时钟戳（装载序，LRU 依据）。
    pub stamp: u32,
}

impl CacheSlot {
    /// 空槽。
    pub const EMPTY: CacheSlot = CacheSlot { used: false, path_id: 0, content_hash: 0, guard: 0, mask: 0, stamp: 0 };
}

/// 声明缓存：8 槽定长 + 逻辑时钟 + 失效事件计数。
pub struct DeclCache {
    /// 槽表。
    pub slots: [CacheSlot; SLOT_N],
    /// 逻辑时钟（单调递增）。
    pub clock: u32,
    /// 失效事件数（哈希失配触发的失效联动次数）。
    pub invalidations: u32,
    /// 装载成功数（store 成功次数）。
    pub stores: u32,
}

impl DeclCache {
    /// 新缓存：8 槽全空。
    pub fn new() -> DeclCache {
        DeclCache {
            slots: [CacheSlot::EMPTY; SLOT_N],
            clock: 0,
            invalidations: 0,
            stores: 0,
        }
    }

    /// 位面域闸：5 位闭集外拒绝。
    fn mask_ok(mask: u8) -> bool {
        (mask as u32) < (1u32 << MASK_BITS)
    }

    /// 装载：path_id + 内容 + 位面入缓存。
    /// 同 path_id 已存在：哈希相等 → 幂等更新戳（不失效）；不等 →
    /// 失效联动（清旧项再入新项，invalidations+1）。
    /// 槽满且无同 id：覆盖最旧戳槽（LRU），invalidations 不计
    /// （覆盖是容量语义，不是失效语义）。
    pub fn store(&mut self, path_id: u32, content: &[u8], mask: u8) -> Result<(), u32> {
        if !Self::mask_ok(mask) {
            return Err(CcCode::MaskDirty.code());
        }
        if self.clock >= STAMP_MAX {
            return Err(CcCode::StampOverflow.code());
        }
        let ch = fnv1a(content);
        // 同 id 在场？
        let mut found = SLOT_N;
        let mut i = 0;
        while i < SLOT_N {
            if self.slots[i].used && self.slots[i].path_id == path_id {
                found = i;
                break;
            }
            i += 1;
        }
        if found < SLOT_N {
            self.clock += 1;
            if self.slots[found].content_hash == ch {
                // 幂等：只刷新戳与守卫。
                self.slots[found].stamp = self.clock;
                self.slots[found].guard = guard_hash(ch, mask);
                self.slots[found].mask = mask;
                self.stores += 1;
                return Ok(());
            }
            // 失效联动：清旧再入新。
            self.invalidations += 1;
            self.slots[found] = CacheSlot::EMPTY;
        }
        // 找空槽或最旧槽。
        let mut empty = SLOT_N;
        let mut oldest = SLOT_N;
        let mut oldest_stamp = u32::MAX;
        let mut k = 0;
        while k < SLOT_N {
            if !self.slots[k].used {
                empty = k;
                break;
            }
            if self.slots[k].stamp < oldest_stamp {
                oldest_stamp = self.slots[k].stamp;
                oldest = k;
            }
            k += 1;
        }
        let target = if empty < SLOT_N {
            empty
        } else if oldest < SLOT_N {
            oldest
        } else {
            return Err(CcCode::LedgerBroken.code());
        };
        self.clock += 1;
        self.slots[target] = CacheSlot {
            used: true,
            path_id,
            content_hash: ch,
            guard: guard_hash(ch, mask),
            mask,
            stamp: self.clock,
        };
        self.stores += 1;
        Ok(())
    }

    /// 查询：path_id + 当前内容字节 → 命中(位面) / 未命中。
    /// 哈希比对：内容哈希不等 → 失效联动（清槽 + invalidations+1），
    /// 返回 StaleHit 错——缓存随文件变更失效的机器断言。
    /// 守卫互证：在场项守卫哈希重算不等 → GuardMismatch（脏缓存）。
    pub fn validate(&mut self, path_id: u32, current: &[u8]) -> Result<u8, u32> {
        let mut i = 0;
        while i < SLOT_N {
            if self.slots[i].used && self.slots[i].path_id == path_id {
                let ch = fnv1a(current);
                // 守卫互证先行（脏缓存注入面）。
                if self.slots[i].guard != guard_hash(self.slots[i].content_hash, self.slots[i].mask) {
                    self.slots[i] = CacheSlot::EMPTY;
                    self.invalidations += 1;
                    return Err(CcCode::GuardMismatch.code());
                }
                if self.slots[i].content_hash != ch {
                    // 失效联动：内容已变 → 清槽。
                    self.slots[i] = CacheSlot::EMPTY;
                    self.invalidations += 1;
                    return Err(CcCode::StaleHit.code());
                }
                return Ok(self.slots[i].mask);
            }
            i += 1;
        }
        Err(CcCode::PathMiss.code())
    }

    /// 查缓存内位面（不校验内容——仅读在场项）。
    pub fn peek(&self, path_id: u32) -> Option<u8> {
        let mut i = 0;
        while i < SLOT_N {
            if self.slots[i].used && self.slots[i].path_id == path_id {
                return Some(self.slots[i].mask);
            }
            i += 1;
        }
        None
    }

    /// 守恒：used 槽数与各槽自洽（守卫可重算）。
    pub fn ledger_ok(&self) -> bool {
        let mut used_n = 0u32;
        let mut i = 0;
        while i < SLOT_N {
            if self.slots[i].used {
                used_n += 1;
                if self.slots[i].guard != guard_hash(self.slots[i].content_hash, self.slots[i].mask) {
                    return false;
                }
                if !Self::mask_ok(self.slots[i].mask) {
                    return false;
                }
            }
            i += 1;
        }
        used_n as usize <= SLOT_N
    }
}

// ── 注入核（4 类坏样本，逐类断言检出） ────────────────────────────
/// 注入 1：内容变更未失效——store 后改内容再 validate → StaleHit。
pub fn inject_stale() -> Result<(), u32> {
    let mut c = DeclCache::new();
    c.store(7, b"<manifest v1/>", 0b10000)?;
    // 文件变更：v1 → v2。
    match c.validate(7, b"<manifest v2/>") {
        Err(cc) if cc == CcCode::StaleHit.code() => {
            // 失效联动：槽已清，再查干净未命中。
            if c.peek(7).is_some() {
                return Err(CcCode::InvalidateBroken.code());
            }
            Ok(())
        }
        Ok(_) => Err(CcCode::StaleHit.code()),
        Err(_) => Err(CcCode::InjectMissed.code()),
    }
}

/// 注入 2：脏缓存——内容哈希被篡改（守卫不动）→ GuardMismatch。
pub fn inject_dirty() -> Result<(), u32> {
    let mut c = DeclCache::new();
    c.store(3, b"<manifest/>", 0b00001)?;
    // 篡改槽 0 的内容哈希（模拟脏缓存）。
    c.slots[0].content_hash = c.slots[0].content_hash ^ 0xDEAD;
    match c.validate(3, b"<manifest/>") {
        Err(cc) if cc == CcCode::GuardMismatch.code() => Ok(()),
        _ => Err(CcCode::InjectMissed.code()),
    }
}

/// 注入 3：未知 path_id 查询——干净未命中不崩。
pub fn inject_path_miss() -> Result<(), u32> {
    let mut c = DeclCache::new();
    c.store(1, b"<a/>", 0b00010)?;
    match c.validate(999, b"<a/>") {
        Err(cc) if cc == CcCode::PathMiss.code() => Ok(()),
        _ => Err(CcCode::InjectMissed.code()),
    }
}

/// 注入 4：槽满覆盖——第 9 个 path_id 入场，最旧（id=0）被逐。
pub fn inject_evict() -> Result<(), u32> {
    let mut c = DeclCache::new();
    let mut id = 0u32;
    while id < SLOT_N as u32 {
        c.store(id, b"<m/>", 0b00001)?;
        id += 1;
    }
    c.store(SLOT_N as u32, b"<m/>", 0b00010)?;
    if c.peek(0).is_some() {
        return Err(CcCode::SlotEvicted.code());
    }
    if c.peek(SLOT_N as u32) != Some(0b00010) {
        return Err(CcCode::InjectMissed.code());
    }
    Ok(())
}

/// 注入轮次：JUDGE_ROUNDS 轮 4 类全检出。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_stale().is_err() || inject_dirty().is_err() || inject_path_miss().is_err() || inject_evict().is_err() {
            return false;
        }
        r += 1;
    }
    true
}

// ── 守恒与确定性 ───────────────────────────────────────────────────
/// 幂等检查：同内容重复装载不产生失效。
pub fn idempotent_check() -> Result<(), u32> {
    let mut c = DeclCache::new();
    c.store(5, b"<same/>", 0b00100)?;
    c.store(5, b"<same/>", 0b00100)?;
    if c.invalidations != 0 {
        return Err(CcCode::NotIdempotent.code());
    }
    if c.validate(5, b"<same/>") != Ok(0b00100) {
        return Err(CcCode::NotIdempotent.code());
    }
    Ok(())
}

/// 哈希口径自检：FNV-1a 手算向量互证（"a" = 0xE40C292C 标准测试向量）。
pub fn hash_vector_check() -> Result<(), u32> {
    // FNV-1a 32 标准向量：fnv1a("")=0x811C9DC5，fnv1a("a")=0xE40C292C。
    if fnv1a(b"") != 0x811C_9DC5 || fnv1a(b"a") != 0xE40C_292C {
        return Err(CcCode::HashMismatch.code());
    }
    Ok(())
}

/// 双跑一致性。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_rounds();
    let b = inject_rounds();
    if a != b {
        return Err(CcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CcCode::ALL.len() {
        let v = CcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：哈希向量→装载→命中→同内容幂等→变内容失效→脏缓存检出
/// →槽满覆盖→守恒→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    hash_vector_check()?;
    let mut c = DeclCache::new();
    // 三个 path 装载，位面各异。
    c.store(10, b"<m10/>", 0b00001)?;
    c.store(11, b"<m11/>", 0b01000)?;
    c.store(12, b"<m12/>", 0b10000)?;
    if c.validate(10, b"<m10/>") != Ok(0b00001) {
        return Err(CcCode::StaleHit.code());
    }
    if c.validate(11, b"<m11/>") != Ok(0b01000) {
        return Err(CcCode::StaleHit.code());
    }
    // 同内容幂等。
    c.store(10, b"<m10/>", 0b00001)?;
    if c.invalidations != 0 {
        return Err(CcCode::NotIdempotent.code());
    }
    // 变内容失效。
    c.store(10, b"<m10-new/>", 0b00010)?;
    if c.invalidations != 1 {
        return Err(CcCode::InvalidateBroken.code());
    }
    if c.peek(10) != Some(0b00010) {
        return Err(CcCode::InvalidateBroken.code());
    }
    // 注入全检出。
    if !inject_rounds() {
        return Err(CcCode::InjectMissed.code());
    }
    // 守恒。
    if !c.ledger_ok() {
        return Err(CcCode::LedgerBroken.code());
    }
    repro_check()?;
    Ok((c.stores, c.invalidations))
}

// ── 判据（16 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18449_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18449 声明缓存与失效联动");
    // 1. 哈希口径自检（标准向量）。
    s.add("J1-哈希向量自检", hash_vector_check().is_ok(), "FNV-1a 标准向量 0x811C9DC5/0xE40C292C");
    // 2. 内容变更失效（J1 前半句主体）。
    s.add("J1-内容变更失效", inject_stale().is_ok(), "v1→v2 validate 失配 → 槽清零");
    // 3. 脏缓存检出（J1 后半句主体）。
    s.add("J1-脏缓存检出", inject_dirty().is_ok(), "哈希篡改守卫互证 GuardMismatch");
    // 4. 未知路径干净未命中。
    s.add("J1-未知路径不崩", inject_path_miss().is_ok(), "PathMiss 显式拒");
    // 5. 槽满覆盖最旧。
    s.add("J1-槽满覆盖最旧", inject_evict().is_ok(), "第 9 项入场 id=0 被逐");
    // 6. 注入多轮复现。
    s.add("J1-注入3轮全检出", inject_rounds(), "4 类 ×3 轮");
    // 7. 同内容幂等。
    s.add("J1-同内容幂等", idempotent_check().is_ok(), "重复装载零失效");
    // 8. 变内容重装载：失效联动 + 新位面在场。
    let rel = {
        let mut c = DeclCache::new();
        c.store(2, b"<old/>", 0b00001).is_ok()
            && c.store(2, b"<new/>", 0b00100).is_ok()
            && c.invalidations == 1
            && c.peek(2) == Some(0b00100)
    };
    s.add("J1-变内容重装载", rel, "失效清旧→新项入位");
    // 9. 位面域闸。
    s.add("J1-位面域闸", DeclCache::new().store(1, b"<x/>", 0b100000).is_err(), "6 位面显式拒");
    // 10. 守卫哈希确定性：同输入两次逐位一致。
    s.add(
        "J1-守卫哈希确定性",
        guard_hash(0x1234_5678, 0b10101) == guard_hash(0x1234_5678, 0b10101)
            && guard_hash(0x1234_5678, 0b10101) != guard_hash(0x1234_5678, 0b10100),
        "同输入同输出/位面变守卫变",
    );
    // 11. 失效事件账：store+validate 联动计数准确。
    let inv = {
        let mut c = DeclCache::new();
        let _ = c.store(4, b"<a/>", 0b00001);
        let _ = c.validate(4, b"<b/>"); // StaleHit → invalidations=1
        c.invalidations == 1
    };
    s.add("J1-失效事件账", inv, "失配即计数");
    // 12. 守恒。
    let cons = {
        let mut c = DeclCache::new();
        let mut k = 0;
        while k < SLOT_N {
            let _ = c.store(k as u32, b"<m/>", 0b00001);
            k += 1;
        }
        c.ledger_ok() && c.stores == SLOT_N as u32
    };
    s.add("J1-缓存账守恒", cons, "8 槽全占守卫全可重算");
    // 13. 逻辑时钟单调：装载序戳严格递增。
    let mono = {
        let mut c = DeclCache::new();
        let _ = c.store(1, b"<a/>", 0b00001);
        let mut s1 = 0u32;
        let _ = c.store(2, b"<b/>", 0b00001);
        let mut s2 = 0u32;
        let mut si = 0;
        while si < SLOT_N {
            if c.slots[si].used && c.slots[si].path_id == 1 {
                s1 = c.slots[si].stamp;
            }
            if c.slots[si].used && c.slots[si].path_id == 2 {
                s2 = c.slots[si].stamp;
            }
            si += 1;
        }
        s2 > s1
    };
    s.add("J1-时钟单调", mono, "stamp 严格递增（LRU 依据）");
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "注入轮次两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EC00 窗口");
    // 16. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18449_checks_green() {
        let set = run_unx18449_checks();
        assert!(set.all_passed(), "UNX-F18449 red: {}", set.red_items_text());
    }

    #[test]
    fn t_fnv_vectors() {
        assert_eq!(fnv1a(b""), 0x811C_9DC5);
        assert_eq!(fnv1a(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a(b"foobar"), 0xBF9C_F968);
    }

    #[test]
    fn t_store_validate_roundtrip() {
        let mut c = DeclCache::new();
        c.store(9, b"<x/>", 0b01010).unwrap_or_else(|cc| panic!("store {}", detail(cc)));
        assert_eq!(c.validate(9, b"<x/>"), Ok(0b01010));
    }

    #[test]
    fn t_stale_then_refill() {
        let mut c = DeclCache::new();
        c.store(6, b"<v1/>", 0b00001).unwrap_or(());
        assert!(c.validate(6, b"<v2/>").is_err());
        c.store(6, b"<v2/>", 0b00010).unwrap_or(());
        assert_eq!(c.validate(6, b"<v2/>"), Ok(0b00010));
        assert_eq!(c.invalidations, 1); // 仅 validate 失配计 1（重装载走空槽不另计）
    }

    #[test]
    fn t_dirty_and_miss() {
        assert!(inject_dirty().is_ok());
        assert!(inject_path_miss().is_ok());
        assert!(inject_evict().is_ok());
    }
}
