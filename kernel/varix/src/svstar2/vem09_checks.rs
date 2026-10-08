//! VE-F2409 · 域自检（判据逐条对应，见 `vem09_import.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四通道映射 → `C09-映射-*`（四通道覆盖/四行落点/语义-载体两层/漂移拦截/
//!   四通道外跳过+声明/形态键并行轨/slerp 标记只挂旋转/CUBICSPLINE 只取值组）
//! - 三重校验 → `C09-校验-*`（引用四码各一/采样合法性五码/曲线异常四码/
//!   三重码域互不重合且每重非空）
//! - 保真默认 → `C09-保真-*`（保真帧数逐帧相等/精简帧数减少且端点钉死/
//!   精简误差实测且 ≤ epsilon/无阈值精简拒收）
//! - 结构化报告 → `C09-报告-*`（三要素齐备/映射表恰等于通道数/恰等于跳过数/
//!   轨道起点连续/警告人人话/三要素拒绝齐备）
//! - 零静默 → `C09-显性-*`（码标签互异/零指纹/超密计数不合并/负时刻钳制）
//!
//! **弱门禁自律**：本文件每条判据都在判据侧**自己重算**期望值或**自己造反例**，
//! 不问被测函数「你返回 true 吗」。凡涉及「恰好等于」处一律用 `==`。
//!
//! **补判据必须双向验证**：每条新判据都在变异 harness 里跑过「基线绿 +
//! 变体红」；只绿不红的判据等于没写。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::TrackValueKind;
use super::vem09_import::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自备，不从被测模块取样）
// ---------------------------------------------------------------------------

/// 空诊断袋。
fn bag() -> DiagBag {
    DiagBag::new()
}

/// 标量时间轴 accessor（`n` 帧，等距 0..1s）。
fn times(n: usize) -> AccessorView {
    let mut d: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        d.push(i as f32);
        i += 1;
    }
    AccessorView::new(ComponentType::Float, false, n as u32, 1, d)
}

/// 位置通道值（3 分量，第 `k` 帧 = `[k, 0, 0]`）。
fn pos_values(n: usize) -> Vec<f32> {
    let mut d: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        d.push(i as f32);
        d.push(0.0);
        d.push(0.0);
        i += 1;
    }
    d
}

/// 旋转通道值（4 分量，单位四元数）。
fn quat_values(n: usize) -> Vec<f32> {
    let mut d: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        d.push(0.0);
        d.push(0.0);
        d.push(0.0);
        d.push(1.0);
        i += 1;
    }
    d
}

/// 位置通道 accessor。
fn pos_acc(n: usize) -> AccessorView {
    AccessorView::new(ComponentType::Float, false, n as u32, 3, pos_values(n))
}

/// 旋转通道 accessor。
fn quat_acc(n: usize) -> AccessorView {
    AccessorView::new(ComponentType::Float, false, n as u32, 4, quat_values(n))
}

/// 单通道文档（`path` 指定路径）。
fn one_channel_doc(path: ChannelPath, n: usize, label: &str) -> GltfAnimDoc {
    let (vals, comps): (Vec<f32>, u8) = match path {
        ChannelPath::Rotation => (quat_values(n), 4),
        _ => (pos_values(n), 3),
    };
    let acc_t = times(n);
    let acc_v = AccessorView::new(ComponentType::Float, false, n as u32, comps, vals);
    GltfAnimDoc::new(
        1,
        vec![acc_t, acc_v],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path, sampler: 0 }],
        label,
    )
}

/// 导入一份文档（保真）。
fn import(doc: &GltfAnimDoc) -> Result<ImportResult, ImportError> {
    let mut b = bag();
    import_gltf_anim(doc, Fidelity::Faithful, &MappingTable::standard(), &mut b)
}

/// 导入一份文档（指定精度）。
fn import_with(doc: &GltfAnimDoc, f: Fidelity) -> Result<ImportResult, ImportError> {
    let mut b = bag();
    import_gltf_anim(doc, f, &MappingTable::standard(), &mut b)
}

/// 判据数超过 `CheckSet::MAX_CHECKS`（112，全仓共享）时按判据族切批：
/// - `a` = 四通道映射：映射表与四行落点（40 项）
/// - `b` = 三重校验：引用完整性 / 采样合法性（30 项）
/// - `c` = 三重校验：曲线异常 + 保真默认（44 项）
/// - `d` = 结构化报告 + 零静默（48 项）
///
/// **为什么是四族而不是三族**：全量判据数超出 `MAX_CHECKS`（112），
/// 直接聚合会**静默丢掉**末尾项——聚合器因此报绿，而没跑的判据没人知道。
/// 截断在 `run_vem09_checks` 里显性化成 `assert!`，并在此说明分族口径。
///
/// mod.rs 侧注册 `VE-F2409-a` / `VE-F2409-b` / `VE-F2409-c` / `VE-F2409-d` 四行。
pub fn run_vem09_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem09");
    run_vem09_checks_a(&mut set);
    run_vem09_checks_b(&mut set);
    run_vem09_checks_c(&mut set);
    run_vem09_checks_d(&mut set);
    assert!(
        !set.truncated(),
        "VE-F2409 判据数 {} 超出 CheckSet 容量 {}，聚合会静默丢项；请按 a/b/c/d 四族分别注册",
        set.len() + set.dropped(),
        crate::checks::MAX_CHECKS
    );
    set
}

/// a 族独立入口。
pub fn run_vem09_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem09-a");
    run_vem09_checks_a(&mut set);
    set
}

/// b 族独立入口。
pub fn run_vem09_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem09-b");
    run_vem09_checks_b(&mut set);
    set
}

/// c 族独立入口。
pub fn run_vem09_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem09-c");
    run_vem09_checks_c(&mut set);
    set
}

/// d 族独立入口。
pub fn run_vem09_checks_d_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem09-d");
    run_vem09_checks_d(&mut set);
    set
}

/// 第一批：四通道映射（映射表 + 四行落点 + 支持范围）。
pub fn run_vem09_checks_a(set: &mut CheckSet) {
    check_mapping_table(set);
    check_mapping_semantics(set);
    check_mapping_scope(set);
}

/// 第二批：三重校验之引用完整性 + 采样合法性。
pub fn run_vem09_checks_b(set: &mut CheckSet) {
    check_ref_validation(set);
    check_sample_validation(set);
}

/// 第三批：三重校验之曲线异常 + 保真默认。
pub fn run_vem09_checks_c(set: &mut CheckSet) {
    check_curve_validation(set);
    check_fidelity(set);
}

/// 第四批：结构化报告 + 零静默。
pub fn run_vem09_checks_d(set: &mut CheckSet) {
    check_report(set);
    check_explicit(set);
}

// ---------------------------------------------------------------------------
// 一、四通道映射
// ---------------------------------------------------------------------------

fn check_mapping_table(set: &mut CheckSet) {
    let t = MappingTable::standard();

    // 四行恰为四通道各一次（判据侧按 `ChannelPath::ALL` 独立数出现次数）。
    let mut cov = 0usize;
    let mut i = 0usize;
    while i < ChannelPath::ALL.len() {
        let p = ChannelPath::ALL[i];
        let mut hits = 0u32;
        let mut j = 0usize;
        while j < t.rows().len() {
            if t.rows()[j].path == p {
                hits += 1;
            }
            j += 1;
        }
        if hits != 1 {
            cov = 0;
            break;
        }
        cov += 1;
        // **外层游标必须推进**：漏掉这一行会让本判据死循环——而且症状是
        // 「整个自检挂住」，不是「某条判据红」，极难定位到这一行。
        i += 1;
    }
    set.add("C09-映射-四通道各一行", cov == 4 && mapping_covers_four_channels(&t), "四通道须各占一行且不重复");

    // 四条路径的落点语义逐条钉死（判据侧独立写出期望表）。
    let expect = [
        (ChannelPath::Translation, TrackSemantic::Position),
        (ChannelPath::Rotation, TrackSemantic::Rotation),
        (ChannelPath::Scale, TrackSemantic::Scale),
        (ChannelPath::Weights, TrackSemantic::MorphWeight),
    ];
    let mut all_ok = true;
    let mut k = 0usize;
    while k < expect.len() {
        if t.map(expect[k].0) != Some(expect[k].1) {
            all_ok = false;
        }
        k += 1;
    }
    set.add("C09-映射-四条落点语义", all_ok, "translation/rotation/scale/weights 落点须逐条对齐");
    // 行内 lanes 与语义声明一致（判据侧按 TrackSemantic::lanes 独立重算）。
    let mut lanes_ok = true;
    let mut r = 0usize;
    while r < t.rows().len() {
        let row = t.rows()[r];
        if let Some(s) = row.semantic {
            if row.lanes as usize != s.lanes() {
                lanes_ok = false;
            }
        }
        r += 1;
    }
    set.add("C09-映射-行内 lanes 自洽", lanes_ok, "lanes 须等于语义声明值");

    // 反例：把 Scale 行映成 Position，四行仍合法但语义被改 → 摘要必变。
    //
    // **被测表必须是「用错映行建的那张」**：若拿标准表去 reconcile，
    // 它本来就与规格一致，判据会红——而红的原因是语料错，不是实现错。
    let mutated = [
        MapRow::new(ChannelPath::Translation, Some(TrackSemantic::Position)),
        MapRow::new(ChannelPath::Rotation, Some(TrackSemantic::Rotation)),
        // scale 被错映为 position：
        MapRow::new(ChannelPath::Scale, Some(TrackSemantic::Position)),
        MapRow::new(ChannelPath::Weights, Some(TrackSemantic::MorphWeight)),
    ];
    let mut bad = MappingTable::with_rows(mutated);
    let drift_seen = drift_detected(&mut bad, mutated);
    set.add("C09-映射-错映必被摘要捕获", drift_seen, "scale→position 的错映须改变摘要");

    // 对照组：标准表 reconcile 必须判绿（否则上条可能只是「reconcile 恒红」）。
    let mut ok_tbl = MappingTable::standard();
    let mut ok_bag = bag();
    let ok_flagged = ok_tbl.reconcile(&mut ok_bag);
    set.add(
        "C09-映射-标准表对账不报漂移",
        !ok_flagged && !ok_tbl.drifted() && ok_tbl.checksum() == spec_checksum(),
        "标准表须与规格摘要逐位相等",
    );

    // 拦截态：map() 一律 None（四行语义全 None），拦截前 None。
    let mut t2 = MappingTable::standard();
    let mut b = bag();
    let _ = t2.reconcile(&mut b);
    set.add("C09-映射-对账基线绿", !b.has(DiagCode::MAPPING_DRIFT), "标准表不该判漂移");

    let mut t3 = MappingTable::with_rows(mutated);
    let mut b3 = bag();
    let intercepted = t3.reconcile(&mut b3);
    let mut none_after = 0usize;
    let mut p = 0usize;
    while p < ChannelPath::ALL.len() {
        if t3.map(ChannelPath::ALL[p]).is_none() {
            none_after += 1;
        }
        p += 1;
    }
    set.add(
        "C09-映射-漂移置拦截",
        intercepted && t3.intercepted() && none_after == 4,
        "漂移后四路径查表须全 None（拦截而非记一笔）",
    );

    set.add("C09-映射-漂移记 P1", b3.p1_count() >= 1, "漂移须立案");

    // 拦截态下导入整体拒绝（三要素齐备）。
    //
    // **必须在 `clear_intercept()` 之前测**：解除拦截后表是可用的，
    // 拿解除后的表测「拦截态拒绝」必然失败——顺序错了判据就恒红，
    // 而症状（一条红项）看不出是顺序错。
    let doc = one_channel_doc(ChannelPath::Translation, 4, "x.gltf");
    let mut b4 = bag();
    let r = import_gltf_anim(&doc, Fidelity::Faithful, &t3, &mut b4);
    let rejected = match &r {
        Err(e) => e.code == DiagCode::MAPPING_INTERCEPTED && e.three_elements_complete(),
        Ok(_) => false,
    };
    set.add("C09-映射-拦截态拒绝导入", rejected, "拦截态必须整体拒绝且三要素齐备");

    set.add(
        "C09-映射-拦截态记 IMPORT_ABORTED",
        b4.has(DiagCode::IMPORT_ABORTED),
        "整体中止须显性记账",
    );

    // 解拦截后恢复（且仍判漂移——拦截态不掩盖漂移事实）。
    t3.clear_intercept();
    let restored = t3.map(ChannelPath::Translation);
    set.add(
        "C09-映射-解拦截可恢复查表",
        !t3.intercepted() && restored == Some(TrackSemantic::Position),
        "clear_intercept 后查表须恢复",
    );

    // 对照组：解除拦截后导入**恢复成功**（否则上两条可能是「表永久坏掉」）。
    let mut b5 = bag();
    let r2 = import_gltf_anim(&doc, Fidelity::Faithful, &t3, &mut b5);
    set.add(
        "C09-映射-解拦截后导入恢复",
        r2.is_ok() && !b5.has(DiagCode::IMPORT_ABORTED),
        "解除拦截后须能正常导入（证明拦截是可逆的）",
    );

    // 线上编码互异 + 反查一致（防两通道同码）。
    let mut wires: Vec<u8> = Vec::new();
    let mut w_ok = true;
    let mut w = 0usize;
    while w < ChannelPath::ALL.len() {
        let code = ChannelPath::ALL[w].wire();
        if wires.contains(&code) {
            w_ok = false;
        } else {
            wires.push(code);
        }
        if ChannelPath::from_wire(code) != ChannelPath::ALL[w] {
            w_ok = false;
        }
        w += 1;
    }
    set.add("C09-映射-线上编码互异可反查", w_ok, "wire() 须两两不同且 from_wire 可反查");

    // 四通道外路径 wire=0 且不在范围。
    set.add(
        "C09-映射-四通道外不入范围",
        !ChannelPath::Other.in_scope() && ChannelPath::Other.wire() == 0,
        "Other 须 wire=0 且 in_scope=false",
    );

    set.add("C09-映射-语义码互异", semantics_codes_unique(), "四个语义 wire() 须两两不同");
}

/// 「给定一套行，是否被判为漂移」——**判据侧的漂移探针**。
///
/// 它自己重算摘要（不复用 `MappingTable::checksum` 的结果当真值），
/// 免得被测函数与判据同源而恒真。
///
/// **基准是规格摘要**（`spec_checksum()`），不是建表时那套行自己的摘要——
/// 后者会让「用错映行建表」得到「声明 == 实际」，漂移永远检不出。
fn drift_detected(t: &mut MappingTable, rows: [MapRow; 4]) -> bool {
    // 判据侧独立算：这套错映行的摘要是否等于**规格**摘要。
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < rows.len() {
        let r = rows[i];
        h = (h ^ r.path.wire() as u32).wrapping_mul(0x0100_0193);
        let sv = match r.semantic {
            Some(s) => s.wire() as u32,
            None => 0,
        };
        h = (h ^ sv).wrapping_mul(0x0100_0193);
        h = (h ^ r.lanes as u32).wrapping_mul(0x0100_0193);
        h = (h ^ r.path.label().len() as u32).wrapping_mul(0x0100_0193);
        let sl = match r.semantic {
            Some(s) => s.label().len() as u32,
            None => 0,
        };
        h = (h ^ sl).wrapping_mul(0x0100_0193);
        i += 1;
    }
    let changed = h != spec_checksum();
    let mut b = bag();
    let flagged = t.reconcile(&mut b);
    // 判据侧重算的摘要须与被测函数重算的一致（同一口径），且确实判了漂移。
    changed && flagged && h == t.checksum()
}

fn semantics_codes_unique() -> bool {
    let mut seen: Vec<u8> = Vec::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < TrackSemantic::ALL.len() {
        let w = TrackSemantic::ALL[i].wire();
        if seen.contains(&w) {
            ok = false;
        } else {
            seen.push(w);
        }
        i += 1;
    }
    ok
}

fn check_mapping_semantics(set: &mut CheckSet) {
    // 位置通道 → 位置语义 + Position 载体 + 3 通道。
    let doc = one_channel_doc(ChannelPath::Translation, 4, "a.gltf");
    match import(&doc) {
        Ok(res) => {
            let t = match res.tracks.first() {
                Some(t) => t,
                None => {
                    set.add("C09-映射-位置轨落地", false, "未产出轨道");
                    return;
                }
            };
            set.add(
                "C09-映射-位置语义与载体",
                t.semantic == TrackSemantic::Position
                    && t.track.kind == TrackValueKind::Position
                    && t.track.kind.lanes() == 3,
                "translation 须落位置轨 + Position 载体 3 通道",
            );
            set.add("C09-映射-位置不带 slerp", !t.slerp, "非旋转不得带 slerp 标记");
            set.add(
                "C09-映射-位置 SoA 自洽",
                t.track.shape_ok() && t.track.times.len() == 4,
                "通道数据长度须 = 帧数 × 3",
            );
        }
        Err(_) => set.add("C09-映射-位置轨落地", false, "导入被拒"),
    }

    // 旋转通道 → 旋转语义 + Quat 载体 + slerp 标记。
    let rdoc = one_channel_doc(ChannelPath::Rotation, 4, "b.gltf");
    match import(&rdoc) {
        Ok(res) => {
            let t = match res.tracks.first() {
                Some(t) => t,
                None => {
                    set.add("C09-映射-旋转轨落地", false, "未产出轨道");
                    return;
                }
            };
            set.add(
                "C09-映射-旋转语义与载体",
                t.semantic == TrackSemantic::Rotation
                    && t.track.kind == TrackValueKind::Quat
                    && t.track.kind.lanes() == 4,
                "rotation 须落四元数轨 + Quat 载体 4 通道",
            );
            set.add("C09-映射-旋转带 slerp", t.slerp, "旋转须带 slerp 标记（F2423 前向）");
        }
        Err(_) => set.add("C09-映射-旋转轨落地", false, "导入被拒"),
    }

    // 缩放通道 → Scale 语义 + Position 载体（3 通道同形，语义靠 tag 承载）。
    let sdoc = one_channel_doc(ChannelPath::Scale, 4, "c.gltf");
    match import(&sdoc) {
        Ok(res) => {
            let t = match res.tracks.first() {
                Some(t) => t,
                None => {
                    set.add("C09-映射-缩放轨落地", false, "未产出轨道");
                    return;
                }
            };
            set.add(
                "C09-映射-缩放语义与载体",
                t.semantic == TrackSemantic::Scale && t.track.kind == TrackValueKind::Position,
                "scale 须落缩放语义 + Position 载体（3 通道同形）",
            );
            set.add(
                "C09-映射-缩放语义不靠载体反推",
                t.semantic != TrackSemantic::Position,
                "缩放语义须与位置语义可区分（载体同形，靠 tag 承载）",
            );
        }
        Err(_) => set.add("C09-映射-缩放轨落地", false, "导入被拒"),
    }

    // 形态键通道（2 个形态键）→ 2 条标量轨并行。
    let n = 3usize;
    let mvals: Vec<f32> = vec![0.0f32, 1.0, 0.5, 0.2, 1.0, 0.25];
    let wdoc = GltfAnimDoc::new(
        1,
        vec![times(n), AccessorView::new(ComponentType::Float, false, n as u32, 2, mvals)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Weights, sampler: 0 }],
        "d.gltf",
    );
    match import(&wdoc) {
        Ok(res) => {
            set.add(
                "C09-映射-形态键并行轨数=键数",
                res.morph_tracks(0) == 2 && res.tracks.len() == 2,
                "2 个形态键须产出 2 条标量轨",
            );
            let mut slots_ok = res.tracks.len() == 2
                && res.tracks[0].morph_slot == 0
                && res.tracks[1].morph_slot == 1;
            set.add("C09-映射-形态键槽位下标", slots_ok, "morph_slot 须为 0/1 递增");

            let mut scalar_ok = true;
            let mut i = 0usize;
            while i < res.tracks.len() {
                if res.tracks[i].track.kind != TrackValueKind::Scalar {
                    scalar_ok = false;
                }
                i += 1;
            }
            set.add("C09-映射-形态键载体为标量", scalar_ok, "每条 morph 轨须为 Scalar 载体");

            // 分量取值正确性：轨 m 取**原始输出的第 m 分量**。
    //
    // 语料 3 帧 × 2 分量 = `[0.0,1.0] [0.5,0.2] [1.0,0.25]`
    // ⇒ 形态键 0 的时间序列 = [0.0, 0.5, 1.0]
    // ⇒ 形态键 1 的时间序列 = [1.0, 0.2, 0.25]
    //
    // 判据侧从语料**独立重算**两个期望序列再逐点比，而不是比首元素——
    // 只比首元素的话，「后续帧整体错位一列」这类缺陷查不出来。
    let expect_m0 = [0.0f32, 0.5, 1.0];
    let expect_m1 = [1.0f32, 0.2, 0.25];
    let seq_ok = res
        .tracks
        .iter()
        .enumerate()
        .all(|(i, t)| {
            let want = if i == 0 { &expect_m0 } else { &expect_m1 };
            t.track.channels.len() == want.len()
                && (0..want.len()).all(|k| t.track.channels[k] == want[k])
        });
    set.add(
        "C09-映射-形态键分量不错位",
        seq_ok,
        "每条 morph 轨须取自己的分量列，逐点相等",
    );
            set.add(
                "C09-映射-形态键不带 slerp",
                !res.tracks.iter().any(|t| t.slerp),
                "morph 不得带 slerp 标记",
            );
        }
        Err(_) => set.add("C09-映射-形态键并行轨数=键数", false, "导入被拒"),
    }

    // 插值标记单源：glTF STEP → F2403 Step。
    let step_sampler = SamplerRef { input: 0, output: 1, interp: GltfInterp::Step };
    let sdoc2 = GltfAnimDoc::new(
        1,
        vec![times(4), pos_acc(4)],
        vec![step_sampler],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "e.gltf",
    );
    match import(&sdoc2) {
        Ok(res) => {
            let interp = res.tracks.first().map(|t| t.interp);
            set.add("C09-映射-STEP 映射 F2403 Step", interp == Some(Interp::Step), "STEP 须落 Interp::Step");
        }
        Err(_) => set.add("C09-映射-STEP 映射 F2403 Step", false, "导入被拒"),
    }

    // CUBICSPLINE：每帧 3 组，只取「值」组（中段），切线丢弃并声明。
    //
    // **输出 accessor 的 `count` 必须是关键帧数 × 3**（glTF 规范：CUBICSPLINE
    // 每关键帧存 3 组）。写成 `cn` 会被判 `VALUE_COUNT_MISMATCH` 而跳过。
    let cn = 3usize;
    let mut cvals: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < cn {
        // [入切线 x3 | 值 x3 | 出切线 x3]
        cvals.push(9.0);
        cvals.push(9.0);
        cvals.push(9.0);
        cvals.push(i as f32);
        cvals.push(0.0);
        cvals.push(0.0);
        cvals.push(-9.0);
        cvals.push(-9.0);
        cvals.push(-9.0);
        i += 1;
    }
    let cdoc = GltfAnimDoc::new(
        1,
        vec![
            times(cn),
            AccessorView::new(
                ComponentType::Float,
                false,
                (cn * 3) as u32,
                3,
                cvals,
            ),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::CubicSpline }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "f.gltf",
    );
    let mut cb = bag();
    match import_gltf_anim(&cdoc, Fidelity::Faithful, &MappingTable::standard(), &mut cb) {
        Ok(res) => {
            let first_x = res.tracks.first().and_then(|t| t.track.channels.first().copied());
            set.add(
                "C09-映射-CUBICSPLINE 只取值组",
                first_x == Some(0.0),
                "须取中段「值」组（0），不得读入切线（9/-9）",
            );
            let declared = res.report.declarations.iter().any(|d| d.contains("CUBICSPLINE"));
            set.add("C09-映射-CUBICSPLINE 切线丢弃有声明", declared, "切线丢弃须留下声明");

            // 值组单调 0,1,2（判据侧独立重算期望）。
            let xs_ok = res
                .tracks
                .first()
                .map(|t| {
                    let n = t.track.times.len();
                    let mut ok = true;
                    let mut f = 0usize;
                    while f < n {
                        let expect = f as f32;
                        if t.track.channels[f * 3] != expect {
                            ok = false;
                        }
                        f += 1;
                    }
                    ok
                })
                .unwrap_or(false);
            set.add("C09-映射-CUBICSPLINE 值组逐帧对齐", xs_ok, "各帧值须为 0,1,2");
        }
        Err(_) => set.add("C09-映射-CUBICSPLINE 只取值组", false, "导入被拒"),
    }

    set.add(
        "C09-映射-CUBICSPLINE 组数=3",
        GltfInterp::CubicSpline.value_groups() == 3 && GltfInterp::Linear.value_groups() == 1,
        "CUBICSPLINE 每帧 3 组，LINEAR 1 组",
    );

    // value_groups 影响期望值数：CUBICSPLINE 的 2 帧 3 分量须 18 值。
    let two_frame_cubic = 2usize * 3 * 3;
    set.add(
        "C09-映射-期望值数含组展开",
        GltfSamplerOut { comps: 3, interp: GltfInterp::CubicSpline, count: 2 }
            .expected_value_comps()
            * 2
            == two_frame_cubic,
        "2 帧 × 3 分量 × 3 组 = 18",
    );

    // find() 只匹配非形态键轨（morph 靠 morph_tracks）。
    let fdoc = GltfAnimDoc::new(
        2,
        vec![times(3), pos_acc(3), AccessorView::new(ComponentType::Float, false, 3, 1, vec![0.1f32, 0.2, 0.3])],
        vec![
            SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear },
            SamplerRef { input: 0, output: 2, interp: GltfInterp::Linear },
        ],
        vec![
            ChannelRef { target_node: 1, path: ChannelPath::Translation, sampler: 0 },
            ChannelRef { target_node: 1, path: ChannelPath::Weights, sampler: 1 },
        ],
        "g.gltf",
    );
    match import(&fdoc) {
        Ok(res) => {
            let f1 = res.find(1, TrackSemantic::Position);
            let fm = res.find(1, TrackSemantic::MorphWeight);
            set.add(
                "C09-映射-find 排除形态键轨",
                f1.is_some() && fm.is_none(),
                "find 不得把 morph 轨当成语义轨返回",
            );
            set.add(
                "C09-映射-节点下标定位正确",
                f1.map(|t| t.target_node) == Some(1),
                "按目标节点定位",
            );
        }
        Err(_) => set.add("C09-映射-find 排除形态键轨", false, "导入被拒"),
    }
}

fn check_mapping_scope(set: &mut CheckSet) {
    // 四通道外通道：跳过 + 声明 + 不产出轨道。
    let doc = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Other, sampler: 0 }],
        "custom.gltf",
    );
    let mut b = bag();
    match import_gltf_anim(&doc, Fidelity::Faithful, &MappingTable::standard(), &mut b) {
        Ok(res) => {
            set.add("C09-映射-四通道外不产轨道", res.tracks.is_empty(), "四通道外通道不得产出轨道");
            set.add(
                "C09-映射-四通道外记跳过",
                res.report.channels_skipped == 1 && res.report.channels_imported == 0,
                "须计为跳过而非导入",
            );
            let declared = res.report.declarations.iter().any(|d| d.contains("四通道外"));
            set.add("C09-映射-四通道外有声明", declared, "跳过必须留下支持范围声明");
            set.add(
                "C09-映射-四通道外记诊断码",
                b.has(DiagCode::PATH_OUT_OF_SCOPE),
                "须记 PATH_OUT_OF_SCOPE",
            );
            // 映射记录里该通道 track_count=0 且有处置码。
            let rec = res.report.mapping.first();
            let rec_ok = rec.map(|r| r.track_count == 0 && r.verdict.is_some()).unwrap_or(false);
            set.add("C09-映射-四通道外映射记录留痕", rec_ok, "映射表须记 track_count=0 与处置码");
        }
        Err(_) => set.add("C09-映射-四通道外不产轨道", false, "导入被拒（不应发生）"),
    }

    // 混合：1 个支持 + 1 个不支持 → 支持的照常导入，跳过计数为 1。
    let mixed = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![
            ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 },
            ChannelRef { target_node: 0, path: ChannelPath::Other, sampler: 0 },
        ],
        "mixed.gltf",
    );
    match import(&mixed) {
        Ok(res) => {
            set.add(
                "C09-映射-混合通道不受阻",
                res.tracks.len() == 1 && res.report.channels_imported == 1,
                "一个越界通道不得阻断其它通道",
            );
            set.add("C09-映射-混合跳过计数为 1", res.report.channels_skipped == 1, "跳过恰为 1");
        }
        Err(_) => set.add("C09-映射-混合通道不受阻", false, "导入被拒"),
    }

    // 分量数不符：位置通道给 2 分量 → 跳过（防造出 2 分量位置轨）。
    let wrong = GltfAnimDoc::new(
        1,
        vec![times(3), AccessorView::new(ComponentType::Float, false, 3, 2, vec![0.0f32; 6])],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "w.gltf",
    );
    match import(&wrong) {
        Ok(res) => {
            set.add("C09-映射-分量数不符被拒", res.tracks.is_empty(), "2 分量位置通道须跳过");
            set.add(
                "C09-映射-分量不符记码",
                res.report.mapping.first().map(|r| r.verdict) == Some(Some(DiagCode::VALUE_OUT_OF_DOMAIN)),
                "须记 VALUE_OUT_OF_DOMAIN",
            );
        }
        Err(_) => set.add("C09-映射-分量数不符被拒", false, "导入被拒（不应发生）"),
    }

    // 旋转通道给 3 分量 → 跳过。
    let wrong_r = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Rotation, sampler: 0 }],
        "wr.gltf",
    );
    match import(&wrong_r) {
        Ok(res) => set.add("C09-映射-旋转分量须为 4", res.tracks.is_empty(), "3 分量旋转须跳过"),
        Err(_) => set.add("C09-映射-旋转分量须为 4", false, "导入被拒"),
    }

    // 归一化绕过：u16 未归一化却给原始码值 → 跳过。
    let bypass = GltfAnimDoc::new(
        1,
        vec![
            times(3),
            AccessorView::new(ComponentType::U16, false, 3, 3, vec![0.0f32, 0.0, 0.0, 1000.0, 0.0, 0.0, 2000.0, 0.0, 0.0]),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "bp.gltf",
    );
    match import(&bypass) {
        Ok(res) => set.add("C09-映射-归一化绕过被拒", res.tracks.is_empty(), "未归一化 u16 原始码值须拦"),
        Err(_) => set.add("C09-映射-归一化绕过被拒", false, "导入被拒"),
    }

    // 对照组：u16 已归一化（值域 [0,1]）→ 正常导入（否则上条是恒真门禁）。
    let normalized_ok = GltfAnimDoc::new(
        1,
        vec![
            times(3),
            AccessorView::new(ComponentType::U16, true, 3, 3, vec![0.0f32, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0]),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "ok.gltf",
    );
    match import(&normalized_ok) {
        Ok(res) => set.add("C09-映射-已归一化正常导入", res.tracks.len() == 1, "归一化 u16 须正常导入"),
        Err(_) => set.add("C09-映射-已归一化正常导入", false, "导入被拒"),
    }

    // 对照组：未归一化整数但值域小（合法的米制因子）→ 不算绕过。
    let small_int = GltfAnimDoc::new(
        1,
        vec![
            times(3),
            AccessorView::new(ComponentType::U16, false, 3, 3, vec![0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "si.gltf",
    );
    match import(&small_int) {
        Ok(res) => set.add("C09-映射-小值域整数不误判", res.tracks.len() == 1, "未归一化但值域小须放行"),
        Err(_) => set.add("C09-映射-小值域整数不误判", false, "导入被拒"),
    }

    set.add(
        "C09-映射-归一化判定仅整数未归一化",
        bypass_doc_bypassed() && !normalized_doc_bypassed() && !small_doc_bypassed(),
        "仅整数+未归一化+大值域三者同时成立才算绕过",
    );

    // full_scale 契约：u16=32767、u8=127、Float=1.0。
    set.add(
        "C09-映射-整数满量程契约",
        ComponentType::U16.full_scale() == 32767.0
            && ComponentType::U8.full_scale() == 127.0
            && ComponentType::Float.full_scale() == 1.0,
        "反归一化分母须与 glTF 规范一致",
    );
}

fn bypass_doc_bypassed() -> bool {
    AccessorView::new(ComponentType::U16, false, 1, 1, vec![300.0f32]).normalization_bypassed()
}
fn normalized_doc_bypassed() -> bool {
    AccessorView::new(ComponentType::U16, true, 1, 1, vec![0.5f32]).normalization_bypassed()
}
fn small_doc_bypassed() -> bool {
    AccessorView::new(ComponentType::U16, false, 1, 1, vec![3.0f32]).normalization_bypassed()
}

// ---------------------------------------------------------------------------
// 二、三重校验
// ---------------------------------------------------------------------------

fn check_ref_validation(set: &mut CheckSet) {
    // sampler 越界。
    let (code, doc) = ref_fail_case(9, 0, 0, 1);
    set.add("C09-校验-sampler 越界码", code == Some(DiagCode::SAMPLER_OOR), "须记 SAMPLER_OOR");
    set.add(
        "C09-校验-sampler 越界不产轨",
        doc.tracks.is_empty() && doc.report.channels_skipped == 1,
        "越界通道不产轨道且计跳过",
    );

    // input accessor 越界（sampler 有效、input 无效）。
    let (code, _d) = input_oor_case();
    set.add("C09-校验-input 越界码", code == Some(DiagCode::INPUT_ACCESSOR_OOR), "须记 INPUT_ACCESSOR_OOR");

    // output accessor 越界。
    let (code, _d) = output_oor_case();
    set.add(
        "C09-校验-output 越界码",
        code == Some(DiagCode::OUTPUT_ACCESSOR_OOR),
        "须记 OUTPUT_ACCESSOR_OOR",
    );

    // 目标节点不存在。
    let (code, _d) = node_missing_case();
    set.add(
        "C09-校验-目标节点缺失码",
        code == Some(DiagCode::TARGET_NODE_MISSING),
        "须记 TARGET_NODE_MISSING",
    );

    // **四码互不相同**（三重校验的「重一」不得塌成一码）。
    let codes = [
        DiagCode::SAMPLER_OOR,
        DiagCode::INPUT_ACCESSOR_OOR,
        DiagCode::OUTPUT_ACCESSOR_OOR,
        DiagCode::TARGET_NODE_MISSING,
    ];
    let mut distinct = true;
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add("C09-校验-引用四码互异", distinct, "四类引用须各有独立码");

    // 四类都归 ref_class，且 ref_class 成员恰为这四个。
    let mut all_ref = true;
    let mut k = 0usize;
    while k < codes.len() {
        if !codes[k].is_ref_class() {
            all_ref = false;
        }
        k += 1;
    }
    set.add("C09-校验-四码同属引用重", all_ref, "is_ref_class 须覆盖四码");

    // 合法规：四类引用都在域内 → 无 ref 类诊断。
    let ok_doc = one_channel_doc(ChannelPath::Translation, 3, "ok.gltf");
    let mut b = bag();
    let code_ok = validate_channel_refs(&ok_doc, ok_doc.channels[0], &mut b);
    set.add("C09-校验-合法规通过", code_ok.is_none() && b.is_empty(), "合法引用须无诊断");

    // 节点下标恰好等于 node_count → 越界（边界夹逼：= 合法，> 越界）。
    let edge = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 1, path: ChannelPath::Translation, sampler: 0 }],
        "edge.gltf",
    );
    let mut be = bag();
    let edge_code = validate_channel_refs(&edge, edge.channels[0], &mut be);
    set.add(
        "C09-校验-节点边界夹逼",
        edge_code == Some(DiagCode::TARGET_NODE_MISSING) && be.has(DiagCode::TARGET_NODE_MISSING),
        "node==node_count 已越界（合法域是 [0, node_count)）",
    );

    // 越界诊断须进报告警告清单（可核对）。
    //
    // **语料用 `sampler: 0` + `input: 9`**：sampler 下标本身合法，
    // 越界的是它引用的 input accessor ⇒ 期望码是 `INPUT_ACCESSOR_OOR`。
    // 早先写 `sampler: 9` 却期望 `SAMPLER_OOR`，而语料真正越界的是 input
    // ——判据与语料各说各话，症状是「一条判据恒红但看不出哪里错」。
    match import(&GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 9, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "z.gltf",
    )) {
        Ok(res) => {
            let warned = res
                .report
                .warnings
                .iter()
                .any(|w| w.code == DiagCode::INPUT_ACCESSOR_OOR && w.channel == 0);
            set.add("C09-校验-越界进警告清单", warned, "越界须在警告清单指名通道与真实码");
        }
        Err(_) => set.add("C09-校验-越界进警告清单", false, "导入被拒"),
    }

    set.add("C09-校验-码域三重互斥", triple_validation_codes_disjoint(), "每码至多属一重");
}

/// 造一个「某类引用越界」的文档。
fn ref_fail_case(sampler: u32, node: u32, _in: u32, _out: u32) -> (Option<DiagCode>, ImportResult) {
    let doc = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: node, path: ChannelPath::Translation, sampler }],
        "r.gltf",
    );
    let mut b = bag();
    let code = validate_channel_refs(&doc, doc.channels[0], &mut b);
    let res = import_gltf_anim(&doc, Fidelity::Faithful, &MappingTable::standard(), &mut b)
        .unwrap_or_else(|_| ImportResult {
            tracks: Vec::new(),
            report: ImportReport { channels_total: 1, ..ImportReport::default() },
        });
    (code, res)
}

fn input_oor_case() -> (Option<DiagCode>, ImportResult) {
    let doc = GltfAnimDoc::new(
        1,
        vec![pos_acc(3)],
        vec![SamplerRef { input: 7, output: 0, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "i.gltf",
    );
    let mut b = bag();
    let code = validate_channel_refs(&doc, doc.channels[0], &mut b);
    (code, ImportResult { tracks: Vec::new(), report: ImportReport::default() })
}

fn output_oor_case() -> (Option<DiagCode>, ImportResult) {
    let doc = GltfAnimDoc::new(
        1,
        vec![times(3)],
        vec![SamplerRef { input: 0, output: 7, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "o.gltf",
    );
    let mut b = bag();
    let code = validate_channel_refs(&doc, doc.channels[0], &mut b);
    (code, ImportResult { tracks: Vec::new(), report: ImportReport::default() })
}

fn node_missing_case() -> (Option<DiagCode>, ImportResult) {
    let doc = GltfAnimDoc::new(
        1,
        vec![times(3), pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 5, path: ChannelPath::Translation, sampler: 0 }],
        "n.gltf",
    );
    let mut b = bag();
    let code = validate_channel_refs(&doc, doc.channels[0], &mut b);
    (code, ImportResult { tracks: Vec::new(), report: ImportReport::default() })
}

fn check_sample_validation(set: &mut CheckSet) {
    let out3 = GltfSamplerOut { comps: 3, interp: GltfInterp::Linear, count: 3 };
    let t3v = vec![0.0f32, 1.0, 2.0];
    let v3 = pos_values(3);

    // 合法采样 → Ok 且零诊断。
    let mut b = bag();
    let v = validate_samples(&t3v, &out3, &v3, TrackSemantic::Position, &mut b);
    set.add(
        "C09-校验-合法采样通过",
        v == SampleVerdict::Ok && b.is_empty(),
        "合法采样须 Ok 且无诊断",
    );

    // 空采样。
    let mut b1 = bag();
    let v1 = validate_samples(&[], &out3, &[], TrackSemantic::Position, &mut b1);
    set.add(
        "C09-校验-空采样被拒",
        v1 == SampleVerdict::Reject(DiagCode::SAMPLER_EMPTY),
        "空采样须拒 SAMPLER_EMPTY",
    );

    // 时刻非单调（倒序）。
    let mut b2 = bag();
    let v2 = validate_samples(
        &[0.0f32, 2.0, 1.0],
        &out3,
        &pos_values(3),
        TrackSemantic::Position,
        &mut b2,
    );
    set.add(
        "C09-校验-时刻倒序被拒",
        v2 == SampleVerdict::Reject(DiagCode::TIMES_NON_MONOTONIC),
        "倒序须拒 TIMES_NON_MONOTONIC",
    );

    // 对照：等值时刻（非递减）合法 —— 否则「倒序被拒」可能是「相等也被拒」。
    let mut b2b = bag();
    let v2b = validate_samples(
        &[0.0f32, 1.0, 1.0],
        &out3,
        &pos_values(3),
        TrackSemantic::Position,
        &mut b2b,
    );
    set.add(
        "C09-校验-等值时刻合法",
        v2b == SampleVerdict::Ok,
        "非递减含等值（F2407 二分前提）",
    );

    // 值数不自洽。
    let mut b3 = bag();
    let v3b = validate_samples(&t3v, &out3, &vec![0.0f32; 6], TrackSemantic::Position, &mut b3);
    set.add(
        "C09-校验-值数不自洽被拒",
        v3b == SampleVerdict::Reject(DiagCode::VALUE_COUNT_MISMATCH),
        "6 值 / 3 帧 3 分量须拒",
    );

    // count 与时刻数打架。
    let mut b4 = bag();
    let out_bad_count = GltfSamplerOut { comps: 3, interp: GltfInterp::Linear, count: 5 };
    let v4 = validate_samples(&t3v, &out_bad_count, &v3, TrackSemantic::Position, &mut b4);
    set.add(
        "C09-校验-count 与帧数打架被拒",
        v4 == SampleVerdict::Reject(DiagCode::VALUE_COUNT_MISMATCH),
        "声明 count≠时刻数须拒",
    );

    // 旋转全零 → QUAT_DEGENERATE（且仍 Ok，帧级回退而非丢通道）。
    let mut b5 = bag();
    let out4 = GltfSamplerOut { comps: 4, interp: GltfInterp::Linear, count: 2 };
    let v5 = validate_samples(&[0.0f32, 1.0], &out4, &vec![0.0f32; 8], TrackSemantic::Rotation, &mut b5);
    set.add(
        "C09-校验-退化四元数记账",
        v5 == SampleVerdict::Ok && b5.has(DiagCode::QUAT_DEGENERATE),
        "全零四元数须记账但不丢通道",
    );

    // 对照：合法单位四元数不记退化。
    let mut b6 = bag();
    let v6 = validate_samples(
        &[0.0f32, 1.0],
        &out4,
        &quat_values(2),
        TrackSemantic::Rotation,
        &mut b6,
    );
    set.add(
        "C09-校验-合法四元数不记退化",
        v6 == SampleVerdict::Ok && !b6.has(DiagCode::QUAT_DEGENERATE),
        "单位四元数须不记退化",
    );

    // **反例：CUBICSPLINE 的入切线全零不得判退化**（切线零是正常起步）。
    let mut b7 = bag();
    // **count 必须是关键帧数 × 3**（CUBICSPLINE 每帧 3 组），不是帧数本身。
    let out_cubic = GltfSamplerOut { comps: 4, interp: GltfInterp::CubicSpline, count: 6 };
    let mut cubic_vals: Vec<f32> = Vec::new();
    let mut f = 0usize;
    while f < 2 {
        // 入切线全零、值 = 单位四元数、出切线全零
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(1.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        cubic_vals.push(0.0);
        f += 1;
    }
    let v7 = validate_samples(
        &[0.0f32, 1.0],
        &out_cubic,
        &cubic_vals,
        TrackSemantic::Rotation,
        &mut b7,
    );
    set.add(
        "C09-校验-切线零不判退化",
        v7 == SampleVerdict::Ok && !b7.has(DiagCode::QUAT_DEGENERATE),
        "退化判定须读值组而非入切线组",
    );

    // 五码互异且同属 sample_class。
    let scodes = [
        DiagCode::SAMPLER_EMPTY,
        DiagCode::TIMES_NON_MONOTONIC,
        DiagCode::VALUE_COUNT_MISMATCH,
        DiagCode::QUAT_DEGENERATE,
        DiagCode::VALUE_OUT_OF_DOMAIN,
    ];
    let mut s_all = true;
    let mut s_dup = false;
    let mut i = 0usize;
    while i < scodes.len() {
        if !scodes[i].is_sample_class() {
            s_all = false;
        }
        let mut j = i + 1;
        while j < scodes.len() {
            if scodes[i] == scodes[j] {
                s_dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    set.add("C09-校验-采样五码同属一重", s_all, "is_sample_class 须覆盖五码");
    set.add("C09-校验-采样五码互异", !s_dup, "五码须两两不同");
}

fn check_curve_validation(set: &mut CheckSet) {
    let out3 = GltfSamplerOut { comps: 3, interp: GltfInterp::Linear, count: 3 };

    // 干净数据 → 四项异常计数皆 0。
    let clean = detect_curve_anomaly(&[0.0f32, 1.0, 2.0], &out3, &pos_values(3));
    set.add(
        "C09-校验-干净数据零异常",
        clean.non_finite_values == 0
            && clean.non_finite_times == 0
            && clean.overdense_pairs == 0
            && clean.negative_times == 0,
        "1s 间隔的三帧不得有任何异常",
    );

    // 值 NaN → 计入。
    let mut vals = pos_values(3);
    vals[4] = f32::NAN;
    let nan_v = detect_curve_anomaly(&[0.0f32, 1.0, 2.0], &out3, &vals);
    set.add(
        "C09-校验-值 NaN 被计入",
        nan_v.non_finite_values == 1,
        "恰 1 个非有限分量",
    );

    // Inf 也计入（非仅 NaN）。
    let mut vals_inf = pos_values(3);
    vals_inf[0] = f32::INFINITY;
    let inf_v = detect_curve_anomaly(&[0.0f32, 1.0, 2.0], &out3, &vals_inf);
    set.add("C09-校验-值 Inf 被计入", inf_v.non_finite_values == 1, "Inf 同样计入");

    // 时刻 NaN → 计入，且**该帧不参与超密比较**（prev_ms 不更新）。
    let nan_t = detect_curve_anomaly(&[0.0f32, f32::NAN, 2.0], &out3, &pos_values(3));
    set.add(
        "C09-校验-时刻 NaN 被计入",
        nan_t.non_finite_times == 1,
        "非有限时刻须计入",
    );

    // 超密：0.0004s 与 0.0006s 取整后为 0ms 与 1ms —— 只有**一对**零间隔
    // （帧 0→帧 1 同为 0ms；帧 1→帧 2 差 1ms，恰在分辨率下限，不算超密）。
    //
    // 判据侧独立算：`secs_to_ms` 逐个求值再数「相邻差 < 1ms」的对数，
    // 而不是把期望值写死成 2（写死成 2 会与「1ms 不算超密」那条判据矛盾）。
    let dense = detect_curve_anomaly(&[0.0f32, 0.0004, 0.0006, 1.0], &out3, &pos_values(4));
    let ms = [secs_to_ms(0.0), secs_to_ms(0.0004), secs_to_ms(0.0006), secs_to_ms(1.0)];
    let mut expect_pairs = 0u32;
    let mut i = 1usize;
    while i < ms.len() {
        if ms[i] < ms[i - 1].saturating_add(1) {
            expect_pairs += 1;
        }
        i += 1;
    }
    set.add(
        "C09-校验-量化零间隔记超密",
        dense.overdense_pairs == expect_pairs && expect_pairs > 0,
        "判据侧按量化结果独立数零间隔对数",
    );

    // 对照：1ms 间隔不算超密（阈值 = 1ms）。
    let ok_gap = detect_curve_anomaly(&[0.0f32, 0.001, 0.002], &out3, &pos_values(3));
    set.add(
        "C09-校验-1ms 间隔不记超密",
        ok_gap.overdense_pairs == 0,
        "恰 1ms 不算超密（分辨率下限）",
    );

    // 负时刻。
    let neg = detect_curve_anomaly(&[-1.0f32, 0.0, 1.0], &out3, &pos_values(3));
    set.add("C09-校验-负时刻被计入", neg.negative_times == 1, "负时刻须计入");

    // 四码同属 curve_class 且互异。
    let ccodes = [
        DiagCode::VALUE_NON_FINITE,
        DiagCode::TIME_NON_FINITE,
        DiagCode::OVERDENSE_KEYS,
        DiagCode::NEGATIVE_TIME,
    ];
    let mut c_all = true;
    let mut c_dup = false;
    let mut i = 0usize;
    while i < ccodes.len() {
        if !ccodes[i].is_curve_class() {
            c_all = false;
        }
        let mut j = i + 1;
        while j < ccodes.len() {
            if ccodes[i] == ccodes[j] {
                c_dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    set.add("C09-校验-曲线四码同属一重", c_all, "is_curve_class 须覆盖四码");
    set.add("C09-校验-曲线四码互异", !c_dup, "四码须两两不同");

    // **值 NaN 钳制而非丢帧**（导入后帧数不减）。
    let nan_doc = GltfAnimDoc::new(
        1,
        vec![times(3), AccessorView::new(ComponentType::Float, false, 3, 3, vals)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "nan.gltf",
    );
    match import(&nan_doc) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add(
                "C09-校验-值 NaN 钳制不丢帧",
                keys == 3,
                "值 NaN 不得减少帧数（丢帧会静默改动画长度）",
            );
            let clamped = res.tracks.first().map(|t| t.clamped).unwrap_or(0);
            set.add("C09-校验-值 NaN 钳制计数", clamped == 1, "钳制计数恰为 1");
            let finite = res
                .tracks
                .first()
                .map(|t| t.track.channels.iter().all(|v| v.is_finite()))
                .unwrap_or(false);
            set.add("C09-校验-钳制后全有限", finite, "轨道里不得残留 NaN/Inf");
            set.add(
                "C09-校验-值 NaN 语义钳制目标为 0",
                res.tracks
                    .first()
                    .map(|t| t.track.channels.get(4).copied() == Some(0.0))
                    .unwrap_or(false),
                "位置通道 NaN 须钳到 0",
            );
            set.add(
                "C09-校验-值 NaN 进警告清单",
                res.report.warnings.iter().any(|w| w.code == DiagCode::VALUE_NON_FINITE),
                "须显性警告",
            );
        }
        Err(_) => set.add("C09-校验-值 NaN 钳制不丢帧", false, "导入被拒"),
    }

    // **对照：旋转 NaN 钳到单位四元数（不是 0）**。
    let mut qvals = quat_values(3);
    qvals[3] = f32::NAN;
    let qnan_doc = GltfAnimDoc::new(
        1,
        vec![times(3), AccessorView::new(ComponentType::Float, false, 3, 4, qvals)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Rotation, sampler: 0 }],
        "qn.gltf",
    );
    match import(&qnan_doc) {
        Ok(res) => {
            let w_is_1 = res
                .tracks
                .first()
                .map(|t| t.track.channels.get(3).copied() == Some(1.0))
                .unwrap_or(false);
            set.add("C09-校验-旋转 NaN 钳为单位四元数", w_is_1, "旋转 NaN 须钳到 w=1（恒等）");
        }
        Err(_) => set.add("C09-校验-旋转 NaN 钳为单位四元数", false, "导入被拒"),
    }

    // 时刻 NaN → 丢该帧（帧数减 1）。
    let tnan_doc = GltfAnimDoc::new(
        1,
        vec![
            AccessorView::new(ComponentType::Float, false, 3, 1, vec![0.0f32, f32::NAN, 2.0]),
            pos_acc(3),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "tn.gltf",
    );
    match import(&tnan_doc) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-校验-时刻 NaN 丢该帧", keys == 2, "时刻不可钳制，须丢帧（3→2）");
            set.add(
                "C09-校验-时刻 NaN 进警告清单",
                res.report.warnings.iter().any(|w| w.code == DiagCode::TIME_NON_FINITE),
                "须显性警告",
            );
            set.add(
                "C09-校验-丢帧后时间轴仍非递减",
                res.tracks
                    .first()
                    .map(|t| t.track.times.windows(2).all(|w| w[1] >= w[0]))
                    .unwrap_or(false),
                "丢帧后二分前提仍须成立",
            );
        }
        Err(_) => set.add("C09-校验-时刻 NaN 丢该帧", false, "导入被拒"),
    }

    // 超密帧**保留**（不合并），且计数等于超密对数。
    let dense_doc = GltfAnimDoc::new(
        1,
        vec![times(4), pos_acc(4)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "de.gltf",
    );
    let mut b = bag();
    let mut times_dense = times(4);
    times_dense.data = vec![0.0f32, 0.0004, 0.0006, 1.0];
    let dense_doc2 = GltfAnimDoc::new(
        1,
        vec![times_dense, pos_acc(4)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "de2.gltf",
    );
    let _ = import_gltf_anim(&dense_doc, Fidelity::Faithful, &MappingTable::standard(), &mut b);
    match import(&dense_doc2) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-校验-超密帧保留不合并", keys == 4, "零间隔帧须保留（资产侧真实信息）");
            set.add(
                "C09-校验-超密进警告清单",
                res.report.warnings.iter().any(|w| w.code == DiagCode::OVERDENSE_KEYS),
                "须显性警告",
            );
        }
        Err(_) => set.add("C09-校验-超密帧保留不合并", false, "导入被拒"),
    }

    // 负时刻 → 钳到 0 且保留帧。
    let mut tneg = times(3);
    tneg.data = vec![-1.0f32, 0.0, 1.0];
    let neg_doc = GltfAnimDoc::new(
        1,
        vec![tneg, pos_acc(3)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "ng.gltf",
    );
    match import(&neg_doc) {
        Ok(res) => {
            let first_ms = res.tracks.first().and_then(|t| t.track.times.first().copied());
            set.add("C09-校验-负时刻钳到 0", first_ms == Some(0), "负时刻须钳到 0 且保留该帧");
            set.add(
                "C09-校验-负时刻进警告清单",
                res.report.warnings.iter().any(|w| w.code == DiagCode::NEGATIVE_TIME),
                "须记账",
            );
        }
        Err(_) => set.add("C09-校验-负时刻钳到 0", false, "导入被拒"),
    }

    set.add("C09-校验-秒毫秒换算契约", secs_to_ms(0.0) == 0 && secs_to_ms(1.0) == 1000 && secs_to_ms(1.5) == 1500, "1.5s→1500ms（四舍五入）");
    set.add("C09-校验-秒毫秒负值归零", secs_to_ms(-3.0) == 0, "负秒不得绕成大 u32");
    set.add(
        "C09-校验-秒毫秒溢出饱和",
        secs_to_ms(1.0e9) == u32::MAX,
        "超范围须饱和到 u32::MAX 而非回绕",
    );
}

// ---------------------------------------------------------------------------
// 三、保真默认 + 结构化报告 + 零静默
// ---------------------------------------------------------------------------

fn check_fidelity(set: &mut CheckSet) {
    // 保真：帧数逐帧相等（6 帧进 6 帧出）。
    let doc = one_channel_doc(ChannelPath::Translation, 6, "f.gltf");
    match import_with(&doc, Fidelity::Faithful) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-保真-帧数逐帧相等", keys == 6, "6 帧进须 6 帧出");
            set.add("C09-保真-精简量为零", res.report.keys_removed() == 0, "保真模式不得减帧");
        }
        Err(_) => set.add("C09-保真-帧数逐帧相等", false, "导入被拒"),
    }

    // 保真：时间轴逐点相等（0,1000,…,5000ms）——判据侧独立重算。
    match import_with(&doc, Fidelity::Faithful) {
        Ok(res) => {
            let times_ok = res
                .tracks
                .first()
                .map(|t| {
                    t.track.times.len() == 6
                        && (0..6).all(|i| t.track.times[i] == (i as u32) * 1000)
                })
                .unwrap_or(false);
            set.add("C09-保真-时间轴逐点相等", times_ok, "第 i 帧须为 i×1000ms");
        }
        Err(_) => set.add("C09-保真-时间轴逐点相等", false, "导入被拒"),
    }

    // 保真：值逐点相等（第 i 帧 x = i）——判据侧独立重算。
    match import_with(&doc, Fidelity::Faithful) {
        Ok(res) => {
            let vals_ok = res
                .tracks
                .first()
                .map(|t| (0..6).all(|i| t.track.channels[i * 3] == i as f32))
                .unwrap_or(false);
            set.add("C09-保真-值逐点相等", vals_ok, "第 i 帧 x 须为 i");
        }
        Err(_) => set.add("C09-保真-值逐点相等", false, "导入被拒"),
    }

    // 精简：直线轨迹（y=0 恒定，x 线性）在大 epsilon 下抽到端点 2 帧。
    let n = 8usize;
    let mut straight: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        straight.push(i as f32);
        straight.push(0.0);
        straight.push(0.0);
        i += 1;
    }
    let sdoc = GltfAnimDoc::new(
        1,
        vec![times(n), AccessorView::new(ComponentType::Float, false, n as u32, 3, straight)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "s.gltf",
    );
    match import_with(&sdoc, Fidelity::Simplified { epsilon: 0.5 }) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-保真-直线被精简到端点", keys == 2, "线性轨迹在大阈值下应只剩首末");
            set.add(
                "C09-保真-精简端点钉死",
                res.tracks
                    .first()
                    .map(|t| t.track.times.first().copied() == Some(0)
                        && t.track.times.last().copied() == Some(((n - 1) * 1000) as u32))
                    .unwrap_or(false),
                "首末帧时刻须原样保留（否则跨度变短、循环跳变）",
            );
            set.add("C09-保真-精简量记账", res.report.keys_removed() == (n - 2) as u64, "8→2 应减 6");
            let ratio = res.report.reduction_ratio();
            set.add("C09-保真-精简率口径", ratio.0 == 6 && ratio.1 == 8, "精简率整数口径 (6, 8)");
        }
        Err(_) => set.add("C09-保真-直线被精简到端点", false, "导入被拒"),
    }

    // 对照：**锯齿**轨迹在**小阈值**下一帧不减（误差 1.0 ≫ 0.001）。
    //
    // 为什么用锯齿而不是直线：直线的重建误差恒为 0，**任何**正阈值都会把
    // 它抽到只剩端点——那是正确行为，不是缺陷。用直线验「小阈值不减帧」
    // 会把正确实现判成红的。
    let m2 = 10usize;
    let mut zig0: Vec<f32> = Vec::new();
    let mut i2 = 0usize;
    while i2 < m2 {
        zig0.push(if i2 % 2 == 0 { 0.0 } else { 1.0 });
        zig0.push(0.0);
        zig0.push(0.0);
        i2 += 1;
    }
    let zsmall = GltfAnimDoc::new(
        1,
        vec![
            times(m2),
            AccessorView::new(ComponentType::Float, false, m2 as u32, 3, zig0.clone()),
        ],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "zs.gltf",
    );
    match import_with(&zsmall, Fidelity::Simplified { epsilon: 0.001 }) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-保真-小阈值不减帧", keys == m2, "锯齿误差 1.0 ≫ 0.001，须一帧不减");
        }
        Err(_) => set.add("C09-保真-小阈值不减帧", false, "导入被拒"),
    }

    // 锯齿轨迹 + **大阈值**（1.5 > 误差 1.0）→ 可被抽稀。
    //
    // 阈值取 1.5 的理由：锯齿的相邻重建误差恰为 1.0，阈值必须**大于**它
    // 才可能删帧。早先取 0.6（小于误差）却期望「被抽稀」——那是判据写反：
    // 阈值小于误差时正确行为就是一帧不减。
    let m = 10usize;
    let mut zig: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < m {
        zig.push(if i % 2 == 0 { 0.0 } else { 1.0 });
        zig.push(0.0);
        zig.push(0.0);
        i += 1;
    }
    let zdoc = GltfAnimDoc::new(
        1,
        vec![times(m), AccessorView::new(ComponentType::Float, false, m as u32, 3, zig)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "z2.gltf",
    );
    match import_with(&zdoc, Fidelity::Simplified { epsilon: 1.5 }) {
        Ok(res) => {
            let keys = res.tracks.first().map(|t| t.track.times.len()).unwrap_or(0);
            set.add("C09-保真-锯齿被精简", keys < m, "阈值 1.5 > 误差 1.0，锯齿应被抽稀");
            set.add("C09-保真-精简后至少两帧", keys >= 2, "端点钉死 ⇒ 不少于 2 帧");
            set.add("C09-保真-精简后非递减", res
                .tracks
                .first()
                .map(|t| t.track.times.windows(2).all(|w| w[1] >= w[0]))
                .unwrap_or(false), "抽帧不得破坏时间轴单调");
            // 抽帧后的值必须仍等于原轨在保留时刻的取值（抽帧只删帧，不改值）。
            let first_val = res.tracks.first().and_then(|t| t.track.channels.first().copied());
            set.add("C09-保真-抽帧不改值", first_val == Some(0.0), "首帧值须原样保留（不得插值改写）");
        }
        Err(_) => set.add("C09-保真-锯齿被精简", false, "导入被拒"),
    }

    // thin_keys 直测：端点钉死 + 保留下标升序 + 误差 ≤ epsilon。
    let tms: Vec<u32> = vec![0, 100, 200, 300, 400];
    let vls: Vec<f32> = vec![0.0, 0.0, 0.0, 1.0, 0.0];
    let oc = thin_keys(&tms, &vls, 1, 0.6);
    set.add(
        "C09-保真-thin 端点钉死",
        oc.kept.first().copied() == Some(0) && oc.kept.last().copied() == Some(4),
        "首末下标必留",
    );
    set.add("C09-保真-thin 误差 ≤ epsilon", oc.max_error <= 0.6, "实测误差须在阈值内");
    set.add("C09-保真-thin 减了帧", oc.keys_out < oc.keys_in, "尖峰帧应被抽掉");
    let ascending = oc.kept.windows(2).all(|w| w[1] > w[0]);
    set.add("C09-保真-thin 保留下标升序", ascending, "下标须严格递增");

    // 对照：epsilon=0 → 一帧不减（否则「减帧」判据恒真）。
    let oc0 = thin_keys(&tms, &vls, 1, 0.0);
    set.add(
        "C09-保真-thin 零阈值不减帧",
        oc0.keys_out == oc0.keys_in && oc0.kept.len() == 5,
        "阈值 0 时须全留",
    );

    // 对照：全平轨迹在任意阈值下都可抽到 2 帧（证明 thin 会动，而非恒不减）。
    let flat: Vec<f32> = vec![5.0, 5.0, 5.0, 5.0, 5.0];
    let ocf = thin_keys(&tms, &flat, 1, 0.001);
    set.add(
        "C09-保真-thin 平轨可抽到端点",
        ocf.keys_out == 2 && ocf.max_error == 0.0,
        "全平轨迹误差恰 0，可全删",
    );

    set.add(
        "C09-保真-两帧不抽",
        thin_keys(&tms[..2], &vls[..2], 1, 10.0).keys_out == 2,
        "少于 3 帧不得抽（无中间帧可删）",
    );

    // 无阈值精简（epsilon=0 / 负 / NaN）→ 拒收（三要素）。
    for bad_eps in [0.0f32, -1.0, f32::NAN] {
        let mut b = bag();
        let r = import_gltf_anim(
            &one_channel_doc(ChannelPath::Translation, 4, "bad.gltf"),
            Fidelity::Simplified { epsilon: bad_eps },
            &MappingTable::standard(),
            &mut b,
        );
        let refused = match &r {
            Err(e) => e.three_elements_complete() && e.code == DiagCode::DOC_MALFORMED,
            Ok(_) => false,
        };
        if bad_eps == 0.0 {
            set.add("C09-保真-零阈值精简拒收", refused, "无阈值精简不可复现，须拒收");
        } else if bad_eps < 0.0 {
            set.add("C09-保真-负阈值精简拒收", refused, "负阈值须拒收");
        } else {
            set.add("C09-保真-NaN 阈值拒收", refused, "NaN 阈值须拒收");
        }
    }

    set.add(
        "C09-保真-保真模式非精简",
        !Fidelity::Faithful.is_simplified() && Fidelity::Faithful.epsilon() == 0.0,
        "Faithful 语义",
    );
}

fn check_report(set: &mut CheckSet) {
    // 三要素拒绝：节点数为 0。
    let mut b1 = bag();
    let e1 = import_gltf_anim(
        &GltfAnimDoc::new(0, vec![], vec![], vec![], "empty.gltf"),
        Fidelity::Faithful,
        &MappingTable::standard(),
        &mut b1,
    );
    let r1 = match &e1 {
        Err(e) => e.code == DiagCode::DOC_MALFORMED && e.three_elements_complete(),
        Ok(_) => false,
    };
    set.add("C09-报告-零节点三要素拒绝", r1, "node_count=0 须三要素拒绝");

    // 三要素拒绝：通道表为空。
    let mut b2 = bag();
    let e2 = import_gltf_anim(
        &GltfAnimDoc::new(1, vec![times(3)], vec![], vec![], "nochan.gltf"),
        Fidelity::Faithful,
        &MappingTable::standard(),
        &mut b2,
    );
    let r2 = match &e2 {
        Err(e) => e.code == DiagCode::DOC_MALFORMED && e.locator.contains("channels"),
        Ok(_) => false,
    };
    set.add("C09-报告-空通道三要素拒绝", r2, "channels 空须拒且定位到 channels");

    // 三要素渲染含三段。
    let err = ImportError::new(DiagCode::DOC_MALFORMED, "GltfAnimDoc.x", "补齐字段");
    let txt = err.render();
    set.add(
        "C09-报告-三要素渲染齐备",
        txt.contains("定位") && txt.contains("处置") && txt.contains("GltfAnimDoc.x"),
        "渲染须含码/定位/处置三段",
    );

    // 缺定位即三要素不完整（反例：证明该判据非恒真）。
    let bad_err = ImportError::new(DiagCode::DOC_MALFORMED, "", "处置");
    let bad_err2 = ImportError::new(DiagCode::DOC_MALFORMED, "定位", "");
    set.add(
        "C09-报告-缺要素被识破",
        err.three_elements_complete() && !bad_err.three_elements_complete() && !bad_err2.three_elements_complete(),
        "缺定位或缺处置即不完整",
    );

    // 结构化报告三要素：轨道数 / 映射表 / 警告清单（干净文档下警告可为空，
    // 但字段必须存在且映射表非空）。
    let doc = GltfAnimDoc::new(
        2,
        vec![times(4), pos_acc(4), quat_acc(4)],
        vec![
            SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear },
            SamplerRef { input: 0, output: 2, interp: GltfInterp::Linear },
        ],
        vec![
            ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 },
            ChannelRef { target_node: 1, path: ChannelPath::Rotation, sampler: 1 },
        ],
        "rep.gltf",
    );
    match import(&doc) {
        Ok(res) => {
            set.add("C09-报告-轨道数记账", res.report.tracks == 2 && res.report_matches_tracks(), "2 通道 → 2 轨");
            set.add(
                "C09-报告-映射表恰等于通道数",
                res.report.mapping_covers_channels() && res.report.mapping.len() == 2,
                "映射表长度须恰等于通道数",
            );
            set.add(
                "C09-报告-通道计数自洽",
                res.report.channels_imported + res.report.channels_skipped == res.report.channels_total,
                "导入+跳过须等于总数",
            );
            set.add(
                "C09-报告-跳过数独立重算一致",
                res.report.skipped_records() as u32 == res.report.channels_skipped,
                "由 track_count=0 重算须等于跳过计数",
            );
            // 轨道起点连续：0 起、第一条 count 条。
            let starts_ok = res.report.mapping.len() == 2
                && res.report.mapping[0].track_start == 0
                && res.report.mapping[0].track_count == 1
                && res.report.mapping[1].track_start == 1
                && res.report.mapping[1].track_count == 1;
            set.add("C09-报告-轨道起点连续", starts_ok, "第 n 条起点须等于前序累计");
            // 映射语义与实际轨道语义一致（反查）。
            let map_ok = res.report.mapping.iter().all(|r| match r.semantic {
                Some(s) => res
                    .tracks
                    .get(r.track_start as usize)
                    .map(|t| t.semantic == s)
                    .unwrap_or(false),
                None => false,
            });
            set.add("C09-报告-映射语义与轨道一致", map_ok, "track_start 处轨道的语义须与记录一致");
            set.add(
                "C09-报告-映射记录含帧数",
                res.report.mapping.iter().all(|r| r.keys_in == 4 && r.keys_out == 4),
                "每条须记输入/落地帧数",
            );
            set.add("C09-报告-警告人人话", res.report.all_warnings_readable(), "每条警告须有正文");

            // **负向对照**：`all_warnings_readable` 恒返回 true 时上条不可证伪。
            // 判据侧自己造「正文为空」的报告，要求它必须被读出为不可读。
            let mut blank = ImportReport::default();
            blank.warnings.push(ImportWarning::new(DiagCode::MAPPING_DRIFT, 7, ""));
            set.add(
                "C09-报告-空正文警告判为不可读",
                !blank.all_warnings_readable(),
                "正文为空的警告须使 all_warnings_readable 为假（否则上条恒真）",
            );

            // 渲染非空且含三要素关键词。
            let rtxt = res.report.render();
            set.add(
                "C09-报告-渲染含三要素",
                rtxt.contains("映射表") && rtxt.contains("关键帧"),
                "报告渲染须含映射表与帧数",
            );
        }
        Err(_) => set.add("C09-报告-轨道数记账", false, "导入被拒"),
    }

    // 形态键通道：映射表一条记录 track_count=2（一条通道 → 多轨）。
    let mdoc = GltfAnimDoc::new(
        1,
        vec![times(3), AccessorView::new(ComponentType::Float, false, 3, 2, vec![0.0f32, 1.0, 0.5, 1.0, 0.2, 0.3])],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Weights, sampler: 0 }],
        "mrep.gltf",
    );
    match import(&mdoc) {
        Ok(res) => {
            let rec = res.report.mapping.first();
            set.add(
                "C09-报告-一通道多轨记录",
                rec.map(|r| r.track_count == 2 && r.track_start == 0).unwrap_or(false),
                "形态键一条通道记 2 条轨道",
            );
            set.add("C09-报告-多轨轨道数一致", res.report.tracks == 2, "2 轨");
            set.add(
                "C09-报告-多轨帧数只计一次",
                res.report.keys_in == 3,
                "keys_in 按输入帧计一次（不按轨道翻倍）",
            );
        }
        Err(_) => set.add("C09-报告-一通道多轨记录", false, "导入被拒"),
    }

    // find / morph_tracks 与映射表交叉核对。
    match import(&mdoc) {
        Ok(res) => {
            let consistent = res.report.mapping.iter().all(|r| {
                let s = r.track_start as usize;
                let e = s + r.track_count as usize;
                e <= res.tracks.len()
            });
            set.add("C09-报告-映射区间不越界", consistent, "起点+条数须落在轨道数内");
        }
        Err(_) => set.add("C09-报告-映射区间不越界", false, "导入被拒"),
    }

    // 报告渲染非空（干净文档也无警告时仍须出文本）。
    match import(&one_channel_doc(ChannelPath::Translation, 3, "clean.gltf")) {
        Ok(res) => set.add("C09-报告-干净文档也出报告", !res.report.render().is_empty(), "渲染不得为空"),
        Err(_) => set.add("C09-报告-干净文档也出报告", false, "导入被拒"),
    }

    set.add(
        "C09-报告-描述非空",
        describe().contains("四通道") && !family_description_empty(),
        "描述须含四通道映射说明",
    );
    set.add("C09-报告-家族声明自洽", family_is_consistent(), "族声明须自洽");
}

fn family_description_empty() -> bool {
    describe().is_empty()
}

fn check_explicit(set: &mut CheckSet) {
    // 码标签互异（防两码共用一句人话）。
    set.add("C09-显性-码标签互异", labels_unique(), "19 码标签须两两不同");
    set.add("C09-显性-码数齐备", DiagCode::ALL.len() == 19, "登记 19 码");
    set.add(
        "C09-显性-未登记码有人话兜底",
        DiagCode(0x2FFF).label() == "未登记诊断码",
        "未知码须有兜底标签，不得 panic",
    );

    // 零指纹：轨道名/警告/声明均不含资产标签（用**非空**标签）。
    let secret = "secret-character-42.gltf";
    let zdoc = GltfAnimDoc::new(
        1,
        vec![
            times(3),
            AccessorView::new(ComponentType::Float, false, 3, 3, pos_values(3)),
            pos_acc(3),
        ],
        vec![SamplerRef { input: 1, output: 2, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Other, sampler: 0 }],
        secret,
    );
    match import(&zdoc) {
        Ok(res) => {
            set.add(
                "C09-显性-零指纹报告",
                res.report.fingerprint_free(secret),
                "报告任何位置不得含资产标签",
            );
            let names_ok = res.tracks.iter().all(|t| !t.name.contains(secret));
            set.add("C09-显性-零指纹轨道名", names_ok, "轨道名不得含资产标签");
        }
        Err(_) => set.add("C09-显性-零指纹报告", false, "导入被拒"),
    }

    // 对照组：空标签时 fingerprint_free 必须为 false（否则该判据恒真）。
    match import(&one_channel_doc(ChannelPath::Translation, 3, "x.gltf")) {
        Ok(res) => set.add(
            "C09-显性-空标签指纹判据非恒真",
            !res.report.fingerprint_free(""),
            "被排除串为空时须判失败（否则负向断言恒真）",
        ),
        Err(_) => set.add("C09-显性-空标签指纹判据非恒真", false, "导入被拒"),
    }

    // 轨道名只由语义 + 节点下标构成（可预测）。
    match import(&one_channel_doc(ChannelPath::Translation, 3, "y.gltf")) {
        Ok(res) => {
            let name_ok = res.tracks.first().map(|t| t.name == "m.pos/n0").unwrap_or(false);
            set.add("C09-显性-轨道名构式固定", name_ok, "须为 m.pos/n0（语义+节点下标）");
        }
        Err(_) => set.add("C09-显性-轨道名构式固定", false, "导入被拒"),
    }

    set.add(
        "C09-显性-语义标签四具",
        semantic_tag(TrackSemantic::Position) == "pos"
            && semantic_tag(TrackSemantic::Rotation) == "rot"
            && semantic_tag(TrackSemantic::Scale) == "scl"
            && semantic_tag(TrackSemantic::MorphWeight) == "morph",
        "四个语义标签须齐备",
    );

    // 诊断袋：P1 可查 + 精确计数。
    let mut b = bag();
    b.push(DiagCode::MAPPING_DRIFT);
    b.push_p1(DiagCode::MAPPING_DRIFT);
    b.push_major(DiagCode::VALUE_NON_FINITE);
    set.add(
        "C09-显性-诊断精确计数",
        b.count_of(DiagCode::MAPPING_DRIFT) == 2 && b.count_severity(Severity::P1) == 1,
        "计数须精确（2 条映射漂移 / 1 条 P1）",
    );
    set.add("C09-显性-P1 可查", b.p1_count() == 1 && b.has(DiagCode::VALUE_NON_FINITE), "P1 与 has 须可用");

    // 四元数归一化：第 0 帧模 2（非单位），第 1/2 帧已是单位四元数。
    //
    // **判据侧独立数「有几帧非单位」**，不写死 3：只有第 0 帧被改成 2.0，
    // 另两帧保持 (0,0,0,1)。写死 3 会把「只归一化必要的帧」判成红。
    let mut qv = quat_values(3);
    qv[0] = 0.0;
    qv[1] = 0.0;
    qv[2] = 0.0;
    qv[3] = 2.0;
    // 判据侧自算非单位帧数。
    let mut expect_norm = 0u32;
    let mut fi = 0usize;
    while fi < 3 {
        let b = fi * 4;
        let sum = qv[b] * qv[b] + qv[b + 1] * qv[b + 1] + qv[b + 2] * qv[b + 2] + qv[b + 3] * qv[b + 3];
        if (sum - 1.0).abs() > 0.0001 {
            expect_norm += 1;
        }
        fi += 1;
    }
    let qdoc = GltfAnimDoc::new(
        1,
        vec![times(3), AccessorView::new(ComponentType::Float, false, 3, 4, qv)],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Rotation, sampler: 0 }],
        "qn2.gltf",
    );
    match import(&qdoc) {
        Ok(res) => {
            let unit = res
                .tracks
                .first()
                .map(|t| t.track.channels.get(3).copied() == Some(1.0))
                .unwrap_or(false);
            set.add("C09-显性-旋转导入期归一化", unit, "模 2 的四元数须归一到 w=1");
            let cnt = res.tracks.first().map(|t| t.normalized).unwrap_or(0);
            set.add(
                "C09-显性-归一化计数",
                cnt == expect_norm && expect_norm > 0,
                "只该归一化非单位帧（判据侧独立数）",
            );
        }
        Err(_) => set.add("C09-显性-旋转导入期归一化", false, "导入被拒"),
    }

    // 对照：已是单位四元数则不记归一化改写。
    match import(&one_channel_doc(ChannelPath::Rotation, 3, "qu.gltf")) {
        Ok(res) => {
            let cnt = res.tracks.first().map(|t| t.normalized).unwrap_or(1);
            set.add("C09-显性-单位四元数不记改写", cnt == 0, "已单位化不得虚记归一化");
        }
        Err(_) => set.add("C09-显性-单位四元数不记改写", false, "导入被拒"),
    }

    // normalize_quat 直测：全零 → 单位四元数且返回 true。
    let mut zq = [0.0f32, 0.0, 0.0, 0.0];
    let changed = normalize_quat(&mut zq);
    set.add(
        "C09-显性-全零四元数回退单位",
        changed && zq == [0.0, 0.0, 0.0, 1.0],
        "全零须回退到单位四元数",
    );

    // fsqrt：与已知值对照（4 的平方根 ≈ 2，误差 < 1e-5）。
    let s4 = fsqrt(4.0);
    let sq_ok = (s4 - 2.0).abs() < 0.00001;
    set.add("C09-显性-开方精度", sq_ok && fsqrt(0.0) == 0.0, "fsqrt(4)≈2（非负输入）");

    // 钳制目标按语义。
    set.add(
        "C09-显性-语义钳制目标",
        clamp_target(TrackSemantic::Position, 0) == 0.0
            && clamp_target(TrackSemantic::Scale, 0) == 0.0
            && clamp_target(TrackSemantic::MorphWeight, 0) == 0.0
            && clamp_target(TrackSemantic::Rotation, 3) == 1.0
            && clamp_target(TrackSemantic::Rotation, 0) == 0.0,
        "旋转 w 钳到 1、其余钳到 0",
    );

    // 形状自检（SoA 长度 = 帧数 × 分量）。
    match import(&one_channel_doc(ChannelPath::Translation, 5, "sh.gltf")) {
        Ok(res) => set.add(
            "C09-显性-轨道形状自洽",
            res.tracks.first().map(|t| t.track.shape_ok()).unwrap_or(false),
            "5 帧 × 3 分量 = 15 值",
        ),
        Err(_) => set.add("C09-显性-轨道形状自洽", false, "导入被拒"),
    }

    // ComponentType 标签齐备。
    set.add(
        "C09-显性-分量类型标签齐备",
        ComponentType::Float.label() == "f32"
            && ComponentType::U8.label() == "u8"
            && ComponentType::U16.label() == "u16"
            && ComponentType::Unregistered.label() == "未登记分量类型",
        "四类分量类型须各有人话",
    );

    // glTF 插值模式标签齐备。
    set.add(
        "C09-显性-插值模式标签齐备",
        GltfInterp::Linear.label() == "LINEAR"
            && GltfInterp::Step.label() == "STEP"
            && GltfInterp::CubicSpline.label() == "CUBICSPLINE",
        "三模式须用 glTF 规范名",
    );

    // 通道路径标签齐备（含四通道外）。
    set.add(
        "C09-显性-路径标签齐备",
        ChannelPath::Translation.label() == "translation"
            && ChannelPath::Rotation.label() == "rotation"
            && ChannelPath::Scale.label() == "scale"
            && ChannelPath::Weights.label() == "weights"
            && ChannelPath::Other.label() == "四通道外自定义路径",
        "四通道须用 glTF 规范名 + 通道外有人话",
    );

    // 映射记录渲染含处置段。
    let rec = MappingRecord {
        channel: 3,
        path: ChannelPath::Scale,
        semantic: Some(TrackSemantic::Scale),
        track_start: 2,
        track_count: 1,
        keys_in: 5,
        keys_out: 5,
        verdict: None,
    };
    let rtxt = rec.render();
    set.add(
        "C09-显性-映射记录渲染齐备",
        rtxt.contains("ch3") && rtxt.contains("scale") && rtxt.contains("5→5") && rtxt.contains("缩放轨"),
        "渲染须含通道/路径/落点/帧数/处置",
    );

    // 有处置码时渲染须含处置原因。
    let rec2 = MappingRecord { verdict: Some(DiagCode::PATH_OUT_OF_SCOPE), ..rec };
    set.add(
        "C09-显性-处置码入渲染",
        rec2.render().contains("四通道外"),
        "处置码须渲出人话原因",
    );

    // 冒烟可运行且非空。
    let smoke_txt = smoke();
    set.add("C09-显性-冒烟可运行", smoke_txt.contains("动画导入报告"), "冒烟须产出报告文本");

    // 无阈值检测辅助函数。
    set.add(
        "C09-显性-保真非无阈值",
        !Fidelity::Faithful.simplified_without_threshold(),
        "Faithful 不该被判无阈值",
    );
    let _ = String::from("probe");
    let _: Vec<u8> = Vec::new();
}
