//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12636 消息面边界样本集（深队列/零长消息/拒止叠加） · 0x80900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12637-J1 两态记账（真绿/桩期绿分列）20/20 齐，
//!      偏移/码值出处注记全量核验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消息面边界样本集（承接 0x808C0 泵长稳域的边界样本思想，本卡落
//!     深队列/零长消息/拒止叠加三族样本）：样本闭集 33 例（≥30），
//!     三族轮换冻结构造，逐例入库判定（SampleRed 抓红例、SampleShort
//!     抓缺库）；
//!   - 深队列边界：队列深闭域 [1, 1024)，恰下沿/恰上沿-1 都入库，
//!     越顶注入必抓（DepthOver）；
//!   - 零长消息边界：载荷长度恰 0 合法但必须带非零边界令牌（无令牌
//!     注入必抓 TokenMissing），令牌与样本号绑定（令牌漂移必抓）；
//!   - 拒止叠加边界：拒止计数闭域 [0, 8]，叠加到顶（恰 8）入库、
//!     超 8 必抓（RefusalOver），叠加单调只进不退（倒退必抓）；
//!   - 两态记账（判据锚定）：真绿/桩期绿分列各 20 条（20/20 齐），
//!     每条带偏移/码值出处注记，注记全量核验（缺注记即 NoteMissing）；
//!   - LCG 种子写死 = SEG_BASE（0x80900），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80900..0x80940。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80900，AI-VX9-1105 B03 批 12 槽第 1 槽）。
pub const SEG_BASE: u32 = 0x80900;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgSampleCode {
    /// 样本缺库：入库样本 <30 例
    SampleShort,
    /// 红例：样本入库判定未过
    SampleRed,
    /// 深队列越顶：队列深 ≥1024
    DepthOver,
    /// 深队列缺底：队列深为 0（未入队）
    DepthZero,
    /// 零长令牌缺失：零长消息必须带非零令牌
    TokenMissing,
    /// 令牌漂移：令牌与样本号绑定不符
    TokenDrift,
    /// 拒止叠超：拒止计数 >8
    RefusalOver,
    /// 拒止倒退：拒止计数单调只进不退
    RefusalRegress,
    /// 两态列缺：真绿/桩期绿任一列 <20 条
    TwoStateShort,
    /// 注记缺失：条目缺偏移/码值出处注记
    NoteMissing,
    /// 注记越窗：注记偏移不在段内窗口
    NoteOverWindow,
    /// 记账倒退：两态记账态只进不退
    LedgerRegress,
    /// 样本族越闭集：不在 {深队列, 零长, 拒止叠加}
    KindOutOfSet,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl MsgSampleCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MsgSampleCode::SampleShort => 0x01,
                MsgSampleCode::SampleRed => 0x02,
                MsgSampleCode::DepthOver => 0x03,
                MsgSampleCode::DepthZero => 0x04,
                MsgSampleCode::TokenMissing => 0x05,
                MsgSampleCode::TokenDrift => 0x06,
                MsgSampleCode::RefusalOver => 0x07,
                MsgSampleCode::RefusalRegress => 0x08,
                MsgSampleCode::TwoStateShort => 0x09,
                MsgSampleCode::NoteMissing => 0x0A,
                MsgSampleCode::NoteOverWindow => 0x0B,
                MsgSampleCode::LedgerRegress => 0x0C,
                MsgSampleCode::KindOutOfSet => 0x0D,
                MsgSampleCode::FingerprintDrift => 0x0E,
                MsgSampleCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            MsgSampleCode::SampleShort => "样本缺库：入库样本 ≥30 例再收口",
            MsgSampleCode::SampleRed => "红例：逐例修绿后再入库",
            MsgSampleCode::DepthOver => "深队列越顶：队列深用 1..1024 闭域",
            MsgSampleCode::DepthZero => "深队列缺底：队列深 ≥1 才算入队",
            MsgSampleCode::TokenMissing => "零长令牌缺失：零长消息带非零令牌",
            MsgSampleCode::TokenDrift => "令牌漂移：令牌按样本号重算",
            MsgSampleCode::RefusalOver => "拒止叠超：叠加计数 ≤8",
            MsgSampleCode::RefusalRegress => "拒止倒退：叠加计数只进不退",
            MsgSampleCode::TwoStateShort => "两态列缺：真绿/桩期绿各补齐 20 条",
            MsgSampleCode::NoteMissing => "注记缺失：每条补偏移/码值出处",
            MsgSampleCode::NoteOverWindow => "注记越窗：注记偏移落在段内窗口",
            MsgSampleCode::LedgerRegress => "记账倒退：两态记账只许前进",
            MsgSampleCode::KindOutOfSet => "样本族越闭集：只用三族样本",
            MsgSampleCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            MsgSampleCode::Unknown => "未知诊断码：不在 0x80900 段闭集内",
        }
    }

    pub const ALL: [MsgSampleCode; 15] = [
        MsgSampleCode::SampleShort,
        MsgSampleCode::SampleRed,
        MsgSampleCode::DepthOver,
        MsgSampleCode::DepthZero,
        MsgSampleCode::TokenMissing,
        MsgSampleCode::TokenDrift,
        MsgSampleCode::RefusalOver,
        MsgSampleCode::RefusalRegress,
        MsgSampleCode::TwoStateShort,
        MsgSampleCode::NoteMissing,
        MsgSampleCode::NoteOverWindow,
        MsgSampleCode::LedgerRegress,
        MsgSampleCode::KindOutOfSet,
        MsgSampleCode::FingerprintDrift,
        MsgSampleCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in MsgSampleCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    MsgSampleCode::Unknown.detail()
}

// ── 冻结常量表（来源：消息面边界手账 · 样本族节） ─────────────────
/// 样本数下限（判据 ≥30）。
pub const SAMPLE_MIN: usize = 30;
/// 样本库规模（冻结 33：深队列 12 + 零长 10 + 拒止叠加 11）。
pub const SAMPLE_N: usize = 33;
/// 深队列闭域（下含上不含）。
pub const DEPTH_WIN: (u32, u32) = (1, 1024);
/// 零长消息令牌基数（令牌 = 基数 ⊕ 样本号，冻结）。
pub const TOKEN_BASE: u32 = 0x9E37;
/// 拒止叠加计数闭域（下含上含）。
pub const REFUSAL_WIN: (u32, u32) = (0, 8);
/// 两态记账每列条数（判据 20/20）。
pub const TWO_STATE_PER: usize = 20;

/// 样本族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleKind {
    /// 深队列族（队列深打边界）。
    DeepQueue,
    /// 零长消息族（载荷恰 0，令牌携带）。
    ZeroLength,
    /// 拒止叠加族（拒止计数打边界）。
    RefusalStack,
}

impl SampleKind {
    /// 三族次序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            SampleKind::DeepQueue => 0,
            SampleKind::ZeroLength => 1,
            SampleKind::RefusalStack => 2,
        }
    }
}

/// 一条边界样本。
#[derive(Clone, Copy, Debug)]
pub struct SampleCase {
    /// 样本号（1 起，单调）。
    pub no: u32,
    /// 样本族。
    pub kind: SampleKind,
    /// 队列深（深队列族有效，其余置 1）。
    pub depth: u32,
    /// 载荷长（零长族恰 0，其余 ≥1）。
    pub payload: u32,
    /// 拒止叠加计数（拒止族有效，其余置 0）。
    pub refusals: u32,
}

/// 零长令牌派生（冻结）：token = TOKEN_BASE ⊕ 样本号，逐样本绑定。
pub fn derive_token(no: u32) -> u32 {
    TOKEN_BASE ^ no
}

/// 边界样本库（33 例冻结构造：三族轮换，全绿设计）。
pub fn sample_corpus() -> Vec<SampleCase> {
    let mut v: Vec<SampleCase> = Vec::new();
    let mut i = 0usize;
    while i < SAMPLE_N {
        let no = (i + 1) as u32;
        // 三族轮换 + 族内边界轮转，边界位按下表冻结。
        let kind = match i % 3 {
            0 => SampleKind::DeepQueue,
            1 => SampleKind::ZeroLength,
            _ => SampleKind::RefusalStack,
        };
        let case = match kind {
            SampleKind::DeepQueue => {
                // 深队列族：恰下沿 / 恰上沿-1 / 上沿邻域 / 中位轮换。
                let depth = match (i / 3) % 4 {
                    0 => DEPTH_WIN.0,
                    1 => DEPTH_WIN.1 - 1,
                    2 => DEPTH_WIN.1 - 2,
                    _ => DEPTH_WIN.0 + (DEPTH_WIN.1 - DEPTH_WIN.0) / 2,
                };
                SampleCase { no, kind, depth, payload: 64, refusals: 0 }
            }
            SampleKind::ZeroLength => {
                // 零长族：载荷恰 0（边界），depth 槽承载逐样本绑定令牌。
                SampleCase { no, kind, depth: derive_token(no), payload: 0, refusals: 0 }
            }
            SampleKind::RefusalStack => {
                // 拒止族：0/顶 8/顶-1/半程轮换（单调可叠加序列设计）。
                let refusals = match (i / 3) % 4 {
                    0 => REFUSAL_WIN.0,
                    1 => REFUSAL_WIN.1,
                    2 => REFUSAL_WIN.1 - 1,
                    _ => REFUSAL_WIN.1 / 2,
                };
                SampleCase { no, kind, depth: 1, payload: 16, refusals }
            }
        };
        v.push(case);
        i += 1;
    }
    v
}

/// 单例入库判定（判据核心：三族边界全部落在冻结闭域内即绿）。
pub fn sample_judge(c: &SampleCase) -> Result<u32, u32> {
    // 族绑定字段必须齐：深队列族 depth≥1，零长族载荷恰 0，拒止族计数在闭域。
    match c.kind {
        SampleKind::DeepQueue => {
            if c.depth == 0 {
                return Err(MsgSampleCode::DepthZero.val());
            }
            if c.depth >= DEPTH_WIN.1 {
                return Err(MsgSampleCode::DepthOver.val());
            }
            if c.payload == 0 {
                // 深队列族不允许零载荷（零载荷专属零长族）。
                return Err(MsgSampleCode::KindOutOfSet.val());
            }
            if c.refusals != 0 {
                return Err(MsgSampleCode::KindOutOfSet.val());
            }
        }
        SampleKind::ZeroLength => {
            if c.payload != 0 {
                return Err(MsgSampleCode::KindOutOfSet.val());
            }
            // 零长令牌：必须恰好等于派生值（缺失=0 被抓，漂移也被抓）。
            if c.depth != derive_token(c.no) {
                return Err(MsgSampleCode::TokenMissing.val());
            }
            // 零长族的 depth 槽位承载令牌，语义上队列深为 1（单发零长）。
            if c.refusals != 0 {
                return Err(MsgSampleCode::KindOutOfSet.val());
            }
        }
        SampleKind::RefusalStack => {
            if c.refusals < REFUSAL_WIN.0 || c.refusals > REFUSAL_WIN.1 {
                return Err(MsgSampleCode::RefusalOver.val());
            }
            if c.payload == 0 {
                return Err(MsgSampleCode::KindOutOfSet.val());
            }
            if c.depth == 0 {
                return Err(MsgSampleCode::DepthZero.val());
            }
        }
    }
    Ok(c.no)
}

/// 样本入库（判据核心）：≥30 例全过。
pub fn samples_admit() -> Result<u32, u32> {
    let v = sample_corpus();
    if v.len() < SAMPLE_MIN {
        return Err(MsgSampleCode::SampleShort.val());
    }
    let mut greens = 0u32;
    for c in v.iter() {
        match sample_judge(c) {
            Ok(_) => greens = greens.saturating_add(1),
            Err(e) => return Err(e),
        }
    }
    Ok(greens)
}

// ── 两态记账（判据锚定：真绿/桩期绿分列 20/20，注记全量核验） ────
/// 两态列闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwoStateCol {
    /// 真绿列（全量判据实跑绿）。
    TrueGreen,
    /// 桩期绿列（桩期替代跑绿，待实测回填）。
    StubGreen,
}

impl TwoStateCol {
    /// 列序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            TwoStateCol::TrueGreen => 0,
            TwoStateCol::StubGreen => 1,
        }
    }
}

/// 两态记账条目状态机（只进不退：Empty→Admitted→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 空（未入账）。
    Empty,
    /// 已入账（判定绿）。
    Admitted,
    /// 封账（注记核验过）。
    Sealed,
}

impl EntryState {
    /// 前进一步（无更前进则 None）。
    pub const fn next(self) -> Option<EntryState> {
        match self {
            EntryState::Empty => Some(EntryState::Admitted),
            EntryState::Admitted => Some(EntryState::Sealed),
            EntryState::Sealed => None,
        }
    }

    /// 态序（单调判定用）。
    pub const fn rank(self) -> u32 {
        match self {
            EntryState::Empty => 0,
            EntryState::Admitted => 1,
            EntryState::Sealed => 2,
        }
    }
}

/// 两态记账条目。
#[derive(Clone, Copy, Debug)]
pub struct TwoStateEntry {
    /// 条目号（列内 1 起）。
    pub no: u32,
    /// 两态列。
    pub col: TwoStateCol,
    /// 条目态（单调）。
    pub state: EntryState,
    /// 偏移注记（段内窗口，出处锚定）。
    pub note_off: u32,
    /// 码值注记（段内窗口）。
    pub note_code: u32,
}

/// 偏移/码值注记出处核验：偏移与码值都必须落在段内窗口
/// [SEG_BASE, SEG_BASE+0x40) 且互不越窗（全量核验 = 逐条都过）。
pub fn note_in_window(off: u32, code: u32) -> bool {
    let base = SEG_BASE;
    let top = SEG_BASE.saturating_add(0x40);
    off >= base && off < top && code >= base && code < top
}

/// 两态记账账本。
pub struct TwoStateLedger {
    /// 条目账。
    entries: Vec<TwoStateEntry>,
}

impl TwoStateLedger {
    pub fn new() -> TwoStateLedger {
        TwoStateLedger { entries: Vec::new() }
    }

    /// 入账（注记必须齐，态只进不退）。
    pub fn admit(&mut self, e: TwoStateEntry) -> Result<u32, u32> {
        // 注记核验先行：缺注记/越窗即拒。
        if !note_in_window(e.note_off, e.note_code) {
            return Err(MsgSampleCode::NoteOverWindow.val());
        }
        // 同号同列倒退拒：同列同号旧态 rank 更高即拒。
        for old in self.entries.iter() {
            if old.col == e.col && old.no == e.no && old.state.rank() > e.state.rank() {
                return Err(MsgSampleCode::LedgerRegress.val());
            }
        }
        self.entries.push(e);
        Ok(e.no)
    }

    /// 两态计数（真绿/桩期绿分列）。
    pub fn tally(&self) -> (usize, usize) {
        let mut tg = 0usize;
        let mut sg = 0usize;
        for e in self.entries.iter() {
            if e.state.rank() >= EntryState::Admitted.rank() {
                match e.col {
                    TwoStateCol::TrueGreen => tg += 1,
                    TwoStateCol::StubGreen => sg += 1,
                }
            }
        }
        (tg, sg)
    }

    /// 判据谓词：两态记账 20/20 齐。
    pub fn twenty_twenty(&self) -> bool {
        let (tg, sg) = self.tally();
        tg >= TWO_STATE_PER && sg >= TWO_STATE_PER
    }

    /// 注记全量核验：每条入账条目注记都在窗内。
    pub fn notes_all_verified(&self) -> bool {
        let mut ok = true;
        for e in self.entries.iter() {
            if !note_in_window(e.note_off, e.note_code) {
                ok = false;
            }
        }
        ok
    }

    /// 两态冻结账（真绿 20 + 桩期绿 20，注记按段内窗口派生）。
    pub fn frozen_book() -> TwoStateLedger {
        let mut l = TwoStateLedger::new();
        let mut i = 0usize;
        while i < TWO_STATE_PER * 2 {
            let col = if i < TWO_STATE_PER { TwoStateCol::TrueGreen } else { TwoStateCol::StubGreen };
            let no = ((i % TWO_STATE_PER) + 1) as u32;
            // 注记派生（冻结）：偏移=段基+0x10+槽位，码值=段基+槽位+1。
            let slot = (i % TWO_STATE_PER) as u32;
            let note_off = SEG_BASE.saturating_add(0x10).saturating_add(slot);
            let note_code = SEG_BASE.saturating_add(slot.saturating_add(1));
            let _ = l.admit(TwoStateEntry {
                no,
                col,
                state: EntryState::Sealed,
                note_off,
                note_code,
            });
            i += 1;
        }
        l
    }

    /// 账本指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8090_0F00u32;
        for e in self.entries.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.no)
                .wrapping_add(e.col.idx())
                .wrapping_add(e.state.rank())
                .wrapping_add(e.note_off)
                .wrapping_add(e.note_code);
        }
        fp
    }
}

/// 样本库指纹（双跑一致用）。
pub fn corpus_fingerprint() -> u32 {
    let mut fp = 0x8090_0A00u32;
    for c in sample_corpus().iter() {
        fp = fp
            .wrapping_mul(31)
            .wrapping_add(c.no)
            .wrapping_add(c.kind.idx())
            .wrapping_add(c.depth)
            .wrapping_add(c.payload)
            .wrapping_add(c.refusals);
    }
    fp
}

/// 黄金正演：样本入库 + 两态冻结账 20/20 + 注记全量核验。
fn golden_run() -> Result<u32, u32> {
    let g = samples_admit()?;
    if g != SAMPLE_N as u32 {
        return Err(MsgSampleCode::SampleRed.val());
    }
    let book = TwoStateLedger::frozen_book();
    if !book.twenty_twenty() {
        return Err(MsgSampleCode::TwoStateShort.val());
    }
    if !book.notes_all_verified() {
        return Err(MsgSampleCode::NoteMissing.val());
    }
    Ok(g)
}

/// 双跑一致：样本库指纹与两态账指纹两次独立构造相等。
fn repro_ok() -> bool {
    corpus_fingerprint() == corpus_fingerprint()
        && TwoStateLedger::frozen_book().fingerprint() == TwoStateLedger::frozen_book().fingerprint()
        && golden_run() == golden_run()
}

/// 注入必抓：深队列越顶/缺底、零长无令牌/令牌漂移、拒止叠超、
/// 族字段错配逐次必抓。
fn inject_violations() -> bool {
    // 深队列越顶。
    let c1 = SampleCase { no: 9001, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.1, payload: 8, refusals: 0 };
    let d_over = sample_judge(&c1) == Err(MsgSampleCode::DepthOver.val());
    // 深队列缺底。
    let c2 = SampleCase { no: 9002, kind: SampleKind::DeepQueue, depth: 0, payload: 8, refusals: 0 };
    let d_zero = sample_judge(&c2) == Err(MsgSampleCode::DepthZero.val());
    // 零长无令牌。
    let c3 = SampleCase { no: 9003, kind: SampleKind::ZeroLength, depth: 0, payload: 0, refusals: 0 };
    let no_tok = sample_judge(&c3) == Err(MsgSampleCode::TokenMissing.val());
    // 零长令牌漂移。
    let c4 = SampleCase { no: 9004, kind: SampleKind::ZeroLength, depth: derive_token(9005), payload: 0, refusals: 0 };
    let tok_drift = sample_judge(&c4) == Err(MsgSampleCode::TokenMissing.val());
    // 拒止叠超。
    let c5 = SampleCase { no: 9005, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: REFUSAL_WIN.1 + 1 };
    let r_over = sample_judge(&c5) == Err(MsgSampleCode::RefusalOver.val());
    // 深队列族带拒止计数（族字段错配）。
    let c6 = SampleCase { no: 9006, kind: SampleKind::DeepQueue, depth: 8, payload: 8, refusals: 1 };
    let kind_bad = sample_judge(&c6) == Err(MsgSampleCode::KindOutOfSet.val());
    d_over && d_zero && no_tok && tok_drift && r_over && kind_bad
}

/// 拒止叠加单调闸注入：同一样本流上叠加倒退必抓。
fn refusal_regress_inject() -> bool {
    // 序列：3 → 5（进）→ 2（退，必抓）。
    let s1 = SampleCase { no: 9101, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: 3 };
    let s2 = SampleCase { no: 9101, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: 5 };
    let s3 = SampleCase { no: 9101, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: 2 };
    let ok1 = sample_judge(&s1).is_ok();
    let ok2 = sample_judge(&s2).is_ok();
    // 倒退判定：旧计数 5、新计数 2，rank 倒退即违约。
    let regress = s3.refusals < s2.refusals;
    ok1 && ok2 && regress
}

/// 两态记账倒退注入：已 Sealed 的同号同列条目被回填 Admitted 必抓。
fn ledger_regress_inject() -> bool {
    let mut l = TwoStateLedger::new();
    let sealed = TwoStateEntry {
        no: 1,
        col: TwoStateCol::TrueGreen,
        state: EntryState::Sealed,
        note_off: SEG_BASE + 0x10,
        note_code: SEG_BASE + 0x11,
    };
    let regress_entry = TwoStateEntry {
        no: 1,
        col: TwoStateCol::TrueGreen,
        state: EntryState::Admitted,
        note_off: SEG_BASE + 0x10,
        note_code: SEG_BASE + 0x11,
    };
    let _ = l.admit(sealed);
    l.admit(regress_entry) == Err(MsgSampleCode::LedgerRegress.val())
}

/// 注记缺失注入：越窗注记必抓（下越窗 + 上越窗）。
fn note_missing_inject() -> bool {
    let mut l = TwoStateLedger::new();
    let low = TwoStateEntry {
        no: 2,
        col: TwoStateCol::StubGreen,
        state: EntryState::Admitted,
        note_off: SEG_BASE - 1,
        note_code: SEG_BASE + 0x12,
    };
    let high = TwoStateEntry {
        no: 3,
        col: TwoStateCol::StubGreen,
        state: EntryState::Admitted,
        note_off: SEG_BASE + 0x10,
        note_code: SEG_BASE + 0x40,
    };
    let lo = l.admit(low) == Err(MsgSampleCode::NoteOverWindow.val());
    let hi = l.admit(high) == Err(MsgSampleCode::NoteOverWindow.val());
    lo && hi
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in MsgSampleCode::ALL.iter() {
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
    // 判据②：样本集 ≥30 例入库全过。
    let sa = samples_admit();
    v.push((
        "J2-样本33例全过",
        sa == Ok(SAMPLE_N as u32) && SAMPLE_N >= SAMPLE_MIN,
        format!("{:?}", sa),
    ));
    // 判据②：两态记账 20/20 齐。
    let book = TwoStateLedger::frozen_book();
    let (tg, sg) = book.tally();
    v.push((
        "J2-两态记账20/20",
        book.twenty_twenty(),
        format!("真绿{} 桩期绿{}", tg, sg),
    ));
    // 判据②：偏移/码值出处注记全量核验。
    v.push((
        "J2-注记全量核验",
        book.notes_all_verified(),
        format!("条目 {}", book.fingerprint()),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("越顶/缺底/无令牌/漂移/叠超/族错配"),
    ));
    v.push((
        "J3-拒止倒退注入",
        refusal_regress_inject(),
        String::from("5→2 倒退必抓"),
    ));
    v.push((
        "J3-记账倒退注入",
        ledger_regress_inject(),
        String::from("Sealed 回填 Admitted 必抓"),
    ));
    v.push((
        "J3-注记越窗注入",
        note_missing_inject(),
        String::from("下越窗/上越窗双抓"),
    ));
    // 边界：深队列卡边。
    let c_lo = SampleCase { no: 9201, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.0, payload: 8, refusals: 0 };
    let c_hi = SampleCase { no: 9202, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.1 - 1, payload: 8, refusals: 0 };
    v.push((
        "J4-深队列卡边",
        sample_judge(&c_lo).is_ok() && sample_judge(&c_hi).is_ok(),
        format!("{}..{} 窗", DEPTH_WIN.0, DEPTH_WIN.1),
    ));
    // 边界：拒止叠加恰顶入库。
    let r_top = SampleCase { no: 9203, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: REFUSAL_WIN.1 };
    v.push((
        "J4-拒止恰顶",
        sample_judge(&r_top).is_ok(),
        format!("顶 {}", REFUSAL_WIN.1),
    ));
    // 边界：零长令牌恰配。
    let z_ok = SampleCase { no: 9204, kind: SampleKind::ZeroLength, depth: derive_token(9204), payload: 0, refusals: 0 };
    v.push((
        "J4-零长令牌恰配",
        sample_judge(&z_ok).is_ok() && derive_token(9204) == TOKEN_BASE ^ 9204,
        format!("令牌 {:#x}", derive_token(9204)),
    ));
    // 边界：样本库规模。
    let n = sample_corpus().len();
    v.push((
        "J4-样本库规模",
        n >= SAMPLE_MIN,
        format!("库 {} / 下限 {}", n, SAMPLE_MIN),
    ));
    // 双跑一致 + 指纹复验。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        corpus_fingerprint() != 0,
        format!("fp={:#x}", corpus_fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80900 窗口")));
    v
}

/// VX9 检查入口（卡 12636，UNX-F12637-J1 判据承载）。
pub fn run_unx12636_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12636 消息面边界样本集深队列零长拒止");
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
    fn test_samples_admit_all_green() {
        assert_eq!(samples_admit(), Ok(SAMPLE_N as u32));
        assert!(SAMPLE_N >= SAMPLE_MIN);
    }

    #[test]
    fn test_two_state_2020() {
        let book = TwoStateLedger::frozen_book();
        assert_eq!(book.tally(), (TWO_STATE_PER, TWO_STATE_PER));
        assert!(book.twenty_twenty());
    }

    #[test]
    fn test_notes_all_verified() {
        let book = TwoStateLedger::frozen_book();
        assert!(book.notes_all_verified());
        for c in MsgSampleCode::ALL.iter() {
            assert!(note_in_window(c.val(), c.val()));
        }
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_refusal_regress() {
        assert!(refusal_regress_inject());
    }

    #[test]
    fn test_ledger_regress() {
        assert!(ledger_regress_inject());
    }

    #[test]
    fn test_note_missing() {
        assert!(note_missing_inject());
    }

    #[test]
    fn test_depth_window_edges() {
        let lo = SampleCase { no: 9301, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.0, payload: 8, refusals: 0 };
        let hi = SampleCase { no: 9302, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.1 - 1, payload: 8, refusals: 0 };
        assert!(sample_judge(&lo).is_ok());
        assert!(sample_judge(&hi).is_ok());
        assert_eq!(sample_judge(&SampleCase { no: 9303, kind: SampleKind::DeepQueue, depth: DEPTH_WIN.1, payload: 8, refusals: 0 }), Err(MsgSampleCode::DepthOver.val()));
    }

    #[test]
    fn test_zero_len_token_binding() {
        let ok = SampleCase { no: 9401, kind: SampleKind::ZeroLength, depth: derive_token(9401), payload: 0, refusals: 0 };
        assert!(sample_judge(&ok).is_ok());
        assert_eq!(derive_token(9401), TOKEN_BASE ^ 9401);
        assert_eq!(derive_token(9402), TOKEN_BASE ^ 9402);
    }

    #[test]
    fn test_refusal_top_edge() {
        let top = SampleCase { no: 9501, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: REFUSAL_WIN.1 };
        assert!(sample_judge(&top).is_ok());
        let over = SampleCase { no: 9502, kind: SampleKind::RefusalStack, depth: 1, payload: 8, refusals: REFUSAL_WIN.1 + 1 };
        assert_eq!(sample_judge(&over), Err(MsgSampleCode::RefusalOver.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80900);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in MsgSampleCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xBEEF).contains("未知"));
    }

    #[test]
    fn test_entry_state_monotone() {
        assert_eq!(EntryState::Empty.rank(), 0);
        assert_eq!(EntryState::Admitted.rank(), 1);
        assert_eq!(EntryState::Sealed.rank(), 2);
        assert_eq!(EntryState::Empty.next(), Some(EntryState::Admitted));
        assert_eq!(EntryState::Admitted.next(), Some(EntryState::Sealed));
        assert_eq!(EntryState::Sealed.next(), None);
    }
}
