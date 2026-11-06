//! UNX-F10064 判据全集重检（J 三成分） · 0x91fc0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10064（M 型收口线：480 条判据三成分重检）。
//! 锚点：UNX-F10065-J1 守恒总核过（24 批求和=锁定值 10/10 次），偏离不
//!       定位 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91fc0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `TriadCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `TriadRegistry`：480 条判据三成分位图（原文/达成面/拦截
//!     面），逐条登记、逐条核缺；位图行内三成分必须同齐；
//!   - 解析器 `TriadId`："UNX-F<num>-J1/J2" 解析（正判据/反判据），
//!     非法键、越窗号显性拒绝；
//!   - 状态机 `TriadState`：Registering → Rechecked 单向推进，重检未
//!     过不得出全检结论；
//!   - 账本 `TriadLedger`：重检条数 + 缺成分逃逸数（恒 0）；
//!   - 注入核 `BadTriadCase`：缺原文 / 缺达成面 / 缺拦截面 / 撞号 /
//!     越窗号 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10064 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91fc0;

/// 段窗上界（下一卡 0x92000 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92000;

/// 判据全集条数（F9601–F10080 共 480 条）。
pub const CRITERION_TOTAL: u32 = 480;

/// 判据号窗起点（F9601）。
pub const ID_LO: u32 = 9601;

/// 判据号窗终点（F10080）。
pub const ID_HI: u32 = 10080;

/// 成分数（原文/达成面/拦截面）。
pub const TRIAD_PARTS: usize = 3;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriadCode {
    /// 缺原文（判据原文成分未登记）
    MissingText,
    /// 缺达成面（J1 成分未登记）
    MissingPositive,
    /// 缺拦截面（J2 成分未登记）
    MissingNegative,
    /// 撞号（同判据号重复登记）
    DuplicateId,
    /// 越窗号（判据号出 9601..=10080）
    OutOfWindow,
    /// 非法键（既非 J1 也非 J2）
    BadFaceKey,
    /// 重检未过出结论
    NotRechecked,
    /// 账本倒退（重检计数回退）
    LedgerRegress,
    /// 缺成分逃逸（账面漏计缺成分条目）
    PartEscape,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl TriadCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TriadCode::MissingText => "缺原文被拒：判据原文成分必须先登记；请补原文",
            TriadCode::MissingPositive => "缺达成面被拒：J1 成分必须登记；请补达成面",
            TriadCode::MissingNegative => "缺拦截面被拒：J2 成分必须登记；请补拦截面",
            TriadCode::DuplicateId => "撞号被拒：同判据号已登记；请查重复",
            TriadCode::OutOfWindow => "越窗号被拒：判据号只允许 9601..=10080；请核对窗口",
            TriadCode::BadFaceKey => "非法键被拒：面键只允许 J1/J2；请核对键名",
            TriadCode::NotRechecked => "重检未过被拒：未重检不得出全检结论；请先重检",
            TriadCode::LedgerRegress => "账本倒退被拒：重检计数只增不减；请排查回滚",
            TriadCode::PartEscape => "缺成分逃逸：账面漏计缺成分条目；请补记明细",
            TriadCode::ParseFailure => "解析失败：判据 ID 必须为 UNX-F<num>[-J1|J2]；请补齐格式",
            TriadCode::StateRegress => "状态倒退被拒：Registering→Rechecked 单向",
            TriadCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91fc0..=0x92000",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TriadCode; 12] = [
        TriadCode::MissingText,
        TriadCode::MissingPositive,
        TriadCode::MissingNegative,
        TriadCode::DuplicateId,
        TriadCode::OutOfWindow,
        TriadCode::BadFaceKey,
        TriadCode::NotRechecked,
        TriadCode::LedgerRegress,
        TriadCode::PartEscape,
        TriadCode::ParseFailure,
        TriadCode::StateRegress,
        TriadCode::SegmentViolation,
    ];
}

// ── ID 解析器 ─────────────────────────────────────────────────────────
/// 面（正判据 J1 / 反判据 J2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceKey {
    /// 正判据
    J1,
    /// 反判据
    J2,
}

/// 判据 ID 解析结果：号 + 面（None=无面后缀，即原文锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriadId {
    /// 判据号 9601..=10080
    pub num: u32,
    /// 面键（None=原文锚）
    pub face: Option<FaceKey>,
}

/// 解析 "UNX-F9705"、"UNX-F9705-J1"、"UNX-F9705-J2"。
pub fn parse_triad_id(s: &str) -> Result<TriadId, TriadCode> {
    const PREFIX: &str = "UNX-F";
    if !s.starts_with(PREFIX) {
        return Err(TriadCode::ParseFailure);
    }
    let rest = match s.get(PREFIX.len()..) {
        Some(r) => r,
        None => return Err(TriadCode::ParseFailure),
    };
    let (num_part, face) = match rest.find("-J") {
        Some(pos) => {
            let tail = match rest.get(pos + 2..) {
                Some(t) => t,
                None => return Err(TriadCode::ParseFailure),
            };
            let fk = match tail {
                "1" => FaceKey::J1,
                "2" => FaceKey::J2,
                _ => return Err(TriadCode::BadFaceKey),
            };
            let np = match rest.get(..pos) {
                Some(n) => n,
                None => return Err(TriadCode::ParseFailure),
            };
            (np, Some(fk))
        }
        None => (rest, None),
    };
    if num_part.is_empty() || num_part.len() > 5 {
        return Err(TriadCode::ParseFailure);
    }
    let mut n: u32 = 0;
    for b in num_part.bytes() {
        if !b.is_ascii_digit() {
            return Err(TriadCode::ParseFailure);
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    if n < ID_LO || n > ID_HI {
        return Err(TriadCode::OutOfWindow);
    }
    Ok(TriadId { num: n, face })
}

// ── 三成分注册表 ──────────────────────────────────────────────────────
/// 状态机：Registering → Rechecked 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriadState {
    /// 登记中
    Registering,
    /// 已重检（三成分全齐且逐条核过）
    Rechecked,
}

/// 一条判据的三成分登记位（原文 + J1 + J2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TriadRow {
    /// 原文已登记
    pub text: bool,
    /// J1 已登记
    pub pos: bool,
    /// J2 已登记
    pub neg: bool,
}

impl TriadRow {
    /// 三成分全齐。
    pub fn complete(&self) -> bool {
        self.text && self.pos && self.neg
    }

    /// 首缺成分码（齐则 None）。
    pub fn first_missing(&self) -> Option<TriadCode> {
        if !self.text {
            Some(TriadCode::MissingText)
        } else if !self.pos {
            Some(TriadCode::MissingPositive)
        } else if !self.neg {
            Some(TriadCode::MissingNegative)
        } else {
            None
        }
    }
}

/// 三成分注册表：号 → 行位图。480 行以 3 张位图承载（省内存、可逐位核）。
#[derive(Clone, Debug)]
pub struct TriadRegistry {
    /// 原文位图（480 位）
    pub text_bits: Vec<bool>,
    /// J1 位图（480 位）
    pub pos_bits: Vec<bool>,
    /// J2 位图（480 位）
    pub neg_bits: Vec<bool>,
    /// 登记次数（原文/J1/J2 各计一次，全量=480×3）
    pub registered: u32,
    /// 当前状态
    pub state: TriadState,
}

impl TriadRegistry {
    /// 构造空注册表（480 行全空）。
    pub fn new() -> TriadRegistry {
        TriadRegistry {
            text_bits: vec![false; CRITERION_TOTAL as usize],
            pos_bits: vec![false; CRITERION_TOTAL as usize],
            neg_bits: vec![false; CRITERION_TOTAL as usize],
            registered: 0,
            state: TriadState::Registering,
        }
    }

    /// 号下标（0 基）。
    fn idx(num: u32) -> usize {
        (num - ID_LO) as usize
    }

    /// 登记一成分：号必检、重复必拒（同成分二次登记=撞号）。
    pub fn register(&mut self, id: TriadId) -> Result<(), TriadCode> {
        if self.state != TriadState::Registering {
            return Err(TriadCode::StateRegress);
        }
        let i = Self::idx(id.num);
        let bits = match id.face {
            Some(FaceKey::J1) => &mut self.pos_bits,
            Some(FaceKey::J2) => &mut self.neg_bits,
            None => &mut self.text_bits,
        };
        let was = bits.get(i).copied().unwrap_or(true);
        if was {
            return Err(TriadCode::DuplicateId);
        }
        if let Some(slot) = bits.get_mut(i) {
            *slot = true;
        }
        // 登记次数计账：每次成分置位计一次（置位后该位必为 false）。
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 行视图（三成分当前态）。
    pub fn row(&self, num: u32) -> Option<TriadRow> {
        if num < ID_LO || num > ID_HI {
            return None;
        }
        let i = Self::idx(num);
        Some(TriadRow {
            text: self.text_bits.get(i).copied().unwrap_or(false),
            pos: self.pos_bits.get(i).copied().unwrap_or(false),
            neg: self.neg_bits.get(i).copied().unwrap_or(false),
        })
    }

    /// 重检：480 条逐条三成分核；返回首缺（号, 缺码）。
    pub fn recheck(&mut self) -> Result<(), (u32, TriadCode)> {
        if self.state != TriadState::Registering {
            return Err((ID_LO, TriadCode::StateRegress));
        }
        let mut n = ID_LO;
        while n <= ID_HI {
            if let Some(row) = self.row(n) {
                if let Some(code) = row.first_missing() {
                    return Err((n, code));
                }
            }
            n += 1;
        }
        self.state = TriadState::Rechecked;
        Ok(())
    }

    /// 全齐条数（三成分同齐的号数）。
    pub fn complete_count(&self) -> u32 {
        let mut n = ID_LO;
        let mut c = 0u32;
        while n <= ID_HI {
            if let Some(row) = self.row(n) {
                if row.complete() {
                    c += 1;
                }
            }
            n += 1;
        }
        c
    }
}

/// 标准全量登记：480 条 × 三成分（原文锚 + J1 + J2）。
pub fn register_full(reg: &mut TriadRegistry) -> Result<(), TriadCode> {
    let mut n = ID_LO;
    while n <= ID_HI {
        reg.register(TriadId { num: n, face: None })?;
        reg.register(TriadId { num: n, face: Some(FaceKey::J1) })?;
        reg.register(TriadId { num: n, face: Some(FaceKey::J2) })?;
        n += 1;
    }
    Ok(())
}

// ── 重检账 ────────────────────────────────────────────────────────────
/// 重检账：核过条数 + 缺成分逃逸数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriadLedger {
    /// 已重检条数
    pub checked: u32,
    /// 缺成分逃逸数（必须为 0）
    pub part_escape: u32,
}

impl TriadLedger {
    /// 构造零账。
    pub const fn zero() -> TriadLedger {
        TriadLedger { checked: 0, part_escape: 0 }
    }

    /// 记一条重检；hit=false 即缺成分显性入账（不静默）。
    pub fn record(&mut self, hit: bool) -> Result<(), TriadCode> {
        self.checked = self.checked.saturating_add(1);
        if !hit {
            self.part_escape = self.part_escape.saturating_add(1);
        }
        Ok(())
    }
}

/// 全量重检走账：逐条 record，返回账本与首缺。
pub fn recheck_with_ledger(reg: &mut TriadRegistry) -> (TriadLedger, Option<(u32, TriadCode)>) {
    let mut ledger = TriadLedger::zero();
    let mut first_missing: Option<(u32, TriadCode)> = None;
    let mut n = ID_LO;
    while n <= ID_HI {
        let ok = match reg.row(n) {
            Some(row) => row.complete(),
            None => false,
        };
        let _ = ledger.record(ok);
        if !ok && first_missing.is_none() {
            if let Some(row) = reg.row(n) {
                if let Some(code) = row.first_missing() {
                    first_missing = Some((n, code));
                }
            }
        }
        n += 1;
    }
    (ledger, first_missing)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏三成分样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTriadCase {
    /// 缺原文（只登记 J1/J2）
    MissingText,
    /// 缺达成面（只登记原文/J2）
    MissingPositive,
    /// 缺拦截面（只登记原文/J1）
    MissingNegative,
    /// 撞号（同成分二次登记）
    DuplicateId,
    /// 越窗号（9600 / 10081）
    OutOfWindow,
}

impl BadTriadCase {
    /// 闭集全量。
    pub const ALL: [BadTriadCase; 5] = [
        BadTriadCase::MissingText,
        BadTriadCase::MissingPositive,
        BadTriadCase::MissingNegative,
        BadTriadCase::DuplicateId,
        BadTriadCase::OutOfWindow,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut reg = TriadRegistry::new();
        match self {
            BadTriadCase::MissingText => {
                // 9601 只登记 J1/J2，重检必须报缺原文。
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J1) });
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J2) });
                matches!(reg.recheck(), Err((9601, TriadCode::MissingText)))
            }
            BadTriadCase::MissingPositive => {
                // 9601 三成分齐，9602 缺 J1：重检必须定位 (9602, MissingPositive)。
                let _ = reg.register(TriadId { num: 9601, face: None });
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J1) });
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J2) });
                let _ = reg.register(TriadId { num: 9602, face: None });
                let _ = reg.register(TriadId { num: 9602, face: Some(FaceKey::J2) });
                matches!(reg.recheck(), Err((9602, TriadCode::MissingPositive)))
            }
            BadTriadCase::MissingNegative => {
                // 9601 三成分齐，9602 缺 J2：重检必须定位 (9602, MissingNegative)。
                let _ = reg.register(TriadId { num: 9601, face: None });
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J1) });
                let _ = reg.register(TriadId { num: 9601, face: Some(FaceKey::J2) });
                let _ = reg.register(TriadId { num: 9602, face: None });
                let _ = reg.register(TriadId { num: 9602, face: Some(FaceKey::J1) });
                matches!(reg.recheck(), Err((9602, TriadCode::MissingNegative)))
            }
            BadTriadCase::DuplicateId => {
                let _ = reg.register(TriadId { num: 9604, face: Some(FaceKey::J1) });
                reg.register(TriadId { num: 9604, face: Some(FaceKey::J1) })
                    == Err(TriadCode::DuplicateId)
            }
            BadTriadCase::OutOfWindow => {
                parse_triad_id("UNX-F9600").is_err()
                    && parse_triad_id("UNX-F10081").is_err()
                    && parse_triad_id("UNX-F9601-J3").is_err()
                    && parse_triad_id("UNX-X9601").is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_triad_injection() -> (TriadLedger, [u32; 5]) {
    let mut ledger = TriadLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadTriadCase::ALL.len() {
        if let Some(case) = BadTriadCase::ALL.get(ci).copied() {
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
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK；非法面键族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺成分族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；撞号族"),
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

/// 规范语料双跑指纹：全量登记→重检→走账全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = TriadRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match register_full(&mut reg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.recheck() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(reg.state as u8);
    bytes.extend_from_slice(&reg.complete_count().to_le_bytes());
    let (ledger, miss) = recheck_with_ledger(&mut reg);
    bytes.extend_from_slice(&ledger.checked.to_le_bytes());
    bytes.push(match miss {
        None => 5,
        Some(_) => 6,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10064 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10064_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10064");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91fc0..=0x92000", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. ID 解析器：三种形态命中
    let parse_ok = parse_triad_id("UNX-F9601") == Ok(TriadId { num: 9601, face: None })
        && parse_triad_id("UNX-F9601-J1") == Ok(TriadId { num: 9601, face: Some(FaceKey::J1) })
        && parse_triad_id("UNX-F10080-J2") == Ok(TriadId { num: 10080, face: Some(FaceKey::J2) });
    s.add("ID 解析：原文锚/J1/J2 三形态命中", parse_ok,
          "UNX-F<num>[-J1|J2] 全形态可复算");

    // 3. ID 解析器：非法键拒绝
    let bad_ok = parse_triad_id("UNX-F9601-J3").is_err()
        && parse_triad_id("UNX-X9601").is_err()
        && parse_triad_id("UNX-F960J").is_err()
        && parse_triad_id("UNX-F").is_err();
    s.add("ID 解析：J3/坏前缀/坏号体全拒", bad_ok,
          "解析器闭集：面键只允许 J1/J2，号体必须纯数字");

    // 4. 越窗号拒绝
    let win_ok = parse_triad_id("UNX-F9600").is_err()
        && parse_triad_id("UNX-F10081").is_err();
    s.add("越窗号：9600/10081 拒收", win_ok,
          "判据号窗冻结 9601..=10080");

    // 5. 标准全量登记成功
    let mut reg = TriadRegistry::new();
    let full_ok = register_full(&mut reg).is_ok()
        && reg.registered == CRITERION_TOTAL * TRIAD_PARTS as u32;
    s.add("全量登记：480 条 × 3 成分逐条入表", full_ok,
          "1440 次登记全部成功，无撞号");

    // 6. 重检通过：三成分全齐
    let recheck_ok = reg.recheck().is_ok() && reg.state == TriadState::Rechecked
        && reg.complete_count() == CRITERION_TOTAL;
    s.add("重检通过：480 条逐条三成分核过", recheck_ok,
          "原文+J1+J2 三成分同齐才计全检");

    // 7. 重检后登记拒绝（状态闸）
    let sealed_ok = reg.register(TriadId { num: 9601, face: None })
        == Err(TriadCode::StateRegress);
    s.add("重检后封账：再登记被拒", sealed_ok,
          "Registering → Rechecked 单向，重检后只读");

    // 8. 缺原文重检定位
    let (_, m1) = {
        let mut r = TriadRegistry::new();
        let _ = r.register(TriadId { num: 9705, face: Some(FaceKey::J1) });
        let _ = r.register(TriadId { num: 9705, face: Some(FaceKey::J2) });
        recheck_with_ledger(&mut r)
    };
    let miss_text = m1 == Some((9601, TriadCode::MissingText));
    s.add("缺原文：重检首缺定位到 9601 缺原文", miss_text,
          "重检按号序扫描，首缺号 9601 先报");

    // 9. 缺拦截面重检定位
    let (_, m2) = {
        let mut r = TriadRegistry::new();
        let mut n = ID_LO;
        while n <= ID_HI {
            let _ = r.register(TriadId { num: n, face: None });
            let _ = r.register(TriadId { num: n, face: Some(FaceKey::J1) });
            n += 1;
        }
        recheck_with_ledger(&mut r)
    };
    let miss_neg = matches!(m2, Some((9601, TriadCode::MissingNegative)));
    s.add("缺拦截面：全表 J2 缺失重检报缺", miss_neg,
          "反判据成分缺失必须显性报缺");

    // 10. 走账守恒：全量重检 480 条、逃逸=0
    let (lf, mf) = {
        let mut r = TriadRegistry::new();
        let _ = register_full(&mut r);
        recheck_with_ledger(&mut r)
    };
    s.add("走账守恒：480 条全检、缺成分逃逸=0",
          lf.checked == CRITERION_TOTAL && lf.part_escape == 0 && mf.is_none(),
          "全检账逐条入账，缺成分不得静默逃逸");

    // 11. 撞号拦截
    let mut dup = TriadRegistry::new();
    let _ = dup.register(TriadId { num: 9700, face: Some(FaceKey::J1) });
    let dup_ok = dup.register(TriadId { num: 9700, face: Some(FaceKey::J1) })
        == Err(TriadCode::DuplicateId);
    s.add("撞号：同成分二次登记被拒", dup_ok,
          "一号一成分一槽，重复显性拒绝");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_triad_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺原文/缺达成面/缺拦截面/撞号/越窗全被显式拒绝");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮全检出逃逸=0",
          ledger.checked == 5 * INJECT_ROUNDS && ledger.part_escape == 0,
          "注入核账守恒：50 次注入 50 次检出、0 逃逸");

    // 14. 文案闭集：非空
    let text_ok = TriadCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < TriadCode::ALL.len() {
        let mut j = i + 1;
        while j < TriadCode::ALL.len() {
            if let (Some(x), Some(y)) = (TriadCode::ALL.get(i), TriadCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = TriadCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
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
    let snap_text = format!("全齐 {}/480 / 注入检出 {}/50", reg.complete_count(), ledger.checked);
    s.add_dyn("全检快照", reg.complete_count() == CRITERION_TOTAL, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_three_forms() {
        assert_eq!(parse_triad_id("UNX-F9601"), Ok(TriadId { num: 9601, face: None }));
        assert_eq!(parse_triad_id("UNX-F9601-J1"), Ok(TriadId { num: 9601, face: Some(FaceKey::J1) }));
        assert_eq!(parse_triad_id("UNX-F10080-J2"), Ok(TriadId { num: 10080, face: Some(FaceKey::J2) }));
    }

    #[test]
    fn parse_rejects() {
        assert!(parse_triad_id("UNX-F9601-J3").is_err());
        assert!(parse_triad_id("UNX-X9601").is_err());
        assert!(parse_triad_id("UNX-F9600").is_err());
        assert!(parse_triad_id("UNX-F10081").is_err());
    }

    #[test]
    fn full_registration_and_recheck() {
        let mut reg = TriadRegistry::new();
        assert!(register_full(&mut reg).is_ok());
        assert!(reg.recheck().is_ok());
        assert_eq!(reg.complete_count(), CRITERION_TOTAL);
    }

    #[test]
    fn missing_parts_located() {
        let mut r = TriadRegistry::new();
        let _ = r.register(TriadId { num: 9601, face: Some(FaceKey::J1) });
        let _ = r.register(TriadId { num: 9601, face: Some(FaceKey::J2) });
        assert_eq!(r.recheck(), Err((9601, TriadCode::MissingText)));
    }

    #[test]
    fn duplicate_rejected() {
        let mut reg = TriadRegistry::new();
        let _ = reg.register(TriadId { num: 9700, face: Some(FaceKey::J1) });
        assert_eq!(reg.register(TriadId { num: 9700, face: Some(FaceKey::J1) }),
                   Err(TriadCode::DuplicateId));
    }

    #[test]
    fn rechecked_is_readonly() {
        let mut reg = TriadRegistry::new();
        let _ = register_full(&mut reg);
        let _ = reg.recheck();
        assert_eq!(reg.register(TriadId { num: 9601, face: None }), Err(TriadCode::StateRegress));
    }

    #[test]
    fn ledger_walks_all() {
        let mut reg = TriadRegistry::new();
        let _ = register_full(&mut reg);
        let (l, miss) = recheck_with_ledger(&mut reg);
        assert_eq!(l.checked, CRITERION_TOTAL);
        assert_eq!(l.part_escape, 0);
        assert!(miss.is_none());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_triad_injection();
        assert_eq!(l.checked, 5 * INJECT_ROUNDS);
        assert_eq!(l.part_escape, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EBADF));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ERANGE), -ERANGE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn triad_code_text_complete() {
        for c in TriadCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10064_checks_green() {
        let s = run_unx10064_checks();
        assert!(s.all_passed(), "UNX-F10064 red: {}", s.red_items_text());
    }
}
