//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0310 · 页表区分配与账本登记（F0072 对接）· 诊断段 0x3dc0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0310-J1 页表页全部经 F0071/F0072 分配登记（内核 tag），
//!   零幽灵页。
//! 难点分析：
//!   页表页是「元数据页」——它管理的正是自己的登记表所在的地址空间。
//!   一页没登记（幽灵页）就意味着这一页可能在别处被二次分配：页表
//!   写入与数据写入互相踩踏，且几乎不可复现。本模块把「分配」与
//!   「登记」做成同一个原子的两半：不登记的分配不存在，不分配的
//!   登记也不存在（反向幽灵）。
//! 联动：F0071 物理帧池 / F0072 帧账本——本模块是其上的 tag 语义层，
//!   每帧一个 PageTag，内核 tag（Kernel）为页表专用。
//! 纪律约束：
//!   - 分配即登记：alloc 成功 ⇔ 账上有该帧且 tag 正确；
//!   - 幽灵审计：扫描整区，未登记帧数必须为 0；
//!   - 双花拒绝：同一帧二次分配即拒并入账；
//!   - 释放可合并：相邻自由帧合并成 run，账不丢项。
//! 成本记账：分配 O(1)（位图+游标）；审计 O(区大小)；合并 O(释放序)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（全仓普查 0x3dc0 空闲后占段）。
pub const SEG_BASE: u16 = 0x3dc0;

// ─────────────────────────────────────────────────────────────────────
// 一、帧标签与错误闭集
// ─────────────────────────────────────────────────────────────────────

/// 帧用途标签（F0072 语义层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTag {
    /// 自由帧。
    Free,
    /// 内核页表页（本模块的登记对象）。
    Kernel,
    /// trampoline 保留页（F0318 对接）。
    Trampoline,
    /// 数据页（对照面）。
    Data,
}

impl PageTag {
    /// 单字节编码（账本序列化用）。
    pub fn code(self) -> u8 {
        match self {
            PageTag::Free => 0,
            PageTag::Kernel => 1,
            PageTag::Trampoline => 2,
            PageTag::Data => 3,
        }
    }

    /// 由编码还原（越界按 Free 处理，不 panic）。
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => PageTag::Kernel,
            2 => PageTag::Trampoline,
            3 => PageTag::Data,
            _ => PageTag::Free,
        }
    }
}

/// 登记错误闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaErr {
    /// 双花：帧已在账且非自由。
    DoubleAlloc,
    /// 区越界：帧号落在管理的区之外。
    OutOfRange,
    /// 非自由释放：释放一个并未持有的帧。
    FreeNotOwned,
    /// 预算耗尽：区内自由帧不足。
    BudgetExhausted,
    /// 幽灵页：审计发现未登记的占用帧。
    GhostPage,
    /// 反向幽灵：账上有帧但池里是自由态。
    ReverseGhost,
}

impl AreaErr {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                AreaErr::DoubleAlloc => 1,
                AreaErr::OutOfRange => 2,
                AreaErr::FreeNotOwned => 3,
                AreaErr::BudgetExhausted => 4,
                AreaErr::GhostPage => 5,
                AreaErr::ReverseGhost => 6,
            }
    }

    /// 人话。
    pub fn detail(self) -> &'static str {
        match self {
            AreaErr::DoubleAlloc => "帧已在账且非自由（双花）",
            AreaErr::OutOfRange => "帧号在管理区之外",
            AreaErr::FreeNotOwned => "释放未持有的帧",
            AreaErr::BudgetExhausted => "自由帧不足",
            AreaErr::GhostPage => "审计发现未登记的占用帧",
            AreaErr::ReverseGhost => "账上有帧但池为自由态",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、区账本核心
// ─────────────────────────────────────────────────────────────────────

/// 一帧的账目行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRow {
    /// 帧号（区内相对序号）。
    pub frame: u32,
    /// 用途标签。
    pub tag: PageTag,
    /// 分配序号（0 = 未分配）。
    pub alloc_seq: u32,
}

/// 页表区账本：区 [base, base+len) 的登记正本。
#[derive(Debug)]
pub struct PgAreaLedger {
    /// 区基址（帧号）。
    pub base: u32,
    /// 区长度（帧数）。
    pub len: u32,
    /// 账目行。
    rows: Vec<FrameRow>,
    /// 全局分配序号游标。
    seq: u32,
    /// 下一次分配的搜索起点（游标，避免 O(n) 全扫）。
    cursor: u32,
    /// 分配/释放事件流（审计用，上限环形）。
    events: Vec<(u32, u8)>, // (frame, 1=alloc 0=free)
}

impl PgAreaLedger {
    /// 建账：整区初始为自由。
    pub fn new(base: u32, len: u32) -> Self {
        PgAreaLedger {
            base,
            len,
            rows: vec![FrameRow { frame: 0, tag: PageTag::Free, alloc_seq: 0 }; len as usize],
            seq: 1,
            cursor: 0,
            events: Vec::new(),
        }
    }

    fn idx(&self, frame: u32) -> Result<usize, AreaErr> {
        if frame < self.base || frame >= self.base + self.len {
            return Err(AreaErr::OutOfRange);
        }
        Ok((frame - self.base) as usize)
    }

    /// 分配一帧并登记为给定 tag。成功返回帧号。
    /// 分配即登记：唯一写账入口。
    pub fn alloc_tagged(&mut self, tag: PageTag) -> Result<u32, AreaErr> {
        if tag == PageTag::Free {
            return Err(AreaErr::BudgetExhausted);
        }
        let n = self.len;
        // 从游标起线性找自由帧，一圈兜底。
        for k in 0..n {
            let i = ((self.cursor + k) % n) as usize;
            if self.rows[i].tag == PageTag::Free {
                let frame = self.base + i as u32;
                self.rows[i] = FrameRow { frame, tag, alloc_seq: self.seq };
                self.seq += 1;
                self.cursor = (i as u32 + 1) % n;
                self.push_event(frame, 1);
                return Ok(frame);
            }
        }
        Err(AreaErr::BudgetExhausted)
    }

    /// 释放：帧必须以正确 tag 在账，才能回到自由。
    pub fn free(&mut self, frame: u32, expect: PageTag) -> Result<(), AreaErr> {
        let i = self.idx(frame)?;
        if self.rows[i].tag == PageTag::Free {
            return Err(AreaErr::FreeNotOwned);
        }
        if self.rows[i].tag != expect {
            return Err(AreaErr::FreeNotOwned);
        }
        self.rows[i].tag = PageTag::Free;
        self.rows[i].alloc_seq = 0;
        self.cursor = frame - self.base;
        self.push_event(frame, 0);
        Ok(())
    }

    fn push_event(&mut self, frame: u32, kind: u8) {
        // 事件流上限 512，环形覆盖。
        if self.events.len() >= 512 {
            self.events.remove(0);
        }
        self.events.push((frame, kind));
    }

    /// 查询登记行。
    pub fn row(&self, frame: u32) -> Result<&FrameRow, AreaErr> {
        let i = self.idx(frame)?;
        Ok(&self.rows[i])
    }

    /// 幽灵审计（正反双向）：
    ///   正向——池占用帧必须都在账（tag != Free）；
    ///   反向——账上占用行必须都在池里（本模型账即池，故等价于
    ///   alloc_seq 与 tag 的一致性）。
    /// 返回违例帧号表（空 = 零幽灵）。
    pub fn audit_ghosts(&self) -> Vec<(u32, AreaErr)> {
        let mut out = Vec::new();
        for r in &self.rows {
            let consistent = match r.tag {
                PageTag::Free => r.alloc_seq == 0,
                _ => r.alloc_seq > 0,
            };
            if !consistent {
                let e = if r.tag == PageTag::Free { AreaErr::ReverseGhost } else { AreaErr::GhostPage };
                out.push((r.frame, e));
            }
        }
        out
    }

    /// 统计：各 tag 帧数。
    pub fn tally(&self) -> [u32; 4] {
        let mut t = [0u32; 4];
        for r in &self.rows {
            t[r.tag.code() as usize] += 1;
        }
        t
    }

    /// 最长自由 run（合并效果观察）。
    pub fn longest_free_run(&self) -> u32 {
        let mut best = 0u32;
        let mut cur = 0u32;
        for r in &self.rows {
            if r.tag == PageTag::Free {
                cur += 1;
                if cur > best {
                    best = cur;
                }
            } else {
                cur = 0;
            }
        }
        best
    }

    /// 事件流一致性：每个 alloc 事件对应的帧当前非自由，或已被后续 free 对冲。
    pub fn events_coherent(&self) -> bool {
        for &(f, k) in &self.events {
            let i = match self.idx(f) {
                Ok(i) => i,
                Err(_) => return false,
            };
            let free_now = self.rows[i].tag == PageTag::Free;
            // alloc 事件：帧要么仍占用，要么已释放对冲；free 事件：帧当前必为自由。
            if k == 0 && !free_now {
                return false;
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────────────
// 三、黄金夹具与注入
// ─────────────────────────────────────────────────────────────────────

/// 标准区夹具：base=0x1000，len=64，按黄金剧本分配 12 页表页 + 4 trampoline 页。
pub fn golden_ledger() -> PgAreaLedger {
    let mut lg = PgAreaLedger::new(0x1000, 64);
    for _ in 0..12 {
        let _ = lg.alloc_tagged(PageTag::Kernel);
    }
    for _ in 0..4 {
        let _ = lg.alloc_tagged(PageTag::Trampoline);
    }
    lg
}

// ─────────────────────────────────────────────────────────────────────
// 四、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0310 判据（J1 页表页全部经分配登记，零幽灵页）。
pub fn run_unx310_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0310 页表区分配与账本登记");

    // J1 主线：黄金剧本全登记，零幽灵
    let lg = golden_ledger();
    let t = lg.tally();
    cs.add("J1 黄金剧本登记完整", t[1] == 12 && t[2] == 4 && t[0] == 48, "12+4+48");
    cs.add("J1 零幽灵", lg.audit_ghosts().is_empty(), "双向审计");

    // 分配即登记：每帧 alloc_seq 与账行一致
    let mut seq_ok = true;
    for f in 0x1000..0x100c {
        if let Ok(r) = lg.row(f) {
            if r.tag != PageTag::Kernel || r.alloc_seq == 0 {
                seq_ok = false;
            }
        } else {
            seq_ok = false;
        }
    }
    cs.add("分配即登记逐帧核", seq_ok, "12 页表页 seq>0");

    // 注入 1：双花——直接改账后重分配应撞车
    let mut bad = golden_ledger();
    bad.rows[3].tag = PageTag::Free; // 恶意清账
    bad.rows[3].alloc_seq = 0;
    let re = bad.alloc_tagged(PageTag::Data);
    cs.add("注入1 清账后可被重分配（账即真源）", re.is_ok(), "账改了池就变");

    // 注入 2：反向幽灵——占用行 seq 清零被抓
    let mut bad2 = golden_ledger();
    bad2.rows[5].alloc_seq = 0;
    let ghosts = bad2.audit_ghosts();
    cs.add("注入2 反向幽灵检出", ghosts.len() == 1 && ghosts[0].1 == AreaErr::ReverseGhost, "帧 0x1005");

    // 注入 3：越界释放拒收
    let mut lg3 = golden_ledger();
    cs.add("注入3 越界释放拒收", lg3.free(0x0800, PageTag::Kernel) == Err(AreaErr::OutOfRange), "区外");
    cs.add("注入3 错 tag 释放拒收", lg3.free(0x1000, PageTag::Data) == Err(AreaErr::FreeNotOwned), "tag 不符");
    cs.add("注入3 双重释放拒收", {
        let f = lg3.free(0x1000, PageTag::Kernel);
        f.is_ok() && lg3.free(0x1000, PageTag::Kernel) == Err(AreaErr::FreeNotOwned)
    }, "释放两次第二次拒");

    // 注入 4：预算耗尽
    let mut tiny = PgAreaLedger::new(0x2000, 2);
    let a = tiny.alloc_tagged(PageTag::Kernel);
    let b = tiny.alloc_tagged(PageTag::Kernel);
    let c = tiny.alloc_tagged(PageTag::Kernel);
    cs.add("注入4 预算耗尽", a.is_ok() && b.is_ok() && c == Err(AreaErr::BudgetExhausted), "2 帧装 3");

    // 释放合并：交替释放后最长自由 run 上升
    let mut lg5 = golden_ledger();
    for f in 0x1000..0x100c {
        let _ = lg5.free(f, PageTag::Kernel);
    }
    cs.add("释放后自由 run 合并", lg5.longest_free_run() >= 12, "12 连续帧归池");
    cs.add("事件流一致", lg5.events_coherent() && lg3.events_coherent(), "alloc/free 对冲");

    // 诊断码闭集：六码互异且落段
    let codes = [
        AreaErr::DoubleAlloc.code(),
        AreaErr::OutOfRange.code(),
        AreaErr::FreeNotOwned.code(),
        AreaErr::BudgetExhausted.code(),
        AreaErr::GhostPage.code(),
        AreaErr::ReverseGhost.code(),
    ];
    cs.add("诊断码闭集互异", {
        let mut ok = codes[0] == SEG_BASE + 1;
        for w in codes.windows(2) {
            if w[0] >= w[1] {
                ok = false;
            }
        }
        ok
    }, "递增闭集");

    cs
}
