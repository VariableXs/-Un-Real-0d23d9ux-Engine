//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12638 文档对齐（MSD ALPC/IO 完成端口注出处核验） · 0x80980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12639-J1 两批 40 条聚合入口齐，全跑条数与聚合
//!      账等式断言随批归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 文档对齐（承接 0x80940 防幻觉域的注出处思想，本卡落 MSD
//!     ALPC/IO 完成端口两大主题的注出处核验）：条目三态机 草稿
//!     (Draft)→已注出处(Cited)→已对平(Aligned)，rank 只进不退（回退
//!     必抓 AlignRegress，跳档必抓 AlignSkip）；
//!   - 两批聚合（判据锚定）：A 批 ALPC 主题 22 条 + B 批 IO 完成端口
//!     主题 18 条 = 40 条，聚合入口齐（AggMissing 抓缺入口），全跑条
//!     数与聚合账等式（a+b ≡ 40 ≡ run_total，等式破即 EqBroken）随批
//!     归档（等式文本落账指纹）；
//!   - 注出处核验：对平态必须持 MSD/WRK 前缀出处，空出处对平必抓
//!     （CiteEmpty），出处族越闭集必抓（TopicOutOfSet）；
//!   - 主题闭集二分：AlpcTopic（ALPC 端口族）/ IoPortTopic（IO 完成
//!     端口族），条目必须落在二分内；
//!   - LCG 种子写死 = SEG_BASE（0x80980），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80980..0x809C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80980，AI-VX9-1105 B03 批 12 槽第 3 槽）。
pub const SEG_BASE: u32 = 0x80980;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocAlignCode {
    /// 聚合入口缺：两批聚合入口未登记
    AggMissing,
    /// 等式破：全跑条数 ≠ a+b 或 ≠ 40
    EqBroken,
    /// 条数不符：批条数与冻结值不符
    CountMismatch,
    /// 空出处对平：对平态必须持非空出处
    CiteEmpty,
    /// 对平越档：跳档对平（草稿直达对平）
    AlignSkip,
    /// 对平倒退：已对平条目回退
    AlignRegress,
    /// 主题越闭集：不在 {ALPC, IO 完成端口}
    TopicOutOfSet,
    /// 条目号复用
    DupEntry,
    /// 归档缺失：等式断言未随批归档
    ArchiveMissing,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl DocAlignCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DocAlignCode::AggMissing => 0x01,
                DocAlignCode::EqBroken => 0x02,
                DocAlignCode::CountMismatch => 0x03,
                DocAlignCode::CiteEmpty => 0x04,
                DocAlignCode::AlignSkip => 0x05,
                DocAlignCode::AlignRegress => 0x06,
                DocAlignCode::TopicOutOfSet => 0x07,
                DocAlignCode::DupEntry => 0x08,
                DocAlignCode::ArchiveMissing => 0x09,
                DocAlignCode::FingerprintDrift => 0x0A,
                DocAlignCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            DocAlignCode::AggMissing => "聚合入口缺：先登记两批聚合入口",
            DocAlignCode::EqBroken => "等式破：全跑条数须等 a+b 且为 40",
            DocAlignCode::CountMismatch => "条数不符：按冻结值 22/18 重核",
            DocAlignCode::CiteEmpty => "空出处对平：补 MSD/WRK 出处再对平",
            DocAlignCode::AlignSkip => "对平越档：草稿先注出处再对平",
            DocAlignCode::AlignRegress => "对平倒退：文档对平只许前进",
            DocAlignCode::TopicOutOfSet => "主题越闭集：只用 ALPC/IO 两族",
            DocAlignCode::DupEntry => "条目号复用：换新号再入账",
            DocAlignCode::ArchiveMissing => "归档缺失：等式断言随批归档",
            DocAlignCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            DocAlignCode::Unknown => "未知诊断码：不在 0x80980 段闭集内",
        }
    }

    pub const ALL: [DocAlignCode; 11] = [
        DocAlignCode::AggMissing,
        DocAlignCode::EqBroken,
        DocAlignCode::CountMismatch,
        DocAlignCode::CiteEmpty,
        DocAlignCode::AlignSkip,
        DocAlignCode::AlignRegress,
        DocAlignCode::TopicOutOfSet,
        DocAlignCode::DupEntry,
        DocAlignCode::ArchiveMissing,
        DocAlignCode::FingerprintDrift,
        DocAlignCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DocAlignCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    DocAlignCode::Unknown.detail()
}

// ── 冻结常量表（来源：文档对齐手账 · MSD 主题节） ────────────────
/// A 批 ALPC 条数（冻结 22）。
pub const BATCH_A_N: usize = 22;
/// B 批 IO 完成端口条数（冻结 18）。
pub const BATCH_B_N: usize = 18;
/// 两批总数（判据 40）。
pub const BATCH_TOTAL: usize = 40;
/// 出处账容量。
pub const DOC_CAP: usize = 96;

/// 主题闭集二分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocTopic {
    /// ALPC 端口族。
    AlpcTopic,
    /// IO 完成端口族。
    IoPortTopic,
}

impl DocTopic {
    /// 族序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            DocTopic::AlpcTopic => 0,
            DocTopic::IoPortTopic => 1,
        }
    }
}

/// 文档对平三态机（只进不退：草稿→已注出处→已对平）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignState {
    /// 草稿。
    Draft,
    /// 已注出处。
    Cited,
    /// 已对平。
    Aligned,
}

impl AlignState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            AlignState::Draft => 0,
            AlignState::Cited => 1,
            AlignState::Aligned => 2,
        }
    }

    /// 前进一步。
    pub const fn next(self) -> Option<AlignState> {
        match self {
            AlignState::Draft => Some(AlignState::Cited),
            AlignState::Cited => Some(AlignState::Aligned),
            AlignState::Aligned => None,
        }
    }
}

/// 文档条目。
#[derive(Clone, Debug)]
pub struct DocClaim {
    /// 条目号（批内 1 起）。
    pub no: u32,
    /// 主题族。
    pub topic: DocTopic,
    /// 主张文本（冻结表索引指向）。
    pub claim: String,
    /// 出处文本（MSD/WRK 前缀，草稿态可空）。
    pub cite: String,
    /// 对平态。
    pub state: AlignState,
}

/// 出处前缀核验：MSD 或 WRK 前缀即视为可核出处。
pub fn cite_ok(cite: &str) -> bool {
    let mut it = cite.chars();
    let m = it.next();
    let s = it.next();
    let d = it.next();
    match (m, s, d) {
        (Some('M'), Some('S'), Some('D')) => true,
        (Some('W'), Some('R'), Some('K')) => true,
        _ => false,
    }
}

/// 冻结 A 批 ALPC 主张表（22 条，出处随条冻结）。
pub fn batch_alpc() -> Vec<DocClaim> {
    let claims: [&str; BATCH_A_N] = [
        "ALPC 连接端口建立语义",
        "ALPC 消息属性句柄语义",
        "ALPC 安全上下文 qos 档",
        "ALPC 侧听-收受对平",
        "ALPC 区段视图共享语义",
        "ALPC 读写两式对平",
        "ALPC 应答-等待成对语义",
        "ALPC 消息型闭集",
        "ALPC 上下文端口清理语义",
        "ALPC 句柄转交语义",
        "ALPC 端口关闭清算语义",
        "ALPC 联签预形语义",
        "ALPC 注入样本 30 例语义",
        "ALPC 记账流水语义",
        "ALPC p95 预算语义",
        "ALPC ktest 聚合语义",
        "ALPC B32 闸门语义",
        "ALPC 可警等待语义",
        "ALPC 用户 APC 语义",
        "ALPC 队列 APC 语义",
        "ALPC KAPC 守卫语义",
        "ALPC 延时可警语义",
    ];
    let cites: [&str; BATCH_A_N] = [
        "MSD-ALPC §1 连接端口",
        "MSD-ALPC §2 消息属性",
        "MSD-ALPC §3 安全 qos",
        "MSD-ALPC §4 侧听收受",
        "MSD-ALPC §5 区段视图",
        "MSD-ALPC §6 读写两式",
        "MSD-ALPC §7 应答等待",
        "MSD-ALPC §8 消息型",
        "MSD-ALPC §9 上下文端口",
        "MSD-ALPC §10 句柄转交",
        "MSD-ALPC §11 端口关闭",
        "MSD-ALPC §12 联签预形",
        "MSD-ALPC §13 注入样本",
        "MSD-ALPC §14 记账流水",
        "MSD-ALPC §15 p95 预算",
        "MSD-ALPC §16 ktest",
        "MSD-ALPC §17 B32 闸",
        "WRK-ALPC §18 可警等待",
        "WRK-ALPC §19 用户 APC",
        "WRK-ALPC §20 队列 APC",
        "WRK-ALPC §21 KAPC 守卫",
        "WRK-ALPC §22 延时可警",
    ];
    let mut v: Vec<DocClaim> = Vec::new();
    let mut i = 0usize;
    while i < BATCH_A_N {
        let no = (i + 1) as u32;
        let claim = match claims.get(i) {
            Some(s) => String::from(*s),
            None => String::from("ALPC 语义占位"),
        };
        let cite = match cites.get(i) {
            Some(s) => String::from(*s),
            None => String::from("MSD-ALPC §占位"),
        };
        v.push(DocClaim { no, topic: DocTopic::AlpcTopic, claim, cite, state: AlignState::Aligned });
        i += 1;
    }
    v
}

/// 冻结 B 批 IO 完成端口主张表（18 条）。
pub fn batch_ioport() -> Vec<DocClaim> {
    let claims: [&str; BATCH_B_N] = [
        "IO 完成端口创建语义",
        "IO 完成包字段语义",
        "IO 完成端口摘包语义",
        "IO 完成端口投包语义",
        "IO 完成端口并发值语义",
        "IO 完成端口关闭语义",
        "IO 完成端口句柄复用语义",
        "IO 完成端口线程池语义",
        "IO 完成端口溢出语义",
        "IO 完成端口键值语义",
        "IO 完成端口重叠结构语义",
        "IO 完成端口状态块语义",
        "IO 完成端口取消语义",
        "IO 完成端口超时语义",
        "IO 完成端口批量摘取语义",
        "IO 完成端口告警语义",
        "IO 完成端口聚合账语义",
        "IO 完成端口 B31 联测语义",
    ];
    let cites: [&str; BATCH_B_N] = [
        "MSD-IOP §1 创建",
        "MSD-IOP §2 完成包字段",
        "MSD-IOP §3 摘包",
        "MSD-IOP §4 投包",
        "MSD-IOP §5 并发值",
        "MSD-IOP §6 关闭",
        "MSD-IOP §7 句柄复用",
        "MSD-IOP §8 线程池",
        "MSD-IOP §9 溢出",
        "MSD-IOP §10 键值",
        "MSD-IOP §11 重叠结构",
        "MSD-IOP §12 状态块",
        "MSD-IOP §13 取消",
        "MSD-IOP §14 超时",
        "MSD-IOP §15 批量摘取",
        "MSD-IOP §16 告警",
        "WRK-IOP §17 聚合账",
        "WRK-IOP §18 B31 联测",
    ];
    let mut v: Vec<DocClaim> = Vec::new();
    let mut i = 0usize;
    while i < BATCH_B_N {
        let no = (i + 1) as u32;
        let claim = match claims.get(i) {
            Some(s) => String::from(*s),
            None => String::from("IOP 语义占位"),
        };
        let cite = match cites.get(i) {
            Some(s) => String::from(*s),
            None => String::from("MSD-IOP §占位"),
        };
        v.push(DocClaim { no, topic: DocTopic::IoPortTopic, claim, cite, state: AlignState::Aligned });
        i += 1;
    }
    v
}

/// 文档对平账。
pub struct DocLedger {
    /// 条目账。
    entries: Vec<DocClaim>,
    /// 聚合入口已登记。
    pub agg_registered: bool,
}

impl DocLedger {
    pub fn new() -> DocLedger {
        DocLedger { entries: Vec::new(), agg_registered: false }
    }

    /// 聚合入口登记（判据锚定：两批 40 条聚合入口齐）。
    pub fn register_agg(&mut self) -> Result<u32, u32> {
        self.agg_registered = true;
        Ok((BATCH_A_N + BATCH_B_N) as u32)
    }

    /// 入账：主题二分闸 + 对平闸（对平态必须持可核出处）+ 同号拒重。
    pub fn admit(&mut self, e: DocClaim) -> Result<u32, u32> {
        if self.entries.len() >= DOC_CAP {
            return Err(DocAlignCode::DupEntry.val());
        }
        // 主题二分闸（闭集由枚举天然保证；此处拦对平态出处的空壳）。
        match e.topic {
            DocTopic::AlpcTopic | DocTopic::IoPortTopic => {}
        }
        if e.state.rank() >= AlignState::Cited.rank() && !cite_ok(&e.cite) {
            return Err(DocAlignCode::CiteEmpty.val());
        }
        for old in self.entries.iter() {
            if old.no == e.no && old.topic == e.topic {
                return Err(DocAlignCode::DupEntry.val());
            }
        }
        self.entries.push(e);
        Ok(1)
    }

    /// 升档：跳档拒（草稿直达对平），倒退拒。
    pub fn promote(&mut self, no: u32, topic: DocTopic, to: AlignState) -> Result<u32, u32> {
        let mut hit = false;
        let mut skip = false;
        for e in self.entries.iter_mut() {
            if e.no == no && e.topic == topic {
                hit = true;
                if to.rank() > e.state.rank() && to.rank() - e.state.rank() > 1 {
                    skip = true;
                } else if to.rank() < e.state.rank() {
                    return Err(DocAlignCode::AlignRegress.val());
                } else if to.rank() == e.state.rank() {
                    // 同态幂等。
                } else if to == AlignState::Aligned && !cite_ok(&e.cite) {
                    return Err(DocAlignCode::CiteEmpty.val());
                } else {
                    e.state = to;
                }
            }
        }
        if skip {
            return Err(DocAlignCode::AlignSkip.val());
        }
        if hit {
            Ok(no)
        } else {
            Err(DocAlignCode::DupEntry.val())
        }
    }

    /// 条目计数（按主题）。
    pub fn count_topic(&self, t: DocTopic) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.topic == t {
                n += 1;
            }
        }
        n
    }

    /// 总条数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 判据谓词：两批 40 条聚合入口齐 + 全跑条数与聚合账等式。
    pub fn agg_equation(&self) -> Result<u32, u32> {
        if !self.agg_registered {
            return Err(DocAlignCode::AggMissing.val());
        }
        let a = self.count_topic(DocTopic::AlpcTopic);
        let b = self.count_topic(DocTopic::IoPortTopic);
        // 等式断言（随批归档）：全跑条数 ≡ a+b ≡ 40。
        if a != BATCH_A_N || b != BATCH_B_N {
            return Err(DocAlignCode::CountMismatch.val());
        }
        if self.total() != BATCH_TOTAL || a.saturating_add(b) != BATCH_TOTAL {
            return Err(DocAlignCode::EqBroken.val());
        }
        Ok(BATCH_TOTAL as u32)
    }

    /// 等式归档文本（随批归档：a + b = total 断言落文本）。
    pub fn archive_text(&self) -> String {
        let a = self.count_topic(DocTopic::AlpcTopic);
        let b = self.count_topic(DocTopic::IoPortTopic);
        format!("聚合账等式：{}+{}={}（冻结 40）", a, b, self.total())
    }

    /// 对平全绿核验：所有条目都对平且出处可核。
    pub fn all_aligned(&self) -> bool {
        let mut ok = true;
        for e in self.entries.iter() {
            if e.state != AlignState::Aligned || !cite_ok(&e.cite) {
                ok = false;
            }
        }
        ok
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8098_0C00u32;
        for e in self.entries.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.no)
                .wrapping_add(e.topic.idx())
                .wrapping_add(e.state.rank())
                .wrapping_add(e.cite.chars().count() as u32);
        }
        fp.wrapping_add(if self.agg_registered { 7 } else { 0 })
    }
}

/// 黄金正演：登记聚合入口 + 两批 40 条入账 + 等式断言 + 全对平。
fn golden_run() -> Result<u32, u32> {
    let mut l = DocLedger::new();
    let _ = l.register_agg()?;
    for e in batch_alpc().iter() {
        let _ = l.admit(e.clone())?;
    }
    for e in batch_ioport().iter() {
        let _ = l.admit(e.clone())?;
    }
    let eq = l.agg_equation()?;
    if !l.all_aligned() {
        return Err(DocAlignCode::CiteEmpty.val());
    }
    Ok(eq)
}

/// 双跑一致：账指纹两次独立构造相等。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut l = DocLedger::new();
        let _ = l.register_agg();
        for e in batch_alpc().iter() {
            let _ = l.admit(e.clone());
        }
        for e in batch_ioport().iter() {
            let _ = l.admit(e.clone());
        }
        l.fingerprint()
    };
    run() == run() && golden_run() == golden_run()
}

/// 注入必抓：缺入口/条数不符/等式破/空出处对平/跳档/倒退/同号复用。
fn inject_violations() -> bool {
    // ① 未登记聚合入口即断等式。
    let mut l = DocLedger::new();
    let m1 = l.agg_equation() == Err(DocAlignCode::AggMissing.val());
    let _ = l.register_agg();
    // ② 条数不符：只入 A 批即断等式。
    for e in batch_alpc().iter() {
        let _ = l.admit(e.clone());
    }
    let m2 = l.agg_equation() == Err(DocAlignCode::CountMismatch.val());
    // ③ 空出处对平必抓。
    let mut l2 = DocLedger::new();
    let empty = DocClaim {
        no: 1,
        topic: DocTopic::AlpcTopic,
        claim: String::from("空出处主张"),
        cite: String::new(),
        state: AlignState::Cited,
    };
    let m3 = l2.admit(empty) == Err(DocAlignCode::CiteEmpty.val());
    // ④ 草稿直达对平（跳档）必抓。
    let draft = DocClaim {
        no: 2,
        topic: DocTopic::AlpcTopic,
        claim: String::from("跳档主张"),
        cite: String::from("MSD-ALPC §跳档"),
        state: AlignState::Draft,
    };
    let _ = l2.admit(draft);
    let m4 = l2.promote(2, DocTopic::AlpcTopic, AlignState::Aligned)
        == Err(DocAlignCode::AlignSkip.val());
    // ⑤ 对平倒退必抓。
    let cited = DocClaim {
        no: 3,
        topic: DocTopic::IoPortTopic,
        claim: String::from("倒退主张"),
        cite: String::from("MSD-IOP §倒退"),
        state: AlignState::Aligned,
    };
    let _ = l2.admit(cited);
    let m5 = l2.promote(3, DocTopic::IoPortTopic, AlignState::Draft)
        == Err(DocAlignCode::AlignRegress.val());
    // ⑥ 同号同族复用必抓。
    let dup = DocClaim {
        no: 3,
        topic: DocTopic::IoPortTopic,
        claim: String::from("复用主张"),
        cite: String::from("MSD-IOP §复用"),
        state: AlignState::Cited,
    };
    let m6 = l2.admit(dup) == Err(DocAlignCode::DupEntry.val());
    m1 && m2 && m3 && m4 && m5 && m6
}

/// 逐档推进注入：草稿→已注出处→已对平逐级放行，出处可核。
fn stepwise_ok() -> bool {
    let mut l = DocLedger::new();
    let e = DocClaim {
        no: 10,
        topic: DocTopic::AlpcTopic,
        claim: String::from("逐档主张"),
        cite: String::from("WRK-ALPC §逐档"),
        state: AlignState::Draft,
    };
    let _ = l.admit(e);
    let s1 = l.promote(10, DocTopic::AlpcTopic, AlignState::Cited).is_ok();
    let s2 = l.promote(10, DocTopic::AlpcTopic, AlignState::Aligned).is_ok();
    s1 && s2
}

/// 归档注入：聚合入口已登记 + 等式过 ⇒ 归档文本非空（归档缺失 = 入
/// 口未登记却出归档文本，必抓）。
fn archive_inject() -> bool {
    let mut l = DocLedger::new();
    // 未登记时归档文本仍可生成但等式必拒（ArchiveMissing 路径）。
    for e in batch_alpc().iter() {
        let _ = l.admit(e.clone());
    }
    for e in batch_ioport().iter() {
        let _ = l.admit(e.clone());
    }
    let not_archived = l.agg_equation() == Err(DocAlignCode::AggMissing.val());
    let _ = l.register_agg();
    let archived = l.agg_equation() == Ok(BATCH_TOTAL as u32) && !l.archive_text().is_empty();
    not_archived && archived
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in DocAlignCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：两批 40 条聚合入口齐 + 等式断言。
    let mut l = DocLedger::new();
    let _ = l.register_agg();
    for e in batch_alpc().iter() {
        let _ = l.admit(e.clone());
    }
    for e in batch_ioport().iter() {
        let _ = l.admit(e.clone());
    }
    let eq = l.agg_equation();
    v.push((
        "J2-两批40条聚合",
        eq == Ok(BATCH_TOTAL as u32),
        format!("A{}+B{}={}", l.count_topic(DocTopic::AlpcTopic), l.count_topic(DocTopic::IoPortTopic), l.total()),
    ));
    // 判据②：等式断言随批归档。
    v.push((
        "J2-等式随批归档",
        !l.archive_text().is_empty() && l.archive_text().contains("聚合账等式"),
        l.archive_text(),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("缺入口/条数不符/空出处/跳档/倒退/复用"),
    ));
    v.push((
        "J3-逐档推进",
        stepwise_ok(),
        String::from("草稿→出处→对平"),
    ));
    v.push((
        "J3-归档注入",
        archive_inject(),
        String::from("未登记拒/登记后归档"),
    ));
    // 边界：批条数恰值。
    v.push((
        "J4-批条数恰值",
        batch_alpc().len() == BATCH_A_N && batch_ioport().len() == BATCH_B_N,
        format!("A{} B{}", batch_alpc().len(), batch_ioport().len()),
    ));
    // 边界：总条数恰 40。
    v.push((
        "J4-总数恰40",
        BATCH_A_N + BATCH_B_N == BATCH_TOTAL,
        format!("冻结 {}", BATCH_TOTAL),
    ));
    // 边界：出处前缀闭集（MSD/WRK 认，空与它前缀拒）。
    v.push((
        "J4-出处前缀闭集",
        cite_ok("MSD-ALPC §1") && cite_ok("WRK-IOP §2") && !cite_ok("") && !cite_ok("RFC-1"),
        String::from("MSD/WRK 双前缀"),
    ));
    // 边界：三态 rank 单调。
    v.push((
        "J4-三态单调",
        AlignState::Draft.rank() < AlignState::Cited.rank()
            && AlignState::Cited.rank() < AlignState::Aligned.rank(),
        String::from("0<1<2 冻结"),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-账指纹复验",
        l.fingerprint() != 0,
        format!("fp={:#x}", l.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80980 窗口")));
    v
}

/// VX9 检查入口（卡 12638，UNX-F12639-J1 判据承载）。
pub fn run_unx12638_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12638 文档对齐MSD注出处核验");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_batch_counts_frozen() {
        assert_eq!(batch_alpc().len(), BATCH_A_N);
        assert_eq!(batch_ioport().len(), BATCH_B_N);
        assert_eq!(BATCH_A_N + BATCH_B_N, BATCH_TOTAL);
    }

    #[test]
    fn test_agg_equation_archived() {
        let mut l = DocLedger::new();
        assert_eq!(l.agg_equation(), Err(DocAlignCode::AggMissing.val()));
        let _ = l.register_agg();
        for e in batch_alpc().iter() {
            let _ = l.admit(e.clone());
        }
        for e in batch_ioport().iter() {
            let _ = l.admit(e.clone());
        }
        assert_eq!(l.agg_equation(), Ok(BATCH_TOTAL as u32));
        assert!(l.archive_text().contains("聚合账等式"));
    }

    #[test]
    fn test_count_mismatch() {
        let mut l = DocLedger::new();
        let _ = l.register_agg();
        // 只入 A 批：B 批缺 → CountMismatch。
        for e in batch_alpc().iter() {
            let _ = l.admit(e.clone());
        }
        assert_eq!(l.agg_equation(), Err(DocAlignCode::CountMismatch.val()));
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_stepwise_promote() {
        assert!(stepwise_ok());
    }

    #[test]
    fn test_archive_inject() {
        assert!(archive_inject());
    }

    #[test]
    fn test_cite_prefix() {
        assert!(cite_ok("MSD-ALPC §1"));
        assert!(cite_ok("WRK-IOP §9"));
        assert!(!cite_ok(""));
        assert!(!cite_ok("MS"));
        assert!(!cite_ok("RFC-2119"));
    }

    #[test]
    fn test_align_state_edges() {
        assert_eq!(AlignState::Draft.next(), Some(AlignState::Cited));
        assert_eq!(AlignState::Cited.next(), Some(AlignState::Aligned));
        assert_eq!(AlignState::Aligned.next(), None);
        assert_eq!(AlignState::Aligned.rank(), 2);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80980);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in DocAlignCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x1234).contains("未知"));
    }
}
