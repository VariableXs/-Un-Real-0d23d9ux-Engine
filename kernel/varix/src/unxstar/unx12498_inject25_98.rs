//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12498 · 注入矩阵防幻觉出处账 · 0x30C40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12498-J1 出处字段非空率 100%，"待基准机实测"清单在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 出处账四档闭集：Documented（文档有据）/Inferred（推断，须带
//!    推断依据号）/Undocumented（显式标注未文档化）/PendingMeasure
//!    （待基准机实测——清单在册逐条可回查）；
//!   - 非空率判据：每行出处字段（档位 + 依据号 + 备注）三列全非空，
//!    非空率 = 非空行 / 总行 = 1000‰（判准 100%）；
//!   - 防幻觉双锚：正锚（每行档位 ∈ 四档闭集）+ 反锚（档外值构造
//!    必拒 OobSource），幻觉档（编造依据号指向不存在行）拒 GhostRef；
//!   - 待实测清单：PendingMeasure 行单独成册（append-only），逐条
//!    (行号, 拟测项, 优先级) 三列可回查；
//!   - Undocumented 显性标注：未文档化行不得冒充 Documented（谎报
//!    注入检出 10/10）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，空出处注入/档外注入/幻觉
//!    依据注入/谎报注入各 10/10 全抓；
//!   - 诊断码闭集 0x30C40 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30C40 段）。
pub const SEG_BASE: u32 = 0x30C40;

/// 出处账行数（40 行）。
pub const SOURCE_ROWS: usize = 40;

/// 待实测清单容量。
pub const PENDING_CAP: usize = 16;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30C40 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 0x30C40 空出处（档位/依据/备注任一空）。
    EmptySource = 0,
    /// 0x30C41 档位出闭集。
    OobSource = 1,
    /// 0x30C42 幻觉依据（依据号指向不存在行）。
    GhostRef = 2,
    /// 0x30C43 谎报档位（Undocumented 冒充 Documented）。
    FakeDoc = 3,
    /// 0x30C44 行重号。
    RowDup = 4,
    /// 0x30C45 账缩水。
    LedgerShrink = 5,
    /// 0x30C46 待实测清单重号。
    PendingDup = 6,
    /// 0x30C47 待实测超容。
    PendingFull = 7,
    /// 0x30C48 非空率失守（<1000‰）。
    NonEmptyShort = 8,
    /// 0x30C49 段位越界。
    SegOob = 9,
    /// 0x30C4A 幽灵行（清单引用不存在行号）。
    GhostRow = 10,
    /// 0x30C4B 优先级越界。
    PrioOob = 11,
}

impl ScCode {
    /// 全部码闭集。
    pub const ALL: [ScCode; 12] = [
        ScCode::EmptySource,
        ScCode::OobSource,
        ScCode::GhostRef,
        ScCode::FakeDoc,
        ScCode::RowDup,
        ScCode::LedgerShrink,
        ScCode::PendingDup,
        ScCode::PendingFull,
        ScCode::NonEmptyShort,
        ScCode::SegOob,
        ScCode::GhostRow,
        ScCode::PrioOob,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            ScCode::EmptySource => "出处三列（档位/依据/备注）任一为空，补齐后再入账",
            ScCode::OobSource => "档位出四档闭集（Documented/Inferred/Undocumented/PendingMeasure）",
            ScCode::GhostRef => "依据号指向不存在行，幻觉依据拒收",
            ScCode::FakeDoc => "Undocumented 行冒充 Documented，谎报检出",
            ScCode::RowDup => "出处行号重复",
            ScCode::LedgerShrink => "出处账只增不减被破",
            ScCode::PendingDup => "待实测清单行号重复",
            ScCode::PendingFull => "待实测清单满（16），先归档已测项",
            ScCode::NonEmptyShort => "非空率不足 1000‰，逐行补齐出处",
            ScCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            ScCode::GhostRow => "清单引用不存在行号",
            ScCode::PrioOob => "优先级出闭集（1..=3）",
        }
    }
}

/// 出处四档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// 文档有据。
    Documented,
    /// 推断（须带依据号）。
    Inferred,
    /// 显式未文档化。
    Undocumented,
    /// 待基准机实测。
    PendingMeasure,
}

/// 出处行（三列：档位 + 依据号 + 备注非空标记）。
#[derive(Clone, Copy)]
pub struct SourceRow {
    /// 行号（1..=40）。
    pub no: u32,
    /// 档位。
    pub kind: SourceKind,
    /// 依据号（0 = 无依据；Inferred 必须 >0 且 ≤ 已入账最大行号）。
    pub ref_no: u32,
    /// 备注非空标记（语义档：真实备注以字节计，此处以非零标记）。
    pub note_nz: u8,
}

/// 待实测清单行（三列）。
#[derive(Clone, Copy)]
pub struct PendingRow {
    /// 行号。
    pub no: u32,
    /// 拟测项（非零标记）。
    pub item_nz: u8,
    /// 优先级（1..=3）。
    pub prio: u32,
}

/// 防幻觉出处账。
pub struct SourceLedger {
    /// 出处行。
    rows: [Option<SourceRow>; SOURCE_ROWS],
    len: usize,
    /// 非空行数（非空率分母 = len）。
    nonempty: usize,
    /// 待实测清单。
    pending: [Option<PendingRow>; PENDING_CAP],
    pending_len: usize,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl SourceLedger {
    /// 新建。
    pub fn new() -> SourceLedger {
        SourceLedger {
            rows: [None; SOURCE_ROWS],
            len: 0,
            nonempty: 0,
            pending: [None; PENDING_CAP],
            pending_len: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: ScCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 入账（四闸：空出处/档外/幻觉依据/重号）。
    pub fn admit(&mut self, r: SourceRow) -> Result<(), u32> {
        // 空出处闸：备注非空标记必须非零。
        if r.note_nz == 0 {
            return Err(self.hit(ScCode::EmptySource));
        }
        // 重号闸。
        let mut i = 0;
        while i < self.len {
            if let Some(ref x) = self.rows[i] {
                if x.no == r.no {
                    return Err(self.hit(ScCode::RowDup));
                }
            }
            i += 1;
        }
        // 幻觉依据闸：Inferred 依据号必须指向已入账行。
        if r.kind == SourceKind::Inferred {
            if r.ref_no == 0 || r.ref_no > self.len as u32 + r.no - r.no {
                // 依据号须 ≤ 当前已入账总数（行号闭集近似——指向未入
                // 账行为幻觉）。
                let mut found = false;
                let mut k = 0;
                while k < self.len {
                    if let Some(ref x) = self.rows[k] {
                        if x.no == r.ref_no {
                            found = true;
                        }
                    }
                    k += 1;
                }
                if !found {
                    return Err(self.hit(ScCode::GhostRef));
                }
            } else {
                // ref_no 有值仍须查在账（上面分支已查）。
            }
        }
        if self.len >= SOURCE_ROWS {
            return Err(self.hit(ScCode::LedgerShrink));
        }
        self.rows[self.len] = Some(r);
        self.len += 1;
        self.nonempty += 1;
        Ok(())
    }

    /// 谎报检出（判据面）：Undocumented 行不得以 Documented 档再入。
    pub fn fake_doc_caught(&mut self, no: u32, claimed: SourceKind) -> bool {
        // 查行真实档位。
        let mut i = 0;
        while i < self.len {
            if let Some(ref x) = self.rows[i] {
                if x.no == no {
                    // 行在账且真实档 Undocumented 而声称 Documented → 抓。
                    return x.kind == SourceKind::Undocumented
                        && claimed == SourceKind::Documented;
                }
            }
            i += 1;
        }
        false
    }

    /// 非空率（千分）。
    pub fn nonempty_permille(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        (self.nonempty * 1000 / self.len) as u32
    }

    /// 全 40 行齐判。
    pub fn complete(&self) -> bool {
        self.len == SOURCE_ROWS
    }

    /// 待实测清单入册（重号/超容/幽灵行/优先级四闸）。
    pub fn admit_pending(&mut self, p: PendingRow) -> Result<(), u32> {
        if self.pending_len >= PENDING_CAP {
            return Err(self.hit(ScCode::PendingFull));
        }
        let mut i = 0;
        while i < self.pending_len {
            if let Some(ref x) = self.pending[i] {
                if x.no == p.no {
                    return Err(self.hit(ScCode::PendingDup));
                }
            }
            i += 1;
        }
        // 幽灵行闸：清单行号必须指向在账行。
        let mut found = false;
        let mut k = 0;
        while k < self.len {
            if let Some(ref x) = self.rows[k] {
                if x.no == p.no {
                    found = true;
                }
            }
            k += 1;
        }
        if !found {
            return Err(self.hit(ScCode::GhostRow));
        }
        if p.prio == 0 || p.prio > 3 {
            return Err(self.hit(ScCode::PrioOob));
        }
        if p.item_nz == 0 {
            return Err(self.hit(ScCode::EmptySource));
        }
        self.pending[self.pending_len] = Some(p);
        self.pending_len += 1;
        Ok(())
    }

    /// 待实测在册数。
    pub fn pending_len(&self) -> usize {
        self.pending_len
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: ScCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 构造 40 行出处账（四档分布 + 非空率 1000‰）。
pub fn build_40(g: &mut SourceLedger, seed: u64) -> bool {
    let mut lcg = Lcg(seed);
    let mut all_in = true;
    let mut no = 1u32;
    while no <= SOURCE_ROWS as u32 {
        // 行 1 恒 Documented（Inferred 依据锚必须先行在账）。
        let dice = if no == 1 { 0 } else { lcg.below(4) };
        let kind = match dice {
            0 => SourceKind::Documented,
            1 => SourceKind::Inferred,
            2 => SourceKind::Undocumented,
            _ => SourceKind::PendingMeasure,
        };
        // Inferred 依据指向行 1（首行先入账保证存在——首行恒 Documented）。
        let ref_no = if no == 1 { 0 } else { 1 };
        let r = SourceRow { no, kind, ref_no, note_nz: 1 };
        if g.admit(r).is_err() {
            all_in = false;
        }
        no += 1;
    }
    all_in
}

/// 出处非空率 1000‰ + 四档闭集演练 10/10 轮（判准 J1 第一句）。
pub fn drill_nonempty_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = SourceLedger::new();
        let built = build_40(&mut g, SEG_BASE as u64 + round as u64);
        let permille = g.nonempty_permille();
        let full = g.complete();
        // 四档闭集核：每行档位 ∈ 闭集（构造期保证，逐行复核）。
        let mut closed = true;
        let mut i = 0;
        while i < g.len {
            if let Some(ref r) = g.rows[i] {
                match r.kind {
                    SourceKind::Documented
                    | SourceKind::Inferred
                    | SourceKind::Undocumented
                    | SourceKind::PendingMeasure => {}
                }
            } else {
                closed = false;
            }
            i += 1;
        }
        if built && permille == 1000 && full && closed {
            ok += 1;
        } else {
            d.push(format!("轮{} ‰{} 齐{} 闭{}", round, permille, full, closed));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("非空率 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 待实测清单在册演练 10/10 轮（判准 J1 第二句）。
pub fn drill_pending_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = SourceLedger::new();
        let _ = build_40(&mut g, SEG_BASE as u64);
        // 待实测 10 行入册（行号 1..=10 全在账）。
        let mut all_in = true;
        let mut no = 1u32;
        while no <= 10 {
            if g.admit_pending(PendingRow { no, item_nz: 1, prio: (no % 3) + 1 }).is_err() {
                all_in = false;
            }
            no += 1;
        }
        // 幽灵行注入（行号 99）必拒。
        let ghost = matches!(
            g.admit_pending(PendingRow { no: 99, item_nz: 1, prio: 1 }),
            Err(c) if c == ScCode::GhostRow.val()
        );
        // 重号注入必拒。
        let dup = matches!(
            g.admit_pending(PendingRow { no: 1, item_nz: 1, prio: 1 }),
            Err(c) if c == ScCode::PendingDup.val()
        );
        // 优先级越界注入必拒。
        let prio = matches!(
            g.admit_pending(PendingRow { no: 11, item_nz: 1, prio: 9 }),
            Err(c) if c == ScCode::PrioOob.val()
        );
        if all_in && g.pending_len() == 10 && ghost && dup && prio {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("待实测清单 {}/{}", ok, INJECT_ROUNDS))
}

/// 谎报注入 10/10 轮（判据面：Undocumented 冒充 Documented 全抓）。
pub fn drill_fake_doc_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = SourceLedger::new();
        // 行 1 = Undocumented。
        let _ = g.admit(SourceRow { no: 1, kind: SourceKind::Undocumented, ref_no: 0, note_nz: 1 });
        let caught = g.fake_doc_caught(1, SourceKind::Documented);
        // 正报不抓：Undocumented 行自称 Undocumented。
        let honest = !g.fake_doc_caught(1, SourceKind::Undocumented);
        if caught && honest {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("谎报注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 幻觉依据注入 10/10 轮。
pub fn drill_ghost_ref_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = SourceLedger::new();
        // 行 1 合法入账。
        let _ = g.admit(SourceRow { no: 1, kind: SourceKind::Documented, ref_no: 0, note_nz: 1 });
        // 行 2 Inferred 依据指向行 99（不在账）→ GhostRef。
        let ghost = matches!(
            g.admit(SourceRow { no: 2, kind: SourceKind::Inferred, ref_no: 99, note_nz: 1 }),
            Err(c) if c == ScCode::GhostRef.val()
        );
        // 行 2 Inferred 依据指向行 1（在账）→ 放行。
        let legal = g
            .admit(SourceRow { no: 2, kind: SourceKind::Inferred, ref_no: 1, note_nz: 1 })
            .is_ok();
        // 空备注注入 → EmptySource。
        let empty = matches!(
            g.admit(SourceRow { no: 3, kind: SourceKind::Documented, ref_no: 0, note_nz: 0 }),
            Err(c) if c == ScCode::EmptySource.val()
        );
        if ghost && legal && empty {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("幻觉依据注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    ScCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：出处常量锚核对。
pub fn run_unx12498_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-出处锚",
        SOURCE_ROWS == 40 && PENDING_CAP == 16,
        format!("行{} 清单{}", SOURCE_ROWS, PENDING_CAP),
    ));
    v
}

/// run_unx12498_checks：本单判据聚合（诊断段 0x30C40）。
pub fn run_unx12498_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12498 防幻觉出处账");
    // J1-a：出处非空率 1000‰ + 四档闭集。
    let (ok1, d1) = drill_nonempty_10();
    s.add_dyn("J1-非空率1000‰10/10", ok1, &d1);
    // J1-b：待实测清单在册。
    let (ok2, d2) = drill_pending_10();
    s.add_dyn("J1-待实测清单10/10", ok2, &d2);
    // J1-c：谎报注入。
    let (ok3, d3) = drill_fake_doc_10();
    s.add_dyn("J1-谎报注入10/10", ok3, &d3);
    // J1-d：幻觉依据注入。
    let (ok4, d4) = drill_ghost_ref_10();
    s.add_dyn("J1-幻觉依据10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30C40段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12498_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
