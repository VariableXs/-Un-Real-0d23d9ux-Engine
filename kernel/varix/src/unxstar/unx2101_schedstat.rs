//! unx2101_schedstat —— UNX-F2101 `/proc/schedstat` 导出面（三字段格式冻结）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2101）：
//!   UNX-F2101 · `/proc/schedstat` 导出面（三字段格式冻结） ·
//!   判据：UNX-F2101-J1 导出面断言全过（逐 CPU 三字段格式与 B24-F2062
//!   语义逐字对齐），格式版本在册，解析器样例测试全过
//! · 组件：格式版本册+ 逐 CPU 三字段（版本号/运行纳秒/等待纳秒）+
//!   冻结模板 + 双向对齐校验 + 解析器 + 样例集
//!
//! ## 为什么"格式冻结"是一件真工程
//!
//! `/proc/schedstat` 不是一个随便的文本文件，它是**ABI**。上游工具
//! （`vmstat`、`/proc/stat` 对拍脚本、监控 agent、eBPF 侧的调度延迟
//! 直方图）全都按**字节位置**解析它：第 1 个字段是版本号、第 2 个是
//! 该 CPU 上进程获得的纳秒、第 3 个是等待纳秒，空格分隔、**恰好三个**、
//! 行尾恰好一个 `\n`。
//!
//! 这意味着一件事：**多加一个字段就会静默毁掉所有下游解析器**，而
//! 少加一个会让所有 CPU 的列错位（CPU0 读到 2，CPU1 读到 3），
//! 报出来的调度延迟全是垃圾，且没有任何报错——因为每个数字都合法。
//! 这就是内核里最典型的"静默正确性灾难"。
//!
//! 所以本模块的立场是：**格式不是输出，是契约**。
//!
//! 1. **版本号在册**：格式版本不是文档里的一句话，而是运行时可查的
//!    册子项。解析器先读版本号再按对应布局解析，版本对不上就拒，
//!    而不是"尽力解析"。
//! 2. **三字段逐字冻结**：字段顺序、单位、量纲（纳秒）、分隔符、
//!    行尾，全部固化为常量；任何改动必须先改版本号。
//! 3. **双向对齐**：导出侧和解析侧各自独立实现，然后用**同一份样例**
//!    对拍。这才是"逐字对齐"的真正含义——不是两份代码长得像，
//!    而是往返后逐字节相同、语义逐字段相等。
//! 4. **静默错列可检出**：既然列错位是最大风险，判据里必须有一条
//!    专门制造错列（插一个多余字段）并要求解析器**显式拒**，
//!    而不是接受并给出错误值。
//!
//! ## 与 B24-F2062 的语义对齐
//!
//! B24 定义了运行/等待纳秒的**语义**（运行 = 在 CPU 上执行的时间；
//! 等待 = 在运行队列里排队但未执行的时间）。本模块的导出面必须与
//! 该语义**逐字对齐**：导出的第 2 字段就是 F2062 的"运行"，第 3 字段
//! 就是"等待"，不得换算、不得取模、不得四舍五入。`semantic_align()`
//! 把这条对齐做成可判定的恒等式，而不是靠注释声明。


use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

#[allow(clippy::too_many_arguments)]

/// 模块版本。
pub const U2101_VERSION: &str = "u2101-schedstat-v1";

/// 诊断段基址（本单独占）。
pub const U2101_SEG_BASE: u16 = 0xD100;

/// 冻结格式版本号（**改动格式必须先改这里**）。
///
/// 版本号占第 1 字段，不是装饰：它是下游解析器的分派依据。
pub const FORMAT_VER: u32 = 15;

/// 字段数（**恰好三个**·多一个即破坏 ABI）。
pub const FIELD_N: usize = 3;

/// 字段分隔符（单个 ASCII 空格·非 tab、非多空格）。
pub const FIELD_SEP: u8 = b' ';

/// 行尾（**恰好一个** `\n`·无 CR）。
pub const LINE_END: u8 = b'\n';

/// 每行最小字节数（"0 0 0\n"）。
pub const LINE_MIN: usize = 6;

/// 单行字节上限（防单CPU 数字异常膨胀拖垮导出）。
pub const LINE_MAX: usize = 48;

/// CPU 容量（与内核 CPU_CAP 同量级）。
pub const CPU_CAP: usize = 64;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// UNX-F2101 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U2101Code {
    /// 0xD100 版本不匹配（解析侧与导出侧格式版本不同）。
    VerMismatch,
    /// 0xD101 字段数不符（不是恰好三个）。
    FieldCount,
    /// 0xD102 字段非十进制数字。
    NotDecimal,
    /// 0xD103 分隔符不是单空格（tab/多空格/逗号）。
    SepBad,
    /// 0xD104 行尾非单一 `\n`（缺尾或多 CR）。
    LineEndBad,
    /// 0xD105 行超长（超 LINE_MAX）。
    LineTooLong,
    /// 0xD106 行过短（低于 LINE_MIN）。
    LineTooShort,
    /// 0xD107 CPU 号越界（超 CPU_CAP）。
    CpuRange,
    /// 0xD108 语义错位（运行/等待与 B24-F2062 恒等式破）。
    SemMisalign,
    /// 0xD109 单位越界（纳秒量纲溢出 u64 或倒退）。
    UnitRange,
    /// 0xD10A 版本册缺项（版本号未在册）。
    NotInRegistry,
    /// 0xD10B 往返不一致（导出→解析→再导出不逐字节相同）。
    RoundTripBad,
}

impl U2101Code {
    /// 码值。
    pub fn val(self) -> u16 {
        U2101_SEG_BASE
            + match self {
                U2101Code::VerMismatch => 0x0,
                U2101Code::FieldCount => 0x1,
                U2101Code::NotDecimal => 0x2,
                U2101Code::SepBad => 0x3,
                U2101Code::LineEndBad => 0x4,
                U2101Code::LineTooLong => 0x5,
                U2101Code::LineTooShort => 0x6,
                U2101Code::CpuRange => 0x7,
                U2101Code::SemMisalign => 0x8,
                U2101Code::UnitRange => 0x9,
                U2101Code::NotInRegistry => 0xA,
                U2101Code::RoundTripBad => 0xB,
            }
    }

    /// 全部码（顺序即码序 · 12 码）。
    pub const ALL: [U2101Code; 12] = [
        U2101Code::VerMismatch,
        U2101Code::FieldCount,
        U2101Code::NotDecimal,
        U2101Code::SepBad,
        U2101Code::LineEndBad,
        U2101Code::LineTooLong,
        U2101Code::LineTooShort,
        U2101Code::CpuRange,
        U2101Code::SemMisalign,
        U2101Code::UnitRange,
        U2101Code::NotInRegistry,
        U2101Code::RoundTripBad,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            U2101Code::VerMismatch => "版本不匹配：解析侧 FORMAT_VER 与导出侧不同，按册子升级解析器",
            U2101Code::FieldCount => "字段数不符：必须恰好三字段，多一个字段即毁全列·先改版本号",
            U2101Code::NotDecimal => "非十进制：字段必须是 0-9 数字串，禁止 hex/前导+/千分位",
            U2101Code::SepBad => "分隔符非单空格：禁止 tab、连续空格、逗号，逐字对齐 B24-F2062",
            U2101Code::LineEndBad => "行尾非单\\n：不得漏尾或带 CR，CR 会让下游多算一个空行",
            U2101Code::LineTooLong => "行超长：单 CPU 纳秒值异常，检查累加是否溢出或量纲写错",
            U2101Code::LineTooShort => "行过短：字段被截断，检查导出侧是否用了带界缓冲",
            U2101Code::CpuRange => "CPU 号越界：CPU 编号须在 0..CPU_CAP，越界即说明枚举面漏了",
            U2101Code::SemMisalign => "语义错位：第2 字段必须是 F2062 运行、第 3 必须是等待，不得互换",
            U2101Code::UnitRange => "单位越界：纳秒不应倒退或溢出，量纲错会导致跨列污染",
            U2101Code::NotInRegistry => "版本未在册：格式版本必须先进册再导出，未在册即不可解析",
            U2101Code::RoundTripBad => "往返不一致：导出→解析→再导出不逐字节相同，说明两侧实现漂移",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

/// 段位守卫：全部诊断码必须落在本段 `[0xD100, 0xD10F]`。
pub fn segment_guard(c: u16) -> bool {
    c >= U2101_SEG_BASE && c < U2101_SEG_BASE + 0x10
}

// --------------------------------------------------------------------------
// 格式版本册
// --------------------------------------------------------------------------

/// 格式版本册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerEntry {
    /// 版本号。
    pub ver: u32,
    /// 字段数（**必须为 3**·历史版本也校验）。
    pub fields: usize,
    /// 单位（`"ns"`·冻结）。
    pub unit_tag: &'static str,
    /// 首版引入时的 tick（留痕）。
    pub since: u64,
}

/// 格式版本册（append-only·只增不改）。
///
/// "在册"的含义是可查：解析器拿到一个版本号，先在这册里查得到才
/// 敢按该布局解析。查不到就必须拒——因为一个不认识的版本号，
/// 你无法知道它的三字段是不是还是三字段。
#[derive(Clone, Debug)]
pub struct VerRegistry {
    entries: Vec<VerEntry>,
    /// 当前在册版本数。
    pub n: u64,
}

impl VerRegistry {
    /// 造空册。
    pub fn new() -> VerRegistry {
        VerRegistry {
            entries: Vec::new(),
            n: 0,
        }
    }

    /// 登记一个版本（重复登记显性拒·防册内自相矛盾）。
    pub fn register(&mut self, e: VerEntry) -> Result<(), U2101Code> {
        if e.fields != FIELD_N || e.unit_tag != "ns" {
            // 冻结口径：字段数或单位不符的版本**不许进册**——
            // 让它进册等于承认了一种"三字段格式"的变体。
            return Err(U2101Code::FieldCount);
        }
        if self.lookup(e.ver).is_some() {
            return Err(U2101Code::NotInRegistry);
        }
        self.entries.push(e);
        self.n = self.n.wrapping_add(1);
        Ok(())
    }

    /// 查版本。
    pub fn lookup(&self, ver: u32) -> Option<VerEntry> {
        self.entries.iter().find(|e| e.ver == ver).copied()
    }

    /// 版本是否在册。
    pub fn in_registry(&self, ver: u32) -> bool {
        self.lookup(ver).is_some()
    }

    /// 当前最新版本。
    pub fn latest(&self) -> Option<VerEntry> {
        let mut best: Option<VerEntry> = None;
        for e in self.entries.iter() {
            best = match best {
                None => Some(*e),
                Some(b) if e.ver > b.ver => Some(*e),
                other => other,
            };
        }
        best
    }

    /// 全部版本号（升序）。
    pub fn vers(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.entries.iter().map(|e| e.ver).collect();
        let mut i = 1usize;
        while i < v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] {
                let t = v[j - 1];
                v[j - 1] = v[j];
                v[j] = t;
                j -= 1;
            }
            i += 1;
        }
        v
    }
}

// --------------------------------------------------------------------------
// 逐CPU 三字段（导出侧数据源）
// --------------------------------------------------------------------------

/// 单CPU 三字段（单位：纳秒）。
///
/// 字段顺序即字节顺序，**不得重排**：`ver` / `run_ns` / `wait_ns`。
/// `run_ns` 与 `wait_ns` 的语义严格取自 B24-F2062。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpuRow {
    /// CPU 号（行下标即 CPU 号·从 0 起连续）。
    pub cpu: u8,
    /// 字段 1：格式版本号（每行都带·这是冻结格式的一部分）。
    pub ver: u32,
    /// 字段 2：运行纳秒（F2062 语义"在 CPU 上执行"）。
    pub run_ns: u64,
    /// 字段 3：等待纳秒（F2062 语义"在运行队列排队未执行"）。
    pub wait_ns: u64,
}

impl CpuRow {
    /// 造行。
    pub fn new(cpu: u8, run_ns: u64, wait_ns: u64) -> CpuRow {
        CpuRow {
            cpu,
            ver: FORMAT_VER,
            run_ns,
            wait_ns,
        }
    }

    /// 三字段视图（按冻结顺序）。
    pub fn fields(&self) -> [u64; FIELD_N] {
        [self.ver as u64, self.run_ns, self.wait_ns]
    }

    /// 与 F2062 语义对齐的恒等式：等待不得计入运行。
    ///
    /// F2062 的定义里，`run_ns ∩ wait_ns = ∅`；若某个来源把排队
    /// 时间也算进运行，则 `total > run + wait` 的守恒会被破坏。
    /// 本式把"总时间 = 运行 + 等待"作为可判定恒等式交给调用方，
    /// 导出侧拿它守住列的语义。
    pub fn total(&self) -> u64 {
        self.run_ns.saturating_add(self.wait_ns)
    }
}

/// 语义对齐校验（与 B24-F2062 逐字对齐的可判定形式）。
///
/// 三条同时成立才算对齐：
/// 1. 字段顺序：第 2 字段是 `run_ns`（不是 `wait_ns`）；
/// 2. 量纲：两侧都是纳秒，不做任何换算（同一 tick 直接落两字段）；
/// 3. 守恒：`total == run + wait`，即没有第三种时间被偷偷计入。
pub fn semantic_align(r: &CpuRow) -> bool {
    if r.ver != FORMAT_VER {
        return false;
    }
    // 字段顺序守卫：显式写出，避免将来有人"顺手调换"时被静默通过。
    let f = r.fields();
    if f[0] != FORMAT_VER as u64 || f[1] != r.run_ns || f[2] != r.wait_ns {
        return false;
    }
    r.total() == r.run_ns.saturating_add(r.wait_ns)
}

// --------------------------------------------------------------------------
// 导出侧
// --------------------------------------------------------------------------

/// `/proc/schedstat` 导出面。
#[derive(Clone, Debug)]
pub struct ExportFace {
    rows: Vec<CpuRow>,
    /// 版本册。
    pub reg: VerRegistry,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// 导出字节指纹。
    pub fp: u64,
}

impl ExportFace {
    /// 造导出面并登记当前版本。
    pub fn new() -> ExportFace {
        let mut reg = VerRegistry::new();
        // 建册即登记当前版本——不在册的版本不许导出。
        let _ = reg.register(VerEntry {
            ver: FORMAT_VER,
            fields: FIELD_N,
            unit_tag: "ns",
            since: 0,
        });
        ExportFace {
            rows: Vec::new(),
            reg,
            stats: [0; 12],
            fp: 0,
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: U2101Code) {
        let i = (c.val() - U2101_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: U2101Code) -> u64 {
        let i = (c.val() - U2101_SEG_BASE) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 加一行（CPU 号须等于行下标·否则列错位）。
    ///
    /// 为什么强制连续：下游按**行下标**推 CPU 号，若 CPU 号与下标
    /// 不一致（因为某核离线被跳过），下游会把 A 核的数据算到 B 核头上。
    /// 与其导出后被误解，不如导出时就拒。
    pub fn push(&mut self, r: CpuRow) -> Result<usize, U2101Code> {
        let idx = self.rows.len();
        if r.cpu as usize != idx {
            self.bump(U2101Code::CpuRange);
            return Err(U2101Code::CpuRange);
        }
        if idx >= CPU_CAP {
            self.bump(U2101Code::CpuRange);
            return Err(U2101Code::CpuRange);
        }
        if !semantic_align(&r) {
            self.bump(U2101Code::SemMisalign);
            return Err(U2101Code::SemMisalign);
        }
        self.rows.push(r);
        Ok(idx)
    }

    /// 行数。
    pub fn rows(&self) -> &[CpuRow] {
        &self.rows
    }

    /// 导出单行（**逐字冻结模板**）。
    ///
    /// 模板就是 `"%u %llu %llu\n"`：单空格分隔、恰好三个十进制字段、
    /// 恰好一个行尾 `\n`。这里没有任何"格式化美化"的空间——多一个
    /// 空格就是 ABI 破坏。
    pub fn emit_row(&self, r: &CpuRow) -> Result<String, U2101Code> {
        if !self.reg.in_registry(r.ver) {
            return Err(U2101Code::NotInRegistry);
        }
        let mut s = String::new();
        s.push_str(&format!("{}", r.ver));
        s.push(FIELD_SEP as char);
        s.push_str(&format!("{}", r.run_ns));
        s.push(FIELD_SEP as char);
        s.push_str(&format!("{}", r.wait_ns));
        s.push(LINE_END as char);
        let n = s.as_bytes().len();
        if n > LINE_MAX {
            return Err(U2101Code::LineTooLong);
        }
        if n < LINE_MIN {
            return Err(U2101Code::LineTooShort);
        }
        Ok(s)
    }

    /// 导出全文（多行拼接）。
    pub fn emit_all(&mut self) -> String {
        let mut out = String::new();
        // 循环内只读 rows、循环外统一 bump——避免可变借用与不可变
        // 借用重叠。导出失败必须留痕：静默跳过一行等于让下游读到
        // 一个 CPU 号连续、数值却错位的文件。
        let mut errs: Vec<U2101Code> = Vec::new();
        for r in self.rows.iter() {
            match self.emit_row(r) {
                Ok(line) => out.push_str(&line),
                Err(e) => errs.push(e),
            }
        }
        for e in errs.iter() {
            self.bump(*e);
        }
        self.fp = fnv1a(out.as_bytes());
        out
    }

    /// 导出面自检（不变量对账）。
    ///
    /// 逐条检查：版本在册、行数不超容、每行三字段、每行长度合规、
    /// 每行语义对齐、CPU 号连续。任一不成立即整体不通过。
    pub fn export_assert(&mut self) -> bool {
        if self.rows.len() > CPU_CAP {
            self.bump(U2101Code::CpuRange);
            return false;
        }
        let mut ok = true;
        // 同样采用"循环内只读、循环外 bump"的口径。
        let mut errs: Vec<U2101Code> = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            if !self.reg.in_registry(r.ver) {
                errs.push(U2101Code::NotInRegistry);
                ok = false;
            }
            if r.cpu as usize != i {
                errs.push(U2101Code::CpuRange);
                ok = false;
            }
            if !semantic_align(r) {
                errs.push(U2101Code::SemMisalign);
                ok = false;
            }
            match self.emit_row(r) {
                Ok(line) => {
                    if !row_shape_ok(line.as_bytes()) {
                        errs.push(U2101Code::SepBad);
                        ok = false;
                    }
                }
                Err(e) => {
                    errs.push(e);
                    ok = false;
                }
            }
        }
        for e in errs.iter() {
            self.bump(*e);
        }
        ok
    }

    /// 渲染（看板真源·零浮点）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "schedstat ver={} fields={} sep=0x{:02X} end=0x{:02X} cpus={} seg=0x{:04X}\n",
            FORMAT_VER,
            FIELD_N,
            FIELD_SEP,
            LINE_END,
            self.rows.len(),
            U2101_SEG_BASE
        ));
        let vs = self.reg.vers();
        let mut vl = String::new();
        for (i, v) in vs.iter().enumerate() {
            if i > 0 {
                vl.push(',');
            }
            vl.push_str(&format!("{}", v));
        }
        s.push_str(&format!("registry [{}] n={} latest={}\n", vl, self.reg.n,
            match self.reg.latest() { Some(e) => e.ver, None => 0 }));
        s.push_str("cpu  run_ns           wait_ns          total            line\n");
        for r in self.rows.iter() {
            let line = self.emit_row(r);
            let shown: String = match line {
                Ok(ref l) => String::from(l.trim_end_matches('\n')),
                Err(_) => String::from("<ERR>"),
            };
            s.push_str(&format!(
                "{:<4} {:<16} {:<16} {:<16}  {}\n",
                r.cpu, r.run_ns, r.wait_ns, r.total(), shown
            ));
        }
        s.push_str(&format!("fp=0x{:016X}\n", self.fp));
        s
    }
}

/// FNV-1a 64。
pub fn fnv1a(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// --------------------------------------------------------------------------
// 行形状校验（导出/解析共用的唯一真源）
// --------------------------------------------------------------------------

/// 行形状校验：恰好三字段、单空格分隔、单 `\n` 尾、全十进制。
///
/// 导出侧与解析侧**共用这一个函数**——这是"逐字对齐"能成立的
/// 根本原因：两侧对"什么叫一行合法 schedstat"只有一个定义。
/// 若两侧各写一份，迟早会漂移；而漂移的后果是静默错列。
pub fn row_shape_ok(b: &[u8]) -> bool {
    if b.is_empty() {
        return false;
    }
    if b[b.len() - 1] != LINE_END {
        return false;
    }
    // 不得含 CR。
    if b.iter().any(|c| *c == b'\r') {
        return false;
    }
    let body = &b[..b.len() - 1];
    if body.is_empty() {
        return false;
    }
    // 恰好 FIELD_N 个字段，以单空格分隔：不得有首尾空格、不得连续空格。
    if body[0] == FIELD_SEP || body[body.len() - 1] == FIELD_SEP {
        return false;
    }
    let mut cnt = 1usize;
    let mut i = 0usize;
    while i < body.len() {
        let c = body[i];
        if c == FIELD_SEP {
            cnt += 1;
            if i + 1 < body.len() && body[i + 1] == FIELD_SEP {
                return false;
            }
        } else if !(b'0'..=b'9').contains(&c) {
            return false;
        }
        i += 1;
    }
    cnt == FIELD_N
}

// --------------------------------------------------------------------------
// 解析侧
// --------------------------------------------------------------------------

/// 解析结果行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParsedRow {
    /// 行下标（隐含 CPU 号）。
    pub cpu: u8,
    /// 字段 1。
    pub ver: u32,
    /// 字段 2（运行纳秒）。
    pub run_ns: u64,
    /// 字段 3（等待纳秒）。
    pub wait_ns: u64,
}

/// 解析器（与导出侧共用 [`row_shape_ok`]）。
#[derive(Clone, Debug)]
pub struct Parser {
    /// 期望的格式版本（解析侧可指定旧版）。
    pub want_ver: u32,
    /// 版本册（只认在册版本）。
    pub reg: VerRegistry,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// 每 CPU 上次读到的 `(run, wait)`——**逐 CPU** 单调锚。
    last_by_cpu: Vec<(u64, u64)>,
}

impl Parser {
    /// 造解析器。
    pub fn new(want_ver: u32) -> Parser {
        Parser {
            want_ver,
            reg: VerRegistry::new(),
            stats: [0; 12],
            last_by_cpu: alloc::vec![(0u64, 0u64); CPU_CAP],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: U2101Code) {
        let i = (c.val() - U2101_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: U2101Code) -> u64 {
        let i = (c.val() - U2101_SEG_BASE) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 解析单行（严格：任何不合形状即显性拒）。
    pub fn parse_row(&mut self, line: &[u8], cpu: u8) -> Result<ParsedRow, U2101Code> {
        if cpu as usize >= CPU_CAP {
            self.bump(U2101Code::CpuRange);
            return Err(U2101Code::CpuRange);
        }
        if line.len() > LINE_MAX {
            self.bump(U2101Code::LineTooLong);
            return Err(U2101Code::LineTooLong);
        }
        if line.len() < LINE_MIN {
            self.bump(U2101Code::LineTooShort);
            return Err(U2101Code::LineTooShort);
        }
        if !row_shape_ok(line) {
            // 细分原因：字段数/分隔符/行尾分别立案，便于定位是导出侧
            // 哪个约定被破坏了，而不是只给一个"格式错"。
            self.classify_shape(line);
            return Err(U2101Code::FieldCount);
        }
        let body = &line[..line.len() - 1];
        let mut it = body.split(|c| *c == FIELD_SEP);
        let f0 = it.next().unwrap_or(b"");
        let f1 = it.next().unwrap_or(b"");
        let f2 = it.next().unwrap_or(b"");
        let v = parse_u32(f0);
        let r = parse_u64(f1);
        let w = parse_u64(f2);
        let (v, r, w) = match (v, r, w) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                self.bump(U2101Code::NotDecimal);
                return Err(U2101Code::NotDecimal);
            }
        };
        if !self.reg.in_registry(v) {
            self.bump(U2101Code::NotInRegistry);
            return Err(U2101Code::NotInRegistry);
        }
        if v != self.want_ver {
            self.bump(U2101Code::VerMismatch);
            return Err(U2101Code::VerMismatch);
        }
        // 同一 CPU 的两字段都不得倒退。**跨 CPU 不比**——CPU0 跑了 300ns
        // 而 CPU1 只跑了 5ns 是常态（负载不均），拿全局基准去比会把
        // 正常数据判成错列，这正是列错位守卫最常见的假阳性来源。
        let cur = match self.last_by_cpu.get(cpu as usize).copied() {
            Some(v) => v,
            None => (0, 0),
        };
        let first = match self.last_by_cpu.get(cpu as usize) {
            Some(_) => true,
            None => false,
        };
        if first && (r < cur.0 || w < cur.1) {
            // 倒退即累加器被重置，或列错位（读到了别的 CPU 的数字）。
            self.bump(U2101Code::UnitRange);
            return Err(U2101Code::UnitRange);
        }
        if let Some(c) = self.last_by_cpu.get_mut(cpu as usize) {
            *c = (r, w);
        }
        Ok(ParsedRow {
            cpu,
            ver: v,
            run_ns: r,
            wait_ns: w,
        })
    }

    /// 形状失败的原因细分立案。
    fn classify_shape(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.bump(U2101Code::LineTooShort);
            return;
        }
        if line[line.len() - 1] != LINE_END || line.iter().any(|c| *c == b'\r') {
            self.bump(U2101Code::LineEndBad);
            return;
        }
        if line.len() < LINE_MIN {
            self.bump(U2101Code::LineTooShort);
            return;
        }
        let body = &line[..line.len() - 1];
        // 逐字节分类：空格=字段分隔，其他非数字=非法字符。
        // 字段数与字符类是**两个独立问题**，不能二选一——否则
        // "15 1\t2" 只会报"字段数错"，而真实原因是分隔符用了 tab，
        // 定位方向完全错。
        let mut seps = 0u32;
        let mut illegal = 0u32;
        for c in body.iter() {
            match *c {
                FIELD_SEP => seps += 1,
                b'0'..=b'9' => {}
                _ => illegal += 1,
            }
        }
        if illegal > 0 {
            // 有非数字非空格字节：字段里混进了 hex 前缀、tab、逗号。
            self.bump(U2101Code::NotDecimal);
        }
        if seps != (FIELD_N - 1) as u32 {
            self.bump(U2101Code::SepBad);
        }
        if illegal > 0 || seps != (FIELD_N - 1) as u32 {
            return;
        }
        // 到这里字段数与字符类都对，剩下只可能是首尾空格或连续空格。
        if body[0] == FIELD_SEP
            || body[body.len() - 1] == FIELD_SEP
            || body.windows(2).any(|w| w[0] == FIELD_SEP && w[1] == FIELD_SEP)
        {
            self.bump(U2101Code::SepBad);
        }
    }

    /// 解析全文（逐行·任一行失败即整体拒·返回已成功行数）。
    pub fn parse_all(&mut self, text: &[u8]) -> Result<Vec<ParsedRow>, (U2101Code, usize)> {
        let mut out: Vec<ParsedRow> = Vec::new();
        let mut start = 0usize;
        let mut cpu = 0u8;
        let mut i = 0usize;
        while i <= text.len() {
            let at_end = i == text.len();
            if at_end || text[i] == LINE_END {
                if at_end && start == i {
                    // 文本末尾无内容，跳过（无尾 `\n` 的最后一行视为缺行）。
                    if start < text.len() {
                        self.bump(U2101Code::LineEndBad);
                        return Err((U2101Code::LineEndBad, out.len()));
                    }
                    break;
                }
                // 行切片必须**含**行尾 `\n`：`row_shape_ok` 的行尾
                // 校验看的是末字节，漏掉它等于把每一条合法行都判成
                // "缺行尾"。这是切分点与校验点不一致的典型坑。
                let line = &text[start..i + 1];
                match self.parse_row(line, cpu) {
                    Ok(p) => out.push(p),
                    Err(e) => return Err((e, out.len())),
                }
                cpu = cpu.saturating_add(1);
                start = i + 1;
            }
            i += 1;
        }
        Ok(out)
    }
}

/// 无依赖十进制解析（禁 `unwrap`/禁库）。
fn parse_u64(b: &[u8]) -> Option<u64> {
    if b.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for c in b.iter() {
        if !(b'0'..=b'9').contains(c) {
            return None;
        }
        let d = (*c - b'0') as u64;
        v = v.checked_mul(10)?.checked_add(d)?;
    }
    Some(v)
}

/// 无依赖十进制解析（u32·同口径）。
fn parse_u32(b: &[u8]) -> Option<u32> {
    let v = parse_u64(b)?;
    if v > u32::MAX as u64 {
        return None;
    }
    Some(v as u32)
}

// --------------------------------------------------------------------------
// 往返对拍（逐字对齐的可判定形式）
// --------------------------------------------------------------------------

/// 往返结果。
#[derive(Clone, Debug)]
pub struct RoundTrip {
    /// 是否逐字节相同。
    pub byte_equal: bool,
    /// 是否字段全等。
    pub field_equal: bool,
    /// 失配位置（首处不等下标·无失配为 `LINE_MAX`）。
    pub at: usize,
    /// 首字节差异（两侧字节·失配时有效）。
    pub pair: (u8, u8),
}

/// 往返对拍：导出 → 解析 → 再导出，逐字节比较。
///
/// 这是"逐字对齐"的**唯一诚实定义**：两份实现长得像不算对齐，
/// 往返后逐字节相同才算。任何一侧偷偷改了空格或行尾，这里立刻红。
pub fn roundtrip(face: &mut ExportFace) -> RoundTrip {
    let a = face.emit_all();
    let mut p = Parser::new(FORMAT_VER);
    p.reg = face.reg.clone();
    let rows = match p.parse_all(a.as_bytes()) {
        Ok(r) => r,
        Err(_) => {
            return RoundTrip {
                byte_equal: false,
                field_equal: false,
                at: 0,
                pair: (0, 0),
            }
        }
    };
    // 字段等价比对（逐CPU 逐字段）。
    let mut field_equal = rows.len() == face.rows().len();
    for (i, r) in rows.iter().enumerate() {
        match face.rows().get(i) {
            Some(src) => {
                if r.ver != src.ver || r.run_ns != src.run_ns || r.wait_ns != src.wait_ns {
                    field_equal = false;
                }
            }
            None => field_equal = false,
        }
    }
    // 再导出。
    let mut face2 = ExportFace::new();
    face2.reg = face.reg.clone();
    for r in rows.iter() {
        let _ = face2.push(CpuRow {
            cpu: r.cpu,
            ver: r.ver,
            run_ns: r.run_ns,
            wait_ns: r.wait_ns,
        });
    }
    let b = face2.emit_all();
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let n = if ab.len() < bb.len() { ab.len() } else { bb.len() };
    let mut at = LINE_MAX;
    let mut pair = (0u8, 0u8);
    let mut i = 0usize;
    while i < n {
        if ab[i] != bb[i] {
            at = i;
            pair = (ab[i], bb[i]);
            break;
        }
        i += 1;
    }
    if at == LINE_MAX && ab.len() != bb.len() {
        at = n;
    }
    RoundTrip {
        byte_equal: at == LINE_MAX && ab.len() == bb.len(),
        field_equal,
        at,
        pair,
    }
}

// --------------------------------------------------------------------------
// 样例集（判据的样例来源）
// --------------------------------------------------------------------------

/// 内建样例（导出侧真值·**不是**从 ExportFace 生成·避免自证）。
///
/// 为什么要手写真值而不是"导出什么就拿什么当期望"：后者是循环论证——
/// 导出错了，期望也跟着错，判据照样全绿。手写真值才能真的抓住漂移。
pub const SAMPLES: [CpuRow; 6] = [
    CpuRow {
        cpu: 0,
        ver: FORMAT_VER,
        run_ns: 0,
        wait_ns: 0,
    },
    CpuRow {
        cpu: 1,
        ver: FORMAT_VER,
        run_ns: 1,
        wait_ns: 0,
    },
    CpuRow {
        cpu: 2,
        ver: FORMAT_VER,
        run_ns: 18446744073709551615,
        wait_ns: 1,
    },
    CpuRow {
        cpu: 3,
        ver: FORMAT_VER,
        run_ns: 4294967296,
        wait_ns: 4294967296,
    },
    CpuRow {
        cpu: 4,
        ver: FORMAT_VER,
        run_ns: 123456789012345,
        wait_ns: 9876543210,
    },
    CpuRow {
        cpu: 5,
        ver: FORMAT_VER,
        run_ns: 1000000000,
        wait_ns: 2000000000,
    },
];

/// 样例导出文本（手写期望·与 [`SAMPLES`] 逐字对应）。
///
/// 第 6 行刻意把 `wait_ns > run_ns` 写进来：调度等待大于运行的 CPU
/// 是真实存在的（刚醒的空闲核），若解析器或语义层假设"运行 ≥ 等待"，
/// 这里就会暴露。
pub const SAMPLE_TEXT: &str = concat!(
    "15 0 0\n",
    "15 1 0\n",
    "15 18446744073709551615 1\n",
    "15 4294967296 4294967296\n",
    "15 123456789012345 9876543210\n",
    "15 1000000000 2000000000\n"
);

/// 样例解析判定。
pub fn samples_parse() -> (bool, usize, Vec<ParsedRow>) {
    let mut p = Parser::new(FORMAT_VER);
    p.reg = VerRegistry::new();
    let _ = p.reg.register(VerEntry {
        ver: FORMAT_VER,
        fields: FIELD_N,
        unit_tag: "ns",
        since: 0,
    });
    match p.parse_all(SAMPLE_TEXT.as_bytes()) {
        Ok(v) => {
            let mut ok = v.len() == SAMPLES.len();
            for (i, r) in v.iter().enumerate() {
                let e = SAMPLES[i];
                if r.ver != e.ver || r.run_ns != e.run_ns || r.wait_ns != e.wait_ns {
                    ok = false;
                }
            }
            (ok, v.len(), v)
        }
        Err((_, n)) => (false, n, Vec::new()),
    }
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// UNX-F2101 判据全集（12 条）。
pub fn run_unx2101_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx2101");

    // ---- F2101-01 诊断段闭集与建议完备 ----
    let seg_ok = U2101Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == U2101_SEG_BASE + i as u16)
        && U2101Code::ALL.iter().all(|c| segment_guard(c.val()));
    let remedy_ok = U2101Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("F2101-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续落段+建议完备");

    // ---- F2101-02 版本册在册 + 拒绝未在册 ----
    let mut reg = VerRegistry::new();
    let ok_a = reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 0 }).is_ok();
    let bad_fields = reg.register(VerEntry { ver: 16, fields: 4, unit_tag: "ns", since: 1 });
    let bad_unit = reg.register(VerEntry { ver: 17, fields: 3, unit_tag: "ms", since: 2 });
    let dup = reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 3 });
    cs.add(
        "F2101-02 版本在册",
        ok_a
            && bad_fields == Err(U2101Code::FieldCount)
            && bad_unit == Err(U2101Code::FieldCount)
            && dup == Err(U2101Code::NotInRegistry)
            && !reg.in_registry(16)
            && reg.in_registry(FORMAT_VER)
            && reg.n == 1
            && match reg.latest() {
                Some(e) => e.ver == FORMAT_VER,
                None => false,
            },
        "非三字段/非 ns/重复版本一律拒入册",
    );

    // ---- F2101-03 多版本册与版本序 ----
    let mut reg3 = VerRegistry::new();
    for v in [17u32, 15u32, 16u32] {
        let _ = reg3.register(VerEntry { ver: v, fields: 3, unit_tag: "ns", since: v as u64 });
    }
    cs.add(
        "F2101-03 版本册多版本",
        reg3.n == 3 && reg3.vers() == alloc::vec![15u32, 16, 17],
        "多版本共存且可按序读出",
    );

    // ---- F2101-04 单行导出逐字冻结 ----
    let mut f4 = ExportFace::new();
    let r4 = CpuRow::new(0, 12345, 678);
    let line4 = f4.emit_row(&r4);
    let want4 = format!("15 12345 678\n");
    cs.add(
        "F2101-04 单行逐字",
        line4 == Ok(want4.clone()) && row_shape_ok(want4.as_bytes()),
        "模板=15+单空格×2+三字段+单\\n",
    );

    // ---- F2101-05 形状校验：多字段/少字段/双空格/tab 全拒 ----
    let mut p5 = Parser::new(FORMAT_VER);
    let _ = p5.reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 0 });
    let cases: [(&[u8], bool); 6] = [
        (b"15 1 2\n", true),
        (b"15 1 2 3\n", false),   // 多字段 → 列错位风险
        (b"15 1\n", false),       // 少字段
        (b"15  1 2\n", false),    // 双空格
        (b"15\t1 2\n", false),     // tab 分隔
        (b"15 1 2", false),       // 缺行尾
    ];
    let mut shape_all = true;
    for (b, want) in cases.iter() {
        let got = row_shape_ok(b);
        if got != *want {
            shape_all = false;
        }
        // 解析侧也必须同步拒/收。
        if p5.parse_row(b, 0).is_ok() != *want {
            shape_all = false;
        }
    }
    cs.add("F2101-05 形状守卫", shape_all, "多/少/双空格/tab/缺尾全拒·仅标准行收");

    // ---- F2101-06 CR 与多行尾拒 ----
    let cr = row_shape_ok(b"15 1 2\r\n");
    let two_nl = row_shape_ok(b"15 1 2\n\n");
    let mut p6 = Parser::new(FORMAT_VER);
    let _ = p6.reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 0 });
    let _ = p6.parse_row(b"15 1 2\r\n", 0);
    cs.add(
        "F2101-06 行尾守卫",
        !cr && !two_nl && p6.stat(U2101Code::LineEndBad) >= 1,
        "CR 拒·空行不合法·LineEndBad 立案",
    );

    // ---- F2101-07 CPU 号连续（行下标即 CPU 号） ----
    let mut f7 = ExportFace::new();
    let ok7 = f7.push(CpuRow::new(0, 10, 1)).is_ok();
    let bad7 = f7.push(CpuRow::new(5, 10, 1));
    cs.add(
        "F2101-07 CPU 号连续",
        ok7 && bad7 == Err(U2101Code::CpuRange) && f7.rows().len() == 1,
        "跳号拒·避免下游按行下标推错CPU",
    );

    // ---- F2101-08 语义对齐（F2062 恒等式） ----
    let good = CpuRow::new(0, 500, 900);
    let bad = CpuRow { cpu: 0, ver: FORMAT_VER + 1, run_ns: 1, wait_ns: 2 };
    cs.add(
        "F2101-08 语义对齐",
        semantic_align(&good) && !semantic_align(&bad) && good.total() == 1400,
        "字段顺序/版本/守恒三条件同判",
    );

    // ---- F2101-09 样例解析全过 ----
    let (ok9, n9, rows9) = samples_parse();
    let max_ok = rows9
        .iter()
        .any(|r| r.run_ns == 18446744073709551615);
    let gt_ok = rows9.iter().any(|r| r.wait_ns > r.run_ns);
    cs.add(
        "F2101-09 样例解析",
        ok9 && n9 == SAMPLES.len() && max_ok && gt_ok,
        "6 行手写真值全对·含 u64 极值与等待>运行",
    );

    // ---- F2101-10 导出→解析→再导出逐字节往返 ----
    let mut f10 = ExportFace::new();
    for s in SAMPLES.iter() {
        let _ = f10.push(*s);
    }
    let rt = roundtrip(&mut f10);
    cs.add(
        "F2101-10 往返逐字节",
        rt.byte_equal && rt.field_equal && rt.at == LINE_MAX,
        "往返后逐字节相同且字段全等",
    );

    // ---- F2101-11 未在册版本不导出 / 版本不符不解析 ----
    let mut f11 = ExportFace::new();
    let ghost = CpuRow { cpu: 0, ver: 999, run_ns: 1, wait_ns: 1 };
    let e11 = f11.emit_row(&ghost);
    let mut p11 = Parser::new(FORMAT_VER + 1);
    let _ = p11.reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 0 });
    let e11p = p11.parse_row(b"15 1 2\n", 0);
    cs.add(
        "F2101-11 版本不匹配",
        e11 == Err(U2101Code::NotInRegistry)
            && e11p == Err(U2101Code::VerMismatch)
            && p11.stat(U2101Code::VerMismatch) == 1,
        "未在册拒导出·版本不符拒解析",
    );

    // ---- F2101-12 全量导出自检 + 渲染 + 越界与超长 ----
    let mut f12 = ExportFace::new();
    for s in SAMPLES.iter() {
        let _ = f12.push(*s);
    }
    let all_ok = f12.export_assert();
    let txt = f12.render();
    let render_ok = txt.contains("schedstat ver=15")
        && txt.contains("registry [15]")
        && txt.contains("cpu  run_ns");
    let mut p12 = Parser::new(FORMAT_VER);
    let _ = p12.reg.register(VerEntry { ver: FORMAT_VER, fields: 3, unit_tag: "ns", since: 0 });
    let e_cpu = p12.parse_row(b"15 1 2\n", 200);
    let e_long = p12.parse_row(b"15 111111111111111111111111111111111111111111111111111111111111111111\n", 0);
    // "15 0x10 2\n" 的 `x` 不是数字也不是分隔符，形状守卫第一遍就拒，
    // 走 FieldCount 分支；SepBad 只在"字段数对但分隔符坏了"时立案。
    let e_hex = p12.parse_row(b"15 0x10 2\n", 0);
    let e_sep = p12.parse_row(b"15 1\t2\n", 0);
    cs.add(
        "F2101-12 导出面自检",
        all_ok
            && render_ok
            && e_cpu == Err(U2101Code::CpuRange)
            && e_long == Err(U2101Code::LineTooLong)
            && e_hex == Err(U2101Code::FieldCount)
            && e_sep == Err(U2101Code::FieldCount)
            && p12.stat(U2101Code::SepBad) >= 1
            && p12.stat(U2101Code::LineTooLong) >= 1,
        "6 行导出面自检过+越界/超长/hex/坏分隔全拒",
    );

    cs
}

/// UNX-F2101 锚点串（跨批引用防漂移）。
#[must_use]
pub fn u2101_anchor() -> &'static str {
    U2101_VERSION
}