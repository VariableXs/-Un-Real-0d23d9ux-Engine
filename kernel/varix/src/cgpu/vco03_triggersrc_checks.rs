//! CGPU-F2243 判据层：降级触发源汇总（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2243`
//!
//! **锚点判据（七类源表/扩展复用/融合复用/三组）→ 判据族**：
//! SRC 3 / FUSE 3 / REUSE 2 / META 4 = 12 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 七类标签**判据侧独立写死逐字对拍**；默认注册表 priority 连续性判据
//! 侧重算；融合裁决（severity 最高/平局取注册序最早）独立构造语料手算
//! 对拍；重复注册/空表/域外/未注册逐码拒；码段判据 `!=` 防自判死
//! （0x50..0x58 全排除）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vco03_triggersrc::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的七类标签（官方序逐字）。
const EXP_LABELS: [&str; 7] = [
    "帧超时", "热档", "续航档", "CGPU 档位", "弱网", "资源紧张", "场景切换",
];

/// 判据侧独立写死的声明句。
const EXP_EXTENSIBLE: &str = "源注册可扩展——新触发源追加注册，不改旧源语义";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [OtCode; 5] = [
    OtCode::DUPLICATE_SOURCE,
    OtCode::EMPTY_EVENTS,
    OtCode::SEVERITY_OUT_OF_RANGE,
    OtCode::UNREGISTERED_SOURCE,
    OtCode::FUSION_CONFLICT,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 12;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vco03 域自检入口。
pub fn run_vco03_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-triggersrc");

    // ================= 一、七类源表（SRC） =================

    // SRC-1：七类闭集 + 标签逐字对拍（判据侧写死）。
    let mut src1 = TriggerKind::ALL.len() == TRIGGER_COUNT && TRIGGER_COUNT == 7;
    for i in 0..7 {
        if TriggerKind::ALL[i].label() != EXP_LABELS[i] {
            src1 = false;
        }
    }
    s.add("O03-SRC-七类闭集标签对拍", src1, "");

    // SRC-2：默认注册表——七源全量、priority 0..6 连续（判据侧重算）、全启用。
    let r = default_registry();
    let mut src2 = r.sources.len() == 7;
    for i in 0..7 {
        match r.sources.get(i) {
            Some(sr) => {
                if sr.kind != TriggerKind::ALL[i] || sr.priority != i as u8 || !sr.enabled {
                    src2 = false;
                }
            }
            None => src2 = false,
        }
    }
    s.add("O03-SRC-默认源表完整连续", src2, "");

    // SRC-3：扩展注册——空表逐源注册七源全成（追加式 priority 递增）；
    // 重复注册专属码拒（扩展是追加不是覆盖）。
    let mut reg = new_registry();
    let mut src3 = true;
    for i in 0..7 {
        match register_source(&mut reg, TriggerKind::ALL[i]) {
            Ok(p) => {
                if p != i as u8 {
                    src3 = false;
                }
            }
            _ => src3 = false,
        }
    }
    src3 = src3
        && register_source(&mut reg, TriggerKind::FrameTimeout) == Err(OtCode::DUPLICATE_SOURCE);
    s.add("O03-SRC-追加扩展与重复拒", src3, "");

    // ================= 二、源融合（FUSE） =================

    let r = default_registry();

    // FUS-1：单源直通——单事件融合 dominant==该源，贡献账恰一条。
    let one = [TriggerEvent { kind: TriggerKind::WeakNet, tick: 1, severity: 6 }];
    let fus1 = match fuse(&one, &r) {
        Ok(o) => o.dominant == TriggerKind::WeakNet && o.contributors.len() == 1,
        _ => false,
    };
    s.add("O03-FUS-单源直通", fus1, "");

    // FUS-2：多源并发裁决手算——severity 3 vs 5 → 热档主源；
    // 平局 4 vs 4 → 注册序最早（帧超时 priority 0）胜（确定性）。
    let multi = [
        TriggerEvent { kind: TriggerKind::FrameTimeout, tick: 1, severity: 3 },
        TriggerEvent { kind: TriggerKind::ThermalStep, tick: 2, severity: 5 },
    ];
    let tie = [
        TriggerEvent { kind: TriggerKind::ThermalStep, tick: 1, severity: 4 },
        TriggerEvent { kind: TriggerKind::FrameTimeout, tick: 2, severity: 4 },
    ];
    let fus2 = match (fuse(&multi, &r), fuse(&tie, &r)) {
        (Ok(a), Ok(b)) => {
            a.dominant == TriggerKind::ThermalStep
                && a.contributors.len() == 2
                && b.dominant == TriggerKind::FrameTimeout
        }
        _ => false,
    };
    s.add("O03-FUS-多源裁决与平局确定性", fus2, "");

    // FUS-3：逐码拒——空表/严重度域外/未注册源（空注册表上任何事件）。
    let empty_reg = new_registry();
    let unreg = [TriggerEvent { kind: TriggerKind::WeakNet, tick: 1, severity: 1 }];
    let over = [TriggerEvent { kind: TriggerKind::WeakNet, tick: 1, severity: 10 }];
    s.add(
        "O03-FUS-逐码拒三向",
        fuse(&[], &r) == Err(OtCode::EMPTY_EVENTS)
            && fuse(&over, &r) == Err(OtCode::SEVERITY_OUT_OF_RANGE)
            && fuse(&unreg, &empty_reg) == Err(OtCode::UNREGISTERED_SOURCE),
        "",
    );

    // ================= 三、复用声明（REUSE） =================

    // RUS-1：F1459 融合复用联动对拍（判据侧写死）。
    s.add(
        "O03-RUS-F1459融合复用对拍",
        FUSION_UPLINK == 1459 && FUSION_UPLINK_NOTE.contains("F1459"),
        "",
    );

    // RUS-2：扩展复用声明逐字对拍。
    s.add("O03-RUS-扩展声明逐字对拍", EXTENSIBLE_NOTE == EXP_EXTENSIBLE, "");

    // ================= 四、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("O03-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x59xx，且 != 0x50..0x58（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x59)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57 && hi != 0x58
        });
    s.add("O03-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("O03-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 11，加自身恰 12）。
    s.add("O03-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
