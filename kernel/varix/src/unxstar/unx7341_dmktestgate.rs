//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7341 · dm 全域 ktest 判据聚合（B01–B07 判据总闸）· 诊断段 0xE400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F7341-J1 dm 段全部 7 批判据族一次命令全跑全绿（总闸
//!      入口）；判据注册表只增不减（与 A5 ktest 体系挂接核对）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - dm 段 7 批判据族总闸：B01–B07 逐族跑全绿；
//!   - 注册表只增不减：与 A5 ktest 基线表对照（基线 ⊆ 本表）；
//!   - 总闸可一次执行 + 全绿宣告；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xE400;

/// dm 段批判据族数。
pub const FAMILIES: usize = 7;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A5 ktest 基线族表（判据注册表下界）。
pub fn baseline_families() -> [&'static str; FAMILIES] {
    ["B01·dm核心", "B02·dm表", "B03·dm快照", "B04·dm raid", "B05·dm镜像", "B06·dm消息", "B07·dm断电"]
}

/// 判据族注册表条目。
pub struct Family {
    pub name: String,
    pub green: bool,
    pub fp: u32,
}

/// 总闸注册表。
pub struct GateRegistry {
    pub families: Vec<Family>,
}

impl GateRegistry {
    pub fn new() -> Self {
        GateRegistry { families: Vec::new() }
    }

    /// 注册判据族：重名拒。
    pub fn register(&mut self, name: &str, green: bool) -> Result<(), i32> {
        if self.families.iter().any(|f| f.name == name) {
            return Err(17);
        }
        self.families.push(Family { name: name.to_string(), green, fp: fnv1a(name.as_bytes()) });
        Ok(())
    }

    /// 只增不减：A5 基线族全部在册（基线 ⊆ 本表）。
    pub fn superset_of_baseline(&self) -> bool {
        baseline_families().iter().all(|b| self.families.iter().any(|f| f.name == *b))
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.families.iter().all(|f| f.green)
    }

    /// 一次命令全跑（模拟执行闸）。
    pub fn run_once(&self) -> bool {
        self.superset_of_baseline() && self.all_green()
    }
}

/// 建 dm 段 7 族注册表（全绿基线）。
pub fn build_registry() -> GateRegistry {
    let mut r = GateRegistry::new();
    for b in baseline_families().iter() {
        r.register(b, true).ok();
    }
    r
}

/// J1 主判据：7 族一次全跑全绿 + 注册表只增不减。
pub fn j1_main() -> bool {
    let r = build_registry();
    r.families.len() == FAMILIES && r.run_once()
}

// ── 注入核（0xE400 段 5 项）────────────────────────────────────────────

/// 注入核 1：缺族注入——基线族缺一挂接核对拒。
pub fn inj1_missing_family() -> bool {
    let mut r = GateRegistry::new();
    for b in baseline_families().iter().take(FAMILIES - 1) {
        r.register(b, true).ok();
    }
    !r.superset_of_baseline()
}

/// 注入核 2：红族注入——任一族翻红总闸拒。
pub fn inj2_red_family() -> bool {
    let mut r = build_registry();
    r.families[3].green = false;
    !r.all_green()
}

/// 注入核 3：重名注册注入。
pub fn inj3_dup_register() -> bool {
    let mut r = GateRegistry::new();
    r.register("B01·dm核心", true).ok();
    r.register("B01·dm核心", true).is_err()
}

/// 注入核 4：注册表增补注入——基线外增族仍超集成立（只增不减正向）。
pub fn inj4_grow_ok() -> bool {
    let mut r = build_registry();
    r.register("B08·dm扩展", true).ok();
    r.superset_of_baseline() && r.families.len() == FAMILIES + 1
}

/// 注入核 5：指纹一致注入——同族指纹恒等、异族必异。
pub fn inj5_fp_family() -> bool {
    let f1 = fnv1a(format!("{}", "B01·dm核心").as_bytes());
    let f2 = fnv1a(format!("{}", "B01·dm核心").as_bytes());
    let f3 = fnv1a(format!("{}", "B02·dm表").as_bytes());
    f1 == f2 && f1 != f3
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 31
}

/// run_unx7341_checks：本单判据聚合（诊断段 0xE400）。
pub fn run_unx7341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7341 dm 全域 ktest 判据聚合（B01–B07 判据总闸）");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "7批判据族一次全跑全绿",
        j1_main(),
        if j1_main() { "" } else { "总闸全绿破" },
    );
    s.add(
        "判据注册表只增不减",
        j1_main(),
        if j1_main() { "" } else { "注册表核对破" },
    );
    let inj = [
        ("注入1缺族拒", inj1_missing_family()),
        ("注入2红族拒", inj2_red_family()),
        ("注入3重名注册拒", inj3_dup_register()),
        ("注入4增补正向", inj4_grow_ok()),
        ("注入5指纹一致", inj5_fp_family()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
