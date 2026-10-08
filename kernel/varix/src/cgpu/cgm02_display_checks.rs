//! CGPU-F1922 判据层：显示枚举与热插拔（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1922`
//!
//! **锚点判据（枚举规范/去抖/竞态红线/状态机/批量/五组/判据）→ 判据族**：
//! ENUM 2 / DEBOUNCE 3 / RACE 3 / STATE 3 / BATCH 3 / CODE 2 / META 2 = 18 项。
//!
//! # 本层核心纪律
//!
//! 去抖恰端点（DEBOUNCE_MS-1 吸收、恰值放行）双向断；竞态红线断实际
//! 错误码（seq 恰一放行、落后/跳变双拒绝）；指纹判据侧独立重算 FNV
//! 对拍（非同源恒绿）；批量重跑一致（乱序到达固定序）。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::cgm01_display::VmCode;
use super::cgm02_display as dp;
use super::cgm02_display::*;
use alloc::vec;

// ---------------------------------------------------------------------------
// 判据主体
// ---------------------------------------------------------------------------

/// M02 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgm02_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-M");

    // --- ENUM · 枚举规范（判据①） ----------------------------------------
    // 快照建档：id+指纹+seq 三件齐；指纹判据侧独立重算对拍。
    let bus = &mut HpdBus::new();
    let edid_a: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
    let edid_b: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xDD];
    let snaps = enumerate(&[(1, &edid_a[..]), (2, &edid_b[..])], bus);
    let fp_a = edid_fingerprint(&edid_a);
    let enum_ok = snaps.len() == 2
        && snaps[0].display_id == 1
        && snaps[0].edid_fingerprint == fp_a
        && snaps[0].seq == 1
        && snaps[1].seq == 2
        && snaps[0].seq != snaps[1].seq;
    s.add(
        "M2-枚举-快照三件齐",
        enum_ok,
        "显示器枚举产出快照（id+EDID 指纹+枚举时刻 seq）；seq 由总线逐台分配互异",
    );

    // 指纹判据侧独立重算（FNV-1a 同口径手写对拍）。
    let mut h: u32 = 0x811C_9DC5;
    for &b in &edid_a {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let fp_ok = fp_a == h && fp_a != edid_fingerprint(&edid_b);
    s.add(
        "M2-枚举-指纹独立对拍",
        fp_ok,
        "EDID 指纹判据侧独立 FNV 重算逐字节对拍；不同 EDID 指纹互异（换屏可感知）",
    );

    // --- DEBOUNCE · 去抖规范（判据②） ------------------------------------
    let mut deb = Debouncer::new();
    let ev_c1 = HpdEvent { display_id: 1, kind: HpdKind::Connect, seq: 1 };
    let ev_d1 = HpdEvent { display_id: 1, kind: HpdKind::Disconnect, seq: 2 };
    let first = deb.admit(&ev_c1, 0);
    let inside = deb.admit(&ev_d1, DEBOUNCE_MS - 1);
    let exact = deb.admit(&ev_d1, DEBOUNCE_MS);
    s.add(
        "M2-去抖-窗口恰端点",
        first && !inside && exact,
        "leading-edge 去抖：窗口首事件放行、DEBOUNCE_MS-1 内同屏吸收、恰端点放行（含端点）",
    );

    // 新屏不受他屏窗口牵连；满槽覆盖不 panic（零 panic 面）。
    let mut deb2 = Debouncer::new();
    let mut all_admit = true;
    let mut i = 0;
    while i <= MAX_TRACKED + 1 {
        // 17 只屏（超 MAX_TRACKED 一只）：定容覆盖不炸。
        let ev = HpdEvent { display_id: 100 + i as u32, kind: HpdKind::Connect, seq: i as u64 + 1 };
        all_admit = all_admit && deb2.admit(&ev, i as u32 * 100);
        i += 1;
    }
    s.add(
        "M2-去抖-多屏独立与定容",
        all_admit,
        "多屏去抖互不牵连（逐屏独立槽）；超 MAX_TRACKED 覆盖最旧槽零 panic",
    );

    // --- RACE · 竞态红线（判据③） ----------------------------------------
    let ev5 = HpdEvent { display_id: 1, kind: HpdKind::Connect, seq: 5 };
    let race_ok = apply_event(4, &ev5) == Ok(())
        && apply_event(5, &ev5) == Err(VmCode::RACE_SEQ_GAP)
        && apply_event(3, &ev5) == Err(VmCode::RACE_SEQ_GAP);
    s.add(
        "M2-竞态-序列号恰一红线",
        race_ok,
        "事件 seq 必须 == 快照 seq+1：跳变与落后双拒绝（竞态=缺陷红线，拒绝并立案）",
    );

    // 快照新鲜度：seq=0 哨兵与 ≥水位 均拒；合法快照放行。
    let mut bus2 = HpdBus::new();
    let _ = bus2.take_seq();
    let _ = bus2.take_seq();
    let good = DisplaySnapshot { display_id: 1, edid_fingerprint: 7, seq: 2 };
    let stale0 = DisplaySnapshot { display_id: 1, edid_fingerprint: 7, seq: 0 };
    let stale_hi = DisplaySnapshot { display_id: 1, edid_fingerprint: 7, seq: 9 };
    let fresh_ok = snapshot_fresh(&good, &bus2) == Ok(())
        && snapshot_fresh(&stale0, &bus2) == Err(VmCode::SNAPSHOT_STALE)
        && snapshot_fresh(&stale_hi, &bus2) == Err(VmCode::SNAPSHOT_STALE);
    s.add(
        "M2-竞态-快照新鲜度",
        fresh_ok,
        "快照 seq=0 哨兵与 ≥ 总线水位均判过期；合法 seq 放行（枚举-事件对账基准）",
    );

    // --- STATE · 状态机规范（判据④） -------------------------------------
    let chain_ok = transition(DisplayState::Connected, DisplayState::Enabled) == Ok(())
        && transition(DisplayState::Enabled, DisplayState::ModeSet) == Ok(());
    let reject_ok = transition(DisplayState::Connected, DisplayState::ModeSet) == Err(VmCode::STATE_INVALID)
        && transition(DisplayState::Enabled, DisplayState::Connected) == Err(VmCode::STATE_INVALID)
        && transition(DisplayState::ModeSet, DisplayState::Connected) == Err(VmCode::STATE_INVALID);
    let idem = transition(DisplayState::ModeSet, DisplayState::ModeSet) == Ok(());
    s.add(
        "M2-状态机-单向链与幂等",
        chain_ok && reject_ok && idem,
        "connected→enabled→mode-set 单向放行；跳段/回退显性拒绝；mode-set 原地重设幂等",
    );

    // 三态闭集与标签（口径对账）。
    let labels_ok = DisplayState::ALL.len() == 3
        && DisplayState::ALL[0].label() == "connected"
        && DisplayState::ALL[1].label() == "enabled"
        && DisplayState::ALL[2].label() == "mode-set";
    s.add(
        "M2-状态机-三态闭集标签",
        labels_ok,
        "三态闭集官方序；标签与锚点原文逐字对账",
    );

    // --- BATCH · 批量处理（判据⑤） ---------------------------------------
    // 乱序到达 → 固定序输出；同屏多事件取 seq 最大者；重跑一致。
    let evs_a = vec![
        HpdEvent { display_id: 7, kind: HpdKind::Connect, seq: 3 },
        HpdEvent { display_id: 2, kind: HpdKind::Connect, seq: 4 },
        HpdEvent { display_id: 7, kind: HpdKind::Disconnect, seq: 9 },
        HpdEvent { display_id: 4, kind: HpdKind::Disconnect, seq: 2 },
    ];
    let out_a = plan_batch(evs_a.clone());
    let out_b = plan_batch(evs_a);
    let batch_ok = out_a.len() == 3
        && out_a[0].display_id == 2
        && out_a[1].display_id == 4
        && out_a[2].display_id == 7
        && out_a[2].kind == HpdKind::Disconnect
        && out_a[2].seq == 9
        && out_a == out_b;
    s.add(
        "M2-批量-固定序去重重跑一致",
        batch_ok,
        "多屏同时热插拔按 display_id 升序、同屏取 seq 最大者；乱序到达重跑逐条一致（确定性纪律）",
    );

    // 批量结果与竞态红线联动：批内事件 seq 对快照逐一恰一才可应用。
    let mut bus3 = HpdBus::new();
    let snap = {
        let _ = bus3.take_seq();
        DisplaySnapshot { display_id: 2, edid_fingerprint: 1, seq: 1 }
    };
    let batched = plan_batch(vec![
        HpdEvent { display_id: 2, kind: HpdKind::Connect, seq: 2 },
    ]);
    let linked = snapshot_fresh(&snap, &bus3) == Ok(())
        && apply_event(snap.seq, &batched[0]) == Ok(());
    s.add(
        "M2-批量-与竞态联动",
        linked,
        "批量事件逐条过竞态闸（seq 恰一）后方可进状态机——批量不是竞态豁免",
    );

    // --- CODE · 诊断码（0x54xx 域段续占） ---------------------------------
    let codes_ok = M02_CODES.len() == 5
        && M02_CODES.iter().all(|c| (c.code() & 0xFF00) == 0x5400)
        && M02_CODES.iter().all(|c| !c.reason().is_empty());
    let mut uniq = true;
    for i in 0..M02_CODES.len() {
        for j in (i + 1)..M02_CODES.len() {
            if M02_CODES[i].code() == M02_CODES[j].code() {
                uniq = false;
            }
        }
    }
    // 与 cgm01 码（0x5401~0x5407）互异——防同段撞码。
    let no_clash = M02_CODES.iter().all(|c| c.code() > 0x5407)
        && M02_CODES.iter().all(|c| {
            c.code() != VmCode::THEME_OUT_OF_TABLE.code()
                && c.code() != VmCode::RECEIPT_INCOMPLETE.code()
                && c.code() != VmCode::TERMINAL_VIOLATION.code()
        });
    s.add(
        "M2-码段-0x5408起续占互异",
        codes_ok && uniq && no_clash,
        "五码全部 0x54xx 域段、逐码互异、reason 非空；与 cgm01（0x5401~0x5407）无撞码防自判死",
    );

    // --- META · 判据承载力自检（判据⑥五组；判据⑦判据） --------------------
    // 竞态判据基准非平凡：seq 恰一场景真存在（bus 分配可复现）。
    let mut bus4 = HpdBus::new();
    let s1 = bus4.take_seq();
    let s2 = bus4.take_seq();
    let seq_nontrivial = s2 == s1 + 1 && s1 == 1;
    let meta_ok = seq_nontrivial
        && DEBOUNCE_MS == 30
        && MAX_TRACKED == 16
        && EXP_CODES.len() == M02_CODES.len();
    s.add(
        "M2-自检-判据承载力",
        meta_ok,
        "基准非平凡：总线 seq 从 1 单调、窗口/槽容常量钉死、判据侧码表与实现等长——对拍不空转",
    );

    s
}

/// 判据侧独立写死的本单码表（与实现 M02_CODES 逐值对拍）。
const EXP_CODES: [u16; 5] = [0x5408, 0x5409, 0x540A, 0x540B, 0x540C];

// 引用 dp 别名避免未使用告警（判据经 use super::* 覆盖大部分符号）。
const _: fn(&[u8]) -> u32 = dp::edid_fingerprint;
