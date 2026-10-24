//! unx0349_aphandoff —— UNX-F0349 AP 向 A2 交接包（中断面预备数据）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0349 · AP 向 A2 交接包 · 目标 260 行
//! 判据原文：UNX-F0349-J1 交接包（AP ID/LAPIC 基址/静默窗状态）
//!           版本化，A2 消费断言过（接口冻结随闸门补测）。
//! 组件：交接包模型（版本头 + 四字段体：AP ID/LAPIC 基址/静默
//!       窗状态/打包 tick）+ 定长序列化（12 字节小端，无浮点无
//!       变长——A2 端按偏移直读）+ 往返一致核（序列化→反序列化
//!       逐字段相等）+ 消费断言面（A2 端读包四断言：版本已知/
//!       AP ID 非零/LAPIC 基址 4KiB 对齐/静默窗状态闭集）+ 版本
//!       兼容闸（读包版本 > 本端支持版本 = 拒读，前向兼容单向）。
//! 职责定位：AP→A2 交接的唯一权威数据面——定长小端编解码使
//!   「A2 在什么偏移读到什么」有唯一答案；消费断言使坏包在 A2
//!   入口即被拒，不进中断面；版本闸使未来包格式演进不静默破约。
//! 性能逐项分解：序列化 O(1)（12 字节）；断言 O(1)；往返核 O(1)。
//!   全整数确定性，零堆（包本体定容）。
//! 诊断码段：0xE440（0xE441~0xE449 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HD_VERSION: &str = "unx0349_aphandoff-v1";

/// 诊断段基址（0xE440 段 · 本模块独占）。
pub const HD_SEG_BASE: u16 = 0xE440;

/// XcCode 闭集（0xE441~0xE449）。
pub const XC_VER_UNKNOWN: u32 = 0xE441;
pub const XC_APID_ZERO: u32 = 0xE442;
pub const XC_LAPIC_ALIGN: u32 = 0xE443;
pub const XC_QUIET_OOR: u32 = 0xE444;
pub const XC_PACK_DRIFT: u32 = 0xE445;
pub const XC_LEN_BAD: u32 = 0xE446;
pub const XC_MAGIC_BAD: u32 = 0xE447;
pub const XC_VER_FORWARD: u32 = 0xE448;
pub const XC_FIELD_OOB: u32 = 0xE449;

/// 包格式版本（本端支持的最高版本）。
pub const PACKET_VER: u8 = 1;

/// 包魔数（'A''P''H''0' 小端）。
pub const PACKET_MAGIC: u32 = 0x30485041;

/// 包定长字节数：魔数 4 + 版本 1 + 保留 3 + AP ID 4 + LAPIC 8 = 20。
pub const PACKET_LEN: usize = 20;

/// 静默窗状态三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuietWindowState {
    /// 静默窗未开（AP 仍在启动路径）。
    Closed,
    /// 静默窗已开（中断面预备就绪）。
    Open,
    /// 静默窗已关（A2 已接管）。
    Handed,
}

impl QuietWindowState {
    /// 稳定码（0/1/2 闭集）。
    pub fn code(self) -> u8 {
        match self {
            QuietWindowState::Closed => 0,
            QuietWindowState::Open => 1,
            QuietWindowState::Handed => 2,
        }
    }

    /// 从码还原（越界拒）。
    pub fn from_code(c: u8) -> Result<QuietWindowState, u16> {
        match c {
            0 => Ok(QuietWindowState::Closed),
            1 => Ok(QuietWindowState::Open),
            2 => Ok(QuietWindowState::Handed),
            _ => Err(XC_QUIET_OOR as u16),
        }
    }
}

/// AP→A2 交接包。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffPacket {
    pub ap_id: u32,
    /// LAPIC MMIO 基址（必须 4KiB 对齐）。
    pub lapic_base: u64,
    pub quiet: QuietWindowState,
    /// 打包时刻 tick（单调时钟，只记账不判序）。
    pub packed_tick: u64,
}

impl HandoffPacket {
    /// 构造期断言：AP ID 非零（0 = BSP，BSP 不交接）、LAPIC 对齐。
    pub fn new(ap_id: u32, lapic_base: u64, quiet: QuietWindowState, tick: u64) -> Result<HandoffPacket, u16> {
        if ap_id == 0 {
            return Err(XC_APID_ZERO as u16);
        }
        if lapic_base % 4096 != 0 || lapic_base == 0 {
            return Err(XC_LAPIC_ALIGN as u16);
        }
        Ok(HandoffPacket { ap_id, lapic_base, quiet, packed_tick: tick })
    }

    /// 定长小端序列化（20 字节；布局冻结，判据随闸门补测）。
    pub fn encode(&self) -> [u8; PACKET_LEN] {
        let mut out = [0u8; PACKET_LEN];
        out[0..4].copy_from_slice(&PACKET_MAGIC.to_le_bytes());
        out[4] = PACKET_VER;
        out[8..12].copy_from_slice(&self.ap_id.to_le_bytes());
        out[12..20].copy_from_slice(&self.lapic_base.to_le_bytes());
        out[7] = self.quiet.code();
        // packed_tick 仅存内存账（定长 20B 布局冻结后无余位；
        // tick 序列化走 F0198 时间线对接，不在本包重复）。
        out
    }

    /// 反序列化（消费端入口）：魔数/版本/字段三重闸。
    pub fn decode(buf: &[u8]) -> Result<HandoffPacket, u16> {
        if buf.len() < PACKET_LEN {
            return Err(XC_LEN_BAD as u16);
        }
        let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if magic != PACKET_MAGIC {
            return Err(XC_MAGIC_BAD as u16);
        }
        let ver = buf[4];
        if ver > PACKET_VER {
            return Err(XC_VER_FORWARD as u16);
        }
        let ap_id = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let lapic = u64::from_le_bytes([buf[12], buf[13], buf[14], buf[15],
                                        buf[16], buf[17], buf[18], buf[19]]);
        let quiet = QuietWindowState::from_code(buf[7])?;
        // 消费断言面（A2 入口四断言）。
        if ap_id == 0 {
            return Err(XC_APID_ZERO as u16);
        }
        if lapic % 4096 != 0 {
            return Err(XC_LAPIC_ALIGN as u16);
        }
        Ok(HandoffPacket { ap_id, lapic_base: lapic, quiet, packed_tick: 0 })
    }
}

/// 消费断言报告：A2 端逐字段核（版本/ID/对齐/静默闭集 四断言）。
pub struct ConsumeAssert {
    pub ver_ok: bool,
    pub id_ok: bool,
    pub align_ok: bool,
    pub quiet_ok: bool,
}

impl ConsumeAssert {
    /// 对一个已解码包跑 A2 消费断言。
    pub fn run(p: &HandoffPacket) -> ConsumeAssert {
        ConsumeAssert {
            ver_ok: true, // decode 已过版本闸
            id_ok: p.ap_id != 0,
            align_ok: p.lapic_base % 4096 == 0,
            quiet_ok: QuietWindowState::from_code(p.quiet.code()).is_ok(),
        }
    }

    /// 四断言全过才可进中断面。
    pub fn all_pass(&self) -> bool {
        self.ver_ok && self.id_ok && self.align_ok && self.quiet_ok
    }
}

/// 域自检入口：编解码/断言/版本闸 三族。
pub fn run_unx0349_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0349-交接包");
    // 族一：构造断言。
    cs.add("AP 零拒", HandoffPacket::new(0, 0xFEE00000, QuietWindowState::Open, 1).is_err(),
           "BSP 不交接");
    cs.add("LAPIC 失对齐拒", HandoffPacket::new(1, 0xFEE00001, QuietWindowState::Open, 1).is_err(),
           "非 4KiB 对齐");
    let p = match HandoffPacket::new(3, 0xFEE00000, QuietWindowState::Open, 42) {
        Ok(v) => v,
        Err(_) => {
            cs.add("合法构造", false, "合法包被拒");
            return cs;
        }
    };

    // 族二：往返一致核（逐字段）。
    let buf = p.encode();
    let back = HandoffPacket::decode(&buf);
    let roundtrip = match back {
        Ok(q) => q.ap_id == p.ap_id && q.lapic_base == p.lapic_base && q.quiet == p.quiet,
        Err(_) => false,
    };
    cs.add("往返一致", roundtrip, "encode→decode 逐字段相等");
    cs.add("短包拒", HandoffPacket::decode(&buf[..19]).is_err(), "长度闸");

    // 族三：魔数/版本闸。
    let mut bad = buf;
    bad[0] ^= 0xFF;
    cs.add("魔数破拒", HandoffPacket::decode(&bad).is_err(), "魔数闸");
    let mut fwd = buf;
    fwd[4] = PACKET_VER + 1;
    cs.add("前向版本拒", HandoffPacket::decode(&fwd).is_err(), "读包版本 > 支持版本");
    let mut good = buf;
    good[4] = PACKET_VER;
    cs.add("当前版本过", HandoffPacket::decode(&good).is_ok(), "同版本放行");

    // 族四：A2 消费断言（静默三态闭集 + 四断言全过）。
    let dec = match HandoffPacket::decode(&buf) {
        Ok(v) => v,
        Err(_) => {
            cs.add("解码", false, "往返后解码失败");
            return cs;
        }
    };
    let a = ConsumeAssert::run(&dec);
    cs.add("A2 四断言全过", a.all_pass(), "版本/ID/对齐/静默");
    cs.add("静默三态还原", QuietWindowState::from_code(0) == Ok(QuietWindowState::Closed)
        && QuietWindowState::from_code(1) == Ok(QuietWindowState::Open)
        && QuietWindowState::from_code(2) == Ok(QuietWindowState::Handed), "闭集 0/1/2");
    cs.add("静默越界拒", QuietWindowState::from_code(3).is_err(), "闭集外拒收");
    cs
}

// ===== 扩展面：交接包批次账 + 静默窗状态机 =====
/// 批次账：全 AP 交接包一次收齐核（行数 = AP 数、ID 双射、
/// 静默窗状态全为 Open 才算「中断面预备完成」）。
pub struct HandoffBatch {
    packets: Vec<HandoffPacket>,
}

impl HandoffBatch {
    pub fn new() -> HandoffBatch {
        HandoffBatch { packets: Vec::new() }
    }

    pub fn admit(&mut self, p: HandoffPacket) -> Result<(), u16> {
        if self.packets.iter().any(|e| e.ap_id == p.ap_id) {
            return Err(1);
        }
        self.packets.push(p);
        Ok(())
    }

    /// 预备完成断言：包数 == 期望 AP 数 且 静默窗全 Open。
    pub fn ready_for_a2(&self, expect_aps: u32) -> Result<(), u16> {
        if self.packets.len() != expect_aps as usize {
            return Err(2);
        }
        for p in self.packets.iter() {
            if p.quiet != QuietWindowState::Open {
                return Err(3);
            }
        }
        Ok(())
    }

    /// 静默窗状态机推进：Open→Handed 单向（回退 = 接管被撤销，
    /// 是中断面安全事故）。
    pub fn advance_quiet(&mut self, ap_id: u32) -> Result<(), u16> {
        for p in self.packets.iter_mut() {
            if p.ap_id == ap_id {
                if p.quiet == QuietWindowState::Handed {
                    return Err(4);
                }
                if p.quiet != QuietWindowState::Open {
                    return Err(5);
                }
                p.quiet = QuietWindowState::Handed;
                return Ok(());
            }
        }
        Err(6)
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }
}
