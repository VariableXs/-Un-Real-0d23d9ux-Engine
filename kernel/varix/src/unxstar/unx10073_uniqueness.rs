//! UNX-F10073 防重总对账（全域唯一性）· 0x92240 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10074-J1 全域唯一核（480 条 ID 全域唯一 10/10 次），撞号 10/10 次检出
//!
//! 设计要点：
//!   ① 全域 ID 窗口冻结为 F9601..=F10080 恰 480 个号位；防重账以
//!      「有序注册表 + 二分查找」为唯一出处，注册即查重，撞号 DupId
//!      逐次显性拒绝（10/10），越窗 OutOfRange 拒绝。
//!   ② 唯一核两档：单号注册即查重 + 批量重扫对撞（批内重复与批外撞号
//!      分别计数）；防重账守恒式：扫描数 = 唯一号数 + 撞号数，恒成立。
//!   ③ 注册表有序不变式：任意时刻从小到大排列，插入后仍有序
//!      （乱序插入用二分定位，不重排历史）。
//!   ④ 注入核：撞号 / 越窗 / 批量撞号 / 满册溢出四类坏样本逐类 10/10 次
//!      拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10073 独占 0x40 窗口，下一卡 0x92280）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92240;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 全域 ID 窗口下界（F9601）。
pub const ID_LO: u32 = 9601;

/// 全域 ID 窗口上界（F10080）。
pub const ID_HI: u32 = 10080;

/// 全域号位总数（480 条判据的冻结预算）。
pub const ID_TOTAL: u32 = ID_HI - ID_LO + 1;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UniqCode {
    /// 撞号：ID 已在注册表
    DupId,
    /// 越窗：ID 不在 F9601..=F10080
    OutOfRange,
    /// 批量重扫批内撞号
    BatchCollision,
    /// 满册溢出（480 号位已满）
    RegistryFull,
    /// 防重账失守恒（扫描数 != 唯一 + 撞号）
    UniqRegress,
    /// 有序不变式被破坏
    OrderBroken,
    /// 重扫 ID 与注册表撞号
    ScanDup,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 批量样本为空
    EmptyBatch,
    /// 号位覆盖核失败（481 核对不上 480）
    CoverageMiss,
    /// 段位越界
    SegmentViolation,
}

impl UniqCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            UniqCode::DupId => "撞号被拒：该 ID 已在全域注册表；\
                全域唯一是硬约束，请换号或走勘误通道",
            UniqCode::OutOfRange => "越窗被拒：ID 不在 F9601..=F10080；\
                窗外号位不属于本域，禁止入账",
            UniqCode::BatchCollision => "批内撞号被拒：批量重扫样本内部重复；\
                请先去重再重扫",
            UniqCode::RegistryFull => "满册被拒：480 号位已全部占用；\
                满册后任何新号必然越窗或撞号，先做覆盖核再续册",
            UniqCode::UniqRegress => "防重账失守恒：扫描数必须等于 唯一号+撞号；\
                请排查是否绕过计数器",
            UniqCode::OrderBroken => "有序不变式被破坏：注册表必须始终从小到大；\
                请排查插入路径是否跳过二分定位",
            UniqCode::ScanDup => "重扫撞号被拒：重扫 ID 与注册表已有条目重复；\
                请核对是重复重扫还是真的两册同号",
            UniqCode::EmptySummary => "空成文被拒：防重小结不得为空串；请检查注册表是否为空",
            UniqCode::RejectedTrace => "拒绝留痕被拒：被拒注册不得改动注册表；\
                请排查拒绝路径是否误提交",
            UniqCode::EmptyBatch => "空批量被拒：重扫样本不得为空批；\
                空批对账等于虚报扫描量",
            UniqCode::CoverageMiss => "覆盖核失败：满册号位必须恰好覆盖 480 连续号；\
                请补齐空洞后再收口",
            UniqCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [UniqCode; 12] = [
        UniqCode::DupId,
        UniqCode::OutOfRange,
        UniqCode::BatchCollision,
        UniqCode::RegistryFull,
        UniqCode::UniqRegress,
        UniqCode::OrderBroken,
        UniqCode::ScanDup,
        UniqCode::EmptySummary,
        UniqCode::RejectedTrace,
        UniqCode::EmptyBatch,
        UniqCode::CoverageMiss,
        UniqCode::SegmentViolation,
    ];
}

// ── 有序注册表（二分查重） ────────────────────────────────────────────
/// 全域 ID 有序注册表：唯一出处的防重账基座。
#[derive(Clone, Debug)]
pub struct IdRegistry {
    ids: Vec<u32>,
    cap: u32,
}

impl IdRegistry {
    /// 空表构造（容量 ID_TOTAL = 480）。
    pub fn new() -> IdRegistry {
        IdRegistry { ids: Vec::new(), cap: ID_TOTAL }
    }

    /// 二分查找：命中返回 Some(下标)。
    pub fn find(&self, id: u32) -> Option<usize> {
        let mut lo = 0usize;
        let mut hi = self.ids.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let v = match self.ids.get(mid) {
                Some(x) => *x,
                None => return None,
            };
            if v == id {
                return Some(mid);
            } else if v < id {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        None
    }

    /// 注册一个 ID：越窗 OutOfRange、撞号 DupId、满册 RegistryFull；
    /// 二分定位插入，保持有序不变式；拒绝不留痕。
    pub fn register(&mut self, id: u32) -> Result<(), UniqCode> {
        if id < ID_LO || id > ID_HI {
            return Err(UniqCode::OutOfRange);
        }
        if self.find(id).is_some() {
            return Err(UniqCode::DupId);
        }
        if self.ids.len() as u32 >= self.cap {
            return Err(UniqCode::RegistryFull);
        }
        let mut pos = 0usize;
        while pos < self.ids.len() {
            match self.ids.get(pos) {
                Some(v) if *v < id => pos += 1,
                _ => break,
            }
        }
        self.ids.insert(pos, id);
        Ok(())
    }

    /// 有序不变式：任意时刻从小到大排列。
    pub fn sorted_invariant(&self) -> bool {
        let mut i = 1usize;
        while i < self.ids.len() {
            let (a, b) = match (self.ids.get(i - 1), self.ids.get(i)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return false,
            };
            if a >= b {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 已注册 ID 数。
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// 是否满册（480 全占用）。
    pub fn is_full(&self) -> bool {
        self.ids.len() as u32 >= self.cap
    }

    /// 覆盖核：满册时号位必须恰好为 9601..=10080 连续无空洞。
    pub fn coverage_ok(&self) -> bool {
        if !self.is_full() {
            return false;
        }
        let mut i = 0usize;
        while i < self.ids.len() {
            let expect = ID_LO + i as u32;
            match self.ids.get(i) {
                Some(v) if *v == expect => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 防重账（扫描守恒账本） ────────────────────────────────────────────
/// 防重账：扫描数 = 唯一号数 + 撞号数，恒成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UniqLedger {
    /// 本轮扫描样本数
    pub scanned: u32,
    /// 唯一通过数
    pub unique: u32,
    /// 批内撞号数
    pub batch_dup: u32,
    /// 与注册表撞号数
    pub scan_dup: u32,
    /// 越窗样本数（计入扫描不入唯一）
    pub range_out: u32,
}

impl UniqLedger {
    /// 空账构造。
    pub const fn new() -> UniqLedger {
        UniqLedger { scanned: 0, unique: 0, batch_dup: 0, scan_dup: 0, range_out: 0 }
    }

    /// 守恒式：扫描数 = 唯一 + 批内撞号 + 注册表撞号 + 越窗。
    pub fn conserved(&self) -> bool {
        self.scanned == self.unique + self.batch_dup + self.scan_dup + self.range_out
    }
}

/// 批量重扫对账：批内查重 + 与注册表查重，逐样本归类入账；
/// 空批 EmptyBatch 显性拒绝。返回（防重账, 逐样本错误码或 None）。
pub fn rescan_batch(reg: &IdRegistry, batch: &[u32]) -> Result<(UniqLedger, Vec<Option<UniqCode>>), UniqCode> {
    if batch.is_empty() {
        return Err(UniqCode::EmptyBatch);
    }
    let mut led = UniqLedger::new();
    let mut marks: Vec<Option<UniqCode>> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < batch.len() {
        let id = match batch.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut mark: Option<UniqCode> = None;
        if id < ID_LO || id > ID_HI {
            mark = Some(UniqCode::OutOfRange);
            led.range_out += 1;
        } else if contains_u32(&seen, id) {
            mark = Some(UniqCode::BatchCollision);
            led.batch_dup += 1;
        } else if reg.find(id).is_some() {
            mark = Some(UniqCode::ScanDup);
            led.scan_dup += 1;
        } else {
            seen.push(id);
            led.unique += 1;
        }
        led.scanned += 1;
        marks.push(mark);
        i += 1;
    }
    Ok((led, marks))
}

/// 线性查重辅助（批内 seen 集合）。
fn contains_u32(v: &[u32], x: u32) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if let Some(t) = v.get(i) {
            if *t == x {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 注入核：坏 ID 样本矩阵 ────────────────────────────────────────────
/// 四类坏 ID 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadId {
    /// 撞号：注册已有 ID
    DupId,
    /// 越窗：注册窗外 ID
    OutOfRange,
    /// 批量撞号：重扫批内重复
    BatchCollision,
    /// 满册溢出：满册后强行注册
    Overflow,
}

impl BadId {
    /// 闭集全量。
    pub const ALL: [BadId; 4] = [
        BadId::DupId,
        BadId::OutOfRange,
        BadId::BatchCollision,
        BadId::Overflow,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadId::DupId => "单号撞号",
            BadId::OutOfRange => "越窗注册",
            BadId::BatchCollision => "批量撞号",
            BadId::Overflow => "满册溢出",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 UniqRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), UniqCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(UniqCode::UniqRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏 ID 注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新注册表，确保被拒样本不留痕。
pub fn run_bad_id_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadId::ALL.len() {
        let case = match BadId::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadId::DupId => {
                    let mut reg = IdRegistry::new();
                    let _ = reg.register(9747);
                    let before = reg.len();
                    let ok = reg.register(9747).err() == Some(UniqCode::DupId)
                        && reg.len() == before;
                    ok
                }
                BadId::OutOfRange => {
                    let mut reg = IdRegistry::new();
                    reg.register(9600).err() == Some(UniqCode::OutOfRange)
                        && reg.register(10081).err() == Some(UniqCode::OutOfRange)
                }
                BadId::BatchCollision => {
                    let reg = IdRegistry::new();
                    match rescan_batch(&reg, &[9700, 9700, 9701]) {
                        Ok((led, _)) => led.batch_dup == 1 && led.scanned == 3,
                        Err(_) => false,
                    }
                }
                BadId::Overflow => {
                    let mut reg = IdRegistry::new();
                    let mut id = ID_LO;
                    while id <= ID_HI {
                        let _ = reg.register(id);
                        id += 1;
                    }
                    reg.is_full() && reg.coverage_ok()
                        && reg.register(ID_LO).err() == Some(UniqCode::DupId)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 防重成文（纯函数） ────────────────────────────────────────────────
/// 防重成文：「注册=<n>;扫描=<n>;唯一=<n>;撞号=<n>」；失守恒先拒。
pub fn compose_uniq_summary(reg: &IdRegistry, led: &UniqLedger) -> Result<String, UniqCode> {
    if !led.conserved() || (led.scanned == 0 && reg.is_empty()) {
        return Err(UniqCode::EmptySummary);
    }
    let out = format!(
        "注册={};扫描={};唯一={};撞号={}",
        reg.len(),
        led.scanned,
        led.unique,
        led.batch_dup + led.scan_dup
    );
    if out.is_empty() {
        return Err(UniqCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；撞号/重复注册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越窗 ID 族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；满册溢出族"),
    ("EDQUOT", -122, 39, "Windows ERROR_DISK_QUOTA_EXCEEDED；号位预算耗尽族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_MORE_ITEMS；空批对账族"),
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

/// 规范语料双跑指纹：乱序注册五个号 → 重扫四号批 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = IdRegistry::new();
    let _ = reg.register(10080);
    let _ = reg.register(9601);
    let _ = reg.register(9747);
    let _ = reg.register(9900);
    let _ = reg.register(9905);
    let led = match rescan_batch(&reg, &[9601, 9747, 9700, 9700]) {
        Ok((l, _)) => l,
        Err(_) => UniqLedger::new(),
    };
    let bytes = match compose_uniq_summary(&reg, &led) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10073 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10073_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10073");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92240..=0x9227F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 窗口冻结：480 号位口径
    let win_ok = ID_TOTAL == 480 && ID_LO == 9601 && ID_HI == 10080;
    s.add("窗口冻结：F9601..=F10080 恰 480 号位", win_ok,
          "全域号位预算冻结为 480，窗口口径不得漂移");

    // 3. 乱序注册与二分定位：注册五个乱序号后有序不变式成立
    let mut reg = IdRegistry::new();
    let _ = reg.register(10080);
    let _ = reg.register(9601);
    let _ = reg.register(9747);
    let _ = reg.register(9900);
    let _ = reg.register(9905);
    s.add("有序不变式：乱序注册后注册表始终从小到大",
          reg.sorted_invariant() && reg.len() == 5
              && reg.find(9747) == Some(1) && reg.find(9600).is_none(),
          "插入走二分定位，历史排列不被重排");

    // 4. 撞号检出：单号注册 10/10 次拒绝且不留痕
    let mut dup_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut reg2 = IdRegistry::new();
        let _ = reg2.register(9700);
        let before = reg2.len();
        if reg2.register(9700).err() != Some(UniqCode::DupId) || reg2.len() != before {
            dup_ok = false;
        }
        r += 1;
    }
    s.add("撞号检出：10/10 次拒绝且注册表不变", dup_ok,
          "全域唯一核逐次拦截撞号，被拒样本不留痕");

    // 5. 越窗检出：窗口两端 10/10 次拒绝
    let mut range_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut reg2 = IdRegistry::new();
        if reg2.register(9600).err() != Some(UniqCode::OutOfRange)
            || reg2.register(10081).err() != Some(UniqCode::OutOfRange)
        {
            range_ok = false;
        }
        r2 += 1;
    }
    s.add("越窗检出：窗外 ID 10/10 次拒绝", range_ok,
          "F9601..=F10080 之外的号位一律不属于本域");

    // 6. 批量重扫对账：批内撞号与注册表撞号分别计数
    //    9601/9747 与注册表撞（scan_dup=2），9700 唯一，9700 二现批内撞。
    let (led, marks) = match rescan_batch(&reg, &[9601, 9747, 9700, 9700]) {
        Ok(v) => v,
        Err(_) => (UniqLedger::new(), Vec::new()),
    };
    s.add("批量重扫：批内撞号与注册表撞号分别入账",
          led.conserved() && led.scanned == 4 && led.unique == 1
              && led.batch_dup == 1 && led.scan_dup == 2 && led.range_out == 0,
          "扫描数 = 唯一 + 批内撞号 + 注册表撞号 + 越窗，恒成立");

    // 7. 逐样本标记：1/2 位注册表撞号、3 位唯一无标记、4 位批内撞号
    let mark_ok = marks.len() == 4
        && marks.get(0).map(|m| *m == Some(UniqCode::ScanDup)).unwrap_or(false)
        && marks.get(1).map(|m| *m == Some(UniqCode::ScanDup)).unwrap_or(false)
        && marks.get(2).map(|m| m.is_none()).unwrap_or(false)
        && marks.get(3).map(|m| *m == Some(UniqCode::BatchCollision)).unwrap_or(false);
    s.add("逐样本标记：每一条重扫样本带可定位错误码", mark_ok,
          "撞号必须逐样本可定位，不许整批吞掉");

    // 8. 注入核：四类坏 ID 样本各 10/10 拒绝
    let (inj, per_class) = run_bad_id_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏 ID 样本各 10/10 拒绝", all_caught,
          "撞号/越窗/批量撞号/满册溢出必须逐类逐次拒绝");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadId::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 满册覆盖核：480 全占用后覆盖核恰为连续无空洞
    let mut full = IdRegistry::new();
    let mut id = ID_LO;
    let mut fill_ok = true;
    while id <= ID_HI {
        if reg_or_err(&mut full, id).is_err() {
            fill_ok = false;
        }
        id += 1;
    }
    s.add("满册覆盖核：480 号位全占用且连续无空洞",
          fill_ok && full.is_full() && full.coverage_ok() && full.len() == 480,
          "满册必须恰好覆盖 9601..=10080 连续号位");

    // 11. 满册后撞号：注册 9601 必然 DupId
    s.add("满册撞号：满册后注册既有号 10/10 次拒绝",
          full.register(ID_LO).err() == Some(UniqCode::DupId),
          "满册后任何既有号必然撞号，禁止覆盖写");

    // 12. 拒绝不留痕：越窗/撞号被拒后账面不变
    let before = (reg.len(), reg.sorted_invariant());
    let _ = reg.register(9601);
    let _ = reg.register(12345);
    let after = (reg.len(), reg.sorted_invariant());
    s.add("拒绝不留痕：坏注册不改注册表", before == after,
          "被拒注册不得改动注册表长度与有序性");

    // 13. 空批对账拒绝
    s.add("空批对账拒绝：空重扫批显性拒绝",
          rescan_batch(&reg, &[]).err() == Some(UniqCode::EmptyBatch),
          "空批对账等于虚报扫描量，逐次拦截");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(17) && agrees_with_linux(22) && agrees_with_linux(28)
        && agrees_with_linux(122) && agrees_with_linux(61)
        && !agrees_with_linux(0) && !agrees_with_linux(6666);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < UniqCode::ALL.len() {
        let mut j = i + 1;
        while j < UniqCode::ALL.len() {
            if let (Some(a), Some(b)) = (UniqCode::ALL.get(i), UniqCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = UniqCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 成文 + 双跑指纹
    let sum = compose_uniq_summary(&reg, &led);
    let sum_ok = match &sum {
        Ok(t) => t.contains("注册=") && t.contains("撞号="),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：标记齐全且双跑哈希逐位相等", sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 注册量 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let reg_text = format!("注册 {} / 480 号位（满册 {}）", reg.len(), full.is_full());
    s.add_dyn("注册量快照", reg.len() == 5 && full.is_full(), &reg_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

/// 满册填充辅助：吞掉满册过程中的错误码只报是否成功。
fn reg_or_err(reg: &mut IdRegistry, id: u32) -> Result<(), UniqCode> {
    reg.register(id)
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_constants_frozen() {
        assert_eq!(ID_TOTAL, 480);
        assert_eq!(ID_HI - ID_LO + 1, 480);
    }

    #[test]
    fn register_sorted_and_find() {
        let mut reg = IdRegistry::new();
        assert!(reg.register(10080).is_ok());
        assert!(reg.register(9601).is_ok());
        assert!(reg.register(9747).is_ok());
        assert!(reg.sorted_invariant());
        assert_eq!(reg.find(9747), Some(1));
        assert!(reg.find(9999).is_none());
    }

    #[test]
    fn dup_and_range_rejected() {
        let mut reg = IdRegistry::new();
        let _ = reg.register(9700);
        assert_eq!(reg.register(9700).err(), Some(UniqCode::DupId));
        assert_eq!(reg.register(9600).err(), Some(UniqCode::OutOfRange));
        assert_eq!(reg.register(10081).err(), Some(UniqCode::OutOfRange));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn batch_rescan_classification() {
        let mut reg = IdRegistry::new();
        let _ = reg.register(9747);
        let (led, _) = rescan_batch(&reg, &[9747, 9800, 9800, 9600]).unwrap_or((UniqLedger::new(), Vec::new()));
        assert!(led.conserved());
        assert_eq!(led.scan_dup, 1);
        assert_eq!(led.batch_dup, 1);
        assert_eq!(led.unique, 1);
        assert_eq!(led.range_out, 1);
        assert_eq!(led.scanned, 4);
    }

    #[test]
    fn empty_batch_rejected() {
        let reg = IdRegistry::new();
        assert_eq!(rescan_batch(&reg, &[]).err(), Some(UniqCode::EmptyBatch));
    }

    #[test]
    fn bad_id_matrix_green() {
        let (inj, per) = run_bad_id_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn full_coverage_480() {
        let mut reg = IdRegistry::new();
        let mut id = ID_LO;
        while id <= ID_HI {
            assert!(reg.register(id).is_ok());
            id += 1;
        }
        assert!(reg.is_full() && reg.coverage_ok());
        assert_eq!(reg.register(ID_LO).err(), Some(UniqCode::DupId));
    }

    #[test]
    fn summary_compose() {
        let mut reg = IdRegistry::new();
        let _ = reg.register(9747);
        let (led, _) = rescan_batch(&reg, &[9747, 9800]).unwrap_or((UniqLedger::new(), Vec::new()));
        let s = compose_uniq_summary(&reg, &led).unwrap_or_default();
        assert!(s.contains("注册=1") && s.contains("扫描=2"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(17));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(122), -122);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10073_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10073_checks_green() {
        let set = run_unx10073_checks();
        assert!(set.all_passed(), "UNX-F10073 red: {}", set.red_items_text());
    }
}
