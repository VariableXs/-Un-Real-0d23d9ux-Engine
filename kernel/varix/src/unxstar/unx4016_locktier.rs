//! unx4016_locktier — UNX-F4016 VFS 全局锁分级基座（诊断段 0x84A0）。
//!
//! 判据原文：
//!   J1 三层锁（sb 重命名锁/dcache 锁/file 偏移锁）获取顺序断言器运行零违例：
//!      合法获取序列必须按等级严格升序。
//!   J2 逆序获取注入 100/100 次被拦截并留账：每次逆序尝试返回 E_ORDER 且
//!      违例账 +1。
//!   J3 释放序无约束但必须持锁：释放未持有的锁返回 E_NOT_HELD。
//!   J4 锁账守恒：持有集大小 == 获取成功数 - 释放成功数，任何时刻成立。
//! 设计纪律：
//!   1. 锁等级锚定 Linux VFS 常见序：i_rwsem(sb 重命名) 0 → d_lock(dcache) 1
//!      → f_pos_lock(file 偏移) 2；等级号越小越先获取，跨级只准升序。
//!   2. 同级重复获取按「不可重入」处理（E_REENTR），避免自锁语义歧义；
//!      递归需求由调用方改用读写语义或分层拆分。
//!   3. 违例拦截必须在「置位之前」发生：入账后持有集不变，保证注入不污染
//!      后续合法序列（判据 J2 的 100/100 稳定拦截）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x84A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORDER: u16 = SEG_BASE + 1;   // 逆序获取（等级下降）
const E_REENTR: u16 = SEG_BASE + 2;  // 同级重复获取（不可重入）
const E_NOT_HELD: u16 = SEG_BASE + 3; // 释放未持有的锁
const E_TIER_BAD: u16 = SEG_BASE + 4; // 等级号越界（非 0/1/2）
const E_HELD_EMPTY: u16 = SEG_BASE + 5; // 空持有集上做依赖检查
const E_BAL: u16 = SEG_BASE + 6;     // 锁账不守恒

/// 三层锁等级（数值即获取优先级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockTier {
    SbRename = 0, // sb 重命名锁（i_rwsem 写侧）
    Dcache = 1,   // dcache 锁（d_lock）
    FilePos = 2,  // file 偏移锁（f_pos_lock）
}

pub const TIER_COUNT: usize = 3;

/// 等级名（日志与导出用）。
pub const TIER_NAMES: [&'static str; TIER_COUNT] = [
    "sb_rename_lock",
    "dcache_lock",
    "file_pos_lock",
];

/// 单把锁的持有信息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockSlot {
    pub tier: LockTier, // 所属等级
    pub held: bool,     // 当前是否持有
    pub acquires: u32,  // 累计成功获取
    pub releases: u32,  // 累计成功释放
    pub contended: u32, // 被拒次数（顺序/重入）
}

impl LockSlot {
    fn new(t: LockTier) -> LockSlot {
        LockSlot { tier: t, held: false, acquires: 0, releases: 0, contended: 0 }
    }
}

/// 持有集状态机：Empty → Partial → Full（按已持等级数推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoldState {
    Empty = 0,   // 未持任何锁
    Partial = 1, // 持有 1..2 把
    Full = 2,    // 三层全持
}

/// 锁集合：三槽 + 严格升序断言器 + 违例账。
pub struct LockSet {
    pub slots: [LockSlot; TIER_COUNT], // 三等级槽
    pub max_rank_held: i8,             // 当前持有最高等级（-1=空集）
    pub order_viol: u32,               // 逆序违例账
    pub reent_viol: u32,               // 重入违例账
    pub ops: u32,                      // 获取/释放总操作账
    pub hold: HoldState,               // 持有集状态机
}

impl LockSet {
    pub fn new() -> LockSet {
        LockSet {
            slots: [
                LockSlot::new(LockTier::SbRename),
                LockSlot::new(LockTier::Dcache),
                LockSlot::new(LockTier::FilePos),
            ],
            max_rank_held: -1,
            order_viol: 0,
            reent_viol: 0,
            ops: 0,
            hold: HoldState::Empty,
        }
    }

    /// 重算持有集状态（每次成功获取/释放后调用）。
    fn refresh_hold(&mut self) {
        let n = self.held_count();
        self.hold = match n {
            0 => HoldState::Empty,
            v if v >= TIER_COUNT as u32 => HoldState::Full,
            _ => HoldState::Partial,
        };
    }

    /// 状态机读出口。
    pub fn hold_state(&self) -> HoldState {
        self.hold
    }

    fn idx(t: LockTier) -> usize {
        t as usize
    }

    /// 获取：升序校验 + 不可重入校验均在置位之前（判据 J1/J2）。
    pub fn acquire(&mut self, t: LockTier) -> Result<(), u16> {
        let i = LockSet::idx(t);
        if i >= TIER_COUNT {
            return Err(E_TIER_BAD);
        }
        self.ops += 1;
        if self.slots[i].held {
            self.slots[i].contended += 1;
            self.reent_viol += 1;
            return Err(E_REENTR);
        }
        let rank = t as i8;
        if rank < self.max_rank_held {
            // 逆序：等级低于当前持有最高等级，拦截且不改持有集。
            self.slots[i].contended += 1;
            self.order_viol += 1;
            return Err(E_ORDER);
        }
        self.slots[i].held = true;
        self.slots[i].acquires += 1;
        if rank > self.max_rank_held {
            self.max_rank_held = rank;
        }
        self.refresh_hold();
        Ok(())
    }

    /// 释放：必须持有；释放后重算 max_rank_held（判据 J3）。
    pub fn release(&mut self, t: LockTier) -> Result<(), u16> {
        let i = LockSet::idx(t);
        if i >= TIER_COUNT {
            return Err(E_TIER_BAD);
        }
        self.ops += 1;
        if !self.slots[i].held {
            return Err(E_NOT_HELD);
        }
        self.slots[i].held = false;
        self.slots[i].releases += 1;
        // 重算最高持有等级：从高往低扫。
        let mut m: i8 = -1;
        let mut k = TIER_COUNT;
        while k > 0 {
            k -= 1;
            if self.slots[k].held {
                m = k as i8;
                break;
            }
        }
        self.max_rank_held = m;
        self.refresh_hold();
        Ok(())
    }

    /// 一次成对获取-释放（便捷入口），返回中间持有数。
    pub fn with<T>(&mut self, t: LockTier, f: T) -> Result<(), u16>
    where
        T: FnOnce() -> Result<(), u16>,
    {
        self.acquire(t)?;
        let r = f();
        self.release(t)?;
        r
    }

    /// 持有集大小。
    pub fn held_count(&self) -> u32 {
        let mut c = 0u32;
        for s in self.slots.iter() {
            if s.held {
                c += 1;
            }
        }
        c
    }

    /// 持有集导出的等级位图（bit i = 等级 i 持有）。
    pub fn held_mask(&self) -> u32 {
        let mut m = 0u32;
        for i in 0..TIER_COUNT {
            if self.slots[i].held {
                m |= 1u32 << i;
            }
        }
        m
    }

    /// 锁账守恒：Σ(acquires - releases) == 持有数（判据 J4）。
    pub fn conserve(&self) -> bool {
        let mut d: i64 = 0;
        for s in self.slots.iter() {
            d += s.acquires as i64 - s.releases as i64;
        }
        d == self.held_count() as i64
    }

    /// max_rank_held 与位图一致校验。
    pub fn rank_consistent(&self) -> bool {
        let mask = self.held_mask();
        if mask == 0 {
            return self.max_rank_held == -1;
        }
        let mut m: i8 = -1;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if mask & (1u32 << i) != 0 {
                m = i as i8;
            }
            i += 1;
        }
        m == self.max_rank_held
    }

    /// 依赖检查：要求同时持有 lower 与 higher（且 lower 等级更小）。
    pub fn requires(&self, lower: LockTier, higher: LockTier) -> Result<(), u16> {
        if self.held_count() == 0 {
            return Err(E_HELD_EMPTY);
        }
        if (lower as i8) >= (higher as i8) {
            return Err(E_TIER_BAD);
        }
        if !self.slots[LockSet::idx(lower)].held || !self.slots[LockSet::idx(higher)].held {
            return Err(E_NOT_HELD);
        }
        Ok(())
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ORDER => "逆序获取（等级下降，顺序断言器拦截）",
        E_REENTR => "同级重复获取（锁不可重入）",
        E_NOT_HELD => "释放或依赖检查时未持有",
        E_TIER_BAD => "等级号越界（非 0/1/2）",
        E_HELD_EMPTY => "空持有集上做依赖检查",
        E_BAL => "锁账不守恒",
        _ => "未知诊断码",
    }
}

/// 确定性演练：升序获取 → 逆序 100/100 拦截 → 释放序 → 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ls = LockSet::new();

    // R1：严格升序获取三层（0→1→2），零违例（判据 J1 原文）。
    ls.acquire(LockTier::SbRename)?;
    ls.acquire(LockTier::Dcache)?;
    ls.acquire(LockTier::FilePos)?;
    if ls.held_count() != 3 || ls.max_rank_held != 2 {
        return Err(E_BAL);
    }
    if ls.hold_state() != HoldState::Full {
        return Err(E_BAL);
    }
    if ls.order_viol != 0 || ls.reent_viol != 0 {
        return Err(E_ORDER);
    }
    if !ls.rank_consistent() || !ls.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：逆序释放（2→1→0）合法，逐层重算最高等级（判据 J3）。
    ls.release(LockTier::FilePos)?;
    if ls.max_rank_held != 1 || ls.held_count() != 2 {
        return Err(E_BAL);
    }
    ls.release(LockTier::Dcache)?;
    if ls.max_rank_held != 0 || ls.held_count() != 1 {
        return Err(E_BAL);
    }
    ls.release(LockTier::SbRename)?;
    if ls.max_rank_held != -1 || ls.held_count() != 0 {
        return Err(E_BAL);
    }
    if ls.hold_state() != HoldState::Empty {
        return Err(E_BAL);
    }
    if !ls.rank_consistent() || !ls.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：跳级获取合法（0→2），跳过 dcache 仍满足升序。
    ls.acquire(LockTier::SbRename)?;
    ls.acquire(LockTier::FilePos)?;
    if ls.held_mask() != 0b101 || ls.max_rank_held != 2 {
        return Err(E_ORDER);
    }
    if ls.hold_state() != HoldState::Partial {
        return Err(E_BAL);
    }
    ls.release(LockTier::FilePos)?;
    ls.release(LockTier::SbRename)?;
    hits += 1;

    // R4：逆序注入 100/100 全拦截并留账（判据 J2 原文）。
    let seq = [
        (LockTier::Dcache, LockTier::SbRename), // 1 后取 0：逆序
        (LockTier::FilePos, LockTier::Dcache),  // 2 后取 1：逆序
        (LockTier::FilePos, LockTier::SbRename), // 2 后取 0：逆序
        (LockTier::Dcache, LockTier::Dcache),   // 同级：重入
    ];
    let mut blocked = 0u32;
    for n in 0..100u32 {
        let (hold, want) = seq[(n % 4) as usize];
        let mut probe = LockSet::new();
        probe.acquire(hold)?;
        let r = probe.acquire(want);
        let expect = if hold == want { E_REENTR } else { E_ORDER };
        if r != Err(expect) {
            return Err(E_ORDER);
        }
        // 拦截后持有集不得变化（注入不污染）。
        if probe.held_count() != 1 {
            return Err(E_BAL);
        }
        blocked += 1;
    }
    if blocked != 100 {
        return Err(E_ORDER);
    }
    hits += 1;

    // R5：连续逆序注入在同一实例上 100 次，违例账精确 +100 且持有集稳定。
    let mut p = LockSet::new();
    p.acquire(LockTier::FilePos)?; // 持有最高级 2
    let mut rej = 0u32;
    for _ in 0..100u32 {
        if p.acquire(LockTier::SbRename) == Err(E_ORDER) {
            rej += 1;
        }
    }
    if rej != 100 || p.order_viol != 100 {
        return Err(E_ORDER);
    }
    if p.held_count() != 1 || p.max_rank_held != 2 {
        return Err(E_BAL);
    }
    if !p.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：依赖检查——同时持 0 与 1 时 requires(0,1) 通过；只持 0 时拒。
    let mut d = LockSet::new();
    d.acquire(LockTier::SbRename)?;
    if d.requires(LockTier::SbRename, LockTier::Dcache) != Err(E_NOT_HELD) {
        return Err(E_NOT_HELD);
    }
    d.acquire(LockTier::Dcache)?;
    if d.requires(LockTier::SbRename, LockTier::Dcache) != Ok(()) {
        return Err(E_NOT_HELD);
    }
    if d.requires(LockTier::Dcache, LockTier::SbRename) != Err(E_TIER_BAD) {
        return Err(E_TIER_BAD);
    }
    hits += 1;

    // R7：成对入口 with() 正常路径——前后持有集回空。
    let mut w = LockSet::new();
    w.with(LockTier::Dcache, || Ok(()))?;
    if w.held_count() != 0 || !w.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：释放未持有注入被拒（判据 J3）。
    if w.release(LockTier::SbRename) != Err(E_NOT_HELD) {
        return Err(E_NOT_HELD);
    }
    hits += 1;

    // I2：同级重复获取注入被拒并计入重入账。
    w.acquire(LockTier::Dcache)?;
    if w.acquire(LockTier::Dcache) != Err(E_REENTR) {
        return Err(E_REENTR);
    }
    if w.reent_viol != 1 {
        return Err(E_BAL);
    }
    w.release(LockTier::Dcache)?;
    hits += 1;

    // I3：空持有集做依赖检查被拒。
    if LockSet::new().requires(LockTier::SbRename, LockTier::Dcache) != Err(E_HELD_EMPTY) {
        return Err(E_HELD_EMPTY);
    }
    hits += 1;

    // I4：逆序注入后合法序列仍可完成——证明拦截不污染（判据 J2 的后置）。
    let mut q = LockSet::new();
    q.acquire(LockTier::Dcache)?;
    let _ = q.acquire(LockTier::SbRename); // 拦截
    q.release(LockTier::Dcache)?;
    q.acquire(LockTier::SbRename)?;
    q.acquire(LockTier::Dcache)?;
    q.acquire(LockTier::FilePos)?;
    if q.held_count() != 3 || q.max_rank_held != 2 {
        return Err(E_ORDER);
    }
    if !q.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
