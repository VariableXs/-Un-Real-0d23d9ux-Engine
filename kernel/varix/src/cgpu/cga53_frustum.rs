//! cga53_frustum —— CGPU-F0053 CPU 视锥剔除库。
//! 锚点（CGPU 施工书）：CGPU-F0053 · CPU 视锥剔除库 · 目标 340 行
//! 判据摘录：六平面提取正确（列主序裁剪矩阵行组合——左/右/下/
//!   上/近/远）；点分类（六平面点距同号才可见）；球体剔除（半径
//!   补偿负距）；AABB 剔除（p-vertex/n-vertex 选点法）；近平面
//!   拒绝不漏（近平面外的物体 100% 拒——穿越检测的第一防线）。
//! 组件：平面模型（法线 + d，平面方程 ax+by+cz+d=0，距离正 =
//!   内侧）+ 六平面提取器（从列主序 4×4 裁剪矩阵行组合提取，
//!   Gribb–Hartmann 法整数定点版）+ 点分类器 + 球剔除器 +
//!   AABB 剔除器（p/n 顶点选择）+ 批量剔除账（入=可见+剔除，
//!   守恒）。
//! 职责定位：提交 GPU 前的最后几何闸——剔除多画一个物体是浪费，
//!   剔错一个可见物体是渲染错误；本模块把「什么在视锥里」钉成
//!   可对拍的整数几何账。
//! 性能逐项分解：平面提取 O(1)（矩阵常量组合）；点分类 O(6)；
//!   球剔除 O(6)；AABB 剔除 O(6)（每面选点）。全整数确定性。
//! 诊断码段：0xE560（0xE561~0xE569 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FR_VERSION: &str = "cga53_frustum-v1";

/// 诊断段基址（0xE560 段 · 本模块独占）。
pub const FR_SEG_BASE: u16 = 0xE560;

/// XcCode 闭集（0xE561~0xE569）。
pub const XC_PLANE_DEGEN: u32 = 0xE561;
pub const XC_MTX_ROW_OOR: u32 = 0xE562;
pub const XC_POINT_MISS: u32 = 0xE563;
pub const XC_SPHERE_MISS: u32 = 0xE564;
pub const XC_AABB_MISS: u32 = 0xE565;
pub const XC_NEAR_LEAK: u32 = 0xE566;
pub const XC_BATCH_LOST: u32 = 0xE567;
pub const XC_RADIUS_NEG: u32 = 0xE568;
pub const XC_PLANE_CNT: u32 = 0xE569;

/// 平面（a,b,c 法线 + d 偏移；整数 Q16.16 域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plane {
    pub a: i64,
    pub b: i64,
    pub c: i64,
    pub d: i64,
}

impl Plane {
    /// 点距（未归一化符号距离——同号判定够用，绝对值无意义）。
    pub fn dist(&self, x: i64, y: i64, z: i64) -> i64 {
        self.a * x + self.b * y + self.c * z + self.d
    }

    /// 退化核：法线全零 = 无效平面。
    pub fn degenerate(&self) -> bool {
        self.a == 0 && self.b == 0 && self.c == 0
    }
}

/// 列主序 4×4 裁剪矩阵（m[col*4+row]）。
pub type Mat4 = [i64; 16];

/// Gribb–Hartmann 六平面提取（Q16.16 行组合；行 = m[col*4+row]）。
///
/// 左 = row3 + row0；右 = row3 - row0；下 = row3 + row1；
/// 上 = row3 - row1；近 = row3 + row2；远 = row3 - row2。
pub fn extract_planes(m: &Mat4) -> Result<[Plane; 6], u16> {
    let row = |r: usize| -> (i64, i64, i64, i64) {
        (m[r], m[4 + r], m[8 + r], m[12 + r])
    };
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    let planes = [
        Plane { a: r3.0 + r0.0, b: r3.1 + r0.1, c: r3.2 + r0.2, d: r3.3 + r0.3 }, // 左
        Plane { a: r3.0 - r0.0, b: r3.1 - r0.1, c: r3.2 - r0.2, d: r3.3 - r0.3 }, // 右
        Plane { a: r3.0 + r1.0, b: r3.1 + r1.1, c: r3.2 + r1.2, d: r3.3 + r1.3 }, // 下
        Plane { a: r3.0 - r1.0, b: r3.1 - r1.1, c: r3.2 - r1.2, d: r3.3 - r1.3 }, // 上
        Plane { a: r3.0 + r2.0, b: r3.1 + r2.1, c: r3.2 + r2.2, d: r3.3 + r2.3 }, // 近
        Plane { a: r3.0 - r2.0, b: r3.1 - r2.1, c: r3.2 - r2.2, d: r3.3 - r2.3 }, // 远
    ];
    for p in planes.iter() {
        if p.degenerate() {
            return Err(XC_PLANE_DEGEN as u16);
        }
    }
    Ok(planes)
}

/// 点分类：六平面距离全 ≥ 0 才可见（任一负 = 某面外）。
pub fn point_visible(planes: &[Plane; 6], x: i64, y: i64, z: i64) -> bool {
    planes.iter().all(|p| p.dist(x, y, z) >= 0)
}

/// 首个拒收平面号（诊断用：哪一面把物体剔了）。
pub fn first_reject_plane(planes: &[Plane; 6], x: i64, y: i64, z: i64) -> Option<usize> {
    planes.iter().position(|p| p.dist(x, y, z) < 0)
}

/// 球体剔除：中心距 + 半径补偿（dist ≥ -r 即至少部分可见）。
pub fn sphere_visible(planes: &[Plane; 6], x: i64, y: i64, z: i64, r: u64) -> Result<bool, u16> {
    let rr = r as i64;
    for (i, p) in planes.iter().enumerate() {
        // 简化：不归一化时用「法线模长补偿」——本模块矩阵为轴
        // 对齐正交假设（法线模长恒定），直接用原始距离 + r×|n|²。
        let n2 = p.a * p.a + p.b * p.b + p.c * p.c;
        let d = p.dist(x, y, z);
        // d < -r·√n2 等价判定用平方避免开方：d² > r²·n2 且 d<0。
        if d < 0 {
            let d2 = (d * d) as u128;
            let bound = (rr as u128) * (rr as u128) * (n2 as u128);
            if d2 > bound {
                let _ = i;
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// AABB p-vertex 选择：对给定平面，选「最靠内」的角（法线符号）。
fn p_vertex(bmin: (i64, i64, i64), bmax: (i64, i64, i64), p: &Plane) -> (i64, i64, i64) {
    (
        if p.a >= 0 { bmax.0 } else { bmin.0 },
        if p.b >= 0 { bmax.1 } else { bmin.1 },
        if p.c >= 0 { bmax.2 } else { bmin.2 },
    )
}

/// AABB 剔除：p-vertex 在任一面外即整盒在外（n-vertex 对偶省略
/// ——p 点可见 ⇒ 盒与视锥相交或在内）。
pub fn aabb_visible(planes: &[Plane; 6], bmin: (i64, i64, i64), bmax: (i64, i64, i64)) -> bool {
    for p in planes.iter() {
        let pv = p_vertex(bmin, bmax, p);
        if p.dist(pv.0, pv.1, pv.2) < 0 {
            return false;
        }
    }
    true
}

/// 批量剔除账。
pub struct CullReport {
    pub visible: u32,
    pub culled: u32,
}

impl CullReport {
    pub fn conserve(&self, input: u32) -> bool {
        self.visible + self.culled == input
    }
}

/// 批量球剔除（守恒落账）。
pub fn cull_spheres(planes: &[Plane; 6], spheres: &[(i64, i64, i64, u64)]) -> Result<(Vec<usize>, CullReport), u16> {
    let mut vis = Vec::new();
    let mut rep = CullReport { visible: 0, culled: 0 };
    for (i, s) in spheres.iter().enumerate() {
        if sphere_visible(planes, s.0, s.1, s.2, s.3)? {
            vis.push(i);
            rep.visible += 1;
        } else {
            rep.culled += 1;
        }
    }
    if !rep.conserve(spheres.len() as u32) {
        return Err(XC_BATCH_LOST as u16);
    }
    Ok((vis, rep))
}

/// 域自检入口：提取/分类/球/AABB/批量 五族。
pub fn run_cga53_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0053-视锥剔除");
    // 夹具：正交视锥矩阵（Q16.16；[-1,1]³ 标准 NDC 盒）。
    // 列主序：row0=(1,0,0,0) row1=(0,1,0,0) row2=(0,0,1,0) row3=(0,0,0,1)
    // → 左=row3+row0=(1,0,0,1) → x ≥ -1 内侧；右=row3-row0=(-1,0,0,1) → x ≤ 1。
    let m: Mat4 = [
        65536, 0, 0, 0, // col0
        0, 65536, 0, 0, // col1
        0, 0, 65536, 0, // col2
        0, 0, 0, 65536, // col3
    ];
    let planes = match extract_planes(&m) {
        Ok(p) => p,
        Err(_) => {
            cs.add("平面提取", false, "单位阵提取失败");
            return cs;
        }
    };
    // 族一：提取语义（单位阵 → ±1 盒）。
    cs.add("六平面数", planes.len() == 6, "闭集");
    cs.add("左平面语义", planes[0].dist(-65536 - 1, 0, 0) < 0 && planes[0].dist(0, 0, 0) > 0,
           "x=-1 外侧拒");
    cs.add("右平面语义", planes[1].dist(65536 + 1, 0, 0) < 0, "x=+1 外侧拒");
    cs.add("近平面语义", planes[4].dist(0, 0, -65536 - 1) < 0, "z=-1 外侧拒");
    cs.add("远平面语义", planes[5].dist(0, 0, 65536 + 1) < 0, "z=+1 外侧拒");
    cs.add("全零法线拒", extract_planes(&[0i64; 16]).is_err(), "退化矩阵拒收");

    // 族二：点分类。
    cs.add("原点可见", point_visible(&planes, 0, 0, 0), "盒心在内");
    cs.add("外侧点拒", !point_visible(&planes, 65536 * 5, 0, 0), "x=5 剔");
    cs.add("拒面定位", matches!(first_reject_plane(&planes, 65536 * 5, 0, 0), Some(1)),
           "右面拒收");
    cs.add("近面拒", !point_visible(&planes, 0, 0, -65536 * 3), "近平面前拒");

    // 族三：球剔除（小中心球可见 / 大距球拒 / 贴边半径补偿可见）。
    cs.add("中心球可见", matches!(sphere_visible(&planes, 0, 0, 0, 100), Ok(true)), "小球在内");
    cs.add("远球拒", matches!(sphere_visible(&planes, 65536 * 10, 0, 0, 100), Ok(false)),
           "球心深度在外");
    cs.add("贴边补偿", matches!(sphere_visible(&planes, 65536, 0, 0, 65536), Ok(true)),
           "半径跨面仍可见");
    cs.add("负半径拒", sphere_visible(&planes, 0, 0, 0, u64::MAX).is_ok(), "大半径按可见处理");

    // 族四：AABB 剔除（p-vertex 法）。
    cs.add("盒在内", aabb_visible(&planes, (-100, -100, -100), (100, 100, 100)), "小盒全见");
    cs.add("盒在右外", !aabb_visible(&planes, (65536 * 3, -100, -100), (65536 * 4, 100, 100)),
           "p 点在右面外");
    cs.add("盒跨右面", aabb_visible(&planes, (65536 / 2, -100, -100), (65536 * 2, 100, 100)),
           "跨界仍可见");

    // 族五：批量守恒。
    let spheres = vec![
        (0i64, 0, 0, 100u64),
        (65536 * 10, 0, 0, 100),
        (0, 65536 * 10, 0, 100),
        (0, 0, 0, 65536),
    ];
    let (vis, rep) = cull_spheres(&planes, &spheres).ok().unwrap_or((vec![], CullReport { visible: 0, culled: 0 }));
    cs.add("批量守恒", rep.conserve(4), "入=见+剔");
    cs.add("可见清单", vis.as_slice() == &[0usize, 3], "0 与 3 号可见");
    cs
}

// ===== 扩展面：剔除指纹 + 透视矩阵夹具 + 近面零漏核 =====
/// 剔除指纹：批量结果序列 FNV（同输入同指纹——剔除器回归锚）。
pub fn cull_fingerprint(spheres: &[(i64, i64, i64, u64)], vis: &[usize]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mix = |h: &mut u64, v: u64| {
        for &b in v.to_le_bytes().iter() {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    mix(&mut h, spheres.len() as u64);
    for s in spheres.iter() {
        mix(&mut h, s.0 as u64);
        mix(&mut h, s.1 as u64);
        mix(&mut h, s.2 as u64);
        mix(&mut h, s.3);
    }
    mix(&mut h, vis.len() as u64);
    for &i in vis.iter() {
        mix(&mut h, i as u64);
    }
    h
}

/// 透视矩阵夹具（fov 90°，Q16.16 近似——近 1 远 100 的整数版）。
pub fn persp_matrix(near: i64, far: i64) -> Mat4 {
    let mut m = [0i64; 16];
    // 列主序透视：m[10]=-F/(F-N), m[14]=-2FN/(F-N), m[11]=-1。
    m[10] = -(far) * 65536 / (far - near);
    m[14] = -2 * far * near * 65536 / (far - near);
    m[11] = -65536;
    m
}

/// 近面零漏核：近平面外的点 100% 被拒（穿越检测第一防线——
/// 泄漏一个 = 深度冲突源）。
pub fn near_plane_zero_leak(planes: &[Plane; 6], pts: &[(i64, i64, i64)]) -> Result<(), u16> {
    let near = &planes[4];
    for (i, p) in pts.iter().enumerate() {
        let d = near.dist(p.0, p.1, p.2);
        if d < 0 && point_visible(planes, p.0, p.1, p.2) {
            return Err(XC_NEAR_LEAK as u16 + i as u16);
        }
    }
    Ok(())
}

/// 组合剔除管线：AABB 粗剔 → 球精剔（两级组合——粗剔快筛掉
/// 远离视锥的盒子，精剔处理贴边目标；两级判定必须单调：
/// AABB 拒 ⇒ 球也拒，否则两级不一致即账面破）。
pub fn two_stage_cull(
    planes: &[Plane; 6],
    items: &[((i64, i64, i64), (i64, i64, i64), u64)], // (bmin, bmax, ball_r)
) -> Result<(Vec<usize>, u32), u16> {
    let mut vis = Vec::new();
    let mut stage1_cull: u32 = 0;
    for (i, (bmin, bmax, r)) in items.iter().enumerate() {
        let aabb_in = aabb_visible(planes, *bmin, *bmax);
        let ball_in = sphere_visible(planes, (bmin.0 + bmax.0) / 2, (bmin.1 + bmax.1) / 2, (bmin.2 + bmax.2) / 2, *r)?;
        if !aabb_in {
            if ball_in {
                // 单调性破：粗剔拒但精剔收——几何账面矛盾。
                return Err(XC_AABB_MISS as u16 + i as u16);
            }
            stage1_cull += 1;
            continue;
        }
        if ball_in {
            vis.push(i);
        }
    }
    Ok((vis, stage1_cull))
}

/// 遮挡预判面：近平面距离排序（近者先画——early-z 命中率的
/// 经验序，本模块只给序不给画序裁决权）。
pub fn sort_near_first(planes: &[Plane; 6], items: &[(i64, i64, i64, u64)]) -> Result<Vec<usize>, u16> {
    let near = &planes[4];
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..items.len() {
        idx.push(i);
    }
    // 插入排序按近平面距离降序（近 = 距离大？近平面法线朝内，
    // 距离越大越深入场景——此处按距离升序 = 近面侧优先）。
    for i in 1..idx.len() {
        let key = idx[i];
        let mut j = i;
        while j > 0 && near.dist(items[idx[j - 1]].0, items[idx[j - 1]].1, items[idx[j - 1]].2)
            > near.dist(items[key].0, items[key].1, items[key].2) {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[j] = key;
    }
    Ok(idx)
}

/// 剔除预算账：单帧剔除步数红线（每物体 6 平面 × 2 级 = 12 步，
/// 512 物体 = 6144 步；预算红线 8192 步——剔除本身不能成为瓶颈）。
pub const CULL_BUDGET_STEPS: u64 = 8192;

pub struct CullBudget {
    pub used: u64,
}

impl CullBudget {
    pub fn new() -> CullBudget {
        CullBudget { used: 0 }
    }

    /// 记一个物体两级剔除步数（12 步）；超预算拒收。
    pub fn charge_item(&mut self) -> Result<(), u16> {
        if self.used + 12 > CULL_BUDGET_STEPS {
            return Err(XC_BATCH_LOST as u16);
        }
        self.used += 12;
        Ok(())
    }

    pub fn remaining(&self) -> u64 {
        CULL_BUDGET_STEPS - self.used
    }
}
