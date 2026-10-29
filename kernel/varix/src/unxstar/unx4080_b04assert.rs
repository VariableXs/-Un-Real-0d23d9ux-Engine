//! unx4080_b04assert — UNX-F4080 B04 批判据聚合断言集（诊断段 0x8C40）。
//!
//! 判据原文：UNX-F4080-J1 本批 19 条判据全部聚合入 ktest vfs B04 面，聚合报告
//!           19/19 无缺项，通过率 100% 才算绿。
//! 可验性质（本模块自证面）：
//!   J1 内嵌判据表恒 19 条（BATCH_N），判据号 1..19 连续，判据 ID 全表唯一，
//!      卡号闭于本批六卡（4075/4076/4077/4078/4079/4080）；
//!   J2 聚合报告五账齐全：total/touched/passed/failed/skipped/missing，
//!      且守恒 touched + missing == total、passed + failed + skipped == touched；
//!   J3 通过率整千分复算一致：rate_permille == passed * 1000 / BATCH_N（整数除法），
//!      与"独立复算式"逐位相等才算账实相符；
//!   J4 绿门：只有 touched == 19 且 passed == 19（即无缺项、零失败、零跳过）才置绿；
//!      任一条 Fail / Skip / 未报，绿门必须关；
//!   J5 登记纪律：同号二次上报拒（E_DUP_REC），越界判据号拒（E_NO_OOR），
//!      拒收路径不得改任何计数（重号不覆盖、越界不入表）。
//! 设计纪律：
//!   1. 判据表是 const 内嵌表：批账不对任何外部模块建依赖（本模块不 import 同批
//!      任何模块文件），19 条判据的 ID/卡号/类型在编译期就固定，运行时只做登记与对账。
//!   2. 判据号 1..19（1 基）与数组下标 0..18（0 基）的换算只在 no_to_idx/idx_to_no
//!      两个收敛点做，别处不许出现 ±1 魔数。
//!   3. 绿门是合取，不是加权：19 条里任意一条不绿整批不绿；通过率只作观察账，
//!      不作放行依据（防"99% 也算过"的稀释）。
//!   4. 上报只进不退：已 Pass 的判据可被改判 Fail（真跑出问题要能反映），但
//!      改判必须记 rejudge 账；不允许把 Fail 悄悄改回 Pass 而不留痕。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8C40 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8C40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_OOR: u16 = SEG_BASE + 1; // 判据号越界（合法 1..19）
const E_DUP_REC: u16 = SEG_BASE + 2; // 同号重复上报（未先改判）
const E_EMPTY: u16 = SEG_BASE + 3; // 无任何上报即出报告
const E_BAL: u16 = SEG_BASE + 4; // 聚合账不平（守恒失守）
const E_TABLE: u16 = SEG_BASE + 5; // 内嵌表自审失败（重号/卡号越界/条数不符）
const E_RATE: u16 = SEG_BASE + 6; // 通过率复算不一致
const E_NOT_GREEN: u16 = SEG_BASE + 7; // 绿门要求未满足
const E_CARD_OOR: u16 = SEG_BASE + 8; // 卡号不在本批闭集

/// 本批判据条数：判据原文 J1 的 19 条硬面。
pub const BATCH_N: usize = 19;

/// 本批六卡的卡号闭集（B04：inode 缓存批）。
pub const CARD_FIRST: u16 = 4075;
pub const CARD_LAST: u16 = 4080;

/// 判据类型闭集：断言 / 注入回归 / 守恒对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CritKind {
    Assert = 0,   // 行为断言
    Regress = 1,  // 注入回归
    Conserve = 2, // 守恒对账
}

/// 单条判据的上报裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Untouched = 0, // 未上报
    Pass = 1,      // 通过
    Fail = 2,      // 失败
    Skip = 3,      // 跳过（计缺项，不算绿）
}

/// 聚合相位机：Collecting（收账中）→ Complete（19/19 全收）或 Deficient（有缺项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    Collecting = 0, // 尚未收满
    Complete = 1,   // 19 条全收（是否绿另判）
    Deficient = 2,  // 明确有缺项（缺项被点出后进入）
}

/// 单条判据声明（编译期常量，运行时只读）。
pub struct Criterion {
    pub no: u8,             // 判据号（1..19）
    pub card: u16,          // 所属卡号（4075..4080）
    pub id: &'static str,   // 判据 ID（全表唯一）
    pub kind: CritKind,     // 判据类型
    pub mandatory: bool,    // 是否强制（本批 19 条全强制）
}

/// 内嵌判据表：19 条，覆盖六卡（4075×3 / 4076×3 / 4077×3 / 4078×3 / 4079×3 / 4080×4）。
pub const TABLE: [Criterion; BATCH_N] = [
    Criterion { no: 1, card: 4075, id: "UNX-F4075-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 2, card: 4075, id: "UNX-F4075-J2", kind: CritKind::Regress, mandatory: true },
    Criterion { no: 3, card: 4075, id: "UNX-F4075-J3", kind: CritKind::Conserve, mandatory: true },
    Criterion { no: 4, card: 4076, id: "UNX-F4076-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 5, card: 4076, id: "UNX-F4076-J2", kind: CritKind::Conserve, mandatory: true },
    Criterion { no: 6, card: 4076, id: "UNX-F4076-J3", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 7, card: 4077, id: "UNX-F4077-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 8, card: 4077, id: "UNX-F4077-J2", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 9, card: 4077, id: "UNX-F4077-J3", kind: CritKind::Regress, mandatory: true },
    Criterion { no: 10, card: 4078, id: "UNX-F4078-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 11, card: 4078, id: "UNX-F4078-J2", kind: CritKind::Regress, mandatory: true },
    Criterion { no: 12, card: 4078, id: "UNX-F4078-J3", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 13, card: 4079, id: "UNX-F4079-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 14, card: 4079, id: "UNX-F4079-J2", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 15, card: 4079, id: "UNX-F4079-J3", kind: CritKind::Regress, mandatory: true },
    Criterion { no: 16, card: 4080, id: "UNX-F4080-J1", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 17, card: 4080, id: "UNX-F4080-J2", kind: CritKind::Conserve, mandatory: true },
    Criterion { no: 18, card: 4080, id: "UNX-F4080-J3", kind: CritKind::Assert, mandatory: true },
    Criterion { no: 19, card: 4080, id: "UNX-F4080-J4", kind: CritKind::Assert, mandatory: true },
];

/// 判据号（1 基）→ 下标（0 基）的唯一换算点。
pub fn no_to_idx(no: u16) -> Result<usize, u16> {
    if no == 0 || no as usize > BATCH_N {
        return Err(E_NO_OOR);
    }
    Ok(no as usize - 1)
}

/// 下标（0 基）→ 判据号（1 基）的唯一换算点。
pub fn idx_to_no(idx: usize) -> Result<u16, u16> {
    if idx >= BATCH_N {
        return Err(E_NO_OOR);
    }
    Ok(idx as u16 + 1)
}

/// 聚合报告：五账 + 通过率 + 绿门。
pub struct AggReport {
    pub total: u32,          // 表内判据总数（恒 19）
    pub touched: u32,        // 已上报条数（Pass/Fail/Skip 都算）
    pub passed: u32,         // 通过条数
    pub failed: u32,         // 失败条数
    pub skipped: u32,        // 跳过条数
    pub missing: u32,        // 缺项条数（Untouched）
    pub rate_permille: u32,  // 通过率整千分（passed*1000/total）
    pub green: bool,         // 绿门（19/19 全 Pass）
}

impl AggReport {
    fn zero() -> AggReport {
        AggReport {
            total: BATCH_N as u32,
            touched: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            missing: BATCH_N as u32,
            rate_permille: 0,
            green: false,
        }
    }

    /// J2 守恒：touched + missing == total，且三类裁决之和 == touched。
    pub fn conserved(&self) -> bool {
        self.touched + self.missing == self.total
            && self.passed + self.failed + self.skipped == self.touched
    }

    /// J3 独立复算：用 total 重算千分率，与报告值逐位比。
    pub fn rate_consistent(&self) -> bool {
        if self.total == 0 {
            return self.rate_permille == 0;
        }
        let recomputed = self.passed.saturating_mul(1000) / self.total;
        recomputed == self.rate_permille
    }

    /// 缺项清单：把 Untouched 的判据号列出来（判据 J1"无缺项"的反面证据）。
    pub fn missing_rows(&self, led: &AggLedger) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < BATCH_N {
            if led.v[i] == Verdict::Untouched as u8 {
                if let Ok(no) = idx_to_no(i) {
                    out.push(no);
                }
            }
            i += 1;
        }
        out
    }
}

/// 批账本体：19 槽裁决 + 上报账 + 负样本账 + 相位。
pub struct AggLedger {
    pub v: [u8; BATCH_N],  // 19 槽裁决码（Verdict 的 u8 投影）
    pub recorded: u32,     // 成功上报次数（含改判）
    pub dup_rejects: u32,  // 重号拒收次数
    pub oor_rejects: u32,  // 越界拒收次数
    pub rejudges: u32,     // 改判次数
    pub state: AggState,   // 相位
}

impl AggLedger {
    pub fn new() -> AggLedger {
        AggLedger {
            v: [Verdict::Untouched as u8; BATCH_N],
            recorded: 0,
            dup_rejects: 0,
            oor_rejects: 0,
            rejudges: 0,
            state: AggState::Collecting,
        }
    }

    /// 上报一条裁决（J5）：同号二次上报默认拒；要改判须显式 rejudge 通道。
    pub fn record(&mut self, no: u16, verdict: Verdict) -> Result<(), u16> {
        let idx = match no_to_idx(no) {
            Ok(i) => i,
            Err(e) => {
                self.oor_rejects += 1;
                return Err(e);
            }
        };
        if verdict == Verdict::Untouched {
            return Err(E_EMPTY); // 上报"未触碰"没有信息量，拒
        }
        if self.v[idx] != Verdict::Untouched as u8 {
            self.dup_rejects += 1;
            return Err(E_DUP_REC);
        }
        self.v[idx] = verdict as u8;
        self.recorded += 1;
        self.refresh_state();
        Ok(())
    }

    /// 改判通道：已上报的判据允许改判（真跑出问题要能反映），但留 rejudge 痕。
    pub fn rejudge(&mut self, no: u16, verdict: Verdict) -> Result<(), u16> {
        let idx = no_to_idx(no)?;
        if verdict == Verdict::Untouched {
            return Err(E_EMPTY);
        }
        if self.v[idx] == Verdict::Untouched as u8 {
            return self.record(no, verdict); // 从未上报过，走正常上报路径
        }
        if self.v[idx] == verdict as u8 {
            return Ok(()); // 同裁决重放：幂等，不计改判
        }
        self.v[idx] = verdict as u8;
        self.rejudges += 1;
        self.refresh_state();
        Ok(())
    }

    /// 相位推进：全收 → Complete；有缺项且已被点出 → Deficient。
    fn refresh_state(&mut self) {
        let mut touched = 0u32;
        let mut i = 0usize;
        while i < BATCH_N {
            if self.v[i] != Verdict::Untouched as u8 {
                touched += 1;
            }
            i += 1;
        }
        self.state = if touched == BATCH_N as u32 {
            AggState::Complete
        } else if self.dup_rejects + self.oor_rejects > 0 || touched > 0 {
            AggState::Deficient
        } else {
            AggState::Collecting
        };
    }

    /// 出报告（J2/J3/J4 的汇合点）：一次遍历，五账同源。
    pub fn report(&self) -> Result<AggReport, u16> {
        let mut r = AggReport::zero();
        let mut i = 0usize;
        while i < BATCH_N {
            match self.v[i] {
                x if x == Verdict::Pass as u8 => {
                    r.passed += 1;
                    r.touched += 1;
                }
                x if x == Verdict::Fail as u8 => {
                    r.failed += 1;
                    r.touched += 1;
                }
                x if x == Verdict::Skip as u8 => {
                    r.skipped += 1;
                    r.touched += 1;
                }
                _ => {}
            }
            i += 1;
        }
        r.missing = r.total - r.touched;
        r.rate_permille = if r.total == 0 {
            0
        } else {
            r.passed.saturating_mul(1000) / r.total
        };
        // 绿门：19/19 全 Pass（无缺项、零失败、零跳过）
        r.green = r.touched == r.total && r.passed == r.total;
        if !r.conserved() {
            return Err(E_BAL);
        }
        if !r.rate_consistent() {
            return Err(E_RATE);
        }
        if r.touched == 0 {
            return Err(E_EMPTY);
        }
        Ok(r)
    }

    /// 内嵌表自审（J1）：条数、判据号连续、ID 唯一、卡号闭集、全强制。
    pub fn audit_table() -> Result<u32, u16> {
        if TABLE.len() != BATCH_N {
            return Err(E_TABLE);
        }
        let mut i = 0usize;
        while i < BATCH_N {
            let c = &TABLE[i];
            if c.no as usize != i + 1 {
                return Err(E_TABLE); // 判据号必须与表序一致（1..19）
            }
            if c.card < CARD_FIRST || c.card > CARD_LAST {
                return Err(E_CARD_OOR);
            }
            if c.id.is_empty() || !c.mandatory {
                return Err(E_TABLE);
            }
            // ID 唯一性：与后续逐条比
            let mut k = i + 1;
            while k < BATCH_N {
                if TABLE[k].id == c.id || TABLE[k].no == c.no {
                    return Err(E_TABLE);
                }
                k += 1;
            }
            i += 1;
        }
        Ok(TABLE.len() as u32)
    }

    /// 按卡聚合：返回该卡在本批的判据条数（卡号越界 → E_CARD_OOR）。
    pub fn count_by_card(card: u16) -> Result<u32, u16> {
        if card < CARD_FIRST || card > CARD_LAST {
            return Err(E_CARD_OOR);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BATCH_N {
            if TABLE[i].card == card {
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 槽位读出口（闭集下标，恒合法）。
    pub fn verdict_of(&self, no: u16) -> Result<Verdict, u16> {
        let idx = no_to_idx(no)?;
        match self.v[idx] {
            x if x == Verdict::Pass as u8 => Ok(Verdict::Pass),
            x if x == Verdict::Fail as u8 => Ok(Verdict::Fail),
            x if x == Verdict::Skip as u8 => Ok(Verdict::Skip),
            _ => Ok(Verdict::Untouched),
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_OOR => "判据号越界（合法 1..19）",
        E_DUP_REC => "同号重复上报（改判须走 rejudge）",
        E_EMPTY => "无任何上报即出报告",
        E_BAL => "聚合账不平（touched/missing 守恒失守）",
        E_TABLE => "内嵌判据表自审失败（重号/条数不符/非强制）",
        E_RATE => "通过率复算与报告值不一致",
        E_NOT_GREEN => "绿门要求未满足（须 19/19 全 Pass）",
        E_CARD_OOR => "卡号不在本批闭集 4075..4080",
        _ => "未知诊断码",
    }
}

/// 确定性演练：表自审 → 19/19 全绿 → 失败/缺项关绿门 → 注入拦截 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：内嵌表自审——19 条、号连续、ID 唯一、卡号闭集（J1）。
    let n = AggLedger::audit_table()?;
    if n != BATCH_N as u32 {
        return Err(E_TABLE);
    }
    if AggLedger::count_by_card(4075)? != 3
        || AggLedger::count_by_card(4076)? != 3
        || AggLedger::count_by_card(4077)? != 3
        || AggLedger::count_by_card(4078)? != 3
        || AggLedger::count_by_card(4079)? != 3
        || AggLedger::count_by_card(4080)? != 4
    {
        return Err(E_TABLE); // 3*5 + 4 == 19
    }
    hits += 1;

    // R2：19 条逐条 Pass → 报告 19/19、通过率 1000、绿门开（J4 正面）。
    let mut led = AggLedger::new();
    let mut no = 1u16;
    while no <= BATCH_N as u16 {
        led.record(no, Verdict::Pass)?;
        no += 1;
    }
    if led.state != AggState::Complete {
        return Err(E_BAL);
    }
    let r = led.report()?;
    if r.total != 19 || r.touched != 19 || r.passed != 19 || r.missing != 0 {
        return Err(E_BAL);
    }
    if r.rate_permille != 1000 {
        return Err(E_RATE);
    }
    if !r.green || !r.conserved() || !r.rate_consistent() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // R3：一条改判 Fail → 绿门必须关，通过率复算 18*1000/19 = 947（J3/J4 负面）。
    led.rejudge(7, Verdict::Fail)?;
    let r2 = led.report()?;
    if r2.green {
        return Err(E_NOT_GREEN);
    }
    if r2.failed != 1 || r2.passed != 18 {
        return Err(E_BAL);
    }
    if r2.rate_permille != 947 {
        return Err(E_RATE);
    }
    if !r2.rate_consistent() {
        return Err(E_RATE);
    }
    if led.rejudges != 1 {
        return Err(E_BAL); // 改判必须留痕
    }
    hits += 1;

    // R4：改回 Pass → 绿门重开（改判可逆，但痕留在 rejudges 账上）。
    led.rejudge(7, Verdict::Pass)?;
    let r3 = led.report()?;
    if !r3.green || r3.rate_permille != 1000 {
        return Err(E_NOT_GREEN);
    }
    if led.rejudges != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：缺项态——新账只报 18 条，missing==1、状态 Deficient、绿门关（J2 缺项面）。
    let mut led2 = AggLedger::new();
    let mut k = 1u16;
    while k <= 18 {
        led2.record(k, Verdict::Pass)?;
        k += 1;
    }
    if led2.state != AggState::Deficient {
        return Err(E_BAL);
    }
    let r4 = led2.report()?;
    if r4.missing != 1 || r4.touched != 18 || r4.green {
        return Err(E_BAL);
    }
    if r4.rate_permille != 947 {
        return Err(E_RATE); // 18/19 的千分率
    }
    let miss = r4.missing_rows(&led2);
    if miss.len() != 1 || miss[0] != 19 {
        return Err(E_BAL); // 缺的正是第 19 条
    }
    hits += 1;

    // R6：补上第 19 条即转 Complete 且全绿（缺项可补齐，补后即 Complete 而非 Deficient）。
    led2.record(19, Verdict::Pass)?;
    if led2.state != AggState::Complete {
        return Err(E_BAL);
    }
    let r5 = led2.report()?;
    if !r5.green || r5.touched != 19 || r5.missing != 0 {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // I1：重号上报 → E_DUP_REC，且 touched 不变、拒收账 +1（J5 主面）。
    let t_before = led2.report()?.touched;
    let dup_before = led2.dup_rejects;
    if led2.record(1, Verdict::Pass) != Err(E_DUP_REC) {
        return Err(E_DUP_REC);
    }
    if led2.dup_rejects != dup_before + 1 {
        return Err(E_BAL);
    }
    if led2.report()?.touched != t_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：判据号越界 → E_NO_OOR（0 / 20 / 65535 三点），越界账逐次 +1。
    let oor_before = led2.oor_rejects;
    if led2.record(0, Verdict::Pass) != Err(E_NO_OOR) {
        return Err(E_NO_OOR);
    }
    if led2.record(20, Verdict::Pass) != Err(E_NO_OOR) {
        return Err(E_NO_OOR);
    }
    if led2.record(0xFFFF, Verdict::Pass) != Err(E_NO_OOR) {
        return Err(E_NO_OOR);
    }
    if led2.oor_rejects != oor_before + 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // I3：上报 Untouched 无信息量 → E_EMPTY；空账出报告 → E_EMPTY。
    if led2.record(2, Verdict::Untouched) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    let empty = AggLedger::new();
    match empty.report() {
        Err(E_EMPTY) => {}
        _ => return Err(E_EMPTY),
    }
    hits += 1;

    // I4：卡号越界 → E_CARD_OOR（4074 / 4081 / 0）。
    if AggLedger::count_by_card(4074) != Err(E_CARD_OOR) {
        return Err(E_CARD_OOR);
    }
    if AggLedger::count_by_card(4081) != Err(E_CARD_OOR) {
        return Err(E_CARD_OOR);
    }
    if AggLedger::count_by_card(0) != Err(E_CARD_OOR) {
        return Err(E_CARD_OOR);
    }
    hits += 1;

    // I5：号↔下标换算两向自洽，且边号 1/19 正确（防 ±1 魔数错位）。
    if no_to_idx(1)? != 0 || no_to_idx(19)? != 18 {
        return Err(E_NO_OOR);
    }
    if idx_to_no(0)? != 1 || idx_to_no(18)? != 19 {
        return Err(E_NO_OOR);
    }
    if no_to_idx(0).is_err() == false || no_to_idx(20).is_err() == false {
        return Err(E_NO_OOR);
    }
    hits += 1;

    // 终审：两本账守恒 + 绿门终态 + 表审计复跑一致（批账对账）。
    let rf = led2.report()?;
    if !rf.conserved() || !rf.rate_consistent() || !rf.green {
        return Err(E_BAL);
    }
    if AggLedger::audit_table()? != BATCH_N as u32 {
        return Err(E_TABLE);
    }
    if led2.verdict_of(19)? != Verdict::Pass {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
