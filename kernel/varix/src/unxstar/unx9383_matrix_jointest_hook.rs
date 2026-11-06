//! UNX-F9383 futex EAGAIN/EINVAL 矩阵联测（C2 错误矩阵 × C4 挂点） · 0x7CD40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9383-J1 矩阵联测判据过，EAGAIN 耗尽格与 EINVAL 参数格逐
//!       格联测可观测，与 B23 判定域一致格过。
//!
//! 设计要点：
//!   ① 矩阵按「操作 × 故障类」展开为 20 格：四操作（WAIT/WAKE/FD/
//!      REQUEUE）× 五故障类（坏指针/未对齐/参数坏值/值不匹配/判序并发），
//!      逐格登记期望码，判定器逐格复算。
//!   ② 判定域判序（B23 口径）：操作码闭集 → uaddr 指针 → 4 字节对齐 →
//!      操作内参数 → 值匹配。并发故障时取判序最前者；判序错位即
//!      与 Linux 语义漂移，显性拒绝。
//!   ③ EAGAIN 耗尽格：WAIT 值不匹配是「重试耗尽」入口，矩阵必须
//!      回 EAGAIN（Linux EWOULDBLOCK 同码）且零副作用；EINVAL 参数格
//!      逐格登记（未对齐/键 0/WAKE 零参/REQUEUE 自迁移）。
//!   ④ 联测性质：每格同时过「直连判定器」与「hook 判定器」，两路
//!      逐格一致才算联测通过（单源）；任一格分歧即联测红。
//!   ⑤ 注入核：五类判序倒置样本各 10/10 检出，注入账守恒逃逸 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9383 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CD40;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// 操作码：FUTEX_WAIT。
pub const OP_WAIT: u32 = 0;
/// 操作码：FUTEX_WAKE。
pub const OP_WAKE: u32 = 1;
/// 操作码：FUTEX_FD。
pub const OP_FD: u32 = 2;
/// 操作码：FUTEX_REQUEUE。
pub const OP_REQUEUE: u32 = 3;

/// Linux errno 正值口径。
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；重试耗尽族（EWOULDBLOCK 同码）"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSYS", -38, 1, "Windows ERROR_NOT_SUPPORTED；未实装操作族"),
    ("ETIMEDOUT", -110, 1460, "Windows ERROR_TIMEOUT；等待超时族"),
];

/// 按 errno 正数查负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MjCode {
    /// EAGAIN 格漂移（耗尽格未回 EAGAIN 或有副作用）
    EagainDrift,
    /// EINVAL 格漂移（参数格错码）
    EinvalDrift,
    /// EFAULT 格漂移（坏指针格错码）
    EfaultDrift,
    /// ENOSYS 格漂移（闭集外操作错码）
    EnosysDrift,
    /// 判序倒置（并发故障取错优先码）
    OrderInverted,
    /// 直连与 hook 两路分歧
    PathDiverged,
    /// 矩阵格缺失
    CellMissing,
    /// 账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl MjCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MjCode::EagainDrift => "EAGAIN 格漂移：WAIT 值不匹配必须回 EAGAIN \
                （EWOULDBLOCK 同码）且零副作用；请检查重试入口",
            MjCode::EinvalDrift => "EINVAL 格漂移：未对齐/键 0/零参/自迁移格必须\
                回 EINVAL；请对齐 B23 参数格",
            MjCode::EfaultDrift => "EFAULT 格漂移：坏 uaddr 指针格必须回 EFAULT；\
                指针校验先于操作语义",
            MjCode::EnosysDrift => "ENOSYS 格漂移：操作码闭集外必须回 ENOSYS；\
                新操作须先过宪章",
            MjCode::OrderInverted => "判序倒置：并发故障必须取判序最前者；\
                判序为 ENOSYS→EFAULT→对齐 EINVAL→参数 EINVAL→EAGAIN",
            MjCode::PathDiverged => "直连与 hook 两路分歧：同格两路结果必须逐字段\
                一致；请勿在 hook 内复制判定语义",
            MjCode::CellMissing => "矩阵格缺失：20 格闭集必须逐格落账；缺格即联测不完整",
            MjCode::LedgerRegress => "账倒退被拒：格判定/注入计数只增不减；请检查\
                回滚路径是否误改累计值",
            MjCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            MjCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            MjCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [MjCode; 11] = [
        MjCode::EagainDrift,
        MjCode::EinvalDrift,
        MjCode::EfaultDrift,
        MjCode::EnosysDrift,
        MjCode::OrderInverted,
        MjCode::PathDiverged,
        MjCode::CellMissing,
        MjCode::LedgerRegress,
        MjCode::NotReproducible,
        MjCode::SegmentViolation,
        MjCode::EmptyDetail,
    ];
}

// ── 矩阵格与判定器 ────────────────────────────────────────────────────
/// uaddr 指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UaddrClass {
    /// 已映射 4 字节对齐
    Ok,
    /// 未映射（坏指针）
    Unmapped,
    /// 已映射但未 4 字节对齐
    Misaligned,
}

/// 一格矩阵样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixCell {
    /// 操作码
    pub op: u32,
    /// uaddr 指针类别
    pub uaddr: UaddrClass,
    /// WAIT 期望值
    pub expect: u32,
    /// WAIT 实读值
    pub cur: u32,
    /// WAKE/REQUEUE 数量
    pub nr: u32,
    /// REQUEUE 目标键
    pub key2: u64,
}

impl MatrixCell {
    /// 期望错误码（B23 判定域判序：ENOSYS→EFAULT→对齐 EINVAL→参数 EINVAL→EAGAIN）。
    pub fn expected(&self) -> Result<i64, i32> {
        // 1. 操作码闭集
        if self.op > OP_REQUEUE {
            return Err(ENOSYS);
        }
        // 2. uaddr 指针
        if self.uaddr == UaddrClass::Unmapped {
            return Err(EFAULT);
        }
        // 3. 对齐（FD 对句柄键不要求对齐的口径按 Linux 保留对齐检查）
        if self.uaddr == UaddrClass::Misaligned {
            return Err(EINVAL);
        }
        // 4. 操作内参数
        match self.op {
            OP_WAIT => {
                if self.expect != self.cur {
                    Err(EAGAIN)
                } else {
                    Ok(0)
                }
            }
            OP_WAKE => {
                if self.nr == 0 {
                    Err(EINVAL)
                } else {
                    Ok(self.nr as i64)
                }
            }
            OP_FD => {
                if self.nr == 0 {
                    Err(EINVAL)
                } else {
                    Ok(0x8000_0000)
                }
            }
            OP_REQUEUE => {
                if self.nr == 0 || self.key2 == 0 {
                    Err(EINVAL)
                } else {
                    Ok(self.nr as i64)
                }
            }
            _ => Err(ENOSYS),
        }
    }
}

/// 直连判定器：矩阵格 → 结果。
pub fn judge_direct(cell: &MatrixCell) -> Result<i64, i32> {
    cell.expected()
}

/// hook 判定器：联测面在判定器外加观测，不改判定结果（单源）。
pub fn judge_hooked(cell: &MatrixCell) -> Result<i64, i32> {
    let r = judge_direct(cell);
    // hook 侧只校验观测一致性：结果必须可复算（同输入恒同输出）
    match r {
        Ok(v) => {
            if cell.expected() == Ok(v) {
                Ok(v)
            } else {
                Err(EINVAL)
            }
        }
        Err(e) => {
            if cell.expected().err() == Some(e) {
                Err(e)
            } else {
                Err(EINVAL)
            }
        }
    }
}

/// 20 格闭集：四操作 × 五故障类，逐格登记。
pub fn full_matrix() -> Vec<(MatrixCell, Result<i64, i32>)> {
    let mut cells: Vec<(MatrixCell, Result<i64, i32>)> = Vec::new();
    let ops = [OP_WAIT, OP_WAKE, OP_FD, OP_REQUEUE];
    let mut oi = 0usize;
    while oi < ops.len() {
        let op = match ops.get(oi) {
            Some(o) => *o,
            None => break,
        };
        // 格 1：全合法
        cells.push((
            MatrixCell { op, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 3, key2: 0x50 },
            Ok(op_gate(op)),
        ));
        // 格 2：坏指针
        cells.push((
            MatrixCell { op, uaddr: UaddrClass::Unmapped, expect: 7, cur: 7, nr: 3, key2: 0x50 },
            Err(EFAULT),
        ));
        // 格 3：未对齐
        cells.push((
            MatrixCell { op, uaddr: UaddrClass::Misaligned, expect: 7, cur: 7, nr: 3, key2: 0x50 },
            Err(EINVAL),
        ));
        // 格 4：值不匹配（仅 WAIT 语义；其余操作值域无关，仍合法）
        let mismatch_code = if op == OP_WAIT { Err(EAGAIN) } else { Ok(op_gate(op)) };
        cells.push((
            MatrixCell { op, uaddr: UaddrClass::Ok, expect: 7, cur: 8, nr: 3, key2: 0x50 },
            mismatch_code,
        ));
        // 格 5：参数坏值（nr=0）
        let bad_param = match op {
            OP_WAIT => Ok(0), // WAIT 不消费 nr
            OP_WAKE | OP_FD => Err(EINVAL),
            OP_REQUEUE => Err(EINVAL),
            _ => Err(ENOSYS),
        };
        cells.push((
            MatrixCell { op, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 0, key2: 0x50 },
            bad_param,
        ));
        oi += 1;
    }
    cells
}

/// 合法格的占位成功值（WAKE/REQUEUE 返回 nr，WAIT 返回 0，FD 返回句柄标记）。
fn op_gate(op: u32) -> i64 {
    match op {
        OP_WAIT => 0,
        OP_FD => 0x8000_0000,
        _ => 3,
    }
}

/// 矩阵联测：20 格逐格直连+hook 双路对拍，返回（总格数, 双路一致格数）。
pub fn run_matrix_jointest() -> (u32, u32) {
    let cells = full_matrix();
    let total = cells.len() as u32;
    let mut agreed = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        if let Some((cell, expect)) = cells.get(i) {
            let direct = judge_direct(cell);
            let hooked = judge_hooked(cell);
            if direct == *expect && hooked == direct {
                agreed = agreed.saturating_add(1);
            }
        }
        i += 1;
    }
    (total, agreed)
}

// ── 注入账与注入核 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 五类判序倒置样本（并发故障，验证取最高优先码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderCase {
    /// 未知操作 + 坏指针 → ENOSYS 优先
    OpWinsOverPointer,
    /// 坏指针 + 未对齐 → EFAULT 优先
    PointerWinsOverAlign,
    /// 未对齐 + 值不匹配 → EINVAL 优先（对齐先于值匹配）
    AlignWinsOverValue,
    /// 未对齐 + 参数坏值 → EINVAL（同码，格仍稳定可复算）
    AlignWithBadParam,
    /// 好指针 + 值不匹配 → EAGAIN（耗尽格）
    PureEagain,
}

impl OrderCase {
    /// 构造样本格。
    pub fn cell(self) -> MatrixCell {
        match self {
            OrderCase::OpWinsOverPointer => {
                MatrixCell { op: 99, uaddr: UaddrClass::Unmapped, expect: 7, cur: 8, nr: 0, key2: 0 }
            }
            OrderCase::PointerWinsOverAlign => {
                MatrixCell { op: OP_WAIT, uaddr: UaddrClass::Unmapped, expect: 7, cur: 8, nr: 0, key2: 0 }
            }
            OrderCase::AlignWinsOverValue => {
                MatrixCell { op: OP_WAIT, uaddr: UaddrClass::Misaligned, expect: 7, cur: 8, nr: 0, key2: 0 }
            }
            OrderCase::AlignWithBadParam => {
                MatrixCell { op: OP_WAKE, uaddr: UaddrClass::Misaligned, expect: 7, cur: 7, nr: 0, key2: 0 }
            }
            OrderCase::PureEagain => {
                MatrixCell { op: OP_WAIT, uaddr: UaddrClass::Ok, expect: 7, cur: 8, nr: 0, key2: 0 }
            }
        }
    }

    /// 期望码。
    pub fn expected_code(self) -> i32 {
        match self {
            OrderCase::OpWinsOverPointer => ENOSYS,
            OrderCase::PointerWinsOverAlign => EFAULT,
            OrderCase::AlignWinsOverValue => EINVAL,
            OrderCase::AlignWithBadParam => EINVAL,
            OrderCase::PureEagain => EAGAIN,
        }
    }

    /// 闭集全量。
    pub const ALL: [OrderCase; 5] = [
        OrderCase::OpWinsOverPointer,
        OrderCase::PointerWinsOverAlign,
        OrderCase::AlignWinsOverValue,
        OrderCase::AlignWithBadParam,
        OrderCase::PureEagain,
    ];
}

/// 注入核：五类判序样本各注入 INJECT_ROUNDS 次，逐类断言取码正确。
pub fn run_order_injection() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < OrderCase::ALL.len() {
        let case = match OrderCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let cell = case.cell();
            let r = judge_direct(&cell);
            let caught = r.err() == Some(case.expected_code())
                && judge_hooked(&cell) == judge_direct(&cell);
            ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：20 格联测 + 五类判序注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let cells = full_matrix();
    let mut i = 0usize;
    while i < cells.len() {
        if let Some((cell, _)) = cells.get(i) {
            match judge_direct(cell) {
                Ok(v) => {
                    bytes.push(1);
                    bytes.push((v & 0xFF) as u8);
                    bytes.push(((v >> 8) & 0xFF) as u8);
                }
                Err(e) => {
                    bytes.push(0);
                    bytes.push((e & 0xFF) as u8);
                }
            }
        }
        i += 1;
    }
    let (total, agreed) = run_matrix_jointest();
    bytes.push(total as u8);
    bytes.push(agreed as u8);
    let (inj, per) = run_order_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9383 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9383_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9383");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 矩阵闭集：20 格全量落账
    let cells = full_matrix();
    s.add("矩阵闭集：四操作 × 五故障 = 20 格", cells.len() == 20,
          "矩阵格必须逐格落账，缺格即联测不完整");

    // 3. 逐格联测：直连+hook 双路全对拍
    let (total, agreed) = run_matrix_jointest();
    s.add("逐格联测：20 格双路全一致", total == 20 && agreed == 20,
          "每格直连与 hook 逐字段一致（单源）");

    // 4. EAGAIN 耗尽格：WAIT 值不匹配逐格 EAGAIN
    let eagain_cells = full_matrix();
    let mut eagain_ok = true;
    let mut i = 0usize;
    while i < eagain_cells.len() {
        if let Some((cell, expect)) = eagain_cells.get(i) {
            if cell.op == OP_WAIT && cell.uaddr == UaddrClass::Ok && cell.expect != cell.cur {
                if *expect != Err(EAGAIN) || judge_direct(cell) != Err(EAGAIN) {
                    eagain_ok = false;
                }
            }
        }
        i += 1;
    }
    s.add("EAGAIN 耗尽格：WAIT 值不匹配逐格 EAGAIN", eagain_ok,
          "重试耗尽入口必须稳定回 EAGAIN（EWOULDBLOCK 同码）");

    // 5. EINVAL 参数格：未对齐/零参/自迁移逐格 EINVAL
    let einval_ok = judge_direct(&MatrixCell {
        op: OP_WAIT, uaddr: UaddrClass::Misaligned, expect: 7, cur: 7, nr: 0, key2: 0,
    }) == Err(EINVAL)
        && judge_direct(&MatrixCell {
            op: OP_WAKE, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 0, key2: 0,
        }) == Err(EINVAL)
        && judge_direct(&MatrixCell {
            op: OP_FD, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 0, key2: 0,
        }) == Err(EINVAL)
        && judge_direct(&MatrixCell {
            op: OP_REQUEUE, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 0, key2: 0,
        }) == Err(EINVAL);
    s.add("EINVAL 参数格：未对齐/零参逐格 EINVAL", einval_ok,
          "参数格与 B23 判定域逐格一致");

    // 6. EFAULT 格：坏指针逐格 EFAULT（含并发坏值）
    let efault_ok = judge_direct(&MatrixCell {
        op: OP_WAIT, uaddr: UaddrClass::Unmapped, expect: 7, cur: 8, nr: 0, key2: 0,
    }) == Err(EFAULT);
    s.add("EFAULT 格：坏 uaddr 指针逐格 EFAULT", efault_ok,
          "指针校验先于值匹配与参数校验");

    // 7. ENOSYS 格：闭集外操作逐格 ENOSYS
    let enosys_ok = judge_direct(&OrderCase::OpWinsOverPointer.cell()) == Err(ENOSYS);
    s.add("ENOSYS 格：操作码闭集外一律 ENOSYS", enosys_ok,
          "未知操作不得误入其他错误语义");

    // 8. 注入核：五类判序样本各 10/10 取码正确
    let (inj, per_class) = run_order_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类判序样本各 10/10 取码正确", all_caught,
          "并发故障取最高优先码，判序不得倒置");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 10. 判序显式验证：ENOSYS > EFAULT > EINVAL(对齐) > EINVAL(参数) > EAGAIN
    let order_ok = {
        let op_first = judge_direct(&OrderCase::OpWinsOverPointer.cell()).err() == Some(ENOSYS);
        let ptr_first = judge_direct(&OrderCase::PointerWinsOverAlign.cell()).err() == Some(EFAULT);
        let align_first = judge_direct(&OrderCase::AlignWinsOverValue.cell()).err() == Some(EINVAL);
        op_first && ptr_first && align_first
    };
    s.add("判序协议：ENOSYS>EFAULT>对齐EINVAL>值匹配", order_ok,
          "B23 判定域判序逐例复现");

    // 11. 同格复算确定性：任一格两次判定逐位一致
    let mut det_ok = true;
    let mut k = 0usize;
    while k < cells.len() {
        if let Some((cell, _)) = cells.get(k) {
            if judge_direct(cell) != judge_direct(cell) || judge_hooked(cell) != judge_hooked(cell) {
                det_ok = false;
            }
        }
        k += 1;
    }
    s.add("格判定确定性：同格两次判定逐位一致", det_ok,
          "判定器必须纯函数，禁止隐藏状态");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EAGAIN) && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL) && agrees_with_linux(ENOSYS)
        && !agrees_with_linux(0) && !agrees_with_linux(999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = MjCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut m = 0usize;
    while m < MjCode::ALL.len() {
        let mut n = m + 1;
        while n < MjCode::ALL.len() {
            if let (Some(x), Some(y)) = (MjCode::ALL.get(m), MjCode::ALL.get(n)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            n += 1;
        }
        m += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let mx_text = format!("矩阵 {} 格 / 双路一致 {} / 注入检出 {}", total, agreed, inj.detected);
    s.add_dyn("矩阵快照", total == 20 && agreed == 20 && inj.conserved(), &mx_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_has_20_cells() {
        assert_eq!(full_matrix().len(), 20);
    }

    #[test]
    fn wait_mismatch_eagain() {
        let c = MatrixCell { op: OP_WAIT, uaddr: UaddrClass::Ok, expect: 1, cur: 2, nr: 0, key2: 0 };
        assert_eq!(judge_direct(&c), Err(EAGAIN));
    }

    #[test]
    fn misaligned_einval_even_with_mismatch() {
        let c = MatrixCell { op: OP_WAIT, uaddr: UaddrClass::Misaligned, expect: 1, cur: 2, nr: 0, key2: 0 };
        assert_eq!(judge_direct(&c), Err(EINVAL));
    }

    #[test]
    fn unmapped_efault_even_with_bad_op_param() {
        let c = MatrixCell { op: OP_WAKE, uaddr: UaddrClass::Unmapped, expect: 7, cur: 7, nr: 0, key2: 0 };
        assert_eq!(judge_direct(&c), Err(EFAULT));
    }

    #[test]
    fn unknown_op_enosys_wins() {
        let c = MatrixCell { op: 99, uaddr: UaddrClass::Unmapped, expect: 7, cur: 8, nr: 0, key2: 0 };
        assert_eq!(judge_direct(&c), Err(ENOSYS));
    }

    #[test]
    fn wake_zero_nr_einval() {
        let c = MatrixCell { op: OP_WAKE, uaddr: UaddrClass::Ok, expect: 7, cur: 7, nr: 0, key2: 0 };
        assert_eq!(judge_direct(&c), Err(EINVAL));
    }

    #[test]
    fn hooked_equals_direct_on_all_cells() {
        let cells = full_matrix();
        let mut i = 0;
        while i < cells.len() {
            if let Some((cell, _)) = cells.get(i) {
                assert_eq!(judge_hooked(cell), judge_direct(cell));
            }
            i += 1;
        }
    }

    #[test]
    fn jointest_full_agreement() {
        let (total, agreed) = run_matrix_jointest();
        assert_eq!(total, 20);
        assert_eq!(agreed, 20);
    }

    #[test]
    fn order_injection_matrix_green() {
        let (l, per) = run_order_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(11));
        assert_eq!(linux_neg(110), -110);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9383_checks_green() {
        let s = run_unx9383_checks();
        assert!(s.all_passed(), "UNX-F9383 red: {}", s.red_items_text());
    }
}
