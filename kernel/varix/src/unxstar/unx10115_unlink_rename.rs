//! UNX-F10115 unlink/rename 正反双判据（删除改名达成与缺失路径误用受控）· 0x92CC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10116-J1 mmap 映射达成（匿名页可读写 LTP mmap01 面 10/10 次）；
//!       UNX-F10116-J2 零长度误用受控（预期 EINVAL 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92CC0（本卡独占 0x40 窗口，下一卡 0x92D00）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_ns_call`：调用行 "verb|p1|p2" 三段闭集 {unlink,
//!      rename, link, touch}，段数不足/动词出闭集逐档拒绝；
//!   ② 命名空间账本 `NsLedger`：路径→(kind, nlink, dev, children, state)
//!      五列，touch/link/unlink/rename 全语义，nlink 归零真实出账；
//!   ③ 状态机 `PathState`：Vacuum→Linked→Renamed→Unlinked→Gone 单向推进，
//!      倒退逐次拒绝；
//!   ④ 注入核 `BadNsOp`：缺失路径 / 跨设备改名 / 非空目录替换 /
//!      目录 unlink 四类坏样本逐类 10/10 次精确复现
//!      ENOENT/EXDEV/ENOTEMPTY/EISDIR 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10115 独占 0x40 窗口，下一卡 0x92D00）。
pub const SEG_BASE: u32 = 0x92CC0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92D00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 路径名长度上界（超过即超长档拒绝）。
pub const NAME_MAX: usize = 64;

/// 命名空间账本容量上限。
pub const LEDGER_CAP: usize = 32;

/// 本册统一设备号（rename 同设备语义）。
pub const DEV_LOCAL: u32 = 1;

/// 外来设备号（跨设备注入样本用）。
pub const DEV_FOREIGN: u32 = 2;

/// 硬链计数上限（防溢出饱和位）。
pub const NLINK_MAX: u32 = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsCode {
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 跨设备改名（EXDEV 族）
    CrossDev,
    /// 非空目录替换（ENOTEMPTY 族）
    NonEmptyDir,
    /// 目录被 unlink（EISDIR 族）
    IsDirUnlink,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 路径超长（ENAMETOOLONG 族）
    PathTruncate,
    /// 重复注册（EEXIST 族）
    DupPath,
    /// 未知动词（EINVAL 族）
    BadVerb,
    /// 账本容量满
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl NsCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            NsCode::MissingPath => "缺失路径被拒：账本中无此路径；ENOENT 精确归档",
            NsCode::CrossDev => "跨设备改名被拒：old 与目标不在同一设备；EXDEV 精确归档",
            NsCode::NonEmptyDir => "非空目录替换被拒：目标目录非空；ENOTEMPTY 精确归档",
            NsCode::IsDirUnlink => "目录 unlink 被拒：unlink 只吃文件；EISDIR 精确归档",
            NsCode::EmptyPath => "空路径被拒：路径名不得为空串；请给出目标路径",
            NsCode::PathTruncate => "路径超长被拒：路径名超过 NAME_MAX；请缩短路径",
            NsCode::DupPath => "重复注册被拒：同路径已存在；EEXIST 精确归档",
            NsCode::BadVerb => "未知动词被拒：动词闭集 {unlink, rename, link, touch}；请核对调用行",
            NsCode::LedgerFull => "账本容量满：请先 unlink 释放路径再注册",
            NsCode::StateRegress => "状态倒退被拒：Vacuum→Linked→Renamed→Unlinked→Gone 单向",
            NsCode::EmptySummary => "空成文被拒：小结不得为空串；请先注册路径再成文",
            NsCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [NsCode; 12] = [
        NsCode::MissingPath,
        NsCode::CrossDev,
        NsCode::NonEmptyDir,
        NsCode::IsDirUnlink,
        NsCode::EmptyPath,
        NsCode::PathTruncate,
        NsCode::DupPath,
        NsCode::BadVerb,
        NsCode::LedgerFull,
        NsCode::StateRegress,
        NsCode::EmptySummary,
        NsCode::SegmentViolation,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的命名空间调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NsCall {
    /// 动词（unlink/rename/link/touch）
    pub verb: String,
    /// 第一路径成分
    pub p1: String,
    /// 第二路径成分（unlink/touch 为空串）
    pub p2: String,
}

/// 解析 "verb|p1|p2" 三段调用行；动词出闭集、段数不足逐档拒绝。
pub fn parse_ns_call(line: &str) -> Result<NsCall, NsCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = match bytes.get(i) {
            Some(b) => *b as char,
            None => break,
        };
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
        i += 1;
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(NsCode::BadVerb);
    }
    let verb = match parts.first() {
        Some(v) => v.clone(),
        None => return Err(NsCode::BadVerb),
    };
    if verb != "unlink" && verb != "rename" && verb != "link" && verb != "touch" {
        return Err(NsCode::BadVerb);
    }
    let p1 = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(NsCode::EmptyPath),
    };
    let p2 = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(NsCode::EmptyPath),
    };
    Ok(NsCall { verb, p1, p2 })
}

// ── 路径状态机 ────────────────────────────────────────────────────────
/// 路径生命周期状态（单调推进，倒退即拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathState {
    /// 未入账
    Vacuum,
    /// 已链接在账
    Linked,
    /// 已改名在账
    Renamed,
    /// 已解链
    Unlinked,
    /// 已消亡（账面移除后留档）
    Gone,
}

impl PathState {
    /// 单调推进：只允许相邻级递进。
    pub fn can_advance(self, next: PathState) -> bool {
        let rank = |s: PathState| match s {
            PathState::Vacuum => 0u8,
            PathState::Linked => 1,
            PathState::Renamed => 2,
            PathState::Unlinked => 3,
            PathState::Gone => 4,
        };
        rank(next) == rank(self) + 1
    }

    /// 中文标签（成文与审计用）。
    pub fn label(self) -> &'static str {
        match self {
            PathState::Vacuum => "未入账",
            PathState::Linked => "在账",
            PathState::Renamed => "已改名",
            PathState::Unlinked => "已解链",
            PathState::Gone => "已消亡",
        }
    }
}

// ── 命名空间账本 ──────────────────────────────────────────────────────
/// 单路径五列记录。
#[derive(Clone, Debug)]
pub struct NsEnt {
    /// 路径名
    pub name: String,
    /// 目录标记
    pub is_dir: bool,
    /// 链接计数
    pub nlink: u32,
    /// 设备号
    pub dev: u32,
    /// 子项数（目录替换判 ENOTEMPTY 用）
    pub children: u32,
    /// 生命周期状态
    pub state: PathState,
}

/// 命名空间账本：路径→五列；判定只读、变更留痕。
#[derive(Clone, Debug)]
pub struct NsLedger {
    paths: Vec<NsEnt>,
    cap: usize,
    /// 累计 link 成功次数。
    pub links: u32,
    /// 累计 unlink 成功次数。
    pub unlinks: u32,
    /// 累计 rename 成功次数。
    pub renames: u32,
}

impl NsLedger {
    /// 空账构造（容量 LEDGER_CAP）。
    pub fn new() -> NsLedger {
        NsLedger { paths: Vec::new(), cap: LEDGER_CAP, links: 0, unlinks: 0, renames: 0 }
    }

    /// 在账路径数。
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// 通用路径校验：空/超长逐档拒绝。
    fn check_name(name: &str) -> Result<(), NsCode> {
        if name.is_empty() {
            return Err(NsCode::EmptyPath);
        }
        if name.len() > NAME_MAX {
            return Err(NsCode::PathTruncate);
        }
        Ok(())
    }

    /// 按名取下标（内部寻址；账外返回 None）。
    fn idx_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 重复名扫描：在账同名即真。
    fn dup_scan(&self, name: &str) -> bool {
        self.idx_of(name).is_some()
    }

    /// 注册文件（touch 语义）：nlink=1，children=0，dev=本地。
    pub fn touch(&mut self, name: &str) -> Result<(), NsCode> {
        Self::check_name(name)?;
        if self.paths.len() >= self.cap {
            return Err(NsCode::LedgerFull);
        }
        if self.dup_scan(name) {
            return Err(NsCode::DupPath);
        }
        let _ = self.paths.push(NsEnt {
            name: String::from(name),
            is_dir: false,
            nlink: 1,
            dev: DEV_LOCAL,
            children: 0,
            state: PathState::Linked,
        });
        Ok(())
    }

    /// 注册外来设备路径（跨设备注入样本装载用）。
    pub fn attach_foreign(&mut self, name: &str, dev: u32) -> Result<(), NsCode> {
        Self::check_name(name)?;
        if self.paths.len() >= self.cap {
            return Err(NsCode::LedgerFull);
        }
        if self.dup_scan(name) {
            return Err(NsCode::DupPath);
        }
        let _ = self.paths.push(NsEnt {
            name: String::from(name),
            is_dir: false,
            nlink: 1,
            dev,
            children: 0,
            state: PathState::Linked,
        });
        Ok(())
    }

    /// 注册目录：children 给定（>0 即非空目录样本）。
    pub fn mkdir(&mut self, name: &str, children: u32) -> Result<(), NsCode> {
        Self::check_name(name)?;
        if self.paths.len() >= self.cap {
            return Err(NsCode::LedgerFull);
        }
        if self.dup_scan(name) {
            return Err(NsCode::DupPath);
        }
        let _ = self.paths.push(NsEnt {
            name: String::from(name),
            is_dir: true,
            nlink: 1,
            dev: DEV_LOCAL,
            children,
            state: PathState::Linked,
        });
        Ok(())
    }

    /// 硬链：在账文件 nlink+1（NLINK_MAX 饱和拒）。
    pub fn link(&mut self, name: &str) -> Result<u32, NsCode> {
        Self::check_name(name)?;
        let idx = match self.idx_of(name) {
            Some(i) => i,
            None => return Err(NsCode::MissingPath),
        };
        let (is_dir, cur) = match self.paths.get(idx) {
            Some(p) => (p.is_dir, p.nlink),
            None => return Err(NsCode::MissingPath),
        };
        if is_dir {
            return Err(NsCode::IsDirUnlink);
        }
        if cur >= NLINK_MAX {
            return Err(NsCode::LedgerFull);
        }
        if let Some(p) = self.paths.get_mut(idx) {
            p.nlink = p.nlink.saturating_add(1);
        }
        self.links = self.links.saturating_add(1);
        Ok(self.links)
    }

    /// unlink：在账文件 nlink−1；归零即账面移除（真实出账）。
    pub fn unlink(&mut self, name: &str) -> Result<u32, NsCode> {
        Self::check_name(name)?;
        let idx = match self.idx_of(name) {
            Some(i) => i,
            None => return Err(NsCode::MissingPath),
        };
        let (is_dir, cur) = match self.paths.get(idx) {
            Some(p) => (p.is_dir, p.nlink),
            None => return Err(NsCode::MissingPath),
        };
        if is_dir {
            return Err(NsCode::IsDirUnlink);
        }
        let after = cur.saturating_sub(1);
        if after == 0 {
            if let Some(p) = self.paths.get_mut(idx) {
                p.nlink = 0;
                p.state = PathState::Gone;
            }
            let _ = self.paths.remove(idx);
            self.unlinks = self.unlinks.saturating_add(1);
            return Ok(0);
        }
        if let Some(p) = self.paths.get_mut(idx) {
            p.nlink = after;
        }
        self.unlinks = self.unlinks.saturating_add(1);
        Ok(after)
    }

    /// rename：old 在账、同设备、目标空目录或不在账三闸。
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), NsCode> {
        Self::check_name(old)?;
        Self::check_name(new)?;
        let idx = match self.idx_of(old) {
            Some(i) => i,
            None => return Err(NsCode::MissingPath),
        };
        let odev = match self.paths.get(idx) {
            Some(p) => p.dev,
            None => return Err(NsCode::MissingPath),
        };
        if odev != DEV_LOCAL {
            return Err(NsCode::CrossDev);
        }
        // 目标闸：目标在账且为非空目录 → ENOTEMPTY；否则目标被替换出账。
        if let Some(tidx) = self.idx_of(new) {
            let (tdir, tchildren) = match self.paths.get(tidx) {
                Some(p) => (p.is_dir, p.children),
                None => (false, 0),
            };
            if tdir && tchildren > 0 {
                return Err(NsCode::NonEmptyDir);
            }
            let _ = self.paths.remove(tidx);
        }
        if let Some(p) = self.paths.get_mut(idx) {
            p.name = String::from(new);
            p.state = PathState::Renamed;
        }
        self.renames = self.renames.saturating_add(1);
        Ok(())
    }

    /// nlink 读取（审计用）。
    pub fn nlink_of(&self, name: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    return Some(p.nlink);
                }
            }
            i += 1;
        }
        None
    }

    /// 在账判定（改名后可打开面用）。
    pub fn contains(&self, name: &str) -> bool {
        self.idx_of(name).is_some()
    }

    /// 状态读取。
    pub fn state_of(&self, name: &str) -> Option<PathState> {
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    return Some(p.state);
                }
            }
            i += 1;
        }
        None
    }
}

// ── J1 达成核：unlink01/rename01 面 10/10 轮 ──────────────────────────
/// 逐轮 6 面断言；返回 (达成轮数, 探针次数)。
pub fn run_unlink_rename_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = NsLedger::new();
        let mut all_ok = true;
        // 面1：touch 后 nlink==1。
        all_ok = all_ok && l.touch("a.txt").is_ok() && l.nlink_of("a.txt") == Some(1);
        probes += 1;
        // 面2：link 后 nlink==2。
        all_ok = all_ok && l.link("a.txt").is_ok() && l.nlink_of("a.txt") == Some(2);
        probes += 1;
        // 面3：unlink 后 nlink 回落 1。
        all_ok = all_ok && l.unlink("a.txt").is_ok() && l.nlink_of("a.txt") == Some(1);
        probes += 1;
        // 面4：rename 改名后新名可达、旧名失联。
        all_ok = all_ok && l.rename("a.txt", "c.txt").is_ok()
            && l.contains("c.txt") && !l.contains("a.txt");
        probes += 1;
        // 面5：改名后实体 nlink 守恒仍为 1。
        all_ok = all_ok && l.nlink_of("c.txt") == Some(1);
        probes += 1;
        // 面6：改名后 unlink 归零移除，账空。
        all_ok = all_ok && l.unlink("c.txt").is_ok() && l.len() == 0;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏命名空间操作矩阵 ────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadNsOp {
    /// 缺失路径 unlink（ENOENT 族）
    MissingPath,
    /// 跨设备 rename（EXDEV 族）
    CrossDev,
    /// 非空目录 rename 替换（ENOTEMPTY 族）
    NonEmptyDir,
    /// 目录 unlink（EISDIR 族）
    DirUnlink,
}

impl BadNsOp {
    /// 闭集全量。
    pub const ALL: [BadNsOp; 4] = [
        BadNsOp::MissingPath,
        BadNsOp::CrossDev,
        BadNsOp::NonEmptyDir,
        BadNsOp::DirUnlink,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadNsOp::MissingPath => "缺失路径 unlink",
            BadNsOp::CrossDev => "跨设备 rename",
            BadNsOp::NonEmptyDir => "非空目录替换",
            BadNsOp::DirUnlink => "目录 unlink",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), NsCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(NsCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏命名空间注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_ns_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadNsOp::ALL.len() {
        let case = match BadNsOp::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = NsLedger::new();
            let caught = match case {
                BadNsOp::MissingPath => {
                    l.unlink("ghost.txt").err() == Some(NsCode::MissingPath)
                }
                BadNsOp::CrossDev => {
                    let _ = l.attach_foreign("x.txt", DEV_FOREIGN);
                    l.rename("x.txt", "y.txt").err() == Some(NsCode::CrossDev)
                }
                BadNsOp::NonEmptyDir => {
                    let _ = l.mkdir("src", 3);
                    let _ = l.mkdir("dst", 2);
                    l.rename("src", "dst").err() == Some(NsCode::NonEmptyDir)
                }
                BadNsOp::DirUnlink => {
                    let _ = l.mkdir("d", 0);
                    l.unlink("d").err() == Some(NsCode::IsDirUnlink)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 命名空间小结成文（纯函数） ────────────────────────────────────────
/// 小结成文：「name=<s> nlink=<n> state=<t>」；账外先拒。
pub fn compose_ns_summary(l: &NsLedger, name: &str) -> Result<String, NsCode> {
    let nlink = match l.nlink_of(name) {
        Some(n) => n,
        None => return Err(NsCode::EmptySummary),
    };
    let st = match l.state_of(name) {
        Some(t) => t.label(),
        None => "未知",
    };
    let out = format!("name={} nlink={} state={}", name, nlink, st);
    if out.is_empty() {
        return Err(NsCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径族"),
    ("EXDEV", -18, 17, "Windows ERROR_NOT_SAME_DEVICE；跨设备改名族"),
    ("ENOTEMPTY", -39, 145, "Windows ERROR_DIR_NOT_EMPTY；非空目录替换族"),
    ("EISDIR", -21, 267, "Windows ERROR_DIRECTORY；目录 unlink 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空路径/坏动词族"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；超长路径族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 规范语料指纹：touch→link→unlink→rename→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = NsLedger::new();
    let _ = l.touch("f0");
    let _ = l.link("f0");
    let _ = l.unlink("f0");
    let _ = l.rename("f0", "g0");
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 2 {
        let name = match e {
            0 => "g0",
            _ => "f0",
        };
        if let Ok(s) = compose_ns_summary(&l, name) {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(b';');
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10115 判据集（15 条静态 + 3 条动态；动态明细 ≤8 条）。
pub fn run_unx10115_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10115");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92CC0..=0x92CFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < NsCode::ALL.len() {
        let mut j = i + 1;
        while j < NsCode::ALL.len() {
            if let (Some(a), Some(b)) = (NsCode::ALL.get(i), NsCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = NsCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 调用行解析器
    let parse_ok = parse_ns_call("unlink|a.txt|").is_ok()
        && parse_ns_call("rename|a|b").is_ok()
        && parse_ns_call("rm|a|").err() == Some(NsCode::BadVerb)
        && parse_ns_call("two|only").err() == Some(NsCode::BadVerb);
    s.add("调用行解析器：三段闭集放行且坏动词逐档拒绝", parse_ok,
          "解析器是 unlink/rename 前置，动词必须落在四动词闭集");

    // 4. 账本注册闸
    let mut l = NsLedger::new();
    let reg_ok = l.touch("f").is_ok() && l.len() == 1
        && l.touch("").err() == Some(NsCode::EmptyPath)
        && l.touch("f").err() == Some(NsCode::DupPath)
        && l.touch(&String::from("x").repeat(NAME_MAX + 1)).err() == Some(NsCode::PathTruncate);
    s.add("命名空间账本：空名/超长/重复注册显性拒绝", reg_ok,
          "注册是 unlink/rename 前置；被拒注册不得污染账面");

    // 5. J1 达成核
    let (ok, probes) = run_unlink_rename_rounds();
    s.add("J1 删除改名达成：nlink 递减与改名后可打开 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面断言全部与账面推导一致，任何一面漂移即失守");

    // 6. link 计数守恒
    let mut l2 = NsLedger::new();
    let _ = l2.touch("g");
    let mut step_ok = true;
    let mut k = 0u32;
    while k < 4 {
        step_ok = step_ok && l2.link("g").is_ok();
        k += 1;
    }
    step_ok = step_ok && l2.nlink_of("g") == Some(5)
        && l2.unlink("g").is_ok() && l2.nlink_of("g") == Some(4)
        && l2.links == 4 && l2.unlinks == 1;
    s.add("link 计数守恒：link/unlink 全程账面一致", step_ok,
          "nlink 是硬链语义的账面根；加减必须逐次可复算");

    // 7. unlink 归零移除
    let mut l3 = NsLedger::new();
    let _ = l3.touch("h");
    let zero_ok = l3.unlink("h").is_ok() && l3.len() == 0 && !l3.contains("h")
        && l3.unlink("h").err() == Some(NsCode::MissingPath);
    s.add("unlink 归零移除：账面回落且二次 unlink 按 ENOENT 归档", zero_ok,
          "nlink 归零必须真实出账；二次 unlink 不得假成功");

    // 8. 注入核
    let (inj, per_class) = run_bad_ns_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "缺失路径/跨设备/非空目录/目录 unlink 必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadNsOp::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. J2 四档独立归因
    let mut l4 = NsLedger::new();
    let _ = l4.attach_foreign("fx", DEV_FOREIGN);
    let j2_ok = l4.unlink("nx").err() == Some(NsCode::MissingPath)
        && l4.rename("fx", "ny").err() == Some(NsCode::CrossDev);
    s.add("J2 缺失路径受控：缺失/跨设备两档独立精确归因", j2_ok,
          "缺失与跨设备互不吞档；归因必须逐码可复算");

    // 10b. 非空目录替换档
    let j2b_ok = {
        let mut m = NsLedger::new();
        let _ = m.mkdir("a", 5);
        let _ = m.mkdir("b", 1);
        m.rename("a", "b").err() == Some(NsCode::NonEmptyDir)
    };
    s.add("非空目录替换：ENOTEMPTY 档独立复现", j2b_ok,
          "目标目录非空时 rename 必须拒绝且两目录账面不动");

    // 11. 状态机
    let sm_ok = PathState::Linked.can_advance(PathState::Renamed)
        && !PathState::Renamed.can_advance(PathState::Linked)
        && PathState::Vacuum.can_advance(PathState::Linked)
        && !PathState::Gone.can_advance(PathState::Linked);
    s.add("状态机：Vacuum→Linked→Renamed→Unlinked→Gone 单向", sm_ok,
          "生命周期状态只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(18) && agrees_with_linux(39)
        && agrees_with_linux(21) && agrees_with_linux(22) && agrees_with_linux(36)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文非空 + 双跑指纹
    let sum = compose_ns_summary(&l, "f");
    let text_ok = match &sum {
        Ok(t) => t.contains("name=f") && t.contains("nlink=1") && t.contains("state=在账"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结三字段齐备且双跑哈希逐位一致",
          text_ok && sum == compose_ns_summary(&l, "f") && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("删除改名达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_ns_call("unlink|a|").is_ok());
        assert!(parse_ns_call("touch|a|").is_ok());
        assert_eq!(parse_ns_call("mv|a|b").err(), Some(NsCode::BadVerb));
        assert_eq!(parse_ns_call("two|only").err(), Some(NsCode::BadVerb));
    }

    #[test]
    fn register_guards() {
        let mut l = NsLedger::new();
        assert!(l.touch("a").is_ok());
        assert_eq!(l.touch("").err(), Some(NsCode::EmptyPath));
        assert_eq!(l.touch("a").err(), Some(NsCode::DupPath));
        let long = String::from("p").repeat(NAME_MAX + 1);
        assert_eq!(l.touch(long.as_str()).err(), Some(NsCode::PathTruncate));
    }

    #[test]
    fn nlink_semantics() {
        let mut l = NsLedger::new();
        let _ = l.touch("f");
        assert!(l.link("f").is_ok());
        assert_eq!(l.nlink_of("f"), Some(2));
        assert!(l.unlink("f").is_ok());
        assert_eq!(l.nlink_of("f"), Some(1));
    }

    #[test]
    fn unlink_zero_removes() {
        let mut l = NsLedger::new();
        let _ = l.touch("f");
        assert!(l.unlink("f").is_ok());
        assert!(!l.contains("f"));
        assert_eq!(l.unlink("f").err(), Some(NsCode::MissingPath));
    }

    #[test]
    fn rename_semantics() {
        let mut l = NsLedger::new();
        let _ = l.touch("old");
        assert!(l.rename("old", "new").is_ok());
        assert!(l.contains("new"));
        assert!(!l.contains("old"));
        assert_eq!(l.state_of("new"), Some(PathState::Renamed));
    }

    #[test]
    fn rename_nonempty_dir_blocked() {
        let mut l = NsLedger::new();
        let _ = l.mkdir("src", 2);
        let _ = l.mkdir("dst", 1);
        assert_eq!(l.rename("src", "dst").err(), Some(NsCode::NonEmptyDir));
        assert!(l.contains("src") && l.contains("dst"));
    }

    #[test]
    fn cross_dev_blocked() {
        let mut l = NsLedger::new();
        let _ = l.attach_foreign("x", DEV_FOREIGN);
        assert_eq!(l.rename("x", "y").err(), Some(NsCode::CrossDev));
    }

    #[test]
    fn dir_unlink_blocked() {
        let mut l = NsLedger::new();
        let _ = l.mkdir("d", 0);
        assert_eq!(l.unlink("d").err(), Some(NsCode::IsDirUnlink));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_ns_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_unlink_rename_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(PathState::Vacuum.can_advance(PathState::Linked));
        assert!(PathState::Linked.can_advance(PathState::Renamed));
        assert!(!PathState::Gone.can_advance(PathState::Vacuum));
        assert_eq!(PathState::Unlinked.label(), "已解链");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10115_checks_green() {
        let set = run_unx10115_checks();
        assert!(set.all_passed(), "UNX-F10115 red: {}", set.red_items_text());
    }
}
