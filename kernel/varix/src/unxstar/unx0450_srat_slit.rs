//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0450 SRAT/SLIT NUMA 账（F0348 消费） · 0x75240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   SRAT 域-内存映射账齐（n 项=推算数），SLIT 域间延迟矩阵 n×n 完整，
//!   F0348 直读表 4/4 一致对照。
//! 设计要点：
//!   1. SRAT 布局闭集建模：表头 36B + 保留区 12B + 条目流；条目只认
//!      Type 0（LAPIC/CPU 亲和，16B）与 Type 1（内存亲和，40B）——
//!      ACPI 6.5 §5.2.16；未知类型显性拒，不静默跳过。
//!   2. 域-内存映射账：每个 Enabled 的 Type 1 条目落一行（域号/基址/
//!      长度），账行数与条目流推算数对拍，再与 F0348 直读内存区数
//!      三方对齐（n 项=推算数口径）。
//!   3. SLIT 布局：表头 36B + 本地性域数 8B + n×n 单字节延迟矩阵
//!      （ACPI 6.5 §5.2.17）；矩阵字节长必须恰为 n×n，对角线恒 10
//!      （同域本地性口径），并按本仓冻结口径校验对称性。
//!   4. F0348 mock 直读对照：直读表 4 字段（域数/内存区数/SLIT 维数/
//!      对角值）与解析账逐项对照，4/4 全等才算账齐。
//!   5. 单调状态机：Idle → HeaderChecked → EntriesWalked →
//!      MemMapSealed → SlitLinked，倒退显性拒。
//!   6. 注入四路（坏条目长/未知类型/对角违约/矩阵截断）逐路必抓；
//!      复算一致 + 双跑一致收口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75240，WBV1104·AI-VX7-1104 F0450 独占窗）。
pub const SEG_BASE: u32 = 0x75240;

// ── 冻结常量表（来源：ACPI 6.5 §5.2.16 SRAT / §5.2.17 SLIT） ──────
/// SRAT/SLIT 表头长（通用表头 36B，§5.2.6）。
pub const TBL_HDR_LEN: usize = 36;
/// SRAT 表头后保留区长（Flags 4B + Reserved 8B = 12B）。
pub const SRAT_RESV_LEN: usize = 12;
/// Type 0：Processor Local APIC 亲和条目长（16B 闭集）。
pub const SRAT_CPU_ENTRY_LEN: usize = 16;
/// Type 1：Memory 亲和条目长（40B 闭集，本仓口径尾保留 4B 封闭）。
pub const SRAT_MEM_ENTRY_LEN: usize = 40;
/// SLIT 域数字段长（8B u64，§5.2.17）。
pub const SLIT_DIM_LEN: usize = 8;
/// SLIT 对角线口径：同域本地性延迟恒 10。
pub const SLIT_DIAG_LOCALITY: u8 = 10;
/// 域号上限（本仓冻结 16，mock 机 2 域）。
pub const MAX_DOMAINS: usize = 16;
/// SLIT 维数上限（本仓冻结 16；2 域机维数 2）。
pub const MAX_SLIT_DIM: usize = 16;
/// 判据口径：F0348 直读对照表 4 字段全等。
pub const F0348_FIELDS: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SratSlitCode {
    /// 表头/保留区长度不足（SRAT < 48B 或 SLIT < 44B）
    HdrTooShort,
    /// 条目类型不在闭集（只认 Type 0/Type 1）
    EntryTypeUnknown,
    /// 条目长度与类型不符（Type 0 应 16 / Type 1 应 40）
    EntryLenBad,
    /// SLIT 域数违约（为 0、超 16 上限，或矩阵长 ≠ n×n）
    SlitDimBad,
    /// SLIT 对角线非 10（同域本地性口径违约）
    SlitDiagNot10,
    /// SLIT 矩阵被截断（字节数不足 n×n）
    SlitTruncated,
    /// 域-内存映射账行数与推算数不符
    LedgerCountMismatch,
    /// 域号越界（≥ 16）
    DomainOver,
    /// 状态机倒退（非单调推进）
    PhaseRegress,
    /// 复算失配（重建账与原账不等）
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl SratSlitCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SratSlitCode::HdrTooShort => "表头长度不足：SRAT 须 ≥48B（36 表头+12 保留）、SLIT 须 ≥44B，建议核对表 length 字段与划线内存边界",
            SratSlitCode::EntryTypeUnknown => "SRAT 条目类型不在闭集（只认 Type 0/1）：建议核对固件 SRAT 版本与条目流划线",
            SratSlitCode::EntryLenBad => "条目长度与类型不符：Type 0 应 16B / Type 1 应 40B，建议核对条目 length 字节",
            SratSlitCode::SlitDimBad => "SLIT 域数违约：域数为 0、超 16 上限或矩阵长 ≠ n×n，建议核对 Number of System Localities 字段",
            SratSlitCode::SlitDiagNot10 => "SLIT 对角线非 10：同域本地性延迟恒 10（§5.2.17），建议核对矩阵对角格",
            SratSlitCode::SlitTruncated => "SLIT 矩阵被截断：矩阵字节数不足 n×n，建议核对表 length 与域数换算",
            SratSlitCode::LedgerCountMismatch => "域-内存映射账行数与推算数不符：建议逐条比对 Type 1 条目流与账行",
            SratSlitCode::DomainOver => "域号越界（≥16）：建议核对 Proximity Domain 字段与 MAX_DOMAINS 冻结值",
            SratSlitCode::PhaseRegress => "状态机倒退：SRAT/SLIT 账只许单调推进，建议按 Idle→Header→Entries→MapSealed→SlitLinked 重排",
            SratSlitCode::ReplayMismatch => "复算失配：重建账与原账不等，建议检查解析路径是否含随机序或未冻结语料",
            SratSlitCode::NotReproducible => "双跑不一致：同一语料两次解析结果不同，复现红线，建议排查全局可变态",
        }
    }
    pub const ALL: [SratSlitCode; 11] = [
        SratSlitCode::HdrTooShort,
        SratSlitCode::EntryTypeUnknown,
        SratSlitCode::EntryLenBad,
        SratSlitCode::SlitDimBad,
        SratSlitCode::SlitDiagNot10,
        SratSlitCode::SlitTruncated,
        SratSlitCode::LedgerCountMismatch,
        SratSlitCode::DomainOver,
        SratSlitCode::PhaseRegress,
        SratSlitCode::ReplayMismatch,
        SratSlitCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段内窗口外一律未知码）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SratSlitCode::ALL.len() {
        if let Some(c) = SratSlitCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75240 段闭集内"
}

// ── 字节读取助手（一律 .get，不裸下标） ─────────────────────────
fn rd_u8(buf: &[u8], off: usize) -> Option<u8> {
    buf.get(off).copied()
}

fn rd_u16(buf: &[u8], off: usize) -> Option<u16> {
    let end = match off.checked_add(2) {
        Some(e) => e,
        None => return None,
    };
    if end > buf.len() {
        return None;
    }
    let lo = match rd_u8(buf, off) {
        Some(b) => u16::from(b),
        None => return None,
    };
    let hi = match rd_u8(buf, off + 1) {
        Some(b) => u16::from(b),
        None => return None,
    };
    Some(lo | (hi << 8))
}

fn rd_u32(buf: &[u8], off: usize) -> Option<u32> {
    let end = match off.checked_add(4) {
        Some(e) => e,
        None => return None,
    };
    if end > buf.len() {
        return None;
    }
    let lo = match rd_u16(buf, off) {
        Some(v) => u32::from(v),
        None => return None,
    };
    let hi = match rd_u16(buf, off + 2) {
        Some(v) => u32::from(v),
        None => return None,
    };
    Some(lo | (hi << 16))
}

fn rd_u64(buf: &[u8], off: usize) -> Option<u64> {
    let end = match off.checked_add(8) {
        Some(e) => e,
        None => return None,
    };
    if end > buf.len() {
        return None;
    }
    let lo = match rd_u32(buf, off) {
        Some(v) => u64::from(v),
        None => return None,
    };
    let hi = match rd_u32(buf, off + 4) {
        Some(v) => u64::from(v),
        None => return None,
    };
    Some(lo | (hi << 32))
}

/// 偏移加法助手：越界溢出显性拒（EntryLenBad 承载）。
fn add_off(off: usize, delta: usize) -> Result<usize, u32> {
    off.checked_add(delta).ok_or(SratSlitCode::EntryLenBad.val())
}

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.push((x & 0xFF) as u8);
    v.push((x >> 8) as u8);
}

fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
    v.push(((x >> 16) & 0xFF) as u8);
    v.push((x >> 24) as u8);
}

fn push_u64(v: &mut Vec<u8>, x: u64) {
    push_u32(v, (x & 0xFFFF_FFFF) as u32);
    push_u32(v, (x >> 32) as u32);
}

// ── mock 表头写入助手（36B 通用表头，字段占位不参与校验） ────────
fn push_tbl_hdr(v: &mut Vec<u8>, sig: &[u8; 4], total_len: u32, rev: u8) {
    for b in sig.iter() {
        v.push(*b);
    }
    push_u32(v, total_len);
    v.push(rev);
    v.push(0); // checksum（mock 不校验）
    // OEM ID 6B：'OEM   '
    v.push(b'O');
    v.push(b'E');
    v.push(b'M');
    v.push(b' ');
    v.push(b' ');
    v.push(b' ');
    // OEM Table ID 8B：'VX7D0450'
    v.push(b'V');
    v.push(b'X');
    v.push(b'7');
    v.push(b'D');
    v.push(b'0');
    v.push(b'4');
    v.push(b'5');
    v.push(b'0');
    push_u32(v, 1); // OEM revision
    // Creator ID 4B + Creator revision 4B
    v.push(b'V');
    v.push(b'X');
    v.push(b'7');
    v.push(b'D');
    push_u32(v, 1);
}

// ── SRAT 条目域模型（闭集：Type 0 / Type 1） ────────────────────
/// Type 0：Processor Local APIC 亲和（16B 闭集，§5.2.16.1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuAffinity {
    /// Proximity Domain（1B 低 8 位口径）
    pub domain: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Flags（bit0 = Enabled）
    pub flags: u32,
}

/// Type 1：Memory 亲和（40B 闭集，§5.2.16.2 本仓口径尾保留 4B）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemAffinity {
    /// Proximity Domain（4B）
    pub domain: u32,
    /// 内存基址（Base Low/High 拼 64 位）
    pub base: u64,
    /// 内存长度（Length Low/High 拼 64 位）
    pub length: u64,
    /// Flags（bit0 = Enabled，bit1 = Hot Pluggable，bit2 = Non-Volatile）
    pub flags: u32,
}

impl MemAffinity {
    /// 该内存条目是否计入可用映射账（Enabled 位）。
    pub fn enabled(&self) -> bool {
        self.flags & 0x1 != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SratLedger {
    /// 解析出的域号去重计数（推算口径之一）
    pub domain_count: usize,
    /// Type 0 条目账
    pub cpu_entries: Vec<CpuAffinity>,
    /// Type 1 条目账
    pub mem_entries: Vec<MemAffinity>,
    /// 域-内存映射账（域号/基址/长度），每个 Enabled 的 Type 1 落一行
    pub mem_map: Vec<(u32, u64, u64)>,
}

/// 域-内存映射账落账：每个 Enabled 的 Type 1 条目一行；Disabled 条目
/// 不入账（单测覆盖该路径，推算口径按 Enabled 计）。
fn mem_map_rows(mem_entries: &[MemAffinity]) -> Vec<(u32, u64, u64)> {
    let mut rows = Vec::new();
    for e in mem_entries {
        if e.enabled() {
            rows.push((e.domain, e.base, e.length));
        }
    }
    rows
}

/// SRAT 解析：表头 36B + 保留 12B + 条目流（闭集 Type 0/1）。
pub fn parse_srat(bytes: &[u8]) -> Result<SratLedger, u32> {
    if bytes.len() < TBL_HDR_LEN + SRAT_RESV_LEN {
        return Err(SratSlitCode::HdrTooShort.val());
    }
    let mut cpu: Vec<CpuAffinity> = Vec::new();
    let mut mem: Vec<MemAffinity> = Vec::new();
    let mut off = add_off(TBL_HDR_LEN, SRAT_RESV_LEN)?;
    while off < bytes.len() {
        let ty = match rd_u8(bytes, off) {
            Some(t) => t,
            None => return Err(SratSlitCode::HdrTooShort.val()),
        };
        let len8 = match rd_u8(bytes, off + 1) {
            Some(l) => l,
            None => return Err(SratSlitCode::HdrTooShort.val()),
        };
        let len = len8 as usize;
        if len < 2 {
            return Err(SratSlitCode::EntryLenBad.val());
        }
        let next = add_off(off, len)?;
        if next > bytes.len() {
            return Err(SratSlitCode::EntryLenBad.val());
        }
        match ty {
            0 => {
                if len != SRAT_CPU_ENTRY_LEN {
                    return Err(SratSlitCode::EntryLenBad.val());
                }
                let domain = match rd_u8(bytes, add_off(off, 2)?) {
                    Some(d) => d,
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                if domain as usize >= MAX_DOMAINS {
                    return Err(SratSlitCode::DomainOver.val());
                }
                let apic_id = match rd_u8(bytes, add_off(off, 3)?) {
                    Some(a) => a,
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                let flags = match rd_u32(bytes, add_off(off, 4)?) {
                    Some(f) => f,
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                cpu.push(CpuAffinity { domain, apic_id, flags });
            }
            1 => {
                if len != SRAT_MEM_ENTRY_LEN {
                    return Err(SratSlitCode::EntryLenBad.val());
                }
                let domain = match rd_u32(bytes, add_off(off, 2)?) {
                    Some(d) => d,
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                if domain as usize >= MAX_DOMAINS {
                    return Err(SratSlitCode::DomainOver.val());
                }
                let base_lo = match rd_u32(bytes, add_off(off, 8)?) {
                    Some(v) => u64::from(v),
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                let len_lo = match rd_u32(bytes, add_off(off, 12)?) {
                    Some(v) => u64::from(v),
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                let base_hi = match rd_u32(bytes, add_off(off, 20)?) {
                    Some(v) => u64::from(v),
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                let len_hi = match rd_u32(bytes, add_off(off, 24)?) {
                    Some(v) => u64::from(v),
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                let flags = match rd_u32(bytes, add_off(off, 32)?) {
                    Some(f) => f,
                    None => return Err(SratSlitCode::EntryLenBad.val()),
                };
                mem.push(MemAffinity {
                    domain,
                    base: base_lo | (base_hi << 32),
                    length: len_lo | (len_hi << 32),
                    flags,
                });
            }
            _ => return Err(SratSlitCode::EntryTypeUnknown.val()),
        }
        off = next;
    }
    // 域号去重计数（推算口径：出现在任一条目流中的域）。
    let mut doms: Vec<u32> = Vec::new();
    for c in &cpu {
        let d = u32::from(c.domain);
        if !doms.contains(&d) {
            doms.push(d);
        }
    }
    for m in &mem {
        if !doms.contains(&m.domain) {
            doms.push(m.domain);
        }
    }
    let rows = mem_map_rows(&mem);
    Ok(SratLedger {
        domain_count: doms.len(),
        cpu_entries: cpu,
        mem_entries: mem,
        mem_map: rows,
    })
}

// ── SLIT 域间延迟矩阵 ───────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlitTable {
    /// 本地性域数 n
    pub dim: usize,
    /// n×n 单字节延迟矩阵（行主序）
    pub matrix: Vec<u8>,
}

impl SlitTable {
    /// 读格：行 i 列 j（行/列任一越界即 None，n×n 语义）。
    pub fn cell(&self, i: usize, j: usize) -> Option<u8> {
        if i >= self.dim || j >= self.dim {
            return None;
        }
        let idx = match i.checked_mul(self.dim) {
            Some(p) => p,
            None => return None,
        };
        let idx = match idx.checked_add(j) {
            Some(p) => p,
            None => return None,
        };
        self.matrix.get(idx).copied()
    }
    /// n×n 完整性：矩阵字节数恰为 dim×dim。
    pub fn complete_ok(&self) -> bool {
        match self.dim.checked_mul(self.dim) {
            Some(n) => self.matrix.len() == n,
            None => false,
        }
    }
    /// 对角线 10 口径：每格 diag(i,i) == 10。
    pub fn diag_ok(&self) -> bool {
        let mut i = 0;
        while i < self.dim {
            match self.cell(i, i) {
                Some(v) if v == SLIT_DIAG_LOCALITY => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
    /// 对称口径（本仓冻结）：cell(i,j) == cell(j,i)。
    pub fn symmetric_ok(&self) -> bool {
        let mut i = 0;
        while i < self.dim {
            let mut j = i + 1;
            while j < self.dim {
                let a = match self.cell(i, j) {
                    Some(v) => v,
                    None => return false,
                };
                let b = match self.cell(j, i) {
                    Some(v) => v,
                    None => return false,
                };
                if a != b {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// SLIT 解析：表头 36B + 域数 8B + n×n 矩阵。
pub fn parse_slit(bytes: &[u8]) -> Result<SlitTable, u32> {
    if bytes.len() < TBL_HDR_LEN + SLIT_DIM_LEN {
        return Err(SratSlitCode::HdrTooShort.val());
    }
    let dim_raw = match rd_u64(bytes, TBL_HDR_LEN) {
        Some(d) => d,
        None => return Err(SratSlitCode::HdrTooShort.val()),
    };
    if dim_raw == 0 || dim_raw as usize > MAX_SLIT_DIM {
        return Err(SratSlitCode::SlitDimBad.val());
    }
    let dim = dim_raw as usize;
    let need = match dim.checked_mul(dim) {
        Some(n) => n,
        None => return Err(SratSlitCode::SlitDimBad.val()),
    };
    let have = match bytes.len().checked_sub(TBL_HDR_LEN + SLIT_DIM_LEN) {
        Some(n) => n,
        None => return Err(SratSlitCode::HdrTooShort.val()),
    };
    if have < need {
        return Err(SratSlitCode::SlitTruncated.val());
    }
    if have != need {
        // 多余尾字节同样拒：账面必须恰好 n×n。
        return Err(SratSlitCode::SlitDimBad.val());
    }
    let mut matrix = Vec::new();
    let mut i = 0;
    while i < need {
        let off = add_off(add_off(TBL_HDR_LEN, SLIT_DIM_LEN)?, i)?;
        match rd_u8(bytes, off) {
            Some(b) => matrix.push(b),
            None => return Err(SratSlitCode::SlitTruncated.val()),
        }
        i += 1;
    }
    let t = SlitTable { dim, matrix };
    if !t.diag_ok() {
        return Err(SratSlitCode::SlitDiagNot10.val());
    }
    Ok(t)
}

// ── F0348 mock 直读对照（4 字段闭集） ───────────────────────────
/// F0348 直读表：绕过本模块解析、独立 mock 直读的 4 字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct F0348DirectRead {
    /// 域数
    pub numa_domains: u32,
    /// 内存区数
    pub mem_regions: u32,
    /// SLIT 维数
    pub slit_dim: u32,
    /// 对角线本地性值
    pub diag_locality: u32,
}

/// mock 直读：从冻结语料常量独立取值（模拟 F0348 既有直读路径）。
pub fn f0348_mock_read() -> F0348DirectRead {
    F0348DirectRead {
        numa_domains: MOCK_DOMAINS,
        mem_regions: MOCK_MEM_REGIONS,
        slit_dim: MOCK_SLIT_DIM,
        diag_locality: u32::from(SLIT_DIAG_LOCALITY),
    }
}

/// 4/4 对照：解析账 vs 直读表，返回命中数（0~4）。
pub fn f0348_compare(ledger: &SratLedger, slit: &SlitTable, mock: &F0348DirectRead) -> usize {
    let mut hit = 0;
    if ledger.domain_count as u32 == mock.numa_domains {
        hit += 1;
    }
    if ledger.mem_entries.len() as u32 == mock.mem_regions {
        hit += 1;
    }
    if slit.dim as u32 == mock.slit_dim {
        hit += 1;
    }
    if u32::from(SLIT_DIAG_LOCALITY) == mock.diag_locality && slit.diag_ok() {
        hit += 1;
    }
    hit
}

// ── 冻结 mock 语料（2 域机：域 0/1，各 1 CPU + 1 内存区） ────────
pub const MOCK_DOMAINS: u32 = 2;
pub const MOCK_MEM_REGIONS: u32 = 2;
pub const MOCK_SLIT_DIM: u32 = 2;
pub const MOCK_SLIT_REMOTE: u8 = 16;

/// 冻结 SRAT 语料：48B 头区 + 2×16B CPU 条目 + 2×40B 内存条目 = 160B。
pub fn srat_corpus() -> Vec<u8> {
    let total = match (TBL_HDR_LEN + SRAT_RESV_LEN)
        .checked_add(2 * SRAT_CPU_ENTRY_LEN)
        .and_then(|s| s.checked_add(2 * SRAT_MEM_ENTRY_LEN))
    {
        Some(t) => t,
        None => 0,
    };
    let mut v = Vec::new();
    push_tbl_hdr(&mut v, b"SRAT", total as u32, 1);
    let mut i = 0;
    while i < SRAT_RESV_LEN {
        v.push(0);
        i += 1;
    }
    // Type 0：域 0 / APIC 0、域 1 / APIC 2（均 Enabled）。
    let cpus = [(0u8, 0u8), (1u8, 2u8)];
    for c in cpus.iter() {
        v.push(0);
        v.push(SRAT_CPU_ENTRY_LEN as u8);
        v.push(c.0);
        v.push(c.1);
        push_u32(&mut v, 1); // Flags: Enabled
        v.push(0); // Local SAPIC EID
        push_u32(&mut v, 0); // Clock domain
        v.push(0);
        v.push(0);
        v.push(0); // Reserved 3B
    }
    // Type 1：域 0 @0x80000000 64MB、域 1 @0xC0000000 64MB。
    let mems = [
        (0u32, 0x8000_0000u64, 0x0400_0000u64),
        (1u32, 0xC000_0000u64, 0x0400_0000u64),
    ];
    for m in mems.iter() {
        v.push(1);
        v.push(SRAT_MEM_ENTRY_LEN as u8);
        push_u32(&mut v, m.0);
        v.push(0);
        v.push(0); // Reserved 2B
        push_u32(&mut v, (m.1 & 0xFFFF_FFFF) as u32);
        push_u32(&mut v, (m.2 & 0xFFFF_FFFF) as u32);
        push_u32(&mut v, 0); // Reserved 4B
        push_u32(&mut v, (m.1 >> 32) as u32);
        push_u32(&mut v, (m.2 >> 32) as u32);
        push_u32(&mut v, 0); // Reserved 4B
        push_u32(&mut v, 1); // Flags: Enabled
        push_u32(&mut v, 0); // Reserved 4B（本仓口径尾部封闭）
    }
    v
}

/// 冻结 SLIT 语料：36B 头 + 域数 8B + 2×2 对称矩阵（对角 10、远端 16）= 48B。
pub fn slit_corpus() -> Vec<u8> {
    let dim_n = MOCK_SLIT_DIM as usize;
    let body = match dim_n
        .checked_mul(dim_n)
        .and_then(|m| m.checked_add(TBL_HDR_LEN + SLIT_DIM_LEN))
    {
        Some(t) => t,
        None => 0,
    };
    let mut v = Vec::new();
    push_tbl_hdr(&mut v, b"SLIT", body as u32, 1);
    push_u64(&mut v, MOCK_SLIT_DIM as u64);
    v.push(SLIT_DIAG_LOCALITY);
    v.push(MOCK_SLIT_REMOTE);
    v.push(MOCK_SLIT_REMOTE);
    v.push(SLIT_DIAG_LOCALITY);
    v
}

// ── 注入语料（四路：坏条目长/未知类型/对角违约/矩阵截断） ────────
/// 注入 1：Type 0 条目长度改 15（闭集要求 16）。
pub fn srat_corpus_bad_entry_len() -> Vec<u8> {
    let mut v = srat_corpus();
    // 第一个 CPU 条目的长度字节位于 48+1。
    let off = TBL_HDR_LEN + SRAT_RESV_LEN + 1;
    if let Some(slot) = v.get_mut(off) {
        *slot = (SRAT_CPU_ENTRY_LEN - 1) as u8;
    }
    v
}

/// 注入 2：首条目类型改 9（闭集外）。
pub fn srat_corpus_unknown_type() -> Vec<u8> {
    let mut v = srat_corpus();
    let off = TBL_HDR_LEN + SRAT_RESV_LEN;
    if let Some(slot) = v.get_mut(off) {
        *slot = 9;
    }
    v
}

/// 注入 3：SLIT 对角线改 11（口径违约）。
pub fn slit_corpus_bad_diag() -> Vec<u8> {
    let mut v = slit_corpus();
    let off = TBL_HDR_LEN + SLIT_DIM_LEN;
    if let Some(slot) = v.get_mut(off) {
        *slot = SLIT_DIAG_LOCALITY + 1;
    }
    v
}

/// 注入 4：SLIT 矩阵截断（砍掉最后一格）。
pub fn slit_corpus_truncated() -> Vec<u8> {
    let mut v = slit_corpus();
    let _ = v.pop();
    v
}

/// 注入 5（边界路）：SRAT 域号改 16（== 上限，必拒）。
pub fn srat_corpus_domain_over() -> Vec<u8> {
    let mut v = srat_corpus();
    let off = TBL_HDR_LEN + SRAT_RESV_LEN + 2;
    if let Some(slot) = v.get_mut(off) {
        *slot = MAX_DOMAINS as u8;
    }
    v
}

/// 注入 6（边界路）：SLIT 维数改 17（> 上限，必拒）。
pub fn slit_corpus_dim_over() -> Vec<u8> {
    let mut v = slit_corpus();
    let off = TBL_HDR_LEN;
    if let Some(slot) = v.get_mut(off) {
        *slot = (MAX_SLIT_DIM + 1) as u8;
    }
    v
}

// ── 单调状态机（Idle→Header→Entries→MapSealed→SlitLinked） ──────
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SratPhase {
    Idle,
    HeaderChecked,
    EntriesWalked,
    MemMapSealed,
    SlitLinked,
}

pub struct SratStateMachine {
    phase: SratPhase,
}

impl SratStateMachine {
    pub fn new() -> SratStateMachine {
        SratStateMachine { phase: SratPhase::Idle }
    }
    pub fn phase(&self) -> SratPhase {
        self.phase
    }
    /// 单调推进：目标相位的秩必须严格大于当前秩，否则显性拒。
    pub fn advance(&mut self, next: SratPhase) -> Result<SratPhase, u32> {
        if next <= self.phase {
            return Err(SratSlitCode::PhaseRegress.val());
        }
        self.phase = next;
        Ok(next)
    }
}

/// 黄金全链：SRAT 解析 → 映射账对拍 → SLIT 解析 → 完整性 → F0348 4/4。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let ledger = parse_srat(&srat_corpus())?;
    // 账行数 = 推算数（Type 1 条目数）。
    if ledger.mem_map.len() != ledger.mem_entries.len() {
        return Err(SratSlitCode::LedgerCountMismatch.val());
    }
    let slit = parse_slit(&slit_corpus())?;
    if !slit.complete_ok() || !slit.diag_ok() || !slit.symmetric_ok() {
        return Err(SratSlitCode::SlitDimBad.val());
    }
    let mock = f0348_mock_read();
    let hit = f0348_compare(&ledger, &slit, &mock);
    if hit != F0348_FIELDS {
        return Err(SratSlitCode::LedgerCountMismatch.val());
    }
    Ok((ledger.cpu_entries.len(), ledger.mem_map.len(), hit))
}

/// 复算：同语料重建两次，账全等。
pub fn replay_ok() -> Result<(), u32> {
    let a = parse_srat(&srat_corpus())?;
    let b = parse_srat(&srat_corpus())?;
    let sa = parse_slit(&slit_corpus())?;
    let sb = parse_slit(&slit_corpus())?;
    if a != b || sa != sb {
        return Err(SratSlitCode::ReplayMismatch.val());
    }
    Ok(())
}

/// 双跑一致：golden_run 两跑同值（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SratSlitCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：SEG_BASE 64 对齐、落在 0x75000~0x781C0、闭集码全在窗内。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < SratSlitCode::ALL.len() {
        if let Some(c) = SratSlitCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 边界：域号/维数上限冻结拒。
pub fn boundary_ok() -> bool {
    let over = match parse_srat(&srat_corpus_domain_over()) {
        Err(e) => e == SratSlitCode::DomainOver.val(),
        Ok(_) => false,
    };
    let dim_bad = match parse_slit(&slit_corpus_dim_over()) {
        Err(e) => e == SratSlitCode::SlitDimBad.val(),
        Ok(_) => false,
    };
    over && dim_bad
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn normal_flow_ok() -> bool {
    match golden_run() {
        Ok((cpu_n, map_n, hit)) => cpu_n == 2 && map_n == 2 && hit == 4,
        Err(_) => false,
    }
}

fn ledger_pair_ok() -> bool {
    let ledger = match parse_srat(&srat_corpus()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // n 项=推算数：账行数与 Type 1 条目数一致，并逐行复算域/基址/长度。
    if ledger.mem_map.len() != ledger.mem_entries.len() {
        return false;
    }
    let mut i = 0;
    while i < ledger.mem_entries.len() {
        let e = match ledger.mem_entries.get(i) {
            Some(e) => e,
            None => return false,
        };
        let row = match ledger.mem_map.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0 != e.domain || row.1 != e.base || row.2 != e.length {
            return false;
        }
        i += 1;
    }
    // 映射账具体值复核（冻结语料）。
    let expect = [
        (0u32, 0x8000_0000u64, 0x0400_0000u64),
        (1u32, 0xC000_0000u64, 0x0400_0000u64),
    ];
    if ledger.mem_map.len() != expect.len() {
        return false;
    }
    let mut j = 0;
    while j < expect.len() {
        match ledger.mem_map.get(j) {
            Some(r) if *r == expect[j] => {}
            _ => return false,
        }
        j += 1;
    }
    true
}

fn slit_integrity_ok() -> bool {
    let slit = match parse_slit(&slit_corpus()) {
        Ok(s) => s,
        Err(_) => return false,
    };
    slit.complete_ok() && slit.diag_ok() && slit.symmetric_ok() && slit.dim == MOCK_SLIT_DIM as usize
}

fn f0348_ok() -> bool {
    match (parse_srat(&srat_corpus()), parse_slit(&slit_corpus())) {
        (Ok(l), Ok(s)) => f0348_compare(&l, &s, &f0348_mock_read()) == F0348_FIELDS,
        _ => false,
    }
}

fn inject_all_caught() -> bool {
    // 路 1：坏条目长必抓。
    let c1 = match parse_srat(&srat_corpus_bad_entry_len()) {
        Err(e) => e == SratSlitCode::EntryLenBad.val(),
        Ok(_) => false,
    };
    // 路 2：未知类型必抓。
    let c2 = match parse_srat(&srat_corpus_unknown_type()) {
        Err(e) => e == SratSlitCode::EntryTypeUnknown.val(),
        Ok(_) => false,
    };
    // 路 3：对角线违约必抓。
    let c3 = match parse_slit(&slit_corpus_bad_diag()) {
        Err(e) => e == SratSlitCode::SlitDiagNot10.val(),
        Ok(_) => false,
    };
    // 路 4：矩阵截断必抓。
    let c4 = match parse_slit(&slit_corpus_truncated()) {
        Err(e) => e == SratSlitCode::SlitTruncated.val(),
        Ok(_) => false,
    };
    c1 && c2 && c3 && c4
}

fn phase_reject_ok() -> bool {
    let mut sm = SratStateMachine::new();
    let a = sm.advance(SratPhase::HeaderChecked).is_ok();
    let b = sm.advance(SratPhase::EntriesWalked).is_ok();
    let c = sm.advance(SratPhase::MemMapSealed).is_ok();
    let d = sm.advance(SratPhase::SlitLinked).is_ok();
    // 倒退注入：SlitLinked 之后再推 EntriesWalked 必拒。
    let regress = match sm.advance(SratPhase::EntriesWalked) {
        Err(e) => e == SratSlitCode::PhaseRegress.val(),
        Ok(_) => false,
    };
    a && b && c && d && regress
}

fn hdr_short_ok() -> bool {
    // 截取 47B（< 48B 头区下沿）→ HdrTooShort 必抓。
    let full = srat_corpus();
    let mut v = Vec::new();
    let mut i = 0;
    while i < TBL_HDR_LEN + SRAT_RESV_LEN - 1 {
        match full.get(i) {
            Some(b) => v.push(*b),
            None => return false,
        }
        i += 1;
    }
    match parse_srat(&v) {
        Err(e) => e == SratSlitCode::HdrTooShort.val(),
        Ok(_) => false,
    }
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn run_unx0450_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0450 SRAT/SLIT NUMA 账");
    s.add_dyn(
        "F0450-黄金全链",
        normal_flow_ok(),
        "2 CPU + 2 内存区 + SLIT 2x2 + F0348 4/4",
    );
    s.add_dyn(
        "F0450-映射账n项对拍",
        ledger_pair_ok(),
        "账行数=Type1 条目数，逐行域/基址/长度复算",
    );
    s.add_dyn("F0450-SLIT完整性", slit_integrity_ok(), "n×n 完整 + 对角 10 + 对称口径");
    s.add_dyn("F0450-F0348直读4/4", f0348_ok(), "域数/内存区数/维数/对角值 逐项一致");
    s.add_dyn(
        "F0450-注入四路必抓",
        inject_all_caught(),
        "坏条目长/未知类型/对角违约/矩阵截断 全检出",
    );
    s.add_dyn("F0450-状态机倒退拒", phase_reject_ok(), "单调四推 + 倒退注入显性拒");
    s.add_dyn("F0450-边界上限", boundary_ok(), "域号 16 拒 + 维数 17 拒（冻结上限口径）");
    s.add_dyn("F0450-复算一致", replay_ok().is_ok(), "同语料重建两次账全等");
    s.add_dyn("F0450-双跑一致", repro_check().is_ok(), "golden_run 两跑同值（复现红线）");
    s.add_dyn(
        "F0450-段位守卫",
        segment_guard(),
        "0x75240 窗口：64 对齐 + 闭集码全在窗内",
    );
    s.add_dyn(
        "F0450-诊断码闭集查表",
        detail(SratSlitCode::SlitDiagNot10.val()) == SratSlitCode::SlitDiagNot10.detail()
            && detail(SEG_BASE + 0x3F).starts_with("未知诊断码"),
        "段内命中 + 窗外未知码",
    );
    s.add_dyn(
        "F0450-表头过短拒",
        hdr_short_ok(),
        "SRAT 语料砍 1 字节必抓 HdrTooShort",
    );
    s.add_dyn(
        "F0450-冻结常量口径",
        MAX_SLIT_DIM == 16 && SLIT_DIAG_LOCALITY == 10 && F0348_FIELDS == 4 && MAX_DOMAINS == 16,
        "常量来源 §5.2.16/§5.2.17 + F0348 四字段口径",
    );
    s.add_dyn(
        "F0450-语料账面自洽",
        srat_corpus().len() == 160 && slit_corpus().len() == 48,
        "SRAT 160B（48+32+80）/ SLIT 48B（44+4 矩阵格）",
    );
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ledger() -> SratLedger {
        SratLedger {
            domain_count: 0,
            cpu_entries: Vec::new(),
            mem_entries: Vec::new(),
            mem_map: Vec::new(),
        }
    }

    #[test]
    fn test_golden_normal_flow() {
        assert_eq!(golden_run(), Ok((2, 2, 4)));
    }

    #[test]
    fn test_ledger_pair() {
        assert!(ledger_pair_ok());
        let l = match parse_srat(&srat_corpus()) {
            Ok(l) => l,
            Err(_) => empty_ledger(),
        };
        assert_eq!(l.mem_map.len(), 2);
        assert_eq!(l.domain_count, 2);
        assert_eq!(l.cpu_entries.len(), 2);
    }

    #[test]
    fn test_slit_integrity() {
        assert!(slit_integrity_ok());
        let s = match parse_slit(&slit_corpus()) {
            Ok(s) => s,
            Err(_) => SlitTable { dim: 0, matrix: Vec::new() },
        };
        assert_eq!(s.cell(0, 1), Some(MOCK_SLIT_REMOTE));
        assert_eq!(s.cell(1, 1), Some(SLIT_DIAG_LOCALITY));
        assert_eq!(s.cell(0, 2), None);
    }

    #[test]
    fn test_f0348_four_fields() {
        let l = match parse_srat(&srat_corpus()) {
            Ok(l) => l,
            Err(_) => empty_ledger(),
        };
        let s = match parse_slit(&slit_corpus()) {
            Ok(s) => s,
            Err(_) => SlitTable { dim: 0, matrix: Vec::new() },
        };
        assert_eq!(f0348_compare(&l, &s, &f0348_mock_read()), 4);
    }

    #[test]
    fn test_injections_caught() {
        assert!(inject_all_caught());
        assert_eq!(
            parse_srat(&srat_corpus_unknown_type()),
            Err(SratSlitCode::EntryTypeUnknown.val())
        );
        assert_eq!(
            parse_slit(&slit_corpus_bad_diag()),
            Err(SratSlitCode::SlitDiagNot10.val())
        );
    }

    #[test]
    fn test_phase_regress() {
        let mut sm = SratStateMachine::new();
        assert!(sm.advance(SratPhase::SlitLinked).is_ok());
        assert_eq!(sm.phase(), SratPhase::SlitLinked);
        assert_eq!(sm.advance(SratPhase::Idle), Err(SratSlitCode::PhaseRegress.val()));
    }

    #[test]
    fn test_boundary() {
        assert!(boundary_ok());
        assert_eq!(
            parse_srat(&srat_corpus_domain_over()),
            Err(SratSlitCode::DomainOver.val())
        );
        assert_eq!(
            parse_slit(&slit_corpus_dim_over()),
            Err(SratSlitCode::SlitDimBad.val())
        );
    }

    #[test]
    fn test_replay_and_repro() {
        assert!(replay_ok().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0);
    }

    #[test]
    fn test_diag_codes_closed_set() {
        let mut i = 0;
        while i < SratSlitCode::ALL.len() {
            if let Some(c) = SratSlitCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!c.detail().is_empty());
            }
            i += 1;
        }
        assert!(detail(0).starts_with("未知诊断码"));
        assert_eq!(SratSlitCode::ALL.len(), 11);
    }

    #[test]
    fn test_disabled_mem_not_in_map() {
        // Disabled 内存条目不入映射账（账行数 < 条目数时推算口径显性）。
        let mut v = srat_corpus();
        let off = TBL_HDR_LEN + SRAT_RESV_LEN + 2 * SRAT_CPU_ENTRY_LEN + SRAT_MEM_ENTRY_LEN + 32;
        if let Some(slot) = v.get_mut(off) {
            *slot = 0;
        }
        let l = match parse_srat(&v) {
            Ok(l) => l,
            Err(_) => empty_ledger(),
        };
        assert_eq!(l.mem_entries.len(), 2);
        assert_eq!(l.mem_map.len(), 1);
    }

    #[test]
    fn test_double_run_checks() {
        let a = run_unx0450_checks();
        let b = run_unx0450_checks();
        assert!(a.all_passed());
        assert!(b.all_passed());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 14);
    }
}
