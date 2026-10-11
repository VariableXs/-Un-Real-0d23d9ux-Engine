//! VE-F0604 · 域自检（判据逐条对应，见 `ved04_clip.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三形态（矩形/路径/圆角，圆角蒙版归 F0650） → `D04-形态-三形态域计算`
//! - 交集级联（父子取交、沿树收窄） → `D04-级联-祖先链逐层求交`
//! - 退化裁剪（空域）→子树跳过 → `D04-级联-空域子树跳过`
//! - 随层变换（局部域随世界矩阵） → `D04-变换-局部随世界矩阵`
//! - 整树剔除 O(1) → `D04-剔除-三案判定`
//! - 路径裁剪失效→降级包围盒并登记 → `D04-降级-路径失效降包围盒`
//! - 交集计算溢出→钳制 → `D04-防护-溢出钳制与非有限拒绝`
//! - F0611 命中一致性 → `D04-命中-渲染命中同域`
//! - 上游 F0601/F0602 对接（稳定 id + Mat2D） → `D04-对接-上游类型复用`
//! - 停用声明不产生裁剪 → `D04-级联-停用层跳过`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::ved02_xform::Mat2D;
use super::ved04_clip::*;
use crate::checks::CheckSet;

/// VE-F0604 域自检。
pub fn run_ved04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved04");

    // ---- 三形态 ----

    // 判据：矩形/路径/圆角三形态统一域计算；圆角按 AABB 且蒙版分工注记在册。
    {
        let r = ClipShape::Rect(RectF::new(0.0, 0.0, 100.0, 80.0).unwrap());
        let p = ClipShape::Path(PathDomain {
            bounds: RectF::new(10.0, 10.0, 60.0, 60.0).unwrap(),
            segments: 8,
        });
        let rd = ClipShape::Rounded {
            bounds: RectF::new(0.0, 0.0, 100.0, 80.0).unwrap(),
            radius: Radius4 { tl: 8.0, tr: 8.0, br: 0.0, bl: 0.0 },
        };
        let ok = r.domain().w == 100.0
            && p.domain().x == 10.0
            && rd.domain().h == 80.0
            && rd.is_rounded()
            && !r.is_rounded()
            && !ROUNDED_MASK_DOC.is_empty();
        set.add("D04-形态-三形态域计算", ok, "");
    }

    // ---- 交集级联：祖先链逐层求交 ----

    // 判据：三层链逐层收窄（0-200 ∩ 50-150 ∩ 80-180 = 80..150），深度账 O(链长)。
    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(RectF::new(0.0, 0.0, 200.0, 200.0).unwrap()));
        let _ = cs.declare(2, ClipShape::Rect(RectF::new(50.0, 50.0, 100.0, 100.0).unwrap()));
        let _ = cs.declare(3, ClipShape::Rect(RectF::new(80.0, 80.0, 100.0, 100.0).unwrap()));
        let id = Mat2D::IDENTITY;
        let eff = cs.effective_clip(&[(1, id), (2, id), (3, id)]).unwrap();
        let ok = match eff {
            EffectiveClip::Rect { bounds, depth_used, .. } => {
                bounds.x == 80.0 && bounds.w == 70.0 && depth_used == 3
            }
            _ => false,
        };
        set.add("D04-级联-祖先链逐层求交", ok, "");
    }

    // 判据：停用层不产生约束（声明保留、语义即关）。
    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(RectF::new(0.0, 0.0, 100.0, 100.0).unwrap()));
        let _ = cs.declare(2, ClipShape::Rect(RectF::new(200.0, 200.0, 50.0, 50.0).unwrap()));
        let _ = cs.set_enabled(2, false);
        let id = Mat2D::IDENTITY;
        let eff = cs.effective_clip(&[(1, id), (2, id)]).unwrap();
        let ok = matches!(eff, EffectiveClip::Rect { ref bounds, .. } if bounds.x == 0.0 && bounds.w == 100.0);
        set.add("D04-级联-停用层跳过", ok, "");
    }

    // ---- 退化空域 → 子树跳过 ----

    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(RectF::new(0.0, 0.0, 100.0, 100.0).unwrap()));
        let _ = cs.declare(2, ClipShape::Rect(RectF::new(500.0, 500.0, 10.0, 10.0).unwrap()));
        let id = Mat2D::IDENTITY;
        let eff = cs.effective_clip(&[(1, id), (2, id)]).unwrap();
        let empty_ok = matches!(eff, EffectiveClip::Empty { at_depth: 1 });
        let culled = cull_subtree(
            &RectF::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            &eff,
        ) == CullDecision::CullWholeSubtree;
        let audited = cs.audits().iter().any(|a| a.contains("子树跳过"));
        set.add("D04-级联-空域子树跳过", empty_ok && culled && audited, "");
    }

    // ---- 随层变换 ----

    // 判据：局部域随世界矩阵平移/缩放到世界域；旋转层取四角 AABB（注记）。
    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(RectF::new(0.0, 0.0, 100.0, 50.0).unwrap()));
        let t = cs
            .effective_clip(&[(1, Mat2D::translation(10.0, 20.0))])
            .unwrap();
        let t_ok = matches!(t, EffectiveClip::Rect { ref bounds, .. } if bounds.x == 10.0 && bounds.y == 20.0);
        let s = cs.effective_clip(&[(1, Mat2D::scaling(2.0, 2.0))]).unwrap();
        let s_ok = matches!(s, EffectiveClip::Rect { ref bounds, .. } if bounds.w == 200.0 && bounds.h == 100.0);
        // 旋转 90°：(0,0,100,50) 的四角 AABB = (-50,0)-(0,100)。
        let r = cs
            .effective_clip(&[(1, Mat2D::rotation(core::f32::consts::FRAC_PI_2))])
            .unwrap();
        let r_ok = match r {
            EffectiveClip::Rect { bounds, .. } => (bounds.w - 50.0).abs() < 1e-3 && (bounds.h - 100.0).abs() < 1e-3,
            _ => false,
        };
        set.add("D04-变换-局部随世界矩阵", t_ok && s_ok && r_ok, "");
    }

    // ---- 整树剔除 O(1) 三案 ----

    {
        let clip = EffectiveClip::Rect {
            bounds: RectF::new(0.0, 0.0, 100.0, 100.0).unwrap(),
            rounded: false,
            depth_used: 1,
        };
        let outside = cull_subtree(&RectF::new(200.0, 200.0, 10.0, 10.0).unwrap(), &clip);
        let inside = cull_subtree(&RectF::new(10.0, 10.0, 20.0, 20.0).unwrap(), &clip);
        let partial = cull_subtree(&RectF::new(90.0, 90.0, 20.0, 20.0).unwrap(), &clip);
        let unclipped = cull_subtree(
            &RectF::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            &EffectiveClip::Unclipped,
        );
        set.add(
            "D04-剔除-三案判定",
            outside == CullDecision::CullWholeSubtree
                && inside == CullDecision::NoClipNeeded
                && partial == CullDecision::ClipToRect
                && unclipped == CullDecision::NoClipNeeded,
            "",
        );
    }

    // ---- 路径失效降级并登记 ----

    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(
            7,
            ClipShape::Path(PathDomain {
                bounds: RectF::new(0.0, 0.0, 40.0, 40.0).unwrap(),
                segments: 0,
            }),
        );
        let registered = cs.degraded().len() == 1 && cs.degraded()[0].contains("降级");
        let degraded_shape = matches!(cs.decl_of(7).map(|d| &d.shape), Some(ClipShape::Rect(_)));
        // 有效路径不降级。
        let _ = cs.declare(
            8,
            ClipShape::Path(PathDomain {
                bounds: RectF::new(0.0, 0.0, 40.0, 40.0).unwrap(),
                segments: 12,
            }),
        );
        let valid_kept = matches!(cs.decl_of(8).map(|d| &d.shape), Some(ClipShape::Path(_)));
        set.add("D04-降级-路径失效降包围盒", registered && degraded_shape && valid_kept, "");
    }

    // ---- 溢出钳制与非有限拒绝 ----

    {
        let big = RectF::new(0.0, 0.0, 10.0, 10.0).unwrap();
        let t = big.transformed_aabb(&Mat2D::scaling(COORD_CLAMP, COORD_CLAMP)).unwrap();
        let clamped = t.x + t.w <= COORD_CLAMP + 1.0;
        let m_nan = Mat2D { a: f32::NAN, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
        let nonfinite_rejected = big.transformed_aabb(&m_nan).is_none();
        // 非法半径显性拒绝。
        let mut cs = ClipSystem::new();
        let bad_radius = cs
            .declare(
                9,
                ClipShape::Rounded {
                    bounds: RectF::new(0.0, 0.0, 40.0, 40.0).unwrap(),
                    radius: Radius4 { tl: 100.0, tr: 0.0, br: 0.0, bl: 0.0 },
                },
            )
            .is_err();
        // 负尺寸显性拒绝。
        let negative = RectF::new(0.0, 0.0, -1.0, 1.0).is_err();
        set.add(
            "D04-防护-溢出钳制与非有限拒绝",
            clamped && nonfinite_rejected && bad_radius && negative,
            "",
        );
    }

    // ---- 命中一致性（F0611） ----

    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(RectF::new(0.0, 0.0, 100.0, 100.0).unwrap()));
        let _ = cs.declare(2, ClipShape::Rect(RectF::new(20.0, 20.0, 50.0, 50.0).unwrap()));
        let id = Mat2D::IDENTITY;
        let eff = cs.effective_clip(&[(1, id), (2, id)]).unwrap();
        let inside_allowed = eff.allows(30.0, 30.0);
        let outside_denied = !eff.allows(10.0, 10.0);
        let narrowed_denied = !eff.allows(75.0, 75.0);
        let empty_all_deny = !EffectiveClip::Empty { at_depth: 0 }.allows(0.0, 0.0);
        set.add(
            "D04-命中-渲染命中同域",
            inside_allowed && outside_denied && narrowed_denied && empty_all_deny,
            "",
        );
    }

    // ---- 上游对接（F0601 稳定 id + F0602 Mat2D） ----

    // 判据：声明按图层稳定 id 主键；世界矩阵直接复用 F0602 类型；重复登记覆盖留痕。
    {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(42, ClipShape::Rect(RectF::new(0.0, 0.0, 10.0, 10.0).unwrap()));
        let _ = cs.declare(42, ClipShape::Rect(RectF::new(0.0, 0.0, 20.0, 20.0).unwrap()));
        let id_keyed = cs.decls_len() == 1 && cs.decl_of(42).is_some();
        let overwritten = cs.decl_of(42).unwrap().shape.domain().w == 20.0;
        let audited = cs.audits().iter().any(|a| a.contains("覆盖更新"));
        let uses_mat2d = Mat2D::IDENTITY.a == 1.0; // 类型来自 F0602（ved02_xform）。
        set.add("D04-对接-上游类型复用", id_keyed && overwritten && audited && uses_mat2d, "");
    }

    // ---- 链深护栏与面板 ----

    {
        let mut cs = ClipSystem::new();
        let id = Mat2D::IDENTITY;
        let mut chain = alloc::vec::Vec::new();
        for i in 0..(MAX_CLIP_DEPTH + 1) {
            chain.push((i as u64, id));
        }
        let guarded = cs.effective_clip(&chain).is_err()
            && cs.errors().iter().any(|(_, c, _)| *c == "E_CHAIN_TOO_DEEP");
        let panel = cs.panel_text().contains("图层裁剪面板");
        set.add("D04-面板-链深护栏与诊断面板", guarded && panel, "");
    }

    set
}
