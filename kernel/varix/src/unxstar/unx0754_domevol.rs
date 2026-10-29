//! UNX-F0754 · 移交包域经卷导出（登记簿快照卷） · 诊断段 0x5d40
//! 判据原文摘录：UNX-F0754-J1 导出账齐（高中档条目卷 + 与登记簿一致 100%
//! + 回执前置核）且低档零混入断言过；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 域经卷只收高/中档：卷内条目来自登记簿快照，档位过滤在导出入口
//!    强制执行，低档条目在任何导出路径都不许入卷（零混入断言）。
//! 2. 与登记簿一致 100%：卷行集与登记簿（高/中档子集）逐条双向核对，
//!    卷有簿无（幽灵条目）或簿有卷无（漏导）都判不一致。
//! 3. 回执前置核：导出前必须先落接收方回执（接收方号 + 接收码），
//!    零回执即拒绝导出——回执是导出的前置条件，不是后补项。
//! 4. 卷哈希封存：卷行集聚合哈希封存，封存后卷只读。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5d40 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5d40;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_REG_DUP: u16 = SEG_BASE + 1; // 登记簿条目重复
const E_REG_FULL: u16 = SEG_BASE + 2; // 登记簿已满
const E_REG_NOTFOUND: u16 = SEG_BASE + 3; // 登记簿条目不存在
const E_TIER_BAD: u16 = SEG_BASE + 4; // 档位值非法（内部映射损坏）
const E_LOW_MIXED: u16 = SEG_BASE + 5; // 低档混入卷（零混入断言失败）
const E_GHOST: u16 = SEG_BASE + 6; // 卷有簿无（幽灵条目）
const E_MISSING: u16 = SEG_BASE + 7; // 簿有卷无（漏导）
const E_NO_RECEIPT: u16 = SEG_BASE + 8; // 零回执：导出被拒（前置核失败）
const E_RECEIPT_DUP: u16 = SEG_BASE + 9; // 回执重复落账
const E_RECEIPT_BAD: u16 = SEG_BASE + 10; // 回执方号越界
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 12; // 卷已封存，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 卷为空
const E_REG_EMPTY: u16 = SEG_BASE + 14; // 登记簿为空

/// 登记簿定容。
pub const MAX_REG: usize = 24;

/// 域经卷定容。
pub const MAX_VOL: usize = 24;

/// 档位闭集（与 F0745 同口径：3=高 2=中 1=低）。
pub const TIER_HIGH: u8 = 3;
pub const TIER_MID: u8 = 2;
pub const TIER_LOW: u8 = 1;

/// 回执方闭集（2 方：移交方=1，接收方=2）。
pub const R_GIVER: u8 = 1;
pub const R_TAKER: u8 = 2;

// ── FNV-1a 64（卷哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_REG_DUP => "登记簿条目重复：主题唯一键",
        E_REG_FULL => "登记簿已满 24 条",
        E_REG_NOTFOUND => "登记簿条目不存在",
        E_TIER_BAD => "档位值非法：闭集 1..=3",
        E_LOW_MIXED => "低档混入卷：导出过滤被绕过，全卷作废",
        E_GHOST => "卷有簿无：导出物超出登记簿范围",
        E_MISSING => "簿有卷无：高/中档条目漏导",
        E_NO_RECEIPT => "零回执：先落接收方回执再导出（前置核）",
        E_RECEIPT_DUP => "回执重复：一方一回执",
        E_RECEIPT_BAD => "回执方号越界：1..=2",
        E_BAL => "守恒破裂：卷行数与登记数账不平",
        E_SEALED => "卷已封存：封存后只读",
        E_EMPTY => "卷为空",
        E_REG_EMPTY => "登记簿为空：先登记再导出",
        _ => "未知诊断码",
    }
}

// ── 档位判断 ─────────────────────────────────────────────────
/// 档位是否可导出（高/中可导，低不可）。
pub fn exportable_tier(tier: u8) -> bool {
    tier == TIER_HIGH || tier == TIER_MID
}

/// 档位中文名。
pub fn tier_zh(tier: u8) -> &'static str {
    match tier {
        TIER_HIGH => "高档",
        TIER_MID => "中档",
        TIER_LOW => "低档",
        _ => "非法档",
    }
}

// ── 登记簿条目 ───────────────────────────────────────────────
/// 登记簿条目：主题 + 档位（F0745 评级结果的投影）。
#[derive(Clone, Debug)]
pub struct RegEntry {
    /// 条目号（1 起连续）。
    pub entry_id: u32,
    /// 主题。
    pub topic: String,
    /// 档位（1..=3）。
    pub tier: u8,
    /// 证据数（≥1 才有效）。
    pub evids: u32,
}

impl RegEntry {
    /// 条目有效性。
    pub fn valid(&self) -> bool {
        !self.topic.is_empty() && (1..=3).contains(&self.tier) && self.evids >= 1
    }
}

// ── 回执行 ───────────────────────────────────────────────────
/// 回执行：回执方 + 接收码（1=收）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptRow {
    /// 回执方（1..=2）。
    pub party: u8,
    /// 接收码（1=收 0=拒）。
    pub accept: u8,
}

// ── 域经卷导出账（主体）──────────────────────────────────────
/// 移交包域经卷导出账：登记簿 → 回执前置核 → 高/中档导出 → 一致核 → 封存。
pub struct DomExpVolume {
    /// 登记簿。
    reg: [Option<RegEntry>; MAX_REG],
    reg_count: usize,
    /// 卷行（已导出条目号）。
    vol: [Option<u32>; MAX_VOL],
    vol_count: usize,
    /// 回执账。
    receipts: Vec<ReceiptRow>,
    /// 是否封存。
    sealed: bool,
    /// 卷哈希。
    sealed_hash: u64,
}

impl DomExpVolume {
    /// 开账。
    pub fn new() -> DomExpVolume {
        DomExpVolume {
            reg: [const { None }; MAX_REG],
            reg_count: 0,
            vol: [None; MAX_VOL],
            vol_count: 0,
            receipts: Vec::new(),
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 登记簿登记：主题唯一 + 档位闭集 + 证据数 ≥1。
    pub fn register(&mut self, topic: &str, tier: u8, evids: u32) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.reg_count >= MAX_REG {
            return Err(E_REG_FULL);
        }
        if topic.is_empty() {
            return Err(E_REG_NOTFOUND);
        }
        if tier == 0 || tier > TIER_HIGH {
            return Err(E_TIER_BAD);
        }
        if evids == 0 {
            return Err(E_REG_NOTFOUND);
        }
        let mut i = 0usize;
        while i < self.reg_count {
            if let Some(e) = &self.reg[i] {
                if e.topic == topic {
                    return Err(E_REG_DUP);
                }
            }
            i += 1;
        }
        let entry_id = (self.reg_count + 1) as u32;
        self.reg[self.reg_count] = Some(RegEntry {
            entry_id,
            topic: topic.to_string(),
            tier,
            evids,
        });
        self.reg_count += 1;
        Ok(entry_id)
    }

    /// 登记簿条目只读快照。
    pub fn get_reg(&self, entry_id: u32) -> Result<RegEntry, u16> {
        if entry_id == 0 || entry_id as usize > self.reg_count {
            return Err(E_REG_NOTFOUND);
        }
        match &self.reg[(entry_id - 1) as usize] {
            Some(e) => Ok(e.clone()),
            None => Err(E_REG_NOTFOUND),
        }
    }

    /// 登记簿条数。
    pub fn reg_count(&self) -> usize {
        self.reg_count
    }

    /// 回执落账：一方一回执。
    pub fn put_receipt(&mut self, party: u8, accept: bool) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if party != R_GIVER && party != R_TAKER {
            return Err(E_RECEIPT_BAD);
        }
        let mut i = 0usize;
        while i < self.receipts.len() {
            if self.receipts[i].party == party {
                return Err(E_RECEIPT_DUP);
            }
            i += 1;
        }
        self.receipts.push(ReceiptRow {
            party,
            accept: if accept { 1 } else { 0 },
        });
        Ok(())
    }

    /// 接收方回执读取：None=零回执。
    pub fn taker_receipt(&self) -> Option<bool> {
        let mut i = 0usize;
        while i < self.receipts.len() {
            if self.receipts[i].party == R_TAKER {
                return Some(self.receipts[i].accept == 1);
            }
            i += 1;
        }
        None
    }

    /// 回执前置核：接收方回执必须存在且为收。
    pub fn receipt_gate(&self) -> Result<(), u16> {
        match self.taker_receipt() {
            Some(true) => Ok(()),
            Some(false) => Err(E_NO_RECEIPT),
            None => Err(E_NO_RECEIPT),
        }
    }

    /// 导出：回执前置核 → 高/中档全量入卷（低档过滤）。
    pub fn export(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.reg_count == 0 {
            return Err(E_REG_EMPTY);
        }
        self.receipt_gate()?;
        if self.vol_count > 0 {
            return Err(E_BAL);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reg_count {
            let e = self.reg[i].as_ref().ok_or(E_REG_NOTFOUND)?;
            if !e.valid() {
                return Err(E_TIER_BAD);
            }
            if exportable_tier(e.tier) {
                if self.vol_count >= MAX_VOL {
                    return Err(E_BAL);
                }
                self.vol[self.vol_count] = Some(e.entry_id);
                self.vol_count += 1;
                n += 1;
            }
            i += 1;
        }
        if n == 0 {
            return Err(E_EMPTY);
        }
        Ok(n)
    }

    /// 卷行数。
    pub fn vol_count(&self) -> usize {
        self.vol_count
    }

    /// 低档零混入断言：卷内逐条档位必须可导。
    pub fn low_free_assert(&self) -> Result<(), u16> {
        if self.vol_count == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.vol_count {
            let id = self.vol[i].ok_or(E_GHOST)?;
            let e = self.get_reg(id)?;
            if !exportable_tier(e.tier) {
                return Err(E_LOW_MIXED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 与登记簿一致 100% 断言：
    /// 正向（卷→簿逐条在且档位可导）+ 反向（簿侧可导条目必在卷）。
    pub fn consistency_assert(&self) -> Result<(), u16> {
        if self.vol_count == 0 || self.reg_count == 0 {
            return Err(E_EMPTY);
        }
        // 正向：卷逐条可解引。
        let mut i = 0usize;
        while i < self.vol_count {
            let id = self.vol[i].ok_or(E_GHOST)?;
            if id == 0 || id as usize > self.reg_count {
                return Err(E_GHOST);
            }
            let e = self.get_reg(id)?;
            if !exportable_tier(e.tier) {
                return Err(E_LOW_MIXED);
            }
            i += 1;
        }
        // 反向：簿侧可导条目逐条在卷。
        let mut j = 0usize;
        while j < self.reg_count {
            let e = self.reg[j].as_ref().ok_or(E_REG_NOTFOUND)?;
            if exportable_tier(e.tier) {
                let mut hit = false;
                let mut k = 0usize;
                while k < self.vol_count {
                    if self.vol[k] == Some(e.entry_id) {
                        hit = true;
                    }
                    k += 1;
                }
                if !hit {
                    return Err(E_MISSING);
                }
            }
            j += 1;
        }
        // 守恒：卷行数 == 簿侧可导条目数。
        let mut exp = 0u32;
        let mut m = 0usize;
        while m < self.reg_count {
            if let Some(e) = &self.reg[m] {
                if exportable_tier(e.tier) {
                    exp += 1;
                }
            }
            m += 1;
        }
        if self.vol_count as u32 != exp {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 封存：一致核 + 零混入 → 卷哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.low_free_assert()?;
        self.consistency_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.vol_count {
            if let Some(id) = self.vol[i] {
                buf.extend_from_slice(id.to_le_bytes().as_slice());
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.receipts.len() {
            buf.push(self.receipts[j].party);
            buf.push(self.receipts[j].accept);
            j += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 卷哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 高/中/低三档登记计数。
    pub fn tier_tally(&self) -> (u32, u32, u32) {
        let mut hi = 0u32;
        let mut mid = 0u32;
        let mut low = 0u32;
        let mut i = 0usize;
        while i < self.reg_count {
            if let Some(e) = &self.reg[i] {
                match e.tier {
                    TIER_HIGH => hi += 1,
                    TIER_MID => mid += 1,
                    _ => low += 1,
                }
            }
            i += 1;
        }
        (hi, mid, low)
    }

    /// 卷行渲染：卷内逐条"卷位|条目号|主题|档位"。
    pub fn volume_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 域经卷 · 高中档快照 ===".to_string());
        let mut i = 0usize;
        while i < self.vol_count {
            if let Some(id) = self.vol[i] {
                let mut line = String::from("V");
                line.push_str(&itoa((i + 1) as u64));
                line.push_str("|E");
                line.push_str(&itoa(id as u64));
                line.push('|');
                match self.get_reg(id) {
                    Ok(e) => {
                        line.push_str(&e.topic);
                        line.push('|');
                        line.push_str(tier_zh(e.tier));
                    }
                    Err(_) => line.push_str("?"),
                }
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 导出预览：不写卷，仅返回将被导出的条目号（消费方预检口）。
    pub fn export_preview(&self) -> Result<Vec<u32>, u16> {
        if self.reg_count == 0 {
            return Err(E_REG_EMPTY);
        }
        self.receipt_gate()?;
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.reg_count {
            if let Some(e) = &self.reg[i] {
                if exportable_tier(e.tier) {
                    out.push(e.entry_id);
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 卷内条目号只读快照。
    pub fn vol_ids(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.vol_count {
            if let Some(id) = self.vol[i] {
                out.push(id);
            }
            i += 1;
        }
        out
    }

    /// 回执账行渲染。
    pub fn receipt_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 回执前置核账 ===".to_string());
        let mut i = 0usize;
        while i < self.receipts.len() {
            let r = &self.receipts[i];
            let mut line = String::from("R|方");
            line.push_str(&itoa(r.party as u64));
            line.push('|');
            line.push_str(if r.accept == 1 { "接收" } else { "拒收" });
            lines.push(line);
            i += 1;
        }
        lines
    }

    /// 证据数合计（卷内条目证据规模盘点）。
    pub fn vol_evid_total(&self) -> Result<u32, u16> {
        if self.vol_count == 0 {
            return Err(E_EMPTY);
        }
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.vol_count {
            let id = self.vol[i].ok_or(E_GHOST)?;
            let e = self.get_reg(id)?;
            s += e.evids;
            i += 1;
        }
        Ok(s)
    }
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：高/中档导出 + 低档零混入 + 回执前置 + 一致 100%。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut dv = DomExpVolume::new();
    // 登记 5 条：2 高 / 2 中 / 1 低。
    let e1 = dv.register("哈希先行校验", TIER_HIGH, 3)?;
    let e2 = dv.register("定容账本模式", TIER_HIGH, 2)?;
    let e3 = dv.register("段位诊断码", TIER_MID, 2)?;
    let e4 = dv.register("整数化评分", TIER_MID, 1)?;
    let e5 = dv.register("临时打样脚本", TIER_LOW, 1)?;
    let _ = (e1, e2, e3, e4, e5);
    if dv.reg_count() != 5 {
        return Err(SEG_BASE);
    }
    // 零回执导出被拒（前置核）。
    match dv.export() {
        Err(c) if c == E_NO_RECEIPT => {}
        _ => return Err(SEG_BASE),
    }
    // 回执拒收 → 仍拒。
    dv.put_receipt(R_TAKER, false)?;
    match dv.export() {
        Err(c) if c == E_NO_RECEIPT => {}
        _ => return Err(SEG_BASE),
    }
    // 回执改收（换账本模拟）→ 导出 4 条。
    let mut dv2 = DomExpVolume::new();
    let _ = dv2.register("哈希先行校验", TIER_HIGH, 3)?;
    let _ = dv2.register("定容账本模式", TIER_HIGH, 2)?;
    let _ = dv2.register("段位诊断码", TIER_MID, 2)?;
    let _ = dv2.register("整数化评分", TIER_MID, 1)?;
    let _ = dv2.register("临时打样脚本", TIER_LOW, 1)?;
    dv2.put_receipt(R_GIVER, true)?;
    dv2.put_receipt(R_TAKER, true)?;
    let n = dv2.export()?;
    if n != 4 {
        return Err(SEG_BASE);
    }
    dv2.low_free_assert()?;
    dv2.consistency_assert()?;
    // 三档计数：2 高 / 2 中 / 1 低。
    let (hi, mid, low) = dv2.tier_tally();
    if hi != 2 || mid != 2 || low != 1 {
        return Err(SEG_BASE);
    }
    let h = dv2.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 取数口：预览、卷行、回执行、证据合计。
    let pv = dv2.export_preview()?;
    if pv.len() != 4 {
        return Err(SEG_BASE);
    }
    let ids = dv2.vol_ids();
    if ids.len() != 4 || ids[0] != 1 || ids[3] != 4 {
        return Err(SEG_BASE);
    }
    let vl = dv2.volume_lines();
    if vl.len() != 5 {
        return Err(SEG_BASE);
    }
    let rl = dv2.receipt_lines();
    if rl.len() != 3 {
        return Err(SEG_BASE);
    }
    let ev = dv2.vol_evid_total()?;
    if ev != 8 {
        return Err(SEG_BASE);
    }
    // 封存后导出拒绝。
    match dv2.export() {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：档位越界。
    let mut dv3 = DomExpVolume::new();
    match dv3.register("越界条目样本", 4, 1) {
        Err(c) if c == E_TIER_BAD => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：低档混入模拟（直改卷行后断言必拦）。
    let mut dv4 = DomExpVolume::new();
    let _ = dv4.register("甲", TIER_HIGH, 1)?;
    let _ = dv4.register("乙", TIER_LOW, 1)?;
    dv4.put_receipt(R_TAKER, true)?;
    let _ = dv4.export()?;
    // 手工混入低档条目 2（模拟攻击路径）。
    dv4.vol[dv4.vol_count] = Some(2);
    dv4.vol_count += 1;
    match dv4.low_free_assert() {
        Err(c) if c == E_LOW_MIXED => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
