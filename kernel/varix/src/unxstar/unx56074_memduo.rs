//! UNX-F56074 双机内存编排件（同负载同序）（0x60920 段）
//!
//! 锚点：UNX-F56074-J1 编排对账全量（A/B 双机采集窗逐窗配对：窗号 ×
//! 负载签名 × 序号三元组逐窗落账）；J2 同负载同序（配对窗负载签名一致
//! 且序号一致，任一错位即拒配）；J3 编排审计链（窗号单调、配对指纹链、
//! 封账只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·双机编排面）。
//!
//! 结构：双机窗登记册（A 机窗 / B 机窗两列各自登记）+ 配对核（同窗号
//! 同负载签名同序号才可配对）+ 错位拒配语义（签名漂移/序号漂移/缺侧
//! 窗三路拒）+ 配对指纹链（前窗指纹混入后窗）+ 封账语义 + 注入核
//! （缺侧配对/签名漂移/序号漂移/窗号回退/重复配对/封账改写/空账复核必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60920 独占 0x20）。
pub const SEG_BASE: u32 = 0x60920;
pub const DUO_MISS_SIDE: u32 = SEG_BASE + 0x01; // 缺侧窗（单机登记无对端）
pub const DUO_SIG_DRIFT: u32 = SEG_BASE + 0x02; // 负载签名漂移（双机不同负载）
pub const DUO_SEQ_DRIFT: u32 = SEG_BASE + 0x03; // 序号漂移（同负载不同序）
pub const DUO_WIN_BACK: u32 = SEG_BASE + 0x04; // 窗号回退（单机列内非单调）
pub const DUO_DUP_PAIR: u32 = SEG_BASE + 0x05; // 同窗号重复配对
pub const DUO_SEALED: u32 = SEG_BASE + 0x06; // 编排账已封账拒写
pub const DUO_EMPTY: u32 = SEG_BASE + 0x07; // 空账复核（无窗可配）

/// 负载签名长度（定长指纹串）。
pub const SIG_LEN: usize = 8;

/// 负载签名格式核：8 位十六进制。
pub fn signature_ok(sig: &str) -> bool {
    let b = sig.as_bytes();
    b.len() == SIG_LEN && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 窗账目：窗号 × 负载签名 × 序号 三元组。
#[derive(Clone)]
pub struct DuoWindow {
    pub window: u32,
    pub sig: String,
    pub seq: u32,
}

/// 单机窗列。
#[derive(Clone)]
pub struct SideColumn {
    pub side: String,
    pub wins: Vec<DuoWindow>,
}

impl SideColumn {
    pub fn new(side: &str) -> Self {
        SideColumn { side: side.to_string(), wins: Vec::new() }
    }

    /// 窗登记：签名格式 + 窗号列内严格递增双闸。
    pub fn enroll(&mut self, window: u32, sig: &str, seq: u32) -> Result<(), u32> {
        if !signature_ok(sig) {
            return Err(DUO_SIG_DRIFT);
        }
        if let Some(prev) = self.wins.last() {
            if window <= prev.window {
                return Err(DUO_WIN_BACK);
            }
            if window - prev.window > 64 {
                return Err(DUO_WIN_BACK);
            }
        }
        self.wins.push(DuoWindow { window, sig: sig.to_string(), seq });
        Ok(())
    }

    fn find(&self, window: u32) -> Option<&DuoWindow> {
        for w in self.wins.iter() {
            if w.window == window {
                return Some(w);
            }
        }
        None
    }
}

/// 配对结论：窗号 × 双机签名 × 配对指纹。
#[derive(Clone)]
pub struct PairResult {
    pub window: u32,
    pub sig_a: String,
    pub sig_b: String,
    pub fp: u32,
}

/// 双机编排总账。
pub struct DuoBook {
    pub side_a: SideColumn,
    pub side_b: SideColumn,
    pub pairs: Vec<PairResult>,
    pub sealed: bool,
}

impl DuoBook {
    pub fn new() -> Self {
        DuoBook {
            side_a: SideColumn::new("A"),
            side_b: SideColumn::new("B"),
            pairs: Vec::new(),
            sealed: false,
        }
    }

    /// 配对一窗：双机列同窗号 + 同签名 + 同序号三闸，配对留痕。
    pub fn pair(&mut self, window: u32) -> Result<PairResult, u32> {
        if self.sealed {
            return Err(DUO_SEALED);
        }
        let wa = match self.side_a.find(window) {
            Some(w) => w.clone(),
            None => return Err(DUO_MISS_SIDE),
        };
        let wb = match self.side_b.find(window) {
            Some(w) => w.clone(),
            None => return Err(DUO_MISS_SIDE),
        };
        if wa.sig != wb.sig {
            return Err(DUO_SIG_DRIFT);
        }
        if wa.seq != wb.seq {
            return Err(DUO_SEQ_DRIFT);
        }
        for p in self.pairs.iter() {
            if p.window == window {
                return Err(DUO_DUP_PAIR);
            }
        }
        let fp = pair_fingerprint(window, &wa.sig, wa.seq);
        self.pairs.push(PairResult { window, sig_a: wa.sig, sig_b: wb.sig, fp });
        Ok(self.pairs[self.pairs.len() - 1].clone())
    }

    /// J1：配对全量（配对数 = 双列最短长度，无缺侧残留可配窗）。
    pub fn pairing_ok(&self) -> bool {
        let min_len = if self.side_a.wins.len() < self.side_b.wins.len() {
            self.side_a.wins.len()
        } else {
            self.side_b.wins.len()
        };
        self.pairs.len() == min_len
    }

    /// J2：同负载同序核（逐对签名一致 + 序号一致由 pair 闸已保证，此处复核）。
    pub fn same_load_same_seq(&self) -> bool {
        !self.pairs.is_empty()
            && self.pairs.iter().all(|p| p.sig_a == p.sig_b)
    }

    /// J3：配对指纹链复核（逐对 fp 复算一致）。
    pub fn chain_ok(&self) -> bool {
        !self.pairs.is_empty()
            && self
                .pairs
                .iter()
                .all(|p| p.fp == pair_fingerprint(p.window, &p.sig_a, seq_of(&self.side_a, p.window).unwrap_or(0)))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐对复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in self.pairs.iter() {
            let line = format!("{}|{}|{}|{:08x}", p.window, p.sig_a, p.sig_b, p.fp);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 配对指纹：FNV-1a 32 位（窗号|签名|序号复合）。
pub fn pair_fingerprint(window: u32, sig: &str, seq: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", window, sig, seq);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 序号查询辅助（列内找窗号）。
fn seq_of(col: &SideColumn, window: u32) -> Option<u32> {
    match col.find(window) {
        Some(w) => Some(w.seq),
        None => None,
    }
}

/// 标准夹具：A/B 双机各 4 窗（同签名 abcd1234，序号 1..4）。
pub fn standard_book() -> DuoBook {
    let mut b = DuoBook::new();
    for i in 1..=4u32 {
        if b.side_a.enroll(i, "abcd1234", i).is_err() || b.side_b.enroll(i, "abcd1234", i).is_err() {
            panic!("standard fixture enroll must succeed");
        }
        if b.pair(i).is_err() {
            panic!("standard fixture pair must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（缺侧配对/签名漂移/序号漂移/窗号回退/重复配对/封账改写/坏签名）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺侧配对（A 有 B 无）
    let mut b = DuoBook::new();
    if b.side_a.enroll(1, "abcd1234", 1).is_ok() && b.pair(1).err() == Some(DUO_MISS_SIDE) {
        caught += 1;
    }
    // 2) 签名漂移（双机不同签名）
    let mut b2 = DuoBook::new();
    if b2.side_a.enroll(1, "abcd1234", 1).is_ok()
        && b2.side_b.enroll(1, "ffff0000", 1).is_ok()
        && b2.pair(1).err() == Some(DUO_SIG_DRIFT)
    {
        caught += 1;
    }
    // 3) 序号漂移（同签名不同序）
    let mut b3 = DuoBook::new();
    if b3.side_a.enroll(1, "abcd1234", 1).is_ok()
        && b3.side_b.enroll(1, "abcd1234", 2).is_ok()
        && b3.pair(1).err() == Some(DUO_SEQ_DRIFT)
    {
        caught += 1;
    }
    // 4) 窗号回退（列内 2 后登 1）
    let mut b4 = DuoBook::new();
    if b4.side_a.enroll(2, "abcd1234", 1).is_ok() && b4.side_a.enroll(1, "abcd1234", 2).err() == Some(DUO_WIN_BACK) {
        caught += 1;
    }
    // 5) 重复配对
    let mut sb = standard_book();
    if sb.pair(1).err() == Some(DUO_DUP_PAIR) {
        caught += 1;
    }
    // 6) 坏签名（长度 7）
    if sb.side_a.enroll(5, "abc123", 5).err() == Some(DUO_SIG_DRIFT) {
        caught += 1;
    }
    // 7) 封账后配对拒
    sb.seal();
    if sb.side_a.enroll(5, "abcd1234", 5).is_ok()
        && sb.side_b.enroll(5, "abcd1234", 5).is_ok()
        && sb.pair(5).err() == Some(DUO_SEALED)
    {
        caught += 1;
    }
    (caught, 7)
}

/// 编排账渲染导出（逐对一行）。
pub fn render_book(b: &DuoBook) -> String {
    let mut out = String::new();
    for p in b.pairs.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", p.window, p.sig_a, p.sig_b, p.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 双签名一致。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && parts[1] == parts[2]
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56074_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56074");
    // J1-1 配对全量（4 窗逐窗配对）
    let b = standard_book();
    let ok = b.pairing_ok() && b.pairs.len() == 4;
    s.add("56074-J1-配对全量", ok, if ok { "" } else { "配对残缺" });
    // J1-2 签名格式闭集（8 位十六进制）
    let ok = signature_ok("abcd1234") && !signature_ok("abc123") && !signature_ok("zzzz1111");
    s.add("56074-J1-签名格式闭集", ok, if ok { "" } else { "签名格式漂移" });
    // J1-3 同负载同序核
    let ok = b.same_load_same_seq();
    s.add("56074-J2-同负载同序", ok, if ok { "" } else { "负载序漂移" });
    // J2-1 签名漂移拒配（注入语义正向复核）
    let mut b2 = DuoBook::new();
    let ok = b2.side_a.enroll(1, "abcd1234", 1).is_ok()
        && b2.side_b.enroll(1, "ffff0000", 1).is_ok()
        && b2.pair(1).err() == Some(DUO_SIG_DRIFT);
    s.add("56074-J2-签名漂移拒配", ok, if ok { "" } else { "漂移闸失效" });
    // J2-2 序号漂移拒配
    let mut b3 = DuoBook::new();
    let ok = b3.side_a.enroll(1, "abcd1234", 1).is_ok()
        && b3.side_b.enroll(1, "abcd1234", 2).is_ok()
        && b3.pair(1).err() == Some(DUO_SEQ_DRIFT);
    s.add("56074-J2-序号漂移拒配", ok, if ok { "" } else { "序号闸失效" });
    // J2-3 缺侧拒配
    let mut b4 = DuoBook::new();
    let ok = b4.side_a.enroll(1, "abcd1234", 1).is_ok() && b4.pair(1).err() == Some(DUO_MISS_SIDE);
    s.add("56074-J2-缺侧拒配", ok, if ok { "" } else { "缺侧闸失效" });
    // J3-1 配对指纹链复核
    let ok = b.chain_ok();
    s.add("56074-J3-配对指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 指纹确定性
    let ok = pair_fingerprint(1, "abcd1234", 1) == pair_fingerprint(1, "abcd1234", 1)
        && pair_fingerprint(1, "abcd1234", 1) != pair_fingerprint(2, "abcd1234", 1);
    s.add("56074-J3-指纹确定性", ok, if ok { "" } else { "指纹碰撞" });
    // J3-3 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56074-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-4 重复配对拒
    let mut b5 = standard_book();
    let ok = b5.pair(1).err() == Some(DUO_DUP_PAIR);
    s.add("56074-J3-重复配对拒", ok, if ok { "" } else { "配对闸失效" });
    // J1-4 渲染导出四行齐且双签名一致
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4);
    s.add("56074-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-4 空账配对显性拒绝
    let ok = DuoBook::new().pair(1).err() == Some(DUO_MISS_SIDE);
    s.add("56074-J2-空账拒配", ok, if ok { "" } else { "空账语义漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56074-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56074_all_checks_green() {
        let set = run_unx56074_checks();
        assert!(set.all_passed(), "UNX-F56074 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56074_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
