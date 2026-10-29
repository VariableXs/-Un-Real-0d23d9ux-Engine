//! UNX-F9077 消费清单逐号核账本（每号"被消费证据"登记格式）（0x62960 段）
//!
//! 锚点：UNX-F9077-J1 登记账（四号段闭集全录 + 号段 × 起点 × 终点逐号
//! 落账 + 渲染导出面）；J2 登记语义（号段连续无缝 + 号段序 + 总跨度 =
//! 各号段和三性质）；J3 封账印（登记指纹 + 封印后只读 + 注入核）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：四号段闭集 + 逐号登记核（无缝闸 + 序核）+ 登记账 + 登记指纹 +
//! 注入核（号段重叠/号段断档/起点倒挂/未知号段/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62960 独占 0x20）。
pub const SEG_BASE: u32 = 0x62960;
pub const CL_INVERT: u32 = SEG_BASE + 0x01; // 起点倒挂
pub const CL_DUP: u32 = SEG_BASE + 0x02; // 号段重复登记
pub const CL_UNK: u32 = SEG_BASE + 0x03; // 未知号段
pub const CL_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const CL_GAP: u32 = SEG_BASE + 0x05; // 号段断档（覆盖不连续）

/// 消费清单四号段闭集。
pub const CL_SLOT_FILES: usize = 0; // 文件族被消费证据
pub const CL_SLOT_MEM: usize = 1; // 内存族被消费证据
pub const CL_SLOT_PROC: usize = 2; // 进程族被消费证据
pub const CL_SLOT_SIG: usize = 3; // 信号族被消费证据
pub const CL_SLOT_COUNT: usize = 4;

/// 号段名渲染。
pub const CL_SLOT_NAMES: [&str; CL_SLOT_COUNT] = ["files", "mem", "proc", "sig"];

/// 单号段登记账。
#[derive(Clone)]
pub struct ConsumeEntry {
    pub slot: usize,
    pub start_no: u64,
    pub end_no: u64,
}

impl ConsumeEntry {
    pub fn span(&self) -> u64 {
        self.end_no.saturating_sub(self.start_no)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}..{}|span{}",
            CL_SLOT_NAMES[self.slot],
            self.start_no,
            self.end_no,
            self.span()
        )
    }
}

/// 号段序核：start ≤ end。
pub fn cl_order_ok(e: &ConsumeEntry) -> bool {
    e.start_no <= e.end_no
}

/// 号段连续核：前号段终点 = 后号段起点。
pub fn cl_continuous(es: &[ConsumeEntry]) -> bool {
    es.windows(2).all(|w| w[0].end_no == w[1].start_no)
}

/// 号段无重叠核。
pub fn cl_no_overlap(es: &[ConsumeEntry]) -> bool {
    es.windows(2).all(|w| w[0].end_no <= w[1].start_no)
}

/// 消费清单逐号核账本。
pub struct ConsumeLedger {
    entries: [Option<ConsumeEntry>; CL_SLOT_COUNT],
    sealed: bool,
}

impl ConsumeLedger {
    pub fn new() -> Self {
        ConsumeLedger {
            entries: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个号段：号段闭集 + 序核 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, start_no: u64, end_no: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CL_SEALED);
        }
        if slot >= CL_SLOT_COUNT {
            return Err(CL_UNK);
        }
        let e = ConsumeEntry { slot, start_no, end_no };
        if !cl_order_ok(&e) {
            return Err(CL_INVERT);
        }
        if self.entries[slot].is_some() {
            return Err(CL_DUP);
        }
        self.entries[slot] = Some(e);
        Ok(())
    }

    pub fn entry(&self, slot: usize) -> Option<&ConsumeEntry> {
        if slot >= CL_SLOT_COUNT {
            return None;
        }
        self.entries[slot].as_ref()
    }

    /// 登记完整性：四号段全录 + 连续无缝 + 无重叠。
    pub fn registry_ok(&self) -> bool {
        let filled: Vec<ConsumeEntry> = self.entries.iter().flatten().map(|e| e.clone()).collect();
        filled.len() == CL_SLOT_COUNT && cl_continuous(&filled) && cl_no_overlap(&filled)
    }

    /// 消费清单总跨度 = 各号段和。
    pub fn total_span(&self) -> u64 {
        self.entries.iter().flatten().map(|e| e.span()).sum()
    }

    /// 最长号段（证据最厚候选）。
    pub fn longest_slot(&self) -> Option<usize> {
        self.entries
            .iter()
            .flatten()
            .max_by_key(|e| e.span())
            .map(|e| e.slot)
    }

    /// 登记指纹。
    pub fn cl_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (e.start_no as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (e.end_no as u32);
            h ^= (e.end_no >> 32) as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.registry_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!("CONSUME-TOTAL|{}\n", self.total_span()));
        if self.sealed {
            out.push_str("CONSUME-SEAL\n");
        }
        out
    }
}

impl Default for ConsumeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四号段连续登记（10..60/60..140/140..260/260..300）。
fn standard_ledger() -> ConsumeLedger {
    let mut l = ConsumeLedger::new();
    let _ = l.register(CL_SLOT_FILES, 10, 60);
    let _ = l.register(CL_SLOT_MEM, 60, 140);
    let _ = l.register(CL_SLOT_PROC, 140, 260);
    let _ = l.register(CL_SLOT_SIG, 260, 300);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：起点倒挂
    let mut l = ConsumeLedger::new();
    if l.register(CL_SLOT_FILES, 90, 40) == Err(CL_INVERT) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut l = standard_ledger();
    if l.register(CL_SLOT_MEM, 60, 200) == Err(CL_DUP) {
        caught += 1;
    }
    // 注 3：未知号段
    let mut l = ConsumeLedger::new();
    if l.register(7, 0, 10) == Err(CL_UNK) {
        caught += 1;
    }
    // 注 4：断档登记不过完整性核
    let mut l = ConsumeLedger::new();
    let _ = l.register(CL_SLOT_FILES, 10, 60);
    let _ = l.register(CL_SLOT_MEM, 90, 140);
    if !l.registry_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut l = ConsumeLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 登记账；J2 登记语义；J3 封账印。
pub fn run_unx9077_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9077");
    // J1-1 四号段全录
    let l = standard_ledger();
    let ok = (0..CL_SLOT_COUNT).all(|i| l.entry(i).is_some()) && l.registry_ok();
    s.add("9077-J1-四号段全录", ok, if ok { "" } else { "登记账不全" });
    // J1-2 渲染导出齐
    let text = standard_ledger().render();
    let ok = text.contains("files|10..60|span50")
        && text.contains("sig|260..300|span40")
        && text.contains("CONSUME-TOTAL|290");
    s.add("9077-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总跨度核（290）
    let ok = l.total_span() == 290;
    s.add("9077-J2-总跨度", ok, if ok { "" } else { "总跨度漂移" });
    // J2-2 最厚号段（proc 跨 120）
    let ok = l.entry(CL_SLOT_PROC).map(|e| e.span() == 120).unwrap_or(false);
    s.add("9077-J2-最厚号段", ok, if ok { "" } else { "号段跨度漂移" });
    // J2-3 无缝连续核
    let es: Vec<ConsumeEntry> = standard_ledger().entries.iter().flatten().map(|e| e.clone()).collect();
    let ok = cl_continuous(&es);
    s.add("9077-J2-无缝连续", ok, if ok { "" } else { "连续性漂移" });
    // J2-4 无重叠核
    let ok = cl_no_overlap(&es);
    s.add("9077-J2-无重叠", ok, if ok { "" } else { "重叠漂移" });
    // J2-5 序核（倒挂拒）
    let bad = ConsumeEntry { slot: CL_SLOT_FILES, start_no: 90, end_no: 40 };
    let ok = !cl_order_ok(&bad);
    s.add("9077-J2-序核", ok, if ok { "" } else { "序核失效" });
    // J2-6 最长号段 = proc
    let ok = l.longest_slot() == Some(CL_SLOT_PROC);
    s.add("9077-J2-最长判定", ok, if ok { "" } else { "最长判定漂移" });
    // J3-1 登记指纹确定
    let ok = standard_ledger().cl_fp() == standard_ledger().cl_fp();
    s.add("9077-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 界变签变
    let mut l2 = standard_ledger();
    let fp0 = l2.cl_fp();
    if let Some(e) = l2.entries[0].as_mut() {
        e.end_no += 1;
    }
    let ok = l2.cl_fp() != fp0;
    s.add("9077-J3-界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l3 = standard_ledger();
    let ok = l3.seal() && l3.sealed();
    s.add("9077-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l3.register(CL_SLOT_FILES, 0, 1) == Err(CL_SEALED);
    s.add("9077-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9077-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9077_all_checks_green() {
        let set = run_unx9077_checks();
        assert!(set.all_passed(), "UNX-F9077 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9077_registry_stable() {
        assert!(standard_ledger().registry_ok());
        assert_eq!(standard_ledger().total_span(), 290);
    }
}
