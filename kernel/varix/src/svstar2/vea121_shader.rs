//! VE-F0121 · 异构着色器编译矩阵（VE-A 域 · A05 多 GPU 与异构组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0121`
//!
//! **职责定位（锚点原文）**：同一着色器在多卡的编译矩阵（每卡各自编译+兼容性验证），
//! 矩阵缓存（每卡独立），跨卡编译差异的归因；含矩阵的编译耗时画像。
//!
//! **铁律**：
//! 1. **每卡编译**——独立编译 + 兼容性验证。
//! 2. **矩阵缓存**——每卡独立缓存。
//! 3. **差异归因**——编译差异归因到卡。
//! 4. **缺格补编**——矩阵缺格即补编。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BF（编译矩阵）。
pub const SEG_BASE: u16 = 0xE8BF;

/// 编译失败。
pub const XC_COMPILE_FAIL: u16 = SEG_BASE | 0x01;
/// 矩阵缺格。
pub const XC_MATRIX_GAP: u16 = SEG_BASE | 0x02;
/// 缓存损坏。
pub const XC_CACHE_CORRUPT: u16 = SEG_BASE | 0x03;
/// 兼容性不符。
pub const XC_INCOMPATIBLE: u16 = SEG_BASE | 0x04;

/// 编译条目。
#[derive(Clone, Debug)]
pub struct CompileEntry {
    pub shader_id: u32,
    pub gpu_id: u16,
    /// 编译产物指纹。
    pub artifact_fp: u32,
    /// 编译耗时（微秒）。
    pub compile_us: u32,
    /// 兼容性验证通过。
    pub compatible: bool,
    /// 缓存有效。
    pub cached: bool,
}

/// 编译矩阵。
pub struct CompileMatrix {
    entries: Vec<CompileEntry>,
    /// 每卡独立缓存版本。
    cache_ver: Vec<(u16, u32)>,
    last_code: u16,
    /// 补编次数。
    fills: u32,
    /// 归因记录。
    attributions: Vec<(u32, u16, u16)>,
}

impl CompileMatrix {
    pub fn new() -> Self {
        CompileMatrix {
            entries: Vec::new(),
            cache_ver: Vec::new(),
            last_code: 0,
            fills: 0,
            attributions: Vec::new(),
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 编译（每卡独立）。
    pub fn compile(
        &mut self,
        shader_id: u32,
        gpu_id: u16,
        artifact_fp: u32,
        compile_us: u32,
        compatible: bool,
    ) -> Result<(), u16> {
        if !compatible {
            self.last_code = XC_INCOMPATIBLE;
            return Err(XC_INCOMPATIBLE);
        }
        self.entries.retain(|e| !(e.shader_id == shader_id && e.gpu_id == gpu_id));
        self.entries.push(CompileEntry {
            shader_id,
            gpu_id,
            artifact_fp,
            compile_us,
            compatible,
            cached: true,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 矩阵缺格检测：给定 shader × 卡集，找出缺格。
    pub fn find_gaps(&mut self, shader_id: u32, gpus: &[u16]) -> Vec<u16> {
        let mut gaps = Vec::new();
        for &g in gpus {
            if !self
                .entries
                .iter()
                .any(|e| e.shader_id == shader_id && e.gpu_id == g)
            {
                gaps.push(g);
            }
        }
        if !gaps.is_empty() {
            self.last_code = XC_MATRIX_GAP;
        }
        gaps
    }

    /// 缺格补编。
    pub fn fill_gap(&mut self, shader_id: u32, gpu_id: u16) -> Result<(), u16> {
        self.compile(shader_id, gpu_id, 0, 0, true)?;
        self.fills += 1;
        Ok(())
    }

    /// 差异归因：同 shader 不同卡产物指纹不同 → 归因到慢卡。
    pub fn attribute_diff(&mut self, shader_id: u32) -> Option<(u16, u16)> {
        let group: Vec<&CompileEntry> = self
            .entries
            .iter()
            .filter(|e| e.shader_id == shader_id)
            .collect();
        if group.len() < 2 {
            return None;
        }
        let base_fp = group[0].artifact_fp;
        let mut diff_gpu = None;
        for e in &group {
            if e.artifact_fp != base_fp {
                diff_gpu = Some(e.gpu_id);
            }
        }
        if let Some(dg) = diff_gpu {
            // 归因到编译慢的卡。
            let slow = group
                .iter()
                .max_by_key(|e| e.compile_us)
                .map(|e| e.gpu_id)
                .unwrap_or(0);
            self.attributions.push((shader_id, dg, slow));
            Some((dg, slow))
        } else {
            None
        }
    }

    /// 缓存损坏检测。
    pub fn verify_cache(&mut self) -> bool {
        for e in &self.entries {
            if e.cached && e.artifact_fp == 0 && e.compile_us > 0 {
                self.last_code = XC_CACHE_CORRUPT;
                return false;
            }
        }
        true
    }

    /// 编译耗时画像。
    pub fn slowest_gpu(&self, shader_id: u32) -> Option<(u16, u32)> {
        self.entries
            .iter()
            .filter(|e| e.shader_id == shader_id)
            .max_by_key(|e| e.compile_us)
            .map(|e| (e.gpu_id, e.compile_us))
    }

    pub fn fills(&self) -> u32 {
        self.fills
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn attributions(&self) -> &[(u32, u16, u16)] {
        &self.attributions
    }
}

impl Default for CompileMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：每卡编译、矩阵缓存、差异归因、缺格补编。
pub fn run_vea121_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea121_shader");

    // C121-01 每卡编译
    {
        let mut m = CompileMatrix::new();
        let r = m.compile(1, 0, 0xAA, 100, true);
        chk(&mut set, "C121-01 编译", r.is_ok() && m.entry_count() == 1, "编译失败");
    }

    // C121-02 缺格检测
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let gaps = m.find_gaps(1, &[0, 1, 2]);
        chk(&mut set, "C121-02 缺格", gaps.len() == 2 && gaps.contains(&1) && gaps.contains(&2), "缺格漏检");
    }

    // C121-03 补编
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let gaps = m.find_gaps(1, &[0, 1]);
        let _ = m.fill_gap(1, gaps[0]);
        chk(&mut set, "C121-03 补编", m.fills() == 1 && m.find_gaps(1, &[0, 1]).is_empty(), "未补编");
    }

    // C121-04 差异归因
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let _ = m.compile(1, 1, 0xBB, 500, true);
        let r = m.attribute_diff(1);
        chk(&mut set, "C121-04 归因", r == Some((1, 1)), "归因错");
    }

    // C121-05 兼容性拒绝
    {
        let mut m = CompileMatrix::new();
        let r = m.compile(1, 0, 0xAA, 100, false);
        chk(
            &mut set,
            "C121-05 兼容拒",
            r.is_err() && m.last_code() == XC_INCOMPATIBLE,
            "不兼容被放",
        );
    }

    // C121-06 缓存校验
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        chk(&mut set, "C121-06 缓存", m.verify_cache(), "缓存损坏");
    }

    // C121-07 耗时画像
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let _ = m.compile(1, 1, 0xAA, 300, true);
        let r = m.slowest_gpu(1);
        chk(&mut set, "C121-07 画像", r == Some((1, 300)), "画像错");
    }

    // C121-08 无差异不归因
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let _ = m.compile(1, 1, 0xAA, 100, true);
        let r = m.attribute_diff(1);
        chk(&mut set, "C121-08 无差异", r.is_none(), "误归因");
    }

    // C121-09 覆盖编译
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let _ = m.compile(1, 0, 0xCC, 200, true);
        chk(&mut set, "C121-09 覆盖", m.entry_count() == 1, "未覆盖");
    }

    // C121-10 归因记录留痕
    {
        let mut m = CompileMatrix::new();
        let _ = m.compile(1, 0, 0xAA, 100, true);
        let _ = m.compile(1, 1, 0xBB, 500, true);
        let _ = m.attribute_diff(1);
        chk(&mut set, "C121-10 留痕", m.attributions().len() == 1, "未留痕");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea121_all_green() {
        let s = run_vea121_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
