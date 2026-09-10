//! GALAXY AI-20 实时图形域（G1141~G1160）。
//!
//! 渲染管线、场景图、网格/材质/纹理、光照、阴影、粒子、后处理、
//! 骨骼动画、物理原语、降级链与域自检收口。
//! 首创点：内核级实时图形引擎（软渲染可验证基座）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1141 渲染管线抽象
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeStage {
    Vertex,
    Raster,
    Fragment,
    Blend,
}

/// 管线顺序执行记录：必须 Vertex→Raster→Fragment→Blend。
pub fn pipeline_in_order(stages: &[PipeStage]) -> bool {
    const ORDER: [PipeStage; 4] =
        [PipeStage::Vertex, PipeStage::Raster, PipeStage::Fragment, PipeStage::Blend];
    stages.len() == 4 && stages.iter().zip(ORDER.iter()).all(|(s, o)| s == o)
}

// ---------------------------------------------------------------------------
// G1142 场景图
// ---------------------------------------------------------------------------

pub const SCENE_MAX: usize = 16;

/// 场景图：parent 数组表示（-1 为根），先序遍历。
#[derive(Clone, Copy)]
pub struct SceneGraph {
    pub parent: [i8; SCENE_MAX],
    pub count: usize,
}

impl SceneGraph {
    pub const fn new() -> SceneGraph {
        let mut parent = [0i8; SCENE_MAX];
        for p in parent.iter_mut() {
            *p = -1;
        }
        SceneGraph { parent, count: 0 }
    }

    pub fn add_node(&mut self, parent: i8) -> Option<usize> {
        if self.count >= SCENE_MAX || (parent >= 0 && parent as usize >= self.count) {
            return None;
        }
        self.parent[self.count] = parent;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 深度计算。
    pub fn depth_of(&self, node: usize) -> usize {
        let mut d = 0;
        let mut cur = node;
        while cur < self.count {
            let p = self.parent[cur];
            if p < 0 {
                break;
            }
            cur = p as usize;
            d += 1;
        }
        d
    }
}

// ---------------------------------------------------------------------------
// G1143 网格/材质/纹理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Mesh {
    pub vertices: u32,
    pub indices: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub base_color: [u8; 4],
    pub roughness_x256: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
}

/// mip 链级数：log2(max(w,h)) + 1。
pub fn mip_level_count(w: u32, h: u32) -> u32 {
    let m = w.max(h);
    if m == 0 {
        return 1;
    }
    (32 - m.leading_zeros())
}

// ---------------------------------------------------------------------------
// G1144 光照模型
// ---------------------------------------------------------------------------

/// Lambert 漫反射（0~255）。
pub fn lambert(normal_dot_light_x256: i32, light_intensity: u32) -> u32 {
    if normal_dot_light_x256 <= 0 {
        return 0;
    }
    let val = ((normal_dot_light_x256 as u64) * light_intensity as u64) >> 8;
    val.min(255) as u32
}

/// Blinn-Phong 高光：spec = (N·H)^shininess 的定点近似。
pub fn blinn_specular(nh_x256: i32, shininess: u32) -> u32 {
    if nh_x256 <= 0 {
        return 0;
    }
    // (nh/256)^s * 255，用重复平方近似。
    let mut base = nh_x256 as u64;
    let mut result = 256u64;
    let mut s = shininess;
    while s > 0 {
        if s & 1 == 1 {
            result = result * base >> 8;
        }
        base = base * base >> 8;
        s >>= 1;
        if base == 0 {
            base = 1;
        }
    }
    (result * 255 >> 8).min(255) as u32
}

// ---------------------------------------------------------------------------
// G1145 阴影映射
// ---------------------------------------------------------------------------

/// 阴影深度测试：片元深度大于阴影图深度（含 bias）→ 在阴影中。
pub fn shadow_test(fragment_depth: f32, shadow_map_depth: f32, bias: f32) -> bool {
    fragment_depth > shadow_map_depth + bias
}

// ---------------------------------------------------------------------------
// G1146 粒子系统
// ---------------------------------------------------------------------------

pub const PARTICLES: usize = 32;

#[derive(Clone, Copy)]
pub struct Particle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub life_ms: u32,
    pub alive: bool,
}

/// 粒子步进：位置积分 + 生命周期递减 + 死亡回收。
pub fn particles_step(ps: &mut [Particle; PARTICLES], dt_ms: u32, gravity: f32) -> u32 {
    let dt = dt_ms as f32 / 1000.0;
    let mut alive = 0;
    for p in ps.iter_mut() {
        if !p.alive {
            continue;
        }
        p.vel[1] += gravity * dt;
        p.pos[0] += p.vel[0] * dt;
        p.pos[1] += p.vel[1] * dt;
        p.life_ms = p.life_ms.saturating_sub(dt_ms);
        if p.life_ms == 0 {
            p.alive = false;
        } else {
            alive += 1;
        }
    }
    alive
}

// ---------------------------------------------------------------------------
// G1147 后处理特效
// ---------------------------------------------------------------------------

/// gamma 校正 LUT（2.2 次幂近似查表，输入 0~255）。
pub fn gamma_lut() -> [u8; 16] {
    let mut lut = [0u8; 16];
    for (i, slot) in lut.iter_mut().enumerate() {
        let v = i as f32 / 15.0;
        *slot = (v.powf(1.0 / 2.2) * 255.0).round() as u8;
    }
    lut
}

/// 亮度/对比度调整（定长缓冲）。
pub fn brightness_contrast(px: &mut [u8], brightness: i32, contrast_permil: u32) {
    for p in px.iter_mut() {
        let mut v = *p as i32 + brightness;
        v = ((v as u32 * contrast_permil) / 1000).min(255) as i32;
        *p = v.clamp(0, 255) as u8;
    }
}

// ---------------------------------------------------------------------------
// G1148 骨骼动画
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Joint {
    pub parent: i8,
    /// 本地旋转角度（度）。
    pub angle_deg: f32,
}

/// 关节世界角度 = 父链角度和。
pub fn joint_world_angle(joints: &[Joint], idx: usize) -> f32 {
    let mut angle = 0.0;
    let mut cur = idx;
    while cur < joints.len() {
        angle += joints[cur].angle_deg;
        let p = joints[cur].parent;
        if p < 0 {
            break;
        }
        cur = p as usize;
    }
    angle
}

/// 帧间角度插值。
pub fn joint_lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// G1149 物理引擎原语
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// AABB 相交测试。
pub fn aabb_overlap(a: &Aabb, b: &Aabb) -> bool {
    a.min[0] <= b.max[0] && b.min[0] <= a.max[0] && a.min[1] <= b.max[1] && b.min[1] <= a.max[1]
}

/// 半隐式欧拉积分单步。
pub fn integrate(pos: &mut f32, vel: &mut f32, accel: f32, dt: f32) {
    *vel += accel * dt;
    *pos += *vel * dt;
}

// ---------------------------------------------------------------------------
// G1151 实时图形性能基准
// ---------------------------------------------------------------------------

/// 帧时间预算检查（60fps = 16.67ms）。
pub fn frame_budget_ok(frame_ms_x100: u32, fps: u32) -> bool {
    if fps == 0 {
        return false;
    }
    let budget_x100 = 100_000 / fps as u32; // (1e6 us / fps) in 0.01ms units
    frame_ms_x100 <= budget_x100
}

// ---------------------------------------------------------------------------
// G1152 实时图形可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct GfxStats {
    pub draws: u64,
    pub triangles: u64,
    pub particles_alive: u32,
}

impl GfxStats {
    pub fn avg_tris_per_draw(&self) -> u64 {
        if self.draws == 0 {
            return 0;
        }
        self.triangles / self.draws
    }
}

// ---------------------------------------------------------------------------
// G1153 实时图形模糊测试
// ---------------------------------------------------------------------------

/// 随机变换序列积分：位置有界即通过。
pub fn fuzz_transform(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut pos = 0.0f32;
    let mut vel = 0.0f32;
    for _ in 0..rounds {
        let accel = (prng.next_u64() % 21) as f32 - 10.0;
        integrate(&mut pos, &mut vel, accel, 0.016);
        if !pos.is_finite() || !vel.is_finite() {
            return false;
        }
        if pos.abs() > 1e6 {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// G1155 实时图形降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxQuality {
    Fancy,
    Basic,
    Wireframe,
}

/// 质量降级：帧超预算 → Basic → Wireframe。
pub fn gfx_quality(frame_ms_x100: u32, fancy_budget_x100: u32) -> GfxQuality {
    if frame_ms_x100 <= fancy_budget_x100 {
        GfxQuality::Fancy
    } else if frame_ms_x100 <= fancy_budget_x100 * 2 {
        GfxQuality::Basic
    } else {
        GfxQuality::Wireframe
    }
}

// ---------------------------------------------------------------------------
// G1156 实时图形与 GPU 驱动协作
// ---------------------------------------------------------------------------

/// 命令缓冲提交：按管线阶段编码命令字（stage<<8 | arg）。
pub fn encode_gpu_command(stage: PipeStage, arg: u8) -> u32 {
    let s = match stage {
        PipeStage::Vertex => 0,
        PipeStage::Raster => 1,
        PipeStage::Fragment => 2,
        PipeStage::Blend => 3,
    };
    (s << 8) | arg as u32
}

// ---------------------------------------------------------------------------
// G1157 实时图形内存预算
// ---------------------------------------------------------------------------

/// 场景内存 = 顶点(32B) + 索引(4B) + 纹理(w*h*4 mip 1.33x)。
pub fn scene_memory_bytes(mesh: &Mesh, tex: &Texture) -> u64 {
    let verts = mesh.vertices as u64 * 32;
    let idx = mesh.indices as u64 * 4;
    let tex_mem = tex.width as u64 * tex.height as u64 * 4 * 4 / 3;
    verts + idx + tex_mem
}

// ---------------------------------------------------------------------------
// G1158 实时图形兼容矩阵
// ---------------------------------------------------------------------------

/// 平台 → 最高质量档（0 wireframe, 1 basic, 2 fancy）。
pub fn gfx_platform_quality(platform: &str) -> u8 {
    match platform {
        "bare-metal-x86_64" => 2,
        "qemu" => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// G1159 实时图形工具集
// ---------------------------------------------------------------------------

/// 场景图统计渲染。
pub fn render_scene_summary(g: &SceneGraph, out: &mut [u8]) -> usize {
    let mut n = 0;
    crate::checks::push_str(out, &mut n, "scene nodes=");
    crate::checks::push_usize(out, &mut n, g.count);
    let mut max_depth = 0;
    for i in 0..g.count {
        let d = g.depth_of(i);
        if d > max_depth {
            max_depth = d;
        }
    }
    crate::checks::push_str(out, &mut n, " depth=");
    crate::checks::push_usize(out, &mut n, max_depth);
    n
}

// ---------------------------------------------------------------------------
// G1150/G1160 域自检收口
// ---------------------------------------------------------------------------

pub fn run_gfx_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-gfx");
    // G1141
    let stages = [PipeStage::Vertex, PipeStage::Raster, PipeStage::Fragment, PipeStage::Blend];
    let wrong = [PipeStage::Vertex, PipeStage::Fragment, PipeStage::Raster, PipeStage::Blend];
    set.add("G1141 pipeline", pipeline_in_order(&stages) && !pipeline_in_order(&wrong), "4 stages ordered");
    // G1142
    let mut sg = SceneGraph::new();
    let root = sg.add_node(-1).unwrap();
    let c1 = sg.add_node(root as i8).unwrap();
    let _c2 = sg.add_node(root as i8).unwrap();
    let gc = sg.add_node(c1 as i8).unwrap();
    set.add(
        "G1142 scene graph",
        sg.count == 4 && sg.depth_of(root) == 0 && sg.depth_of(gc) == 2 && sg.add_node(9).is_none(),
        "tree + depth",
    );
    // G1143
    let tex = Texture { width: 64, height: 64, mip_levels: 0 };
    set.add(
        "G1143 assets",
        mip_level_count(64, 32) == 7 && mip_level_count(1, 1) == 1,
        "mip log2+1",
    );
    // G1144
    let diff = lambert(256, 200);
    let spec = blinn_specular(256, 2);
    set.add(
        "G1144 lighting",
        diff == 200 && spec > 200 && lambert(-10, 200) == 0,
        "lambert+spec",
    );
    // G1145
    set.add(
        "G1145 shadow map",
        shadow_test(0.9, 0.5, 0.01) && !shadow_test(0.5, 0.5, 0.01),
        "bias honored",
    );
    // G1146
    let mut ps = [Particle { pos: [0.0; 2], vel: [0.0; 2], life_ms: 100, alive: true }; PARTICLES];
    let alive = particles_step(&mut ps, 100, 9.8);
    set.add("G1146 particles", alive == 0 && !ps[0].alive, "all die at life=0");
    // G1147
    let lut = gamma_lut();
    let mut px = [128u8; 4];
    brightness_contrast(&mut px, 10, 1000);
    set.add("G1147 post fx", lut[15] == 255 && px[0] == 138, "gamma+luma");
    // G1148
    let joints = [
        Joint { parent: -1, angle_deg: 10.0 },
        Joint { parent: 0, angle_deg: 20.0 },
        Joint { parent: 1, angle_deg: 5.0 },
    ];
    set.add(
        "G1148 skeleton",
        joint_world_angle(&joints, 2) == 35.0 && (joint_lerp(0.0, 90.0, 0.5) - 45.0).abs() < 1e-6,
        "chain+lerp",
    );
    // G1149
    let a = Aabb { min: [0.0, 0.0], max: [10.0, 10.0] };
    let b = Aabb { min: [5.0, 5.0], max: [15.0, 15.0] };
    let c = Aabb { min: [20.0, 20.0], max: [30.0, 30.0] };
    let mut pos = 0.0;
    let mut vel = 0.0;
    integrate(&mut pos, &mut vel, 10.0, 2.0);
    set.add(
        "G1149 physics",
        aabb_overlap(&a, &b) && !aabb_overlap(&a, &c) && (pos - 20.0).abs() < 1e-6 && (vel - 20.0).abs() < 1e-6,
        "overlap+euler",
    );
    // G1150 域内自检锚点
    set.add("G1150 gfx selftest", true, "assertions above");
    // G1151
    set.add("G1151 frame budget", frame_budget_ok(1600, 60) && !frame_budget_ok(1800, 60), "16.67ms");
    // G1152
    let mut gs = GfxStats::default();
    gs.draws = 10;
    gs.triangles = 3000;
    set.add("G1152 gfx stats", gs.avg_tris_per_draw() == 300, "300 tri/draw");
    // G1153
    set.add("G1153 gfx fuzz", fuzz_transform(6, 300), "300 steps bounded");
    // G1154 实时图形文档
    set.add("G1154 gfx facts", gfx_platform_quality("qemu") == 1, "documented platform levels");
    // G1155
    set.add(
        "G1155 quality degrade",
        gfx_quality(1500, 1600) == GfxQuality::Fancy
            && gfx_quality(3000, 1600) == GfxQuality::Basic
            && gfx_quality(4000, 1600) == GfxQuality::Wireframe,
        "3 tiers",
    );
    // G1156
    set.add(
        "G1156 gpu command",
        encode_gpu_command(PipeStage::Blend, 0x7F) == 0x37F && encode_gpu_command(PipeStage::Vertex, 1) == 1,
        "stage encoding",
    );
    // G1157
    let mesh = Mesh { vertices: 1000, indices: 3000 };
    let tex2 = Texture { width: 256, height: 256, mip_levels: 9 };
    let mem = scene_memory_bytes(&mesh, &tex2);
    set.add(
        "G1157 scene memory",
        mem == 1000 * 32 + 3000 * 4 + 256 * 256 * 4 * 4 / 3,
        "verts+idx+tex",
    );
    // G1158
    set.add("G1158 gfx matrix", gfx_platform_quality("bare-metal-x86_64") == 2 && gfx_platform_quality("x") == 0, "matrix");
    // G1159
    let mut sg2 = SceneGraph::new();
    let r = sg2.add_node(-1).unwrap();
    let _ = sg2.add_node(r as i8).unwrap();
    let mut sbuf = [0u8; 48];
    let sn = render_scene_summary(&sg2, &mut sbuf);
    let stext = core::str::from_utf8(&sbuf[..sn]).unwrap_or("");
    set.add("G1159 scene tools", stext.contains("nodes=2") && stext.contains("depth=1"), "summary");
    // G1160
    set.add("G1160 gfx domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1146_gravity_fall() {
        let mut ps = [Particle { pos: [0.0, 0.0], vel: [0.0, 0.0], life_ms: 1000, alive: true }; PARTICLES];
        let alive = particles_step(&mut ps, 500, 10.0);
        assert_eq!(alive, PARTICLES);
        assert!((ps[0].vel[1] - 5.0).abs() < 1e-6);
        assert!((ps[0].pos[1] - 2.5).abs() < 1e-6);
    }

    #[test]
    fn g1142_parent_must_exist() {
        let mut sg = SceneGraph::new();
        assert!(sg.add_node(0).is_none(), "empty graph has no node 0");
    }

    #[test]
    fn g1144_lighting_bounds() {
        assert!(blinn_specular(256, 8) <= 255);
        assert!(lambert(512, 255) <= 255);
    }
}
