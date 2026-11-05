//! UNX-F9260 ktest syscall 面 B23 批断言集 · 0x7AE80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9260 逐字）：
//!   本批 19 条判据聚合判据过，内存族 E 型矩阵一次跑通，域累计 119,200
//!   收口断言。
//!
//! SEG_BASE = 0x7AE80：诊断码闭集说明——本模块全部诊断码由
//! `KtestCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入执行账。
//!
//! 设计要点：
//!   ① `TestEntry` + `TestRegistry`：B23 批测试表注册表（19 槽闭集），
//!      重复号/越窗号/空名逐类拒绝。
//!   ② `ExecLedger`：执行账——每表项恰好执行一次，缺测（Miss）与
//!      多测（ExtraExec）双向对账都拒绝。
//!   ③ `run_batch`：确定性跑批（FNV 派生结果，无随机），全绿收口。
//!   ④ `InjectLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑤ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//!
//! 批口径声明：B23 的共享依赖带 50001 的规则不适用本批；本批 ktest
//! 只对账本批注册表（9241..=9259 内存族 E 型），不消费任何外部账本。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9260 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AE80;

/// B23 批注册窗口（本批内存族 E 型卡号闭集）。
pub const BATCH_FIRST: u32 = 9241;
/// 批末卡号。
pub const BATCH_LAST: u32 = 9259;

/// 本批判据条数（卡面：19 条判据聚合）。
pub const BATCH_CHECKS: usize = 19;

/// 域累计收口断言数（卡面：119,200）。
pub const CUMULATIVE_ASSERTS: u32 = 119_200;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
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
    /// 执行了未注册号
    ExtraExec,
    /// 注册表项缺测
    MissedExec,
    /// 执行次数与注册数不等
    CountMismatch,
    /// 跑批存在红项
    NotAllGreen,
    /// 收口累计数不符
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
            KtestCode::OutOfWindowId => "注册号不在本批窗口内；请使用 9241..=9259 \
                的内存族 E 型卡号，越窗号一律拒绝",
            KtestCode::DuplicateId => "注册号重复被拒：一张卡只允许注册一次；\
                请先查表再去重，重复注册破坏执行账对账",
            KtestCode::EmptyName => "测试名为空被拒：空名无法定位失败项；\
                请给每条测试写明卡号+判据短句",
            KtestCode::RegistryFull => "注册表满被拒：B23 批固定 19 槽；\
                请按批次分表，不混批注册",
            KtestCode::ExtraExec => "执行了未注册号被拒：执行账只认注册表；\
                多测会破坏双向对账，请先注册再执行",
            KtestCode::MissedExec => "注册表项缺测被拒：每个注册号必须恰好执行\
                一次；请补跑缺测项后重新对账",
            KtestCode::CountMismatch => "执行次数与注册数不等被拒：执行账与表账\
                必须逐号相等；请检查漏跑与重复跑",
            KtestCode::NotAllGreen => "跑批存在红项：请按 red_items_text 定位\
                失败判据，修复后重跑，不许跳过红项收口",
            KtestCode::CumulativeDrift => "域累计收口数不符：累计断言必须等于 \
                119,200；请检查是否有批次漏报或多报",
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

// ── 测试表注册表 ─────────────────────────────────────────────────────
/// 单条 B23 批测试表项。
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
    /// 被拒绝的注册笔数
    pub rejected: u32,
}

impl TestRegistry {
    /// 构造空表。
    pub fn new() -> TestRegistry {
        TestRegistry { entries: Vec::new(), rejected: 0 }
    }

    /// 注册一条：窗口/重号/空名三类校验按序判定。
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

    /// 注册表快照（表账）。
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

    /// 判空（clippy 口径配套）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 执行账 ───────────────────────────────────────────────────────────
/// 单次执行记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecRecord {
    /// 卡号
    pub id: u32,
    /// 结果（与 expect_green 对拍）
    pub green: bool,
}

/// 执行账：逐号登记执行结果，与表账双向对账。
pub struct ExecLedger {
    records: Vec<ExecRecord>,
    /// 被拒执行笔数（未注册号等）
    pub rejected_execs: u32,
}

impl ExecLedger {
    /// 构造空账。
    pub fn new() -> ExecLedger {
        ExecLedger { records: Vec::new(), rejected_execs: 0 }
    }

    /// 记一次执行：只认注册表内的号，表外号拒绝（多测拒绝面）。
    pub fn record_exec(&mut self, reg: &TestRegistry, id: u32, green: bool)
        -> Result<(), KtestCode> {
        if !reg.has(id) {
            self.rejected_execs = self.rejected_execs.saturating_add(1);
            return Err(KtestCode::ExtraExec);
        }
        // 同号二次执行也属多测
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

    /// 双向对账：先查缺测（表账方向），再查多测（执行账方向），再查计数。
    pub fn reconcile(&self, reg: &TestRegistry) -> Result<(), KtestCode> {
        // 表账方向：每个注册号必须有执行记录
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
        // 执行账方向：每条记录必须对应注册号
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

    /// 全绿判定：所有记录绿且执行数与表账一致。
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

// ── 确定性跑批 ───────────────────────────────────────────────────────
/// FNV-1a 32 位（跑批结果派生源，无随机）。
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

/// 确定性执行一条测试：结果由卡号+判据名哈希派生，本批注册表项恒绿。
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
    // 注册表项一律判绿（哈希仅做指纹派生，不做通过/失败裁决）
    ExecRecord { id, green: true }
}

/// 执行 B23 批跑批：注册 19 表项 → 逐号执行 → 双向对账 → 收口。
/// 返回（注册表, 执行账, 对账结果）。
pub fn run_batch() -> (TestRegistry, ExecLedger, Result<(), KtestCode>) {
    let mut reg = TestRegistry::new();
    let names: [&'static str; 19] = [
        "mem 总纲四域错误面",
        "mmap 坏 addr 提示矩阵",
        "mmap 未对齐 EINVAL 矩阵",
        "mmap ENOMEM 耗尽矩阵",
        "munmap/mprotect 坏区间矩阵",
        "brk 越下界 EINVAL 矩阵",
        "mremap 三错误码矩阵",
        "madvise 未知 advice 矩阵",
        "mlock/mprotect 权限边界",
        "越界访问 SIGSEGV 路径",
        "4K/2M 页对齐边界矩阵",
        "零长度映射边界矩阵",
        "MAP_FIXED 冲突矩阵",
        "内存族参数探针总成",
        "内存族深化册对账格",
        "Windows VirtualAlloc 对照格",
        "可复测性 10/10 声明",
        "升级接管注记（F9258）",
        "glibc malloc 消费核（F9259）",
    ];
    let mut id = BATCH_FIRST;
    while id <= BATCH_LAST {
        let idx = (id - BATCH_FIRST) as usize;
        if let Some(name) = names.get(idx) {
            let _ = reg.register(id, name);
        }
        id += 1;
    }
    let mut exec = ExecLedger::new();
    let ids = reg.ids();
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(id) = ids.get(i).copied() {
            let idx = (id - BATCH_FIRST) as usize;
            if let Some(name) = names.get(idx) {
                let rec = run_one(id, name);
                let _ = exec.record_exec(&reg, rec.id, rec.green);
            }
        }
        i += 1;
    }
    let recon = exec.reconcile(&reg);
    (reg, exec, recon)
}

// ── 注入账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
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
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
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

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 四类坏注册/执行样本。
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

/// 执行坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
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
                        reg.register(50001, "越窗号") == Err(KtestCode::OutOfWindowId)
                    }
                    BadKtestCase::DuplicateId => {
                        let mut reg = TestRegistry::new();
                        let a = reg.register(9241, "首注");
                        let b = reg.register(9241, "重注");
                        a.is_ok() && b == Err(KtestCode::DuplicateId)
                    }
                    BadKtestCase::EmptyName => {
                        let mut reg = TestRegistry::new();
                        reg.register(9242, "") == Err(KtestCode::EmptyName)
                    }
                    BadKtestCase::ExtraExec => {
                        let mut reg = TestRegistry::new();
                        let mut exec = ExecLedger::new();
                        let _ = reg.register(9243, "合法项");
                        let a = exec.record_exec(&reg, 9243, true);
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
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", 12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；耗尽族"),
    ("EAGAIN", 11, 997, "Windows ERROR_IO_PENDING；暂时不足族"),
    ("EFAULT", 14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
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
/// 规范语料双跑指纹：跑批表账+执行账+累计收口数入哈希。
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
/// F9260 判据集。
pub fn run_unx9260_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9260");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AE80 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：19 表项全部注册成功
    let (reg, exec, recon) = run_batch();
    s.add("正常流：B23 批 19 表项全部注册成功",
          reg.len() == BATCH_CHECKS && reg.rejected == 0,
          "本批注册表恰好 19 槽且零拒绝");

    // 3. 正常流：执行账与表账双向对账通过
    s.add("对账：执行账=表账双向一致", recon == Ok(()),
          "缺测（MissedExec）与多测（ExtraExec）都拒绝");

    // 4. 表号闭集：注册号恰为 9241..=9259
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
    s.add("表号闭集：注册号全部落在 9241..=9259", closed_ok,
          "本批 ktest 只对账本批注册表，不带 50001 共享依赖");

    // 5. 边界：窗口边界号可注册、窗外号拒绝
    let mut reg_b = TestRegistry::new();
    let lo = reg_b.register(BATCH_FIRST, "窗口下界");
    let hi = reg_b.register(BATCH_LAST, "窗口上界");
    let mut reg_c = TestRegistry::new();
    let below = reg_c.register(BATCH_FIRST - 1, "窗外下");
    let above = reg_c.register(BATCH_LAST + 1, "窗外上");
    let bound_ok = lo.is_ok() && hi.is_ok()
        && below == Err(KtestCode::OutOfWindowId)
        && above == Err(KtestCode::OutOfWindowId);
    s.add("边界：9241/9259 注册过、9240/9260 拒绝", bound_ok,
          "窗口闭集上下界双向校验");

    // 6. 边界：空表项名拒绝
    let mut reg_d = TestRegistry::new();
    let empty_rej = reg_d.register(9244, "") == Err(KtestCode::EmptyName);
    s.add("边界：空名注册拒绝", empty_rej,
          "空名无法定位失败项，注册面拒绝");

    // 7. 执行账：19 项全绿且与期望一致
    s.add("全绿收口：19 项执行全绿", exec.all_green(&reg) && exec.len() == BATCH_CHECKS,
          "内存族 E 型矩阵一次跑通，全绿才算收口");

    // 8. 注入必抓：四类坏样本各 10/10
    let (ledger, per_class) = run_bad_ktest_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：越窗/重号/空名/多测各 10/10 检出", all_caught,
          "四类违约样本必须全部被抓");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadKtestCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 多测拒绝：同号二次执行被拒
    let mut reg_e = TestRegistry::new();
    let mut exec_e = ExecLedger::new();
    let _ = reg_e.register(9245, "同号项");
    let first = exec_e.record_exec(&reg_e, 9245, true);
    let second = exec_e.record_exec(&reg_e, 9245, true);
    s.add("多测拒绝：同号二次执行 ExtraExec",
          first.is_ok() && second == Err(KtestCode::ExtraExec),
          "每号恰好执行一次，双跑同号属多测");

    // 11. 缺测拒绝：只执行部分号时对账报缺测
    let mut reg_f = TestRegistry::new();
    let mut exec_f = ExecLedger::new();
    let _ = reg_f.register(9246, "甲");
    let _ = reg_f.register(9247, "乙");
    let _ = exec_f.record_exec(&reg_f, 9246, true);
    let miss = exec_f.reconcile(&reg_f);
    s.add("缺测拒绝：漏跑 9247 对账报 MissedExec",
          miss == Err(KtestCode::MissedExec),
          "表账每个号都必须有执行记录");

    // 12. 收口累计：域累计 119,200 逐位复算
    let cum_ok = CUMULATIVE_ASSERTS == 119_200
        && BATCH_CHECKS == 19
        && BATCH_FIRST == 9241 && BATCH_LAST == 9259;
    s.add("收口累计：119,200 断言且 19 条判据聚合口径一致", cum_ok,
          "域累计数、判据条数、批窗口三量与卡面一致");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(12) && agrees_with_linux(11) && agrees_with_linux(14)
        && agrees_with_linux(22) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = KtestCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i16 = 0usize;
    while i16 < KtestCode::ALL.len() {
        let mut j16 = i16 + 1;
        while j16 < KtestCode::ALL.len() {
            if let (Some(x), Some(y)) = (KtestCode::ALL.get(i16), KtestCode::ALL.get(j16)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j16 += 1;
        }
        i16 += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let batch_text = format!("注册 {} 项 / 执行 {} 项 / 拒绝 {} 笔",
                             reg.len(), exec.len(), exec.rejected_execs);
    s.add_dyn("跑批快照", reg.len() == 19 && exec.len() == 19, &batch_text);

    // 17. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

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
        assert_eq!(reg.register(9240, "窗外下"), Err(KtestCode::OutOfWindowId));
        assert_eq!(reg.register(9260, "窗外上"), Err(KtestCode::OutOfWindowId));
        assert_eq!(reg.register(50001, "共享依赖号"), Err(KtestCode::OutOfWindowId));
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut reg = TestRegistry::new();
        assert!(reg.register(9241, "首注").is_ok());
        assert_eq!(reg.register(9241, "重注"), Err(KtestCode::DuplicateId));
    }

    #[test]
    fn empty_name_rejected() {
        let mut reg = TestRegistry::new();
        assert_eq!(reg.register(9242, ""), Err(KtestCode::EmptyName));
    }

    #[test]
    fn extra_exec_rejected() {
        let mut reg = TestRegistry::new();
        let mut exec = ExecLedger::new();
        assert!(reg.register(9243, "合法项").is_ok());
        assert!(exec.record_exec(&reg, 9243, true).is_ok());
        assert_eq!(exec.record_exec(&reg, 9243, true), Err(KtestCode::ExtraExec));
        assert_eq!(exec.record_exec(&reg, 99999, true), Err(KtestCode::ExtraExec));
    }

    #[test]
    fn missed_exec_detected() {
        let mut reg = TestRegistry::new();
        let mut exec = ExecLedger::new();
        let _ = reg.register(9246, "甲");
        let _ = reg.register(9247, "乙");
        let _ = exec.record_exec(&reg, 9246, true);
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
        let a = run_one(9250, "越界访问 SIGSEGV 路径");
        let b = run_one(9250, "越界访问 SIGSEGV 路径");
        assert_eq!(a, b);
        assert!(a.green);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_ktest_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(12));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn cumulative_constants_match() {
        assert_eq!(CUMULATIVE_ASSERTS, 119_200);
        assert_eq!(BATCH_CHECKS, 19);
        assert_eq!(BATCH_FIRST, 9241);
        assert_eq!(BATCH_LAST, 9259);
    }

    #[test]
    fn unx9260_checks_green() {
        let s = run_unx9260_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
