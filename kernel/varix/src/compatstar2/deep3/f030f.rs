//! F030 深化批次四 · 安装脚本求值面（compatstar2/deep3 · G-A-30）。
//!
//! 批次一~三覆盖三族安装器向导/解包监听/产物登记等程序可见面；本批
//! 补齐主册【功能定义】「全语义对齐」的序列化/账本/容错面：微型脚本
//! 解释器（固定操作集 mkdir/copy/reg/lnk 四指令 × 定长 24 指令池，逐条
//! 求值带结果码）、变量替换（$VAR 名值表定长 8，未定义变量拒绝执行并
//! 记账）、条件分支求值（条件位图 → 跳转目标，非法跳转显性拒绝）、
//! 执行日志账（每指令一条结果记录定长 24，失败即止策略可开关）。判据
//! 对账：主册 G-A-30【设计细节】沙盒 temp 映射/.lnk 生成/蜂巢写入 + MS
//! MSI/NSIS 安装脚本动作语义对拍（四类动作；跳转只许前向防死循环——
//! 域内口径）。零堆纪律：定长指令池 + 定长日志 + 定长变量表，无
//! Vec/String/Box/format!，错误一律 Err 或计数账面，零静默。

use crate::checks::CheckSet;

/// 指令池容量（定长 24）。
pub const POOL_LEN: usize = 24;
/// 变量名值表容量（定长 8）。
pub const VARS_LEN: usize = 8;
/// 结果码：成功。
pub const RESULT_OK: u8 = 0;
/// 结果码：未定义变量（拒绝执行）。
pub const RESULT_UNDEF_VAR: u8 = 1;
/// 结果码：非法跳转（越界/非前向）。
pub const RESULT_ILLEGAL_JUMP: u8 = 2;
/// 条件位：无条件（恒执行）。
pub const NO_COND: u8 = 0xFF;

// 指令与变量面 ---------------------------------------------------------------

/// 固定操作集四指令（MS 安装脚本动作语义对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Mkdir,
    Copy,
    Reg,
    Lnk,
}

impl Op {
    pub const fn name(self) -> &'static str {
        match self {
            Op::Mkdir => "mkdir", Op::Copy => "copy", Op::Reg => "reg", Op::Lnk => "lnk",
        }
    }
}

/// 一条指令：操作 × 双参数 × 条件位 × 跳转目标。arg 为 "$NAME" 时按变量
/// 解析；cond_bit == NO_COND 恒执行；jump == -1 无跳转，≥0 为目标池下标
/// （须严格前向——防死循环，域内口径）。
#[derive(Clone, Copy)]
pub struct Instr {
    pub op: Op,
    pub arg1: &'static str,
    pub arg2: &'static str,
    pub cond_bit: u8,
    pub jump: i8,
}

/// 变量名值表（定长 8；已定义覆盖，满显性拒绝）。
pub struct VarTable {
    pub pairs: [Option<(&'static str, &'static str)>; VARS_LEN],
}

impl VarTable {
    pub const fn new() -> Self {
        VarTable { pairs: [None; VARS_LEN] }
    }

    /// 定义变量：满 → Err("vars-full")。
    pub fn define(&mut self, name: &'static str, value: &'static str) -> Result<(), &'static str> {
        for slot in self.pairs.iter_mut() {
            if let Some((n, _)) = slot {
                if *n == name { *slot = Some((name, value)); return Ok(()); }
            }
        }
        match self.pairs.iter_mut().find(|s| s.is_none()) {
            Some(slot) => { *slot = Some((name, value)); Ok(()) }
            None => Err("vars-full"),
        }
    }

    /// 查变量。
    pub fn lookup(&self, name: &str) -> Option<&'static str> {
        self.pairs.iter().flatten().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

/// $VAR 替换到定长缓冲：未定义变量显性拒绝；缓冲不足显性拒绝。返回字节数。
pub fn subst(src: &str, vars: &VarTable, out: &mut [u8]) -> Result<usize, &'static str> {
    let bytes = src.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            let name = core::str::from_utf8(&bytes[i + 1..j]).map_err(|_| "bad-var-name")?;
            match vars.lookup(name) {
                Some(v) => {
                    for &b in v.as_bytes() {
                        if n >= out.len() { return Err("subst-overflow"); }
                        out[n] = b;
                        n += 1;
                    }
                }
                None => return Err("undefined-var"),
            }
            i = j;
        } else {
            if n >= out.len() { return Err("subst-overflow"); }
            out[n] = bytes[i];
            n += 1;
            i += 1;
        }
    }
    Ok(n)
}

// 解释器与执行日志账 ----------------------------------------------------------

/// 一条执行日志（池下标 + 结果码 + 注记）。
#[derive(Clone, Copy)]
pub struct LogRec { pub pc: u8, pub code: u8, pub note: &'static str }

/// 微型脚本解释器：定长指令池逐条求值，每指令一条结果记录。
pub struct Interp {
    pub vars: VarTable,
    pub pool: [Option<Instr>; POOL_LEN],
    pub n: usize,
    /// 条件位图（16 位）。
    pub cond: u16,
    pub log: [Option<LogRec>; POOL_LEN],
    pub log_n: usize,
    /// 失败即止策略（可开关——主册【状态与异常】显性策略口径）。
    pub fail_fast: bool,
    pub halted: bool,
    pub jumps_taken: u32,
    pub illegal_jumps: u32,
    pub undefined_vars: u32,
}

impl Interp {
    pub const fn new(fail_fast: bool) -> Self {
        Interp {
            vars: VarTable::new(), pool: [None; POOL_LEN], n: 0, cond: 0,
            log: [None; POOL_LEN], log_n: 0, fail_fast, halted: false,
            jumps_taken: 0, illegal_jumps: 0, undefined_vars: 0,
        }
    }

    /// 装载指令：池满显性拒绝。
    pub fn load(&mut self, i: Instr) -> Result<usize, &'static str> {
        if self.n >= POOL_LEN { return Err("pool-full"); }
        self.pool[self.n] = Some(i);
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 设条件位。
    pub fn set_cond(&mut self, bit: u8, on: bool) {
        if bit < 16 && on { self.cond |= 1 << bit; }
        if bit < 16 && !on { self.cond &= !(1 << bit); }
    }

    /// 参数校验："$NAME" 形态必须可解析（未定义 → 显性拒绝）。
    fn arg_resolves(&self, arg: &'static str) -> bool {
        arg.strip_prefix('$').map_or(true, |rest| self.vars.lookup(rest).is_some())
    }

    /// 逐条求值：条件成立且带跳转 → 前向跳（非法跳转记账拒绝）；否则执行。
    /// 返回（执行条数, 失败条数）。前向跳保证每 pc 至多访问一次。
    pub fn run(&mut self) -> (usize, usize) {
        let mut executed = 0usize;
        let mut failed = 0usize;
        let mut pc = 0usize;
        while pc < self.n && !self.halted {
            let instr = self.pool[pc].unwrap();
            let cond_on = instr.cond_bit == NO_COND || (instr.cond_bit < 16 && (self.cond >> instr.cond_bit) & 1 == 1);
            if cond_on && instr.jump >= 0 {
                let t = instr.jump as usize;
                if t <= pc || t >= self.n {
                    self.illegal_jumps += 1;
                    self.log[pc] = Some(LogRec { pc: pc as u8, code: RESULT_ILLEGAL_JUMP, note: "illegal-jump" });
                    self.log_n += 1;
                    executed += 1; failed += 1;
                    if self.fail_fast { self.halted = true; }
                } else {
                    self.jumps_taken += 1;
                    self.log[pc] = Some(LogRec { pc: pc as u8, code: RESULT_OK, note: "jump" });
                    self.log_n += 1;
                    executed += 1;
                    pc = t;
                    continue;
                }
            } else {
                let ok = self.arg_resolves(instr.arg1) && self.arg_resolves(instr.arg2);
                let (code, note) = if ok {
                    (RESULT_OK, instr.op.name())
                } else {
                    self.undefined_vars += 1;
                    (RESULT_UNDEF_VAR, "undefined-var")
                };
                self.log[pc] = Some(LogRec { pc: pc as u8, code, note });
                self.log_n += 1;
                executed += 1;
                if code != RESULT_OK {
                    failed += 1;
                    if self.fail_fast { self.halted = true; }
                }
            }
            pc += 1;
        }
        (executed, failed)
    }
}

/// 装载一条 mkdir 指令（池满账自检的填充件）。
fn load_simple(it: &mut Interp, arg: &'static str) -> bool {
    it.load(Instr { op: Op::Mkdir, arg1: arg, arg2: "", cond_bit: NO_COND, jump: -1 }).is_ok()
}

/// 域自检（F030 深化批次四 · 解释器/变量/分支/日志账）。
pub fn run_f030f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F030-instscript-d4");
    // 1) 固定操作集四指令命名（MS 安装动作语义对拍）。
    cs.add(
        "op_set_four_instructions",
        Op::Mkdir.name() == "mkdir" && Op::Copy.name() == "copy" && Op::Reg.name() == "reg" && Op::Lnk.name() == "lnk",
        "",
    );
    // 2) 四指令序列全绿：逐条求值，日志逐条落账。
    let mut it = Interp::new(true);
    let _ = it.load(Instr { op: Op::Mkdir, arg1: "dir", arg2: "", cond_bit: NO_COND, jump: -1 });
    let _ = it.load(Instr { op: Op::Copy, arg1: "a.dat", arg2: "b.dat", cond_bit: NO_COND, jump: -1 });
    let _ = it.load(Instr { op: Op::Reg, arg1: "hkcu", arg2: "key", cond_bit: NO_COND, jump: -1 });
    let _ = it.load(Instr { op: Op::Lnk, arg1: "app", arg2: "menu", cond_bit: NO_COND, jump: -1 });
    let (exec, fail) = it.run();
    cs.add("four_ops_sequence_ok", exec == 4 && fail == 0 && it.log_n == 4, "");
    // 3) 变量替换：$TMP 展开为沙盒路径（定长缓冲，18 字节）。
    let mut vt = VarTable::new();
    let _ = vt.define("TMP", "D:/sandbox/tmp");
    let mut buf = [0u8; 64];
    let sub = subst("$TMP/app", &vt, &mut buf);
    cs.add("var_substitution_ok", sub == Ok(18) && &buf[..18] == b"D:/sandbox/tmp/app", "");
    // 4) 未定义变量拒绝执行并记账（fail_fast 即止，后续不执行）。
    let mut bad = Interp::new(true);
    let _ = bad.load(Instr { op: Op::Copy, arg1: "$MISSING", arg2: "x", cond_bit: NO_COND, jump: -1 });
    let _ = bad.load(Instr { op: Op::Mkdir, arg1: "never", arg2: "", cond_bit: NO_COND, jump: -1 });
    let (exec, fail) = bad.run();
    cs.add("undefined_var_rejected_halt", exec == 1 && fail == 1 && bad.halted && bad.undefined_vars == 1 && bad.log_n == 1, "");
    // 5) fail_fast 关闭：失败后继续，逐条落账。
    let mut soft = Interp::new(false);
    let _ = soft.load(Instr { op: Op::Mkdir, arg1: "ok", arg2: "", cond_bit: NO_COND, jump: -1 });
    let _ = soft.load(Instr { op: Op::Copy, arg1: "$GONE", arg2: "x", cond_bit: NO_COND, jump: -1 });
    let _ = soft.load(Instr { op: Op::Lnk, arg1: "app", arg2: "menu", cond_bit: NO_COND, jump: -1 });
    let (exec, fail) = soft.run();
    cs.add("fail_fast_off_continues", exec == 3 && fail == 1 && !soft.halted && soft.undefined_vars == 1, "");
    // 6) 条件跳转：位 0 置位 → pc0 前向跳 pc3，中段被跳过。
    let mut jt = Interp::new(true);
    let _ = jt.load(Instr { op: Op::Mkdir, arg1: "skip", arg2: "", cond_bit: 0, jump: 3 });
    let _ = jt.load(Instr { op: Op::Copy, arg1: "skipped", arg2: "", cond_bit: NO_COND, jump: -1 });
    let _ = jt.load(Instr { op: Op::Reg, arg1: "skipped", arg2: "", cond_bit: NO_COND, jump: -1 });
    let _ = jt.load(Instr { op: Op::Lnk, arg1: "hit", arg2: "", cond_bit: NO_COND, jump: -1 });
    jt.set_cond(0, true);
    let (exec, fail) = jt.run();
    cs.add("cond_jump_taken_skips", exec == 2 && fail == 0 && jt.jumps_taken == 1 && jt.log_n == 2, "");
    // 7) 条件不成立：同池照常顺序执行（条件位图 → 跳转目标的反面）。
    let mut jf = Interp::new(true);
    let _ = jf.load(Instr { op: Op::Mkdir, arg1: "exec", arg2: "", cond_bit: 0, jump: 3 });
    let _ = jf.load(Instr { op: Op::Copy, arg1: "a", arg2: "b", cond_bit: NO_COND, jump: -1 });
    let _ = jf.load(Instr { op: Op::Reg, arg1: "k", arg2: "v", cond_bit: NO_COND, jump: -1 });
    let _ = jf.load(Instr { op: Op::Lnk, arg1: "app", arg2: "menu", cond_bit: NO_COND, jump: -1 });
    let (exec, fail) = jf.run();
    cs.add("cond_false_executes_normally", exec == 4 && fail == 0 && jf.jumps_taken == 0, "");
    // 8) 非法跳转双态显性拒绝：越界（≥ 池长）与回跳（≤ pc，防死循环）。
    let mut ij = Interp::new(true);
    let _ = ij.load(Instr { op: Op::Mkdir, arg1: "x", arg2: "", cond_bit: NO_COND, jump: 9 });
    let _ = ij.run();
    let over_refused = ij.illegal_jumps == 1 && ij.halted;
    let mut bj = Interp::new(true);
    let _ = bj.load(Instr { op: Op::Mkdir, arg1: "a", arg2: "", cond_bit: NO_COND, jump: -1 });
    let _ = bj.load(Instr { op: Op::Copy, arg1: "b", arg2: "", cond_bit: 0, jump: 0 });
    bj.set_cond(0, true);
    let _ = bj.run();
    cs.add("illegal_jump_refused", over_refused && bj.illegal_jumps == 1 && bj.halted, "");
    // 9) 容量满双账显性拒绝：指令池第 25 条 → pool-full；变量表第 9 名 → vars-full。
    let mut pf = Interp::new(true);
    let mut full_ok = (0..POOL_LEN).all(|_| load_simple(&mut pf, "d"));
    full_ok = full_ok && pf.load(Instr { op: Op::Mkdir, arg1: "e", arg2: "", cond_bit: NO_COND, jump: -1 }) == Err("pool-full");
    let mut vf = VarTable::new();
    full_ok = full_ok && ["v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8"].iter().all(|n| vf.define(n, "x").is_ok())
        && vf.define("v9", "x") == Err("vars-full");
    cs.add("pool_and_vars_full_refused", full_ok, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_ledger_records_each_pc() {
        // 每指令一条结果记录：pc 序号与结果码逐条对账。
        let mut it = Interp::new(false);
        let _ = it.load(Instr { op: Op::Mkdir, arg1: "a", arg2: "", cond_bit: NO_COND, jump: -1 });
        let _ = it.load(Instr { op: Op::Copy, arg1: "$GONE", arg2: "", cond_bit: NO_COND, jump: -1 });
        let _ = it.load(Instr { op: Op::Lnk, arg1: "c", arg2: "", cond_bit: NO_COND, jump: -1 });
        let (exec, fail) = it.run();
        assert_eq!((exec, fail), (3, 1));
        assert_eq!(it.log[0].unwrap().pc, 0);
        assert_eq!(it.log[0].unwrap().code, RESULT_OK);
        assert_eq!(it.log[1].unwrap().code, RESULT_UNDEF_VAR);
        assert_eq!(it.log[2].unwrap().pc, 2);
    }

    #[test]
    fn subst_overflow_and_undefined_refused() {
        // 缓冲不足与未定义变量双拒绝（零静默）。
        let mut vt = VarTable::new();
        let _ = vt.define("BIG", "0123456789");
        let mut small = [0u8; 4];
        assert_eq!(subst("$BIG", &vt, &mut small), Err("subst-overflow"));
        let mut out = [0u8; 32];
        assert_eq!(subst("$NOPE", &vt, &mut out), Err("undefined-var"));
        let n = subst("plain", &vt, &mut out).unwrap();
        assert_eq!(&out[..n], b"plain");
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f030f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
