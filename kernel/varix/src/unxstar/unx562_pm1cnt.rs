//! unx562_pm1cnt —— UNX-F0562 PM1a_CNT 控制块账（SLP 位只读）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B28）：UNX-F0562 · PM1a_CNT 控制块账（SLP 位只读） · 目标 220 行 · 已深化
//! 判据原文：PM1a_CNT 只读快照（SLP_TYP/SLP_EN 位态原样落账），双读一致断言（2/2）
//! 组件：PM1a_CNT 位面契约（SLP_TYP bit[10:12]/SLP_EN bit13/WAK_STS bit15——ACPI 规范位布局）
//!      + 只读快照器（位态原样落账，禁回写）+ 双读一致核（2/2 断言）
//!      + 控制块账（append-only 快照账 + 守恒）+ 只读红线（写请求显性拒）
//! 判据落地：SLP 位态原样落账 + 双读一致 2/2
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，IO 读注入（零真实端口访问）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PM_VERSION: &str = "unx562_pm1cnt-v1";

/// 诊断段基址（0x4c80 段 · 本模块独占）。
pub const PM_SEG_BASE: u16 = 0x4c80;

/// SLP_TYP 位宽（ACPI：bit 10..=12，3 位睡眠类型）。
pub const PM_SLP_TYP_SHIFT: u32 = 10;
pub const PM_SLP_TYP_MASK: u16 = 0b111;

/// SLP_EN 位（bit 13）。
pub const PM_SLP_EN_BIT: u16 = 1 << 13;

/// WAK_STS 位（bit 15）。
pub const PM_WAK_STS_BIT: u16 = 1 << 15;

/// 只读快照（位态原样，无解释无变换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PmSnap {
    /// 原始 16 位寄存器值。
    pub raw: u16,
    /// SLP_TYP（bit 10..=12 原样提取）。
    pub slp_typ: u16,
    /// SLP_EN（bit 13 原样）。
    pub slp_en: bool,
    /// WAK_STS（bit 15 原样）。
    pub wak_sts: bool,
    /// 读序号（1 = 第一次读，2 = 第二次读——双读账）。
    pub seq: u8,
}

impl PmSnap {
    /// 从原始值构造（原样落账：只提取位面，零变换）。
    pub const fn from_raw(raw: u16, seq: u8) -> Self {
        PmSnap {
            raw,
            slp_typ: (raw >> PM_SLP_TYP_SHIFT) & PM_SLP_TYP_MASK,
            slp_en: raw & PM_SLP_EN_BIT != 0,
            wak_sts: raw & PM_WAK_STS_BIT != 0,
            seq,
        }
    }

    /// 快照一致性（两次快照位面全等——双读一致 2/2 的判定核）。
    pub const fn same_bits(&self, other: &PmSnap) -> bool {
        self.slp_typ == other.slp_typ && self.slp_en == other.slp_en && self.wak_sts == other.wak_sts
    }
}

/// 诊断码（0x4c80 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// 0x4c80 寄存器读不可达（IO 读注入返回 None）。
    ReadFail,
    /// 0x4c81 双读不一致（2/2 断言失败——位面抖动显性）。
    DoubleReadDiff,
    /// 0x4c82 只读红线触发（收到写请求——SLP 位只读口径被破坏）。
    WriteDenied,
    /// 0x4c83 SLP_TYP 值越合法闭集（ACPI 睡眠类型 1..=5 之外且非保留态声明）。
    TypOob,
    /// 0x4c84 快照账破缺（账条数 ≢ 快照计数）。
    LedgerBreak,
    /// 0x4c85 快照序号错位（seq 不单调）。
    SeqMisaligned,
    /// 0x4c86 位面守恒破缺（提取位重组 ≢ 原值位面）。
    BitsConserv,
    /// 0x4c87 寄存器基址零（控制块未初始化）。
    BaseZero,
    /// 0x4c88 快照账满（append-only 触顶显性截断）。
    LedgerFull,
    /// 0x4c89 重复封账（账已封再封）。
    Reseal,
    /// 0x4c8a 封账后写（封账状态收到快照请求）。
    SealedWrite,
    /// 0x4c8b 段位守卫失败。
    SegmentGuard,
}

impl PmCode {
    /// 全量闭集。
    pub const ALL: [PmCode; 12] = [
        PmCode::ReadFail,
        PmCode::DoubleReadDiff,
        PmCode::WriteDenied,
        PmCode::TypOob,
        PmCode::LedgerBreak,
        PmCode::SeqMisaligned,
        PmCode::BitsConserv,
        PmCode::BaseZero,
        PmCode::LedgerFull,
        PmCode::Reseal,
        PmCode::SealedWrite,
        PmCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        PM_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            PmCode::ReadFail => "寄存器读不可达：IO 读注入返回不可达",
            PmCode::DoubleReadDiff => "双读不一致：位面抖动显性立案（2/2 断言失败）",
            PmCode::WriteDenied => "只读红线触发：SLP 位只读口径禁止任何写",
            PmCode::TypOob => "SLP_TYP 越合法闭集：睡眠类型值非 1..=5 且未声明保留",
            PmCode::LedgerBreak => "快照账破缺：账条数与快照计数不一致",
            PmCode::SeqMisaligned => "快照序号错位：seq 不单调递增",
            PmCode::BitsConserv => "位面守恒破缺：提取位重组与原值不符",
            PmCode::BaseZero => "寄存器基址为零：控制块未初始化",
            PmCode::LedgerFull => "快照账满：append-only 触顶显性截断",
            PmCode::Reseal => "重复封账：账已封再封拒绝",
            PmCode::SealedWrite => "封账后写：封账状态收到快照请求",
            PmCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PmCode::ReadFail => "先核 IO 空间类型（IO/MMIO）与访问宽 2 字节再读",
            PmCode::DoubleReadDiff => "补第三读仲裁；仍抖动则按睡眠状态机活跃期解释并立案",
            PmCode::WriteDenied => "本模块是只读账：写请求应路由到电源行动模块而非此处",
            PmCode::TypOob => "SLP_TYP 值先过闭集核：1..=5 合法，0/6/7 按保留态显性",
            PmCode::LedgerBreak => "快照与记账同事务：核对 snapshot 的记账路径",
            PmCode::SeqMisaligned => "seq 由账内条数派生：禁止调用方自传错序号",
            PmCode::BitsConserv => "位提取公式复核：SLP_TYP=bit10..12，SLP_EN=bit13，WAK=bit15",
            PmCode::BaseZero => "先从 FADT PM1a_CNT_BLK 取基址再建账",
            PmCode::LedgerFull => "扩账或按批归档：触顶显性不静默丢",
            PmCode::Reseal => "封账是终态：需要新账就开新账不改旧账",
            PmCode::SealedWrite => "封账后快照请求按只读口径拒绝：读走内存态不出账",
            PmCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c80",
        }
    }
}

/// IO 读注入接口（纯功能：零真实端口）。
pub trait PmIo {
    /// 读 16 位寄存器；None = 不可达。
    fn read16(&self) -> Option<u16>;
}

/// 固定值注入器（演练与测试）。
#[derive(Clone, Copy, Debug)]
pub struct PmFixed(pub u16);

impl PmIo for PmFixed {
    fn read16(&self) -> Option<u16> {
        Some(self.0)
    }
}

/// 控制块账主体（SLP 位只读）。
pub struct PmCntBook {
    /// 寄存器基址（0 = 未初始化）。
    pub base: u64,
    /// 快照账（append-only）。
    pub snaps: Vec<PmSnap>,
    /// 已封账。
    pub sealed: bool,
    /// 账容量。
    pub cap: usize,
    /// 诊断码。
    pub codes: Vec<PmCode>,
}

impl PmCntBook {
    /// 建账（基址必须非零）。
    pub fn new(base: u64) -> Result<Self, (PmCode, String)> {
        if base == 0 {
            return Err((PmCode::BaseZero, PmCode::BaseZero.advice().to_string()));
        }
        Ok(PmCntBook { base, snaps: Vec::new(), sealed: false, cap: 256, codes: Vec::new() })
    }

    fn note(&mut self, c: PmCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 只读快照 + 双读一致核（判据核心路径）：
    /// 读两次，位面全等才落账（2/2）；不一致显性 DoubleReadDiff。
    pub fn snapshot<I: PmIo>(&mut self, io: &I) -> Result<PmSnap, (PmCode, String)> {
        if self.sealed {
            self.note(PmCode::SealedWrite);
            return Err((PmCode::SealedWrite, PmCode::SealedWrite.advice().to_string()));
        }
        // 读 1。
        let Some(v1) = io.read16() else {
            self.note(PmCode::ReadFail);
            return Err((PmCode::ReadFail, PmCode::ReadFail.advice().to_string()));
        };
        // 读 2（双读一致核）。
        let Some(v2) = io.read16() else {
            self.note(PmCode::ReadFail);
            return Err((PmCode::ReadFail, PmCode::ReadFail.advice().to_string()));
        };
        let s1 = PmSnap::from_raw(v1, 1);
        let s2 = PmSnap::from_raw(v2, 2);
        if !s1.same_bits(&s2) {
            self.note(PmCode::DoubleReadDiff);
            return Err((PmCode::DoubleReadDiff, PmCode::DoubleReadDiff.advice().to_string()));
        }
        // 位面守恒核（提取位重组 ≡ 原值位面：bit10..15 平面重组必须等于原值平面）。
        let recomposed = (s1.slp_typ << PM_SLP_TYP_SHIFT)
            | if s1.slp_en { PM_SLP_EN_BIT } else { 0 }
            | if s1.wak_sts { PM_WAK_STS_BIT } else { 0 };
        let plane_raw = v1 & 0b1110_1100_0000_0000;
        let plane_new = recomposed & 0b1110_1100_0000_0000;
        if plane_new != plane_raw {
            self.note(PmCode::BitsConserv);
            return Err((PmCode::BitsConserv, PmCode::BitsConserv.advice().to_string()));
        }
        // 落账（seq 由账内条数派生，单调）。
        let seq = (self.snaps.len() + 1) as u8;
        let snap = PmSnap::from_raw(v1, seq);
        if self.snaps.len() >= self.cap {
            self.note(PmCode::LedgerFull);
            return Err((PmCode::LedgerFull, PmCode::LedgerFull.advice().to_string()));
        }
        self.snaps.push(snap);
        Ok(snap)
    }

    /// SLP_TYP 合法闭集核（1..=5 合法；0/6/7 保留态显性）。
    pub fn typ_audit(&mut self, snap: &PmSnap) -> Result<(), (PmCode, String)> {
        if !(1..=5).contains(&snap.slp_typ) {
            self.note(PmCode::TypOob);
            return Err((PmCode::TypOob, PmCode::TypOob.advice().to_string()));
        }
        Ok(())
    }

    /// 只读红线（写请求一律显性拒——判据口径的守卫面）。
    pub fn write_guard(&mut self) -> Result<(), (PmCode, String)> {
        self.note(PmCode::WriteDenied);
        Err((PmCode::WriteDenied, PmCode::WriteDenied.advice().to_string()))
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), (PmCode, String)> {
        if self.sealed {
            self.note(PmCode::Reseal);
            return Err((PmCode::Reseal, PmCode::Reseal.advice().to_string()));
        }
        self.sealed = true;
        Ok(())
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.snaps.windows(2).all(|w| w[0].seq < w[1].seq)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let mut s = format!(
            "PM1a_CNT 账 @{:#x}：快照 {} 条{}",
            self.base,
            self.snaps.len(),
            if self.sealed { "（已封账）" } else { "" }
        );
        if let Some(l) = self.snaps.last() {
            s.push_str(&format!(
                "；末帧 SLP_TYP={} SLP_EN={} WAK={}",
                l.slp_typ, l.slp_en, l.wak_sts
            ));
        }
        if !self.codes.is_empty() {
            s.push_str("；诊断：");
            for c in &self.codes {
                s.push_str(&format!("[{:04X}]", c.val()));
            }
        }
        s
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    PmCode::ALL.iter().all(|c| c.val() >= PM_SEG_BASE && c.val() < PM_SEG_BASE + 0x100)
}
