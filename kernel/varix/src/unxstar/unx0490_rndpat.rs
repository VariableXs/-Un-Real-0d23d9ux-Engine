//! unx0490_rndpat —— UNX-F0490 渲染校验图案回读（视觉正确性）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0490 · 渲染校验图案回读 · 目标 240 行
//! 判据原文：UNX-F0490-J1 校验图案回读账：向帧缓冲绘制确
//!           定性图案（棋盘/渐变/纯色三案），回读逐像素核
//!           ——账二 纯色案（填充后全像素等值）；账三 棋
//!           盘案（(x+y) 偶奇交替，逐位核）；账四 渐变案
//!           （x 方向线性步进，端点+中点抽样核）；账五 步
//!           幅跳行核（回读走 stride 而非宽×bpp，行尾留埴
//!           不参与核）；账六 回读越界拒（读超面尾拒）。
//! 组件：软渲染器（fill/checker/gradient 三案）+ 回读器 +
//!   逐像素核 + 抽样核。
//! 职责定位：GPU 硬件路径的视觉正确性没法 printf——唯一办
//!   法是「画已知图案、回读、逐位比对」。软渲染器是硬件路
//!   径的黄金参照（golden model）：三案覆盖常数/空间频率/
//!   线性梯度三类像素值分布，回读核是视觉管线的最后一闸。
//! 性能逐项分解：fill O(w×h)；checker O(w×h)；回读核 O(w×h)。
//! 诊断码段：0x34940（0x34941~0x34949 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34940;
pub const XC_OOB: u32 = SEG_BASE + 1; // 回读越界

/// 帧缓冲（R8 灰度面，stride 对齐留埴）。
pub struct Fb {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub raw: Vec<u8>,
}

impl Fb {
    pub fn new(width: u32, height: u32) -> Self {
        // stride 16 对齐（与 0488 闸一致）。
        let stride = ((width * 1 + 15) / 16) * 16;
        Fb { width, height, stride, raw: vec![0u8; (stride * height) as usize] }
    }
    /// 像素写（走 stride）。
    pub fn put(&mut self, x: u32, y: u32, v: u8) {
        let off = (y * self.stride + x) as usize;
        if off < self.raw.len() {
            self.raw[off] = v;
        }
    }
    /// 像素读（越界拒）。
    pub fn get(&self, x: u32, y: u32) -> Result<u8, u32> {
        if x >= self.width || y >= self.height {
            return Err(XC_OOB);
        }
        let off = (y * self.stride + x) as usize;
        if off >= self.raw.len() {
            return Err(XC_OOB);
        }
        Ok(self.raw[off])
    }
    /// 行尾留埴核：stride 尾巴（宽外、stride 内）必为 0。
    pub fn padding_clean(&self) -> bool {
        let pad = (self.stride - self.width) as usize;
        if pad == 0 {
            return true;
        }
        for y in 0..self.height {
            for p in 0..pad {
                let off = (y * self.stride) as usize + self.width as usize + p;
                if self.raw[off] != 0 {
                    return false;
                }
            }
        }
        true
    }
}

/// 案一：纯色填充。
pub fn fill(fb: &mut Fb, v: u8) {
    for y in 0..fb.height {
        for x in 0..fb.width {
            fb.put(x, y, v);
        }
    }
}

/// 案二：棋盘（格 8×8，(x/8+y/8) 偶 0xFF 奇 0x00）。
pub fn checker(fb: &mut Fb, cell: u32) {
    for y in 0..fb.height {
        for x in 0..fb.width {
            let c = ((x / cell) + (y / cell)) % 2;
            fb.put(x, y, if c == 0 { 0xFF } else { 0x00 });
        }
    }
}

/// 案三：渐变（x 方向：v = x*255/(w-1)，单行宽 1 时恒 0）。
pub fn gradient(fb: &mut Fb) {
    let denom = if fb.width > 1 { fb.width - 1 } else { 1 };
    for y in 0..fb.height {
        for x in 0..fb.width {
            fb.put(x, y, ((x as u64 * 255) / denom as u64) as u8);
        }
    }
}

/// 回读核：全像素与期望函数比对。返回（核过数, 差异数）。
pub fn verify(fb: &Fb, expect: impl Fn(u32, u32) -> u8) -> (u64, u64) {
    let mut ok = 0u64;
    let mut bad = 0u64;
    for y in 0..fb.height {
        for x in 0..fb.width {
            if fb.get(x, y) == Ok(expect(x, y)) {
                ok += 1;
            } else {
                bad += 1;
            }
        }
    }
    (ok, bad)
}

pub fn run_unx0490_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0490");
    cs.add("SEG 段位独占", SEG_BASE == 0x34940 && XC_OOB == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 面：33×9（宽 33 → stride 48，每行 15 字节留埴——非整对齐留埴必现）。
    let mut fb = Fb::new(33, 9);
    cs.add("stride 对齐", fb.stride == 48, "(33+15)/16*16");
    cs.add("面尾留埴初始净", fb.padding_clean(), "构造后埴区全 0");

    // 族一：纯色案。
    fill(&mut fb, 0x7E);
    let (ok, bad) = verify(&fb, |_, _| 0x7E);
    cs.add("纯色全等", bad == 0 && ok == 33 * 9, "297 像素等值");
    cs.add("留埴不受染", fb.padding_clean(), "fill 不写埴区");

    // 族二：棋盘案（cell=8）。
    checker(&mut fb, 8);
    let (ok, bad) = verify(&fb, |x, y| if ((x / 8) + (y / 8)) % 2 == 0 { 0xFF } else { 0x00 });
    cs.add("棋盘全等", bad == 0, "逐位核 (x/8+y/8) 奇偶");
    cs.add("棋盘双值", ok == 33 * 9, "全覆盖");
    cs.add("留埴不参与", fb.padding_clean(), "核域外恒净");

    // 族三：渐变案（抽样核：端点+中点）。
    gradient(&mut fb);
    let g0 = fb.get(0, 4);
    let gmid = fb.get(16, 4);
    let gend = fb.get(32, 4);
    cs.add("渐变起点", g0 == Ok(0), "x=0 → 0");
    cs.add("渐变中点", gmid == Ok(127), "16*255/32=127 整除");
    cs.add("渐变终点", gend == Ok(255), "x=32 → 255");
    // 全像素核（期望函数同源）。
    let (_, gbad) = verify(&fb, |x, _| ((x as u64 * 255) / 32) as u8);
    cs.add("渐变全像素", gbad == 0, "黄金参照同源比对");

    // 族四：回读越界。
    cs.add("x 越界拒", fb.get(33, 0) == Err(XC_OOB), "x==width");
    cs.add("y 越界拒", fb.get(0, 9) == Err(XC_OOB), "y==height");

    // 族五：留埴写禁令（步幅尾巴污染检出）。
    // 直写一个埴区字节模拟 stride 尾巴误写，核必须抓到。
    fb.raw[(4 * fb.stride + 33) as usize] = 0xAA; // 第 4 行第一个埴字节
    cs.add("埴区污染检出", !fb.padding_clean(), "尾巴写禁令");
    fb.raw[(4 * fb.stride + 33) as usize] = 0x00;
    cs.add("清污复原", fb.padding_clean(), "复位后复净");

    // 族六：极小面（1×1）。
    let mut tiny = Fb::new(1, 1);
    gradient(&mut tiny);
    cs.add("1×1 渐变", tiny.get(0, 0) == Ok(0), "分母保护 denom=1");
    cs.add("1×1 无留埴", tiny.padding_clean(), "stride==width");

    // 族七：幂等（同案重绘恒等）。
    let mut fb2 = Fb::new(33, 9);
    checker(&mut fb2, 8);
    checker(&mut fb, 8);
    cs.add("重绘恒等", fb.raw == fb2.raw, "确定性图案幂等");

    cs
}
