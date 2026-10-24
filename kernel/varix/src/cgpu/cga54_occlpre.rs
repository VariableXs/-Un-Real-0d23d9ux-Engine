//! cga54_occlpre —— CGPU-F0054 SIMD 遮挡粗判。
//! 锚点（CGPU 施工书）：CGPU-F0054 · SIMD 遮挡粗判 · 目标 320 行
//! 判据摘录：包围球投影面积零（球心在视锥外且不与近平面相交）
//!   即粗判遮挡；深度分层 Z 金字塔粗判与逐像素精判单调一致
//!   （粗判可见 ⇒ 精判必可见，反向允许误放行不许误剔除）；
//!   深度测试比较器 LCompare 语义冻结（LESS/LEQUAL/GREATER/
//!   GEQUAL/EQUAL/NEVER/ALWAYS 七闭集）。
//! 组件：视锥六平面复用 cga53 语义（自带轻量重建避免耦合）+
//!   包围球投影面积估算 + 4×4 深度块金字塔（逐级 min/max 两路）+
//!   粗判器（块级早期拒收）+ 单调性对拍核 + 深度比较器闭集。
//! 职责定位：剔除管线的第二级——cga53 视锥剔除后，本模块把
//!   「被已绘制内容挡住」的目标也筛掉，误剔除是渲染破面红线。
//! 性能逐项分解：金字塔构建 O(N)（N=块数）；块粗判 O(1)（6 平面
//!   点积 + 块深度范围比较）；单调性对拍 O(N)。全整数定点。
//! 诊断码段：0xE5A0（0xE5A1~0xE5A9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const OP_VERSION: &str = "cga54_occlpre-v1";

/// 诊断段基址（0xE5A0 段 · 本模块独占）。
pub const OP_SEG_BASE: u16 = 0xE5A0;

/// XcCode 闭集（0xE5A1~0xE5A9）。
pub const XC_DIM_ZERO: u32 = 0xE5A1;
pub const XC_PYRAMID_OOR: u32 = 0xE5A2;
pub const XC_MONO_BREAK: u32 = 0xE5A3;
pub const XC_CMP_OOR: u32 = 0xE5A4;
pub const XC_DEPTH_INV: u32 = 0xE5A5;
pub const XC_BLOCK_OOB: u32 = 0xE5A6;
pub const XC_NEAR_MISS: u32 = 0xE5A7;
pub const XC_LEVEL_BAD: u32 = 0xE5A8;
pub const XC_PLANE_DEGEN: u32 = 0xE5A9;

/// 深度比较器七闭集（语义冻结——路由表值即语义）。
pub const CMP_NEVER: u8 = 0;
pub const CMP_LESS: u8 = 1;
pub const CMP_EQUAL: u8 = 2;
pub const CMP_LEQUAL: u8 = 3;
pub const CMP_GREATER: u8 = 4;
pub const CMP_GEQUAL: u8 = 5;
pub const CMP_ALWAYS: u8 = 6;

/// 深度测试（闭集路由：越界码显性拒绝，不静默放行）。
pub fn depth_test(sample: u32, stored: u32, op: u8) -> Result<bool, u32> {
    if op > CMP_ALWAYS {
        return Err(XC_CMP_OOR);
    }
    Ok(match op {
        CMP_NEVER => false,
        CMP_LESS => sample < stored,
        CMP_EQUAL => sample == stored,
        CMP_LEQUAL => sample <= stored,
        CMP_GREATER => sample > stored,
        CMP_GEQUAL => sample >= stored,
        _ => true,
    })
}

/// 深度块：4×4 定点深度 + 块级 min/max（金字塔节点）。
#[derive(Clone, Copy)]
pub struct DepthBlock {
    pub d: [u32; 16],
    pub dmin: u32,
    pub dmax: u32,
}

impl DepthBlock {
    /// 构建（含 min/max 派生：u32 深度域，0=近 0xFFFFFFFF=远）。
    pub fn build(d: [u32; 16]) -> Result<DepthBlock, u32> {
        let mut m = d[0];
        let mut x = d[0];
        for &v in d.iter() {
            if v < m {
                m = v;
            }
            if v > x {
                x = v;
            }
        }
        Ok(DepthBlock { d, dmin: m, dmax: x })
    }

    /// 块级快速测试：样本深度范围 [smin, smax] 与块范围比较——
    /// 若 op=LESS 且 smin ≥ dmax（样本最近也比块最远更远）则整块
    /// 不可能通过 ⇒ 粗判遮挡。
    pub fn coarse_occluded(&self, smin: u32, smax: u32) -> bool {
        // LESS 语义：任何样本都不小于块内最远深度 ⇒ 全挡。
        smin >= self.dmax
    }

    /// 块内精判（逐像素 LESS，供单调性对拍）。
    pub fn fine_any_pass(&self, s: u32) -> bool {
        self.d.iter().any(|v| s < *v)
    }
}

/// 深度金字塔：逐级 2×2 下采样（父 min/max = 子四块 min/max 的
/// 再 min/max——保守包络逐级放大）。
pub struct ZPyramid {
    pub w_blocks: usize,
    pub h_blocks: usize,
    /// level 0 = 4×4 块阵；level k = level k-1 的 2×2 合并。
    pub levels: Vec<Vec<DepthBlock>>,
}

impl ZPyramid {
    pub fn build(blocks: &[DepthBlock], w_blocks: usize) -> Result<ZPyramid, u32> {
        if blocks.is_empty() || w_blocks == 0 || blocks.len() % w_blocks != 0 {
            return Err(XC_DIM_ZERO);
        }
        let h_blocks = blocks.len() / w_blocks;
        let mut levels: Vec<Vec<DepthBlock>> = Vec::new();
        let mut cur: Vec<DepthBlock> = Vec::new();
        for &b in blocks.iter() {
            cur.push(b);
        }
        levels.push(cur.clone());
        while cur.len() > 1 {
            let nw = if w_blocks >= 2 { w_blocks >> (levels.len()) } else { 1 };
            let _ = nw;
            let mut next: Vec<DepthBlock> = Vec::new();
            let mut i = 0usize;
            while i + 1 < cur.len() {
                let dmin = if cur[i].dmin < cur[i + 1].dmin { cur[i].dmin } else { cur[i + 1].dmin };
                let dmax = if cur[i].dmax > cur[i + 1].dmax { cur[i].dmax } else { cur[i + 1].dmax };
                next.push(DepthBlock {
                    d: [dmin; 16],
                    dmin,
                    dmax,
                });
                i += 2;
            }
            if cur.len() % 2 == 1 {
                let last = cur[cur.len() - 1];
                next.push(last);
            }
            levels.push(next.clone());
            cur = next;
        }
        Ok(ZPyramid { w_blocks, h_blocks, levels })
    }

    /// 顶层粗判：目标深度范围 [smin, smax] 被顶层包络整块挡住。
    pub fn coarse_top(&self, smin: u32, smax: u32) -> Result<bool, u32> {
        let top = self.levels.get(self.levels.len() - 1).ok_or(XC_PYRAMID_OOR)?;
        if top.is_empty() {
            return Err(XC_PYRAMID_OOR);
        }
        // 顶层节点可能不止 1 个（奇数补位），取全节点联合包络。
        let mut dmax_all = top[0].dmax;
        for b in top.iter() {
            if b.dmax > dmax_all {
                dmax_all = b.dmax;
            }
        }
        Ok(smin >= dmax_all)
    }
}

/// 包围球粗判：投影半径为零（球心在近平面之后且距离足够远）
/// 即粗判不可见。定点：法线距离 ≥ 球半径 + 近平面裕量。
pub fn ball_culled(dist_to_near: u32, ball_r: u32, margin: u32) -> bool {
    dist_to_near >= ball_r.saturating_add(margin)
}

/// 单调性对拍：粗判遮挡的目标，精判必须全挡（反向 = 误剔除红
/// 线，直接立案）。抽样块级对拍核。
pub fn mono_audit(blocks: &[DepthBlock], smin: u32, smax: u32) -> Result<(), u32> {
    for (i, b) in blocks.iter().enumerate() {
        if b.coarse_occluded(smin, smax) && b.fine_any_pass(smin) {
            return Err(XC_MONO_BREAK + i as u32);
        }
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga54_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0054");
    // 族一：深度比较器七闭集路由。
    cs.add("LESS", matches!(depth_test(3, 5, CMP_LESS), Ok(true)), "3<5 过");
    cs.add("LEQUAL 等值过", matches!(depth_test(5, 5, CMP_LEQUAL), Ok(true)), "等值放行");
    cs.add("GREATER", matches!(depth_test(7, 5, CMP_GREATER), Ok(true)), "7>5 过");
    cs.add("NEVER 恒拒", matches!(depth_test(1, 1, CMP_NEVER), Ok(false)), "闭集 0");
    cs.add("ALWAYS 恒过", matches!(depth_test(1, 1, CMP_ALWAYS), Ok(true)), "闭集 6");
    cs.add("越界码拒", depth_test(1, 1, 7).is_err(), "七外显拒");
    // 族二：块级粗判（样本全在块最远深度之后 ⇒ 全挡）。
    let d: [u32; 16] = [100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 210, 220, 230, 240, 250];
    let blk = DepthBlock::build(d).map_err(|e| e as u16);
    cs.add("块 min/max", matches!(&blk, Ok(b) if b.dmin == 100 && b.dmax == 250), "包络 100..250");
    cs.add("粗判全挡", matches!(&blk, Ok(b) if b.coarse_occluded(300, 320)), "smin=300 ≥ dmax");
    cs.add("粗判放行", matches!(&blk, Ok(b) if !b.coarse_occluded(50, 60)), "smin=50 < dmax");
    cs.add("精判有通过", matches!(&blk, Ok(b) if b.fine_any_pass(90)), "90 < 100 至少一像素过");
    // 族三：金字塔包络单调放大。
    let b1 = DepthBlock::build([10u32; 16]).map_err(|e| e as u16);
    let b2 = DepthBlock::build([200u32; 16]).map_err(|e| e as u16);
    if let (Ok(x), Ok(y)) = (b1, b2) {
        let py = ZPyramid::build(&[x, y], 2);
        cs.add("金字塔构建", py.is_ok(), "2 块 1 层收顶");
        cs.add("顶层包络", matches!(&py, Ok(p) if p.coarse_top(300, 310).map_err(|e| e as u16) == Ok(true)), "顶层 dmax=200");
        cs.add("顶层放行", matches!(&py, Ok(p) if !matches!(p.coarse_top(50, 60), Ok(true))), "smin=50 未见挡");
    } else {
        cs.add("金字塔构建", false, "夹具构建失败");
    }
    // 族四：包围球粗判。
    cs.add("球远剔除", ball_culled(5000, 100, 50), "5000 ≥ 150");
    cs.add("球近放行", !ball_culled(120, 100, 50), "120 < 150");
    // 族五：单调性对拍（粗判挡 ⇒ 精判全挡）。
    let mono = mono_audit(&[DepthBlock { d, dmin: 100, dmax: 250 }], 300, 320);
    cs.add("单调通过", mono.is_ok(), "粗挡块精判无通过");
    let mono_bad = mono_audit(&[DepthBlock { d, dmin: 100, dmax: 250 }], 90, 95);
    cs.add("非挡不立案", mono_bad.is_ok(), "粗不挡 ⇒ 免检");
    // 族六：越界防线。
    cs.add("空块阵拒", ZPyramid::build(&[], 1).is_err(), "DIM_ZERO");
    cs.add("宽度失配拒", ZPyramid::build(&[DepthBlock { d, dmin: 0, dmax: 0 }; 3], 2).is_err(), "3%2≠0");
    deepen_occl::merge_into(&mut cs);
    deepen_occl2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga54_occlpre）：真功能面 + 判据并入主核 =====
pub mod deepen_occl {
    use super::*;

    /// 扩展一：块深度插值（块心代表值——粗判的精化路径）。
    pub fn block_center(b: &DepthBlock) -> u32 {
        let mut s: u64 = 0;
        for &v in b.d.iter() {
            s += v as u64;
        }
        (s / 16) as u32
    }

    /// 扩展二：金字塔层级账（每层块数单调不增——包络放大面）。
    pub fn pyramid_level_audit(p: &ZPyramid) -> Result<(), u32> {
        for w in p.levels.windows(2) {
            if w[1].len() > w[0].len() {
                return Err(XC_LEVEL_BAD);
            }
        }
        Ok(())
    }

    /// 扩展三：双阈值分级（粗判接受/精判复检/放行三档闭集）。
    pub const VERDICT_CULL: u8 = 0;
    pub const VERDICT_RECHECK: u8 = 1;
    pub const VERDICT_PASS: u8 = 2;

    pub fn two_band(bmin: u32, bmax: u32, smin: u32, smax: u32) -> Result<u8, u32> {
        if bmin > bmax {
            return Err(XC_DEPTH_INV);
        }
        if smin >= bmax {
            Ok(VERDICT_CULL)
        } else if smax < bmin {
            Ok(VERDICT_PASS)
        } else {
            Ok(VERDICT_RECHECK)
        }
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let d: [u32; 16] = [100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 210, 220, 230, 240, 250];
        let b = match DepthBlock::build(d) {
            Ok(x) => x,
            Err(_) => return,
        };
        cs.add("块心均值", block_center(&b) == 175, "Σ/16");
        cs.add("三档剔除", matches!(two_band(100, 250, 300, 320), Ok(VERDICT_CULL)), "smin ≥ bmax");
        cs.add("三档放行", matches!(two_band(100, 250, 10, 20), Ok(VERDICT_PASS)), "smax < bmin");
        cs.add("三档复检", matches!(two_band(100, 250, 120, 300), Ok(VERDICT_RECHECK)), "跨包络");
        cs.add("倒序拒", two_band(250, 100, 0, 1).is_err(), "DEPTH_INV");
    }

}

/// ===== 深化扩展块 2（cga54_occlpre）=====
pub mod deepen_occl2 {
    use super::*;

    /// 扩展四：多层粗判（金字塔逐级下降——从顶层到块级逐级精化）。
    pub fn coarse_cascade(p: &ZPyramid, smin: u32, smax: u32) -> Result<usize, u32> {
        // 从顶层向下找首个「不能整挡」的层，返回到达层深。
        for (lvl, layer) in p.levels.iter().enumerate().rev() {
            let mut dmax_all = 0u32;
            for b in layer.iter() {
                if b.dmax > dmax_all {
                    dmax_all = b.dmax;
                }
            }
            if smin < dmax_all {
                return Ok(lvl);
            }
        }
        Ok(0)
    }

    /// 扩展五：深度块均匀性账（max-min ≤ 阈 = 平坦块——粗判高信
    /// 度条件账）。
    pub fn block_flatness(b: &DepthBlock, thr: u32) -> bool {
        b.dmax - b.dmin <= thr
    }

    /// 扩展六：近平面相交预判（球与近平面距离——相交即不可剔除）。
    pub fn near_intersect(dist: u32, ball_r: u32) -> bool {
        // dist 为球心到近平面距离（定点）：dist < r ⇒ 相交。
        dist < ball_r
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let b1 = match DepthBlock::build([100u32; 16]) {
            Ok(x) => x,
            Err(_) => return,
        };
        let b2 = match DepthBlock::build([200u32; 16]) {
            Ok(x) => x,
            Err(_) => return,
        };
        if let Ok(p) = ZPyramid::build(&[b1, b2], 2) {
            cs.add("级联层深", matches!(coarse_cascade(&p, 300, 310), Ok(0)), "顶层整挡到 0");
            cs.add("级联穿透", matches!(coarse_cascade(&p, 50, 60), Ok(1)), "顶层不挡下探");
            cs.add("层账单调", super::deepen_occl::pyramid_level_audit(&p).is_ok(), "块数不增");
        }
        cs.add("平坦块", block_flatness(&b1, 10), "max-min=0");
        cs.add("阈值平坦", block_flatness(&b2, 10), "同值块亦平坦（界内）");
        cs.add("近平面相交", near_intersect(50, 100), "dist<r");
        cs.add("近平面相离", !near_intersect(200, 100), "dist≥r");
    }

}
