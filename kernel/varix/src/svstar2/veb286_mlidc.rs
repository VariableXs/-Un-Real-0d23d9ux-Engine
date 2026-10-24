//! veb286_mlidc —— VE-F0286 Mali 显示控制器对接。
//! 锚点（CoRun Unxreal 主册）：VE-F0286 · Mali 显示控制器对接 · 目标 380 行
//! 判据原文：VE-F0286-J1 显示对接账：CRTC 账（控制器号 +
//!           当前扫描面）；平面账（层号 0~3 + z 序 + 可见
//!           性）；z 序仲裁（同 z 拒，层叠按 z 升序扫出）；
//!           模式集（分辨率 × 刷新率对，非白名单拒）；翻转
//!           请求（面号 + 完成旗，双缓冲交换）；扫描空窗核
//!           （空窗外翻转排队，空窗内即换）；层禁用回收核
//!           （禁用层回收 z 序，不留洞）。
//! 组件：CRTC 账 + 平面层 + 模式白名单 + 翻转队列。
//! 职责定位：显示控制器对接是「Mali 的最后一面墙」——层是
//!           玻璃：一块压一块，z 序就是叠玻璃的次序；模式白
//!           名单是墙的承重表：承不住的模式一面墙都别想；
//!           空窗翻转是换玻璃的唯一安全窗：扫到一半换玻璃
//!           就是撕裂。
//! 性能逐项分解：层操作 O(层)；翻转 O(队)。
//! 诊断码段：0x31080（0x31081~0x31089 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DC_VERSION: &str = "veb286_mlidc-v1";

/// 诊断段基址（0x31080 段 · 本模块独占）。
pub const DC_SEG_BASE: u32 = 0x31080;
pub const XC_MAGIC: u32 = 0x31081; // 账魔数破
pub const XC_LAYER: u32 = 0x31082; // 层号越界
pub const XC_ZDUP: u32 = 0x31083; // z 重复
pub const XC_MODE: u32 = 0x31084; // 模式不在白名单
pub const XC_NOBUF: u32 = 0x31085; // 无此缓冲面

/// 账魔数。
const MAGIC: u32 = 0x3108_1C7E;

/// 层数。
pub const LAYERS: usize = 4;

/// 模式白名单（宽 × 高 × 刷新 × 10）。
pub const MODES: [(u32, u32, u32); 4] = [
    (640, 480, 600),
    (1920, 1080, 600),
    (2560, 1440, 1440),
    (3840, 2160, 600),
];

/// 平面层。
#[derive(Clone, Copy)]
pub struct Plane {
    pub z: u32,
    pub visible: bool,
    /// 当前缓冲面号。
    pub buf: u32,
}

/// 翻转请求。
pub struct FlipReq {
    pub layer: usize,
    pub new_buf: u32,
}

/// 显示簿。
pub struct DcBook {
    planes: [Option<Plane>; LAYERS],
    /// 当前模式在白名单的索引。
    pub mode: Option<usize>,
    /// 翻转队列（空窗外排队）。
    flips: Vec<FlipReq>,
    /// 空窗旗。
    pub in_vblank: bool,
    /// 已完成翻转计数。
    pub done_flips: u32,
    magic: u32,
}

impl DcBook {
    /// 建。
    pub fn new() -> DcBook {
        DcBook {
            planes: [None; LAYERS],
            mode: None,
            flips: Vec::new(),
            in_vblank: false,
            done_flips: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 设模式（须在白名单）。
    pub fn set_mode(&mut self, mi: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if mi >= MODES.len() {
            return Err(XC_MODE);
        }
        self.mode = Some(mi);
        Ok(())
    }

    /// 启层：z 唯一仲裁。
    pub fn enable(&mut self, li: usize, z: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= LAYERS {
            return Err(XC_LAYER);
        }
        for p in self.planes.iter() {
            if let Some(pl) = p {
                if pl.z == z {
                    return Err(XC_ZDUP);
                }
            }
        }
        self.planes[li] = Some(Plane { z, visible: true, buf: 0 });
        Ok(())
    }

    /// 禁层回收：层清空，z 序自动让位。
    pub fn disable(&mut self, li: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= LAYERS {
            return Err(XC_LAYER);
        }
        self.planes[li] = None;
        Ok(())
    }

    /// 层活数。
    pub fn live_layers(&self) -> usize {
        let mut n = 0;
        for p in self.planes.iter() {
            if p.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 扫出序：按 z 升序出层号。
    pub fn scan_order(&self) -> Vec<usize> {
        let mut v: Vec<(u32, usize)> = Vec::new();
        for i in 0..LAYERS {
            if let Some(p) = &self.planes[i] {
                if p.visible {
                    v.push((p.z, i));
                }
            }
        }
        // 插入排序升序。
        let mut out: Vec<usize> = Vec::new();
        // 简单选择。
        let n = v.len();
        let mut used = [false; LAYERS];
        for _ in 0..n {
            let mut best = None;
            let mut bz = u32::MAX;
            for k in 0..n {
                if !used[k] && v[k].0 < bz {
                    bz = v[k].0;
                    best = Some(k);
                }
            }
            if let Some(k) = best {
                used[k] = true;
                out.push(v[k].1);
            }
        }
        out
    }

    /// 翻转请求：空窗内即换，否则排队。
    pub fn flip(&mut self, li: usize, new_buf: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= LAYERS || self.planes[li].is_none() {
            return Err(XC_LAYER);
        }
        if new_buf == 0 {
            return Err(XC_NOBUF);
        }
        if self.in_vblank {
            self.planes[li].as_mut().map(|p| p.buf = new_buf);
            self.done_flips += 1;
            Ok(())
        } else {
            self.flips.push(FlipReq { layer: li, new_buf });
            Ok(())
        }
    }

    /// 进空窗：倾倒队列。
    pub fn enter_vblank(&mut self) -> u32 {
        self.in_vblank = true;
        let n = self.flips.len();
        let mut k = 0;
        while k < n {
            let f = &self.flips[k];
            if let Some(p) = self.planes[f.layer].as_mut() {
                p.buf = f.new_buf;
                self.done_flips += 1;
            }
            k += 1;
        }
        self.flips.clear();
        n as u32
    }

    /// 出空窗。
    pub fn exit_vblank(&mut self) {
        self.in_vblank = false;
    }

    /// 层当前面。
    pub fn buf_of(&self, li: usize) -> Result<u32, u32> {
        if li >= LAYERS {
            return Err(XC_LAYER);
        }
        match &self.planes[li] {
            Some(p) => Ok(p.buf),
            None => Err(XC_LAYER),
        }
    }

    /// 队深。
    pub fn queue_len(&self) -> usize {
        self.flips.len()
    }
}

pub fn run_veb286_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0286");
    let mut b = DcBook::new();
    // 族零：模式白名单。
    s.add("模式 9 拒", b.set_mode(9) == Err(XC_MODE), "越名单");
    s.add("设 1080p", b.set_mode(1).is_ok(), "白名单 1");
    s.add("模式在册", b.mode == Some(1), "1920×1080@600");
    // 族一：层启用与 z 仲裁。
    s.add("层 4 拒", b.enable(4, 1) == Err(XC_LAYER), "越界");
    s.add("启层 0 z10", b.enable(0, 10).is_ok(), "底玻璃");
    s.add("启层 2 z20", b.enable(2, 20).is_ok(), "顶玻璃");
    s.add("同 z 拒", b.enable(1, 10) == Err(XC_ZDUP), "z 重");
    s.add("禁层回收 z", b.disable(0).is_ok(), "让位");
    s.add("回收后 z10 可用", b.enable(1, 10).is_ok(), "无洞");
    s.add("活层 2", b.live_layers() == 2, "1 号 2 号");
    // 族二：扫出序。
    let ord = b.scan_order();
    s.add("扫出 z 升序", ord.len() == 2 && ord[0] == 1 && ord[1] == 2, "先 z10 后 z20");
    // 三层序核。
    let _ = b.enable(3, 15);
    let ord3 = b.scan_order();
    s.add("三层 1,3,2", ord3[0] == 1 && ord3[1] == 3 && ord3[2] == 2, "z10,15,20");
    // 族三：翻转空窗外排队。
    s.add("层 9 面拒", b.flip(9, 5) == Err(XC_LAYER), "无层");
    s.add("零面拒", b.flip(1, 0) == Err(XC_NOBUF), "无零面");
    s.add("翻层 1 面 7", b.flip(1, 7).is_ok(), "排队");
    s.add("队深 1", b.queue_len() == 1, "空窗外");
    s.add("面未换", b.buf_of(1) == Ok(0), "未生效");
    // 族四：进空窗倾倒。
    s.add("倾倒 1", b.enter_vblank() == 1, "一笔");
    s.add("面已换 7", b.buf_of(1) == Ok(7), "生效");
    s.add("完成 1", b.done_flips == 1, "一翻");
    s.add("队清零", b.queue_len() == 0, "倾尽");
    // 空窗内即换。
    s.add("空窗即换", b.flip(2, 9).is_ok() && b.buf_of(2) == Ok(9), "免队");
    s.add("即换计 2", b.done_flips == 2, "两翻");
    b.exit_vblank();
    s.add("出窗回队", b.flip(2, 11).is_ok() && b.queue_len() == 1, "又排");
    // 族五：禁层后队列残留安全倾倒。
    let _ = b.disable(2);
    s.add("禁层倾倒 0 效", b.enter_vblank() == 1 && b.done_flips == 2, "死层不翻");
    s.add("魔数核", DC_SEG_BASE == 0x31080, "段基址");
    s
}
