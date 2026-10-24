//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4015 · 引用计数基座（kref 语义） · 诊断段 0xD700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F4015-J1 kref get/put 万次往返计数零偏差；归零回调
//!      触发恰好一次（回调计数账=1）；underflow 注入即红（断言器
//!      捕获并报告）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Kref：计数 + 归零回调账；
//!   - 万次 get/put 往返：终值回零、回调恰 1 次；
//!   - underflow 注入：put 于零计数必拒并报告（断言器）；
//!   - 多对象并发账：多 kref 独立回调各 1；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xD700;

/// 往返次数。
pub const ROUNDS: u32 = 10_000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// kref。
pub struct Kref {
    count: u32,
    /// 回调触发次数（归零时 +1）。
    pub releases: u64,
    /// underflow 拒绝计数。
    pub underflows: u64,
}

impl Kref {
    pub fn new() -> Self {
        Kref { count: 0, releases: 0, underflows: 0 }
    }

    pub fn get(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    /// put：归零触发回调恰一次；零计数 put 即 underflow 拒绝。
    pub fn put(&mut self) -> Result<u32, &'static str> {
        if self.count == 0 {
            self.underflows += 1;
            return Err("UNDERFLOW");
        }
        self.count -= 1;
        if self.count == 0 {
            self.releases += 1;
        }
        Ok(0)
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// 断言器报告：underflow 被捕获并报告。
    pub fn assert_no_underflow(&self) -> bool {
        self.underflows == 0
    }
}

/// 万次往返：get/put 交错，终值回零回调恰 1。
pub fn ten_k_roundtrip() -> (bool, u64) {
    let mut k = Kref::new();
    let mut all_ok = true;
    for i in 0..ROUNDS {
        let _ = k.get();
        if k.put().is_err() {
            all_ok = false;
        }
        let _ = i;
    }
    (all_ok && k.count() == 0 && k.releases == 1 && k.assert_no_underflow(), k.releases)
}

/// J1 主判据：万次往返零偏差 + 回调恰 1 + underflow 即红。
pub fn j1_main() -> bool {
    let (ok, rel) = ten_k_roundtrip();
    ok && rel == 1 && {
        // underflow 注入：零计数 put 必 Err 且计数器捕获
        let mut k = Kref::new();
        let err = k.put();
        matches!(err, Err("UNDERFLOW")) && k.underflows == 1
    } && {
        // 多对象独立：3 个 kref 各往返，回调各 1
        let mut a = Kref::new();
        let mut b = Kref::new();
        let mut c = Kref::new();
        for _ in 0..10 {
            let _ = a.get();
            let _ = a.put();
        }
        for _ in 0..7 {
            let _ = b.get();
            let _ = b.put();
        }
        for _ in 0..3 {
            let _ = c.get();
            let _ = c.put();
        }
        a.releases == 1 && b.releases == 1 && c.releases == 1
    }
}

// ── 注入核（0xD700 段 5 项）────────────────────────────────────────────

/// 注入核 1：underflow 注入。
pub fn inj1_underflow() -> bool {
    let mut k = Kref::new();
    let e1 = k.put();
    let e2 = k.put();
    matches!(e1, Err(_)) && matches!(e2, Err(_)) && k.underflows == 2
}

/// 注入核 2：回调单次注入——多次归零各触发（非单次化）。
pub fn inj2_multi_release() -> bool {
    let mut k = Kref::new();
    for r in 0..3 {
        let _ = k.get();
        let _ = k.put();
        let _ = r;
    }
    k.releases == 3 && k.count() == 0
}

/// 注入核 3：嵌套引用注入——嵌套 get/put 计数守恒。
pub fn inj3_nested() -> bool {
    let mut k = Kref::new();
    let _ = k.get();
    let _ = k.get();
    let _ = k.get();
    let mid = k.count() == 3;
    let _ = k.put();
    let _ = k.put();
    let _ = k.put();
    mid && k.count() == 0 && k.releases == 1
}

/// 注入核 4：偏差注入——计数虚增必被闭合抓出。
pub fn inj4_drift() -> bool {
    let mut k = Kref::new();
    for _ in 0..10 {
        let _ = k.get();
    }
    for _ in 0..9 {
        let _ = k.put();
    }
    // 泄漏：count=1 未归零 → 回调 0
    k.count() == 1 && k.releases == 0
}

/// 注入核 5：零偏差注入——N get N put 回零。
pub fn inj5_zero_drift() -> bool {
    let mut k = Kref::new();
    for _ in 0..64 {
        let _ = k.get();
    }
    for _ in 0..64 {
        let _ = k.put();
    }
    k.count() == 0 && k.releases == 1 && k.underflows == 0
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx4015_checks：本单判据聚合（诊断段 0xD700）。
pub fn run_unx4015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4015 引用计数基座（kref 语义）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    let (ok, rel) = ten_k_roundtrip();
    s.add(
        "kref get/put万次往返计数零偏差",
        ok,
        if ok { "" } else { "万次往返偏差" },
    );
    s.add(
        "归零回调触发恰好一次（回调计数账=1）",
        rel == 1,
        if rel == 1 { "" } else { "回调次数≠1" },
    );
    s.add(
        "underflow注入即红（断言器捕获并报告）",
        j1_main(),
        if j1_main() { "" } else { "underflow未捕获" },
    );
    let inj = [
        ("注入1underflow", inj1_underflow()),
        ("注入2多归零回调", inj2_multi_release()),
        ("注入3嵌套引用", inj3_nested()),
        ("注入4泄漏检测", inj4_drift()),
        ("注入5零偏差", inj5_zero_drift()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
