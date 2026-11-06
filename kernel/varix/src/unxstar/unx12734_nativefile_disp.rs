//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12734 · 文件族 NativeFile* 对照回归（disposition 五档） · 0x82180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12735-J1 注册表族对照回归绿，键路径语义维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NativeFile* 五操作对照闭集（Open/Read/Write/QueryInformation/Close，
//!     NT 原生文件 API 语义）：句柄表定容 8 槽，开关与操作全记账；
//!   - disposition 五档闭集（完成/挂起/重试/否决/故障）：每请求恰结算一次
//!     （settle-once 单调，重结算显性拒），五档全出现即对照回归绿；
//!   - 注册表族对照回归（判据锚点）：\Registry\* 根与 \Device\* 根共走
//!     同一条键路径归一化管线，两族根闭集冻结、嵌套前缀语义同构；
//!   - 键路径语义维持（判据锚点）：分隔符折叠（\ 与 / 同权、重复/尾随
//!     折叠）、节点字符闭集校验、深度上限、大小写不敏感同一性；
//!   - 注入三族（键路径违约/重复结算/句柄耗尽）各 10 轮必抓全抓；
//!   - LCG 种子写死 = SEG_BASE（0x82180）；no_std + alloc；
//!     零 std/零浮点/零 IO/零墙钟/零随机/零 unwrap/零裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（VX9 公式：0x80000 + 0x40 × (12734−12600) = 0x82180）。
pub const SEG_BASE: u32 = 0x82180;

// ── 冻结常量表 ─────────────────────────────────────────────────────
/// 句柄表容量（NativeFile 句柄并发上限；冻结值）。
pub const HANDLE_CAP: usize = 8;
/// 键路径深度上限（根两节 + 业务六节；冻结值）。
pub const MAX_KEY_DEPTH: usize = 8;
/// 五档闭集档数（disposition 对照面）。
pub const DISP_TIERS: usize = 5;
/// 文件操作闭集数（Open/Read/Write/Query/Close 五操作）。
pub const FILE_OP_N: usize = 5;

/// 两节根闭集（出处：NT 对象命名空间惯例——注册表对象根 \Registry\*、
/// 设备对象根 \Device\*；大小写不敏感，配对冻结）。
pub const ROOT_PAIRS: [(&'static str, &'static str); 4] = [
    ("Registry", "Machine"),
    ("Registry", "User"),
    ("Device", "HarddiskVolume1"),
    ("Device", "HarddiskVolume2"),
];

/// 五档冻结状态值（NTSTATUS 惯例三段对照；值冻结，仅作档位指纹）。
pub const DISP_STATUS: [u32; DISP_TIERS] = [
    0x0000_0000, // 完成 STATUS_SUCCESS
    0x0000_0103, // 挂起 STATUS_PENDING
    0x8000_0011, // 重试（警告段对照值）
    0xC000_0022, // 否决 STATUS_ACCESS_DENIED 对照值
    0xC000_0005, // 故障 STATUS_ACCESS_VIOLATION 对照值
];

// ── 诊断码闭集（段内 0x40 窗口，10 码） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileDiag {
    /// 空路径（无节点可归一）
    PathEmpty,
    /// 节点字符越闭集（合法集：字母/数字/_ - . 空格）
    PathCharBad,
    /// 键路径越深度上限（>8 节）
    PathDepthOver,
    /// 首两节不落在根闭集内（根未知）
    RootUnknown,
    /// 句柄表满（容量耗尽）
    HandleExhausted,
    /// 句柄越表（越界或未开）
    HandleBad,
    /// 关闭后操作（策略否决）
    OpAfterClose,
    /// 重复结算（settle-once 破坏）
    SettleReplay,
    /// 对账不守恒（Σ文件操作账≠表完成账）
    LedgerBreak,
    /// 操作闭集外（NativeFile* 五操作之外）
    OpClosedOff,
}

impl FileDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FileDiag::PathEmpty => "空路径：至少给出根两节",
            FileDiag::PathCharBad => "节点字符越闭集：限字母数字与_- . 空格",
            FileDiag::PathDepthOver => "路径越深度上限：8 节之内重试",
            FileDiag::RootUnknown => "根未知：Registry/* 或 Device/* 之内",
            FileDiag::HandleExhausted => "句柄表满：先关旧句柄再开新文件",
            FileDiag::HandleBad => "句柄越表：0..8 之内的有效句柄",
            FileDiag::OpAfterClose => "关闭后操作：先重开再操作",
            FileDiag::SettleReplay => "重复结算：一请求恰结算一次",
            FileDiag::LedgerBreak => "对账不守恒：Σ文件操作账≠表完成账",
            FileDiag::OpClosedOff => "操作越闭集：五操作之内",
        }
    }
    pub const ALL: [FileDiag; 10] = [
        FileDiag::PathEmpty,
        FileDiag::PathCharBad,
        FileDiag::PathDepthOver,
        FileDiag::RootUnknown,
        FileDiag::HandleExhausted,
        FileDiag::HandleBad,
        FileDiag::OpAfterClose,
        FileDiag::SettleReplay,
        FileDiag::LedgerBreak,
        FileDiag::OpClosedOff,
    ];
}

/// 诊断码查表（闭集返回；未知码给中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FileDiag::ALL.len() {
        if let Some(c) = FileDiag::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82180 段闭集内"
}

// ── 键路径语义（判据锚点：键路径语义维持） ─────────────────────────
/// 键路径：归一化节点序列（注册表族与文件族共走同一条管线）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeyPath {
    /// 归一化节点（分隔符已折叠、空段已弃、字符已校验）
    pub nodes: Vec<String>,
}

/// NativeFile* 五操作闭集（NT 原生文件 API 语义对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOp {
    /// NtOpenFile 对照（表级操作，不占操作账）
    Open,
    /// NtReadFile 对照
    Read,
    /// NtWriteFile 对照
    Write,
    /// NtQueryInformationFile 对照
    Query,
    /// NtClose 对照（表级操作，不占操作账）
    Close,
}

impl FileOp {
    /// 操作名（对照回归文案面）。
    pub fn name(self) -> &'static str {
        match self {
            FileOp::Open => "Open",
            FileOp::Read => "Read",
            FileOp::Write => "Write",
            FileOp::Query => "Query",
            FileOp::Close => "Close",
        }
    }
    /// 是否占文件操作账（Open/Close 为表级操作）。
    pub fn counts(self) -> bool {
        match self {
            FileOp::Read | FileOp::Write | FileOp::Query => true,
            FileOp::Open | FileOp::Close => false,
        }
    }
}

/// disposition 五档闭集（判据锚点：五档对照回归绿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileDisp {
    /// 完成：合法操作走通
    Completed,
    /// 挂起：文件忙，请求在途待续
    Pending,
    /// 重试：重试窗内暂缓一次
    Retry,
    /// 否决：策略拒（坏路径/关闭后操作）
    Vetoed,
    /// 故障：句柄越表/参数错
    Faulted,
}

impl FileDisp {
    /// 档位序号（守恒账下标）。
    pub fn tier(self) -> usize {
        match self {
            FileDisp::Completed => 0,
            FileDisp::Pending => 1,
            FileDisp::Retry => 2,
            FileDisp::Vetoed => 3,
            FileDisp::Faulted => 4,
        }
    }
    /// 档位中文名（对照文案面）。
    pub fn name(self) -> &'static str {
        match self {
            FileDisp::Completed => "完成",
            FileDisp::Pending => "挂起",
            FileDisp::Retry => "重试",
            FileDisp::Vetoed => "否决",
            FileDisp::Faulted => "故障",
        }
    }
    /// 冻结状态值（档位指纹）。
    pub fn status(self) -> u32 {
        match DISP_STATUS.get(self.tier()) {
            Some(v) => *v,
            None => 0xFFFF_FFFF,
        }
    }
}

impl KeyPath {
    /// 节点字符闭集校验：限字母/数字/_/-/./空格（冻结语义）。
    fn charset_ok(node: &str) -> bool {
        for &b in node.as_bytes().iter() {
            let ok = (b >= b'a' && b <= b'z')
                || (b >= b'A' && b <= b'Z')
                || (b >= b'0' && b <= b'9')
                || b == b'_'
                || b == b'-'
                || b == b'.'
                || b == b' ';
            if !ok {
                return false;
            }
        }
        true
    }

    /// 入表一节：字符校验 + 深度上限。
    fn push_node(nodes: &mut Vec<String>, node: &str) -> Result<(), FileDiag> {
        if !Self::charset_ok(node) {
            return Err(FileDiag::PathCharBad);
        }
        if nodes.len() + 1 > MAX_KEY_DEPTH {
            return Err(FileDiag::PathDepthOver);
        }
        nodes.push(String::from(node));
        Ok(())
    }

    /// 解析归一化：分隔符 \\ 与 / 同权、重复/首尾分隔符折叠、
    /// 空路径拒、节点字符闭集校验、深度上限、首两节根闭集校验。
    pub fn parse(input: &str) -> Result<KeyPath, FileDiag> {
        if input.is_empty() {
            return Err(FileDiag::PathEmpty);
        }
        let mut nodes: Vec<String> = Vec::new();
        let mut cur = String::new();
        let bytes = input.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(b) => *b,
                None => break,
            };
            if b == b'\\' || b == b'/' {
                if !cur.is_empty() {
                    Self::push_node(&mut nodes, &cur)?;
                    cur = String::new();
                }
                // 空段折叠（重复/首尾分隔符）。
            } else {
                // 非闭集字符（含多字节 UTF-8 尾字节）将由 charset_ok 拒。
                cur.push(b as char);
            }
            i += 1;
        }
        if !cur.is_empty() {
            Self::push_node(&mut nodes, &cur)?;
        }
        if nodes.len() < 2 {
            return Err(FileDiag::PathEmpty);
        }
        // 根闭集：首两节必须 ci 匹配四组冻结根对之一。
        let rooted = {
            let mut hit = false;
            let mut r = 0usize;
            while r < ROOT_PAIRS.len() {
                if let Some((a, b)) = ROOT_PAIRS.get(r) {
                    let n0 = match nodes.get(0) {
                        Some(n) => n.as_str(),
                        None => "",
                    };
                    let n1 = match nodes.get(1) {
                        Some(n) => n.as_str(),
                        None => "",
                    };
                    if n0.eq_ignore_ascii_case(a) && n1.eq_ignore_ascii_case(b) {
                        hit = true;
                    }
                }
                r += 1;
            }
            hit
        };
        if !rooted {
            return Err(FileDiag::RootUnknown);
        }
        Ok(KeyPath { nodes })
    }

    /// 规范式：\Node1\Node2\...（保留原文大小写的呈现形）。
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                out.push('\\');
                out.push_str(n);
            }
            i += 1;
        }
        out
    }

    /// 同一指纹式：全节点 ASCII 小写的规范形
    /// （键路径语义核心基准形——大小写不敏感同一性的对照凭证）。
    pub fn canon_ci(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                out.push('\\');
                for c in n.chars() {
                    out.push(c.to_ascii_lowercase());
                }
            }
            i += 1;
        }
        out
    }

    /// 大小写不敏感同一性（键路径语义核心：同形即同键）。
    pub fn eq_ci(&self, other: &KeyPath) -> bool {
        if self.nodes.len() != other.nodes.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            match (self.nodes.get(i), other.nodes.get(i)) {
                (Some(a), Some(b)) => {
                    if !a.eq_ignore_ascii_case(b) {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 嵌套语义：self 是否落在 parent 之下（父为真前缀，ci 比较）。
    pub fn is_under(&self, parent: &KeyPath) -> bool {
        if parent.nodes.len() >= self.nodes.len() {
            return false;
        }
        let mut i = 0usize;
        while i < parent.nodes.len() {
            match (self.nodes.get(i), parent.nodes.get(i)) {
                (Some(a), Some(b)) => {
                    if !a.eq_ignore_ascii_case(b) {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

// ── NativeFile 句柄表 ──────────────────────────────────────────────
/// 已开文件：路径 + 开关态 + 操作账。
#[derive(Clone)]
pub struct NativeFile {
    /// 句柄号
    pub handle: usize,
    /// 归一化键路径
    pub path: KeyPath,
    /// 开关态
    pub open: bool,
    /// 读/写/查询操作账
    pub reads: u32,
    pub writes: u32,
    pub queries: u32,
    /// 文件忙窗（挂起档触发面）
    pub busy: bool,
}

/// 句柄表：定容 8 槽 + 五档守恒账 + 在途请求。
#[derive(Clone)]
pub struct HandleTable {
    pub slots: [Option<NativeFile>; HANDLE_CAP],
    /// 五档守恒账
    pub disp_counts: [u32; DISP_TIERS],
    /// 总账（每结算恰 +1）
    pub total: u32,
    /// 完成账（完成档专用，对账用）
    pub completed: u32,
    /// 已关闭条目的残留操作账（槽位复用时转入，对账不断链）
    pub retired_ops: u32,
    /// 在途请求（settle-once 单调核）
    pub parked: Option<(usize, FileOp)>,
    /// 重试窗（单发自清）
    pub retry_window: bool,
    /// 开/关累计账
    pub opens: u32,
    pub closes: u32,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable {
            // NativeFile 含堆型路径（KeyPath），非 Copy——用 from_fn 定容建表。
            slots: core::array::from_fn(|_| None),
            disp_counts: [0u32; DISP_TIERS],
            total: 0,
            completed: 0,
            retired_ops: 0,
            parked: None,
            retry_window: false,
            opens: 0,
            closes: 0,
        }
    }

    /// 档位记账（每请求恰调一次）。
    fn record(&mut self, d: FileDisp) {
        let t = d.tier();
        if let Some(c) = self.disp_counts.get_mut(t) {
            *c = c.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
        if d == FileDisp::Completed {
            self.completed = self.completed.saturating_add(1);
        }
    }

    /// 开文件：路径归一化→根闭集→空槽/已关槽分配（真满容即故障档拒）。
    /// 复用已关条目时其残留操作账转入 retired_ops（对账不断链）。
    pub fn open(&mut self, path: &str) -> Result<usize, FileDiag> {
        let kp = match KeyPath::parse(path) {
            Ok(k) => k,
            Err(e) => {
                // 坏路径 = 策略否决档（仍恰记一档）。
                self.record(FileDisp::Vetoed);
                return Err(e);
            }
        };
        let mut i = 0usize;
        while i < HANDLE_CAP {
            let free = match self.slots.get(i) {
                Some(None) => true,
                Some(Some(f)) => !f.open,
                _ => false,
            };
            if free {
                // 复用已关条目：残留账退役入 retired_ops。
                if let Some(Some(old)) = self.slots.get(i) {
                    let residual = old.reads.saturating_add(old.writes).saturating_add(old.queries);
                    self.retired_ops = self.retired_ops.saturating_add(residual);
                }
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(NativeFile {
                        handle: i,
                        path: kp,
                        open: true,
                        reads: 0,
                        writes: 0,
                        queries: 0,
                        busy: false,
                    });
                }
                self.opens = self.opens.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        self.record(FileDisp::Faulted);
        Err(FileDiag::HandleExhausted)
    }

    /// 关文件：重复关 = 账目倒退（否决档）。
    pub fn close(&mut self, handle: usize) -> Result<(), FileDiag> {
        let open_now = match self.slots.get(handle) {
            Some(Some(f)) => f.open,
            _ => false,
        };
        if !open_now {
            self.record(FileDisp::Vetoed);
            return Err(FileDiag::HandleBad);
        }
        if let Some(Some(f)) = self.slots.get_mut(handle) {
            f.open = false;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }

    /// 执行一记操作账（Read/Write/Query 各记其账）。
    fn perform(&mut self, handle: usize, op: FileOp) -> Result<(), FileDiag> {
        if !op.counts() {
            return Err(FileDiag::OpClosedOff);
        }
        match self.slots.get_mut(handle) {
            Some(Some(f)) => {
                match op {
                    FileOp::Read => f.reads = f.reads.saturating_add(1),
                    FileOp::Write => f.writes = f.writes.saturating_add(1),
                    FileOp::Query => f.queries = f.queries.saturating_add(1),
                    _ => return Err(FileDiag::OpClosedOff),
                }
                Ok(())
            }
            _ => Err(FileDiag::HandleBad),
        }
    }

    /// 提交请求：句柄界检→关闭否决→忙挂起→重试窗→完成。五档分类核。
    pub fn submit(&mut self, handle: usize, op: FileOp) -> Result<FileDisp, FileDiag> {
        if handle >= HANDLE_CAP {
            self.record(FileDisp::Faulted);
            return Err(FileDiag::HandleBad);
        }
        let open_now = match self.slots.get(handle) {
            Some(Some(f)) => f.open,
            _ => false,
        };
        if !open_now {
            self.record(FileDisp::Vetoed);
            return Err(FileDiag::OpAfterClose);
        }
        let busy = match self.slots.get(handle) {
            Some(Some(f)) => f.busy,
            _ => false,
        };
        if busy {
            self.parked = Some((handle, op));
            self.record(FileDisp::Pending);
            return Ok(FileDisp::Pending);
        }
        if self.retry_window {
            self.retry_window = false;
            self.record(FileDisp::Retry);
            return Ok(FileDisp::Retry);
        }
        self.perform(handle, op)?;
        self.record(FileDisp::Completed);
        Ok(FileDisp::Completed)
    }

    /// 续在途请求：无在途即重结算拒（settle-once 显性拒）。
    pub fn resume(&mut self) -> Result<FileDisp, FileDiag> {
        let (handle, op) = match self.parked {
            Some(p) => p,
            None => return Err(FileDiag::SettleReplay),
        };
        self.parked = None;
        if let Some(Some(f)) = self.slots.get_mut(handle) {
            f.busy = false;
        }
        self.perform(handle, op)?;
        self.record(FileDisp::Completed);
        Ok(FileDisp::Completed)
    }

    /// 对账守恒：Σ在槽文件操作账 + 已退役残留账 ≡ 完成账。
    pub fn conservation(&self) -> Result<(), FileDiag> {
        let mut sum = self.retired_ops;
        let mut i = 0usize;
        while i < HANDLE_CAP {
            if let Some(Some(f)) = self.slots.get(i) {
                sum = sum.saturating_add(f.reads).saturating_add(f.writes).saturating_add(f.queries);
            }
            i += 1;
        }
        if sum != self.completed {
            return Err(FileDiag::LedgerBreak);
        }
        Ok(())
    }

    /// 五档守恒：Σ五档账 ≡ 总账。
    pub fn disp_conservation(&self) -> Result<(), FileDiag> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < DISP_TIERS {
            if let Some(c) = self.disp_counts.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        if sum != self.total {
            return Err(FileDiag::LedgerBreak);
        }
        Ok(())
    }

    /// 五档全出现判定（对照回归绿核心）。
    pub fn five_tiers_seen(&self) -> bool {
        let mut i = 0usize;
        while i < DISP_TIERS {
            match self.disp_counts.get(i) {
                Some(&c) if c > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 开放句柄计数。
    pub fn open_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < HANDLE_CAP {
            if let Some(Some(f)) = self.slots.get(i) {
                if f.open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：开/操作/五档/键路径对照 ──────────────────────────────
/// 黄金正演返回断言通过数（满绿 = 12）。
pub fn golden_run() -> Result<u32, FileDiag> {
    let mut t = HandleTable::new();
    // 注册表族路径与文件族路径共走同一条归一化管线。
    let h0 = t.open("\\Registry\\Machine\\Software\\VX9")?;
    let h1 = t.open("/device/harddiskvolume1/vx9/case.dat")?;
    // 大小写变体：同形即同键（键路径语义）。
    let h2 = t.open("\\REGISTRY\\machine\\SOFTWARE\\vx9")?;
    let p0 = match t.slots.get(h0) {
        Some(Some(f)) => f.path.clone(),
        _ => return Err(FileDiag::HandleBad),
    };
    let p2 = match t.slots.get(h2) {
        Some(Some(f)) => f.path.clone(),
        _ => return Err(FileDiag::HandleBad),
    };
    if !p0.eq_ci(&p2) || p0.canon_ci() != p2.canon_ci() {
        return Err(FileDiag::RootUnknown);
    }
    // 嵌套语义：\Registry\Machine 是其子键的父。
    let parent = KeyPath::parse("\\Registry\\Machine")?;
    if !p0.is_under(&parent) || p0.is_under(&p0) {
        return Err(FileDiag::RootUnknown);
    }
    // 完成档：Read/Write/Query 三账各记一笔。
    if t.submit(h0, FileOp::Read) != Ok(FileDisp::Completed) {
        return Err(FileDiag::OpClosedOff);
    }
    if t.submit(h0, FileOp::Write) != Ok(FileDisp::Completed) {
        return Err(FileDiag::OpClosedOff);
    }
    if t.submit(h1, FileOp::Query) != Ok(FileDisp::Completed) {
        return Err(FileDiag::OpClosedOff);
    }
    // 挂起档：文件忙 → 挂起 → 续单完成。
    if let Some(Some(f)) = t.slots.get_mut(h1) {
        f.busy = true;
    }
    if t.submit(h1, FileOp::Read) != Ok(FileDisp::Pending) {
        return Err(FileDiag::OpClosedOff);
    }
    if t.resume() != Ok(FileDisp::Completed) {
        return Err(FileDiag::SettleReplay);
    }
    // 重试档：重试窗单发 → 重试 → 重发完成。
    t.retry_window = true;
    if t.submit(h0, FileOp::Read) != Ok(FileDisp::Retry) {
        return Err(FileDiag::OpClosedOff);
    }
    if t.submit(h0, FileOp::Read) != Ok(FileDisp::Completed) {
        return Err(FileDiag::OpClosedOff);
    }
    // 否决档：坏路径开文件。
    if t.open("\\Foo\\Bar\\Baz").is_ok() {
        return Err(FileDiag::RootUnknown);
    }
    // 故障档：句柄越表。
    if t.submit(HANDLE_CAP, FileOp::Read).is_ok() {
        return Err(FileDiag::HandleBad);
    }
    // 关闭后操作 = 否决档（结算入否决档，调用面收 Err 定性）。
    t.close(h2)?;
    if t.submit(h2, FileOp::Read).is_ok() {
        return Err(FileDiag::OpAfterClose);
    }
    // 对账双守恒 + 五档全现。
    t.conservation()?;
    t.disp_conservation()?;
    if !t.five_tiers_seen() {
        return Err(FileDiag::OpClosedOff);
    }
    Ok(run_asserts())
}

/// 对照断言组（12 条，全过 = 12）。
pub fn run_asserts() -> u32 {
    let mut pass = 0u32;
    // A1：根闭集冻结为 4 组两节根对。
    if ROOT_PAIRS.len() == 4 {
        pass += 1;
    }
    // A2：五操作闭集数冻结 + 档位状态值全非零映射（完成=0 除外）。
    let mut status_ok = FileDisp::Completed.status() == 0;
    let mut k = 1usize;
    while k < DISP_TIERS {
        if let Some(s) = DISP_STATUS.get(k) {
            if *s == 0 {
                status_ok = false;
            }
        }
        k += 1;
    }
    if status_ok && FILE_OP_N == 5 {
        pass += 1;
    }
    // A3：分隔符折叠（\ 与 / 同权、重复/尾随折叠）。
    let a = match KeyPath::parse("\\Registry\\Machine\\\\Software\\") {
        Ok(p) => p,
        Err(_) => return pass,
    };
    let b = match KeyPath::parse("registry/machine/software") {
        Ok(p) => p,
        Err(_) => return pass,
    };
    if a.eq_ci(&b) && a.canonical() == "\\Registry\\Machine\\Software" {
        pass += 1;
    }
    // A4：空路径必拒。
    if KeyPath::parse("").is_err() && KeyPath::parse("\\\\").is_err() {
        pass += 1;
    }
    // A5：节点字符越闭集必拒。
    if KeyPath::parse("\\Registry\\Machine\\So<ft>").is_err() {
        pass += 1;
    }
    // A6：深度上限必拒（9 节）。
    if KeyPath::parse("\\Registry\\Machine\\a\\b\\c\\d\\e\\f\\g").is_err() {
        pass += 1;
    }
    // A7：根未知必拒。
    if KeyPath::parse("\\Foo\\Bar").is_err() {
        pass += 1;
    }
    // A8：句柄表满容必拒。
    let mut t8 = HandleTable::new();
    let mut full = true;
    let mut i = 0usize;
    while i < HANDLE_CAP {
        if t8.open("\\Registry\\User\\Slot").is_err() {
            full = false;
        }
        i += 1;
    }
    if full && t8.open("\\Registry\\User\\Overflow").is_err() {
        pass += 1;
    }
    // A9：重复结算必拒。
    let mut t9 = HandleTable::new();
    if t9.resume().is_err() {
        pass += 1;
    }
    // A10：关闭后操作必拒（否决档）。
    let mut ta = HandleTable::new();
    let ha = match ta.open("\\Device\\HarddiskVolume2\\f.bin") {
        Ok(h) => h,
        Err(_) => return pass,
    };
    let _ = ta.close(ha);
    if ta.submit(ha, FileOp::Read).is_err() {
        pass += 1;
    }
    // A11：嵌套语义（父子前缀 + 自嵌套拒 + 异族不同键）。
    let fa = match KeyPath::parse("\\Device\\HarddiskVolume1\\dir\\f") {
        Ok(p) => p,
        Err(_) => return pass,
    };
    let fb = match KeyPath::parse("\\Device\\HarddiskVolume1\\dir") {
        Ok(p) => p,
        Err(_) => return pass,
    };
    let rg = match KeyPath::parse("\\Registry\\Machine\\dir") {
        Ok(p) => p,
        Err(_) => return pass,
    };
    if fa.is_under(&fb) && !fb.is_under(&fa) && !fa.eq_ci(&rg) {
        pass += 1;
    }
    // A12：对账双守恒。
    let mut tc = HandleTable::new();
    let hc = match tc.open("\\Registry\\Machine\\k") {
        Ok(h) => h,
        Err(_) => return pass,
    };
    let _ = tc.submit(hc, FileOp::Read);
    let _ = tc.submit(hc, FileOp::Write);
    if tc.conservation().is_ok() && tc.disp_conservation().is_ok() {
        pass += 1;
    }
    pass
}

// ── 注入面：三族 × 10 轮全抓 ──────────────────────────────────────
/// 注入账本：三族各自 10 轮尝试/抓获。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 全抓判定：三族抓获数 ≡ 尝试数。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 轮数守恒：每族恰 10 轮。
    pub fn ten_rounds_each(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            match self.tries.get(i) {
                Some(&10) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 注入三族 × 10 轮：族0=键路径违约（字符/根/深度随机）、族1=重复结算、
/// 族2=句柄耗尽。LCG 变参（种子=SEG_BASE），违约必抓。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族0：键路径违约注入——四种违约轮换必抓。
        if let Some(t0) = ledger.tries.get_mut(0) {
            *t0 += 1;
        }
        let kind = rng.below(4);
        let bad = match kind {
            0 => String::from(""),
            1 => {
                let n = rng.below(0x20) as u8;
                let mut s = String::from("\\Registry\\Machine\\So");
                s.push(n as char);
                s
            }
            2 => String::from("\\Foo\\Bar\\Baz"),
            _ => String::from("\\Registry\\Machine\\a\\b\\c\\d\\e\\f\\g"),
        };
        if KeyPath::parse(&bad).is_err() {
            if let Some(c0) = ledger.caught.get_mut(0) {
                *c0 += 1;
            }
        }
        // 族1：重复结算注入——无在途续单必抓。
        if let Some(t1) = ledger.tries.get_mut(1) {
            *t1 += 1;
        }
        let mut t1 = HandleTable::new();
        if t1.resume().is_err() {
            if let Some(c1) = ledger.caught.get_mut(1) {
                *c1 += 1;
            }
        }
        // 族2：句柄耗尽注入——8 槽满后再开必抓。
        if let Some(t2) = ledger.tries.get_mut(2) {
            *t2 += 1;
        }
        let mut t2 = HandleTable::new();
        let mut filled = true;
        let mut i = 0usize;
        while i < HANDLE_CAP {
            if t2.open("\\Registry\\User\\Slot").is_err() {
                filled = false;
            }
            i += 1;
        }
        if filled && t2.open("\\Registry\\User\\More").is_err() {
            if let Some(c2) = ledger.caught.get_mut(2) {
                *c2 += 1;
            }
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ──────────────────────
/// 注册表族对照回归绿：Registry 根全收、Device 根全收、异族分键。
fn registry_family_ok() -> bool {
    // 注册表族四路径全收。
    let mut r = 0usize;
    while r < ROOT_PAIRS.len() {
        let (a, b) = match ROOT_PAIRS.get(r) {
            Some((a, b)) => (*a, *b),
            None => return false,
        };
        let mut p = String::from("\\");
        p.push_str(a);
        p.push('\\');
        p.push_str(b);
        p.push_str("\\key");
        match KeyPath::parse(&p) {
            Ok(kp) => {
                if kp.nodes.len() < 3 {
                    return false;
                }
            }
            Err(_) => return false,
        }
        r += 1;
    }
    // 异族不同键：Registry 根与 Device 根不同一。
    let reg = match KeyPath::parse("\\Registry\\Machine\\k") {
        Ok(p) => p,
        Err(_) => return false,
    };
    let dev = match KeyPath::parse("\\Device\\HarddiskVolume1\\k") {
        Ok(p) => p,
        Err(_) => return false,
    };
    !reg.eq_ci(&dev)
}

/// 键路径语义维持：折叠/同一性/嵌套/深度四语义全维持。
fn keypath_ok() -> bool {
    // 分隔符折叠与大小写同一。
    let a = match KeyPath::parse("\\Registry\\Machine\\S\\\\t/") {
        Ok(p) => p,
        Err(_) => return false,
    };
    let b = match KeyPath::parse("registry/machine/s/t") {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !a.eq_ci(&b) || a.canon_ci() != b.canon_ci() {
        return false;
    }
    // 嵌套：父前缀成立、自嵌套不成立、跨族不成立。
    let parent = match KeyPath::parse("\\Registry\\Machine") {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !a.is_under(&parent) || a.is_under(&a) {
        return false;
    }
    // 深度恰好 8 节可行、9 节必拒（边界双边）。
    match KeyPath::parse("\\Registry\\Machine\\a\\b\\c\\d\\e\\f") {
        Ok(kp) => {
            if kp.nodes.len() != MAX_KEY_DEPTH {
                return false;
            }
        }
        Err(_) => return false,
    }
    KeyPath::parse("\\Registry\\Machine\\a\\b\\c\\d\\e\\f\\g").is_err()
}

/// 文件族五档对照回归绿：黄金正演走通 + 五档全现。
fn file_disp_ok() -> bool {
    match golden_run() {
        Ok(12) => true,
        _ => false,
    }
}

/// 判定次序：开→操作→关、挂起→续单，乱序必拒。
fn order_ok() -> bool {
    let mut t = HandleTable::new();
    // 未开先操作 → 拒。
    if t.submit(0, FileOp::Read).is_ok() {
        return false;
    }
    let h = match t.open("\\Registry\\User\\ord") {
        Ok(h) => h,
        Err(_) => return false,
    };
    // 开后操作放行，关后操作拒。
    if t.submit(h, FileOp::Write) != Ok(FileDisp::Completed) {
        return false;
    }
    if t.close(h).is_err() {
        return false;
    }
    t.submit(h, FileOp::Read).is_err()
}

/// 容量耗尽：8 槽满拒，关闭一槽后可复开（计数守恒）。
fn capacity_ok() -> bool {
    let mut t = HandleTable::new();
    let mut i = 0usize;
    while i < HANDLE_CAP {
        if t.open("\\Device\\HarddiskVolume1\\f").is_err() {
            return false;
        }
        i += 1;
    }
    if t.open("\\Device\\HarddiskVolume1\\g").is_ok() {
        return false;
    }
    if t.close(3).is_err() {
        return false;
    }
    t.open("\\Device\\HarddiskVolume1\\g").is_ok() && t.open_count() == HANDLE_CAP
}

/// 双跑一致：黄金正演跑两遍结果逐位一致。
fn double_run_ok() -> bool {
    golden_run() == golden_run()
}

/// 对账一致/计数守恒：文件账+档账+注入账三守恒。
fn conservation_ok() -> bool {
    let mut t = HandleTable::new();
    let h = match t.open("\\Registry\\Machine\\led") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let mut i = 0u32;
    while i < 4 {
        if t.submit(h, FileOp::Read) != Ok(FileDisp::Completed) {
            return false;
        }
        i += 1;
    }
    if t.conservation().is_err() || t.disp_conservation().is_err() {
        return false;
    }
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught() && led.ten_rounds_each()
}

/// 文案闭集：10 码全查得非空中文，未知码给兜底。
fn text_closed_ok() -> bool {
    let mut i = 0usize;
    while i < FileDiag::ALL.len() {
        if let Some(c) = FileDiag::ALL.get(i) {
            let d = detail(c.val());
            if d.is_empty() || d == "未知诊断码：不在 0x82180 段闭集内" {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F) == "未知诊断码：不在 0x82180 段闭集内"
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 区间，诊断码全在段窗内。
fn segment_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < FileDiag::ALL.len() {
        if let Some(c) = FileDiag::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 收口只增不减：断言组重跑不劣化。
fn closing_ok() -> bool {
    let a = run_asserts();
    let b = run_asserts();
    b >= a && a == 12
}

// ── 检查名/静态明细闭集（match 查表，无裸下标） ───────────────────
/// 第 i 条检查的静态名（闭集 match 查表，越表给兜底）。
fn check_name(i: usize) -> &'static str {
    match i {
        0 => "J1-注册表族对照回归绿",
        1 => "J1-键路径语义维持",
        2 => "J1-五档对照回归绿",
        3 => "J1-重复结算显性拒",
        4 => "J1-判定次序先行",
        5 => "J1-容量耗尽必抓",
        6 => "J1-注入族一·键路径10轮全抓",
        7 => "J1-注入族二·重结算全抓",
        8 => "J1-注入族三·句柄耗尽全抓",
        9 => "J1-双跑一致",
        10 => "J1-对账一致·三账守恒",
        11 => "J1-文案闭集",
        12 => "J1-段位守卫0x82180",
        13 => "J1-收口只增不减",
        _ => "越表检查项",
    }
}

/// 第 i 条检查的静态明细兜底（第 9 条起走 s.add 静态明细）。
fn check_static(i: usize) -> &'static str {
    match i {
        0 => "Registry/Device根闭集全收·异族分键",
        1 => "折叠/同一/嵌套/深度四语义全维持",
        2 => "完成/挂起/重试/否决/故障全现",
        3 => "settle-once·无在途续单必拒",
        4 => "开→操作→关乱序必拒",
        5 => "8槽满拒·关一复开守恒",
        6 => "键路径违约注入10/10",
        7 => "重结算注入10/10",
        8 => "句柄耗尽注入10/10",
        9 => "两次黄金正演逐位一致",
        10 => "文件账+档账+注入账三守恒",
        11 => "10码查表非空·未知码兜底",
        12 => "0x40对齐·VX9区间·段窗内",
        13 => "断言组重跑不劣化",
        _ => "明细缺表",
    }
}

/// extra_checks：14 条检查（正常流/边界/注入/判定次序/双跑/守恒/守卫/收口）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 1：注册表族对照回归绿（判据锚点主谓词）。
    v.push((
        String::from(check_name(0)),
        registry_family_ok(),
        String::from("Registry/Device 四根全收·异族分键"),
    ));
    // 2：键路径语义维持（判据锚点副谓词）。
    let kp_canon = match KeyPath::parse("\\Registry\\Machine\\\\S/") {
        Ok(p) => p.canonical(),
        Err(e) => format!("parse_err={:?}", e),
    };
    v.push((
        String::from(check_name(1)),
        keypath_ok(),
        format!("canonical={}", kp_canon),
    ));
    // 3：五档对照回归绿（题面档位面）。
    let golden = golden_run();
    v.push((
        String::from(check_name(2)),
        file_disp_ok(),
        format!("asserts={:?}", golden),
    ));
    // 4：重复结算显性拒。
    v.push((String::from(check_name(3)), settle_once_ok(), String::from(check_static(3))));
    // 5：判定次序先行。
    v.push((String::from(check_name(4)), order_ok(), String::from(check_static(4))));
    // 6：容量耗尽必抓。
    v.push((String::from(check_name(5)), capacity_ok(), String::from(check_static(5))));
    // 7~9：注入三族各 10 轮全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let mut f = 0usize;
    while f < 3 {
        let t = match led.tries.get(f) {
            Some(x) => *x,
            None => 0,
        };
        let c = match led.caught.get(f) {
            Some(x) => *x,
            None => 0,
        };
        v.push((
            String::from(check_name(6 + f)),
            c == t && t == 10,
            format!("c/t={}/{}", c, t),
        ));
        f += 1;
    }
    // 10：双跑一致。
    v.push((String::from(check_name(9)), double_run_ok(), String::from(check_static(9))));
    // 11：对账一致/三账守恒。
    v.push((
        String::from(check_name(10)),
        conservation_ok(),
        String::from("文件账+档账+注入账三守恒"),
    ));
    // 12：文案闭集。
    v.push((String::from(check_name(11)), text_closed_ok(), String::from(check_static(11))));
    // 13：段位守卫。
    v.push((String::from(check_name(12)), segment_guard_ok(), String::from(check_static(12))));
    // 14：收口只增不减。
    v.push((String::from(check_name(13)), closing_ok(), String::from(check_static(13))));
    v
}

/// 重复结算显性拒谓词（第 4 条检查的判定核）。
fn settle_once_ok() -> bool {
    let mut t = HandleTable::new();
    // 无在途续单必拒。
    if t.resume().is_ok() {
        return false;
    }
    // 挂起一次→续单恰一次→再续必拒（settle-once 单调）。
    let h = match t.open("\\Device\\HarddiskVolume2\\once.bin") {
        Ok(h) => h,
        Err(_) => return false,
    };
    if let Some(Some(f)) = t.slots.get_mut(h) {
        f.busy = true;
    }
    if t.submit(h, FileOp::Read) != Ok(FileDisp::Pending) {
        return false;
    }
    if t.resume() != Ok(FileDisp::Completed) {
        return false;
    }
    t.resume().is_err()
}

// ── 检查入口 ───────────────────────────────────────────────────────
/// VX9 检查入口：extra_checks 逐条进 set（动态槽只给最关键 8 条）。
pub fn run_unx12734_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12734 文件族 NativeFile* 对照回归（disposition 五档）");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_name_dyn, passed, detail_dyn)) = items.get(i) {
            let name = check_name(i);
            if i < 8 {
                // 动态明细槽（≤8）：判据锚点与注入族优先。
                s.add_dyn(name, *passed, detail_dyn);
            } else {
                // 静态明细：不占动态槽，红项照记不丢。
                s.add(name, *passed, check_static(i));
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_family_cross_check_green() {
        // 判据锚点：注册表族对照回归绿（四根全收 + 异族分键）。
        assert!(registry_family_ok());
    }

    #[test]
    fn keypath_semantics_held() {
        // 判据锚点：键路径语义维持（折叠/同一/嵌套/深度四语义）。
        assert!(keypath_ok());
        let a = KeyPath::parse("\\Registry\\Machine\\X").unwrap_or(KeyPath { nodes: Vec::new() });
        let b = KeyPath::parse("/registry/MACHINE/x").unwrap_or(KeyPath { nodes: Vec::new() });
        assert!(a.eq_ci(&b));
        assert_eq!(a.canonical(), "\\Registry\\Machine\\X");
    }

    #[test]
    fn five_dispositions_all_seen() {
        // 题面锚点：五档（完成/挂起/重试/否决/故障）全出现且守恒。
        assert!(file_disp_ok());
        assert_eq!(golden_run(), Ok(12));
    }

    #[test]
    fn settle_replay_explicitly_rejected() {
        // 注入：settle-once 破坏必抓（空续单/二次续单）。
        assert!(settle_once_ok());
        let mut t = HandleTable::new();
        assert!(t.resume().is_err());
    }

    #[test]
    fn op_order_enforced() {
        // 判序：未开先操作/关后操作必拒；正序全绿。
        assert!(order_ok());
    }

    #[test]
    fn handle_capacity_exhaustion() {
        // 边界：8 槽满拒、关一复开、开数守恒。
        assert!(capacity_ok());
        let mut t = HandleTable::new();
        let mut i = 0usize;
        while i < HANDLE_CAP {
            assert!(t.open("\\Registry\\User\\s").is_ok());
            i += 1;
        }
        assert!(t.open("\\Registry\\User\\x").is_err());
    }

    #[test]
    fn path_violations_all_caught() {
        // 边界：空路径/坏字符/越深/未知根四类违约必抓。
        assert!(KeyPath::parse("").is_err());
        assert!(KeyPath::parse("\\Registry\\Machine\\a<b").is_err());
        assert!(KeyPath::parse("\\Registry\\Machine\\a\\b\\c\\d\\e\\f\\g").is_err());
        assert!(KeyPath::parse("\\Foo\\Bar").is_err());
        assert!(KeyPath::parse("\\Registry\\Machine").is_ok());
    }

    #[test]
    fn injections_three_families_all_caught() {
        // 注入：键路径违约/重结算/句柄耗尽三族各 10 轮全抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught());
        assert!(led.ten_rounds_each());
    }

    #[test]
    fn double_run_identical() {
        // 双跑一致：两次黄金正演逐位一致。
        assert!(double_run_ok());
        assert_eq!(extra_checks().len(), 14);
    }

    #[test]
    fn conservation_triple_ledger() {
        // 守恒：文件账+档账+注入账三账全守恒。
        assert!(conservation_ok());
        let mut t = HandleTable::new();
        let h = t.open("\\Registry\\Machine\\c").unwrap_or(0);
        let _ = t.submit(h, FileOp::Query);
        assert!(t.conservation().is_ok());
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集 + 未知码兜底。
        assert!(segment_guard_ok());
        assert!(text_closed_ok());
        assert_eq!(SEG_BASE, 0x82180);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0xFFFFFFFF), "未知诊断码：不在 0x82180 段闭集内");
    }

    #[test]
    fn checks_all_green_in_set() {
        // 全绿：检查入口 14 条全过、无截断、动态槽不超 8。
        let s = run_unx12734_checks();
        assert_eq!(s.len(), 14);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.dyn_used() <= 8);
    }
}
