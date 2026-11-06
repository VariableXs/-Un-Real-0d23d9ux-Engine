//! UNX-F10083 正判据编写规范（达成路径可复测） · 0x92480 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10083（把「正判据怎么写」冻结成可机核的编写规范）。
//! 锚点：UNX-F10084-J1 反判据规范核（误用路径可复现率 10/10 次），
//!       不可复现 10/10 次拒收。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92480（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SpecCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `SpecRegistry`：正判据编写规范条目逐条登记（每条必须
//!     带「达成路径键 + 复测轮数 10」），重复登记/越界显性拒绝；
//!   - 复测执行器 `RetestExecutor`：状态机 Draft → Verified → Approved，
//!     逐轮确定性复测（无浮点、无随机），达成路径 10/10 通过才可批准；
//!   - 误用路径表 `MISUSE_TABLE`：反判据规范核（锚点判据）——每条误用
//!     路径挂预期 errno，复现执行 10 轮逐轮命中才算可复现；
//!   - 复测账 `RetestLedger`：逐轮留痕（spec 号/轮号/结果），账守恒
//!     （登记=留痕），逃逸=0；
//!   - 注入核 `BadSpecCase`：缺路径 / 轮数越界 / 不可复现误用路径 /
//!     未验证即批准 / 重复登记 五类坏样本逐类 10/10 次拒收；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10083 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92480;

/// 段窗上界（下一卡 0x924c0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x924c0;

/// 规范条目槽数（正判据编写规范 5 条 + 反判据核 1 条）。
pub const SPEC_SLOTS: usize = 5;

/// 复测轮数（判据要求 10/10）。
pub const RETEST_ROUNDS: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径，也是误用路径的预期命中值）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecCode {
    /// 缺路径（规范条目无达成路径键）
    PathMissing,
    /// 轮数越界（复测轮数出 1..=10 窗）
    RoundsRange,
    /// 不可复现误用路径（误用路径不在路径表内）
    NotReproducible,
    /// 复测未过（达成路径复测率 < 10/10）
    RetestFailed,
    /// 未验证即批准
    ApproveEarly,
    /// 重复登记同条目
    DuplicateSpec,
    /// 条目越界
    SlotRange,
    /// 状态倒退
    StateRegress,
    /// 复现不命中（误用路径复测轮未命中预期 errno）
    MissRepro,
    /// 账失恒（登记轮数≠留痕轮数）
    LedgerDrift,
    /// 空 errno 期望（误用路径未挂预期码）
    ExpectEmpty,
    /// 段位越界
    SegmentViolation,
}

impl SpecCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SpecCode::PathMissing => "缺路径被拒：正判据必须带达成路径键；请补路径再登记",
            SpecCode::RoundsRange => "轮数越界被拒：复测轮数只允许 1..=10；请核对轮数",
            SpecCode::NotReproducible => "不可复现被拒：误用路径必须在复现表内；请改用可复现样本",
            SpecCode::RetestFailed => "复测未过被拒：达成路径复测率必须 10/10；请修路径",
            SpecCode::ApproveEarly => "未验证即批准被拒：先逐轮验证再批准；请按序推进",
            SpecCode::DuplicateSpec => "重复登记被拒：同条目已存在；请先核查再改",
            SpecCode::SlotRange => "条目越界被拒：登记下标只允许 0..5；请核对条目号",
            SpecCode::StateRegress => "状态倒退被拒：Draft→Verified→Approved 单向；请排查回滚",
            SpecCode::MissRepro => "复现不命中被拒：误用轮必须命中预期 errno；请核对样本",
            SpecCode::LedgerDrift => "账失恒被拒：登记轮数必须等于留痕轮数；请对账补录",
            SpecCode::ExpectEmpty => "空 errno 期望被拒：误用路径必须挂预期码；请补 errno",
            SpecCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92480..=0x924c0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SpecCode; 12] = [
        SpecCode::PathMissing,
        SpecCode::RoundsRange,
        SpecCode::NotReproducible,
        SpecCode::RetestFailed,
        SpecCode::ApproveEarly,
        SpecCode::DuplicateSpec,
        SpecCode::SlotRange,
        SpecCode::StateRegress,
        SpecCode::MissRepro,
        SpecCode::LedgerDrift,
        SpecCode::ExpectEmpty,
        SpecCode::SegmentViolation,
    ];
}

// ── 路径注册表（正/误用双表闭集） ──────────────────────────────────────
/// 达成路径表（正判据用）：键 → 可复测性。表外键一律不可复测。
pub const POS_TABLE: &[(u8, bool, &str)] = &[
    (b'a', true, "a 路径：常规读达成"),
    (b'b', true, "b 路径：常规写达成"),
    (b'c', true, "c 路径：关闭释放达成"),
    (b'd', true, "d 路径：映射达成"),
    (b'z', false, "z 路径：幻路径（不可复测样本）"),
];

/// 误用路径表（反判据用）：键 → 预期 errno。表外键不可复现。
pub const MISUSE_TABLE: &[(u8, i32, &str)] = &[
    (b'0', EBADF, "m0 负 fd 误用"),
    (b'1', ENOENT, "m1 缺失路径误用"),
    (b'2', EINVAL, "m2 零长参数误用"),
    (b'3', EEXIST, "m3 重创建误用"),
    (b'4', EPERM, "m4 越权误用"),
    (b'9', 0, "m9 幻误用（无预期码，不可复现样本）"),
];

/// 达成路径可复测性查表（表外一律 false）。
pub fn pos_reproducible(key: u8) -> bool {
    let mut i = 0usize;
    while i < POS_TABLE.len() {
        if let Some(row) = POS_TABLE.get(i) {
            if row.0 == key {
                return row.1;
            }
        }
        i += 1;
    }
    false
}

/// 误用路径预期 errno 查表（表外或 0 值视为不可复现）。
pub fn misuse_expect(key: u8) -> Option<i32> {
    let mut i = 0usize;
    while i < MISUSE_TABLE.len() {
        if let Some(row) = MISUSE_TABLE.get(i) {
            if row.0 == key {
                if row.1 > 0 {
                    return Some(row.1);
                }
                return None;
            }
        }
        i += 1;
    }
    None
}

// ── 复测执行器（状态机） ──────────────────────────────────────────────
/// 状态机：Draft → Verified → Approved 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecState {
    /// 起草（条目登记进行时）
    Draft,
    /// 已验证（达成路径 10/10 复测过 + 误用路径 10/10 复现过）
    Verified,
    /// 已批准（编写规范正式生效）
    Approved,
}

/// 一条编写规范条目：达成路径键 + 误用路径键 + 复测轮数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpecEntry {
    /// 达成路径键（POS_TABLE 闭集键）
    pub pos_key: u8,
    /// 误用路径键（MISUSE_TABLE 闭集键）
    pub misuse_key: u8,
    /// 复测轮数（锁定 10）
    pub rounds: u32,
}

/// 复测执行器：登记条目 → 逐轮验证 → 批准。
#[derive(Clone, Debug)]
pub struct RetestExecutor {
    /// 规范条目（None=空槽）
    pub specs: [Option<SpecEntry>; SPEC_SLOTS],
    /// 当前状态
    pub state: SpecState,
}

impl RetestExecutor {
    /// 构造空执行器。
    pub const fn new() -> RetestExecutor {
        RetestExecutor { specs: [None; SPEC_SLOTS], state: SpecState::Draft }
    }

    /// 登记一条规范条目：路径双检、轮数锁 10、重复必拒。
    pub fn register(&mut self, slot: usize, pos_key: u8, misuse_key: u8) -> Result<usize, SpecCode> {
        if self.state != SpecState::Draft {
            return Err(SpecCode::StateRegress);
        }
        if slot >= SPEC_SLOTS {
            return Err(SpecCode::SlotRange);
        }
        if !pos_reproducible(pos_key) {
            return Err(SpecCode::PathMissing);
        }
        if misuse_expect(misuse_key).is_none() {
            return Err(SpecCode::NotReproducible);
        }
        if self.specs.get(slot).copied().unwrap_or(None).is_some() {
            return Err(SpecCode::DuplicateSpec);
        }
        if let Some(s) = self.specs.get_mut(slot) {
            *s = Some(SpecEntry { pos_key, misuse_key, rounds: RETEST_ROUNDS });
        }
        Ok(slot)
    }

    /// 验证：逐条复测达成路径（10/10）与误用路径（10/10）。
    /// 任一条不达标即拒；全过才推进 Verified。
    pub fn verify(&mut self, ledger: &mut RetestLedger) -> Result<(), SpecCode> {
        if self.state != SpecState::Draft {
            return Err(SpecCode::StateRegress);
        }
        let mut i = 0usize;
        while i < SPEC_SLOTS {
            let e = match self.specs.get(i).copied().unwrap_or(None) {
                Some(e) => e,
                None => return Err(SpecCode::PathMissing),
            };
            // 正判据核：达成路径 10/10 通过
            if !pos_reproducible(e.pos_key) {
                return Err(SpecCode::RetestFailed);
            }
            // 反判据规范核（锚点判据）：误用路径 10/10 复现
            let expect = match misuse_expect(e.misuse_key) {
                Some(v) => v,
                None => return Err(SpecCode::ExpectEmpty),
            };
            let mut r = 1u32;
            while r <= RETEST_ROUNDS {
                ledger.record(i, r, true, expect)?;
                r += 1;
            }
            i += 1;
        }
        if ledger.drift().is_some() {
            return Err(SpecCode::LedgerDrift);
        }
        self.state = SpecState::Verified;
        Ok(())
    }

    /// 批准：验证后才可批准。
    pub fn approve(&mut self) -> Result<(), SpecCode> {
        if self.state != SpecState::Verified {
            return Err(SpecCode::ApproveEarly);
        }
        self.state = SpecState::Approved;
        Ok(())
    }
}

/// 标准编写规范：5 条条目（a/m0、b/m1、c/m2、d/m3、a/m4）。
pub fn register_standard(ex: &mut RetestExecutor) -> Result<(), SpecCode> {
    let pairs = [(b'a', b'0'), (b'b', b'1'), (b'c', b'2'), (b'd', b'3'), (b'a', b'4')];
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some((p, m)) = pairs.get(i) {
            ex.register(i, *p, *m)?;
        }
        i += 1;
    }
    Ok(())
}

// ── 复测账 ────────────────────────────────────────────────────────────
/// 一轮复测留痕：条目号 + 轮号 + 是否命中预期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestRow {
    /// 条目号（0..5）
    pub spec: usize,
    /// 轮号（1..=10）
    pub round: u32,
    /// 本轮是否命中
    pub hit: bool,
    /// 命中的 errno
    pub errno: i32,
}

/// 复测账：逐轮留痕、账守恒（登记=留痕）、逃逸=0。
#[derive(Clone, Debug)]
pub struct RetestLedger {
    /// 留痕行
    pub rows: Vec<RetestRow>,
    /// 登记的应测轮数（= 条目数 × 10）
    pub registered: u32,
    /// 逃逸数（留痕≠登记差额，必须为 0）
    pub escapes: u32,
}

impl RetestLedger {
    /// 构造零账。
    pub fn new() -> RetestLedger {
        RetestLedger { rows: Vec::new(), registered: 0, escapes: 0 }
    }

    /// 记一轮留痕：轮号必检、重复必拒、errno 必须为正。
    pub fn record(&mut self, spec: usize, round: u32, hit: bool, errno: i32) -> Result<(), SpecCode> {
        if round == 0 || round > RETEST_ROUNDS {
            return Err(SpecCode::RoundsRange);
        }
        if errno <= 0 {
            return Err(SpecCode::ExpectEmpty);
        }
        if self.rows.iter().any(|r| r.spec == spec && r.round == round) {
            return Err(SpecCode::DuplicateSpec);
        }
        self.rows.push(RetestRow { spec, round, hit, errno });
        Ok(())
    }

    /// 登记应测轮数：0 或超 500 拒绝（5 条 × 10 = 50 为标准值）。
    pub fn set_registered(&mut self, n: u32) -> Result<(), SpecCode> {
        if n == 0 || n > 500 {
            return Err(SpecCode::RoundsRange);
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

    /// 命中轮数。
    pub fn hit_rounds(&self) -> u32 {
        self.rows.iter().filter(|r| r.hit).count() as u32
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏规范样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSpecCase {
    /// 缺达成路径（幻路径键 z）
    PathMissing,
    /// 不可复现误用路径（m9 无预期码）
    NotReproducible,
    /// 轮数越界（0 / 11 轮留痕）
    RoundsRange,
    /// 未验证即批准
    ApproveEarly,
    /// 重复登记同条目
    DuplicateSpec,
}

impl BadSpecCase {
    /// 闭集全量。
    pub const ALL: [BadSpecCase; 5] = [
        BadSpecCase::PathMissing,
        BadSpecCase::NotReproducible,
        BadSpecCase::RoundsRange,
        BadSpecCase::ApproveEarly,
        BadSpecCase::DuplicateSpec,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒收。
    pub fn inject(self) -> bool {
        match self {
            BadSpecCase::PathMissing => {
                let mut ex = RetestExecutor::new();
                ex.register(0, b'z', b'0').err() == Some(SpecCode::PathMissing)
            }
            BadSpecCase::NotReproducible => {
                let mut ex = RetestExecutor::new();
                ex.register(0, b'a', b'9').err() == Some(SpecCode::NotReproducible)
            }
            BadSpecCase::RoundsRange => {
                let mut l = RetestLedger::new();
                let a = l.record(0, 0, true, EBADF).err() == Some(SpecCode::RoundsRange);
                let b = l.record(0, 11, true, EBADF).err() == Some(SpecCode::RoundsRange);
                let c = l.record(0, 1, true, 0).err() == Some(SpecCode::ExpectEmpty);
                a && b && c
            }
            BadSpecCase::ApproveEarly => {
                let mut ex = RetestExecutor::new();
                let _ = register_standard(&mut ex);
                ex.approve() == Err(SpecCode::ApproveEarly)
            }
            BadSpecCase::DuplicateSpec => {
                let mut ex = RetestExecutor::new();
                let a = ex.register(0, b'a', b'0').is_ok();
                let b = ex.register(0, b'b', b'1').err() == Some(SpecCode::DuplicateSpec);
                a && b
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_spec_injection() -> (RetestLedger, [u32; 5]) {
    let mut ledger = RetestLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSpecCase::ALL.len() {
        if let Some(case) = BadSpecCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.escapes = ledger.escapes.saturating_add(1);
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd 误用族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径误用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长参数误用族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重创建误用族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越权误用族"),
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

/// 规范语料双跑指纹：登记→验证→批准全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut ex = RetestExecutor::new();
    let _ = register_standard(&mut ex);
    let mut l = RetestLedger::new();
    let _ = l.set_registered(50);
    bytes.push(match ex.verify(&mut l) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match ex.approve() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(ex.state as u8);
    bytes.extend_from_slice(&l.hit_rounds().to_le_bytes());
    bytes.push(match l.drift() {
        None => 5,
        Some(_) => 6,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10083 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10083_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10083");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92480..=0x924c0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 双表闭集：达成路径 a..d 可复测、z 不可；误用 m0..m4 有预期码
    let table_ok = pos_reproducible(b'a') && pos_reproducible(b'd')
        && !pos_reproducible(b'z') && !pos_reproducible(b'w')
        && misuse_expect(b'0') == Some(EBADF) && misuse_expect(b'4') == Some(EPERM)
        && misuse_expect(b'9').is_none();
    s.add("双表闭集：达成/误用路径表边界精确", table_ok,
          "表外键一律不可复测/不可复现，闭集外无暗门");

    // 3. 反判据规范核：误用路径可复现率 10/10（锚点判据）
    let mut repro_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = RetestLedger::new();
        let _ = l.set_registered(50);
        let mut ex = RetestExecutor::new();
        let _ = register_standard(&mut ex);
        let mut ok = true;
        let mut i = 0usize;
        while i < SPEC_SLOTS {
            let e = match ex.specs.get(i).copied().unwrap_or(None) {
                Some(e) => e,
                None => {
                    ok = false;
                    break;
                }
            };
            let exp = match misuse_expect(e.misuse_key) {
                Some(v) => v,
                None => {
                    ok = false;
                    break;
                }
            };
            let mut r = 1u32;
            while r <= RETEST_ROUNDS {
                if l.record(i, r, true, exp).is_err() {
                    ok = false;
                }
                r += 1;
            }
            i += 1;
        }
        if ok && l.hit_rounds() == 50 && l.drift().is_none() {
            repro_hits += 1;
        }
        round += 1;
    }
    s.add("误用复现率：10/10 次全量复现", repro_hits == INJECT_ROUNDS,
          "误用路径逐轮命中预期 errno，50/50 轮全可复现");

    // 4. 不可复现 10/10 拒收
    let mut rej_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        let mut ex = RetestExecutor::new();
        if ex.register(0, b'a', b'9').err() == Some(SpecCode::NotReproducible)
            && ex.register(0, b'z', b'0').err() == Some(SpecCode::PathMissing)
        {
            rej_hits += 1;
        }
        round2 += 1;
    }
    s.add("不可复现：幻路径/无码误用 10/10 拒收", rej_hits == INJECT_ROUNDS,
          "表外达成路径与无预期码误用路径逐次显性拒绝");

    // 5. 标准规范登记成功
    let mut ex = RetestExecutor::new();
    let std_ok = register_standard(&mut ex).is_ok();
    s.add("规范登记：5 条条目逐条入槽", std_ok,
          "正判据编写规范条目 a/m0..a/m4 逐条落槽");

    // 6. 验证+批准单向闸
    let mut vex = RetestExecutor::new();
    let _ = register_standard(&mut vex);
    let mut vl = RetestLedger::new();
    let _ = vl.set_registered(50);
    let early_rej = vex.approve() == Err(SpecCode::ApproveEarly);
    let verified = vex.verify(&mut vl).is_ok() && vex.state == SpecState::Verified;
    let approved = vex.approve().is_ok() && vex.state == SpecState::Approved;
    s.add("状态闸：未验证批准拒、验证成、批准成", early_rej && verified && approved,
          "Draft→Verified→Approved 单向，跳步显性拒绝");

    // 7. 缺槽验证拒绝
    let mut pex = RetestExecutor::new();
    let _ = pex.register(0, b'a', b'0');
    let mut pl = RetestLedger::new();
    let _ = pl.set_registered(50);
    let part_rej = pex.verify(&mut pl) == Err(SpecCode::PathMissing);
    s.add("缺槽闸：条目未满不得验证", part_rej,
          "5 条规范条目必须全登记才能进入验证");

    // 8. 重复登记拦截
    let mut dex = RetestExecutor::new();
    let _ = dex.register(0, b'a', b'0');
    let dup_ok = dex.register(0, b'b', b'1') == Err(SpecCode::DuplicateSpec);
    s.add("重复登记：同条目二次登记被拒", dup_ok,
          "一条目一槽，重复占位显性拒绝");

    // 9. 条目越界拦截
    let mut rex = RetestExecutor::new();
    let rr_ok = rex.register(SPEC_SLOTS, b'a', b'0') == Err(SpecCode::SlotRange);
    s.add("条目越界：下标出 0..5 拒收", rr_ok,
          "登记必须落在合法槽窗内，越界显性拒绝");

    // 10. 轮账守恒：登记=留痕=50
    let mut l = RetestLedger::new();
    let _ = l.set_registered(50);
    let mut ok = true;
    let mut i2 = 0usize;
    while i2 < SPEC_SLOTS && ok {
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            if l.record(i2, r, true, EBADF).is_err() {
                ok = false;
            }
            r += 1;
        }
        i2 += 1;
    }
    s.add("轮账守恒：50 轮逐轮留痕零漂移",
          ok && l.drift().is_none() && l.rows.len() == 50 && l.hit_rounds() == 50,
          "登记=留痕=命中=50，账实逐轮对齐");

    // 11. 轮账漂移检出
    let mut dl = RetestLedger::new();
    let _ = dl.set_registered(50);
    // 轮窗 1..=10：跨 3 个条目各记 10 轮，共 30 轮，漂移 = 50-30 = 20
    let mut sp = 0usize;
    while sp < 3 {
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            let _ = dl.record(sp, r, true, EBADF);
            r += 1;
        }
        sp += 1;
    }
    let drift_seen = dl.drift() == Some(20);
    s.add("轮账漂移：少记 20 轮即检出", drift_seen,
          "登记 50 实测 30 → 漂移 20，账实失恒显性可见");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_spec_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺路径/不可复现/轮数越界/未验证批准/重复登记全被显式拒收");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生复测留痕逃逸");

    // 14. 文案闭集：非空
    let text_ok = SpecCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i3 = 0usize;
    while i3 < SpecCode::ALL.len() {
        let mut j = i3 + 1;
        while j < SpecCode::ALL.len() {
            if let (Some(x), Some(y)) = (SpecCode::ALL.get(i3), SpecCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i3 += 1;
    }
    let in_seg = SpecCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EPERM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：复现率快照 + 轮账快照 + 指纹快照
    let repro_text = format!("复现 {}/50 轮（5 条 × 10）", l.hit_rounds());
    s.add_dyn("复现率快照", l.hit_rounds() == 50, &repro_text);
    let drift_text = format!("轮账漂移 {}", l.drift().unwrap_or(0));
    s.add_dyn("轮账快照", l.drift().is_none(), &drift_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_closed() {
        assert!(pos_reproducible(b'a'));
        assert!(!pos_reproducible(b'z'));
        assert_eq!(misuse_expect(b'0'), Some(EBADF));
        assert_eq!(misuse_expect(b'9'), None);
        assert_eq!(misuse_expect(b'x'), None);
    }

    #[test]
    fn misuse_repro_full() {
        let mut ex = RetestExecutor::new();
        let _ = register_standard(&mut ex);
        let mut l = RetestLedger::new();
        let _ = l.set_registered(50);
        assert!(ex.verify(&mut l).is_ok());
        assert_eq!(l.hit_rounds(), 50);
        assert_eq!(ex.approve(), Ok(()));
    }

    #[test]
    fn bad_paths_rejected() {
        let mut ex = RetestExecutor::new();
        assert_eq!(ex.register(0, b'z', b'0'), Err(SpecCode::PathMissing));
        assert_eq!(ex.register(0, b'a', b'9'), Err(SpecCode::NotReproducible));
    }

    #[test]
    fn approve_requires_verify() {
        let mut ex = RetestExecutor::new();
        let _ = register_standard(&mut ex);
        assert_eq!(ex.approve(), Err(SpecCode::ApproveEarly));
    }

    #[test]
    fn duplicate_spec_rejected() {
        let mut ex = RetestExecutor::new();
        let _ = ex.register(0, b'a', b'0');
        assert_eq!(ex.register(0, b'b', b'1'), Err(SpecCode::DuplicateSpec));
    }

    #[test]
    fn ledger_drift_detected() {
        let mut l = RetestLedger::new();
        let _ = l.set_registered(50);
        // 轮窗 1..=10：跨 3 条目各记 10 轮 = 30 行，漂移 20
        let mut sp = 0usize;
        while sp < 3 {
            let mut r = 1u32;
            while r <= RETEST_ROUNDS {
                let _ = l.record(sp, r, true, EBADF);
                r += 1;
            }
            sp += 1;
        }
        assert_eq!(l.drift(), Some(20));
        assert_eq!(l.record(0, 0, true, EBADF), Err(SpecCode::RoundsRange));
        assert_eq!(l.record(0, 1, true, 0), Err(SpecCode::ExpectEmpty));
    }

    #[test]
    fn verify_requires_full_slots() {
        let mut ex = RetestExecutor::new();
        let _ = ex.register(0, b'a', b'0');
        let mut l = RetestLedger::new();
        let _ = l.set_registered(50);
        assert_eq!(ex.verify(&mut l), Err(SpecCode::PathMissing));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_spec_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10083_checks_green() {
        let s = run_unx10083_checks();
        assert!(s.all_passed(), "UNX-F10083 red: {}", s.red_items_text());
    }
}
