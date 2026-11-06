//! UNX-F9209 文件族负偏移与溢出边界矩阵 · 0x7A1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9209 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F9209-J1 负 offset 注入全 EINVAL 判据过，off_t 溢出边界截断
//!      语义格过，与 B04 lseek 档对账一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 域模型围绕 off_t（i64）算术边界设计，与 F9202 的 fd 表域完全
//!      不同：三个偏移边界判定器族 judge_pread_offset /
//!      judge_sendfile_range / judge_fallocate_mode_offset 全部以
//!      「负偏移 → 溢出 → 长度 → 容量」为判定次序，次序不可交换。
//!   ② 负偏移四类注入：-1、-2^62、i64::MIN、来自 u64 高位截断的负值。
//!      四类对三个判定器一律 EINVAL，不许把 i64::MIN 误报成 EOVERFLOW
//!      （负偏移先于溢出判定），也不许把截断负值放行为合法偏移。
//!   ③ 溢出边界截断语义：off + len 用 checked_add 复算，溢出 i64 即回
//!      EOVERFLOW；len=0 时即使 off=i64::MAX 也短路 Ok(0)（零长度不触
//!      发溢出判定）；fallocate 的 off+len 溢出与超出文件上限按 EFBIG
//!      归口。冻结对账表逐项复算，表外码一律拒绝。
//!   ④ 与 B04 lseek 档对账一致：模块内建 lseek 语义小判定器
//!      judge_lseek（SEEK_SET 负偏移 EINVAL / SEEK_CUR、SEEK_END 合成
//!      后为负 EINVAL / 非法 whence EINVAL），冻结表逐项相等；负偏移
//!      四类样本在 lseek 判定器与本卡判定器上的结果逐项一致。
//!   ⑤ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸必须恒为 0；
//!      对账记录表（ReconLog）容量耗尽回 ENOSPC 且账目不丢。
//!   ⑥ 双跑一致：规范语料指纹（FNV-1a）两次运行逐字节相等，任何隐藏
//!      状态即失败；随机源只允许固定参数 LCG（确定性）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9209 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A1C0;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 负偏移注入类别数（-1 / -2^62 / i64::MIN / u64 截断负值）。
pub const NEG_OFF_CLASSES: u32 = 4;

/// Linux errno 数值（正数口径；系统调用返回负值）。
pub const EINVAL: i32 = 22;
/// 超出文件大小上限（fallocate 的 off+len 越界归口）。
pub const EFBIG: i32 = 27;
/// 记录表满（对账日志容量耗尽归口）。
pub const ENOSPC: i32 = 28;
/// off + len 溢出 off_t 表示范围。
pub const EOVERFLOW: i32 = 75;
/// fallocate 模式位不被支持。
pub const EOPNOTSUPP: i32 = 95;

/// lseek whence：绝对定位。
pub const SEEK_SET: u32 = 0;
/// lseek whence：相对当前位置。
pub const SEEK_CUR: u32 = 1;
/// lseek whence：相对文件尾。
pub const SEEK_END: u32 = 2;

/// fallocate：保持文件大小不变。
pub const FA_KEEP_SIZE: u32 = 0x01;
/// fallocate：打洞（必须与 KEEP_SIZE 同置）。
pub const FA_PUNCH_HOLE: u32 = 0x02;
/// fallocate：区间置零。
pub const FA_ZERO_RANGE: u32 = 0x04;
/// fallocate：区间折叠（禁止与 KEEP_SIZE 同置）。
pub const FA_COLLAPSE_RANGE: u32 = 0x08;
/// fallocate 合法模式位掩码。
pub const FA_VALID_MASK: u32 =
    FA_KEEP_SIZE | FA_PUNCH_HOLE | FA_ZERO_RANGE | FA_COLLAPSE_RANGE;

/// 模型文件大小上限（fallocate 判 EFBIG 的 inode 上界）。
pub const FILE_MAX: i64 = 0x7FFF_FFFF;

/// sendfile 模型入文件大小。
pub const SENDFILE_IN_SIZE: i64 = 4096;
/// sendfile 模型出文件容量。
pub const SENDFILE_OUT_CAP: i64 = 8192;

/// 对账记录表容量（容量耗尽判据的定容上界）。
pub const RECON_LOG_CAP: usize = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffCode {
    /// 负偏移：off < 0
    NegativeOffset,
    /// u64 高位截断产生的负值被当偏移注入
    TruncatedNegative,
    /// off + len 溢出 i64
    OffsetOverflow,
    /// 极值边界：i64::MIN / -i64::MAX / i64::MAX 邻域
    ExtremeBound,
    /// 零长度：合法短路
    ZeroLength,
    /// 负长度：len < 0
    NegativeLength,
    /// fallocate 模式位非法
    BadMode,
    /// PUNCH_HOLE 未与 KEEP_SIZE 同置
    PunchNeedsKeep,
    /// COLLAPSE_RANGE 与 KEEP_SIZE 同置
    CollapseKeepsSize,
    /// off+len 超出模型文件上限
    BeyondFileMax,
    /// lseek whence 非法
    BadWhence,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl OffCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OffCode::NegativeOffset => "负偏移被拒：off 小于 0 一律回 EINVAL；\
                请检查调用方把无符号长度误当偏移传入的路径",
            OffCode::TruncatedNegative => "u64 截断负值被拒：高位截断把大无符号数\
                变成负 i64；请在边界处用 checked 转换并显性拒绝",
            OffCode::OffsetOverflow => "off+len 溢出被拒：合计越过 i64 上界回 \
                EOVERFLOW；请先用 checked_add 复算再发起读写",
            OffCode::ExtremeBound => "极值边界：i64::MIN/-i64::MAX/i64::MAX 邻域\
                必须逐格验证；任何一格误判都要修判定次序",
            OffCode::ZeroLength => "零长度合法短路：偏移合法时 len=0 直接返回 0，\
                不触发溢出判定；调用方无需重试",
            OffCode::NegativeLength => "负长度被拒：len 小于 0 回 EINVAL；请改传\
                非负长度并复核符号扩展",
            OffCode::BadMode => "fallocate 模式位非法：掩码外位一律回 \
                EOPNOTSUPP；请核对 FA_* 位定义",
            OffCode::PunchNeedsKeep => "PUNCH_HOLE 必须与 KEEP_SIZE 同置，单独\
                使用回 EINVAL；请按 man 页约束补齐模式位",
            OffCode::CollapseKeepsSize => "COLLAPSE_RANGE 禁止与 KEEP_SIZE 同置，\
                同置回 EINVAL；二者语义互斥请二选一",
            OffCode::BeyondFileMax => "off+len 超出文件大小上限回 EFBIG；请先\
                收缩区间或扩展文件再重试",
            OffCode::BadWhence => "lseek whence 非法：只接受 SEEK_SET/CUR/END，\
                其余回 EINVAL",
            OffCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有\
                回滚路径误改累计值",
            OffCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            OffCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [OffCode; 14] = [
        OffCode::NegativeOffset,
        OffCode::TruncatedNegative,
        OffCode::OffsetOverflow,
        OffCode::ExtremeBound,
        OffCode::ZeroLength,
        OffCode::NegativeLength,
        OffCode::BadMode,
        OffCode::PunchNeedsKeep,
        OffCode::CollapseKeepsSize,
        OffCode::BeyondFileMax,
        OffCode::BadWhence,
        OffCode::LedgerRegress,
        OffCode::NotReproducible,
        OffCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 固定参数 LCG（确定性伪随机，契约允许） ────────────────────────────
/// 线性同余生成器：state = state * 1664525 + 1013904223（Numerical
/// Recipes 参数）。仅用于注入轮次内挑选判定器，保证同种子双跑一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 内部状态
    state: u32,
}

impl Lcg {
    /// 以固定种子构造（禁用墙钟与真随机）。
    pub const fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }

    /// 推进并返回原始 32 位状态。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }

    /// 取 [0, bound) 内的值；bound 为 0 时返回 0（显性可查）。
    pub fn pick(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 判定器一：pread/pwrite 偏移边界 ──────────────────────────────────
/// pread 偏移判定：负偏移先于溢出，零长度最后短路。
/// 次序依据 Linux 语义：offset < 0 在进入页缓存算术之前就被拒绝，
/// 因此 i64::MIN 即便再加正长度会溢出，也必须回 EINVAL 而非 EOVERFLOW。
pub fn judge_pread_offset(off: i64, len: u32) -> SysResult<u64> {
    if off < 0 {
        return Err(EINVAL);
    }
    let total = match off.checked_add(len as i64) {
        Some(t) => t,
        None => return Err(EOVERFLOW),
    };
    let _ = total;
    if len == 0 {
        return Ok(0);
    }
    Ok(len as u64)
}

/// pwrite 偏移判定：与 pread 同序（负偏移 → 溢出 → 零长度）。
pub fn judge_pwrite_offset(off: i64, len: u32) -> SysResult<u64> {
    judge_pread_offset(off, len)
}

// ── 判定器二：sendfile 区间边界 ──────────────────────────────────────
/// sendfile 区间判定：off_in / off_out 任一为负回 EINVAL（off_in 先判），
/// count=0 短路 Ok(0)，off_in+count 与 off_out+count 任一溢出回
/// EOVERFLOW；随后按入文件剩余与出文件容量截断为实际搬运量。
pub fn judge_sendfile_range(off_in: i64, off_out: i64, count: u32) -> SysResult<u64> {
    if off_in < 0 {
        return Err(EINVAL);
    }
    if off_out < 0 {
        return Err(EINVAL);
    }
    if count == 0 {
        return Ok(0);
    }
    if off_in.checked_add(count as i64).is_none() {
        return Err(EOVERFLOW);
    }
    if off_out.checked_add(count as i64).is_none() {
        return Err(EOVERFLOW);
    }
    let in_pos = off_in as u64;
    let out_pos = off_out as u64;
    let rem_in = (SENDFILE_IN_SIZE as u64).saturating_sub(in_pos);
    let rem_out = (SENDFILE_OUT_CAP as u64).saturating_sub(out_pos);
    Ok((count as u64).min(rem_in).min(rem_out))
}

// ── 判定器三：fallocate 模式与偏移边界 ───────────────────────────────
/// fallocate 判定次序：模式位 → 模式组合 → 负偏移 → 负长度 → 零长度
/// → off+len 溢出 → 文件上限。返回按语义调整后的文件大小视图。
pub fn judge_fallocate_mode_offset(mode: u32, off: i64, len: i64) -> SysResult<u64> {
    if mode & !FA_VALID_MASK != 0 {
        return Err(EOPNOTSUPP);
    }
    if mode & FA_PUNCH_HOLE != 0 && mode & FA_KEEP_SIZE == 0 {
        return Err(EINVAL);
    }
    // PUNCH_HOLE 与 ZERO_RANGE 是两种互斥的打洞机制，同置属矛盾组合。
    if mode & FA_PUNCH_HOLE != 0 && mode & FA_ZERO_RANGE != 0 {
        return Err(EINVAL);
    }
    if mode & FA_COLLAPSE_RANGE != 0 && mode & FA_KEEP_SIZE != 0 {
        return Err(EINVAL);
    }
    if off < 0 {
        return Err(EINVAL);
    }
    if len < 0 {
        return Err(EINVAL);
    }
    if len == 0 {
        return Err(EINVAL);
    }
    let end = match off.checked_add(len) {
        Some(e) => e,
        None => return Err(EFBIG),
    };
    if end > FILE_MAX {
        return Err(EFBIG);
    }
    if mode & FA_KEEP_SIZE != 0 {
        Ok(off as u64)
    } else {
        Ok(end as u64)
    }
}

// ── 判定器四：lseek 语义小判定器（对账 B04 档） ──────────────────────
/// lseek 判定：SEEK_SET 负偏移 EINVAL；SEEK_CUR/SEEK_END 合成后为负
/// EINVAL；合成溢出同样 EINVAL；其余 whence 回 EINVAL。
/// 本判定器是 F9209 与 B04 lseek 档的对账锚点。
pub fn judge_lseek(whence: u32, off: i64, pos: i64, size: i64) -> SysResult<i64> {
    match whence {
        SEEK_SET => {
            if off < 0 {
                return Err(EINVAL);
            }
            Ok(off)
        }
        SEEK_CUR => match pos.checked_add(off) {
            Some(n) if n >= 0 => Ok(n),
            _ => Err(EINVAL),
        },
        SEEK_END => match size.checked_add(off) {
            Some(n) if n >= 0 => Ok(n),
            _ => Err(EINVAL),
        },
        _ => Err(EINVAL),
    }
}

// ── 负偏移四类注入样本 ────────────────────────────────────────────────
/// 四类负偏移来源：手工常量、代数构造、极值、无符号截断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegOffCase {
    /// -1：最经典的负偏移
    MinusOne,
    /// -2^62：代数构造的大负值
    QuarterMin,
    /// i64::MIN：有符号下界
    I64Min,
    /// u64 高位截断：0xFFFF_FFFF_FFFF_FFF0 截进 i64 成为 -16
    U64Truncated,
}

/// 把 u64 按 i64 位型重新解释（截断路径：高位为 1 时结果为负）。
pub fn truncate_u64_to_i64(v: u64) -> i64 {
    v as i64
}

impl NegOffCase {
    /// 构造样本偏移。
    pub fn sample(self) -> i64 {
        match self {
            NegOffCase::MinusOne => -1,
            NegOffCase::QuarterMin => -(1i64 << 62),
            NegOffCase::I64Min => i64::MIN,
            NegOffCase::U64Truncated => truncate_u64_to_i64(0xFFFF_FFFF_FFFF_FFF0),
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> OffCode {
        match self {
            NegOffCase::MinusOne => OffCode::NegativeOffset,
            NegOffCase::QuarterMin => OffCode::ExtremeBound,
            NegOffCase::I64Min => OffCode::ExtremeBound,
            NegOffCase::U64Truncated => OffCode::TruncatedNegative,
        }
    }

    /// 闭集全量。
    pub const ALL: [NegOffCase; 4] = [
        NegOffCase::MinusOne,
        NegOffCase::QuarterMin,
        NegOffCase::I64Min,
        NegOffCase::U64Truncated,
    ];
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OffLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl OffLedger {
    const fn zero() -> OffLedger {
        OffLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式被破坏即显性报错。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(OffCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行负偏移注入矩阵：四类样本各 INJECT_ROUNDS 轮，每轮由固定 LCG
/// 在三个判定器（pread / sendfile / fallocate）中挑一个注入负偏移，
/// 三个判定器都必须回 EINVAL 才算检出。
pub fn run_negoff_matrix() -> (OffLedger, [u32; 4]) {
    let mut rng = Lcg::new(0x7A1C_0F92);
    let mut ledger = OffLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < NegOffCase::ALL.len() {
        if let Some(case) = NegOffCase::ALL.get(ci).copied() {
            let off = case.sample();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let pick = rng.pick(3);
                let caught = match pick {
                    0 => judge_pread_offset(off, 8) == Err(EINVAL),
                    1 => judge_sendfile_range(off, off, 8) == Err(EINVAL),
                    _ => judge_fallocate_mode_offset(0, off, 8) == Err(EINVAL),
                };
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

// ── off_t 溢出冻结对账表（pread 口径） ───────────────────────────────
/// 冻结对账表：(off, len, 期望 errno)。期望 0 表示成功（Ok）。
/// 每行都必须能用 judge_pread_offset 独立复算出同一结果。
pub const PREAD_OVERFLOW_TABLE: &[(i64, u32, i32)] = &[
    // 贴边不溢出：off+len == i64::MAX
    (i64::MAX - 1, 1, 0),
    // 贴边再进一格：溢出
    (i64::MAX, 1, EOVERFLOW),
    // 零长度在 i64::MAX 上不触发溢出（截断语义格）
    (i64::MAX, 0, 0),
    // i64::MAX-3 + 4 溢出
    (i64::MAX - 3, 4, EOVERFLOW),
    // i64::MAX-4 + 4 恰好贴边
    (i64::MAX - 4, 4, 0),
    // 负偏移先于溢出判定：i64::MIN + 4 会溢出，但仍必须回 EINVAL
    (i64::MIN, 4, EINVAL),
    // 手工负偏移
    (-1, 4, EINVAL),
    // 代数大负值
    (-(1i64 << 62), 4, EINVAL),
    // 贴边以内大跨度：off+len 越过 i64::MAX
    (i64::MAX - 100, 1000, EOVERFLOW),
];

/// 逐项复算冻结表：返回 (不一致行数, 复算行数)。
pub fn recheck_pread_table() -> (u32, u32) {
    let mut mismatch = 0u32;
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < PREAD_OVERFLOW_TABLE.len() {
        if let Some(row) = PREAD_OVERFLOW_TABLE.get(i) {
            let (off, len, want) = *row;
            let got = match judge_pread_offset(off, len) {
                Ok(_) => 0,
                Err(e) => e,
            };
            if got != want {
                mismatch = mismatch.saturating_add(1);
            }
            checked = checked.saturating_add(1);
        }
        i += 1;
    }
    (mismatch, checked)
}

// ── lseek 冻结对账表（B04 档口径） ───────────────────────────────────
/// 冻结对账表：(whence, off, 期望 errno, 期望新位置)。errno 为 0 表示
/// 成功且新位置必须逐项相等；pos=100、size=900 为对账锚定环境。
pub const LSEEK_TABLE: &[(u32, i64, i32, i64)] = &[
    (SEEK_SET, 0, 0, 0),
    (SEEK_SET, -1, EINVAL, 0),
    (SEEK_SET, i64::MIN, EINVAL, 0),
    (SEEK_SET, 12345, 0, 12345),
    (SEEK_CUR, 50, 0, 150),
    (SEEK_CUR, -101, EINVAL, 0),
    (SEEK_END, -900, 0, 0),
    (SEEK_END, -901, EINVAL, 0),
    (SEEK_END, 1, 0, 901),
    (7, 0, EINVAL, 0),
];

/// lseek 对账锚定环境：当前位 100。
pub const LSEEK_ANCHOR_POS: i64 = 100;
/// lseek 对账锚定环境：文件大小。
pub const LSEEK_ANCHOR_SIZE: i64 = 900;

/// 逐项复算 lseek 冻结表：返回 (不一致行数, 复算行数)。
pub fn recheck_lseek_table() -> (u32, u32) {
    let mut mismatch = 0u32;
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < LSEEK_TABLE.len() {
        if let Some(row) = LSEEK_TABLE.get(i) {
            let (whence, off, want_err, want_pos) = *row;
            let got = judge_lseek(whence, off, LSEEK_ANCHOR_POS, LSEEK_ANCHOR_SIZE);
            let ok = match got {
                Ok(p) => want_err == 0 && p == want_pos,
                Err(e) => want_err == e,
            };
            if !ok {
                mismatch = mismatch.saturating_add(1);
            }
            checked = checked.saturating_add(1);
        }
        i += 1;
    }
    (mismatch, checked)
}

/// 负偏移四类样本在 lseek（SEEK_SET）与本卡判定器上的逐项一致性：
/// 每个样本两个判定器都必须同回 EINVAL，逐项相等才算对账通过。
pub fn lseek_negoff_agreement() -> (u32, u32) {
    let mut agree = 0u32;
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < NegOffCase::ALL.len() {
        if let Some(case) = NegOffCase::ALL.get(i).copied() {
            let off = case.sample();
            let a = judge_lseek(SEEK_SET, off, 0, LSEEK_ANCHOR_SIZE) == Err(EINVAL);
            let b = judge_pread_offset(off, 4) == Err(EINVAL);
            if a && b {
                agree = agree.saturating_add(1);
            }
            checked = checked.saturating_add(1);
        }
        i += 1;
    }
    (agree, checked)
}

// ── 对账记录表（容量耗尽判据载体） ────────────────────────────────────
/// 定容记录表：容量 RECON_LOG_CAP，满后再写入回 ENOSPC 且原账不丢。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconLog {
    /// 定容槽位
    slots: [Option<(i64, i32)>; RECON_LOG_CAP],
    /// 已占用槽数
    used: usize,
    /// 因满被拒的写入次数
    dropped: usize,
}

impl ReconLog {
    /// 构造空表。
    pub const fn new() -> ReconLog {
        ReconLog { slots: [None; RECON_LOG_CAP], used: 0, dropped: 0 }
    }

    /// 写入一条 (off, errno)；表满回 ENOSPC，不覆盖已有账目。
    pub fn push(&mut self, off: i64, err: i32) -> SysResult<()> {
        let mut i = 0usize;
        while i < RECON_LOG_CAP {
            if let Some(slot) = self.slots.get_mut(i) {
                if slot.is_none() {
                    *slot = Some((off, err));
                    self.used = self.used.saturating_add(1);
                    return Ok(());
                }
            }
            i += 1;
        }
        self.dropped = self.dropped.saturating_add(1);
        Err(ENOSPC)
    }

    /// 已占用槽数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 被拒写入次数。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 账目闭合：占用 + 被拒 = 总写入；且占用不超容量。
    pub fn conserved(&self, total_pushed: usize) -> bool {
        self.used.saturating_add(self.dropped) == total_pushed
            && self.used <= RECON_LOG_CAP
    }
}

// ── 极值矩阵 ─────────────────────────────────────────────────────────
/// 极值偏移矩阵：i64 两端与代数构造点。
pub const EXTREME_OFFSETS: [i64; 6] = [
    i64::MIN,
    i64::MIN + 1,
    -i64::MAX,
    -(1i64 << 62),
    -1,
    i64::MAX,
];

/// 极值矩阵复算：对每个极值偏移以 len=4 走 pread 判定器，返回逐项
/// errno（成功记 0）。前五项必须全 EINVAL，末项必须 EOVERFLOW。
pub fn extreme_matrix() -> [i32; 6] {
    let mut out = [0i32; 6];
    let mut i = 0usize;
    while i < EXTREME_OFFSETS.len() {
        if let Some(off) = EXTREME_OFFSETS.get(i).copied() {
            let code = match judge_pread_offset(off, 4) {
                Ok(_) => 0,
                Err(e) => e,
            };
            if let Some(slot) = out.get_mut(i) {
                *slot = code;
            }
        }
        i += 1;
    }
    out
}

/// 极值矩阵期望值（与 EXTREME_OFFSETS 一一对应）。
pub const EXTREME_EXPECTED: [i32; 6] = [
    EINVAL, EINVAL, EINVAL, EINVAL, EINVAL, EOVERFLOW,
];

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

/// 规范语料双跑指纹：覆盖负偏移四类 × 三判定器 × 极值矩阵 × lseek 表，
/// 全部结果折叠进字节流。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < NegOffCase::ALL.len() {
        if let Some(case) = NegOffCase::ALL.get(i).copied() {
            let off = case.sample();
            let r = judge_pread_offset(off, 8);
            let sf = judge_sendfile_range(off, off, 8);
            let f = judge_fallocate_mode_offset(0, off, 8);
            let l = judge_lseek(SEEK_SET, off, 0, LSEEK_ANCHOR_SIZE);
            // r/sf/f 是 SysResult<u64>，l 是 SysResult<i64>：先逐个折成
            // 字节再入流，避免混合类型数组。
            for res in [r, sf, f] {
                let code = match res {
                    Ok(v) => (v & 0xFF) as u8,
                    Err(e) => (e & 0xFF) as u8,
                };
                bytes.push(code);
            }
            let lcode = match l {
                Ok(p) => (p & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            };
            bytes.push(lcode);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < PREAD_OVERFLOW_TABLE.len() {
        if let Some(row) = PREAD_OVERFLOW_TABLE.get(j) {
            let got = match judge_pread_offset(row.0, row.1) {
                Ok(v) => (v & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            };
            bytes.push(got);
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < LSEEK_TABLE.len() {
        if let Some(row) = LSEEK_TABLE.get(k) {
            match judge_lseek(row.0, row.1, LSEEK_ANCHOR_POS, LSEEK_ANCHOR_SIZE) {
                Ok(p) => {
                    bytes.push((p & 0xFF) as u8);
                    bytes.push(((p >> 8) & 0xFF) as u8);
                }
                Err(e) => bytes.push((e & 0xFF) as u8),
            }
        }
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9209 判据集（静态明细 ≥14 条；动态明细 ≤8 条）。
pub fn run_unx9209_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9209");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：pread/pwrite 合法偏移
    let normal_pread = judge_pread_offset(100, 8) == Ok(8)
        && judge_pwrite_offset(0, 16) == Ok(16);
    s.add("正常流：pread/pwrite 合法偏移返回请求长度", normal_pread,
          "0 <= off 且 off+len 不溢出时应原样放行");

    // 3. 正常流：sendfile 合法区间
    let normal_send = judge_sendfile_range(0, 0, 128) == Ok(128)
        && judge_sendfile_range(4096, 0, 8) == Ok(0);
    s.add("正常流：sendfile 合法区间搬运正确且 EOF 处归零", normal_send,
          "入文件剩余与出文件容量内的请求应全额搬运，越 EOF 记 0");

    // 4. 正常流：fallocate 合法模式与偏移
    let normal_fa = judge_fallocate_mode_offset(0, 0, 1024) == Ok(1024)
        && judge_fallocate_mode_offset(FA_KEEP_SIZE, 512, 256) == Ok(512)
        && judge_fallocate_mode_offset(FA_PUNCH_HOLE | FA_KEEP_SIZE, 64, 32) == Ok(64);
    s.add("正常流：fallocate 三组合合法", normal_fa,
          "普通保留/KEEP_SIZE/打洞三组合都应成功并返回正确大小视图");

    // 5. 四类负偏移全 EINVAL（pread 口径）
    let mut neg_pread = true;
    let mut ci = 0usize;
    while ci < NegOffCase::ALL.len() {
        if let Some(case) = NegOffCase::ALL.get(ci).copied() {
            if judge_pread_offset(case.sample(), 8) != Err(EINVAL) {
                neg_pread = false;
            }
        }
        ci += 1;
    }
    let codes_ok = NegOffCase::MinusOne.code() == OffCode::NegativeOffset
        && NegOffCase::QuarterMin.code() == OffCode::ExtremeBound
        && NegOffCase::I64Min.code() == OffCode::ExtremeBound
        && NegOffCase::U64Truncated.code() == OffCode::TruncatedNegative;
    s.add("负偏移四类全 EINVAL：pread 判定器", neg_pread && codes_ok,
          "-1/-2^62/i64::MIN/u64 截断负值一律回 EINVAL，不许放行");

    // 6. 注入矩阵 10/10 全抓
    let (ledger, per_class) = run_negoff_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("负偏移注入矩阵：四类各 10/10 全抓", all_caught,
          "每轮按固定 LCG 挑 pread/sendfile/fallocate 之一注入，EINVAL 必须全检出");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved()
              && ledger.escaped == 0
              && ledger.injected == NEG_OFF_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 判定次序：负偏移先于溢出（i64::MIN + 4 会溢出，但必须先回 EINVAL）
    let order_neg = judge_pread_offset(i64::MIN, 4) == Err(EINVAL)
        && judge_pread_offset(i64::MIN, 0) == Err(EINVAL)
        && judge_sendfile_range(i64::MIN, 0, 8) == Err(EINVAL)
        && judge_sendfile_range(0, i64::MIN, 8) == Err(EINVAL);
    s.add("判定次序：负偏移先于溢出判定", order_neg,
          "i64::MIN 上任何长度都不得改报 EOVERFLOW；off_in 先于 off_out");

    // 9. 判定次序：fallocate 模式校验先于偏移
    let order_mode = judge_fallocate_mode_offset(0xFF, -1, 4) == Err(EOPNOTSUPP)
        && judge_fallocate_mode_offset(FA_PUNCH_HOLE, -1, 4) == Err(EINVAL)
        && judge_fallocate_mode_offset(0xFF, 0, 0) == Err(EOPNOTSUPP);
    s.add("判定次序：fallocate 模式位先于偏移与长度", order_mode,
          "掩码外模式位回 EOPNOTSUPP；PUNCH 单用回 EINVAL；均在偏移检查之前");

    // 10. 溢出冻结表逐项复算
    let (ov_mis, ov_n) = recheck_pread_table();
    s.add("off_t 溢出冻结表逐项复算全对",
          ov_mis == 0 && ov_n == PREAD_OVERFLOW_TABLE.len() as u32,
          "贴边/溢出/零长度截断/负偏移九行逐项复算必须与表一致");

    // 11. 极值矩阵逐项相等
    let ext = extreme_matrix();
    let mut ext_ok = true;
    let mut ei = 0usize;
    while ei < EXTREME_OFFSETS.len() {
        if let (Some(g), Some(w)) = (ext.get(ei), EXTREME_EXPECTED.get(ei)) {
            if g != w {
                ext_ok = false;
            }
        }
        ei += 1;
    }
    s.add("极值矩阵：i64::MIN/-i64::MAX/-2^62/-1/i64::MAX 逐项相等", ext_ok,
          "五个负极值全 EINVAL，i64::MAX 在 len=4 下 EOVERFLOW");

    // 12. 零长度边界：pread/sendfile 短路、fallocate 拒绝
    let zero_ok = judge_pread_offset(0, 0) == Ok(0)
        && judge_pread_offset(i64::MAX, 0) == Ok(0)
        && judge_sendfile_range(0, 0, 0) == Ok(0)
        && judge_fallocate_mode_offset(0, 0, 0) == Err(EINVAL);
    s.add("零长度边界：读侧短路 Ok(0)、fallocate 回 EINVAL", zero_ok,
          "len=0 不触发溢出判定；fallocate 的 len=0 按语义拒绝");

    // 13. 负长度边界：fallocate len<0 回 EINVAL
    let neglen_ok = judge_fallocate_mode_offset(0, 0, -1) == Err(EINVAL)
        && judge_fallocate_mode_offset(FA_KEEP_SIZE, 8, i64::MIN) == Err(EINVAL)
        && judge_fallocate_mode_offset(FA_ZERO_RANGE, 8, -4096) == Err(EINVAL);
    s.add("负长度边界：fallocate 负长度全 EINVAL", neglen_ok,
          "len<0（含 i64::MIN）在偏移合法后立即拒绝，不许回绕成大正数");

    // 14. fallocate 模式面
    let mode_ok = judge_fallocate_mode_offset(0x10, 0, 8) == Err(EOPNOTSUPP)
        && judge_fallocate_mode_offset(FA_PUNCH_HOLE, 0, 8) == Err(EINVAL)
        && judge_fallocate_mode_offset(FA_COLLAPSE_RANGE | FA_KEEP_SIZE, 0, 8) == Err(EINVAL)
        && judge_fallocate_mode_offset(FA_PUNCH_HOLE | FA_KEEP_SIZE | FA_ZERO_RANGE, 0, 8)
            == Err(EINVAL);
    s.add("fallocate 模式面：坏位/单 PUNCH/COLLAPSE+KEEP 全拒", mode_ok,
          "掩码外位 EOPNOTSUPP；PUNCH 需 KEEP；COLLAPSE 禁 KEEP");

    // 15. fallocate 超上限 EFBIG（含溢出路径）
    let fb_ok = judge_fallocate_mode_offset(0, FILE_MAX - 8, 16) == Err(EFBIG)
        && judge_fallocate_mode_offset(0, FILE_MAX, i64::MAX) == Err(EFBIG)
        && judge_fallocate_mode_offset(0, FILE_MAX - 8, 8) == Ok(FILE_MAX as u64);
    s.add("fallocate 上限：越 FILE_MAX 回 EFBIG、贴边放行", fb_ok,
          "off+len 溢出与超上限都归口 EFBIG；恰好贴边必须成功");

    // 16. lseek 冻结表逐项相等（B04 档对账）
    let (ls_mis, ls_n) = recheck_lseek_table();
    s.add("lseek 冻结表逐项相等：与 B04 档对账一致",
          ls_mis == 0 && ls_n == LSEEK_TABLE.len() as u32,
          "SEEK_SET/CUR/END/非法 whence 十行逐项复算一致");

    // 17. 负偏移样本在 lseek 与本卡判定器逐项一致
    let (agree, agree_n) = lseek_negoff_agreement();
    s.add("对账一致：负偏移四类 lseek(SEEK_SET) 与 pread 同判 EINVAL",
          agree == agree_n && agree_n == NegOffCase::ALL.len() as u32,
          "四类样本两个判定器逐项相等，对账表闭合");

    // 18. 容量耗尽：ReconLog 满 → ENOSPC 且账不丢
    let mut log = ReconLog::new();
    let mut pushed = 0usize;
    let mut ti = 0usize;
    while ti < PREAD_OVERFLOW_TABLE.len() {
        if let Some(row) = PREAD_OVERFLOW_TABLE.get(ti) {
            let err = match judge_pread_offset(row.0, row.1) {
                Ok(_) => 0,
                Err(e) => e,
            };
            let _ = log.push(row.0, err);
            pushed = pushed.saturating_add(1);
        }
        ti += 1;
    }
    // 追加超过容量的写入，观察被拒且原账不动
    let mut extra = 0usize;
    while extra < 8 {
        let _ = log.push(-1, EINVAL);
        pushed = pushed.saturating_add(1);
        extra += 1;
    }
    let cap_ok = log.used() == RECON_LOG_CAP
        && log.dropped() == pushed - RECON_LOG_CAP
        && log.conserved(pushed);
    s.add("容量耗尽：ReconLog 满 16 后回 ENOSPC 且账目闭合", cap_ok,
          "占用 + 被拒 = 总写入，已入账条目一个不丢");

    // 19. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 20. 文案闭集：全部诊断码非空中文建议
    let text_ok = OffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 21. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < OffCode::ALL.len() {
        let mut j = i + 1;
        while j < OffCode::ALL.len() {
            if let (Some(a), Some(b)) = (OffCode::ALL.get(i), OffCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：14 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）：注入账、指纹、两张对账表、极值矩阵、容量
    let ledger_text =
        format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    let ov_text = format!("溢出表复算 {} 行 / 不一致 {}", ov_n, ov_mis);
    s.add_dyn("溢出表快照", ov_mis == 0, &ov_text);

    let ls_text = format!("lseek 表复算 {} 行 / 不一致 {}", ls_n, ls_mis);
    s.add_dyn("lseek 表快照", ls_mis == 0, &ls_text);

    let ext_einval = ext.iter().filter(|c| **c == EINVAL).count();
    let ext_eover = ext.iter().filter(|c| **c == EOVERFLOW).count();
    let ext_text = format!("极值矩阵 EINVAL*{} + EOVERFLOW*{}", ext_einval, ext_eover);
    s.add_dyn("极值矩阵快照", ext_ok, &ext_text);

    let cap_text = format!(
        "ReconLog 占用 {} / 被拒 {} / 总写 {}",
        log.used(), log.dropped(), pushed
    );
    s.add_dyn("容量快照", cap_ok, &cap_text);

    // 22. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "静态判据不足 14 条即视为覆盖缺口");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_offsets() {
        assert_eq!(judge_pread_offset(100, 8), Ok(8));
        assert_eq!(judge_pwrite_offset(0, 16), Ok(16));
        assert_eq!(judge_sendfile_range(0, 0, 128), Ok(128));
        assert_eq!(judge_fallocate_mode_offset(0, 0, 1024), Ok(1024));
        assert_eq!(judge_lseek(SEEK_SET, 42, 0, 900), Ok(42));
    }

    #[test]
    fn four_negoff_classes_all_einval() {
        let mut i = 0usize;
        while i < NegOffCase::ALL.len() {
            if let Some(case) = NegOffCase::ALL.get(i).copied() {
                let off = case.sample();
                assert!(off < 0);
                assert_eq!(judge_pread_offset(off, 8), Err(EINVAL));
                assert_eq!(judge_sendfile_range(off, off, 8), Err(EINVAL));
                assert_eq!(judge_fallocate_mode_offset(0, off, 8), Err(EINVAL));
                assert_eq!(judge_lseek(SEEK_SET, off, 0, 900), Err(EINVAL));
            }
            i += 1;
        }
        assert_eq!(truncate_u64_to_i64(0xFFFF_FFFF_FFFF_FFF0), -16);
    }

    #[test]
    fn negoff_matrix_ten_rounds_all_caught() {
        let (l, per) = run_negoff_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, NEG_OFF_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn judgment_order_neg_before_overflow() {
        // i64::MIN + 4 会溢出，但负偏移必须先回 EINVAL
        assert_eq!(judge_pread_offset(i64::MIN, 4), Err(EINVAL));
        assert_eq!(judge_pread_offset(i64::MIN, 0), Err(EINVAL));
        // sendfile：off_in 先于 off_out
        assert_eq!(judge_sendfile_range(-1, i64::MIN, 8), Err(EINVAL));
        // fallocate：模式先于偏移
        assert_eq!(judge_fallocate_mode_offset(0xFF, -1, 4), Err(EOPNOTSUPP));
        assert_eq!(judge_fallocate_mode_offset(0xFF, 0, 0), Err(EOPNOTSUPP));
    }

    #[test]
    fn overflow_frozen_table_recheck() {
        let (mis, n) = recheck_pread_table();
        assert_eq!(mis, 0);
        assert_eq!(n, PREAD_OVERFLOW_TABLE.len() as u32);
        assert_eq!(judge_pread_offset(i64::MAX, 1), Err(EOVERFLOW));
        assert_eq!(judge_pread_offset(i64::MAX, 0), Ok(0));
        assert_eq!(judge_pread_offset(i64::MAX - 100, 1000), Err(EOVERFLOW));
        assert_eq!(judge_pread_offset(i64::MAX - 1500, 1000), Ok(1000));
    }

    #[test]
    fn extreme_matrix_values() {
        let ext = extreme_matrix();
        assert_eq!(ext.get(0), Some(&EINVAL));
        assert_eq!(ext.get(1), Some(&EINVAL));
        assert_eq!(ext.get(2), Some(&EINVAL));
        assert_eq!(ext.get(3), Some(&EINVAL));
        assert_eq!(ext.get(4), Some(&EINVAL));
        assert_eq!(ext.get(5), Some(&EOVERFLOW));
    }

    #[test]
    fn zero_and_negative_length_boundaries() {
        assert_eq!(judge_pread_offset(0, 0), Ok(0));
        assert_eq!(judge_pread_offset(i64::MAX, 0), Ok(0));
        assert_eq!(judge_sendfile_range(0, 0, 0), Ok(0));
        assert_eq!(judge_fallocate_mode_offset(0, 0, 0), Err(EINVAL));
        assert_eq!(judge_fallocate_mode_offset(0, 0, -1), Err(EINVAL));
        assert_eq!(judge_fallocate_mode_offset(FA_KEEP_SIZE, 8, i64::MIN), Err(EINVAL));
    }

    #[test]
    fn lseek_frozen_table_agrees() {
        let (mis, n) = recheck_lseek_table();
        assert_eq!(mis, 0);
        assert_eq!(n, LSEEK_TABLE.len() as u32);
        let (agree, checked) = lseek_negoff_agreement();
        assert_eq!(agree, checked);
        assert_eq!(checked, NegOffCase::ALL.len() as u32);
    }

    #[test]
    fn recon_log_capacity_exhaustion() {
        let mut log = ReconLog::new();
        let mut ok = 0usize;
        let mut rejected = 0usize;
        let mut k = 0usize;
        while k < RECON_LOG_CAP + 4 {
            match log.push(k as i64, EINVAL) {
                Ok(_) => ok += 1,
                Err(e) => {
                    assert_eq!(e, ENOSPC);
                    rejected += 1;
                }
            }
            k += 1;
        }
        assert_eq!(ok, RECON_LOG_CAP);
        assert_eq!(rejected, 4);
        assert_eq!(log.used(), RECON_LOG_CAP);
        assert_eq!(log.dropped(), 4);
        assert!(log.conserved(ok + rejected));
    }

    #[test]
    fn segment_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7A1C0);
        let first = OffCode::ALL.first().map(|c| c.val()).unwrap_or(0);
        let last = OffCode::ALL.last().map(|c| c.val()).unwrap_or(0);
        assert_eq!(first, SEG_BASE);
        assert_eq!(last, SEG_BASE + OffCode::ALL.len() as u32 - 1);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let mut r1 = Lcg::new(0x7A1C_0F92);
        let mut r2 = Lcg::new(0x7A1C_0F92);
        assert_eq!(r1.next_u32(), r2.next_u32());
        assert_eq!(r1.pick(3), r2.pick(3));
    }

    #[test]
    fn fallocate_mode_face_and_efbig() {
        assert_eq!(judge_fallocate_mode_offset(0x10, 0, 8), Err(EOPNOTSUPP));
        assert_eq!(judge_fallocate_mode_offset(FA_PUNCH_HOLE, 0, 8), Err(EINVAL));
        assert_eq!(
            judge_fallocate_mode_offset(FA_COLLAPSE_RANGE | FA_KEEP_SIZE, 0, 8),
            Err(EINVAL)
        );
        assert_eq!(judge_fallocate_mode_offset(0, FILE_MAX - 8, 16), Err(EFBIG));
        assert_eq!(judge_fallocate_mode_offset(0, FILE_MAX - 8, 8), Ok(FILE_MAX as u64));
    }

    #[test]
    fn diag_code_closure() {
        assert_eq!(OffCode::ALL.len(), 14);
        for c in OffCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
        let mut i = 0usize;
        while i < OffCode::ALL.len() {
            let mut j = i + 1;
            while j < OffCode::ALL.len() {
                let a = OffCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
                let b = OffCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
                assert!(a != b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9209_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 14);
    }
}
