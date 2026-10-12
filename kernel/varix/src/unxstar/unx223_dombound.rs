//! unx223_dombound ↔ UNX-F0223 域边界清单（A1 与邻域接口总表）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0223，UNX-A1-B12）：
//! 「域边界清单（A1 与邻域接口总表）| 260 | 已深化 |
//!  UNX-F0223-J1 对 A2/A4/A5/B4/L1/K4 的接口冻结点逐一登记，缺联签项告警」
//!
//! 设计要点（全部围绕引导域 A1 与邻域协作的真实状态）：
//! 1. **邻域闭集**：A1 与六个邻域有接口冻结点——A2（中断接管）、A4（内存
//!    归并）、A5（早期堆）、B4（PCI 枚举）、L1（日志面）、K4（时钟面）；
//!    闭集外域名登记即拒（表外接口不存在，登记它=账本被污染）。
//! 2. **联签制**：每个冻结点需 A1 侧与邻域侧**双方联签**（契约号+指纹+
//!    双方工号）；只签了一方 = 缺联签 → **告警账**（append-only），
//!    不静默放行也不阻断（引导继续，但欠账显性挂着）。
//! 3. **冻结点指纹**：接口契约内容（方向/数据面/时序窗口三段）FNV 指纹，
//!    双方各自落账后指纹必须一致——不一致 = 两侧理解分叉，升级告警。
//! 4. **完整性口径**：6 邻域 × 1 主冻结点 = 6 项；全部联签 = 清单齐备；
//!    缺任何一项，`completeness()` 返回缺口清单（点名域与缺什么签）。
//! 5. **纯函数内核**：零 IO 零墙钟，tick 注入，快照 FNV 可复算。
//!
//! 诊断码：`DomBoundCode` 十六进制段 0x92（0x9200..=0x920F）独占。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX223_DOMBOUND_VERSION: &str = "unx223-dombound-v1";

/// 邻域闭集（6 个；编号即下标，表外登记拒收）。
pub const NEIGHBOR_DOMAINS: [&str; 6] = ["A2", "A4", "A5", "B4", "L1", "K4"];

/// 邻域职责一句话（诊断上下文，登记时带出）。
pub const NEIGHBOR_ROLES: [&str; 6] = [
    "中断接管（AP 静默窗交接）",
    "内存归并（E820 排序归并）",
    "早期堆（堆窗与白名单）",
    "PCI 枚举（ECAM/CF8 裁定）",
    "日志面（四级 emit 闸）",
    "时钟面（锚点与 RTC 语义）",
];

/// 告警账容量（append-only）。
pub const ALARM_LEDGER_CAP: usize = 32;

/// 契约方向闭集（A1 视角）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfaceDir {
    /// A1 生产、邻域消费
    Produce,
    /// A1 消费、邻域生产
    Consume,
    /// 双向协商
    Negotiate,
}

/// 单个接口冻结点（联签主体）。
#[derive(Clone, Debug)]
pub struct FreezePoint {
    /// 邻域下标（NEIGHBOR_DOMAINS 下标）
    pub domain: u8,
    /// 契约号（如 "A1-A2-IF01"）
    pub contract_id: &'static str,
    /// 接口方向
    pub dir: IfaceDir,
    /// 契约内容指纹（三段语义的 FNV）
    pub fingerprint: u64,
    /// A1 侧联签工号（None = 未签）
    pub signed_a1: Option<&'static str>,
    /// 邻域侧联签工号（None = 未签）
    pub signed_peer: Option<&'static str>,
    /// 登记 tick
    pub registered_tick: u64,
}

/// 告警账单条（append-only；缺联签/指纹分叉都在这）。
#[derive(Clone, Debug)]
pub struct AlarmEntry {
    pub tick: u64,
    pub domain: u8,
    pub kind: AlarmKind,
    pub detail: &'static str,
}

/// 告警种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmKind {
    /// 缺 A1 侧联签
    MissingA1Sign,
    /// 缺邻域侧联签
    MissingPeerSign,
    /// 双方指纹分叉（理解不一致）
    FingerprintDiverge,
}

impl AlarmKind {
    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            AlarmKind::MissingA1Sign => "DOMBOUND-MISSING-A1-SIGN",
            AlarmKind::MissingPeerSign => "DOMBOUND-MISSING-PEER-SIGN",
            AlarmKind::FingerprintDiverge => "DOMBOUND-FP-DIVERGE",
        }
    }
}

/// 诊断码闭集（0x92 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomBoundCode {
    /// 0x9200 邻域下标越界（表外域）
    DomainOutOfRange,
    /// 0x9201 冻结点重复登记
    DuplicateFreeze,
    /// 0x9202 告警账已满
    LedgerFull,
    /// 0x9203 契约号为空（无契约的冻结点不可签）
    ContractEmpty,
    /// 0x9204 联签通过（成功码）
    Countersigned,
}

impl DomBoundCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            DomBoundCode::DomainOutOfRange => 0x9200,
            DomBoundCode::DuplicateFreeze => 0x9201,
            DomBoundCode::LedgerFull => 0x9202,
            DomBoundCode::ContractEmpty => 0x9203,
            DomBoundCode::Countersigned => 0x9204,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            DomBoundCode::DomainOutOfRange => "DOMBOUND-DOMAIN-OUT-OF-RANGE",
            DomBoundCode::DuplicateFreeze => "DOMBOUND-DUPLICATE-FREEZE",
            DomBoundCode::LedgerFull => "DOMBOUND-LEDGER-FULL",
            DomBoundCode::ContractEmpty => "DOMBOUND-CONTRACT-EMPTY",
            DomBoundCode::Countersigned => "DOMBOUND-COUNTERSIGNED",
        }
    }

    /// 段独占双写互斥（0x92 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9200 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            DomBoundCode::DomainOutOfRange => "邻域下标越界：登记了闭集外域，A1 只与 A2/A4/A5/B4/L1/K4 有接口",
            DomBoundCode::DuplicateFreeze => "冻结点重复登记：同域已有冻结点在账，重复登记=账本被污染",
            DomBoundCode::LedgerFull => "告警账已满：无法记录本次告警，按显性失败处理",
            DomBoundCode::ContractEmpty => "契约号为空：无契约号的冻结点不可联签",
            DomBoundCode::Countersigned => "联签通过：双方指纹一致且双侧签齐",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx223_dombound.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 边界清单错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomBoundError {
    DomainOutOfRange { idx: u8 },
    DuplicateFreeze { domain: u8 },
    LedgerFull { cap: usize },
    ContractEmpty,
}

impl DomBoundError {
    /// 映射诊断码。
    pub const fn code(self) -> DomBoundCode {
        match self {
            DomBoundError::DomainOutOfRange { .. } => DomBoundCode::DomainOutOfRange,
            DomBoundError::DuplicateFreeze { .. } => DomBoundCode::DuplicateFreeze,
            DomBoundError::LedgerFull { .. } => DomBoundCode::LedgerFull,
            DomBoundError::ContractEmpty => DomBoundCode::ContractEmpty,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

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

/// 接口契约三段语义（方向/数据面/时序窗口），指纹从这里算。
#[derive(Clone, Copy)]
pub struct IfaceSemantics<'a> {
    pub direction: &'a str,
    pub data_plane: &'a str,
    pub timing_window: &'a str,
}

/// 接口契约三段语义 → 冻结指纹（双方各自复算，一致才算同一份契约）。
pub fn semantics_fingerprint(sem: &IfaceSemantics) -> u64 {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(sem.direction.as_bytes());
    b.push(0x1F);
    b.extend_from_slice(sem.data_plane.as_bytes());
    b.push(0x1F);
    b.extend_from_slice(sem.timing_window.as_bytes());
    fnv1a64(&b)
}

/// 域边界清单（A1 视角登记面）。
#[derive(Clone, Debug)]
pub struct DomainBoundary {
    freeze: Vec<FreezePoint>,
    alarms: Vec<AlarmEntry>,
    peer_views: Vec<PeerView>,
}

impl DomainBoundary {
    /// 空清单。
    pub const fn new() -> Self {
        DomainBoundary {
            freeze: Vec::new(),
            alarms: Vec::new(),
            peer_views: Vec::new(),
        }
    }

    /// 登记冻结点（同域重复拒；契约号空拒；下标越界拒）。
    pub fn register(
        &mut self,
        domain: u8,
        contract_id: &'static str,
        dir: IfaceDir,
        fp: u64,
        tick: u64,
    ) -> Result<(), DomBoundError> {
        if (domain as usize) >= NEIGHBOR_DOMAINS.len() {
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        if contract_id.is_empty() {
            return Err(DomBoundError::ContractEmpty);
        }
        if self.freeze.iter().any(|f| f.domain == domain) {
            return Err(DomBoundError::DuplicateFreeze { domain });
        }
        self.freeze.push(FreezePoint {
            domain,
            contract_id,
            dir,
            fingerprint: fp,
            signed_a1: None,
            signed_peer: None,
            registered_tick: tick,
        });
        Ok(())
    }

    /// A1 侧联签（签后复核指纹分叉 → 告警）。
    pub fn sign_a1(&mut self, domain: u8, who: &'static str, tick: u64) -> Result<(), DomBoundError> {
        if (domain as usize) >= NEIGHBOR_DOMAINS.len() {
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        // 无冻结点先登记告警（缺联签 = 冻结点不在账）
        if !self.freeze.iter().any(|f| f.domain == domain) {
            self.push_alarm(tick, domain, AlarmKind::MissingPeerSign, "A1 侧先签但冻结点未登记")?;
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        if let Some(f) = self.freeze.iter_mut().find(|f| f.domain == domain) {
            f.signed_a1 = Some(who);
        }
        self.reevaluate(domain, tick);
        Ok(())
    }

    /// 邻域侧联签。
    pub fn sign_peer(&mut self, domain: u8, who: &'static str, tick: u64) -> Result<(), DomBoundError> {
        if (domain as usize) >= NEIGHBOR_DOMAINS.len() {
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        if !self.freeze.iter().any(|f| f.domain == domain) {
            self.push_alarm(tick, domain, AlarmKind::MissingA1Sign, "邻域侧先签但冻结点未登记")?;
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        if let Some(f) = self.freeze.iter_mut().find(|f| f.domain == domain) {
            f.signed_peer = Some(who);
        }
        self.reevaluate(domain, tick);
        Ok(())
    }

    /// 单点联签态复核：缺签/分叉 → 告警账（不静默、不阻断）。
    fn reevaluate(&mut self, domain: u8, tick: u64) {
        let fp_expect = match self.freeze.iter().find(|f| f.domain == domain) {
            Some(f) => f.fingerprint,
            None => return,
        };
        let (a1, peer) = match self.freeze.iter().find(|f| f.domain == domain) {
            Some(f) => (f.signed_a1.is_some(), f.signed_peer.is_some()),
            None => return,
        };
        // 指纹分叉判定：双侧都签了但账面指纹与对方复核指纹不同
        // （邻域复核指纹由 register_peer_view 落进 peer_view 表）
        let diverged = match self.peer_views.iter().find(|v| v.domain == domain) {
            Some(v) => v.peer_fingerprint != fp_expect,
            None => false,
        };
        if diverged {
            let _ = self.push_alarm(tick, domain, AlarmKind::FingerprintDiverge, "双方指纹分叉：两侧对同一契约理解不一致");
            return; // 分叉是最高级告警，缺签告警不再重复记
        }
        if a1 && !peer {
            // 缺邻域签：只记一次（同域同类告警去重，账不被刷爆）
            if !self.alarms.iter().any(|a| a.domain == domain && matches!(a.kind, AlarmKind::MissingPeerSign)) {
                let _ = self.push_alarm(tick, domain, AlarmKind::MissingPeerSign, "缺邻域侧联签");
            }
        }
        if peer && !a1 {
            if !self.alarms.iter().any(|a| a.domain == domain && matches!(a.kind, AlarmKind::MissingA1Sign)) {
                let _ = self.push_alarm(tick, domain, AlarmKind::MissingA1Sign, "缺 A1 侧联签");
            }
        }
    }

    /// 告警落账（满则显性 Err，不静默覆盖）。
    fn push_alarm(&mut self, tick: u64, domain: u8, kind: AlarmKind, detail: &'static str) -> Result<(), DomBoundError> {
        if self.alarms.len() >= ALARM_LEDGER_CAP {
            return Err(DomBoundError::LedgerFull { cap: ALARM_LEDGER_CAP });
        }
        self.alarms.push(AlarmEntry { tick, domain, kind, detail });
        Ok(())
    }

    /// 邻域视角指纹登记（对账用；分叉在 reevaluate 中显性化）。
    pub fn register_peer_view(&mut self, domain: u8, peer_fingerprint: u64, tick: u64) -> Result<(), DomBoundError> {
        if (domain as usize) >= NEIGHBOR_DOMAINS.len() {
            return Err(DomBoundError::DomainOutOfRange { idx: domain });
        }
        if let Some(v) = self.peer_views.iter_mut().find(|v| v.domain == domain) {
            v.peer_fingerprint = peer_fingerprint;
        } else {
            self.peer_views.push(PeerView { domain, peer_fingerprint, tick });
        }
        self.reevaluate(domain, tick);
        Ok(())
    }

    /// 清单齐备否：全部 6 域登记 + 双侧联签 + 无分叉。
    pub fn completeness(&self) -> (bool, Vec<u8>) {
        let mut missing: Vec<u8> = Vec::new();
        for d in 0..NEIGHBOR_DOMAINS.len() as u8 {
            let ok = match self.freeze.iter().find(|f| f.domain == d) {
                Some(f) => f.signed_a1.is_some() && f.signed_peer.is_some(),
                None => false,
            };
            let diverged = self
                .peer_views
                .iter()
                .any(|v| v.domain == d && v.peer_fingerprint != self.freeze.iter().find(|f| f.domain == d).map(|f| f.fingerprint).unwrap_or(0));
            if !ok || diverged {
                missing.push(d);
            }
        }
        (missing.is_empty(), missing)
    }

    /// 已联签点数。
    pub fn signed_count(&self) -> usize {
        self.freeze
            .iter()
            .filter(|f| f.signed_a1.is_some() && f.signed_peer.is_some())
            .count()
    }

    /// 冻结点数 / 告警条数。
    pub fn freeze_count(&self) -> usize {
        self.freeze.len()
    }
    pub fn alarm_count(&self) -> usize {
        self.alarms.len()
    }

    /// 清单快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for f in self.freeze.iter() {
            b.push(f.domain);
            b.extend_from_slice(f.contract_id.as_bytes());
            b.push(f.dir as u8);
            b.extend_from_slice(&f.fingerprint.to_le_bytes());
            b.push(if f.signed_a1.is_some() { 1 } else { 0 });
            b.push(if f.signed_peer.is_some() { 1 } else { 0 });
        }
        for a in self.alarms.iter() {
            b.extend_from_slice(&a.tick.to_le_bytes());
            b.push(a.domain);
            b.push(a.kind as u8);
        }
        for v in self.peer_views.iter() {
            b.push(v.domain);
            b.extend_from_slice(&v.peer_fingerprint.to_le_bytes());
        }
        fnv1a64(&b)
    }
}

/// 邻域视角指纹账（对账双源之一）。
#[derive(Clone, Copy, Debug)]
pub struct PeerView {
    pub domain: u8,
    pub peer_fingerprint: u64,
    pub tick: u64,
}

/// 域自检（`run_unx223_checks`）：J1 判据逐条可失败断言。
pub fn run_unx223_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx223");

    // C223-01 六域冻结点全登记 + 双侧联签 = 齐备（J1 正面）
    {
        let mut b = DomainBoundary::new();
        let mut all_ok = true;
        for d in 0..6u8 {
            all_ok &= b.register(d, "A1-IF", IfaceDir::Negotiate, 0x100 + d as u64, d as u64).is_ok();
        }
        for d in 0..6u8 {
            all_ok &= b.sign_a1(d, "A1-worker", d as u64).is_ok();
            all_ok &= b.sign_peer(d, "peer-worker", d as u64).is_ok();
        }
        let (complete, missing) = b.completeness();
        all_ok &= complete && missing.is_empty() && b.signed_count() == 6;
        s.add("C223-01-六域全联签齐备", all_ok, if all_ok { "" } else { "六域联签未齐" });
    }

    // C223-02 缺联签项告警（J1 核心：缺联签必须显性告警）
    {
        let mut b = DomainBoundary::new();
        for d in 0..6u8 {
            let _ = b.register(d, "A1-IF", IfaceDir::Produce, 0x200 + d as u64, d as u64);
        }
        // 只签 A1 侧：6 域全部缺邻域签
        for d in 0..6u8 {
            let _ = b.sign_a1(d, "A1-worker", d as u64);
        }
        let (complete, missing) = b.completeness();
        let ok = !complete && missing.len() == 6 && b.alarm_count() >= 6;
        s.add("C223-02-缺联签逐项告警", ok, if ok { "" } else { "缺联签未被告警" });
    }

    // C223-03 指纹分叉升级告警（双方签了但理解不一致）
    {
        let sem = IfaceSemantics {
            direction: "A1-produce-peer-consume",
            data_plane: "handover-packet-v1",
            timing_window: "before-ap-silence-lift",
        };
        let fp = semantics_fingerprint(&sem);
        let mut b = DomainBoundary::new();
        let _ = b.register(0, "A1-A2-IF01", IfaceDir::Produce, fp, 1);
        // 邻域用不同语义算指纹 → 分叉
        let other_fp = semantics_fingerprint(&IfaceSemantics {
            direction: "A1-produce-peer-consume",
            data_plane: "handover-packet-v2", // 版本不同 → 指纹必变
            timing_window: "before-ap-silence-lift",
        });
        let _ = b.register_peer_view(0, other_fp, 2);
        let _ = b.sign_a1(0, "A1", 3);
        let _ = b.sign_peer(0, "A2", 4);
        let diverged = b
            .alarms
            .iter()
            .any(|a| a.domain == 0 && matches!(a.kind, AlarmKind::FingerprintDiverge));
        let (complete, _) = b.completeness();
        let ok = diverged && !complete;
        s.add("C223-03-指纹分叉升级告警", ok, if ok { "" } else { "分叉未被检出" });
    }

    // C223-04 表外域登记拒绝（闭集外不存在接口）
    {
        let mut b = DomainBoundary::new();
        let r = b.register(9, "A1-IF", IfaceDir::Produce, 1, 0);
        let ok = matches!(r, Err(DomBoundError::DomainOutOfRange { idx: 9 }));
        s.add("C223-04-表外域拒绝", ok, if ok { "" } else { "表外域被登记" });
    }

    // C223-05 同域重复登记拒绝
    {
        let mut b = DomainBoundary::new();
        let r1 = b.register(2, "A1-A5-IF01", IfaceDir::Consume, 0x10, 0);
        let r2 = b.register(2, "A1-A5-IF02", IfaceDir::Consume, 0x11, 1);
        let ok = r1.is_ok() && matches!(r2, Err(DomBoundError::DuplicateFreeze { domain: 2 }));
        s.add("C223-05-重复登记拒绝", ok, if ok { "" } else { "同域重复登记未被拦" });
    }

    // C223-06 契约号为空拒绝
    {
        let mut b = DomainBoundary::new();
        let r = b.register(1, "", IfaceDir::Produce, 1, 0);
        let ok = matches!(r, Err(DomBoundError::ContractEmpty));
        s.add("C223-06-空契约号拒绝", ok, if ok { "" } else { "空契约号被放行" });
    }

    // C223-07 告警去重：同域同类告警只记一次（账不被刷爆）
    {
        let mut b = DomainBoundary::new();
        let _ = b.register(4, "A1-L1-IF01", IfaceDir::Produce, 0x30, 0);
        let _ = b.sign_a1(4, "A1", 1);
        // 反复 reevaluate（反复 sign 同侧）不应追加重复告警
        for t in 2..6u64 {
            let _ = b.sign_a1(4, "A1", t);
        }
        let missing_peer_alarms = b
            .alarms
            .iter()
            .filter(|a| a.domain == 4 && matches!(a.kind, AlarmKind::MissingPeerSign))
            .count();
        let ok = missing_peer_alarms == 1;
        s.add("C223-07-同类告警去重", ok, if ok { "" } else { "告警被重复刷账" });
    }

    // C223-08 指纹语义敏感（三段任一变更指纹必变）
    {
        let base = semantics_fingerprint(&IfaceSemantics { direction: "d", data_plane: "p", timing_window: "w" });
        let d1 = semantics_fingerprint(&IfaceSemantics { direction: "d2", data_plane: "p", timing_window: "w" });
        let d2 = semantics_fingerprint(&IfaceSemantics { direction: "d", data_plane: "p2", timing_window: "w" });
        let d3 = semantics_fingerprint(&IfaceSemantics { direction: "d", data_plane: "p", timing_window: "w2" });
        let ok = base != d1 && base != d2 && base != d3 && base != 0;
        s.add("C223-08-契约指纹三段敏感", ok, if ok { "" } else { "指纹对契约变更不敏感" });
    }

    // C223-09 快照指纹确定且签态敏感
    {
        let mut b1 = DomainBoundary::new();
        let mut b2 = DomainBoundary::new();
        for b in [&mut b1, &mut b2].iter_mut() {
            let _ = b.register(0, "A1-A2-IF01", IfaceDir::Negotiate, 0x77, 0);
        }
        let f1a = b1.fingerprint();
        let f1b = b1.fingerprint();
        let _ = b2.sign_a1(0, "A1", 1);
        let f2 = b2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C223-09-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C223-10 诊断码 0x92 段独占
    {
        let codes = [
            DomBoundCode::DomainOutOfRange,
            DomBoundCode::DuplicateFreeze,
            DomBoundCode::LedgerFull,
            DomBoundCode::ContractEmpty,
            DomBoundCode::Countersigned,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C223-10-诊断码 0x92 段独占", all, if all { "" } else { "码值越出 0x92 段" });
    }

    // C223-11 邻域职责上下文齐备（6 域每域有职责说明，诊断可用）
    {
        let ok = NEIGHBOR_ROLES.len() == NEIGHBOR_DOMAINS.len()
            && NEIGHBOR_ROLES.iter().all(|r| !r.is_empty());
        s.add("C223-11-邻域职责表齐备", ok, if ok { "" } else { "职责表与域表不对齐" });
    }

    s
}
