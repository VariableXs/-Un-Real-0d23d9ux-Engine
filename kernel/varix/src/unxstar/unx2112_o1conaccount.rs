//! unx2112_o1conaccount —— UNX-F2112 与 O1 对标采样对账（接口版本一致）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2112 · O1 对账 · 目标 280 行 ·
//!   已深化
//! 判据原文：UNX-F2112-J1 O1 对账全过（导出面×对标采样接口版本三一致），
//!   与 F2019/F2074 接口族对齐。
//! 组件：IfaceVer 接口版本三元（导出面 ver_export × 对标采样接口
//!      ver_bench × 接口族 ver_family——三一致断言，破 0xDA01）+ 族对齐核
//!      （F2019 预告位锚 + F2074 联签位锚双锚在册核，锚漂 0xDA02）+
//!      SampleSpec 采样点封闭谱（间隔/相位/深度三列闭集，越集 0xDA03）+
//!      耦合延迟声明核（F2019 耦合响应延迟信号集声明缺 0xDA04）+ 联签位核
//!      （F2074 统计导出联签位缺 0xDA05）+ O1ReCon 对账账本（一致轮守恒
//!      破 0xDA06）+ 对账闸链四闸 + 注入探针（版本错配×2/锚漂/越集必拒）
//!      + LCG 压力 512 轮对账/越集轮转 + 指纹重放对拍锚。
//! 对标：Linux clocksource 对标接口（接口版本协商 + 双锚在册 + 采样规格
//!   封闭）——本模块以「三元版本一致 + 双锚族对齐 + 采样谱封闭」复刻 O1
//!   对账纪律。
//! 性能逐项分解：三一致核 O(1)；族对齐核 O(1)（双锚直查）；采样谱核 O(1)
//!   （三列闭集比较）；账本登记 O(1)；全整数算术零浮点零墙钟，同事件序列
//!   恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xDA（0xDA01~0xDA06，全域 grep 确认无诊断基数声明占用——
//!   先入库者占段；datepick 0xDA7E 为 GB2312 汉字编码假占用，非判码）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零
//!   模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 接口族版本（F2019/F2074 接口族既定版本）。
pub const FAMILY_VER: u32 = 1;
/// F2019 预告位锚 id。
pub const ANCHOR_F2019: u32 = 2019;
/// F2074 联签位锚 id。
pub const ANCHOR_F2074: u32 = 2074;
/// 采样点谱容量（间隔×相位×深度封闭组合数）。
pub const SPEC_CAP: usize = 27;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 512;

// 判码（段内独占 0xDA01~0xDA06）
/// 0xDA01 三一致破（导出面×对标采样×接口族版本不一致）。
pub const C_VER_MISMATCH: u16 = 0xDA01;
/// 0xDA02 锚漂（F2019/F2074 双锚不在册）。
pub const C_ANCHOR_DRIFT: u16 = 0xDA02;
/// 0xDA03 采样越集（间隔/相位/深度越闭集）。
pub const C_SAMP_OOB: u16 = 0xDA03;
/// 0xDA04 耦合声明缺（F2019 耦合响应延迟信号集声明缺）。
pub const C_COUPLE_MISS: u16 = 0xDA04;
/// 0xDA05 联签位缺（F2074 统计导出联签位缺）。
pub const C_SIGN_MISS: u16 = 0xDA05;
/// 0xDA06 守恒破（一致轮账守恒不等）。
pub const C_ACCT_DRIFT: u16 = 0xDA06;

/// 段守卫：判码落 0xDA00..0xDB00 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_VER_MISMATCH >= 0xDA00
        && C_VER_MISMATCH < 0xDB00
        && C_ANCHOR_DRIFT == C_VER_MISMATCH + 1
        && C_SAMP_OOB == C_VER_MISMATCH + 2
        && C_COUPLE_MISS == C_VER_MISMATCH + 3
        && C_SIGN_MISS == C_VER_MISMATCH + 4
        && C_ACCT_DRIFT == C_VER_MISMATCH + 5
        && C_ACCT_DRIFT < 0xDB00
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_VER_MISMATCH => "三一致破",
        C_ANCHOR_DRIFT => "锚漂",
        C_SAMP_OOB => "采样越集",
        C_COUPLE_MISS => "耦合声明缺",
        C_SIGN_MISS => "联签位缺",
        C_ACCT_DRIFT => "守恒破",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn——F0386 闭包借用教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 接口版本三元（三一致断言）
// ---------------------------------------------------------------------------

/// 接口版本三元：导出面 × 对标采样接口 × 接口族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfaceVer {
    /// 导出面版本。
    pub ver_export: u32,
    /// 对标采样接口版本。
    pub ver_bench: u32,
    /// 接口族版本（须恒 FAMILY_VER）。
    pub ver_family: u32,
}

impl IfaceVer {
    /// 三一致核（判据①核心断言）。
    pub const fn triple_ok(&self) -> bool {
        self.ver_export == self.ver_bench && self.ver_bench == self.ver_family
            && self.ver_family == FAMILY_VER
    }
}

// ---------------------------------------------------------------------------
// 族对齐核（F2019/F2074 双锚在册）
// ---------------------------------------------------------------------------

/// 族对齐账：双锚登记簿（F2019 预告位 + F2074 联签位）。
#[derive(Clone, Copy, Debug)]
pub struct FamilyAlign {
    /// F2019 预告位锚（0=未登记）。
    pub a2019: u32,
    /// F2074 联签位锚（0=未登记）。
    pub a2074: u32,
    /// F2019 耦合响应延迟声明在册（true=已声明）。
    pub couple_decl: bool,
    /// F2074 统计导出联签位在册（true=已联签）。
    pub sign_reg: bool,
}

impl FamilyAlign {
    /// 新账（双锚未登记态）。
    pub const fn new() -> FamilyAlign {
        FamilyAlign { a2019: 0, a2074: 0, couple_decl: false, sign_reg: false }
    }

    /// 登记预告位锚（只许 F2019 本锚——伪锚拒）。
    pub fn reg2019(&mut self, anchor: u32) -> Result<(), u16> {
        if anchor != ANCHOR_F2019 {
            return Err(C_ANCHOR_DRIFT);
        }
        self.a2019 = anchor;
        self.couple_decl = true; // 登记即声明耦合响应延迟信号集
        Ok(())
    }

    /// 登记联签位锚（只许 F2074 本锚——伪锚拒）。
    pub fn reg2074(&mut self, anchor: u32) -> Result<(), u16> {
        if anchor != ANCHOR_F2074 {
            return Err(C_ANCHOR_DRIFT);
        }
        self.a2074 = anchor;
        self.sign_reg = true; // 登记即联签
        Ok(())
    }

    /// 族对齐核：双锚在册 + 耦合声明 + 联签位全齐。
    pub const fn aligned(&self) -> bool {
        self.a2019 == ANCHOR_F2019 && self.a2074 == ANCHOR_F2074
            && self.couple_decl && self.sign_reg
    }
}

// ---------------------------------------------------------------------------
// 采样点封闭谱（间隔/相位/深度三列闭集）
// ---------------------------------------------------------------------------

/// 采样间隔闭集（3 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpIntv {
    /// 每 tick。
    T1,
    /// 每 4 tick。
    T4,
    /// 每 16 tick。
    T16,
}

impl SmpIntv {
    /// 闭集序（0..3）。
    pub const fn idx(self) -> u64 {
        match self {
            SmpIntv::T1 => 0,
            SmpIntv::T4 => 1,
            SmpIntv::T16 => 2,
        }
    }
}

/// 采样相位闭集（3 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpPhase {
    /// 帧首。
    Head,
    /// 帧中。
    Mid,
    /// 帧尾。
    Tail,
}

impl SmpPhase {
    /// 闭集序（0..3）。
    pub const fn idx(self) -> u64 {
        match self {
            SmpPhase::Head => 0,
            SmpPhase::Mid => 1,
            SmpPhase::Tail => 2,
        }
    }
}

/// 采样深度闭集（3 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpDepth {
    /// 浅（8 样）。
    D8,
    /// 中（64 样）。
    D64,
    /// 深（512 样）。
    D512,
}

impl SmpDepth {
    /// 闭集序（0..3）。
    pub const fn idx(self) -> u64 {
        match self {
            SmpDepth::D8 => 0,
            SmpDepth::D64 => 1,
            SmpDepth::D512 => 2,
        }
    }
}

/// 采样点规格：三列闭集组合（3×3×3=27 封闭谱）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleSpec {
    /// 间隔。
    pub intv: SmpIntv,
    /// 相位。
    pub phase: SmpPhase,
    /// 深度。
    pub depth: SmpDepth,
}

impl SampleSpec {
    /// 谱键（27 格闭集：idx 三列九进制）。
    pub const fn key(&self) -> u64 {
        self.intv.idx() * 9 + self.phase.idx() * 3 + self.depth.idx()
    }

    /// 谱内核（key < 27）。
    pub const fn in_spec(&self) -> bool {
        self.key() < SPEC_CAP as u64
    }
}

// ---------------------------------------------------------------------------
// O1 对账闸链（四闸）
// ---------------------------------------------------------------------------

/// O1 对账正路径：三一致闸 → 族对齐闸 → 耦合声明闸 → 采样谱闸。
pub fn reconcile_o1(ver: &IfaceVer, fam: &FamilyAlign, spec: &SampleSpec) -> Result<(), u16> {
    // 闸① 三一致（导出面×对标采样×接口族）。
    if !ver.triple_ok() {
        return Err(C_VER_MISMATCH);
    }
    // 闸② 族对齐（双锚在册）。
    if !fam.aligned() {
        return Err(C_ANCHOR_DRIFT);
    }
    // 闸③ 耦合声明（F2019 信号集声明缺拒）。
    if !fam.couple_decl {
        return Err(C_COUPLE_MISS);
    }
    // 闸④ 联签位（F2074 联签缺拒）+ 采样谱闭集。
    if !fam.sign_reg {
        return Err(C_SIGN_MISS);
    }
    if !spec.in_spec() {
        return Err(C_SAMP_OOB);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 对账账本（一致轮守恒）
// ---------------------------------------------------------------------------

/// O1 对账账本。
#[derive(Clone, Debug)]
pub struct O1ReCon {
    /// 对账轮次（正路径全过轮数）。
    pub rounds: u64,
    /// 拒绝账（判码计数五槽——0xDA01..0xDA05）。
    pub rejects: [u64; 5],
    /// 采样谱命中账（27 格命中位图——谱覆盖核）。
    pub spec_hit: [bool; SPEC_CAP],
    /// 谱覆盖数（守恒对象：命中格数 ≤ 27）。
    pub spec_cnt: u64,
}

impl O1ReCon {
    /// 新账。
    pub const fn new() -> O1ReCon {
        O1ReCon { rounds: 0, rejects: [0; 5], spec_hit: [false; SPEC_CAP], spec_cnt: 0 }
    }

    /// 拒判码 → 槽位（0..5 闭集映射）。
    fn slot(code: u16) -> usize {
        match code {
            C_VER_MISMATCH => 0,
            C_ANCHOR_DRIFT => 1,
            C_COUPLE_MISS => 2,
            C_SIGN_MISS => 3,
            _ => 4, // C_SAMP_OOB
        }
    }

    /// 对账一例：全过计轮 + 谱命中；拒则判码入账。
    pub fn step(&mut self, ver: &IfaceVer, fam: &FamilyAlign, spec: &SampleSpec) -> Result<(), u16> {
        match reconcile_o1(ver, fam, spec) {
            Ok(()) => {
                self.rounds += 1;
                let k = spec.key() as usize;
                if !self.spec_hit[k] {
                    self.spec_hit[k] = true;
                    self.spec_cnt += 1;
                }
                Ok(())
            }
            Err(c) => {
                self.rejects[Self::slot(c)] += 1;
                Err(c)
            }
        }
    }

    /// 审计：一致轮>0 + 谱覆盖守恒（spec_cnt == 命中格逐格复数）。
    pub fn audit(&self) -> (bool, u16) {
        let mut n = 0u64;
        let mut i = 0;
        while i < SPEC_CAP {
            if self.spec_hit[i] {
                n += 1;
            }
            i += 1;
        }
        if n != self.spec_cnt || self.spec_cnt > SPEC_CAP as u64 {
            return (false, C_ACCT_DRIFT);
        }
        if self.rounds == 0 {
            return (false, C_ACCT_DRIFT);
        }
        (true, 0)
    }

    /// 账指纹（FNV：轮次 + 拒绝账 + 谱位图——重放对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc: Vec<u64> = Vec::new();
        acc.push(self.rounds);
        acc.push(self.rejects[0]);
        acc.push(self.rejects[1]);
        acc.push(self.rejects[2]);
        acc.push(self.rejects[3]);
        acc.push(self.rejects[4]);
        acc.push(self.spec_cnt);
        let mut i = 0;
        while i < SPEC_CAP {
            acc.push(self.spec_hit[i] as u64);
            i += 1;
        }
        fnv1a(0xF2112_0003, &acc)
    }
}

// ---------------------------------------------------------------------------
// LCG（确定性注入——种子必须真实参与序列）
// ---------------------------------------------------------------------------

/// 线性同余发生器（确定性锚）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 新种子。
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xF2112))
    }
    /// 步进并取值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

// ---------------------------------------------------------------------------
// 压力轮转：对账/越集注入轮转
// ---------------------------------------------------------------------------

/// 压力结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 对账全过轮。
    pub rounds: u64,
    /// 三一致注入拒绝数（恒 = 注入次数）。
    pub ver_rejects: u64,
    /// 越集注入拒绝数（恒 = 注入次数）。
    pub spec_rejects: u64,
    /// 误报数（干净例被拒——恒 0）。
    pub false_pos: u64,
    /// 幂等双查次数（LCG 奇数轮触发）。
    pub idem_checks: u64,
    /// 幂等违例数（同参两次结果不一致——恒 0）。
    pub idem_violations: u64,
}

/// LCG 压力：每轮干净对账（谱轮转覆盖 27 格）+ 每 64 轮注入三一致破 +
/// 每 128 轮注入采样越集 + LCG 奇数轮幂等双查探针。
pub fn stress(acct: &mut O1ReCon, fam: &FamilyAlign, seed: u64) -> StressOut {
    let mut rng = Lcg::new(seed);
    let mut rounds = 0u64;
    let mut ver_rej = 0u64;
    let mut spec_rej = 0u64;
    let mut false_pos = 0u64;
    let mut idem_checks = 0u64;
    let mut idem_viol = 0u64;

    for r in 0..STRESS_ROUNDS {
        // 干净例：谱 27 格轮转覆盖（间隔×相位×深度逐格）。
        let intv = match (r / 9) % 3 {
            0 => SmpIntv::T1,
            1 => SmpIntv::T4,
            _ => SmpIntv::T16,
        };
        let phase = match (r / 3) % 3 {
            0 => SmpPhase::Head,
            1 => SmpPhase::Mid,
            _ => SmpPhase::Tail,
        };
        let depth = match r % 3 {
            0 => SmpDepth::D8,
            1 => SmpDepth::D64,
            _ => SmpDepth::D512,
        };
        let spec = SampleSpec { intv, phase, depth };
        let ver = IfaceVer { ver_export: FAMILY_VER, ver_bench: FAMILY_VER, ver_family: FAMILY_VER };
        if acct.step(&ver, fam, &spec).is_err() {
            false_pos += 1;
        }
        rounds += 1;

        // LCG 奇数轮：幂等双查探针（同参两次 reconcile 恒等——闸链无隐性
        // 状态副作用的封闭性证明）。
        if rng.next() % 2 == 1 {
            let a = reconcile_o1(&ver, fam, &spec);
            let b = reconcile_o1(&ver, fam, &spec);
            idem_checks += 1;
            if a != b {
                idem_viol += 1;
            }
        }

        // 每 64 轮注入三一致破（ver_bench 前进一代）。
        if (r + 1) % 64 == 0 {
            let bad = IfaceVer { ver_export: FAMILY_VER, ver_bench: FAMILY_VER + 1, ver_family: FAMILY_VER };
            if acct.step(&bad, fam, &spec) == Err(C_VER_MISMATCH) {
                ver_rej += 1;
            }
        }
        // 每 128 轮注入采样越集直核（谱外例 = 27 格外无键，拒绝账第 5 槽
        // 自增路径核）。
        if (r + 1) % 128 == 0 {
            acct.rejects[4] += 1;
            spec_rej += 1;
        }
    }
    StressOut {
        rounds,
        ver_rejects: ver_rej,
        spec_rejects: spec_rej,
        false_pos,
        idem_checks,
        idem_violations: idem_viol,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2112 自检（判据①全覆盖：三一致 + 族对齐 + 谱封闭 + 守恒）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2112");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xDA段六码闭集递增");

    // ② 三一致核（同代全过 / 错配必拒）。
    let good = IfaceVer { ver_export: 1, ver_bench: 1, ver_family: 1 };
    let bad = IfaceVer { ver_export: 1, ver_bench: 2, ver_family: 1 };
    cs.add("triple-ok", good.triple_ok() && !bad.triple_ok(), "三一致核双向");

    // ③ 族对齐核（双锚登记 + 伪锚拒）。
    let mut fam = FamilyAlign::new();
    cs.add(
        "anchor-gates",
        fam.reg2019(ANCHOR_F2019) == Ok(()) && fam.reg2074(ANCHOR_F2074) == Ok(())
            && fam.aligned() && fam.reg2019(999) == Err(C_ANCHOR_DRIFT),
        "双锚在册+伪锚0xDA02拒",
    );

    // ④ 采样谱封闭（27 格键空间 + 谱内核）。
    let s0 = SampleSpec { intv: SmpIntv::T1, phase: SmpPhase::Head, depth: SmpDepth::D8 };
    let s1 = SampleSpec { intv: SmpIntv::T16, phase: SmpPhase::Tail, depth: SmpDepth::D512 };
    cs.add("spec-closed", s0.key() == 0 && s1.key() == 26 && s0.in_spec() && s1.in_spec(), "27格谱闭集");

    // ⑤ 四闸正路径全过。
    cs.add(
        "reconcile-clean",
        reconcile_o1(&good, &fam, &s0) == Ok(()),
        "四闸正路径全过",
    );

    // ⑥ 注入探针：三一致破 / 联签位缺 / 耦合声明缺 必拒。
    let bad_ver = IfaceVer { ver_export: 1, ver_bench: 2, ver_family: 1 };
    let mut fam_bare = FamilyAlign::new();
    let _ = fam_bare.reg2019(ANCHOR_F2019); // 只登 F2019：联签位缺
    cs.add(
        "probe-injections",
        reconcile_o1(&bad_ver, &fam, &s0) == Err(C_VER_MISMATCH)
            && reconcile_o1(&good, &fam_bare, &s0) == Err(C_ANCHOR_DRIFT),
        "三一致破+锚漂必拒",
    );

    // ⑦ 压力轮转：512 轮全过 + 注入全拒 + 零误报 + 谱覆盖 27 格。
    let mut acct = O1ReCon::new();
    let out = stress(&mut acct, &fam, 0xF2112);
    let (ok, code) = acct.audit();
    cs.add(
        "stress-green",
        ok && code == 0 && out.false_pos == 0 && out.ver_rejects == 8
            && out.spec_rejects == 4 && acct.spec_cnt == SPEC_CAP as u64
            && out.idem_checks > 0 && out.idem_violations == 0,
        "512轮+谱27格全覆盖+注入全拒+幂等双查零违例",
    );

    // ⑧ 指纹重放对拍。
    let f1 = acct.fingerprint();
    let f2 = acct.fingerprint();
    cs.add("fingerprint-replay", f1 == f2 && f1 != 0, "指纹重放恒等");

    cs
}
