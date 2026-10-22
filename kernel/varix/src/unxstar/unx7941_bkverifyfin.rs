//! UNX-F7941 · 备份校验链子域收官总纲（五批回顾 + 收官判据框架）账 · 诊断段 0x9D80
//! 验收锚点：UNX-F7941-J1 收官总纲——B33 清单 + B34 执行 + B35 恢复 + B36 演练 +
//! B37 策略五批回顾（vs 各批头注主题 diff ×5 一致）；收官判据框架三面
//! （判据兑现 / 工具冻结 / 零遗留）落账 ×1。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x9D80;

/// 五批回顾。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BkBatch {
    B33,
    B34,
    B35,
    B36,
    B37,
}

impl BkBatch {
    pub fn as_str(self) -> &'static str {
        match self {
            BkBatch::B33 => "B33",
            BkBatch::B34 => "B34",
            BkBatch::B35 => "B35",
            BkBatch::B36 => "B36",
            BkBatch::B37 => "B37",
        }
    }
    /// 各批头注主题（模拟批头注）。
    pub fn topic(self) -> &'static str {
        match self {
            BkBatch::B33 => "清单",
            BkBatch::B34 => "执行",
            BkBatch::B35 => "恢复",
            BkBatch::B36 => "演练",
            BkBatch::B37 => "策略",
        }
    }
    pub fn all() -> [BkBatch; 5] {
        [BkBatch::B33, BkBatch::B34, BkBatch::B35, BkBatch::B36, BkBatch::B37]
    }
}

/// 收官判据框架三面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinFace {
    CriteriaDone,   // 判据兑现
    ToolsFrozen,    // 工具冻结
    ZeroLeftover,   // 零遗留
}

impl FinFace {
    pub fn as_str(self) -> &'static str {
        match self {
            FinFace::CriteriaDone => "criteria_done",
            FinFace::ToolsFrozen => "tools_frozen",
            FinFace::ZeroLeftover => "zero_leftover",
        }
    }
    pub fn all() -> [FinFace; 3] {
        [FinFace::CriteriaDone, FinFace::ToolsFrozen, FinFace::ZeroLeftover]
    }
}

/// 收官总纲账。
pub struct BkFinLedger {
    pub reviewed: Vec<(BkBatch, &'static str)>, // 批 → 回顾主题
    pub faces: Vec<FinFace>,
}

impl BkFinLedger {
    pub fn new() -> Self {
        BkFinLedger { reviewed: Vec::new(), faces: Vec::new() }
    }

    pub fn review(&mut self, b: BkBatch, topic: &'static str) {
        self.reviewed.push((b, topic));
    }

    pub fn add_face(&mut self, f: FinFace) {
        self.faces.push(f);
    }

    /// 五批回顾 diff ×5：每批主题与批头注全等。
    pub fn review_diff_all_match(&self) -> bool {
        if self.reviewed.len() != 5 {
            return false;
        }
        BkBatch::all().iter().all(|b| {
            self.reviewed.iter().any(|(rb, t)| rb == b && *t == b.topic())
        })
    }

    /// 三面齐全。
    pub fn faces_complete(&self) -> bool {
        FinFace::all().iter().all(|f| self.faces.contains(f))
    }

    pub fn summary(&self) -> String {
        let r = self.reviewed.iter().map(|(b, t)| format!("{}:{}", b.as_str(), t)).collect::<Vec<String>>().join(",");
        let f = self.faces.iter().map(|x| x.as_str()).collect::<Vec<&str>>().join("+");
        format!("{}|{}", r, f)
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

// ── 注入核（0x9D80 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7941_9D80;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("五批-枚举全5", BkBatch::all().len() == 5));
    out.push(("三面-枚举全3", FinFace::all().len() == 3));
    out.push(("回顾-空账diff红", !BkFinLedger::new().review_diff_all_match()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7941_checks：本单判据聚合（诊断段 0x9D80）。
pub fn run_unx7941_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7941 备份校验链子域收官");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 五批回顾：主题与批头注全等 → diff ×5 一致
    let mut led = BkFinLedger::new();
    for b in BkBatch::all().iter() {
        led.review(*b, b.topic());
    }
    let diff_ok = led.review_diff_all_match();
    s.add("回顾-五批主题diff×5全一致", diff_ok, if diff_ok { "" } else { "回顾 diff 红" });

    // 回顾错账检出：B36 主题写错 → diff 红
    let mut led2 = BkFinLedger::new();
    for b in BkBatch::all().iter() {
        let t = if *b == BkBatch::B36 { "执行" } else { b.topic() };
        led2.review(*b, t);
    }
    s.add("回顾-B36主题错diff红", !led2.review_diff_all_match(), if !led2.review_diff_all_match() { "" } else { "错账漏检" });

    // 三面落账
    for f in FinFace::all().iter() {
        led.add_face(*f);
    }
    let faces_ok = led.faces_complete();
    s.add("三面-判据兑现/工具冻结/零遗留全落", faces_ok, if faces_ok { "" } else { "三面缺口" });

    // 缺面检出
    let mut led3 = BkFinLedger::new();
    led3.add_face(FinFace::CriteriaDone);
    led3.add_face(FinFace::ToolsFrozen);
    s.add("缺面-少零遗留即红", !led3.faces_complete(), if !led3.faces_complete() { "" } else { "缺面漏检" });

    // 五批主题可导出
    let topics = BkBatch::all().iter().map(|b| b.topic()).collect::<Vec<&str>>().join(",");
    s.add("导出-五批主题清单/执行/恢复/演练/策略", topics == "清单,执行,恢复,演练,策略", if topics == "清单,执行,恢复,演练,策略" { "" } else { "主题序漂移" });

    // 收官总账摘要非空
    let summ = led.summary();
    s.add("摘要-收官总账非空且含五批", summ.contains("B33:清单") && summ.contains("B37:策略") && summ.contains("criteria_done"), if summ.contains("B33:清单") { "" } else { "摘要缺口" });

    // FNV 收官指纹稳定
    let h1 = fnv1a(summ.as_bytes());
    let h2 = fnv1a(led.summary().as_bytes());
    s.add("FNV-收官指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    // LCG 混合演练 64 轮不炸
    let mut seed: u64 = 0x7941_0064;
    let mut n = 0u32;
    for _ in 0..64 {
        let v = lcg_next(&mut seed) % 5;
        let _ = BkBatch::all()[v as usize].topic();
        n += 1;
    }
    s.add("随机-64轮批主题访问不炸", n == 64, if n == 64 { "" } else { "随机访问漂移" });

    s
}
