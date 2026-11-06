//! UNX-F10004 战线维聚合器（五战线） · 0x91100段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10005-J1 模块聚合零缺模（ltp_* 全清单核对 10/10 次），缺模
//! 10/10 次检出。
//!
//! 设计要点：
//!   ① 五战线按「线册」建模：战线一名一册闭集写死（战线一..战线五），
//!      B01..B40 按冻结映射 f = ((b-1)×5)/40 + 1 落线（每线恰好 8 批）；
//!      一批只能归属一条线，跨线重复显性拒绝。
//!   ② 线册收账状态机：Idle → Staffed → Closed；Closed 再收账、Idle
//!      直接闭册均拒绝；闭册要求五线逐线非空（零缺线）。
//!   ③ 缺线注入按「逐线各抽空一次」5 类 × 10 轮复测，必须 10/10 次检出；
//!      注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ④ 线行数守恒：五线合计 = 逐批之和，闭册前强制复核。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10004 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91100;

/// 战线数（五战线）。
pub const FRONT_COUNT: usize = 5;

/// 批次总数（B01..B40）。
pub const BATCH_COUNT: usize = 40;

/// 每线批数配额（40 批 ÷ 5 线）。
pub const QUOTA_PER_FRONT: usize = BATCH_COUNT / FRONT_COUNT;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;

/// 战线名闭集（冻结名录，顺序即线序）。
pub const FRONT_NAMES: [&str; 5] = ["战线一", "战线二", "战线三", "战线四", "战线五"];

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrontCode {
    /// 线号越界：不在 1..=5
    FrontOutOfRange,
    /// 线名未知：不在冻结名录
    UnknownFront,
    /// 批号越界：不在 1..=40
    BatchOutOfRange,
    /// 跨线重复：同批已在其他线入账
    CrossFrontDuplicate,
    /// 缺线：闭册时存在空线
    MissingFront,
    /// 配额未满：闭册时某线批数不足 8
    QuotaShort,
    /// 闭册后收账
    AddAfterClose,
    /// 未收账先闭册
    CloseBeforeStaff,
    /// 状态回退
    StateRegress,
    /// 守恒破约：五线合计 ≠ 逐批之和
    ConservationBreach,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl FrontCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FrontCode::FrontOutOfRange => "线号越界被拒：战线只允许 1..=5；请按\
                冻结名录的五线编号入账",
            FrontCode::UnknownFront => "未知战线被拒：线名不在冻结名录内；请对照\
                战线一..战线五修正",
            FrontCode::BatchOutOfRange => "批号越界被拒：批号只允许 1..=40；\
                越界批一律拒绝",
            FrontCode::CrossFrontDuplicate => "跨线重复被拒：一批只能归属一条线；\
                请先查该批既有归属再入账",
            FrontCode::MissingFront => "缺线被拒：闭册要求五线逐线非空；请补齐\
                空线批次",
            FrontCode::QuotaShort => "配额未满被拒：每线应收满 8 批；请按冻结映射\
                补齐该线配额",
            FrontCode::AddAfterClose => "闭册后收账被拒：线册已封定即只读；如需\
                增补请新开一期线册",
            FrontCode::CloseBeforeStaff => "未收账先闭册被拒：至少有一线入账才能\
                走闭册复核",
            FrontCode::StateRegress => "状态回退被拒：Idle→Staffed→Closed 只进\
                不退，回退视为私改线册",
            FrontCode::ConservationBreach => "守恒破约：五线行数合计与逐批之和不\
                等；请核对漏线或双计",
            FrontCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            FrontCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [FrontCode; 12] = [
        FrontCode::FrontOutOfRange,
        FrontCode::UnknownFront,
        FrontCode::BatchOutOfRange,
        FrontCode::CrossFrontDuplicate,
        FrontCode::MissingFront,
        FrontCode::QuotaShort,
        FrontCode::AddAfterClose,
        FrontCode::CloseBeforeStaff,
        FrontCode::StateRegress,
        FrontCode::ConservationBreach,
        FrontCode::NotReproducible,
        FrontCode::SegmentViolation,
    ];
}

/// 冻结线映射：批号 b（1..=40）→ 线序（1..=5），每线 8 批。
pub fn front_of_batch(b: usize) -> Result<usize, FrontCode> {
    if b < 1 || b > BATCH_COUNT {
        return Err(FrontCode::BatchOutOfRange);
    }
    let f = ((b - 1) * FRONT_COUNT) / BATCH_COUNT + 1;
    if f < 1 || f > FRONT_COUNT {
        return Err(FrontCode::FrontOutOfRange);
    }
    Ok(f)
}

/// 线名查线序（冻结名录精确匹配），未知名返回 0。
pub fn front_id_of_name(name: &str) -> usize {
    let mut i = 0usize;
    while i < FRONT_NAMES.len() {
        if let Some(n) = FRONT_NAMES.get(i) {
            if *n == name {
                return i + 1;
            }
        }
        i += 1;
    }
    0
}

// ── 线册状态机 ────────────────────────────────────────────────────────
/// 线册生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrontState {
    /// 未收账
    Idle,
    /// 收账中
    Staffed,
    /// 已闭册
    Closed,
}

/// 单线账：线名 + 本线批次 + 行数。
#[derive(Clone, Debug)]
pub struct FrontCell {
    /// 线序（1..=5）
    pub id: usize,
    /// 线名（冻结名录）
    pub name: &'static str,
    /// 本线已收批号
    pub batches: Vec<usize>,
    /// 本线累计行数
    pub lines: u32,
}

/// 五战线聚合器。
#[derive(Clone, Debug)]
pub struct FrontRegistry {
    state: FrontState,
    cells: Vec<FrontCell>,
    /// 逐批行数合计（守恒基准）
    batch_lines: u32,
    /// 已收批数
    received: u32,
}

impl FrontRegistry {
    /// 构造 Idle 空线册（五线按冻结名录预置）。
    pub fn new() -> FrontRegistry {
        let mut cells = Vec::new();
        let mut i = 0usize;
        while i < FRONT_COUNT {
            let name = match FRONT_NAMES.get(i) {
                Some(n) => *n,
                None => "",
            };
            cells.push(FrontCell { id: i + 1, name, batches: Vec::new(), lines: 0 });
            i += 1;
        }
        FrontRegistry { state: FrontState::Idle, cells, batch_lines: 0, received: 0 }
    }

    /// 当前状态。
    pub fn state(&self) -> FrontState {
        self.state
    }

    /// 入账一批：越界/跨线重复拒绝；Idle 自动转 Staffed。
    pub fn add_batch(&mut self, batch: usize, lines: u32) -> Result<usize, FrontCode> {
        if self.state == FrontState::Closed {
            return Err(FrontCode::AddAfterClose);
        }
        let f = front_of_batch(batch)?;
        // 跨线重复：全册查同批号。
        let mut ci = 0usize;
        while ci < self.cells.len() {
            if let Some(cell) = self.cells.get(ci) {
                let mut bi = 0usize;
                while bi < cell.batches.len() {
                    if let Some(b) = cell.batches.get(bi) {
                        if *b == batch {
                            return Err(FrontCode::CrossFrontDuplicate);
                        }
                    }
                    bi += 1;
                }
            }
            ci += 1;
        }
        self.state = FrontState::Staffed;
        let cell = match self.cells.get_mut(f - 1) {
            Some(c) => c,
            None => return Err(FrontCode::FrontOutOfRange),
        };
        cell.batches.push(batch);
        cell.lines = cell.lines.saturating_add(lines);
        self.batch_lines = self.batch_lines.saturating_add(lines);
        self.received = self.received.saturating_add(1);
        Ok(f)
    }

    /// 取线（越界返回 None）。
    pub fn cell(&self, front: usize) -> Option<&FrontCell> {
        if front < 1 || front > FRONT_COUNT {
            return None;
        }
        self.cells.get(front - 1)
    }

    /// 空线数。
    pub fn empty_fronts(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.batches.is_empty() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 配额缺量：Σ max(0, 8 − 本线批数)。
    pub fn quota_shortfall(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.batches.len() < QUOTA_PER_FRONT {
                    n = n.saturating_add((QUOTA_PER_FRONT - c.batches.len()) as u32);
                }
            }
            i += 1;
        }
        n
    }

    /// 五线行数合计。
    pub fn front_lines_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                sum = sum.saturating_add(c.lines);
            }
            i += 1;
        }
        sum
    }

    /// 逐批行数合计（守恒基准）。
    pub fn batch_lines_total(&self) -> u32 {
        self.batch_lines
    }

    /// 已收批数。
    pub fn received_batches(&self) -> u32 {
        self.received
    }

    /// 闭册：Idle 拒、Closed 拒；要求零空线、配额满、守恒过。
    pub fn close(&mut self) -> Result<(), FrontCode> {
        if self.state == FrontState::Idle {
            return Err(FrontCode::CloseBeforeStaff);
        }
        if self.state == FrontState::Closed {
            return Err(FrontCode::StateRegress);
        }
        if self.empty_fronts() != 0 {
            return Err(FrontCode::MissingFront);
        }
        if self.quota_shortfall() != 0 {
            return Err(FrontCode::QuotaShort);
        }
        if self.front_lines_total() != self.batch_lines {
            return Err(FrontCode::ConservationBreach);
        }
        self.state = FrontState::Closed;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
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
            return Err(-(FrontCode::ConservationBreach as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造一份缺第 `drop` 线（1..=5）全部批次的线册（其余线配额满）。
pub fn registry_missing_front(drop: usize) -> FrontRegistry {
    let mut reg = FrontRegistry::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        if let Ok(f) = front_of_batch(b) {
            if f != drop {
                let _ = reg.add_batch(b, 10);
            }
        }
        b += 1;
    }
    reg
}

/// 执行缺线注入矩阵：5 线各抽空一次 × 10 轮，返回账本与逐线检出数。
pub fn run_missing_front_matrix() -> (InjectLedger, [u32; FRONT_COUNT]) {
    let mut ledger = InjectLedger::zero();
    let mut per_front = [0u32; FRONT_COUNT];
    let mut f = 0usize;
    while f < FRONT_COUNT {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut reg = registry_missing_front(f + 1);
            let caught = reg.close().err() == Some(FrontCode::MissingFront);
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_front.get_mut(f) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        f += 1;
    }
    (ledger, per_front)
}

/// 全量线册：40 批逐批入账（行数 = 8 + 批号 mod 5，确定性）。
pub fn build_full_registry() -> FrontRegistry {
    let mut reg = FrontRegistry::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = reg.add_batch(b, (8 + b % 5) as u32);
        b += 1;
    }
    let _ = reg.close();
    reg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；状态非法/越位族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺线/未知线名族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；跨线重复族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界批/线号族"),
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

/// 线册指纹：五线行数 + 已收批数 + 状态逐字节入哈希。
pub fn registry_fingerprint(reg: &FrontRegistry) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut f = 1usize;
    while f <= FRONT_COUNT {
        let lines = match reg.cell(f) {
            Some(c) => c.lines,
            None => 0,
        };
        bytes.push(((lines >> 8) & 0xFF) as u8);
        bytes.push((lines & 0xFF) as u8);
        f += 1;
    }
    bytes.push((reg.received_batches() & 0xFF) as u8);
    bytes.push(match reg.state() {
        FrontState::Idle => 0,
        FrontState::Staffed => 1,
        FrontState::Closed => 2,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10004 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10004_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10004");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 冻结名录：五线名逐一可查且互不相同
    let mut name_ok = FRONT_NAMES.len() == FRONT_COUNT;
    let mut i = 0usize;
    while i < FRONT_NAMES.len() {
        let mut j = i + 1;
        while j < FRONT_NAMES.len() {
            if let (Some(a), Some(b)) = (FRONT_NAMES.get(i), FRONT_NAMES.get(j)) {
                if a == b {
                    name_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 1usize;
    while k <= FRONT_COUNT {
        if front_id_of_name(FRONT_NAMES[k - 1]) != k {
            name_ok = false;
        }
        k += 1;
    }
    s.add("冻结名录：五线名互异且逐一可查", name_ok,
          "线名是战线维主键，重名或查无都会破坏归属");

    // 3. 线映射：40 批逐一落线
    let mut map_ok = true;
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        if front_of_batch(b).is_err() {
            map_ok = false;
        }
        b += 1;
    }
    s.add("线映射：40 批逐一落线且无越界", map_ok,
          "映射式 (b-1)×5/40+1 必须把全部批号映进 1..=5");

    // 4. 全量线册：五线逐线非空、配额满、闭册成功
    let full = build_full_registry();
    s.add("零缺线：五线逐线非空且配额各满 8 批",
          full.empty_fronts() == 0 && full.quota_shortfall() == 0
              && full.state() == FrontState::Closed,
          "40 批入账后每线恰好 8 批，闭册必须成功");

    // 5. 线行数守恒
    s.add("线守恒：五线行数合计=逐批之和",
          full.front_lines_total() == full.batch_lines_total()
              && full.received_batches() == BATCH_COUNT as u32,
          "行数在线间不许增减，守恒式必须逐位相等");

    // 6. 缺线注入矩阵：5 线各抽空一次 × 10/10 检出
    let (ledger, per_front) = run_missing_front_matrix();
    let mut all_caught = true;
    let mut f = 0usize;
    while f < per_front.len() {
        if let Some(n) = per_front.get(f) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        f += 1;
    }
    s.add("缺线注入：逐线各抽空 10/10 检出", all_caught,
          "五线逐一抽空后闭册必须 10/10 次报缺线");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == FRONT_COUNT as u32 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 8. 跨线重复拒绝
    let mut dup = FrontRegistry::new();
    let first = dup.add_batch(6, 10);
    // 批 6 按映射落线一；直接对线一再塞批 6 应拒。
    let again = dup.add_batch(6, 10);
    s.add("跨线重复：同批二次入账被拒",
          first.is_ok() && again.err() == Some(FrontCode::CrossFrontDuplicate),
          "一批一线是归属唯一性硬约束");

    // 9. 越界批号拒绝
    let mut rng = FrontRegistry::new();
    s.add("越界拒绝：B00/B41 均拒",
          rng.add_batch(0, 10).err() == Some(FrontCode::BatchOutOfRange)
              && rng.add_batch(BATCH_COUNT + 1, 10)
                  == Err(FrontCode::BatchOutOfRange),
          "批号域 1..=40 之外一律拒绝");

    // 10. 未知线名拒绝
    s.add("未知线名：名录外查号返回 0",
          front_id_of_name("战线六") == 0 && front_id_of_name("") == 0,
          "名录外线名查号必须显性返回 0，不许猜线");

    // 11. 状态机：Idle 闭册拒、Closed 收账拒
    let mut idle = FrontRegistry::new();
    let idle_close = idle.close();
    let mut closed = build_full_registry();
    let closed_add = closed.add_batch(1, 10);
    s.add("状态机：Idle 闭册/Closed 收账均拒",
          idle_close.err() == Some(FrontCode::CloseBeforeStaff)
              && closed_add.err() == Some(FrontCode::AddAfterClose),
          "Idle→Staffed→Closed 只进不退，越位操作全部拦截");

    // 12. 配额缺量复核
    let short = registry_missing_front(5);
    s.add("配额缺量：缺第五线时缺量恰为 8",
          short.empty_fronts() == 1 && short.quota_shortfall() == QUOTA_PER_FRONT as u32,
          "抽空一线的缺量必须等于该线配额");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = registry_fingerprint(&build_full_registry());
    let fp_b = registry_fingerprint(&build_full_registry());
    s.add("双跑一致：线册指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = FrontCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < FrontCode::ALL.len() {
        let mut j = i + 1;
        while j < FrontCode::ALL.len() {
            if let (Some(a), Some(b)) = (FrontCode::ALL.get(i), FrontCode::ALL.get(j)) {
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

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("线册指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_names_frozen() {
        assert_eq!(FRONT_NAMES.len(), 5);
        assert_eq!(front_id_of_name("战线三"), 3);
        assert_eq!(front_id_of_name("战线九"), 0);
    }

    #[test]
    fn front_mapping_partitions() {
        let mut counts = [0usize; 5];
        let mut b = 1usize;
        while b <= 40 {
            let f = front_of_batch(b).unwrap_or(0);
            assert!(f >= 1 && f <= 5);
            counts[f - 1] += 1;
            b += 1;
        }
        for c in counts.iter() {
            assert_eq!(*c, 8);
        }
    }

    #[test]
    fn full_registry_closes() {
        let reg = build_full_registry();
        assert_eq!(reg.state(), FrontState::Closed);
        assert_eq!(reg.empty_fronts(), 0);
        assert_eq!(reg.front_lines_total(), reg.batch_lines_total());
    }

    #[test]
    fn missing_front_detected() {
        let mut f = 1usize;
        while f <= 5 {
            let mut reg = registry_missing_front(f);
            assert_eq!(reg.close(), Err(FrontCode::MissingFront));
            f += 1;
        }
    }

    #[test]
    fn cross_front_duplicate_rejected() {
        let mut reg = FrontRegistry::new();
        assert!(reg.add_batch(6, 10).is_ok());
        assert_eq!(reg.add_batch(6, 10), Err(FrontCode::CrossFrontDuplicate));
    }

    #[test]
    fn out_of_range_batch_rejected() {
        let mut reg = FrontRegistry::new();
        assert_eq!(reg.add_batch(41, 10), Err(FrontCode::BatchOutOfRange));
        assert_eq!(reg.add_batch(0, 10), Err(FrontCode::BatchOutOfRange));
    }

    #[test]
    fn idle_close_rejected() {
        let mut reg = FrontRegistry::new();
        assert_eq!(reg.close(), Err(FrontCode::CloseBeforeStaff));
    }

    #[test]
    fn closed_add_rejected() {
        let mut reg = build_full_registry();
        assert_eq!(reg.add_batch(1, 10), Err(FrontCode::AddAfterClose));
    }

    #[test]
    fn missing_front_matrix_green() {
        let (l, per) = run_missing_front_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, (FRONT_COUNT as u32) * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(
            registry_fingerprint(&build_full_registry()),
            registry_fingerprint(&build_full_registry())
        );
    }

    #[test]
    fn unx10004_checks_green() {
        let set = run_unx10004_checks();
        assert!(set.all_passed(), "UNX-F10004 red: {}", set.red_items_text());
    }
}
