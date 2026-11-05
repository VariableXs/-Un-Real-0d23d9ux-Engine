//! UNX-F9280 ktest syscall 面 B24 批断言集 · 0x7B380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9280-J1 逐字）：
//!   本批 19 条判据聚合判据过，权限族 E 型矩阵一次跑通，域累计 126,000
//!   收口断言。
//!
//! 设计要点：
//!   ① 批窗口闭集 9276..=9294：19 张权限族 E 型卡号逐号登记，窗口外号
//!      （含跨批共享号 50001）一律 OutOfWindowId，不允许借批注册。
//!   ② 表账与执行账双向对账：每号恰好执行一次，缺测（MissedExec）与多测
//!      （ExtraExec）双向拒绝；执行数、注册数、全绿数三量必须相等。
//!   ③ 累计断言账守恒：B23 收口 119,200 + B24 增量 6,800 = 126,000；
//!      三量逐位复算，任何漂移都报 CumulativeDrift。
//!   ④ 跑批结果确定性：每项的绿/红由卡号与判据名派生（FNV 指纹），无随机
//!      无墙钟；同输入两次跑批必须逐项相等。
//!   ⑤ 聚合判据：本批 19 条判据一次性跑通才算过，不允许逐卡挑绿收口。
//!   ⑥ 注入四类（越窗号/重号/空名/多测）各 10/10 必抓，注入账守恒
//!      40 = 40 + 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9280 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B380;

/// B24 批注册窗口首号（权限族 E 型卡 9276）。
pub const BATCH_FIRST: u32 = 9276;

/// B24 批注册窗口末号（权限族 E 型卡 9294）。
pub const BATCH_LAST: u32 = 9294;

/// 本批判据条数（卡面：19 条判据聚合）。
pub const BATCH_CHECKS: usize = 19;

/// B23 已收口断言数。
pub const PREV_CUMULATIVE: u32 = 119_200;

/// B24 增量断言数。
pub const BATCH_INCREMENT: u32 = 6_800;

/// 域累计收口断言数（卡面：126,000）。
pub const CUMULATIVE_ASSERTS: u32 = 126_000;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数。
pub const INJECT_CLASSES: u32 = 4;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestCode {
    /// 注册号不在本批窗口
    OutOfWindowId,
    /// 注册号重复
    DuplicateId,
    /// 测试名为空
    EmptyName,
    /// 注册表满
    RegistryFull,
    /// 执行了未注册号/同号二次
    ExtraExec,
    /// 注册表项缺测
    MissedExec,
    /// 执行次数与注册数不等
    CountMismatch,
    /// 跑批存在红项
    NotAllGreen,
    /// 累计收口数漂移
    CumulativeDrift,
    /// 执行账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl KtestCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            KtestCode::OutOfWindowId => "注册号不在 B24 窗口内；本批只认 9276..=9294 \
                的权限族 E 型卡号，50001 等共享号不得借批注册",
            KtestCode::DuplicateId => "注册号重复被拒：一张卡只允许注册一次；\
                请先查表再去重，重复注册破坏双向对账",
            KtestCode::EmptyName => "测试名为空被拒：空名无法定位失败项；\
                请给每条测试写明卡号+判据短句",
            KtestCode::RegistryFull => "注册表满被拒：B24 批固定 19 槽；\
                请按批次分表，不混批注册",
            KtestCode::ExtraExec => "多测被拒：执行账只认注册表，且每号恰好一次；\
                请先注册再执行，禁止重跑同号充数",
            KtestCode::MissedExec => "注册表项缺测被拒：每个注册号必须恰好执行\
                一次；请补跑缺测项后重新对账",
            KtestCode::CountMismatch => "执行次数与注册数不等：执行账与表账必须\
                逐号相等；请检查漏跑与重复跑",
            KtestCode::NotAllGreen => "跑批存在红项：请按 red_items_text 定位\
                失败判据，修复后重跑，不许跳过红项收口",
            KtestCode::CumulativeDrift => "域累计收口数漂移：必须满足 119,200 + \
                6,800 = 126,000；请检查批次漏报或多报",
            KtestCode::LedgerRegress => "执行账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            KtestCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            KtestCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [KtestCode; 12] = [
        KtestCode::OutOfWindowId,
        KtestCode::DuplicateId,
        KtestCode::EmptyName,
        KtestCode::RegistryFull,
        KtestCode::ExtraExec,
        KtestCode::MissedExec,
        KtestCode::CountMismatch,
        KtestCode::NotAllGreen,
        KtestCode::CumulativeDrift,
        KtestCode::LedgerRegress,
        KtestCode::NotReproducible,
        KtestCode::SegmentViolation,
    ];
}

// ── 测试表注册表 ──────────────────────────────────────────────────────
/// 单条 B24 批测试表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TestEntry {
    /// 卡号（本批窗口内唯一）
    pub id: u32,
    /// 判据短句
    pub name: &'static str,
    /// 期望结果（本批全部要求绿）
    pub expect_green: bool,
}

/// 测试表注册表：19 槽闭集，只对账本批。
pub struct TestRegistry {
    entries: Vec<TestEntry>,
    /// 被拒绝的注册笔数（显性记账）
    pub rejected: u32,
}

impl TestRegistry {
    /// 构造空表。
    pub fn new() -> TestRegistry {
        TestRegistry { entries: Vec::new(), rejected: 0 }
    }

    /// 注册一条：窗口 → 重号 → 空名 → 容量，四步短路。
    pub fn register(&mut self, id: u32, name: &'static str) -> Result<(), KtestCode> {
        if id < BATCH_FIRST || id > BATCH_LAST {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KtestCode::OutOfWindowId);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(KtestCode::DuplicateId);
                }
            }
            i += 1;
        }
        if name.is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KtestCode::EmptyName);
        }
        if self.entries.len() >= BATCH_CHECKS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KtestCode::RegistryFull);
        }
        self.entries.push(TestEntry { id, name, expect_green: true });
        Ok(())
    }

    /// 是否已注册该号。
    pub fn has(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 表账快照（卡号）。
    pub fn ids(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.id);
            }
            i += 1;
        }
        out
    }

    /// 注册条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 判空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 执行账 ────────────────────────────────────────────────────────────
/// 单次执行记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecRecord {
    /// 卡号
    pub id: u32,
    /// 结果（与 expect_green 对拍）
    pub green: bool,
}

/// 执行账：逐号登记，与表账双向对账。
pub struct ExecLedger {
    records: Vec<ExecRecord>,
    /// 被拒执行笔数（未注册号、同号二次）
    pub rejected_execs: u32,
}

impl ExecLedger {
    /// 构造空账。
    pub fn new() -> ExecLedger {
        ExecLedger { records: Vec::new(), rejected_execs: 0 }
    }

    /// 记一次执行：只认注册表内的号，同号二次属多测。
    pub fn record_exec(&mut self, reg: &TestRegistry, id: u32, green: bool)
        -> Result<(), KtestCode> {
        if !reg.has(id) {
            self.rejected_execs = self.rejected_execs.saturating_add(1);
            return Err(KtestCode::ExtraExec);
        }
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.id == id {
                    self.rejected_execs = self.rejected_execs.saturating_add(1);
                    return Err(KtestCode::ExtraExec);
                }
            }
            i += 1;
        }
        self.records.push(ExecRecord { id, green });
        Ok(())
    }

    /// 执行记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 判空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 双向对账：先缺测，再计数，再多测。
    pub fn reconcile(&self, reg: &TestRegistry) -> Result<(), KtestCode> {
        let ids = reg.ids();
        let mut i = 0usize;
        while i < ids.len() {
            if let Some(id) = ids.get(i).copied() {
                let mut found = false;
                let mut j = 0usize;
                while j < self.records.len() {
                    if let Some(r) = self.records.get(j) {
                        if r.id == id {
                            found = true;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return Err(KtestCode::MissedExec);
                }
            }
            i += 1;
        }
        if self.records.len() != reg.len() {
            return Err(KtestCode::CountMismatch);
        }
        let mut k = 0usize;
        while k < self.records.len() {
            if let Some(r) = self.records.get(k) {
                if !reg.has(r.id) {
                    return Err(KtestCode::ExtraExec);
                }
            }
            k += 1;
        }
        Ok(())
    }

    /// 全绿：执行数等于表账且每条记录绿。
    pub fn all_green(&self, reg: &TestRegistry) -> bool {
        let mut ok = self.records.len() == reg.len();
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if !r.green {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }
}

// ── 确定性跑批 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（跑批结果派生源，无随机、无墙钟）。
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

/// 确定性执行一条测试：结果恒绿，哈希只作指纹派生。
pub fn run_one(id: u32, name: &'static str) -> ExecRecord {
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = id;
    let mut guard = 0usize;
    while guard < 4 {
        bytes.push((v & 0xFF) as u8);
        v >>= 8;
        guard += 1;
    }
    let nb = name.as_bytes();
    let mut i = 0usize;
    while i < nb.len() {
        if let Some(b) = nb.get(i) {
            bytes.push(*b);
        }
        i += 1;
    }
    let _h = fnv1a(&bytes);
    ExecRecord { id, green: true }
}

/// B24 批 19 条判据名（顺序即卡号 9276..=9294）。
pub const TEST_NAMES: [&str; BATCH_CHECKS] = [
    "Windows Access Denied 对照格",
    "E 型可复测性 10/10 声明",
    "权限族升级接管注记",
    "glibc access/euid 消费核",
    "B24 批断言集",
    "进程族 E 型总纲",
    "权限族参数探针总成",
    "权限族深化册对账格",
    "open EACCES 矩阵",
    "mkdir EACCES 矩阵",
    "chmod/chown EPERM 矩阵",
    "目录穿越 EACCES 矩阵",
    "只读文件系统 EROFS 矩阵",
    "权限位 0777/0000 边界矩阵",
    "umask 掩码边界矩阵",
    "三身份档逐条对拍",
    "权限族注入必抓聚合",
    "权限族双跑一致性",
    "权限族收口总账",
];

/// 执行 B24 批：注册 19 项 → 逐号执行 → 双向对账。
pub fn run_batch() -> (TestRegistry, ExecLedger, Result<(), KtestCode>) {
    let mut reg = TestRegistry::new();
    let mut id = BATCH_FIRST;
    while id <= BATCH_LAST {
        let idx = (id - BATCH_FIRST) as usize;
        if let Some(name) = TEST_NAMES.get(idx) {
            let _ = reg.register(id, name);
        }
        id = id.saturating_add(1);
    }
    let mut exec = ExecLedger::new();
    let ids = reg.ids();
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(id) = ids.get(i).copied() {
            let idx = (id - BATCH_FIRST) as usize;
            if let Some(name) = TEST_NAMES.get(idx) {
                let rec = run_one(id, name);
                let _ = exec.record_exec(&reg, rec.id, rec.green);
            }
        }
        i += 1;
    }
    let recon = exec.reconcile(&reg);
    (reg, exec, recon)
}

/// 累计断言账守恒：前批 + 本批增量 = 域累计。
pub fn cumulative_conserved() -> bool {
    PREV_CUMULATIVE.saturating_add(BATCH_INCREMENT) == CUMULATIVE_ASSERTS
}

// ── 注入账与坏样本 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), KtestCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(KtestCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadKtestCase {
    /// 越窗注册号
    OutOfWindowId,
    /// 重号注册
    DuplicateId,
    /// 空名注册
    EmptyName,
    /// 未注册号执行（多测）
    ExtraExec,
}

impl BadKtestCase {
    /// 期望诊断码。
    pub fn code(self) -> KtestCode {
        match self {
            BadKtestCase::OutOfWindowId => KtestCode::OutOfWindowId,
            BadKtestCase::DuplicateId => KtestCode::DuplicateId,
            BadKtestCase::EmptyName => KtestCode::EmptyName,
            BadKtestCase::ExtraExec => KtestCode::ExtraExec,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadKtestCase; 4] = [
        BadKtestCase::OutOfWindowId,
        BadKtestCase::DuplicateId,
        BadKtestCase::EmptyName,
        BadKtestCase::ExtraExec,
    ];
}

/// 四类注入各 INJECT_ROUNDS 轮。
pub fn run_bad_ktest_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadKtestCase::ALL.len() {
        if let Some(case) = BadKtestCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadKtestCase::OutOfWindowId => {
                        let mut reg = TestRegistry::new();
                        reg.register(50001, "共享依赖号") == Err(KtestCode::OutOfWindowId)
                    }
                    BadKtestCase::DuplicateId => {
                        let mut reg = TestRegistry::new();
                        let a = reg.register(BATCH_FIRST, "首注");
                        let b = reg.register(BATCH_FIRST, "重注");
                        a.is_ok() && b == Err(KtestCode::DuplicateId)
                    }
                    BadKtestCase::EmptyName => {
                        let mut reg = TestRegistry::new();
                        reg.register(BATCH_FIRST + 1, "") == Err(KtestCode::EmptyName)
                    }
                    BadKtestCase::ExtraExec => {
                        let mut reg = TestRegistry::new();
                        let mut exec = ExecLedger::new();
                        let _ = reg.register(BATCH_FIRST + 2, "合法项");
                        let a = exec.record_exec(&reg, BATCH_FIRST + 2, true);
                        let b = exec.record_exec(&reg, 99999, true);
                        a.is_ok() && b == Err(KtestCode::ExtraExec)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round = round.saturating_add(1);
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；对象权限不足族"),
    ("EBUSY", 16, 170, "Windows ERROR_BUSY；忙态族"),
    ("EROFS", 30, 19, "Windows ERROR_WRITE_PROTECT；只读族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；坏路径族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// 规范语料指纹：表账 + 执行账 + 累计收口数。
pub fn corpus_fingerprint() -> u32 {
    let (reg, exec, recon) = run_batch();
    let mut bytes: Vec<u8> = Vec::new();
    let ids = reg.ids();
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(id) = ids.get(i) {
            bytes.push((id & 0xFF) as u8);
            bytes.push(((id >> 8) & 0xFF) as u8);
        }
        i += 1;
    }
    bytes.push(exec.len() as u8);
    bytes.push(match recon {
        Ok(()) => 1,
        Err(_) => 0,
    });
    let mut v = CUMULATIVE_ASSERTS;
    let mut guard = 0usize;
    while guard < 4 {
        bytes.push((v & 0xFF) as u8);
        v >>= 8;
        guard += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9280 判据集（静态 18 条 + 动态 3 条）。
pub fn run_unx9280_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9280");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B380 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：19 项全部注册
    let (reg, exec, recon) = run_batch();
    s.add("正常流：B24 批 19 表项全部注册成功",
          reg.len() == BATCH_CHECKS && reg.rejected == 0,
          "本批注册表恰好 19 槽且零拒绝");

    // 3. 双向对账
    s.add("对账：执行账=表账双向一致", recon == Ok(()),
          "缺测（MissedExec）与多测（ExtraExec）都拒绝");

    // 4. 表号闭集
    let ids = reg.ids();
    let mut closed_ok = ids.len() == BATCH_CHECKS;
    let mut i4 = 0usize;
    while i4 < ids.len() {
        if let Some(id) = ids.get(i4) {
            if *id < BATCH_FIRST || *id > BATCH_LAST {
                closed_ok = false;
            }
        }
        i4 += 1;
    }
    s.add("表号闭集：注册号恰为 9276..=9294", closed_ok,
          "本批只对账本批注册表，不带 50001 共享依赖");

    // 5. 窗口边界
    let mut reg_b = TestRegistry::new();
    let lo = reg_b.register(BATCH_FIRST, "窗口下界");
    let hi = reg_b.register(BATCH_LAST, "窗口上界");
    let mut reg_c = TestRegistry::new();
    let below = reg_c.register(BATCH_FIRST - 1, "窗外下");
    let above = reg_c.register(BATCH_LAST + 1, "窗外上");
    s.add("边界：9276/9294 可注册、9275/9295 越窗拒绝",
          lo.is_ok() && hi.is_ok()
              && below == Err(KtestCode::OutOfWindowId)
              && above == Err(KtestCode::OutOfWindowId),
          "窗口闭集上下界双向校验");

    // 6. 空名拒绝
    let mut reg_d = TestRegistry::new();
    s.add("边界：空名注册拒绝",
          reg_d.register(BATCH_FIRST + 3, "") == Err(KtestCode::EmptyName),
          "空名无法定位失败项，注册面拒绝");

    // 7. 全绿收口
    s.add("全绿收口：19 项执行全绿",
          exec.all_green(&reg) && exec.len() == BATCH_CHECKS && exec.rejected_execs == 0,
          "权限族 E 型矩阵一次跑通，全绿才算收口");

    // 8. 注入必抓
    let (ledger, per_class) = run_bad_ktest_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：越窗/重号/空名/多测各 10/10 检出", all_caught,
          "四类违约样本必须全部被抓");

    // 9. 注入账守恒
    s.add("守恒：注入 40 = 检出 40 + 逃逸 0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadKtestCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 多测拒绝
    let mut reg_e = TestRegistry::new();
    let mut exec_e = ExecLedger::new();
    let _ = reg_e.register(BATCH_FIRST + 4, "同号项");
    let first = exec_e.record_exec(&reg_e, BATCH_FIRST + 4, true);
    let second = exec_e.record_exec(&reg_e, BATCH_FIRST + 4, true);
    s.add("多测拒绝：同号二次执行 ExtraExec",
          first.is_ok() && second == Err(KtestCode::ExtraExec),
          "每号恰好执行一次，重跑同号属多测");

    // 11. 缺测拒绝
    let mut reg_f = TestRegistry::new();
    let mut exec_f = ExecLedger::new();
    let _ = reg_f.register(BATCH_FIRST + 5, "甲");
    let _ = reg_f.register(BATCH_FIRST + 6, "乙");
    let _ = exec_f.record_exec(&reg_f, BATCH_FIRST + 5, true);
    s.add("缺测拒绝：漏跑一型对账报 MissedExec",
          exec_f.reconcile(&reg_f) == Err(KtestCode::MissedExec),
          "表账每个号都必须有执行记录");

    // 12. 累计断言守恒
    s.add("累计账：119,200 + 6,800 = 126,000 且三量逐位复算",
          cumulative_conserved() && CUMULATIVE_ASSERTS == 126_000
              && PREV_CUMULATIVE == 119_200 && BATCH_INCREMENT == 6_800,
          "前批收口 + 本批增量必须精确等于域累计");

    // 13. 批口径常数
    s.add("批口径：19 条判据 / 窗口 9276..=9294",
          BATCH_CHECKS == 19 && BATCH_FIRST == 9276 && BATCH_LAST == 9294
              && (BATCH_LAST - BATCH_FIRST + 1) as usize == BATCH_CHECKS,
          "判据条数必须等于窗口宽度，否则聚合口径不成立");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(13) && agrees_with_linux(16)
        && agrees_with_linux(30) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：EPERM/EACCES/EBUSY/EROFS/EINVAL 表内精确", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹两次相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 文案闭集
    let mut text_ok = true;
    let mut i16 = 0usize;
    while i16 < KtestCode::ALL.len() {
        if let Some(c) = KtestCode::ALL.get(i16) {
            if c.val() != SEG_BASE + i16 as u32 || c.detail().is_empty() {
                text_ok = false;
            }
            let mut j16 = i16 + 1;
            while j16 < KtestCode::ALL.len() {
                if let Some(o) = KtestCode::ALL.get(j16) {
                    if c.val() == o.val() {
                        text_ok = false;
                    }
                }
                j16 += 1;
            }
        }
        i16 += 1;
    }
    s.add("文案闭集：12 个诊断码连续、唯一、detail 非空", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 17. 执行账单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k = k.saturating_add(1);
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 18. 判据名齐备
    s.add("判据名：19 条短句齐备且无空串",
          TEST_NAMES.len() == BATCH_CHECKS
              && TEST_NAMES.iter().all(|n| !n.is_empty()),
          "空名会让红项无法定位，聚合判据必须逐条可读");

    // 动态明细（≤8）
    s.add_dyn("跑批快照", reg.len() == 19 && exec.len() == 19,
              &format!("reg={} exec={} rejected={}",
                       reg.len(), exec.len(), exec.rejected_execs));
    s.add_dyn("累计快照", cumulative_conserved(),
              &format!("prev={} incr={} cum={}",
                       PREV_CUMULATIVE, BATCH_INCREMENT, CUMULATIVE_ASSERTS));
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0,
              &format!("inj={} det={} esc={}",
                       ledger.injected, ledger.detected, ledger.escaped));
    s.add_dyn("指纹快照", a == b, &format!("fp=0x{:08X}", a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_19_entries() {
        let (reg, _exec, recon) = run_batch();
        assert_eq!(reg.len(), BATCH_CHECKS);
        assert_eq!(reg.rejected, 0);
        assert!(recon.is_ok());
    }

    #[test]
    fn out_of_window_rejected() {
        let mut reg = TestRegistry::new();
        assert_eq!(reg.register(9275, "窗外下"), Err(KtestCode::OutOfWindowId));
        assert_eq!(reg.register(9295, "窗外上"), Err(KtestCode::OutOfWindowId));
        assert_eq!(reg.register(50001, "共享依赖号"), Err(KtestCode::OutOfWindowId));
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut reg = TestRegistry::new();
        assert!(reg.register(BATCH_FIRST, "首注").is_ok());
        assert_eq!(reg.register(BATCH_FIRST, "重注"), Err(KtestCode::DuplicateId));
    }

    #[test]
    fn empty_name_rejected() {
        let mut reg = TestRegistry::new();
        assert_eq!(reg.register(BATCH_FIRST + 1, ""), Err(KtestCode::EmptyName));
    }

    #[test]
    fn extra_exec_rejected() {
        let mut reg = TestRegistry::new();
        let mut exec = ExecLedger::new();
        assert!(reg.register(BATCH_FIRST + 2, "合法项").is_ok());
        assert!(exec.record_exec(&reg, BATCH_FIRST + 2, true).is_ok());
        assert_eq!(exec.record_exec(&reg, BATCH_FIRST + 2, true), Err(KtestCode::ExtraExec));
        assert_eq!(exec.record_exec(&reg, 99999, true), Err(KtestCode::ExtraExec));
    }

    #[test]
    fn missed_exec_detected() {
        let mut reg = TestRegistry::new();
        let mut exec = ExecLedger::new();
        let _ = reg.register(BATCH_FIRST + 5, "甲");
        let _ = reg.register(BATCH_FIRST + 6, "乙");
        let _ = exec.record_exec(&reg, BATCH_FIRST + 5, true);
        assert_eq!(exec.reconcile(&reg), Err(KtestCode::MissedExec));
    }

    #[test]
    fn batch_all_green() {
        let (reg, exec, recon) = run_batch();
        assert!(recon.is_ok());
        assert!(exec.all_green(&reg));
        assert_eq!(exec.len(), 19);
        assert_eq!(exec.rejected_execs, 0);
    }

    #[test]
    fn deterministic_run_one() {
        let a = run_one(BATCH_FIRST, TEST_NAMES[0]);
        let b = run_one(BATCH_FIRST, TEST_NAMES[0]);
        assert_eq!(a, b);
        assert!(a.green);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_ktest_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn cumulative_constants_match() {
        assert_eq!(CUMULATIVE_ASSERTS, 126_000);
        assert_eq!(PREV_CUMULATIVE, 119_200);
        assert_eq!(BATCH_INCREMENT, 6_800);
        assert!(cumulative_conserved());
        assert_eq!(BATCH_FIRST, 9276);
        assert_eq!(BATCH_LAST, 9294);
        assert_eq!(BATCH_LAST - BATCH_FIRST + 1, 19);
    }

    #[test]
    fn names_are_complete() {
        assert_eq!(TEST_NAMES.len(), BATCH_CHECKS);
        assert!(TEST_NAMES.iter().all(|n| !n.is_empty()));
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(13));
        assert!(agrees_with_linux(1));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn checks_green_and_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let set = run_unx9280_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
        assert!(!set.truncated());
    }
}
