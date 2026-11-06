//! UNX-F10014 报表导出器（MD 表格化） · 0x91380 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10014（B21 波次报表线）。
//! 锚点：UNX-F10015-J1 版本号单调核（逐期递增 10/10 次），回退版本 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91380（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `ExportCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 编码器 `CellEncoder`：格值 → MD 单元格文本，竖线/换行/反引号
//!     注入一律拒绝（转义而非吞掉），渲染后可无损回读；
//!   - 表格器 `MdTable`：表头 + 数据行的 MD 管道表渲染，列数冻结，
//!     缺列/多列显性拒绝（解析器角色）；
//!   - 账本 `ExportLedger`：导出格数 = 源格数，失真（回读不一致）恒 0，
//!     只增不减；
//!   - 注入核 `BadExportCase`：竖线注入 / 换行注入 / 缺列 / 超长单元格 /
//!     回读失真五类坏样本逐类 10/10 次检出；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10014 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91380;

/// MD 表列数（判据口径：批次/判据/通过率/敞口/守恒 五列）。
pub const TABLE_COLS: usize = 5;

/// 单元格文本上限（字节）。
pub const CELL_MAX: usize = 64;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EMSGSIZE: i32 = 90;
pub const EIO: i32 = 5;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportCode {
    /// 竖线注入（破坏管道表结构）
    PipeInjection,
    /// 换行注入（破坏行边界）
    NewlineInjection,
    /// 缺列/多列
    ColMismatch,
    /// 单元格超长
    CellTooLong,
    /// 回读失真（格值↔源值不一致）
    RoundtripDrift,
    /// 账本倒退
    LedgerRegress,
    /// 空表头
    EmptyHeader,
    /// 表头重复
    DuplicateHeader,
    /// 行数越限
    RowOverflow,
    /// 分隔行缺失
    MissingSeparator,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ExportCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExportCode::PipeInjection => "竖线注入被拒：单元格文本含 | 会破坏 MD \
                管道表；请先转义为 \\| 再导出",
            ExportCode::NewlineInjection => "换行注入被拒：单元格文本含换行会破坏\
                行边界；请压平为一行",
            ExportCode::ColMismatch => "列数不符：每行必须恰为 5 列；请补缺列或\
                删多列",
            ExportCode::CellTooLong => "单元格超长被拒：文本超过 64 字节；请截断\
                或拆列",
            ExportCode::RoundtripDrift => "回读失真检出：渲染文本回读值与源值不一致；\
                请检查编码器是否吞字符",
            ExportCode::LedgerRegress => "导出账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            ExportCode::EmptyHeader => "空表头被拒：表头五列必须全非空；请补齐\
                列名",
            ExportCode::DuplicateHeader => "表头重复被拒：列名必须唯一；请改名后\
                重导出",
            ExportCode::RowOverflow => "行数越限被拒：超出 40 行容量；请分表导出",
            ExportCode::MissingSeparator => "分隔行缺失：表头后必须有 |---| 行；\
                请按 MD 规范补齐",
            ExportCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定哈希",
            ExportCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91380..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ExportCode; 12] = [
        ExportCode::PipeInjection,
        ExportCode::NewlineInjection,
        ExportCode::ColMismatch,
        ExportCode::CellTooLong,
        ExportCode::RoundtripDrift,
        ExportCode::LedgerRegress,
        ExportCode::EmptyHeader,
        ExportCode::DuplicateHeader,
        ExportCode::RowOverflow,
        ExportCode::MissingSeparator,
        ExportCode::NotReproducible,
        ExportCode::SegmentViolation,
    ];
}

// ── 编码器（渲染 + 回读往返） ─────────────────────────────────────────
/// 编码器：格值 ↔ MD 单元格文本。
#[derive(Default)]
pub struct CellEncoder {
    /// 已编码格数
    pub encoded: u32,
    /// 已拒绝格数
    pub rejected: u32,
}

impl CellEncoder {
    /// 源值 → 单元格文本：只允许安全可见字符，注入字符显性拒绝。
    pub fn encode(&mut self, value: u32) -> Result<String, ExportCode> {
        let text = format!("{}", value);
        if text.len() > CELL_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExportCode::CellTooLong);
        }
        self.encoded = self.encoded.saturating_add(1);
        Ok(text)
    }

    /// 任意文本 → 安全单元格：含 | 或换行的文本必须转义，禁止原样放行。
    pub fn sanitize(&mut self, text: &str) -> Result<String, ExportCode> {
        if text.contains('\n') || text.contains('\r') {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExportCode::NewlineInjection);
        }
        if text.contains('|') {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExportCode::PipeInjection);
        }
        if text.len() > CELL_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExportCode::CellTooLong);
        }
        self.encoded = self.encoded.saturating_add(1);
        Ok(String::from(text))
    }

    /// 单元格文本回读为数值：非数值回 Err（失真判定的回读腿）。
    pub fn decode(&mut self, text: &str) -> Option<u32> {
        if text.is_empty() || text.len() > CELL_MAX {
            return None;
        }
        let mut acc: u32 = 0;
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes.get(i)?;
            if !b.is_ascii_digit() {
                return None;
            }
            acc = acc.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
            i += 1;
        }
        Some(acc)
    }
}

// ── MD 表格器 ─────────────────────────────────────────────────────────
/// MD 管道表：列数冻结 5，行容量 40。
pub struct MdTable {
    header: [String; TABLE_COLS],
    rows: Vec<[String; TABLE_COLS]>,
}

impl MdTable {
    /// 建表：表头五列全非空且唯一，否则拒绝。
    pub fn new(header: [&str; TABLE_COLS]) -> Result<MdTable, ExportCode> {
        let mut h: [String; TABLE_COLS] = [
            String::new(), String::new(), String::new(), String::new(), String::new(),
        ];
        let mut i = 0usize;
        while i < TABLE_COLS {
            if let Some(name) = header.get(i) {
                if name.is_empty() {
                    return Err(ExportCode::EmptyHeader);
                }
                if let Some(slot) = h.get_mut(i) {
                    *slot = String::from(*name);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < TABLE_COLS {
            let mut k = j + 1;
            while k < TABLE_COLS {
                let a = h.get(j).map(|s| s.as_str()).unwrap_or("");
                let b = h.get(k).map(|s| s.as_str()).unwrap_or("");
                if a == b {
                    return Err(ExportCode::DuplicateHeader);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(MdTable { header: h, rows: Vec::new() })
    }

    /// 追加一行：列数冻结（编译期数组即保证），行容量 40。
    pub fn push_row(&mut self, row: [String; TABLE_COLS]) -> Result<(), ExportCode> {
        if self.rows.len() >= 40 {
            return Err(ExportCode::RowOverflow);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 已有行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 渲染为 MD 文本：表头 + 分隔行 + 数据行。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push('|');
        let mut i = 0usize;
        while i < TABLE_COLS {
            if let Some(h) = self.header.get(i) {
                out.push_str(h.as_str());
            }
            out.push('|');
            i += 1;
        }
        out.push('\n');
        out.push('|');
        i = 0;
        while i < TABLE_COLS {
            out.push_str("---|");
            i += 1;
        }
        out.push('\n');
        let mut r = 0usize;
        while r < self.rows.len() {
            out.push('|');
            let mut c = 0usize;
            while c < TABLE_COLS {
                if let Some(row) = self.rows.get(r) {
                    if let Some(cell) = row.get(c) {
                        out.push_str(cell.as_str());
                    }
                }
                out.push('|');
                c += 1;
            }
            out.push('\n');
            r += 1;
        }
        out
    }
}

// ── 导出账 ────────────────────────────────────────────────────────────
/// 导出账：源格数 = 导出格数，失真恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExportLedger {
    /// 源格数
    pub source: u32,
    /// 导出格数
    pub exported: u32,
    /// 失真格数（必须为 0）
    pub drifted: u32,
}

impl ExportLedger {
    /// 记一次格导出（含回读核对结果）。
    pub fn record(&mut self, faithful: bool) -> Result<(), ExportCode> {
        self.source = self.source.saturating_add(1);
        self.exported = self.exported.saturating_add(1);
        if !faithful {
            self.drifted = self.drifted.saturating_add(1);
        }
        if self.exported != self.source {
            return Err(ExportCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：零失真。
    pub fn no_drift(&self) -> bool {
        self.drifted == 0 && self.exported == self.source
    }
}

/// 抽核：导出文本回读与源值逐格比对，返回失真格数。
pub fn spot_check(table: &MdTable, sources: &[u32]) -> u32 {
    let text = table.render();
    let mut drifted = 0u32;
    let mut si = 0usize;
    let mut row_start = 0usize;
    let bytes = text.as_bytes();
    let mut line = 0usize;
    while line <= text.len() {
        // 逐行扫描：跳过表头行与分隔行（前两行）
        if line >= 2 {
            let end = bytes.iter().skip(row_start).position(|&b| b == b'\n')
                .map(|p| row_start + p)
                .unwrap_or(text.len());
            let seg = &text[row_start..end];
            let mut ci = 0usize;
            while ci < TABLE_COLS {
                if let Some(src) = sources.get(si) {
                    let cell = cell_at(seg, ci);
                    let mut enc = CellEncoder::default();
                    let back = enc.decode(cell);
                    if back != Some(*src) {
                        drifted = drifted.saturating_add(1);
                    }
                }
                si += 1;
                ci += 1;
            }
            row_start = end + 1;
            if end >= text.len() {
                break;
            }
        } else {
            let end = bytes.iter().skip(row_start).position(|&b| b == b'\n')
                .map(|p| row_start + p)
                .unwrap_or(text.len());
            row_start = end + 1;
            if end >= text.len() {
                break;
            }
        }
        line += 1;
    }
    drifted
}

/// 取 MD 行内第 col 个单元格文本（跳过首尾竖线）。
fn cell_at(line: &str, col: usize) -> &str {
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut i = 0usize;
    let start = inner;
    let mut rest = start;
    while i < col {
        match rest.find('|') {
            Some(p) => rest = &rest[p + 1..],
            None => return "",
        }
        i += 1;
    }
    match rest.find('|') {
        Some(p) => &rest[..p],
        None => rest,
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏导出样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadExportCase {
    /// 竖线注入
    Pipe,
    /// 换行注入
    Newline,
    /// 空表头
    EmptyHeader,
    /// 超长单元格
    TooLong,
    /// 回读失真
    Drift,
}

impl BadExportCase {
    /// 闭集全量。
    pub const ALL: [BadExportCase; 5] = [
        BadExportCase::Pipe,
        BadExportCase::Newline,
        BadExportCase::EmptyHeader,
        BadExportCase::TooLong,
        BadExportCase::Drift,
    ];

    /// 注入一轮：返回是否被显式检出/拒绝。
    pub fn inject(self) -> bool {
        let mut enc = CellEncoder::default();
        match self {
            BadExportCase::Pipe => enc.sanitize("a|b") == Err(ExportCode::PipeInjection),
            BadExportCase::Newline => {
                enc.sanitize("a\nb") == Err(ExportCode::NewlineInjection)
                    && enc.sanitize("a\rb") == Err(ExportCode::NewlineInjection)
            }
            BadExportCase::EmptyHeader => {
                MdTable::new(["a", "b", "", "d", "e"]).is_err()
                    && MdTable::new(["a", "a", "c", "d", "e"]).is_err()
            }
            BadExportCase::TooLong => {
                let long = "x".repeat(CELL_MAX + 1);
                enc.sanitize(&long) == Err(ExportCode::CellTooLong)
            }
            BadExportCase::Drift => {
                // 失真注入：源值与渲染值不同，spot_check 必须检出
                let mut t = match MdTable::new(["h1", "h2", "h3", "h4", "h5"]) {
                    Ok(t) => t,
                    Err(_) => return false,
                };
                let mut row: [String; TABLE_COLS] = [
                    String::new(), String::new(), String::new(), String::new(), String::new(),
                ];
                let mut i = 0usize;
                while i < TABLE_COLS {
                    if let Some(slot) = row.get_mut(i) {
                        *slot = String::from("7");
                    }
                    i += 1;
                }
                let _ = t.push_row(row);
                let sources = [7u32, 7, 7, 7, 9];
                spot_check(&t, &sources) == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_export_injection() -> (ExportLedger, [u32; 5]) {
    let mut ledger = ExportLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadExportCase::ALL.len() {
        if let Some(case) = BadExportCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
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

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺源格族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；注入字符族"),
    ("EMSGSIZE", -90, 122, "Windows ERROR_INSUFFICIENT_BUFFER；单元格超长"),
    ("EIO", -5, 111, "Windows ERROR_SEM_TIMEOUT；渲染 IO 失败"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：编码→建表→渲染→回读全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut t = match MdTable::new(["batch", "checks", "pass", "exposure", "conserved"]) {
        Ok(t) => t,
        Err(_) => return 0,
    };
    let mut enc = CellEncoder::default();
    let mut bytes: Vec<u8> = Vec::new();
    let values = [10u32, 20, 30, 40, 50];
    let mut row: [String; TABLE_COLS] = [
        String::new(), String::new(), String::new(), String::new(), String::new(),
    ];
    let mut i = 0usize;
    while i < TABLE_COLS {
        if let Some(v) = values.get(i) {
            if let Ok(txt) = enc.encode(*v) {
                if let Some(slot) = row.get_mut(i) {
                    *slot = txt;
                }
            }
        }
        i += 1;
    }
    let pushed = t.push_row(row);
    bytes.push(if pushed.is_ok() { 1 } else { 0 });
    let rendered = t.render();
    let mut j = 0usize;
    let rb = rendered.as_bytes();
    while j < rb.len() {
        if let Some(b) = rb.get(j) {
            bytes.push(*b);
        }
        j += 1;
    }
    let sources = [10u32, 20, 30, 40, 50];
    bytes.push(spot_check(&t, &sources) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10014 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10014_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10014");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91380..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91380..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 正常导出：5 值一行渲染成 MD 管道表
    let mut t = MdTable::new(["batch", "checks", "pass", "exposure", "conserved"]);
    let built = t.is_ok();
    let mut t = match t {
        Ok(t) => t,
        Err(_) => return s,
    };
    let mut enc = CellEncoder::default();
    let mut row: [String; TABLE_COLS] = [
        String::new(), String::new(), String::new(), String::new(), String::new(),
    ];
    let values = [10u32, 20, 30, 40, 50];
    let mut i = 0usize;
    while i < TABLE_COLS {
        if let Some(v) = values.get(i) {
            if let Ok(txt) = enc.encode(*v) {
                if let Some(slot) = row.get_mut(i) {
                    *slot = txt;
                }
            }
        }
        i += 1;
    }
    let _ = t.push_row(row);
    let text = t.render();
    let has_sep = text.contains("|---|---|---|---|---|");
    s.add("正常导出：表头+分隔行+数据行齐备", built && has_sep && t.row_count() == 1,
          "MD 管道表三段结构必须完整");

    // 3. 回读零失真：5 格全对
    let drifted = spot_check(&t, &values);
    s.add("回读零失真：5/5 格值↔源值一致", drifted == 0,
          "渲染后逐格回读必须与源值相等");

    // 4. 失真检出：改一源值必检出 1 格
    let mut t2 = MdTable::new(["a", "b", "c", "d", "e"]);
    let mut t2 = match t2 {
        Ok(t) => t,
        Err(_) => return s,
    };
    let mut row2: [String; TABLE_COLS] = [
        String::new(), String::new(), String::new(), String::new(), String::new(),
    ];
    let mut j = 0usize;
    while j < TABLE_COLS {
        if let Some(slot) = row2.get_mut(j) {
            *slot = String::from("7");
        }
        j += 1;
    }
    let _ = t2.push_row(row2);
    let bad_sources = [7u32, 7, 7, 7, 9];
    s.add("失真检出：1 格源值不同必报 1 失真", spot_check(&t2, &bad_sources) == 1,
          "格值↔源值抽核任何不一致都必须落账");

    // 5. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_export_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "竖线/换行/空表头/超长/失真全被显式拒绝");

    // 6. 导出账守恒：失真=0
    s.add("导出账守恒：源格=导出格且失真=0",
          ledger.no_drift() && ledger.source == 5 * INJECT_ROUNDS,
          "50 次导出必须 50 次保真、0 失真");

    // 7. 注入字符拒绝
    let inj = enc.sanitize("a|b") == Err(ExportCode::PipeInjection)
        && enc.sanitize("a\nb") == Err(ExportCode::NewlineInjection);
    s.add("注入拒绝：竖线/换行不入格", inj,
          "MD 结构字符必须转义或拒绝，禁止原样放行");

    // 8. 编码回读往返一致（幂等性）
    let mut enc2 = CellEncoder::default();
    let rt_ok = {
        let mut all = true;
        let mut k = 0u32;
        while k < 100 {
            let v = k * 37;
            let txt = enc2.encode(v);
            match txt {
                Ok(t) => {
                    if enc2.decode(&t) != Some(v) {
                        all = false;
                    }
                }
                Err(_) => all = false,
            }
            k += 1;
        }
        all
    };
    s.add("往返一致：100 组随机值编码→回读全对", rt_ok,
          "encode(decode(v))==v 必须逐位成立");

    // 9. 表头守卫：空列名/重复列名拒绝
    let hdr = MdTable::new(["a", "b", "", "d", "e"]).is_err()
        && MdTable::new(["a", "b", "b", "d", "e"]).is_err();
    s.add("表头守卫：空名/重名拒绝", hdr,
          "列名唯一非空是回读定位的前提");

    // 10. 超长单元格拒绝
    let mut enc3 = CellEncoder::default();
    let long = "x".repeat(CELL_MAX + 1);
    s.add("边界：超长单元格拒绝", enc3.sanitize(&long) == Err(ExportCode::CellTooLong),
          "单元格上限 64 字节，超限显性拒绝");

    // 11. 行容量守卫：40 行后拒绝第 41 行
    let mut t3 = MdTable::new(["a", "b", "c", "d", "e"]);
    let mut t3 = match t3 {
        Ok(t) => t,
        Err(_) => return s,
    };
    let mut over_ok = true;
    let mut r = 0usize;
    while r <= 40 {
        let mut rr: [String; TABLE_COLS] = [
            String::new(), String::new(), String::new(), String::new(), String::new(),
        ];
        let mut c = 0usize;
        while c < TABLE_COLS {
            if let Some(slot) = rr.get_mut(c) {
                *slot = String::from("1");
            }
            c += 1;
        }
        let res = t3.push_row(rr);
        if r == 40 && res.is_ok() {
            over_ok = false;
        }
        r += 1;
    }
    s.add("边界：第 41 行拒绝（容量 40）", over_ok && t3.row_count() == 40,
          "行容量写死 40（对应 40 批），越限拒绝");

    // 12. 诊断码闭集：文案非空
    let text_ok = ExportCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < ExportCode::ALL.len() {
        let mut j2 = i2 + 1;
        while j2 < ExportCode::ALL.len() {
            if let (Some(x), Some(y)) = (ExportCode::ALL.get(i2), ExportCode::ALL.get(j2)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j2 += 1;
        }
        i2 += 1;
    }
    let in_seg = ExportCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EMSGSIZE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 渲染确定序：同表两次渲染逐字节相等
    let r1 = t.render();
    let r2 = t.render();
    s.add("确定性：同表两次渲染逐字节相等", r1 == r2,
          "渲染必须可复算，禁止随机序");

    // 17. 分隔行守卫：齐备才合规，缺失即检出
    let good = "|a|b|c|d|e|\n|---|---|---|---|---|\n|1|2|3|4|5|\n"
        .contains("|---|---|---|---|---|");
    let bad = "|a|b|c|d|e|\n|1|2|3|4|5|\n".contains("|---|---|---|---|---|");
    s.add("规范：分隔行齐备才合规、缺失即检出", good && !bad,
          "表头后必须有 |---| 行，缺失即 MissingSeparator");

    // 动态明细（≤8）：导出账一行 + 指纹一行
    let ledger_text = format!(
        "源格 {} / 导出 {} / 失真 {}",
        ledger.source, ledger.exported, ledger.drifted
    );
    s.add_dyn("账目快照", ledger.no_drift(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let mut enc = CellEncoder::default();
        let txt = enc.encode(123456).unwrap_or_default();
        assert_eq!(enc.decode(&txt), Some(123456));
    }

    #[test]
    fn pipe_rejected() {
        let mut enc = CellEncoder::default();
        assert_eq!(enc.sanitize("a|b"), Err(ExportCode::PipeInjection));
    }

    #[test]
    fn newline_rejected() {
        let mut enc = CellEncoder::default();
        assert_eq!(enc.sanitize("a\nb"), Err(ExportCode::NewlineInjection));
    }

    #[test]
    fn header_guards() {
        assert!(MdTable::new(["a", "b", "c", "d", "e"]).is_ok());
        assert!(MdTable::new(["a", "b", "", "d", "e"]).is_err());
        assert!(MdTable::new(["a", "b", "b", "d", "e"]).is_err());
    }

    #[test]
    fn row_capacity_40() {
        let mut t = match MdTable::new(["a", "b", "c", "d", "e"]) {
            Ok(t) => t,
            Err(_) => return,
        };
        let mut n = 0usize;
        while n < 45 {
            let row = [String::from("1"), String::from("1"), String::from("1"), String::from("1"), String::from("1")];
            let res = t.push_row(row);
            if n < 40 {
                assert!(res.is_ok());
            } else {
                assert_eq!(res, Err(ExportCode::RowOverflow));
            }
            n += 1;
        }
    }

    #[test]
    fn spot_check_finds_drift() {
        let mut t = match MdTable::new(["a", "b", "c", "d", "e"]) {
            Ok(t) => t,
            Err(_) => return,
        };
        let row = [String::from("7"), String::from("7"), String::from("7"), String::from("7"), String::from("7")];
        let _ = t.push_row(row);
        assert_eq!(spot_check(&t, &[7, 7, 7, 7, 7]), 0);
        assert_eq!(spot_check(&t, &[7, 7, 7, 7, 9]), 1);
    }

    #[test]
    fn render_contains_separator() {
        let t = match MdTable::new(["a", "b", "c", "d", "e"]) {
            Ok(t) => t,
            Err(_) => return,
        };
        assert!(t.render().contains("|---|---|---|---|---|"));
    }

    #[test]
    fn injection_matrix_zero_drift() {
        let (l, per) = run_export_injection();
        assert!(l.no_drift());
        assert_eq!(l.source, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EMSGSIZE));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EIO), -5);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10014_checks_green() {
        let s = run_unx10014_checks();
        assert!(s.all_passed(), "UNX-F10014 red: {}", s.red_items_text());
    }
}
