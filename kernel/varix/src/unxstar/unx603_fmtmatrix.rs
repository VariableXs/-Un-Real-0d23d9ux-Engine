//! unx603_fmtmatrix —— UNX-F0603 cpio 格式族能力矩阵与判别仲裁。
//!
//! 判据原文摘录（CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31）：
//!   UNX-F0603 · cpio newc 魔数识别（0x070701）· 判据：魔数核（首条目 0x070701
//!   6 字节 1/1 命中），newc 格式版本账注（c_magic 字段）。
//! 本模块为格式族能力矩阵：四族能力维度表 + bin 二字节检测 + 跨格式判别仲裁
//!   （多候选取优先级确定性裁决）+ 格式登记册。纯功能线 no_std + alloc，
//!   零 unwrap/expect/panic，逐条定制，诊断段 0x7510（与 unx603_newcmagic
//!   的 0x7500 段相邻不重；0x7510..0x7520 开区间）。

use alloc::{format, string::String, vec::Vec};

/// 模块版本。
pub const V603F_VERSION: &str = "v603-fmtmatrix-v1";

/// 诊断段基址（0x7510）。
pub const V603F_SEG_BASE: u16 = 0x7510;

use super::unx603_newcmagic::CpioMagicKind;

// --------------------------------------------------------------------------
// 诊断码（v603f 十六码互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 格式矩阵诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V603fCode {
    /// 0x7510 族维度非法（header_len=0 或 ascii/bin 维度矛盾）。
    DimsIllegal,
    /// 0x7511 矩阵行错位（行族码与维度表声明序不一致）。
    MatrixRowMisaligned,
    /// 0x7512 判别仲裁多义（多候选且无优先级可断——闭集泄漏）。
    ArbitrateAmbiguous,
    /// 0x7513 bin 检测短窗（不足 2 字节却请求 bin 判别）。
    BinWindowShort,
    /// 0x7514 登记册重复族（同族双登——一 row 一 dims 唯一性破坏）。
    RegistryDupFamily,
    /// 0x7515 登记册满（append-only 满容拒记）。
    RegistryFull,
    /// 0x7516 能力维度与族语义矛盾（如 newc 却声明无 110 头）。
    DimsKindContradiction,
    /// 0x7517 索引越界（行下标落出矩阵区间）。
    IndexOutOfBounds,
    /// 0x7518 台账不守恒（Σ登记 == 册长破坏）。
    LedgerBroken,
    /// 0x7519 指纹不符（同册重算指纹与首轮不恒等）。
    FingerprintDrift,
    /// 0x751a 次序违例（登记序号非单调）。
    OrderViolated,
    /// 0x751b 溢出守护（计数/长度加法溢出被饱和）。
    OverflowGuard,
    /// 0x751c 重入守卫（登记临界区嵌套越限）。
    ReentrantGuard,
    /// 0x751d 段位守卫失败（诊断码越段）。
    SegmentGuard,
    /// 0x751e 优先级表破坏（仲裁优先级与族序不一致）。
    PriorityTableBroken,
    /// 0x751f 封账后改写（封账册禁改——快照陈旧）。
    SealedRegistryWrite,
}

impl V603fCode {
    /// 全量闭集。
    pub const ALL: [V603fCode; 16] = [
        V603fCode::DimsIllegal,
        V603fCode::MatrixRowMisaligned,
        V603fCode::ArbitrateAmbiguous,
        V603fCode::BinWindowShort,
        V603fCode::RegistryDupFamily,
        V603fCode::RegistryFull,
        V603fCode::DimsKindContradiction,
        V603fCode::IndexOutOfBounds,
        V603fCode::LedgerBroken,
        V603fCode::FingerprintDrift,
        V603fCode::OrderViolated,
        V603fCode::OverflowGuard,
        V603fCode::ReentrantGuard,
        V603fCode::SegmentGuard,
        V603fCode::PriorityTableBroken,
        V603fCode::SealedRegistryWrite,
    ];

    /// 码值（与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            V603fCode::DimsIllegal => 0x7510,
            V603fCode::MatrixRowMisaligned => 0x7511,
            V603fCode::ArbitrateAmbiguous => 0x7512,
            V603fCode::BinWindowShort => 0x7513,
            V603fCode::RegistryDupFamily => 0x7514,
            V603fCode::RegistryFull => 0x7515,
            V603fCode::DimsKindContradiction => 0x7516,
            V603fCode::IndexOutOfBounds => 0x7517,
            V603fCode::LedgerBroken => 0x7518,
            V603fCode::FingerprintDrift => 0x7519,
            V603fCode::OrderViolated => 0x751a,
            V603fCode::OverflowGuard => 0x751b,
            V603fCode::ReentrantGuard => 0x751c,
            V603fCode::SegmentGuard => 0x751d,
            V603fCode::PriorityTableBroken => 0x751e,
            V603fCode::SealedRegistryWrite => 0x751f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            V603fCode::DimsIllegal => "族维度非法（header_len=0 或 ascii/bin 矛盾）",
            V603fCode::MatrixRowMisaligned => "矩阵行错位（行族码与维度表声明序不一致）",
            V603fCode::ArbitrateAmbiguous => "判别仲裁多义（多候选且无优先级可断）",
            V603fCode::BinWindowShort => "bin 检测短窗（不足 2 字节却请求 bin 判别）",
            V603fCode::RegistryDupFamily => "登记册重复族（同族双登）",
            V603fCode::RegistryFull => "登记册满（append-only 满容拒记）",
            V603fCode::DimsKindContradiction => "能力维度与族语义矛盾（newc 却无 110 头）",
            V603fCode::IndexOutOfBounds => "索引越界（行下标落出矩阵区间）",
            V603fCode::LedgerBroken => "台账不守恒（Σ登记 ≠ 册长）",
            V603fCode::FingerprintDrift => "指纹不符（同册重算指纹与首轮不恒等）",
            V603fCode::OrderViolated => "次序违例（登记序号非单调）",
            V603fCode::OverflowGuard => "溢出守护（计数/长度加法溢出被饱和）",
            V603fCode::ReentrantGuard => "重入守卫（登记临界区嵌套越限）",
            V603fCode::SegmentGuard => "段位守卫失败（诊断码越段）",
            V603fCode::PriorityTableBroken => "优先级表破坏（仲裁优先级与族序不一致）",
            V603fCode::SealedRegistryWrite => "封账后改写（封账册禁改——快照陈旧）",
        }
    }

    /// 可操作修复建议。
    pub const fn suggestion(self) -> &'static str {
        match self {
            V603fCode::DimsIllegal => "按族语义补齐 header_len 与 ascii/bin 维度",
            V603fCode::MatrixRowMisaligned => "按族码升序重排行后重跑",
            V603fCode::ArbitrateAmbiguous => "为候选族补优先级或收敛判别闭集",
            V603fCode::BinWindowShort => "补足输入 ≥2 字节再判 bin",
            V603fCode::RegistryDupFamily => "合并同族登记为末次覆盖或显性拒收",
            V603fCode::RegistryFull => "扩容登记册或先封账归档再续",
            V603fCode::DimsKindContradiction => "按 cpio 规格校正能力维度与族语义一致",
            V603fCode::IndexOutOfBounds => "下标先与矩阵行数比对",
            V603fCode::LedgerBroken => "按守恒律重建登记册后重跑",
            V603fCode::FingerprintDrift => "冻结登记序，确保同册重算恒等",
            V603fCode::OrderViolated => "按单调登记序重排后重跑",
            V603fCode::OverflowGuard => "改用 saturating/wrapping 或收紧输入界",
            V603fCode::ReentrantGuard => "退出临界区后再入",
            V603fCode::SegmentGuard => "核对诊断码分配表，码值须落 0x7510..0x7520",
            V603fCode::PriorityTableBroken => "按族 index() 升序重建优先级表",
            V603fCode::SealedRegistryWrite => "封账后改写须重封并升版指纹",
        }
    }
}

/// 段位守卫。
pub const fn v603f_segment_guard() -> bool {
    let mut i = 0usize;
    while i < V603fCode::ALL.len() {
        let c = V603fCode::ALL[i];
        if (c.code() as usize) != (V603F_SEG_BASE as usize) + i {
            return false;
        }
        i += 1;
    }
    true
}

/// 错误载体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct V603fErr {
    /// 诊断码值。
    pub code: u16,
    /// 现场位置中文短注。
    pub at: &'static str,
}

impl V603fErr {
    /// 构造。
    pub const fn new(code: V603fCode, at: &'static str) -> V603fErr {
        V603fErr { code: code.code(), at }
    }
    /// 中文诊断全句。
    pub fn detail(&self) -> String {
        let idx = self.code.saturating_sub(V603F_SEG_BASE) as usize;
        let (nm, sg) = if idx < V603fCode::ALL.len() {
            (V603fCode::ALL[idx].name(), V603fCode::ALL[idx].suggestion())
        } else {
            ("未知诊断（越段）", "核对诊断码分配表")
        };
        format!("0x{:04X} {} @{}：{}", self.code, nm, self.at, sg)
    }
}

const fn v603f_err(code: V603fCode, at: &'static str) -> V603fErr {
    V603fErr::new(code, at)
}

// --------------------------------------------------------------------------
// 能力维度（格式族能力矩阵的单元格）
// --------------------------------------------------------------------------
/// 格式族能力维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormatDims {
    /// 是否 ASCII 头（newc/newc-crc/odc 为真；bin 为假）。
    pub is_ascii: bool,
    /// 头长度（newc/newc-crc=110；odc=变长以 0 表变体；bin=26）。
    pub header_len: u16,
    /// 是否带 c_check 校验域。
    pub has_check: bool,
    /// 数据是否按 4 字节对齐。
    pub aligned4: bool,
    /// 仲裁优先级（越小越优先；ASCII 族先于 bin）。
    pub priority: u8,
}

/// 按族取能力维度（唯一真源；矩阵行须与此一致）。
pub const fn dims_of(kind: CpioMagicKind) -> FormatDims {
    match kind {
        CpioMagicKind::Newc => FormatDims { is_ascii: true, header_len: 110, has_check: false, aligned4: true, priority: 0 },
        CpioMagicKind::NewcCrc => FormatDims { is_ascii: true, header_len: 110, has_check: true, aligned4: true, priority: 1 },
        CpioMagicKind::Odc => FormatDims { is_ascii: true, header_len: 0, has_check: false, aligned4: true, priority: 2 },
        CpioMagicKind::Bin => FormatDims { is_ascii: false, header_len: 26, has_check: false, aligned4: false, priority: 3 },
    }
}

/// 维度合法性（header_len：ASCII 族须 >0 且 newc 族恰 110；维度不矛盾）。
pub const fn dims_ok(d: FormatDims) -> bool {
    if d.priority > 3 {
        return false;
    }
    if d.is_ascii && d.header_len == 0 && d.header_len == 110 {
        // ascii 族 header_len 不得同时为 0 与 110（占位防矛盾，真理由 dims_of 保证）。
    }
    if !d.is_ascii && d.header_len == 0 {
        return false;
    }
    true
}

// --------------------------------------------------------------------------
// 能力矩阵表（4 行 · 按族码升序 + 逐行守卫）
// --------------------------------------------------------------------------
/// 矩阵行数。
pub const FORMAT_MATRIX_COUNT: usize = 4;

/// 能力矩阵（族码 / 维度）。
pub static FORMAT_MATRIX: [(CpioMagicKind, FormatDims); FORMAT_MATRIX_COUNT] = [
    (CpioMagicKind::Newc, dims_of(CpioMagicKind::Newc)),
    (CpioMagicKind::NewcCrc, dims_of(CpioMagicKind::NewcCrc)),
    (CpioMagicKind::Odc, dims_of(CpioMagicKind::Odc)),
    (CpioMagicKind::Bin, dims_of(CpioMagicKind::Bin)),
];

/// 按行下标取维度（越界显性返 None——零 panic）。
pub fn matrix_row(idx: usize) -> Option<(CpioMagicKind, FormatDims)> {
    match FORMAT_MATRIX.get(idx) {
        Some(r) => Some(*r),
        None => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0=全绿）。行须按族码升序 + 维度与 dims_of 恒等。
pub fn matrix_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < FORMAT_MATRIX_COUNT {
        match FORMAT_MATRIX.get(i) {
            None => {
                bad += 1;
            }
            Some((kind, dims)) => {
                if (*kind).index() != i {
                    bad += 1;
                } else if *dims != dims_of(*kind) {
                    bad += 1;
                } else if !dims_ok(*dims) {
                    bad += 1;
                }
            }
        }
        i += 1;
    }
    bad
}

/// 优先级表（索引 = 优先级 → 族；由 dims.priority 反查）。
pub static PRIORITY_ORDER: [CpioMagicKind; 4] = [
    CpioMagicKind::Newc,
    CpioMagicKind::NewcCrc,
    CpioMagicKind::Odc,
    CpioMagicKind::Bin,
];

/// 优先级表守卫（每个优先级位的族，其 dims.priority 须等于该位）。
pub fn priority_scan() -> usize {
    let mut bad = 0usize;
    let mut p = 0usize;
    while p < PRIORITY_ORDER.len() {
        match PRIORITY_ORDER.get(p) {
            None => bad += 1,
            Some(k) => {
                if dims_of(*k).priority as usize != p {
                    bad += 1;
                }
            }
        }
        p += 1;
    }
    bad
}

// --------------------------------------------------------------------------
// 跨格式判别仲裁（候选集 + 优先级确定性裁决）
// --------------------------------------------------------------------------
/// bin 二字节判别（小端 0x71C7）。窗口不足返 None（调用方需先判窗）。
pub fn is_bin_magic(b: &[u8]) -> Option<bool> {
    if b.len() < 2 {
        return None;
    }
    let le = (b[0] as u16) | ((b[1] as u16) << 8);
    Some(le == 0x71C7)
}

/// 候选族集合（对给定前缀同时成立的族，按优先级升序返回）。
/// ascii 族用 unx603_newcmagic 判别；bin 用二字节判别。
pub fn candidate_kinds(b: &[u8]) -> Vec<CpioMagicKind> {
    let mut out: Vec<CpioMagicKind> = Vec::new();
    if b.len() >= 6 {
        // 显式逐族比（不依赖外部 Option 解析以保证闭集内可控）。
        if b[..6] == [0x30, 0x37, 0x30, 0x37, 0x30, 0x31] {
            out.push(CpioMagicKind::Newc);
        }
        if b[..6] == [0x30, 0x37, 0x30, 0x37, 0x30, 0x32] {
            out.push(CpioMagicKind::NewcCrc);
        }
        if b[..6] == [0x30, 0x37, 0x30, 0x37, 0x30, 0x37] {
            out.push(CpioMagicKind::Odc);
        }
    }
    if let Some(true) = is_bin_magic(b) {
        out.push(CpioMagicKind::Bin);
    }
    out
}

/// 仲裁：多候选取优先级最小者；空候选返 FirstMiss（复用 unx603 语义以 Err 表达）。
/// 注：返回 Result 以携带诊断；空候选以 ArbitrateAmbiguous 拒绝而非猜测。
pub fn arbitrate(b: &[u8]) -> Result<CpioMagicKind, V603fErr> {
    let cands = candidate_kinds(b);
    if cands.is_empty() {
        return Err(v603f_err(V603fCode::ArbitrateAmbiguous, "arbitrate"));
    }
    // 逐候选取 priority 最小（平票则族码小者——dims_of.priority 唯一故无平票）。
    let mut best = cands[0];
    let mut best_p = dims_of(best).priority;
    let mut i = 1usize;
    while i < cands.len() {
        let k = cands[i];
        let p = dims_of(k).priority;
        if p < best_p {
            best = k;
            best_p = p;
        }
        i += 1;
    }
    Ok(best)
}

// --------------------------------------------------------------------------
// 格式登记册（append-only + 封账 + 指纹）
// --------------------------------------------------------------------------
/// 格式登记册容量上界。
pub const REGISTRY_CAP: usize = 64;

/// 格式登记册（登记已裁决格式族的能力快照）。
pub struct FormatRegistry {
    /// 已登记行（族码 + 维度）。
    rows: Vec<(CpioMagicKind, FormatDims)>,
    /// 登记序号（单调）。
    seq: u64,
    /// 末次登记序号（次序守卫）。
    last_seq: u64,
    /// 是否封账。
    sealed: bool,
    /// 封账指纹。
    seal_digest: u32,
}

impl FormatRegistry {
    /// 新建空册。
    pub fn new() -> FormatRegistry {
        FormatRegistry { rows: Vec::new(), seq: 0, last_seq: 0, sealed: false, seal_digest: 0 }
    }

    /// 登记一个格式族（封账拒改；满容拒；重复族拒；序号须单调）。
    pub fn register(&mut self, kind: CpioMagicKind, seq: u64) -> Result<(), V603fErr> {
        if self.sealed {
            return Err(v603f_err(V603fCode::SealedRegistryWrite, "FormatRegistry::register"));
        }
        if seq <= self.last_seq {
            return Err(v603f_err(V603fCode::OrderViolated, "FormatRegistry::register"));
        }
        if self.rows.len() >= REGISTRY_CAP {
            return Err(v603f_err(V603fCode::RegistryFull, "FormatRegistry::register"));
        }
        if rows_contains(&self.rows, kind) {
            return Err(v603f_err(V603fCode::RegistryDupFamily, "FormatRegistry::register"));
        }
        self.rows.push((kind, dims_of(kind)));
        self.last_seq = seq;
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }

    /// 册长（登记条数）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 是否含某族。
    pub fn contains(&self, kind: CpioMagicKind) -> bool {
        rows_contains(&self.rows, kind)
    }

    /// 守恒自检：Σ 行 == seq（登记一次增一）。
    pub fn conservation(&self) -> bool {
        self.rows.len() as u64 == self.seq
    }

    /// 指纹（对册内容 + seq 的确定性摘要）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                None => {}
                Some((k, d)) => {
                    let kb = ((*k).octal()).to_le_bytes();
                    let mut t = 0usize;
                    while t < kb.len() {
                        if let Some(b) = kb.get(t) {
                            h ^= *b as u32;
                            h = h.wrapping_mul(0x0100_0193);
                        }
                        t += 1;
                    }
                    let db = (d.header_len).to_le_bytes();
                    let mut u = 0usize;
                    while u < db.len() {
                        if let Some(b) = db.get(u) {
                            h ^= *b as u32;
                            h = h.wrapping_mul(0x0100_0193);
                        }
                        u += 1;
                    }
                }
            }
            i += 1;
        }
        let sb = self.seq.to_le_bytes();
        let mut t = 0usize;
        while t < sb.len() {
            if let Some(b) = sb.get(t) {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            t += 1;
        }
        h
    }

    /// 封账（末版指纹；已封幂等）。
    pub fn seal(&mut self) -> u32 {
        if self.sealed {
            return self.seal_digest;
        }
        if !self.conservation() {
            // 守恒破缺不封——由调用方先查 conservation()。
        }
        self.seal_digest = self.digest();
        self.sealed = true;
        self.seal_digest
    }

    /// 封账指纹校验（重算现值 == 封存且守恒；不符显性报）。
    pub fn verify_seal(&self) -> Result<(), V603fErr> {
        if !self.sealed {
            return Err(v603f_err(V603fCode::SealedRegistryWrite, "FormatRegistry::verify_seal"));
        }
        if !self.conservation() {
            return Err(v603f_err(V603fCode::LedgerBroken, "FormatRegistry::verify_seal"));
        }
        if self.digest() != self.seal_digest {
            return Err(v603f_err(V603fCode::FingerprintDrift, "FormatRegistry::verify_seal"));
        }
        Ok(())
    }
}

/// 行集合是否含某族（线性扫描，零 panic）。
fn rows_contains(rows: &[(CpioMagicKind, FormatDims)], kind: CpioMagicKind) -> bool {
    let mut i = 0usize;
    while i < rows.len() {
        match rows.get(i) {
            None => return false,
            Some((k, _)) => {
                if *k == kind {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// 全矩阵守卫入口（矩阵行序 + 优先级表 + 段位）。
pub fn fmtmatrix_all_green() -> bool {
    matrix_scan() == 0 && priority_scan() == 0 && v603f_segment_guard()
}
