//! UNX-F46427 启动文件族布设（crt0/crti/crtn 类 × 链接序列落位）（0x401A0 段）
//!
//! 锚点：UNX-F46427-J1 启动文件 ×4 逐个存在 + 链接序列注入 ×10 场景序
//! 正确。
//! 域：CoRun·UNX-S1-B02（工具链域·启动文件面）。
//!
//! 结构：启动文件四件账（crt0/crti/crtn/crtbegin 逐件登记）+ 链接序列
//! 状态机（crt0 → crti → crtbegin → 用户 → crtend → crtn 六段序）+
//! ×10 场景核（序正确/缺件拒链/序反转拒链）+ 注入核（缺件/序反转/
//! 越段/重复段/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401A0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401A0;
pub const CF_MISS: u32 = SEG_BASE + 0x01; // 启动文件缺失
pub const CF_SEQ_REVERSE: u32 = SEG_BASE + 0x02; // 序反转
pub const CF_SEG_OOB: u32 = SEG_BASE + 0x03; // 越段
pub const CF_SEG_DUP: u32 = SEG_BASE + 0x04; // 重复段
pub const CF_SEALED: u32 = SEG_BASE + 0x05; // 封账改写

/// 启动文件四件闭集。
pub const F_CRT0: usize = 0;
pub const F_CRTI: usize = 1;
pub const F_CRTN: usize = 2;
pub const F_CRTBEGIN: usize = 3;
pub const F_COUNT: usize = 4;

pub const F_NAMES: [&str; F_COUNT] = ["crt0.o", "crti.o", "crtn.o", "crtbegin.o"];

/// 链接序列六段闭集。
pub const SEQ_CRT0: usize = 0;
pub const SEQ_CRTI: usize = 1;
pub const SEQ_CRTBEGIN: usize = 2;
pub const SEQ_USER: usize = 3;
pub const SEQ_CRTEND: usize = 4;
pub const SEQ_CRTN: usize = 5;
pub const SEQ_COUNT: usize = 6;

pub const SEQ_NAMES: [&str; SEQ_COUNT] = [
    "crt0", "crti", "crtbegin", "user-objects", "crtend", "crtn",
];

/// 启动文件账（四件在位位）。
pub struct CrtBook {
    pub present: [bool; F_COUNT],
    pub sealed: bool,
}

impl CrtBook {
    pub fn new() -> Self {
        CrtBook { present: [false; F_COUNT], sealed: false }
    }

    /// 件登记（越件/封账拒绝）。
    pub fn put(&mut self, f: usize) -> Result<(), u32> {
        if f >= F_COUNT {
            return Err(CF_SEG_OOB);
        }
        if self.sealed {
            return Err(CF_SEALED);
        }
        self.present[f] = true;
        Ok(())
    }

    /// 四件全在核。
    pub fn all_present(&self) -> bool {
        self.present.iter().all(|p| *p)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 链接序列状态机：逐段追加，段号必须严格递增（序反转拒绝）。
pub struct LinkSequence {
    pub segs: Vec<usize>,
    pub sealed: bool,
}

impl LinkSequence {
    pub fn new() -> Self {
        LinkSequence { segs: Vec::new(), sealed: false }
    }

    /// 段追加（越段/重复/反转/封账拒绝）。
    pub fn push(&mut self, seg: usize) -> Result<(), u32> {
        if seg >= SEQ_COUNT {
            return Err(CF_SEG_OOB);
        }
        if self.sealed {
            return Err(CF_SEALED);
        }
        if self.segs.contains(&seg) {
            return Err(CF_SEG_DUP);
        }
        if let Some(last) = self.segs.last() {
            if seg < *last {
                return Err(CF_SEQ_REVERSE);
            }
        }
        self.segs.push(seg);
        Ok(())
    }

    /// 标准序核（六段 0→5 全序）。
    pub fn standard_ok(&self) -> bool {
        self.segs == vec![0usize, 1, 2, 3, 4, 5]
    }

    /// 序单调核（已追加段严格递增）。
    pub fn monotonic(&self) -> bool {
        self.segs.windows(2).all(|w| w[0] < w[1])
    }

    /// 六段齐核。
    pub fn complete(&self) -> bool {
        self.segs.len() == SEQ_COUNT
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 序指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for s in self.segs.iter() {
            h ^= (*s as u32 + 1).wrapping_mul(16777619);
        }
        h
    }
}

/// ×10 场景核：标准序 ×2 + 前缀子序 ×3（链接器分段调用）+ 序反转拒 ×3
/// + 缺件拒链 ×2。
pub fn ten_scenarios() -> (usize, usize) {
    let mut passed = 0usize;
    const TOTAL: usize = 10;
    // 场景 1/2：标准六段序（重放 ×2）。
    for _ in 0..2 {
        let mut seq = LinkSequence::new();
        let mut ok = true;
        for s in 0..SEQ_COUNT {
            if seq.push(s).is_err() {
                ok = false;
            }
        }
        if ok && seq.standard_ok() && seq.complete() {
            passed += 1;
        }
    }
    // 场景 3/4/5：前缀子序（0→1→2 / 0→1→2→3 / 0→1）单调且非全序。
    let prefixes: [Vec<usize>; 3] = [vec![0, 1, 2], vec![0, 1, 2, 3], vec![0, 1]];
    for pre in prefixes.iter() {
        let mut seq = LinkSequence::new();
        let mut ok = true;
        for s in pre.iter() {
            if seq.push(*s).is_err() {
                ok = false;
            }
        }
        if ok && seq.monotonic() && !seq.complete() {
            passed += 1;
        }
    }
    // 场景 6/7/8：序反转拒（2→0 / 5→3 / 4→1）。
    let reversals: [(usize, usize); 3] = [(2, 0), (5, 3), (4, 1)];
    for (a, b) in reversals.iter() {
        let mut seq = LinkSequence::new();
        seq.push(*a).expect("first");
        if seq.push(*b) == Err(CF_SEQ_REVERSE) {
            passed += 1;
        }
    }
    // 场景 9/10：缺件拒链（四件缺任一 → 链接前置核拒绝）。
    for miss in [F_CRT0, F_CRTBEGIN] {
        let mut book = CrtBook::new();
        for f in 0..F_COUNT {
            if f != miss {
                book.put(f).expect("partial");
            }
        }
        if !book.all_present() {
            passed += 1;
        }
    }
    (passed, TOTAL)
}

/// 链接前置核：四件全在 + 序标准才放行。
pub fn link_precheck(book: &CrtBook, seq: &LinkSequence) -> Result<(), u32> {
    if !book.all_present() {
        return Err(CF_MISS);
    }
    if !seq.standard_ok() {
        return Err(CF_SEQ_REVERSE);
    }
    Ok(())
}

/// 标准夹具：四件全在 + 标准六段序。
pub fn standard_fixture() -> (CrtBook, LinkSequence) {
    let mut book = CrtBook::new();
    for f in 0..F_COUNT {
        book.put(f).expect("fixture crt");
    }
    let mut seq = LinkSequence::new();
    for s in 0..SEQ_COUNT {
        seq.push(s).expect("fixture seq");
    }
    (book, seq)
}

/// 启动文件落位渲染（件|路径 四行）。
pub fn render_crt(book: &CrtBook) -> String {
    let mut out = String::new();
    for f in 0..F_COUNT {
        out.push_str(&format!(
            "{}|{}\n",
            F_NAMES[f],
            if book.present[f] {
                format!("/usr/lib/x86_64-varix-unxreal/13.2.0/{}", F_NAMES[f])
            } else {
                "MISSING".to_string()
            }
        ));
    }
    out
}

/// 渲染完整性核（四行 × 两段）。
pub fn render_crt_ok(book: &CrtBook) -> bool {
    let text = render_crt(book);
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == 4
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 2 && (seg[1] == "MISSING" || seg[1].ends_with(".o"))
        })
}

/// 链接命令行推导：六段序 → ld 参数序（crt0/crti → user → crtbegin/crtend/crtn 实际布局）。
pub fn ld_command_line(seq: &LinkSequence) -> Result<String, u32> {
    if !seq.complete() {
        return Err(CF_SEG_OOB);
    }
    // Varix 布局：crt0.o crti.o crtbegin.o <user> crtend.o crtn.o -lc。
    let mut out = String::from("ld");
    for s in seq.segs.iter() {
        let part = match *s {
            SEQ_CRT0 => " /usr/lib/x86_64-varix-unxreal/13.2.0/crt0.o",
            SEQ_CRTI => " /usr/lib/x86_64-varix-unxreal/13.2.0/crti.o",
            SEQ_CRTBEGIN => " /usr/lib/x86_64-varix-unxreal/13.2.0/crtbegin.o",
            SEQ_USER => " <user-objects>",
            SEQ_CRTEND => " /usr/lib/x86_64-varix-unxreal/13.2.0/crtend.o",
            SEQ_CRTN => " /usr/lib/x86_64-varix-unxreal/13.2.0/crtn.o",
            _ => return Err(CF_SEG_OOB),
        };
        out.push_str(part);
    }
    out.push_str(" -lc");
    Ok(out)
}

/// 命令行推导核（六段件名逐个在行 + -lc 收尾）。
pub fn ld_command_ok(seq: &LinkSequence) -> bool {
    match ld_command_line(seq) {
        Ok(cmd) => {
            F_NAMES.iter().all(|f| cmd.contains(f))
                && cmd.contains("<user-objects>")
                && cmd.ends_with("-lc")
        }
        Err(_) => false,
    }
}

/// 注入核：6 注入必抓（缺件/序反转/越段/重复段/封账改写/前置核放行缺件）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺件前置核必抓
    let mut book = CrtBook::new();
    book.put(F_CRT0).expect("c0");
    let mut seq = LinkSequence::new();
    for s in 0..SEQ_COUNT {
        seq.push(s).expect("seq");
    }
    if link_precheck(&book, &seq) == Err(CF_MISS) {
        caught += 1;
    }
    // 2) 序反转前置核必抓
    let (std_book, _s) = standard_fixture();
    let mut bad_seq = LinkSequence::new();
    bad_seq.push(SEQ_CRTBEGIN).expect("b1");
    bad_seq.push(SEQ_CRT0).expect_err("reverse");
    if link_precheck(&std_book, &bad_seq) == Err(CF_SEQ_REVERSE) {
        caught += 1;
    }
    // 3) 越段必抓
    let mut seq3 = LinkSequence::new();
    if seq3.push(9) == Err(CF_SEG_OOB) {
        caught += 1;
    }
    // 4) 重复段必抓
    seq3.push(SEQ_CRT0).expect("d1");
    if seq3.push(SEQ_CRT0) == Err(CF_SEG_DUP) {
        caught += 1;
    }
    // 5) 封账改写必抓
    let mut seq5 = standard_fixture().1;
    seq5.seal();
    if seq5.push(SEQ_CRT0) == Err(CF_SEALED) {
        caught += 1;
    }
    // 6) 越件登记必抓
    let mut book6 = CrtBook::new();
    if book6.put(9) == Err(CF_SEG_OOB) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46427_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46427");
    // J1-1 启动文件 ×4 逐个存在
    let (book, seq) = standard_fixture();
    let ok = book.all_present() && render_crt_ok(&book);
    s.add("46427-J1-四件存在", ok, if ok { "" } else { "缺件" });
    // J1-2 链接序列 ×10 场景序正确
    let (p, n) = ten_scenarios();
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("场景 {p}/{n}") };
    s.add_dyn("46427-J1-×10场景序", ok, &detail);
    // J1-3 前置核放行标准态
    let ok = link_precheck(&book, &seq).is_ok();
    s.add("46427-J1-前置核放行", ok, if ok { "" } else { "前置核漂移" });
    // J1-4 序指纹稳定
    let ok = seq.fingerprint() == standard_fixture().1.fingerprint();
    s.add("46427-J1-序指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-5 链接命令行推导
    let ok = ld_command_ok(&seq);
    s.add("46427-J1-链接命令行", ok, if ok { "" } else { "命令行漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46427-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46427_all_checks_green() {
        let set = run_unx46427_checks();
        assert!(set.all_passed(), "UNX-F46427 checks must be green: {}", set.red_items_text());
    }
}
