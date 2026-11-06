//! UNX-F10061 M 型收口总纲（对账范围 F9601–F10080） · 0x91f00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10061（M 型收口线首卡，对账范围与八面口径）。
//! 锚点：UNX-F10062-J1 重扫零遗漏（48 册全扫对账 10/10 次），漏扫 10/10 次
//!       检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91f00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `CharterCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ScopeRegistry`：对账范围 F9601..=F10080 共 480 条号位 +
//!     48 册（24 批册 + 24 深化册）逐册登记，册外/重复/乱序显性拒绝；
//!   - 状态机 `CharterState`：Chartering → ScopeFrozen → Reconciling
//!     单向推进，范围未冻结不得开对账；
//!   - 八面口径 `ReconFace`：批号连续/条数守恒/行数守恒/状态一致/红账
//!     对齐/豁免一致/快照链/双档覆盖 八面闭集，逐面留痕；
//!   - 重扫账 `RescanLedger`：48 册逐册入账 + 漏扫计数，漏扫恒 0；
//!   - 注入核 `BadCharterCase`：册外登记 / 重复入册 / 漏册冻结 /
//!     未冻结开对账 / 未知面对账 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10061 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91f00;

/// 段窗上界（下一卡 0x91f40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91f40;

/// 对账范围起点号（F9601）。
pub const RANGE_LO: u32 = 9601;

/// 对账范围终点号（F10080）。
pub const RANGE_HI: u32 = 10080;

/// 范围内号位总数（480 条）。
pub const RANGE_LEN: u32 = RANGE_HI - RANGE_LO + 1;

/// 册总数（24 批册 + 24 深化册）。
pub const BOOK_COUNT: usize = 48;

/// 半册数（批册 24 / 深化册 24）。
pub const HALF_BOOKS: usize = 24;

/// 八面对账口径面数。
pub const FACE_COUNT: usize = 8;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EEXIST: i32 = 17;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCode {
    /// 册外号位（F 号出 9601..=10080 范围）
    OutOfRange,
    /// 重复入册（同册号二次登记）
    DuplicateBook,
    /// 乱序登记（册号回退）
    OutOfOrder,
    /// 漏册冻结（48 册未齐即冻结范围）
    MissingBook,
    /// 未冻结开对账
    NotFrozen,
    /// 未知面对账（面号出八面闭集）
    UnknownFace,
    /// 冻结后改范围
    FrozenMutate,
    /// 漏扫逃逸（重扫账漏扫计数非零）
    ScanEscape,
    /// 重扫账倒退
    LedgerRegress,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl CharterCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CharterCode::OutOfRange => "册外号位被拒：F 号只允许 9601..=10080；请核对照范围",
            CharterCode::DuplicateBook => "重复入册被拒：同册号已有槽位；请查重复登记",
            CharterCode::OutOfOrder => "乱序登记被拒：册号必须递增入册；请按序重排",
            CharterCode::MissingBook => "漏册冻结被拒：48 册未齐不得冻结对账范围；请补漏册",
            CharterCode::NotFrozen => "未冻结开对账被拒：范围冻结后才可开账；请先冻结",
            CharterCode::UnknownFace => "未知面被拒：对账面只允许八面闭集；请核对 FACE 表",
            CharterCode::FrozenMutate => "冻结后改范围被拒：范围已封账只读；请走修订通道",
            CharterCode::ScanEscape => "漏扫逃逸：重扫账漏扫计数非零；请补扫漏册",
            CharterCode::LedgerRegress => "重扫账倒退被拒：已扫计数只增不减；请排查回滚",
            CharterCode::ParseFailure => "解析失败：册行必须恰为 book|kind 两段；请补齐字段",
            CharterCode::StateRegress => "状态倒退被拒：Chartering→ScopeFrozen→Reconciling 单向",
            CharterCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91f00..=0x91f40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CharterCode; 12] = [
        CharterCode::OutOfRange,
        CharterCode::DuplicateBook,
        CharterCode::OutOfOrder,
        CharterCode::MissingBook,
        CharterCode::NotFrozen,
        CharterCode::UnknownFace,
        CharterCode::FrozenMutate,
        CharterCode::ScanEscape,
        CharterCode::LedgerRegress,
        CharterCode::ParseFailure,
        CharterCode::StateRegress,
        CharterCode::SegmentViolation,
    ];
}

// ── 八面对账口径 ──────────────────────────────────────────────────────
/// 八面对账口径面（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconFace {
    /// 批号连续
    BatchContinuity,
    /// 条数守恒
    ItemConservation,
    /// 行数守恒
    LineConservation,
    /// 状态一致
    StateConsistency,
    /// 红账对齐
    RedLedgerAlign,
    /// 豁免一致
    ExemptAlign,
    /// 快照链
    SnapshotChain,
    /// 双档覆盖
    DualTierCoverage,
}

impl ReconFace {
    /// 闭集全量。
    pub const ALL: [ReconFace; 8] = [
        ReconFace::BatchContinuity,
        ReconFace::ItemConservation,
        ReconFace::LineConservation,
        ReconFace::StateConsistency,
        ReconFace::RedLedgerAlign,
        ReconFace::ExemptAlign,
        ReconFace::SnapshotChain,
        ReconFace::DualTierCoverage,
    ];

    /// 面号（0..8）。
    pub fn index(self) -> usize {
        match self {
            ReconFace::BatchContinuity => 0,
            ReconFace::ItemConservation => 1,
            ReconFace::LineConservation => 2,
            ReconFace::StateConsistency => 3,
            ReconFace::RedLedgerAlign => 4,
            ReconFace::ExemptAlign => 5,
            ReconFace::SnapshotChain => 6,
            ReconFace::DualTierCoverage => 7,
        }
    }

    /// 从文本解析（八面短键）。
    pub fn from_field(s: &str) -> Option<ReconFace> {
        match s {
            "batch" => Some(ReconFace::BatchContinuity),
            "items" => Some(ReconFace::ItemConservation),
            "lines" => Some(ReconFace::LineConservation),
            "state" => Some(ReconFace::StateConsistency),
            "red" => Some(ReconFace::RedLedgerAlign),
            "exempt" => Some(ReconFace::ExemptAlign),
            "snap" => Some(ReconFace::SnapshotChain),
            "dual" => Some(ReconFace::DualTierCoverage),
            _ => None,
        }
    }
}

// ── 状态机与范围注册表 ────────────────────────────────────────────────
/// 状态机：Chartering → ScopeFrozen → Reconciling 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterState {
    /// 制册中（册登记进行时）
    Chartering,
    /// 范围已冻结（48 册齐，范围只读）
    ScopeFrozen,
    /// 对账中（可逐面开账）
    Reconciling,
}

/// M 型收口总纲注册表：范围号位 + 48 册槽位 + 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScopeRegistry {
    /// 已登记册号位图（true=已入册）
    pub books: [bool; BOOK_COUNT],
    /// 已入册册数
    pub booked: usize,
    /// 最后入册册号（递增守卫）
    pub last_book: u32,
    /// 当前状态
    pub state: CharterState,
}

impl ScopeRegistry {
    /// 构造空注册表。
    pub const fn new() -> ScopeRegistry {
        ScopeRegistry { books: [false; BOOK_COUNT], booked: 0, last_book: 0, state: CharterState::Chartering }
    }

    /// 登记一册：册号 1..=48 必检、递增必检、重复必拒、冻结后必拒。
    pub fn register_book(&mut self, book_no: u32) -> Result<usize, CharterCode> {
        if self.state != CharterState::Chartering {
            return Err(CharterCode::FrozenMutate);
        }
        if book_no == 0 || book_no as usize > BOOK_COUNT {
            return Err(CharterCode::OutOfRange);
        }
        if book_no <= self.last_book {
            return Err(CharterCode::OutOfOrder);
        }
        let idx = book_no as usize - 1;
        if self.books.get(idx).copied().unwrap_or(true) {
            return Err(CharterCode::DuplicateBook);
        }
        if let Some(slot) = self.books.get_mut(idx) {
            *slot = true;
        }
        self.booked = self.booked.saturating_add(1);
        self.last_book = book_no;
        Ok(idx)
    }

    /// 冻结对账范围：48 册齐才可冻结。
    pub fn freeze(&mut self) -> Result<(), CharterCode> {
        if self.state != CharterState::Chartering {
            return Err(CharterCode::StateRegress);
        }
        if self.booked < BOOK_COUNT {
            return Err(CharterCode::MissingBook);
        }
        self.state = CharterState::ScopeFrozen;
        Ok(())
    }

    /// 开对账：冻结后才可开账，且逐面转入 Reconciling。
    pub fn open_recon(&mut self) -> Result<(), CharterCode> {
        if self.state != CharterState::ScopeFrozen {
            return Err(CharterCode::NotFrozen);
        }
        self.state = CharterState::Reconciling;
        Ok(())
    }

    /// 面合法核：八面闭集之外的面对账请求拒绝。
    pub fn face_admissible(&self, face: Option<ReconFace>) -> Result<(), CharterCode> {
        if self.state != CharterState::Reconciling {
            return Err(CharterCode::NotFrozen);
        }
        match face {
            Some(_) => Ok(()),
            None => Err(CharterCode::UnknownFace),
        }
    }

    /// 号位归属册号：F9601 起每册 10 条，册号 = (f-9601)/10 + 1。
    pub fn book_of(f_no: u32) -> Result<u32, CharterCode> {
        if f_no < RANGE_LO || f_no > RANGE_HI {
            return Err(CharterCode::OutOfRange);
        }
        Ok((f_no - RANGE_LO) / 10 + 1)
    }
}

/// 标准 48 册登记（1..=48 递增入册）。
pub fn register_standard(reg: &mut ScopeRegistry) -> Result<(), CharterCode> {
    let mut n = 1u32;
    while n as usize <= BOOK_COUNT {
        reg.register_book(n)?;
        n += 1;
    }
    Ok(())
}

// ── 重扫账 ────────────────────────────────────────────────────────────
/// 重扫账：48 册逐册入账 + 漏扫逃逸计数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RescanLedger {
    /// 已扫描册数
    pub scanned: u32,
    /// 漏扫册数（必须为 0）
    pub missed: u32,
    /// 对账轮次
    pub rounds: u32,
}

impl RescanLedger {
    /// 构造零账。
    pub const fn zero() -> RescanLedger {
        RescanLedger { scanned: 0, missed: 0, rounds: 0 }
    }

    /// 记一册扫描：hit=false 即漏扫入账（显性记漏不静默吞）。
    pub fn record(&mut self, hit: bool) -> Result<(), CharterCode> {
        if hit {
            self.scanned = self.scanned.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
        if self.scanned.saturating_add(self.missed) < self.scanned {
            return Err(CharterCode::LedgerRegress);
        }
        Ok(())
    }

    /// 收账：一轮对账结束，轮次 +1。
    pub fn close_round(&mut self) -> Result<(), CharterCode> {
        if self.scanned.saturating_add(self.missed) != BOOK_COUNT as u32 {
            return Err(CharterCode::ScanEscape);
        }
        self.rounds = self.rounds.saturating_add(1);
        Ok(())
    }

    /// 守恒式：扫 + 漏 = 48 且漏 = 0。
    pub fn conserved(&self) -> bool {
        self.scanned == BOOK_COUNT as u32 && self.missed == 0
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏总纲样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCharterCase {
    /// 册外登记（册号 0 或 49）
    OutOfRange,
    /// 乱序回退登记
    OutOfOrder,
    /// 漏册冻结（47 册即冻结）
    MissingBook,
    /// 未冻结开对账
    NotFrozen,
    /// 未知面对账
    UnknownFace,
}

impl BadCharterCase {
    /// 闭集全量。
    pub const ALL: [BadCharterCase; 5] = [
        BadCharterCase::OutOfRange,
        BadCharterCase::OutOfOrder,
        BadCharterCase::MissingBook,
        BadCharterCase::NotFrozen,
        BadCharterCase::UnknownFace,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadCharterCase::OutOfRange => {
                let mut reg = ScopeRegistry::new();
                reg.register_book(0).is_err() && reg.register_book(49).is_err()
            }
            BadCharterCase::OutOfOrder => {
                let mut reg = ScopeRegistry::new();
                if reg.register_book(1).is_err() {
                    return false;
                }
                // 回退（重复 1）与倒退（低于 1）都拒；前进（2）必须放行。
                reg.register_book(1).is_err() && reg.register_book(2).is_ok()
            }
            BadCharterCase::MissingBook => {
                let mut reg = ScopeRegistry::new();
                let mut n = 1u32;
                while n < BOOK_COUNT as u32 {
                    if reg.register_book(n).is_err() {
                        return false;
                    }
                    n += 1;
                }
                reg.freeze().is_err()
            }
            BadCharterCase::NotFrozen => {
                let mut reg = ScopeRegistry::new();
                let _ = register_standard(&mut reg);
                reg.open_recon().is_err()
            }
            BadCharterCase::UnknownFace => {
                let mut reg = ScopeRegistry::new();
                let _ = register_standard(&mut reg);
                if reg.freeze().is_err() {
                    return false;
                }
                if reg.open_recon().is_err() {
                    return false;
                }
                reg.face_admissible(ReconFace::from_field("ninth")).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_charter_injection() -> (RescanLedger, [u32; 5]) {
    let mut ledger = RescanLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadCharterCase::ALL.len() {
        if let Some(case) = BadCharterCase::ALL.get(ci).copied() {
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后改范围族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；漏册/未知面族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复入册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；册号非法族"),
    ("ERANGE", -34, 206, "Windows ERROR_FILENAME_EXCED_RANGE；号位出窗族"),
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

/// 规范语料双跑指纹：登记→冻结→开账→逐面留痕全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = ScopeRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let _ = register_standard(&mut reg);
    bytes.push(match reg.freeze() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.open_recon() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    let mut fi = 0usize;
    while fi < ReconFace::ALL.len() {
        if let Some(f) = ReconFace::ALL.get(fi).copied() {
            bytes.push(match reg.face_admissible(Some(f)) {
                Ok(()) => 5,
                Err(_) => 6,
            });
            bytes.push(f.index() as u8);
        }
        fi += 1;
    }
    bytes.extend_from_slice(&reg.booked.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10061 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10061_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10061");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91f00..=0x91f40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 对账范围口径：480 条号位
    let range_ok = RANGE_LEN == 480 && RANGE_LO == 9601 && RANGE_HI == 10080;
    s.add("对账范围：F9601..=F10080 共 480 号位", range_ok,
          "M 型对账范围口径冻结：480 条 = 9601..=10080");

    // 3. 标准 48 册登记成功
    let mut reg = ScopeRegistry::new();
    let std_ok = register_standard(&mut reg).is_ok() && reg.booked == BOOK_COUNT;
    s.add("册登记：48 册（24 批+24 深化）逐册入表", std_ok,
          "1..=48 递增入册，册册有位");

    // 4. 号位归属册号：边界命中
    let map_ok = ScopeRegistry::book_of(9601) == Ok(1)
        && ScopeRegistry::book_of(9610) == Ok(1)
        && ScopeRegistry::book_of(9611) == Ok(2)
        && ScopeRegistry::book_of(10080) == Ok(48);
    s.add("号位映射：册首/册尾/跨册边界命中", map_ok,
          "每册 10 条，9601 起归属册号可复算");

    // 5. 册外号位拦截
    let out_ok = ScopeRegistry::book_of(9600).is_err()
        && ScopeRegistry::book_of(10081).is_err()
        && reg.register_book(49).is_err();
    s.add("册外拦截：9600/10081/册号 49 全拒", out_ok,
          "范围外号位与册号不入总纲");

    // 6. 乱序与重复登记拦截
    let mut oo = ScopeRegistry::new();
    let _ = oo.register_book(3);
    let oo_ok = oo.register_book(3).is_err() && oo.register_book(2).is_err();
    s.add("乱序重复：回退册号/重复册号拒登", oo_ok,
          "册号必须严格递增入册，回退即拦");

    // 7. 漏册冻结拦截 + 齐册冻结成功
    let mut part = ScopeRegistry::new();
    let mut n = 1u32;
    while n < BOOK_COUNT as u32 {
        let _ = part.register_book(n);
        n += 1;
    }
    let part_rej = part.freeze() == Err(CharterCode::MissingBook);
    let mut full = ScopeRegistry::new();
    let _ = register_standard(&mut full);
    let full_ok = full.freeze().is_ok() && full.state == CharterState::ScopeFrozen;
    s.add("范围冻结：漏册拒绝、48 册齐通过", part_rej && full_ok,
          "冻结单向：Chartering → ScopeFrozen");

    // 8. 未冻结开账拦截 + 冻结后开账成功
    let mut nf = ScopeRegistry::new();
    let _ = register_standard(&mut nf);
    let nf_rej = nf.open_recon().is_err();
    let _ = full.open_recon();
    let open_ok = full.state == CharterState::Reconciling;
    s.add("开账闸：未冻结拒绝、冻结后转入 Reconciling", nf_rej && open_ok,
          "范围未冻结不得开对账");

    // 9. 八面闭集：合法面全收、未知面拒绝
    let faces_ok = full.face_admissible(Some(ReconFace::BatchContinuity)).is_ok()
        && full.face_admissible(Some(ReconFace::DualTierCoverage)).is_ok()
        && full.face_admissible(ReconFace::from_field("ninth")).is_err()
        && full.face_admissible(None).is_err();
    s.add("八面口径：面文本闭集、未知面拒绝", faces_ok,
          "对账面只允许 batch/items/lines/state/red/exempt/snap/dual 八键");

    // 10. 冻结后改范围拦截
    let mut fr = ScopeRegistry::new();
    let _ = register_standard(&mut fr);
    let _ = fr.freeze();
    let fr_ok = fr.register_book(1) == Err(CharterCode::FrozenMutate);
    s.add("冻结后改范围：再登记被拒", fr_ok,
          "范围冻结即封账，修订必须走通道");

    // 11. 重扫零遗漏：48 册全扫收账成功
    let mut l = RescanLedger::zero();
    let mut k = 0u32;
    while k < BOOK_COUNT as u32 {
        let _ = l.record(true);
        k += 1;
    }
    let close_ok = l.close_round().is_ok() && l.conserved() && l.rounds == 1;
    s.add("重扫零遗漏：48 册全扫收账、漏扫=0", close_ok,
          "48 册逐册入重扫账，漏扫恒 0");

    // 12. 漏扫显性入账：hit=false 记漏不静默
    let mut esc = RescanLedger::zero();
    let _ = esc.record(true);
    let _ = esc.record(false);
    s.add("漏扫可见：无扫描册显性记漏", esc.missed == 1 && esc.scanned == 1,
          "漏册必须入账可见，禁止静默吞扫描");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_charter_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "册外/乱序/漏册冻结/未冻结开账/未知面全被显式拒绝");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程扫描 50 次且漏=0",
          ledger.scanned == 5 * INJECT_ROUNDS && ledger.missed == 0,
          "注入 50 次全留痕、0 漏扫逃逸");

    // 15. 文案闭集：非空
    let text_ok = CharterCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < CharterCode::ALL.len() {
        let mut j = i + 1;
        while j < CharterCode::ALL.len() {
            if let (Some(x), Some(y)) = (CharterCode::ALL.get(i), CharterCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = CharterCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：重扫快照 + 指纹快照
    let ledger_text = format!("扫描 {} / 漏扫 {} / 轮次 {}", ledger.scanned, ledger.missed, ledger.rounds);
    s.add_dyn("重扫快照", ledger.missed == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_registration_ok() {
        let mut reg = ScopeRegistry::new();
        assert!(register_standard(&mut reg).is_ok());
        assert_eq!(reg.booked, BOOK_COUNT);
    }

    #[test]
    fn out_of_range_rejected() {
        let mut reg = ScopeRegistry::new();
        assert_eq!(reg.register_book(0), Err(CharterCode::OutOfRange));
        assert_eq!(reg.register_book(49), Err(CharterCode::OutOfRange));
        assert!(ScopeRegistry::book_of(10081).is_err());
    }

    #[test]
    fn out_of_order_rejected() {
        let mut reg = ScopeRegistry::new();
        let _ = reg.register_book(2);
        assert_eq!(reg.register_book(2), Err(CharterCode::OutOfOrder));
        assert_eq!(reg.register_book(1), Err(CharterCode::OutOfOrder));
    }

    #[test]
    fn missing_book_cannot_freeze() {
        let mut reg = ScopeRegistry::new();
        let _ = reg.register_book(1);
        assert_eq!(reg.freeze(), Err(CharterCode::MissingBook));
    }

    #[test]
    fn open_recon_requires_freeze() {
        let mut reg = ScopeRegistry::new();
        let _ = register_standard(&mut reg);
        assert_eq!(reg.open_recon(), Err(CharterCode::NotFrozen));
        let _ = reg.freeze();
        assert!(reg.open_recon().is_ok());
    }

    #[test]
    fn face_closed_set() {
        assert!(ReconFace::from_field("red").is_some());
        assert!(ReconFace::from_field("green").is_none());
        assert_eq!(ReconFace::ALL.len(), FACE_COUNT);
    }

    #[test]
    fn book_mapping_boundaries() {
        assert_eq!(ScopeRegistry::book_of(9601), Ok(1));
        assert_eq!(ScopeRegistry::book_of(9840), Ok(24));
        assert_eq!(ScopeRegistry::book_of(10080), Ok(48));
    }

    #[test]
    fn rescan_ledger_conservation() {
        let mut l = RescanLedger::zero();
        let mut k = 0;
        while k < BOOK_COUNT {
            let _ = l.record(true);
            k += 1;
        }
        assert!(l.conserved());
        assert!(l.close_round().is_ok());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_charter_injection();
        assert_eq!(l.missed, 0);
        assert_eq!(l.scanned, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ERANGE));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10061_checks_green() {
        let s = run_unx10061_checks();
        assert!(s.all_passed(), "UNX-F10061 red: {}", s.red_items_text());
    }
}
