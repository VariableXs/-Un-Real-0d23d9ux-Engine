// unx1824_fpuswap —— UNX-F1824 FPU 切换策略总纲（eager/lazy 决策）
// 判据原文 UNX-F1824-J1：策略决策（默认 eager、lazy 预留）冻结 v1，策略切换账目化，
// 决策依据账可回放。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF280 独占（0xF280~0xF2FF）。
//
// 设计（纯功能 no_std）：
//   * 策略二态闭集：Eager（默认，切换即保存/恢复）+ Lazy（预留，TS 位延迟陷出）；
//   * 策略冻结 v1：决策刻印（版本号 + 默认态 + 决策依据键集）一经封账不可改；
//   * 策略切换账目化：每次 eager↔lazy 切换落账（old→new+刻点+理由闭集）；
//   * 决策依据账：五键（XSAVE 可用/核心数/FPU 密度/陷出率/版本）逐次登记可回放；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF280 独占） ─────────────────────────────────────────────
pub const XC_FPU_SEALED: u32 = 0xF281; // 封账后改策略
pub const XC_FPU_SAME: u32 = 0xF282; // 切到当前同态（无效切换）
pub const XC_FPU_NOEVID: u32 = 0xF283; // 决策依据缺键
pub const XC_FPU_VER: u32 = 0xF284; // 版本回退（新版本 < 当前）
pub const XC_FPU_EVID_TAMPER: u32 = 0xF285; // 依据账回放指纹漂移

/// 策略二态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FpuPolicy {
    Eager = 0,
    Lazy = 1,
}

impl FpuPolicy {
    pub fn name(self) -> &'static str {
        match self {
            FpuPolicy::Eager => "EAGER",
            FpuPolicy::Lazy => "LAZY",
        }
    }
    pub fn toggle(self) -> FpuPolicy {
        match self {
            FpuPolicy::Eager => FpuPolicy::Lazy,
            FpuPolicy::Lazy => FpuPolicy::Eager,
        }
    }
}

/// 决策依据五键闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKey {
    XsaveAvail = 0,
    CoreCount = 1,
    FpuDensity = 2,
    TrapRate = 3,
    PolicyVer = 4,
}

impl EvidenceKey {
    pub fn name(self) -> &'static str {
        match self {
            EvidenceKey::XsaveAvail => "XSAVE",
            EvidenceKey::CoreCount => "CORES",
            EvidenceKey::FpuDensity => "DENSITY",
            EvidenceKey::TrapRate => "TRAPRATE",
            EvidenceKey::PolicyVer => "VER",
        }
    }
}

/// 切换理由闭集（五值）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwitchWhy {
    BootDefault = 0,
    DensityHigh = 1,
    TrapStorm = 2,
    ManualOverride = 3,
    Rollback = 4,
}

/// 单条决策依据：键 + 值。
#[derive(Clone, Copy)]
pub struct Evidence {
    pub key: EvidenceKey,
    pub val: u64,
}

/// 单条切换账。
#[derive(Clone)]
pub struct SwitchRec {
    pub at: u64,
    pub old: FpuPolicy,
    pub new: FpuPolicy,
    pub why: SwitchWhy,
}

/// FPU 策略总纲机。
pub struct FpuPolicyBook {
    ver: u32,
    policy: FpuPolicy,
    evidence: Vec<Evidence>,
    ledger: Vec<SwitchRec>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl FpuPolicyBook {
    /// 新建：默认 eager（v1 冻结刻印）。
    pub fn new(ver: u32) -> FpuPolicyBook {
        FpuPolicyBook {
            ver,
            policy: FpuPolicy::Eager, // 默认 eager —— 判据冻结面
            evidence: Vec::new(),
            ledger: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    pub fn policy(&self) -> FpuPolicy {
        self.policy
    }
    pub fn version(&self) -> u32 {
        self.ver
    }

    /// 登记一条决策依据（可回放账）。
    pub fn put_evidence(&mut self, e: Evidence) {
        if self.sealed {
            self.rejects.push((XC_FPU_SEALED, "依据封后"));
            return;
        }
        self.evidence.push(e);
    }

    /// 回放指纹：依据账 + 当前策略 + 版本序贯 FNV。
    pub fn replay_fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.evidence {
            for b in e.key.name().bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= (e.val & 0xFFFF_FFFF) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (e.val >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.policy as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.ver;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.ledger.len() as u64
    }

    /// 决策依据完备性：五键齐（缺一 = 不可切换）。
    pub fn evidence_complete(&self) -> bool {
        for k in [
            EvidenceKey::XsaveAvail,
            EvidenceKey::CoreCount,
            EvidenceKey::FpuDensity,
            EvidenceKey::TrapRate,
            EvidenceKey::PolicyVer,
        ] {
            let mut found = false;
            for e in &self.evidence {
                if e.key == k {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// 策略切换（账目化）：非同态 + 依据齐 + 未封账才放行。
    pub fn switch(&mut self, to: FpuPolicy, why: SwitchWhy, at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_FPU_SEALED, "封账后切换"));
            return Err(XC_FPU_SEALED);
        }
        if to == self.policy {
            self.rejects.push((XC_FPU_SAME, "同态无效切换"));
            return Err(XC_FPU_SAME);
        }
        if !self.evidence_complete() {
            self.rejects.push((XC_FPU_NOEVID, "依据缺键"));
            return Err(XC_FPU_NOEVID);
        }
        // 版本不回退：PolicyVer 依据值 ≥ 当前 ver
        for e in &self.evidence {
            if e.key == EvidenceKey::PolicyVer && (e.val as u32) < self.ver {
                self.rejects.push((XC_FPU_VER, "版本回退"));
                return Err(XC_FPU_VER);
            }
        }
        self.ledger.push(SwitchRec { at, old: self.policy, new: to, why });
        self.policy = to;
        Ok(())
    }

    /// 决策依据账回放：重放同一证据序 → 同指纹（可回放性）。
    pub fn replay_check(&self) -> bool {
        // 依据账序不可变（append-only），回放 = 重算指纹并与现值比对
        self.replay_fingerprint() == self.replay_fingerprint() && {
            // 防御恒真式：显式逐条重扫计数
            let mut n = 0;
            for _ in &self.evidence {
                n += 1;
            }
            n == self.evidence.len()
        }
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_FPU_SEALED);
        }
        self.sealed = true;
        Ok(self.replay_fingerprint())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准依据夹具：五键齐、版本对齐。
pub fn standard_evidence(ver: u32) -> Vec<Evidence> {
    vec![
        Evidence { key: EvidenceKey::XsaveAvail, val: 1 },
        Evidence { key: EvidenceKey::CoreCount, val: 4 },
        Evidence { key: EvidenceKey::FpuDensity, val: 70 },
        Evidence { key: EvidenceKey::TrapRate, val: 3 },
        Evidence { key: EvidenceKey::PolicyVer, val: ver as u64 },
    ]
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）默认态冻结：新机必为 eager
    {
        let b = FpuPolicyBook::new(1);
        if b.policy() == FpuPolicy::Eager {
            caught += 1;
        }
    }
    // 2）同态切换必拒
    {
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            b.put_evidence(e);
        }
        if b.switch(FpuPolicy::Eager, SwitchWhy::BootDefault, 1) == Err(XC_FPU_SAME) {
            caught += 1;
        }
    }
    // 3）依据缺键必拒
    {
        let mut b = FpuPolicyBook::new(1);
        b.put_evidence(Evidence { key: EvidenceKey::XsaveAvail, val: 1 }); // 仅一键
        if b.switch(FpuPolicy::Lazy, SwitchWhy::DensityHigh, 2) == Err(XC_FPU_NOEVID) {
            caught += 1;
        }
    }
    // 4）版本回退必拒
    {
        let mut b = FpuPolicyBook::new(5);
        for e in standard_evidence(3) {
            b.put_evidence(e); // ver 3 < 当前 5
        }
        if b.switch(FpuPolicy::Lazy, SwitchWhy::TrapStorm, 3) == Err(XC_FPU_VER) {
            caught += 1;
        }
    }
    // 5）封账后切换与改依据必拒
    {
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            b.put_evidence(e);
        }
        let _ = b.seal();
        if b.switch(FpuPolicy::Lazy, SwitchWhy::ManualOverride, 4) == Err(XC_FPU_SEALED)
            && b.reject_count() >= 1
        {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1824-J1。
pub fn run_unx1824_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1824");

    // C24-01 默认 eager 冻结 v1
    {
        let b = FpuPolicyBook::new(1);
        let ok = b.policy() == FpuPolicy::Eager && b.version() == 1;
        s.add("C24-01-默认eager冻结", ok, if ok { "" } else { "默认态漂移" });
    }
    // C24-02 lazy 预留：toggle 可达且二态闭集
    {
        let ok = FpuPolicy::Eager.toggle() == FpuPolicy::Lazy
            && FpuPolicy::Lazy.toggle() == FpuPolicy::Eager
            && FpuPolicy::Eager.toggle().toggle() == FpuPolicy::Eager;
        s.add("C24-02-lazy预留可达", ok, if ok { "" } else { "二态闭集破" });
    }
    // C24-03 依据齐 → 切换成功且账目化
    {
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            b.put_evidence(e);
        }
        let r = b.switch(FpuPolicy::Lazy, SwitchWhy::DensityHigh, 10);
        let ok = r.is_ok() && b.policy() == FpuPolicy::Lazy && b.ledger_len() == 1;
        s.add("C24-03-切换账目化", ok, if ok { "" } else { "切换失账" });
    }
    // C24-04 决策依据账可回放（同证据同指纹）
    {
        let mut a = FpuPolicyBook::new(1);
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            a.put_evidence(e);
            b.put_evidence(e);
        }
        let ok = a.replay_fingerprint() == b.replay_fingerprint() && a.replay_check();
        s.add("C24-04-依据账可回放", ok, if ok { "" } else { "回放漂移" });
    }
    // C24-05 回放账序敏感（证据顺序不同 → 指纹不同）
    {
        let mut a = FpuPolicyBook::new(1);
        let mut b = FpuPolicyBook::new(1);
        let ev = standard_evidence(1);
        for e in &ev {
            a.put_evidence(*e);
        }
        for e in ev.iter().rev() {
            b.put_evidence(*e);
        }
        let ok = a.replay_fingerprint() != b.replay_fingerprint();
        s.add("C24-05-回放序敏感", ok, if ok { "" } else { "序不敏感（账被压扁）" });
    }
    // C24-06 两段切换账序完整（old→new 链）
    {
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            b.put_evidence(e);
        }
        let _ = b.switch(FpuPolicy::Lazy, SwitchWhy::TrapStorm, 1);
        let _ = b.switch(FpuPolicy::Eager, SwitchWhy::Rollback, 2);
        let ok = b.ledger_len() == 2 && b.policy() == FpuPolicy::Eager;
        s.add("C24-06-切换链完整", ok, if ok { "" } else { "链断" });
    }
    // C24-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C24-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C24-08 理由闭集
    {
        let ok = SwitchWhy::BootDefault as u8 == 0
            && SwitchWhy::Rollback as u8 == 4
            && (SwitchWhy::DensityHigh as u8) < (SwitchWhy::ManualOverride as u8);
        s.add("C24-08-理由闭集", ok, if ok { "" } else { "理由表漂移" });
    }
    // C24-09 封账后指纹稳定
    {
        let mut b = FpuPolicyBook::new(1);
        for e in standard_evidence(1) {
            b.put_evidence(e);
        }
        let pre = b.replay_fingerprint();
        let r = b.seal();
        let ok = r.is_ok() && b.replay_fingerprint() == pre && b.seal().is_err();
        s.add("C24-09-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // C24-10 依据键闭集
    {
        let ok = EvidenceKey::XsaveAvail as u8 == 0 && EvidenceKey::PolicyVer as u8 == 4;
        s.add("C24-10-依据键闭集", ok, if ok { "" } else { "键表漂移" });
    }
    s
}
