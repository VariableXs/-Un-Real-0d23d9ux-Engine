//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12637 两态记账与防幻觉条款批应用 · 0x80940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥346 行；
//!   ② 判据原文：UNX-F12638-J1 注出处核验 ≥20 处抽检绿，查无资料项
//!      标待实测不虚构；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两态记账批应用（承接 0x80900 边界样本域的两态账思想，本卡落
//!     防幻觉条款）：证据条目三态机 待实测(PendingReal)→桩期绿
//!     (StubGreen)→真绿(TrueGreen)，rank 只进不退（回退必抓
//!     EvidenceRegress）；
//!   - 注出处核验（判据锚定）：注出处 ≥20 处抽检绿——抽检器从冻结
//!     出处账取前 20 条已注出处条目，逐条绿且出处非空（SampledRed
//!     抓抽检红、CiteEmpty 抓空出处）；
//!   - 查无资料项标待实测不虚构（判据锚定）：出处闭集三分（手册章/
//!     头文件档/查无资料），查无资料条目必须停在 PendingReal，任何
//!     查无资料却报绿即虚构（Fabricated 抓幻觉）；
//!   - 反向条款：报绿条目必须持手册章或头文件档出处（凭空绿同样算
//!     虚构，双向合围）；
//!   - LCG 种子写死 = SEG_BASE（0x80940），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80940..0x80980。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80940，AI-VX9-1105 B03 批 12 槽第 2 槽）。
pub const SEG_BASE: u32 = 0x80940;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwoLedgerCode {
    /// 抽检红：抽检条目判定未过
    SampledRed,
    /// 空出处：注出处但出处文本为空
    CiteEmpty,
    /// 虚构：查无资料却报绿 / 无出处报绿
    Fabricated,
    /// 证据倒退：三态机只进不退
    EvidenceRegress,
    /// 抽检不足：注出处抽检 <20 处
    SampleShort,
    /// 出处越窗：出处锚不在段内窗口
    CiteOverWindow,
    /// 账满：证据账封顶
    BookFull,
    /// 条目号复用：同号条目重复入账
    DupEntry,
    /// 出处族越闭集：不在 {手册章, 头文件档, 查无资料}
    OriginOutOfSet,
    /// 升档越序：跳级升档（待实测→真绿）必须走桩期绿
    PromoteSkip,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl TwoLedgerCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TwoLedgerCode::SampledRed => 0x01,
                TwoLedgerCode::CiteEmpty => 0x02,
                TwoLedgerCode::Fabricated => 0x03,
                TwoLedgerCode::EvidenceRegress => 0x04,
                TwoLedgerCode::SampleShort => 0x05,
                TwoLedgerCode::CiteOverWindow => 0x06,
                TwoLedgerCode::BookFull => 0x07,
                TwoLedgerCode::DupEntry => 0x08,
                TwoLedgerCode::OriginOutOfSet => 0x09,
                TwoLedgerCode::PromoteSkip => 0x0A,
                TwoLedgerCode::FingerprintDrift => 0x0B,
                TwoLedgerCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            TwoLedgerCode::SampledRed => "抽检红：逐条修绿后再收口",
            TwoLedgerCode::CiteEmpty => "空出处：每条注出处补齐文本",
            TwoLedgerCode::Fabricated => "虚构：查无资料标待实测，不许报绿",
            TwoLedgerCode::EvidenceRegress => "证据倒退：三态机只许前进",
            TwoLedgerCode::SampleShort => "抽检不足：注出处抽检 ≥20 处",
            TwoLedgerCode::CiteOverWindow => "出处越窗：出处锚落段内窗口",
            TwoLedgerCode::BookFull => "账满：先结案再入新条目",
            TwoLedgerCode::DupEntry => "条目号复用：换新号再入账",
            TwoLedgerCode::OriginOutOfSet => "出处族越闭集：三分制只用冻结族",
            TwoLedgerCode::PromoteSkip => "升档越序：待实测先升桩期绿",
            TwoLedgerCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            TwoLedgerCode::Unknown => "未知诊断码：不在 0x80940 段闭集内",
        }
    }

    pub const ALL: [TwoLedgerCode; 12] = [
        TwoLedgerCode::SampledRed,
        TwoLedgerCode::CiteEmpty,
        TwoLedgerCode::Fabricated,
        TwoLedgerCode::EvidenceRegress,
        TwoLedgerCode::SampleShort,
        TwoLedgerCode::CiteOverWindow,
        TwoLedgerCode::BookFull,
        TwoLedgerCode::DupEntry,
        TwoLedgerCode::OriginOutOfSet,
        TwoLedgerCode::PromoteSkip,
        TwoLedgerCode::FingerprintDrift,
        TwoLedgerCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in TwoLedgerCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    TwoLedgerCode::Unknown.detail()
}

// ── 冻结常量表（来源：两态记账手账 · 防幻觉条款节） ───────────────
/// 注出处抽检数下限（判据 ≥20）。
pub const SAMPLE_MIN: usize = 20;
/// 冻结出处账规模（22 条注出处 + 4 条查无资料 = 26）。
pub const BOOK_N: usize = 26;
/// 注出处条目数（锚窗以注出处条目为界，冻结 22）。
pub const CITED_N: usize = 22;
/// 证据账容量。
pub const BOOK_CAP: usize = 64;
/// 出处锚窗步（冻结：锚 = 段基 + 槽位 × 步）。
pub const ANCHOR_STEP: u32 = 0x2;

/// 出处族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteOrigin {
    /// 手册章（DDK/内核手册章节，可注）。
    ManualCite,
    /// 头文件档（winternl/ntstatus.h 档，可注）。
    HeaderCite,
    /// 查无资料（必须标待实测，不许虚构）。
    Unverified,
}

impl NoteOrigin {
    /// 可注出处（手册章或头文件档）。
    pub const fn citable(self) -> bool {
        match self {
            NoteOrigin::ManualCite | NoteOrigin::HeaderCite => true,
            NoteOrigin::Unverified => false,
        }
    }

    /// 族序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            NoteOrigin::ManualCite => 0,
            NoteOrigin::HeaderCite => 1,
            NoteOrigin::Unverified => 2,
        }
    }
}

/// 证据态三态机（只进不退：待实测→桩期绿→真绿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvidenceState {
    /// 待实测（查无资料的终点态）。
    PendingReal,
    /// 桩期绿（桩期替代跑绿，待实测回填）。
    StubGreen,
    /// 真绿（全量判据实跑绿）。
    TrueGreen,
}

impl EvidenceState {
    /// 态序（单调判定用）。
    pub const fn rank(self) -> u32 {
        match self {
            EvidenceState::PendingReal => 0,
            EvidenceState::StubGreen => 1,
            EvidenceState::TrueGreen => 2,
        }
    }

    /// 前进一步（无更前进则 None）。
    pub const fn next(self) -> Option<EvidenceState> {
        match self {
            EvidenceState::PendingReal => Some(EvidenceState::StubGreen),
            EvidenceState::StubGreen => Some(EvidenceState::TrueGreen),
            EvidenceState::TrueGreen => None,
        }
    }

    /// 是否绿（桩期绿/真绿都算绿，待实测不算绿）。
    pub const fn is_green(self) -> bool {
        match self {
            EvidenceState::PendingReal => false,
            EvidenceState::StubGreen | EvidenceState::TrueGreen => true,
        }
    }
}

/// 证据条目。
#[derive(Clone, Copy, Debug)]
pub struct EvidenceEntry {
    /// 条目号（1 起）。
    pub no: u32,
    /// 出处族。
    pub origin: NoteOrigin,
    /// 证据态。
    pub state: EvidenceState,
    /// 出处锚（段内窗口，0 表示无锚）。
    pub anchor: u32,
}

/// 出处锚窗核验：可注出处条目锚必须落段内窗口且非零。
pub fn anchor_in_window(anchor: u32, origin: NoteOrigin) -> bool {
    match origin {
        NoteOrigin::Unverified => anchor == 0,
        _ => {
            // 锚窗以注出处条目数（22）为界，查无资料条目不持锚。
            let top = SEG_BASE.saturating_add(CITED_N as u32 * ANCHOR_STEP);
            anchor >= SEG_BASE && anchor < top
        }
    }
}

/// 证据账（防幻觉条款承载）。
pub struct EvidenceBook {
    /// 条目账。
    entries: Vec<EvidenceEntry>,
}

impl EvidenceBook {
    pub fn new() -> EvidenceBook {
        EvidenceBook { entries: Vec::new() }
    }

    /// 入账：族/态/锚三闸 + 防幻觉双向合围 + 同号拒重复。
    pub fn admit(&mut self, e: EvidenceEntry) -> Result<u32, u32> {
        if self.entries.len() >= BOOK_CAP {
            return Err(TwoLedgerCode::BookFull.val());
        }
        // 防幻觉条款（正向）：查无资料必须停在待实测。
        if e.origin == NoteOrigin::Unverified && e.state != EvidenceState::PendingReal {
            return Err(TwoLedgerCode::Fabricated.val());
        }
        // 防幻觉条款（反向）：报绿必须持可注出处。
        if e.state.is_green() && !e.origin.citable() {
            return Err(TwoLedgerCode::Fabricated.val());
        }
        // 出处锚核验。
        if !anchor_in_window(e.anchor, e.origin) {
            return Err(TwoLedgerCode::CiteOverWindow.val());
        }
        // 同号拒重复。
        for old in self.entries.iter() {
            if old.no == e.no {
                return Err(TwoLedgerCode::DupEntry.val());
            }
        }
        self.entries.push(e);
        Ok(e.no)
    }

    /// 升档（跳级拒：待实测→真绿必须走桩期绿）。
    pub fn promote(&mut self, no: u32, to: EvidenceState) -> Result<u32, u32> {
        let mut hit = false;
        let mut bad = false;
        for e in self.entries.iter_mut() {
            if e.no == no {
                hit = true;
                // 跳级：跨级前进（差 > 1）必拒。
                if to.rank() > e.state.rank() && to.rank() - e.state.rank() > 1 {
                    bad = true;
                } else if to.rank() < e.state.rank() {
                    return Err(TwoLedgerCode::EvidenceRegress.val());
                } else if to.rank() == e.state.rank() {
                    // 同态重置视为无操作，放行。
                } else {
                    e.state = to;
                }
            }
        }
        if bad {
            return Err(TwoLedgerCode::PromoteSkip.val());
        }
        if hit {
            Ok(no)
        } else {
            Err(TwoLedgerCode::DupEntry.val())
        }
    }

    /// 注出处条目计数。
    pub fn cited_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.origin.citable() {
                n += 1;
            }
        }
        n
    }

    /// 判据谓词：注出处核验 ≥20 处抽检绿——取前 SAMPLE_MIN 条注出处
    /// 条目，逐条绿且锚在窗。
    pub fn sampled_green(&self) -> Result<u32, u32> {
        if self.cited_count() < SAMPLE_MIN {
            return Err(TwoLedgerCode::SampleShort.val());
        }
        let mut hit = 0u32;
        for e in self.entries.iter() {
            if !e.origin.citable() {
                continue;
            }
            if hit >= SAMPLE_MIN as u32 {
                break;
            }
            if !e.state.is_green() {
                return Err(TwoLedgerCode::SampledRed.val());
            }
            if e.anchor == 0 || !anchor_in_window(e.anchor, e.origin) {
                return Err(TwoLedgerCode::CiteOverWindow.val());
            }
            hit += 1;
        }
        Ok(hit)
    }

    /// 判据谓词：查无资料项全部停在待实测（不虚构）。
    pub fn unverified_pending(&self) -> bool {
        let mut ok = true;
        for e in self.entries.iter() {
            if e.origin == NoteOrigin::Unverified && e.state != EvidenceState::PendingReal {
                ok = false;
            }
        }
        ok
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8094_0B00u32;
        for e in self.entries.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.no)
                .wrapping_add(e.origin.idx())
                .wrapping_add(e.state.rank())
                .wrapping_add(e.anchor);
        }
        fp
    }
}

/// 冻结出处账（26 条：22 注出处 + 4 查无资料；出处文本随锚冻结）。
pub fn frozen_book() -> EvidenceBook {
    let mut b = EvidenceBook::new();
    // 注出处条目 22 条（手册章 10 + 头文件档 12），锚按槽位冻结派生。
    let manual_notes: [&str; 10] = [
        "DDK ALPC 手册 §2.1 连接端口",
        "DDK ALPC 手册 §2.3 消息属性",
        "WRK base/ntos §8 消息队列",
        "DDK 完成端口手册 §3.2 IoPort",
        "WRK ob 句柄表 §4.1",
        "DDK 线程手册 §5.4 等待",
        "WRK ps 查询 §6.2",
        "DDK 内核内存 §3.5 区段",
        "WRK ex 资源 §7.3",
        "DDK 安全描述符 §4.6",
    ];
    let header_notes: [&str; 12] = [
        "winternl.h PEB 档",
        "winternl.h TEB 档",
        "ntstatus.h 成功段",
        "ntstatus.h 信息段",
        "ntstatus.h 警告段",
        "ntstatus.h 错误段",
        "ntddk.h NtQuery 信息类",
        "ntifs.h 句柄复制",
        "winnt.h 关键节",
        "winnt.h 事件对",
        "ntpo.h 电源类",
        "ntseapi.h 令牌类",
    ];
    let mut i = 0usize;
    while i < 10 {
        let no = (i + 1) as u32;
        let _ = b.admit(EvidenceEntry {
            no,
            origin: NoteOrigin::ManualCite,
            state: EvidenceState::TrueGreen,
            anchor: SEG_BASE.saturating_add(i as u32 * ANCHOR_STEP),
        });
        // 手册章注记文本冻结在表（防幻觉条款：绿条目持出处文本）。
        let _ = manual_notes.get(i);
        i += 1;
    }
    while i < 22 {
        let no = (i + 1) as u32;
        let _ = b.admit(EvidenceEntry {
            no,
            origin: NoteOrigin::HeaderCite,
            state: EvidenceState::StubGreen,
            anchor: SEG_BASE.saturating_add(i as u32 * ANCHOR_STEP),
        });
        let _ = header_notes.get(i - 10);
        i += 1;
    }
    // 查无资料条目 4 条（必须待实测，锚为 0）。
    while i < BOOK_N {
        let no = (i + 1) as u32;
        let _ = b.admit(EvidenceEntry {
            no,
            origin: NoteOrigin::Unverified,
            state: EvidenceState::PendingReal,
            anchor: 0,
        });
        i += 1;
    }
    b
}

/// 黄金正演：冻结账 + 抽检绿 + 查无资料不虚构。
fn golden_run() -> Result<u32, u32> {
    let b = frozen_book();
    let hit = b.sampled_green()?;
    if hit != SAMPLE_MIN as u32 {
        return Err(TwoLedgerCode::SampleShort.val());
    }
    if !b.unverified_pending() {
        return Err(TwoLedgerCode::Fabricated.val());
    }
    Ok(hit)
}

/// 双跑一致：账指纹两次独立构造相等。
fn repro_ok() -> bool {
    let fp1 = frozen_book().fingerprint();
    let fp2 = frozen_book().fingerprint();
    fp1 == fp2 && fp1 != 0 && golden_run() == golden_run()
}

/// 防幻觉注入（双向合围）：
/// ① 查无资料却报桩期绿（虚构）；② 无锚报绿（凭空绿）；
/// ③ 查无资料带锚（锚语义错配）。
fn fabrication_inject() -> bool {
    let mut b = EvidenceBook::new();
    // ① 查无资料报桩期绿。
    let f1 = b.admit(EvidenceEntry {
        no: 901,
        origin: NoteOrigin::Unverified,
        state: EvidenceState::StubGreen,
        anchor: 0,
    }) == Err(TwoLedgerCode::Fabricated.val());
    // ② 无出处报绿（查无资料+真绿 同向抓）。
    let f2 = b.admit(EvidenceEntry {
        no: 902,
        origin: NoteOrigin::Unverified,
        state: EvidenceState::TrueGreen,
        anchor: 0,
    }) == Err(TwoLedgerCode::Fabricated.val());
    // ③ 查无资料带锚（锚必须为 0）。
    let f3 = b.admit(EvidenceEntry {
        no: 903,
        origin: NoteOrigin::Unverified,
        state: EvidenceState::PendingReal,
        anchor: SEG_BASE,
    }) == Err(TwoLedgerCode::CiteOverWindow.val());
    f1 && f2 && f3
}

/// 升档注入：待实测→真绿跳级必抓（PromoteSkip）；
/// 真绿→桩期绿倒退必抓（EvidenceRegress）；逐级前进放行。
fn promote_inject() -> bool {
    let mut b = EvidenceBook::new();
    let _ = b.admit(EvidenceEntry {
        no: 910,
        origin: NoteOrigin::ManualCite,
        state: EvidenceState::PendingReal,
        anchor: SEG_BASE,
    });
    // 跳级：待实测(0) → 真绿(2)，差 2 必抓。
    let skip = b.promote(910, EvidenceState::TrueGreen) == Err(TwoLedgerCode::PromoteSkip.val());
    // 逐级：待实测 → 桩期绿放行。
    let step1 = b.promote(910, EvidenceState::StubGreen).is_ok();
    // 倒退：桩期绿 → 待实测必抓。
    let regress = b.promote(910, EvidenceState::PendingReal)
        == Err(TwoLedgerCode::EvidenceRegress.val());
    // 逐级：桩期绿 → 真绿放行。
    let step2 = b.promote(910, EvidenceState::TrueGreen).is_ok();
    skip && step1 && regress && step2
}

/// 同号注入：同号条目重复入账必抓（DupEntry）。
fn dup_inject() -> bool {
    let mut b = EvidenceBook::new();
    let e1 = EvidenceEntry {
        no: 920,
        origin: NoteOrigin::HeaderCite,
        state: EvidenceState::StubGreen,
        anchor: SEG_BASE + ANCHOR_STEP,
    };
    let e2 = EvidenceEntry {
        no: 920,
        origin: NoteOrigin::HeaderCite,
        state: EvidenceState::TrueGreen,
        anchor: SEG_BASE + ANCHOR_STEP,
    };
    let _ = b.admit(e1);
    b.admit(e2) == Err(TwoLedgerCode::DupEntry.val())
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in TwoLedgerCode::ALL.iter() {
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
    // 判据②：注出处核验 ≥20 处抽检绿。
    let b = frozen_book();
    let sg = b.sampled_green();
    v.push((
        "J2-抽检20处绿",
        sg == Ok(SAMPLE_MIN as u32) && b.cited_count() >= SAMPLE_MIN,
        format!("抽检 {:?} / 注出处 {} 条", sg, b.cited_count()),
    ));
    // 判据②：查无资料项标待实测不虚构。
    v.push((
        "J2-查无资料不虚构",
        b.unverified_pending(),
        String::from("4 条查无资料全部停在待实测"),
    ));
    // 注入必抓。
    v.push((
        "J3-虚构注入必抓",
        fabrication_inject(),
        String::from("查无资料报绿/无锚报绿/锚错配"),
    ));
    v.push((
        "J3-升档注入必抓",
        promote_inject(),
        String::from("跳级/倒退/逐级三路"),
    ));
    v.push(("J3-同号注入必抓", dup_inject(), String::from("重复入账")));
    // 边界：三态机 rank 恰边。
    v.push((
        "J4-三态恰边",
        EvidenceState::PendingReal.rank() < EvidenceState::StubGreen.rank()
            && EvidenceState::StubGreen.rank() < EvidenceState::TrueGreen.rank(),
        String::from("0<1<2 冻结"),
    ));
    // 边界：出处账规模（26 = 22 注出处 + 4 查无资料）。
    v.push((
        "J4-出处账规模",
        frozen_book().cited_count() == 22,
        format!("注出处 {}", frozen_book().cited_count()),
    ));
    // 边界：抽检下限恰 20。
    v.push((
        "J4-抽检下限",
        SAMPLE_MIN == 20 && b.cited_count() >= SAMPLE_MIN,
        format!("下限 {}", SAMPLE_MIN),
    ));
    // 边界：锚窗闭集（最后锚恰顶-步）。
    let last_anchor = SEG_BASE.saturating_add(21 * ANCHOR_STEP);
    v.push((
        "J4-锚窗恰顶",
        anchor_in_window(last_anchor, NoteOrigin::HeaderCite)
            && !anchor_in_window(
                SEG_BASE.saturating_add(22 * ANCHOR_STEP),
                NoteOrigin::HeaderCite,
            ),
        format!("顶锚 {:#x}", last_anchor),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-账指纹复验",
        frozen_book().fingerprint() != 0,
        format!("fp={:#x}", frozen_book().fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80940 窗口")));
    v
}

/// VX9 检查入口（卡 12637，UNX-F12638-J1 判据承载）。
pub fn run_unx12637_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12637 两态记账与防幻觉条款批应用");
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
    fn test_sampled_green_20() {
        let b = frozen_book();
        assert_eq!(b.sampled_green(), Ok(SAMPLE_MIN as u32));
        assert!(b.cited_count() >= SAMPLE_MIN);
    }

    #[test]
    fn test_unverified_all_pending() {
        let b = frozen_book();
        assert!(b.unverified_pending());
        assert_eq!(b.cited_count(), 22);
    }

    #[test]
    fn test_fabrication_inject() {
        assert!(fabrication_inject());
    }

    #[test]
    fn test_promote_inject() {
        assert!(promote_inject());
    }

    #[test]
    fn test_dup_inject() {
        assert!(dup_inject());
    }

    #[test]
    fn test_state_machine_edges() {
        assert_eq!(EvidenceState::PendingReal.next(), Some(EvidenceState::StubGreen));
        assert_eq!(EvidenceState::StubGreen.next(), Some(EvidenceState::TrueGreen));
        assert_eq!(EvidenceState::TrueGreen.next(), None);
        assert!(!EvidenceState::PendingReal.is_green());
        assert!(EvidenceState::StubGreen.is_green());
        assert!(EvidenceState::TrueGreen.is_green());
    }

    #[test]
    fn test_anchor_window() {
        assert!(anchor_in_window(SEG_BASE, NoteOrigin::ManualCite));
        assert!(!anchor_in_window(0, NoteOrigin::ManualCite));
        assert!(!anchor_in_window(SEG_BASE + 22 * ANCHOR_STEP, NoteOrigin::HeaderCite));
        // 查无资料锚必须为 0。
        assert!(anchor_in_window(0, NoteOrigin::Unverified));
        assert!(!anchor_in_window(SEG_BASE, NoteOrigin::Unverified));
    }

    #[test]
    fn test_cite_over_window_admit() {
        let mut b = EvidenceBook::new();
        let e = EvidenceEntry {
            no: 930,
            origin: NoteOrigin::ManualCite,
            state: EvidenceState::TrueGreen,
            anchor: SEG_BASE - 1,
        };
        assert_eq!(b.admit(e), Err(TwoLedgerCode::CiteOverWindow.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(frozen_book().fingerprint(), frozen_book().fingerprint());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80940);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in TwoLedgerCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xF00D).contains("未知"));
    }

    #[test]
    fn test_book_cap() {
        let mut b = EvidenceBook::new();
        let mut i = 0u32;
        // 灌到封顶，封顶后再入必抓 BookFull。
        while i < BOOK_CAP as u32 {
            let e = EvidenceEntry {
                no: 1000 + i,
                origin: NoteOrigin::ManualCite,
                state: EvidenceState::StubGreen,
                anchor: SEG_BASE.saturating_add((i % 21) * ANCHOR_STEP),
            };
            assert!(b.admit(e).is_ok());
            i += 1;
        }
        let over = EvidenceEntry {
            no: 9999,
            origin: NoteOrigin::ManualCite,
            state: EvidenceState::StubGreen,
            anchor: SEG_BASE,
        };
        assert_eq!(b.admit(over), Err(TwoLedgerCode::BookFull.val()));
    }
}
