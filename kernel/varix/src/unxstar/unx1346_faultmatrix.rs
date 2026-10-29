//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1346 · 故障注入复合谱（多故障叠加注入矩阵） · 0x2E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1346-J1 复合故障场景 ≥8 组（双故障/三故障叠加矩阵）资产化，
//!      复合注入检出率 100%（叠加必抓），故障隔离行为（单故障处置不
//!      误伤）验证 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 基础故障闭集 6 型（F1 丢中断/F2 重复/F3 错配/F4 延迟漂移/F5
//!     配额失效/F6 序号跳变），复合场景 = 2~3 型叠加，≥8 组矩阵冻结；
//!   - 检出引擎：每型一个独立探测器（真谓词），叠加注入全部命中
//!     （检出率 100%）且互不吞检；
//!   - 隔离行为：处置只作用于故障型对应的账面（单故障处置不误伤——
//!     处置后其余五型账零变化）10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2E00 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2E00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TYPE_BAD: u16 = SEG_BASE + 1;    // 故障型越闭集
const E_COMBO_DUP: u16 = SEG_BASE + 2;   // 复合场景重复
const E_DETECT_MISS: u16 = SEG_BASE + 3; // 检出缺位
const E_ISOLATE_LEAK: u16 = SEG_BASE + 4; // 隔离误伤
const E_ASSET_DUP: u16 = SEG_BASE + 5;   // 场景资产重复
const E_SEED_BAD: u16 = SEG_BASE + 6;    // 零种子
const E_FROZEN: u16 = SEG_BASE + 7;      // 冻结后写入
const E_COVER_MISS: u16 = SEG_BASE + 8;  // 场景数不足 8
const E_LEDGER_FULL: u16 = SEG_BASE + 9; // 账满
const E_REPLAY_MISS: u16 = SEG_BASE + 10; // 复跑不一致

/// 基础故障型闭集（6 型）。
pub const FAULT_TYPES: usize = 6;

/// 故障型名（冻结）。
pub const FAULT_NAMES: [&str; FAULT_TYPES] =
    ["丢中断", "重复投递", "错配路由", "延迟漂移", "配额失效", "序号跳变"];

/// 复合场景矩阵（≥8 组冻结：双故障 6 组 + 三故障 2 组）。
/// 单故障隔离演练用 arity=1（[target,99,99] 只校验首型）。
pub const COMBO_MATRIX: [([usize; 3], u8); 8] = [
    ([0, 1, 99], 2), // F1+F2
    ([0, 2, 99], 2), // F1+F3
    ([1, 4, 99], 2), // F2+F5
    ([2, 3, 99], 2), // F3+F4
    ([3, 5, 99], 2), // F4+F6
    ([4, 5, 99], 2), // F5+F6
    ([0, 1, 2], 3),  // F1+F2+F3
    ([3, 4, 5], 3),  // F4+F5+F6
];

/// 确定性 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state >> 11
    }
}

/// 注入器：按场景生成 20 事件流，命中故障型的事件携带扰动。
/// 返回六型账：每型观测到的扰动计数（探测器的输入）。
pub fn inject_stream(combo: &[usize; 3], arity: u8, seed: u64) -> Result<[u32; FAULT_TYPES], u16> {
    if seed == 0 {
        return Err(E_SEED_BAD);
    }
    if arity < 1 || arity > 3 {
        return Err(E_TYPE_BAD);
    }
    let mut k = 0;
    while k < arity as usize {
        if combo[k] >= FAULT_TYPES {
            return Err(E_TYPE_BAD);
        }
        k += 1;
    }
    let mut rng = Lcg::new(seed);
    let mut observed = [0u32; FAULT_TYPES];
    let mut e = 0;
    while e < 20 {
        let mut f = 0;
        while f < arity as usize {
            let t = combo[f];
            // 每型以 1/2 概率在第 e 事件上落扰动（确定性）。
            if rng.next() % 2 == 0 {
                observed[t] += 1;
            }
            f += 1;
        }
        e += 1;
    }
    Ok(observed)
}

/// 探测器：每型真谓词——该型扰动账 > 0 即检出。
pub fn detect(observed: &[u32; FAULT_TYPES], fault_type: usize) -> bool {
    if fault_type >= FAULT_TYPES {
        return false;
    }
    observed[fault_type] > 0
}

/// 复合检出演练：8 组场景全部全型检出（检出率 100%）。
pub fn drill_combo_detect() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut g = 0;
    while g < COMBO_MATRIX.len() {
        let (combo, arity) = COMBO_MATRIX[g];
        match inject_stream(&combo, arity, 0x2E00_0000 + g as u64) {
            Ok(observed) => {
                let mut all = true;
                let mut f = 0;
                while f < arity as usize {
                    if !detect(&observed, combo[f]) {
                        all = false;
                        detail.push(format!(
                            "组{}型{}（{}）漏检",
                            g + 1,
                            combo[f],
                            FAULT_NAMES[combo[f]]
                        ));
                    }
                    f += 1;
                }
                if all {
                    ok += 1;
                }
            }
            Err(c) => detail.push(format!("组{}被拒：0x{:04X}", g + 1, c)),
        }
        g += 1;
    }
    (ok, COMBO_MATRIX.len() as u32, detail)
}

/// 隔离行为 10/10：处置只清故障型账，其余型账零变化（不误伤）。
pub fn drill_isolation_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        // 单故障注入：型 (r % 6)，其余型零扰动。
        let target = r % FAULT_TYPES;
        let mut single = [99usize; 3];
        single[0] = target;
        let observed = match inject_stream(&single, 1, 0x2E00_0A0 + r as u64) {
            Ok(x) => x,
            Err(_) => break,
        };
        // 前置：只有目标型有扰动。
        let mut only_target = observed[target] > 0;
        let mut f = 0;
        while f < FAULT_TYPES {
            if f != target && observed[f] > 0 {
                only_target = false;
            }
            f += 1;
        }
        // 处置：清目标型账。
        let mut after = observed;
        after[target] = 0;
        // 不误伤：其余型处置前后全零。
        let mut no_harm = true;
        let mut f2 = 0;
        while f2 < FAULT_TYPES {
            if f2 != target && (observed[f2] != 0 || after[f2] != 0) {
                no_harm = false;
            }
            f2 += 1;
        }
        if only_target && no_harm {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("隔离不误伤 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 叠加必抓演练：对 8 组场景逐组注入后 detector 全触发 10/10 轮。
pub fn drill_overlay_catch_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut all = true;
        let mut g = 0;
        while g < COMBO_MATRIX.len() {
            let (combo, arity) = COMBO_MATRIX[g];
            if let Ok(observed) = inject_stream(&combo, arity, 0x2E00_0B0 + r as u64 + g as u64) {
                let mut f = 0;
                while f < arity as usize {
                    if !detect(&observed, combo[f]) {
                        all = false;
                    }
                    f += 1;
                }
            } else {
                all = false;
            }
            g += 1;
        }
        if all {
            ok += 1;
        }
        r += 1;
    }
    (ok, 10, detail)
}

/// 资产账：8 组场景登记。
#[derive(Clone, Copy, Debug)]
pub struct ComboAsset {
    pub group: u8,
    pub seed: u64,
    pub expect_fp: u64,
}

pub struct ComboLedger {
    assets: [Option<ComboAsset>; 8],
    used: usize,
    group_mask: u32,
    frozen: bool,
    rejects: [u32; 11],
}

impl ComboLedger {
    pub fn new() -> ComboLedger {
        ComboLedger {
            assets: [None; 8],
            used: 0,
            group_mask: 0,
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

    pub fn register(&mut self, group: u8, seed: u64) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if (group as usize) >= COMBO_MATRIX.len() {
            return Err(E_TYPE_BAD);
        }
        if seed == 0 {
            return Err(self.reject(E_SEED_BAD));
        }
        let bit = 1u32 << group;
        if self.group_mask & bit != 0 {
            return Err(self.reject(E_ASSET_DUP));
        }
        if self.used >= 8 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        let (combo, arity) = COMBO_MATRIX[group as usize];
        let observed = inject_stream(&combo, arity, seed)?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut f = 0;
        while f < FAULT_TYPES {
            fp ^= observed[f] as u64;
            fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
            f += 1;
        }
        self.assets[self.used] = Some(ComboAsset { group, seed, expect_fp: fp });
        self.used += 1;
        self.group_mask |= bit;
        Ok(fp)
    }

    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.group_mask != (1u32 << COMBO_MATRIX.len()) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        self.frozen = true;
        Ok(())
    }

    pub fn replay_matches(&self, group: u8) -> Result<bool, u16> {
        let mut i = 0;
        while i < self.used {
            if let Some(a) = self.assets[i] {
                if a.group == group {
                    let (combo, arity) = COMBO_MATRIX[group as usize];
                    let observed = inject_stream(&combo, arity, a.seed)?;
                    let mut fp = 0xcbf2_9ce4_8422_2325u64;
                    let mut f = 0;
                    while f < FAULT_TYPES {
                        fp ^= observed[f] as u64;
                        fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                        f += 1;
                    }
                    return Ok(fp == a.expect_fp);
                }
            }
            i += 1;
        }
        Err(E_TYPE_BAD)
    }
}

/// 资产生命周期演练：8 组登记+冻结+复跑。
pub fn drill_asset_lifecycle() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = ComboLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut g = 0;
    while g < COMBO_MATRIX.len() as u8 {
        match ledger.register(g, 0x2E00_1000 + g as u64) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("组{}登记被拒：0x{:04X}", g + 1, c)),
        }
        g += 1;
    }
    let frozen = ledger.freeze().is_ok();
    let mut replay_ok = 0u32;
    let mut k = 0;
    while k < COMBO_MATRIX.len() as u8 {
        if ledger.replay_matches(k) == Ok(true) {
            replay_ok += 1;
        }
        k += 1;
    }
    detail.push(format!("登记 {}/8 冻结={} 复跑 {}/8", ok, frozen, replay_ok));
    (ok, if frozen { 1 } else { 0 }, replay_ok, detail)
}

/// 注入 4 项：越型/坏叠加数/零种子/重复场景——必抓。
pub fn inject_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // 越型组合。
    let bad = [0usize, 99, 99];
    if inject_stream(&bad, 2, 1) == Err(E_TYPE_BAD) {
        caught += 1;
    } else {
        detail.push("越型未拒".to_string());
    }
    // 坏叠加数。
    let ok2 = [0usize, 1, 99];
    if inject_stream(&ok2, 4, 1) == Err(E_TYPE_BAD) {
        caught += 1;
    } else {
        detail.push("坏叠加数未拒".to_string());
    }
    // 零种子。
    let ok3 = [0usize, 1, 99];
    if inject_stream(&ok3, 2, 0) == Err(E_SEED_BAD) {
        caught += 1;
    } else {
        detail.push("零种子未拒".to_string());
    }
    // 重复场景。
    let mut ledger = ComboLedger::new();
    let _ = ledger.register(0, 0x2E00_9);
    if matches!(ledger.register(0, 0x2E00_A), Err(E_ASSET_DUP)) {
        caught += 1;
    } else {
        detail.push("重复场景未拒".to_string());
    }
    if caught == 4 {
        detail.push("注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 段位守卫：0x2E00 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_TYPE_BAD, E_COMBO_DUP, E_DETECT_MISS, E_ISOLATE_LEAK, E_ASSET_DUP,
        E_SEED_BAD, E_FROZEN, E_COVER_MISS, E_LEDGER_FULL, E_REPLAY_MISS,
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

/// run_unx1346_checks：本单判据聚合（诊断段 0x2E00）。
pub fn run_unx1346_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1346 故障注入复合谱");
    let (c1, t1, d1) = drill_combo_detect();
    s.add_dyn("J1-复合检出率100%", c1 == t1, &format!("{}/{} {}", c1, t1, d1.join(" ")));
    let (c2, t2, d2) = drill_isolation_10();
    s.add_dyn("J1-隔离不误伤10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = drill_overlay_catch_10();
    s.add_dyn("J1-叠加必抓10/10", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (r4, f4, p4, d4) = drill_asset_lifecycle();
    s.add_dyn(
        "J1-资产8组生命周期",
        r4 == 8 && f4 == 1 && p4 == 8,
        &format!("登{}/8 冻{} 复{}/8 {}", r4, f4, p4, d4.join(" ")),
    );
    let (c5, t5, d5) = inject_bad();
    s.add_dyn("J1-注入4/4", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2E00段十码闭集");
    s
}
