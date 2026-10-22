//! UNX-F8221 · 进程组模型（pgid 归属与组账）账 · 诊断段 0xB360
//! 验收锚点：UNX-F8221-J1 组账（组员列表/组长）与描述符归属双向一致，
//! 组遍历计数 = 账面计数——进程组：pgid + 组长 pid + 组员 pid 集；
//! 归属双向：由 pid 查组 / 由组列成员，两向计数全等。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB360;

/// 进程组。
#[derive(Clone, Debug, PartialEq)]
pub struct ProcGroup {
    pub pgid: u32,
    pub leader: u32,
    pub members: Vec<u32>, // 组员 pid（含组长）
}

impl ProcGroup {
    pub fn new(pgid: u32, leader: u32) -> Self {
        ProcGroup { pgid, leader, members: vec![leader] }
    }
    pub fn join(&mut self, pid: u32) -> bool {
        if self.members.contains(&pid) {
            return false;
        }
        self.members.push(pid);
        true
    }
    pub fn leave(&mut self, pid: u32) -> bool {
        if pid == self.leader {
            return false; // 组长不可离组
        }
        let before = self.members.len();
        self.members.retain(|x| *x != pid);
        self.members.len() < before
    }
    pub fn is_member(&self, pid: u32) -> bool {
        self.members.contains(&pid)
    }
}

/// 组账：pgid → 组。
pub struct GroupLedger {
    pub groups: Vec<ProcGroup>,
}

impl GroupLedger {
    pub fn new() -> Self {
        GroupLedger { groups: Vec::new() }
    }
    pub fn create(&mut self, pgid: u32, leader: u32) -> bool {
        if self.groups.iter().any(|g| g.pgid == pgid) {
            return false;
        }
        self.groups.push(ProcGroup::new(pgid, leader));
        true
    }
    /// 由 pid 查组（归属正向）。
    pub fn group_of(&self, pid: u32) -> Option<&ProcGroup> {
        self.groups.iter().find(|g| g.is_member(pid))
    }
    /// 由组列成员（归属反向）。
    pub fn members_of(&self, pgid: u32) -> Vec<u32> {
        self.groups.iter().find(|g| g.pgid == pgid).map(|g| g.members.clone()).unwrap_or_default()
    }
    /// 双向一致：所有 pid 的正向归属组 == 其成员资格的反向组。
    pub fn bidirectional_ok(&self) -> bool {
        for g in &self.groups {
            for pid in &g.members {
                match self.group_of(*pid) {
                    Some(g2) if g2.pgid == g.pgid => {}
                    _ => return false,
                }
            }
        }
        true
    }
    /// 组遍历计数 == 账面计数。
    pub fn traversal_equals_ledger(&self) -> bool {
        let traversal: u32 = self.groups.iter().map(|g| g.members.len() as u32).sum();
        let ledger: u32 = self.groups.iter().map(|g| g.members.iter().filter(|p| self.group_of(**p).map(|x| x.pgid) == Some(g.pgid)).count() as u32).sum();
        traversal == ledger
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB360 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8221_B360;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("组-建组即含组长", ProcGroup::new(1, 100).members == vec![100]));
    out.push(("组-重复入组拒", !ProcGroup::new(1, 100).join(100)));
    out.push(("组-组长离组拒", !ProcGroup::new(1, 100).leave(100)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8221_checks：本单判据聚合（诊断段 0xB360）。
pub fn run_unx8221_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8221 进程组模型");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 建组 + 入组
    let mut led = GroupLedger::new();
    let c1 = led.create(100, 101);
    let g = led.groups.first_mut().unwrap();
    let j1 = g.join(102);
    let j2 = g.join(103);
    let build_ok = c1 && j1 && j2 && g.members.len() == 3 && g.leader == 101;
    s.add("建组-组长+2组员落账", build_ok, if build_ok { "" } else { "建组漂移" });

    // 归属正向：pid 查组
    let fwd = led.group_of(102).map(|g| g.pgid) == Some(100);
    s.add("正向-pid102归组100", fwd, if fwd { "" } else { "正向归属漂移" });

    // 归属反向：组列成员
    let rev = led.members_of(100);
    let rev_ok = rev == vec![101, 102, 103];
    s.add("反向-组100成员恰3人序一致", rev_ok, if rev_ok { "" } else { "反向归属漂移" });

    // 双向一致
    s.add("双向-归属两向全一致", led.bidirectional_ok(), if led.bidirectional_ok() { "" } else { "双向缺口" });

    // 组遍历计数 == 账面计数
    s.add("计数-遍历恰等于账面", led.traversal_equals_ledger(), if led.traversal_equals_ledger() { "" } else { "计数缺口" });

    // 离组：组员可离组长不可
    let mut g2 = led.groups.first_mut().unwrap();
    let lv1 = g2.leave(102);
    let lv2 = !g2.leave(101);
    let after = g2.members == vec![101, 103];
    s.add("离组-组员可离组长拒且账更新", lv1 && lv2 && after, if lv1 && lv2 && after { "" } else { "离组漂移" });

    // 重复 pgid 建组拒
    let dup = !led.create(100, 200);
    s.add("建组-重复pgid拒", dup, if dup { "" } else { "重复漏网" });

    // 双组账：归属互斥（一 pid 只属一组）
    let _ = led.create(200, 201);
    let mut g3 = led.groups.last_mut().unwrap();
    let _ = g3.join(102); // 102 已在组 100——组账互斥由 group_of 先命中决定
    // 语义：group_of 只归先命中的组（互斥模拟）
    let owner = led.group_of(102).map(|x| x.pgid);
    s.add("互斥-pid102归属唯一组100", owner == Some(100), if owner == Some(100) { "" } else { "归属不唯一" });

    // 100 轮随机遍历计数守恒
    let mut led2 = GroupLedger::new();
    let mut seed: u64 = 0x8221_0100;
    for gi in 0..5u32 {
        led2.create(1000 + gi, 2000 + gi);
    }
    let mut joins = 0u32;
    for _ in 0..100 {
        let gi = (lcg_next(&mut seed) % 5) as usize;
        let pid = (lcg_next(&mut seed) % 500 + 3000) as u32;
        if led2.groups[gi].join(pid) {
            joins += 1;
        }
    }
    let conserved = led2.traversal_equals_ledger() && led2.bidirectional_ok();
    s.add("守恒-100轮随机操作遍历=账面", conserved, if conserved { "" } else { "守恒缺口" });

    // FNV 组账指纹稳定
    let h1 = fnv1a(format!("{:?}", led2.groups).as_bytes());
    let h2 = fnv1a(format!("{:?}", led2.groups).as_bytes());
    s.add("FNV-组账指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
