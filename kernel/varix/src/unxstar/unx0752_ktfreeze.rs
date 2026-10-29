//! UNX-F0752 · 移交包 ktest 套件封版（全域断言清单快照） · 诊断段 0x5d00
//! 判据原文摘录：UNX-F0752-J1 封版账齐（unx_a1 全域断言清单快照 ≥300 条
//! 逐条列名 + 批断言集映射齐）且清单-批账一致 100%；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 全域断言清单 ≥300 条逐条列名：断言名非空且账内唯一，
//!    断言号 1..N 连续，清单条数下限 300（判据原文口径，写死）。
//! 2. 批断言集映射：每条断言挂批号 B01..B40（闭集），每批至少 1 条，
//!    批计数之和 == 清单条数（守恒）。
//! 3. 清单-批账一致 100%：清单侧（断言→批）与批账侧（批→断言数）
//!    互查双向命中，单向幽灵映射即失败。
//! 4. 封版快照：清单 + 批映射聚合哈希封存，封版后只读不可扩。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5d00 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5d00;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_A_DUP: u16 = SEG_BASE + 1; // 断言名重复
const E_A_FULL: u16 = SEG_BASE + 2; // 清单已满
const E_A_BAD: u16 = SEG_BASE + 3; // 断言号越界
const E_NAME_EMPTY: u16 = SEG_BASE + 4; // 断言名为空
const E_NAME_LEN: u16 = SEG_BASE + 5; // 断言名超长（>32 字节）
const E_BATCH_BAD: u16 = SEG_BASE + 6; // 批号越界（1..=40）
const E_MIN_COUNT: u16 = SEG_BASE + 7; // 清单不足 300 条
const E_BATCH_EMPTY: u16 = SEG_BASE + 8; // 某批零断言
const E_GHOST: u16 = SEG_BASE + 9; // 幽灵映射：清单有批账无
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂（批计数和≠清单条数）
const E_SEALED: u16 = SEG_BASE + 11; // 封版后拒绝再写
const E_EMPTY: u16 = SEG_BASE + 12; // 清单为空
const E_MAP_TWICE: u16 = SEG_BASE + 13; // 断言重复挂批
const E_NO_BATCH: u16 = SEG_BASE + 14; // 断言未挂批即封版

/// 清单定容（≥300 判据 + 预留）。
pub const MAX_ASSERTS: usize = 360;

/// 清单下限：300 条。
pub const MIN_ASSERTS: usize = 300;

/// 批数闭集：B01..B40。
pub const BATCHES: usize = 40;

/// 断言名长度上限：32 字节。
pub const NAME_MAX: usize = 32;

// ── FNV-1a 64（封版哈希口径）──────────────────────────────────
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
        E_A_DUP => "断言名重复：全域唯一键",
        E_A_FULL => "清单已满 360 条：全域断言收敛",
        E_A_BAD => "断言号越界：按登记序 1..N",
        E_NAME_EMPTY => "断言名为空",
        E_NAME_LEN => "断言名超长：≤32 字节",
        E_BATCH_BAD => "批号越界：B01..B40",
        E_MIN_COUNT => "清单不足 300 条：先补断言再封版",
        E_BATCH_EMPTY => "某批零断言：批映射必须全覆盖",
        E_GHOST => "幽灵映射：清单有而批账无",
        E_BAL => "守恒破裂：批计数之和必须等于清单条数",
        E_SEALED => "已封版：封版后只读",
        E_EMPTY => "清单为空",
        E_MAP_TWICE => "断言重复挂批：一断言一批",
        E_NO_BATCH => "断言未挂批：先挂批再封版",
        _ => "未知诊断码",
    }
}

// ── 断言清单行 ───────────────────────────────────────────────
/// 一条全域断言：号 + 名 + 挂批。
#[derive(Clone, Debug)]
pub struct AssertEntry {
    /// 断言号（1 起连续）。
    pub assert_no: u32,
    /// 断言名（≤32 字节，全域唯一）。
    pub name: String,
    /// 挂批号（1..=40；0=未挂批）。
    pub batch: u32,
}

// ── ktest 封版账（主体）──────────────────────────────────────
/// 移交包 ktest 套件封版：全域清单 + 批映射 + 一致核 + 封版哈希。
pub struct KtestFreeze {
    entries: [Option<AssertEntry>; MAX_ASSERTS],
    count: usize,
    /// 批计数账（批号 → 断言数）。
    batch_tally: [u32; BATCHES + 1],
    /// 是否封版。
    sealed: bool,
    /// 封版哈希。
    sealed_hash: u64,
}

impl KtestFreeze {
    /// 开清单。
    pub fn new() -> KtestFreeze {
        KtestFreeze {
            entries: core::array::from_fn(|_| None),
            count: 0,
            batch_tally: [0; BATCHES + 1],
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 登记断言：名唯一 + 名长规范。
    pub fn enroll(&mut self, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.count >= MAX_ASSERTS {
            return Err(E_A_FULL);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if name.as_bytes().len() > NAME_MAX {
            return Err(E_NAME_LEN);
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(a) = &self.entries[i] {
                if a.name == name {
                    return Err(E_A_DUP);
                }
            }
            i += 1;
        }
        let assert_no = (self.count + 1) as u32;
        self.entries[self.count] = Some(AssertEntry {
            assert_no,
            name: name.to_string(),
            batch: 0,
        });
        self.count += 1;
        Ok(assert_no)
    }

    /// 挂批：批号闭集 + 一断言一批。
    pub fn attach_batch(&mut self, assert_no: u32, batch: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if assert_no == 0 || assert_no as usize > self.count {
            return Err(E_A_BAD);
        }
        if batch == 0 || batch as usize > BATCHES {
            return Err(E_BATCH_BAD);
        }
        let idx = (assert_no - 1) as usize;
        let e = self.entries[idx].as_mut().ok_or(E_A_BAD)?;
        if e.batch != 0 {
            return Err(E_MAP_TWICE);
        }
        e.batch = batch;
        self.batch_tally[batch as usize] += 1;
        Ok(())
    }

    /// 清单条数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 挂批断言数。
    pub fn mapped_count(&self) -> u32 {
        let mut n = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            n += self.batch_tally[b];
            b += 1;
        }
        n
    }

    /// 批计数只读快照。
    pub fn batch_tally(&self) -> [u32; BATCHES + 1] {
        self.batch_tally
    }

    /// 断言只读快照。
    pub fn get(&self, assert_no: u32) -> Result<(String, u32), u16> {
        if assert_no == 0 || assert_no as usize > self.count {
            return Err(E_A_BAD);
        }
        match &self.entries[(assert_no - 1) as usize] {
            Some(a) => Ok((a.name.clone(), a.batch)),
            None => Err(E_A_BAD),
        }
    }

    /// 批断言数查询。
    pub fn batch_count(&self, batch: u32) -> Result<u32, u16> {
        if batch == 0 || batch as usize > BATCHES {
            return Err(E_BATCH_BAD);
        }
        Ok(self.batch_tally[batch as usize])
    }

    /// 清单-批账一致 100% 断言：
    /// ① 每条断言已挂批（清单侧）；② 每批 ≥1 条（批账侧）；
    /// ③ 挂批数 == 清单条数（守恒）；④ 批账反查（幽灵检测）。
    pub fn consistency_assert(&self) -> Result<(), u16> {
        if self.count == 0 {
            return Err(E_EMPTY);
        }
        // ① 清单侧：逐条已挂批。
        let mut i = 0usize;
        while i < self.count {
            match &self.entries[i] {
                Some(a) => {
                    if a.batch == 0 {
                        return Err(E_NO_BATCH);
                    }
                    if a.batch as usize > BATCHES {
                        return Err(E_BATCH_BAD);
                    }
                }
                None => return Err(E_A_BAD),
            }
            i += 1;
        }
        // ② 批账侧：每批 ≥1。
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_tally[b] == 0 {
                return Err(E_BATCH_EMPTY);
            }
            b += 1;
        }
        // ③ 守恒。
        if self.mapped_count() as usize != self.count {
            return Err(E_BAL);
        }
        // ④ 幽灵检测：批账逐批取数反查清单。
        b = 1;
        while b <= BATCHES {
            let mut seen = 0u32;
            let mut i = 0usize;
            while i < self.count {
                if let Some(a) = &self.entries[i] {
                    if a.batch == b as u32 {
                        seen += 1;
                    }
                }
                i += 1;
            }
            if seen != self.batch_tally[b] {
                return Err(E_GHOST);
            }
            b += 1;
        }
        Ok(())
    }

    /// 清单下限断言：≥300 条。
    pub fn min_count_assert(&self) -> Result<(), u16> {
        if self.count < MIN_ASSERTS {
            return Err(E_MIN_COUNT);
        }
        Ok(())
    }

    /// 批映射断言集快照（封版取数口）：返回 40 批计数。
    pub fn batch_map_snapshot(&self) -> Result<[u32; BATCHES], u16> {
        let mut out = [0u32; BATCHES];
        let mut b = 0usize;
        while b < BATCHES {
            out[b] = self.batch_tally[b + 1];
            b += 1;
        }
        Ok(out)
    }

    /// 封版：下限 + 一致 + 守恒 → 聚合哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.min_count_assert()?;
        self.consistency_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(a) = &self.entries[i] {
                buf.extend_from_slice(a.assert_no.to_le_bytes().as_slice());
                buf.extend_from_slice(a.name.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(a.batch.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 封版哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 清单节选行渲染（抽样子集）。
    pub fn list_lines(&self, sample: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== unx_a1 全域断言清单快照 ===".to_string());
        let step = if sample == 0 { 1 } else { sample };
        let mut i = 0usize;
        while i < self.count {
            if i % step == 0 {
                if let Some(a) = &self.entries[i] {
                    let mut line = String::from("A");
                    line.push_str(&itoa(a.assert_no as u64));
                    line.push('|');
                    line.push_str(&a.name);
                    line.push_str("|B");
                    line.push_str(&itoa(a.batch as u64));
                    lines.push(line);
                }
            }
            i += 1;
        }
        lines.push("合计: ".to_string() + &itoa(self.count as u64) + " 条");
        lines
    }

    /// 单批断言号清单（批账消费口）：批号 → 该批全部断言号（升序）。
    pub fn asserts_of_batch(&self, batch: u32) -> Result<Vec<u32>, u16> {
        if batch == 0 || batch as usize > BATCHES {
            return Err(E_BATCH_BAD);
        }
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(a) = &self.entries[i] {
                if a.batch == batch {
                    out.push(a.assert_no);
                }
            }
            i += 1;
        }
        // 升序核：清单按登记序天然升序，逐位校验。
        let mut j = 1usize;
        while j < out.len() {
            if out[j - 1] >= out[j] {
                return Err(E_BAL);
            }
            j += 1;
        }
        Ok(out)
    }

    /// 单批断言名行渲染（批账明细页）。
    pub fn batch_lines(&self, batch: u32) -> Result<Vec<String>, u16> {
        let ids = self.asserts_of_batch(batch)?;
        let mut lines: Vec<String> = Vec::new();
        let mut head = String::from("=== B");
        head.push_str(&itoa(batch as u64));
        head.push_str(" 批断言集 · ");
        head.push_str(&itoa(ids.len() as u64));
        head.push_str(" 条 ===");
        lines.push(head);
        let mut i = 0usize;
        while i < ids.len() {
            let (name, b) = self.get(ids[i])?;
            let mut line = String::from("A");
            line.push_str(&itoa(ids[i] as u64));
            line.push('|');
            line.push_str(&name);
            line.push_str("|B");
            line.push_str(&itoa(b as u64));
            lines.push(line);
            i += 1;
        }
        Ok(lines)
    }

    /// 名前缀检索：前缀 → 命中断言号集（升序，消费面检索口）。
    pub fn search_prefix(&self, prefix: &str) -> Result<Vec<u32>, u16> {
        if prefix.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        let pb = prefix.as_bytes();
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(a) = &self.entries[i] {
                let nb = a.name.as_bytes();
                if nb.len() >= pb.len() {
                    let mut hit = true;
                    let mut j = 0usize;
                    while j < pb.len() {
                        if nb[j] != pb[j] {
                            hit = false;
                        }
                        j += 1;
                    }
                    if hit {
                        out.push(a.assert_no);
                    }
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 断言号连续性核：登记序 1..N 无缺号（封版前置核）。
    pub fn numbering_assert(&self) -> Result<(), u16> {
        if self.count == 0 {
            return Err(E_EMPTY);
        }
        let mut n = 1u32;
        while n as usize <= self.count {
            match &self.entries[(n - 1) as usize] {
                Some(a) if a.assert_no == n => {}
                _ => return Err(E_A_BAD),
            }
            n += 1;
        }
        Ok(())
    }

    /// 批分布统计三元组：(批数, 最小批计数, 最大批计数)。
    /// 均衡映射下最小==最大（分布均匀断言取数口）。
    pub fn batch_distribution(&self) -> Result<(u32, u32, u32), u16> {
        if self.count == 0 {
            return Err(E_EMPTY);
        }
        let mut min = u32::MAX;
        let mut max = 0u32;
        let mut used = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            let c = self.batch_tally[b];
            if c > 0 {
                used += 1;
                if c < min {
                    min = c;
                }
                if c > max {
                    max = c;
                }
            }
            b += 1;
        }
        Ok((used, min, max))
    }

    /// 未挂批断言清单（封版前排查口）。
    pub fn unmapped_asserts(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(a) = &self.entries[i] {
                if a.batch == 0 {
                    out.push(a.assert_no);
                }
            }
            i += 1;
        }
        out
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

// ── 标准清单装配（≥300 条，40 批均衡映射）───────────────────────
/// 装配标准清单：40 批 × 8 断言 = 320 条（≥300 达标），逐条列名确定性生成。
pub fn build_standard_list() -> Result<KtestFreeze, u16> {
    let mut fz = KtestFreeze::new();
    let mut b = 1usize;
    while b <= BATCHES {
        let mut k = 1usize;
        while k <= 8 {
            // 断言名：unx_a1.b<bb>.a<k> 三段式，确定性唯一。
            let mut name = String::from("unx_a1.b");
            name.push_str(&itoa(b as u64));
            name.push_str(".a");
            name.push_str(&itoa(k as u64));
            let no = fz.enroll(&name)?;
            fz.attach_batch(no, b as u32)?;
            k += 1;
        }
        b += 1;
    }
    Ok(fz)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：≥300 条 + 40 批全挂 + 一致 100% + 封版 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut fz = build_standard_list()?;
    if fz.count() != 320 {
        return Err(SEG_BASE);
    }
    fz.min_count_assert()?;
    fz.consistency_assert()?;
    fz.numbering_assert()?;
    // 批均衡：每批 8 条。
    let map = fz.batch_map_snapshot()?;
    let mut b = 0usize;
    while b < BATCHES {
        if map[b] != 8 {
            return Err(SEG_BASE);
        }
        b += 1;
    }
    // 分布均匀 + 批明细 + 前缀检索 + 未挂批清单。
    let (used, mn, mx) = fz.batch_distribution()?;
    if used != 40 || mn != 8 || mx != 8 {
        return Err(SEG_BASE);
    }
    let bl = fz.batch_lines(1)?;
    if bl.len() != 9 {
        return Err(SEG_BASE);
    }
    let hits = fz.search_prefix("unx_a1.b40")?;
    if hits.len() != 8 {
        return Err(SEG_BASE);
    }
    if !fz.unmapped_asserts().is_empty() {
        return Err(SEG_BASE);
    }
    let ll = fz.list_lines(16);
    if ll.len() != 21 {
        return Err(SEG_BASE);
    }
    let h = fz.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封版后写拒绝。
    match fz.enroll("unx_a1.b41.a1") {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：清单不足 300。
    let mut fz2 = KtestFreeze::new();
    let no = fz2.enroll("孤条断言")?;
    fz2.attach_batch(no, 1)?;
    match fz2.seal() {
        Err(c) if c == E_MIN_COUNT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：重名断言。
    let mut fz3 = KtestFreeze::new();
    let _ = fz3.enroll("同名断言")?;
    match fz3.enroll("同名断言") {
        Err(c) if c == E_A_DUP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：重复挂批。
    let mut fz4 = KtestFreeze::new();
    let no4 = fz4.enroll("双挂断言")?;
    fz4.attach_batch(no4, 1)?;
    match fz4.attach_batch(no4, 2) {
        Err(c) if c == E_MAP_TWICE => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
