# -*- coding: utf-8 -*-
"""任务37 vfsguard.rs 补丁：deny 语义 + 拒绝优先 + RuleBook 热更新 + Arbiter trait"""
import io, sys

P = 'kernel/varix/src/vfsguard.rs'
s = io.open(P, encoding='utf-8').read()

def rep(old, new, tag):
    global s
    assert old in s, 'ANCHOR MISS: ' + tag
    assert s.count(old) == 1, 'ANCHOR DUP: ' + tag
    s = s.replace(old, new, 1)

# ---------- 1) Rule 结构体加 deny 字段 ----------
rep(
"""/// 一条白名单规则：目录级前缀 + 操作位。组件边界匹配（`/app` 不匹配 `/apps`）。
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    prefix: NormPath,
    read: bool,
    write: bool,
}""",
"""/// 一条白名单规则：目录级前缀 + 操作位。组件边界匹配（`/app` 不匹配 `/apps`）。
/// `deny=true` = 显式拒绝规则（任务37 冲突仲裁：deny 与 allow 叠加时**拒绝优先**）。
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    prefix: NormPath,
    read: bool,
    write: bool,
    deny: bool,
}""",
    'Rule struct')

# ---------- 2) DenyReason 加 Denied ----------
rep(
"""pub enum DenyReason {
    /// 无规则命中——白名单外，默认拒绝的常态。
    NoRule,
    /// 命中规则但操作位未授予。
    OpMask,
    /// 路径规范化失败（含逃逸）。
    BadPath(GuardError),
}""",
"""pub enum DenyReason {
    /// 无规则命中——白名单外，默认拒绝的常态。
    NoRule,
    /// 命中规则但操作位未授予。
    OpMask,
    /// 命中显式 deny 规则（任务37 冲突仲裁：叠加时拒绝优先的可观测出口）。
    Denied,
    /// 路径规范化失败（含逃逸）。
    BadPath(GuardError),
}""",
    'DenyReason')

# ---------- 3) RuleSet::new 默认值 ----------
rep(
"""        RuleSet {
            rules: [Rule {
                prefix: NormPath { b: [0; PATH_MAX], len: 0, depth: 0 },
                read: false,
                write: false,
            }; RULES_MAX],
            len: 0,
        }""",
"""        RuleSet {
            rules: [Rule {
                prefix: NormPath { b: [0; PATH_MAX], len: 0, depth: 0 },
                read: false,
                write: false,
                deny: false,
            }; RULES_MAX],
            len: 0,
        }""",
    'RuleSet::new')

# ---------- 4) add() 保持 allow 语义 + 新增 add_deny ----------
rep(
"""        self.rules[self.len] = Rule { prefix, read, write };
        self.len += 1;
        Ok(self.len - 1)
    }""",
"""        self.rules[self.len] = Rule { prefix, read, write, deny: false };
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
    }""",
    'add/add_deny')

# ---------- 5) parse 支持 deny 行 ----------
rep(
"""    /// 解析规则文本（SHARED/whitelist/ 的内核侧加载面）：
    /// 行格式 `allow r|w|rw <绝对路径>`；空行与 `#` 注释跳过；坏行计数不致命。
    /// 返回 (成功条数, 坏行数)。""",
"""    /// 解析规则文本（SHARED/whitelist/ 的内核侧加载面）：
    /// 行格式 `allow r|w|rw <绝对路径>` 或 `deny r|w|rw <绝对路径>`（任务37）；
    /// 空行与 `#` 注释跳过；坏行计数不致命。
    /// 返回 (成功条数, 坏行数)。""",
    'parse doc')

rep(
"""            if cnt != 3 || parts[0] != b"allow" {
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
            match self.add(read, write, parts[2]) {
                Ok(_) => ok += 1,
                Err(_) => bad += 1,
            }""",
"""            let is_allow = parts[0] == b"allow";
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
            }""",
    'parse deny')

# ---------- 6) decide 改写：拒绝优先 ----------
rep(
"""    /// 裁决：任一命中规则授予操作位 → 允许；否则默认拒绝。
    pub fn decide(&self, p: &NormPath, op: Op) -> Decision {
        for i in 0..self.len {
            let r = &self.rules[i];
            if !Self::matches(r, p) {
                continue;
            }
            let granted = match op {
                Op::Read => r.read,
                Op::Write => r.write,
            };
            if granted {
                return Decision { allow: true, rule: Some(i), deny: None };
            }
            return Decision { allow: false, rule: Some(i), deny: Some(DenyReason::OpMask) };
        }
        Decision { allow: false, rule: None, deny: Some(DenyReason::NoRule) }
    }""",
"""    /// 裁决（任务37 冲突仲裁语义，两遍扫描显式定优先级）：
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
    }""",
    'decide')

# ---------- 7) RuleBook + Arbiter（插在 trim 之前） ----------
rep(
"""fn trim(b: &[u8]) -> &[u8] {""",
"""// ---------------------------------------------------------------------------
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
        let mut set = RuleSet::new();
        let (ok, bad) = set.parse(text);
        self.set = set;
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

fn trim(b: &[u8]) -> &[u8] {""",
    'RuleBook block')

io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
print('patch OK,', len(s.splitlines()), 'lines')
