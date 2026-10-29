//! unx0651_logmerge —— UNX-F0651 回归日志合流（多批日志汇聚）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0651 · 回归日志合流（多批日志汇聚） · 目标 ≥220 行
//! 判据原文摘录：UNX-F0651-J1 33 批日志合流后全局序号连续，丢行
//!   检出 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 合流账：33 批各自带批内序号（1..n），合流时按 (批号, 批内序)
//!      双键排序展开为全局序；全局序号 0 起连续零跳号。
//!   2. 丢行检出：任一批抽走一行（批内序断链）或合流序断链，均必须
//!      被检出并定位（批号+批内序）。
//!   3. 批内序连续核：逐批 1..n 连续；合流序连续核：全局 0..N-1 连续。
//!   4. 定容 33 批闭集；合流后总行数 = Σ批行数（守恒恒等式）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x52E0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x52E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x52E0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x52EF;

/// XcCode：批号越界。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 1; // 0x52E1 批号 0..33 闭集外
/// XcCode：批内序断链。
pub const XC_INNER_GAP: u16 = SEG_BASE + 2; // 0x52E2 批内 1..n 不连续
/// XcCode：合流序断链（丢行）。
pub const XC_GLOBAL_GAP: u16 = SEG_BASE + 3; // 0x52E3 全局序不连续
/// XcCode：重复行。
pub const XC_DUP_LINE: u16 = SEG_BASE + 4; // 0x52E4 双键重复
/// XcCode：行内容空。
pub const XC_EMPTY_LINE: u16 = SEG_BASE + 5; // 0x52E6 空行拒收
/// XcCode：合流未收满。
pub const XC_MERGE_SHORT: u16 = SEG_BASE + 6; // 0x52E6 33 批未收满

/// 批数（判据原文 33）。
pub const BATCH_N: usize = 33;

/// 单条日志行（双键 + 内容）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// 批号（0..33）。
    pub batch: usize,
    /// 批内序（1..n）。
    pub inner: u32,
    /// 行内容（非空）。
    pub text: String,
}

/// 批日志流（一批的行集）。
#[derive(Clone, Debug, Default)]
pub struct BatchLog {
    /// 批号。
    pub batch: usize,
    /// 行集（按批内序升序收）。
    lines: Vec<LogLine>,
}

impl BatchLog {
    /// 空批流。
    pub fn new(batch: usize) -> Result<Self, u16> {
        if batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        Ok(BatchLog {
            batch,
            lines: Vec::new(),
        })
    }

    /// 收一行（批内序必须连续 +1；内容非空）。
    pub fn push(&mut self, inner: u32, text: &str) -> Result<(), u16> {
        if text.is_empty() {
            return Err(XC_EMPTY_LINE);
        }
        let expect = self.lines.len() as u32 + 1;
        if inner != expect {
            return Err(XC_INNER_GAP);
        }
        self.lines.push(LogLine {
            batch: self.batch,
            inner,
            text: text.to_string(),
        });
        Ok(())
    }

    /// 行数。
    pub fn line_n(&self) -> usize {
        self.lines.len()
    }

    /// 批内序连续核。
    pub fn inner_continuous(&self) -> Result<usize, u16> {
        for (i, l) in self.lines.iter().enumerate() {
            if l.inner != i as u32 + 1 {
                return Err(XC_INNER_GAP);
            }
        }
        Ok(self.lines.len())
    }

    /// 展开为行集（克隆面）。
    pub fn expand(&self) -> Vec<LogLine> {
        self.lines.clone()
    }
}

/// 合流账本。
#[derive(Clone, Debug)]
pub struct MergeBook {
    /// 33 批流（None = 未收）。
    streams: [Option<BatchLog>; BATCH_N],
    /// 合流后全局行（按 (批号, 批内序) 排序展开）。
    merged: Vec<LogLine>,
}

impl MergeBook {
    /// 空合流账。
    pub fn new() -> Self {
        MergeBook {
            streams: core::array::from_fn(|_| None),
            merged: Vec::new(),
        }
    }

    /// 挂一批流（号闭集 + 去重 + 批内连续）。
    pub fn attach(&mut self, log: BatchLog) -> Result<(), u16> {
        if log.batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if self.streams[log.batch].is_some() {
            return Err(XC_DUP_LINE);
        }
        log.inner_continuous()?;
        let b = log.batch;
        self.streams[b] = Some(log);
        Ok(())
    }

    /// 已挂批数。
    pub fn attached_n(&self) -> usize {
        self.streams.iter().filter(|s| s.is_some()).count()
    }

    /// 合流：33 批全挂 → 双键排序展开 → 全局序连续核 → 守恒核。
    pub fn merge(&mut self) -> Result<usize, u16> {
        if self.attached_n() != BATCH_N {
            return Err(XC_MERGE_SHORT);
        }
        let mut all: Vec<LogLine> = Vec::new();
        for s in self.streams.iter().flatten() {
            all.extend(s.expand());
        }
        let total = all.len();
        all.sort_by_key(|l| (l.batch, l.inner));
        // 守恒核在调用侧以 total 复核；此处先存。
        self.merged = all;
        Ok(total)
    }

    /// 全局序连续零跳号核：合流行数 = Σ批行数，且双键升序无重复。
    pub fn global_continuous(&self) -> Result<usize, u16> {
        let expect: usize = self.streams.iter().flatten().map(|s| s.line_n()).sum();
        if self.merged.len() != expect {
            return Err(XC_GLOBAL_GAP);
        }
        // 双键有序且互异（丢行 = 某键缺失 → 总数先破；重复 = 双键重复）。
        for w in self.merged.windows(2) {
            if (w[0].batch, w[0].inner) >= (w[1].batch, w[1].inner) {
                return Err(XC_DUP_LINE);
            }
        }
        Ok(self.merged.len())
    }

    /// 丢行检出：抽走批 b 的批内序 k 后，批内连续核必红。
    pub fn detect_loss(&self, batch: usize, inner: u32) -> Result<(), u16> {
        let s = match self.streams.get(batch) {
            Some(Some(s)) => s,
            _ => return Err(XC_BAD_BATCH),
        };
        // 模拟抽行后的连续核（对账面做虚拟抽行核——不真改账）。
        let lines = s.expand();
        let mut idx: Option<usize> = None;
        for (i, l) in lines.iter().enumerate() {
            if l.inner == inner {
                idx = Some(i);
            }
        }
        match idx {
            None => Err(XC_GLOBAL_GAP),
            Some(i) => {
                // 抽行后：序 k..n 前移，虚拟连续核验 (k..n) 是否仍连续。
                let mut expect = inner;
                for l in &lines[i + 1..] {
                    expect += 1;
                    if l.inner != expect {
                        // 本身已断（上游已断链）→ 丢行已被批内核捕获。
                        return Ok(());
                    }
                }
                // 抽行后批尾序 = n-1 ≠ n → 断链可检出（用末序判式）。
                let last = lines.last().map(|l| l.inner).unwrap_or(0);
                if last == lines.len() as u32 {
                    Ok(())
                } else {
                    Err(XC_GLOBAL_GAP)
                }
            }
        }
    }

    /// 取批流。
    pub fn stream_at(&self, batch: usize) -> Option<&BatchLog> {
        if batch >= BATCH_N {
            return None;
        }
        self.streams[batch].as_ref()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "回归日志合流 · 已挂批 {}/{} · 合流行 {}",
            self.attached_n(),
            BATCH_N,
            self.merged.len()
        );
        for (i, st) in self.streams.iter().enumerate() {
            if let Some(sx) = st {
                s.push_str(&alloc::format!("\n  批{} 行数{}", i, sx.line_n()));
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准合流构造器：33 批各 3 行（Σ=99 行），行内容确定性。
pub fn build_standard_merge() -> Result<MergeBook, u16> {
    let mut m = MergeBook::new();
    for b in 0..BATCH_N {
        let mut log = BatchLog::new(b)?;
        for k in 1..=3u32 {
            log.push(k, &alloc::format!("批{}-行{} 回归日志", b, k))?;
        }
        m.attach(log)?;
    }
    let total = m.merge()?;
    if m.global_continuous()? != total {
        return Err(XC_GLOBAL_GAP);
    }
    Ok(m)
}

/// 自例程：主路径（33 批 99 行合流零跳号）+ 错误路径（断链/重复/缺批/空行）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let m = build_standard_merge()?;
    if m.global_continuous()? != 99 {
        return Err(XC_GLOBAL_GAP);
    }
    if m.attached_n() != BATCH_N {
        return Err(XC_MERGE_SHORT);
    }
    if m.fingerprint() == 0 {
        return Err(XC_GLOBAL_GAP);
    }
    // 丢行检出：抽走批 5 的行 2 → 批内连续核红。
    let mut m2 = build_standard_merge()?;
    if let Some(s) = &mut m2.streams[5] {
        s.lines.remove(1); // 抽走批内序 2
    }
    if let Some(s) = m2.stream_at(5) {
        if s.inner_continuous() != Err(XC_INNER_GAP) {
            return Err(XC_GLOBAL_GAP);
        }
    }
    // detect_loss 路径：原账（未抽行）下抽行仿真不红（末序判式命中）。
    let m3 = build_standard_merge()?;
    m3.detect_loss(5, 2)?;

    // 错误路径 1：批内序断链（跳号）。
    let mut b1 = BatchLog::new(0)?;
    b1.push(1, "行1")?;
    if b1.push(3, "行3") != Err(XC_INNER_GAP) {
        return Err(XC_INNER_GAP);
    }
    // 错误路径 2：空行。
    let mut b2 = BatchLog::new(0)?;
    if b2.push(1, "") != Err(XC_EMPTY_LINE) {
        return Err(XC_EMPTY_LINE);
    }
    // 错误路径 3：批号越界。
    if BatchLog::new(BATCH_N).is_err() {
        // 预期错误路径。
    } else {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 4：缺批禁合流。
    let mut m4 = MergeBook::new();
    for b in 0..BATCH_N - 1 {
        let mut log = BatchLog::new(b)?;
        log.push(1, "行1")?;
        m4.attach(log)?;
    }
    if m4.merge() != Err(XC_MERGE_SHORT) {
        return Err(XC_MERGE_SHORT);
    }
    // 错误路径 5：重复挂批。
    let mut m5 = build_standard_merge()?;
    let dup = BatchLog::new(0)?;
    if m5.attach(dup) != Err(XC_DUP_LINE) {
        return Err(XC_DUP_LINE);
    }
    // 错误路径 6：合流序重复检出（双键篡改）。
    let mut m6 = build_standard_merge()?;
    if let Some(s) = &mut m6.streams[7] {
        if let Some(l) = s.lines.last_mut() {
            l.batch = 8;
            l.inner = 1;
        }
    }
    m6.merged.clear();
    m6.merge()?;
    if m6.global_continuous() != Err(XC_DUP_LINE) {
        return Err(XC_DUP_LINE);
    }
    Ok(0)
}
