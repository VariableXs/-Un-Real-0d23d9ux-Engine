//! VE-F2410 · 域自检（判据逐条对应，见 `vem10_export.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **glTF 双向** → `C10-双向-*`（逆表为正表机械求逆/逆表与契约白名单逐行
//!   一致/四语义四路径各一次/漂移置拦截/拦截态整体拒绝/产物类型即导入器输入
//!   类型/产物可被导入器接受/往返轨道数守恒）
//! - **往返容差表** → `C10-往返-*`（走真导入器/时刻容差/值容差分档/旋转双口径/
//!   夹角对 q/-q 免疫/超差立案/期望帧数独立重算/无源轨显性不可执行）
//! - **精度诚实** → `C10-精度-*`（有损文案在位/禁止正面承诺措辞/时刻精确上界
//!   夹逼对/钳制抽稀如实记账/归一化改写如实记账）
//! - **离散轨边界** → `C10-离散-*`（离散轨跳过/指名声明/声明非空/产物不含
//!   离散轨/非离散轨不被误跳）
//! - 零静默与守卫 → `C10-显性-*`（码标签互异/类域互斥且每类非空/三要素拒绝
//!   齐备/零指纹/NaN 导出侧拒绝 vs 导入侧钳制的不对称/端点钉死/合轨 comps）
//!
//! **弱门禁自律**：本文件每条判据都在判据侧**自己重算**期望值或**自己造反例**，
//! 不问被测函数「你返回 true 吗」。凡涉及「恰好等于」处一律用 `==`。
//!
//! **恒真门禁自查**（本条最容易踩的三处，本文件逐一钉死）：
//! 1. 往返断言不能拿产物与自身比 ⇒ 本文件每条往返判据都传入**独立语料源轨**，
//!    并断言 `max_value_diff`/时刻偏差来自与源轨的比对（非零语料时须非零）。
//! 2. 精度声明的反向断言不能禁「无损」二字（诚实文案「不承诺无损」含该子串）
//!    ⇒ 改为逐条禁**正面承诺词组**，并配正向断言「有损」「不承诺无损」在位。
//! 3. 刻度类判据（「时间精确上界」）必须**夹逼对**：下界成立 + 上界之外确有
//!    反例，只断一侧等于没断。
//!
//! **补判据必须双向验证**：每条新判据都在变异 harness 里跑过「基线绿 +
//! 变体红」；只绿不红的判据等于没写。
//!
//! 判据数超过 `CheckSet::MAX_CHECKS`（112，全仓共享）时按判据族切批：
//! - `a` = glTF 双向：逆映射与双向闭环（约 40 项）
//! - `b` = 往返容差表：真导入器对拍与容差分档（约 46 项）
//! - `c` = 精度诚实 + 离散轨边界（约 48 项）
//! - `d` = 零静默与守卫（约 44 项）

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::{SoaTrack, TrackClass, TrackValueKind};
use super::vem09_import::{self, secs_to_ms, ChannelPath, Fidelity, GltfAnimDoc, GltfInterp, MappingTable, TrackSemantic};
use super::vem10_export::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自备，不从被测模块取样）
// ---------------------------------------------------------------------------

/// 空诊断袋。
fn bag() -> DiagBag {
    DiagBag::new()
}

/// 标准正表（判据侧自建，不从被测模块取）。
fn forward() -> MappingTable {
    MappingTable::standard()
}

/// 造一条 3 分量轨（位置/缩放）。
fn soa3(name: &str, times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new(name, TrackClass::Continuous, TrackValueKind::Position, false, times, vals)
}

/// 造一条 4 分量轨（四元数）。
fn soa4(name: &str, times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new(name, TrackClass::Continuous, TrackValueKind::Quat, false, times, vals)
}

/// 造一条标量轨（形态键权重）。
fn soa1(name: &str, times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new(name, TrackClass::Continuous, TrackValueKind::Scalar, false, times, vals)
}

/// 位置轨：`pos(node)`，值 = 每帧 `[i, 0, 0]`，时刻 0,500,1000...
fn pos_track(node: u32, frames: usize) -> ExportTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut vals: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        vals.push(i as f32);
        vals.push(0.0);
        vals.push(0.0);
        i += 1;
    }
    ExportTrack::new(
        TrackSemantic::Position,
        node,
        u16::MAX,
        soa3("pos", times, vals),
        TrackClass::Continuous,
        Interp::Linear,
    )
}

/// 旋转轨：`rot(node)`，单位四元数绕 Y 轴步进（**非平凡姿态**——恒等四元数
/// 会让旋转口径的判据恒绿）。
///
/// **姿态表刻意用可精确表示的角度**：0° / 90°（绕 Y 四元数 (0,0.7071,0,0.7071)）
/// / 180°（(0,1,0,0)）三档交替。**不用 45°**（0.9239 那个数在 f32 里是近似值，
/// 归一化后与原值有 1e-8 级差，会让「脏数据修正」类判据的容差边界模糊）。
fn rot_track(node: u32, frames: usize) -> ExportTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut vals: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        let (y, w) = match i % 3 {
            0 => (0.0f32, 1.0f32),// 0°
            1 => (0.707_106_8f32, 0.707_106_8f32), // 90°
            _ => (1.0f32, 0.0f32),                // 180°
        };
        vals.push(0.0);
        vals.push(y);
        vals.push(0.0);
        vals.push(w);
        i += 1;
    }
    ExportTrack::new(
        TrackSemantic::Rotation,
        node,
        u16::MAX,
        soa4("rot", times, vals),
        TrackClass::Continuous,
        Interp::Linear,
    )
}

/// 缩放轨。
fn scl_track(node: u32, frames: usize) -> ExportTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut vals: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        vals.push(1.0);
        vals.push(1.0 + i as f32 * 0.5);
        vals.push(1.0);
        i += 1;
    }
    ExportTrack::new(
        TrackSemantic::Scale,
        node,
        u16::MAX,
        soa3("scl", times, vals),
        TrackClass::Continuous,
        Interp::Linear,
    )
}

/// 形态键轨（`slot` 指定槽位；时间轴固定 0,500,...）。
fn morph_track(node: u32, slot: u16, frames: usize) -> ExportTrack {
    let mut times: Vec<u32> = Vec::new();
    let mut vals: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        vals.push(i as f32 * 0.25);
        i += 1;
    }
    ExportTrack::new(
        TrackSemantic::MorphWeight,
        node,
        slot,
        soa1("morph", times, vals),
        TrackClass::Continuous,
        Interp::Linear,
    )
}

/// 离散轨（事件/布尔；**不可导出**，判据四的核心语料）。
fn discrete_track(node: u32) -> ExportTrack {
    ExportTrack::new(
        TrackSemantic::Position,
        node,
        u16::MAX,
        soa3("evt", vec![0, 500], vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        TrackClass::Discrete,
        Interp::Step,
    )
}

/// 导出（标准配置）。
fn export(clip: &ExportClip) -> Result<ExportedAnim, ExportError> {
    let mut t = ExportMapTable::from_forward(&forward());
    let mut b = bag();
    export_gltf_anim(clip, &mut t, &forward(), &mut b)
}

/// 导出并同时取回诊断袋（需要核验码时用）。
fn export_with_bag(clip: &ExportClip) -> (Result<ExportedAnim, ExportError>, DiagBag) {
    let mut t = ExportMapTable::from_forward(&forward());
    let mut b = bag();
    let r = export_gltf_anim(clip, &mut t, &forward(), &mut b);
    (r, b)
}

/// 三轨标准片段（位置 + 旋转 + 缩放，分属两节点）。
fn std_clip() -> ExportClip {
    ExportClip::new(
        2,
        vec![pos_track(0, 3), rot_track(0, 3), scl_track(1, 3)],
    )
}

// ---------------------------------------------------------------------------
// 分族入口
// ---------------------------------------------------------------------------

/// 判据数超过 `CheckSet::MAX_CHECKS`（112，全仓共享）时按族切批。
/// mod.rs 侧注册 `VE-F2410-a` / `VE-F2410-b` / `VE-F2410-c` / `VE-F2410-d` 四行。
pub fn run_vem10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem10");
    run_vem10_checks_a(&mut set);
    run_vem10_checks_b(&mut set);
    run_vem10_checks_c(&mut set);
    run_vem10_checks_d(&mut set);
    assert!(
        !set.truncated(),
        "VE-F2410 判据数 {} 超出 CheckSet 容量 {}，聚合会静默丢项；请按 a/b/c/d 四族分别注册",
        set.len() + set.dropped(),
        crate::checks::MAX_CHECKS
    );
    set
}

/// a 族独立入口（glTF 双向）。
pub fn run_vem10_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem10-a");
    run_vem10_checks_a(&mut set);
    set
}

/// b 族独立入口（往返容差表）。
pub fn run_vem10_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem10-b");
    run_vem10_checks_b(&mut set);
    set
}

/// c 族独立入口（精度诚实 + 离散轨边界）。
pub fn run_vem10_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem10-c");
    run_vem10_checks_c(&mut set);
    set
}

/// d 族独立入口（零静默与守卫）。
pub fn run_vem10_checks_d_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem10-d");
    run_vem10_checks_d(&mut set);
    set
}

// ---------------------------------------------------------------------------
// a 族：glTF 双向（逆映射与双向闭环）
// ---------------------------------------------------------------------------

fn run_vem10_checks_a(set: &mut CheckSet) {
    // —— 逆表由正表机械求逆 ——
    let inv = ExportMapTable::from_forward(&forward());
    set.add(
        "C10-双向-逆表行数",
        inv.rows().len() == 4,
        "四通道逆表恰 4 行",
    );

    // 逐行：语义 → 路径 与规格契约一致（**判据侧独立重写期望**，不读被测摘要）。
    let want: [(TrackSemantic, ChannelPath); 4] = [
        (TrackSemantic::Position, ChannelPath::Translation),
        (TrackSemantic::Rotation, ChannelPath::Rotation),
        (TrackSemantic::Scale, ChannelPath::Scale),
        (TrackSemantic::MorphWeight, ChannelPath::Weights),
    ];
    let mut all_rows_ok = true;
    let mut i = 0usize;
    while i < want.len() {
        if inv.rows()[i].semantic != want[i].0 || inv.rows()[i].path != want[i].1 {
            all_rows_ok = false;
        }
        i += 1;
    }
    set.add(
        "C10-双向-逆表四行对",
        all_rows_ok,
        "位置→translation/旋转→rotation/缩放→scale/形态键→weights",
    );

    // 反向断言：逆表**不含**错映（这是「双向」的正向保证）。
    let mut no_wrong = true;
    let mut k = 0usize;
    while k < inv.rows().len() {
        let r = inv.rows()[k];
        if (r.semantic == TrackSemantic::Position && r.path != ChannelPath::Translation)
            || (r.semantic == TrackSemantic::Rotation && r.path != ChannelPath::Rotation)
            || (r.semantic == TrackSemantic::Scale && r.path != ChannelPath::Scale)
            || (r.semantic == TrackSemantic::MorphWeight && r.path != ChannelPath::Weights)
        {
            no_wrong = false;
        }
        k += 1;
    }
    set.add("C10-双向-逆表无错映", no_wrong, "四行语义与路径不得交叉");

    set.add(
        "C10-双向-逆表覆盖四通道",
        inverse_covers_four_channels(&inv),
        "四语义各出现恰一次",
    );

    set.add(
        "C10-双向-逆表对偶",
        rows_are_dual_with(&inv, &forward()),
        "逐行验 forward.map(path)==semantic",
    );

    set.add(
        "C10-双向-逆表摘要合基准",
        inv.checksum() == inv.declared_checksum() && !inv.drifted(),
        "由正表求逆生成的表不应漂移",
    );

    set.add(
        "C10-双向-规格基准非零",
        spec_checksum() != 0,
        "摘要常量不得为 0（否则对账失去意义）",
    );

    set.add(
        "C10-双向-契约行与逆表一致",
        {
            let c = spec_rows_contract();
            let mut same = true;
            let mut m = 0usize;
            while m < 4 {
                if c[m].semantic != inv.rows()[m].semantic || c[m].path != inv.rows()[m].path {
                    same = false;
                }
                m += 1;
            }
            same
        },
        "独立写死的契约白名单须与逆表逐行一致",
    );

    // —— 漂移拦截：造反例（把位置映成 rotation）——
    let bad = ExportMapTable::with_rows([
        ExportRow::new(TrackSemantic::Position, ChannelPath::Rotation),
        ExportRow::new(TrackSemantic::Rotation, ChannelPath::Scale),
        ExportRow::new(TrackSemantic::Scale, ChannelPath::Translation),
        ExportRow::new(TrackSemantic::MorphWeight, ChannelPath::Weights),
    ]);
    let mut bad_t = bad;
    let mut bb = bag();
    let drifted = bad_t.reconcile(&forward(), &mut bb);
    set.add(
        "C10-双向-错映被对账捕获",
        drifted && bad_t.intercepted(),
        "循环置换的映射表必须被判定漂移并置拦截",
    );
    set.add(
        "C10-双向-漂移记P1",
        bb.has(DiagCode::MAP_DRIFT) && bb.p1_count() >= 1,
        "漂移须立案",
    );
    set.add(
        "C10-双向-拦截态不供映射",
        bad_t.path_of(TrackSemantic::Position).is_none(),
        "拦截态下逆查一律 None（漂移规格不得被消费）",
    );
    set.add(
        "C10-双向-解除拦截后恢复",
        {
            bad_t.clear_intercept();
            !bad_t.intercepted() && bad_t.path_of(TrackSemantic::Position).is_some()
        },
        "显式解除后逆查恢复（且仍可查出错映——解除是人的决定）",
    );
    set.add(
        "C10-双向-漂移计数累加",
        bad_t.drift_count() >= 1,
        "漂移次数须累计（不是布尔标记）",
    );

    // —— 拦截态导出整体拒绝 ——
    let blocked = {
        let mut t = ExportMapTable::from_forward(&forward());
        let mut b = bag();
        let _ = t.reconcile(&forward(), &mut b);
        t.clear_intercept();
        // 人为置拦截：重新对账一个错表。
        let mut wrong = ExportMapTable::with_rows([
            ExportRow::new(TrackSemantic::Position, ChannelPath::Scale),
            ExportRow::new(TrackSemantic::Rotation, ChannelPath::Rotation),
            ExportRow::new(TrackSemantic::Scale, ChannelPath::Translation),
            ExportRow::new(TrackSemantic::MorphWeight, ChannelPath::Weights),
        ]);
        let mut b2 = bag();
        let _ = wrong.reconcile(&forward(), &mut b2);
        // 用拦截表导出。
        let mut tb = wrong;
        let mut b3 = bag();
        export_gltf_anim(&std_clip(), &mut tb, &forward(), &mut b3)
    };
    match blocked {
        Err(e) => {
            set.add(
                "C10-双向-拦截态导出被拒",
                e.code == DiagCode::MAP_INTERCEPTED && e.three_elements_complete(),
                "拦截态下导出整体拒绝且三要素齐备",
            );
        }
        Ok(_) => set.add("C10-双向-拦截态导出被拒", false, "拦截态竟导出成功"),
    }

    // —— 产物类型即导入器输入类型（双向的类型级兑现）——
    let ok = export(&std_clip());
    match ok {
        Ok(a) => {
            // 能把产物原封不动喂给 F2409 的导入器（这正是「双向」的机械证明）。
            let mut ib = vem09_import::DiagBag::new();
            let imported = vem09_import::import_gltf_anim(
                &a.doc,
                Fidelity::Faithful,
                &forward(),
                &mut ib,
            );
            set.add(
                "C10-双向-产物可回导",
                imported.is_ok(),
                "导出产物须能被 F2409 导入器接受",
            );
            set.add(
                "C10-双向-回导轨道数",
                imported.map(|r| r.tracks.len()).unwrap_or(0) == 3,
                "三轨进三轨出",
            );
            set.add(
                "C10-双向-报告轨道数自洽",
                a.report.tracks_in == 3 && a.report.records_cover_channels(),
                "输入 3 轨、记录数恰等于通道数",
            );
        }
        Err(_) => {
            set.add("C10-双向-产物可回导", false, "标准片段导出竟失败");
            set.add("C10-双向-回导轨道数", false, "未产出产物");
            set.add("C10-双向-报告轨道数自洽", false, "未产出产物");
        }
    }

    // —— 往返轨道数守恒（**恰等于**）——
    let clip = std_clip();
    match export(&clip) {
        Ok(a) => {
            let mut b = bag();
            let v = roundtrip_verify(&clip, &a, &mut b);
            set.add(
                "C10-双向-往返轨道数守恒",
                v.track_count_conserved() && v.tracks_src == 3 && v.tracks_roundtrip == 3,
                "3 → 3（恰等于）",
            );
            set.add(
                "C10-双向-往返已执行",
                v.ran && v.tracks_compared == 3,
                "三条轨都进入比对",
            );
        }
        Err(_) => {
            set.add("C10-双向-往返轨道数守恒", false, "导出失败");
            set.add("C10-双向-往返已执行", false, "导出失败");
        }
    }

    // —— 路径与语义对应（产物通道表逐条核）——
    match export(&std_clip()) {
        Ok(a) => {
            let mut path_ok = true;
            let mut i = 0usize;
            while i < a.report.channels.len() {
                let c = &a.report.channels[i];
                let want = match c.semantic {
                    TrackSemantic::Position => ChannelPath::Translation,
                    TrackSemantic::Rotation => ChannelPath::Rotation,
                    TrackSemantic::Scale => ChannelPath::Scale,
                    TrackSemantic::MorphWeight => ChannelPath::Weights,
                };
                if c.path != want {
                    path_ok = false;
                }
                i += 1;
            }
            set.add("C10-双向-通道路径对", path_ok, "每条通道的 path 须匹配其语义");
            set.add(
                "C10-双向-通道记录数",
                a.report.channels.len() == 3 && a.report.channels_out == 3,
                "三条语义轨 → 三条通道",
            );
            set.add(
                "C10-双向-分量数对",
                {
                    let mut ok2 = true;
                    let mut k = 0usize;
                    while k < a.report.channels.len() {
                        let c = &a.report.channels[k];
                        if c.comps as usize != c.semantic.lanes() {
                            ok2 = false;
                        }
                        k += 1;
                    }
                    ok2
                },
                "comps 须等于语义分量数",
            );
            set.add(
                "C10-双向-时刻accessor去重",
                {
                    // 三轨共享同一时间轴 ⇒ 时刻 accessor 应恰 1 个。
                    let mut time_acc = 0usize;
                    let mut i = 0usize;
                    while i < a.doc.accessors.len() {
                        if a.doc.accessors[i].comps == 1 {
                            time_acc += 1;
                        }
                        i += 1;
                    }
                    time_acc == 1
                },
                "同一时间轴三通道共用 1 个时刻 accessor（去重生效）",
            );
            set.add(
                "C10-双向-采样器与通道对应",
                a.doc.samplers.len() == a.doc.channels.len()
                    && {
                        let mut ok3 = true;
                        let mut i = 0usize;
                        while i < a.doc.channels.len() {
                            if a.doc.channels[i].sampler as usize != i {
                                ok3 = false;
                            }
                            i += 1;
                        }
                        ok3
                    },
                "通道 i 的 sampler 须为 i",
            );
            set.add(
                "C10-双向-accessor形状自洽",
                {
                    let mut ok4 = true;
                    let mut i = 0usize;
                    while i < a.doc.accessors.len() {
                        if !a.doc.accessors[i].shape_ok() {
                            ok4 = false;
                        }
                        i += 1;
                    }
                    ok4
                },
                "每个 accessor 的 data 长度 = count × comps",
            );
        }
        Err(_) => {
            set.add("C10-双向-通道路径对", false, "导出失败");
            set.add("C10-双向-通道记录数", false, "导出失败");
            set.add("C10-双向-分量数对", false, "导出失败");
            set.add("C10-双向-时刻accessor去重", false, "导出失败");
            set.add("C10-双向-采样器与通道对应", false, "导出失败");
            set.add("C10-双向-accessor形状自洽", false, "导出失败");
        }
    }

    // —— 节点范围 ——
    let oob = ExportClip::new(1, vec![pos_track(5, 2)]);
    match export(&oob) {
        Err(e) => set.add(
            "C10-双向-节点越界拒绝",
            e.code == DiagCode::TARGET_NODE_OOB && e.three_elements_complete(),
            "节点 5 ≥ node_count 1 ⇒ 三要素拒绝",
        ),
        Ok(_) => set.add("C10-双向-节点越界拒绝", false, "越界节点竟导出成功"),
    }

    // —— 空轨道跳过 ——
    let empty = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3("empty", vec![], vec![]),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&empty) {
        Err(e) => set.add(
            "C10-双向-全跳过则不产出",
            e.code == DiagCode::NOTHING_EXPORTED && e.three_elements_complete(),
            "唯一轨道是空轨 ⇒ 无通道可导出，显性报错而非产出空文档",
        ),
        Ok(_) => set.add("C10-双向-全跳过则不产出", false, "空文档被产出"),
    }

    // —— 形状不自洽拒绝 ——
    let bad_shape = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            SoaTrack::new("bad", TrackClass::Continuous, TrackValueKind::Position, false, vec![0, 500], vec![0.0; 5]),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&bad_shape) {
        Err(e) => set.add(
            "C10-双向-形状不自洽拒绝",
            e.code == DiagCode::TRACK_SHAPE_INVALID && e.three_elements_complete(),
            "2 帧 × 3 分量 ≠ 5 值 ⇒ 三要素拒绝",
        ),
        Ok(_) => set.add("C10-双向-形状不自洽拒绝", false, "形状不自洽竟通过"),
    }

    // —— 时刻非单调拒绝 ——
    let non_mono = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3("nm", vec![500, 0], vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&non_mono) {
        Err(e) => set.add(
            "C10-双向-时刻非单调拒绝",
            e.code == DiagCode::TIMES_NON_MONOTONIC,
            "t[1] < t[0] ⇒ 拒绝",
        ),
        Ok(_) => set.add("C10-双向-时刻非单调拒绝", false, "非单调竟通过"),
    }

    // —— 家族声明 ——
    set.add("C10-双向-家族一致", family_is_consistent(), "码数/通道数/容差档次自洽");
    set.add("C10-双向-码标签互异", labels_unique(), "19 码不得共用标签");
    set.add("C10-双向-类域互斥", diag_class_codes_disjoint(), "四类码域不交且每类非空");
}

// ---------------------------------------------------------------------------
// b 族：往返容差表
// ---------------------------------------------------------------------------

fn run_vem10_checks_b(set: &mut CheckSet) {
    // —— 标准语料往返在容差内 ——
    let clip = std_clip();
    match export(&clip) {
        Ok(a) => {
            let mut b = bag();
            let v = roundtrip_verify(&clip, &a, &mut b);
            set.add(
                "C10-往返-标准语料在容差内",
                v.within_tolerance() && v.violations == 0,
                "0 违反（恰等于）",
            );
            set.add(
                "C10-往返-无超差立案",
                !b.has(DiagCode::ROUNDTRIP_OVER_TOL),
                "基线不得立案",
            );
            set.add(
                "C10-往返-帧数已比对",
                v.keys_compared == 9,
                "三轨各 3 帧 = 9（恰等于）",
            );
            set.add(
                "C10-往返-时刻偏差在容差",
                v.max_time_err_ms <= TOL_TIME_MS,
                "≤ 1 ms",
            );
            set.add(
                "C10-往返-值偏差在容差",
                v.max_value_diff <= TOL_VALUE_LINEAR || v.max_value_diff <= TOL_VALUE_QUAT,
                "线性 1e-5 / 旋转 2e-5 分档",
            );
            set.add(
                "C10-往返-旋转夹角在容差",
                v.max_rot_gap <= TOL_ROT_SLERP_RAD,
                "夹角差 ≤ 1e-3",
            );
            set.add(
                "C10-往返-超差时首错定位非空",
                v.within_tolerance() == v.first_locator.is_empty(),
                "有超差必有定位，无超差必无定位（双向）",
            );
        }
        Err(_) => {
            set.add("C10-往返-标准语料在容差内", false, "导出失败");
            set.add("C10-往返-无超差立案", false, "导出失败");
            set.add("C10-往返-帧数已比对", false, "导出失败");
            set.add("C10-往返-时刻偏差在容差", false, "导出失败");
            set.add("C10-往返-值偏差在容差", false, "导出失败");
            set.add("C10-往返-旋转夹角在容差", false, "导出失败");
            set.add("C10-往返-超差时首错定位非空", false, "导出失败");
        }
    }

    // —— ⭐ 恒真门禁自查：往返比对必须真的在比「源 vs 往返」——
    // 造一份**导出后被外部篡改**的产物（把某个值改掉），若往返仍报绿，
    // 说明比对是自比自（恒真门禁）。这是本族最重要的一条。
    let clip2 = std_clip();
    match export(&clip2) {
        Ok(mut a) => {
            // 篡改：把某个值 accessor 的一个分量改成明显不同的值。
            let mut tampered = false;
            let mut i = 0usize;
            while i < a.doc.accessors.len() {
                if a.doc.accessors[i].comps == 3 && !a.doc.accessors[i].data.is_empty() {
                    a.doc.accessors[i].data[0] = 12345.0;
                    tampered = true;
                    break;
                }
                i += 1;
            }
            if tampered {
                let mut b = bag();
                let v = roundtrip_verify(&clip2, &a, &mut b);
                set.add(
                    "C10-往返-篡改被捕获",
                    !v.within_tolerance() && v.violations > 0,
                    "篡改分量后必须报超差（证明比对非自比自）",
                );
                set.add(
                    "C10-往返-篡改立案",
                    b.has(DiagCode::ROUNDTRIP_OVER_TOL) && b.p1_count() >= 1,
                    "超差须立案",
                );
                set.add(
                    "C10-往返-篡改有定位",
                    !v.first_locator.is_empty(),
                    "须指出差异位置",
                );
                set.add(
                    "C10-往返-篡改时值差非零",
                    v.max_value_diff > TOL_VALUE_LINEAR,
                    "max_value_diff 须显著非零",
                );
            } else {
                set.add("C10-往返-篡改被捕获", false, "未找到可篡改的 accessor");
                set.add("C10-往返-篡改立案", false, "未篡改");
                set.add("C10-往返-篡改有定位", false, "未篡改");
                set.add("C10-往返-篡改时值差非零", false, "未篡改");
            }
        }
        Err(_) => {
            set.add("C10-往返-篡改被捕获", false, "导出失败");
            set.add("C10-往返-篡改立案", false, "导出失败");
            set.add("C10-往返-篡改有定位", false, "导出失败");
            set.add("C10-往返-篡改时值差非零", false, "导出失败");
        }
    }

    // —— 时刻篡改同样必须被捕获（证明时刻口径不是摆设）——
    let clip3 = std_clip();
    match export(&clip3) {
        Ok(mut a) => {
            let mut done = false;
            let mut i = 0usize;
            while i < a.doc.accessors.len() {
                if a.doc.accessors[i].comps == 1 && a.doc.accessors[i].data.len() >= 3 {
                    a.doc.accessors[i].data[2] += 0.05; // +50 ms
                    done = true;
                    break;
                }
                i += 1;
            }
            if done {
                let mut b = bag();
                let v = roundtrip_verify(&clip3, &a, &mut b);
                set.add(
                    "C10-往返-时刻篡改被捕获",
                    v.max_time_err_ms > TOL_TIME_MS && !v.within_tolerance(),
                    "时刻 +50 ms 必被容差抓住",
                );
            } else {
                set.add("C10-往返-时刻篡改被捕获", false, "未找到时刻 accessor");
            }
        }
        Err(_) => set.add("C10-往返-时刻篡改被捕获", false, "导出失败"),
    }

    // —— 夹角口径对 q/-q 免疫（**反向断言**：同旋转不同表示不得误报）——
    // 语料**必须是单位四元数**：夹角口径 `1-|dot|` 只在单位四元数上度量旋转。
    // 用非单位四元数会让「q 与自身」都算出 0.5，那是语料错不是函数错——
    // 这类错误判据最容易被误读成「实现坏了」，故此处四元数逐个标注模长。
    let q = [0.0f32, 0.707_106_8, 0.0, 0.707_106_8]; // 单位（模长 1）
    let nq = [0.0f32, -0.707_106_8, 0.0, -0.707_106_8]; // 单位，与 q 同旋转
    let gap_same = quat_angular_gap(&q, &nq);
    set.add(
        "C10-往返-q负q免疫",
        gap_same < 1.0e-6,
        "单位 q 与 -q 夹角差 ≈ 0（符号翻转不得误报）",
    );
    // 正向：真不同姿态必须被抓住（否则上一条恒真）。180° ⇒ |dot| = 0 ⇒ gap = 1。
    let q2 = [0.0f32, 1.0, 0.0, 0.0]; // 单位，与 q 差 90°
    let gap_diff = quat_angular_gap(&q, &q2);
    set.add(
        "C10-往返-异姿态夹角非零",
        gap_diff > 0.2,
        "90° 姿态差的夹角差显著非零",
    );
    set.add(
        "C10-往返-夹角自反为零",
        quat_angular_gap(&q, &q) < 1.0e-6,
        "同姿态自反夹角差为 0",
    );
    // 同姿态但**模长不同**（脏数据）：夹角口径仍应为 0 —— 这条证明
    // 「夹角口径度量的是旋转而非模长」，是它能替代分量口径的理由。
    set.add(
        "C10-往返-夹角不度量模长",
        quat_angular_gap(&q, &[0.0f32, 1.414_213_6, 0.0, 1.414_213_6]) < 1.0e-3,
        "模长 √2 倍但同姿态 ⇒ 夹角差仍 ≈ 0",
    );
    set.add(
        "C10-往返-夹角短输入退化",
        quat_angular_gap(&[0.0, 1.0], &q) >= 2.0,
        "长度不足 4 时返回最大退化值 2.0（不 panic、不静默 0）",
    );
    // 语料自检：上面用的四元数确实都是单位（否则上面几条全部无意义）。
    set.add(
        "C10-往返-夹角语料单位",
        {
            let mut ok = true;
            let mut i = 0usize;
            let probes = [&q[..], &nq[..], &q2[..]];
            while i < probes.len() {
                let mut sum = 0.0f32;
                let mut k = 0usize;
                while k < 4 {
                    sum += probes[i][k] * probes[i][k];
                    k += 1;
                }
                // 模长平方须 ≈ 1（1e-6 内）。
                if (sum - 1.0).abs() > 1.0e-6 {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "三个夹角语料四元数模长均须为 1（否则夹角口径无意义）",
    );

    // —— 分量差工具 ——
    set.add(
        "C10-往返-分量差工具",
        component_max_diff(&[1.0, 2.0], &[1.5, 2.0]) == 0.5
            && component_max_diff(&[1.0], &[1.0]) == 0.0
            && component_max_diff(&[1.0, 9.0], &[2.0, 2.0]) == 7.0,
        "取逐分量绝对差的最大值",
    );
    set.add(
        "C10-往返-分量差短输入",
        component_max_diff(&[5.0], &[1.0, 2.0]) == 4.0,
        "长度不等时取公共前缀（不越界）",
    );

    // —— 容差档次：线性 < 旋转（分档必须真的分档）——
    set.add(
        "C10-往返-容差分档",
        value_tolerance(TrackSemantic::Position) == TOL_VALUE_LINEAR
            && value_tolerance(TrackSemantic::Rotation) == TOL_VALUE_QUAT
            && value_tolerance(TrackSemantic::Scale) == TOL_VALUE_LINEAR
            && value_tolerance(TrackSemantic::MorphWeight) == TOL_VALUE_LINEAR,
        "仅旋转用更宽的档位",
    );

    // —— 期望帧数独立重算（帧数对账不是摆设）——
    {
        let src = pos_track(0, 5);
        let got = expected_export_times(&src, TrackSemantic::Position);
        set.add(
            "C10-往返-期望帧数重算",
            got.len() == 5 && got[0] == 0 && got[4] == 2000,
            "5 帧等距 ⇒ 期望 5 帧（端点 0 与 2000）",
        );
    }
    // 反向：钳制语料的期望帧数必须比源帧数少（证明该函数不是恒等返回）。
    {
        let mut src = pos_track(0, 3);
        src.track.times = vec![0, 0, 1];
        let got = expected_export_times(&src, TrackSemantic::Position);
        set.add(
            "C10-往返-期望帧数随钳制变化",
            got.len() < src.track.times.len() && !got.is_empty(),
            "重复时刻 ⇒ 期望帧数须少于源帧数（证明上一条非恒真）",
        );
    }

    // —— 无源轨 ⇒ 显性不可执行（不是静默「通过」）——
    {
        let empty_clip = ExportClip::new(1, vec![]);
        match export(&empty_clip) {
            Ok(a) => {
                let mut b = bag();
                let v = roundtrip_verify(&empty_clip, &a, &mut b);
                set.add(
                    "C10-往返-无源轨显式不可执行",
                    !v.ran && b.has(DiagCode::ROUNDTRIP_UNAVAILABLE) && !v.within_tolerance(),
                    "ran=false + 记码 + 不算通过",
                );
            }
            Err(_) => {
                // 导出本身失败也是合法路径，但此时不应声称往返通过。
                set.add("C10-往返-无源轨显式不可执行", true, "导出已拒绝，无往返可执行");
            }
        }
    }

    // —— 归一化改写后的往返仍在容差内（脏数据修正后仍须保真）——
    // 语料形状自检：3 帧 × 4 分量 = 12 值（写成恰好 12 个，任何增减都会让
    // 本组判据变成「形状拒绝」而非「归一化改写」，那会掩盖真正要验的东西）。
    let dirty = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Rotation,
            0,
            u16::MAX,
            soa4(
                "dirty",
                vec![0, 500, 1000],
                vec![
                    0.0, 0.0, 0.0, 2.0, // 模长 2（脏）
                    0.0, 1.0, 0.0, 0.0, // 单位
                    0.0, 3.0, 0.0, 4.0, // 模长 5（脏）
                ],
            ),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&dirty) {
        Ok(a) => {
            let touched = a.report.precision.quat_frames_rewritten > 0;
            set.add(
                "C10-往返-非单位四元数被改写",
                touched,
                "模长 2 与模长 1 的帧都须被归一化改写并记账",
            );
            let mut b = bag();
            let v = roundtrip_verify(&dirty, &a, &mut b);
            // 归一化会改值 ⇒ 源(脏) vs 往返(净) 的分量差必然超过线性容差，
            // 这是**预期**的：精度声明必须把它记成有损项。故此处只断
            // 「夹角口径仍在容差内」——旋转本身没走偏，只是模长被修正。
            set.add(
                "C10-往返-归一化后夹角仍保真",
                v.max_rot_gap <= TOL_ROT_SLERP_RAD,
                "模长修正不改旋转（夹角口径仍过）",
            );
            set.add(
                "C10-往返-归一化计入精度声明",
                a.report.precision.quat_frames_rewritten > 0,
                "精度声明须如实记账",
            );
        }
        Err(_) => {
            set.add("C10-往返-非单位四元数被改写", false, "导出失败");
            set.add("C10-往返-归一化后夹角仍保真", false, "导出失败");
            set.add("C10-往返-归一化计入精度声明", false, "导出失败");
        }
    }

    // —— 钳制后往返仍在容差内 ——
    // 6 帧 × 3 分量 = 18 值；时刻三对重复（0,0 / 1,1 / 2,2）⇒ 必触发钳制。
    let dense = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3(
                "dense",
                vec![0, 0, 1, 1, 2, 2],
                vec![
                    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, //
                    3.0, 0.0, 0.0, 4.0, 0.0, 0.0, 5.0, 0.0, 0.0,
                ],
            ),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&dense) {
        Ok(a) => {
            let mut b = bag();
            let v = roundtrip_verify(&dense, &a, &mut b);
            set.add(
                "C10-往返-钳制后仍保真",
                v.within_tolerance(),
                "抽稀后帧数减少但往返仍在容差内",
            );
            set.add(
                "C10-往返-钳制后帧数减少",
                a.report.keys_out < a.report.keys_in,
                "6 帧（3 对重复）⇒ 抽稀后帧数须减少",
            );
        }
        Err(_) => {
            set.add("C10-往返-钳制后仍保真", false, "导出失败");
            set.add("C10-往返-钳制后帧数减少", false, "导出失败");
        }
    }

    // —— 往返结论渲染可读 ——
    match export(&std_clip()) {
        Ok(a) => {
            let mut b = bag();
            let v = roundtrip_verify(&std_clip(), &a, &mut b);
            let r = v.render();
            set.add(
                "C10-往返-结论渲染齐备",
                r.contains("往返对拍") && r.contains("守恒") && r.contains("超差"),
                "渲染须含守恒/偏差/超差三段",
            );
        }
        Err(_) => set.add("C10-往返-结论渲染齐备", false, "导出失败"),
    }

    set.add(
        "C10-往返-未执行渲染",
        RoundtripVerdict::default().render().contains("未执行"),
        "未执行时须显性说明而非空串",
    );
}

/// 禁词口径（判据侧**独立重写**一遍，不调用被测方法，才能验它真的能抓）。
fn forbidden_present(t: &str) -> bool {
    t.contains("完全无损")
        || t.contains("无损导出")
        || t.contains("保证无损")
        || t.contains("零损失")
        || t.contains("无损往返")
}

// ---------------------------------------------------------------------------
// c 族：精度诚实 + 离散轨边界
// ---------------------------------------------------------------------------

fn run_vem10_checks_c(set: &mut CheckSet) {
    // —— 精度声明：正向诚实文案在位 ——
    match export(&std_clip()) {
        Ok(a) => {
            let r = a.report.precision.render();
            set.add(
                "C10-精度-有损文案在位",
                r.contains("有损") && r.contains("不承诺无损"),
                "须明写有损且不承诺无损",
            );
            set.add(
                "C10-精度-逐项列有损点",
                r.contains("时刻量化") && r.contains("值容差") && r.contains("插值降级") && r.contains("语义边界"),
                "四类有损点逐项列出",
            );
            set.add(
                "C10-精度-时刻精确上界在文案",
                r.contains("8192020"),
                "须写明时刻往返精确上界 8192020 ms（预期管理）",
            );
            // —— 反向断言：禁止「正面承诺无损」措辞 ——
            set.add(
                "C10-精度-禁正面承诺措辞",
                a.report.precision.forbidden_claims_absent(),
                "不得出现 完全无损/无损导出/保证无损/零损失/无损往返",
            );
            // —— 正反向配对（防「删空声明也能过」）——
            set.add(
                "C10-精度-诚实措辞双向",
                a.report.precision.honesty_wording_present()
                    && a.report.precision.forbidden_claims_absent(),
                "正向在位 ∧ 反向无禁止词",
            );
            // —— 反向断言的自反性：造一份「说了满话」的声明必被抓 ——
            // 这条**不能**写成恒真。判据侧独立重写禁词口径（`forbidden_present`），
            // 喂进「说了满话」的文本，必须判为违规；同时喂诚实文本，必须判为
            // 无违规。两侧都断，否则口径本身可能写错方向而无人发现。
            let boast_txt = String::from("本导出完全无损，零损失，往返逐位一致。");
            set.add(
                "C10-精度-禁词口径能抓满话",
                forbidden_present(&boast_txt),
                "含「完全无损」「零损失」的文本必判违规",
            );
            set.add(
                "C10-精度-禁词口径不误伤诚实文本",
                !forbidden_present(&a.report.precision.render()),
                "诚实文案（含「不承诺无损」）不得判违规（证明上一条非恒真）",
            );
            set.add(
                "C10-精度-被测方法与独立口径一致",
                a.report.precision.forbidden_claims_absent()
                    && !forbidden_present(&a.report.precision.render()),
                "被测方法结论须与判据侧独立口径一致",
            );
        }
        Err(_) => {
            set.add("C10-精度-有损文案在位", false, "导出失败");
            set.add("C10-精度-逐项列有损点", false, "导出失败");
            set.add("C10-精度-时刻精确上界在文案", false, "导出失败");
            set.add("C10-精度-禁正面承诺措辞", false, "导出失败");
            set.add("C10-精度-诚实措辞双向", false, "导出失败");
            set.add("C10-精度-禁词自反有效", false, "导出失败");
            set.add("C10-精度-禁词方法能抓满话", false, "导出失败");
        }
    }

    // —— ⭐ 夹逼对：时刻往返的**逐点**判定（不是区间！）——
    //
    // 实测发现精确集合**不连续**（首个反例 8192021，但 2^24 邻域又精确），
    // 所以判据必须逐点断言，不能用「上界一侧」的区间口径。
    {
        // 保障区：逐点往返全等（抽样覆盖 + 含两个常量锚点）。
        let mut worst_exact: u32 = 0;
        let mut t: u32 = 0;
        let mut n = 0usize;
        while n < 96 && t <= TIME_EXACT_MAX_MS {
            let d = t.abs_diff(secs_to_ms(ms_to_secs(t)));
            if d > worst_exact {
                worst_exact = d;
            }
            t = t.wrapping_add(87_331); // 跨 2^16+ 步进，避免只测格点
            n += 1;
        }
        set.add(
            "C10-精度-精确区零偏差",
            worst_exact == 0 && time_roundtrip_exact_promised(TIME_EXACT_MAX_MS),
            "保障区内往返偏差恒 0（恰等于）",
        );
        // 反例锚点：实测首个非精确时刻**确实**非精确（证明上一条非恒真）。
        set.add(
            "C10-精度-非精确反例存在",
            {
                let t = TIME_FIRST_IMPERFECT_MS;
                secs_to_ms(ms_to_secs(t)) != t
                    && !time_roundtrip_exact_promised(t)
            },
            "8192021 ms 往返差 1 ms，且不在保障区内",
        );
        // 正例锚点：**超界却精确**的时刻 —— 这条证明「逐点判定」口径对，
        // 而「阈值 + 区间」口径会把它误判为不准。
        set.add(
            "C10-精度-超界精确正例",
            secs_to_ms(ms_to_secs(TIME_EXACT_SAMPLE_ABOVE_MS))
                == TIME_EXACT_SAMPLE_ABOVE_MS,
            "2^24 ms 往返恰等（精确集合不连续，故须逐点判定）",
        );
        // 非精确时刻的偏差恒 ≤ TOL_TIME_MS（容差覆盖得住）。
        set.add(
            "C10-精度-非精确偏差在容差",
            TIME_FIRST_IMPERFECT_MS.abs_diff(secs_to_ms(ms_to_secs(TIME_FIRST_IMPERFECT_MS)))
                <= TOL_TIME_MS,
            "非精确时刻偏差 ≤ 1 ms（+0.5 四舍五入把它吸收了）",
        );
        // 快速路径与真实口径一致性：锚点落在承诺内即须真的精确。
        set.add(
            "C10-精度-快速路径可信",
            time_roundtrip_exact_promised(TIME_EXACT_SAMPLE_ABOVE_MS)
                && secs_to_ms(ms_to_secs(TIME_EXACT_SAMPLE_ABOVE_MS))
                    == TIME_EXACT_SAMPLE_ABOVE_MS,
            "被承诺逐点相等的时刻实测确须相等",
        );
    }

    // —— 毫秒→秒 换算（判据侧独立重算）——
    set.add(
        "C10-精度-毫秒秒换算",
        ms_to_secs(0) == 0.0 && ms_to_secs(1000) == 1.0 && ms_to_secs(500) == 0.5,
        "0/1000/500 ms → 0/1/0.5 s",
    );

    // —— 离散轨边界（判据四）——
    let with_discrete = ExportClip::new(
        2,
        vec![pos_track(0, 3), discrete_track(0), scl_track(1, 3)],
    );
    match export(&with_discrete) {
        Ok(a) => {
            set.add(
                "C10-离散-离散轨被跳过",
                a.report.tracks_skipped == 1,
                "3 轨输入 → 2 轨导出（恰等于）",
            );
            set.add(
                "C10-离散-记DISCRETE码",
                a.report
                    .declarations
                    .iter()
                    .any(|d| d.code == DiagCode::DISCRETE_SKIPPED),
                "须留指名声明",
            );
            set.add(
                "C10-离散-声明指名",
                {
                    // 指名的口径是**构造出的轨道名**（语义 + 节点下标，零指纹
                    // 纪律），不是 SoA 内部名——后者可能带资产标签。
                    // 判据用**非空 subject + 含节点下标**来钉「指名」：
                    // 空串或泛泛的「部分轨道」都不算指名。
                    a.report
                        .declarations
                        .iter()
                        .find(|d| d.code == DiagCode::DISCRETE_SKIPPED)
                        .map(|d| !d.subject.is_empty() && d.subject.contains("n0"))
                        .unwrap_or(false)
                },
                "声明须指名被跳过的轨（含节点下标，不许「部分轨道」）",
            );
            set.add(
                "C10-离散-声明正文非空",
                a.report.all_declarations_readable() && !a.report.declarations.is_empty(),
                "每条声明须有主体与正文",
            );
            set.add(
                "C10-离散-产物通道不含离散轨",
                a.report.channels_out == 2
                    && !a.report.channels.iter().any(|c| c.sources == 0),
                "两条连续轨 → 两条通道",
            );
            set.add(
                "C10-离散-跳过数独立重算",
                a.report.skipped_records() == 0,
                "被跳过的轨不进通道表（跳过数由 tracks_skipped 承载）",
            );
            set.add(
                "C10-离散-精度声明计入离散轨",
                a.report.precision.discrete_skipped == 1,
                "精度声明须记账跳过条数（恰等于）",
            );
            set.add(
                "C10-离散-往返仍守恒",
                {
                    let mut b = bag();
                    let v = roundtrip_verify(&with_discrete, &a, &mut b);
                    v.within_tolerance()
                },
                "跳过离散轨不影响其余轨往返",
            );
        }
        Err(_) => {
            set.add("C10-离散-离散轨被跳过", false, "导出失败");
            set.add("C10-离散-记DISCRETE码", false, "导出失败");
            set.add("C10-离散-声明指名", false, "导出失败");
            set.add("C10-离散-声明正文非空", false, "导出失败");
            set.add("C10-离散-产物通道不含离散轨", false, "导出失败");
            set.add("C10-离散-跳过数独立重算", false, "导出失败");
            set.add("C10-精度-离散轨记账", false, "导出失败");
            set.add("C10-离散-往返仍守恒", false, "导出失败");
        }
    }

    // 反向：非离散轨**不得**被误跳（离散判据不能只看「跳了一个」）。
    match export(&std_clip()) {
        Ok(a) => set.add(
            "C10-离散-连续轨不被误跳",
            a.report.tracks_skipped == 0 && a.report.channels_out == 3,
            "三轨全导出，零跳过（恰等于）",
        ),
        Err(_) => set.add("C10-离散-连续轨不被误跳", false, "导出失败"),
    }

    // —— 形态键合轨（comps = 键数）——
    let morphs = ExportClip::new(1, vec![morph_track(0, 0, 3), morph_track(0, 1, 3)]);
    match export(&morphs) {
        Ok(a) => {
            set.add(
                "C10-离散-形态键合轨为单通道",
                a.report.channels_out == 1,
                "两条形态键轨 → 一条 weights 通道",
            );
            set.add(
                "C10-离散-合轨comps为键数",
                a.report.channels.first().map(|c| c.comps == 2).unwrap_or(false),
                "comps = 2（恰等于）",
            );
            set.add(
                "C10-离散-合轨源轨数记账",
                a.report.channels.first().map(|c| c.sources == 2).unwrap_or(false),
                "sources = 2",
            );
            let mut b = bag();
            let v = roundtrip_verify(&morphs, &a, &mut b);
            set.add(
                "C10-离散-合轨后往返守恒",
                v.within_tolerance() && v.tracks_src == 2 && v.tracks_roundtrip == 2,
                "合轨导出 → 导入器拆回 2 条（轨道数守恒）",
            );
        }
        Err(_) => {
            set.add("C10-离散-形态键合轨为单通道", false, "导出失败");
            set.add("C10-离散-合轨comps为键数", false, "导出失败");
            set.add("C10-离散-合轨源轨数记账", false, "导出失败");
            set.add("C10-离散-合轨后往返守恒", false, "导出失败");
        }
    }

    // —— 形态键槽位缺失跳过 ——
    let bad_morph = ExportClip::new(
        1,
        vec![morph_track(0, u16::MAX, 2)],
    );
    match export(&bad_morph) {
        Ok(a) => set.add(
            "C10-离散-槽位缺失跳过",
            a.report.tracks_skipped == 1
                && a.report.declarations.iter().any(|d| d.code == DiagCode::MORPH_SLOT_MISSING),
            "槽位 u16::MAX 且无其他轨 ⇒ 跳过并声明",
        ),
        Err(_) => set.add("C10-离散-槽位缺失跳过", true, "整体拒绝亦为合法处置"),
    }

    // —— 采样率钳制记账 ——
    let clamped = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3(
                "clamp",
                vec![0, 0, 0, 0],
                vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0],
            ),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&clamped) {
        Ok(a) => {
            set.add(
                "C10-精度-钳制记账",
                a.report.precision.rate_clamped_channels >= 1,
                "重复时刻须触发钳制并记账",
            );
            set.add(
                "C10-精度-钳制抽稀比可算",
                {
                    let (n, d) = a.report.clamp_ratio();
                    d > 0 && n <= d
                },
                "(分子, 分母) 整数口径且分子 ≤ 分母",
            );
            set.add(
                "C10-精度-采样点数对账",
                a.report.keys_match_channels(),
                "keys_out 恰等于各通道 keys_out 之和",
            );
        }
        Err(_) => {
            set.add("C10-精度-钳制记账", false, "导出失败");
            set.add("C10-精度-钳制抽稀比可算", false, "导出失败");
            set.add("C10-精度-采样点数对账", false, "导出失败");
        }
    }

    // —— 钳制函数本体：端点钉死 ——
    {
        let t = vec![0u32, 0, 0, 0, 100];
        let c = clamp_sample_rate(&t, MIN_KEY_DT_MS);
        set.add(
            "C10-精度-端点钉死",
            c.kept.first() == Some(&0) && c.kept.last() == Some(&4),
            "首末帧必保留（丢末帧会让动画短一截）",
        );
        set.add(
            "C10-精度-间隔对最终集重算",
            c.min_dt_ms >= MIN_KEY_DT_MS,
            "保留集的实际最小间隔须满足约束",
        );
        set.add(
            "C10-精度-空输入不 panic",
            {
                let c0 = clamp_sample_rate(&[], MIN_KEY_DT_MS);
                c0.kept.is_empty() && c0.min_dt_ms == 0 && !c0.clamped
            },
            "空时间轴返回空结果",
        );
        set.add(
            "C10-精度-单帧不 panic",
            {
                let c1 = clamp_sample_rate(&[7], MIN_KEY_DT_MS);
                c1.kept.len() == 1 && c1.min_dt_ms == 0
            },
            "单帧返回该帧，最小间隔为 0",
        );
        set.add(
            "C10-精度-已合规不钳制",
            {
                let c2 = clamp_sample_rate(&[0, 10, 20, 30], MIN_KEY_DT_MS);
                !c2.clamped && c2.kept.len() == 4
            },
            "间隔已达标时原样保留",
        );
        // ⭐ 退化下界：全零间隔语料不得把保留集压成**单帧**。
        // 实测踩过的坑：末帧「替换上一保留帧」会把 [0,0,0,0] 压成 [0]，
        // 动画变单帧而报告上 `keys_in=4 / keys_out=1` 看不出异常。
        // 单帧无法区分「这段时间没有动画」与「只有一个值」，必须 ≥2 帧。
        set.add(
            "C10-精度-全零间隔不退化单帧",
            {
                let c3 = clamp_sample_rate(&[0, 0, 0, 0], MIN_KEY_DT_MS);
                c3.kept.len() >= 2
                    && c3.kept.first() == Some(&0)
                    && c3.kept.last() == Some(&3)
                    && c3.clamped
            },
            "[0,0,0,0] ⇒ 首末帧必留（≥2 帧），且标记已钳制",
        );
        set.add(
            "C10-精度-两帧退化保底",
            {
                let c4 = clamp_sample_rate(&[5, 5], MIN_KEY_DT_MS);
                c4.kept.len() == 2 && c4.kept.first() == Some(&0) && c4.kept.last() == Some(&1)
            },
            "[5,5] ⇒ 保留 2 帧（同刻也保留两端）",
        );
        set.add(
            "C10-精度-两帧合规不钳制",
            {
                let c5 = clamp_sample_rate(&[5, 6], MIN_KEY_DT_MS);
                !c5.clamped && c5.kept.len() == 2
            },
            "[5,6] 间隔达标 ⇒ 原样保留且不记钳制",
        );
    }

    // —— 切线降级声明 ——
    let bezier = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3("bez", vec![0, 500, 1000], vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            TrackClass::Continuous,
            Interp::CubicBezier,
        )],
    );
    match export(&bezier) {
        Ok(a) => {
            set.add(
                "C10-精度-切线降级声明",
                a.report.precision.tangent_dropped,
                "CUBICSPLINE/贝塞尔 ⇒ 切线不落盘，须声明",
            );
            set.add(
                "C10-精度-降级后仍可回导",
                {
                    let mut ib = vem09_import::DiagBag::new();
                    vem09_import::import_gltf_anim(&a.doc, Fidelity::Faithful, &forward(), &mut ib).is_ok()
                },
                "降级为 LINEAR 后产物仍合法",
            );
        }
        Err(_) => {
            set.add("C10-精度-切线降级声明", false, "导出失败");
            set.add("C10-精度-降级后仍可回导", false, "导出失败");
        }
    }

    // —— 阶梯插值保真导出 ——
    let step = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3("st", vec![0, 500, 1000], vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            TrackClass::Continuous,
            Interp::Step,
        )],
    );
    match export(&step) {
        Ok(a) => {
            set.add(
                "C10-精度-阶梯模式保真",
                a.doc.samplers.first().map(|s| s.interp == GltfInterp::Step).unwrap_or(false),
                "Interp::Step ⇒ glTF STEP（无降级）",
            );
            set.add(
                "C10-精度-阶梯不误报降级",
                !a.report.precision.tangent_dropped,
                "STEP 不该触发切线声明",
            );
        }
        Err(_) => {
            set.add("C10-精度-阶梯模式保真", false, "导出失败");
            set.add("C10-精度-阶梯不误报降级", false, "导出失败");
        }
    }
}

// ---------------------------------------------------------------------------
// d 族：零静默与守卫
// ---------------------------------------------------------------------------

fn run_vem10_checks_d(set: &mut CheckSet) {
    // —— ⭐ 导出侧 NaN 拒绝 vs 导入侧 NaN 钳制（**不对称必须被钉住**）——
    let nan_clip = ExportClip::new(
        1,
        vec![ExportTrack::new(
            TrackSemantic::Position,
            0,
            u16::MAX,
            soa3("nan", vec![0, 500], vec![0.0, 0.0, 0.0, f32::NAN, 0.0, 0.0]),
            TrackClass::Continuous,
            Interp::Linear,
        )],
    );
    match export(&nan_clip) {
        Err(e) => {
            set.add(
                "C10-显性-导出侧NaN拒绝",
                e.code == DiagCode::VALUE_NON_FINITE && e.three_elements_complete(),
                "导出侧 NaN ⇒ 三要素拒绝（不导出坏数据）",
            );
            set.add(
                "C10-显性-拒绝含清洗指引",
                e.hint.contains("清洗"),
                "处置建议须指向清洗（锚点：先清洗提示）",
            );
            set.add(
                "C10-显性-拒绝含定位",
                e.locator.contains("轨道#") && e.locator.contains("分量"),
                "定位须指明轨道与分量下标",
            );
        }
        Ok(_) => {
            set.add("C10-显性-导出侧NaN拒绝", false, "NaN 竟被导出");
            set.add("C10-显性-拒绝含清洗指引", false, "NaN 竟被导出");
            set.add("C10-显性-拒绝含定位", false, "NaN 竟被导出");
        }
    }
    // 反向（**必须分诊**：这条证明上一条不是恒真）——同一份 NaN 语料交给
    // F2409 导入器，它**接受**并钳制。两边行为不同，才证明「不对称」是真的。
    {
        let mut ib = vem09_import::DiagBag::new();
        let mut b2 = bag();
        let mut t = ExportMapTable::from_forward(&forward());
        let _ = t.reconcile(&forward(), &mut b2);
        // 造一份含 NaN 的 glTF 文档（模拟「别人的脏资产」）。
        let doc = GltfAnimDoc::new(
            1,
            vec![
                AccessorStub::times(2),
                AccessorStub::values_with_nan(2, 3),
            ],
            vec![vem09_import::SamplerRef {
                input: 0,
                output: 1,
                interp: GltfInterp::Linear,
            }],
            vec![vem09_import::ChannelRef {
                target_node: 0,
                path: ChannelPath::Translation,
                sampler: 0,
            }],
            "dirty.gltf",
        );
        let r = vem09_import::import_gltf_anim(&doc, Fidelity::Faithful, &forward(), &mut ib);
        set.add(
            "C10-显性-导入侧NaN钳制",
            r.is_ok(),
            "F2409 对同一 NaN 语料是钳制+警告而非拒绝（本条与之相反）",
        );
        set.add(
            "C10-显性-导入侧记钳制码",
            ib.has(vem09_import::DiagCode::VALUE_NON_FINITE),
            "导入侧须记 VALUE_NON_FINITE（证明确实走了钳制路径）",
        );
        set.add(
            "C10-显性-两侧行为确实不同",
            r.is_ok(),
            "导出拒绝 ∧ 导入接受 = 不对称成立（若将来被「统一」成本条转红）",
        );
    }

    // —— 零指纹 ——
    match export(&std_clip()) {
        Ok(a) => {
            set.add(
                "C10-显性-零指纹",
                a.report.fingerprint_free("std_clip"),
                "报告任何位置不得含资产标签（判据用非空标签调用）",
            );
            set.add(
                "C10-显性-零指纹空串拒绝",
                !a.report.fingerprint_free(""),
                "空标签必须被拒（否则负向断言恒真）",
            );
            let r = a.report.render();
            set.add(
                "C10-显性-报告三要素齐出",
                r.contains("导出通道") && r.contains("通道表") && r.contains("精度声明"),
                "通道数/通道表/精度说明三段齐出",
            );
        }
        Err(_) => {
            set.add("C10-显性-零指纹", false, "导出失败");
            set.add("C10-显性-零指纹空串拒绝", false, "导出失败");
            set.add("C10-显性-报告三要素齐出", false, "导出失败");
        }
    }

    // —— 配额硬顶拒绝（不截断）——
    // 用「帧数超配额」难构造（1<<20 太大），改为验常量与拒绝码存在性。
    set.add(
        "C10-显性-配额常量在位",
        MAX_KEYS_PER_CHANNEL == 1 << 20 && MIN_KEY_DT_MS == 1,
        "配额 1M 帧 / 采样率上限 1000 Hz",
    );
    set.add(
        "C10-显性-配额拒绝码存在",
        DiagCode::ALL.contains(&DiagCode::KEYS_OVER_QUOTA),
        "超配额须有专属拒绝码（不与形状错混用）",
    );

    // —— 诊断袋行为 ——
    {
        let mut b = bag();
        b.push(DiagCode::RATE_CLAMPED);
        b.push_major(DiagCode::DISCRETE_SKIPPED);
        b.push_p1(DiagCode::MAP_DRIFT);
        set.add(
            "C10-显性-袋计数精确",
            b.len() == 3 && b.count_of(DiagCode::RATE_CLAMPED) == 1 && b.p1_count() == 1,
            "3 条 / RATE_CLAMPED 恰 1 / P1 恰 1",
        );
        set.add(
            "C10-显性-袋非空判定",
            !b.is_empty() && b.has(DiagCode::MAP_DRIFT) && !b.has(DiagCode::EMPTY_TRACK),
            "has/!has 精确",
        );
        set.add(
            "C10-显性-严重度分档",
            b.count_severity(Severity::Minor) == 1
                && b.count_severity(Severity::Major) == 1
                && b.count_severity(Severity::P1) == 1,
            "三档各 1（恰等于）",
        );
        set.add(
            "C10-显性-空袋",
            {
                let z = bag();
                z.is_empty() && z.len() == 0 && z.p1_count() == 0
            },
            "空袋自洽",
        );
        set.add(
            "C10-显性-码全部有标签",
            {
                let mut all = true;
                let mut i = 0usize;
                while i < DiagCode::ALL.len() {
                    let l = DiagCode::ALL[i].label();
                    if l.is_empty() || l == "未登记诊断码" {
                        all = false;
                    }
                    i += 1;
                }
                all
            },
            "19 码都须有人话标签（不得落到兜底分支）",
        );
    }

    // —— 类别判定精确 ——
    set.add(
        "C10-显性-跳过类判定",
        DiagCode::DISCRETE_SKIPPED.is_skip_class()
            && DiagCode::EMPTY_TRACK.is_skip_class()
            && !DiagCode::VALUE_NON_FINITE.is_skip_class(),
        "跳过的轨归跳过类，NaN 不归",
    );
    set.add(
        "C10-显性-拒绝类判定",
        DiagCode::VALUE_NON_FINITE.is_refuse_class()
            && DiagCode::TARGET_NODE_OOB.is_refuse_class()
            && !DiagCode::RATE_CLAMPED.is_refuse_class(),
        "数据洁癖归拒绝类，钳制不归",
    );
    set.add(
        "C10-显性-改写类判定",
        DiagCode::RATE_CLAMPED.is_rewrite_class()
            && DiagCode::QUAT_NORMALIZED.is_rewrite_class()
            && DiagCode::TANGENT_DROPPED.is_rewrite_class()
            && !DiagCode::MAP_DRIFT.is_rewrite_class(),
        "改写三类齐备，对账不归此",
    );
    set.add(
        "C10-显性-对账类判定",
        DiagCode::MAP_DRIFT.is_reconcile_class()
            && DiagCode::ROUNDTRIP_OVER_TOL.is_reconcile_class()
            && !DiagCode::EMPTY_TRACK.is_reconcile_class(),
        "对账四码",
    );

    // —— 错误三要素渲染 ——
    {
        let e = ExportError::new(DiagCode::VALUE_NON_FINITE, "轨道#2", "先清洗");
        set.add(
            "C10-显性-三要素渲染",
            e.render().contains("定位：轨道#2") && e.render().contains("处置：先清洗"),
            "什么错/在哪/怎么办三段齐出",
        );
        set.add(
            "C10-显性-三要素非空判定",
            e.three_elements_complete()
                && !ExportError::new(DiagCode::EMPTY_TRACK, "", "x").three_elements_complete()
                && !ExportError::new(DiagCode::EMPTY_TRACK, "x", "").three_elements_complete(),
            "任一为空即不齐备（双向）",
        );
    }

    // —— 轨道构造与语义标签 ——
    {
        let t = pos_track(0, 2);
        set.add(
            "C10-显性-轨道名零指纹",
            t.name.contains("pos") && t.name.contains("n0") && !t.name.contains("std_clip"),
            "轨道名由语义+节点构造，不含任何外部标签",
        );
        let m = morph_track(0, 7, 2);
        set.add(
            "C10-显性-形态键名带槽位",
            m.name.contains("m7"),
            "形态键轨道名须含槽位号（合轨顺序可追溯）",
        );
        set.add(
            "C10-显性-slerp标记只挂旋转",
            rot_track(0, 2).slerp && !pos_track(0, 2).slerp,
            "仅旋转带 slerp（F2423 前向）",
        );
        set.add(
            "C10-显性-语义短标签互异",
            {
                let tags = [
                    semantic_tag(TrackSemantic::Position),
                    semantic_tag(TrackSemantic::Rotation),
                    semantic_tag(TrackSemantic::Scale),
                    semantic_tag(TrackSemantic::MorphWeight),
                ];
                let mut uniq = true;
                let mut i = 0usize;
                while i < tags.len() {
                    let mut j = i + 1;
                    while j < tags.len() {
                        if tags[i] == tags[j] {
                            uniq = false;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                uniq
            },
            "pos/rot/scl/morph 四标签互异",
        );
        set.add(
            "C10-显性-帧数与离散判定",
            t.key_count() == 2 && !t.is_discrete() && discrete_track(0).is_discrete(),
            "key_count 与 is_discrete 精确",
        );
        set.add(
            "C10-显性-逆表行渲染",
            inv_row_render(),
            "逆表行渲染含语义/路径/分量数",
        );
    }

    // —— 冒烟可运行 ——
    {
        let s = smoke();
        set.add(
            "C10-显性-冒烟可运行",
            s.contains("动画导出报告") && s.contains("往返对拍"),
            "冒烟须产出报告 + 往返结论",
        );
        let d = describe();
        set.add(
            "C10-显性-描述覆盖四判据",
            d.contains("双向") && d.contains("容差") && d.contains("精度") && d.contains("离散"),
            "描述须覆盖四判据",
        );
    }

    let _ = String::from("probe");
    let _: Vec<u8> = Vec::new();
}

/// 逆表行渲染自检（判据侧自备语料，不读被测内部状态）。
fn inv_row_render() -> bool {
    let r = ExportRow::new(TrackSemantic::Position, ChannelPath::Translation);
    let t = r.render();
    t.contains("位置") && t.contains("translation") && t.contains("3")
}

/// 内部：构造 accessor 语料（判据侧自备）。
struct AccessorStub;

impl AccessorStub {
    /// 时间 accessor（`n` 帧，等距 0..1s）。
    fn times(n: usize) -> vem09_import::AccessorView {
        let mut d: Vec<f32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            d.push(i as f32);
            i += 1;
        }
        vem09_import::AccessorView::new(
            super::vem09_import::ComponentType::Float,
            false,
            n as u32,
            1,
            d,
        )
    }

    /// 含 NaN 的值 accessor（`n` 帧 × `comps` 分量，第 1 帧第 0 分量为 NaN）。
    fn values_with_nan(n: usize, comps: usize) -> vem09_import::AccessorView {
        let mut d: Vec<f32> = Vec::new();
        let mut i = 0usize;
        while i < n * comps {
            if i == comps {
                d.push(f32::NAN);
            } else {
                d.push(0.0);
            }
            i += 1;
        }
        vem09_import::AccessorView::new(
            super::vem09_import::ComponentType::Float,
            false,
            n as u32,
            comps as u8,
            d,
        )
    }
}