//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0451 表清单快照与 ACPI 版本账 · 0x75280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   全表 4 列快照（签名/revision/长度/地址，行数=表数）落账，版本谱系
//!   账注（mixed 显性）。
//! 设计要点：
//!   1. 快照行模型 4 列冻结：签名 4 字节 / revision u8 / 长度 u32 /
//!      地址 u64——通用 ACPI 表头布局（ACPI 6.5 §5.2.6）逐列落账。
//!   2. 行数对拍：快照行数必须等于底层表清单推算表数（mock 机 6 表），
//!      多一行少一行都算账不平。
//!   3. 版本谱系三态闭集：全 v1（revision ≤ 1）/ 全 v2+ / mixed——
//!      逐表 revision 归类后整体归档，mixed 显性落账注、不猜不折中。
//!   4. 封账单调：Building → Sealed 单向，封账后追加行显性拒
//!      （快照是封账物，不许二次改写）。
//!   5. FNV-1a 64 位指纹：逐行 4 列按字节序流哈希，封账落指纹；
//!      重放（同清单重建快照）指纹必须一致，已知向量（""/"a"/
//!      "foobar"）锚定算法口径。
//!   6. 注入必抓：全零签名/零长度/零地址/封账后追加/行数不符/空账封账
//!      逐路显性拒；复算一致 + 双跑一致收口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75280，WBV1104·AI-VX7-1104 F0451 独占窗）。
pub const SEG_BASE: u32 = 0x75280;

// ── 冻结常量表（来源：ACPI 6.5 §5.2.6 通用表头 / FNV-1a 公开口径） ─
/// ACPI 签名长（4 字节 ASCII）。
pub const ACPI_SIG_LEN: usize = 4;
/// v1 上限：revision ≤ 1 归 v1 代。
pub const REV_V1_MAX: u8 = 1;
/// FNV-1a 64 位偏移基（公开常量 0xcbf29ce484222325）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位素数（公开常量 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// 快照行容量上限（本仓冻结 32）。
pub const SNAP_MAX_ROWS: usize = 32;
/// mock 机表清单推算表数（6 表：XSDT/RSDT/FACP/APIC/HPET/MCFG）。
pub const MOCK_TABLE_COUNT: usize = 6;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapCode {
    /// 签名违约（全零或含 NUL 的 4 字节非 ASCII 语义）
    SigBad,
    /// 表长度为 0（通用表头最小 36）
    LengthZero,
    /// 表地址为 0（物理地址 0 不可作表基址）
    AddrZero,
    /// 封账后追加行（快照封账物不许改写）
    AppendAfterSeal,
    /// 行数与推算表数不符（对拍失配）
    RowCountMismatch,
    /// 快照行超容量上限（32）
    RowCapOver,
    /// 指纹重放不一致
    FingerprintMismatch,
    /// 谱系归档失败（空账无行可归类）
    LineageAmbiguous,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl SnapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SnapCode::SigBad => "签名违约：4 字节须为非零 ASCII 语义（如 XSDT/FACP），建议核对表头 Signature 字段划线",
            SnapCode::LengthZero => "表长度为 0：通用表头最小 36B，建议核对 length 字段与表划线内存",
            SnapCode::AddrZero => "表地址为 0：物理地址 0 不可作表基址，建议核对 RSDT/XSDT 内指针项",
            SnapCode::AppendAfterSeal => "封账后追加：快照已 Sealed 不许改写，建议重建新快照走 Building→Sealed",
            SnapCode::RowCountMismatch => "行数与推算表数不符：建议对拍根表指向的子表清单与快照行数",
            SnapCode::RowCapOver => "快照行超容量 32：建议核对固件表清单或提升冻结上限",
            SnapCode::FingerprintMismatch => "指纹重放不一致：同清单重建快照指纹应全等，建议排查非冻结语料或哈希口径漂移",
            SnapCode::LineageAmbiguous => "谱系归档失败：空账无行可归类，建议先落行再封账",
            SnapCode::NotReproducible => "双跑不一致：同一清单两次快照结果不同，复现红线，建议排查全局可变态",
        }
    }
    pub const ALL: [SnapCode; 9] = [
        SnapCode::SigBad,
        SnapCode::LengthZero,
        SnapCode::AddrZero,
        SnapCode::AppendAfterSeal,
        SnapCode::RowCountMismatch,
        SnapCode::RowCapOver,
        SnapCode::FingerprintMismatch,
        SnapCode::LineageAmbiguous,
        SnapCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段内窗口外一律未知码）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        if let Some(c) = SnapCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75280 段闭集内"
}

// ── 4 列快照行模型 ──────────────────────────────────────────────
/// 快照行：签名/revision/长度/地址 四列冻结（§5.2.6 通用表头口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// 签名（4 字节，如 b"XSDT"）
    pub sig: [u8; ACPI_SIG_LEN],
    /// 表 revision（版本谱系归类依据）
    pub revision: u8,
    /// 表长度（u32，≥ 36 才合法，此处冻结非零）
    pub length: u32,
    /// 表物理地址（u64，非零）
    pub addr: u64,
}

/// 4 字符静态串 → 签名数组（不足补 0，超出截断）。
pub fn mk_sig(s: &'static str) -> [u8; ACPI_SIG_LEN] {
    let b = s.as_bytes();
    let mut out = [0u8; ACPI_SIG_LEN];
    let mut i = 0;
    while i < ACPI_SIG_LEN {
        out[i] = b.get(i).copied().unwrap_or(0);
        i += 1;
    }
    out
}

/// 签名合法性：非全零且不含 NUL。
pub fn sig_ok(sig: &[u8; ACPI_SIG_LEN]) -> bool {
    let mut nonzero = false;
    for b in sig.iter() {
        if *b == 0 {
            return false;
        }
        nonzero = true;
    }
    nonzero
}

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a 64：h ^= byte; h = h × 素数（wrapping，公开口径）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for b in bytes.iter() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 单行哈希字节流：签名 4B + revision 1B + 长度 LE 4B + 地址 LE 8B = 17B。
pub fn row_bytes(row: &TableRow) -> Vec<u8> {
    let mut v = Vec::new();
    for b in row.sig.iter() {
        v.push(*b);
    }
    v.push(row.revision);
    v.push((row.length & 0xFF) as u8);
    v.push(((row.length >> 8) & 0xFF) as u8);
    v.push(((row.length >> 16) & 0xFF) as u8);
    v.push((row.length >> 24) as u8);
    v.push((row.addr & 0xFF) as u8);
    v.push(((row.addr >> 8) & 0xFF) as u8);
    v.push(((row.addr >> 16) & 0xFF) as u8);
    v.push(((row.addr >> 24) & 0xFF) as u8);
    v.push(((row.addr >> 32) & 0xFF) as u8);
    v.push(((row.addr >> 40) & 0xFF) as u8);
    v.push(((row.addr >> 48) & 0xFF) as u8);
    v.push((row.addr >> 56) as u8);
    v
}

/// 整账指纹：逐行按行序哈希。
pub fn rows_fingerprint(rows: &[TableRow]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for r in rows.iter() {
        for b in row_bytes(r).iter() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

// ── 版本谱系三态闭集（mixed 显性归档不猜） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionClass {
    /// 全 v1（revision ≤ 1，RSDT 代）
    AllV1,
    /// 全 v2+（revision ≥ 2，XSDT 代）
    AllV2Plus,
    /// mixed（混代，显性归档不猜）
    Mixed,
}

impl VersionClass {
    pub fn label(self) -> &'static str {
        match self {
            VersionClass::AllV1 => "全 v1（RSDT 代）",
            VersionClass::AllV2Plus => "全 v2+（XSDT 代）",
            VersionClass::Mixed => "mixed（混代显性归档，不猜不折中）",
        }
    }
}

/// 逐表归类：revision ≤ 1 → v1；≥ 2 → v2+。整体归档取三态闭集。
pub fn classify(rows: &[TableRow]) -> Result<VersionClass, u32> {
    if rows.is_empty() {
        return Err(SnapCode::LineageAmbiguous.val());
    }
    let mut v1 = 0;
    let mut v2 = 0;
    for r in rows.iter() {
        if r.revision <= REV_V1_MAX {
            v1 += 1;
        } else {
            v2 += 1;
        }
    }
    if v1 == rows.len() {
        return Ok(VersionClass::AllV1);
    }
    if v2 == rows.len() {
        return Ok(VersionClass::AllV2Plus);
    }
    Ok(VersionClass::Mixed)
}

// ── 快照账（Building → Sealed 单调，封账落指纹） ────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapPhase {
    /// 建账中（可追加行）
    Building,
    /// 已封账（指纹落账，追加显性拒）
    Sealed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSnapshot {
    /// 快照行（4 列）
    pub rows: Vec<TableRow>,
    /// 单调相位
    pub phase: SnapPhase,
    /// 封账指纹（Sealed 后有效）
    pub fnv: u64,
}

impl TableSnapshot {
    pub fn new() -> TableSnapshot {
        TableSnapshot { rows: Vec::new(), phase: SnapPhase::Building, fnv: 0 }
    }

    /// 追加行（Building 期专用，四闸：签名/长度/地址/容量）。
    pub fn add_row(&mut self, row: TableRow) -> Result<usize, u32> {
        if self.phase == SnapPhase::Sealed {
            return Err(SnapCode::AppendAfterSeal.val());
        }
        if !sig_ok(&row.sig) {
            return Err(SnapCode::SigBad.val());
        }
        if row.length == 0 {
            return Err(SnapCode::LengthZero.val());
        }
        if row.addr == 0 {
            return Err(SnapCode::AddrZero.val());
        }
        if self.rows.len() >= SNAP_MAX_ROWS {
            return Err(SnapCode::RowCapOver.val());
        }
        self.rows.push(row);
        Ok(self.rows.len())
    }

    /// 封账：落 FNV-1a 指纹，相位单向推进；空账显性拒。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == SnapPhase::Sealed {
            return Err(SnapCode::AppendAfterSeal.val());
        }
        if self.rows.is_empty() {
            return Err(SnapCode::LineageAmbiguous.val());
        }
        self.fnv = rows_fingerprint(&self.rows);
        self.phase = SnapPhase::Sealed;
        Ok(self.fnv)
    }

    /// 行数对拍：快照行数是否恰等于推算表数。
    pub fn row_count_ok(&self, expected: usize) -> bool {
        self.phase == SnapPhase::Sealed && self.rows.len() == expected
    }

    /// 重放校验：封账后按现行重算指纹，须与封账值全等。
    pub fn replay_fingerprint(&self) -> Result<u64, u32> {
        if self.phase != SnapPhase::Sealed {
            return Err(SnapCode::LineageAmbiguous.val());
        }
        let h = rows_fingerprint(&self.rows);
        if h != self.fnv {
            return Err(SnapCode::FingerprintMismatch.val());
        }
        Ok(h)
    }
}

// ── mock 表清单（推算表数 6：mixed 谱系——FACP rev4 为 v2+ 代） ──
pub fn machine_tables() -> Vec<TableRow> {
    vec![
        TableRow { sig: mk_sig("XSDT"), revision: 1, length: 36, addr: 0x7FFE_0000 },
        TableRow { sig: mk_sig("RSDT"), revision: 1, length: 36, addr: 0x7FFD_F000 },
        TableRow { sig: mk_sig("FACP"), revision: 4, length: 276, addr: 0x7FFD_B000 },
        TableRow { sig: mk_sig("APIC"), revision: 1, length: 148, addr: 0x7FFD_A000 },
        TableRow { sig: mk_sig("HPET"), revision: 1, length: 56, addr: 0x7FFD_9000 },
        TableRow { sig: mk_sig("MCFG"), revision: 1, length: 60, addr: 0x7FFD_8000 },
    ]
}

/// 全 v1 谱系清单（FACP rev 改 1）。
pub fn tables_all_v1() -> Vec<TableRow> {
    let mut v = machine_tables();
    for r in v.iter_mut() {
        r.revision = 1;
    }
    v
}

/// 全 v2+ 谱系清单（全表 rev 2）。
pub fn tables_all_v2() -> Vec<TableRow> {
    let mut v = machine_tables();
    for r in v.iter_mut() {
        r.revision = 2;
    }
    v
}

/// 建账封账全链：逐行落账 → 封账落指纹。
pub fn build_snapshot(tables: &[TableRow]) -> Result<TableSnapshot, u32> {
    let mut snap = TableSnapshot::new();
    for t in tables.iter() {
        snap.add_row(*t)?;
    }
    snap.seal()?;
    Ok(snap)
}

// ── 注入语料（六路） ────────────────────────────────────────────
/// 注入 1：全零签名行必抓。
pub fn inject_zero_sig() -> Result<(), u32> {
    let mut snap = TableSnapshot::new();
    let r = TableRow { sig: [0, 0, 0, 0], revision: 1, length: 36, addr: 0x7FFD_7000 };
    match snap.add_row(r) {
        Err(e) if e == SnapCode::SigBad.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

/// 注入 2：零长度行必抓。
pub fn inject_zero_length() -> Result<(), u32> {
    let mut snap = TableSnapshot::new();
    let r = TableRow { sig: mk_sig("XXXX"), revision: 1, length: 0, addr: 0x7FFD_7000 };
    match snap.add_row(r) {
        Err(e) if e == SnapCode::LengthZero.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

/// 注入 3：零地址行必抓。
pub fn inject_zero_addr() -> Result<(), u32> {
    let mut snap = TableSnapshot::new();
    let r = TableRow { sig: mk_sig("XXXX"), revision: 1, length: 36, addr: 0 };
    match snap.add_row(r) {
        Err(e) if e == SnapCode::AddrZero.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

/// 注入 4：封账后追加必抓。
pub fn inject_append_after_seal() -> Result<(), u32> {
    let mut snap = build_snapshot(&machine_tables())?;
    let r = TableRow { sig: mk_sig("BERT"), revision: 1, length: 64, addr: 0x7FFD_6000 };
    match snap.add_row(r) {
        Err(e) if e == SnapCode::AppendAfterSeal.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

/// 注入 5：行数不符必抓（清单 6 行、快照只落 5 行）。
pub fn inject_row_count_mismatch() -> Result<(), u32> {
    let tables = machine_tables();
    let mut snap = TableSnapshot::new();
    let mut i = 0;
    while i + 1 < tables.len() {
        let t = match tables.get(i) {
            Some(t) => *t,
            None => return Err(SnapCode::NotReproducible.val()),
        };
        snap.add_row(t)?;
        i += 1;
    }
    if snap.row_count_ok(MOCK_TABLE_COUNT) {
        return Err(SnapCode::NotReproducible.val());
    }
    Ok(())
}

/// 注入 6：行超容量必抓（33 行 → RowCapOver）。
pub fn inject_row_cap_over() -> Result<(), u32> {
    let mut snap = TableSnapshot::new();
    let mut i = 0;
    while i <= SNAP_MAX_ROWS {
        let r = TableRow {
            sig: mk_sig("DUMY"),
            revision: 1,
            length: 36,
            addr: 0x7000_0000 + i as u64,
        };
        let res = snap.add_row(r);
        if i >= SNAP_MAX_ROWS {
            return match res {
                Err(e) if e == SnapCode::RowCapOver.val() => Ok(()),
                _ => Err(SnapCode::NotReproducible.val()),
            };
        }
        res?;
        i += 1;
    }
    Ok(())
}

// ── 黄金全链 / 复算 / 双跑 / 段位守卫 ───────────────────────────
/// 黄金全链：建账 6 行 → 封账指纹 → 行数对拍 → 谱系 mixed → 重放一致。
pub fn golden_run() -> Result<(u64, VersionClass), u32> {
    let snap = build_snapshot(&machine_tables())?;
    if !snap.row_count_ok(MOCK_TABLE_COUNT) {
        return Err(SnapCode::RowCountMismatch.val());
    }
    let cls = classify(&snap.rows)?;
    let h = snap.replay_fingerprint()?;
    Ok((h, cls))
}

/// 复算：同清单重建两次指纹全等。
pub fn replay_ok() -> Result<(), u32> {
    let a = build_snapshot(&machine_tables())?;
    let b = build_snapshot(&machine_tables())?;
    if a.fnv != b.fnv || a.rows != b.rows {
        return Err(SnapCode::FingerprintMismatch.val());
    }
    Ok(())
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SnapCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：SEG_BASE 64 对齐、落在 0x75000~0x781C0、闭集码全在窗内。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        if let Some(c) = SnapCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn normal_flow_ok() -> bool {
    match golden_run() {
        Ok((_, cls)) => cls == VersionClass::Mixed,
        Err(_) => false,
    }
}

fn four_columns_ok() -> bool {
    let snap = match build_snapshot(&machine_tables()) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // 逐列抽查第 3 行（FACP rev4 276 0x7FFD_B000）。
    let r = match snap.rows.get(2) {
        Some(r) => *r,
        None => return false,
    };
    r.sig == mk_sig("FACP") && r.revision == 4 && r.length == 276 && r.addr == 0x7FFD_B000
}

fn lineage_ok() -> bool {
    let m = match build_snapshot(&machine_tables()) {
        Ok(s) => classify(&s.rows),
        Err(_) => return false,
    };
    let a = match build_snapshot(&tables_all_v1()) {
        Ok(s) => classify(&s.rows),
        Err(_) => return false,
    };
    let b = match build_snapshot(&tables_all_v2()) {
        Ok(s) => classify(&s.rows),
        Err(_) => return false,
    };
    m == Ok(VersionClass::Mixed)
        && a == Ok(VersionClass::AllV1)
        && b == Ok(VersionClass::AllV2Plus)
}

fn fingerprint_ok() -> bool {
    let snap = match build_snapshot(&machine_tables()) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if snap.fnv == 0 {
        return false;
    }
    // 已知向量锚定算法口径（FNV-1a 64 公开测试向量）。
    fnv1a64(b"") == FNV_OFFSET_BASIS
        && fnv1a64(b"a") == 0xaf63_dc4c_8601_ec8c
        && fnv1a64(b"foobar") == 0x8594_4171_f739_67e8
        && snap.replay_fingerprint().is_ok()
}

fn inject_all_caught() -> bool {
    inject_zero_sig().is_ok()
        && inject_zero_length().is_ok()
        && inject_zero_addr().is_ok()
        && inject_append_after_seal().is_ok()
        && inject_row_count_mismatch().is_ok()
        && inject_row_cap_over().is_ok()
}

fn empty_seal_ok() -> bool {
    let mut snap = TableSnapshot::new();
    match snap.seal() {
        Err(e) => e == SnapCode::LineageAmbiguous.val(),
        Ok(_) => false,
    }
}

fn row_bytes_len_ok() -> bool {
    // 单行哈希字节流定长 17B：4 签名 + 1 rev + 4 长 + 8 址。
    let r = TableRow { sig: mk_sig("XSDT"), revision: 1, length: 36, addr: 0x7FFE_0000 };
    row_bytes(&r).len() == 17
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn run_unx0451_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0451 表清单快照与 ACPI 版本账");
    let fp_txt = match golden_run() {
        Ok((h, cls)) => format!("指纹=0x{:016X} 谱系={}", h, cls.label()),
        Err(_) => String::from("golden_run 红"),
    };
    s.add_dyn("F0451-快照落账正常流", normal_flow_ok(), &fp_txt);
    s.add_dyn(
        "F0451-行数=表数对拍",
        match build_snapshot(&machine_tables()) {
            Ok(snap) => snap.row_count_ok(MOCK_TABLE_COUNT),
            Err(_) => false,
        },
        "快照 6 行 = 推算表数 6",
    );
    s.add_dyn(
        "F0451-4列逐列抽查",
        four_columns_ok(),
        "FACP 行：签名/rev/长度/地址 四列全中",
    );
    s.add_dyn(
        "F0451-谱系三态",
        lineage_ok(),
        "mixed 显性 + 全 v1 + 全 v2+ 三态闭集归档",
    );
    s.add_dyn(
        "F0451-封账指纹与已知向量",
        fingerprint_ok(),
        "FNV-1a 64 锚定 \"\"/\"a\"/\"foobar\" + 重放一致",
    );
    s.add_dyn(
        "F0451-注入六路必抓",
        inject_all_caught(),
        "全零签名/零长/零址/封账追加/行数不符/超容 全检出",
    );
    s.add_dyn(
        "F0451-空账封账拒",
        empty_seal_ok(),
        "Building 空账 seal 显性拒 LineageAmbiguous",
    );
    s.add_dyn(
        "F0451-复算一致",
        replay_ok().is_ok(),
        "同清单重建两次指纹与行账全等",
    );
    s.add_dyn(
        "F0451-双跑一致",
        repro_check().is_ok(),
        "golden_run 两跑同值（复现红线）",
    );
    s.add_dyn(
        "F0451-段位守卫",
        segment_guard(),
        "0x75280 窗口：64 对齐 + 闭集码全在窗内",
    );
    s.add_dyn(
        "F0451-诊断码闭集查表",
        detail(SnapCode::AppendAfterSeal.val()) == SnapCode::AppendAfterSeal.detail()
            && detail(SEG_BASE + 0x3F).starts_with("未知诊断码"),
        "段内命中 + 窗外未知码",
    );
    s.add_dyn(
        "F0451-行哈希流定长",
        row_bytes_len_ok(),
        "单行字节流 17B（4+1+4+8 冻结口径）",
    );
    s.add_dyn(
        "F0451-冻结常量口径",
        SNAP_MAX_ROWS == 32 && MOCK_TABLE_COUNT == 6 && FNV_PRIME == 0x100000001b3,
        "常量来源 §5.2.6 + FNV-1a 公开口径",
    );
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_normal_flow() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.unwrap_or((0, VersionClass::Mixed)).1, VersionClass::Mixed);
    }

    #[test]
    fn test_row_count_pair() {
        let snap = build_snapshot(&machine_tables()).unwrap_or(TableSnapshot {
            rows: Vec::new(),
            phase: SnapPhase::Building,
            fnv: 0,
        });
        assert!(snap.row_count_ok(6));
        assert!(!snap.row_count_ok(5));
    }

    #[test]
    fn test_four_columns() {
        assert!(four_columns_ok());
        let snap = build_snapshot(&machine_tables()).unwrap_or(TableSnapshot {
            rows: Vec::new(),
            phase: SnapPhase::Building,
            fnv: 0,
        });
        assert_eq!(snap.rows.len(), 6);
        let r0 = snap.rows.get(0).copied().unwrap_or(TableRow {
            sig: [0; 4],
            revision: 0,
            length: 0,
            addr: 0,
        });
        assert_eq!(r0.sig, mk_sig("XSDT"));
        assert_eq!(r0.length, 36);
    }

    #[test]
    fn test_lineage_three_states() {
        assert!(lineage_ok());
        assert_eq!(classify(&machine_tables()), Ok(VersionClass::Mixed));
        assert_eq!(classify(&tables_all_v1()), Ok(VersionClass::AllV1));
        assert_eq!(classify(&tables_all_v2()), Ok(VersionClass::AllV2Plus));
        assert_eq!(
            classify(&[]),
            Err(SnapCode::LineageAmbiguous.val())
        );
    }

    #[test]
    fn test_fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn test_injections_all_caught() {
        assert!(inject_all_caught());
        assert!(inject_zero_sig().is_ok());
        assert!(inject_append_after_seal().is_ok());
        assert!(inject_row_cap_over().is_ok());
    }

    #[test]
    fn test_empty_seal_reject() {
        assert!(empty_seal_ok());
    }

    #[test]
    fn test_replay_and_repro() {
        assert!(replay_ok().is_ok());
        assert!(repro_check().is_ok());
        let a = build_snapshot(&tables_all_v1()).unwrap_or(TableSnapshot {
            rows: Vec::new(),
            phase: SnapPhase::Building,
            fnv: 0,
        });
        let b = build_snapshot(&tables_all_v1()).unwrap_or(TableSnapshot {
            rows: Vec::new(),
            phase: SnapPhase::Building,
            fnv: 0,
        });
        assert_eq!(a.fnv, b.fnv);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_diag_codes_closed_set() {
        let mut i = 0;
        while i < SnapCode::ALL.len() {
            if let Some(c) = SnapCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!c.detail().is_empty());
            }
            i += 1;
        }
        assert_eq!(SnapCode::ALL.len(), 9);
        assert!(detail(0).starts_with("未知诊断码"));
    }

    #[test]
    fn test_mk_sig_boundaries() {
        assert_eq!(mk_sig("AB"), [b'A', b'B', 0, 0]);
        assert_eq!(mk_sig("ABCDEFGH"), [b'A', b'B', b'C', b'D']);
        assert!(sig_ok(&mk_sig("HPET")));
        assert!(!sig_ok(&[0, 0, 0, 0]));
    }

    #[test]
    fn test_double_run_checks() {
        let a = run_unx0451_checks();
        let b = run_unx0451_checks();
        assert!(a.all_passed());
        assert!(b.all_passed());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 13);
    }
}
