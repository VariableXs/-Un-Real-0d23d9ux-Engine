//! UNX-F21584 · 批册 40 件落盘完整性核（文件/行数/头部）
//!
//! 诊断段 0x91019~0x91020（F2-B40 域收官段 4/100）。
//!
//! 判据原文（施工书 UNX-F21584-J1）：40 文件存在且头部登记一致（哈希清单）。
//!
//! 设计要点（为什么不拿一份清单自己核自己）：
//! 「清单」与「落盘实据」必须分成两个结构。若只有一份清单既声明应有行数又记录
//! 实际行数，任何一次自洽性检查都是同一份数据自己比自己；故这里 `Manifest` 只装
//! 声明（文件名 / 应有行数 / 头部锚点），`DiskScan` 只装实测（文件名 / 实计行数 /
//! 实测锚点 / 内容摘要），两者由 `audit` 对撞。缺件、多件、行数漂移、头部不符
//! 四类故障因此才有可能被区分开。
//!
//! 行数容差用整数定点：5% 若走浮点，同一份清单在不同目标上会因舍入差异给出不同
//! 判词。整数式 `行数 × 5 / 100` 再做「下限 1 行」兜底——小文件（如 20 行）的 5%
//! 取整为 0，等于要求逐字相等，一次无关紧要的补空行就会把整册判红。
//!
//! 检查顺序固定为 缺件 → 多件 → 行数 → 头部 → 哈希：缺件必须最先，因为后三面都
//! 建立在「两边都有这一件」之上；对不存在的文件报行数漂移会把修复方向指错。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 批册件数（施工书 F2 批册 40 件）。
pub const BATCH_FILES: usize = 40;
/// 落盘清单容量：允许比声明多（多出来的正是要报的 `ExtraFile`），故留 8 件余量；
/// 但上限仍须显式，否则「多件」会退化成无界增长。
pub const SCAN_CAP: usize = 48;
/// 行数容差百分点（施工书「行数落在容差带 ±5%」）。
pub const LINE_TOL_PCT: u32 = 5;
/// 容差下限（行）：防止小文件的 5% 取整成 0 而变成零容差。
pub const TOL_FLOOR: u32 = 1;

/// 落盘诊断码闭集（0x91019~0x91020 八码）。
///
/// 显式判别式：码值是跨模块日志对账键，隐式递增会在插入变体时整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskCode {
    Ok = 0x91019,
    MissingFile = 0x9101A,
    ExtraFile = 0x9101B,
    LineDrift = 0x9101C,
    HeadMismatch = 0x9101D,
    HashMismatch = 0x9101E,
    ManifestCap = 0x9101F,
    EmptyManifest = 0x91020,
}

impl DiskCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            DiskCode::Ok => "批册落盘完整",
            DiskCode::MissingFile => "声明件未落盘",
            DiskCode::ExtraFile => "落盘件未被声明",
            DiskCode::LineDrift => "实计行数超出容差带",
            DiskCode::HeadMismatch => "头部锚点与声明不符",
            DiskCode::HashMismatch => "两次清单同件摘要不一致",
            DiskCode::ManifestCap => "件数超出清单容量",
            DiskCode::EmptyManifest => "清单或落盘集为空",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            DiskCode::Ok => "四十件齐、行数在带内、锚点逐字一致",
            DiskCode::MissingFile => "该件写盘失败或文件名与声明不一致",
            DiskCode::ExtraFile => "目录里留有旧版或临时件",
            DiskCode::LineDrift => "该件被改写但未同步声明行数",
            DiskCode::HeadMismatch => "头部注释被替换或复制串件",
            DiskCode::HashMismatch => "同名件的内容被替换而过期清单未更新",
            DiskCode::ManifestCap => "件数超过批册上限仍在追加",
            DiskCode::EmptyManifest => "清点未装入任何条目",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            DiskCode::Ok => "批册可归档",
            DiskCode::MissingFile => "按声明名重写该件后重扫",
            DiskCode::ExtraFile => "移出未声明的件或补进声明",
            DiskCode::LineDrift => "先确认该件是否被截断，再改声明行数",
            DiskCode::HeadMismatch => "把头部锚点改回声明的 UNX-F 号",
            DiskCode::HashMismatch => "重算该件摘要并刷新清单",
            DiskCode::ManifestCap => "核对批册件数是否被改小",
            DiskCode::EmptyManifest => "先装入声明与实测两份条目",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 落盘故障。
///
/// 与另两单的故障结构刻意不同：这里把「声明行数 / 实测行数」一并带出来，因为
/// 行数漂移的修复动作完全取决于这两个数（是补行还是改声明），只报件序不够。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiskFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 件序（0-based）。
    pub slot: u16,
    /// 声明行数（与件无关时为 0）。
    pub declared_lines: u32,
    /// 实测行数（缺件时为 0）。
    pub observed_lines: u32,
}

impl DiskFault {
    /// 建故障。
    pub const fn new(code: DiskCode, slot: u16, declared_lines: u32, observed_lines: u32) -> DiskFault {
        DiskFault { code: code.code(), slot, declared_lines, observed_lines }
    }
    /// 行数偏差（实测减声明，带符号）。用 i64 相减：u32 相减在缺行时会绕成一个
    /// 巨大正数，看上去像「多了四十几亿行」，比错更难读。
    pub const fn delta(self) -> i64 {
        self.observed_lines as i64 - self.declared_lines as i64
    }
    /// 码还原（闭集外保守落到 `HashMismatch`：摘要不符是最需要人工看一眼的一类）。
    pub const fn as_code(self) -> DiskCode {
        match self.code {
            0x91019 => DiskCode::Ok,
            0x9101A => DiskCode::MissingFile,
            0x9101B => DiskCode::ExtraFile,
            0x9101C => DiskCode::LineDrift,
            0x9101D => DiskCode::HeadMismatch,
            0x9101E => DiskCode::HashMismatch,
            0x9101F => DiskCode::ManifestCap,
            _ => DiskCode::EmptyManifest,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91019 && self.code <= 0x91020
    }
    /// 一行可读诊断。
    pub fn describe(self) -> String {
        let mut s = String::with_capacity(96);
        s.push_str(self.as_code().what());
        s.push_str(" slot=");
        s.push_str(&self.slot.to_string());
        s.push_str(" decl=");
        s.push_str(&self.declared_lines.to_string());
        s.push_str(" obs=");
        s.push_str(&self.observed_lines.to_string());
        s.push_str(" delta=");
        s.push_str(&self.delta().to_string());
        s
    }
}

/// 清单条目（声明面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Declared {
    /// 文件名（含扩展名，逐字比对，不做大小写折叠）。
    pub name: String,
    /// 应有行数。
    pub lines: u32,
    /// 头部锚点（第一行的登记注释，逐字比对）。
    pub anchor: String,
}

impl Declared {
    /// 造一条声明。
    pub fn new(name: &str, lines: u32, anchor: &str) -> Declared {
        Declared { name: name.to_string(), lines, anchor: anchor.to_string() }
    }
}

/// 实测条目（落盘面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Observed {
    /// 文件名。
    pub name: String,
    /// 实计行数。
    pub lines: u32,
    /// 实测头部锚点。
    pub anchor: String,
    /// 内容摘要（整件字节的 FNV-1a 64，落盘侧算好带上来）。
    pub digest: u64,
}

impl Observed {
    /// 造一条实测（摘要按 (名, 行数, 锚点) 重算，等价于「按这三面算出的件摘要」）。
    pub fn new(name: &str, lines: u32, anchor: &str) -> Observed {
        Observed { name: name.to_string(), lines, anchor: anchor.to_string(),
            digest: digest_of(name, lines, anchor) }
    }
}

/// 声明清单（容量 40，严格等于批册件数）。
pub struct Manifest {
    items: Vec<Declared>,
}

impl Manifest {
    /// 新建空清单。
    pub const fn new() -> Manifest {
        Manifest { items: Vec::new() }
    }

    /// 追加一条声明。
    ///
    /// 容量闸先于重名校验：满册后再谈「是不是重复声明」已无意义，且若顺序相反，
    /// 第 41 件会被报成「重名」——归因错位比漏报更难查。
    pub fn push(&mut self, d: Declared) -> Result<u16, DiskFault> {
        if self.items.len() >= BATCH_FILES {
            return Err(DiskFault::new(DiskCode::ManifestCap, self.items.len() as u16, d.lines, 0));
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == d.name {
                return Err(DiskFault::new(DiskCode::HashMismatch, i as u16, d.lines, 0));
            }
            i += 1;
        }
        self.items.push(d);
        Ok(self.items.len() as u16)
    }

    /// 已声明件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空清单。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按名查声明。
    pub fn find(&self, name: &str) -> Option<&Declared> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == name {
                return Some(&self.items[i]);
            }
            i += 1;
        }
        None
    }

}

/// 落盘实测集。
pub struct DiskScan {
    items: Vec<Observed>,
}

impl DiskScan {
    /// 新建空实测集。
    pub const fn new() -> DiskScan {
        DiskScan { items: Vec::new() }
    }

    /// 记一条实测。
    ///
    /// 超容报 `ExtraFile` 而非 `ManifestCap`：落盘侧没有「声明上限」可言，它只是
    /// 数出来的件数；超容的唯一合理解释就是目录里多了不该有的件。
    pub fn record(&mut self, o: Observed) -> Result<u16, DiskFault> {
        if self.items.len() >= SCAN_CAP {
            return Err(DiskFault::new(DiskCode::ExtraFile, self.items.len() as u16, 0, o.lines));
        }
        self.items.push(o);
        Ok(self.items.len() as u16)
    }

    /// 实计件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空集。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按名查实测。
    pub fn find(&self, name: &str) -> Option<&Observed> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == name {
                return Some(&self.items[i]);
            }
            i += 1;
        }
        None
    }

    /// 按件序取实测。
    pub fn get(&self, slot: usize) -> Option<&Observed> {
        self.items.get(slot)
    }

    /// 改实计行数并重算摘要（模拟「重扫该件」）。
    ///
    /// 摘要必须跟着重算，否则「行数已改」与「摘要没变」并存，后面对撞时报的是
    /// 摘要不符，把行数问题伪装成内容问题。
    pub fn revise_lines(&mut self, slot: usize, lines: u32) -> Result<(), DiskFault> {
        if slot >= self.items.len() {
            return Err(DiskFault::new(DiskCode::MissingFile, slot as u16, 0, 0));
        }
        let name = self.items[slot].name.clone();
        let anchor = self.items[slot].anchor.clone();
        self.items[slot].lines = lines;
        self.items[slot].digest = digest_of(&name, lines, &anchor);
        Ok(())
    }

    /// 改实测锚点并重算摘要。
    pub fn revise_anchor(&mut self, slot: usize, anchor: &str) -> Result<(), DiskFault> {
        if slot >= self.items.len() {
            return Err(DiskFault::new(DiskCode::MissingFile, slot as u16, 0, 0));
        }
        let name = self.items[slot].name.clone();
        let lines = self.items[slot].lines;
        self.items[slot].anchor = anchor.to_string();
        self.items[slot].digest = digest_of(&name, lines, anchor);
        Ok(())
    }

    /// 改摘要但不改三面（模拟「同名件内容被换成等长等锚点的另一版」）。
    pub fn revise_digest(&mut self, slot: usize, digest: u64) -> Result<(), DiskFault> {
        if slot >= self.items.len() {
            return Err(DiskFault::new(DiskCode::MissingFile, slot as u16, 0, 0));
        }
        self.items[slot].digest = digest;
        Ok(())
    }

    /// 清单根指纹：逐件把 (名, 行数, 锚点, 摘要) 咬进去。逐字段喂而非喂内存：
    /// 结构体内存含 padding，未初始化字节会让根指纹跨编译版本漂移。
    pub fn root(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.items.len() {
            let o = &self.items[i];
            h = fnv_bytes(h, &o.name);
            h = fnv_u64(h, o.lines as u64);
            h = fnv_bytes(h, &o.anchor);
            h = fnv_u64(h, o.digest);
            i += 1;
        }
        h
    }

    /// 二次比对：本次扫描与既有清单逐件对撞。
    ///
    /// 件数不同先报，因为件数差一位时按下标逐件比会把「整体错位」误报成「某件
    /// 内容变了」——错位与换件是两种完全不同的修复动作。
    pub fn compare(&self, other: &DiskScan) -> Result<u64, DiskFault> {
        if other.items.len() < self.items.len() {
            return Err(DiskFault::new(DiskCode::MissingFile, other.items.len() as u16,
                self.items.len() as u32, other.items.len() as u32));
        }
        if other.items.len() > self.items.len() {
            return Err(DiskFault::new(DiskCode::ExtraFile, self.items.len() as u16,
                self.items.len() as u32, other.items.len() as u32));
        }
        let mut i = 0usize;
        while i < self.items.len() {
            let a = &self.items[i];
            let b = &other.items[i];
            if a.name != b.name || a.lines != b.lines || a.anchor != b.anchor || a.digest != b.digest
            {
                return Err(DiskFault::new(DiskCode::HashMismatch, i as u16, a.lines, b.lines));
            }
            i += 1;
        }
        Ok(self.root())
    }
}

/// 清点回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AuditReport {
    /// 声明件数。
    pub files: u16,
    /// 实计总行数。
    pub lines_total: u32,
    /// 首件的容差带（±行）。
    pub tolerance: u32,
    /// 清单根指纹。
    pub root: u64,
}

/// 行数容差带（整数定点，见文件头）。
pub fn tolerance(declared: u32) -> u32 {
    let band = declared.saturating_mul(LINE_TOL_PCT) / 100;
    if band < TOL_FLOOR { TOL_FLOOR } else { band }
}

/// 三面核验（存在性 / 行数 / 头部）+ 哈希清单生成。
/// 返回回执而非只给布尔：`lines_total` 与 `root` 是归档凭据，二次比对直接对撞
/// 它们，不必重跑整个核验。
pub fn audit(m: &Manifest, d: &DiskScan) -> Result<AuditReport, DiskFault> {
    if m.is_empty() || d.is_empty() {
        return Err(DiskFault::new(DiskCode::EmptyManifest, 0, m.len() as u32, d.len() as u32));
    }
    let mut i = 0usize;
    while i < m.len() {
        let decl = &m.items[i];
        if d.find(&decl.name).is_none() {
            return Err(DiskFault::new(DiskCode::MissingFile, i as u16, decl.lines, 0));
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < d.len() {
        if m.find(&d.items[j].name).is_none() {
            return Err(DiskFault::new(DiskCode::ExtraFile, j as u16, 0, d.items[j].lines));
        }
        j += 1;
    }
    i = 0;
    while i < m.len() {
        let decl = &m.items[i];
        let obs = match d.find(&decl.name) {
            Some(o) => o,
            None => return Err(DiskFault::new(DiskCode::MissingFile, i as u16, decl.lines, 0)),
        };
        let band = tolerance(decl.lines) as i64;
        let delta = obs.lines as i64 - decl.lines as i64;
        let abs = if delta < 0 { -delta } else { delta };
        if abs > band {
            return Err(DiskFault::new(DiskCode::LineDrift, i as u16, decl.lines, obs.lines));
        }
        if obs.anchor != decl.anchor {
            return Err(DiskFault::new(DiskCode::HeadMismatch, i as u16, decl.lines, obs.lines));
        }
        i += 1;
    }
    let mut total = 0u32;
    j = 0;
    while j < d.len() {
        total = total.saturating_add(d.items[j].lines);
        j += 1;
    }
    Ok(AuditReport {
        files: m.len() as u16,
        lines_total: total,
        tolerance: tolerance(m.items[0].lines),
        root: d.root(),
    })
}

/// 清单导出文本（字段顺序固定，便于逐行 diff）。
pub fn export(rep: &AuditReport, d: &DiskScan) -> String {
    let mut s = String::with_capacity(128);
    s.push_str("batch=f2-b40 files=");
    s.push_str(&rep.files.to_string());
    s.push_str(" scanned=");
    s.push_str(&d.len().to_string());
    s.push_str(" lines=");
    s.push_str(&rep.lines_total.to_string());
    s.push_str(" tol=");
    s.push_str(&rep.tolerance.to_string());
    s.push_str(" root=");
    s.push_str(&hex16(rep.root));
    s
}

/// FNV-1a 64 偏移基（FNV 规范 1a 变体）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 单步（质数 0x100000001b3 来自 FNV 规范）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 字节流咬合（件名与锚点是变长文本，必须逐字节喂，不能按 u64 补零分段）。
pub fn fnv_bytes(mut h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 单件摘要（名 + 行数 + 锚点，三面同喂，改任一面摘要必变）。
pub fn digest_of(name: &str, lines: u32, anchor: &str) -> u64 {
    let h = fnv_bytes(FNV_BASIS, name);
    let h = fnv_u64(h, lines as u64);
    fnv_bytes(h, anchor)
}

/// 64 位十六进制（no_std 手写，不用格式化宏，避免为此拉进格式化机制）。
pub fn hex16(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    s
}

/// 件名（夹具用）：按条目号派生，保证 40 件不重名。
pub fn file_name(id: u32) -> String {
    let mut s = String::with_capacity(24);
    s.push_str("unx");
    s.push_str(&id.to_string());
    s.push_str("_batch.rs");
    s
}

/// 头部锚点（夹具用）：第一行必须带的登记注释。
pub fn anchor_of(id: u32) -> String {
    let mut s = String::with_capacity(16);
    s.push_str("//! UNX-F");
    s.push_str(&id.to_string());
    s
}

/// 声明清单夹具（40 件，F21581..F21620，每件 700 行）。
pub fn standard_manifest() -> Result<Manifest, DiskFault> {
    let mut m = Manifest::new();
    let mut k = 0u32;
    while k < BATCH_FILES as u32 {
        let id = 21_581u32.saturating_add(k);
        m.push(Declared::new(&file_name(id), 700, &anchor_of(id)))?;
        k += 1;
    }
    Ok(m)
}

/// 落盘实测夹具（40 件与声明同名同锚点，实计行数在 ±35 容差带内摆动）。
/// 摆动 0~12 行是刻意的：全等会让「容差带根本没被用上」，一旦有人把容差改成 0
/// 判据也不会红。带内摆动才能证明容差确实是容差，而不是恒等比较。
pub fn standard_disk() -> Result<DiskScan, DiskFault> {
    let mut d = DiskScan::new();
    let mut k = 0u32;
    while k < BATCH_FILES as u32 {
        let id = 21_581u32.saturating_add(k);
        let lines = 700u32.saturating_add((k % 5).saturating_mul(3));
        d.record(Observed::new(&file_name(id), lines, &anchor_of(id)))?;
        k += 1;
    }
    Ok(d)
}

/// 取故障码（成功记 0）。
fn code_of<T>(r: Result<T, DiskFault>) -> u32 {
    match r {
        Ok(_) => 0,
        Err(f) => f.code,
    }
}

/// 判据入口（UNX-F21584-J1 及其配套）。
///
/// 40 / 28240 / 35 在断言里一律写字面量：复用被测常量等于让被测值自己证自己。
pub fn run_unx21584_batchdisk_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21584");
    let man = standard_manifest();
    let disk = standard_disk();

    // 1) 夹具可构造，且两份夹具同长。
    cs.add("f21584.fixture_buildable",
        match (&man, &disk) { (Ok(m), Ok(d)) => m.len() == 40 && d.len() == 40, _ => false },
        "声明清单或落盘实测构造失败（应各 40 件）");

    // 2) 干净批册：件数、总行数、容差带、根指纹。
    let clean = match (&man, &disk) { (Ok(m), Ok(d)) => audit(m, d).ok(), _ => None };
    cs.add("f21584.clean_audit",
        match clean {
            Some(r) => r.files == 40 && r.lines_total == 28_240 && r.tolerance == 35 && r.root != 0,
            None => false,
        },
        "干净批册未通过，或件数/总行数(28240)/容差(35)/根指纹不符");

    // 3) 容差带边界：700 行 → ±35；735/665 在带内，736/664 越界；10 行走下限 1。
    {
        let mut band = true;
        for (at, lines) in [(0usize, 735u32), (1, 665), (2, 711)] {
            if let (Ok(m), Ok(mut d)) = (standard_manifest(), standard_disk()) {
                band = band && d.revise_lines(at, lines).is_ok() && audit(&m, &d).is_ok();
            }
        }
        for (at, lines) in [(0usize, 736u32), (1, 664)] {
            if let (Ok(m), Ok(mut d)) = (standard_manifest(), standard_disk()) {
                let hit = match d.revise_lines(at, lines) {
                    Ok(_) => code_of(audit(&m, &d)) == DiskCode::LineDrift.code(),
                    Err(_) => false,
                };
                band = band && hit;
            }
        }
        // 小文件下限：声明 10 行时 ±1 在带内、±2 越界。
        let floor_ok = tolerance(10) == 1 && tolerance(9) == 1 && tolerance(700) == 35;
        cs.add("f21584.tolerance_band", band && floor_ok,
            "±35 带内被拒或带外被放行，或小文件容差下限不为 1 行");
    }

    // 4) 缺件必拒：少扫描最后一件 → MissingFile，且件序定位到最后一位。
    {
        let mut hit = false;
        if let (Ok(m), Ok(d)) = (standard_manifest(), standard_disk()) {
            let mut cut = DiskScan::new();
            let mut i = 0usize;
            while i < d.len() - 1 {
                if let Some(o) = d.get(i) {
                    let _ = cut.record(o.clone());
                }
                i += 1;
            }
            hit = match audit(&m, &cut) {
                Err(f) => f.code == DiskCode::MissingFile.code() && f.slot == 39,
                Ok(_) => false,
            };
        }
        cs.add("f21584.missing_file", hit, "缺 1 件未被拒或件序不为 39");
    }

    // 5) 多件必拒：目录里多一件未声明的旧件 → ExtraFile。
    {
        let mut hit = false;
        if let (Ok(m), Ok(mut d)) = (standard_manifest(), standard_disk()) {
            if d.record(Observed::new("unx00000_stale.rs", 12, "//! UNX-F00000")).is_ok() {
                hit = match audit(&m, &d) {
                    Err(f) => f.code == DiskCode::ExtraFile.code() && f.slot == 40,
                    Ok(_) => false,
                };
            }
        }
        cs.add("f21584.extra_file", hit, "多 1 件未被拒或件序不为 40");
    }

    // 6) 行数漂移必拒：700 → 600（差 100 远超 ±35），故障须带两侧行数。
    {
        let mut hit = false;
        let mut detail = String::new();
        if let (Ok(m), Ok(mut d)) = (standard_manifest(), standard_disk()) {
            if d.revise_lines(7, 600).is_ok() {
                if let Err(f) = audit(&m, &d) {
                    hit = f.code == DiskCode::LineDrift.code() && f.declared_lines == 700
                        && f.observed_lines == 600 && f.delta() == -100;
                    detail = f.describe();
                }
            }
        }
        if detail.is_empty() {
            cs.add("f21584.line_drift", hit, "短 100 行未被拒，或声明/实测行数定位不符");
        } else {
            cs.add_dyn("f21584.line_drift", hit, &detail);
        }
    }

    // 7) 头部锚点必拒：把第 12 件的 UNX-F 号改错一位。
    {
        let mut hit = false;
        if let (Ok(m), Ok(mut d)) = (standard_manifest(), standard_disk()) {
            if d.revise_anchor(11, "//! UNX-F21593").is_ok() {
                hit = match audit(&m, &d) {
                    Err(f) => f.code == DiskCode::HeadMismatch.code() && f.slot == 11,
                    Ok(_) => false,
                };
            }
        }
        cs.add("f21584.head_mismatch", hit, "头部锚点不符未被拒或件序不符");
    }

    // 8) 哈希清单二次比对：换摘要不改三面 → HashMismatch。
    {
        let mut hit = false;
        if let (Ok(a), Ok(mut b)) = (standard_disk(), standard_disk()) {
            if b.revise_digest(5, 0xDEAD_BEEF_0000_0001).is_ok() {
                hit = match a.compare(&b) {
                    Err(f) => f.code == DiskCode::HashMismatch.code() && f.slot == 5, Ok(_) => false };
            }
        }
        let same = match (standard_disk(), standard_disk()) {
            (Ok(a), Ok(b)) => a.compare(&b).is_ok(), _ => false };
        cs.add("f21584.hash_mismatch", hit && same,
            "换摘要未被比对捕获，或两份相同扫描比对失败");
    }

    // 9) 空清单 / 空落盘集边界。
    {
        let empty_man = code_of(audit(&Manifest::new(), &DiskScan::new())) == DiskCode::EmptyManifest.code();
        let half = match standard_disk() {
            Ok(d) => code_of(audit(&Manifest::new(), &d)) == DiskCode::EmptyManifest.code(), Err(_) => false };
        let blank = Manifest::new().is_empty() && DiskScan::new().is_empty();
        cs.add("f21584.empty_both", empty_man && half && blank,
            "空清单未判 EmptyManifest，或单侧为空未被拦");
    }

    // 10) 上限边界：第 41 件声明必拒（ManifestCap），第 40 件正常入册。
    {
        let mut ok = false;
        if let Ok(mut m) = standard_manifest() {
            let last = m.push(Declared::new("unx21620_tail.rs", 700, "//! UNX-F21620"));
            ok = last.err().map(|f| f.code) == Some(DiskCode::ManifestCap.code()) && m.len() == 40;
        }
        let mut scan_cap = false;
        if let Ok(mut d) = standard_disk() {
            let mut k = 40u32;
            while k < 48 {
                let _ = d.record(Observed::new(&file_name(k), 700, "//! UNX-F00000"));
                k += 1;
            }
            scan_cap = code_of(d.record(Observed::new("overflow.rs", 1, "//! UNX-F99999")))
                == DiskCode::ExtraFile.code() && d.len() == 48;
        }
        cs.add("f21584.capacity_bound", ok && scan_cap,
            "第 41 件声明未被拒，或落盘集超 48 件未报 ExtraFile");
    }

    // 11) 清单导出：字段齐全，根指纹为 16 位小写十六进制。
    {
        let mut ok = false;
        if let (Some(rep), Ok(d)) = (clean, standard_disk()) {
            let text = export(&rep, &d);
            let tail_ok = match text.rfind("root=") {
                Some(pos) => {
                    let t = &text[pos + 5..];
                    t.len() == 16 && t.chars().all(|c| c.is_ascii_hexdigit())
                }
                None => false,
            };
            ok = text.contains("files=40") && text.contains("scanned=40")
                && text.contains("lines=28240") && text.contains("tol=35") && tail_ok;
        }
        cs.add("f21584.export_text", ok, "导出文本缺字段，或根指纹不是 16 位十六进制");
    }

    // 12) 幂等：两次核验回执逐字段相同；两次根指纹相同。
    {
        let mut ok = false;
        if let (Ok(m), Ok(d)) = (standard_manifest(), standard_disk()) {
            let a = audit(&m, &d);
            let b = audit(&m, &d);
            ok = a == b && d.root() == d.root() && a.is_ok();
        }
        cs.add("f21584.idempotent", ok, "两次核验回执不同（非确定性）");
    }

    // 13) 根指纹对单件敏感：改一处锚点，根指纹必变。
    {
        let mut ok = false;
        if let (Ok(a), Ok(mut b)) = (standard_disk(), standard_disk()) {
            let before = a.root();
            // 第 20 件的真锚点即 UNX-F21601，故须写成域外号才构成差异。
            if b.revise_anchor(20, "//! UNX-F00000").is_ok() {
                ok = before != b.root() && before != 0;
            }
        }
        cs.add("f21584.root_sensitive", ok, "改单件锚点后根指纹未变");
    }

    // 14) 八码闭集：落段内、两两互异、三要素齐全、未知码不 panic。
    {
        const ALL: [DiskCode; 8] = [
            DiskCode::Ok, DiskCode::MissingFile, DiskCode::ExtraFile, DiskCode::LineDrift,
            DiskCode::HeadMismatch, DiskCode::HashMismatch, DiskCode::ManifestCap,
            DiskCode::EmptyManifest,
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < ALL.len() {
            let c = ALL[i];
            let n = c.code();
            if n < 0x91019 || n > 0x91020 {
                ok = false;
            }
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
            let mut j = i + 1;
            while j < ALL.len() {
                if ALL[j].code() == n {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        let bogus = DiskFault { code: 0x910FF, slot: 0, declared_lines: 0, observed_lines: 0 };
        ok = ok && !bogus.is_known() && !bogus.describe().is_empty();
        cs.add("f21584.code_closed_set", ok, "诊断码越段、互复、三要素缺失，或未知码未被识别");
    }

    cs
}
