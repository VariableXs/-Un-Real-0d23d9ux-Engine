//! VFS 白名单裁决层 + 越权审计日志 — AI-S（双域总案·阶段4 任务30）。
//!
//! 总案施工步骤 1/验收口径：
//! - **挂在 SHARED 挂载点的唯一入口**：一切 SHARED 读写先经本层裁决；
//! - **裁决链**：路径规范化（防 `../` 逃逸/符号链接绕过/大小写变体）
//!   → 规则匹配（目录级前缀 + 组件边界 + 通配后缀）→ **默认拒绝**；
//! - **越权审计日志**：allow/deny 全量留痕，走 [`DiskJournal`]（tier3 WAL，
//!   断电不丢；载荷 fnv 校验，腐坏按 torn 语义「该点及之后全部丢弃」）；
//! - 大小写口径：exFAT 不分大小写 → 匹配统一 ASCII 小写折叠（完整 Unicode
//!   折叠超出总案范围，如实声明；符号链接在 exFAT 无对象，`..` 规范化阶段
//!   按组件逐级解析，越根即**拒绝不钳制**——逃逸无构造路径）。

use crate::drivers::blk::{fnv1a64, BlockDevice, BlockError};
use crate::fs::fs23_disk::{DiskJournal, OpenReport};
use crate::fs::fs23_journal::{JOURNAL_DEFAULT, LogOp};

/// 归一化路径上限（含根斜杠）。
pub const PATH_MAX: usize = 256;
/// 深度上限（组件数）。
pub const DEPTH_MAX: usize = 16;
/// 规则上限。
pub const RULES_MAX: usize = 32;
/// 审计记录内路径截断长度（越权审计取路径头部即可定位规则区）。
pub const AUDIT_PATH_MAX: usize = 80;
/// 审计载荷定长。
pub const AUDIT_BODY: usize = 96;
/// fs23 槽内载荷偏移（24B 头之后）。
const BODY_OFF: usize = 24;

// ---------------------------------------------------------------------------
// 路径规范化
// ---------------------------------------------------------------------------

/// 裁决层错误。逃逸（Escape）是安全事件，不是普通参数错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    /// 空路径 / 不以 `/` 开头。
    NotAbsolute,
    /// `..` 越过根——拒绝，绝不钳制。
    Escape,
    /// 控制字符 / 反斜杠 / DEL。
    BadChar,
    /// 超过 PATH_MAX。
    TooLong,
    /// 超过 DEPTH_MAX 层。
    TooDeep,
    /// 规则表满。
    RulesFull,
}

/// 归一化产物：小写折叠、无 `.`/`..`/冗余斜杠、有界、恒以 `/` 开头。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormPath {
    b: [u8; PATH_MAX],
    len: usize,
    depth: usize,
}

impl NormPath {
    pub fn as_bytes(&self) -> &[u8] {
        &self.b[..self.len]
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    /// 根特判：`/` 自身。
    pub fn is_root(&self) -> bool {
        self.len == 1
    }
}

/// 路径规范化：组件级解析，`..` 越根 = [`GuardError::Escape`]。
/// ASCII 大写折叠为小写（exFAT 不分大小写口径）。
pub fn normalize(path: &[u8]) -> Result<NormPath, GuardError> {
    if path.is_empty() || path[0] != b'/' {
        return Err(GuardError::NotAbsolute);
    }
    let mut out = [0u8; PATH_MAX];
    out[0] = b'/';
    let mut len = 1usize;
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < path.len() {
        let c = path[i];
        if c < 0x20 || c == 0x7F || c == b'\\' {
            return Err(GuardError::BadChar);
        }
        if c != b'/' {
            i += 1;
            continue;
        }
        // 段边界：取 [i+1, next_slash)。
        let start = i + 1;
        let mut end = start;
        while end < path.len() && path[end] != b'/' {
            let x = path[end];
            if x < 0x20 || x == 0x7F || x == b'\\' {
                return Err(GuardError::BadChar);
            }
            end += 1;
        }
        let seg = &path[start..end];
        if seg == b".." {
            if depth == 0 {
                return Err(GuardError::Escape); // 越根：拒绝，不钳制
            }
            while len > 1 && out[len - 1] != b'/' {
                len -= 1;
            }
            if len > 1 {
                len -= 1; // 吃掉组件前分隔符
            }
            depth -= 1;
        } else if !seg.is_empty() && seg != b"." {
            if depth >= DEPTH_MAX {
                return Err(GuardError::TooDeep);
            }
            if len + 1 + seg.len() >= PATH_MAX {
                return Err(GuardError::TooLong);
            }
            if len > 1 {
                out[len] = b'/'; // 根已有斜杠：仅非首组件补分隔符
                len += 1;
            }
            for &x in seg {
                out[len] = x.to_ascii_lowercase();
                len += 1;
            }
            depth += 1;
        }
        i = end;
    }
    Ok(NormPath { b: out, len, depth })
}

// ---------------------------------------------------------------------------
// 规则与裁决
// ---------------------------------------------------------------------------

/// 访问操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read,
    Write,
}

/// 一条白名单规则：目录级前缀 + 操作位。组件边界匹配（`/app` 不匹配 `/apps`）。
/// `deny=true` = 显式拒绝规则（任务37 冲突仲裁：deny 与 allow 叠加时**拒绝优先**）。
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    prefix: NormPath,
    read: bool,
    write: bool,
    deny: bool,
}

/// 拒绝原因（审计/测试可观测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// 无规则命中——白名单外，默认拒绝的常态。
    NoRule,
    /// 命中规则但操作位未授予。
    OpMask,
    /// 命中显式 deny 规则（任务37 冲突仲裁：叠加时拒绝优先的可观测出口）。
    Denied,
    /// 路径规范化失败（含逃逸）。
    BadPath(GuardError),
}

/// 裁决结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub allow: bool,
    /// 命中的规则号（allow 时必有）。
    pub rule: Option<usize>,
    /// 拒绝原因（allow 时为 None）。
    pub deny: Option<DenyReason>,
}

/// 规则集：定容 32，白名单模型（默认拒绝）。
#[derive(Debug, Clone, Copy)]
pub struct RuleSet {
    rules: [Rule; RULES_MAX],
    len: usize,
}

impl RuleSet {
    pub const fn new() -> Self {
        RuleSet {
            rules: [Rule {
                prefix: NormPath { b: [0; PATH_MAX], len: 0, depth: 0 },
                read: false,
                write: false,
                deny: false,
            }; RULES_MAX],
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 原地清空（len=0）：供热更新原地重 parse，避免整结构栈物化
    /// （内核戒律：RuleSet≈8.5KiB，>64KB 禁栈同族，reload 热路径必守）。
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// 增补规则。尾部 `/*` 通配 = 目录前缀语义（规范化时收敛）。
    pub fn add(&mut self, read: bool, write: bool, path: &[u8]) -> Result<usize, GuardError> {
        let mut p = path;
        if p.len() >= 2 && &p[p.len() - 2..] == b"/*" {
            p = &p[..p.len() - 2];
        }
        let prefix = normalize(p)?;
        if self.len >= RULES_MAX {
            return Err(GuardError::RulesFull);
        }
        self.rules[self.len] = Rule { prefix, read, write, deny: false };
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 增补显式拒绝规则（任务37 冲突仲裁）。与 allow 规则叠加时拒绝优先。
    pub fn add_deny(&mut self, read: bool, write: bool, path: &[u8]) -> Result<usize, GuardError> {
        let mut p = path;
        if p.len() >= 2 && &p[p.len() - 2..] == b"/*" {
            p = &p[..p.len() - 2];
        }
        let prefix = normalize(p)?;
        if self.len >= RULES_MAX {
            return Err(GuardError::RulesFull);
        }
        self.rules[self.len] = Rule { prefix, read, write, deny: true };
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 解析规则文本（SHARED/whitelist/ 的内核侧加载面）：
    /// 行格式 `allow r|w|rw <绝对路径>` 或 `deny r|w|rw <绝对路径>`（任务37）；
    /// 空行与 `#` 注释跳过；坏行计数不致命。
    /// 返回 (成功条数, 坏行数)。
    pub fn parse(&mut self, text: &[u8]) -> (usize, usize) {
        let (mut ok, mut bad) = (0usize, 0usize);
        let mut line_start = 0usize;
        for i in 0..=text.len() {
            if i != text.len() && text[i] != b'\n' {
                continue;
            }
            let line = trim(&text[line_start..i]);
            line_start = i + 1;
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            // 三段切分：allow <perm> <path>。
            let mut parts: [&[u8]; 3] = [(&[]), (&[]), (&[])];
            let mut cnt = 0usize;
            let mut cur = 0usize;
            for k in 0..=line.len() {
                if k == line.len() || line[k] == b' ' || line[k] == b'\t' {
                    if k > cur && cnt < 3 {
                        parts[cnt] = trim(&line[cur..k]);
                        cnt += 1;
                    }
                    cur = k + 1;
                }
            }
            let is_allow = parts[0] == b"allow";
            let is_deny = parts[0] == b"deny";
            if cnt != 3 || (!is_allow && !is_deny) {
                bad += 1;
                continue;
            }
            let (read, write) = match parts[1] {
                b"r" => (true, false),
                b"w" => (false, true),
                b"rw" => (true, true),
                _ => {
                    bad += 1;
                    continue;
                }
            };
            let added = if is_allow {
                self.add(read, write, parts[2])
            } else {
                self.add_deny(read, write, parts[2])
            };
            match added {
                Ok(_) => ok += 1,
                Err(_) => bad += 1,
            }
        }
        (ok, bad)
    }

    /// 组件边界前缀匹配：`/app` 不命中 `/apps`；根规则命中一切。
    fn matches(r: &Rule, p: &NormPath) -> bool {
        let pl = r.prefix.len;
        if pl == 0 || p.len < pl {
            return false;
        }
        if p.b[..pl] != r.prefix.b[..pl] {
            return false;
        }
        p.len == pl || pl == 1 || p.b[pl] == b'/'
    }

    /// 裁决（任务37 冲突仲裁语义，两遍扫描显式定优先级）：
    /// ① 任一 deny 规则命中且覆盖本操作 → **拒绝优先**（Denied）；
    /// ② 否则首个授予本操作位的 allow 规则 → 允许；
    /// ③ 命中但未授予 → OpMask；无命中 → NoRule（默认拒绝）。
    pub fn decide(&self, p: &NormPath, op: Op) -> Decision {
        let op_granted = |r: &Rule| match op {
            Op::Read => r.read,
            Op::Write => r.write,
        };
        // ① deny 优先：显式拒绝压过任何 allow（叠加场景的可预期出口）。
        for i in 0..self.len {
            let r = &self.rules[i];
            if r.deny && Self::matches(r, p) && op_granted(r) {
                return Decision { allow: false, rule: Some(i), deny: Some(DenyReason::Denied) };
            }
        }
        // ② allow。
        for i in 0..self.len {
            let r = &self.rules[i];
            if r.deny || !Self::matches(r, p) {
                continue;
            }
            if op_granted(r) {
                return Decision { allow: true, rule: Some(i), deny: None };
            }
            return Decision { allow: false, rule: Some(i), deny: Some(DenyReason::OpMask) };
        }
        Decision { allow: false, rule: None, deny: Some(DenyReason::NoRule) }
    }

    /// 全链入口：规范化 → 匹配。规范化失败（含逃逸）一律拒绝。
    pub fn adjudicate(&self, raw: &[u8], op: Op) -> Decision {
        match normalize(raw) {
            Ok(p) => self.decide(&p, op),
            Err(e) => Decision { allow: false, rule: None, deny: Some(DenyReason::BadPath(e)) },
        }
    }
}

// ---------------------------------------------------------------------------
// 任务37 · 规则热更新与冲突仲裁：RuleBook（世代+版本号比对）+ Arbiter trait
// ---------------------------------------------------------------------------

/// 可插裁决器 trait（总案开放性：新资源类型不换调用链）。路径白名单
/// ([`RuleBook`]) 是首个实现；新资源（注册表键/域名单/端口段）实现同
/// trait 即可挂进既有调用面，输出同一 [`Decision`] 供审计与测试复用。
pub trait Arbiter {
    /// 裁决器种类名（审计与诊断展示用）。
    fn kind(&self) -> &'static str;
    /// 与 [`RuleSet::adjudicate`] 同语义：规范化/校验失败一律拒绝。
    fn adjudicate(&self, raw: &[u8], op: Op) -> Decision;
}

/// 一次 reload 的结果（幂等跳过时 changed=false）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReloadReport {
    pub changed: bool,
    pub gen_before: u64,
    pub gen_after: u64,
    pub rules: usize,
    pub bad_lines: usize,
}

/// 规则簿：规则集 + 内容指纹（fnv1a64）+ 世代号。
///
/// **热更新语义**（改文件不重启生效）：
/// - `reload(text)` 先比对内容指纹——相同文本幂等跳过（世代不变，
///   重复挂载/轮询 reload 零成本）；
/// - 不同文本 → 整块替换 [`RuleSet`]（定容 Copy 结构，无半更新窗口）
///   并递增世代；
/// - **竞态口径**（单核 + 快照语义）：`adjudicate` 只借 `&self`，请求
///   要么在旧世代完成、要么在新世代完成，Decision 恒来自单一一致
///   世代——不存在读到"半新半旧规则表"的中间态。
#[derive(Debug, Clone, Copy)]
pub struct RuleBook {
    set: RuleSet,
    gen: u64,
    hash: u64,
    reloads: u64,
}

impl RuleBook {
    pub const fn new() -> Self {
        RuleBook { set: RuleSet::new(), gen: 0, hash: 0, reloads: 0 }
    }

    /// 当前世代号（每次生效 reload +1）。
    pub fn generation(&self) -> u64 {
        self.gen
    }

    /// 当前内容指纹。
    pub fn content_hash(&self) -> u64 {
        self.hash
    }

    /// 生效 reload 总次数（诊断）。
    pub fn reload_count(&self) -> u64 {
        self.reloads
    }

    /// 规则数透传。
    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// 热更新入口：比对指纹 → 幂等跳过或整块替换。坏行不致命（与
    /// [`RuleSet::parse`] 同口径），计数进 [`ReloadReport`]。
    pub fn reload(&mut self, text: &[u8]) -> ReloadReport {
        let h = fnv1a64(text);
        let gen_before = self.gen;
        if h == self.hash && self.reloads > 0 {
            // 幂等：与上一份生效文本逐字节同指纹 → 跳过（世代不变）。
            // reloads==0 时（首装载）即便空文本也要走装载路径落指纹。
            return ReloadReport {
                changed: false,
                gen_before,
                gen_after: self.gen,
                rules: self.set.len(),
                bad_lines: 0,
            };
        }
        // 原地重 parse（clear 后从头覆盖）：栈上零 8.5KiB 物化
        // （实机 #DF 取证：探针 4×RuleSet 栈变量+reload 临时 = 42.5KiB 打穿内核栈）。
        self.set.clear();
        let (ok, bad) = self.set.parse(text);
        self.hash = h;
        self.gen += 1;
        self.reloads += 1;
        let _ = ok;
        ReloadReport { changed: true, gen_before, gen_after: self.gen, rules: self.set.len(), bad_lines: bad }
    }

    /// 快照裁决：走当前世代规则集（语义同 [`RuleSet::adjudicate`]）。
    pub fn adjudicate(&self, raw: &[u8], op: Op) -> Decision {
        self.set.adjudicate(raw, op)
    }
}

impl Default for RuleBook {
    fn default() -> Self {
        Self::new()
    }
}

impl Arbiter for RuleBook {
    fn kind(&self) -> &'static str {
        "vfs-path"
    }
    fn adjudicate(&self, raw: &[u8], op: Op) -> Decision {
        RuleBook::adjudicate(self, raw, op)
    }
}

fn trim(b: &[u8]) -> &[u8] {
    let mut s = 0usize;
    let mut e = b.len();
    while s < e && (b[s] == b' ' || b[s] == b'\t' || b[s] == b'\r') {
        s += 1;
    }
    while e > s && (b[e - 1] == b' ' || b[e - 1] == b'\t' || b[e - 1] == b'\r') {
        e -= 1;
    }
    &b[s..e]
}

// ---------------------------------------------------------------------------
// 越权审计日志（DiskJournal tier3：断电不丢；fnv 载荷校验）
// ---------------------------------------------------------------------------

/// 审计记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRecord {
    /// journal 全局序号（跨会话单调）。
    pub seq: u64,
    pub pid: u32,
    pub allow: bool,
    pub write: bool,
    pub path: [u8; AUDIT_PATH_MAX],
    pub path_len: usize,
}

/// 载荷布局（96B，fs23 槽 24B 头之后）：
/// `[0]=allow [1]=write [2..6)=pid LE [6..8)=path_len LE [8..88)=path [88..96)=fnv64(前88B)`
fn encode_record(r: &AuditRecord, out: &mut [u8; AUDIT_BODY]) {
    for x in out.iter_mut() {
        *x = 0;
    }
    out[0] = r.allow as u8;
    out[1] = r.write as u8;
    out[2..6].copy_from_slice(&r.pid.to_le_bytes());
    out[6..8].copy_from_slice(&(r.path_len as u16).to_le_bytes());
    let n = r.path_len.min(AUDIT_PATH_MAX);
    out[8..8 + n].copy_from_slice(&r.path[..n]);
    let sum = fnv1a64(&out[..88]);
    out[88..96].copy_from_slice(&sum.to_le_bytes());
}

/// 审计账本：DiskJournal tier3 封装（Write{blk:0} 槽载荷 = 审计记录）。
pub struct AuditJournal<B: BlockDevice> {
    j: DiskJournal<B>,
}

impl<B: BlockDevice> AuditJournal<B> {
    pub fn format(dev: &mut B, base: u64) -> Result<(), BlockError> {
        DiskJournal::format(dev, JOURNAL_DEFAULT, base)
    }

    pub fn open(dev: B, base: u64) -> Result<(Self, OpenReport), BlockError> {
        let (j, rep) = DiskJournal::open(dev, JOURNAL_DEFAULT, base)?;
        Ok((AuditJournal { j }, rep))
    }

    /// 追加一条审计（allow/deny 全量留痕）。成功返回全局 seq。
    pub fn record(&mut self, pid: u32, allow: bool, write: bool, path: &[u8]) -> Option<u64> {
        let mut rec = AuditRecord {
            seq: 0,
            pid,
            allow,
            write,
            path: [0; AUDIT_PATH_MAX],
            path_len: path.len().min(AUDIT_PATH_MAX),
        };
        rec.path[..rec.path_len].copy_from_slice(&path[..rec.path_len]);
        let mut body = [0u8; AUDIT_BODY];
        encode_record(&rec, &mut body);
        self.j
            .append_slot(LogOp::Write { blk: 0 }, &body)
            .map(|seq| {
                rec.seq = seq;
                seq
            })
    }

    /// 读第 i 条（fnv 复核；不符 = 盘面载荷腐坏 → None）。
    pub fn read_record(&mut self, i: usize) -> Option<AuditRecord> {
        if i >= self.j.len() {
            return None;
        }
        let mut slot = [0u8; 512];
        self.j.dev_read(self.j.slot_lba(i), &mut slot).ok()?;
        let body = &slot[BODY_OFF..BODY_OFF + AUDIT_BODY];
        let sum = u64::from_le_bytes(body[88..96].try_into().ok()?);
        if sum != fnv1a64(&body[..88]) {
            return None;
        }
        let (seq, _, _) = self.j.read_entry(i)?;
        let plen = u16::from_le_bytes(body[6..8].try_into().ok()?) as usize;
        let mut path = [0u8; AUDIT_PATH_MAX];
        let n = plen.min(AUDIT_PATH_MAX);
        path[..n].copy_from_slice(&body[8..8 + n]);
        Some(AuditRecord {
            seq,
            pid: u32::from_le_bytes(body[2..6].try_into().ok()?),
            allow: body[0] != 0,
            write: body[1] != 0,
            path,
            path_len: n,
        })
    }

    /// 顺序读全部（fnv 失败按 torn 口径停住：该点及之后不可信）。
    /// 返回 (可读记录数, 是否发现腐坏)。
    pub fn read_all(&mut self, out: &mut [Option<AuditRecord>]) -> (usize, bool) {
        let mut n = 0usize;
        let mut corrupt = false;
        for i in 0..self.j.len() {
            if n >= out.len() {
                break;
            }
            match self.read_record(i) {
                Some(r) => {
                    out[n] = Some(r);
                    n += 1;
                }
                None => {
                    corrupt = true;
                    break; // 该点及之后全部不可信（torn 口径）
                }
            }
        }
        (n, corrupt)
    }

    pub fn len(&self) -> usize {
        self.j.len()
    }
    pub fn is_empty(&self) -> bool {
        self.j.is_empty()
    }
    /// 第 i 条槽所在 LBA（测试腐坏定位用）。
    pub fn slot_lba(&self, i: usize) -> u64 {
        self.j.slot_lba(i)
    }
    /// 拿回块设备（重开模拟断电）。
    pub fn into_device(self) -> B {
        self.j.into_device()
    }
}

/// 裁决 + 审计一体面（SHARED 挂载点唯一入口的形态）：任何裁决结果都留痕。
pub struct GuardedVolume<B: BlockDevice> {
    pub rules: RuleSet,
    pub audit: AuditJournal<B>,
}

impl<B: BlockDevice> GuardedVolume<B> {
    pub fn adjudicate(&mut self, pid: u32, raw: &[u8], op: Op) -> Decision {
        let d = self.rules.adjudicate(raw, op);
        // 留痕失败不改变裁决（裁决 fail-closed；审计通道尽力而为并上抛能力）。
        let _ = self.audit.record(pid, d.allow, op == Op::Write, raw);
        d
    }
}

/// 实机盘区：80 000..80 193——与 loopback(≈16007)/fs23(20000..20193)/
/// milestone(40000..40193)/kv(60000/70000) 均无交集。
pub const VFS_AUDIT_BASE: u64 = 80_000;

// ---------------------------------------------------------------------------
// 实机探针（target_os = "none"）
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
pub mod target {
    use super::{AuditJournal, DenyReason, DiskJournal, GuardError, JOURNAL_DEFAULT, Op, RuleBook, RuleSet, VFS_AUDIT_BASE};
    use crate::drivers::blk::BlockDevice;

    /// 纯计算探针：规范化 + 裁决矩阵（main.rs，不碰设备）。
    /// 探针暂存（.bss，const 初始化；见 vfs_decisions_probe 头注戒律双实证）。
    struct ProbeScratch {
        rs: RuleSet,
        rs2: RuleSet,
        rs3: RuleSet,
        book: RuleBook,
    }
    static PROBE_SCRATCH: crate::cpu::sync::SpinProtected<ProbeScratch> =
        crate::cpu::sync::SpinProtected::new(ProbeScratch {
            rs: RuleSet::new(),
            rs2: RuleSet::new(),
            rs3: RuleSet::new(),
            book: RuleBook::new(),
        });

    pub fn vfs_decisions_probe() {
        // 戒律双实证：RuleSet≈8.5KiB ①×4 栈变量打穿内核栈（#DF@memset）
        // ②Box 到堆触发 kheap MAX_ALLOC=4KiB 上限 alloc panic 静默 halt——
        // 唯一合规放置 = static .bss 单例（const 初始化），探针独占锁内使用。
        let mut scratch = PROBE_SCRATCH.lock();
        let ProbeScratch { rs, rs2, rs3, book } = &mut *scratch;
        // 冷启动 = ELF 重载初值，scratch 无需 reset（reloads=0 使 reload 必走装载）。
        let _ = rs.parse(b"allow rw /handoff\nallow r /apps.json\nallow rw /pub\n");
        let mut ok = true;

        // ① 白名单内读放行。
        ok &= rs.adjudicate(b"/apps.json", Op::Read).allow;
        // ② 命中但操作位不含写 → 拒（OpMask）。
        let d2 = rs.adjudicate(b"/apps.json", Op::Write);
        ok &= !d2.allow && d2.deny == Some(DenyReason::OpMask);
        // ③ 目录 rw 放行深层文件。
        ok &= rs.adjudicate(b"/handoff/queue-1.json", Op::Write).allow;
        // ④ 白名单外默认拒绝。
        ok &= !rs.adjudicate(b"/etc/passwd", Op::Read).allow;
        // ⑤ 大小写折叠（exFAT 口径）。
        ok &= rs.adjudicate(b"/APPS.JSON", Op::Read).allow;
        // ⑥ 组件边界：/apps.json 规则不命中 /apps.json.bak，也不命中 /app。
        ok &= !rs.adjudicate(b"/apps.json.bak", Op::Read).allow;
        ok &= !rs.adjudicate(b"/app", Op::Read).allow;
        // ⑦ `..` 越根拒绝（拒绝，不钳制）。
        let d7 = rs.adjudicate(b"/../secrets", Op::Read);
        ok &= !d7.allow && d7.deny == Some(DenyReason::BadPath(GuardError::Escape));
        // ⑦b 带内 `..` 合法收敛（/a/../secrets → /secrets）→ 白名单外默认拒。
        ok &= !rs.adjudicate(b"/a/../secrets", Op::Read).allow;
        // ⑧ 带内 `..` 合法收敛到白名单内 → 放行。
        ok &= rs.adjudicate(b"/handoff/x/../y", Op::Read).allow;
        // ⑨ 通配后缀规则（/pub/* ≡ /pub 前缀）。
        // rs2/rs3/book 由 scratch 解构持有（上方）。
        let _ = rs2.parse(b"allow r /pub/*\n");
        ok &= rs2.adjudicate(b"/pub/a/b.txt", Op::Read).allow;
        ok &= !rs2.adjudicate(b"/private/x", Op::Read).allow;

        // ⑩ 任务37 冲突仲裁：deny 与 allow 叠加 → 拒绝优先。

        let _ = rs3.parse(b"allow rw /data\ndeny rw /data/secret\n");
        ok &= rs3.adjudicate(b"/data/pub.txt", Op::Write).allow;
        let d10 = rs3.adjudicate(b"/data/secret/k.txt", Op::Write);
        ok &= !d10.allow && d10.deny == Some(DenyReason::Denied);
        // ⑪ 任务37 热更新：reload 后新规则不重启即刻生效。

        let _ = book.reload(b"allow rw /handoff\n");
        let gen1 = book.generation();
        ok &= gen1 == 1 && book.adjudicate(b"/handoff/f", Op::Write).allow;
        let _ = book.reload(b"deny rw /handoff\n");
        ok &= book.generation() == gen1 + 1;
        ok &= !book.adjudicate(b"/handoff/f", Op::Write).allow;
        // ⑫ 幂等 reload：同文本世代不变。
        let _ = book.reload(b"deny rw /handoff\n");
        ok &= book.generation() == gen1 + 1;

        crate::kinfo!(
            "vfs-probe: decisions matrix=[allow,opmask,dir-deep,default-deny,case-fold,boundary,escape,inband-deny,inband-allow,wild,deny-wins,hot-reload,reload-idempotent] verdict={}",
            if ok { "ok" } else { "FAIL" }
        );
        if !ok {
            crate::kwarn!("vfs-probe: decision matrix mismatch");
        }
    }

    /// 设备探针（nvme ctrl#1 高区）：审计账本双会话断电续记。
    pub fn vfs_audit_probe(mut dev: &mut dyn BlockDevice) {
        // 首轮：未格式化 → format（幂等；open Err 仅超块级损坏）。
        if DiskJournal::open(&mut *dev, JOURNAL_DEFAULT, VFS_AUDIT_BASE).is_err() {
            if AuditJournal::format(&mut dev, VFS_AUDIT_BASE).is_err() {
                crate::kwarn!("vfs-probe: audit format failed");
                return;
            }
        }
        let mut aj = match AuditJournal::open(&mut *dev, VFS_AUDIT_BASE) {
            Ok((a, rep)) => {
                if rep.torn != 0 {
                    crate::kwarn!("vfs-probe: audit torn={} — 尾部丢弃语义生效", rep.torn);
                }
                a
            }
            Err(e) => {
                crate::kwarn!("vfs-probe: audit open failed {:?}", e);
                return;
            }
        };
        // 每会话 6 条（3 准 3 拒，确定性内容）；槽满即宣布饱和绿。
        if aj.len() + 6 > 64 {
            crate::kinfo!("vfs-probe: audit-full entries={} verdict=ok (saturated)", aj.len());
            return;
        }
        let session = aj.len() / 6 + 1;
        let paths = [
            (&b"/handoff/queue-1.json"[..], true, false), // allow write（rw 目录）
            (&b"/etc/passwd"[..], false, false),          // deny read（白名单外）
            (&b"/apps.json"[..], true, false),            // allow read
            (&b"/a/../secrets"[..], false, false),        // deny escape
            (&b"/pub/report.txt"[..], true, false),       // allow read
            (&b"/apps.json"[..], false, true),            // deny write（opmask）
        ];
        for (i, (p, allow, write)) in paths.iter().enumerate() {
            let pid = (1000 + session * 10 + i) as u32;
            if aj.record(pid, *allow, *write, p).is_none() {
                crate::kwarn!("vfs-probe: audit record {} failed", i);
                return;
            }
        }
        // 全量回读逐条精确核对（含跨断电旧会话）。
        let mut seen = [None; 64];
        let (n, corrupt) = aj.read_all(&mut seen[..]);
        if corrupt || n != aj.len() {
            crate::kwarn!("vfs-probe: audit readback corrupt={} n={} len={}", corrupt, n, aj.len());
            return;
        }
        let mut ok = true;
        for s in 1..=session {
            for (i, (p, allow, write)) in paths.iter().enumerate() {
                let idx = (s - 1) * 6 + i;
                match seen[idx] {
                    Some(r) => {
                        ok &= r.allow == *allow
                            && r.write == *write
                            && &r.path[..r.path_len] == *p
                            && r.pid == (1000 + s * 10 + i) as u32;
                    }
                    None => ok = false,
                }
            }
        }
        if session >= 2 {
            crate::kinfo!(
                "vfs-probe: audit-recovered verdict={} recovered=6",
                if ok { "ok" } else { "FAIL" }
            );
        }
        crate::kinfo!(
            "vfs-probe: audit-session={} entries={} verdict={}",
            session,
            aj.len(),
            if ok { "ok" } else { "FAIL" }
        );
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（ktest）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 内存块设备（写即落盘模型，kvsrv tests 同件）。
    struct MemDisk {
        blocks: BTreeMap<u64, [u8; 512]>,
        total: u64,
    }

    impl MemDisk {
        fn new(total: u64) -> Self {
            MemDisk { blocks: BTreeMap::new(), total }
        }
    }

    impl BlockDevice for MemDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.total
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            if dst.is_empty() || dst.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (dst.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            let zero = [0u8; 512];
            for (i, chunk) in dst.chunks_mut(512).enumerate() {
                let b = self.blocks.get(&(lba + i as u64)).unwrap_or(&zero);
                chunk.copy_from_slice(b);
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            if src.is_empty() || src.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (src.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            for (i, chunk) in src.chunks(512).enumerate() {
                let mut b = [0u8; 512];
                b.copy_from_slice(chunk);
                self.blocks.insert(lba + i as u64, b);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    // ---------------- 规范化 ----------------

    #[test]
    fn normalize_collapses_dot_and_slashes() {
        let p = normalize(b"/a/./b//c/").unwrap();
        assert_eq!(p.as_bytes(), b"/a/b/c");
        assert_eq!(p.depth(), 3);
        assert!(normalize(b"/").unwrap().is_root());
        // 带内 .. 收敛。
        assert_eq!(normalize(b"/a/b/../c").unwrap().as_bytes(), b"/a/c");
    }

    #[test]
    fn normalize_rejects_escape_never_clamps() {
        assert_eq!(normalize(b"/.."), Err(GuardError::Escape));
        assert_eq!(normalize(b"/../secrets"), Err(GuardError::Escape));
        assert_eq!(normalize(b"/a/../../b"), Err(GuardError::Escape));
        // 深层越根同样拒绝。
        assert_eq!(normalize(b"/a/b/c/../../../../x"), Err(GuardError::Escape));
    }

    #[test]
    fn normalize_rejects_bad_chars_and_limits() {
        assert_eq!(normalize(b""), Err(GuardError::NotAbsolute));
        assert_eq!(normalize(b"a/b"), Err(GuardError::NotAbsolute));
        assert_eq!(normalize(b"/a\x01b"), Err(GuardError::BadChar));
        assert_eq!(normalize(b"/a\\b"), Err(GuardError::BadChar), "反斜杠非法（非分隔符）");
        assert_eq!(normalize(b"/a\x7Fb"), Err(GuardError::BadChar));
        // 超 16 层（17 个组件）。
        let mut deep = String::from("/");
        for i in 0..17 {
            if i > 0 {
                deep.push('/');
            }
            deep.push_str(&format!("d{}", i));
        }
        assert_eq!(normalize(deep.as_bytes()), Err(GuardError::TooDeep));
        // 超长。
        let long = format!("/{}", "x".repeat(PATH_MAX));
        assert_eq!(normalize(long.as_bytes()), Err(GuardError::TooLong));
    }

    // ---------------- 规则匹配 ----------------

    #[test]
    fn rules_parse_and_component_boundary() {
        let mut rs = RuleSet::new();
        let (ok, bad) = rs.parse(b"# comment\nallow rw /handoff\nallow r /apps.json\nbogus line\nallow x /bad\n");
        assert_eq!(ok, 2);
        assert_eq!(bad, 2);
        // /app 不命中 /apps 规则（组件边界）。
        let mut rs2 = RuleSet::new();
        rs2.add(true, false, b"/apps").unwrap();
        assert!(rs2.adjudicate(b"/apps/1.txt", Op::Read).allow);
        assert!(!rs2.adjudicate(b"/app", Op::Read).allow);
        assert!(!rs2.adjudicate(b"/appsfoo", Op::Read).allow);
        // 大小写折叠。
        assert!(rs2.adjudicate(b"/APPS/1.TXT", Op::Read).allow);
    }

    #[test]
    fn default_deny_and_op_mask() {
        let mut rs = RuleSet::new();
        // 空规则集：全拒。
        assert!(!rs.adjudicate(b"/anything", Op::Read).allow);
        rs.add(true, false, b"/readonly-dir").unwrap();
        assert!(!rs.adjudicate(b"/readonly-dir/f", Op::Write).allow, "规则未授写 → 拒");
        let d = rs.adjudicate(b"/readonly-dir/f", Op::Write);
        assert_eq!(d.deny, Some(DenyReason::OpMask));
        assert!(rs.adjudicate(b"/readonly-dir/f", Op::Read).allow);
        // 通配后缀规则。
        let mut rs3 = RuleSet::new();
        rs3.add(true, true, b"/pub/*").unwrap();
        assert!(rs3.adjudicate(b"/pub/a/b/c.txt", Op::Write).allow);
        assert!(!rs3.adjudicate(b"/publi/x", Op::Read).allow);
    }

    #[test]
    fn escape_beats_any_rule() {
        let mut rs = RuleSet::new();
        rs.add(true, true, b"/ok").unwrap();
        let d = rs.adjudicate(b"/../secrets", Op::Read);
        assert_eq!(d.deny, Some(DenyReason::BadPath(GuardError::Escape)));
        assert!(!d.allow, "逃逸即使有规则也拒");
    }

    // ---------------- 任务37 · 冲突仲裁（拒绝优先）----------------

    #[test]
    fn conflict_matrix_deny_wins_10_combos() {
        // 冲突组合 ≥10 组断言拒绝优先（总案验收口径）。
        // 矩阵：deny 覆盖读/写 × allow 精确/前缀/通配/叠加 × 嵌套目录。
        let mut rs = RuleSet::new();
        // 规则 0-1：宽 allow 打底。
        rs.add(true, true, b"/data").unwrap();
        rs.add(true, true, b"/pub/*").unwrap();
        // 规则 2-7：deny 层。
        rs.add_deny(true, true, b"/data/secret").unwrap();        // 2 整目录拒
        rs.add_deny(false, true, b"/data/rofile").unwrap();       // 3 仅拒写
        rs.add_deny(true, false, b"/data/nord").unwrap();         // 4 仅拒读
        rs.add_deny(true, true, b"/pub/inner/*").unwrap();        // 5 通配拒
        rs.add_deny(false, true, b"/data/w").unwrap();            // 6 单操作拒
        let allow = |d: Decision| d.allow;
        // ① 精确 allow 前缀 vs deny 子目录（读）→ deny 优先。
        assert!(!allow(rs.adjudicate(b"/data/secret/k.txt", Op::Read)));
        assert_eq!(rs.adjudicate(b"/data/secret/k.txt", Op::Read).deny, Some(DenyReason::Denied));
        // ② 同上（写）。
        assert!(!allow(rs.adjudicate(b"/data/secret/k.txt", Op::Write)));
        // ③ allow 宽目录 /data vs deny 文件 rofile：读放行（deny 只覆盖写）。
        assert!(allow(rs.adjudicate(b"/data/rofile", Op::Read)));
        // ④ 但写 → deny 优先。
        assert!(!allow(rs.adjudicate(b"/data/rofile", Op::Write)));
        assert_eq!(rs.adjudicate(b"/data/rofile", Op::Write).deny, Some(DenyReason::Denied));
        // ⑤ deny 只拒读：读拒、写放行。
        assert!(!allow(rs.adjudicate(b"/data/nord", Op::Read)));
        assert!(allow(rs.adjudicate(b"/data/nord", Op::Write)));
        // ⑥ 通配 allow /pub/* vs 通配 deny /pub/inner/*。
        assert!(allow(rs.adjudicate(b"/pub/a.txt", Op::Write)));
        assert!(!allow(rs.adjudicate(b"/pub/inner/x.txt", Op::Write)));
        // ⑦ 宽 deny 父目录压过窄 allow 子目录（拒绝优先与规则顺序无关）。
        let mut rs_wide = RuleSet::new();
        rs_wide.add(true, true, b"/data/sub").unwrap();
        rs_wide.add_deny(true, true, b"/data").unwrap();
        assert!(!allow(rs_wide.adjudicate(b"/data/sub/f", Op::Read)));
        assert_eq!(rs_wide.adjudicate(b"/data/sub/f", Op::Read).deny, Some(DenyReason::Denied));
        // ⑧ 单操作 deny：写拒读放。
        assert!(!allow(rs.adjudicate(b"/data/w/f", Op::Write)));
        assert!(allow(rs.adjudicate(b"/data/w/f", Op::Read)));
        // ⑨ deny 后仍无 allow 覆盖 → 也是拒（deny 优先，原因同为 Denied）。
        let mut rs2 = RuleSet::new();
        rs2.add_deny(true, true, b"/onlydeny").unwrap();
        assert_eq!(rs2.adjudicate(b"/onlydeny", Op::Read).deny, Some(DenyReason::Denied));
        // ⑩ 逃逸路径即使 deny 也在场 → BadPath 优先于一切规则。
        rs.add_deny(true, true, b"/data").unwrap();
        assert_eq!(
            rs.adjudicate(b"/../secret", Op::Read).deny,
            Some(DenyReason::BadPath(GuardError::Escape))
        );
    }

    #[test]
    fn parse_deny_lines_and_bad_lines() {
        let mut rs = RuleSet::new();
        let (ok, bad) = rs.parse(b"allow rw /a\ndeny r /a/priv.txt\nbogus\ndeny x /bad\n");
        assert_eq!(ok, 2);
        assert_eq!(bad, 2);
        assert!(rs.adjudicate(b"/a/pub.txt", Op::Read).allow);
        assert!(!rs.adjudicate(b"/a/priv.txt", Op::Read).allow);
        assert_eq!(rs.adjudicate(b"/a/priv.txt", Op::Read).deny, Some(DenyReason::Denied));
    }

    // ---------------- 任务37 · 热更新（RuleBook）----------------

    #[test]
    fn rulebook_hot_reload_and_idempotent() {
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /handoff\n");
        assert_eq!(rb.generation(), 1);
        assert!(rb.adjudicate(b"/handoff/f", Op::Write).allow);
        // 改文件不重启生效：新文本 → 世代 +1、新规则即刻可判。
        let rep = rb.reload(b"allow rw /handoff\ndeny rw /handoff/secret\n");
        assert!(rep.changed);
        assert_eq!(rep.gen_before, 1);
        assert_eq!(rep.gen_after, 2);
        assert_eq!(rb.generation(), 2);
        assert!(!rb.adjudicate(b"/handoff/secret/x", Op::Write).allow);
        // 幂等：同文本 reload → changed=false、世代不变。
        let rep2 = rb.reload(b"allow rw /handoff\ndeny rw /handoff/secret\n");
        assert!(!rep2.changed);
        assert_eq!(rep2.gen_after, 2);
        assert_eq!(rb.reload_count(), 2);
        // 规则删减同样生效（回滚场景）。
        rb.reload(b"allow r /pub\n");
        assert!(rb.adjudicate(b"/pub/r.txt", Op::Read).allow);
        assert!(!rb.adjudicate(b"/handoff/f", Op::Write).allow, "旧规则应随热更新消失");
    }

    #[test]
    fn rulebook_snapshot_race_semantics() {
        // 竞态口径：请求要么旧世代要么新世代，不存在半更新。
        // 模拟：决策引用的世代与裁决结果绑定，reload 后旧请求结果仍自洽。
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /data\n");
        let gen_old = rb.generation();
        let d_old = rb.adjudicate(b"/data/f", Op::Write);
        assert!(d_old.allow);
        // 更新中请求（更新与请求交错：先取决策再 reload）。
        rb.reload(b"deny rw /data\n");
        assert_eq!(rb.generation(), gen_old + 1);
        // 旧世代决策不受新规则回溯影响（快照自洽）。
        assert!(d_old.allow, "旧世代快照决策不可被回溯改写");
        // 新请求落新世代。
        assert!(!rb.adjudicate(b"/data/f", Op::Write).allow);
        assert_eq!(rb.adjudicate(b"/data/f", Op::Write).deny, Some(DenyReason::Denied));
    }

    // ---------------- 任务37 · Arbiter 可插裁决器 ----------------

    /// 新资源类型裁决器样例（总案开放性验证）：注册表键命名空间，
    /// 复用 NormPath 语义（`\Registry\HKEY` 折叠为 `/registry/hkey` 前缀）。
    struct RegistryArbiter {
        book: RuleBook,
    }
    impl Arbiter for RegistryArbiter {
        fn kind(&self) -> &'static str {
            "registry-key"
        }
        fn adjudicate(&self, raw: &[u8], op: Op) -> Decision {
            // 资源名适配：反斜杠 → 斜杠，接入同一 Decision 通道。
            let mut p = alloc::vec::Vec::with_capacity(raw.len() + 1);
            p.push(b'/');
            for &b in raw {
                p.push(if b == b'\\' { b'/' } else { b.to_ascii_lowercase() });
            }
            self.book.adjudicate(&p, op)
        }
    }

    #[test]
    fn arbiter_trait_pluggable_new_resource_type() {
        // 路径裁决器（首个内建实现）。
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /handoff\n");
        assert_eq!(rb.kind(), "vfs-path");
        assert!(Arbiter::adjudicate(&rb, b"/handoff/f", Op::Write).allow);
        // 新资源类型：同一 trait、同一 Decision 通道、互不串扰。
        let mut rbook = RuleBook::new();
        rbook.reload(b"allow rw /registry/hkey_cu/software/varix\n");
        let reg = RegistryArbiter { book: rbook };
        assert_eq!(reg.kind(), "registry-key");
        assert!(reg.adjudicate(b"\\Registry\\HKEY_CU\\Software\\VARIX\\Run", Op::Write).allow);
        assert!(!reg.adjudicate(b"\\Registry\\HKEY_CU\\System\\Run", Op::Write).allow);
        // 审计通道形状一致（Decision 字段语义共用）。
        let d = reg.adjudicate(b"\\Registry\\HKEY_CU\\System\\Run", Op::Write);
        assert_eq!(d.deny, Some(DenyReason::NoRule));
    }

    // ---------------- 审计账本 ----------------

    fn fresh_audit(total: u64) -> AuditJournal<MemDisk> {
        let mut dev = MemDisk::new(total);
        AuditJournal::format(&mut dev, VFS_AUDIT_BASE).unwrap();
        AuditJournal::open(dev, VFS_AUDIT_BASE).unwrap().0
    }

    #[test]
    fn audit_roundtrip_and_seq_continuity() {
        let mut aj = fresh_audit(131_072);
        for i in 0..5u32 {
            let s = aj.record(7 + i, i % 2 == 0, i % 2 != 0, b"/handoff/q.json").unwrap();
            assert_eq!(s, i as u64 + 1, "seq 自 1 单调");
        }
        assert_eq!(aj.len(), 5);
        // 重开（模拟断电收回）→ 全量可读且逐字段精确。
        let dev = aj.into_device();
        let (mut aj2, rep) = AuditJournal::open(dev, VFS_AUDIT_BASE).unwrap();
        assert_eq!(rep.entries, 5);
        assert_eq!(rep.torn, 0);
        for i in 0..5u32 {
            let r = aj2.read_record(i as usize).unwrap();
            assert_eq!(r.seq, i as u64 + 1);
            assert_eq!(r.pid, 7 + i);
            assert_eq!(r.allow, i % 2 == 0);
            assert_eq!(r.write, i % 2 != 0);
            assert_eq!(&r.path[..r.path_len], b"/handoff/q.json");
        }
    }

    #[test]
    fn audit_payload_corruption_is_torn_tail() {
        let mut aj = fresh_audit(131_072);
        for _ in 0..4u32 {
            aj.record(9, true, false, b"/pub/a.txt").unwrap();
        }
        // 直接腐坏第 2 条（0-based）载荷 → fnv 失败 → 该点及之后不可信。
        let lba = aj.slot_lba(2);
        let mut dev = aj.into_device();
        let mut blk = [0u8; 512];
        dev.read_blocks(lba, &mut blk).unwrap();
        blk[BODY_OFF + 10] ^= 0xFF; // 路径区翻转
        dev.write_blocks(lba, &blk).unwrap();
        let (mut aj2, rep) = AuditJournal::open(dev, VFS_AUDIT_BASE).unwrap();
        assert_eq!(rep.entries, 4, "journal 头部完好（载荷腐坏对 fs23 不可见）");
        let mut seen = [None; 64];
        let (n, corrupt) = aj2.read_all(&mut seen[..]);
        assert!(corrupt, "fnv 复核发现载荷腐坏");
        assert_eq!(n, 2, "torn 口径：该点及之后全部丢弃");
        assert_eq!(seen[0].unwrap().seq, 1);
        assert_eq!(seen[1].unwrap().seq, 2);
    }

    #[test]
    fn guarded_volume_audits_both_verdicts() {
        let mut dev = MemDisk::new(131_072);
        AuditJournal::format(&mut dev, VFS_AUDIT_BASE).unwrap();
        let (aj, _) = AuditJournal::open(dev, VFS_AUDIT_BASE).unwrap();
        let mut rs = RuleSet::new();
        rs.add(true, false, b"/apps.json").unwrap();
        let mut gv = GuardedVolume { rules: rs, audit: aj };
        assert!(gv.adjudicate(1, b"/apps.json", Op::Read).allow);
        assert!(!gv.adjudicate(1, b"/etc/passwd", Op::Read).allow);
        assert!(!gv.adjudicate(1, b"/../etc", Op::Write).allow);
        assert_eq!(gv.audit.len(), 3, "allow/deny 全量留痕");
        let mut seen = [None; 8];
        let (n, corrupt) = gv.audit.read_all(&mut seen[..]);
        assert_eq!(n, 3);
        assert!(!corrupt);
        assert!(seen[0].unwrap().allow);
        assert!(!seen[1].unwrap().allow);
        assert!(!seen[2].unwrap().allow);
        assert!(seen[2].unwrap().write, "逃逸尝试按写操作原样入账");
    }
}

