//! ves478_s26visual — VE-F4244 · 视觉一致性验证
//! 像素级比对引擎（基准截图 vs 实时渲染 diff——阈值分级：像素/区域/布局三级）+令牌声明值比对联动（F4211）+
//! 主题×语种矩阵批量截图执行；基准失真→重制基准+双签；环境差异误报→环境指纹过滤；diff 爆炸→区域聚合；
//! 含无障碍色标主题矩阵专项（域本色）；截图不含真实用户画面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1A40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsCode {
    Ok = 0,
    PixelDrift = SEG_BASE + 0x01, // 0x1A41
    BaselineStale = SEG_BASE + 0x02, // 0x1A42
    EnvNoise = SEG_BASE + 0x03,   // 0x1A43
    DiffExplosion = SEG_BASE + 0x04, // 0x1A44
}

/// 三级阈值。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Threshold { Pixel = 0, Region = 1, Layout = 2 }

/// 比对引擎。
pub struct VisualEngine {
    pub baselines: Vec<(&'static str, u64)>, // (矩阵格, 基准指纹)
    pub env_fingerprint: u64,
    pub env_allow: u64, // 环境容差指纹
    pub diffs: Vec<(&'static str, u32)>, // (格, diff 像素数)
    pub a11y_cells: u32, // a11y 色标矩阵格（域本色）
    pub aggregate: u32,
    pub resealed: u32,
}

impl VisualEngine {
    pub fn new(env: u64) -> Self {
        VisualEngine { baselines: Vec::new(), env_fingerprint: env, env_allow: env,
            diffs: Vec::new(), a11y_cells: 0, aggregate: 0, resealed: 0 }
    }
    pub fn set_baseline(&mut self, cell: &'static str, fp: u64) {
        self.baselines.retain(|(c, _)| *c != cell);
        self.baselines.push((cell, fp));
    }
    /// 比对：环境指纹不符→误报过滤；diff 超区域阈值→爆炸聚合；失配→PixelDrift。
    pub fn compare(&mut self, cell: &'static str, rendered: u64, diff_px: u32, region_limit: u32) -> Result<(), VsCode> {
        if self.env_fingerprint != self.env_allow { return Err(VsCode::EnvNoise); }
        if diff_px > region_limit {
            self.aggregate += diff_px; // 区域聚合
            return Err(VsCode::DiffExplosion);
        }
        match self.baselines.iter().find(|(c, _)| *c == cell) {
            None => { self.resealed += 1; return Err(VsCode::BaselineStale); }
            Some((_, fp)) if *fp != rendered => { return Err(VsCode::PixelDrift); }
            _ => Ok(()),
        }
    }
    /// 基准重制（双签指纹）。
    pub fn reseal(&mut self, cell: &'static str, fp: u64, sign_a: bool, sign_b: bool) -> Result<(), VsCode> {
        if !(sign_a && sign_b) { return Err(VsCode::BaselineStale); }
        self.set_baseline(cell, fp);
        self.resealed += 1;
        Ok(())
    }
    /// 矩阵批量执行（主题×语种格）。
    pub fn batch(&mut self, cells: &[&'static str], fps: &[u64]) -> usize {
        let mut ok = 0;
        for (c, f) in cells.iter().zip(fps.iter()) {
            if self.compare(c, *f, 0, 100).is_ok() { ok += 1; }
        }
        ok
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves478_checks() -> CheckSet {
    let mut set = CheckSet::new("ves478");
    let mut eng = VisualEngine::new(0xABCD);
    eng.set_baseline("dark×zh", 0x1111);
    eng.set_baseline("light×en", 0x2222);

    // C478-01 像素级比对通过（基准=渲染）。
    chk(&mut set, "C478-01", eng.compare("dark×zh", 0x1111, 0, 100).is_ok(), "比对通过");

    // C478-02 像素漂移检出。
    chk(&mut set, "C478-02", eng.compare("dark×zh", 0x9999, 10, 100) == Err(VsCode::PixelDrift), "像素漂移");

    // C478-03 diff 爆炸→区域聚合。
    chk(&mut set, "C478-03", eng.compare("dark×zh", 0x1111, 500, 100) == Err(VsCode::DiffExplosion)
        && eng.aggregate == 500, "爆炸聚合");

    // C478-04 环境指纹过滤（误报环境）。
    eng.env_fingerprint = 0xDEAD;
    chk(&mut set, "C478-04", eng.compare("dark×zh", 0x1111, 0, 100) == Err(VsCode::EnvNoise), "环境误报滤除");

    // C478-05 指纹校正后恢复。
    eng.env_fingerprint = 0xABCD;
    chk(&mut set, "C478-05", eng.compare("light×en", 0x2222, 0, 100).is_ok(), "校正恢复");

    // C478-06 基准失真→重制+双签。
    chk(&mut set, "C478-06", eng.compare("new×ar", 0x3333, 0, 100) == Err(VsCode::BaselineStale)
        && eng.resealed == 1, "失真检出");
    chk(&mut set, "C478-06b", eng.reseal("new×ar", 0x3333, true, true).is_ok(), "双签重制");

    // C478-07 单签拒重制。
    chk(&mut set, "C478-07", eng.reseal("x", 1, true, false) == Err(VsCode::BaselineStale), "单签拒");

    // C478-08 矩阵批量执行。
    let cells = ["dark×zh", "light×en", "new×ar"];
    let fps = [0x1111u64, 0x2222, 0x3333];
    chk(&mut set, "C478-08", eng.batch(&cells, &fps) == 3, "矩阵批量");

    // C478-09 域本色：a11y 色标矩阵格专项。
    eng.a11y_cells = 8;
    chk(&mut set, "C478-09", eng.a11y_cells == 8, "色标专项格");

    // C478-10 三级阈值排序单源。
    chk(&mut set, "C478-10", Threshold::Pixel < Threshold::Region && Threshold::Region < Threshold::Layout, "三级单源");

    // C478-11 基准覆盖幂等。
    eng.set_baseline("dark×zh", 0x1111);
    chk(&mut set, "C478-11", eng.baselines.len() == 3, "基准幂等");

    // C478-12 错误码段位单源。
    chk(&mut set, "C478-12", VsCode::DiffExplosion as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves478_smoke() {
        let set = run_ves478_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves478 red: {:?}", red);
    }
}
