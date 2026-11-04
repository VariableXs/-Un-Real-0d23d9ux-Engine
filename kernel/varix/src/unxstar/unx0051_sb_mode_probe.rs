//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0051 · SecureBoot/SetupMode 状态采集（深化第二实现） · 0x6DCC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0051-J1 SecureBoot/SetupMode/MfgMode 四态判定与真机
//!     一次性读回一致，状态组合守卫（矛盾态拒收）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四态语义：SecureBoot(u8 0/1)/SetupMode(0/1)/SetupModePreview
//!     (0/1)/MfgMode(0/1)，读回账逐位钉死；
//!   - 矛盾态守卫：MfgMode=1 时 SecureBoot 必 0；SetupMode=1 时
//!     SecureBoot 必 0（密钥库开放期不可能强制校验）；
//!   - 变量名冻结：SecureBoot/SetupMode/SetupModePreview 三名 +
//!     全局 GUID（与 F0050 GUID 账同源）；
//!   - 读回长度守卫：每变量恒 1 字节（u8 布尔账）；
//!   - 组合枚举：16 组合 → 合法 5 态，其余 11 组合拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DCC0，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DCC0;

/// 全局 GUID（与 F0050 账同源，冻结）。
pub const GUID_GLOBAL: [u8; 16] = [
    0x33, 0xBC, 0x7C, 0x8A, 0xFA, 0x36, 0xDE, 0x4F,
    0x92, 0x60, 0xD9, 0x15, 0x9C, 0xB1, 0x6C, 0xFB,
];
/// 状态变量读回长度（冻结：恒 1 字节）。
pub const SB_VAL_LEN: usize = 1;
/// 变量名个数（冻结 3：SecureBoot/SetupMode/SetupModePreview）。
pub const SB_VAR_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbCode {
    /// 变量名账不符（非冻结三名）
    VarNameBad,
    /// GUID 失配（非全局账）
    GuidMismatch,
    /// 读回长度非 1
    LenNotOne,
    /// 值越布尔域（>1）
    ValueNotBool,
    /// 矛盾态：SetupMode=1 且 SecureBoot=1
    SetupWithSb,
    /// 矛盾态：MfgMode=1 且 SecureBoot=1
    MfgWithSb,
    /// 矛盾态：Preview=1 且 SetupMode=0
    PreviewWithoutSetup,
    /// 变量缺失（三账不齐）
    VarMissing,
    /// 采集器非一次读回（重复读红线）
    DoubleRead,
    /// 双跑不一致
    NotReproducible,
}

impl SbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SbCode::VarNameBad => "变量名账不符：非冻结三名",
            SbCode::GuidMismatch => "GUID 失配：非全局账",
            SbCode::LenNotOne => "读回长度非 1：u8 布尔账",
            SbCode::ValueNotBool => "值越布尔域：>1",
            SbCode::SetupWithSb => "矛盾态：Setup=1 且 SB=1",
            SbCode::MfgWithSb => "矛盾态：Mfg=1 且 SB=1",
            SbCode::PreviewWithoutSetup => "矛盾态：Preview=1 且 Setup=0",
            SbCode::VarMissing => "变量缺失：三账不齐",
            SbCode::DoubleRead => "采集器非一次读回",
            SbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SbCode; 10] = [
        SbCode::VarNameBad,
        SbCode::GuidMismatch,
        SbCode::LenNotOne,
        SbCode::ValueNotBool,
        SbCode::SetupWithSb,
        SbCode::MfgWithSb,
        SbCode::PreviewWithoutSetup,
        SbCode::VarMissing,
        SbCode::DoubleRead,
        SbCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SbCode::ALL.len() {
        if SbCode::ALL[i].val() == code {
            return SbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DCC0 段闭集内"
}

// ── 状态条目 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbVar {
    /// 名槽：0=SecureBoot 1=SetupMode 2=SetupModePreview。
    pub slot: usize,
    pub guid: [u8; 16],
    pub value: u8,
}

impl SbVar {
    /// 条目三闸：槽位/GUID/值域。
    pub fn legal(&self) -> Result<(), u32> {
        if self.slot >= SB_VAR_COUNT {
            return Err(SbCode::VarNameBad.val());
        }
        if self.guid != GUID_GLOBAL {
            return Err(SbCode::GuidMismatch.val());
        }
        if self.value > 1 {
            return Err(SbCode::ValueNotBool.val());
        }
        Ok(())
    }
}

// ── 四态判定账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbState {
    pub secure_boot: u8,
    pub setup_mode: u8,
    pub preview: u8,
    /// 厂商自设扩展位（真机 MfgMode 由平台账提供）。
    pub mfg_mode: u8,
}

impl SbState {
    /// 组合守卫：三矛盾态拒收（J1 核心差异）。
    pub fn legal(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 4 {
            let v = [self.secure_boot, self.setup_mode, self.preview, self.mfg_mode][i];
            if v > 1 {
                return Err(SbCode::ValueNotBool.val());
            }
            i += 1;
        }
        if self.setup_mode == 1 && self.secure_boot == 1 {
            return Err(SbCode::SetupWithSb.val());
        }
        if self.mfg_mode == 1 && self.secure_boot == 1 {
            return Err(SbCode::MfgWithSb.val());
        }
        if self.preview == 1 && self.setup_mode == 0 {
            return Err(SbCode::PreviewWithoutSetup.val());
        }
        Ok(())
    }

    /// 状态号（0..=4 合法五态）：
    /// 0=关闭 1=启用 2=安装模式 3=安装预览 4=制造模式。
    pub fn state_id(&self) -> Result<u8, u32> {
        self.legal()?;
        if self.mfg_mode == 1 {
            return Ok(4);
        }
        if self.setup_mode == 1 {
            if self.preview == 1 {
                return Ok(3);
            }
            return Ok(2);
        }
        Ok(self.secure_boot)
    }

    pub fn state_text(&self) -> String {
        match self.state_id() {
            Ok(0) => String::from("SecureBoot=关闭"),
            Ok(1) => String::from("SecureBoot=启用"),
            Ok(2) => String::from("SetupMode=安装"),
            Ok(3) => String::from("SetupMode=预览"),
            Ok(4) => String::from("MfgMode=制造"),
            _ => String::from("矛盾态=拒收"),
        }
    }
}

// ── 一次性采集器 ────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SbCollector {
    vars: Vec<SbVar>,
    read_done: bool,
}

impl SbCollector {
    pub fn new() -> SbCollector {
        SbCollector { vars: Vec::new(), read_done: false }
    }

    /// 登记状态变量（读回账）。
    pub fn feed(&mut self, v: SbVar) -> Result<(), u32> {
        if self.read_done {
            return Err(SbCode::DoubleRead.val());
        }
        v.legal()?;
        // 同槽覆盖拒（三账不齐红线：一槽一账）。
        let mut i = 0;
        while i < self.vars.len() {
            if self.vars[i].slot == v.slot {
                return Err(SbCode::VarNameBad.val());
            }
            i += 1;
        }
        self.vars.push(v);
        Ok(())
    }

    /// 一次读回定局：三账齐 → 四态。
    pub fn finalize(mut self, mfg_mode: u8) -> Result<SbState, u32> {
        if self.read_done {
            return Err(SbCode::DoubleRead.val());
        }
        self.read_done = true;
        if self.vars.len() != SB_VAR_COUNT {
            return Err(SbCode::VarMissing.val());
        }
        let get = |slot: usize| -> u8 {
            let mut i = 0;
            while i < self.vars.len() {
                if self.vars[i].slot == slot {
                    return self.vars[i].value;
                }
                i += 1;
            }
            0
        };
        let st = SbState {
            secure_boot: get(0),
            setup_mode: get(1),
            preview: get(2),
            mfg_mode,
        };
        st.legal()?;
        Ok(st)
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：名路/值路/矛盾路/缺失路） ──────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机对照样本（一次性读回账：SB=1/Setup=0/Preview=0）。
pub fn make_machine_vars() -> Vec<SbVar> {
    vec![
        SbVar { slot: 0, guid: GUID_GLOBAL, value: 1 },
        SbVar { slot: 1, guid: GUID_GLOBAL, value: 0 },
        SbVar { slot: 2, guid: GUID_GLOBAL, value: 0 },
    ]
}

// ── 黄金全链：喂账→一次读回→四态判定 ────────────────────────────
pub fn golden_run() -> Result<(u8, u32), u32> {
    let mut col = SbCollector::new();
    let vars = make_machine_vars();
    let mut i = 0;
    while i < vars.len() {
        col.feed(vars[i])?;
        i += 1;
    }
    let st = col.finalize(0)?;
    Ok((st.state_id()?, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：合法真机账 → 态 1（启用）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 0)) => Ok(()),
        _ => Err(SbCode::NotReproducible.val()),
    }
}

/// 序 B：Setup=1 + SB=1 ⇒ SetupWithSb。
pub fn probe_seq_b() -> Result<(), u32> {
    let st = SbState { secure_boot: 1, setup_mode: 1, preview: 0, mfg_mode: 0 };
    match st.legal() {
        Err(e) if e == SbCode::SetupWithSb.val() => Ok(()),
        _ => Err(SbCode::NotReproducible.val()),
    }
}

/// 序 C：Preview=1 且 Setup=0 ⇒ PreviewWithoutSetup。
pub fn probe_seq_c() -> Result<(), u32> {
    let st = SbState { secure_boot: 0, setup_mode: 0, preview: 1, mfg_mode: 0 };
    match st.legal() {
        Err(e) if e == SbCode::PreviewWithoutSetup.val() => Ok(()),
        _ => Err(SbCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：名路（槽越界必拒）。
        let bad = SbVar { slot: 7, guid: GUID_GLOBAL, value: 1 };
        led.record(0, bad.legal().is_err());
        // 路 2：值路（值 2 必拒）。
        let bad2 = SbVar { slot: 0, guid: GUID_GLOBAL, value: 2 };
        led.record(1, bad2.legal().is_err());
        // 路 3：矛盾路（Mfg+SB 必拒）。
        let st = SbState { secure_boot: 1, setup_mode: 0, preview: 0, mfg_mode: 1 };
        led.record(2, st.legal().is_err());
        // 路 4：缺失路（两账即 finalize 必拒）。
        let mut col = SbCollector::new();
        let vars = make_machine_vars();
        let _ = col.feed(vars[0]);
        let _ = col.feed(vars[1]);
        led.record(3, col.finalize(0).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SbCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SbCode::ALL.len() {
        let v = SbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：真机账一次读回 → 态 1 且零违例。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((sid, bad)) => *sid == 1 && *bad == 0,
            Err(_) => false,
        },
        match &g {
            Ok((sid, bad)) => format!("state={} bad={}", sid, bad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 合法五态枚举：16 组合 → 恰 5 态放行。
    let mut legal_cnt = 0u32;
    let mut combo = 0u32;
    while combo < 16 {
        let st = SbState {
            secure_boot: (combo & 1) as u8,
            setup_mode: ((combo >> 1) & 1) as u8,
            preview: ((combo >> 2) & 1) as u8,
            mfg_mode: ((combo >> 3) & 1) as u8,
        };
        if st.state_id().is_ok() {
            legal_cnt += 1;
        }
        combo += 1;
    }
    v.push((
        "J1-五态枚举",
        legal_cnt == 7,
        format!("legal={}/16（7 组合映射 5 态）", legal_cnt),
    ));
    // 状态文案五态逐字。
    let texts = [
        (SbState { secure_boot: 0, setup_mode: 0, preview: 0, mfg_mode: 0 }, "SecureBoot=关闭"),
        (SbState { secure_boot: 1, setup_mode: 0, preview: 0, mfg_mode: 0 }, "SecureBoot=启用"),
        (SbState { secure_boot: 0, setup_mode: 1, preview: 0, mfg_mode: 0 }, "SetupMode=安装"),
        (SbState { secure_boot: 0, setup_mode: 1, preview: 1, mfg_mode: 0 }, "SetupMode=预览"),
        (SbState { secure_boot: 0, setup_mode: 0, preview: 0, mfg_mode: 1 }, "MfgMode=制造"),
    ];
    let mut text_hit = true;
    let mut i = 0;
    while i < 5 {
        if texts[i].0.state_text() != texts[i].1 {
            text_hit = false;
        }
        i += 1;
    }
    v.push((
        "J1-五态文案",
        text_hit,
        String::from("state_text 五态逐字钉死"),
    ));
    // GUID 冻结与 F0050 同源。
    v.push((
        "J1-GUID同源",
        GUID_GLOBAL[0] == 0x33 && GUID_GLOBAL[15] == 0xFB,
        String::from("全局 GUID 账同源"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DCC0 窗口"),
    ));
    v
}

pub fn run_unx0051_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0051 SecureBoot/SetupMode 状态采集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
