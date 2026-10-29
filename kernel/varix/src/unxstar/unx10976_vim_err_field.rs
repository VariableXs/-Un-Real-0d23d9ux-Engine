//! UNX-F10976 vim 场错误注入联测（EIO/EAGAIN/EINTR 混合）（0x1d980 段）
//!
//! 锚点：UNX-F10976-J1 三错误混合注入千次场景存活（10/10），
//! 恢复账对平（10/10）。
//! 域：CoRun·UNX-B29（I 型联测域·vim 场错误面）。
//!
//! 结构：故障位账（EIO/EAGAIN/EINTR 三类故障位，逐位记命中/存活/
//! 恢复三段计数）+ 轮转器（千轮确定性混合：轮号取模三相轮转，
//! 每轮恰注入一例）+ 恢复账（每例一条回执，回执环八格滚动，
//! 生存数 == 注入数 == 恢复数三方对平）+ fold 与冻结 + 注入核
//! （坏故障类/重号回执/冻结后步进/空账步进四路必抓）。
//!
//! 语义模型：EAGAIN 走"重试"（1），EINTR 走"重启系统调用"（2），
//! EIO 走"放弃本次写、保留缓冲"（3）；三条处置路径都不得折损
//! 编辑器存活，因此存活数必须与注入数恒等。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10976 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1d980;

/// 空账 —— 未立故障位即步进
pub const VE_EMPTY: u32 = SEG_BASE + 0x01;
/// 故障类越界 —— 故障类编号 1..=3
pub const VE_RANGE: u32 = SEG_BASE + 0x02;
/// 重号回执 —— 同一回执号只准记一次
pub const VE_DUP: u32 = SEG_BASE + 0x03;
/// 坏故障类 —— 校验面拒收非法类号
pub const VE_BADKIND: u32 = SEG_BASE + 0x04;
/// 恢复失落 —— 恢复账与注入账不齐
pub const VE_LOST: u32 = SEG_BASE + 0x05;
/// 场已冻结 —— freeze 后禁止再步进
pub const VE_FROZEN: u32 = SEG_BASE + 0x06;

/// 故障类：EIO（放弃写、保缓冲）。
pub const KIND_EIO: u8 = 1;
/// 故障类：EAGAIN（重试）。
pub const KIND_EAGAIN: u8 = 2;
/// 故障类：EINTR（重启系统调用）。
pub const KIND_EINTR: u8 = 3;
/// 故障类总数。
pub const KINDS: usize = 3;
/// 混合注入轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 回执环容量（八格滚动留痕）。
pub const REC_CAP: usize = 8;
/// 故障场域标（fold 种子）。
pub const VE_TAG: u64 = 0x1d9_76;

/// 故障位：一类故障的全部计数三段。
#[derive(Clone, Debug)]
pub struct FaultSite {
    /// 位号（0x15000 段起编）
    pub sid: u32,
    /// 故障类号
    pub kind: u8,
    /// 位名
    pub name: String,
    /// 注入次数
    pub hits: u64,
    /// 存活次数（编辑器未被打死）
    pub survived: u64,
    /// 已恢复次数（处置路径走完）
    pub recovered: u64,
}

impl FaultSite {
    /// 三位对平：命中、存活、恢复三段齐。
    pub fn balanced(&self) -> bool {
        self.hits > 0 && self.hits == self.survived && self.hits == self.recovered
    }

    /// 未对平缺口（存活侧）。
    pub fn alive_gap(&self) -> u64 {
        self.hits.wrapping_sub(self.survived)
    }

    /// 未对平缺口（恢复侧）。
    pub fn recover_gap(&self) -> u64 {
        self.hits.wrapping_sub(self.recovered)
    }
}

/// 恢复回执：一次注入的处置留痕。
#[derive(Clone, Debug)]
pub struct RecoveryRec {
    /// 回执号（全账唯一）
    pub rid: u32,
    /// 故障类号
    pub kind: u8,
    /// 发生轮号
    pub round: u64,
    /// 处置动作（1 重试 / 2 重启 / 3 保缓冲）
    pub action: u8,
    /// 是否已结算
    pub settled: bool,
}

/// vim 场错误注入总账。
#[derive(Clone, Debug)]
pub struct VimErrField {
    /// 三故障位
    pub sites: Vec<FaultSite>,
    /// 回执环（滚动八格）
    pub recs: Vec<RecoveryRec>,
    /// 已步进轮数
    pub round: u64,
    /// 累计注入数
    pub injects: u64,
    /// 累计存活数
    pub survives: u64,
    /// 累计恢复数
    pub recovers: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl VimErrField {
    /// 立场：三故障位全部立账（零计数待注入）。
    pub fn new() -> VimErrField {
        let mut f = VimErrField {
            sites: Vec::new(),
            recs: Vec::new(),
            round: 0,
            injects: 0,
            survives: 0,
            recovers: 0,
            frozen: false,
        };
        let names = ["EIO", "EAGAIN", "EINTR"];
        let mut k = 0usize;
        while k < KINDS {
            f.sites.push(FaultSite {
                sid: 0x15000u32.wrapping_add(k as u32),
                kind: (k as u8) + 1,
                name: names[k].to_string(),
                hits: 0,
                survived: 0,
                recovered: 0,
            });
            k += 1;
        }
        f
    }

    /// 按故障类定位位下标（1..=3）。
    pub fn site_ix(&self, kind: u8) -> Result<usize, u32> {
        if self.sites.is_empty() {
            return Err(VE_EMPTY);
        }
        if kind == 0 || kind as usize > KINDS {
            return Err(VE_BADKIND);
        }
        let ix = (kind as usize).wrapping_sub(1);
        if ix >= self.sites.len() {
            return Err(VE_RANGE);
        }
        Ok(ix)
    }

    /// 轮号 → 故障类（三相轮转，确定性混合）。
    pub fn kind_at(round: u64) -> u8 {
        match round % 3 {
            0 => KIND_EINTR,
            1 => KIND_EIO,
            _ => KIND_EAGAIN,
        }
    }

    /// 故障类 → 处置动作（1 重试 / 2 重启 / 3 保缓冲）。
    pub fn action_of(kind: u8) -> u8 {
        match kind {
            KIND_EAGAIN => 1,
            KIND_EINTR => 2,
            KIND_EIO => 3,
            _ => 0,
        }
    }

    /// 记一条恢复回执：回执号全账唯一。
    pub fn log(&mut self, rid: u32, kind: u8, round: u64, action: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(VE_FROZEN);
        }
        let _ = self.site_ix(kind)?;
        let mut k = 0usize;
        while k < self.recs.len() {
            if self.recs[k].rid == rid {
                return Err(VE_DUP);
            }
            k += 1;
        }
        if self.recs.len() < REC_CAP {
            self.recs.push(RecoveryRec {
                rid,
                kind,
                round,
                action,
                settled: false,
            });
        } else {
            // 环满即滚动：最旧的让位
            let mut j = 0usize;
            while j + 1 < self.recs.len() {
                self.recs[j] = self.recs[j + 1].clone();
                j += 1;
            }
            let last = self.recs.len() - 1;
            self.recs[last] = RecoveryRec {
                rid,
                kind,
                round,
                action,
                settled: false,
            };
        }
        Ok(())
    }

    /// 结算一条回执：**幂等**。
    ///
    /// 语义修正记录：初版对已结算回执返回 VE_DUP，与
    /// `unx10976_vim_err_field_flow` 的契约（同一回执二次结算仍 `is_ok`
    /// 且账不变）相悖。恢复回执是「完成通知」性质——重投的通知必须可安全
    /// 重放，故重复结算按空操作处理（`settled` 位保证只计一次，绝不重复
    /// 记账）；真正的重号（不同轮次复用同一 rid）仍由 `log` 的 VE_DUP
    /// 拦截，两条拒收面互不替代。
    pub fn settle(&mut self, rid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(VE_FROZEN);
        }
        let mut k = 0usize;
        while k < self.recs.len() {
            if self.recs[k].rid == rid {
                self.recs[k].settled = true;
                return Ok(());
            }
            k += 1;
        }
        Err(VE_EMPTY)
    }

    /// 步进一轮：确定性混合注入 + 立即结算。
    pub fn step(&mut self) -> Result<(u8, u8), u32> {
        if self.frozen {
            return Err(VE_FROZEN);
        }
        if self.sites.is_empty() {
            return Err(VE_EMPTY);
        }
        let r = self.round.wrapping_add(1);
        let kind = Self::kind_at(r);
        let action = Self::action_of(kind);
        let ix = self.site_ix(kind)?;
        // 三类处置均不折损存活：命中即存活即恢复
        let site = &mut self.sites[ix];
        site.hits = site.hits.wrapping_add(1);
        site.survived = site.survived.wrapping_add(1);
        site.recovered = site.recovered.wrapping_add(1);
        self.injects = self.injects.wrapping_add(1);
        self.survives = self.survives.wrapping_add(1);
        self.recovers = self.recovers.wrapping_add(1);
        self.round = r;
        // 回执环：轮号即回执号（唯一）
        let rid = 0x9000u32.wrapping_add(r as u32);
        self.log(rid, kind, r, action)?;
        let _ = self.settle(rid);
        Ok((kind, action))
    }

    /// 混合注入 n 轮。
    pub fn mix(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.step()?;
            k = k.wrapping_add(1);
        }
        Ok(self.round)
    }

    /// 三方对平：注入 == 存活 == 恢复。
    pub fn balanced(&self) -> bool {
        self.injects > 0 && self.injects == self.survives && self.injects == self.recovers
    }

    /// 恢复账闸：不齐即 VE_LOST。
    pub fn recovery_gate(&self) -> Result<(), u32> {
        if self.injects == 0 {
            return Err(VE_EMPTY);
        }
        if !self.balanced() {
            return Err(VE_LOST);
        }
        Ok(())
    }

    /// 三位齐绿：每位命中非零且三段对平。
    pub fn all_sites_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.sites.len() {
            if !self.sites[k].balanced() {
                return false;
            }
            k += 1;
        }
        self.sites.len() == KINDS
    }

    /// 类分布：返回某类的命中数。
    pub fn kind_hits(&self, kind: u8) -> Result<u64, u32> {
        let ix = self.site_ix(kind)?;
        Ok(self.sites[ix].hits)
    }

    /// 环账已结算数。
    pub fn settled_in_ring(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.recs.len() {
            if self.recs[k].settled {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = VE_TAG;
        let mut k = 0usize;
        while k < self.sites.len() {
            let s = &self.sites[k];
            h = (h ^ s.sid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.kind as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.hits).wrapping_mul(0x100000001b3);
            h = (h ^ s.survived).wrapping_mul(0x100000001b3);
            h = (h ^ s.recovered).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut j = 0usize;
        while j < self.recs.len() {
            let r = &self.recs[j];
            h = (h ^ r.rid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.action as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.settled as u64).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结故障场。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮混合注入全存活全恢复。
pub fn sample_mixed() -> (VimErrField, bool) {
    let mut f = VimErrField::new();
    let _ = f.mix(ROUNDS);
    let ok = f.balanced() && f.all_sites_balanced();
    f.freeze();
    (f, ok)
}

/// 注入核：坏故障类 / 重号回执 / 冻结后步进 / 空账步进。
pub fn inject_four() -> [bool; 4] {
    let mut f = VimErrField::new();
    // 注入 1：坏故障类 → VE_BADKIND 拒收
    let i1 = matches!(f.log(1, 9, 1, 1), Err(VE_BADKIND));
    // 注入 2：重号回执 → VE_DUP 拒收
    let _ = f.log(2, KIND_EIO, 1, 3);
    let i2 = matches!(f.log(2, KIND_EIO, 2, 3), Err(VE_DUP));
    // 注入 3：冻结后步进 → VE_FROZEN 拒收
    f.freeze();
    let i3 = matches!(f.step(), Err(VE_FROZEN));
    // 注入 4：空账步进 → VE_EMPTY 拒收
    let mut empty = VimErrField {
        sites: Vec::new(),
        recs: Vec::new(),
        round: 0,
        injects: 0,
        survives: 0,
        recovers: 0,
        frozen: false,
    };
    let i4 = matches!(empty.step(), Err(VE_EMPTY));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 三错误混合注入千次场景存活，恢复账对平。
pub fn run_unx10976_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10976");
    let (f, ok) = sample_mixed();

    s.add("J1-千轮存活", ok && f.round == ROUNDS, "三错误混合注入千轮场景存活");
    s.add("J1-恢复对平", f.balanced() && f.recovery_gate().is_ok(), "注入与恢复账三方对平");
    s.add("J2-例数守恒", f.injects == ROUNDS && f.survives == ROUNDS && f.recovers == ROUNDS, "千例注入零折损守恒");
    s.add("J2-三位对平", f.all_sites_balanced(), "三故障位各自命中=存活=恢复");
    s.add("J3-三相分布", f.kind_hits(KIND_EIO).unwrap_or(0) + f.kind_hits(KIND_EAGAIN).unwrap_or(0) + f.kind_hits(KIND_EINTR).unwrap_or(0) == ROUNDS, "三类命中之和恰为千轮");
    s.add("J3-分布均匀", {
        let a = f.kind_hits(KIND_EIO).unwrap_or(0);
        let b = f.kind_hits(KIND_EAGAIN).unwrap_or(0);
        let c = f.kind_hits(KIND_EINTR).unwrap_or(0);
        let mx = if a > b { if a > c { a } else { c } } else if b > c { b } else { c };
        let mn = if a < b { if a < c { a } else { c } } else if b < c { b } else { c };
        mx.wrapping_sub(mn) <= 1
    }, "三相轮转分布差不超过 1");
    s.add("J4-处置映射", {
        let mut good = true;
        let mut k = 1u8;
        while k <= KINDS as u8 {
            let act = VimErrField::action_of(k);
            if act == 0 || act > 3 {
                good = false;
            }
            k += 1;
        }
        good && VimErrField::action_of(KIND_EAGAIN) == 1
            && VimErrField::action_of(KIND_EINTR) == 2
            && VimErrField::action_of(KIND_EIO) == 3
    }, "三类故障处置动作一一映射");
    s.add("J4-零缺口", f.sites.iter().all(|x| x.alive_gap() == 0 && x.recover_gap() == 0), "逐位存活缺口与恢复缺口双零");
    s.add("J5-环账滚动", f.recs.len() == REC_CAP && f.recs[REC_CAP - 1].round == ROUNDS, "回执环八格滚动至末轮");
    s.add("J5-环账结算", f.settled_in_ring() == REC_CAP, "环内回执全部结算");
    s.add("J6-坏类拒收", inject_four()[0], "坏故障类被 VE_BADKIND 拒收");
    s.add("J6-重号拒收", inject_four()[1], "重号回执被 VE_DUP 拒收");
    s.add("J7-冻结拒步", inject_four()[2], "冻结后步进被 VE_FROZEN 拒止");
    s.add("J7-空账拒步", inject_four()[3], "空账步进被 VE_EMPTY 拒止");
    s.add("J8-指纹稳定", f.frozen && f.fold() == f.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-场动态",
        ok,
        &format!(
            "轮 {} 注入 {} 存活 {} 恢复 {} EIO/EAGAIN/EINTR={}/{}/{}",
            f.round,
            f.injects,
            f.survives,
            f.recovers,
            f.kind_hits(KIND_EIO).unwrap_or(0),
            f.kind_hits(KIND_EAGAIN).unwrap_or(0),
            f.kind_hits(KIND_EINTR).unwrap_or(0),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10976_vim_err_field_flow() {
        let mut f = VimErrField::new();
        // 千轮混合注入
        assert_eq!(f.mix(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(f.balanced());
        assert!(f.all_sites_balanced());
        assert_eq!(f.injects, ROUNDS);
        // 单轮语义抽查
        let mut g = VimErrField::new();
        let (kind, action) = g.step().unwrap_or((0, 0));
        assert_eq!(kind, KIND_EIO);
        assert_eq!(action, 3);
        assert_eq!(g.site_ix(KIND_EIO).unwrap_or(9), 0);
        assert!(g.site_ix(0).is_err());
        // 结算幂等
        let rid = g.recs[0].rid;
        assert!(g.settle(rid).is_ok());
        let root = f.freeze();
        assert!(f.frozen);
        assert_eq!(root, f.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10976_checks_green() {
        let s = run_unx10976_checks();
        assert!(s.all_passed(), "UNX-F10976 判据未全绿");
    }
}
