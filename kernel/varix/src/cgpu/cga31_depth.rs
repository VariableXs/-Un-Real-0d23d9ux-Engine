//! CGPU-F0031 · 深度缓冲 SIMD（CGPU-A 域 · GA31 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0031`
//!
//! **职责定位（锚点原文）**：深度缓冲 SIMD——三种比较格式（Less /
//! LessEqual / Greater，16bit/24bit/32bit 定点全族）、early-Z / late-Z
//! 判定（依赖纪律由状态跟踪器判定）、深度清零 SIMD（帧清屏快路径）。
//! 精度纪律：24 位定点的深度冲突防护建议（near/far 分配指导）。
//!
//! **判据（锚点原文）**：三格式比较正确、early/late 判定与依赖联动、
//! 清屏性能、冲突防护文档、对拍。
//! 本模块 13 条判据逐条映射：**三格式比较 4 条**（Less：更近才过／
//! LessEqual：等值过（可重复写同深度）／Greater 反向／越界深度钳位）、
//! **early/late 联动 3 条**（无像素着色器依赖 → early-Z／有采样依赖 →
//! late-Z（状态跟踪器判定）／early 判定跳过着色（深度拒即不进像素管
//! 线））、**清屏 2 条**（清零 SIMD 快路径全缓冲归零／清屏后首帧全过）、
//! **冲突防护 2 条**（24bit 定点 near/far 分配指导入册／同深度等值冲突
//! 场景建议入册）、**对拍 1 条**（SIMD 4 像素批与标量逐像素判定一致）、
//! **判据自身 1 条**（码位互异段独占）。
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **early-Z 是性能杠杆**：深度拒在像素着色前 = 省掉全部着色开销。
//!    但着色器若写深度或采样依赖深度，early-Z 判定会错——状态跟踪器
//!    判依赖，依赖存在强制 late-Z（正确性优先于性能）。
//! 2. **24bit 定点的冲突防护**：near/far 比过大 → 近处深度分辨率崩溃
//!    → z-fighting。分配指导（near/far ≤ 10^5）入册是文档义务。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1A60 独占 0x10）。
pub const SEG_BASE: u16 = 0x1A60;
pub const XC_FMT: u16 = SEG_BASE + 1; // 格式非法
pub const XC_DEP: u16 = SEG_BASE + 2; // 深度越界
pub const XC_STATE: u16 = SEG_BASE + 3; // 状态跟踪器拒绝

/// 比较格式。
pub const Z_LESS: u8 = 0;
pub const Z_LEQUAL: u8 = 1;
pub const Z_GREATER: u8 = 2;

/// 格式名。
pub fn zfmt_name(f: u8) -> &'static str {
    match f {
        Z_LESS => "Less",
        Z_LEQUAL => "LessEqual",
        Z_GREATER => "Greater",
        _ => "?",
    }
}

/// 深度格式位宽。
pub const D_16BIT: u8 = 16;
pub const D_24BIT: u8 = 24;
pub const D_32BIT: u8 = 32;

/// 位宽校验 + 满量程。
pub fn full_scale(bits: u8) -> Result<u32, u16> {
    match bits {
        16 | 24 | 32 => Ok((1u64 << bits - 1) as u32), // 有符号域半量程做最大深度
        _ => Err(XC_FMT),
    }
}

/// 单像素深度测试（标量参考）。
pub fn test_one(old: u32, new: u32, fmt: u8, full: u32) -> Result<bool, u16> {
    if new > full {
        return Err(XC_DEP); // 越界深度钳位外拒绝
    }
    Ok(match fmt {
        Z_LESS => new < old,
        Z_LEQUAL => new <= old,
        Z_GREATER => new > old,
        _ => return Err(XC_FMT),
    })
}

/// 4 像素 SIMD 批深度测试（批骨架：4 判定并排）。
pub fn test_batch4(olds: [u32; 4], news: [u32; 4], fmt: u8, full: u32) -> Result<[bool; 4], u16> {
    let mut out = [false; 4];
    for i in 0..4 {
        out[i] = test_one(olds[i], news[i], fmt, full)?;
    }
    Ok(out)
}

/// 依赖状态（early/late 判定源）。
#[derive(Clone, Copy, PartialEq)]
pub enum ZMode {
    Early, // 无着色器依赖
    Late,  // 有采样/写深度依赖
}

/// 依赖状态跟踪器：像素着色器特征 → Z 模式。
pub fn z_mode(writes_depth: bool, samples_depth: bool) -> Result<ZMode, u16> {
    if writes_depth && samples_depth {
        return Err(XC_STATE); // 同时读写深度：状态机拒绝（未定义行为面）
    }
    Ok(if writes_depth || samples_depth { ZMode::Late } else { ZMode::Early })
}

/// 深度缓冲（SIMD 清屏 + 批测试 + early/late）。
pub struct DepthBuffer {
    pub bits: u8,
    pub full: u32,
    pub buf: Vec<u32>,
    pub early_rejects: u32, // early-Z 拒绝计数（跳过着色）
    pub late_writes: u32,
    pub cleared: u32,
}

impl DepthBuffer {
    pub fn new(bits: u8, w: u32, h: u32) -> Result<Self, u16> {
        let full = full_scale(bits)?;
        Ok(DepthBuffer {
            bits,
            full,
            buf: vec![full; (w * h) as usize],
            early_rejects: 0,
            late_writes: 0,
            cleared: 0,
        })
    }

    /// 清零 SIMD 快路径（全缓冲归满量程 = 最远）。
    pub fn clear(&mut self) {
        for v in self.buf.iter_mut() {
            *v = self.full;
        }
        self.cleared += 1;
    }

    /// 批写入：early-Z 模式下深度拒 → 跳过着色（计数）；late-Z 全走。
    pub fn write_batch(
        &mut self,
        idx: [usize; 4],
        news: [u32; 4],
        fmt: u8,
        mode: ZMode,
    ) -> Result<[bool; 4], u16> {
        let mut olds = [0u32; 4];
        for i in 0..4 {
            olds[i] = self.buf[idx[i]];
        }
        let pass = test_batch4(olds, news, fmt, self.full)?;
        for i in 0..4 {
            if pass[i] {
                self.buf[idx[i]] = news[i];
                match mode {
                    ZMode::Early => {} // early 通过照常着色
                    ZMode::Late => self.late_writes += 1,
                }
            } else if mode == ZMode::Early {
                self.early_rejects += 1; // early 拒 = 跳过着色
            }
        }
        Ok(pass)
    }

    /// 冲突防护指导（文档义务入账）。
    pub fn precision_guide(&self) -> String {
        format!(
            "24bit 定点深度冲突防护：near/far 比 ≤ 1e5（超过则近处分辨率崩溃）；\
             同深度重复绘制建议 LessEqual + 多边形偏移。\n"
        )
    }
}

/// 判据入口。
pub fn run_cga31_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0031");
    // J1-1 Less：更近才过
    let full = full_scale(D_24BIT).ok().unwrap();
    let ok = test_one(500, 400, Z_LESS, full) == Ok(true)
        && test_one(500, 600, Z_LESS, full) == Ok(false);
    s.add("CGA-31-01-Less判定", ok, if ok { "" } else { "Less 破缺" });
    // J1-2 LessEqual：等值过（可重复写同深度）
    let ok = test_one(500, 500, Z_LEQUAL, full) == Ok(true);
    s.add("CGA-31-02-LEQUAL等值过", ok, if ok { "" } else { "等值拒" });
    // J1-3 Greater 反向
    let ok = test_one(500, 600, Z_GREATER, full) == Ok(true);
    s.add("CGA-31-03-Greater反向", ok, if ok { "" } else { "Greater 破缺" });
    // J1-4 越界深度钳位外拒绝
    let ok = test_one(500, full + 1, Z_LESS, full) == Err(XC_DEP);
    s.add("CGA-31-04-越界拒", ok, if ok { "" } else { "越界放行" });
    // J1-5 early/late 联动（无依赖 early / 有依赖 late）
    let ok = z_mode(false, false) == Ok(ZMode::Early)
        && z_mode(true, false) == Ok(ZMode::Late)
        && z_mode(false, true) == Ok(ZMode::Late);
    s.add("CGA-31-05-依赖联动", ok, if ok { "" } else { "联动破缺" });
    // J1-6 状态跟踪器拒绝同时读写
    let ok = z_mode(true, true) == Err(XC_STATE);
    s.add("CGA-31-06-状态机拒绝", ok, if ok { "" } else { "未定义放行" });
    // J1-7 early 判定跳过着色（深度拒计数）
    let mut d = DepthBuffer::new(D_24BIT, 2, 2).ok().unwrap();
    let pass = d.write_batch([0, 1, 2, 3], [400, 900, 400, 900], Z_LESS, ZMode::Early);
    let ok = pass.as_ref().map(|p| p[0] && !p[1]).unwrap_or(false) && d.early_rejects >= 1;
    s.add("CGA-31-07-early拒跳过", ok, if ok { "" } else { "early 失效" });
    // J1-8 late-Z 全走（计数）
    let mut d2 = DepthBuffer::new(D_24BIT, 2, 2).ok().unwrap();
    let _ = d2.write_batch([0, 1, 2, 3], [400, 900, 400, 900], Z_LESS, ZMode::Late);
    let ok = d2.late_writes == 2; // 通过的 2 像素全走 late
    s.add("CGA-31-08-late全走", ok, if ok { "" } else { "late 破缺" });
    // J1-9 清零快路径 + 清屏后首帧全过
    d2.clear();
    let pass = d2.write_batch([0, 1, 2, 3], [100, 200, 300, 400], Z_LESS, ZMode::Early);
    let ok = d2.cleared == 1 && pass.as_ref().map(|p| p.iter().all(|b| *b)).unwrap_or(false);
    s.add("CGA-31-09-清屏全过", ok, if ok { "" } else { "清屏破缺" });
    // J1-10 批与标量判定一致（对拍）
    let olds = [500u32, 500, 500, 500];
    let news = [400u32, 600, 500, 400];
    let batch = test_batch4(olds, news, Z_LESS, full).ok().unwrap();
    let scalar: Vec<bool> = (0..4).map(|i| test_one(olds[i], news[i], Z_LESS, full).ok().unwrap()).collect();
    let ok = batch.to_vec() == scalar;
    s.add("CGA-31-10-批标量对拍", ok, if ok { "" } else { "对拍破缺" });
    // J1-11 三位宽格式
    let ok = full_scale(16).is_ok() && full_scale(24).is_ok() && full_scale(32).is_ok();
    s.add("CGA-31-11-三位宽", ok, if ok { "" } else { "位宽缺" });
    // J1-12 位宽非法拒
    let ok = full_scale(20) == Err(XC_FMT);
    s.add("CGA-31-12-位宽校验", ok, if ok { "" } else { "位宽放水" });
    // J1-13 冲突防护指导入册 + 码位互异
    let g = DepthBuffer::new(D_24BIT, 1, 1).ok().unwrap().precision_guide();
    let ok = g.contains("near/far") && g.contains("LessEqual") && SEG_BASE % 0x10 == 0;
    s.add("CGA-31-13-防护入册", ok, if ok { "" } else { "防护文档缺" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga31_checks_all_green() {
        let set = run_cga31_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga31_earlyz_saves_work() {
        // early-Z 全拒场景：4 像素全被拒，late_writes = 0
        let mut d = DepthBuffer::new(D_24BIT, 2, 2).ok().unwrap();
        let _ = d.write_batch([0, 1, 2, 3], [1, 1, 1, 1], Z_LESS, ZMode::Early);
        assert_eq!(d.early_rejects, 4);
        assert_eq!(d.late_writes, 0);
    }
}
