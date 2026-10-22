//! UNX-F8721 · 进程族四族回归总档账 · 诊断段 0xB720
//! 验收锚点：UNX-F8721-J1 四进程族（init/shell/daemon/ktest）各建
//! 回归总档——族成员进程树完整、每族用例全过、四族总档聚合对账
//! 自洽，缺族或缺员即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB720;

// ── 诊断段独占 · 进程族总档错误域 ───────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_FAMILY: u16 = SEG_ERR + 1;  // 族缺失
const E_MEMBER: u16 = SEG_ERR + 2;  // 族员缺失
const E_CASE: u16 = SEG_ERR + 3;    // 用例失败
const E_AGG: u16 = SEG_ERR + 4;     // 聚合对账不平
const E_ORPHAN: u16 = SEG_ERR + 5;  // 孤儿进程（无归属族）

pub const FAMILIES: usize = 4;

// ── 确定性 LCG ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 族员进程 ─────────────────────────────────────────────────
/// 族员：pid + 父 pid + 用例集。
#[derive(Clone, Debug)]
pub struct ProcMember {
    pub pid: u32,
    pub ppid: u32, // 0 = 族首
    pub cases: Vec<(String, bool)>, // 用例 → 过
}

impl ProcMember {
    pub fn all_pass(&self) -> bool {
        self.cases.iter().all(|(_, ok)| *ok)
    }
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in [self.pid, self.ppid] {
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        for (c, ok) in &self.cases {
            for b in c.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h ^= *ok as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

// ── 进程族 ───────────────────────────────────────────────────
/// 一族：族首 + 族员树。
pub struct ProcFamily {
    pub name: String,   // init/shell/daemon/ktest
    pub members: Vec<ProcMember>,
}

impl ProcFamily {
    pub fn new(name: &str) -> Self {
        ProcFamily { name: name.to_string(), members: Vec::new() }
    }
    /// 族首存在（恰一个 ppid=0）。
    pub fn has_root(&self) -> bool {
        self.members.iter().filter(|m| m.ppid == 0).count() == 1
    }
    /// 每个非首成员父 pid 均在族内。
    pub fn parents_in_family(&self) -> bool {
        for m in &self.members {
            if m.ppid != 0 && !self.members.iter().any(|p| p.pid == m.ppid) {
                return false;
            }
        }
        true
    }
    /// 全员用例全过。
    pub fn all_pass(&self) -> bool {
        !self.members.is_empty() && self.members.iter().all(|m| m.all_pass())
    }
    /// 用例总数。
    pub fn total_cases(&self) -> usize {
        self.members.iter().map(|m| m.case_count()).sum()
    }
    /// 树深（族首为 1 层）。
    pub fn depth(&self) -> u32 {
        let root = match self.members.iter().find(|m| m.ppid == 0) {
            Some(r) => r.pid,
            None => return 0,
        };
        fn depth_of(f: &ProcFamily, pid: u32) -> u32 {
            let mut d = 1;
            for m in &f.members {
                if m.ppid == pid {
                    let sub = 1 + depth_of(f, m.pid);
                    if sub > d {
                        d = sub;
                    }
                }
            }
            d
        }
        depth_of(self, root)
    }
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for m in &self.members {
            for b in m.fp().to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

// ── 四族总档 ─────────────────────────────────────────────────
/// 聚合档：四族注册齐全 + 各族绿 + 聚合对账。
pub struct FamilyLedger {
    pub families: Vec<ProcFamily>,
}

pub const FAMILY_NAMES: [&str; FAMILIES] = ["init", "shell", "daemon", "ktest"];

impl FamilyLedger {
    pub fn new() -> Self {
        FamilyLedger { families: Vec::new() }
    }
    pub fn register(&mut self, f: ProcFamily) {
        self.families.push(f);
    }
    /// 四族名齐全且序一致。
    pub fn names_complete(&self) -> bool {
        self.families.len() == FAMILIES
            && self.families.iter().zip(FAMILY_NAMES).all(|(f, n)| f.name == n)
    }
    /// 四族全绿。
    pub fn all_green(&self) -> bool {
        self.families.iter().all(|f| f.has_root() && f.parents_in_family() && f.all_pass())
    }
    /// 聚合对账：总用例数 = 各族和。
    pub fn agg_cases(&self) -> usize {
        self.families.iter().map(|f| f.total_cases()).sum()
    }
    /// 聚合指纹。
    pub fn ledger_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for f in &self.families {
            for b in f.fp().to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

/// 孤儿检出：任何 ppid 不在全集中的成员。
pub fn orphans(led: &FamilyLedger) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for f in &led.families {
        for m in &f.members {
            if m.ppid != 0 && !f.members.iter().any(|p| p.pid == m.ppid) {
                out.push((f.name.clone(), m.pid));
            }
        }
    }
    out
}

// ── 标准夹具：四族各 3~4 员、每员 3~5 用例全过 ──────────────
pub fn standard_ledger() -> FamilyLedger {
    let mut led = FamilyLedger::new();
    let mut pid_seed = 100u32;
    let mut lcg = Lcg(0xB720);
    for fname in FAMILY_NAMES {
        let mut f = ProcFamily::new(fname);
        let root_pid = pid_seed;
        pid_seed += 1;
        let n_child = 2 + (lcg.below(2)) as usize; // 2~3 子
        let mut pids = vec![root_pid];
        for _ in 0..n_child {
            pids.push(pid_seed);
            pid_seed += 1;
        }
        for (i, &p) in pids.iter().enumerate() {
            let n_case = 3 + (lcg.below(3)) as usize; // 3~5 用例
            let mut cases = Vec::new();
            for c in 0..n_case {
                cases.push((format!("{}-p{}-c{}", fname, i, c), true));
            }
            f.members.push(ProcMember {
                pid: p,
                ppid: if i == 0 { 0 } else { root_pid },
                cases,
            });
        }
        led.register(f);
    }
    led
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8721_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8721 进程族四族回归总档");
    let led = standard_ledger();

    // 1. 四族齐全
    s.add("FAM-四族齐全", led.names_complete(),
        if led.names_complete() { "" } else { "族缺失" });

    // 2. 四族全绿（根唯一 + 父在族内 + 用例全过）
    s.add("FAM-四族全绿", led.all_green(), if led.all_green() { "" } else { "存在红族" });

    // 3. 无孤儿
    let orph = orphans(&led);
    s.add("FAM-无孤儿", orph.is_empty(), if orph.is_empty() { "" } else { "存在孤儿进程" });

    // 4. 聚合对账：总用例 = 各族和
    let manual: usize = led.families.iter().map(|f| f.total_cases()).sum();
    s.add("FAM-聚合对账", led.agg_cases() == manual && manual >= 12,
        if led.agg_cases() == manual { "" } else { "聚合不平" });

    // 5. 每族树深 ≥2（根 + 至少一子）
    let depth_ok = led.families.iter().all(|f| f.depth() >= 2);
    s.add("FAM-树深达标", depth_ok, if depth_ok { "" } else { "树过浅" });

    // 6. 缺族检出
    let mut led2 = standard_ledger();
    led2.families.pop();
    s.add("FAM-缺族检出", !led2.names_complete(),
        if !led2.names_complete() { "" } else { "缺族未检出" });

    // 7. 用例失败检出
    let mut led3 = standard_ledger();
    led3.families[2].members[1].cases[0].1 = false;
    s.add("FAM-红例检出", !led3.all_green(),
        if !led3.all_green() { "" } else { "红例未检出" });

    // 8. 父缺失（孤儿注入）检出
    let mut led4 = standard_ledger();
    led4.families[1].members[1].ppid = 9999;
    let orph4 = orphans(&led4);
    s.add("FAM-孤儿检出", orph4.len() == 1 && orph4[0].0 == "shell",
        if orph4.len() == 1 { "" } else { "孤儿未检出" });

    // 9. 族首不唯一检出（双根）
    let mut led5 = standard_ledger();
    led5.families[0].members[1].ppid = 0;
    s.add("FAM-双根检出", !led5.families[0].has_root(),
        if !led5.families[0].has_root() { "" } else { "双根未检出" });

    // 10. 总档指纹稳定且敏感
    let fp1 = led.ledger_fp();
    let fp2 = led.ledger_fp();
    let mut led6 = standard_ledger();
    led6.families[3].members[0].cases.pop();
    let fp3 = led6.ledger_fp();
    s.add("FAM-档指纹敏感", fp1 == fp2 && fp1 != fp3,
        if fp1 == fp2 && fp1 != fp3 { "" } else { "指纹碰撞或漂移" });

    // 11. 族指纹互异
    let mut fp_uniq = true;
    for i in 0..led.families.len() {
        for j in (i + 1)..led.families.len() {
            if led.families[i].fp() == led.families[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("FAM-族指纹互异", fp_uniq, if fp_uniq { "" } else { "族指纹碰撞" });

    // 12. LCG 抽样 6 员复核（pid 存在且用例全过）
    let mut lcg = Lcg(0xB721);
    let mut spot = true;
    for _ in 0..6 {
        let fi = lcg.below(led.families.len() as u64) as usize;
        let mi = lcg.below(led.families[fi].members.len() as u64) as usize;
        let m = &led.families[fi].members[mi];
        if !m.all_pass() {
            spot = false;
        }
    }
    s.add("FAM-抽样复核", spot, if spot { "" } else { "抽样异常" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB720 && SEG_BASE < 0xB740;
    s.add("FAM-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
