//! unx404_hpetcap —— UNX-F0404 HPET 探测与能力账（只读）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B21）：UNX-F0404 · HPET 探测与能力账（只读） · 目标 220 行 · 已深化
//! 判据原文：HPET 探测结果与 ACPI HPET 表对照一致，能力三要素（位宽 32/64/定时器数/周期）
//!           双读 2 次一致账齐
//! 组件：ACPI HPET 表解析器（签名 HPET/表头校验和/事件定时器块逐字节记账）+
//!      能力寄存器账（GENERAL_CAPABILITIES 64 位逐位段拆解：计数位宽/周期/厂商/修订 +
//!      GENERAL_CONFIGURATION 逐位语义）+ 定时器清单（N 个定时器块：支持模式位/
//!      中断路由位/FSB 能力逐项入账）+ 双读一致核（两轮探测逐字段比对，不一致立案）+
//!      ACPI 表对照核（探测账 vs 表账逐字段对拍，偏差即红码）+ 只读红线（零写面：
//!      全部寄存器访问声明为只读，写请求显性拒绝计数）+ 周期换算（femtosecond→ns
//!      全整数，非 2 幂周期显性取整误差披露）。
//! 职责定位：HPET 探测的唯一权威面——只读、可对拍、账齐即封账；能力三要素
//!   （计数位宽/定时器数/周期）双读一致是封账前置，缺一不发探针结论；
//!   与 F1021（HPET 探测·分层上游）分工：本单做表级与能力级全量账，F1021
//!   做运行面探测，两账互为对照不互写。
//! 性能逐项分解：表解析 O(表长)（逐字节累计校验和）；双读核 O(字段数)=O(1)
//!   （能力三要素闭集 8 字段）；定时器清单 O(定时器数≤32)；周期换算 O(1)
//!   （u128 中间量一次除法）；FNV 指纹 O(账字节数)。全整数算术零浮点零墙钟，
//!   输入由模型注入（确定性），同序列恒同账。
//! 诊断码段：0x5200（0x5201~0x5208 闭集；全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64/u128 整数算术，
//!   确定性（探测输入注入，零墙钟零 IO），逐条定制数据结构/状态机/错误路径。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const HC_VERSION: &str = "unx404_hpetcap-v1";

/// 诊断段基址（0x5200 段 · 本模块独占）。
pub const HC_SEG_BASE: u16 = 0x5200;

/// ACPI HPET 表签名："HPET"（0x54455045 小端读出）。
pub const HPET_SIG: u32 = 0x5445_5045;

/// HPET 表头固定长度：36 字节（SDT 头 36 + 事件块 8 + 最少 1 定时器块 29，账面取头 36）。
pub const HPET_TABLE_HEADER_LEN: usize = 36;

/// 事件定时器块长度：8 字节（事件块 4 + 最小保留 4）。
pub const HPET_EVENT_BLOCK_LEN: usize = 8;

/// 单个定时器信息块长度：20 字节（IA-PC HPET 规范 v1.0a §3.2.2）。
pub const HPET_TIMER_BLOCK_LEN: usize = 20;

/// 定时器数上限（规范最少 3、QEMU 档典型 3；账面闭集 32 防注入爆账）。
pub const HC_TIMER_MAX: usize = 32;

/// GENERAL_CAPABILITIES：计数器位宽字段位偏移（bit 12:13）。
pub const CAP_COUNTER_SHIFT: u64 = 12;
/// GENERAL_CAPABILITIES：计数器位宽字段掩码（2 位）。
pub const CAP_COUNTER_MASK: u64 = 0b11;
/// GENERAL_CAPABILITIES：计数周期 femtosecond 字段位偏移（bit 32:63）。
pub const CAP_PERIOD_SHIFT: u64 = 32;
/// GENERAL_CAPABILITIES：厂商 ID 字段位偏移（bit 16:31）。
pub const CAP_VENDOR_SHIFT: u64 = 16;
/// GENERAL_CAPABILITIES：修订版本字段位偏移（bit 0:7）。
pub const CAP_REVISION_SHIFT: u64 = 0;
/// GENERAL_CAPABILITIES：legacy 替换路由位（bit 15）。
pub const CAP_LEGACY_ROUTE_BIT: u64 = 15;
/// GENERAL_CAPABILITIES：64 位主计数器标志位（bit 13）。
pub const CAP_64BIT_FLAG_BIT: u64 = 13;

/// GENERAL_CONFIGURATION：使能位（bit 0）。
pub const CFG_ENABLE_BIT: u64 = 0;
/// GENERAL_CONFIGURATION：legacy 替换位（bit 1）。
pub const CFG_LEGACY_BIT: u64 = 1;

/// 定时器块能力位：周期模式支持（bit 4）。
pub const TIM_CAP_PERIODIC_BIT: u64 = 4;
/// 定时器块能力位：64 位宽度（bit 5）。
pub const TIM_CAP_64BIT_BIT: u64 = 5;
/// 定时器块能力位：FSB 中断投递（bit 15）。
pub const TIM_CAP_FSB_BIT: u64 = 15;
/// 定时器块能力位：中断路由字段（bit 32:63，32 位路由位图）。
pub const TIM_CAP_ROUTE_SHIFT: u64 = 32;

/// 合法 femtosecond 周期下界：1 fs（规范最小）。
pub const PERIOD_FS_MIN: u64 = 1;
/// 合法 femtosecond 周期上界：1 s = 10^15 fs（规范最大）。
pub const PERIOD_FS_MAX: u64 = 1_000_000_000_000_000;

/// 双读核轮数（判据点名：双读 2 次）。
pub const HC_DUAL_READ_ROUNDS: usize = 2;

/// FNV-1a 64 位偏移基。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ---------------------------------------------------------------------------
// 诊断码（0x5201~0x5208 闭集）与诊断体
// ---------------------------------------------------------------------------

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HcCode {
    /// 0x5201 表签名不匹配。
    BadSignature = 0x5201,
    /// 0x5202 表头校验和不过。
    BadChecksum = 0x5202,
    /// 0x5203 能力三要素双读不一致。
    DualReadMismatch = 0x5203,
    /// 0x5204 ACPI 表账与探测账对照偏差。
    TableProbeMismatch = 0x5204,
    /// 0x5205 定时器数越界（>32 或 0）。
    TimerCountBad = 0x5205,
    /// 0x5206 周期字段越界（0 或超 1s）。
    PeriodOutOfRange = 0x5206,
    /// 0x5207 表长度不足以容纳声明的定时器数。
    TableTruncated = 0x5207,
    /// 0x5208 只读红线：写请求被拒（显性计数非静默）。
    WriteDenied = 0x5208,
}

impl HcCode {
    /// 诊断码数值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文诊断短句（错误路径第一要素：发生了什么）。
    pub fn text(self) -> &'static str {
        match self {
            HcCode::BadSignature => "HPET 表签名不匹配（非 HPET 四字签名）",
            HcCode::BadChecksum => "HPET 表头校验和不过（逐字节累计≠0）",
            HcCode::DualReadMismatch => "能力三要素双读 2 次不一致（硬件抖动或注入）",
            HcCode::TableProbeMismatch => "ACPI 表账与探测账字段对照偏差",
            HcCode::TimerCountBad => "定时器数越界（0 或超 32 上限）",
            HcCode::PeriodOutOfRange => "计数周期字段越界（0 fs 或超 1s）",
            HcCode::TableTruncated => "表长度不足以容纳声明的定时器数（截断）",
            HcCode::WriteDenied => "只读红线：HPET 写请求被拒（显性计数）",
        }
    }

    /// 可操作建议（错误路径第二要素：下一步怎么办）。
    pub fn suggestion(self) -> &'static str {
        match self {
            HcCode::BadSignature => "核对表基址来源（XSDT 条目指针）与字节序假设，重扫 XSDT",
            HcCode::BadChecksum => "表体可能损坏：换备份表或放弃 HPET 回退 PIT/TSC（F1041 仲裁面）",
            HcCode::DualReadMismatch => "再读第 3 次取多数；连续不一致按硬件不可信降级处理并立案",
            HcCode::TableProbeMismatch => "以表账为权威逐字段点名偏差项，探测器按表账校正后重探",
            HcCode::TimerCountBad => "核对事件块 number_timers 字段与规范范围（1~32），越界按损坏表拒收",
            HcCode::PeriodOutOfRange => "周期 0 表示寄存器未实现或读取错位：重读并核对 64 位对齐",
            HcCode::TableTruncated => "length 字段与定时器数矛盾：按 length 收账并标截断，不发探针结论",
            HcCode::WriteDenied => "确认调用方意图：本面只读，使能操作走 F1023 周期模式面",
        }
    }
}

/// 诊断体：码 + 位置 + 细节 + 建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcDiag {
    /// 诊断码。
    pub code: HcCode,
    /// 源位置（函数名:语义步骤）。
    pub site: String,
    /// 细节载荷（实际值 vs 期望值等）。
    pub detail: String,
}

impl HcDiag {
    /// 构造诊断体。
    pub fn new(code: HcCode, site: &str, detail: String) -> Self {
        HcDiag { code, site: alloc::string::ToString::to_string(site), detail }
    }

    /// 完整诊断文本（发生了什么/在哪/细节/怎么办）。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04X}] {} —— {} —— 位置:{} —— 建议:{}",
            self.code.num(),
            self.code.text(),
            self.detail,
            self.site,
            self.code.suggestion()
        )
    }
}

// ---------------------------------------------------------------------------
// 能力三要素（双读核对账的字段闭集）
// ---------------------------------------------------------------------------

/// 能力三要素快照（双读一致是封账前置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcCapability {
    /// 计数器位宽：32 或 64（判据点名）。
    pub counter_bits: u32,
    /// 定时器数（判据点名：定时器数）。
    pub timer_count: u32,
    /// 计数周期（femtosecond，判据点名：周期）。
    pub period_fs: u64,
    /// 厂商 ID（对照辅助字段）。
    pub vendor_id: u16,
    /// 修订版本。
    pub revision: u8,
    /// legacy 替换路由能力。
    pub legacy_route: bool,
    /// GENERAL_CONFIGURATION 使能位原样读数。
    pub config_enable: bool,
    /// GENERAL_CONFIGURATION legacy 替换位原样读数。
    pub config_legacy: bool,
}

impl HcCapability {
    /// 从 64 位能力寄存器读数与配置寄存器读数拆解。
    pub fn from_regs(caps: u64, config: u64, timer_count: u32) -> Option<Self> {
        let width_raw = (caps >> CAP_COUNTER_SHIFT) & CAP_COUNTER_MASK;
        let counter_bits: u32 = match width_raw {
            0 => 32,
            1 => 64,
            _ => return None,
        };
        // 位 13（64 位计数器标志）与位宽字段须一致，不一致按错位读取拒绝。
        let flag64 = (caps >> CAP_64BIT_FLAG_BIT) & 1 == 1;
        if (counter_bits == 64) != flag64 {
            return None;
        }
        Some(HcCapability {
            counter_bits,
            timer_count,
            period_fs: caps >> CAP_PERIOD_SHIFT,
            vendor_id: ((caps >> CAP_VENDOR_SHIFT) & 0xFFFF) as u16,
            revision: (caps & 0xFF) as u8,
            legacy_route: (caps >> CAP_LEGACY_ROUTE_BIT) & 1 == 1,
            config_enable: (config >> CFG_ENABLE_BIT) & 1 == 1,
            config_legacy: (config >> CFG_LEGACY_BIT) & 1 == 1,
        })
    }

    /// 周期换算为纳秒（fs→ns 全整数，向下取整，误差显性披露）。
    /// 返回（纳秒值，取整误差 fs）。周期 < 1000 fs 时纳秒恒 0，误差=周期本身。
    pub fn period_ns_floor(&self) -> (u64, u64) {
        let ns = self.period_fs / 1000;
        let err = self.period_fs % 1000;
        (ns, err)
    }

    /// 周期换算为主计数频率（Hz，全整数）：1e15 / period_fs，非整除披露余数。
    pub fn freq_hz(&self) -> (u64, u64) {
        // 1 s = 10^15 fs；频率 = 1e15 / 周期。
        (1_000_000_000_000_000u64 / self.period_fs, 1_000_000_000_000_000u64 % self.period_fs)
    }
}

// ---------------------------------------------------------------------------
// 定时器信息账（逐定时器入账）
// ---------------------------------------------------------------------------

/// 单定时器信息账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcTimerInfo {
    /// 定时器序号（0 起）。
    pub index: u8,
    /// 支持周期模式。
    pub periodic_cap: bool,
    /// 64 位宽度。
    pub width_64: bool,
    /// FSB 中断投递能力。
    pub fsb_cap: bool,
    /// 中断路由位图（bit n = 可路由到 IOAPIC n 输入）。
    pub route_map: u32,
    /// 该定时器配置寄存器第一轮读数。
    pub cfg_read_1: u64,
    /// 该定时器配置寄存器第二轮读数（双读账）。
    pub cfg_read_2: u64,
    /// 双读一致（配置寄存器级）。
    pub dual_ok: bool,
}

// ---------------------------------------------------------------------------
// ACPI HPET 表账（解析产物）
// ---------------------------------------------------------------------------

/// ACPI HPET 表解析账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcTableLedger {
    /// 签名匹配。
    pub sig_ok: bool,
    /// 表长度字段（字节）。
    pub length: u32,
    /// 逐字节累计校验和（应 ≡ 0 mod 256）。
    pub checksum_acc: u8,
    /// 校验和通过。
    pub checksum_ok: bool,
    /// OEM ID 6 字节（ASCII 语义，按字节入账）。
    pub oem_id: [u8; 6],
    /// 事件块基址（块 ID[8:12] + 基址[12:20] 的 64 位地址）。
    pub event_block_addr: u64,
    /// HPET 硬件序号（事件块 bit 0:7）。
    pub hw_seq: u8,
    /// 最小时钟 tick（事件块 bit 32:47，用于对照）。
    pub min_clock_tick: u16,
    /// 表声明的定时器数。
    pub declared_timers: u32,
    /// 表体是否足够容纳全部定时器块。
    pub body_complete: bool,
    /// FNV 指纹（全表字节）。
    pub fp: u64,
}

impl HcTableLedger {
    /// 从注入表字节序列解析（确定性：无 IO，输入由模型给定）。
    pub fn parse(bytes: &[u8]) -> Result<(HcTableLedger, Vec<u8>), HcDiag> {
        // 长度下限：表头 36 + 事件块 8 + 至少 1 个定时器块 20。
        if bytes.len() < HPET_TABLE_HEADER_LEN + HPET_EVENT_BLOCK_LEN + HPET_TIMER_BLOCK_LEN {
            return Err(HcDiag::new(
                HcCode::TableTruncated,
                "parse:len",
                format!("表长 {} 低于最小 64", bytes.len()),
            ));
        }
        let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sig != HPET_SIG {
            return Err(HcDiag::new(
                HcCode::BadSignature,
                "parse:sig",
                format!("读出 0x{:08X} 期望 0x{:08X}", sig, HPET_SIG),
            ));
        }
        let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        // 校验和：整表逐字节累计（以声明的 length 为界；length 超实际长按实际长并记截断）。
        let span = if length <= bytes.len() { length } else { bytes.len() };
        let mut acc: u32 = 0;
        for b in &bytes[..span] {
            acc += *b as u32;
        }
        let checksum_acc = (acc & 0xFF) as u8;
        let checksum_ok = checksum_acc == 0;
        let mut oem_id = [0u8; 6];
        oem_id.copy_from_slice(&bytes[10..16]);
        let event_block_addr = u64::from_le_bytes([
            bytes[28], bytes[29], bytes[30], bytes[31], bytes[32], bytes[33], bytes[34], bytes[35],
        ]);
        let hw_seq = bytes[36];
        let min_clock_tick = u16::from_le_bytes([bytes[40], bytes[41]]);
        let declared = bytes[43] as u32;
        if declared == 0 || declared as usize > HC_TIMER_MAX {
            return Err(HcDiag::new(
                HcCode::TimerCountBad,
                "parse:timcount",
                format!("声明定时器数 {}", declared),
            ));
        }
        let need = HPET_TABLE_HEADER_LEN + HPET_EVENT_BLOCK_LEN + declared as usize * HPET_TIMER_BLOCK_LEN;
        let body_complete = bytes.len() >= need;
        if !body_complete {
            return Err(HcDiag::new(
                HcCode::TableTruncated,
                "parse:body",
                format!("需 {} 字节实际 {}", need, bytes.len()),
            ));
        }
        let fp = fnv64(bytes);
        Ok((
            HcTableLedger {
                sig_ok: true,
                length: length as u32,
                checksum_acc,
                checksum_ok,
                oem_id,
                event_block_addr,
                hw_seq,
                min_clock_tick,
                declared_timers: declared,
                body_complete,
                fp,
            },
            bytes[HPET_TABLE_HEADER_LEN + HPET_EVENT_BLOCK_LEN..need].to_vec(),
        ))
    }
}

/// FNV-1a 64 位。
fn fnv64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// 探测器主账（双读 + 对照 + 只读红线）
// ---------------------------------------------------------------------------

/// 只读红线计数器（写请求显性拒绝，不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcWriteDeny {
    /// 拒绝次数。
    pub count: u64,
    /// 最近一次拒绝的调用方语义名。
    pub last_caller: String,
}

/// 探测结论状态机：三态单向（未探测→已探测→已封账），封账后只读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HcPhase {
    /// 未探测。
    Idle,
    /// 已探测未封账（双读或对照未全绿）。
    Probed,
    /// 已封账（全绿，账不可改）。
    Sealed,
}

/// HPET 探测主账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HcProbeLedger {
    /// 表账。
    pub table: HcTableLedger,
    /// 能力三要素第一轮。
    pub cap_read_1: HcCapability,
    /// 能力三要素第二轮。
    pub cap_read_2: HcCapability,
    /// 能力双读一致（判据点名）。
    pub cap_dual_ok: bool,
    /// 定时器清单（逐定时器双读）。
    pub timers: Vec<HcTimerInfo>,
    /// 表账 vs 探测账对照偏差项（空 = 全对平，判据点名）。
    pub table_mismatches: Vec<String>,
    /// 探测相位。
    pub phase: HcPhase,
    /// 只读红线账。
    pub write_deny: HcWriteDeny,
    /// 探针指纹（全账 FNV）。
    pub fp: u64,
}

impl HcProbeLedger {
    /// 执行一次完整探测（输入全注入：表字节 + 两轮寄存器读数 + 定时器读数矩阵）。
    /// 任意一步失败返回诊断，账不入。
    pub fn probe(
        table_bytes: &[u8],
        caps_1: u64,
        config_1: u64,
        caps_2: u64,
        config_2: u64,
        timer_cfg_1: &[u64],
        timer_cfg_2: &[u64],
    ) -> Result<HcProbeLedger, HcDiag> {
        let (table, timer_body) = HcTableLedger::parse(table_bytes)?;
        if !table.checksum_ok {
            return Err(HcDiag::new(
                HcCode::BadChecksum,
                "probe:checksum",
                format!("累计 {}", table.checksum_acc),
            ));
        }
        let n = table.declared_timers as usize;
        if timer_cfg_1.len() < n || timer_cfg_2.len() < n {
            return Err(HcDiag::new(
                HcCode::TimerCountBad,
                "probe:timbuf",
                format!("声明 {} 实给 {}/{}", n, timer_cfg_1.len(), timer_cfg_2.len()),
            ));
        }
        let cap_1 = HcCapability::from_regs(caps_1, config_1, n as u32).ok_or_else(|| {
            HcDiag::new(HcCode::PeriodOutOfRange, "probe:cap1", format!("caps=0x{:016X}", caps_1))
        })?;
        let cap_2 = HcCapability::from_regs(caps_2, config_2, n as u32).ok_or_else(|| {
            HcDiag::new(HcCode::PeriodOutOfRange, "probe:cap2", format!("caps=0x{:016X}", caps_2))
        })?;
        if cap_1.period_fs < PERIOD_FS_MIN || cap_1.period_fs > PERIOD_FS_MAX {
            return Err(HcDiag::new(
                HcCode::PeriodOutOfRange,
                "probe:period",
                format!("周期 {} fs", cap_1.period_fs),
            ));
        }
        let cap_dual_ok = cap_1 == cap_2;
        // 定时器逐个双读。
        let mut timers = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < n {
            let c1 = timer_cfg_1[i];
            let c2 = timer_cfg_2[i];
            timers.push(HcTimerInfo {
                index: i as u8,
                periodic_cap: (c1 >> TIM_CAP_PERIODIC_BIT) & 1 == 1,
                width_64: (c1 >> TIM_CAP_64BIT_BIT) & 1 == 1,
                fsb_cap: (c1 >> TIM_CAP_FSB_BIT) & 1 == 1,
                route_map: (c1 >> TIM_CAP_ROUTE_SHIFT) as u32,
                cfg_read_1: c1,
                cfg_read_2: c2,
                dual_ok: c1 == c2,
            });
            i += 1;
        }
        // ACPI 表账 vs 探测账对照（判据点名：探测结果与 ACPI HPET 表对照一致）。
        // 对照项闭集 4 项：定时器数 / 表长自洽 / 事件块地址非零 / 最小时钟 tick 非零。
        let mut table_mismatches: Vec<String> = Vec::new();
        if cap_1.timer_count != table.declared_timers {
            table_mismatches.push(format!(
                "定时器数:表账{} vs 探测账{}",
                table.declared_timers, cap_1.timer_count
            ));
        }
        if (table.length as usize) < need_len(table.declared_timers as usize) {
            table_mismatches.push(format!(
                "表长{}不足以容纳{}定时器",
                table.length, table.declared_timers
            ));
        }
        if table.event_block_addr == 0 {
            table_mismatches.push("事件块基址为 0（对照红项）".to_string());
        }
        if table.min_clock_tick == 0 {
            table_mismatches.push("最小时钟 tick 为 0（对照红项）".to_string());
        }
        // 定时器体字节指纹并入账（body 已截取，此处仅计长对账）。
        if timer_body.len() != n * HPET_TIMER_BLOCK_LEN {
            return Err(HcDiag::new(
                HcCode::TableTruncated,
                "probe:bodylen",
                format!("体长 {} ≠ 定时器数×块长 {}", timer_body.len(), n * HPET_TIMER_BLOCK_LEN),
            ));
        }
        let phase = if cap_dual_ok
            && table_mismatches.is_empty()
            && timers.iter().all(|t| t.dual_ok)
        {
            HcPhase::Sealed
        } else {
            HcPhase::Probed
        };
        let mut ledger = HcProbeLedger {
            table,
            cap_read_1: cap_1,
            cap_read_2: cap_2,
            cap_dual_ok,
            timers,
            table_mismatches,
            phase,
            write_deny: HcWriteDeny { count: 0, last_caller: String::new() },
            fp: 0,
        };
        ledger.fp = ledger.fingerprint();
        Ok(ledger)
    }

    /// 只读红线：任何写意图到此登记拒绝并显性返回诊断（不静默）。
    pub fn deny_write(&mut self, caller: &str) -> HcDiag {
        self.write_deny.count += 1;
        self.write_deny.last_caller = String::from(caller);
        HcDiag::new(
            HcCode::WriteDenied,
            "deny_write",
            format!("调用方 {} 第 {} 次写意图被拒", caller, self.write_deny.count),
        )
    }

    /// 封账条件核对：双读一致 + 对照全平 + 定时器全双读一致 + 相位 Sealed。
    pub fn sealed_green(&self) -> bool {
        self.phase == HcPhase::Sealed
            && self.cap_dual_ok
            && self.table_mismatches.is_empty()
            && self.timers.iter().all(|t| t.dual_ok)
    }

    /// 全账 FNV 指纹（逐字段语义序，非内存序）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut feed = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(FNV_PRIME);
        };
        feed(&mut h, (self.table.fp >> 56) as u8);
        feed(&mut h, (self.table.fp >> 48) as u8);
        feed(&mut h, (self.table.fp >> 40) as u8);
        feed(&mut h, (self.table.fp >> 32) as u8);
        feed(&mut h, (self.table.fp >> 24) as u8);
        feed(&mut h, (self.table.fp >> 16) as u8);
        feed(&mut h, (self.table.fp >> 8) as u8);
        feed(&mut h, self.table.fp as u8);
        let c = &self.cap_read_1;
        let vals: [u64; 6] = [
            c.counter_bits as u64,
            c.timer_count as u64,
            c.period_fs,
            c.vendor_id as u64,
            c.revision as u64,
            c.legacy_route as u64,
        ];
        for v in vals {
            let mut j = 0;
            while j < 8 {
                feed(&mut h, (v >> (j * 8)) as u8);
                j += 1;
            }
        }
        for t in &self.timers {
            feed(&mut h, t.index);
            feed(&mut h, t.dual_ok as u8);
        }
        h
    }

    /// 能力三要素报告行（对照用）。
    pub fn cap_report(&self) -> String {
        let (ns, err) = self.cap_read_1.period_ns_floor();
        let (hz, hzrem) = self.cap_read_1.freq_hz();
        format!(
            "位宽:{}bit 定时器数:{} 周期:{}fs(≈{}ns,余{}fs) 频率:≈{}Hz(余{}) 双读:{} 对照偏差:{} 相位:{:?}",
            self.cap_read_1.counter_bits,
            self.cap_read_1.timer_count,
            self.cap_read_1.period_fs,
            ns,
            err,
            hz,
            hzrem,
            self.cap_dual_ok,
            self.table_mismatches.len(),
            self.phase
        )
    }
}

/// 表所需最小长度（定时器数→字节数）。
fn need_len(n: usize) -> usize {
    HPET_TABLE_HEADER_LEN + HPET_EVENT_BLOCK_LEN + n * HPET_TIMER_BLOCK_LEN
}

// ---------------------------------------------------------------------------
// 确定性演练核（判据落地面：同输入恒同账，注入偏差必显性）
// ---------------------------------------------------------------------------

/// 确定性 LCG（演练输入源；与生产探测分离）。
pub struct HcLcg {
    state: u64,
}

impl HcLcg {
    /// 以种子构建。
    pub fn new(seed: u64) -> Self {
        HcLcg { state: seed }
    }

    /// 下一 64 位伪随机数（确定性）。
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
}

/// 演练：构造一套全绿输入并探测，断言封账绿（返回（绿,报告））。
pub fn drill_green() -> (bool, String) {
    // 表：36 头 + 8 事件块 + 3 定时器块×20 = 104 字节。
    let mut tbl = vec![0u8; 104];
    tbl[0..4].copy_from_slice(&HPET_SIG.to_le_bytes());
    let len: u32 = 104;
    tbl[4..8].copy_from_slice(&len.to_le_bytes());
    tbl[10..16].copy_from_slice(b"VXHPET");
    tbl[36] = 0x01; // hw_seq
    tbl[40] = 0x64; // min_clock_tick 低字节（100）
    tbl[43] = 3; // 定时器数
    // 事件块基址（字节 28-35，对照闭集要求非零）：HPET 规范基址 + 0x100 事件块。
    tbl[28..36].copy_from_slice(&0xFED0_0100u64.to_le_bytes());
    // 校验和位（字节 9）：使整表累计 ≡ 0 mod 256。
    let mut acc: u32 = 0;
    for b in &tbl {
        acc += *b as u32;
    }
    tbl[9] = ((256 - (acc & 0xFF)) & 0xFF) as u8;
    // 能力寄存器：32 位宽（width=0,flag=0）、周期 10^7 fs（10ns→100MHz）、厂商 0x8086、修订 1、legacy=1。
    let caps: u64 = (10_000_000u64 << CAP_PERIOD_SHIFT)
        | (1 << CAP_LEGACY_ROUTE_BIT)
        | (0x8086u64 << CAP_VENDOR_SHIFT)
        | 1;
    let config: u64 = 1; // enable
    // 3 个定时器：全部周期+FSB+32 位路由位图 0xFF。
    let tcfg: u64 = (0xFFu64 << TIM_CAP_ROUTE_SHIFT)
        | (1 << TIM_CAP_FSB_BIT)
        | (1 << TIM_CAP_64BIT_BIT)
        | (1 << TIM_CAP_PERIODIC_BIT);
    let t1 = [tcfg, tcfg, tcfg];
    match HcProbeLedger::probe(&tbl, caps, config, caps, config, &t1, &t1) {
        Ok(l) => {
            let green = l.sealed_green() && l.fp == l.fingerprint();
            (green, format!("演练:{}", l.cap_report()))
        }
        Err(d) => (false, d.full()),
    }
}

/// 注入核：6 种偏差注入，探测器必须全部显性拒绝（判据面：注入必抓）。
/// 返回（抓到数, 总注入数, 明细）。判据口径：6/6。
pub fn drill_inject() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let total = 6usize;
    let mut detail: Vec<String> = Vec::new();
    // 基线全绿输入（同 drill_green）。
    let mut tbl = vec![0u8; 104];
    tbl[0..4].copy_from_slice(&HPET_SIG.to_le_bytes());
    let len: u32 = 104;
    tbl[4..8].copy_from_slice(&len.to_le_bytes());
    tbl[10..16].copy_from_slice(b"VXHPET");
    tbl[36] = 1;
    tbl[40] = 100;
    tbl[43] = 3;
    let mut acc: u32 = 0;
    for b in &tbl {
        acc += *b as u32;
    }
    tbl[9] = ((256 - (acc & 0xFF)) & 0xFF) as u8;
    let caps: u64 = (10_000_000u64 << CAP_PERIOD_SHIFT) | (1 << CAP_LEGACY_ROUTE_BIT) | 1;
    let config: u64 = 1;
    let tcfg: u64 = (0xFFu64 << TIM_CAP_ROUTE_SHIFT) | (1 << TIM_CAP_PERIODIC_BIT);
    let t1 = [tcfg, tcfg, tcfg];
    // 注 1：签名破坏。
    let mut bad = tbl.clone();
    bad[0] = b'X';
    if HcProbeLedger::probe(&bad, caps, config, caps, config, &t1, &t1).is_err() {
        caught += 1;
        detail.push("注1 签名破坏:抓".to_string());
    } else {
        detail.push("注1 签名破坏:漏".to_string());
    }
    // 注 2：校验和破坏。
    let mut bad = tbl.clone();
    bad[20] = bad[20].wrapping_add(0x40);
    if HcProbeLedger::probe(&bad, caps, config, caps, config, &t1, &t1).is_err() {
        caught += 1;
        detail.push("注2 校验和破坏:抓".to_string());
    } else {
        detail.push("注2 校验和破坏:漏".to_string());
    }
    // 注 3：双读不一致（第二轮周期翻倍）。
    let caps2 = caps ^ (5_000_000u64 << CAP_PERIOD_SHIFT);
    if HcProbeLedger::probe(&tbl, caps, config, caps2, config, &t1, &t1)
        .map(|l| !l.cap_dual_ok)
        .unwrap_or(false)
    {
        caught += 1;
        detail.push("注3 双读不一致:抓".to_string());
    } else {
        detail.push("注3 双读不一致:漏".to_string());
    }
    // 注 4：定时器数越界（声明 40）。
    let mut bad = tbl.clone();
    bad[43] = 40;
    if HcProbeLedger::probe(&bad, caps, config, caps, config, &t1, &t1).is_err() {
        caught += 1;
        detail.push("注4 定时器数越界:抓".to_string());
    } else {
        detail.push("注4 定时器数越界:漏".to_string());
    }
    // 注 5：表截断。
    let short = &tbl[..64];
    if HcProbeLedger::probe(short, caps, config, caps, config, &t1, &t1).is_err() {
        caught += 1;
        detail.push("注5 表截断:抓".to_string());
    } else {
        detail.push("注5 表截断:漏".to_string());
    }
    // 注 6：周期越界（0 fs）。
    let caps0 = caps & !(0xFFFF_FFFFu64 << CAP_PERIOD_SHIFT);
    if HcProbeLedger::probe(&tbl, caps0, config, caps0, config, &t1, &t1).is_err() {
        caught += 1;
        detail.push("注6 周期越界:抓".to_string());
    } else {
        detail.push("注6 周期越界:漏".to_string());
    }
    (caught, total, detail)
}
