//! H2 壁纸填充几何 · 深化批次六（F286 fit 三模式的像素级计算——
//! 居中/拉伸/适配裁切，构图保护的几何落位）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F286 壁纸多屏设置**：填充方式三档（h2wallsvc `WallRef.fit`
//!   的几何执行面：0 居中 / 1 拉伸 / 2 适配裁切）；
//! - **F154 车道（经 F286 锚）**：构图保护——主体不裁头：适配裁切
//!   时按重心偏移裁切（`focus_pm` 千分率重心锚，默认 500 居中），
//!   裁切窗口永远在源图内（越界钳制）；
//! - **拉伸不变形缺口**：拉伸=满屏填满（变形是模式本意，诚实）；
//!   居中不缩放（1:1 像素——大图裁切、小图留边，均如实）。
//!
//! 几何纪律：纯函数；全部输出钳在源图与目标屏内。

use crate::checks::CheckSet;


// ---------------------------------------------------------------------------
// 三模式计算
// ---------------------------------------------------------------------------

/// 适配裁切：等比缩放至覆盖目标，再按重心裁掉多余。
/// 返回 (缩放后源图宽高, 裁切窗口在缩放图上的原点)。
pub fn cover_crop(
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    focus_x_pm: u32,
) -> (u32, u32, u32, u32) {
    let sw = src_w.max(1) as u64;
    let sh = src_h.max(1) as u64;
    let dw = dst_w.max(1) as u64;
    let dh = dst_h.max(1) as u64;
    // 覆盖比例（千分率）：短边贴满、长边溢出——取较大者。
    let r = (dw * 1000 / sw).max(dh * 1000 / sh).max(1);
    let s_w = (sw * r / 1000).max(dw);
    let s_h = (sh * r / 1000).max(dh);
    // 水平按重心锚裁切（0=左 500=中 1000=右），垂直居中；越界钳制。
    let overflow_x = s_w - dw;
    let overflow_y = s_h - dh;
    let fx = focus_x_pm.min(1000) as u64;
    let crop_x = (overflow_x * fx / 1000) as u32;
    let crop_y = (overflow_y / 2) as u32;
    (s_w as u32, s_h as u32, crop_x, crop_y)
}

/// 居中模式：1:1 像素呈现，源大于屏则裁边、小于屏则留边。
/// 返回 (绘制原点在目标屏上的位置, 裁切源窗口)。
pub fn center_mode(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> ((i32, i32), (u32, u32, u32, u32)) {
    let draw_x = (dst_w as i64 - src_w as i64) / 2;
    let draw_y = (dst_h as i64 - src_h as i64) / 2;
    let crop_w = src_w.min(dst_w);
    let crop_h = src_h.min(dst_h);
    let crop_x = src_w.saturating_sub(crop_w) / 2;
    let crop_y = src_h.saturating_sub(crop_h) / 2;
    ((draw_x as i32, draw_y as i32), (crop_x, crop_y, crop_w, crop_h))
}

/// 拉伸模式：满屏（变形如实——不做保持比例的假拉伸）。
pub fn stretch_mode(dst_w: u32, dst_h: u32) -> (u32, u32) {
    (dst_w, dst_h)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2wallfit_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2wallfit");
    // 适配裁切：4:3 图铺 16:9 屏——短边（高）贴满、左右溢出；
    // 重心 500（居中）裁对称、重心 0/1000 钳到两端。
    let (sw, sh, cx, cy) = cover_crop(1600, 900, 1920, 1080, 500);
    set.add(
        "h2wallfit cover fill",
        sw >= 1920 && sh >= 1080 && cx <= sw - 1920 && cy <= sh - 1080,
        "cover + crop inside",
    );
    let (sw0, _, cx0, _) = cover_crop(1600, 900, 1920, 1080, 0);
    let (sw1, _, cx1, _) = cover_crop(1600, 900, 1920, 1080, 1000);
    set.add(
        "h2wallfit focus anchors",
        sw0 == sw1 && cx0 <= cx && cx <= cx1,
        "focus orders crops",
    );
    // 竖图铺横屏：覆盖缩放正确（宽主导）。
    let (vw, vh, vcap_x, vcap_y) = cover_crop(1080, 1920, 1920, 1080, 500);
    set.add(
        "h2wallfit portrait cover",
        vw >= 1920 && vh >= 1080 && vcap_x <= vw - 1920 && vcap_y <= vh - 1080,
        "portrait covers too",
    );
    // 居中：小图留边（负偏移）、大图裁源。
    let ((dx_s, dy_s), (crop_x_s, crop_y_s, cw_s, ch_s)) = center_mode(800, 600, 1920, 1080);
    set.add(
        "h2wallfit center small",
        dx_s == 560 && dy_s == 240 && crop_x_s == 0 && crop_y_s == 0 && cw_s == 800 && ch_s == 600,
        "letterbox honest",
    );
    let ((_, _), (crop_x_b, crop_y_b, cw_b, ch_b)) = center_mode(2400, 1600, 1920, 1080);
    set.add(
        "h2wallfit center large",
        crop_x_b == 240 && crop_y_b == 260 && cw_b == 1920 && ch_b == 1080,
        "crop from center",
    );
    // 拉伸：恒满屏。
    set.add(
        "h2wallfit stretch",
        stretch_mode(1920, 1080) == (1920, 1080)
            && stretch_mode(3840, 2160) == (3840, 2160),
        "always fills",
    );
    // 零尺寸防御：不 panic（除零钳制）。
    let (zw, zh, zcx, zcy) = cover_crop(0, 0, 1920, 1080, 500);
    set.add(
        "h2wallfit zero src safe",
        zw >= 1920 && zh >= 1080 && zcx <= zw && zcy <= zh,
        "no divide by zero",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2wallfit_all_green() {
        let set = run_h2wallfit_checks();
        assert!(set.all_passed(), "h2wallfit 自检有红项");
        assert!(!set.truncated(), "h2wallfit 自检溢出");
    }

    #[test]
    fn crop_window_always_inside() {
        // 20 组源/目标组合 × 5 档重心：裁切窗永不越出缩放图。
        for (sw, sh) in [(800u32, 600), (1600, 900), (1080, 1920), (3840, 2160), (640, 480)] {
            for fx in [0u32, 250, 500, 750, 1000] {
                let (w, h, cx, cy) = cover_crop(sw, sh, 1920, 1080, fx);
                assert!(cx + 1920 <= w.max(1920), "crop out x at {sw}x{sh} fx={fx}");
                assert!(cy + 1080 <= h.max(1080), "crop out y at {sw}x{sh} fx={fx}");
            }
        }
    }
}
