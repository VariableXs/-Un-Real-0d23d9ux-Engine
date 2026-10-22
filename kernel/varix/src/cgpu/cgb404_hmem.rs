//! CGPU-F0404 · 与 H 显存域接口集成（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 显存集成：显存账户落地（F0338 四维预算的显存维度——显存预算账户
//!   与 H 域 F1121+ 预算池对接）；分配申请走预算（显存分配前先过预算
//!   校验——超预算拒绝或降质）；泄漏联动（H 域泄漏防线 B05 理念的预算
//!   侧：显存超用异常→预算告警）；回收联动（显存回收→预算即时归还）。
//! - 判据：账户对接、预算校验、泄漏联动、回收归还、判据。
//!
//! 设计纪律：
//! - 账户对接：本域显存账户 ↔ H 域预算池双记账——每次分配两账同步
//!   增加、每次回收两账同步归还，任何一侧漂移即对接红。
//! - 预算校验前置：分配申请先过账户余量校验，超预算走两条路：
//!   硬类（时帧缓冲）拒绝，软类（贴图/附件）降质（对半砍档重试）。
//! - 泄漏联动：账户占用连续超窗口（帧数）不降即超用异常 → 告警事件
//!   （B05 理念的预算侧镜像）；告警不自动回收（H 域职责分离）。
//! - 回收归还：回收必须带分配令牌（同源校验），归还即时入余量；
//!   双账同步归还差额恒零。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C70 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C70，全仓独占）。
pub const SEG_BASE: u16 = 0x0C70;

pub const E_HPOOL_DRIFT: u16 = SEG_BASE + 1; // 双账漂移
pub const E_HBUDGET_DENY: u16 = SEG_BASE + 2; // 硬类超预算拒绝
pub const E_HTOKEN_BAD: u16 = SEG_BASE + 3; // 回收令牌不合法
pub const E_HLEAK_WATCH: u16 = SEG_BASE + 4; // 超用异常（告警事件）
pub const E_HACCT_NOACC: u16 = SEG_BASE + 5; // 账户未对接
pub const E_HDEGRADE_FLOOR: u16 = SEG_BASE + 6; // 降质到底仍超

/// 分配类别：硬类（拒绝路径）/ 软类（降质路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemClass {
    Hard,
    Soft,
}

/// 降质档序列（软类砍档：按字节 1/2 → 1/4 → 1/8，到底仍超即拒）。
pub const DEGRADE_STEPS: [u32; 3] = [2, 4, 8];

/// 一条分配令牌（回收的凭据——分配时签发）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub id: u64,
    pub acct: u32,
    pub bytes: u32,
}

/// 显存账户（C 侧）。
#[derive(Clone, Debug)]
pub struct MemAccount {
    pub acct: u32,
    /// 预算（字节）。
    pub budget: u64,
    /// 已占用（字节）。
    pub used: u64,
    /// 连续超窗口帧计数（泄漏联动时钟）。
    pub over_since: Option<u64>,
}

impl MemAccount {
    pub fn avail(&self) -> u64 {
        self.budget - self.used
    }
}

/// H 域预算池（对端镜像账）。
#[derive(Clone, Debug, Default)]
pub struct HPool {
    /// 对接账户的池内记账（acct → 字节）。
    pub mirror: Vec<(u32, u64)>,
    /// 池总量与已用。
    pub capacity: u64,
    pub used: u64,
}

/// 显存集成控制器。
#[derive(Clone, Debug)]
pub struct HMemIntegrator {
    pub accounts: Vec<MemAccount>,
    pub pool: HPool,
    pub tokens: Vec<Token>,
    /// 告警事件链（泄漏联动产物）。
    pub alerts: Vec<(u32, u64)>,
    /// 超用判定窗口（帧）。
    pub leak_window: u64,
    next_token: u64,
}

impl HMemIntegrator {
    pub fn new(pool_capacity: u64, leak_window: u64) -> Self {
        HMemIntegrator {
            accounts: Vec::new(),
            pool: HPool { mirror: Vec::new(), capacity: pool_capacity, used: 0 },
            tokens: Vec::new(),
            alerts: Vec::new(),
            leak_window,
            next_token: 1,
        }
    }

    /// 账户对接：C 侧账户 + H 侧镜像位同时建（双账同步起点）。
    pub fn attach(&mut self, acct: u32, budget: u64) -> Result<(), u16> {
        if self.accounts.iter().any(|a| a.acct == acct) {
            return Err(E_HACCT_NOACC);
        }
        self.accounts.push(MemAccount {
            acct,
            budget,
            used: 0,
            over_since: None,
        });
        self.pool.mirror.push((acct, 0));
        Ok(())
    }

    /// 池校验 + 双账一致性自检。
    pub fn consistent(&self) -> Result<(), u16> {
        for a in &self.accounts {
            let m = self
                .pool
                .mirror
                .iter()
                .find(|(k, _)| *k == a.acct)
                .map(|(_, v)| *v)
                .unwrap_or(u64::MAX);
            if m != a.used {
                return Err(E_HPOOL_DRIFT);
            }
        }
        let pool_sum: u64 = self.pool.mirror.iter().map(|(_, v)| *v).sum();
        if pool_sum != self.pool.used {
            return Err(E_HPOOL_DRIFT);
        }
        Ok(())
    }

    /// 分配申请（预算前置校验）：硬类超预算拒；软类逐档降质重试。
    /// 返回签发令牌。
    pub fn request(
        &mut self,
        acct: u32,
        bytes: u32,
        class: MemClass,
    ) -> Result<Token, u16> {
        let ai = self
            .accounts
            .iter()
            .position(|a| a.acct == acct)
            .ok_or(E_HACCT_NOACC)?;
        let mut want = bytes as u64;
        if want > self.accounts[ai].avail() {
            if class == MemClass::Hard {
                return Err(E_HBUDGET_DENY);
            }
            // 软类降质：逐档砍到能放进余量。
            let mut placed = false;
            for d in DEGRADE_STEPS {
                want = bytes as u64 / d as u64;
                if want > 0 && want <= self.accounts[ai].avail() {
                    placed = true;
                    break;
                }
            }
            if !placed {
                return Err(E_HDEGRADE_FLOOR);
            }
        }
        // 池容量校验。
        if self.pool.used + want > self.pool.capacity {
            return Err(E_HBUDGET_DENY);
        }
        // 双账同步入账。
        self.accounts[ai].used += want;
        let mi = self
            .pool
            .mirror
            .iter()
            .position(|(k, _)| *k == acct)
            .ok_or(E_HACCT_NOACC)?;
        self.pool.mirror[mi].1 += want;
        self.pool.used += want;
        let t = Token { id: self.next_token, acct, bytes: want as u32 };
        self.next_token += 1;
        self.tokens.push(t);
        self.consistent()?;
        Ok(t)
    }

    /// 回收归还：令牌校验（存在且账户匹配）→ 双账同步归还。
    pub fn release(&mut self, token: Token) -> Result<u64, u16> {
        let ti = self
            .tokens
            .iter()
            .position(|t| *t == token)
            .ok_or(E_HTOKEN_BAD)?;
        let ai = self
            .accounts
            .iter()
            .position(|a| a.acct == token.acct)
            .ok_or(E_HACCT_NOACC)?;
        self.accounts[ai].used = self.accounts[ai].used.saturating_sub(token.bytes as u64);
        let mi = self
            .pool
            .mirror
            .iter()
            .position(|(k, _)| *k == token.acct)
            .ok_or(E_HACCT_NOACC)?;
        self.pool.mirror[mi].1 = self.pool.mirror[mi].1.saturating_sub(token.bytes as u64);
        self.pool.used = self.pool.used.saturating_sub(token.bytes as u64);
        self.tokens.remove(ti);
        self.consistent()?;
        Ok(self.accounts[ai].avail())
    }

    /// 泄漏联动：逐帧巡检——账户占用 ≥ 预算（余量为 0）连续 leak_window
    /// 帧即告警一次（不重复刷）；占用回落则时钟重置。
    pub fn tick(&mut self, frame: u64) -> Result<(), u16> {
        for a in self.accounts.iter_mut() {
            if a.avail() == 0 {
                match a.over_since {
                    None => a.over_since = Some(frame),
                    Some(s) => {
                        if frame - s >= self.leak_window
                            && !self.alerts.iter().any(|(k, _)| *k == a.acct)
                        {
                            self.alerts.push((a.acct, frame));
                        }
                    }
                }
            } else {
                a.over_since = None;
            }
        }
        // 告警在册但账户已回落 → 告警保留（审计链不删）；但账户时钟重置
        // 已完成。此处校验双账一致性兜底。
        self.consistent()
    }

    /// 池内账户的 H 侧镜像值（对接查询口）。
    pub fn pool_mirror_of(&self, acct: u32) -> u64 {
        self.pool
            .mirror
            .iter()
            .find(|(k, _)| *k == acct)
            .map(|(_, v)| *v)
            .unwrap_or(0)
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn it() -> HMemIntegrator {
        let mut i = HMemIntegrator::new(1 << 20, 8);
        i.attach(1, 100_000).ok();
        i.attach(2, 50_000).ok();
        i
    }

    #[test]
    fn a01_账户对接_双账同建同涨() {
        let i = it();
        assert_eq!(i.consistent(), Ok(()));
        assert_eq!(i.pool.mirror.len(), 2);
        // 重复对接拒。
        let mut i2 = it();
        assert_eq!(i2.attach(1, 10), Err(E_HACCT_NOACC));
    }

    #[test]
    fn a02_预算校验_硬类拒绝软类降质() {
        let mut i = it();
        // 硬类 90k 放得下。
        let t = i.request(1, 90_000, MemClass::Hard).ok();
        assert_eq!(t.map(|t| t.bytes), Some(90_000));
        // 再申请硬类 20k 超余量（10k）⇒ 拒。
        assert_eq!(i.request(1, 20_000, MemClass::Hard), Err(E_HBUDGET_DENY));
        // 软类 20k：砍 1/2=10k 放得下 ⇒ 降质成交。
        let t2 = i.request(1, 20_000, MemClass::Soft).ok();
        assert_eq!(t2.map(|t| t.bytes), Some(10_000));
        // 池总容量校验：账户 2 预算 50k 但池只余 ~946k——构造池顶死。
        let mut i3 = HMemIntegrator::new(60_000, 8);
        i3.attach(1, 100_000).ok();
        assert_eq!(i3.request(1, 70_000, MemClass::Hard), Err(E_HBUDGET_DENY));
        // 降质到底仍超 ⇒ 拒。
        let mut i4 = it();
        i4.request(1, 99_999, MemClass::Hard).ok();
        assert_eq!(i4.request(1, 40_000, MemClass::Soft), Err(E_HDEGRADE_FLOOR));
    }

    #[test]
    fn a03_泄漏联动_连续超窗告警一次() {
        let mut i = it();
        i.request(1, 100_000, MemClass::Hard).ok(); // 打满账户 1
        // 前 8 帧无告警（窗口 8 未满）。
        for f in 0..8 {
            i.tick(f).ok();
        }
        assert_eq!(i.alerts.len(), 0);
        // 第 8 帧连续超窗告警。
        i.tick(8).ok();
        assert_eq!(i.alerts.len(), 1);
        assert_eq!(i.alerts[0], (1, 8));
        // 不重复刷。
        i.tick(20).ok();
        assert_eq!(i.alerts.len(), 1);
        // 回落重置时钟：归还后不再累计。
        let mut i2 = it();
        let t = i2.request(2, 50_000, MemClass::Hard).ok();
        for f in 0..10 {
            i2.tick(f).ok();
        }
        assert_eq!(i2.alerts.len(), 1);
        i2.release(t.unwrap_or(Token { id: 0, acct: 2, bytes: 0 })).ok();
        i2.tick(11).ok();
        // 已有告警保留（审计链不删），但不再新增。
        assert_eq!(i2.alerts.len(), 1);
    }

    #[test]
    fn a04_回收归还_令牌校验与双账同步() {
        let mut i = it();
        let t1 = i.request(1, 30_000, MemClass::Hard).ok();
        let t1 = t1.unwrap_or(Token { id: 0, acct: 1, bytes: 0 });
        // 伪造令牌拒。
        assert_eq!(
            i.release(Token { id: 999, acct: 1, bytes: 30_000 }),
            Err(E_HTOKEN_BAD)
        );
        // 账户不匹配拒。
        assert_eq!(
            i.release(Token { id: t1.id, acct: 2, bytes: 30_000 }),
            Err(E_HTOKEN_BAD)
        );
        // 正常归还：双账同步、余量恢复。
        let avail = i.release(t1).ok();
        assert_eq!(avail, Some(100_000));
        assert_eq!(i.consistent(), Ok(()));
        assert_eq!(i.pool.used, 0);
        // 重复归还（令牌已消费）拒。
        assert_eq!(i.release(t1), Err(E_HTOKEN_BAD));
    }

    #[test]
    fn a05_对接查询_镜像视图与池守恒() {
        let mut i = it();
        let ta = i.request(1, 40_000, MemClass::Hard).ok();
        let tb = i.request(2, 10_000, MemClass::Soft).ok();
        assert_eq!(i.pool_mirror_of(1), 40_000);
        assert_eq!(i.pool_mirror_of(2), 10_000);
        assert_eq!(i.pool.used, 50_000);
        // 释放一半后守恒。
        i.release(ta.unwrap_or(Token { id: 0, acct: 1, bytes: 0 })).ok();
        assert_eq!(i.pool.used, 10_000);
        assert_eq!(i.pool_mirror_of(1), 0);
        assert_eq!(i.consistent(), Ok(()));
    }
}
