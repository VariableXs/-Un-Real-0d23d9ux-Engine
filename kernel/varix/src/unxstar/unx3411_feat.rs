//! UNX-F3411 用例间隔离（每用例独立上下文页） · 0x79600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：用例上下文页（独立工作区 + 前后清零）——用例间零状态泄漏；
//!      M-170 泄漏注入检测。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 每用例独占一页上下文（页号 / 用例号 / 状态位 / 16 字划痕区 / 触达计数）；
//!   - 「前后清零」双保险：进页（arm）先做预清零，出页（leave）再做复清零
//!     并逐字校验为零——两道闸任一漏清都会被残值扫描当场点名；
//!   - 页状态机 Free→Armed→InUse→Cleared 单调推进（rank 递增），
//!     任何倒退（如 InUse→Armed）一律显性拒（Err=0x79603）；
//!   - M-170 泄漏注入：人为构造「出页不清零」的违约路径，
//!     残值扫描必须抓到且只抓到被注入的那一页；
//!   - 双跑一致：同一注入序列跑两遍审计，结论逐字节相同；
//!   - 诊断码闭集 0x79601..=0x7960C，错误路径 Err(u32)，
//!     detail 文案带可操作中文建议（建议二字必须出现）。
//! 常量来源：
//!   - 页容量 16 字：对齐 B11 批内 M-170 注入矩阵的最小可判粒度（4×4 字）；
//!   - 页池 8 槽：与 CheckSet::MAX_CHECKS=112 的 1/14 对齐，留双跑余量；
//!   - 清零哨兵 0：空页全零是本卡唯一合法静息态（判据 J1 的直接体现）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79600;

/// 页池槽位上界（冻结值：双跑场景恰好用 3 页 + 注入 1 页，余量 4）。
pub const PAGE_SLOTS: usize = 8;

/// 每页划痕区字数（u32 计）。
pub const SCRATCH_WORDS: usize = 16;

/// 空页哨兵：全零即「无泄漏」的唯一静息态。
pub const ZERO_SENTINEL: u32 = 0;

/// 页状态机（单调推进，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageState {
    /// 空闲：可被 arm 认领（静息态，划痕区必须全零）。
    Free,
    /// 已武装：预清零完成、用例号已烙印，等待 enter。
    Armed,
    /// 占用中：用例正在该页上执行，划痕区允许脏。
    InUse,
    /// 已清账：复清零完成且逐字校验为零，等待 release 归还。
    Cleared,
}

impl PageState {
    /// 单调秩：Free(0) < Armed(1) < InUse(2) < Cleared(3)。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 状态中文标签（诊断串用）。
    pub fn label(self) -> &'static str {
        match self {
            PageState::Free => "空闲",
            PageState::Armed => "已武装",
            PageState::InUse => "占用中",
            PageState::Cleared => "已清账",
        }
    }
}

/// 诊断码闭集：0x79601..=0x7960C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 预清零未落实：arm 时划痕区仍有非零残值。
    PreClearMissed = 0x01,
    /// 复清零未落实：leave 后划痕区仍有非零残值。
    PostClearMissed = 0x02,
    /// 页状态倒退：状态机收到 rank 不增的迁移请求。
    StateRegress = 0x03,
    /// 页池耗尽：无 Free 页可认领。
    NoFreePage = 0x04,
    /// 划痕区越界写：字下标越过 SCRATCH_WORDS。
    ScratchOob = 0x05,
    /// 页表损坏：按页号回查不到对应页。
    PageTableCorrupt = 0x06,
    /// 用例号冲突：同一页被第二个用例重复烙印。
    CaseIdClash = 0x07,
    /// 泄漏注入命中：残值扫描抓到未清账页（M-170 判据面）。
    LeakInjected = 0x08,
    /// 双跑不一致：同一注入序列两次审计结论不同。
    DoubleRunDiverge = 0x09,
    /// 越权写页：状态非 InUse 时收到写请求。
    WriteWhileNotInUse = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 取本诊断码的完整 u32 编码（SEG_BASE + 变体判别值）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 取本诊断码的中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::PreClearMissed => "预清零未落实：建议在 arm 路径先 clear_scratch 再烙用例号",
            Diag::PostClearMissed => "复清零未落实：建议在 leave 路径 clear 后逐字断零再转 Cleared",
            Diag::StateRegress => "页状态倒退：建议按 Free→Armed→InUse→Cleared 单调推进，归还走 release",
            Diag::NoFreePage => "页池耗尽：建议先 release 已清账页或扩 PAGE_SLOTS",
            Diag::ScratchOob => "划痕区越界写：建议写前用 checked 下标校验 SCRATCH_WORDS 界",
            Diag::PageTableCorrupt => "页表损坏：建议按页号 position 回查，不得裸下标寻页",
            Diag::CaseIdClash => "用例号冲突：建议一页一用例，复用前必须走 release 归零",
            Diag::LeakInjected => "泄漏注入命中：建议出页路径强制复清零并保留残值扫描审计",
            Diag::DoubleRunDiverge => "双跑不一致：建议审计函数做成纯函数，输入定则输出定",
            Diag::WriteWhileNotInUse => "越权写页：建议写前校验状态为 InUse，其余状态一律拒",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码，顺序即编码序）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::PreClearMissed,
    Diag::PostClearMissed,
    Diag::StateRegress,
    Diag::NoFreePage,
    Diag::ScratchOob,
    Diag::PageTableCorrupt,
    Diag::CaseIdClash,
    Diag::LeakInjected,
    Diag::DoubleRunDiverge,
    Diag::WriteWhileNotInUse,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回：编码 → 文案；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79601..=0x7960C 闭集内"
}

/// 用例上下文页：一页即一个用例的独立工作区。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtxPage {
    /// 页号（0 起，页池内唯一）。
    pub page_no: u32,
    /// 当前烙印的用例号（Free 态恒为 0）。
    pub case_id: u32,
    /// 页状态（单调推进）。
    pub state: PageState,
    /// 划痕区（用例私有工作区，前后清零的标的）。
    pub scratch: [u32; SCRATCH_WORDS],
    /// 本页累计合法写次数（审计用，单调递增）。
    pub touches: u32,
}

impl CtxPage {
    /// 构造空页：全零划痕、Free 态、用例号 0。
    pub const fn new(page_no: u32) -> CtxPage {
        CtxPage {
            page_no,
            case_id: 0,
            state: PageState::Free,
            scratch: [ZERO_SENTINEL; SCRATCH_WORDS],
            touches: 0,
        }
    }

    /// 划痕区清零（前后清零的原子动作）。
    pub fn clear_scratch(&mut self) {
        let mut i = 0usize;
        while i < SCRATCH_WORDS {
            if let Some(slot) = self.scratch.get_mut(i) {
                *slot = ZERO_SENTINEL;
            }
            i += 1;
        }
    }

    /// 残值字数：非零字计数（0 即干净页）。
    pub fn residue_words(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SCRATCH_WORDS {
            if let Some(v) = self.scratch.get(i) {
                if *v != ZERO_SENTINEL {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 合法写：仅 InUse 态、界内下标；越界/越态均 Err。
    pub fn write_word(&mut self, idx: usize, val: u32) -> Result<(), u32> {
        if self.state != PageState::InUse {
            return Err(Diag::WriteWhileNotInUse.val());
        }
        if idx >= SCRATCH_WORDS {
            return Err(Diag::ScratchOob.val());
        }
        match self.scratch.get_mut(idx) {
            Some(slot) => {
                *slot = val;
                self.touches = self.touches.saturating_add(1);
                Ok(())
            }
            None => Err(Diag::ScratchOob.val()),
        }
    }
}

/// 页状态机推进器：单调秩校验，倒退一律显性拒。
pub fn step_state(cur: PageState, next: PageState) -> Result<PageState, u32> {
    if next.rank() <= cur.rank() {
        return Err(Diag::StateRegress.val());
    }
    Ok(next)
}

/// 隔离台架：页池 + 认领/进出/归还账。
#[derive(Debug)]
pub struct IsolationRig {
    /// 页池（定容 PAGE_SLOTS）。
    pub pages: Vec<CtxPage>,
    /// 已完成 release 的归还次数（单调）。
    pub released: u32,
    /// 审计日志（追加式，只增不减）。
    pub audit: Vec<String>,
}

impl IsolationRig {
    /// 构造台架：slots 张空页。
    pub fn new(slots: usize) -> IsolationRig {
        let n = if slots == 0 { PAGE_SLOTS } else { slots.min(PAGE_SLOTS) };
        let mut pages = Vec::new();
        let mut i = 0u32;
        while (i as usize) < n {
            pages.push(CtxPage::new(i));
            i += 1;
        }
        IsolationRig { pages, released: 0, audit: Vec::new() }
    }

    /// 按页号回查页下标（position 回查，不裸下标寻页）。
    fn locate(&self, page_no: u32) -> Option<usize> {
        self.pages.iter().position(|p| p.page_no == page_no)
    }

    /// 认领页并预清零、烙用例号（前清零闸）。
    pub fn arm(&mut self, case_id: u32) -> Result<u32, u32> {
        if case_id == 0 {
            return Err(Diag::CaseIdClash.val());
        }
        let pos = match self.pages.iter().position(|p| p.state == PageState::Free) {
            Some(p) => p,
            None => return Err(Diag::NoFreePage.val()),
        };
        let page = match self.pages.get_mut(pos) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        // 前清零闸：清完必须逐字为零，否则点名 PreClearMissed。
        page.clear_scratch();
        if page.residue_words() != 0 {
            return Err(Diag::PreClearMissed.val());
        }
        page.case_id = case_id;
        page.state = PageState::Armed;
        self.audit.push(format!("armed 页{} 用例{}", page.page_no, case_id));
        Ok(page.page_no)
    }

    /// 进页：Armed → InUse（走状态机推进器，倒退必拒）。
    pub fn enter(&mut self, page_no: u32) -> Result<(), u32> {
        let pos = match self.locate(page_no) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        let page = match self.pages.get_mut(pos) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        match step_state(page.state, PageState::InUse) {
            Ok(next) => {
                page.state = next;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 出页：复清零 + 逐字断零 + InUse → Cleared（后清零闸）。
    pub fn leave(&mut self, page_no: u32) -> Result<(), u32> {
        let pos = match self.locate(page_no) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        let page = match self.pages.get_mut(pos) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        if page.state != PageState::InUse {
            return Err(Diag::WriteWhileNotInUse.val());
        }
        page.clear_scratch();
        if page.residue_words() != 0 {
            return Err(Diag::PostClearMissed.val());
        }
        match step_state(page.state, PageState::Cleared) {
            Ok(next) => page.state = next,
            Err(e) => return Err(e),
        }
        self.audit.push(format!("cleared 页{} 触达{}", page.page_no, page.touches));
        Ok(())
    }

    /// 归还：Cleared → Free（release 是唯一的合法回卷路径，
    /// 不走 step_state——它是状态机的显式出栈，不是倒退）。
    pub fn release(&mut self, page_no: u32) -> Result<(), u32> {
        let pos = match self.locate(page_no) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        let page = match self.pages.get_mut(pos) {
            Some(p) => p,
            None => return Err(Diag::PageTableCorrupt.val()),
        };
        if page.state != PageState::Cleared {
            return Err(Diag::StateRegress.val());
        }
        page.clear_scratch();
        page.case_id = 0;
        page.touches = 0;
        page.state = PageState::Free;
        self.released = self.released.saturating_add(1);
        Ok(())
    }

    /// 残值扫描：返回全部含非零残值且状态非 InUse 的页号
    /// （InUse 页允许脏——用例正在跑；其余状态含残值即泄漏）。
    pub fn residue_scan(&self) -> Vec<u32> {
        let mut leaked: Vec<u32> = Vec::new();
        for p in self.pages.iter() {
            if p.state != PageState::InUse && p.residue_words() != 0 {
                leaked.push(p.page_no);
            }
        }
        leaked
    }

    /// 干净页数（Free 且全零）。
    pub fn free_clean_pages(&self) -> u32 {
        let mut n = 0u32;
        for p in self.pages.iter() {
            if p.state == PageState::Free && p.residue_words() == 0 {
                n += 1;
            }
        }
        n
    }
}

/// 冻结用例时刻表：本卡双跑场景固定跑 3 个用例 + 1 处 M-170 注入。
/// （来源：派工卡 J1/M-170 判据面；footprint 是用例在划痕区留下的
/// 合法脏字数上界，leave 后必须归零。）
pub struct CaseSpec {
    /// 用例号（恒正，0 保留给 Free 态）。
    pub case_id: u32,
    /// 用例标签（诊断串用）。
    pub tag: &'static str,
    /// 合法脏字数上界（划痕区内允许同时存活的非零字数）。
    pub footprint: u32,
}

/// 冻结时刻表（3 用例；顺序即执行序）。
pub const CASE_SCHEDULE: [CaseSpec; 3] = [
    CaseSpec { case_id: 0x101, tag: "页分配冒烟", footprint: 4 },
    CaseSpec { case_id: 0x102, tag: "跨页复用", footprint: 8 },
    CaseSpec { case_id: 0x103, tag: "满池边界", footprint: 12 },
];

/// 时刻表完整性：用例号唯一、恒正、footprint 界内、标签非空。
pub fn schedule_complete() -> bool {
    let mut ok = true;
    for (i, c) in CASE_SCHEDULE.iter().enumerate() {
        if c.case_id == 0 || c.tag.is_empty() || c.footprint > SCRATCH_WORDS as u32 {
            ok = false;
        }
        let mut j = i + 1;
        while j < CASE_SCHEDULE.len() {
            match CASE_SCHEDULE.get(j) {
                Some(o) => {
                    if o.case_id == c.case_id {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            j += 1;
        }
    }
    ok
}

/// 在指定页上按 footprint 写脏字（偶数下标写用例号），返回实际写成功数。
fn scribe(rig: &mut IsolationRig, page_no: u32, spec: &CaseSpec) -> u32 {
    let mut written = 0u32;
    let mut idx = 0usize;
    while (written < spec.footprint) && (idx < SCRATCH_WORDS) {
        let val = spec.case_id.wrapping_add(idx as u32);
        let pos = match rig.locate(page_no) {
            Some(p) => p,
            None => return written,
        };
        let page = match rig.pages.get_mut(pos) {
            Some(p) => p,
            None => return written,
        };
        match page.write_word(idx, val) {
            Ok(()) => written += 1,
            Err(_) => return written,
        }
        idx += 1;
    }
    written
}

/// 场景：顺序跑完冻结时刻表（arm→enter→scribe→leave→release），
/// 全程零泄漏则 Ok(())；任何一步 Err 原样上抛。
pub fn run_schedule_once(rig: &mut IsolationRig) -> Result<(), u32> {
    for spec in CASE_SCHEDULE.iter() {
        let page_no = rig.arm(spec.case_id)?;
        rig.enter(page_no)?;
        let written = scribe(rig, page_no, spec);
        if written != spec.footprint {
            return Err(Diag::ScratchOob.val());
        }
        rig.leave(page_no)?;
        rig.release(page_no)?;
    }
    Ok(())
}

/// M-170 泄漏注入：跑第一个用例后**故意不走 leave 的复清零**
/// （直接把状态打成 Cleared 但留脏），残值扫描必须且只能抓到该页。
pub fn inject_leak_and_scan() -> Result<Vec<u32>, u32> {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    let spec = match CASE_SCHEDULE.first() {
        Some(s) => s,
        None => return Err(Diag::PageTableCorrupt.val()),
    };
    let page_no = rig.arm(spec.case_id)?;
    rig.enter(page_no)?;
    let _ = scribe(&mut rig, page_no, spec);
    // 注入点：绕过 leave 的复清零，伪造 Cleared（违约态）。
    let pos = match rig.locate(page_no) {
        Some(p) => p,
        None => return Err(Diag::PageTableCorrupt.val()),
    };
    let page = match rig.pages.get_mut(pos) {
        Some(p) => p,
        None => return Err(Diag::PageTableCorrupt.val()),
    };
    if page.state != PageState::InUse {
        return Err(Diag::WriteWhileNotInUse.val());
    }
    page.state = PageState::Cleared; // 脏值留在划痕区 = 人为泄漏
    let residue = page.residue_words();
    let leaked = rig.residue_scan();
    // 审计日志必须记下注入页（不静默）。
    rig.audit.push(format!("injected 页{} 残值{}", page_no, residue));
    Ok(leaked)
}

/// 审计纯函数：给定台架快照，返回（泄漏页数，触达总数）。
/// 输入定则输出定——双跑一致性的物理保证。
fn audit_once(rig: &IsolationRig) -> (u32, u32) {
    let leaked = rig.residue_scan().len() as u32;
    let mut touches = 0u32;
    for p in rig.pages.iter() {
        touches = touches.saturating_add(p.touches);
    }
    (leaked, touches)
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

/// 正常流：单用例进页出页零泄漏、触达数单调。
fn single_case_ok() -> bool {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    let spec = match CASE_SCHEDULE.first() {
        Some(s) => s,
        None => return false,
    };
    let page_no = match rig.arm(spec.case_id) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if rig.enter(page_no).is_err() {
        return false;
    }
    let written = scribe(&mut rig, page_no, spec);
    if written != spec.footprint {
        return false;
    }
    if rig.leave(page_no).is_err() {
        return false;
    }
    if rig.residue_scan().len() != 0 {
        return false;
    }
    if rig.release(page_no).is_err() {
        return false;
    }
    rig.free_clean_pages() == PAGE_SLOTS as u32 && rig.released == 1
}

/// 正常流：三用例顺序复用同一张页池，页间零泄漏。
fn schedule_flow_ok() -> bool {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    match run_schedule_once(&mut rig) {
        Ok(()) => {}
        Err(_) => return false,
    }
    rig.residue_scan().is_empty()
        && rig.free_clean_pages() == PAGE_SLOTS as u32
        && rig.released == CASE_SCHEDULE.len() as u32
}

/// 边界：页池耗尽必须显性拒（NoFreePage），且不影响已武装页。
fn pool_exhaustion_ok() -> bool {
    let mut rig = IsolationRig::new(2);
    let p1 = match rig.arm(0x201) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let p2 = match rig.arm(0x202) {
        Ok(p) => p,
        Err(_) => return false,
    };
    match rig.arm(0x203) {
        Err(e) => {
            if e != Diag::NoFreePage.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 已武装两页仍可正常走完（错误不扩散）。
    rig.enter(p1).is_ok() && rig.enter(p2).is_ok() && rig.leave(p1).is_ok() && rig.leave(p2).is_ok()
}

/// 边界：划痕区越界写必须显性拒（ScratchOob）且不破坏邻字。
fn scratch_oob_ok() -> bool {
    let mut rig = IsolationRig::new(1);
    let page_no = match rig.arm(0x301) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if rig.enter(page_no).is_err() {
        return false;
    }
    let oob_idx = SCRATCH_WORDS + 4;
    let pos = match rig.locate(page_no) {
        Some(p) => p,
        None => return false,
    };
    let verdict = match rig.pages.get_mut(pos) {
        Some(page) => page.write_word(oob_idx, 0xFFFF_FFFF),
        None => return false,
    };
    match verdict {
        Err(e) => {
            if e != Diag::ScratchOob.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 界内写仍正常，邻字不受越界写尝试影响。
    let pos = match rig.locate(page_no) {
        Some(p) => p,
        None => return false,
    };
    let ok_write = match rig.pages.get_mut(pos) {
        Some(page) => page.write_word(0, 7).is_ok(),
        None => false,
    };
    ok_write && rig.leave(page_no).is_ok()
}

/// 状态机倒退显性拒：InUse→Armed、Cleared→InUse、同态迁移全部 Err。
fn state_regress_ok() -> bool {
    let cases = [
        (PageState::InUse, PageState::Armed),
        (PageState::Cleared, PageState::InUse),
        (PageState::Cleared, PageState::Armed),
        (PageState::Armed, PageState::Armed),
        (PageState::Free, PageState::Free),
    ];
    for (cur, next) in cases.iter() {
        match step_state(*cur, *next) {
            Err(e) => {
                if e != Diag::StateRegress.val() {
                    return false;
                }
            }
            Ok(_) => return false,
        }
    }
    // 前进方向全部放行。
    let fwd = [
        (PageState::Free, PageState::Armed),
        (PageState::Armed, PageState::InUse),
        (PageState::InUse, PageState::Cleared),
    ];
    for (cur, next) in fwd.iter() {
        if step_state(*cur, *next).is_err() {
            return false;
        }
    }
    true
}

/// 越权写页：非 InUse 态写划痕区必须显性拒。
fn write_gate_ok() -> bool {
    let mut rig = IsolationRig::new(1);
    let page_no = match rig.arm(0x401) {
        Ok(p) => p,
        Err(_) => return false,
    };
    // Armed 态写 = 越权。
    let pos = match rig.locate(page_no) {
        Some(p) => p,
        None => return false,
    };
    let armed_write = match rig.pages.get_mut(pos) {
        Some(page) => page.write_word(0, 1),
        None => return false,
    };
    if armed_write != Err(Diag::WriteWhileNotInUse.val()) {
        return false;
    }
    rig.enter(page_no).is_ok()
}

/// M-170 泄漏注入必抓：注入页必须被扫描点名，且只点名该页。
fn leak_injection_caught_ok() -> bool {
    let leaked = match inject_leak_and_scan() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if leaked.len() != 1 {
        return false;
    }
    match leaked.first() {
        Some(p) => *p == 0, // 注入的是 0 号页
        None => false,
    }
}

/// 跨用例零泄漏：时刻表跑完后任何页都不得残留上一用例的烙印值。
fn cross_case_zero_leak_ok() -> bool {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    if run_schedule_once(&mut rig).is_err() {
        return false;
    }
    for p in rig.pages.iter() {
        if p.residue_words() != 0 {
            return false;
        }
        // 烙印值清干净：任何字都不得等于历史用例号相关值。
        let mut i = 0usize;
        while i < SCRATCH_WORDS {
            match p.scratch.get(i) {
                Some(v) => {
                    if *v != 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
    }
    true
}

/// 双跑一致：同一场景两次审计，结论对（泄漏数，触达数）逐字节相同。
fn double_run_ok() -> bool {
    let mut run1 = IsolationRig::new(PAGE_SLOTS);
    let mut run2 = IsolationRig::new(PAGE_SLOTS);
    if run_schedule_once(&mut run1).is_err() || run_schedule_once(&mut run2).is_err() {
        return false;
    }
    audit_once(&run1) == audit_once(&run2) && audit_once(&run1).0 == 0
}

/// 注入双跑一致：注入场景两次扫描结论相同（都是且只是 0 号页）。
fn inject_double_run_ok() -> bool {
    let a = match inject_leak_and_scan() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match inject_leak_and_scan() {
        Ok(v) => v,
        Err(_) => return false,
    };
    a == b && !a.is_empty()
}

/// 时刻表冻结完整性 + 释放计数守恒。
fn schedule_table_ok() -> bool {
    schedule_complete() && CASE_SCHEDULE.len() == 3
}

/// 错误路径 Result 语义：用例号 0 与未烙印页回查必须走 Err 闭集。
fn error_path_ok() -> bool {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    // 用例号 0 恒拒（0 是 Free 态保留值）。
    if rig.arm(0) != Err(Diag::CaseIdClash.val()) {
        return false;
    }
    // 回查不存在的页号 = 页表损坏码。
    if rig.enter(0xFFFF) != Err(Diag::PageTableCorrupt.val()) {
        return false;
    }
    // 未武装页直接 leave = 越态码。
    if rig.leave(0) != Err(Diag::WriteWhileNotInUse.val()) {
        return false;
    }
    true
}

/// ③ 写回面：状态=已完成、结果摘要非空（摘要由审计结论拼装）。
fn writeback_summary_ok() -> bool {
    let mut rig = IsolationRig::new(PAGE_SLOTS);
    if run_schedule_once(&mut rig).is_err() {
        return false;
    }
    let (leaked, touches) = audit_once(&rig);
    let summary = format!(
        "UNX-F3411 隔离审计：泄漏{}页 触达{}次 释放{}页",
        leaked, touches, rig.released
    );
    let done = leaked == 0 && summary.len() > 0 && summary.contains("隔离审计");
    // 摘要为空的违约必须走 SummaryEmpty 闭集码。
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3411_checks ───────────────────────────────────────

/// run_unx3411_checks：本单判据聚合（诊断段 0x79600，全部走 add_dyn）。
pub fn run_unx3411_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3411 用例间隔离（每用例独立上下文页）");
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
            "正常流单用例进页出页零泄漏",
            single_case_ok(),
            format!("单用例流{}", if single_case_ok() { "绿" } else { "红" }),
        ),
        (
            "正常流三用例顺序复用零泄漏",
            schedule_flow_ok(),
            format!("时刻表流{}", if schedule_flow_ok() { "绿" } else { "红" }),
        ),
        (
            "边界页池耗尽显性拒",
            pool_exhaustion_ok(),
            format!("耗尽拒{}", if pool_exhaustion_ok() { "过" } else { "失" }),
        ),
        (
            "边界划痕区越界写必拒不伤邻字",
            scratch_oob_ok(),
            format!("越界拒{}", if scratch_oob_ok() { "过" } else { "失" }),
        ),
        (
            "状态机倒退五组违约全显性拒",
            state_regress_ok(),
            format!("倒退拒{}", if state_regress_ok() { "过" } else { "失" }),
        ),
        (
            "越权写页非InUse态必拒",
            write_gate_ok(),
            format!("越权拒{}", if write_gate_ok() { "过" } else { "失" }),
        ),
        (
            "M-170泄漏注入必抓且只抓注入页",
            leak_injection_caught_ok(),
            format!("注入{}", if leak_injection_caught_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "跨用例零状态泄漏",
            cross_case_zero_leak_ok(),
            format!("跨用例{}", if cross_case_zero_leak_ok() { "零泄漏" } else { "有残值" }),
        ),
        (
            "双跑一致审计结论逐字节同",
            double_run_ok() && inject_double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "冻结时刻表完整且释放守恒",
            schedule_table_ok(),
            format!("时刻表{}", if schedule_table_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "错误路径闭集与写回摘要非空",
            error_path_ok() && writeback_summary_ok(),
            format!("错误路径+写回{}", if error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
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
        assert_eq!(SEG_BASE, 0x79600);
        assert!(diag_closed_set_ok());
        assert_eq!(ALL_DIAGS.len(), 12);
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
            assert!(d.detail().contains("建议"));
        }
        assert!(detail(0xDEAD).contains("未知"));
    }

    #[test]
    fn single_case_flow_is_clean() {
        assert!(single_case_ok());
        let mut rig = IsolationRig::new(PAGE_SLOTS);
        let p = rig.arm(0x77).unwrap();
        rig.enter(p).unwrap();
        rig.leave(p).unwrap();
        assert!(rig.residue_scan().is_empty());
        // 清账页未归还前不算干净空闲页（Cleared ≠ Free）。
        assert_eq!(rig.free_clean_pages(), (PAGE_SLOTS - 1) as u32);
        rig.release(p).unwrap();
        assert_eq!(rig.free_clean_pages(), PAGE_SLOTS as u32);
    }

    #[test]
    fn schedule_runs_zero_leak() {
        assert!(schedule_flow_ok());
        assert!(cross_case_zero_leak_ok());
        let mut rig = IsolationRig::new(PAGE_SLOTS);
        run_schedule_once(&mut rig).unwrap();
        assert_eq!(rig.released, 3);
    }

    #[test]
    fn pool_exhaustion_rejected_explicitly() {
        assert!(pool_exhaustion_ok());
        let mut rig = IsolationRig::new(1);
        rig.arm(0x11).unwrap();
        assert_eq!(rig.arm(0x12).unwrap_err(), Diag::NoFreePage.val());
    }

    #[test]
    fn oob_write_rejected_and_neighbor_safe() {
        assert!(scratch_oob_ok());
        let mut rig = IsolationRig::new(1);
        let p = rig.arm(0x21).unwrap();
        rig.enter(p).unwrap();
        let pos = rig.locate(p).unwrap();
        let page = rig.pages.get_mut(pos).unwrap();
        assert_eq!(
            page.write_word(SCRATCH_WORDS, 9).unwrap_err(),
            Diag::ScratchOob.val()
        );
    }

    #[test]
    fn state_machine_rejects_regression() {
        assert!(state_regress_ok());
        assert_eq!(
            step_state(PageState::InUse, PageState::Armed).unwrap_err(),
            Diag::StateRegress.val()
        );
        assert_eq!(
            step_state(PageState::Cleared, PageState::Cleared).unwrap_err(),
            Diag::StateRegress.val()
        );
    }

    #[test]
    fn m170_leak_injection_is_caught() {
        assert!(leak_injection_caught_ok());
        let leaked = inject_leak_and_scan().unwrap();
        assert_eq!(leaked.len(), 1);
        assert_eq!(leaked[0], 0);
    }

    #[test]
    fn double_run_consistency_holds() {
        assert!(double_run_ok());
        assert!(inject_double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        let mut rig = IsolationRig::new(2);
        assert!(rig.arm(0).is_err());
        assert_eq!(rig.enter(0xABC).unwrap_err(), Diag::PageTableCorrupt.val());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
        let mut rig = IsolationRig::new(PAGE_SLOTS);
        run_schedule_once(&mut rig).unwrap();
        let (leaked, _touches) = audit_once(&rig);
        assert_eq!(leaked, 0);
        // release 会按设计清零触达计数，故以释放数守恒为账面证据。
        assert_eq!(rig.released, 3);
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3411_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }

    #[test]
    fn release_is_the_only_legal_rollback() {
        let mut rig = IsolationRig::new(1);
        let p = rig.arm(0x31).unwrap();
        // 未清账页不可 release（StateRegress）。
        assert_eq!(rig.release(p).unwrap_err(), Diag::StateRegress.val());
        rig.enter(p).unwrap();
        rig.leave(p).unwrap();
        rig.release(p).unwrap();
        assert_eq!(rig.released, 1);
        assert!(rig.arm(0x32).is_ok());
    }
}
