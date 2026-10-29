//! unx48249_phrasecol — UNX-F48249 短语冲突消解（同键短语序稳定、编辑不留死角）（诊断段 0xC000）。
//!
//! 判据原文：
//!   J1 冲突 ×10 消解确定；判据锚定 体验六 输入。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同键冲突账：PhraseCol 以 code 为键收多短语，槽位序（slot）
//!      即消解序——同键多短语按入账序稳定呈现（体验六口径：不因
//!      编辑跳动）。slot 连续密集（0 起连续编号），这是"不留死角"
//!      的账面形态。
//!   2. 编辑三 op 全走账：insert（尾插，slot 自动顺延）、delete
//!      （删槽后立即压实 compact：后槽前移，无洞）、edit（原位换
//!      槽内容，slot 不变——编辑不改序）。压实后 slot 密集审计
//!      必过，任何洞即 E_HOLE。
//!   3. 消解确定性双保险：其一，呈现序 == slot 序（present 不重排）；
//!      其二，同 (code, phrase) 重复入账拒（E_DUP）——冲突面只许
//!      不同短语，同短语多占槽即账破。
//!   4. J1 主判据 ×10：十组冲突场景（2..6 短语同键 + 随机编辑序：
//!      删中间/删首/删尾/改写/重插），每组跑后断言：呈现序与期望
//!      slot 序逐位相等、slot 密集无洞、删除后总长守恒。
//!   5. 期望函数独立：场景的期望呈现序由构造期直接记入（不走
//!      present 代码路径），双算对账。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC000;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_HOLE: u16 = SEG_BASE + 1; // slot 有洞（死角）
const E_DUP: u16 = SEG_BASE + 2; // 同 (code, phrase) 重复入账
const E_MISS: u16 = SEG_BASE + 3; // 删/改不存在的槽
const E_ORDER: u16 = SEG_BASE + 4; // 呈现序与 slot 序不符
const E_LEN_BAD: u16 = SEG_BASE + 5; // 删除后总长不守恒
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_SCEN_MISS: u16 = SEG_BASE + 7; // 场景数不足 ×10

/// 单槽：slot 号 + 短语 id。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub code: u32,
    pub slot: u32,
    pub phrase: u32,
}

/// 短语冲突账（全局单列，code 分组呈现）。
pub struct PhraseCol {
    pub slots: Vec<Slot>,
}

impl PhraseCol {
    pub fn new() -> PhraseCol {
        PhraseCol { slots: Vec::new() }
    }

    /// insert：尾插，slot = 该 code 当前最大 slot + 1（0 起）。
    pub fn insert(&mut self, code: u32, phrase: u32) -> Result<u32, u16> {
        let mut max_slot: i64 = -1;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].code == code {
                if self.slots[i].phrase == phrase {
                    return Err(E_DUP); // 同短语不重占槽（纪律 3）
                }
                if (self.slots[i].slot as i64) > max_slot {
                    max_slot = self.slots[i].slot as i64;
                }
            }
            i += 1;
        }
        let slot = (max_slot + 1) as u32;
        self.slots.push(Slot { code, slot, phrase });
        Ok(slot)
    }

    /// delete：删槽后立即压实（纪律 2）。
    pub fn delete(&mut self, code: u32, slot: u32) -> Result<(), u16> {
        let mut idx: i64 = -1;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].code == code && self.slots[i].slot == slot {
                idx = i as i64;
                break;
            }
            i += 1;
        }
        if idx < 0 {
            return Err(E_MISS);
        }
        self.slots.remove(idx as usize);
        self.compact(code);
        Ok(())
    }

    /// edit：原位换内容，slot 不变（编辑不改序）。
    pub fn edit(&mut self, code: u32, slot: u32, phrase: u32) -> Result<(), u16> {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].code == code && self.slots[i].slot == slot {
                self.slots[i].phrase = phrase;
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(E_MISS);
        }
        Ok(())
    }

    /// 压实：该 code 槽位按原相对序重排为 0 起连续。
    pub fn compact(&mut self, code: u32) {
        let mut next: u32 = 0;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].code == code {
                self.slots[i].slot = next;
                next += 1;
            }
            i += 1;
        }
    }

    /// 呈现序 == slot 序（纪律 3）：升序扫描产出短语序。
    pub fn present(&self, code: u32) -> Vec<u32> {
        // 收集 + 选择排序（slot 升序、同 slot 不可能）。
        let mut idxs: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].code == code {
                idxs.push(i);
            }
            i += 1;
        }
        let mut out: Vec<u32> = Vec::new();
        let n = idxs.len();
        let mut used: Vec<bool> = Vec::new();
        let mut i = 0usize;
        while i < n {
            used.push(false);
            i += 1;
        }
        let mut i = 0usize;
        while i < n {
            let mut best = usize::MAX;
            let mut j = 0usize;
            while j < n {
                if !used[j] && (best == usize::MAX || self.slots[idxs[j]].slot < self.slots[idxs[best]].slot) {
                    best = j;
                }
                j += 1;
            }
            if best == usize::MAX {
                break;
            }
            used[best] = true;
            out.push(self.slots[idxs[best]].phrase);
            i += 1;
        }
        out
    }

    /// slot 密集审计（纪律 1）：每 code 槽位 0 起连续无洞。
    pub fn audit_dense(&self) -> Result<(), u16> {
        // 收集 code 闭集。
        let mut codes: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            let mut seen = false;
            let mut j = 0usize;
            while j < codes.len() {
                if codes[j] == self.slots[i].code {
                    seen = true;
                }
                j += 1;
            }
            if !seen {
                codes.push(self.slots[i].code);
            }
            i += 1;
        }
        // 每 code 数槽位数，最大 slot 必须恰为计数-1（密集充要）。
        let mut c = 0usize;
        while c < codes.len() {
            let mut count = 0u32;
            let mut max_slot: i64 = -1;
            let mut i = 0usize;
            while i < self.slots.len() {
                if self.slots[i].code == codes[c] {
                    count += 1;
                    if (self.slots[i].slot as i64) > max_slot {
                        max_slot = self.slots[i].slot as i64;
                    }
                }
                i += 1;
            }
            if max_slot + 1 != count as i64 {
                return Err(E_HOLE);
            }
            c += 1;
        }
        Ok(())
    }

    /// 全局查重（纪律 3）：同 (code, phrase) 不得多占槽。
    pub fn audit_dup(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.slots.len() {
            let mut j = i + 1;
            while j < self.slots.len() {
                if self.slots[i].code == self.slots[j].code
                    && self.slots[i].phrase == self.slots[j].phrase
                {
                    return Err(E_DUP);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

/// LCG（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（冲突 ×10 消解确定），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xC000_0010);

    // R1 主判据 ×10：十组冲突场景（插入 N 笔 + 随机编辑 op 序），
    //    跑后呈现序与构造期记账的期望序逐位相等 + 密集审计 + 查重审计。
    let mut i = 0u32;
    while i < 10 {
        let n = 2 + rng.below(5); // 2..6 短语同键
        let code = 100 + i;
        let mut col = PhraseCol::new();
        let mut want: Vec<u32> = Vec::new();
        let mut pid: u32 = 1_000 + i * 100;
        // 插入期：期望序同步记账。
        let mut k = 0u32;
        while k < n {
            col.insert(code, pid)?;
            want.push(pid);
            pid += 1;
            k += 1;
        }
        // 编辑期：2..4 个随机 op（删中间/删首/删尾/改写）。
        let ops = 2 + rng.below(3);
        let mut k = 0u32;
        while k < ops {
            if want.len() <= 1 {
                break; // 至少留一笔，空呈现另案
            }
            let op = rng.below(4);
            if op == 0 && want.len() >= 2 {
                // 删中间。
                let mid = 1 + rng.below((want.len() - 1) as u32) as usize;
                let mid = if mid >= want.len() { want.len() - 1 } else { mid };
                col.delete(code, mid as u32)?;
                want.remove(mid);
            } else if op == 1 {
                // 删首。
                col.delete(code, 0)?;
                want.remove(0);
            } else if op == 2 {
                // 删尾。
                let last = (want.len() - 1) as u32;
                col.delete(code, last)?;
                want.remove(want.len() - 1);
            } else {
                // 改写：原位换 id（期望序同位换值）。
                let at = rng.below(want.len() as u32) as usize;
                let new_pid = pid;
                pid += 1;
                col.edit(code, at as u32, new_pid)?;
                want[at] = new_pid;
            }
            k += 1;
        }
        // 对账三连：呈现序 == 期望、密集无洞、无重占槽。
        let got = col.present(code);
        if got.len() != want.len() {
            return Err(E_LEN_BAD);
        }
        let mut k = 0usize;
        while k < got.len() {
            if got[k] != want[k] {
                return Err(E_ORDER);
            }
            k += 1;
        }
        col.audit_dense()?;
        col.audit_dup()?;
        i += 1;
    }
    hits += 1;

    // R2 稳定性：同账两跑呈现序逐位相等（不因调用跳动）。
    let mut col = PhraseCol::new();
    col.insert(500, 1)?;
    col.insert(500, 2)?;
    col.insert(500, 3)?;
    let a = col.present(500);
    let b = col.present(500);
    if a.len() != 3 || a[0] != 1 || a[1] != 2 || a[2] != 3 {
        return Err(E_ORDER);
    }
    if a != b {
        return Err(E_ORDER);
    }
    hits += 1;

    // R3 删中压实（纪律 2）：[1,2,3] 删 slot1 → [1,3] 且 slot 重排
    //    0/1——呈现序 [1,3]，密集审计过。
    col.delete(500, 1)?;
    let got = col.present(500);
    if got.len() != 2 || got[0] != 1 || got[1] != 3 {
        return Err(E_ORDER);
    }
    col.audit_dense()?;
    hits += 1;

    // R4 edit 不改序（纪律 2）：原位换内容，slot 与序不动。
    col.edit(500, 1, 9)?;
    let got = col.present(500);
    if got.len() != 2 || got[0] != 1 || got[1] != 9 {
        return Err(E_ORDER);
    }
    hits += 1;

    // R5 死角探针：构造带洞账（直接塞槽 0/2）→ 密集审计必炸。
    let bad = PhraseCol {
        slots: {
            let mut v = Vec::new();
            v.push(Slot { code: 600, slot: 0, phrase: 1 });
            v.push(Slot { code: 600, slot: 2, phrase: 2 });
            v
        },
    };
    if bad.audit_dense().is_ok() {
        return Err(E_HOLE);
    }
    hits += 1;

    // R6 消解确定探针：同键 6 笔（含重复 id 拒、重插后序不变）。
    let mut col = PhraseCol::new();
    let mut k = 0u32;
    while k < 5 {
        col.insert(700, 10 + k)?;
        k += 1;
    }
    if col.insert(700, 10).is_ok() {
        return Err(E_DUP); // 同短语重占槽
    }
    // 删首再尾插同 id：合法（不同时在账即不重）。
    col.delete(700, 0)?;
    col.insert(700, 10)?;
    let got = col.present(700);
    if got.len() != 5 || got[4] != 10 || got[0] != 11 {
        return Err(E_ORDER); // 重插进队尾，序稳定
    }
    col.audit_dense()?;
    col.audit_dup()?;
    hits += 1;

    // R7 删不存在槽拒 + 写回（判据 J2）。
    if col.delete(700, 99).is_ok() {
        return Err(E_MISS);
    }
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x3000)?;
    hits += 1;

    Ok(hits)
}
