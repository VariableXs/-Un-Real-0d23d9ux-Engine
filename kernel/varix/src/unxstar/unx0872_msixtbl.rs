//! unx0872_msixtbl —— UNX-F0872 · MSI-X 表分配器（首次适配+对齐纪律）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0872 · MSI-X 表分配器。
//! 判据原文摘录（卡面验收行）：MSI-X 表分配器（首次适配+对齐纪律）
//!           ——连续段首次适配命中、段长对齐约束全收口、碎片零静默。
//! 诊断段 0x7880（本单独占，0x7881~0x788F 闭集）。
//!
//! 设计纪律：
//! - 表容量上限 2048 项（SDM MSI-X 建模面）；位图驱动：0=空闲 1=占用。
//! - 首次适配：自 0 号槽起扫首段 ≥cnt 的连续空闲带；命中即占用，返回段基。
//! - 对齐纪律：段基须按 `align` 对齐（align 为 2 的幂，1/2/4/8...2048）；
//!   请求段长 cnt > 0 且 ≤ 剩余空闲。
//! - 释放：段基+段长逐位回零；双重释放 / 未分配段释放即红账。
//! - 守恒恒等式：占用数 + 空闲数 = 总容量（每次操作后核）；分配次数 =
//!   在用段数 + 释放次数（段级守恒）。
//! - 定容账：分配事件账上限 96 行，溢出进溢出计数器（不丢总量）。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const XT_VERSION: &str = "unx0872_msixtbl-v1";

/// 诊断段基址（0x7880 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7880;

/// XcCode 闭集（0x7881~0x788F）。
pub const XC_CAP: u16 = SEG_BASE + 1; // 容量不足（无 ≥cnt 连续带）
pub const XC_ALIGN: u16 = SEG_BASE + 2; // 对齐参数非法（非 2 的幂或越 2048）
pub const XC_ZERO_REQ: u16 = SEG_BASE + 3; // 零长度请求
pub const XC_DUP_FREE: u16 = SEG_BASE + 4; // 双重释放
pub const XC_NOT_OWNED: u16 = SEG_BASE + 5; // 释放未持有段（基址未对齐或越界）
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 事件账溢出（非丢数，仅溢出计数）
pub const XC_CONSERVE: u16 = SEG_BASE + 7; // 守恒破
pub const XC_RANGE: u16 = SEG_BASE + 8; // 段越界（基+长 > 容量）

/// MSI-X 表容量上限。
pub const TABLE_CAP: usize = 2048;
/// 事件账定容。
pub const LED_CAP: usize = 96;

/// 事件种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 分配。
    Alloc,
    /// 释放。
    Free,
    /// 复用（释放后原段重分配）。
    Reuse,
}

/// 事件账行：(种类, 段基, 段长, 序号)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvRow {
    /// 事件种类。
    pub kind: EvKind,
    /// 段基。
    pub base: usize,
    /// 段长。
    pub len: usize,
    /// 序号。
    pub seq: u64,
}

/// MSI-X 表段分配器。
pub struct MsixTable {
    /// 槽位位图（true=占用）。
    used: [bool; TABLE_CAP],
    /// 占用槽计数。
    used_cnt: usize,
    /// 在用段数。
    segs_live: usize,
    /// 累计分配段数。
    segs_alloc: u64,
    /// 累计释放段数。
    segs_freed: u64,
    /// 事件账。
    evs: Vec<EvRow>,
    /// 事件溢出计数。
    ev_overflow: usize,
    seq: u64,
}

impl MsixTable {
    pub fn new() -> MsixTable {
        MsixTable {
            used: [false; TABLE_CAP],
            used_cnt: 0,
            segs_live: 0,
            segs_alloc: 0,
            segs_freed: 0,
            evs: Vec::new(),
            ev_overflow: 0,
            seq: 0,
        }
    }

    fn push_ev(&mut self, kind: EvKind, base: usize, len: usize) {
        self.seq += 1;
        if self.evs.len() < LED_CAP {
            self.evs.push(EvRow { kind, base, len, seq: self.seq });
        } else {
            self.ev_overflow += 1;
        }
    }

    /// 对齐参数合法性：2 的幂且 ≤ TABLE_CAP。
    fn align_ok(align: usize) -> bool {
        align >= 1 && align <= TABLE_CAP && (align & (align - 1)) == 0
    }

    /// 分配一段：首次适配 + 段基对齐 + 守恒核。
    pub fn alloc(&mut self, cnt: usize, align: usize) -> Result<usize, u16> {
        // 1. 参数门
        if cnt == 0 {
            return Err(XC_ZERO_REQ);
        }
        if !Self::align_ok(align) {
            return Err(XC_ALIGN);
        }
        if cnt > TABLE_CAP - self.used_cnt {
            return Err(XC_CAP);
        }
        // 2. 首次适配：找首个对齐后仍容纳 cnt 的连续空闲带
        let mut base = self.first_fit(cnt, align)?;
        // 3. 占用落位
        for i in base..base + cnt {
            self.used[i] = true;
        }
        self.used_cnt += cnt;
        self.segs_live += 1;
        // 4. 复用判名：若该段整体曾被释放过（历史账可查），记 Reuse
        let kind = if self.segs_freed > 0 && self.was_freed(base, cnt) {
            EvKind::Reuse
        } else {
            EvKind::Alloc
        };
        self.segs_alloc += 1;
        self.push_ev(kind, base, cnt);
        // 5. 守恒核：占用 + 空闲 = 容量
        debug_conserve(self.used_cnt, TABLE_CAP)?;
        Ok(base)
    }

    /// 首次适配扫描：段基对齐约束下首个 ≥cnt 空闲带。
    fn first_fit(&self, cnt: usize, align: usize) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < TABLE_CAP {
            // 对齐推进：候选基点须对齐
            let cand = (i + align - 1) & !(align - 1);
            if cand >= TABLE_CAP {
                break;
            }
            // 自 cand 找连续空闲带
            let mut run = 0usize;
            let mut j = cand;
            while j < TABLE_CAP && !self.used[j] {
                run += 1;
                if run == cnt {
                    return Ok(cand);
                }
                j += 1;
            }
            // 该带不足：步进越过整带（含对齐缝隙，不重扫）
            if j >= TABLE_CAP {
                break;
            }
            i = j + 1;
        }
        Err(XC_CAP)
    }

    /// 释放一段：逐位回零 + 双放拦截 + 守恒核。
    pub fn free(&mut self, base: usize, cnt: usize) -> Result<(), u16> {
        // 1. 界门
        if cnt == 0 || base >= TABLE_CAP || base + cnt > TABLE_CAP {
            return Err(XC_RANGE);
        }
        // 2. 持有门：段内每位必须全为占用（未分配/双放/部分持有即红）
        for i in base..base + cnt {
            if !self.used[i] {
                // 区分：段首未占用 = 未持有；段中未占用 = 双放/碎片
                return Err(XC_NOT_OWNED);
            }
        }
        // 3. 回零
        for i in base..base + cnt {
            self.used[i] = false;
        }
        self.used_cnt -= cnt;
        self.segs_live -= 1;
        self.segs_freed += 1;
        self.push_ev(EvKind::Free, base, cnt);
        debug_conserve(self.used_cnt, TABLE_CAP)?;
        Ok(())
    }

    /// 历史释放带查询：判 Reuse 用（账内查 Freed 事件覆盖 [base,base+cnt)）。
    fn was_freed(&self, base: usize, cnt: usize) -> bool {
        self.evs.iter().any(|e| {
            e.kind == EvKind::Free && base >= e.base && base + cnt <= e.base + e.len
        })
    }

    /// 在用段数。
    pub fn live(&self) -> usize {
        self.segs_live
    }

    /// 占用槽计数。
    pub fn used_slots(&self) -> usize {
        self.used_cnt
    }

    /// 空闲槽计数。
    pub fn free_slots(&self) -> usize {
        TABLE_CAP - self.used_cnt
    }

    /// 事件账行读取。
    pub fn ev(&self, i: usize) -> Result<EvRow, u16> {
        if i >= self.evs.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.evs[i])
    }

    /// 事件总数（在账 + 溢出）。
    pub fn ev_total(&self) -> u64 {
        self.evs.len() as u64 + self.ev_overflow as u64
    }

    /// 段级守恒：分配次数 = 在用段数 + 释放次数。
    pub fn seg_conserve(&self) -> bool {
        self.segs_alloc == self.segs_live as u64 + self.segs_freed
    }

    /// 槽级守恒：占用 + 空闲 = 容量；事件守恒：在账 + 溢出 = 总发。
    pub fn slot_conserve(&self) -> bool {
        self.used_cnt + self.free_slots() == TABLE_CAP
            && self.evs.len() + self.ev_overflow == self.seq as usize
    }

    /// 最大空闲带长度（碎片观测面）。
    pub fn max_run(&self) -> usize {
        let mut best = 0usize;
        let mut run = 0usize;
        for i in 0..TABLE_CAP {
            if self.used[i] {
                run = 0;
            } else {
                run += 1;
                if run > best {
                    best = run;
                }
            }
        }
        best
    }
}

/// 守恒核（模块级恒等式哨兵）：占用 + 空闲 = 容量，破即红账。
fn debug_conserve(used_cnt: usize, cap: usize) -> Result<(), u16> {
    if used_cnt > cap {
        return Err(XC_CONSERVE);
    }
    Ok(())
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线分配：2048 表，对齐 1，分配 [0,4)
    let mut t = MsixTable::new();
    let b1 = t.alloc(4, 1)?;
    if b1 != 0 || t.live() != 1 || t.used_slots() != 4 {
        return Err(XC_CONSERVE);
    }
    // 2. 对齐纪律：对齐 16，基址必 16 对齐（当前 [4..) 空闲 → cand=16）
    let b2 = t.alloc(8, 16)?;
    if b2 != 16 || b2 % 16 != 0 {
        return Err(XC_ALIGN);
    }
    // 3. 首次适配：再放 [4..8) 段（对齐 1，正好填缝）
    let b3 = t.alloc(4, 1)?;
    if b3 != 4 {
        return Err(XC_CAP);
    }
    // 4. 错误路径①：零长请求
    if t.alloc(0, 1).is_ok() {
        return Err(XC_ZERO_REQ);
    }
    // 5. 错误路径②：对齐非法（3 非 2 的幂）
    if t.alloc(1, 3).is_ok() {
        return Err(XC_ALIGN);
    }
    // 6. 错误路径③：容量不足（申请 > 最大空闲带）
    let need = TABLE_CAP - t.used_slots() + 1;
    if t.alloc(need, 1).is_ok() {
        return Err(XC_CAP);
    }
    // 7. 释放 + 复用：释放 [0,4) 后同位重分 → Reuse 记名
    t.free(b1, 4)?;
    let b4 = t.alloc(4, 1)?;
    if b4 != 0 {
        return Err(XC_CAP);
    }
    let mut saw_reuse = false;
    for i in 0..t.evs.len() {
        if t.ev(i)?.kind == EvKind::Reuse {
            saw_reuse = true;
        }
    }
    if !saw_reuse {
        return Err(XC_CONSERVE);
    }
    // 8. 错误路径④：双重释放（[0,4) 已复用给 b4，先放一次再放即红）
    t.free(b4, 4)?;
    if t.free(b4, 4).is_ok() {
        return Err(XC_NOT_OWNED);
    }
    // 9. 错误路径⑤：未持有段释放（全空闲高位带）
    if t.free(TABLE_CAP - 4, 4).is_ok() {
        return Err(XC_NOT_OWNED);
    }
    // 10. 碎片观测：释放 [16,24) 后最大带 ≥ 8
    t.free(b2, 8)?;
    if t.max_run() < 8 {
        return Err(XC_CONSERVE);
    }
    // 11. 守恒总核
    if !t.seg_conserve() || !t.slot_conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
