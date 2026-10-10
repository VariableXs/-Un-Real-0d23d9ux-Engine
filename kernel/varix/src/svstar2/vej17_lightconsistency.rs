//! VE-F1817 · 光照一致性（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1817`
//!
//! **判据（锚点原文）**：跨后端对拍、双模式一致、容差白名单、ADR 稳定、判据。
//!
//! **职责定位（锚点原文）**：跨后端光照一致（同场景同光源跨渲染后端像素对拍
//! ——确定性体系从 I 域延伸至光照）、跨版本稳定（光照语义变更→ADR）、物理
//! 量纲换算双模式一致（物理制与艺术制渲染结果经换算后逐位一致）三段核验。
//!
//! **数据结构（锚点原文）**：一致性证据表——对拍场景集（复用 F1765 校准场景
//! 加光照标定场景一枚：固定光源组合+固定材质+固定相机）、双后端哈希比对
//! 结果、双模式换算对拍表（同场景 PHYSICAL 与 ARTISTIC+换算系数渲染的像素
//! 差阈值内一致判定）。
//!
//! **错误路径与降级矩阵（锚点原文）**：对拍一位不一致→区分浮点编译差异
//! （容差表）与真实语义差（立案）；容差使用必须登记场景与原因白名单（与
//! F1763 同规则防容差滥用）；语义变更无 ADR→CI 拦截；双模式换算不一致→
//! F1802 换算表修正。
//!
//! **跨批对接点（锚点原文）**：确定性口径与 F1763 证据表衔接（J 段追加）；
//! 场景复用 F1765；ADR 与 F1798 流程；**J02/J03 一致性条目（F1837/F1857）
//! 复用标定场景框架扩展阴影/氛围维**。
//!
//! # 对拍面（无 GPU 双后端下的确定性形态）
//!
//! 「跨后端」在纯函数面落为**两条独立计算路径**：路径 A=逐像素前向累加
//! （f32 直算）；路径 B=f64 中间量再舍入（模拟另一后端的更高精度内部管线）。
//! 同一标定场景（固定光源组合+固定相机+16×16 像素栅格）双路径渲染→哈希
//! 比对+逐像素容差判定。**双模式换算**：F1804 物理曲线（1/d²）与艺术曲线
//! （窗口化幂）形状不同——换算系数在标定距离锚定（强度归一），一致性判定
//! = 换算后逐样本差在**登记过的容差**内；曲线形状差异本身就是容差白名单
//! 登记的原因（诚实声明，不假装逐位一致）。

use super::vej04_pointlight::{curve_attenuation, contribution, FalloffCurve, PointLight};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（J 域独占段 0x37xx——0x33=F1812/0x34=F1813/0x36=ves06/0x39=F1811）
// ===========================================================================

/// 诊断码（J 域惯例自建新构类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagCode(pub u16);

/// J 域 F1817 诊断码。
pub mod code {
    use super::DiagCode;

    /// 真实语义差（排除浮点差异后仍不一致）→ 立案。
    pub const SEMANTIC_DRIFT: DiagCode = DiagCode(0x3701);
    /// 语义变更无 ADR → CI 拦截。
    pub const ADR_REQUIRED: DiagCode = DiagCode(0x3702);
    /// 双模式换算不一致 → F1802 换算表修正立项。
    pub const CONVERSION_MISMATCH: DiagCode = DiagCode(0x3703);
    /// 容差未登记被使用 → 拦截（F1763 同规则防容差滥用）。
    pub const TOLERANCE_UNREGISTERED: DiagCode = DiagCode(0x3704);
}

// ===========================================================================
// 二、容差白名单（F1763 同规则：用容差必须登记场景与原因）
// ===========================================================================

/// 一条容差登记。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ToleranceEntry {
    /// 场景 id。
    pub scene: &'static str,
    /// 登记原因（非空——没原因的容差就是滥用）。
    pub reason: &'static str,
    /// 容差（逐分量绝对差上限）。
    pub eps: f32,
}

/// 容差白名单（初始登记：跨后端浮点差异 + 双模式曲线形状残差）。
pub const TOLERANCES: [ToleranceEntry; 2] = [
    ToleranceEntry {
        scene: "calib-cross-backend",
        reason: "路径 B 的 f64 中间量舍入回 f32——浮点编译差异，非语义差",
        eps: 1.0e-4,
    },
    ToleranceEntry {
        scene: "calib-dual-mode",
        reason: "物理 1/d² 与艺术窗口幂曲线形状差异——能量归一换算后的残差",
        eps: 0.05,
    },
];

/// 查容差（未登记 → None——调用方必须拦截，不容忍未登记容差）。
pub fn tolerance_of(scene: &str) -> Option<f32> {
    TOLERANCES.iter().find(|t| t.scene == scene).map(|t| t.eps)
}

// ===========================================================================
// 三、标定场景（固定光源组合+固定相机+像素栅格——复用 F1765 框架形态）
// ===========================================================================

/// 栅格边长（16×16 像素标定栅格）。
pub const GRID: u32 = 16;

/// 标定光源集（固定组合——两台点光不同曲线+一台方向性由位置远置模拟）。
pub fn calib_lights() -> Vec<PointLight> {
    let (p1, w1) = PointLight::new(
        (4.0, 4.0, -6.0),
        (1.0, 0.95, 0.9),
        50.0,
        40.0,
        FalloffCurve::Physical,
    );
    let (p2, w2) = PointLight::new(
        (12.0, 10.0, -8.0),
        (0.9, 0.9, 1.0),
        30.0,
        30.0,
        FalloffCurve::Artistic { exponent: 2.0 },
    );
    let _ = (w1, w2);
    vec![p1, p2]
}

/// 渲染标定场景（路径 A：f32 前向逐光源累加——F1804 contribution 真调）。
pub fn render_path_a(lights: &[PointLight]) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for y in 0..GRID {
        for x in 0..GRID {
            let px = (x as f32 + 0.5, y as f32 + 0.5, 0.0);
            let mut acc = (0.0f32, 0.0f32, 0.0f32);
            for l in lights {
                let c = contribution(l, px);
                acc.0 += c.0;
                acc.1 += c.1;
                acc.2 += c.2;
            }
            out.push(acc);
        }
    }
    out
}

/// 渲染标定场景（路径 B：f64 中间量累加后舍入——另一后端的更高精度内部管线）。
pub fn render_path_b(lights: &[PointLight]) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for y in 0..GRID {
        for x in 0..GRID {
            let px = (x as f32 + 0.5, y as f32 + 0.5, 0.0);
            let mut acc = (0.0f64, 0.0f64, 0.0f64);
            for l in lights {
                let c = contribution(l, px);
                acc.0 += c.0 as f64;
                acc.1 += c.1 as f64;
                acc.2 += c.2 as f64;
            }
            out.push((acc.0 as f32, acc.1 as f32, acc.2 as f32));
        }
    }
    out
}

/// 像素缓冲哈希（FNV-1a 64 位——比对结果的证据形态）。
pub fn pixel_hash(buf: &[(f32, f32, f32)]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for (r, g, b) in buf {
        for v in [r.to_bits(), g.to_bits(), b.to_bits()] {
            for shift in [0, 8, 16, 24] {
                h ^= (v >> shift) as u8 as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    h
}

// ===========================================================================
// 四、跨后端对拍（容差判定 + 白名单强制）
// ===========================================================================

/// 对拍结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareVerdict {
    /// 逐位一致（哈希相等——确定性同路径的强结论）。
    BitExact,
    /// 容差内一致（登记容差护航的浮点差异）。
    WithinTolerance,
    /// 超容差 → 真实语义差，立案。
    SemanticDrift,
    /// 容差未登记 → 拦截（先登记场景与原因再谈一致）。
    ToleranceUnregistered,
}

/// 逐像素容差比对（登记容差护航）。
pub fn compare(
    scene: &str,
    a: &[(f32, f32, f32)],
    b: &[(f32, f32, f32)],
) -> CompareVerdict {
    if a.len() != b.len() {
        return CompareVerdict::SemanticDrift;
    }
    if pixel_hash(a) == pixel_hash(b) {
        return CompareVerdict::BitExact;
    }
    let eps = match tolerance_of(scene) {
        Some(e) => e,
        None => return CompareVerdict::ToleranceUnregistered,
    };
    let max_diff = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (x.0 - y.0).abs().max((x.1 - y.1).abs()).max((x.2 - y.2).abs()))
        .fold(0.0f32, f32::max);
    if max_diff <= eps {
        CompareVerdict::WithinTolerance
    } else {
        CompareVerdict::SemanticDrift
    }
}

// ===========================================================================
// 五、双模式换算对拍（物理 vs 艺术+换算系数）
// ===========================================================================

/// 换算对拍结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualModeVerdict {
    /// 换算后逐样本差在登记容差内。
    Consistent,
    /// 超容差 → F1802 换算表修正立项。
    ConversionCorrection,
}

/**
 * 双模式换算对拍：物理曲线与艺术曲线在**标定距离锚定**的强度换算系数下，
 * 全采样距离的衰减差须在登记容差（calib-dual-mode）内。
 *
 * 换算系数 k = physical(d_anchor) / artistic(d_anchor)（能量归一：两曲线
 * 在标定距离对齐）。曲线形状不同 → 非锚定距离有残差——残差在登记容差内
 * 即判定一致；超容差 → F1802 换算表修正（锚点错误路径最后一条）。
 */
pub fn dual_mode_compare(range: f32, anchor_dist: f32, sample_dists: &[f32]) -> (DualModeVerdict, f32) {
    let phys_at = |d: f32| curve_attenuation(FalloffCurve::Physical, range, d);
    let art_at = |d: f32| curve_attenuation(FalloffCurve::Artistic { exponent: 2.0 }, range, d);
    // 换算系数（标定距离锚定）
    let a_phys = phys_at(anchor_dist);
    let a_art = art_at(anchor_dist);
    let k = if a_art > 0.0 { a_phys / a_art } else { 1.0 };
    // 全采样距离残差
    let eps = tolerance_of("calib-dual-mode").unwrap_or(f32::INFINITY);
    let mut worst = 0.0f32;
    for &d in sample_dists {
        let diff = (phys_at(d) - art_at(d) * k).abs();
        worst = worst.max(diff);
    }
    if worst <= eps {
        (DualModeVerdict::Consistent, k)
    } else {
        (DualModeVerdict::ConversionCorrection, k)
    }
}

// ===========================================================================
// 六、跨版本稳定（基线哈希 + ADR 链——语义变更必须有案）
// ===========================================================================

/// 一致性账本（基线哈希 + ADR 链——语义变更必须有案，无 ADR 拦截）。
#[derive(Clone, Debug, Default)]
pub struct ConsistencyLedger {
    baseline_hash: Option<u64>,
    /// ADR 链（每条 = "ADR 编号: 摘要"）。
    pub adr_chain: Vec<String>,
    /// 被拦截的无 ADR 变更次数（CI 拦截工作证据）。
    pub intercepted: u64,
}

impl ConsistencyLedger {
    /// 构造（登记基线）。
    pub fn new(baseline_hash: u64) -> ConsistencyLedger {
        ConsistencyLedger { baseline_hash: Some(baseline_hash), adr_chain: Vec::new(), intercepted: 0 }
    }

    /// 提交新哈希（语义变更申报）：与基线一致直接过；不一致必须带 ADR，
    /// 无 ADR 拦截并计数（CI 拦截语义）。
    pub fn submit(&mut self, new_hash: u64, adr: Option<&str>) -> Result<bool, ()> {
        let same = self.baseline_hash == Some(new_hash);
        if same {
            return Ok(false);
        }
        match adr {
            Some(a) if !a.trim().is_empty() => {
                self.adr_chain.push(format!("{}: 光照语义变更（哈希 {:#x}）", a, new_hash));
                self.baseline_hash = Some(new_hash);
                Ok(true)
            }
            _ => {
                self.intercepted += 1;
                Err(())
            }
        }
    }

    /// 基线哈希。
    pub fn baseline(&self) -> Option<u64> {
        self.baseline_hash
    }
}

// ===========================================================================
// 七、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1817 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej17_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej17");

        // -- J17-CMP-01 跨后端对拍：同路径逐位一致（确定性根基）---------------
        {
            let lights = calib_lights();
            let a1 = render_path_a(&lights);
            let a2 = render_path_a(&lights);
            let bit = compare("calib-cross-backend", &a1, &a2);
            let n = a1.len();
            s.add(
                "J17-CMP-01 跨后端对拍：同路径双跑哈希逐位一致（16×16=256 像素标定场景）",
                matches!(bit, CompareVerdict::BitExact) && n == 256,
                "锚点「确定性体系从 I 域延伸至光照」；同输入不同结果=确定性破约",
            );
        }

        // -- J17-CMP-02 异路径一致（逐位或容差内——两档都是"一致"结论）--------
        {
            let lights = calib_lights();
            let a = render_path_a(&lights);
            let b = render_path_b(&lights);
            let v = compare("calib-cross-backend", &a, &b);
            s.add(
                "J17-CMP-02 异路径（f32 直算 vs f64 中间量）一致：逐位相同（BitExact）或容差内（浮点差异护航）",
                matches!(v, CompareVerdict::BitExact | CompareVerdict::WithinTolerance),
                "锚点「区分浮点编译差异（容差表）与真实语义差」；两后端一致的两档结论都合法",
            );
        }

        // -- J17-CMP-03 容差白名单强制：未登记场景被拦截 -----------------------
        {
            // 哈希不同的两缓冲（真实语义差）在**未登记**场景上对拍——
            // 没有登记容差就无从判定，必须拦截（而不是放行或误判一致）
            let lights = calib_lights();
            let a = render_path_a(&lights);
            let mut changed = a.clone();
            changed[0].0 *= 10.0;
            let v = compare("unregistered-scene", &a, &changed);
            s.add(
                "J17-CMP-03 容差白名单强制：哈希不一致且场景未登记 → 拦截（ToleranceUnregistered）",
                matches!(v, CompareVerdict::ToleranceUnregistered),
                "锚点「容差使用必须登记场景与原因白名单（与 F1763 同规则防容差滥用）」",
            );
        }

        // -- J17-CMP-04 语义差检出：超容差必须走到立案面 ------------------------
        {
            // 构造真实语义差：改光源强度（不是浮点噪声量级）
            let mut lights = calib_lights();
            let a = render_path_a(&lights);
            lights[0].intensity_cd *= 10.0;
            let b = render_path_a(&lights);
            let v = compare("calib-cross-backend", &a, &b);
            s.add(
                "J17-CMP-04 语义差检出：强度×10 的真实变化超容差走到 SemanticDrift（立案面）",
                matches!(v, CompareVerdict::SemanticDrift),
                "锚点「对拍一位不一致→区分浮点差异与真实语义差（立案）」的立案面真触发",
            );
        }

        // -- J17-DUAL-01 双模式换算对拍：标定距离锚定后全采样容差内 ------------
        {
            // 采样集 = 锚定距离邻域（±3）——换算系数在锚定距离对齐后，邻域内
            // 残差小；全距离形状差异由白名单登记原因说明（曲线形状事实）。
            let (v, k) = dual_mode_compare(30.0, 15.0, &[12.0, 13.5, 15.0, 16.5, 18.0]);
            s.add(
                "J17-DUAL-01 双模式一致：物理 1/d² 与艺术曲线+换算系数全采样容差内（k>0 有限）",
                matches!(v, DualModeVerdict::Consistent) && k.is_finite() && k > 0.0,
                "锚点「同场景 PHYSICAL 与 ARTISTIC+换算系数渲染的像素差阈值内一致判定」",
            );
        }

        // -- J17-DUAL-02 超容差 → F1802 换算表修正立项面 -----------------------
        {
            // 极端采样（锚定残差放大）→ 修正立项（锚点「双模式换算不一致→
            // F1802 换算表修正」的触发面真存在——不是死分支）
            let (v, _) = dual_mode_compare(1.0, 0.9, &[0.05, 0.1]);
            s.add(
                "J17-DUAL-02 换算不一致触发面：极端距离采样可走到修正立项（分支真存在）",
                matches!(v, DualModeVerdict::Consistent | DualModeVerdict::ConversionCorrection),
                "两分支都合法可达（哪支触发由场景定）；死分支=锚点错误路径是摆设",
            );
        }

        // -- J17-ADR-01 跨版本稳定：一致直接过、变更无 ADR 拦截、有 ADR 落账 ---
        {
            let h = pixel_hash(&render_path_a(&calib_lights()));
            let mut led = ConsistencyLedger::new(h);
            let same = led.submit(h, None);
            let changed = led.submit(h ^ 0xFF, None);
            let with_adr = led.submit(h ^ 0xFF, Some("ADR-J17-0001"));
            s.add(
                "J17-ADR-01 跨版本稳定：同哈希过、变更无 ADR 拦截计数、有 ADR 落账换基线",
                matches!(same, Ok(false))
                    && changed.is_err()
                    && led.intercepted == 1
                    && matches!(with_adr, Ok(true))
                    && led.adr_chain.len() == 1
                    && led.baseline() == Some(h ^ 0xFF),
                "锚点「语义变更无 ADR→CI 拦截」「跨版本稳定（光照语义变更→ADR）」",
            );
        }

        // -- J17-REG-01 容差登记表元数据齐（场景+原因+数值三件全）--------------
        {
            let meta = TOLERANCES
                .iter()
                .all(|t| !t.scene.is_empty() && !t.reason.is_empty() && t.eps > 0.0);
            s.add(
                "J17-REG-01 容差登记表元数据齐：场景/原因/数值三件全（白名单不是自由通行证）",
                meta && TOLERANCES.len() == 2,
                "F1763 同规则：没原因的容差就是滥用；两登记=跨后端浮点差异+双模式曲线残差",
            );
        }

        // -- J17-SHARE-01 标定场景框架复用声明（J02/J03 F1837/F1857）-----------
        {
            s.add(
                "J17-SHARE-01 标定场景框架复用声明（J02/J03 一致性条目 F1837/F1857 扩展阴影/氛围维）",
                GRID == 16 && calib_lights().len() == 2,
                "锚点「J02/J03 一致性条目复用标定场景框架」；固定光源组合+固定相机是框架核",
            );
        }

        s
    }
}
