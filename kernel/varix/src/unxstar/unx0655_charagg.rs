//! UNX-F0655 · 域字账聚合（660 条字数总账）· 诊断段 0x5360（本单独占）
//!
//! 目标行数：≥200（纯功能代码）
//! 判据原文摘录：UNX-F0655-J1 660 条深化字数聚合总账与逐批册锁值对冲全平，误差 0。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 守恒恒等式：Σ(660 条字数) = Σ(逐批小计) = Σ(逐批册锁值)，三方全平误差 0。
//!  3. 批册锁值落账即冻结（locked=true 后不可改），改锁即拦截。
//!  4. 字数口径：ASCII 字节计数（整数），零浮点。
//!  5. 诊断段 0x5360 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5360;

// ── 诊断码闭集 ───────────────────────────────────────────────
pub const E_COUNT: u16 = SEG_BASE + 1; // 条目字数为 0（口径非法）
pub const E_BATCH: u16 = SEG_BASE + 2; // 批号越界
pub const E_LOCKED: u16 = SEG_BASE + 3; // 册已锁值再改
pub const E_HEDGE: u16 = SEG_BASE + 4; // 对冲不平（总账≠Σ小计≠Σ锁值）
pub const E_OPEN: u16 = SEG_BASE + 5;  // 批册未封口先聚合
pub const E_ENTRY: u16 = SEG_BASE + 6; // 条目数不足 660

/// 总条目数（判据固定 660）。
pub const N_ENTRIES: usize = 660;
/// 批数（660 条 = 10 批 × 66 条）。
pub const N_BATCH: usize = 10;
/// 每批条目数。
pub const PER_BATCH: usize = 66;

/// 单条目账行。
#[derive(Clone, Debug)]
pub struct CharEntry {
    /// 全局条号（1..=660）。
    pub eid: u16,
    /// 所属批号（1..=10）。
    pub batch: u16,
    /// 条目内容（深化字数统计对象）。
    pub text: String,
    /// 字数（ASCII 字节口径）。
    pub chars: u32,
}

/// 批册：66 条一组，小计落账后锁值。
pub struct BatchBook {
    pub batch: u16,
    pub sub_eids: Vec<u16>,
    pub subtotal: u32,
    pub locked: bool,
    /// 锁值 = 封口时小计（冻结副本，对冲基准）。
    pub locked_value: u32,
}

/// 字数总账。
pub struct CharLedger {
    pub entries: Vec<CharEntry>,
    pub batches: Vec<BatchBook>,
    pub grand_total: u32,
}

impl CharLedger {
    pub fn new() -> CharLedger {
        CharLedger { entries: Vec::new(), batches: Vec::new(), grand_total: 0 }
    }

    /// 添加条目：字数按字节口径现算，非 0 校验。
    pub fn add_entry(&mut self, eid: u16, batch: u16, text: &str) -> Result<u32, u16> {
        if text.is_empty() {
            return Err(E_COUNT);
        }
        if batch == 0 || batch as usize > N_BATCH {
            return Err(E_BATCH);
        }
        let chars = text.len() as u32;
        if chars == 0 {
            return Err(E_COUNT);
        }
        self.entries.push(CharEntry {
            eid,
            batch,
            text: String::from(text),
            chars,
        });
        Ok(chars)
    }

    /// 批册封口：先逐条求和后落账（先求和后落账，零虚记），随即锁值冻结。
    pub fn seal_batch(&mut self, batch: u16) -> Result<u32, u16> {
        if batch == 0 || batch as usize > N_BATCH {
            return Err(E_BATCH);
        }
        if self
            .batches
            .iter()
            .any(|b| b.batch == batch && b.locked)
        {
            return Err(E_LOCKED);
        }
        let mut sub_eids = Vec::new();
        let mut subtotal: u32 = 0;
        // 先求和：逐条累加，全程整数
        for e in &self.entries {
            if e.batch == batch {
                subtotal += e.chars;
                sub_eids.push(e.eid);
            }
        }
        if sub_eids.len() != PER_BATCH {
            return Err(E_ENTRY);
        }
        // 后落账 + 锁值冻结
        self.batches.push(BatchBook {
            batch,
            sub_eids,
            subtotal,
            locked: true,
            locked_value: subtotal,
        });
        Ok(subtotal)
    }

    /// 总账聚合：Σ小计 → grand_total（未封批拦截）。
    pub fn aggregate(&mut self) -> Result<u32, u16> {
        if self.batches.len() != N_BATCH {
            return Err(E_OPEN);
        }
        let mut total: u32 = 0;
        for b in &self.batches {
            total += b.subtotal;
        }
        self.grand_total = total;
        Ok(total)
    }

    /// 逐条直和（独立路径），用于对冲。
    pub fn direct_sum(&self) -> u32 {
        let mut s: u32 = 0;
        for e in &self.entries {
            s += e.chars;
        }
        s
    }

    /// Σ锁值（第三条独立路径）。
    pub fn locked_sum(&self) -> u32 {
        let mut s: u32 = 0;
        for b in &self.batches {
            s += b.locked_value;
        }
        s
    }

    /// 三方对冲：直和 = Σ小计 = Σ锁值 = 总账，误差 0。
    pub fn hedge(&self) -> Result<(), u16> {
        if self.grand_total == 0 {
            return Err(E_OPEN);
        }
        let d = self.direct_sum();
        let l = self.locked_sum();
        if d != self.grand_total || l != self.grand_total {
            return Err(E_HEDGE);
        }
        for b in &self.batches {
            if b.locked_value != b.subtotal {
                return Err(E_HEDGE);
            }
            // 锁值可解引复核：Σ成员条字数 = 锁值
            let mut m: u32 = 0;
            for e in &self.entries {
                if b.sub_eids.contains(&e.eid) {
                    m += e.chars;
                }
            }
            if m != b.locked_value {
                return Err(E_HEDGE);
            }
        }
        Ok(())
    }

    /// 锁后改账拦截：任何对已锁批成员的修改路径（模拟）必须拒。
    pub fn try_mutate_locked(&self, eid: u16) -> Result<(), u16> {
        let e = self.entries.iter().find(|e| e.eid == eid).ok_or(E_ENTRY)?;
        let b = self
            .batches
            .iter()
            .find(|b| b.batch == e.batch)
            .ok_or(E_OPEN)?;
        if b.locked {
            return Err(E_LOCKED);
        }
        Ok(())
    }

    /// 总账摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("字数总账: 条数=");
        s.push_str(&self.entries.len().to_string());
        s.push_str(" 批数=");
        s.push_str(&self.batches.len().to_string());
        s.push_str(" 总字数=");
        s.push_str(&self.grand_total.to_string());
        s
    }
}

/// 确定性条目文本生成（字数可预算：len = 8 + (eid % 23)）。
pub fn synth_text(eid: u16) -> String {
    let len = 8 + (eid % 23) as usize;
    let mut s = String::new();
    let mut st = (eid as u64) | 1;
    for _ in 0..len {
        st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        s.push((b'a' + ((st >> 33) % 26) as u8) as char);
    }
    s
}

/// 自例程：660 条入账 → 10 批封口锁值 → 三方对冲全平误差 0 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut led = CharLedger::new();
    for i in 0..N_ENTRIES {
        let eid = (i + 1) as u16;
        let batch = ((eid - 1) / PER_BATCH as u16) + 1;
        led.add_entry(eid, batch, &synth_text(eid))?;
    }
    if led.entries.len() != N_ENTRIES {
        return Err(E_ENTRY);
    }
    let mut sum_small: u32 = 0;
    for b in 1..=(N_BATCH as u16) {
        sum_small += led.seal_batch(b)?;
    }
    let grand = led.aggregate()?;
    // 守恒：Σ批小计 = 总账；直和 = 锁值和
    if grand != sum_small {
        return Err(E_HEDGE);
    }
    led.hedge()?;
    // 注入 1：锁后改账 → 拦截
    if led.try_mutate_locked(1).is_ok() {
        return Err(SEG_BASE + 15);
    }
    // 注入 2：未封满批先聚合 → 拦截
    let mut led2 = CharLedger::new();
    for i in 0..PER_BATCH {
        let eid = (i + 1) as u16;
        led2.add_entry(eid, 1, &synth_text(eid))?;
    }
    led2.seal_batch(1)?;
    if led2.aggregate() != Err(E_OPEN) {
        return Err(SEG_BASE + 16);
    }
    // 注入 3：批号越界 → 拦截
    if led2.seal_batch(11) != Err(E_BATCH) {
        return Err(SEG_BASE + 17);
    }
    Ok(0)
}
