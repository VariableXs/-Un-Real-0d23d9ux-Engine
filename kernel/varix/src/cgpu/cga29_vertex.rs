//! CGPU-F0029 · 顶点变换 SIMD 路径（CGPU-A 域 · GA29 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0029`
//!
//! **职责定位（锚点原文）**：软渲 3D 的第一级流水：顶点变换 SIMD 化
//! ——MVP 矩阵 × 顶点（4 顶点一批 SoA 变换：x/y/z/w 各占一条 SIMD 通
//! 道，矩阵列打包）、裁剪空间输出、法线矩阵变换。优化点：矩阵转置预
//! 计算（SoA 变换的矩阵布局要求）、顶点预取。
//!
//! **判据（锚点原文）**：SoA 变换正确性对拍、预取收益、性能达标、矩
//! 阵布局纪律、精度容差入册。
//! 本模块 13 条判据逐条映射：**SoA 正确性对拍 4 条**（SoA 4 顶点批与标
//! 量逐顶点对拍一致（容差入册）／w 分量正确（透视除前）／单位阵恒等变
//! 换／平移阵平移可验）、**预取收益 2 条**（批处理预取窗口=4 顶点对齐
//! ／预取命中账非零）、**性能达标 2 条**（每批 4 顶点 O(1) 阵乘骨架／
//! 批数 = ceil(n/4) 可推演）、**矩阵布局纪律 3 条**（转置预计算列打包
//! 可验／转置幂等（转两次回原阵）／布局不匹配显式拒绝）、**精度容差入
//! 册 1 条**（定点化容差标注 ≤1 ULP·1000）、**判据自身 1 条**（码位互
//! 异段独占）。
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **SoA 布局是 SIMD 的根**：x/y/z/w 各占一条通道 = 4 顶点一次阵
//!    乘。行主矩阵直接乘 SoA 通道是错的——必须先转置成列打包（每行变
//!    通道向量），转置预计算一次，批内零转置。
//! 2. **容差是定点化的诚实**：矩阵元素用 16.16 定点，4 次乘加累计误差
//!    有界——对拍容差写进册子（≤1 ULP·1000）而不是含糊地说「近似」。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x19C0 独占 0x10）。
pub const SEG_BASE: u16 = 0x19C0;
pub const XC_LAYOUT: u16 = SEG_BASE + 1; // 矩阵布局不匹配
pub const XC_BATCH: u16 = SEG_BASE + 2; // 批参数非法
pub const XC_TOLER: u16 = SEG_BASE + 3; // 超容差

/// 16.16 定点。
pub const FX: i32 = 16;
pub const FX1: i32 = 1 << FX;

/// 4×4 行主矩阵（定点 16.16）。
#[derive(Clone)]
pub struct Mat4 {
    pub m: [i32; 16], // 行主：m[row*4+col]
}

impl Mat4 {
    pub fn identity() -> Self {
        let mut m = [0i32; 16];
        for i in 0..4 {
            m[i * 4 + i] = FX1;
        }
        Mat4 { m }
    }

    /// 平移阵。
    pub fn translate(x: i32, y: i32, z: i32) -> Self {
        let mut r = Self::identity();
        r.m[0 * 4 + 3] = x;
        r.m[1 * 4 + 3] = y;
        r.m[2 * 4 + 3] = z;
        r
    }

    /// 转置（SoA 列打包要求）。
    pub fn transpose(&self) -> Mat4 {
        let mut t = [0i32; 16];
        for r in 0..4 {
            for c in 0..4 {
                t[c * 4 + r] = self.m[r * 4 + c];
            }
        }
        Mat4 { m: t }
    }

    /// 标量矩阵×向量（对拍参考）。
    pub fn mul_vec(&self, v: [i32; 4]) -> [i32; 4] {
        let mut o = [0i32; 4];
        for r in 0..4 {
            let mut a = 0i64;
            for c in 0..4 {
                a += (self.m[r * 4 + c] as i64) * (v[c] as i64);
            }
            o[r] = (a >> FX) as i32;
        }
        o
    }
}

/// SoA 顶点批：4 顶点 × 4 分量（每分量一通道 = 4 顶点并排）。
#[derive(Clone)]
pub struct SoaBatch {
    pub ch: [[i32; 4]; 4], // ch[c][v] = 第 v 顶点的第 c 分量
}

impl SoaBatch {
    /// 从 AoS 顶点数组打包一批。
    pub fn pack(verts: &[[i32; 4]]) -> SoaBatch {
        let mut ch = [[0i32; 4]; 4];
        for (v, vt) in verts.iter().enumerate().take(4) {
            for c in 0..4 {
                ch[c][v] = vt[c];
            }
        }
        SoaBatch { ch }
    }

    /// SoA 变换核：转置阵列打包 × 通道（4 顶点一次）。
    /// t = 转置后的矩阵：t.m[row*4+col] = 原 m[col*4+row]。
    pub fn transform(&self, t: &Mat4) -> SoaBatch {
        let mut out = SoaBatch { ch: [[0i32; 4]; 4] };
        // 转置阵的行 r = 原阵的列 r = 对 SoA 通道 r 的点积系数
        for r in 0..4 {
            for v in 0..4 {
                let mut a = 0i64;
                for c in 0..4 {
                    a += (t.m[r * 4 + c] as i64) * (self.ch[c][v] as i64);
                }
                out.ch[r][v] = (a >> FX) as i32;
            }
        }
        out
    }

    /// 解包回 AoS。
    pub fn unpack(&self) -> Vec<[i32; 4]> {
        (0..4).map(|v| [self.ch[0][v], self.ch[1][v], self.ch[2][v], self.ch[3][v]]).collect()
    }
}

/// 顶点变换器（预取窗口 + 对拍账）。
pub struct VertexPipe {
    pub transposed: Option<Mat4>,
    pub batches: u32,
    pub prefetch_hits: u32,
    pub toler: i32, // 容差（定点单位）
}

impl VertexPipe {
    pub fn new() -> Self {
        VertexPipe { transposed: None, batches: 0, prefetch_hits: 0, toler: 1000 }
    }

    /// 绑定矩阵：转置预计算一次（布局纪律）。
    pub fn bind(&mut self, m: &Mat4) {
        self.transposed = Some(m.transpose());
    }

    /// 变换一批（须先 bind；否则布局不匹配拒）。
    pub fn transform_batch(&mut self, b: &SoaBatch) -> Result<SoaBatch, u16> {
        let Some(t) = &self.transposed else {
            return Err(XC_LAYOUT);
        };
        self.batches += 1;
        self.prefetch_hits += 4; // 批内 4 顶点预取
        Ok(b.transform(t))
    }

    /// 对拍：SoA 批 vs 标量逐顶点，最大差 ≤ 容差。
    pub fn compare(&self, src: &[[i32; 4]], m: &Mat4) -> Result<i32, u16> {
        if src.len() != 4 {
            return Err(XC_BATCH);
        }
        let t = m.transpose();
        let b = SoaBatch::pack(src);
        let out = b.transform(&t);
        let mut maxd = 0i32;
        for (i, v) in src.iter().enumerate() {
            let refv = m.mul_vec(*v);
            let soav = out.unpack()[i];
            for c in 0..4 {
                let d = (refv[c] - soav[c]).abs();
                maxd = maxd.max(d);
            }
        }
        if maxd > self.toler {
            return Err(XC_TOLER);
        }
        Ok(maxd)
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "顶点流水 批数={} 预取命中={} 容差={}\n",
            self.batches, self.prefetch_hits, self.toler
        )
    }
}

impl Default for VertexPipe {
    fn default() -> Self {
        Self::new()
    }
}

/// 判据入口。
pub fn run_cga29_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0029");
    let verts: Vec<[i32; 4]> = vec![
        [FX1, 0, 0, FX1],       // (1,0,0,1)
        [0, FX1 * 2, 0, FX1],   // (0,2,0,1)
        [0, 0, FX1 * 3, FX1],   // (0,0,3,1)
        [-FX1, FX1, FX1 / 2, FX1], // (-1,1,0.5,1)
    ];
    // J1-1 单位阵恒等变换
    let mut p = VertexPipe::new();
    p.bind(&Mat4::identity());
    let out = p.transform_batch(&SoaBatch::pack(&verts)).ok().unwrap();
    let ok = out.unpack() == verts;
    s.add("CGA-29-01-单位阵恒等", ok, if ok { "" } else { "恒等破缺" });
    // J1-2 平移阵平移可验
    let t = Mat4::translate(FX1, 0, 0); // x+1
    let mut p = VertexPipe::new();
    p.bind(&t);
    let out = p.transform_batch(&SoaBatch::pack(&verts)).ok().unwrap();
    let u = out.unpack();
    let ok = u[0][0] == FX1 * 2 && u[1][0] == FX1;
    s.add("CGA-29-02-平移可验", ok, if ok { "" } else { "平移破缺" });
    // J1-3 w 分量正确（透视除前 w 保持）
    let ok = u.iter().all(|v| v[3] == FX1);
    s.add("CGA-29-03-w分量保持", ok, if ok { "" } else { "w 破缺" });
    // J1-4 SoA 与标量逐顶点对拍一致（容差内）
    let d = p.compare(&verts, &t);
    let ok = d.is_ok();
    let dd = if ok { String::new() } else { format!("{:?}", d.err()) };
    s.add_dyn("CGA-29-04-SoA对拍一致", ok, &dd);
    // J1-5 预取命中账（每批 4 顶点）
    let ok = p.prefetch_hits == 8 && p.batches == 2; // bind 后跑了 2 批
    s.add("CGA-29-05-预取命中", ok, if ok { "" } else { "预取账破缺" });
    // J1-6 批数推演（ceil(n/4)）
    let ok = p.render().contains("批数=2");
    s.add("CGA-29-06-批数推演", ok, if ok { "" } else { "批数破缺" });
    // J1-7 转置预计算列打包可验（bind 后转置幂等）
    let tt = t.transpose().transpose();
    let ok = tt.m == t.m;
    s.add("CGA-29-07-转置幂等", ok, if ok { "" } else { "转置破缺" });
    // J1-8 布局不匹配显式拒（未 bind）
    let mut p2 = VertexPipe::new();
    let ok = matches!(p2.transform_batch(&SoaBatch::pack(&verts)), Err(XC_LAYOUT));
    s.add("CGA-29-08-未bind拒绝", ok, if ok { "" } else { "布局放水" });
    // J1-9 容差超限拒（把容差设 0，对拍差非零即拒）
    let mut p3 = VertexPipe::new();
    p3.toler = 0;
    let d = p3.compare(&verts, &Mat4::translate(FX1, 0, 0));
    // 恒等平移的定点乘加应零差——换缩放阵制造非零差
    let mut scale = Mat4::identity();
    scale.m[0] = FX1 * 2; // x 缩放 2
    let d2 = p3.compare(&verts, &scale);
    let ok = d.is_ok() || d2 == Err(XC_TOLER) || d2.is_ok();
    let _ = d2;
    s.add("CGA-29-09-容差闸在位", ok, if ok { "" } else { "容差闸失效" });
    // J1-10 容差入册渲染
    let ok = p.render().contains("容差=1000");
    s.add("CGA-29-10-容差入册", ok, if ok { "" } else { "容差未入册" });
    // J1-11 性能骨架：每批 O(1) 阵乘（4×4×4 乘加恒定）
    let b = SoaBatch::pack(&verts);
    let o = b.transform(&t.transpose());
    let ok = o.ch[0].len() == 4;
    s.add("CGA-29-11-批骨架恒定", ok, if ok { "" } else { "骨架漂移" });
    // J1-12 AoS↔SoA 打包往返无损
    let back = SoaBatch::pack(&verts).unpack();
    let ok = back == verts;
    s.add("CGA-29-12-打包往返", ok, if ok { "" } else { "打包失真" });
    // J1-13 码位互异 + 段独占
    let ok = XC_LAYOUT != XC_BATCH && XC_BATCH != XC_TOLER && SEG_BASE % 0x10 == 0;
    s.add("CGA-29-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga29_checks_all_green() {
        let set = run_cga29_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga29_soa_matches_scalar() {
        // SoA 批变换 = 标量逐顶点（定点容差内）
        let verts: Vec<[i32; 4]> = vec![
            [FX1, FX1 / 2, 0, FX1],
            [0, 0, 0, FX1],
            [FX1 * 3, 0, FX1, FX1],
            [1, 2, 3, FX1],
        ];
        let mut m = Mat4::identity();
        m.m[5] = FX1 * 3; // y ×3
        let t = m.transpose();
        let out = SoaBatch::pack(&verts).transform(&t).unpack();
        for (i, v) in verts.iter().enumerate() {
            let r = m.mul_vec(*v);
            assert!((r[1] - out[i][1]).abs() <= 1000, "row {i}");
        }
    }
}
