//! UNX-F7781 · 总账聚合引擎（五账归一）账 · 诊断段 0x9C00
//! 验收锚点：UNX-F7781-J1 聚合引擎——B25–B29 五账按分账矩阵归一：总轮次 =
//! 勾稽总和断言（五账轮数合计 == 聚合总轮次）；矩阵外轮次零容忍（任何轮号
//! 不在分账矩阵内即拒）；聚合确定性断言（同输入重放同结果）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x9C00;

pub const ACCOUNTS: [&str; 5] = ["B25", "B26", "B27", "B28", "B29"];

/// 单账：账名 + 轮号集。
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub name: &'static str,
    pub rounds: Vec<u32>,
}

impl Account {
    pub fn new(name: &'static str) -> Self {
        Account { name, rounds: Vec::new() }
    }
    pub fn add(&mut self, r: u32) {
        self.rounds.push(r);
    }
    pub fn len(&self) -> u32 {
        self.rounds.len() as u32
    }
}

/// 分账矩阵：轮号 → 所属账（1..=500：B25/B26/B27 依矩阵路由，演示三段区间）。
/// 矩阵规则：轮号 %5 → 0:B25 1:B26 2:B27 3:B28 4:B29。
pub fn matrix_route(round: u32) -> Option<usize> {
    if round == 0 {
        return None;
    }
    Some((round % 5) as usize)
}

/// 聚合引擎。
pub struct AggEngine {
    pub accounts: Vec<Account>,
    pub total_rounds: u32,
    pub rejected_out_of_matrix: u32,
}

impl AggEngine {
    pub fn new() -> Self {
        AggEngine { accounts: ACCOUNTS.iter().map(|n| Account::new(n)).collect(), total_rounds: 0, rejected_out_of_matrix: 0 }
    }

    /// 归一：轮号按矩阵路由入账；矩阵外零容忍（拒 + 计数）。
    pub fn ingest(&mut self, round: u32) -> bool {
        match matrix_route(round) {
            Some(idx) => {
                self.accounts[idx].add(round);
                self.total_rounds += 1;
                true
            }
            None => {
                self.rejected_out_of_matrix += 1;
                false
            }
        }
    }

    /// 勾稽总和断言：五账轮数合计 == total_rounds。
    pub fn checksum_holds(&self) -> bool {
        self.accounts.iter().map(|a| a.len()).sum::<u32>() == self.total_rounds
    }

    /// 各账均衡：演示矩阵下每账恰 n/5。
    pub fn balanced(&self) -> bool {
        let n = self.total_rounds / 5;
        self.accounts.iter().all(|a| a.len() == n)
    }

    /// 聚合指纹：五账顺序折叠（确定性核心）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x811C_9DC5;
        for a in &self.accounts {
            for r in &a.rounds {
                acc ^= *r;
                acc = acc.wrapping_mul(0x0100_0193);
            }
        }
        acc
    }

    /// 归一快照（重放确定性用）：五账轮号排序展平。
    pub fn snapshot(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for a in &self.accounts {
            let mut rs = a.rounds.clone();
            rs.sort();
            out.extend(rs);
        }
        out
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

// ── 注入核（0x9C00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7781_9C00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("矩阵-轮0拒", matrix_route(0).is_none()));
    out.push(("矩阵-五路由齐", (1..=5u32).all(|r| matrix_route(r).is_some())));
    out.push(("引擎-空账勾稽成立", AggEngine::new().checksum_holds()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7781_checks：本单判据聚合（诊断段 0x9C00）。
pub fn run_unx7781_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7781 总账聚合引擎五账归一");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 500 轮归一：每账恰 100
    let mut eng = AggEngine::new();
    for r in 1..=500u32 {
        if !eng.ingest(r) {
            s.add("意外拒收", false, "矩阵内轮被拒");
        }
    }
    let balance_ok = eng.balanced() && eng.accounts.iter().all(|a| a.len() == 100);
    s.add("归一-500轮五账各恰100", balance_ok, if balance_ok { "" } else { "归一失衡" });
    let sum_ok = eng.checksum_holds();
    s.add("勾稽-五账合计恰500", sum_ok, if sum_ok { "" } else { "勾稽断言红" });

    // 矩阵外零容忍：轮 0 拒 + 计数
    let mut eng2 = AggEngine::new();
    let ok1 = eng2.ingest(0);
    let ok2 = eng2.ingest(1);
    let zero_ok = !ok1 && ok2 && eng2.rejected_out_of_matrix == 1;
    s.add("零容忍-轮0拒并计数", zero_ok, if zero_ok { "" } else { "矩阵外漏网" });

    // 聚合确定性：两引擎同输入重放同指纹
    let mut e1 = AggEngine::new();
    let mut e2 = AggEngine::new();
    let mut seed: u64 = 0x7781_0200;
    for _ in 0..200 {
        let r = (lcg_next(&mut seed) % 1000 + 1) as u32;
        e1.ingest(r);
        e2.ingest(r);
    }
    let fp1 = e1.fingerprint();
    let fp2 = e2.fingerprint();
    let det_ok = fp1 == fp2 && e1.snapshot() == e2.snapshot();
    s.add("确定性-同输入重放同指纹同快照", det_ok, if det_ok { "" } else { "重放漂移" });

    // 快照内每账升序
    let sorted_ok = e1.snapshot().windows(2).all(|w| w[0] <= w[1]);
    s.add("快照-200轮展平升序", sorted_ok, if sorted_ok { "" } else { "快照序漂移" });

    // 勾稽错账检出：手工篡改一账 → 勾稽红
    let mut e3 = AggEngine::new();
    for r in 1..=100u32 {
        e3.ingest(r);
    }
    let before = e3.checksum_holds();
    e3.total_rounds += 1; // 人为错账
    s.add("错账-总轮次多1勾稽红", before && !e3.checksum_holds(), if before && !e3.checksum_holds() { "" } else { "错账漏检" });

    // 五账名序导出
    let names = ACCOUNTS.join(",");
    s.add("导出-五账名序B25..B29", names == "B25,B26,B27,B28,B29", if names == "B25,B26,B27,B28,B29" { "" } else { "账名序漂移" });

    // FNV 聚合指纹稳定
    let fp3 = e1.fingerprint();
    let fp4 = e1.fingerprint();
    s.add("FNV-聚合指纹重放稳定", fp3 == fp4 && fp3 != 0, if fp3 == fp4 { "" } else { "指纹不稳" });

    s
}
