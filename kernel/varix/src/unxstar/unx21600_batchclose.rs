//! UNX-F21600 · B40 收官聚合与 F2 域闭环宣示
//!
//! 诊断段 0x91099~0x910A0（F2-B40 域收官段 20/100，八码）。
//!
//! 判据原文（施工书 UNX-F21600-J1）：终条：域 40 批闭环宣示 + 对全局 Varix 内核计划的
//! 对齐声明（全部围绕内核）留痕。
//!
//! 设计要点（为什么聚合器不能只信「总数」）：
//! 四十批 × 20 条 × 300 行 = 240,000，这个总数可以被「一批多写、一批少写」凑出来。
//! 所以本模型把总数拆成两层：逐批配额（条目 20、判据 20）与行数的派生对拍（行数必须等于
//! 条目 × 300）。两者分工明确——条目与判据是**原始量**，逐批钉死；行数是**派生量**，
//! 只能靠比例反算来对拍，钉死一个派生量等于把「公式写错」这类错误变成不可见。
//!
//! 双算对拍的第二路刻意不重放累加顺序：一路按插入序做「滚动求和」，另一路按批号从 40
//! 递减反查再做「派生求和」。两路的失败模式不同（前者怕漏批，后者怕批号错位），只有在
//! 表结构与数值同时自洽时才会相等；相等的意义因此比「都等于 240000」更强。
//!
//! 对齐声明放在最后判：数字都没闭环时先谈「围绕内核」是空话，先闭环再谈对齐，
//! 诊断顺序本身就是纪律的一部分。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 域内批数（B01~B40）。
pub const BATCHES: usize = 40;
/// 每批条目数（施工书：一批 20 条）。
pub const ITEMS_PER_BATCH: u16 = 20;
/// 每条折合行数（20 条 × 300 行 = 6000 行/批，来源：施工书域账口径）。
pub const LINES_PER_ITEM: u32 = 300;
/// 每批判据数。
pub const CHECKS_PER_BATCH: u16 = 20;
/// 域条目总数（40 × 20）。
pub const DOMAIN_ITEMS: u32 = 800;
/// 域行数总数（40 × 6000）。
pub const DOMAIN_LINES: u64 = 240_000;
/// 域判据总数（40 × 20）。
pub const DOMAIN_CHECKS: u32 = 800;
/// 终条编号（施工书 UNX-F21600）。
pub const TAIL_NO: u16 = 21_600;

/// 闭环诊断码闭集（0x91099~0x910A0 八码）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体会整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 0x91099 域闭环成立。
    CloseOk = 0x91099,
    /// 0x9109A 批数不足或批号不连续。
    BatchCountOff = 0x9109A,
    /// 0x9109B 某批条目/判据不足，或该批未收口。
    BatchUnderfilled = 0x9109B,
    /// 0x9109C 某批条目/判据超出配额。
    BatchOverfilled = 0x9109C,
    /// 0x9109D 双算分叉或总数不符。
    SumDiverge = 0x9109D,
    /// 0x9109E 对齐声明缺项。
    AlignMissing = 0x9109E,
    /// 0x9109F 已闭环仍再次闭环。
    AlreadyClosed = 0x9109F,
    /// 0x910A0 批表为空。
    EmptyRoll = 0x910A0,
}

impl CloseCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            CloseCode::CloseOk => "域闭环成立",
            CloseCode::BatchCountOff => "批数不足或批号不连续",
            CloseCode::BatchUnderfilled => "某批配额不足或未收口",
            CloseCode::BatchOverfilled => "某批配额超出",
            CloseCode::SumDiverge => "双算分叉或总数不符",
            CloseCode::AlignMissing => "对齐声明缺项",
            CloseCode::AlreadyClosed => "已闭环仍再次闭环",
            CloseCode::EmptyRoll => "批表为空",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            CloseCode::CloseOk => "四十批齐、逐批足额、双算合、对齐四要素全勾",
            CloseCode::BatchCountOff => "批表被截断，或批号跳跃/重号",
            CloseCode::BatchUnderfilled => "存在未满 20 条、未满 20 判据或未收口的批",
            CloseCode::BatchOverfilled => "存在被塞入超额条目的批",
            CloseCode::SumDiverge => "行数与条目数不成 300 倍比例，或总数不等于域配额",
            CloseCode::AlignMissing => "内核计划四要素未逐条打勾",
            CloseCode::AlreadyClosed => "闭环是终态操作，重复执行会覆盖留痕单",
            CloseCode::EmptyRoll => "尚未装入任何批",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            CloseCode::CloseOk => "归档留痕单，B40 域作业结束",
            CloseCode::BatchCountOff => "补齐缺批或按批号重排后再闭环",
            CloseCode::BatchUnderfilled => "补齐条目/判据并把该批标记收口",
            CloseCode::BatchOverfilled => "拆分超额批次，不要就地扩容配额",
            CloseCode::SumDiverge => "按条目数复算行数，定位不成比例的那一批",
            CloseCode::AlignMissing => "逐条打勾：内核态实现 / no_std / 判据锚 / 域归属",
            CloseCode::AlreadyClosed => "读取既有留痕单，不要重复闭环",
            CloseCode::EmptyRoll => "先装入四十批再闭环",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会截断成 0x1099）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 单批行（批号 / 条目 / 行数 / 判据 / 收口状态）。
///
/// `closed` 与数值分开存：数值够了但没宣布收口的批，仍然不算闭环——「算完」与「收口」
/// 是两件事，合并成一个字段就会让「偷偷补数」看起来等于「正式收口」。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BatchRow {
    /// 批序号（1-based，必须与所在位置一致）。
    pub no: u16,
    /// 条目数。
    pub items: u16,
    /// 声明行数。
    pub lines: u32,
    /// 判据数。
    pub checks: u16,
    /// 是否已宣布收口。
    pub closed: bool,
}

impl BatchRow {
    /// 构造一批。
    pub const fn new(no: u16, items: u16, lines: u32, checks: u16, closed: bool) -> BatchRow {
        BatchRow { no, items, lines, checks, closed }
    }

    /// 按「条目 × 300 行」派生出的应有行数。
    ///
    /// 派生而不是读取：读取 `lines` 再和它自己比是恒真。派生值来自条目这个原始量，
    /// 才构成对 `lines` 的独立约束。
    pub const fn derived_lines(&self) -> u32 {
        (self.items as u32).saturating_mul(LINES_PER_ITEM)
    }

    /// 条目是否恰好足额。
    pub const fn items_at_quota(&self) -> bool {
        self.items == ITEMS_PER_BATCH
    }

    /// 判据是否恰好足额。
    pub const fn checks_at_quota(&self) -> bool {
        self.checks == CHECKS_PER_BATCH
    }
}

/// 聚合总数（三路同构：条目 / 行数 / 判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Totals {
    /// 条目合计。
    pub items: u32,
    /// 行数合计。
    pub lines: u64,
    /// 判据合计。
    pub checks: u32,
}

impl Totals {
    /// 与另一组总数是否逐位相同（双算对拍的第一条断言）。
    pub const fn agree(&self, other: &Totals) -> bool {
        self.items == other.items && self.lines == other.lines && self.checks == other.checks
    }

    /// 是否恰为域配额（800 / 240000 / 800）。
    pub const fn is_domain(&self) -> bool {
        self.items == DOMAIN_ITEMS && self.lines == DOMAIN_LINES && self.checks == DOMAIN_CHECKS
    }
}

/// 对齐声明（内核计划四要素，逐条打勾）。
///
/// 四要素各自独立成位而不是一个 `aligned: bool`：留痕单要能回答「哪一条没勾」，
/// 一个布尔只能回答「有没有勾」，复核时等于没有信息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Alignment {
    /// 内核态实现（不是用户态脚本拼出来的）。
    pub kernel_space: bool,
    /// no_std（不依赖 std 与运行期堆管理策略）。
    pub no_std: bool,
    /// 判据锚（每条判据都能回指施工书条目）。
    pub criteria_anchor: bool,
    /// 域归属（条目落在 F2-B40 域内）。
    pub domain_owner: bool,
}

impl Alignment {
    /// 四要素全勾的声明（标准夹具用）。
    pub const fn full() -> Alignment {
        Alignment { kernel_space: true, no_std: true, criteria_anchor: true, domain_owner: true }
    }

    /// 逐条打勾结果打包成 4 位位图（留痕单里一眼看出缺哪条）。
    pub const fn bits(&self) -> u8 {
        let mut b = 0u8;
        if self.kernel_space {
            b |= 0b0001;
        }
        if self.no_std {
            b |= 0b0010;
        }
        if self.criteria_anchor {
            b |= 0b0100;
        }
        if self.domain_owner {
            b |= 0b1000;
        }
        b
    }

    /// 已勾条数。
    pub const fn count(&self) -> u8 {
        self.bits().count_ones() as u8
    }

    /// 是否四要素齐备。
    pub const fn is_complete(&self) -> bool {
        self.count() == 4
    }
}

/// 闭环故障（码 + 定位批号，缺一不可）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 定位批号（0 表示与具体批无关）。
    pub batch: u16,
}

impl CloseFault {
    const fn new(code: CloseCode, batch: u16) -> CloseFault {
        CloseFault { code: code.code(), batch }
    }
    /// 码是否落在本单码段内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91099 && self.code <= 0x910A0
    }
    /// 三要素拼一行（运维面板直读）。
    pub fn text(&self) -> String {
        let c = match self.code {
            0x91099 => CloseCode::CloseOk,
            0x9109A => CloseCode::BatchCountOff,
            0x9109B => CloseCode::BatchUnderfilled,
            0x9109C => CloseCode::BatchOverfilled,
            0x9109D => CloseCode::SumDiverge,
            0x9109E => CloseCode::AlignMissing,
            0x9109F => CloseCode::AlreadyClosed,
            _ => CloseCode::EmptyRoll,
        };
        let mut out = String::with_capacity(192);
        out.push_str(c.what());
        out.push_str(" | ");
        out.push_str(c.why());
        out.push_str(" | ");
        out.push_str(c.advice());
        out.push_str(" | batch=");
        out.push_str(&self.batch.to_string());
        out
    }
}

/// 留痕单（闭环回执）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseSlip {
    /// 批数。
    pub batches: u16,
    /// 三路总数。
    pub totals: Totals,
    /// 对齐声明位图。
    pub align_bits: u8,
    /// 合成指纹（逐批 + 对齐位咬合）。
    pub composite: u64,
    /// 终条编号。
    pub tail_no: u16,
}

impl CloseSlip {
    /// 留痕单是否自洽（数字命中 + 四要素全勾 + 终条在位 + 指纹非零）。
    pub const fn is_complete(&self) -> bool {
        self.batches == BATCHES as u16
            && self.totals.is_domain()
            && self.align_bits == 0b1111
            && self.tail_no == TAIL_NO
            && self.composite != 0
    }

    /// 留痕单一行文本（字段序固定，便于逐行 diff）。
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(192);
        out.push_str("batchclose domain=F2-B40 batches=");
        out.push_str(&self.batches.to_string());
        out.push_str(" items=");
        out.push_str(&self.totals.items.to_string());
        out.push_str(" lines=");
        out.push_str(&self.totals.lines.to_string());
        out.push_str(" checks=");
        out.push_str(&self.totals.checks.to_string());
        out.push_str(" align=");
        out.push_str(&self.align_bits.to_string());
        out.push_str(" tail=");
        out.push_str(&self.tail_no.to_string());
        out.push_str(" fp=");
        out.push_str(&hex64(self.composite));
        out
    }
}

/// 批闭环聚合器。
pub struct BatchRoll {
    rows: Vec<BatchRow>,
    align: Alignment,
    closed: bool,
    /// 成功闭环次数（幂等观测点：成功一次后恒为 1）。
    close_count: u16,
    /// 被拒闭环次数（重复闭环也要留痕）。
    rejects: u16,
    /// 闭环后的合成指纹（0 表示尚未闭环）。
    composite: u64,
}

impl BatchRoll {
    /// 空批表（对齐声明默认全不勾：未声明就是未对齐，不给「默认合规」的便宜）。
    pub const fn new() -> BatchRoll {
        BatchRoll {
            rows: Vec::new(),
            align: Alignment { kernel_space: false, no_std: false, criteria_anchor: false, domain_owner: false },
            closed: false,
            close_count: 0,
            rejects: 0,
            composite: 0,
        }
    }

    /// 设置对齐声明（闭环前必须四要素齐）。
    pub fn set_alignment(&mut self, align: Alignment) {
        self.align = align;
    }

    /// 当前对齐声明。
    pub const fn align(&self) -> Alignment {
        self.align
    }

    /// 装入一批。
    ///
    /// 批号必须等于「已装数 + 1」：批表是有序表，允许乱序装会让双算的第二路（按批号反查）
    /// 与第一路（按插入序累加）失去共同的参照系，「双算」就退化成「同一路算两遍」。
    pub fn push(&mut self, row: BatchRow) -> Result<(), CloseFault> {
        let expect = (self.rows.len() as u16).saturating_add(1);
        if row.no != expect {
            return Err(CloseFault::new(CloseCode::BatchCountOff, row.no));
        }
        if self.rows.len() >= BATCHES {
            return Err(CloseFault::new(CloseCode::BatchCountOff, row.no));
        }
        self.rows.push(row);
        Ok(())
    }

    /// 批数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 只读视图。
    pub fn rows(&self) -> &[BatchRow] {
        &self.rows
    }    /// 可写视图（判据构造反例用）。
    pub fn rows_mut(&mut self) -> &mut [BatchRow] {
        &mut self.rows
    }

    /// 第一路：按插入序滚动累加声明值。
    ///
    /// 不做任何乘法或比例假设——它忠实回答「账面上写了多少」，对「比例公式写错」不敏感。
    pub fn totals_a(&self) -> Totals {
        let mut t = Totals::default();
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            t.items = t.items.saturating_add(r.items as u32);
            t.lines = t.lines.saturating_add(r.lines as u64);
            t.checks = t.checks.saturating_add(r.checks as u32);
            i += 1;
        }
        t
    }

    /// 第二路：按批号从 40 递减反查，行数由条目派生。
    ///
    /// 反查而不是顺序遍历：它检验的是「批号到行」的映射完整且唯一——缺批、批号错位、
    /// 行数与条目不成比例，都会让这一路与第一路分叉。顺序遍历做不到这一点，因为顺序
    /// 遍历不认识批号，只认识位置。
    pub fn totals_b(&self) -> Totals {
        let mut t = Totals::default();
        let mut no = BATCHES as u16;
        while no >= 1 {
            let mut i = 0usize;
            while i < self.rows.len() {
                if self.rows[i].no == no {
                    let r = self.rows[i];
                    t.items = t.items.saturating_add(r.items as u32);
                    t.lines = t.lines.saturating_add(r.derived_lines() as u64);
                    t.checks = t.checks.saturating_add(r.checks as u32);
                    break;
                }
                i += 1;
            }
            // 缺批就什么都不加：第二路不认识「位置上有东西」，它只认批号，
            // 于是缺一批会直接在总数上少一份，与第一路分叉——分叉本身就是诊断信号。
            no -= 1;
        }
        t
    }

    /// 首个未闭环批（未收口或条目不足），用于「未闭环批定位」。
    pub fn first_unclosed(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if !r.closed || !r.items_at_quota() || !r.checks_at_quota() {
                return Some(r.no);
            }
            i += 1;
        }
        None
    }

    /// 合成指纹：逐批 (no, items, lines, checks, closed) 咬合，末尾再咬对齐位。
    ///
    /// 逐字段喂而不是喂内存：结构体内存含 padding，未初始化字节会让指纹在换编译器版本
    /// 时漂移，跨版本复核就永远对不上。
    pub fn composite(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            h = fnv_u64(h, r.no as u64);
            h = fnv_u64(h, r.items as u64);
            h = fnv_u64(h, r.lines as u64);
            h = fnv_u64(h, r.checks as u64);
            h = fnv_u64(h, if r.closed { 1 } else { 0 });
            i += 1;
        }
        fnv_u64(h, self.align.bits() as u64)
    }

    /// 闭环（四十批齐 + 逐批足额 + 双算合 + 对齐四要素齐）。
    ///
    /// 顺序「空 → 已闭环 → 批数/批号 → 逐批配额 → 双算/总数 → 对齐」：
    /// 已闭环必须排在批数之前，否则重复闭环会先报「批数不对」（其实批数一直是 40），
    /// 值班人会去补根本不缺的批。对齐放最后，因为数字没闭环时谈对齐是空话。
    pub fn close(&mut self) -> Result<CloseSlip, CloseFault> {
        let outcome = self.settle_close();
        if outcome.is_err() {
            self.rejects = self.rejects.saturating_add(1);
        }
        outcome
    }

    /// 闭环本体（不含留痕计数）。
    fn settle_close(&mut self) -> Result<CloseSlip, CloseFault> {
        if self.rows.is_empty() {
            return Err(CloseFault::new(CloseCode::EmptyRoll, 0));
        }
        if self.closed {
            return Err(CloseFault::new(CloseCode::AlreadyClosed, 0));
        }
        if self.rows.len() != BATCHES {
            return Err(CloseFault::new(CloseCode::BatchCountOff, self.rows.len() as u16));
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            let expect = (i as u16).saturating_add(1);
            if r.no != expect {
                return Err(CloseFault::new(CloseCode::BatchCountOff, r.no));
            }
            if !r.closed {
                return Err(CloseFault::new(CloseCode::BatchUnderfilled, r.no));
            }
            if r.items < ITEMS_PER_BATCH || r.checks < CHECKS_PER_BATCH {
                return Err(CloseFault::new(CloseCode::BatchUnderfilled, r.no));
            }
            if r.items > ITEMS_PER_BATCH || r.checks > CHECKS_PER_BATCH {
                return Err(CloseFault::new(CloseCode::BatchOverfilled, r.no));
            }
            i += 1;
        }
        let a = self.totals_a();
        let b = self.totals_b();
        if !a.agree(&b) {
            return Err(CloseFault::new(CloseCode::SumDiverge, 0));
        }
        if !a.is_domain() {
            return Err(CloseFault::new(CloseCode::SumDiverge, 0));
        }
        if !self.align.is_complete() {
            return Err(CloseFault::new(CloseCode::AlignMissing, 0));
        }
        self.closed = true;
        self.close_count = self.close_count.saturating_add(1);
        let fp = self.composite();
        self.composite = fp;
        Ok(CloseSlip {
            batches: self.rows.len() as u16,
            totals: a,
            align_bits: self.align.bits(),
            composite: fp,
            tail_no: TAIL_NO,
        })
    }

    /// 是否已闭环。
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// 成功闭环次数。
    pub const fn close_count(&self) -> u16 {
        self.close_count
    }

    /// 被拒闭环次数。
    pub const fn rejects(&self) -> u16 {
        self.rejects
    }

    /// 留痕单导出（未闭环时 composite 位写 `-`，字段位永远在）。
    pub fn export(&self) -> String {
        let t = self.totals_a();
        let mut out = String::with_capacity(256);
        out.push_str("batchclose batches=");
        out.push_str(&self.rows.len().to_string());
        out.push_str(" items=");
        out.push_str(&t.items.to_string());
        out.push_str(" lines=");
        out.push_str(&t.lines.to_string());
        out.push_str(" checks=");
        out.push_str(&t.checks.to_string());
        out.push_str(" align=");
        out.push_str(&self.align.bits().to_string());
        out.push_str(" closed=");
        out.push_str(if self.closed { "1" } else { "0" });
        out.push_str(" count=");
        out.push_str(&self.close_count.to_string());
        out.push_str(" rejects=");
        out.push_str(&self.rejects.to_string());
        out.push_str(" tail=");
        out.push_str(&TAIL_NO.to_string());
        out.push_str(" fp=");
        let fp = if self.composite == 0 { "-".to_string() } else { hex64(self.composite) };
        out.push_str(fp.as_str());
        out
    }
}

/// FNV-1a 64 单步（质数与偏移基来源：FNV 规范 1a 变体）。
fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化器的分配）。
fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 装满 `n` 批的批表（每批 20 条 / 6000 行 / 20 判据 / 已收口，对齐四要素全勾）。
///
/// 返回 `Result` 而不是直出：夹具本身要走一遍 `push` 的批号校验，否则「批表结构正确」
/// 这个前提就是假设出来的——后面十几条判据都会站在一个未经验证的假设上。
pub fn roll_of(n: usize) -> Result<BatchRoll, CloseFault> {
    let mut roll = BatchRoll::new();
    roll.set_alignment(Alignment::full());
    let mut i = 0usize;
    while i < n {
        let no = (i as u16).saturating_add(1);
        let row = BatchRow::new(
            no,
            ITEMS_PER_BATCH,
            (ITEMS_PER_BATCH as u32).saturating_mul(LINES_PER_ITEM),
            CHECKS_PER_BATCH,
            true,
        );
        roll.push(row)?;
        i += 1;
    }
    Ok(roll)
}

/// 标准夹具：完整四十批且对齐齐备（但不闭环，留给判据各自去闭环）。
pub fn standard_roll() -> Result<BatchRoll, CloseFault> {
    roll_of(BATCHES)
}

/// 闭环并取故障：被接受时回一个码 0 的占位故障，让位置断言必然落空。
///
/// 判据要「拿故障码」，而成功时没有故障可拿。本批禁 panic，故用「码 0」这个闭集外的
/// 值做占位：任何 `== Some(0x9109x)` 的断言都会失败，失败信息仍指出「本该被拒却被接受」。
fn refusal(roll: &mut BatchRoll) -> CloseFault {
    match roll.close() {
        Ok(_) => CloseFault { code: 0, batch: 0 },
        Err(f) => f,
    }
}

/// 判据入口（UNX-F21600-J1 及其配套十四条）。
///
/// 判据常量独立写死（40 / 20 / 6000 / 800 / 240000 / 800 / 21600），不复用被测常量名：
/// 若断言写成 `totals.items == DOMAIN_ITEMS`，把常量改成 801 时判据会跟着变，等于自证。
pub fn run_unx21600_batchclose_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21600");

    // 1) 夹具可构造（先证夹具，再证被测逻辑）。
    let standard = standard_roll();
    cs.add("f21600.fixture_buildable", standard.is_ok(), "标准四十批批表构造失败");

    // 2) 第一路：按插入序累加，三路总数命中 800 / 240000 / 800。
    let mut ok = false;
    if let Ok(roll) = &standard {
        let a = roll.totals_a();
        ok = a.items == 800 && a.lines == 240_000 && a.checks == 800 && a.is_domain();
    }
    cs.add("f21600.totals_a", ok, "第一路总数未命中 800/240000/800");

    // 3) 第二路：按批号反查 + 行数派生，与第一路逐位一致（双算对拍）。
    let mut ok = false;
    if let Ok(roll) = &standard {
        let a = roll.totals_a();
        let b = roll.totals_b();
        ok = a.agree(&b) && b.items == 800 && b.lines == 240_000 && b.checks == 800;
    }
    cs.add("f21600.totals_b", ok, "两路总数不一致或第二路数值不符");

    // 4) 正向：四十批齐 + 对齐齐 → 闭环成立，留痕单完整。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        if let Ok(slip) = roll.close() {
            ok = slip.is_complete()
                && slip.batches == 40
                && slip.totals.items == 800
                && slip.totals.lines == 240_000
                && slip.totals.checks == 800
                && slip.align_bits == 15
                && slip.tail_no == 21_600
                && roll.align().count() == 4
                && roll.is_closed()
                && roll.composite() == slip.composite
                && slip.text().contains("tail=21600")
                && slip.text().contains("align=15");
        }
    }
    cs.add("f21600.close_ok", ok, "四十批齐却闭环失败或留痕单不完整");

    // 5) 边界：39 批 → 0x9109A（批数不足）。
    let mut ok = false;
    if let Ok(mut roll) = roll_of(39) {
        let e = refusal(&mut roll);
        ok = e.code == 0x9109A && e.is_known() && !roll.is_closed();
    }
    cs.add("f21600.count_39", ok, "39 批未被拒或码不符");

    // 6) 反向：批号断口（第 21 批被改成 22）→ 0x9109A。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        roll.rows_mut()[20].no = 22;
        let e = refusal(&mut roll);
        ok = e.code == 0x9109A && e.batch == 22 && e.text().contains("批数不足或批号不连续");
    }
    cs.add("f21600.numbering_gap", ok, "批号断口未被拒或定位错");

    // 7) 反向：某批条目 19（不足）→ 0x9109B，且定位到该批。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        roll.rows_mut()[6].items = 19;
        roll.rows_mut()[6].lines = 5_700;
        let e = refusal(&mut roll);
        ok = e.code == 0x9109B && e.batch == 7;
    }
    cs.add("f21600.items_short", ok, "条目 19 未被拒或批号定位错");

    // 8) 反向：某批条目 21（超配额）→ 0x9109C。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        roll.rows_mut()[6].items = 21;
        roll.rows_mut()[6].lines = 6_300;
        let e = refusal(&mut roll);
        ok = e.code == 0x9109C && e.batch == 7;
    }
    cs.add("f21600.items_over", ok, "条目 21 未被拒或码不符");

    // 9) 反向：某批判据 19（不足）→ 0x9109B。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        roll.rows_mut()[11].checks = 19;
        let e = refusal(&mut roll);
        ok = e.code == 0x9109B && e.batch == 12;
    }
    cs.add("f21600.checks_short", ok, "判据 19 未被拒或批号定位错");

    // 10) 反向：行数与条目不成比例（条目 20 但只报 5900 行）→ 双算分叉 0x9109D。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        roll.rows_mut()[4].lines = 5_900;
        let diverge = !roll.totals_a().agree(&roll.totals_b());
        let e = refusal(&mut roll);
        ok = diverge && e.code == 0x9109D;
    }
    cs.add("f21600.lines_not_derived", ok, "行数不成比例未触发双算分叉或码不符");

    // 11) 反向：对齐缺一项（no_std 未勾）→ 0x9109E。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        let partial = Alignment { kernel_space: true, no_std: false, criteria_anchor: true, domain_owner: true };
        roll.set_alignment(partial);
        let e = refusal(&mut roll);
        ok = e.code == 0x9109E
            && partial.bits() == 0b1101
            && partial.count() == 3
            && !partial.is_complete();
    }
    cs.add("f21600.align_missing", ok, "对齐缺项未被拒或位图不符");

    // 12) 边界：空批表 → 0x910A0。
    let ok = {
        let mut roll = BatchRoll::new();
        let e = refusal(&mut roll);
        e.code == 0x910A0 && roll.len() == 0 && roll.first_unclosed().is_none()
    };
    cs.add("f21600.empty_roll", ok, "空批表未被拒或码不符");

    // 13) 未闭环批定位：第 9 批取消收口 → first_unclosed 指到 9。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        ok = roll.first_unclosed().is_none() && roll.rows().len() == 40;
        roll.rows_mut()[8].closed = false;
        ok = ok && roll.first_unclosed() == Some(9);
        let e = refusal(&mut roll);
        ok = ok && e.code == 0x9109B && e.batch == 9;
    }
    cs.add("f21600.locate_unclosed", ok, "未闭环批未被定位到 9 或码不符");

    // 14) 幂等：重复闭环被拒，合成指纹不变，留痕单文本可重放。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        if roll.close().is_ok() {
            let before = roll.composite();
            let again = roll.close();
            let text_a = roll.export();
            let text_b = roll.export();
            ok = again.as_ref().err().map(|f| f.code) == Some(0x9109F)
                && roll.close_count() == 1
                && roll.rejects() == 1
                && roll.composite() == before
                && before != 0
                && text_a == text_b
                && text_a.contains("closed=1")
                && text_a.contains("count=1")
                && text_a.contains("rejects=1");
        }
    }
    cs.add("f21600.close_idempotent", ok, "重复闭环未被拒，或计数/指纹/导出文本不稳定");

    // 15) 留痕单字段：批数、三路总数、对齐位、终条编号、指纹齐全（可归档性）。
    let mut ok = false;
    if let Ok(mut roll) = standard_roll() {
        if roll.close().is_ok() {
            let t = roll.export();
            ok = t.starts_with("batchclose batches=40 items=800 lines=240000 checks=800 align=15")
                && t.contains(" tail=21600")
                && t.contains(" fp=")
                && !t.contains("fp=-")
                && t.lines().count() == 1;
        }
    }
    cs.add("f21600.trace_slip", ok, "留痕单缺字段或指纹未落");

    cs
}
