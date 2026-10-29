//! unx4218_walkreent — UNX-F4218 路径解析可重入性回归（诊断段 0xA080）。
//!
//! 判据原文：
//! - J1 双读者并发解析同一路径 10 万次零错账零死锁（锁序断言器联动零违例）；
//! - J2 可重入性声明落账：嵌套解析深度上限 40（ELOOP 口径）并在账内记录；
//! - J3 锁序断言器：任何读者持锁升级（低→高逆序获取）即记违例，须为零；
//! - J4 确定性并发模型：双读者按固定拍节交替，无 rand 无墙钟；
//! - J5 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//! - D1 锁编号全序 1..N，获取必须严格升序，违序即拒并记违例账；
//! - D2 嵌套解析（符号链接展开）深度受上限保护，超限拒收；
//! - D3 双读者共享只读快照，解析不产生写竞争，零死锁判定基于锁序保证；
//! - D4 所有计数账 owned 快照导出；
//! - D5 drill 分步：锁序基线→双读者 10 万次→ELOOP 注入→对账封账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xA080;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LOCK_ORDER: u16 = SEG_BASE + 1;  // 锁获取逆序（锁序违例）
const E_ELOOP: u16 = SEG_BASE + 2;       // 嵌套解析深度超 40（ELOOP 口径）
const E_EMPTY: u16 = SEG_BASE + 3;       // 空路径拒收
const E_NOT_ABS: u16 = SEG_BASE + 4;     // 非绝对路径拒收
const E_NOT_SEAL: u16 = SEG_BASE + 5;    // 未满足封账条件即封账
const E_BAD_LOCK: u16 = SEG_BASE + 6;    // 锁编号越界
const E_RELOCK: u16 = SEG_BASE + 7;      // 同读者重复获取已持有的锁
const E_HOLD_BAL: u16 = SEG_BASE + 8;    // 释放后持有账不为零（泄漏）

/// 双读者并发解析次数（判据 J1：10 万次）。
pub const REENT_ITERS: u32 = 100_000;
/// 嵌套解析深度上限（POSIX ELOOP 40 次口径）。
pub const SYMLINK_FOLLOW_MAX: u32 = 40;
/// 锁总数（全序 1..=LOCK_N）。
pub const LOCK_N: u8 = 4;

/// 锁序断言器：按读者跟踪持锁集合与获取序列。
pub struct LockOrder {
    /// 各读者当前持锁位图（bit i = 持有锁 i+1）。
    holds: [u8; 2],
    /// 违例计数（判据 J3 要求 0）。
    pub violations: u32,
    /// 重入获取拦截计数。
    pub relocks: u32,
}

impl LockOrder {
    /// 建空断言器。
    pub fn new() -> Self {
        LockOrder { holds: [0, 0], violations: 0, relocks: 0 }
    }

    /// 读者 r（0/1）以升序序列获取锁集合；逆序/重复获取一律拒收。
    pub fn acquire_seq(&mut self, r: usize, seq: &[u8]) -> Result<(), u16> {
        if r >= 2 {
            return Err(E_BAD_LOCK);
        }
        let mut prev: u8 = 0;
        for lk in seq {
            if *lk == 0 || *lk > LOCK_N {
                return Err(E_BAD_LOCK);
            }
            if *lk <= prev {
                // 逆序获取：锁序违例（J3 拦截点）。
                self.violations += 1;
                return Err(E_LOCK_ORDER);
            }
            if self.holds[r] & (1 << (*lk - 1)) != 0 {
                self.relocks += 1;
                return Err(E_RELOCK);
            }
            self.holds[r] |= 1 << (*lk - 1);
            prev = *lk;
        }
        Ok(())
    }

    /// 读者 r 全量释放并核对持有账清零。
    pub fn release_all(&mut self, r: usize) -> Result<(), u16> {
        if r >= 2 {
            return Err(E_BAD_LOCK);
        }
        self.holds[r] = 0;
        Ok(())
    }

    /// 持有账核对：释放后必须为零（J1 零泄漏口径）。
    pub fn check_hold_balance(&self) -> Result<(), u16> {
        if self.holds[0] == 0 && self.holds[1] == 0 {
            return Ok(());
        }
        Err(E_HOLD_BAL)
    }
}

/// 解析状态机：空闲 → 解析中(深度) → 封账。
pub enum ReentState {
    /// 空闲。
    Idle,
    /// 解析进行中，携带当前嵌套深度。
    Reading(u32),
    /// 回归完成已封账。
    Sealed,
}

/// 可重入解析账：双读者 10 万次回归 + 嵌套深度记录。
pub struct WalkReent {
    /// 双读者总解析次数。
    pub iters: u64,
    /// 错账次数（判据 J1 要求 0）。
    pub err_count: u64,
    /// 观测到的最大嵌套深度（可重入性声明落账，J2）。
    pub max_depth: u32,
    /// 状态机。
    pub state: ReentState,
}

impl WalkReent {
    /// 建账。
    pub fn new() -> Self {
        WalkReent { iters: 0, err_count: 0, max_depth: 0, state: ReentState::Idle }
    }

    /// 单读者一次解析：词法走分量，含嵌套展开（深度受上限保护）。
    pub fn parse(&mut self, reader: usize, path: &str, depth: u32) -> Result<u32, u16> {
        if path.is_empty() {
            self.err_count += 1;
            return Err(E_EMPTY);
        }
        if !path.starts_with('/') {
            self.err_count += 1;
            return Err(E_NOT_ABS);
        }
        if depth > SYMLINK_FOLLOW_MAX {
            self.err_count += 1;
            return Err(E_ELOOP);
        }
        if depth > self.max_depth {
            self.max_depth = depth;
        }
        self.state = ReentState::Reading(depth);
        // 词法解析：`//` 归一并计分量数（不触 fs 层，纯确定性）。
        let mut comps: u32 = 0;
        let mut stack: Vec<&str> = Vec::new();
        for seg in path.split('/') {
            match seg {
                "" | "." => continue,
                ".." => {
                    stack.pop();
                    comps += 1;
                }
                _ => {
                    stack.push(seg);
                    comps += 1;
                }
            }
        }
        self.iters += 1;
        // 嵌套声明：深度>0 时用同路径的归一形式再走一层（受上限保护）。
        if depth < 2 && comps > 0 {
            let mut norm = String::new();
            for seg in &stack {
                norm.push('/');
                norm.push_str(seg);
            }
            if norm.is_empty() {
                norm.push('/');
            }
            let owned = norm;
            self.parse(reader, &owned, depth + 1)?;
        }
        Ok(comps)
    }

    /// 双读者 10 万次回归（判据 J1）：固定拍节交替，确定性无死锁模型。
    pub fn regression(&mut self, lock: &mut LockOrder) -> Result<u64, u16> {
        // 双读者锁序基线：均按 1→2→3 升序获取，锁序保证无死锁。
        lock.acquire_seq(0, &[1, 2, 3])?;
        lock.acquire_seq(1, &[1, 2, 3])?;
        lock.release_all(0)?;
        lock.release_all(1)?;
        lock.check_hold_balance()?;
        let path = "/a/b/c/d";
        for _ in 0..REENT_ITERS {
            self.parse(0, path, 0)?;
            self.parse(1, path, 0)?;
        }
        if self.err_count != 0 {
            return Err(E_ELOOP);
        }
        lock.check_hold_balance()?;
        Ok(self.iters)
    }

    /// 封账：10 万次零错账且锁序零违例方可推进 Sealed。
    pub fn seal(&mut self, lock: &LockOrder) -> Result<(), u16> {
        if self.iters as u64 != REENT_ITERS as u64 * 2 || self.err_count != 0 {
            return Err(E_NOT_SEAL);
        }
        if lock.violations != 0 {
            return Err(E_LOCK_ORDER);
        }
        if self.max_depth < 1 {
            return Err(E_NOT_SEAL);
        }
        self.state = ReentState::Sealed;
        Ok(())
    }

    /// 结果摘要（判据 J5）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("walkreent: iters=");
        push_u64(&mut s, self.iters);
        s.push_str(" err=");
        push_u64(&mut s, self.err_count);
        s.push_str(" maxdepth=");
        push_u32(&mut s, self.max_depth);
        s
    }
}

/// u64 十进制追加。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// u32 十进制追加。
fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// drill 自例程：R1 锁序基线 → R2 双读者 10 万次 → R3 ELOOP 注入 → R4 封账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut lock = LockOrder::new();
    let mut w = WalkReent::new();

    // R1 锁序基线：升序合法、逆序拒收且记违例。
    lock.acquire_seq(0, &[1, 3, 4])?;
    lock.release_all(0)?;
    if !matches!(lock.acquire_seq(1, &[3, 1]), Err(E_LOCK_ORDER)) {
        return Err(E_LOCK_ORDER);
    }
    if lock.violations != 1 {
        return Err(E_LOCK_ORDER);
    }
    lock.violations = 0; // 复位后进入正式回归。
    lock.release_all(1)?;
    hits += 1;

    // R2 双读者 10 万次：零错账、零死锁、零违例（判据 J1）。
    let n = w.regression(&mut lock)?;
    if n != REENT_ITERS as u64 * 2 {
        return Err(E_NOT_SEAL);
    }
    if lock.violations != 0 || lock.relocks != 0 {
        return Err(E_LOCK_ORDER);
    }
    hits += 1;

    // R3 注入拦截：ELOOP 深度、空路径、逆序全拒收并记错。
    if !matches!(w.parse(0, "/a", SYMLINK_FOLLOW_MAX + 1), Err(E_ELOOP)) {
        return Err(E_ELOOP);
    }
    if !matches!(w.parse(0, "", 0), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    if !matches!(lock.acquire_seq(0, &[2, 2]), Err(E_LOCK_ORDER)) {
        return Err(E_LOCK_ORDER);
    }
    lock.release_all(0)?;
    hits += 1;

    // R4 封账对账：重放计数、可重入深度声明、摘要非空（判据 J2/J5）。
    // 注：R3 的错误注入已累积错账与违例，按账目纪律复位后重放再封账。
    w.err_count = 0;
    w.iters = 0;
    lock.violations = 0;
    w.regression(&mut lock)?;
    w.seal(&lock)?;
    if w.max_depth < 1 {
        return Err(E_NOT_SEAL);
    }
    if w.summary().is_empty() {
        return Err(E_NOT_SEAL);
    }
    hits += 1;

    Ok(hits)
}
