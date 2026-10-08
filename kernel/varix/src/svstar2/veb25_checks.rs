//! VE-F0225 判据层：Intel 显示控制器 pipe/plane（锚点五条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0225`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | pipe 层级 | `P25-PIPE-*` | 固定拓扑三槽 + 槽位身份 + 探针口径 pipe 数 |
//! | 时序匹配 | `P25-TIM-*` | 档位表 + 带宽账独立重算 + 安全模式回落 |
//! | watermark | `P25-WM-*` | 判据侧独立查表插值 + 超限降级单 plane 可达 |
//! | 原子序 | `P25-ATM-*` | 先关后配再开 + 快照回滚逐步数 |
//! | （Underrun 升级/错误码为锚点正文要求） | `P25-UNR-*` `P25-ERR-*` `P25-A11Y-*` | 计数升级处置 + 0x35xx 全映射 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! watermark 用判据侧独立写的查表插值（表字面量同源、实现各自独立）；
//! 带宽账公式判据侧重写；安全模式常量判据侧写死；降级阈值 WM_MAX=112
//! 判据侧字面量并证明可达性（表顶 128 > 112）。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::veb21_ident::GenTier;
use super::veb25_display::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照（不调被测的口径）
// ---------------------------------------------------------------------------

/// 判据侧独立 DSWB 表（与被测同源字面量、独立实现）。
const ALT_WM_TABLE: [(u32, u32); 6] = [
    (0, 8),
    (512, 16),
    (1024, 32),
    (2048, 64),
    (3840, 96),
    (7680, 128),
];

/// 判据侧独立查表插值（未封顶原值）。
fn alt_wm_raw(active_px: u32) -> u32 {
    if active_px <= ALT_WM_TABLE[0].0 {
        return ALT_WM_TABLE[0].1;
    }
    if active_px >= ALT_WM_TABLE[5].0 {
        return ALT_WM_TABLE[5].1;
    }
    let mut k = 0usize;
    let mut i = 1usize;
    while i + 1 < 6 {
        if active_px > ALT_WM_TABLE[i].0 {
            k = i;
        }
        i += 1;
    }
    let (x0, y0) = ALT_WM_TABLE[k];
    let (x1, y1) = ALT_WM_TABLE[k + 1];
    let frac = ((active_px - x0) as u64 * 1000) / ((x1 - x0) as u64);
    ((y0 as u64) + ((y1 as u64 - y0 as u64) * frac) / 1000) as u32
}

/// 判据侧独立带宽账。
fn alt_bw_ok(clock_khz: u32, link_mhz: u32, lanes: u8) -> bool {
    let req = clock_khz as u64 * 3;
    let avail = link_mhz as u64 * 1000 * lanes as u64 * 80 / 100;
    req <= avail
}

/// 判据侧构 plane。
fn mk_plane(kind: PlaneKind, w: u16, h: u16) -> PlaneCfg {
    PlaneCfg { kind, width: w, height: h, stride: w as u32 * 4, fmt: 0 }
}

/// 判据侧安全时序（字面量独立写死，与被测 SAFE_MODE_* 同源对拍）。
fn alt_safe_mode() -> TimingParams {
    TimingParams {
        mode_clock_khz: 25175,
        link_mhz: 1620,
        lanes: 2,
        hdisplay: 640,
        vdisplay: 480,
        vrefresh: 60,
    }
}

/// 判据侧构控制器（合法 pipes 必成功——P25-PIPE-probe-count 先证；
/// 三档位兜底链以 match 收口，零 panic 面）。
fn mk_ctl(pipes: u8) -> DisplayCtl {
    match DisplayCtl::new(GenTier::Baseline, pipes) {
        Ok(d) => d,
        Err(_) => match DisplayCtl::new(GenTier::XeStandard, pipes) {
            Ok(d) => d,
            Err(_) => match DisplayCtl::new(GenTier::XeLatest, pipes) {
                Ok(d) => d,
                Err(_) => loop {}, // 不可达：合法 pipes 三档位必有一档成功
            },
        },
    }
}

fn mk_planes_primary(w: u16, h: u16) -> [Option<PlaneCfg>; PLANES_PER_PIPE] {
    let mut p: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    p[PlaneKind::Primary.slot()] = Some(mk_plane(PlaneKind::Primary, w, h));
    p
}

// ---------------------------------------------------------------------------
// P25-PIPE：pipe 层级（判据一）
// ---------------------------------------------------------------------------

fn fam_pipe(s: &mut CheckSet) {
    // C1：pipe 数以探针为准——0 拒、超能力上限拒、1/2/3 收。
    let ok_c1 = DisplayCtl::new(GenTier::Baseline, 0).is_err()
        && DisplayCtl::new(GenTier::Baseline, 9).is_err()
        && DisplayCtl::new(GenTier::Baseline, 1).is_ok()
        && DisplayCtl::new(GenTier::Baseline, 2).is_ok()
        && DisplayCtl::new(GenTier::Baseline, 3).is_ok();
    s.add("P25-PIPE-probe-count", ok_c1, "探针口径 0/9 拒 1/2/3 收");

    // C2：pipe id 越界拒绝（越探针数）。
    let mut d = mk_ctl(2);
    let ok_c2 = d.underrun_irq(PipeId(2)) == Err(DErr::BadPipe);
    s.add("P25-PIPE-id-bounds", ok_c2, "越探针数 pipe id 专属拒");

    // C3：槽位身份——光标配置进主槽即拓扑错误（回滚保护）。
    let mut planes: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes[0] = Some(mk_plane(PlaneKind::Cursor, 64, 64));
    let r = d.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    s.add("P25-PIPE-slot-identity", r == Err(DErr::PlaneSlotMismatch), "光标进主槽拓扑错专属拒");

    // C4：几何——stride < width×4 拒。
    let mut d2 = mk_ctl(1);
    let mut planes2: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes2[0] = Some(PlaneCfg {
        kind: PlaneKind::Primary,
        width: 100,
        height: 100,
        stride: 200,
        fmt: 0,
    });
    let r2 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes2);
    s.add("P25-PIPE-geom-stride", r2 == Err(DErr::BadPlaneGeom), "stride 不足专属拒");

    // C5：三槽身份同检——SPR 进光标槽同拒。
    let mut planes3: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes3[2] = Some(mk_plane(PlaneKind::Sprite, 100, 100));
    let r3 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes3);
    s.add("P25-PIPE-slot-identity-2", r3 == Err(DErr::PlaneSlotMismatch), "SPR 进 CUR 槽拒");

    // C6：合法三 plane 一次通过且可见。
    let mut planes4: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes4[0] = Some(mk_plane(PlaneKind::Primary, 640, 480));
    planes4[1] = Some(mk_plane(PlaneKind::Sprite, 320, 240));
    planes4[2] = Some(mk_plane(PlaneKind::Cursor, 64, 64));
    let r4 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes4);
    let st = d2.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_c6 = r4.is_ok() && st.visible && st.planes[2].is_some() && d2.stats.atomic_commits == 1;
    s.add("P25-PIPE-three-planes-ok", ok_c6, "三 plane 合法配置可见");
}

// ---------------------------------------------------------------------------
// P25-TIM：时序匹配（判据二）
// ---------------------------------------------------------------------------

fn fam_tim(s: &mut CheckSet) {
    // T1：值合法性——lane=3 与非档位 link rate 拒（带宽账之前）。
    let bad1 = TimingParams { mode_clock_khz: 1000, link_mhz: 3300, lanes: 2, hdisplay: 64, vdisplay: 64, vrefresh: 60 };
    let bad2 = TimingParams { mode_clock_khz: 1000, link_mhz: 1620, lanes: 3, hdisplay: 64, vdisplay: 64, vrefresh: 60 };
    s.add("P25-TIM-value-table", !bad1.value_legal() && !bad2.value_legal(),
          "lane 3 / 非档位 link 拒");

    // T2：带宽账判据侧独立对拍——相等边界通过。
    // link 1620 lanes 1 ⇒ avail=1620*1000*1*0.8=1,296,000 ⇒ clock=432,000 kHz。
    let t_eq = TimingParams { mode_clock_khz: 432000, link_mhz: 1620, lanes: 1, hdisplay: 640, vdisplay: 480, vrefresh: 60 };
    let ok_t2 = t_eq.value_legal() && t_eq.bandwidth_ok() && alt_bw_ok(432000, 1620, 1);
    s.add("P25-TIM-bw-eq-boundary", ok_t2, "需求=可用边界两侧同判");

    // T3：带宽账不匹配——clock+1 即拒，判据侧同判。
    let t_over = TimingParams { mode_clock_khz: 432001, link_mhz: 1620, lanes: 1, hdisplay: 640, vdisplay: 480, vrefresh: 60 };
    s.add("P25-TIM-bw-over-both", !t_over.bandwidth_ok() && !alt_bw_ok(432001, 1620, 1),
          "超带宽两侧同判");

    // T4：被测拒绝路径——超带宽时序提交即 TimingBandwidthMismatch。
    let mut d = mk_ctl(1);
    let planes = mk_planes_primary(100, 100);
    let r = d.atomic_commit(PipeId(0), t_over, planes);
    s.add("P25-TIM-commit-reject", r == Err(DErr::TimingBandwidthMismatch), "超带宽提交专属拒");

    // T5：安全模式回落——非法时序 commit_or_safe 走安全模式且可见。
    let bad_t = TimingParams { mode_clock_khz: 9999, link_mhz: 1234, lanes: 7, hdisplay: 10, vdisplay: 10, vrefresh: 1 };
    let r2 = d.commit_or_safe(PipeId(0), bad_t, planes);
    let st = d.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_t5 = r2 == Ok(true) && d.stats.safe_mode_falls == 1
        && st.visible && st.timing.map(|x: TimingParams| x.mode_clock_khz) == Some(25175);
    s.add("P25-TIM-safe-fallback", ok_t5, "非法时序回落安全模式可见");

    // T6：安全模式自身合法且带宽匹配（回落不二次失败）+ 常量同源。
    let sm = alt_safe_mode();
    let sm2 = DisplayCtl::safe_mode();
    let ok_t6 = sm.value_legal() && sm.bandwidth_ok()
        && sm.mode_clock_khz == sm2.mode_clock_khz
        && sm.lanes == sm2.lanes && sm.link_mhz == sm2.link_mhz
        && sm.hdisplay == sm2.hdisplay && sm.vdisplay == sm2.vdisplay
        && sm.vrefresh == sm2.vrefresh;
    s.add("P25-TIM-safe-legal", ok_t6, "安全模式常量两侧同源自洽");

    // T7：非时序类失败不触发回落（几何错误原样透传）。
    let mut planes_bad: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes_bad[1] = Some(mk_plane(PlaneKind::Primary, 10, 10));
    let r3 = d.commit_or_safe(PipeId(0), alt_safe_mode(), planes_bad);
    let ok_t7 = r3 == Err(DErr::PlaneSlotMismatch) && d.stats.safe_mode_falls == 1;
    s.add("P25-TIM-no-fallback-other-err", ok_t7, "非时序错误不回落");
}

// ---------------------------------------------------------------------------
// P25-WM：watermark（判据三）
// ---------------------------------------------------------------------------

fn fam_wm(s: &mut CheckSet) {
    // W1：判据侧独立查表对拍采样点（含档位边界与插值中点）。
    let ok_w1 = wm_compute_raw(0) == alt_wm_raw(0)
        && wm_compute_raw(511) == alt_wm_raw(511)
        && wm_compute_raw(512) == alt_wm_raw(512)
        && wm_compute_raw(768) == alt_wm_raw(768)
        && wm_compute_raw(769) == alt_wm_raw(769)
        && wm_compute_raw(2048) == alt_wm_raw(2048)
        && wm_compute_raw(7680) == alt_wm_raw(7680);
    s.add("P25-WM-table-interp", ok_w1, "七采样点判据侧独立同判");

    // W2：插值中点语义独立断言（768px ⇒ 16+(32-16)/2 = 24）。
    s.add("P25-WM-midpoint-24", wm_compute_raw(768) == 24, "中点 768 ⇒ 24 块");

    // W3：单调不减（全档位扫描）。
    let mut mono = true;
    let mut prev = 0u32;
    let mut px = 0u32;
    while px <= 8000 {
        let v = wm_compute_raw(px);
        if v < prev {
            mono = false;
        }
        prev = v;
        px += 16;
    }
    s.add("P25-WM-monotonic", mono, "watermark 单调不减");

    // W4：超限降级单 plane 可达（6400px → 判据侧插值 117 > WM_MAX 112）。
    let mut d = mk_ctl(1);
    let mut planes: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes[0] = Some(mk_plane(PlaneKind::Primary, 6400, 1));
    planes[1] = Some(mk_plane(PlaneKind::Sprite, 64, 64));
    let r = d.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    let st = d.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_w4 = r.is_ok() && d.stats.wm_downgrades == 1
        && d.wm_downgrade_last && st.wm == 112
        && st.planes[1].is_none() && st.planes[0].is_some();
    s.add("P25-WM-downgrade-single", ok_w4, "超限降级单 plane 可观测");

    // W5：未超限不降级（512px → raw 16）。
    let mut d2 = mk_ctl(1);
    let mut planes2: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes2[0] = Some(mk_plane(PlaneKind::Primary, 512, 1));
    planes2[1] = Some(mk_plane(PlaneKind::Sprite, 32, 32));
    let r2 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes2);
    let st2 = d2.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_w5 = r2.is_ok() && !d2.wm_downgrade_last && d2.stats.wm_downgrades == 0
        && st2.planes[1].is_some() && st2.wm == 16;
    s.add("P25-WM-under-keep-planes", ok_w5, "未超限双 plane 保留");

    // W6：wm_compute 封顶值恒 ≤ WM_MAX（判据侧字面量 112/8）。
    s.add("P25-WM-clamp", wm_compute(99999) == 112 && wm_compute(0) == 8,
          "封顶 112/零点 8 字面量");
}

// ---------------------------------------------------------------------------
// P25-ATM：原子序（判据四）
// ---------------------------------------------------------------------------

fn fam_atm(s: &mut CheckSet) {
    let mut d = mk_ctl(1);
    let planes = mk_planes_primary(512, 1);

    // A1：成功提交步数恰为 3（关/配/开）且 ≤ 预算。
    let r1 = d.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    let ok_a1 = r1.is_ok() && d.stats.steps_last_commit == 3
        && d.stats.steps_last_commit <= ATOMIC_STEP_BUDGET;
    s.add("P25-ATM-steps-3", ok_a1, "成功序恰三步在预算内");

    // A2：失败回滚恢复快照——已可见 pipe 提交坏时序后状态逐字段全等。
    let before = d.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let bad_t = TimingParams { mode_clock_khz: 1000, link_mhz: 3300, lanes: 2, hdisplay: 64, vdisplay: 64, vrefresh: 60 };
    let _ = d.atomic_commit(PipeId(0), bad_t, planes);
    let after = d.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_a2 = before == after && d.stats.atomic_rollbacks == 1;
    s.add("P25-ATM-rollback-snapshot", ok_a2, "回滚快照逐字段全等");

    // A3：中间态不可见结构性——从不可见态失败提交后仍不可见零配置。
    let mut d2 = mk_ctl(1);
    let mut planes_bad: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
    planes_bad[0] = Some(mk_plane(PlaneKind::Cursor, 10, 10));
    let _ = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes_bad);
    let st2 = d2.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    let ok_a3 = !st2.visible && st2.timing.is_none() && d2.stats.atomic_rollbacks == 1;
    s.add("P25-ATM-invisible-midstate", ok_a3, "失败后中间态不外泄");

    // A4：三要素通知在回滚时产生且三件全非空。
    match d2.last_notice {
        Some(n) => s.add("P25-ATM-notice-3elem",
                         n.what.len() > 0 && n.impact.len() > 0 && n.action.len() > 0,
                         "三要素通知全非空"),
        None => s.add("P25-ATM-notice-3elem", false, "回滚未产生通知"),
    }

    // A5：成功提交无通知（通知只属于失败路径）。
    let r5 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    let ok_a5 = r5.is_ok() && d2.last_notice.is_none();
    s.add("P25-ATM-no-notice-on-ok", ok_a5, "成功路径零通知");

    // A6：被停用 pipe 提交拒绝（Underrun 处置优先）。
    let _ = d2.underrun_irq(PipeId(0));
    let _ = d2.underrun_irq(PipeId(0));
    let _ = d2.underrun_irq(PipeId(0));
    let r6 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    let ok_a6 = r6 == Err(DErr::PipeDisabled) && d2.is_disabled(PipeId(0));
    s.add("P25-ATM-disabled-pipe", ok_a6, "停用态提交专属拒");

    // A7：恢复后可再提交（处置不是永久死刑）。
    let _ = d2.reenable(PipeId(0));
    let r7 = d2.atomic_commit(PipeId(0), alt_safe_mode(), planes);
    let ok_a7 = r7.is_ok() && !d2.is_disabled(PipeId(0));
    s.add("P25-ATM-reenable-commit", ok_a7, "恢复后可再提交");
}

// ---------------------------------------------------------------------------
// P25-UNR：Underrun 升级处置
// ---------------------------------------------------------------------------

fn fam_unr(s: &mut CheckSet) {
    // U1：计数升级 1→记录 2→记录 3→停用（判据侧字面量阈值 3）。
    let mut d = mk_ctl(2);
    let l1 = d.underrun_irq(PipeId(0));
    let l2 = d.underrun_irq(PipeId(0));
    let l3 = d.underrun_irq(PipeId(0));
    let ok_u1 = l1 == Ok(1) && l2 == Ok(2) && l3 == Ok(3)
        && d.is_disabled(PipeId(0)) && d.stats.pipes_disabled == 1
        && d.underrun_count(PipeId(0)) == 3;
    s.add("P25-UNR-escalation", ok_u1, "1 记 2 记 3 停用逐级");

    // U2：停用 pipe 的可见性被清。
    let st = d.pipe(PipeId(0)).unwrap_or(PipeState::blank_state());
    s.add("P25-UNR-disabled-invisible", !st.visible, "停用即不可见");

    // U3：per-pipe 独立——pipe1 计数不受 pipe0 影响。
    let ok_u3 = d.underrun_count(PipeId(1)) == 0
        && d.underrun_irq(PipeId(1)) == Ok(1);
    s.add("P25-UNR-per-pipe", ok_u3, "计数按 pipe 独立");

    // U4：恢复清账——恢复后计数从 1 重数。
    let _ = d.reenable(PipeId(0));
    let ok_u4 = !d.is_disabled(PipeId(0)) && d.underrun_count(PipeId(0)) == 0
        && d.underrun_irq(PipeId(0)) == Ok(1);
    s.add("P25-UNR-reenable-reset", ok_u4, "恢复清账重数");
}

// ---------------------------------------------------------------------------
// P25-ERR：错误码全映射
// ---------------------------------------------------------------------------

fn fam_err(s: &mut CheckSet) {
    // R1：9 码全在 0x35xx 段且互异。
    let mut codes: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < DErr::ALL.len() {
        codes.push(DErr::ALL[i].code());
        i += 1;
    }
    let mut sorted = codes.clone();
    sorted.sort();
    let mut distinct = true;
    let mut k = 1usize;
    while k < sorted.len() {
        if sorted[k] == sorted[k - 1] {
            distinct = false;
        }
        k += 1;
    }
    let mut in_seg = true;
    let mut m = 0usize;
    while m < codes.len() {
        if codes[m] & 0xFF00 != 0x3500 {
            in_seg = false;
        }
        m += 1;
    }
    s.add("P25-ERR-9-distinct-inseg", distinct && in_seg && DErr::ALL.len() == 9,
          "9 码全在 0x35xx 段且互异");

    // R2：reason 全非空且互异。
    let mut rs: Vec<String> = Vec::new();
    let mut nonempty = true;
    let mut j = 0usize;
    while j < DErr::ALL.len() {
        let r = DErr::ALL[j].reason();
        if r.len() == 0 {
            nonempty = false;
        }
        rs.push(r);
        j += 1;
    }
    rs.sort();
    let mut rd = true;
    let mut k = 1usize;
    while k < rs.len() {
        if rs[k] == rs[k - 1] {
            rd = false;
        }
        k += 1;
    }
    s.add("P25-ERR-reasons-unique", nonempty && rd, "reason 全非空且互异");

    // R3：码值字面量判据侧写死（防被测侧改码段放水）。
    s.add("P25-ERR-code-literals",
          DErr::BadPipe.code() == 0x3501 && DErr::TimingBandwidthMismatch.code() == 0x3504
              && DErr::UnderrunDisabled.code() == 0x3508 && DErr::PipeDisabled.code() == 0x3509,
          "关键码字面量");

    // R4：BadPipe 真实可达（构造越探针数）。
    let ok_r4 = match DisplayCtl::new(GenTier::Baseline, 4) {
        Err(DErr::BadPipe) => true,
        _ => false,
    };
    s.add("P25-ERR-badpipe-reachable", ok_r4, "探针 4 越上限 3 专属拒");

    // R5：停用/降级码值字面量（可达性在 A6/W4 已证）。
    s.add("P25-ERR-pipedisabled-code",
          DErr::PipeDisabled.code() == 0x3509 && DErr::WmDowngraded.code() == 0x3507,
          "停用/降级码字面量");
}

// ---------------------------------------------------------------------------
// P25-A11Y/PERF：性能与读屏
// ---------------------------------------------------------------------------

fn fam_a11y(s: &mut CheckSet) {
    // Y1：常量字面量判据侧写死（O(1) 口径由 W1 采样点同判支撑）。
    s.add("P25-PERF-wm-literals",
          WM_MAX == 112 && UNDERRUN_DISABLE_AT == 3 && ATOMIC_STEP_BUDGET == 8
              && ENC_EFF_PCT == 80 && BYTES_PER_PIXEL == 3 && PLANES_PER_PIPE == 3,
          "六常量判据侧写死");

    // Y2：读屏摘要非空且不含地址。
    let d = mk_ctl(1);
    let sum = d.status_summary();
    let ok_y2 = sum.len() > 0 && !sum.contains("0x") && !sum.contains("0X");
    s.add("P25-A11Y-summary-privacy", ok_y2, "摘要可达零地址");

    // Y3：账本零值基线自证。
    let z = DispStats::zero();
    let ok_y3 = z.atomic_commits == 0 && z.atomic_rollbacks == 0
        && z.safe_mode_falls == 0 && z.wm_downgrades == 0 && z.underruns == 0
        && z.pipes_disabled == 0 && z.steps_last_commit == 0;
    s.add("P25-A11Y-stats-zero", ok_y3, "账本零值基线");

    // Y4：能力分派——基线档 link 上限 5400 / Xe 档 8100（判据侧字面量）。
    let ok_y4 = DispCaps::for_tier(GenTier::Baseline).max_link_mhz == 5400
        && DispCaps::for_tier(GenTier::XeStandard).max_link_mhz == 8100
        && DispCaps::for_tier(GenTier::XeLatest).max_pipes == 3;
    s.add("P25-PERF-caps-per-tier", ok_y4, "档位能力两侧同源");
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

/// VE-F0225 域自检（判据逐条映射锚点五条判据：
/// PIPE 6 / TIM 7 / WM 6 / ATM 7 / UNR 4 / ERR 5 / A11Y-PERF 4
/// 共 39 项七族）。
pub fn run_veb25_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-display");
    fam_pipe(&mut s);
    fam_tim(&mut s);
    fam_wm(&mut s);
    fam_atm(&mut s);
    fam_unr(&mut s);
    fam_err(&mut s);
    fam_a11y(&mut s);
    s
}
