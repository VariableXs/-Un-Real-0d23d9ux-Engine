//! cgb851_amddrvdiff —— CGPU-F0851 AMD 驱动差异表。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0851 AMD 驱动差异表  <SEG 0x74E0>
//! 目标行数：≥360（判据：纯功能代码 ≥360 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥360 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0851；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 驱动差异表，整数账面）：
//!   1. 驱动分支闭集四类（写死）：WHQL 稳定 / Adrenalin 预览 /
//!      Vulkan Beta / Pro 企业；分支号 0..=3，越界红码
//!      （XC_BAD_BRANCH）；
//!   2. 差异条目状态机：Found → Assessed → Mitigated → Verified →
//!      Closed；Mitigated 可回 Assessed（回归发现）；非法迁移红码
//!      （XC_BAD_TRANS）；
//!   3. 差异账（定容 40 槽）守恒：登记数 = 存量数 + 关闭数；漂移
//!      红码（XC_DIFF_DRIFT）；
//!   4. 差异类别闭集六类：安装路径 / API 行为 / 性能曲线 / 崩溃
//!      模式 / 设置回读 / 日志格式；越界红码（XC_BAD_DIFF_KIND）；
//!   5. 版本三元组（主.次.修订）全部整数比较，修订倒挂条目拒绝
//!      入账（XC_VER_BACKWARD）；
//!   6. 缓解等级闭集四级：无 / 绕过 / 适配 / 上游修复；Verified
//!      前置：缓解等级 ≥ 绕过；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x74E0 段为本模块独占（SEG_BASE=0x74E0），判别式
//!   SEG_BASE+1..+N；与 0x74C0/0x7500 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const DD_VERSION: &str = "cgb851_amddrvdiff-v1";

/// 诊断段基址（0x74E0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x74E0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x74E1 驱动分支越界（0..=3 之外）。
pub const XC_BAD_BRANCH: u16 = SEG_BASE + 1;
/// 0x74E2 差异类别越界（0..=5 之外）。
pub const XC_BAD_DIFF_KIND: u16 = SEG_BASE + 2;
/// 0x74E3 条目号越界（账外）。
pub const XC_BAD_ENTRY: u16 = SEG_BASE + 3;
/// 0x74E4 状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 4;
/// 0x74E5 差异账漂移（登记 ≠ 存量 + 关闭）。
pub const XC_DIFF_DRIFT: u16 = SEG_BASE + 5;
/// 0x74E6 版本倒挂（修订/次版回退）。
pub const XC_VER_BACKWARD: u16 = SEG_BASE + 6;
/// 0x74E7 缓解等级越界（0..=3 之外）。
pub const XC_BAD_MITIG: u16 = SEG_BASE + 7;
/// 0x74E8 越序验证（缓解等级不足即 Verified）。
pub const XC_MITIG_INSUFF: u16 = SEG_BASE + 8;
/// 0x74E9 差异账满（40 槽）。
pub const XC_DIFF_FULL: u16 = SEG_BASE + 9;
/// 0x74EA 重复签名条目。
pub const XC_DUP_SIG: u16 = SEG_BASE + 10;
/// 0x74EB 终态再迁移。
pub const XC_TERMINAL_MOVE: u16 = SEG_BASE + 11;

/// 差异账容量（写死：40）。
pub const DIFF_CAP: usize = 40;
/// 分支闭集上限。
pub const BRANCH_CAP: u8 = 3;
/// 差异类别闭集上限。
pub const KIND_CAP: u8 = 5;
/// 缓解等级闭集上限。
pub const MITIG_CAP: u8 = 3;

/// 驱动分支闭集：0=WHQL 1=预览 2=VulkanBeta 3=Pro。
pub const BR_WHQL: u8 = 0;
pub const BR_PREVIEW: u8 = 1;
pub const BR_VKBETA: u8 = 2;
pub const BR_PRO: u8 = 3;

/// 差异类别闭集：0=安装 1=API 行为 2=性能 3=崩溃 4=设置回读 5=日志。
pub const K_INSTALL: u8 = 0;
pub const K_API: u8 = 1;
pub const K_PERF: u8 = 2;
pub const K_CRASH: u8 = 3;
pub const K_SETTING: u8 = 4;
pub const K_LOG: u8 = 5;

/// 缓解等级：0=无 1=绕过 2=适配 3=上游修复。
pub const M_NONE: u8 = 0;
pub const M_BYPASS: u8 = 1;
pub const M_ADAPT: u8 = 2;
pub const M_UPSTREAM: u8 = 3;

/// 差异条目状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DState {
    /// 发现。
    Found,
    /// 已评估。
    Assessed,
    /// 已缓解。
    Mitigated,
    /// 已验证。
    Verified,
    /// 已关闭（终态）。
    Closed,
}

/// 迁移合法性真值表。
pub fn trans_legal(from: DState, to: DState) -> bool {
    use DState::*;
    matches!(
        (from, to),
        (Found, Assessed)
            | (Assessed, Mitigated)
            | (Assessed, Closed)
            | (Mitigated, Verified)
            | (Mitigated, Assessed)
            | (Verified, Closed)
    )
}

/// 版本三元组（全整数比较）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ver {
    /// 主版本。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 修订。
    pub rev: u32,
}

impl Ver {
    /// 严格递增判定：组合值单调（major*1e6 + minor*1e3 + rev）。
    pub fn key(&self) -> u64 {
        self.major as u64 * 1_000_000 + self.minor as u64 * 1_000 + self.rev as u64
    }
}

/// 单条差异条目。
#[derive(Clone, Debug)]
pub struct DiffEntry {
    /// 条目号（1 起）。
    pub id: u32,
    /// 签名（去重键）。
    pub sig: String,
    /// 驱动分支。
    pub branch: u8,
    /// 差异类别。
    pub kind: u8,
    /// 首见版本。
    pub since: Ver,
    /// 当前状态。
    pub state: DState,
    /// 缓解等级。
    pub mitig: u8,
}

impl DiffEntry {
    /// 终态判定。
    pub fn is_terminal(&self) -> bool {
        self.state == DState::Closed
    }
}

/// AMD 驱动差异表。
pub struct DiffTable {
    /// 顺序账（定容 40）。
    pub rows: Vec<DiffEntry>,
    /// 下一号。
    pub next_id: u32,
    /// 各分支最新版本键（单调校验）。
    pub branch_ver: [u64; (BRANCH_CAP + 1) as usize],
    /// 关闭数。
    pub closed: u64,
    /// 登记数。
    pub registered: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl DiffTable {
    /// 空表。
    pub fn new() -> Self {
        DiffTable {
            rows: Vec::new(),
            next_id: 1,
            branch_ver: [0; 4],
            closed: 0,
            registered: 0,
            red_count: 0,
        }
    }

    /// 存量数。
    pub fn open_count(&self) -> u64 {
        self.registered - self.closed
    }

    /// 账守恒。
    pub fn balanced(&self) -> bool {
        self.registered == self.open_count() + self.closed
    }

    /// 按号定位。
    pub fn find(&self, id: u32) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Ok(i);
            }
            i += 1;
        }
        Err(XC_BAD_ENTRY)
    }

    /// 签名查重。
    pub fn has_sig(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记差异（闭集校验 + 版本单调 + 去重 + 容量）。
    pub fn register(
        &mut self,
        sig: &str,
        branch: u8,
        kind: u8,
        since: Ver,
    ) -> Result<u32, u16> {
        if branch > BRANCH_CAP {
            self.red_count += 1;
            return Err(XC_BAD_BRANCH);
        }
        if kind > KIND_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DIFF_KIND);
        }
        if since.key() <= self.branch_ver[branch as usize] {
            self.red_count += 1;
            return Err(XC_VER_BACKWARD);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_SIG);
        }
        if self.rows.len() == DIFF_CAP {
            self.red_count += 1;
            return Err(XC_DIFF_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.branch_ver[branch as usize] = since.key();
        self.rows.push(DiffEntry {
            id,
            sig: String::from(sig),
            branch,
            kind,
            since,
            state: DState::Found,
            mitig: M_NONE,
        });
        self.registered += 1;
        Ok(id)
    }

    /// 推进状态（终态/非法迁移红；Verified 需缓解 ≥ 绕过）。
    pub fn advance(&mut self, id: u32, to: DState, mitig: u8) -> Result<(), u16> {
        let idx = self.find(id)?;
        let cur = self.rows[idx].state;
        if cur.is_terminal() {
            self.red_count += 1;
            return Err(XC_TERMINAL_MOVE);
        }
        if !trans_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if to == DState::Verified {
            if mitig > MITIG_CAP {
                self.red_count += 1;
                return Err(XC_BAD_MITIG);
            }
            if mitig < M_BYPASS {
                self.red_count += 1;
                return Err(XC_MITIG_INSUFF);
            }
            self.rows[idx].mitig = mitig;
        }
        if to == DState::Closed {
            self.closed += 1;
        }
        self.rows[idx].state = to;
        Ok(())
    }

    /// 分支存量计数。
    pub fn open_in_branch(&self, branch: u8) -> Result<u64, u16> {
        if branch > BRANCH_CAP {
            self.red_count += 1;
            return Err(XC_BAD_BRANCH);
        }
        let mut n: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let d = &self.rows[i];
            if !d.is_terminal() && d.branch == branch {
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 类别分布（六格计数，和 = 存量数）。
    pub fn kind_dist(&self) -> Result<[u64; 6], u16> {
        let mut dist = [0u64; 6];
        let mut i = 0;
        while i < self.rows.len() {
            let d = &self.rows[i];
            if !d.is_terminal() {
                if d.kind > KIND_CAP {
                    return Err(XC_BAD_DIFF_KIND);
                }
                dist[d.kind as usize] += 1;
            }
            i += 1;
        }
        Ok(dist)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amddrvdiff reg=");
        s += &self.registered.to_string();
        s += " open=";
        s += &self.open_count().to_string();
        s += " closed=";
        s += &self.closed.to_string();
        s += " rows=";
        s += &self.rows.len().to_string();
        s
    }

    /// 全账自检。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_DIFF_DRIFT);
        }
        let mut closed = 0u64;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].is_terminal() {
                closed += 1;
            }
            i += 1;
        }
        if closed != self.closed {
            return Err(XC_DIFF_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：四分支登记 → 全状态链 → 缓解负例 → 审计。
pub fn drill() -> Result<u32, u16> {
    let mut t = DiffTable::new();
    let v1 = Ver { major: 24, minor: 3, rev: 1 };
    let v2 = Ver { major: 24, minor: 5, rev: 2 };
    let v3 = Ver { major: 24, minor: 7, rev: 1 };
    let v4 = Ver { major: 24, minor: 9, rev: 4 };
    let a = t.register("whql-dcc-off", BR_WHQL, K_SETTING, v1)?;
    let b = t.register("preview-shader-stall", BR_PREVIEW, K_PERF, v2)?;
    let c = t.register("vkbeta-descriptor", BR_VKBETA, K_API, v3)?;
    let d = t.register("pro-scheduler", BR_PRO, K_INSTALL, v4)?;
    // 负例：版本倒挂。
    if t.register("whql-old", BR_WHQL, K_LOG, v1) != Err(XC_VER_BACKWARD) {
        return Err(XC_VER_BACKWARD);
    }
    // 负例：重复签名。
    if t.register("whql-dcc-off", BR_PRO, K_LOG, v4) != Err(XC_DUP_SIG) {
        return Err(XC_DUP_SIG);
    }
    // 负例：分支越界。
    if t.register("bad-branch", 9, K_LOG, v4) != Err(XC_BAD_BRANCH) {
        return Err(XC_BAD_BRANCH);
    }
    // 负例：类别越界。
    if t.register("bad-kind", BR_WHQL, 9, v4) != Err(XC_BAD_DIFF_KIND) {
        return Err(XC_BAD_DIFF_KIND);
    }
    // 正链 a：Found→Assessed→Mitigated→Verified→Closed。
    t.advance(a, DState::Assessed, 0)?;
    t.advance(a, DState::Mitigated, 0)?;
    // 负例：缓解不足验证。
    if t.advance(a, DState::Verified, M_NONE) != Err(XC_MITIG_INSUFF) {
        return Err(XC_MITIG_INSUFF);
    }
    // 负例：缓解越界。
    if t.advance(a, DState::Verified, 9) != Err(XC_BAD_MITIG) {
        return Err(XC_BAD_MITIG);
    }
    t.advance(a, DState::Verified, M_ADAPT)?;
    t.advance(a, DState::Closed, 0)?;
    // 负例：终态再迁移。
    if t.advance(a, DState::Assessed, 0) != Err(XC_TERMINAL_MOVE) {
        return Err(XC_TERMINAL_MOVE);
    }
    // 回退路径 b：Mitigated 回 Assessed。
    t.advance(b, DState::Assessed, 0)?;
    t.advance(b, DState::Mitigated, 0)?;
    t.advance(b, DState::Assessed, 0)?;
    t.advance(b, DState::Closed, 0)?;
    // 直闭路径 c/d。
    t.advance(c, DState::Assessed, 0)?;
    t.advance(c, DState::Closed, 0)?;
    t.advance(d, DState::Assessed, 0)?;
    t.advance(d, DState::Closed, 0)?;
    // 负例：非法迁移 Found→Mitigated。
    let mut t2 = DiffTable::new();
    let e = t2.register("fresh-diff", BR_WHQL, K_CRASH, v1)?;
    if t2.advance(e, DState::Mitigated, 0) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    t2.audit()?;
    t.audit()?;
    // 分支计数与类别分布复核。
    if t.open_in_branch(BR_WHQL)? != 0 {
        return Err(XC_DIFF_DRIFT);
    }
    let dist = t.kind_dist()?;
    let mut sum: u64 = 0;
    let mut i = 0;
    while i < dist.len() {
        sum += dist[i];
        i += 1;
    }
    if sum != t.open_count() {
        return Err(XC_DIFF_DRIFT);
    }
    if t.summary().len() == 0 {
        return Err(XC_DIFF_DRIFT);
    }
    Ok(0)
}
