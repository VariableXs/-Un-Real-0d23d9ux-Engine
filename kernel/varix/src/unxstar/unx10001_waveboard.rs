//! UNX-F10001 波次报表总纲（维度与口径） · 0x91040段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10002-J1 波次聚合零缺波（六波逐波入表 10/10 次），缺波
//! 10/10 次检出。
//!
//! 设计要点：
//!   ① 报表总纲按「五维口径注册表」建模：波/批/战线/模块/判据五个维度
//!      各自冻结成员数（波 6 / 批 40 / 战线 5 / 模块 ltp 全家 / 判据 20），
//!      注册条目必须挂到合法维度，重复名与未知维度显性拒绝。
//!   ② 波名册是总纲的承重面：wave-01..wave-06 六波逐波入表，缺任何
//!      一波都不得冻结；缺波注入按「逐波各缺一次」6 类 × 10 轮复测，
//!      必须 10/10 次检出。
//!   ③ 维度生命周期是单向状态机：Draft → Frozen → Published，只进不退；
//!      Draft 直接发布、冻结后再注册、发布后再发布全部拒绝。
//!   ④ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0，账只增不减。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；指纹双跑逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10001 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91040;

/// 波维成员数（wave-01..wave-06）。
pub const WAVE_COUNT: usize = 6;

/// 批维成员数（B01..B40）。
pub const BATCH_COUNT: usize = 40;

/// 战线维成员数（五战线）。
pub const FRONT_COUNT: usize = 5;

/// 判据维成员数（每卡 20 条判据口径）。
pub const CRITERION_COUNT: usize = 20;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;

// ── 五维口径 ──────────────────────────────────────────────────────────
/// 报表五维（闭集），与总纲口径一一对应。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimKind {
    /// 波维
    Wave,
    /// 批维
    Batch,
    /// 战线维
    Front,
    /// 模块维
    Module,
    /// 判据维
    Criterion,
}

impl DimKind {
    /// 该维冻结的成员数（报表分母口径来源）。
    pub fn frozen_members(self) -> usize {
        match self {
            DimKind::Wave => WAVE_COUNT,
            DimKind::Batch => BATCH_COUNT,
            DimKind::Front => FRONT_COUNT,
            DimKind::Module => module_family_len(),
            DimKind::Criterion => CRITERION_COUNT,
        }
    }

    /// 维度中文名（注册校验用）。
    pub fn label(self) -> &'static str {
        match self {
            DimKind::Wave => "波",
            DimKind::Batch => "批",
            DimKind::Front => "战线",
            DimKind::Module => "模块",
            DimKind::Criterion => "判据",
        }
    }

    /// 五维闭集全量。
    pub const ALL: [DimKind; 5] = [
        DimKind::Wave,
        DimKind::Batch,
        DimKind::Front,
        DimKind::Module,
        DimKind::Criterion,
    ];
}

/// ltp 模块族冻结清单（模块维成员数来源，闭集）。
pub const MODULE_FAMILY: [&str; 8] = [
    "ltp_read",
    "ltp_write",
    "ltp_open",
    "ltp_close",
    "ltp_dup",
    "ltp_lseek",
    "ltp_fcntl",
    "ltp_pipe",
];

/// 模块族长度（供 DimKind::frozen_members 复算）。
pub fn module_family_len() -> usize {
    MODULE_FAMILY.len()
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardCode {
    /// 空标签
    EmptyLabel,
    /// 未知维度
    UnknownDim,
    /// 重复注册
    DuplicateLabel,
    /// 缺波：波名册缺员
    MissingWave,
    /// 波号越界：wave-00 / wave-07
    WaveOutOfRange,
    /// 波名句式坏：不符 wave-NN
    BadWaveLabel,
    /// 未冻结发布
    PublishBeforeFreeze,
    /// 冻结后再注册
    RegisterAfterFreeze,
    /// 状态回退
    StateRegress,
    /// 维度有缺
    MissingDim,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl BoardCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BoardCode::EmptyLabel => "空标签被拒：注册条目必须有名字；请先给条目\
                命名再挂维度",
            BoardCode::UnknownDim => "未知维度被拒：维度不在五维闭集内；请按\
                波/批/战线/模块/判据归类",
            BoardCode::DuplicateLabel => "重复注册被拒：同名条目已在册；请先查册\
                再补录，重复回 EEXIST",
            BoardCode::MissingWave => "缺波被拒：波名册六波未齐不得冻结；请逐波\
                补齐 wave-01..wave-06",
            BoardCode::WaveOutOfRange => "波号越界被拒：波号只允许 01..=06；\
                wave-00 与 wave-07 均非法",
            BoardCode::BadWaveLabel => "波名句式坏被拒：必须为 wave-NN 两段式；\
                请修正拼写后再入册",
            BoardCode::PublishBeforeFreeze => "未冻结发布被拒：总纲必须先冻结\
                才能发布；请走 Draft→Frozen→Published 次序",
            BoardCode::RegisterAfterFreeze => "冻结后注册被拒：口径冻结即闭册；\
                如需增补请开新一期总纲",
            BoardCode::StateRegress => "状态回退被拒：生命周期只进不退；回退\
                视为私改口径",
            BoardCode::MissingDim => "维度有缺：存在零条目维度，总纲不成立；\
                请回查漏注册的维度",
            BoardCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            BoardCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [BoardCode; 12] = [
        BoardCode::EmptyLabel,
        BoardCode::UnknownDim,
        BoardCode::DuplicateLabel,
        BoardCode::MissingWave,
        BoardCode::WaveOutOfRange,
        BoardCode::BadWaveLabel,
        BoardCode::PublishBeforeFreeze,
        BoardCode::RegisterAfterFreeze,
        BoardCode::StateRegress,
        BoardCode::MissingDim,
        BoardCode::NotReproducible,
        BoardCode::SegmentViolation,
    ];
}

// ── 波名解析器 ────────────────────────────────────────────────────────
/// 波名解析：wave-NN 两段式，NN ∈ 01..=06；返回波序 1..=6。
pub fn parse_wave_label(text: &str) -> Result<usize, BoardCode> {
    let bytes = text.as_bytes();
    // 句式：'w''a''v''e''-' + 两位数字 + 结尾，共 7 字节。
    if bytes.len() != 7 {
        return Err(BoardCode::BadWaveLabel);
    }
    let head: [u8; 5] = [b'w', b'a', b'v', b'e', b'-'];
    let mut i = 0usize;
    while i < 5 {
        match bytes.get(i) {
            Some(b) if *b == head[i] => {}
            _ => return Err(BoardCode::BadWaveLabel),
        }
        i += 1;
    }
    let d1 = match bytes.get(5) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BoardCode::BadWaveLabel),
    };
    let d2 = match bytes.get(6) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BoardCode::BadWaveLabel),
    };
    let n = (d1 as usize) * 10 + (d2 as usize);
    if n < 1 || n > WAVE_COUNT {
        return Err(BoardCode::WaveOutOfRange);
    }
    Ok(n)
}

/// 生成规范波名（1..=6），越界返回空串（显性可查）。
pub fn wave_label(n: usize) -> &'static str {
    match n {
        1 => "wave-01",
        2 => "wave-02",
        3 => "wave-03",
        4 => "wave-04",
        5 => "wave-05",
        6 => "wave-06",
        _ => "",
    }
}

// ── 维度生命周期状态机 ────────────────────────────────────────────────
/// 总纲生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardState {
    /// 起草中：可注册
    Draft,
    /// 已冻结：闭册待发布
    Frozen,
    /// 已发布：只读
    Published,
}

/// 口径注册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaliberEntry {
    /// 条目名（波名/批名/模块名等）
    pub label: &'static str,
    /// 所属维度
    pub kind: DimKind,
}

/// 报表总纲注册表：注册 → 冻结 → 发布三段状态机。
#[derive(Clone, Debug)]
pub struct CaliberBoard {
    state: BoardState,
    entries: Vec<CaliberEntry>,
}

impl CaliberBoard {
    /// 构造 Draft 空册。
    pub fn new() -> CaliberBoard {
        CaliberBoard { state: BoardState::Draft, entries: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> BoardState {
        self.state
    }

    /// 注册条目：Draft 才可注册；空名/重复名/未知维度拒绝。
    pub fn register(&mut self, label: &'static str, kind: DimKind) -> Result<(), BoardCode> {
        if self.state != BoardState::Draft {
            return Err(BoardCode::RegisterAfterFreeze);
        }
        if label.is_empty() {
            return Err(BoardCode::EmptyLabel);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.label == label {
                    return Err(BoardCode::DuplicateLabel);
                }
            }
            i += 1;
        }
        // 维度合法性：五维闭集之外没有归属。
        if !DimKind::ALL.iter().any(|k| *k == kind) {
            return Err(BoardCode::UnknownDim);
        }
        self.entries.push(CaliberEntry { label, kind });
        Ok(())
    }

    /// 某维条目数。
    pub fn count_of(&self, kind: DimKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 五维是否全非零。
    pub fn all_dims_present(&self) -> bool {
        let mut i = 0usize;
        while i < DimKind::ALL.len() {
            if let Some(k) = DimKind::ALL.get(i) {
                if self.count_of(*k) == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 波名册完备：wave-01..06 逐波在册（零缺波）。
    pub fn waves_complete(&self) -> bool {
        let mut n = 1usize;
        while n <= WAVE_COUNT {
            let label = wave_label(n);
            if label.is_empty() {
                return false;
            }
            let mut found = false;
            let mut i = 0usize;
            while i < self.entries.len() {
                if let Some(e) = self.entries.get(i) {
                    if e.label == label && e.kind == DimKind::Wave {
                        found = true;
                    }
                }
                i += 1;
            }
            if !found {
                return false;
            }
            n += 1;
        }
        true
    }

    /// 冻结：Draft → Frozen；要求五维齐且波名册零缺波。
    pub fn freeze(&mut self) -> Result<(), BoardCode> {
        if self.state != BoardState::Draft {
            return Err(BoardCode::StateRegress);
        }
        if !self.waves_complete() {
            return Err(BoardCode::MissingWave);
        }
        if !self.all_dims_present() {
            return Err(BoardCode::MissingDim);
        }
        self.state = BoardState::Frozen;
        Ok(())
    }

    /// 发布：Frozen → Published；Draft 直接发布被拒。
    pub fn publish(&mut self) -> Result<(), BoardCode> {
        if self.state == BoardState::Draft {
            return Err(BoardCode::PublishBeforeFreeze);
        }
        if self.state == BoardState::Published {
            return Err(BoardCode::StateRegress);
        }
        self.state = BoardState::Published;
        Ok(())
    }
}

// ── 缺波注入核 ────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
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

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(BoardCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造一份缺第 `drop` 波（1..=6）的注册册（其余波齐、五维齐）。
pub fn board_missing_wave(drop: usize) -> CaliberBoard {
    let mut b = CaliberBoard::new();
    let mut n = 1usize;
    while n <= WAVE_COUNT {
        if n != drop {
            let _ = b.register(wave_label(n), DimKind::Wave);
        }
        n += 1;
    }
    // 补齐其余四维各一条，使缺波成为唯一缺陷。
    let _ = b.register("B01", DimKind::Batch);
    let _ = b.register("战线一", DimKind::Front);
    let _ = b.register("ltp_read", DimKind::Module);
    let _ = b.register("J1", DimKind::Criterion);
    b
}

/// 执行缺波注入矩阵：6 波各缺一次 × 10 轮，返回账本与逐波检出数。
pub fn run_missing_wave_matrix() -> (InjectLedger, [u32; WAVE_COUNT]) {
    let mut ledger = InjectLedger::zero();
    let mut per_wave = [0u32; WAVE_COUNT];
    let mut w = 0usize;
    while w < WAVE_COUNT {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut b = board_missing_wave(w + 1);
            let caught = b.freeze().err() == Some(BoardCode::MissingWave);
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_wave.get_mut(w) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        w += 1;
    }
    (ledger, per_wave)
}

/// 构造完整总纲：六波 + 批/战线/模块/判据代表条目，并冻结。
pub fn build_full_board() -> CaliberBoard {
    let mut b = CaliberBoard::new();
    let mut n = 1usize;
    while n <= WAVE_COUNT {
        let _ = b.register(wave_label(n), DimKind::Wave);
        n += 1;
    }
    let _ = b.register("B01", DimKind::Batch);
    let _ = b.register("战线一", DimKind::Front);
    let mut m = 0usize;
    while m < MODULE_FAMILY.len() {
        if let Some(name) = MODULE_FAMILY.get(m) {
            let _ = b.register(name, DimKind::Module);
        }
        m += 1;
    }
    let _ = b.register("J1", DimKind::Criterion);
    let _ = b.freeze();
    b
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越权/状态非法族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺员/未注册族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复注册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏句式族"),
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

/// 总纲指纹：五维计数 + 生命期状态逐字节入哈希。
pub fn board_fingerprint(b: &CaliberBoard) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < DimKind::ALL.len() {
        if let Some(k) = DimKind::ALL.get(i) {
            let n = b.count_of(*k);
            bytes.push((n & 0xFF) as u8);
        }
        i += 1;
    }
    bytes.push(match b.state() {
        BoardState::Draft => 0,
        BoardState::Frozen => 1,
        BoardState::Published => 2,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10001 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10001_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10001");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 五维口径冻结：成员数逐一与冻结值一致
    let frozen_ok = DimKind::Wave.frozen_members() == WAVE_COUNT
        && DimKind::Batch.frozen_members() == BATCH_COUNT
        && DimKind::Front.frozen_members() == FRONT_COUNT
        && DimKind::Module.frozen_members() == MODULE_FAMILY.len()
        && DimKind::Criterion.frozen_members() == CRITERION_COUNT;
    s.add("口径冻结：五维成员数与冻结值逐一相等", frozen_ok,
          "维度成员数是报表分母口径，必须成文冻结");

    // 3. 波名解析：01..=06 全过
    let mut parse_ok = true;
    let mut n = 1usize;
    while n <= WAVE_COUNT {
        if parse_wave_label(wave_label(n)) != Ok(n) {
            parse_ok = false;
        }
        n += 1;
    }
    s.add("波名解析：wave-01..06 逐字命中", parse_ok,
          "六波规范名必须全部解析回正确波序");

    // 4. 波名越界/坏句式拒绝
    let reject_ok = parse_wave_label("wave-00") == Err(BoardCode::WaveOutOfRange)
        && parse_wave_label("wave-07") == Err(BoardCode::WaveOutOfRange)
        && parse_wave_label("wave-1") == Err(BoardCode::BadWaveLabel)
        && parse_wave_label("wav-01") == Err(BoardCode::BadWaveLabel);
    s.add("波名拒绝：越界与坏句式逐类拒绝", reject_ok,
          "wave-00/07 回越界，句式破格回坏句式");

    // 5. 完整总纲：六波逐波入表且冻结成功
    let full = build_full_board();
    s.add("零缺波：完整总纲六波齐且冻结成功",
          full.waves_complete() && full.state() == BoardState::Frozen,
          "完整名册冻结必须成功，缺任何一波都不得冻结");

    // 6. 缺波注入矩阵：6 波各缺一次 × 10/10 检出
    let (ledger, per_wave) = run_missing_wave_matrix();
    let mut all_caught = true;
    let mut w = 0usize;
    while w < per_wave.len() {
        if let Some(c) = per_wave.get(w) {
            if *c != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        w += 1;
    }
    s.add("缺波注入：逐波各缺 10/10 检出", all_caught,
          "六波逐一抽走后冻结必须 10/10 次报缺波");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == WAVE_COUNT as u32 * INJECT_ROUNDS,
          "60 次注入必须 60 次检出、0 逃逸");

    // 8. 重复注册拒绝
    let mut dup_board = CaliberBoard::new();
    let a = dup_board.register("wave-01", DimKind::Wave);
    let b = dup_board.register("wave-01", DimKind::Wave);
    s.add("重复拒绝：同名二次注册被拒",
          a.is_ok() && b.err() == Some(BoardCode::DuplicateLabel),
          "重复名必须显性拒绝，不许静默覆盖");

    // 9. 未冻结发布拒绝
    let mut draft = CaliberBoard::new();
    let _ = draft.register("wave-01", DimKind::Wave);
    s.add("越闸拦截：Draft 直接发布被拒",
          draft.publish().err() == Some(BoardCode::PublishBeforeFreeze),
          "未冻结先发布视为越闸，必须拦截");

    // 10. 冻结后注册拒绝、发布只进不退
    let mut flow = build_full_board();
    let reg_after = flow.register("extra", DimKind::Wave);
    let pub_ok = flow.publish().is_ok();
    let pub_again = flow.publish();
    s.add("状态机：冻结后闭册、发布只进不退",
          reg_after.err() == Some(BoardCode::RegisterAfterFreeze)
              && pub_ok
              && pub_again.err() == Some(BoardCode::StateRegress),
          "Draft→Frozen→Published 单向流，任何回退/越位都拒绝");

    // 11. 五维有缺拒绝冻结
    let mut thin = CaliberBoard::new();
    let _ = thin.register("wave-01", DimKind::Wave);
    let thin_wave = thin.waves_complete();
    let thin_freeze = thin.freeze();
    s.add("维度有缺：单波名册冻结被拒",
          !thin_wave && thin_freeze.err() == Some(BoardCode::MissingWave),
          "五维不齐或波缺员时冻结必须失败");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑指纹一致
    let fp_a = board_fingerprint(&build_full_board());
    let fp_b = board_fingerprint(&build_full_board());
    s.add("双跑一致：总纲指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 14. 文案闭集
    let text_ok = BoardCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < BoardCode::ALL.len() {
        let mut j = i + 1;
        while j < BoardCode::ALL.len() {
            if let (Some(a), Some(b)) = (BoardCode::ALL.get(i), BoardCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 发布态计数稳定：发布不改变五维计数
    let mut pubd = build_full_board();
    let cnt_before = pubd.count_of(DimKind::Wave);
    let _ = pubd.publish();
    s.add("发布无副作用：发布后波计数不变",
          pubd.count_of(DimKind::Wave) == cnt_before,
          "发布只推进状态位，计数必须保持");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("总纲指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_labels_parse() {
        let mut n = 1usize;
        while n <= 6 {
            assert_eq!(parse_wave_label(wave_label(n)), Ok(n));
            n += 1;
        }
    }

    #[test]
    fn bad_wave_labels_rejected() {
        assert_eq!(parse_wave_label("wave-07"), Err(BoardCode::WaveOutOfRange));
        assert_eq!(parse_wave_label("wave-00"), Err(BoardCode::WaveOutOfRange));
        assert_eq!(parse_wave_label("wv-01"), Err(BoardCode::BadWaveLabel));
    }

    #[test]
    fn full_board_freezes_and_publishes() {
        let mut b = build_full_board();
        assert_eq!(b.state(), BoardState::Frozen);
        assert!(b.publish().is_ok());
        assert_eq!(b.state(), BoardState::Published);
    }

    #[test]
    fn missing_wave_detected_per_wave() {
        let mut drop = 1usize;
        while drop <= 6 {
            let mut b = board_missing_wave(drop);
            assert_eq!(b.freeze(), Err(BoardCode::MissingWave));
            drop += 1;
        }
    }

    #[test]
    fn duplicate_label_rejected() {
        let mut b = CaliberBoard::new();
        assert!(b.register("wave-01", DimKind::Wave).is_ok());
        assert_eq!(b.register("wave-01", DimKind::Wave), Err(BoardCode::DuplicateLabel));
    }

    #[test]
    fn publish_before_freeze_rejected() {
        let mut b = CaliberBoard::new();
        assert_eq!(b.publish(), Err(BoardCode::PublishBeforeFreeze));
    }

    #[test]
    fn register_after_freeze_rejected() {
        let mut b = build_full_board();
        assert_eq!(b.register("extra", DimKind::Wave), Err(BoardCode::RegisterAfterFreeze));
    }

    #[test]
    fn missing_wave_matrix_green() {
        let (l, per) = run_missing_wave_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, (WAVE_COUNT as u32) * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(board_fingerprint(&build_full_board()), board_fingerprint(&build_full_board()));
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(EEXIST));
        assert_eq!(linux_neg(EPERM), -EPERM);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10001_checks_green() {
        let set = run_unx10001_checks();
        assert!(set.all_passed(), "UNX-F10001 red: {}", set.red_items_text());
    }
}
