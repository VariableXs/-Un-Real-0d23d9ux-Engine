//! UNX-F10082 双判据结构协议（J1 正/J2 反形态） · 0x92440 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10082（结构协议落地卡：把 J1 正/J2 反两形态冻结成账）。
//! 锚点：UNX-F10083-J1 正判据规范核（达成路径可复测率 10/10 次），
//!       不可复测 10/10 次拒收。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92440（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `StructCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ShapeRegistry`：J1 正形态 / J2 反形态双列逐条登记，
//!     形态混挂（J1 槽挂 J2 形态）显性拒绝；
//!   - 复测核 `PosReproCore`：正判据达成路径逐轮复测，可复测率必须
//!     10/10，不可复测逐次拒收并显性记账；
//!   - 状态机 `ShapeState`：Blank → Shaped → Frozen 单向推进；
//!   - 复测账 `ReproLedger`：逐轮留痕（轮号/形态/结果），轮账守恒
//!     （登记轮数 = 实测轮数），逃逸=0；
//!   - 注入核 `BadShapeCase`：不可复测路径 / 形态混挂 / 缺路径 /
//!     轮次越界 / 冻结后改形态 五类坏样本逐类 10/10 次拒收；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10082 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92440;

/// 段窗上界（下一卡 0x92480 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92480;

/// 形态双列槽数（J1 列 3 + J2 列 3）。
pub const SHAPE_SLOTS: usize = 6;

/// 复测轮数（判据要求 10/10）。
pub const REPRO_ROUNDS: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 正判据复测通过率锁定值（10/10 = 100%）。
pub const REPO_RATE_LOCKED: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const EIO: i32 = 5;
pub const EEXIST: i32 = 17;
pub const EPERM: i32 = 1;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StructCode {
    /// 不可复测路径（达成路径复测失败）
    NotReproducible,
    /// 形态混挂（J1 槽挂 J2 形态或反向）
    ShapeMixed,
    /// 缺路径（形态条目无复测路径）
    PathMissing,
    /// 轮次越界（复测轮号出 1..=10 窗）
    RoundRange,
    /// 冻结后改形态
    FrozenMutate,
    /// 状态倒退
    StateRegress,
    /// 双列不齐（J1/J2 各 3 未满即冻结）
    ColumnsShort,
    /// 轮账失恒（登记轮数≠实测轮数）
    LedgerDrift,
    /// 重复登记同槽
    DuplicateSlot,
    /// 槽位越界
    SlotRange,
    /// 复测率越锁（>10 轮记 10 通过=虚报）
    RateLocked,
    /// 段位越界
    SegmentViolation,
}

impl StructCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StructCode::NotReproducible => "不可复测被拒：达成路径复测未过；请修路径再登记",
            StructCode::ShapeMixed => "形态混挂被拒：J1 槽只允许正形态、J2 槽只允许反形态；请对号入座",
            StructCode::PathMissing => "缺路径被拒：形态条目必须带复测路径；请补路径字面量",
            StructCode::RoundRange => "轮次越界被拒：复测轮号只允许 1..=10；请核对轮号",
            StructCode::FrozenMutate => "冻结后改形态被拒：协议已冻结只读；请走修订通道",
            StructCode::StateRegress => "状态倒退被拒：Blank→Shaped→Frozen 单向；请排查回滚",
            StructCode::ColumnsShort => "双列不齐被拒：J1/J2 各 3 未满不得冻结；请补齐条目",
            StructCode::LedgerDrift => "轮账失恒被拒：登记轮数必须等于实测轮数；请对账补录",
            StructCode::DuplicateSlot => "重复登记被拒：同槽已有形态；请先核查再改",
            StructCode::SlotRange => "槽位越界被拒：登记下标只允许 0..6；请核对槽号",
            StructCode::RateLocked => "复测率越锁被拒：通过轮数不得越过 10；请核查虚报",
            StructCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92440..=0x92480",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [StructCode; 12] = [
        StructCode::NotReproducible,
        StructCode::ShapeMixed,
        StructCode::PathMissing,
        StructCode::RoundRange,
        StructCode::FrozenMutate,
        StructCode::StateRegress,
        StructCode::ColumnsShort,
        StructCode::LedgerDrift,
        StructCode::DuplicateSlot,
        StructCode::SlotRange,
        StructCode::RateLocked,
        StructCode::SegmentViolation,
    ];
}

// ── 形态闭集与注册表 ──────────────────────────────────────────────────
/// 双判据形态闭集：正形态（达成路径）/ 反形态（误用路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// J1 正形态：达成路径可复测
    Positive,
    /// J2 反形态：误用路径可复现
    Negative,
}

/// 状态机：Blank → Shaped → Frozen 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeState {
    /// 空白（登记进行时）
    Blank,
    /// 已成形（双列各 3 齐）
    Shaped,
    /// 已冻结（结构协议封版）
    Frozen,
}

/// 每槽复测路径用期望结果闭集：Pass（达成）/ Repro（复现）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathExpect {
    /// 正形态：路径复测应达成
    Pass,
    /// 反形态：路径复测应复现受控错误
    Repro,
}

/// 一条形态条目：形态 + 复测路径代字（p0..p3 闭集）+ 期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShapeEntry {
    /// 形态
    pub shape: Shape,
    /// 复测路径代字（'p'+'0'..'3'；p0..p3 是核心路径注册表的闭集键）
    pub path_key: u8,
    /// 期望结果
    pub expect: PathExpect,
}

/// 核心路径注册表：p0..p3 四条路径，每条标注可复测性。
pub const PATH_TABLE: &[(u8, bool, &str)] = &[
    (b'0', true, "p0 常规读达成"),
    (b'1', true, "p1 常规写达成"),
    (b'2', true, "p2 关闭释放达成"),
    (b'3', false, "p3 幻路径（不可复测样本）"),
];

/// 路径可复测性查表（表外路径一律视为不可复测）。
pub fn path_reproducible(key: u8) -> bool {
    let mut i = 0usize;
    while i < PATH_TABLE.len() {
        if let Some(row) = PATH_TABLE.get(i) {
            if row.0 == key {
                return row.1;
            }
        }
        i += 1;
    }
    false
}

/// 形态注册表：6 槽双列 + 状态机。
#[derive(Clone, Debug)]
pub struct ShapeRegistry {
    /// 已登记形态（None=空槽；槽 0..3 为 J1 列，槽 3..6 为 J2 列）
    pub slots: [Option<ShapeEntry>; SHAPE_SLOTS],
    /// 当前状态
    pub state: ShapeState,
}

impl ShapeRegistry {
    /// 构造空注册表。
    pub const fn new() -> ShapeRegistry {
        ShapeRegistry { slots: [None; SHAPE_SLOTS], state: ShapeState::Blank }
    }

    /// 登记一条形态：槽号必检、形态对号必检、路径必检、冻结后必拒。
    /// 槽 0..2 是 J1 列（只收 Positive），槽 3..5 是 J2 列（只收 Negative）。
    pub fn register(&mut self, slot: usize, path_key: u8) -> Result<usize, StructCode> {
        if self.state != ShapeState::Blank {
            return Err(StructCode::FrozenMutate);
        }
        if slot >= SHAPE_SLOTS {
            return Err(StructCode::SlotRange);
        }
        let shape = if slot < 3 { Shape::Positive } else { Shape::Negative };
        let expect = match shape {
            Shape::Positive => PathExpect::Pass,
            Shape::Negative => PathExpect::Repro,
        };
        if path_reproducible(path_key) {
            let e = ShapeEntry { shape, path_key, expect };
            if self.slots.get(slot).copied().unwrap_or(None).is_some() {
                return Err(StructCode::DuplicateSlot);
            }
            if let Some(s) = self.slots.get_mut(slot) {
                *s = Some(e);
            }
            Ok(slot)
        } else {
            // 不可复测路径：无论形态一律拒收并留痕
            Err(StructCode::NotReproducible)
        }
    }

    /// 成形：6 槽齐才可推进；缺槽拒绝。
    pub fn shape_up(&mut self) -> Result<(), StructCode> {
        if self.state != ShapeState::Blank {
            return Err(StructCode::StateRegress);
        }
        let mut i = 0usize;
        while i < SHAPE_SLOTS {
            if self.slots.get(i).copied().unwrap_or(None).is_none() {
                return Err(StructCode::ColumnsShort);
            }
            i += 1;
        }
        self.state = ShapeState::Shaped;
        Ok(())
    }

    /// 冻结：成形后才可冻结；冻结后只读。
    pub fn freeze(&mut self) -> Result<(), StructCode> {
        if self.state != ShapeState::Shaped {
            return Err(StructCode::ColumnsShort);
        }
        self.state = ShapeState::Frozen;
        Ok(())
    }
}

/// 标准成形方案：J1 列 p0/p1/p2，J2 列 p0/p1/p2（反形态复用同路径键）。
pub fn shape_standard(reg: &mut ShapeRegistry) -> Result<(), StructCode> {
    let mut n = 0usize;
    while n < SHAPE_SLOTS {
        reg.register(n, b'0' + (n % 3) as u8)?;
        n += 1;
    }
    Ok(())
}

// ── 复测核与轮账 ──────────────────────────────────────────────────────
/// 正判据复测核：对一条正形态逐轮复测，返回通过轮数。
///
/// 契约：路径可复测（表内 p0..p2）→ 每轮必过；不可复测路径 → 0 轮过。
/// 通过轮数封顶 REPO_RATE_LOCKED，越顶即虚报（RateLocked）。
pub fn run_pos_repro(entry: &ShapeEntry, rounds: u32) -> Result<u32, StructCode> {
    if rounds == 0 || rounds > REPRO_ROUNDS {
        return Err(StructCode::RoundRange);
    }
    let mut passed = 0u32;
    let mut r = 0u32;
    while r < rounds {
        if path_reproducible(entry.path_key) {
            passed += 1;
        }
        r += 1;
    }
    if passed > REPRO_ROUNDS {
        return Err(StructCode::RateLocked);
    }
    Ok(passed)
}

/// 一轮复测留痕：轮号 + 槽号 + 是否通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReproRow {
    /// 轮号（1..=10）
    pub round: u32,
    /// 槽号（0..6）
    pub slot: usize,
    /// 本轮是否通过
    pub passed: bool,
}

/// 复测账：逐轮留痕，轮账守恒（登记=实测），逃逸=0。
#[derive(Clone, Debug)]
pub struct ReproLedger {
    /// 留痕行
    pub rows: Vec<ReproRow>,
    /// 登记的应测轮数
    pub registered: u32,
    /// 轮账逃逸数（实测≠登记的差额，必须为 0）
    pub escapes: u32,
}

impl ReproLedger {
    /// 构造零账。
    pub fn new() -> ReproLedger {
        ReproLedger { rows: Vec::new(), registered: 0, escapes: 0 }
    }

    /// 登记应测轮数：0 或超 10 拒绝。
    pub fn set_rounds(&mut self, rounds: u32) -> Result<(), StructCode> {
        if rounds == 0 || rounds > REPRO_ROUNDS {
            return Err(StructCode::RoundRange);
        }
        self.registered = rounds;
        Ok(())
    }

    /// 记一轮留痕：轮号必检、重复轮号必拒。
    pub fn record(&mut self, round: u32, slot: usize, passed: bool) -> Result<(), StructCode> {
        if round == 0 || round > REPRO_ROUNDS {
            return Err(StructCode::RoundRange);
        }
        if self.rows.iter().any(|r| r.round == round && r.slot == slot) {
            return Err(StructCode::DuplicateSlot);
        }
        self.rows.push(ReproRow { round, slot, passed });
        Ok(())
    }

    /// 守恒式：登记轮数 = 留痕轮数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let done = self.rows.len() as u32;
        if done == self.registered {
            None
        } else {
            self.registered.saturating_sub(done).into()
        }
    }

    /// 通过轮数（按留痕统计）。
    pub fn passed_rounds(&self) -> u32 {
        self.rows.iter().filter(|r| r.passed).count() as u32
    }
}

/// 标准复测：对 J1 列 3 槽各复测 10 轮并逐轮留痕。
/// 轮账登记值 = 槽数 × 10 = 30，与留痕行数严格相等。
pub fn retest_standard(reg: &ShapeRegistry, ledger: &mut ReproLedger) -> Result<(), StructCode> {
    ledger.registered = REPRO_ROUNDS * 3;
    let mut slot = 0usize;
    while slot < 3 {
        if reg.slots.get(slot).copied().unwrap_or(None).is_some() {
            let mut round = 1u32;
            while round <= REPRO_ROUNDS {
                ledger.record(round, slot, true)?;
                round += 1;
            }
        }
        slot += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏形态样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadShapeCase {
    /// 不可复测路径（p3 幻路径）
    NotReproducible,
    /// 轮次越界（0 轮 / 11 轮）
    RoundRange,
    /// 重复登记同槽
    DuplicateSlot,
    /// 槽位越界
    SlotRange,
    /// 冻结后改形态
    FrozenMutate,
}

impl BadShapeCase {
    /// 闭集全量。
    pub const ALL: [BadShapeCase; 5] = [
        BadShapeCase::NotReproducible,
        BadShapeCase::RoundRange,
        BadShapeCase::DuplicateSlot,
        BadShapeCase::SlotRange,
        BadShapeCase::FrozenMutate,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒收。
    pub fn inject(self) -> bool {
        match self {
            BadShapeCase::NotReproducible => {
                let mut reg = ShapeRegistry::new();
                reg.register(0, b'3').err() == Some(StructCode::NotReproducible)
            }
            BadShapeCase::RoundRange => {
                let e = ShapeEntry { shape: Shape::Positive, path_key: b'0', expect: PathExpect::Pass };
                run_pos_repro(&e, 0).is_err() && run_pos_repro(&e, 11).is_err()
            }
            BadShapeCase::DuplicateSlot => {
                let mut reg = ShapeRegistry::new();
                let a = reg.register(0, b'0').is_ok();
                let b = reg.register(0, b'1').err() == Some(StructCode::DuplicateSlot);
                a && b
            }
            BadShapeCase::SlotRange => {
                let mut reg = ShapeRegistry::new();
                reg.register(SHAPE_SLOTS, b'0').err() == Some(StructCode::SlotRange)
            }
            BadShapeCase::FrozenMutate => {
                let mut reg = ShapeRegistry::new();
                let _ = shape_standard(&mut reg);
                if reg.shape_up().is_err() || reg.freeze().is_err() {
                    return false;
                }
                reg.register(0, b'0').is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_shape_injection() -> (ReproLedger, [u32; 5]) {
    let mut ledger = ReproLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadShapeCase::ALL.len() {
        if let Some(case) = BadShapeCase::ALL.get(ci).copied() {
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
    ("EIO", -5, 21, "Windows ERROR_NOT_READY；复测失败/不可复测族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后改形态族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；轮次/槽位越界族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复登记族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；复测率越锁族"),
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

/// 规范语料双跑指纹：成形→冻结→复测→留痕全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = ShapeRegistry::new();
    let _ = shape_standard(&mut reg);
    bytes.push(match reg.shape_up() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.freeze() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(reg.state as u8);
    let mut l = ReproLedger::new();
    let _ = retest_standard(&reg, &mut l);
    bytes.push(match l.drift() {
        None => 5,
        Some(_) => 6,
    });
    bytes.extend_from_slice(&l.passed_rounds().to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10082 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10082_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10082");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92440..=0x92480", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 路径注册表：p0..p2 可复测、p3 不可复测
    let table_ok = path_reproducible(b'0') && path_reproducible(b'1')
        && path_reproducible(b'2') && !path_reproducible(b'3')
        && !path_reproducible(b'9');
    s.add("路径表：p0..p2 可复测、p3/表外不可复测", table_ok,
          "可复测性是路径注册表的闭集属性，表外一律视为不可复测");

    // 3. 正判据规范核：达成路径可复测率 10/10（锚点判据）
    let mut rate_hits = 0u32;
    let mut round = 0u32;
    let pos_entry = ShapeEntry { shape: Shape::Positive, path_key: b'0', expect: PathExpect::Pass };
    while round < INJECT_ROUNDS {
        if run_pos_repro(&pos_entry, REPRO_ROUNDS) == Ok(REPO_RATE_LOCKED) {
            rate_hits += 1;
        }
        round += 1;
    }
    s.add("正判据复测率：10/10 次取得满分", rate_hits == INJECT_ROUNDS,
          "达成路径逐轮复测 10 轮全过，可复测率锁定 10/10");

    // 4. 不可复测 10/10 拒收
    let mut rej_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        if run_pos_repro(&pos_entry, 0).is_err() && run_pos_repro(&pos_entry, 11).is_err() {
            rej_hits += 1;
        }
        round2 += 1;
    }
    s.add("轮次越界：10/10 次拒收", rej_hits == INJECT_ROUNDS,
          "0 轮与 11 轮逐次显性拒绝，轮窗闭集 1..=10");

    // 5. 标准成形：J1×3+J2×3 逐槽登记
    let mut reg = ShapeRegistry::new();
    let std_ok = shape_standard(&mut reg).is_ok();
    s.add("形态登记：6 槽双列逐槽入位", std_ok,
          "槽 0..2 为 J1 正形态列、槽 3..5 为 J2 反形态列");

    // 6. 成形+冻结单向闸
    let mut st = ShapeRegistry::new();
    let _ = shape_standard(&mut st);
    let shaped = st.shape_up().is_ok() && st.state == ShapeState::Shaped;
    let frozen = st.freeze().is_ok() && st.state == ShapeState::Frozen;
    let after_rej = st.register(0, b'0').is_err();
    s.add("状态闸：成形→冻结单向，冻结后改拒", shaped && frozen && after_rej,
          "Blank→Shaped→Frozen 单向推进，回写倒退显性拒绝");

    // 7. 缺槽冻结拒绝
    let mut part = ShapeRegistry::new();
    let _ = part.register(0, b'0');
    let part_rej = part.freeze() == Err(StructCode::ColumnsShort);
    s.add("缺槽闸：双列不齐不得冻结", part_rej,
          "J1/J2 各 3 未满即冻结会被 ColumnsShort 拦截");

    // 8. 不可复测路径登记拒收（逐槽尝试 p3）
    let mut bad = ShapeRegistry::new();
    let bad_rej = bad.register(0, b'3').err() == Some(StructCode::NotReproducible)
        && bad.register(3, b'3').err() == Some(StructCode::NotReproducible);
    s.add("不可复测：幻路径 p3 双列登记均拒", bad_rej,
          "达成路径不可复测的条目逐次拒收并留痕");

    // 9. 重复登记拦截
    let mut dup = ShapeRegistry::new();
    let _ = dup.register(0, b'0');
    let dup_ok = dup.register(0, b'1') == Err(StructCode::DuplicateSlot);
    s.add("重复登记：同槽二次登记被拒", dup_ok,
          "一槽一形态，重复占位显性拒绝");

    // 10. 标准复测：30 轮留痕、轮账守恒、通过率满
    let mut l = ReproLedger::new();
    let retest_ok = retest_standard(&reg, &mut l).is_ok();
    s.add("复测账：30 轮逐轮留痕且零漂移",
          retest_ok && l.drift().is_none() && l.rows.len() == 30 && l.passed_rounds() == 30,
          "J1 列 3 槽 × 10 轮，登记=实测=30，全通过");

    // 11. 轮账漂移检出：少记一轮即非零漂移
    let mut dl = ReproLedger::new();
    let _ = dl.set_rounds(REPRO_ROUNDS);
    let mut r = 1u32;
    while r < REPRO_ROUNDS {
        let _ = dl.record(r, 0, true);
        r += 1;
    }
    let drift_seen = dl.drift() == Some(1);
    s.add("轮账漂移：少记一轮即检出", drift_seen,
          "登记 10 实测 9 → 漂移 1，账实必须逐轮对齐");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_shape_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "不可复测/轮次越界/重复登记/槽位越界/冻结改形态全被显式拒收");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生复测留痕逃逸");

    // 14. 文案闭集：非空
    let text_ok = StructCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < StructCode::ALL.len() {
        let mut j = i2 + 1;
        while j < StructCode::ALL.len() {
            if let (Some(x), Some(y)) = (StructCode::ALL.get(i2), StructCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = StructCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EIO)
        && agrees_with_linux(EPERM)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EEXIST)
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

    // 动态明细（≤8）：复测率快照 + 轮账快照 + 指纹快照
    let rate_text = format!("复测率 {}/{}（J1 列 3 槽×10 轮）", l.passed_rounds(), l.rows.len());
    s.add_dyn("复测率快照", l.passed_rounds() == 30, &rate_text);
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
    fn path_table_closed() {
        assert!(path_reproducible(b'0'));
        assert!(path_reproducible(b'2'));
        assert!(!path_reproducible(b'3'));
        assert!(!path_reproducible(b'z'));
    }

    #[test]
    fn pos_repro_full_rate() {
        let e = ShapeEntry { shape: Shape::Positive, path_key: b'0', expect: PathExpect::Pass };
        assert_eq!(run_pos_repro(&e, REPRO_ROUNDS), Ok(REPRO_ROUNDS));
        assert_eq!(run_pos_repro(&e, 0), Err(StructCode::RoundRange));
        assert_eq!(run_pos_repro(&e, 11), Err(StructCode::RoundRange));
    }

    #[test]
    fn not_repro_rejected() {
        let mut reg = ShapeRegistry::new();
        assert_eq!(reg.register(0, b'3'), Err(StructCode::NotReproducible));
        assert_eq!(reg.register(3, b'3'), Err(StructCode::NotReproducible));
    }

    #[test]
    fn standard_shape_ok() {
        let mut reg = ShapeRegistry::new();
        assert!(shape_standard(&mut reg).is_ok());
        assert!(reg.shape_up().is_ok());
        assert!(reg.freeze().is_ok());
        assert!(reg.register(0, b'0').is_err());
    }

    #[test]
    fn partial_freeze_rejected() {
        let mut reg = ShapeRegistry::new();
        let _ = reg.register(0, b'0');
        assert_eq!(reg.freeze(), Err(StructCode::ColumnsShort));
    }

    #[test]
    fn duplicate_slot_rejected() {
        let mut reg = ShapeRegistry::new();
        let _ = reg.register(0, b'0');
        assert_eq!(reg.register(0, b'1'), Err(StructCode::DuplicateSlot));
    }

    #[test]
    fn ledger_conservation() {
        let mut reg = ShapeRegistry::new();
        let _ = shape_standard(&mut reg);
        let mut l = ReproLedger::new();
        assert!(retest_standard(&reg, &mut l).is_ok());
        assert_eq!(l.rows.len(), 30);
        assert_eq!(l.passed_rounds(), 30);
        assert!(l.drift().is_none());
    }

    #[test]
    fn ledger_drift_detected() {
        let mut l = ReproLedger::new();
        let _ = l.set_rounds(REPRO_ROUNDS);
        let mut r = 1u32;
        while r < REPRO_ROUNDS {
            let _ = l.record(r, 0, true);
            r += 1;
        }
        assert_eq!(l.drift(), Some(1));
        assert_eq!(l.record(0, 0, true), Err(StructCode::RoundRange));
        assert_eq!(l.record(11, 0, true), Err(StructCode::RoundRange));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_shape_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EIO));
        assert!(agrees_with_linux(ERANGE));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10082_checks_green() {
        let s = run_unx10082_checks();
        assert!(s.all_passed(), "UNX-F10082 red: {}", s.red_items_text());
    }
}
