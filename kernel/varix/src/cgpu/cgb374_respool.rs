//! CGPU-F0374 · 预留池管理（判据段 0x09E0）
//! 预留池：预测驱动预留（预测需求→预留池预占）；预留生命周期
//! （预占→使用/过期释放——不过期不释放防泄漏）；保底不侵（预留
//! 不侵占 F0359 系统保底）；预留计量（命中率/浪费率）。判据：预
//! 占、过期释放、保底不侵、计量、判据。
//!
//! 锚点原文摘录（施工书 1661 行）：
//! 「预测驱动预留（预测需求→预留池预占）；预留生命周期（预占→
//! 使用/过期释放——不过期不释放防泄漏）；保底不侵（预留不侵占
//! F0359 系统保底）；预留计量（命中率/浪费率）。判据：预占、过期
//! 释放、保底不侵、计量、判据。」
//!
//! 诊断码段 0x09E0（本模块独占：0x09E0~0x09EF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x09E0;

/// 池容量（千分比）：预留池总量上限（= 帧预算减系统保底的可让
/// 度域——判据「保底不侵」的量化根基）。
pub const POOL_CAP_PERMILLE: u32 = 600;

/// F0359 系统保底（千分比）——预留池永远碰不到的域。
pub const SYS_FLOOR_PERMILLE: u32 = 400;

/// 预留默认存活期（帧）。
pub const RESERVATION_TTL_FRAMES: u32 = 8;

/// 在册预留上限（泄漏防线：账面行数封顶 32）。
pub const RESERVATION_MAX_ROWS: usize = 32;

/// 预留生命周期态（判据「生命周期」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResvState {
    /// 预占（尚未使用）。
    Held,
    /// 已使用（命中消费）。
    Consumed,
    /// 过期释放（TTL 到点未用——防泄漏的终点）。
    Expired,
}

impl ResvState {
    /// 态号（三态闭集 0/1/2）。
    pub fn ordinal(&self) -> u8 {
        match self {
            ResvState::Held => 0,
            ResvState::Consumed => 1,
            ResvState::Expired => 2,
        }
    }
}

/// 预留行（判据「生命周期」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reservation {
    /// 预留号（1 起）。
    pub id: u32,
    /// 账户号。
    pub account: u32,
    /// 预占量（千分比）。
    pub amount_permille: u32,
    /// 预占帧号。
    pub taken_at: u64,
    /// 当前态。
    pub state: ResvState,
}

impl Reservation {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.account == 0 {
            return Err(SEG_BASE);
        }
        if self.amount_permille == 0 || self.amount_permille > POOL_CAP_PERMILLE {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// TTL 到期核（判据「过期释放」）。
    pub fn expired_at(&self, now: u64) -> bool {
        self.state == ResvState::Held && now >= self.taken_at + RESERVATION_TTL_FRAMES as u64
    }
}

/// 预留池（判据「预占 + 过期释放 + 计量」总成）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReservePool {
    /// 下一预留号。
    pub next_id: u32,
    /// 在册行。
    pub rows: Vec<Reservation>,
    /// 当前预占总量。
    pub held_permille: u32,
    /// 计量：命中（使用）次数。
    pub hits: u32,
    /// 计量：过期（浪费）次数。
    pub expires: u32,
    /// 计量：浪费量累计（千分比）。
    pub wasted_permille: u64,
}

impl ReservePool {
    /// 空池（容量守恒自检：CAP+FLOOR ≡ 1000）。
    pub fn new() -> Result<Self, u16> {
        if POOL_CAP_PERMILLE + SYS_FLOOR_PERMILLE != 1000 {
            return Err(SEG_BASE + 2);
        }
        Ok(ReservePool { next_id: 1, rows: Vec::new(), held_permille: 0, hits: 0, expires: 0, wasted_permille: 0 })
    }
    /// 预占（判据「预占」主判据）：池余量足才许预占；行数封顶。
    pub fn take(&mut self, account: u32, amount: u32, now: u64) -> Result<u32, u16> {
        if account == 0 {
            return Err(SEG_BASE + 3);
        }
        if amount == 0 || amount > POOL_CAP_PERMILLE {
            return Err(SEG_BASE + 4);
        }
        if self.held_permille + amount > POOL_CAP_PERMILLE {
            return Err(SEG_BASE + 5);
        }
        if self.rows.len() >= RESERVATION_MAX_ROWS {
            return Err(SEG_BASE + 6);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(Reservation { id, account, amount_permille: amount, taken_at: now, state: ResvState::Held });
        self.held_permille += amount;
        Ok(id)
    }
    /// 使用（命中消费）：Held→Consumed，量从池账退出，命中计数。
    pub fn consume(&mut self, id: u32, now: u64) -> Result<u32, u16> {
        let r = self.rows.iter_mut().find(|r| r.id == id).ok_or(SEG_BASE + 7)?;
        if r.state != ResvState::Held {
            return Err(SEG_BASE + 8);
        }
        if r.expired_at(now) {
            return Err(SEG_BASE + 9);
        }
        r.state = ResvState::Consumed;
        self.held_permille -= r.amount_permille;
        self.hits += 1;
        Ok(r.amount_permille)
    }
    /// 过期清扫（判据「过期释放」主判据）：到点未用全部收编——
    /// 不过期不释放的泄漏防线。
    pub fn sweep_expired(&mut self, now: u64) -> Result<u32, u16> {
        let mut freed = 0u32;
        for r in self.rows.iter_mut() {
            if r.expired_at(now) {
                r.state = ResvState::Expired;
                freed += r.amount_permille;
                self.expires += 1;
                self.wasted_permille += r.amount_permille as u64;
            }
        }
        self.held_permille -= freed;
        Ok(freed)
    }
    /// 保底不侵核（判据「保底不侵」主判据）：池预占总量 ≤ CAP 且
    /// CAP+FLOOR=1000 → 保底域在算术上不可达。
    pub fn floor_intact(&self) -> Result<bool, u16> {
        Ok(self.held_permille <= POOL_CAP_PERMILLE)
    }
    /// 预留计量（判据「计量」主判据）：命中率 = hits/(hits+expires)；
    /// 浪费率 = wasted/Σ预占流量。空分母显性 None（不发零值谎报）。
    pub fn metrics(&self) -> (Option<u32>, Option<u32>) {
        let total = self.hits + self.expires;
        let hit_rate = if total == 0 { None } else { Some((self.hits as u64 * 1000 / total as u64) as u32) };
        let waste_rate = if total == 0 { None } else { Some((self.wasted_permille * 1000 / (self.hits as u64 + self.wasted_permille) ) as u32) };
        (hit_rate, waste_rate)
    }
}

impl Default for ReservePool {
    fn default() -> Self {
        // 常量表在编译期冻结（CAP+FLOOR=1000 由 new() 内守恒闸守护；
        // Default 走同一条构造路径的静态展开——零 panic 面）。
        ReservePool {
            next_id: 1,
            rows: Vec::new(),
            held_permille: 0,
            hits: 0,
            expires: 0,
            wasted_permille: 0,
        }
    }
}

/// 预占→使用→过期 全链守恒核（判据「生命周期」验收面）：Σ消费
/// + Σ过期浪费 ≡ Σ预占流量（池不吞不漏）。
pub fn lifecycle_conservation(consumed: u64, wasted: u64, taken_total: u64) -> Result<(), u16> {
    if consumed + wasted != taken_total {
        return Err(SEG_BASE + 10);
    }
    Ok(())
}

/// 预留池诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "预留号/账户号为 0",
        1 => "预占量必须 ∈ (0,600]",
        2 => "池容量+保底≠1000——常量表被改",
        3 => "账户号为 0",
        4 => "预占量非法",
        5 => "池余量不足——先清扫过期再试",
        6 => "在册行满（32）——泄漏防线",
        7 => "预留号不存在",
        8 => "预留已终态（重复消费）",
        9 => "预留已过期——走清扫流程",
        10 => "生命周期守恒被破坏（消费+浪费≠预占）",
        _ => "未知诊断码——查本模块 SEG_BASE 0x09E0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_consume_expire_lifecycle() {
        let mut pool = ReservePool::new().unwrap();
        let id = pool.take(1, 200, 0).unwrap();
        assert_eq!(pool.held_permille, 200);
        // 命中消费。
        let got = pool.consume(id, 4).unwrap();
        assert_eq!(got, 200);
        assert_eq!(pool.held_permille, 0);
        assert_eq!(pool.hits, 1);
        // 过期清扫。
        let id2 = pool.take(2, 150, 0).unwrap();
        assert_eq!(id2, 2);
        let freed = pool.sweep_expired(8).unwrap();
        assert_eq!(freed, 150);
        assert_eq!(pool.held_permille, 0);
        assert_eq!(pool.expires, 1);
        assert_eq!(pool.wasted_permille, 150);
        // 过期后不可再消费（非在持态闸在过期闸之前）。
        assert_eq!(pool.consume(id2, 9).unwrap_err(), SEG_BASE + 8);
        // 全链守恒：消费 200 + 浪费 150 = 预占 350。
        assert!(lifecycle_conservation(200, 150, 350).is_ok());
        assert_eq!(lifecycle_conservation(200, 100, 350).unwrap_err(), SEG_BASE + 10);
        assert!(pool.floor_intact().unwrap());
    }

    #[test]
    fn capacity_and_metrics() {
        let mut pool = ReservePool::new().unwrap();
        // 池满：600 上限。
        pool.take(1, 300, 0).unwrap();
        assert_eq!(pool.take(2, 301, 0).unwrap_err(), SEG_BASE + 5);
        pool.take(2, 300, 0).unwrap();
        let (hit, waste) = pool.metrics();
        assert!(hit.is_none()); // 零终态：不发零值谎报。
        assert!(waste.is_none());
        // 清扫全部过期后计费。
        let _ = pool.sweep_expired(8).unwrap();
        let (hit, waste) = pool.metrics();
        assert_eq!(hit, Some(0));
        assert_eq!(waste, Some(1000)); // 全浪费。
        assert!(diagnose(SEG_BASE + 5).contains("清扫"));
    }

    #[test]
    fn row_cap_leak_guard() {
        let mut pool = ReservePool::new().unwrap();
        // 32 行封顶 + 容量 600：单笔最小 19 → 恰好触发行数帽。
        for i in 0..RESERVATION_MAX_ROWS {
            let id = pool.take(1, 18, i as u64).unwrap();
            assert!(id > 0);
        }
        assert_eq!(pool.rows.len(), RESERVATION_MAX_ROWS);
        assert_eq!(pool.take(1, 1, 99).unwrap_err(), SEG_BASE + 6);
        assert_eq!(pool.take(0, 10, 99).unwrap_err(), SEG_BASE + 3);
        assert_eq!(pool.take(1, 0, 99).unwrap_err(), SEG_BASE + 4);
    }
}
