//! UNX-F10085 双判据行数三拆协议（正/反/断言） · 0x92500 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10085（把双判据条目的行数预算冻结成三拆守恒协议）。
//! 锚点：UNX-F10086-J1 双口径复测核（J1 与 J2 各 10/10 留痕），
//!       单侧复测 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92500（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SplitCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_split_line`：三拆账行 "正|反|断言" 逐段解析，
//!     段数≠3 / 非数字 / 负数 逐类显性拒绝；
//!   - 守恒账 `SplitLedger`：逐条目登记三拆行数，正+反+断言=总行数
//!     守恒核，破拆样本逐条拦截、账实漂移逐条定位；
//!   - 状态机 `SplitState`：Open → Balanced → Frozen 单向推进，
//!     冻结后拆分只读；
//!   - 双口径复测账 `DualRetestLedger`：J1 与 J2 两口径各 10 轮留痕，
//!     单侧复测（缺 J1 或缺 J2）逐次拦截；
//!   - 注入核 `BadSplitCase`：分段不足 / 非数字段 / 求和不守恒 /
//!     空段 / 单侧复测 五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10085 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92500;

/// 段窗上界（下一卡 0x92540 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92540;

/// 三拆账条目槽数（8 个双判据条目）。
pub const SPLIT_SLOTS: usize = 8;

/// 单条目行数预算锁定值（正 132 + 反 132 + 断言 66 = 330）。
pub const ITEM_BUDGET: u32 = 330;

/// 段预算均值（330 / 3 = 110；比例核用）。
pub const SEG_MEAN: u32 = 110;

/// 双口径复测轮数（J1 与 J2 各 10/10）。
pub const RETEST_ROUNDS: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const EBADMSG: i32 = 74;
pub const ERANGE: i32 = 34;
pub const ENODATA: i32 = 61;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitCode {
    /// 分段不足（三拆账行段数≠3）
    TooFewSegs,
    /// 非数字段（段内容含非数字字节）
    NotNumeric,
    /// 空段（某段为空）
    EmptySeg,
    /// 求和不守恒（正+反+断言≠330）
    SumBroken,
    /// 比例失衡（断言段出均值 ±60%..140% 带）
    RatioBroken,
    /// 条目越界
    SlotRange,
    /// 重复登记同条目
    DuplicateSlot,
    /// 冻结后改拆分
    FrozenMutate,
    /// 状态倒退
    StateRegress,
    /// 单侧复测（J1 或 J2 口径缺 10 轮留痕）
    OneSidedRetest,
    /// 轮账失恒（登记轮数≠留痕轮数）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl SplitCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SplitCode::TooFewSegs => "分段不足被拒：三拆账行必须恰为 正|反|断言 三段；请补齐",
            SplitCode::NotNumeric => "非数字段被拒：三段内容必须为纯数字行数；请改写",
            SplitCode::EmptySeg => "空段被拒：三段任一为空即无效；请补行数",
            SplitCode::SumBroken => "求和不守恒被拒：正+反+断言必须等于 330；请重拆",
            SplitCode::RatioBroken => "比例失衡被拒：单段行数出均值 ±40% 带即拦截；请均衡三拆",
            SplitCode::SlotRange => "条目越界被拒：登记下标只允许 0..8；请核对条目号",
            SplitCode::DuplicateSlot => "重复登记被拒：同条目已存在；请先核查再改",
            SplitCode::FrozenMutate => "冻结后改拆分被拒：协议已冻结只读；请走修订通道",
            SplitCode::StateRegress => "状态倒退被拒：Open→Balanced→Frozen 单向；请排查回滚",
            SplitCode::OneSidedRetest => "单侧复测被拒：J1 与 J2 必须各 10 轮留痕；请补另一口径",
            SplitCode::LedgerDrift => "轮账失恒被拒：登记轮数必须等于留痕轮数；请对账补录",
            SplitCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92500..=0x92540",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SplitCode; 12] = [
        SplitCode::TooFewSegs,
        SplitCode::NotNumeric,
        SplitCode::EmptySeg,
        SplitCode::SumBroken,
        SplitCode::RatioBroken,
        SplitCode::SlotRange,
        SplitCode::DuplicateSlot,
        SplitCode::FrozenMutate,
        SplitCode::StateRegress,
        SplitCode::OneSidedRetest,
        SplitCode::LedgerDrift,
        SplitCode::SegmentViolation,
    ];
}

// ── 三拆账行解析器 ────────────────────────────────────────────────────
/// 一条三拆样本：正/反/断言三段行数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriSplit {
    /// 正判据段行数
    pub pos: u32,
    /// 反判据段行数
    pub neg: u32,
    /// 断言段行数
    pub assert: u32,
}

/// 按分隔符切分成最多 3 段（返回段区间；段数≠3 返回 None）。
fn cut3(line: &str) -> Option<[usize; 4]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 2 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 2 {
        return None;
    }
    cuts[3] = b.len();
    Some(cuts)
}

/// 段数字解析：纯数字（0..=999），空段或含非数字即拒。
fn parse_seg_num(seg: &str) -> Result<u32, SplitCode> {
    if seg.is_empty() {
        return Err(SplitCode::EmptySeg);
    }
    let mut v: u32 = 0;
    for b in seg.bytes() {
        if !b.is_ascii_digit() {
            return Err(SplitCode::NotNumeric);
        }
        v = v * 10 + (b - b'0') as u32;
        if v > 999 {
            return Err(SplitCode::NotNumeric);
        }
    }
    Ok(v)
}

/// 三拆账行解析器："132|132|66" → TriSplit。
/// 段数≠3 / 空段 / 非数字 逐类显性拒绝。
pub fn parse_split_line(line: &str) -> Result<TriSplit, SplitCode> {
    let cuts = match cut3(line) {
        Some(c) => c,
        None => return Err(SplitCode::TooFewSegs),
    };
    let s0 = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let s1 = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let s2 = line.get(cuts[2]..cuts[3]).unwrap_or("");
    let pos = parse_seg_num(s0)?;
    let neg = parse_seg_num(s1)?;
    let assert_v = parse_seg_num(s2)?;
    Ok(TriSplit { pos, neg, assert: assert_v })
}

/// 三拆守恒核：求和恰等 ITEM_BUDGET（330）。
pub fn sum_ok(sp: &TriSplit) -> bool {
    sp.pos.saturating_add(sp.neg).saturating_add(sp.assert) == ITEM_BUDGET
}

/// 比例核：单段行数落在均值 110 的 60%..140% 带内（66..154）。
pub fn ratio_ok(sp: &TriSplit) -> bool {
    let lo = SEG_MEAN * 60 / 100;
    let hi = SEG_MEAN * 140 / 100;
    sp.pos >= lo && sp.pos <= hi && sp.neg >= lo && sp.neg <= hi && sp.assert >= lo && sp.assert <= hi
}

// ── 守恒账与状态机 ────────────────────────────────────────────────────
/// 状态机：Open → Balanced → Frozen 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitState {
    /// 开账（拆分登记进行时）
    Open,
    /// 已均衡（8 条目全守恒）
    Balanced,
    /// 已冻结（三拆协议封版）
    Frozen,
}

/// 守恒账：8 条目三拆逐条登记 + 状态机。
#[derive(Clone, Debug)]
pub struct SplitLedger {
    /// 已登记拆分（None=空槽）
    pub items: [Option<TriSplit>; SPLIT_SLOTS],
    /// 当前状态
    pub state: SplitState,
}

impl SplitLedger {
    /// 构造零账。
    pub const fn new() -> SplitLedger {
        SplitLedger { items: [None; SPLIT_SLOTS], state: SplitState::Open }
    }

    /// 登记一条三拆：槽号必检、守恒必检、比例必检、重复必拒、冻结后必拒。
    pub fn register(&mut self, slot: usize, sp: TriSplit) -> Result<usize, SplitCode> {
        if self.state != SplitState::Open {
            return Err(SplitCode::FrozenMutate);
        }
        if slot >= SPLIT_SLOTS {
            return Err(SplitCode::SlotRange);
        }
        if !sum_ok(&sp) {
            return Err(SplitCode::SumBroken);
        }
        if !ratio_ok(&sp) {
            return Err(SplitCode::RatioBroken);
        }
        if self.items.get(slot).copied().unwrap_or(None).is_some() {
            return Err(SplitCode::DuplicateSlot);
        }
        if let Some(s) = self.items.get_mut(slot) {
            *s = Some(sp);
        }
        Ok(slot)
    }

    /// 登记一条三拆账行（解析+登记二合一）。
    pub fn register_line(&mut self, slot: usize, line: &str) -> Result<usize, SplitCode> {
        let sp = parse_split_line(line)?;
        self.register(slot, sp)
    }

    /// 均衡：8 槽全守恒才可推进。
    pub fn balance(&mut self) -> Result<(), SplitCode> {
        if self.state != SplitState::Open {
            return Err(SplitCode::StateRegress);
        }
        let mut i = 0usize;
        while i < SPLIT_SLOTS {
            if self.items.get(i).copied().unwrap_or(None).is_none() {
                return Err(SplitCode::SumBroken);
            }
            i += 1;
        }
        self.state = SplitState::Balanced;
        Ok(())
    }

    /// 冻结：均衡后才可冻结；冻结后拆分只读。
    pub fn freeze(&mut self) -> Result<(), SplitCode> {
        if self.state != SplitState::Balanced {
            return Err(SplitCode::SumBroken);
        }
        self.state = SplitState::Frozen;
        Ok(())
    }

    /// 总行数（8 条目 × 330 = 2640）。
    pub fn total_lines(&self) -> u32 {
        self.items.iter().fold(0u32, |acc, it| match it {
            Some(_) => acc.saturating_add(ITEM_BUDGET),
            None => acc,
        })
    }
}

/// 标准三拆方案：每条目 132|132|66。
pub const SPLIT_STANDARD: TriSplit = TriSplit { pos: 132, neg: 132, assert: 66 };

/// 标准守恒账：8 条目全 132|132|66。
pub fn split_standard(ledger: &mut SplitLedger) -> Result<(), SplitCode> {
    let mut n = 0usize;
    while n < SPLIT_SLOTS {
        ledger.register(n, SPLIT_STANDARD)?;
        n += 1;
    }
    Ok(())
}

// ── 双口径复测账 ──────────────────────────────────────────────────────
/// 双判据口径闭集：J1 正口径 / J2 反口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestTrack {
    /// J1 正口径（达成路径留痕）
    J1,
    /// J2 反口径（误用路径留痕）
    J2,
}

/// 一轮双口径留痕：条目号 + 口径 + 轮号 + 是否通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrackRow {
    /// 条目号（0..8）
    pub item: usize,
    /// 口径
    pub track: RetestTrack,
    /// 轮号（1..=10）
    pub round: u32,
    /// 本轮是否通过
    pub passed: bool,
}

/// 双口径复测账：J1 与 J2 各 10 轮留痕，单侧复测逐次拦截。
#[derive(Clone, Debug)]
pub struct DualRetestLedger {
    /// 留痕行
    pub rows: Vec<TrackRow>,
    /// 登记的应测轮数（= 条目数 × 2 口径 × 10）
    pub registered: u32,
    /// 单侧复测检出数（缺口径的尝试，显性记账）
    pub one_sided: u32,
}

impl DualRetestLedger {
    /// 构造零账。
    pub fn new() -> DualRetestLedger {
        DualRetestLedger { rows: Vec::new(), registered: 0, one_sided: 0 }
    }

    /// 记一轮留痕：轮号必检、同条目同口径同轮号重复必拒。
    pub fn record(&mut self, item: usize, track: RetestTrack, round: u32, passed: bool) -> Result<(), SplitCode> {
        if round == 0 || round > RETEST_ROUNDS {
            return Err(SplitCode::LedgerDrift);
        }
        if self.rows.iter().any(|r| r.item == item && r.track == track && r.round == round) {
            return Err(SplitCode::DuplicateSlot);
        }
        self.rows.push(TrackRow { item, track, round, passed });
        Ok(())
    }

    /// 登记应测轮数：0 或超 500 拒绝。
    pub fn set_registered(&mut self, n: u32) -> Result<(), SplitCode> {
        if n == 0 || n > 500 {
            return Err(SplitCode::LedgerDrift);
        }
        self.registered = n;
        Ok(())
    }

    /// 守恒式：登记轮数 = 留痕轮数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let done = self.rows.len() as u32;
        if done == self.registered {
            None
        } else if done < self.registered {
            Some(self.registered - done)
        } else {
            Some(0)
        }
    }

    /// 单口径轮数统计。
    pub fn track_rounds(&self, track: RetestTrack) -> u32 {
        self.rows.iter().filter(|r| r.track == track).count() as u32
    }

    /// 双口径完整核：J1 与 J2 各 ≥ RETEST_ROUNDS 轮留痕。
    /// 任一口径缺 10 轮即单侧复测，显性拦截并记账。
    pub fn verify_dual(&mut self) -> Result<(), SplitCode> {
        let j1 = self.track_rounds(RetestTrack::J1);
        let j2 = self.track_rounds(RetestTrack::J2);
        if j1 < RETEST_ROUNDS || j2 < RETEST_ROUNDS {
            self.one_sided = self.one_sided.saturating_add(1);
            return Err(SplitCode::OneSidedRetest);
        }
        Ok(())
    }
}

/// 标准双口径复测：条目 0 的 J1 与 J2 各 10 轮全通过留痕。
pub fn retest_dual_standard(ledger: &mut DualRetestLedger) -> Result<(), SplitCode> {
    let _ = ledger.set_registered(20);
    let mut r = 1u32;
    while r <= RETEST_ROUNDS {
        ledger.record(0, RetestTrack::J1, r, true)?;
        ledger.record(0, RetestTrack::J2, r, true)?;
        r += 1;
    }
    ledger.verify_dual()
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏三拆样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSplitCase {
    /// 分段不足（两段/四段）
    TooFewSegs,
    /// 非数字段
    NotNumeric,
    /// 求和不守恒
    SumBroken,
    /// 空段
    EmptySeg,
    /// 单侧复测（缺 J2 口径）
    OneSidedRetest,
}

impl BadSplitCase {
    /// 闭集全量。
    pub const ALL: [BadSplitCase; 5] = [
        BadSplitCase::TooFewSegs,
        BadSplitCase::NotNumeric,
        BadSplitCase::SumBroken,
        BadSplitCase::EmptySeg,
        BadSplitCase::OneSidedRetest,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadSplitCase::TooFewSegs => {
                parse_split_line("132|132").is_err() && parse_split_line("132|132|66|66").is_err()
            }
            BadSplitCase::NotNumeric => {
                parse_split_line("132|abc|66").is_err() && parse_split_line("132|132|6x").is_err()
            }
            BadSplitCase::SumBroken => {
                let sp = TriSplit { pos: 132, neg: 130, assert: 66 };
                !sum_ok(&sp)
                    && SplitLedger::new().register_line(0, "132|130|66").err()
                        == Some(SplitCode::SumBroken)
            }
            BadSplitCase::EmptySeg => {
                parse_split_line("132||66").err() == Some(SplitCode::EmptySeg)
                    && parse_split_line("|132|66").err() == Some(SplitCode::EmptySeg)
            }
            BadSplitCase::OneSidedRetest => {
                let mut l = DualRetestLedger::new();
                let _ = l.set_registered(20);
                // 只留 J1 口径 10 轮 → 单侧复测必须被拦
                let mut r = 1u32;
                while r <= RETEST_ROUNDS {
                    let _ = l.record(0, RetestTrack::J1, r, true);
                    r += 1;
                }
                l.verify_dual() == Err(SplitCode::OneSidedRetest) && l.one_sided == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类拦截数。
pub fn run_split_injection() -> (DualRetestLedger, [u32; 5]) {
    let mut ledger = DualRetestLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSplitCase::ALL.len() {
        if let Some(case) = BadSplitCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.one_sided = ledger.one_sided.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADMSG", -74, 13, "Windows ERROR_INVALID_DATA；分段/格式错误族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非数字段/越界族"),
    ("ENODATA", -61, 2, "Windows ERROR_FILE_NOT_FOUND；空段族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；求和不守恒族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后改拆分族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：解析→登记→均衡→冻结→双口径复测全链入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut lg = SplitLedger::new();
    let _ = split_standard(&mut lg);
    bytes.push(match lg.balance() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match lg.freeze() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(lg.state as u8);
    bytes.extend_from_slice(&lg.total_lines().to_le_bytes());
    let mut dl = DualRetestLedger::new();
    bytes.push(match retest_dual_standard(&mut dl) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(match dl.drift() {
        None => 7,
        Some(_) => 8,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10085 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10085_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10085");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92500..=0x92540", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 三拆账行解析器：标准行命中
    let parsed = parse_split_line("132|132|66");
    let parse_ok = parsed == Ok(SPLIT_STANDARD);
    s.add("账行解析：132|132|66 逐段命中", parse_ok,
          "三拆账行恰三段纯数字，标准行解析后与常量逐值一致");

    // 3. 求和守恒核：132+132+66=330（锚点判据的守恒面）
    let cons_ok = sum_ok(&SPLIT_STANDARD) && ITEM_BUDGET == 330
        && SPLIT_STANDARD.pos + SPLIT_STANDARD.neg + SPLIT_STANDARD.assert == ITEM_BUDGET;
    s.add("求和守恒：132+132+66=330 逐值可复算", cons_ok,
          "三拆协议锁定总预算 330，正/反/断言求和核零偏离");

    // 4. 破拆 10/10 拦截（锚点判据）
    let mut broken_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let sp = TriSplit { pos: 132, neg: 130, assert: 66 };
        let rej = SplitLedger::new().register_line(0, "132|130|66").err()
            == Some(SplitCode::SumBroken);
        if !sum_ok(&sp) && rej {
            broken_hits += 1;
        }
        round += 1;
    }
    s.add("拆分不守恒：10/10 次拦截", broken_hits == INJECT_ROUNDS,
          "求和≠330 的三拆样本逐次显性拦截");

    // 5. 解析器坏形态：分段不足/非数字/空段逐类拒绝
    let bad_parse = parse_split_line("132|132").err() == Some(SplitCode::TooFewSegs)
        && parse_split_line("132|132|66|66").err() == Some(SplitCode::TooFewSegs)
        && parse_split_line("132|abc|66").err() == Some(SplitCode::NotNumeric)
        && parse_split_line("132||66").err() == Some(SplitCode::EmptySeg);
    s.add("坏形态：两段/四段/非数字/空段全拒", bad_parse,
          "解析器闭集：恰三段纯数字，缺段多段脏段都拒");

    // 6. 比例核：均衡方案过、单段失衡拦
    let ratio_pass = ratio_ok(&SPLIT_STANDARD);
    let ratio_rej = !ratio_ok(&TriSplit { pos: 200, neg: 64, assert: 66 });
    s.add("比例核：均值 ±40% 带内过、失衡拦", ratio_pass && ratio_rej,
          "断言段 66、正/反段 132 均落在 66..154 带内");

    // 7. 标准守恒账：8 条目全登记零偏离
    let mut lg = SplitLedger::new();
    let std_ok = split_standard(&mut lg).is_ok();
    s.add("守恒账：8 条目全 132|132|66 入账", std_ok,
          "三拆协议 8 条双判据条目逐条落账，条目守恒零偏离");

    // 8. 均衡+冻结单向闸
    let mut bl = SplitLedger::new();
    let _ = split_standard(&mut bl);
    let balanced = bl.balance().is_ok() && bl.state == SplitState::Balanced;
    let frozen = bl.freeze().is_ok() && bl.state == SplitState::Frozen;
    let after_rej = bl.register(0, SPLIT_STANDARD).is_err();
    s.add("状态闸：均衡→冻结单向，冻结后改拒", balanced && frozen && after_rej,
          "Open→Balanced→Frozen 单向推进，回写倒退显性拒绝");

    // 9. 缺槽冻结拒绝
    let mut pl = SplitLedger::new();
    let _ = pl.register(0, SPLIT_STANDARD);
    let part_rej = pl.freeze() == Err(SplitCode::SumBroken);
    s.add("缺槽闸：条目未满不得冻结", part_rej,
          "8 条目未满即冻结会被显性拦截");

    // 10. 重复登记拦截
    let mut dl2 = SplitLedger::new();
    let _ = dl2.register(0, SPLIT_STANDARD);
    let dup_ok = dl2.register(0, TriSplit { pos: 130, neg: 134, assert: 66 })
        == Err(SplitCode::DuplicateSlot);
    s.add("重复登记：同条目二次登记被拒", dup_ok,
          "一条目一拆，重复占位显性拒绝");

    // 11. 双口径复测：J1 与 J2 各 10/10 留痕（锚点判据）
    let mut rl = DualRetestLedger::new();
    let dual_ok = retest_dual_standard(&mut rl).is_ok()
        && rl.track_rounds(RetestTrack::J1) == RETEST_ROUNDS
        && rl.track_rounds(RetestTrack::J2) == RETEST_ROUNDS
        && rl.drift().is_none();
    s.add("双口径复测：J1/J2 各 10 轮留痕零漂移", dual_ok,
          "双口径各 10/10 留痕，登记=实测=20");

    // 12. 单侧复测 10/10 拦截（锚点判据）
    let mut one_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        let mut sl = DualRetestLedger::new();
        let _ = sl.set_registered(20);
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            let _ = sl.record(0, RetestTrack::J1, r, true);
            r += 1;
        }
        if sl.verify_dual() == Err(SplitCode::OneSidedRetest) && sl.one_sided == 1 {
            one_hits += 1;
        }
        round2 += 1;
    }
    s.add("单侧复测：缺 J2 口径 10/10 次拦截", one_hits == INJECT_ROUNDS,
          "只留 J1 口径的复测逐次显性拦截并记账");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_split_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "分段不足/非数字/求和不守恒/空段/单侧复测全被显式拦截");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.one_sided == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生单侧复测逃逸");

    // 15. 文案闭集：非空
    let text_ok = SplitCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < SplitCode::ALL.len() {
        let mut j = i2 + 1;
        while j < SplitCode::ALL.len() {
            if let (Some(x), Some(y)) = (SplitCode::ALL.get(i2), SplitCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = SplitCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADMSG)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENODATA)
        && agrees_with_linux(ERANGE)
        && agrees_with_linux(EPERM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：总行数快照 + 双口径快照 + 指纹快照
    let total_text = format!("8 条目 × 330 = {} 行", lg.total_lines());
    s.add_dyn("总行数快照", lg.total_lines() == SPLIT_SLOTS as u32 * ITEM_BUDGET, &total_text);
    let dual_text = format!("J1 {} 轮 / J2 {} 轮",
        rl.track_rounds(RetestTrack::J1), rl.track_rounds(RetestTrack::J2));
    s.add_dyn("双口径快照", dual_ok, &dual_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok() {
        assert_eq!(parse_split_line("132|132|66"), Ok(SPLIT_STANDARD));
        assert_eq!(parse_split_line("130|134|66").map(|sp| sum_ok(&sp)), Ok(true));
    }

    #[test]
    fn parse_bad_shapes() {
        assert_eq!(parse_split_line("132|132"), Err(SplitCode::TooFewSegs));
        assert_eq!(parse_split_line("132|132|66|66"), Err(SplitCode::TooFewSegs));
        assert_eq!(parse_split_line("132|abc|66"), Err(SplitCode::NotNumeric));
        assert_eq!(parse_split_line("132||66"), Err(SplitCode::EmptySeg));
    }

    #[test]
    fn sum_conservation() {
        assert!(sum_ok(&SPLIT_STANDARD));
        assert!(!sum_ok(&TriSplit { pos: 132, neg: 130, assert: 66 }));
        assert_eq!(ITEM_BUDGET, 330);
    }

    #[test]
    fn ratio_band() {
        assert!(ratio_ok(&SPLIT_STANDARD));
        assert!(!ratio_ok(&TriSplit { pos: 200, neg: 64, assert: 66 }));
    }

    #[test]
    fn standard_ledger_ok() {
        let mut lg = SplitLedger::new();
        assert!(split_standard(&mut lg).is_ok());
        assert!(lg.balance().is_ok());
        assert!(lg.freeze().is_ok());
        assert!(lg.register(0, SPLIT_STANDARD).is_err());
        assert_eq!(lg.total_lines(), SPLIT_SLOTS as u32 * ITEM_BUDGET);
    }

    #[test]
    fn partial_freeze_rejected() {
        let mut lg = SplitLedger::new();
        let _ = lg.register(0, SPLIT_STANDARD);
        assert_eq!(lg.freeze(), Err(SplitCode::SumBroken));
        assert_eq!(lg.balance(), Err(SplitCode::SumBroken));
    }

    #[test]
    fn duplicate_slot_rejected() {
        let mut lg = SplitLedger::new();
        let _ = lg.register(0, SPLIT_STANDARD);
        assert_eq!(lg.register(0, TriSplit { pos: 130, neg: 134, assert: 66 }),
                   Err(SplitCode::DuplicateSlot));
    }

    #[test]
    fn dual_retest_full() {
        let mut rl = DualRetestLedger::new();
        assert!(retest_dual_standard(&mut rl).is_ok());
        assert_eq!(rl.track_rounds(RetestTrack::J1), RETEST_ROUNDS);
        assert_eq!(rl.track_rounds(RetestTrack::J2), RETEST_ROUNDS);
        assert!(rl.drift().is_none());
    }

    #[test]
    fn one_sided_retest_caught() {
        let mut rl = DualRetestLedger::new();
        let _ = rl.set_registered(20);
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            let _ = rl.record(0, RetestTrack::J1, r, true);
            r += 1;
        }
        assert_eq!(rl.verify_dual(), Err(SplitCode::OneSidedRetest));
        assert_eq!(rl.one_sided, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_split_injection();
        assert_eq!(l.one_sided, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADMSG));
        assert!(agrees_with_linux(ENODATA));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ERANGE), -ERANGE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10085_checks_green() {
        let s = run_unx10085_checks();
        assert!(s.all_passed(), "UNX-F10085 red: {}", s.red_items_text());
    }
}
