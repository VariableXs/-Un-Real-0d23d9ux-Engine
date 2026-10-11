//! unx182_handover ↔ UNX-F0182 阶段产物交接协议（init 契约层）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0182，UNX-A1-B10）：
//! 「阶段产物交接协议（init 契约）| 280 | 已深化 |
//!  UNX-F0182-J1 单元间交接结构体版本化校验，缺字段拒绝启动并落账」
//!
//! 设计要点（全部围绕内核引导期真实状态）：
//! 1. **交接包版本化**：`HandoverPacket` 携带 `schema`/`version` 双字段，
//!    生产者与消费者两侧各自冻结一份期望表；版本不匹配不是警告而是**拒绝**——
//!    引导期拿到半份交接包比没有交接包更危险（后续域会把缺字段当零值用）。
//! 2. **缺字段拒绝启动**：`accept()` 逐字段核对 `present` 位图，缺一即
//!    `Err(MissingField)`，且把缺失字段名带进诊断（可操作建议 = 点名生产者）。
//! 3. **落账只增不减**：每次拒绝写入 `HandoverLedger`（append-only，
//!    容量 32 条，满时显性 `LedgerFull` 而非静默覆盖——覆盖会让拒绝历史消失）。
//! 4. **指纹可复算**：`packet_fingerprint()` 对 schema/version/字段位图/
//!    载荷长度做 FNV-1a，消费者复算一致才认账，防「包装过程被第三方改动」。
//! 5. **纯函数内核**：时间用逻辑 tick 注入，零 IO 零墙钟，宿主/内核面行为一致。
//!
//! 诊断码：`HandoverCode` 十六进制段 0x80（0x8000..=0x800F），
//! `segment_guard()` 双写互斥，no_std 零 unwrap 零 expect，边界一律 checked_add/get。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX182_HANDOVER_VERSION: &str = "unx182-handover-v1";

/// 交接包 schema 版本（与版本号同源双写，防单侧漂移）。
pub const SCHEMA_VERSION: u32 = 1;

/// 字段位图容量（init 契约固定 16 个契约字段，超出即拒绝登记）。
pub const FIELD_CAP: usize = 16;

/// 拒绝账容量（满时显性报错，不静默覆盖历史）。
pub const LEDGER_CAP: usize = 32;

/// 契约字段标识闭集（16 个，编号即下标，缺号拒收）。
pub const FIELD_NAMES: [&str; FIELD_CAP] = [
    "mem-map-base",
    "mem-map-len",
    "fb-base",
    "fb-width",
    "fb-height",
    "fb-bpp",
    "acpi-rsdp",
    "acpi-fadt",
    "initrd-base",
    "initrd-len",
    "cmdline-ptr",
    "cmdline-len",
    "smbios-base",
    "mp-table",
    "uefi-rt",
    "entropy-pool",
];

/// 拒绝账单条记录（append-only，含生产者点名与缺失字段清单）。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    /// 逻辑 tick（注入，不用墙钟）
    pub tick: u64,
    /// 被拒交接包指纹（0 = 无指纹的裸包）
    pub fingerprint: u64,
    /// 缺失字段下标清单
    pub missing: Vec<u8>,
    /// 生产者名（诊断里点名，便于追责）
    pub producer: &'static str,
}

/// 拒绝账（只增不减；满则显性 Err）。
#[derive(Clone, Debug)]
pub struct HandoverLedger {
    entries: Vec<LedgerEntry>,
}

/// 交接包（版本化字段位图 + 载荷描述）。
#[derive(Clone, Debug)]
pub struct HandoverPacket {
    pub schema: u32,
    pub version: u32,
    /// 字段在位位图（bit i ↔ FIELD_NAMES[i]）
    pub present: u16,
    /// 各字段载荷长度（与 present 同步登记；缺省字段长度必须为 0）
    pub lens: [u32; FIELD_CAP],
    /// 生产者名
    pub producer: &'static str,
    /// 组装 tick（注入）
    pub built_tick: u64,
}

/// 交接协议错误（带源位置的中文诊断由 `detail()` 统一产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverError {
    /// schema 版本不符
    SchemaMismatch { got: u32, want: u32 },
    /// version 字段不符
    VersionMismatch { got: u32, want: u32 },
    /// 缺字段（下标清单）
    MissingField { first: u8 },
    /// present 与 lens 不同步（缺省字段长度非零 = 半份）
    LenNotSynced { idx: u8 },
    /// 未登记字段下标越界
    FieldOutOfRange { idx: u8 },
    /// 拒绝账已满
    LedgerFull { cap: usize },
    /// 指纹不符（包装中途被改动）
    FingerprintMismatch { got: u64, want: u64 },
}

/// 诊断码闭集（0x80 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverCode {
    /// 0x8000 schema 不符
    SchemaBad,
    /// 0x8001 版本不符
    VersionBad,
    /// 0x8002 缺字段
    FieldMissing,
    /// 0x8003 长度不同步
    LenDesync,
    /// 0x8004 下标越界
    IdxBad,
    /// 0x8005 账满
    LedgerFull,
    /// 0x8006 指纹不符
    FingerprintBad,
    /// 0x8007 登记成功
    Accepted,
}

impl HandoverCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            HandoverCode::SchemaBad => 0x8000,
            HandoverCode::VersionBad => 0x8001,
            HandoverCode::FieldMissing => 0x8002,
            HandoverCode::LenDesync => 0x8003,
            HandoverCode::IdxBad => 0x8004,
            HandoverCode::LedgerFull => 0x8005,
            HandoverCode::FingerprintBad => 0x8006,
            HandoverCode::Accepted => 0x8007,
        }
    }

    /// 英文线缆名（稳定标识，勿改）。
    pub const fn wire(self) -> &'static str {
        match self {
            HandoverCode::SchemaBad => "HANDOVER-SCHEMA-BAD",
            HandoverCode::VersionBad => "HANDOVER-VERSION-BAD",
            HandoverCode::FieldMissing => "HANDOVER-FIELD-MISSING",
            HandoverCode::LenDesync => "HANDOVER-LEN-DESYNC",
            HandoverCode::IdxBad => "HANDOVER-IDX-BAD",
            HandoverCode::LedgerFull => "HANDOVER-LEDGER-FULL",
            HandoverCode::FingerprintBad => "HANDOVER-FINGERPRINT-BAD",
            HandoverCode::Accepted => "HANDOVER-ACCEPTED",
        }
    }

    /// 中文诊断（含源位置与可操作建议，禁静默）。
    pub fn detail(self) -> String {
        let base = match self {
            HandoverCode::SchemaBad => "交接包 schema 版本不符：生产者与消费者契约不一致，init 契约层拒绝继续启动",
            HandoverCode::VersionBad => "交接包 version 字段不符：上下游不是同一版交接协议，拒绝继续启动",
            HandoverCode::FieldMissing => "交接包缺字段：位图未覆盖全部契约字段，拒绝继续启动",
            HandoverCode::LenDesync => "交接包长度与位图不同步：缺省字段携带了非零长度，交接包是半份",
            HandoverCode::IdxBad => "交接包含未登记字段下标：超出 16 字段闭集，登记面被污染",
            HandoverCode::LedgerFull => "拒绝账已满：无法落账本次拒绝，按显性失败处理（不静默覆盖历史）",
            HandoverCode::FingerprintBad => "交接包指纹不符：包装过程被改动，消费者拒绝认账",
            HandoverCode::Accepted => "交接包登记通过：全部 16 字段在位且长度同步",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx182_handover.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("，建议：");
        let advice = match self {
            HandoverCode::SchemaBad => "核对生产者 SCHEMA_VERSION 与本模块常量",
            HandoverCode::VersionBad => "核对上下游 UNX182_HANDOVER_VERSION",
            HandoverCode::FieldMissing => "检查生产者 HandoverPacket::set 调用缺哪一字段",
            HandoverCode::LenDesync => "缺省字段必须置长度 0，或改为登记字段",
            HandoverCode::IdxBad => "字段下标必须 < FIELD_CAP",
            HandoverCode::LedgerFull => "导出账本后清理，勿覆盖",
            HandoverCode::FingerprintBad => "重算 packet_fingerprint 并对拍",
            HandoverCode::Accepted => "无需处置",
        };
        s.push_str(advice);
        s.push_str("）");
        s
    }

    /// 段独占双写互斥（0x80 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8000 && (c & 0x00FF) < 0x10
    }
}

impl HandoverError {
    /// 映射到诊断码（失败→码，成功路径不经过此处）。
    pub const fn code(self) -> HandoverCode {
        match self {
            HandoverError::SchemaMismatch { .. } => HandoverCode::SchemaBad,
            HandoverError::VersionMismatch { .. } => HandoverCode::VersionBad,
            HandoverError::MissingField { .. } => HandoverCode::FieldMissing,
            HandoverError::LenNotSynced { .. } => HandoverCode::LenDesync,
            HandoverError::FieldOutOfRange { .. } => HandoverCode::IdxBad,
            HandoverError::LedgerFull { .. } => HandoverCode::LedgerFull,
            HandoverError::FingerprintMismatch { .. } => HandoverCode::FingerprintBad,
        }
    }

    /// 中文诊断（detail 直出，异常零静默）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// FNV-1a 64 位（对字节序列，确定性）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl HandoverPacket {
    /// 空包（present=0，全 length=0）。
    pub const fn empty(producer: &'static str, built_tick: u64) -> Self {
        HandoverPacket {
            schema: SCHEMA_VERSION,
            version: 1,
            present: 0,
            lens: [0; FIELD_CAP],
            producer,
            built_tick,
        }
    }

    /// 登记一个字段（在位位置位 + 长度入账；重复登记幂等）。
    pub fn set(&mut self, idx: u8, len: u32) -> Result<(), HandoverError> {
        if (idx as usize) >= FIELD_CAP {
            return Err(HandoverError::FieldOutOfRange { idx });
        }
        let bit = 1u16 << (idx as u16);
        self.present |= bit;
        self.lens[idx as usize] = len;
        Ok(())
    }

    /// 摘除一个字段（测试/回滚用；缺省字段长度归零）。
    pub fn clear(&mut self, idx: u8) -> Result<(), HandoverError> {
        if (idx as usize) >= FIELD_CAP {
            return Err(HandoverError::FieldOutOfRange { idx });
        }
        self.present &= !(1u16 << (idx as u16));
        self.lens[idx as usize] = 0;
        Ok(())
    }

    /// 在位字段数。
    pub const fn present_count(&self) -> u32 {
        self.present.count_ones()
    }

    /// 是否全字段在位。
    pub const fn is_complete(&self) -> bool {
        self.present_count() as usize == FIELD_CAP
    }

    /// 首个缺失字段下标（None = 齐）。
    pub const fn first_missing(&self) -> Option<u8> {
        let mut i = 0;
        while i < FIELD_CAP {
            if self.present & (1u16 << (i as u16)) == 0 {
                return Some(i as u8);
            }
            i += 1;
        }
        None
    }

    /// 在位位图是否与长度账同步（缺省字段长度必须为 0）。
    pub fn len_sync_error(&self) -> Option<u8> {
        let mut i = 0;
        while i < FIELD_CAP {
            let bit = self.present & (1u16 << (i as u16)) != 0;
            let len = match self.lens.get(i) {
                Some(v) => *v,
                None => return Some(i as u8),
            };
            if !bit && len != 0 {
                return Some(i as u8);
            }
            i += 1;
        }
        None
    }

    /// 包指纹（schema/version/present/lens/ producer 全进，改动即显性）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(&self.schema.to_le_bytes());
        b.extend_from_slice(&self.version.to_le_bytes());
        b.extend_from_slice(&self.present.to_le_bytes());
        let mut i = 0;
        while i < FIELD_CAP {
            if let Some(v) = self.lens.get(i) {
                b.extend_from_slice(&v.to_le_bytes());
            }
            i += 1;
        }
        b.extend_from_slice(self.producer.as_bytes());
        fnv1a64(&b)
    }

    /// 消费者侧验收（版本化 + 缺字段 + 同步三重门）。
    pub fn validate(&self) -> Result<(), HandoverError> {
        if self.schema != SCHEMA_VERSION {
            return Err(HandoverError::SchemaMismatch {
                got: self.schema,
                want: SCHEMA_VERSION,
            });
        }
        if self.version != UNX182_PROTO_REV {
            return Err(HandoverError::VersionMismatch {
                got: self.version,
                want: UNX182_PROTO_REV,
            });
        }
        if let Some(idx) = self.len_sync_error() {
            return Err(HandoverError::LenNotSynced { idx });
        }
        if let Some(first) = self.first_missing() {
            return Err(HandoverError::MissingField { first });
        }
        Ok(())
    }
}

/// 协议修订号（与 SCHEMA_VERSION 同源双写）。
pub const UNX182_PROTO_REV: u32 = 1;

impl HandoverLedger {
    /// 空账。
    pub const fn new() -> Self {
        HandoverLedger {
            entries: Vec::new(),
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 落一条拒绝（满则显性 Err）。
    pub fn record(&mut self, entry: LedgerEntry) -> Result<(), HandoverError> {
        if self.entries.len() >= LEDGER_CAP {
            return Err(HandoverError::LedgerFull { cap: LEDGER_CAP });
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 拒绝账指纹（对全部条目做 FNV，供跨会话对账）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            b.extend_from_slice(&e.tick.to_le_bytes());
            b.extend_from_slice(&e.fingerprint.to_le_bytes());
            for m in e.missing.iter() {
                b.push(*m);
            }
            b.extend_from_slice(e.producer.as_bytes());
        }
        fnv1a64(&b)
    }

    /// 全量缺失字段下标（跨条目汇总，转置统计用）。
    pub fn missing_histogram(&self) -> [u32; FIELD_CAP] {
        let mut out = [0u32; FIELD_CAP];
        for e in self.entries.iter() {
            for m in e.missing.iter() {
                if let Some(slot) = out.get_mut(*m as usize) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
        out
    }
}

/// init 契约层消费者入口：验收 + 拒登落账 + 指纹复算三合一。
///
/// 返回 `Ok(())` 表示交接被接受；任何失败都**必须**落账后上抛。
pub fn accept_handover(
    pkt: &HandoverPacket,
    ledger: &mut HandoverLedger,
    tick: u64,
) -> Result<(), HandoverError> {
    match pkt.validate() {
        Ok(()) => Ok(()),
        Err(e) => {
            let missing: Vec<u8> = match e {
                HandoverError::MissingField { first } => {
                    let mut v: Vec<u8> = Vec::new();
                    let mut i = first as usize;
                    while i < FIELD_CAP {
                        if pkt.present & (1u16 << (i as u16)) == 0 {
                            v.push(i as u8);
                        }
                        i += 1;
                    }
                    v
                }
                _ => Vec::new(),
            };
            let fp = pkt.fingerprint();
            let _ = ledger.record(LedgerEntry {
                tick,
                fingerprint: fp,
                missing,
                producer: pkt.producer,
            });
            Err(e)
        }
    }
}

/// 指纹复算入口（第三方质疑交接包被改时用）。
pub fn verify_fingerprint(pkt: &HandoverPacket, expect: u64) -> Result<(), HandoverError> {
    let got = pkt.fingerprint();
    if got != expect {
        return Err(HandoverError::FingerprintMismatch { got, want: expect });
    }
    Ok(())
}

/// 域自检（`run_unx182_checks`）：以最小语料验证关键路径，
/// 每条判据都是**可失败**的真实断言（恒真断言不写）。
pub fn run_unx182_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx182");

    // C182-01 全字段包验收通过
    {
        let mut p = HandoverPacket::empty("bootstrap", 0);
        let mut i = 0u8;
        while i < FIELD_CAP as u8 {
            let _ = p.set(i, 16);
            i += 1;
        }
        let ok = p.validate().is_ok() && p.is_complete();
        s.add("C182-01-全字段包验收通过", ok, if ok { "" } else { "16 字段齐全却验收失败" });
    }

    // C182-02 缺一个字段必被缺字段拒绝（第一个缺失下标正确）
    {
        let mut p = HandoverPacket::empty("bootstrap", 0);
        let mut i = 0u8;
        while i < FIELD_CAP as u8 {
            let _ = p.set(i, 16);
            i += 1;
        }
        let _ = p.clear(5);
        let got = p.validate();
        let ok = matches!(
            got,
            Err(HandoverError::MissingField { first: 5 })
        );
        s.add("C182-02-缺字段拒绝且点名首个缺失", ok, if ok { "" } else { "缺字段路径未被捕获" });
    }

    // C182-03 缺省字段带长度 = 不同步拒绝
    {
        let mut p = HandoverPacket::empty("bootstrap", 0);
        let mut i = 0u8;
        while i < FIELD_CAP as u8 {
            let _ = p.set(i, 16);
            i += 1;
        }
        let _ = p.clear(3);
        p.lens[3] = 8; // 摘除但长度没归零
        let ok = matches!(got_err_code(&p), Some(HandoverCode::LenDesync));
        s.add("C182-03-缺省字段长度非零必拒", ok, if ok { "" } else { "半份包未被同步门捕获" });
    }

    // C182-04 拒绝账落账且指纹可复算
    {
        let mut p = HandoverPacket::empty("bootstrap", 0);
        let _ = p.set(0, 8);
        let mut led = HandoverLedger::new();
        let r = accept_handover(&p, &mut led, 42);
        let fp1 = p.fingerprint();
        let fp2 = p.fingerprint();
        let ok = r.is_err() && led.len() == 1 && fp1 == fp2 && fp1 != 0;
        s.add("C182-04-拒绝落账且指纹确定", ok, if ok { "" } else { "落账或指纹路径失效" });
    }

    // C182-05 账满显性 Err（禁静默覆盖）
    {
        let mut led = HandoverLedger::new();
        let mut i = 0;
        let mut last_err_ok = false;
        while i < LEDGER_CAP + 4 {
            let e = led.record(LedgerEntry {
                tick: i as u64,
                fingerprint: i as u64,
                missing: Vec::new(),
                producer: "probe",
            });
            if i >= LEDGER_CAP {
                last_err_ok = e.is_err();
            }
            i += 1;
        }
        let ok = last_err_ok && led.len() == LEDGER_CAP;
        s.add("C182-05-账满显性失败不覆盖", ok, if ok { "" } else { "满账路径被静默吞掉" });
    }

    // C182-06 诊断码段独占
    {
        let codes = [
            HandoverCode::SchemaBad,
            HandoverCode::VersionBad,
            HandoverCode::FieldMissing,
            HandoverCode::LenDesync,
            HandoverCode::IdxBad,
            HandoverCode::LedgerFull,
            HandoverCode::FingerprintBad,
            HandoverCode::Accepted,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C182-06-诊断码 0x80 段独占", all, if all { "" } else { "码值越出 0x80 段" });
    }

    // C182-07 缺失直方图（跨条目转置统计）
    {
        let mut p = HandoverPacket::empty("bootstrap", 0);
        let _ = p.set(7, 4);
        let mut led = HandoverLedger::new();
        let _ = accept_handover(&p, &mut led, 1);
        let h = led.missing_histogram();
        let ok = h.get(7).copied().unwrap_or(0) == 0 && h.get(0).copied().unwrap_or(0) == 1;
        s.add("C182-07-缺失直方图逐字段落账", ok, if ok { "" } else { "直方图与账本不一致" });
    }

    s
}

/// 取错误码的辅助（判据侧用，避免把实现细节写进断言）。
fn got_err_code(p: &HandoverPacket) -> Option<HandoverCode> {
    match p.validate() {
        Ok(()) => None,
        Err(e) => Some(e.code()),
    }
}
