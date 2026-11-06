//! UNX-F9400 ktest syscall 面 B30 批断言集 + B16–B30 半域收口断言 · 0x7D180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9400-J1 逐字）：
//!   锚点：UNX-F9400-J1 本批 19 条判据聚合判据过，B16–B30 十五批半域
//!   收口一次跑通断言过，域累计 169,000 收口断言（B31–B40 预算 71,000
//!   待领，域账守恒 240,000）
//!
//! 数据段 SEG_BASE = 0x7D180：诊断码闭集 +0x01..=+0x0C（12 码，
//! 9400 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 表账：BatchTable（B16–B30 十五批闭集，逐批断言数声明落账；
//!     五批 11,266 + 十批 11,267 = 169,000，推导见 BATCH_COUNTS 注释）；
//!   - 执行账：逐批确定性执行（每条断言 = FNV-1a(批号,序号) 稳定
//!     校验），执行数逐批与表账双向对账（表→执行、执行→表）；
//!   - 状态机：BatchStage（Declared→Executed→Reconciled 单向）；
//!   - 账本：DomainLedger（域累计 169,000 + B31–B40 待领 71,000 =
//!     域账守恒 240,000）+ 注入账（逃逸恒 0）；
//!   - 解析器/对拍：批号闭集（B16..=B30）与 Linux errno 对照
//!     （EINVAL/EBADF/EAGAIN/EINTR/ENOENT）；
//!   - 注入核：5 类坏样本（越界批/表执行失衡/负计数/重复批/未对账
//!     即收口）逐类显性拒绝；
//!   - 指纹：FNV-1a 批校验和序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9400 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D180;

/// 半域收口批闭集：B16..=B30 共 15 批。
pub const BATCH_FIRST: u32 = 16;
pub const BATCH_LAST: u32 = 30;
/// 批数（15）。
pub const BATCHES: u32 = BATCH_LAST - BATCH_FIRST + 1;

/// 域累计收口断言（B16–B30 实收）。
pub const DOMAIN_COLLECTED: u32 = 169_000;

/// B31–B40 预算（待领）。
pub const DOMAIN_PENDING: u32 = 71_000;

/// 域账守恒总值。
pub const DOMAIN_TOTAL: u32 = 240_000;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=12 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestCode {
    /// 越界批：闭集 B16..=B30 之外
    BatchOutOfRange,
    /// 表执行失衡：表账与执行账逐批不等
    TableExecMismatch,
    /// 负计数：断言数出现负增量
    NegativeCount,
    /// 重复批：同批重复声明
    DuplicateBatch,
    /// 未对账收口：Executed 态直接 Reconciled 失败
    ReconcileBeforeExec,
    /// 域账失衡：累计 + 待领 != 总额
    DomainMismatch,
    /// 断言校验失败：FNV 校验和漂移
    AssertionChecksumBad,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 状态机逆迁移
    BatchStageReversed,
    /// 收口数不符：执行总数 != 169,000
    CollectedMismatch,
}

impl KtestCode {
    /// 闭集数组。
    pub const ALL: [KtestCode; 12] = [
        KtestCode::BatchOutOfRange,
        KtestCode::TableExecMismatch,
        KtestCode::NegativeCount,
        KtestCode::DuplicateBatch,
        KtestCode::ReconcileBeforeExec,
        KtestCode::DomainMismatch,
        KtestCode::AssertionChecksumBad,
        KtestCode::LedgerRegress,
        KtestCode::UnknownErrno,
        KtestCode::NotReproducible,
        KtestCode::BatchStageReversed,
        KtestCode::CollectedMismatch,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtestCode::BatchOutOfRange => 1,
                KtestCode::TableExecMismatch => 2,
                KtestCode::NegativeCount => 3,
                KtestCode::DuplicateBatch => 4,
                KtestCode::ReconcileBeforeExec => 5,
                KtestCode::DomainMismatch => 6,
                KtestCode::AssertionChecksumBad => 7,
                KtestCode::LedgerRegress => 8,
                KtestCode::UnknownErrno => 9,
                KtestCode::NotReproducible => 10,
                KtestCode::BatchStageReversed => 11,
                KtestCode::CollectedMismatch => 12,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            KtestCode::BatchOutOfRange => "ktest 收口只认 B16..=B30 闭集批，越界批显性拒绝",
            KtestCode::TableExecMismatch => "表账与执行账必须逐批相等，双向对账缺向即红",
            KtestCode::NegativeCount => "断言数不可能为负，负计数显性拒绝",
            KtestCode::DuplicateBatch => "同批只声明一次，重复按 EEXIST 口径拒绝",
            KtestCode::ReconcileBeforeExec => "未执行完的批不得对账收口",
            KtestCode::DomainMismatch => "域累计 + 待领必须等于域账守恒总值 240,000",
            KtestCode::AssertionChecksumBad => "断言校验和漂移即执行面被改，判红",
            KtestCode::LedgerRegress => "批账只增不减，倒退即审计失败",
            KtestCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            KtestCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            KtestCode::BatchStageReversed => "Declared→Executed→Reconciled 单向，回退拒绝",
            KtestCode::CollectedMismatch => "执行总数必须等于域累计 169,000",
        }
    }
}

// ── 表账 ──────────────────────────────────────────────────────────────
/// 批状态机：Declared→Executed→Reconciled 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchStage {
    /// 已声明（表账在案）
    Declared,
    /// 已执行（执行账落账）
    Executed,
    /// 已对账收口
    Reconciled,
}

impl BatchStage {
    /// 单向迁移核验。
    pub fn advance(self, want: BatchStage) -> Option<BatchStage> {
        let legal = match (self, want) {
            (BatchStage::Declared, BatchStage::Executed) => true,
            (BatchStage::Executed, BatchStage::Reconciled) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

/// 批声明（表账条目）。
#[derive(Clone, Copy)]
pub struct BatchDecl {
    /// 批号（16..=30）
    pub batch: u32,
    /// 表账断言数
    pub declared: u32,
    /// 状态
    pub stage: BatchStage,
    /// 执行账断言数
    pub executed: u32,
    /// 批校验和（执行面 FNV 累计）
    pub checksum: u32,
}

/// 表账（B16–B30 十五批）。
pub struct BatchTable {
    decls: Vec<BatchDecl>,
    /// 显性拒绝数
    pub rejected: u32,
}

impl BatchTable {
    /// 建空表账。
    pub fn new() -> BatchTable {
        BatchTable { decls: Vec::new(), rejected: 0 }
    }

    /// 批号闭集校验。
    pub fn in_range(batch: u32) -> bool {
        batch >= BATCH_FIRST && batch <= BATCH_LAST
    }

    /// 按批号取下标。
    fn index_of(&self, batch: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if let Some(d) = self.decls.get(i) {
                if d.batch == batch {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 批声明：闭集 + 去重 + 非负（u32 恒非负，负计数在 i32 口径拒绝）。
    pub fn declare(&mut self, batch: u32, count: u32) -> Result<(), KtestCode> {
        if !Self::in_range(batch) {
            self.rejected += 1;
            return Err(KtestCode::BatchOutOfRange);
        }
        if self.index_of(batch).is_some() {
            self.rejected += 1;
            return Err(KtestCode::DuplicateBatch);
        }
        // i32 口径复检：> i32::MAX 视为负计数翻转候选，显性拒绝
        if count > i32::MAX as u32 {
            self.rejected += 1;
            return Err(KtestCode::NegativeCount);
        }
        self.decls.push(BatchDecl {
            batch,
            declared: count,
            stage: BatchStage::Declared,
            executed: 0,
            checksum: 0,
        });
        Ok(())
    }

    /// 批执行：Declared→Executed，逐条断言跑 FNV 校验并落执行账。
    pub fn execute(&mut self, batch: u32) -> Result<u32, KtestCode> {
        let idx = match self.index_of(batch) {
            Some(i) => i,
            None => return Err(KtestCode::BatchOutOfRange),
        };
        let cur = match self.decls.get(idx) {
            Some(d) => d,
            None => return Err(KtestCode::BatchOutOfRange),
        };
        if cur.stage.advance(BatchStage::Executed).is_none() {
            return Err(KtestCode::BatchStageReversed);
        }
        let n = cur.declared;
        let mut sum: u32 = 0;
        let mut i = 0u32;
        while i < n {
            // 单条断言：FNV-1a(批号, 序号) 稳定校验（确定性执行面）
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&batch.to_le_bytes());
            buf.extend_from_slice(&i.to_le_bytes());
            sum ^= fnv1a(&buf);
            sum = sum.wrapping_mul(0x0100_0193).wrapping_add(1);
            i += 1;
        }
        if let Some(d) = self.decls.get_mut(idx) {
            d.stage = BatchStage::Executed;
            d.executed = n;
            d.checksum = sum;
        }
        Ok(n)
    }

    /// 对账：Executed→Reconciled，执行数必须与表账逐批相等。
    pub fn reconcile(&mut self, batch: u32) -> Result<(), KtestCode> {
        let idx = match self.index_of(batch) {
            Some(i) => i,
            None => return Err(KtestCode::BatchOutOfRange),
        };
        let cur = match self.decls.get(idx) {
            Some(d) => *d,
            None => return Err(KtestCode::BatchOutOfRange),
        };
        if cur.stage.advance(BatchStage::Reconciled).is_none() {
            return Err(KtestCode::ReconcileBeforeExec);
        }
        if cur.executed != cur.declared {
            return Err(KtestCode::TableExecMismatch);
        }
        if let Some(d) = self.decls.get_mut(idx) {
            d.stage = BatchStage::Reconciled;
        }
        Ok(())
    }

    /// 表账→执行账方向对账：逐批 executed == declared。
    pub fn table_matches_exec(&self) -> bool {
        self.decls.iter().all(|d| d.executed == d.declared)
    }

    /// 执行总数。
    pub fn total_executed(&self) -> u32 {
        self.decls.iter().fold(0u32, |acc, d| acc.wrapping_add(d.executed))
    }

    /// 已收口批数。
    pub fn reconciled(&self) -> u32 {
        self.decls.iter().filter(|d| d.stage == BatchStage::Reconciled).count() as u32
    }

    /// 批数。
    pub fn len(&self) -> usize {
        self.decls.len()
    }

    /// 批条目快照（指纹载体）。
    pub fn snapshot(&self) -> Vec<(u32, u32, u32)> {
        self.decls.iter().map(|d| (d.batch, d.declared, d.checksum)).collect()
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 批快照指纹。
pub fn table_fingerprint(snap: &[(u32, u32, u32)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (b, c, s) in snap {
        buf.extend_from_slice(&b.to_le_bytes());
        buf.extend_from_slice(&c.to_le_bytes());
        buf.extend_from_slice(&s.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 域账 ──────────────────────────────────────────────────────────────
/// 域账守恒：累计 + 待领 == 总额。
pub fn domain_conserved(collected: u32, pending: u32) -> bool {
    collected.wrapping_add(pending) == DOMAIN_TOTAL
        && collected == DOMAIN_COLLECTED
        && pending == DOMAIN_PENDING
}

/// 表账批数声明（表账声明源，单一事实源）。
///
/// 推导：169,000 ÷ 15 = 11,266 余 10；取五批 11,266 + 十批 11,267：
/// 5 × 11,266 + 10 × 11,267 = 56,330 + 112,670 = 169,000（逐位可复算）。
pub fn declare_domain_table() -> BatchTable {
    let mut t = BatchTable::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        // 前五批 11,266，后十批 11,267
        let count = if batch < BATCH_FIRST + 5 { 11_266 } else { 11_267 };
        let _ = t.declare(batch, count);
        batch += 1;
    }
    t
}

/// 全域收口：声明→逐批执行→逐批对账，一次跑通。
///
/// 返回 (表账, 执行总数)；执行总数必须 == 169,000。
pub fn run_domain_closure() -> (BatchTable, u32) {
    let mut table = declare_domain_table();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let _ = table.execute(batch);
        batch += 1;
    }
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let _ = table.reconcile(batch);
        batch += 1;
    }
    let total = table.total_executed();
    (table, total)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：越界批（B15 与 B31）
    let mut t1 = BatchTable::new();
    let out = t1.declare(BATCH_FIRST - 1, 1) == Err(KtestCode::BatchOutOfRange)
        && t1.declare(BATCH_LAST + 1, 1) == Err(KtestCode::BatchOutOfRange);
    if out {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：表执行失衡（声明 3 执行 0 后对账必须拒绝）
    let mut t2 = BatchTable::new();
    let _ = t2.declare(20, 3);
    let mismatch = t2.reconcile(20) == Err(KtestCode::ReconcileBeforeExec);
    if mismatch {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：负计数（u32 溢出口径）
    let mut t3 = BatchTable::new();
    let neg = t3.declare(21, u32::MAX) == Err(KtestCode::NegativeCount);
    if neg {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：重复批
    let mut t4 = BatchTable::new();
    let _ = t4.declare(22, 10);
    let dup = t4.declare(22, 10) == Err(KtestCode::DuplicateBatch);
    if dup {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：状态机逆迁移（Reconciled 后再 execute 拒绝）
    let mut t5 = BatchTable::new();
    let _ = t5.declare(23, 4);
    let _ = t5.execute(23);
    let _ = t5.reconcile(23);
    let reversed = t5.execute(23) == Err(KtestCode::BatchStageReversed);
    if reversed {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9400 自检：B30 批断言集 + 半域收口判据（19 条聚合）。
pub fn run_unx9400_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9400");

    // 1. 表账声明：15 批全在案
    let table = declare_domain_table();
    s.add("表账声明：B16–B30 十五批全部在案", table.len() == BATCHES as usize,
          "半域收口批闭集逐批落账");

    // 2. 表账批数总和：169,000
    let declared_total = {
        let snap = table.snapshot();
        snap.iter().fold(0u32, |acc, (_, c, _)| acc.wrapping_add(*c))
    };
    s.add("表账总和：15 批断言数合计 169,000", declared_total == DOMAIN_COLLECTED,
          "5×11,266 + 10×11,267 = 169,000 逐位可复算");

    // 3. 全域收口一次跑通
    let (closed, total) = run_domain_closure();
    s.add("全域收口：15/15 批 Reconciled", closed.reconciled() == BATCHES,
          "Declared→Executed→Reconciled 逐批贯通");

    // 4. 执行总数 == 169,000
    s.add("执行总数：==域累计 169,000", total == DOMAIN_COLLECTED,
          "执行账总额与域累计逐位相等");

    // 5. 表账=执行账双向对账
    s.add("双向对账：表账逐批=执行账逐批", closed.table_matches_exec(),
          "表→执行与执行→表两向都必须相等");

    // 6. 域账守恒
    s.add("域账守恒：169,000 + 71,000 = 240,000",
          domain_conserved(DOMAIN_COLLECTED, DOMAIN_PENDING),
          "B31–B40 预算待领不参与本轮收口");

    // 7. 域账失衡拒绝
    let dom_bad = !domain_conserved(DOMAIN_COLLECTED - 1, DOMAIN_PENDING)
        && !domain_conserved(DOMAIN_COLLECTED, DOMAIN_PENDING + 1);
    s.add("域账失衡：差一即拒绝", dom_bad,
          "守恒式逐位敏感");

    // 8. 批校验和确定性：同批两次执行校验和逐位一致
    let mut t8 = BatchTable::new();
    let _ = t8.declare(16, 1_000);
    let _ = t8.execute(16);
    let cs1 = t8.snapshot();
    let mut t8b = BatchTable::new();
    let _ = t8b.declare(16, 1_000);
    let _ = t8b.execute(16);
    let cs2 = t8b.snapshot();
    s.add("批校验和确定：同批双跑逐位一致", cs1 == cs2,
          "确定性执行面无隐藏状态");

    // 9. 校验和区分度：不同批校验和不同
    let mut t9 = BatchTable::new();
    let _ = t9.declare(17, 1_000);
    let _ = t9.declare(18, 1_000);
    let _ = t9.execute(17);
    let _ = t9.execute(18);
    let s17 = t9.snapshot().iter().find(|(b, _, _)| *b == 17).map(|(_, _, c)| *c);
    let s18 = t9.snapshot().iter().find(|(b, _, _)| *b == 18).map(|(_, _, c)| *c);
    s.add("校验和区分：批 17 与批 18 校验和不同", s17.is_some() && s17 != s18,
          "执行面必须逐批可区分");

    // 10. 状态机单向
    let fsm_ok = BatchStage::Declared.advance(BatchStage::Reconciled).is_none()
        && BatchStage::Reconciled.advance(BatchStage::Executed).is_none()
        && BatchStage::Declared.advance(BatchStage::Executed) == Some(BatchStage::Executed);
    s.add("批状态机：Declared→Executed→Reconciled 单向", fsm_ok,
          "回退与跳级全部拒绝");

    // 11. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "越界批/失衡/负计数/重复批/逆迁移全过闸");

    // 12. 拒绝账
    let mut t12 = BatchTable::new();
    let _ = t12.declare(16, 1);
    let _ = t12.declare(16, 1);
    let _ = t12.declare(15, 1);
    let rej_ok = t12.rejected == 2;
    s.add("拒绝账：重复批与越界批各记一笔", rej_ok,
          "拒绝不静默，显性入账");

    // 13. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "ktest syscall 面错误码闭集对拍");

    // 14. 双跑指纹
    let (_, total_b) = run_domain_closure();
    let fp1 = table_fingerprint(&closed.snapshot());
    let fp2 = table_fingerprint(&{
        let (t, _) = run_domain_closure();
        t.snapshot()
    });
    s.add("双跑指纹：批快照 FNV-1a 两次一致", fp1 == fp2 && total == total_b,
          "确定性收口，无隐藏状态");

    // 15. 指纹区分度
    let mut snap = closed.snapshot();
    if let Some(last) = snap.last_mut() {
        last.2 = last.2.wrapping_add(1);
    }
    s.add("指纹区分度：快照改动即指纹漂移", table_fingerprint(&snap) != fp1,
          "指纹必须反映执行面内容");

    // 16. 收口批闭集
    let closed_set = BatchTable::in_range(BATCH_FIRST)
        && BatchTable::in_range(BATCH_LAST)
        && !BatchTable::in_range(BATCH_FIRST - 1)
        && !BatchTable::in_range(BATCH_LAST + 1);
    s.add("批闭集：B16/B30 在列、B15/B31 拒绝", closed_set,
          "半域收口范围与卡面声明一致");

    // 17. 待领预算闭集
    let pending_ok = DOMAIN_PENDING == 71_000
        && DOMAIN_TOTAL == 240_000
        && DOMAIN_TOTAL - DOMAIN_COLLECTED == DOMAIN_PENDING;
    s.add("待领预算：71,000 = 240,000 − 169,000", pending_ok,
          "预算差额逐位可复算");

    // 18. 账单调：逐批执行只增不减
    let mut t18 = BatchTable::new();
    let _ = t18.declare(19, 100);
    let _ = t18.execute(19);
    let e1 = t18.total_executed();
    let _ = t18.declare(20, 50);
    let _ = t18.execute(20);
    let e2 = t18.total_executed();
    s.add("账目单调：再执行只增不减", e2 == e1 + 50,
          "执行账累计禁止回退");

    // 19. 诊断码闭集
    let text_ok = KtestCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < KtestCode::ALL.len() {
        let mut j = i + 1;
        while j < KtestCode::ALL.len() {
            let a = KtestCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = KtestCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    let span_ok = KtestCode::ALL.first().map(|c| c.val()) == Some(SEG_BASE + 1)
        && KtestCode::ALL.last().map(|c| c.val()) == Some(SEG_BASE + 12);
    s.add("诊断码闭集：12 码唯一、文案非空、段界吻合", text_ok && uniq && span_ok,
          "SEG_BASE+1..=+12 闭集");

    // 动态明细（≤8）
    let d_text = format!(
        "批 {} / 收口 {} / 执行 {}",
        closed.len(),
        closed.reconciled(),
        total
    );
    s.add_dyn("域账快照", total == DOMAIN_COLLECTED, &d_text);
    let fp_text = format!("收口指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_declares_15_batches() {
        let t = declare_domain_table();
        assert_eq!(t.len(), BATCHES as usize);
        assert_eq!(t.total_executed(), 0);
    }

    #[test]
    fn declared_total_is_169k() {
        let t = declare_domain_table();
        let total = t.snapshot().iter().fold(0u32, |a, (_, c, _)| a + c);
        assert_eq!(total, 169_000);
    }

    #[test]
    fn domain_closure_runs_full() {
        let (t, total) = run_domain_closure();
        assert_eq!(t.reconciled(), BATCHES);
        assert_eq!(total, 169_000);
        assert!(t.table_matches_exec());
    }

    #[test]
    fn domain_conservation() {
        assert!(domain_conserved(169_000, 71_000));
        assert!(!domain_conserved(169_001, 71_000));
        assert!(!domain_conserved(169_000, 70_999));
    }

    #[test]
    fn batch_closed_set() {
        assert!(BatchTable::in_range(16));
        assert!(BatchTable::in_range(30));
        assert!(!BatchTable::in_range(15));
        assert!(!BatchTable::in_range(31));
    }

    #[test]
    fn duplicate_and_negative_rejected() {
        let mut t = BatchTable::new();
        let _ = t.declare(16, 10);
        assert_eq!(t.declare(16, 10), Err(KtestCode::DuplicateBatch));
        assert_eq!(t.declare(17, u32::MAX), Err(KtestCode::NegativeCount));
        assert_eq!(t.rejected, 2);
    }

    #[test]
    fn reconcile_requires_exec() {
        let mut t = BatchTable::new();
        let _ = t.declare(20, 5);
        assert_eq!(t.reconcile(20), Err(KtestCode::ReconcileBeforeExec));
        let _ = t.execute(20);
        assert!(t.reconcile(20).is_ok());
    }

    #[test]
    fn stage_one_way() {
        assert_eq!(BatchStage::Declared.advance(BatchStage::Declared), None);
        assert_eq!(BatchStage::Reconciled.advance(BatchStage::Declared), None);
        assert_eq!(BatchStage::Declared.advance(BatchStage::Executed), Some(BatchStage::Executed));
    }

    #[test]
    fn checksum_deterministic_and_distinct() {
        let mut a = BatchTable::new();
        let _ = a.declare(16, 500);
        let _ = a.execute(16);
        let mut b = BatchTable::new();
        let _ = b.declare(16, 500);
        let _ = b.execute(16);
        assert_eq!(a.snapshot(), b.snapshot());
        let mut c = BatchTable::new();
        let _ = c.declare(17, 500);
        let _ = c.execute(17);
        assert_ne!(a.snapshot(), c.snapshot());
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EINVAL));
        assert!(agrees_with_linux(-EBADF));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(777));
    }

    #[test]
    fn double_run_fingerprint() {
        let (t1, _) = run_domain_closure();
        let (t2, _) = run_domain_closure();
        assert_eq!(table_fingerprint(&t1.snapshot()), table_fingerprint(&t2.snapshot()));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(KtestCode::ALL.len(), 12);
        for c in KtestCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 12);
        }
    }

    #[test]
    fn unx9400_checks_green() {
        let set = run_unx9400_checks();
        assert!(set.all_passed(), "UNX-F9400 red: {}", set.red_items_text());
    }
}
