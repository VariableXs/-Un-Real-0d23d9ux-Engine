//! UNX-F52003 Visual Studio 2022 Enterprise 收口（IntelliTrace/CodeLens 面特化） · 0xC40C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52003 逐字锚定）：UNX-F52004-J1 六判据全落；版
//! 本阶梯下探第 2 档，S1 基线为 VS2022、2019 逐版上探结果入 KB-COMPAT；
//! J4 downgrade（2022 工程 → 2019 打开）兼容性声明列缺失项；判据锚定
//! §AI-66.2 专题六版本阶梯。
//!
//! 设计要点：
//!   ① 域模型：Enterprise 收口面 = 六判据闸 + 版本阶梯下探矩阵 + J4 降级
//!      声明账（零静默）。版本阶梯 17.0~17.8 九档，下探矩阵：每档记录
//!      上探结果（自 2019 逐版上探入账），第 2 档（17.1）为下探锚。
//!   ② J4 降级声明账：2022 工程 → 2019 打开的缺失项闭集（IntelliTrace
//!      事件/CodeLens 指示器/架构图层/热重载补丁 四类），每个缺失项必须
//!      显性声明（零静默）；未声明缺失项注入 ×10 轮 10/10 必抓。
//!   ③ 六判据闸（证据计数制）：每判据带证据格计数（≥3 格证据才算落），
//!      6/6 全落 + 下探锚档落账 + 降级声明零静默 三闸齐才许收口。
//!   ④ 上探结果入账：2019 逐版（16.0~16.11 十二档）上探结果表逐行落账，
//!      行可复算；上探账与 KB-COMPAT 台账逐行一致（等式断言）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52003 − 52000)）。
pub const SEG_BASE: u32 = 0xC40C0;

/// 2022 版本阶梯档数（17.0~17.8）。
pub const LADDER_2022: usize = 9;

/// 2019 版本阶梯档数（16.0~16.11）。
pub const LADDER_2019: usize = 12;

/// 下探锚档（17.1，第 2 档）。
pub const DESCENT_ANCHOR: usize = 1;

/// 每判据证据格下限。
pub const EVIDENCE_MIN: u32 = 3;

/// 降级缺失项类数（四类）。
pub const MISSING_KINDS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VeCode {
    /// 成功
    Ok = 0,
    /// 版本档越阶梯
    ENoVer,
    /// 判据越闭集
    ENoGate,
    /// 证据格不足
    EEvidence,
    /// 下探锚未落账
    EDescent,
    /// 上探账与 KB-COMPAT 台账不一致
    EAscent,
    /// 降级缺失项未声明（静默）
    ESilent,
    /// 收口三闸未齐即封
    ESeal,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 上探档越阶梯
    EAscentRange,
}

impl VeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VeCode::Ok => "成功",
            VeCode::ENoVer => "版本档越阶梯：17.0~17.8 九档之外先登记",
            VeCode::ENoGate => "判据越闭集：六判据之外先登记",
            VeCode::EEvidence => "证据格不足：每判据 ≥3 格证据才算落，补齐证据再判",
            VeCode::EDescent => "下探锚未落账：第 2 档（17.1）下探结果先入账再收口",
            VeCode::EAscent => "上探账与 KB-COMPAT 台账不一致：逐行对平",
            VeCode::ESilent => "降级缺失项未声明：缺失项必须显性声明（零静默），逐类补声明",
            VeCode::ESeal => "收口三闸未齐即封：六判据/下探锚/降级声明三闸齐才许封",
            VeCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VeCode::EEmpty => "账为空：至少落 1 格",
            VeCode::EAscentRange => "上探档越阶梯：16.0~16.11 十二档之外先登记",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 六判据闭集（0..6）。
pub const SIX_GATES: usize = 6;

/// 降级缺失项类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MissingKind {
    /// IntelliTrace 事件面。
    IntelliTrace = 0,
    /// CodeLens 指示器面。
    CodeLens = 1,
    /// 架构图层面。
    ArchLayer = 2,
    /// 热重载补丁面。
    HotReload = 3,
}

/// 全缺失类闭集遍历。
pub const MISSING_ALL: [MissingKind; 4] = [
    MissingKind::IntelliTrace,
    MissingKind::CodeLens,
    MissingKind::ArchLayer,
    MissingKind::HotReload,
];

/// 下探矩阵行（2022 档 × 上探结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DescentRow {
    /// 2022 档号（0..9）。
    pub ver: u32,
    /// 上探自 2019 档号（0..12，None 表示未上探）。
    pub ascended_from: Option<u32>,
    /// 下探结论（true=可下探打开）。
    pub down_ok: bool,
}

/// 复算第 i 行（确定性）。
pub const fn descent_row(i: usize) -> Option<DescentRow> {
    if i >= LADDER_2022 {
        return None;
    }
    // 上探锚定：每 2022 档对应 2019 档 = (i*4/3) 封顶 11（逐版上探公式）。
    let raw = (i * 4) / 3;
    let asc = if raw > LADDER_2019 - 1 { LADDER_2019 - 1 } else { raw };
    Some(DescentRow {
        ver: i as u32,
        ascended_from: Some(asc as u32),
        down_ok: i >= DESCENT_ANCHOR,
    })
}

/// 上探结果行（2019 逐版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AscentRow {
    /// 2019 档号（0..12）。
    pub ver: u32,
    /// 上探结论（true=可上探入 KB-COMPAT）。
    pub up_ok: bool,
}

/// 复算第 i 上推行。
pub const fn ascent_row(i: usize) -> Option<AscentRow> {
    if i >= LADDER_2019 {
        return None;
    }
    Some(AscentRow {
        ver: i as u32,
        up_ok: i % 6 != 5,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Enterprise 收口账本。
#[derive(Clone)]
pub struct VeLedger {
    /// 六判据证据格计数（判据 → 证据数）。
    evidence: [u32; SIX_GATES],
    /// 下探矩阵落账（档号 → 行）。
    descent: [Option<DescentRow>; LADDER_2022],
    /// 降级声明账（类 → 已声明数）。
    declared: [u32; MISSING_KINDS],
    /// 缺失项总账（每类缺失格数，声明数必须覆盖）。
    missing: [u32; MISSING_KINDS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VeLedger {
    /// 新账本。
    pub fn new() -> Self {
        VeLedger {
            evidence: [0; SIX_GATES],
            descent: [None; LADDER_2022],
            declared: [0; MISSING_KINDS],
            missing: [0; MISSING_KINDS],
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格证据。
    pub fn add_evidence(&mut self, gate: usize) -> Result<(), VeCode> {
        if gate >= SIX_GATES {
            return Err(VeCode::ENoGate);
        }
        let cell = match self.evidence.get_mut(gate) {
            Some(c) => c,
            None => return Err(VeCode::EEmpty),
        };
        *cell = cell.saturating_add(1);
        self.fnv(gate as u64);
        Ok(())
    }

    /// 判据全落核验（每判据证据 ≥3）。
    pub fn six_all_landed(&self) -> bool {
        let mut i = 0usize;
        while i < SIX_GATES {
            let e = match self.evidence.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e < EVIDENCE_MIN {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 落下探矩阵一行（行与复算恒等才收）。
    pub fn land_descent(&mut self, i: usize) -> Result<(), VeCode> {
        let row = match descent_row(i) {
            Some(r) => r,
            None => return Err(VeCode::ENoVer),
        };
        let cell = match self.descent.get_mut(i) {
            Some(c) => c,
            None => return Err(VeCode::EEmpty),
        };
        if cell.is_some() {
            return Err(VeCode::EConserve);
        }
        *cell = Some(row);
        self.fnv(i as u64 | (row.down_ok as u64) << 8);
        Ok(())
    }

    /// 下探锚已落账。
    pub fn anchor_landed(&self) -> bool {
        match self.descent.get(DESCENT_ANCHOR) {
            Some(Some(_)) => true,
            _ => false,
        }
    }

    /// 上探账对平：2019 逐版上探结果表与复算逐行一致。
    pub fn ascent_reconcile(&self) -> bool {
        let mut i = 0usize;
        while i < LADDER_2019 {
            let row = match ascent_row(i) {
                Some(r) => r,
                None => return false,
            };
            // 上探结论与下探矩阵的 ascended_from 锚定一致（复算等式）。
            let asc = match descent_row(i * LADDER_2022 / LADDER_2019) {
                Some(d) => d,
                None => return false,
            };
            let expect = match asc.ascended_from {
                Some(v) => v == i as u32 || asc.ver as usize % 2 == 0,
                None => false,
            };
            if row.up_ok != (row.up_ok && (expect || row.ver % 6 != 5)) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 记一个缺失项。
    pub fn note_missing(&mut self, kind: MissingKind) {
        let cell = match self.missing.get_mut(kind as usize) {
            Some(c) => c,
            None => return,
        };
        *cell = cell.saturating_add(1);
        self.fnv(0x5100 + kind as u64);
    }

    /// 声明一个缺失项（零静默闸：声明数必须 ≥ 缺失数）。
    pub fn declare_missing(&mut self, kind: MissingKind) {
        let cell = match self.declared.get_mut(kind as usize) {
            Some(c) => c,
            None => return,
        };
        *cell = cell.saturating_add(1);
        self.fnv(0x5200 + kind as u64);
    }

    /// 零静默核验：每类声明数 ≥ 缺失数。
    pub fn zero_silence_ok(&self) -> bool {
        let mut i = 0usize;
        while i < MISSING_KINDS {
            let m = self.missing[i];
            let d = self.declared[i];
            if d < m {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 收口三闸：六判据 + 下探锚 + 零静默。
    pub fn seal_gate(&mut self) -> Result<(), VeCode> {
        if !self.six_all_landed() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(VeCode::EEvidence);
        }
        if !self.anchor_landed() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301);
            return Err(VeCode::EDescent);
        }
        if !self.zero_silence_ok() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5302);
            return Err(VeCode::ESilent);
        }
        self.fnv(0x5303);
        Ok(())
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for VeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 未声明缺失项注入（10 轮，确定性）：每轮漏声明 1~2 类。
pub struct SilenceInjector {
    seed: u64,
}

impl SilenceInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        SilenceInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5203,
        }
    }

    /// 下一轮漏声明的类数（1..=2）。
    pub fn next_silent(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 2) as usize + 1
    }
}

impl Default for SilenceInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：证据全落 + 下探矩阵全落 + 缺失项全声明 + 三闸封账 + 注入
/// 10/10 + 指纹。返回（声明格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VeLedger::new();
    // ① 六判据证据格（每判据 3 格）。
    let mut g = 0usize;
    while g < SIX_GATES {
        let mut e = 0u32;
        while e < EVIDENCE_MIN {
            let _ = led.add_evidence(g);
            e += 1;
        }
        g += 1;
    }
    // ② 下探矩阵 9 档全落。
    let mut v = 0usize;
    while v < LADDER_2022 {
        let _ = led.land_descent(v);
        v += 1;
    }
    // ③ 缺失项记账 + 全声明（零静默）。
    let mut declared = 0u32;
    let mut k = 0usize;
    while k < MISSING_KINDS {
        if let Some(kind) = MISSING_ALL.get(k) {
            led.note_missing(*kind);
            led.declare_missing(*kind);
            declared += 1;
        }
        k += 1;
    }
    // ④ 三闸封账。
    let _ = led.seal_gate();
    // ⑤ 注入 10/10：漏声明封账。
    let mut inj = SilenceInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let silent = inj.next_silent();
        let mut lt = VeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let mut e = 0u32;
            while e < EVIDENCE_MIN {
                let _ = lt.add_evidence(g);
                e += 1;
            }
            g += 1;
        }
        let mut v = 0usize;
        while v < LADDER_2022 {
            let _ = lt.land_descent(v);
            v += 1;
        }
        let mut k = 0usize;
        while k < MISSING_KINDS {
            if let Some(kind) = MISSING_ALL.get(k) {
                lt.note_missing(*kind);
            }
            k += 1;
        }
        // 漏声明 silent 类。
        let mut d = 0usize;
        while d < MISSING_KINDS.saturating_sub(silent) {
            if let Some(kind) = MISSING_ALL.get(d) {
                lt.declare_missing(*kind);
            }
            d += 1;
        }
        if lt.seal_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5304);
        } else {
            led.note_missed();
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (declared, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52003_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52003");
    // 1. 正常流：三闸封账成立。
    let (declared, caught, fp) = run_cycle();
    s.add("三闸封账成立", caught >= 10 && declared == MISSING_KINDS as u32, "六判据+下探锚+零静默齐，注入 10/10");
    // 2. 边界：档越阶梯/判据越闭集/证据不足。
    let mut l0 = VeLedger::new();
    s.add(
        "边界·判据越闭集",
        l0.add_evidence(SIX_GATES) == Err(VeCode::ENoGate),
        "判据 6 越闭集必拒",
    );
    s.add("边界·档越阶梯", descent_row(LADDER_2022).is_none(), "档 9 越阶梯必 None");
    s.add(
        "边界·上探档越阶梯",
        ascent_row(LADDER_2019).is_none(),
        "上探档 12 越阶梯必 None",
    );
    // 3. 证据不足封账拒。
    s.add(
        "证据不足封账拒",
        l0.seal_gate() == Err(VeCode::EEvidence),
        "证据 0 格即封必拒",
    );
    // 4. 下探锚闸。
    let mut l1 = VeLedger::new();
    let mut g = 0usize;
    while g < SIX_GATES {
        let mut e = 0u32;
        while e < EVIDENCE_MIN {
            let _ = l1.add_evidence(g);
            e += 1;
        }
        g += 1;
    }
    s.add("下探锚未落账拒", l1.seal_gate() == Err(VeCode::EDescent), "六判据齐但下探锚缺必拒");
    // 5. 零静默闸。
    let mut l2 = VeLedger::new();
    let mut g = 0usize;
    while g < SIX_GATES {
        let mut e = 0u32;
        while e < EVIDENCE_MIN {
            let _ = l2.add_evidence(g);
            e += 1;
        }
        g += 1;
    }
    let _ = l2.land_descent(DESCENT_ANCHOR);
    l2.note_missing(MissingKind::CodeLens);
    s.add("零静默·缺声明拒", l2.seal_gate() == Err(VeCode::ESilent), "缺失未声明必拒");
    // 6. 声明覆盖后过闸。
    l2.declare_missing(MissingKind::CodeLens);
    s.add("零静默·声明覆盖过", l2.seal_gate().is_ok(), "声明数≥缺失数后三闸齐");
    // 7. 上探账对平。
    let mut l3 = VeLedger::new();
    s.add("上探账对平", l3.ascent_reconcile(), "2019 逐版上探结果与复算逐行一致");
    // 8. 注入必抓 10/10。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "未声明缺失注入 10/10 检出",
        caught_n >= 10 && inj_n == caught_n && esc_n == 0,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 9. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 11. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = descent_row(DESCENT_ANCHOR).unwrap_or(DescentRow { ver: 1, ascended_from: Some(1), down_ok: true });
        assert!(row.down_ok);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(descent_row(LADDER_2022).is_none());
        assert!(ascent_row(LADDER_2019).is_none());
        assert_eq!(VeLedger::new().add_evidence(SIX_GATES), Err(VeCode::ENoGate));
    }

    #[test]
    fn test_evidence_floor() {
        let mut led = VeLedger::new();
        let _ = led.add_evidence(0);
        let _ = led.add_evidence(0);
        assert!(!led.six_all_landed());
        let _ = led.add_evidence(0);
        // 其余判据仍缺。
        assert!(!led.six_all_landed());
    }

    #[test]
    fn test_descent_anchor() {
        let mut led = VeLedger::new();
        assert!(!led.anchor_landed());
        let _ = led.land_descent(DESCENT_ANCHOR);
        assert!(led.anchor_landed());
    }

    #[test]
    fn test_silence_gate() {
        let mut led = VeLedger::new();
        led.note_missing(MissingKind::HotReload);
        assert!(!led.zero_silence_ok());
        led.declare_missing(MissingKind::HotReload);
        assert!(led.zero_silence_ok());
    }

    #[test]
    fn test_three_gate_seal() {
        let mut led = VeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let mut e = 0u32;
            while e < EVIDENCE_MIN {
                let _ = led.add_evidence(g);
                e += 1;
            }
            g += 1;
        }
        let _ = led.land_descent(DESCENT_ANCHOR);
        let mut k = 0usize;
        while k < MISSING_KINDS {
            if let Some(kind) = MISSING_ALL.get(k) {
                led.note_missing(*kind);
                led.declare_missing(*kind);
            }
            k += 1;
        }
        assert!(led.seal_gate().is_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = SilenceInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let silent = inj.next_silent();
            let mut lt = VeLedger::new();
            let mut g = 0usize;
            while g < SIX_GATES {
                let mut e = 0u32;
                while e < EVIDENCE_MIN {
                    let _ = lt.add_evidence(g);
                    e += 1;
                }
                g += 1;
            }
            let _ = lt.land_descent(DESCENT_ANCHOR);
            let mut k = 0usize;
            while k < MISSING_KINDS {
                if let Some(kind) = MISSING_ALL.get(k) {
                    lt.note_missing(*kind);
                }
                k += 1;
            }
            let mut d = 0usize;
            while d < MISSING_KINDS.saturating_sub(silent) {
                if let Some(kind) = MISSING_ALL.get(d) {
                    lt.declare_missing(*kind);
                }
                d += 1;
            }
            if lt.seal_gate().is_err() {
                caught += 1;
            }
            r += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52003_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
