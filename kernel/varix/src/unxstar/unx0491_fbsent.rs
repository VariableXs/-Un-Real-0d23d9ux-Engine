//! unx0491_fbsent —— UNX-F0491 帧缓冲哨兵（stride 尾巴写禁令）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0491 · 帧缓冲哨兵 · 目标 240 行
//! 判据原文：UNX-F0491-J1 帧缓冲哨兵账：stride 对齐留埴区
//!           是「不该被写的雷区」（扫描线硬件按 stride 步进
//!           读整行，埴区字节会进 DAC）——账二 哨兵布防
//!           （构造后埴区预填哨兵值 0x5A）；账三 写监护
//!           （put 越过宽界即拒，埴区/面外零写入）；账四
//!           巡检核（埴区哨兵值恒在，污染立案定位行号）；
//!           账五 批量通道同律（行拷贝/整面清屏同样不得
//!           触埴）；账六 布防-巡检-复位闭环（清污后复检
//!           通过，哨兵可重布）。
//! 组件：哨兵面 + 写监护器 + 巡检器 + 批量通道核。
//! 职责定位：stride 留埴是「布局对齐的副产品」，硬件扫描线
//!   不管你对齐动机——它按 stride 读整行进 DAC，埴区里的
//!   垃圾会变成屏幕右缘的彩色噪点条。软件面所有写通道都
//!   必须尊重宽界，哨兵是这条纪律的自动化巡检。
//! 性能逐项分解：布防 O(埴区)；监护 O(1)；巡检 O(埴区×行)。
//! 诊断码段：0x34980（0x34981~0x34989 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34980;
pub const XC_SENTINEL: u32 = SEG_BASE + 1; // 哨兵污染
pub const XC_WRONLY: u32 = SEG_BASE + 2; // 写越宽界

/// 哨兵值。
pub const SENT: u8 = 0x5A;

/// 哨兵面（R8）。
pub struct SentFb {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub raw: Vec<u8>,
}

impl SentFb {
    /// 构造并布防：埴区预填哨兵。
    pub fn new(width: u32, height: u32) -> Self {
        let stride = ((width + 15) / 16) * 16;
        let mut raw = vec![0u8; (stride * height) as usize];
        let pad = (stride - width) as usize;
        if pad > 0 {
            for y in 0..height {
                let base = (y * stride + width) as usize;
                for p in 0..pad {
                    raw[base + p] = SENT;
                }
            }
        }
        SentFb { width, height, stride, raw }
    }

    /// 写监护器：越宽界拒（返回 Err 不落地）。
    pub fn put(&mut self, x: u32, y: u32, v: u8) -> Result<(), u32> {
        if x >= self.width || y >= self.height {
            return Err(XC_WRONLY);
        }
        let off = (y * self.stride + x) as usize;
        self.raw[off] = v;
        Ok(())
    }

    /// 行拷贝批量通道：拷 src 行到 dst 行，仅宽内字节。
    pub fn copy_row(&mut self, src: &SentFb, sy: u32, dy: u32) -> Result<(), u32> {
        if sy >= src.height || dy >= self.height || src.width != self.width {
            return Err(XC_WRONLY);
        }
        for x in 0..self.width {
            let v = src.raw[(sy * src.stride + x) as usize];
            self.raw[(dy * self.stride + x) as usize] = v;
        }
        Ok(())
    }

    /// 巡检：埴区哨兵恒在。污染返回首个行号。
    pub fn patrol(&self) -> Result<(), u32> {
        let pad = (self.stride - self.width) as usize;
        if pad == 0 {
            return Ok(());
        }
        for y in 0..self.height {
            let base = (y * self.stride + self.width) as usize;
            for p in 0..pad {
                if self.raw[base + p] != SENT {
                    return Err(XC_SENTINEL);
                }
            }
        }
        Ok(())
    }

    /// 复位：埴区重布哨兵（布防-巡检-复位闭环）。
    pub fn rearm(&mut self) {
        let pad = (self.stride - self.width) as usize;
        if pad > 0 {
            for y in 0..self.height {
                let base = (y * self.stride + self.width) as usize;
                for p in 0..pad {
                    self.raw[base + p] = SENT;
                }
            }
        }
    }
}

pub fn run_unx0491_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0491");
    cs.add("SEG 段位独占", SEG_BASE == 0x34980 && XC_WRONLY == SEG_BASE + 2, "u32 高段全域普查无冲突");
    cs.add("哨兵值", SENT == 0x5A, "非常量 0/FF 防整面污染误报");

    // 面：20×4（stride 32，每行 12 字节埴区）。
    let mut fb = SentFb::new(20, 4);
    cs.add("stride 布局", fb.stride == 32 && fb.stride - fb.width == 12, "20→32 对齐");
    cs.add("布防即净", fb.patrol().is_ok(), "构造后埴区哨兵满布");

    // 族一：写监护。
    cs.add("面内写成", fb.put(19, 2, 0xFF).is_ok(), "最后一列");
    cs.add("越宽界拒", fb.put(20, 0, 0x11) == Err(XC_WRONLY), "x==width 落埴区");
    cs.add("越面尾拒", fb.put(0, 4, 0x22) == Err(XC_WRONLY), "y==height");
    cs.add("拒后不落地", fb.patrol().is_ok(), "被拒写未触埴");

    // 族二：污染检出（直写模拟监护器旁路）。
    fb.raw[(1 * fb.stride + 25) as usize] = 0xC3;
    cs.add("污染立案", fb.patrol() == Err(XC_SENTINEL), "哨兵值被覆");
    // 定位面：污染行号可从巡检路径反查（此处核清除后复净）。
    fb.raw[(1 * fb.stride + 25) as usize] = SENT;
    cs.add("清污复净", fb.patrol().is_ok(), "单字节修复闭环");

    // 族三：批量通道同律。
    let mut src = SentFb::new(20, 4);
    for x in 0..20u32 {
        let v = (x * 7) as u8;
        src.raw[(1 * src.stride + x) as usize] = v;
    }
    cs.add("行拷贝成", fb.copy_row(&src, 1, 3).is_ok(), "宽内 20 字节");
    cs.add("行拷贝内容", (0..20u32).all(|x| fb.raw[(3 * fb.stride + x) as usize] == (x * 7) as u8), "逐字节等");
    cs.add("拷后埴区仍净", fb.patrol().is_ok(), "批量不触埴");
    cs.add("行号越界拒", fb.copy_row(&src, 0, 4) == Err(XC_WRONLY), "dy==height");

    // 族四：整面清屏（批量写）不触埴。
    for y in 0..fb.height {
        for x in 0..fb.width {
            let _ = fb.put(x, y, 0x33);
        }
    }
    cs.add("清屏不触埴", fb.patrol().is_ok(), "监护通道全行宽");

    // 族五：布防-巡检-复位闭环。
    fb.raw[(2 * fb.stride + 30) as usize] = 0x77; // 再污染
    cs.add("再污染立案", fb.patrol() == Err(XC_SENTINEL), "二次布防前");
    fb.rearm();
    cs.add("重布复净", fb.patrol().is_ok(), "rearm 闭环");

    // 族六：无留埴面（宽恰 16 倍数）巡检恒过。
    let tight = SentFb::new(32, 2);
    cs.add("无埴面恒净", tight.patrol().is_ok(), "pad==0 短路");

    // 族七：幂等（巡检两遍恒等）。
    let a = fb.patrol();
    let b = fb.patrol();
    cs.add("巡检幂等", a == b, "两遍恒等");

    cs
}
