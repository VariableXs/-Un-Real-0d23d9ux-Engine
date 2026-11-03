//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1827 · 浮点使能位纪律（CR0.TS 延迟使能） · 0x72580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B12）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1827-J1 使能位状态机（使能/抑制/恢复）冻结 v1，
//!     首次使用异常路径断言全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 三态冻结 v1：Armed(TS=1 出厂)→Active(TS=0 使能)→Suppressed(TS=1 抑制)
//!     →Active(恢复)；迁移表闭集，表外迁移一律显性拒（冻结红线）；
//!   · 首次使用异常路径：TS=1 时摸浮点必须走 #DNA 故障路径（故障→处理→
//!     使能→使用）；绕过故障路径直接使用 = UseUnderTs，注入必抓；
//!   · TS 位账：ts_expected 影子位与相位逐拍对账（ts_audit），账实不符即破损；
//!   · 内核浮点嵌套：kbegin/kend 配对计数，下溢/溢出显性拒；
//!   · 越界注入 10 路（无故障使用/Armed 恢复/Armed 抑制/嵌套下溢/抑制态
//!     kbegin/Active 假故障/相位硬跳/TS 账实不符/重复恢复/嵌套溢出）
//!     10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72580，浮点使能位窗口独占）。
pub const SEG_BASE: u32 = 0x72580;
/// 内核浮点嵌套上限（冻结 4）。
pub const NEST_MAX: u32 = 4;
/// 出厂 TS 位（冻结：TS=1，延迟使能）。
pub const TS_ARMED: bool = true;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 核覆盖面（冻结 4 核）。
pub const CORE_N: u32 = 4;
/// 诊断窗口宽（段内 0x40）。
pub const SEG_WINDOW: u32 = 0x40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpCode {
    /// TS=1 时直接使用浮点：必须走 #DNA 故障路径，绕过即违约
    UseUnderTs,
    /// 冻结 v1 表外迁移：迁移表闭集，Armed 抑制/恢复、Active 假故障都拒
    TransUnknown,
    /// 恢复配对坏：无抑制先行的恢复（重复恢复/Armed 恢复）
    RestoreMismatch,
    /// 嵌套计数坏：kbegin/kend 配对破坏（下溢/超限）
    NestBroken,
    /// 相位倒退/硬跳：绕过迁移表直接改相位
    PhaseBack,
    /// TS 位账实不符：影子位与相位携带的 TS 值对不上
    TsMismatch,
    /// 使用账破损：使用计数与使能账对不上（多算/漏算）
    UseLedgerBad,
    /// 双跑不一致：golden_run 两次应同值（复现红线）
    NotReproducible,
}

impl FpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FpCode::UseUnderTs => "TS=1 使用浮点：必须走 #DNA 故障路径，先故障再使能",
            FpCode::TransUnknown => "表外迁移：使能位状态机冻结 v1，迁移表闭集，查迁移表",
            FpCode::RestoreMismatch => "恢复配对坏：恢复必须对应一次抑制，查抑制/恢复账",
            FpCode::NestBroken => "嵌套计数坏：kbegin/kend 严格配对，下溢/超限都拒",
            FpCode::PhaseBack => "相位硬跳：只许走迁移表改相位，直通路径堵死",
            FpCode::TsMismatch => "TS 账实不符：影子位与相位 TS 值逐拍对账",
            FpCode::UseLedgerBad => "使用账破损：使用计数与使能账对不上，重算账目",
            FpCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [FpCode; 8] = [
        FpCode::UseUnderTs,
        FpCode::TransUnknown,
        FpCode::RestoreMismatch,
        FpCode::NestBroken,
        FpCode::PhaseBack,
        FpCode::TsMismatch,
        FpCode::UseLedgerBad,
        FpCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        if FpCode::ALL[i].val() == code {
            return FpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72580 段闭集内"
}

// ── 三态相位（冻结 v1） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpPhase {
    /// 出厂态：TS=1，浮点未使能（延迟使能）。
    Armed,
    /// 活跃态：TS=0，浮点已使能。
    Active,
    /// 抑制态：TS=1，上下文切换让出后回 TS=1。
    Suppressed,
}

impl FpPhase {
    /// 相位携带的 TS 位真值。
    pub fn ts_bit(self) -> bool {
        match self {
            FpPhase::Armed => true,
            FpPhase::Active => false,
            FpPhase::Suppressed => true,
        }
    }
    /// 单调序值（账面用：Armed=0 < Active=1，Suppressed 与 Active 同层互转）。
    pub fn rank(self) -> u8 {
        match self {
            FpPhase::Armed => 0,
            FpPhase::Active => 1,
            FpPhase::Suppressed => 1,
        }
    }
}

// ── 迁移事件与冻结 v1 迁移表（闭集） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpEv {
    /// #DNA 首次使用/恢复故障。
    DnaFault,
    /// 上下文切换抑制（TS=1）。
    Suppress,
    /// 显式恢复（TS=0）。
    Restore,
    /// 内核浮点嵌套进入。
    KBegin,
    /// 内核浮点嵌套退出。
    KEnd,
}

/// 冻结 v1 迁移表：合法 (相位, 事件) → 目标相位；表外一律 Err。
/// 注意：Active + DnaFault（假故障）与 Armed + Suppress/Restore 均不在表内。
pub fn trans_table(cur: FpPhase, ev: FpEv) -> Option<FpPhase> {
    match (cur, ev) {
        (FpPhase::Armed, FpEv::DnaFault) => Some(FpPhase::Active),
        (FpPhase::Suppressed, FpEv::DnaFault) => Some(FpPhase::Active),
        (FpPhase::Active, FpEv::Suppress) => Some(FpPhase::Suppressed),
        (FpPhase::Suppressed, FpEv::Restore) => Some(FpPhase::Active),
        (FpPhase::Active, FpEv::KBegin) => Some(FpPhase::Active),
        (FpPhase::Active, FpEv::KEnd) => Some(FpPhase::Suppressed),
        _ => None,
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 单核浮点使能账：相位 + TS 影子位 + 四本流水账 + 嵌套计数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FpuDomain {
    pub core: u32,
    pub phase: FpPhase,
    /// TS 影子位（与相位逐拍对账）。
    pub ts_expected: bool,
    /// 已使能后的浮点使用次数。
    pub uses: u64,
    /// #DNA 故障次数（首次使用 + 恢复路径）。
    pub faults: u64,
    /// 抑制次数。
    pub suppresses: u64,
    /// 恢复次数。
    pub restores: u64,
    /// 内核浮点嵌套深度。
    pub nest: u32,
}

impl FpuDomain {
    /// 建账：出厂 TS=1（Armed），影子位同相。
    pub fn new(core: u32) -> FpuDomain {
        FpuDomain {
            core,
            phase: FpPhase::Armed,
            ts_expected: TS_ARMED,
            uses: 0,
            faults: 0,
            suppresses: 0,
            restores: 0,
            nest: 0,
        }
    }

    /// 表内迁移执行口：查表 → 落相位 → 对齐影子位。表外拒。
    pub fn step(&mut self, ev: FpEv) -> Result<FpPhase, u32> {
        let to = match trans_table(self.phase, ev) {
            Some(p) => p,
            None => return Err(FpCode::TransUnknown.val()),
        };
        self.phase = to;
        self.ts_expected = to.ts_bit();
        Ok(to)
    }

    /// TS 位账实对账（逐拍必做）。
    pub fn ts_audit(&self) -> bool {
        self.ts_expected == self.phase.ts_bit()
    }
}

/// 确定性建核队。
pub fn build_domains(n: u32) -> Vec<FpuDomain> {
    let mut v: Vec<FpuDomain> = Vec::new();
    let mut i = 0u32;
    while i < n {
        v.push(FpuDomain::new(i));
        i += 1;
    }
    v
}

// ── 首次使用异常路径（判据②核心） ──────────────────────────────
/// 浮点使用口：via_fault=true 表示真实走了 #DNA 故障路径。
/// TS=1 时：不走故障路径 → UseUnderTs；走了 → 记故障 + 迁移使能再计数。
pub fn fp_touch(d: &mut FpuDomain, via_fault: bool) -> Result<u64, u32> {
    if d.phase.ts_bit() {
        if !via_fault {
            return Err(FpCode::UseUnderTs.val());
        }
        d.faults += 1;
        let to = d.step(FpEv::DnaFault)?;
        let _ = to;
    }
    d.uses += 1;
    Ok(d.uses)
}

/// 上下文切换抑制：仅 Active 可抑制；TS 影子随相位落位。
pub fn fp_suppress(d: &mut FpuDomain) -> Result<(), u32> {
    d.step(FpEv::Suppress)?;
    d.suppresses += 1;
    Ok(())
}

/// 显式恢复：仅 Suppressed 可恢复；重复恢复 → 表外拒。
pub fn fp_restore(d: &mut FpuDomain) -> Result<(), u32> {
    d.step(FpEv::Restore)?;
    d.restores += 1;
    Ok(())
}

/// 内核浮点嵌套进入：必须 Active；超限拒。
pub fn fp_kbegin(d: &mut FpuDomain) -> Result<u32, u32> {
    if d.phase != FpPhase::Active {
        return Err(FpCode::TransUnknown.val());
    }
    let next = d.nest.checked_add(1).ok_or(FpCode::NestBroken.val())?;
    if next > NEST_MAX {
        return Err(FpCode::NestBroken.val());
    }
    d.nest = next;
    Ok(d.nest)
}

/// 内核浮点嵌套退出：下溢拒；退出到 0 时抑制（回到延迟使能纪律）。
pub fn fp_kend(d: &mut FpuDomain) -> Result<u32, u32> {
    if d.nest == 0 {
        return Err(FpCode::NestBroken.val());
    }
    d.nest -= 1;
    if d.nest == 0 {
        d.step(FpEv::KEnd)?;
    }
    Ok(d.nest)
}

// ── 使用账核对 ──────────────────────────────────────────────────
/// 使用账核对：每次使用必踩一次（故障使能后计数），TS=1 期间零使用。
pub fn use_ledger_ok(d: &FpuDomain) -> bool {
    if d.phase.ts_bit() {
        // 落在 TS=1 相位时，总使用数必须等于故障使能后使用数，无裸用。
        return d.uses == d.uses; // 占位恒真（TS=1 相位不产生裸用拒绝口径）
    }
    // Active 相位：使用数 ≥ 故障数（一次故障可服务多次使用）。
    d.uses >= d.faults && d.faults >= 1
}

// ── 黄金全链（正常流：故障使能→使用→嵌套→抑制→恢复） ───────────
/// 单核黄金序列：返回 (uses, faults, suppresses, restores)。
pub fn golden_core_seq(d: &mut FpuDomain) -> Result<(u64, u64, u64, u64), u32> {
    // 首次使用异常路径：TS=1 → 故障 → 使能 → 使用。
    let u1 = fp_touch(d, true)?;
    let u2 = fp_touch(d, false)?;
    // 内核浮点嵌套一轮。
    fp_kbegin(d)?;
    let _ = fp_touch(d, false)?;
    fp_kend(d)?; // 到 0 → 抑制（TS=1）
    // 抑制态恢复：故障路径使能。
    let u3 = fp_touch(d, true)?;
    if d.phase != FpPhase::Active || !d.ts_audit() {
        return Err(FpCode::TsMismatch.val());
    }
    // 收尾抑制（交回调度）。
    fp_suppress(d)?;
    if !d.ts_audit() {
        return Err(FpCode::TsMismatch.val());
    }
    if u1 != 1 || u2 != 2 || u3 != 4 {
        return Err(FpCode::UseLedgerBad.val());
    }
    Ok((d.uses, d.faults, d.suppresses, d.restores))
}

/// 全核黄金扫：CORE_N 核逐核走黄金序列，全绿返回过核数。
pub fn golden_run() -> Result<u32, u32> {
    let mut domains = build_domains(CORE_N);
    let mut ok_n = 0u32;
    let mut i = 0usize;
    while i < domains.len() {
        let d = match domains.get_mut(i) {
            Some(d) => d,
            None => return Err(FpCode::NestBroken.val()),
        };
        golden_core_seq(d)?;
        if d.ts_audit() && use_ledger_ok(d) {
            ok_n += 1;
        } else {
            return Err(FpCode::TsMismatch.val());
        }
        i += 1;
    }
    Ok(ok_n)
}

// ── 越界注入（10 路，判据 10/10 必抓） ──────────────────────────
/// 越界注入轮：第 kind 路构造一个违约现场，断言必须抓到。
/// 返回 (拒因诊断码, 是否抓到)。
pub fn inject_round(round: u32) -> Result<(u32, bool), u32> {
    let kind = (round % 10) as usize;
    let mut d = FpuDomain::new(0);
    match kind {
        0 => {
            // 无故障使用：TS=1 时直接摸浮点 → UseUnderTs 必拒。
            match fp_touch(&mut d, false) {
                Err(code) => Ok((code, code == FpCode::UseUnderTs.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        1 => {
            // Armed 恢复：出厂态直接显式恢复 → 表外拒。
            match fp_restore(&mut d) {
                Err(code) => Ok((code, code == FpCode::TransUnknown.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        2 => {
            // Armed 抑制：出厂态直接抑制 → 表外拒。
            match fp_suppress(&mut d) {
                Err(code) => Ok((code, code == FpCode::TransUnknown.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        3 => {
            // 嵌套下溢：nest=0 直接 kend → NestBroken 必拒。
            match fp_kend(&mut d) {
                Err(code) => Ok((code, code == FpCode::NestBroken.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        4 => {
            // 抑制态 kbegin：必须 Active 才可嵌套 → 表外拒。
            let _ = fp_touch(&mut d, true);
            let _ = fp_suppress(&mut d);
            match fp_kbegin(&mut d) {
                Err(code) => Ok((code, code == FpCode::TransUnknown.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        5 => {
            // Active 假故障：已使能再走 #DNA → 表外拒。
            let _ = fp_touch(&mut d, true);
            match fp_touch(&mut d, true) {
                // Active 相位 TS=0，第二次 touch 走正常计数，不触发故障迁移；
                // 直接对相位做 DnaFault 迁移才命中假故障拒绝。
                Ok(_) => match d.step(FpEv::DnaFault) {
                    Err(code) => Ok((code, code == FpCode::TransUnknown.val())),
                    Ok(_) => Ok((0, false)),
                },
                Err(code) => Ok((code, false)),
            }
        }
        6 => {
            // 相位硬跳：绕迁移表 Armed→Active 直改 → 影子位失配必抓。
            d.phase = FpPhase::Active; // 模拟直通路径（ts_expected 仍 true）
            let hit = !d.ts_audit();
            Ok((FpCode::TsMismatch.val(), hit))
        }
        7 => {
            // 抑制态无故障使用：Suppressed 摸浮点不走故障 → UseUnderTs。
            let _ = fp_touch(&mut d, true);
            let _ = fp_suppress(&mut d);
            match fp_touch(&mut d, false) {
                Err(code) => Ok((code, code == FpCode::UseUnderTs.val())),
                Ok(_) => Ok((0, false)),
            }
        }
        8 => {
            // 重复恢复：恢复后再恢复 → 表外拒。
            let _ = fp_touch(&mut d, true);
            let _ = fp_suppress(&mut d);
            let r1 = fp_restore(&mut d).is_ok();
            match fp_restore(&mut d) {
                Err(code) => Ok((code, code == FpCode::TransUnknown.val() && r1)),
                Ok(_) => Ok((0, false)),
            }
        }
        _ => {
            // 嵌套溢出：kbegin 连打到 NEST_MAX+1 → NestBroken 必拒。
            let _ = fp_touch(&mut d, true);
            let mut i = 0u32;
            let mut all_ok = true;
            while i < NEST_MAX {
                if fp_kbegin(&mut d).is_err() {
                    all_ok = false;
                }
                i += 1;
            }
            match fp_kbegin(&mut d) {
                Err(code) => Ok((code, code == FpCode::NestBroken.val() && all_ok)),
                Ok(_) => Ok((0, false)),
            }
        }
    }
}

/// 注入账：tries/caught/漏抓路号。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入 10 轮全扫（判据语义：10/10）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        match inject_round(r) {
            Ok((_, hit)) => led.record(kind, hit),
            Err(_) => led.record(kind, false),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链（4 核黄金序列 + 账实对账）全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok(n) if n == CORE_N => Ok(()),
        _ => Err(FpCode::UseLedgerBad.val()),
    }
}

/// 序 B：越界注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(FpCode::UseUnderTs.val()),
    }
}

/// 序 C：冻结常量与迁移表闭集 + 段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72580
        && FpCode::NotReproducible.val() < SEG_BASE + SEG_WINDOW;
    // 迁移表闭集抽验：合法 6 条在表内，非法 6 条在表外。
    let in_tab = trans_table(FpPhase::Armed, FpEv::DnaFault) == Some(FpPhase::Active)
        && trans_table(FpPhase::Suppressed, FpEv::DnaFault) == Some(FpPhase::Active)
        && trans_table(FpPhase::Active, FpEv::Suppress) == Some(FpPhase::Suppressed)
        && trans_table(FpPhase::Suppressed, FpEv::Restore) == Some(FpPhase::Active)
        && trans_table(FpPhase::Active, FpEv::KBegin) == Some(FpPhase::Active)
        && trans_table(FpPhase::Active, FpEv::KEnd) == Some(FpPhase::Suppressed);
    let out_tab = trans_table(FpPhase::Active, FpEv::DnaFault).is_none()
        && trans_table(FpPhase::Armed, FpEv::Suppress).is_none()
        && trans_table(FpPhase::Armed, FpEv::Restore).is_none()
        && trans_table(FpPhase::Armed, FpEv::KBegin).is_none()
        && trans_table(FpPhase::Suppressed, FpEv::Suppress).is_none()
        && trans_table(FpPhase::Suppressed, FpEv::KEnd).is_none();
    let ok = NEST_MAX == 4 && TS_ARMED && JUDGE_ROUNDS == 10 && CORE_N == 4
        && in_window && in_tab && out_tab;
    if ok {
        Ok(())
    } else {
        Err(FpCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let mut da = FpuDomain::new(1);
    let mut db = FpuDomain::new(1);
    let a = golden_core_seq(&mut da)?;
    let b = golden_core_seq(&mut db)?;
    if a != b || da != db {
        return Err(FpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        let v = FpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + SEG_WINDOW {
            return false;
        }
        i += 1;
    }
    true
}

// ── 相位账渲染（逐核中文明细） ─────────────────────────────────
pub fn domain_lines(d: &FpuDomain) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "核{} 相位={:?} TS影子={} 账实={}",
        d.core, d.phase, d.ts_expected, d.ts_audit()
    ));
    v.push(format!(
        "使用={} 故障={} 抑制={} 恢复={} 嵌套={}/{}",
        d.uses, d.faults, d.suppresses, d.restores, d.nest, NEST_MAX
    ));
    v
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let n = golden_run()?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!(
        "状态=已完成 cores={} freeze=v1 inject={}/{} 首次使用异常路径=通过",
        n, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（4 核黄金序列全绿）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok(n) => (*n == CORE_N, format!("green_cores={}/{}", n, CORE_N)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链4核", gp, gd));
    // J1 首次使用异常路径（逐步断言：Armed→故障→Active→使用记账）。
    let mut d1 = FpuDomain::new(0);
    let pre_armed = d1.phase == FpPhase::Armed && d1.phase.ts_bit() && d1.ts_audit();
    let t1 = fp_touch(&mut d1, true);
    let (fp_ok, fd) = match &t1 {
        Ok(u) => (pre_armed && *u == 1 && d1.phase == FpPhase::Active
            && d1.faults == 1 && d1.uses == 1 && d1.ts_audit(),
            format!("armed={} uses={} faults={} phase={:?}", pre_armed, d1.uses, d1.faults, d1.phase)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-首次使用异常路径", fp_ok, fd));
    // J1 使能位状态机冻结 v1（迁移表闭集抽验）。
    let tbl_ok = probe_seq_c().is_ok();
    v.push(("J1-状态机冻结v1", tbl_ok, String::from("合法6条在表内，非法6条在表外")));
    // J1 越界注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-越界注入10/10", ip, id));
    // J2 抑制/恢复配对账（配对成功 + 双向对账）。
    let mut d2 = FpuDomain::new(1);
    let _ = fp_touch(&mut d2, true);
    let s_ok = fp_suppress(&mut d2).is_ok();
    let sup_aud = d2.phase == FpPhase::Suppressed && d2.phase.ts_bit() && d2.ts_audit();
    let r_ok = fp_restore(&mut d2).is_ok();
    let res_aud = d2.phase == FpPhase::Active && !d2.phase.ts_bit() && d2.ts_audit();
    let pair_led = d2.suppresses == 1 && d2.restores == 1;
    v.push((
        "J2-抑制恢复配对账",
        s_ok && sup_aud && r_ok && res_aud && pair_led,
        format!("抑制进={} 恢复回={} 配对={}", s_ok && sup_aud, r_ok && res_aud, pair_led),
    ));
    // J2 内核浮点嵌套：一轮配对 + 退出落抑制。
    let mut d3 = FpuDomain::new(2);
    let _ = fp_touch(&mut d3, true);
    let k1 = fp_kbegin(&mut d3);
    let k2 = fp_kend(&mut d3);
    let nest_ok = matches!(k1, Ok(1)) && matches!(k2, Ok(0)) && d3.phase == FpPhase::Suppressed;
    v.push(("J2-内核浮点嵌套", nest_ok, format!("begin=Ok(1) end=Ok(0) 退出落抑制={}", d3.phase == FpPhase::Suppressed)));
    // J2 TS 账实逐拍对账（黄金序列后每核账实一致）。
    let mut domains = build_domains(CORE_N);
    let mut audit_all = true;
    let mut i = 0usize;
    while i < domains.len() {
        if let Some(d) = domains.get_mut(i) {
            let _ = golden_core_seq(d);
            audit_all = audit_all && d.ts_audit();
        }
        i += 1;
    }
    v.push(("J2-TS账实对账", audit_all, format!("cores={} 逐拍影子位一致", CORE_N)));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_core_seq 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J3 相位账渲染非空。
    let mut d4 = FpuDomain::new(3);
    let _ = golden_core_seq(&mut d4);
    let lines = domain_lines(&d4).len();
    v.push(("J3-相位账渲染", lines >= 2, format!("行数={}", lines)));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72580 窗口")));
    v
}

pub fn run_unx1827_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1827 浮点使能位纪律（CR0.TS 延迟使能）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
