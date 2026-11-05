//! UNX-F1599 域闭账恒等式（800 条=40 批×20=240,000 行三向恒等） · 0x793C0段
//!
//! 判据原文（UNX-F1599-J1 摘录）：「域闭账恒等式成立（条数 800=批数 40×20；
//! 行数 240,000=四段和；判据号连续 F0801–F1600 三向恒等脚本实核），恒等破坏
//! 注入 10/10 检出」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三向恒等硬口径：条数恒等（申报 800=实核 800=批数 40×20）/行数恒等
//!    （申报 240,000=实核 240,000=四段和）/判据号恒等（F0801..=F1600 连续
//!    覆盖无缺无重），三向逐向实核，恒等式全对齐才转平衡相位；
//! 2. 每批恒等子式：每批恰 20 条、批行数 6000（=20 条×300 行/条）、判据号
//!    起点 =801+(批序-1)×20，批内子式破即点出该批；
//! 3. 四段和口径：四十批按十批一段分四段，每段和恰 60,000，段和破即点出
//!    该段——行数守恒不靠申报自证，靠四段逐段实核；
//! 4. 恒等破坏注入 10/10：条数破/段行破/批数缺/批序乱/号起点错/申报条数
//!    破/申报行数破/空账/批数溢/另段行破十种注入，逐种必须被实核口检出；
//! 5. 相位机 起草中→已平衡 单向，未平衡先出闭账显性拒；
//! 6. 诊断码闭集 0x793C1..0x793CC，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x793C0;

/// 域判据总条数（判据原文 800）。
pub const COUNT_TOTAL: u32 = 800;

/// 域批数（判据原文 40 批）。
pub const BATCH_COUNT: usize = 40;

/// 每批条数（判据原文 20）。
pub const PER_BATCH: u32 = 20;

/// 域账总行数（判据原文 240,000）。
pub const LINE_TOTAL: u64 = 240_000;

/// 每条行数（240,000 = 800 条 × 300 行）。
pub const LINES_PER_ITEM: u64 = 300;

/// 每批行数（20 条 × 300 行 = 6,000）。
pub const BATCH_LINES: u64 = PER_BATCH as u64 * LINES_PER_ITEM;

/// 四段和每段行数（240,000 / 4 = 60,000）。
pub const SEGMENT_LINES: u64 = LINE_TOTAL / 4;

/// 每段批数（40 批 / 4 段）。
pub const SEGMENT_BATCHES: usize = BATCH_COUNT / 4;

/// 判据号下沿（F0801）。
pub const ID_MIN: u32 = 801;

/// 判据号上沿（F1600）。
pub const ID_MAX: u32 = 1600;

/// 恒等破坏注入种数（判据原文 10/10）。
pub const BREAK_PROBES: usize = 10;

/// 指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 域闭账恒等式诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（起草中→已平衡 单向）。
    EPhase,
    /// 批序断（批号不按 1..=40 递增落账）。
    ESeq,
    /// 空账（无批不构成闭账）。
    ENoBatch,
    /// 批条数非 20。
    ECount,
    /// 判据号起点错（起点 ≠801+(批序-1)×20）。
    EIdStart,
    /// 判据号覆盖断（末批尾号 ≠1600）。
    EIdGap,
    /// 批数非 40。
    ELack,
    /// 条数恒等破（申报≠实核或≠批数×每批）。
    ECountTotal,
    /// 行数恒等破（申报≠实核或≠240,000）。
    ELinesTotal,
    /// 四段和破（段和≠60,000）。
    ESegBroken,
    /// 未平衡先出闭账。
    EBalance,
    /// 申报缺失（条数/行数零申报）。
    EDeclare,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::ESeq => 0x02,
                Diag::ENoBatch => 0x03,
                Diag::ECount => 0x04,
                Diag::EIdStart => 0x05,
                Diag::EIdGap => 0x06,
                Diag::ELack => 0x07,
                Diag::ECountTotal => 0x08,
                Diag::ELinesTotal => 0x09,
                Diag::ESegBroken => 0x0a,
                Diag::EBalance => 0x0b,
                Diag::EDeclare => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：起草中→已平衡 单向，倒退即拒",
            Diag::ESeq => "批序断：批号按 1..=40 递增落账，先修批序再实核",
            Diag::ENoBatch => "空账：无批不构成闭账，先逐批入账",
            Diag::ECount => "批条数非 20：逐批核条数，破批先补齐 20 条",
            Diag::EIdStart => "判据号起点错：起点必须 =801+(批序-1)×20，逐批对号",
            Diag::EIdGap => "判据号覆盖断：末批尾号必须 =1600，先补覆盖再实核",
            Diag::ELack => "批数非 40：逐批补齐 40 批再实核恒等式",
            Diag::ECountTotal => "条数恒等破：申报条数、实核条数、批数×每批三向对齐",
            Diag::ELinesTotal => "行数恒等破：申报行数、实核行数、240,000 三向对齐",
            Diag::ESegBroken => "四段和破：每段和必须 60,000，破段逐批对行数",
            Diag::EBalance => "未平衡先出闭账：先过恒等实核转平衡，再出闭账",
            Diag::EDeclare => "申报缺失：条数与行数先申报再实核，零申报不核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::ESeq,
    Diag::ENoBatch,
    Diag::ECount,
    Diag::EIdStart,
    Diag::EIdGap,
    Diag::ELack,
    Diag::ECountTotal,
    Diag::ELinesTotal,
    Diag::ESegBroken,
    Diag::EBalance,
    Diag::EDeclare,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x793C1..0x793CC 闭集内"
}

/// 闭账相位（起草中→已平衡 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosurePhase {
    /// 起草中：批册入账与恒等实核中。
    Drafting,
    /// 已平衡：三向恒等全对齐。
    Balanced,
}

impl ClosurePhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            ClosurePhase::Drafting => 0,
            ClosurePhase::Balanced => 1,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ClosurePhase::Drafting => "起草中",
            ClosurePhase::Balanced => "已平衡",
        }
    }
}

/// 一批账（批号 + 条数 + 行数 + 判据号起点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchAccount {
    /// 批号（1..=40，递增落账）。
    pub batch_no: u32,
    /// 本批条数。
    pub items: u32,
    /// 本批行数。
    pub lines: u64,
    /// 本批判据号起点（F 起号）。
    pub id_start: u32,
}

impl BatchAccount {
    /// 批判据号尾号（起点 + 条数 - 1）。
    pub fn id_end(&self) -> u32 {
        self.id_start + self.items - 1
    }
}

/// 域闭账（申报 + 批册 + 相位）。
#[derive(Clone, Debug)]
pub struct DomainClosure {
    /// 申报条数。
    declared_count: u32,
    /// 申报行数。
    declared_lines: u64,
    batches: Vec<BatchAccount>,
    phase: ClosurePhase,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl DomainClosure {
    /// 新空闭账（起草中，零申报）。
    pub fn new() -> DomainClosure {
        DomainClosure {
            declared_count: 0,
            declared_lines: 0,
            batches: Vec::new(),
            phase: ClosurePhase::Drafting,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> ClosurePhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已入批数。
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// 申报条数。
    pub fn declared_count(&self) -> u32 {
        self.declared_count
    }

    /// 申报行数。
    pub fn declared_lines(&self) -> u64 {
        self.declared_lines
    }

    /// 申报（条数/行数一次落定；零申报在实核口点出）。
    pub fn declare(&mut self, count: u32, lines: u64) -> Result<(), u32> {
        if self.phase != ClosurePhase::Drafting {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        self.declared_count = count;
        self.declared_lines = lines;
        Ok(())
    }

    /// 按批号取批。
    pub fn batch_of(&self, batch_no: u32) -> Option<&BatchAccount> {
        self.batches.iter().find(|b| b.batch_no == batch_no)
    }

    /// 入批（批号必须按 1..=N 递增落账；相位倒退拒）。
    pub fn add_batch(&mut self, batch: BatchAccount) -> Result<(), u32> {
        if self.phase != ClosurePhase::Drafting {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let expect = (self.batches.len() as u32) + 1;
        if batch.batch_no != expect {
            self.rejects += 1;
            return Err(Diag::ESeq.val());
        }
        self.batches.push(batch);
        Ok(())
    }

    /// 三向恒等实核（条数向/行数向/判据号向逐向过闸）。
    /// 全过返回三向见证 [实核条数, 实核行数, 覆盖跨度]。
    /// 实核为纯读面：相位不符直接拒，不计拒绝数（拒绝数只记写入面）。
    pub fn verify(&self) -> Result<[u64; 3], u32> {
        if self.phase != ClosurePhase::Drafting {
            return Err(Diag::EPhase.val());
        }
        if self.batches.is_empty() {
            return Err(Diag::ENoBatch.val());
        }
        if self.declared_count == 0 || self.declared_lines == 0 {
            return Err(Diag::EDeclare.val());
        }
        // 逐批子式：批序/条数/号起点。
        for (idx, b) in self.batches.iter().enumerate() {
            if b.batch_no != (idx as u32) + 1 {
                return Err(Diag::ESeq.val());
            }
            if b.items != PER_BATCH {
                return Err(Diag::ECount.val());
            }
            let want_start = ID_MIN + (idx as u32) * PER_BATCH;
            if b.id_start != want_start {
                return Err(Diag::EIdStart.val());
            }
        }
        // 判据号覆盖：末批尾号必须恰 F1600（连续无缺无重由起点公式保证）。
        let last = match self.batches.last() {
            Some(b) => *b,
            None => return Err(Diag::ENoBatch.val()),
        };
        if last.id_end() != ID_MAX {
            return Err(Diag::EIdGap.val());
        }
        // 批数恒等子式。
        if self.batches.len() != BATCH_COUNT {
            return Err(Diag::ELack.val());
        }
        // 条数向：申报 = 实核 = 批数×每批。
        let mut count_sum: u64 = 0;
        for b in &self.batches {
            count_sum += b.items as u64;
        }
        if count_sum != COUNT_TOTAL as u64
            || count_sum != BATCH_COUNT as u64 * PER_BATCH as u64
            || self.declared_count as u64 != count_sum
        {
            return Err(Diag::ECountTotal.val());
        }
        // 四段和：每段和恰 60,000（段核先于总核，破段先点段）。
        let mut seg = 0usize;
        while seg < 4 {
            let mut seg_sum: u64 = 0;
            let mut k = 0usize;
            while k < SEGMENT_BATCHES {
                let idx = seg * SEGMENT_BATCHES + k;
                let b = match self.batches.get(idx) {
                    Some(x) => *x,
                    None => return Err(Diag::ELack.val()),
                };
                seg_sum += b.lines;
                k += 1;
            }
            if seg_sum != SEGMENT_LINES {
                return Err(Diag::ESegBroken.val());
            }
            seg += 1;
        }
        // 行数向：申报 = 实核 = 240,000。
        let mut lines_sum: u64 = 0;
        for b in &self.batches {
            lines_sum += b.lines;
        }
        if lines_sum != LINE_TOTAL || self.declared_lines != lines_sum {
            return Err(Diag::ELinesTotal.val());
        }
        Ok([count_sum, lines_sum, ID_MAX as u64 - ID_MIN as u64 + 1])
    }

    /// 平衡收束（实核全过才转已平衡；单向，倒退拒）。
    pub fn balance(&mut self) -> Result<[u64; 3], u32> {
        let witness = self.verify()?;
        self.phase = ClosurePhase::Balanced;
        Ok(witness)
    }

    /// 闭账（已平衡才出；三向恒等行落档）。
    /// 闭账为纯读面：未平衡直接拒，不计拒绝数（拒绝数只记写入面）。
    pub fn closeout(&self) -> Result<String, u32> {
        if self.phase != ClosurePhase::Balanced {
            return Err(Diag::EBalance.val());
        }
        let mut out = format!(
            "域闭账恒等式 条数{} 批数{} 每批{} 行数{}\n",
            COUNT_TOTAL, BATCH_COUNT, PER_BATCH, LINE_TOTAL
        );
        out.push_str(&format!(
            "条数向 {}={}x{}\n",
            COUNT_TOTAL, BATCH_COUNT, PER_BATCH
        ));
        out.push_str(&format!(
            "行数向 {}=四段和({}x{})\n",
            LINE_TOTAL, 4, SEGMENT_LINES
        ));
        out.push_str(&format!(
            "判据号向 F{}..F{} 连续{}\n",
            ID_MIN, ID_MAX, COUNT_TOTAL
        ));
        Ok(out)
    }
}

/// 恒等指纹（FNV-1a 整数折叠：三向见证混折）。
pub fn identity_digest(witness: &[u64; 3]) -> u64 {
    let mut h = DIGEST_SEED;
    let mut k = 0usize;
    while k < 3 {
        let mut w = match witness.get(k) {
            Some(x) => *x,
            None => 0,
        };
        let mut b = 0u32;
        while b < 8 {
            h ^= w & 0xFF;
            h = h.wrapping_mul(DIGEST_PRIME);
            w >>= 8;
            b += 1;
        }
        k += 1;
    }
    h
}

/// 标准闭账：四十批齐、申报齐（起草中相位，未平衡）。
pub fn build_closure() -> DomainClosure {
    let mut c = DomainClosure::new();
    let mut no = 1u32;
    while no <= BATCH_COUNT as u32 {
        let batch = BatchAccount {
            batch_no: no,
            items: PER_BATCH,
            lines: BATCH_LINES,
            id_start: ID_MIN + (no - 1) * PER_BATCH,
        };
        let _ = c.add_batch(batch);
        no += 1;
    }
    let _ = c.declare(COUNT_TOTAL, LINE_TOTAL);
    c
}

/// 标准已平衡闭账（平衡相位，可出闭账）。
pub fn build_balanced() -> DomainClosure {
    let mut c = build_closure();
    let _ = c.balance();
    c
}

/// 恒等破坏注入 10/10：十种注入逐种构造破账，逐种必须被实核口或闭账口
/// 检出。返回十布尔（true=该注入被检出）。
pub fn break_matrix() -> [bool; BREAK_PROBES] {
    let mut out = [false; BREAK_PROBES];
    // 注入1：条数破（首批 19 条）。
    let mut c = build_closure();
    let b1 = match c.batch_of(1) {
        Some(x) => *x,
        None => BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN },
    };
    replace_batch(&mut c, 1, BatchAccount { items: 19, ..b1 });
    if let Some(cell) = out.get_mut(0) {
        *cell = c.verify() == Err(Diag::ECount.val());
    }
    // 注入2：段行破（首批行数 5999，段一和破）。
    let mut c = build_closure();
    let b1 = match c.batch_of(1) {
        Some(x) => *x,
        None => BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN },
    };
    replace_batch(&mut c, 1, BatchAccount { lines: 5999, ..b1 });
    if let Some(cell) = out.get_mut(1) {
        *cell = c.verify() == Err(Diag::ESegBroken.val());
    }
    // 注入3：批数缺（39 批，覆盖尾号 1580 断）。
    let mut c = build_closure();
    c.batches.pop();
    if let Some(cell) = out.get_mut(2) {
        *cell = c.verify() == Err(Diag::EIdGap.val());
    }
    // 注入4：批序乱（第五批批号被改为 6，批序断）。
    let mut c = build_closure();
    let b5 = match c.batch_of(5) {
        Some(x) => *x,
        None => BatchAccount { batch_no: 5, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN + 4 * PER_BATCH },
    };
    replace_batch(&mut c, 5, BatchAccount { batch_no: 6, ..b5 });
    if let Some(cell) = out.get_mut(3) {
        *cell = c.verify() == Err(Diag::ESeq.val());
    }
    // 注入5：号起点错（第十批起点 1001，应为 991）。
    let mut c = build_closure();
    let b10 = match c.batch_of(10) {
        Some(x) => *x,
        None => BatchAccount { batch_no: 10, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN + 9 * PER_BATCH },
    };
    replace_batch(&mut c, 10, BatchAccount { id_start: 1001, ..b10 });
    if let Some(cell) = out.get_mut(4) {
        *cell = c.verify() == Err(Diag::EIdStart.val());
    }
    // 注入6：申报条数破（申报 799）。
    let mut c = build_closure();
    let _ = c.declare(799, LINE_TOTAL);
    if let Some(cell) = out.get_mut(5) {
        *cell = c.verify() == Err(Diag::ECountTotal.val());
    }
    // 注入7：申报行数破（申报 239,999）。
    let mut c = build_closure();
    let _ = c.declare(COUNT_TOTAL, LINE_TOTAL - 1);
    if let Some(cell) = out.get_mut(6) {
        *cell = c.verify() == Err(Diag::ELinesTotal.val());
    }
    // 注入8：空账。
    let mut c = DomainClosure::new();
    let _ = c.declare(COUNT_TOTAL, LINE_TOTAL);
    if let Some(cell) = out.get_mut(7) {
        *cell = c.verify() == Err(Diag::ENoBatch.val());
    }
    // 注入9：批数溢（41 批，覆盖尾号 1620 越上沿）。
    let mut c = build_closure();
    let _ = c.add_batch(BatchAccount {
        batch_no: 41,
        items: PER_BATCH,
        lines: BATCH_LINES,
        id_start: ID_MAX + 1,
    });
    if let Some(cell) = out.get_mut(8) {
        *cell = c.verify() == Err(Diag::EIdGap.val());
    }
    // 注入10：另段行破（第 31 批行数 6001，段四和破）。
    let mut c = build_closure();
    let b31 = match c.batch_of(31) {
        Some(x) => *x,
        None => BatchAccount { batch_no: 31, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN + 30 * PER_BATCH },
    };
    replace_batch(&mut c, 31, BatchAccount { lines: 6001, ..b31 });
    if let Some(cell) = out.get_mut(9) {
        *cell = c.verify() == Err(Diag::ESegBroken.val());
    }
    out
}

/// 以破批替换指定批（保持批位；注入构造专用）。
fn replace_batch(closure: &mut DomainClosure, batch_no: u32, batch: BatchAccount) {
    if let Some(slot) = closure.batches.iter_mut().find(|b| b.batch_no == batch_no) {
        *slot = batch;
    }
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x793C0
}

/// 三向恒等成立（平衡相位 + 申报对齐 + 逐批形状对齐钦定值）。
/// 平衡相位下 verify 因相位拒，故批册形状在此独立实核（双保险面）。
fn identity_ok(c: &DomainClosure) -> bool {
    if c.phase() != ClosurePhase::Balanced {
        return false;
    }
    if c.len() != BATCH_COUNT || c.declared_count() != COUNT_TOTAL || c.declared_lines() != LINE_TOTAL {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_COUNT {
        let b = match c.batch_of((i + 1) as u32) {
            Some(b) => *b,
            None => return false,
        };
        if b.batch_no != (i as u32) + 1 {
            return false;
        }
        if b.items != PER_BATCH || b.lines != BATCH_LINES {
            return false;
        }
        if b.id_start != ID_MIN + (i as u32) * PER_BATCH {
            return false;
        }
        if b.id_end() != ID_MIN + ((i as u32) + 1) * PER_BATCH - 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 恒等破坏注入 10/10 检出。
fn break_all_ok() -> bool {
    break_matrix() == [true; BREAK_PROBES]
}

/// 双跑一致（同构输入两次实核见证一致且指纹非零）。
fn dual_run_ok() -> bool {
    let mut a = build_closure();
    let mut b = build_closure();
    let wa = a.balance();
    let wb = b.balance();
    let same = wa == wb;
    let digest_nonzero = match &wa {
        Ok(w) => identity_digest(w) != 0,
        Err(_) => false,
    };
    same && digest_nonzero
}

/// 域闭账恒等式判据：UNX-F1599 域闭账恒等式。
pub fn run_unx1599_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1599");
    let mut closure = build_closure();

    s.add_dyn(
        "F1599-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 钦定常量闭面：800=40×20、240,000=四段和、F0801..F1600。
    s.add_dyn(
        "F1599-02 钦定常量三向自洽",
        COUNT_TOTAL == BATCH_COUNT as u32 * PER_BATCH
            && LINE_TOTAL == SEGMENT_LINES * 4
            && SEGMENT_LINES == SEGMENT_BATCHES as u64 * BATCH_LINES
            && ID_MAX - ID_MIN + 1 == COUNT_TOTAL,
        &format!("800={}x{} lines={} seg={}", BATCH_COUNT, PER_BATCH, LINE_TOTAL, SEGMENT_LINES),
    );

    // 逐批批判据号起止对齐（首尾抽查）。
    let c = build_closure();
    let first_ok = c.batch_of(1).map(|b| b.id_start == ID_MIN && b.id_end() == ID_MIN + PER_BATCH - 1).unwrap_or(false);
    let last_ok = c.batch_of(BATCH_COUNT as u32).map(|b| b.id_end() == ID_MAX).unwrap_or(false);
    s.add_dyn(
        "F1599-03 判据号起止对齐",
        first_ok && last_ok,
        &format!("first_start={} last_end={}",
            c.batch_of(1).map(|b| b.id_start).unwrap_or(0),
            c.batch_of(BATCH_COUNT as u32).map(|b| b.id_end()).unwrap_or(0)),
    );

    // 申报缺失显性拒（零申报不核）。
    let mut undeclared = DomainClosure::new();
    let mut no = 1u32;
    while no <= BATCH_COUNT as u32 {
        let _ = undeclared.add_batch(BatchAccount {
            batch_no: no,
            items: PER_BATCH,
            lines: BATCH_LINES,
            id_start: ID_MIN + (no - 1) * PER_BATCH,
        });
        no += 1;
    }
    let declare_rej = undeclared.verify() == Err(Diag::EDeclare.val());
    s.add_dyn(
        "F1599-04 申报缺失显性拒",
        declare_rej && undeclared.declared_count() == 0,
        &format!("declared={}", undeclared.declared_count()),
    );

    // 空账实核显性拒。
    let mut empty = DomainClosure::new();
    let _ = empty.declare(COUNT_TOTAL, LINE_TOTAL);
    let empty_rej = empty.verify() == Err(Diag::ENoBatch.val());
    s.add_dyn(
        "F1599-05 空账实核显性拒",
        empty_rej,
        &format!("empty_rejected={}", empty_rej),
    );

    // 恒等破坏注入 10/10。
    s.add_dyn(
        "F1599-06 恒等破坏十种全检出",
        break_all_ok(),
        &format!("matrix={:?}", break_matrix()),
    );

    // 批序落账门：跳号显性拒。
    let mut seq = DomainClosure::new();
    let _ = seq.add_batch(BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN });
    let seq_rej = seq.add_batch(BatchAccount { batch_no: 3, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN + 2 * PER_BATCH }) == Err(Diag::ESeq.val());
    s.add_dyn(
        "F1599-07 批序跳号显性拒",
        seq_rej && seq.rejects() >= 1,
        &format!("rejects={}", seq.rejects()),
    );

    // 平衡收束：三向见证对齐钦定值。
    let witness = closure.balance();
    let witness_ok = match &witness {
        Ok(w) => {
            w.get(0) == Some(&(COUNT_TOTAL as u64))
                && w.get(1) == Some(&LINE_TOTAL)
                && w.get(2) == Some(&(COUNT_TOTAL as u64))
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1599-08 三向恒等见证对齐",
        witness_ok && identity_ok(&closure) && closure.phase() == ClosurePhase::Balanced,
        &format!("witness={:?}", match &witness {
            Ok(w) => *w,
            Err(_) => [0u64; 3],
        }),
    );

    // 已平衡后实核与重平衡显性拒（相位单向）。
    let re_verify = closure.verify() == Err(Diag::EPhase.val());
    let re_balance = closure.balance() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1599-09 已平衡后实核显性拒",
        re_verify && re_balance && closure.phase().ord() == 1,
        &format!("phase_ord={}", closure.phase().ord()),
    );

    // 闭账落档：三向恒等行全名。
    let out = closure.closeout();
    let out_ok = match &out {
        Ok(t) => {
            t.contains("条数800")
                && t.contains("批数40")
                && t.contains("行数240000")
                && t.contains("条数向")
                && t.contains("行数向")
                && t.contains("判据号向")
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1599-10 闭账三向恒等行落档",
        out_ok,
        &format!("bytes={}", out.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 未平衡先出闭账显性拒。
    let draft_rej = build_closure().closeout() == Err(Diag::EBalance.val());
    s.add_dyn(
        "F1599-11 未平衡先出闭账显性拒",
        draft_rej,
        &format!("draft_rejected={}", draft_rej),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1599-12 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EDeclare.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EDeclare.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x793C0);
    }

    #[test]
    fn frozen_constants_identity() {
        assert_eq!(COUNT_TOTAL, 800);
        assert_eq!(BATCH_COUNT * PER_BATCH as usize, 800);
        assert_eq!(LINE_TOTAL, 240_000);
        assert_eq!(BATCH_LINES, 6_000);
        assert_eq!(SEGMENT_LINES, 60_000);
        assert_eq!(ID_MAX - ID_MIN + 1, COUNT_TOTAL);
    }

    #[test]
    fn batch_id_ranges_contiguous() {
        let c = build_closure();
        let mut no = 1u32;
        while no <= BATCH_COUNT as u32 {
            let b = c.batch_of(no).unwrap_or(&BatchAccount {
                batch_no: 0,
                items: 0,
                lines: 0,
                id_start: 0,
            });
            let want_start = ID_MIN + (no - 1) * PER_BATCH;
            assert_eq!(b.id_start, want_start, "批{} 起点须 {}", no, want_start);
            assert_eq!(b.id_end(), want_start + PER_BATCH - 1);
            no += 1;
        }
    }

    #[test]
    fn add_batch_order_gate() {
        let mut c = DomainClosure::new();
        assert_eq!(
            c.add_batch(BatchAccount { batch_no: 2, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN + PER_BATCH }),
            Err(Diag::ESeq.val())
        );
        assert_eq!(
            c.add_batch(BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN }),
            Ok(())
        );
        assert_eq!(
            c.add_batch(BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN }),
            Err(Diag::ESeq.val())
        );
    }

    #[test]
    fn break_ten_of_ten() {
        assert_eq!(break_matrix(), [true; BREAK_PROBES]);
    }

    #[test]
    fn verify_witness_values() {
        let mut c = build_closure();
        let w = c.balance();
        assert_eq!(
            w,
            Ok([800u64, 240_000, 800])
        );
        assert_eq!(c.phase(), ClosurePhase::Balanced);
    }

    #[test]
    fn undeclared_and_empty_rejected() {
        let mut c = DomainClosure::new();
        let _ = c.declare(COUNT_TOTAL, LINE_TOTAL);
        assert_eq!(c.verify(), Err(Diag::ENoBatch.val()));
        let mut d = DomainClosure::new();
        let _ = d.add_batch(BatchAccount { batch_no: 1, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MIN });
        assert_eq!(d.verify(), Err(Diag::EDeclare.val()));
    }

    #[test]
    fn phase_monotonic() {
        let mut c = build_closure();
        assert_eq!(c.phase(), ClosurePhase::Drafting);
        let _ = c.balance();
        assert_eq!(c.balance(), Err(Diag::EPhase.val()));
        assert_eq!(c.verify(), Err(Diag::EPhase.val()));
        assert_eq!(c.declare(1, 1), Err(Diag::EPhase.val()));
        assert_eq!(c.add_batch(BatchAccount { batch_no: 41, items: PER_BATCH, lines: BATCH_LINES, id_start: ID_MAX + 1 }), Err(Diag::EPhase.val()));
        assert_eq!(c.phase().ord(), 1);
    }

    #[test]
    fn closeout_needs_balance() {
        let draft = build_closure();
        assert_eq!(draft.closeout(), Err(Diag::EBalance.val()));
        let balanced = build_balanced();
        let out = balanced.closeout();
        assert!(out.is_ok());
        let t = out.unwrap_or_default();
        assert!(t.contains("条数800"));
        assert!(t.contains("条数向"));
        assert!(t.contains("行数向"));
        assert!(t.contains("判据号向"));
    }

    #[test]
    fn identity_digest_stable() {
        let a = identity_digest(&[800, 240_000, 800]);
        let b = identity_digest(&[800, 240_000, 800]);
        let c = identity_digest(&[799, 240_000, 800]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1599_checks();
        assert_eq!(s.len(), 12);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
