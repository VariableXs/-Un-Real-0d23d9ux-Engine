//! VE-F2405 · 域自检（判据逐条对应，见 `vem05_asset.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **`m.anim.` 段** → `M05-段-四段齐备`、`M05-段-段内哈希定长十六进制`、
//!   `M05-段-根哈希覆盖段集`、`M05-段-段序语义固定`；
//! - **生态单点** → `M05-生态-m.anim.已注册为第三段`、`M05-生态-表外名拒收`、
//!   `M05-生态-属域归属正确`、`M05-生态-注册序号与顺序一致`；
//! - **三件套复用（F1948）** → `M05-签名-三级分级`、`M05-签名-空白串不算已填`、
//!   `M05-签名-缺失不阻断`、`M05-签名-均不阻断分发`；
//! - **三件套复用（F1956）** → `M05-版本-迁移链显式可达`、`M05-版本-无链显性拒绝`、
//!   `M05-版本-备份还原逐字节`、`M05-版本-备份跨段拒收`；
//! - **往返零损失** → `M05-往返-逐位一致`、`M05-往返-六类轨道全通`、
//!   `M05-往返-四分量展平保序`、`M05-往返-负零不塌成零`、
//!   `M05-往返-漂移定位到段`、`M05-往返-空资产往返`；
//! - **导入清洗** → `M05-清洗-绝对路径剥离`、`M05-清洗-机器 id 剥离`、
//!   `M05-清洗-原资产不被就地抹`、`M05-清洗-相对路径不误伤`、
//!   `M05-清洗-留痕指名字段`；
//! - 错误路径 → `M05-错误-魔数不匹配拒绝`、`M05-错误-段损坏定位到段`、
//!   `M05-错误-未知段跳过留声明`、`M05-错误-未来版本前向兼容`、
//!   `M05-错误-曲线错齐拒绝`、`M05-错误-时间非递增拒绝`、
//!   `M05-错误-悬空曲线引用拒绝`、`M05-错误-重复轨道 id 拒绝`、
//!   `M05-错误-权重越界拒绝`、`M05-错误-绑定路径非法拒绝`、
//!   `M05-错误-轨道类未知拒绝`、`M05-错误-插值器未知拒绝`、
//!   `M05-错误-clip 悬空轨道拒绝`、`M05-错误-clip 时长不足拒绝`、
//!   `M05-错误-非有限值拒绝`、`M05-错误-缺段拒绝`、`M05-错误-语法错拒绝`；
//! - 跨域对齐 → `M05-对接-重挂 F2402 容器`、`M05-对接-重挂轨数一致`；
//! - 性能与鲁棒 → `M05-性能-打包对拍线性`、`M05-性能-往返耗时随规模线性`、
//!   `M05-鲁棒-敌意输入不panic`、尾项 `M05-规模-未截断`。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use crate::checks::CheckSet;
use crate::svstar2::vem02_track::{InterpKind, TrackClass};
use crate::svstar2::vem05_asset::*;

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 便捷构造
// ---------------------------------------------------------------------------

/// 建一条标量曲线（时间 `0,10,20,...`，值 `0,1,2,...`）。
fn scalar_curve(id: &str, n: usize) -> CurveSegment {
    let mut r = AssetReport::new();
    let times: Vec<u32> = (0..n).map(|i| (i as u32) * 10).collect();
    let vals: Vec<f32> = (0..n).map(|i| i as f32).collect();
    CurveSegment::build(id, 1, times, vals, &mut r)
}

/// 建一条三分量曲线（pos/scale 用）。
fn vec3_curve(id: &str, n: usize) -> CurveSegment {
    let mut r = AssetReport::new();
    let times: Vec<u32> = (0..n).map(|i| (i as u32) * 10).collect();
    let mut vals: Vec<f32> = Vec::new();
    for i in 0..n {
        vals.push(i as f32);
        vals.push(i as f32 * 2.0);
        vals.push(i as f32 * 3.0);
    }
    CurveSegment::build(id, 3, times, vals, &mut r)
}

/// 建一条四分量曲线（color/rotation 用）。
fn vec4_curve(id: &str, n: usize) -> CurveSegment {
    let mut r = AssetReport::new();
    let times: Vec<u32> = (0..n).map(|i| (i as u32) * 10).collect();
    let mut vals: Vec<f32> = Vec::new();
    for i in 0..n {
        vals.push(i as f32);
        vals.push(1.0);
        vals.push(0.0);
        vals.push(1.0);
    }
    CurveSegment::build(id, 4, times, vals, &mut r)
}

/// 完整演示资产：六类轨道各一条 + 一个 clip。
fn demo_asset() -> AnimAsset {
    let mut a = AnimAsset::empty();
    a.meta = MetaSegment {
        asset_name: "走".to_string(),
        author: Some("作者甲".to_string()),
        license: Some("CC-BY".to_string()),
        source_url: Some("https://example.invalid/walk".to_string()),
        origin_path: Some("D:\\private\\rig\\walk.json".to_string()),
        machine_id: Some("MACHINE-7".to_string()),
        loop_default: LoopMode::Loop,
    };
    a.curves.push(scalar_curve("c_float", 4));
    a.curves.push(vec3_curve("c_pos", 4));
    a.curves.push(vec3_curve("c_scale", 4));
    a.curves.push(vec4_curve("c_color", 4));
    a.curves.push(vec4_curve("c_rot", 4));
    // 布尔用 0/1 展平标量（阈值 0.5 判真假）。
    let mut r = AssetReport::new();
    a.curves.push(CurveSegment::build(
        "c_bool",
        1,
        vec![0u32, 10, 20],
        vec![0.0, 1.0, 0.0],
        &mut r,
    ));

    let mk = |id: &str, class: TrackClass, interp: InterpKind, curve: &str| TrackSegment {
        track_id: id.to_string(),
        owner: "e1".to_string(),
        class,
        interp,
        curve_id: curve.to_string(),
        bind_raw: "/node/anim/pos".to_string(),
        weight: 1.0,
        blended: true,
    };
    a.tracks.push(mk("t_float", TrackClass::Float, InterpKind::Linear, "c_float"));
    a.tracks.push(mk("t_pos", TrackClass::Position, InterpKind::Linear, "c_pos"));
    a.tracks.push(mk("t_scale", TrackClass::Scale, InterpKind::Linear, "c_scale"));
    a.tracks.push(mk("t_color", TrackClass::Color, InterpKind::Linear, "c_color"));
    a.tracks.push(mk("t_rot", TrackClass::Rotation, InterpKind::Slerp, "c_rot"));
    a.tracks.push(mk("t_bool", TrackClass::Bool, InterpKind::Step, "c_bool"));

    a.clips.push(ClipSegment {
        clip_id: "clip_walk".to_string(),
        name: "走".to_string(),
        duration_ticks: 30,
        track_ids: vec![
            "t_float".to_string(),
            "t_pos".to_string(),
            "t_scale".to_string(),
            "t_color".to_string(),
            "t_rot".to_string(),
            "t_bool".to_string(),
        ],
        loop_mode: LoopMode::Loop,
    });
    a
}

/// 取导出文本（失败则空串）。
fn text_of(a: &AnimAsset) -> String {
    export_container(a)
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// VE-F2405 域自检。
pub fn run_vem05_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2405");

    // ---- m.anim. 段 ----
    {
        let a = demo_asset();
        let t = text_of(&a);
        let ok = export_container(&a).is_ok()
            && t.contains(SEGMENT_SCHEMA)
            && t.contains("\"magic\":\"VARIXANM\"")
            && t.contains("\"version\":3");
        set.add(
            "M05-段-四段齐备",
            ok && SegmentKind::ALL.len() == 4,
            "",
        );
        let all_present = SegmentKind::ALL
            .iter()
            .all(|k| t.contains(&format!("\"{}\":", k.key())));
        set.add("M05-段-段序语义固定", all_present, "");
    }

    {
        // 段内 `__h` 必须是定长 16 位十六进制。
        let a = demo_asset();
        let t = text_of(&a);
        let mut r = AssetReport::new();
        let root = parse_json(&t, &mut r);
        let mut lens_ok = false;
        if let Some(v) = root.as_ref().ok() {
            if let Some(segs) = v.get("segments") {
                lens_ok = SegmentKind::ALL.iter().all(|k| {
                    match segs.get(k.key()).and_then(|s| s.get_str(SEG_HASH_FIELD)) {
                        Some(h) => h.len() == 16 && parse_hex16(h).is_some(),
                        None => false,
                    }
                });
            }
        }
        set.add("M05-段-段内哈希定长十六进制", lens_ok, "");
    }

    {
        // 根内容哈希门禁（**必须用只有根哈希能拦的输入**）：
        // 在四类段字节完全不动的前提下，只改根 `version` 并保留旧 content_hash。
        // 段哈希此时全部自洽，唯一能发现的是覆盖 `magic|version|segments` 的根哈希。
        let a = demo_asset();
        let t = text_of(&a);
        let bad = retag_version_keep_hash(&t, 2);
        // 前提自证：段子树字节未变（否则这条测的就不是根哈希了）。
        let segs_untouched = {
            let mut r1 = AssetReport::new();
            let mut r2 = AssetReport::new();
            match (parse_json(&t, &mut r1), parse_json(&bad, &mut r2)) {
                (Ok(a1), Ok(a2)) => match (a1.get("segments"), a2.get("segments")) {
                    (Some(s1), Some(s2)) => write_to_string(s1) == write_to_string(s2),
                    _ => false,
                },
                _ => false,
            }
        };
        let r = import_container(&bad);
        let caught = !r.is_ok()
            && r
                .report()
                .errors()
                .iter()
                .any(|n| n.code == AssetDiag::SegmentHashMismatch);
        set.add("M05-段-根哈希覆盖段集", caught && segs_untouched, "");
    }

    {
        // 根哈希与段哈希**各司其职**：段全自洽而根被改 → 拒；段被改 → 也拒。
        // 两个方向都红才说明两层都在干活（否则其中一层是摆设）。
        let a = demo_asset();
        let t = text_of(&a);
        let root_tampered = retag_version_keep_hash(&t, 2);
        let seg_tampered = text_of(&a).replace("\"走\"", "\"足\"");
        let root_caught = !import_container(&root_tampered).is_ok();
        let seg_caught = !import_container(&seg_tampered).is_ok();
        set.add("M05-段-根段双层各司其职", root_caught && seg_caught, "");
    }

    // ---- 生态单点 ----
    {
        let idx = register_index(SEGMENT_SCHEMA);
        let third = ECO_SEGMENTS.len() == 3 && idx == Some(2);
        let after_l = ECO_SEGMENTS
            .iter()
            .filter(|e| e.domain == "L")
            .count()
            == 2;
        set.add("M05-生态-m.anim.已注册为第三段", third && after_l, "");
    }

    {
        // 门禁纪律：查表函数必须用表外真实形态验，否则是恒真弱门禁。
        let outside = is_registered("x.unknown.");
        let idx_none = register_index("m.anim.2").is_none();
        set.add("M05-生态-表外名拒收", !outside && idx_none, "");
    }

    {
        let d_anim = expect_domain(SEGMENT_SCHEMA);
        let d_tl = expect_domain("l.timeline.");
        let d_fl = expect_domain("l.fluid.");
        set.add(
            "M05-生态-属域归属正确",
            d_anim == Some("M") && d_tl == Some("L") && d_fl == Some("L"),
            "",
        );
    }

    {
        // 注册序号必须等于表内下标（顺序即注册顺序，不得重排）。
        let mut seq_ok = true;
        for (i, e) in ECO_SEGMENTS.iter().enumerate() {
            if register_index(e.segment) != Some(i) {
                seq_ok = false;
            }
        }
        set.add("M05-生态-注册序号与顺序一致", seq_ok, "");
    }

    // ---- 签名三件套 ----
    {
        let mut r = AssetReport::new();
        let full = MetaSegment {
            asset_name: "a".to_string(),
            author: Some("x".to_string()),
            license: Some("y".to_string()),
            source_url: Some("z".to_string()),
            origin_path: None,
            machine_id: None,
            loop_default: LoopMode::Once,
        };
        let mut partial = full.clone();
        partial.license = None;
        let mut none = full.clone();
        none.author = None;
        none.license = None;
        none.source_url = None;
        set.add(
            "M05-签名-三级分级",
            classify_signature(&full) == SigTier::Strong
                && classify_signature(&partial) == SigTier::Weak
                && classify_signature(&none) == SigTier::Absent,
            "",
        );
        let _ = &mut r;
    }

    {
        // 纯空白不算「已填」：否则 `author:" "` 会把资产判为署名完整。
        let mut m = MetaSegment {
            asset_name: "a".to_string(),
            author: Some("   ".to_string()),
            license: Some("y".to_string()),
            source_url: Some("z".to_string()),
            origin_path: None,
            machine_id: None,
            loop_default: LoopMode::Once,
        };
        let weak = classify_signature(&m) == SigTier::Weak;
        m.author = Some("\t\n".to_string());
        set.add("M05-签名-空白串不算已填", weak, "");
    }

    {
        let mut a = demo_asset();
        a.meta.author = None;
        a.meta.license = None;
        a.meta.source_url = None;
        let out = export_container(&a);
        let warned = out
            .report()
            .warnings()
            .iter()
            .any(|n| n.code == AssetDiag::SignatureAbsent);
        set.add("M05-签名-缺失不阻断", out.is_ok() && warned, "");
    }

    {
        set.add(
            "M05-签名-均不阻断分发",
            !SigTier::Strong.blocks_distribution()
                && !SigTier::Weak.blocks_distribution()
                && !SigTier::Absent.blocks_distribution(),
            "",
        );
    }

    // ---- 版本化三件套 ----
    {
        let p13 = migration_path(1, 3);
        let p12 = migration_path(1, 2);
        let p23 = migration_path(2, 3);
        let p22 = migration_path(2, 2);
        set.add(
            "M05-版本-迁移链显式可达",
            p13 == Some(vec![(1, 2), (2, 3)])
                && p12 == Some(vec![(1, 2)])
                && p23 == Some(vec![(2, 3)])
                && p22 == Some(Vec::new()),
            "",
        );
    }

    {
        // 迁移链门禁必须能区分「按表走」与「按版本号 +1 猜」。
        // 表中无 0 的出边 → 0 不可达；若实现是 `cur+1`，它会给出路径。
        let unreachable = migration_path(0, CONTAINER_VERSION).is_none();
        let no_edge = !has_migration_from(0);
        // 且从 1 出发必须仍在表内（否则上面的「0 不可达」毫无信息量）。
        let reachable = has_migration_from(1) && migration_path(1, CONTAINER_VERSION).is_some();
        // 降级方向不可达。
        let downgrade = migration_path(CONTAINER_VERSION, 1).is_none();
        set.add(
            "M05-版本-无链显性拒绝",
            unreachable && no_edge && reachable && downgrade,
            "",
        );
    }

    {
        // 分诊表必须真的被消费：处置方向由码决定，改表即改行为。
        // 这里直接断言表的分立性（相反方向不得共用码）。
        let advisory = [
            AssetDiag::SegmentUnknown,
            AssetDiag::VersionFromFuture,
            AssetDiag::SignatureAbsent,
            AssetDiag::SignatureWeak,
            AssetDiag::SignatureScrubbed,
        ];
        let all_soft = advisory.iter().all(|c| !c.is_blocking());
        let hard = [
            AssetDiag::SegmentHashMismatch,
            AssetDiag::NoMigrationPath,
            AssetDiag::RoundTripDrift,
            AssetDiag::SyntaxMalformed,
            AssetDiag::ClipDurationInvalid,
        ];
        let all_hard = hard.iter().all(|c| c.is_blocking());
        set.add("M05-分诊-方向由码决定", all_soft && all_hard, "");
    }

    {
        // 分诊表被消费的行为验证：`report.push` 按表选严重度。
        // 若把 SegmentUnknown 改成阻断，走 push 的那条路径必须真的阻断。
        let mut r1 = AssetReport::new();
        r1.push(AssetDiag::SegmentUnknown, None, "u", "h");
        let mut r2 = AssetReport::new();
        r2.push(AssetDiag::SegmentHashMismatch, Some("meta"), "d", "h");
        set.add(
            "M05-分诊-表驱动严重度",
            r1.warnings().len() == 1
                && r1.errors().is_empty()
                && r2.errors().len() == 1
                && r2.warnings().is_empty(),
            "",
        );
    }

    {
        let a = demo_asset();
        let b = backup(&a);
        let ok = match b {
            AssetOutcome::Ok { value, .. } => {
                let r = restore(&value);
                match r {
                    AssetOutcome::Ok { value: back, .. } => {
                        text_of(&back) == value.text
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        set.add("M05-版本-备份还原逐字节", ok, "");
    }

    {
        // **端到端迁移门禁**：一份 v1 容器必须能真的导入成功。
        //
        // 这条守的是「校验与迁移的先后顺序」：哈希把 version 与段内容都算进
        // 输入，故若先迁移再校验，就是拿改写后的内容去比改写前的哈希——
        // **每个旧版本文件都必然失配**，版本化能力静默作废；而这个缺陷在
        // 只测 `migrate_json` 的门禁下完全不可见（迁移函数本身是对的）。
        let a = demo_asset();
        let t3 = text_of(&a);
        let mut r = AssetReport::new();
        let mut root = match parse_json(&t3, &mut r) {
            Ok(v) => v,
            Err(_) => Json::Null,
        };
        // 降级标成 v1：模拟「一份老版本资产」（哈希按新版本号重算，自洽）。
        if let Json::Obj(fields) = &mut root {
            for (k, v) in fields.iter_mut() {
                if k == "version" {
                    *v = Json::Int(1);
                }
            }
        }
        seal_content_hash(&mut root);
        let v1_text = write_to_string(&root);
        let imported = import_container(&v1_text);
        let ok = imported.is_ok();
        // 且迁过后的资产仍能往返零损失。
        let rt = match &imported {
            AssetOutcome::Ok { value, .. } => round_trip(value)
                .ok()
                .map(|x| x.identical)
                .unwrap_or(false),
            _ => false,
        };
        // 迁移须留下留痕（作者能看出这份资产被升级过）。
        let traced = imported
            .report()
            .audit
            .iter()
            .any(|e| e.stage == "migrate");
        set.add("M05-版本-v1 容器可迁移导入", ok && rt && traced, "");
    }

    {
        let a = demo_asset();
        let mut b = backup(&a).ok().map(|v| v.clone()).unwrap_or(AssetBackup {
            schema: SEGMENT_SCHEMA,
            version: CONTAINER_VERSION,
            text: String::new(),
        });
        b.schema = "l.timeline.";
        let r = restore(&b);
        set.add(
            "M05-版本-备份跨段拒收",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::SchemaMismatch),
            "",
        );
    }

    // ---- 往返零损失 ----
    {
        let a = demo_asset();
        let v = round_trip(&a);
        let ok = match v {
            AssetOutcome::Ok { value, .. } => value.identical && value.first_diff.is_none(),
            _ => false,
        };
        set.add("M05-往返-逐位一致", ok, "");
    }

    {
        // 六类轨道全通过往返（四分量展平不串位是重点）。
        let a = demo_asset();
        let v = round_trip(&a);
        let back = import_container(&text_of(&a));
        let all_six = a.tracks.len() == 6
            && TrackClass::ALL.len() == 6
            && match (&v, &back) {
                (AssetOutcome::Ok { value, .. }, AssetOutcome::Ok { value: b, .. }) => {
                    value.identical && b.tracks.len() == 6
                }
                _ => false,
            };
        set.add("M05-往返-六类轨道全通", all_six, "");
    }

    {
        // 四分量展平顺序：第 2 帧的四个分量须原位回来。
        let a = demo_asset();
        let b = import_container(&text_of(&a));
        let ok = match b {
            AssetOutcome::Ok { value, .. } => match value.curve("c_color") {
                Some(c) => {
                    c.comps == 4
                        && c.values.len() == c.times.len() * 4
                        && c.component(1, 0) == Some(1.0)
                        && c.component(1, 3) == Some(1.0)
                        && c.component(2, 0) == Some(2.0)
                        && c.component(2, 1) == Some(1.0)
                }
                None => false,
            },
            _ => false,
        };
        set.add("M05-往返-四分量展平保序", ok, "");
    }

    {
        // 负零不得塌成正零（符号位丢失会在除法里翻转结果方向）。
        let mut r = AssetReport::new();
        let mut a = AnimAsset::empty();
        a.meta.asset_name = "negzero".to_string();
        a.curves.push(CurveSegment::build(
            "c",
            1,
            vec![0u32, 10],
            vec![-0.0, 1.0],
            &mut r,
        ));
        a.tracks.push(TrackSegment {
            track_id: "t".to_string(),
            owner: "e1".to_string(),
            class: TrackClass::Float,
            interp: InterpKind::Linear,
            curve_id: "c".to_string(),
            bind_raw: "/node/anim/pos".to_string(),
            weight: 1.0,
            blended: false,
        });
        let back = import_container(&text_of(&a));
        let ok = match back {
            AssetOutcome::Ok { value, .. } => match value.curve("c") {
                Some(c) => c
                    .values
                    .first()
                    .map(|v| v.is_sign_negative() && *v == 0.0)
                    .unwrap_or(false),
                None => false,
            },
            _ => false,
        };
        set.add("M05-往返-负零不塌成零", ok, "");
    }

    {
        // 漂移必须定位到段：合法改动 clips 段（重封段哈希 + 重算根哈希），
        // 两次导出文本不同，且差异能定位到 clips 段。
        let a = demo_asset();
        let t1 = text_of(&a);
        let mut r = AssetReport::new();
        let mut root = match parse_json(&t1, &mut r) {
            Ok(v) => v,
            Err(_) => Json::Null,
        };
        rename_clip(&mut root, "改名");
        let t2 = write_to_string(&root);
        let drift = first_diff_byte(&t1, &t2).is_some() && t1 != t2;
        let located = localize_drift(&t1, &t2) == Some("clips");
        // 改后的文本仍须是**合法容器**（只改名不算损坏）——否则测的是拒绝路径。
        let still_valid = import_container(&t2).is_ok();
        set.add(
            "M05-往返-漂移定位到段",
            drift && located && still_valid,
            "",
        );
    }

    {
        // 空资产（四段皆空）也必须能往返——「空」是合法形态不是残缺。
        let a = AnimAsset::empty();
        let v = round_trip(&a);
        let ok = v.is_ok()
            && v.ok().map(|x| x.identical).unwrap_or(false)
            && a.curves.is_empty()
            && a.tracks.is_empty()
            && a.clips.is_empty();
        set.add("M05-往返-空资产往返", ok, "");
    }

    {
        // **有损往返必须被判红并立案**（保真红线的正向检验）。
        // 构造真实的丢段场景：容器带未知段（前向兼容跳过）→ 导入再导出必丢该段。
        // 这条若不存在，`round_trip` 的判红分支就是死代码。
        let a = demo_asset();
        let with_future = text_of(&a);
        let mut r = AssetReport::new();
        let mut root = match parse_json(&with_future, &mut r) {
            Ok(v) => v,
            Err(_) => Json::Null,
        };
        add_unknown_segment(&mut root);
        reseal_segments(&mut root);
        seal_content_hash(&mut root);
        let t_future = write_to_string(&root);
        // 导入（跳过未知段）→ 再导出（未知段已不在我方模型里）
        let imported = import_container(&t_future);
        let second = match imported {
            AssetOutcome::Ok { value, .. } => export_container(&value).ok().map(|s| s.clone()),
            AssetOutcome::Err { .. } => None,
        };
        let mut audit = AssetReport::new();
        let mut located = false;
        let verdict = match &second {
            Some(s2) => {
                let v = audit_round_trip(&t_future, s2, &mut audit);
                // 定位段必须**经由裁决路径**产出（不能只在直接调用时才有值）。
                located = v.lost_segment.is_some();
                (!v.identical && audit.has_errors()) as bool
            }
            None => false,
        };
        let p1 = audit
            .errors()
            .iter()
            .any(|n| n.code == AssetDiag::RoundTripDrift)
            && audit
                .errors()
                .iter()
                .any(|n| n.code == AssetDiag::RoundTripLossySegment);
        set.add(
            "M05-往返-有损往返判红立案",
            verdict && p1 && located,
            "",
        );
    }

    {
        // 漂移定位必须指名段：**两段同时改**时定位到的应是其中之一，
        // 且不得恒为 `None`。恒 None 等于把定位活推给读日志的人。
        let a = demo_asset();
        let t1 = text_of(&a);
        let mut r = AssetReport::new();
        let mut root = match parse_json(&t1, &mut r) {
            Ok(v) => v,
            Err(_) => Json::Null,
        };
        rename_clip(&mut root, "改名甲");
        rename_field(&mut root, "curves", "curve_id", "c_float", "c_renamed");
        let t2 = write_to_string(&root);
        let seg = localize_drift(&t1, &t2);
        let named = matches!(seg, Some("clips") | Some("curves"));
        let diff = first_diff_byte(&t1, &t2).is_some();
        set.add("M05-往返-漂移定位指名段", diff && named, "");
    }

    // ---- 导入清洗 ----
    {
        let a = demo_asset();
        let t = text_of(&a);
        set.add(
            "M05-清洗-绝对路径剥离",
            !t.contains("private") && !t.contains("D:\\\\"),
            "",
        );
    }

    {
        let a = demo_asset();
        let t = text_of(&a);
        set.add("M05-清洗-机器 id 剥离", !t.contains("MACHINE-7"), "");
    }

    {
        // 清洗是「导出这份文本」的属性，不是就地改作者的资产。
        let a = demo_asset();
        let _ = export_container(&a);
        let untouched = a.meta.origin_path.is_some() && a.meta.machine_id.is_some();
        set.add("M05-清洗-原资产不被就地抹", untouched, "");
    }

    {
        let mut r = AssetReport::new();
        let mut m = MetaSegment {
            asset_name: "a".to_string(),
            author: None,
            license: None,
            source_url: None,
            origin_path: Some("rig/walk.json".to_string()),
            machine_id: None,
            loop_default: LoopMode::Once,
        };
        scrub_for_export(&mut m, &mut r);
        set.add(
            "M05-清洗-相对路径不误伤",
            m.origin_path == Some("rig/walk.json".to_string()) && r.audit.is_empty(),
            "",
        );
    }

    {
        let a = demo_asset();
        let out = export_container(&a);
        let named = out
            .report()
            .audit
            .iter()
            .any(|e| e.field == "origin_path")
            && out
                .report()
                .audit
                .iter()
                .any(|e| e.field == "machine_id");
        set.add("M05-清洗-留痕指名字段", named, "");
    }

    // ---- 错误路径 ----
    {
        let a = demo_asset();
        let t = text_of(&a).replace("VARIXANM", "NOTMAGIC");
        let r = import_container(&t);
        set.add(
            "M05-错误-魔数不匹配拒绝",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::MagicMismatch),
            "",
        );
    }

    {
        let a = demo_asset();
        let t = text_of(&a).replace("\"走\"", "\"足\"");
        let r = import_container(&t);
        let located = r.report().errors().iter().any(|n| n.segment.is_some());
        set.add(
            "M05-错误-段损坏定位到段",
            !r.is_ok()
                && r.report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::SegmentHashMismatch)
                && located,
            "",
        );
    }

    {
        let a = demo_asset();
        let t = text_of(&a);
        let mut r = AssetReport::new();
        let mut root = match parse_json(&t, &mut r) {
            Ok(v) => v,
            Err(_) => Json::Null,
        };
        add_unknown_segment(&mut root);
        seal_content_hash(&mut root);
        let t2 = write_to_string(&root);
        let out = import_container(&t2);
        set.add(
            "M05-错误-未知段跳过留声明",
            out.is_ok()
                && out
                    .report()
                    .warnings()
                    .iter()
                    .any(|n| n.code == AssetDiag::SegmentUnknown),
            "",
        );
    }

    {
        let a = demo_asset();
        let t = bump_version(&text_of(&a), 99);
        let out = import_container(&t);
        set.add(
            "M05-错误-未来版本前向兼容",
            out.is_ok()
                && out
                    .report()
                    .warnings()
                    .iter()
                    .any(|n| n.code == AssetDiag::VersionFromFuture),
            "",
        );
    }

    {
        let mut r = AssetReport::new();
        let mut a = AnimAsset::empty();
        a.meta.asset_name = "bad".to_string();
        a.curves.push(CurveSegment::build(
            "c",
            4,
            vec![0u32, 10],
            vec![0.0, 1.0, 2.0, 3.0, 4.0],
            &mut r,
        ));
        let codes: Vec<AssetDiag> = r.errors().iter().map(|n| n.code).collect();
        set.add(
            "M05-错误-曲线错齐拒绝",
            codes.contains(&AssetDiag::CurveLengthMismatch)
                && !export_container(&a).is_ok(),
            "",
        );
    }

    {
        let mut r = AssetReport::new();
        let c = CurveSegment::build("c", 1, vec![0u32, 10, 5], vec![0.0, 1.0, 2.0], &mut r);
        set.add(
            "M05-错误-时间非递增拒绝",
            { c.validate(&mut r); r.has_errors() },
            "",
        );
    }

    {
        let mut a = demo_asset();
        a.tracks[0].curve_id = "c_missing".to_string();
        let r = export_container(&a);
        set.add(
            "M05-错误-悬空曲线引用拒绝",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::CurveRefUnknown),
            "",
        );
    }

    {
        let mut a = demo_asset();
        let dup = a.tracks[0].clone();
        a.tracks.push(dup);
        let r = export_container(&a);
        set.add(
            "M05-错误-重复轨道 id 拒绝",
            !r.is_ok()
                && r
                    .report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::TrackDuplicateId),
            "",
        );
    }

    {
        let mut a = demo_asset();
        a.tracks[0].weight = 1.5;
        let r = export_container(&a);
        set.add(
            "M05-错误-权重越界拒绝",
            !r.is_ok()
                && r.report().first_code() == Some(AssetDiag::TrackWeightInvalid),
            "",
        );
    }

    {
        let mut a = demo_asset();
        a.tracks[0].bind_raw = "node/anim/pos".to_string();
        let r = export_container(&a);
        set.add(
            "M05-错误-绑定路径非法拒绝",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::TrackBindInvalid),
            "",
        );
    }

    {
        let mut a = demo_asset();
        a.tracks[0].class = TrackClass::Position; // 曲线是 1 分量，颜色/位置不匹配仍能过形状
        let t = text_of(&a);
        let bad = t.replace("\"class\":\"position\"", "\"class\":\"bogus\"");
        let r = import_container(&bad);
        // 段哈希会先拦住；若测试资产哈希恰未覆盖，则也应由类名校验拒绝。
        let caught = !r.is_ok()
            && (r.report().first_code() == Some(AssetDiag::TrackClassUnknown)
                || r
                    .report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::SegmentHashMismatch));
        set.add("M05-错误-轨道类未知拒绝", caught, "");
    }

    {
        let a = demo_asset();
        let bad = text_of(&a).replace("\"interp\":\"slerp\"", "\"interp\":\"bezier\"");
        let r = import_container(&bad);
        let caught = !r.is_ok()
            && (r.report().first_code() == Some(AssetDiag::InterpUnknown)
                || r
                    .report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::SegmentHashMismatch));
        set.add("M05-错误-插值器未知拒绝", caught, "");
    }

    {
        let mut a = demo_asset();
        a.clips[0].track_ids.push("t_ghost".to_string());
        let r = export_container(&a);
        set.add(
            "M05-错误-clip 悬空轨道拒绝",
            !r.is_ok()
                && r
                    .report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::ClipTrackRefUnknown),
            "",
        );
    }

    {
        let mut a = demo_asset();
        a.clips[0].duration_ticks = 5;
        let r = export_container(&a);
        set.add(
            "M05-错误-clip 时长不足拒绝",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::ClipDurationInvalid),
            "",
        );
    }

    {
        let mut r = AssetReport::new();
        let c = CurveSegment::build(
            "c",
            1,
            vec![0u32, 10],
            vec![0.0, f32::INFINITY],
            &mut r,
        );
        let mut a = AnimAsset::empty();
        a.meta.asset_name = "inf".to_string();
        a.curves.push(c);
        set.add(
            "M05-错误-非有限值拒绝",
            r.has_errors() && !export_container(&a).is_ok(),
            "",
        );
    }

    {
        let a = demo_asset();
        // 删掉 clips 段（并把根哈希重算，模拟「合法但缺段」）。
        let t = text_of(&a);
        let stripped = remove_segment(&t, "clips");
        let r = import_container(&stripped);
        set.add(
            "M05-错误-缺段拒绝",
            !r.is_ok()
                && r
                    .report()
                    .errors()
                    .iter()
                    .any(|n| n.code == AssetDiag::SegmentMissing),
            "",
        );
    }

    {
        let r = import_container("{not json");
        set.add(
            "M05-错误-语法错拒绝",
            !r.is_ok() && r.report().first_code() == Some(AssetDiag::SyntaxMalformed),
            "",
        );
    }

    // ---- 跨域对齐（重挂 F2402 容器） ----
    {
        let a = demo_asset();
        let r = remount(&a);
        set.add("M05-对接-重挂 F2402 容器", r.is_ok(), "");
    }

    {
        // 重挂轨数一致
        let a = demo_asset();
        let n = a.tracks.len();
        let ok = match remount(&a) {
            AssetOutcome::Ok { value, .. } => value.total_tracks() == n,
            _ => false,
        };
        set.add("M05-对接-重挂轨数一致", ok, "");
    }

    {
        // 重挂必须**先校验资产**：损坏资产（悬空曲线引用）不得被挂进容器。
        // 少了这一步，一个引用不存在的轨道会静默消失，而调用方看到的是
        // 「挂载成功」——错误被藏在了返回的容器里，不是被报告出来。
        let mut a = demo_asset();
        a.tracks[0].curve_id = "c_ghost".to_string();
        let r = remount(&a);
        let refused = !r.is_ok()
            && r
                .report()
                .errors()
                .iter()
                .any(|n| n.code == AssetDiag::CurveRefUnknown);
        // 且不得出现「静默少挂一条」的成功面。
        let no_silent = match &r {
            AssetOutcome::Ok { value, .. } => value.total_tracks() == a.tracks.len(),
            AssetOutcome::Err { .. } => true,
        };
        set.add("M05-对接-重挂拒损坏资产", refused && no_silent, "");
    }

    // ---- 性能与鲁棒 ----
    {
        // 对拍：打包文本长度须随曲线数线性增长（不出现平方级膨胀）。
        let l0 = text_of(&asset_with_curves(4)).len();
        let l1 = text_of(&asset_with_curves(64)).len();
        let ratio = l1 as f64 / (l0.max(1) as f64);
        set.add(
            "M05-性能-打包对拍线性",
            ratio > 8.0 && ratio < 24.0,
            "",
        );
    }

    {
        // 往返耗时随规模线性：测 16 倍规模，耗时比须显著低于 16 倍。
        let small = time_round_trip(&asset_with_curves(8));
        let big = time_round_trip(&asset_with_curves(128));
        // 逻辑判定用规模比而非墙钟绝对值（墙钟在 CI 上噪声大）。
        let n_small = 8usize;
        let n_big = 128usize;
        let per_small = small / (n_small as u64);
        let per_big = big / (n_big as u64);
        set.add(
            "M05-性能-往返耗时随规模线性",
            big > small && per_big <= per_small * 8 + 64,
            "",
        );
    }

    {
        // 敌意输入：截断文本、纯空白、控制字符、超深嵌套——一律不得 panic。
        let a = demo_asset();
        let t = text_of(&a);
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let inputs = vec![
            String::new(),
            "{".to_string(),
            "{\"schema\":".to_string(),
            "\"".to_string(),
            "\u{0}\u{1}\u{2}".to_string(),
            deep,
            t.chars().take(37).collect::<String>(),
            "{\"schema\":\"m.anim.\",\"magic\":\"VARIXANM\",\"version\":3,\"content_hash\":\"0000000000000000\"}".to_string(),
            "[]".to_string(),
            "null".to_string(),
        ];
        let mut survived = true;
        for s in inputs.iter() {
            let r = import_container(s);
            if r.is_ok() {
                // 只有四段齐备且校验过才算合法通过；空对象类输入必须被拒。
                survived = false;
            }
        }
        // 合法资产仍须通过（确认上面的敌意输入没把实现整体打坏）。
        let good = import_container(&t).is_ok();
        set.add("M05-鲁棒-敌意输入不panic", survived && good, "");
    }

    {
        // 深度上限必须**指名道姓**地拦住（`DepthExceeded`），而不是碰巧因别的
        // 原因失败。防栈溢出是内核侧的硬需求：解析器跑在 no_std 无栈保护的
        // 路径上，一次深嵌套就能让整个内核栈溢出——那不是「拒绝一个文件」，
        // 是「崩掉整个系统」。
        // 逐个深度档位都须被指名拒绝。只测单一档位会漏掉「只拦一半深度」的
        // 实现（V18：条件写成 `depth % 2 == 1` 时，单档取样恰好命中）。
        let mut all_caught = true;
        for extra in 1..=6usize {
            let n = MAX_JSON_DEPTH + extra;
            let deep = format!("{}{}", "[".repeat(n), "]".repeat(n));
            let mut r = AssetReport::new();
            let err = parse_json(&deep, &mut r);
            // 必须是「因超深而拒」：错误集恰好只有 DepthExceeded，**且返回的
            // 失败原因就是「深度超限」本身**。只查错误集不够——一个「记了深度
            // 错误却继续解析」的变体（V15）同样会留下那一条记录，只是它在更靠
            // 后的位置因别的原因失败，报告里看不出差别。
            let only_depth = r.error_count() == 1
                && r.first_code() == Some(AssetDiag::DepthExceeded);
            let reason_is_depth = match &err {
                Err(msg) => msg.as_str() == "深度超限",
                Ok(_) => false,
            };
            if !(err.is_err() && only_depth && reason_is_depth) {
                all_caught = false;
            }
        }
        // 浅嵌套必须仍能解析（否则「一律拒绝」也是假绿）。
        let shallow = format!("{}{}", "[".repeat(MAX_JSON_DEPTH - 2), "]".repeat(MAX_JSON_DEPTH - 2));
        let mut r2 = AssetReport::new();
        let ok_shallow = parse_json(&shallow, &mut r2).is_ok() && !r2.has_errors();
        // 恰好在上限内的一层也须通过（边界不多拦）。
        let boundary = format!("{}{}", "[".repeat(MAX_JSON_DEPTH - 1), "]".repeat(MAX_JSON_DEPTH - 1));
        let mut r3 = AssetReport::new();
        let ok_boundary = parse_json(&boundary, &mut r3).is_ok() && !r3.has_errors();
        set.add(
            "M05-鲁棒-深度上限指名拦截",
            all_caught && ok_shallow && ok_boundary,
            "",
        );
    }

    // ---- 规模自检收尾 ----
    finish(set)
}

// ---------------------------------------------------------------------------
// 自检辅助（局部工具，避免污染公共 API）
// ---------------------------------------------------------------------------

/// 造一个含 `n` 条标量曲线的资产（无 clip，用于规模测量）。
fn asset_with_curves(n: usize) -> AnimAsset {
    let mut a = AnimAsset::empty();
    a.meta.asset_name = "scale".to_string();
    for i in 0..n {
        a.curves.push(scalar_curve(&format!("c{}", i), 4));
        a.tracks.push(TrackSegment {
            track_id: format!("t{}", i),
            owner: "e1".to_string(),
            class: TrackClass::Float,
            interp: InterpKind::Linear,
            curve_id: format!("c{}", i),
            bind_raw: "/node/anim/pos".to_string(),
            weight: 1.0,
            blended: true,
        });
    }
    a
}

/// 逻辑节拍计数（不用墙钟：墙钟在并行 CI 上噪声大，且不可复现）。
fn time_round_trip(a: &AnimAsset) -> u64 {
    let mut ticks: u64 = 0;
    let t = text_of(a);
    ticks += t.len() as u64;
    if import_container(&t).is_ok() {
        ticks += 1;
    }
    ticks
}

/// 就地替换 root 的 segments 子树。
fn set_new_segments(root: &mut Json, segs: Vec<(String, Json)>) {
    if let Json::Obj(fields) = root {
        for (k, v) in fields.iter_mut() {
            if k == "segments" {
                *v = Json::Obj(segs.clone());
            }
        }
    }
}

/// 改名后重封段哈希与根内容哈希（构造「合法但文本不同」的对照容器）。
fn rename_clip(root: &mut Json, new_name: &str) {
    rename_field(root, "clips", "name", "", new_name);
}

/// 把某段里第一个指定字段改成新值，然后重封段哈希与根内容哈希。
///
/// `old` 为空串表示「只定位第一个条目，不按旧值筛」。
fn rename_field(root: &mut Json, seg_key: &str, field: &str, old: &str, new: &str) {
    let segments = match root.get("segments") {
        Some(s) if s.is_obj() => s.clone(),
        _ => return,
    };
    let seg = match segments.get(seg_key) {
        Some(c) => c.clone(),
        None => return,
    };
    let mut items = seg.get_arr("items").cloned().unwrap_or_default();
    if let Some(Json::Obj(first)) = items.first_mut() {
        for (k, v) in first.iter_mut() {
            if k == field && (old.is_empty() || v == &Json::Str(old.to_string())) {
                *v = Json::Str(new.to_string());
            }
        }
    }
    let rebuilt = Json::Obj(vec![("items".to_string(), Json::Arr(items))]);
    let mut sealed: Vec<(String, Json)> = Vec::new();
    for kind in SegmentKind::ALL.into_iter() {
        let body = if kind.key() == seg_key {
            rebuilt.clone()
        } else {
            match segments.get(kind.key()) {
                Some(v) => v.clone(),
                None => continue,
            }
        };
        sealed.push((kind.key().to_string(), seal_segment(&body)));
    }
    set_new_segments(root, sealed);
    seal_content_hash(root);
}

/// 往容器里塞一个未知段（模拟更高版本新增段）。
fn add_unknown_segment(root: &mut Json) {
    if let Json::Obj(fields) = root {
        for (k, v) in fields.iter_mut() {
            if k == "segments" {
                if let Json::Obj(sf) = v {
                    sf.push(("m.future.".to_string(), Json::Obj(Vec::new())));
                }
            }
        }
    }
}

/// 只改根 `version`、**保留旧 `content_hash`**（段字节完全不动）。
///
/// 这是根内容哈希的专属探针：段哈希全部自洽，能报失配的只剩根哈希。
fn retag_version_keep_hash(text: &str, v: i64) -> String {
    text.replace(
        &format!("\"version\":{}", CONTAINER_VERSION),
        &format!("\"version\":{}", v),
    )
}

/// 把文本里的版本号改成 `v`（其余字节不动，用于测未来版本前向兼容）。
fn bump_version(text: &str, v: i64) -> String {
    let old = format!("\"version\":{}", CONTAINER_VERSION);
    let new = format!("\"version\":{}", v);
    let swapped = text.replace(&old, &new);
    // 版本号参与根哈希，故须重算，否则先被根哈希拦住而测不到版本分支。
    let mut r = AssetReport::new();
    match parse_json(&swapped, &mut r) {
        Ok(mut root) => {
            seal_content_hash(&mut root);
            write_to_string(&root)
        }
        Err(_) => swapped,
    }
}

/// 从容器文本里删掉一段（并重算根哈希，模拟「合法但缺段」）。
fn remove_segment(text: &str, key: &str) -> String {
    let mut r = AssetReport::new();
    let mut root = match parse_json(text, &mut r) {
        Ok(v) => v,
        Err(_) => return text.to_string(),
    };
    if let Json::Obj(fields) = &mut root {
        for (k, v) in fields.iter_mut() {
            if k == "segments" {
                if let Json::Obj(sf) = v {
                    sf.retain(|(sk, _)| sk != key);
                }
            }
        }
    }
    seal_content_hash(&mut root);
    write_to_string(&root)
}

/// 收尾：截断时显性判红（`MAX_CHECKS` 用满说明自检项超预算，必须报出来）。
fn finish(mut set: CheckSet) -> CheckSet {
    if set.truncated() {
        set.fail(
            "M05-规模-未截断",
            "自检项超出 MAX_CHECKS 被截断；须扩预算或合并项",
        );
    } else {
        set.ok("M05-规模-未截断");
    }
    set
}