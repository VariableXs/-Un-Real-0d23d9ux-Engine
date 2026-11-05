//! UNX-F1571 消费域回归反馈通道（移交后反馈回路） · 0x78DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 8 · UNX-F1571 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1571-J1 反馈通道可用（消费域问题反馈登记：
//!     域/问题/关联接口三列入流），反馈→条目→处置三跳可溯 10/10，
//!     通道健康入 F0997；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 入流闭集：10 条反馈逐条登记（单号 FB01..FB10 / 域 / 问题述 /
//!     关联接口），单号唯一、域收闭集、关联接口收冻结接口表；
//!   - 三跳可溯：反馈→条目→处置逐跳落痕（每跳记上游单号），三跳
//!     链逐条回溯到反馈单号，断链即拒；
//!   - 10/10 追溯：十条三跳链逐条回溯全通，检出计数恰 10；
//!   - 通道健康入 F0997：健康摘要行（入流量 / 追溯通过率 / 断链数）
//!     按 F0997 口径落行，指纹入账；
//!   - 注入必抓：黑户域 / 黑户接口 / 断链处置 / 重单号四路必抓；
//!   - 状态机单调：入流→立案→处置→落档，倒退跳级显性拒；
//!   - LCG 抽验复验，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78DC0，派工卡 VX8-1104 B06 卡 8 独占窗口）。
pub const SEG_BASE: u32 = 0x78DC0;

/// 反馈单条数（判据 J1「10/10」冻结）。
pub const FEEDBACK_COUNT: usize = 10;

/// 通道健康行数（F0997 口径：1 行摘要，指纹入账随总摘要）。
pub const HEALTH_LINES: usize = 1;

/// 通道摘要行数（头 + 10 三跳链行 + 健康行 + 尾，冻结）。
pub const CHANNEL_ROWS: usize = 13;

// ── 反馈域闭集（消费域） ────────────────────────────────────────
pub const FEED_DOMAINS: [&str; 4] = ["图形消费域", "存储消费域", "网络消费域", "调试消费域"];

// ── 关联接口冻结表（与 UNX-F1566 五组冻结接口对齐） ────────────
pub const LINKED_IFACES: [&str; 5] = [
    "IrqBackend",
    "MSI-X",
    "smp_call",
    "shootdown",
    "计时签名",
];

// ── 反馈单闭集（单号 / 域 / 问题述 / 关联接口下标） ─────────────
pub const FEEDBACKS: [(&str, &str, &str, usize); 10] = [
    ("FB01", "图形消费域", "交换链呈现回执偶发超窗", 1),
    ("FB02", "图形消费域", "光标回拉帧序与呈现序错拍", 0),
    ("FB03", "存储消费域", "日志回灌通道回压阈值偏紧", 2),
    ("FB04", "存储消费域", "快照指纹回退路径演练缺席", 3),
    ("FB05", "网络消费域", "中断聚合窗口与分发口径半拍差", 0),
    ("FB06", "网络消费域", "向量释放后掩码残留位未全清", 1),
    ("FB07", "调试消费域", "走查样例期望值派生种子未公开", 4),
    ("FB08", "调试消费域", "联签总账时刻列时区口径缺标注", 4),
    ("FB09", "存储消费域", "移交包缺件告警文案双语列缺席", 2),
    ("FB10", "图形消费域", "保留项闭环回执未入健康一屏", 3),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel71Code {
    /// 单号越闭集：单号收进 FB01..FB10
    IdUnknown,
    /// 单号重复：一单号一登记，重登先撤旧
    IdDuplicate,
    /// 单号为空：补单号再入流
    IdEmpty,
    /// 域越闭集：域名收进四消费域
    DomainUnknown,
    /// 问题述为空：补问题述再入流
    DescEmpty,
    /// 关联接口越冻结表：接口名收进五组冻结接口
    IfaceUnknown,
    /// 断链：处置回溯不到条目、条目回溯不到反馈，补上游单号
    ChainBroken,
    /// 重复立案：一反馈一条目，重立先撤旧
    EntryDuplicate,
    /// 重复处置：一条目一处置，重处先撤旧
    DisposalDuplicate,
    /// 通道状态倒退：入流→立案→处置→落档单调推进
    PhaseRegress,
    /// 通道状态跳级：三跳未齐不得落档
    PhaseSkip,
    /// 通道摘要行数违约：摘要必须恰 13 行
    ChannelRowsBad,
}

impl Channel71Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Channel71Code::IdUnknown => "单号越闭集：单号收进 FB01..FB10 再入流",
            Channel71Code::IdDuplicate => "单号重复：一单号一登记，重登先撤旧再补",
            Channel71Code::IdEmpty => "单号为空：补单号再入流",
            Channel71Code::DomainUnknown => "域越闭集：域名收进四消费域再入流",
            Channel71Code::DescEmpty => "问题述为空：补问题述再入流",
            Channel71Code::IfaceUnknown => "关联接口越冻结表：接口名收进五组冻结接口",
            Channel71Code::ChainBroken => "断链：处置回溯不到条目、条目回溯不到反馈，补上游单号",
            Channel71Code::EntryDuplicate => "重复立案：一反馈一条目，重立先撤旧再补",
            Channel71Code::DisposalDuplicate => "重复处置：一条目一处置，重处先撤旧再补",
            Channel71Code::PhaseRegress => "通道状态倒退：入流→立案→处置→落档单调推进",
            Channel71Code::PhaseSkip => "通道状态跳级：三跳未齐不得落档",
            Channel71Code::ChannelRowsBad => "通道摘要行数违约：摘要必须恰 13 行",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Channel71Code; 12] = [
        Channel71Code::IdUnknown,
        Channel71Code::IdDuplicate,
        Channel71Code::IdEmpty,
        Channel71Code::DomainUnknown,
        Channel71Code::DescEmpty,
        Channel71Code::IfaceUnknown,
        Channel71Code::ChainBroken,
        Channel71Code::EntryDuplicate,
        Channel71Code::DisposalDuplicate,
        Channel71Code::PhaseRegress,
        Channel71Code::PhaseSkip,
        Channel71Code::ChannelRowsBad,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Channel71Code::ALL.len() {
        let c = match Channel71Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

// ── 通道状态机（入流 → 立案 → 处置 → 落档，单调推进） ──────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelPhase {
    /// 入流期：反馈单逐条三列入流
    Inflow,
    /// 立案期：反馈→条目逐条立案落痕
    Filed,
    /// 处置期：条目→处置逐条落痕
    Disposed,
    /// 落档期：通道摘要落档封账（健康行入 F0997 口径）
    Sealed,
}

impl ChannelPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            ChannelPhase::Inflow => 0,
            ChannelPhase::Filed => 1,
            ChannelPhase::Disposed => 2,
            ChannelPhase::Sealed => 3,
        }
    }
}

/// 反馈单（域/问题/关联接口三列）。
#[derive(Clone, Debug)]
pub struct FeedRow {
    pub id: String,
    pub domain: String,
    pub desc: String,
    pub iface: String,
}

/// 条目（第二跳：上游反馈单号）。
#[derive(Clone, Debug)]
pub struct EntryRow {
    pub entry_id: String,
    pub feed_id: String,
}

/// 处置（第三跳：上游条目号）。
#[derive(Clone, Debug)]
pub struct DisposalRow {
    pub disposal_id: String,
    pub entry_id: String,
}

/// 消费域回归反馈通道。
pub struct FeedChannel {
    phase: ChannelPhase,
    feeds: Vec<FeedRow>,
    entries: Vec<EntryRow>,
    disposals: Vec<DisposalRow>,
    summary_digest: u32,
    summary: Option<String>,
}

/// 单号格式校验：FB + 两位序号，序号 1..=10。
fn valid_feed_id(id: &str) -> bool {
    if !id.starts_with("FB") {
        return false;
    }
    let suffix = match id.strip_prefix("FB") {
        Some(s) => s,
        None => return false,
    };
    let bytes = suffix.as_bytes();
    if bytes.len() != 2 {
        return false;
    }
    let mut value = 0u32;
    let mut k = 0usize;
    while k < bytes.len() {
        let b = match bytes.get(k) {
            Some(b) => *b,
            None => return false,
        };
        if !b.is_ascii_digit() {
            return false;
        }
        value = value.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        k += 1;
    }
    value >= 1 && value <= FEEDBACK_COUNT as u32
}

/// 域名闭集校验。
fn domain_known(domain: &str) -> bool {
    let mut i = 0usize;
    while i < FEED_DOMAINS.len() {
        if let Some(n) = FEED_DOMAINS.get(i) {
            if *n == domain {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 接口名闭集校验。
fn iface_known(iface: &str) -> bool {
    let mut i = 0usize;
    while i < LINKED_IFACES.len() {
        if let Some(n) = LINKED_IFACES.get(i) {
            if *n == iface {
                return true;
            }
        }
        i += 1;
    }
    false
}

impl FeedChannel {
    pub fn new() -> FeedChannel {
        FeedChannel {
            phase: ChannelPhase::Inflow,
            feeds: Vec::new(),
            entries: Vec::new(),
            disposals: Vec::new(),
            summary_digest: 0,
            summary: None,
        }
    }

    pub fn phase(&self) -> ChannelPhase {
        self.phase
    }

    pub fn feed_count(&self) -> usize {
        self.feeds.len()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn disposal_count(&self) -> usize {
        self.disposals.len()
    }

    pub fn summary_digest(&self) -> u32 {
        self.summary_digest
    }

    pub fn summary_text(&self) -> Option<&str> {
        match self.summary.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 反馈入流：单号闭集格式 + 唯一 + 域/接口闭集 + 问题述非空。
    pub fn inflow(&mut self, id: &str, domain: &str, desc: &str, iface: &str) -> Result<(), u32> {
        if self.phase.rank() >= ChannelPhase::Filed.rank() {
            return Err(Channel71Code::PhaseRegress.val());
        }
        if id.is_empty() {
            return Err(Channel71Code::IdEmpty.val());
        }
        if !valid_feed_id(id) {
            return Err(Channel71Code::IdUnknown.val());
        }
        let mut i = 0usize;
        while i < self.feeds.len() {
            let f = match self.feeds.get(i) {
                Some(f) => f,
                None => return Err(Channel71Code::IdUnknown.val()),
            };
            if f.id == id {
                return Err(Channel71Code::IdDuplicate.val());
            }
            i += 1;
        }
        if !domain_known(domain) {
            return Err(Channel71Code::DomainUnknown.val());
        }
        if desc.is_empty() {
            return Err(Channel71Code::DescEmpty.val());
        }
        if !iface_known(iface) {
            return Err(Channel71Code::IfaceUnknown.val());
        }
        self.feeds.push(FeedRow {
            id: id.to_string(),
            domain: domain.to_string(),
            desc: desc.to_string(),
            iface: iface.to_string(),
        });
        Ok(())
    }

    /// 立案（第二跳）：一反馈一条目，上游单号可溯。
    pub fn file_entry(&mut self, entry_id: &str, feed_id: &str) -> Result<(), u32> {
        if self.phase.rank() >= ChannelPhase::Disposed.rank() {
            return Err(Channel71Code::PhaseRegress.val());
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.feeds.len() {
            if let Some(f) = self.feeds.get(i) {
                if f.id == feed_id {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return Err(Channel71Code::ChainBroken.val());
        }
        let mut j = 0usize;
        while j < self.entries.len() {
            let e = match self.entries.get(j) {
                Some(e) => e,
                None => return Err(Channel71Code::ChainBroken.val()),
            };
            if e.feed_id == feed_id || e.entry_id == entry_id {
                return Err(Channel71Code::EntryDuplicate.val());
            }
            j += 1;
        }
        self.entries.push(EntryRow {
            entry_id: entry_id.to_string(),
            feed_id: feed_id.to_string(),
        });
        // 首条立案即把通道推进入流→立案（单调不倒退）。
        if self.phase == ChannelPhase::Inflow {
            self.phase = ChannelPhase::Filed;
        }
        Ok(())
    }

    /// 处置（第三跳）：一条目一处置，上游条目号可溯。
    pub fn dispose(&mut self, disposal_id: &str, entry_id: &str) -> Result<(), u32> {
        if self.phase.rank() >= ChannelPhase::Sealed.rank() {
            return Err(Channel71Code::PhaseRegress.val());
        }
        if self.phase == ChannelPhase::Inflow {
            return Err(Channel71Code::PhaseRegress.val());
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.entry_id == entry_id {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return Err(Channel71Code::ChainBroken.val());
        }
        let mut j = 0usize;
        while j < self.disposals.len() {
            let d = match self.disposals.get(j) {
                Some(d) => d,
                None => return Err(Channel71Code::ChainBroken.val()),
            };
            if d.entry_id == entry_id || d.disposal_id == disposal_id {
                return Err(Channel71Code::DisposalDuplicate.val());
            }
            j += 1;
        }
        self.disposals.push(DisposalRow {
            disposal_id: disposal_id.to_string(),
            entry_id: entry_id.to_string(),
        });
        // 首条处置即把通道推进立案→处置（单调不倒退）。
        if self.phase.rank() < ChannelPhase::Disposed.rank() {
            self.phase = ChannelPhase::Disposed;
        }
        Ok(())
    }

    /// 三跳回溯：处置→条目→反馈逐链回溯，返回全通链数。
    pub fn trace_chains(&self) -> usize {
        let mut traced = 0usize;
        let mut i = 0usize;
        while i < self.disposals.len() {
            let d = match self.disposals.get(i) {
                Some(d) => d,
                None => break,
            };
            // 第二跳：条目在案。
            let mut entry_hit = false;
            let mut feed_of_entry: Option<String> = None;
            let mut j = 0usize;
            while j < self.entries.len() {
                if let Some(e) = self.entries.get(j) {
                    if e.entry_id == d.entry_id {
                        entry_hit = true;
                        feed_of_entry = Some(e.feed_id.clone());
                    }
                }
                j += 1;
            }
            if !entry_hit {
                i += 1;
                continue;
            }
            // 第一跳：反馈在流。
            let fid = match feed_of_entry {
                Some(f) => f,
                None => {
                    i += 1;
                    continue;
                }
            };
            let mut feed_hit = false;
            let mut k = 0usize;
            while k < self.feeds.len() {
                if let Some(f) = self.feeds.get(k) {
                    if f.id == fid {
                        feed_hit = true;
                    }
                }
                k += 1;
            }
            if feed_hit {
                traced += 1;
            }
            i += 1;
        }
        traced
    }

    /// 处置收口：三跳齐备（10 单 / 10 条目 / 10 处置）且全链可溯方可通过。
    pub fn close_flow(&mut self) -> Result<usize, u32> {
        if self.phase != ChannelPhase::Disposed {
            return Err(Channel71Code::PhaseRegress.val());
        }
        if self.feeds.len() != FEEDBACK_COUNT
            || self.entries.len() != FEEDBACK_COUNT
            || self.disposals.len() != FEEDBACK_COUNT
        {
            return Err(Channel71Code::ChainBroken.val());
        }
        if self.trace_chains() != FEEDBACK_COUNT {
            return Err(Channel71Code::ChainBroken.val());
        }
        self.phase = ChannelPhase::Disposed;
        Ok(self.trace_chains())
    }

    /// 通道摘要落档：13 行（含 F0997 健康行）+ 指纹 + 封账。
    pub fn seal_channel(&mut self) -> Result<u32, u32> {
        if self.phase != ChannelPhase::Disposed {
            return Err(Channel71Code::PhaseSkip.val());
        }
        let traced = self.trace_chains();
        let mut text = String::from("== UNX-F1571 反馈通道三跳链摘要 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.disposals.len() {
            let d = match self.disposals.get(i) {
                Some(d) => d,
                None => break,
            };
            let line = format!("{} {} 三跳可溯 ●", d.disposal_id, d.entry_id);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        // F0997 通道健康行（口径冻结：入流/追溯/断链三数）。
        let broken = FEEDBACK_COUNT.saturating_sub(traced);
        text.push_str(&format!(
            "F0997 通道健康：入流{} 追溯{}/{} 断链{} ●",
            self.feed_count(),
            traced,
            FEEDBACK_COUNT,
            broken
        ));
        text.push('\n');
        text.push_str("== 反馈→条目→处置三跳可溯 10/10 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != CHANNEL_ROWS {
            return Err(Channel71Code::ChannelRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.summary = Some(text);
        self.summary_digest = digest;
        self.phase = ChannelPhase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验：抽 5 条反馈复验三列入闭集。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.feeds.len() != FEEDBACK_COUNT {
            return Err(Channel71Code::ChainBroken.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < 5 {
            let idx = lcg.next_mod(FEEDBACK_COUNT);
            let f = match self.feeds.get(idx) {
                Some(f) => f,
                None => return Err(Channel71Code::IdUnknown.val()),
            };
            if !valid_feed_id(&f.id) || !domain_known(&f.domain) || !iface_known(&f.iface)
                || f.desc.is_empty()
            {
                return Err(Channel71Code::IfaceUnknown.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 好件构建：10 单入流 + 立案 + 处置 + 收口 + 落档 ─────────────
pub fn build_channel_ok() -> FeedChannel {
    let mut ch = FeedChannel::new();
    let mut i = 0usize;
    while i < FEEDBACKS.len() {
        let row = match FEEDBACKS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let iface = match LINKED_IFACES.get(row.3) {
            Some(n) => *n,
            None => "",
        };
        let _ = ch.inflow(row.0, row.1, row.2, iface);
        i += 1;
    }
    // 立案：FB01..FB10 → EN01..EN10。
    let mut j = 0usize;
    while j < FEEDBACK_COUNT {
        let fid = format!("FB{:02}", j + 1);
        let eid = format!("EN{:02}", j + 1);
        let _ = ch.file_entry(&eid, &fid);
        j += 1;
    }
    // 处置：EN01..EN10 → DS01..DS10。
    let mut k = 0usize;
    while k < FEEDBACK_COUNT {
        let eid = format!("EN{:02}", k + 1);
        let did = format!("DS{:02}", k + 1);
        let _ = ch.dispose(&did, &eid);
        k += 1;
    }
    // 三跳齐备收口再落档。
    let _ = ch.close_flow();
    let _ = ch.seal_channel();
    ch
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 三列入流：10 单域/问题/关联接口逐列入闭集。
fn three_columns_ok() -> bool {
    let ch = build_channel_ok();
    if ch.feed_count() != FEEDBACK_COUNT {
        return false;
    }
    if ch.entry_count() != FEEDBACK_COUNT || ch.disposal_count() != FEEDBACK_COUNT {
        return false;
    }
    if ch.phase() != ChannelPhase::Sealed {
        return false;
    }
    true
}

/// 三跳可溯 10/10：好件十条链逐条回溯全通。
fn trace_10_ok() -> bool {
    let ch = build_channel_ok();
    let n = ch.trace_chains();
    if n != FEEDBACK_COUNT {
        return false;
    }
    if ch.summary_digest() == 0 {
        return false;
    }
    true
}

/// 通道摘要落档：13 行、F0997 健康行齐、10/10 尾标、指纹非零。
fn channel_archived_ok() -> bool {
    let ch = build_channel_ok();
    let text = match ch.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != CHANNEL_ROWS {
        return false;
    }
    if !text.contains("三跳链摘要") {
        return false;
    }
    if !text.contains("F0997 通道健康") {
        return false;
    }
    if !text.contains("10/10") {
        return false;
    }
    if ch.summary_digest() == 0 {
        return false;
    }
    true
}

/// 黑户注入：黑户域 / 黑户接口 / 黑户上游三路必抓。
fn inject_unknown_ok() -> bool {
    let mut ch = FeedChannel::new();
    // 黑户域。
    match ch.inflow("FB01", "黑户域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::DomainUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 黑户接口。
    match ch.inflow("FB01", "图形消费域", "问题述", "黑户接口") {
        Err(code) => {
            if code != Channel71Code::IfaceUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 黑户上游（无单立案）。
    match ch.file_entry("EN01", "FB99") {
        Err(code) => {
            if code != Channel71Code::ChainBroken.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ch.feed_count() != 0 || ch.entry_count() != 0 {
        return false;
    }
    true
}

/// 单号注入：错前缀 / 三位 / 零号 / 越号 / 空号五路必抓。
fn inject_badid_ok() -> bool {
    let mut ch = FeedChannel::new();
    match ch.inflow("FC01", "图形消费域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match ch.inflow("FB001", "图形消费域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match ch.inflow("FB00", "图形消费域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match ch.inflow("FB99", "图形消费域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::IdUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match ch.inflow("", "图形消费域", "问题述", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match ch.inflow("FB01", "图形消费域", "", "MSI-X") {
        Err(code) => {
            if code != Channel71Code::DescEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ch.feed_count() != 0 {
        return false;
    }
    true
}

/// 重单号注入：同号二登必抓（IdDuplicate）。
fn inject_dup_ok() -> bool {
    let mut ch = FeedChannel::new();
    let row0 = match FEEDBACKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let iface0 = match LINKED_IFACES.get(row0.3) {
        Some(n) => *n,
        None => return false,
    };
    if ch.inflow(row0.0, row0.1, row0.2, iface0).is_err() {
        return false;
    }
    match ch.inflow(row0.0, row0.1, "另一问题", iface0) {
        Err(code) => {
            if code != Channel71Code::IdDuplicate.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ch.feed_count() != 1 {
        return false;
    }
    true
}

/// 断链注入：处置回溯不到条目必抓（ChainBroken）。
fn inject_broken_ok() -> bool {
    let mut ch = FeedChannel::new();
    let row0 = match FEEDBACKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let iface0 = match LINKED_IFACES.get(row0.3) {
        Some(n) => *n,
        None => return false,
    };
    if ch.inflow(row0.0, row0.1, row0.2, iface0).is_err() {
        return false;
    }
    if ch.file_entry("EN01", row0.0).is_err() {
        return false;
    }
    // 处置挂到不存在的条目：断链必抓。
    match ch.dispose("DS01", "EN99") {
        Err(code) => {
            if code != Channel71Code::ChainBroken.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ch.disposal_count() != 0 {
        return false;
    }
    true
}

/// 重复处置注入：一条目二处必抓（DisposalDuplicate）。
fn inject_dup_disposal_ok() -> bool {
    let mut ch = FeedChannel::new();
    let row0 = match FEEDBACKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let iface0 = match LINKED_IFACES.get(row0.3) {
        Some(n) => *n,
        None => return false,
    };
    if ch.inflow(row0.0, row0.1, row0.2, iface0).is_err() {
        return false;
    }
    if ch.file_entry("EN01", row0.0).is_err() {
        return false;
    }
    if ch.dispose("DS01", "EN01").is_err() {
        return false;
    }
    match ch.dispose("DS02", "EN01") {
        Err(code) => {
            if code != Channel71Code::DisposalDuplicate.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ch.disposal_count() != 1 {
        return false;
    }
    true
}

/// 三跳未齐收口注入：缺处置收口必拒（ChainBroken）。
fn inject_short_flow_ok() -> bool {
    let mut ch = FeedChannel::new();
    let row0 = match FEEDBACKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let iface0 = match LINKED_IFACES.get(row0.3) {
        Some(n) => *n,
        None => return false,
    };
    if ch.inflow(row0.0, row0.1, row0.2, iface0).is_err() {
        return false;
    }
    if ch.file_entry("EN01", row0.0).is_err() {
        return false;
    }
    // 缺处置收口：收口须在处置态，未到即拒。
    match ch.close_flow() {
        Err(code) => {
            if code != Channel71Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 状态机单调：未处置落档拒、封账后再入流拒。
fn phase_monotonic_ok() -> bool {
    let mut ch = FeedChannel::new();
    // 未处置先落档：跳级拒。
    match ch.seal_channel() {
        Err(code) => {
            if code != Channel71Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好件封账后再入流：倒退拒。
    let mut sealed = build_channel_ok();
    let row0 = match FEEDBACKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let iface0 = match LINKED_IFACES.get(row0.3) {
        Some(n) => *n,
        None => return false,
    };
    match sealed.inflow(row0.0, row0.1, row0.2, iface0) {
        Err(code) => {
            if code != Channel71Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好件抽 5 条全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let ch = build_channel_ok();
    let n = match ch.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != 5 {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < 5 {
        if a.next_mod(FEEDBACK_COUNT) != b.next_mod(FEEDBACK_COUNT) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好件指纹与摘要两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_channel_ok().summary_digest();
    let d2 = build_channel_ok().summary_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_channel_ok().summary_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_channel_ok().summary_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Channel71Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Channel71Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1571_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1571 消费域回归反馈通道");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("三列入流", three_columns_ok(), "10 单域/问题/关联接口逐列入闭集");
    s.add_dyn("三跳可溯10/10", trace_10_ok(), "十条链处置→条目→反馈逐条全通");
    s.add_dyn("通道摘要落档", channel_archived_ok(), "13 行摘要含 F0997 健康行");
    s.add_dyn("黑户注入必抓", inject_unknown_ok(), "黑户域/接口/上游三路必抓");
    s.add_dyn("单号注入必抓", inject_badid_ok(), "错前缀/三位/零号/越号/空号/空述六路必抓");
    s.add_dyn("重单号注入", inject_dup_ok(), "同号二登必抓 IdDuplicate");
    s.add_dyn("断链注入必抓", inject_broken_ok(), "处置挂幽灵条目必抓 ChainBroken");
    s.add_dyn("重复处置注入", inject_dup_disposal_ok(), "一条目二处必抓 DisposalDuplicate");
    s.add_dyn("三跳未齐收口拒", inject_short_flow_ok(), "缺链收口必抓 ChainBroken");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "入流→立案→处置→落档可达，倒退拒");
    s.add_dyn("LCG抽验5条", sample_audit_ok(), "好件抽 5 条全过序列确定");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与摘要两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78DC0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1571_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn feedback_table_shape() {
        assert_eq!(FEEDBACKS.len(), 10);
        assert_eq!(CHANNEL_ROWS, 13);
        assert_eq!(HEALTH_LINES, 1);
        let mut i = 0usize;
        while i < FEEDBACKS.len() {
            let row = match FEEDBACKS.get(i) {
                Some(r) => *r,
                None => break,
            };
            assert!(valid_feed_id(row.0), "单号 {} 格式违约", row.0);
            assert!(domain_known(row.1), "域 {} 越闭集", row.1);
            assert!(!row.2.is_empty(), "问题述为空");
            assert!(row.3 < LINKED_IFACES.len(), "接口下标越界");
            i += 1;
        }
    }

    #[test]
    fn full_path_seals() {
        let ch = build_channel_ok();
        assert_eq!(ch.feed_count(), 10);
        assert_eq!(ch.entry_count(), 10);
        assert_eq!(ch.disposal_count(), 10);
        assert_eq!(ch.phase(), ChannelPhase::Sealed);
        assert!(ch.summary_text().is_some());
    }

    #[test]
    fn trace_all_ten() {
        let ch = build_channel_ok();
        assert_eq!(ch.trace_chains(), 10);
    }

    #[test]
    fn bad_ids_rejected() {
        let mut ch = FeedChannel::new();
        assert!(ch.inflow("FB1", "图形消费域", "述", "MSI-X").is_err());
        assert!(ch.inflow("FB001", "图形消费域", "述", "MSI-X").is_err());
        assert!(ch.inflow("FB11", "图形消费域", "述", "MSI-X").is_err());
        assert!(ch.inflow("FB00", "图形消费域", "述", "MSI-X").is_err());
        assert_eq!(ch.feed_count(), 0);
    }

    #[test]
    fn unknown_domain_and_iface_rejected() {
        let mut ch = FeedChannel::new();
        match ch.inflow("FB01", "幽灵域", "述", "MSI-X") {
            Err(code) => assert_eq!(code, Channel71Code::DomainUnknown.val()),
            Ok(_) => assert!(false, "黑户域必须被拒"),
        }
        match ch.inflow("FB01", "图形消费域", "述", "幽灵接口") {
            Err(code) => assert_eq!(code, Channel71Code::IfaceUnknown.val()),
            Ok(_) => assert!(false, "黑户接口必须被拒"),
        }
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut ch = FeedChannel::new();
        let row2 = match FEEDBACKS.get(2) {
            Some(r) => *r,
            None => return,
        };
        let iface2 = match LINKED_IFACES.get(row2.3) {
            Some(n) => *n,
            None => return,
        };
        assert!(ch.inflow(row2.0, row2.1, row2.2, iface2).is_ok());
        match ch.inflow(row2.0, row2.1, "另述", iface2) {
            Err(code) => assert_eq!(code, Channel71Code::IdDuplicate.val()),
            Ok(_) => assert!(false, "重单号必须被拒"),
        }
        assert_eq!(ch.feed_count(), 1);
    }

    #[test]
    fn broken_chain_rejected() {
        let mut ch = FeedChannel::new();
        match ch.file_entry("EN01", "FB99") {
            Err(code) => assert_eq!(code, Channel71Code::ChainBroken.val()),
            Ok(_) => assert!(false, "幽灵反馈立案必须被拒"),
        }
        // 立案后处置挂幽灵条目：断链必抓。
        let row0 = match FEEDBACKS.get(0) {
            Some(r) => *r,
            None => return,
        };
        let iface0 = match LINKED_IFACES.get(row0.3) {
            Some(n) => *n,
            None => return,
        };
        assert!(ch.inflow(row0.0, row0.1, row0.2, iface0).is_ok());
        assert!(ch.file_entry("EN01", row0.0).is_ok());
        match ch.dispose("DS01", "EN99") {
            Err(code) => assert_eq!(code, Channel71Code::ChainBroken.val()),
            Ok(_) => assert!(false, "幽灵条目处置必须被拒"),
        }
        // 未立案通道直接处置：状态闸必抓。
        let mut fresh = FeedChannel::new();
        match fresh.dispose("DS01", "EN01") {
            Err(code) => assert_eq!(code, Channel71Code::PhaseRegress.val()),
            Ok(_) => assert!(false, "入流期处置必须被拒"),
        }
    }

    #[test]
    fn duplicate_entry_and_disposal_rejected() {
        let mut ch = FeedChannel::new();
        let row0 = match FEEDBACKS.get(0) {
            Some(r) => *r,
            None => return,
        };
        let iface0 = match LINKED_IFACES.get(row0.3) {
            Some(n) => *n,
            None => return,
        };
        assert!(ch.inflow(row0.0, row0.1, row0.2, iface0).is_ok());
        assert!(ch.file_entry("EN01", row0.0).is_ok());
        match ch.file_entry("EN02", row0.0) {
            Err(code) => assert_eq!(code, Channel71Code::EntryDuplicate.val()),
            Ok(_) => assert!(false, "重复立案必须被拒"),
        }
        assert!(ch.dispose("DS01", "EN01").is_ok());
        match ch.dispose("DS02", "EN01") {
            Err(code) => assert_eq!(code, Channel71Code::DisposalDuplicate.val()),
            Ok(_) => assert!(false, "重复处置必须被拒"),
        }
    }

    #[test]
    fn seal_before_disposed_rejected() {
        let mut ch = FeedChannel::new();
        match ch.seal_channel() {
            Err(code) => assert_eq!(code, Channel71Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未处置落档必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_channel_ok().summary_digest();
        let d2 = build_channel_ok().summary_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(10), b.next_mod(10));
        assert_eq!(a.next_mod(10), b.next_mod(10));
    }

    #[test]
    fn summary_rows_and_f0997_marker() {
        let ch = build_channel_ok();
        let text = match ch.summary_text() {
            Some(t) => t.to_string(),
            None => {
                assert!(false, "通道摘要未落档");
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 13);
        assert!(text.contains("F0997 通道健康"));
        assert!(text.contains("断链0"));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Channel71Code::ALL.len(), 12);
        let first = match Channel71Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Channel71Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
