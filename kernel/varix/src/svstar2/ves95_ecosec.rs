// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves95_ecosec
// VE-F3695 · 生态安全治理（R05 组 · 生态运营组）
//
// 锚点：VE-F3695
// 判据原文：外链 P0、洗钱 P0、情报协同、四面清单、反刷单源复述。
//
// 难点分析：
//   1. 资产带钓鱼链接=生态攻击——外链扫描是 P0 红线，扫描不到即视同放行攻击面。
//   2. 洗钱检测看交易模式异常（小额多笔归集/快进快出），命中即 P0 并法务联动。
//   3. 四面威胁清单（刷量产业链/钓鱼资产/恶意评价产业链/洗钱）须全量登记且面面有防线。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x8F00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x8F00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C8FCode(pub u16);

impl C8FCode {
    pub const PHISH_LINK: Self = Self(SEG_BASE);        // 钓鱼外链 P0
    pub const LAUNDER_PATTERN: Self = Self(SEG_BASE + 1); // 洗钱模式 P0
    pub const FARM_CHAIN: Self = Self(SEG_BASE + 2);    // 刷量产业链 P0
    pub const REVIEW_RING: Self = Self(SEG_BASE + 3);   // 恶意评价链
    pub const INTEL_BROKEN: Self = Self(SEG_BASE + 4);  // 情报断链
    pub const SCAN_CLEAN: Self = Self(SEG_BASE + 5);    // 扫描清洁
    pub const INTEL_SYNCED: Self = Self(SEG_BASE + 6);  // 情报协同

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::PHISH_LINK => "资产外链命中钓鱼特征——外链扫描红线 P0",
            Self::LAUNDER_PATTERN => "交易命中洗钱模式——洗钱检测红线 P0",
            Self::FARM_CHAIN => "刷量产业链检出——反刷单源复述",
            Self::REVIEW_RING => "恶意评价产业链检出——治理联动",
            Self::INTEL_BROKEN => "情报协同断链——协同修正",
            Self::SCAN_CLEAN => "外链扫描清洁",
            Self::INTEL_SYNCED => "情报已协同同步",
            _ => "未知诊断",
        }
    }
}

// ── 四面威胁清单 ──────────────────────────────────────────────────

/// 威胁面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreatFace {
    /// 刷量产业链。
    FarmChain,
    /// 钓鱼资产。
    PhishAsset,
    /// 恶意评价产业链。
    ReviewRing,
    /// 洗钱（虚拟分成）。
    Launder,
}

impl ThreatFace {
    pub fn name(self) -> &'static str {
        match self {
            Self::FarmChain => "刷量产业链",
            Self::PhishAsset => "钓鱼资产",
            Self::ReviewRing => "恶意评价产业链",
            Self::Launder => "洗钱",
        }
    }

    /// 四面全量。
    pub fn all() -> [ThreatFace; 4] {
        [Self::FarmChain, Self::PhishAsset, Self::ReviewRing, Self::Launder]
    }

    /// 对应诊断码。
    pub fn code(self) -> C8FCode {
        match self {
            Self::FarmChain => C8FCode::FARM_CHAIN,
            Self::PhishAsset => C8FCode::PHISH_LINK,
            Self::ReviewRing => C8FCode::REVIEW_RING,
            Self::Launder => C8FCode::LAUNDER_PATTERN,
        }
    }
}

/// 四面清单登记表——面面有防线。
pub struct ThreatRegister {
    /// 每面防线是否就位。
    defense_up: [bool; 4],
    /// 每面检出数。
    caught: [u32; 4],
}

impl ThreatRegister {
    pub fn new() -> Self {
        Self { defense_up: [false; 4], caught: [0; 0 + 4] }
    }

    fn idx(face: ThreatFace) -> usize {
        match face {
            ThreatFace::FarmChain => 0,
            ThreatFace::PhishAsset => 1,
            ThreatFace::ReviewRing => 2,
            ThreatFace::Launder => 3,
        }
    }

    /// 部署某面防线。
    pub fn deploy(&mut self, face: ThreatFace) {
        self.defense_up[Self::idx(face)] = true;
    }

    /// 全部四面防线就位？
    pub fn all_defended(&self) -> bool {
        self.defense_up.iter().all(|&b| b)
    }

    /// 登记一次检出（防线未就位也计数，暴露盲区）。
    pub fn record(&mut self, face: ThreatFace) {
        let i = Self::idx(face);
        self.caught[i] = self.caught[i].saturating_add(1);
    }

    pub fn caught_of(&self, face: ThreatFace) -> u32 {
        self.caught[Self::idx(face)]
    }

    pub fn total_caught(&self) -> u32 {
        self.caught.iter().sum()
    }
}

impl Default for ThreatRegister {
    fn default() -> Self {
        Self::new()
    }
}

// ── 外链扫描（钓鱼） ──────────────────────────────────────────────

/// 外链特征。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkVerdict {
    /// 清洁。
    Clean,
    /// 可疑（需人工复核）。
    Suspicious,
    /// 钓鱼（P0）。
    Phishing,
}

impl LinkVerdict {
    pub fn is_p0(self) -> bool {
        matches!(self, Self::Phishing)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Clean => "清洁",
            Self::Suspicious => "可疑",
            Self::Phishing => "钓鱼",
        }
    }
}

/// 钓鱼特征规则。
pub struct PhishRule {
    /// 域名字面特征（示意：摘要匹配）。
    pub domain_digest: u64,
    /// 是否 IP 直连特征。
    pub ip_literal: bool,
    /// 是否超长混淆路径。
    pub obfuscated_path: bool,
}

impl PhishRule {
    pub fn match_link(&self, domain_digest: u64, ip_literal: bool, obfuscated_path: bool) -> LinkVerdict {
        if domain_digest == self.domain_digest || (self.ip_literal && ip_literal) {
            return LinkVerdict::Phishing;
        }
        if self.obfuscated_path && obfuscated_path {
            return LinkVerdict::Suspicious;
        }
        LinkVerdict::Clean
    }
}

/// 外链扫描器。
pub struct LinkScanner {
    rules: Vec<PhishRule>,
    /// 扫描过的链接数。
    scanned: u32,
    /// 命中 P0 数。
    p0_hits: u32,
}

impl LinkScanner {
    pub fn new() -> Self {
        Self { rules: Vec::new(), scanned: 0, p0_hits: 0 }
    }

    pub fn add_rule(&mut self, rule: PhishRule) {
        self.rules.push(rule);
    }

    /// 扫描一条外链。命中钓鱼即 P0 拒绝。
    pub fn scan(&mut self, domain_digest: u64, ip_literal: bool, obfuscated_path: bool) -> Result<LinkVerdict, C8FCode> {
        self.scanned = self.scanned.saturating_add(1);
        for rule in &self.rules {
            let v = rule.match_link(domain_digest, ip_literal, obfuscated_path);
            if v.is_p0() {
                self.p0_hits = self.p0_hits.saturating_add(1);
                return Err(C8FCode::PHISH_LINK);
            }
            if v == LinkVerdict::Suspicious {
                return Ok(LinkVerdict::Suspicious);
            }
        }
        Ok(LinkVerdict::Clean)
    }

    pub fn scanned(&self) -> u32 {
        self.scanned
    }

    pub fn p0_hits(&self) -> u32 {
        self.p0_hits
    }
}

impl Default for LinkScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 洗钱检测 ──────────────────────────────────────────────────────

/// 交易样本。
pub struct TxnSample {
    /// 付款方。
    pub from: u64,
    /// 收款方。
    pub to: u64,
    /// 金额（虚拟币最小单位）。
    pub amount: u64,
    /// 时间戳（分钟）。
    pub at_min: u32,
}

impl TxnSample {
    pub fn new(from: u64, to: u64, amount: u64, at_min: u32) -> Self {
        Self { from, to, amount, at_min }
    }
}

/// 交易模式异常检测。
pub struct LaunderDetector {
    /// 归集阈值：同一收款方短窗内笔数。
    pub cluster_window_min: u32,
    /// 归集笔数阈值。
    pub cluster_min_count: u32,
    /// 快进快出阈值（分钟）。
    pub pass_through_max_min: u32,
    /// 检出数。
    hits: u32,
}

impl LaunderDetector {
    pub fn standard() -> Self {
        Self {
            cluster_window_min: 60,
            cluster_min_count: 5,
            pass_through_max_min: 30,
            hits: 0,
        }
    }

    /// 归集模式：窗口内同一收款方笔数超阈值。
    pub fn detect_cluster(&mut self, samples: &[TxnSample], to: u64) -> Result<(), C8FCode> {
        if samples.is_empty() {
            return Ok(());
        }
        let mut min_t = u32::MAX;
        let mut count = 0u32;
        for s in samples {
            if s.to == to {
                if s.at_min < min_t {
                    min_t = s.at_min;
                }
                count = count.saturating_add(1);
            }
        }
        if count >= self.cluster_min_count {
            // 检查是否落在窗口内（近似：最大-最小 ≤ window）
            let mut max_t = 0u32;
            for s in samples {
                if s.to == to && s.at_min > max_t {
                    max_t = s.at_min;
                }
            }
            if max_t.saturating_sub(min_t) <= self.cluster_window_min {
                self.hits = self.hits.saturating_add(1);
                return Err(C8FCode::LAUNDER_PATTERN);
            }
        }
        Ok(())
    }

    /// 快进快出：收款方在极短时间把等额转出。
    pub fn detect_pass_through(&mut self, in_amount: u64, out_amount: u64, gap_min: u32) -> Result<(), C8FCode> {
        if out_amount >= in_amount / 2 && gap_min <= self.pass_through_max_min {
            self.hits = self.hits.saturating_add(1);
            return Err(C8FCode::LAUNDER_PATTERN);
        }
        Ok(())
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }
}

// ── 情报协同 ──────────────────────────────────────────────────────

/// 情报源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntelSource {
    /// 平台安全。
    PlatformSec,
    /// 法务。
    Legal,
    /// 外部生态。
    External,
}

impl IntelSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::PlatformSec => "平台安全",
            Self::Legal => "法务",
            Self::External => "外部生态",
        }
    }
}

/// 情报协同表。
pub struct IntelBridge {
    /// 各源最近同步分钟戳。
    last_sync: Vec<(IntelSource, u32)>,
    /// 断链数（超时未同步）。
    breaks: u32,
    /// 断链判定阈值（分钟）。
    pub stale_threshold_min: u32,
}

impl IntelBridge {
    pub fn new() -> Self {
        Self { last_sync: Vec::new(), breaks: 0, stale_threshold_min: 120 }
    }

    /// 同步心跳。
    pub fn heartbeat(&mut self, src: IntelSource, at_min: u32) {
        for entry in self.last_sync.iter_mut() {
            if entry.0 == src {
                entry.1 = at_min;
                return;
            }
        }
        self.last_sync.push((src, at_min));
    }

    /// 巡检：超时未同步即断链。
    pub fn patrol(&mut self, now_min: u32) -> Result<(), C8FCode> {
        let mut broken = false;
        for &(_, t) in &self.last_sync {
            if now_min.saturating_sub(t) > self.stale_threshold_min {
                broken = true;
            }
        }
        if broken {
            self.breaks = self.breaks.saturating_add(1);
            return Err(C8FCode::INTEL_BROKEN);
        }
        Ok(())
    }

    /// 三源全在线？
    pub fn all_online(&self) -> bool {
        self.last_sync.len() == 3
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }
}

impl Default for IntelBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ── 生态安全治理总线 ──────────────────────────────────────────────

/// 生态安全治理总线。
pub struct EcoSecBus {
    pub register: ThreatRegister,
    pub scanner: LinkScanner,
    pub launder: LaunderDetector,
    pub intel: IntelBridge,
}

impl EcoSecBus {
    pub fn new() -> Self {
        Self {
            register: ThreatRegister::new(),
            scanner: LinkScanner::new(),
            launder: LaunderDetector::standard(),
            intel: IntelBridge::new(),
        }
    }

    /// 部署四面防线全量。
    pub fn deploy_all(&mut self) {
        for face in ThreatFace::all() {
            self.register.deploy(face);
        }
    }
}

impl Default for EcoSecBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves95_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves95_ecosec");
    {
        // 01 四面清单全量且防线可全就位
        let mut reg = ThreatRegister::new();
        for f in ThreatFace::all() {
            reg.deploy(f);
        }
        chk(
            &mut set,
            "C38F-01",
            reg.all_defended() && ThreatFace::all().len() == 4,
            "四面威胁清单须面面有防线",
        );
    }
    {
        // 02 外链钓鱼即 P0
        let mut sc = LinkScanner::new();
        sc.add_rule(PhishRule { domain_digest: 0xBAD, ip_literal: true, obfuscated_path: true });
        let r = sc.scan(0xBAD, false, false);
        chk(
            &mut set,
            "C38F-02",
            r.err() == Some(C8FCode::PHISH_LINK) && sc.p0_hits() == 1,
            "资产带钓鱼链接即 P0（外链扫描红线）",
        );
    }
    {
        // 03 IP 直连特征命中
        let mut sc = LinkScanner::new();
        sc.add_rule(PhishRule { domain_digest: 0x1, ip_literal: true, obfuscated_path: false });
        let r = sc.scan(0x2, true, false);
        chk(&mut set, "C38F-03", r.err() == Some(C8FCode::PHISH_LINK), "IP 直连外链须按钓鱼特征拦截");
    }
    {
        // 04 清洁外链放行
        let mut sc = LinkScanner::new();
        sc.add_rule(PhishRule { domain_digest: 0x1, ip_literal: true, obfuscated_path: true });
        let r = sc.scan(0x2, false, false);
        chk(&mut set, "C38F-04", r == Ok(LinkVerdict::Clean), "无特征外链清洁放行");
    }
    {
        // 05 混淆路径仅可疑不 P0
        let mut sc = LinkScanner::new();
        sc.add_rule(PhishRule { domain_digest: 0x1, ip_literal: false, obfuscated_path: true });
        let r = sc.scan(0x2, false, true);
        chk(
            &mut set,
            "C38F-05",
            r == Ok(LinkVerdict::Suspicious),
            "混淆路径走可疑通道（人工复核），不直接 P0",
        );
    }
    {
        // 06 归集模式 P0
        let mut ld = LaunderDetector::standard();
        let samples = alloc::vec![
            TxnSample::new(1, 9, 10, 0),
            TxnSample::new(2, 9, 10, 5),
            TxnSample::new(3, 9, 10, 10),
            TxnSample::new(4, 9, 10, 15),
            TxnSample::new(5, 9, 10, 20),
        ];
        let r = ld.detect_cluster(&samples, 9);
        chk(
            &mut set,
            "C38F-06",
            r.err() == Some(C8FCode::LAUNDER_PATTERN) && ld.hits() == 1,
            "窗口内小额多笔归集命中洗钱模式 P0",
        );
    }
    {
        // 07 归集不超阈值放行
        let mut ld = LaunderDetector::standard();
        let samples = alloc::vec![
            TxnSample::new(1, 9, 10, 0),
            TxnSample::new(2, 9, 10, 5),
        ];
        let r = ld.detect_cluster(&samples, 9);
        chk(&mut set, "C38F-07", r.is_ok(), "笔数不足阈值不触发");
    }
    {
        // 08 快进快出 P0
        let mut ld = LaunderDetector::standard();
        let r = ld.detect_pass_through(1000, 900, 10);
        chk(
            &mut set,
            "C38F-08",
            r.err() == Some(C8FCode::LAUNDER_PATTERN),
            "等额快进快出（10 分钟）命中洗钱模式 P0",
        );
    }
    {
        // 09 正常周转放行
        let mut ld = LaunderDetector::standard();
        let r = ld.detect_pass_through(1000, 100, 200);
        chk(&mut set, "C38F-09", r.is_ok(), "小额慢周转不触发");
    }
    {
        // 10 情报断链
        let mut ib = IntelBridge::new();
        ib.heartbeat(IntelSource::PlatformSec, 0);
        let r = ib.patrol(500);
        chk(
            &mut set,
            "C38F-10",
            r.err() == Some(C8FCode::INTEL_BROKEN) && ib.breaks() == 1,
            "超时未同步即情报断链",
        );
    }
    {
        // 11 心跳保活
        let mut ib = IntelBridge::new();
        ib.heartbeat(IntelSource::PlatformSec, 0);
        ib.heartbeat(IntelSource::Legal, 0);
        ib.heartbeat(IntelSource::External, 0);
        ib.heartbeat(IntelSource::PlatformSec, 100);
        ib.heartbeat(IntelSource::Legal, 100);
        ib.heartbeat(IntelSource::External, 100);
        let r = ib.patrol(150);
        chk(&mut set, "C38F-11", r.is_ok() && ib.all_online(), "三源心跳保活全在线");
    }
    {
        // 12 总线四面部署
        let mut bus = EcoSecBus::new();
        bus.deploy_all();
        chk(
            &mut set,
            "C38F-12",
            bus.register.all_defended(),
            "总线一键四面防线全就位",
        );
    }
    set
}
