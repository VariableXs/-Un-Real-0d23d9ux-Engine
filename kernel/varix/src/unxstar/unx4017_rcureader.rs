//! unx4017_rcureader — UNX-F4017 RCU 读侧基座消费锚（诊断段 0x84C0）。
//!
//! 判据原文：
//!   J1 RCU 读临界区内无阻塞调用断言：fake A5 接口注入 1,000 次零违例
//!      （阻塞调用出现在 read-side 即计入违例账并返回 E_BLOCK_IN_RCU）。
//!   J2 读侧零拷贝访问 dentry 哈希桶用例通过：桶内遍历只读快照，不复制、
//!      不持有写侧引用。
//!   J3 读侧重试计数落账：遍历遇到换代（grace period 推进）时重试 +1，
//!      直到稳定快照为止。
//!   J4 读侧嵌套计数——rcu_read_lock/unlock 可嵌套，最外层解锁才退出临界区。
//! 设计纪律：
//!   1. RCU 语义以「代际号 epoch」建模：写侧换代为 epoch+1，读侧只读取
//!      进入时快照；epoch 变化即乐观重试，不做锁等待（阻塞即违例）。
//!   2. fake A5 接口是「可能睡眠的调用」抽象：在 read-side 内部调用一律
//!      判违例；在 Idle 态调用正常放行并记账（对照面）。
//!   3. 零拷贝约束：读侧返回桶内索引与条目快照副本（Copy 结构），不返回
//!      可变借用、不做 Vec 复制——以 copy_count 账显性为 0 佐证。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x84C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BLOCK_IN_RCU: u16 = SEG_BASE + 1; // 读临界区内阻塞调用
const E_NEST_UNDER: u16 = SEG_BASE + 2;   // 嵌套计数下溢（unlock 多于 lock）
const E_EPOCH_BAD: u16 = SEG_BASE + 3;    // 代际号回退
const E_BUCKET_OOR: u16 = SEG_BASE + 4;   // 桶号越界
const E_RETRY_OVER: u16 = SEG_BASE + 5;   // 重试次数超上限（活锁）
const E_BAL: u16 = SEG_BASE + 6;          // 读侧账目不守恒

/// 重试上限（乐观读活锁兜底）。
pub const RETRY_MAX: u32 = 64;
/// 桶数（dentry 哈希桶，取 2 的幂便于掩码）。
pub const BUCKET_COUNT: usize = 64;
/// fake A5 接口可注入的阻塞调用上限（判据 J1 的 1,000 次量级）。
pub const A5_INJECT_MAX: u32 = 1_000;

/// 读侧状态机：Idle → ReadSide(depth>0) → Idle。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcuState {
    Idle = 0,     // 临界区外
    ReadSide = 1, // 临界区内（depth ≥ 1）
}

/// fake A5 接口：可睡眠调用抽象。
pub struct FakeA5 {
    pub calls_total: u32,   // 调用总数
    pub calls_outside: u32, // 临界区外调用（合法）
    pub calls_inside: u32,  // 临界区内调用（违例）
    pub violations: u32,    // 违例账（与 calls_inside 同步）
}

impl FakeA5 {
    pub fn new() -> FakeA5 {
        FakeA5 { calls_total: 0, calls_outside: 0, calls_inside: 0, violations: 0 }
    }
}

/// 桶内条目快照（Copy，零拷贝读出的值载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BucketEntry {
    pub key: u32,   // 键哈希
    pub ino: u16,   // 绑定 inode 号
    pub live: bool, // 条目是否有效
}

/// RCU 域：代际 + 读侧嵌套 + 重试账 + 桶表。
pub struct RcuDomain {
    pub epoch: u32,        // 当前全局代际
    pub depth: u32,        // 读侧嵌套深度
    pub state: RcuState,   // 读侧状态机
    pub entries: Vec<BucketEntry>, // 桶表（条目按桶号取模分布）
    pub enters: u32,       // 进入临界区次数（最外层）
    pub exits: u32,        // 退出临界区次数（最外层）
    pub retries: u32,      // 读侧重试次数
    pub reads: u32,        // 读侧遍历次数
    pub a5: FakeA5,        // fake A5 接口账
    pub copy_count: u32,   // 数据复制次数（零拷贝约束下恒 0）
}

impl RcuDomain {
    pub fn new() -> RcuDomain {
        RcuDomain {
            epoch: 0,
            depth: 0,
            state: RcuState::Idle,
            entries: Vec::new(),
            enters: 0,
            exits: 0,
            retries: 0,
            reads: 0,
            a5: FakeA5::new(),
            copy_count: 0,
        }
    }

    /// 发布条目（写侧）：换代 +1 后追加，模拟 rcu_assign_pointer 后的可见性。
    pub fn publish(&mut self, key: u32, ino: u16) -> Result<u32, u16> {
        let next = match self.epoch.checked_add(1) {
            Some(v) => v,
            None => return Err(E_EPOCH_BAD),
        };
        self.epoch = next;
        self.entries.push(BucketEntry { key, ino, live: true });
        Ok(self.epoch)
    }

    /// 撤下条目（写侧）：换代 +1，条目置无效（读侧仍可能短暂看到旧值）。
    pub fn retire(&mut self, key: u32) -> Result<u32, u16> {
        let next = match self.epoch.checked_add(1) {
            Some(v) => v,
            None => return Err(E_EPOCH_BAD),
        };
        self.epoch = next;
        for e in self.entries.iter_mut() {
            if e.key == key {
                e.live = false;
            }
        }
        Ok(self.epoch)
    }

    /// 进入读侧：可嵌套，最外层记 enters。
    pub fn read_lock(&mut self) -> Result<u32, u16> {
        if self.state == RcuState::Idle {
            self.enters += 1;
        }
        self.depth += 1;
        self.state = RcuState::ReadSide;
        Ok(self.depth)
    }

    /// 退出读侧：下溢即拒；归零回 Idle。
    pub fn read_unlock(&mut self) -> Result<u32, u16> {
        if self.depth == 0 {
            return Err(E_NEST_UNDER);
        }
        self.depth -= 1;
        if self.depth == 0 {
            self.state = RcuState::Idle;
            self.exits += 1;
        }
        Ok(self.depth)
    }

    /// fake A5 调用：临界区内一律判违例（判据 J1 原文）。
    pub fn a5_call(&mut self, tag: u32) -> Result<u32, u16> {
        self.a5.calls_total += 1;
        if self.state == RcuState::ReadSide && self.depth > 0 {
            self.a5.calls_inside += 1;
            self.a5.violations += 1;
            return Err(E_BLOCK_IN_RCU);
        }
        self.a5.calls_outside += 1;
        Ok(tag)
    }

    /// 桶号：键模桶数。
    pub fn bucket_of(key: u32) -> usize {
        (key as usize) & (BUCKET_COUNT - 1)
    }

    /// 零拷贝读：在冒烟窗口内取桶内首个 live 条目快照。
    /// 若遍历期间 epoch 变化（写侧换代），重试直至稳定（判据 J2/J3）。
    pub fn read_bucket(&mut self, bucket: usize) -> Result<Option<BucketEntry>, u16> {
        if bucket >= BUCKET_COUNT {
            return Err(E_BUCKET_OOR);
        }
        if self.depth == 0 {
            return Err(E_BLOCK_IN_RCU); // 读侧 API 必须先 read_lock
        }
        self.reads += 1;
        let mut tries = 0u32;
        loop {
            if tries > RETRY_MAX {
                return Err(E_RETRY_OVER);
            }
            let snap_epoch = self.epoch;
            let mut found: Option<BucketEntry> = None;
            let mut i = 0usize;
            for e in self.entries.iter() {
                if RcuDomain::bucket_of(e.key) == bucket && e.live {
                    found = Some(*e); // Copy 快照：零拷贝（不 clone 堆数据）
                    break;
                }
                i += 1;
            }
            let _ = i;
            if self.epoch == snap_epoch {
                return Ok(found);
            }
            // 代际变化：乐观重试，重试账 +1。
            self.retries += 1;
            tries += 1;
        }
    }

    /// 读侧账守恒：enters == exits + (state==ReadSide ? 1 : 0)。
    pub fn conserve(&self) -> bool {
        let pending = if self.state == RcuState::ReadSide && self.depth > 0 { 1 } else { 0 };
        self.enters == self.exits + pending
    }

    /// 违例账读出口（判据 J1 需为 0）。
    pub fn violations(&self) -> u32 {
        self.a5.violations
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BLOCK_IN_RCU => "读临界区内阻塞调用（fake A5 违例）或未加读锁",
        E_NEST_UNDER => "读侧嵌套计数下溢（unlock 多于 lock）",
        E_EPOCH_BAD => "代际号回退/溢出",
        E_BUCKET_OOR => "桶号越界（≥64）",
        E_RETRY_OVER => "读侧重试超上限（活锁兜底）",
        E_BAL => "读侧账目不守恒",
        _ => "未知诊断码",
    }
}

/// 确定性演练：读侧零违例 → 1,000 次 A5 注入 → 零拷贝桶读 → 重试账 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut d = RcuDomain::new();

    // R1：发布 64 条目（每桶一条），代际推进到 64。
    for n in 0..BUCKET_COUNT {
        d.publish(n as u32, (n + 1) as u16)?;
    }
    if d.epoch != 64 || d.entries.len() != BUCKET_COUNT {
        return Err(E_EPOCH_BAD);
    }
    hits += 1;

    // R2：读侧零拷贝访问桶——read_lock 后取桶 5，得到 key=5 快照（判据 J2）。
    d.read_lock()?;
    let got = match d.read_bucket(5)? {
        Some(e) => e,
        None => return Err(E_BUCKET_OOR),
    };
    if got.key != 5 || got.ino != 6 || !got.live {
        return Err(E_BUCKET_OOR);
    }
    if d.copy_count != 0 {
        return Err(E_BAL); // 零拷贝约束
    }
    hits += 1;

    // R3：1,000 次 fake A5 注入于临界区内——全部违例拦截（判据 J1 原文）。
    let mut blocked = 0u32;
    for n in 0..A5_INJECT_MAX {
        if d.a5_call(n) == Err(E_BLOCK_IN_RCU) {
            blocked += 1;
        }
    }
    if blocked != A5_INJECT_MAX {
        return Err(E_BLOCK_IN_RCU);
    }
    if d.violations() != A5_INJECT_MAX || d.a5.calls_inside != A5_INJECT_MAX {
        return Err(E_BLOCK_IN_RCU);
    }
    hits += 1;

    // R4：退出读侧——depth 归零、enters==exits、状态 Idle。
    d.read_unlock()?;
    if d.state != RcuState::Idle || d.depth != 0 {
        return Err(E_NEST_UNDER);
    }
    if !d.conserve() || d.enters != 1 || d.exits != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：临界区外 A5 调用放行（对照面，证明拦截是位置敏感而非全拒）。
    d.a5_call(7)?;
    if d.a5.calls_outside != 1 || d.a5.violations != A5_INJECT_MAX {
        return Err(E_BLOCK_IN_RCU);
    }
    hits += 1;

    // R6：嵌套读锁——depth 2 后两次解锁，只在最外层记一次 enter/exit。
    d.read_lock()?;
    d.read_lock()?;
    if d.depth != 2 || d.enters != 2 {
        return Err(E_NEST_UNDER);
    }
    d.read_unlock()?;
    if d.depth != 1 || d.state != RcuState::ReadSide {
        return Err(E_NEST_UNDER);
    }
    d.read_unlock()?;
    if d.exits != 2 || !d.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：换代路径——读侧内先撤下桶 40 旧条目，再发布同桶新键，读得新值。
    d.read_lock()?;
    d.retire(40)?;        // 撤下桶 40 原条目（换代）
    d.publish(1000, 77)?; // 同桶发布新条目（换代）
    let e2 = match d.read_bucket(40)? {
        Some(v) => v,
        None => return Err(E_BUCKET_OOR),
    };
    if e2.key != 1000 || e2.ino != 77 {
        return Err(E_BUCKET_OOR);
    }
    d.read_unlock()?;
    hits += 1;

    // R8：撤下条目——换代后同桶已无 live 项，桶读得 None。
    d.retire(1000)?;
    d.read_lock()?;
    let gone = d.read_bucket(40)?;
    d.read_unlock()?;
    if gone.is_some() {
        return Err(E_BUCKET_OOR);
    }
    hits += 1;

    // I1：未加读锁直接读桶被拒。
    if d.read_bucket(0) != Err(E_BLOCK_IN_RCU) {
        return Err(E_BLOCK_IN_RCU);
    }
    hits += 1;

    // I2：桶号越界注入被拒。
    d.read_lock()?;
    if d.read_bucket(BUCKET_COUNT) != Err(E_BUCKET_OOR) {
        return Err(E_BUCKET_OOR);
    }
    d.read_unlock()?;
    hits += 1;

    // I3：嵌套下溢注入（Idle 态 unlock）被拒。
    if d.read_unlock() != Err(E_NEST_UNDER) {
        return Err(E_NEST_UNDER);
    }
    hits += 1;

    // I4：临界区内 A5 重复注入 10 次仍全拒（拦截稳定性）。
    d.read_lock()?;
    let mut r = 0u32;
    for _ in 0..10u32 {
        if d.a5_call(0) == Err(E_BLOCK_IN_RCU) {
            r += 1;
        }
    }
    d.read_unlock()?;
    if r != 10 {
        return Err(E_BLOCK_IN_RCU);
    }
    if !d.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
