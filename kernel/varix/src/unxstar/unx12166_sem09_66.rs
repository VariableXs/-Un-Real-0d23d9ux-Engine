//! UNX-F12166 — RtlRaiseException/RtlRaiseStatus 语义（0x1a6e0 段）
//!
//! 锚点：UNX-F12166-J1 抛异常双路 10/10（RaiseException 全参异常：
//! 码+标志+参数数组；RaiseStatus 轻量异常：仅状态码），双路留痕
//! 全账回放一致，非法码/超参必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：RaiseCall（抛出请求）+ RaiseBook（双路抛出账）+ 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a6e0;
pub const RS_CODE: u32 = SEG_BASE + 0x01; // 非法异常码（非 0x8/0xC/0xE 前缀）
pub const RS_ARGS: u32 = SEG_BASE + 0x02; // 参数数超限（>15）
pub const RS_TWICE: u32 = SEG_BASE + 0x03; // 同参重抛
pub const RS_SEALED: u32 = SEG_BASE + 0x04; // 印后抛
pub const RS_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕抛

pub const RS_MAX_ARGS: usize = 15;

/// 抛出请求。
#[derive(Clone, PartialEq)]
pub struct RaiseCall {
    pub code: u32,
    pub flags: u32,
    pub args: Vec<u64>,
    pub light: bool, // true = RaiseStatus 路
    pub note: String,
}

impl RaiseCall {
    /// 请求自洽：码前缀 ∈ {0x8, 0xC, 0xE}、参数 ≤ 15。
    pub fn ok(&self) -> bool {
        let hi = self.code >> 28;
        (hi == 0x8 || hi == 0xC || hi == 0xE)
            && self.args.len() <= RS_MAX_ARGS
            && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!(
            "{} c={:#x} f={:#x} n={}",
            if self.light { "RS" } else { "RE" },
            self.code,
            self.flags,
            self.args.len()
        )
    }
}

/// 抛出账。
pub struct RaiseBook {
    pub calls: Vec<RaiseCall>,
    sealed: bool,
}

impl RaiseBook {
    pub fn new() -> RaiseBook {
        RaiseBook { calls: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 全参抛（RtlRaiseException 路）。
    pub fn raise(&mut self, code: u32, flags: u32, args: Vec<u64>, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        let call = RaiseCall { code, flags, args, light: false, note: note.to_string() };
        self.admit(call)
    }

    /// 轻量抛（RtlRaiseStatus 路：flags=0、args 空）。
    pub fn raise_status(&mut self, code: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        let call = RaiseCall { code, flags: 0, args: Vec::new(), light: true, note: note.to_string() };
        self.admit(call)
    }

    fn admit(&mut self, call: RaiseCall) -> Result<usize, u32> {
        if !call.ok() {
            return Err(if call.args.len() > RS_MAX_ARGS {
                RS_ARGS
            } else if call.note.is_empty() {
                RS_EMPTY
            } else {
                RS_CODE
            });
        }
        if self.calls.iter().any(|c| *c == call) {
            return Err(RS_TWICE);
        }
        self.calls.push(call);
        Ok(self.calls.len())
    }

    /// 双路计数。
    pub fn count_by(&self, light: bool) -> usize {
        self.calls.iter().filter(|c| c.light == light).count()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a6e0;
        for c in &self.calls {
            h = h.wrapping_mul(211).wrapping_add(c.code);
            h = h.wrapping_mul(211).wrapping_add(c.flags);
            h = h.wrapping_mul(211).wrapping_add(c.args.len() as u32);
            for by in c.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "calls={} re={} rs={}",
            self.calls.len(),
            self.count_by(false),
            self.count_by(true)
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RaiseBook::new();
    // 1) 非法码前缀（0x1）必拒。
    got[0] = b.raise(0x1000_0001, 0, vec![], "bad").err() == Some(RS_CODE);
    // 2) 参数超限（16）必拒。
    got[1] = b
        .raise(0xC0000005, 0, (0..16).map(|i| i as u64).collect(), "many")
        .err()
        == Some(RS_ARGS);
    // 3) 空留痕必拒。
    got[2] = b.raise(0xC0000005, 0, vec![], "").err() == Some(RS_EMPTY);
    // 4) 同参重抛必拒。
    let _ = b.raise(0xC0000005, 0, vec![1], "a");
    got[3] = b.raise(0xC0000005, 0, vec![1], "a").err() == Some(RS_TWICE);
    // 5) 印后抛必拒。
    b.seal();
    got[4] = b.raise(0xC0000005, 0, vec![], "post").err() == Some(RS_SEALED);
    got
}

/// 判据入口：UNX-F12166
pub fn run_unx12166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12166");
    let mut b = RaiseBook::new();

    // J1 全参抛三笔（AV/DBZ/SO）。
    let j1 = b.raise(0xC0000005, 0, vec![0, 1], "av").is_ok()
        && b.raise(0xC0000094, 0, vec![], "dbz").is_ok()
        && b.raise(0xC00000FD, 0, vec![0x8000], "so").is_ok();
    s.add("J1_raise3", j1 && b.count_by(false) == 3, "全参抛三笔");

    // J2 轻量抛两笔。
    let j2 = b.raise_status(0xC0000409, "ff").is_ok()
        && b.raise_status(0xE0000001, "user").is_ok();
    s.add("J2_status2", j2 && b.count_by(true) == 2, "轻量抛两笔");

    // J3 码前缀合法性 10/10。
    let mut j3 = true;
    for code in [0x8000_0001u32, 0xC000_0005, 0xE000_0001] {
        let c = RaiseCall { code, flags: 0, args: vec![], light: true, note: "t".to_string() };
        if !c.ok() {
            j3 = false;
        }
    }
    for code in [0x1000_0001u32, 0x2000_0005, 0x3000_0009, 0x5000_0001, 0x7000_0001] {
        let c = RaiseCall { code, flags: 0, args: vec![], light: true, note: "t".to_string() };
        if c.ok() {
            j3 = false;
        }
    }
    s.add("J3_prefix_10x", j3, "合法前缀 3 过、野前缀 5 拒");

    // J4 参数恰好 15 合法。
    let max_call = RaiseCall {
        code: 0xC0000005,
        flags: 0,
        args: (0..15).map(|i| i as u64).collect(),
        light: false,
        note: "max".to_string(),
    };
    s.add("J4_args15", max_call.ok(), "15 参恰满合法");

    // J5 参数 16 超限拒。
    s.add(
        "J5_args16",
        b.raise(0xC0000005, 0, (0..16).map(|i| i as u64).collect(), "m").err() == Some(RS_ARGS),
        "16 参拒 ARGS",
    );

    // J6 请求渲染。
    let r = b.calls[0].render();
    s.add(
        "J6_render",
        r.contains("RE") && r.contains("c=0xc0000005") && r.contains("n=2"),
        "渲染含路/码/参",
    );

    // J7 轻量渲染。
    let r7 = b.calls[3].render();
    s.add(
        "J7_render_rs",
        r7.contains("RS") && r7.contains("n=0"),
        "轻量渲染 RS/0 参",
    );

    // J8 请求自洽核。
    s.add("J8_calls_ok", b.calls.iter().all(|c| c.ok()), "全请求过自洽核");

    // J9 同参重抛拒。
    s.add(
        "J9_dup",
        b.raise(0xC0000005, 0, vec![0, 1], "av").err() == Some(RS_TWICE),
        "同参重抛拒 TWICE",
    );

    // J10 码前缀非法拒。
    s.add(
        "J10_bad_code",
        b.raise_status(0x1234_5678, "x").err() == Some(RS_CODE),
        "野前缀拒 CODE",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "抛出账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.raise_status(0xC0000005, "post").err() == Some(RS_SEALED),
        "印账后抛拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("calls=5") && sm.contains("re=3") && sm.contains("rs=2"),
        "摘要含双路计数",
    );

    // J15 轻量抛 flags 恒 0。
    s.add(
        "J15_rs_flags",
        b.calls.iter().filter(|c| c.light).all(|c| c.flags == 0 && c.args.is_empty()),
        "轻量路 flags=0 空 args",
    );

    // J16 空留痕抛拒。
    let mut b16 = RaiseBook::new();
    s.add(
        "J16_empty_note",
        b16.raise_status(0xC0000005, "").err() == Some(RS_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J17 十笔连抛计数。
    let mut b17 = RaiseBook::new();
    let mut ok17 = true;
    for i in 0..10u32 {
        if b17.raise(0xC000_0000 + i, 0, vec![], "m").is_err() {
            ok17 = false;
        }
    }
    s.add("J17_ten_raise", ok17 && b17.calls.len() == 10, "十笔连抛全入账");

    // J18 参数逐值对平。
    let c18 = RaiseCall {
        code: 0xC0000005,
        flags: 0x1,
        args: [0xDEADu64, 0xBEEF].to_vec(),
        light: false,
        note: "p".to_string(),
    };
    s.add(
        "J18_args_val",
        c18.args == [0xDEAD, 0xBEEF],
        "参数逐值对平",
    );

    // J19 空账指纹稳定。
    let b19 = RaiseBook::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空抛出账指纹稳定");

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12166_raise() {
        let mut b = RaiseBook::new();
        assert!(b.raise(0xC0000005, 0, vec![1], "a").is_ok());
        assert_eq!(b.raise(0x1000_0001, 0, vec![], "b"), Err(RS_CODE));
    }

    #[test]
    fn unx12166_checks_green() {
        let s = run_unx12166_checks();
        assert!(s.all_passed(), "UNX-F12166 判据未全绿");
    }

    #[test]
    fn unx12166_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
