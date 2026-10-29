//! UNX-F10981 多进程 IPC 压测场搭建（进程组+资源池全链）（0x1da20 段）
//!
//! 锚点：UNX-F10981-J1 压测场全链搭建（10/10），资源池四账对平（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·压测场）。
//!
//! 结构：进程组账（十组：组号 + 起停进程数 + 逐类申领数）+ 资源池账
//! （四池：管道 / 共享内存 / 信号量 / 消息队列，逐池记申领/授予/
//! 释放/在握四段）+ 搭场器（先起组后分配，组不完即拒分配）+ 对平闸
//! （四池逐池 申领 == 授予 + 等待 且 在握 == 申领 - 释放）+ fold 与
//! 冻结 + 注入核（坏池类/越界组号/重复起组/冻结后分配四路必抓）。
//!
//! 对平律：十组各池申领之和 == 池账申领；池账在握 == 申领 - 释放；
//! 组账与池账必须双向可核，任一侧不齐即 SF_IMBALANCE。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10981 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1da20;

/// 空场 —— 未立组即分配
pub const SF_EMPTY: u32 = SEG_BASE + 0x01;
/// 场号越界 —— 十组 0..10 / 四池 1..=4
pub const SF_RANGE: u32 = SEG_BASE + 0x02;
/// 重复起组 —— 一组只准起一次
pub const SF_DUP: u32 = SEG_BASE + 0x03;
/// 坏池类 —— 池类编号 1..=4
pub const SF_BADCLASS: u32 = SEG_BASE + 0x04;
/// 四账不齐 —— 申领/授予/释放/在握对不平
pub const SF_IMBALANCE: u32 = SEG_BASE + 0x05;
/// 场已冻结 —— freeze 后禁止再分配
pub const SF_FROZEN: u32 = SEG_BASE + 0x06;

/// 进程组数（十组）。
pub const GROUPS: usize = 10;
/// 资源池数（四池）。
pub const POOLS: usize = 4;
/// 每组进程数。
pub const PROCS_PER_GROUP: u8 = 4;
/// 池容量（每组每池至多一件，另有冗余）。
pub const POOL_CAP: u64 = 16;
/// 池类：管道。
pub const POOL_PIPE: u8 = 1;
/// 池类：共享内存。
pub const POOL_SHM: u8 = 2;
/// 池类：信号量。
pub const POOL_SEM: u8 = 3;
/// 池类：消息队列。
pub const POOL_MSG: u8 = 4;
/// 压测场域标（fold 种子）。
pub const SF_TAG: u64 = 0x1da_81;

/// 进程组：一组的起停与逐类申领。
#[derive(Clone, Debug)]
pub struct ProcGroup {
    /// 组号（0x15400 段起编）
    pub gid: u32,
    /// 组序（0..10）
    pub no: u8,
    /// 起停进程数
    pub started: u8,
    /// 逐类申领数（按池序 0..4）
    pub claims: [u64; POOLS],
}

impl ProcGroup {
    /// 组是否已起满。
    pub fn full(&self) -> bool {
        self.started == PROCS_PER_GROUP
    }

    /// 组申领总数。
    pub fn claim_total(&self) -> u64 {
        let mut t = 0u64;
        let mut k = 0usize;
        while k < POOLS {
            t = t.wrapping_add(self.claims[k]);
            k += 1;
        }
        t
    }

    /// 组内逐类齐领（每池至少一件）。
    pub fn all_classes(&self) -> bool {
        let mut k = 0usize;
        while k < POOLS {
            if self.claims[k] == 0 {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// 资源池：一类的四段计数。
#[derive(Clone, Debug)]
pub struct ResPool {
    /// 池号（0x15500 段起编）
    pub pid: u32,
    /// 池类号
    pub class: u8,
    /// 池名
    pub name: String,
    /// 申领总数
    pub req: u64,
    /// 授予总数
    pub grant: u64,
    /// 释放总数
    pub freed: u64,
    /// 在握数
    pub held: u64,
}

impl ResPool {
    /// 四账对平：申领 == 授予 + 等待，且在握 == 申领 - 释放。
    pub fn balanced(&self) -> bool {
        let waiting = self.req.wrapping_sub(self.grant);
        self.held == self.req.wrapping_sub(self.freed) && self.grant.wrapping_add(waiting) == self.req
    }

    /// 池是否满（在握达容量）。
    pub fn saturated(&self) -> bool {
        self.held >= POOL_CAP
    }

    /// 池水位（千分比）。
    pub fn watermark_pm(&self) -> u64 {
        if POOL_CAP == 0 {
            return 0;
        }
        self.held.wrapping_mul(1000) / POOL_CAP
    }
}

/// 多进程 IPC 压测场。
#[derive(Clone, Debug)]
pub struct StressField {
    /// 十进程组
    pub groups: Vec<ProcGroup>,
    /// 四资源池
    pub pools: Vec<ResPool>,
    /// 已起组数
    pub started_groups: usize,
    /// 已分配件数
    pub granted: u64,
    /// 拒配次数
    pub rejects: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl StressField {
    /// 立场：十组四池全部登记（零计数）。
    pub fn new() -> StressField {
        let mut f = StressField {
            groups: Vec::new(),
            pools: Vec::new(),
            started_groups: 0,
            granted: 0,
            rejects: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < GROUPS {
            f.groups.push(ProcGroup {
                gid: 0x15400u32.wrapping_add(k as u32),
                no: k as u8,
                started: 0,
                claims: [0u64; POOLS],
            });
            k += 1;
        }
        let names = ["pipe", "shm", "sem", "msg"];
        let mut j = 0usize;
        while j < POOLS {
            f.pools.push(ResPool {
                pid: 0x15500u32.wrapping_add(j as u32),
                class: (j as u8) + 1,
                name: names[j].to_string(),
                req: 0,
                grant: 0,
                freed: 0,
                held: 0,
            });
            j += 1;
        }
        f
    }

    /// 池类定位。
    pub fn pool_ix(&self, class: u8) -> Result<usize, u32> {
        if self.pools.is_empty() {
            return Err(SF_EMPTY);
        }
        if class == 0 || class as usize > POOLS {
            return Err(SF_BADCLASS);
        }
        let ix = (class as usize).wrapping_sub(1);
        if ix >= self.pools.len() {
            return Err(SF_RANGE);
        }
        Ok(ix)
    }

    /// 起一组：一组只准起一次，起满即组齐。
    pub fn spawn_group(&mut self, no: usize) -> Result<u8, u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        if self.groups.is_empty() {
            return Err(SF_EMPTY);
        }
        if no >= self.groups.len() {
            return Err(SF_RANGE);
        }
        let g = &mut self.groups[no];
        if g.started != 0 {
            return Err(SF_DUP);
        }
        g.started = PROCS_PER_GROUP;
        self.started_groups += 1;
        Ok(g.started)
    }

    /// 分配一件资源：组必须先起满。
    pub fn assign(&mut self, no: usize, class: u8, n: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        if self.groups.is_empty() || self.pools.is_empty() {
            return Err(SF_EMPTY);
        }
        if no >= self.groups.len() {
            return Err(SF_RANGE);
        }
        let pix = self.pool_ix(class)?;
        if !self.groups[no].full() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SF_EMPTY);
        }
        // 容量校验：超出即记等待（不授予），四账仍须对平
        self.groups[no].claims[pix] = self.groups[no].claims[pix].wrapping_add(n);
        let p = &mut self.pools[pix];
        p.req = p.req.wrapping_add(n);
        if p.held.wrapping_add(n) <= POOL_CAP {
            p.grant = p.grant.wrapping_add(n);
            p.held = p.held.wrapping_add(n);
            self.granted = self.granted.wrapping_add(n);
        }
        Ok(p.held)
    }

    /// 释放一件资源。
    pub fn free_one(&mut self, class: u8) -> Result<u64, u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        if self.pools.is_empty() {
            return Err(SF_EMPTY);
        }
        let pix = self.pool_ix(class)?;
        if self.pools[pix].held == 0 {
            return Err(SF_IMBALANCE);
        }
        let p = &mut self.pools[pix];
        p.freed = p.freed.wrapping_add(1);
        p.held = p.held.wrapping_sub(1);
        Ok(p.held)
    }

    /// 全链搭场：十组起满 + 每组每池各领一件。
    pub fn build(&mut self) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.groups.len() {
            self.spawn_group(k)?;
            let mut c = 1u8;
            while c <= POOLS as u8 {
                self.assign(k, c, 1)?;
                c += 1;
            }
            k += 1;
        }
        Ok(self.granted)
    }

    /// 全链判定：十组起满、逐组逐池齐领、四池对平。
    pub fn full_chain(&self) -> bool {
        if self.groups.len() != GROUPS || self.pools.len() != POOLS {
            return false;
        }
        let mut k = 0usize;
        while k < self.groups.len() {
            if !self.groups[k].full() || !self.groups[k].all_classes() {
                return false;
            }
            k += 1;
        }
        self.pools_balanced()
    }

    /// 四池逐池对平。
    pub fn pools_balanced(&self) -> bool {
        if self.pools.len() != POOLS {
            return false;
        }
        let mut k = 0usize;
        while k < self.pools.len() {
            if !self.pools[k].balanced() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 双向对平：组账申领之和 == 池账申领。
    pub fn two_way(&self) -> bool {
        let mut k = 0usize;
        while k < POOLS {
            let mut gsum = 0u64;
            let mut j = 0usize;
            while j < self.groups.len() {
                gsum = gsum.wrapping_add(self.groups[j].claims[k]);
                j += 1;
            }
            if gsum != self.pools[k].req {
                return false;
            }
            k += 1;
        }
        !self.groups.is_empty()
    }

    /// 全场放空：四池在握全部释放归零。
    pub fn drain(&mut self) -> Result<u64, u32> {
        let mut c = 1u8;
        while c <= POOLS as u8 {
            loop {
                let pix = self.pool_ix(c)?;
                if self.pools[pix].held == 0 {
                    break;
                }
                self.free_one(c)?;
            }
            c += 1;
        }
        let mut t = 0u64;
        let mut k = 0usize;
        while k < self.pools.len() {
            t = t.wrapping_add(self.pools[k].held);
            k += 1;
        }
        Ok(t)
    }

    /// 某种池的申领数。
    pub fn req_of(&self, class: u8) -> Result<u64, u32> {
        let ix = self.pool_ix(class)?;
        Ok(self.pools[ix].req)
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SF_TAG;
        let mut k = 0usize;
        while k < self.groups.len() {
            let g = &self.groups[k];
            h = (h ^ g.gid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.started as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.claim_total()).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut j = 0usize;
        while j < self.pools.len() {
            let p = &self.pools[j];
            h = (h ^ p.req).wrapping_mul(0x100000001b3);
            h = (h ^ p.grant).wrapping_mul(0x100000001b3);
            h = (h ^ p.held).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结压测场。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：十组四池全链搭起并四账对平。
pub fn sample_built() -> (StressField, bool) {
    let mut f = StressField::new();
    let _ = f.build();
    let ok = f.full_chain() && f.two_way() && f.rejects == 0;
    f.freeze();
    (f, ok)
}

/// 注入核：坏池类 / 越界组号 / 重复起组 / 冻结后分配。
pub fn inject_four() -> [bool; 4] {
    let mut f = StressField::new();
    // 注入 1：坏池类 → SF_BADCLASS 拒收
    let i1 = matches!(f.assign(0, 9, 1), Err(SF_BADCLASS));
    // 注入 2：越界组号 → SF_RANGE 拒收
    let i2 = matches!(f.spawn_group(GROUPS), Err(SF_RANGE));
    // 注入 3：重复起组 → SF_DUP 拒收
    let _ = f.spawn_group(0);
    let i3 = matches!(f.spawn_group(0), Err(SF_DUP));
    // 注入 4：冻结后分配 → SF_FROZEN 拒收
    f.freeze();
    let i4 = matches!(f.assign(1, POOL_PIPE, 1), Err(SF_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 压测场全链搭建，资源池四账对平。
pub fn run_unx10981_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10981");
    let (f, ok) = sample_built();

    s.add("J1-全链搭建", ok && f.full_chain(), "十组四池全链搭建完成");
    s.add("J1-四账对平", f.pools_balanced(), "四资源池逐池四账对平");
    s.add("J2-十组起满", f.started_groups == GROUPS && f.groups.iter().all(|g| g.full()), "十进程组全部起满");
    s.add("J2-四池齐备", f.pools.len() == POOLS && f.groups.iter().all(|g| g.all_classes()), "四池齐备且每组逐池齐领");
    s.add("J3-双向对平", f.two_way(), "组账申领与池账申领双向对平");
    s.add("J3-授予等于申领", f.pools.iter().all(|p| p.grant == p.req), "四池授予恰等于申领");
    s.add("J3-在握守恒", f.pools.iter().all(|p| p.held == p.req.wrapping_sub(p.freed)), "四池在握等于申领减释放");
    s.add("J4-件数守恒", f.granted == (GROUPS as u64).wrapping_mul(POOLS as u64), "授予件数恰为十组乘四池");
    s.add("J4-水位", f.pools.iter().all(|p| p.watermark_pm() <= 1000), "四池水位千分比不越满");
    let mut d5 = StressField::new();
    let built = d5.build() == Ok((GROUPS * POOLS) as u64);
    // 冻结后的场不可再释放，放空须在活跃场上进行。
    let ran = built && d5.drain() == Ok(0);
    s.add(
        "J5-放空归零",
        ran && d5.pools.iter().all(|p| p.held == 0) && d5.pools_balanced(),
        "全场放空后在握归零且四账仍平",
    );
    s.add("J5-部分非全链", {
        let mut p = StressField::new();
        let _ = p.spawn_group(0);
        !p.full_chain() && p.pools.iter().all(|x| x.req == 0)
    }, "只起一组即非全链");
    s.add("J6-坏类拒配", inject_four()[0], "坏池类被 SF_BADCLASS 拒收");
    s.add("J6-越界拒起", inject_four()[1], "越界组号被 SF_RANGE 拒收");
    s.add("J7-重起拒止", inject_four()[2], "重复起组被 SF_DUP 拒止");
    s.add("J7-冻结拒配", inject_four()[3], "冻结后分配被 SF_FROZEN 拒止");
    s.add("J8-未起拒配", {
        let mut g = StressField::new();
        g.assign(0, POOL_PIPE, 1).is_err() && g.rejects == 1
    }, "未起组即分配被拒且留拒配痕");
    s.add("J8-指纹稳定", f.frozen && f.fold() == f.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-压测场动态",
        ok,
        &format!(
            "组 {}/{} 池 {} 授予 {} 拒配 {} 管道申领 {}",
            f.started_groups,
            GROUPS,
            f.pools.len(),
            f.granted,
            f.rejects,
            f.req_of(POOL_PIPE).unwrap_or(0),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10981_stress_field_flow() {
        let mut f = StressField::new();
        assert_eq!(f.build().unwrap_or(0), (GROUPS * POOLS) as u64);
        assert!(f.full_chain());
        assert!(f.two_way());
        assert_eq!(f.started_groups, GROUPS);
        // 单步语义抽查
        let mut g = StressField::new();
        let _ = g.spawn_group(3);
        assert_eq!(g.assign(3, POOL_MSG, 2).unwrap_or(9), 2);
        assert_eq!(g.req_of(POOL_MSG).unwrap_or(0), 2);
        assert!(g.pool_ix(0).is_err());
        assert!(g.free_one(POOL_PIPE).is_err());
        let root = f.freeze();
        assert!(f.frozen);
        assert_eq!(root, f.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10981_checks_green() {
        let s = run_unx10981_checks();
        assert!(s.all_passed(), "UNX-F10981 判据未全绿");
    }
}
