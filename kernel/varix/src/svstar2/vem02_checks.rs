//! VE-F2402 · 域自检（判据逐条对应，见 `vem02_track.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **六类轨道** → `M02-六类-注册齐备`、`M02-六类-载荷目标一一对应`、
//!   `M02-六类-插值器族语义正确`、`M02-六类-规格自洽无缺口`；
//! - **容器多轨** → `M02-容器-六类同挂一实体`、`M02-容器-同类并存须显式blended`、
//!   `M02-容器-配额拒绝不截断`、`M02-容器-卸载索引自洽`、`M02-容器-权重区间校验`；
//! - **绑定协议** → `M02-绑定-路径语法五类拒绝`、`M02-绑定-三根与深度`、
//!   `M02-绑定-解析成功给目标`、`M02-绑定-目标销毁显性失效`、
//!   `M02-绑定-失效可逆恢复`、`M02-绑定-类型不匹配独立成码`；
//! - **单源扩展** → `M02-单源-声明指向F1345`、`M02-单源-禁止项齐备`、
//!   `M02-单源-本域不持关键帧数组`；
//! - 零静默纪律 → `M02-零静默-未注册拒绝并指引`、`M02-零静默-错误与告警分通道`、
//!   `M02-零静默-未知实体不假装成功`；
//! - 性能逐项分解 → `M02-性能-二分OlogN对拍线性`、`M02-性能-SoA分列布局`；
//! - 鲁棒面 → `M02-鲁棒-敌意输入不panic`。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vem02_track::*;
use crate::checks::CheckSet;

/// 便捷构造：满目录（覆盖六类目标属性）。
fn full_catalog() -> PropertyCatalog {
    let mut c = PropertyCatalog::new();
    c.insert("/node/transform/position", TargetType::Float3);
    c.insert("/node/transform/rotation", TargetType::Quaternion);
    c.insert("/node/transform/scale", TargetType::Float3);
    c.insert("/material/baseColor", TargetType::Float4);
    c.insert("/material/emissive", TargetType::Float4);
    c.insert("/custom/gameplay/opacity", TargetType::Float);
    c.insert("/custom/gameplay/open", TargetType::Bool);
    c
}

/// 便捷构造：某类的载荷。
fn payload_of(class: TrackClass, n: usize) -> TrackPayload {
    match class {
        TrackClass::Position | TrackClass::Scale => TrackPayload::Vec3(vec![[0.0, 0.0, 0.0]; n]),
        TrackClass::Rotation => TrackPayload::Quat(vec![[0.0, 0.0, 0.0, 1.0]; n]),
        TrackClass::Color => TrackPayload::Vec4(vec![[1.0, 1.0, 1.0, 1.0]; n]),
        TrackClass::Float => TrackPayload::Scalar(vec![0.0; n]),
        TrackClass::Bool => TrackPayload::Bool((0..n).map(|i| i % 2 == 0).collect()),
    }
}

/// 便捷构造：某类的默认绑定路径（目标类型与规格一致）。
fn default_path(class: TrackClass) -> &'static str {
    match class {
        TrackClass::Position => "/node/transform/position",
        TrackClass::Rotation => "/node/transform/rotation",
        TrackClass::Scale => "/node/transform/scale",
        TrackClass::Color => "/material/baseColor",
        TrackClass::Float => "/custom/gameplay/opacity",
        TrackClass::Bool => "/custom/gameplay/open",
    }
}

/// 便捷构造：挂载输入。
fn input_for(id: &str, owner: &str, class: TrackClass, n: usize) -> MountInput {
    MountInput::new(
        id,
        owner,
        class.as_str(),
        payload_of(class, n),
        KeyframeRef::new(&format!("kf_{id}"), n),
        default_path(class),
    )
}

/// VE-F2402 域自检。
pub fn run_vem02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem02");

    // ---- 六类轨道（判据一） ----

    {
        let mut bag = DiagBag::new();
        let audited = audit_six_classes(&mut bag);
        set.add(
            "M02-六类-注册齐备",
            audited.is_ok() && audited.ok() == Some(&6) && TRACK_SPECS.len() == 6,
            "",
        );
    }

    {
        // 逐类：载荷类型、目标类型、默认插值器、离散性、外推标志，逐一与锚点表对拍。
        let pos = TrackClass::Position.spec().unwrap();
        let rot = TrackClass::Rotation.spec().unwrap();
        let scl = TrackClass::Scale.spec().unwrap();
        let col = TrackClass::Color.spec().unwrap();
        let flt = TrackClass::Float.spec().unwrap();
        let bl = TrackClass::Bool.spec().unwrap();
        let ok = pos.payload_type == PayloadType::F32x3
            && pos.target_type == TargetType::Float3
            && pos.default_interp == InterpKind::Linear
            && pos.extrap_allowed
            && rot.payload_type == PayloadType::Quat
            && rot.target_type == TargetType::Quaternion
            && rot.default_interp == InterpKind::Slerp
            && !rot.extrap_allowed
            && !rot.allowed_interp.contains(&InterpKind::Linear)
            && scl.payload_type == PayloadType::F32x3
            && !scl.extrap_allowed
            && col.payload_type == PayloadType::F32x4
            && col.target_type == TargetType::Float4
            && flt.payload_type == PayloadType::F32
            && flt.target_type == TargetType::Float
            && bl.payload_type == PayloadType::Bool
            && bl.discrete
            && bl.default_interp == InterpKind::Step
            && !bl.extrap_allowed;
        set.add("M02-六类-载荷目标一一对应", ok, "");
    }

    {
        // 插值器族语义：布尔仅阶梯；旋转禁linear；四类连续类均为 linear。
        let bl = TrackClass::Bool.spec().unwrap();
        let rot = TrackClass::Rotation.spec().unwrap();
        let discrete_pure = bl.allowed_interp.len() == 1
            && bl.allowed_interp[0] == InterpKind::Step
            && !bl.allowed_interp.iter().any(|k| k.is_continuous());
        let rot_pure_slerp = rot.allowed_interp.len() == 1 && rot.allowed_interp[0] == InterpKind::Slerp;
        let continuous_linear = [TrackClass::Position, TrackClass::Scale, TrackClass::Color, TrackClass::Float]
            .iter()
            .all(|k| k.spec().unwrap().default_interp == InterpKind::Linear);
        set.add(
            "M02-六类-插值器族语义正确",
            discrete_pure && rot_pure_slerp && continuous_linear,
            "",
        );
    }

    {
        // 规格自洽：默认插值器在允许族内；离散类无连续插值器；连续类有连续插值器。
        let mut bag = DiagBag::new();
        let r = audit_six_classes(&mut bag);
        let self_consistent = r.is_ok()
            && TRACK_SPECS.iter().all(|s| {
                s.allowed_interp.contains(&s.default_interp)
                    && (!s.discrete || !s.allowed_interp.iter().any(|k| k.is_continuous()))
                    && (s.discrete || s.allowed_interp.iter().any(|k| k.is_continuous()))
            });
        let no_gap = bag.errors_by(DiagCode::TrackClassCoverageGap).is_empty()
            && bag.errors_by(DiagCode::TrackSpecInconsistent).is_empty();
        set.add("M02-六类-规格自洽无缺口", self_consistent && no_gap, "");
    }

    // ---- 容器多轨（判据二） ----

    {
        let mut c = TrackContainer::new();
        let mut all_ok = true;
        for k in TrackClass::ALL {
            let id = format!("t_{}", k.as_str());
            if !c.mount(input_for(&id, "ent_multi", k, 4)).is_ok() {
                all_ok = false;
            }
        }
        let count = c.count_of("ent_multi");
        let active = c.active_indices("ent_multi").len();
        let kinds: Vec<TrackClass> = c
            .tracks_of("ent_multi")
            .iter()
            .map(|t| t.class)
            .collect();
        // 六类互不覆盖：容器里恰有六个不同类。
        let distinct = {
            let mut v = kinds.clone();
            v.sort_by_key(|k| k.as_str());
            v.dedup();
            v.len() == 6
        };
        set.add(
            "M02-容器-六类同挂一实体",
            all_ok && count == 6 && active == 6 && distinct && c.active_weight_sum("ent_multi") == 6.0,
            "",
        );
    }

    {
        let mut c = TrackContainer::new();
        let first = c.mount(input_for("d1", "ent_dup", TrackClass::Position, 3));
        let dup = c.mount(input_for("d2", "ent_dup", TrackClass::Position, 3));
        let blended = c
            .mount(input_for("d3", "ent_dup", TrackClass::Position, 3).with_blended(true));
        let ok = first.is_ok()
            && dup.code() == Some(DiagCode::TrackDuplicateMount)
            && blended.is_ok()
            && c.count_of("ent_dup") == 2;
        // 被拒轨道不得入池（零静默的反面：拒绝必须真的拒）。
        let rejected_absent = c.track("d2").is_none();
        set.add("M02-容器-同类并存须显式blended", ok && rejected_absent, "");
    }

    {
        let mut c = TrackContainer::new();
        let mut mounted = 0usize;
        for i in 0..TRACK_QUOTA_PER_ENTITY {
            let id = format!("q{i}");
            // 每条都用不同类名绕开同类重复检查：quota 是唯一变量。
            let classes = [
                "float",
                "position",
                "rotation",
                "scale",
                "color",
                "bool",
            ];
            let cn = classes[i % 6];
            let mut inp = MountInput::new(
                &id,
                "ent_q",
                cn,
                payload_of(TrackClass::from_name(cn).unwrap(), 2),
                KeyframeRef::new(&format!("kf_q{i}"), 2),
                default_path(TrackClass::from_name(cn).unwrap()),
            );
            // 六类各自在ent_q 里会撞同类，故第二个起一律 blended。
            if i >= 6 {
                inp = inp.with_blended(true);
            }
            if c.mount(inp).is_ok() {
                mounted += 1;
            }
        }
        let over = c.mount(input_for("q_over", "ent_q", TrackClass::Float, 2).with_blended(true));
        let ok = mounted == TRACK_QUOTA_PER_ENTITY
            && over.code() == Some(DiagCode::TrackQuotaExceeded)
            && c.count_of("ent_q") == TRACK_QUOTA_PER_ENTITY;
        set.add("M02-容器-配额拒绝不截断", ok, "");
    }

    {
        let mut c = TrackContainer::new();
        for (id, k) in [
            ("u1", TrackClass::Position),
            ("u2", TrackClass::Rotation),
            ("u3", TrackClass::Scale),
        ] {
            let _ = c.mount(input_for(id, "ent_u", k, 2));
        }
        let removed = c.unmount("u2");
        let again = c.unmount("u2");
        let ids: Vec<String> = c.tracks_of("ent_u").iter().map(|t| t.id.clone()).collect();
        let ok = removed
            && !again
            && c.count_of("ent_u") == 2
            && c.track("u2").is_none()
            && ids == vec!["u1".to_string(), "u3".to_string()];
        set.add("M02-容器-卸载索引自洽", ok, "");
    }

    {
        let mut c = TrackContainer::new();
        let nan = c.mount(input_for("w1", "ent_w", TrackClass::Float, 2).with_weight(f32::NAN));
        let hi = c.mount(input_for("w2", "ent_w", TrackClass::Float, 2).with_weight(2.0));
        let lo = c.mount(input_for("w3", "ent_w", TrackClass::Float, 2).with_weight(-1.0));
        let inf = c.mount(input_for("w4", "ent_w", TrackClass::Float, 2).with_weight(f32::INFINITY));
        let good = c.mount(input_for("w5", "ent_w", TrackClass::Float, 2).with_weight(0.5));
        let bad = DiagCode::TrackWeightInvalid;
        set.add(
            "M02-容器-权重区间校验",
            nan.code() == Some(bad)
                && hi.code() == Some(bad)
                && lo.code() == Some(bad)
                && inf.code() == Some(bad)
                && good.is_ok()
                && c.count_of("ent_w") == 1,
            "",
        );
    }

    // ---- 绑定协议（判据三） ----

    {
        let cases: [&str; 8] = [
            "node/transform/position",
            "/",
            "/bogus/x",
            "/node//position",
            "/node/transform/",
            "/node/1bad",
            "/node/a.b",
            "/node",
        ];
        let mut all_rejected = true;
        for raw in cases {
            let mut bag = DiagBag::new();
            if parse_bind_path(raw, &mut bag).is_some() {
                all_rejected = false;
            }
            if bag.first_code() != Some(DiagCode::BindPathMalformed) {
                all_rejected = false;
            }
            if bag.error_count() != 1 {
                all_rejected = false;
            }
        }
        set.add("M02-绑定-路径语法五类拒绝", all_rejected, "");
    }

    {
        let mut bag = DiagBag::new();
        let n = parse_bind_path("/node/transform/position", &mut bag).unwrap();
        let m = parse_bind_path("/material/baseColor", &mut bag).unwrap();
        let cu = parse_bind_path("/custom/gameplay/open", &mut bag).unwrap();
        let ok = n.root == BindRoot::Node
            && m.root == BindRoot::Material
            && cu.root == BindRoot::Custom
            && n.depth() == 3
            && m.depth() == 2
            && n.leaf() == "position"
            && m.leaf() == "baseColor"
            && BindRoot::ALL.len() == 3
            && bag.error_count() == 0;
        set.add("M02-绑定-三根与深度", ok, "");
    }

    {
        let catalog = full_catalog();
        let mut bag = DiagBag::new();
        let p = parse_bind_path("/material/baseColor", &mut bag).unwrap();
        let r = resolve_binding(&p, &catalog);
        match r {
            BindResolution::Resolved(t) => {
                let ok = t.target_type == TargetType::Float4
                    && t.leaf == "baseColor"
                    && t.root == BindRoot::Material;
                set.add("M02-绑定-解析成功给目标", ok, "");
            }
            _ => set.add("M02-绑定-解析成功给目标", false, ""),
        }
    }

    {
        let mut c = TrackContainer::new();
        let mut cat = full_catalog();
        let _ = c.mount(input_for("alive", "ent_i", TrackClass::Position, 2));
        let _ = c.mount(input_for("doomed", "ent_i", TrackClass::Float, 2));
        let first = c.detect_invalidation("ent_i", &cat);
        let ok_first = first.ok().map(|r| r.dead.is_empty()).unwrap_or(false);

        // 目标销毁。
        let removed = cat.remove("/custom/gameplay/opacity");
        let rep = c.detect_invalidation("ent_i", &cat);
        let r = match rep.ok() {
            Some(r) => r.clone(),
            None => {
                set.add("M02-绑定-目标销毁显性失效", false, "");
                InvalidationReport {
                    dead: Vec::new(),
                    recovered: Vec::new(),
                    diagnostics: Vec::new(),
                }
            }
        };
        let explicit_warn = r
            .diagnostics
            .iter()
            .any(|d| d.code == DiagCode::TrackInvalidated);
        let t = c.track("doomed").unwrap();
        let ok = removed
            && ok_first
            && r.dead == vec!["doomed".to_string()]
            && explicit_warn
            && t.state == TrackState::Invalid
            && !t.invalid_reason.is_empty()
            && c.active_indices("ent_i").len() == 1;
        set.add("M02-绑定-目标销毁显性失效", ok, "");
    }

    {
        let mut c = TrackContainer::new();
        let mut cat = full_catalog();
        let _ = c.mount(input_for("rev", "ent_r", TrackClass::Float, 2));
        cat.remove("/custom/gameplay/opacity");
        let _ = c.detect_invalidation("ent_r", &cat);
        let was_invalid = c.track("rev").map(|t| t.state) == Some(TrackState::Invalid);
        cat.insert("/custom/gameplay/opacity", TargetType::Float);
        let rep = c.detect_invalidation("ent_r", &cat);
        let recovered = rep.ok().map(|r| r.recovered.clone()).unwrap_or_default();
        let t = c.track("rev").unwrap();
        set.add(
            "M02-绑定-失效可逆恢复",
            was_invalid
                && recovered == vec!["rev".to_string()]
                && t.state == TrackState::Active
                && t.invalid_reason.is_empty(),
            "",
        );
    }

    {
        let mut c = TrackContainer::new();
        let mut cat = full_catalog();
        // 颜色轨道绑到 float 属性 → 类型不匹配（目标在，类型错）。
        let r = c.mount(MountInput::new(
            "mm",
            "ent_mm",
            "color",
            payload_of(TrackClass::Color, 2),
            KeyframeRef::new("kf_mm", 2),
            "/custom/gameplay/opacity",
        ));
        let mounted = r.is_ok();
        let rep = c.detect_invalidation("ent_mm", &cat);
        let d = rep.ok().map(|x| x.diagnostics.clone()).unwrap_or_default();
        let mismatch = d
            .iter()
            .any(|x| x.code == DiagCode::BindTypeMismatch);

        // 目标没了 → 必须是另一个码（处置方向不同，故分立）。
        cat.remove("/custom/gameplay/opacity");
        let rep2 = c.detect_invalidation("ent_mm", &cat);
        let d2 = rep2.ok().map(|x| x.diagnostics.clone()).unwrap_or_default();
        let unresolved = d2.iter().any(|x| x.code == DiagCode::TrackInvalidated);
        let distinct = !d2.iter().any(|x| x.code == DiagCode::BindTypeMismatch);
        set.add(
            "M02-绑定-类型不匹配独立成码",
            mounted && mismatch && unresolved && distinct,
            "",
        );
    }

    // ---- 单源扩展（判据四） ----

    {
        let v = assert_single_source();
        let ok = v.pass
            && SINGLE_SOURCE.data_owner.contains("F1345")
            && SINGLE_SOURCE.extends.contains("F1345");
        set.add("M02-单源-声明指向F1345", ok, "");
    }

    {
        let forbidden = SingleSourceDeclaration::FORBIDDEN;
        let ok = forbidden.len() == 3
            && forbidden.iter().all(|f| !f.trim().is_empty())
            && SingleSourceDeclaration::CONSUMED_TYPES
                == ["KeyframeRef", "TrackPayload"];
        set.add("M02-单源-禁止项齐备", ok, "");
    }

    {
        // 物检：本域轨道只持引用与载荷，无「时间戳数组 + 值数组」的第二套定义。
        // `KeyframeRef` 只有 asset_id + count（引用），本域的 SoA 时间列由调用方
        // 在求值期填入（`TrackSoa::times` 为空Vec），不来自任何轨道字段。
        let r = KeyframeRef::new("kf_x", 7);
        let ref_is_handle = r.count == 7 && r.asset_id == "kf_x";
        let mut c = TrackContainer::new();
        let _ = c.mount(input_for("h", "ent_h", TrackClass::Position, 3));
        let t = c.track("h").unwrap();
        // 轨道上没有时间戳字段：只有 F1345 引用 + 值载荷。
        let has_no_time_field = t.frames.count == 3 && t.payload.value_count() == 3;
        let soa_time_empty = {
            let mut bag = DiagBag::new();
            let soa = TrackSoa::from_container(&c, &mut bag).ok().cloned();
            soa.map(|s| s.times.is_empty()).unwrap_or(false)
        };
        set.add(
            "M02-单源-本域不持关键帧数组",
            ref_is_handle && has_no_time_field && soa_time_empty,
            "",
        );
    }

    // ---- 零静默纪律 ----

    {
        let mut c = TrackContainer::new();
        let r = c.mount(MountInput::new(
            "unreg",
            "ent_z",
            "velocity",
            payload_of(TrackClass::Position, 2),
            KeyframeRef::new("kf_unreg", 2),
            "/node/transform/position",
        ));
        let hint = r.err().map(|f| f.hint.clone()).unwrap_or_default();
        let listed = ["position", "rotation", "scale", "color", "float", "bool"]
            .iter()
            .all(|k| hint.contains(k));
        set.add(
            "M02-零静默-未注册拒绝并指引",
            r.code() == Some(DiagCode::TrackTypeUnregistered) && listed && c.total_tracks() == 0,
            "",
        );
    }

    {
        // 错误与告警必须分属两条通道：连续类配 step 只产告警、不过闸门。
        let mut c = TrackContainer::new();
        let r = c.mount(
            input_for("warn_only", "ent_ch", TrackClass::Position, 2)
                .with_interp(InterpKind::Step),
        );
        let mut bag = DiagBag::new();
        bag.warn(DiagCode::ContinuousTrackForcedToStep, "m", "h");
        bag.push(DiagCode::TrackWeightInvalid, "m", "h");
        let channels_separate = bag.error_count() == 1
            && bag.warnings().len() == 1
            && bag.total() == 2;
        let mount_ok = r.is_ok();
        let warned = match &r {
            Outcome::Ok { diagnostics, .. } => diagnostics
                .iter()
                .any(|d| d.code == DiagCode::ContinuousTrackForcedToStep),
            _ => false,
        };
        // 反向：离散类配连续插值必须拒绝（两码方向相反，不得合并）。
        let mut c2 = TrackContainer::new();
        let reject = c2.mount(
            input_for("hard", "ent_ch2", TrackClass::Bool, 2).with_interp(InterpKind::Linear),
        );
        set.add(
            "M02-零静默-错误与告警分通道",
            channels_separate
                && mount_ok
                && warned
                && reject.code() == Some(DiagCode::DiscreteTrackRequiresStep)
                && c2.total_tracks() == 0,
            "",
        );
    }

    {
        let c = TrackContainer::new();
        let of = c.of("ghost");
        let det = { let mut cc = c.clone(); cc.detect_invalidation("ghost", &full_catalog()) };
        let ok = of.code() == Some(DiagCode::EntityUnknown)
            && det.code() == Some(DiagCode::EntityUnknown)
            && c.count_of("ghost") == 0;
        set.add("M02-零静默-未知实体不假装成功", ok, "");
    }

    // ---- 性能逐项分解 ----

    {
        // 二分与线性对拍：400 帧、覆盖全表与首尾外。
        let mut times: Vec<u32> = Vec::new();
        let mut acc = 0u32;
        for i in 0..400u32 {
            acc += i % 7 + 1;
            times.push(acc);
        }
        let mut agree = true;
        for probe in 0..(acc + 10) {
            let span = locate_span(&times, probe);
            let expect = times.iter().rposition(|x| *x <= probe).unwrap_or(0);
            if span.lo != expect {
                agree = false;
            }
        }
        // 首帧时间戳是 `0 % 7 + 1 = 1`，故 probe=0 确实落在首帧之前。
        let head = locate_span(&times, 0);
        let tail = locate_span(&times, acc + 5);
        let empty_safe = locate_span(&[], 7).before_start;
        // 中间点亦须落在合法区间（非首尾、非 before/after）。
        let mid = locate_span(&times, times[200]);
        let mid_ok = !mid.before_start && !mid.after_end && mid.lo == 200;
        set.add(
            "M02-性能-二分OlogN对拍线性",
            agree && head.before_start && tail.after_end && empty_safe && mid_ok,
            "",
        );
    }

    {
        // SoA 分列：位置进 vec3 列、浮点进标量列、颜色/旋转进四分量列、布尔不进数值列。
        let mut c = TrackContainer::new();
        for k in TrackClass::ALL {
            let id = format!("s_{}", k.as_str());
            let _ = c.mount(input_for(&id, "ent_soa", k, 3));
        }
        let mut bag = DiagBag::new();
        let soa = TrackSoa::from_container(&c, &mut bag);
        let s = soa.ok().cloned();
        let ok = match s {
            Some(s) => {
                s.vecs3.len() == 2 // 位置 + 缩放
                    && s.scalars.len() == 1 // 浮点
                    && s.vecs4.len() == 2 // 颜色 + 旋转
                    && s.lane_count() == 5 // 布尔不进数值列
                    && s.times.is_empty() // 时间列由调用方填，本域不生成
                    && bag.error_count() == 0
            }
            None => false,
        };
        set.add("M02-性能-SoA分列布局", ok, "");
    }

    // ---- 鲁棒面 ----

    {
        let mut c = TrackContainer::new();
        // 敌意输入：空串类名、巨长路径、超配额、NaN 权重、空载荷、重复 id、超大帧数。
        let _ = c.mount(MountInput::new("", "", "", TrackPayload::Scalar(Vec::new()), KeyframeRef::new("", 0), ""));
        let _ = c.mount(MountInput::new(
            "long",
            "e",
            "position",
            payload_of(TrackClass::Position, 1),
            KeyframeRef::new("k", 1),
            &format!("/node/{}", "a".repeat(8192)),
        ));
        let _ = c.mount(input_for("dup_id", "e", TrackClass::Float, 1));
        let _ = c.mount(input_for("dup_id", "e", TrackClass::Float, 1));
        let _ = c.mount(input_for("mismatch", "e", TrackClass::Float, 1).with_weight(f32::NAN));
        let _ = c.mount(MountInput::new(
            "frame_lie",
            "e",
            "float",
            TrackPayload::Scalar(vec![1.0, 2.0]),
            KeyframeRef::new("k", 999),
            "/custom/gameplay/opacity",
        ));
        let _ = parse_bind_path("//", &mut DiagBag::new());
        let _ = parse_bind_path("", &mut DiagBag::new());
        let _ = locate_span(&[9, 5, 1], 4); // 乱序时间表
        // 存活面：巨长路径段名合法故可挂（8192 字符单段），`dup_id` 首挂成功；
        // 其余（空串类名/ 重复 id / NaN 权重 / 帧数谎报）全部被诊断拦下。
        // 关键不是「谁活下来」，而是**没有任何一条敌意输入绕过诊断或引发 panic**。
        let survived = c.total_tracks() == 2 && c.track("long").is_some() && c.track("dup_id").is_some();
        let no_panic = true; // 走到这里即未 panic
        set.add("M02-鲁棒-敌意输入不panic", survived && no_panic, "");
    }

    set
}
