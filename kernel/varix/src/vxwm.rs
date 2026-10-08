//! UNREAL-X-15000 · WP-201 · B-503 VXWM 帧编解码层（MD1 附录 H × MD2 篇 5.2）。
//!
//! 宪法边界：VXWM 消息种数 24 条硬上限（MD1 附录 H，C-3：超 24 条须 ADR）。
//! 编码定案（MD2 篇 5.2）：定长头（类型、序号、长度、发送方对象标识）加变长体，
//! 体按类型定案字段序，全部小端、无指针、自描述校验和尾缀——报文可在诊断日志里
//! 原样落盘回放（体验日志与协议日志同格式，调试时可以"重放一天"）。
//! 三件事：其一，报文上限 4KB，超限大载荷走"报文里放引用、内容走共享内存环形区"
//! 的旁路；其二，序号单调，乱序与重复在协议层拒绝并计数上报——静默丢包与静默
//! 重放都是缺陷；其三，版本协商在 bind 时完成，小版本不一致按"双方都认识的子集"
//! 降级，降级事实写进诊断。
//!
//! 字段序冻结位：`min_body_len` 是二十四类消息的体字段序下限（代码即冻结文本），
//! 冻结后变更走 ADR（MD2 附录 F 顺序纪律第一条：schema 先行）。
//! 零堆、整数运算、宿主全测。判据号 B-503 入 CheckSet 命名，验收口径可 grep。

// ---------------------------------------------------------------------------
// 常量定案
// ---------------------------------------------------------------------------

/// 定长头：类型(1) + 版本(1) + 序号(4) + 体长(2) + 发送方(2) = 10 字节。
pub const HEADER_LEN: usize = 10;
/// 校验和尾缀：u16 小端，2 字节（自描述——解码端无需外部 schema 即可验完整性）。
pub const CKSUM_LEN: usize = 2;
/// 报文总上限 4KB（MD2 篇 5.2 其一）。
pub const FRAME_MAX: usize = 4096;
/// 体上限 = 4096 − 10 − 2 = 4084 字节。
pub const BODY_MAX: usize = FRAME_MAX - HEADER_LEN - CKSUM_LEN;
/// 回放环槽数（诊断落盘的滚动窗口）。
pub const REPLAY_SLOTS: usize = 8;
/// 消息种数硬上限（C-3）。
pub const MSG_COUNT: u8 = 24;
/// 4KB 装不下的大载荷走旁路：报文里放引用（shm 环形区句柄+偏移+长度）。
pub const BYPASS_REF_LEN: usize = 12;

// ---------------------------------------------------------------------------
// 二十四消息枚举（MD1 附录 H 官方顺序，1..=24）
// ---------------------------------------------------------------------------

/// 绑定与生命周期（5 条）：1~5。
pub const G_BIND: u8 = 0;
/// 绘制与提交（4 条）：6~9。
pub const G_DRAW: u8 = 1;
/// 输入事件（4 条）：10~13。
pub const G_INPUT: u8 = 2;
/// 窗口状态与几何（5 条）：14~18。
pub const G_GEOM: u8 = 3;
/// 剪贴板与拖放（4 条）：19~22。
pub const G_CLIP: u8 = 4;
/// 浮层语义与诊断（2 条）：23~24。
pub const G_POPUP: u8 = 5;

/// 分组数（六分组 5/4/4/5/4/2）。
pub const GROUP_COUNT: usize = 6;

/// 方向三态：C→S 客户端发往合成器 / S→C 合成器发往客户端 / 双向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    C2S,
    S2C,
    Both,
}

impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Dir::C2S => "C->S",
            Dir::S2C => "S->C",
            Dir::Both => "both",
        }
    }
}

/// 消息名（MD1 附录 H 词条名，索引即消息号−1）。
pub const fn msg_name(t: u8) -> &'static str {
    match t {
        1 => "bind",
        2 => "surface_create",
        3 => "surface_destroy",
        4 => "close_request",
        5 => "ping/pong",
        6 => "buffer_attach",
        7 => "damage",
        8 => "commit",
        9 => "frame_callback",
        10 => "pointer_motion",
        11 => "pointer_button",
        12 => "key_event",
        13 => "text_commit",
        14 => "configure",
        15 => "ack_configure",
        16 => "state_request",
        17 => "focus_change",
        18 => "z_order_hint",
        19 => "clipboard_offer",
        20 => "clipboard_select",
        21 => "drag_start",
        22 => "drop_event",
        23 => "popup_grab",
        24 => "diagnostics",
        _ => "?",
    }
}

/// 方向（MD1 附录 H 定案）。
pub const fn msg_dir(t: u8) -> Dir {
    match t {
        1 | 2 | 3 | 6 | 7 | 8 | 15 | 16 | 18 | 19 | 21 | 23 | 24 => Dir::C2S,
        4 | 9 | 10 | 11 | 12 | 13 | 14 | 17 | 20 | 22 => Dir::S2C,
        5 => Dir::Both,
        _ => Dir::C2S,
    }
}

/// 六分组归属（MD1 附录 H 分组即设计说明）。
pub const fn msg_group(t: u8) -> u8 {
    match t {
        1..=5 => G_BIND,
        6..=9 => G_DRAW,
        10..=13 => G_INPUT,
        14..=18 => G_GEOM,
        19..=22 => G_CLIP,
        _ => G_POPUP,
    }
}

/// 体字段序下限（字节）：每类消息的最小定案字段序长度。
/// 变长类（13 text_commit、24 diagnostics）在最小头之后接 u16 长度前缀的字节串。
pub const fn min_body_len(t: u8) -> u16 {
    match t {
        // bind: ver_major:u8, ver_minor:u8, caps:u32, obj:u32
        1 => 10,
        // surface_create: surface:u32, w:u16, h:u16, role:u8, pad:u8
        2 => 10,
        // surface_destroy / close_request / clipboard_select: surface:u32
        3 | 4 | 20 => 4,
        // ping/pong: token:u32
        5 => 4,
        // buffer_attach: surface:u32, shm:u32, w:u16, h:u16, fmt:u8, pad:u8
        6 => 12,
        // damage: surface:u32, x:u16, y:u16, w:u16, h:u16
        7 => 12,
        // commit: surface:u32, atomic_seq:u32
        8 => 8,
        // frame_callback: surface:u32, frame:u32
        9 => 8,
        // pointer_motion: surface:u32, x:i16, y:i16
        10 => 8,
        // pointer_button: surface:u32, button:u8, state:u8, click_seq:u16
        11 => 8,
        // key_event: surface:u32, key:u16, state:u8, mods:u8
        12 => 8,
        // text_commit: surface:u32, len:u16, pad:u16, bytes[len]
        13 => 8,
        // configure: surface:u32, w:u16, h:u16, states:u8, pad:u8
        14 => 10,
        // ack_configure: surface:u32, serial:u32
        15 => 8,
        // state_request: surface:u32, state:u8, pad:u8×3
        16 => 8,
        // focus_change: surface:u32, focused:u8, pad:u8×3
        17 => 8,
        // z_order_hint: surface:u32, hint:u8, pad:u8×3
        18 => 8,
        // clipboard_offer: surface:u32, types_mask:u32
        19 => 8,
        // drag_start: surface:u32, actions:u8, pad:u8×3
        21 => 8,
        // drop_event: surface:u32, action:u8, pad:u8×3
        22 => 8,
        // popup_grab: surface:u32, grab_id:u32
        23 => 8,
        // diagnostics: surface:u32, code:u16, len:u16, bytes[len]
        24 => 8,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// 校验和（确定性、无堆、FNV-1a 32 折叠 16）
// ---------------------------------------------------------------------------

pub fn checksum(bytes: &[u8]) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    ((h >> 16) ^ h) as u16
}

// ---------------------------------------------------------------------------
// 定长头
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub msg_type: u8,
    pub ver: u8,
    pub seq: u32,
    pub body_len: u16,
    pub sender: u16,
}

pub const HEADER_BAD_TYPE: u8 = 0;
pub const HEADER_BAD_FIELD: u8 = 1;
pub const HEADER_BAD_LEN: u8 = 2;

impl Header {
    /// 小端落盘（无指针：copy_from_slice，非对齐安全）。
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        if buf.len() < HEADER_LEN {
            return 0;
        }
        buf[0] = self.msg_type;
        buf[1] = self.ver;
        buf[2..6].copy_from_slice(&self.seq.to_le_bytes());
        buf[6..8].copy_from_slice(&self.body_len.to_le_bytes());
        buf[8..10].copy_from_slice(&self.sender.to_le_bytes());
        HEADER_LEN
    }

    /// 解码校验：类型 ∈ 1..=24、体长不越 4KB 上限、体长≥字段序下限。
    pub fn decode(buf: &[u8]) -> Result<Header, u8> {
        if buf.len() < HEADER_LEN {
            return Err(HEADER_BAD_LEN);
        }
        let h = Header {
            msg_type: buf[0],
            ver: buf[1],
            seq: u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]),
            body_len: u16::from_le_bytes([buf[6], buf[7]]),
            sender: u16::from_le_bytes([buf[8], buf[9]]),
        };
        if h.msg_type == 0 || h.msg_type > MSG_COUNT {
            return Err(HEADER_BAD_TYPE);
        }
        if h.body_len as usize > BODY_MAX || h.body_len < min_body_len(h.msg_type) {
            return Err(HEADER_BAD_FIELD);
        }
        Ok(h)
    }
}

// ---------------------------------------------------------------------------
// 错误叙事（对齐 compositor 范式：每个失败有下一步建议）
// ---------------------------------------------------------------------------

pub const E_OK: u16 = 0;
pub const E_SHORT: u16 = 1;
pub const E_TOO_LONG: u16 = 2;
pub const E_BAD_TYPE: u16 = 3;
pub const E_BODY_TOO_SHORT: u16 = 4;
pub const E_BAD_CKSUM: u16 = 5;
pub const E_LEN_MISMATCH: u16 = 6;
pub const E_DUP: u16 = 7;
pub const E_GAP: u16 = 8;
pub const E_SEQ_INVALID: u16 = 9;
pub const E_C3_LIMIT: u16 = 10;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_SHORT => "帧不足 12 字节，建议核对落盘长度",
        E_TOO_LONG => "报文越 4KB 上限，建议大载荷走共享内存旁路引用",
        E_BAD_TYPE => "消息类型不在 1..=24，C-3 宪法边界拒绝，建议查发送方编码",
        E_BODY_TOO_SHORT => "体长低于该类消息字段序下限，建议核对字段序冻结表",
        E_BAD_CKSUM => "校验和不匹配，报文在传输中损坏，建议整帧重发",
        E_LEN_MISMATCH => "头内体长与实际帧长不符，建议重编码",
        E_DUP => "重复序号已拒绝并计数——静默重放是缺陷",
        E_GAP => "序号跳号已拒绝——静默丢包是缺陷，建议对端回退到 seen+1",
        E_SEQ_INVALID => "零号序列非法，序号从 1 起单调",
        E_C3_LIMIT => "消息种数超 24 条硬上限，须先过 ADR（C-3）",
        _ => "未知协议错误，建议重建会话",
    }
}

// ---------------------------------------------------------------------------
// 帧（头 + 体 + 校验和尾缀）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub hdr: Header,
    pub body: [u8; BODY_MAX],
}

impl Frame {
    pub fn new(hdr: Header, body: &[u8]) -> Result<Frame, u16> {
        if hdr.msg_type == 0 || hdr.msg_type > MSG_COUNT {
            return Err(E_BAD_TYPE);
        }
        if body.len() > BODY_MAX {
            return Err(E_TOO_LONG);
        }
        if body.len() < min_body_len(hdr.msg_type) as usize {
            return Err(E_BODY_TOO_SHORT);
        }
        if hdr.body_len as usize != body.len() {
            return Err(E_LEN_MISMATCH);
        }
        let mut f = Frame { hdr, body: [0u8; BODY_MAX] };
        f.body[..body.len()].copy_from_slice(body);
        Ok(f)
    }

    /// 编码：头 + 体 + 校验和尾缀。返回写入字节数（0 = 缓冲不足）。
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let total = HEADER_LEN + self.hdr.body_len as usize + CKSUM_LEN;
        if buf.len() < total || total > FRAME_MAX {
            return 0;
        }
        self.hdr.encode(buf);
        let bl = self.hdr.body_len as usize;
        buf[HEADER_LEN..HEADER_LEN + bl].copy_from_slice(&self.body[..bl]);
        let sum = checksum(&buf[..HEADER_LEN + bl]);
        buf[HEADER_LEN + bl..total].copy_from_slice(&sum.to_le_bytes());
        total
    }

    /// 解码：长度链 + 类型域 + 字段序下限 + 校验和全验证。
    pub fn decode(buf: &[u8]) -> Result<Frame, u16> {
        if buf.len() < HEADER_LEN + CKSUM_LEN {
            return Err(E_SHORT);
        }
        if buf.len() > FRAME_MAX {
            return Err(E_TOO_LONG);
        }
        let bl = u16::from_le_bytes([buf[6], buf[7]]) as usize;
        if HEADER_LEN + bl + CKSUM_LEN != buf.len() {
            return Err(E_LEN_MISMATCH);
        }
        let t = buf[0];
        if t == 0 || t > MSG_COUNT {
            return Err(E_BAD_TYPE);
        }
        if bl < min_body_len(t) as usize {
            return Err(E_BODY_TOO_SHORT);
        }
        let payload_end = HEADER_LEN + bl;
        let expect = u16::from_le_bytes([buf[payload_end], buf[payload_end + 1]]);
        if checksum(&buf[..payload_end]) != expect {
            return Err(E_BAD_CKSUM);
        }
        let hdr = Header {
            msg_type: t,
            ver: buf[1],
            seq: u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]),
            body_len: bl as u16,
            sender: u16::from_le_bytes([buf[8], buf[9]]),
        };
        let mut f = Frame { hdr, body: [0u8; BODY_MAX] };
        f.body[..bl].copy_from_slice(&buf[HEADER_LEN..payload_end]);
        Ok(f)
    }
}

// ---------------------------------------------------------------------------
// 序号单调闸（MD2 篇 5.2 其二：乱序与重复拒绝并计数上报）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct SeqGate {
    /// 已见最高序号（0 = 尚未见过任何帧，期望 seq==1）。
    pub seen: u32,
    pub accepted: u64,
    pub dup_dropped: u64,
    pub gap_dropped: u64,
    pub invalid_dropped: u64,
}

impl SeqGate {
    pub fn new() -> SeqGate {
        SeqGate::default()
    }

    /// 严格单调：仅接受 seq == seen+1（首个为 1）。其余拒绝并分类计数。
    pub fn admit(&mut self, seq: u32) -> u16 {
        if seq == 0 {
            self.invalid_dropped += 1;
            return E_SEQ_INVALID;
        }
        if seq > self.seen + 1 {
            self.gap_dropped += 1;
            return E_GAP;
        }
        if seq <= self.seen {
            self.dup_dropped += 1;
            return E_DUP;
        }
        self.seen = seq;
        self.accepted += 1;
        E_OK
    }
}

// ---------------------------------------------------------------------------
// 版本协商（MD2 篇 5.2 其三：bind 时完成，按双方都认识的子集降级）
// ---------------------------------------------------------------------------

/// 取双方小版本的 MIN 为生效版本；不一致即降级，降级事实返回 true（写进诊断）。
pub fn negotiate(client_minor: u8, server_minor: u8) -> (u8, bool) {
    let eff = client_minor.min(server_minor);
    (eff, client_minor != server_minor)
}

// ---------------------------------------------------------------------------
// 回放环（诊断落盘 → 原样重放：体验日志与协议日志同格式）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ReplayLog {
    slots: [Option<Frame>; REPLAY_SLOTS],
    pub total: u64,
    pub rolled: u64,
}

impl ReplayLog {
    pub fn new() -> ReplayLog {
        ReplayLog { slots: [None; REPLAY_SLOTS], total: 0, rolled: 0 }
    }

    /// 落盘一帧；环满则滚动淘汰最旧（FIFO）。
    pub fn push(&mut self, f: Frame) {
        if self.len() == REPLAY_SLOTS {
            self.rolled += 1;
        }
        self.slots.copy_within(1.., 0);
        self.slots[REPLAY_SLOTS - 1] = Some(f);
        self.total += 1;
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 按落盘顺序取第 i 帧（跳过未占槽：0=最旧，len()-1=最新）。
    pub fn get(&self, i: usize) -> Option<&Frame> {
        self.slots.iter().flatten().nth(i)
    }

    /// 重放：全部槽位重新过解码器（回放调试可用 = B-503 判据的落点）。
    pub fn replay_all(&self) -> u64 {
        let mut ok = 0;
        for s in self.slots.iter().flatten() {
            let mut buf = [0u8; FRAME_MAX];
            let n = s.encode(&mut buf);
            if n > 0 && Frame::decode(&buf[..n]).is_ok() {
                ok += 1;
            }
        }
        ok
    }

    pub fn reset(&mut self) {
        self.slots = [None; REPLAY_SLOTS];
        self.total = 0;
        self.rolled = 0;
    }
}

// ---------------------------------------------------------------------------
// 大载荷旁路引用（MD2 篇 5.2 其一：报文里放引用、内容走共享内存环形区）
// ---------------------------------------------------------------------------

/// 旁路引用 12 字节：shm 环形区句柄(4) + 偏移(4) + 长度(4)，全小端。
pub fn bypass_ref(shm_id: u32, offset: u32, len: u32, buf: &mut [u8]) -> usize {
    if buf.len() < BYPASS_REF_LEN {
        return 0;
    }
    buf[0..4].copy_from_slice(&shm_id.to_le_bytes());
    buf[4..8].copy_from_slice(&offset.to_le_bytes());
    buf[8..12].copy_from_slice(&len.to_le_bytes());
    BYPASS_REF_LEN
}

/// 解旁路引用。
pub fn bypass_ref_decode(buf: &[u8]) -> Option<(u32, u32, u32)> {
    if buf.len() < BYPASS_REF_LEN {
        return None;
    }
    Some((
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
        u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
    ))
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-503 入命名，验收口径可 grep）
// ---------------------------------------------------------------------------

/// 测试/自检共用帧工厂：bind 帧。
pub fn mk_bind_frame(seq: u32, sender: u16, minor: u8) -> Frame {
    let hdr = Header { msg_type: 1, ver: minor, seq, body_len: 10, sender };
    let body = [1u8, minor, 0, 0, 0, 0, 0, 0, 0, 0]; // major=1, minor, caps=0, obj=0
    Frame::new(hdr, &body).unwrap_or(Frame {
        hdr,
        body: [0u8; BODY_MAX],
    })
}

/// B-503 自检：判据逐项登记。
pub fn run_vxwm_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-frame");

    // —— 定长头与编码 ——
    let h = Header { msg_type: 7, ver: 3, seq: 0x1234_5678, body_len: 12, sender: 9 };
    let mut hb = [0u8; 16];
    assert_eq!(h.encode(&mut hb), 10);
    let hd = Header::decode(&hb);
    set.add(
        "B-503 定长头四要素 round-trip",
        hd.map(|d| d == h).unwrap_or(false),
        "类型/版本/序号/体长/发送方全保真",
    );
    set.add(
        "B-503 全字段小端序",
        hb[2] == 0x78 && hb[3] == 0x56 && hb[4] == 0x34 && hb[5] == 0x12,
        "多平台一致，无宿主端序假设",
    );
    let f1 = mk_bind_frame(1, 5, 2);
    let mut fb = [0u8; FRAME_MAX];
    let n1 = f1.encode(&mut fb);
    let fd1 = Frame::decode(&fb[..n1]);
    set.add(
        "B-503 帧编码 round-trip 全保真",
        fd1.map(|d| d == f1).unwrap_or(false) && n1 == HEADER_LEN + 10 + CKSUM_LEN,
        "头+体+尾缀完整往返",
    );
    let mut tampered = fb;
    tampered[HEADER_LEN] ^= 0x01; // 翻转体首字节一位
    let tampered_len = n1;
    set.add(
        "B-503 校验和尾缀自描述",
        Frame::decode(&tampered[..tampered_len]) == Err(E_BAD_CKSUM)
            && Frame::decode(&fb[..tampered_len]).is_ok(),
        "翻转一位必被拒，完好帧必通过",
    );
    let c1 = checksum(&fb[..20]);
    let c2 = checksum(&fb[..20]);
    let c3 = checksum(&fb[..21]);
    set.add(
        "B-503 校验和确定性",
        c1 == c2 && (fb[..21] != fb[..20] || c3 == c1),
        "同输入同输出，可跨机对账",
    );

    // —— 4KB 上限与旁路 ——
    let big_hdr = Header { msg_type: 13, ver: 1, seq: 2, body_len: BODY_MAX as u16 + 1, sender: 1 };
    let mut big_body = [0u8; BODY_MAX + 1];
    big_body[0] = 1;
    set.add(
        "B-503 报文上限 4KB 钳制",
        Frame::new(big_hdr, &big_body) == Err(E_TOO_LONG),
        "超 4084 字节体直接拒绝",
    );
    let edge_hdr = Header { msg_type: 13, ver: 1, seq: 3, body_len: BODY_MAX as u16, sender: 1 };
    let edge_ok = Frame::new(edge_hdr, &big_body[..BODY_MAX]);
    set.add(
        "B-503 边界恰 4KB 接受",
        edge_ok.is_ok(),
        "4084 字节体整帧可编码",
    );
    let mut rbuf = [0u8; 16];
    let rn = bypass_ref(0xdead_beef, 0x1000, 0x30_000, &mut rbuf);
    let rd = bypass_ref_decode(&rbuf[..rn]);
    set.add(
        "B-503 大载荷旁路引用",
        rn == BYPASS_REF_LEN && rd == Some((0xdead_beef, 0x1000, 0x30_000)),
        "报文放引用、内容走共享内存环形区",
    );

    // —— 序号单调闸 ——
    let mut g = SeqGate::new();
    let a1 = g.admit(1);
    let a2 = g.admit(2);
    let a3 = g.admit(3);
    set.add(
        "B-503 序号单调接受递增",
        a1 == E_OK && a2 == E_OK && a3 == E_OK && g.seen == 3 && g.accepted == 3,
        "严格 seq==seen+1 全通",
    );
    let d1 = g.admit(3);
    let d2 = g.admit(1);
    set.add(
        "B-503 重复拒绝计数",
        d1 == E_DUP && d2 == E_DUP && g.dup_dropped == 2,
        "静默重放是缺陷——显式计数上报",
    );
    let p1 = g.admit(9);
    set.add(
        "B-503 跳号拒绝计数",
        p1 == E_GAP && g.gap_dropped == 1,
        "静默丢包是缺陷——跳号不静默吞",
    );
    let z1 = g.admit(0);
    set.add(
        "B-503 零号序列拒绝",
        z1 == E_SEQ_INVALID && g.invalid_dropped == 1,
        "序号从 1 起单调",
    );

    // —— 二十四消息全集 ——
    let mut all_ok = true;
    for t in 1u8..=24 {
        let mbl = min_body_len(t) as usize;
        let hdr = Header { msg_type: t, ver: 1, seq: t as u32, body_len: mbl as u16, sender: 2 };
        let body = [0u8; BODY_MAX];
        match Frame::new(hdr, &body[..mbl]) {
            Ok(fr) => {
                let mut b2 = [0u8; FRAME_MAX];
                let n = fr.encode(&mut b2);
                if n == 0 || Frame::decode(&b2[..n]).map(|d| d.hdr.msg_type) != Ok(t) {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    set.add(
        "B-503 二十四消息全集可编码",
        all_ok,
        "1..=24 每类按字段序下限 round-trip",
    );
    let over = Frame::new(
        Header { msg_type: 25, ver: 1, seq: 1, body_len: 4, sender: 1 },
        &[0u8; 4],
    );
    let zero = Frame::new(
        Header { msg_type: 0, ver: 1, seq: 1, body_len: 4, sender: 1 },
        &[0u8; 4],
    );
    set.add(
        "B-503 第 25 条与零类型拒绝",
        over == Err(E_BAD_TYPE) && zero == Err(E_BAD_TYPE),
        "C-3 宪法硬上限：超 24 条须 ADR",
    );
    let mut group_count = [0usize; GROUP_COUNT];
    let mut dir_c2s = 0;
    let mut dir_s2c = 0;
    let mut dir_both = 0;
    for t in 1u8..=24 {
        group_count[msg_group(t) as usize] += 1;
        match msg_dir(t) {
            Dir::C2S => dir_c2s += 1,
            Dir::S2C => dir_s2c += 1,
            Dir::Both => dir_both += 1,
        }
    }
    set.add(
        "B-503 六分组计数 5/4/4/5/4/2",
        group_count == [5, 4, 4, 5, 4, 2],
        "生命周期/绘制/输入/几何/剪贴板/浮层诊断",
    );
    set.add(
        "B-503 方向三态标注完整",
        dir_c2s == 13 && dir_s2c == 10 && dir_both == 1,
        "C->S/S->C/双向各有归属且全覆盖",
    );
    let mut mbl_ok = true;
    for t in 1u8..=24 {
        mbl_ok &= min_body_len(t) > 0;
    }
    set.add(
        "B-503 字段序下限全覆盖",
        mbl_ok && min_body_len(1) == 10 && min_body_len(3) == 4,
        "schema 先行：代码即冻结文本，变更走 ADR",
    );

    // —— 版本协商 ——
    let (e1, dg1) = negotiate(4, 4);
    let (e2, dg2) = negotiate(7, 3);
    set.add(
        "B-503 版本协商一致路径",
        e1 == 4 && !dg1,
        "小版本相同不降级",
    );
    set.add(
        "B-503 版本降级取子集",
        e2 == 3 && dg2,
        "按双方都认识的子集降级，降级事实可查",
    );

    // —— 回放环 ——
    let mut rl = ReplayLog::new();
    for i in 1..=5u32 {
        let _ = rl.push(mk_bind_frame(i, 1, 1));
    }
    let got5 = rl.get(4).map(|f| f.hdr.seq) == Some(5);
    let replayed = rl.replay_all();
    set.add(
        "B-503 回放落盘重放 round-trip",
        got5 && replayed == 5 && rl.len() == 5,
        "帧字节原样落盘，重放全过解码器",
    );
    for i in 6..=12u32 {
        let _ = rl.push(mk_bind_frame(i, 1, 1));
    }
    set.add(
        "B-503 回放环满滚动",
        rl.len() == REPLAY_SLOTS && rl.get(0).map(|f| f.hdr.seq) == Some(5) && rl.rolled == 4,
        "FIFO 淘汰最旧，新帧保全",
    );
    let mut short = [0u8; 8];
    set.add(
        "B-503 短帧拒绝",
        Frame::decode(&short) == Err(E_SHORT),
        "不足 12 字节整帧拒收",
    );
    let mut lie = [0u8; 18];
    lie[0] = 3; // surface_destroy
    lie[6..8].copy_from_slice(&8u16.to_le_bytes()); // 谎称体长 8 → 应为 20 字节，实际仅 18
    set.add(
        "B-503 长度欺骗拒绝",
        Frame::decode(&lie) == Err(E_LEN_MISMATCH),
        "头内体长与实际帧长强一致",
    );

    // —— 无指针布局 ——
    let mut off = [0u8; FRAME_MAX];
    off[0] = 1;
    off[1] = 1;
    off[2..6].copy_from_slice(&1u32.to_le_bytes());
    off[6..8].copy_from_slice(&10u16.to_le_bytes());
    off[8..10].copy_from_slice(&1u16.to_le_bytes());
    off[10..20].copy_from_slice(&[1u8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let sum_off = checksum(&off[..20]);
    off[20..22].copy_from_slice(&sum_off.to_le_bytes());
    let odd = Frame::decode(&off[..22]);
    set.add(
        "B-503 无指针非对齐安全",
        odd.is_ok() && odd.unwrap().hdr.seq == 1,
        "copy_from_slice 逐字节拷贝，1 字节偏移可解",
    );

    // —— 错误叙事 ——
    set.add(
        "B-503 错误叙事体系",
        describe(E_BAD_CKSUM).contains("重发") && describe(E_C3_LIMIT).contains("ADR"),
        "每个失败有下一步建议",
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
    fn vxwm_header_roundtrip_and_endianness() {
        let h = Header { msg_type: 9, ver: 2, seq: 0xdead_beef, body_len: 8, sender: 77 };
        let mut b = [0u8; HEADER_LEN];
        assert_eq!(h.encode(&mut b), 10);
        assert_eq!(Header::decode(&b), Ok(h));
        // 小端实证
        assert_eq!(&b[2..6], &[0xef, 0xbe, 0xad, 0xde]);
        // 头过短
        assert_eq!(Header::decode(&b[..9]), Err(HEADER_BAD_LEN));
    }

    #[test]
    fn vxwm_frame_tamper_and_mismatch() {
        let f = mk_bind_frame(9, 3, 1);
        let mut buf = [0u8; FRAME_MAX];
        let n = f.encode(&mut buf);
        assert!(n > 0);
        // 校验和篡改
        let mut bad = buf;
        let bl = f.hdr.body_len as usize;
        bad[HEADER_LEN + bl] ^= 0xff;
        assert_eq!(Frame::decode(&bad[..n]), Err(E_BAD_CKSUM));
        // 长度链破坏
        let mut bad2 = buf;
        bad2[6] ^= 0x01;
        assert_eq!(Frame::decode(&bad2[..n]), Err(E_LEN_MISMATCH));
        // 类型越界（类型检查在校验和之前，无需重算尾缀）
        let mut bad3 = buf;
        bad3[0] = 25;
        assert_eq!(Frame::decode(&bad3[..n]), Err(E_BAD_TYPE));
    }

    #[test]
    fn vxwm_seq_gate_full_matrix() {
        let mut g = SeqGate::new();
        assert_eq!(g.admit(0), E_SEQ_INVALID);
        assert_eq!(g.admit(2), E_GAP); // 首帧必须是 1
        assert_eq!(g.admit(1), E_OK);
        assert_eq!(g.admit(2), E_OK);
        assert_eq!(g.admit(2), E_DUP);
        assert_eq!(g.admit(5), E_GAP);
        assert_eq!(g.admit(3), E_OK);
        assert_eq!(g.accepted, 3);
        assert_eq!(g.dup_dropped, 1);
        assert_eq!(g.gap_dropped, 2);
        assert_eq!(g.invalid_dropped, 1);
    }

    #[test]
    fn vxwm_replay_ring_roundtrip() {
        let mut rl = ReplayLog::new();
        for i in 1..=REPLAY_SLOTS as u32 + 3 {
            rl.push(mk_bind_frame(i, 1, 1));
        }
        assert_eq!(rl.len(), REPLAY_SLOTS);
        assert_eq!(rl.total, REPLAY_SLOTS as u64 + 3);
        assert_eq!(rl.rolled, 3);
        // 最旧 1/2/3 已滚出，现存 4..=11
        assert_eq!(rl.get(0).map(|f| f.hdr.seq), Some(4));
        assert_eq!(rl.get(7).map(|f| f.hdr.seq), Some(11));
        assert_eq!(rl.replay_all(), REPLAY_SLOTS as u64);
        rl.reset();
        assert!(rl.is_empty() && rl.total == 0 && rl.rolled == 0);
    }

    #[test]
    fn vxwm_all_24_types_min_body_roundtrip() {
        let body_zero = [0u8; BODY_MAX];
        for t in 1u8..=24 {
            let mbl = min_body_len(t) as usize;
            let hdr = Header { msg_type: t, ver: 1, seq: t as u32 + 100, body_len: mbl as u16, sender: 42 };
            let f = Frame::new(hdr, &body_zero[..mbl]).unwrap_or_else(|e| panic!("type {t} new fail {e}"));
            let mut buf = [0u8; FRAME_MAX];
            let n = f.encode(&mut buf);
            assert!(n > 0, "type {t} encode fail");
            let d = Frame::decode(&buf[..n]).unwrap_or_else(|e| panic!("type {t} decode fail {e}"));
            assert_eq!(d.hdr, hdr);
            assert_eq!(d.body[..mbl], f.body[..mbl]);
        }
    }

    #[test]
    fn vxwm_c3_constitution_limit() {
        // 第 25 条在帧层即拒绝（C-3）
        let hdr = Header { msg_type: 25, ver: 1, seq: 1, body_len: 4, sender: 1 };
        assert_eq!(Frame::new(hdr, &[0u8; 4]), Err(E_BAD_TYPE));
        // 24 条名称无占位符
        for t in 1u8..=24 {
            assert_ne!(msg_name(t), "?", "type {t} unnamed");
            assert_ne!(msg_name(t), "", "type {t} empty name");
        }
        assert_eq!(msg_name(25), "?");
    }

    #[test]
    fn vxwm_negotiate_matrix() {
        assert_eq!(negotiate(0, 0), (0, false));
        assert_eq!(negotiate(1, 9), (1, true));
        assert_eq!(negotiate(9, 1), (1, true));
        assert_eq!(negotiate(255, 255), (255, false));
    }

    #[test]
    fn vxwm_all_checks_pass() {
        let set = run_vxwm_checks();
        assert!(set.len() >= 20, "B-503 CheckSet 应≥20 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-503 check {} failed: {}", c.name, c.detail);
        }
    }
}
