//! UNX-F10066 状态机一致性核验（[已深化] 全检） · 0x92040 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10066（M 型收口线：三册条目状态机一致性）。
//! 锚点：UNX-F10067-J1 三方对账零差异（批号/条数/行数逐格 10/10 次），
//!       差异不记 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92040（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `StateCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `EntryState`：Raw（原始）→ Deepened（[已深化]）单向迁移，
//!     迁移表 `transition_ok` 闭集判定，倒退迁移显性拒绝；
//!   - 三册账 `TriBookRegistry`：总纲/批册/深化册各 480 条状态位图，
//!     逐条置态、逐条核对（三册同条同态）；
//!   - 解析器 `parse_state_text`：状态文本 "Raw"/"[已深化]" 闭集解析，
//!     未知态文本显性拒绝；
//!   - 状态账 `StateLedger`：核对条数 + 异态逃逸数（恒 0）；
//!   - 注入核 `BadStateCase`：异态条目 / 非法倒退迁移 / 半迁移 /
//!     未知态文本 / 越窗条目 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10066 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92040;

/// 段窗上界（下一卡 0x92080 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92080;

/// 条目号窗起点（F9601）。
pub const ID_LO: u32 = 9601;

/// 条目号窗终点（F10080）。
pub const ID_HI: u32 = 10080;

/// 条目总数（480）。
pub const ENTRY_TOTAL: u32 = ID_HI - ID_LO + 1;

/// 册数（总纲/批册/深化册）。
pub const BOOK_COUNT: usize = 3;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;
pub const EIO: i32 = 5;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateCode {
    /// 越窗条目（号出 9601..=10080）
    OutOfWindow,
    /// 未知态文本
    UnknownStateText,
    /// 非法迁移（Deepened→Raw 倒退）
    IllegalTransition,
    /// 半迁移（三册同条异态）
    HalfMigrated,
    /// 册号越界（出 0..3）
    BookRange,
    /// 全检未过出结论
    NotFullyChecked,
    /// 状态账倒退
    LedgerRegress,
    /// 异态逃逸（账面漏计异态）
    DivergeEscape,
    /// 重复置态（同册同条重复迁移）
    DoubleMark,
    /// 解析失败
    ParseFailure,
    /// 状态倒退（核验状态机自身回退）
    PhaseRegress,
    /// 段位越界
    SegmentViolation,
}

impl StateCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StateCode::OutOfWindow => "越窗条目被拒：号只允许 9601..=10080；请核对条目号",
            StateCode::UnknownStateText => "未知态文本被拒：状态只允许 Raw/[已深化]；请核对文本",
            StateCode::IllegalTransition => "非法迁移被拒：[已深化] 不得回退 Raw；请走修订通道",
            StateCode::HalfMigrated => "半迁移被拒：三册同条必须同态；请补齐其余册置态",
            StateCode::BookRange => "册号越界被拒：册只允许 总纲/批册/深化册；请核对册号",
            StateCode::NotFullyChecked => "全检未过被拒：480 条未逐条核对不得出结论",
            StateCode::LedgerRegress => "状态账倒退被拒：核对计数只增不减；请排查回滚",
            StateCode::DivergeEscape => "异态逃逸：账面漏计异态；请补记异态明细",
            StateCode::DoubleMark => "重复置态被拒：同册同条已置 [已深化]；请查重复迁移",
            StateCode::ParseFailure => "解析失败：条目 ID 必须为 F<num>；请补齐格式",
            StateCode::PhaseRegress => "核验状态倒退被拒：Marking→Checked 单向",
            StateCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92040..=0x92080",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [StateCode; 12] = [
        StateCode::OutOfWindow,
        StateCode::UnknownStateText,
        StateCode::IllegalTransition,
        StateCode::HalfMigrated,
        StateCode::BookRange,
        StateCode::NotFullyChecked,
        StateCode::LedgerRegress,
        StateCode::DivergeEscape,
        StateCode::DoubleMark,
        StateCode::ParseFailure,
        StateCode::PhaseRegress,
        StateCode::SegmentViolation,
    ];
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 条目状态（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 原始
    Raw,
    /// 已深化
    Deepened,
}

impl EntryState {
    /// 状态文本（"[已深化]" 为 M 型冻结记号）。
    pub fn text(self) -> &'static str {
        match self {
            EntryState::Raw => "Raw",
            EntryState::Deepened => "[已深化]",
        }
    }

    /// 从文本解析（闭集）。
    pub fn from_text(s: &str) -> Option<EntryState> {
        match s {
            "Raw" => Some(EntryState::Raw),
            "[已深化]" => Some(EntryState::Deepened),
            _ => None,
        }
    }

    /// 状态序号（0=Raw，1=Deepened）。
    pub fn rank(self) -> u32 {
        match self {
            EntryState::Raw => 0,
            EntryState::Deepened => 1,
        }
    }
}

/// 迁移表：Raw→Deepened 合法；Raw→Raw 幂等；Deepened→Raw 倒退非法。
pub fn transition_ok(from: EntryState, to: EntryState) -> bool {
    to.rank() >= from.rank()
}

/// 状态文本解析器：非法文本 → UnknownStateText。
pub fn parse_state_text(s: &str) -> Result<EntryState, StateCode> {
    match EntryState::from_text(s) {
        Some(st) => Ok(st),
        None => Err(StateCode::UnknownStateText),
    }
}

/// 条目 ID 解析："F9705" → 9705。
pub fn parse_entry_id(s: &str) -> Result<u32, StateCode> {
    let body = match s.strip_prefix('F') {
        Some(b) => b,
        None => return Err(StateCode::ParseFailure),
    };
    if body.is_empty() || body.len() > 5 {
        return Err(StateCode::ParseFailure);
    }
    let mut n: u32 = 0;
    for b in body.bytes() {
        if !b.is_ascii_digit() {
            return Err(StateCode::ParseFailure);
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    if n < ID_LO || n > ID_HI {
        return Err(StateCode::OutOfWindow);
    }
    Ok(n)
}

// ── 单条状态机账 ──────────────────────────────────────────────────────
/// 单条状态机账：Raw → Deepened 单向，倒退拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkLedger {
    /// 当前态
    pub state: EntryState,
    /// 迁移次数
    pub transitions: u32,
}

impl MarkLedger {
    /// 构造 Raw 账。
    pub const fn new() -> MarkLedger {
        MarkLedger { state: EntryState::Raw, transitions: 0 }
    }

    /// 迁移：按迁移表判定；倒退显性拒绝。
    pub fn transition(&mut self, to: EntryState) -> Result<(), StateCode> {
        if !transition_ok(self.state, to) {
            return Err(StateCode::IllegalTransition);
        }
        if self.state == to {
            return Ok(()); // 幂等：同态迁移无副作用
        }
        self.state = to;
        self.transitions = self.transitions.saturating_add(1);
        Ok(())
    }
}

// ── 三册账 ────────────────────────────────────────────────────────────
/// 核验阶段：Marking → Checked 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 置态中
    Marking,
    /// 已全检
    Checked,
}

/// 三册状态账：总纲/批册/深化册各 480 条 Deepened 位图。
#[derive(Clone, Debug)]
pub struct TriBookRegistry {
    /// 三册位图（true=[已深化]）
    pub books: [Vec<bool>; BOOK_COUNT],
    /// 已核对条数
    pub checked: u32,
    /// 当前阶段
    pub phase: Phase,
}

impl TriBookRegistry {
    /// 构造空账（三册全 Raw）。
    pub fn new() -> TriBookRegistry {
        TriBookRegistry {
            books: [
                vec![false; ENTRY_TOTAL as usize],
                vec![false; ENTRY_TOTAL as usize],
                vec![false; ENTRY_TOTAL as usize],
            ],
            checked: 0,
            phase: Phase::Marking,
        }
    }

    /// 条目下标。
    fn idx(num: u32) -> usize {
        (num - ID_LO) as usize
    }

    /// 单册置态：册号必检、号必检、重复迁移必拒。
    pub fn mark(&mut self, book: usize, num: u32) -> Result<(), StateCode> {
        if self.phase != Phase::Marking {
            return Err(StateCode::PhaseRegress);
        }
        if book >= BOOK_COUNT {
            return Err(StateCode::BookRange);
        }
        if num < ID_LO || num > ID_HI {
            return Err(StateCode::OutOfWindow);
        }
        let i = Self::idx(num);
        let row = match self.books.get_mut(book) {
            Some(r) => r,
            None => return Err(StateCode::BookRange),
        };
        if row.get(i).copied().unwrap_or(true) {
            return Err(StateCode::DoubleMark);
        }
        if let Some(slot) = row.get_mut(i) {
            *slot = true;
        }
        Ok(())
    }

    /// 单册读态。
    pub fn state_of(&self, book: usize, num: u32) -> Option<EntryState> {
        if book >= BOOK_COUNT || num < ID_LO || num > ID_HI {
            return None;
        }
        let i = Self::idx(num);
        self.books
            .get(book)
            .and_then(|r| r.get(i))
            .map(|deep| if *deep { EntryState::Deepened } else { EntryState::Raw })
    }

    /// 异态核对：三册同条同态才通过；返回首个异态条目号。
    pub fn first_divergent(&self) -> Option<u32> {
        let mut n = ID_LO;
        while n <= ID_HI {
            let i = Self::idx(n);
            let mut b0 = None;
            let mut bi = 0usize;
            while bi < BOOK_COUNT {
                let cur = self
                    .books
                    .get(bi)
                    .and_then(|r| r.get(i))
                    .copied()
                    .unwrap_or(false);
                match b0 {
                    None => b0 = Some(cur),
                    Some(base) => {
                        if base != cur {
                            return Some(n);
                        }
                    }
                }
                bi += 1;
            }
            n += 1;
        }
        None
    }

    /// 全检：逐条核对走账；有异态则拒绝并保留定位；核完转 Checked。
    pub fn check_all(&mut self, ledger: &mut StateLedger) -> Result<(), StateCode> {
        if self.phase != Phase::Marking {
            return Err(StateCode::PhaseRegress);
        }
        let mut n = ID_LO;
        while n <= ID_HI {
            let same = match self.first_divergent_at(n) {
                Some(_) => false,
                None => true,
            };
            let _ = ledger.record(same);
            if !same {
                return Err(StateCode::HalfMigrated);
            }
            n += 1;
        }
        self.checked = ledger.checked;
        self.phase = Phase::Checked;
        Ok(())
    }

    /// 指定条目是否三册同态。
    fn first_divergent_at(&self, num: u32) -> Option<u32> {
        let i = Self::idx(num);
        let base = self
            .books
            .first()
            .and_then(|r| r.get(i))
            .copied()
            .unwrap_or(false);
        let mut bi = 1usize;
        while bi < BOOK_COUNT {
            let cur = self
                .books
                .get(bi)
                .and_then(|r| r.get(i))
                .copied()
                .unwrap_or(false);
            if cur != base {
                return Some(num);
            }
            bi += 1;
        }
        None
    }
}

/// 状态账：核对条数 + 异态逃逸数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateLedger {
    /// 已核对条数
    pub checked: u32,
    /// 异态逃逸数（必须为 0）
    pub diverge_escape: u32,
}

impl StateLedger {
    /// 构造零账。
    pub const fn zero() -> StateLedger {
        StateLedger { checked: 0, diverge_escape: 0 }
    }

    /// 记一条核对；same=false 即异态显性入账（不静默）。
    pub fn record(&mut self, same: bool) -> Result<(), StateCode> {
        self.checked = self.checked.saturating_add(1);
        if !same {
            self.diverge_escape = self.diverge_escape.saturating_add(1);
        }
        Ok(())
    }
}

/// 标准全量置态：480 条三册全置 [已深化]。
pub fn mark_full(reg: &mut TriBookRegistry) -> Result<(), StateCode> {
    let mut n = ID_LO;
    while n <= ID_HI {
        let mut b = 0usize;
        while b < BOOK_COUNT {
            reg.mark(b, n)?;
            b += 1;
        }
        n += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏状态样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadStateCase {
    /// 非法倒退迁移（Deepened→Raw）
    IllegalTransition,
    /// 半迁移（只置总纲一册）
    HalfMigrated,
    /// 未知态文本
    UnknownStateText,
    /// 越窗条目
    OutOfWindow,
    /// 重复置态
    DoubleMark,
}

impl BadStateCase {
    /// 闭集全量。
    pub const ALL: [BadStateCase; 5] = [
        BadStateCase::IllegalTransition,
        BadStateCase::HalfMigrated,
        BadStateCase::UnknownStateText,
        BadStateCase::OutOfWindow,
        BadStateCase::DoubleMark,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadStateCase::IllegalTransition => {
                let mut m = MarkLedger::new();
                if m.transition(EntryState::Deepened).is_err() {
                    return false;
                }
                m.transition(EntryState::Raw).is_err()
            }
            BadStateCase::HalfMigrated => {
                let mut reg = TriBookRegistry::new();
                let mut ledger = StateLedger::zero();
                if reg.mark(0, 9650).is_err() {
                    return false;
                }
                reg.check_all(&mut ledger).is_err()
                    && reg.first_divergent() == Some(9650)
                    && ledger.diverge_escape == 1
            }
            BadStateCase::UnknownStateText => {
                parse_state_text("已深化").is_err()
                    && parse_state_text("[deepened]").is_err()
                    && parse_state_text("").is_err()
            }
            BadStateCase::OutOfWindow => {
                parse_entry_id("F9600").is_err()
                    && parse_entry_id("F10081").is_err()
                    && parse_entry_id("X9700").is_err()
                    && parse_entry_id("F970x").is_err()
            }
            BadStateCase::DoubleMark => {
                let mut reg = TriBookRegistry::new();
                let a = reg.mark(1, 9701).is_ok();
                let b = reg.mark(1, 9701).is_err();
                a && b
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_state_injection() -> (StateLedger, [u32; 5]) {
    let mut ledger = StateLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadStateCase::ALL.len() {
        if let Some(case) = BadStateCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；倒退迁移/封账后置态族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知态文本族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；半迁移 IO 不一致族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；格式族"),
    ("ERANGE", -34, 206, "Windows ERROR_FILENAME_EXCED_RANGE；越窗族"),
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

/// 规范语料双跑指纹：全量置态→全检→异态探测全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = TriBookRegistry::new();
    let mut ledger = StateLedger::zero();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match mark_full(&mut reg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.check_all(&mut ledger) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(reg.phase as u8);
    bytes.extend_from_slice(&ledger.checked.to_le_bytes());
    bytes.extend_from_slice(&ledger.diverge_escape.to_le_bytes());
    // 异态场景：只置总纲 9800 → 首异态定位入哈希。
    let mut half = TriBookRegistry::new();
    let _ = half.mark(0, 9800);
    bytes.push(match half.first_divergent() {
        Some(9800) => 5,
        _ => 6,
    });
    bytes.push(EntryState::Deepened.rank() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10066 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10066_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10066");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92040..=0x92080", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 状态机迁移表：前向放行、倒退拒绝、同态幂等
    let trans_ok = transition_ok(EntryState::Raw, EntryState::Deepened)
        && transition_ok(EntryState::Raw, EntryState::Raw)
        && !transition_ok(EntryState::Deepened, EntryState::Raw);
    s.add("迁移表：Raw→[已深化] 放行、倒退拒绝、同态幂等", trans_ok,
          "迁移闭集按秩比较，倒退显性拒绝");

    // 3. 单条状态机账：迁移计数与幂等
    let mut m = MarkLedger::new();
    let idem = m.transition(EntryState::Raw).is_ok() && m.transitions == 0;
    let fwd = m.transition(EntryState::Deepened).is_ok() && m.transitions == 1
        && m.state.text() == "[已深化]";
    let back = m.transition(EntryState::Raw).is_err();
    s.add("状态机账：幂等零副作用、前向计数、倒退拒绝", idem && fwd && back,
          "迁移账可复算：幂等迁移不计数");

    // 4. 状态文本闭集
    let text_ok = parse_state_text("Raw") == Ok(EntryState::Raw)
        && parse_state_text("[已深化]") == Ok(EntryState::Deepened)
        && parse_state_text("已深化").is_err()
        && parse_state_text("").is_err();
    s.add("状态文本：Raw/[已深化] 命中、变体全拒", text_ok,
          "状态文本闭集，未知文本显性拒绝");

    // 5. 条目 ID 解析
    let id_ok = parse_entry_id("F9601") == Ok(9601)
        && parse_entry_id("F10080") == Ok(10080)
        && parse_entry_id("F9600").is_err()
        && parse_entry_id("F10081").is_err()
        && parse_entry_id("F970x").is_err();
    s.add("ID 解析：窗内命中、窗外/非数字拒绝", id_ok,
          "条目号窗冻结 9601..=10080");

    // 6. 标准全量置态+全检
    let mut reg = TriBookRegistry::new();
    let mut ledger = StateLedger::zero();
    let full_ok = mark_full(&mut reg).is_ok()
        && reg.check_all(&mut ledger).is_ok()
        && reg.phase == Phase::Checked
        && ledger.checked == ENTRY_TOTAL
        && ledger.diverge_escape == 0;
    s.add("全检通过：480 条三册同条同态", full_ok,
          "1440 次置态全成功，逐条核对 480/480");

    // 7. 册号越界拦截
    let mut br = TriBookRegistry::new();
    let br_ok = br.mark(3, 9601).is_err() && br.mark(usize::MAX, 9601).is_err();
    s.add("册号窗：只允许 0..3 三册", br_ok,
          "总纲/批册/深化册之外无册");

    // 8. 重复置态拦截
    let mut dm = TriBookRegistry::new();
    let _ = dm.mark(2, 9701);
    let dm_ok = dm.mark(2, 9701).is_err();
    s.add("重复置态：同册同条二次迁移被拒", dm_ok,
          "一册一条一态，重复显性拒绝");

    // 9. 半迁移检出：只置总纲，核对拒绝且定位
    let mut hm = TriBookRegistry::new();
    let mut hm_ledger = StateLedger::zero();
    let _ = hm.mark(0, 9650);
    let hm_ok = hm.check_all(&mut hm_ledger).is_err()
        && hm.first_divergent() == Some(9650)
        && hm_ledger.diverge_escape == 1;
    s.add("半迁移：异态检出并定位 9650、逃逸入账", hm_ok,
          "三册同条异态必须显性检出，逃逸恒计账");

    // 10. 异态定位在中间册与尾册
    let mut mid = TriBookRegistry::new();
    let _ = mid.mark(1, 9900);
    let mut tail = TriBookRegistry::new();
    let _ = tail.mark(2, 10080);
    let loc_ok = mid.first_divergent() == Some(9900) && tail.first_divergent() == Some(10080);
    s.add("异态定位：中间册/尾册均可指认", loc_ok,
          "定位与册序无关，逐条扫描可复算");

    // 11. 全检后置态拦截（阶段闸）
    let mut ph = TriBookRegistry::new();
    let mut ph_ledger = StateLedger::zero();
    let _ = mark_full(&mut ph);
    let _ = ph.check_all(&mut ph_ledger);
    let ph_ok = ph.mark(0, 9601) == Err(StateCode::PhaseRegress);
    s.add("阶段闸：Checked 后再置态被拒", ph_ok,
          "Marking → Checked 单向，核后只读");

    // 12. 注入核：五类坏样本全检出
    let (ledger2, per_class) = run_state_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "倒退/半迁移/未知文本/越窗/重复全被显式拒绝");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮全检出逃逸=0",
          ledger2.checked == 5 * INJECT_ROUNDS && ledger2.diverge_escape == 0,
          "注入核账守恒：50 次注入 50 次检出、0 逃逸");

    // 14. 文案闭集：非空
    let text_ok2 = StateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok2,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < StateCode::ALL.len() {
        let mut j = i + 1;
        while j < StateCode::ALL.len() {
            if let (Some(x), Some(y)) = (StateCode::ALL.get(i), StateCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = StateCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EIO)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：全检快照 + 指纹快照
    let snap_text = format!("核对 {}/480 / 异态逃逸 {}", ledger.checked, ledger.diverge_escape);
    s.add_dyn("全检快照", ledger.checked == ENTRY_TOTAL && ledger.diverge_escape == 0, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        assert!(transition_ok(EntryState::Raw, EntryState::Deepened));
        assert!(!transition_ok(EntryState::Deepened, EntryState::Raw));
    }

    #[test]
    fn mark_ledger_idempotent() {
        let mut m = MarkLedger::new();
        assert!(m.transition(EntryState::Raw).is_ok());
        assert_eq!(m.transitions, 0);
        assert!(m.transition(EntryState::Deepened).is_ok());
        assert_eq!(m.transitions, 1);
        assert!(m.transition(EntryState::Raw).is_err());
    }

    #[test]
    fn state_text_closed_set() {
        assert_eq!(EntryState::from_text("[已深化]"), Some(EntryState::Deepened));
        assert_eq!(EntryState::from_text("Deepened"), None);
    }

    #[test]
    fn entry_id_window() {
        assert_eq!(parse_entry_id("F9601"), Ok(9601));
        assert!(parse_entry_id("F10081").is_err());
        assert!(parse_entry_id("F9600").is_err());
    }

    #[test]
    fn full_mark_and_check() {
        let mut reg = TriBookRegistry::new();
        let mut ledger = StateLedger::zero();
        assert!(mark_full(&mut reg).is_ok());
        assert!(reg.check_all(&mut ledger).is_ok());
        assert_eq!(ledger.checked, ENTRY_TOTAL);
        assert_eq!(ledger.diverge_escape, 0);
    }

    #[test]
    fn half_migration_detected() {
        let mut reg = TriBookRegistry::new();
        let mut ledger = StateLedger::zero();
        let _ = reg.mark(0, 9650);
        assert!(reg.check_all(&mut ledger).is_err());
        assert_eq!(reg.first_divergent(), Some(9650));
        assert_eq!(ledger.diverge_escape, 1);
    }

    #[test]
    fn double_mark_rejected() {
        let mut reg = TriBookRegistry::new();
        assert!(reg.mark(1, 9701).is_ok());
        assert_eq!(reg.mark(1, 9701), Err(StateCode::DoubleMark));
    }

    #[test]
    fn book_range_rejected() {
        let mut reg = TriBookRegistry::new();
        assert_eq!(reg.mark(3, 9601), Err(StateCode::BookRange));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_state_injection();
        assert_eq!(l.checked, 5 * INJECT_ROUNDS);
        assert_eq!(l.diverge_escape, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EIO));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ERANGE), -ERANGE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10066_checks_green() {
        let s = run_unx10066_checks();
        assert!(s.all_passed(), "UNX-F10066 red: {}", s.red_items_text());
    }
}
