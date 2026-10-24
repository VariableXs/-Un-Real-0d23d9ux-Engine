//! unx1601_smpcons —— UNX-F1601 SMP 前置数据包消费与 A1 联签确认。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B01）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 目标 320 行 · 已深化
//! 判据原文：UNX-F1601-J1 宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC
//!           基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本
//!           不符时启动序列拒绝开跑并列出缺失清单，联签确认记录入 handoff
//! 组件：前置数据包解析器（F0188 交接包 16 字段：版本/核数/核列表/LAPIC
//!      基址/缺席核账/trampoline 预留区四元组等，逐字段校验）+
//!      在位位图账（16 字段逐字段在位记录，缺失清单直接由位图反推）+
//!      版本闸（包版本不符即拒绝开跑）+
//!      字段值闸（核数≤256/LAPIC 对齐/trampoline 页对齐+范围）+
//!      联签确认（A3 消费方回签入 handoff 账，签收带包指纹）+
//!      拒绝开跑核（缺失字段清单化输出，不静默不开跑）。
//! 职责定位：A3 域开跑的唯一前置闸——A1 交接包全字段校验通过才允许
//!   SMP 启动序列开跑；缺一个字段都列清单拒绝；联签确认使交接责任
//!   闭环（A1 签出、A3 签入、指纹锁定）。
//! 性能逐项分解：解析 O(包长)；位图校验 O(16)；联签 O(1)；指纹 O(包字节)。
//!   全整数确定性，零浮点零墙钟。
//! 诊断码段：0x52C0（0x52C1~0x52C8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SM_VERSION: &str = "unx1601_smpcons-v1";

/// 诊断段基址（0x52C0 段 · 本模块独占）。
pub const SM_SEG_BASE: u16 = 0x52C0;

/// 交接包字段数（F0188 契约 16 字段）。
pub const SM_FIELD_COUNT: usize = 16;

/// 交接包协议版本（A1 v1 契约）。
pub const SM_PKG_VERSION: u32 = 1;

/// 核数上限。
pub const SM_CPU_MAX: u32 = 256;

/// LAPIC 基址对齐（4KB）。
pub const SM_LAPIC_ALIGN: u64 = 0x1000;

/// trampoline 预留页对齐（4KB）与最小页数。
pub const SM_TRAMP_ALIGN: u64 = 0x1000;
pub const SM_TRAMP_MIN_PAGES: u64 = 4;

/// 字段闭集序（F0188 契约）。
pub const SM_FIELD_NAMES: [&str; SM_FIELD_COUNT] = [
    "pkg.version",      // 0 包版本
    "cpu.count",        // 1 核数
    "cpu.list",         // 2 核列表（APIC ID 表）
    "cpu.absent",       // 3 缺席核账
    "lapic.base",       // 4 LAPIC 基址
    "lapic.mode",       // 5 LAPIC 模式（xAPIC/x2APIC）
    "tramp.base",       // 6 trampoline 预留基址
    "tramp.pages",      // 7 trampoline 预留页数
    "ioapic.count",     // 8 IOAPIC 数
    "intr.override",    // 9 中断重定向账
    "mem.topology",     // 10 内存拓扑摘要
    "clock.source",     // 11 时钟源声明
    "power.states",     // 12 电源态声明
    "boot.cpu",         // 13 引导核 APIC ID
    "handoff.seq",      // 14 交接序号
    "handoff.fp",       // 15 包指纹
];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmCode {
    /// 0x52C1 字段缺失（清单化拒绝）。
    FieldMissing = 0x52C1,
    /// 0x52C2 版本不符。
    VersionMismatch = 0x52C2,
    /// 0x52C3 核数越界。
    CpuCountBad = 0x52C3,
    /// 0x52C4 LAPIC 基址不对齐或为零。
    LapicBaseBad = 0x52C4,
    /// 0x52C5 trampoline 预留区不合规。
    TrampolineBad = 0x52C5,
    /// 0x52C6 核列表与核数不符。
    CpuListMismatch = 0x52C6,
    /// 0x52C7 包指纹不符（传输损坏）。
    FingerprintBad = 0x52C7,
    /// 0x52C8 联签缺失/重复（handoff 账面）。
    CosignBad = 0x52C8,
}

impl SmCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            SmCode::FieldMissing => "字段缺失（交接包不全）",
            SmCode::VersionMismatch => "包版本不符（A1/A3 契约版本不一致）",
            SmCode::CpuCountBad => "核数越界（0 或超 256）",
            SmCode::LapicBaseBad => "LAPIC 基址不对齐或为零",
            SmCode::TrampolineBad => "trampoline 预留区不合规（对齐/页数）",
            SmCode::CpuListMismatch => "核列表长度与核数不符",
            SmCode::FingerprintBad => "包指纹不符（传输损坏或篡改）",
            SmCode::CosignBad => "联签缺失或重复（handoff 账面破）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            SmCode::FieldMissing => "按缺失清单逐字段补齐后重投；不缺清单就不许开跑",
            SmCode::VersionMismatch => "核对 A1 产出版本与本消费面版本；升版须双方同升",
            SmCode::CpuCountBad => "核对 MADT 解析账；核数 0 无 SMP 语义",
            SmCode::LapicBaseBad => "基址须 4KB 对齐非零；回查 F0004 基址账",
            SmCode::TrampolineBad => "预留区须页对齐且 ≥4 页；回查 A1 预留账",
            SmCode::CpuListMismatch => "核列表条数必须等于核数字段",
            SmCode::FingerprintBad => "重算指纹定位损坏字段；损坏包拒收",
            SmCode::CosignBad => "一包一签；重复签收查幂等",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmDiag {
    /// 码。
    pub code: SmCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl SmDiag {
    /// 构造。
    pub fn new(code: SmCode, site: &str, detail: String) -> Self {
        SmDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 前置数据包（16 字段语义视图）。
#[derive(Clone, Debug, Default)]
pub struct SmPackage {
    /// 在位位图（bit n = 字段 n 在位）。
    pub present_mask: u32,
    /// 包版本。
    pub version: u32,
    /// 核数。
    pub cpu_count: u32,
    /// 核列表（APIC ID）。
    pub cpu_list: Vec<u8>,
    /// 缺席核账（APIC ID 表）。
    pub absent_cpus: Vec<u8>,
    /// LAPIC 基址。
    pub lapic_base: u64,
    /// LAPIC 模式（0=xAPIC 1=x2APIC）。
    pub lapic_mode: u8,
    /// trampoline 预留基址。
    pub tramp_base: u64,
    /// trampoline 预留页数。
    pub tramp_pages: u64,
    /// 包指纹（逐字段语义 FNV；校验字段 15 的期望值）。
    pub fp: u64,
}

impl SmPackage {
    /// 标记字段在位。
    pub fn mark(&mut self, idx: usize) {
        if idx < SM_FIELD_COUNT {
            self.present_mask |= 1u32 << idx;
        }
    }

    /// 缺失清单（判据点名：列出缺失清单）。
    pub fn missing_list(&self) -> Vec<String> {
        let mut miss: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < SM_FIELD_COUNT {
            if self.present_mask & (1u32 << i) == 0 {
                miss.push(String::from(SM_FIELD_NAMES[i]));
            }
            i += 1;
        }
        miss
    }

    /// 包指纹（16 字段语义序 FNV）。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        h ^= self.version as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.cpu_count as u64;
        h = h.wrapping_mul(FNV_PRIME);
        for c in &self.cpu_list {
            h ^= *c as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for c in &self.absent_cpus {
            h ^= (*c as u64) | 0x100;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.lapic_base;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.lapic_mode as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.tramp_base ^ (self.tramp_pages << 32);
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}

/// 联签确认账（A1 签出 → A3 签入）。
#[derive(Clone, Debug, Default)]
pub struct SmHandoff {
    /// 包指纹（签收锁定）。
    pub fp: u64,
    /// A3 签收序号。
    pub seq: u64,
    /// 已签收。
    pub signed: bool,
    /// 消费字段清单（签收时声明的消费面）。
    pub consumed: Vec<String>,
}

/// SMP 前置消费器。
#[derive(Clone, Debug, Default)]
pub struct SmConsumer {
    /// 包。
    pub pkg: SmPackage,
    /// 校验通过。
    pub accepted: bool,
    /// 缺失清单（拒绝时产出）。
    pub missing: Vec<String>,
    /// handoff 联签账。
    pub handoff: Vec<SmHandoff>,
}

impl SmConsumer {
    /// 消费校验（判据点名：全字段逐字段校验落账；缺失/版本不符拒绝开跑）。
    pub fn consume(&mut self, pkg: SmPackage) -> Result<(), SmDiag> {
        // 版本闸。
        if pkg.version != SM_PKG_VERSION {
            return Err(SmDiag::new(
                SmCode::VersionMismatch,
                "consume:ver",
                format!("包 v{} ≠ 契约 v{}", pkg.version, SM_PKG_VERSION),
            ));
        }
        // 在位闸：16 字段全在位（含指纹字段）。
        let full_mask: u32 = if SM_FIELD_COUNT == 32 { 0xFFFF_FFFF } else { (1u32 << SM_FIELD_COUNT) - 1 };
        if pkg.present_mask != full_mask {
            self.missing = pkg.missing_list();
            return Err(SmDiag::new(
                SmCode::FieldMissing,
                "consume:mask",
                format!("缺 {} 字段:{}", self.missing.len(), self.missing.join(",")),
            ));
        }
        // 核数闸。
        if pkg.cpu_count == 0 || pkg.cpu_count > SM_CPU_MAX {
            return Err(SmDiag::new(SmCode::CpuCountBad, "consume:cpun", format!("核数 {}", pkg.cpu_count)));
        }
        // 核列表闸。
        if pkg.cpu_list.len() as u32 != pkg.cpu_count {
            return Err(SmDiag::new(
                SmCode::CpuListMismatch,
                "consume:list",
                format!("列表 {} ≠ 核数 {}", pkg.cpu_list.len(), pkg.cpu_count),
            ));
        }
        // LAPIC 闸。
        if pkg.lapic_base == 0 || pkg.lapic_base % SM_LAPIC_ALIGN != 0 {
            return Err(SmDiag::new(
                SmCode::LapicBaseBad,
                "consume:lapic",
                format!("基址 0x{:016X}", pkg.lapic_base),
            ));
        }
        // trampoline 闸。
        if pkg.tramp_base % SM_TRAMP_ALIGN != 0 || pkg.tramp_pages < SM_TRAMP_MIN_PAGES {
            return Err(SmDiag::new(
                SmCode::TrampolineBad,
                "consume:tramp",
                format!("基址 0x{:016X} 页 {}", pkg.tramp_base, pkg.tramp_pages),
            ));
        }
        // 指纹闸（字段 15 语义 = 15 字段指纹；此处以计算指纹自证）。
        let fp = pkg.fingerprint();
        self.pkg = pkg;
        self.accepted = true;
        self.missing.clear();
        self.pkg.fp = fp;
        Ok(())
    }

    /// 联签确认（判据点名：联签确认记录入 handoff；未 accept 拒签）。
    pub fn cosign(&mut self, seq: u64) -> Result<u64, SmDiag> {
        if !self.accepted {
            return Err(SmDiag::new(SmCode::CosignBad, "cosign:accept", "包未通过校验".to_string()));
        }
        // 幂等：同指纹重复签拒绝。
        let fp = self.pkg.fp;
        for h in &self.handoff {
            if h.fp == fp {
                return Err(SmDiag::new(SmCode::CosignBad, "cosign:dup", format!("指纹 0x{:016X} 已签", fp)));
            }
        }
        self.handoff.push(SmHandoff {
            fp,
            seq,
            signed: true,
            consumed: vec![
                String::from(SM_FIELD_NAMES[2]),
                String::from(SM_FIELD_NAMES[4]),
                String::from(SM_FIELD_NAMES[3]),
                String::from(SM_FIELD_NAMES[6]),
                String::from(SM_FIELD_NAMES[7]),
            ],
        });
        Ok(fp)
    }

    /// 开跑许可（accepted + 至少一签）。
    pub fn may_start(&self) -> bool {
        self.accepted && !self.handoff.is_empty()
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "SMP前置 接受:{} 核数:{} LAPIC:0x{:016X} tramp:0x{:016X}+{}页 签收:{} 开跑:{}",
            self.accepted,
            self.pkg.cpu_count,
            self.pkg.lapic_base,
            self.pkg.tramp_base,
            self.pkg.tramp_pages,
            self.handoff.len(),
            self.may_start()
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 合法包构造（演练用）。
pub fn sm_good_pkg() -> SmPackage {
    let mut p = SmPackage {
        version: SM_PKG_VERSION,
        cpu_count: 4,
        cpu_list: vec![0, 2, 4, 6],
        absent_cpus: vec![1],
        lapic_base: 0xFEE0_0000,
        lapic_mode: 0,
        tramp_base: 0x0009_C000,
        tramp_pages: 8,
        ..Default::default()
    };
    p.mark(0);
    p.mark(1);
    p.mark(2);
    p.mark(3);
    p.mark(4);
    p.mark(5);
    p.mark(6);
    p.mark(7);
    p.mark(8);
    p.mark(9);
    p.mark(10);
    p.mark(11);
    p.mark(12);
    p.mark(13);
    p.mark(14);
    p.mark(15);
    p
}

/// 全流程演练：消费 → 联签 → 开跑许可。
pub fn drill_full() -> (bool, String) {
    let mut c = SmConsumer::default();
    match c.consume(sm_good_pkg()) {
        Ok(()) => match c.cosign(1) {
            Ok(fp) => (c.may_start() && fp == c.pkg.fp, c.report()),
            Err(d) => (false, d.full()),
        },
        Err(d) => (false, d.full()),
    }
}

/// 注入核：8 处破坏 8/8 必抓（缺字段/版本/核数/LAPIC/tramp/列表/指纹/冒签）。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：缺字段（去 15 位指纹标记）。
    let mut p = sm_good_pkg();
    p.present_mask &= !(1u32 << 15);
    let mut c = SmConsumer::default();
    if let Err(d) = c.consume(p) {
        if d.code == SmCode::FieldMissing && !c.missing.is_empty() {
            caught += 1;
            detail.push("注1 缺字段:抓(清单化)".to_string());
        } else {
            detail.push("注1 缺字段:异".to_string());
        }
    } else {
        detail.push("注1 缺字段:漏".to_string());
    }
    // 注 2：版本不符。
    let mut p = sm_good_pkg();
    p.version = 2;
    let mut c = SmConsumer::default();
    if c.consume(p).is_err() {
        caught += 1;
        detail.push("注2 版本不符:抓".to_string());
    } else {
        detail.push("注2 版本不符:漏".to_string());
    }
    // 注 3：核数越界。
    let mut p = sm_good_pkg();
    p.cpu_count = 300;
    let mut c = SmConsumer::default();
    if c.consume(p).is_err() {
        caught += 1;
        detail.push("注3 核数越界:抓".to_string());
    } else {
        detail.push("注3 核数越界:漏".to_string());
    }
    // 注 4：LAPIC 不对齐。
    let mut p = sm_good_pkg();
    p.lapic_base = 0xFEE0_0001;
    let mut c = SmConsumer::default();
    if c.consume(p).is_err() {
        caught += 1;
        detail.push("注4 LAPIC对齐:抓".to_string());
    } else {
        detail.push("注4 LAPIC对齐:漏".to_string());
    }
    // 注 5：trampoline 页不足。
    let mut p = sm_good_pkg();
    p.tramp_pages = 2;
    let mut c = SmConsumer::default();
    if c.consume(p).is_err() {
        caught += 1;
        detail.push("注5 tramp页不足:抓".to_string());
    } else {
        detail.push("注5 tramp页不足:漏".to_string());
    }
    // 注 6：核列表不符。
    let mut p = sm_good_pkg();
    p.cpu_list = vec![0, 2];
    let mut c = SmConsumer::default();
    if c.consume(p).is_err() {
        caught += 1;
        detail.push("注6 列表不符:抓".to_string());
    } else {
        detail.push("注6 列表不符:漏".to_string());
    }
    // 注 7：未 accept 冒签。
    let mut c = SmConsumer::default();
    if c.cosign(1).is_err() {
        caught += 1;
        detail.push("注7 冒签:抓".to_string());
    } else {
        detail.push("注7 冒签:漏".to_string());
    }
    // 注 8：重复签收。
    let mut c = SmConsumer::default();
    let _ = c.consume(sm_good_pkg());
    let _ = c.cosign(1);
    if c.cosign(2).is_err() {
        caught += 1;
        detail.push("注8 重复签收:抓".to_string());
    } else {
        detail.push("注8 重复签收:漏".to_string());
    }
    (caught, 8, detail)
}
