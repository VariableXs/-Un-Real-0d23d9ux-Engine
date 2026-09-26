//! F551 特权操作确认窗 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：触发操作白名单；内核通道判据；会话内合并授权；留痕；
//! 伪造注入失败测试。
//!
//! **设计要点（主册）**：
//! - 系统级敏感操作（改引导项/格式化系统区/装内核级驱动）统一特权确认窗：
//!   全屏压暗 + 居中确认卡（操作方名称 + 要做什么 + 「允许一次/取消」）；
//! - 与 F406 安全屏同走内核通道**不可伪造**——本模块把「通道」做成显式枚举：
//!   只有 `KernelChannel` 发起的请求进入确认流，用户态通道的请求与
//!   「伪造的已授权记录」一律拒绝并留痕；
//! - 连续同类操作**会话内合并授权**（不每步都问）——合并按
//!   (操作方, 操作类) 二元组记会话授权，会话结束（`end_session`）即收回；
//! - 确认留痕走环形账（主册 F372 时间线的内核侧对账面）。
//!
//! 依赖接缝（F372 时间线 / F406 安全屏）按 AI-K2 纪律以**注入口**承接：
//! 本模块只持自洽模型，不反向制造对未落地模块的编译依赖。
//!
//! 钟注入式（ms 实参），无外部依赖。

use crate::checks::CheckSet;
use crate::istar::ibase::{CloneLog, ISTAR_DOMAIN};

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册功能定义）
// ---------------------------------------------------------------------------

/// 全屏压暗不透明度（%，主册「全屏压暗」定档）。
pub const DIM_OPACITY_PCT: u8 = 60;

/// 留痕环形账容量（条——超限滚动淘汰最旧）。
pub const AUDIT_RING: usize = 64;

/// 会话合并授权上限（类·操作方二元组数——超出即整体失效回逐次询问，
/// 防白名单被撑爆成「永久全免问」）。
pub const MERGE_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 通道与操作类（白名单唯一源）
// ---------------------------------------------------------------------------

/// 请求通道——内核通道判据的唯一事实源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// 内核安全屏通道（F406 同源）——唯一合法入口。
    Kernel,
    /// 用户态通道——技术上不合法，请求必拒并留痕。
    User,
}

/// 触发操作白名单（主册点名三类；本枚举即白名单唯一源——枚举外没有第四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// 改引导项。
    BootEntryEdit,
    /// 格式化系统区。
    SystemFormat,
    /// 装内核级驱动。
    KernelDriverInstall,
}

impl OpKind {
    /// 确认卡上「要做什么」的人话文案（三要素之一：发生了什么）。
    pub fn what(self) -> &'static str {
        match self {
            OpKind::BootEntryEdit => "修改系统引导项",
            OpKind::SystemFormat => "格式化系统分区",
            OpKind::KernelDriverInstall => "安装内核级驱动",
        }
    }
}

/// 确认裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 弹窗待决（首次同类操作）。
    Pending,
    /// 会话内已合并授权——直接放行（不再弹窗）。
    MergedGrant,
    /// 本次确认「允许一次」。
    AllowedOnce,
    /// 取消。
    Denied,
    /// 通道不合法——请求在门外即拒（不弹窗）。
    RejectedChannel,
    /// 白名单外操作——请求在门外即拒（不弹窗）。
    RejectedWhitelist,
}

/// 一条留痕。
#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub ms: u64,
    pub caller: String,
    pub kind: OpKind,
    pub verdict: Verdict,
}

// ---------------------------------------------------------------------------
// 确认窗状态机
// ---------------------------------------------------------------------------

/// 特权确认窗（全屏压暗层 + 居中确认卡 + 会话合并账 + 留痕环）。
pub struct PrivGate {
    /// 当前弹出的待决请求（None = 窗未显示）。
    pending: Option<(String, OpKind)>,
    /// 会话内合并授权账：(操作方, 操作类) 二元组。
    merged: [(String, OpKind); MERGE_CAP],
    merged_len: usize,
    /// 会话开关（end_session 收回全部合并授权）。
    session_open: bool,
    /// 留痕环。
    audit: CloneLog<AuditEntry, AUDIT_RING>,
    now_ms: u64,
}

impl PrivGate {
    pub fn new() -> PrivGate {
        PrivGate {
            pending: None,
            merged: [
                (String::new(), OpKind::BootEntryEdit),
                (String::new(), OpKind::SystemFormat),
                (String::new(), OpKind::KernelDriverInstall),
                (String::new(), OpKind::BootEntryEdit),
                (String::new(), OpKind::SystemFormat),
                (String::new(), OpKind::KernelDriverInstall),
                (String::new(), OpKind::BootEntryEdit),
                (String::new(), OpKind::SystemFormat),
                (String::new(), OpKind::KernelDriverInstall),
                (String::new(), OpKind::BootEntryEdit),
                (String::new(), OpKind::SystemFormat),
                (String::new(), OpKind::KernelDriverInstall),
                (String::new(), OpKind::BootEntryEdit),
                (String::new(), OpKind::SystemFormat),
                (String::new(), OpKind::KernelDriverInstall),
                (String::new(), OpKind::BootEntryEdit),
            ],
            merged_len: 0,
            session_open: true,
            audit: CloneLog::default(),
            now_ms: 0,
        }
    }

    /// 前进时钟（请求/裁决以 ms 实参统一走此口，保账目单调）。
    pub fn tick(&mut self, ms: u64) {
        if ms > self.now_ms {
            self.now_ms = ms;
        }
    }

    /// 发起敏感操作请求。
    ///
    /// 顺序即防线：① 通道判据（Kernel 才继续）→ ② 白名单判据（枚举即名单，
    /// 传入 OpKind 已天然在名单内；本步防的是「会话已关」态）→
    /// ③ 会话内合并（命中即 MergedGrant 不弹窗）→ ④ 弹窗 Pending。
    pub fn request(&mut self, channel: Channel, caller: &str, kind: OpKind) -> Verdict {
        self.tick(self.now_ms); // 占位推进（真实时刻由宿主经 tick 注入）
        if channel != Channel::Kernel {
            self.leave_trace(caller, kind, Verdict::RejectedChannel);
            return Verdict::RejectedChannel;
        }
        if !self.session_open {
            self.leave_trace(caller, kind, Verdict::RejectedWhitelist);
            return Verdict::RejectedWhitelist;
        }
        if self.has_merge(caller, kind) {
            self.leave_trace(caller, kind, Verdict::MergedGrant);
            return Verdict::MergedGrant;
        }
        self.pending = Some((String::from(caller), kind));
        self.leave_trace(caller, kind, Verdict::Pending);
        Verdict::Pending
    }

    /// 确认卡字段齐备性（操作方名称 + 要做什么 + 两钮文案）——渲染层唯一取数口。
    pub fn card(&self) -> Option<(&str, &'static str)> {
        self.pending
            .as_ref()
            .map(|(c, k)| (c.as_str(), k.what()))
    }

    /// 全屏压暗层参数（唯一渲染事实源）。
    pub fn dim_opacity(&self) -> u8 {
        DIM_OPACITY_PCT
    }

    /// 「允许一次」。
    pub fn allow_once(&mut self) -> Option<OpKind> {
        let (_, kind) = self.pending.take()?;
        self.leave_trace("允许一次", kind, Verdict::AllowedOnce);
        Some(kind)
    }

    /// 「取消」。
    pub fn deny(&mut self) -> Option<OpKind> {
        let (_, kind) = self.pending.take()?;
        self.leave_trace("取消", kind, Verdict::Denied);
        Some(kind)
    }

    /// 会话内合并授权（确认窗第二问起可见——「同类不再问」勾选语义的落点）。
    pub fn grant_and_merge(&mut self) -> Option<OpKind> {
        let (caller, kind) = self.pending.take()?;
        if self.merged_len < MERGE_CAP {
            self.merged[self.merged_len] = (caller.clone(), kind);
            self.merged_len += 1;
        } else {
            // 超上限：整体失效（本会话回到逐次询问）——诚实降级，不静默挤兑。
            self.merged_len = 0;
            for slot in self.merged.iter_mut() {
                slot.0.clear();
            }
        }
        self.leave_trace(&caller, kind, Verdict::AllowedOnce);
        Some(kind)
    }

    /// 会话结束：合并授权全部收回（「问过一次同类不再烦」只在会话内成立）。
    pub fn end_session(&mut self) -> usize {
        let n = self.merged_len;
        self.merged_len = 0;
        for slot in self.merged.iter_mut() {
            slot.0.clear();
        }
        n
    }

    /// 留痕条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 留痕按新到旧（时间线对账面）。
    pub fn audit(&self) -> alloc::vec::Vec<AuditEntry> {
        self.audit.newest_first()
    }

    // -- 内部 --

    fn has_merge(&self, caller: &str, kind: OpKind) -> bool {
        self.merged[..self.merged_len]
            .iter()
            .any(|(c, k)| c == caller && *k == kind)
    }

    fn leave_trace(&mut self, caller: &str, kind: OpKind, verdict: Verdict) {
        self.audit.push(AuditEntry {
            ms: self.now_ms,
            caller: String::from(caller),
            kind,
            verdict,
        });
    }
}

impl Default for PrivGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_privconfirm_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 白名单三类齐备且文案三要素在卡上可读。
    let whats = [OpKind::BootEntryEdit.what(), OpKind::SystemFormat.what(), OpKind::KernelDriverInstall.what()];
    set.add(
        "whitelist three kinds with card text",
        whats.iter().all(|w| !w.is_empty()),
        "",
    );

    // 2. 内核通道判据：Kernel 通道 → Pending 弹窗；User 通道 → 门外即拒。
    let mut g = PrivGate::new();
    let k = g.request(Channel::Kernel, "diskmgr", OpKind::SystemFormat);
    let u = g.request(Channel::User, "forged", OpKind::SystemFormat);
    set.add(
        "kernel channel only",
        k == Verdict::Pending && u == Verdict::RejectedChannel,
        "",
    );

    // 3. 全屏压暗 + 居中确认卡字段齐（操作方 + 要做什么）。
    let card_ok = g
        .card()
        .map(|(c, w)| c == "diskmgr" && w == OpKind::SystemFormat.what())
        .unwrap_or(false);
    set.add("dim and card fields", g.dim_opacity() == DIM_OPACITY_PCT && card_ok, "");

    // 4. 允许一次：放行且窗收；取消：拒且窗收。
    let mut g2 = PrivGate::new();
    g2.request(Channel::Kernel, "bootcfg", OpKind::BootEntryEdit);
    let a = g2.allow_once();
    let mut g3 = PrivGate::new();
    g3.request(Channel::Kernel, "bootcfg", OpKind::BootEntryEdit);
    let d = g3.deny();
    set.add(
        "allow once and deny both close",
        a == Some(OpKind::BootEntryEdit) && d == Some(OpKind::BootEntryEdit) && g2.card().is_none() && g3.card().is_none(),
        "",
    );

    // 5. 会话内合并授权：同类第二问 MergedGrant 不弹窗；异类仍 Pending。
    let mut g4 = PrivGate::new();
    g4.request(Channel::Kernel, "devtool", OpKind::KernelDriverInstall);
    g4.grant_and_merge();
    let again = g4.request(Channel::Kernel, "devtool", OpKind::KernelDriverInstall);
    let other = g4.request(Channel::Kernel, "devtool", OpKind::SystemFormat);
    set.add(
        "session merge same kind only",
        again == Verdict::MergedGrant && other == Verdict::Pending,
        "",
    );

    // 6. 会话结束收回：end_session 后同类再问回 Pending。
    let mut g5 = PrivGate::new();
    g5.request(Channel::Kernel, "devtool", OpKind::KernelDriverInstall);
    g5.grant_and_merge();
    let closed = g5.end_session();
    let after = g5.request(Channel::Kernel, "devtool", OpKind::KernelDriverInstall);
    set.add(
        "session end revokes merge",
        closed == 1 && after == Verdict::Pending,
        "",
    );

    // 7. 合并上限诚实降级：撑爆后账清空、回到逐次询问（不静默挤兑旧账）。
    let mut g6 = PrivGate::new();
    let mut last = Verdict::Pending;
    for i in 0..MERGE_CAP + 2 {
        let caller = if i < MERGE_CAP {
            alloc::format!("app{}", i)
        } else {
            String::from("overflow")
        };
        let kind = OpKind::BootEntryEdit;
        g6.request(Channel::Kernel, &caller, kind);
        last = if g6.grant_and_merge().is_some() {
            Verdict::AllowedOnce
        } else {
            Verdict::Denied
        };
    }
    let after_cap = g6.request(Channel::Kernel, "app0", OpKind::BootEntryEdit);
    set.add(
        "merge cap degrades honestly",
        last == Verdict::AllowedOnce && after_cap == Verdict::Pending,
        "",
    );

    // 8. 留痕：请求-裁决逐条入环、时间单调、环容量滚动。
    let mut g7 = PrivGate::new();
    for t in 0..(AUDIT_RING as u64 + 8) {
        g7.tick(t * 1_000);
        g7.request(Channel::Kernel, "sweeper", OpKind::BootEntryEdit);
        let _ = g7.allow_once();
    }
    let aud = g7.audit();
    let monotonic = aud.windows(2).all(|w| w[0].ms >= w[1].ms);
    set.add(
        "audit ring monotonic and capped",
        g7.audit_len() == AUDIT_RING && monotonic,
        "",
    );

    // 9. 伪造注入失败测试 A：User 通道请求必拒且留痕在案。
    let mut g8 = PrivGate::new();
    let forged = g8.request(Channel::User, "malware", OpKind::SystemFormat);
    let traced = g8
        .audit()
        .first()
        .map(|e| e.verdict == Verdict::RejectedChannel && e.caller == "malware")
        .unwrap_or(false);
    set.add("forged channel rejected and traced", forged == Verdict::RejectedChannel && traced, "");

    // 10. 伪造注入失败测试 B：伪造「已授权记录」不可行——合并账只经
    //     grant_and_merge 单口写入，门外拒绝路径不产生合并账。
    let mut g9 = PrivGate::new();
    g9.request(Channel::User, "malware", OpKind::KernelDriverInstall);
    let not_merged = g9.request(Channel::Kernel, "malware", OpKind::KernelDriverInstall);
    set.add(
        "forged grant impossible",
        not_merged == Verdict::Pending,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_then_allow_flow() {
        let mut g = PrivGate::new();
        g.tick(1_000);
        assert_eq!(g.request(Channel::Kernel, "diskmgr", OpKind::SystemFormat), Verdict::Pending);
        let (caller, what) = g.card().unwrap();
        assert_eq!(caller, "diskmgr");
        assert_eq!(what, "格式化系统分区");
        assert_eq!(g.allow_once(), Some(OpKind::SystemFormat));
        assert!(g.card().is_none());
    }

    #[test]
    fn allow_without_pending_is_none() {
        let mut g = PrivGate::new();
        assert_eq!(g.allow_once(), None);
        assert_eq!(g.deny(), None);
    }

    #[test]
    fn merge_is_pairwise() {
        let mut g = PrivGate::new();
        g.request(Channel::Kernel, "a", OpKind::BootEntryEdit);
        g.grant_and_merge();
        g.request(Channel::Kernel, "b", OpKind::BootEntryEdit);
        g.grant_and_merge();
        // 同操作类、不同操作方 → 不共享合并。
        assert_eq!(g.request(Channel::Kernel, "a", OpKind::BootEntryEdit), Verdict::MergedGrant);
        assert_eq!(g.request(Channel::Kernel, "b", OpKind::BootEntryEdit), Verdict::MergedGrant);
        assert_eq!(g.request(Channel::Kernel, "c", OpKind::BootEntryEdit), Verdict::Pending);
    }

    #[test]
    fn audit_newest_first_with_ms() {
        let mut g = PrivGate::new();
        g.tick(5_000);
        g.request(Channel::Kernel, "x", OpKind::SystemFormat);
        g.tick(6_000);
        g.deny();
        let aud = g.audit();
        assert_eq!(aud.len(), 2);
        assert_eq!(aud[0].verdict, Verdict::Denied);
        assert_eq!(aud[0].ms, 6_000);
        assert_eq!(aud[1].verdict, Verdict::Pending);
        assert_eq!(aud[1].ms, 5_000);
    }

    #[test]
    fn clock_is_monotonic_injection() {
        let mut g = PrivGate::new();
        g.tick(10_000);
        g.tick(9_000); // 回拨被吞——账目单调纪律
        g.request(Channel::Kernel, "x", OpKind::SystemFormat);
        assert_eq!(g.audit()[0].ms, 10_000);
    }
}
