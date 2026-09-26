//! F449 摄像头预览测试 · 完整设计（STAR I 主册 G-I-49）。
//!
//! **判据（主册）**：预览延迟 <200ms；参数两路（硬件/标注）行为；镜像
//! 开关；指示常亮判据；预览关闭即释放摄像头（指示灭）。＋通12。
//!
//! 设计：摄像头预览核——预览帧入窗延迟账（<200ms 判据，超线诚实
//! 计数）；画面参数三调（亮度/对比度/饱和度）两路：硬件支持 → 写硬件
//! 返回 Ok，不支持 → 诚实标注 Err「该摄像头不支持此调节」——不假装
//! 调好了；镜像开关（视频会议习惯——预览水平翻转语义）；F323 指示
//! 常亮判据（预览开着 = 指示亮——不逃避「我也在被监视」的自觉）；
//! 关闭即释放（帧缓冲清空 + 指示灭 + 释放账——资源不留后台）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 预览延迟判线（ms）。
pub const PREVIEW_LATENCY_BUDGET_MS: u64 = 200;

/// 画面参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamParam {
    Brightness,
    Contrast,
    Saturation,
}

/// 摄像头预览核。
pub struct CameraPreview {
    /// 硬件可调参数白名单（能力探测结果——注入）。
    pub hw_supported: [bool; 3],
    /// 参数当前值（0-100）。
    pub values: [u8; 3],
    pub mirrored: bool,
    /// 预览态。
    pub open: bool,
    /// 延迟账。
    pub frame_latencies_ms: Vec<u64>,
    pub over_budget_frames: u64,
    /// F323 指示。
    pub indicator_on: bool,
    /// 释放账（正常关闭 = 1；异常丢失 = 由自检发现）。
    pub releases: u64,
}

impl CameraPreview {
    pub fn new(hw_supported: [bool; 3]) -> CameraPreview {
        CameraPreview {
            hw_supported,
            values: [50, 50, 50],
            mirrored: false,
            open: false,
            frame_latencies_ms: Vec::new(),
            over_budget_frames: 0,
            indicator_on: false,
            releases: 0,
        }
    }

    /// 开预览：指示立即亮（隐私优先——指示先于第一帧）。
    pub fn open_preview(&mut self) {
        self.open = true;
        self.indicator_on = true;
    }

    /// 帧入窗：延迟记账。
    pub fn frame_tick(&mut self, latency_ms: u64) {
        if latency_ms > PREVIEW_LATENCY_BUDGET_MS {
            self.over_budget_frames += 1;
        }
        self.frame_latencies_ms.push(latency_ms);
    }

    /// 参数调节两路：硬件支持 → 生效；不支持 → 诚实标注（不假装）。
    pub fn set_param(&mut self, p: CamParam, v: u8) -> Result<u8, &'static str> {
        if !self.open {
            return Err("预览未开启——先打开预览再调节");
        }
        let idx = p as usize;
        if !self.hw_supported[idx] {
            return Err("该摄像头不支持此调节——参数仅作展示");
        }
        self.values[idx] = v.clamp(0, 100);
        Ok(self.values[idx])
    }

    pub fn toggle_mirror(&mut self) -> bool {
        self.mirrored = !self.mirrored;
        self.mirrored
    }

    /// 关闭即释放：指示灭 + 帧缓冲停 + 释放账 +1。
    pub fn close_preview(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.indicator_on = false;
        self.frame_latencies_ms.clear();
        self.releases += 1;
        true
    }
}

pub fn run_camtest_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F449");
    // 硬件支持亮度/对比度、不支持饱和度（能力探测注入）。
    let mut c = CameraPreview::new([true, true, false]);
    c.open_preview();
    // 指示常亮判据：预览开着指示就亮（第一帧之前就亮）。
    set.add(
        "f449-indicator-constant",
        c.open && c.indicator_on && c.frame_latencies_ms.is_empty(),
        "",
    );
    // 预览延迟：<200ms；超线记账。
    c.frame_tick(90);
    c.frame_tick(180);
    set.add("f449-latency-under-200", c.over_budget_frames == 0 && c.frame_latencies_ms.len() == 2, "");
    c.frame_tick(240);
    set.add("f449-over-budget-logged", c.over_budget_frames == 1, "");
    // 参数两路：硬件 → 生效；不支持 → 诚实标注。
    set.add(
        "f449-param-hw-path",
        c.set_param(CamParam::Brightness, 70) == Ok(70) && c.values[0] == 70,
        "",
    );
    set.add(
        "f449-param-honest-unsupported",
        matches!(
            c.set_param(CamParam::Saturation, 80),
            Err("该摄像头不支持此调节——参数仅作展示")
        ) && c.values[2] == 50,
        "",
    );
    set.add("f449-param-clamped", c.set_param(CamParam::Contrast, 200) == Ok(100), "");
    // 镜像开关。
    set.add("f449-mirror-toggle", !c.mirrored && c.toggle_mirror() && c.mirrored, "");
    // 关闭即释放：指示灭 + 释放账 + 再关无动作。
    set.add(
        "f449-close-releases",
        c.close_preview() && !c.indicator_on && c.releases == 1 && c.frame_latencies_ms.is_empty() && !c.close_preview(),
        "",
    );
    // 未开预览调参 → 拒绝（先开再调）。
    set.add(
        "f449-param-needs-preview",
        matches!(c.set_param(CamParam::Brightness, 50), Err("预览未开启——先打开预览再调节")),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_never_lags_frames() {
        let mut c = CameraPreview::new([true; 3]);
        // 第一帧到达前指示必须已亮（隐私先于画面）。
        c.open_preview();
        let indicator_before_first_frame = c.indicator_on;
        c.frame_tick(50);
        assert!(indicator_before_first_frame);
        assert!(c.indicator_on);
        c.close_preview();
        assert!(!c.indicator_on, "关预览 → 指示灭（不常驻监听）");
    }

    #[test]
    fn release_account_honest() {
        let mut c = CameraPreview::new([true; 3]);
        c.open_preview();
        assert!(c.close_preview());
        assert_eq!(c.releases, 1);
        assert!(!c.close_preview(), "重复关闭不计入释放账（防虚账）");
        assert_eq!(c.releases, 1);
        c.open_preview();
        assert!(c.close_preview());
        assert_eq!(c.releases, 2);
    }
}
