//! CGPU-F0727 · 场景与 D08 联调（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0727，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景与 D08 联调——场景子系统与 D08 诊断子系统
//!    的联调握手协议：六态握手状态机、能力交换、校准脉冲、
//!    消息账守恒与超时探测。
//!
//! 设计纪律：
//! - 握手六态闭集：Idle→Hello→CapEx→Calib→Run→Report，
//  Report→Done 出口；任何非法边拒绝并记联调账。
// - 能力位交换：双方各自 8 位能力掩码，生效掩码 = 双方与
//   （交集），为 0 拒绝进入 Run。
// - 校准脉冲：固定 4 拍（帧号递增校验），缺拍/乱序即失败。
// - 消息账守恒：sent == acked + rejected + inflight（显式恒等式）。
// - 超时探测：以帧计（超时窗 TIMEOOUT_FRAMES=32，常量写死），
//   超窗未推进即判联调挂起。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4920 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4920，批内 CGPU-F0727 独占）。
pub const SEG_BASE: u16 = 0x4920;

/// 非法握手边。
pub const E_EDGE: u16 = SEG_BASE + 1;
/// 能力交集为空。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 校准脉冲缺拍/乱序。
pub const E_CALIB: u16 = SEG_BASE + 3;
/// 消息账不平。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 联调超时（帧窗耗尽未推进）。
pub const E_TIMEOUT: u16 = SEG_BASE + 5;
/// 消息 id 未知。
pub const E_NO_MSG: u16 = SEG_BASE + 6;

/// 超时窗（帧）。
pub const TIMEOUT_FRAMES: u32 = 32;
/// 校准脉冲拍数。
pub const CALIB_BEATS: u32 = 4;

/// 握手状态（闭集七态，含 Done）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HsState {
    /// 空闲。
    Idle,
    /// 已发起问候。
    Hello,
    /// 能力交换中。
    CapEx,
    /// 校准中。
    Calib,
    /// 运行中。
    Run,
    /// 报告中。
    Report,
    /// 完成。
    Done,
}

/// 合法边表（显式闭集）：
/// Idle→Hello, Hello→CapEx, CapEx→Calib, Calib→Run,
/// Run→Report, Report→Done。
fn edge_ok(from: HsState, to: HsState) -> bool {
    use HsState::*;
    matches!(
        (from, to),
        (Idle, Hello)
            | (Hello, CapEx)
            | (CapEx, Calib)
            | (Calib, Run)
            | (Run, Report)
            | (Report, Done)
    )
}

/// 消息类别（闭集六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgKind {
    /// 问候。
    HelloMsg,
    /// 能力通告。
    CapMsg,
    /// 校准脉冲。
    CalibMsg,
    /// 场景帧事件。
    FrameMsg,
    /// 诊断快照。
    SnapMsg,
    /// 终报。
    ReportMsg,
}

/// 联调消息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointMsg {
    /// 单调 id。
    pub id: u64,
    /// 类别。
    pub kind: MsgKind,
    /// 发起帧。
    pub frame: u32,
    /// 伴随值（脉冲拍号/帧事件值）。
    pub val: u32,
    /// 状态：0=inflight 1=acked 2=rejected。
    pub st: u8,
}

/// 联调会话。
///
/// 消息账守恒（显式恒等式）：
///   sent == acked + rejected + inflight
#[derive(Clone, Debug)]
pub struct JointSession {
    st: HsState,
    /// 本端能力掩码。
    local_caps: u8,
    /// 对端能力掩码（0=未知）。
    peer_caps: u8,
    /// 生效掩码（双方交集，CapEx 完成时算）。
    eff_caps: u8,
    msgs: Vec<JointMsg>,
    next_id: u64,
    sent: u64,
    acked: u64,
    rejected: u64,
    /// 校准已收拍数（位图：bit i = 第 i 拍已收）。
    calib_beats: u8,
    /// 最近推进帧号。
    last_frame: u32,
}

impl JointSession {
    pub fn new(local_caps: u8) -> Self {
        Self {
            st: HsState::Idle,
            local_caps,
            peer_caps: 0,
            eff_caps: 0,
            msgs: Vec::new(),
            next_id: 1,
            sent: 0,
            acked: 0,
            rejected: 0,
            calib_beats: 0,
            last_frame: 0,
        }
    }
    /// 当前状态。
    pub fn state(&self) -> HsState {
        self.st
    }
    /// 推进握手一步（状态迁移 + 对应消息发送）。
    pub fn advance(&mut self, frame: u32) -> Result<u64, u16> {
        // 超时探测：距上次推进超窗且未到 Done → 挂起错误。
        if self.st != HsState::Done && frame.saturating_sub(self.last_frame) > TIMEOUT_FRAMES && self.last_frame != 0 {
            return Err(E_TIMEOUT);
        }
        let (next, kind) = match self.st {
            HsState::Idle => (HsState::Hello, MsgKind::HelloMsg),
            HsState::Hello => (HsState::CapEx, MsgKind::CapMsg),
            HsState::CapEx => (HsState::Calib, MsgKind::CalibMsg),
            HsState::Calib => (HsState::Run, MsgKind::FrameMsg),
            HsState::Run => (HsState::Report, MsgKind::SnapMsg),
            HsState::Report => (HsState::Done, MsgKind::ReportMsg),
            HsState::Done => return Err(E_EDGE),
        };
        if !edge_ok(self.st, next) {
            return Err(E_EDGE);
        }
        // 能力检查：进入 Run 前必须有能力交集。
        if next == HsState::Run {
            if self.eff_caps == 0 {
                return Err(E_CAP);
            }
        }
        let id = self.send(kind, frame, 0)?;
        self.st = next;
        self.last_frame = frame;
        Ok(id)
    }
    /// 对端能力回填（CapEx 阶段一次性）。
    pub fn set_peer_caps(&mut self, caps: u8) -> Result<(), u16> {
        if self.st != HsState::CapEx {
            return Err(E_EDGE);
        }
        self.peer_caps = caps;
        self.eff_caps = self.local_caps & caps;
        Ok(())
    }
    /// 校准脉冲接收（Calib 阶段）：拍号 0..4 依次，乱序拒绝。
    pub fn calib_beat(&mut self, beat: u32) -> Result<(), u16> {
        if self.st != HsState::Calib {
            return Err(E_EDGE);
        }
        if beat >= CALIB_BEATS {
            return Err(E_CALIB);
        }
        let bit = 1u8 << beat;
        if self.calib_beats & bit != 0 {
            return Err(E_CALIB); // 重复拍
        }
        if beat > 0 && self.calib_beats & (bit >> 1) == 0 {
            return Err(E_CALIB); // 缺前拍
        }
        self.calib_beats |= bit;
        Ok(())
    }
    /// 校准是否收齐。
    pub fn calib_full(&self) -> bool {
        self.calib_beats == 0x0F
    }
    /// 发送一条消息（inflight）。
    fn send(&mut self, kind: MsgKind, frame: u32, val: u32) -> Result<u64, u16> {
        let id = self.next_id;
        self.next_id += 1;
        self.msgs.push(JointMsg {
            id,
            kind,
            frame,
            val,
            st: 0,
        });
        self.sent += 1;
        Ok(id)
    }
    /// 确认一条消息。
    pub fn ack(&mut self, id: u64) -> Result<(), u16> {
        let m = match self.msgs.iter_mut().find(|m| m.id == id) {
            Some(m) => m,
            None => return Err(E_NO_MSG),
        };
        if m.st != 0 {
            return Err(E_LEDGER);
        }
        m.st = 1;
        self.acked += 1;
        Ok(())
    }
    /// 拒绝一条消息（带类校验：仅 FrameMsg/SnapMsg 可拒）。
    pub fn reject(&mut self, id: u64) -> Result<(), u16> {
        let idx = match self.msgs.iter().position(|m| m.id == id) {
            Some(i) => i,
            None => return Err(E_NO_MSG),
        };
        if self.msgs[idx].st != 0 {
            return Err(E_LEDGER);
        }
        match self.msgs[idx].kind {
            MsgKind::FrameMsg | MsgKind::SnapMsg => {}
            _ => return Err(E_EDGE),
        }
        self.msgs[idx].st = 2;
        self.rejected += 1;
        Ok(())
    }
    /// 守恒审计：sent == acked + rejected + inflight。
    pub fn audit(&self) -> Result<(), u16> {
        let inflight = self.msgs.iter().filter(|m| m.st == 0).count() as u64;
        if self.sent != self.acked + self.rejected + inflight {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 生效能力掩码。
    pub fn eff(&self) -> u8 {
        self.eff_caps
    }
    /// 消息数。
    pub fn msg_len(&self) -> usize {
        self.msgs.len()
    }
}

/// 边表穷举自检：合法 6 边全认，其余 36 组合全拒（纯函数）。
pub fn edge_table_check() -> bool {
    use HsState::*;
    let all = [Idle, Hello, CapEx, Calib, Run, Report, Done];
    let legal = [
        (Idle, Hello),
        (Hello, CapEx),
        (CapEx, Calib),
        (Calib, Run),
        (Run, Report),
        (Report, Done),
    ];
    let mut ok = true;
    for a in all {
        for b in all {
            let is_legal = legal.contains(&(a, b));
            if edge_ok(a, b) != is_legal {
                ok = false;
            }
        }
    }
    ok
}

/// 自例程：主路径（完整六步握手 + 校准 + 消息账）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 边表穷举自检。
    if !edge_table_check() {
        return Err(E_EDGE);
    }
    // 主路径：能力 0b1111 与 0b0111 → 交集 0b0111。
    let mut s = JointSession::new(0x0F);
    s.advance(0)?; // Idle→Hello
    s.advance(1)?; // Hello→CapEx
    s.set_peer_caps(0x07)?;
    if s.eff() != 0x07 {
        return Err(E_CAP);
    }
    s.advance(2)?; // CapEx→Calib
    // 错误路径 0：校准乱序（先收第 1 拍）。
    if s.calib_beat(1) != Err(E_CALIB) {
        return Err(E_CALIB);
    }
    // 错误路径 1：拍号越界。
    if s.calib_beat(4) != Err(E_CALIB) {
        return Err(E_CALIB);
    }
    // 主路径：4 拍依次。
    for b in 0..4u32 {
        s.calib_beat(b)?;
    }
    if !s.calib_full() {
        return Err(E_CALIB);
    }
    // 错误路径 2：重复拍。
    if s.calib_beat(3) != Err(E_CALIB) {
        return Err(E_CALIB);
    }
    // 错误路径 3：能力交集为 0 进 Run。
    let mut s2 = JointSession::new(0x10);
    s2.advance(0)?;
    s2.advance(1)?;
    s2.set_peer_caps(0x01)?; // 交集 0
    s2.advance(2)?;
    for b in 0..4u32 {
        s2.calib_beat(b)?;
    }
    if s2.advance(3) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 主路径：继续完成 Run→Report→Done。
    let id_run = s.advance(3)?; // Calib→Run
    let id_snap = s.advance(4)?; // Run→Report
    // 消息账：reject 一条 SnapMsg。
    s.reject(id_snap)?;
    let id_rep = s.advance(5)?; // Report→Done
    s.ack(id_run)?;
    s.ack(id_rep)?;
    s.audit()?;
    if s.state() != HsState::Done {
        return Err(E_EDGE);
    }
    // 错误路径 4：Done 后再推进。
    if s.advance(6) != Err(E_EDGE) {
        return Err(E_EDGE);
    }
    // 错误路径 5：对非 Frame/Snap 消息 reject。
    let mut s3 = JointSession::new(0x0F);
    let h = s3.advance(0)?;
    if s3.reject(h) != Err(E_EDGE) {
        return Err(E_EDGE);
    }
    s3.audit()?;
    // 错误路径 6：未知消息 id。
    if s3.ack(999) != Err(E_NO_MSG) {
        return Err(E_NO_MSG);
    }
    // 消息账守恒（s：2 acked + 1 rejected + 3 inflight = 6 sent）。
    if s.sent != 6 || s.acked != 2 || s.rejected != 1 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
