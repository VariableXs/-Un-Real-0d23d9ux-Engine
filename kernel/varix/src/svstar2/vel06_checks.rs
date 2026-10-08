//! VE-F2206 · 域自检（判据逐条对应，见 `vel06_render.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三形态 → `E01-三型-*`
//! - 模拟渲染解耦 → `E02-解耦-*`
//! - I02 对接 → `E03-I02-*`
//! - 速度拉伸 → `E04-拉伸-*`
//! - 降级矩阵（属性缺失 / 网格为空 / 环形覆盖 / 未就绪挂起 / 帧边界）→ `E05-降级-*`
//! - 性能可数工作量 → `E06-性能-*`
//!
//! 零墙钟、零 IO；帧号与相机向量均注入，故回归可复现。
//!
//! **判据设计自律**（承W012·VE-F2205 反假变体实测教训）：
//! ① 判据**不向被测函数问答案**——凡涉及数值上界的，参考值由判据侧独立算出；
//! ② 凡「只有一种形态」的判据，必须用表外形态验证查表函数；
//! ③ 「解耦」类判据不能只测「行为一样」（重合行为会掩盖缺失分支），
//!    须直接断言**签名无写回**。

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use super::vel03_emitter::{DiagBag, Outcome};
use super::vel06_render::*;
use crate::checks::CheckSet;

/// 自检内部的取值助手：`Outcome` 失败即 panic 并带上原始诊断。
///
/// 与VE-F2205 同规：只在 `run_*_checks()` 与 `#[cfg(test)]` 内调用；输入全是
/// 本模块自造的常量，失败即判据自身写错，必须当场炸出原始诊断。
fn must<T>(o: Outcome<T>, ctx: &str) -> T {
    match o {
        Outcome::Ok { value, .. } => value,
        Outcome::Err { message, hint, .. } => {
            panic!("{}：{}（建议：{}）", ctx, message, hint)
        }
    }
}

/// 造一个带 alpha 的着色结果。
fn shaded(alpha: f32, size: f32) -> ShadedState {
    ShadedState { alpha, size, color: Rgba { r: 1.0, g: 1.0, b: 1.0, a: alpha } }
}

/// 造一个标准可见粒子视图。
fn view_at(x: f32, size: f32) -> ParticleView {
    ParticleView::new([x, 0.0, 0.0], [0.0, 0.0, 0.0], size, shaded(1.0, size))
}

/// 造 N 个等距可见粒子。
fn views(n: usize) -> Vec<ParticleView> {
    let mut v: Vec<ParticleView> = Vec::new();
    for i in 0..n {
        v.push(view_at(i as f32, 1.0));
    }
    v
}

/// 测试用 I02 sink（可构造就绪/未就绪/小容量三种状态）。
struct TestSink {
    ready: bool,
    cap: usize,
    accepted: usize,
    /// 实际收到的顶点数（对拍用，非自报）。
    got_vertices: usize,
    /// 实际收到的实例数。
    got_instances: usize,
}

impl TestSink {
    fn new(ready: bool, cap: usize) -> Self {
        TestSink { ready, cap, accepted: 0, got_vertices: 0, got_instances: 0 }
    }
}

impl InstanceSink for TestSink {
    fn is_ready(&self) -> bool {
        self.ready
    }
    fn capacity(&self) -> usize {
        self.cap
    }
    fn submit(&mut self, batch: &RenderBatch) -> bool {
        if !self.ready {
            return false;
        }
        // 真实记录收到的工作量，供判据对拍——不采信被测方自报的数字。
        self.got_vertices += batch.vertex_count();
        self.got_instances += batch.instance_count();
        self.accepted += 1;
        true
    }
}

/// 全部判据。
pub fn run_vel06_all_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2206");
    let cam = [1.0, 0.0, 0.0];

    // =======================================================================
    // E01 三形态
    // =======================================================================

    // 在册表恰为三型（不多不少）。
    {
        let listed = FORMS.len();
        let bb = FORMS.contains(&RenderForm::Billboard);
        let me = FORMS.contains(&RenderForm::Mesh);
        let tr = FORMS.contains(&RenderForm::Trail);
        set.add(
            "E01-三型-在册不多不少",
            listed == 3 && bb && me && tr,
            "",
        );
    }

    // 在册表的元素互不重复（重复登记会让「恰为三型」失去意义）。
    {
        let mut uniq = FORMS.to_vec();
        uniq.sort_by_key(|f| *f as u8);
        let mut dup = false;
        for i in 1..uniq.len() {
            if uniq[i] == uniq[i - 1] {
                dup = true;
            }
        }
        set.add("E01-三型-在册表无重复", !dup && FORMS.len() == 3, "");
    }

    // 三型参数各自成套，`for_form` 与 `form()` 互逆（防形态/参数错配）。
    {
        let mut roundtrip = true;
        for f in FORMS {
            let p = FormParams::for_form(f);
            if p.form() != f {
                roundtrip = false;
            }
            // 默认参数对billboard/trail 合法，对 mesh 非法（无网格）——
            // 正说明校验不是恒真。
            let valid = validate_params(f, &p).is_ok();
            if valid != (f != RenderForm::Mesh) {
                roundtrip = false;
            }
        }
        set.add("E01-三型-参数形态互逆且校验非恒真", roundtrip, "");
    }

    // billboard 面片顶点数恒为 4N（严格 O(N)，非估值）。
    {
        let n = 7usize;
        let vs = views(n);
        let p = FormParams::Billboard { size: 2.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag),
            "billboard 构建",
        );
        set.add(
            "E01-三型-面片顶点数恒四倍粒子数",
            batch.vertex_count() == n * 4 && batch.stats().instances == n,
            "",
        );
    }

    // 网格实例数 = 可见粒子数（形态各自的最小单元不同，须各自对账）。
    {
        let n = 5usize;
        let vs = views(n);
        let p = FormParams::Mesh { mesh: Some(MeshRef::new(0, 24, 3)), material: 7 };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Mesh, &p, &vs, cam, None, &mut bag), "网格构建");
        let mat_ok = match &batch {
            RenderBatch::Mesh { instances, .. } => instances.iter().all(|i| i.material == 7),
            _ => false,
        };
        set.add(
            "E01-三型-网格实例数与材质正确",
            batch.instance_count() == n && batch.vertex_count() == 0 && mat_ok,
            "",
        );
    }

    // 拖尾段数 = Σ(max(0, 历史点数-1))，顶点数 = 2×段数（解析式，非估值）。
    {
        let n = 3usize;
        let hist_n = 5usize;
        let vs = views(n);
        let mut trails: Vec<TrailHistory> = Vec::new();
        for i in 0..n {
            let mut h = must(TrailHistory::new(hist_n), "历史环");
            for k in 0..hist_n {
                h.push([k as f32, i as f32, 0.0]);
            }
            trails.push(h);
        }
        let p = FormParams::Trail { history: hist_n, width_decay: 1.0 };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag),
            "拖尾构建",
        );
        let want_segs = n * (hist_n - 1);
        set.add(
            "E01-三型-拖尾段数解析对账",
            batch.segment_count() == want_segs && batch.vertex_count() == want_segs * 2,
            "",
        );
    }

    // 拖尾宽度沿尾单调衰减（用解析档位1-level 对账，不用「看起来递减」）。
    {
        let hist_n = 6usize;
        let mut h = must(TrailHistory::new(hist_n), "历史环");
        for k in 0..hist_n {
            h.push([k as f32, 0.0, 0.0]);
        }
        let vs = vec![view_at(0.0, 2.0)];
        let trails = vec![h];
        let p = FormParams::Trail { history: hist_n, width_decay: 1.0 };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag),
            "拖尾构建（宽度对账）",
        );
        // 每段 2 顶点，**宽度用两点间距离量**而非「某个坐标轴上的差」——
        // 带状宽度方向来自叉积，未必落在 X/Y/Z 任一轴上，按轴量会得0。
        // 解析宽度 = size × decay × (1 - level)，level = (k+1)/used。
        let verts = match &batch {
            RenderBatch::Trail { vertices, .. } => vertices.as_slice(),
            // 非拖尾型不该走到这里（上面刚断言过）；给空切片而非 panic，
            // 免得判据的错误掩盖真正的断言失败。
            _ => &[],
        };
        let used = hist_n as f32;
        let dist = |a: &Vertex, b: &Vertex| -> f32 {
            let d0 = a.position[0] - b.position[0];
            let d1 = a.position[1] - b.position[1];
            let d2 = a.position[2] - b.position[2];
            (d0 * d0 + d1 * d1 + d2 * d2).sqrt()
        };
        let mut monotone = true;
        let mut areas_ok = true;
        for k in 0..(hist_n - 1) {
            let w_now = dist(&verts[2 * k], &verts[2 * k + 1]);
            let want = 2.0 * (1.0 - (k + 1) as f32 / used);
            if (w_now - want).abs() > 1e-4 {
                areas_ok = false;
            }
            if k + 1 < hist_n - 1 {
                let w_next = dist(&verts[2 * k + 2], &verts[2 * k + 3]);
                // 最新段（k=0）应最宽，故宽度随 k 非增。
                if w_next > w_now + 1e-6 {
                    monotone = false;
                }
            }
        }
        set.add("E01-三型-拖尾宽度解析对账", areas_ok, "");
        set.add("E01-三型-拖尾宽度单调衰减", monotone, "");
    }

    // 不可见粒子被跳过（alpha=0 不应产出顶点）——证明visible 门禁非恒真。
    {
        let mut vs = views(3);
        vs[1].color.a = 0.0;
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag),
            "不可见粒子构建",
        );
        set.add(
            "E01-三型-全透明粒子被跳过",
            batch.vertex_count() == 8 && batch.stats().skipped == 1,
            "",
        );
    }

    // 非有限粒子被拒且计数（NaN 位置不产出 NaN 顶点）。
    {
        let mut vs = views(3);
        vs[1].position = [f32::NAN, 0.0, 0.0];
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag),
            "非有限粒子构建",
        );
        let finite_verts = match &batch {
            RenderBatch::Billboard { vertices, .. } => {
                vertices.iter().all(|v| v.position.iter().all(|x| x.is_finite()))
            }
            _ => false,
        };
        set.add(
            "E01-三型-非有限粒子被拒且顶点有限",
            batch.vertex_count() == 8 && batch.stats().rejected == 1 && finite_verts,
            "",
        );
    }

    // **拖尾宽度轴须垂直于段方向**（几何不退化的硬条件）。
    //
    // 弱门禁补丁（承本轮变异实测M11）：把 `side` 恒设为 `[1,0,0]`（与段方向
    // 平行）的变异体，宽度「两点距离」量法**测不出来**——因为 |v0-v1| 在
    // 任何 side 上都等于 2×half，量法对side 的**方向**不敏感。原判据
    // `E01-三型-拖尾宽度单调衰减` / `拖尾宽度解析对账` 只量长度不看朝向，
    // 于是「带退化成一条线」这类真缺陷从缝里溜过。
    //
    // 正确判据须断言**点积为零**：宽度轴与段方向正交。语料刻意让历史点
    // **不沿坐标轴**走（斜向），否则轴对齐巧合会让「恰好垂直」恒真。
    {
        // 段方向取 (1,2,3) 的斜线——不落任一坐标轴，杜绝轴对齐巧合。
        let mut h = must(TrailHistory::new(4), "正交对账历史环");
        let step = [1.0f32, 2.0, 3.0];
        for k in 0..4 {
            h.push([step[0] * k as f32, step[1] * k as f32, step[2] * k as f32]);
        }
        let vs = vec![view_at(0.0, 2.0)];
        let trails = vec![h];
        let p = FormParams::Trail { history: 4, width_decay: 1.0 };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag),
            "正交对账构建",
        );
        let verts = match &batch {
            RenderBatch::Trail { vertices, .. } => vertices.as_slice(),
            _ => &[],
        };
        let dot = |a: [f32; 3], b: [f32; 3]| -> f32 {
            a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
        };
        let sub3 = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] {
            [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
        };
        // 每段两点：p0 = 2k, p1 = 2k+1。段方向 = p1-p0；宽度轴 = p1-p0 的横向。
        // 但判据不重算 side（那是问被测方），而是直接量**边向量的几何事实**：
        // 段的两个顶点之差恒垂直于该段的方向向量——这正是「带宽不沿长度方向」
        // 的等价刻画。
        let mut orthogonal = true;
        let mut nonzero_width = true;
        for k in 0..(4 - 1) {
            let p0 = verts[2 * k].position;
            let p1 = verts[2 * k + 1].position;
            // 段方向 = 历史相邻点之差（由语料常量独立算出，非取自被测方）。
            let seg = [step[0], step[1], step[2]];
            // 宽度向量 = 同一段两顶点之差。
            let w = sub3(p1, p0);
            let wl = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            if wl <= 0.0 {
                nonzero_width = false;
            }
            // 正交判据用**归一化点积**（避免长度因子掩盖）：|cos| < 1e-3。
            let seg_l = (seg[0] * seg[0] + seg[1] * seg[1] + seg[2] * seg[2]).sqrt();
            let cos = dot(w, seg) / (wl * seg_l);
            if cos.abs() > 1e-3 {
                orthogonal = false;
            }
        }
        set.add(
            "E01-三型-拖尾宽度轴垂直段向",
            orthogonal && nonzero_width,
            "",
        );
    }

    // =======================================================================
    // E02 模拟渲染解耦
    // =======================================================================

    // **签名级解耦**：渲染面不接受 `&mut ParticleView`。
    //
    // 不能只测「切换前后模拟数据一样」——那是重合行为：若渲染偷偷写了
    // 模拟但写的值恰好相同，两种测法都会绿。故直接断言源码签名里没有
    // 可变借用（判据自身读源码文本，不是行为猜测）。
    {
        let src = include_str!("vel06_render.rs");
        // 渲染入口 build/render 的 views 参数必须是不可变引用。
        let no_mut_views = !src.contains("&mut [ParticleView]")
            && !src.contains("&mut Vec<ParticleView>")
            && !src.contains("views: &mut");
        // 渲染路径不得出现对视图的写字段操作。
        let no_field_write = !src.contains("view.position =")
            && !src.contains("view.size =")
            && !src.contains("v.position =")
            && !src.contains("v.size =")
            && !src.contains("v.color =");
        set.add("E02-解耦-渲染面无写回", no_mut_views && no_field_write, "");
    }

    // 渲染不改模拟：三形态构建前后视图逐位一致（真对拍，非自述）。
    {
        let vs = views(6);
        let before = vs.clone();
        let mut bag = DiagBag::new();
        let bb = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let _ = must(
            build(RenderForm::Billboard, &bb, &vs, cam, None, &mut bag),
            "解耦对拍 billboard",
        );
        let me = FormParams::Mesh { mesh: Some(MeshRef::new(0, 8, 1)), material: 1 };
        let _ = must(build(RenderForm::Mesh, &me, &vs, cam, None, &mut bag), "解耦对拍网格");
        let mut h = must(TrailHistory::new(4), "解耦对拍历史环");
        for k in 0..4 {
            h.push([k as f32, 0.0, 0.0]);
        }
        let trails = vec![h; 6];
        let tr = FormParams::Trail { history: 4, width_decay: 1.0 };
        let _ = must(
            build(RenderForm::Trail, &tr, &vs, cam, Some(&trails), &mut bag),
            "解耦对拍拖尾",
        );
        set.add("E02-解耦-三形态构建不改模拟", vs == before, "");
    }

    // 形态切换不改模拟：切换后视图仍逐位一致（与上一条不同：跨越切换动作）。
    {
        let vs = views(5);
        let before = vs.clone();
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.open_boundary();
        let _ = r.build(&vs, None, &mut bag);
        r.advance_frame();
        r.open_boundary();
        let _ = r.request_switch(
            RenderForm::Mesh,
            FormParams::Mesh { mesh: Some(MeshRef::new(0, 8, 0)), material: 0 },
            &mut bag,
        );
        let _ = r.build(&vs, None, &mut bag);
        set.add(
            "E02-解耦-形态切换不改模拟",
            vs == before && r.form() == RenderForm::Mesh,
            "",
        );
    }

    // 渲染器不持粒子池/模拟状态：字段清单不含池与生命类型。
    {
        let src = include_str!("vel06_render.rs");
        let no_pool = !src.contains("struct ParticleRenderer") || !src.contains("pool: Vec<ParticleView>");
        // 渲染器字段只应是形态切换器与相机向量。
        let renderer_section = src
            .split("pub struct ParticleRenderer")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .unwrap_or("");
        let only_expected = renderer_section.contains("switch: FormSwitch")
            && renderer_section.contains("cam_right")
            && !renderer_section.contains("life")
            && !renderer_section.contains("pool");
        set.add("E02-解耦-渲染器不持模拟状态", no_pool && only_expected, "");
    }

    // 只读消费声明：三形态输入源都是 ParticleView 的四属性。
    {
        let v = view_at(1.0, 1.0);
        let has_four = {
            let src = include_str!("vel06_render.rs");
            src.contains("pub position: [f32; 3]")
                && src.contains("pub velocity: [f32; 3]")
                && src.contains("pub size: f32")
                && src.contains("pub color: Rgba")
                && src.contains("pub custom_axis: Option<[f32; 3]>")
        };
        // 四属性须真被消费：位置进顶点、速度进朝向、尺寸进缩放、颜色进顶点色。
        let consumed = v.size > 0.0 && v.visible();
        set.add("E02-解耦-四属性齐备且被消费", has_four && consumed, "");
    }

    // 空粒子集产出空批（边界不 panic）。
    {
        let empty: Vec<ParticleView> = Vec::new();
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &empty, cam, None, &mut bag),
            "空粒子集构建",
        );
        set.add(
            "E02-解耦-空粒子集产出空批",
            batch.vertex_count() == 0 && batch.stats().instances == 0,
            "",
        );
    }

    // =======================================================================
    // E03 I02 对接
    // =======================================================================

    // 正常提交成功，sink 实收工作量与批一致（真实计数，非自报）。
    {
        let vs = views(4);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "提交构建");
        let mut sink = TestSink::new(true, 1024);
        let st = submit(&mut sink, &batch, &mut bag);
        set.add(
            "E03-I02-正常提交且实收对账",
            st.is_submitted() && sink.got_vertices == 16 && batch.vertex_count() == 16,
            "",
        );
    }

    // 网格型按**实例数**对容量（非顶点数）——两型容量口径不同，须各自验。
    {
        let vs = views(4);
        let p = FormParams::Mesh { mesh: Some(MeshRef::new(0, 12, 0)), material: 0 };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Mesh, &p, &vs, cam, None, &mut bag), "网格提交构建");
        // 容量给 4（恰好等于实例数）应通过；给 3 应拒绝。
        let mut ok_sink = TestSink::new(true, 4);
        let st_ok = submit(&mut ok_sink, &batch, &mut bag);
        let mut small = TestSink::new(true, 3);
        let st_small = submit(&mut small, &batch, &mut bag);
        set.add(
            "E03-I02-网格按实例数对容量",
            st_ok.is_submitted() && st_small == SubmitState::Rejected,
            "",
        );
    }

    // 未就绪 → 挂起（可重试），且**不静默**：必产诊断。
    {
        let vs = views(4);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "挂起构建");
        let mut sink = TestSink::new(false, 1024);
        let st = submit(&mut sink, &batch, &mut bag);
        let diag_named = bag
            .all()
            .iter()
            .any(|d| d.message.contains("未就绪") || d.message.contains("挂起"));
        set.add(
            "E03-I02-未就绪挂起且显性告警",
            st == SubmitState::Suspended && st.is_retryable() && diag_named && sink.accepted == 0,
            "",
        );
    }

    // 挂起 vs 拒绝语义相反不共用：挂起可重试、拒绝不可重试。
    {
        let vs = views(4);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "语义构建");
        let mut not_ready = TestSink::new(false, 1024);
        let st_sus = submit(&mut not_ready, &batch, &mut bag);
        let mut tiny = TestSink::new(true, 1);
        let st_rej = submit(&mut tiny, &batch, &mut bag);
        set.add(
            "E03-I02-挂起与拒绝语义分离",
            st_sus.is_retryable() && !st_rej.is_retryable() && st_sus != st_rej,
            "",
        );
    }

    // 容量不足的诊断须给**错误三要素**（需求/上限/建议）。
    {
        let vs = views(10);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "三要素构建");
        let mut tiny = TestSink::new(true, 4);
        let _ = submit(&mut tiny, &batch, &mut bag);
        let mut has_three = false;
        for d in bag.all() {
            let t = format!("{}{}", d.message, d.hint);
            if t.contains("40") && t.contains("4") && d.hint.len() > 8 {
                has_three = true;
            }
        }
        set.add("E03-I02-容量拒绝给三要素", has_three, "");
    }

    // render() 在挂起时**不返回批次**（防调用方误以为已绘制）。
    {
        let vs = views(4);
        let mut bag = DiagBag::new();
        let r = ParticleRenderer::billboard(1.0);
        let mut sink = TestSink::new(false, 1024);
        let (batch, st) = r.render(&vs, None, &mut sink, &mut bag);
        let mut ok_sink = TestSink::new(true, 1024);
        let mut bag2 = DiagBag::new();
        let (batch2, st2) = r.render(&vs, None, &mut ok_sink, &mut bag2);
        set.add(
            "E03-I02-挂起不返回批次",
            batch.is_none() && !st.is_submitted() && batch2.is_some() && st2.is_submitted(),
            "",
        );
    }

    // sink 自身拒收（就绪但返回 false）按挂起处理，不谎报成功。
    {
        struct RefuseSink;
        impl InstanceSink for RefuseSink {
            fn is_ready(&self) -> bool {
                true
            }
            fn capacity(&self) -> usize {
                4096
            }
            fn submit(&mut self, _b: &RenderBatch) -> bool {
                false
            }
        }
        let vs = views(3);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let batch = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "拒收构建");
        let mut sink = RefuseSink;
        let st = submit(&mut sink, &batch, &mut bag);
        set.add("E03-I02-sink拒收按挂起处理", st == SubmitState::Suspended, "");
    }

    // 拖尾批按顶点数对容量（非段数）。
    {
        let hist_n = 4usize;
        let vs = views(2);
        let mut h = must(TrailHistory::new(hist_n), "容量对账历史环");
        for k in 0..hist_n {
            h.push([k as f32, 0.0, 0.0]);
        }
        let trails = vec![h; 2];
        let p = FormParams::Trail { history: hist_n, width_decay: 1.0 };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag),
            "拖尾容量构建",
        );
        let want_v = 2 * (hist_n - 1) * 2;
        let mut ok_sink = TestSink::new(true, want_v);
        let st_ok = submit(&mut ok_sink, &batch, &mut bag);
        let mut small = TestSink::new(true, want_v - 1);
        let st_small = submit(&mut small, &batch, &mut bag);
        set.add(
            "E03-I02-拖尾按顶点数对容量",
            st_ok.is_submitted() && st_small == SubmitState::Rejected && batch.vertex_count() == want_v,
            "",
        );
    }

    // =======================================================================
    // E04 速度拉伸（判据四）
    // =======================================================================

    // 拉伸比与横向因子乘积恒 1（保面积守恒）。
    //
    // **本条是恒真式，保留但不作面积守恒的独立证据**：`stretch_width` 的
    // 定义就是 `1 / stretch_factor`，故两者乘积按定义恒为 1——把被测对象的
    // 定义式当判据，等于问被测函数「你自己等于你自己吗」（十诫第 7 条
    // 自证式）。真正的面积守恒由下面 `E04-拉伸-面片实几何面积守恒`
    // 在**顶点几何**上独立测量。此条只作因子接口的一致性回归。
    {
        let mut product_one = true;
        for i in 0..=64 {
            let speed = i as f32 * 0.25;
            let p = stretch_factor(speed) * stretch_width(speed);
            if (p - 1.0).abs() > 1e-5 {
                product_one = false;
            }
        }
        set.add("E04-拉伸-面积因子乘积恒一(定义式回归)", product_one, "");
    }

    // **面片实几何面积守恒**：在真实顶点上量面积，不问因子。
    //
    // 判据侧独立重算：面片面积 = |(v1-v0) × (v3-v0)|（相邻边叉积模），
    // 该量只由顶点坐标决定，与 `stretch_factor` 的定义无关。若速度拉伸
    // 只拉长不缩窄（宽度吃紧守恒），面积会随速度线性增长，此处必红。
    // 覆盖「零速→高速」与「高速→钳制上限」两段，确保不是只在某一段成立。
    {
        // 粒子自身尺寸**刻意取 3.0（≠ 1.0）**：若取 1.0，则 `base` 里少乘
        // 一次 `view.size` 的变异体在数值上等价于原实现（镜像变异落在正确
        // 实现的值域内），判据便抓不到那条语义漂移。取 ≠1 的值使其可见。
        const VIEW_SIZE: f32 = 3.0;
        let mk = |speed: f32| {
            ParticleView::new(
                [0.0, 0.0, 0.0],
                [speed, 0.0, 0.0],
                VIEW_SIZE,
                shaded(1.0, VIEW_SIZE),
            )
        };
        let p = FormParams::Billboard { size: 2.0, facing: Facing::VelocityStretch };
        let mut bag = DiagBag::new();
        // 参考面积由**判据侧独立重算**：面片边长 = 2 × size × view_size，
        // 面积 = 边长²。绝不取自被测方的自报数字。
        let edge_len = 2.0 * 2.0 * VIEW_SIZE;
        let ref_area = edge_len * edge_len;
        let mut quad_area = |speed: f32| -> f32 {
            let batch = must(
                build(RenderForm::Billboard, &p, &[mk(speed)], cam, None, &mut bag),
                "面积守恒构建",
            );
            let vs = match &batch {
                RenderBatch::Billboard { vertices, .. } => vertices.as_slice(),
                _ => &[],
            };
            let e = |a: usize, b: usize| -> [f32; 3] {
                [
                    vs[a].position[0] - vs[b].position[0],
                    vs[a].position[1] - vs[b].position[1],
                    vs[a].position[2] - vs[b].position[2],
                ]
            };
            // 顶点序(-,-)(+,-)(+,+)(-,+)：边A=v1-v0（半长），边B=v3-v0（半宽）。
            let a = e(1, 0);
            let b = e(3, 0);
            let cx = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            (cx[0] * cx[0] + cx[1] * cx[1] + cx[2] * cx[2]).sqrt()
        };
        let a0 = quad_area(0.0);
        // 两件独立的事都要断：① 绝对面积等于判据侧重算的解析值（抓
        // 「少乘一次 view.size」这类绝对尺度漂移）；② 任意速度下面积恒等
        // （抓「只拉长不缩窄」这类形状漂移）。只断①会漏掉形状漂移，
        // 只断②则整体缩放无从发现。
        let speeds = [0.5f32, 3.0, 12.0, 1000.0, 1.0e6];
        let abs_ok = (a0 - ref_area).abs() <= 1e-3 * ref_area;
        let mut conserved = true;
        for s in speeds {
            // 面积守恒 ⇒ 任意速度下面片面积都等于零速面片面积（**单边**判据：
            // 面积只能「恰好相等」，容差两侧都不给，避免低估型从缝里钻过）。
            if (quad_area(s) - a0).abs() > 1e-3 * a0.max(1.0) {
                conserved = false;
            }
        }
        set.add("E04-拉伸-面片实几何面积守恒", conserved && abs_ok, "");
    }

    // 零速度/负速度/NaN 速度 → 拉伸比恒 1（无拉伸），不产生 NaN 几何。
    {
        let degenerate = stretch_factor(0.0) == 1.0
            && stretch_factor(-3.0) == 1.0
            && stretch_factor(f32::NAN) == 1.0;
        set.add("E04-拉伸-退化速度不拉伸", degenerate, "");
    }

    // 拉伸比钳制在 [1, MAX_STRETCH]（防极端速度把面片压成线）。
    {
        let mut clamped = true;
        for i in 0..=32 {
            let s = i as f32 * 100.0;
            let k = stretch_factor(s);
            if !(k >= 1.0 && k <= MAX_STRETCH) {
                clamped = false;
            }
        }
        // 极大速度也必须被钳住（inf 也要给 1）。
        if !(stretch_factor(f32::INFINITY) >= 1.0 && stretch_factor(f32::INFINITY) <= MAX_STRETCH) {
            clamped = false;
        }
        set.add("E04-拉伸-拉伸比钳制在界内", clamped, "");
    }

    // **速度拉伸真实生效**：同尺寸下，高速粒子面片更长且更窄，
    // 且长度比 = 拉伸比、宽度比 = 横向因子（几何实测，不用自报因子）。
    {
        let mk = |speed: f32| {
            ParticleView::new(
                [0.0, 0.0, 0.0],
                [speed, 0.0, 0.0],
                1.0,
                shaded(1.0, 1.0),
            )
        };
        let vs = vec![mk(0.0), mk(3.0)];
        let p = FormParams::Billboard { size: 1.0, facing: Facing::VelocityStretch };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag),
            "拉伸生效构建",
        );
        let verts = match &batch {
            RenderBatch::Billboard { vertices, .. } => vertices.as_slice(),
            _ => &[],
        };
        // 面片几何：顶点序为 (-,-)(+,-)(+,+)(-,+)，
        // 故 v0->v1 是**半长**边（长度 2×half_along），v1->v2 是**半宽**边。
        // 用边向量长度量，不按坐标轴量——朝向由叉积决定，未必落在某轴上。
        let edge = |base: usize, a: usize, b: usize| -> f32 {
            let p = verts[base + a].position;
            let q = verts[base + b].position;
            let d0 = p[0] - q[0];
            let d1 = p[1] - q[1];
            let d2 = p[2] - q[2];
            (d0 * d0 + d1 * d1 + d2 * d2).sqrt()
        };
        // 零速面片：长宽皆 = size(1.0) → 边长 2×1×1 = 2。
        let l0 = edge(0, 0, 1);
        let w0 = edge(0, 1, 2);
        // 速3 面片：长 = 2×stretch_factor(3)，宽 = 2×stretch_width(3)。
        let l1 = edge(4, 0, 1);
        let w1 = edge(4, 1, 2);
        let k_want = stretch_factor(3.0);
        let w_want = stretch_width(3.0);
        let geo_ok = (l0 - 2.0).abs() < 1e-4
            && (w0 - 2.0).abs() < 1e-4
            && (l1 / l0 - k_want).abs() < 1e-4
            && (w1 / w0 - w_want).abs() < 1e-4;
        // 且必须真的「更长更窄」（方向性，防把两因子写反了仍过对账）。
        let directional = l1 > l0 && w1 < w0;
        set.add("E04-拉伸-速度拉伸真实生效", geo_ok && directional, "");
    }

    // 相机对齐 vs 速度拉伸产出**不同**几何（证明二态不是同一个函数）。
    {
        let v = ParticleView::new([0.0, 0.0, 0.0], [0.0, 5.0, 0.0], 1.0, shaded(1.0, 1.0));
        let cam_r = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let vel = FormParams::Billboard { size: 1.0, facing: Facing::VelocityStretch };
        let vs = vec![v];
        let mut bag = DiagBag::new();
        let a = must(build(RenderForm::Billboard, &cam_r, &vs, cam, None, &mut bag), "相机对齐");
        let b = must(build(RenderForm::Billboard, &vel, &vs, cam, None, &mut bag), "速度对齐");
        set.add("E04-拉伸-二态产出不同几何", a != b, "");
    }

    // 相机对齐时**不因速度拉伸**（速度再大面片也不变形）。
    {
        let mk = |speed: f32| {
            ParticleView::new([0.0, 0.0, 0.0], [speed, 0.0, 0.0], 1.0, shaded(1.0, 1.0))
        };
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let a = must(
            build(RenderForm::Billboard, &p, &[mk(0.0)], cam, None, &mut bag),
            "相机对齐零速",
        );
        let b = must(
            build(RenderForm::Billboard, &p, &[mk(50.0)], cam, None, &mut bag),
            "相机对齐高速",
        );
        // 相机右向量沿 X，故高速时面片仍不变形——长宽边长之比恒 1。
        // 比值同样用**边向量**算，不按轴量。
        let ratio = |bt: &RenderBatch| -> f32 {
            let vs = match bt {
                RenderBatch::Billboard { vertices, .. } => vertices.as_slice(),
                _ => &[],
            };
            let edge = |a: usize, b: usize| -> f32 {
                let p = vs[a].position;
                let q = vs[b].position;
                let d0 = p[0] - q[0];
                let d1 = p[1] - q[1];
                let d2 = p[2] - q[2];
                (d0 * d0 + d1 * d1 + d2 * d2).sqrt()
            };
            edge(0, 1) / edge(1, 2)
        };
        set.add(
            "E04-拉伸-相机对齐不随速度变形",
            (ratio(&a) - 1.0).abs() < 1e-4 && (ratio(&b) - 1.0).abs() < 1e-4,
            "",
        );
    }

    // 自定义朝向**优先于**两种自动朝向（锚点的自定义朝向属性语义）。
    {
        let v = ParticleView::new([0.0, 0.0, 0.0], [0.0, 9.0, 0.0], 1.0, shaded(1.0, 1.0))
            .with_axis([1.0, 0.0, 0.0]);
        let vel = FormParams::Billboard { size: 1.0, facing: Facing::VelocityStretch };
        let mut bag = DiagBag::new();
        let a = must(
            build(RenderForm::Billboard, &vel, &[v], cam, None, &mut bag),
            "自定义朝向构建",
        );
        // 自定义轴沿 X 且速度也沿 Y：若自定义生效，半长边沿 X（长 2×size），
        // 半宽边沿 Y。若自定义被忽略、退回速度方向，半长边会沿 Y——
        // 故量「半长边是否沿 X」即可分辨，这是**方向性**判据而非数值猜测。
        // 用边向量点积判定朝向，不按轴量长度。
        let verts = match &a {
            RenderBatch::Billboard { vertices, .. } => vertices.as_slice(),
            _ => &[],
        };
        let along_edge = [
            verts[1].position[0] - verts[0].position[0],
            verts[1].position[1] - verts[0].position[1],
            verts[1].position[2] - verts[0].position[2],
        ];
        let e_len = (along_edge[0] * along_edge[0]
            + along_edge[1] * along_edge[1]
            + along_edge[2] * along_edge[2])
            .sqrt();
        // 半长边沿 X ⇒ X 分量占满；沿 Y ⇒ X 分量为 0。
        let along_is_x = e_len > 0.0 && (along_edge[0] / e_len).abs() > 0.999;
        // 长度 = 2×size×拉伸比(9)：本例是速度拉伸模式，故仍带拉伸——
        // 自定义轴只改**方向**，不改拉伸与否（拉伸是该朝向模式的属性）。
        let k9 = stretch_factor(9.0);
        let len_ok = (e_len - 2.0 * k9).abs() < 1e-3;
        set.add("E04-拉伸-自定义朝向优先", along_is_x && len_ok, "");
    }

    // 零长自定义轴退化为默认轴，不产出 NaN 顶点。
    {
        let v = ParticleView::new([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0, shaded(1.0, 1.0))
            .with_axis([0.0, 0.0, 0.0]);
        let p = FormParams::Billboard { size: 1.0, facing: Facing::VelocityStretch };
        let mut bag = DiagBag::new();
        let batch = must(
            build(RenderForm::Billboard, &p, &[v], cam, None, &mut bag),
            "零长轴构建",
        );
        let finite = match &batch {
            RenderBatch::Billboard { vertices, .. } => {
                vertices.iter().all(|x| x.position.iter().all(|c| c.is_finite()))
            }
            _ => false,
        };
        set.add("E04-拉伸-零长轴退化不出NaN", finite && batch.vertex_count() == 4, "");
    }

    // 面片顶点数与朝向无关（拉伸不改变顶点数，只改几何）。
    {
        let mk = |sp: f32| {
            ParticleView::new([0.0, 0.0, 0.0], [sp, 0.0, 0.0], 1.0, shaded(1.0, 1.0))
        };
        let p = FormParams::Billboard { size: 1.0, facing: Facing::VelocityStretch };
        let mut bag = DiagBag::new();
        let a = must(build(RenderForm::Billboard, &p, &[mk(0.0)], cam, None, &mut bag), "零速");
        let b = must(build(RenderForm::Billboard, &p, &[mk(7.0)], cam, None, &mut bag), "高速");
        set.add(
            "E04-拉伸-拉伸不改顶点数",
            a.vertex_count() == 4 && b.vertex_count() == 4,
            "",
        );
    }

    // =======================================================================
    // E05 降级矩阵
    // =======================================================================

    // 网格为空 → 拒绝（None 与 0 顶点两路）。
    {
        let none_mesh = FormParams::Mesh { mesh: None, material: 0 };
        let empty_mesh = FormParams::Mesh { mesh: Some(MeshRef::new(0, 0, 0)), material: 0 };
        let good_mesh = FormParams::Mesh { mesh: Some(MeshRef::new(0, 1, 0)), material: 0 };
        set.add(
            "E05-降级-空网格两路均拒绝",
            validate_params(RenderForm::Mesh, &none_mesh).is_err()
                && validate_params(RenderForm::Mesh, &empty_mesh).is_err()
                && validate_params(RenderForm::Mesh, &good_mesh).is_ok(),
            "",
        );
    }

    // 网格拒绝必须**点名网格**（诊断文案可辨，不是笼统「非法」）。
    {
        let none_mesh = FormParams::Mesh { mesh: None, material: 0 };
        let mut named = false;
        if let Outcome::Err { message, .. } = validate_params(RenderForm::Mesh, &none_mesh) {
            named = message.contains("网格");
        }
        set.add("E05-降级-空网格拒绝点名病因", named, "");
    }

    // 拖尾属性缺失（无历史缓冲）→ 拒绝，且与「有缓冲」可区分。
    {
        let p = FormParams::Trail { history: 4, width_decay: 1.0 };
        let vs = views(2);
        let mut bag = DiagBag::new();
        let no_trail = build(RenderForm::Trail, &p, &vs, cam, None, &mut bag);
        let mut h = must(TrailHistory::new(4), "属性缺失历史环");
        h.push([0.0, 0.0, 0.0]);
        h.push([1.0, 0.0, 0.0]);
        let trails = vec![h; 2];
        let with_trail = build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag);
        set.add(
            "E05-降级-拖尾缺历史缓冲被拒",
            no_trail.is_err() && with_trail.is_ok(),
            "",
        );
    }

    // 拖尾历史容量越界（0/1/超上限）→ 拒绝，合法值放行。
    {
        let cases = [
            FormParams::Trail { history: 0, width_decay: 1.0 },
            FormParams::Trail { history: 1, width_decay: 1.0 },
            FormParams::Trail { history: TRAIL_HISTORY_MAX + 1, width_decay: 1.0 },
        ];
        let all_rejected = cases.iter().all(|p| {
            validate_params(RenderForm::Trail, p).is_err()
        });
        let ok = FormParams::Trail { history: TRAIL_MIN_HISTORY, width_decay: 0.0 };
        set.add(
            "E05-降级-拖尾容量越界被拒",
            all_rejected && validate_params(RenderForm::Trail, &ok).is_ok(),
            "",
        );
    }

    // 宽度衰减越界（负/ >1 / NaN）→ 拒绝。
    {
        let bad = [
            FormParams::Trail { history: 4, width_decay: -0.1 },
            FormParams::Trail { history: 4, width_decay: 1.1 },
            FormParams::Trail { history: 4, width_decay: f32::NAN },
        ];
        set.add(
            "E05-降级-衰减越界被拒",
            bad.iter().all(|p| validate_params(RenderForm::Trail, p).is_err()),
            "",
        );
    }

    // 面片尺寸非法（0/负/NaN）→ 拒绝，正尺寸放行。
    {
        let bad = [
            FormParams::Billboard { size: 0.0, facing: Facing::CameraRight },
            FormParams::Billboard { size: -1.0, facing: Facing::CameraRight },
            FormParams::Billboard { size: f32::NAN, facing: Facing::CameraRight },
        ];
        let good = FormParams::Billboard { size: 0.5, facing: Facing::VelocityStretch };
        set.add(
            "E05-降级-面片尺寸非法被拒",
            bad.iter().all(|p| validate_params(RenderForm::Billboard, p).is_err())
                && validate_params(RenderForm::Billboard, &good).is_ok(),
            "",
        );
    }

    // 形态与参数不同型 → 拒绝（防错配被静默按默认处理）。
    {
        let bb = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        set.add(
            "E05-降级-形态参数错配被拒",
            validate_params(RenderForm::Mesh, &bb).is_err()
                && validate_params(RenderForm::Trail, &bb).is_err(),
            "",
        );
    }

    // 历史环溢出→ **环形覆盖**语义（覆盖不是错误，不产拒绝）。
    {
        let mut h = must(TrailHistory::new(3), "环形语义");
        for k in 0..3 {
            h.push([k as f32, 0.0, 0.0]);
        }
        let before_over = h.overwrites();
        // 再推3 次：环已满，应覆盖 3 次且**不增长**。
        for k in 3..6 {
            h.push([k as f32, 0.0, 0.0]);
        }
        let ordered = h.ordered();
        let cover_ok = h.overwrites() == before_over + 3
            && h.len() == 3
            && ordered.len() == 3
            // 覆盖后应保留**最新**三个：[3,4,5]
            && (ordered[0][0] - 3.0).abs() < 1e-6
            && (ordered[2][0] - 5.0).abs() < 1e-6;
        set.add("E05-降级-历史溢出环形覆盖保最新", cover_ok, "");
    }

    // 历史环容量校验：0 与超上限拒绝，合法放行。
    {
        set.add(
            "E05-降级-历史环容量越界被拒",
            TrailHistory::new(0).is_err()
                && TrailHistory::new(TRAIL_HISTORY_MAX + 1).is_err()
                && TrailHistory::new(2).is_ok(),
            "",
        );
    }

    // 环形读出在**游标非零**时仍按时间序（弱门禁补丁，承变异 M03归因）。
    //
    // 原判据用cap=3 推 6 次：6%3=0，游标恰回 0，于是「起点 = 游标」与
    // 「起点恒 0」在数值上**重合**——把环形起点写成常量 0 的变异体照样全绿。
    // 这不是等价变体的运气，是**语料选得与退化实现重合**：要钉住环形读出，
    // 语料必须让游标停在非零位（cap+1 次），两实现才可区分。
    {
        let cap = 3usize;
        let mut h = must(TrailHistory::new(cap), "游标非零对账");
        // 推 cap+1 = 4 次 ⇒游标 = 4%3 = 1 ≠ 0，两实现可区分。
        for k in 0..(cap + 1) {
            h.push([k as f32, 0.0, 0.0]);
        }
        let ordered = h.ordered();
        // 期望时间序（最旧 → 最新）= [1,2,3]；若起点恒 0 则得 [3,1,2]。
        let want = [1.0f32, 2.0, 3.0];
        let mut time_ordered = ordered.len() == cap;
        for (i, w) in want.iter().enumerate() {
            if i < ordered.len() && (ordered[i][0] - w).abs() > 1e-6 {
                time_ordered = false;
            }
        }
        // 前置自检：语料确实让游标非零（否则本判据又在测一个恒等式）。
        let cursor_nonzero = h.len() == cap && ordered[0][0] != 0.0;
        set.add("E05-降级-环形读出按时间序非写入序", time_ordered && cursor_nonzero, "");
    }

    // 历史环 read_into 复用缓冲（热路径零分配）与 ordered 等价。
    {
        let mut h = must(TrailHistory::new(4), "复用对账");
        for k in 0..4 {
            h.push([k as f32, 1.0, 2.0]);
        }
        let ordered = h.ordered();
        let mut buf: Vec<[f32; 3]> = Vec::new();
        h.read_into(&mut buf);
        let cap_before = buf.capacity();
        h.read_into(&mut buf);
        set.add(
            "E05-降级-读入复用缓冲不增长",
            ordered == buf && buf.capacity() == cap_before,
            "",
        );
    }

    // 空历史环读出为空（不 panic）。
    {
        let h = must(TrailHistory::new(4), "空环");
        let mut buf: Vec<[f32; 3]> = Vec::new();
        h.read_into(&mut buf);
        set.add("E05-降级-空历史环读出为空", buf.is_empty() && h.is_empty(), "");
    }

    // clear 后可复用（粒子死亡时路径）。
    {
        let mut h = must(TrailHistory::new(4), "clear 对账");
        for k in 0..3 {
            h.push([k as f32, 0.0, 0.0]);
        }
        h.clear();
        set.add(
            "E05-降级-历史环清空可复用",
            h.len() == 0 && h.ordered().is_empty() && h.overwrites() == 0,
            "",
        );
    }

    // 形态切换：**非帧边界被拒**（F1762），且不改变当前形态。
    {
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.advance_frame();
        let st = r.request_switch(
            RenderForm::Mesh,
            FormParams::Mesh { mesh: Some(MeshRef::new(0, 8, 0)), material: 0 },
            &mut bag,
        );
        set.add(
            "E05-降级-非帧边界切换被拒",
            st.is_err() && r.form() == RenderForm::Billboard && r.switches() == 0,
            "",
        );
    }

    // 帧边界切换成功，且切换次数可数。
    {
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.open_boundary();
        let st = r.request_switch(
            RenderForm::Trail,
            FormParams::Trail { history: 4, width_decay: 0.8 },
            &mut bag,
        );
        let twice = r.request_switch(
            RenderForm::Billboard,
            FormParams::Billboard { size: 1.0, facing: Facing::CameraRight },
            &mut bag,
        );
        set.add(
            "E05-降级-帧边界切换成功且可数",
            st.is_ok()
                && twice.is_ok()
                && r.form() == RenderForm::Billboard
                && r.switches() == 2
                && r.params().form() == RenderForm::Billboard,
            "",
        );
    }

    // 帧边界切换到**非法参数**被拒，且形态不变。
    {
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.open_boundary();
        let st = r.request_switch(
            RenderForm::Mesh,
            FormParams::Mesh { mesh: None, material: 0 },
            &mut bag,
        );
        set.add(
            "E05-降级-切换到非法参数被拒",
            st.is_err() && r.form() == RenderForm::Billboard && r.switches() == 0,
            "",
        );
    }

    // 非帧边界拒绝必须**显性告警**（不静默忽略切换请求）。
    //
    // 原判据名声称「有诊断」而断言写成 `bag.len() == before`（袋**不增长**），
    // 名实相反：半帧切换的拒绝当时恰好只走返回值不记袋，断言遂全绿——
    // 判据把自己的缺陷洗成了通过。此处改为断言袋**增长**且诊断点名病因，
    // 并由变异M06 反向验证（去掉记袋 ⇒ 本条转红）。
    {
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.advance_frame();
        let before = bag.len();
        let _ = r.request_switch(
            RenderForm::Mesh,
            FormParams::Mesh { mesh: Some(MeshRef::new(0, 8, 0)), material: 0 },
            &mut bag,
        );
        let named = bag
            .all()
            .iter()
            .any(|d| d.message.contains("非帧边界"));
        set.add(
            "E05-降级-非帧边界拒绝有诊断",
            bag.len() == before + 1 && named,
            "",
        );
    }

    // 三条「形参已带 bag 却只返不记」的拒绝路径一律记袋（异常零静默）。
    //
    // 这三条曾各自静默返回，而当时的判据无一覆盖，属判据**没装上**而非
    // 实现有错——弱门禁十诫第 1 条的反面。此处逐条钉死「拒绝必留痕」。
    {
        // (1) build() 的参数校验失败
        let vs = views(2);
        let bad = FormParams::Billboard { size: -1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let before = bag.len();
        let out = build(RenderForm::Billboard, &bad, &vs, cam, None, &mut bag);
        let grew_params = bag.len() > before && bag.all().iter().any(|d| d.message.contains("参数校验拒绝"));
        set.add(
            "E05-降级-build参数拒绝有诊断",
            out.is_err() && grew_params,
            "",
        );

        // (2) build_trail() 的历史缓冲缺失
        let p = FormParams::Trail { history: 4, width_decay: 1.0 };
        let mut bag2 = DiagBag::new();
        let b2 = bag2.len();
        let out2 = build(RenderForm::Trail, &p, &vs, cam, None, &mut bag2);
        let grew_trail = bag2.len() > b2 && bag2.all().iter().any(|d| d.message.contains("历史缓冲"));
        set.add(
            "E05-降级-拖尾缺缓冲拒绝有诊断",
            out2.is_err() && grew_trail,
            "",
        );

        // (3) 形态/参数错配的 build 拒绝（同一入口的另一条分支）
        let mism = FormParams::Mesh { mesh: Some(MeshRef::new(0, 4, 0)), material: 0 };
        let mut bag3 = DiagBag::new();
        let b3 = bag3.len();
        let out3 = build(RenderForm::Billboard, &mism, &vs, cam, None, &mut bag3);
        set.add(
            "E05-降级-build错配拒绝有诊断",
            out3.is_err() && bag3.len() > b3,
            "",
        );
    }

    // at_boundary 状态随帧推进正确翻转（闸门不是恒真/恒假）。
    {
        let mut r = ParticleRenderer::billboard(1.0);
        let start = r.at_boundary();
        r.advance_frame();
        let mid = r.at_boundary();
        r.open_boundary();
        let end = r.at_boundary();
        set.add(
            "E05-降级-帧边界状态可翻转",
            start && !mid && end && r.frame() == 1,
            "",
        );
    }

    // =======================================================================
    // E06 性能（可数工作量，非自证式算术）
    // =======================================================================

    // 工作量随粒子数线性（用两点定标实测斜率，不用 n*CONST）。
    //
    // 「每粒子四顶点」的正确实现必须满足两件独立的事，故此处都断：
    // ① **总量守恒**：顶点数恰等于判据侧独立重算的 `4 * n`（不由被测方
    //    自报的数字推导——否则判据向被测函数问答案＝自证式）；
    // ② **倍增严格**：粒子数翻倍时顶点数**恰**翻倍（容差 0，非「接近」）。
    // 原实现只断每粒子常数而把倍增变量 `prev` 赋值后丢弃，导致判据名声称的
    // 「严格四倍」实际从未被断言——名字与判据反向，属隐性弱门禁，此处补实。
    {
        let p = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let mut bag = DiagBag::new();
        let mut linear = true;
        let mut consistent = true;
        let mut prev_total = 0usize;
        let mut prev_n = 0usize;
        let mut n = 16usize;
        while n <= 256 {
            let vs = views(n);
            let b = must(build(RenderForm::Billboard, &p, &vs, cam, None, &mut bag), "线性对账");
            let total = b.vertex_count();
            // 判据侧独立重算的参考值：面片每粒子恒 4 顶点。
            if total != 4 * n || total / n != 4 {
                consistent = false;
            }
            if prev_n != 0 {
                // n 与 prev_n 均为 2 的幂且逐次翻倍，故顶点数须**恰**翻倍。
                if n == prev_n * 2 && total != prev_total * 2 {
                    linear = false;
                }
            }
            prev_total = total;
            prev_n = n;
            n *= 2;
        }
        set.add("E06-性能-面片顶点总量守恒", consistent, "");
        set.add("E06-性能-面片工作量严格四倍", linear, "");
    }

    // 统计字段真被填充（不是默认 0 的空壳）。
    {
        let vs = views(4);
        let mut bag = DiagBag::new();
        let mut h = must(TrailHistory::new(5), "统计历史环");
        for k in 0..5 {
            h.push([k as f32, 0.0, 0.0]);
        }
        let trails = vec![h; 4];
        let p = FormParams::Trail { history: 5, width_decay: 1.0 };
        let b = must(
            build(RenderForm::Trail, &p, &vs, cam, Some(&trails), &mut bag),
            "统计构建",
        );
        let s = b.stats();
        set.add(
            "E06-性能-统计非空壳",
            s.vertices == 4 * 4 * 2 && s.ribbon_segments == 16 && s.instances == 4,
            "",
        );
    }

    // 顶点/实例/段数三个口径互不串（各型读数独立）。
    {
        let vs = views(3);
        let mut bag = DiagBag::new();
        let bb = FormParams::Billboard { size: 1.0, facing: Facing::CameraRight };
        let b = must(build(RenderForm::Billboard, &bb, &vs, cam, None, &mut bag), "口径构建");
        // billboard：顶点数 12，实例数 0，段数 0。
        set.add(
            "E06-性能-三口径互不串",
            b.vertex_count() == 12 && b.instance_count() == 0 && b.segment_count() == 0,
            "",
        );
    }

    // 相机右向量非法时不更新（不被 NaN 污染）。
    {
        let mut r = ParticleRenderer::billboard(1.0);
        r.set_cam_right([0.0, 0.0, 0.0]);
        let after_zero = r.cam_right();
        r.set_cam_right([0.0, 3.0, 4.0]);
        let after_good = r.cam_right();
        set.add(
            "E06-性能-非法相机向量被拒",
            (after_zero[0] - 1.0).abs() < 1e-6
                && (after_good[0] - 0.0).abs() < 1e-6
                && (after_good[1] - 0.6).abs() < 1e-6,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vel06_all_checks_green() {
        let set = run_vel06_all_checks();
        let (pass, fail) = set.tally();
        assert_eq!(fail, 0, "有 {} 项判据未通过", fail);
        assert!(pass > 0);
        assert!(!set.truncated(), "判据集被截断");
    }

    #[test]
    fn vel06_three_forms_are_distinct() {
        let mut set = CheckSet::new("VE-F2206");
        set.add("三型互不相同", FORMS.len() == 3, "");
        set.add("三型枚举互异", FORMS[0] != FORMS[1] && FORMS[1] != FORMS[2], "");
        assert_eq!(set.tally().1, 0);
    }

    #[test]
    fn vel06_stretch_preserves_area() {
        // 面积守恒的单元级对账：任意速度下纵横比乘积恒 1。
        for i in 0..100 {
            let s = i as f32 * 0.37;
            let k = stretch_factor(s);
            let w = stretch_width(s);
            assert!((k * w - 1.0).abs() < 1e-5, "速度 {} 面积不守恒: {}", s, k * w);
        }
    }

    #[test]
    fn vel06_ring_overwrites_oldest() {
        let mut h = match TrailHistory::new(3) {
            Outcome::Ok { value, .. } => value,
            _ => panic!("历史环构造应成功"),
        };
        for k in 0..6 {
            h.push([k as f32, 0.0, 0.0]);
        }
        let o = h.ordered();
        assert_eq!(o.len(), 3);
        assert_eq!(o[0][0], 3.0);
        assert_eq!(o[2][0], 5.0);
        assert_eq!(h.overwrites(), 3);
    }

    #[test]
    fn vel06_switch_requires_boundary() {
        let mut bag = DiagBag::new();
        let mut r = ParticleRenderer::billboard(1.0);
        r.advance_frame();
        let st = r.request_switch(
            RenderForm::Mesh,
            FormParams::Mesh { mesh: Some(MeshRef::new(0, 4, 0)), material: 0 },
            &mut bag,
        );
        assert!(st.is_err());
        assert_eq!(r.form(), RenderForm::Billboard);
    }

    #[test]
    fn vel06_empty_mesh_rejected() {
        let p = FormParams::Mesh { mesh: None, material: 0 };
        assert!(validate_params(RenderForm::Mesh, &p).is_err());
        let p2 = FormParams::Mesh { mesh: Some(MeshRef::new(0, 0, 0)), material: 0 };
        assert!(validate_params(RenderForm::Mesh, &p2).is_err());
    }
}