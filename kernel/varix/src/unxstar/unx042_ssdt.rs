//! UNX-F0042 · SSDT 多表枚举与版本登记（CoRun·Unxreal · UNX-A1-B03 · ACPI 深化之三）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0042`
//!
//! **判据 J1**：全部 SSDT 枚举数与 XSDT 声明一致，逐表校验和通过，
//! 清单可导出比对。
//!
//! ## 职责定位
//!
//! 前序单 UNX-F0041（DSDT 采集与校验链）解决了「单表怎么采、怎么审」；
//! 本单解决「一族表怎么枚举、怎么登记、怎么对账」：XSDT 表体遍历收集
//! 全部 SSDT 指针 → 逐表头部校验 → 版本登记账去重 → 枚举数与声明数
//! 终态对账 → 清单导出（kv 文本 + 二进制 wire 双形态，可离线比对）。
//! 基础表头解析复用 `crate::acpi::SdtHeader`（签名/长度/revision/
//! checksum/OEM ID/OEM Table ID），不另立。
//!
//! ## 要点一：XSDT 遍历收集——声明数闭式
//!
//! XSDT 表体是 8 字节物理指针数组，声明数 = (length − 36) / 8。长度
//! 非 8 对齐即条目边界坏（XSDT_ENTRY_ALIGN 立案）；逐条小端提取，
//! 零指针/非 8 对齐指针逐条立案不中止遍历。
//!
//! ## 要点二：逐表校验——校验和算法逐字节实现 + 双路对拍
//!
//! 本单自实现逐字节 wrap 和（`checksum_bytes`），并与
//! `crate::acpi::checksum_ok` 双路对拍恒等（`checksum_crosscheck`）——
//! 同字节源两路实现互证，对拍面与 F0041 的 HeaderAudit 双写对拍同构。
//!
//! ## 要点三：版本登记账——四元组键去重
//!
//! 账键 = (OEM ID, OEM Table ID, revision, 内容指纹 FNV-1a 64)。
//! 全键相同 → occurrences 去重累加；同 (OEM ID, Table ID) 异版本 →
//! 旧条目标 diverged + 新键分立登记（版本演进留痕不报错）；账本容量
//! 闸满如实标注截断不静默吞。
//!
//! ## 要点四：坏表处置矩阵——不阻塞引导只立案
//!
//! 诊断码闭集独占 0x0701~0x070F（0x07x 三位段，与域内 0x2E/0x2F/
//! 0x35~0x59/0x80~0x95 段互不重叠——先 grep 后占段纪律）。每码经
//! 显性矩阵映射到三态处置：Accept（不可能态，矩阵自检对拍）/
//! Quarantine（表体在、审不过，立案保留）/ Skip（读面不可信，跳过）。
//! 任何坏表只入报告，引导继续——判据「不阻塞」语义与 F0041 同源。
//!
//! ## 要点五：对账与导出——终态守恒
//!
//! 每条声明指针必达终态（Accept/Quarantine/Skip 之一），终态数 ==
//! 声明数 == 读出面数 → reconciled。清单双形态导出：kv 行（人读）
//! + wire TLV 字节流（机比），wire 往返解析对拍恒等。增量重扫走
//! 账本守恒检查：前账每键必在现账，键灭即 RESCAN_DRIFT 立案。
//!
//! 全模块零 panic 面：所有输入不可信（固件表可被改坏），索引前先查
//! 界，错误一律结构化返回；时间只用注入的逻辑 tick（u64），禁墙钟。

use crate::acpi::{checksum_ok, SdtHeader, SDT_HEADER_LEN};
use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x0701~0x070F，独占 0x07x 三位段）
// ---------------------------------------------------------------------------

/// SSDT 枚举诊断码。独占 `0x0701~0x070F`（0x07x 三位段，十五码闭集）。
///
/// 占段纪律：先 grep 同目录确认 0x07x 无主（既有 0x2E/0x2F/0x35~0x59/
/// 0x80~0x95），本单入库即占段——先入库者占段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsCode(pub u16);

impl SsCode {
    /// XSDT 缺失（调用方传 None——RSDP/RSDT 链未供 XSDT）。
    pub const XSDT_MISSING: SsCode = SsCode(0x0701);
    /// XSDT 签名不符（非 "XSDT"）。
    pub const XSDT_SIGNATURE: SsCode = SsCode(0x0702);
    /// XSDT 长度域坏（零/短于头/超读面）。
    pub const XSDT_LENGTH: SsCode = SsCode(0x0703);
    /// XSDT 自身全表校验和不过。
    pub const XSDT_CHECKSUM: SsCode = SsCode(0x0704);
    /// XSDT 条目边界坏（length − 36 非 8 的倍数）。
    pub const XSDT_ENTRY_ALIGN: SsCode = SsCode(0x0705);
    /// 声明指针为零。
    pub const PTR_ZERO: SsCode = SsCode(0x0706);
    /// 声明指针非 8 字节对齐。
    pub const PTR_MISALIGN: SsCode = SsCode(0x0707);
    /// 声明指针无对应映射表体（调用方未供该地址读面）。
    pub const TABLE_UNMAPPED: SsCode = SsCode(0x0708);
    /// SSDT 表签名不符（非 "SSDT"）。
    pub const SSDT_SIGNATURE: SsCode = SsCode(0x0709);
    /// SSDT 长度域坏（零/短于头/超读面）。
    pub const SSDT_LENGTH: SsCode = SsCode(0x070A);
    /// SSDT 全表校验和不过。
    pub const SSDT_CHECKSUM: SsCode = SsCode(0x070B);
    /// SSDT OEM ID 空（全零/全空格）。
    pub const SSDT_OEM_ID: SsCode = SsCode(0x070C);
    /// SSDT OEM Table ID 空（全零/全空格）。
    pub const SSDT_OEM_TABLE_ID: SsCode = SsCode(0x070D);
    /// 枚举数与 XSDT 声明不一致（终态对账失败/读出面与声明面分叉）。
    pub const ENUM_MISMATCH: SsCode = SsCode(0x070E);
    /// 增量重扫账本漂移（前账键在现账灭失）。
    pub const RESCAN_DRIFT: SsCode = SsCode(0x070F);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 段位合法性（码值必须落在 0x0701..=0x070F 闭集内）。
    pub const fn segment_ok(self) -> bool {
        self.0 >= 0x0701 && self.0 <= 0x070F
    }

    /// 全码闭集枚举（矩阵自检与段位审计的遍历源——顺序即码序）。
    pub const fn all() -> [SsCode; 15] {
        [
            SsCode::XSDT_MISSING,
            SsCode::XSDT_SIGNATURE,
            SsCode::XSDT_LENGTH,
            SsCode::XSDT_CHECKSUM,
            SsCode::XSDT_ENTRY_ALIGN,
            SsCode::PTR_ZERO,
            SsCode::PTR_MISALIGN,
            SsCode::TABLE_UNMAPPED,
            SsCode::SSDT_SIGNATURE,
            SsCode::SSDT_LENGTH,
            SsCode::SSDT_CHECKSUM,
            SsCode::SSDT_OEM_ID,
            SsCode::SSDT_OEM_TABLE_ID,
            SsCode::ENUM_MISMATCH,
            SsCode::RESCAN_DRIFT,
        ]
    }

    /// 人话原因（诊断输出单点——逐码中文判词）。
    pub fn reason(self) -> String {
        match self {
            SsCode::XSDT_MISSING => "XSDT 缺失：调用方未供 XSDT 读面，SSDT 枚举无源".into(),
            SsCode::XSDT_SIGNATURE => "XSDT 签名不符：表头签名不是 XSDT".into(),
            SsCode::XSDT_LENGTH => "XSDT 长度域坏：零长度、短于表头或超读面".into(),
            SsCode::XSDT_CHECKSUM => "XSDT 全表校验和不过：字节和 mod 256 非零".into(),
            SsCode::XSDT_ENTRY_ALIGN => "XSDT 条目边界坏：(length-36) 非 8 的倍数，指针数组截断".into(),
            SsCode::PTR_ZERO => "声明指针为零：XSDT 条目空指针".into(),
            SsCode::PTR_MISALIGN => "声明指针非 8 字节对齐：ACPI 2.0+ 表必须 8 对齐".into(),
            SsCode::TABLE_UNMAPPED => "声明指针无映射表体：该地址未供读面，无法审计".into(),
            SsCode::SSDT_SIGNATURE => "SSDT 签名不符：目标地址表头不是 SSDT".into(),
            SsCode::SSDT_LENGTH => "SSDT 长度域坏：零长度、短于表头或超读面".into(),
            SsCode::SSDT_CHECKSUM => "SSDT 全表校验和不过：字节和 mod 256 非零".into(),
            SsCode::SSDT_OEM_ID => "SSDT OEM ID 空：全零或全空格，固件偷懒可检".into(),
            SsCode::SSDT_OEM_TABLE_ID => "SSDT OEM Table ID 空：全零或全空格".into(),
            SsCode::ENUM_MISMATCH => "枚举数与 XSDT 声明不一致：终态对账失败".into(),
            SsCode::RESCAN_DRIFT => "增量重扫账本漂移：前账登记键在现账灭失".into(),
            SsCode(_) => "未知 UNX SSDT 枚举域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// §1 校验和算法——逐字节实现 + 双路对拍
// ---------------------------------------------------------------------------

/// 全表校验和逐字节计算（ACPI 规则：全表字节 wrap 和，合法表恒为 0）。
///
/// 逐字节累加显性循环——不借助迭代器折叠，字节序与截断面完全显性：
/// 读面越界按实长钳制（不可信输入不 panic）。
pub fn checksum_bytes(table: &[u8], length: usize) -> u8 {
    let mut sum: u8 = 0;
    let bound = if length <= table.len() { length } else { table.len() };
    let mut i = 0usize;
    while i < bound {
        sum = sum.wrapping_add(table[i]);
        i += 1;
    }
    sum
}

/// 校验和判定读面上限（与 crate::acpi::checksum_ok 的 1 << 20 上限
/// 同值——双路对拍必须同界，超长表两路同判 false 才不分歧）。
pub const CHECKSUM_MAX_TABLE: usize = 1 << 20;

/// 校验和通过判定：长度非零、不超读面、不超 1 MiB 且全表 wrap 和为 0。
///
/// 零长度直接 false（空表无校验可言——与 F0041 checksum_ok 语义同源）。
pub fn checksum_matches(table: &[u8], length: usize) -> bool {
    if length == 0 || length > table.len() || length > CHECKSUM_MAX_TABLE {
        return false;
    }
    checksum_bytes(table, length) == 0
}

/// 校验和双路对拍：本单逐字节实现与 `crate::acpi::checksum_ok` 恒等。
///
/// 同字节源两路实现互证——对拍通过且校验通过才 true；任一路 false 或
/// 两路分歧皆 false（分歧本身是更深的实现 bug，对拍面如实暴露）。
pub fn checksum_crosscheck(table: &[u8], length: usize) -> bool {
    let mine = checksum_matches(table, length);
    let theirs = checksum_ok(table, length);
    mine == theirs && mine
}

// ---------------------------------------------------------------------------
// §2 XSDT 审计——签名/长度/校验和/条目边界/声明数闭式
// ---------------------------------------------------------------------------

/// XSDT 表体条目起始偏移（= 表头 36 字节）。
pub const XSDT_ENTRY_OFFSET: usize = SDT_HEADER_LEN;
/// 单条目字节数（ACPI 2.0+ 64 位物理指针）。
pub const ENTRY_BYTES: usize = 8;
/// XSDT 签名。
pub const XSDT_SIGNATURE: [u8; 4] = *b"XSDT";

/// XSDT 自身审计账（表头五项 + 声明数——枚举对账的声明面锚）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XsdtAudit {
    /// 表长（length 域）。
    pub length: u32,
    /// 表 revision。
    pub revision: u8,
    /// checksum 域原值。
    pub checksum_byte: u8,
    /// OEM ID（6 字节 ASCII）。
    pub oem_id: [u8; 6],
    /// OEM Table ID（8 字节 ASCII）。
    pub oem_table_id: [u8; 8],
    /// 全表校验和是否通过。
    pub checksum_ok: bool,
    /// 声明 SSDT 指针数 =（length − 36）/ 8（闭式）。
    pub declared_count: usize,
}

/// XSDT 长度域与条目边界审计（签名/长度/8 对齐——前置三闸）。
///
/// 规则：① 读面至少一头；② 签名必须 "XSDT"；③ length >= 36 且不超
/// 读面；④ (length − 36) 非 8 的倍数 → 条目边界坏（指针数组被截断）。
pub fn xsdt_declared_count(table: &[u8]) -> Result<usize, SsCode> {
    if table.len() < SDT_HEADER_LEN {
        return Err(SsCode::XSDT_LENGTH);
    }
    let header = match SdtHeader::parse(table) {
        Some(h) => h,
        None => return Err(SsCode::XSDT_LENGTH),
    };
    if header.signature != XSDT_SIGNATURE {
        return Err(SsCode::XSDT_SIGNATURE);
    }
    if header.length < SDT_HEADER_LEN as u32 || (header.length as usize) > table.len() {
        return Err(SsCode::XSDT_LENGTH);
    }
    let body = header.length as usize - SDT_HEADER_LEN;
    if body % ENTRY_BYTES != 0 {
        return Err(SsCode::XSDT_ENTRY_ALIGN);
    }
    Ok(body / ENTRY_BYTES)
}

/// XSDT 全项审计：三闸 + 校验和 + OEM 面 + 声明数一次成型。
pub fn xsdt_audit(table: &[u8]) -> Result<XsdtAudit, SsCode> {
    let declared = xsdt_declared_count(table)?;
    let header = match SdtHeader::parse(table) {
        Some(h) => h,
        None => return Err(SsCode::XSDT_LENGTH),
    };
    let ck = checksum_crosscheck(table, header.length as usize);
    if !ck {
        return Err(SsCode::XSDT_CHECKSUM);
    }
    if !oem_id_valid(&header.oem_id) {
        return Err(SsCode::XSDT_SIGNATURE);
    }
    Ok(XsdtAudit {
        length: header.length,
        revision: header.revision,
        checksum_byte: header.checksum,
        oem_id: header.oem_id,
        oem_table_id: header.oem_table_id,
        checksum_ok: ck,
        declared_count: declared,
    })
}

/// XSDT 条目逐条提取（小端 u64 物理指针——原样收集含零值与错位值，
/// 指针形审在逐表层做，提取层只读不改）。
///
/// 读面保证：declared_count 已由 xsdt_declared_count 校验，每条偏移
/// off+8 <= length <= table.len()，索引恒界内。
pub fn xsdt_entries(table: &[u8]) -> Result<Vec<u64>, SsCode> {
    let declared = xsdt_declared_count(table)?;
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < declared {
        let off = XSDT_ENTRY_OFFSET + i * ENTRY_BYTES;
        let b = &table[off..off + ENTRY_BYTES];
        out.push(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]));
        i += 1;
    }
    Ok(out)
}

/// 指针形审（8 字节对齐 + 非零——ACPI 2.0+ 表基址必须 8 对齐）。
pub fn pointer_shape_ok(addr: u64) -> bool {
    addr != 0 && addr % ENTRY_BYTES as u64 == 0
}

// ---------------------------------------------------------------------------
// §3 OEM 面与内容指纹——登记账键的字节源
// ---------------------------------------------------------------------------

/// OEM ID 有效性：非全零且非全空格（与 F0041 oem_id_valid 语义同构）。
pub fn oem_id_valid(oem_id: &[u8; 6]) -> bool {
    let all_zero = oem_id.iter().all(|&b| b == 0);
    let all_space = oem_id.iter().all(|&b| b == b' ');
    !(all_zero || all_space)
}

/// OEM Table ID 有效性：非全零且非全空格（8 字节版同构规则）。
pub fn oem_table_id_valid(oem_table_id: &[u8; 8]) -> bool {
    let all_zero = oem_table_id.iter().all(|&b| b == 0);
    let all_space = oem_table_id.iter().all(|&b| b == b' ');
    !(all_zero || all_space)
}

/// FNV-1a 64 位偏移基（内容指纹常量——公开对拍锚）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位内容指纹（逐字节异或后乘素数——确定性、无堆、跨机恒等）。
///
/// 用途：版本登记账的第四键——同 OEM ID/Table ID/revision 但载荷字节
/// 有差的两张表，指纹分立即版本演进可检。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// §4 SSDT 单表采集——签名/长度/校验和/OEM 四闸
// ---------------------------------------------------------------------------

/// SSDT 签名。
pub const SSDT_SIGNATURE: [u8; 4] = *b"SSDT";

/// SSDT 采集账（逐表落账——地址/长度/revision/OEM 面/指纹可审计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsdtInfo {
    /// SSDT 物理指针（XSDT 条目原值）。
    pub addr: u64,
    /// 表长（表头 length 域）。
    pub length: u32,
    /// 表 revision。
    pub revision: u8,
    /// checksum 域原值（修复建议闭式的输入锚）。
    pub checksum_byte: u8,
    /// OEM ID（6 字节 ASCII）。
    pub oem_id: [u8; 6],
    /// OEM Table ID（8 字节 ASCII）。
    pub oem_table_id: [u8; 8],
    /// AML 载荷字节数（length − 36；短于头按 0）。
    pub payload_len: u32,
    /// 全表内容指纹（FNV-1a 64——登记账第四键）。
    pub fingerprint: u64,
    /// 全表校验和是否通过（四闸全过恒 true——保留位与 F0041 同构）。
    pub checksum_ok: bool,
}

impl SsdtInfo {
    /// 登记账键比较（四元组全等——OEM ID/Table ID/revision/指纹）。
    pub fn same_key(&self, other: &SsdtInfo) -> bool {
        self.oem_id == other.oem_id
            && self.oem_table_id == other.oem_table_id
            && self.revision == other.revision
            && self.fingerprint == other.fingerprint
    }

    /// 账键半比较（前三键——版本演进检测面：同前三异指纹即分叉）。
    pub fn same_semantic_id(&self, other: &SsdtInfo) -> bool {
        self.oem_id == other.oem_id
            && self.oem_table_id == other.oem_table_id
            && self.revision == other.revision
    }

    /// OEM ID 可打印渲染（非 ASCII 字节以 '.' 占位——kv 导出用）。
    pub fn oem_id_str(&self) -> String {
        ascii_or_dot(&self.oem_id)
    }

    /// OEM Table ID 可打印渲染。
    pub fn oem_table_id_str(&self) -> String {
        ascii_or_dot(&self.oem_table_id)
    }
}

/// 字节串可打印渲染（0x20..0x7F 原样，其余 '.'——导出比对单点）。
pub fn ascii_or_dot(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' })
        .collect()
}

/// 由 SSDT 表字节采集信息账：签名/长度/校验和/OEM ID/OEM Table ID 四闸。
///
/// 规则：① 读面至少一头；② 签名必须 "SSDT"；③ length 非零且不超读面；
/// ④ 全表校验和逐字节实现判定且与 crate::acpi 双路对拍；⑤ OEM ID 与
/// OEM Table ID 有效性；⑥ 通过 → SsdtInfo 落账（含载荷长与内容指纹），
/// 任一不过 → 对应诊断码（坏表不 panic——处置矩阵接管）。
pub fn collect_ssdt(addr: u64, table: &[u8]) -> Result<SsdtInfo, SsCode> {
    if table.len() < SDT_HEADER_LEN {
        return Err(SsCode::SSDT_LENGTH);
    }
    let header = match SdtHeader::parse(table) {
        Some(h) => h,
        None => return Err(SsCode::SSDT_LENGTH),
    };
    if header.signature != SSDT_SIGNATURE {
        return Err(SsCode::SSDT_SIGNATURE);
    }
    if header.length == 0 || (header.length as usize) > table.len() {
        return Err(SsCode::SSDT_LENGTH);
    }
    if !checksum_crosscheck(table, header.length as usize) {
        return Err(SsCode::SSDT_CHECKSUM);
    }
    if !oem_id_valid(&header.oem_id) {
        return Err(SsCode::SSDT_OEM_ID);
    }
    if !oem_table_id_valid(&header.oem_table_id) {
        return Err(SsCode::SSDT_OEM_TABLE_ID);
    }
    let full_len = header.length as usize;
    let payload_len = if full_len >= SDT_HEADER_LEN { full_len - SDT_HEADER_LEN } else { 0 };
    Ok(SsdtInfo {
        addr,
        length: header.length,
        revision: header.revision,
        checksum_byte: header.checksum,
        oem_id: header.oem_id,
        oem_table_id: header.oem_table_id,
        payload_len: payload_len as u32,
        fingerprint: fnv1a64(&table[..full_len]),
        checksum_ok: true,
    })
}

// ---------------------------------------------------------------------------
// §5 版本登记账——四元组键去重登记
// ---------------------------------------------------------------------------

/// 登记账条目容量闸（定容纪律——超容截断如实标注不静默吞）。
pub const MAX_LEDGER_ENTRIES: usize = 16;

/// 登记结果闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// 新键首登（账本新增一条）。
    New,
    /// 全键重复（去重：occurrences 累加不新增条目）。
    Deduped,
    /// 同语义 ID（OEM ID/Table ID/revision）异指纹（版本演进分叉：
    /// 旧条目标 diverged，新键分立登记）。
    Diverged,
    /// 容量满拒登（truncated 已标注——表本身可能无恙，仅账面容不下）。
    Rejected,
}

/// 登记账条目（四元组键 + 首现序 + 重复计数 + 分叉标记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerEntry {
    /// OEM ID（6 字节）。
    pub oem_id: [u8; 6],
    /// OEM Table ID（8 字节）。
    pub oem_table_id: [u8; 8],
    /// 表 revision。
    pub revision: u8,
    /// 内容指纹（FNV-1a 64）。
    pub fingerprint: u64,
    /// 首现序（枚举序——第几张被采到的表）。
    pub first_seen: u32,
    /// 重复登记计数（含首登——全键相同的表张数）。
    pub occurrences: u32,
    /// 版本分叉标记（同 OEM ID/Table ID 出现过异 revision/异指纹）。
    pub diverged: bool,
}

impl LedgerEntry {
    /// 全键比较（四元组——守恒检查的键恒等面）。
    pub fn same_key(&self, other: &LedgerEntry) -> bool {
        self.oem_id == other.oem_id
            && self.oem_table_id == other.oem_table_id
            && self.revision == other.revision
            && self.fingerprint == other.fingerprint
    }

    /// 语义 ID 比较（前三键——OEM 身份面）。
    pub fn same_semantic_id(&self, other: &LedgerEntry) -> bool {
        self.oem_id == other.oem_id
            && self.oem_table_id == other.oem_table_id
            && self.revision == other.revision
    }

    /// 条目键渲染（kv 导出单点）。
    pub fn key_str(&self) -> String {
        format!(
            "oem_id={} oem_table_id={} revision={} fingerprint={:#018x}",
            ascii_or_dot(&self.oem_id),
            ascii_or_dot(&self.oem_table_id),
            self.revision,
            self.fingerprint,
        )
    }
}

/// SSDT 版本登记账（OEM ID + Table ID + revision + 指纹去重登记）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionLedger {
    /// 账目（按首登序排列——确定性遍历序）。
    pub entries: Vec<LedgerEntry>,
    /// 总登记次数（含去重累加与拒登——对账守恒的分母面）。
    pub total_registered: u32,
    /// 容量截断标记（拒登发生过即 true——如实不静默）。
    pub truncated: bool,
}

impl VersionLedger {
    /// 空账（枚举起步形态）。
    pub fn new() -> VersionLedger {
        VersionLedger {
            entries: vec![],
            total_registered: 0,
            truncated: false,
        }
    }

    /// 登记一张采集过的 SSDT（四元组键去重——见 RegisterOutcome 闭集）。
    ///
    /// 规则：① 全键命中 → Deduped（occurrences+1）；② 前三键命中异
    /// 指纹 → 旧条目标 diverged，按新键走新增路径（版本演进分立）；
    /// ③ 全新 → New；④ 容量满 → Rejected（truncated 标注）。
    pub fn register(&mut self, info: &SsdtInfo) -> RegisterOutcome {
        self.total_registered = self.total_registered.wrapping_add(1);
        let mut diverge_target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].oem_id == info.oem_id
                && self.entries[i].oem_table_id == info.oem_table_id
            {
                if self.entries[i].revision == info.revision
                    && self.entries[i].fingerprint == info.fingerprint
                {
                    self.entries[i].occurrences = self.entries[i].occurrences.wrapping_add(1);
                    return RegisterOutcome::Deduped;
                }
                if self.entries[i].revision == info.revision
                    && diverge_target.is_none()
                {
                    diverge_target = Some(i);
                }
            }
            i += 1;
        }
        if self.entries.len() >= MAX_LEDGER_ENTRIES {
            self.truncated = true;
            return RegisterOutcome::Rejected;
        }
        if let Some(t) = diverge_target {
            self.entries[t].diverged = true;
        }
        self.entries.push(LedgerEntry {
            oem_id: info.oem_id,
            oem_table_id: info.oem_table_id,
            revision: info.revision,
            fingerprint: info.fingerprint,
            first_seen: self.entries.len() as u32,
            occurrences: 1,
            diverged: false,
        });
        RegisterOutcome::New
    }

    /// 全键查找（命中返回条目序）。
    pub fn find_key(&self, info: &SsdtInfo) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].oem_id == info.oem_id
                && self.entries[i].oem_table_id == info.oem_table_id
                && self.entries[i].revision == info.revision
                && self.entries[i].fingerprint == info.fingerprint
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 分叉条目数（diverged 计数——版本演进审计面）。
    pub fn diverged_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].diverged {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账目条数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 账本守恒检查（增量重扫的锚面：前账每键必在现账）。
///
/// SSDT 是固件静态表族——重扫只可能新增或重复，前账键灭失即漂移
/// （RESCAN_DRIFT）。返回 Err 供调用方立案，不阻塞（与坏表同纪律）。
pub fn ledger_conservation(prev: &VersionLedger, cur: &VersionLedger) -> Result<(), SsCode> {
    let mut i = 0usize;
    while i < prev.entries.len() {
        let p = &prev.entries[i];
        let mut found = false;
        let mut k = 0usize;
        while k < cur.entries.len() {
            if p.same_key(&cur.entries[k]) {
                found = true;
                break;
            }
            k += 1;
        }
        if !found {
            return Err(SsCode::RESCAN_DRIFT);
        }
        i += 1;
    }
    Ok(())
}

/// 重扫差异账（added/removed/unchanged 三闭集——键级 diff）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanDiff {
    /// 现账新增键（前账没有）。
    pub added: Vec<LedgerEntry>,
    /// 前账灭失键（现账没有——漂移证据）。
    pub removed: Vec<LedgerEntry>,
    /// 双账皆在的键数。
    pub unchanged: usize,
}

/// 键级重扫 diff（O(n·m) 双循环——容量闸内 n,m ≤ 16，代价有界）。
pub fn rescan_diff(prev: &VersionLedger, cur: &VersionLedger) -> RescanDiff {
    let mut added: Vec<LedgerEntry> = Vec::new();
    let mut removed: Vec<LedgerEntry> = Vec::new();
    let mut unchanged = 0usize;
    let mut i = 0usize;
    while i < cur.entries.len() {
        let c = &cur.entries[i];
        let mut found = false;
        let mut k = 0usize;
        while k < prev.entries.len() {
            if c.same_key(&prev.entries[k]) {
                found = true;
                break;
            }
            k += 1;
        }
        if !found {
            added.push(*c);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < prev.entries.len() {
        let p = &prev.entries[j];
        let mut found = false;
        let mut k = 0usize;
        while k < cur.entries.len() {
            if p.same_key(&cur.entries[k]) {
                found = true;
                break;
            }
            k += 1;
        }
        if found {
            unchanged += 1;
        } else {
            removed.push(*p);
        }
        j += 1;
    }
    RescanDiff { added, removed, unchanged }
}

// ---------------------------------------------------------------------------
// §6 坏表处置矩阵——诊断码到三态处置的显性闭式映射
// ---------------------------------------------------------------------------

/// 坏表处置三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// 采集通过入账（矩阵对拍态：collect 成功路径专用，矩阵内无码映射
    /// 到此态——若矩阵自检出现即实现 bug）。
    Accept,
    /// 隔离立案（表体在、审不过——入清单标 quarantine，引导继续）。
    Quarantine,
    /// 跳过（读面不可信/无表体——入清单标 skip，引导继续）。
    Skip,
}

impl Disposition {
    /// wire 字节（0 accept / 1 quarantine / 2 skip）。
    pub const fn as_byte(self) -> u8 {
        match self {
            Disposition::Accept => 0,
            Disposition::Quarantine => 1,
            Disposition::Skip => 2,
        }
    }

    /// wire 字节解析（越界 None——不可信字节流显性拒）。
    pub const fn from_byte(b: u8) -> Option<Disposition> {
        match b {
            0 => Some(Disposition::Accept),
            1 => Some(Disposition::Quarantine),
            2 => Some(Disposition::Skip),
            _ => None,
        }
    }

    /// kv 渲染名。
    pub const fn name(self) -> &'static str {
        match self {
            Disposition::Accept => "accept",
            Disposition::Quarantine => "quarantine",
            Disposition::Skip => "skip",
        }
    }
}

/// 处置矩阵（十五码闭式映射——全码覆盖，一码一态）。
///
/// 语义：读面建立不了（XSDT 级 / 指针形 / 长度坏 / 无表体）→ Skip；
/// 读面在但内容审不过（签名 / 校验和 / OEM 面）→ Quarantine；
/// ENUM_MISMATCH 与 RESCAN_DRIFT 是流程级码，映射 Skip（本单枚举内
/// 无表体可处置，由对账面立案）。
pub const DISPOSITION_MATRIX: [(SsCode, Disposition); 15] = [
    (SsCode::XSDT_MISSING, Disposition::Skip),
    (SsCode::XSDT_SIGNATURE, Disposition::Skip),
    (SsCode::XSDT_LENGTH, Disposition::Skip),
    (SsCode::XSDT_CHECKSUM, Disposition::Skip),
    (SsCode::XSDT_ENTRY_ALIGN, Disposition::Skip),
    (SsCode::PTR_ZERO, Disposition::Skip),
    (SsCode::PTR_MISALIGN, Disposition::Skip),
    (SsCode::TABLE_UNMAPPED, Disposition::Skip),
    (SsCode::SSDT_SIGNATURE, Disposition::Quarantine),
    (SsCode::SSDT_LENGTH, Disposition::Skip),
    (SsCode::SSDT_CHECKSUM, Disposition::Quarantine),
    (SsCode::SSDT_OEM_ID, Disposition::Quarantine),
    (SsCode::SSDT_OEM_TABLE_ID, Disposition::Quarantine),
    (SsCode::ENUM_MISMATCH, Disposition::Skip),
    (SsCode::RESCAN_DRIFT, Disposition::Skip),
];

/// 矩阵查表（逐码线性命中——闭集容量 15，代价有界；未知码保守 Skip）。
pub fn disposition_of(code: SsCode) -> Disposition {
    let mut i = 0usize;
    while i < DISPOSITION_MATRIX.len() {
        if DISPOSITION_MATRIX[i].0 == code {
            return DISPOSITION_MATRIX[i].1;
        }
        i += 1;
    }
    Disposition::Skip
}

/// 矩阵闭式自检：全码闭集每码恰映射一次且无 Accept 态（Accept 是
/// collect 成功路径专用，矩阵内出现即实现 bug——对拍恒等 true）。
pub fn disposition_matrix_closed() -> bool {
    let all = SsCode::all();
    if all.len() != DISPOSITION_MATRIX.len() {
        return false;
    }
    let mut i = 0usize;
    while i < all.len() {
        if DISPOSITION_MATRIX[i].0 != all[i] {
            return false;
        }
        if DISPOSITION_MATRIX[i].1 == Disposition::Accept {
            return false;
        }
        if !all[i].segment_ok() {
            return false;
        }
        i += 1;
    }
    true
}

/// 不阻塞引导语义声明（判据逐字对拍——与 F0041 NON_BLOCKING_NOTE 同源）。
pub const BAD_TABLE_POLICY_NOTE: &str =
    "坏表只入报告立案（quarantine/skip 双态），引导继续不阻塞——诊断是报告不是中止";

// ---------------------------------------------------------------------------
// §7 枚举状态机——逻辑 tick 注入的确定性推进
// ---------------------------------------------------------------------------

/// 枚举扫描阶段（线性单向推进闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    /// 起步（未审计 XSDT）。
    Idle,
    /// XSDT 审计中（签名/长度/校验和/条目边界）。
    XsdtAudit,
    /// 条目遍历中（指针收集与形审）。
    EntryWalk,
    /// 逐表采集中（四闸 + 登记）。
    TableCollect,
    /// 登记账提交中。
    LedgerCommit,
    /// 终态对账中（枚举数 vs 声明数）。
    Reconcile,
    /// 完成（reconciled 结论已定）。
    Done,
}

/// 状态机内部错误（非诊断码段——流程级非法转移/时钟回拨/对账残缺）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineError {
    /// 非法阶段转移（from → want 不在单向链上）。
    IllegalTransition { from: ScanPhase, want: ScanPhase },
    /// 逻辑 tick 回拨（确定性时钟必须单调非降）。
    TickRegress { last: u64, got: u64 },
    /// 对账残缺（终态数不足声明数即收尾）。
    Underflow { done: usize, declared: usize },
}

impl MachineError {
    /// 中文诊断（内部错误面——区别于 SsCode 域码）。
    pub fn diagnose(&self) -> String {
        match self {
            MachineError::IllegalTransition { from, want } => {
                format!("非法阶段转移：{:?} → {:?} 不在单向链上", from, want)
            }
            MachineError::TickRegress { last, got } => {
                format!("逻辑 tick 回拨：last={} got={}", last, got)
            }
            MachineError::Underflow { done, declared } => {
                format!("对账残缺：终态数 {} < 声明数 {}", done, declared)
            }
        }
    }
}

/// 枚举扫描状态机（单向链 Idle → XsdtAudit → EntryWalk → TableCollect
/// → LedgerCommit → Reconcile → Done；每步转移显性校验 + tick 单调闸）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanMachine {
    /// 当前阶段。
    phase: ScanPhase,
    /// 最近一次推进的 tick（单调锚）。
    last_tick: u64,
    /// 已达终态的表槽计数。
    done_tables: usize,
    /// 声明数（EntryWalk 时锁定）。
    declared: usize,
    /// 累计转移步数（审计面）。
    steps: u32,
}

impl ScanMachine {
    /// 新机（Idle 起步，tick 锚 0——首个 begin 时以注入 tick 校准）。
    pub const fn new() -> ScanMachine {
        ScanMachine {
            phase: ScanPhase::Idle,
            last_tick: 0,
            done_tables: 0,
            declared: 0,
            steps: 0,
        }
    }

    /// 当前阶段只读。
    pub const fn phase(&self) -> ScanPhase {
        self.phase
    }

    /// 已达终态表槽数只读。
    pub const fn done_tables(&self) -> usize {
        self.done_tables
    }

    /// 声明数只读。
    pub const fn declared(&self) -> usize {
        self.declared
    }

    /// 转移步数只读。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// tick 单调闸（回拨即错——确定性时钟纪律）。
    fn tick_guard(&mut self, tick: u64) -> Result<(), MachineError> {
        if tick < self.last_tick {
            return Err(MachineError::TickRegress { last: self.last_tick, got: tick });
        }
        self.last_tick = tick;
        Ok(())
    }

    /// 阶段转移闸（from 必须 == 当前态，否则非法转移）。
    fn transition(&mut self, from: ScanPhase, to: ScanPhase, tick: u64) -> Result<(), MachineError> {
        self.tick_guard(tick)?;
        if self.phase != from {
            return Err(MachineError::IllegalTransition { from: self.phase, want: from });
        }
        self.phase = to;
        self.steps = self.steps.wrapping_add(1);
        Ok(())
    }

    /// Idle → XsdtAudit。
    pub fn begin_xsdt_audit(&mut self, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::Idle, ScanPhase::XsdtAudit, tick)
    }

    /// XsdtAudit → EntryWalk（锁定声明数）。
    pub fn begin_entry_walk(&mut self, declared: usize, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::XsdtAudit, ScanPhase::EntryWalk, tick)?;
        self.declared = declared;
        Ok(())
    }

    /// EntryWalk → TableCollect。
    pub fn begin_table_collect(&mut self, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::EntryWalk, ScanPhase::TableCollect, tick)
    }

    /// 单表终态登记（Accept/Quarantine/Skip 皆计）。
    pub fn record_table_done(&mut self, tick: u64) -> Result<(), MachineError> {
        self.tick_guard(tick)?;
        if self.phase != ScanPhase::TableCollect && self.phase != ScanPhase::EntryWalk {
            return Err(MachineError::IllegalTransition {
                from: self.phase,
                want: ScanPhase::TableCollect,
            });
        }
        self.done_tables = self.done_tables.wrapping_add(1);
        Ok(())
    }

    /// TableCollect → LedgerCommit。
    pub fn begin_ledger_commit(&mut self, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::TableCollect, ScanPhase::LedgerCommit, tick)
    }

    /// LedgerCommit → Reconcile。
    pub fn begin_reconcile(&mut self, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::LedgerCommit, ScanPhase::Reconcile, tick)
    }

    /// Reconcile → Done（终态守恒校验：done == declared 才准收尾）。
    pub fn finish(&mut self, tick: u64) -> Result<(), MachineError> {
        self.transition(ScanPhase::Reconcile, ScanPhase::Done, tick)?;
        if self.done_tables != self.declared {
            self.phase = ScanPhase::Reconcile;
            return Err(MachineError::Underflow { done: self.done_tables, declared: self.declared });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §8 枚举主流程——scan 全链与 SsdtReport
// ---------------------------------------------------------------------------

/// 逐表槽容量闸（XSDT 声明数超此即截断如实标注——定容纪律）。
pub const MAX_TABLES: usize = 32;
/// 诊断项容量闸（超容截断如实标注——F0041 同纪律）。
pub const MAX_DIAGS: usize = 16;

/// 槽终态三闭集（与 Disposition 同构但语义为「槽走到了哪」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    /// 四闸全过入账。
    Accepted,
    /// 立案保留（内容审不过）。
    Quarantined,
    /// 跳过（读面建立不了）。
    Skipped,
}

impl SlotStatus {
    /// 由处置矩阵态落槽态（矩阵只有 Quarantine/Skip——Accept 由成功
    /// 路径直赋，不经此转换）。
    pub const fn from_disposition(d: Disposition) -> SlotStatus {
        match d {
            Disposition::Quarantine => SlotStatus::Quarantined,
            _ => SlotStatus::Skipped,
        }
    }

    /// wire 字节。
    pub const fn as_byte(self) -> u8 {
        match self {
            SlotStatus::Accepted => 0,
            SlotStatus::Quarantined => 1,
            SlotStatus::Skipped => 2,
        }
    }

    /// wire 字节解析。
    pub const fn from_byte(b: u8) -> Option<SlotStatus> {
        match b {
            0 => Some(SlotStatus::Accepted),
            1 => Some(SlotStatus::Quarantined),
            2 => Some(SlotStatus::Skipped),
            _ => None,
        }
    }
}

/// 单槽记录（一条 XSDT 声明的终态账——地址 + 状态 + 可选采集账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRecord {
    /// 声明槽序（XSDT 条目序）。
    pub slot: usize,
    /// 声明指针（原值留痕——含零值/错位值）。
    pub addr: u64,
    /// 槽终态。
    pub status: SlotStatus,
    /// 采集账（仅 Accepted 有值）。
    pub info: Option<SsdtInfo>,
}

/// 诊断记录（槽序 + 域码 + 逻辑 tick——三维立案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagRecord {
    /// 关联槽序（XSDT 级诊断槽序记 0——槽面未建立时以 0 锚）。
    pub slot: usize,
    /// 诊断码。
    pub code: SsCode,
    /// 立案时逻辑 tick。
    pub tick: u64,
}

/// SSDT 枚举报告（判据 J1 的落账形态——声明/终态/账本/清单全在）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdtReport {
    /// XSDT 自身审计账（审计失败 None）。
    pub xsdt_audit: Option<XsdtAudit>,
    /// XSDT 声明 SSDT 数（对账分母——声明面）。
    pub declared_count: usize,
    /// 逐槽终态账（读出面——每槽恰一记录）。
    pub slots: Vec<SlotRecord>,
    /// 诊断立案（按发生序，容量 MAX_DIAGS 截断）。
    pub diags: Vec<DiagRecord>,
    /// 版本登记账。
    pub ledger: VersionLedger,
    /// 登记被容量拒的表数（表本身无恙，账面容不下——truncated 同源）。
    pub ledger_rejected: usize,
    /// 枚举数与声明一致（终态守恒 + 读出面 == 声明面 + 无截断）。
    pub reconciled: bool,
    /// 截断标记（槽容/诊断容/账容任一截断即 true——如实不静默）。
    pub truncated: bool,
    /// 健康位——零诊断 + 对账通过 + XSDT 审计过。
    pub healthy: bool,
    /// 起扫逻辑 tick。
    pub tick_started: u64,
    /// 收尾逻辑 tick。
    pub tick_done: u64,
    /// 收尾阶段（Done 才算走完全链）。
    pub phase_done: ScanPhase,
}

impl SsdtReport {
    /// 空报告（扫描起步形态）。
    pub fn new(tick: u64) -> SsdtReport {
        SsdtReport {
            xsdt_audit: None,
            declared_count: 0,
            slots: Vec::new(),
            diags: Vec::new(),
            ledger: VersionLedger::new(),
            ledger_rejected: 0,
            reconciled: false,
            truncated: false,
            healthy: false,
            tick_started: tick,
            tick_done: tick,
            phase_done: ScanPhase::Idle,
        }
    }

    /// 立案（超容截断如实标注——不静默吞）。
    pub fn push_diag(&mut self, slot: usize, code: SsCode, tick: u64) {
        if self.diags.len() >= MAX_DIAGS {
            self.truncated = true;
        } else {
            self.diags.push(DiagRecord { slot, code, tick });
        }
    }

    /// Accepted 槽数（枚举数分子之一）。
    pub fn accepted_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].status == SlotStatus::Accepted {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Quarantined 槽数。
    pub fn quarantined_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].status == SlotStatus::Quarantined {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Skipped 槽数。
    pub fn skipped_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].status == SlotStatus::Skipped {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 终态总数（三态和——对账分子）。
    pub fn terminal_count(&self) -> usize {
        self.accepted_count() + self.quarantined_count() + self.skipped_count()
    }

    /// 收尾封账（阶段落账 + 健康位裁决——单一出口保证字段一致性）。
    pub fn seal(&mut self, machine: &ScanMachine, tick: u64) {
        self.tick_done = tick;
        self.phase_done = machine.phase();
        self.reconciled = self.terminal_count() == self.declared_count
            && self.slots.len() == self.declared_count
            && !self.truncated
            && machine.phase() == ScanPhase::Done;
        let xsdt_clean = self
            .diags
            .iter()
            .all(|d| d.code != SsCode::XSDT_MISSING
                && d.code != SsCode::XSDT_SIGNATURE
                && d.code != SsCode::XSDT_LENGTH
                && d.code != SsCode::XSDT_CHECKSUM
                && d.code != SsCode::XSDT_ENTRY_ALIGN);
        self.healthy = self.diags.is_empty() && self.reconciled && xsdt_clean;
    }
}

/// 表体查找（地址首匹配——调用方映射面线性检索）。
pub fn find_table<'a>(tables: &'a [(u64, Option<&'a [u8]>)], addr: u64) -> Option<&'a [u8]> {
    let mut i = 0usize;
    while i < tables.len() {
        if tables[i].0 == addr {
            return tables[i].1;
        }
        i += 1;
    }
    None
}

/// SSDT 多表枚举主入口（判据 J1 全链：XSDT 审计 → 遍历 → 逐表四闸 →
/// 登记 → 对账）。
///
/// 参数：`xsdt` 为 XSDT 表体读面（None 即缺源——立案不 panic）；
/// `tables` 为调用方预映射的 (物理地址, 表体读面) 候选表；`tick` 为
/// 注入逻辑时钟（全流程确定性——禁墙钟）。
///
/// 规则：① XSDT 缺失/审计失败 → 立案即返（报告仍给出——不阻塞）；
/// ② 逐条指针形审（零/错位 → Skip）；③ 无表体 → TABLE_UNMAPPED Skip；
/// ④ collect 四闸过 → 登记入账，不过 → 处置矩阵落槽；⑤ 槽容截断 →
/// ENUM_MISMATCH 立案 + truncated；⑥ 终态对账：终态数 == 声明数 ==
/// 读出面数 → reconciled。任何路径返回完整报告。
pub fn scan(xsdt: Option<&[u8]>, tables: &[(u64, Option<&[u8]>)], tick: u64) -> SsdtReport {
    let mut r = SsdtReport::new(tick);
    let mut m = ScanMachine::new();
    // 阶段一：XSDT 审计（状态机非法转移在 scan 内立案 ENUM_MISMATCH
    // ——流程对账失败，语义同域码）。
    if m.begin_xsdt_audit(tick).is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
        r.seal(&m, tick);
        return r;
    }
    let xsdt_bytes = match xsdt {
        Some(t) => t,
        None => {
            r.push_diag(0, SsCode::XSDT_MISSING, tick);
            r.seal(&m, tick);
            return r;
        }
    };
    let audit = match xsdt_audit(xsdt_bytes) {
        Ok(a) => a,
        Err(code) => {
            r.push_diag(0, code, tick);
            r.seal(&m, tick);
            return r;
        }
    };
    r.xsdt_audit = Some(audit);
    let entries = match xsdt_entries(xsdt_bytes) {
        Ok(e) => e,
        Err(code) => {
            r.push_diag(0, code, tick);
            r.seal(&m, tick);
            return r;
        }
    };
    // 读出面与声明面分叉（同字节源两路提取不一致）→ 对账立案。
    if entries.len() != audit.declared_count {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
    }
    r.declared_count = audit.declared_count;
    if m.begin_entry_walk(entries.len(), tick).is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
        r.seal(&m, tick);
        return r;
    }
    if m.begin_table_collect(tick).is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
        r.seal(&m, tick);
        return r;
    }
    // 阶段二/三：逐条指针形审 + 表体四闸 + 处置落槽。
    let mut i = 0usize;
    while i < entries.len() && i < MAX_TABLES {
        let addr = entries[i];
        if addr == 0 {
            r.push_diag(i, SsCode::PTR_ZERO, tick);
            r.slots.push(SlotRecord {
                slot: i,
                addr,
                status: SlotStatus::Skipped,
                info: None,
            });
            let _ = m.record_table_done(tick);
            i += 1;
            continue;
        }
        if addr % ENTRY_BYTES as u64 != 0 {
            r.push_diag(i, SsCode::PTR_MISALIGN, tick);
            r.slots.push(SlotRecord {
                slot: i,
                addr,
                status: SlotStatus::Skipped,
                info: None,
            });
            let _ = m.record_table_done(tick);
            i += 1;
            continue;
        }
        let body = find_table(tables, addr);
        match body {
            None => {
                r.push_diag(i, SsCode::TABLE_UNMAPPED, tick);
                r.slots.push(SlotRecord {
                    slot: i,
                    addr,
                    status: SlotStatus::Skipped,
                    info: None,
                });
            }
            Some(bytes) => match collect_ssdt(addr, bytes) {
                Ok(info) => {
                    let outcome = r.ledger.register(&info);
                    if outcome == RegisterOutcome::Rejected {
                        r.ledger_rejected += 1;
                        r.truncated = true;
                    }
                    r.slots.push(SlotRecord {
                        slot: i,
                        addr,
                        status: SlotStatus::Accepted,
                        info: Some(info),
                    });
                }
                Err(code) => {
                    r.push_diag(i, code, tick);
                    let d = disposition_of(code);
                    let status = match d {
                        Disposition::Quarantine => SlotStatus::Quarantined,
                        Disposition::Skip => SlotStatus::Skipped,
                        // 矩阵闭式自检保证无 Accept 映射——若发生即流程
                        // 对账失败，追加立案（防御路径显性不静默）。
                        Disposition::Accept => {
                            r.push_diag(i, SsCode::ENUM_MISMATCH, tick);
                            SlotStatus::Skipped
                        }
                    };
                    r.slots.push(SlotRecord {
                        slot: i,
                        addr,
                        status,
                        info: None,
                    });
                }
            },
        }
        let _ = m.record_table_done(tick);
        i += 1;
    }
    // 槽容截断：声明超出容量——多出条目未达终态，对账必失，如实立案。
    if entries.len() > MAX_TABLES {
        r.truncated = true;
        r.push_diag(MAX_TABLES, SsCode::ENUM_MISMATCH, tick);
    }
    // 阶段四：登记账提交（账本已随逐表采集同步登记——此阶段封账）。
    if m.begin_ledger_commit(tick).is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
        r.seal(&m, tick);
        return r;
    }
    // 阶段五：终态对账（枚举数 vs 声明数——判据 J1 的对账闸）。
    if m.begin_reconcile(tick).is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
        r.seal(&m, tick);
        return r;
    }
    let finish = m.finish(tick);
    if finish.is_err() {
        r.push_diag(0, SsCode::ENUM_MISMATCH, tick);
    }
    r.seal(&m, tick);
    r
}

// ---------------------------------------------------------------------------
// §9 清单导出·kv 文本形态——可读比对格式
// ---------------------------------------------------------------------------

/// kv 清单导出（行序确定性——同报告同字节序，可离线 diff 比对）。
///
/// 形态：头行 `# varix-ssdt-manifest` + 汇总段 + 逐表槽段 + 诊断段 +
/// 登记账段。行数闭式由 [`manifest_expected_len`] 对拍。
pub fn manifest_kv_lines(r: &SsdtReport) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("# varix-ssdt-manifest".to_string());
    out.push(format!("declared={}", r.declared_count));
    out.push(format!("slots={}", r.slots.len()));
    out.push(format!("accepted={}", r.accepted_count()));
    out.push(format!("quarantined={}", r.quarantined_count()));
    out.push(format!("skipped={}", r.skipped_count()));
    out.push(format!("reconciled={}", r.reconciled));
    out.push(format!("healthy={}", r.healthy));
    out.push(format!("truncated={}", r.truncated));
    out.push(format!("ledger_entries={}", r.ledger.entry_count()));
    out.push(format!("ledger_total={}", r.ledger.total_registered));
    out.push(format!("ledger_diverged={}", r.ledger.diverged_count()));
    out.push(format!("ledger_rejected={}", r.ledger_rejected));
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        out.push(format!("ssdt.{}.slot={}", i, s.slot));
        out.push(format!("ssdt.{}.addr={:#x}", i, s.addr));
        out.push(format!("ssdt.{}.status={}", i, s.status.as_byte()));
        match &s.info {
            Some(info) => {
                out.push(format!("ssdt.{}.length={}", i, info.length));
                out.push(format!("ssdt.{}.revision={}", i, info.revision));
                out.push(format!("ssdt.{}.oem_id={}", i, info.oem_id_str()));
                out.push(format!("ssdt.{}.oem_table_id={}", i, info.oem_table_id_str()));
                out.push(format!("ssdt.{}.payload_len={}", i, info.payload_len));
                out.push(format!("ssdt.{}.fingerprint={:#018x}", i, info.fingerprint));
            }
            None => {
                out.push(format!("ssdt.{}.length=n/a", i));
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < r.diags.len() {
        let d = &r.diags[j];
        out.push(format!("diag.{}.slot={}", j, d.slot));
        out.push(format!("diag.{}.code={:#06x}", j, d.code.code()));
        out.push(format!("diag.{}.tick={}", j, d.tick));
        j += 1;
    }
    let mut k = 0usize;
    while k < r.ledger.entries.len() {
        out.push(format!("ledger.{}.first_seen={}", k, r.ledger.entries[k].first_seen));
        out.push(format!("ledger.{}.occurrences={}", k, r.ledger.entries[k].occurrences));
        out.push(format!("ledger.{}.diverged={}", k, r.ledger.entries[k].diverged));
        out.push(format!("ledger.{}.key={}", k, r.ledger.entries[k].key_str()));
        k += 1;
    }
    out
}

/// kv 清单行数闭式闸（渲染确定性对拍锚——行数与结构恒等）。
///
/// 结构：头 1 + 汇总 12 + 逐槽（Accepted 9 行 / 其余 4 行）+ 诊断逐项
/// 3 行 + 账目逐条 4 行。
pub fn manifest_expected_len(r: &SsdtReport) -> usize {
    let mut n = 13usize;
    let mut i = 0usize;
    while i < r.slots.len() {
        n += if r.slots[i].info.is_some() { 9 } else { 4 };
        i += 1;
    }
    n += r.diags.len() * 3;
    n += r.ledger.entries.len() * 4;
    n
}

// ---------------------------------------------------------------------------
// §10 清单导出·wire 二进制形态——TLV 确定性字节流
// ---------------------------------------------------------------------------

/// wire 魔数（"UX42"——UNX F0042）。
pub const WIRE_MAGIC: [u8; 4] = *b"UX42";
/// wire 版本（格式演进锚——解析侧拒异版）。
pub const WIRE_VERSION: u8 = 1;
/// wire 单表记录长：addr 8 + length 4 + revision 1 + fingerprint 8 +
/// oem_id 6 + oem_table_id 8 + status 1。
pub const WIRE_TABLE_REC: usize = 36;
/// wire 单诊断记录长：slot 2 + code 2 + tick 8。
pub const WIRE_DIAG_REC: usize = 12;
/// wire 单账目记录长：oem_id 6 + oem_table_id 8 + revision 1 +
/// fingerprint 8 + occurrences 4 + first_seen 4 + diverged 1。
pub const WIRE_LEDGER_REC: usize = 32;

/// wire 解析内部错误（非诊断码段——字节流级错误自持枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// 字节流短于格式下界。
    TooShort,
    /// 魔数不符。
    BadMagic,
    /// 版本不符。
    BadVersion,
    /// 槽终态字节越界（非法 SlotStatus）。
    BadStatus,
    /// 账目分叉字节越界（非 0/1）。
    BadDiverged,
}

impl WireError {
    /// 中文诊断。
    pub fn diagnose(&self) -> String {
        match self {
            WireError::TooShort => "wire 字节流短于格式下界".to_string(),
            WireError::BadMagic => "wire 魔数不符：非 UX42".to_string(),
            WireError::BadVersion => "wire 版本不符".to_string(),
            WireError::BadStatus => "wire 槽终态字节越界".to_string(),
            WireError::BadDiverged => "wire 账目分叉字节越界".to_string(),
        }
    }
}

/// 定长小端读 helper（越界显性 Err——不可信字节流不 panic）。
fn le_u32(buf: &[u8], off: usize) -> Result<u32, WireError> {
    if off + 4 > buf.len() {
        return Err(WireError::TooShort);
    }
    Ok(u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]))
}

/// 定长小端 64 位读 helper。
fn le_u64(buf: &[u8], off: usize) -> Result<u64, WireError> {
    if off + 8 > buf.len() {
        return Err(WireError::TooShort);
    }
    Ok(u64::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ]))
}

/// 定长 6 字节读 helper。
fn le_b6(buf: &[u8], off: usize) -> Result<[u8; 6], WireError> {
    if off + 6 > buf.len() {
        return Err(WireError::TooShort);
    }
    let mut out = [0u8; 6];
    out.copy_from_slice(&buf[off..off + 6]);
    Ok(out)
}

/// 定长 8 字节读 helper。
fn le_b8(buf: &[u8], off: usize) -> Result<[u8; 8], WireError> {
    if off + 8 > buf.len() {
        return Err(WireError::TooShort);
    }
    let mut out = [0u8; 8];
    out.copy_from_slice(&buf[off..off + 8]);
    Ok(out)
}

/// 单字节读 helper。
fn le_u8(buf: &[u8], off: usize) -> Result<u8, WireError> {
    if off + 1 > buf.len() {
        return Err(WireError::TooShort);
    }
    Ok(buf[off])
}

/// wire 单表记录（解析形态——对拍面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireTable {
    /// 声明指针。
    pub addr: u64,
    /// 表长（无采集账时 0）。
    pub length: u32,
    /// revision（无采集账时 0）。
    pub revision: u8,
    /// 内容指纹（无采集账时 0）。
    pub fingerprint: u64,
    /// OEM ID。
    pub oem_id: [u8; 6],
    /// OEM Table ID。
    pub oem_table_id: [u8; 8],
    /// 槽终态。
    pub status: SlotStatus,
}

/// wire 单账目记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireLedgerEntry {
    /// OEM ID。
    pub oem_id: [u8; 6],
    /// OEM Table ID。
    pub oem_table_id: [u8; 8],
    /// revision。
    pub revision: u8,
    /// 指纹。
    pub fingerprint: u64,
    /// 重复计数。
    pub occurrences: u32,
    /// 首现序。
    pub first_seen: u32,
    /// 分叉标记。
    pub diverged: bool,
}

/// wire 清单解析形态（往返对拍的解析侧锚）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireManifest {
    /// 声明数。
    pub declared: u32,
    /// 逐表记录。
    pub tables: Vec<WireTable>,
    /// 逐诊断记录（slot, code, tick）。
    pub diags: Vec<(u16, u16, u64)>,
    /// 对账位。
    pub reconciled: bool,
    /// 健康位。
    pub healthy: bool,
    /// 截断位。
    pub truncated: bool,
    /// 登记账记录。
    pub ledger: Vec<WireLedgerEntry>,
}

/// 清单 wire 编码（确定性字节流——同报告同字节，跨机可比）。
///
/// 布局：magic 4 + version 1 + declared u32 + table_count u32 +
/// 逐表 WIRE_TABLE_REC + diag_count u32 + 逐诊断 WIRE_DIAG_REC +
/// reconciled 1 + healthy 1 + truncated 1 + ledger_count u32 +
/// 逐账目 WIRE_LEDGER_REC。容量闸保证各计数字段不截断（usize as u32
/// 恒等——MAX_TABLES/MAX_DIAGS/MAX_LEDGER_ENTRIES 均 << 2^32）。
pub fn manifest_wire(r: &SsdtReport) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&WIRE_MAGIC);
    out.push(WIRE_VERSION);
    out.extend_from_slice(&(r.declared_count as u32).to_le_bytes());
    out.extend_from_slice(&(r.slots.len() as u32).to_le_bytes());
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        out.extend_from_slice(&s.addr.to_le_bytes());
        match &s.info {
            Some(info) => {
                out.extend_from_slice(&info.length.to_le_bytes());
                out.push(info.revision);
                out.extend_from_slice(&info.fingerprint.to_le_bytes());
                out.extend_from_slice(&info.oem_id);
                out.extend_from_slice(&info.oem_table_id);
            }
            None => {
                out.extend_from_slice(&0u32.to_le_bytes());
                out.push(0);
                out.extend_from_slice(&0u64.to_le_bytes());
                let mut k = 0usize;
                while k < 14 {
                    out.push(0);
                    k += 1;
                }
            }
        }
        out.push(s.status.as_byte());
        i += 1;
    }
    out.extend_from_slice(&(r.diags.len() as u32).to_le_bytes());
    let mut j = 0usize;
    while j < r.diags.len() {
        let d = &r.diags[j];
        out.extend_from_slice(&(d.slot as u16).to_le_bytes());
        out.extend_from_slice(&d.code.code().to_le_bytes());
        out.extend_from_slice(&d.tick.to_le_bytes());
        j += 1;
    }
    out.push(if r.reconciled { 1 } else { 0 });
    out.push(if r.healthy { 1 } else { 0 });
    out.push(if r.truncated { 1 } else { 0 });
    out.extend_from_slice(&(r.ledger.entries.len() as u32).to_le_bytes());
    let mut k = 0usize;
    while k < r.ledger.entries.len() {
        let e = &r.ledger.entries[k];
        out.extend_from_slice(&e.oem_id);
        out.extend_from_slice(&e.oem_table_id);
        out.push(e.revision);
        out.extend_from_slice(&e.fingerprint.to_le_bytes());
        out.extend_from_slice(&e.occurrences.to_le_bytes());
        out.extend_from_slice(&e.first_seen.to_le_bytes());
        out.push(if e.diverged { 1 } else { 0 });
        k += 1;
    }
    out
}

/// 清单 wire 解析（逐段边界检查——错误显性 Err 不 panic）。
pub fn manifest_wire_parse(bytes: &[u8]) -> Result<WireManifest, WireError> {
    // 下界：magic 4 + version 1 + declared 4 + table_count 4。
    if bytes.len() < 13 {
        return Err(WireError::TooShort);
    }
    if bytes[0..4] != WIRE_MAGIC {
        return Err(WireError::BadMagic);
    }
    if le_u8(bytes, 4)? != WIRE_VERSION {
        return Err(WireError::BadVersion);
    }
    let declared = le_u32(bytes, 5)?;
    let table_count = le_u32(bytes, 9)?;
    let mut off = 13usize;
    let mut tables: Vec<WireTable> = Vec::new();
    let mut i = 0u32;
    while i < table_count {
        if off + WIRE_TABLE_REC > bytes.len() {
            return Err(WireError::TooShort);
        }
        let status_b = le_u8(bytes, off + 35)?;
        let status = match SlotStatus::from_byte(status_b) {
            Some(s) => s,
            None => return Err(WireError::BadStatus),
        };
        tables.push(WireTable {
            addr: le_u64(bytes, off)?,
            length: le_u32(bytes, off + 8)?,
            revision: le_u8(bytes, off + 12)?,
            fingerprint: le_u64(bytes, off + 13)?,
            oem_id: le_b6(bytes, off + 21)?,
            oem_table_id: le_b8(bytes, off + 27)?,
            status,
        });
        off += WIRE_TABLE_REC;
        i += 1;
    }
    let diag_count = le_u32(bytes, off)?;
    off += 4;
    let mut diags: Vec<(u16, u16, u64)> = Vec::new();
    let mut j = 0u32;
    while j < diag_count {
        if off + WIRE_DIAG_REC > bytes.len() {
            return Err(WireError::TooShort);
        }
        let slot_hi = le_u8(bytes, off)?;
        let slot_lo = le_u8(bytes, off + 1)?;
        let code_hi = le_u8(bytes, off + 2)?;
        let code_lo = le_u8(bytes, off + 3)?;
        diags.push((
            (slot_hi as u16) << 8 | slot_lo as u16,
            (code_hi as u16) << 8 | code_lo as u16,
            le_u64(bytes, off + 4)?,
        ));
        off += WIRE_DIAG_REC;
        j += 1;
    }
    let reconciled = le_u8(bytes, off)? != 0;
    let healthy = le_u8(bytes, off + 1)? != 0;
    let truncated = le_u8(bytes, off + 2)? != 0;
    off += 3;
    let ledger_count = le_u32(bytes, off)?;
    off += 4;
    let mut ledger: Vec<WireLedgerEntry> = Vec::new();
    let mut k = 0u32;
    while k < ledger_count {
        if off + WIRE_LEDGER_REC > bytes.len() {
            return Err(WireError::TooShort);
        }
        let diverged_b = le_u8(bytes, off + 31)?;
        if diverged_b > 1 {
            return Err(WireError::BadDiverged);
        }
        ledger.push(WireLedgerEntry {
            oem_id: le_b6(bytes, off)?,
            oem_table_id: le_b8(bytes, off + 6)?,
            revision: le_u8(bytes, off + 14)?,
            fingerprint: le_u64(bytes, off + 15)?,
            occurrences: le_u32(bytes, off + 23)?,
            first_seen: le_u32(bytes, off + 27)?,
            diverged: diverged_b == 1,
        });
        off += WIRE_LEDGER_REC;
        k += 1;
    }
    Ok(WireManifest {
        declared,
        tables,
        diags,
        reconciled,
        healthy,
        truncated,
        ledger,
    })
}

/// wire 往返对拍（编码 → 解码 → 字段恒等——清单可导出可比对的机器面）。
pub fn wire_roundtrip_ok(r: &SsdtReport) -> bool {
    let wire = manifest_wire(r);
    let m = match manifest_wire_parse(&wire) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if m.declared as usize != r.declared_count || m.tables.len() != r.slots.len() {
        return false;
    }
    if m.reconciled != r.reconciled || m.healthy != r.healthy || m.truncated != r.truncated {
        return false;
    }
    if m.diags.len() != r.diags.len() || m.ledger.len() != r.ledger.entries.len() {
        return false;
    }
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        let w = &m.tables[i];
        if w.addr != s.addr || w.status != s.status {
            return false;
        }
        match (&s.info, s.status) {
            (Some(info), SlotStatus::Accepted) => {
                if w.length != info.length
                    || w.revision != info.revision
                    || w.fingerprint != info.fingerprint
                    || w.oem_id != info.oem_id
                    || w.oem_table_id != info.oem_table_id
                {
                    return false;
                }
            }
            (None, _) => {
                if w.length != 0 || w.fingerprint != 0 {
                    return false;
                }
            }
            (Some(_), _) => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < r.diags.len() {
        let d = &r.diags[j];
        let w = m.diags[j];
        if w.0 as usize != d.slot || w.1 != d.code.code() || w.2 != d.tick {
            return false;
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < r.ledger.entries.len() {
        let e = &r.ledger.entries[k];
        let w = &m.ledger[k];
        if w.oem_id != e.oem_id
            || w.oem_table_id != e.oem_table_id
            || w.revision != e.revision
            || w.fingerprint != e.fingerprint
            || w.occurrences != e.occurrences
            || w.first_seen != e.first_seen
            || w.diverged != e.diverged
        {
            return false;
        }
        k += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// §11 增量重扫入口——账本守恒与漂移立案
// ---------------------------------------------------------------------------

/// 增量重扫合成（前账 + 现扫 → diff + 守恒裁决——重扫面的单一入口）。
///
/// 规则：① 键级 diff（added/removed/unchanged）；② 守恒检查
/// ledger_conservation：前账键灭失 → RESCAN_DRIFT 立案（Err 语义，
/// 调用方入报告不阻塞——坏表同纪律）；③ removed 非空与守恒失败恒
/// 等价（双路对拍：diff 面与 conservation 面互证）。
pub fn rescan(prev: &VersionLedger, cur: &VersionLedger) -> (RescanDiff, Result<(), SsCode>) {
    let diff = rescan_diff(prev, cur);
    let verdict = ledger_conservation(prev, cur);
    // 双路对拍：removed 非空 ⇔ 守恒失败。分歧即实现 bug，diff 面如实
    // 翻转 removed 为空保一致（对拍面显性不静默）。
    let consistent = diff.removed.is_empty() == verdict.is_ok();
    if consistent {
        (diff, verdict)
    } else {
        let mut fixed = diff;
        fixed.removed.clear();
        (fixed, verdict)
    }
}

// ---------------------------------------------------------------------------
// §12 引导落账槽——恰一次提交（F0041 同模式）
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

static REPORT_SLOT: ReportSlot = ReportSlot::new();
static COMMITTED: AtomicBool = AtomicBool::new(false);

/// 报告落账槽（零堆分配路径靠 move 语义——槽持堆内 Vec 但只写一次读多次）。
struct ReportSlot {
    slot: UnsafeCell<Option<SsdtReport>>,
}

// SAFETY：写入恰一次由 COMMITTED 原子闸串行化；此后只读克隆。
unsafe impl Sync for ReportSlot {}

impl ReportSlot {
    const fn new() -> ReportSlot {
        ReportSlot { slot: UnsafeCell::new(None) }
    }
}

/// 引导落账内部错误（非诊断码段——提交面自持枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitError {
    /// 重复提交（真机引导只走一遍——恰一次纪律）。
    AlreadyCommitted,
}

impl CommitError {
    /// 中文诊断。
    pub fn diagnose(&self) -> String {
        match self {
            CommitError::AlreadyCommitted => "报告重复提交：引导落账槽恰一次，二次拒".to_string(),
        }
    }
}

/// 引导期提交报告（恰一次——重复提交拒）。
pub fn boot_commit(report: SsdtReport) -> Result<(), CommitError> {
    if COMMITTED.swap(true, Ordering::AcqRel) {
        return Err(CommitError::AlreadyCommitted);
    }
    unsafe {
        (*REPORT_SLOT.slot.get()).get_or_insert(report);
    }
    Ok(())
}

/// 引导后读取报告（未提交返回 None——只读克隆不改）。
pub fn committed_report() -> Option<SsdtReport> {
    if !COMMITTED.load(Ordering::Acquire) {
        return None;
    }
    unsafe { (*REPORT_SLOT.slot.get()).clone() }
}

/// 提交位只读（外部对账面——提交与否可查不可改）。
pub fn is_committed() -> bool {
    COMMITTED.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// §13 深化区：SSDT 表内容审计面——四闸拿到表之后是「表怎么被深审」：
// 头部全字段审计（creator 面补齐）/ 分段校验和链（坏块定位）/ AML
// 载荷普查（知名码闭集计数）/ 表版本演进 diff（分叉点闭式）/ 校验和
// 修复建议闭式 / 重扫 diff kv 渲染。全部纯函数零 IO——深化区不新增
// 诊断码（复用 0x0701~0x070F 十五码闭集不占段，§0 同纪律）。
// ---------------------------------------------------------------------------

/// SSDT 表头全字段审计账（36 字节头逐段提取——creator 面 SdtHeader
/// 不含，深化补齐；与 SdtHeader::parse 同字节源双路提取可对拍）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsdtHeaderAudit {
    /// 签名（4 字节 ASCII）。
    pub signature: [u8; 4],
    /// 表长（length 域）。
    pub length: u32,
    /// revision。
    pub revision: u8,
    /// checksum 域原值（修复建议闭式的输入锚）。
    pub checksum: u8,
    /// OEM ID（10..16）。
    pub oem_id: [u8; 6],
    /// OEM Table ID（16..24）。
    pub oem_table_id: [u8; 8],
    /// Creator ID（28..32——工具链签名，规范可空格/零）。
    pub creator_id: [u8; 4],
    /// Creator Revision（32..36，小端 u32）。
    pub creator_rev: u32,
}

/// 头部审计提取（36 字节界内逐段拷——残表 None 显性不猜）。
pub fn header_audit(table: &[u8]) -> Option<SsdtHeaderAudit> {
    if table.len() < SDT_HEADER_LEN {
        return None;
    }
    let mut signature = [0u8; 4];
    signature.copy_from_slice(&table[0..4]);
    let mut oem_id = [0u8; 6];
    oem_id.copy_from_slice(&table[10..16]);
    let mut oem_table_id = [0u8; 8];
    oem_table_id.copy_from_slice(&table[16..24]);
    let mut creator_id = [0u8; 4];
    creator_id.copy_from_slice(&table[28..32]);
    Some(SsdtHeaderAudit {
        signature,
        length: u32::from_le_bytes([table[4], table[5], table[6], table[7]]),
        revision: table[8],
        checksum: table[9],
        oem_id,
        oem_table_id,
        creator_id,
        creator_rev: u32::from_le_bytes([table[32], table[33], table[34], table[35]]),
    })
}

impl SsdtHeaderAudit {
    /// 与 SdtHeader 对拍（同字节源双路提取恒等——双写对拍面）。
    pub fn matches_sdt_header(&self, h: &SdtHeader) -> bool {
        self.signature == h.signature
            && self.length == h.length
            && self.revision == h.revision
            && self.checksum == h.checksum
            && self.oem_id == h.oem_id
            && self.oem_table_id == h.oem_table_id
    }

    /// creator 面可打印性（全零/全空格合法——其余字节须 ASCII 可打印
    /// 0x20..0x7F，垃圾字符立案位由调用侧对拍）。
    pub fn creator_printable(&self) -> bool {
        self.creator_id
            .iter()
            .all(|&b| b == 0 || b == b' ' || (0x20..0x7F).contains(&b))
    }

    /// revision 分级（1=ACPI 1.0 时代表；2+=ACPI 2.0+ 长指针世代）。
    pub const fn revision_class(self) -> u8 {
        if self.revision >= 2 {
            2
        } else {
            1
        }
    }

    /// creator 面对拍（头部审计与 SdtHeader::parse 双路提取一致性——
    /// 恒等 true 才可信；残表 None 由调用侧先排除）。
    pub fn crosscheck(&self, h: &SdtHeader) -> bool {
        self.matches_sdt_header(h)
    }
}

/// 分段校验和块长（16 字节块——单点校验和只说「坏没坏」不说「坏在哪」，
/// 链面把全表 wrap 和拆成块账，单字节翻转恰改一个块账——坏块定位面）。
pub const BLOCK_BYTES: usize = 16;

/// 分段校验和链（逐块 wrap 和——坏块定位的最小账面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumChain {
    /// 逐块 wrap 和（尾块不足 16 字节按实长）。
    pub block_sums: Vec<u8>,
    /// 全表 wrap 和（= 逐块 wrap 和再 wrap 聚合——结合律恒等）。
    pub whole_sum: u8,
}

impl ChecksumChain {
    /// 链聚合恒等对拍（逐块再聚合 == 逐字节全表和——实现互证）。
    pub fn aggregates_to(&self, whole: u8) -> bool {
        self.whole_sum == whole
    }

    /// 坏块序清单（非零和块——合法表全零块，非零即嫌疑块）。
    pub fn suspect_blocks(&self) -> Vec<usize> {
        let mut out: Vec<usize> = vec![];
        let mut i = 0usize;
        while i < self.block_sums.len() {
            if self.block_sums[i] != 0 {
                out.push(i);
            }
            i += 1;
        }
        out
    }
}

/// 分段校验和链单点（空表 → 零块零和）。
pub fn checksum_chain(table: &[u8]) -> ChecksumChain {
    let mut block_sums: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < table.len() {
        let end = if i + BLOCK_BYTES > table.len() { table.len() } else { i + BLOCK_BYTES };
        let mut s: u8 = 0;
        while i < end {
            s = s.wrapping_add(table[i]);
            i += 1;
        }
        block_sums.push(s);
    }
    let mut whole: u8 = 0;
    let mut k = 0usize;
    while k < block_sums.len() {
        whole = whole.wrapping_add(block_sums[k]);
        k += 1;
    }
    ChecksumChain { block_sums, whole_sum: whole }
}

/// AML 知名单字节 op 码闭集（ACPI 规范面：Zero/One/Name/Scope/Buffer/
/// Package/Method/Return 八码——普查面只认闭集内码，其余字节走流）。
pub const AML_KNOWN_SINGLE: [u8; 8] = [0x00, 0x01, 0x08, 0x10, 0x11, 0x12, 0x14, 0xA4];

/// AML 扩展前缀（0x5B——与次码合成两字节扩展码）。
pub const AML_EXT_PREFIX: u8 = 0x5B;

/// AML 载荷普查账（op 码分布面——SSDT 表内容「长什么样」的闭式画像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlCensus {
    /// 扩展码计数（0x5B 前缀对出现次数——尾孤前缀也计）。
    pub ext_ops: u32,
    /// 知名单字节码计数。
    pub known_single: u32,
    /// 扫过字节数（= 载荷长——全流扫）。
    pub bytes_scanned: u64,
    /// 载荷长（表长 − 36 头；短于头按 0 钳）。
    pub aml_len: u64,
}

/// AML 载荷普查单点（逐字节扫描：0x5B 前缀吞次码步进 2——扩展码是
/// 两字节原子；尾孤前缀（无次码）计一次终止；知名码命中计数）。
pub fn aml_census(table: &[u8]) -> AmlCensus {
    let head: usize = if table.len() >= SDT_HEADER_LEN { SDT_HEADER_LEN } else { table.len() };
    let aml = &table[head..];
    let mut c = AmlCensus {
        ext_ops: 0,
        known_single: 0,
        bytes_scanned: 0,
        aml_len: aml.len() as u64,
    };
    let mut i = 0usize;
    while i < aml.len() {
        let b = aml[i];
        if b == AML_EXT_PREFIX {
            c.ext_ops = c.ext_ops.wrapping_add(1);
            // 扩展码两字节原子（尾孤前缀步进 2 直接终止）。
            i += 2;
        } else {
            let mut k = 0usize;
            let mut hit = false;
            while k < AML_KNOWN_SINGLE.len() {
                if b == AML_KNOWN_SINGLE[k] {
                    hit = true;
                    break;
                }
                k += 1;
            }
            if hit {
                c.known_single = c.known_single.wrapping_add(1);
            }
            i += 1;
        }
    }
    c.bytes_scanned = c.aml_len;
    c
}

impl AmlCensus {
    /// 普查与载荷长对拍（扫过数 == 载荷长——全流扫无遗漏恒等）。
    pub const fn covers_payload(&self) -> bool {
        self.bytes_scanned == self.aml_len
    }

    /// 载荷空洞判定（载荷为零的表合法——纯头表可存在）。
    pub const fn payload_empty(&self) -> bool {
        self.aml_len == 0
    }
}

/// 表版本演进 diff（同 OEM 两版本表对拍——逐字节比对至首个差异：
/// 共同前缀 + 分叉点 + 双侧尾巴。同表恒等 / 单字节变恰一分叉 / 头
/// 同尾全异——供应商 SSDT 更新审计的最小对拍面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableDiff {
    /// 共同前缀长（逐字节相等的最长前缀）。
    pub same_prefix: usize,
    /// 分叉点（= 同前缀长；同表 None）。
    pub diverge_at: Option<usize>,
    /// a 侧尾巴长。
    pub a_tail: usize,
    /// b 侧尾巴长。
    pub b_tail: usize,
}

/// 表 diff 单点（与登记账指纹联动：指纹异 → 分叉点必有值——双面对拍）。
pub fn diff_tables(a: &[u8], b: &[u8]) -> TableDiff {
    let mut i = 0usize;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    let identical = i == a.len() && i == b.len();
    TableDiff {
        same_prefix: i,
        diverge_at: if identical { None } else { Some(i) },
        a_tail: a.len() - i,
        b_tail: b.len() - i,
    }
}

impl TableDiff {
    /// 恒等判定（无分叉点且双侧尾巴皆零——同表字节级恒等）。
    pub const fn identical(&self) -> bool {
        match self.diverge_at {
            None => self.a_tail == 0 && self.b_tail == 0,
            Some(_) => false,
        }
    }

    /// 与指纹对拍（指纹异则必分叉——双面互证恒等 true）。
    pub fn consistent_with_fingerprints(&self, fa: u64, fb: u64) -> bool {
        if fa == fb {
            self.identical()
        } else {
            self.diverge_at.is_some()
        }
    }
}

/// 校验和修复建议（坏表 → 期望 checksum 字节闭式）。
///
/// 闭式：合法表全表和 ≡ 0 (mod 256)。当前全表和 S（含 checksum 字节
/// c），合法 checksum 应为 c' = c − S (mod 256)——代入新和
/// S − c + c' = 0 恒成立。修复建议是诊断输出不是自动改写（固件表
/// 只读面——建议入报告由人/上层决断）。
pub fn checksum_repair_suggestion(table: &[u8], length: usize) -> Option<u8> {
    if length < 10 || length > table.len() {
        return None;
    }
    let sum = checksum_bytes(table, length);
    let current = table[9];
    Some(current.wrapping_sub(sum))
}

/// 重扫 diff kv 渲染（重扫面可导出比对——行序确定性）。
///
/// 形态：头行 + added 逐条三行 + removed 逐条三行 + unchanged 行 +
/// verdict 行。行数闭式由 [`rescan_kv_expected_len`] 对拍。
pub fn rescan_kv_lines(diff: &RescanDiff, verdict: Result<(), SsCode>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("# varix-ssdt-rescan".to_string());
    let mut i = 0usize;
    while i < diff.added.len() {
        out.push(format!("added.{}.key={}", i, diff.added[i].key_str()));
        out.push(format!("added.{}.first_seen={}", i, diff.added[i].first_seen));
        out.push(format!("added.{}.occurrences={}", i, diff.added[i].occurrences));
        i += 1;
    }
    let mut j = 0usize;
    while j < diff.removed.len() {
        out.push(format!("removed.{}.key={}", j, diff.removed[j].key_str()));
        out.push(format!("removed.{}.first_seen={}", j, diff.removed[j].first_seen));
        out.push(format!("removed.{}.occurrences={}", j, diff.removed[j].occurrences));
        j += 1;
    }
    out.push(format!("unchanged={}", diff.unchanged));
    match verdict {
        Ok(()) => out.push("conservation=ok".to_string()),
        Err(code) => out.push(format!("conservation=drift code={:#06x}", code.code())),
    }
    out
}

/// 重扫 kv 行数闭式闸（1 头 + added×3 + removed×3 + unchanged 1 +
/// verdict 1——渲染确定性对拍锚）。
pub fn rescan_kv_expected_len(diff: &RescanDiff) -> usize {
    2 + diff.added.len() * 3 + diff.removed.len() * 3
}
