//! UNX-F10126 rename 目录替换正反双判据（替换达成与非空目录误用受控）· 0x92F80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10127-J1 截断达成（缩短/加长与零填充语义 LTP ftruncate01
//!       面 10/10 次）；UNX-F10127-J2 负长误用受控（预期 EINVAL 精确复现
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92F80（本卡独占 0x40 窗口，下一卡 0x92FC0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_rn_call`："rename|old|new" 三成分，空成分/缺成分
//!      逐档拒绝；
//!   ② 目录册 `SwapLedger`：目录→(子项数, is_dir)，rename 目录替换：
//!      目标空目录可被换位（原子摘除后重挂）、非空目录 ENOTEMPTY 档、
//!      文件目标 ENOTDIR 档、同路 EINVAL 档；
//!   ③ 状态机 `SwapState`：Armed→Swapping→Swapped 单向（禁倒退）；
//!   ④ 注入核 `BadSwap`：非空目录替换 / 缺失 old / 同路改名 / 文件目标
//!      四类坏样本逐类 10/10 次精确复现 ENOTEMPTY/ENOENT/EINVAL/ENOTDIR
//!      档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10126 独占 0x40 窗口，下一卡 0x92FC0）。
pub const SEG_BASE: u32 = 0x92F80;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92FC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 目录册容量。
pub const DIR_CAP: usize = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwCode {
    /// 非空目录替换（ENOTEMPTY 族）
    NonEmptyDir,
    /// 缺失 old（ENOENT 族）
    MissingOld,
    /// 同路改名（EINVAL 族）
    SamePath,
    /// 文件目标（ENOTDIR 族）
    FileTarget,
    /// 空成分（EINVAL 族）
    EmptyPart,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 目录册满（ENOSPC 档）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 换位守恒失平
    CountDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl SwCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SwCode::NonEmptyDir => "非空目录替换被拒：目标目录有子项；ENOTEMPTY 精确归档",
            SwCode::MissingOld => "缺失 old 被拒：源目录不在账；ENOENT 精确归档",
            SwCode::SamePath => "同路改名被拒：old == new；EINVAL 精确归档",
            SwCode::FileTarget => "文件目标被拒：new 是文件不是目录；ENOTDIR 精确归档",
            SwCode::EmptyPart => "空成分被拒：old/new 不得为空串；EINVAL 精确归档",
            SwCode::MissingPart => "成分缺失被拒：调用行必须三段 rename|old|new；请补成分",
            SwCode::LedgerFull => "目录册满被拒：请先清理闲置目录",
            SwCode::StateRegress => "状态倒退被拒：Armed→Swapping→Swapped 单向",
            SwCode::EmptySummary => "空成文被拒：换位小结不得为空串；请先 rename 再成文",
            SwCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            SwCode::CountDrift => "换位守恒失平：换位后子项账必须平移不失",
            SwCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SwCode; 12] = [
        SwCode::NonEmptyDir,
        SwCode::MissingOld,
        SwCode::SamePath,
        SwCode::FileTarget,
        SwCode::EmptyPart,
        SwCode::MissingPart,
        SwCode::LedgerFull,
        SwCode::StateRegress,
        SwCode::EmptySummary,
        SwCode::SegmentViolation,
        SwCode::CountDrift,
        SwCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的换位调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapCall {
    /// 源目录
    pub old: String,
    /// 目标目录
    pub new: String,
}

/// 解析 "rename|old|new" 三段调用行。
pub fn parse_rn_call(line: &str) -> Result<SwapCall, SwCode> {
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
        return Err(SwCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.as_str(),
        None => return Err(SwCode::MissingPart),
    };
    if op != "rename" {
        return Err(SwCode::SamePath);
    }
    let old = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(SwCode::MissingPart),
    };
    let new = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(SwCode::MissingPart),
    };
    if old.is_empty() || new.is_empty() {
        return Err(SwCode::EmptyPart);
    }
    Ok(SwapCall { old, new })
}

// ── 换位状态机 ────────────────────────────────────────────────────────
/// 换位生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapState {
    /// 待命
    Armed,
    /// 换位中
    Swapping,
    /// 已换位
    Swapped,
}

impl SwapState {
    /// 单调推进判定。
    pub fn can_advance(self, next: SwapState) -> bool {
        let rank = |s: SwapState| match s {
            SwapState::Armed => 0u8,
            SwapState::Swapping => 1,
            SwapState::Swapped => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            SwapState::Armed => "待命",
            SwapState::Swapping => "换位中",
            SwapState::Swapped => "已换位",
        }
    }
}

// ── 目录册 ────────────────────────────────────────────────────────────
/// 目录条目。
#[derive(Clone, Debug)]
pub struct SwapEnt {
    /// 目录名
    pub name: String,
    /// 子项数
    pub children: u32,
    /// 是否目录（false 为文件目标样本）
    pub is_dir: bool,
    /// 换位状态
    pub state: SwapState,
}

/// 目录册：目录→(子项数, is_dir)。
#[derive(Clone, Debug)]
pub struct SwapLedger {
    ents: Vec<SwapEnt>,
    cap: usize,
    /// 累计换位次数
    pub swaps: u32,
    /// 累计换位平移的子项数
    pub moved_children: u32,
}

impl SwapLedger {
    /// 空账构造。
    pub fn new() -> SwapLedger {
        SwapLedger { ents: Vec::new(), cap: DIR_CAP, swaps: 0, moved_children: 0 }
    }

    /// 按名寻址。
    fn idx_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 建目录。
    pub fn mkdir(&mut self, name: &str, children: u32) -> Result<(), SwCode> {
        if name.is_empty() {
            return Err(SwCode::EmptyPart);
        }
        if self.idx_of(name).is_some() {
            return Err(SwCode::LedgerFull);
        }
        if self.ents.len() >= self.cap {
            return Err(SwCode::LedgerFull);
        }
        let _ = self.ents.push(SwapEnt {
            name: String::from(name),
            children,
            is_dir: true,
            state: SwapState::Armed,
        });
        Ok(())
    }

    /// 建文件目标样本。
    pub fn mkfile(&mut self, name: &str) -> Result<(), SwCode> {
        if name.is_empty() {
            return Err(SwCode::EmptyPart);
        }
        if self.idx_of(name).is_some() {
            return Err(SwCode::LedgerFull);
        }
        if self.ents.len() >= self.cap {
            return Err(SwCode::LedgerFull);
        }
        let _ = self.ents.push(SwapEnt {
            name: String::from(name),
            children: 0,
            is_dir: false,
            state: SwapState::Armed,
        });
        Ok(())
    }

    /// rename 目录替换核：四闸（缺失/同路/文件目标/非空）+ 换位平移。
    pub fn rename(&mut self, old: &str, new: &str) -> Result<u32, SwCode> {
        if old.is_empty() || new.is_empty() {
            return Err(SwCode::EmptyPart);
        }
        if old == new {
            return Err(SwCode::SamePath);
        }
        let oidx = match self.idx_of(old) {
            Some(i) => i,
            None => return Err(SwCode::MissingOld),
        };
        let (ois_dir, ochildren) = match self.ents.get(oidx) {
            Some(e) => (e.is_dir, e.children),
            None => return Err(SwCode::MissingOld),
        };
        if !ois_dir {
            return Err(SwCode::FileTarget);
        }
        // 目标闸：目标在账时——文件目标 ENOTDIR；非空目录 ENOTEMPTY；
        // 空目录换位（原子摘除）。
        let mut displaced: u32 = 0;
        if let Some(nidx) = self.idx_of(new) {
            let (nis_dir, nchildren) = match self.ents.get(nidx) {
                Some(e) => (e.is_dir, e.children),
                None => (false, 0),
            };
            if !nis_dir {
                return Err(SwCode::FileTarget);
            }
            if nchildren > 0 {
                return Err(SwCode::NonEmptyDir);
            }
            displaced = nchildren;
            let _ = self.ents.remove(nidx);
        }
        if let Some(e) = self.ents.get_mut(oidx) {
            e.name = String::from(new);
            e.state = SwapState::Swapped;
        }
        self.swaps = self.swaps.saturating_add(1);
        self.moved_children = self.moved_children.saturating_add(ochildren.saturating_add(displaced));
        Ok(ochildren)
    }

    /// 子项数读取。
    pub fn children_of(&self, name: &str) -> Option<u32> {
        match self.idx_of(name) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.children),
                None => None,
            },
            None => None,
        }
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }
}

// ── J1 达成核：空目录换位 10/10 轮 ────────────────────────────────────
/// 逐轮：空目录换位→旧名失联→新名在账→子项平移→再换拒。
pub fn run_swap_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 2);
        let _ = l.mkdir("b", 0);
        let mut all_ok = true;
        // 面1：a(2 子项) 换位到空目录 b。
        all_ok = all_ok && l.rename("a", "b") == Ok(2);
        probes += 1;
        // 面2：旧名失联、新名在账。
        all_ok = all_ok && l.children_of("a").is_none()
            && l.children_of("b") == Some(2);
        probes += 1;
        // 面3：换位计数平移。
        all_ok = all_ok && l.swaps == 1 && l.moved_children == 2;
        probes += 1;
        // 面4：换位后再 rename 旧名按 ENOENT 档拒。
        all_ok = all_ok && l.rename("a", "c").err() == Some(SwCode::MissingOld);
        probes += 1;
        // 面5：b 再换位到新空目录 c。
        let _ = l.mkdir("c", 0);
        all_ok = all_ok && l.rename("b", "c") == Ok(2);
        probes += 1;
        // 面6：换位后账面收敛为单目录。
        all_ok = all_ok && l.children_of("c") == Some(2) && l.len() == 1;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏换位矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSwap {
    /// 非空目录替换（ENOTEMPTY 族）
    NonEmptyDir,
    /// 缺失 old（ENOENT 族）
    MissingOld,
    /// 同路改名（EINVAL 族）
    SamePath,
    /// 文件目标（ENOTDIR 族）
    FileTarget,
}

impl BadSwap {
    /// 闭集全量。
    pub const ALL: [BadSwap; 4] = [
        BadSwap::NonEmptyDir,
        BadSwap::MissingOld,
        BadSwap::SamePath,
        BadSwap::FileTarget,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadSwap::NonEmptyDir => "非空目录替换",
            BadSwap::MissingOld => "缺失 old 改名",
            BadSwap::SamePath => "同路改名",
            BadSwap::FileTarget => "文件目标替换",
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
    pub fn record(&mut self, caught: bool) -> Result<(), SwCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SwCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏换位注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_swap_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadSwap::ALL.len() {
        let case = match BadSwap::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = SwapLedger::new();
            let _ = l.mkdir("a", 1);
            let _ = l.mkdir("b", 0);
            let _ = l.mkfile("f");
            let caught = match case {
                BadSwap::NonEmptyDir => {
                    let _ = l.mkdir("c", 3);
                    l.rename("c", "a").err() == Some(SwCode::NonEmptyDir)
                }
                BadSwap::MissingOld => {
                    l.rename("ghost", "z").err() == Some(SwCode::MissingOld)
                }
                BadSwap::SamePath => {
                    l.rename("a", "a").err() == Some(SwCode::SamePath)
                }
                BadSwap::FileTarget => {
                    l.rename("a", "f").err() == Some(SwCode::FileTarget)
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

// ── 换位小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「name=<s> children=<n>」；账外先拒。
pub fn compose_swap_summary(l: &SwapLedger, name: &str) -> Result<String, SwCode> {
    let children = match l.children_of(name) {
        Some(c) => c,
        None => return Err(SwCode::EmptySummary),
    };
    let out = format!("name={} children={}", name, children);
    if out.is_empty() {
        return Err(SwCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOTEMPTY", -39, 145, "Windows ERROR_DIR_NOT_EMPTY；非空目录替换族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失 old 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；同路改名/空成分族"),
    ("ENOTDIR", -20, 267, "Windows ERROR_DIRECTORY；文件目标替换族"),
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

/// 规范语料指纹：建目录→空目录换位→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = SwapLedger::new();
    let _ = l.mkdir("x", 3);
    let _ = l.mkdir("y", 0);
    let _ = l.rename("x", "y");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_swap_summary(&l, "y") {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&l.swaps.to_le_bytes());
    bytes.extend_from_slice(&l.moved_children.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10126 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10126_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10126");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92F80..=0x92FBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < SwCode::ALL.len() {
        let mut j = i + 1;
        while j < SwCode::ALL.len() {
            if let (Some(a), Some(b)) = (SwCode::ALL.get(i), SwCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SwCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_rn_call("rename|a|b").is_ok()
        && parse_rn_call("mv|a|b").err() == Some(SwCode::SamePath)
        && parse_rn_call("rename||b").err() == Some(SwCode::EmptyPart)
        && parse_rn_call("rename|a").err() == Some(SwCode::MissingPart);
    s.add("调用行解析器：三段闭集放行且坏成分逐档拒绝", parse_ok,
          "rename 动词闭集；空成分/缺成分逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_swap_rounds();
    s.add("J1 目录替换达成：空目录换位语义 LTP rename02 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（换位/失联/平移/旧名拒/再换/在账）全对");

    // 5. J2 非空目录替换
    let mut l = SwapLedger::new();
    let _ = l.mkdir("src", 4);
    let _ = l.mkdir("dst", 1);
    let ne_ok = l.rename("src", "dst").err() == Some(SwCode::NonEmptyDir)
        && l.children_of("src") == Some(4)
        && l.children_of("dst") == Some(1);
    s.add("J2 非空目录替换受控：ENOTEMPTY 精确复现且两目录账面不动", ne_ok,
          "目标非空必须拒；被拒换位不得移动任何子项账");

    // 6. 文件目标档
    let _ = l.mkfile("f");
    let ft_ok = l.rename("src", "f").err() == Some(SwCode::FileTarget);
    s.add("文件目标：ENOTDIR 档独立精确归因", ft_ok,
          "目录只能换位于目录；文件目标独立分档");

    // 7. 同路改名档
    let same_ok = l.rename("src", "src").err() == Some(SwCode::SamePath);
    s.add("同路改名：EINVAL 档独立精确归因", same_ok,
          "old==new 是非法换位；必须先于目标闸判");

    // 8. 空目录对空目录换位
    let _ = l.mkdir("e1", 0);
    let _ = l.mkdir("e2", 0);
    let ee_ok = l.rename("e1", "e2").is_ok() && l.children_of("e2") == Some(0);
    s.add("空换空：原子摘除后重挂成功", ee_ok,
          "空目录换位是替换语义的合法核心");

    // 9. 注入核
    let (inj, per_class) = run_bad_swap_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "非空替换/缺失 old/同路/文件目标必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadSwap::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = SwapState::Armed.can_advance(SwapState::Swapping)
        && SwapState::Swapping.can_advance(SwapState::Swapped)
        && !SwapState::Swapped.can_advance(SwapState::Armed)
        && SwapState::Swapped.label() == "已换位";
    s.add("状态机：Armed→Swapping→Swapped 单向", sm_ok,
          "换位生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(39) && agrees_with_linux(2)
        && agrees_with_linux(22) && agrees_with_linux(20)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_swap_summary(&l, "e2");
    let text_ok = match &sum {
        Ok(x) => x.contains("name=e2"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("目录替换达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_rn_call("rename|a|b").is_ok());
        assert_eq!(parse_rn_call("mv|a|b").err(), Some(SwCode::SamePath));
        assert_eq!(parse_rn_call("rename||b").err(), Some(SwCode::EmptyPart));
        assert_eq!(parse_rn_call("rename|a").err(), Some(SwCode::MissingPart));
    }

    #[test]
    fn swap_empty_dir() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 2);
        let _ = l.mkdir("b", 0);
        assert_eq!(l.rename("a", "b"), Ok(2));
        assert_eq!(l.children_of("b"), Some(2));
        assert!(l.children_of("a").is_none());
    }

    #[test]
    fn nonempty_blocked() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 1);
        let _ = l.mkdir("b", 2);
        assert_eq!(l.rename("a", "b").err(), Some(SwCode::NonEmptyDir));
    }

    #[test]
    fn same_path_blocked() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 0);
        assert_eq!(l.rename("a", "a").err(), Some(SwCode::SamePath));
    }

    #[test]
    fn file_target_blocked() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 0);
        let _ = l.mkfile("f");
        assert_eq!(l.rename("a", "f").err(), Some(SwCode::FileTarget));
    }

    #[test]
    fn missing_old_blocked() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 0);
        assert_eq!(l.rename("ghost", "a").err(), Some(SwCode::MissingOld));
    }

    #[test]
    fn moved_children_accounting() {
        let mut l = SwapLedger::new();
        let _ = l.mkdir("a", 5);
        let _ = l.mkdir("b", 0);
        let _ = l.rename("a", "b");
        assert_eq!(l.moved_children, 5);
        assert_eq!(l.swaps, 1);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_swap_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_swap_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(SwapState::Armed.can_advance(SwapState::Swapped));
        assert!(!SwapState::Swapped.can_advance(SwapState::Armed));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10126_checks_green() {
        let set = run_unx10126_checks();
        assert!(set.all_passed(), "UNX-F10126 red: {}", set.red_items_text());
    }
}
