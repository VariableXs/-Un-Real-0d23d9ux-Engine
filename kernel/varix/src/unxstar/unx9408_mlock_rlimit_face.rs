//! UNX-F8408 mlock/munlock 错误面 · 0x7D380段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：收库会话自领单（与 VX11 既有 9001~9407 卡面零重复：F8408 聚焦
//!       mlock/munlock 页账幂等与 RLIMIT_MEMLOCK 额度判序错误面，既有
//!       9130/9131 卡走 mlockall 语义路径，未涉足页账全等判定面）。
//!
//!   UNX-F8408-J1 mlock 页对齐注入判据过：非对齐 addr 全 EUnaligned 格过；
//!   RLIMIT_MEMLOCK 剩余额度判序格过（cur_locked + 新页 > rlim →
//!   EOverRlim，rlim=0 全拒、RLIM_INFINITY 永不拒）；len==0 零误伤
//!   （ok 空操作）格过；双重锁同页幂等（refcnt 不重复计额度）格过；
//!   munlock 未锁页 ENotLocked 格过；munlock 区间部分覆盖按页区间账
//!   全等判定（ERangeMismatch）格过；refcnt 下溢守卫格过；注入必抓格过；
//!   双跑一致（确定性）格过；段位守卫格过；闭集诊断码格过。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 真实判定器：mlock/munlock 页账模型（PAGE_SIZE=4096），判序与
//!      Linux 一致：封账 → 账容 → addr 页对齐（EUnaligned）→ 空长零误伤
//!      → 区间溢出（EAddrOverflow）→ 模型护栏（ERangeHuge）→ 额度
//!      （EOverRlim）→ 记账生效。
//!   2) 页账幂等记账：LockTable {(page_no, refcnt)} 按页号升序；lock 对
//!      重叠页 refcnt++（幂等），额度只按「新增页 × 4096」计——双重锁
//!      同页不重复计额度（零误伤）；total_locked = refcnt>0 的页数 ×
//!      4096，refcnt 只表达重叠锁层数。
//!   3) munlock 判序：未锁页 ENotLocked（含 refcnt 下溢守卫）→ 页区间账
//!      全等判定（极大连续锁程含入区间则放行，部分覆盖 ERangeMismatch）
//!      → refcnt-- 归零移除。
//!   4) RLIMIT_MEMLOCK：rlim_bytes 判定 cur + 新页 > rlim → EOverRlim；
//!      rlim=0 全拒（len==0 仍零误伤）；RLIM_INFINITY（u64::MAX 哨兵）
//!      永不拒。
//!   5) 注入器 10 类违约/语义（非对齐 0x1001、超额锁、未锁页解、部分
//!      覆盖解、封账后锁、账本满、len==0 零误伤、双重锁幂等、rlim=0、
//!      refcnt 下溢），断言必抓 10/10（精确诊断码匹配）。
//!   6) 双跑一致：同一输入序列跑两遍，FNV-1a 64 位指纹逐位相等；
//!      段位守卫 0x7D380 步进 0x40 内不越界。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/裸下标；
//!      取值走 match/if let/get/iter/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D380;

// ════════════════════════ 域常量 ════════════════════════

/// 页尺寸（x86_64 基准页）。
pub const PAGE_SIZE: u64 = 4096;
/// mlock 粒度 = 页尺寸。
pub const MLOCK_GRANULARITY: u64 = 4096;
/// 锁区间向下圆整基数（addr 已强制对齐，圆整偏移为 0）。
pub const LOCK_ROUNDDOWN: u64 = 0;
/// RLIM_INFINITY 哨兵（u64::MAX，对账表按 i64 记 -1）。
pub const RLIM_INFINITY: u64 = u64::MAX;
/// mlock2 MLOCK_ONFAULT 标志位。
pub const MLOCK_ONFAULT: i64 = 1;
/// mlockall MCL_CURRENT 标志位。
pub const MCL_CURRENT: i64 = 1;

/// 模型护栏：单次区间页数上界（防迭代风暴，超出即 ERangeHuge）。
const MODEL_PAGE_GUARD: u64 = 1 << 20;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MlockErr {
    /// addr 未页对齐。
    EUnaligned,
    /// cur_locked + 新页 > RLIMIT_MEMLOCK。
    EOverRlim,
    /// munlock 未锁页 / refcnt 下溢守卫。
    ENotLocked,
    /// munlock 区间与锁账不全等（部分覆盖）。
    ERangeMismatch,
    /// 封账后写。
    ESealed,
    /// 账本满（容量守恒被触）。
    ELedgerFull,
    /// addr + len 跨地址空间溢出。
    EAddrOverflow,
    /// 区间页数超模型护栏（防迭代风暴）。
    ERangeHuge,
    /// 账本 step 回退（单调违序，审计）。
    EStepReg,
    /// total_locked > rlim（额度漂移，审计）。
    ERlimDrift,
    /// 页账存在 refcnt==0 或页号乱序（存在性漂移，审计）。
    ERunDrift,
}

impl MlockErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MlockErr::EUnaligned => "addr 未按 4096 页对齐",
            MlockErr::EOverRlim => "超 RLIMIT_MEMLOCK 剩余额度",
            MlockErr::ENotLocked => "munlock 未锁页或 refcnt 下溢守卫",
            MlockErr::ERangeMismatch => "munlock 区间与页账不全等（部分覆盖）",
            MlockErr::ESealed => "封账后禁写",
            MlockErr::ELedgerFull => "账本容量触顶",
            MlockErr::EAddrOverflow => "addr+len 跨地址空间溢出",
            MlockErr::ERangeHuge => "区间页数超模型护栏",
            MlockErr::EStepReg => "账本 step 回退，单调违序",
            MlockErr::ERlimDrift => "total_locked 漂移超 rlim",
            MlockErr::ERunDrift => "页账 refcnt==0 或页号乱序",
        }
    }
    pub const ALL: [MlockErr; 11] = [
        MlockErr::EUnaligned,
        MlockErr::EOverRlim,
        MlockErr::ENotLocked,
        MlockErr::ERangeMismatch,
        MlockErr::ESealed,
        MlockErr::ELedgerFull,
        MlockErr::EAddrOverflow,
        MlockErr::ERangeHuge,
        MlockErr::EStepReg,
        MlockErr::ERlimDrift,
        MlockErr::ERunDrift,
    ];
}

// ════════════════════════ 状态机：LockTable ════════════════════════

/// Phase 状态机：Fresh → Live → Sealed，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Fresh,
    Live,
    Sealed,
}

/// 账本操作种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Lock,
    Unlock,
}

/// 账本记录（每次调用入账，成败皆记，err=0 为成功）。
#[derive(Clone, Copy, Debug)]
pub struct OpRec {
    pub step: u64,
    pub kind: OpKind,
    /// 首页页号（区间请求原样，未折算）。
    pub first: u64,
    /// 末页页号。
    pub last: u64,
    /// 0 = 成功，非 0 = 诊断码。
    pub err: u32,
}

pub const LEDGER_CAP: usize = 64;

/// mlock 页账 + RLIMIT_MEMLOCK 额度守恒。
pub struct LockTable {
    /// 锁页账：(page_no, refcnt)，按页号严格升序，refcnt ≥ 1。
    pages: Vec<(u64, u32)>,
    rlim_bytes: u64,
    phase: Phase,
    ledger: Vec<OpRec>,
    step: u64,
    sealed: bool,
}

impl LockTable {
    /// 默认额度 = RLIM_INFINITY（u64::MAX 哨兵，永不 EOverRlim）。
    pub fn new() -> Self {
        LockTable::new_with_rlim(RLIM_INFINITY)
    }

    pub fn new_with_rlim(rlim_bytes: u64) -> Self {
        LockTable {
            pages: Vec::new(),
            rlim_bytes,
            phase: Phase::Fresh,
            ledger: Vec::new(),
            step: 0,
            sealed: false,
        }
    }

    pub fn rlim_bytes(&self) -> u64 {
        self.rlim_bytes
    }
    pub fn pages(&self) -> &[(u64, u32)] {
        &self.pages
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn ledger(&self) -> &[OpRec] {
        &self.ledger
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// total_locked = refcnt>0 的页数 × 4096（refcnt 只表达重叠层数）。
    pub fn total_locked(&self) -> u64 {
        let n = self.pages.iter().filter(|(_, rc)| *rc > 0).count() as u64;
        n.checked_mul(PAGE_SIZE).unwrap_or(u64::MAX)
    }

    /// 页 p 当前 refcnt（未锁为 0）。
    fn refcnt_of(&self, page: u64) -> u32 {
        match self.pages.iter().find(|(p, _)| *p == page) {
            Some((_, rc)) => *rc,
            None => 0,
        }
    }

    fn is_locked(&self, page: u64) -> bool {
        self.refcnt_of(page) > 0
    }

    /// 单页 refcnt++（幂等入账；已存在则增层，不存在则插入，保持升序）。
    fn lock_one(&mut self, page: u64) {
        match self.pages.iter().position(|(p, _)| *p == page) {
            Some(idx) => {
                if let Some(entry) = self.pages.get_mut(idx) {
                    let (_, rc) = entry;
                    *rc = rc.saturating_add(1);
                }
            }
            None => self.pages.push((page, 1)),
        }
    }

    /// mlock 判定主入口。判序：封账 → 账容 → 页对齐（EUnaligned）→
    /// 区间溢出 → 模型护栏 → 空长零误伤 → 额度（EOverRlim，按新增页
    /// 幂等计）→ refcnt++ 生效。
    pub fn lock(&mut self, addr: u64, len: u64) -> Result<(u64, u64), u32> {
        // 判序 ①：封账后禁写。
        if self.sealed {
            return Err(MlockErr::ESealed.val());
        }
        // 判序 ②：账本容量守恒。
        if self.ledger.len() >= LEDGER_CAP {
            return Err(MlockErr::ELedgerFull.val());
        }
        let record = |this: &mut Self, first: u64, last: u64, err: u32| {
            this.ledger.push(OpRec {
                step: this.step,
                kind: OpKind::Lock,
                first,
                last,
                err,
            });
            this.step = this.step.saturating_add(1);
            if let Phase::Fresh = this.phase {
                this.phase = Phase::Live;
            }
        };
        // 判序 ③：addr 页对齐。
        if addr % PAGE_SIZE != 0 {
            let page = addr / PAGE_SIZE;
            record(self, page, page, MlockErr::EUnaligned.val());
            return Err(MlockErr::EUnaligned.val());
        }
        // 判序 ④：区间溢出。
        let end = match addr.checked_add(len) {
            Some(e) => e,
            None => {
                let page = addr / PAGE_SIZE;
                record(self, page, page, MlockErr::EAddrOverflow.val());
                return Err(MlockErr::EAddrOverflow.val());
            }
        };
        let first = addr / PAGE_SIZE;
        let last = if len == 0 { first } else { (end - 1) / PAGE_SIZE };
        // 判序 ⑤：模型护栏（页数上界，防迭代风暴）。
        let span = last.saturating_sub(first).saturating_add(1);
        if span > MODEL_PAGE_GUARD {
            record(self, first, last, MlockErr::ERangeHuge.val());
            return Err(MlockErr::ERangeHuge.val());
        }
        // 判序 ⑥：len==0 零误伤（ok 空操作）。
        if len == 0 {
            record(self, first, first, 0);
            return Ok((first, first));
        }
        // 判序 ⑦：额度——只按「新增页 × 4096」幂等计（双重锁不重复计）。
        let mut new_pages = 0u64;
        let mut p = first;
        loop {
            if !self.is_locked(p) {
                new_pages = new_pages.saturating_add(1);
            }
            if p == last {
                break;
            }
            p = match p.checked_add(1) {
                Some(x) => x,
                None => break,
            };
        }
        if self.rlim_bytes != RLIM_INFINITY {
            let need = self
                .total_locked()
                .checked_add(new_pages.checked_mul(PAGE_SIZE).unwrap_or(u64::MAX));
            match need {
                Some(n) if n > self.rlim_bytes => {
                    record(self, first, last, MlockErr::EOverRlim.val());
                    return Err(MlockErr::EOverRlim.val());
                }
                None => {
                    record(self, first, last, MlockErr::EOverRlim.val());
                    return Err(MlockErr::EOverRlim.val());
                }
                _ => {}
            }
        }
        // 判序 ⑧：生效——重叠页 refcnt++（幂等），新页插入。
        let mut p = first;
        loop {
            self.lock_one(p);
            if p == last {
                break;
            }
            p = match p.checked_add(1) {
                Some(x) => x,
                None => break,
            };
        }
        record(self, first, last, 0);
        Ok((first, last))
    }

    /// munlock 判定主入口。判序：封账 → 账容 → 页对齐 → 空长零误伤 →
    /// 未锁页 ENotLocked（含下溢守卫）→ 页区间账全等（部分覆盖
    /// ERangeMismatch）→ refcnt-- 归零移除。
    pub fn unlock(&mut self, addr: u64, len: u64) -> Result<(u64, u64), u32> {
        // 判序 ①：封账后禁写。
        if self.sealed {
            return Err(MlockErr::ESealed.val());
        }
        // 判序 ②：账本容量守恒。
        if self.ledger.len() >= LEDGER_CAP {
            return Err(MlockErr::ELedgerFull.val());
        }
        let record = |this: &mut Self, first: u64, last: u64, err: u32| {
            this.ledger.push(OpRec {
                step: this.step,
                kind: OpKind::Unlock,
                first,
                last,
                err,
            });
            this.step = this.step.saturating_add(1);
            if let Phase::Fresh = this.phase {
                this.phase = Phase::Live;
            }
        };
        // 判序 ③：addr 页对齐。
        if addr % PAGE_SIZE != 0 {
            let page = addr / PAGE_SIZE;
            record(self, page, page, MlockErr::EUnaligned.val());
            return Err(MlockErr::EUnaligned.val());
        }
        let end = match addr.checked_add(len) {
            Some(e) => e,
            None => {
                let page = addr / PAGE_SIZE;
                record(self, page, page, MlockErr::EAddrOverflow.val());
                return Err(MlockErr::EAddrOverflow.val());
            }
        };
        let first = addr / PAGE_SIZE;
        let last = if len == 0 { first } else { (end - 1) / PAGE_SIZE };
        let span = last.saturating_sub(first).saturating_add(1);
        if span > MODEL_PAGE_GUARD {
            record(self, first, last, MlockErr::ERangeHuge.val());
            return Err(MlockErr::ERangeHuge.val());
        }
        // 判序 ④：len==0 零误伤（ok 空操作）。
        if len == 0 {
            record(self, first, first, 0);
            return Ok((first, first));
        }
        // 判序 ⑤：区间每页须已锁（下溢守卫：缺失页即 ENotLocked）。
        {
            let mut p = first;
            loop {
                if !self.is_locked(p) {
                    record(self, first, last, MlockErr::ENotLocked.val());
                    return Err(MlockErr::ENotLocked.val());
                }
                if p == last {
                    break;
                }
                p = match p.checked_add(1) {
                    Some(x) => x,
                    None => break,
                };
            }
        }
        // 判序 ⑥：页区间账全等——极大连续锁程须恰为请求区间
        // （部分覆盖 → ERangeMismatch）。
        let mut run_first = first;
        let mut run_last = last;
        while run_first > 0 && self.is_locked(run_first - 1) {
            run_first -= 1;
        }
        loop {
            let next = match run_last.checked_add(1) {
                Some(x) => x,
                None => break,
            };
            if self.is_locked(next) {
                run_last = next;
            } else {
                break;
            }
        }
        if run_first != first || run_last != last {
            record(self, first, last, MlockErr::ERangeMismatch.val());
            return Err(MlockErr::ERangeMismatch.val());
        }
        // 判序 ⑦：生效——区间每页 refcnt--，归零移除（下溢由 saturating
        // 兜底，存在性已由判序 ⑤ 保证）。
        let mut out: Vec<(u64, u32)> = Vec::new();
        for (p, rc) in self.pages.iter() {
            if *p >= first && *p <= last {
                let n = rc.saturating_sub(1);
                if n > 0 {
                    out.push((*p, n));
                }
            } else {
                out.push((*p, *rc));
            }
        }
        self.pages = out;
        record(self, first, last, 0);
        Ok((first, last))
    }

    /// 封账：Phase 单调推进到 Sealed；重复封账拒止。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(MlockErr::ESealed.val());
        }
        self.sealed = true;
        self.phase = Phase::Sealed;
        Ok(self.ledger.len())
    }

    /// 审计：额度守恒 + 页账存在性（refcnt ≥ 1、页号严格升序无重）+
    /// 账本单调 + Phase 单调。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rlim_bytes != RLIM_INFINITY && self.total_locked() > self.rlim_bytes {
            return Err(MlockErr::ERlimDrift.val());
        }
        let mut prev: Option<u64> = None;
        for (p, rc) in self.pages.iter() {
            if *rc == 0 {
                return Err(MlockErr::ERunDrift.val());
            }
            if let Some(pp) = prev {
                if *p <= pp {
                    return Err(MlockErr::ERunDrift.val());
                }
            }
            prev = Some(*p);
        }
        let mut prev_step: Option<u64> = None;
        for r in self.ledger.iter() {
            if let Some(s) = prev_step {
                if r.step <= s {
                    return Err(MlockErr::EStepReg.val());
                }
            }
            prev_step = Some(r.step);
        }
        match self.phase {
            Phase::Fresh if !self.ledger.is_empty() => return Err(MlockErr::EStepReg.val()),
            Phase::Sealed if !self.sealed => return Err(MlockErr::EStepReg.val()),
            _ => {}
        }
        Ok(())
    }

    /// 不变量（audit 全绿 + 账本容量守恒）。
    pub fn invariant_ok(&self) -> bool {
        self.ledger.len() <= LEDGER_CAP && self.audit().is_ok()
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（精确诊断码匹配），共 10 类。
pub struct Injector;

impl Injector {
    /// 10 类注入样本，逐一构造并断言必抓。返回 (抓到数, 总数)。
    /// 共享表 `t` 走主场景（对齐/全等/幂等/封账），额度类场景用局部表。
    pub fn run_all(t: &mut LockTable) -> (usize, usize) {
        let mut caught = 0usize;
        let total = 10usize;
        // ① 非对齐 addr 0x1001 → EUnaligned。
        if t.lock(0x1001, PAGE_SIZE) == Err(MlockErr::EUnaligned.val()) {
            caught += 1;
        }
        // ② 超额度锁：局部表 rlim=4096 锁 2 页 → EOverRlim。
        let mut a = LockTable::new_with_rlim(PAGE_SIZE);
        if a.lock(0x0000, 2 * PAGE_SIZE) == Err(MlockErr::EOverRlim.val()) {
            caught += 1;
        }
        // ③ 解未锁页 → ENotLocked。
        if t.unlock(0x9000, PAGE_SIZE) == Err(MlockErr::ENotLocked.val()) {
            caught += 1;
        }
        // ④ 部分覆盖解：锁页 2..3，只解页 2 → ERangeMismatch；随后
        //    全等解页 2..3 放行（零误杀对照）。
        if t.lock(0x2000, 2 * PAGE_SIZE).is_ok()
            && t.unlock(0x2000, PAGE_SIZE) == Err(MlockErr::ERangeMismatch.val())
            && t.unlock(0x2000, 2 * PAGE_SIZE).is_ok()
        {
            caught += 1;
        }
        // ⑤ 双重锁幂等（零误杀）：refcnt=2 而 total_locked 不变。
        if t.lock(0x0000, PAGE_SIZE).is_ok() && t.lock(0x0000, PAGE_SIZE).is_ok() {
            let rc = match t.pages().first() {
                Some((_, rc)) => *rc,
                None => 0,
            };
            if rc == 2 && t.total_locked() == PAGE_SIZE {
                caught += 1;
            }
        }
        // ⑥ len==0 零误伤：ok 空操作，页账与额度不变。
        let before = t.total_locked();
        if t.lock(0x5000, 0).is_ok() && t.total_locked() == before {
            caught += 1;
        }
        // ⑦ rlim=0：一切真锁全拒（EOverRlim），len==0 仍零误伤。
        let mut z = LockTable::new_with_rlim(0);
        if z.lock(0x0000, PAGE_SIZE) == Err(MlockErr::EOverRlim.val())
            && z.lock(0x1000, 0).is_ok()
        {
            caught += 1;
        }
        // ⑧ refcnt 下溢守卫：锁一解一再解 → ENotLocked。
        let mut c = LockTable::new();
        if c.lock(0x5000, PAGE_SIZE).is_ok() && c.unlock(0x5000, PAGE_SIZE).is_ok() {
            if c.unlock(0x5000, PAGE_SIZE) == Err(MlockErr::ENotLocked.val()) {
                caught += 1;
            }
        }
        // ⑨ 账本满：len==0 空操作灌满后下一次写 → ELedgerFull。
        let mut d = LockTable::new();
        while d.ledger.len() < LEDGER_CAP {
            let _ = d.lock(0x0000, 0);
        }
        if d.lock(0x0000, PAGE_SIZE) == Err(MlockErr::ELedgerFull.val()) {
            caught += 1;
        }
        // ⑩ 封账后锁 → ESealed。
        let _ = t.seal();
        if t.lock(0x6000, PAGE_SIZE) == Err(MlockErr::ESealed.val())
            && t.unlock(0x6000, PAGE_SIZE) == Err(MlockErr::ESealed.val())
        {
            caught += 1;
        }
        (caught, total)
    }
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 对账表条目：mlock/rlimit 域常量对号（名, 原始记账值, 规范槽位）。
/// RLIM_INFINITY 按 u64::MAX 哨兵记账，i64 侧记 -1（Linux getrlimit 惯例）。
pub const RECON_TABLE: [(&str, i64, i32); 6] = [
    ("PAGE_SIZE", 4096, 0),
    ("MLOCK_GRANULARITY", 4096, 1),
    ("LOCK_ROUNDDOWN", 0, 2),
    ("RLIM_INFINITY", -1, 3),
    ("MLOCK_ONFAULT", 1, 4),
    ("MCL_CURRENT", 1, 5),
];

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9408_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9408");
    // J1-① 非对齐地址全抓。
    {
        let bad: [u64; 6] = [1, 0x1001, PAGE_SIZE - 1, 0x1801, 0x2001, u64::MAX];
        let mut all_ebad = true;
        for addr in bad.iter() {
            let mut t = LockTable::new();
            if t.lock(*addr, PAGE_SIZE) != Err(MlockErr::EUnaligned.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-01 非对齐全EUnaligned", all_ebad, "6 类非对齐 addr 全拒");
    }
    // J1-② 对齐零误伤。
    {
        let mut all_ok = true;
        for addr in [0u64, PAGE_SIZE, 0x10000, 0x10_0000].iter() {
            let mut t = LockTable::new();
            if t.lock(*addr, PAGE_SIZE).is_err() {
                all_ok = false;
            }
        }
        s.add("J1-02 对齐零误伤", all_ok, "对齐 addr 全绿");
    }
    // J1-③ 超额度判序。
    {
        let mut t = LockTable::new_with_rlim(4 * PAGE_SIZE);
        let first_ok = t.lock(0x0, 4 * PAGE_SIZE).is_ok();
        let over = t.lock(4 * PAGE_SIZE, PAGE_SIZE) == Err(MlockErr::EOverRlim.val());
        s.add(
            "J1-03 额度判序EOverRlim",
            first_ok && over,
            "恰容 4 页绿，第 5 页拒（cur+新页 > rlim）",
        );
    }
    // J1-④ len==0 零误伤（ok 空操作）。
    {
        let mut t = LockTable::new();
        let r = t.lock(0x3000, 0);
        let ok = match r {
            Ok((f, l)) => f == 0x3000 / PAGE_SIZE && l == f && t.total_locked() == 0,
            Err(_) => false,
        };
        s.add("J1-04 空长零误伤", ok, "len==0 返回 ok 且页账零变化");
    }
    // J1-⑤ 双重锁幂等：refcnt 增层、额度不重复计。
    {
        let mut t = LockTable::new();
        let _ = t.lock(0x0, PAGE_SIZE);
        let total_once = t.total_locked();
        let _ = t.lock(0x0, PAGE_SIZE);
        let rc = match t.pages().first() {
            Some((_, rc)) => *rc,
            None => 0,
        };
        let ok = rc == 2 && total_once == PAGE_SIZE && t.total_locked() == PAGE_SIZE;
        s.add(
            "J1-05 双重锁幂等",
            ok,
            "refcnt=2 而 total_locked 恒 4096，幂等不重复计",
        );
    }
    // J1-⑥ 解未锁页全抓。
    {
        let mut all_ebad = true;
        for addr in [0x9000u64, 0x10000, 0x2_0000].iter() {
            let mut t = LockTable::new();
            if t.unlock(*addr, PAGE_SIZE) != Err(MlockErr::ENotLocked.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-06 解未锁页全ENotLocked", all_ebad, "未锁页 unlock 全拒");
    }
    // J1-⑦ 部分覆盖解全抓，全等解放行。
    {
        let mut t = LockTable::new();
        let _ = t.lock(0x2000, 2 * PAGE_SIZE);
        let partial = t.unlock(0x2000, PAGE_SIZE) == Err(MlockErr::ERangeMismatch.val());
        let partial_hi = t.unlock(0x3000, PAGE_SIZE) == Err(MlockErr::ERangeMismatch.val());
        let exact = t.unlock(0x2000, 2 * PAGE_SIZE).is_ok() && t.total_locked() == 0;
        s.add(
            "J1-07 页账全等判定",
            partial && partial_hi && exact,
            "低/高端部分覆盖全拒，全等解放行且页账清零",
        );
    }
    // J1-⑧ refcnt 下溢守卫。
    {
        let mut t = LockTable::new();
        let _ = t.lock(0x4000, PAGE_SIZE);
        let _ = t.unlock(0x4000, PAGE_SIZE);
        let under = t.unlock(0x4000, PAGE_SIZE) == Err(MlockErr::ENotLocked.val());
        s.add(
            "J1-08 下溢守卫",
            under && t.invariant_ok(),
            "解后再解落于 ENotLocked，页账不产生 refcnt=0",
        );
    }
    // J1-⑨ rlim=0 全拒，RLIM_INFINITY 永不拒。
    {
        let mut z = LockTable::new_with_rlim(0);
        let zero_reject = z.lock(0x0, PAGE_SIZE) == Err(MlockErr::EOverRlim.val());
        let zero_len_ok = z.lock(0x0, 0).is_ok();
        let mut inf = LockTable::new();
        let mut inf_ok = true;
        let mut addr = 0u64;
        let mut k = 0u64;
        while k < 8 {
            if inf.lock(addr, PAGE_SIZE).is_err() {
                inf_ok = false;
            }
            addr += 0x10_0000;
            k += 1;
        }
        s.add(
            "J1-09 rlim0与无穷",
            zero_reject && zero_len_ok && inf_ok,
            "rlim=0 真锁全拒空长绿；RLIM_INFINITY 八页全绿",
        );
    }
    // J1-⑩ 注入器 10/10。
    {
        let mut t = LockTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        s.add(
            "J1-10 注入器10/10必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑪ 对账表 + 段位守卫。
    {
        let mut ok = true;
        for (name, raw, slot) in RECON_TABLE.iter() {
            let hit = match *slot {
                0 => *raw == PAGE_SIZE as i64,
                1 => *raw == MLOCK_GRANULARITY as i64,
                2 => *raw == LOCK_ROUNDDOWN as i64,
                3 => *raw == -1 && RLIM_INFINITY == u64::MAX,
                4 => *raw == MLOCK_ONFAULT,
                5 => *raw == MCL_CURRENT,
                _ => false,
            };
            let _ = name;
            if !hit {
                ok = false;
            }
        }
        let seg_ok = SEG_BASE == 0x7D380
            && MlockErr::ALL
                .iter()
                .all(|e| e.val() >= SEG_BASE && e.val() < SEG_BASE + 0x40);
        s.add(
            "J1-11 对账表与段位守卫",
            ok && seg_ok,
            "RECON_TABLE 六行对号；0x7D380 段步进 0x40 内不越界",
        );
    }
    // J1-⑫ 双跑一致。
    {
        let run = || -> u64 {
            let mut t = LockTable::new_with_rlim(8 * PAGE_SIZE);
            let mut log = String::new();
            let seq: [(bool, u64, u64); 10] = [
                (true, 0x0, PAGE_SIZE),
                (true, 0x0, PAGE_SIZE),
                (true, 0x1000, 2 * PAGE_SIZE),
                (false, 0x1000, PAGE_SIZE),
                (true, 0x4000, 0),
                (false, 0x4000, 0),
                (true, 0x6000, 2 * PAGE_SIZE),
                (false, 0x6000, PAGE_SIZE),
                (false, 0x0, PAGE_SIZE),
                (true, 0x8000, PAGE_SIZE),
            ];
            for (is_lock, addr, len) in seq.iter() {
                let r = if *is_lock {
                    t.lock(*addr, *len)
                } else {
                    t.unlock(*addr, *len)
                };
                match r {
                    Ok((f, l)) => {
                        let line = format!("{},{},{},{};", *is_lock as u8, f, l, t.total_locked());
                        log.push_str(&line);
                    }
                    Err(_) => log.push('E'),
                }
            }
            let _ = t.seal();
            fnv1a(log.as_bytes())
        };
        let a = run();
        let b = run();
        s.add("J1-12 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑬ 封账纪律。
    {
        let mut t = LockTable::new();
        let _ = t.lock(0x0, PAGE_SIZE);
        let sealed = t.seal().is_ok() && t.sealed();
        let lock_rej = t.lock(0x1000, PAGE_SIZE) == Err(MlockErr::ESealed.val());
        let unlock_rej = t.unlock(0x0, PAGE_SIZE) == Err(MlockErr::ESealed.val());
        let reseal_rej = t.seal().is_err();
        s.add(
            "J1-13 封账纪律",
            sealed && lock_rej && unlock_rej && reseal_rej,
            "封账置位、锁与解双拒、重复封账拒止",
        );
    }
    // J1-⑭ 审计/不变量守恒。
    {
        let mut t = LockTable::new_with_rlim(4 * PAGE_SIZE);
        let _ = t.lock(0x0, 2 * PAGE_SIZE);
        let _ = t.lock(0x1000, 2 * PAGE_SIZE);
        let ok = t.audit().is_ok()
            && t.invariant_ok()
            && t.total_locked() == 3 * PAGE_SIZE
            && t.total_locked() <= t.rlim_bytes();
        s.add(
            "J1-14 审计不变量守恒",
            ok,
            "额度守恒/refcnt 存在性/账本单调全过",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", ledger_depth_ok(), "每次调用账本恰增一格（成败皆记）");
    s.add_dyn("封账状态", seal_state_ok(), "封账后 sealed 置位且拒写");
    s.add_dyn("指纹非零", fnv_nonzero(), "FNV 指纹非零，可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn ledger_depth_ok() -> bool {
    let mut t = LockTable::new();
    let _ = t.lock(0x0, PAGE_SIZE);
    t.ledger().len() == 1
}

fn seal_state_ok() -> bool {
    let mut t = LockTable::new();
    let _ = t.lock(0x0, PAGE_SIZE);
    t.seal().is_ok() && t.sealed()
}

fn fnv_nonzero() -> bool {
    fnv1a(&[1u8, 2, 3]) != 0
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_unaligned_rejected() {
        for addr in [1u64, 0x1001, PAGE_SIZE - 1] {
            let mut t = LockTable::new();
            assert_eq!(t.lock(addr, PAGE_SIZE), Err(MlockErr::EUnaligned.val()));
            assert_eq!(t.unlock(addr, PAGE_SIZE), Err(MlockErr::EUnaligned.val()));
        }
    }

    #[test]
    fn t02_zero_len_noop_ok() {
        let mut t = LockTable::new();
        let r = t.lock(0x2000, 0);
        match r {
            Ok((f, l)) => {
                assert_eq!(f, 2);
                assert_eq!(l, 2);
            }
            Err(_) => assert!(false, "len==0 须零误伤"),
        }
        assert_eq!(t.total_locked(), 0);
        assert!(t.unlock(0x2000, 0).is_ok());
    }

    #[test]
    fn t03_over_rlim_and_infinity() {
        let mut t = LockTable::new_with_rlim(PAGE_SIZE);
        assert!(t.lock(0x0, PAGE_SIZE).is_ok());
        assert_eq!(
            t.lock(PAGE_SIZE, PAGE_SIZE),
            Err(MlockErr::EOverRlim.val())
        );
        let mut inf = LockTable::new();
        assert!(inf.lock(0x0, 16 * PAGE_SIZE).is_ok());
    }

    #[test]
    fn t04_double_lock_idempotent_refcnt() {
        let mut t = LockTable::new();
        let _ = t.lock(0x0, 2 * PAGE_SIZE);
        let _ = t.lock(0x1000, 2 * PAGE_SIZE); // 与前锁重叠页 1
        // 三页 distinct（0/1/2），重叠页 1 计两层。
        assert_eq!(t.total_locked(), 3 * PAGE_SIZE);
        let rc0 = match t.pages().first() {
            Some((_, rc)) => *rc,
            None => 0,
        };
        let rc1 = match t.pages().get(1) {
            Some((_, rc)) => *rc,
            None => 0,
        };
        assert_eq!(rc0, 1);
        assert_eq!(rc1, 2);
        // 同页二次锁幂等：total 不变、refcnt 增层。
        let before = t.total_locked();
        let _ = t.lock(0x0, 2 * PAGE_SIZE);
        assert_eq!(t.total_locked(), before);
        let rc0b = match t.pages().first() {
            Some((_, rc)) => *rc,
            None => 0,
        };
        assert_eq!(rc0b, 2);
    }

    #[test]
    fn t05_unlock_partial_range_mismatch() {
        let mut t = LockTable::new();
        let _ = t.lock(0x2000, 3 * PAGE_SIZE);
        // 部分覆盖（子区间）：页账全等判定 → ERangeMismatch。
        assert_eq!(
            t.unlock(0x2000, PAGE_SIZE),
            Err(MlockErr::ERangeMismatch.val())
        );
        assert_eq!(
            t.unlock(0x3000, PAGE_SIZE),
            Err(MlockErr::ERangeMismatch.val())
        );
        // 超集区间含未锁页：判序⑤未锁页先判 → ENotLocked。
        assert_eq!(
            t.unlock(0x2000, 4 * PAGE_SIZE),
            Err(MlockErr::ENotLocked.val())
        );
        // 全等解放行，页账清零。
        assert!(t.unlock(0x2000, 3 * PAGE_SIZE).is_ok());
        assert_eq!(t.total_locked(), 0);
    }

    #[test]
    fn t06_unlock_not_locked() {
        let mut t = LockTable::new();
        assert_eq!(t.unlock(0x9000, PAGE_SIZE), Err(MlockErr::ENotLocked.val()));
        // 有锁存在但解别的页：同样 ENotLocked。
        let _ = t.lock(0x0, PAGE_SIZE);
        assert_eq!(t.unlock(0x9000, PAGE_SIZE), Err(MlockErr::ENotLocked.val()));
    }

    #[test]
    fn t07_refcnt_underflow_guard() {
        let mut t = LockTable::new();
        let _ = t.lock(0x5000, PAGE_SIZE);
        assert!(t.unlock(0x5000, PAGE_SIZE).is_ok());
        assert_eq!(
            t.unlock(0x5000, PAGE_SIZE),
            Err(MlockErr::ENotLocked.val())
        );
        assert!(t.audit().is_ok());
    }

    #[test]
    fn t08_rlim_zero_all_locks_rejected() {
        let mut t = LockTable::new_with_rlim(0);
        assert_eq!(t.lock(0x0, PAGE_SIZE), Err(MlockErr::EOverRlim.val()));
        assert!(t.lock(0x0, 0).is_ok(), "len==0 在 rlim=0 下仍零误伤");
    }

    #[test]
    fn t09_injector_full_catch() {
        let mut t = LockTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        assert_eq!(caught, total);
        assert_eq!(total, 10);
    }

    #[test]
    fn t10_seal_discipline() {
        let mut t = LockTable::new();
        let _ = t.lock(0x0, PAGE_SIZE);
        assert!(t.seal().is_ok());
        assert_eq!(t.lock(0x1000, PAGE_SIZE), Err(MlockErr::ESealed.val()));
        assert_eq!(t.unlock(0x0, PAGE_SIZE), Err(MlockErr::ESealed.val()));
        assert!(t.seal().is_err());
    }

    #[test]
    fn t11_diag_codes_closed_set() {
        let mut prev = 0u32;
        for (i, e) in MlockErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
        assert_eq!(MlockErr::ALL.len(), 11);
    }

    #[test]
    fn t12_checks_all_pass() {
        let s = run_unx9408_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t13_dual_run_determinism() {
        let run = || {
            let mut t = LockTable::new_with_rlim(8 * PAGE_SIZE);
            let _ = t.lock(0x0, 2 * PAGE_SIZE);
            let _ = t.lock(0x1000, 2 * PAGE_SIZE);
            let _ = t.unlock(0x0, PAGE_SIZE);
            let _ = t.seal();
            (t.total_locked(), t.ledger().len())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn t14_seg_base_and_recon() {
        assert_eq!(SEG_BASE, 0x7D380);
        assert_eq!(RECON_TABLE.len(), 6);
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(RLIM_INFINITY, u64::MAX);
        for (name, raw, slot) in RECON_TABLE.iter() {
            let hit = match *slot {
                0 => *raw == PAGE_SIZE as i64,
                1 => *raw == MLOCK_GRANULARITY as i64,
                2 => *raw == LOCK_ROUNDDOWN as i64,
                3 => *raw == -1,
                4 => *raw == MLOCK_ONFAULT,
                5 => *raw == MCL_CURRENT,
                _ => false,
            };
            assert!(hit, "{} 对账错位", name);
        }
    }
}
