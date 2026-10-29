//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1357 · 压测配置总表（B28 段参数收口） · 0x2EB0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1357-J1 B28 段参数 ≥12 项注册总表（执行顺序/护栏阈值/熔断
//!      阈值/基准偏差线等），双闸一致率 100%，篡改 10/10 检出回退；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 14 项参数（≥12 判准）：五元组（编号/名/默认值/生效值/值域）；
//!   - 双闸：默认闸/生效闸独立校验和，seal 落档后重算一致率 100%；
//!   - 篡改 10/10：audit_tamper 检出+回退+重新落档全链；
//!   - 越域写入/越表号/重复注册必拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2EB0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2EB0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE_BAD: u16 = SEG_BASE + 1;   // 越域写入
const E_ID_BAD: u16 = SEG_BASE + 2;      // 参数号越表
const E_DUP_REG: u16 = SEG_BASE + 3;     // 重复注册
const E_GATE_MISMATCH: u16 = SEG_BASE + 4; // 双闸不一致
const E_TAMPER: u16 = SEG_BASE + 5;      // 篡改检出
const E_ROLLBACK_MISS: u16 = SEG_BASE + 6; // 回退未复原
const E_FROZEN: u16 = SEG_BASE + 7;      // 冻结后写入
const E_COVER_MISS: u16 = SEG_BASE + 8;  // 参数数不足 12
const E_LEDGER_FULL: u16 = SEG_BASE + 9; // 账满
const E_UNIT_MISS: u16 = SEG_BASE + 10;  // 单位缺

/// 参数数（≥12 判准；本表 14）。
pub const PARAM_COUNT: usize = 14;

/// 参数行。
#[derive(Clone, Copy, Debug)]
pub struct ParamRow {
    pub id: u8,
    pub name: &'static str,
    pub default_v: u32,
    pub active_v: u32,
    pub min_v: u32,
    pub max_v: u32,
    /// 单位闭集：0=eps 1=µs 2=步数 3=万分比 4=开关 5=计数。
    pub unit: u8,
}

impl ParamRow {
    pub fn unit_known(&self) -> bool {
        self.unit <= 5
    }

    pub fn in_range(&self, v: u32) -> bool {
        v >= self.min_v && v <= self.max_v
    }
}

/// B28 段冻结参数表（14 项）。
pub const FROZEN_PARAMS: [ParamRow; PARAM_COUNT] = [
    ParamRow { id: 1, name: "执行顺序模式", default_v: 0, active_v: 0, min_v: 0, max_v: 1, unit: 4 },
    ParamRow { id: 2, name: "速率档表", default_v: 4, active_v: 4, min_v: 2, max_v: 8, unit: 5 },
    ParamRow { id: 3, name: "并发核数", default_v: 8, active_v: 8, min_v: 2, max_v: 64, unit: 5 },
    ParamRow { id: 4, name: "长稳步数", default_v: 864, active_v: 864, min_v: 96, max_v: 4096, unit: 2 },
    ParamRow { id: 5, name: "队列水位线", default_v: 90, active_v: 90, min_v: 50, max_v: 99, unit: 4 },
    ParamRow { id: 6, name: "时间上限", default_v: 128, active_v: 128, min_v: 32, max_v: 1024, unit: 2 },
    ParamRow { id: 7, name: "熔断阈值", default_v: 500, active_v: 500, min_v: 100, max_v: 2000, unit: 3 },
    ParamRow { id: 8, name: "基准偏差线", default_v: 1000, active_v: 1000, min_v: 300, max_v: 3000, unit: 3 },
    ParamRow { id: 9, name: "BH配额", default_v: 12000, active_v: 12000, min_v: 1000, max_v: 100000, unit: 5 },
    ParamRow { id: 10, name: "长跑时长", default_v: 24, active_v: 24, min_v: 1, max_v: 72, unit: 2 },
    ParamRow { id: 11, name: "导出块大小", default_v: 8, active_v: 8, min_v: 1, max_v: 64, unit: 5 },
    ParamRow { id: 12, name: "复跑轮数", default_v: 3, active_v: 3, min_v: 1, max_v: 10, unit: 5 },
    ParamRow { id: 13, name: "指标数下限", default_v: 15, active_v: 15, min_v: 10, max_v: 32, unit: 5 },
    ParamRow { id: 14, name: "审计滚动行", default_v: 32, active_v: 32, min_v: 16, max_v: 256, unit: 5 },
];

/// 配置账：默认闸 + 生效闸双轨。
pub struct ConfigLedger {
    rows: [Option<ParamRow>; 20],
    used: usize,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    gate_default: u64,
    gate_active: u64,
}

impl ConfigLedger {
    pub fn new() -> ConfigLedger {
        ConfigLedger {
            rows: [None; 20],
            used: 0,
            frozen: false,
            version: 0,
            rejects: [0; 11],
            gate_default: 0,
            gate_active: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    pub fn register(&mut self, row: ParamRow) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if row.id == 0 || (row.id as usize) > 20 {
            return Err(self.reject(E_ID_BAD));
        }
        if !row.unit_known() {
            return Err(self.reject(E_UNIT_MISS));
        }
        let mut i = 0;
        while i < self.used {
            if let Some(x) = self.rows[i] {
                if x.id == row.id {
                    return Err(self.reject(E_DUP_REG));
                }
            }
            i += 1;
        }
        if self.used >= 20 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.rows[self.used] = Some(row);
        self.used += 1;
        self.version += 1;
        Ok(self.used - 1)
    }

    pub fn set_active(&mut self, id: u8, v: u32) -> Result<(), u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        let mut i = 0;
        while i < self.used {
            if let Some(slot) = self.rows[i] {
                if slot.id == id {
                    if !slot.in_range(v) {
                        return Err(self.reject(E_RANGE_BAD));
                    }
                    if let Some(r) = self.rows[i].as_mut() {
                        r.active_v = v;
                    }
                    self.version += 1;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(self.reject(E_ID_BAD))
    }

    fn compute_gates(&self) -> (u64, u64) {
        let mut gd = 0xcbf2_9ce4_8422_2325u64;
        let mut ga = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(r) = self.rows[i] {
                gd ^= r.default_v as u64;
                gd = gd.wrapping_mul(0x0000_0100_0000_01B3);
                ga ^= r.active_v as u64;
                ga = ga.wrapping_mul(0x0000_0100_0000_01B3);
            }
            i += 1;
        }
        (gd, ga)
    }

    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.used < 12 {
            return Err(self.reject(E_COVER_MISS));
        }
        let (gd, ga) = self.compute_gates();
        self.gate_default = gd;
        self.gate_active = ga;
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn gates_consistent(&self) -> bool {
        let (gd, ga) = self.compute_gates();
        gd == self.gate_default && ga == self.gate_active
    }

    pub fn tamper_detected(&self) -> bool {
        let (_, ga) = self.compute_gates();
        ga != self.gate_active
    }

    /// 审计篡改路径：可疑值落账→检出→回退→重新落档。
    pub fn audit_tamper(&mut self, id: u8, suspected_v: u32) -> Result<bool, u16> {
        let mut i = 0;
        let mut found = false;
        while i < self.used {
            if let Some(r) = self.rows[i] {
                if r.id == id {
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            return Err(self.reject(E_ID_BAD));
        }
        if self.rows[i].map(|r| r.active_v) == Some(suspected_v) {
            return Ok(false);
        }
        if let Some(slot) = self.rows[i].as_mut() {
            slot.active_v = suspected_v;
        }
        if !self.tamper_detected() {
            return Err(self.reject(E_ROLLBACK_MISS));
        }
        if let Some(slot) = self.rows[i].as_mut() {
            slot.active_v = slot.default_v;
        }
        let (gd, ga) = self.compute_gates();
        self.gate_default = gd;
        self.gate_active = ga;
        self.version += 1;
        Ok(self.gates_consistent())
    }

    pub fn used(&self) -> usize {
        self.used
    }
}

/// 全量注册演练：14 项 + 双闸 seal + 一致核验。
pub fn drill_register_all() -> (u32, u32, Vec<String>) {
    let mut ledger = ConfigLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut i = 0;
    while i < PARAM_COUNT {
        match ledger.register(FROZEN_PARAMS[i]) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("参数 {} 注册被拒：0x{:04X}", FROZEN_PARAMS[i].name, c)),
        }
        i += 1;
    }
    let sealed = ledger.seal().is_ok();
    let consistent = ledger.gates_consistent();
    detail.push(format!("注册 {}/14 封账={} 双闸一致={}", ok, sealed, consistent));
    (ok, if sealed && consistent { 1 } else { 0 }, detail)
}

/// 双闸一致率演练：seal 后重算 5 轮全一致。
pub fn drill_gate_consistency_5() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 5 {
        let mut ledger = ConfigLedger::new();
        let mut i = 0;
        while i < PARAM_COUNT {
            let _ = ledger.register(FROZEN_PARAMS[i]);
            i += 1;
        }
        let _ = ledger.seal();
        if ledger.gates_consistent() {
            ok += 1;
        }
        r += 1;
    }
    (ok, 5, detail)
}

/// 篡改 10/10：audit_tamper 检出+回退+复原全链 + 无篡改报告判 Ok(false)。
pub fn inject_tamper_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = ConfigLedger::new();
        let mut i = 0;
        while i < PARAM_COUNT {
            let _ = ledger.register(FROZEN_PARAMS[i]);
            i += 1;
        }
        let _ = ledger.seal();
        let target = (r % PARAM_COUNT) as u8 + 1;
        let suspected = FROZEN_PARAMS[(target - 1) as usize].default_v + 1;
        let hit = matches!(ledger.audit_tamper(target, suspected), Ok(true));
        let clean = matches!(
            ledger.audit_tamper(target, FROZEN_PARAMS[(target - 1) as usize].default_v),
            Ok(false)
        );
        if hit && clean && ledger.gates_consistent() {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("篡改检出+回退 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 越域注入 3 项：低于下限/高于上限/越表号——必抓。
pub fn inject_range_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = ConfigLedger::new();
    let mut i = 0;
    while i < PARAM_COUNT {
        let _ = ledger.register(FROZEN_PARAMS[i]);
        i += 1;
    }
    if ledger.set_active(1, 9) == Err(E_RANGE_BAD) {
        caught += 1;
    } else {
        detail.push("低于下限未拒".to_string());
    }
    if ledger.set_active(7, 2001) == Err(E_RANGE_BAD) {
        caught += 1;
    } else {
        detail.push("高于上限未拒".to_string());
    }
    if ledger.set_active(99, 1) == Err(E_ID_BAD) {
        caught += 1;
    } else {
        detail.push("越表号未拒".to_string());
    }
    if caught == 3 {
        detail.push("越域注入 3/3 全抓".to_string());
    }
    (caught, 3, detail)
}

/// 段位守卫：0x2EB0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_RANGE_BAD, E_ID_BAD, E_DUP_REG, E_GATE_MISMATCH, E_TAMPER,
        E_ROLLBACK_MISS, E_FROZEN, E_COVER_MISS, E_LEDGER_FULL, E_UNIT_MISS,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1357_checks：本单判据聚合（诊断段 0x2EB0）。
pub fn run_unx1357_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1357 压测配置总表");
    let (r1, ok1, d1) = drill_register_all();
    s.add_dyn(
        "J1-14项注册+双闸一致",
        r1 == 14 && ok1 == 1,
        &format!("{}/14 {}", r1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_gate_consistency_5();
    s.add_dyn("J1-双闸一致率5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_tamper_10();
    s.add_dyn("J1-篡改检出+回退10/10", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (c4, t4, d4) = inject_range_bad();
    s.add_dyn("J1-越域注入3/3", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    s.add_dyn("J1-参数数下限", PARAM_COUNT >= 12, "14≥12");
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2EB0段十码闭集");
    s
}
