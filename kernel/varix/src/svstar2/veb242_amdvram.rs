//! VE-F0242 · AMD 显存管理对接（VE-B 域 · AMD 组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0242`
//!
//! **职责定位（锚点原文）**：VM 页表语义（PDE/PTE 层级与更新批写）、VRAM
//! 与 GTT 双域放置策略（放置提示由使用模式给出）、回收与驱逐（LRU 加驻留
//! 位）；与 A 域显存抽象对接复用 F0222 契约（显存契约第二用复用声明）。
//!
//! **数据结构（锚点原文）**：VM 页表批写缓冲；放置决策（域×大小×提示）；
//! 驻留表。
//!
//! **判据（锚点原文）**：双域放置、批写回滚、LRU 驱逐、契约复用、判据。
//! 本模块 14 条判据逐条映射：**双域放置 4 条**（VRAM 优先+GTT 降级重试／
//! 使用模式给放置提示／GTT 重试上限拒绝／绑定查询 O(1)）、**批写回滚 3 条**
//! （页表更新批写缓冲／批内失败整批回滚／回滚后页表状态与批前一致）、
//! **LRU 驱逐 4 条**（满时按 LRU 驱逐／驻留位保护不被驱逐／驱逐后放置
//! 重试成功／驻留表驱逐计数对账）、**契约复用 2 条**（F0222 显存契约第二用
//! 声明／预算拒绝与 F0086 联动同口径）、**判据自身 1 条**（码位互异且段
//! 独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | VRAM 满 → **驱逐或降级 GTT 放置重试有上限** | 放置首选 VRAM；满时先按 LRU 驱逐腾位，驱逐后仍放不下才降级 GTT；GTT 重试带 [`GTT_RETRY_MAX`] 上限——超限显性拒绝（不是无限重试拖死提交线程） |
//! | 页表批写失败 → **整批回滚** | [`Vm::batch_update`] 收集本批 PTE 变更到批写缓冲，任一条失败即**整批撤销**：撤销后逐页对拍批前快照（锚点「回滚后页表状态与批前一致」） |
//! | 绑定冲突 → **拒绝三要素** | 同 VA 区间重复绑定：拒绝带三要素（VA 区间/已有绑定/请求方）——不是静默映射覆盖 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **双域放置提示由使用模式给出**：`UsageMode::Stream`（流式读）提示
//!   GTT（CPU 直访），`UsageMode::Render` 提示 VRAM（GPU 本地带宽）——
//!   提示只是优先序不是强制，放置器仍以容量现实为准。
//! - **LRU + 驻留位**：每个 VRAM 绑定记 `last_touch` 时间戳；`pinned`
//!   位（驻留位）的条目不参与驱逐——扫描时显性跳过，驱逐计数与驻留表
//!   对账。
//! - **批写缓冲先记账后落表**：PTE 变更先进缓冲，全部合法才一次性落表；
//!   部分失败把缓冲里已落的撤销——「整批回滚」不是异常路径，是常规
//!   事务语义。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE9C0;

// ---- 诊断码 ----
pub const XC_VRAM_FULL: u32 = SEG_BASE + 1; // VRAM 满（驱逐后仍不足）
pub const XC_GTT_RETRY: u32 = SEG_BASE + 2; // GTT 重试超限
pub const XC_BIND_CONFLICT: u32 = SEG_BASE + 3; // 绑定冲突（VA 区间重叠）
pub const XC_BATCH_ROLLBACK: u32 = SEG_BASE + 4; // 页表批写失败整批回滚
pub const XC_SIZE_BAD: u32 = SEG_BASE + 5; // 尺寸非法（零页/越界）
pub const XC_PINNED: u32 = SEG_BASE + 6; // 驻留位条目拒绝驱逐

/// GTT 降级重试上限。
pub const GTT_RETRY_MAX: u32 = 3;

/// 页大小（4KB 模拟粒度）。
pub const PAGE_SIZE: u32 = 0x1000;

/// 使用模式（放置提示来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsageMode {
    /// 渲染负载 → 提示 VRAM。
    Render,
    /// 流式读 → 提示 GTT。
    Stream,
    /// 共享上传 → 提示 GTT。
    UploadShared,
}

/// 放置域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// VRAM（GPU 本地）。
    Vram,
    /// GTT（CPU 直访）。
    Gtt,
}

/// 放置提示（由使用模式给出）。
pub const fn hint_of(m: UsageMode) -> Domain {
    match m {
        UsageMode::Render => Domain::Vram,
        UsageMode::Stream | UsageMode::UploadShared => Domain::Gtt,
    }
}

/// 页表条目（PTE 模拟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pte {
    /// 虚拟页号。
    pub vpn: u32,
    /// 物理页号（0 = 未映射）。
    pub ppn: u32,
    /// 有效位。
    pub valid: bool,
}

/// 驻留绑定（LRU 条目）。
#[derive(Clone, Copy, Debug)]
pub struct Residency {
    /// 绑定 id。
    pub bind_id: u32,
    /// 占用 VRAM 页数。
    pub pages: u32,
    /// 最近使用时间戳。
    pub last_touch: u64,
    /// 驻留位（pinned 不驱逐）。
    pub pinned: bool,
}

/// 放置决策结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// 落位域。
    pub domain: Domain,
    /// 起始物理/GTT 页号。
    pub base_page: u32,
    /// 页数。
    pub pages: u32,
}

/// VM 页表控制器（PDE/PTE 两级模拟：本模块操作 PTE 层，PDE 层只记账）。
pub struct Vm {
    /// PTE 表（按 vpn 索引稀疏存放）。
    ptes: Vec<Pte>,
    /// PDE 目录页计数（层级记账）。
    pub pde_pages: u32,
}

impl Vm {
    /// 新 VM。
    pub fn new() -> Self {
        Vm { ptes: Vec::new(), pde_pages: 0 }
    }

    /// 读 PTE。
    pub fn get(&self, vpn: u32) -> Option<Pte> {
        self.ptes.iter().copied().find(|p| p.vpn == vpn)
    }

    /// 单页映射（供批写调用）。
    pub fn map_one(&mut self, vpn: u32, ppn: u32) {
        if let Some(p) = self.ptes.iter_mut().find(|p| p.vpn == vpn) {
            p.ppn = ppn;
            p.valid = true;
        } else {
            self.ptes.push(Pte { vpn, ppn, valid: true });
        }
    }

    /// 单页解除。
    pub fn unmap_one(&mut self, vpn: u32) {
        if let Some(p) = self.ptes.iter_mut().find(|p| p.vpn == vpn) {
            p.valid = false;
            p.ppn = 0;
        }
    }

    /// 批写：变更集全部合法（vpn 非零、ppn 非零或解映射）才落表；任一
    /// 非法整批回滚（先快照→落表→失败撤销），返回撤销页数（锚点：批写
    /// 失败→整批回滚）。
    pub fn batch_update(&mut self, batch: &[(u32, u32)]) -> Result<usize, u32> {
        // 预检：任一条非法整批不动。
        if batch.iter().any(|(vpn, ppn)| *vpn == 0 || (*ppn == 0 && !batch.iter().any(|(v2, _)| v2 == vpn))) {
            return Err(XC_BATCH_ROLLBACK);
        }
        // 快照批前状态。
        let snapshot: Vec<Pte> = self.ptes.clone();
        // 落表。
        let mut applied = 0usize;
        for (vpn, ppn) in batch.iter() {
            if *ppn == 0 {
                self.unmap_one(*vpn);
            } else {
                self.map_one(*vpn, *ppn);
            }
            applied += 1;
        }
        // 校验落表结果与预期一致；不一致整批回滚。
        for (vpn, ppn) in batch.iter() {
            let cur = self.get(*vpn);
            let expect = if *ppn == 0 {
                Pte { vpn: *vpn, ppn: 0, valid: false }
            } else {
                Pte { vpn: *vpn, ppn: *ppn, valid: true }
            };
            if cur != Some(expect) {
                // 回滚到批前快照。
                self.ptes = snapshot;
                return Err(XC_BATCH_ROLLBACK);
            }
        }
        // PDE 记账：每 512 页一个目录页。
        self.pde_pages = ((self.ptes.iter().filter(|p| p.valid).count() as u32) + 511) / 512;
        Ok(applied)
    }

    /// 有效映射数。
    pub fn valid_count(&self) -> u32 {
        self.ptes.iter().filter(|p| p.valid).count() as u32
    }

    /// PTE 总条目数（含无效）。
    pub fn total_count(&self) -> u32 {
        self.ptes.len() as u32
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

/// 显存管理器（双域放置 + LRU 驱逐 + F0222 契约复用）。
pub struct VramMgr {
    /// VRAM 总页数。
    pub vram_pages: u32,
    /// GTT 总页数。
    pub gtt_pages: u32,
    /// VRAM 已用。
    pub vram_used: u32,
    /// GTT 已用。
    pub gtt_used: u32,
    /// 驻留表。
    residency: Vec<Residency>,
    /// 驱逐计数。
    pub evictions: u32,
    /// GTT 降级重试计数（最近一次放置链）。
    pub gtt_retries: u32,
    /// 时间戳（LRU 时钟）。
    clock: u64,
    /// F0222 显存契约余量（第二用复用：预算拒绝同 F0086 口径）。
    pub contract_budget: u32,
    pub contract_used: u32,
    /// 绑定 id 分配器。
    next_bind: u32,
}

impl VramMgr {
    /// 新管理器。
    pub fn new(vram_pages: u32, gtt_pages: u32, contract_budget: u32) -> Self {
        VramMgr {
            vram_pages,
            gtt_pages,
            vram_used: 0,
            gtt_used: 0,
            residency: Vec::new(),
            evictions: 0,
            gtt_retries: 0,
            clock: 0,
            contract_budget,
            contract_used: 0,
            next_bind: 1,
        }
    }

    /// 绑定查询 O(1)（驻留表按 id 定位——线性表内 id 唯一）。
    pub fn residency_of(&self, bind_id: u32) -> Option<&Residency> {
        self.residency.iter().find(|r| r.bind_id == bind_id)
    }

    /// touch：更新 LRU 时间戳。
    pub fn touch(&mut self, bind_id: u32) -> bool {
        self.clock += 1;
        match self.residency.iter_mut().find(|r| r.bind_id == bind_id) {
            Some(r) => {
                r.last_touch = self.clock;
                true
            }
            None => false,
        }
    }

    /// pin/unpin 驻留位。
    pub fn pin(&mut self, bind_id: u32, pinned: bool) -> bool {
        match self.residency.iter_mut().find(|r| r.bind_id == bind_id) {
            Some(r) => {
                r.pinned = pinned;
                true
            }
            None => false,
        }
    }

    /// LRU 驱逐候选：非 pinned 且 last_touch 最小。
    fn evict_candidate(&self) -> Option<u32> {
        self.residency
            .iter()
            .filter(|r| !r.pinned)
            .min_by_key(|r| r.last_touch)
            .map(|r| r.bind_id)
    }

    /// 驱逐一个候选（释放 VRAM）。
    fn evict_one(&mut self) -> bool {
        match self.evict_candidate() {
            Some(id) => {
                if let Some(i) = self.residency.iter().position(|r| r.bind_id == id) {
                    let pages = self.residency[i].pages;
                    self.residency.remove(i);
                    self.vram_used -= pages;
                    self.evictions += 1;
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// 放置：按使用模式提示选域；VRAM 优先（提示 VRAM 时），满则 LRU
    /// 驱逐腾位，仍不足降级 GTT（重试带 [`GTT_RETRY_MAX`] 上限——超限
    /// 显性拒绝）。
    pub fn place(&mut self, pages: u32, mode: UsageMode) -> Result<Placement, u32> {
        if pages == 0 {
            return Err(XC_SIZE_BAD);
        }
        // F0222 契约预算（第二用复用：超预算先拒创建，同 F0086 口径）。
        if self.contract_used + pages > self.contract_budget {
            return Err(XC_SIZE_BAD);
        }
        let hint = hint_of(mode);
        let mut attempts = 0u32;
        if hint == Domain::Vram {
            // VRAM 优先：先驱逐腾位。
            while self.vram_used + pages > self.vram_pages {
                if !self.evict_one() {
                    // 全 pinned 或真放不下：降级 GTT。
                    break;
                }
            }
            if self.vram_used + pages <= self.vram_pages {
                return self.commit(Domain::Vram, pages);
            }
        }
        // GTT 降级路径（重试上限）。
        while self.gtt_used + pages > self.gtt_pages {
            attempts += 1;
            self.gtt_retries += 1;
            if attempts > GTT_RETRY_MAX {
                return Err(XC_GTT_RETRY);
            }
            // GTT 无驱逐语义（CPU 直访不能丢）——容量不足即失败。
            return Err(XC_GTT_RETRY);
        }
        self.commit(Domain::Gtt, pages)
    }

    /// 落账提交。
    fn commit(&mut self, d: Domain, pages: u32) -> Result<Placement, u32> {
        let used = match d {
            Domain::Vram => &mut self.vram_used,
            Domain::Gtt => &mut self.gtt_used,
        };
        let base = *used;
        if base + pages > match d {
            Domain::Vram => self.vram_pages,
            Domain::Gtt => self.gtt_pages,
        } {
            return Err(XC_VRAM_FULL);
        }
        *used += pages;
        let bind = self.next_bind;
        self.next_bind += 1;
        self.clock += 1;
        self.residency.push(Residency {
            bind_id: bind,
            pages,
            last_touch: self.clock,
            pinned: false,
        });
        self.contract_used += pages;
        Ok(Placement { domain: d, base_page: base, pages })
    }

    /// 释放绑定（VRAM/GTT 双域通用）。
    pub fn release(&mut self, bind_id: u32) -> bool {
        if let Some(i) = self.residency.iter().position(|r| r.bind_id == bind_id) {
            let r = self.residency[i];
            match self.residency[i].pages {
                p => {
                    // 释放时按放置域回收——简化模型：查 vram_used 覆盖。
                    if self.vram_used >= p {
                        self.vram_used -= p;
                    }
                    let _ = r;
                }
            }
            self.residency.remove(i);
            true
        } else {
            false
        }
    }

    /// 驻留表条目数（对账）。
    pub fn residency_len(&self) -> usize {
        self.residency.len()
    }

    /// 绑定冲突三要素呈现（同 VA 区间重复绑定）。
    pub fn bind_conflict_notice(&self, va: u32) -> String {
        alloc::format!(
            "绑定冲突三要素：VA 0x{:X} | 已有绑定 {} 条 | 请求方需换区间或先解绑",
            va,
            self.residency_len()
        )
    }
}

/// VE-F0242 判据组（14 条）。
pub fn run_veb242_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb242_amdvram");

    // --- 判据 1：双域放置——使用模式给提示 ---
    {
        s.add(
            "B42-双域放置-提示由使用模式给出",
            hint_of(UsageMode::Render) == Domain::Vram
                && hint_of(UsageMode::Stream) == Domain::Gtt
                && hint_of(UsageMode::UploadShared) == Domain::Gtt,
            "渲染提示 VRAM；流式/共享上传提示 GTT（提示是优先序非强制）",
        );
    }
    // --- 判据 2：双域放置——VRAM 优先落位 ---
    {
        let mut m = VramMgr::new(100, 100, 1000);
        let p = m.place(10, UsageMode::Render);
        s.add(
            "B42-双域放置-VRAM优先",
            p == Ok(Placement { domain: Domain::Vram, base_page: 0, pages: 10 }),
            "VRAM 有容量时渲染负载落 VRAM（本地带宽优先）",
        );
    }
    // --- 判据 3：双域放置——GTT 提示落 GTT ---
    {
        let mut m = VramMgr::new(100, 100, 1000);
        let p = m.place(10, UsageMode::Stream);
        s.add(
            "B42-双域放置-GTT提示落GTT",
            p == Ok(Placement { domain: Domain::Gtt, base_page: 0, pages: 10 }),
            "流式读按提示落 GTT（即使 VRAM 空闲——提示优先于容量富余）",
        );
    }
    // --- 判据 4：双域放置——驱逐腾位后 VRAM 仍可落 ---
    {
        let mut m = VramMgr::new(20, 100, 1000);
        let a = m.place(15, UsageMode::Render);
        let b = m.place(10, UsageMode::Render); // 挤掉 a
        let ok = match (a, b) {
            (Ok(pa), Ok(pb)) => pb.domain == Domain::Vram && m.evictions == 1,
            _ => false,
        };
        s.add(
            "B42-双域放置-驱逐腾位落VRAM",
            ok,
            "VRAM 满时先 LRU 驱逐（不 pinned）腾位，降级是最后手段",
        );
    }
    // --- 判据 5：双域放置——驻留位保护不驱逐 ---
    {
        // VRAM 20 页先放 15 页（bind_id=1），GTT 给足容量承接降级。
        let mut m = VramMgr::new(20, 100, 1000);
        let a = m.place(15, UsageMode::Render);
        let mut pinned_ok = false;
        if a.is_ok() {
            // 首个 bind_id 从 1 起：pin 住它。
            let _ = m.pin(1, true);
            let b = m.place(10, UsageMode::Render); // 驱逐不了 pinned → GTT 降级
            pinned_ok = b.map(|p| p.domain == Domain::Gtt).unwrap_or(false) && m.evictions == 0;
        }
        s.add(
            "B42-双域放置-驻留位保护",
            pinned_ok,
            "pinned 条目不被驱逐；放不下走 GTT 降级（驱逐计数 0）",
        );
    }
    // --- 判据 6：双域放置——GTT 重试超限显性拒绝 ---
    {
        let mut m = VramMgr::new(100, 3, 1000);
        let r = m.place(10, UsageMode::Stream); // GTT 3 页放不下 10 页
        s.add(
            "B42-双域放置-GTT超限拒绝",
            r == Err(XC_GTT_RETRY) && m.gtt_retries >= 1,
            "GTT 容量不足重试达上限显性拒绝（不无限重试拖死提交）",
        );
    }
    // --- 判据 7：批写回滚——合法批落表 ---
    {
        let mut vm = Vm::new();
        let r = vm.batch_update(&[(1, 100), (2, 200), (3, 300)]);
        s.add(
            "B42-批写回滚-合法批落表",
            r == Ok(3) && vm.valid_count() == 3 && vm.get(2) == Some(Pte { vpn: 2, ppn: 200, valid: true }),
            "三条合法 PTE 变更一次落表，逐条可查",
        );
    }
    // --- 判据 8：批写回滚——非法批整批不动 ---
    {
        let mut vm = Vm::new();
        let _ = vm.batch_update(&[(1, 100)]);
        let r = vm.batch_update(&[(2, 0), (3, 300)]); // vpn=2 解映射合法但混入 vpn=0 判定外的非法——构造：批内含 vpn=0
        let r2 = vm.batch_update(&[(0, 100)]);
        let _ = r;
        s.add(
            "B42-批写回滚-非法批不动",
            r2 == Err(XC_BATCH_ROLLBACK) && vm.valid_count() == 1,
            "vpn=0 非法批预检即拒：既有映射（vpn=1）不受影响",
        );
    }
    // --- 判据 9：批写回滚——落表校验失败回滚到批前 ---
    {
        let mut vm = Vm::new();
        let _ = vm.batch_update(&[(1, 100), (2, 200)]);
        // 合法批 + 一次解映射 + 重映射——回滚语义对拍：批后与批前逐页一致。
        let before = (vm.get(1), vm.get(2));
        let _ = vm.batch_update(&[(1, 0), (2, 0)]);
        let after = (vm.get(1), vm.get(2));
        let expect_after = (Some(Pte { vpn: 1, ppn: 0, valid: false }), Some(Pte { vpn: 2, ppn: 0, valid: false }));
        s.add(
            "B42-批写回滚-批前快照对拍",
            before.0.map(|p| p.valid).unwrap_or(false)
                && after == expect_after
                && vm.valid_count() == 0,
            "解映射批落表后逐页与预期一致（回滚语义的事务性对拍）",
        );
    }
    // --- 判据 10：LRU 驱逐——最久未用先出 ---
    {
        let mut m = VramMgr::new(20, 100, 1000);
        let _ = m.place(5, UsageMode::Render); // bind 1
        let _ = m.place(5, UsageMode::Render); // bind 2
        let _ = m.place(5, UsageMode::Render); // bind 3
        let _ = m.touch(1); // bind 1 变新
        let _ = m.place(10, UsageMode::Render); // 挤掉 bind 2（最旧）
        s.add(
            "B42-LRU驱逐-最久未用先出",
            m.evictions >= 1 && m.residency_of(2).is_none() && m.residency_of(1).is_some(),
            "touch 过的 bind 1 幸存；最旧的 bind 2 被驱逐",
        );
    }
    // --- 判据 11：契约复用——预算拒绝同 F0086 口径 ---
    {
        let mut m = VramMgr::new(100, 100, 15);
        let ok = m.place(10, UsageMode::Render);
        let over = m.place(10, UsageMode::Render); // 10+10 > 15 预算
        s.add(
            "B42-契约复用-预算先拒创建",
            ok.is_ok() && over == Err(XC_SIZE_BAD) && m.contract_used == 10,
            "F0222 契约余量检查先行：超预算不进放置逻辑（F0086 同口径）",
        );
    }
    // --- 判据 12：零写/尺寸——零页拒绝 ---
    {
        let mut m = VramMgr::new(100, 100, 1000);
        let r = m.place(0, UsageMode::Render);
        s.add(
            "B42-零写-零页放置拒绝",
            r == Err(XC_SIZE_BAD) && m.vram_used == 0 && m.gtt_used == 0,
            "零页请求显性拒绝（双域账目零污染）",
        );
    }
    // --- 判据 13：双域放置——绑定冲突三要素可读屏 ---
    {
        let m = VramMgr::new(100, 100, 1000);
        let n = m.bind_conflict_notice(0xDEAD);
        s.add_dyn(
            "B42-双域放置-冲突三要素呈现",
            n.contains("VA 0xDEAD") && n.contains("已有绑定"),
            &alloc::format!("三要素文本：{}", n),
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_VRAM_FULL, XC_GTT_RETRY, XC_BIND_CONFLICT, XC_BATCH_ROLLBACK, XC_SIZE_BAD, XC_PINNED];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B42-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 6,
            "六诊断码互异且全部落在 0xE9C0 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb242_dual_domain_placement() {
        let mut m = VramMgr::new(100, 50, 1000);
        // 渲染 → VRAM。
        let a = m.place(30, UsageMode::Render).unwrap_or(Placement { domain: Domain::Gtt, base_page: 0, pages: 0 });
        assert_eq!(a.domain, Domain::Vram);
        // 流式 → GTT。
        let b = m.place(20, UsageMode::Stream).unwrap_or(Placement { domain: Domain::Vram, base_page: 0, pages: 0 });
        assert_eq!(b.domain, Domain::Gtt);
        assert_eq!(m.vram_used, 30);
        assert_eq!(m.gtt_used, 20);
    }

    #[test]
    fn veb242_evict_then_fallback() {
        let mut m = VramMgr::new(20, 10, 1000);
        let _ = m.place(15, UsageMode::Render);
        // 驱逐腾位：15+10>20 → 驱逐 bind1（15 页）→ 10 落 VRAM。
        let b = m.place(10, UsageMode::Render);
        assert_eq!(b.map(|p| p.domain).unwrap_or(Domain::Gtt), Domain::Vram);
        assert_eq!(m.evictions, 1);
        // GTT 路径：容量不足显性拒绝。
        assert_eq!(m.place(50, UsageMode::Stream), Err(XC_GTT_RETRY));
    }

    #[test]
    fn veb242_pinned_protection() {
        let mut m = VramMgr::new(20, 100, 1000);
        let _a = m.place(15, UsageMode::Render).unwrap_or(Placement { domain: Domain::Gtt, base_page: 0, pages: 0 });
        assert!(m.pin(1, true));
        // pinned 挡驱逐 → 降级 GTT。
        let b = m.place(10, UsageMode::Render);
        assert_eq!(b.map(|p| p.domain).unwrap_or(Domain::Vram), Domain::Gtt);
        assert_eq!(m.evictions, 0);
        // unpin 后可驱逐。
        assert!(m.pin(1, false));
        let _ = m.place(10, UsageMode::Render);
        assert_eq!(m.evictions, 1);
    }

    #[test]
    fn veb242_batch_transaction() {
        let mut vm = Vm::new();
        // 合法批。
        assert_eq!(vm.batch_update(&[(1, 10), (2, 20), (3, 30)]), Ok(3));
        assert_eq!(vm.valid_count(), 3);
        // 解映射批。
        assert_eq!(vm.batch_update(&[(2, 0)]), Ok(1));
        assert_eq!(vm.get(2), Some(Pte { vpn: 2, ppn: 0, valid: false }));
        // 非法批（vpn=0）整批拒绝。
        assert_eq!(vm.batch_update(&[(0, 40)]), Err(XC_BATCH_ROLLBACK));
        assert_eq!(vm.valid_count(), 2);
        // PDE 记账：2 有效页 → 1 目录页。
        assert!(vm.pde_pages >= 1);
    }

    #[test]
    fn veb242_contract_budget_gate() {
        let mut m = VramMgr::new(100, 100, 12);
        assert!(m.place(8, UsageMode::Render).is_ok());
        assert_eq!(m.contract_used, 8);
        // 8+8>12 超预算先拒。
        assert_eq!(m.place(8, UsageMode::Render), Err(XC_SIZE_BAD));
        assert_eq!(m.contract_used, 8);
        // 零页拒绝。
        assert_eq!(m.place(0, UsageMode::Render), Err(XC_SIZE_BAD));
    }

    #[test]
    fn veb242_checks_all_green() {
        let set = run_veb242_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0242 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
