//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2 码流层+L3 解码层 · 目标 520 行）
//! 锚点：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1204`
//! 本文件是 F1204 的**权威实现外壳**：解码器按职责拆为九个子模块（诊断与算术/NAL 与位读取器/
//! 参数集/切片头/帧内预测/运动补偿与重建/DPB 与计数/熵解码与上下文/fuzz 语料），本文件只做再导出，
//! 保持 `svstar2::veg04_h264::*` 的对外路径与语义完全不变；判据正文见子模块文件头。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

mod core;
mod nal;
mod ps;
mod sh;
mod pix;
mod rec;
mod dpb;
mod ent;
mod fz;

pub use core::*;
pub use nal::*;
pub use ps::*;
pub use sh::*;
pub use pix::*;
pub use rec::*;
pub use dpb::*;
pub use ent::*;
pub use fz::*;

