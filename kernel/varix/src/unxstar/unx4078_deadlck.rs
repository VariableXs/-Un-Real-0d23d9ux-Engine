//! unx4078_deadlck — UNX-F4078 死锁注入回归（锁序断言器联动）（诊断段 0x8C00）。
//!
//! 判据原文：UNX-F4078-J1 缓存锁与 B01 三层锁的获取顺序断言器联动回归零违例，
//!           双锁交叉注入 100/100 次被拦截。
//! 可验性质（本模块自证面）：
//!   J1 六类锁的等级表全序且两两不同，等级值升序获取时断言器零违例；
//!   J2 反向获取（先高后低）必须被拦，100 次双锁交叉注入 100/100 拦截，
//!      拦截后 assertor 的 held 栈必须回到注入前状态（不残留半持锁）；
//!   J3 合法方向不得误伤：先低后高的同组合 100 次必须 100/100 放行；
//!   J4 持锁/放锁账守恒：acq == rel + held_len，深度峰值不得超过深度上限；
//!   J5 重入（同锁二次获取）与非法放锁（放非栈顶 / 放未持）各自独立拒绝，
//!      拒绝路径不入 held 栈、不动 acq 账。
//! 设计纪律：
//!   1. 锁等级写死并注来源：VFS 侧 s_umount(1) < i_rwsem(2) < d_lock(3)，
//!      B01 三层锁（L1=10/L2=11/L3=12）整体在 VFS 之上，故 VFS 锁永远先于
//!      B01 锁获取——跨层倒序即死锁候选，断言器直接拦。
//!   2. 断言器只做"顺序判定"，不真加锁：它记录 held 栈并检查单调性，是纯功能
//!      模型；真锁由调用方持有（本模块不下探调度器，无 IO、无墙钟）。
//!   3. 拦截必须干净：注入失败时把本次已获取的那把也放掉（自愈），否则一次
//!      注入就把账带偏，回归基线失效。
//!   4. 深度上限 MAX_DEPTH 取 8：VFS 侧最深路径（sb→inode→dentry→创建者）加
//!      B01 三层仍有余量，触顶即视为疑似环路。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8C00 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8C00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORDER_INV: u16 = SEG_BASE + 1; // 锁序倒置（先高后低，死锁候选）
const E_REENTRANT: u16 = SEG_BASE + 2; // 同锁重入（非递归锁）
const E_NOT_TOP: u16 = SEG_BASE + 3; // 放锁非栈顶（破坏 LIFO）
const E_NOT_HELD: u16 = SEG_BASE + 4; // 放未持之锁
const E_DEPTH: u16 = SEG_BASE + 5; // 持锁深度触顶（疑似环路）
const E_CLASS_OOR: u16 = SEG_BASE + 6; // 锁类码越界
const E_BAL: u16 = SEG_BASE + 7; // 持放账不平
const E_INJECT_MISS: u16 = SEG_BASE + 8; // 注入未被拦截（断言器失效）

/// 锁类闭集：VFS 三把 + B01 三层锁（跨层联动是判据的联动面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockClass {
    Sb = 0,     // s_umount（超级块卸载互斥）
    Inode = 1,  // i_rwsem（inode 读写信号量）
    Dentry = 2, // d_lock（dentry 自旋锁）
    B01L1 = 3,  // B01 第一层锁
    B01L2 = 4,  // B01 第二层锁
    B01L3 = 5,  // B01 第三层锁
}

/// 锁类总数，与 CLS_N 强绑定。
pub const CLS_N: usize = 6;

/// 持锁深度上限（触顶视为疑似环路，见设计纪律 4）。
pub const MAX_DEPTH: usize = 8;

/// 单把锁的持锁态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoldState {
    Idle = 0,    // 未持有
    Held = 1,    // 已持有
    Waiting = 2, // 等待中（本模型只登记，不做真等待）
}

/// 断言裁决闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockVerdict {
    Ordered = 0,     // 顺序合法
    Inversion = 1,   // 顺序倒置
    Reentrant = 2,   // 重入
    TooDeep = 3,     // 深度触顶
}

/// 锁等级表：VFS 侧 1/2/3，B01 三层 10/11/12（设计纪律 1 的来源写在此处）。
pub fn rank_of(c: LockClass) -> u8 {
    match c {
        LockClass::Sb => 1,
        LockClass::Inode => 2,
        LockClass::Dentry => 3,
        LockClass::B01L1 => 10,
        LockClass::B01L2 => 11,
        LockClass::B01L3 => 12,
    }
}

/// 锁类码 → 闭集（对外唯一入口，越界拒收）。
pub fn class_by_code(code: u16) -> Result<LockClass, u16> {
    match code {
        0 => Ok(LockClass::Sb),
        1 => Ok(LockClass::Inode),
        2 => Ok(LockClass::Dentry),
        3 => Ok(LockClass::B01L1),
        4 => Ok(LockClass::B01L2),
        5 => Ok(LockClass::B01L3),
        _ => Err(E_CLASS_OOR),
    }
}

/// 单锁登记槽：态 + 获取计数（每类锁的本地账）。
pub struct LockSlot {
    pub class: LockClass,   // 锁类
    pub state: HoldState,   // 持锁态
    pub acquisitions: u32,  // 该类锁累计获取次数
    pub releases: u32,      // 该类锁累计释放次数
}

impl LockSlot {
    fn new(c: LockClass) -> LockSlot {
        LockSlot {
            class: c,
            state: HoldState::Idle,
            acquisitions: 0,
            releases: 0,
        }
    }
}

/// 锁序断言器：held 栈 + 违例账 + 深度峰值。
pub struct OrderAssertor {
    pub slots: [LockSlot; CLS_N], // 六类锁槽
    pub held: Vec<LockClass>,     // 持锁栈（严格升序）
    pub violations: u32,          // 违例总账（三种违例都记）
    pub inversion: u32,           // 倒置违例
    pub reentrant: u32,           // 重入违例
    pub acq_total: u32,           // 获取成功总数
    pub rel_total: u32,           // 释放成功总数
    pub max_depth: u32,           // 深度峰值
}

impl OrderAssertor {
    pub fn new() -> OrderAssertor {
        OrderAssertor {
            slots: [
                LockSlot::new(LockClass::Sb),
                LockSlot::new(LockClass::Inode),
                LockSlot::new(LockClass::Dentry),
                LockSlot::new(LockClass::B01L1),
                LockSlot::new(LockClass::B01L2),
                LockSlot::new(LockClass::B01L3),
            ],
            held: Vec::new(),
            violations: 0,
            inversion: 0,
            reentrant: 0,
            acq_total: 0,
            rel_total: 0,
            max_depth: 0,
        }
    }

    fn idx(c: LockClass) -> usize {
        c as usize
    }

    /// 是否已持有该类锁（held 栈里查，栈深 <= MAX_DEPTH，线性足够）。
    pub fn is_held(&self, c: LockClass) -> bool {
        let mut i = 0usize;
        while i < self.held.len() {
            if self.held[i] == c {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 栈顶锁类（空栈返回 None）。
    pub fn top(&self) -> Option<LockClass> {
        if self.held.is_empty() {
            None
        } else {
            Some(self.held[self.held.len() - 1])
        }
    }

    /// 纯判定：不落账、不改栈，供注入前置探针使用。
    pub fn judge(&self, c: LockClass) -> LockVerdict {
        if self.is_held(c) {
            return LockVerdict::Reentrant;
        }
        if self.held.len() >= MAX_DEPTH {
            return LockVerdict::TooDeep;
        }
        if let Some(t) = self.top() {
            if rank_of(c) <= rank_of(t) {
                return LockVerdict::Inversion;
            }
        }
        LockVerdict::Ordered
    }

    /// 获取：判定通过才入栈；被拒时栈与账都不动（J5）。
    pub fn acquire(&mut self, c: LockClass) -> Result<(), u16> {
        match self.judge(c) {
            LockVerdict::Ordered => {}
            LockVerdict::Inversion => {
                self.violations += 1;
                self.inversion += 1;
                return Err(E_ORDER_INV);
            }
            LockVerdict::Reentrant => {
                self.violations += 1;
                self.reentrant += 1;
                return Err(E_REENTRANT);
            }
            LockVerdict::TooDeep => {
                self.violations += 1;
                return Err(E_DEPTH);
            }
        }
        self.held.push(c);
        let i = OrderAssertor::idx(c);
        self.slots[i].state = HoldState::Held;
        self.slots[i].acquisitions += 1;
        self.acq_total += 1;
        let d = self.held.len() as u32;
        if d > self.max_depth {
            self.max_depth = d;
        }
        Ok(())
    }

    /// 释放：必须放栈顶（LIFO），否则拒（E_NOT_TOP / E_NOT_HELD）。
    pub fn release(&mut self, c: LockClass) -> Result<(), u16> {
        if self.held.is_empty() {
            return Err(E_NOT_HELD);
        }
        let top = self.held[self.held.len() - 1];
        if top != c {
            if self.is_held(c) {
                return Err(E_NOT_TOP); // 持着但不是栈顶：破坏 LIFO，直接拒
            }
            return Err(E_NOT_HELD);
        }
        self.held.pop();
        let i = OrderAssertor::idx(c);
        self.slots[i].state = HoldState::Idle;
        self.slots[i].releases += 1;
        self.rel_total += 1;
        Ok(())
    }

    /// 全部放掉（drill 复位用），按栈顶顺序弹。
    pub fn release_all(&mut self) -> u32 {
        let mut n = 0u32;
        while !self.held.is_empty() {
            let top = self.held[self.held.len() - 1];
            if self.release(top).is_err() {
                break;
            }
            n += 1;
        }
        n
    }

    /// 持放账守恒：acq == rel + held_len（J4）。
    pub fn balanced(&self) -> bool {
        self.acq_total == self.rel_total + self.held.len() as u32
    }

    /// 栈内等级严格升序复核（断言器自证：自己的栈永远合序）。
    pub fn stack_ordered(&self) -> bool {
        let mut i = 1usize;
        while i < self.held.len() {
            if rank_of(self.held[i]) <= rank_of(self.held[i - 1]) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 逐类锁本地账复核：acquisitions - releases 只能取 0 或 1（非递归锁）。
    pub fn per_class_clean(&self) -> bool {
        let mut i = 0usize;
        while i < CLS_N {
            let s = &self.slots[i];
            let diff = s.acquisitions.saturating_sub(s.releases);
            if diff > 1 {
                return false;
            }
            if s.state == HoldState::Held && diff != 1 {
                return false;
            }
            if s.state == HoldState::Idle && diff != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 死锁注入回归器：在断言器上做双向交叉注入，账"注入数与拦截数"。
pub struct DeadLockRegress {
    pub assertor: OrderAssertor, // 被检断言器
    pub injected: u32,           // 注入次数
    pub intercepted: u32,        // 被拦截次数
    pub passed: u32,             // 合法方向放行次数
    pub resets: u32,             // 注入后复位次数
}

impl DeadLockRegress {
    pub fn new() -> DeadLockRegress {
        DeadLockRegress {
            assertor: OrderAssertor::new(),
            injected: 0,
            intercepted: 0,
            passed: 0,
            resets: 0,
        }
    }

    /// 单次交叉注入：先拿 first 再试 second。
    /// 返回 Ok(true)=第二把被拦（期望），Ok(false)=第二把放行。
    /// 无论结果如何都 release_all 自愈，保证下次注入从空栈起（设计纪律 3）。
    pub fn cross_inject(&mut self, first: LockClass, second: LockClass) -> Result<bool, u16> {
        self.injected += 1;
        self.assertor.acquire(first)?;
        let r = self.assertor.acquire(second);
        let stopped = match r {
            Err(E_ORDER_INV) => true,
            Err(E_REENTRANT) => true,
            Err(E_DEPTH) => true,
            Err(e) => return Err(e),
            Ok(()) => false,
        };
        if stopped {
            self.intercepted += 1;
        } else {
            self.passed += 1;
        }
        self.assertor.release_all();
        self.resets += 1;
        if !self.assertor.held.is_empty() {
            return Err(E_BAL); // 自愈失败：栈残留，回归基线被污染
        }
        Ok(stopped)
    }

    /// 注入命中率判定：拦截数必须等于注入数（零漏拦）。
    pub fn all_intercepted(&self) -> bool {
        self.injected > 0 && self.intercepted == self.injected && self.assertor.held.is_empty()
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ORDER_INV => "锁序倒置（先高后低，死锁候选）",
        E_REENTRANT => "同锁重入（非递归锁）",
        E_NOT_TOP => "放锁非栈顶（破坏 LIFO）",
        E_NOT_HELD => "放未持之锁",
        E_DEPTH => "持锁深度触顶（疑似环路）",
        E_CLASS_OOR => "锁类码越界（合法 0..5）",
        E_BAL => "持放账不平或注入后栈残留",
        E_INJECT_MISS => "注入未被拦截（断言器失效）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：合法序 → 双向注入 100/100 → 跨层联动 → 边界 → 交付。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全序合法获取（VFS 三把 → B01 三层），零违例、深度恰为 6（J1）。
    let mut a = OrderAssertor::new();
    a.acquire(LockClass::Sb)?;
    a.acquire(LockClass::Inode)?;
    a.acquire(LockClass::Dentry)?;
    a.acquire(LockClass::B01L1)?;
    a.acquire(LockClass::B01L2)?;
    a.acquire(LockClass::B01L3)?;
    if a.violations != 0 {
        return Err(E_ORDER_INV);
    }
    if a.held.len() != CLS_N || a.max_depth != CLS_N as u32 {
        return Err(E_BAL);
    }
    if !a.stack_ordered() || !a.balanced() || !a.per_class_clean() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：反序 LIFO 放锁，六放六成，栈空且账平。
    let n = a.release_all();
    if n != CLS_N as u32 {
        return Err(E_BAL);
    }
    if !a.held.is_empty() || !a.balanced() || !a.per_class_clean() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：缓存锁(Dentry) 先于 Inode 的倒置注入 100 次，100/100 被拦（J2 主面）。
    let mut reg = DeadLockRegress::new();
    let mut k = 0u32;
    while k < 100 {
        if reg.cross_inject(LockClass::Dentry, LockClass::Inode)? != true {
            return Err(E_INJECT_MISS);
        }
        k += 1;
    }
    if reg.injected != 100 || reg.intercepted != 100 || reg.passed != 0 {
        return Err(E_BAL);
    }
    if !reg.all_intercepted() {
        return Err(E_INJECT_MISS);
    }
    hits += 1;

    // R4：合法方向不得误伤——Inode 先于 Dentry 100 次全部放行（J3 反面对照）。
    let mut reg2 = DeadLockRegress::new();
    let mut k2 = 0u32;
    while k2 < 100 {
        if reg2.cross_inject(LockClass::Inode, LockClass::Dentry)? {
            return Err(E_ORDER_INV); // 合法方向被误拦同样是失守
        }
        k2 += 1;
    }
    if reg2.passed != 100 || reg2.intercepted != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：跨层联动——缓存锁先拿后 B01 三层逐层试，三层全合法、零违例（判据联动面）。
    let mut a2 = OrderAssertor::new();
    a2.acquire(LockClass::Dentry)?;
    a2.acquire(LockClass::B01L1)?;
    a2.acquire(LockClass::B01L2)?;
    a2.acquire(LockClass::B01L3)?;
    if a2.violations != 0 || a2.held.len() != 4 {
        return Err(E_ORDER_INV);
    }
    if !a2.stack_ordered() {
        return Err(E_ORDER_INV);
    }
    hits += 1;

    // R6：B01 三层已被缓存锁压住时反过来插缓存锁，三层各 100 次注入全拦（100×3）。
    let mut reg3 = DeadLockRegress::new();
    let layers = [LockClass::B01L1, LockClass::B01L2, LockClass::B01L3];
    let mut li = 0usize;
    while li < 3 {
        let mut m = 0u32;
        while m < 100 {
            if reg3.cross_inject(layers[li], LockClass::Dentry)? != true {
                return Err(E_INJECT_MISS);
            }
            m += 1;
        }
        li += 1;
    }
    if reg3.injected != 300 || reg3.intercepted != 300 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重入——同锁二次获取被拒，且栈长不变、违例账 +1（J5）。
    let mut a3 = OrderAssertor::new();
    a3.acquire(LockClass::Sb)?;
    let held_before = a3.held.len();
    if a3.acquire(LockClass::Sb) != Err(E_REENTRANT) {
        return Err(E_REENTRANT);
    }
    if a3.held.len() != held_before || a3.reentrant != 1 || a3.violations != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：放非栈顶（持 Sb+Inode 时放 Sb）→ E_NOT_TOP；放未持 → E_NOT_HELD。
    a3.acquire(LockClass::Inode)?;
    if a3.release(LockClass::Sb) != Err(E_NOT_TOP) {
        return Err(E_NOT_TOP);
    }
    if a3.release(LockClass::B01L1) != Err(E_NOT_HELD) {
        return Err(E_NOT_HELD);
    }
    if a3.held.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I3：空栈放锁 → E_NOT_HELD。
    let mut a4 = OrderAssertor::new();
    if a4.release(LockClass::Sb) != Err(E_NOT_HELD) {
        return Err(E_NOT_HELD);
    }
    hits += 1;

    // I4：深度触顶——同一断言器反复拿同一把会被重入先拦，故按"重入优先"验证
    //     深度上限只对不同类型的锁生效：六类拿满后第七次必须是重入或深度之一。
    let mut a5 = OrderAssertor::new();
    a5.acquire(LockClass::Sb)?;
    a5.acquire(LockClass::Inode)?;
    a5.acquire(LockClass::Dentry)?;
    a5.acquire(LockClass::B01L1)?;
    a5.acquire(LockClass::B01L2)?;
    a5.acquire(LockClass::B01L3)?;
    if a5.held.len() != MAX_DEPTH - 2 {
        return Err(E_BAL); // 六类拿满，深度 6，上限 8，尚有余量
    }
    if a5.acquire(LockClass::Sb) != Err(E_REENTRANT) {
        return Err(E_REENTRANT);
    }
    hits += 1;

    // I5：锁类码越界 → E_CLASS_OOR（6 与 65535 双点）。
    if class_by_code(CLS_N as u16) != Err(E_CLASS_OOR) {
        return Err(E_CLASS_OOR);
    }
    if class_by_code(0xFFFF) != Err(E_CLASS_OOR) {
        return Err(E_CLASS_OOR);
    }
    hits += 1;

    // 终审：断言器自证三不变式（合序、账平、逐类干净）。
    if !a5.stack_ordered() || !a5.balanced() || !a5.per_class_clean() {
        return Err(E_BAL);
    }
    if a5.violations != a5.inversion + a5.reentrant + 0 {
        // 违例总账 = 各分类之和（深度违例本路径未触发，故记 0）
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
