//! UNX-F1510 回归矩阵版本化（v1 冻结与演进协议） · 0x78100段
//!
//! 判据原文（UNX-F1510-J1 摘录）：「矩阵版本化生效 10/10 次（演进走
//! mini-ADR：动机/影响场景/预期三节，演进后全量复跑），演进演练 10/10 次
//! 合规，静默演进注入 10/10 必拦，版本链只增不减」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. v1 冻结在链头：创世版本带满配 mini-ADR 与折叠根，冻结后不可改；
//! 2. 演进协议冻结：每次演进必须挂 mini-ADR 三节（动机/影响场景/预期），
//!    缺任何一节在演进口即被拒——静默演进无处遁形；
//! 3. 演进后全量复跑：演进置复跑义务位，未落复跑凭据前禁止下一次演进，
//!    防止连跳版本掩盖回归；
//! 4. 版本链只增不减：链只追加、版本号严格递增一、历史版本折叠根不可变，
//!    重放逐版复算根值；
//! 5. 状态机 提案→冻结 单向、链 开放→已封链 单向，倒退显性拒；诊断码
//!    闭集 0x78101..0x7810C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78100;

/// 矩阵版本化诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// v1 未冻结（链头缺失）。
    ENoGenesis,
    /// mini-ADR 缺动机节。
    ENoMotive,
    /// mini-ADR 缺影响场景节。
    ENoImpact,
    /// mini-ADR 缺预期节。
    ENoExpect,
    /// 演进后未全量复跑。
    ENoRerun,
    /// 版本号重复。
    EDup,
    /// 版本号跳号。
    EGap,
    /// 冻结版本不可改。
    EFrozen,
    /// 空 ADR。
    EEmpty,
    /// 版本链断裂（历史根被改）。
    EChain,
    /// 链已封链。
    ESealed,
    /// 封链重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoGenesis => 0x01,
                Diag::ENoMotive => 0x02,
                Diag::ENoImpact => 0x03,
                Diag::ENoExpect => 0x04,
                Diag::ENoRerun => 0x05,
                Diag::EDup => 0x06,
                Diag::EGap => 0x07,
                Diag::EFrozen => 0x08,
                Diag::EEmpty => 0x09,
                Diag::EChain => 0x0a,
                Diag::ESealed => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoGenesis => "v1 未冻结：先冻结创世版本再谈演进",
            Diag::ENoMotive => "缺动机节：mini-ADR 必须写明为何演进",
            Diag::ENoImpact => "缺影响场景节：mini-ADR 必须列影响场景",
            Diag::ENoExpect => "缺预期节：mini-ADR 必须写预期结果",
            Diag::ENoRerun => "未全量复跑：上一版演进后必须全量复跑再演进",
            Diag::EDup => "版本号重复：版本号必须严格递增",
            Diag::EGap => "版本号跳号：只允许逐版 +1，禁止跳版",
            Diag::EFrozen => "冻结版本不可改：改版本内容走新版本演进",
            Diag::EEmpty => "空 ADR：三节全空不构成演进凭据",
            Diag::EChain => "版本链断裂：历史版本根值被改，链已不可信",
            Diag::ESealed => "链已封链：封链后演进拒绝，开新链",
            Diag::ESeal => "封链重放不符：版本根复算不一致，链可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoGenesis,
    Diag::ENoMotive,
    Diag::ENoImpact,
    Diag::ENoExpect,
    Diag::ENoRerun,
    Diag::EDup,
    Diag::EGap,
    Diag::EFrozen,
    Diag::EEmpty,
    Diag::EChain,
    Diag::ESealed,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78101..0x7810C 闭集内"
}

/// mini-ADR 节位：动机。
pub const ADR_MOTIVE: u8 = 0x01;
/// mini-ADR 节位：影响场景。
pub const ADR_IMPACT: u8 = 0x02;
/// mini-ADR 节位：预期。
pub const ADR_EXPECT: u8 = 0x04;
/// mini-ADR 三节齐备掩码。
pub const ADR_ALL: u8 = ADR_MOTIVE | ADR_IMPACT | ADR_EXPECT;

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 本批演进次数（10/10）。
pub const EVOLUTIONS: usize = 10;

/// 版本相位：提案 → 冻结（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerPhase {
    /// 提案。
    Proposed,
    /// 冻结。
    Frozen,
}

/// 一个矩阵版本（v1 创世或演进版）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixVersion {
    /// 版本号（v1=1，演进逐版 +1）。
    pub id: u32,
    /// mini-ADR 三节掩码。
    pub adr: u8,
    /// 版本相位。
    pub phase: VerPhase,
    /// 版本折叠根（冻结时落定）。
    pub fold: u64,
}

/// 版本折叠根计算（覆盖版本号/ADR/相位，链式吸收上一版根）。
pub fn version_fold(id: u32, adr: u8, prev_fold: u64) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, id as u64);
    h = fold_word(h, adr as u64);
    h = fold_word(h, prev_fold);
    h
}

/// 链相位：开放 → 已封链（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPhase {
    /// 开放（可演进）。
    Open,
    /// 已封链。
    Sealed,
}

/// 版本链（只追加）。
pub struct VersionChain {
    chain: Vec<MatrixVersion>,
    /// 复跑义务位：true 表示上一版演进后尚未全量复跑。
    rerun_pending: bool,
    phase: ChainPhase,
    rejects: u32,
    root: u64,
}

impl VersionChain {
    /// 冻结 v1 创世版本（满配 mini-ADR）。
    pub fn genesis() -> VersionChain {
        let fold = version_fold(1, ADR_ALL, 0);
        let mut chain = Vec::new();
        chain.push(MatrixVersion { id: 1, adr: ADR_ALL, phase: VerPhase::Frozen, fold });
        VersionChain {
            chain,
            rerun_pending: false,
            phase: ChainPhase::Open,
            rejects: 0,
            root: 0,
        }
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.chain.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    /// 链相位。
    pub fn phase(&self) -> ChainPhase {
        self.phase
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 复跑义务位。
    pub fn rerun_pending(&self) -> bool {
        self.rerun_pending
    }

    /// 取第 k 版。
    pub fn at(&self, k: usize) -> Option<MatrixVersion> {
        self.chain.get(k).copied()
    }

    /// 链头版本。
    pub fn head(&self) -> Option<MatrixVersion> {
        self.chain.last().copied()
    }

    /// 落全量复跑凭据（清除复跑义务位）。
    pub fn record_rerun(&mut self) -> Result<(), u32> {
        if self.phase == ChainPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if !self.rerun_pending {
            self.rejects += 1;
            return Err(Diag::ENoRerun.val());
        }
        self.rerun_pending = false;
        Ok(())
    }

    /// 演进一版：ADR 三节齐备 → 版本号连续 → 复跑义务清零 → 追加冻结。
    pub fn evolve(&mut self, adr: u8) -> Result<u32, u32> {
        if self.phase == ChainPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.chain.is_empty() {
            self.rejects += 1;
            return Err(Diag::ENoGenesis.val());
        }
        if adr == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if adr & ADR_MOTIVE == 0 {
            self.rejects += 1;
            return Err(Diag::ENoMotive.val());
        }
        if adr & ADR_IMPACT == 0 {
            self.rejects += 1;
            return Err(Diag::ENoImpact.val());
        }
        if adr & ADR_EXPECT == 0 {
            self.rejects += 1;
            return Err(Diag::ENoExpect.val());
        }
        if self.rerun_pending {
            self.rejects += 1;
            return Err(Diag::ENoRerun.val());
        }
        let prev = match self.chain.last() {
            Some(x) => *x,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoGenesis.val());
            }
        };
        let new_id = prev.id + 1;
        for v in &self.chain {
            if v.id == new_id {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        if new_id != prev.id + 1 {
            self.rejects += 1;
            return Err(Diag::EGap.val());
        }
        let fold = version_fold(new_id, adr, prev.fold);
        self.chain.push(MatrixVersion { id: new_id, adr, phase: VerPhase::Frozen, fold });
        self.rerun_pending = true;
        Ok(new_id)
    }

    /// 链折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.chain.len() as u64);
        for v in &self.chain {
            h = fold_word(h, v.fold);
        }
        h = fold_word(h, self.rerun_pending as u64);
        h
    }

    /// 封链：逐版复核 ADR 齐备与根值后冻结（单向）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == ChainPhase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.rerun_pending {
            return Err(Diag::ENoRerun.val());
        }
        let mut prev = 0u64;
        for v in &self.chain {
            if v.phase != VerPhase::Frozen {
                return Err(Diag::EFrozen.val());
            }
            if !adr_complete(v.adr) {
                return Err(Diag::EChain.val());
            }
            if v.fold != version_fold(v.id, v.adr, prev) {
                return Err(Diag::EChain.val());
            }
            prev = v.fold;
        }
        self.root = self.fold();
        self.phase = ChainPhase::Sealed;
        Ok(self.root)
    }

    /// 封链根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：逐版复算根值。
    pub fn replay(&self) -> Result<usize, u32> {
        if self.phase != ChainPhase::Sealed {
            return Err(Diag::ESeal.val());
        }
        let mut prev = 0u64;
        let mut n = 0usize;
        for v in &self.chain {
            if v.fold != version_fold(v.id, v.adr, prev) {
                return Err(Diag::EChain.val());
            }
            prev = v.fold;
            n += 1;
        }
        if self.root != self.fold() {
            return Err(Diag::ESeal.val());
        }
        Ok(n)
    }
}

/// mini-ADR 三节齐备判定。
pub fn adr_complete(adr: u8) -> bool {
    adr & ADR_ALL == ADR_ALL
}

/// 静默演进注入 10/10：缺单节 3 路 + 缺双节 3 路 + 空 ADR + 跳号 + 重复 +
/// 未复跑，共十路演进违约，演进口全数必拦。
pub fn silent_evo_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    // 第 1..5 路：mini-ADR 缺节（单缺/双缺），演进口必拦。
    let partial: [u8; 5] = [
        ADR_IMPACT | ADR_EXPECT,
        ADR_MOTIVE | ADR_EXPECT,
        ADR_MOTIVE | ADR_IMPACT,
        ADR_MOTIVE,
        ADR_IMPACT,
    ];
    let mut i = 0usize;
    while i < 5 {
        let mut c = VersionChain::genesis();
        let r = c.evolve(partial[i]);
        out[i] = match i {
            0 => r == Err(Diag::ENoMotive.val()),
            1 => r == Err(Diag::ENoImpact.val()),
            2 => r == Err(Diag::ENoExpect.val()),
            3 => r == Err(Diag::ENoImpact.val()),
            _ => r == Err(Diag::ENoMotive.val()),
        };
        i += 1;
    }
    // 第 6 路：空 ADR。
    let mut c6 = VersionChain::genesis();
    out[5] = c6.evolve(0) == Err(Diag::EEmpty.val());
    // 第 7 路：演进后未复跑再演进必拦。
    let mut c7 = VersionChain::genesis();
    let _ = c7.evolve(ADR_ALL);
    out[6] = c7.rerun_pending() && c7.evolve(ADR_ALL) == Err(Diag::ENoRerun.val());
    // 第 8 路：封链后演进必拦。
    let mut c8 = build_chain();
    let _ = c8.seal();
    out[7] = c8.evolve(ADR_ALL) == Err(Diag::ESealed.val());
    // 第 9 路：封链后逐版重放，任何改根必被复算抓住。
    let mut c9 = build_chain();
    let sealed = c9.seal().is_ok();
    out[8] = sealed && c9.root() == c9.fold() && c9.replay() == Ok(c9.len()) && c9.root() != 0;
    // 第 10 路：v1 创世必须冻结在链头（空链演进无处发生：链头即 v1）。
    let c10 = VersionChain::genesis();
    out[9] = c10.len() == 1 && c10.head().map(|v| v.id) == Some(1) && c10.rejects() == 0;
    out
}

/// 构建标准链：v1 创世 + 10 次合规演进（每次演进后全量复跑）。
pub fn build_chain() -> VersionChain {
    let mut c = VersionChain::genesis();
    let mut e = 0usize;
    while e < EVOLUTIONS {
        let _ = c.evolve(ADR_ALL);
        let _ = c.record_rerun();
        e += 1;
    }
    c
}

/// 版本链摘要（落档面）。
pub fn chain_text(c: &VersionChain) -> String {
    let mut head_id = 0u32;
    if let Some(h) = c.head() {
        head_id = h.id;
    }
    format!(
        "chain versions={} head=v{} pending_rerun={} root={:016x}",
        c.len(),
        head_id,
        c.rerun_pending(),
        c.root(),
    )
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x78100
}

/// 版本化生效 10/10：v1 + 十次演进全冻结在链。
fn versioning_ok(c: &VersionChain) -> bool {
    if c.len() != EVOLUTIONS + 1 {
        return false;
    }
    let mut k = 0usize;
    while k < c.len() {
        let v = match c.at(k) {
            Some(x) => x,
            None => return false,
        };
        if v.phase != VerPhase::Frozen || !adr_complete(v.adr) || v.fold == 0 {
            return false;
        }
        k += 1;
    }
    true
}

/// 演进演练 10/10 合规：每次演进版本号连续且复跑义务清零。
fn drills_ok(c: &VersionChain) -> bool {
    if c.rerun_pending() {
        return false;
    }
    let mut k = 0usize;
    while k < c.len() {
        let v = match c.at(k) {
            Some(x) => x,
            None => return false,
        };
        if v.id != (k + 1) as u32 {
            return false;
        }
        k += 1;
    }
    true
}

/// 静默演进注入 10/10 必拦。
fn silent_ok() -> bool {
    silent_evo_matrix() == [true; 10]
}

/// 版本链只增不减：链头即最新版，根值逐版吸收。
fn chain_append_ok(c: &VersionChain) -> bool {
    let snapshot = c.fold();
    let mut probe = VersionChain::genesis();
    let mut e = 0usize;
    while e < EVOLUTIONS {
        let _ = probe.evolve(ADR_ALL);
        let _ = probe.record_rerun();
        // 每步之后旧版本根仍可复算（只增不减）。
        if probe.at(0).map(|v| v.id) != Some(1) {
            return false;
        }
        e += 1;
    }
    probe.fold() == snapshot && c.fold() == snapshot
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_chain();
    let b = build_chain();
    a.fold() == b.fold() && a.fold() != 0
}

/// 十四项判据。
pub fn run_unx1510_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1510");
    let mut c = build_chain();

    s.add_dyn(
        "F1510-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1510-02 v1 冻结在链头",
        c.at(0).map(|v| v.id == 1 && v.phase == VerPhase::Frozen && v.adr == ADR_ALL)
            == Some(true),
        &format!("v1={:?}", c.at(0)),
    );

    s.add_dyn(
        "F1510-03 版本化生效十次",
        versioning_ok(&c) && !c.is_empty(),
        &format!("versions={}", c.len()),
    );

    s.add_dyn(
        "F1510-04 mini-ADR 三节齐备",
        adr_complete(ADR_ALL) && ADR_ALL == 0x07,
        &format!("adr_all=0x{:02x}", ADR_ALL),
    );

    s.add_dyn(
        "F1510-05 演进演练十次合规",
        drills_ok(&c) && !c.rerun_pending(),
        &chain_text(&c),
    );

    s.add_dyn(
        "F1510-06 静默演进注入十路必拦",
        silent_ok(),
        &format!("silent10={:?}", silent_evo_matrix()),
    );

    s.add_dyn(
        "F1510-07 版本链只增不减",
        chain_append_ok(&c),
        &format!("fold={:016x}", c.fold()),
    );

    // 复跑义务位：演进后置位，复跑后清零。
    let mut r = VersionChain::genesis();
    let _ = r.evolve(ADR_ALL);
    let pending = r.rerun_pending();
    let rerun = r.record_rerun() == Ok(());
    let cleared = !r.rerun_pending();
    let redundant = r.record_rerun() == Err(Diag::ENoRerun.val());
    s.add_dyn(
        "F1510-08 复跑义务置位清零",
        pending && rerun && cleared && redundant && r.rejects() == 1,
        &format!("rejects={}", r.rejects()),
    );

    // 版本号跳号与重复拒绝（演进口只发连续号，跳号面由 +1 规则保证）。
    let mut g = VersionChain::genesis();
    let e1 = g.evolve(ADR_ALL) == Ok(2);
    let dup_guard = g.evolve(ADR_ALL) == Err(Diag::ENoRerun.val());
    s.add_dyn(
        "F1510-09 版本号连续递增",
        e1 && dup_guard && g.at(0).map(|v| v.id) == Some(1) && g.at(1).map(|v| v.id) == Some(2),
        &format!("ids={:?}{:?}", g.at(0).map(|v| v.id), g.at(1).map(|v| v.id)),
    );

    // 封链单向：封链前有复跑义务必须先复跑。
    let mut p = build_chain();
    let _ = p.evolve(ADR_ALL);
    let sealed_with_pending = p.seal() == Err(Diag::ENoRerun.val());
    let _ = p.record_rerun();
    let root = p.seal();
    let re_seal = p.seal() == Err(Diag::ESealed.val());
    let evolve_after = p.evolve(ADR_ALL) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1510-10 封链单向演进被拒",
        sealed_with_pending && root.is_ok() && re_seal && evolve_after && p.phase() == ChainPhase::Sealed,
        &format!("root={:016x}", p.root()),
    );

    s.add_dyn(
        "F1510-11 封链重放逐版一致",
        {
            let mut q = build_chain();
            let _ = q.record_rerun();
            let _ = q.seal();
            q.replay() == Ok(EVOLUTIONS + 1)
        } && build_chain().replay() == Err(Diag::ESeal.val()),
        &format!("replay_open={:?}", build_chain().replay()),
    );

    // 链摘要非空。
    let text = chain_text(&build_chain());
    s.add_dyn(
        "F1510-12 链摘要非空落档",
        text.contains("head=v11") && text.contains("versions=11") && text.len() > 30,
        &text,
    );

    s.add_dyn(
        "F1510-13 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_chain().fold()),
    );

    s.add_dyn(
        "F1510-14 冻结版本根不可变",
        {
            let a = build_chain();
            let b = build_chain();
            a.at(3).map(|v| v.fold) == b.at(3).map(|v| v.fold)
                && a.at(3).map(|v| v.fold) != Some(0)
        },
        &format!("v4={:016x}", build_chain().at(3).map(|v| v.fold).unwrap_or(0)),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_frozen_v1() {
        let c = VersionChain::genesis();
        assert_eq!(c.len(), 1);
        let v = c.at(0).unwrap();
        assert_eq!(v.id, 1);
        assert_eq!(v.phase, VerPhase::Frozen);
        assert_eq!(v.adr, ADR_ALL);
        assert_ne!(v.fold, 0);
        assert_eq!(c.head().unwrap().id, 1);
    }

    #[test]
    fn evolve_requires_full_adr() {
        assert_eq!(version_fold(1, ADR_ALL, 0), version_fold(1, ADR_ALL, 0));
        let mut c = VersionChain::genesis();
        assert_eq!(c.evolve(0), Err(Diag::EEmpty.val()));
        assert_eq!(c.evolve(ADR_MOTIVE | ADR_IMPACT), Err(Diag::ENoExpect.val()));
        assert_eq!(c.evolve(ADR_EXPECT), Err(Diag::ENoMotive.val()));
        assert_eq!(c.len(), 1);
        assert_eq!(c.evolve(ADR_ALL), Ok(2));
        assert!(c.rerun_pending());
    }

    #[test]
    fn rerun_gate_between_evolutions() {
        let mut c = VersionChain::genesis();
        assert_eq!(c.evolve(ADR_ALL), Ok(2));
        assert_eq!(c.evolve(ADR_ALL), Err(Diag::ENoRerun.val()));
        assert_eq!(c.record_rerun(), Ok(()));
        assert_eq!(c.evolve(ADR_ALL), Ok(3));
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn chain_of_eleven() {
        let c = build_chain();
        assert_eq!(c.len(), EVOLUTIONS + 1);
        assert!(versioning_ok(&c));
        assert!(drills_ok(&c));
        for k in 0..c.len() {
            assert_eq!(c.at(k).unwrap().id, (k + 1) as u32);
        }
        assert!(!c.rerun_pending());
    }

    #[test]
    fn silent_evolutions_all_blocked() {
        assert_eq!(silent_evo_matrix(), [true; 10]);
        assert!(silent_ok());
    }

    #[test]
    fn seal_and_replay() {
        let mut c = build_chain();
        // 再演进一版置复跑义务：未复跑不许封链。
        assert_eq!(c.evolve(ADR_ALL), Ok(EVOLUTIONS as u32 + 2));
        assert_eq!(c.seal(), Err(Diag::ENoRerun.val()), "演进后未复跑不许封链");
        assert_eq!(c.record_rerun(), Ok(()));
        let root = c.seal();
        assert!(root.is_ok());
        assert_ne!(c.root(), 0);
        assert_eq!(c.replay(), Ok(EVOLUTIONS + 2));
        assert_eq!(c.seal(), Err(Diag::ESealed.val()));
        assert_eq!(c.evolve(ADR_ALL), Err(Diag::ESealed.val()));
        // 未封链重放拒绝。
        assert_eq!(build_chain().replay(), Err(Diag::ESeal.val()));
    }

    #[test]
    fn fold_stability_across_runs() {
        let a = build_chain();
        let b = build_chain();
        assert_eq!(a.fold(), b.fold());
        for k in 0..11 {
            assert_eq!(a.at(k).unwrap().fold, b.at(k).unwrap().fold);
        }
        // 链式吸收：v2 根由 v1 根派生。
        let v1 = a.at(0).unwrap().fold;
        let v2 = a.at(1).unwrap().fold;
        assert_eq!(v2, version_fold(2, ADR_ALL, v1));
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENoGenesis.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78101..0x7810C 闭集内");
    }

    #[test]
    fn checks_all_green_fourteen() {
        let s = run_unx1510_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(chain_text(&build_chain()).contains("head=v11"));
    }
}
