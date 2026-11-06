//! UNX-F50108 直通与双面透传（Duplex 声明透传档）（0x439a0 段）
//!
//! 锚点：UNX-F50108-J1 双面透传 ×3 模落账；判据锚定 §AI-63 §1 防重声明（B24 联签）。
//!
//! 双面（duplex）在直通路径上同样是**声明**：直通层只解析票据/PS 字典里的
//! 声明值、按模式把页号映射到纸张正反面，随后原样递交；不做重排、不做拼版、
//! 不补空白页内容——末页背面缺页只登记为 `blank`，由设备侧印空白。
//!
//! 三模：`Simplex`（单面）、`LongEdge`（长边翻转）、`ShortEdge`（短边翻转）。
//! 长/短边的唯一差别是 tumble 位，因此「三模落账」必须证明这个位跟着声明走，
//! 而不是被直通层统一成某一档。
//!
//! 数据段：`SEG_BASE = 0x439a0`；诊断码闭集 = `SEG_BASE + 1..=9`。
//!
//! 结构清单：
//! * `DuplexMode` / `TicketForm` / `DuplexTicket` —— 声明值 + 两方言票据；
//! * `parse_ticket` / `parse_kv` / `parse_ps` —— KV 与 PS 字典双解析器；
//! * `Sheet` + `SheetPhase` —— 每张纸的正反面状态机；
//! * `DuplexLedger` —— 按页流绑定、收口、递交的账本；
//! * `DuplexRegistry` —— 作业防重 + 三模普查；
//! * `injection_core` —— 6 类坏票据逐类断码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（本模块独占 0x439a0）。
pub const SEG_BASE: u32 = 0x439a0;

pub const E_EMPTY: u16 = 1;
pub const E_UNKNOWN: u16 = 2;
pub const E_DUP: u16 = 3;
pub const E_CAPACITY: u16 = 4;
pub const E_RANGE: u16 = 5;
pub const E_ORDER: u16 = 6;
pub const E_TRUNCATED: u16 = 7;
pub const E_CONFLICT: u16 = 8;
pub const E_TUMBLE: u16 = 9;

/// 单作业最大页数（2000 页 = 1000 张纸；超过即判越界而不是静默截断）。
pub const MAX_PAGES: u16 = 2000;

/// 双面透传错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtDuplexError {
    /// 票据为空 / 页数为 0。
    Empty,
    /// 模式字面量或数值不在闭集。
    Unknown,
    /// 同一作业重复声明且模式相同。
    Dup,
    /// 注册表/账本容量满。
    Capacity,
    /// 作业号 0、页数超上限、副本数越界。
    Range,
    /// 纸张状态机非法转移（乱序绑定、重复递交、未收口即递交）。
    Order,
    /// PS 字典 `<<` 与 `>>` 不配对。
    Truncated,
    /// 同一作业两次声明模式不同；或 `/Duplex false` 却给了双面模式。
    Conflict,
    /// 声明值与 tumble 位自相矛盾。
    Tumble,
}

impl PtDuplexError {
    /// 段内偏移（1..=9）。
    pub fn n(self) -> u16 {
        match self {
            PtDuplexError::Empty => E_EMPTY,
            PtDuplexError::Unknown => E_UNKNOWN,
            PtDuplexError::Dup => E_DUP,
            PtDuplexError::Capacity => E_CAPACITY,
            PtDuplexError::Range => E_RANGE,
            PtDuplexError::Order => E_ORDER,
            PtDuplexError::Truncated => E_TRUNCATED,
            PtDuplexError::Conflict => E_CONFLICT,
            PtDuplexError::Tumble => E_TUMBLE,
        }
    }

    /// 全仓唯一诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.n() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            PtDuplexError::Empty => "duplex ticket empty or zero pages",
            PtDuplexError::Unknown => "duplex mode outside closed set",
            PtDuplexError::Dup => "duplicate job declaration",
            PtDuplexError::Capacity => "registry or ledger full",
            PtDuplexError::Range => "job/pages/copies out of range",
            PtDuplexError::Order => "sheet state transition rejected",
            PtDuplexError::Truncated => "postscript dict not closed",
            PtDuplexError::Conflict => "duplex declaration conflicts with existing job",
            PtDuplexError::Tumble => "tumble bit contradicts declared mode",
        }
    }

    /// 码值反解（零漂移判据用）。
    pub fn from_code(code: u32) -> Option<PtDuplexError> {
        if code <= SEG_BASE {
            return None;
        }
        match (code - SEG_BASE) as u16 {
            E_EMPTY => Some(PtDuplexError::Empty),
            E_UNKNOWN => Some(PtDuplexError::Unknown),
            E_DUP => Some(PtDuplexError::Dup),
            E_CAPACITY => Some(PtDuplexError::Capacity),
            E_RANGE => Some(PtDuplexError::Range),
            E_ORDER => Some(PtDuplexError::Order),
            E_TRUNCATED => Some(PtDuplexError::Truncated),
            E_CONFLICT => Some(PtDuplexError::Conflict),
            E_TUMBLE => Some(PtDuplexError::Tumble),
            _ => None,
        }
    }
}

/// 三档双面模式。`tumble` 是长/短边的唯一差异位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexMode {
    Simplex,
    LongEdge,
    ShortEdge,
}

impl DuplexMode {
    /// 线值（与 PJL `DUPLEX` 取值对齐：1=单面 2=长边 3=短边）。
    pub fn code(self) -> u8 {
        match self {
            DuplexMode::Simplex => 1,
            DuplexMode::LongEdge => 2,
            DuplexMode::ShortEdge => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<DuplexMode, PtDuplexError> {
        match c {
            1 => Ok(DuplexMode::Simplex),
            2 => Ok(DuplexMode::LongEdge),
            3 => Ok(DuplexMode::ShortEdge),
            _ => Err(PtDuplexError::Unknown),
        }
    }

    /// 短边翻转 = tumble；长边与单面都不翻。
    pub fn tumble(self) -> bool {
        matches!(self, DuplexMode::ShortEdge)
    }

    /// 承载 `pages` 页所需纸张数（双面按 2 页/张向上取整）。
    pub fn sheets_for(self, pages: u16) -> u16 {
        match self {
            DuplexMode::Simplex => pages,
            DuplexMode::LongEdge | DuplexMode::ShortEdge => pages.saturating_add(1) / 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DuplexMode::Simplex => "simplex",
            DuplexMode::LongEdge => "duplex-long-edge",
            DuplexMode::ShortEdge => "duplex-short-edge",
        }
    }
}

/// 票据方言：键值串（驱动/队列）或 PS 字典（原生 PS 直通流）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketForm {
    KeyValue,
    PsDict,
}

/// 解析后的声明。`form` 只用于溯源，不参与调度判等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplexTicket {
    pub job: u16,
    pub mode: DuplexMode,
    pub pages: u16,
    pub copies: u8,
    pub form: TicketForm,
}

fn is_sep(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | b'<' | b'>' | b'(' | b')')
}

fn trim(s: &[u8]) -> &[u8] {
    let mut lo = 0usize;
    let mut hi = s.len();
    while lo < hi {
        match s.get(lo) {
            Some(&c) if is_sep(c) => lo += 1,
            _ => break,
        }
    }
    while hi > lo {
        match s.get(hi - 1) {
            Some(&c) if is_sep(c) => hi -= 1,
            _ => break,
        }
    }
    match s.get(lo..hi) {
        Some(v) => v,
        None => &s[0..0],
    }
}

fn eq_ic(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(&v) => v.to_ascii_lowercase(),
            None => return false,
        };
        let y = match b.get(i) {
            Some(&v) => v.to_ascii_lowercase(),
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

fn parse_u16(word: &[u8]) -> Result<u16, PtDuplexError> {
    if word.is_empty() || word.len() > 5 {
        return Err(PtDuplexError::Unknown);
    }
    let s = match core::str::from_utf8(word) {
        Ok(v) => v,
        Err(_) => return Err(PtDuplexError::Unknown),
    };
    match s.parse::<u16>() {
        Ok(v) => Ok(v),
        Err(_) => Err(PtDuplexError::Unknown),
    }
}

fn tokenize(text: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        let c = match text.get(i) {
            Some(&v) => v,
            None => break,
        };
        if is_sep(c) && c != b'<' && c != b'>' {
            i += 1;
            continue;
        }
        if c == b'<' {
            if let Some(&b'<') = text.get(i + 1) {
                if let Some(sl) = text.get(i..i + 2) {
                    out.push(sl);
                }
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if c == b'>' {
            if let Some(&b'>') = text.get(i + 1) {
                if let Some(sl) = text.get(i..i + 2) {
                    out.push(sl);
                }
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        // 名字（/xxx）与裸词都取到下一个分隔符为止。
        let start = i;
        if c == b'/' {
            i += 1;
        }
        while i < text.len() {
            match text.get(i) {
                Some(&d) if !is_sep(d) => i += 1,
                _ => break,
            }
        }
        if let Some(sl) = text.get(start..i) {
            if !sl.is_empty() {
                out.push(sl);
            }
        }
    }
    out
}

fn parse_kv(job: u16, text: &[u8]) -> Result<DuplexTicket, PtDuplexError> {
    let mut mode: Option<DuplexMode> = None;
    let mut pages: Option<u16> = None;
    let mut copies: u16 = 1;
    let mut tumble: Option<bool> = None;
    for seg in text.split(|&b| b == b';') {
        let seg = trim(seg);
        if seg.is_empty() {
            continue;
        }
        let mut eq: Option<usize> = None;
        let mut k = 0usize;
        while k < seg.len() {
            if let Some(&b) = seg.get(k) {
                if b == b'=' {
                    eq = Some(k);
                    break;
                }
            }
            k += 1;
        }
        let pos = match eq {
            Some(p) => p,
            None => return Err(PtDuplexError::Unknown),
        };
        let key = trim(match seg.get(..pos) {
            Some(v) => v,
            None => return Err(PtDuplexError::Unknown),
        });
        let val = trim(match seg.get(pos + 1..) {
            Some(v) => v,
            None => return Err(PtDuplexError::Unknown),
        });
        if eq_ic(key, b"duplex") {
            // 键值方言只接受无歧义字面量；`TUMBLE` 单独出现语义不明，判 Unknown。
            let m = if eq_ic(val, b"none") || eq_ic(val, b"simplex") || eq_ic(val, b"false") {
                DuplexMode::Simplex
            } else if eq_ic(val, b"long") || eq_ic(val, b"longedge") {
                DuplexMode::LongEdge
            } else if eq_ic(val, b"short") || eq_ic(val, b"shortedge") {
                DuplexMode::ShortEdge
            } else {
                return Err(PtDuplexError::Unknown);
            };
            mode = Some(m);
        } else if eq_ic(key, b"pages") {
            pages = Some(parse_u16(val)?);
        } else if eq_ic(key, b"copies") {
            let c = parse_u16(val)?;
            if c == 0 || c > 255 {
                return Err(PtDuplexError::Range);
            }
            copies = c;
        } else if eq_ic(key, b"tumble") {
            if eq_ic(val, b"1") || eq_ic(val, b"true") {
                tumble = Some(true);
            } else if eq_ic(val, b"0") || eq_ic(val, b"false") {
                tumble = Some(false);
            } else {
                return Err(PtDuplexError::Unknown);
            }
        }
        // 未知键忽略：驱动扩展键不归直通层管（前向兼容）。
    }
    let mode = match mode {
        Some(m) => m,
        None => return Err(PtDuplexError::Unknown),
    };
    let pages = match pages {
        Some(p) => p,
        None => return Err(PtDuplexError::Empty),
    };
    if pages == 0 {
        return Err(PtDuplexError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(PtDuplexError::Range);
    }
    if let Some(t) = tumble {
        // 声明与 tumble 位必须自洽：短边才翻，长边/单面不翻。
        if t != mode.tumble() {
            return Err(PtDuplexError::Tumble);
        }
    }
    Ok(DuplexTicket {
        job,
        mode,
        pages,
        copies: copies as u8,
        form: TicketForm::KeyValue,
    })
}

fn parse_bool(w: &[u8]) -> Result<bool, PtDuplexError> {
    if eq_ic(w, b"true") {
        Ok(true)
    } else if eq_ic(w, b"false") {
        Ok(false)
    } else {
        Err(PtDuplexError::Unknown)
    }
}

fn parse_ps_mode(w: &[u8]) -> Result<DuplexMode, PtDuplexError> {
    if eq_ic(w, b"/duplextumble") {
        Ok(DuplexMode::ShortEdge)
    } else if eq_ic(w, b"/duplexnotumble") {
        Ok(DuplexMode::LongEdge)
    } else if eq_ic(w, b"/simplex") {
        Ok(DuplexMode::Simplex)
    } else {
        Err(PtDuplexError::Unknown)
    }
}

fn parse_ps(job: u16, text: &[u8]) -> Result<DuplexTicket, PtDuplexError> {
    let tk = tokenize(text);
    if tk.is_empty() {
        return Err(PtDuplexError::Empty);
    }
    let mut opens = 0u16;
    let mut closes = 0u16;
    let mut i = 0usize;
    while i < tk.len() {
        match tk.get(i) {
            Some(&t) if t == &b"<<"[..] => opens = opens.saturating_add(1),
            Some(&t) if t == &b">>"[..] => closes = closes.saturating_add(1),
            _ => {}
        }
        i += 1;
    }
    if opens != closes || opens == 0 {
        return Err(PtDuplexError::Truncated);
    }
    let mut duplex: Option<bool> = None;
    let mut mode: Option<DuplexMode> = None;
    let mut pages: Option<u16> = None;
    let mut copies: u16 = 1;
    let mut i = 0usize;
    while i < tk.len() {
        let t = match tk.get(i) {
            Some(&v) => v,
            None => break,
        };
        let next = match tk.get(i + 1) {
            Some(&v) => Some(v),
            None => None,
        };
        if t == &b"/Duplex"[..] {
            if let Some(v) = next {
                duplex = Some(parse_bool(v)?);
            }
        } else if t == &b"/DuplexMode"[..] {
            if let Some(v) = next {
                mode = Some(parse_ps_mode(v)?);
            }
        } else if t == &b"/NumPages"[..] {
            if let Some(v) = next {
                pages = Some(parse_u16(v)?);
            }
        } else if t == &b"/NumCopies"[..] {
            if let Some(v) = next {
                let c = parse_u16(v)?;
                if c == 0 || c > 255 {
                    return Err(PtDuplexError::Range);
                }
                copies = c;
            }
        }
        i += 1;
    }
    let d = match duplex {
        Some(v) => v,
        None => return Err(PtDuplexError::Unknown),
    };
    let m = match mode {
        Some(v) => v,
        None => return Err(PtDuplexError::Unknown),
    };
    if !d && m != DuplexMode::Simplex {
        return Err(PtDuplexError::Conflict);
    }
    let effective = if d { m } else { DuplexMode::Simplex };
    let pages = match pages {
        Some(p) => p,
        None => return Err(PtDuplexError::Empty),
    };
    if pages == 0 {
        return Err(PtDuplexError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(PtDuplexError::Range);
    }
    Ok(DuplexTicket {
        job,
        mode: effective,
        pages,
        copies: copies as u8,
        form: TicketForm::PsDict,
    })
}

/// 解析一张票据（两种方言共用同一错误闭集）。
pub fn parse_ticket(job: u16, text: &[u8], form: TicketForm) -> Result<DuplexTicket, PtDuplexError> {
    if job == 0 {
        return Err(PtDuplexError::Range);
    }
    if text.is_empty() {
        return Err(PtDuplexError::Empty);
    }
    match form {
        TicketForm::KeyValue => parse_kv(job, text),
        TicketForm::PsDict => parse_ps(job, text),
    }
}

// ---------------------------------------------------------------------------
// 纸张状态机与账本
// ---------------------------------------------------------------------------

/// 单张纸的状态：空白 → 已置正面 → 双面齐 → 已递交。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetPhase {
    Blank,
    Front,
    Complete,
    Emitted,
}

/// 一张纸的正反面页号。`None` 表示该面没有内容（末页背面空白）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub index: u16,
    pub front: Option<u16>,
    pub back: Option<u16>,
    pub phase: SheetPhase,
}

/// 双面透传账本：页流按声明逐个绑定到纸张面，收口后逐张递交。
pub struct DuplexLedger {
    pub ticket: DuplexTicket,
    sheets: Vec<Sheet>,
    next_page: u16,
    blanks: u16,
    emitted: u16,
    closed: bool,
    rejected: u16,
}

impl DuplexLedger {
    /// 按票据建账（只建空纸，不预填页号——页流必须逐页到达）。
    pub fn open(ticket: DuplexTicket) -> Result<DuplexLedger, PtDuplexError> {
        if ticket.pages == 0 {
            return Err(PtDuplexError::Empty);
        }
        if ticket.pages > MAX_PAGES {
            return Err(PtDuplexError::Range);
        }
        let n = ticket.mode.sheets_for(ticket.pages);
        let mut sheets = Vec::with_capacity(n as usize);
        let mut i = 0u16;
        while i < n {
            sheets.push(Sheet {
                index: i,
                front: None,
                back: None,
                phase: SheetPhase::Blank,
            });
            i += 1;
        }
        Ok(DuplexLedger {
            ticket,
            sheets,
            next_page: 1,
            blanks: 0,
            emitted: 0,
            closed: false,
            rejected: 0,
        })
    }

    pub fn sheet_count(&self) -> u16 {
        self.sheets.len() as u16
    }

    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    pub fn next_page(&self) -> u16 {
        self.next_page
    }

    pub fn blanks(&self) -> u16 {
        self.blanks
    }

    pub fn emitted(&self) -> u16 {
        self.emitted
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 页流绑定：页号必须恰好是下一张待绑页（乱序即拒，保证不重排）。
    pub fn bind_next(&mut self, page: u16) -> Result<usize, PtDuplexError> {
        if self.closed {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if page != self.next_page {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if page == 0 || page > self.ticket.pages {
            self.rejected += 1;
            return Err(PtDuplexError::Range);
        }
        let (idx, front_side) = match self.ticket.mode {
            DuplexMode::Simplex => (page - 1, true),
            DuplexMode::LongEdge | DuplexMode::ShortEdge => ((page - 1) / 2, page % 2 == 1),
        };
        let slot = idx as usize;
        let phase = match self.sheets.get(slot) {
            Some(sh) => sh.phase,
            None => {
                self.rejected += 1;
                return Err(PtDuplexError::Range);
            }
        };
        if front_side {
            if phase != SheetPhase::Blank {
                self.rejected += 1;
                return Err(PtDuplexError::Order);
            }
        } else if phase != SheetPhase::Front {
            // 背面必须在正面之后：单页/乱序都会走到这里。
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if let Some(sh) = self.sheets.get_mut(slot) {
            if front_side {
                sh.front = Some(page);
                sh.phase = if self.ticket.mode == DuplexMode::Simplex {
                    SheetPhase::Complete
                } else {
                    SheetPhase::Front
                };
            } else {
                sh.back = Some(page);
                sh.phase = SheetPhase::Complete;
            }
        }
        self.next_page = self.next_page.saturating_add(1);
        Ok(slot)
    }

    /// 收口：页流必须覆盖 1..=pages；奇页双面的末张背面记空白（不造内容）。
    pub fn close(&mut self) -> Result<u16, PtDuplexError> {
        if self.closed {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if self.next_page != self.ticket.pages.saturating_add(1) {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        let mut i = 0usize;
        while i < self.sheets.len() {
            let phase = match self.sheets.get(i) {
                Some(sh) => sh.phase,
                None => SheetPhase::Blank,
            };
            if phase == SheetPhase::Front {
                // 只剩正面：双面模式的末张（页数为奇），背面按空白落账。
                if let Some(sh) = self.sheets.get_mut(i) {
                    sh.phase = SheetPhase::Complete;
                }
                self.blanks = self.blanks.saturating_add(1);
            }
            i += 1;
        }
        self.closed = true;
        Ok(self.blanks)
    }

    /// 绑定 1..=pages 全部页并收口（测试与批量递交共用）。
    pub fn bind_all(&mut self) -> Result<u16, PtDuplexError> {
        let pages = self.ticket.pages;
        let mut p = 1u16;
        while p <= pages {
            self.bind_next(p)?;
            p = p.saturating_add(1);
        }
        self.close()
    }

    /// 逐张递交：必须先收口，且每张只能递交一次。
    pub fn emit_next(&mut self) -> Result<u16, PtDuplexError> {
        if !self.closed {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if self.emitted >= self.sheet_count() {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        let slot = self.emitted as usize;
        let phase = match self.sheets.get(slot) {
            Some(sh) => sh.phase,
            None => {
                self.rejected += 1;
                return Err(PtDuplexError::Order);
            }
        };
        if phase != SheetPhase::Complete {
            self.rejected += 1;
            return Err(PtDuplexError::Order);
        }
        if let Some(sh) = self.sheets.get_mut(slot) {
            sh.phase = SheetPhase::Emitted;
        }
        self.emitted = self.emitted.saturating_add(1);
        Ok(self.emitted)
    }

    /// 页轨迹：按纸张顺序展开正反面页号（跳过空白面）。
    /// 用于证明「不重排」——轨迹必须恒等于 1,2,3,...,pages。
    pub fn page_trace(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.ticket.pages as usize);
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                if let Some(p) = sh.front {
                    out.push(p);
                }
                if let Some(p) = sh.back {
                    out.push(p);
                }
            }
            i += 1;
        }
        out
    }

    /// 面占用统计：`(正面数, 背面数, 空白背面数)`。
    pub fn face_census(&self) -> (u16, u16, u16) {
        let mut f = 0u16;
        let mut b = 0u16;
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                if sh.front.is_some() {
                    f += 1;
                }
                if sh.back.is_some() {
                    b += 1;
                }
            }
            i += 1;
        }
        (f, b, self.blanks)
    }
}

/// 作业登记表：防重 + 模式冲突 + 三模普查。
pub struct DuplexRegistry {
    cap: usize,
    ledgers: Vec<DuplexLedger>,
    rejected: u32,
}

impl DuplexRegistry {
    pub fn new(cap: usize) -> DuplexRegistry {
        DuplexRegistry {
            cap,
            ledgers: Vec::new(),
            rejected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn ledgers(&self) -> &[DuplexLedger] {
        &self.ledgers
    }

    pub fn ledger_of(&self, job: u16) -> Option<&DuplexLedger> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.ticket.job == job {
                    return Some(l);
                }
            }
            i += 1;
        }
        None
    }

    /// 三模普查：`[simplex, long, short]` 各落几条——J1「×3 模落账」的度量。
    pub fn mode_census(&self) -> [u16; 3] {
        let mut out = [0u16; 3];
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                let k = match l.ticket.mode {
                    DuplexMode::Simplex => 0usize,
                    DuplexMode::LongEdge => 1usize,
                    DuplexMode::ShortEdge => 2usize,
                };
                if let Some(c) = out.get_mut(k) {
                    *c = c.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 提交票据：解析 → 防重/冲突 → 建账。任一步失败都显式计数。
    pub fn submit(
        &mut self,
        job: u16,
        text: &[u8],
        form: TicketForm,
    ) -> Result<usize, PtDuplexError> {
        if self.cap == 0 || self.ledgers.len() >= self.cap {
            self.rejected += 1;
            return Err(PtDuplexError::Capacity);
        }
        let ticket = match parse_ticket(job, text, form) {
            Ok(t) => t,
            Err(e) => {
                self.rejected += 1;
                return Err(e);
            }
        };
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.ticket.job == job {
                    self.rejected += 1;
                    return Err(if l.ticket.mode == ticket.mode {
                        PtDuplexError::Dup
                    } else {
                        PtDuplexError::Conflict
                    });
                }
            }
            i += 1;
        }
        let ledger = match DuplexLedger::open(ticket) {
            Ok(l) => l,
            Err(e) => {
                self.rejected += 1;
                return Err(e);
            }
        };
        let idx = self.ledgers.len();
        self.ledgers.push(ledger);
        Ok(idx)
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 6 类坏票据：每类断言**具体**诊断码，不做「任意错误都算过」。
pub fn injection_core() -> (usize, usize) {
    let total = 6usize;
    let mut caught = 0usize;
    let mut reg = DuplexRegistry::new(8);

    if reg.submit(1, b"", TicketForm::KeyValue).err() == Some(PtDuplexError::Empty) {
        caught += 1;
    }
    if reg.submit(2, b"DUPLEX=TUMBLE;PAGES=4", TicketForm::KeyValue).err()
        == Some(PtDuplexError::Unknown)
    {
        caught += 1;
    }
    if reg.submit(3, b"PAGES=4;DUPLEX=SHORT", TicketForm::KeyValue).is_ok() {
        if reg.submit(4, b"DUPLEX=XY;PAGES=4", TicketForm::KeyValue).err()
            == Some(PtDuplexError::Unknown)
        {
            caught += 1;
        }
        if reg.submit(3, b"DUPLEX=SHORT;PAGES=4", TicketForm::KeyValue).err()
            == Some(PtDuplexError::Dup)
        {
            caught += 1;
        }
        if reg
            .submit(3, b"DUPLEX=LONG;PAGES=4", TicketForm::KeyValue)
            .err()
            == Some(PtDuplexError::Conflict)
        {
            caught += 1;
        }
    }
    if reg
        .submit(5, b"<< /Duplex true /DuplexMode /DuplexTumble /NumPages 4", TicketForm::PsDict)
        .err()
        == Some(PtDuplexError::Truncated)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn ticket_of(job: u16, mode: DuplexMode, pages: u16) -> DuplexTicket {
    DuplexTicket {
        job,
        mode,
        pages,
        copies: 1,
        form: TicketForm::KeyValue,
    }
}

pub fn run_unx50108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50108");

    // J1-1 三模落账：三张票据各建一本账，普查 [1,1,1]
    let mut reg = DuplexRegistry::new(8);
    let mut ok = true;
    if reg.submit(101, b"DUPLEX=NONE;PAGES=5", TicketForm::KeyValue).is_err() {
        ok = false;
    }
    if reg.submit(102, b"DUPLEX=LONG;PAGES=5", TicketForm::KeyValue).is_err() {
        ok = false;
    }
    if reg.submit(103, b"DUPLEX=SHORT;PAGES=5", TicketForm::KeyValue).is_err() {
        ok = false;
    }
    let census = reg.mode_census();
    ok = ok && census == [1, 1, 1] && reg.len() == 3;
    let det = format!("census={},{},{}", census[0], census[1], census[2]);
    s.add_dyn("50108-J1-三模落账", ok, &det);

    // J1-2 纸张数：单面 5 张 / 双面 3 张（页数相同时两双面模式一致）
    let mut counts_ok = true;
    let mut i = 0usize;
    while i < reg.ledgers().len() {
        match reg.ledgers().get(i) {
            Some(l) => {
                let want = l.ticket.mode.sheets_for(5);
                if l.sheet_count() != want {
                    counts_ok = false;
                }
            }
            None => counts_ok = false,
        }
        i += 1;
    }
    let simplex_n = match reg.ledger_of(101) {
        Some(l) => l.sheet_count(),
        None => 0,
    };
    let long_n = match reg.ledger_of(102) {
        Some(l) => l.sheet_count(),
        None => 0,
    };
    let short_n = match reg.ledger_of(103) {
        Some(l) => l.sheet_count(),
        None => 0,
    };
    let ok = counts_ok && simplex_n == 5 && long_n == 3 && short_n == 3;
    let det = format!("sheets={}/{}/{}", simplex_n, long_n, short_n);
    s.add_dyn("50108-J1-纸张数", ok, &det);

    // J1-3 页序不重排：三本账绑定后页轨迹恒为 1..=5
    let mut ok = true;
    let modes = [DuplexMode::Simplex, DuplexMode::LongEdge, DuplexMode::ShortEdge];
    let mut books: Vec<DuplexLedger> = Vec::new();
    let mut m = 0usize;
    while m < modes.len() {
        match DuplexLedger::open(ticket_of(200 + m as u16, modes[m], 5)) {
            Ok(l) => books.push(l),
            Err(_) => ok = false,
        }
        m += 1;
    }
    let mut i = 0usize;
    while i < books.len() {
        if let Some(l) = books.get_mut(i) {
            let _ = l.bind_all();
            let trace = l.page_trace();
            if trace.len() != 5 {
                ok = false;
            }
            let mut k = 0usize;
            while k < trace.len() {
                match trace.get(k) {
                    Some(&p) if p as usize == k + 1 => {}
                    _ => ok = false,
                }
                k += 1;
            }
        }
        i += 1;
    }
    s.add("50108-J1-页序不重排", ok, if ok { "" } else { "页序被重排" });

    // J1-4 tumble 位随声明：长边 false、短边 true、单面 false
    let lt = ticket_of(301, DuplexMode::LongEdge, 4);
    let st = ticket_of(302, DuplexMode::ShortEdge, 4);
    let xt = ticket_of(303, DuplexMode::Simplex, 4);
    let ok = !lt.mode.tumble() && st.mode.tumble() && !xt.mode.tumble();
    s.add("50108-J1-tumble随声明", ok, if ok { "" } else { "翻转位被统一" });

    // J1-5 面守恒：正面数 + 背面数 = 页数；空白背是「无内容面」，不参与页计数
    let mut ok = true;
    let mut detail = format!("pages={}", 5);
    let mut j = 0usize;
    while j < books.len() {
        if let Some(l) = books.get_mut(j) {
            let _ = l.bind_all();
            let (f, b, blank) = l.face_census();
            if f.saturating_add(b) != l.ticket.pages {
                ok = false;
            }
            let odd_duplex = l.ticket.mode != DuplexMode::Simplex && l.ticket.pages % 2 == 1;
            if odd_duplex && blank != 1 {
                ok = false;
            }
            if !odd_duplex && blank != 0 {
                ok = false;
            }
            if l.ticket.mode == DuplexMode::Simplex && b != 0 {
                ok = false;
            }
            detail = format!("m{}:f{} b{} z{}", l.ticket.mode.code(), f, b, blank);
        }
        j += 1;
    }
    s.add_dyn("50108-J1-面数守恒", ok, &detail);

    // J1-6 奇页补空：双面 5 页 → 1 张空白背面；5 张纸面占满 10 面
    let mut dup = DuplexLedger::open(ticket_of(401, DuplexMode::LongEdge, 5));
    let blanks = match dup.as_mut() {
        Ok(l) => l.bind_all(),
        Err(_) => Err(PtDuplexError::Order),
    };
    let ok = blanks == Ok(1);
    let sheets_n = match dup.as_ref() {
        Ok(l) => l.sheet_count(),
        Err(_) => 0,
    };
    let det = format!("blank={:?} sheets={}", blanks, sheets_n);
    s.add_dyn("50108-J1-奇页补空", ok, &det);

    // J1-7 单面零空白：单面 5 页 → 0 空白背面
    let mut l = match DuplexLedger::open(ticket_of(402, DuplexMode::Simplex, 5)) {
        Ok(v) => v,
        Err(_) => {
            s.add("50108-J1-单面无空背", false, "建账失败");
            return s;
        }
    };
    let b = l.bind_all();
    let ok = b == Ok(0) && l.face_census() == (5, 0, 0);
    s.add("50108-J1-单面无空背", ok, if ok { "" } else { "单面出现背面" });

    // J1-8 防重与冲突：同模式 Dup、异模式 Conflict，账本长度不变
    let mut reg3 = DuplexRegistry::new(4);
    let _ = reg3.submit(501, b"DUPLEX=LONG;PAGES=4", TicketForm::KeyValue);
    let n0 = reg3.len();
    let d = reg3.submit(501, b"DUPLEX=LONG;PAGES=4", TicketForm::KeyValue);
    let c = reg3.submit(501, b"DUPLEX=SHORT;PAGES=4", TicketForm::KeyValue);
    let ok = d.err() == Some(PtDuplexError::Dup)
        && c.err() == Some(PtDuplexError::Conflict)
        && reg3.len() == n0
        && reg3.rejected() == 2;
    s.add("50108-J1-防重与冲突", ok, if ok { "" } else { "重复未拦" });

    // J1-9 两方言等价：KV 与 PS 字典解析出同一 (mode,pages,copies)
    let kv = parse_ticket(601, b"DUPLEX=SHORT;PAGES=7;COPIES=2", TicketForm::KeyValue);
    let ps = parse_ticket(
        601,
        b"<< /Duplex true /DuplexMode /DuplexTumble /NumPages 7 /NumCopies 2 >>",
        TicketForm::PsDict,
    );
    let ok = match (kv, ps) {
        (Ok(a), Ok(b)) => {
            a.mode == b.mode && a.pages == b.pages && a.copies == b.copies && a.mode == DuplexMode::ShortEdge
        }
        _ => false,
    };
    s.add("50108-J1-方言等价", ok, if ok { "" } else { "两方言不一致" });

    // J1-10 页数上限：2001 页 → Range（不静默截断到 2000）
    let over = parse_ticket(701, b"DUPLEX=LONG;PAGES=2001", TicketForm::KeyValue);
    let cap = parse_ticket(702, b"DUPLEX=LONG;PAGES=2000", TicketForm::KeyValue);
    let ok = over.err() == Some(PtDuplexError::Range) && cap.is_ok();
    s.add("50108-J1-页数上限", ok, if ok { "" } else { "上限判定错" });

    // J1-11 空页/零页：PAGES=0 → Empty，缺 PAGES → Empty
    let z = parse_ticket(703, b"DUPLEX=LONG;PAGES=0", TicketForm::KeyValue);
    let m = parse_ticket(704, b"DUPLEX=LONG", TicketForm::KeyValue);
    let ok = z.err() == Some(PtDuplexError::Empty) && m.err() == Some(PtDuplexError::Empty);
    s.add("50108-J1-零页显式", ok, if ok { "" } else { "零页被当合法" });

    // J1-12 PS 语义冲突：/Duplex false 却给 DuplexTumble → Conflict
    let cf = parse_ticket(
        705,
        b"<< /Duplex false /DuplexMode /DuplexTumble /NumPages 4 >>",
        TicketForm::PsDict,
    );
    let ok = cf.err() == Some(PtDuplexError::Conflict);
    s.add("50108-J1-PS语义冲突", ok, if ok { "" } else { "false 被当双面" });

    // J1-13 tumble 位自相矛盾：LONG + TUMBLE=1 → Tumble
    let tm = parse_ticket(706, b"DUPLEX=LONG;PAGES=4;TUMBLE=1", TicketForm::KeyValue);
    let tv = parse_ticket(707, b"DUPLEX=SHORT;PAGES=4;TUMBLE=1", TicketForm::KeyValue);
    let ok = tm.err() == Some(PtDuplexError::Tumble) && tv.is_ok();
    s.add("50108-J1-tumble自洽", ok, if ok { "" } else { "tumble 位失配未拦" });

    // J1-14 状态机：乱序绑定 / 未收口递交 / 重复递交全部 Order
    let mut l = match DuplexLedger::open(ticket_of(801, DuplexMode::LongEdge, 4)) {
        Ok(v) => v,
        Err(_) => {
            s.add("50108-J1-纸张状态机", false, "建账失败");
            return s;
        }
    };
    let early = l.emit_next();
    let ooo = l.bind_next(2);
    let mut stages_ok = early.err() == Some(PtDuplexError::Order) && ooo.err() == Some(PtDuplexError::Order);
    let _ = l.bind_next(1);
    let twice = l.bind_next(1);
    stages_ok = stages_ok && twice.err() == Some(PtDuplexError::Order);
    let _ = l.bind_next(2);
    let _ = l.bind_next(3);
    let _ = l.bind_next(4);
    let _ = l.close();
    let e1 = l.emit_next();
    let e2 = l.emit_next();
    let e3 = l.emit_next();
    stages_ok = stages_ok && e1 == Ok(1) && e2 == Ok(2) && e3.err() == Some(PtDuplexError::Order);
    let det = format!("rej={} em={}", l.rejected(), l.emitted());
    s.add_dyn("50108-J1-纸张状态机", stages_ok, &det);

    // J1-15 容量：cap=1 时第二张票据 Capacity，已落账不丢
    let mut reg4 = DuplexRegistry::new(1);
    let a = reg4.submit(901, b"DUPLEX=NONE;PAGES=2", TicketForm::KeyValue);
    let b = reg4.submit(902, b"DUPLEX=NONE;PAGES=2", TicketForm::KeyValue);
    let ok = a.is_ok() && b.err() == Some(PtDuplexError::Capacity) && reg4.len() == 1;
    s.add("50108-J1-容量显式", ok, if ok { "" } else { "容量闸失效" });

    // J1-16 码值零漂移：9 个码互异且可反解
    let all = [
        PtDuplexError::Empty,
        PtDuplexError::Unknown,
        PtDuplexError::Dup,
        PtDuplexError::Capacity,
        PtDuplexError::Range,
        PtDuplexError::Order,
        PtDuplexError::Truncated,
        PtDuplexError::Conflict,
        PtDuplexError::Tumble,
    ];
    let mut ok = true;
    let mut seen = [false; 9];
    let mut i = 0usize;
    while i < all.len() {
        let n = all[i].n() as usize;
        if n < 1 || n > 9 || seen[n - 1] {
            ok = false;
        } else {
            seen[n - 1] = true;
        }
        if PtDuplexError::from_code(all[i].code()) != Some(all[i]) {
            ok = false;
        }
        i += 1;
    }
    ok = ok && PtDuplexError::from_code(SEG_BASE).is_none();
    s.add("50108-J1-码值不漂", ok, if ok { "" } else { "码值漂移" });

    // J1-17 注入核：6 类坏票据全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50108-J1-注入必抓", ok, &det);

    // J1-18 确定性：同一票据两次解析逐字段一致
    let a = parse_ticket(950, b"DUPLEX=SHORT;PAGES=9;COPIES=3", TicketForm::KeyValue);
    let b = parse_ticket(950, b"DUPLEX=SHORT;PAGES=9;COPIES=3", TicketForm::KeyValue);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x == y && x.copies == 3,
        _ => false,
    };
    s.add("50108-J1-解析确定", ok, if ok { "" } else { "两次解析不同" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50108_checks_green() {
        let set = run_unx50108_checks();
        assert!(set.all_passed(), "UNX-F50108 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50108_duplex_pairs_pages_in_order() {
        let mut l = DuplexLedger::open(ticket_of(1, DuplexMode::ShortEdge, 5)).expect("ledger");
        assert_eq!(l.bind_all(), Ok(1));
        let trace = l.page_trace();
        assert_eq!(trace.len(), 5);
        let mut i = 0usize;
        while i < trace.len() {
            assert_eq!(trace.get(i), Some(&((i + 1) as u16)));
            i += 1;
        }
        assert_eq!(l.face_census(), (3, 2, 1));
    }

    #[test]
    fn unx50108_mode_codes_are_stable() {
        assert_eq!(DuplexMode::Simplex.code(), 1);
        assert_eq!(DuplexMode::LongEdge.code(), 2);
        assert_eq!(DuplexMode::ShortEdge.code(), 3);
        let mut c = 1u8;
        while c <= 3 {
            assert_eq!(DuplexMode::from_code(c).map(|m| m.code()), Ok(c));
            c += 1;
        }
        assert_eq!(DuplexMode::from_code(4).err(), Some(PtDuplexError::Unknown));
    }
}
