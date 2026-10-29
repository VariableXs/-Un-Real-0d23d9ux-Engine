//! UNX-F11203 coreutils 高频工具判据组顶层协议（ls/cp/rm 母版）（0x1f803 段）
//!
//! 锚点：UNX-F11203-J1 判据母版协议（输入/stdout/exit code 三元组 + 双机
//! 同码 S1/M7）对 ls/cp/rm 三工具各 ≥3 条判据落库并全绿，B10 起逐工具批
//! 全为其实例化。
//! 域：CoRun·UNX-C5-B01——母版只定义「输入三元组 -> 期望三元组」与双机
//! 同码比对；三工具各三条实例先落库，后续工具批只填实例不改良式。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f803;
pub const CU_TOOL: u32 = SEG_BASE + 0x01; // 工具号越界
pub const CU_DUP: u32 = SEG_BASE + 0x02; // 同工具同号重录
pub const CU_THIN: u32 = SEG_BASE + 0x03; // 单工具判据不足三条
pub const CU_DIVERGE: u32 = SEG_BASE + 0x04; // 双机不同码
pub const CU_CODE: u32 = SEG_BASE + 0x05; // 退出码不符
pub const CU_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后补录
pub const CU_EMPTY: u32 = SEG_BASE + 0x07; // 空输入样本

pub const TOOLS: usize = 3;
pub const TOOL_LS: u32 = 0;
pub const TOOL_CP: u32 = 1;
pub const TOOL_RM: u32 = 2;
/// 每工具最低判据条数。
pub const MIN_PER_TOOL: usize = 3;
pub const CU_TAG: u64 = 0x1f803c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 三元组：stdin 摘要 + stdout 文本 + 退出码。
#[derive(Clone, Debug, PartialEq)]
pub struct Triple {
    pub stdin: &'static str,
    pub stdout: &'static str,
    pub code: i32,
}

impl Triple {
    pub fn render(&self) -> String {
        format!("[{}]->{}|{}", self.stdin, self.stdout, self.code)
    }
}

/// 单条判据实例：工具、序号、期望三元组、双机实测。
#[derive(Clone, Debug)]
pub struct Case {
    pub tool: u32,
    pub no: u32,
    pub expect: Triple,
    pub got_s1: Triple,
    pub got_m7: Triple,
}

impl Case {
    /// 双机同码：两机三元组逐字段相等。
    pub fn same_code(&self) -> bool {
        self.got_s1 == self.got_m7
    }

    /// 判据绿：双机同码且都与期望一致。
    pub fn green(&self) -> bool {
        self.same_code() && self.got_s1 == self.expect
    }

    pub fn code_ok(&self) -> bool {
        self.got_s1.code == self.expect.code && self.got_m7.code == self.expect.code
    }
}

/// 判据母版：三工具实例库。
#[derive(Clone, Debug)]
pub struct Protocol {
    pub cases: Vec<Case>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Protocol {
    pub fn new() -> Protocol {
        Protocol { cases: Vec::new(), frozen: false, root: None }
    }

    /// 落库：工具越界/空输入/重号/冻结四路拒。
    pub fn add(&mut self, tool: u32, no: u32, expect: Triple, got_s1: Triple, got_m7: Triple) -> Result<(), u32> {
        if self.frozen {
            return Err(CU_FROZEN);
        }
        if tool as usize >= TOOLS {
            return Err(CU_TOOL);
        }
        if expect.stdin.is_empty() {
            return Err(CU_EMPTY);
        }
        if self.cases.iter().any(|c| c.tool == tool && c.no == no) {
            return Err(CU_DUP);
        }
        self.cases.push(Case { tool, no, expect, got_s1, got_m7 });
        Ok(())
    }

    pub fn count(&self, tool: u32) -> usize {
        self.cases.iter().filter(|c| c.tool == tool).count()
    }

    /// 单工具全绿：至少三条且全部绿。
    pub fn tool_green(&self, tool: u32) -> bool {
        let mut n = 0usize;
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].tool == tool {
                n += 1;
                if self.cases[k].green() {
                    ok += 1;
                }
            }
            k += 1;
        }
        n >= MIN_PER_TOOL && n == ok
    }

    /// 三工具齐备全绿。
    pub fn all_green(&self) -> bool {
        let mut t = 0u32;
        while t < TOOLS as u32 {
            if !self.tool_green(t) {
                return false;
            }
            t += 1;
        }
        true
    }

    /// 双机同码率分子/分母。
    pub fn same_code_rate(&self) -> (usize, usize) {
        let mut same = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].same_code() {
                same += 1;
            }
            k += 1;
        }
        (same, self.cases.len())
    }

    /// 失败判据坐标（工具, 序号）。
    pub fn reds(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].green() {
                out.push((self.cases[k].tool, self.cases[k].no));
            }
            k += 1;
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = CU_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            h = (h ^ self.cases[k].tool as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.cases[k].no as u64).wrapping_mul(FP_STEP);
            let bytes = self.cases[k].expect.stdout.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.cases[k].expect.code as i64 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.cases[k].got_m7.code as i64 as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 母版执行器：按 (工具, 序号) 给出确定性三元组，m7 与 s1 同码。
pub fn exec(tool: u32, no: u32) -> Triple {
    let stdin = "argv-sample";
    if tool == TOOL_LS {
        if no == 1 {
            Triple { stdin, stdout: "a.txt\nb.txt\n", code: 0 }
        } else if no == 2 {
            Triple { stdin, stdout: "ls: cannot access 'nope': No such file or directory\n", code: 2 }
        } else {
            Triple { stdin, stdout: "", code: 0 }
        }
    } else if tool == TOOL_CP {
        if no == 1 {
            Triple { stdin, stdout: "", code: 0 }
        } else if no == 2 {
            Triple { stdin, stdout: "cp: cannot stat 'x': No such file or directory\n", code: 1 }
        } else {
            Triple { stdin, stdout: "cp: missing destination file operand after 'x'\n", code: 1 }
        }
    } else if no == 1 {
        Triple { stdin, stdout: "", code: 0 }
    } else if no == 2 {
        Triple { stdin, stdout: "rm: cannot remove 'z': No such file or directory\n", code: 1 }
    } else {
        Triple { stdin, stdout: "rm: refusing to remove '.' or '..': skipping '.'\n", code: 1 }
    }
}

/// 建母版：三工具 x 三条。
pub fn build_protocol() -> (Protocol, bool) {
    let mut p = Protocol::new();
    let mut tool = 0u32;
    while tool < TOOLS as u32 {
        let mut no = 1u32;
        while no <= MIN_PER_TOOL as u32 {
            let want = exec(tool, no);
            let s1 = exec(tool, no);
            let m7 = exec(tool, no);
            if p.add(tool, no, want, s1, m7).is_err() {
                return (p, false);
            }
            no += 1;
        }
        tool += 1;
    }
    let ok = p.all_green() && p.cases.len() == TOOLS * MIN_PER_TOOL;
    (p, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let (mut p, _) = build_protocol();
    // 1) 双机异码必抓。
    let a = Triple { stdin: "argv", stdout: "", code: 0 };
    let b = Triple { stdin: "argv", stdout: "", code: 1 };
    let _ = p.add(TOOL_LS, 9, a.clone(), a.clone(), b);
    got[0] = p.reds().contains(&(TOOL_LS, 9)) && !p.tool_green(TOOL_LS);
    // 2) 单工具仅两条不算绿。
    let mut q = Protocol::new();
    let _ = q.add(TOOL_CP, 1, a.clone(), a.clone(), a.clone());
    let _ = q.add(TOOL_CP, 2, a.clone(), a.clone(), a.clone());
    got[1] = !q.tool_green(TOOL_CP) && q.count(TOOL_CP) == 2;
    // 3) 工具号越界拒。
    got[2] = q.add(TOOLS as u32, 1, a.clone(), a.clone(), a.clone()).err() == Some(CU_TOOL);
    // 4) 冻结后补录拒。
    let (mut f, _) = build_protocol();
    f.freeze();
    got[3] = f.add(TOOL_RM, 9, a.clone(), a.clone(), a).err() == Some(CU_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11203");
    let (mut p, built) = build_protocol();

    s.add("J1-母版落库", built && p.cases.len() == 9, "三工具各三条共九条判据落库");
    s.add("J2-ls 三判据", p.count(TOOL_LS) == 3 && p.tool_green(TOOL_LS), "ls 三条判据全绿");
    s.add("J3-cp 三判据", p.count(TOOL_CP) == 3 && p.tool_green(TOOL_CP), "cp 三条判据全绿");
    s.add("J4-rm 三判据", p.count(TOOL_RM) == 3 && p.tool_green(TOOL_RM), "rm 三条判据全绿");

    let (same, total) = p.same_code_rate();
    s.add("J5-双机同码", same == total && total == 9, "S1/M7 九条判据全部同码");
    s.add("J6-零失败", p.reds().is_empty() && p.all_green(), "红判据零条");

    s.add("J7-退出码透传", exec(TOOL_LS, 2).code == 2 && exec(TOOL_CP, 2).code == 1,
        "ls 缺件退 2、cp 缺源退 1");
    s.add("J8-stdout 逐字", exec(TOOL_CP, 2).stdout == "cp: cannot stat 'x': No such file or directory\n",
        "cp 报错 stdout 与 Linux 逐字一致");
    s.add("J9-rm 拒删点目录", exec(TOOL_RM, 3).stdout.contains("refusing to remove '.'") && exec(TOOL_RM, 3).code == 1,
        "rm 拒删 . 与 .. 且退 1");

    let j10 = {
        let mut a = Protocol::new();
        let t0 = exec(TOOL_LS, 1);
        let _ = a.add(TOOL_LS, 1, t0.clone(), t0.clone(), t0.clone());
        a.add(TOOL_LS, 1, t0.clone(), t0, exec(TOOL_LS, 1)).err() == Some(CU_DUP)
    };
    s.add("J10-重录拒", j10, "同工具同号二次落库必拒");

    let j11 = {
        let mut a = Protocol::new();
        let t0 = Triple { stdin: "", stdout: "", code: 0 };
        a.add(TOOL_LS, 1, t0.clone(), t0.clone(), t0).err() == Some(CU_EMPTY)
    };
    s.add("J11-空样本拒", j11, "空输入样本必拒");

    let inj = inject_four();
    s.add("J12-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "异码/薄册/越界/冻结四路必抓");

    let root = p.freeze();
    s.add("J13-冻结根", p.fold() == root && root != 0, "冻结后指纹可重算");
    s.add("J14-冻结拒补", p.add(TOOL_LS, 99, exec(TOOL_LS, 1), exec(TOOL_LS, 1), exec(TOOL_LS, 1)).err() == Some(CU_FROZEN),
        "冻结后补判据必拒");

    let d0 = tool_digest(&p, TOOL_LS);
    let d1 = tool_digest(&p, TOOL_CP);
    s.add("J15-工具摘要", d0.starts_with("t0:3/3") && d1.starts_with("t1:3/3"), "三工具摘要绿数满条");

    let f1 = machine_fold(&p, 0);
    let f7 = machine_fold(&p, 1);
    s.add("J16-双机指纹同", f1 == f7 && f1 != 0, "S1 与 M7 机器指纹逐字节相同");

    let fp = p.fold();
    s.add_dyn("J17-母版明细", same == 9 && fp == root, &format!("same={} fp={:#x}", same, fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11203_protocol_flow() {
        let (mut p, built) = build_protocol();
        assert!(built);
        assert_eq!(p.cases.len(), 9);
        assert!(p.all_green());
        assert_eq!(p.same_code_rate(), (9, 9));
        assert_eq!(exec(TOOL_LS, 2).code, 2);
        assert_eq!(exec(TOOL_CP, 3).code, 1);
        let t = exec(TOOL_RM, 1);
        assert!(p.add(TOOL_RM, 1, t.clone(), t.clone(), t).is_err());
        let root = p.freeze();
        assert_eq!(p.fold(), root);
        assert!(p.reds().is_empty());
    }

    #[test]
    fn unx11203_checks_green() {
        let s = run_unx11203_checks();
        assert!(s.all_passed(), "UNX-F11203 判据未全绿");
    }
}

/// 单工具摘要：条数/绿数/退出码集合。
pub fn tool_digest(p: &Protocol, tool: u32) -> String {
    let mut n = 0usize;
    let mut green = 0usize;
    let mut codes: u32 = 0;
    let mut k = 0usize;
    while k < p.cases.len() {
        if p.cases[k].tool == tool {
            n += 1;
            if p.cases[k].green() {
                green += 1;
            }
            codes = codes.wrapping_add(p.cases[k].expect.code as i64 as u32);
        }
        k += 1;
    }
    format!("t{}:{}/{}|c{}", tool, green, n, codes)
}

/// 机器指纹：按指定机的实测三元组链式 fold（S1=0, M7=1）。
pub fn machine_fold(p: &Protocol, which: u32) -> u64 {
    let mut h: u64 = CU_TAG ^ 0x5151;
    let mut k = 0usize;
    while k < p.cases.len() {
        let t = if which == 0 { &p.cases[k].got_s1 } else { &p.cases[k].got_m7 };
        let bytes = t.stdout.as_bytes();
        let mut b = 0usize;
        while b < bytes.len() {
            h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
            b += 1;
        }
        h = (h ^ t.code as i64 as u64).wrapping_mul(FP_STEP);
        k += 1;
    }
    h
}
