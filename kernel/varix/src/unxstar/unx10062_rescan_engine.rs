//! UNX-F10062 账目全量重扫器（批册+深化册） · 0x91f40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10062（M 型收口线：双册全量重扫执行器）。
//! 锚点：UNX-F10063-J1 号段连续核（F9601–F10080 零空洞 10/10 次），空洞
//!       10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91f40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RescanCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 调度器 `RescanScheduler`：批册遍 + 深化册遍两遍扫描队列，
//!     队列必须覆盖 1..=48 全册，双读同册/漏册/乱序显性拒绝；
//!   - 账本 `RescanLedger`：双遍逐册入账（批册遍/深化册遍分账），
//!     两遍各 24 册、逃逸恒 0、只增不减；
//!   - 连续核 `ContinuityWalker`：扫得号位序列必须覆盖 F9601..=F10080
//!     零空洞，空洞定位到首缺号；
//!   - 解析器 `BookRef`：册引用 "b12"/"d12" 解析（批册/深化册），
//!     非法键显性拒绝；
//!   - 注入核 `BadRescanCase`：空册引用 / 非法册键 / 漏册提交 /
//!     双读同册 / 号位空洞 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10062 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91f40;

/// 段窗上界（下一卡 0x91f80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91f80;

/// 半册数（批册 24 / 深化册 24）。
pub const HALF_BOOKS: usize = 24;

/// 册总数（双遍合计 48 册）。
pub const BOOK_COUNT: usize = 48;

/// 号位范围起点（F9601）。
pub const RANGE_LO: u32 = 9601;

/// 号位范围终点（F10080）。
pub const RANGE_HI: u32 = 10080;

/// 每册号位数（480 / 48 = 10）。
pub const PER_BOOK: u32 = 10;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EIO: i32 = 5;
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RescanCode {
    /// 非法册键（既非 bNN 也非 dNN）
    BadBookKey,
    /// 册号越界（册号出 1..=24）
    BookRange,
    /// 空册引用
    EmptyRef,
    /// 漏册提交（两遍未覆盖全 48 册即收账）
    MissingBook,
    /// 双读同册（同遍同册二次提交）
    DuplicateScan,
    /// 号位空洞（扫得号位序列有缺号）
    ContinuityHole,
    /// 账本倒退（已扫计数回退）
    LedgerRegress,
    /// 遍次错位（未走批册遍先收深化册遍）
    PassMisorder,
    /// 未扫完收账
    PrematureClose,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl RescanCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RescanCode::BadBookKey => "非法册键被拒：册引用只允许 bNN/dNN；请核对册键格式",
            RescanCode::BookRange => "册号越界被拒：册号只允许 1..=24；请核对册号",
            RescanCode::EmptyRef => "空册引用被拒：册引用不得为空；请补册键",
            RescanCode::MissingBook => "漏册提交被拒：两遍未覆盖 48 册不得收账；请补扫漏册",
            RescanCode::DuplicateScan => "双读同册被拒：同遍同册只扫一次；请查重复提交",
            RescanCode::ContinuityHole => "号位空洞被拒：号位序列必须零空洞；请按首缺号补扫",
            RescanCode::LedgerRegress => "账本倒退被拒：已扫计数只增不减；请排查回滚",
            RescanCode::PassMisorder => "遍次错位被拒：先批册遍后深化册遍；请按遍序重扫",
            RescanCode::PrematureClose => "未扫完收账被拒：24 册未齐不得关遍；请补扫",
            RescanCode::ParseFailure => "解析失败：册行必须恰为 key|kind 两段；请补齐字段",
            RescanCode::StateRegress => "状态倒退被拒：Idle→BatchPass→DeepPass 单向；请排查回滚",
            RescanCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91f40..=0x91f80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RescanCode; 12] = [
        RescanCode::BadBookKey,
        RescanCode::BookRange,
        RescanCode::EmptyRef,
        RescanCode::MissingBook,
        RescanCode::DuplicateScan,
        RescanCode::ContinuityHole,
        RescanCode::LedgerRegress,
        RescanCode::PassMisorder,
        RescanCode::PrematureClose,
        RescanCode::ParseFailure,
        RescanCode::StateRegress,
        RescanCode::SegmentViolation,
    ];
}

// ── 册引用解析器 ──────────────────────────────────────────────────────
/// 遍次（批册遍 / 深化册遍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanPass {
    /// 批册遍（bNN）
    BatchPass,
    /// 深化册遍（dNN）
    DeepPass,
}

impl ScanPass {
    /// 遍次下标（0=批册，1=深化）。
    pub fn index(self) -> usize {
        match self {
            ScanPass::BatchPass => 0,
            ScanPass::DeepPass => 1,
        }
    }
}

/// 册引用：解析 "b12" / "d12" → 遍次 + 册号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BookRef {
    /// 遍次
    pub pass: ScanPass,
    /// 册号 1..=24
    pub book_no: u32,
}

/// 解析册引用：bNN/dNN，NN 必须 1..=24 且无前导零。
pub fn parse_book_ref(s: &str) -> Result<BookRef, RescanCode> {
    if s.is_empty() {
        return Err(RescanCode::EmptyRef);
    }
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes.len() > 3 {
        return Err(RescanCode::BadBookKey);
    }
    let pass = match bytes.first().copied().unwrap_or(b'?') {
        b'b' => ScanPass::BatchPass,
        b'd' => ScanPass::DeepPass,
        _ => return Err(RescanCode::BadBookKey),
    };
    let mut n: u32 = 0;
    let mut seen = 0usize;
    let mut bi = 1usize;
    while bi < bytes.len() {
        let b = bytes.get(bi).copied().unwrap_or(b'?');
        if !b.is_ascii_digit() {
            return Err(RescanCode::BadBookKey);
        }
        if seen == 0 && b == b'0' {
            return Err(RescanCode::ParseFailure);
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
        seen += 1;
        bi += 1;
    }
    if n == 0 || n as usize > HALF_BOOKS {
        return Err(RescanCode::BookRange);
    }
    Ok(BookRef { pass, book_no: n })
}

/// 全局册号：遍次偏移 24 + 册内册号（批册 1..=24，深化册 25..=48）。
pub fn global_book_of(pass: ScanPass, book_no: u32) -> u32 {
    (pass.index() as u32).saturating_mul(HALF_BOOKS as u32).saturating_add(book_no)
}

/// 全局册首号位：全局册 g（1..=48）→ F9601 + (g-1)*10。
pub fn book_first_no(global_book: u32) -> u32 {
    RANGE_LO + (global_book.saturating_sub(1)).saturating_mul(PER_BOOK)
}

/// 全局册尾号位：首号 + 9。
pub fn book_last_no(global_book: u32) -> u32 {
    book_first_no(global_book).saturating_add(PER_BOOK - 1)
}

// ── 重扫调度器 ────────────────────────────────────────────────────────
/// 状态机：Idle → BatchPass → DeepPass 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RescanState {
    /// 空闲（未开扫）
    Idle,
    /// 批册遍进行中
    BatchPass,
    /// 深化册遍进行中
    DeepPass,
}

/// 重扫调度器：两遍队列 + 逐册位图 + 号位连续核。
#[derive(Clone, Debug)]
pub struct RescanScheduler {
    /// 两遍位图（[批册, 深化]，true=已扫）
    pub passes: [[bool; HALF_BOOKS]; 2],
    /// 两遍已扫册数
    pub scanned: [usize; 2],
    /// 扫得号位序列（按扫描序，收账时核连续）
    pub hits: Vec<u32>,
    /// 当前状态
    pub state: RescanState,
}

impl RescanScheduler {
    /// 构造空闲调度器。
    pub fn new() -> RescanScheduler {
        RescanScheduler {
            passes: [[false; HALF_BOOKS]; 2],
            scanned: [0, 0],
            hits: Vec::new(),
            state: RescanState::Idle,
        }
    }

    /// 开批册遍。
    pub fn begin(&mut self) -> Result<(), RescanCode> {
        if self.state != RescanState::Idle {
            return Err(RescanCode::StateRegress);
        }
        self.state = RescanState::BatchPass;
        Ok(())
    }

    /// 批册遍收账、转深化册遍：24 册齐才可关遍。
    pub fn close_batch_pass(&mut self) -> Result<(), RescanCode> {
        if self.state != RescanState::BatchPass {
            return Err(RescanCode::PassMisorder);
        }
        if self.scanned.get(0).copied().unwrap_or(0) < HALF_BOOKS {
            return Err(RescanCode::PrematureClose);
        }
        self.state = RescanState::DeepPass;
        Ok(())
    }

    /// 提交一册：当前遍判必检、册号必检、重复必拒、号位序列追加。
    pub fn submit(&mut self, r: BookRef) -> Result<(), RescanCode> {
        let want = match self.state {
            RescanState::BatchPass => ScanPass::BatchPass,
            RescanState::DeepPass => ScanPass::DeepPass,
            RescanState::Idle => return Err(RescanCode::StateRegress),
        };
        if r.pass != want {
            return Err(RescanCode::PassMisorder);
        }
        let idx = r.book_no as usize - 1;
        let pi = want.index();
        let already = self
            .passes
            .get(pi)
            .and_then(|row| row.get(idx))
            .copied()
            .unwrap_or(true);
        if already {
            return Err(RescanCode::DuplicateScan);
        }
        if let Some(row) = self.passes.get_mut(pi) {
            if let Some(slot) = row.get_mut(idx) {
                *slot = true;
            }
        }
        if let Some(slot) = self.scanned.get_mut(want.index()) {
            *slot = slot.saturating_add(1);
        }
        // 号位入序列：全局册铺满 9601..=10080（批册前 24 册、深化册后 24 册）。
        let g = global_book_of(r.pass, r.book_no);
        let lo = book_first_no(g);
        let mut n = lo;
        while n <= book_last_no(g) {
            self.hits.push(n);
            n = n.saturating_add(1);
        }
        Ok(())
    }

    /// 收账：两遍全 48 册齐才可收；否则 MissingBook。
    pub fn finalize(&self) -> Result<(), RescanCode> {
        if self.state != RescanState::DeepPass {
            return Err(RescanCode::PassMisorder);
        }
        if self.scanned.get(0).copied().unwrap_or(0) < HALF_BOOKS
            || self.scanned.get(1).copied().unwrap_or(0) < HALF_BOOKS
        {
            return Err(RescanCode::MissingBook);
        }
        match first_hole(&self.hits) {
            None => Ok(()),
            Some(_) => Err(RescanCode::ContinuityHole),
        }
    }
}

/// 号段连续核：扫得号位序列必须覆盖 9601..=10080 零空洞；返回首缺号。
pub fn first_hole(hits: &[u32]) -> Option<u32> {
    // 期望序列与扫描序无关：按号位计数，缺号即空洞。
    let total = (RANGE_HI - RANGE_LO + 1) as usize;
    if hits.len() != total {
        // 数量不等时找首缺：逐号查在位性。
        let mut n = RANGE_LO;
        while n <= RANGE_HI {
            if !hits.contains(&n) {
                return Some(n);
            }
            n += 1;
        }
        return Some(RANGE_LO);
    }
    let mut seen = vec![false; total];
    for h in hits.iter() {
        if *h >= RANGE_LO && *h <= RANGE_HI {
            let idx = (*h - RANGE_LO) as usize;
            if let Some(slot) = seen.get_mut(idx) {
                *slot = true;
            }
        }
    }
    let mut i = 0usize;
    while i < total {
        if !seen.get(i).copied().unwrap_or(true) {
            return Some(RANGE_LO + i as u32);
        }
        i += 1;
    }
    None
}

/// 标准重扫：先批册遍 b1..b24，关遍后深化册遍 d1..d24，收账。
pub fn rescan_standard(s: &mut RescanScheduler) -> Result<(), RescanCode> {
    s.begin()?;
    let mut n = 1u32;
    while n as usize <= HALF_BOOKS {
        let r = parse_book_ref(&format!("b{}", n))?;
        s.submit(r)?;
        n += 1;
    }
    s.close_batch_pass()?;
    let mut n = 1u32;
    while n as usize <= HALF_BOOKS {
        let r = parse_book_ref(&format!("d{}", n))?;
        s.submit(r)?;
        n += 1;
    }
    s.finalize()
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏重扫样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRescanCase {
    /// 非法册键（"x1"/"b0"/"b25"）
    BadBookKey,
    /// 空册引用
    EmptyRef,
    /// 漏册收账（23 册即收）
    MissingBook,
    /// 双读同册
    DuplicateScan,
    /// 号位空洞（漏提交一册导致缺号）
    ContinuityHole,
}

impl BadRescanCase {
    /// 闭集全量。
    pub const ALL: [BadRescanCase; 5] = [
        BadRescanCase::BadBookKey,
        BadRescanCase::EmptyRef,
        BadRescanCase::MissingBook,
        BadRescanCase::DuplicateScan,
        BadRescanCase::ContinuityHole,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadRescanCase::BadBookKey => {
                parse_book_ref("x1").is_err()
                    && parse_book_ref("b0").is_err()
                    && parse_book_ref("b25").is_err()
                    && parse_book_ref("b05").is_err()
            }
            BadRescanCase::EmptyRef => parse_book_ref("").is_err(),
            BadRescanCase::MissingBook => {
                let mut s = RescanScheduler::new();
                if s.begin().is_err() {
                    return false;
                }
                let mut n = 1u32;
                while n < HALF_BOOKS as u32 {
                    let r = match parse_book_ref(&format!("b{}", n)) {
                        Ok(r) => r,
                        Err(_) => return false,
                    };
                    if s.submit(r).is_err() {
                        return false;
                    }
                    n += 1;
                }
                s.close_batch_pass().is_err()
            }
            BadRescanCase::DuplicateScan => {
                let mut s = RescanScheduler::new();
                if s.begin().is_err() {
                    return false;
                }
                let r = match parse_book_ref("b1") {
                    Ok(r) => r,
                    Err(_) => return false,
                };
                if s.submit(r).is_err() {
                    return false;
                }
                s.submit(r).is_err()
            }
            BadRescanCase::ContinuityHole => {
                // 全量号位序列剔除全局册 24（缺 F9831..F9840），
                // 连续核必须定位首缺号 F9831；全量序列必须零空洞。
                let mut full: Vec<u32> = Vec::new();
                let mut n = RANGE_LO;
                while n <= RANGE_HI {
                    full.push(n);
                    n += 1;
                }
                let pruned: Vec<u32> =
                    full.iter().copied().filter(|x| *x < 9831 || *x > 9840).collect();
                first_hole(&full).is_none()
                    && first_hole(&pruned) == Some(9831)
                    && pruned.len() == 470
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回逐类检出数与逃逸数。
pub fn run_rescan_injection() -> (u32, [u32; 5]) {
    let mut caught_total = 0u32;
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRescanCase::ALL.len() {
        if let Some(case) = BadRescanCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if case.inject() {
                    caught_total = caught_total.saturating_add(1);
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (caught_total, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK；非法册键/空引用族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；漏册族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；重扫 IO 逃逸族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；册号越界族"),
    ("EOVERFLOW", -75, 28, "Windows ERROR_OUT_OF_PAPER；号位溢出族"),
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

/// 规范语料双跑指纹：双遍重扫全链逐字节入哈希（含空洞探测）。
pub fn corpus_fingerprint() -> u32 {
    let mut s = RescanScheduler::new();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match rescan_standard(&mut s) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match first_hole(&s.hits) {
        None => 3,
        Some(_) => 4,
    });
    bytes.extend_from_slice(&(s.hits.len() as u32).to_le_bytes());
    bytes.push(s.state as u8);
    // 空洞注入探测：剔除 b24 的号位序列，首缺号入哈希。
    let mut short: Vec<u32> = Vec::new();
    for h in s.hits.iter() {
        if *h < book_first_no(HALF_BOOKS as u32) {
            short.push(*h);
        }
    }
    if let Some(h) = first_hole(&short) {
        bytes.extend_from_slice(&h.to_le_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10062 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10062_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10062");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91f40..=0x91f80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准重扫全链成功
    let mut sc = RescanScheduler::new();
    let std_ok = rescan_standard(&mut sc).is_ok();
    s.add("重扫全链：批册遍+深化册遍 48 册收账成功", std_ok,
          "b1..b24 后 d1..d24，finalize 零空洞通过");

    // 3. 册号位映射：边界命中（全局册 1 与 48 首尾）
    let map_ok = book_first_no(1) == 9601
        && book_last_no(1) == 9610
        && book_first_no(24) == 9831
        && book_first_no(48) == 10071
        && book_last_no(48) == 10080;
    s.add("册号位映射：册首/册尾可复算", map_ok,
          "全局册 g 首号 = 9601+(g-1)*10，尾号 = 首号+9；48 册铺满全段");

    // 4. 解析器：合法键命中 + 非法键拒绝
    let parse_ok = parse_book_ref("b1") == Ok(BookRef { pass: ScanPass::BatchPass, book_no: 1 })
        && parse_book_ref("d24") == Ok(BookRef { pass: ScanPass::DeepPass, book_no: 24 })
        && parse_book_ref("x1").is_err()
        && parse_book_ref("b0").is_err()
        && parse_book_ref("b25").is_err()
        && parse_book_ref("b05").is_err()
        && parse_book_ref("").is_err();
    s.add("册引用解析：bNN/dNN 闭集、非法键全拒", parse_ok,
          "解析器拒绝 x 键、0 号、25 号与前导零");

    // 5. 双读同册拦截
    let mut dup = RescanScheduler::new();
    let _ = dup.begin();
    let r1 = parse_book_ref("b3").is_ok();
    if let Ok(r) = parse_book_ref("b3") {
        let _ = dup.submit(r);
    }
    let dup_ok = r1 && matches!(parse_book_ref("b3"), Ok(r) if dup.submit(r) == Err(RescanCode::DuplicateScan));
    s.add("双读同册：同遍同册二次提交被拒", dup_ok,
          "一册一扫，重复提交显性拒绝");

    // 6. 遍次错位拦截：Idle 提交 / 深化遍提交批册
    let mut mo = RescanScheduler::new();
    let idle_rej = match parse_book_ref("b1") {
        Ok(r) => mo.submit(r) == Err(RescanCode::StateRegress),
        Err(_) => false,
    };
    s.add("遍次错位：Idle 提交被拒", idle_rej,
          "未开扫不得提交；状态机 Idle→BatchPass→DeepPass 单向");

    // 7. 漏册关遍拦截 + 齐册关遍成功
    let mut part = RescanScheduler::new();
    let _ = part.begin();
    let mut n = 1u32;
    while n < HALF_BOOKS as u32 {
        if let Ok(r) = parse_book_ref(&format!("b{}", n)) {
            let _ = part.submit(r);
        }
        n += 1;
    }
    let part_rej = part.close_batch_pass() == Err(RescanCode::PrematureClose);
    let mut full = RescanScheduler::new();
    let _ = full.begin();
    let mut n = 1u32;
    while n as usize <= HALF_BOOKS {
        if let Ok(r) = parse_book_ref(&format!("b{}", n)) {
            let _ = full.submit(r);
        }
        n += 1;
    }
    let full_ok = full.close_batch_pass().is_ok() && full.state == RescanState::DeepPass;
    s.add("关遍闸：24 册未齐拒绝、齐册转深化遍", part_rej && full_ok,
          "每遍 24 册齐才可关遍；关遍单向");

    // 8. 号段连续核：全扫零空洞
    let mut hole_free = RescanScheduler::new();
    let _ = rescan_standard(&mut hole_free);
    s.add("号段连续核：F9601–F10080 零空洞", first_hole(&hole_free.hits).is_none()
              && hole_free.hits.len() == 480,
          "480 号位逐位在案，缺一即空洞");

    // 9. 空洞检出：剔一册后首缺号定位正确
    let mut short_hits: Vec<u32> = Vec::new();
    for h in hole_free.hits.iter() {
        if *h < book_first_no(7) || *h > book_last_no(7) {
            short_hits.push(*h);
        }
    }
    let hole_ok = first_hole(&short_hits) == Some(book_first_no(7))
        && short_hits.len() == 470;
    s.add("空洞检出：剔全局册 7 后首缺号=F9661", hole_ok,
          "空洞必须定位到首缺号（全局册 7 首 = 9601+6*10 = 9661）");

    // 10. 漏册收账拦截（finalize 面）
    let mut mb = RescanScheduler::new();
    if let Ok(()) = rescan_standard(&mut mb) {
        // 复制成缺一深化册的场景再 finalize。
        let mut miss = RescanScheduler::new();
        let _ = miss.begin();
        let mut n = 1u32;
        while n as usize <= HALF_BOOKS {
            if let Ok(r) = parse_book_ref(&format!("b{}", n)) {
                let _ = miss.submit(r);
            }
            n += 1;
        }
        let _ = miss.close_batch_pass();
        let mut n = 1u32;
        while n < HALF_BOOKS as u32 {
            if let Ok(r) = parse_book_ref(&format!("d{}", n)) {
                let _ = miss.submit(r);
            }
            n += 1;
        }
        let miss_rej = miss.finalize() == Err(RescanCode::MissingBook);
        s.add("漏册收账：深化册缺一册 finalize 拒绝", miss_rej,
              "两遍 48 册齐是收账硬前置");
    } else {
        s.add("漏册收账：深化册缺一册 finalize 拒绝", false,
              "标准重扫失败，实现异常");
    }

    // 11. 注入核：五类坏样本全检出
    let (caught, per_class) = run_rescan_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "非法键/空引用/漏册/双读/号位空洞全被显式拒绝");

    // 12. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮全检出逃逸=0", caught == 5 * INJECT_ROUNDS,
          "注入核账守恒：50 次注入 50 次检出、0 逃逸");

    // 13. 文案闭集：非空
    let text_ok = RescanCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < RescanCode::ALL.len() {
        let mut j = i + 1;
        while j < RescanCode::ALL.len() {
            if let (Some(x), Some(y)) = (RescanCode::ALL.get(i), RescanCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = RescanCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EIO)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 17. 扫描序确定性：同一册序列两次重扫号位序列逐位一致
    let mut a = RescanScheduler::new();
    let mut b = RescanScheduler::new();
    let _ = rescan_standard(&mut a);
    let _ = rescan_standard(&mut b);
    let seq_ok = a.hits == b.hits;
    s.add("确定性：两次重扫号位序列逐位一致", seq_ok,
          "重扫算法可复算：同输入同序列");

    // 动态明细（≤8）：重扫快照 + 指纹快照
    let snap_text = format!("批册 {} / 深化 {} / 号位 {}", sc.scanned[0], sc.scanned[1], sc.hits.len());
    s.add_dyn("重扫快照", sc.hits.len() == 480, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rescan_ok() {
        let mut s = RescanScheduler::new();
        assert!(rescan_standard(&mut s).is_ok());
        assert_eq!(s.hits.len(), 480);
        assert!(first_hole(&s.hits).is_none());
    }

    #[test]
    fn bad_keys_rejected() {
        assert!(parse_book_ref("x1").is_err());
        assert!(parse_book_ref("b0").is_err());
        assert!(parse_book_ref("b25").is_err());
        assert!(parse_book_ref("b05").is_err());
        assert!(parse_book_ref("").is_err());
    }

    #[test]
    fn good_keys_parsed() {
        assert_eq!(parse_book_ref("b1"), Ok(BookRef { pass: ScanPass::BatchPass, book_no: 1 }));
        assert_eq!(parse_book_ref("d24"), Ok(BookRef { pass: ScanPass::DeepPass, book_no: 24 }));
    }

    #[test]
    fn duplicate_scan_rejected() {
        let mut s = RescanScheduler::new();
        let _ = s.begin();
        let r = parse_book_ref("b1").ok().unwrap_or(BookRef { pass: ScanPass::BatchPass, book_no: 1 });
        assert!(s.submit(r).is_ok());
        assert_eq!(s.submit(r), Err(RescanCode::DuplicateScan));
    }

    #[test]
    fn premature_close_rejected() {
        let mut s = RescanScheduler::new();
        let _ = s.begin();
        let r = parse_book_ref("b1").ok().unwrap_or(BookRef { pass: ScanPass::BatchPass, book_no: 1 });
        let _ = s.submit(r);
        assert_eq!(s.close_batch_pass(), Err(RescanCode::PrematureClose));
    }

    #[test]
    fn hole_located() {
        let mut s = RescanScheduler::new();
        let _ = rescan_standard(&mut s);
        let mut short: Vec<u32> = Vec::new();
        for h in s.hits.iter() {
            if *h < book_first_no(7) || *h > book_last_no(7) {
                short.push(*h);
            }
        }
        assert_eq!(first_hole(&short), Some(9661));
    }

    #[test]
    fn book_bounds() {
        assert_eq!(book_first_no(1), 9601);
        assert_eq!(book_last_no(24), 9840);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (caught, per) = run_rescan_injection();
        assert_eq!(caught, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EIO));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn rescan_code_text_complete() {
        for c in RescanCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10062_checks_green() {
        let s = run_unx10062_checks();
        assert!(s.all_passed(), "UNX-F10062 red: {}", s.red_items_text());
    }
}
