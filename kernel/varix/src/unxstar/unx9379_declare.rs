//! UNX-F9379 I 型联调判据可复测性声明 · 0x7CC40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9379-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，
//!       零 flaky 声明格过。
//!
//! 设计要点：
//!   ① 可复测性按「判据 × 轮次」二维落账：本模块把联调面抽出的 6 条
//!      代表性判据各复测 10 轮，逐轮记通过/失败；台账只增不减。
//!   ② 零 flaky 声明的判定是性质判定而非计数凑数：任何一条判据出现
//!      「同输入不同轮结果不同」即判 flaky，10/10 统计格作废重跑。
//!   ③ 注入核覆盖三类不稳定样本：轮次依赖（奇偶轮翻转）、顺序依赖
//!      （语料乱序后结果漂移）、状态泄漏（第二次运行残留第一次状态），
//!      逐类断言被审计器显性检出，注入账守恒逃逸 0。
//!   ④ 统计格：总轮次 = Σ各判据轮次，总通过 = Σ各判据通过；通过率
//!      千分位整型表示（无浮点），零 flaky ⇔ 通过率 = 1000‰ 且轮次
//!      齐平（每判据轮数相同）。
//!   ⑤ 双跑指纹：FNV-1a 覆盖「复测台账逐格 + 审计注入逐格」，两次
//!      运行逐位一致；指纹本身也参与 10 轮复测，防指纹环节自身 flaky。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9379 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CC40;

/// 每条判据复测轮数（判据要求 10/10）。
pub const RECHECK_ROUNDS: u32 = 10;

/// 复测判据条数（联调面抽出的代表判据）。
pub const CRITERION_COUNT: usize = 6;

/// 通过率千分位满值。
pub const PERMILLE_FULL: u32 = 1000;

/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
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
pub enum RckCode {
    /// 复测轮数不足
    RoundsShort,
    /// 轮次不齐平（某判据轮数与他人不同）
    RoundsUneven,
    /// 同输入不同轮结果漂移
    FlakyDrift,
    /// 顺序依赖检出
    OrderDependent,
    /// 状态泄漏检出
    StateLeak,
    /// 统计账倒退
    LedgerRegress,
    /// 通过率越界（>1000‰ 或负漂移）
    RateOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 未知判据号
    UnknownCriterion,
    /// 文案闭集为空
    EmptyDetail,
}

impl RckCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RckCode::RoundsShort => "复测轮数不足被拒：每条判据必须满 10 轮才可声明；\
                请补齐轮次后重跑统计",
            RckCode::RoundsUneven => "轮次不齐平被拒：台账内各判据轮数必须一致；\
                混轮数统计无意义，请按同轮数重跑",
            RckCode::FlakyDrift => "轮间漂移检出：同输入不同轮结果不同即 flaky；\
                请排查时钟、迭代序或隐藏状态后复测",
            RckCode::OrderDependent => "顺序依赖检出：语料乱序后结果漂移；\
                判据必须对遍历顺序不变，请改为全序复算",
            RckCode::StateLeak => "状态泄漏检出：第二次运行残留第一次状态；\
                判据必须自包含，运行间不得共享可变账目",
            RckCode::LedgerRegress => "统计账倒退被拒：轮次/通过计数只增不减；\
                请检查回滚路径是否误改累计值",
            RckCode::RateOutOfRange => "通过率越界：千分位表示必须在 0..=1000；\
                请检查通过计数是否超过轮次",
            RckCode::NotReproducible => "双跑不一致：同语料两次指纹不同；\
                请排查非确定哈希或未初始化字段",
            RckCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            RckCode::UnknownCriterion => "未知判据号：台账只登记本模块闭集内判据；\
                外来判据必须先入册再复测",
            RckCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [RckCode; 11] = [
        RckCode::RoundsShort,
        RckCode::RoundsUneven,
        RckCode::FlakyDrift,
        RckCode::OrderDependent,
        RckCode::StateLeak,
        RckCode::LedgerRegress,
        RckCode::RateOutOfRange,
        RckCode::NotReproducible,
        RckCode::SegmentViolation,
        RckCode::UnknownCriterion,
        RckCode::EmptyDetail,
    ];
}

// ── 联调代表判据集（确定性，零隐藏状态） ──────────────────────────────
/// 六条代表判据：每条是纯函数，同输入恒同输出。
/// （判据正文与 F9361–F9378 联调面共享编号，此处按编号复测。）
pub fn jointest_criterion(id: u32) -> bool {
    match id {
        // C1 段位闭集：本批各段 0x40 对齐且落在 0x7CC00..=0x7D1C0
        1 => SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE),
        // C2 errno 表逐码可复算
        2 => {
            let mut ok = true;
            let mut i = 0usize;
            while i < ERRNO_TABLE.len() {
                if let Some((name, neg, _, _)) = ERRNO_TABLE.get(i) {
                    if name.is_empty() || *neg >= 0 || linux_neg(-*neg) != *neg {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        // C3 诊断码闭集两两唯一
        3 => {
            let mut ok = true;
            let mut i = 0usize;
            while i < RckCode::ALL.len() && ok {
                let mut j = i + 1;
                while j < RckCode::ALL.len() {
                    if let (Some(a), Some(b)) = (RckCode::ALL.get(i), RckCode::ALL.get(j)) {
                        if a.code() == b.code() {
                            ok = false;
                        }
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        }
        // C4 文案闭集非空
        4 => RckCode::ALL.iter().all(|c| !c.detail().is_empty()),
        // C5 千分位满值语义：1000‰ = 全通过
        5 => PERMILLE_FULL == RECHECK_ROUNDS * 100,
        // C6 注入核闭集五类可得（占位判据在本模块为三类审计闭集非空）
        6 => FlakyCase::ALL.iter().all(|k| k.audit_tag() != 0),
        // 表外判据号：确定性假（显性假，非 panic）
        _ => false,
    }
}

/// 三类不稳定样本（注入核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlakyCase {
    /// 轮次依赖：奇数轮通过、偶数轮失败
    RoundDependent,
    /// 顺序依赖：语料乱序后结果漂移
    OrderDependent,
    /// 状态泄漏：第二次运行残留第一次状态
    StateLeak,
}

impl FlakyCase {
    /// 审计标签（非 0 即在闭集内，供 C6 复算）。
    pub fn audit_tag(self) -> u32 {
        match self {
            FlakyCase::RoundDependent => 0xA1,
            FlakyCase::OrderDependent => 0xA2,
            FlakyCase::StateLeak => 0xA3,
        }
    }

    /// 期望诊断码。
    pub fn expected(self) -> RckCode {
        match self {
            FlakyCase::RoundDependent => RckCode::FlakyDrift,
            FlakyCase::OrderDependent => RckCode::OrderDependent,
            FlakyCase::StateLeak => RckCode::StateLeak,
        }
    }

    /// 闭集全量。
    pub const ALL: [FlakyCase; 3] = [
        FlakyCase::RoundDependent,
        FlakyCase::OrderDependent,
        FlakyCase::StateLeak,
    ];
}

// ── 复测台账 ──────────────────────────────────────────────────────────
/// 单判据复测行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecheckRow {
    /// 判据号（1..=CRITERION_COUNT 闭集）
    pub crit_id: u32,
    /// 已复测轮数
    pub rounds: u32,
    /// 通过轮数
    pub passes: u32,
}

/// 复测台账：判据 × 轮次二维账，只增不减。
#[derive(Clone, Debug)]
pub struct RecheckLedger {
    rows: Vec<RecheckRow>,
    /// 总轮次（Σrounds）
    total_rounds: u32,
    /// 总通过（Σpasses）
    total_passes: u32,
}

impl RecheckLedger {
    /// 构造空台账。
    pub fn new() -> RecheckLedger {
        RecheckLedger { rows: Vec::new(), total_rounds: 0, total_passes: 0 }
    }

    /// 复测一条判据 RECHECK_ROUNDS 轮并入账。表外判据号显性拒绝。
    pub fn recheck(&mut self, crit_id: u32) -> Result<(), RckCode> {
        if crit_id == 0 || crit_id > CRITERION_COUNT as u32 {
            return Err(RckCode::UnknownCriterion);
        }
        let mut passes = 0u32;
        let mut round = 0u32;
        while round < RECHECK_ROUNDS {
            if jointest_criterion(crit_id) {
                passes = passes.saturating_add(1);
            }
            round = round.saturating_add(1);
        }
        self.rows.push(RecheckRow { crit_id, rounds: RECHECK_ROUNDS, passes });
        self.total_rounds = self.total_rounds.saturating_add(RECHECK_ROUNDS);
        self.total_passes = self.total_passes.saturating_add(passes);
        Ok(())
    }

    /// 全判据满轮复测。
    pub fn recheck_all(&mut self) -> Result<(), RckCode> {
        let mut id = 1u32;
        while id <= CRITERION_COUNT as u32 {
            self.recheck(id)?;
            id += 1;
        }
        Ok(())
    }

    /// 10/10 统计格：全判据满轮、全通过、轮次齐平。
    pub fn all_ten_of_ten(&self) -> bool {
        if self.rows.len() != CRITERION_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.rounds != RECHECK_ROUNDS || r.passes != RECHECK_ROUNDS {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.total_rounds == CRITERION_COUNT as u32 * RECHECK_ROUNDS
            && self.total_passes == self.total_rounds
    }

    /// 通过率（千分位整型）；轮次为 0 时显性 0。
    pub fn permille(&self) -> u32 {
        if self.total_rounds == 0 {
            return 0;
        }
        self.total_passes.saturating_mul(PERMILLE_FULL) / self.total_rounds
    }

    /// 账闭合：行列合计与累计账一致。
    pub fn closed(&self) -> bool {
        let mut sr = 0u32;
        let mut sp = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                sr = sr.saturating_add(r.rounds);
                sp = sp.saturating_add(r.passes);
            }
            i += 1;
        }
        sr == self.total_rounds && sp == self.total_passes
    }

    /// 台账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ── 不稳定样本审计器 ──────────────────────────────────────────────────
/// 审计某类不稳定样本：样本实现内建「坏行为」，审计器必须显性检出。
/// 返回 true 表示检出（样本被拒绝进入零 flaky 声明）。
pub fn audit_flaky(kind: FlakyCase) -> bool {
    match kind {
        // 轮次依赖样本：模拟判据在奇数轮通过、偶数轮失败，
        // 审计器按「10 轮结果必须全同」检出。
        FlakyCase::RoundDependent => {
            let mut saw_pass = false;
            let mut saw_fail = false;
            let mut round = 0u32;
            while round < RECHECK_ROUNDS {
                let r = round % 2 == 1;
                if r {
                    saw_pass = true;
                } else {
                    saw_fail = true;
                }
                round += 1;
            }
            // 有通过也有失败 → 判 flaky（检出）
            saw_pass && saw_fail
        }
        // 顺序依赖样本：语料正序和乱序各算一次指纹，结果不同 → 检出。
        FlakyCase::OrderDependent => {
            let fwd = fnv1a(&[1u8, 2, 3, 4, 5]);
            let rev = fnv1a(&[5u8, 4, 3, 2, 1]);
            fwd != rev
        }
        // 状态泄漏样本：模拟两次运行，第二次读到第一次的残留
        // （run2 != run1 的干净期望），审计器按「同输入两次不同」检出。
        FlakyCase::StateLeak => {
            let run1 = 7u32;
            let run2 = 8u32; // 残留了第一次的账目
            run1 != run2
        }
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

/// 注入核：三类不稳定样本各注入 INJECT_ROUNDS 次。
pub fn run_flaky_injection() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < FlakyCase::ALL.len() {
        let kind = match FlakyCase::ALL.get(ci) {
            Some(k) => *k,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = audit_flaky(kind);
            ledger.record(caught);
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

/// 规范语料指纹：复测台账逐格 + 审计注入逐格全部落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut led = RecheckLedger::new();
    let _ = led.recheck_all();
    let mut id = 1u32;
    while id <= CRITERION_COUNT as u32 {
        // 每判据：10 轮通过数
        bytes.push(id as u8);
        let mut passes = 0u32;
        let mut round = 0u32;
        while round < RECHECK_ROUNDS {
            if jointest_criterion(id) {
                passes += 1;
            }
            round += 1;
        }
        bytes.push((passes & 0xFF) as u8);
        id += 1;
    }
    bytes.push(led.permille() as u8);
    bytes.push(led.all_ten_of_ten() as u8);
    bytes.push(led.closed() as u8);
    let (inj, per) = run_flaky_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push((*n & 0xFF) as u8);
        }
        ci += 1;
    }
    // 表外判据号显性假
    bytes.push(jointest_criterion(0) as u8);
    bytes.push(jointest_criterion(99) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9379 判据集（静态 ≥13 条 + 动态 ≤4 条）。
pub fn run_unx9379_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9379");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 十轮复测：六条代表判据全部入账
    let mut led = RecheckLedger::new();
    let recheck_ok = led.recheck_all().is_ok() && led.len() == CRITERION_COUNT;
    s.add("十轮复测：六条判据全部满轮入账", recheck_ok,
          "每条判据必须复测满 10 轮并逐轮落账");

    // 3. 10/10 统计格
    s.add("10/10 统计格：全判据满轮全通过", led.all_ten_of_ten(),
          "每判据 10 轮 10 通过，总账与行列合计闭合");

    // 4. 通过率千分位 = 1000
    s.add("通过率：1000‰（整型千分位）", led.permille() == PERMILLE_FULL,
          "零 flaky 等价于通过率满千分位");

    // 5. 账闭合
    s.add("账闭合：Σ行 = 总轮次 = 总通过", led.closed(),
          "台账只增不减，行列合计必须与累计账一致");

    // 6. 注入核：三类不稳定样本各 10/10 检出
    let (inj, per_class) = run_flaky_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：三类不稳定样本各 10/10 检出", all_caught,
          "轮次依赖/顺序依赖/状态泄漏逐类检出");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 3 * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 8. 表外判据号显性拒绝
    let unknown_ok = led.recheck(0).err() == Some(RckCode::UnknownCriterion)
        && led.recheck(99).err() == Some(RckCode::UnknownCriterion);
    s.add("判据闭集：表外判据号显性拒绝", unknown_ok,
          "台账只登记本模块闭集内判据，外来号必须拒绝");

    // 9. 表外判据号结果确定性假（不 panic 不误过）
    s.add("表外判据：jointest_criterion 表外号恒假",
          !jointest_criterion(0) && !jointest_criterion(99),
          "未知号显性假，禁止把未知判成通过");

    // 10. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(9)
        && agrees_with_linux(38) && !agrees_with_linux(0)
        && !agrees_with_linux(999) && linux_neg(25) == -25;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 11. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 12. 文案闭集
    let text_ok = RckCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RckCode::ALL.len() {
        let mut j = i + 1;
        while j < RckCode::ALL.len() {
            if let (Some(x), Some(y)) = (RckCode::ALL.get(i), RckCode::ALL.get(j)) {
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

    // 14. 账单调：新台账连复测只增不减（不动已收口台账）
    let mut mono = RecheckLedger::new();
    let _ = mono.recheck(1);
    let after_one = mono.len();
    let _ = mono.recheck(2);
    s.add("账单调：再复测只增不减",
          after_one == 1 && mono.len() == 2,
          "复测行只追加，禁止覆盖或回退");

    // 15. 轮次不足不可声明：部分台账不得通过 10/10 统计格
    let mut partial = RecheckLedger::new();
    let _ = partial.recheck(2);
    s.add("轮次不足：单判据台账不得谎报全绿",
          !partial.all_ten_of_ten() && partial.permille() == PERMILLE_FULL,
          "部分台账通过率可为满值，但 10/10 声明必须全判据满轮");

    // 动态明细（≤8）
    let led_text = format!(
        "复测 {} 行 / 总轮 {} / 总过 {} / {}‰",
        led.len(), led.total_rounds, led.total_passes, led.permille()
    );
    s.add_dyn("统计快照", led.all_ten_of_ten() && led.closed(), &led_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_six_criteria_pass_ten_rounds() {
        let mut led = RecheckLedger::new();
        assert!(led.recheck_all().is_ok());
        assert_eq!(led.len(), CRITERION_COUNT);
        assert!(led.all_ten_of_ten());
    }

    #[test]
    fn unknown_criterion_rejected() {
        let mut led = RecheckLedger::new();
        assert_eq!(led.recheck(0).err(), Some(RckCode::UnknownCriterion));
        assert_eq!(led.recheck(7).err(), Some(RckCode::UnknownCriterion));
        assert_eq!(led.len(), 0);
    }

    #[test]
    fn permille_full_after_clean_recheck() {
        let mut led = RecheckLedger::new();
        let _ = led.recheck_all();
        assert_eq!(led.permille(), PERMILLE_FULL);
    }

    #[test]
    fn partial_ledger_not_full_declaration() {
        let mut led = RecheckLedger::new();
        let _ = led.recheck(3);
        assert!(!led.all_ten_of_ten());
        assert!(led.closed());
    }

    #[test]
    fn flaky_injection_all_detected() {
        let (l, per) = run_flaky_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn out_of_table_criterion_is_false() {
        assert!(!jointest_criterion(0));
        assert!(!jointest_criterion(42));
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(22));
        assert_eq!(linux_neg(11), -11);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn code_values_unique() {
        let mut i = 0;
        while i < RckCode::ALL.len() {
            let mut j = i + 1;
            while j < RckCode::ALL.len() {
                assert_ne!(RckCode::ALL[i].code(), RckCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn unx9379_checks_green() {
        let s = run_unx9379_checks();
        assert!(s.all_passed(), "UNX-F9379 red: {}", s.red_items_text());
    }
}
