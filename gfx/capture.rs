//! TRINITY-500 · AI-05 · F120 截图/录屏（capture 通道）
//!
//! 内核侧只做**采集与预算**，不做编码（无编解码器是事实，不假装支持 H.264）。
//! 帧数据按原始像素落盘，由用户态工具转码。

use crate::gfx::surface::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFormat {
    /// 原始 BGRA 行（无损，体积大）。
    RawPixels,
    /// 位图封装（BMP，无压缩，Windows 与 Varix 都能直接看）。
    Bmp,
}

impl CaptureFormat {
    pub fn name(self) -> &'static str {
        match self {
            CaptureFormat::RawPixels => "raw",
            CaptureFormat::Bmp => "bmp",
        }
    }

    /// 每像素额外头部开销（BMP 有 54 字节文件头）。
    pub fn header_bytes(self) -> u64 {
        match self {
            CaptureFormat::RawPixels => 0,
            CaptureFormat::Bmp => 54,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureRequest {
    pub rect: Rect,
    pub format: CaptureFormat,
    pub fps: u32,
    pub seconds: u32,
}

impl CaptureRequest {
    pub fn frame_bytes(&self) -> u64 {
        let px = (self.rect.w.max(0) as u64) * (self.rect.h.max(0) as u64);
        px * 4 + self.format.header_bytes()
    }

    pub fn total_frames(&self) -> u64 {
        if self.fps == 0 {
            return 1; // 截图
        }
        self.fps as u64 * self.seconds as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.frame_bytes() * self.total_frames()
    }

    pub fn is_screenshot(&self) -> bool {
        self.fps == 0 || self.seconds == 0
    }
}

pub const CAPTURE_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// 录屏是否超预算（超了就拒绝，不静默丢帧）。
pub fn within_budget(req: &CaptureRequest) -> bool {
    req.total_bytes() <= CAPTURE_BUDGET_BYTES
}

/// 建议的最长录制秒数（在给定预算内）。
pub fn max_seconds(req: &CaptureRequest) -> u32 {
    if req.fps == 0 || req.frame_bytes() == 0 {
        return 0;
    }
    let s = CAPTURE_BUDGET_BYTES / (req.frame_bytes() * req.fps as u64);
    if s > u32::MAX as u64 {
        u32::MAX
    } else {
        s as u32
    }
}

// ---------------------------------------------------------------------------
// 采集状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Capturing,
    Paused,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureSession {
    pub state: CaptureState,
    pub frames_done: u64,
    pub frames_target: u64,
    pub dropped: u64,
}

impl CaptureSession {
    pub fn start(req: &CaptureRequest) -> Result<CaptureSession, &'static str> {
        if !within_budget(req) {
            return Err("capture exceeds budget");
        }
        Ok(CaptureSession {
            state: CaptureState::Capturing,
            frames_done: 0,
            frames_target: req.total_frames(),
            dropped: 0,
        })
    }

    /// 记录一帧。`ok=false` 表示这一帧没抓到（如实计入 dropped，不伪造帧）。
    /// 丢帧也算一次尝试——尝试数到目标就停，避免录屏永远结束不了。
    pub fn frame(&mut self, ok: bool) {
        if self.state != CaptureState::Capturing {
            return;
        }
        if ok {
            self.frames_done += 1;
        } else {
            self.dropped += 1;
        }
        if self.frames_done + self.dropped >= self.frames_target {
            self.state = CaptureState::Done;
        }
    }

    pub fn pause(&mut self) {
        if self.state == CaptureState::Capturing {
            self.state = CaptureState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == CaptureState::Paused {
            self.state = CaptureState::Capturing;
        }
    }

    pub fn stop(&mut self) {
        if matches!(self.state, CaptureState::Capturing | CaptureState::Paused) {
            self.state = CaptureState::Done;
        }
    }

    /// 完成百分比（permille）；dropped 也算进度，避免假进度。
    pub fn progress_permille(&self) -> u16 {
        if self.frames_target == 0 {
            return 1000;
        }
        let done = (self.frames_done + self.dropped).min(self.frames_target);
        ((done * 1000) / self.frames_target) as u16
    }

    /// 掉帧率 permille——超 50‰ 视为采集管线跟不上。
    pub fn drop_rate_permille(&self) -> u16 {
        let total = self.frames_done + self.dropped;
        if total == 0 {
            return 0;
        }
        ((self.dropped * 1000) / total) as u16
    }
}

/// 截图文件名（不含路径）：`shot-<序号>.bmp`，序号 16 进制定长。
pub fn shot_name(index: u32, out: &mut [u8]) -> usize {
    let prefix = b"shot-";
    let hex = b"0123456789abcdef";
    let mut n = 0usize;
    for &b in prefix.iter() {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    }
    for shift in (0..8).rev() {
        if n < out.len() {
            out[n] = hex[((index >> (shift * 4)) & 0xF) as usize];
            n += 1;
        }
    }
    for &b in b".bmp".iter() {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f120_frame_size_and_budget() {
        let req = CaptureRequest {
            rect: Rect::new(0, 0, 1920, 1080),
            format: CaptureFormat::Bmp,
            fps: 30,
            seconds: 10,
        };
        assert_eq!(req.frame_bytes(), 1920 * 1080 * 4 + 54);
        assert_eq!(req.total_frames(), 300);
        assert!(!req.is_screenshot());
        assert!(!within_budget(&req));
        assert!(max_seconds(&req) < 10);
    }

    #[test]
    fn f120_screenshot_is_single_frame() {
        let req = CaptureRequest {
            rect: Rect::new(0, 0, 64, 64),
            format: CaptureFormat::RawPixels,
            fps: 0,
            seconds: 0,
        };
        assert!(req.is_screenshot());
        assert_eq!(req.total_frames(), 1);
        assert!(within_budget(&req));
    }

    #[test]
    fn f120_session_counts_drops() {
        let req = CaptureRequest {
            rect: Rect::new(0, 0, 8, 8),
            format: CaptureFormat::RawPixels,
            fps: 10,
            seconds: 1,
        };
        let mut s = CaptureSession::start(&req).unwrap();
        for _ in 0..9 {
            s.frame(true);
        }
        s.frame(false);
        assert_eq!(s.state, CaptureState::Done);
        assert_eq!(s.dropped, 1);
        assert_eq!(s.drop_rate_permille(), 100);
        assert_eq!(s.progress_permille(), 1000);
    }

    #[test]
    fn f120_over_budget_is_refused() {
        let req = CaptureRequest {
            rect: Rect::new(0, 0, 3840, 2160),
            format: CaptureFormat::RawPixels,
            fps: 60,
            seconds: 600,
        };
        assert!(CaptureSession::start(&req).is_err());
    }

    #[test]
    fn f120_shot_name_is_fixed_width() {
        let mut out = [0u8; 32];
        let n = shot_name(0x1F, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "shot-0000001f.bmp");
    }
}
