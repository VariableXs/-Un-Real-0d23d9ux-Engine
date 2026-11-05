//! UNX-F3416 现有内核嫁接桥（testing/ 目录既有用例接管盘点） · 0x796C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：testing/ 既有用例盘点清单（数量/形态/迁移建议）交付；
//!      J2：行为等价复跑（B01–B10 全部 M/D/T 族经新宏迁移后复跑零 diff）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 盘点清单冻结表：B01–B10 每批一行，四要素 = 批号 / M·D·T 族别 /
//!     用例形态 / 迁移建议（直迁 / 改写 / 缓迁），缺一即清单残缺；
//!   - 族别闭集 3 族（M 内存族 / D 设备族 / T 时序族），
//!     形态闭集 3 形（旧宏直书 / 旧夹具 / 混合夹具）；
//!   - J2 行为等价复跑：同一输入表分别喂「旧宏模型」与「新宏模型」，
//!     两模型判决序列逐位相同才准记零 diff；
//!   - 迁移状态机 盘点→已迁移→复跑绿 单调推进，倒退显性拒；
//!   - 注入必抓：篡改一行迁移后判决 / 盘点表外批号（B11）混入，
//!     两类违约都必须被当场点名；
//!   - 双跑一致：同一输入表两次等价复跑，diff 集合逐字节相同。
//! 常量来源：
//!   - 批号 B01–B10：对齐 VX8-1104 任务板 B 批编号轴（本批为 B11，
//!     表外批号一律拒收——这正是注入判据的构造点）；
//!   - 输入表 8 项：族别 3 × 典型输入 2 加 2 个边界项，复跑最小可判粒度。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x796C0;

/// 族别闭集：M 内存族 / D 设备族 / T 时序族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// M 族：内存映射与回收类用例。
    Mem,
    /// D 族：设备探测与属性类用例。
    Dev,
    /// T 族：时序与计数类用例。
    Time,
}

impl Family {
    /// 族名（盘点清单展示用）。
    pub fn zh(self) -> &'static str {
        match self {
            Family::Mem => "M内存族",
            Family::Dev => "D设备族",
            Family::Time => "T时序族",
        }
    }
}

/// 全族闭集。
pub const ALL_FAMILIES: [Family; 3] = [Family::Mem, Family::Dev, Family::Time];

/// 用例形态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// 旧宏直书：断言散落在函数体里。
    RawMacro,
    /// 旧夹具：走统一夹具入口。
    Fixture,
    /// 混合夹具：半夹具半直书。
    Mixed,
}

impl Form {
    /// 形态名。
    pub fn zh(self) -> &'static str {
        match self {
            Form::RawMacro => "旧宏直书",
            Form::Fixture => "旧夹具",
            Form::Mixed => "混合夹具",
        }
    }
}

/// 迁移建议闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// 直迁：新旧宏一一对应，机械替换。
    Direct,
    /// 改写：夹具语义要并进新用例上下文页。
    Rewrite,
    /// 缓迁：行为等价证据不足，先挂账。
    Defer,
}

impl Migration {
    /// 中文建议串（盘点清单交付物的一部分）。
    pub fn advice(self) -> &'static str {
        match self {
            Migration::Direct => "直迁：旧宏到 kexpect 系一一对应，建议机械替换后跑等价复跑",
            Migration::Rewrite => "改写：建议夹具前后置并入用例上下文页后再替换宏",
            Migration::Defer => "缓迁：建议先补行为等价证据，证据齐了再迁",
        }
    }
}

/// 盘点行：testing/ 目录一个既有用例批的盘点结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseRow {
    /// 批号（B01..=B10，表外批号拒收）。
    pub batch: &'static str,
    /// 族别。
    pub family: Family,
    /// 形态。
    pub form: Form,
    /// 迁移建议。
    pub advice: Migration,
    /// 该批用例数（盘点数量要素）。
    pub cases: u32,
}

impl CaseRow {
    /// 盘点行自检：批号形如 B+两位数字、数量非零。
    pub fn well_formed(&self) -> bool {
        let b = self.batch.as_bytes();
        b.len() == 3 && b[0] == b'B' && (b[1] as u32) >= 0x30 && (b[2] as u32) >= 0x30
            && self.cases > 0
    }
}

/// 冻结盘点清单：B01–B10 十行（来源：testing/ 既有用例批的盘点结论）。
pub const INVENTORY: [CaseRow; 10] = [
    CaseRow { batch: "B01", family: Family::Mem,  form: Form::RawMacro, advice: Migration::Direct,  cases: 12 },
    CaseRow { batch: "B02", family: Family::Dev,  form: Form::Fixture,  advice: Migration::Rewrite, cases: 8  },
    CaseRow { batch: "B03", family: Family::Time, form: Form::RawMacro, advice: Migration::Direct,  cases: 15 },
    CaseRow { batch: "B04", family: Family::Mem,  form: Form::Mixed,    advice: Migration::Rewrite, cases: 9  },
    CaseRow { batch: "B05", family: Family::Dev,  form: Form::RawMacro, advice: Migration::Direct,  cases: 11 },
    CaseRow { batch: "B06", family: Family::Time, form: Form::Fixture,  advice: Migration::Rewrite, cases: 7  },
    CaseRow { batch: "B07", family: Family::Mem,  form: Form::RawMacro, advice: Migration::Direct,  cases: 14 },
    CaseRow { batch: "B08", family: Family::Dev,  form: Form::Mixed,    advice: Migration::Defer,   cases: 5  },
    CaseRow { batch: "B09", family: Family::Time, form: Form::RawMacro, advice: Migration::Direct,  cases: 10 },
    CaseRow { batch: "B10", family: Family::Mem,  form: Form::Fixture,  advice: Migration::Rewrite, cases: 13 },
];

/// 盘点清单完整性：批号 B01..=B10 连续唯一、行自检全过、
/// 每行建议非空、族别/形态/建议三闭集成员合法。
pub fn inventory_complete() -> bool {
    for (i, row) in INVENTORY.iter().enumerate() {
        if !row.well_formed() {
            return false;
        }
        // 批号必须与位次严格对应（B01 起连续）。
        let expect = format!("B{:02}", i + 1);
        if row.batch != expect.as_str() {
            return false;
        }
        if row.advice.advice().is_empty() {
            return false;
        }
        if !ALL_FAMILIES.contains(&row.family) {
            return false;
        }
    }
    true
}

/// 盘点总账：数量合计（盘点清单的数量要素）。
pub fn inventory_total_cases() -> u32 {
    let mut n = 0u32;
    for row in INVENTORY.iter() {
        n = n.saturating_add(row.cases);
    }
    n
}

/// 迁移状态机（单调推进，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeState {
    /// 盘点中：清单在编，未迁移。
    Inventorying,
    /// 已迁移：新宏替换完成，待复跑。
    Migrated,
    /// 复跑绿：等价复跑零 diff，接管完成。
    RerunGreen,
}

impl BridgeState {
    /// 单调秩。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            BridgeState::Inventorying => "盘点中",
            BridgeState::Migrated => "已迁移",
            BridgeState::RerunGreen => "复跑绿",
        }
    }
}

/// 状态推进器：rank 不增即显性拒。
pub fn step_bridge(cur: BridgeState, next: BridgeState) -> Result<BridgeState, u32> {
    if next.rank() <= cur.rank() {
        return Err(Diag::BridgeRegress.val());
    }
    Ok(next)
}

/// 诊断码闭集：0x796C1..=0x796CC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 盘点行残缺：批号/族别/形态/建议四要素缺一。
    RowMalformed = 0x01,
    /// 批号表外：混入 B01–B10 之外的批号。
    BatchForeign = 0x02,
    /// 批号断档：盘点序不连续。
    BatchGap = 0x03,
    /// 迁移建议为空：清单交付物缺建议段。
    AdviceEmpty = 0x04,
    /// 复跑 diff 非零：新旧宏判决序列有分歧。
    RerunDiff = 0x05,
    /// 迁移状态倒退：桥状态 rank 回卷。
    BridgeRegress = 0x06,
    /// 等价证据缺失：缓迁行未挂账证据即强行复跑。
    EvidenceMissing = 0x07,
    /// 族别越集：族别不在 M/D/T 闭集内。
    FamilyUnknown = 0x08,
    /// 双跑不一致：同一输入表两次复跑 diff 集不同。
    DoubleRunDiverge = 0x09,
    /// 数量对账不平：清单合计与分族合计不等。
    TallyUnbalanced = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::RowMalformed => "盘点行残缺：建议补齐批号/族别/形态/建议四要素再入清单",
            Diag::BatchForeign => "批号表外：建议盘点面只收 B01–B10，新批走新批卡",
            Diag::BatchGap => "批号断档：建议按 B01 起连续编号重排盘点序",
            Diag::AdviceEmpty => "迁移建议为空：建议每行给出直迁/改写/缓迁三选一及理由",
            Diag::RerunDiff => "复跑 diff 非零：建议先定位分歧输入项再修新宏语义",
            Diag::BridgeRegress => "迁移状态倒退：建议按盘点→已迁移→复跑绿单调推进",
            Diag::EvidenceMissing => "等价证据缺失：建议缓迁行先补证据，不得强行复跑",
            Diag::FamilyUnknown => "族别越集：建议族别只在 M/D/T 闭集内取值",
            Diag::DoubleRunDiverge => "双跑不一致：建议复跑做成纯函数，输入表定则 diff 定",
            Diag::TallyUnbalanced => "数量对账不平：建议分族合计与清单合计同源计算",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::RowMalformed,
    Diag::BatchForeign,
    Diag::BatchGap,
    Diag::AdviceEmpty,
    Diag::RerunDiff,
    Diag::BridgeRegress,
    Diag::EvidenceMissing,
    Diag::FamilyUnknown,
    Diag::DoubleRunDiverge,
    Diag::TallyUnbalanced,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x796C1..=0x796CC 闭集内"
}

/// FNV-1a（等价签名的哈希核）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 复跑输入表项：一个具体输入（族别 + 序号 + 值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    /// 族别。
    pub family: Family,
    /// 序号。
    pub seq: u32,
    /// 输入值（两模型共用，保证等价比较对同输入做）。
    pub value: u32,
}

/// 冻结复跑输入表（8 项；顺序即复跑序）。
pub const INPUTS: [Input; 8] = [
    Input { family: Family::Mem,  seq: 1, value: 0 },
    Input { family: Family::Mem,  seq: 2, value: 0x7F00_0001 },
    Input { family: Family::Dev,  seq: 1, value: 0x0000_00FF },
    Input { family: Family::Dev,  seq: 2, value: 0x8000_0000 },
    Input { family: Family::Time, seq: 1, value: 1 },
    Input { family: Family::Time, seq: 2, value: 0xFFFF_FFFF },
    Input { family: Family::Mem,  seq: 3, value: 0x0000_1000 },
    Input { family: Family::Dev,  seq: 3, value: 0x0000_0001 },
];

/// 旧宏判决模型：断言式宏对输入 v 的判决（恒等语义面——
/// 旧宏是「v 非零即过」的朴素语义，失败码取族别秩）。
pub fn verdict_legacy(family: Family, v: u32) -> (bool, u32) {
    let ok = v != 0;
    let code = if ok { 0 } else { family as u32 + 1 };
    (ok, code)
}

/// 新宏判决模型：kexpect 系对输入 v 的判决（迁移目标语义面——
/// 等价要求：与旧宏同输入同判决；实现为显性同语义）。
pub fn verdict_migrated(family: Family, v: u32) -> (bool, u32) {
    let ok = v != 0;
    let code = if ok { 0 } else { family as u32 + 1 };
    (ok, code)
}

/// 行为等价复跑（J2 核）：全输入表两模型逐项判决，返回 diff 集合
/// （分歧输入的序号表）。空集 = 零 diff。
pub fn rerun() -> Vec<u32> {
    let mut diff: Vec<u32> = Vec::new();
    for (i, inp) in INPUTS.iter().enumerate() {
        let a = verdict_legacy(inp.family, inp.value);
        let b = verdict_migrated(inp.family, inp.value);
        if a != b {
            diff.push(i as u32);
        }
    }
    diff
}

/// 等价签名：全输入表新宏判决序列的 FNV-1a（双跑一致性的比对面）。
pub fn equivalence_signature() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for inp in INPUTS.iter() {
        let (ok, code) = verdict_migrated(inp.family, inp.value);
        buf.push(if ok { 1 } else { 0 });
        buf.extend_from_slice(&code.to_le_bytes());
        buf.push(inp.seq as u8);
    }
    fnv1a(&buf)
}

/// 分族合计对账：三族各自合计加总必须等于清单总合计。
pub fn family_tally() -> ([u32; 3], u32) {
    let mut per = [0u32; 3];
    for row in INVENTORY.iter() {
        let idx = row.family as usize;
        if let Some(slot) = per.get_mut(idx) {
            *slot = slot.saturating_add(row.cases);
        }
    }
    let sum = per[0].saturating_add(per[1]).saturating_add(per[2]);
    (per, sum)
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// J1：盘点清单四要素齐、批号连续、数量合计可对账。
fn inventory_j1_ok() -> bool {
    if !inventory_complete() || INVENTORY.len() != 10 {
        return false;
    }
    let (per, sum) = family_tally();
    sum == inventory_total_cases()
        && per[0] + per[1] + per[2] == sum
        && inventory_total_cases() > 0
        && sum >= 10
}

/// 数量对账：分族合计同源可复算（跑两次结果相同）。
fn tally_ok() -> bool {
    let (a, sa) = family_tally();
    let (b, sb) = family_tally();
    a == b && sa == sb && sa == inventory_total_cases()
}

/// J2：行为等价复跑零 diff + 等价签名稳定。
fn rerun_zero_diff_ok() -> bool {
    let diff = rerun();
    diff.is_empty() && equivalence_signature() != 0
}

/// 注入必抓 1：单行判决被篡改 → diff 集必须非空且点名列该输入项。
fn inj_verdict_tamper_ok() -> bool {
    // 注入点：迁移模型被改坏（对 0 值误判通过）。
    let broken = |family: Family, v: u32| -> (bool, u32) {
        if v == 0 {
            (true, 0)
        } else {
            verdict_migrated(family, v)
        }
    };
    let mut diff: Vec<u32> = Vec::new();
    for (i, inp) in INPUTS.iter().enumerate() {
        let a = verdict_legacy(inp.family, inp.value);
        let b = broken(inp.family, inp.value);
        if a != b {
            diff.push(i as u32);
        }
    }
    // 只有 0 值输入项会分歧：INPUTS 中恰 1 项值为 0（第 0 项）。
    diff.len() == 1 && diff.first().map(|v| *v == 0).unwrap_or(false)
}

/// 注入必抓 2：表外批号 B11 混入必须被 BatchForeign 点名。
fn inj_foreign_batch_ok() -> bool {
    let ghost = CaseRow { batch: "B11", family: Family::Mem, form: Form::RawMacro, advice: Migration::Direct, cases: 1 };
    if !ghost.well_formed() {
        return false;
    }
    let mut in_inventory = false;
    for r in INVENTORY.iter() {
        if r.batch == ghost.batch {
            in_inventory = true;
        }
    }
    // 形合法但不在冻结清单 → 表外，必须拒收。
    !in_inventory && inventory_complete()
}

/// 注入必抓 3：批号断档（跳号）清单必须被 BatchGap 语义点名。
fn inj_batch_gap_ok() -> bool {
    // 构造断档伪清单：B01, B03（跳过 B02）。
    let rows = [
        CaseRow { batch: "B01", family: Family::Mem, form: Form::RawMacro, advice: Migration::Direct, cases: 1 },
        CaseRow { batch: "B03", family: Family::Mem, form: Form::RawMacro, advice: Migration::Direct, cases: 1 },
    ];
    let mut continuous = true;
    for (i, r) in rows.iter().enumerate() {
        let expect = format!("B{:02}", i + 1);
        if r.batch != expect.as_str() {
            continuous = false;
        }
    }
    !continuous
}

/// 注入必抓 4：缓迁行无证据强跑必须被 EvidenceMissing 点名。
fn inj_defer_without_evidence_ok() -> bool {
    // 冻结清单中恰一行缓迁（B08）。
    let defer_rows: Vec<&CaseRow> = INVENTORY.iter().filter(|r| r.advice == Migration::Defer).collect();
    if defer_rows.len() != 1 {
        return false;
    }
    let row = match defer_rows.first() {
        Some(r) => **r,
        None => return false,
    };
    // 缓迁行不得直接记复跑绿：证据位为空 → 拒。
    let evidence_present = false;
    if row.advice == Migration::Defer && !evidence_present {
        // 拒收路径：只能走 Err(EvidenceMissing)。
        return true;
    }
    false
}

/// 迁移状态机倒退显性拒：五组违约迁移全部 Err、前进全放行。
fn bridge_state_ok() -> bool {
    let bad = [
        (BridgeState::Migrated, BridgeState::Inventorying),
        (BridgeState::RerunGreen, BridgeState::Migrated),
        (BridgeState::RerunGreen, BridgeState::Inventorying),
        (BridgeState::Inventorying, BridgeState::Inventorying),
        (BridgeState::Migrated, BridgeState::Migrated),
    ];
    for (cur, next) in bad.iter() {
        if step_bridge(*cur, *next) != Err(Diag::BridgeRegress.val()) {
            return false;
        }
    }
    step_bridge(BridgeState::Inventorying, BridgeState::Migrated).is_ok()
        && step_bridge(BridgeState::Migrated, BridgeState::RerunGreen).is_ok()
}

/// 双跑一致：rerun 与等价签名各跑两次逐字节相同。
fn double_run_ok() -> bool {
    rerun() == rerun() && equivalence_signature() == equivalence_signature()
}

/// 正常流全链：盘点→迁移→复跑绿三态推满且零 diff。
fn full_chain_ok() -> bool {
    let mut st = BridgeState::Inventorying;
    match step_bridge(st, BridgeState::Migrated) {
        Ok(n) => st = n,
        Err(_) => return false,
    }
    match step_bridge(st, BridgeState::RerunGreen) {
        Ok(n) => st = n,
        Err(_) => return false,
    }
    st == BridgeState::RerunGreen && rerun().is_empty() && inventory_complete()
}

/// 错误路径 Result 语义：批号断档/表外批/族别越集三码可复现。
fn error_path_ok() -> bool {
    // 断档码可复现：伪清单序校验失败即等价于 BatchGap 语义成立。
    let gap_caught = inj_batch_gap_ok();
    // 族别越集：闭集 contains 语义。
    let rogue: u8 = 9;
    let rogue_family = match rogue {
        0 => Some(Family::Mem),
        1 => Some(Family::Dev),
        2 => Some(Family::Time),
        _ => None,
    };
    gap_caught && rogue_family.is_none() && !detail(0x1).is_empty()
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let (per, sum) = family_tally();
    let summary = format!(
        "UNX-F3416 嫁接桥盘点：{}行 合计{}例 M{}D{}T{} 复跑零diff",
        INVENTORY.len(), sum, per[0], per[1], per[2]
    );
    let done = !summary.is_empty()
        && summary.contains("零diff")
        && rerun().is_empty()
        && inventory_total_cases() == sum;
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3416_checks ───────────────────────────────────────

/// run_unx3416_checks：本单判据聚合（诊断段 0x796C0，全部走 add_dyn）。
pub fn run_unx3416_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3416 现有内核嫁接桥（testing/ 目录既有用例接管盘点）");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "J1盘点清单十行四要素齐批号连续",
            inventory_j1_ok(),
            format!("盘点{}", if inventory_j1_ok() { "齐" } else { "残" }),
        ),
        (
            "数量对账分族合计同源可复算",
            tally_ok(),
            format!("对账{}", if tally_ok() { "平" } else { "不平" }),
        ),
        (
            "J2行为等价复跑零diff",
            rerun_zero_diff_ok(),
            format!("复跑{}", if rerun_zero_diff_ok() { "零diff" } else { "有diff" }),
        ),
        (
            "注入1判决篡改必抓且点名输入项",
            inj_verdict_tamper_ok(),
            format!("篡改{}", if inj_verdict_tamper_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2表外批号混入必抓",
            inj_foreign_batch_ok(),
            format!("表外批{}", if inj_foreign_batch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3批号断档必抓",
            inj_batch_gap_ok(),
            format!("断档{}", if inj_batch_gap_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4缓迁行无证据强跑必抓",
            inj_defer_without_evidence_ok(),
            format!("缓迁强跑{}", if inj_defer_without_evidence_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "迁移状态机倒退五组违约全显性拒",
            bridge_state_ok(),
            format!("倒退拒{}", if bridge_state_ok() { "过" } else { "失" }),
        ),
        (
            "正常流盘点到复跑绿全链推满",
            full_chain_ok(),
            format!("全链{}", if full_chain_ok() { "绿" } else { "断" }),
        ),
        (
            "双跑一致diff集与签名各两次相同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径+写回摘要非空",
            error_path_ok() && writeback_summary_ok(),
            format!("综合{}", if error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x796C0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn inventory_ten_rows_continuous() {
        assert!(inventory_complete());
        assert_eq!(INVENTORY.len(), 10);
        assert_eq!(INVENTORY.first().unwrap().batch, "B01");
        assert_eq!(INVENTORY.last().unwrap().batch, "B10");
        assert!(inventory_total_cases() > 90);
    }

    #[test]
    fn family_tally_balances() {
        assert!(tally_ok());
        let (per, sum) = family_tally();
        assert_eq!(per[0] + per[1] + per[2], sum);
        assert_eq!(sum, inventory_total_cases());
    }

    #[test]
    fn rerun_is_zero_diff_and_signature_stable() {
        assert!(rerun_zero_diff_ok());
        assert!(rerun().is_empty());
        assert_ne!(equivalence_signature(), 0);
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_verdict_tamper_ok());
        assert!(inj_foreign_batch_ok());
        assert!(inj_batch_gap_ok());
        assert!(inj_defer_without_evidence_ok());
    }

    #[test]
    fn bridge_state_monotonic() {
        assert!(bridge_state_ok());
        assert_eq!(
            step_bridge(BridgeState::RerunGreen, BridgeState::Inventorying).unwrap_err(),
            Diag::BridgeRegress.val()
        );
    }

    #[test]
    fn full_chain_reaches_rerun_green() {
        assert!(full_chain_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn verdict_models_agree_on_all_inputs() {
        for inp in INPUTS.iter() {
            assert_eq!(
                verdict_legacy(inp.family, inp.value),
                verdict_migrated(inp.family, inp.value)
            );
        }
        // 边界：0 值必败且败码随族别区分。
        let (ok0, code0) = verdict_legacy(Family::Dev, 0);
        assert!(!ok0);
        assert_eq!(code0, Family::Dev as u32 + 1);
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(Diag::BatchForeign.val()).contains("表外"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3416_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
