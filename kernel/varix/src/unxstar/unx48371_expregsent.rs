//! unx48371_expregsent — UNX-F48371 体验日志三类窗口回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严：三窗事件源标签正确）（诊断段 0xCF40）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗 ×10 轮源标签零混淆；
//!   判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 源标签即出身证：三类窗口（vxwm 原生/Wine/Xwayland）每条埋点事件
//!      在发射侧就打上源标签（窗口类码 + 窗口号），路由器按标签分发进
//!      各自的分窗日志——标签对不上窗口出身的事件一律拒收，绝不宽容。
//!   2. 三窗 ×10 轮：三类窗口 ×10 轮共 30 条运行，每轮每窗 50 条事件，
//!      全部通过路由且逐条落进正确分窗日志，任何一条落错窗即混淆。
//!   3. 零混淆可审计：分窗日志每条记录打包（源标签, 载荷）整存，审计
//!      扫描逐条解包核对源标签与所属窗口一致——混淆必须能从账面扫出，
//!      不依赖运行时自觉。
//!   4. 标签往返保真：源标签编码→解码必须还原同一窗口类，三窗 ×10 轮
//!      全部事件逐一往返核对；标签域外值解码失败（不做宽容映射）。
//!   5. 加严自查：人工注入错标事件（张冠李戴）必须被路由器全部拒收
//!      （三窗各注 1 条，3/3 拒收）；干净重放账指纹必须一致。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCF40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_TAG_MISMATCH: u16 = SEG_BASE + 1;  // 源标签与窗口出身不符
const E_ROUTE_FAULT: u16 = SEG_BASE + 2;   // 事件未落入正确分窗日志
const E_LOGLEN_FAULT: u16 = SEG_BASE + 3;  // 分窗日志条数不符
const E_TAG_ROUNDTRIP: u16 = SEG_BASE + 4; // 标签往返失真
const E_FOREIGN_ACCEPTED: u16 = SEG_BASE + 5; // 标签域外值被解码接受
const E_MISLABEL_PASSED: u16 = SEG_BASE + 6; // 错标注入未被拒收
const E_REPLAY_FAULT: u16 = SEG_BASE + 7;  // 干净重放账指纹不一致
const E_RUN_SHORT: u16 = SEG_BASE + 8;     // 运行条数不足（三窗 ×10 轮）
const E_CODE_RANGE: u16 = SEG_BASE + 9;    // 诊断码越段

/// 三类窗口。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WinClass {
    /// vxwm 原生窗（协议内直连）。
    VxwmNative,
    /// Wine 窗（兼容层转译）。
    Wine,
    /// Xwayland 窗（X 协议桥接）。
    Xwayland,
}

/// 三窗枚举顺序表。
pub const WINDOW_CLASSES: [WinClass; 3] = [
    WinClass::VxwmNative,
    WinClass::Wine,
    WinClass::Xwayland,
];

/// 窗口类码（源标签编码域：1..=3）。
pub fn tag_of(w: WinClass) -> u8 {
    match w {
        WinClass::VxwmNative => 1,
        WinClass::Wine => 2,
        WinClass::Xwayland => 3,
    }
}

/// 标签解码：域内值 → 窗口类；域外一律 None（不宽容映射）。
pub fn tag_decode(t: u8) -> Option<WinClass> {
    match t {
        1 => Some(WinClass::VxwmNative),
        2 => Some(WinClass::Wine),
        3 => Some(WinClass::Xwayland),
        _ => None,
    }
}

/// 回归轮数（×10 轮，宪法加严）。
pub const ROUNDS: u32 = 10;

/// 每轮每窗事件数。
pub const EVENTS_PER_RUN: u32 = 50;

/// 埋点事件：携带发射侧源标签。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SrcEvent {
    pub win: WinClass,
    pub tag: u8,
    pub seq: u32,
    pub payload: u16,
}

/// 分窗日志记录：打包（源标签 << 16 | 载荷），整存便于审计。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LogEntry {
    pub packed: u32,
}

/// 打包：源标签 + 载荷。
pub fn pack(tag: u8, payload: u16) -> u32 {
    ((tag as u32) << 16) | payload as u32
}

/// 解包：返回（源标签, 载荷）。
pub fn unpack(p: u32) -> (u8, u16) {
    ((p >> 16) as u8, (p & 0xFFFF) as u16)
}

/// 三类窗口回归哨兵：路由器 + 三份分窗日志。
pub struct RegSentinel {
    logs: [Vec<LogEntry>; 3],
}

impl RegSentinel {
    pub fn new() -> Self {
        RegSentinel {
            logs: [Vec::new(), Vec::new(), Vec::new()],
        }
    }

    /// 路由一条事件：源标签必须与窗口出身一致，否则拒收。
    /// 返回 Ok(true) = 已入账；Ok(false) = 不可能；Err = 错标拒收。
    pub fn route(&mut self, e: &SrcEvent) -> Result<bool, u16> {
        if tag_of(e.win) != e.tag {
            return Err(E_TAG_MISMATCH);
        }
        let idx = match e.win {
            WinClass::VxwmNative => 0usize,
            WinClass::Wine => 1usize,
            WinClass::Xwayland => 2usize,
        };
        self.logs[idx].push(LogEntry {
            packed: pack(e.tag, e.payload),
        });
        Ok(true)
    }

    pub fn log_len(&self, idx: usize) -> usize {
        self.logs[idx].len()
    }

    pub fn log(&self, idx: usize) -> &[LogEntry] {
        &self.logs[idx]
    }
}

/// 确定性线性同余发生器（事件载荷生成统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 三窗 ×10 轮事件流：每轮每窗 50 条，载荷 LCG 确定，标签始终正确。
pub fn stream() -> Vec<SrcEvent> {
    let mut out: Vec<SrcEvent> = Vec::new();
    let mut w = 0usize;
    while w < 3 {
        let mut r = 0u32;
        while r < ROUNDS {
            let mut lc = Lcg::new((0x4837_1001u32) ^ ((w as u32) * 4099 + r * 761));
            let mut k = 0u32;
            while k < EVENTS_PER_RUN {
                out.push(SrcEvent {
                    win: WINDOW_CLASSES[w],
                    tag: tag_of(WINDOW_CLASSES[w]),
                    seq: r * EVENTS_PER_RUN + k,
                    payload: (lc.next() & 0xFFFF) as u16,
                });
                k += 1;
            }
            r += 1;
        }
        w += 1;
    }
    out
}

/// 哨兵账指纹：FNV-1a 折叠三分窗日志。
pub fn fingerprint(s: &RegSentinel) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut idx = 0usize;
    while idx < 3 {
        let mut i = 0usize;
        while i < s.log(idx).len() {
            h ^= s.log(idx)[i].packed as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        idx += 1;
    }
    h
}

/// 自例程判据：逐条落实 J1 的全部断言（三窗 ×10 轮源标签零混淆）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let evs = stream();

    // R1 三窗 ×10 轮全量路由：1500 条事件全部通过且零拒收。
    let mut sent = RegSentinel::new();
    let mut i = 0usize;
    while i < evs.len() {
        if sent.route(&evs[i]).is_err() {
            return Err(E_ROUTE_FAULT);
        }
        i += 1;
    }
    let expect_per = (ROUNDS * EVENTS_PER_RUN) as usize;
    let mut w = 0usize;
    while w < 3 {
        if sent.log_len(w) != expect_per {
            return Err(E_RUN_SHORT);
        }
        w += 1;
    }
    hits += 1;

    // R2 零混淆审计：逐窗逐条解包，源标签必须与所属窗口类一致。
    w = 0;
    while w < 3 {
        let log = sent.log(w);
        let mut i = 0usize;
        while i < log.len() {
            let (t, _p) = unpack(log[i].packed);
            let wcls = tag_decode(t).ok_or(E_TAG_MISMATCH)?;
            if tag_of(wcls) != tag_of(WINDOW_CLASSES[w]) {
                return Err(E_TAG_MISMATCH);
            }
            i += 1;
        }
        w += 1;
    }
    hits += 1;

    // R3 标签往返保真：全部 1500 条事件的标签编码→解码还原同一窗口类。
    i = 0;
    while i < evs.len() {
        let back = tag_decode(evs[i].tag).ok_or(E_TAG_ROUNDTRIP)?;
        if tag_of(back) != evs[i].tag || back != evs[i].win {
            return Err(E_TAG_ROUNDTRIP);
        }
        i += 1;
    }
    hits += 1;

    // R4 标签域封闭：域外值（0、4、255）解码必须失败。
    const FOREIGN: [u8; 3] = [0, 4, 255];
    let mut i = 0usize;
    while i < FOREIGN.len() {
        if tag_decode(FOREIGN[i]).is_some() {
            return Err(E_FOREIGN_ACCEPTED);
        }
        i += 1;
    }
    hits += 1;

    // R5 加严自查·错标拒收：三窗各注入 1 条错标事件（标签互换），必须
    // 3/3 拒收，且账面条数不变。
    let mut sent2 = RegSentinel::new();
    let bads: [SrcEvent; 3] = [
        SrcEvent {
            win: WinClass::VxwmNative,
            tag: tag_of(WinClass::Wine),
            seq: 90001,
            payload: 0x1234,
        },
        SrcEvent {
            win: WinClass::Wine,
            tag: tag_of(WinClass::Xwayland),
            seq: 90002,
            payload: 0x5678,
        },
        SrcEvent {
            win: WinClass::Xwayland,
            tag: tag_of(WinClass::VxwmNative),
            seq: 90003,
            payload: 0x9ABC,
        },
    ];
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < bads.len() {
        if sent2.route(&bads[i]).is_err() {
            rejected += 1;
        }
        i += 1;
    }
    if rejected != 3 || sent2.log_len(0) + sent2.log_len(1) + sent2.log_len(2) != 0 {
        return Err(E_MISLABEL_PASSED);
    }
    hits += 1;

    // R6 路由落位精确：事件载荷必须原样出现在其窗口类的分窗日志里
    // （抽查三窗各 100 条：解包载荷与事件流载荷逐一相等）。
    w = 0;
    while w < 3 {
        let log = sent.log(w);
        let mut i = 0usize;
        let mut k = 0usize;
        while i < evs.len() && k < 100 {
            if evs[i].win == WINDOW_CLASSES[w] {
                let (_t, p) = unpack(log[k].packed);
                if p != evs[i].payload {
                    return Err(E_ROUTE_FAULT);
                }
                k += 1;
            }
            i += 1;
        }
        if k != 100 {
            return Err(E_LOGLEN_FAULT);
        }
        w += 1;
    }
    hits += 1;

    // R7 干净重放一致：同一事件流二次路由，账指纹必须一致。
    let fp1 = fingerprint(&sent);
    let mut sent3 = RegSentinel::new();
    let mut i = 0usize;
    while i < evs.len() {
        let _ = sent3.route(&evs[i]);
        i += 1;
    }
    if fingerprint(&sent3) != fp1 {
        return Err(E_REPLAY_FAULT);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 9] = [
        E_TAG_MISMATCH,
        E_ROUTE_FAULT,
        E_LOGLEN_FAULT,
        E_TAG_ROUNDTRIP,
        E_FOREIGN_ACCEPTED,
        E_MISLABEL_PASSED,
        E_REPLAY_FAULT,
        E_RUN_SHORT,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
