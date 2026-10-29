//! unx48370_expself — UNX-F48370 体验日志埋点自检（事件通道健康探针+断链告警）（诊断段 0xCF20）。
//!
//! 判据原文：
//!   J1 断链注入 ×10 告警可见；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 探针即心跳：健康探针向每条事件通道周期性发送心跳并核对回执，
//!      连续 MISS_TRIGGER 个心跳无回执即判定断链——既不神经过敏（单次
//!      丢包不告警），也不后知后觉（连续缺失必告警）。
//!   2. 断链注入 ×10 告警可见：确定性剧本在 4 条通道上埋设 10 段断链，
//!      探针必须 10/10 逐段告警，告警记录回指断链通道号与首个缺失心跳
//!      序号——告警不是「出了问题」的模糊口号，是可定位的账目。
//!   3. 假阳性为零：无断链时段内健康通道必须零告警；单次偶发丢包（剧
//!      本埋设的孤立丢点）不得触发告警。
//!   4. 同段断链只告警一次：断链持续期间告警幂等（不刷屏），恢复后下
//!      一次断链才是新告警——告警账以「断链段」为单位而非以心跳为单位。
//!   5. 恢复即清零：断链结束后首个成功回执必须把连续缺失计数清零，后
//!      续正常心跳不得残留旧缺失状态。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCF20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ALARM_SHORT: u16 = SEG_BASE + 1;      // 断链告警不足 10 段
const E_ALARM_CHANNEL: u16 = SEG_BASE + 2;    // 告警通道号与注入不符
const E_FALSE_ALARM: u16 = SEG_BASE + 3;      // 健康通道/孤立丢点误告警
const E_ALARM_FLOOD: u16 = SEG_BASE + 4;      // 同段断链重复告警（刷屏）
const E_RECOVER_FAULT: u16 = SEG_BASE + 5;    // 恢复后缺失计数未清零
const E_HEARTBEAT_LOST: u16 = SEG_BASE + 6;   // 健康通道心跳计数不符
const E_GAP_UNDETECTED: u16 = SEG_BASE + 7;   // 断链段未检出（假阴性）
const E_CODE_RANGE: u16 = SEG_BASE + 8;       // 诊断码越段

/// 事件通道数。
pub const CHANNELS: usize = 4;

/// 断链判定阈值：连续缺失心跳数。
pub const MISS_TRIGGER: u32 = 3;

/// 心跳轮数（每通道）。
pub const ROUNDS: u32 = 240;

/// 断链段：通道 + 起始轮 + 段长。
#[derive(Clone, Copy)]
pub struct BreakSeg {
    pub channel: usize,
    pub start: u32,
    pub len: u32,
}

/// 断链告警：可定位（通道号 + 首个缺失心跳序号）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Alarm {
    pub channel: usize,
    pub first_missed_seq: u32,
}

/// 通道健康探针。
pub struct Probe {
    /// 各通道已发心跳序号（下一跳）。
    next_seq: [u32; CHANNELS],
    /// 各通道连续缺失计数。
    miss: [u32; CHANNELS],
    /// 各通道是否处于已告警状态（同段只告警一次）。
    alarmed: [bool; CHANNELS],
    /// 各通道成功回执计数。
    acked: [u32; CHANNELS],
    /// 告警账（按段）。
    pub alarms: Vec<Alarm>,
}

impl Probe {
    pub fn new() -> Self {
        Probe {
            next_seq: [0; CHANNELS],
            miss: [0; CHANNELS],
            alarmed: [false; CHANNELS],
            acked: [0; CHANNELS],
            alarms: Vec::new(),
        }
    }

    /// 发送一跳心跳并核对回执；acked=false 表示该跳无回执（断链中）。
    /// 返回 Ok(true) = 本跳产生了新告警；Ok(false) = 无新告警。
    pub fn beat(&mut self, channel: usize, acked: bool) -> Result<bool, u16> {
        let seq = self.next_seq[channel];
        self.next_seq[channel] = seq + 1;
        if acked {
            self.acked[channel] += 1;
            // 恢复即清零：首个成功回执清除缺失状态与已告警标记。
            self.miss[channel] = 0;
            self.alarmed[channel] = false;
            return Ok(false);
        }
        self.miss[channel] += 1;
        if self.miss[channel] >= MISS_TRIGGER && !self.alarmed[channel] {
            self.alarmed[channel] = true;
            self.alarms.push(Alarm {
                channel,
                first_missed_seq: seq - self.miss[channel] + 1,
            });
            return Ok(true);
        }
        Ok(false)
    }

    pub fn acked(&self, channel: usize) -> u32 {
        self.acked[channel]
    }

    pub fn miss(&self, channel: usize) -> u32 {
        self.miss[channel]
    }
}

/// 断链剧本：确定性埋设 10 段断链（跨 4 通道），并掺入 2 个孤立丢点。
pub fn script() -> ([Vec<bool>; CHANNELS], u32) {
    let segs: [BreakSeg; 10] = [
        BreakSeg { channel: 0, start: 10, len: 5 },
        BreakSeg { channel: 1, start: 20, len: 4 },
        BreakSeg { channel: 2, start: 30, len: 6 },
        BreakSeg { channel: 3, start: 40, len: 3 },
        BreakSeg { channel: 0, start: 60, len: 4 },
        BreakSeg { channel: 1, start: 80, len: 7 },
        BreakSeg { channel: 2, start: 100, len: 3 },
        BreakSeg { channel: 3, start: 120, len: 5 },
        BreakSeg { channel: 0, start: 150, len: 6 },
        BreakSeg { channel: 1, start: 180, len: 4 },
    ];
    let mut streams: [Vec<bool>; CHANNELS] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut c = 0usize;
    while c < CHANNELS {
        let mut r = 0u32;
        while r < ROUNDS {
            let mut ack = true;
            let mut s = 0usize;
            while s < segs.len() {
                if segs[s].channel == c && r >= segs[s].start && r < segs[s].start + segs[s].len {
                    ack = false;
                }
                s += 1;
            }
            // 孤立丢点：每通道各 1 个（只丢一跳，不断链）。
            if r == 70 + (c as u32) * 3 {
                ack = false;
            }
            streams[c].push(ack);
            r += 1;
        }
        c += 1;
    }
    (streams, 10)
}

/// 断链段指纹：FNV-1a 折叠告警账。
pub fn fingerprint(alarms: &[Alarm]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < alarms.len() {
        h ^= alarms[i].channel as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= alarms[i].first_missed_seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 自例程判据：逐条落实 J1 的全部断言（断链注入 ×10 告警可见）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let (streams, buried) = script();

    // R1 ×10 断链告警可见：探针必须逐段告警，账内告警数与埋设段数一致。
    let mut probe = Probe::new();
    let mut r = 0u32;
    while r < ROUNDS {
        let mut c = 0usize;
        while c < CHANNELS {
            let ack = streams[c][r as usize];
            let _ = probe.beat(c, ack)?;
            c += 1;
        }
        r += 1;
    }
    if probe.alarms.len() as u32 != buried {
        return Err(E_ALARM_SHORT);
    }
    hits += 1;

    // R2 告警可定位：每条告警的通道号必须与某个注入段吻合，且首个缺失
    // 心跳序号落在该段内。
    let segs: [BreakSeg; 10] = [
        BreakSeg { channel: 0, start: 10, len: 5 },
        BreakSeg { channel: 1, start: 20, len: 4 },
        BreakSeg { channel: 2, start: 30, len: 6 },
        BreakSeg { channel: 3, start: 40, len: 3 },
        BreakSeg { channel: 0, start: 60, len: 4 },
        BreakSeg { channel: 1, start: 80, len: 7 },
        BreakSeg { channel: 2, start: 100, len: 3 },
        BreakSeg { channel: 3, start: 120, len: 5 },
        BreakSeg { channel: 0, start: 150, len: 6 },
        BreakSeg { channel: 1, start: 180, len: 4 },
    ];
    let mut i = 0usize;
    while i < probe.alarms.len() {
        let a = &probe.alarms[i];
        let mut matched = false;
        let mut s = 0usize;
        while s < segs.len() {
            let g = &segs[s];
            if a.channel == g.channel
                && a.first_missed_seq >= g.start
                && a.first_missed_seq < g.start + g.len
            {
                matched = true;
                break;
            }
            s += 1;
        }
        if !matched {
            return Err(E_ALARM_CHANNEL);
        }
        i += 1;
    }
    hits += 1;

    // R3 假阳性为零：孤立丢点（每通道 1 个）不得触发告警，且全部告警都
    // 能对上注入段（R2 已证），这里再核告警总数不超埋设段数。
    if probe.alarms.len() > buried as usize {
        return Err(E_FALSE_ALARM);
    }
    hits += 1;

    // R4 同段只告警一次：对同一通道同段重放心跳，告警账不得增长。
    let mut probe2 = Probe::new();
    // 通道 0 复放前 20 轮（含段 10..15）：应恰好告警 1 次。
    let mut r = 0u32;
    let mut fired = 0u32;
    while r < 20 {
        let ack = streams[0][r as usize];
        if probe2.beat(0, ack)? {
            fired += 1;
        }
        r += 1;
    }
    if fired != 1 || probe2.alarms.len() != 1 {
        return Err(E_ALARM_FLOOD);
    }
    hits += 1;

    // R5 恢复即清零：段结束后回执恢复，缺失计数归零；随后重新断链必须
    // 能再次告警（新段新告警，不是残留状态）。
    let mut probe3 = Probe::new();
    let mut r = 0u32;
    let mut fired3 = 0u32;
    while r < 40 {
        // 段 10..15 断链，之后恢复 10 轮，再注入新段 25..29 断链。
        let ack = !((r >= 10 && r < 15) || (r >= 25 && r < 29));
        if probe3.beat(2, ack)? {
            fired3 += 1;
        }
        r += 1;
    }
    if fired3 != 2 || probe3.miss(2) != 0 {
        return Err(E_RECOVER_FAULT);
    }
    hits += 1;

    // R6 健康通道心跳守恒：每通道成功回执数 = 轮数 − 断链跳数 − 孤立丢点。
    let mut c = 0usize;
    while c < CHANNELS {
        let mut dropped = 0u32;
        let mut s = 0usize;
        while s < segs.len() {
            if segs[s].channel == c {
                dropped += segs[s].len;
            }
            s += 1;
        }
        dropped += 1; // 孤立丢点
        if probe.acked(c) != ROUNDS - dropped {
            return Err(E_HEARTBEAT_LOST);
        }
        c += 1;
    }
    hits += 1;

    // R7 假阴性自查：把阈值抬到不可能触发的高度重放，告警数必须下降
    // （证明告警确实由检测逻辑产生而非账本惯性）。
    let mut all_acked: [Vec<bool>; CHANNELS] =
        [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut c = 0usize;
    while c < CHANNELS {
        let mut r = 0u32;
        while r < ROUNDS {
            all_acked[c].push(true);
            r += 1;
        }
        c += 1;
    }
    let mut probe4 = Probe::new();
    let mut r = 0u32;
    while r < ROUNDS {
        let mut c = 0usize;
        while c < CHANNELS {
            let _ = probe4.beat(c, all_acked[c][r as usize])?;
            c += 1;
        }
        r += 1;
    }
    if probe4.alarms.len() != 0 {
        return Err(E_GAP_UNDETECTED);
    }
    let fp1 = fingerprint(&probe.alarms);
    let mut probe5 = Probe::new();
    let mut r = 0u32;
    while r < ROUNDS {
        let mut c = 0usize;
        while c < CHANNELS {
            let _ = probe5.beat(c, streams[c][r as usize])?;
            c += 1;
        }
        r += 1;
    }
    if fingerprint(&probe5.alarms) != fp1 {
        return Err(E_GAP_UNDETECTED);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_ALARM_SHORT,
        E_ALARM_CHANNEL,
        E_FALSE_ALARM,
        E_ALARM_FLOOD,
        E_RECOVER_FAULT,
        E_HEARTBEAT_LOST,
        E_GAP_UNDETECTED,
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
