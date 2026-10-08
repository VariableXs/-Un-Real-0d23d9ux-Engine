//! VE-F0210 · virtio 光标通道（VE-B 域 · GPU 驱动矩阵 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0210`
//!
//! **判据（锚点原文）**：通道分离、最新位保序、软件兜底、热点保留、判据。
//! - **通道分离**：cursorq 与 controlq 分离（上游 F0201 已定两条队列）——
//!   光标移动只进 cursorq，不走主渲染管线（撕裂之源）；
//! - **最新位保序**：位置更新轻量化（仅坐标×热点，零拷贝不含重绘），
//!   cursorq 满 → 丢弃中间帧保最新位（单槽覆盖，O(1)）；
//! - **软件兜底**：cursorq 不可用 → 降级软件光标，切换**显性通知**（回切
//!   同样通知）；
//! - **图像资源独立创建与更新**：图像按需上传；过大图像 → 拒绝并保旧图
//!   （不静默截断）；
//! - **热点保留**：hotspot 随消息与状态双保留，供读屏坐标换算（无障碍面）。
//!
//! 性能逐项分解：位置更新 O(1) 零拷贝；图像上传按需；最新位丢弃 O(1)。

use super::veb02_proto::CMD_UPDATE_CURSOR;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、光标资源与消息
// ---------------------------------------------------------------------------

/// 光标图像（独立资源；ARGB 8888 一族）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorImage {
    pub resource_id: u32,
    pub width: u32,
    pub height: u32,
    /// 热点（图像内坐标——判据"热点保留"的第一保留点）
    pub hot_x: u32,
    pub hot_y: u32,
    /// 字节数（须恰为 w*h*4——ARGB 8888；不含图像本体的轻量记账）
    pub data_len: u32,
}

/// 位置更新消息（仅坐标×热点×资源引用——零拷贝，无图像数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PosUpdate {
    pub scanout: u32,
    pub x: u32,
    pub y: u32,
    pub resource_id: u32,
}

/// 通道模式（判据：通道分离的落点——光标走哪条通路）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelMode {
    /// cursorq 硬件通道
    Hardware,
    /// 软件光标兜底（合成器叠加，不进 cursorq）
    Software,
}

/// 通道切换通知（判据：切换显性通知——不静默变通路）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchNotice {
    pub from: ChannelMode,
    pub to: ChannelMode,
    pub reason: String,
}

impl SwitchNotice {
    pub fn is_explicit(&self) -> bool {
        !self.reason.is_empty()
    }
}

/// 光标通道错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl CursorError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

fn err(code: &'static str, what: String, why: String, next: String) -> CursorError {
    CursorError { code, what, why, next }
}

/// 通道配置（尺寸上限与队列深度可配）。
#[derive(Clone, Copy, Debug)]
pub struct CursorConfig {
    /// 单边最大像素（virtio 惯例 64×64；上限留到 128 容忍高 DPI 光标）
    pub max_dim: u32,
    /// 图像字节上限（拒绝超大资源的闸线）
    pub max_image_bytes: u32,
}

impl CursorConfig {
    pub const DEFAULT_MAX_DIM: u32 = 128;

    pub fn default_config() -> CursorConfig {
        CursorConfig {
            max_dim: CursorConfig::DEFAULT_MAX_DIM,
            max_image_bytes: 128 * 128 * 4,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、光标通道
// ---------------------------------------------------------------------------

/// 光标通道状态机（判据四条的载体）。
#[derive(Debug)]
pub struct CursorChannel {
    cfg: CursorConfig,
    mode: ChannelMode,
    /// cursorq 是否可用（false = 必须走软件兜底）
    queue_available: bool,
    /// 当前光标图像（旧图在被替换/拒绝时的保留处）
    image: Option<CursorImage>,
    /// 最新位单槽（判据"最新位保序"的 O(1) 载体：中间帧被覆盖即丢弃）
    pending_pos: Option<PosUpdate>,
    /// 切换通知流（显性记录，可读屏）
    pub notices: Vec<SwitchNotice>,
    /// 位置更新计数（遥测）
    pub pos_updates: u64,
    /// 丢弃的中间帧计数（遥测——"悄悄丢弃"是红线，计数公开）
    pub dropped_intermediate: u64,
}

impl CursorChannel {
    pub fn new(cfg: CursorConfig) -> CursorChannel {
        CursorChannel {
            cfg,
            mode: ChannelMode::Hardware,
            queue_available: true,
            image: None,
            pending_pos: None,
            notices: Vec::new(),
            pos_updates: 0,
            dropped_intermediate: 0,
        }
    }

    pub fn mode(&self) -> ChannelMode {
        self.mode
    }

    pub fn image(&self) -> Option<&CursorImage> {
        self.image.as_ref()
    }

    /// 热点读数（判据：热点保留——读屏坐标换算的直接消费面）。
    pub fn hotspot(&self) -> (u32, u32) {
        match &self.image {
            Some(img) => (img.hot_x, img.hot_y),
            None => (0, 0),
        }
    }

    /// cursorq 可用性变更：与当前模式不一致时切换并产出显性通知。
    pub fn set_queue_available(&mut self, avail: bool) -> Option<SwitchNotice> {
        self.queue_available = avail;
        let want = if avail { ChannelMode::Hardware } else { ChannelMode::Software };
        if self.mode == want {
            return None;
        }
        let notice = SwitchNotice {
            from: self.mode,
            to: want,
            reason: if avail {
                "cursorq 恢复可用——光标回切硬件通道".to_string()
            } else {
                "cursorq 不可用——降级软件光标兜底（合成器叠加）".to_string()
            },
        };
        self.mode = want;
        self.notices.push(notice.clone());
        Some(notice)
    }

    /// 光标图像创建/替换（判据：过大 → 拒绝并保旧图）。
    pub fn set_image(&mut self, img: CursorImage) -> Result<(), CursorError> {
        // 边界防护：尺寸与字节量双闸
        if img.width == 0 || img.height == 0 || img.width > self.cfg.max_dim || img.height > self.cfg.max_dim {
            return Err(err(
                "E_CURSOR_DIM",
                format!(
                    "光标图像 {}×{} 超出 {}×{} 上限",
                    img.width, img.height, self.cfg.max_dim, self.cfg.max_dim
                ),
                "超大光标资源会拖垮每帧位置更新通道".to_string(),
                format!("换用不超过 {}×{} 的图像", self.cfg.max_dim, self.cfg.max_dim),
            ));
        }
        let expect = img.width * img.height * 4;
        if img.data_len != expect {
            return Err(err(
                "E_CURSOR_FORMAT",
                format!("图像字节数 {} 与 {}×{}×4 不符", img.data_len, img.width, img.height),
                "ARGB 8888 光标的字节数是硬约束——不符说明格式记录错了".to_string(),
                "核对宽高与格式标注".to_string(),
            ));
        }
        if img.data_len > self.cfg.max_image_bytes {
            // 拒绝并保旧图（不静默截断）
            return Err(err(
                "E_CURSOR_TOO_BIG",
                format!("光标图像 {} 字节超过上限 {}", img.data_len, self.cfg.max_image_bytes),
                "超大图像拒绝接受——旧光标继续显示，不出现无光标窗口".to_string(),
                "缩小图像或调整 CursorConfig::max_image_bytes".to_string(),
            ));
        }
        self.image = Some(img);
        Ok(())
    }

    /// 位置更新（判据：最新位保序——cursorq 满时丢中间帧保最新位，O(1)；
    /// 判据：位置更新零拷贝——消息只含坐标与引用，不含图像字节）。
    /// 软件兜底模式下位置更新照常记账（合成器消费同一最新位）。
    pub fn move_to(&mut self, scanout: u32, x: u32, y: u32) -> PosUpdate {
        let pos = PosUpdate {
            scanout,
            x,
            y,
            resource_id: self.image.as_ref().map(|i| i.resource_id).unwrap_or(0),
        };
        if self.pending_pos.is_some() {
            // 单槽覆盖 = 中间帧丢弃（显性计数，不悄悄丢）
            self.dropped_intermediate += 1;
        }
        self.pending_pos = Some(pos);
        self.pos_updates += 1;
        pos
    }

    /// 取走最新位（发送方消费；取走后单槽清空）。
    pub fn take_pending_pos(&mut self) -> Option<PosUpdate> {
        self.pending_pos.take()
    }

    /// 位置消息编码（cursorq wire 形态：8 字 u32——类型码+坐标+资源+热点；
    /// 零拷贝：不含图像字节，热点随行保留）。命令码复用 F0202 的
    /// CMD_UPDATE_CURSOR（单一事实源）。
    pub fn encode_pos_msg(&self, pos: &PosUpdate) -> [u32; 8] {
        let (hx, hy) = self.hotspot();
        [
            CMD_UPDATE_CURSOR,
            pos.scanout,
            pos.x,
            pos.y,
            pos.resource_id,
            hx,
            hy,
            self.mode as u32,
        ]
    }

    /// 图像消息按需编码（仅图像变化时调用方触发；字节数字段供分块搬运）。
    pub fn image_upload_len(&self) -> Option<u32> {
        self.image.as_ref().map(|i| i.data_len)
    }
}

/// F0210 判据自检（域聚合入口）。
pub fn run_veb10_checks() -> crate::checks::CheckSet {
    super::veb10_checks::run_veb10_checks()
}
