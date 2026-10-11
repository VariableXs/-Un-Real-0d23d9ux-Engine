//! unx1561_cosign —— UNX-F1561 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B39）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 目标 340 行 · 已深化
//! 判据原文：UNX-F1561-J1 联签总表齐备（全域消费域逐域登记：域/消费接口/
//!           联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! 组件：联签总表（全域消费域逐域三列登记：域名/消费接口/联签状态）+
//!      消费域闭集（A3 SMP/A4 负载/A5 均衡/G1 守护/B4 观测等 12 域冻结）+
//!      联签状态机（Unsigned→Pending→Cosigned 三态单向）+
//!      接口指纹账（每域消费的接口签名 FNV 在册）+
//!      漏域注入核（10 处漏域/冒签/越序注入 10/10 必抓）+
//!      F1566 冻结账对平核（总表 vs 冻结账逐域对平 100%）。
//! 职责定位：A2 产出的消费方登记的唯一权威表——谁消费哪些接口、签没签，
//!   全域逐域在册；漏域即下游消费无契约（接管面裸奔），漏域注入必抓
//!   是总表完备性的判据；与 F1566 冻结账对平 100% 是双账互证。
//! 性能逐项分解：登记 O(域数)=O(12)；对平 O(12)；指纹 O(签名长)。
//!   全整数确定性。
//! 诊断码段：0x52A0（0x52A1~0x52A8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CS_VERSION: &str = "unx1561_cosign-v1";

/// 诊断段基址（0x52A0 段 · 本模块独占）。
pub const CS_SEG_BASE: u16 = 0x52A0;

/// 消费域闭集（判据点名：A3/A4/A5/G1/B4 等——闭集 12 域）。
pub const CS_CONSUMER_DOMAINS: [&str; 12] = [
    "A3-SMP调度",
    "A4-负载追踪",
    "A5-均衡执行",
    "G1-守护面",
    "B4-观测面",
    "B5-遥测面",
    "C1-容错面",
    "C2-降级面",
    "D1-内存面",
    "E1-文件面",
    "F1-网络面",
    "W1-兼容面",
];

/// 联签状态三态（单向：Unsigned→Pending→Cosigned）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsState {
    /// 未签（域未登记联签）。
    Unsigned,
    /// 待签（接口已投递，等待域回签）。
    Pending,
    /// 已签（联签确认完成）。
    Cosigned,
}

impl CsState {
    /// 号。
    pub fn num(self) -> u8 {
        match self {
            CsState::Unsigned => 0,
            CsState::Pending => 1,
            CsState::Cosigned => 2,
        }
    }
    /// 中文。
    pub fn text(self) -> &'static str {
        match self {
            CsState::Unsigned => "未签",
            CsState::Pending => "待签",
            CsState::Cosigned => "已签",
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsCode {
    /// 0x52A1 漏域（闭集域未登记）。
    DomainMissing = 0x52A1,
    /// 0x52A2 闭集外域（冒名域）。
    DomainUnknown = 0x52A2,
    /// 0x52A3 联签越序（Unsigned 直跳 Cosigned）。
    StateSkipped = 0x52A3,
    /// 0x52A4 域重登。
    DomainDup = 0x52A4,
    /// 0x52A5 接口指纹漂移（同域同接口异指纹）。
    IfaceDrift = 0x52A5,
    /// 0x52A6 对平失败（总表 vs F1566 冻结账不齐）。
    ReconcileMismatch = 0x52A6,
    /// 0x52A7 消费接口空（域登记但无消费接口）。
    IfaceEmpty = 0x52A7,
    /// 0x52A8 冒签（未 Pending 的域直接回签成功）。
    ForgedCosign = 0x52A8,
}

impl CsCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            CsCode::DomainMissing => "漏域（消费域闭集内有域未登记）",
            CsCode::DomainUnknown => "闭集外域（冒名域登记被拒）",
            CsCode::StateSkipped => "联签越序（未签不可直跳已签）",
            CsCode::DomainDup => "域重登（一域一签）",
            CsCode::IfaceDrift => "接口指纹漂移（消费契约被改）",
            CsCode::ReconcileMismatch => "对平失败（总表与冻结账不齐）",
            CsCode::IfaceEmpty => "消费接口空（登记无契约）",
            CsCode::ForgedCosign => "冒签（未走待签流程的回签被拒）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            CsCode::DomainMissing => "按闭集逐域点验补登；漏域的下游消费立即冻结",
            CsCode::DomainUnknown => "新消费域须先进闭集（升版）再登记",
            CsCode::StateSkipped => "联签走三态单向：投递→待签→已签",
            CsCode::DomainDup => "同域更新走接口追加，不重登",
            CsCode::IfaceDrift => "接口签名是契约：漂移即升版重签",
            CsCode::ReconcileMismatch => "逐域对平找差异；两账必须同源",
            CsCode::IfaceEmpty => "登记时必须带至少一个消费接口",
            CsCode::ForgedCosign => "回签前必须先走待签；直签是伪造",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsDiag {
    /// 码。
    pub code: CsCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl CsDiag {
    /// 构造。
    pub fn new(code: CsCode, site: &str, detail: String) -> Self {
        CsDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 单域联签行（三列：域/消费接口/联签状态）。
#[derive(Clone, Debug)]
pub struct CsRow {
    /// 域名。
    pub domain: String,
    /// 消费接口签名（FNV of 接口名串）。
    pub iface_fp: u64,
    /// 消费接口名。
    pub iface_name: String,
    /// 联签状态。
    pub state: CsState,
}

impl CsRow {
    /// 接口指纹（FNV-1a）。
    pub fn iface_fingerprint(name: &str) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for b in name.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

/// 联签总表。
#[derive(Clone, Debug, Default)]
pub struct CsLedger {
    /// 行表（按登记序）。
    pub rows: Vec<CsRow>,
    /// 冻结指纹（0=未冻结）。
    pub seal_fp: u64,
}

impl CsLedger {
    /// 登记域（闭集闸 + 重登闸 + 接口空闸；初态 Unsigned）。
    pub fn enroll(&mut self, domain: &str, iface_name: &str) -> Result<(), CsDiag> {
        // 闭集闸。
        let mut known = false;
        for d in CS_CONSUMER_DOMAINS {
            if *d == *domain {
                known = true;
                break;
            }
        }
        if !known {
            return Err(CsDiag::new(CsCode::DomainUnknown, "enroll:set", format!("域 [{}]", domain)));
        }
        for r in &self.rows {
            if r.domain == domain {
                return Err(CsDiag::new(CsCode::DomainDup, "enroll:dup", format!("域 [{}]", domain)));
            }
        }
        if iface_name.is_empty() {
            return Err(CsDiag::new(CsCode::IfaceEmpty, "enroll:iface", format!("域 [{}]", domain)));
        }
        self.rows.push(CsRow {
            domain: String::from(domain),
            iface_name: String::from(iface_name),
            iface_fp: CsRow::iface_fingerprint(iface_name),
            state: CsState::Unsigned,
        });
        Ok(())
    }

    /// 投递待签（Unsigned→Pending）。
    pub fn offer(&mut self, domain: &str) -> Result<(), CsDiag> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].domain == domain {
                if self.rows[i].state != CsState::Unsigned {
                    return Err(CsDiag::new(
                        CsCode::StateSkipped,
                        "offer:state",
                        format!("域 [{}] 态 {:?}", domain, self.rows[i].state),
                    ));
                }
                self.rows[i].state = CsState::Pending;
                return Ok(());
            }
            i += 1;
        }
        Err(CsDiag::new(CsCode::DomainMissing, "offer:miss", format!("域 [{}] 未登记", domain)))
    }

    /// 回签（Pending→Cosigned；未 Pending 即冒签拒）。
    pub fn cosign(&mut self, domain: &str, iface_fp: u64) -> Result<(), CsDiag> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].domain == domain {
                if self.rows[i].state != CsState::Pending {
                    return Err(CsDiag::new(
                        CsCode::ForgedCosign,
                        "cosign:state",
                        format!("域 [{}] 态 {:?} 直签", domain, self.rows[i].state),
                    ));
                }
                if self.rows[i].iface_fp != iface_fp {
                    return Err(CsDiag::new(
                        CsCode::IfaceDrift,
                        "cosign:fp",
                        format!("域 [{}] 指纹 0x{:016X}≠0x{:016X}", domain, self.rows[i].iface_fp, iface_fp),
                    ));
                }
                self.rows[i].state = CsState::Cosigned;
                return Ok(());
            }
            i += 1;
        }
        Err(CsDiag::new(CsCode::DomainMissing, "cosign:miss", format!("域 [{}] 未登记", domain)))
    }

    /// 已签计数。
    pub fn cosigned_count(&self) -> usize {
        self.rows.iter().filter(|r| r.state == CsState::Cosigned).count()
    }

    /// 漏域核（判据点名：漏域注入必抓）——闭集 12 域全在册且全部已签才算齐备。
    pub fn complete(&self) -> (bool, Vec<String>) {
        let mut missing: Vec<String> = Vec::new();
        for d in CS_CONSUMER_DOMAINS {
            let mut found = false;
            for r in &self.rows {
                if r.domain == *d {
                    found = true;
                    if r.state != CsState::Cosigned {
                        missing.push(format!("{}(态:{})", d, r.state.text()));
                    }
                    break;
                }
            }
            if !found {
                missing.push(format!("{}(未登记)", d));
            }
        }
        (missing.is_empty(), missing)
    }

    /// F1566 冻结账对平（判据点名：对平 100%）。
    /// 冻结账 = 期望的（域,接口指纹）序对表（语义由调用方注入）。
    pub fn reconcile(&self, frozen: &[(&str, u64)]) -> Result<(), CsDiag> {
        if frozen.len() != self.rows.len() {
            return Err(CsDiag::new(
                CsCode::ReconcileMismatch,
                "reconcile:len",
                format!("冻结账 {} vs 总表 {}", frozen.len(), self.rows.len()),
            ));
        }
        for (dom, fp) in frozen {
            let mut hit = false;
            for r in &self.rows {
                if r.domain == *dom {
                    if r.iface_fp != *fp {
                        return Err(CsDiag::new(
                            CsCode::IfaceDrift,
                            "reconcile:fp",
                            format!("域 [{}] 指纹不齐", dom),
                        ));
                    }
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(CsDiag::new(
                    CsCode::ReconcileMismatch,
                    "reconcile:dom",
                    format!("域 [{}] 总表缺", dom),
                ));
            }
        }
        Ok(())
    }

    /// 总表指纹。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for r in &self.rows {
            for b in r.domain.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= r.iface_fp;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= r.state.num() as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 报表。
    pub fn report(&self) -> String {
        let (comp, miss) = self.complete();
        format!(
            "总表 {}/{} 已签:{} 齐备:{} 缺:{} 指纹:0x{:016X}",
            self.cosigned_count(),
            CS_CONSUMER_DOMAINS.len(),
            self.rows.len(),
            comp,
            miss.len(),
            self.fingerprint()
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全签演练：12 域全流程（登记→投递→回签）→ 齐备 → 对平。
pub fn drill_cosign() -> (bool, String) {
    let mut led = CsLedger::default();
    let ifaces = [
        "sched_engine", "load_window", "balance_chain", "guard_gate", "observe_bus",
        "telemetry_bus", "fault_iso", "degrade_ladder", "mem_ledger", "fs_ledger",
        "net_budget", "compat_route",
    ];
    let mut i = 0usize;
    while i < 12 {
        if led.enroll(CS_CONSUMER_DOMAINS[i], ifaces[i]).is_err() {
            return (false, format!("登记 {} 失败", CS_CONSUMER_DOMAINS[i]));
        }
        if led.offer(CS_CONSUMER_DOMAINS[i]).is_err() {
            return (false, format!("投递 {} 失败", CS_CONSUMER_DOMAINS[i]));
        }
        let fp = CsRow::iface_fingerprint(ifaces[i]);
        if led.cosign(CS_CONSUMER_DOMAINS[i], fp).is_err() {
            return (false, format!("回签 {} 失败", CS_CONSUMER_DOMAINS[i]));
        }
        i += 1;
    }
    let (comp, _) = led.complete();
    // 冻结账对平。
    let mut frozen: Vec<(&str, u64)> = Vec::new();
    let mut j = 0usize;
    while j < 12 {
        frozen.push((CS_CONSUMER_DOMAINS[j], CsRow::iface_fingerprint(ifaces[j])));
        j += 1;
    }
    let rec = led.reconcile(&frozen).is_ok();
    (comp && rec && led.cosigned_count() == 12, led.report())
}

/// 漏域/冒签注入核：10 处 10/10 必抓。
pub fn inject_leak() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：漏域（只登 11 域）。
    let mut led = CsLedger::default();
    let mut i = 0usize;
    while i < 11 {
        let _ = led.enroll(CS_CONSUMER_DOMAINS[i], "ifx");
        i += 1;
    }
    let (comp, miss) = led.complete();
    if !comp && !miss.is_empty() {
        caught += 1;
        detail.push("注1 漏域:抓".to_string());
    } else {
        detail.push("注1 漏域:漏".to_string());
    }
    // 注 2：闭集外域。
    if led.enroll("X9-幽灵域", "ifx").is_err() {
        caught += 1;
        detail.push("注2 闭集外:抓".to_string());
    } else {
        detail.push("注2 闭集外:漏".to_string());
    }
    // 注 3：域重登。
    let mut led3 = CsLedger::default();
    let _ = led3.enroll("A3-SMP调度", "ifx");
    if led3.enroll("A3-SMP调度", "ifx").is_err() {
        caught += 1;
        detail.push("注3 域重登:抓".to_string());
    } else {
        detail.push("注3 域重登:漏".to_string());
    }
    // 注 4：冒签（Unsigned 直签）。
    if led3.cosign("A3-SMP调度", CsRow::iface_fingerprint("ifx")).is_err() {
        caught += 1;
        detail.push("注4 冒签:抓".to_string());
    } else {
        detail.push("注4 冒签:漏".to_string());
    }
    // 注 5：越序（offer 后再 offer）。
    let mut led5 = CsLedger::default();
    let _ = led5.enroll("A4-负载追踪", "ifx");
    let _ = led5.offer("A4-负载追踪");
    if led5.offer("A4-负载追踪").is_err() {
        caught += 1;
        detail.push("注5 越序投递:抓".to_string());
    } else {
        detail.push("注5 越序投递:漏".to_string());
    }
    // 注 6：指纹漂移回签。
    let mut led6 = CsLedger::default();
    let _ = led6.enroll("B4-观测面", "observe_bus");
    let _ = led6.offer("B4-观测面");
    if led6.cosign("B4-观测面", 0xDEAD).is_err() {
        caught += 1;
        detail.push("注6 指纹漂移:抓".to_string());
    } else {
        detail.push("注6 指纹漂移:漏".to_string());
    }
    // 注 7：消费接口空。
    if led6.enroll("C1-容错面", "").is_err() {
        caught += 1;
        detail.push("注7 接口空:抓".to_string());
    } else {
        detail.push("注7 接口空:漏".to_string());
    }
    // 注 8：对平长度不齐。
    let mut led8 = CsLedger::default();
    let _ = led8.enroll("D1-内存面", "mem_ledger");
    if led8.reconcile(&[("D1-内存面", 1), ("E1-文件面", 2)]).is_err() {
        caught += 1;
        detail.push("注8 对平长度:抓".to_string());
    } else {
        detail.push("注8 对平长度:漏".to_string());
    }
    // 注 9：对平指纹不齐。
    let fp9 = CsRow::iface_fingerprint("mem_ledger");
    if led8.reconcile(&[("D1-内存面", fp9 ^ 1)]).is_err() {
        caught += 1;
        detail.push("注9 对平指纹:抓".to_string());
    } else {
        detail.push("注9 对平指纹:漏".to_string());
    }
    // 注 10：对平缺域。
    if led8.reconcile(&[("E1-文件面", 2)]).is_err() {
        caught += 1;
        detail.push("注10 对平缺域:抓".to_string());
    } else {
        detail.push("注10 对平缺域:漏".to_string());
    }
    (caught, 10, detail)
}
