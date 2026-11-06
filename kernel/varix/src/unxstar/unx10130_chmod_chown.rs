//! UNX-F10130 chmod/chown 权限面正反双判据（位操作达成与越权误用受控）· 0x93080 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10131-J1 原子追加达成（多写端偏移独立终位正确 10/10 次）；
//!       UNX-F10131-J2 无 O_APPEND 竌态受控（偏移覆盖可复现并注 L1 条款
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93080（本卡独占 0x40 窗口，下一卡 0x930C0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_chmod_call`："op|path|arg" 三成分（op ∈ {chmod,
//!      chown}），mode 十进制 0..=511 闭集、uid 非负闭集，越闭集解析层
//!      不吞（判定层拒）；
//!   ② 权限账 `PermLedger`：路径→(mode 位 9 位, owner uid)，chmod 置位/
//!      清位回读一致，chown 换属主；
//!   ③ 状态机 `PermState`：Inherited→Modified→Frozen 单向（禁倒退）；
//!   ④ 注入核 `BadPerm`：非属主 chmod / 非属主 chown / mode 越九位 /
//!      缺失路径 四类坏样本逐类 10/10 档精确复现 EPERM/EINVAL/ENOENT 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10130 独占 0x40 窗口，下一卡 0x930C0）。
pub const SEG_BASE: u32 = 0x93080;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x930C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 权限账容量。
pub const PERM_CAP: usize = 16;

/// mode 九位闭集上限。
pub const MODE_MAX: u32 = 0o777;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// 非属主 chmod/chown（EPERM 族）
    NotOwner,
    /// mode 越九位（EINVAL 族）
    BadMode,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 位账失平
    BitDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl PmCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PmCode::NotOwner => "非属主被拒：chmod/chown 只许属主调用；EPERM 精确归档",
            PmCode::BadMode => "mode 越九位被拒：0..=0o777 闭集；EINVAL 精确归档",
            PmCode::MissingPath => "缺失路径被拒：权限账无此路径；ENOENT 精确归档",
            PmCode::EmptyPath => "空路径被拒：路径不得为空串；EINVAL 精确归档",
            PmCode::BadOp => "非法 op 被拒：op 闭集 {chmod, chown}；请核对调用行",
            PmCode::BadNumber => "非数字成分被拒：mode/uid 必须为十进制；请改写",
            PmCode::MissingPart => "成分缺失被拒：调用行必须三段 op|path|arg；请补成分",
            PmCode::StateRegress => "状态倒退被拒：Inherited→Modified→Frozen 单向",
            PmCode::EmptySummary => "空成文被拒：权限小结不得为空串；请先登记再成文",
            PmCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            PmCode::BitDrift => "位账失平被拒：置位/清位回读必须逐位一致",
            PmCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PmCode; 12] = [
        PmCode::NotOwner,
        PmCode::BadMode,
        PmCode::MissingPath,
        PmCode::EmptyPath,
        PmCode::BadOp,
        PmCode::BadNumber,
        PmCode::MissingPart,
        PmCode::StateRegress,
        PmCode::EmptySummary,
        PmCode::SegmentViolation,
        PmCode::BitDrift,
        PmCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的权限调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermCall {
    /// 动词
    pub op: String,
    /// 路径
    pub path: String,
    /// 参数（chmod 的 mode / chown 的 uid，允许越闭集入判定层）
    pub arg: i64,
}

/// 解析 "op|path|arg" 三段调用行。
pub fn parse_perm_call(line: &str) -> Result<PermCall, PmCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(PmCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(PmCode::MissingPart),
    };
    if op != "chmod" && op != "chown" {
        return Err(PmCode::BadOp);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(PmCode::MissingPart),
    };
    let as_ = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(PmCode::MissingPart),
    };
    if as_.is_empty() {
        return Err(PmCode::BadNumber);
    }
    let (neg, digits) = match as_.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, as_.as_str()),
    };
    if digits.is_empty() {
        return Err(PmCode::BadNumber);
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return Err(PmCode::BadNumber);
        }
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    Ok(PermCall { op, path, arg: if neg { -acc } else { acc } })
}

// ── 权限状态机 ────────────────────────────────────────────────────────
/// 权限生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermState {
    /// 继承态
    Inherited,
    /// 已修改
    Modified,
    /// 已冻结
    Frozen,
}

impl PermState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: PermState) -> bool {
        let rank = |s: PermState| match s {
            PermState::Inherited => 0u8,
            PermState::Modified => 1,
            PermState::Frozen => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            PermState::Inherited => "继承态",
            PermState::Modified => "已修改",
            PermState::Frozen => "已冻结",
        }
    }
}

// ── 权限账 ────────────────────────────────────────────────────────────
/// 权限条目。
#[derive(Clone, Debug)]
pub struct PermEnt {
    /// 路径
    pub path: String,
    /// 九位 mode
    pub mode: u32,
    /// 属主 uid
    pub owner: u32,
    /// 权限状态
    pub state: PermState,
}

/// 权限账：路径→(mode, owner)。
#[derive(Clone, Debug)]
pub struct PermLedger {
    ents: Vec<PermEnt>,
    cap: usize,
    /// 当前执行者 uid
    pub caller: u32,
    /// 累计 chmod 次数
    pub chmods: u32,
    /// 累计 chown 次数
    pub chowns: u32,
    /// 累计越权拒止次数
    pub denies: u32,
}

impl PermLedger {
    /// 空账构造（caller=0）。
    pub fn new() -> PermLedger {
        PermLedger { ents: Vec::new(), cap: PERM_CAP, caller: 0, chmods: 0, chowns: 0, denies: 0 }
    }

    /// 按路径寻址。
    fn idx_of(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记文件（默认 mode 0o644，owner 0）。
    pub fn create(&mut self, path: &str, owner: u32) -> Result<(), PmCode> {
        if path.is_empty() {
            return Err(PmCode::EmptyPath);
        }
        if self.idx_of(path).is_some() || self.ents.len() >= self.cap {
            return Err(PmCode::BitDrift);
        }
        let _ = self.ents.push(PermEnt {
            path: String::from(path),
            mode: 0o644,
            owner,
            state: PermState::Inherited,
        });
        Ok(())
    }

    /// chmod：属主闸→mode 九位闸→置位回读。
    pub fn chmod(&mut self, path: &str, mode: u32) -> Result<u32, PmCode> {
        if path.is_empty() {
            return Err(PmCode::EmptyPath);
        }
        if mode > MODE_MAX {
            return Err(PmCode::BadMode);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(PmCode::MissingPath),
        };
        let owner = match self.ents.get(idx) {
            Some(e) => e.owner,
            None => return Err(PmCode::MissingPath),
        };
        if owner != self.caller {
            self.denies = self.denies.saturating_add(1);
            return Err(PmCode::NotOwner);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.mode = mode;
            if e.state.can_advance(PermState::Modified) {
                e.state = PermState::Modified;
            }
        }
        self.chmods = self.chmods.saturating_add(1);
        Ok(mode)
    }

    /// chown：属主闸→换属主。
    pub fn chown(&mut self, path: &str, uid: u32) -> Result<u32, PmCode> {
        if path.is_empty() {
            return Err(PmCode::EmptyPath);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(PmCode::MissingPath),
        };
        let owner = match self.ents.get(idx) {
            Some(e) => e.owner,
            None => return Err(PmCode::MissingPath),
        };
        if owner != self.caller {
            self.denies = self.denies.saturating_add(1);
            return Err(PmCode::NotOwner);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.owner = uid;
        }
        self.chowns = self.chowns.saturating_add(1);
        Ok(uid)
    }

    /// mode 读取。
    pub fn mode_of(&self, path: &str) -> Option<u32> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.mode),
                None => None,
            },
            None => None,
        }
    }

    /// owner 读取。
    pub fn owner_of(&self, path: &str) -> Option<u32> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.owner),
                None => None,
            },
            None => None,
        }
    }

    /// 位账审计：全账 mode ≤ MODE_MAX。
    pub fn bits_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.mode > MODE_MAX {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── J1 达成核：mode 置位/清位回读一致 10/10 轮 ────────────────────────
/// 逐轮：置位回读→清位回读→位或/位与可复算→越权拒。
pub fn run_perm_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = PermLedger::new();
        let _ = l.create("f", 0);
        let mut all_ok = true;
        // 面1：置位 0o600 回读。
        all_ok = all_ok && l.chmod("f", 0o600) == Ok(0o600)
            && l.mode_of("f") == Some(0o600);
        probes += 1;
        // 面2：改置 0o755 回读。
        all_ok = all_ok && l.chmod("f", 0o755) == Ok(0o755)
            && l.mode_of("f") == Some(0o755);
        probes += 1;
        // 面3：清位语义：0o644 = 0o755 & !0o111。
        all_ok = all_ok && l.chmod("f", 0o755 & !0o111) == Ok(0o644)
            && l.mode_of("f") == Some(0o644);
        probes += 1;
        // 面4：chown 换属主回读。
        all_ok = all_ok && l.chown("f", 7) == Ok(7) && l.owner_of("f") == Some(7);
        probes += 1;
        // 面5：换属主后原属主越权拒（EPERM）。
        l.caller = 0;
        all_ok = all_ok && l.chmod("f", 0o600).err() == Some(PmCode::NotOwner);
        probes += 1;
        // 面6：位账全平（三次 chmod + 一次 chown）。
        all_ok = all_ok && l.bits_consistent() && l.chmods == 3 && l.chowns == 1;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏权限矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPerm {
    /// 非属主 chmod（EPERM 族）
    NotOwnerChmod,
    /// 非属主 chown（EPERM 族）
    NotOwnerChown,
    /// mode 越九位（EINVAL 族）
    BadMode,
    /// 缺失路径（ENOENT 族）
    MissingPath,
}

impl BadPerm {
    /// 闭集全量。
    pub const ALL: [BadPerm; 4] = [
        BadPerm::NotOwnerChmod,
        BadPerm::NotOwnerChown,
        BadPerm::BadMode,
        BadPerm::MissingPath,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadPerm::NotOwnerChmod => "非属主 chmod",
            BadPerm::NotOwnerChown => "非属主 chown",
            BadPerm::BadMode => "mode 越九位",
            BadPerm::MissingPath => "缺失路径 chmod",
        }
    }
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), PmCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PmCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏权限注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_perm_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPerm::ALL.len() {
        let case = match BadPerm::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = PermLedger::new();
            let _ = l.create("f", 0);
            l.caller = 9;
            let caught = match case {
                BadPerm::NotOwnerChmod => {
                    l.chmod("f", 0o600).err() == Some(PmCode::NotOwner)
                }
                BadPerm::NotOwnerChown => {
                    l.chown("f", 1).err() == Some(PmCode::NotOwner)
                }
                BadPerm::BadMode => {
                    l.caller = 0;
                    l.chmod("f", MODE_MAX + 1).err() == Some(PmCode::BadMode)
                }
                BadPerm::MissingPath => {
                    l.caller = 0;
                    l.chmod("ghost", 0o600).err() == Some(PmCode::MissingPath)
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

// ── 权限小结成文（纯函数） ────────────────────────────────────────────
/// 八进制 mode 成文：「path=<s> mode=<o> owner=<u>」。
pub fn compose_perm_summary(l: &PermLedger, path: &str) -> Result<String, PmCode> {
    let mode = match l.mode_of(path) {
        Some(m) => m,
        None => return Err(PmCode::EmptySummary),
    };
    let owner = match l.owner_of(path) {
        Some(o) => o,
        None => return Err(PmCode::EmptySummary),
    };
    let out = format!("path={} mode=0o{:03o} owner={}", path, mode, owner);
    if out.is_empty() {
        return Err(PmCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；非属主越权族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；mode 越九位族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径族"),
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：置位→清位→chown→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = PermLedger::new();
    let _ = l.create("f", 0);
    let _ = l.chmod("f", 0o755);
    let _ = l.chmod("f", 0o644);
    let _ = l.chown("f", 3);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_perm_summary(&l, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10130 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10130_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10130");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93080..=0x930BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < PmCode::ALL.len() {
        let mut j = i + 1;
        while j < PmCode::ALL.len() {
            if let (Some(a), Some(b)) = (PmCode::ALL.get(i), PmCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = PmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_perm_call("chmod|f|448").is_ok()
        && parse_perm_call("chown|f|-1").is_ok()
        && parse_perm_call("attr|f|1").err() == Some(PmCode::BadOp)
        && parse_perm_call("chmod|f|4x8").err() == Some(PmCode::BadNumber);
    s.add("调用行解析器：op 双词闭集放行且坏成分逐档拒绝", parse_ok,
          "chmod/chown 双动词闭集；arg 允许负值入判定层");

    // 4. J1 达成核
    let (ok, probes) = run_perm_rounds();
    s.add("J1 位操作达成：mode 置位/清位回读一致 LTP chmod01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（置位/改置/清位/chown/越权拒/位账）全对");

    // 5. J2 非属主越权
    let mut l = PermLedger::new();
    let _ = l.create("a", 0);
    l.caller = 5;
    let eperm_ok = l.chmod("a", 0o600).err() == Some(PmCode::NotOwner)
        && l.chown("a", 1).err() == Some(PmCode::NotOwner)
        && l.mode_of("a") == Some(0o644);
    s.add("J2 非属主越权受控：EPERM 双入口精确复现且账面不动", eperm_ok,
          "chmod/chown 双入口都必须过属主闸；被拒不改账");

    // 6. mode 越九位档
    l.caller = 0;
    let mode_ok = l.chmod("a", MODE_MAX + 1).err() == Some(PmCode::BadMode)
        && l.chmod("a", MODE_MAX).is_ok();
    s.add("mode 越九位：EINVAL 档精确复现且边界值放行", mode_ok,
          "0o777 放行、0o1000 拒；边界必须分明");

    // 7. 位账全平
    let bits_ok = l.bits_consistent();
    s.add("位账全平：全账 mode 落九位闭集", bits_ok,
          "任何越位 mode 入账即 BitDrift");

    // 8. 属主转移后双向闸
    let _ = l.chown("a", 9);
    l.caller = 0;
    let t_ok = l.chmod("a", 0o600).err() == Some(PmCode::NotOwner)
        && {
            l.caller = 9;
            l.chmod("a", 0o600).is_ok()
        };
    s.add("属主转移：新属主可写、旧属主被拒", t_ok,
          "chown 后权限闸随属主迁移");

    // 9. 注入核
    let (inj, per_class) = run_bad_perm_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "非属主双入口/越位/缺失必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadPerm::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = PermState::Inherited.can_advance(PermState::Modified)
        && PermState::Modified.can_advance(PermState::Frozen)
        && !PermState::Frozen.can_advance(PermState::Inherited)
        && PermState::Frozen.label() == "已冻结";
    s.add("状态机：Inherited→Modified→Frozen 单向", sm_ok,
          "权限生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(2)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_perm_summary(&l, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("mode=0o600"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结八进制可复算且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("位操作达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_perm_call("chmod|f|448").is_ok());
        assert_eq!(parse_perm_call("attr|f|1").err(), Some(PmCode::BadOp));
        assert_eq!(parse_perm_call("chmod|f|4x8").err(), Some(PmCode::BadNumber));
        let c = parse_perm_call("chown|f|-2").unwrap_or(PermCall { op: String::new(), path: String::new(), arg: 0 });
        assert_eq!(c.arg, -2);
    }

    #[test]
    fn chmod_roundtrip() {
        let mut l = PermLedger::new();
        let _ = l.create("f", 0);
        assert_eq!(l.chmod("f", 0o755), Ok(0o755));
        assert_eq!(l.mode_of("f"), Some(0o755));
        assert_eq!(l.chmod("f", 0o644), Ok(0o644));
        assert_eq!(l.mode_of("f"), Some(0o644));
    }

    #[test]
    fn not_owner_denied() {
        let mut l = PermLedger::new();
        let _ = l.create("f", 0);
        l.caller = 3;
        assert_eq!(l.chmod("f", 0o600).err(), Some(PmCode::NotOwner));
        assert_eq!(l.chown("f", 1).err(), Some(PmCode::NotOwner));
        assert_eq!(l.mode_of("f"), Some(0o644));
    }

    #[test]
    fn mode_boundary() {
        let mut l = PermLedger::new();
        let _ = l.create("f", 0);
        assert_eq!(l.chmod("f", MODE_MAX + 1).err(), Some(PmCode::BadMode));
        assert!(l.chmod("f", MODE_MAX).is_ok());
    }

    #[test]
    fn chown_transfers_gate() {
        let mut l = PermLedger::new();
        let _ = l.create("f", 0);
        let _ = l.chown("f", 9);
        assert_eq!(l.owner_of("f"), Some(9));
        l.caller = 0;
        assert_eq!(l.chmod("f", 0o600).err(), Some(PmCode::NotOwner));
        l.caller = 9;
        assert!(l.chmod("f", 0o600).is_ok());
    }

    #[test]
    fn missing_path_denied() {
        let mut l = PermLedger::new();
        assert_eq!(l.chmod("ghost", 0o600).err(), Some(PmCode::MissingPath));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_perm_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_perm_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(PermState::Inherited.can_advance(PermState::Frozen));
        assert!(!PermState::Frozen.can_advance(PermState::Modified));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10130_checks_green() {
        let set = run_unx10130_checks();
        assert!(set.all_passed(), "UNX-F10130 red: {}", set.red_items_text());
    }
}
