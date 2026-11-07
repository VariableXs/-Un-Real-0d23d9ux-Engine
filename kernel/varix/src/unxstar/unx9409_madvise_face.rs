//! UNX-F9409 madvise 错误面 · 0x7D3C0段
/*!
 * J1 判据原文摘录（验收卡片）：
 * - 注入必抓：全部注入场景逐条拦截，错误码精确匹配，一条不漏；
 * - 判序与 Linux 一致：addr 页对齐 → 区间已映射 → advice 闭集 →
 *   len==0 合法 no-op → MADV_FREE 需匿名映射，判序不得颠倒；
 * - 双跑一致：同一操作序列在两份全新状态机上重放，FNV-1a 64 位
 *   指纹逐位相同；
 * - 段位守卫：诊断段 0x7D3C0 为本单元独占，错误码 = 段基 + 序号，
 *   闭集内单调不重叠；
 * - 闭集诊断码：错误码与 advice 取值均为闭集，闭集外一律拒绝；
 * - 完成时 API：run_unx9409_checks() 返回 CheckSet，供聚合层消费；
 * - 软件写回：状态=已完成、结果摘要非空（status_summary 不留白）。
 *
 * 设计要点（7 条）：
 *  1. no_std + alloc 纯功能线：禁用原语族（不安全块/强解/急停）
 *     全避，整数确定性算术，无浮点、无墙钟、无随机源。
 *  2. advice 闭集恰 10 项（MADV_NORMAL=0 .. MADV_SOFT_OFFLINE=101），
 *     原始 i32 经 from_raw 单点判别，闭集外（如 7、-5）直接拒。
 *  3. 判序冻结：对齐 → 映射 → 闭集 → 零长 no-op → FREE 匿名，
 *     前面一关不过即返回，绝不"顺手"做后面的检查（零误伤前提）。
 *  4. 状态机单调迁移：Mapping → Advising → Sealed，只进不退；
 *     封印后一切 advise 拒绝；Mapping 阶段 advise 拒绝（EPhase）。
 *  5. 定容账本：advice 账 cap=64，满后拒绝新账（ELedgerFull），
 *     拒绝显性记账不静默吞。
 *  6. FREE 语义：仅匿名页可 FREE；FREE 后同页再 FREE → EFreeTwice；
 *     全区间先审后落账，避免半提交。
 *  7. 指纹对拍：状态序列化后 FNV-1a 64 位压缩，双跑一致判据的
 *     唯一裁决依据；RECON_TABLE 10 行与闭集逐行对账。
 */
#![allow(dead_code)]
use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

// ── 段位守卫 ─────────────────────────────────────────────────
/// 诊断段基址：0x7D3C0，UNX-F9409 独占。
pub const SEG_BASE: u32 = 0x7D3C0;

// ── 页粒度常量 ───────────────────────────────────────────────
/// 页大小 4096（Linux madvise addr 必须页对齐）。
pub const PAGE_SIZE: u64 = 4096;
/// 页号窗口上限：1 Mi 页 = 4 GiB 地址空间账面。
pub const PAGE_WINDOW: u64 = 1_048_576;
/// advice 账本容量（cap 64）。
pub const LEDGER_CAP: usize = 64;

// ── 错误码闭集（10~12 变体）─────────────────────────────────
/// madvise 错误面闭集：val() = SEG_BASE + 序号，单调不重叠。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadvErr {
    /// addr 未按页对齐（判序第 1 关）
    EUnaligned,
    /// 区间内存在未映射页（判序第 2 关）
    ENomap,
    /// advice 不在 10 项闭集内（判序第 3 关）
    EBadAdvice,
    /// MADV_FREE 作用于文件映射页（判序第 5 关）
    ENotAnon,
    /// 同页重复 MADV_FREE
    EFreeTwice,
    /// 封印后仍尝试 advise
    ESealed,
    /// Mapping 阶段尚未冻结即 advise
    EPhase,
    /// advice 账本已满（cap 64）
    ELedgerFull,
    /// addr+len 算术回绕
    ERangeOvfl,
    /// 区间越过页号窗口上限
    EWindow,
    /// 状态机不变量破坏（对账抓包）
    EInvState,
    /// 原始诊断码不在本段闭集（兜底）
    EUnknown,
}

impl MadvErr {
    /// 诊断码：段基 + 序号（闭集内单调）。
    pub fn val(&self) -> u32 {
        SEG_BASE + (*self as u32)
    }

    /// 中文明细，逐变体互异、非空。
    pub fn detail(&self) -> &'static str {
        match self {
            MadvErr::EUnaligned => "addr 未按 4096 页对齐，判序第一关即拒",
            MadvErr::ENomap => "区间 [addr, addr+len) 存在未映射页",
            MadvErr::EBadAdvice => "advice 不在 10 项闭集内，闭集外一律拒绝",
            MadvErr::ENotAnon => "MADV_FREE 仅允许匿名映射，文件映射页拒绝",
            MadvErr::EFreeTwice => "同页重复 MADV_FREE，释放态不可二释放",
            MadvErr::ESealed => "状态机已封印，一切 advise 拒绝",
            MadvErr::EPhase => "映射尚未冻结（Mapping 阶段），advise 越相",
            MadvErr::ELedgerFull => "advice 账本已满（cap 64），新账拒绝",
            MadvErr::ERangeOvfl => "addr+len 无符号回绕，区间非法",
            MadvErr::EWindow => "页号越过 1Mi 页窗口上限",
            MadvErr::EInvState => "账本不变量对账不符，状态机告警",
            MadvErr::EUnknown => "原始诊断码不在 0x7D3C0 段闭集",
        }
    }

    /// 闭集全体（与 RECON_TABLE 对账用）。
    pub const ALL: [MadvErr; 12] = [
        MadvErr::EUnaligned,
        MadvErr::ENomap,
        MadvErr::EBadAdvice,
        MadvErr::ENotAnon,
        MadvErr::EFreeTwice,
        MadvErr::ESealed,
        MadvErr::EPhase,
        MadvErr::ELedgerFull,
        MadvErr::ERangeOvfl,
        MadvErr::EWindow,
        MadvErr::EInvState,
        MadvErr::EUnknown,
    ];

    /// 由原始诊断码解码；越界归 EUnknown（兜底不静默）。
    pub fn from_code(code: u32) -> MadvErr {
        match code.checked_sub(SEG_BASE) {
            Some(off) => match MadvErr::ALL.get(off as usize) {
                Some(e) => *e,
                None => MadvErr::EUnknown,
            },
            None => MadvErr::EUnknown,
        }
    }
}

// ── advice 闭集（恰 10 项）───────────────────────────────────
/// madvise advice 闭集，原始取值与 Linux 对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadvAdvice {
    /// MADV_NORMAL = 0
    Normal,
    /// MADV_SEQUENTIAL = 1
    Sequential,
    /// MADV_RANDOM = 2
    Random,
    /// MADV_WILLNEED = 3
    Willneed,
    /// MADV_DONTNEED = 4
    Dontneed,
    /// MADV_FREE = 8（仅匿名映射）
    Free,
    /// MADV_DONTFORK = 10
    Dontfork,
    /// MADV_DOFORK = 11
    Dofork,
    /// MADV_HWPOISON = 100
    Hwpoison,
    /// MADV_SOFT_OFFLINE = 101
    SoftOffline,
}

impl MadvAdvice {
    /// 原始 i32 → 闭集单点判别；闭集外（7、-5、1024…）返回 None。
    pub fn from_raw(raw: i32) -> Option<MadvAdvice> {
        match raw {
            0 => Some(MadvAdvice::Normal),
            1 => Some(MadvAdvice::Sequential),
            2 => Some(MadvAdvice::Random),
            3 => Some(MadvAdvice::Willneed),
            4 => Some(MadvAdvice::Dontneed),
            8 => Some(MadvAdvice::Free),
            10 => Some(MadvAdvice::Dontfork),
            11 => Some(MadvAdvice::Dofork),
            100 => Some(MadvAdvice::Hwpoison),
            101 => Some(MadvAdvice::SoftOffline),
            _ => None,
        }
    }

    /// 原始取值回读。
    pub fn raw(&self) -> i32 {
        match self {
            MadvAdvice::Normal => 0,
            MadvAdvice::Sequential => 1,
            MadvAdvice::Random => 2,
            MadvAdvice::Willneed => 3,
            MadvAdvice::Dontneed => 4,
            MadvAdvice::Free => 8,
            MadvAdvice::Dontfork => 10,
            MadvAdvice::Dofork => 11,
            MadvAdvice::Hwpoison => 100,
            MadvAdvice::SoftOffline => 101,
        }
    }

    /// 中文名（闭集诊断文本对账用）。
    pub fn name(&self) -> &'static str {
        match self {
            MadvAdvice::Normal => "MADV_NORMAL",
            MadvAdvice::Sequential => "MADV_SEQUENTIAL",
            MadvAdvice::Random => "MADV_RANDOM",
            MadvAdvice::Willneed => "MADV_WILLNEED",
            MadvAdvice::Dontneed => "MADV_DONTNEED",
            MadvAdvice::Free => "MADV_FREE",
            MadvAdvice::Dontfork => "MADV_DONTFORK",
            MadvAdvice::Dofork => "MADV_DOFORK",
            MadvAdvice::Hwpoison => "MADV_HWPOISON",
            MadvAdvice::SoftOffline => "MADV_SOFT_OFFLINE",
        }
    }

    /// 闭集全体，序与 RECON_TABLE 一致。
    pub const ALL: [MadvAdvice; 10] = [
        MadvAdvice::Normal,
        MadvAdvice::Sequential,
        MadvAdvice::Random,
        MadvAdvice::Willneed,
        MadvAdvice::Dontneed,
        MadvAdvice::Free,
        MadvAdvice::Dontfork,
        MadvAdvice::Dofork,
        MadvAdvice::Hwpoison,
        MadvAdvice::SoftOffline,
    ];
}

// ── 对账表（名称, 原始值, 闭集序号）─────────────────────────
/// RECON_TABLE：10 行逐行与闭集对账。
pub const RECON_TABLE: [(&str, i32, i32); 10] = [
    ("MADV_NORMAL", 0, 0),
    ("MADV_SEQUENTIAL", 1, 1),
    ("MADV_RANDOM", 2, 2),
    ("MADV_WILLNEED", 3, 3),
    ("MADV_DONTNEED", 4, 4),
    ("MADV_FREE", 8, 5),
    ("MADV_DONTFORK", 10, 6),
    ("MADV_DOFORK", 11, 7),
    ("MADV_HWPOISON", 100, 8),
    ("MADV_SOFT_OFFLINE", 101, 9),
];

// ── FNV-1a 64 位指纹 ────────────────────────────────────────
/// FNV-1a 64 位（双跑一致判据的唯一裁决函数）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        match data.get(i) {
            Some(b) => {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            None => {}
        }
        i += 1;
    }
    h
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    let bytes = v.to_le_bytes();
    buf.extend_from_slice(&bytes);
}

/// 域标签（软件写回摘要头，供聚合层拼装）。
pub fn domain_tag() -> String {
    "F9409 madvise 错误面".to_string()
}

// ── 状态机相（单调迁移）─────────────────────────────────────
/// 相：Mapping → Advising → Sealed，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 建账相：只许 map_page
    Mapping,
    /// 冻结相：map_page 拒、advise 可
    Advising,
    /// 封印相：一切写拒
    Sealed,
}

// ── advice 账目 ──────────────────────────────────────────────
/// 一条 advice 落账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdviceRec {
    /// 起始页号
    pub start_page: u64,
    /// 覆盖页数（≥1）
    pub len_pages: u64,
    /// advice 闭集成员
    pub advice: MadvAdvice,
    /// 单调流水号
    pub seq: u64,
}

// ── 域模型：页映射账 ────────────────────────────────────────
/// 页映射账：mapped[(page_no, anon)] + 释放集 + advice 账本。
pub struct PageMap {
    /// 已映射页：(页号, 是否匿名)
    mapped: Vec<(u64, bool)>,
    /// 已 FREE 页号集（匿名页专有）
    freed: Vec<u64>,
    /// advice 账本（定容 LEDGER_CAP）
    ledger: Vec<AdviceRec>,
    /// 相（单调）
    phase: Phase,
    /// 单调流水号
    seq: u64,
    /// 拒绝计数（按错误码闭集）
    rejects: Vec<(MadvErr, u32)>,
}

impl PageMap {
    /// 新建空账（Mapping 相）。
    pub fn new() -> PageMap {
        PageMap {
            mapped: Vec::new(),
            freed: Vec::new(),
            ledger: Vec::new(),
            phase: Phase::Mapping,
            seq: 0,
            rejects: Vec::new(),
        }
    }

    fn note_reject(&mut self, e: MadvErr) {
        let mut done = false;
        for pair in self.rejects.iter_mut() {
            if pair.0 == e {
                pair.1 = pair.1.wrapping_add(1);
                done = true;
            }
        }
        if !done {
            self.rejects.push((e, 1));
        }
    }

    /// 登记一页映射（仅 Mapping 相；重复登记幂等）。
    pub fn map_page(&mut self, page_no: u64, anon: bool) -> Result<(), MadvErr> {
        match self.phase {
            Phase::Mapping => {}
            _ => {
                self.note_reject(MadvErr::EPhase);
                return Err(MadvErr::EPhase);
            }
        }
        if page_no >= PAGE_WINDOW {
            self.note_reject(MadvErr::EWindow);
            return Err(MadvErr::EWindow);
        }
        let mut exists = false;
        for pair in self.mapped.iter() {
            if pair.0 == page_no {
                exists = true;
            }
        }
        if exists {
            // 幂等重映射：不重复落账、不改匿名属性
            return Ok(());
        }
        self.mapped.push((page_no, anon));
        Ok(())
    }

    /// 冻结映射：Mapping → Advising（幂等）。
    pub fn freeze_map(&mut self) -> Result<(), MadvErr> {
        match self.phase {
            Phase::Mapping => {
                self.phase = Phase::Advising;
                Ok(())
            }
            Phase::Advising => Ok(()),
            Phase::Sealed => Err(MadvErr::ESealed),
        }
    }

    /// 封印：任何非封印相 → Sealed；二次封印拒绝（显性）。
    pub fn seal(&mut self) -> Result<(), MadvErr> {
        match self.phase {
            Phase::Sealed => Err(MadvErr::ESealed),
            _ => {
                self.phase = Phase::Sealed;
                Ok(())
            }
        }
    }

    /// 页是否已映射。
    pub fn is_mapped(&self, page_no: u64) -> bool {
        for pair in self.mapped.iter() {
            if pair.0 == page_no {
                return true;
            }
        }
        false
    }

    /// 页匿名属性：None=未映射。
    pub fn anon_of(&self, page_no: u64) -> Option<bool> {
        for pair in self.mapped.iter() {
            if pair.0 == page_no {
                return Some(pair.1);
            }
        }
        None
    }

    /// 页是否已 FREE。
    pub fn is_freed(&self, page_no: u64) -> bool {
        for f in self.freed.iter() {
            if *f == page_no {
                return true;
            }
        }
        false
    }

    /// 当前相。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 账本长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 第 idx 条账目（.get 防越界）。
    pub fn ledger_at(&self, idx: usize) -> Option<AdviceRec> {
        match self.ledger.get(idx) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// madvise 主判序（与 Linux 一致，逐关冻结）：
    /// 相守卫 → 页对齐 → 区间映射 → advice 闭集 → len==0 no-op →
    /// FREE 匿名/重复 → 账本容量 → 落账。
    pub fn advise(&mut self, addr: u64, len: u64, raw_advice: i32) -> Result<u64, MadvErr> {
        // 第 0 关：状态机相守卫（先于一切 Linux 判序）
        match self.phase {
            Phase::Sealed => {
                self.note_reject(MadvErr::ESealed);
                return Err(MadvErr::ESealed);
            }
            Phase::Mapping => {
                self.note_reject(MadvErr::EPhase);
                return Err(MadvErr::EPhase);
            }
            Phase::Advising => {}
        }
        // 第 1 关：addr 页对齐
        if addr % PAGE_SIZE != 0 {
            self.note_reject(MadvErr::EUnaligned);
            return Err(MadvErr::EUnaligned);
        }
        // 第 2 关前置：addr+len 回绕检查（含尾页向上取整）
        let end_addr = match addr.checked_add(len) {
            Some(v) => v,
            None => {
                self.note_reject(MadvErr::ERangeOvfl);
                return Err(MadvErr::ERangeOvfl);
            }
        };
        let end_page = match end_addr.checked_add(PAGE_SIZE - 1) {
            Some(v) => v / PAGE_SIZE,
            None => {
                self.note_reject(MadvErr::ERangeOvfl);
                return Err(MadvErr::ERangeOvfl);
            }
        };
        let start_page = addr / PAGE_SIZE;
        // 第 2 关前置：页号窗口
        if end_page > PAGE_WINDOW {
            self.note_reject(MadvErr::EWindow);
            return Err(MadvErr::EWindow);
        }
        // 第 2 关：区间 [addr, addr+len) 全部已映射（len==0 空转天然过）
        let mut p = start_page;
        while p < end_page {
            if !self.is_mapped(p) {
                self.note_reject(MadvErr::ENomap);
                return Err(MadvErr::ENomap);
            }
            p += 1;
        }
        // 第 3 关：advice 闭集
        let advice = match MadvAdvice::from_raw(raw_advice) {
            Some(a) => a,
            None => {
                self.note_reject(MadvErr::EBadAdvice);
                return Err(MadvErr::EBadAdvice);
            }
        };
        // 第 4 关：len==0 合法 no-op（零误伤：不落账、不报错）
        if len == 0 {
            return Ok(0);
        }
        // 第 5 关：MADV_FREE 语义（先全区间审后落账，避免半提交）
        if advice == MadvAdvice::Free {
            let mut p = start_page;
            while p < end_page {
                match self.anon_of(p) {
                    Some(true) => {}
                    Some(false) => {
                        self.note_reject(MadvErr::ENotAnon);
                        return Err(MadvErr::ENotAnon);
                    }
                    None => {
                        self.note_reject(MadvErr::ENomap);
                        return Err(MadvErr::ENomap);
                    }
                }
                if self.is_freed(p) {
                    self.note_reject(MadvErr::EFreeTwice);
                    return Err(MadvErr::EFreeTwice);
                }
                p += 1;
            }
            let mut p = start_page;
            while p < end_page {
                self.freed.push(p);
                p += 1;
            }
        }
        // 第 6 关：账本容量守卫
        if self.ledger.len() >= LEDGER_CAP {
            self.note_reject(MadvErr::ELedgerFull);
            return Err(MadvErr::ELedgerFull);
        }
        // 落账
        self.seq = self.seq.wrapping_add(1);
        let pages = end_page - start_page;
        self.ledger.push(AdviceRec {
            start_page,
            len_pages: pages,
            advice,
            seq: self.seq,
        });
        Ok(pages)
    }

    /// 不变量对账：账单调、区间在映射集内、释放页必匿名已映射、
    /// 容量守恒、映射页号在窗口内且无重复。
    pub fn invariant_ok(&self) -> bool {
        // 相合法
        let phase_ok = match self.phase {
            Phase::Mapping | Phase::Advising | Phase::Sealed => true,
        };
        // 账本容量
        if self.ledger.len() > LEDGER_CAP {
            return false;
        }
        // 流水号严格递增
        let mut prev: u64 = 0;
        let mut first = true;
        for r in self.ledger.iter() {
            if first {
                first = false;
            } else if r.seq <= prev {
                return false;
            }
            prev = r.seq;
        }
        // 账目区间在映射集内、页号在窗口内、页数非零
        for r in self.ledger.iter() {
            if r.len_pages == 0 {
                return false;
            }
            if r.start_page >= PAGE_WINDOW {
                return false;
            }
            let end = match r.start_page.checked_add(r.len_pages) {
                Some(v) => v,
                None => return false,
            };
            if end > PAGE_WINDOW {
                return false;
            }
            let mut p = r.start_page;
            while p < end {
                if !self.is_mapped(p) {
                    return false;
                }
                p += 1;
            }
        }
        // 释放页必已映射且匿名
        for f in self.freed.iter() {
            match self.anon_of(*f) {
                Some(true) => {}
                _ => return false,
            }
        }
        // 映射页无重复、页号在窗口内
        let n = self.mapped.len();
        let mut i = 0usize;
        while i < n {
            let a = match self.mapped.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.0 >= PAGE_WINDOW {
                return false;
            }
            let mut j = i + 1;
            while j < n {
                let b = match self.mapped.get(j) {
                    Some(v) => *v,
                    None => return false,
                };
                if a.0 == b.0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        phase_ok
    }

    /// 状态序列化后 FNV-1a 64 位指纹（双跑一致）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.phase {
            Phase::Mapping => 1u8,
            Phase::Advising => 2u8,
            Phase::Sealed => 3u8,
        });
        push_u64(&mut buf, self.seq);
        push_u64(&mut buf, self.mapped.len() as u64);
        for pair in self.mapped.iter() {
            push_u64(&mut buf, pair.0);
            buf.push(if pair.1 { 1u8 } else { 0u8 });
        }
        push_u64(&mut buf, self.freed.len() as u64);
        for f in self.freed.iter() {
            push_u64(&mut buf, *f);
        }
        push_u64(&mut buf, self.ledger.len() as u64);
        for r in self.ledger.iter() {
            push_u64(&mut buf, r.start_page);
            push_u64(&mut buf, r.len_pages);
            push_u64(&mut buf, r.seq);
            buf.push(r.advice.raw() as u8);
        }
        fnv1a64(&buf)
    }

    /// 软件写回：状态=已完成、结果摘要非空。
    pub fn status_summary(&self) -> String {
        format!(
            "{} 状态=已完成 段=0x{:X} 相={:?} 映射页={} 释放页={} 账目={} 指纹={:#018X}",
            domain_tag(),
            SEG_BASE,
            self.phase,
            self.mapped.len(),
            self.freed.len(),
            self.ledger.len(),
            self.fingerprint()
        )
    }
}

// ── 注入器（注入必抓）───────────────────────────────────────
/// 注入器：逐场景注入违规，错误码精确匹配才计抓。
pub struct Injector {
    caught: usize,
    total: usize,
    log: Vec<String>,
}

impl Injector {
    pub fn new() -> Injector {
        Injector {
            caught: 0,
            total: 0,
            log: Vec::new(),
        }
    }

    /// 记一例：got 与 expect 精确相等才算抓到。
    pub fn case(&mut self, name: &str, expect: MadvErr, got: Result<u64, MadvErr>) {
        self.total += 1;
        match got {
            Ok(_) => {
                let line = format!("漏抓[{}] 期望 {} 实得 Ok", name, expect.detail());
                self.log.push(line);
            }
            Err(e) => {
                if e == expect {
                    self.caught += 1;
                    let line = format!("抓到[{}] 码={:#X}", name, e.val());
                    self.log.push(line);
                } else {
                    let line = format!("错抓[{}] 期望 {:#X} 实得 {:#X}", name, expect.val(), e.val());
                    self.log.push(line);
                }
            }
        }
    }

    /// 全量违规注入：返回 (抓到数, 总数)。
    pub fn run_all() -> (usize, usize) {
        let mut inj = Injector::new();

        // 场景 1：addr 未页对齐（advice 合法、页已映射——对齐第一关）
        let mut pm = fresh_even_map(4);
        let _ = pm.freeze_map();
        inj.case("未对齐addr", MadvErr::EUnaligned, pm.advise(4097, 4096, 0));

        // 场景 2：区间映射洞（advice 合法——映射关先于闭集关）
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.map_page(1, true);
        let _ = pm.map_page(3, true); // 页 2 留洞
        let _ = pm.freeze_map();
        inj.case("映射洞", MadvErr::ENomap, pm.advise(0, 4 * PAGE_SIZE, 0));

        // 场景 3：advice=7（闭集洞）
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        inj.case("advice=7", MadvErr::EBadAdvice, pm.advise(0, PAGE_SIZE, 7));

        // 场景 4：advice=-5（负闭集外）
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        inj.case("advice=-5", MadvErr::EBadAdvice, pm.advise(0, PAGE_SIZE, -5));

        // 场景 5：FREE 作用于文件映射页
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.map_page(1, false); // 文件映射
        let _ = pm.freeze_map();
        inj.case(
            "FREE文件页",
            MadvErr::ENotAnon,
            pm.advise(PAGE_SIZE, PAGE_SIZE, 8),
        );

        // 场景 6：同页 FREE 两次
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.freeze_map();
        let _ = pm.advise(0, PAGE_SIZE, 8); // 首次合法
        inj.case("FREE二次", MadvErr::EFreeTwice, pm.advise(0, PAGE_SIZE, 8));

        // 场景 7：封印后 advise
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        let _ = pm.seal();
        inj.case("封印后advise", MadvErr::ESealed, pm.advise(0, PAGE_SIZE, 0));

        // 场景 8：Mapping 相未冻结即 advise
        let mut pm = fresh_even_map(2);
        inj.case("未冻结advise", MadvErr::EPhase, pm.advise(0, PAGE_SIZE, 0));

        // 场景 9：账本满（cap 64，第 65 笔拒）
        let mut pm = PageMap::new();
        let mut i = 0u64;
        while i < 64 {
            let _ = pm.map_page(i, true);
            i += 1;
        }
        let _ = pm.freeze_map();
        let mut i = 0u64;
        while i < 64 {
            let _ = pm.advise(i * PAGE_SIZE, PAGE_SIZE, 0);
            i += 1;
        }
        inj.case("账本满", MadvErr::ELedgerFull, pm.advise(0, PAGE_SIZE, 1));

        // 场景 10：addr+len 回绕（对齐的超大地址）
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        let big = 0xFFFF_FFFF_FFFF_F000u64;
        inj.case("区间回绕", MadvErr::ERangeOvfl, pm.advise(big, 8192, 0));

        // 场景 11：页号越窗（对齐、闭集内 advice）
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        let beyond = PAGE_WINDOW * PAGE_SIZE;
        inj.case("页号越窗", MadvErr::EWindow, pm.advise(beyond, PAGE_SIZE, 0));

        (inj.caught, inj.total)
    }

    /// 零误伤阳性集：合法操作一个都不许拒。返回 (通过数, 总数)。
    pub fn run_positives() -> (usize, usize) {
        let mut pass = 0usize;
        let total = 4usize;
        // 阳 1：len==0 no-op（对齐、闭集内 advice）
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        if let Ok(0) = pm.advise(0, 0, 0) {
            pass += 1;
        }
        // 阳 2：DONTNEED 作用于匿名页
        if let Ok(1) = pm.advise(0, PAGE_SIZE, 4) {
            pass += 1;
        }
        // 阳 3：FREE 首次作用于匿名页
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.freeze_map();
        if let Ok(1) = pm.advise(0, PAGE_SIZE, 8) {
            pass += 1;
        }
        // 阳 4：WILLNEED 跨 3 页合法区间
        let mut pm = fresh_even_map(4);
        let _ = pm.freeze_map();
        if let Ok(3) = pm.advise(0, 3 * PAGE_SIZE, 3) {
            pass += 1;
        }
        (pass, total)
    }

    /// 注入日志（逐行中文，供聚合摘要）。
    pub fn log(&self) -> Vec<String> {
        self.log.clone()
    }
}

/// 造一台 [0, n) 全匿名的映射账并停在 Mapping 相。
fn fresh_even_map(n: u64) -> PageMap {
    let mut pm = PageMap::new();
    let mut i = 0u64;
    while i < n {
        let _ = pm.map_page(i, true);
        i += 1;
    }
    pm
}

// ── 完成时 API：域自检 ──────────────────────────────────────
/// UNX-F9409 域自检（J1 全判据落点）。
pub fn run_unx9409_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9409");

    // J1-01 注入必抓
    let (c, t) = Injector::run_all();
    s.add("J1-01 注入必抓", c == t && t >= 10, "注入场景逐条拦截且错误码精确匹配，总数≥10");

    // J1-02 零误伤
    let (pc, pt) = Injector::run_positives();
    s.add("J1-02 零误伤阳性", pc == pt, "len0/DONTNEED匿名/FREE首次/WILLNEED 跨页全过");

    // J1-03 判序：对齐关先于闭集关
    let mut pm = fresh_even_map(2);
    let _ = pm.freeze_map();
    let order1 = match pm.advise(4097, PAGE_SIZE, 7) {
        Err(MadvErr::EUnaligned) => true,
        _ => false,
    };
    s.add("J1-03 判序对齐先行", order1, "未对齐+坏advice → EUnaligned，判序与 Linux 一致");

    // J1-04 判序：映射关先于闭集关
    let mut pm = PageMap::new();
    let _ = pm.map_page(0, true);
    let _ = pm.freeze_map();
    let order2 = match pm.advise(PAGE_SIZE, PAGE_SIZE, 7) {
        Err(MadvErr::ENomap) => true,
        _ => false,
    };
    s.add("J1-04 判序映射先行", order2, "未映射+坏advice → ENomap，映射关先于闭集关");

    // J1-05 判序：闭集关先于 FREE 匿名关
    let mut pm = PageMap::new();
    let _ = pm.map_page(0, false);
    let _ = pm.freeze_map();
    let order3 = match pm.advise(0, PAGE_SIZE, 7) {
        Err(MadvErr::EBadAdvice) => true,
        _ => false,
    };
    s.add("J1-05 判序闭集先行", order3, "文件页+坏advice → EBadAdvice，非 ENotAnon");

    // J1-06 advice 闭集恰 10 项且 roundtrip
    let mut set_ok = MadvAdvice::ALL.len() == 10;
    for a in MadvAdvice::ALL.iter() {
        match MadvAdvice::from_raw(a.raw()) {
            Some(b) => {
                if *a != b {
                    set_ok = false;
                }
            }
            None => set_ok = false,
        }
    }
    s.add("J1-06 advice闭集10项", set_ok, "10 项原始值逐项 roundtrip 恒等");

    // J1-07 advice 闭集外拒绝（7/-5/1024/负大）
    let rej = MadvAdvice::from_raw(7).is_none()
        && MadvAdvice::from_raw(-5).is_none()
        && MadvAdvice::from_raw(1024).is_none()
        && MadvAdvice::from_raw(-2_147_483_648).is_none();
    s.add("J1-07 闭集外拒绝", rej, "7/-5/1024/i32::MIN 全部 None");

    // J1-08 FREE 文件页拒 + 同页二次 FREE 拒
    let mut pm = PageMap::new();
    let _ = pm.map_page(0, false);
    let _ = pm.map_page(1, true);
    let _ = pm.freeze_map();
    let f1 = match pm.advise(0, PAGE_SIZE, 8) {
        Err(MadvErr::ENotAnon) => true,
        _ => false,
    };
    let f2 = match pm.advise(PAGE_SIZE, PAGE_SIZE, 8) {
        Ok(1) => true,
        _ => false,
    };
    let f3 = match pm.advise(PAGE_SIZE, PAGE_SIZE, 8) {
        Err(MadvErr::EFreeTwice) => true,
        _ => false,
    };
    s.add("J1-08 FREE语义双拒", f1 && f2 && f3, "文件页 FREE 拒；匿名首 FREE 过；同页二 FREE 拒");

    // J1-09 相守卫：未冻结拒 + 封印后拒 + 二次封印拒
    let mut pm = fresh_even_map(1);
    let g1 = match pm.advise(0, PAGE_SIZE, 0) {
        Err(MadvErr::EPhase) => true,
        _ => false,
    };
    let _ = pm.freeze_map();
    let _ = pm.seal();
    let g2 = match pm.advise(0, PAGE_SIZE, 0) {
        Err(MadvErr::ESealed) => true,
        _ => false,
    };
    let g3 = match pm.seal() {
        Err(MadvErr::ESealed) => true,
        _ => false,
    };
    s.add("J1-09 相守卫三连", g1 && g2 && g3, "未冻结EPhase；封印后ESealed；二次封印ESealed");

    // J1-10 账本容量守卫（cap 64）
    let mut pm = PageMap::new();
    let mut i = 0u64;
    while i < 64 {
        let _ = pm.map_page(i, true);
        i += 1;
    }
    let _ = pm.freeze_map();
    let mut i = 0u64;
    while i < 64 {
        let _ = pm.advise(i * PAGE_SIZE, PAGE_SIZE, 0);
        i += 1;
    }
    let cap_ok = pm.ledger_len() == LEDGER_CAP
        && match pm.advise(0, PAGE_SIZE, 2) {
            Err(MadvErr::ELedgerFull) => true,
            _ => false,
        };
    s.add("J1-10 账本容量64", cap_ok, "cap=64 满 后第 65 笔 ELedgerFull");

    // J1-11 不变量对账
    let mut pm = fresh_even_map(8);
    let _ = pm.freeze_map();
    let _ = pm.advise(0, 2 * PAGE_SIZE, 4);
    let _ = pm.advise(PAGE_SIZE, PAGE_SIZE, 8);
    let _ = pm.advise(4097, PAGE_SIZE, 0); // 注入一次拒绝
    s.add("J1-11 不变量对账", pm.invariant_ok(), "混合操作后账单调/区间在映射集/释放页匿名");

    // J1-12 RECON_TABLE 10 行对账
    let mut recon_ok = RECON_TABLE.len() == 10;
    for row in RECON_TABLE.iter() {
        let (name, raw, idx) = *row;
        match MadvAdvice::from_raw(raw) {
            Some(a) => {
                if a.name() != name {
                    recon_ok = false;
                }
                match MadvAdvice::ALL.get(idx as usize) {
                    Some(b) => {
                        if *b != a {
                            recon_ok = false;
                        }
                    }
                    None => recon_ok = false,
                }
            }
            None => recon_ok = false,
        }
    }
    s.add("J1-12 RECON对账", recon_ok, "10 行名称/原始值/闭集序号三方一致");

    // J1-13 段位守卫
    let mut seg_ok = SEG_BASE == 0x7D3C0;
    let mut prev = 0u32;
    let mut first = true;
    for e in MadvErr::ALL.iter() {
        let v = e.val();
        if v < SEG_BASE || v >= SEG_BASE + 12 {
            seg_ok = false;
        }
        if !first && v <= prev {
            seg_ok = false;
        }
        first = false;
        prev = v;
    }
    s.add("J1-13 段位守卫", seg_ok, "0x7D3C0 独占；12 诊断码段内单调不重叠");

    // J1-14 软件写回摘要非空
    let mut pm = fresh_even_map(4);
    let _ = pm.freeze_map();
    let _ = pm.advise(0, PAGE_SIZE, 0);
    let summary = pm.status_summary();
    s.add("J1-14 摘要非空", summary.len() > 0 && summary.contains("已完成"), "状态=已完成、结果摘要非空");

    // 动态明细：双跑指纹一致
    let fp1 = replay_fingerprint();
    let fp2 = replay_fingerprint();
    s.add_dyn("J1-15 双跑指纹", fp1 == fp2, "同序列两台新账指纹逐位相等");

    // 动态明细：注入计数
    let (c2, t2) = Injector::run_all();
    s.add_dyn("J1-16 注入计数", c2 == t2 && t2 >= 10, "抓到=总数且≥10");

    // 动态明细：诊断码闭集 detail 互异
    let mut det_ok = true;
    let n = MadvErr::ALL.len();
    let mut i = 0usize;
    while i < n {
        let mut j = i + 1;
        while j < n {
            let a = match MadvErr::ALL.get(i) {
                Some(v) => v.detail(),
                None => "",
            };
            let b = match MadvErr::ALL.get(j) {
                Some(v) => v.detail(),
                None => "",
            };
            if a == b || a.len() == 0 {
                det_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add_dyn("J1-17 诊断明细互异", det_ok, "12 变体中文明细非空互异");

    // 动态明细：阳性计数
    let (pc2, pt2) = Injector::run_positives();
    s.add_dyn("J1-18 阳性计数", pc2 == pt2, "阳性全过不误伤");

    s
}

/// 固定操作序列的重放指纹（双跑一致判据载体）。
fn replay_fingerprint() -> u64 {
    let mut pm = PageMap::new();
    let mut i = 0u64;
    while i < 6 {
        let _ = pm.map_page(i, i % 2 == 0);
        i += 1;
    }
    let _ = pm.freeze_map();
    let _ = pm.advise(0, 2 * PAGE_SIZE, 1);
    let _ = pm.advise(0, PAGE_SIZE, 4);
    let _ = pm.advise(0, PAGE_SIZE, 8);
    let _ = pm.advise(4097, PAGE_SIZE, 0);
    let _ = pm.advise(0, 0, 0);
    pm.fingerprint()
}

// ── 测试（≥8 条；断言族仅在此允许）─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 用 match 断错（无 unwrap/expect；泛型兼容各 Ok 载体）。
    fn expect_err<T>(got: Result<T, MadvErr>, want: MadvErr) -> bool {
        match got {
            Err(e) => e == want,
            Ok(_) => false,
        }
    }

    #[test]
    fn seg_base_and_err_vals() {
        assert_eq!(SEG_BASE, 0x7D3C0);
        let mut prev = 0u32;
        let mut first = true;
        for e in MadvErr::ALL.iter() {
            let v = e.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 12);
            if !first {
                assert!(v > prev);
            }
            first = false;
            prev = v;
        }
        assert_eq!(MadvErr::ALL.len(), 12);
        // 解码往返
        for e in MadvErr::ALL.iter() {
            assert_eq!(MadvErr::from_code(e.val()), *e);
        }
        assert_eq!(MadvErr::from_code(SEG_BASE + 99), MadvErr::EUnknown);
        assert_eq!(MadvErr::from_code(0x123), MadvErr::EUnknown);
    }

    #[test]
    fn advice_closed_set() {
        assert_eq!(MadvAdvice::ALL.len(), 10);
        for a in MadvAdvice::ALL.iter() {
            match MadvAdvice::from_raw(a.raw()) {
                Some(b) => assert_eq!(*a, b),
                None => assert!(false, "闭集成员解析失败"),
            }
        }
        assert!(MadvAdvice::from_raw(7).is_none());
        assert!(MadvAdvice::from_raw(-5).is_none());
        assert!(MadvAdvice::from_raw(1024).is_none());
        assert!(MadvAdvice::from_raw(5).is_none());
        assert!(MadvAdvice::from_raw(6).is_none());
        assert!(MadvAdvice::from_raw(9).is_none());
    }

    #[test]
    fn judge_order_frozen() {
        // 对齐关第一
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        assert!(expect_err(pm.advise(4097, PAGE_SIZE, 7), MadvErr::EUnaligned));
        // 映射关先于闭集关
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.freeze_map();
        assert!(expect_err(pm.advise(PAGE_SIZE, PAGE_SIZE, 7), MadvErr::ENomap));
        // 闭集关先于 FREE 匿名关
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, false);
        let _ = pm.freeze_map();
        assert!(expect_err(pm.advise(0, PAGE_SIZE, 7), MadvErr::EBadAdvice));
        // len==0 在闭集关之后：len0 + 坏 advice → EBadAdvice
        assert!(expect_err(pm.advise(0, 0, 7), MadvErr::EBadAdvice));
    }

    #[test]
    fn closed_set_injection_all_caught() {
        let (c, t) = Injector::run_all();
        assert!(t >= 10, "注入场景不足 10");
        assert_eq!(c, t, "有漏抓或错抓");
    }

    #[test]
    fn zero_false_kill_positives() {
        let (p, t) = Injector::run_positives();
        assert_eq!(p, t, "阳性被误伤");
        assert_eq!(t, 4);
        // len0 显式 no-op
        let mut pm = fresh_even_map(2);
        let _ = pm.freeze_map();
        match pm.advise(0, 0, 0) {
            Ok(0) => {}
            _ => assert!(false, "len0 no-op 应过"),
        }
        // len0 不落账
        assert_eq!(pm.ledger_len(), 0);
    }

    #[test]
    fn seal_and_phase_monotonic() {
        let mut pm = fresh_even_map(1);
        assert!(expect_err(pm.advise(0, PAGE_SIZE, 0), MadvErr::EPhase));
        assert!(pm.freeze_map().is_ok());
        // 冻结后 map_page 拒（越相）
        assert!(expect_err(pm.map_page(9, true).map(|_| 0).map_err(|_| MadvErr::EPhase), MadvErr::EPhase));
        assert!(pm.seal().is_ok());
        assert!(expect_err(pm.advise(0, PAGE_SIZE, 0), MadvErr::ESealed));
        assert!(expect_err(pm.map_page(9, true).map(|_| 0).map_err(|_| MadvErr::EPhase), MadvErr::EPhase));
        // 二次封印显性拒
        assert!(expect_err(pm.seal().map(|_| 0).map_err(|_| MadvErr::ESealed), MadvErr::ESealed));
    }

    #[test]
    fn free_semantics() {
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.map_page(1, false);
        let _ = pm.freeze_map();
        // 文件页 FREE 拒
        assert!(expect_err(pm.advise(PAGE_SIZE, PAGE_SIZE, 8), MadvErr::ENotAnon));
        // 匿名首 FREE 过
        match pm.advise(0, PAGE_SIZE, 8) {
            Ok(1) => {}
            _ => assert!(false, "匿名首 FREE 应过"),
        }
        assert!(pm.is_freed(0));
        // 同页二 FREE 拒
        assert!(expect_err(pm.advise(0, PAGE_SIZE, 8), MadvErr::EFreeTwice));
        // 跨页区间含已 FREE 页 → EFreeTwice
        assert!(expect_err(pm.advise(0, 2 * PAGE_SIZE, 8), MadvErr::EFreeTwice));
    }

    #[test]
    fn ledger_cap_64() {
        let mut pm = PageMap::new();
        let mut i = 0u64;
        while i < 64 {
            let _ = pm.map_page(i, true);
            i += 1;
        }
        let _ = pm.freeze_map();
        let mut i = 0u64;
        while i < 64 {
            match pm.advise(i * PAGE_SIZE, PAGE_SIZE, 0) {
                Ok(1) => {}
                _ => assert!(false, "前 64 笔应全过"),
            }
            i += 1;
        }
        assert_eq!(pm.ledger_len(), LEDGER_CAP);
        assert!(expect_err(pm.advise(0, PAGE_SIZE, 1), MadvErr::ELedgerFull));
    }

    #[test]
    fn invariant_and_overflow_window() {
        let mut pm = fresh_even_map(8);
        let _ = pm.freeze_map();
        let _ = pm.advise(0, 2 * PAGE_SIZE, 4);
        let _ = pm.advise(2 * PAGE_SIZE, PAGE_SIZE, 8);
        assert!(pm.invariant_ok());
        // 回绕
        assert!(expect_err(
            pm.advise(0xFFFF_FFFF_FFFF_F000, 8192, 0),
            MadvErr::ERangeOvfl
        ));
        // 越窗
        assert!(expect_err(
            pm.advise(PAGE_WINDOW * PAGE_SIZE, PAGE_SIZE, 0),
            MadvErr::EWindow
        ));
        assert!(pm.invariant_ok());
    }

    #[test]
    fn dual_run_fingerprint_equal() {
        let a = replay_fingerprint();
        let b = replay_fingerprint();
        assert_eq!(a, b, "双跑指纹漂移");
        // 不同序列 → 指纹不同（对拍灵敏度）
        let mut pm = PageMap::new();
        let _ = pm.map_page(0, true);
        let _ = pm.freeze_map();
        let c = pm.fingerprint();
        assert_ne!(a, c);
    }

    #[test]
    fn recon_table_crosscheck() {
        assert_eq!(RECON_TABLE.len(), 10);
        for row in RECON_TABLE.iter() {
            let (name, raw, idx) = *row;
            let a = match MadvAdvice::from_raw(raw) {
                Some(v) => v,
                None => {
                    assert!(false, "RECON 行解析失败");
                    continue;
                }
            };
            assert_eq!(a.name(), name);
            let b = match MadvAdvice::ALL.get(idx as usize) {
                Some(v) => *v,
                None => {
                    assert!(false, "RECON 序号越界");
                    continue;
                }
            };
            assert_eq!(a, b);
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9409_checks();
        assert!(s.all_passed(), "域自检有红项");
        assert!(!s.truncated(), "域自检被截断");
        assert!(s.len() >= 12, "检查项不足 12");
        // 状态摘要写回
        let pm = fresh_even_map(1);
        let sum = pm.status_summary();
        assert!(sum.len() > 0);
        assert!(sum.contains("0x7D3C0"));
    }
}
