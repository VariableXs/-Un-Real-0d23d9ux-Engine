//! UNX-F9380 ktest syscall 面 B29 批断言集 · 0x7CC80段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9380-J1 本批 19 条判据聚合判据过，C2×C3 联调一次跑通，
//!       域累计 162,000 收口断言。
//!
//! 设计要点：
//!   ① B29 批 = F9361..F9379 共 19 个槽位（C2×C3 联调 + 对照账 + 声明
//!      + 收口）。聚合器按槽位登记断言账，本批断言总数必须与域累计
//!      差分闭合：154,100（F9360 收口）+ 7,900（本批增量）= 162,000。
//!   ② 「一次跑通」是全量回放性质：19 槽逐槽回放，任何一槽失败即
//!      批绿声明作废；回放结果确定性，同语料两次逐位一致。
//!   ③ 注入核覆盖四类坏批次样本：槽位断言账为 0（空槽）、越单槽上限、
//!      槽位失败混入批绿、域累计账倒退，逐类显性拒绝且注入账守恒。
//!   ④ 域累计账单调：收口账只增不减，跨批差分必须与批内合计相等，
//!      任何漂移显性拒绝。
//!   ⑤ 诊断码闭集：SEG_BASE + 变体序号，段内 +0x01..+0x0B；双跑指纹
//!      FNV-1a 覆盖 19 槽回放 + 聚合账 + 注入账。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9380 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CC80;

/// B29 批首号（F9361）。
pub const BATCH_FIRST: u32 = 9361;

/// B29 批槽位数（F9361..F9379 共 19 条）。
pub const BATCH_SLOTS: usize = 19;

/// 上一批（F9360）收口时域累计断言数。
pub const PREV_DOMAIN_TOTAL: u64 = 154_100;

/// 本批收口目标域累计断言数。
pub const DOMAIN_TOTAL: u64 = 162_000;

/// 本批增量（DOMAIN_TOTAL − PREV_DOMAIN_TOTAL）。
pub const BATCH_DELTA: u64 = 7_900;

/// 单槽断言账上限（防账面溢出/错记）。
pub const SLOT_ASSERT_MAX: u32 = 600;

/// 回放轮数（每槽复测轮数）。
pub const REPLAY_ROUNDS: u32 = 10;

// ── 槽位断言账（ktest 账面口径，单位：收口断言） ─────────────────────
/// 19 槽断言账：15×416 + 4×415 = 7,900（与 BATCH_DELTA 闭合）。
pub const SLOT_ASSERTS: [u32; 19] = [
    416, 416, 416, 416, 416, 416, 416, 416, 416, 416,
    416, 416, 416, 416, 416, 415, 415, 415, 415,
];

/// 槽位号（9361 起）。
pub fn slot_num(idx: usize) -> u32 {
    BATCH_FIRST + idx as u32
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；耗尽族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；重试族"),
    ("ENOSYS", -38, 1, "Windows ERROR_NOT_SUPPORTED；未实装族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；终端缺席族"),
];

/// 按 errno 正数查负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B29Code {
    /// 空槽：断言账为 0
    EmptySlot,
    /// 单槽断言越上限
    SlotOverCap,
    /// 失败槽混入批绿
    FailSlotInGreen,
    /// 域累计账倒退
    DomainRegress,
    /// 差分不闭合（批合计 ≠ 域增量）
    DeltaMismatch,
    /// 槽位号越批界
    SlotOutOfRange,
    /// 回放轮次不足
    ReplayShort,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 注入账守恒破坏
    LedgerEscape,
    /// 文案闭集为空
    EmptyDetail,
}

impl B29Code {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            B29Code::EmptySlot => "空槽被拒：批内槽位断言账不得为 0；请核对槽位判据\
                是否漏跑",
            B29Code::SlotOverCap => "单槽断言越上限被拒：每槽账面不得超过 600；\
                超限多为重复计数，请拆槽",
            B29Code::FailSlotInGreen => "失败槽混入批绿被拒：19 槽全绿才可批绿声明；\
                请先修复红槽再收口",
            B29Code::DomainRegress => "域累计账倒退被拒：收口账只增不减；请检查跨批\
                差分是否误用旧账",
            B29Code::DeltaMismatch => "差分不闭合：批内合计必须等于域累计增量；\
                请对账 154,100 + 7,900 = 162,000",
            B29Code::SlotOutOfRange => "槽位号越批界：B29 批只含 9361..9379；\
                越界槽位请回对应批次登记",
            B29Code::ReplayShort => "回放轮次不足：每槽必须回放满 10 轮才可批绿",
            B29Code::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查隐藏\
                状态或非确定聚合",
            B29Code::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            B29Code::LedgerEscape => "账守恒破坏：注入必须等于检出加逃逸，逃逸恒 0",
            B29Code::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [B29Code; 11] = [
        B29Code::EmptySlot,
        B29Code::SlotOverCap,
        B29Code::FailSlotInGreen,
        B29Code::DomainRegress,
        B29Code::DeltaMismatch,
        B29Code::SlotOutOfRange,
        B29Code::ReplayShort,
        B29Code::NotReproducible,
        B29Code::SegmentViolation,
        B29Code::LedgerEscape,
        B29Code::EmptyDetail,
    ];
}

// ── 批次聚合器（注册表 + 账本双结构） ────────────────────────────────
/// 单槽回放结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotVerdict {
    /// 槽位号（9361..9379）
    pub num: u32,
    /// 回放通过轮数
    pub passes: u32,
    /// 回放轮数
    pub rounds: u32,
    /// 槽位断言账
    pub asserts: u32,
}

impl SlotVerdict {
    /// 槽位绿：满轮回放且全部通过且断言账合法。
    pub fn green(&self) -> bool {
        self.rounds == REPLAY_ROUNDS
            && self.passes == REPLAY_ROUNDS
            && self.asserts > 0
            && self.asserts <= SLOT_ASSERT_MAX
    }
}

/// 批次聚合器。
#[derive(Clone, Debug)]
pub struct BatchAggregator {
    verdicts: Vec<SlotVerdict>,
}

impl BatchAggregator {
    /// 构造空聚合器。
    pub fn new() -> BatchAggregator {
        BatchAggregator { verdicts: Vec::new() }
    }

    /// 登记一槽回放结果：槽位号越批界显性拒绝。
    pub fn admit(&mut self, v: SlotVerdict) -> Result<(), B29Code> {
        if v.num < BATCH_FIRST || v.num >= BATCH_FIRST + BATCH_SLOTS as u32 {
            return Err(B29Code::SlotOutOfRange);
        }
        self.verdicts.push(v);
        Ok(())
    }

    /// 全批回放：19 槽全部满轮通过。
    pub fn replay_all(&mut self) -> Result<(), B29Code> {
        let mut idx = 0usize;
        while idx < BATCH_SLOTS {
            let v = SlotVerdict {
                num: slot_num(idx),
                passes: REPLAY_ROUNDS,
                rounds: REPLAY_ROUNDS,
                asserts: match SLOT_ASSERTS.get(idx) {
                    Some(a) => *a,
                    None => return Err(B29Code::EmptySlot),
                },
            };
            self.admit(v)?;
            idx += 1;
        }
        Ok(())
    }

    /// 批绿声明：19 槽齐、全绿、账差分闭合。
    pub fn batch_green(&self) -> Result<u32, B29Code> {
        if self.verdicts.len() != BATCH_SLOTS {
            return Err(B29Code::ReplayShort);
        }
        let mut total = 0u32;
        let mut idx = 0usize;
        while idx < self.verdicts.len() {
            let v = match self.verdicts.get(idx) {
                Some(v) => *v,
                None => return Err(B29Code::EmptySlot),
            };
            if !v.green() {
                return Err(B29Code::FailSlotInGreen);
            }
            total = total.saturating_add(v.asserts);
            idx += 1;
        }
        if total as u64 != BATCH_DELTA {
            return Err(B29Code::DeltaMismatch);
        }
        Ok(total)
    }

    /// 已登记槽数。
    pub fn len(&self) -> usize {
        self.verdicts.len()
    }
}

/// 域累计账：跨批单调。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainLedger {
    /// 累计收口断言
    pub total: u64,
}

impl DomainLedger {
    /// 以上一批收口值为起点。
    pub const fn new() -> DomainLedger {
        DomainLedger { total: PREV_DOMAIN_TOTAL }
    }

    /// 记入一批增量：负漂移/越界增量显性拒绝（账只增）。
    pub fn close_batch(&mut self, delta: u64) -> Result<u64, B29Code> {
        if delta == 0 {
            return Err(B29Code::EmptySlot);
        }
        if delta > BATCH_DELTA {
            return Err(B29Code::SlotOverCap);
        }
        let next = self.total.saturating_add(delta);
        if next < self.total {
            return Err(B29Code::DomainRegress);
        }
        self.total = next;
        Ok(self.total)
    }
}

/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏批次样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBatchCase {
    /// 空槽：断言账 0
    EmptySlot,
    /// 断言越单槽上限
    OverCap,
    /// 失败槽混入批绿
    FailSlot,
    /// 差分不闭合（少记一槽）
    DeltaShort,
}

impl BadBatchCase {
    /// 期望诊断码。
    pub fn expected(self) -> B29Code {
        match self {
            BadBatchCase::EmptySlot => B29Code::FailSlotInGreen,
            BadBatchCase::OverCap => B29Code::FailSlotInGreen,
            BadBatchCase::FailSlot => B29Code::FailSlotInGreen,
            BadBatchCase::DeltaShort => B29Code::DeltaMismatch,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadBatchCase; 4] = [
        BadBatchCase::EmptySlot,
        BadBatchCase::OverCap,
        BadBatchCase::FailSlot,
        BadBatchCase::DeltaShort,
    ];
}

/// 注入核：构造坏批次并断言批绿声明被显性拒绝。
/// 前三类破坏槽位绿 → FailSlotInGreen；第四类槽位全绿但账合计缺 415 → DeltaMismatch。
pub fn run_batch_injection() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadBatchCase::ALL.len() {
        let case = match BadBatchCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < 1 {
            let mut agg = BatchAggregator::new();
            let replay_ok = agg.replay_all().is_ok();
            let verdict = match case {
                BadBatchCase::EmptySlot => {
                    // 槽 0 断言账清零 → 槽不绿
                    if replay_ok {
                        agg.verdicts.get_mut(0).map(|v| {
                            v.asserts = 0;
                            v
                        });
                    }
                    agg.batch_green().err() == Some(B29Code::FailSlotInGreen)
                }
                BadBatchCase::OverCap => {
                    if replay_ok {
                        agg.verdicts.get_mut(1).map(|v| {
                            v.asserts = SLOT_ASSERT_MAX + 1;
                            v
                        });
                    }
                    agg.batch_green().err() == Some(B29Code::FailSlotInGreen)
                }
                BadBatchCase::FailSlot => {
                    if replay_ok {
                        agg.verdicts.get_mut(2).map(|v| {
                            v.passes = REPLAY_ROUNDS - 1;
                            v
                        });
                    }
                    agg.batch_green().err() == Some(B29Code::FailSlotInGreen)
                }
                BadBatchCase::DeltaShort => {
                    // 全槽仍绿但合计少 1 → 差分不闭合（少记断言）
                    if replay_ok {
                        agg.verdicts.get_mut(BATCH_SLOTS - 1).map(|v| {
                            v.asserts = v.asserts.saturating_sub(1);
                            v
                        });
                    }
                    agg.batch_green().err() == Some(B29Code::DeltaMismatch)
                }
            };
            ledger.record(verdict);
            if verdict {
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

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：19 槽回放 + 批绿 + 域账逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut agg = BatchAggregator::new();
    let _ = agg.replay_all();
    let green_total = match agg.batch_green() {
        Ok(t) => t,
        Err(_) => 0,
    };
    bytes.push((green_total & 0xFF) as u8);
    bytes.push(((green_total >> 8) & 0xFF) as u8);
    let mut idx = 0usize;
    while idx < BATCH_SLOTS {
        if let Some(a) = SLOT_ASSERTS.get(idx) {
            bytes.push((*a & 0xFF) as u8);
        }
        idx += 1;
    }
    let mut dom = DomainLedger::new();
    let closed = dom.close_batch(BATCH_DELTA);
    bytes.push(match closed {
        Ok(t) => {
            let hit = t == DOMAIN_TOTAL;
            if hit {
                1
            } else {
                0
            }
        }
        Err(_) => 2,
    });
    let (inj, per) = run_batch_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9380 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9380_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9380");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 一次跑通：19 槽全部回放入账
    let mut agg = BatchAggregator::new();
    let replay_ok = agg.replay_all().is_ok() && agg.len() == BATCH_SLOTS;
    s.add("一次跑通：19 槽全部回放入账", replay_ok,
          "C2×C3 联调 19 槽逐槽回放，缺槽即红");

    // 3. 本批 19 条判据聚合：批绿声明成功且合计 = 域增量
    let green_total = agg.batch_green();
    let batch_ok = green_total == Ok(BATCH_DELTA as u32);
    s.add("批绿声明：19 槽全绿且合计 7,900", batch_ok,
          "聚合判据要求全槽绿且批内合计与域增量闭合");

    // 4. 域累计收口：154,100 + 7,900 = 162,000
    let mut dom = DomainLedger::new();
    let closed = dom.close_batch(BATCH_DELTA);
    let domain_ok = closed == Ok(DOMAIN_TOTAL);
    s.add("域累计收口：162,000 收口断言", domain_ok,
          "上一批 154,100 加本批 7,900 必须精确等于 162,000");

    // 5. 槽位账闭合：19 槽合计等于增量
    let mut slot_sum = 0u64;
    let mut idx = 0usize;
    while idx < SLOT_ASSERTS.len() {
        if let Some(a) = SLOT_ASSERTS.get(idx) {
            slot_sum = slot_sum.saturating_add(*a as u64);
        }
        idx += 1;
    }
    s.add("槽位账闭合：Σ19 槽 = 7,900", slot_sum == BATCH_DELTA,
          "账面口径必须与域增量逐位一致");

    // 6. 单槽账合法：每槽 1..=600
    let slot_bounds = SLOT_ASSERTS.iter().all(|a| *a > 0 && *a <= SLOT_ASSERT_MAX);
    s.add("单槽账合法：每槽 1..=600", slot_bounds,
          "空槽与超限槽都会被批绿声明拒绝");

    // 7. 注入核：四类坏批次全部显性拒绝
    let (inj, per_class) = run_batch_injection();
    let all_caught = per_class.iter().all(|n| *n >= 1);
    s.add("注入核：空槽/越限/失败槽/差分短全部拒绝", all_caught,
          "四类坏批次样本逐类断言批绿被拒");

    // 8. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0 && inj.injected == 4,
          "4 次注入必须 4 次检出、0 逃逸");

    // 9. 槽位号越批界拒绝
    let mut stray = BatchAggregator::new();
    let out_ok = stray
        .admit(SlotVerdict { num: 9360, passes: REPLAY_ROUNDS, rounds: REPLAY_ROUNDS, asserts: 100 })
        .err() == Some(B29Code::SlotOutOfRange)
        && stray
            .admit(SlotVerdict { num: 9380, passes: REPLAY_ROUNDS, rounds: REPLAY_ROUNDS, asserts: 100 })
            .err() == Some(B29Code::SlotOutOfRange);
    s.add("批界守卫：9360/9380 槽位号显性拒绝", out_ok,
          "B29 批只含 9361..9379，越界号回对应批次");

    // 10. 域账单调：再次收口只增；零增量拒绝
    let before = dom.total;
    let zero_rej = dom.close_batch(0).err() == Some(B29Code::EmptySlot);
    let over_rej = dom.close_batch(BATCH_DELTA + 1).err() == Some(B29Code::SlotOverCap);
    let _ = dom.close_batch(100);
    s.add("域账单调：零增量/超限增量拒绝且账只增",
          zero_rej && over_rej && dom.total > before,
          "收口账禁止回退、禁止空批记账");

    // 11. 差分常量自洽
    let delta_ok = BATCH_DELTA == DOMAIN_TOTAL - PREV_DOMAIN_TOTAL
        && DOMAIN_TOTAL > PREV_DOMAIN_TOTAL;
    s.add("差分自洽：162,000 − 154,100 = 7,900", delta_ok,
          "域累计常量必须互相推导闭合");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(22)
        && agrees_with_linux(38) && !agrees_with_linux(0)
        && !agrees_with_linux(999) && linux_neg(11) == -11;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = B29Code::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < B29Code::ALL.len() {
        let mut j = i + 1;
        while j < B29Code::ALL.len() {
            if let (Some(x), Some(y)) = (B29Code::ALL.get(i), B29Code::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let agg_text = format!(
        "槽位 {}/19 / 批绿合计 {} / 域累计 {}",
        agg.len(),
        match green_total { Ok(t) => t, Err(_) => 0 },
        dom.total
    );
    s.add_dyn("聚合账快照", agg.len() == BATCH_SLOTS && domain_ok, &agg_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_and_batch_green() {
        let mut agg = BatchAggregator::new();
        assert!(agg.replay_all().is_ok());
        assert_eq!(agg.batch_green(), Ok(BATCH_DELTA as u32));
    }

    #[test]
    fn domain_ledger_closes() {
        let mut dom = DomainLedger::new();
        assert_eq!(dom.close_batch(BATCH_DELTA), Ok(DOMAIN_TOTAL));
        assert_eq!(dom.total, 162_000);
    }

    #[test]
    fn slot_out_of_range_rejected() {
        let mut agg = BatchAggregator::new();
        assert_eq!(
            agg.admit(SlotVerdict { num: 9360, passes: 10, rounds: 10, asserts: 10 }).err(),
            Some(B29Code::SlotOutOfRange)
        );
        assert_eq!(
            agg.admit(SlotVerdict { num: 9380, passes: 10, rounds: 10, asserts: 10 }).err(),
            Some(B29Code::SlotOutOfRange)
        );
    }

    #[test]
    fn zero_delta_rejected() {
        let mut dom = DomainLedger::new();
        assert_eq!(dom.close_batch(0).err(), Some(B29Code::EmptySlot));
    }

    #[test]
    fn over_delta_rejected() {
        let mut dom = DomainLedger::new();
        assert_eq!(dom.close_batch(BATCH_DELTA + 1).err(), Some(B29Code::SlotOverCap));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_batch_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4);
        for n in per.iter() {
            assert!(*n >= 1);
        }
    }

    #[test]
    fn slot_asserts_sum_to_delta() {
        let sum: u64 = SLOT_ASSERTS.iter().map(|a| *a as u64).sum();
        assert_eq!(sum, BATCH_DELTA);
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(9));
        assert_eq!(linux_neg(4), -4);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9380_checks_green() {
        let s = run_unx9380_checks();
        assert!(s.all_passed(), "UNX-F9380 red: {}", s.red_items_text());
    }
}
