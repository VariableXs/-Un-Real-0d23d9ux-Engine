//! veb267_nvvdec —— VE-F0267 NVIDIA NVDEC/NVENC 能力位。
//! 锚点（CoRun Unxreal 主册）：VE-F0267 · NVIDIA NVDEC/NVENC 能力位 · 目标 320 行
//! 判据原文：VE-F0267-J1 编解码能力位表：NVDEC（解码）与
//!           NVENC（编码）各一套编解码器位图（H264/HEVC/AV1/
//!           VP9）；代际裁剪（老代际无 AV1）；会话数上限（每
//!           编解码器并发会话）；能力查询核（位图与代际表一
//!           致）；会话开闭账（开 = 关 + 活跃）。
//! 组件：能力位图 + 代际裁剪表 + 会话账。
//! 职责定位：编解码能力位是「硬解硬编的菜单」——菜没上的代际
//!           点了就是黑屏；AV1 是新菜（Ampere 解码才有，编码
//!           Ada 才有）；会话数是厨房容量：4K 多路并发挤爆就
//!           全卡，不如早拒。
//! 性能逐项分解：能力查 O(1)；会话账 O(1)。
//! 诊断码段：0x30BC0（0x30BC1~0x30BC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const CK_VERSION: &str = "veb267_nvvdec-v1";

/// 诊断段基址（0x30BC0 段 · 本模块独占）。
pub const CK_SEG_BASE: u32 = 0x30BC0;
pub const XC_NO_CODEC: u32 = 0x30BC1; // 代际无此编解码器
pub const XC_SESS_CAP: u32 = 0x30BC2; // 会话满
pub const XC_MAGIC: u32 = 0x30BC3; // 账魔数破
pub const XC_GEN_OOR: u32 = 0x30BC4; // 代际越域
pub const XC_NOENT: u32 = 0x30BC5; // 无此会话
pub const XC_IMBAL: u32 = 0x30BC6; // 会话账不平
pub const XC_CODEC_OOR: u32 = 0x30BC7; // 编解码器越域
pub const XC_SESS_ZERO: u32 = 0x30BC8; // 会话零
pub const XC_WRONG_DIR: u32 = 0x30BC9; // 解码器开编码会话

/// 编解码器位。
pub const C_H264: u32 = 1 << 0;
pub const C_HEVC: u32 = 1 << 1;
pub const C_VP9: u32 = 1 << 2;
pub const C_AV1: u32 = 1 << 3;

/// 会话上限（每方向）。
pub const SESS_CAP: u32 = 8;

/// 代际（复用 veb261 语义，本地枚举防跨模块耦合）。
pub type GenNum = u32;

/// 代际能力表（dec 位图，enc 位图）。
pub fn caps_for(gen: GenNum) -> Option<(u32, u32)> {
    // Pascal=3, Turing=4, Ampere=5, Ada=6。
    match gen {
        3 => Some((C_H264 | C_HEVC | C_VP9, C_H264 | C_HEVC)),
        4 => Some((C_H264 | C_HEVC | C_VP9, C_H264 | C_HEVC)),
        5 => Some((C_H264 | C_HEVC | C_VP9 | C_AV1, C_H264 | C_HEVC)),
        6 => Some((C_H264 | C_HEVC | C_VP9 | C_AV1, C_H264 | C_HEVC | C_AV1)),
        _ => None,
    }
}

/// 方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// 解码 NVDEC。
    Dec = 0,
    /// 编码 NVENC。
    Enc = 1,
}

/// 会话账。
pub struct CodecSess {
    gen: GenNum,
    dec_active: u32,
    enc_active: u32,
    dec_opened: u64,
    enc_opened: u64,
    dec_closed: u64,
    enc_closed: u64,
    magic: u32,
}

impl CodecSess {
    /// 建。
    pub fn new(gen: GenNum) -> Result<CodecSess, u32> {
        if caps_for(gen).is_none() {
            return Err(XC_GEN_OOR);
        }
        Ok(CodecSess {
            gen, dec_active: 0, enc_active: 0,
            dec_opened: 0, enc_opened: 0, dec_closed: 0, enc_closed: 0,
            magic: 0x0BC0_C0D3,
        })
    }

    /// 开会话。
    pub fn open(&mut self, dir: Dir, codec: u32) -> Result<u32, u32> {
        if self.magic != 0x0BC0_C0D3 {
            return Err(XC_MAGIC);
        }
        let (dec, enc) = match caps_for(self.gen) {
            Some(c) => c,
            None => return Err(XC_GEN_OOR),
        };
        let bitmap = match dir {
            Dir::Dec => dec,
            Dir::Enc => enc,
        };
        if codec & !(C_H264 | C_HEVC | C_VP9 | C_AV1) != 0 {
            return Err(XC_CODEC_OOR);
        }
        if bitmap & codec == 0 {
            return Err(XC_NO_CODEC);
        }
        match dir {
            Dir::Dec => {
                if self.dec_active >= SESS_CAP {
                    return Err(XC_SESS_CAP);
                }
                self.dec_active += 1;
                self.dec_opened += 1;
            }
            Dir::Enc => {
                if self.enc_active >= SESS_CAP {
                    return Err(XC_SESS_CAP);
                }
                self.enc_active += 1;
                self.enc_opened += 1;
            }
        }
        Ok(match dir {
            Dir::Dec => self.dec_active,
            Dir::Enc => self.enc_active,
        })
    }

    /// 关会话。
    pub fn close(&mut self, dir: Dir) -> Result<u32, u32> {
        match dir {
            Dir::Dec => {
                if self.dec_active == 0 {
                    return Err(XC_SESS_ZERO);
                }
                self.dec_active -= 1;
                self.dec_closed += 1;
                Ok(self.dec_active)
            }
            Dir::Enc => {
                if self.enc_active == 0 {
                    return Err(XC_SESS_ZERO);
                }
                self.enc_active -= 1;
                self.enc_closed += 1;
                Ok(self.enc_active)
            }
        }
    }

    /// 会话账核。
    pub fn audit(&self) -> Result<(u32, u32), u32> {
        if self.magic != 0x0BC0_C0D3 {
            return Err(XC_MAGIC);
        }
        if self.dec_opened - self.dec_closed != self.dec_active as u64 {
            return Err(XC_IMBAL);
        }
        if self.enc_opened - self.enc_closed != self.enc_active as u64 {
            return Err(XC_IMBAL);
        }
        Ok((self.dec_active, self.enc_active))
    }

    /// 活跃总数。
    pub fn active_total(&self) -> u32 {
        self.dec_active + self.enc_active
    }
}

pub fn run_veb267_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0267");
    // 族一：代际能力表。
    let p3 = caps_for(3);
    s.add("Pascal 无 AV1 解", matches!(p3, Some((d, _)) if d & C_AV1 == 0), "老代");
    s.add("Pascal 无 AV1 编", matches!(p3, Some((_, e)) if e & C_AV1 == 0), "全无");
    let a5 = caps_for(5);
    s.add("Ampere AV1 解有", matches!(a5, Some((d, _)) if d & C_AV1 != 0), "新代");
    s.add("Ampere AV1 编无", matches!(a5, Some((_, e)) if e & C_AV1 == 0), "编码 Ada 才有");
    let a6 = caps_for(6);
    s.add("Ada AV1 双全", matches!(a6, Some((d, e)) if d & C_AV1 != 0 && e & C_AV1 != 0), "全通");
    s.add("代际 2 无表", caps_for(2).is_none(), "Maxwell 不辖");
    // 族二：会话开闭。
    let mut c = match CodecSess::new(5) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败", false, "不应到");
            return s;
        }
    };
    s.add("开 H264 解", c.open(Dir::Dec, C_H264) == Ok(1), "一会话");
    s.add("开 HEVC 解", c.open(Dir::Dec, C_HEVC) == Ok(2), "二会话");
    s.add("开 AV1 编拒", c.open(Dir::Enc, C_AV1) == Err(XC_NO_CODEC), "Ampere 编码无 AV1");
    s.add("开 HEVC 编", c.open(Dir::Enc, C_HEVC) == Ok(1), "一编会话");
    s.add("账 2+1", c.audit() == Ok((2, 1)), "三活跃");
    s.add("关解一", c.close(Dir::Dec) == Ok(1), "回收");
    s.add("账 1+1", c.audit() == Ok((1, 1)), "两活跃");
    // 族三：会话满。
    let mut f = match CodecSess::new(6) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 2", false, "不应到");
            return s;
        }
    };
    for _ in 0..8 {
        let _ = f.open(Dir::Dec, C_H264);
    }
    s.add("解会话满 8", f.open(Dir::Dec, C_H264) == Err(XC_SESS_CAP), "顶格");
    s.add("编会话独立", f.open(Dir::Enc, C_AV1) == Ok(1), "编不受解满影响");
    s.add("活跃 9", f.active_total() == 9, "8+1");
    // 族四：空关与代际。
    let mut z = match CodecSess::new(4) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 3", false, "不应到");
            return s;
        }
    };
    s.add("空关拒", z.close(Dir::Enc) == Err(XC_SESS_ZERO), "无会话");
    s.add("老代建拒", matches!(CodecSess::new(2), Err(XC_GEN_OOR)), "无表");
    s.add("魔数核", CK_SEG_BASE == 0x30BC0, "段基址");
    s
}
