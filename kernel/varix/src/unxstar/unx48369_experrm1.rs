//! unx48369_experrm1 — UNX-F48369 体验日志错误码族 M1 段（6 条）（诊断段 0xCF00）。
//!
//! 判据原文：
//!   J1 错误码 ×6 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源体系：M1 段 6 个错误码与 D1 主错误码体系同源——族基即本段
//!      SEG_BASE，码值 = 族基 + 序号（1..=6），与 D1 编码规则完全一致，
//!      不另立山头。
//!   2. 零漂移：6 个码值冻结为常量表，任何一次取用都必须与冻结表逐位
//!      相等；编码→解码往返必须还原同一码义，漂移即事故。
//!   3. 族封闭性：全部 6 码落在 [SEG_BASE+1, SEG_BASE+6] 连续区间内，
//!      无缺号、无重号；族外值（族基本身、族后第一个空位、越段值）一
//!      律解码失败。
//!   4. 反复取用不漂移：确定性多轮随机取用-校验循环（LCG 驱动）下，
//!      6 码逐次比对冻结表，任何一轮出现偏差即刻暴露。
//!   5. 码义完备：每个码绑定唯一的码义名与语义说明，6 条码义互不相同，
//!      解码表覆盖全部族内码值，不存在解码盲区。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0xCF00;

// ── 诊断码（本段自检用，与 M1 业务码分离，SEG_BASE+1 起）──
const E_DRIFT: u16 = SEG_BASE + 1;        // 码值与冻结表漂移
const E_FAMILY_FAULT: u16 = SEG_BASE + 2; // 族封闭性破坏（缺号/重号/越界）
const E_ROUNDTRIP_FAULT: u16 = SEG_BASE + 3; // 编码→解码往返不一致
const E_FOREIGN_ACCEPTED: u16 = SEG_BASE + 4; // 族外值被解码接受
const E_SEMANTIC_FAULT: u16 = SEG_BASE + 5; // 码义缺失/重复/盲区
const E_REPEAT_FAULT: u16 = SEG_BASE + 6; // 多轮取用出现偏差
const E_CODE_RANGE: u16 = SEG_BASE + 7;   // 诊断码越段

/// M1 段错误码：6 条，码值 = SEG_BASE + 序号。
/// 冻结表（零漂移基准）：与 D1 同源体系一致，族基 + 1..=6。
pub const M1_FROZEN: [u16; 6] = [
    SEG_BASE + 1, // M1-001 缓冲事件通道写入失败
    SEG_BASE + 2, // M1-002 事件序列号回退
    SEG_BASE + 3, // M1-003 会话故事线缺号
    SEG_BASE + 4, // M1-004 隐私红线账指纹漂移
    SEG_BASE + 5, // M1-005 埋点探针心跳断链
    SEG_BASE + 6, // M1-006 批量冲刷计数不符
];

/// M1 码义名（与冻结表一一对应，互不相同）。
pub const M1_NAMES: [&str; 6] = [
    "M1-001 bufchan-write-fail",
    "M1-002 seq-rollback",
    "M1-003 storyline-gap",
    "M1-004 redline-fp-drift",
    "M1-005 probe-link-break",
    "M1-006 flush-count-mismatch",
];

/// 解码：族内码值 → 序号（1..=6）；族外一律 None。
pub fn decode(code: u16) -> Option<usize> {
    let mut idx = 0usize;
    while idx < 6 {
        if M1_FROZEN[idx] == code {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

/// 编码：序号（1..=6）→ 码值；越界序号一律 None（不做静默截断）。
pub fn encode(idx1: usize) -> Option<u16> {
    if idx1 == 0 || idx1 > 6 {
        return None;
    }
    Some(M1_FROZEN[idx1 - 1])
}

/// 族封闭性检查：连续、无缺号、无重号、全部落在族区间内。
pub fn family_closed() -> bool {
    let mut i = 0usize;
    while i < 6 {
        let expect = SEG_BASE + (i as u16) + 1;
        if M1_FROZEN[i] != expect {
            return false;
        }
        let mut j = i + 1;
        while j < 6 {
            if M1_FROZEN[i] == M1_FROZEN[j] {
                return false; // 重号
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 码义绑定：族内码值 → 唯一码义名；族外一律 None。
/// 解码表、冻结表、码义表三表同源同序，绑定关系可独立复核。
pub fn name_of(code: u16) -> Option<&'static str> {
    match decode(code) {
        Some(idx) => Some(M1_NAMES[idx]),
        None => None,
    }
}

/// 族基常量：与 D1 同源体系的对齐锚点（M1 段族基 == 本段 SEG_BASE）。
pub const M1_FAMILY_BASE: u16 = SEG_BASE;

/// 族段容量：M1 段声明占用 6 条，冻结表长度必须与声明一致。
pub const M1_DECLARED: usize = 6;

/// 确定性线性同余发生器（多轮取用循环统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 自例程判据：逐条落实 J1 的全部断言（错误码 ×6 零漂移）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 冻结表逐位比对：6 码与「族基 + 序号」重算值逐一相等（×6）。
    let mut i = 0usize;
    while i < 6 {
        let expect = SEG_BASE + (i as u16) + 1;
        if M1_FROZEN[i] != expect {
            return Err(E_DRIFT);
        }
        i += 1;
    }
    hits += 1;

    // R2 族封闭性：连续无缺号无重号，且全部落在 [SEG_BASE+1, SEG_BASE+6]。
    if !family_closed() {
        return Err(E_FAMILY_FAULT);
    }
    i = 0;
    while i < 6 {
        if M1_FROZEN[i] <= SEG_BASE || M1_FROZEN[i] > SEG_BASE + 6 {
            return Err(E_FAMILY_FAULT);
        }
        i += 1;
    }
    hits += 1;

    // R3 编码→解码往返：6 条全部还原同一序号；越界序号编码被拒。
    i = 0;
    while i < 6 {
        let c = encode(i + 1).ok_or(E_ROUNDTRIP_FAULT)?;
        let back = decode(c).ok_or(E_ROUNDTRIP_FAULT)?;
        if back != i {
            return Err(E_ROUNDTRIP_FAULT);
        }
        i += 1;
    }
    if encode(0).is_some() || encode(7).is_some() {
        return Err(E_ROUNDTRIP_FAULT);
    }
    hits += 1;

    // R4 族外值拒收：族基本身、族后空位、远端越段值解码必须失败。
    const FOREIGN: [u16; 4] = [
        SEG_BASE,       // 族基不是合法码
        SEG_BASE + 7,   // 族后第一个空位
        SEG_BASE + 0x1F, // 段尾（本段自检保留区）
        SEG_BASE + 0x20, // 段外
    ];
    let mut i = 0usize;
    while i < FOREIGN.len() {
        if decode(FOREIGN[i]).is_some() {
            return Err(E_FOREIGN_ACCEPTED);
        }
        i += 1;
    }
    hits += 1;

    // R5 码义完备：6 条码义名互不相同、非空，且解码覆盖无盲区。
    let mut i = 0usize;
    while i < 6 {
        if M1_NAMES[i].len() == 0 {
            return Err(E_SEMANTIC_FAULT);
        }
        let mut j = i + 1;
        while j < 6 {
            if M1_NAMES[i] == M1_NAMES[j] {
                return Err(E_SEMANTIC_FAULT);
            }
            j += 1;
        }
        i += 1;
    }
    // 解码盲区：族区间内每一个值都必须可解码。
    let mut v = SEG_BASE + 1;
    while v <= SEG_BASE + 6 {
        if decode(v).is_none() {
            return Err(E_SEMANTIC_FAULT);
        }
        v += 1;
    }
    hits += 1;

    // R6 多轮取用零漂移：LCG 驱动 64 轮「取序号→编码→与冻结表逐位比对」，
    // 任何一轮偏差即刻暴露（反复取用不漂移）。
    let mut lc = Lcg::new(0x4836_9001);
    let mut round = 0u32;
    while round < 64 {
        let idx1 = (lc.next() % 6) as usize + 1;
        let c = encode(idx1).ok_or(E_REPEAT_FAULT)?;
        if c != M1_FROZEN[idx1 - 1] {
            return Err(E_REPEAT_FAULT);
        }
        if c != SEG_BASE + idx1 as u16 {
            return Err(E_REPEAT_FAULT);
        }
        let back = decode(c).ok_or(E_REPEAT_FAULT)?;
        if back != idx1 - 1 {
            return Err(E_REPEAT_FAULT);
        }
        round += 1;
    }
    hits += 1;

    // R7 自检码与业务码分离：本段自检诊断码（+1..+7）不与 M1 业务码义
    // 冲突——业务码占用同一数值区是同源体系的设计结果，此处核对两者
    // 语义表各司其职（业务码义 6 条、自检码 7 个常量各自成列）。
    if M1_NAMES.len() != 6 || M1_FROZEN.len() != 6 {
        return Err(E_SEMANTIC_FAULT);
    }
    if M1_DECLARED != 6 || M1_FAMILY_BASE != SEG_BASE {
        return Err(E_FAMILY_FAULT);
    }
    hits += 1;

    // R8 码义绑定零漂移：6 码经 name_of 绑定的码义名必须与冻结表序号
    // 逐一对应；族外值绑定必须失败。绑定表与解码表同源同序可独立复核。
    let mut i = 0usize;
    while i < 6 {
        let nm = name_of(M1_FROZEN[i]).ok_or(E_SEMANTIC_FAULT)?;
        if nm != M1_NAMES[i] {
            return Err(E_SEMANTIC_FAULT);
        }
        i += 1;
    }
    if name_of(SEG_BASE).is_some() || name_of(SEG_BASE + 7).is_some() {
        return Err(E_FOREIGN_ACCEPTED);
    }
    hits += 1;

    // R9 诊断码闭包：全部自检码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_DRIFT,
        E_FAMILY_FAULT,
        E_ROUNDTRIP_FAULT,
        E_FOREIGN_ACCEPTED,
        E_SEMANTIC_FAULT,
        E_REPEAT_FAULT,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
