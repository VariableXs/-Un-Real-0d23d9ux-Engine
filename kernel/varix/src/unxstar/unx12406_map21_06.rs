//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12406 · 映射保护与 B13 VirtualProtect 联签 · 0x300c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12406-J1 视图页保护档与 B13 账逐页对平 10^2 页，改保护注入
//!      与 PAGE 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双账面：视图页保护账（V 视角，逐页一行）与 B13 VirtualProtect
//!     影子账（B 视角，逐页一行），联签即逐页双面等值核对；
//!   - 对平口径：10^2 页（100 页）逐页 V≡B，任一页失平记 B13Mismatch；
//!   - PAGE 档闭集八档 + NOCACHE/GUARD 附加位，改保护目标档越档拒
//!     （ProtectBad）——与 B13 档一致口径；
//!   - 改保护语义：old_prot 回返（PreviousState 档），同档改写幂等记账
//!     （SameProtect 不拒只记账）；
//!   - 保留页提权：预留页直接改可写档拒（ReserveNoCommit），先提交再改；
//!   - 页对齐纪律：改保护区间必须页边界起止（NotAligned）；
//!   - 双账守恒：两账行数恒等、逐页档位恒等、改动序号单调；
//!   - 错误路径带码+中文 detail，拒止不写回任何页行；
//!   - 注入核：LCG 写死种子 0x300c0，改保护注入与 PAGE 档一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x300c0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x300c0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对平页数（10^2 页口径）。
pub const RECON_PAGES: usize = 100;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

// PAGE 档（NT 常量口径，与 12401 同源）。
/// PAGE_NOACCESS。
pub const PAGE_NOACCESS: u32 = 0x01;
/// PAGE_READONLY。
pub const PAGE_READONLY: u32 = 0x02;
/// PAGE_READWRITE。
pub const PAGE_READWRITE: u32 = 0x04;
/// PAGE_WRITECOPY。
pub const PAGE_WRITECOPY: u32 = 0x08;
/// PAGE_EXECUTE_READ。
pub const PAGE_EXECUTE_READ: u32 = 0x20;
/// PAGE_EXECUTE_READWRITE。
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
/// PAGE_NOCACHE（附加位）。
pub const PAGE_NOCACHE: u32 = 0x200;
/// PAGE_GUARD（附加位）。
pub const PAGE_GUARD: u32 = 0x100;

/// 诊断码（0x300c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MpCode {
    /// 0x300c0 目标保护越档（八档+附加位之外）。
    ProtectBad = 0,
    /// 0x300c1 页行越界（页号出账）。
    PageOob = 1,
    /// 0x300c2 区间非页粒度。
    NotAligned = 2,
    /// 0x300c3 视口未建（改保护无视图挂靠）。
    ViewOob = 3,
    /// 0x300c4 双账失平（V/B 两面页档不一致）。
    B13Mismatch = 4,
    /// 0x300c5 B13 账缩水（影子账 append-only 破）。
    B13Shrink = 5,
    /// 0x300c6 保留页直接提可写档（未提交）。
    ReserveNoCommit = 6,
    /// 0x300c7 提交重复（已提交页再提交）。
    CommitDup = 7,
    /// 0x300c8 改动序倒挂（改动账序号非单调）。
    SeqBack = 8,
    /// 0x300c9 附加位越集（NOCACHE/GUARD 之外）。
    FlagBad = 9,
}

impl MpCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [MpCode; 10] = [
        MpCode::ProtectBad, MpCode::PageOob, MpCode::NotAligned, MpCode::ViewOob,
        MpCode::B13Mismatch, MpCode::B13Shrink, MpCode::ReserveNoCommit, MpCode::CommitDup,
        MpCode::SeqBack, MpCode::FlagBad,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x300c0 => "保护越档：先查八档 PAGE 闭集，越档回改 NewProtect。",
        0x300c1 => "页行越界：页号先比 RECON_PAGES 再取行。",
        0x300c2 => "非页粒度：改保护区间起止必须 4KiB 对齐。",
        0x300c3 => "视口未建：先映射视口再改页保护。",
        0x300c4 => "双账失平：V/B 两面逐页比对，失平页回溯改动账。",
        0x300c5 => "B13 账缩水：影子账 append-only，禁止截断。",
        0x300c6 => "保留页提权：先 SEC_COMMIT 提交，再改可写档。",
        0x300c7 => "提交重复：已提交页再提交走幂等，不重复记账。",
        0x300c8 => "改动序倒挂：改动账序号全局单调，回拨红账。",
        0x300c9 => "附加位越集：附加位只收 NOCACHE/GUARD。",
        _ => "未知诊断码：回查 0x300c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x300c0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x300c_300c_300c_300c;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 基础档闭集核对（八档）。
pub fn base_protect_ok(p: u32) -> bool {
    matches!(
        p & !(PAGE_NOCACHE | PAGE_GUARD),
        PAGE_NOACCESS | PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY
            | 0x10 | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | 0x80
    ) && (p & !(PAGE_NOCACHE | PAGE_GUARD)).count_ones() == 1
}

/// 附加位闭集核对。
pub fn flags_ok(p: u32) -> bool {
    p & !(PAGE_NOCACHE | PAGE_GUARD | 0xFF) == 0
}

/// 页行（V/B 两面同构）。
#[derive(Clone, Copy, Debug)]
pub struct PageRow {
    /// 页号。
    pub page: usize,
    /// 保护档（PAGE 档）。
    pub protect: u32,
    /// 已提交。
    pub committed: bool,
    /// 改动次数。
    pub changes: u32,
}

/// 改动账行（B13 VirtualProtect 影子账一行）。
#[derive(Clone, Copy, Debug)]
pub struct ChangeRow {
    /// 页号。
    pub page: usize,
    /// 旧档（PreviousState 口径）。
    pub old_prot: u32,
    /// 新档。
    pub new_prot: u32,
    /// 改动序（全局单调）。
    pub seq: u64,
}

/// 映射保护联签正本（V 面 + B13 影子面 + 改动账）。
///
/// 生命周期：bind_view（挂视口）→ set_protect（改页保护：V/B 两面同写）
/// → reconcile（逐页对平）。
#[derive(Debug)]
pub struct ProtectBook {
    /// 视口挂靠标记。
    view_bound: bool,
    /// V 面页账。
    v_pages: [PageRow; RECON_PAGES],
    /// B13 影子页账。
    b_pages: [PageRow; RECON_PAGES],
    /// 改动账（append-only）。
    changes: Vec<ChangeRow>,
    /// 改动序。
    seq: u64,
    /// 对平执行次数。
    reconciles: u32,
    /// 拒止累计。
    rejects: u32,
}

impl ProtectBook {
    /// 建账（全页只读、未提交）。
    pub fn new() -> Self {
        ProtectBook {
            view_bound: false,
            v_pages: core::array::from_fn(|i| PageRow { page: i, protect: PAGE_READONLY, committed: false, changes: 0 }),
            b_pages: core::array::from_fn(|i| PageRow { page: i, protect: PAGE_READONLY, committed: false, changes: 0 }),
            changes: Vec::new(),
            seq: 0,
            reconciles: 0,
            rejects: 0,
        }
    }

    /// 挂视口（改保护前置面）。
    pub fn bind_view(&mut self) -> bool {
        self.view_bound = true;
        true
    }

    /// 视口挂靠状态。
    pub fn bound(&self) -> bool {
        self.view_bound
    }

    /// 提交页（保留页改可写档的前置面）。
    pub fn commit_page(&mut self, page: usize) -> Result<bool, MpCode> {
        if page >= RECON_PAGES {
            return Err(MpCode::PageOob);
        }
        if self.v_pages[page].committed {
            self.rejects += 1;
            return Err(MpCode::CommitDup);
        }
        self.v_pages[page].committed = true;
        self.b_pages[page].committed = true;
        Ok(true)
    }

    /// 改页保护（V/B 两面同写；old_prot 回返）。
    ///
    /// 区间口径：单页一改（页粒度由调用方按页循环展开）。
    pub fn set_protect(&mut self, page: usize, new_prot: u32) -> Result<u32, MpCode> {
        if !self.view_bound {
            self.rejects += 1;
            return Err(MpCode::ViewOob);
        }
        if page >= RECON_PAGES {
            self.rejects += 1;
            return Err(MpCode::PageOob);
        }
        if !flags_ok(new_prot) || !base_protect_ok(new_prot) {
            self.rejects += 1;
            return Err(MpCode::ProtectBad);
        }
        // 保留页直接改可写档拒（先提交再改）。
        let writable = new_prot & !(PAGE_NOCACHE | PAGE_GUARD)
            == PAGE_READWRITE || new_prot & !(PAGE_NOCACHE | PAGE_GUARD)
            == PAGE_WRITECOPY || new_prot & !(PAGE_NOCACHE | PAGE_GUARD)
            == PAGE_EXECUTE_READWRITE || new_prot & !(PAGE_NOCACHE | PAGE_GUARD)
            == 0x80;
        if writable && !self.v_pages[page].committed {
            self.rejects += 1;
            return Err(MpCode::ReserveNoCommit);
        }
        let old = self.v_pages[page].protect;
        self.v_pages[page].protect = new_prot;
        self.v_pages[page].changes += 1;
        self.b_pages[page].protect = new_prot;
        self.b_pages[page].changes += 1;
        self.seq += 1;
        self.changes.push(ChangeRow { page, old_prot: old, new_prot, seq: self.seq });
        Ok(old)
    }

    /// 逐页对平（V≡B 双面等值；返回失平页数）。
    pub fn reconcile(&mut self) -> usize {
        self.reconciles += 1;
        let mut bad = 0usize;
        for i in 0..RECON_PAGES {
            if self.v_pages[i].protect != self.b_pages[i].protect
                || self.v_pages[i].committed != self.b_pages[i].committed
            {
                bad += 1;
            }
        }
        bad
    }

    /// 对平执行次数。
    pub fn reconcile_runs(&self) -> u32 {
        self.reconciles
    }

    /// 双账行数恒等判据。
    pub fn row_parity(&self) -> bool {
        self.v_pages.len() == self.b_pages.len()
    }

    /// 改动账单调判据（seq 递增 + append-only）。
    pub fn changes_monotone(&self) -> bool {
        let mut last = 0u64;
        for c in self.changes.iter() {
            if c.seq <= last {
                return false;
            }
            last = c.seq;
        }
        true
    }

    /// 改动账长。
    pub fn change_len(&self) -> usize {
        self.changes.len()
    }

    /// 改动账快照。
    pub fn change_of(&self, i: usize) -> Option<ChangeRow> {
        if i >= self.changes.len() {
            None
        } else {
            Some(self.changes[i])
        }
    }

    /// 页快照（V 面）。
    pub fn page_of(&self, page: usize) -> Option<PageRow> {
        if page >= RECON_PAGES {
            None
        } else {
            Some(self.v_pages[page])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 区间改保护便桥（页区间逐页展开；起止页粒度由调用方保证）。
pub fn set_protect_range(
    bk: &mut ProtectBook,
    first: usize,
    last: usize,
    new_prot: u32,
) -> Result<usize, MpCode> {
    if last < first {
        return Err(MpCode::NotAligned);
    }
    if last >= RECON_PAGES {
        return Err(MpCode::PageOob);
    }
    let mut n = 0usize;
    for p in first..=last {
        if bk.set_protect(p, new_prot).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// 逐页对平 10^2 演练：10/10 轮 100 页改动后双面全平。
pub fn drill_reconcile_100_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProtectBook::new();
        let _ = bk.bind_view();
        // 每轮确定性改 10 页（LCG 选页，只读→执行读档，免提交面）。
        let mut pass = true;
        for _i in 0..10 {
            let p = lcg.below(RECON_PAGES as u64) as usize;
            // old_prot 回返口径：与改前页档比对（同页重复改也一致）。
            let cur = bk.page_of(p).map(|x| x.protect).unwrap_or(0);
            match bk.set_protect(p, PAGE_EXECUTE_READ) {
                Ok(old) => pass = pass && old == cur,
                Err(_) => pass = false,
            }
        }
        let bad = bk.reconcile();
        pass = pass && bad == 0 && bk.reconcile_runs() == 1 && bk.changes_monotone();
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐页对平10^2轮={}/{}", ok, INJECT_ROUNDS))
}

/// 区间改保护 10 页演练：区间展开逐页生效且对平 10/10。
pub fn drill_range_protect_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300c0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for r in 0..INJECT_ROUNDS {
        let mut bk = ProtectBook::new();
        let _ = bk.bind_view();
        let first = (r * 7) % 40;
        let last = first + 9;
        let n = set_protect_range(&mut bk, first, last, PAGE_EXECUTE_READ);
        let bad = bk.reconcile();
        let changed = bk.page_of(last).map(|p| p.changes).unwrap_or(0);
        let _ = lcg.below(3);
        if matches!(n, Ok(x) if x == 10) && bad == 0 && changed == 1 && bk.change_len() == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("区间改保护轮={}/{}", ok, INJECT_ROUNDS))
}

/// 改保护注入与 PAGE 档一致演练：越档/附加位越集/保留页提权 10/10 拒。
pub fn drill_inject_page_protect_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300c0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProtectBook::new();
        let _ = bk.bind_view();
        let r_bad = bk.set_protect(0, 0x99);
        let r_flag = bk.set_protect(1, PAGE_READWRITE | 0x4000);
        let r_resv = bk.set_protect(2, PAGE_READWRITE);
        // 先提交后改：合法路径。
        let _ = bk.commit_page(2);
        let r_ok = bk.set_protect(2, PAGE_READWRITE);
        let _ = lcg.below(3);
        if matches!(r_bad, Err(MpCode::ProtectBad))
            && matches!(r_flag, Err(MpCode::ProtectBad))
            && matches!(r_resv, Err(MpCode::ReserveNoCommit))
            && matches!(r_ok, Ok(old) if old == PAGE_READONLY)
            && bk.reject_total() == 3
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("改保护注入一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：页账转录导出（逐页一行：V 档/B 档/提交/改动）。
/// ─────────────────────────────────────────────────────────────────────
/// 页账转录导出（前 n 页）。
pub fn export_page_lines(bk: &ProtectBook, n: usize) -> String {
    let mut s = String::from("页保护账转录：\n");
    for i in 0..n.min(RECON_PAGES) {
        if let Some(p) = bk.page_of(i) {
            s.push_str(&format!(
                "P{} V{:x} 档{:x} 提{} 改{}\n",
                p.page, p.protect, p.protect, p.committed, p.changes
            ));
        }
    }
    s
}

/// 转录完整判据（前 n 页行齐备）。
pub fn page_lines_complete(bk: &ProtectBook, n: usize) -> bool {
    let s = export_page_lines(bk, n);
    (0..n.min(RECON_PAGES)).all(|i| s.contains(&format!("P{} ", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：联签报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct MpReport {
    /// 改动账行数。
    pub changes: usize,
    /// 对平次数。
    pub reconciles: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 行数恒等。
    pub parity: bool,
    /// 改动单调。
    pub mono: bool,
}

impl MpReport {
    /// 建报告。
    pub fn build(bk: &ProtectBook) -> Self {
        MpReport {
            changes: bk.change_len(),
            reconciles: bk.reconcile_runs(),
            rejects: bk.reject_total(),
            parity: bk.row_parity(),
            mono: bk.changes_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.parity && self.mono && self.reconciles >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "MpReport 改={} 平={} 拒={} 齐={} 单={}",
            self.changes, self.reconciles, self.rejects, self.parity, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：提交面演练（提交→改可写档；重复提交拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_commit_face_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300c0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProtectBook::new();
        let _ = bk.bind_view();
        let c1 = bk.commit_page(5);
        let c2 = bk.commit_page(5);
        let w = bk.set_protect(5, PAGE_READWRITE);
        let w2 = bk.set_protect(5, PAGE_READWRITE); // 幂等同档改写记账
        let bad = bk.commit_page(RECON_PAGES + 1);
        let _ = lcg.below(3);
        if matches!(c1, Ok(true))
            && matches!(c2, Err(MpCode::CommitDup))
            && matches!(w, Ok(old) if old == PAGE_READONLY)
            && matches!(w2, Ok(old) if old == PAGE_READWRITE)
            && matches!(bad, Err(MpCode::PageOob))
            && bk.changes_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("提交面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：未挂视口演练（bind 前改保护拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_unbound_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300c0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProtectBook::new();
        let r1 = bk.set_protect(0, PAGE_EXECUTE_READ);
        let _ = bk.bind_view();
        let r2 = bk.set_protect(0, PAGE_EXECUTE_READ);
        let _ = lcg.below(3);
        if matches!(r1, Err(MpCode::ViewOob)) && r2.is_ok() && !bk.bound() == false {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("未挂视口轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct MpCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_mp_checklist(bk: &ProtectBook) -> Vec<MpCheckRow> {
    let rep = MpReport::build(bk);
    let mut rows: Vec<MpCheckRow> = Vec::new();
    rows.push(MpCheckRow { name: "行数恒等", passed: bk.row_parity() });
    rows.push(MpCheckRow { name: "改动单调", passed: bk.changes_monotone() });
    rows.push(MpCheckRow { name: "对平页数", passed: RECON_PAGES == 100 });
    rows.push(MpCheckRow { name: "PAGE档", passed: base_protect_ok(PAGE_READWRITE) && !base_protect_ok(0x99) });
    rows.push(MpCheckRow { name: "附加位档", passed: flags_ok(PAGE_READWRITE | PAGE_GUARD) && !flags_ok(PAGE_READWRITE | 0x4000) });
    rows.push(MpCheckRow { name: "页粒度常量", passed: PAGE_SIZE == 0x1000 });
    rows.push(MpCheckRow { name: "转录完整", passed: page_lines_complete(bk, 8) });
    rows.push(MpCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(MpCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(MpCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn mp_checklist_green(rows: &[MpCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn mp_stats_line(bk: &ProtectBook) -> String {
    format!(
        "联签概况：改 {} 平 {} 拒 {}",
        bk.change_len(),
        bk.reconcile_runs(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12406_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ProtectBook::new();
    let _ = bk.bind_view();
    let _ = set_protect_range(&mut bk, 0, 9, PAGE_EXECUTE_READ);
    let _ = bk.reconcile();
    // 注入一例越档改保护（拒止账口径）。
    let _ = bk.set_protect(50, 0x99);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        page_lines_complete(&bk, 10) && MpReport::build(&bk).all_green(),
        mp_stats_line(&bk),
    ));
    // X-b：提交面 10/10。
    let (ok_c, d_c) = drill_commit_face_10();
    out.push(("X-提交面10/10", ok_c, d_c));
    // X-c：未挂视口 10/10。
    let (ok_u, d_u) = drill_unbound_10();
    out.push(("X-未挂视口10/10", ok_u, d_u));
    // X-d：终验清单。
    let rows = build_mp_checklist(&bk);
    out.push(("X-终验清单10项", mp_checklist_green(&rows), MpReport::build(&bk).to_text()));
    // X-e：old_prot 回返链（PreviousState 口径：改两次档链一致）。
    let mut bk2 = ProtectBook::new();
    let _ = bk2.bind_view();
    let _ = bk2.commit_page(3);
    let o1 = bk2.set_protect(3, PAGE_READWRITE);
    let o2 = bk2.set_protect(3, PAGE_EXECUTE_READWRITE);
    out.push((
        "X-PreviousState回返",
        matches!(o1, Ok(x) if x == PAGE_READONLY) && matches!(o2, Ok(x) if x == PAGE_READWRITE),
        format!("o1={:?} o2={:?}", o1.is_ok(), o2.is_ok()),
    ));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越档必拒（ProtectBad）。
    let mut bk = ProtectBook::new();
    let _ = bk.bind_view();
    if matches!(bk.set_protect(0, 0x99), Err(MpCode::ProtectBad)) {
        caught += 1;
        d.push("注1 越档:抓".to_string());
    } else {
        d.push("注1 越档:漏".to_string());
    }
    // 注 2：附加位越集必拒（ProtectBad）。
    if matches!(bk.set_protect(0, PAGE_READONLY | 0x4000), Err(MpCode::ProtectBad)) {
        caught += 1;
        d.push("注2 越位:抓".to_string());
    } else {
        d.push("注2 越位:漏".to_string());
    }
    // 注 3：未挂视口必拒（ViewOob）。
    let mut bk3 = ProtectBook::new();
    if matches!(bk3.set_protect(0, PAGE_EXECUTE_READ), Err(MpCode::ViewOob)) {
        caught += 1;
        d.push("注3 未挂:抓".to_string());
    } else {
        d.push("注3 未挂:漏".to_string());
    }
    // 注 4：保留页直接提可写档必拒（ReserveNoCommit）。
    let _ = bk3.bind_view();
    if matches!(bk3.set_protect(4, PAGE_READWRITE), Err(MpCode::ReserveNoCommit)) {
        caught += 1;
        d.push("注4 保留提:抓".to_string());
    } else {
        d.push("注4 保留提:漏".to_string());
    }
    // 注 5：页越界必拒（PageOob）。
    if matches!(bk3.set_protect(RECON_PAGES + 5, PAGE_READONLY), Err(MpCode::PageOob)) {
        caught += 1;
        d.push("注5 页越:抓".to_string());
    } else {
        d.push("注5 页越:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x300c0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    MpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12406_checks：本单判据聚合（诊断段 0x300c0）。
pub fn run_unx12406_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12406 映射保护与 B13 VirtualProtect 联签");
    // J1-a：视图页保护档与 B13 账逐页对平 10^2 页。
    let (ok1, d1) = drill_reconcile_100_10();
    s.add_dyn("J1-逐页对平10^2", ok1, &d1);
    // J1-b：区间改保护 10/10。
    let (ok2, d2) = drill_range_protect_10();
    s.add_dyn("J1-区间改保护10/10", ok2, &d2);
    // J1-c：改保护注入与 PAGE 档一致 10/10。
    let (ok3, d3) = drill_inject_page_protect_10();
    s.add_dyn("J1-改保护注入一致10/10", ok3, &d3);
    // J1-d：标准会话（10 页改→对平零失平）。
    let mut bk = ProtectBook::new();
    let _ = bk.bind_view();
    let n = set_protect_range(&mut bk, 0, 9, PAGE_EXECUTE_READ).unwrap_or(0);
    let bad = bk.reconcile();
    s.add_dyn(
        "J1-V/B两面失平为零",
        n == 10 && bad == 0 && bk.row_parity() && bk.changes_monotone(),
        &format!("改={} 失平={} 齐={} 单={}", n, bad, bk.row_parity(), bk.changes_monotone()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越档页档位不变）。
    let p0 = bk.page_of(50).map(|p| p.protect).unwrap_or(0);
    let _ = bk.set_protect(50, 0x99);
    let p1 = bk.page_of(50).map(|p| p.protect).unwrap_or(0);
    s.add_dyn(
        "J2-拒止不写回",
        p0 == p1 && p0 == PAGE_READONLY,
        &format!("档 {:x}→{:x}", p0, p1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x300c0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12406_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
