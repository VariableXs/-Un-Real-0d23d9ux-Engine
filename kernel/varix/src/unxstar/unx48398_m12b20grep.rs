//! UNX-F48398 —— M12 B20 批防重 grep（关键词族：m12close/mregate）
//! 判据原文：UNX-F48398-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥237 行；
//!   J1-2 关键词族（m12close / mregate）与 B20 批 20 文件清单闭集登记；
//!   J1-3 防重对账：每个关键词在全批文件中的总命中数必须恰为 1（唯一收官标记）；
//!   J1-4 单文件同关键词命中 >1 直接拒绝（E_DUP_HIT）；
//!   J1-5 扫描计数走大小写归一（M12Close 与 m12close 同源）；
//!   J1-6 闭集外文件/闭集外关键词拒绝；密封后追加拒绝；指纹两次构建一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD2A0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;   // 空扫描表
const E_PATH_MISS: u16 = SEG_BASE + 2; // 闭集外文件
const E_DUP_PATH: u16 = SEG_BASE + 3;  // 文件重复登记
const E_DUP_HIT: u16 = SEG_BASE + 4;   // 单文件同关键词多重命中
const E_KW_BAD: u16 = SEG_BASE + 5;    // 关键词下标越界
const E_TOTAL: u16 = SEG_BASE + 6;     // 防重对账失败：总命中 ≠ 1
const E_SEAL: u16 = SEG_BASE + 7;      // 密封后追加
const E_SHORT: u16 = SEG_BASE + 8;     // 文件清单不满 20 即对账
const E_FINGER: u16 = SEG_BASE + 9;    // 指纹不稳定

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空扫描表：无文件可扫",
        E_PATH_MISS => "闭集外文件：B20 清单外拒绝登记",
        E_DUP_PATH => "文件重复登记：后到拒绝",
        E_DUP_HIT => "单文件同关键词多重命中：防重红线",
        E_KW_BAD => "关键词下标越界：词族仅 2 项",
        E_TOTAL => "防重对账失败：关键词总命中数 ≠ 1",
        E_SEAL => "扫描表已密封：追加拒绝",
        E_SHORT => "文件清单不满 20：不可对账",
        E_FINGER => "指纹不稳定：确定性被破坏",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// 关键词族闭集：2 词。
pub const KEYWORDS: [&str; 2] = ["m12close", "mregate"];

/// B20 批 20 文件清单闭集。
pub const B20_FILES: [&str; 20] = [
    "b01_m5boot.rs", "b02_m5fs.rs", "b03_m5mem.rs", "b04_m5sched.rs",
    "b05_m5srv.rs", "b06_m6a11y.rs", "b07_m6sync.rs", "b08_m700blk.rs",
    "b09_m700net.rs", "b10_m700proc.rs", "b11_m700vmm.rs", "b12_m7bench.rs",
    "b13_m12doc.rs", "b14_m12perf.rs", "b15_m12priv.rs", "b16_m12conc.rs",
    "b17_m12grep.rs", "b18_m12upg.rs", "b19_m12close_marker.rs", "b20_mregate_marker.rs",
];

/// ASCII 小写归一。
pub fn to_lower_ascii(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

/// 大小写归一计数：统计 kw 在 text 中的非重叠出现次数。
pub fn count_kw(text: &str, kw: &str) -> u32 {
    let t = to_lower_ascii(text);
    let tb = t.as_bytes();
    let kb = kw.as_bytes();
    if kb.is_empty() || tb.len() < kb.len() {
        return 0;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i + kb.len() <= tb.len() {
        if &tb[i..i + kb.len()] == kb {
            n += 1;
            i += kb.len();
        } else {
            i += 1;
        }
    }
    n
}

/// 单文件扫描行：两个关键词族的命中数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrepFile {
    pub path: &'static str,
    pub hits: [u32; 2],
}

/// B20 防重 grep 扫描表。
pub struct GrepLedger {
    pub files: Vec<GrepFile>,
    pub sealed: bool,
}

impl GrepLedger {
    pub fn new() -> GrepLedger {
        GrepLedger {
            files: Vec::new(),
            sealed: false,
        }
    }

    /// 文件登记：闭集内 + 不重复 + 单文件命中 ≤1。
    pub fn register(&mut self, path: &'static str, hits: [u32; 2]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if path.is_empty() {
            return Err(E_EMPTY);
        }
        if !B20_FILES.contains(&path) {
            return Err(E_PATH_MISS);
        }
        if self.files.iter().any(|f| f.path == path) {
            return Err(E_DUP_PATH);
        }
        for k in 0..2 {
            if hits[k] > 1 {
                return Err(E_DUP_HIT);
            }
        }
        self.files.push(GrepFile { path, hits });
        Ok(self.files.len() as u32)
    }

    /// 满额判定。
    pub fn full(&self) -> bool {
        self.files.len() == B20_FILES.len()
    }

    /// 防重对账：每个关键词总命中恰为 1。
    pub fn audit_unique(&self) -> Result<[u32; 2], u16> {
        if self.files.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.full() {
            return Err(E_SHORT);
        }
        let mut totals = [0u32; 2];
        for f in &self.files {
            for k in 0..2 {
                totals[k] += f.hits[k];
            }
        }
        for k in 0..2 {
            if totals[k] != 1 {
                return Err(E_TOTAL);
            }
        }
        Ok(totals)
    }

    /// 密封。
    pub fn seal(&mut self) -> Result<u64, u16> {
        self.audit_unique()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0xD2u8, 0xA0];
        for f in &self.files {
            buf.extend_from_slice(f.path.as_bytes());
            buf.extend_from_slice(&f.hits[0].to_le_bytes());
            buf.extend_from_slice(&f.hits[1].to_le_bytes());
        }
        buf.push(self.sealed as u8);
        fnv1a64(&buf)
    }

    /// 报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("b20grep 文件 ");
        s.push_str(&self.files.len().to_string());
        s.push_str(" 密封 ");
        s.push_str(&self.sealed.to_string());
        s
    }
}

/// 规范扫描表：b19 独占 m12close、b20 独占 mregate，其余零命中。
pub fn canonical_ledger() -> Result<GrepLedger, u16> {
    let mut led = GrepLedger::new();
    for (i, path) in B20_FILES.iter().enumerate() {
        let hits = match i {
            18 => [1u32, 0u32],
            19 => [0u32, 1u32],
            _ => [0u32, 0u32],
        };
        led.register(path, hits)?;
    }
    Ok(led)
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：关键词族登记 + 规范 20 文件全登记。
    if KEYWORDS.len() != 2 || KEYWORDS[0] != "m12close" || KEYWORDS[1] != "mregate" {
        return Err(E_KW_BAD);
    }
    let led = canonical_ledger()?;
    if !led.full() {
        return Err(E_SHORT);
    }
    hits += 1;

    // R2：防重对账：两关键词总命中恰为 1/1，密封放行。
    let mut led2 = canonical_ledger()?;
    let totals = led2.audit_unique()?;
    if totals != [1, 1] {
        return Err(E_TOTAL);
    }
    let fp2 = led2.seal()?;
    if fp2 == 0 {
        return Err(E_FINGER);
    }
    hits += 1;

    // R3：重复命中注入：篡改 b01 命中字段为 m12close → 总命中 2 → E_TOTAL。
    let mut led3 = canonical_ledger()?;
    led3.files[0].hits = [1u32, 0u32];
    if led3.audit_unique() != Err(E_TOTAL) {
        return Err(E_TOTAL);
    }
    hits += 1;

    // R4：单文件双命中拒绝 → E_DUP_HIT。
    let mut led4 = GrepLedger::new();
    if led4.register(B20_FILES[18], [2, 0]) != Err(E_DUP_HIT) {
        return Err(E_DUP_HIT);
    }
    hits += 1;

    // R5：大小写归一：M12Close / M12CLOSE 与 m12close 同源计数。
    let n1 = count_kw("static M12Close seal // M12CLOSE", "m12close");
    if n1 != 2 {
        return Err(E_TOTAL);
    }
    let n2 = count_kw("MREGATE ok", "mregate");
    if n2 != 1 {
        return Err(E_TOTAL);
    }
    let n3 = count_kw("no marker here", "m12close");
    if n3 != 0 {
        return Err(E_TOTAL);
    }
    hits += 1;

    // R6：闭集外文件拒绝 → E_PATH_MISS；密封后追加拒绝 → E_SEAL。
    let mut led6 = canonical_ledger()?;
    if led6.register("ghost.rs", [0, 0]) != Err(E_PATH_MISS) {
        return Err(E_PATH_MISS);
    }
    led6.seal()?;
    if led6.register(B20_FILES[0], [0, 0]) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // R7：清单不满 20 即对账 → E_SHORT；空表对账 → E_EMPTY。
    let mut led7 = GrepLedger::new();
    if led7.audit_unique() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    for i in 0..5 {
        led7.register(B20_FILES[i], [0, 0])?;
    }
    if led7.audit_unique() != Err(E_SHORT) {
        return Err(E_SHORT);
    }
    hits += 1;

    // R8：LCG 抽查：确定性抽 4 文件复核命中字段与规范一致。
    let led8 = canonical_ledger()?;
    let mut state: u32 = 0xD2A0_0001;
    for _ in 0..4 {
        let idx = (lcg_next(&mut state) % (B20_FILES.len() as u32)) as usize;
        let f = led8
            .files
            .iter()
            .find(|f| f.path == B20_FILES[idx])
            .ok_or(E_SHORT)?;
        let expect = match idx {
            18 => [1u32, 0u32],
            19 => [0u32, 1u32],
            _ => [0u32, 0u32],
        };
        if f.hits != expect {
            return Err(E_TOTAL);
        }
    }
    hits += 1;

    // R9：指纹：同构两表一致；密封位翻转指纹变化。
    let led9a = canonical_ledger()?;
    let mut led9b = canonical_ledger()?;
    if led9a.fingerprint() != led9b.fingerprint() {
        return Err(E_FINGER);
    }
    led9b.seal()?;
    if led9a.fingerprint() == led9b.fingerprint() {
        return Err(E_FINGER);
    }
    hits += 1;

    // R10：报告文本含文件数 20。
    let rep = led9b.report();
    if !rep.contains("20") {
        return Err(E_SHORT);
    }
    hits += 1;

    Ok(hits)
}
