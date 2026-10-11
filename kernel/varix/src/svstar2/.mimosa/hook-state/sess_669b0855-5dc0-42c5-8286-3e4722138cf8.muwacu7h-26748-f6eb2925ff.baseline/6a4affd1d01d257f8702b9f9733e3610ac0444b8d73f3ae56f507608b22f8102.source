//! VE-F0202 · virtio 控制队列协议封装（VE-B 域 · GPU 驱动矩阵 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0202`
//!
//! **规格原文**：virtio-gpu 的控制面是 controlq 上的命令-响应对。本功能封装
//! 协议层：命令族全实现（RESOURCE_CREATE_2D/3D、RESOURCE_ATTACH/DETACH_BACKING、
//! TRANSFER_TO/FROM_HOST_2D/3D、RESOURCE_FLUSH、SET_SCANOUT、UPDATE_CURSOR、
//! GET_DISPLAY_INFO、EDID 获取等），每命令编码/解码与 virtio-gpu 规范逐字段
//! 对齐。……错误响应（VIRTIO_GPU_RESP_*）全分类映射到 A 域错误码段
//! （F0190 分配的 B 域段）。判据：全命令族 roundtrip 对拍规范、在途配对零
//! 串扰、超时联动丢失状态机、错误映射全覆盖、编码零字节冗余。
//!
//! **设计要点**（本文件 = 协议层）：
//! - wire 布局逐字段对齐规范：24 字节控制头 + 各命令体，小端字节序，
//!   编码长度与规范定义**逐字节相等**（规格判据"编码零字节冗余"——
//!   头部 ring_idx 后的 3 字节 padding 是规范自带的，不算冗余）；
//! - 对拍基准 = 本文件内置的参考设备 `ReferenceDevice`，按规范语义应答
//!   （资源不存在→RESP_ERR_INVALID_RESOURCE_ID、scanout 越界→
//!   RESP_ERR_INVALID_SCANOUT_ID），引擎 roundtrip 与它对拍；
//! - 错误映射全覆盖：每个 RESP_* 常量都有 B 域段错误条目，带三要素
//!   （发生了什么/为什么/下一步）——映射表漏一条即构建期缺陷。

use super::veb01_device::{DisplayCfg, MAX_SCANOUTS};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、命令码与响应码（virtio-gpu 规范定值）
// ---------------------------------------------------------------------------

pub const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const CMD_SET_SCANOUT: u32 = 0x0103;
pub const CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const CMD_GET_CAPSET_INFO: u32 = 0x0108;
pub const CMD_GET_CAPSET: u32 = 0x0109;
pub const CMD_GET_EDID: u32 = 0x010A;
pub const CMD_RESOURCE_ASSIGN_UUID: u32 = 0x010B;
pub const CMD_UPDATE_CURSOR: u32 = 0x010C;
pub const CMD_RESOURCE_CREATE_3D: u32 = 0x0110;
pub const CMD_TRANSFER_TO_HOST_3D: u32 = 0x0111;
pub const CMD_TRANSFER_FROM_HOST_3D: u32 = 0x0112;

pub const RESP_OK_NODATA: u32 = 0x1100;
pub const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const RESP_OK_CAPSET_INFO: u32 = 0x1102;
pub const RESP_OK_CAPSET: u32 = 0x1103;
pub const RESP_OK_EDID: u32 = 0x1104;
pub const RESP_ERR_UNSPEC: u32 = 0x1200;
pub const RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
pub const RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
pub const RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

/// 控制头长度（virtio_gpu_ctrl_hdr：type4+flags4+fence8+ctx4+ring1+pad3）。
pub const CTRL_HDR_LEN: usize = 24;
/// EDID 响应体内 EDID 载荷定长（规范 1024 字节）。
pub const EDID_LEN: usize = 1024;
/// 每命令默认超时（规格点名 2 秒）。
pub const CMD_TIMEOUT_US: u64 = 2_000_000;

// ---------------------------------------------------------------------------
// 二、命令族类型
// ---------------------------------------------------------------------------

/// 2D 矩形（rect 在 wire 上是 4×u32）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// attach_backing 的内存条目（wire：addr u64 + length u32 + pad u32 = 16 字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemEntry {
    pub addr: u64,
    pub length: u32,
}

/// 控制命令全集（规格点名的命令族全实现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CtrlCommand {
    GetDisplayInfo,
    ResourceCreate2d {
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    },
    ResourceCreate3d {
        resource_id: u32,
        target: u32,
        format: u32,
        width: u32,
        height: u32,
        depth: u32,
        array_size: u32,
        last_level: u32,
        nr_samples: u32,
        flags: [u32; 3],
    },
    ResourceUnref {
        resource_id: u32,
    },
    SetScanout {
        scanout_id: u32,
        resource_id: u32,
        rect: Rect,
    },
    ResourceFlush {
        resource_id: u32,
        rect: Rect,
    },
    TransferToHost2d {
        resource_id: u32,
        rect: Rect,
        offset: u64,
    },
    TransferToHost3d {
        resource_id: u32,
        /// 3D box：x/y/z/w/h/d
        box_xyzwhd: [u32; 6],
        offset: u64,
        level: u32,
        stride: u32,
        layer_stride: u32,
    },
    TransferFromHost3d {
        resource_id: u32,
        box_xyzwhd: [u32; 6],
        offset: u64,
        level: u32,
        stride: u32,
        layer_stride: u32,
    },
    ResourceAttachBacking {
        resource_id: u32,
        entries: Vec<MemEntry>,
    },
    ResourceDetachBacking {
        resource_id: u32,
    },
    GetCapsetInfo {
        capset_index: u32,
    },
    GetCapset {
        capset_id: u32,
        version: u32,
    },
    GetEdid {
        scanout: u32,
    },
    UpdateCursor {
        scanout_id: u32,
        x: u32,
        y: u32,
        resource_id: u32,
        hot_x: u32,
        hot_y: u32,
    },
}

impl CtrlCommand {
    /// 命令码。
    pub fn code(&self) -> u32 {
        match self {
            CtrlCommand::GetDisplayInfo => CMD_GET_DISPLAY_INFO,
            CtrlCommand::ResourceCreate2d { .. } => CMD_RESOURCE_CREATE_2D,
            CtrlCommand::ResourceCreate3d { .. } => CMD_RESOURCE_CREATE_3D,
            CtrlCommand::ResourceUnref { .. } => CMD_RESOURCE_UNREF,
            CtrlCommand::SetScanout { .. } => CMD_SET_SCANOUT,
            CtrlCommand::ResourceFlush { .. } => CMD_RESOURCE_FLUSH,
            CtrlCommand::TransferToHost2d { .. } => CMD_TRANSFER_TO_HOST_2D,
            CtrlCommand::TransferToHost3d { .. } => CMD_TRANSFER_TO_HOST_3D,
            CtrlCommand::TransferFromHost3d { .. } => CMD_TRANSFER_FROM_HOST_3D,
            CtrlCommand::ResourceAttachBacking { .. } => CMD_RESOURCE_ATTACH_BACKING,
            CtrlCommand::ResourceDetachBacking { .. } => CMD_RESOURCE_DETACH_BACKING,
            CtrlCommand::GetCapsetInfo { .. } => CMD_GET_CAPSET_INFO,
            CtrlCommand::GetCapset { .. } => CMD_GET_CAPSET,
            CtrlCommand::GetEdid { .. } => CMD_GET_EDID,
            CtrlCommand::UpdateCursor { .. } => CMD_UPDATE_CURSOR,
        }
    }

    /// 人话标签（读屏与日志用）。
    pub fn label(&self) -> &'static str {
        match self {
            CtrlCommand::GetDisplayInfo => "GET_DISPLAY_INFO",
            CtrlCommand::ResourceCreate2d { .. } => "RESOURCE_CREATE_2D",
            CtrlCommand::ResourceCreate3d { .. } => "RESOURCE_CREATE_3D",
            CtrlCommand::ResourceUnref { .. } => "RESOURCE_UNREF",
            CtrlCommand::SetScanout { .. } => "SET_SCANOUT",
            CtrlCommand::ResourceFlush { .. } => "RESOURCE_FLUSH",
            CtrlCommand::TransferToHost2d { .. } => "TRANSFER_TO_HOST_2D",
            CtrlCommand::TransferToHost3d { .. } => "TRANSFER_TO_HOST_3D",
            CtrlCommand::TransferFromHost3d { .. } => "TRANSFER_FROM_HOST_3D",
            CtrlCommand::ResourceAttachBacking { .. } => "RESOURCE_ATTACH_BACKING",
            CtrlCommand::ResourceDetachBacking { .. } => "RESOURCE_DETACH_BACKING",
            CtrlCommand::GetCapsetInfo { .. } => "GET_CAPSET_INFO",
            CtrlCommand::GetCapset { .. } => "GET_CAPSET",
            CtrlCommand::GetEdid { .. } => "GET_EDID",
            CtrlCommand::UpdateCursor { .. } => "UPDATE_CURSOR",
        }
    }

    /// 规范 wire 长度（含 24 字节头）——编码零字节冗余的对拍基准。
    pub fn spec_wire_len(&self) -> usize {
        let body = match self {
            CtrlCommand::GetDisplayInfo => 0,
            CtrlCommand::ResourceCreate2d { .. } => 16,
            CtrlCommand::ResourceCreate3d { .. } => 48,
            CtrlCommand::ResourceUnref { .. } => 8,
            CtrlCommand::SetScanout { .. } => 24,
            CtrlCommand::ResourceFlush { .. } => 24,
            CtrlCommand::TransferToHost2d { .. } => 32,
            CtrlCommand::TransferToHost3d { .. } | CtrlCommand::TransferFromHost3d { .. } => 48,
            CtrlCommand::ResourceAttachBacking { entries, .. } => {
                8 + entries.len() * 16
            }
            CtrlCommand::ResourceDetachBacking { .. } => 8,
            CtrlCommand::GetCapsetInfo { .. } | CtrlCommand::GetCapset { .. } => 8,
            CtrlCommand::GetEdid { .. } => 8,
            CtrlCommand::UpdateCursor { .. } => 32,
        };
        CTRL_HDR_LEN + body
    }
}

// ---------------------------------------------------------------------------
// 三、响应类型
// ---------------------------------------------------------------------------

/// display info 里单个 scanout 的信息（wire 24 字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayOne {
    pub rect: Rect,
    pub enabled: u32,
    pub flags: u32,
}

/// 错误响应码（可判别全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RespErr {
    Unspec,
    OutOfMemory,
    InvalidScanoutId,
    InvalidResourceId,
    InvalidContextId,
    InvalidParameter,
}

impl RespErr {
    pub fn code(self) -> u32 {
        match self {
            RespErr::Unspec => RESP_ERR_UNSPEC,
            RespErr::OutOfMemory => RESP_ERR_OUT_OF_MEMORY,
            RespErr::InvalidScanoutId => RESP_ERR_INVALID_SCANOUT_ID,
            RespErr::InvalidResourceId => RESP_ERR_INVALID_RESOURCE_ID,
            RespErr::InvalidContextId => RESP_ERR_INVALID_CONTEXT_ID,
            RespErr::InvalidParameter => RESP_ERR_INVALID_PARAMETER,
        }
    }

    pub fn from_code(v: u32) -> Option<RespErr> {
        match v {
            RESP_ERR_UNSPEC => Some(RespErr::Unspec),
            RESP_ERR_OUT_OF_MEMORY => Some(RespErr::OutOfMemory),
            RESP_ERR_INVALID_SCANOUT_ID => Some(RespErr::InvalidScanoutId),
            RESP_ERR_INVALID_RESOURCE_ID => Some(RespErr::InvalidResourceId),
            RESP_ERR_INVALID_CONTEXT_ID => Some(RespErr::InvalidContextId),
            RESP_ERR_INVALID_PARAMETER => Some(RespErr::InvalidParameter),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RespErr::Unspec => "ERR_UNSPEC",
            RespErr::OutOfMemory => "ERR_OUT_OF_MEMORY",
            RespErr::InvalidScanoutId => "ERR_INVALID_SCANOUT_ID",
            RespErr::InvalidResourceId => "ERR_INVALID_RESOURCE_ID",
            RespErr::InvalidContextId => "ERR_INVALID_CONTEXT_ID",
            RespErr::InvalidParameter => "ERR_INVALID_PARAMETER",
        }
    }
}

/// 控制响应全集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CtrlResponse {
    OkNoData,
    OkDisplayInfo {
        scanouts: Vec<DisplayOne>,
    },
    OkCapsetInfo {
        capset_id: u32,
        version: u32,
        max_size: u32,
    },
    OkCapset {
        capset_id: u32,
        version: u32,
        payload: Vec<u8>,
    },
    OkEdid {
        size: u32,
        edid: Vec<u8>,
    },
    Err(RespErr),
}

impl CtrlResponse {
    pub fn code(&self) -> u32 {
        match self {
            CtrlResponse::OkNoData => RESP_OK_NODATA,
            CtrlResponse::OkDisplayInfo { .. } => RESP_OK_DISPLAY_INFO,
            CtrlResponse::OkCapsetInfo { .. } => RESP_OK_CAPSET_INFO,
            CtrlResponse::OkCapset { .. } => RESP_OK_CAPSET,
            CtrlResponse::OkEdid { .. } => RESP_OK_EDID,
            CtrlResponse::Err(e) => e.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、编码/解码（逐字段对齐规范）
// ---------------------------------------------------------------------------

/// 解码失败（三要素，异常零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

fn push_hdr(buf: &mut Vec<u8>, cmd_code: u32, fence_id: u64, ctx_id: u32) {
    buf.extend_from_slice(&cmd_code.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags
    buf.extend_from_slice(&fence_id.to_le_bytes());
    buf.extend_from_slice(&ctx_id.to_le_bytes());
    buf.push(0); // ring_idx
    buf.extend_from_slice(&[0u8; 3]); // 规范自带 padding
}

fn push_rect(buf: &mut Vec<u8>, r: &Rect) {
    buf.extend_from_slice(&r.x.to_le_bytes());
    buf.extend_from_slice(&r.y.to_le_bytes());
    buf.extend_from_slice(&r.width.to_le_bytes());
    buf.extend_from_slice(&r.height.to_le_bytes());
}

impl CtrlCommand {
    /// 编码为规范 wire 格式。长度与 `spec_wire_len` 逐字节相等。
    pub fn encode(&self, fence_id: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::with_capacity(self.spec_wire_len());
        push_hdr(&mut b, self.code(), fence_id, 0);
        match self {
            CtrlCommand::GetDisplayInfo => {}
            CtrlCommand::ResourceCreate2d {
                resource_id,
                format,
                width,
                height,
            } => {
                b.extend_from_slice(&format.to_le_bytes());
                b.extend_from_slice(&width.to_le_bytes());
                b.extend_from_slice(&height.to_le_bytes());
                b.extend_from_slice(&resource_id.to_le_bytes());
            }
            CtrlCommand::ResourceCreate3d {
                resource_id,
                target,
                format,
                width,
                height,
                depth,
                array_size,
                last_level,
                nr_samples,
                flags,
            } => {
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&target.to_le_bytes());
                b.extend_from_slice(&format.to_le_bytes());
                b.extend_from_slice(&width.to_le_bytes());
                b.extend_from_slice(&height.to_le_bytes());
                b.extend_from_slice(&depth.to_le_bytes());
                b.extend_from_slice(&array_size.to_le_bytes());
                b.extend_from_slice(&last_level.to_le_bytes());
                b.extend_from_slice(&nr_samples.to_le_bytes());
                for f in flags.iter() {
                    b.extend_from_slice(&f.to_le_bytes());
                }
            }
            CtrlCommand::ResourceUnref { resource_id }
            | CtrlCommand::ResourceDetachBacking { resource_id } => {
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
            }
            CtrlCommand::SetScanout {
                scanout_id,
                resource_id,
                rect,
            } => {
                b.extend_from_slice(&scanout_id.to_le_bytes());
                b.extend_from_slice(&resource_id.to_le_bytes());
                push_rect(&mut b, rect);
            }
            CtrlCommand::ResourceFlush { resource_id, rect } => {
                push_rect(&mut b, rect);
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
            }
            CtrlCommand::TransferToHost2d {
                resource_id,
                rect,
                offset,
            } => {
                push_rect(&mut b, rect);
                b.extend_from_slice(&offset.to_le_bytes());
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
            }
            CtrlCommand::TransferToHost3d {
                resource_id,
                box_xyzwhd,
                offset,
                level,
                stride,
                layer_stride,
            }
            | CtrlCommand::TransferFromHost3d {
                resource_id,
                box_xyzwhd,
                offset,
                level,
                stride,
                layer_stride,
            } => {
                b.extend_from_slice(&resource_id.to_le_bytes());
                for v in box_xyzwhd.iter() {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                b.extend_from_slice(&offset.to_le_bytes());
                b.extend_from_slice(&level.to_le_bytes());
                b.extend_from_slice(&stride.to_le_bytes());
                b.extend_from_slice(&layer_stride.to_le_bytes());
            }
            CtrlCommand::ResourceAttachBacking { resource_id, entries } => {
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                for e in entries.iter() {
                    b.extend_from_slice(&e.addr.to_le_bytes());
                    b.extend_from_slice(&e.length.to_le_bytes());
                    b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
                }
            }
            CtrlCommand::GetCapsetInfo { capset_index } => {
                b.extend_from_slice(&capset_index.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
            }
            CtrlCommand::GetCapset {
                capset_id,
                version,
            } => {
                b.extend_from_slice(&capset_id.to_le_bytes());
                b.extend_from_slice(&version.to_le_bytes());
            }
            CtrlCommand::GetEdid { scanout } => {
                b.extend_from_slice(&scanout.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
            }
            CtrlCommand::UpdateCursor {
                scanout_id,
                x,
                y,
                resource_id,
                hot_x,
                hot_y,
            } => {
                b.extend_from_slice(&scanout_id.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
                b.extend_from_slice(&y.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // pos 规范 padding
                b.extend_from_slice(&resource_id.to_le_bytes());
                b.extend_from_slice(&hot_x.to_le_bytes());
                b.extend_from_slice(&hot_y.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 尾部规范 padding（体 8×u32=32）
            }
        }
        b
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.buf.len() {
            return Err(DecodeError {
                code: "E_DECODE_SHORT",
                what: format!("wire 数据在第 {} 字节处不足 {} 字节", self.pos, n),
                why: "长度与命令类型不符——头部 type 与实际载荷长度不一致".to_string(),
                next: "复核发送方编码；短包直接拒绝不猜字段".to_string(),
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    fn rect(&mut self) -> Result<Rect, DecodeError> {
        let x = self.u32()?;
        let y = self.u32()?;
        let w = self.u32()?;
        let h = self.u32()?;
        Ok(Rect {
            x,
            y,
            width: w,
            height: h,
        })
    }
}

impl CtrlResponse {
    /// 规范 wire 长度。
    pub fn spec_wire_len(&self) -> usize {
        let body = match self {
            CtrlResponse::OkNoData | CtrlResponse::Err(_) => 0,
            CtrlResponse::OkDisplayInfo { scanouts } => 8 + scanouts.len() * 24,
            CtrlResponse::OkCapsetInfo { .. } => 12,
            CtrlResponse::OkCapset { payload, .. } => 8 + payload.len(),
            CtrlResponse::OkEdid { .. } => 16 + EDID_LEN,
        };
        CTRL_HDR_LEN + body
    }

    pub fn encode(&self, fence_id: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::with_capacity(self.spec_wire_len());
        push_hdr(&mut b, self.code(), fence_id, 0);
        match self {
            CtrlResponse::OkNoData | CtrlResponse::Err(_) => {}
            CtrlResponse::OkDisplayInfo { scanouts } => {
                b.extend_from_slice(&(scanouts.len() as u32).to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes()); // 规范 padding
                for s in scanouts.iter() {
                    push_rect(&mut b, &s.rect);
                    b.extend_from_slice(&s.enabled.to_le_bytes());
                    b.extend_from_slice(&s.flags.to_le_bytes());
                }
            }
            CtrlResponse::OkCapsetInfo {
                capset_id,
                version,
                max_size,
            } => {
                b.extend_from_slice(&capset_id.to_le_bytes());
                b.extend_from_slice(&version.to_le_bytes());
                b.extend_from_slice(&max_size.to_le_bytes());
            }
            CtrlResponse::OkCapset {
                capset_id,
                version,
                payload,
            } => {
                b.extend_from_slice(&capset_id.to_le_bytes());
                b.extend_from_slice(&version.to_le_bytes());
                b.extend_from_slice(payload);
            }
            CtrlResponse::OkEdid { size, edid } => {
                b.extend_from_slice(&size.to_le_bytes());
                b.extend_from_slice(&[0u8; 12]); // 规范 padding[3]
                let mut e = edid.clone();
                e.resize(EDID_LEN, 0);
                b.extend_from_slice(&e);
            }
        }
        b
    }

    /// 解码（fence_id 原样带回，供在途配对）。
    pub fn decode(wire: &[u8]) -> Result<(u64, CtrlResponse), DecodeError> {
        if wire.len() < CTRL_HDR_LEN {
            return Err(DecodeError {
                code: "E_DECODE_SHORT",
                what: format!("响应 wire 长度 {} 不足头部 24", wire.len()),
                why: "连控制头都不完整，无从解析".to_string(),
                next: "复核设备应答通路；短包直接拒绝".to_string(),
            });
        }
        let mut r = Reader { buf: wire, pos: 0 };
        let type_ = r.u32()?;
        let _flags = r.u32()?;
        let fence_id = r.u64()?;
        let _ctx = r.u32()?;
        let _ring = r.take(4)?;
        let resp = match type_ {
            RESP_OK_NODATA => CtrlResponse::OkNoData,
            RESP_OK_DISPLAY_INFO => {
                let n = r.u32()?;
                let _pad = r.u32()?;
                let mut scanouts: Vec<DisplayOne> = Vec::new();
                for _ in 0..n {
                    let rect = r.rect()?;
                    let enabled = r.u32()?;
                    let flags = r.u32()?;
                    scanouts.push(DisplayOne {
                        rect,
                        enabled,
                        flags,
                    });
                }
                CtrlResponse::OkDisplayInfo { scanouts }
            }
            RESP_OK_CAPSET_INFO => {
                let capset_id = r.u32()?;
                let version = r.u32()?;
                let max_size = r.u32()?;
                CtrlResponse::OkCapsetInfo {
                    capset_id,
                    version,
                    max_size,
                }
            }
            RESP_OK_CAPSET => {
                let capset_id = r.u32()?;
                let version = r.u32()?;
                let payload = wire[r.pos..].to_vec();
                CtrlResponse::OkCapset {
                    capset_id,
                    version,
                    payload,
                }
            }
            RESP_OK_EDID => {
                let size = r.u32()?;
                let _pad = r.take(12)?;
                let edid = wire[r.pos..].to_vec();
                CtrlResponse::OkEdid { size, edid }
            }
            v if RespErr::from_code(v).is_some() => CtrlResponse::Err(RespErr::from_code(v).unwrap()),
            _ => {
                return Err(DecodeError {
                    code: "E_DECODE_TYPE",
                    what: format!("响应 type 0x{:04X} 不在响应码全集", type_),
                    why: "未知响应码可能是设备行为越出规范，也可能是通路错位".to_string(),
                    next: "按设备可疑处置并联动丢失状态机（F0009）".to_string(),
                });
            }
        };
        Ok((fence_id, resp))
    }
}

// ---------------------------------------------------------------------------
// 五、B 域段错误映射（判据：错误映射全覆盖）
// ---------------------------------------------------------------------------

/// B 域段错误条目（F0190 分配的 B 域错误码段 + 三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BSegError {
    /// B 域段错误码（VEB-B 前缀 + RESP 码位）
    pub seg_code: &'static str,
    pub resp: RespErr,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl BSegError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// RESP_* → B 域段错误全映射。映射表必须覆盖 RespErr 全集
/// （自检逐条核对，漏一条即构建期缺陷）。
pub fn map_resp_err(e: RespErr) -> BSegError {
    match e {
        RespErr::Unspec => BSegError {
            seg_code: "VEB-E-1200",
            resp: e,
            what: "设备返回未指定错误（RESP_ERR_UNSPEC）".to_string(),
            why: "设备没有给出可归因的错误类别，常见于设备内部状态异常".to_string(),
            next: "按设备可疑处置并联动丢失状态机（F0009）；连续出现则走驱动重置"
                .to_string(),
        },
        RespErr::OutOfMemory => BSegError {
            seg_code: "VEB-E-1201",
            resp: e,
            what: "设备资源耗尽（RESP_ERR_OUT_OF_MEMORY）".to_string(),
            why: "宿主侧显存/内存配额不足，资源创建或转移被拒".to_string(),
            next: "先削减在途资源再重试；持续出现提示用户提高虚拟机显存配额"
                .to_string(),
        },
        RespErr::InvalidScanoutId => BSegError {
            seg_code: "VEB-E-1202",
            resp: e,
            what: "扫描出口号越界（RESP_ERR_INVALID_SCANOUT_ID）".to_string(),
            why: "scanout_id 超出设备报告的出口数——驱动侧按出口数钳制即可根治"
                .to_string(),
            next: "以 GET_DISPLAY_INFO 返回的出口数重排 scanout 绑定".to_string(),
        },
        RespErr::InvalidResourceId => BSegError {
            seg_code: "VEB-E-1203",
            resp: e,
            what: "资源号不存在（RESP_ERR_INVALID_RESOURCE_ID）".to_string(),
            why: "引用了已 UNREF 或从未创建的资源——驱动侧句柄表应为唯一事实源"
                .to_string(),
            next: "对账驱动侧句柄表与设备侧资源集，修复泄漏后再重放该命令"
                .to_string(),
        },
        RespErr::InvalidContextId => BSegError {
            seg_code: "VEB-E-1204",
            resp: e,
            what: "上下文号不存在（RESP_ERR_INVALID_CONTEXT_ID）".to_string(),
            why: "命令携带的 ctx_id 未在设备侧登记（3D 上下文先行创建）".to_string(),
            next: "确认 CONTEXT_INIT 特性已协商且上下文创建命令先于使用".to_string(),
        },
        RespErr::InvalidParameter => BSegError {
            seg_code: "VEB-E-1205",
            resp: e,
            what: "命令参数非法（RESP_ERR_INVALID_PARAMETER）".to_string(),
            why: "字段值越出规范允许域（如尺寸 0、非对齐 offset）——编码侧缺陷"
                .to_string(),
            next: "核对命令编码与规范字段域；这是驱动缺陷不是设备问题".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// 六、参考设备（对拍基准）
// ---------------------------------------------------------------------------

/// 资源记录（参考设备的资源表条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefResource {
    pub id: u32,
    pub is_3d: bool,
    pub width: u32,
    pub height: u32,
    pub backing: u32,
}

/// 参考设备：按规范语义应答的"标准答案"。
///
/// 判据"全命令族 roundtrip 对拍规范"的对拍锚——引擎侧行为与它不一致
/// 即驱动缺陷。零 IO、确定性。
pub struct ReferenceDevice {
    pub display: DisplayCfg,
    resources: Vec<RefResource>,
    /// scanout → (resource_id, rect)
    scanouts: Vec<Option<(u32, Rect)>>,
    capsets: Vec<(u32, u32, u32)>,
    edid_bytes: [u8; EDID_LEN],
    pub handled: Vec<u32>,
}

impl ReferenceDevice {
    pub fn new(display: DisplayCfg) -> ReferenceDevice {
        let n = display.scanouts.min(MAX_SCANOUTS) as usize;
        ReferenceDevice {
            display,
            resources: Vec::new(),
            scanouts: {
                let mut v: Vec<Option<(u32, Rect)>> = Vec::new();
                for _ in 0..n {
                    v.push(None);
                }
                v
            },
            capsets: vec![(0, 1, 4096)],
            edid_bytes: [0u8; EDID_LEN],
            handled: Vec::new(),
        }
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    fn find_resource(&self, id: u32) -> Option<&RefResource> {
        self.resources.iter().find(|r| r.id == id)
    }

    /// 处理一条控制命令，按规范语义给出响应。
    pub fn handle(&mut self, cmd: &CtrlCommand) -> CtrlResponse {
        self.handled.push(cmd.code());
        match cmd {
            CtrlCommand::GetDisplayInfo => {
                let mut v: Vec<DisplayOne> = Vec::new();
                for (i, s) in self.scanouts.iter().enumerate() {
                    let rect = s.map(|(_, r)| r).unwrap_or(Rect {
                        x: 0,
                        y: 0,
                        width: self.display.max_width,
                        height: self.display.max_height,
                    });
                    v.push(DisplayOne {
                        rect,
                        enabled: if s.is_some() { 1 } else { 0 },
                        flags: i as u32,
                    });
                }
                CtrlResponse::OkDisplayInfo { scanouts: v }
            }
            CtrlCommand::ResourceCreate2d {
                resource_id,
                format,
                width,
                height,
            } => {
                if *width == 0 || *height == 0 {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                if self.find_resource(*resource_id).is_some() {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                self.resources.push(RefResource {
                    id: *resource_id,
                    is_3d: false,
                    width: *width,
                    height: *height,
                    backing: 0,
                });
                let _ = format;
                CtrlResponse::OkNoData
            }
            CtrlCommand::ResourceCreate3d {
                resource_id,
                width,
                height,
                ..
            } => {
                if *width == 0 || *height == 0 {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                if self.find_resource(*resource_id).is_some() {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                self.resources.push(RefResource {
                    id: *resource_id,
                    is_3d: true,
                    width: *width,
                    height: *height,
                    backing: 0,
                });
                CtrlResponse::OkNoData
            }
            CtrlCommand::ResourceUnref { resource_id } => {
                match self.resources.iter().position(|r| r.id == *resource_id) {
                    Some(i) => {
                        self.resources.remove(i);
                        // 规范语义：UNREF 同时解除 scanout 绑定
                        for s in self.scanouts.iter_mut() {
                            if s.map(|(id, _)| id) == Some(*resource_id) {
                                *s = None;
                            }
                        }
                        CtrlResponse::OkNoData
                    }
                    None => CtrlResponse::Err(RespErr::InvalidResourceId),
                }
            }
            CtrlCommand::SetScanout {
                scanout_id,
                resource_id,
                rect,
            } => {
                if *scanout_id as usize >= self.scanouts.len() {
                    return CtrlResponse::Err(RespErr::InvalidScanoutId);
                }
                if *resource_id != 0 && self.find_resource(*resource_id).is_none() {
                    return CtrlResponse::Err(RespErr::InvalidResourceId);
                }
                self.scanouts[*scanout_id as usize] = Some((*resource_id, *rect));
                CtrlResponse::OkNoData
            }
            CtrlCommand::ResourceFlush { resource_id, .. } => {
                if self.find_resource(*resource_id).is_none() {
                    return CtrlResponse::Err(RespErr::InvalidResourceId);
                }
                CtrlResponse::OkNoData
            }
            CtrlCommand::TransferToHost2d { resource_id, .. }
            | CtrlCommand::TransferToHost3d { resource_id, .. }
            | CtrlCommand::TransferFromHost3d { resource_id, .. } => {
                if self.find_resource(*resource_id).is_none() {
                    return CtrlResponse::Err(RespErr::InvalidResourceId);
                }
                CtrlResponse::OkNoData
            }
            CtrlCommand::ResourceAttachBacking { resource_id, entries } => {
                match self.find_resource(*resource_id) {
                    None => CtrlResponse::Err(RespErr::InvalidResourceId),
                    Some(_) => {
                        // 规范语义：重复 attach 是参数错误（先 detach 再 attach）
                        if self
                            .find_resource(*resource_id)
                            .map(|r| r.backing > 0)
                            .unwrap_or(false)
                        {
                            return CtrlResponse::Err(RespErr::InvalidParameter);
                        }
                        let n = entries.len() as u32;
                        if let Some(r) = self.resources.iter_mut().find(|r| r.id == *resource_id) {
                            r.backing = n;
                        }
                        CtrlResponse::OkNoData
                    }
                }
            }
            CtrlCommand::ResourceDetachBacking { resource_id } => {
                match self.find_resource(*resource_id) {
                    None => CtrlResponse::Err(RespErr::InvalidResourceId),
                    Some(_) => {
                        if let Some(r) = self.resources.iter_mut().find(|r| r.id == *resource_id) {
                            r.backing = 0;
                        }
                        CtrlResponse::OkNoData
                    }
                }
            }
            CtrlCommand::GetCapsetInfo { capset_index } => {
                if (*capset_index as usize) >= self.capsets.len() {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                let (id, ver, size) = self.capsets[*capset_index as usize];
                CtrlResponse::OkCapsetInfo {
                    capset_id: id,
                    version: ver,
                    max_size: size,
                }
            }
            CtrlCommand::GetCapset { capset_id, version } => {
                if !self
                    .capsets
                    .iter()
                    .any(|c| c.0 == *capset_id && c.1 == *version)
                {
                    return CtrlResponse::Err(RespErr::InvalidParameter);
                }
                CtrlResponse::OkCapset {
                    capset_id: *capset_id,
                    version: *version,
                    payload: vec![0xAB; 16],
                }
            }
            CtrlCommand::GetEdid { scanout } => {
                if *scanout as usize >= self.scanouts.len() {
                    return CtrlResponse::Err(RespErr::InvalidScanoutId);
                }
                CtrlResponse::OkEdid {
                    size: 128,
                    edid: self.edid_bytes[..128].to_vec(),
                }
            }
            CtrlCommand::UpdateCursor {
                scanout_id,
                resource_id,
                ..
            } => {
                if *scanout_id as usize >= self.scanouts.len() {
                    return CtrlResponse::Err(RespErr::InvalidScanoutId);
                }
                if *resource_id != 0 && self.find_resource(*resource_id).is_none() {
                    return CtrlResponse::Err(RespErr::InvalidResourceId);
                }
                CtrlResponse::OkNoData
            }
        }
    }
}
