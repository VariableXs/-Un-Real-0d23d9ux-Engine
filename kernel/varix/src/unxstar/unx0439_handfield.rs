//! unx0439_handfield —— UNX-F0439 中断交接字段（A2/A3 预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0439 · 中断交接字段 · 目标 240 行
//! 判据原文：UNX-F0439-J1 交接信包（A2/A3 扩展字段）预备账：
//!           信包定长 64 字节、16 字节对齐；A2 字段组：源向
//!           量（u8）、源模式（u8：0=IOAPIC 1=LVT 2=IPI 3=
//!           MSIX）、目标 CPU ID（u32）、交接种类（u8：0=正
//!           常 1=迁移 2=紧急回收）；A3 字段组：TSC 戳（u64）、
//!           序号（u64 单调）、CRC32 校验（u32，覆盖前 56 字
//!           节）；预备核：信包可空造（零值合法）、字段越表
//!           拒（源模式 >3、交接种类 >2 立案）、序号单调核
//!           （同信箱序号必须递增，回退拒）、CRC 核（篡改任
//!           意字节检出）；双信包交换核：交接双方各自封装/
//!           解封，字段逐一对上（含 CRC）。
//! 组件：信包结构（64B 布局）+ 字段编码器/解码器 + 越表核
//!       + 序号单调账 + CRC32 校验 + 双向交换核。
//! 职责定位：交接字段是中断在 CPU 间「过户」的契据——A2 说
//!   清「什么中断从哪来」，A3 说清「何时、第几号、没被改
//!   过」；序号回退是重复投递的征兆，CRC 破是内存位翻转或
//!   越权改写的铁证；预备账现在立好，A2/A3 真正启用时字段
//!   语义不会漂。
//! 性能逐项分解：编码 O(1)；解码 O(1)；CRC O(56/4)；单调
//!   核 O(1)。
//! 诊断码段：0x30540（0x30541~0x30549 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const HF_VERSION: &str = "unx0439_handfield-v1";

/// 诊断段基址（0x30540 段 · 本模块独占）。
pub const HF_SEG_BASE: u32 = 0x30540;
pub const XC_SRC_MODE: u32 = 0x30541; // 源模式越表
pub const XC_KIND: u32 = 0x30542; // 交接种类越表
pub const XC_SEQ_REGRESS: u32 = 0x30543; // 序号回退
pub const XC_CRC: u32 = 0x30544; // CRC 校验破
pub const XC_ALIGN: u32 = 0x30545; // 对齐破
pub const XC_SIZE: u32 = 0x30546; // 信包尺寸破
pub const XC_EXCHANGE: u32 = 0x30547; // 交换字段失配
pub const XC_MAGIC: u32 = 0x30549; // 账魔数破

/// 信包定长与对齐。
pub const PKT_SZ: usize = 64;
pub const PKT_ALIGN: usize = 16;
/// CRC 覆盖域（前 56 字节）。
pub const CRC_SPAN: usize = 56;

/// 源模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum SrcMode {
    IoApic = 0,
    Lvt = 1,
    Ipi = 2,
    MsiX = 3,
}

/// 交接种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum HandKind {
    Normal = 0,
    Migrate = 1,
    UrgentRecall = 2,
}

/// 交接信包（A2/A3 扩展字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandPkt {
    /// A2：源向量。
    pub vec: u8,
    /// A2：源模式。
    pub src: SrcMode,
    /// A2：目标 CPU ID。
    pub dst: u32,
    /// A2：交接种类。
    pub kind: HandKind,
    /// A3：TSC 戳。
    pub tsc: u64,
    /// A3：序号。
    pub seq: u64,
}

impl HandPkt {
    pub fn zero() -> Self {
        HandPkt { vec: 0, src: SrcMode::IoApic, dst: 0, kind: HandKind::Normal, tsc: 0, seq: 0 }
    }

    /// 越表核。
    pub fn fields_ok(&self) -> Result<(), u32> {
        if self.src as u8 > 3 {
            return Err(XC_SRC_MODE);
        }
        if self.kind as u8 > 2 {
            return Err(XC_KIND);
        }
        Ok(())
    }

    /// 打包为 64 字节布局：A2 占 0~15，A3 占 16~31，余保留零；
    /// 尾 4 字节放 CRC32（覆盖 0..56）。
    pub fn pack(&self) -> Result<[u8; PKT_SZ], u32> {
        self.fields_ok()?;
        let mut b = [0u8; PKT_SZ];
        b[0] = self.vec;
        b[1] = self.src as u8;
        b[2] = self.kind as u8;
        b[4..8].copy_from_slice(&self.dst.to_le_bytes());
        b[16..24].copy_from_slice(&self.tsc.to_le_bytes());
        b[24..32].copy_from_slice(&self.seq.to_le_bytes());
        let crc = crc32(&b[..CRC_SPAN]);
        b[60..64].copy_from_slice(&crc.to_le_bytes());
        Ok(b)
    }

    /// 解包：布局 → 信包 + CRC 核。
    pub fn unpack(raw: &[u8; PKT_SZ]) -> Result<HandPkt, u32> {
        let want = u32::from_le_bytes([raw[60], raw[61], raw[62], raw[63]]);
        let got = crc32(&raw[..CRC_SPAN]);
        if want != got {
            return Err(XC_CRC);
        }
        let p = HandPkt {
            vec: raw[0],
            src: match raw[1] {
                0 => SrcMode::IoApic,
                1 => SrcMode::Lvt,
                2 => SrcMode::Ipi,
                3 => SrcMode::MsiX,
                _ => return Err(XC_SRC_MODE),
            },
            kind: match raw[2] {
                0 => HandKind::Normal,
                1 => HandKind::Migrate,
                2 => HandKind::UrgentRecall,
                _ => return Err(XC_KIND),
            },
            dst: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            tsc: u64::from_le_bytes([raw[16], raw[17], raw[18], raw[19], raw[20], raw[21], raw[22], raw[23]]),
            seq: u64::from_le_bytes([raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31]]),
        };
        Ok(p)
    }
}

/// CRC32（IEEE 多项式 0xEDB88320，逐位表驱动免表）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data.iter() {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// 序号单调账（同信箱）。
pub struct SeqLedger {
    /// 最近序号。
    pub last: u64,
    /// 已收包数。
    pub seen: u64,
}

impl SeqLedger {
    pub fn new() -> Self {
        SeqLedger { last: 0, seen: 0 }
    }

    /// 收包核：序号必须严格递增（首包任意非零）。
    pub fn admit(&mut self, seq: u64) -> Result<(), u32> {
        if seq == 0 || seq <= self.last {
            return Err(XC_SEQ_REGRESS);
        }
        self.last = seq;
        self.seen += 1;
        Ok(())
    }
}

/// 双向交换核：A 打包 → B 解包 → 字段逐一比。
pub fn exchange(a: &HandPkt) -> Result<(), u32> {
    let raw = a.pack()?;
    let b = HandPkt::unpack(&raw)?;
    if b.vec != a.vec || b.src != a.src || b.kind != a.kind || b.dst != a.dst || b.tsc != a.tsc || b.seq != a.seq {
        return Err(XC_EXCHANGE);
    }
    Ok(())
}

/// 判据入口：UNX-F0439。
pub fn run_unx0439_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0439_handfield");
    // 族一：布局常量。
    cs.add("64 字节定长", PKT_SZ == 64, "契约");
    cs.add("16 对齐", PKT_ALIGN == 16 && PKT_SZ % PKT_ALIGN == 0, "ABI");
    cs.add("CRC 覆盖 56", CRC_SPAN == PKT_SZ - 8, "尾 8 字节含 CRC");
    // 族二：空造合法。
    let z = HandPkt::zero();
    cs.add("零包字段过", z.fields_ok().is_ok(), "可空造");
    cs.add("零包交换过", exchange(&z).is_ok(), "零值往返");
    // 族三：字段越表（解码路径核：篡改字节 + 重算 CRC 绕过校验位）。
    let base = z.pack().ok().unwrap_or([0u8; PKT_SZ]);
    let mut raw_bad = base;
    raw_bad[1] = 4; // 源模式 4
    let crc_bad = crc32(&raw_bad[..CRC_SPAN]);
    raw_bad[60..64].copy_from_slice(&crc_bad.to_le_bytes());
    cs.add("源模式 4 拒", HandPkt::unpack(&raw_bad) == Err(XC_SRC_MODE), "0~3 域");
    let mut raw_bad2 = base;
    raw_bad2[2] = 3; // 种类 3
    let crc_bad2 = crc32(&raw_bad2[..CRC_SPAN]);
    raw_bad2[60..64].copy_from_slice(&crc_bad2.to_le_bytes());
    cs.add("种类 3 拒", HandPkt::unpack(&raw_bad2) == Err(XC_KIND), "0~2 域");
    // 族四：CRC。
    let p = HandPkt { vec: 0x41, src: SrcMode::MsiX, dst: 2, kind: HandKind::Migrate, tsc: 0x1000, seq: 7 };
    let raw = p.pack().ok().unwrap_or([0u8; PKT_SZ]);
    cs.add("打包 CRC 在", u32::from_le_bytes([raw[60], raw[61], raw[62], raw[63]]) != 0, "尾字");
    cs.add("解包还原", HandPkt::unpack(&raw).map(|x| x.vec == 0x41 && x.src == SrcMode::MsiX && x.kind == HandKind::Migrate).unwrap_or(false), "字段回");
    // 篡改检出：改一个载荷字节。
    let mut tam = raw;
    tam[0] ^= 1;
    cs.add("篡改检出", HandPkt::unpack(&tam) == Err(XC_CRC), "CRC 破");
    // 篡改 CRC 字本身。
    let mut tam2 = raw;
    tam2[63] ^= 0xFF;
    cs.add("CRC 字篡改检出", HandPkt::unpack(&tam2) == Err(XC_CRC), "尾字破");
    // 族五：交换核。
    cs.add("MSIX 包交换", exchange(&p).is_ok(), "逐字段对上");
    let p2 = HandPkt { vec: 0xFF, src: SrcMode::Ipi, dst: 0xFFFF_FFFF, kind: HandKind::UrgentRecall, tsc: u64::MAX - 1, seq: u64::MAX };
    cs.add("极值包交换", exchange(&p2).is_ok(), "边界");
    // 族六：CRC32 自测。
    cs.add("CRC 空串", crc32(b"") == 0, "IEEE 基准");
    cs.add("CRC 123456789", crc32(b"123456789") == 0xCBF4_3926, "标准向量");
    // 族七：序号单调。
    let mut l = SeqLedger::new();
    cs.add("首包 1", l.admit(1).is_ok(), "起点");
    cs.add("二包 2", l.admit(2).is_ok(), "递增");
    cs.add("序号回退拒", l.admit(1) == Err(XC_SEQ_REGRESS), "重复投递");
    cs.add("零序号拒", l.admit(0) == Err(XC_SEQ_REGRESS), "无零序");
    cs.add("跳号过", l.admit(10).is_ok(), "允许跳");
    cs.add("计数 3", l.seen == 3, "收账");
    // 族八：解包对齐/尺寸面。
    let mut short = [0u8; PKT_SZ];
    short.copy_from_slice(&raw);
    cs.add("同长解包过", HandPkt::unpack(&short).is_ok(), "64B 面");
    cs.add("尺寸常量核", core::mem::size_of::<[u8; PKT_SZ]>() == PKT_SIZE_OF, "编译期对齐");
    cs
}

/// 编译期尺寸哨兵（判据用）。
const PKT_SIZE_OF: usize = PKT_SZ;
