//! VE-F1626 · 域自检（判据逐条对应，见 `vef29_clipconv.rs` 头注）
//!
//! 锚点判据四条 → 自检项映射：
//! - **NDC 封装** → `C29-NDC-01` ~ `06`（二态封闭 + 仿射向量字面量 +
//!   单点互逆手算 + 深度行端点吸收 + 往返验证 + 声明字面量）
//! - **深度精度** → `C29-精度-01` ~ `04`（近优于远单调 + 建议不强制 +
//!   内容非空 + 默认面字面量）
//! - **裁剪声明** → `C29-裁剪-01` ~ `05`（二态封闭 + 声明行 + 统一语义
//!   字面量 + 合法性双向 + 后端接入体三事实）
//! - **判据** → 版本在案 + 条数离账
//!
//! **判据设计硬规矩**：仿射向量 (20000,−10000)/(5000,5000)/(10000,0)
//! 与深度行 [0,0,10000,−5000]→[0,0,20000,−25000] 判据侧手算写死
//! （不引用被测常量）；0.3→−0.4→0.3 互逆与端点 0/1 双向对拍；建议
//! is_forced 恒 false（「建议而非强制」是域纪律的可断言面）。

use crate::checks::CheckSet;
use crate::svstar2::vef29_clipconv as cc;
use alloc::string::ToString;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死）
// ---------------------------------------------------------------------------

const REF_SCALE: i64 = 10_000;
const REF_AFFINE_Z2G: (i64, i64) = (20_000, -10_000);
const REF_AFFINE_G2Z: (i64, i64) = (5_000, 5_000);
const REF_NOTICE: &str = "后端深度范围差异在投影矩阵深度行吸收——上层语义深度零感知";
const REF_UNIFIED: &str =
    "统一语义：NDC 域外几何不产生可见片段——图元被裁或深度被钳，二者对上层等价";
const REF_NEAR: i64 = 100;
const REF_FAR: i64 = 1_000_000;
const REF_VERSION: &str = "F29-clip-v1";

/// F1626 裁剪空间约定判据（四条映射 17 项）。
pub fn run_vef29_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1626");

    // ================= 一、NDC 封装 =================

    {
        let passed = cc::DepthRange::ZeroToOne.say() == "D3D 系 [0,1]"
            && cc::DepthRange::NegOneToOne.say() == "OpenGL 系 [-1,1]";
        s.add(
            "C29-NDC-01 二态封闭",
            passed,
            if passed { "两约定 say 逐字对拍（表外无处安放）" } else { "深度约定名漂移" },
        );
    }
    {
        let a = cc::depth_affine(cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne);
        let b = cc::depth_affine(cc::DepthRange::NegOneToOne, cc::DepthRange::ZeroToOne);
        let c = cc::depth_affine(cc::DepthRange::ZeroToOne, cc::DepthRange::ZeroToOne);
        let passed = a == REF_AFFINE_Z2G && b == REF_AFFINE_G2Z && c == (REF_SCALE, 0);
        s.add(
            "C29-NDC-02 仿射向量对拍",
            passed,
            if passed { "(20000,−10000)/(5000,5000)/恒等 判据侧写死逐格" } else { "仿射向量漂移" },
        );
    }
    {
        let z = 3000i64;
        let gl = cc::convert_ndc_depth(z, cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne);
        let back = cc::convert_ndc_depth(gl, cc::DepthRange::NegOneToOne, cc::DepthRange::ZeroToOne);
        let end0 = cc::convert_ndc_depth(0, cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne);
        let end1 = cc::convert_ndc_depth(REF_SCALE, cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne);
        let passed = gl == -4000 && back == z && end0 == -REF_SCALE && end1 == REF_SCALE;
        s.add(
            "C29-NDC-03 互逆与端点",
            passed,
            if passed { "0.3→−0.4→0.3 手算互逆；端点 0→−1、1→1 双向" } else { "换算或互逆错位" },
        );
    }
    {
        let row = [0i64, 0, 10_000, -5_000];
        let r = cc::adapt_depth_row(row, cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne);
        let passed = r == [0, 0, 20_000, -25_000];
        s.add(
            "C29-NDC-04 深度行端点吸收",
            passed,
            if passed { "深度行手算 [0,0,20000,−25000]（差异在矩阵行吸收）" } else { "深度行适配不符" },
        );
    }
    {
        let row = [3i64, -7, 12_345, -6_789];
        let passed = cc::depth_row_roundtrip(row, cc::DepthRange::ZeroToOne, cc::DepthRange::NegOneToOne)
            && cc::depth_row_roundtrip(row, cc::DepthRange::NegOneToOne, cc::DepthRange::ZeroToOne);
        s.add(
            "C29-NDC-05 行往返验证",
            passed,
            if passed { "from→to→from 容差 ≤1（万分位 0.0001）双向" } else { "深度行往返不闭合" },
        );
    }
    {
        let passed = cc::PROJECTION_ADAPTS_NOTICE == REF_NOTICE;
        s.add(
            "C29-NDC-06 零感知声明",
            passed,
            if passed { "投影矩阵吸收声明字面量冻结" } else { "声明漂移" },
        );
    }

    // ================= 二、深度精度 =================

    {
        let p = cc::precision_profile(REF_NEAR, REF_FAR, 24);
        let passed = p[0].1 < p[1].1 && p[1].1 < p[2].1;
        s.add(
            "C29-精度-01 非线性单调",
            passed,
            if passed { "近<中<远 分辨率数值单调（近处更精细）" } else { "精度分布不单调（非线性证据失效）" },
        );
    }
    {
        let rz = cc::reversed_z_advice();
        let lg = cc::log_depth_advice();
        let passed = !rz.is_forced && !lg.is_forced;
        s.add(
            "C29-精度-02 建议不强制",
            passed,
            if passed { "两建议 is_forced 恒 false（域纪律可断言面）" } else { "建议越权为强制" },
        );
    }
    {
        let rz = cc::reversed_z_advice();
        let lg = cc::log_depth_advice();
        let passed = rz.name == "反转 Z" && lg.name == "对数深度"
            && !rz.reason.is_empty() && !rz.cost.is_empty()
            && !lg.reason.is_empty() && !lg.cost.is_empty();
        s.add(
            "C29-精度-03 建议内容齐备",
            passed,
            if passed { "名/理由/代价三事实非空（有头绪的建议）" } else { "建议内容缺项" },
        );
    }
    {
        let passed = cc::DEFAULT_NEAR_PER10K == REF_NEAR && cc::DEFAULT_FAR_PER10K == REF_FAR;
        s.add(
            "C29-精度-04 默认面对账",
            passed,
            if passed { "near=100/far=1000000 万分位字面量对拍" } else { "默认面漂移" },
        );
    }

    // ================= 三、裁剪声明 =================

    {
        let passed = !cc::ClipBehavior::PrimitiveClipping.say().is_empty()
            && !cc::ClipBehavior::VertexClamping.say().is_empty()
            && cc::ClipBehavior::PrimitiveClipping.say() != cc::ClipBehavior::VertexClamping.say();
        s.add(
            "C29-裁剪-01 二态封闭",
            passed,
            if passed { "两行为 say 非空且互异（封闭集无第三态）" } else { "裁剪行为名缺或混同" },
        );
    }
    {
        let line = cc::declare_clip_behavior("D3D12", cc::ClipBehavior::PrimitiveClipping);
        let passed = line.contains("D3D12") && line.contains("图元裁剪");
        s.add(
            "C29-裁剪-02 声明行含事实",
            passed,
            if passed { "声明行含后端名与行为事实（可播报）" } else { "声明行缺关键事实" },
        );
    }
    {
        let passed = cc::UNIFIED_CLIP_SEMANTICS == REF_UNIFIED;
        s.add(
            "C29-裁剪-03 统一语义字面量",
            passed,
            if passed { "跨后端统一语义逐字冻结（上层唯一依赖）" } else { "统一语义漂移" },
        );
    }
    {
        let ok = cc::clip_declare_legal("GL", Some(cc::ClipBehavior::VertexClamping));
        let bad_name = cc::clip_declare_legal("", Some(cc::ClipBehavior::PrimitiveClipping));
        let bad_none = cc::clip_declare_legal("GL", None);
        let passed = ok && !bad_name && !bad_none;
        s.add(
            "C29-裁剪-04 合法性双向",
            passed,
            if passed { "正常声明收、空名/缺行为双向拒" } else { "声明校验放水" },
        );
    }
    {
        let d3d = cc::NdcBackend {
            name: "D3D12".to_string(),
            range: cc::DepthRange::ZeroToOne,
            behavior: cc::ClipBehavior::PrimitiveClipping,
        };
        let gl = cc::NdcBackend {
            name: "OpenGL".to_string(),
            range: cc::DepthRange::NegOneToOne,
            behavior: cc::ClipBehavior::VertexClamping,
        };
        let line = d3d.declare();
        let z = d3d.convert_to(&gl, 3000);
        let back = gl.convert_to(&d3d, z);
        let passed = d3d.legal() && gl.legal()
            && line.contains("D3D12") && line.contains("[0,1]")
            && z == -4000 && back == 3000;
        s.add(
            "C29-裁剪-05 后端接入体",
            passed,
            if passed { "三件齐备声明+convert_to 互逆（粘合面可用）" } else { "后端接入体不符" },
        );
    }

    // ================= 四、判据 =================

    {
        let passed = cc::CLIP_CONV_VERSION == REF_VERSION;
        s.add(
            "C29-判据-版本在案",
            passed,
            if passed { "CLIP_CONV_VERSION=F29-clip-v1 溯源键稳定" } else { "版本键漂移" },
        );
    }
    {
        let passed = s.len() == 16;
        s.add(
            "C29-判据-条数对账",
            passed,
            if passed { "判据 17 项离账：对账点前 16 项与设计清单一一对应" } else { "判据条数与设计不符（漏项/多项）" },
        );
    }

    s
}
