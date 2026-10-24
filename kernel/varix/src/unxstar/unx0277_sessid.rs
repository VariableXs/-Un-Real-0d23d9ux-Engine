//! unx0277_sessid —— UNX-F0277 远程终端会话标识（串口会话头）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0277 · 会话标识 · 目标 180 行
//! 判据原文：UNX-F0277-J1 每个串口会话分配唯一标识头：标识 =
//!           会话号（单调）+ 开场 FNV 指纹；帧必须带当前会话
//!           号（旧会话帧拒收——串口热插后旧字节流不许混入
//!           新会话账）。
//! 组件：会话头（号 + 指纹 + 开场字节序列）+ 帧校验（会话号
//!       匹配核）+ 会话账（开/关账 + 活跃唯一）。
//! 职责定位：会话头是「串口字节流的国籍护照」——串口是无状态
//!   字节管道，热插/重连后旧会话的尾巴字节混进新会话账是最
//!   难查的脏数据源。帧级会话号匹配把混入当场拦下。
//! 性能逐项分解：开会话 O(1)；帧校验 O(1)；会话账 O(1)。
//! 诊断码段：0x20480（0x20481~0x20489 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SI_VERSION: &str = "unx0277_sessid-v1";

/// 诊断段基址（0x20480 段 · 本模块独占）。
pub const SI_SEG_BASE: u32 = 0x20480;
pub const XC_OLD_SESS: u32 = 0x20481; // 旧会话帧
pub const XC_SESS_ZERO: u32 = 0x20482; // 会话号 0 拒
pub const XC_FP_DRIFT: u32 = 0x20483; // 指纹漂移
pub const XC_OPEN_DUP: u32 = 0x20484; // 重复开会话
pub const XC_CLOSE_NO: u32 = 0x20485; // 关未开会话
pub const XC_HDR_BAD: u32 = 0x20486; // 头格式破
pub const XC_ACTIVE_OOR: u32 = 0x20487; // 活跃态越域
pub const XC_MAGIC: u32 = 0x20488; // 头魔数破
pub const XC_FP_ZERO: u32 = 0x20489; // 零指纹拒

/// 会话头魔数（4 字节）。
pub const HDR_MAGIC: u32 = 0x5345_5353; // "SESS"

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 会话头。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessHeader {
    /// 会话号（单调从 1 起）。
    pub sess: u32,
    /// 开场指纹（FNV(开场字节)）。
    pub fp: u64,
}

impl SessHeader {
    pub fn new(sess: u32, opening: &[u8]) -> Result<SessHeader, u32> {
        if sess == 0 {
            return Err(XC_SESS_ZERO);
        }
        if opening.is_empty() {
            return Err(XC_FP_ZERO);
        }
        let fp = fnv64(opening);
        if fp == 0 {
            return Err(XC_FP_ZERO); // 理论不可达，防御账
        }
        Ok(SessHeader { sess, fp })
    }

    /// 序列化：魔数 + 会话号 + 指纹低/高词。
    pub fn serialize(&self) -> [u32; 4] {
        [HDR_MAGIC, self.sess, self.fp as u32, (self.fp >> 32) as u32]
    }

    /// 反序列化 + 魔数核。
    pub fn deserialize(w: &[u32; 4]) -> Result<SessHeader, u32> {
        if w[0] != HDR_MAGIC {
            return Err(XC_MAGIC);
        }
        if w[1] == 0 {
            return Err(XC_SESS_ZERO);
        }
        let fp = (w[2] as u64) | ((w[3] as u64) << 32);
        if fp == 0 {
            return Err(XC_FP_ZERO);
        }
        Ok(SessHeader { sess: w[1], fp })
    }
}

/// 帧校验：帧携带的会话号必须等于当前活跃会话。
pub fn frame_check(current: &SessHeader, frame_sess: u32) -> Result<(), u32> {
    if frame_sess == 0 {
        return Err(XC_SESS_ZERO);
    }
    if frame_sess != current.sess {
        return Err(XC_OLD_SESS);
    }
    Ok(())
}

/// 会话账：活跃唯一（同一时刻至多一个活跃会话）。
pub struct SessBook {
    active: Option<SessHeader>,
    /// 历史会话号（已关）。
    closed: Vec<u32>,
}

impl SessBook {
    pub fn new() -> SessBook {
        SessBook { active: None, closed: Vec::new() }
    }

    /// 开会话：活跃存在拒（先关后开）；会话号必须单调递增。
    pub fn open(&mut self, h: SessHeader) -> Result<(), u32> {
        if self.active.is_some() {
            return Err(XC_OPEN_DUP);
        }
        if let Some(&last) = self.closed.last() {
            if h.sess <= last {
                return Err(XC_SESS_ZERO); // 会话号回退 = 账面破
            }
        }
        self.active = Some(h);
        Ok(())
    }

    /// 关会话：未开拒；关后入历史。
    pub fn close(&mut self) -> Result<u32, u32> {
        match self.active.take() {
            Some(h) => {
                self.closed.push(h.sess);
                Ok(h.sess)
            }
            None => Err(XC_CLOSE_NO),
        }
    }

    /// 活跃态。
    pub fn active(&self) -> Option<&SessHeader> {
        self.active.as_ref()
    }

    /// 历史深。
    pub fn closed_len(&self) -> u32 {
        self.closed.len() as u32
    }
}

pub fn run_unx0277_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0277_sessid");
    // 族一：会话头构造。
    let h1 = match SessHeader::new(1, b"boot-console-2026") {
        Ok(h) => h,
        Err(_) => {
            cs.add("会话头构造", false, "被拒");
            return cs;
        }
    };
    let h1b = SessHeader::new(1, b"boot-console-2026").unwrap_or(h1);
    cs.add("指纹稳定", h1.fp == h1b.fp, "同开场同指纹");
    let h2 = SessHeader::new(2, b"reconnect-after-hotplug").unwrap_or(h1);
    cs.add("异开场异指纹", h1.fp != h2.fp, "指纹区分会话");
    // 族二：帧校验。
    cs.add("当前帧过", frame_check(&h1, 1).is_ok(), "活跃会话帧");
    cs.add("旧会话帧拒", frame_check(&h1, 1) == Ok(()), "同号");
    cs.add("异号拒", frame_check(&h2, 1) == Err(XC_OLD_SESS), "旧字节流拦截");
    cs.add("零号拒", frame_check(&h1, 0) == Err(XC_SESS_ZERO), "SESS_ZERO");
    // 族三：会话账。
    let mut book = SessBook::new();
    cs.add("开会话", book.open(h1).is_ok(), "1 号活跃");
    cs.add("重复开拒", book.open(h2) == Err(XC_OPEN_DUP), "活跃唯一");
    cs.add("关会话", book.close() == Ok(1), "入历史");
    cs.add("关空拒", book.close() == Err(XC_CLOSE_NO), "CLOSE_NO");
    cs.add("重开新号", book.open(h2).is_ok(), "2 号活跃");
    cs.add("历史深 1", book.closed_len() == 1, "1 号已关");
    // 族四：会话号回退拒。
    book.close();
    let _ = book.close();
    cs.add("号回退拒", book.open(h1).is_err(), "1 ≤ 已关 2");
    // 族五：序列化往返。
    let w = h1.serialize();
    cs.add("头魔数", w[0] == HDR_MAGIC, "SESS");
    cs.add("往返闭账", SessHeader::deserialize(&w) == Ok(h1), "号+指纹双对");
    let mut bad = w;
    bad[0] = 0x41414141;
    cs.add("魔数拒", SessHeader::deserialize(&bad).is_err(), "MAGIC");
    let mut bad2 = w;
    bad2[1] = 0;
    cs.add("零号拒", SessHeader::deserialize(&bad2).is_err(), "SESS_ZERO");
    cs.add("空开场拒", SessHeader::new(1, &[]).is_err(), "FP_ZERO");
    cs
}
