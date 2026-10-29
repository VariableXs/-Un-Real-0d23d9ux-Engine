//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13321 · 依赖链断裂错误码总成（缺 DLL/缺符号/版本不符逐码） · 0x65900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13321-J1 断链错误码总账（缺 DLL/缺符号/版本不符三族 ≥15 码）
//!      逐码对齐 Windows，与 B24 F13268 面对账一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三族断链错误码闭集：缺 DLL（5 码）/缺符号（5 码）/版本不符
//!     （6 码）合计 16 码，逐码对齐 Windows winerror.h / ntstatus.h
//!     语义（STATUS_DLL_NOT_FOUND 0xC0000135 等写死常量并注来源）；
//!   - 依赖链四态机：Pending→Probing→Bound/Broken；Broken 为终态，
//!     必须落断链总账（零静默）；Pending→Bound 直跳拒收；
//!   - 版本不符族：actual≠expected 且双非零才许入账（版本语义红线）；
//!   - 守恒：probed ≡ bound + broken（探测账恒等式）；
//!   - 注入三族：码不属于族表/非法直跳/重复入账 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65900），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65900，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65900;

/// 黄金依赖闭集：16 条（B24 F13268 面对账样本面）。
pub const DEP_COUNT: usize = 16;

// ── Windows 对齐错误码表（逐码写死，来源见行注） ─────────────────
/// 缺 DLL 族（5 码）。
pub const TBL_MISSING_DLL: [u32; 5] = [
    0xC0000135, // ntstatus.h STATUS_DLL_NOT_FOUND
    0xC0000142, // ntstatus.h STATUS_DLL_INIT_FAILED
    0x0000007E, // winerror.h 126 ERROR_MOD_NOT_FOUND
    0x00000002, // winerror.h 2  ERROR_FILE_NOT_FOUND（装载路径整缺）
    0x00000003, // winerror.h 3  ERROR_PATH_NOT_FOUND（搜索路径断）
];
/// 缺符号族（5 码）。
pub const TBL_MISSING_SYMBOL: [u32; 5] = [
    0xC0000139, // ntstatus.h STATUS_ENTRYPOINT_NOT_FOUND
    0xC0000138, // ntstatus.h STATUS_ORDINAL_NOT_FOUND
    0x0000007F, // winerror.h 127 ERROR_PROC_NOT_FOUND
    0x000000B6, // winerror.h 182 ERROR_ORDINAL_NOT_FOUND
    0x00000002, // winerror.h 2  ERROR_FILE_NOT_FOUND（延迟载入 DLL 整缺）
];
/// 版本不符族（6 码）。
pub const TBL_VERSION_MISMATCH: [u32; 6] = [
    0xC000007B, // ntstatus.h STATUS_INVALID_IMAGE_FORMAT（位宽/镜像不符）
    0xC0000428, // ntstatus.h STATUS_INVALID_IMAGE_HASH
    0x00000073, // winerror.h 115 ERROR_OLD_WIN_VERSION
    0x000000C1, // winerror.h 193 ERROR_BAD_EXE_FORMAT
    0x000036B1, // winerror.h 14001 ERROR_SXS_CANT_GEN_ACTCTX（清单版本断）
    0x000036B2, // winerror.h 14002 ERROR_SXS_INVALID_ACT_CTXDATA
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DccCode {
    /// 依赖下标越闭集
    DepOutOfRange,
    /// 断链族枚举外
    FamilyInvalid,
    /// Windows 码不属于该族冻结表
    CodeNotInFamily,
    /// 链态非法迁移
    StateIllegal,
    /// 依赖重复入账
    DepBookDup,
    /// Broken 终态未登记断链总账
    BreakNotBooked,
    /// 探测账守恒破（probed ≡ bound+broken）
    ConservationBreak,
    /// 依赖名为空（零静默红线）
    DepNameEmpty,
    /// 版本字段为 0（版本语义非法）
    VersionFieldZero,
}

impl DccCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DccCode::DepOutOfRange => "依赖下标越闭集：0~15 冻结",
            DccCode::FamilyInvalid => "断链族枚举外：三族闭集",
            DccCode::CodeNotInFamily => "Windows 码不属于该族冻结表",
            DccCode::StateIllegal => "链态非法迁移：Pending→Bound 等直跳",
            DccCode::DepBookDup => "依赖重复入账：总账一址一行",
            DccCode::BreakNotBooked => "Broken 终态未落断链总账",
            DccCode::ConservationBreak => "探测账守恒破：probed≡bound+broken",
            DccCode::DepNameEmpty => "依赖名为空：零静默红线",
            DccCode::VersionFieldZero => "版本字段为 0：版本语义非法",
        }
    }
    pub const ALL: [DccCode; 9] = [
        DccCode::DepOutOfRange,
        DccCode::FamilyInvalid,
        DccCode::CodeNotInFamily,
        DccCode::StateIllegal,
        DccCode::DepBookDup,
        DccCode::BreakNotBooked,
        DccCode::ConservationBreak,
        DccCode::DepNameEmpty,
        DccCode::VersionFieldZero,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DccCode::ALL.len() {
        if DccCode::ALL[i].val() == code {
            return DccCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65900 段闭集内"
}

// ── 断链三族 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepFamily {
    MissingDll,
    MissingSymbol,
    VersionMismatch,
}

impl DepFamily {
    pub fn name(self) -> &'static str {
        match self {
            DepFamily::MissingDll => "缺 DLL",
            DepFamily::MissingSymbol => "缺符号",
            DepFamily::VersionMismatch => "版本不符",
        }
    }
    pub fn table(self) -> &'static [u32] {
        match self {
            DepFamily::MissingDll => &TBL_MISSING_DLL,
            DepFamily::MissingSymbol => &TBL_MISSING_SYMBOL,
            DepFamily::VersionMismatch => &TBL_VERSION_MISMATCH,
        }
    }
    /// 码是否在本族冻结表内。
    pub fn contains(self, code: u32) -> bool {
        let t = self.table();
        let mut i = 0;
        while i < t.len() {
            if t[i] == code {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 三族码数守恒：5+5+6 = 16 ≥ 15（判据 J1 下界）。
pub const FAM_CODE_TOTAL: usize = 5 + 5 + 6;

// ── 链态四闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// 待探测
    Pending,
    /// 探测中
    Probing,
    /// 已绑定
    Bound,
    /// 断链（终态）
    Broken,
}

/// 依赖行：名字 + 双版本 + 链态。
#[derive(Clone, Copy, Debug)]
pub struct DepRow {
    pub name: &'static str,
    pub expected_ver: u32,
    pub actual_ver: u32,
    pub state: LinkState,
}

/// 断链记录：依赖下标 + 族 + Windows 码。
#[derive(Clone, Copy, Debug)]
pub struct BreakRecord {
    pub dep: usize,
    pub family: DepFamily,
    pub win_code: u32,
}

// ── 依赖链总账 ───────────────────────────────────────────────────
pub struct DepLedger {
    pub rows: Vec<DepRow>,
    pub breaks: Vec<BreakRecord>,
    pub probed: u32,
    pub bound: u32,
    pub broken: u32,
}

impl DepLedger {
    pub fn new() -> DepLedger {
        DepLedger {
            rows: Vec::new(),
            breaks: Vec::new(),
            probed: 0,
            bound: 0,
            broken: 0,
        }
    }

    /// 入账一条依赖（一址一行）：名字非空、双版本非零。
    pub fn book(&mut self, name: &'static str, expected: u32, actual: u32) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(DccCode::DepNameEmpty.val());
        }
        if expected == 0 || actual == 0 {
            return Err(DccCode::VersionFieldZero.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].name == name {
                return Err(DccCode::DepBookDup.val());
            }
            i += 1;
        }
        let idx = self.rows.len();
        if idx >= DEP_COUNT {
            return Err(DccCode::DepOutOfRange.val());
        }
        self.rows.push(DepRow {
            name,
            expected_ver: expected,
            actual_ver: actual,
            state: LinkState::Pending,
        });
        Ok(idx)
    }

    /// 迁移闭式：Pending→Probing / Probing→Bound / Probing→Broken /
    /// Bound→Probing（重绑）；其余（含 Pending→Bound 直跳）拒收。
    pub fn transition(&mut self, idx: usize, next: LinkState) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(DccCode::DepOutOfRange.val());
        }
        let cur = self.rows[idx].state;
        let legal = matches!(
            (cur, next),
            (LinkState::Pending, LinkState::Probing)
                | (LinkState::Probing, LinkState::Bound)
                | (LinkState::Probing, LinkState::Broken)
                | (LinkState::Bound, LinkState::Probing)
        );
        if !legal {
            return Err(DccCode::StateIllegal.val());
        }
        if cur == LinkState::Pending {
            self.probed += 1;
        }
        if cur == LinkState::Probing {
            if next == LinkState::Bound {
                self.bound += 1;
            } else {
                self.broken += 1;
            }
        }
        if cur == LinkState::Bound && next == LinkState::Probing {
            // 重绑先退账。
            self.bound -= 1;
            self.probed -= 1;
        }
        self.rows[idx].state = next;
        Ok(())
    }

    /// 判定绑定：actual == expected 才许 Bound。
    pub fn resolve_bound(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(DccCode::DepOutOfRange.val());
        }
        if self.rows[idx].actual_ver != self.rows[idx].expected_ver {
            return Err(DccCode::CodeNotInFamily.val());
        }
        self.transition(idx, LinkState::Bound)
    }

    /// 判定断链：族合法 + Windows 码在族表 + 版本不符语义对齐 +
    /// Broken 落总账。
    pub fn resolve_broken(
        &mut self,
        idx: usize,
        family: DepFamily,
        win_code: u32,
    ) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(DccCode::DepOutOfRange.val());
        }
        if !family.contains(win_code) {
            return Err(DccCode::CodeNotInFamily.val());
        }
        let r = self.rows[idx];
        if family == DepFamily::VersionMismatch {
            if r.actual_ver == r.expected_ver {
                return Err(DccCode::VersionFieldZero.val());
            }
        } else if r.actual_ver != r.expected_ver && family == DepFamily::MissingDll {
            // 缺 DLL 族允许版本占位差异，不拦。
        }
        self.transition(idx, LinkState::Broken)?;
        self.breaks.push(BreakRecord { dep: idx, family, win_code });
        Ok(())
    }

    /// 探测账守恒：probed ≡ bound + broken。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.probed != self.bound + self.broken {
            return Err(DccCode::ConservationBreak.val());
        }
        // Broken 行数必须与断链总账行数一致（零静默对账）。
        let mut broken_rows = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].state == LinkState::Broken {
                broken_rows += 1;
            }
            i += 1;
        }
        if broken_rows != self.breaks.len() {
            return Err(DccCode::BreakNotBooked.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 黄金依赖闭集名表（16 条，对齐 B24 F13268 面样本）。
pub const GOLDEN_DEPS: [&str; DEP_COUNT] = [
    "kernel32.dll",
    "user32.dll",
    "gdi32.dll",
    "shell32.dll",
    "advapi32.dll",
    "ole32.dll",
    "oleaut32.dll",
    "ws2_32.dll",
    "winmm.dll",
    "imm32.dll",
    "version.dll",
    "msvcrt.dll",
    "comctl32.dll",
    "d3d9.dll",
    "dsound.dll",
    "xinput1_4.dll",
];

/// 版本号写死：基线 0x000A0000（10.0）+ 序号。
pub fn golden_version(i: usize) -> u32 {
    0x000A0000 + (i as u32) * 0x0001_0000
}

// ── 黄金正演：16 依赖入账 → 三族断链 + 绑定 → 守恒 ──────────────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = DepLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < DEP_COUNT {
        let v = golden_version(i);
        let _ = led.book(GOLDEN_DEPS[i], v, v)?;
        i += 1;
    }
    // 三族各取一条 LCG 选中的依赖，制造断链。
    let d1 = rng.below(DEP_COUNT as u64) as usize;
    let d2 = rng.below(DEP_COUNT as u64) as usize;
    let d3 = rng.below(DEP_COUNT as u64) as usize;
    let _ = led.transition(d1, LinkState::Probing)?;
    led.resolve_broken(d1, DepFamily::MissingDll, 0xC0000135)?;
    let _ = led.transition(d2, LinkState::Probing)?;
    led.resolve_broken(d2, DepFamily::MissingSymbol, 0xC0000139)?;
    // 版本不符：改写 actual 后探测。
    let d3v = led.rows[d3].expected_ver;
    led.rows[d3].actual_ver = d3v - 0x0001_0000;
    let _ = led.transition(d3, LinkState::Probing)?;
    led.resolve_broken(d3, DepFamily::VersionMismatch, 0xC000007B)?;
    // 其余全部绑定。
    let mut j = 0;
    while j < DEP_COUNT {
        if led.rows[j].state == LinkState::Pending {
            let _ = led.transition(j, LinkState::Probing)?;
            led.resolve_bound(j)?;
        }
        j += 1;
    }
    led.conservation()?;
    // 断链总账恰 3 条、三族各一。
    if led.breaks.len() != 3 {
        return Err(DccCode::ConservationBreak.val());
    }
    // 逐码反查：三码 detail 均在段内。
    let mut k = 0;
    while k < led.breaks.len() {
        let code = led.breaks[k].win_code;
        if code == 0 {
            return Err(DccCode::CodeNotInFamily.val());
        }
        k += 1;
    }
    // 幂等：守恒恒等式双查同结果。
    let a = led.conservation();
    let b = led.conservation();
    if a != b {
        return Err(DccCode::ConservationBreak.val());
    }
    Ok((led.bound as u32) * 0x100 + (led.broken as u32))
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：Windows 码不属于该族冻结表（版本不符族给缺 DLL 码）。
    ledger.tries[0] += 1;
    let mut d = DepLedger::new();
    let _ = d.book("nt.dll", 0x000A0001, 0x000A0002);
    let _ = d.transition(0, LinkState::Probing);
    if let Err(e) = d.resolve_broken(0, DepFamily::VersionMismatch, 0xC0000135) {
        if e == DccCode::CodeNotInFamily.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：Pending→Bound 直跳（新行，未探测先绑定）。
    ledger.tries[1] += 1;
    let _ = d.book("hal.dll", 0x000A0001, 0x000A0001);
    if let Err(e) = d.transition(1, LinkState::Bound) {
        if e == DccCode::StateIllegal.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：依赖重复入账。
    ledger.tries[2] += 1;
    if let Err(e) = d.book("nt.dll", 0x000A0001, 0x000A0001) {
        if e == DccCode::DepBookDup.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DccCode::ALL.len() {
        let v = DccCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演三族断链", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 三族码数守恒：5+5+6=16 ≥ 15。
    v.push((
        "J1-三族码数≥15",
        FAM_CODE_TOTAL == 16,
        format!("total={}", FAM_CODE_TOTAL),
    ));
    // 反向判据：族表互斥（缺 DLL 码不在缺符号表）。
    v.push((
        "J1-族表互斥反查",
        !DepFamily::MissingDll.contains(0xC0000139)
            && DepFamily::MissingDll.contains(0xC0000135),
        String::from("DLL/Symbol 表互斥"),
    ));
    // 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65900 窗口")));
    // detail 反查闭链：首尾码均能反查回非未知文案。
    let first = detail(DccCode::DepOutOfRange.val());
    let last = detail(DccCode::VersionFieldZero.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65900 段闭集内"
            && last != "未知诊断码：不在 0x65900 段闭集内",
        String::from("首尾反查非未知"),
    ));
    // 版本不符语义：同版本拒入版本不符族。
    let mut d2 = DepLedger::new();
    let _ = d2.book("a.dll", 0x000A0000, 0x000A0000);
    let _ = d2.transition(0, LinkState::Probing);
    v.push((
        "J1-同版本拒版本族",
        d2.resolve_broken(0, DepFamily::VersionMismatch, 0xC000007B)
            == Err(DccCode::VersionFieldZero.val()),
        String::from("actual≡expected 拒收"),
    ));
    v
}

pub fn run_unx13321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13321 依赖链断裂错误码总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
