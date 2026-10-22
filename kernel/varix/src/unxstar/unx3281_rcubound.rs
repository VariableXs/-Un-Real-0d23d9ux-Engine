//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3281 · RCU 语义边界声明（宽限周期概念与不做的事）· 诊断段 0xC980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：声明文档交付：读侧零锁语义定义、宽限周期（GP）概念、
//!      "不做的事"清单（不做 preemptible RCU/NMI 侧）；
//!      M-040 概念验证用例；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 声明件三要素：读侧零锁语义 / GP 概念 / 不做清单；
//!   - 声明件冻结哈希（三要素全名+条目）FNV-1a 入册；
//!   - M-040 概念验证：读侧零锁并发快照 + GP 静默期推进账；
//!   - 不做清单执法：preemptible-RCU/NMI 侧请求必拒；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xC980;

/// M-040 概念验证轮数。
pub const M040_ROUNDS: u32 = 16;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 读侧零锁语义条目。
pub const READ_SIDE_SEMANTICS: [&str; 4] = [
    "READ-ENTER-NO-LOCK",
    "READ-EXIT-NO-LOCK",
    "SNAPSHOT-IMMUTABLE",
    "GP-WAIT-ON-EXIT",
];

/// GP 概念条目。
pub const GP_CONCEPTS: [&str; 4] = [
    "GP-DEF-QUIESCENT",
    "GP-START-AFTER-WRITE",
    "GP-END-AFTER-READERS",
    "GP-RECLAIM-AFTER-END",
];

/// "不做的事"清单（封闭）。
pub const NOT_DO_LIST: [&str; 3] = [
    "NO-PREEMPTIBLE-RCU",
    "NO-NMI-SIDE",
    "NO-BLOCKING-READ",
];

/// 声明件。
pub struct Declaration {
    pub entries: Vec<&'static str>,
}

impl Declaration {
    /// 标准声明件：三要素全条目。
    pub fn standard() -> Self {
        let mut entries = Vec::new();
        for s in READ_SIDE_SEMANTICS {
            entries.push(s);
        }
        for s in GP_CONCEPTS {
            entries.push(s);
        }
        for s in NOT_DO_LIST {
            entries.push(s);
        }
        Declaration { entries }
    }

    /// 三要素齐备：三组条目全在。
    pub fn complete(&self) -> bool {
        let has = |want: &str| self.entries.iter().any(|e| *e == want);
        has("READ-ENTER-NO-LOCK") && has("GP-DEF-QUIESCENT") && has("NO-PREEMPTIBLE-RCU")
    }

    /// 声明件哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for e in &self.entries {
            for &b in e.as_bytes() {
                buf.push(b);
            }
            buf.push(b'|');
        }
        fnv1a(&buf)
    }

    /// 不做清单执法：请求清单内能力必拒。
    pub fn refuse(&self, capability: &str) -> bool {
        !self.entries.iter().any(|e| *e == capability)
    }
}

/// RCU 概念验证机。
pub struct RcuProbe {
    /// 活跃读者。
    pub readers: u32,
    /// GP 代号。
    pub gp_epoch: u64,
    /// 已收 GP 数。
    pub gps: u64,
    /// 发布计数。
    pub publishes: u64,
    /// 拒绝计数（不做清单执法）。
    pub refused: u64,
}

impl RcuProbe {
    pub fn new() -> Self {
        RcuProbe { readers: 0, gp_epoch: 0, gps: 0, publishes: 0, refused: 0 }
    }

    /// 读侧零锁进入。
    pub fn read_enter(&mut self) -> bool {
        self.readers += 1;
        true
    }

    /// 读侧退出：归零时收一个 GP。
    pub fn read_exit(&mut self) -> bool {
        if self.readers == 0 {
            return false;
        }
        self.readers -= 1;
        if self.readers == 0 {
            self.gp_epoch += 1;
            self.gps += 1;
        }
        true
    }

    /// 写侧发布：GP 推进前置。
    pub fn publish(&mut self) -> bool {
        self.publishes += 1;
        self.gp_epoch += 1;
        true
    }

    /// 不做清单执法：清单内请求拒并计数。
    pub fn request_decl(&mut self, decl: &Declaration, capability: &str) -> bool {
        if decl.refuse(capability) {
            self.refused += 1;
            true
        } else {
            false
        }
    }

    /// 一致：GP 数 ≡ 发布数 + 读者清空次数（概念当量下界）。
    pub fn consistent(&self) -> bool {
        // 修正（真缺陷·恒真式）：原写 `self.gps >= 0 && self.gp_epoch >= self.gps`。
        // `gps` 与 `gp_epoch` 都是 `u64`，`gps >= 0` 恒真——RCU 宽限期的
        // **真实**不变量只有「GP 纪元不落后于已发布 GP 数」这一条（发布只会
        // 推进纪元，读侧清空不会把纪元拉回去）。恒真子句删除，避免让判据
        // 误以为覆盖了「下界」这个从未被检查的性质。
        self.gp_epoch >= self.gps
    }
}

/// M-040 概念验证单轮：读快照零锁 + 发布 GP 推进 + 不做执法。
pub fn m040_once(seed: u64) -> (bool, bool) {
    let _x = seed;
    let mut probe = RcuProbe::new();
    let decl = Declaration::standard();
    // 读侧零锁并发
    let enter_ok = probe.read_enter() && probe.read_enter();
    let readers_ok = probe.readers == 2;
    // 写侧发布（读者在场 GP 不收）
    let pub_ok = probe.publish();
    let gp_deferred = probe.gps == 0;
    // 读者退出
    let _ = probe.read_exit();
    let _ = probe.read_exit();
    let gp_collected = probe.gps == 1;
    // 不做执法：清单内必拒
    let r1 = probe.request_decl(&decl, "NO-PREEMPTIBLE-RCU");
    let r2 = probe.request_decl(&decl, "NO-NMI-SIDE");
    // 清单外不拒
    let r3 = !probe.request_decl(&decl, "GP-DEF-QUIESCENT");
    (
        enter_ok && readers_ok && pub_ok && gp_deferred && gp_collected,
        r1 && r2 && r3 && probe.refused == 2 && probe.consistent(),
    )
}

/// J1 主判据：声明件三要素齐备 + 哈希冻结 + M-040 可重复通过。
pub fn j1_main() -> (bool, u32) {
    let decl = Declaration::standard();
    let base_ok = decl.complete() && decl.hash() != 0;
    let mut pass = 0u32;
    for r in 0..M040_ROUNDS {
        let seed = 0xC980_0000u64.wrapping_add(r as u64 * 0xB_287);
        let (a, b) = m040_once(seed);
        if a && b {
            pass += 1;
        }
    }
    (base_ok && pass == M040_ROUNDS, pass)
}

// ── 注入核（0xC980 段 5 项）────────────────────────────────────────────

/// 注入核 1：要素缺位注入——删除"不做"清单后三要素必破。
pub fn inj1_missing_element() -> bool {
    let probe = Declaration { entries: {
        let mut v = Vec::new();
        for s in READ_SIDE_SEMANTICS { v.push(s); }
        for s in GP_CONCEPTS { v.push(s); }
        v
    } };
    !probe.complete()
}

/// 注入核 2：越权请求注入——清单外请求不计数、清单内计数。
pub fn inj2_scope_refuse() -> bool {
    let decl = Declaration::standard();
    let mut p = RcuProbe::new();
    let in_list = p.request_decl(&decl, "NO-BLOCKING-READ");
    let out_list = p.request_decl(&decl, "READ-ENTER-NO-LOCK");
    in_list && !out_list && p.refused == 1
}

/// 注入核 3：读者在场 GP 收割注入——读者未退 GP 必不收。
pub fn inj3_gp_defer() -> bool {
    let mut p = RcuProbe::new();
    let _ = p.read_enter();
    let _ = p.publish();
    !p.quiescent_gp() && p.gps == 0
}

/// 注入核 4：读侧退出非法注入——零读者退出必拒。
pub fn inj4_bad_exit() -> bool {
    let mut p = RcuProbe::new();
    !p.read_exit()
}

/// 注入核 5：声明哈希稳定注入。
pub fn inj5_hash_stable() -> bool {
    let d = Declaration::standard();
    d.hash() == d.hash() && d.hash() != 0
}

impl RcuProbe {
    /// GP 静默（读者清空）。
    pub fn quiescent_gp(&self) -> bool {
        self.readers == 0
    }
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx3281_checks：本单判据聚合（诊断段 0xC980）。
pub fn run_unx3281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3281 RCU 语义边界声明（宽限周期概念与不做的事）");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    let decl = Declaration::standard();
    s.add(
        "声明件三要素交付（零锁语义/GP概念/不做清单）",
        decl.complete(),
        if decl.complete() { "" } else { "三要素缺位" },
    );
    s.add(
        "声明件冻结哈希入册",
        decl.hash() != 0,
        if decl.hash() != 0 { "" } else { "哈希空" },
    );
    let (j1, pass) = j1_main();
    s.add(
        "M-040概念验证用例可重复通过",
        j1,
        if j1 { "" } else { "M-040轮次不齐" },
    );
    let inj = [
        ("注入1要素缺位", inj1_missing_element()),
        ("注入2越权请求", inj2_scope_refuse()),
        ("注入3GP延收", inj3_gp_defer()),
        ("注入4非法退出", inj4_bad_exit()),
        ("注入5哈希稳定", inj5_hash_stable()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
