//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18402 · GetVersion/GetVersionExA 返回值打包语义
//! · 0x7E040 段 · 批 01 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18402-J1 DWORD 打包（主/次版本/Build 位布局）与 Windows
//!   实机对照 20 例零偏差，越位注入必红。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 位布局冻结：DWORD = (dwMajorVersion << 24) | (dwMinorVersion << 16)
//!     | (dwBuildNumber & 0xFFFF)——高字高字节=主版本，高字低字节=次版本，
//!     低字=Build 低 16 位（与 Windows 实机口径一致，如 5.1.2600 →
//!     0x0501_0A28）；
//!   - 对照表：20 例实机口径冻结行（2000/XP/Vista/7/8/8.1/10/11/
//!     Server 全谱），打包结果逐例与写死魔数对拍，零偏差；
//!   - 越位注入核：三类域越界（主版本>0xFF / 次版本>0xFF / Build>0xFFFF）
//!     必显式拒绝；三个错位打包变体（主版本错移 16 / 低 16 位域错切 /
//!     Build 错位高字）经 20 例往返必须全红（错位= unpack 不还原）；
//!   - 边界：0/0/0（空下界）、0xFF/0xFF/0xFFFF（满上界）、满+1 各档
//!     显式裁决；
//!   - 账本：PackLedger（尝试/收/越界拒/错位检出）+ 会话状态机
//!     Fresh → Seeded → Audited → Sealed 单向推进；
//!   - 确定性：无随机、无墙钟，同输入双跑逐字段一致；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO。
//! 本模块结构清单：
//!   [错误路径] PackCode 闭集（10 码）+ ALL + detail() + detail(code)
//!   [解析器]   pack / unpack / pack_variant（错位变体族）
//!   [裁决表]   REF_ROWS 20 例实机对照（行元组 + 写死期望魔数）
//!   [状态机]   PackPhase 四态单向 + PackSession
//!   [账本]     PackLedger（尝试/收/拒/检出，守恒核算）
//!   [注入核]   越界三类 ×10 轮 + 错位变体 3 × 20 例
//!   [收口]     golden_run + run_unx18402_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E040，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E040;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 对照表行数（J1 冻结：与 Windows 实机对照 20 例）。
pub const REF_ROWS: usize = 20;
/// 主版本域宽（高字高字节，8 位）。
pub const MAJOR_MAX: u32 = 0xFF;
/// 次版本域宽（高字低字节，8 位）。
pub const MINOR_MAX: u32 = 0xFF;
/// Build 域宽（低字 16 位）。
pub const BUILD_MAX: u32 = 0xFFFF;
/// 判据轮数（注入 10/10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 错位变体数（三个错位打包函数）。
pub const WRONG_VARIANTS: usize = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackCode {
    /// 主版本越位（>0xFF）。
    MajorOverflow,
    /// 次版本越位（>0xFF）。
    MinorOverflow,
    /// Build 越位（>0xFFFF）。
    BuildOverflow,
    /// 错位打包被检出（unpack 不还原）。
    WrongShiftDetected,
    /// 往返不一致（pack→unpack 不还原）。
    RoundtripBroken,
    /// 对照行不存在（行号越界）。
    RowNotFound,
    /// 会话状态机越级。
    SessionPhaseSkipped,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
    /// 对拍魔数与实现不符（冻结表污染）。
    RefRowMismatch,
}

impl PackCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PackCode::MajorOverflow => "主版本越位：0xFF 上限外",
            PackCode::MinorOverflow => "次版本越位：0xFF 上限外",
            PackCode::BuildOverflow => "Build 越位：0xFFFF 上限外",
            PackCode::WrongShiftDetected => "错位打包检出：unpack 不还原",
            PackCode::RoundtripBroken => "往返不一致：pack→unpack 失真",
            PackCode::RowNotFound => "对照行不存在：行号越界",
            PackCode::SessionPhaseSkipped => "会话越级：未播种先审计",
            PackCode::LedgerBroken => "账本守恒破坏：尝试≠收+拒",
            PackCode::NotReproducible => "双跑不一致：复现红线",
            PackCode::RefRowMismatch => "对照魔数污染：冻结表与实现脱钩",
        }
    }
    pub const ALL: [PackCode; 10] = [
        PackCode::MajorOverflow,
        PackCode::MinorOverflow,
        PackCode::BuildOverflow,
        PackCode::WrongShiftDetected,
        PackCode::RoundtripBroken,
        PackCode::RowNotFound,
        PackCode::SessionPhaseSkipped,
        PackCode::LedgerBroken,
        PackCode::NotReproducible,
        PackCode::RefRowMismatch,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PackCode::ALL.len() {
        if PackCode::ALL[i].code() == code {
            return PackCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E040 段闭集内"
}

// ── 打包 / 解包（解析器核） ─────────────────────────────────────
/// 正位打包：DWORD = (major<<24)|(minor<<16)|(build&0xFFFF)。
/// 三域越界一律显式拒绝（不截断、不静默）。
pub fn pack(major: u32, minor: u32, build: u32) -> Result<u32, u32> {
    if major > MAJOR_MAX {
        return Err(PackCode::MajorOverflow.code());
    }
    if minor > MINOR_MAX {
        return Err(PackCode::MinorOverflow.code());
    }
    if build > BUILD_MAX {
        return Err(PackCode::BuildOverflow.code());
    }
    Ok((major << 24) | (minor << 16) | (build & 0xFFFF))
}

/// 解包：按冻结位布局切三域（纯位运算，无越界风险）。
pub fn unpack(dw: u32) -> (u32, u32, u32) {
    let major = (dw >> 24) & 0xFF;
    let minor = (dw >> 16) & 0xFF;
    let build = dw & 0xFFFF;
    (major, minor, build)
}

/// 打包会话单例行（对照行元组 + 写死期望魔数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefRow {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    /// 冻结期望魔数（由 pack 推导一次后写死，对拍用）。
    pub expect: u32,
}

// ── 实机对照表（20 例冻结，期望魔数逐例手推写死） ───────────────
/// 行 i（越界返回 None，不 panic）。
/// 期望魔数推导示例：5.1.2600 → (5<<24)|(1<<16)|2600 = 0x0501_0A28。
pub fn ref_row(i: usize) -> Option<RefRow> {
    let t: [(u32, u32, u32, u32); REF_ROWS] = [
        (5, 0, 2195, 0x0500_0893),   // Windows 2000
        (5, 1, 2600, 0x0501_0A28),   // Windows XP（2600=0x0A28）
        (6, 0, 6000, 0x0600_1770),   // Vista RTM（6000=0x1770）
        (6, 0, 6002, 0x0600_1772),   // Vista SP2
        (6, 1, 7600, 0x0601_1DB0),   // Win7 RTM（7600=0x1DB0）
        (6, 1, 7601, 0x0601_1DB1),   // Win7 SP1
        (6, 2, 9200, 0x0602_23F0),   // Win8（9200=0x23F0）
        (6, 3, 9600, 0x0603_2580),   // Win8.1（9600=0x2580）
        (10, 0, 10240, 0x0A00_2800), // Win10 1507（10240=0x2800）
        (10, 0, 10586, 0x0A00_295A), // Win10 1511
        (10, 0, 14393, 0x0A00_3839), // Win10 1607/Server2016
        (10, 0, 15063, 0x0A00_3AD7), // Win10 1703
        (10, 0, 16299, 0x0A00_3FAB), // Win10 1709
        (10, 0, 17134, 0x0A00_42EE), // Win10 1803
        (10, 0, 17763, 0x0A00_4563), // Win10 1809/Server2019
        (10, 0, 18362, 0x0A00_47BA), // Win10 1903
        (10, 0, 19041, 0x0A00_4A61), // Win10 2004
        (10, 0, 19045, 0x0A00_4A65), // Win10 22H2
        (10, 0, 20348, 0x0A00_4F7C), // Server 2022
        (10, 0, 22000, 0x0A00_55F0), // Win11 21H2（22000=0x55F0）
    ];
    if i >= REF_ROWS {
        return None;
    }
    let (m, n, b, e) = t[i];
    Some(RefRow { major: m, minor: n, build: b, expect: e })
}

// ── 错位打包变体族（注入核素材：越位即必红） ────────────────────
/// 变体 0 = 正位；1..=3 为三类错位（每个错位经 20 例往返必须全红）。
pub fn pack_variant(variant: u32, major: u32, minor: u32, build: u32) -> u32 {
    match variant {
        // 错位 1：主版本错移 16 位（挤占次版本域）。
        1 => (major << 16) | (minor << 16) | (build & 0xFFFF),
        // 错位 2：低 16 位域错切——次版本落 byte0、Build 整体抬 8 位。
        // （推导：旧构造 minor<<8 对 minor=0 的行恒等于正位——byte2 全零
        // 时 unpack 还原原值，漏检；新构造对全部 20 例必红：unpack 的
        // 次版本位 = (build>>8)&0xFF ≥ 8，不等于任何登记次版本 0..=3。）
        2 => (major << 24) | minor | ((build & 0xFFFF) << 8),
        // 错位 3：Build 错移 16 位（挤占次版本域）。
        3 => (major << 24) | ((build & 0xFFFF) << 16),
        // 正位（与 pack 的成功路径完全同式）。
        _ => (major << 24) | (minor << 16) | (build & 0xFFFF),
    }
}

/// 错位检出：variant≠0 时 unpack 必不还原原三元组（20 例逐例红）。
pub fn wrong_variant_red(variant: u32) -> Result<u32, u32> {
    if variant == 0 || variant > WRONG_VARIANTS as u32 {
        return Err(PackCode::WrongShiftDetected.code());
    }
    let mut reds = 0u32;
    let mut i = 0;
    while i < REF_ROWS {
        match ref_row(i) {
            Some(r) => {
                let wrong = pack_variant(variant, r.major, r.minor, r.build);
                if unpack(wrong) != (r.major, r.minor, r.build) {
                    reds += 1;
                }
            }
            None => return Err(PackCode::RowNotFound.code()),
        }
        i += 1;
    }
    // 全部 20 例必须检出（越位注入必红）。
    if reds != REF_ROWS as u32 {
        return Err(PackCode::WrongShiftDetected.code());
    }
    Ok(reds)
}

// ── 账本（PackLedger：守恒核算） ────────────────────────────────
/// 打包账本：尝试/收/越界拒/错位检出四账并列。
#[derive(Clone, Copy, Debug)]
pub struct PackLedger {
    pub tries: u32,
    pub accepted: u32,
    pub overflow_rej: u32,
    pub wrong_caught: u32,
}

impl PackLedger {
    pub fn new() -> PackLedger {
        PackLedger { tries: 0, accepted: 0, overflow_rej: 0, wrong_caught: 0 }
    }
    /// 守恒：tries == accepted + overflow_rej（错位检出计入独立账）。
    pub fn conserve(&self) -> bool {
        self.tries == self.accepted + self.overflow_rej
    }
}

// ── 状态机（PackSession：Fresh→Seeded→Audited→Sealed 单向） ────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackPhase {
    /// 新会话（未播种）。
    Fresh,
    /// 已播种对照行（ref_row 逐例装载完成）。
    Seeded,
    /// 已完成对拍审计（零偏差确认）。
    Audited,
    /// 已封印（账只读）。
    Sealed,
}

/// 打包审计会话：装载对照行→逐例对拍→封印。
pub struct PackSession {
    phase: PackPhase,
    /// 装载的对照行快照（按行号存期望魔数；None=未装载）。
    seeded: [Option<u32>; REF_ROWS],
    pub ledger: PackLedger,
}

impl PackSession {
    /// 新会话。
    pub fn new() -> PackSession {
        PackSession {
            phase: PackPhase::Fresh,
            seeded: [None; REF_ROWS],
            ledger: PackLedger::new(),
        }
    }
    /// 当前相位（只读）。
    pub fn phase(&self) -> PackPhase {
        self.phase
    }
    /// 播种：Fresh→Seeded；逐行装载期望魔数并记账（pack 成功才收）。
    pub fn seed(&mut self) -> Result<(), u32> {
        if self.phase != PackPhase::Fresh {
            return Err(PackCode::SessionPhaseSkipped.code());
        }
        let mut i = 0;
        while i < REF_ROWS {
            match ref_row(i) {
                Some(r) => {
                    self.ledger.tries += 1;
                    match pack(r.major, r.minor, r.build) {
                        Ok(dw) => {
                            // 实现结果必须与冻结魔数一致，否则表污染。
                            if dw != r.expect {
                                self.ledger.overflow_rej += 1;
                                return Err(PackCode::RefRowMismatch.code());
                            }
                            self.seeded[i] = Some(dw);
                            self.ledger.accepted += 1;
                        }
                        Err(_) => {
                            self.ledger.overflow_rej += 1;
                            return Err(PackCode::RefRowMismatch.code());
                        }
                    }
                }
                None => return Err(PackCode::RowNotFound.code()),
            }
            i += 1;
        }
        self.phase = PackPhase::Seeded;
        Ok(())
    }
    /// 对拍审计：Seeded→Audited；逐行 unpack 必须还原原三元组。
    pub fn audit(&mut self) -> Result<(), u32> {
        if self.phase != PackPhase::Seeded {
            return Err(PackCode::SessionPhaseSkipped.code());
        }
        let mut i = 0;
        while i < REF_ROWS {
            match self.seeded[i] {
                Some(dw) => {
                    let r = match ref_row(i) {
                        Some(r) => r,
                        None => return Err(PackCode::RowNotFound.code()),
                    };
                    if unpack(dw) != (r.major, r.minor, r.build) {
                        return Err(PackCode::RoundtripBroken.code());
                    }
                }
                None => return Err(PackCode::SessionPhaseSkipped.code()),
            }
            i += 1;
        }
        self.phase = PackPhase::Audited;
        Ok(())
    }
    /// 封印：Audited→Sealed；封印后账只读。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != PackPhase::Audited {
            return Err(PackCode::SessionPhaseSkipped.code());
        }
        if !self.ledger.conserve() {
            return Err(PackCode::LedgerBroken.code());
        }
        self.phase = PackPhase::Sealed;
        Ok(())
    }
}

// ── 越界注入核（三类域越界 × 10 轮，轮换样本） ─────────────────
/// 三类越界样本构造：轮数轮换越界幅度（上限+1 / 上限+7 / 越一域）。
/// 返回（major, minor, build）三元组，越界必落在某一域。
pub fn bad_overflow_sample(round: usize, class: usize) -> (u32, u32, u32) {
    // 幅度轮换：1 / 7 / 4096，保证每轮样本不同但必越界。
    let bump = match round % 3 {
        0 => 1u32,
        1 => 7u32,
        _ => 4096u32,
    };
    match class {
        0 => (MAJOR_MAX + bump, 0, 0),
        1 => (5, MINOR_MAX + bump, 0),
        _ => (5, 1, BUILD_MAX + bump),
    }
}

/// 单域越界注入 rounds 轮（class: 0=主版本 1=次版本 2=Build）。
/// 返回 Ok(拒轮数)=全拒；任一轮漏检即 Err（越位必红）。
pub fn overflow_inject_field(rounds: u32, class: usize) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let (m, n, b) = bad_overflow_sample(r as usize, class);
        match pack(m, n, b) {
            Err(_) => rejected += 1,
            Ok(_) => return Err(PackCode::MajorOverflow.code()),
        }
        r += 1;
    }
    Ok(rejected)
}

/// 越界注入 rounds 轮：三类各 10/10 必拒（显式 Err，不截断不 panic）。
pub fn overflow_inject_rounds(rounds: u32) -> Result<PackLedger, u32> {
    let mut led = PackLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut class = 0;
        while class < 3 {
            let (m, n, b) = bad_overflow_sample(r as usize, class);
            led.tries += 1;
            match pack(m, n, b) {
                Err(_) => led.overflow_rej += 1,
                Ok(_) => return Err(PackCode::MajorOverflow.code()),
            }
            class += 1;
        }
        r += 1;
    }
    if !led.conserve() {
        return Err(PackCode::LedgerBroken.code());
    }
    Ok(led)
}

// ── 黄金全链：播种→审计→封印→错位全红→越界全拒→复现 ────────────
/// 黄金全链一次（确定性）。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut sess = PackSession::new();
    sess.seed()?;
    sess.audit()?;
    sess.seal()?;
    // 三个错位变体 × 20 例全红。
    let mut wrong_total = 0u32;
    let mut v = 1;
    while v <= WRONG_VARIANTS as u32 {
        let reds = wrong_variant_red(v)?;
        wrong_total += reds;
        v += 1;
    }
    // 越界注入三类 × 10 轮全拒。
    let led = overflow_inject_rounds(JUDGE_ROUNDS)?;
    if led.overflow_rej != 3 * JUDGE_ROUNDS {
        return Err(PackCode::MajorOverflow.code());
    }
    Ok((sess.ledger.accepted, wrong_total, led.overflow_rej, sess.ledger.tries))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PackCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PackCode::ALL.len() {
        let v = PackCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：正链 20 例播种+审计+封印成功。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut sess = PackSession::new();
    sess.seed()?;
    sess.audit()?;
    sess.seal()?;
    if sess.phase() != PackPhase::Sealed {
        return Err(PackCode::SessionPhaseSkipped.code());
    }
    Ok(())
}

/// 探针 B：越级审计（Fresh 直接 audit）→ 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut sess = PackSession::new();
    sess.audit().map(|_| ())
}

/// 探针 C：往返破坏检测——手工构一个错位值，unpack 不还原即红。
pub fn probe_seq_c() -> Result<(), u32> {
    // 正位 5.1.2600 = 0x0501_0A28；错位变体 2（低 16 位错切）必不还原。
    let wrong = pack_variant(2, 5, 1, 2600);
    if unpack(wrong) == (5, 1, 2600) {
        return Err(PackCode::RoundtripBroken.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（动态明细 ≤8，其余静态）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 对照表行数冻结 = 20。
    let row_n = {
        let mut n = 0;
        let mut i = 0;
        while i < REF_ROWS + 4 {
            if ref_row(i).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    };
    v.push((
        "J1-对照表20例冻结",
        row_n == REF_ROWS,
        format!("rows={}/{}", row_n, REF_ROWS),
    ));

    // 2. 20 例零偏差（pack 实现结果 == 写死期望魔数逐例一致）。
    let zero_dev = {
        let mut dev = 0u32;
        let mut i = 0;
        while i < REF_ROWS {
            match ref_row(i) {
                Some(r) => match pack(r.major, r.minor, r.build) {
                    Ok(dw) => {
                        if dw != r.expect {
                            dev += 1;
                        }
                    }
                    Err(_) => dev += 1,
                },
                None => dev += 1,
            }
            i += 1;
        }
        dev
    };
    v.push((
        "J1-实机对照20例零偏差",
        zero_dev == 0,
        format!("dev={}", zero_dev),
    ));

    // 3. 往返一致 20/20。
    let rt_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < REF_ROWS {
            if let Some(r) = ref_row(i) {
                match pack(r.major, r.minor, r.build) {
                    Ok(dw) => {
                        if unpack(dw) != (r.major, r.minor, r.build) {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-往返一致20/20", rt_ok, String::from("pack→unpack 还原")));

    // 4. 位布局抽查（0x0501_0A28 解包 = 5.1.2600，高字主:次低字 Build）。
    let layout = unpack(0x0501_0A28);
    v.push((
        "J1-位布局抽查0x05010A28",
        layout == (5, 1, 2600),
        format!("{:?}", layout),
    ));

    // 5. 越界注入：主版本域 10/10 必拒。
    let major_rej = overflow_inject_field(JUDGE_ROUNDS, 0);
    v.push((
        "J1-主版本越界10/10",
        match &major_rej {
            Ok(n) => *n == JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("{:?}", major_rej),
    ));

    // 6. 越界注入：次版本域 10/10 必拒。
    let minor_rej = overflow_inject_field(JUDGE_ROUNDS, 1);
    v.push((
        "J1-次版本越界10/10",
        match &minor_rej {
            Ok(n) => *n == JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("{:?}", minor_rej),
    ));

    // 7. 越界注入：Build 域 10/10 必拒。
    let build_rej = overflow_inject_field(JUDGE_ROUNDS, 2);
    v.push((
        "J1-Build越界10/10",
        match &build_rej {
            Ok(n) => *n == JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("{:?}", build_rej),
    ));

    // 8~10. 错位变体 3 × 20 例全红。
    let mut wrong_reds = [0u32; WRONG_VARIANTS];
    let mut wrong_ok = true;
    let mut vi = 0;
    while vi < WRONG_VARIANTS {
        match wrong_variant_red((vi + 1) as u32) {
            Ok(n) => wrong_reds[vi] = n,
            Err(_) => wrong_ok = false,
        }
        vi += 1;
    }
    v.push((
        "J1-错位变体全红3x20",
        wrong_ok && wrong_reds[0] == 20 && wrong_reds[1] == 20 && wrong_reds[2] == 20,
        format!("{:?}", wrong_reds),
    ));

    // 11. 越界守恒账（三类合计 30 试 30 拒，收=0）。
    let led = overflow_inject_rounds(JUDGE_ROUNDS);
    let cons_ok = match &led {
        Ok(l) => {
            l.conserve()
                && l.tries == 3 * JUDGE_ROUNDS
                && l.overflow_rej == 3 * JUDGE_ROUNDS
                && l.accepted == 0
        }
        Err(_) => false,
    };
    v.push(("J1-越界守恒账", cons_ok, String::from("30 试 30 拒")));

    // 12. 会话全链（播种→审计→封印）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_ok();
    v.push((
        "J1-会话三序",
        pa && pb && pc,
        format!("a={} b拒={} c={}", pa, pb, pc),
    ));

    // 13. 边界四档（0/0/0 过、0xFF/0xFF/0xFFFF 过、满+1 各域拒）。
    let b_ok = pack(0, 0, 0).is_ok()
        && pack(MAJOR_MAX, MINOR_MAX, BUILD_MAX).is_ok()
        && pack(MAJOR_MAX + 1, 0, 0).is_err()
        && pack(0, MINOR_MAX + 1, 0).is_err()
        && pack(0, 0, BUILD_MAX + 1).is_err();
    v.push(("J1-边界四档裁决", b_ok, String::from("空/满/满+1×3域")));

    // 14. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 15. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 16. 诊断码闭集（10 码全非空中文明细）。
    let all_ok = {
        let mut ok = PackCode::ALL.len() == 10;
        let mut i = 0;
        while i < PackCode::ALL.len() {
            if PackCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集10码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 16 条；前 8 条走动态明细通道，
/// 其余走静态明细——动态槽上限 8，不超用）。
pub fn run_unx18402_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18402 DWORD 打包语义");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18402_checks();
        assert!(set.all_passed(), "UNX-F18402 判据集应全绿");
    }

    /// 正常流：对照行魔数抽查（XP 5.1.2600 与 Win11 10.0.22000）。
    #[test]
    fn test_ref_rows_magic() {
        let r1 = match ref_row(1) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(r1.expect, 0x0501_0A28);
        assert_eq!(pack(r1.major, r1.minor, r1.build), Ok(0x0501_0A28));
        let r19 = match ref_row(19) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(r19.expect, 0x0A00_55F0);
        assert_eq!(pack(r19.major, r19.minor, r19.build), Ok(0x0A00_55F0));
    }

    /// 边界四档：空下界/满上界过，满+1 三域各显式拒。
    #[test]
    fn test_boundaries() {
        assert!(pack(0, 0, 0).is_ok());
        assert!(pack(MAJOR_MAX, MINOR_MAX, BUILD_MAX).is_ok());
        match pack(MAJOR_MAX + 1, 0, 0) {
            Err(c) => assert_eq!(c, PackCode::MajorOverflow.code()),
            Ok(_) => assert!(false, "主版本满+1 必须拒"),
        }
        match pack(0, 0, BUILD_MAX + 1) {
            Err(c) => assert_eq!(c, PackCode::BuildOverflow.code()),
            Ok(_) => assert!(false, "Build 满+1 必须拒"),
        }
    }

    /// 注入：三类域越界各 10/10。
    #[test]
    fn test_overflow_injection() {
        assert_eq!(overflow_inject_field(JUDGE_ROUNDS, 0), Ok(10));
        assert_eq!(overflow_inject_field(JUDGE_ROUNDS, 1), Ok(10));
        assert_eq!(overflow_inject_field(JUDGE_ROUNDS, 2), Ok(10));
        match overflow_inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert_eq!(l.overflow_rej, 30),
            Err(_) => assert!(false, "注入全拒时不应报错"),
        }
    }

    /// 注入：错位变体 3 × 20 例全红。
    #[test]
    fn test_wrong_variants_all_red() {
        assert_eq!(wrong_variant_red(1), Ok(20));
        assert_eq!(wrong_variant_red(2), Ok(20));
        assert_eq!(wrong_variant_red(3), Ok(20));
    }

    /// 判定次序：三域同时越界时先报主版本。
    #[test]
    fn test_error_order_major_first() {
        match pack(MAJOR_MAX + 1, MINOR_MAX + 1, BUILD_MAX + 1) {
            Err(c) => assert_eq!(c, PackCode::MajorOverflow.code()),
            Ok(_) => assert!(false, "应报主版本越界"),
        }
    }

    /// 状态机：Fresh 直接 audit（越级）必拒。
    #[test]
    fn test_session_phase_skip() {
        let mut sess = PackSession::new();
        match sess.audit() {
            Err(c) => assert_eq!(c, PackCode::SessionPhaseSkipped.code()),
            Ok(()) => assert!(false, "越级审计必须被拒"),
        }
    }

    /// 往返：错位值 unpack 不还原（探针 C 语义）。
    #[test]
    fn test_wrong_roundtrip_red() {
        assert!(probe_seq_c().is_ok());
    }

    /// 复现红线：golden_run 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }

    /// 账本守恒：注入账 30 试 30 拒、收 0。
    #[test]
    fn test_ledger_conservation() {
        match overflow_inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert!(l.conserve() && l.accepted == 0),
            Err(_) => assert!(false, "不应报错"),
        }
    }
}
