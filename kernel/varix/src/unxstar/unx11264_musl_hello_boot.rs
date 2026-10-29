//! UNX-F11264 musl 静态 hello 一级自举判据（0x1f400 段）
//! 纯功能模型：静态 hello 的 10 轮双机自举账——每轮记甲乙两机 stdout 与
//! 退出码，甲机为基准（输出必须逐字 "hello"、退出 0），乙机与甲机逐字同码；
//! 轮号越界、重轮、输出走样、退出非零、双机不同码五路各报自有码。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f400 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f400;
/// 轮号越界（0 或超上界）。
pub const HB_ROUND: u32 = SEG_BASE + 0x01;
/// 轮号重登。
pub const HB_DUP: u32 = SEG_BASE + 0x02;
/// 基准机输出非 "hello"。
pub const HB_OUT: u32 = SEG_BASE + 0x03;
/// 退出码非 0。
pub const HB_EXIT: u32 = SEG_BASE + 0x04;
/// 双机输出不同码。
pub const HB_MACH: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const HB_FROZEN: u32 = SEG_BASE + 0x06;
/// 轮次表已满。
pub const HB_CAP: u32 = SEG_BASE + 0x07;

/// 自举轮数（10 轮全绿）。
pub const ROUND_NEED: usize = 10;
/// 轮号上界（与定容解耦，保证越界码与满表码各自可达）。
pub const NO_MAX: u32 = 99;
/// 轮次表定容。
pub const ROUND_CAP: usize = 12;
/// 期望输出（逐字）。
pub const HELLO: &str = "hello";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_64;

/// 单轮自举：甲乙两机输出与退出码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Round {
    pub no: u32,
    pub out_a: String,
    pub out_b: String,
    pub code_a: i32,
    pub code_b: i32,
}

impl Round {
    /// 双机同码且均为 hello/0。
    pub fn green(&self) -> bool {
        self.out_a == HELLO
            && self.out_b == HELLO
            && self.code_a == 0
            && self.code_b == 0
            && self.out_a == self.out_b
    }

    /// 渲染：`R01 hello/hello 0/0`。
    pub fn render(&self) -> String {
        format!("R{:02} {}/{} {}/{}", self.no, self.out_a, self.out_b, self.code_a, self.code_b)
    }
}

/// 自举账：逐轮登记 → 全绿判据 → 封册。
#[derive(Clone, Debug)]
pub struct BootLog {
    pub rounds: Vec<Round>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl BootLog {
    pub fn new() -> BootLog {
        BootLog { rounds: Vec::new(), accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 登记一轮。判定序：封册 → 轮号域 → 重轮 → 基准输出 → 退出码
    /// → 双机同码 → 定容。重轮早于输出：重复号报重轮，不被输出门禁吞。
    pub fn submit(&mut self, no: u32, a_out: &str, a_code: i32, b_out: &str, b_code: i32)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_FROZEN);
        }
        if no == 0 || no > NO_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_ROUND);
        }
        if self.rounds.iter().any(|r| r.no == no) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_DUP);
        }
        if a_out != HELLO {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_OUT);
        }
        if a_code != 0 || b_code != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_EXIT);
        }
        if a_out != b_out {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_MACH);
        }
        if self.rounds.len() >= ROUND_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(HB_CAP);
        }
        self.rounds.push(Round {
            no,
            out_a: clone_str(a_out),
            out_b: clone_str(b_out),
            code_a: a_code,
            code_b: b_code,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.rounds.len().wrapping_sub(1))
    }

    /// 红轮轮号清单（登记序）。
    pub fn red(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rounds.len() {
            if let Some(r) = self.rounds.get(k) {
                if !r.green() {
                    v.push(r.no);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 全体绿：10 轮齐、逐轮双机 hello/0 同码、红轮零。
    pub fn green(&self) -> bool {
        if self.rounds.len() != ROUND_NEED {
            return false;
        }
        let mut k = 0usize;
        while k < self.rounds.len() {
            match self.rounds.get(k) {
                Some(r) => {
                    if !r.green() {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 双机逐轮同码（不判内容，只判同一性）。
    pub fn same_code(&self) -> bool {
        self.rounds.iter().all(|r| r.out_a == r.out_b && r.code_a == r.code_b)
    }

    /// FNV-1a 链指纹：轮号/双机输出/双机退出码序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.rounds.len() {
            if let Some(r) = self.rounds.get(k) {
                h = (h ^ r.no as u64).wrapping_mul(FNV_PRIME);
                for s in [&r.out_a, &r.out_b] {
                    let b = s.as_bytes();
                    let mut j = 0usize;
                    while j < b.len() {
                        if let Some(x) = b.get(j) {
                            h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                        }
                        j = j.wrapping_add(1);
                    }
                }
                h = (h ^ r.code_a as u32 as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.code_b as u32 as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 借串入堆。
fn clone_str(s: &str) -> String {
    let mut o = String::new();
    o.push_str(s);
    o
}

/// 十轮全绿基准账。
pub fn full_boot() -> BootLog {
    let mut l = BootLog::new();
    let mut i = 1u32;
    while i <= ROUND_NEED as u32 {
        let _ = l.submit(i, HELLO, 0, HELLO, 0);
        i = i.wrapping_add(1);
    }
    l
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 轮号越界
    let mut a = BootLog::new();
    got[0] = a.submit(0, HELLO, 0, HELLO, 0).err() == Some(HB_ROUND);
    // (b) 重轮
    let mut b = BootLog::new();
    let _ = b.submit(1, HELLO, 0, HELLO, 0);
    got[1] = b.submit(1, HELLO, 0, HELLO, 0).err() == Some(HB_DUP);
    // (c) 输出走样
    let mut c = BootLog::new();
    got[2] = c.submit(1, "helo", 0, "helo", 0).err() == Some(HB_OUT);
    // (d) 封册后写账
    let mut d = full_boot();
    let _ = d.freeze();
    got[3] = d.submit(11, HELLO, 0, HELLO, 0).err() == Some(HB_FROZEN);
    got
}

/// 静态 hello 一级自举判据。
pub fn run_unx11264_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11264");

    // X1 十轮全绿
    let x1 = full_boot().green();
    s.add("X1-十轮全绿", x1, "10 轮双机 hello/0 全绿");

    // X2 双机同码
    let x2 = {
        let l = full_boot();
        l.same_code() && l.rounds.len() == ROUND_NEED
    };
    s.add("X2-双机同码", x2, "逐轮输出与退出码甲乙一致");

    // X3 红轮零
    let x3 = {
        let l = full_boot();
        l.red().is_empty() && l.accepts == 10 && l.rejects == 0
    };
    s.add("X3-红轮零", x3, "红轮清单空且计数 10/0");

    // X4 输出走样必抓
    let x4 = {
        let mut l = BootLog::new();
        l.submit(1, "hello!", 0, "hello!", 0).err() == Some(HB_OUT)
    };
    s.add("X4-输出走样", x4, "多一字节亦报输出走样");

    // X5 退出码必抓
    let x5 = {
        let mut l = BootLog::new();
        l.submit(1, HELLO, 0, HELLO, 3).err() == Some(HB_EXIT)
    };
    s.add("X5-退出非零", x5, "乙机退出 3 报退出码错");

    // X6 双机异码必抓
    let x6 = {
        let mut l = BootLog::new();
        l.submit(1, HELLO, 0, "hell0", 0).err() == Some(HB_MACH)
    };
    s.add("X6-双机异码", x6, "甲乙输出不同报异码");

    // X7 重轮
    let x7 = {
        let mut l = BootLog::new();
        let _ = l.submit(4, HELLO, 0, HELLO, 0);
        l.submit(4, HELLO, 0, HELLO, 0).err() == Some(HB_DUP)
    };
    s.add("X7-重轮", x7, "同轮二次登记报重轮");

    // X8 轮号域
    let x8 = {
        let mut a = BootLog::new();
        let zero = a.submit(0, HELLO, 0, HELLO, 0).err() == Some(HB_ROUND);
        let over = a.submit(NO_MAX.wrapping_add(1), HELLO, 0, HELLO, 0).err() == Some(HB_ROUND);
        zero && over
    };
    s.add("X8-轮号域", x8, "0 号与 100 号均报越界");

    // X9 轮数不足不绿
    let x9 = {
        let mut l = BootLog::new();
        let mut i = 1u32;
        while i < ROUND_NEED as u32 {
            let _ = l.submit(i, HELLO, 0, HELLO, 0);
            i = i.wrapping_add(1);
        }
        !l.green() && l.rounds.len() == ROUND_NEED - 1
    };
    s.add("X9-轮数不足", x9, "9 轮齐但不足 10 轮不绿");

    // X10 轮次定容
    let x10 = {
        let mut l = BootLog::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= ROUND_CAP as u32 {
            if l.submit(i, HELLO, 0, HELLO, 0).is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && l.submit(13, HELLO, 0, HELLO, 0).err() == Some(HB_CAP) && !l.green()
    };
    s.add("X10-轮次定容", x10, "12 轮后第 13 轮报满且不算绿");

    // X11 指纹确定
    let x11 = {
        let l = full_boot();
        let a = l.fold();
        a == l.fold() && a != 0
    };
    s.add("X11-指纹确定", x11, "同账两次指纹一致且非零");

    // X12 封册幂等
    let x12 = {
        let mut l = full_boot();
        let r1 = l.freeze();
        let r2 = l.freeze();
        let live = l.fold();
        l.rounds.clear();
        let r3 = l.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != l.fold()
    };
    s.add("X12-封册幂等", x12, "二次封册回原根，账变不改根");

    // X13 封册拒写与账行明细
    let (x13, line13) = {
        let mut l = full_boot();
        let _ = l.freeze();
        let rej = l.submit(11, HELLO, 0, HELLO, 0).err() == Some(HB_FROZEN);
        let line = format!("frozen={} last={}", rej, match l.rounds.last() {
            Some(r) => r.render(),
            None => String::from("-"),
        });
        (rej && line.contains("R10 hello/hello 0/0"), line)
    };
    s.add_dyn("X13-封册明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11264_flow() {
        let mut l = full_boot();
        assert!(l.green());
        assert!(l.same_code());
        assert!(l.red().is_empty());
        assert_eq!(l.accepts, 10);
        assert_eq!(l.rejects, 0);
        assert_eq!(l.rounds.len(), 10);
        let r1 = l.freeze();
        let r2 = l.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(l.root == r1);
        assert!(l.submit(11, HELLO, 0, HELLO, 0).err() == Some(HB_FROZEN));
    }

    #[test]
    fn unx11264_gates() {
        let mut l = BootLog::new();
        assert!(l.submit(1, HELLO, 0, HELLO, 0).is_ok());
        assert!(l.submit(1, HELLO, 0, HELLO, 0).err() == Some(HB_DUP));
        assert!(l.submit(0, HELLO, 0, HELLO, 0).err() == Some(HB_ROUND));
        assert!(l.submit(100, HELLO, 0, HELLO, 0).err() == Some(HB_ROUND));
        assert!(l.submit(2, "hell", 0, "hell", 0).err() == Some(HB_OUT));
        assert!(l.submit(2, HELLO, 1, HELLO, 1).err() == Some(HB_EXIT));
        assert!(l.submit(2, HELLO, 0, "hell0", 0).err() == Some(HB_MACH));
        assert!(l.submit(2, HELLO, 0, HELLO, 0).is_ok());
        let mut no = 3u32;
        while no <= ROUND_CAP as u32 {
            assert!(l.submit(no, HELLO, 0, HELLO, 0).is_ok());
            no += 1;
        }
        assert!(l.submit(13, HELLO, 0, HELLO, 0).err() == Some(HB_CAP));
        assert!(l.freeze().is_some());
        assert!(l.submit(14, HELLO, 0, HELLO, 0).err() == Some(HB_FROZEN));
        assert_eq!(l.accepts, 12);
        assert_eq!(l.rejects, 8);
    }

    #[test]
    fn unx11264_checks_green() {
        assert!(run_unx11264_checks().all_passed(), "UNX-F11264 判据未全绿");
    }
}
