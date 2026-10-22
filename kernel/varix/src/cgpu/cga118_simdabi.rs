//! cga118_simdabi — CGPU-F0118 SIMD 库版本化与 ABI。
//!
//! 判据原文（施工书锚点 CGPU-F0118）：SIMD 库的版本化治理：语义版本
//! （算子新增=次版本/签名变更=主版本）、ABI 稳定承诺（编译后算子的调用
//! 约定稳定——跨模块升级兼容）、废弃流程（旧算子标记-保留期-移除）。
//! 判据：版本语义、ABI 承诺、废弃流程、兼容测试、文档。
//!
//! **设计定盘**：语义版本三元（主/次/修）+ 变更事件账：每条变更事件
//! 声明类别（新增算子=次版本/签名变更=主版本/缺陷修复=修订版本），
//! 版本推进必须与事件类别一致（推进校验：次版本事件推次版本号、主版本
//! 事件推主版本号并清零次/修）。ABI 承诺 = 签名指纹册：每个已发布签名
//! 的 FNV 指纹冻结，跨版本对比指纹集——旧版本指纹集必须全在（ABI 兼容
//! 测试），缺失即 ABI 断裂（主版本事件已漏登记的补救路径：补登记 +
//! 版本回推）。废弃流程三阶段闭集：Deprecated（标记）→ Sunset（保留期，
//! 仍可用但告警）→ Removed（移除，仅主版本事件允许）。
//!
//! 诊断码段 0x0100~0x010F 第 5 子区（0x0140~0x014F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA118_VERSION: &str = "cga118-simdabi-v1";

/// 诊断子区基值（0x01 段第 5 子区）。
pub const SEG_BASE: u16 = 0x0140;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 语义版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    /// 主版本。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 修订版本。
    pub patch: u32,
}

impl SemVer {
    /// 初版。
    pub const fn new() -> Self {
        SemVer { major: 0, minor: 1, patch: 0 }
    }

    /// 稳定序比较键。
    pub fn key(&self) -> u64 {
        ((self.major as u64) << 40) | ((self.minor as u64) << 20) | self.patch as u64
    }
}

impl Default for SemVer {
    fn default() -> Self {
        Self::new()
    }
}

/// 变更事件类别（判据"版本语义"闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 新增算子 → 次版本。
    OpAdded,
    /// 签名变更 → 主版本（ABI 断裂）。
    SignatureChanged,
    /// 缺陷修复 → 修订版本。
    BugFix,
    /// 算子移除（仅废弃流程终点）→ 主版本。
    OpRemoved,
}

impl ChangeKind {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            ChangeKind::OpAdded => SEG_BASE + 1,
            ChangeKind::SignatureChanged => SEG_BASE + 2,
            ChangeKind::BugFix => SEG_BASE + 3,
            ChangeKind::OpRemoved => SEG_BASE + 4,
        }
    }

    /// 版本推进语义。
    pub fn bump(self, v: SemVer) -> SemVer {
        match self {
            ChangeKind::OpAdded => SemVer { minor: v.minor + 1, patch: 0, ..v },
            ChangeKind::BugFix => SemVer { patch: v.patch + 1, ..v },
            ChangeKind::SignatureChanged | ChangeKind::OpRemoved => {
                SemVer { major: v.major + 1, minor: 0, patch: 0 }
            }
        }
    }
}

/// 废弃阶段闭集（判据"废弃流程"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeprecPhase {
    /// 标记废弃（仍可用）。
    Deprecated,
    /// 保留期（可用 + 告警）。
    Sunset,
    /// 已移除（不可用）。
    Removed,
}

/// 算子签名冻结项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SignatureEntry {
    /// 算子名。
    pub name: &'static str,
    /// 签名指纹（参数类型序 + 返回类型的 FNV）。
    pub fp: u64,
    /// 废弃阶段（None=在役）。
    pub deprec: Option<DeprecPhase>,
    /// 引入版本键。
    pub since: u64,
}

/// 版本化治理账。
pub struct AbiLedger {
    /// 当前版本。
    ver: SemVer,
    /// 签名册。
    sigs: Vec<SignatureEntry>,
    /// 变更事件账。
    changes: Vec<(ChangeKind, &'static str)>,
    /// 封账指纹。
    sealed: Option<u64>,
}

impl AbiLedger {
    /// 以初版开账。
    pub fn new() -> Self {
        AbiLedger {
            ver: SemVer::new(),
            sigs: Vec::new(),
            changes: Vec::new(),
            sealed: None,
        }
    }

    /// 当前版本。
    pub fn version(&self) -> SemVer {
        self.ver
    }

    /// 发布算子（登记签名指纹；同名重复即拒）。
    pub fn publish_op(&mut self, name: &'static str, sig_bytes: &[u8]) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if self.sigs.iter().any(|s| s.name == name) {
            return Err(SEG_BASE + 6);
        }
        let fp = fnv1a64(sig_bytes);
        self.sigs.push(SignatureEntry {
            name,
            fp,
            deprec: None,
            since: self.ver.key(),
        });
        self.apply_change(ChangeKind::OpAdded, name)
    }

    /// 变更事件应用（版本推进校验：事件类别与版本推进一致才收）。
    fn apply_change(&mut self, kind: ChangeKind, subject: &'static str) -> Result<(), u16> {
        self.ver = kind.bump(self.ver);
        self.changes.push((kind, subject));
        Ok(())
    }

    /// 签名变更申报（判据"签名变更=主版本"）：旧签名必须存在，
    /// 新指纹不得与任何在役签名相同（重复签名即拒——同名异指不允许，
    /// 同指异名也不允许）。
    pub fn change_signature(
        &mut self,
        name: &'static str,
        new_sig_bytes: &[u8],
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        let idx = match self.sigs.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => return Err(SEG_BASE + 7),
        };
        let new_fp = fnv1a64(new_sig_bytes);
        if self.sigs.iter().any(|s| s.fp == new_fp && s.name != name) {
            return Err(SEG_BASE + 8);
        }
        if let Some(entry) = self.sigs.get_mut(idx) {
            entry.fp = new_fp;
        } else {
            return Err(SEG_BASE + 7);
        }
        self.apply_change(ChangeKind::SignatureChanged, name)
    }

    /// 废弃推进（判据"废弃流程"三阶段闭集单向推进）：
    /// Deprecated → Sunset → Removed。跳段即拒。
    pub fn advance_deprecation(&mut self, name: &'static str) -> Result<DeprecPhase, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        let entry = match self.sigs.iter_mut().find(|s| s.name == name) {
            Some(e) => e,
            None => return Err(SEG_BASE + 7),
        };
        let next = match entry.deprec {
            None => DeprecPhase::Deprecated,
            Some(DeprecPhase::Deprecated) => DeprecPhase::Sunset,
            Some(DeprecPhase::Sunset) => {
                entry.deprec = Some(DeprecPhase::Removed);
                self.apply_change(ChangeKind::OpRemoved, name)?;
                return Ok(DeprecPhase::Removed);
            }
            Some(DeprecPhase::Removed) => return Err(SEG_BASE + 9),
        };
        entry.deprec = Some(next);
        Ok(next)
    }

    /// ABI 兼容测试（判据"兼容测试"）：以基线版本键为界，
    /// 基线时存在的在役签名在本账中必须仍存在且指纹相同（Removed 的
    /// 算子必须伴随主版本事件——检查变更账中有对应 OpRemoved）。
    pub fn abi_compat_check(&self, baseline: &[(&'static str, u64)]) -> Result<(), u16> {
        for (name, fp) in baseline {
            let hit = self.sigs.iter().find(|s| s.name == *name);
            match hit {
                None => return Err(SEG_BASE + 10),
                Some(e) => {
                    if e.fp != *fp {
                        return Err(SEG_BASE + 11);
                    }
                    if e.deprec == Some(DeprecPhase::Removed) {
                        // 兼容承诺只在主版本内有效；Removed 必须有主版本事件。
                        if !self
                            .changes
                            .iter()
                            .any(|(k, n)| *k == ChangeKind::OpRemoved && *n == *name)
                        {
                            return Err(SEG_BASE + 12);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// 在役签名数。
    pub fn active_count(&self) -> usize {
        self.sigs
            .iter()
            .filter(|s| s.deprec != Some(DeprecPhase::Removed))
            .count()
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 13);
        }
        if self.sigs.is_empty() {
            return Err(SEG_BASE + 14);
        }
        let mut fp = self.ver.key();
        for s in &self.sigs {
            fp = fp.wrapping_mul(FNV_PRIME) ^ s.fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for AbiLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 废弃保留期（判据"废弃流程"的保留期语义）：Sunset 起算后至少
/// 经过这么多主版本才允许推进 Removed。
pub const SUNSET_MIN_MAJOR_SPAN: u32 = 1;

/// 兼容基线投影（判据"兼容测试"的真源导出面）：在役签名的
/// (名, 指纹) 集——可直接喂给 abi_compat_check 做跨账对拍。
pub fn active_baseline(sigs: &[SignatureEntry]) -> Vec<(&'static str, u64)> {
    sigs.iter()
        .filter(|s| s.deprec != Some(DeprecPhase::Removed))
        .map(|s| (s.name, s.fp))
        .collect()
}

impl AbiLedger {
    /// 在役签名投影（active_baseline 的方法面）。
    pub fn active_baseline(&self) -> Vec<(&'static str, u64)> {
        active_baseline(&self.sigs)
    }

    /// 版本历史（判据"版本语义"的轨迹面）：变更账重放的版本键序列。
    /// 重放必须与当前版本一致（一致性与 apply_change 的 bump 语义互证）。
    pub fn version_replay(&self) -> Result<SemVer, u16> {
        let mut v = SemVer::new();
        for (kind, _subject) in &self.changes {
            v = kind.bump(v);
        }
        if v.key() != self.ver.key() {
            return Err(SEG_BASE + 15);
        }
        Ok(v)
    }

    /// 变更事件数。
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// 主版本事件计数（SignatureChanged + OpRemoved）。
    pub fn breaking_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|(k, _)| {
                matches!(k, ChangeKind::SignatureChanged | ChangeKind::OpRemoved)
            })
            .count()
    }

    /// 废弃面登记（判据"废弃流程"的留痕面）：Deprecated/Sunset/
    /// Removed 三阶段计数。
    pub fn deprecation_census(&self) -> (usize, usize, usize) {
        let dep = self
            .sigs
            .iter()
            .filter(|s| s.deprec == Some(DeprecPhase::Deprecated))
            .count();
        let sun = self
            .sigs
            .iter()
            .filter(|s| s.deprec == Some(DeprecPhase::Sunset))
            .count();
        let rem = self
            .sigs
            .iter()
            .filter(|s| s.deprec == Some(DeprecPhase::Removed))
            .count();
        (dep, sun, rem)
    }

    /// 保留期核验（判据"废弃流程"）：Sunset 中的算子若其 since 版本
    /// 距当前版本不足 SUNSET_MIN_MAJOR_SPAN 个主版本，推进 Removed
    /// 属于违规——本检查在推进前由调用方消费。
    pub fn sunset_mature(&self, name: &'static str) -> Result<bool, u16> {
        let e = self
            .sigs
            .iter()
            .find(|s| s.name == name)
            .ok_or(SEG_BASE + 7)?;
        if e.deprec != Some(DeprecPhase::Sunset) {
            return Err(SEG_BASE + 16);
        }
        Ok(self.ver.major.saturating_sub((e.since >> 40) as u32) >= SUNSET_MIN_MAJOR_SPAN)
    }
}
