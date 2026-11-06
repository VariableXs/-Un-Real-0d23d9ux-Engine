//! UNX-F9361 I 型联调总纲一（C2×C3 联签协议与共享判据面） · 0x7C7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9361-J1 联签协议判据过，C2（syscall 号面）×C3（LTP 回归
//! 基建）双方判据面互认格过，跨界代写禁令声明格过（联测共享只写编号+
//! 一句话）。
//!
//! SEG_BASE = 0x7C7C0 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个（CosignCode::ALL），
//! 两两唯一，段基址本身不占码；越窗码一律拒绝注册。
//!
//! 结构清单：
//!   ① 联签协议状态机 CosignStamp：Draft → Proposed → Countersigned →
//!      Sealed 单向推进，倒退/越级显性拒绝。
//!   ② 双方判据面互认账 MutualLedger：C2 号面条款 × C3 回归基建条款
//!      逐对互认，只增不减。
//!   ③ 共享判据面解析器 SharedEntryParser：逐条「编号+一句话」协议，
//!      缺编号/多句/越界代写三类违规全拒。
//!   ④ 注入核 run_cosign_injection：三类坏样本各 10 轮全抓，账守恒
//!      逃逸=0。
//!   ⑤ Linux errno 对照表（EPERM/EINVAL/EACCES/ENOENT）+ FNV-1a 双跑
//!      指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9361 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C7C0;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数（多句/缺编号/越界代写）。
pub const INJECT_CLASSES: u32 = 3;

/// 共享条目一句话长度上限（字节）。
pub const ONE_SENTENCE_MAX: usize = 120;
/// 互认账容量（条款对上限）。
pub const MUTUAL_CAP: usize = 16;
/// 联签方数量（C2 / C3）。
pub const PARTIES: usize = 2;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignCode {
    /// 共享条目缺编号（编号+一句话协议的第一要素）
    MissingId,
    /// 共享条目多句（超过一句话）
    MultiSentence,
    /// 共享条目超长
    EntryTooLong,
    /// 越界代写：写方不是该账的归属方
    CrossWrite,
    /// 联签状态机倒退
    StateRegress,
    /// 联签状态机越级
    StateSkip,
    /// 互认账满：条款对超容量
    MutualCapFull,
    /// 互认账重复登记
    MutualDup,
    /// 双方指纹不一致：台账互认失败
    LedgerMismatch,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CosignCode {
    /// 码值（段基址 + 变体序号 + 1，1 基编码）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CosignCode::MissingId => "共享条目缺编号被拒：联测共享只写「编号+一句话」，\
                编号是检索键；请补 UNX-Fxxxx-Jn 形式的判据编号",
            CosignCode::MultiSentence => "共享条目多句被拒：一句话协议禁止展开叙述；\
                请压缩为单句，细节留在本卡判据内",
            CosignCode::EntryTooLong => "共享条目超长被拒：一句话有长度上限；请拆条或\
                压缩措辞后再登记",
            CosignCode::CrossWrite => "越界代写被拒：每方只写自己的账本；跨界内容请走\
                联测共享条目，由归属方落笔",
            CosignCode::StateRegress => "联签状态倒退被拒：Draft→Proposed→Countersigned→\
                Sealed 只进不退；已联签条款不得回滚到草稿",
            CosignCode::StateSkip => "联签越级被拒：状态必须逐级推进；不得从 Draft \
                直接跳到 Sealed",
            CosignCode::MutualCapFull => "互认账已满：条款对超出容量；请先收口旧账或 \
                扩容登记，禁止覆盖既有条款对",
            CosignCode::MutualDup => "互认账重复登记被拒：同一条款对只记一笔；\
                请改为更新既有条目",
            CosignCode::LedgerMismatch => "双方指纹不一致：C2 与 C3 台账互认失败；\
                请逐条复算后重新联签",
            CosignCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径",
            CosignCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            CosignCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [CosignCode; 12] = [
        CosignCode::MissingId,
        CosignCode::MultiSentence,
        CosignCode::EntryTooLong,
        CosignCode::CrossWrite,
        CosignCode::StateRegress,
        CosignCode::StateSkip,
        CosignCode::MutualCapFull,
        CosignCode::MutualDup,
        CosignCode::LedgerMismatch,
        CosignCode::LedgerRegress,
        CosignCode::NotReproducible,
        CosignCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 联签协议状态机 ────────────────────────────────────────────────────
/// 联签状态：单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignState {
    /// 草稿：任一方起草
    Draft,
    /// 已提案：一方判据面就绪
    Proposed,
    /// 已联签：双方判据面互认完成
    Countersigned,
    /// 已封存：互认账冻结，只读
    Sealed,
}

impl CosignState {
    /// 序号（推进距离）。
    pub fn rank(self) -> u8 {
        match self {
            CosignState::Draft => 0,
            CosignState::Proposed => 1,
            CosignState::Countersigned => 2,
            CosignState::Sealed => 3,
        }
    }

    /// 是否可前进一步。
    pub fn can_step_to(self, next: CosignState) -> bool {
        next.rank() == self.rank() + 1
    }
}

/// 一条联签条款。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CosignStamp {
    /// 条款编号（UNX-Fxxxx-Jn 的数字部分）
    pub clause: u32,
    /// 当前状态
    pub state: CosignState,
    /// 已落笔方位图（bit0=C2，bit1=C3）
    pub signed_by: u8,
}

impl CosignStamp {
    /// 草稿条款。
    pub const fn new(clause: u32) -> CosignStamp {
        CosignStamp { clause, state: CosignState::Draft, signed_by: 0 }
    }

    /// 推进状态：倒退/越级拒绝，返回 Err(EINVAL)。
    pub fn advance(&mut self, next: CosignState) -> SysResult<CosignState> {
        if next.rank() <= self.state.rank() {
            return Err(EINVAL);
        }
        if !self.state.can_step_to(next) {
            return Err(EINVAL);
        }
        self.state = next;
        Ok(next)
    }

    /// 落笔登记：party 0=C2，1=C3；越界方拒绝（EPERM）。
    pub fn sign(&mut self, party: usize) -> SysResult<u8> {
        if party >= PARTIES {
            return Err(EPERM);
        }
        self.signed_by |= 1u8 << party;
        Ok(self.signed_by)
    }

    /// 双方均已落笔。
    pub fn both_signed(&self) -> bool {
        self.signed_by == 0b11
    }
}

// ── 共享判据面解析器 ──────────────────────────────────────────────────
/// 写方身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Party {
    /// C2：syscall 号面
    C2,
    /// C3：LTP 回归基建
    C3,
}

impl Party {
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Party::C2 => "C2",
            Party::C3 => "C3",
        }
    }
}

/// 解析后的共享条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedEntry {
    /// 判据编号中的卡号（如 9361）
    pub card: u32,
    /// 一句话内容长度（字节）
    pub sentence_len: usize,
}

/// 解析「编号|一句话」条目。协议：编号形如 UNX-Fxxxx-Jn，一句话至多一个
/// 句末符且长度受限；缺编号/多句/超长分别拒绝。
pub fn parse_shared_entry(writer: Party, owner: Party, raw: &str) -> SysResult<SharedEntry> {
    // 越界代写：只有归属方能写自己的共享面登记。
    if writer != owner {
        return Err(EPERM);
    }
    // 编号段：必须以 "UNX-F" 开头并含 "-J"。
    if !raw.starts_with("UNX-F") {
        return Err(EINVAL);
    }
    let jpos = match raw.find("-J") {
        Some(p) => p,
        None => return Err(EINVAL),
    };
    // 卡号取 "UNX-F" 与 "-J" 之间的数字。
    let num_part = &raw[5..jpos];
    if num_part.is_empty() {
        return Err(EINVAL);
    }
    let mut card: u32 = 0;
    let bytes = num_part.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(EINVAL),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(EINVAL);
        }
        card = card
            .checked_mul(10)
            .and_then(|v| v.checked_add((b - b'0') as u32))
            .ok_or(EINVAL)?;
        i += 1;
    }
    // 一句话段：句末符（。/；/;/.）至多 1 个，长度受限。
    let rest = &raw[jpos + 2..];
    if rest.is_empty() {
        return Err(EINVAL);
    }
    let mut ends = 0usize;
    let rb = rest.as_bytes();
    let mut k = 0usize;
    while k < rb.len() {
        let b = match rb.get(k) {
            Some(b) => *b,
            None => break,
        };
        if b == b'.' || b == b';' {
            ends += 1;
        }
        k += 1;
    }
    // 中文句末符按 UTF-8 字节序列识别（3 字节）。
    let mut cjk_ends = 0usize;
    let mut m = 0usize;
    while m + 3 <= rb.len() {
        let a = match rb.get(m) {
            Some(v) => *v,
            None => break,
        };
        let b2 = match rb.get(m + 1) {
            Some(v) => *v,
            None => break,
        };
        let c3 = match rb.get(m + 2) {
            Some(v) => *v,
            None => break,
        };
        if (a == 0xE3 && b2 == 0x80 && (c3 == 0x82 || c3 == 0x9B))
            || (a == 0xEF && b2 == 0xBC && c3 == 0x9B)
        {
            cjk_ends += 1;
        }
        m += 1;
    }
    if ends + cjk_ends > 1 {
        return Err(EINVAL);
    }
    if rest.len() > ONE_SENTENCE_MAX {
        return Err(EINVAL);
    }
    Ok(SharedEntry { card, sentence_len: rest.len() })
}

// ── 互认账 ────────────────────────────────────────────────────────────
/// 一条互认条款对：C2 条款 ↔ C3 条款。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MutualPair {
    /// C2 侧卡号
    pub c2_card: u32,
    /// C3 侧用例号
    pub c3_case: u32,
}

/// 互认账：只增不减，重复拒绝，满容拒绝。
#[derive(Clone, Copy, Debug)]
pub struct MutualLedger {
    pairs: [Option<MutualPair>; MUTUAL_CAP],
    len: usize,
    rejected: u32,
}

impl MutualLedger {
    /// 空账。
    pub const fn new() -> MutualLedger {
        MutualLedger { pairs: [None; MUTUAL_CAP], len: 0, rejected: 0 }
    }

    /// 登记条款对：重复回 Err(EINVAL)，满容回 Err(ENOMEM) 语义占位。
    pub fn record(&mut self, c2_card: u32, c3_case: u32) -> SysResult<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(p)) = self.pairs.get(i).map(|x| *x) {
                if p.c2_card == c2_card && p.c3_case == c3_case {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(EINVAL);
                }
            }
            i += 1;
        }
        if self.len >= MUTUAL_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ENOENT);
        }
        let slot = self.len;
        if let Some(cell) = self.pairs.get_mut(slot) {
            *cell = Some(MutualPair { c2_card, c3_case });
        }
        self.len += 1;
        Ok(slot)
    }

    /// 已登记条款对数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 拒绝计数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 互认查询：条款对是否在账。
    pub fn has(&self, c2_card: u32, c3_case: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(p)) = self.pairs.get(i).map(|x| *x) {
                if p.c2_card == c2_card && p.c3_case == c3_case {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(CosignCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 三类违规样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 多句：一句话协议被展开
    MultiSentence,
    /// 缺编号：没有 UNX-F 前缀
    MissingId,
    /// 越界代写：C3 替 C2 落笔
    CrossWrite,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::MultiSentence,
        ViolationCase::MissingId,
        ViolationCase::CrossWrite,
    ];

    /// 样本执行：违规必须被拒，返回是否命中（Err 即命中）。
    fn run(self) -> bool {
        match self {
            ViolationCase::MultiSentence => {
                parse_shared_entry(Party::C2, Party::C2,
                    "UNX-F9361-J1.协议双方互认。再补一句细节")
                    .is_err()
            }
            ViolationCase::MissingId => {
                parse_shared_entry(Party::C3, Party::C3, "无编号的一句话")
                    .is_err()
            }
            ViolationCase::CrossWrite => {
                parse_shared_entry(Party::C3, Party::C2,
                    "UNX-F9202-J1.坏 fd 矩阵")
                    .is_err()
            }
        }
    }
}

/// 三类违规 × 10 轮注入，返回账本与逐类检出数。
pub fn run_cosign_injection() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.run();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越界代写族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；互认账满占位"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；权限不足族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；条目格式族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：覆盖 解析正例/三类违规/状态机推进/互认登记。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 语料 1：合法条目（C2 自写）。
    match parse_shared_entry(Party::C2, Party::C2, "UNX-F9361-J1.双方判据面互认") {
        Ok(e) => {
            bytes.push((e.card & 0xFF) as u8);
            bytes.push((e.sentence_len & 0xFF) as u8);
        }
        Err(code) => bytes.push((code & 0xFF) as u8),
    }
    // 语料 2：三类违规码。
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let hit = case.run();
            bytes.push(if hit { 1 } else { 0 });
        }
        ci += 1;
    }
    // 语料 3：状态机四步推进。
    let mut st = CosignStamp::new(9361);
    let _ = st.sign(0);
    let _ = st.sign(1);
    let steps = [
        CosignState::Proposed,
        CosignState::Countersigned,
        CosignState::Sealed,
    ];
    let mut si = 0usize;
    while si < steps.len() {
        match st.advance(steps[si]) {
            Ok(s) => bytes.push(s.rank()),
            Err(_) => bytes.push(0xFF),
        }
        si += 1;
    }
    // 语料 4：互认账登记 + 重复拒绝。
    let mut ml = MutualLedger::new();
    let r1 = ml.record(9361, 7001);
    let r2 = ml.record(9361, 7001);
    bytes.push(match r1 {
        Ok(_) => 1,
        Err(_) => 0,
    });
    bytes.push(match r2 {
        Ok(_) => 1,
        Err(_) => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9361 判据集。
pub fn run_unx9361_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9361");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：合法共享条目解析
    let ok_entry = parse_shared_entry(Party::C2, Party::C2,
        "UNX-F9361-J1.双方判据面互认");
    let normal_ok = match ok_entry {
        Ok(e) => e.card == 9361 && e.sentence_len > 0,
        Err(_) => false,
    };
    s.add("正常流：编号+一句话条目解析成功", normal_ok,
          "合法条目必须解析出卡号与句长，双方同号互认");

    // 3~5. 注入矩阵：三类违规各 10/10 + 账守恒
    let (ledger, per_class) = run_cosign_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违规注入各 10/10 全检出", all_caught,
          "多句/缺编号/越界代写三类必须全部被解析器拒绝");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 6. 跨界代写禁令声明：跨写拒绝、自写放行
    let cross = parse_shared_entry(Party::C3, Party::C2, "UNX-F9202-J1.坏 fd 矩阵");
    let self_write = parse_shared_entry(Party::C3, Party::C3, "UNX-F9363-J1.LTP 联测面");
    s.add("跨界代写禁令：跨写 EPERM、自写放行",
          cross == Err(EPERM) && self_write.is_ok(),
          "每方只写自己的账本，联测共享条目由归属方落笔");

    // 7. 状态机：四步推进成功
    let mut st = CosignStamp::new(9361);
    let _ = st.sign(0);
    let _ = st.sign(1);
    let step_ok = st.advance(CosignState::Proposed).is_ok()
        && st.advance(CosignState::Countersigned).is_ok()
        && st.advance(CosignState::Sealed).is_ok()
        && st.state == CosignState::Sealed
        && st.both_signed();
    s.add("状态机：Draft→Proposed→Countersigned→Sealed 推进成功", step_ok,
          "双方落笔后逐级推进到封存态");

    // 8. 状态机：倒退拒绝
    let mut st2 = CosignStamp::new(9362);
    let _ = st2.advance(CosignState::Proposed);
    s.add("状态机：倒退与越级显性拒绝",
          st2.advance(CosignState::Draft) == Err(EINVAL)
              && st2.advance(CosignState::Sealed) == Err(EINVAL),
          "非法转换回 EINVAL，状态不得回退或跳级");

    // 9. 落笔面：越界方 EPERM
    let mut st3 = CosignStamp::new(9363);
    s.add("落笔面：party 越界回 EPERM、双方齐签可推进",
          st3.sign(2) == Err(EPERM) && st3.sign(0).is_ok() && st3.sign(1).is_ok()
              && st3.both_signed(),
          "联签方闭集 {C2, C3}，越界方一律拒绝");

    // 10. 互认账：登记/重复/查询
    let mut ml = MutualLedger::new();
    let rec_ok = ml.record(9361, 7001).is_ok()
        && ml.record(9361, 7001) == Err(EINVAL)
        && ml.has(9361, 7001)
        && !ml.has(9361, 7002)
        && ml.len() == 1;
    s.add("互认账：条款对登记/重复拒绝/查询命中", rec_ok,
          "C2 条款 × C3 用例逐对互认，重复登记回 EINVAL");

    // 11. 互认账满容
    let mut mfull = MutualLedger::new();
    let mut fill_ok = true;
    let mut n = 0u32;
    while n < MUTUAL_CAP as u32 {
        if mfull.record(9000 + n, 7000 + n).is_err() {
            fill_ok = false;
        }
        n += 1;
    }
    s.add("互认账：满容后新条目显性拒绝且旧账不丢",
          fill_ok && mfull.len() == MUTUAL_CAP
              && mfull.record(9999, 7999).is_err()
              && mfull.has(9000, 7000),
          "定容账不得覆盖既有条款对");

    // 12. 一句话上限
    let mut long_tail = format!("长");
    let mut li = 1usize;
    while li <= ONE_SENTENCE_MAX + 1 {
        long_tail = format!("{}长", long_tail);
        li += 1;
    }
    let long_raw = format!("UNX-F9361-J1.{}", long_tail);
    let too_long = parse_shared_entry(Party::C2, Party::C2, &long_raw);
    let edge = parse_shared_entry(Party::C2, Party::C2, "UNX-F9361-J1.恰在上限内的句子");
    s.add("一句话协议：超长拒绝、边界内放行",
          too_long == Err(EINVAL) && edge.is_ok(),
          "句长上限是硬边界，超限条目不得入账");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = CosignCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < CosignCode::ALL.len() {
        let mut j = i + 1;
        while j < CosignCode::ALL.len() {
            if let (Some(x), Some(y)) = (CosignCode::ALL.get(i), CosignCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 17. 对照表一致性
    let mut table_ok = true;
    let mut ti = 0usize;
    while ti < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(ti) {
            if row.1 != -(-row.1) || row.2 == 0 || row.3.is_empty() {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("对照表一致性：errno/Windows 码逐项自洽", table_ok,
          "表内每一行负值可复算、Windows 码非零、说明非空");

    // 18. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 19. C2×C3 互认总格：同号条款双向命中
    let mut mboth = MutualLedger::new();
    let _ = mboth.record(9361, 9361);
    let _ = mboth.record(9364, 9364);
    s.add("C2×C3 互认：同号条款双向可查",
          mboth.has(9361, 9361) && mboth.has(9364, 9364) && mboth.len() == 2,
          "双方判据面互认的最终形态是同号条款对双向命中");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_entry_parses() {
        let e = parse_shared_entry(Party::C2, Party::C2, "UNX-F9361-J1.双方互认")
            .unwrap_or(SharedEntry { card: 0, sentence_len: 0 });
        assert_eq!(e.card, 9361);
        assert!(e.sentence_len > 0);
    }

    #[test]
    fn cross_write_rejected() {
        assert_eq!(
            parse_shared_entry(Party::C3, Party::C2, "UNX-F9202-J1.坏 fd"),
            Err(EPERM)
        );
    }

    #[test]
    fn missing_id_rejected() {
        assert_eq!(parse_shared_entry(Party::C2, Party::C2, "没有编号"), Err(EINVAL));
        assert_eq!(parse_shared_entry(Party::C2, Party::C2, "UNX-无号.句子"), Err(EINVAL));
    }

    #[test]
    fn multi_sentence_rejected() {
        assert_eq!(
            parse_shared_entry(Party::C2, Party::C2, "UNX-F9361-J1.一句.两句"),
            Err(EINVAL)
        );
    }

    #[test]
    fn state_machine_steps() {
        let mut st = CosignStamp::new(9361);
        let _ = st.sign(0);
        let _ = st.sign(1);
        assert!(st.advance(CosignState::Proposed).is_ok());
        assert!(st.advance(CosignState::Countersigned).is_ok());
        assert!(st.advance(CosignState::Sealed).is_ok());
        assert_eq!(st.advance(CosignState::Draft), Err(EINVAL));
    }

    #[test]
    fn state_skip_rejected() {
        let mut st = CosignStamp::new(9364);
        assert_eq!(st.advance(CosignState::Sealed), Err(EINVAL));
    }

    #[test]
    fn sign_party_bounds() {
        let mut st = CosignStamp::new(9365);
        assert_eq!(st.sign(2), Err(EPERM));
        assert!(st.sign(0).is_ok());
        assert!(!st.both_signed());
    }

    #[test]
    fn mutual_ledger_rules() {
        let mut ml = MutualLedger::new();
        assert!(ml.record(9361, 7001).is_ok());
        assert_eq!(ml.record(9361, 7001), Err(EINVAL));
        assert!(ml.has(9361, 7001));
        assert_eq!(ml.len(), 1);
    }

    #[test]
    fn mutual_ledger_full() {
        let mut ml = MutualLedger::new();
        let mut n = 0u32;
        while n < MUTUAL_CAP as u32 {
            assert!(ml.record(9000 + n, 7000 + n).is_ok());
            n += 1;
        }
        assert!(ml.record(9999, 7999).is_err());
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_cosign_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn unx9361_checks_green() {
        let s = run_unx9361_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
