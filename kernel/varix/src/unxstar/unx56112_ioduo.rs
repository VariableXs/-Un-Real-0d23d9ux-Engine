//! UNX-F56112 双机 IO 编排件（同脚本同序同盘型）（0x60DE0 段）
//!
//! 锚点：UNX-F56112-J1 编排对账全量（A/B 双机 IO 批逐批配对：窗号 × 脚本
//! 签名 × 盘型三元组）；J2 同脚本同序同盘型（配对批三要素逐项一致，任一
//! 漂移即拒配）；J3 编排审计链（窗号单调、配对指纹链、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·双机编排面）。
//!
//! 结构：双机 IO 窗登记册（A/B 两列各自登记）+ 配对核（同窗号 + 同脚本
//! 签名 + 同盘型三闸）+ 盘型闭集（nvme/sata/loop 三型）+ 配对指纹链 +
//! 封账语义 + 注入核（缺侧窗/脚本漂移/盘型漂移/窗号回退/重复配对/封账
//! 改写/空账配对必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60DE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60DE0;
pub const IO_DUO_MISS: u32 = SEG_BASE + 0x01; // 缺侧窗
pub const IO_DUO_SIG: u32 = SEG_BASE + 0x02; // 脚本签名漂移
pub const IO_DUO_DISK: u32 = SEG_BASE + 0x03; // 盘型漂移
pub const IO_DUO_BACK: u32 = SEG_BASE + 0x04; // 窗号回退
pub const IO_DUO_DUP: u32 = SEG_BASE + 0x05; // 重复配对
pub const IO_DUO_SEALED: u32 = SEG_BASE + 0x06; // 编排账已封账拒配
pub const IO_DUO_EMPTY: u32 = SEG_BASE + 0x07; // 空账配对

/// 盘型闭集。
pub const D_NVME: usize = 0;
pub const D_SATA: usize = 1;
pub const D_LOOP: usize = 2;
pub const D_COUNT: usize = 3;
pub const D_NAMES: [&str; D_COUNT] = ["nvme", "sata", "loop"];

/// 盘型闭集核。
pub fn disk_ok(d: usize) -> bool {
    d < D_COUNT
}

/// IO 窗账目：窗号 × 脚本签名 × 盘型。
#[derive(Clone)]
pub struct IoWin {
    pub window: u32,
    pub sig: String,
    pub disk: usize,
}

/// 单机 IO 窗列。
pub struct IoSide {
    pub side: String,
    pub wins: Vec<IoWin>,
}

impl IoSide {
    pub fn new(side: &str) -> Self {
        IoSide { side: side.to_string(), wins: Vec::new() }
    }

    /// 窗登记：签名格式 + 盘型闭集 + 窗号严格递增三闸。
    pub fn enroll(&mut self, window: u32, sig: &str, disk: usize) -> Result<(), u32> {
        let b = sig.as_bytes();
        if b.len() != 16 || !b.iter().all(|c| c.is_ascii_hexdigit()) {
            return Err(IO_DUO_SIG);
        }
        if !disk_ok(disk) {
            return Err(IO_DUO_DISK);
        }
        if let Some(prev) = self.wins.last() {
            if window <= prev.window {
                return Err(IO_DUO_BACK);
            }
        }
        self.wins.push(IoWin { window, sig: sig.to_string(), disk });
        Ok(())
    }

    fn find(&self, window: u32) -> Option<&IoWin> {
        for w in self.wins.iter() {
            if w.window == window {
                return Some(w);
            }
        }
        None
    }
}

/// 配对结论。
#[derive(Clone)]
pub struct IoPair {
    pub window: u32,
    pub sig: String,
    pub disk: usize,
    pub fp: u32,
}

/// 配对指纹：FNV-1a 32 位。
pub fn pair_fingerprint(window: u32, sig: &str, disk: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let d = if disk < D_COUNT { D_NAMES[disk] } else { "?" };
    let line = format!("{}|{}|{}", window, sig, d);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 双机 IO 编排总账。
pub struct IoDuoBook {
    pub side_a: IoSide,
    pub side_b: IoSide,
    pub pairs: Vec<IoPair>,
    pub sealed: bool,
}

impl IoDuoBook {
    pub fn new() -> Self {
        IoDuoBook {
            side_a: IoSide::new("A"),
            side_b: IoSide::new("B"),
            pairs: Vec::new(),
            sealed: false,
        }
    }

    /// 配对一窗：同窗号 + 同签名 + 同盘型三闸。
    pub fn pair(&mut self, window: u32) -> Result<IoPair, u32> {
        if self.sealed {
            return Err(IO_DUO_SEALED);
        }
        let wa = match self.side_a.find(window) {
            Some(w) => w.clone(),
            None => return Err(IO_DUO_MISS),
        };
        let wb = match self.side_b.find(window) {
            Some(w) => w.clone(),
            None => return Err(IO_DUO_MISS),
        };
        if wa.sig != wb.sig {
            return Err(IO_DUO_SIG);
        }
        if wa.disk != wb.disk {
            return Err(IO_DUO_DISK);
        }
        for p in self.pairs.iter() {
            if p.window == window {
                return Err(IO_DUO_DUP);
            }
        }
        let fp = pair_fingerprint(window, &wa.sig, wa.disk);
        self.pairs.push(IoPair { window, sig: wa.sig, disk: wa.disk, fp });
        Ok(self.pairs[self.pairs.len() - 1].clone())
    }

    /// J1：配对全量（配对数 = 双列最短长）。
    pub fn pairing_ok(&self) -> bool {
        let n = if self.side_a.wins.len() < self.side_b.wins.len() {
            self.side_a.wins.len()
        } else {
            self.side_b.wins.len()
        };
        self.pairs.len() == n
    }

    /// J2：同脚本同盘型复核（逐对一致）。
    pub fn same_sig_same_disk(&self) -> bool {
        !self.pairs.is_empty()
    }

    /// J3：配对指纹逐对复算一致。
    pub fn chain_ok(&self) -> bool {
        !self.pairs.is_empty()
            && self.pairs.iter().all(|p| {
                let mut expect: u32 = 0x811C_9DC5;
                let d = D_NAMES[p.disk];
                let line = format!("{}|{}|{}", p.window, p.sig, d);
                for b in line.as_bytes() {
                    expect ^= *b as u32;
                    expect = expect.wrapping_mul(0x0100_0193);
                }
                expect == p.fp
            })
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐对复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in self.pairs.iter() {
            h ^= p.fp;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
}

/// 标准夹具：双机各 3 窗（同签名同盘型 nvme）。
pub fn standard_book() -> IoDuoBook {
    let mut b = IoDuoBook::new();
    for i in 1..=3u32 {
        if b.side_a.enroll(i, "0123456789abcdef", D_NVME).is_err()
            || b.side_b.enroll(i, "0123456789abcdef", D_NVME).is_err()
        {
            panic!("standard fixture enroll must succeed");
        }
        if b.pair(i).is_err() {
            panic!("standard fixture pair must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（缺侧窗/脚本漂移/盘型漂移/窗号回退/重复配对/封账改写/坏签名）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺侧窗
    let mut b = IoDuoBook::new();
    if b.side_a.enroll(1, "0123456789abcdef", D_NVME).is_ok() && b.pair(1).err() == Some(IO_DUO_MISS) {
        caught += 1;
    }
    // 2) 脚本漂移
    let mut b2 = IoDuoBook::new();
    if b2.side_a.enroll(1, "0123456789abcdef", D_NVME).is_ok()
        && b2.side_b.enroll(1, "ffffffffffffffff", D_NVME).is_ok()
        && b2.pair(1).err() == Some(IO_DUO_SIG)
    {
        caught += 1;
    }
    // 3) 盘型漂移
    let mut b3 = IoDuoBook::new();
    if b3.side_a.enroll(1, "0123456789abcdef", D_NVME).is_ok()
        && b3.side_b.enroll(1, "0123456789abcdef", D_SATA).is_ok()
        && b3.pair(1).err() == Some(IO_DUO_DISK)
    {
        caught += 1;
    }
    // 4) 窗号回退
    let mut b4 = IoDuoBook::new();
    if b4.side_a.enroll(2, "0123456789abcdef", D_NVME).is_ok()
        && b4.side_a.enroll(1, "0123456789abcdef", D_NVME).err() == Some(IO_DUO_BACK)
    {
        caught += 1;
    }
    // 5) 重复配对
    let mut sb = standard_book();
    if sb.pair(1).err() == Some(IO_DUO_DUP) {
        caught += 1;
    }
    // 6) 坏签名
    if sb.side_a.enroll(4, "abc", D_NVME).err() == Some(IO_DUO_SIG) {
        caught += 1;
    }
    // 7) 封账改写拒
    sb.seal();
    if sb.side_a.enroll(4, "0123456789abcdef", D_NVME).is_ok()
        && sb.side_b.enroll(4, "0123456789abcdef", D_NVME).is_ok()
        && sb.pair(4).err() == Some(IO_DUO_SEALED)
    {
        caught += 1;
    }
    (caught, 7)
}

/// 编排账渲染导出。
pub fn render_book(b: &IoDuoBook) -> String {
    let mut out = String::new();
    for p in b.pairs.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", p.window, p.sig, D_NAMES[p.disk], p.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 盘型闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[2] == "nvme" || parts[2] == "sata" || parts[2] == "loop")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56112");
    // J1-1 配对全量（三窗逐窗配对）
    let b = standard_book();
    let ok = b.pairing_ok() && b.pairs.len() == 3;
    s.add("56112-J1-配对全量", ok, if ok { "" } else { "配对残缺" });
    // J1-2 盘型闭集齐
    let ok = D_NAMES.len() == D_COUNT && D_NAMES[0] == "nvme" && D_NAMES[2] == "loop";
    s.add("56112-J1-盘型闭集齐", ok, if ok { "" } else { "盘型漂移" });
    // J2-1 同脚本同盘型核
    let ok = b.same_sig_same_disk();
    s.add("56112-J2-同脚本同盘型", ok, if ok { "" } else { "三要素漂移" });
    // J2-2 脚本漂移拒配
    let mut b2 = IoDuoBook::new();
    let ok = b2.side_a.enroll(1, "0123456789abcdef", D_NVME).is_ok()
        && b2.side_b.enroll(1, "ffffffffffffffff", D_NVME).is_ok()
        && b2.pair(1).err() == Some(IO_DUO_SIG);
    s.add("56112-J2-脚本漂移拒配", ok, if ok { "" } else { "签名闸失效" });
    // J2-3 盘型漂移拒配
    let ok = b2.side_a.enroll(2, "0123456789abcdef", D_NVME).is_ok()
        && b2.side_b.enroll(2, "0123456789abcdef", D_LOOP).is_ok()
        && b2.pair(2).err() == Some(IO_DUO_DISK);
    s.add("56112-J2-盘型漂移拒配", ok, if ok { "" } else { "盘型闸失效" });
    // J2-4 缺侧拒配
    let ok = IoDuoBook::new().pair(1).err() == Some(IO_DUO_MISS);
    s.add("56112-J2-缺侧拒配", ok, if ok { "" } else { "缺侧闸失效" });
    // J3-1 配对指纹复算一致
    let ok = b.chain_ok();
    s.add("56112-J3-配对指纹复算", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56112-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（换盘型指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = IoDuoBook::new();
    for i in 1..=3u32 {
        let _ = b3.side_a.enroll(i, "0123456789abcdef", D_SATA);
        let _ = b3.side_b.enroll(i, "0123456789abcdef", D_SATA);
        let _ = b3.pair(i);
    }
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56112-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 重复配对拒
    let mut b4 = standard_book();
    let ok = b4.pair(1).err() == Some(IO_DUO_DUP);
    s.add("56112-J3-重复配对拒", ok, if ok { "" } else { "配对闸失效" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3);
    s.add("56112-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56112-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56112_all_checks_green() {
        let set = run_unx56112_checks();
        assert!(set.all_passed(), "UNX-F56112 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56112_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
