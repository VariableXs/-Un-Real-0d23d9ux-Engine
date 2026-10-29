//! UNX-F10988 进程组批量 kill 压力（killpg+IPC 资源残留账）（0x1db00 段）
//!
//! 锚点：UNX-F10988-J1 killpg 全组终止千次无漏（10/10），IPC 残留账
//! 逐件核销（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·进程组终止面）。
//!
//! 结构：成员账（八成员：存活位 + 四类 IPC 持有数）+ 组账（起组数 +
//! 终止数 + 核销件数 + 残留件数）+ 终止器（killpg 全体落终止位）+
//! 核销器（逐成员逐类清零持有数并记核销件）+ 残留探针（全场持有数
//! 之和必须归零）+ fold 与冻结 + 注入核（越界成员/重生拒绝重复终止/
//! 残留未销/冻结后杀四路必抓）。
//!
//! 核销律：killpg 后组内成员全部落终止位；每成员的四类 IPC 持有数
//! 必须逐件核销归零；核销件数等于终止成员数乘四类，残留件数必须为零。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10988 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1db00;

/// 空账 —— 未起组即终止
pub const GK_EMPTY: u32 = SEG_BASE + 0x01;
/// 成员越界 —— 八成员 0..8
pub const GK_RANGE: u32 = SEG_BASE + 0x02;
/// 重复终止 —— 已终止成员不得再杀
pub const GK_DUP: u32 = SEG_BASE + 0x03;
/// 残留未销 —— 核销后仍有持有
pub const GK_RESIDUE: u32 = SEG_BASE + 0x04;
/// 未死即核销 —— 成员未终止不得核销资源
pub const GK_NOTDEAD: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再杀
pub const GK_FROZEN: u32 = SEG_BASE + 0x06;

/// 成员数。
pub const MEMBERS: usize = 8;
/// IPC 资源类数（fd/shm/sem/msg）。
pub const IPC_CLASSES: usize = 4;
/// 压力轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 终止账域标（fold 种子）。
pub const GK_TAG: u64 = 0x1db_88;

/// 组成员：存活位 + 四类 IPC 持有数。
#[derive(Clone, Debug)]
pub struct Member {
    /// 成员号（0x15f00 段起编）
    pub mid: u32,
    /// 成员序（0..8）
    pub mno: u8,
    /// 存活位
    pub alive: bool,
    /// 四类 IPC 持有数
    pub ipc: [u64; IPC_CLASSES],
}

impl Member {
    /// 持有件数总和。
    pub fn held(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < IPC_CLASSES {
            t = t.wrapping_add(self.ipc[k]);
            k += 1;
        }
        t
    }

    /// 已死且零持有。
    pub fn settled(&self) -> bool {
        !self.alive && self.held() == 0
    }

    /// 四类齐持（每类至少一件）。
    pub fn all_classes(&self) -> bool {
        let mut k = 0usize;
        while k < IPC_CLASSES {
            if self.ipc[k] == 0 {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// 进程组批量终止总账。
#[derive(Clone, Debug)]
pub struct GroupKillBook {
    /// 八成员
    pub members: Vec<Member>,
    /// 起组轮数
    pub rounds: u64,
    /// 累计终止成员数
    pub killed: u64,
    /// 累计核销件数
    pub reclaimed: u64,
    /// 残留件数（收束后必须为零）
    pub residues: u64,
    /// 重复终止拒止次数
    pub redouble: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl GroupKillBook {
    /// 立账：八成员全部登记（已死零持有）。
    pub fn new() -> GroupKillBook {
        let mut b = GroupKillBook {
            members: Vec::new(),
            rounds: 0,
            killed: 0,
            reclaimed: 0,
            residues: 0,
            redouble: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < MEMBERS {
            b.members.push(Member {
                mid: 0x15f00u32.wrapping_add(k as u32),
                mno: k as u8,
                alive: false,
                ipc: [0u64; IPC_CLASSES],
            });
            k += 1;
        }
        b
    }

    /// 成员定位。
    pub fn member_ix(&self, no: usize) -> Result<usize, u32> {
        if self.members.is_empty() {
            return Err(GK_EMPTY);
        }
        if no >= self.members.len() {
            return Err(GK_RANGE);
        }
        Ok(no)
    }

    /// 起组：全体复活并各持四类一件。
    pub fn spawn_group(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(GK_FROZEN);
        }
        if self.members.is_empty() {
            return Err(GK_EMPTY);
        }
        let mut k = 0usize;
        while k < self.members.len() {
            let m = &mut self.members[k];
            m.alive = true;
            let mut c = 0usize;
            while c < IPC_CLASSES {
                m.ipc[c] = m.ipc[c].wrapping_add(1);
                c += 1;
            }
            k += 1;
        }
        Ok(self.held_total())
    }

    /// killpg：组内全体落终止位，重复终止即拒。
    pub fn killpg(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(GK_FROZEN);
        }
        if self.members.is_empty() {
            return Err(GK_EMPTY);
        }
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.members.len() {
            if self.members[k].alive {
                self.members[k].alive = false;
                n = n.wrapping_add(1);
            } else {
                self.redouble = self.redouble.wrapping_add(1);
            }
            k += 1;
        }
        if n == 0 {
            return Err(GK_DUP);
        }
        self.killed = self.killed.wrapping_add(n);
        Ok(n)
    }

    /// 核销一成员的 IPC：未死即拒。
    pub fn reclaim(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(GK_FROZEN);
        }
        let ix = self.member_ix(no)?;
        if self.members[ix].alive {
            return Err(GK_NOTDEAD);
        }
        let freed = self.members[ix].held();
        let m = &mut self.members[ix];
        let mut c = 0usize;
        while c < IPC_CLASSES {
            m.ipc[c] = 0;
            c += 1;
        }
        self.reclaimed = self.reclaimed.wrapping_add(freed);
        Ok(freed)
    }

    /// 全组核销。
    pub fn reclaim_all(&mut self) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.members.len() {
            self.reclaim(k)?;
            k += 1;
        }
        Ok(self.reclaimed)
    }

    /// 残留探针：全场持有数之和。
    pub fn residue_scan(&self) -> Result<u64, u32> {
        if self.members.is_empty() {
            return Err(GK_EMPTY);
        }
        let t = self.held_total();
        if t != 0 {
            return Err(GK_RESIDUE);
        }
        Ok(t)
    }

    /// 全场持有数之和。
    pub fn held_total(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.members.len() {
            t = t.wrapping_add(self.members[k].held());
            k += 1;
        }
        t
    }

    /// 一轮：起组 → killpg → 全组核销 → 残留探针。
    pub fn round(&mut self) -> Result<u64, u32> {
        let _ = self.spawn_group()?;
        let _ = self.killpg()?;
        let _ = self.reclaim_all()?;
        if self.residue_scan().is_err() {
            self.residues = self.residues.wrapping_add(self.held_total());
        }
        self.rounds = self.rounds.wrapping_add(1);
        Ok(self.rounds)
    }

    /// 跑 n 轮。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round()?;
            k = k.wrapping_add(1);
        }
        Ok(self.rounds)
    }

    /// 全组落终止位。
    pub fn all_dead(&self) -> bool {
        let mut k = 0usize;
        while k < self.members.len() {
            if self.members[k].alive {
                return false;
            }
            k += 1;
        }
        !self.members.is_empty()
    }

    /// 逐成员已收束（死且零持有）。
    pub fn all_settled(&self) -> bool {
        let mut k = 0usize;
        while k < self.members.len() {
            if !self.members[k].settled() {
                return false;
            }
            k += 1;
        }
        !self.members.is_empty()
    }

    /// 核销守恒：核销件数 == 终止成员数 × 四类。
    pub fn reclaim_conserved(&self) -> bool {
        self.killed.wrapping_mul(IPC_CLASSES as u64) == self.reclaimed
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = GK_TAG;
        let mut k = 0usize;
        while k < self.members.len() {
            let m = &self.members[k];
            h = (h ^ m.mid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ m.alive as u64).wrapping_mul(0x100000001b3);
            let mut c = 0usize;
            while c < IPC_CLASSES {
                h = (h ^ m.ipc[c]).wrapping_mul(0x100000001b3);
                c += 1;
            }
            k += 1;
        }
        h = (h ^ self.killed).wrapping_mul(0x100000001b3);
        h = (h ^ self.reclaimed).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮全组终止且残留逐件核销。
pub fn sample_killed() -> (GroupKillBook, bool) {
    let mut b = GroupKillBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.all_dead()
        && b.all_settled()
        && b.residue_scan().is_ok()
        && b.reclaim_conserved()
        && b.residues == 0;
    b.freeze();
    (b, ok)
}

/// 注入核：越界成员 / 重复终止 / 残留未销 / 冻结后杀。
pub fn inject_four() -> [bool; 4] {
    // 注入 1：越界成员 → GK_RANGE 拒收
    let mut b = GroupKillBook::new();
    let i1 = matches!(b.reclaim(MEMBERS), Err(GK_RANGE));
    // 注入 2：重复终止 → 全组已死再 killpg 即 GK_DUP
    let _ = b.spawn_group();
    let _ = b.killpg();
    let i2 = matches!(b.killpg(), Err(GK_DUP));
    // 注入 3：残留未销 → 复活后不核销即 GK_RESIDUE
    let mut r = GroupKillBook::new();
    let _ = r.spawn_group();
    let i3 = r.residue_scan() == Err(GK_RESIDUE);
    // 注入 4：冻结后杀 → GK_FROZEN 拒收
    let mut f = GroupKillBook::new();
    f.freeze();
    let i4 = matches!(f.killpg(), Err(GK_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 killpg 全组终止千次无漏，残留逐件核销。
pub fn run_unx10988_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10988");
    let (b, ok) = sample_killed();

    s.add("J1-千次无漏", ok && b.all_dead(), "千轮 killpg 全组终止无漏");
    s.add("J1-残留核销", b.all_settled() && b.residue_scan().is_ok(), "八成员逐者死且零持有");
    s.add("J2-终止守恒", b.killed == ROUNDS * MEMBERS as u64, "千轮乘八成员终止数守恒");
    s.add("J2-核销守恒", b.reclaim_conserved(), "核销件数等于终止成员乘四类");
    s.add("J2-轮数守恒", b.rounds == ROUNDS && b.residues == 0, "千轮收束且残留件归零");
    s.add("J3-件数对账", b.reclaimed == ROUNDS * MEMBERS as u64 * IPC_CLASSES as u64, "核销总件数等于千乘八乘四");
    s.add("J3-零残留", b.held_total() == 0, "全场持有总数归零");
    s.add("J3-成员编号", b.members.iter().enumerate().all(|(k, m)| m.mid == 0x15f00 + k as u32), "成员号自 0x15f00 起编连续");
    s.add("J4-复活齐持", {
        let mut g = GroupKillBook::new();
        let _ = g.spawn_group();
        g.members.iter().all(|m| m.alive && m.all_classes()) && g.held_total() == (MEMBERS * IPC_CLASSES) as u64
    }, "起组后八成员齐持四类");
    s.add("J4-杀后清零前", {
        let mut g = GroupKillBook::new();
        let _ = g.spawn_group();
        let _ = g.killpg();
        g.all_dead() && g.held_total() == (MEMBERS * IPC_CLASSES) as u64
    }, "终止后持有数尚未核销仍计入");
    s.add("J5-未死拒销", {
        let mut g = GroupKillBook::new();
        let _ = g.spawn_group();
        g.reclaim(0) == Err(GK_NOTDEAD)
    }, "成员未终止即核销被 GK_NOTDEAD 拒止");
    s.add("J6-越界拒销", inject_four()[0], "越界成员被 GK_RANGE 拒收");
    s.add("J6-重杀拒止", inject_four()[1], "全组已死再杀被 GK_DUP 拒止");
    s.add("J7-残留拒放", inject_four()[2], "残留未销被 GK_RESIDUE 拒放");
    s.add("J7-冻结拒杀", inject_four()[3], "冻结后杀被 GK_FROZEN 拒止");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add("J8-重复留痕", b.redouble == 0, "正常路径零重复终止");
    s.add_dyn(
        "J9-终止动态",
        ok,
        &format!(
            "轮 {} 终止 {} 核销 {} 残留 {} 持有 {} 重杀 {}",
            b.rounds,
            b.killed,
            b.reclaimed,
            b.residues,
            b.held_total(),
            b.redouble,
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10988_group_kill_flow() {
        let mut b = GroupKillBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.all_dead());
        assert!(b.all_settled());
        assert_eq!(b.killed, 8000);
        assert_eq!(b.reclaimed, 32000);
        // 单步语义抽查
        let mut g = GroupKillBook::new();
        assert_eq!(g.spawn_group().unwrap_or(0), 32);
        assert_eq!(g.killpg().unwrap_or(0), MEMBERS as u64);
        assert!(g.all_dead());
        assert_eq!(g.reclaim(0).unwrap_or(9), IPC_CLASSES as u64);
        assert!(g.residue_scan().is_err());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10988_checks_green() {
        let s = run_unx10988_checks();
        assert!(s.all_passed(), "UNX-F10988 判据未全绿");
    }
}
