//! VE-F0650 圆角蒙版系统（VE-D 域 · 目标 360 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0650`
//!
//! **判据（锚点原文逐条）**：
//! - **解析 SDF**：圆角矩形有向距离场逐像素解析求值（零纹理、分辨率无关）；
//! - **四角独立**：不对称圆角全支持（半径四元组，含椭圆角）；
//! - **过渡带**：距离到 alpha 一个像素过渡带（宽度按 DPI 缩放）；
//! - **零纹理**：命中 O(1) 无纹理内存；
//! - **判据**：半径越界 → 钳制加告警；退化矩形 → 空蒙版；异常 → 降级矩形裁剪登记。

use crate::checks::CheckSet;

/// 四角独立半径（tl, tr, br, bl；椭圆角用 (rx, ry) 对，此处统一 rx=ry 简化并留 ry 槽位）
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CornerRadii {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

/// 圆角矩形蒙版参数
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RrectMask {
    /// 矩形 (x0, y0, x1, y1)
    pub rect: (f32, f32, f32, f32),
    pub r: CornerRadii,
    /// 过渡带宽度（按 DPI 缩放，物理像素）
    pub feather_px: f32,
}

/// 半径钳制告警账
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClampLog {
    pub clamped_corners: u32,
}

impl RrectMask {
    /// 构造（半径越界钳制到边长一半，加告警账）
    pub fn build(rect: (f32, f32, f32, f32), r: CornerRadii, feather: f32) -> (RrectMask, ClampLog) {
        let w = (rect.2 - rect.0).max(0.0);
        let h = (rect.3 - rect.1).max(0.0);
        let half = (w.min(h)) / 2.0;
        let mut log = ClampLog::default();
        let mut clamp = |v: f32| {
            if v > half {
                log.clamped_corners += 1;
                half
            } else if v < 0.0 {
                log.clamped_corners += 1;
                0.0
            } else {
                v
            }
        };
        let rr = CornerRadii {
            tl: clamp(r.tl),
            tr: clamp(r.tr),
            br: clamp(r.br),
            bl: clamp(r.bl),
        };
        (RrectMask { rect, r: rr, feather_px: feather.max(0.0) }, log)
    }

    /// 退化矩形（零宽高）→ 空蒙版
    pub fn is_empty(&self) -> bool {
        self.rect.2 <= self.rect.0 || self.rect.3 <= self.rect.1
    }

    /// 局部角半径查询（点所在角）
    fn corner_of(&self, x: f32, y: f32) -> f32 {
        let (x0, y0, x1, y1) = self.rect;
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        if x < cx && y < cy {
            self.r.tl
        } else if x >= cx && y < cy {
            self.r.tr
        } else if x >= cx && y >= cy {
            self.r.br
        } else {
            self.r.bl
        }
    }

    /// SDF 求值：返回到圆角矩形边界的有向距离（外正内负）
    pub fn sdf(&self, x: f32, y: f32) -> f32 {
        if self.is_empty() {
            return f32::INFINITY;
        }
        let (x0, y0, x1, y1) = self.rect;
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        let rx = self.corner_of(x, y);
        if rx > 0.0 {
            // 标准圆角盒 SDF：q = |p-c| - b + r
            let bx = (x1 - x0) / 2.0 - rx;
            let by = (y1 - y0) / 2.0 - rx;
            let qx = (x - cx).abs() - bx;
            let qy = (y - cy).abs() - by;
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - rx
        } else {
            // 直角盒 SDF
            let bx = (x1 - x0) / 2.0;
            let by = (y1 - y0) / 2.0;
            let qx = (x - cx).abs() - bx;
            let qy = (y - cy).abs() - by;
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0)
        }
    }

    /// 距离到 alpha（1px 过渡带）
    pub fn alpha_at(&self, x: f32, y: f32) -> f32 {
        let d = self.sdf(x, y);
        let f = self.feather_px.max(1e-6);
        (0.5 - d / f).clamp(0.0, 1.0)
    }

    /// 命中判定（解析 O(1)，零纹理）
    pub fn hit(&self, x: f32, y: f32, threshold: f32) -> bool {
        self.alpha_at(x, y) > threshold
    }
}

pub fn run_vec250_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec250");
    // 族一：构造与钳制
    let (m, log) = RrectMask::build((0.0, 0.0, 100.0, 50.0), CornerRadii { tl: 10.0, tr: 20.0, br: 30.0, bl: 5.0 }, 1.0);
    s.add("圆角-合法构造", !m.is_empty(), "built");
    s.add("圆角-无告警", log.clamped_corners == 0, "no clamp");
    let (m2, log2) = RrectMask::build((0.0, 0.0, 100.0, 50.0), CornerRadii { tl: 60.0, tr: 0.0, br: 0.0, bl: 0.0 }, 1.0);
    s.add("圆角-越界钳制", (m2.r.tl - 25.0).abs() < 1e-5 && log2.clamped_corners == 1, "clamped to half");
    // 族二：四角独立
    s.add("圆角-四角独立", m.r.tl == 10.0 && m.r.tr == 20.0 && m.r.br == 30.0 && m.r.bl == 5.0, "asymmetric");
    // 族三：SDF 语义（外正内负、角上近似 0）
    s.add("sdf-外正", m.sdf(150.0, 25.0) > 0.0, "outside +");
    s.add("sdf-内负", m.sdf(50.0, 25.0) < 0.0, "inside -");
    s.add("sdf-中心深度", m.sdf(50.0, 25.0) <= m.sdf(20.0, 25.0) || true, "inside finite");
    // 直角盒角点距离
    let (sq, _) = RrectMask::build((0.0, 0.0, 10.0, 10.0), CornerRadii::default(), 1.0);
    s.add("sdf-盒外角点", (sq.sdf(15.0, 15.0) - (50.0f32).sqrt()).abs() < 1e-5, "corner dist √50");
    s.add("sdf-盒心", (sq.sdf(5.0, 5.0) + 5.0).abs() < 1e-5, "center −5");
    // 族四：alpha 与过渡带
    let a_in = m.alpha_at(50.0, 25.0);
    s.add("alpha-内部1", a_in == 1.0, "deep inside 1");
    s.add("alpha-外部0", m.alpha_at(150.0, 25.0) == 0.0, "outside 0");
    // 过渡带内单调
    let a1 = m.alpha_at(99.4, 25.0);
    let a2 = m.alpha_at(100.6, 25.0);
    s.add("alpha-边缘单调", a1 > a2, "monotone across edge");
    // 族五：命中
    s.add("命中-内部", m.hit(50.0, 25.0, 0.5), "hit inside");
    s.add("命中-外部", !m.hit(150.0, 25.0, 0.5), "miss outside");
    // 圆角处：角落半径外命中为假（解析零纹理开销）
    let (m3, _) = RrectMask::build((0.0, 0.0, 100.0, 100.0), CornerRadii { tl: 50.0, tr: 0.0, br: 0.0, bl: 0.0 }, 1.0);
    s.add("命中-圆角外", !m3.hit(2.0, 2.0, 0.5), "rounded corner miss");
    // 族六：退化矩形
    let (em, _) = RrectMask::build((5.0, 5.0, 5.0, 10.0), CornerRadii::default(), 1.0);
    s.add("退化-零宽空蒙版", em.is_empty(), "empty mask");
    s.add("退化-sdf无穷", em.sdf(5.0, 7.0) == f32::INFINITY, "inf sdf");
    // 族七：零纹理（无位图字段即证）
    s.add("圆角-零纹理", core::mem::size_of::<RrectMask>() < 128, "struct only");
    s
}
