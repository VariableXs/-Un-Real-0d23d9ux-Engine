// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves106_compatscan
// VE-F3706 · 主题兼容性扫描（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3706
// 判据原文：不兼容阻断、特性透明、矩阵复述、闭合复用、三单源、判据。
//
// 难点分析：
//   1. 不兼容发布即阻断——版本范围/平台特性/资源闭合三关一关不过就不能发布。
//   2. 静默差异是红线——平台不支持特性必须显式检出，静默降级=特性表不透明。
//   3. 版本×平台矩阵必须全覆盖，缺格即补跑。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9A00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9A00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9ACode(pub u16);

impl C9ACode {
    pub const INCOMPATIBLE: Self = Self(SEG_BASE);      // 不兼容
    pub const FEATURE_UNSUPPORTED: Self = Self(SEG_BASE + 1); // 特性不支持
    pub const RESOURCE_MISSING: Self = Self(SEG_BASE + 2);   // 资源缺失
    pub const MATRIX_GAP: Self = Self(SEG_BASE + 3);    // 矩阵缺格
    pub const COMPAT_PASS: Self = Self(SEG_BASE + 4);   // 兼容通过
    pub const FEATURE_LISTED: Self = Self(SEG_BASE + 5); // 特性已列明
    pub const MATRIX_COVERED: Self = Self(SEG_BASE + 6); // 矩阵全覆盖

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::INCOMPATIBLE => "版本不兼容——不兼容发布即阻断",
            Self::FEATURE_UNSUPPORTED => "平台特性不支持——静默差异红线",
            Self::RESOURCE_MISSING => "引用资源缺失——引用闭合红线",
            Self::MATRIX_GAP => "兼容矩阵缺格——补跑",
            Self::COMPAT_PASS => "兼容性扫描通过",
            Self::FEATURE_LISTED => "特性表列明合规",
            Self::MATRIX_COVERED => "矩阵全覆盖",
            _ => "未知诊断",
        }
    }
}

// ── 版本兼容 ──────────────────────────────────────────────────────

/// 引擎版本范围。
pub struct VersionRange {
    /// 最低版本。
    pub min: u16,
    /// 最高版本。
    pub max: u16,
}

impl VersionRange {
    pub fn new(min: u16, max: u16) -> Self {
        Self { min, max }
    }

    /// 声明是否有效（min ≤ max）。
    pub fn is_valid(&self) -> bool {
        self.min <= self.max
    }

    /// 当前引擎版本是否在范围内。
    pub fn supports(&self, engine_ver: u16) -> bool {
        engine_ver >= self.min && engine_ver <= self.max
    }
}

/// 版本验证器。
pub struct VersionValidator;

impl VersionValidator {
    /// 验证主题声明 vs 当前引擎。
    pub fn check(range: &VersionRange, engine_ver: u16) -> Result<(), C9ACode> {
        if !range.is_valid() {
            return Err(C9ACode::INCOMPATIBLE);
        }
        if range.supports(engine_ver) {
            Ok(())
        } else {
            Err(C9ACode::INCOMPATIBLE)
        }
    }
}

// ── 平台特性 ──────────────────────────────────────────────────────

/// 平台标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    /// 桌面。
    Desktop,
    /// 移动。
    Mobile,
    /// 嵌入式。
    Embedded,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "桌面",
            Self::Mobile => "移动",
            Self::Embedded => "嵌入式",
        }
    }

    pub fn all() -> [Platform; 3] {
        [Self::Desktop, Self::Mobile, Self::Embedded]
    }
}

/// 特性需求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeatureNeed {
    /// 是否必须。
    pub required: bool,
    /// 特性 ID。
    pub feature_id: u16,
}

impl FeatureNeed {
    pub fn new(feature_id: u16, required: bool) -> Self {
        Self { required, feature_id }
    }
}

/// 平台特性检查器（特性表透明）。
pub struct FeatureChecker {
    /// 平台支持表：(platform, feature_id, supported)。
    support_table: Vec<(Platform, u16, bool)>,
    /// 静默差异数。
    silent_diffs: u32,
}

impl FeatureChecker {
    pub fn new() -> Self {
        Self { support_table: Vec::new(), silent_diffs: 0 }
    }

    /// 登记平台特性支持。
    pub fn register(&mut self, p: Platform, fid: u16, supported: bool) {
        self.support_table.push((p, fid, supported));
    }

    /// 检查平台×特性。必须特性不支持即拒绝（不静默）。
    pub fn check(&mut self, p: Platform, need: FeatureNeed) -> Result<(), C9ACode> {
        for &(plat, fid, sup) in &self.support_table {
            if plat == p && fid == need.feature_id {
                if !sup && need.required {
                    self.silent_diffs = self.silent_diffs.saturating_add(1);
                    return Err(C9ACode::FEATURE_UNSUPPORTED);
                }
                return Ok(());
            }
        }
        // 特性表中没有该条目——未知特性必须特性也拒绝
        if need.required {
            self.silent_diffs = self.silent_diffs.saturating_add(1);
            return Err(C9ACode::FEATURE_UNSUPPORTED);
        }
        Ok(())
    }

    pub fn silent_diffs(&self) -> u32 {
        self.silent_diffs
    }
}

impl Default for FeatureChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ── 资源闭合 ──────────────────────────────────────────────────────

/// 资源引用闭合检查。
pub struct ResourceClosure {
    /// 引用的资源 ID 列表。
    referenced: Vec<u64>,
    /// 实际存在的资源 ID。
    available: Vec<u64>,
    missing: u32,
}

impl ResourceClosure {
    pub fn new() -> Self {
        Self { referenced: Vec::new(), available: Vec::new(), missing: 0 }
    }

    pub fn reference(&mut self, id: u64) {
        self.referenced.push(id);
    }

    pub fn provide(&mut self, id: u64) {
        self.available.push(id);
    }

    /// 闭合检查：所有引用必须存在。
    pub fn check(&mut self) -> Result<(), C9ACode> {
        self.missing = 0;
        for &r in &self.referenced {
            if !self.available.contains(&r) {
                self.missing = self.missing.saturating_add(1);
            }
        }
        if self.missing > 0 {
            return Err(C9ACode::RESOURCE_MISSING);
        }
        Ok(())
    }

    pub fn missing(&self) -> u32 {
        self.missing
    }
}

impl Default for ResourceClosure {
    fn default() -> Self {
        Self::new()
    }
}

// ── 兼容矩阵 ──────────────────────────────────────────────────────

/// 版本×平台矩阵采样。
pub struct CompatMatrix {
    /// hit[ver_idx * 3 + plat_idx]，版本 0..2，平台 3 种。
    hit: [bool; 9],
}

impl CompatMatrix {
    pub fn new() -> Self {
        Self { hit: [false; 9] }
    }

    fn idx(ver_idx: usize, plat_idx: usize) -> usize {
        ver_idx * 3 + plat_idx
    }

    /// 标记一格。
    pub fn mark(&mut self, ver_idx: usize, plat_idx: usize) {
        if ver_idx < 3 && plat_idx < 3 {
            let i = Self::idx(ver_idx, plat_idx);
            self.hit[i] = true;
        }
    }

    /// 全覆盖？
    pub fn is_full(&self) -> bool {
        self.hit.iter().all(|&b| b)
    }

    /// 缺格数。
    pub fn gaps(&self) -> usize {
        self.hit.iter().filter(|&&b| !b).count()
    }
}

impl Default for CompatMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves106_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves106_compatscan");
    {
        // 01 版本范围有效且兼容
        let r = VersionRange::new(10, 20);
        chk(
            &mut set,
            "C39A-01",
            r.is_valid() && VersionValidator::check(&r, 15).is_ok(),
            "v15 在 [10,20] 内兼容",
        );
    }
    {
        // 02 版本不兼容被拒
        let r = VersionRange::new(10, 20);
        chk(
            &mut set,
            "C39A-02",
            VersionValidator::check(&r, 25).err() == Some(C9ACode::INCOMPATIBLE),
            "v25 超 [10,20]→不兼容阻断",
        );
    }
    {
        // 03 无效范围
        let r = VersionRange::new(20, 10);
        chk(
            &mut set,
            "C39A-03",
            VersionValidator::check(&r, 15).err() == Some(C9ACode::INCOMPATIBLE),
            "min>max 无效声明→不兼容",
        );
    }
    {
        // 04 必须特性不支持被拒
        let mut fc = FeatureChecker::new();
        fc.register(Platform::Mobile, 1, false);
        let need = FeatureNeed::new(1, true);
        let r = fc.check(Platform::Mobile, need);
        chk(
            &mut set,
            "C39A-04",
            r.err() == Some(C9ACode::FEATURE_UNSUPPORTED) && fc.silent_diffs() == 1,
            "必须特性不支持→显式拒绝（不静默）",
        );
    }
    {
        // 05 可选特性不支持放行
        let mut fc = FeatureChecker::new();
        fc.register(Platform::Mobile, 1, false);
        let need = FeatureNeed::new(1, false);
        chk(&mut set, "C39A-05", fc.check(Platform::Mobile, need).is_ok(), "可选特性不支持不阻断");
    }
    {
        // 06 特性支持放行
        let mut fc = FeatureChecker::new();
        fc.register(Platform::Desktop, 2, true);
        let need = FeatureNeed::new(2, true);
        chk(&mut set, "C39A-06", fc.check(Platform::Desktop, need).is_ok(), "特性支持放行");
    }
    {
        // 07 资源闭合通过
        let mut rc = ResourceClosure::new();
        rc.reference(1);
        rc.reference(2);
        rc.provide(1);
        rc.provide(2);
        chk(&mut set, "C39A-07", rc.check().is_ok(), "引用全有→闭合");
    }
    {
        // 08 资源缺失被拒
        let mut rc = ResourceClosure::new();
        rc.reference(1);
        rc.reference(3); // 无提供
        rc.provide(1);
        rc.provide(2);
        let r = rc.check();
        chk(
            &mut set,
            "C39A-08",
            r.err() == Some(C9ACode::RESOURCE_MISSING) && rc.missing() == 1,
            "引用缺失→闭合红线",
        );
    }
    {
        // 09 矩阵缺格
        let m = CompatMatrix::new();
        chk(
            &mut set,
            "C39A-09",
            !m.is_full() && m.gaps() == 9,
            "空矩阵 9 缺格",
        );
    }
    {
        // 10 矩阵全覆盖
        let mut m = CompatMatrix::new();
        for v in 0..3 {
            for p in 0..3 {
                m.mark(v, p);
            }
        }
        chk(&mut set, "C39A-10", m.is_full() && m.gaps() == 0, "3×3 矩阵全覆盖");
    }
    {
        // 11 未知必须特性被拒
        let mut fc = FeatureChecker::new();
        let need = FeatureNeed::new(99, true);
        chk(
            &mut set,
            "C39A-11",
            fc.check(Platform::Embedded, need).err() == Some(C9ACode::FEATURE_UNSUPPORTED),
            "特性表无条目+必须=拒绝",
        );
    }
    {
        // 12 边界版本兼容
        let r = VersionRange::new(10, 10);
        chk(&mut set, "C39A-12", VersionValidator::check(&r, 10).is_ok(), "单版本范围边界兼容");
    }
    set
}
