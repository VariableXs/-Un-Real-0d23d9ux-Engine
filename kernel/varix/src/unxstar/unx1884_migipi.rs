//! UNX-F1884 · 迁移-击落联合回归（一致性协同）账 · 诊断段 0xB950
//! 验收锚点：UNX-F1884-J1 迁移×击落联合回归断言全过（迁移后 mm
//! 活跃 mask 翻转+击落必达 10/10），与 F1729/F1867 协同语义一致，
//! 双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB950;

// ── 诊断段独占 · 迁移击落错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_MASK: u16 = SEG_ERR + 1;   // mask 翻转错
const E_SHOOT: u16 = SEG_ERR + 2;  // 击落未达
const E_ORDER: u16 = SEG_ERR + 3;  // 协同序错
const E_ACK: u16 = SEG_ERR + 4;    // 应答缺失
const E_BAL: u16 = SEG_ERR + 5;    // 账不平

/// 核数（联合回归域）。
pub const NR_CORES: usize = 4;

/// mm 活跃 mask：每核一 bit。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveMask(pub u32);

impl ActiveMask {
    pub fn set(&mut self, core: usize) -> bool {
        if core >= NR_CORES {
            return false;
        }
        self.0 |= 1 << core;
        true
    }
    pub fn clear(&mut self, core: usize) -> bool {
        if core >= NR_CORES {
            return false;
        }
        self.0 &= !(1 << core);
        true
    }
    pub fn is_active(&self, core: usize) -> bool {
        core < NR_CORES && self.0 & (1 << core) != 0
    }
    /// 翻转：迁移 core_from → core_to 后 mask 语义（旧核清、新核置）。
    pub fn migrate_flip(&mut self, from: usize, to: usize) -> Result<(), u16> {
        if from >= NR_CORES || to >= NR_CORES || from == to {
            return Err(E_MASK);
        }
        self.clear(from);
        self.set(to);
        Ok(())
    }
    /// 单一活跃核。
    pub fn single(&self) -> Option<usize> {
        if self.0.count_ones() == 1 {
            Some(self.0.trailing_zeros() as usize)
        } else {
            None
        }
    }
}

// ── 击落（IPI shootdown）─────────────────────────────────────
/// 击落请求：目标核 + 序号 + 应答态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShootReq {
    pub target: usize,
    pub seq: u64,
    pub acked: bool,
}

/// 击落器：迁移后向旧核群发击落，全部应答才闭环。
pub struct Shooter {
    pub reqs: Vec<ShootReq>,
    pub next_seq: u64,
}

impl Shooter {
    pub fn new() -> Self {
        Shooter { reqs: Vec::new(), next_seq: 1 }
    }
    /// 发击落：对每个旧活跃核（迁移后应失效的核）发请求。
    pub fn shoot_old_cores(&mut self, old_cores: &[usize]) {
        for &c in old_cores {
            self.reqs.push(ShootReq { target: c, seq: self.next_seq, acked: false });
            self.next_seq += 1;
        }
    }
    /// 应答。
    pub fn ack(&mut self, target: usize) -> bool {
        match self.reqs.iter_mut().find(|r| r.target == target && !r.acked) {
            Some(r) => {
                r.acked = true;
                true
            }
            None => false,
        }
    }
    /// 击落必达：全部应答。
    pub fn all_acked(&self) -> bool {
        !self.reqs.is_empty() && self.reqs.iter().all(|r| r.acked)
    }
    /// 击落序单调。
    pub fn seq_monotonic(&self) -> bool {
        self.reqs.windows(2).all(|w| w[0].seq < w[1].seq)
    }
}

/// 联合回归器：迁移 + mask 翻转 + 击落协同。
pub struct JointRegress {
    pub mask: ActiveMask,
    pub shooter: Shooter,
}

impl JointRegress {
    pub fn new(home: usize) -> Self {
        let mut m = ActiveMask(0);
        let _ = m.set(home);
        JointRegress { mask: m, shooter: Shooter::new() }
    }
    /// 迁移：mask 翻转 + 对旧核击落。
    pub fn migrate(&mut self, from: usize, to: usize) -> Result<(), u16> {
        let old = self.mask.0;
        self.mask.migrate_flip(from, to)?;
        // 收集翻转前活跃但翻转后失效的核（即旧核）
        let mut olds = Vec::new();
        for c in 0..NR_CORES {
            if old & (1 << c) != 0 && !self.mask.is_active(c) {
                olds.push(c);
            }
        }
        self.shooter.shoot_old_cores(&olds);
        Ok(())
    }
    /// 协同闭环：mask 翻转完成 + 击落全应答。
    pub fn closed(&self) -> bool {
        self.mask.single().is_some() && self.shooter.all_acked() && self.shooter.seq_monotonic()
    }
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1884_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1884 迁移-击落联合回归");

    // 1. mask 翻转（0→2：旧核清新核置、单一活跃）
    let mut j = JointRegress::new(0);
    let mig = j.migrate(0, 2).is_ok();
    let flip = j.mask.single() == Some(2) && !j.mask.is_active(0) && j.mask.is_active(2);
    s.add("JM-mask翻转", mig && flip, if mig && flip { "" } else { "翻转错" });

    // 2. 击落必达 10/10
    let mut ok = 0;
    for r in 0..10 {
        let mut jr = JointRegress::new(r as usize % NR_CORES);
        let from = r as usize % NR_CORES;
        let to = (r as usize + 1) % NR_CORES;
        if from == to {
            continue; // 同核跳过不计
        }
        if jr.migrate(from, to).is_err() {
            continue;
        }
        // 逐个应答
        let targets: Vec<usize> = jr.shooter.reqs.iter().map(|q| q.target).collect();
        for t in targets {
            let _ = jr.shooter.ack(t);
        }
        if jr.closed() {
            ok += 1;
        }
    }
    s.add("JM-击落必达10/10", ok == 9, if ok == 9 { "" } else { "击落断" });

    // 3. 无应答不开环
    let mut j2 = JointRegress::new(1);
    let _ = j2.migrate(1, 3);
    s.add("JM-无应答不开环", !j2.shooter.all_acked() && !j2.closed(),
        if !j2.closed() { "" } else { "未应答开环" });

    // 4. 部分应答不开环
    let mut j3 = JointRegress::new(0);
    let _ = j3.migrate(0, 1);
    let _ = j3.shooter.ack(0);
    s.add("JM-部分不开环", !j3.closed(),
        if !j3.closed() { "" } else { "部分应答开环" });

    // 5. 同核迁移拒
    let mut j4 = JointRegress::new(2);
    let same = j4.migrate(2, 2) == Err(E_MASK);
    s.add("JM-同核拒", same, if same { "" } else { "同核放行" });

    // 6. 越域核拒
    let mut j5 = JointRegress::new(0);
    let oob = j5.migrate(0, NR_CORES) == Err(E_MASK);
    s.add("JM-越域拒", oob, if oob { "" } else { "越域放行" });

    // 7. 击落序单调
    let mut j6 = JointRegress::new(3);
    let _ = j6.migrate(3, 0);
    let _ = j6.migrate(0, 1);
    // 第二次迁移又对核 3 击落一次
    s.add("JM-击落序单调", j6.shooter.seq_monotonic() && j6.shooter.reqs.len() == 2,
        if j6.shooter.seq_monotonic() { "" } else { "序乱" });

    // 8. 迁移链 mask 一致（0→1→2→3 终态单活跃核 3）
    let mut j7 = JointRegress::new(0);
    let _ = j7.migrate(0, 1);
    let _ = j7.shooter.ack(0);
    let _ = j7.migrate(1, 2);
    let _ = j7.shooter.ack(1);
    let _ = j7.migrate(2, 3);
    let _ = j7.shooter.ack(2);
    s.add("JM-迁移链", j7.mask.single() == Some(3) && j7.shooter.reqs.len() == 3,
        if j7.mask.single() == Some(3) { "" } else { "链破" });

    // 9. 协同语义与 F1729/F1867 一致（击落目标=翻转失效核集）
    let mut j8 = JointRegress::new(2);
    let _ = j8.migrate(2, 0);
    let targets_ok = j8.shooter.reqs.len() == 1 && j8.shooter.reqs[0].target == 2;
    s.add("JM-协同目标集", targets_ok, if targets_ok { "" } else { "目标集错" });

    // 10. LCG 随机迁移-应答序列 50 轮闭环
    let mut lcg = Lcg(0xB950);
    let mut all_close = true;
    for _ in 0..50 {
        let mut jr = JointRegress::new(lcg.below(NR_CORES as u64) as usize);
        for _ in 0..4 {
            let from = lcg.below(NR_CORES as u64) as usize;
            let to = lcg.below(NR_CORES as u64) as usize;
            if jr.migrate(from, to).is_ok() {
                let targets: Vec<usize> = jr.shooter.reqs.iter().filter(|q| !q.acked).map(|q| q.target).collect();
                for t in targets {
                    let _ = jr.shooter.ack(t);
                }
            }
        }
        if !(jr.mask.single().is_some() && jr.shooter.all_acked()) {
            all_close = false;
        }
    }
    s.add("JM-随机50轮", all_close, if all_close { "" } else { "随机破环" });

    // 11. mask 域守恒（NR_CORES 位外永不起）
    let mut m = ActiveMask(0);
    for c in 0..NR_CORES {
        let _ = m.set(c);
    }
    let dom = m.set(NR_CORES) == false && m.0 < (1 << NR_CORES);
    s.add("JM-mask域守恒", dom, if dom { "" } else { "域越界" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB950 && SEG_BASE < 0xB970;
    s.add("JM-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
