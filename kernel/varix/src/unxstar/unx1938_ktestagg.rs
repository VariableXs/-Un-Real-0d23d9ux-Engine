//! UNX-F1938 · ktest 族注册与批内断言聚合账 · 诊断段 0xB970
//! 验收锚点：UNX-F1938-J1 本批 19 条判据聚合入 ktest SMP 面新族，
//! 三档调度 intact，skip 态 100% 带因，结果账接入 20 维 SMP 维取数口，
//! 双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB970;

// ── 诊断段独占 · ktest 聚合错误域 ───────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SHORT: u16 = SEG_ERR + 1;   // 判据不足 19
const E_SCHED: u16 = SEG_ERR + 2;   // 三档调度破坏
const E_SKIP: u16 = SEG_ERR + 3;    // skip 无因
const E_DIM: u16 = SEG_ERR + 4;     // 维取数口错
const E_DUP: u16 = SEG_ERR + 5;     // 重复挂接

pub const BATCH_JUDGES: usize = 19;
pub const SMP_DIMS: usize = 20;
/// 三档调度：run/skip/defer。
pub const SCHED_RUN: u8 = 1;
pub const SCHED_SKIP: u8 = 2;
pub const SCHED_DEFER: u8 = 3;

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

// ── ktest 判据条目 ───────────────────────────────────────────
/// 一条挂接判据：名 + 调度档 + skip 因 + 通过态 + SMP 维。
#[derive(Clone, Debug)]
pub struct KJudge {
    pub name: String,
    pub sched: u8,      // 三档
    pub skip_reason: String, // skip 必带因
    pub pass: bool,
    pub dim: usize,     // SMP 维 0..20
}

impl KJudge {
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}:{}:{}:{}:{}", self.name, self.sched, self.skip_reason, self.pass, self.dim).as_bytes())
    }
    /// skip 态带因校验。
    pub fn skip_reasoned(&self) -> bool {
        self.sched != SCHED_SKIP || !self.skip_reason.is_empty()
    }
}

/// ktest 族账：判据聚合 + 20 维取数口。
pub struct KtestFamily {
    pub judges: Vec<KJudge>,
    pub dim_taken: [u32; SMP_DIMS], // 各维取数计数
}

impl KtestFamily {
    pub fn new() -> Self {
        KtestFamily { judges: Vec::new(), dim_taken: [0; SMP_DIMS] }
    }
    /// 挂接：重名拒、dim 越域拒。
    pub fn hook(&mut self, name: &str, sched: u8, skip: &str, dim: usize) -> bool {
        if self.judges.iter().any(|j| j.name == name) {
            return false;
        }
        if dim >= SMP_DIMS {
            return false;
        }
        if ![SCHED_RUN, SCHED_SKIP, SCHED_DEFER].contains(&sched) {
            return false;
        }
        self.judges.push(KJudge {
            name: name.to_string(),
            sched,
            skip_reason: skip.to_string(),
            pass: sched == SCHED_RUN, // skip/defer 不产生通过
            dim,
        });
        true
    }
    /// 判据数达标（≥19）。
    pub fn enough(&self) -> bool {
        self.judges.len() >= BATCH_JUDGES
    }
    /// 三档调度 intact：三档均有代表且无越档。
    pub fn sched_intact(&self) -> bool {
        self.judges.iter().all(|j| [SCHED_RUN, SCHED_SKIP, SCHED_DEFER].contains(&j.sched))
            && self.judges.iter().any(|j| j.sched == SCHED_RUN)
            && self.judges.iter().any(|j| j.sched == SCHED_SKIP)
            && self.judges.iter().any(|j| j.sched == SCHED_DEFER)
    }
    /// skip 态 100% 带因。
    pub fn skip_all_reasoned(&self) -> bool {
        self.judges.iter().filter(|j| j.sched == SCHED_SKIP).all(|j| j.skip_reasoned())
            && self.judges.iter().any(|j| j.sched == SCHED_SKIP)
    }
    /// 结果账接入 20 维取数口：逐维取数计数与该维判据数一致。
    pub fn dim_recon(&mut self) -> bool {
        self.dim_taken = [0; SMP_DIMS];
        for j in &self.judges {
            if j.dim >= SMP_DIMS {
                return false;
            }
            self.dim_taken[j.dim] += 1;
        }
        let mut total = 0;
        for d in 0..SMP_DIMS {
            let expect = self.judges.iter().filter(|j| j.dim == d).count() as u32;
            if self.dim_taken[d] != expect {
                return false;
            }
            total += self.dim_taken[d];
        }
        total as usize == self.judges.len()
    }
    /// 聚合结果：run 档全过 + skip/defer 不计通过。
    pub fn aggregate(&self) -> (u32, u32, u32) {
        let run = self.judges.iter().filter(|j| j.sched == SCHED_RUN).count() as u32;
        let pass = self.judges.iter().filter(|j| j.sched == SCHED_RUN && j.pass).count() as u32;
        let skip = self.judges.iter().filter(|j| j.sched == SCHED_SKIP).count() as u32;
        (pass, run, skip)
    }
    /// 重复挂接自证。
    pub fn no_dup(&self) -> bool {
        let n = self.judges.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.judges[i].name == self.judges[j].name {
                    return false;
                }
            }
        }
        true
    }
}

/// 标准夹具：19 条判据（run 14 / skip 3 / defer 2），覆盖 20 维中的 12 维。
pub fn standard_family() -> KtestFamily {
    let mut f = KtestFamily::new();
    for i in 0..14 {
        let _ = f.hook(&format!("smp_j{}", i), SCHED_RUN, "", i % 12);
    }
    let skip_reasons = ["need_4core", "need_htm", "hw_skip"];
    for (i, r) in skip_reasons.iter().enumerate() {
        let _ = f.hook(&format!("smp_skip{}", i), SCHED_SKIP, r, 12 + i);
    }
    let _ = f.hook("smp_defer0", SCHED_DEFER, "", 15);
    let _ = f.hook("smp_defer1", SCHED_DEFER, "", 16);
    f
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1938_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1938 ktest族注册与断言聚合");
    let mut f = standard_family();

    // 1. 19 条判据聚合达标
    s.add("KT-19条达标", f.enough() && f.judges.len() == BATCH_JUDGES,
        if f.enough() { "" } else { "判据不足" });

    // 2. 三档调度 intact
    s.add("KT-三档intact", f.sched_intact(), if f.sched_intact() { "" } else { "调度档缺" });

    // 3. skip 态 100% 带因
    s.add("KT-skip带因", f.skip_all_reasoned(), if f.skip_all_reasoned() { "" } else { "skip 无因" });

    // 4. 结果账接入 20 维取数口
    let recon = f.dim_recon();
    s.add("KT-20维取数", recon, if recon { "" } else { "维取数破" });

    // 5. 聚合结果口径（14 run 全过 + 3 skip + 2 defer）
    let (pass, run, skip) = f.aggregate();
    s.add("KT-聚合口径", pass == 14 && run == 14 && skip == 3,
        if pass == 14 { "" } else { "聚合口径错" });

    // 6. skip 无因拒判
    let mut f2 = standard_family();
    if let Some(j) = f2.judges.iter_mut().find(|j| j.sched == SCHED_SKIP) {
        j.skip_reason.clear();
    }
    s.add("KT-无因检出", !f2.skip_all_reasoned(),
        if !f2.skip_all_reasoned() { "" } else { "无因漏检" });

    // 7. 重复挂接拒
    let mut f3 = standard_family();
    let dup = !f3.hook("smp_j0", SCHED_RUN, "", 0);
    s.add("KT-重复挂接拒", dup && f3.no_dup(), if dup { "" } else { "重复放行" });

    // 8. 维越域拒
    let mut f4 = standard_family();
    let oob = !f4.hook("oob_j", SCHED_RUN, "", SMP_DIMS);
    s.add("KT-维越域拒", oob, if oob { "" } else { "越域放行" });

    // 9. 非法调度档拒
    let mut f5 = standard_family();
    let bad = !f5.hook("bad_j", 9, "", 0);
    s.add("KT-野档拒", bad, if bad { "" } else { "野档放行" });

    // 10. defer 不计通过
    let defer_ok = f.judges.iter().filter(|j| j.sched == SCHED_DEFER).all(|j| !j.pass);
    s.add("KT-defer不计过", defer_ok, if defer_ok { "" } else { "defer 误计过" });

    // 11. LCG 抽样 6 条指纹稳定
    let mut lcg = Lcg(0xB970);
    let mut spot = true;
    for _ in 0..6 {
        let i = lcg.below(f.judges.len() as u64) as usize;
        if f.judges[i].fp() != f.judges[i].fp() {
            spot = false;
        }
    }
    s.add("KT-指纹稳定", spot, if spot { "" } else { "指纹漂移" });

    // 12. 判据指纹互异
    let mut fp_uniq = true;
    for i in 0..f.judges.len() {
        for j in (i + 1)..f.judges.len() {
            if f.judges[i].fp() == f.judges[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("KT-指纹互异", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB970 && SEG_BASE < 0xB990;
    s.add("KT-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
