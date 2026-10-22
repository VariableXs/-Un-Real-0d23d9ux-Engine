//! UNX-F21587 · 域守恒终声明（行数/条数/判据三面）
//!
//! 诊断段 0x91031~0x91038（F2-B40 域收官段 7/100）。
//!
//! 判据原文（施工书 UNX-F21587-J1）：三面重算 800 条/240,000 行/800 判据全绿。
//!
//! 设计要点（为什么三面不能共用一个总数变量）：
//! 三面如果都由同一个 `total` 派生，那么「三面一致」就是同一句话重复三遍——
//! 它只能证明那次乘法没写错，证明不了账本本身对。故三面各走一条独立重算路径：
//!   条目面：逐批累加实报条目数（无乘法、无基数常量）；
//!   行数面：写死基数 6,000×40，再回放每批偏差（实数减配额）；
//!   判据面：逐批累加实报判据数。
//! 三面只有「各自独立算完还对得上」才有意义，抄错一条就会分叉。
//!
//! 互校矩阵是三面之外的第四个约束，且不可省：把 B05 的条目改成 21、B06 改成 19，
//! 三面总数仍是 800/240,000/800，逐面核对全绿——错的是「批号与数字的绑定」。故
//! 按批做比值互校（每批行数 = 条目 × 300、每批判据 = 条目），才能抓到这类重分配。
//!
//! 声明单必须带指纹而不是只带三个数：三个数相同不代表是同一本账（上面的重分配
//! 就是现成反例）。指纹覆盖逐批绑定，重复声明同一本账返回同一指纹，账变了则
//! 报「声明失效」而不是逐面报不足——声明已对外承诺过某个指纹，先报作废才对。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 域内批数（B01~B40）。
pub const BATCHES: u16 = 40;
/// 单批条目数。
pub const ITEMS_PER_BATCH: u16 = 20;
/// 单批行数配额（写死，作为行数面的基数）。
pub const ROWS_PER_BATCH: u32 = 6_000;
/// 单批判据数。
pub const CHECKS_PER_BATCH: u16 = 20;
/// 一条条目摊到的行数（6,000 ÷ 20 = 300，互校矩阵的换算系数）。
pub const ROWS_PER_ITEM: u32 = 300;
/// 域条目总数（40 × 20）。
pub const DOMAIN_ITEMS: u32 = 800;
/// 域行数总数（40 × 6,000）。
pub const DOMAIN_ROWS: u64 = 240_000;
/// 域判据总数（40 × 20）。
pub const DOMAIN_CHECKS: u32 = 800;

/// 终声明诊断码闭集（0x91031~0x91038）。
///
/// 显式判别式而非隐式递增：码值是与外部日志对账的键，插入变体会让后续码整体漂
/// 移，历史归档无法归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsCode {
    /// 0x91031 三面守恒，声明单签发。
    Conserved = 0x91031,
    /// 0x91032 条目面不足。
    ItemShort = 0x91032,
    /// 0x91033 条目面超出（含批数越限）。
    ItemOver = 0x91033,
    /// 0x91034 行数面不足。
    RowShort = 0x91034,
    /// 0x91035 行数面超出。
    RowOver = 0x91035,
    /// 0x91036 判据面偏离。
    CritOff = 0x91036,
    /// 0x91037 两面互校不一致（总数对但分布错）。
    FaceDisagree = 0x91037,
    /// 0x91038 账已变更，既有声明失效。
    StaleDeclaration = 0x91038,
}

impl ConsCode {
    /// 三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ConsCode::Conserved => "条目面/行数面/判据面三面守恒",
            ConsCode::ItemShort => "条目面低于 800 条",
            ConsCode::ItemOver => "条目面高于 800 条",
            ConsCode::RowShort => "行数面低于 240,000 行",
            ConsCode::RowOver => "行数面高于 240,000 行",
            ConsCode::CritOff => "判据面偏离 800 条",
            ConsCode::FaceDisagree => "三面总数相符但批级比值互校不一致",
            ConsCode::StaleDeclaration => "账本指纹与已签发声明单不符",
        }
    }
    /// 三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ConsCode::Conserved => "三路独立重算同值、批级互校全过、指纹稳定",
            ConsCode::ItemShort => "有批漏登条目，或整批未装入",
            ConsCode::ItemOver => "有批多登条目，或同一批被装入两次",
            ConsCode::RowShort => "实计行数少于配额且未补行",
            ConsCode::RowOver => "实计行数超出配额仍被接纳",
            ConsCode::CritOff => "某批判据条数与条目数脱钩",
            ConsCode::FaceDisagree => "批间重分配：一批多、一批少，总数恰好抵消",
            ConsCode::StaleDeclaration => "签发后又改了账面数字",
        }
    }
    /// 三要素之三：下一步。
    pub const fn advice(self) -> &'static str {
        match self {
            ConsCode::Conserved => "归档声明单，域收官",
            ConsCode::ItemShort => "按批号补登缺失条目后重签",
            ConsCode::ItemOver => "删除多登条目后重签",
            ConsCode::RowShort => "补齐欠账行后重签",
            ConsCode::RowOver => "裁剪超额行后重签",
            ConsCode::CritOff => "按批补齐判据条数后重签",
            ConsCode::FaceDisagree => "按批核对条目/行数/判据的比值，勿只看总数",
            ConsCode::StaleDeclaration => "作废旧声明单，走完整签名流程重签",
        }
    }
    /// 数值形式。返回 u32：五位数码（0x91031）超 u16 值域，`as u16` 会静默截断成
    /// 0x1031 与低段码撞车——截断后的码在日志里长得像「另一个模块的故障」。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 互校矩阵的面配对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacePair {
    /// 条目面 × 行数面（每批行数应为条目 × 300）。
    ItemRow,
    /// 条目面 × 判据面（每批判据应等于条目）。
    ItemCrit,
    /// 行数面 × 判据面（每批行数应为判据 × 300）。
    RowCrit,
}

impl FacePair {
    /// 面配对名（报告与判据断言用）。
    pub const fn label(self) -> &'static str {
        match self {
            FacePair::ItemRow => "item*row",
            FacePair::ItemCrit => "item*crit",
            FacePair::RowCrit => "row*crit",
        }
    }
}

/// 单批三面账：条目数、配额、实计行数、判据数。
///
/// 条目数与行数分开存、配额与实计分开存，都是同一件事：合并字段等于把「应该有」
/// 和「实际有」压成同一事实，偏差就无从回放，行数面的独立重算路径也就废了。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FaceRow {
    /// 批号。
    pub batch: u16,
    /// 本批条目数。
    pub items: u16,
    /// 本批行数配额。
    pub quota: u32,
    /// 本批实计行数。
    pub rows: u32,
    /// 本批判据数。
    pub checks: u16,
}

impl FaceRow {
    /// 构造一批（校验在各闸口统一做）。
    pub const fn new(batch: u16, items: u16, quota: u32, rows: u32, checks: u16) -> FaceRow {
        FaceRow { batch, items, quota, rows, checks }
    }
    /// 本批偏差（实计减配额，带符号）。
    ///
    /// 用 i64 而非 u32 相减：短账时 u32 绕回成巨大正数，恰好能与别处的超账抵消，
    /// 正是行数面要防的「错误互相遮盖」。
    pub const fn deviation(&self) -> i64 {
        self.rows as i64 - self.quota as i64
    }
    /// 批级互校：返回首个不成立的面配对（按条目×行 → 条目×判据 → 行×判据）。
    pub const fn break_kind(&self) -> Option<FacePair> {
        if self.rows as u64 != self.items as u64 * ROWS_PER_ITEM as u64 {
            return Some(FacePair::ItemRow);
        }
        if self.checks != self.items {
            return Some(FacePair::ItemCrit);
        }
        if self.rows as u64 != self.checks as u64 * ROWS_PER_ITEM as u64 {
            return Some(FacePair::RowCrit);
        }
        None
    }
}

/// 三面总值。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FaceTotals {
    pub items: u32,
    pub rows: u64,
    pub checks: u32,
}

impl FaceTotals {
    /// 三面是否逐面命中（写死常量，不接受外部口径）。
    pub const fn hits(&self) -> bool {
        self.items == DOMAIN_ITEMS && self.rows == DOMAIN_ROWS && self.checks == DOMAIN_CHECKS
    }
}

/// 互校矩阵。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CrossMatrix {
    /// 全部批满足「行数 = 条目 × 300」。
    pub item_row: bool,
    /// 全部批满足「判据 = 条目」。
    pub item_crit: bool,
    /// 全部批满足「行数 = 判据 × 300」。
    pub row_crit: bool,
}

impl CrossMatrix {
    /// 三格是否全过。
    pub const fn all_hold(&self) -> bool {
        self.item_row && self.item_crit && self.row_crit
    }
    /// 首个不成立的面配对（全过返回 None）。顺序即优先级：条目×行 是主约束，
    /// 另外两格由它派生，先报主约束才能避免一个根因报三次。
    pub const fn first_failed(&self) -> Option<FacePair> {
        if !self.item_row {
            return Some(FacePair::ItemRow);
        }
        if !self.item_crit {
            return Some(FacePair::ItemCrit);
        }
        if !self.row_crit {
            return Some(FacePair::RowCrit);
        }
        None
    }
}

/// 终声明单。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Declaration {
    pub items: u32,
    pub rows: u64,
    pub checks: u32,
    /// 逐批绑定指纹（三数相同不足以识别同一本账）。
    pub fingerprint: u64,
    /// 签核位。
    pub signer: u8,
    /// 是否为「同账重声明」。
    pub reaffirmed: bool,
}

impl Declaration {
    /// 单行文本（归档用；字段顺序固定便于 diff）。
    pub fn as_line(&self) -> String {
        let mut s = String::with_capacity(96);
        s.push_str("decl items=");
        s.push_str(&self.items.to_string());
        s.push_str(" rows=");
        s.push_str(&self.rows.to_string());
        s.push_str(" checks=");
        s.push_str(&self.checks.to_string());
        s.push_str(" fp=");
        s.push_str(&hex64(self.fingerprint));
        s.push_str(" signer=");
        s.push_str(&self.signer.to_string());
        s.push_str(" reaffirmed=");
        s.push_str(if self.reaffirmed { "1" } else { "0" });
        s
    }
    /// 数值面是否命中（不看指纹）。
    pub const fn faces_hit(&self) -> bool {
        self.items == DOMAIN_ITEMS && self.rows == DOMAIN_ROWS && self.checks == DOMAIN_CHECKS
    }
}

/// 签署故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 相关批号（0 表示与具体批无关）。
    pub batch: u16,
}

impl ConsFault {
    const fn new(code: ConsCode, batch: u16) -> ConsFault {
        ConsFault { code: code.code(), batch }
    }
}

/// 守恒账本 + 终声明签发器。
///
/// 为什么账本自己持有 `sealed`：声明的语义是「这本账在签发时守恒」。若允许反复
/// 签发而不记账，第二次就把「账已变」误判成「从未签过」，失效诊断整个丢失。
pub struct ConservSealer {
    rows: Vec<FaceRow>,
    sealed: Option<Declaration>,
    declarations: u16,
    rejected: u16,
}

impl ConservSealer {
    /// 空账。
    pub const fn new() -> ConservSealer {
        ConservSealer { rows: Vec::new(), sealed: None, declarations: 0, rejected: 0 }
    }
    /// 装入一批。
    ///
    /// 容量闸用 `ItemOver` 而不另设「容量码」：第 41 批一旦装入，条目面必然超
    /// 800（每批至少 1 条），根因与码本来就同一件事，分两码等于让运维去猜。
    pub fn push(&mut self, row: FaceRow) -> Result<(), ConsFault> {
        if self.rows.len() >= BATCHES as usize {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ConsFault::new(ConsCode::ItemOver, row.batch));
        }
        self.rows.push(row);
        Ok(())
    }
    /// 已装批数。
    pub fn batch_count(&self) -> usize {
        self.rows.len()
    }
    /// 已被拒的装批次数（容量闸可观测）。
    pub const fn rejected(&self) -> u16 {
        self.rejected
    }
    /// 已签发次数（幂等观测点：同账重声明会累加，但指纹不变）。
    pub const fn declarations(&self) -> u16 {
        self.declarations
    }
    /// 改一批的面值（造重分配与失效样本；批号不存在返回 false）。
    pub fn retune(&mut self, batch: u16, row: FaceRow) -> bool {
        for r in &mut self.rows {
            if r.batch == batch {
                *r = row;
                return true;
            }
        }
        false
    }
    /// 条目面重算：逐批累加（无乘法、无基数常量）。
    pub fn recount_items(&self) -> u32 {
        let mut n = 0u32;
        for r in &self.rows {
            n = n.saturating_add(r.items as u32);
        }
        n
    }
    /// 行数面重算：基数乘法 + 逐批偏差回放。
    ///
    /// 基数按**已装批数**乘配额常量，而不按 40 写死：写死 40 会让空账也回放成
    /// 240,000（「零批也有满账」），空账因此永远查不出来。配额口径仍取常量——
    /// 若基数也取账本里的配额之和，「配额被改成 6,001」会因两边同改而互相掩护。
    pub fn recount_rows(&self) -> u64 {
        let base = (ROWS_PER_BATCH as u64).saturating_mul(self.rows.len() as u64);
        let mut dev = 0i64;
        for r in &self.rows {
            dev = dev.saturating_add(r.deviation());
        }
        if dev >= 0 {
            base.saturating_add(dev as u64)
        } else {
            base.saturating_sub((-dev) as u64)
        }
    }
    /// 判据面重算：逐批累加实报判据数。
    pub fn recount_checks(&self) -> u32 {
        let mut n = 0u32;
        for r in &self.rows {
            n = n.saturating_add(r.checks as u32);
        }
        n
    }
    /// 三面总值（每面走自己的重算路径，不共享中间量）。
    pub fn faces(&self) -> FaceTotals {
        FaceTotals { items: self.recount_items(), rows: self.recount_rows(), checks: self.recount_checks() }
    }
    /// 互校矩阵。
    pub fn cross(&self) -> CrossMatrix {
        let mut m = CrossMatrix { item_row: true, item_crit: true, row_crit: true };
        for r in &self.rows {
            if r.rows as u64 != r.items as u64 * ROWS_PER_ITEM as u64 {
                m.item_row = false;
            }
            if r.checks != r.items {
                m.item_crit = false;
            }
            if r.rows as u64 != r.checks as u64 * ROWS_PER_ITEM as u64 {
                m.row_crit = false;
            }
        }
        m
    }
    /// 首个互校失败的批号（全过返回 0）。
    pub fn first_cross_batch(&self) -> u16 {
        for r in &self.rows {
            if r.break_kind().is_some() {
                return r.batch;
            }
        }
        0
    }
    /// 逐批绑定指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h = fnv_u64(h, self.rows.len() as u64);
        for r in &self.rows {
            h = fnv_u64(h, r.batch as u64);
            h = fnv_u64(h, r.items as u64);
            h = fnv_u64(h, r.quota as u64);
            h = fnv_u64(h, r.rows as u64);
            h = fnv_u64(h, r.checks as u64);
        }
        h
    }
    /// 签发终声明。
    ///
    /// 闸门顺序固定：空账/批数 → 条目面 → 行数面 → 判据面 → 互校 → 失效。
    /// 失效放最后是有意的：账被改过时，若先报「条目面差 1」，运维会去补一条，
    /// 补完发现声明早该作废；先让三面与互校把「现在这本账是否守恒」判完，再判
    /// 「它还是不是原来那本账」，诊断顺序才与修复顺序一致。
    pub fn declare(&mut self, signer: u8) -> Result<Declaration, ConsFault> {
        if self.rows.is_empty() {
            return Err(ConsFault::new(ConsCode::ItemShort, 0));
        }
        if self.rows.len() != BATCHES as usize {
            return Err(ConsFault::new(ConsCode::ItemShort, self.rows.len() as u16));
        }
        let f = self.faces();
        if f.items < DOMAIN_ITEMS {
            return Err(ConsFault::new(ConsCode::ItemShort, 0));
        }
        if f.items > DOMAIN_ITEMS {
            return Err(ConsFault::new(ConsCode::ItemOver, 0));
        }
        if f.rows < DOMAIN_ROWS {
            return Err(ConsFault::new(ConsCode::RowShort, 0));
        }
        if f.rows > DOMAIN_ROWS {
            return Err(ConsFault::new(ConsCode::RowOver, 0));
        }
        // 判据面只设一个「偏离」码：判据超 800 必然先由条目面超出（每批判据与
        // 条目 1:1），这里再分「超/不足」两码就是对同一根因双报。
        if f.checks != DOMAIN_CHECKS {
            return Err(ConsFault::new(ConsCode::CritOff, 0));
        }
        let cross = self.cross();
        if !cross.all_hold() {
            return Err(ConsFault::new(ConsCode::FaceDisagree, self.first_cross_batch()));
        }
        let fp = self.fingerprint();
        let reaffirmed = match self.sealed {
            Some(prev) => {
                if prev.fingerprint != fp {
                    return Err(ConsFault::new(ConsCode::StaleDeclaration, 0));
                }
                true
            }
            None => false,
        };
        let decl = Declaration { items: f.items, rows: f.rows, checks: f.checks, fingerprint: fp, signer, reaffirmed };
        self.sealed = Some(decl);
        self.declarations = self.declarations.saturating_add(1);
        Ok(decl)
    }
    /// 复核已签发声明：重算三面与指纹，与声明单逐项比对。
    ///
    /// 不复用 `declare` 的中间量：声明自己验自己，永远为真。
    pub fn verify(&self, decl: &Declaration) -> Result<(), ConsFault> {
        let f = self.faces();
        if f.items != decl.items || f.rows != decl.rows || f.checks != decl.checks {
            return Err(ConsFault::new(ConsCode::FaceDisagree, 0));
        }
        if self.fingerprint() != decl.fingerprint {
            return Err(ConsFault::new(ConsCode::StaleDeclaration, 0));
        }
        Ok(())
    }
    /// 声明摘要（含三面数值 + 指纹 + 签核位）。
    pub fn report(&self) -> String {
        let f = self.faces();
        let mut s = String::with_capacity(128);
        s.push_str("conserve batches=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" items=");
        s.push_str(&f.items.to_string());
        s.push_str(" rows=");
        s.push_str(&f.rows.to_string());
        s.push_str(" checks=");
        s.push_str(&f.checks.to_string());
        s.push_str(" hits=");
        s.push_str(if f.hits() { "1" } else { "0" });
        s.push_str(" fp=");
        s.push_str(&hex64(self.fingerprint()));
        s
    }
}

impl Default for ConservSealer {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准账本（40 批 × 20 条 / 6,000 行 / 20 判据）。
pub fn standard_book() -> Result<ConservSealer, ConsFault> {
    let mut s = ConservSealer::new();
    let mut b: u16 = 1;
    while b <= BATCHES {
        s.push(FaceRow::new(b, ITEMS_PER_BATCH, ROWS_PER_BATCH, ROWS_PER_BATCH, CHECKS_PER_BATCH))?;
        b += 1;
    }
    Ok(s)
}

/// FNV-1a 64 单步（写死质数与偏移基，来源：FNV 规范 1a 变体）。
fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化分配）。
fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    s
}

/// 判据入口（UNX-F21587-J1 及其配套）。判据常量独立写死：800 / 240_000 / 0x91032
/// 等直接以字面量出现，不复用被测常量。
pub fn run_unx21587_conservseal_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21587");

    // 1) 夹具可构造：40 批、三面各 800/240,000/800、互校全过。
    let book = standard_book();
    let fixture_ok = match &book {
        Ok(s) => {
            let f = s.faces();
            s.batch_count() == 40 && f.items == 800 && f.rows == 240_000 && f.checks == 800 && s.cross().all_hold()
        }
        Err(_) => false,
    };
    cs.add("f21587.fixture_buildable", fixture_ok, "标准 40 批账本构造失败或三面不符");

    // 2) 正向·三面独立重算：三条路径各自命中，且偏差回放后与实计求和同值。
    if let Ok(s) = &book {
        let f = s.faces();
        let rep = s.report();
        cs.add("f21587.three_faces_exact", s.recount_items() == 800 && s.recount_rows() == 240_000 && s.recount_checks() == 800 && f.hits() && s.first_cross_batch() == 0 && rep.contains("items=800") && rep.contains("rows=240000") && rep.contains("hits=1"), "三面重算未命中 800/240000/800，或报告缺字段");
    } else {
        cs.add("f21587.three_faces_exact", false, "夹具不可用");
    }

    // 3) 正向·签发 + 复核：声明单三面数值/指纹/签核位对，且复核（独立重算）必过。
    if let Ok(mut s) = standard_book() {
        match s.declare(7) {
            Ok(d) => {
                let line = d.as_line();
                let ok = d.faces_hit() && d.fingerprint != 0 && d.signer == 7 && !d.reaffirmed
                    && line.contains("items=800") && line.contains("rows=240000") && line.contains("checks=800");
                cs.add("f21587.declare_ok", ok, "声明单数值/指纹/签核位不符，或缺字段");
                cs.add("f21587.verify_ok", s.verify(&d).is_ok(), "签发后复核不通过");
            }
            Err(_) => {
                cs.add("f21587.declare_ok", false, "标准账签发被拒");
                cs.add("f21587.verify_ok", false, "无声明单可复核");
            }
        }
    } else {
        cs.add("f21587.declare_ok", false, "夹具不可用");
        cs.add("f21587.verify_ok", false, "夹具不可用");
    }

    // 5) 反向·条目面不足：B03 条目 19（判据同比），总数 799 → 0x91032。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let ok = s.retune(3, FaceRow::new(3, 19, 6_000, 6_000, 19));
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.item_face_short", ok && code == Some(0x91032), "条目面短 1 条未被判 0x91032");
    }

    // 6) 反向·条目面超出：B03 条目 21（判据同比），总数 801 → 0x91033。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let ok = s.retune(3, FaceRow::new(3, 21, 6_000, 6_000, 21));
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.item_face_over", ok && code == Some(0x91033), "条目面多 1 条未被判 0x91033");
    }

    // 7) 反向·行数面不足：B03 实计 5,999 → 239,999 → 0x91034。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let ok = s.retune(3, FaceRow::new(3, 20, 6_000, 5_999, 20));
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.row_face_short", ok && code == Some(0x91034), "行数面短 1 行未被判 0x91034");
    }

    // 8) 反向·行数面超出：B03 实计 6,001 → 240,001 → 0x91035。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let ok = s.retune(3, FaceRow::new(3, 20, 6_000, 6_001, 20));
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.row_face_over", ok && code == Some(0x91035), "行数面多 1 行未被判 0x91035");
    }

    // 9) 反向·判据面偏离：B03 判据 19（条目仍 20），总数 799 → 0x91036。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let ok = s.retune(3, FaceRow::new(3, 20, 6_000, 6_000, 19));
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.crit_face_off", ok && code == Some(0x91036), "判据面少 1 条未被判 0x91036");
    }

    // 10) 反向·互校不一致（三面总数全对，分布错）：B05 条目 21、B06 条目 19，
    //     行数不动 → 800/240,000/800 全绿，但 B05 的 21 条摊不到 6,000 行。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let a = s.retune(5, FaceRow::new(5, 21, 6_000, 6_000, 21));
        let b = s.retune(6, FaceRow::new(6, 19, 6_000, 6_000, 19));
        let f = s.faces();
        let cross = s.cross();
        let code = s.declare(1).err().map(|f| f.code);
        cs.add(
            "f21587.cross_disagree",
            a && b && f.items == 800 && f.rows == 240_000 && f.checks == 800 && !cross.item_row && cross.first_failed() == Some(FacePair::ItemRow) && FacePair::ItemRow.label() == "item*row" && code == Some(0x91037) && s.first_cross_batch() == 5,
            "总数相符的重分配未被互校抓到（应 0x91037、面 item*row、批 5）",
        );
    }

    // 11) 边界·空账：0 批 → 条目面不足 0x91032，三面为 0，不 panic。
    {
        let mut s = ConservSealer::new();
        let f = s.faces();
        let code = s.declare(1).err().map(|f| f.code);
        cs.add("f21587.empty_book", s.batch_count() == 0 && f.items == 0 && f.rows == 0 && code == Some(0x91032) && !f.hits(), "空账未被判 0x91032，或三面非零");
    }

    // 12) 边界·单批：1 批 20 条 → 批数不足仍在条目面报（0x91032，批号 1）。
    {
        let mut s = ConservSealer::new();
        let pushed = s.push(FaceRow::new(1, 20, 6_000, 6_000, 20)).is_ok();
        let f = s.faces();
        let err = s.declare(1).err();
        let hit = match err {
            Some(e) => e.code == 0x91032 && e.batch == 1,
            None => false,
        };
        cs.add("f21587.single_batch", pushed && f.items == 20 && hit, "单批账未被判条目面不足（批号应为 1）");
    }

    // 13) 边界·第 41 批（上限 +1）：容量闸拒绝，计数只加一次，批数仍 40。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let err = s.push(FaceRow::new(41, 20, 6_000, 6_000, 20));
        let hit = match err {
            Err(e) => e.code == 0x91033 && e.batch == 41,
            Ok(_) => false,
        };
        cs.add("f21587.capacity_41", hit && s.rejected() == 1 && s.batch_count() == 40, "第 41 批未被拒或计数不符（应 0x91033/批 41/拒 1 次）");
    }

    // 14) 幂等·同账重声明：指纹逐位相同，第二次标 reaffirmed，签发次数加 1。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let a = s.declare(7).ok();
        let b = s.declare(7).ok();
        let same = match (a, b) {
            (Some(x), Some(y)) => x.fingerprint == y.fingerprint && y.reaffirmed && !x.reaffirmed && x.signer == y.signer,
            _ => false,
        };
        cs.add("f21587.redeclare_idempotent", same && s.declarations() == 2, "同账重声明指纹变化或 reaffirmed 未置位");
    }

    // 15) 反向·声明失效：签发后把 B05/B06 重分配成 21:6,300 与 19:5,700（三面
    //     总数与互校全过，只有逐批绑定变了）→ 再签必判 0x91038，旧单复核也失效。
    {
        let mut s = match standard_book() { Ok(s) => s, Err(_) => ConservSealer::new() };
        let first = s.declare(3).ok();
        let a = s.retune(5, FaceRow::new(5, 21, 6_000, 6_300, 21));
        let b = s.retune(6, FaceRow::new(6, 19, 6_000, 5_700, 19));
        let f = s.faces();
        let code = s.declare(3).err().map(|e| e.code);
        let verify_code = match &first {
            Some(d) => s.verify(d).err().map(|e| e.code),
            None => None,
        };
        cs.add(
            "f21587.stale_declaration",
            a && b && f.hits() && s.cross().all_hold() && code == Some(0x91038) && verify_code == Some(0x91038),
            "账面重绑定后声明未失效（应 0x91038，且旧单复核同样失效）",
        );
    }

    cs
}
