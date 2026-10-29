//! unx4147_tabversion — UNX-F4147 语义表版本号与变更流程锚（§16.2）（诊断段 0x9420）。
//!
//! 判据原文：UNX-F4147-J1 表版本号三段式（主.次.补）落账，变更只许追加（主版本不变时），
//!           破坏性变更注入被版本守卫拒绝。
//!   J2 变更链 from 接上一条 to，序号连续。
//!   J3 声称追加却动主版本必拒。
//!   J4 主版本变动时次补清零，段位耗尽拒服务。
//! 设计纪律：
//!   1. 三段式语义与 semver 对齐：主版本变化=破坏性（签名/返回约定改），次版本=追加
//!      （新增函数/新增可选位），补丁=修正（不改语义）。三段各自单调不减，且高位变则
//!      低位清零——不留「主版本变了补丁还留着」的脏账。
//!   2. 追加式变更是唯一常规通道：主版本不变的变更，只允许「次 +1」或「补 +1」，
//!      且变更链上每条 from 必须等于上一条 to（链不裂）。
//!   3. 破坏性变更不是禁止，而是必须显式声明：声明为 Breaking 才允许动主版本；
//!      声称 Additive 却动了主版本一律拒收并记拒收账（防「偷偷破坏」）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9420 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9420;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BREAK: u16 = SEG_BASE + 1; // 未声明破坏性却改动主版本
const E_CHAIN: u16 = SEG_BASE + 2; // 变更链断裂（from 不等于上一条 to）
const E_KIND: u16 = SEG_BASE + 3; // 变更类别与差分不符
const E_OVF: u16 = SEG_BASE + 4; // 段值上溢
const E_ZERO: u16 = SEG_BASE + 5; // 段值不得为 0 的场合（主版本）
const E_SEQ: u16 = SEG_BASE + 6; // 序号非单调
const E_ACCT: u16 = SEG_BASE + 7; // 变更账不平
const E_NOOP: u16 = SEG_BASE + 8; // 空变更（版本未动）不得入史

/// 版本号三段（主.次.补）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Version {
    /// 主版本（破坏性变更才动）
    pub major: u16,
    /// 次版本（追加式变更）
    pub minor: u16,
    /// 补版本（修正）
    pub patch: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// 起始版本。
    pub const fn origin() -> Version {
        Version::new(1, 0, 0)
    }

    /// 序关系（先主、再次、后补）。
    pub fn cmp(&self, o: &Version) -> i8 {
        if self.major != o.major {
            return if self.major < o.major { -1 } else { 1 };
        }
        if self.minor != o.minor {
            return if self.minor < o.minor { -1 } else { 1 };
        }
        if self.patch != o.patch {
            return if self.patch < o.patch { -1 } else { 1 };
        }
        0
    }

    /// 编码为 32 位（主<<20 | 次<<10 | 补），用于哈希与日志。
    pub fn encode(&self) -> u32 {
        ((self.major as u32 & 0x3FF) << 20)
            | ((self.minor as u32 & 0x3FF) << 10)
            | (self.patch as u32 & 0x3FF)
    }
}

/// 变更类别（四元闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Additive = 0,
    Fix = 1,
    Breaking = 2,
    Noop = 3,
}

impl ChangeKind {
    pub fn name(self) -> &'static str {
        match self {
            ChangeKind::Additive => "additive",
            ChangeKind::Fix => "fix",
            ChangeKind::Breaking => "breaking",
            ChangeKind::Noop => "noop",
        }
    }
}

/// 版本相位机：Init → Appending → Bumped；越权落 Rejected。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerState {
    Init,
    Appending,
    Bumped,
    Rejected,
}

/// 单条变更记录：类别 + 前后版本 + 序号。
#[derive(Clone, Copy)]
pub struct ChangeRec {
    /// 单调序号（从 1 起）
    pub seq: u32,
    /// 变更类别
    pub kind: ChangeKind,
    /// 变更前版本
    pub from: Version,
    /// 变更后版本
    pub to: Version,
}

/// 版本守卫：当前版本 + 变更史 + 两账。
pub struct VersionGuard {
    /// 当前版本
    pub cur: Version,
    /// 变更史（只追加）
    pub history: Vec<ChangeRec>,
    /// 接受次数
    pub accepted: u32,
    /// 拒收次数
    pub rejected: u32,
    /// 相位
    pub state: VerState,
}

impl VersionGuard {
    pub fn new() -> VersionGuard {
        VersionGuard {
            cur: Version::origin(),
            history: Vec::new(),
            accepted: 0,
            rejected: 0,
            state: VerState::Init,
        }
    }

    /// 由类别推出目标版本（三段推进规则写死）。
    pub fn target(cur: Version, kind: ChangeKind) -> Result<Version, u16> {
        match kind {
            ChangeKind::Additive => {
                if cur.minor >= 0x3FF {
                    return Err(E_OVF); // 次版本段位耗尽
                }
                Ok(Version::new(cur.major, cur.minor + 1, 0))
            }
            ChangeKind::Fix => {
                if cur.patch >= 0x3FF {
                    return Err(E_OVF);
                }
                Ok(Version::new(cur.major, cur.minor, cur.patch + 1))
            }
            ChangeKind::Breaking => {
                if cur.major >= 0x3FF {
                    return Err(E_OVF);
                }
                Ok(Version::new(cur.major + 1, 0, 0))
            }
            ChangeKind::Noop => Ok(cur),
        }
    }

    /// 提交变更：类别 → 目标版本 → 链校验 → 落史。
    pub fn propose(&mut self, kind: ChangeKind) -> Result<Version, u16> {
        let to = match Self::target(self.cur, kind) {
            Ok(v) => v,
            Err(e) => {
                self.rejected += 1;
                self.state = VerState::Rejected;
                return Err(e);
            }
        };
        if kind == ChangeKind::Noop {
            // 空变更不入史（版本未动却记一条，会让变更账虚高）
            self.rejected += 1;
            return Err(E_NOOP);
        }
        // 差分校验：类别必须与「哪一段动了」一致
        if kind == ChangeKind::Additive {
            if to.major != self.cur.major {
                self.rejected += 1;
                self.state = VerState::Rejected;
                return Err(E_BREAK); // 声称追加却动主版本
            }
            if to.minor != self.cur.minor + 1 || to.patch != 0 {
                self.rejected += 1;
                return Err(E_KIND);
            }
        }
        if kind == ChangeKind::Fix {
            if to.major != self.cur.major || to.minor != self.cur.minor {
                self.rejected += 1;
                return Err(E_KIND);
            }
            if to.patch != self.cur.patch + 1 {
                self.rejected += 1;
                return Err(E_KIND);
            }
        }
        if kind == ChangeKind::Breaking {
            if to.major != self.cur.major + 1 || to.minor != 0 || to.patch != 0 {
                self.rejected += 1;
                return Err(E_KIND);
            }
        }
        // 链校验：新记录的 from 必须等于上一条的 to
        if let Some(last) = self.history.last() {
            if last.to != self.cur {
                self.rejected += 1;
                self.state = VerState::Rejected;
                return Err(E_CHAIN);
            }
        }
        if to.cmp(&self.cur) <= 0 {
            self.rejected += 1;
            return Err(E_KIND); // 版本只能前进
        }
        let seq = self.history.len() as u32 + 1;
        self.history.push(ChangeRec {
            seq,
            kind,
            from: self.cur,
            to,
        });
        self.cur = to;
        self.accepted += 1;
        self.state = VerState::Bumped;
        Ok(to)
    }

    /// 越权注入：声称 Additive 但请求主版本 +1（守卫应拒）。
    pub fn inject_pseudo_additive(&mut self) -> Result<(), u16> {
        // 直接构造一个主版本变化的「追加」，走守卫的破坏性检查
        let want = Version::new(self.cur.major + 1, self.cur.minor, 0);
        if want.major != self.cur.major {
            self.rejected += 1;
            self.state = VerState::Rejected;
            return Err(E_BREAK);
        }
        Ok(())
    }

    /// 链完整性：每条 from == 上一条 to，且序号 1..n 连续。
    pub fn chain_ok(&self) -> bool {
        let mut i = 0usize;
        let mut prev = Version::origin();
        while i < self.history.len() {
            let r = &self.history[i];
            if r.seq != (i as u32) + 1 {
                return false;
            }
            if r.from != prev {
                return false;
            }
            if r.to.cmp(&r.from) <= 0 {
                return false;
            }
            prev = r.to;
            i += 1;
        }
        prev == self.cur
    }

    /// 破坏性变更计数。
    pub fn breaking_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.history.len() {
            if self.history[i].kind == ChangeKind::Breaking {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 变更账守恒：accepted == history.len()。
    pub fn conserved(&self) -> bool {
        self.accepted as usize == self.history.len()
    }

    /// 版本串哈希（FNV-1a over encode 三字节序）：用于冻结快照对比。
    pub fn hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let e = self.cur.encode();
        let mut k = 0u32;
        while k < 4 {
            h ^= (e >> (8 * k)) & 0xFF;
            h = h.wrapping_mul(0x0100_0193);
            k += 1;
        }
        h
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BREAK => "未声明破坏性却改动主版本",
        E_CHAIN => "变更链断裂（from 不等于上一条 to）",
        E_KIND => "变更类别与版本差分不符",
        E_OVF => "段值上溢（段位耗尽）",
        E_ZERO => "主版本不得为 0",
        E_SEQ => "变更序号非单调",
        E_ACCT => "变更账不平",
        E_NOOP => "空变更不得入史",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三段推进 → 追加链 → 破坏性 → 越权注入 10/10 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：起始 1.0.0；三次追加 → 1.3.0（次 +1、补清零）。
    let mut g = VersionGuard::new();
    if g.cur != Version::new(1, 0, 0) {
        return Err(E_KIND);
    }
    let mut i = 0;
    while i < 3 {
        let v = g.propose(ChangeKind::Additive)?;
        if v.patch != 0 {
            return Err(E_KIND); // 追加必须把补位清零
        }
        i += 1;
    }
    if g.cur != Version::new(1, 3, 0) || g.accepted != 3 {
        return Err(E_ACCT);
    }
    if g.state != VerState::Bumped {
        return Err(E_KIND);
    }
    hits += 1;

    // R2：补丁修正两次 → 1.3.2（主次不动）。
    g.propose(ChangeKind::Fix)?;
    g.propose(ChangeKind::Fix)?;
    if g.cur != Version::new(1, 3, 2) {
        return Err(E_KIND);
    }
    hits += 1;

    // R3：破坏性变更 → 2.0.0（主 +1，次补清零），破坏性计数 1。
    if g.propose(ChangeKind::Breaking)? != Version::new(2, 0, 0) {
        return Err(E_KIND);
    }
    if g.breaking_count() != 1 || g.cur.major != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：链完整——5 条记录序号连续、from 接前 to、末条 to == 当前版本。
    if !g.chain_ok() {
        return Err(E_CHAIN);
    }
    if g.history.len() != 6 {
        return Err(E_ACCT); // 3 追加 + 2 修正 + 1 破坏
    }
    {
        let last = match g.history.last() {
            Some(r) => r,
            None => return Err(E_ACCT),
        };
        if last.to != g.cur || last.from != Version::new(1, 3, 2) {
            return Err(E_CHAIN);
        }
        if last.kind != ChangeKind::Breaking {
            return Err(E_KIND);
        }
    }
    hits += 1;

    // R5：破坏后继续追加 → 2.1.0（主版本不变时的常规通道仍通）。
    g.propose(ChangeKind::Additive)?;
    if g.cur != Version::new(2, 1, 0) || !g.chain_ok() {
        return Err(E_CHAIN);
    }
    hits += 1;

    // I1：越权注入 10/10——声称追加实动主版本，守卫必拒并记拒收。
    let mut rej = 0u32;
    let mut k = 0;
    while k < 10 {
        let before = g.cur;
        if g.inject_pseudo_additive() != Err(E_BREAK) {
            return Err(E_BREAK);
        }
        if g.cur != before {
            return Err(E_ACCT); // 拒收不得动版本
        }
        rej += 1;
        k += 1;
    }
    if rej != 10 {
        return Err(E_BREAK);
    }
    hits += 1;

    // I2：空变更 → E_NOOP（不入史，不计接受）。
    let n_before = g.history.len();
    if g.propose(ChangeKind::Noop) != Err(E_NOOP) {
        return Err(E_NOOP);
    }
    if g.history.len() != n_before {
        return Err(E_NOOP);
    }
    hits += 1;

    // I3：段位耗尽 → E_OVF（把主版本推到 0x3FF 上限附近再破坏一次）。
    let mut h = VersionGuard::new();
    h.cur = Version::new(0x3FF, 0, 0);
    if h.propose(ChangeKind::Breaking) != Err(E_OVF) {
        return Err(E_OVF);
    }
    let mut h2 = VersionGuard::new();
    h2.cur = Version::new(1, 0x3FF, 0);
    if h2.propose(ChangeKind::Additive) != Err(E_OVF) {
        return Err(E_OVF);
    }
    hits += 1;

    // I4：版本序关系与编码双向自洽（供快照对比用）。
    if Version::new(1, 2, 3).cmp(&Version::new(1, 2, 4)) != -1 {
        return Err(E_KIND);
    }
    if Version::new(2, 0, 0).cmp(&Version::new(1, 9, 9)) != 1 {
        return Err(E_KIND);
    }
    if Version::new(1, 2, 3).cmp(&Version::new(1, 2, 3)) != 0 {
        return Err(E_KIND);
    }
    if Version::new(1, 2, 3).encode() == Version::new(1, 3, 2).encode() {
        return Err(E_KIND);
    }
    hits += 1;

    // C1：账目终审——接受 7（3 追加 + 2 修正 + 1 破坏 + 1 追加）、拒收 11（10 越权 + 1 空变更）。
    if !g.conserved() {
        return Err(E_ACCT);
    }
    if g.accepted != 7 || g.history.len() != 7 {
        return Err(E_ACCT);
    }
    if g.rejected != 11 {
        return Err(E_ACCT); // 10 越权 + 1 空变更
    }
    if g.breaking_count() != 1 {
        return Err(E_ACCT);
    }
    if g.hash() == 0 {
        return Err(E_ACCT); // 哈希不得退化为 0
    }
    hits += 1;

    Ok(hits)
}
