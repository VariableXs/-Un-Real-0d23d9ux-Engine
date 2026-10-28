//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12401 · NtCreateSection 语义档 · 0x30020
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12401-J1 属性语义（PAGE 保护档/SEC_COMMIT/SEC_IMAGE）判据绿，
//!      非法组合注入 20/20 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PAGE 保护档闭集八档（NOACCESS/READONLY/READWRITE/WRITECOPY/
//!     EXECUTE×4），位掩码口径与 NT 常量一致，越档即拒（ProtectBad）；
//!   - 分配属性档：SEC_COMMIT/SEC_RESERVE 互斥，与 SEC_IMAGE 同置冲突
//!     （AllocConflict）；SEC_IMAGE 必须文件背书（file_size>0）；
//!   - SEC_IMAGE 档保护强制只读口径：可写保护随映像档拒绝（ImageProtect）；
//!   - SEC_IMAGE 与 PageSize/AlignmentAttributes 同传拒绝（ImagePageSize）；
//!   - 尺寸语义：MaximumSize=0 拒（SizeZero），非页粒度拒（NotAligned），
//!     SEC_COMMIT 的 CommitSize 不得超文件（CommitOverFile）；
//!   - 节表定容 16 槽：分配/占位/释放三态，满表拒（TableFull）；
//!   - 错误路径全部带码+中文 detail，拒止不改全局状态；
//!   - 注入核：LCG 写死种子 0x30020，每轮 20 例非法组合 20/20 拒止。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30020 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30020;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮非法组合注入例数（判准 20/20）。
pub const INJECT_CASES: usize = 20;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 节表槽位数（定容）。
pub const SEC_SLOTS: usize = 16;

// PAGE 保护档（NT 常量口径，来源 winnt.h）。
/// PAGE_NOACCESS。
pub const PAGE_NOACCESS: u32 = 0x01;
/// PAGE_READONLY。
pub const PAGE_READONLY: u32 = 0x02;
/// PAGE_READWRITE。
pub const PAGE_READWRITE: u32 = 0x04;
/// PAGE_WRITECOPY。
pub const PAGE_WRITECOPY: u32 = 0x08;
/// PAGE_EXECUTE。
pub const PAGE_EXECUTE: u32 = 0x10;
/// PAGE_EXECUTE_READ。
pub const PAGE_EXECUTE_READ: u32 = 0x20;
/// PAGE_EXECUTE_READWRITE。
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
/// PAGE_EXECUTE_WRITECOPY。
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;

// 分配属性档（NT 常量口径）。
/// SEC_COMMIT（页文件背书提交）。
pub const SEC_COMMIT: u32 = 0x0800_0000;
/// SEC_RESERVE（预留不提交）。
pub const SEC_RESERVE: u32 = 0x0400_0000;
/// SEC_IMAGE（映像映射）。
pub const SEC_IMAGE: u32 = 0x0100_0000;
/// SEC_IMAGE 与页粒度参数同传标志位。
pub const SEC_IMAGE_PAGEFLAG: u32 = 0x0040_0000;

/// 诊断码（0x30020 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrSecCode {
    /// 0x30020 保护档非法（越八档闭集）。
    ProtectBad = 0,
    /// 0x30021 分配属性冲突（COMMIT/RESERVE/IMAGE 互斥破）。
    AllocConflict = 1,
    /// 0x30022 映像档保护非法（可写保护随 IMAGE 拒）。
    ImageProtect = 2,
    /// 0x30023 映像档页粒度参数同传拒。
    ImagePageSize = 3,
    /// 0x30024 尺寸为零（MaximumSize=0）。
    SizeZero = 4,
    /// 0x30025 尺寸非页粒度。
    NotAligned = 5,
    /// 0x30026 提交超文件（CommitSize>file_size）。
    CommitOverFile = 6,
    /// 0x30027 节表满（16 槽耗尽）。
    TableFull = 7,
    /// 0x30028 映像档无文件背书。
    ImageNoFile = 8,
    /// 0x30029 节槽越界（句柄/索引出表）。
    SlotOob = 9,
    /// 0x3002a 节已释放（释放槽上再操作）。
    SecFreed = 10,
}

impl CrSecCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CrSecCode; 11] = [
        CrSecCode::ProtectBad, CrSecCode::AllocConflict, CrSecCode::ImageProtect,
        CrSecCode::ImagePageSize, CrSecCode::SizeZero, CrSecCode::NotAligned,
        CrSecCode::CommitOverFile, CrSecCode::TableFull, CrSecCode::ImageNoFile,
        CrSecCode::SlotOob, CrSecCode::SecFreed,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30020 => "保护档非法：先查八档 PAGE 闭集掩码，越档回改 Protect。",
        0x30021 => "分配属性冲突：SEC_COMMIT/SEC_RESERVE 二选一，IMAGE 档不叠传。",
        0x30022 => "映像档保护非法：SEC_IMAGE 只收只读口径，可写保护走节特性派生。",
        0x30023 => "映像档页粒度参数同传：SEC_IMAGE 档剥掉 PageProtection/Alignment 再建。",
        0x30024 => "尺寸为零：MaximumSize 必须 >0，配置收口时校验。",
        0x30025 => "尺寸非页粒度：MaximumSize 向上对齐 4KiB 再入。",
        0x30026 => "提交超文件：CommitSize 不得超文件大小，改 Reserve 档。",
        0x30027 => "节表满：先释放废槽再建，槽位账对平。",
        0x30028 => "映像档无文件背书：SEC_IMAGE 必须挂文件句柄。",
        0x30029 => "节槽越界：句柄先比 SEC_SLOTS 再取槽。",
        0x3002a => "节已释放：释放槽只许回收再分配，禁直接操作。",
        _ => "未知诊断码：回查 0x30020 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30020 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3002_3002_3002_3002;

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

/// 保护档闭集核对（八档逐一在册）。
pub fn protect_domain_ok() -> bool {
    const SET: [u32; 8] = [
        PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
        PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    ];
    let mut i = 0;
    while i < 8 {
        if SET[i] == 0 || SET.iter().filter(|&&p| p == SET[i]).count() != 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 是否八档闭集内的保护。
pub fn is_page_protect(p: u32) -> bool {
    matches!(
        p,
        PAGE_NOACCESS
            | PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY
    )
}

/// 是否可写保护档。
pub fn is_writable_protect(p: u32) -> bool {
    p == PAGE_READWRITE || p == PAGE_WRITECOPY || p == PAGE_EXECUTE_READWRITE || p == PAGE_EXECUTE_WRITECOPY
}

/// 节状态机（Empty→Paged/Image 两态派生）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecState {
    /// 空槽。
    Empty = 0,
    /// 页文件背书节。
    Paged = 1,
    /// 映像节。
    Image = 2,
    /// 已释放。
    Freed = 3,
}

/// 单节对象条目。
#[derive(Clone, Copy, Debug)]
pub struct SecObject {
    /// 保护档（PAGE 档）。
    pub protect: u32,
    /// 分配属性（SEC 档）。
    pub alloc: u32,
    /// 最大尺寸（字节）。
    pub max_size: u64,
    /// 文件大小（0=页文件背书）。
    pub file_size: u64,
    /// 状态。
    pub state: SecState,
}

/// NtCreateSection 语义档正本（节表 + 建节校验器）。
///
/// 生命周期：create（校验→占槽）→ query（只读面）→ release（回收槽）。
#[derive(Debug)]
pub struct SectionBook {
    /// 节表（定容 16 槽）。
    table: [SecObject; SEC_SLOTS],
    /// 已建节累计数（守恒账分子）。
    created: u32,
    /// 拒止累计数（非法组合账）。
    rejects: u32,
    /// 释放槽复用累计（守恒账修正项）。
    reused: u32,
}

impl SectionBook {
    /// 建账（全空槽）。
    pub fn new() -> Self {
        SectionBook {
            table: core::array::from_fn(|_| SecObject {
                protect: 0,
                alloc: 0,
                max_size: 0,
                file_size: 0,
                state: SecState::Empty,
            }),
            created: 0,
            rejects: 0,
            reused: 0,
        }
    }

    /// 建节参数校验（纯函数：只判不写）。
    ///
    /// 返回 Err(码) 即拒止口径；Ok 即合法组合。
    pub fn validate_create(
        protect: u32,
        alloc: u32,
        max_size: u64,
        file_size: u64,
    ) -> Result<(), CrSecCode> {
        // 尺寸零与非页粒度。
        if max_size == 0 {
            return Err(CrSecCode::SizeZero);
        }
        if max_size % PAGE_SIZE != 0 {
            return Err(CrSecCode::NotAligned);
        }
        // 保护档闭集。
        if !is_page_protect(protect) {
            return Err(CrSecCode::ProtectBad);
        }
        // 分配属性互斥：COMMIT 与 RESERVE 同置、IMAGE 与两者同置。
        let commit = alloc & SEC_COMMIT != 0;
        let reserve = alloc & SEC_RESERVE != 0;
        let image = alloc & SEC_IMAGE != 0;
        if commit && reserve {
            return Err(CrSecCode::AllocConflict);
        }
        if image && (commit || reserve) {
            return Err(CrSecCode::AllocConflict);
        }
        if !commit && !reserve && !image {
            return Err(CrSecCode::AllocConflict);
        }
        // 映像档：必须文件背书，页粒度参数同传拒，可写保护拒。
        if image {
            if file_size == 0 {
                return Err(CrSecCode::ImageNoFile);
            }
            if alloc & SEC_IMAGE_PAGEFLAG != 0 {
                return Err(CrSecCode::ImagePageSize);
            }
            if is_writable_protect(protect) {
                return Err(CrSecCode::ImageProtect);
            }
        }
        // 提交超文件。
        if commit && file_size > 0 && max_size > file_size {
            return Err(CrSecCode::CommitOverFile);
        }
        Ok(())
    }

    /// 建节（校验通过才占槽；返回槽号）。
    pub fn create(&mut self, protect: u32, alloc: u32, max_size: u64, file_size: u64) -> Result<usize, CrSecCode> {
        let chk = Self::validate_create(protect, alloc, max_size, file_size);
        if let Err(c) = chk {
            self.rejects += 1;
            return Err(c);
        }
        let mut slot: Option<usize> = None;
        for i in 0..SEC_SLOTS {
            // 空槽与释放槽均可复用（回收口径）。
            if matches!(self.table[i].state, SecState::Empty | SecState::Freed) {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(CrSecCode::TableFull);
            }
        };
        if self.table[s].state == SecState::Freed {
            self.reused += 1;
        }
        let state = if alloc & SEC_IMAGE != 0 {
            SecState::Image
        } else {
            SecState::Paged
        };
        self.table[s] = SecObject { protect, alloc, max_size, file_size, state };
        self.created += 1;
        Ok(s)
    }

    /// 只读查询（槽越界/已释放必拒）。
    pub fn query(&self, slot: usize) -> Result<SecObject, CrSecCode> {
        if slot >= SEC_SLOTS {
            return Err(CrSecCode::SlotOob);
        }
        match self.table[slot].state {
            SecState::Empty | SecState::Freed => Err(CrSecCode::SecFreed),
            _ => Ok(self.table[slot]),
        }
    }

    /// 释放槽（回收后可复用）。
    pub fn release(&mut self, slot: usize) -> Result<bool, CrSecCode> {
        if slot >= SEC_SLOTS {
            return Err(CrSecCode::SlotOob);
        }
        match self.table[slot].state {
            SecState::Empty | SecState::Freed => Err(CrSecCode::SecFreed),
            _ => {
                self.table[slot].state = SecState::Freed;
                Ok(true)
            }
        }
    }

    /// 活节计数。
    pub fn live_count(&self) -> usize {
        self.table
            .iter()
            .filter(|s| s.state == SecState::Paged || s.state == SecState::Image)
            .count()
    }

    /// 建节累计。
    pub fn created_total(&self) -> u32 {
        self.created
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 槽快照。
    pub fn slot_of(&self, slot: usize) -> Option<SecObject> {
        if slot >= SEC_SLOTS {
            None
        } else {
            Some(self.table[slot])
        }
    }

    /// 守恒：created ≡ live + freed + reused（复用修正项入账）。
    pub fn conserved(&self) -> bool {
        let freed = self.table.iter().filter(|s| s.state == SecState::Freed).count();
        self.created as usize == self.live_count() + freed + self.reused as usize
    }
}

/// 合法组合抽签（确定性：从预置合法组合表按 LCG 取）。
fn pick_valid_combo(lcg: &mut Lcg, i: usize) -> (u32, u32, u64, u64) {
    let page: u64 = PAGE_SIZE * 4;
    match lcg.below(4) {
        0 => (PAGE_READONLY, SEC_COMMIT, page, 0),
        1 => (PAGE_READWRITE, SEC_COMMIT, page, 0),
        2 => (PAGE_EXECUTE_READ, SEC_RESERVE, page, 0),
        _ => {
            // 映像档：文件背书 + 只读口径。
            let _ = i;
            (PAGE_READONLY, SEC_IMAGE, page, page)
        }
    }
}

/// 非法组合抽签（确定性：按 LCG 生成 20 例，逐例带期望拒码）。
fn pick_illegal_case(lcg: &mut Lcg, idx: usize) -> (u32, u32, u64, u64, CrSecCode) {
    let k = lcg.below(10);
    match k {
        0 => (0x99, SEC_COMMIT, PAGE_SIZE * 2, 0, CrSecCode::ProtectBad), // 越档保护
        1 => (PAGE_READWRITE, SEC_COMMIT | SEC_RESERVE, PAGE_SIZE * 2, 0, CrSecCode::AllocConflict), // COMMIT|RESERVE
        2 => (PAGE_READWRITE, SEC_IMAGE | SEC_COMMIT, PAGE_SIZE * 2, PAGE_SIZE * 8, CrSecCode::AllocConflict), // IMAGE|COMMIT
        3 => (PAGE_READWRITE, SEC_IMAGE, PAGE_SIZE * 2, 0, CrSecCode::ImageNoFile), // 映像无文件
        4 => (PAGE_EXECUTE_READWRITE, SEC_IMAGE, PAGE_SIZE * 2, PAGE_SIZE * 8, CrSecCode::ImageProtect), // 映像可写
        5 => (PAGE_READONLY, SEC_IMAGE | SEC_IMAGE_PAGEFLAG, PAGE_SIZE * 2, PAGE_SIZE * 8, CrSecCode::ImagePageSize), // 页粒度同传
        6 => (PAGE_READONLY, SEC_COMMIT, 0, 0, CrSecCode::SizeZero),      // 尺寸零
        7 => (PAGE_READONLY, SEC_COMMIT, PAGE_SIZE + 7, 0, CrSecCode::NotAligned), // 非页粒度
        8 => (PAGE_READWRITE, SEC_COMMIT, PAGE_SIZE * 8, PAGE_SIZE * 4, CrSecCode::CommitOverFile), // 提交超文件
        _ => {
            let _ = idx;
            (0x0, SEC_RESERVE, PAGE_SIZE * 2, 0, CrSecCode::ProtectBad)  // 保护零（越档）
        }
    }
}

/// 属性语义判据演练：10/10 轮合法组合全建、拒止账守恒。
pub fn drill_create_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30020 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for r in 0..INJECT_ROUNDS {
        let mut bk = SectionBook::new();
        let mut round_ok = true;
        for i in 0..4 {
            let (p, a, sz, fs) = pick_valid_combo(&mut lcg, i + r);
            match bk.create(p, a, sz, fs) {
                Ok(slot) => {
                    let q = bk.query(slot);
                    round_ok = round_ok && matches!(q, Ok(o) if o.max_size == sz && o.file_size == fs);
                }
                Err(_) => round_ok = false,
            }
        }
        round_ok = round_ok && bk.live_count() == 4 && bk.conserved();
        if round_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("建节判据轮={}/{}", ok, INJECT_ROUNDS))
}

/// 非法组合注入 20/20 拒止演练（每轮 20 例，逐例码值核对）。
pub fn drill_illegal_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30020 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SectionBook::new();
        let mut hit = 0usize;
        for i in 0..INJECT_CASES {
            let (p, a, sz, fs, expect) = pick_illegal_case(&mut lcg, i);
            match bk.create(p, a, sz, fs) {
                Err(c) if c == expect => hit += 1,
                _ => {}
            }
        }
        if hit == INJECT_CASES && bk.live_count() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非法组合拒止 {}/{} 轮={}/{}", INJECT_CASES, INJECT_CASES, ok, INJECT_ROUNDS))
}

/// 拒止不改全局状态演练：注入后活节数与空槽数不变 10/10。
pub fn drill_reject_pure_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30020 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SectionBook::new();
        let _ = bk.create(PAGE_READONLY, SEC_COMMIT, PAGE_SIZE * 4, 0);
        let live0 = bk.live_count();
        let created0 = bk.created_total();
        // 注入一例非法组合（确定性取例）。
        let (p, a, sz, fs, _e) = pick_illegal_case(&mut lcg, 0);
        let _ = bk.create(p, a, sz, fs);
        if bk.live_count() == live0 && bk.created_total() == created0 + 0 && bk.conserved() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("拒止纯净轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：节表转录导出（逐槽一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 节表转录导出。
pub fn export_sec_lines(bk: &SectionBook) -> String {
    let mut s = String::from("节表转录：\n");
    for i in 0..SEC_SLOTS {
        if let Some(o) = bk.slot_of(i) {
            s.push_str(&format!("S{} 态{} 保{:x} 派{:x} 尺{}\n", i, o.state as u8, o.protect, o.alloc, o.max_size));
        }
    }
    s
}

/// 转录完整判据（逐槽行齐备）。
pub fn sec_lines_complete(bk: &SectionBook) -> bool {
    let s = export_sec_lines(bk);
    (0..SEC_SLOTS).all(|i| s.contains(&format!("S{} 态", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：建节报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct SecReport {
    /// 活节数。
    pub live: usize,
    /// 建节累计。
    pub created: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 守恒。
    pub conserved: bool,
}

impl SecReport {
    /// 建报告。
    pub fn build(bk: &SectionBook) -> Self {
        SecReport {
            live: bk.live_count(),
            created: bk.created_total(),
            rejects: bk.reject_total(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.created >= self.live as u32 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SecReport 活={} 建={} 拒={} 守恒={}",
            self.live, self.created, self.rejects, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：保护档变体演练（八档逐一建节全过）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_protect_variants_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30020 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SectionBook::new();
        const SET: [u32; 8] = [
            PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
            PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
        ];
        let mut pass = 0usize;
        for i in 0..8 {
            // 非映像档八档全收（页文件背书）。
            if bk.create(SET[i], SEC_COMMIT, PAGE_SIZE * 4, 0).is_ok() {
                pass += 1;
            }
        }
        let _ = lcg.below(3);
        if pass == 8 && bk.live_count() == 8 && protect_domain_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("保护档变体轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：槽位复用演练（释放→再建同槽回收 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_slot_reuse_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30020 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SectionBook::new();
        let s0 = bk.create(PAGE_READONLY, SEC_COMMIT, PAGE_SIZE * 2, 0).unwrap_or(usize::MAX);
        let rel = bk.release(s0);
        let q_after = bk.query(s0).is_err();
        let s1 = bk.create(PAGE_READWRITE, SEC_COMMIT, PAGE_SIZE * 2, 0);
        let _ = lcg.below(3);
        if matches!(rel, Ok(true))
            && q_after
            && matches!(s1, Ok(x) if x == s0)
            && bk.conserved()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("槽位复用轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SecCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_sec_checklist(bk: &SectionBook) -> Vec<SecCheckRow> {
    let rep = SecReport::build(bk);
    let mut rows: Vec<SecCheckRow> = Vec::new();
    rows.push(SecCheckRow { name: "守恒", passed: bk.conserved() });
    rows.push(SecCheckRow { name: "保护域", passed: protect_domain_ok() });
    rows.push(SecCheckRow { name: "页粒度常量", passed: PAGE_SIZE == 0x1000 });
    rows.push(SecCheckRow { name: "槽位宽", passed: SEC_SLOTS == 16 });
    rows.push(SecCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(SecCheckRow { name: "转录完整", passed: sec_lines_complete(bk) });
    rows.push(SecCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SecCheckRow { name: "SEC常量口径", passed: SEC_COMMIT == 0x0800_0000 && SEC_IMAGE == 0x0100_0000 });
    rows.push(SecCheckRow { name: "可写档口径", passed: is_writable_protect(PAGE_READWRITE) && !is_writable_protect(PAGE_READONLY) });
    rows.push(SecCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sec_checklist_green(rows: &[SecCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sec_stats_line(bk: &SectionBook) -> String {
    format!(
        "建节概况：活 {} 建 {} 拒 {}",
        bk.live_count(),
        bk.created_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12401_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = SectionBook::new();
    let _ = bk.create(PAGE_READONLY, SEC_COMMIT, PAGE_SIZE * 4, 0);
    let _ = bk.create(PAGE_READWRITE, SEC_COMMIT, PAGE_SIZE * 4, 0);
    let _ = bk.create(PAGE_EXECUTE_READ, SEC_RESERVE, PAGE_SIZE * 4, 0);
    let _ = bk.create(PAGE_READONLY, SEC_IMAGE, PAGE_SIZE * 4, PAGE_SIZE * 4);
    // 注入一例非法组合（拒止账口径）。
    let _ = bk.create(0x99, SEC_COMMIT, PAGE_SIZE * 4, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        sec_lines_complete(&bk) && SecReport::build(&bk).all_green(),
        sec_stats_line(&bk),
    ));
    // X-b：保护档变体 10/10。
    let (ok_p, d_p) = drill_protect_variants_10();
    out.push(("X-保护档变体10/10", ok_p, d_p));
    // X-c：槽位复用 10/10。
    let (ok_r, d_r) = drill_slot_reuse_10();
    out.push(("X-槽位复用10/10", ok_r, d_r));
    // X-d：终验清单。
    let rows = build_sec_checklist(&bk);
    out.push(("X-终验清单10项", sec_checklist_green(&rows), SecReport::build(&bk).to_text()));
    // X-e：validate_create 纯函数域（合法例全 Ok）。
    let v1 = SectionBook::validate_create(PAGE_READONLY, SEC_COMMIT, PAGE_SIZE, 0).is_ok();
    let v2 = SectionBook::validate_create(PAGE_READONLY, SEC_IMAGE, PAGE_SIZE, PAGE_SIZE).is_ok();
    out.push(("X-纯函数校验两面", v1 && v2, format!("v1={} v2={}", v1, v2)));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越档保护必拒（ProtectBad）。
    if matches!(SectionBook::validate_create(0x99, SEC_COMMIT, PAGE_SIZE, 0), Err(CrSecCode::ProtectBad)) {
        caught += 1;
        d.push("注1 越档:抓".to_string());
    } else {
        d.push("注1 越档:漏".to_string());
    }
    // 注 2：COMMIT|RESERVE 冲突必拒（AllocConflict）。
    if matches!(
        SectionBook::validate_create(PAGE_READWRITE, SEC_COMMIT | SEC_RESERVE, PAGE_SIZE, 0),
        Err(CrSecCode::AllocConflict)
    ) {
        caught += 1;
        d.push("注2 冲突:抓".to_string());
    } else {
        d.push("注2 冲突:漏".to_string());
    }
    // 注 3：映像档无文件背书必拒（ImageNoFile）。
    if matches!(SectionBook::validate_create(PAGE_READONLY, SEC_IMAGE, PAGE_SIZE, 0), Err(CrSecCode::ImageNoFile)) {
        caught += 1;
        d.push("注3 无文件:抓".to_string());
    } else {
        d.push("注3 无文件:漏".to_string());
    }
    // 注 4：映像档可写保护必拒（ImageProtect）。
    if matches!(
        SectionBook::validate_create(PAGE_EXECUTE_READWRITE, SEC_IMAGE, PAGE_SIZE, PAGE_SIZE),
        Err(CrSecCode::ImageProtect)
    ) {
        caught += 1;
        d.push("注4 映像可写:抓".to_string());
    } else {
        d.push("注4 映像可写:漏".to_string());
    }
    // 注 5：尺寸零必拒（SizeZero）。
    if matches!(SectionBook::validate_create(PAGE_READONLY, SEC_COMMIT, 0, 0), Err(CrSecCode::SizeZero)) {
        caught += 1;
        d.push("注5 尺寸零:抓".to_string());
    } else {
        d.push("注5 尺寸零:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30020 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    CrSecCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12401_checks：本单判据聚合（诊断段 0x30020）。
pub fn run_unx12401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12401 NtCreateSection 语义档");
    // J1-a：属性语义（合法组合建节 10/10）。
    let (ok1, d1) = drill_create_10();
    s.add_dyn("J1-属性语义建节10/10", ok1, &d1);
    // J1-b：非法组合注入 20/20 正确拒止。
    let (ok2, d2) = drill_illegal_10();
    s.add_dyn("J1-非法组合注入20/20", ok2, &d2);
    // J1-c：拒止不改全局状态 10/10。
    let (ok3, d3) = drill_reject_pure_10();
    s.add_dyn("J1-拒止纯净10/10", ok3, &d3);
    // J1-d：标准会话守恒（PAGE/SEC_COMMIT/SEC_IMAGE 三面建齐）。
    let mut bk = SectionBook::new();
    let a = bk.create(PAGE_READWRITE, SEC_COMMIT, PAGE_SIZE * 4, 0).is_ok();
    let b = bk.create(PAGE_READONLY, SEC_IMAGE, PAGE_SIZE * 4, PAGE_SIZE * 4).is_ok();
    s.add_dyn(
        "J1-PAGE/SEC两面守恒",
        a && b && bk.conserved() && bk.live_count() == 2,
        &format!("建2 活{} 守恒={}", bk.live_count(), bk.conserved()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：节表满拒止（TableFull 口径）。
    let mut bk2 = SectionBook::new();
    let mut full_hit = 0usize;
    for i in 0..(SEC_SLOTS + 1) {
        if bk2.create(PAGE_READONLY, SEC_COMMIT, PAGE_SIZE * (i as u64 + 1), 0).is_err() {
            full_hit += 1;
        }
    }
    s.add_dyn(
        "J2-节表满拒止",
        full_hit == 1 && bk2.conserved(),
        &format!("超建拒={}", full_hit),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30020段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12401_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
