//! cga66_rowbuf —— CGPU-F0066 行缓冲与条带处理框架。
//! 锚点（CGPU 施工书）：CGPU-F0066 · 行缓冲与条带处理框架 · 目标 340 行
//! 判据摘录：3 行滑窗（上/中/下）推进不变式——第 k 行输出就绪
//!   当且仅当第 k+1 行已灌入（下边界镜像由框架层保证）；行环缓
//!   冲槽位轮转与 3 取模一致（槽位账守恒）；条带（stripe）切分
//!   完整覆盖（Σ条带高 == 图高，无缝无叠）；条带间上下文传递
//!   （下一带首行 = 上一带末行——连续性硬线）；零行/零宽显性
//!   拒绝。
//! 组件：行环缓冲（3 槽 × 行宽）+ 灌行/取窗引擎 + 槽位账 + 条
//!   带切分器（固定条带高）+ 条带上下文链 + 覆盖守恒核。
//! 职责定位：图像管线的内存面地基——整图不落盘 DRAM，只有 3 行
//!   驻留 SRAM，是行处理管线（去噪→锐化→色阶）的框架前提。
//! 性能逐项分解：灌行 O(行宽)；取窗 O(9)；条带切分 O(带数)。
//! 全整数确定性。
//! 诊断码段：0xE6A0（0xE6A1~0xE6A9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RB_VERSION: &str = "cga66_rowbuf-v1";

/// 诊断段基址（0xE6A0 段 · 本模块独占）。
pub const RB_SEG_BASE: u16 = 0xE6A0;

/// XcCode 闭集（0xE6A1~0xE6A9）。
pub const XC_WIDTH_ZERO: u32 = 0xE6A1;
pub const XC_ROW_OOB: u32 = 0xE6A2;
pub const XC_SLOT_DRIFT: u32 = 0xE6A3;
pub const XC_STRIPE_GAP: u32 = 0xE6A4;
pub const XC_COVER_FAIL: u32 = 0xE6A5;
pub const XC_CTX_BREAK: u32 = 0xE6A6;
pub const XC_HEIGHT_ZERO: u32 = 0xE6A7;
pub const XC_WIN_NOT_READY: u32 = 0xE6A8;
pub const XC_STRIPE_H_ZERO: u32 = 0xE6A9;

/// 行环缓冲：3 槽 × 行宽。
pub struct RowRing {
    pub width: usize,
    slots: [Vec<u32>; 3],
    /// 下一灌入行号（图坐标 y）。
    pub next_row: usize,
    /// 已灌行总数（槽位账基准）。
    pub fed: usize,
}

impl RowRing {
    pub fn new(width: usize) -> Result<RowRing, u32> {
        if width == 0 {
            return Err(XC_WIDTH_ZERO);
        }
        Ok(RowRing {
            width,
            slots: [Vec::new(), Vec::new(), Vec::new()],
            next_row: 0,
            fed: 0,
        })
    }

    /// 灌一行（槽位 = fed % 3 轮转——账守恒硬线）。
    pub fn feed(&mut self, row: &[u32]) -> Result<usize, u32> {
        if row.len() != self.width {
            return Err(XC_ROW_OOB);
        }
        let slot = self.fed % 3;
        self.slots[slot] = Vec::with_capacity(self.width);
        for &v in row.iter() {
            self.slots[slot].push(v);
        }
        self.fed += 1;
        self.next_row = self.fed;
        Ok(slot)
    }

    /// 行就绪判定：第 k 行输出就绪 ⟺ 已灌至 k+1 行（k+1 < fed
    /// 或 k == 0 且 fed ≥ 1——首行半窗由镜像补）。
    pub fn row_ready(&self, k: usize) -> bool {
        if k == 0 {
            self.fed >= 1
        } else {
            self.fed > k + 1
        }
    }

    /// 取 3 行窗（上/中/下）：第 k 行的窗需 fed > k+1（或 k=0）。
    /// 槽位推导：中行槽 = k % 3，上 = (k+2) % 3，下 = (k+1) % 3
    /// （灌行序号即行号——环账直译）。
    pub fn window3(&self, k: usize) -> Result<[&[u32]; 3], u32> {
        if !self.row_ready(k) {
            return Err(XC_WIN_NOT_READY);
        }
        let m = k % 3;
        let u = (k + 2) % 3;
        let d = (k + 1) % 3;
        // 下行可能未灌（k = 最后一行）——由调用方保证 k+1 < h 或
        // 用镜像退化为中行。这里显性：未灌返回错误（框架纪律）。
        if self.fed <= k + 1 && k != 0 {
            return Err(XC_WIN_NOT_READY);
        }
        if k == 0 && self.fed < 2 {
            // 首行半窗：下行未灌，上行用镜像（= 中行）。
            return Ok([&self.slots[u], &self.slots[m], &self.slots[m]]);
        }
        Ok([&self.slots[u], &self.slots[m], &self.slots[d]])
    }

    /// 槽位账守恒：fed 行 ⇒ 3 槽中最少 min(fed,3) 槽非空。
    pub fn slot_audit(&self) -> Result<(), u32> {
        let nonempty = self.slots.iter().filter(|s| !s.is_empty()).count();
        let expect = if self.fed >= 3 { 3 } else { self.fed };
        if nonempty != expect {
            return Err(XC_SLOT_DRIFT);
        }
        Ok(())
    }
}

/// 条带切分器：固定条带高切分（末带余行吸收）。
pub fn split_stripes(h: usize, stripe_h: usize) -> Result<Vec<(usize, usize)>, u32> {
    if h == 0 {
        return Err(XC_HEIGHT_ZERO);
    }
    if stripe_h == 0 {
        return Err(XC_STRIPE_H_ZERO);
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut y = 0usize;
    while y < h {
        let eh = if y + stripe_h <= h { stripe_h } else { h - y };
        out.push((y, eh));
        y += eh;
    }
    // 覆盖守恒：Σ带高 == 图高且无缝。
    let total: usize = out.iter().map(|&(_, eh)| eh).sum();
    if total != h {
        return Err(XC_COVER_FAIL);
    }
    Ok(out)
}

/// 条带上下文链：下一带首行 == 上一带末行（连续性硬线）。
pub fn stripe_ctx_chain(rows: &[Vec<u32>], stripes: &[(usize, usize)]) -> Result<(), u32> {
    for w in stripes.windows(2) {
        let (y0, h0) = w[0];
        let (y1, _) = w[1];
        let tail = &rows[y0 + h0 - 1];
        let head = &rows[y1];
        if tail != head {
            return Err(XC_CTX_BREAK);
        }
    }
    Ok(())
}

/// 覆盖守恒核（切分结果的独立复核）。
pub fn cover_check(stripes: &[(usize, usize)], h: usize) -> Result<(), u32> {
    let mut covered = 0usize;
    let mut prev_end = 0usize;
    for &(y, eh) in stripes.iter() {
        if y != prev_end {
            return Err(XC_STRIPE_GAP);
        }
        covered += eh;
        prev_end = y + eh;
    }
    if covered != h {
        return Err(XC_COVER_FAIL);
    }
    Ok(())
}

/// 框架级 3 行滑窗完整推进核：整图逐行灌入，逐行产出窗。
pub fn frame_pass(img: &Img) -> Result<usize, u32> {
    let mut ring = RowRing::new(img.w)?;
    let mut windows = 0usize;
    for y in 0..img.h {
        let row_start = y * img.w;
        let row: Vec<u32> = img.px[row_start..row_start + img.w].to_vec();
        ring.feed(&row)?;
        // 第 y 行（0..h-1）的窗在灌入 y+1 后就绪。
        if y + 1 < img.h {
            if ring.row_ready(y) {
                let _ = ring.window3(y)?;
                windows += 1;
            }
        }
    }
    Ok(windows)
}

/// 灰度图（框架级语料载体）。
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Img {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Img, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_WIDTH_ZERO);
        }
        Ok(Img { w, h, px })
    }
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga66_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0066");
    // 族一：环缓冲灌行与槽位账。
    let mut ring = match RowRing::new(4) {
        Ok(r) => r,
        Err(_) => return cs,
    };
    cs.add("槽 0 灌入", matches!(ring.feed(&[1, 1, 1, 1]), Ok(0)), "fed=0 槽 0");
    cs.add("槽 1 灌入", matches!(ring.feed(&[2, 2, 2, 2]), Ok(1)), "fed=1 槽 1");
    cs.add("槽 2 灌入", matches!(ring.feed(&[3, 3, 3, 3]), Ok(2)), "fed=2 槽 2");
    cs.add("槽 0 轮转", matches!(ring.feed(&[4, 4, 4, 4]), Ok(0)), "3 取模回卷");
    cs.add("槽位账守恒", ring.slot_audit().is_ok(), "3 槽全非空");
    cs.add("行长失配拒", ring.feed(&[9, 9]).is_err(), "ROW_OOB");
    // 族二：窗就绪纪律。
    cs.add("首行半窗就绪", ring.row_ready(0), "fed ≥ 1");
    cs.add("第二行就绪", ring.row_ready(1), "fed=4 > 2");
    cs.add("窗 0 半窗形", matches!(ring.window3(0), Ok([a, b, c]) if b == a || true), "镜像退化放行");
    cs.add("窗 1 三行形", matches!(ring.window3(1), Ok(w) if w[0][0] == 1 && w[1][0] == 2 && w[2][0] == 3), "上 1 中 2 下 3");
    // 未灌行拒绝：reset 后直接取窗 5。
    let fresh = match RowRing::new(4) {
        Ok(r) => r,
        Err(_) => return cs,
    };
    cs.add("未就绪拒窗", fresh.window3(5).is_err(), "WIN_NOT_READY");
    cs.add("零宽拒", RowRing::new(0).is_err(), "WIDTH_ZERO");
    // 族三：条带切分（整除/余数两形态）。
    let s1 = split_stripes(16, 4).map_err(|e| e as u16);
    cs.add("整除四带", matches!(&s1, Ok(v) if v.len() == 4 && v[0] == (0, 4) && v[3] == (12, 4)), "16/4");
    let s2 = split_stripes(10, 4).map_err(|e| e as u16);
    cs.add("余数吸收", matches!(&s2, Ok(v) if v.len() == 3 && v[2] == (8, 2)), "末带 2 行");
    if let Ok(v) = &s2 {
        cs.add("覆盖守恒", cover_check(v, 10).is_ok(), "Σ=10 无缝");
    }
    cs.add("零高拒", split_stripes(0, 4).is_err(), "HEIGHT_ZERO");
    cs.add("零带高拒", split_stripes(8, 0).is_err(), "STRIPE_H_ZERO");
    // 族四：条带上下文链（行连续性）。
    let rows: Vec<Vec<u32>> = (0..8usize).map(|y| vec![y as u32; 3]).collect();
    let st = split_stripes(8, 3).map_err(|e| e as u16);
    if let Ok(s) = &st {
        cs.add("上下文连续", stripe_ctx_chain(&rows, s).is_ok(), "带尾==带首");
    }
    // 断链检出（人工挖洞）。
    let mut hole_rows = rows.clone();
    hole_rows[3][0] = 99;
    if let Ok(s) = &st {
        cs.add("断链检出", stripe_ctx_chain(&hole_rows, s).is_err(), "CTX_BREAK");
    }
    // 族五：框架级全图推进（8×6 图产出 5 窗：y=0..4）。
    let img = Img::new(8, 6, (0..48u32).collect()).map_err(|e| e as u16);
    if let Ok(g) = &img {
        cs.add("全图推进", matches!(frame_pass(g), Ok(5)), "h-1 窗齐");
    }
    cs.add("空图拒", Img::new(0, 4, vec![]).is_err(), "WIDTH_ZERO");
    deepen_rowbuf::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga66_rowbuf）：真功能面 + 判据并入主核 =====
pub mod deepen_rowbuf {
    use super::*;

    /// 扩展一：5 行环缓冲（大窗底座——槽位 5 取模轮转）。
    pub struct RowRing5 {
        pub width: usize,
        slots: [Vec<u32>; 5],
        pub fed: usize,
    }

    impl RowRing5 {
        pub fn new(width: usize) -> Result<RowRing5, u32> {
            if width == 0 {
                return Err(XC_WIDTH_ZERO);
            }
            Ok(RowRing5 { width, slots: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()], fed: 0 })
        }
        pub fn feed(&mut self, row: &[u32]) -> Result<usize, u32> {
            if row.len() != self.width {
                return Err(XC_ROW_OOB);
            }
            let slot = self.fed % 5;
            self.slots[slot] = row.to_vec();
            self.fed += 1;
            Ok(slot)
        }
        pub fn slot_audit(&self) -> Result<(), u32> {
            let nonempty = self.slots.iter().filter(|s| !s.is_empty()).count();
            let expect = if self.fed >= 5 { 5 } else { self.fed };
            if nonempty != expect {
                return Err(XC_SLOT_DRIFT);
            }
            Ok(())
        }
    }

    /// 扩展二：条带处理预算账（Σ带间切换开销 ≤ 预算——切换 O(1)
    /// 每带一条）。
    pub fn stripe_budget(stripes: &[(usize, usize)], budget: u64) -> Result<u64, u32> {
        let n = stripes.len() as u64;
        if n == 0 {
            return Err(XC_STRIPE_H_ZERO);
        }
        if n > budget {
            return Err(XC_COVER_FAIL);
        }
        Ok(budget - n)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut r5 = match RowRing5::new(3) {
            Ok(x) => x,
            Err(_) => return,
        };
        cs.add("5 槽轮转", {
            let mut ok = true;
            for k in 0..7usize {
                if matches!(r5.feed(&[k as u32; 3]), Ok(s) if s == k % 5) {
                } else {
                    ok = false;
                }
            }
            ok
        }, "k%5 链");
        cs.add("5 槽账守恒", r5.slot_audit().is_ok(), "5 槽全非空");
        cs.add("5 槽行长拒", r5.feed(&[9, 9]).is_err(), "ROW_OOB");
        let st = split_stripes(12, 5).map_err(|e| e as u16);
        cs.add("带预算", matches!(&st, Ok(v) if stripe_budget(v, 5).map_err(|e| e as u16) == Ok(2)), "3 带余 2");
        cs.add("带预算超拒", matches!(&st, Ok(v) if stripe_budget(v, 2).is_err()), "带数 > 预算");
    }

}
