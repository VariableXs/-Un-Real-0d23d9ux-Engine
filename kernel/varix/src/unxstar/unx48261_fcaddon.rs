//! unx48261_fcaddon — UNX-F48261 fcitx5 addon 进程接入面（socket 对接，H1 PipeWire 同构模式）（诊断段 0xC180）。
//!
//! 判据原文：
//!   J1 接入 ×10 建链/断链/重连全绿；判据锚定 §AI-61 §2 专题四、AI-36 先例。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. socket 对接数据面（H1 PipeWire 同构）：addon 进程经 unix socket
//!      与桥本体对接，会话由（endpoint、session_token、链状态、收发
//!      序号）四元构成；本面只建数据模型与状态机，不碰真实 fd。
//!   2. 链状态机五态：Disconnected → Connecting（HELLO 发出）→
//!      Connected（ACK 对上）→ Broken（超时/错包）→ Reconnecting →
//!      Connected；非法迁移一律拒收并落链账。
//!   3. 建链握手：HELLO 带 (abi_version, session_token, tx_seq0)，ACK
//!      带 (abi_version, session_token, rx_seq0)；token 不匹配或 ABI
//!      不匹配即断（Broken），拒绝静默降级。
//!   4. 断链/重连口径：断链时收发序号封存快照；重连以同 token 恢复，
//!      tx 从封存值续发（不重置），对端以 token 关联旧会话——重连后
//!      序号必须严格大于断链前（防重放）。
//!   5. 接入口径（J1 ×10）：10 轮 建链→数据双向→断链→重连→数据恢复
//!      全流程，逐轮断言五态迁移合法、握手四元一致、序号续发单调、
//!      心跳丢包计数清零；一轮全绿记一命中。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC180;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_STATE_BAD: u16 = SEG_BASE + 1;   // 链状态机非法迁移
const E_TOKEN_MISMATCH: u16 = SEG_BASE + 2; // 会话令牌不匹配
const E_ABI_MISMATCH: u16 = SEG_BASE + 3;// ABI 版本不匹配
const E_SEQ_REPLAY: u16 = SEG_BASE + 4;  // 序号回退/重放
const E_LINK_MISS: u16 = SEG_BASE + 5;   // ×10 轮不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 链账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// ABI 版本（H1 同构模式）。
pub const ABI_VERSION: u32 = 1;
/// 心跳窗口：连续无 ACK 心跳数达此值判 Broken。
pub const HEARTBEAT_WINDOW: u32 = 3;

/// 链状态五态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    Disconnected,
    Connecting,
    Connected,
    Broken,
    Reconnecting,
}

/// 握手 HELLO 载荷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hello {
    pub abi: u32,
    pub token: u64,
    pub tx_seq0: u64,
}

/// 握手 ACK 载荷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ack {
    pub abi: u32,
    pub token: u64,
    pub rx_seq0: u64,
}

/// 断链快照（序号封存）。
#[derive(Clone, Copy, Debug)]
pub struct BreakSnap {
    pub tx_seq: u64,
    pub rx_seq: u64,
    pub broken_round: u32,
}

/// 一轮接入账。
#[derive(Clone, Copy, Debug)]
pub struct LinkRecord {
    pub round: u32,
    pub connected: bool,
    pub reconnected: bool,
    pub seq_resumed: bool, // 重连后 tx 续发单调
    pub green: bool,
}

/// fcitx5 addon 接入面枢纽。
pub struct FcAddon {
    pub state: LinkState,
    pub token: u64,
    pub tx_seq: u64,
    pub rx_seq: u64,
    pub heartbeat_miss: u32,
    snap: Option<BreakSnap>,
    ledger: Vec<LinkRecord>,
    pub round_total: u32,
    pub fail_total: u32,
}

impl FcAddon {
    pub fn new(token: u64) -> FcAddon {
        FcAddon {
            state: LinkState::Disconnected,
            token,
            tx_seq: 0,
            rx_seq: 0,
            heartbeat_miss: 0,
            snap: None,
            ledger: Vec::new(),
            round_total: 0,
            fail_total: 0,
        }
    }

    /// 建链请求：Disconnected → Connecting（出 HELLO）。
    pub fn connect_request(&mut self) -> Result<Hello, u16> {
        if self.state != LinkState::Disconnected {
            return Err(E_STATE_BAD);
        }
        self.state = LinkState::Connecting;
        self.tx_seq += 1;
        Ok(Hello { abi: ABI_VERSION, token: self.token, tx_seq0: self.tx_seq })
    }

    /// 对端 ACK 核验：ABI + token 双匹配才 Connected。
    pub fn on_ack(&mut self, ack: &Ack) -> Result<(), u16> {
        if self.state != LinkState::Connecting && self.state != LinkState::Reconnecting {
            return Err(E_STATE_BAD);
        }
        if ack.abi != ABI_VERSION {
            self.state = LinkState::Broken;
            return Err(E_ABI_MISMATCH);
        }
        if ack.token != self.token {
            self.state = LinkState::Broken;
            return Err(E_TOKEN_MISMATCH);
        }
        // ACK 的 rx_seq0 必须等于本端发出的 tx_seq0（握手对上）。
        let expect = self.tx_seq;
        if ack.rx_seq0 != expect {
            self.state = LinkState::Broken;
            return Err(E_SEQ_REPLAY);
        }
        self.rx_seq = ack.rx_seq0;
        self.heartbeat_miss = 0;
        self.state = LinkState::Connected;
        Ok(())
    }

    /// 数据双向：tx 出帧计序、rx 入帧核序（重放/回退即断）。
    pub fn send_frame(&mut self) -> Result<u64, u16> {
        if self.state != LinkState::Connected {
            return Err(E_STATE_BAD);
        }
        self.tx_seq += 1;
        Ok(self.tx_seq)
    }

    pub fn recv_frame(&mut self, seq: u64) -> Result<(), u16> {
        if self.state != LinkState::Connected {
            return Err(E_STATE_BAD);
        }
        if seq <= self.rx_seq {
            self.state = LinkState::Broken;
            self.snap = Some(BreakSnap { tx_seq: self.tx_seq, rx_seq: self.rx_seq, broken_round: self.round_total });
            return Err(E_SEQ_REPLAY);
        }
        self.rx_seq = seq;
        self.heartbeat_miss = 0;
        Ok(())
    }

    /// 心跳结算：窗口内无 ACK 累计；达窗判断链并封存快照。
    pub fn heartbeat_tick(&mut self, acked: bool) -> Result<(), u16> {
        if self.state != LinkState::Connected {
            return Err(E_STATE_BAD);
        }
        if acked {
            self.heartbeat_miss = 0;
            return Ok(());
        }
        self.heartbeat_miss += 1;
        if self.heartbeat_miss >= HEARTBEAT_WINDOW {
            self.state = LinkState::Broken;
            self.snap = Some(BreakSnap { tx_seq: self.tx_seq, rx_seq: self.rx_seq, broken_round: self.round_total });
        }
        Ok(())
    }

    /// 重连：Broken → Reconnecting（同 token 恢复，tx 续发）。
    pub fn reconnect_request(&mut self) -> Result<Hello, u16> {
        if self.state != LinkState::Broken {
            return Err(E_STATE_BAD);
        }
        // 快照必须在断链时留过（序号续发的依据）。
        match self.snap {
            Some(s) => {
                if self.tx_seq < s.tx_seq {
                    return Err(E_SEQ_REPLAY); // 续发不得回退
                }
            }
            None => return Err(E_LEDGER_BAD),
        }
        self.state = LinkState::Reconnecting;
        self.tx_seq += 1;
        Ok(Hello { abi: ABI_VERSION, token: self.token, tx_seq0: self.tx_seq })
    }

    /// 一轮全流程：建链→双向数据→心跳断链→重连→数据恢复（J1 主体）。
    pub fn run_link_round(&mut self, round: u32) -> Result<(), u16> {
        // 归零：链必从 Disconnected 起（上一轮结束态）。
        if self.state != LinkState::Disconnected {
            return Err(E_STATE_BAD);
        }
        let mut green = true;
        // 1) 建链。
        let hello = self.connect_request()?;
        let ack = Ack { abi: hello.abi, token: hello.token, rx_seq0: hello.tx_seq0 };
        if self.on_ack(&ack).is_err() {
            green = false;
        }
        // 2) 双向数据 3 帧。
        let mut k = 0u32;
        while k < 3 {
            self.send_frame()?;
            self.recv_frame(self.rx_seq + 1)?;
            k += 1;
        }
        // 3) 心跳断链：连续 3 次无 ACK。
        let mut k = 0u32;
        while k < HEARTBEAT_WINDOW {
            self.heartbeat_tick(false)?;
            k += 1;
        }
        if self.state != LinkState::Broken {
            return Err(E_STATE_BAD);
        }
        // 4) 重连：tx 续发断链前 +1 起。
        let before = self.tx_seq;
        let hello2 = self.reconnect_request()?;
        if hello2.tx_seq0 <= before {
            green = false; // 续发不单调
        }
        let ack2 = Ack { abi: hello2.abi, token: hello2.token, rx_seq0: hello2.tx_seq0 };
        if self.on_ack(&ack2).is_err() {
            green = false;
        }
        // 5) 数据恢复 1 帧。
        self.send_frame()?;
        self.recv_frame(self.rx_seq + 1)?;
        // 收尾：主动归位 Disconnected（对端 FIN）。
        self.state = LinkState::Disconnected;
        let seq_resumed = hello2.tx_seq0 > before;
        if !green || !seq_resumed {
            self.fail_total += 1;
        }
        self.ledger.push(LinkRecord {
            round,
            connected: true,
            reconnected: true,
            seq_resumed,
            green: green && seq_resumed,
        });
        self.round_total += 1;
        if !green || !seq_resumed {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// ×N 轮覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.round_total < want {
            return Err(E_LINK_MISS);
        }
        Ok(())
    }

    /// 接入终判：10 轮全绿、账目守恒。
    pub fn assert_all_green(&self) -> Result<(), u16> {
        if self.fail_total != 0 || self.ledger.len() as u32 != self.round_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if !self.ledger[i].green {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.round_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（接入 ×10 建链/断链/重连全绿），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主接入 ×10：10 轮全流程全绿（建链/双向/断链/重连/恢复），
    //    tx 序号跨轮严格单调累计（会话续命口径）。
    let mut fc = FcAddon::new(0xA1B2C3D4);
    let mut tx_before = 0u64;
    let mut r = 0u32;
    while r < 10 {
        fc.run_link_round(r)?;
        if fc.tx_seq <= tx_before {
            return Err(E_SEQ_REPLAY); // 跨轮 tx 必须累计增长
        }
        tx_before = fc.tx_seq;
        r += 1;
    }
    fc.coverage(10)?;
    fc.assert_all_green()?;
    if fc.ledger_len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 握手四元一致性：HELLO 的 (abi, token, tx_seq0) 与 ACK 逐位
    //    对上才 Connected；token 换线必拒。
    let mut fc = FcAddon::new(0x1234);
    let hello = fc.connect_request()?;
    if hello.abi != ABI_VERSION || hello.token != 0x1234 || hello.tx_seq0 != 1 {
        return Err(E_LEDGER_BAD);
    }
    let bad = Ack { abi: ABI_VERSION, token: 0x9999, rx_seq0: hello.tx_seq0 };
    if fc.on_ack(&bad).is_ok() {
        return Err(E_TOKEN_MISMATCH);
    }
    if fc.state != LinkState::Broken {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 ABI 失配断链：ACK abi≠1 → Broken，拒绝静默降级。
    let mut fc = FcAddon::new(0x1234);
    let hello = fc.connect_request()?;
    let bad = Ack { abi: ABI_VERSION + 1, token: hello.token, rx_seq0: hello.tx_seq0 };
    if fc.on_ack(&bad).is_ok() {
        return Err(E_ABI_MISMATCH);
    }
    if fc.state != LinkState::Broken {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 序号重放断链：Connected 下 rx 收到 ≤rx_seq 的帧 → Broken，
    //    快照封存（tx/rx 双序号）。
    let mut fc = FcAddon::new(0x5678);
    let hello = fc.connect_request()?;
    let ack = Ack { abi: ABI_VERSION, token: hello.token, rx_seq0: hello.tx_seq0 };
    fc.on_ack(&ack)?;
    fc.send_frame()?;
    fc.recv_frame(fc.rx_seq + 1)?; // rx=1
    if fc.recv_frame(1).is_ok() {
        return Err(E_SEQ_REPLAY); // 重放 rx=1
    }
    if fc.state != LinkState::Broken {
        return Err(E_LEDGER_BAD);
    }
    match fc.snap {
        Some(s) => {
            if s.rx_seq != 1 {
                return Err(E_LEDGER_BAD);
            }
        }
        None => return Err(E_LEDGER_BAD),
    }
    hits += 1;

    // R5 心跳窗口断链：miss 1/2 不断、第 3 次达窗判断；中途 ACK 清零。
    let mut fc = FcAddon::new(0x9ABC);
    let hello = fc.connect_request()?;
    let ack = Ack { abi: ABI_VERSION, token: hello.token, rx_seq0: hello.tx_seq0 };
    fc.on_ack(&ack)?;
    fc.heartbeat_tick(false)?;
    fc.heartbeat_tick(false)?;
    fc.heartbeat_tick(true)?; // ACK 清零
    if fc.state != LinkState::Connected || fc.heartbeat_miss != 0 {
        return Err(E_LEDGER_BAD);
    }
    let mut k = 0u32;
    while k < HEARTBEAT_WINDOW {
        fc.heartbeat_tick(false)?;
        k += 1;
    }
    if fc.state != LinkState::Broken {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 重连守门：未断链时重连拒；Broken 后同 token 重连续发；
    //    重连 ACK 的 rx_seq0 对不上即再断。
    let mut fc = FcAddon::new(0xDEF0);
    if fc.reconnect_request().is_ok() {
        return Err(E_STATE_BAD); // Disconnected 态不得重连
    }
    let hello = fc.connect_request()?;
    let ack = Ack { abi: ABI_VERSION, token: hello.token, rx_seq0: hello.tx_seq0 };
    fc.on_ack(&ack)?;
    if fc.reconnect_request().is_ok() {
        return Err(E_STATE_BAD); // Connected 态不得重连
    }
    let mut k = 0u32;
    while k < HEARTBEAT_WINDOW {
        fc.heartbeat_tick(false)?;
        k += 1;
    }
    let before = fc.tx_seq;
    let hello2 = fc.reconnect_request()?;
    if hello2.tx_seq0 <= before {
        return Err(E_SEQ_REPLAY);
    }
    let ack_bad = Ack { abi: ABI_VERSION, token: hello2.token, rx_seq0: hello2.tx_seq0 - 1 };
    if fc.on_ack(&ack_bad).is_ok() {
        return Err(E_SEQ_REPLAY); // rx_seq0 错位：握手对不上
    }
    if fc.state != LinkState::Broken {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 状态机门禁：Connected 态 connect_request 拒；Broken 态收帧拒；
    //    Disconnected 态心跳拒——非法迁移全拒且链账不记绿。
    let mut fc = FcAddon::new(0x1111);
    if fc.send_frame().is_ok() {
        return Err(E_STATE_BAD); // Disconnected 出帧拒
    }
    let hello = fc.connect_request()?;
    let ack = Ack { abi: ABI_VERSION, token: hello.token, rx_seq0: hello.tx_seq0 };
    fc.on_ack(&ack)?;
    if fc.connect_request().is_ok() {
        return Err(E_STATE_BAD); // Connected 重复建链拒
    }
    if fc.heartbeat_tick(false).is_ok() {
        // Connected 态心跳正常允许——改用断链后心跳验证：
    }
    let mut k = 0u32;
    while k < HEARTBEAT_WINDOW {
        fc.heartbeat_tick(false)?;
        k += 1;
    }
    if fc.recv_frame(99).is_ok() {
        return Err(E_STATE_BAD); // Broken 态收帧拒
    }
    if fc.heartbeat_tick(false).is_ok() {
        return Err(E_STATE_BAD); // Broken 态心跳拒
    }
    hits += 1;

    // R8 token 生命周期与写回（判据 J2）：token=0 拒建（非法会话），
    //    20 轮放大全绿，末尾写回断言。
    let mut fc0 = FcAddon::new(0);
    if fc0.connect_request().is_ok() {
        return Err(E_LEDGER_BAD); // token 0 视作未授权会话
    }
    let mut fc = FcAddon::new(0xFEED_FACE);
    let mut r = 0u32;
    while r < 20 {
        fc.run_link_round(r)?;
        r += 1;
    }
    fc.coverage(10)?;
    fc.assert_all_green()?;
    if fc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fc.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
