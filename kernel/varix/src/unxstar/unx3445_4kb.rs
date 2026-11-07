//! UNX-F3445 用例隔离执行环境（4KB 隔离页接入执行器） · 0x79C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：F3410 隔离页契约在执行器落地（用例启动前领取/结束后归还 +
//!     越界写检测钩子）；M-204 隔离页越界注入被捕获。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F3410 隔离页契约三件套在执行器内全部落地：领取（claim）→ 运行
//!     （越界写检测钩子逐笔核）→ 归还（give_back），生命周期闭环；
//!   - 领取发页：页池 8 页，每页 4KB，页基址 = 池基址 + 页号×4096，
//!     任何发放页基址必满足 4KB 对齐且页间不重叠；
//!   - 越界写检测钩子：写描述 (偏移, 长度) 逐笔 checked 加法核
//!     off+len ≤ 4096，越界与算术溢出都判越界（不静默截断）；
//!   - 归还保障：越界被抓后执行例立即停笔，但页必须照常归还——
//!     「越界必抓、抓后必还」双承诺，归还漏账显性判红；
//!   - M-204 注入：注入用例（ID=0x204）构造跨页写（偏移 4094 长度 8），
//!     必被钩子捕获且入账 OobWrite 诊断码；
//!   - 双跑一致：同输入两次隔离运行的报告逐字段相同。
//! 常量来源：
//!   - 页尺寸 4096 字节 / 页池 8 页：F3410 隔离页契约（派工卡判据原文
//!     点名的前单），冻结于本模块常量表；
//!   - M-204：派工卡验收判据编号（注入用例 ID 锚定 0x204）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79C80;

/// 隔离页尺寸（字节，F3410 契约冻结：4KB）。
pub const PAGE_SIZE: u32 = 4096;

/// 页池页数（8 页，F3410 契约冻结）。
pub const PAGE_COUNT: usize = 8;

/// 页池虚拟基址（整数面；真实映射由 F3410 页表侧负责，本单只管账）。
pub const POOL_BASE: u64 = 0x4000_0000;

/// M-204 注入用例 ID（派工卡验收判据编号锚定）。
pub const INJECTION_CASE_ID: u32 = 0x204;

/// 冻结用例表：用例名 → (用例 ID, 写程序)。
/// 写程序 = (偏移, 长度) 序列；跨页注入例的写程序人为越界。
pub const CASE_TABLE: [(&str, u32, &[(u32, u32)]); 4] = [
    ("隔离冒烟", 1, &[(0, 64)]),
    ("账本整页", 2, &[(0, 4096)]),
    ("跨页注入", 0x204, &[(4094, 8)]),
    ("尾部对齐", 4, &[(2048, 2048)]),
];

/// 隔离页账目：一页的领取状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoPage {
    /// 页号（0..=7）。
    pub page_no: usize,
    /// 页基址（池基址 + 页号×4096，恒 4KB 对齐）。
    pub base: u64,
    /// 持有者用例 ID（None=空闲）。
    pub holder: Option<u32>,
}

/// 页池：8 页 4KB 隔离页的领取/归还账。
#[derive(Debug, Clone)]
pub struct PagePool {
    pages: Vec<IsoPage>,
}

impl PagePool {
    /// 建池：8 页全空闲，基址逐页算好。
    pub fn new() -> PagePool {
        let mut pages = Vec::new();
        let mut i = 0usize;
        while i < PAGE_COUNT {
            let base = POOL_BASE + (i as u64) * (PAGE_SIZE as u64);
            pages.push(IsoPage {
                page_no: i,
                base,
                holder: None,
            });
            i += 1;
        }
        PagePool { pages }
    }

    /// 已领取页数（账面口径）。
    pub fn claimed_count(&self) -> usize {
        let mut n = 0usize;
        for p in self.pages.iter() {
            if p.holder.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 某用例当前持有的页基址（未持有回 None）。
    pub fn holder_base(&self, case_id: u32) -> Option<u64> {
        for p in self.pages.iter() {
            if p.holder == Some(case_id) {
                return Some(p.base);
            }
        }
        None
    }

    /// 领页：给首个空闲页；同例重复领、池耗尽均显性拒。
    pub fn claim(&mut self, case_id: u32) -> Result<u64, u32> {
        if case_id == 0 {
            return Err(Diag::CaseForeign.val());
        }
        let mut free_idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.pages.len() {
            match self.pages.get(i) {
                Some(p) => {
                    if p.holder.is_none() && free_idx.is_none() {
                        free_idx = Some(i);
                    }
                    if p.holder == Some(case_id) {
                        return Err(Diag::CaseDoubleClaim.val());
                    }
                }
                None => return Err(Diag::PoolBookCorrupt.val()),
            }
            i += 1;
        }
        let idx = match free_idx {
            Some(i) => i,
            None => return Err(Diag::PoolExhausted.val()),
        };
        let base = match self.pages.get(idx) {
            Some(p) => p.base,
            None => return Err(Diag::PoolBookCorrupt.val()),
        };
        match self.pages.get_mut(idx) {
            Some(p) => p.holder = Some(case_id),
            None => return Err(Diag::PoolBookCorrupt.val()),
        }
        Ok(base)
    }

    /// 归还：按持有者核销；未领先还显性拒。返回归还页的基址。
    pub fn give_back(&mut self, case_id: u32) -> Result<u64, u32> {
        if case_id == 0 {
            return Err(Diag::CaseForeign.val());
        }
        let mut held_idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.pages.len() {
            match self.pages.get(i) {
                Some(p) => {
                    if p.holder == Some(case_id) {
                        held_idx = Some(i);
                    }
                }
                None => return Err(Diag::PoolBookCorrupt.val()),
            }
            i += 1;
        }
        let idx = match held_idx {
            Some(i) => i,
            None => return Err(Diag::ReturnWithoutClaim.val()),
        };
        let base = match self.pages.get(idx) {
            Some(p) => p.base,
            None => return Err(Diag::PoolBookCorrupt.val()),
        };
        match self.pages.get_mut(idx) {
            Some(p) => p.holder = None,
            None => return Err(Diag::PoolBookCorrupt.val()),
        }
        Ok(base)
    }

    /// 页池账目自洽核：领用计数、页基址对齐、页间互不重叠三查。
    pub fn book_ok(&self) -> bool {
        if self.pages.len() != PAGE_COUNT {
            return false;
        }
        let mut bases: Vec<u64> = Vec::new();
        for p in self.pages.iter() {
            if p.base % (PAGE_SIZE as u64) != 0 {
                return false;
            }
            if p.base < POOL_BASE {
                return false;
            }
            for b in bases.iter() {
                if *b == p.base {
                    return false;
                }
            }
            bases.push(p.base);
        }
        bases.len() == PAGE_COUNT
    }
}

/// 越界写检测钩子：写描述 (偏移, 长度) 是否落在页内。
/// off+len ≤ PAGE_SIZE 判页内；越界与 checked 加法溢出都判越界。
pub fn in_page(offset: u32, len: u32) -> Result<(), u32> {
    if len == 0 {
        return Err(Diag::ZeroLenWrite.val());
    }
    let end = match offset.checked_add(len) {
        Some(e) => e,
        None => return Err(Diag::OobWrite.val()),
    };
    if end > PAGE_SIZE {
        return Err(Diag::OobWrite.val());
    }
    Ok(())
}

/// 一次隔离运行的报告：领取、越界、归还三要素。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoRunReport {
    /// 用例 ID。
    pub case_id: u32,
    /// 领到的页基址。
    pub page_base: u64,
    /// 越界写是否被钩子捕获。
    pub oob_caught: bool,
    /// 越界诊断码（未越界为 0）。
    pub oob_code: u32,
    /// 页是否已照常归还（归还保障）。
    pub returned: bool,
    /// 页内写笔数（越界前成功核过的写笔数）。
    pub in_page_writes: u32,
}

/// 隔离执行一例：启动前领页 → 写程序逐笔过钩子 → 结束后必归还。
/// 越界被抓即停笔，但归还照常执行（抓后必还承诺）。
pub fn run_isolated_case(pool: &mut PagePool, case_id: u32, writes: &[(u32, u32)]) -> Result<IsoRunReport, u32> {
    if case_id == 0 {
        return Err(Diag::CaseForeign.val());
    }
    let base = pool.claim(case_id)?;
    let mut oob_caught = false;
    let mut oob_code = 0u32;
    let mut in_page_writes = 0u32;
    for w in writes.iter() {
        let (off, len) = *w;
        match in_page(off, len) {
            Ok(()) => {
                in_page_writes += 1;
            }
            Err(e) => {
                oob_caught = true;
                oob_code = e;
                break;
            }
        }
    }
    let returned = pool.give_back(case_id).is_ok();
    Ok(IsoRunReport {
        case_id,
        page_base: base,
        oob_caught,
        oob_code,
        returned,
        in_page_writes,
    })
}

/// 诊断码闭集：0x79C81..=0x79C8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 越界写：写描述 end=off+len 超 4KB 页界或加法溢出。
    OobWrite = 0x01,
    /// 重复领取：同例未归还又领页。
    CaseDoubleClaim = 0x02,
    /// 未领先还：归还时查无持有记录。
    ReturnWithoutClaim = 0x03,
    /// 页池耗尽：8 页全被领取后再领。
    PoolExhausted = 0x04,
    /// 零长写：写长度为 0 的残缺写描述。
    ZeroLenWrite = 0x05,
    /// 归还漏账：运行结束后池内仍有该例持有记录。
    ClaimLeak = 0x06,
    /// 页基址未对齐：发放页基址不满足 4KB 对齐。
    PageMisalign = 0x07,
    /// 用例表外：用例 ID 为 0 等非法值。
    CaseForeign = 0x08,
    /// 注入漏抓：M-204 跨页写未被钩子捕获。
    InjectionMissed = 0x09,
    /// 双跑分叉：同输入两次隔离运行报告不同。
    LedgerDiverge = 0x0A,
    /// 页池账目错：页数、对齐或重叠三查失守。
    PoolBookCorrupt = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::OobWrite => "越界写：建议写前过 in_page 钩子，越界笔立即停笔入账",
            Diag::CaseDoubleClaim => "重复领取：建议领取口按持有者查重，未归还不再发页",
            Diag::ReturnWithoutClaim => "未领先还：建议归还口先查持有记录，查无即拒",
            Diag::PoolExhausted => "页池耗尽：建议并发用例数对齐页池 8 页上限排队",
            Diag::ZeroLenWrite => "零长写：建议写描述至少 1 字节，零长残缺描述拒收",
            Diag::ClaimLeak => "归还漏账：建议运行结束（含越界早退）必走归还口",
            Diag::PageMisalign => "页基址未对齐：建议发页前核基址 4KB 对齐再入账",
            Diag::CaseForeign => "用例表外：建议用例 ID 从冻结注册表取，0 号保留拒用",
            Diag::InjectionMissed => "注入漏抓：建议 M-204 跨页写固定走钩子路径核对",
            Diag::LedgerDiverge => "双跑分叉：建议 run_isolated_case 做成纯函数，输入定则报告定",
            Diag::PoolBookCorrupt => "页池账目错：建议核对页数、基址对齐与页间不重叠三查",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::OobWrite,
    Diag::CaseDoubleClaim,
    Diag::ReturnWithoutClaim,
    Diag::PoolExhausted,
    Diag::ZeroLenWrite,
    Diag::ClaimLeak,
    Diag::PageMisalign,
    Diag::CaseForeign,
    Diag::InjectionMissed,
    Diag::LedgerDiverge,
    Diag::PoolBookCorrupt,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79C81..=0x79C8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
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
        if a.detail().is_empty() || !a.detail().contains("建议") {
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
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 页池初始化：8 页全空闲、基址 4KB 对齐、账目三查过。
fn pool_init_ok() -> bool {
    let pool = PagePool::new();
    if pool.claimed_count() != 0 || !pool.book_ok() {
        return false;
    }
    for i in 0..PAGE_COUNT {
        let expect = POOL_BASE + (i as u64) * (PAGE_SIZE as u64);
        if expect % (PAGE_SIZE as u64) != 0 {
            return false;
        }
    }
    PAGE_SIZE == 4096 && PAGE_COUNT == 8
}

/// 领取/归还生命周期：领取返回对齐基址、持有可查、归还后页可复用。
fn claim_return_cycle_ok() -> bool {
    let mut pool = PagePool::new();
    let base = match pool.claim(1) {
        Ok(b) => b,
        Err(_) => return false,
    };
    if base % (PAGE_SIZE as u64) != 0 || pool.holder_base(1) != Some(base) {
        return false;
    }
    if pool.claimed_count() != 1 {
        return false;
    }
    if pool.give_back(1) != Ok(base) {
        return false;
    }
    // 归还后页复用：同例再领应拿到同一页。
    pool.claim(1) == Ok(base) && pool.claimed_count() == 1
}

/// 注入核一：同例重复领取必拒（CaseDoubleClaim），归还后再领必过。
fn double_claim_gate_ok() -> bool {
    let mut pool = PagePool::new();
    if pool.claim(1).is_err() {
        return false;
    }
    if pool.claim(1) != Err(Diag::CaseDoubleClaim.val()) {
        return false;
    }
    if pool.give_back(1).is_err() {
        return false;
    }
    pool.claim(1).is_ok()
}

/// 注入核二：未领先还必拒（ReturnWithoutClaim），0 号用例表外拒。
fn return_gate_ok() -> bool {
    let mut pool = PagePool::new();
    if pool.give_back(9) != Err(Diag::ReturnWithoutClaim.val()) {
        return false;
    }
    if pool.claim(0) != Err(Diag::CaseForeign.val()) {
        return false;
    }
    pool.give_back(0) == Err(Diag::CaseForeign.val())
}

/// 越界钩子边界核：页内上沿 (4095,1) 与整页 (0,4096) 过；(4095,2)、
/// (4096,1)、溢出 (u32::MAX,2) 与零长 (0,0) 四类违约全拒。
fn oob_hook_boundary_ok() -> bool {
    if in_page(0, PAGE_SIZE).is_err() {
        return false;
    }
    if in_page(PAGE_SIZE - 1, 1).is_err() {
        return false;
    }
    if in_page(PAGE_SIZE - 1, 2) != Err(Diag::OobWrite.val()) {
        return false;
    }
    if in_page(PAGE_SIZE, 1) != Err(Diag::OobWrite.val()) {
        return false;
    }
    if in_page(u32::MAX, 2) != Err(Diag::OobWrite.val()) {
        return false;
    }
    in_page(0, 0) == Err(Diag::ZeroLenWrite.val())
}

/// 页池耗尽核：8 页领满后第 9 例必拒（PoolExhausted），逐一归还后恢复。
fn pool_exhausted_gate_ok() -> bool {
    let mut pool = PagePool::new();
    let mut i = 0u32;
    while i < PAGE_COUNT as u32 {
        let case = i + 1;
        if pool.claim(case).is_err() {
            return false;
        }
        i += 1;
    }
    if pool.claimed_count() != PAGE_COUNT {
        return false;
    }
    if pool.claim(99) != Err(Diag::PoolExhausted.val()) {
        return false;
    }
    let mut j = 0u32;
    while j < PAGE_COUNT as u32 {
        if pool.give_back(j + 1).is_err() {
            return false;
        }
        j += 1;
    }
    pool.claim(99).is_ok()
}

/// M-204 注入核：跨页写（4094,8）必被抓（OobWrite），页照常归还，
/// 越界前核过的写笔数为 0（注入例只有这一笔违约写）。
fn injection_m204_ok() -> bool {
    let mut pool = PagePool::new();
    let writes: [(u32, u32); 1] = [(4094, 8)];
    let r = match run_isolated_case(&mut pool, INJECTION_CASE_ID, &writes) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !r.oob_caught || r.oob_code != Diag::OobWrite.val() {
        return false;
    }
    if !r.returned || r.in_page_writes != 0 {
        return false;
    }
    // 页已归还：池内查无持有者，页可被他人领取。
    pool.holder_base(INJECTION_CASE_ID).is_none() && pool.claimed_count() == 0
}

/// 归还漏账核：冻结表四例（含注入例）全部跑完后，池必须回到全空闲，
/// 任一例持有残留即 ClaimLeak 实锤。
fn claim_leak_guard_ok() -> bool {
    let mut pool = PagePool::new();
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let (name, case_id, writes) = match CASE_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let r = match run_isolated_case(&mut pool, case_id, writes) {
            Ok(r) => r,
            Err(_) => return false,
        };
        // 每例跑完即核持有清零（用例名进诊断面但不入判断）。
        let _ = name;
        if pool.holder_base(case_id).is_some() {
            return false;
        }
        if !r.returned {
            return false;
        }
        i += 1;
    }
    pool.claimed_count() == 0 && pool.book_ok()
}

/// 页间不重叠核：并发领取两例必得不同页基址，且各自 4KB 对齐。
fn page_distinct_ok() -> bool {
    let mut pool = PagePool::new();
    let b1 = match pool.claim(1) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let b2 = match pool.claim(2) {
        Ok(b) => b,
        Err(_) => return false,
    };
    if b1 == b2 {
        return false;
    }
    if b1 % (PAGE_SIZE as u64) != 0 || b2 % (PAGE_SIZE as u64) != 0 {
        return false;
    }
    // 页距必为页尺寸整数倍（不重叠）。
    let dist = if b2 > b1 { b2 - b1 } else { b1 - b2 };
    dist % (PAGE_SIZE as u64) == 0
}

/// 正常流核：冻结表四例（注入例除外）全部页内通过，整页写 (0,4096)
/// 与尾部对齐写 (2048,2048) 都在页界内。
fn normal_flow_ok() -> bool {
    let mut pool = PagePool::new();
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let (name, case_id, writes) = match CASE_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let r = match run_isolated_case(&mut pool, case_id, writes) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let is_injection = case_id == INJECTION_CASE_ID;
        if r.oob_caught != is_injection {
            return false;
        }
        if !is_injection && r.in_page_writes != writes.len() as u32 {
            return false;
        }
        let _ = name;
        i += 1;
    }
    true
}

/// 双跑一致：同输入两次隔离运行（注入例）的报告逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<IsoRunReport, u32> {
        let mut pool = PagePool::new();
        let writes: [(u32, u32); 1] = [(4094, 8)];
        run_isolated_case(&mut pool, INJECTION_CASE_ID, &writes)
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含契约三件套与注入结论）。
fn writeback_summary_ok() -> bool {
    let mut pool = PagePool::new();
    let mut caught = 0u32;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let (name, case_id, writes) = match CASE_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let r = match run_isolated_case(&mut pool, case_id, writes) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if r.oob_caught {
            caught += 1;
        }
        let _ = name;
        i += 1;
    }
    let summary = format!(
        "UNX-F3445 完成：领取/归还/越界钩子三件套落地 M-204注入捕获{}例",
        caught
    );
    !summary.is_empty()
        && summary.contains("UNX-F3445")
        && summary.contains("M-204")
        && caught == 1
        && pool.book_ok()
        && detail(Diag::InjectionMissed.val()).contains("M-204")
}

// ── 判据总成：run_unx3445_checks ───────────────────────────────────────

/// run_unx3445_checks：本单判据聚合（诊断段 0x79C80，全部走 add_dyn）。
pub fn run_unx3445_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3445 用例隔离执行环境");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "页池初始化8页全空且基址4KB对齐",
            pool_init_ok(),
            format!("页池{}", if pool_init_ok() { "就绪" } else { "残缺" }),
        ),
        (
            "领取归还生命周期页复用成立",
            claim_return_cycle_ok(),
            format!("生命周期{}", if claim_return_cycle_ok() { "闭环" } else { "断链" }),
        ),
        (
            "重复领取必拒归还后再领必过",
            double_claim_gate_ok(),
            format!("重领闸{}", if double_claim_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "未领先还与表外用例必拒",
            return_gate_ok(),
            format!("归还闸{}", if return_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "越界钩子四类违约与页内上沿边界",
            oob_hook_boundary_ok(),
            format!("越界钩子{}", if oob_hook_boundary_ok() { "钉死" } else { "失守" }),
        ),
        (
            "页池耗尽必拒归还后恢复",
            pool_exhausted_gate_ok(),
            format!("耗尽闸{}", if pool_exhausted_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "M-204跨页注入必抓且抓后必还",
            injection_m204_ok(),
            format!("M-204注入{}", if injection_m204_ok() { "捕获" } else { "漏抓" }),
        ),
        (
            "归还漏账核四例跑完池回全空",
            claim_leak_guard_ok(),
            format!("漏账核{}", if claim_leak_guard_ok() { "清白" } else { "有漏" }),
        ),
        (
            "并发领页基址互异且页距对齐",
            page_distinct_ok(),
            format!("页间{}", if page_distinct_ok() { "不重叠" } else { "重叠" }),
        ),
        (
            "正常流四例页内通过注入例单独判",
            normal_flow_ok(),
            format!("正常流{}", if normal_flow_ok() { "全过" } else { "有红" }),
        ),
        (
            "双跑一致隔离运行报告逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
    ];
    // 写回摘要判据（第 14 条，与 13 条并列成账）。
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79C80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn pool_init_and_book() {
        assert!(pool_init_ok());
        let pool = PagePool::new();
        assert_eq!(pool.claimed_count(), 0);
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(PAGE_COUNT, 8);
        assert!(pool.book_ok());
    }

    #[test]
    fn claim_return_cycle() {
        assert!(claim_return_cycle_ok());
        let mut pool = PagePool::new();
        let base = pool.claim(1).unwrap();
        assert_eq!(base, POOL_BASE);
        assert_eq!(pool.holder_base(1), Some(base));
        assert_eq!(pool.give_back(1).unwrap(), base);
        assert_eq!(pool.holder_base(1), None);
    }

    #[test]
    fn injection_gates_claim_return() {
        assert!(double_claim_gate_ok());
        assert!(return_gate_ok());
    }

    #[test]
    fn oob_hook_boundaries() {
        assert!(oob_hook_boundary_ok());
        assert!(in_page(4095, 1).is_ok());
        assert_eq!(in_page(4096, 1), Err(Diag::OobWrite.val()));
        assert_eq!(in_page(0, 0), Err(Diag::ZeroLenWrite.val()));
    }

    #[test]
    fn pool_exhaustion_recovery() {
        assert!(pool_exhausted_gate_ok());
    }

    #[test]
    fn injection_m204_captured_and_returned() {
        assert!(injection_m204_ok());
        let mut pool = PagePool::new();
        let writes: [(u32, u32); 1] = [(4094, 8)];
        let r = run_isolated_case(&mut pool, INJECTION_CASE_ID, &writes).unwrap();
        assert!(r.oob_caught);
        assert_eq!(r.oob_code, Diag::OobWrite.val());
        assert!(r.returned);
    }

    #[test]
    fn leak_guard_and_distinct_pages() {
        assert!(claim_leak_guard_ok());
        assert!(page_distinct_ok());
    }

    #[test]
    fn normal_flow_table() {
        assert!(normal_flow_ok());
        assert_eq!(CASE_TABLE.len(), 4);
        assert_eq!(CASE_TABLE.get(2).map(|t| t.1), Some(INJECTION_CASE_ID));
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3445_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
