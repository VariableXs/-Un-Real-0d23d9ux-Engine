//! unx0447_fadttiming —— UNX-F0447 FADT 解析（计时字段账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0447 · FADT 解析（计时字段账） · 目标 240 行
//! 判据原文：UNX-F0447-J1 FADT（签名 "FACP"）计时字段族五账：
//!           账一 PM1a/PM1b 事件块（PM1_EVT_LEN @56，PM1a_
//!           EVT_BLK @76，PM1b_EVT_BLK @80）；账二 PM1a/PM1b
//!           控制块（PM1_CNT_LEN @65，PM1a_CNT_BLK @81，
//!           PM1b_CNT_BLK @85）；账三 PM_TIMER 频率判别（标志
//!           位 8 = TMR_VAL_EXT：0 → 24 位计数 3.579545MHz，
//!           1 → 32 位计数 3.579545MHz——频率恒定，位宽分
//!           叉）；账四 睡眠态寄存器（SLEEP_CONTROL @117 ×
//!           SLEEP_STATUS @118，二者必须同存同亡：只出现一
//!           个立案）；账五 世纪寄存器（CENTURY @108，0 =
//!           无世纪寄存器，RTC 走 Century 字段兜底）。块长
//!           核：PM1_EVT_LEN 合法域 {4, 8}（两半各 4 字节）。
//! 组件：FACP 字段拆解器 + 五账逐项核 + 频率/位宽分叉器 +
//!       同存同亡闸 + 块长域核。
//! 职责定位：FADT 是「固件留给内核的寄存器地图」——PM 计
//!   时、睡眠、RTC 世纪全在图上；PM_TIMER 位宽分叉是 24/32
//!   位回绕差异的源头（算错位宽节拍就漂）；同存同亡闸防的
//!   是「只给控制不给状态」的半残固件。
//! 性能逐项分解：字段拆解 O(1)；五账 O(1)；域核 O(1)。
//! 诊断码段：0x34100（0x34101~0x34109 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FT_VERSION: &str = "unx0447_fadttiming-v1";

/// 诊断段基址（0x34100 段 · 本模块独占）。
pub const FT_SEG_BASE: u32 = 0x34100;
pub const XC_SIG: u32 = 0x34101; // 签名非 FACP
pub const XC_EVT_LEN: u32 = 0x34102; // PM1_EVT_LEN 越域
pub const XC_HALF_ORPHAN: u32 = 0x34103; // 半块孤案（a 块 0 但 b 块非 0）
pub const XC_SLEEP_HALF: u32 = 0x34104; // 同存同亡破
pub const XC_HDR_SHORT: u32 = 0x34105; // 表不足最长字段偏移
pub const XC_MAGIC: u32 = 0x34109; // 账魔数破

/// FACP 字段偏移。
pub const OFF_EVT_LEN: usize = 56;
pub const OFF_CNT_LEN: usize = 65;
pub const OFF_CENTURY: usize = 108;
pub const OFF_EVT_A: usize = 76;
pub const OFF_EVT_B: usize = 80;
pub const OFF_CNT_A: usize = 81;
pub const OFF_CNT_B: usize = 85;
pub const OFF_SLEEP_CTRL: usize = 117;
pub const OFF_SLEEP_STAT: usize = 118;
pub const OFF_FLAGS: usize = 112;
/// Flags 位 8：TMR_VAL_EXT（32 位 PM_TIMER）。
pub const FLAG_TMR_EXT: u32 = 1 << 8;
/// PM_TIMER 恒定频率（Hz，规范常数）。
pub const PM_TIMER_HZ: u64 = 3_579_545;

/// 解析产出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FadtTiming {
    pub evt_a: u32,
    pub evt_b: u32,
    pub evt_len: u8,
    pub cnt_a: u32,
    pub cnt_b: u32,
    pub cnt_len: u8,
    pub century: u8,
    pub sleep_ctrl: u8,
    pub sleep_stat: u8,
    pub flags: u32,
}

impl FadtTiming {
    /// PM_TIMER 位宽：24 或 32。
    pub fn timer_bits(&self) -> u32 {
        if self.flags & FLAG_TMR_EXT != 0 {
            32
        } else {
            24
        }
    }

    /// PM_TIMER 回绕值。
    pub fn timer_wrap(&self) -> u64 {
        1u64 << self.timer_bits()
    }

    /// 世纪寄存器有无。
    pub fn has_century(&self) -> bool {
        self.century != 0
    }
}

/// FADT 解析（计时字段面）。
pub fn parse_fadt(raw: &[u8]) -> Result<FadtTiming, u32> {
    if raw.len() < 4 || &raw[0..4] != b"FACP" {
        return Err(XC_SIG);
    }
    if raw.len() <= OFF_SLEEP_STAT {
        return Err(XC_HDR_SHORT);
    }
    let u16_at = |o: usize| u16::from_le_bytes([raw[o], raw[o + 1]]) as u32;
    let f = FadtTiming {
        evt_a: u16_at(OFF_EVT_A),
        evt_b: u16_at(OFF_EVT_B),
        evt_len: raw[OFF_EVT_LEN],
        cnt_a: u16_at(OFF_CNT_A),
        cnt_b: u16_at(OFF_CNT_B),
        cnt_len: raw[OFF_CNT_LEN],
        century: raw[OFF_CENTURY],
        sleep_ctrl: raw[OFF_SLEEP_CTRL],
        sleep_stat: raw[OFF_SLEEP_STAT],
        flags: u32::from_le_bytes([raw[OFF_FLAGS], raw[OFF_FLAGS + 1], raw[OFF_FLAGS + 2], raw[OFF_FLAGS + 3]]),
    };
    // 块长域核：PM1_EVT_LEN ∈ {4, 8}（0 = 无事件块亦合法）。
    if !(f.evt_len == 0 || f.evt_len == 4 || f.evt_len == 8) {
        return Err(XC_EVT_LEN);
    }
    // 半块孤案：a 块为 0 但 b 块非 0 → 固件乱序。
    if f.evt_a == 0 && f.evt_b != 0 {
        return Err(XC_HALF_ORPHAN);
    }
    // 同存同亡闸。
    if (f.sleep_ctrl == 0) != (f.sleep_stat == 0) {
        return Err(XC_SLEEP_HALF);
    }
    Ok(f)
}

/// 造 FACP（夹具）。
pub fn mk_fadt(fields: &[(usize, u64)], fbytes: usize) -> Vec<u8> {
    let mut raw = vec![0u8; fbytes.max(OFF_SLEEP_STAT + 1)];
    raw[0..4].copy_from_slice(b"FACP");
    for &(o, v) in fields.iter() {
        match o {
            OFF_FLAGS => {
                raw[OFF_FLAGS..OFF_FLAGS + 4].copy_from_slice(&(v as u32).to_le_bytes());
            }
            OFF_EVT_A | OFF_EVT_B | OFF_CNT_A | OFF_CNT_B => {
                raw[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
            }
            _ => {
                raw[o] = v as u8;
            }
        }
    }
    raw
}

/// 判据入口：UNX-F0447。
pub fn run_unx0447_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0447_fadttiming");
    // 族一：基本五账。
    let f = mk_fadt(&[
        (OFF_EVT_LEN, 8), (OFF_CNT_LEN, 4),
        (OFF_EVT_A, 0x400), (OFF_EVT_B, 0), (OFF_CNT_A, 0x404), (OFF_CNT_B, 0),
        (OFF_CENTURY, 0x32), (OFF_FLAGS, FLAG_TMR_EXT as u64), (OFF_SLEEP_CTRL, 0), (OFF_SLEEP_STAT, 0),
    ], 200);
    let p = parse_fadt(&f);
    cs.add("事件块 a=0x400", p.as_ref().map(|x| x.evt_a == 0x400).unwrap_or(false), "PM1a");
    cs.add("控制块 a=0x404", p.as_ref().map(|x| x.cnt_a == 0x404).unwrap_or(false), "PM1aCNT");
    cs.add("块长 8", p.as_ref().map(|x| x.evt_len == 8).unwrap_or(false), "两半各 4");
    cs.add("世纪 0x32", p.as_ref().map(|x| x.century == 0x32).unwrap_or(false), "CMOS 槽");
    // 族二：位宽分叉。
    cs.add("32 位分叉", p.as_ref().map(|x| x.timer_bits() == 32).unwrap_or(false), "位 8 置");
    cs.add("回绕 2^32", p.as_ref().map(|x| x.timer_wrap() == 0x1_0000_0000).unwrap_or(false), "32 位绕");
    let f24 = mk_fadt(&[(OFF_EVT_LEN, 4), (OFF_FLAGS, 0)], 200);
    let p24 = parse_fadt(&f24);
    cs.add("24 位分叉", p24.as_ref().map(|x| x.timer_bits() == 24).unwrap_or(false), "位 8 清");
    cs.add("回绕 2^24", p24.as_ref().map(|x| x.timer_wrap() == 0x100_0000).unwrap_or(false), "24 位绕");
    // 族三：频率恒定。
    cs.add("恒频 3579545", PM_TIMER_HZ == 3_579_545, "规范常数");
    // 族四：块长域。
    let bad_len = mk_fadt(&[(OFF_EVT_LEN, 6)], 200);
    cs.add("块长 6 拒", parse_fadt(&bad_len) == Err(XC_EVT_LEN), "非 0/4/8");
    cs.add("块长 0 过", parse_fadt(&mk_fadt(&[(OFF_EVT_LEN, 0)], 200)).is_ok(), "无事件块");
    // 族五：半块孤案。
    let orphan = mk_fadt(&[(OFF_EVT_A, 0), (OFF_EVT_B, 0x500)], 200);
    cs.add("b 有 a 无立案", parse_fadt(&orphan) == Err(XC_HALF_ORPHAN), "乱序固件");
    let both = mk_fadt(&[(OFF_EVT_A, 0x400), (OFF_EVT_B, 0x500)], 200);
    cs.add("双块过", parse_fadt(&both).map(|x| x.evt_a == 0x400 && x.evt_b == 0x500).unwrap_or(false), "a b 齐");
    // 族六：同存同亡。
    let half_ctrl = mk_fadt(&[(OFF_SLEEP_CTRL, 0x200), (OFF_SLEEP_STAT, 0)], 200);
    cs.add("只控无态立案", parse_fadt(&half_ctrl) == Err(XC_SLEEP_HALF), "半残固件");
    let half_stat = mk_fadt(&[(OFF_SLEEP_CTRL, 0), (OFF_SLEEP_STAT, 0x201)], 200);
    cs.add("只态无控立案", parse_fadt(&half_stat) == Err(XC_SLEEP_HALF), "镜像半残");
    let both_sleep = mk_fadt(&[(OFF_SLEEP_CTRL, 0x200), (OFF_SLEEP_STAT, 0x201)], 200);
    cs.add("同存过", parse_fadt(&both_sleep).is_ok(), "对齐");
    let none_sleep = mk_fadt(&[(OFF_SLEEP_CTRL, 0), (OFF_SLEEP_STAT, 0)], 200);
    cs.add("同亡过", parse_fadt(&none_sleep).is_ok(), "全零对");
    // 族七：世纪兜底。
    cs.add("世纪 0 无寄存器", parse_fadt(&mk_fadt(&[(OFF_CENTURY, 0)], 200)).map(|x| !x.has_century()).unwrap_or(false), "走 Century 字段");
    // 族八：签名/短头。
    cs.add("非 FACP 拒", parse_fadt(b"HPETxxxx") == Err(XC_SIG), "签名闸");
    cs.add("短表拒", parse_fadt(b"FACP") == Err(XC_HDR_SHORT), "118 不足");
    // 族九：cnt_len 透传。
    cs.add("cnt_len 4", p.as_ref().map(|x| x.cnt_len == 4).unwrap_or(false), "字段直通");
    cs
}
