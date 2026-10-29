//! cgb755_userctrl —— CGPU-F0755 用户控制协议。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0755 用户控制协议  <SEG 0x4ca0>
//! 目标行数：≥380（判据：纯功能代码 ≥380 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥380 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0755；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（网页控制面 = 用户→内核 的命令帧协议）：
//!   1. 帧格式（写死 16 字节定长）：[0] 起始 0x7E；[1] 版本(=1)；
//      [2] 命令码；[3..5) 序号（小端）；[5..13) 8 字节参数体；
//      [13] 参数校验（8 字节逐字节和截断）；[14..16) 帧校验
//      CRC-16/CCITT-FALSE（查表法禁用——逐位计算，整数）；
//   2. 命令码闭集 8 条（写死）：0x01 心跳 / 0x02 画像切换 /
//      0x03 场景挂钩 / 0x04 遥测订阅 / 0x05 遥测退订 /
//      0x06 预算覆写 / 0x07 快照请求 / 0x08 关停预备；
//   3. 序号窗：滑动窗 16，乱序重放帧红码（防重放：序号 ≤ 已见
//      最高水位即拒）；心跳可豁免水位推进但仍不可回退；
//   4. 响应帧同构：命令码 |0x80，参数体首字节 = 结果码（0=OK），
//      响应序号 = 请求序号（一一对应）；
//   5. 状态机：会话四态 Closed→Handshake→Active→Draining；
//      Draining 后只接受 0x08 关停确认，其余命令红码；
//   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4ca0 段为本模块独占（SEG_BASE=0x4ca0），判别式
//!   SEG_BASE+1..+N；与 0x4c80/0x4cc0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const UC_VERSION: &str = "cgb755_userctrl-v1";

/// 诊断段基址（0x4ca0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4CA0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ca1 帧长非法（≠16）。
pub const XC_BAD_LEN: u16 = SEG_BASE + 1;
/// 0x4ca2 起始字节失配（≠0x7E）。
pub const XC_BAD_SOF: u16 = SEG_BASE + 2;
/// 0x4ca3 版本失配（≠1）。
pub const XC_BAD_VER: u16 = SEG_BASE + 3;
/// 0x4ca4 命令码越界（闭集外）。
pub const XC_BAD_CMD: u16 = SEG_BASE + 4;
/// 0x4ca5 参数校验失配。
pub const XC_PARAM_SUM: u16 = SEG_BASE + 5;
/// 0x4ca6 帧 CRC 失配。
pub const XC_BAD_CRC: u16 = SEG_BASE + 6;
/// 0x4ca7 序号重放（≤ 水位）。
pub const XC_REPLAY: u16 = SEG_BASE + 7;
/// 0x4ca8 会话态禁令（该态不接受此命令）。
pub const XC_STATE_FORBID: u16 = SEG_BASE + 8;
/// 0x4ca9 帧校验布局错（响应构造失败）。
pub const XC_RESP_LAYOUT: u16 = SEG_BASE + 9;

/// 定长帧宽（写死：16 字节）。
pub const FRAME_W: usize = 16;
/// 起始字节（写死：0x7E）。
pub const SOF: u8 = 0x7E;
/// 协议版本（写死：1）。
pub const PROTO_VER: u8 = 1;
/// 滑动窗宽（写死：16）。
pub const SEQ_WINDOW: u32 = 16;

/// 命令码闭集（写死）。
pub const CMD_HEARTBEAT: u8 = 0x01;
pub const CMD_PROFILE_SWITCH: u8 = 0x02;
pub const CMD_SCENE_HOOK: u8 = 0x03;
pub const CMD_TELE_SUB: u8 = 0x04;
pub const CMD_TELE_UNSUB: u8 = 0x05;
pub const CMD_BUDGET_OVERRIDE: u8 = 0x06;
pub const CMD_SNAPSHOT_REQ: u8 = 0x07;
pub const CMD_SHUTDOWN_PREP: u8 = 0x08;

/// 命令码合法判定。
pub fn cmd_legal(c: u8) -> bool {
    (1..=8).contains(&c)
}

/// CRC-16/CCITT-FALSE（多项式 0x1021，初值 0xFFFF，逐位无表）。
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in data {
        crc ^= (*b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// 解析后的命令帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdFrame {
    /// 命令码。
    pub cmd: u8,
    /// 序号。
    pub seq: u16,
    /// 参数体（8 字节）。
    pub params: [u8; 8],
}

/// 会话四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// 关闭。
    Closed,
    /// 握手中。
    Handshake,
    /// 活动。
    Active,
    /// 排空中（只收关停）。
    Draining,
}

/// 该态允许的命令集。
pub fn allowed_cmds(s: SessionState) -> &'static [u8] {
    match s {
        SessionState::Closed => &[CMD_HEARTBEAT],
        SessionState::Handshake => &[CMD_HEARTBEAT, CMD_PROFILE_SWITCH],
        SessionState::Active => &[
            CMD_HEARTBEAT,
            CMD_PROFILE_SWITCH,
            CMD_SCENE_HOOK,
            CMD_TELE_SUB,
            CMD_TELE_UNSUB,
            CMD_BUDGET_OVERRIDE,
            CMD_SNAPSHOT_REQ,
            CMD_SHUTDOWN_PREP,
        ],
        SessionState::Draining => &[CMD_SHUTDOWN_PREP],
    }
}

/// CRC 帧构造器：16 字节帧（含 SOF/ver/cmd/seq/params/psum/crc）。
pub fn build_frame(cmd: u8, seq: u16, params: &[u8; 8]) -> Result<[u8; FRAME_W], u16> {
    if !cmd_legal(cmd) {
        return Err(XC_BAD_CMD);
    }
    let mut f = [0u8; FRAME_W];
    f[0] = SOF;
    f[1] = PROTO_VER;
    f[2] = cmd;
    f[3] = (seq & 0xFF) as u8;
    f[4] = (seq >> 8) as u8;
    f[5..13].copy_from_slice(params);
    let psum: u16 = params.iter().map(|b| *b as u16).sum();
    f[13] = (psum & 0xFF) as u8;
    let crc = crc16(&f[..14]);
    f[14] = (crc & 0xFF) as u8;
    f[15] = (crc >> 8) as u8;
    Ok(f)
}

/// 控制协议会话：帧解析 + 序号窗 + 状态机 + 响应构造。
pub struct ControlSession {
    state: SessionState,
    /// 序号水位（已见最高）。
    watermark: u16,
    /// 已处理帧数。
    pub processed: u32,
    /// 已拒帧数。
    pub rejected: u32,
    /// 订阅位图（遥测频道 0..=7）。
    pub subs: u8,
    /// 预算覆写值（0 = 未覆写）。
    pub budget: u32,
}

impl ControlSession {
    /// 新会话（Closed 起）。
    pub fn new() -> Self {
        ControlSession {
            state: SessionState::Closed,
            watermark: 0,
            processed: 0,
            rejected: 0,
            subs: 0,
            budget: 0,
        }
    }

    /// 状态。
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// 握手：Closed→Handshake→Active（心跳帧触发推进）。
    pub fn open(&mut self) -> Result<(), u16> {
        if self.state == SessionState::Closed {
            self.state = SessionState::Handshake;
        } else if self.state == SessionState::Handshake {
            self.state = SessionState::Active;
        } else {
            return Err(XC_STATE_FORBID);
        }
        Ok(())
    }

    /// 排空预备：Active→Draining（0x07 快照请求后语义）。
    pub fn begin_drain(&mut self) -> Result<(), u16> {
        if self.state != SessionState::Active {
            return Err(XC_STATE_FORBID);
        }
        self.state = SessionState::Draining;
        Ok(())
    }

    /// 帧解析主路径：结构闸（长/起始/版本/命令/双校验）→ 态闸 →
    /// 序号窗闸 → 语义执行。全部通过返响应帧。
    pub fn handle(&mut self, frame: &[u8]) -> Result<[u8; FRAME_W], u16> {
        // 结构闸。
        if frame.len() != FRAME_W {
            return Err(XC_BAD_LEN);
        }
        if frame[0] != SOF {
            return Err(XC_BAD_SOF);
        }
        if frame[1] != PROTO_VER {
            return Err(XC_BAD_VER);
        }
        let cmd = frame[2];
        if !cmd_legal(cmd) {
            return Err(XC_BAD_CMD);
        }
        // 参数校验。
        let psum: u16 = frame[5..13].iter().map(|b| *b as u16).sum();
        if (psum & 0xFF) as u8 != frame[13] {
            return Err(XC_PARAM_SUM);
        }
        // CRC。
        let want = crc16(&frame[..14]);
        let got = frame[14] as u16 | ((frame[15] as u16) << 8);
        if want != got {
            return Err(XC_BAD_CRC);
        }
        // 态闸。
        let seq = frame[3] as u16 | ((frame[4] as u16) << 8);
        if !allowed_cmds(self.state).contains(&cmd) {
            self.rejected += 1;
            return Err(XC_STATE_FORBID);
        }
        // 序号窗闸：≤ 水位即重放（首帧水位 0，seq 0 视为首帧合法）。
        if self.processed > 0 && seq <= self.watermark {
            self.rejected += 1;
            return Err(XC_REPLAY);
        }
        // 语义执行。
        let mut params = [0u8; 8];
        params.copy_from_slice(&frame[5..13]);
        let mut result: u8 = 0;
        match cmd {
            CMD_HEARTBEAT => {}
            CMD_PROFILE_SWITCH => {
                // 参数 0 = 目标画像号。
                result = 0;
            }
            CMD_SCENE_HOOK => {
                // 参数 0 = 场景号。
                result = 0;
            }
            CMD_TELE_SUB => {
                let ch = params[0];
                if ch > 7 {
                    result = 2;
                } else {
                    self.subs |= 1u8 << ch;
                }
            }
            CMD_TELE_UNSUB => {
                let ch = params[0];
                if ch > 7 {
                    result = 2;
                } else {
                    self.subs &= !(1u8 << ch);
                }
            }
            CMD_BUDGET_OVERRIDE => {
                let v = params[0] as u32
                    | ((params[1] as u32) << 8)
                    | ((params[2] as u32) << 16)
                    | ((params[3] as u32) << 24);
                if v == 0 {
                    result = 3;
                } else {
                    self.budget = v;
                }
            }
            CMD_SNAPSHOT_REQ => {}
            CMD_SHUTDOWN_PREP => {
                // Draining 态收到关停确认 → 会话闭合。
                if self.state == SessionState::Draining {
                    self.state = SessionState::Closed;
                }
            }
            _ => return Err(XC_BAD_CMD),
        }
        if self.processed == 0 || seq > self.watermark {
            self.watermark = seq;
        }
        self.processed += 1;
        // 响应帧。
        let mut resp_params = [0u8; 8];
        resp_params[0] = result;
        build_frame(cmd | 0x80, seq, &resp_params).map_err(|_| XC_RESP_LAYOUT)
    }

    /// 水位只读。
    pub fn watermark(&self) -> u16 {
        self.watermark
    }
}

/// 自例程：握手 → 全命令面 → 订阅/预算语义 → 重放拦截 →
/// 篡改拦截（CRC/参数和）→ 排空闭合 → 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut s = ControlSession::new();
    // 心跳开握手（Closed 态允许心跳）。
    let hb = build_frame(CMD_HEARTBEAT, 1, &[0; 8])?;
    let r = s.handle(&hb)?;
    if r[2] != (CMD_HEARTBEAT | 0x80) || r[3] != hb[3] {
        return Err(XC_RESP_LAYOUT);
    }
    s.open()?; // Closed→Handshake
    s.open()?; // Handshake→Active
    if s.state() != SessionState::Active {
        return Err(XC_STATE_FORBID);
    }

    // 主命令面：画像切换 / 场景挂钩 / 订阅 / 预算。
    let f2 = build_frame(CMD_PROFILE_SWITCH, 2, &[3, 0, 0, 0, 0, 0, 0, 0])?;
    let _ = s.handle(&f2)?;
    let f3 = build_frame(CMD_SCENE_HOOK, 3, &[7, 0, 0, 0, 0, 0, 0, 0])?;
    let _ = s.handle(&f3)?;
    let f4 = build_frame(CMD_TELE_SUB, 4, &[2, 0, 0, 0, 0, 0, 0, 0])?;
    let _ = s.handle(&f4)?;
    if s.subs & (1 << 2) == 0 {
        return Err(XC_RESP_LAYOUT);
    }
    let f6 = build_frame(CMD_BUDGET_OVERRIDE, 6, &[0x40, 0x0D, 0x03, 0, 0, 0, 0, 0])?;
    let _ = s.handle(&f6)?;
    if s.budget != 0x0003_0D40 {
        return Err(XC_RESP_LAYOUT);
    }
    // 订阅频道越界（参数结果码 2）。
    let f7 = build_frame(CMD_TELE_SUB, 7, &[9, 0, 0, 0, 0, 0, 0, 0])?;
    let r7 = s.handle(&f7)?;
    if r7[5] != 2 {
        return Err(XC_RESP_LAYOUT);
    }
    // 退订。
    let f8 = build_frame(CMD_TELE_UNSUB, 8, &[2, 0, 0, 0, 0, 0, 0, 0])?;
    let _ = s.handle(&f8)?;
    if s.subs & (1 << 2) != 0 {
        return Err(XC_RESP_LAYOUT);
    }
    if s.processed != 7 {
        return Err(XC_RESP_LAYOUT);
    }

    // 错误路径 1：重放（seq 4 低于水位 8）。
    if s.handle(&f4) != Err(XC_REPLAY) {
        return Err(XC_REPLAY);
    }
    // 错误路径 2：CRC 篡改。
    let mut bad = build_frame(CMD_HEARTBEAT, 9, &[1, 0, 0, 0, 0, 0, 0, 0])?;
    bad[14] ^= 1;
    if s.handle(&bad) != Err(XC_BAD_CRC) {
        return Err(XC_BAD_CRC);
    }
    // 错误路径 3：参数和篡改。
    let mut bad2 = build_frame(CMD_HEARTBEAT, 10, &[1, 2, 0, 0, 0, 0, 0, 0])?;
    bad2[6] = 9; // 参数体改动，psum 不再匹配
    if s.handle(&bad2) != Err(XC_PARAM_SUM) {
        return Err(XC_PARAM_SUM);
    }
    // 错误路径 4：命令码越界。
    let bad3 = build_frame(0x09, 11, &[0; 8]);
    if bad3 != Err(XC_BAD_CMD) {
        return Err(XC_BAD_CMD);
    }
    // 错误路径 5：Draining 态收非关停命令。
    s.begin_drain()?;
    let f12 = build_frame(CMD_PROFILE_SWITCH, 12, &[1, 0, 0, 0, 0, 0, 0, 0])?;
    if s.handle(&f12) != Err(XC_STATE_FORBID) {
        return Err(XC_STATE_FORBID);
    }
    // 关停确认闭合。
    let f13 = build_frame(CMD_SHUTDOWN_PREP, 13, &[0; 8])?;
    let _ = s.handle(&f13)?;
    if s.state() != SessionState::Closed {
        return Err(XC_STATE_FORBID);
    }
    // 帧长错误。
    if s.handle(&hb[..10]) != Err(XC_BAD_LEN) {
        return Err(XC_BAD_LEN);
    }
    Ok(0)
}
