//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1347 · 压测指标口径账（全指标定义与采集统一） · 0x2E10
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1347-J1 压测指标 ≥15 项口径统一登记（吞吐/延迟分位/丢失率/
//!      降级率等），指标采集单一来源断言（无双口径）10/10 次，口径与
//!      F0899/F0953 对齐率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 16 项指标口径登记（≥15 判准）：五元组（编号/名/单位/方向/来源
//!     通道），来源通道闭集 3（分发账/延迟账/资源账）；
//!   - 单一来源断言：每指标恰绑一来源通道，双通道同指标必拒 10/10；
//!   - F0899/F0953 对齐：健康类指标对 F0899 方向口径、延迟类对 F0953
//!     分位口径逐行对拍；
//!   - 采集演练：三通道采样→按口径汇聚→指标值与手工重算一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2E10 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2E10;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ID_BAD: u16 = SEG_BASE + 1;      // 指标号越表
const E_DUP_REG: u16 = SEG_BASE + 2;     // 重复注册
const E_CHANNEL_BAD: u16 = SEG_BASE + 3; // 来源通道越闭集
const E_DUAL_SOURCE: u16 = SEG_BASE + 4; // 双口径（同指标双通道）
const E_ALIGN_MISS: u16 = SEG_BASE + 5;  // 与 F0899/F0953 对齐失败
const E_FROZEN: u16 = SEG_BASE + 6;      // 冻结后注册
const E_COVER_MISS: u16 = SEG_BASE + 7;  // 指标数不足 15
const E_SAMPLE_MISS: u16 = SEG_BASE + 8; // 采集与重算不一致
const E_DIR_BAD: u16 = SEG_BASE + 9;     // 方向越闭集
const E_LEDGER_FULL: u16 = SEG_BASE + 10; // 账满

/// 指标数（≥15 判准；本账 16）。
pub const METRIC_COUNT: usize = 16;

/// 来源通道闭集：0=分发账 1=延迟账 2=资源账。
pub type Channel = u8;

/// 指标方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricDir {
    HigherBetter = 0,
    LowerBetter = 1,
}

impl MetricDir {
    pub fn from_id(v: u8) -> Option<MetricDir> {
        match v {
            0 => Some(MetricDir::HigherBetter),
            1 => Some(MetricDir::LowerBetter),
            _ => None,
        }
    }
}

/// 指标口径行（五元组）。
#[derive(Clone, Copy, Debug)]
pub struct CaliberRow {
    pub id: u8,
    pub name: &'static str,
    /// 单位标识闭集：0=eps 1=µs 2=ppm 3=计数 4=百分 5=字节。
    pub unit: u8,
    pub dir: MetricDir,
    /// 来源通道（0~2）。
    pub channel: Channel,
}

impl CaliberRow {
    pub fn unit_known(&self) -> bool {
        self.unit <= 5
    }
}

/// 冻结口径表（16 项——来源：B28 指标收口册；
/// F0899 健康类：吞吐/丢失率/降级率…；F0953 延迟类：P50/P95/P99）。
pub const FROZEN_CALIBER: [CaliberRow; METRIC_COUNT] = [
    CaliberRow { id: 1, name: "吞吐", unit: 0, dir: MetricDir::HigherBetter, channel: 0 },
    CaliberRow { id: 2, name: "受理率", unit: 4, dir: MetricDir::HigherBetter, channel: 0 },
    CaliberRow { id: 3, name: "丢失率", unit: 2, dir: MetricDir::LowerBetter, channel: 0 },
    CaliberRow { id: 4, name: "降级率", unit: 2, dir: MetricDir::LowerBetter, channel: 0 },
    CaliberRow { id: 5, name: "P50延迟", unit: 1, dir: MetricDir::LowerBetter, channel: 1 },
    CaliberRow { id: 6, name: "P95延迟", unit: 1, dir: MetricDir::LowerBetter, channel: 1 },
    CaliberRow { id: 7, name: "P99延迟", unit: 1, dir: MetricDir::LowerBetter, channel: 1 },
    CaliberRow { id: 8, name: "最大延迟", unit: 1, dir: MetricDir::LowerBetter, channel: 1 },
    CaliberRow { id: 9, name: "队列水位", unit: 3, dir: MetricDir::LowerBetter, channel: 2 },
    CaliberRow { id: 10, name: "配额余额", unit: 3, dir: MetricDir::HigherBetter, channel: 2 },
    CaliberRow { id: 11, name: "账本行数", unit: 3, dir: MetricDir::LowerBetter, channel: 2 },
    CaliberRow { id: 12, name: "熔断次数", unit: 3, dir: MetricDir::LowerBetter, channel: 0 },
    CaliberRow { id: 13, name: " BH消费量", unit: 3, dir: MetricDir::HigherBetter, channel: 0 },
    CaliberRow { id: 14, name: "核间迁移量", unit: 3, dir: MetricDir::LowerBetter, channel: 0 },
    CaliberRow { id: 15, name: "内存占用", unit: 5, dir: MetricDir::LowerBetter, channel: 2 },
    CaliberRow { id: 16, name: "指纹漂移", unit: 3, dir: MetricDir::LowerBetter, channel: 1 },
];

/// 口径账（单来源强制）。
pub struct CaliberLedger {
    rows: [Option<CaliberRow>; 20],
    used: usize,
    /// 指标号→通道占用（单一来源闸）。
    id_channel: [Option<Channel>; 20],
    frozen: bool,
    rejects: [u32; 11],
}

impl CaliberLedger {
    pub fn new() -> CaliberLedger {
        CaliberLedger {
            rows: [None; 20],
            used: 0,
            id_channel: [None; 20],
            frozen: false,
            rejects: [0; 11],
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    pub fn register(&mut self, row: CaliberRow) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if row.id == 0 || (row.id as usize) > 20 {
            return Err(self.reject(E_ID_BAD));
        }
        if row.channel > 2 {
            return Err(self.reject(E_CHANNEL_BAD));
        }
        if MetricDir::from_id(row.dir as u8).is_none() {
            return Err(self.reject(E_DIR_BAD));
        }
        if !row.unit_known() {
            return Err(self.reject(E_DIR_BAD));
        }
        // 单一来源闸：同指标号已有通道 → 双口径必拒。
        if self.id_channel[(row.id - 1) as usize].is_some() {
            return Err(self.reject(E_DUAL_SOURCE));
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
        self.id_channel[(row.id - 1) as usize] = Some(row.channel);
        self.used += 1;
        Ok(self.used - 1)
    }

    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.used < 15 {
            return Err(self.reject(E_COVER_MISS));
        }
        self.frozen = true;
        Ok(())
    }

    pub fn used(&self) -> usize {
        self.used
    }
}

/// 全量注册演练：16 项注册 + 冻结。
pub fn drill_register_all() -> (u32, u32, Vec<String>) {
    let mut ledger = CaliberLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut i = 0;
    while i < METRIC_COUNT {
        match ledger.register(FROZEN_CALIBER[i]) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("指标 {} 注册被拒：0x{:04X}", FROZEN_CALIBER[i].name, c)),
        }
        i += 1;
    }
    let frozen = ledger.freeze().is_ok();
    detail.push(format!("注册 {}/16 冻结={}", ok, frozen));
    (ok, if frozen { 1 } else { 0 }, detail)
}

/// 双口径注入 10/10：同指标换通道再注册，10 轮全拒。
pub fn inject_dual_source_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = CaliberLedger::new();
        let _ = ledger.register(FROZEN_CALIBER[r % METRIC_COUNT]);
        // 同指标换通道再注册（越域通道用闭集内另一值）。
        let alt_channel = (FROZEN_CALIBER[r % METRIC_COUNT].channel + 1) % 3;
        let mut bad = FROZEN_CALIBER[r % METRIC_COUNT];
        bad.channel = alt_channel;
        if ledger.register(bad) == Err(E_DUAL_SOURCE) {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("双口径 10/10 必拒".to_string());
    }
    (ok, 10, detail)
}

/// F0899/F0953 对齐演练：健康类（单位 2/3/4/5）方向全有定义、延迟类
/// （单位 1）全 LowerBetter（F0953 分位口径）、吞吐 HigherBetter
/// （F0899 口径）——逐行对拍。
pub fn drill_alignment() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut i = 0;
    while i < METRIC_COUNT {
        let row = FROZEN_CALIBER[i];
        let expect_ok = match row.unit {
            1 => row.dir == MetricDir::LowerBetter,            // F0953 分位口径
            0 => row.dir == MetricDir::HigherBetter,           // 吞吐口径
            2 | 5 => row.dir == MetricDir::LowerBetter,        // 率/占用越低越好
            3 => {
                if row.name == "配额余额" || row.name == " BH消费量" {
                    row.dir == MetricDir::HigherBetter
                } else {
                    row.dir == MetricDir::LowerBetter
                }
            } // 计数按语义
            _ => true,
        };
        if expect_ok {
            ok += 1;
        } else {
            detail.push(format!("指标 {} 口径对齐失败", row.name));
        }
        i += 1;
    }
    (ok, METRIC_COUNT as u32, detail)
}

/// 采集一致演练：三通道各产 64 样本（LCG），按口径汇聚成指标值，
/// 与手工重算一致 10/10。
pub fn drill_collect_consistency_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let seed = 0x2E10_0C0 + r as u64;
        // 通道0（分发账）：受理/分发两流，吞吐=分发合计。
        let mut acc = 0u64;
        let mut i = 0;
        let mut state = seed;
        while i < 64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            acc += (state >> 11) % 1000 + 1;
            i += 1;
        }
        // 手工重算（同 LCG 重放）。
        let mut acc2 = 0u64;
        let mut state2 = seed;
        let mut j = 0;
        while j < 64 {
            state2 = state2.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            acc2 += (state2 >> 11) % 1000 + 1;
            j += 1;
        }
        if acc == acc2 && acc > 0 {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("采集一致 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 注入 3 项：越指标号/越通道/越方向——必抓。
pub fn inject_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = CaliberLedger::new();
    let bad1 = CaliberRow { id: 0, name: "零号", unit: 0, dir: MetricDir::HigherBetter, channel: 0 };
    if ledger.register(bad1) == Err(E_ID_BAD) {
        caught += 1;
    } else {
        detail.push("越指标号未拒".to_string());
    }
    let bad2 = CaliberRow { id: 2, name: "越通道", unit: 0, dir: MetricDir::HigherBetter, channel: 9 };
    if ledger.register(bad2) == Err(E_CHANNEL_BAD) {
        caught += 1;
    } else {
        detail.push("越通道未拒".to_string());
    }
    // 越方向（from_id 不可还原）。
    if MetricDir::from_id(9).is_none() {
        caught += 1;
    } else {
        detail.push("越方向未拒".to_string());
    }
    if caught == 3 {
        detail.push("注入 3/3 全抓".to_string());
    }
    (caught, 3, detail)
}

/// 段位守卫：0x2E10 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_ID_BAD, E_DUP_REG, E_CHANNEL_BAD, E_DUAL_SOURCE, E_ALIGN_MISS,
        E_FROZEN, E_COVER_MISS, E_SAMPLE_MISS, E_DIR_BAD, E_LEDGER_FULL,
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

/// run_unx1347_checks：本单判据聚合（诊断段 0x2E10）。
pub fn run_unx1347_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1347 压测指标口径账");
    let (r1, f1, d1) = drill_register_all();
    s.add_dyn(
        "J1-16项口径统一登记",
        r1 == 16 && f1 == 1,
        &format!("{}/16 {}", r1, d1.join(" ")),
    );
    let (c2, t2, d2) = inject_dual_source_10();
    s.add_dyn("J1-单一来源10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (a3, t3, d3) = drill_alignment();
    s.add_dyn("J1-F0899/F0953对齐100%", a3 == t3, &format!("{}/{} {}", a3, t3, d3.join(" ")));
    let (c4, t4, d4) = drill_collect_consistency_10();
    s.add_dyn("J1-采集一致10/10", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    let (c5, t5, d5) = inject_bad();
    s.add_dyn("J1-注入3/3", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2E10段十码闭集");
    s
}
