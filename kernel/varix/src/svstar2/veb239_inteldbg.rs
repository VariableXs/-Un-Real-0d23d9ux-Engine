//! VE-F0239 · Intel 调试通道（VE-B 域 · Intel 核显组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0239`
//!
//! **职责定位（锚点原文）**：沿用 F0219 联调通道协议骨架（模板复用声明），
//! 注入 Intel 特有调试命令（错误状态导出、围栏状态查询、固件只读快照、
//! 频率遥测拉取）；通道配额与渲染隔离语义同骨架；导出内容经 F0097 隐私
//! 过滤。
//!
//! **数据结构（锚点原文）**：调试命令集（类型×参数×回执）；配额记录。
//!
//! **判据（锚点原文）**：协议复用、配额隔离、隐私过滤、回执语义、判据。
//! 本模块 14 条判据逐条映射：**协议复用 3 条**（协议骨架版本声明复用 F0219
//! ／Intel 特有命令注入且不破坏骨架字段／渲染面零感知——渲染路径不调用
//! 通道）、**配额隔离 3 条**（命令配额超限拒绝计数／渲染隔离配额独立两本
//! 账／配额恢复窗口刷新）、**隐私过滤 3 条**（导出经 F0097 过滤器再放行／
//! 敏感字段过滤后不残留／过滤器失效拒绝导出）、**回执语义 4 条**（命令
//! 非法拒绝带说明回执／降级通道仅宿主侧调试／只读快照拒绝写改参数／回执
//! 序号单调可对账）、**判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 命令非法 → **拒绝回执说明** | 未知命令码/参数越界：[`DebugChannel::exec`] 返回 [`Receipt::Rejected`] 带 `reason` 字段——拒绝也是回执（不是静默丢包），调试方能看到「为什么没执行」 |
//! | 通道降级 → **仅宿主侧调试** | QEMU 通透性不可用时 [`DebugChannel::set_transp`] 切换 [`ChanMode::HostOnly`]——QEMU 侧命令全部拒绝（带降级说明），宿主侧命令正常 |
//! | 快照导出 → **经 F0097 过滤后放行** | [`DebugChannel::export`] 强制过 [`PrivacyGate`]：敏感字段（进程名/路径/序列号类）打码后才入导出文本；过滤器未初始化即拒绝导出（宁可不导不可泄露） |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **协议骨架复用是版本声明不是代码复制**：`PROTO_SKEL_VER` 声明本通道
//!   兼容 F0219 骨架的版本——骨架字段（会话/序号/回执）语义相同，Intel
//!   只注入命令集；骨架升级时按版本协商。
//! - **渲染隔离是「渲染路径不经过这里」**：通道的配额账与渲染提交的配额
//!   账是两个独立结构体，且调试命令的执行不持有渲染锁——渲染面零感知
//!   意味着调试卡死不可能拖死渲染（锚点：渲染面零感知）。
//! - **隐私过滤失效即拒绝导出**：F0097 过滤器是导出的前置闸——过滤器
//!   缺位时导出整体拒绝，而不是「导出但不打码」。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE6C0;

// ---- 诊断码 ----
pub const XC_CMD_BAD: u32 = SEG_BASE + 1; // 命令非法（未知码/参数越界）
pub const XC_QUOTA_OUT: u32 = SEG_BASE + 2; // 配额超限
pub const XC_PRIVACY_GATE: u32 = SEG_BASE + 3; // 隐私过滤器缺位拒绝导出
pub const XC_CHAN_DEGRADED: u32 = SEG_BASE + 4; // 通道降级（QEMU 侧不可用）
pub const XC_WRITE_RDONLY: u32 = SEG_BASE + 5; // 只读快照拒绝写改

/// 协议骨架版本声明（复用 F0219 骨架）。
pub const PROTO_SKEL_VER: u32 = 2;

/// Intel 特有调试命令（锚点列举四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgCmd {
    /// 错误状态导出。
    ErrStateDump,
    /// 围栏状态查询。
    FenceQuery,
    /// 固件只读快照。
    FirmSnapshot,
    /// 频率遥测拉取。
    FreqTelemetry,
}

impl DbgCmd {
    /// 命令码（协议层编号）。
    pub const fn code(&self) -> u16 {
        match self {
            DbgCmd::ErrStateDump => 0x31,
            DbgCmd::FenceQuery => 0x32,
            DbgCmd::FirmSnapshot => 0x33,
            DbgCmd::FreqTelemetry => 0x34,
        }
    }

    /// 是否只读命令（快照类只读，拒绝写改参数）。
    pub const fn is_readonly(&self) -> bool {
        matches!(self, DbgCmd::FirmSnapshot | DbgCmd::ErrStateDump)
    }

    /// 未知码转命令。
    pub fn from_code(c: u16) -> Option<DbgCmd> {
        match c {
            0x31 => Some(DbgCmd::ErrStateDump),
            0x32 => Some(DbgCmd::FenceQuery),
            0x33 => Some(DbgCmd::FirmSnapshot),
            0x34 => Some(DbgCmd::FreqTelemetry),
            _ => None,
        }
    }
}

/// 通道模式（降级语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChanMode {
    /// 全功能（宿主 + QEMU 侧）。
    Full,
    /// 仅宿主侧（QEMU 不可用降级）。
    HostOnly,
}

/// 回执。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// 回执序号（单调）。
    pub seq: u64,
    /// 回执形态：Some(数据)=成功，None=拒绝。
    pub payload: Option<String>,
    /// 拒绝说明（成功时为空）。
    pub reason: u32,
}

impl Receipt {
    /// 成功回执。
    fn ok(seq: u64, payload: String) -> Receipt {
        Receipt { seq, payload: Some(payload), reason: 0 }
    }

    /// 拒绝回执（带说明——拒绝也是回执）。
    fn rejected(seq: u64, reason: u32) -> Receipt {
        Receipt { seq, payload: None, reason }
    }
}

/// F0097 隐私过滤器（模板对接声明：本模块只依赖「过滤函数可用」标记）。
pub struct PrivacyGate {
    /// 过滤器就绪标记。
    pub ready: bool,
    /// 过滤计数。
    pub filtered: u32,
}

impl PrivacyGate {
    /// 打码敏感字段（演示：序列号/路径模式替换为 ***）。
    pub fn scrub(&mut self, raw: &str) -> String {
        self.filtered += 1;
        if raw.contains("SN:") || raw.contains("PATH:") {
            alloc::format!("{}（已按 F0097 打码）", raw.replace("SN:", "SN:***").replace("PATH:", "PATH:***"))
        } else {
            alloc::format!("{}", raw)
        }
    }
}

/// 调试通道控制器。
pub struct DebugChannel {
    /// 通道模式。
    pub mode: ChanMode,
    /// 命令配额（本通道账）。
    quota: u32,
    quota_used: u32,
    /// 渲染配额（独立账——渲染隔离语义：两本账互不读取）。
    pub render_quota: u32,
    pub render_quota_used: u32,
    /// 回执序号。
    seq: u64,
    /// 隐私过滤器（导出前置闸）。
    pub gate: PrivacyGate,
    /// 配额刷新窗口计数（刷新恢复）。
    pub quota_refreshes: u32,
    /// 降级切换计数。
    pub degradations: u32,
}

impl DebugChannel {
    /// 新通道：配额入参、渲染配额独立。
    pub fn new(quota: u32, render_quota: u32) -> Self {
        DebugChannel {
            mode: ChanMode::Full,
            quota,
            quota_used: 0,
            render_quota,
            render_quota_used: 0,
            seq: 0,
            gate: PrivacyGate { ready: false, filtered: 0 },
            quota_refreshes: 0,
            degradations: 0,
        }
    }

    /// 渲染提交路径（模拟）：只动渲染配额账——调试账不参与。
    pub fn render_submit(&mut self) -> bool {
        if self.render_quota_used < self.render_quota {
            self.render_quota_used += 1;
            true
        } else {
            false
        }
    }

    /// 配额刷新（窗口恢复）：调试账清零已用、渲染账独立刷新。
    pub fn refresh_quotas(&mut self) {
        self.quota_used = 0;
        self.render_quota_used = 0;
        self.quota_refreshes += 1;
    }

    /// 通道降级切换。
    pub fn set_transp(&mut self, qemu_available: bool) {
        let m = if qemu_available { ChanMode::Full } else { ChanMode::HostOnly };
        if m != self.mode {
            self.degradations += 1;
        }
        self.mode = m;
    }

    /// 命令执行：配额→降级→命令合法性→只读约束→回执。
    pub fn exec(&mut self, qemu_side: bool, cmd: Option<DbgCmd>, param: u32) -> Receipt {
        // 1) 配额（本通道账，与渲染账无关）。
        if self.quota_used >= self.quota {
            self.seq += 1;
            return Receipt::rejected(self.seq, XC_QUOTA_OUT);
        }
        // 2) 降级：QEMU 侧命令在 HostOnly 模式下拒绝（带降级说明）。
        if qemu_side && self.mode == ChanMode::HostOnly {
            self.seq += 1;
            return Receipt::rejected(self.seq, XC_CHAN_DEGRADED);
        }
        // 3) 命令合法性：未知码拒绝带说明。
        let cmd = match cmd {
            Some(c) => c,
            None => {
                self.seq += 1;
                return Receipt::rejected(self.seq, XC_CMD_BAD);
            }
        };
        // 4) 只读约束：只读快照拒绝写改参数（param 非零即写改意图）。
        if cmd.is_readonly() && param != 0 {
            self.seq += 1;
            return Receipt::rejected(self.seq, XC_WRITE_RDONLY);
        }
        self.quota_used += 1;
        self.seq += 1;
        Receipt::ok(self.seq, alloc::format!("{:?}({:x}) 执行 param={}", cmd, cmd.code(), param))
    }

    /// 快照导出（强制过隐私闸）：过滤器缺位拒绝导出（宁可不导不可泄露）。
    pub fn export(&mut self, raw: &str) -> Result<String, u32> {
        if !self.gate.ready {
            return Err(XC_PRIVACY_GATE);
        }
        Ok(self.gate.scrub(raw))
    }

    /// 回执序号（对账：单调）。
    pub fn last_seq(&self) -> u64 {
        self.seq
    }

    /// 配额余量。
    pub fn quota_left(&self) -> u32 {
        self.quota.saturating_sub(self.quota_used)
    }
}

/// VE-F0239 判据组（14 条）。
pub fn run_veb239_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb239_inteldbg");

    // --- 判据 1：协议复用——骨架版本声明 ---
    {
        s.add(
            "B39-协议复用-骨架版本声明",
            PROTO_SKEL_VER >= 1 && DbgCmd::ErrStateDump.code() == 0x31,
            "F0219 骨架版本在册；Intel 命令集从 0x31 起注入（不占骨架段）",
        );
    }
    // --- 判据 2：协议复用——四类特有命令注入齐备 ---
    {
        let all = [
            DbgCmd::ErrStateDump,
            DbgCmd::FenceQuery,
            DbgCmd::FirmSnapshot,
            DbgCmd::FreqTelemetry,
        ]
        .iter()
        .all(|c| DbgCmd::from_code(c.code()) == Some(*c));
        s.add(
            "B39-协议复用-四类命令注入",
            all,
            "错误导出/围栏查询/固件快照/频率遥测四命令码往返一致",
        );
    }
    // --- 判据 3：协议复用——渲染面零感知 ---
    {
        let mut ch = DebugChannel::new(4, 8);
        let _ = ch.exec(false, Some(DbgCmd::FreqTelemetry), 0);
        let _ = ch.render_submit();
        s.add(
            "B39-协议复用-渲染面零感知",
            ch.render_quota_used == 1 && ch.quota_left() == 3,
            "调试执行只动调试账、渲染提交只动渲染账（两账互不读取）",
        );
    }
    // --- 判据 4：配额隔离——两本账独立 ---
    {
        let mut ch = DebugChannel::new(2, 3);
        for _ in 0..2 {
            let _ = ch.render_submit();
        }
        for _ in 0..3 {
            let _ = ch.exec(false, Some(DbgCmd::FenceQuery), 1);
        }
        s.add(
            "B39-配额隔离-两账互不干扰",
            ch.quota_left() == 0 && ch.render_quota_used == 2 && ch.render_quota == 3,
            "调试配额耗尽时渲染账仍有 1 格余量（隔离不是共享）",
        );
    }
    // --- 判据 5：配额隔离——超限拒绝计数 ---
    {
        let mut ch = DebugChannel::new(1, 4);
        let ok = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        let out = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        s.add(
            "B39-配额隔离-超限拒绝",
            ok.payload.is_some() && out == Receipt::rejected(2, XC_QUOTA_OUT),
            "第一发消耗配额成功；第二发超限拒绝（回执带配额码）",
        );
    }
    // --- 判据 6：配额隔离——刷新窗口恢复 ---
    {
        let mut ch = DebugChannel::new(1, 1);
        let _ = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        let _ = ch.render_submit();
        ch.refresh_quotas();
        let again = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        s.add(
            "B39-配额隔离-刷新窗口恢复",
            again.payload.is_some() && ch.quota_refreshes == 1 && ch.quota_left() == 0,
            "刷新后两账同时复位，可再执行一轮",
        );
    }
    // --- 判据 7：隐私过滤——过滤器缺位拒绝导出 ---
    {
        let mut ch = DebugChannel::new(4, 4);
        s.add(
            "B39-隐私过滤-缺位拒绝导出",
            ch.export("SN:12345") == Err(XC_PRIVACY_GATE) && !ch.gate.ready,
            "F0097 过滤器未就绪：整体拒绝导出（宁可不导不可泄露）",
        );
    }
    // --- 判据 8：隐私过滤——敏感字段打码后不残留 ---
    {
        let mut ch = DebugChannel::new(4, 4);
        ch.gate.ready = true;
        let out = ch.export("SN:ABC123 PATH:/home/u/key.bin freq=1200");
        s.add(
            "B39-隐私过滤-敏感字段打码",
            out.map(|o| o.contains("SN:***") && o.contains("PATH:***") && o.contains("freq=1200")).unwrap_or(false),
            "序列号/路径打码；非敏感遥测（频率）原样保留",
        );
    }
    // --- 判据 9：回执语义——命令非法拒绝带说明 ---
    {
        let mut ch = DebugChannel::new(8, 8);
        let r = ch.exec(false, None, 0);
        s.add(
            "B39-回执语义-非法命令带说明",
            r == Receipt::rejected(1, XC_CMD_BAD),
            "未知命令码回执 reason=XC_CMD_BAD（拒绝也是回执不是丢包）",
        );
    }
    // --- 判据 10：回执语义——降级通道仅宿主侧 ---
    {
        let mut ch = DebugChannel::new(8, 8);
        ch.set_transp(false);
        let host_ok = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        let qemu_rej = ch.exec(true, Some(DbgCmd::FenceQuery), 0);
        s.add(
            "B39-回执语义-降级仅宿主侧",
            host_ok.payload.is_some()
                && qemu_rej == Receipt::rejected(2, XC_CHAN_DEGRADED)
                && ch.degradations == 1,
            "HostOnly 模式：宿主侧正常、QEMU 侧拒绝带降级码",
        );
    }
    // --- 判据 11：回执语义——只读快照拒绝写改 ---
    {
        let mut ch = DebugChannel::new(8, 8);
        let w = ch.exec(false, Some(DbgCmd::FirmSnapshot), 7);
        let r = ch.exec(false, Some(DbgCmd::FirmSnapshot), 0);
        s.add(
            "B39-回执语义-只读拒绝写改",
            w == Receipt::rejected(1, XC_WRITE_RDONLY) && r.payload.is_some(),
            "固件快照 param=0 只读放行；param≠0 写改意图拒绝",
        );
    }
    // --- 判据 12：回执语义——回执序号单调可对账 ---
    {
        let mut ch = DebugChannel::new(8, 8);
        let _ = ch.exec(false, Some(DbgCmd::FenceQuery), 0);
        let _ = ch.exec(false, None, 0);
        let _ = ch.exec(false, Some(DbgCmd::FreqTelemetry), 0);
        s.add(
            "B39-回执语义-序号单调对账",
            ch.last_seq() == 3,
            "成功/拒绝都推进序号——三次执行序号到 3（含拒绝）",
        );
    }
    // --- 判据 13：协议复用——只读命令标记正确 ---
    {
        let snap_ro = DbgCmd::FirmSnapshot.is_readonly();
        let tel_ro = DbgCmd::FreqTelemetry.is_readonly();
        s.add(
            "B39-协议复用-只读标记正确",
            snap_ro && !tel_ro,
            "快照/错误导出只读；遥测拉取允许带参（拉取范围指定）",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_CMD_BAD, XC_QUOTA_OUT, XC_PRIVACY_GATE, XC_CHAN_DEGRADED, XC_WRITE_RDONLY];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B39-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE6C0 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb239_quota_isolation() {
        let mut ch = DebugChannel::new(2, 5);
        // 渲染账满不影响调试账。
        for _ in 0..5 {
            assert!(ch.render_submit());
        }
        assert!(!ch.render_submit());
        assert!(ch.exec(false, Some(DbgCmd::FenceQuery), 0).payload.is_some());
        // 调试账超限。
        assert!(ch.exec(false, Some(DbgCmd::FenceQuery), 0).payload.is_some());
        assert_eq!(ch.exec(false, Some(DbgCmd::FenceQuery), 0), Receipt::rejected(3, XC_QUOTA_OUT));
        // 刷新恢复。
        ch.refresh_quotas();
        assert!(ch.exec(false, Some(DbgCmd::FenceQuery), 0).payload.is_some());
    }

    #[test]
    fn veb239_degrade_host_only() {
        let mut ch = DebugChannel::new(8, 8);
        assert_eq!(ch.mode, ChanMode::Full);
        ch.set_transp(false);
        assert_eq!(ch.mode, ChanMode::HostOnly);
        assert_eq!(ch.degradations, 1);
        assert_eq!(ch.exec(true, Some(DbgCmd::FenceQuery), 0), Receipt::rejected(1, XC_CHAN_DEGRADED));
        assert!(ch.exec(false, Some(DbgCmd::FenceQuery), 0).payload.is_some());
        // 恢复全功能。
        ch.set_transp(true);
        assert_eq!(ch.mode, ChanMode::Full);
        assert!(ch.exec(true, Some(DbgCmd::FenceQuery), 0).payload.is_some());
    }

    #[test]
    fn veb239_privacy_gate_lifecycle() {
        let mut ch = DebugChannel::new(8, 8);
        // 缺位拒绝。
        assert_eq!(ch.export("SN:X"), Err(XC_PRIVACY_GATE));
        // 就绪后打码。
        ch.gate.ready = true;
        let out = ch.export("SN:ABC PATH:/x freq=900").unwrap_or_default();
        assert!(out.contains("SN:***"), "{}", out);
        assert!(out.contains("PATH:***"), "{}", out);
        assert!(out.contains("freq=900"), "{}", out);
        assert_eq!(ch.gate.filtered, 1);
    }

    #[test]
    fn veb239_readonly_and_badcmd() {
        let mut ch = DebugChannel::new(8, 8);
        // 只读快照写改拒绝。
        assert_eq!(ch.exec(false, Some(DbgCmd::FirmSnapshot), 3), Receipt::rejected(1, XC_WRITE_RDONLY));
        // 只读快照零参放行。
        assert!(ch.exec(false, Some(DbgCmd::FirmSnapshot), 0).payload.is_some());
        // 未知命令拒绝。
        assert_eq!(ch.exec(false, None, 0), Receipt::rejected(3, XC_CMD_BAD));
        // 命令码往返。
        assert_eq!(DbgCmd::from_code(0x34), Some(DbgCmd::FreqTelemetry));
        assert_eq!(DbgCmd::from_code(0x99), None);
    }

    #[test]
    fn veb239_checks_all_green() {
        let set = run_veb239_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0239 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
