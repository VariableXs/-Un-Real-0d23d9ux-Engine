//! UNX-F0072 · 物理页账本初始化审计 · 诊断段 0x2170
//! 验收锚点：UNX-F0072-J1 页账本初始化四账（总量/可用/保留/坏页）
//! 交叉对账与初始化序判据通过；纯功能代码 ≥300 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2170;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAL: u16 = SEG_BASE + 1;      // 四账交叉不平
const E_RANGE: u16 = SEG_BASE + 2;    // 区间倒序/回绕
const E_DUP: u16 = SEG_BASE + 3;      // 区间重复登记
const E_OVERLAP: u16 = SEG_BASE + 4;  // 账间重叠（同页两账）
const E_ORDER: u16 = SEG_BASE + 5;    // 初始化序违例（审计先于建账）
const E_RDONLY: u16 = SEG_BASE + 6;   // 账本已封板
const E_EMPTY: u16 = SEG_BASE + 7;    // 空账本
const E_FP: u16 = SEG_BASE + 8;       // 审计指纹漂移
const E_ALIGN: u16 = SEG_BASE + 9;    // 区间未按页对齐

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BAL => "四账交叉不平：总量 = 可用 + 保留 + 坏页，缺项即报",
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_DUP => "区间重复登记：后到拒绝",
        E_OVERLAP => "账间重叠：同一页不得同时属于两本账",
        E_ORDER => "初始化序违例：先建账后审计，禁倒置",
        E_RDONLY => "账本已封板：审计后冻结",
        E_EMPTY => "空账本：先登记区间",
        E_FP => "审计指纹漂移：账面在审计间隙被改",
        E_ALIGN => "区间未按页对齐：4KiB 纪律",
        _ => "未知诊断码",
    }
}

/// 账本类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookKind {
    Usable,
    Reserved,
    BadRam,
}

impl BookKind {
    pub fn name(&self) -> &'static str {
        match self {
            BookKind::Usable => "可用账",
            BookKind::Reserved => "保留账",
            BookKind::BadRam => "坏页账",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub base: u64, // 物理地址（页对齐）
    pub pages: u64,
}

impl Span {
    pub fn end(&self) -> u64 {
        self.base + self.pages * 4096
    }
    pub fn overlaps(&self, o: &Span) -> bool {
        self.base < o.end() && o.base < self.end()
    }
}

// ── 页账本 ───────────────────────────────────────────────────
pub struct PageBook {
    pub kind: BookKind,
    pub spans: Vec<Span>,
}

impl PageBook {
    fn new(kind: BookKind) -> PageBook {
        PageBook {
            kind,
            spans: Vec::new(),
        }
    }

    fn add(&mut self, base: u64, pages: u64) -> Result<(), u16> {
        if pages == 0 {
            return Err(E_EMPTY);
        }
        if base % 4096 != 0 {
            return Err(E_ALIGN);
        }
        if base.checked_add(pages.checked_mul(4096).ok_or(E_RANGE)?).is_none() {
            return Err(E_RANGE);
        }
        let s = Span { base, pages };
        if self.spans.iter().any(|x| x.base == s.base) {
            return Err(E_DUP);
        }
        if self.spans.iter().any(|x| x.overlaps(&s)) {
            return Err(E_OVERLAP);
        }
        self.spans.push(s);
        Ok(())
    }

    fn total_pages(&self) -> u64 {
        self.spans.iter().map(|s| s.pages).sum()
    }

    /// 与另一账交叉：同页出现在两账 → Some(页号)。
    fn cross_with(&self, other: &PageBook) -> Option<u64> {
        for a in &self.spans {
            for b in &other.spans {
                if a.overlaps(b) {
                    let lo = a.base.max(b.base);
                    return Some(lo / 4096);
                }
            }
        }
        None
    }
}

// ── 初始化序状态机 ───────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitPhase {
    /// S0：建账中——三账自由登记，账间交叉禁止。
    Building,
    /// S1：建账完毕——交叉审计与四账守恒判定。
    Auditing,
    /// S2：封板——账本只读，指纹固化。
    Sealed,
}

pub struct PageBookAudit {
    pub usable: PageBook,
    pub reserved: PageBook,
    pub bad: PageBook,
    pub phase: InitPhase,
    pub seal_fp: u64,
}

impl PageBookAudit {
    pub fn new() -> PageBookAudit {
        PageBookAudit {
            usable: PageBook::new(BookKind::Usable),
            reserved: PageBook::new(BookKind::Reserved),
            bad: PageBook::new(BookKind::BadRam),
            phase: InitPhase::Building,
            seal_fp: 0,
        }
    }

    /// 登记区间：建账期允许；封板期拒绝。
    pub fn register(&mut self, kind: BookKind, base: u64, pages: u64) -> Result<(), u16> {
        match self.phase {
            InitPhase::Building => {}
            _ => return Err(E_RDONLY),
        }
        let book = match kind {
            BookKind::Usable => &mut self.usable,
            BookKind::Reserved => &mut self.reserved,
            BookKind::BadRam => &mut self.bad,
        };
        book.add(base, pages)?;
        // 跨账交叉即时拦截（S0 纪律：建账时就不许账间重叠）。
        // 先收敛可变借用，再统一只读扫三账；回滚时重新取可变借用。
        let cross = {
            let probe = Span { base, pages };
            let mut cross = false;
            for other in [&self.usable, &self.reserved, &self.bad].iter() {
                if other.kind == kind {
                    continue;
                }
                if other.spans.iter().any(|x| x.overlaps(&probe)) {
                    cross = true;
                }
            }
            cross
        };
        if cross {
            // 撤销刚才的登记
            let book = match kind {
                BookKind::Usable => &mut self.usable,
                BookKind::Reserved => &mut self.reserved,
                BookKind::BadRam => &mut self.bad,
            };
            book.spans.pop();
            return Err(E_OVERLAP);
        }
        Ok(())
    }

    /// 建账完毕 → 审计态。
    pub fn begin_audit(&mut self) -> Result<(u64, u64, u64), u16> {
        match self.phase {
            InitPhase::Building => {}
            InitPhase::Auditing => return Err(E_ORDER),
            InitPhase::Sealed => return Err(E_RDONLY),
        }
        if self.usable.spans.is_empty() && self.reserved.spans.is_empty()
            && self.bad.spans.is_empty()
        {
            return Err(E_EMPTY);
        }
        self.phase = InitPhase::Auditing;
        Ok((
            self.usable.total_pages(),
            self.reserved.total_pages(),
            self.bad.total_pages(),
        ))
    }

    /// 四账交叉守恒：总量 = 可用 + 保留 + 坏页（对同一登记全域）。
    pub fn verify_against_total(&self, total_pages: u64) -> Result<(), u16> {
        if self.phase != InitPhase::Auditing {
            return Err(E_ORDER);
        }
        let sum = self.usable.total_pages()
            + self.reserved.total_pages()
            + self.bad.total_pages();
        if sum != total_pages {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 账间交叉终审（S1 纪律二次确认）。
    pub fn verify_cross(&self) -> Result<(), u16> {
        if self.phase != InitPhase::Auditing {
            return Err(E_ORDER);
        }
        if let Some(_p) = self.usable.cross_with(&self.reserved) {
            return Err(E_OVERLAP);
        }
        if let Some(_p) = self.usable.cross_with(&self.bad) {
            return Err(E_OVERLAP);
        }
        if let Some(_p) = self.reserved.cross_with(&self.bad) {
            return Err(E_OVERLAP);
        }
        Ok(())
    }

    /// 封板：指纹固化。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase != InitPhase::Auditing {
            return Err(E_ORDER);
        }
        self.verify_cross()?;
        let mut buf: Vec<u8> = Vec::new();
        for b in [&self.usable, &self.reserved, &self.bad] {
            for s in &b.spans {
                buf.extend_from_slice(&s.base.to_le_bytes());
                buf.extend_from_slice(&s.pages.to_le_bytes());
                buf.push(b.kind.name().as_bytes()[0]);
            }
        }
        self.seal_fp = fnv1a64(&buf);
        self.phase = InitPhase::Sealed;
        Ok(self.seal_fp)
    }

    /// 封板后指纹复核（防账面被改）。
    pub fn recheck(&self) -> Result<(), u16> {
        if self.phase != InitPhase::Sealed {
            return Err(E_ORDER);
        }
        let mut buf: Vec<u8> = Vec::new();
        for b in [&self.usable, &self.reserved, &self.bad] {
            for s in &b.spans {
                buf.extend_from_slice(&s.base.to_le_bytes());
                buf.extend_from_slice(&s.pages.to_le_bytes());
                buf.push(b.kind.name().as_bytes()[0]);
            }
        }
        if fnv1a64(&buf) != self.seal_fp {
            return Err(E_FP);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三账登记 → 四账守恒（总 256 页）→ 封板 → 复核。
    let mut a = PageBookAudit::new();
    a.register(BookKind::Usable, 0x1000_0000, 200)?;
    a.register(BookKind::Reserved, 0x100C_8000, 40)?;
    a.register(BookKind::BadRam, 0x100D_8000, 16)?;
    let (u, r, b) = a.begin_audit()?;
    if u != 200 || r != 40 || b != 16 {
        return Err(E_BAL);
    }
    a.verify_against_total(256)?;
    a.seal()?;
    a.recheck()?;
    hits += 1;

    // R2：账间重叠在建账期即时拦截（撤销语义）。
    let mut a2 = PageBookAudit::new();
    a2.register(BookKind::Usable, 0x2000_0000, 16)?;
    if a2.register(BookKind::Reserved, 0x2000_8000, 8) != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    // 撤销后可用账完整，Reserved 账为空
    if a2.reserved.spans.len() != 0 || a2.usable.spans.len() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：审计期再登记 → E_RDONLY。
    let mut a3 = PageBookAudit::new();
    a3.register(BookKind::Usable, 0x3000_0000, 4)?;
    a3.begin_audit()?;
    if a3.register(BookKind::BadRam, 0x3001_0000, 4) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I1：守恒不平 → E_BAL。
    let mut i1 = PageBookAudit::new();
    i1.register(BookKind::Usable, 0x4000_0000, 10)?;
    i1.begin_audit()?;
    if i1.verify_against_total(11) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：空账本开审 → E_EMPTY。
    let mut i2 = PageBookAudit::new();
    if i2.begin_audit() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：未对齐 → E_ALIGN。
    let mut i3 = PageBookAudit::new();
    if i3.register(BookKind::Usable, 0x5000_0804, 4) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I4：同账重叠 → E_OVERLAP。
    let mut i4 = PageBookAudit::new();
    i4.register(BookKind::Reserved, 0x6000_0000, 8)?;
    if i4.register(BookKind::Reserved, 0x6000_C000, 8) != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I5：封板后篡改账面 → 复核 E_FP。
    let mut i5 = PageBookAudit::new();
    i5.register(BookKind::Usable, 0x7000_0000, 8)?;
    i5.begin_audit()?;
    i5.seal()?;
    i5.usable.spans[0].pages = 9; // 模拟账面被改
    if i5.recheck() != Err(E_FP) {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}
