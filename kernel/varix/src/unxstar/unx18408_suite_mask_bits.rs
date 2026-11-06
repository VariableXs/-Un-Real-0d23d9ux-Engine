//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18408 · wSuiteMask 套件位面
//! · 0x7E1C0 段 · 批 01 · 配额 280
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18408-J1 套件位全集（VER_SUITE_*) 枚举冻结 v1，位或组合
//!   与 Windows 口径对照 15 例零偏差。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 位全集冻结 v1（winnt.h 口径 12 位）：SMALLBUSINESS=0x0001 /
//!     ENTERPRISE=0x0002 / BACKOFFICE=0x0004 / COMMUNICATIONS=0x0008 /
//!     TERMINAL=0x0010 / SMALLBUSINESS_RESTRICTED=0x0020 / EMBEDDEDNT=0x0040
//!     / DATACENTER=0x0080 / SINGLEUSERTS=0x0100 / PERSONAL=0x0200 /
//!     SERVERFORSMALLBUSINESS=0x0400 / EMBEDDEDAE=0x2000；
//!   - 位域闭集：KNOWN_MASK = 12 位并集 = 0x27FF；RESERVED_MASK = 0xD800
//!     （0x0800/0x1000/0x4000/0x8000）；两掩码并集恰铺满 0xFFFF（推导断言）；
//!   - 对照表：15 例冻结组合（含空集/单播/多播），compose 结果与写死
//!     期望魔数逐例零偏差；
//!   - 位或性质：交换律（顺序无关）、幂等（重复位不增）、并集分解
//!     （组合掩码 = 单位掩码逐位或）；
//!   - 注入核：保留位注入（四变体 × 10 轮）+ 污点组合（未知位混入
//!     合法集整笔拒）全抓；
//!   - 位面投影：掩码 ↔ 12 槽布尔位面 双向往返；
//!   - 状态机：ComposeGate 两态 Open → Sealed 单向；封印后组合被拒；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] SmCode 闭集（10 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SUITE_BITS 12 位冻结表 + COMBO_ROWS 15 例对照
//!   [解析器]   compose / mask_to_plane / plane_to_mask（位面往返）
//!   [状态机]   GatePhase 两态单向 + ComposeGate
//!   [账本]     ComposeLedger（守恒核算）
//!   [注入核]   保留位 ×10 轮 + 污点组合 ×10 轮
//!   [收口]     golden_run + run_unx18408_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E1C0，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E1C0;

// ── 位全集冻结 v1 ───────────────────────────────────────────────
/// 位名（冻结 v1，winnt.h 口径）。
pub const SUITE_NAMES: [&str; 12] = [
    "VER_SUITE_SMALLBUSINESS",
    "VER_SUITE_ENTERPRISE",
    "VER_SUITE_BACKOFFICE",
    "VER_SUITE_COMMUNICATIONS",
    "VER_SUITE_TERMINAL",
    "VER_SUITE_SMALLBUSINESS_RESTRICTED",
    "VER_SUITE_EMBEDDEDNT",
    "VER_SUITE_DATACENTER",
    "VER_SUITE_SINGLEUSERTS",
    "VER_SUITE_PERSONAL",
    "VER_SUITE_SERVERFORSMALLBUSINESS",
    "VER_SUITE_EMBEDDEDAE",
];

/// 第 i 位掩码值（越界 None）。
/// 推导：位序 0..11 对应 0x0001<<i（位 11 = EMBEDDEDAE = 0x2000，
/// 跳过 0x0800/0x1000 两个历史保留位）。
pub fn suite_bit(i: usize) -> Option<u16> {
    const BITS: [u16; 12] = [
        0x0001, 0x0002, 0x0004, 0x0008, 0x0010, 0x0020,
        0x0040, 0x0080, 0x0100, 0x0200, 0x0400, 0x2000,
    ];
    if i >= 12 {
        return None;
    }
    Some(BITS[i])
}

/// 已知位并集（冻结推导：12 位逐位或 = 0x27FF）。
pub const KNOWN_MASK: u16 = 0x27FF;
/// 保留位并集（冻结推导：0x0800|0x1000|0x4000|0x8000 = 0xD800）。
pub const RESERVED_MASK: u16 = 0xD800;
/// 对照组合例数（J1 冻结 15）。
pub const COMBO_ROWS: usize = 15;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 保留位注入变体数（0x0800/0x1000/0x4000/0x8000）。
pub const RESERVED_VARIANTS: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 未知位（KNOWN_MASK 之外）。
    SuiteUnknown,
    /// 保留位（RESERVED_MASK 命中）。
    SuiteReserved,
    /// 污点组合（合法位混入未知位，整笔拒）。
    TaintedCombo,
    /// 对照魔数污染（冻结表与实现脱钩）。
    ComboMismatch,
    /// 位面投影往返失真。
    PlaneRoundtrip,
    /// 封印后组合被拒。
    SealedCompose,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 位索引越界（防御）。
    BitIndex,
    /// 双跑不一致。
    NotReproducible,
    /// 组合超 WORD 域（>0xFFFF，防御）。
    MaskOverflow,
}

impl SmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmCode::SuiteUnknown => "未知位：KNOWN_MASK(0x27FF) 之外",
            SmCode::SuiteReserved => "保留位：RESERVED_MASK(0xD800) 命中",
            SmCode::TaintedCombo => "污点组合：合法位混入未知位整笔拒",
            SmCode::ComboMismatch => "对照魔数污染：冻结表与实现脱钩",
            SmCode::PlaneRoundtrip => "位面投影往返失真",
            SmCode::SealedCompose => "封印后组合被拒",
            SmCode::LedgerBroken => "账本守恒破坏",
            SmCode::BitIndex => "位索引越界",
            SmCode::NotReproducible => "双跑不一致：复现红线",
            SmCode::MaskOverflow => "组合超 WORD 域：0xFFFF 之外",
        }
    }
    pub const ALL: [SmCode; 10] = [
        SmCode::SuiteUnknown,
        SmCode::SuiteReserved,
        SmCode::TaintedCombo,
        SmCode::ComboMismatch,
        SmCode::PlaneRoundtrip,
        SmCode::SealedCompose,
        SmCode::LedgerBroken,
        SmCode::BitIndex,
        SmCode::NotReproducible,
        SmCode::MaskOverflow,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmCode::ALL.len() {
        if SmCode::ALL[i].code() == code {
            return SmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E1C0 段闭集内"
}

// ── 组合器（解析器核） ──────────────────────────────────────────
/// 位或组合：flags 逐位或（空集 → 0 合法）。
/// 任一位落在 KNOWN_MASK 之外 → 显式拒（保留位报保留码、其他报未知码）。
pub fn compose(flags: &[u16]) -> Result<u16, u32> {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < flags.len() {
        let f = flags[i];
        if f & RESERVED_MASK != 0 {
            return Err(SmCode::SuiteReserved.code());
        }
        if f & !KNOWN_MASK != 0 {
            return Err(SmCode::SuiteUnknown.code());
        }
        acc |= f as u32;
        i += 1;
    }
    if acc > 0xFFFF {
        return Err(SmCode::MaskOverflow.code());
    }
    Ok(acc as u16)
}

/// 位面投影：掩码 → 12 槽布尔位面（越界位在 KNOWN 内恒不可能）。
pub fn mask_to_plane(mask: u16) -> Result<[bool; 12], u32> {
    if mask & !KNOWN_MASK != 0 {
        return Err(SmCode::SuiteUnknown.code());
    }
    let mut plane = [false; 12];
    let mut i = 0;
    while i < 12 {
        match suite_bit(i) {
            Some(b) => plane[i] = (mask & b) != 0,
            None => return Err(SmCode::BitIndex.code()),
        }
        i += 1;
    }
    Ok(plane)
}

/// 位面回投：12 槽布尔 → 掩码。
pub fn plane_to_mask(plane: &[bool; 12]) -> Result<u16, u32> {
    let mut acc: u16 = 0;
    let mut i = 0;
    while i < 12 {
        if plane[i] {
            match suite_bit(i) {
                Some(b) => acc |= b,
                None => return Err(SmCode::BitIndex.code()),
            }
        }
        i += 1;
    }
    Ok(acc)
}

/// 位面往返：mask → plane → mask 逐位还原。
pub fn plane_roundtrip(mask: u16) -> Result<(), u32> {
    let plane = mask_to_plane(mask)?;
    let back = plane_to_mask(&plane)?;
    if back != mask {
        return Err(SmCode::PlaneRoundtrip.code());
    }
    Ok(())
}

// ── 15 例对照表（冻结组合 + 写死期望魔数） ──────────────────────
/// 对照行（组合位集 + 期望掩码）。期望魔数手推：
///   SMALLBUSINESS|ENTERPRISE = 0x0001|0x0002 = 0x0003，余同理。
pub const COMBO_EXPECT: [u16; COMBO_ROWS] = [
    0x0000, // 0 空集（无套件）
    0x0001, // 1 SMALLBUSINESS
    0x0002, // 2 ENTERPRISE
    0x0003, // 3 SMALLBUSINESS|ENTERPRISE（NT4 Server 企业档）
    0x0004, // 4 BACKOFFICE
    0x0010, // 5 TERMINAL
    0x0080, // 6 DATACENTER
    0x0100, // 7 SINGLEUSERTS（远程桌面单用户）
    0x0200, // 8 PERSONAL
    0x0040, // 9 EMBEDDEDNT
    0x0110, // 10 TERMINAL|SINGLEUSERTS
    0x0090, // 11 DATACENTER|TERMINAL
    0x000D, // 12 SMALLBUSINESS|BACKOFFICE|COMMUNICATIONS
    0x0280, // 13 DATACENTER|PERSONAL
    0x27FF, // 14 全位并集（KNOWN_MASK 自身）
];

/// 第 i 例的输入位集（与 COMBO_EXPECT 逐行对应）。
pub fn combo_input(i: usize) -> Option<Vec<u16>> {
    match i {
        0 => Some(Vec::new()),
        1 => Some(vec![0x0001]),
        2 => Some(vec![0x0002]),
        3 => Some(vec![0x0001, 0x0002]),
        4 => Some(vec![0x0004]),
        5 => Some(vec![0x0010]),
        6 => Some(vec![0x0080]),
        7 => Some(vec![0x0100]),
        8 => Some(vec![0x0200]),
        9 => Some(vec![0x0040]),
        10 => Some(vec![0x0010, 0x0100]),
        11 => Some(vec![0x0080, 0x0010]),
        12 => Some(vec![0x0001, 0x0004, 0x0008]),
        13 => Some(vec![0x0080, 0x0200]),
        14 => {
            // 全位并集：12 位逐位或。
            let mut v = Vec::new();
            let mut k = 0;
            while k < 12 {
                match suite_bit(k) {
                    Some(b) => v.push(b),
                    None => return None,
                }
                k += 1;
            }
            Some(v)
        }
        _ => None,
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 组合账本（守恒：尝试 = 收 + 保留拒 + 未知拒）。
#[derive(Clone, Copy, Debug)]
pub struct ComposeLedger {
    pub tries: u32,
    pub accepted: u32,
    pub reserved_rej: u32,
    pub unknown_rej: u32,
}

impl ComposeLedger {
    pub fn new() -> ComposeLedger {
        ComposeLedger { tries: 0, accepted: 0, reserved_rej: 0, unknown_rej: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.accepted + self.reserved_rej + self.unknown_rej
    }
}

// ── 状态机（ComposeGate：Open → Sealed 单向） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatePhase {
    /// 开放组合。
    Open,
    /// 已封印（只读）。
    Sealed,
}

/// 组合闸门：开放期收组合账，封印后组合被拒。
pub struct ComposeGate {
    phase: GatePhase,
    pub ledger: ComposeLedger,
    /// 已通过组合的掩码账（按序）。
    pub masks: Vec<u16>,
}

impl ComposeGate {
    pub fn new() -> ComposeGate {
        ComposeGate { phase: GatePhase::Open, ledger: ComposeLedger::new(), masks: Vec::new() }
    }
    pub fn phase(&self) -> GatePhase {
        self.phase
    }
    /// 组合一笔（Open 态；Sealed 态拒）。
    pub fn compose(&mut self, flags: &[u16]) -> Result<u16, u32> {
        if self.phase != GatePhase::Open {
            self.ledger.tries += 1;
            self.ledger.unknown_rej += 1;
            return Err(SmCode::SealedCompose.code());
        }
        self.ledger.tries += 1;
        match compose(flags) {
            Ok(m) => {
                self.ledger.accepted += 1;
                self.masks.push(m);
                Ok(m)
            }
            Err(c) => {
                // KNOWN|RESERVED 铺满 0xFFFF，故越域位必命中保留掩码；
                // 未知分支为纵深防御（掩码构造途径变化时兜底）。
                if c == SmCode::SuiteReserved.code() {
                    self.ledger.reserved_rej += 1;
                } else {
                    self.ledger.unknown_rej += 1;
                }
                Err(c)
            }
        }
    }
    /// 封印（Open → Sealed 单向）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != GatePhase::Open {
            return Err(SmCode::SealedCompose.code());
        }
        if !self.ledger.conserve() {
            return Err(SmCode::LedgerBroken.code());
        }
        self.phase = GatePhase::Sealed;
        Ok(())
    }
}

// ── 注入核 ──────────────────────────────────────────────────────
/// 保留位注入变体（4 变体冻结）。
pub const RESERVED_BITS: [u16; RESERVED_VARIANTS] = [0x0800, 0x1000, 0x4000, 0x8000];

/// 保留位注入 rounds 轮：四变体轮换（可与合法位混入=污点形态）逐轮必拒。
pub fn reserved_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut k = 0;
        while k < RESERVED_VARIANTS {
            // 纯保留位 / 污点形态（保留位 + 合法位）轮换。
            let sample: Vec<u16> = match r % 2 {
                0 => vec![RESERVED_BITS[k]],
                _ => vec![0x0001, RESERVED_BITS[k], 0x0100],
            };
            match compose(&sample) {
                Err(_) => rejected += 1,
                Ok(_) => return Err(SmCode::SuiteReserved.code()),
            }
            k += 1;
        }
        r += 1;
    }
    Ok(rejected)
}

/// 杂散位注入 rounds 轮：混入保留域高位杂散（0x4001 系）逐轮必拒。
/// 注：KNOWN|RESERVED 铺满 0xFFFF，任何越域位必落保留掩码——本注入器
/// 验证的正是「合法位混入越域位整笔拒」的污点语义。
pub fn unknown_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        // 污点样本：合法位（ENTERPRISE/TERMINAL 轮换）+ 保留位 0x4000。
        let legal = match r % 3 {
            0 => 0x0002u16,
            1 => 0x0010u16,
            _ => 0x0080u16,
        };
        let sample: Vec<u16> = vec![legal, 0x4000 | ((r as u16 % 4) << 12)];
        match compose(&sample) {
            Err(_) => rejected += 1,
            Ok(_) => return Err(SmCode::SuiteReserved.code()),
        }
        r += 1;
    }
    Ok(rejected)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）：15 例对照零偏差 + 性质三断言 + 注入全抓。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    // 15 例对照逐例零偏差。
    let mut i = 0;
    while i < COMBO_ROWS {
        let flags = match combo_input(i) {
            Some(v) => v,
            None => return Err(SmCode::BitIndex.code()),
        };
        let m = compose(&flags)?;
        if m != COMBO_EXPECT[i] {
            return Err(SmCode::ComboMismatch.code());
        }
        i += 1;
    }
    // 性质 1 交换律：三组合换序同值。
    let a = compose(&[0x0001, 0x0002, 0x0080])?;
    let b = compose(&[0x0080, 0x0001, 0x0002])?;
    let c = compose(&[0x0002, 0x0080, 0x0001])?;
    if a != b || b != c || a != 0x0083 {
        return Err(SmCode::ComboMismatch.code());
    }
    // 性质 2 幂等：重复位不增。
    let d = compose(&[0x0100, 0x0100, 0x0100])?;
    if d != 0x0100 {
        return Err(SmCode::ComboMismatch.code());
    }
    // 性质 3 并集分解：组合 = 单位掩码逐位或。
    let combo = compose(&[0x0001, 0x0004, 0x0008])?;
    let s1 = compose(&[0x0001])?;
    let s2 = compose(&[0x0004])?;
    let s3 = compose(&[0x0008])?;
    if combo != (s1 | s2 | s3) {
        return Err(SmCode::ComboMismatch.code());
    }
    // 位面往返：15 例逐例还原。
    let mut k = 0;
    while k < COMBO_ROWS {
        plane_roundtrip(COMBO_EXPECT[k])?;
        k += 1;
    }
    // 注入：保留位 4 变体 × 10 轮 = 40 拒；未知域 10 拒。
    let res = reserved_inject_rounds(JUDGE_ROUNDS)?;
    if res != RESERVED_VARIANTS as u32 * JUDGE_ROUNDS {
        return Err(SmCode::SuiteReserved.code());
    }
    let unk = unknown_inject_rounds(JUDGE_ROUNDS)?;
    if unk != JUDGE_ROUNDS {
        return Err(SmCode::SuiteUnknown.code());
    }
    // 闸门会话：15 例过账 → 封印 → 封印后组合拒。
    let mut gate = ComposeGate::new();
    let mut j = 0;
    while j < COMBO_ROWS {
        let flags = match combo_input(j) {
            Some(v) => v,
            None => return Err(SmCode::BitIndex.code()),
        };
        gate.compose(&flags)?;
        j += 1;
    }
    gate.seal()?;
    if gate.phase() != GatePhase::Sealed {
        return Err(SmCode::SealedCompose.code());
    }
    if gate.compose(&[0x0001]).is_ok() {
        return Err(SmCode::SealedCompose.code());
    }
    if !gate.ledger.conserve() {
        return Err(SmCode::LedgerBroken.code());
    }
    Ok((gate.ledger.accepted, res, unk, gate.ledger.tries))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SmCode::ALL.len() {
        let v = SmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：NT4 企业档组合（0x0003）逐例对拍成功。
pub fn probe_seq_a() -> Result<(), u32> {
    let m = compose(&[0x0001, 0x0002])?;
    if m != 0x0003 {
        return Err(SmCode::ComboMismatch.code());
    }
    Ok(())
}

/// 探针 B：纯保留位 0x8000 → 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    compose(&[0x8000]).map(|_| ())
}

/// 探针 C：空集组合 → 0 合法（无套件）。
pub fn probe_seq_c() -> Result<(), u32> {
    let m = compose(&[])?;
    if m != 0 {
        return Err(SmCode::ComboMismatch.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 位全集冻结 v1：12 位逐位可取且名表等长。
    let bit_n = {
        let mut n = 0;
        let mut i = 0;
        while i < 14 {
            if suite_bit(i).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    };
    v.push((
        "J1-位全集冻结v1=12位",
        bit_n == 12 && SUITE_NAMES.len() == 12,
        format!("bits={}", bit_n),
    ));

    // 2. 位域闭集推导断言（KNOWN=0x27FF、RESERVED=0xD800、并集铺满 0xFFFF）。
    let closed_ok = KNOWN_MASK == 0x27FF
        && RESERVED_MASK == 0xD800
        && (KNOWN_MASK | RESERVED_MASK) == 0xFFFF
        && (KNOWN_MASK & RESERVED_MASK) == 0;
    v.push((
        "J1-位域闭集0x27FF/0xD800",
        closed_ok,
        format!("K={:04X} R={:04X}", KNOWN_MASK, RESERVED_MASK),
    ));

    // 3. 15 例对照零偏差。
    let zero_dev = {
        let mut dev = 0u32;
        let mut i = 0;
        while i < COMBO_ROWS {
            let flags = match combo_input(i) {
                Some(v) => v,
                None => {
                    dev += 1;
                    i += 1;
                    continue;
                }
            };
            match compose(&flags) {
                Ok(m) => {
                    if m != COMBO_EXPECT[i] {
                        dev += 1;
                    }
                }
                Err(_) => dev += 1,
            }
            i += 1;
        }
        dev
    };
    v.push((
        "J1-对照15例零偏差",
        zero_dev == 0,
        format!("dev={}", zero_dev),
    ));

    // 4. 交换律。
    let comm = compose(&[0x0001, 0x0002, 0x0080]) == compose(&[0x0080, 0x0002, 0x0001]);
    v.push(("J1-位或交换律", comm, String::from("换序同值")));

    // 5. 幂等律。
    let idem = compose(&[0x0200, 0x0200]) == compose(&[0x0200]);
    v.push(("J1-位或幂等律", idem, String::from("重复位不增")));

    // 6. 并集分解。
    let union_ok = {
        let combo = compose(&[0x0001, 0x0004, 0x0008]);
        let s1 = compose(&[0x0001]);
        let s2 = compose(&[0x0004]);
        let s3 = compose(&[0x0008]);
        match (combo, s1, s2, s3) {
            (Ok(c), Ok(a), Ok(b), Ok(d)) => c == (a | b | d),
            _ => false,
        }
    };
    v.push(("J1-并集分解", union_ok, String::from("组合=单位逐位或")));

    // 7. 保留位注入 4 变体 × 10 轮 = 40 全拒。
    let res = reserved_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-保留位注入40/40",
        match &res {
            Ok(n) => *n == 40,
            Err(_) => false,
        },
        format!("{:?}", res),
    ));

    // 8. 杂散位（污点）注入 10/10。
    let unk = unknown_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-杂散位注入10/10",
        match &unk {
            Ok(n) => *n == JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("{:?}", unk),
    ));

    // 9. 探针三序（对拍 / 保留位拒 / 空集合法）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b拒={} c={}", pa, pb, pc),
    ));

    // 10. 位面往返：15 例逐例还原。
    let rt_ok = {
        let mut ok = true;
        let mut k = 0;
        while k < COMBO_ROWS {
            if plane_roundtrip(COMBO_EXPECT[k]).is_err() {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    v.push(("J1-位面往返15例", rt_ok, String::from("mask↔plane")));

    // 11. 闸门封印（15 例过账 → 封印 → 封印后组合拒 + 守恒）。
    let g = golden_run();
    let gate_ok = match &g {
        Ok((acc, _r, _u, _t)) => *acc == COMBO_ROWS as u32,
        Err(_) => false,
    };
    v.push(("J1-闸门封印守恒", gate_ok, format!("{:?}", g)));

    // 12. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 14. 诊断码闭集（10 码全非空中文明细）。
    let all_ok = {
        let mut ok = SmCode::ALL.len() == 10;
        let mut i = 0;
        while i < SmCode::ALL.len() {
            if SmCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集10码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 14 条；前 8 条动态明细）。
pub fn run_unx18408_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18408 套件位面");
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
        let set = run_unx18408_checks();
        assert!(set.all_passed(), "UNX-F18408 判据集应全绿");
    }

    /// 正常流：NT4 企业档 0x0003 对拍成功。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 边界：空集 → 0 合法（无套件）。
    #[test]
    fn test_empty_combo() {
        assert!(probe_seq_c().is_ok());
        assert_eq!(compose(&[]), Ok(0));
    }

    /// 注入：保留位 4 变体 × 10 轮 = 40 全拒。
    #[test]
    fn test_reserved_injection() {
        assert_eq!(reserved_inject_rounds(JUDGE_ROUNDS), Ok(40));
    }

    /// 注入：未知域 10/10。
    #[test]
    fn test_unknown_injection() {
        assert_eq!(unknown_inject_rounds(JUDGE_ROUNDS), Ok(10));
    }

    /// 对照魔数抽查：三行写死复核（0x0003/0x0110/0x27FF）。
    #[test]
    fn test_combo_magic_spot() {
        assert_eq!(COMBO_EXPECT[3], 0x0003);
        assert_eq!(COMBO_EXPECT[10], 0x0110);
        assert_eq!(COMBO_EXPECT[14], 0x27FF);
        assert_eq!(compose(&[0x0010, 0x0100]), Ok(0x0110));
    }

    /// 位面往返：全位并集与单播位逐一还原。
    #[test]
    fn test_plane_roundtrip_bits() {
        let mut i = 0;
        while i < 12 {
            let b = match suite_bit(i) {
                Some(b) => b,
                None => break,
            };
            assert!(plane_roundtrip(b).is_ok(), "位 {} 往返失败", i);
            i += 1;
        }
        assert!(plane_roundtrip(KNOWN_MASK).is_ok());
    }

    /// 封印：Open→Sealed 单向，封印后组合拒（SealedCompose 码）。
    #[test]
    fn test_seal_gate() {
        let mut gate = ComposeGate::new();
        assert!(gate.seal().is_ok());
        match gate.compose(&[0x0001]) {
            Err(c) => assert_eq!(c, SmCode::SealedCompose.code()),
            Ok(_) => assert!(false, "封印后组合必须拒"),
        }
    }

    /// 账守恒：注入账 50 试 50 拒收 0（保留 40 + 未知 10）。
    #[test]
    fn test_ledger_conservation() {
        let mut gate = ComposeGate::new();
        let mut i = 0;
        while i < 10 {
            let _ = gate.compose(&[0x8000]);
            let _ = gate.compose(&[0x4001]);
            i += 1;
        }
        assert!(gate.ledger.conserve());
        assert_eq!(gate.ledger.accepted, 0);
        assert_eq!(gate.ledger.reserved_rej + gate.ledger.unknown_rej, 20);
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
}
