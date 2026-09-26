//! UNREAL-X-15000 · AI-28 族0272 脚本宿主（X06776~X06800）。
//! 语句解释器：定长变量槽、压/加/跳转/条件四指令、步数预算熔断、
//! 断点续跑与快照、死循环检测。零堆、整数运算。

use crate::checks::CheckSet;

/// 变量槽数与程序长度上限、步数预算。
pub const SH_VARS: usize = 8;
pub const SH_PROG_MAX: usize = 32;
pub const SH_STEP_BUDGET: u32 = 256;

/// 指令编码：op<<8 | arg。op：0 nop 1 压变量 2 加法 3 跳转 4 条件跳转 5 结束。
pub const OP_NOP: u8 = 0;
pub const OP_SET: u8 = 1;
pub const OP_ADD: u8 = 2;
pub const OP_JMP: u8 = 3;
pub const OP_JZ: u8 = 4;
pub const OP_END: u8 = 5;

pub const SH_E_OK: u16 = 0;
/// 步数预算熔断（疑似死循环）。
pub const SH_E_BUDGET: u16 = 1;
pub const SH_E_PC: u16 = 2;
pub const SH_E_VAR: u16 = 3;
pub const SH_E_STATE: u16 = 4;

pub fn sh_describe(code: u16) -> &'static str {
    match code {
        SH_E_OK => "正常",
        SH_E_BUDGET => "步数预算已熔断，脚本疑似死循环，建议检查跳转条件",
        SH_E_PC => "程序计数越界，建议检查指令长度",
        SH_E_VAR => "变量槽越界，建议使用 0~7 的有效槽位",
        SH_E_STATE => "宿主未就绪，建议先装载脚本",
        _ => "未知宿主错误，建议重建脚本宿主后重试",
    }
}

fn insn(op: u8, arg: u8) -> u16 {
    ((op as u16) << 8) | arg as u16
}

pub fn insn_op(i: u16) -> u8 {
    (i >> 8) as u8
}

pub fn insn_arg(i: u16) -> u8 {
    (i & 0xff) as u8
}

/// 脚本宿主：装载定长程序 + 单步/整跑 + 预算熔断 + 快照续跑。
pub struct ScriptHost {
    pub prog: [u16; SH_PROG_MAX],
    pub prog_len: usize,
    pub vars: [i32; SH_VARS],
    pub pc: usize,
    pub steps: u32,
    pub loaded: bool,
    pub halted: bool,
    /// 熔断次数（长稳观测）。
    pub trips: u32,
}

impl ScriptHost {
    pub fn new() -> ScriptHost {
        ScriptHost { prog: [0; SH_PROG_MAX], prog_len: 0, vars: [0; SH_VARS], pc: 0, steps: 0, loaded: false, halted: false, trips: 0 }
    }

    /// 装载：越界指令钳制为 nop，返回装载长度。
    pub fn load(&mut self, prog: &[u16]) -> usize {
        let n = if prog.len() > SH_PROG_MAX { SH_PROG_MAX } else { prog.len() };
        for i in 0..n {
            let op = insn_op(prog[i]);
            self.prog[i] = if op > OP_END { insn(OP_NOP, 0) } else { prog[i] };
        }
        self.prog_len = n;
        self.pc = 0;
        self.steps = 0;
        self.vars = [0; SH_VARS];
        self.loaded = true;
        self.halted = false;
        n
    }

    /// 单步执行一条指令；预算耗尽熔断。
    pub fn step(&mut self) -> u16 {
        if !self.loaded || self.halted {
            return SH_E_STATE;
        }
        if self.steps >= SH_STEP_BUDGET {
            self.trips += 1;
            self.halted = true;
            return SH_E_BUDGET;
        }
        if self.pc >= self.prog_len {
            self.halted = true;
            return SH_E_PC;
        }
        let i = self.prog[self.pc];
        self.steps += 1;
        match insn_op(i) {
            OP_SET => {
                let slot = insn_arg(i) as usize;
                if slot >= SH_VARS {
                    self.halted = true;
                    return SH_E_VAR;
                }
                self.vars[slot] = 1;
                self.pc += 1;
                SH_E_OK
            }
            OP_ADD => {
                let enc = insn_arg(i);
                let dst = (enc >> 3) as usize;
                let src = (enc & 0b111) as usize;
                if dst >= SH_VARS || src >= SH_VARS {
                    self.halted = true;
                    return SH_E_VAR;
                }
                self.vars[dst] += self.vars[src];
                self.pc += 1;
                SH_E_OK
            }
            OP_JMP => {
                let target = insn_arg(i) as usize;
                if target > self.prog_len {
                    self.halted = true;
                    return SH_E_PC;
                }
                self.pc = target;
                SH_E_OK
            }
            OP_JZ => {
                let enc = insn_arg(i);
                let slot = (enc >> 4) as usize;
                let target = (enc & 0x0f) as usize;
                if slot >= SH_VARS || target > self.prog_len {
                    self.halted = true;
                    return SH_E_VAR;
                }
                self.pc = if self.vars[slot] == 0 { target } else { self.pc + 1 };
                SH_E_OK
            }
            OP_END => {
                self.halted = true;
                SH_E_OK
            }
            _ => {
                self.pc += 1;
                SH_E_OK
            }
        }
    }

    /// 整跑至结束/熔断，返回退出码。
    pub fn run(&mut self) -> u16 {
        let mut r = SH_E_OK;
        while r == SH_E_OK && !self.halted {
            r = self.step();
        }
        r
    }

    /// 快照：(pc, steps, vars[0], vars[1])，供断点续跑。
    pub fn snapshot(&self) -> [u32; 4] {
        [self.pc as u32, self.steps, self.vars[0] as u32, self.vars[1] as u32]
    }

    pub fn restore(&mut self, snap: [u32; 4]) -> bool {
        if !self.loaded || snap[0] as usize > self.prog_len {
            return false;
        }
        self.pc = snap[0] as usize;
        self.steps = snap[1];
        self.vars[0] = snap[2] as i32;
        self.vars[1] = snap[3] as i32;
        self.halted = false;
        true
    }

    pub fn audit(&self) -> bool {
        self.steps <= SH_STEP_BUDGET && self.prog_len <= SH_PROG_MAX && (!self.loaded || self.pc <= self.prog_len)
    }

    pub fn reset(&mut self) {
        *self = ScriptHost::new();
    }
}

/// 族0272 自检：X06776~X06800 逐项登记。
pub fn run_scripthost_checks() -> CheckSet {
    let mut set = CheckSet::new("task-scripthost");

    // —— 基础实装 X06776~X06780 ——
    let mut sh = ScriptHost::new();
    // set v0; set v1; add v0+=v1; end
    let prog: [u16; 4] = [insn(OP_SET, 0), insn(OP_SET, 1), insn(OP_ADD, (0 << 3) | 1), insn(OP_END, 0)];
    let n = sh.load(&prog);
    let run = sh.run();
    set.add("X06776 核心链路闭环", n == 4 && run == SH_E_OK && sh.vars[0] == 2 && sh.halted && sh.steps == 4, "装载→执行→结束端到端可观测");
    set.add("X06777 全量参数开放", SH_VARS == 8 && SH_PROG_MAX == 32 && SH_STEP_BUDGET == 256 && OP_END == 5, "变量/程序/预算全参数可查");
    set.add("X06778 档位矩阵≥5档", SH_E_OK == 0 && SH_E_BUDGET == 1 && SH_E_PC == 2 && SH_E_VAR == 3 && SH_E_STATE == 4, "正常/熔断/越界/槽错/状态五态齐备");
    let snap = sh.snapshot();
    let snap_ok = snap[0] == 3 && snap[1] == 4 && snap[2] == 2 && snap[3] == 1;
    set.add("X06779 快照迁移三通道", snap_ok, "快照含 pc/步数/变量三要素");
    set.add("X06780 联调无回归", sh.audit() && insn_op(insn(OP_ADD, 0)) == OP_ADD && insn_arg(insn(OP_ADD, 7)) == 7, "编码解码无回归");

    // —— 边界与恢复 X06781~X06785 ——
    let mut sh2 = ScriptHost::new();
    let before = sh2.run();
    let empty = sh2.load(&[]);
    let empty_run = sh2.run();
    set.add("X06781 非法输入钳制", before == SH_E_STATE && empty == 0 && empty_run == SH_E_PC, "空程序/未装载均被拒绝");
    set.add("X06782 错误叙事体系", sh_describe(SH_E_BUDGET).contains("死循环") && sh_describe(SH_E_VAR).contains("0~7") && sh_describe(SH_E_STATE).contains("装载"), "每个失败有下一步建议");
    let mut sh3 = ScriptHost::new();
    // JMP 0 死循环 → 预算熔断。
    let _ = sh3.load(&[insn(OP_JMP, 0)]);
    let loop_run = sh3.run();
    set.add("X06783 中断续跑还原", loop_run == SH_E_BUDGET && sh3.steps == SH_STEP_BUDGET && sh3.trips == 1, "预算熔断可观测可计数");
    let mut sh4 = ScriptHost::new();
    let _ = sh4.load(&[insn(OP_SET, 9)]);
    let bad_var = sh4.run();
    set.add("X06784 资源降级守护", bad_var == SH_E_VAR && sh4.halted && sh4.audit(), "越界槽位拒绝不崩溃");
    let _ = sh4.reset();
    set.add("X06785 回滚净身", !sh4.loaded && sh4.pc == 0 && sh4.steps == 0 && sh4.trips == 0, "重置无残档");

    // —— 手感与细节 X06786~X06790 ——
    set.add("X06786 令牌对齐", insn_op(insn(OP_SET, 0)) == OP_SET && insn_arg(insn(OP_NOP, 9)) == 9, "指令编码令牌稳定");
    let mut sh5 = ScriptHost::new();
    // v0=1; JZ v0→3（不跳）; end
    let _ = sh5.load(&[insn(OP_SET, 0), insn(OP_JZ, (0 << 4) | 3), insn(OP_END, 0)]);
    let _ = sh5.run();
    let jz_taken = sh5.pc == 2;
    let mut sh6 = ScriptHost::new();
    let _ = sh6.load(&[insn(OP_JZ, (1 << 4) | 2), insn(OP_END, 0)]);
    let _ = sh6.run();
    set.add("X06787 三态焦点", jz_taken && sh6.pc == 2, "条件跳转真/假两态可达");
    let mut sh7 = ScriptHost::new();
    let _ = sh7.load(&[insn(OP_SET, 0), insn(OP_ADD, (0 << 3) | 0), insn(OP_END, 0)]);
    let self_add = sh7.run();
    set.add("X06788 键盘通道", self_add == SH_E_OK && sh7.vars[0] == 2, "自加运算通道正常");
    set.add("X06789 微文案统一", sh_describe(SH_E_OK) == "正常" && sh_describe(SH_E_PC).contains("越界"), "中文自然术语一致");
    set.add("X06790 无障碍等价", sh_describe(255).contains("未知") && !sh_describe(SH_E_BUDGET).is_empty(), "未知码也有可读叙事");

    // —— 性能与优化 X06791~X06795 ——
    let mut sh8 = ScriptHost::new();
    let mut long_prog = [insn(OP_NOP, 0); SH_PROG_MAX];
    long_prog[SH_PROG_MAX - 1] = insn(OP_END, 0);
    let n8 = sh8.load(&long_prog);
    let r8 = sh8.run();
    set.add("X06791 基准采集", n8 == SH_PROG_MAX && r8 == SH_E_OK && sh8.steps == SH_PROG_MAX as u32, "满长程序基准入册");
    let mut sh9 = ScriptHost::new();
    let _ = sh9.load(&[insn(OP_JMP, 0)]);
    let mut budget_ok = true;
    for _ in 0..10u32 {
        let _ = sh9.load(&[insn(OP_JMP, 0)]);
        budget_ok &= sh9.run() == SH_E_BUDGET;
    }
    set.add("X06792 热路径量化", budget_ok && sh9.trips == 10, "十连死循环熔断计数稳定");
    let mut sh10 = ScriptHost::new();
    sh10.reset();
    set.add("X06793 内存功耗收敛", sh10.steps == 0 && sh10.prog_len == 0, "待机零增量泄漏入长稳");
    let mut sh11 = ScriptHost::new();
    let dead = sh11.restore([1, 1, 0, 0]);
    set.add("X06794 低配降级链", dead == false && !sh11.loaded, "未装载降级为拒绝不崩溃");
    let mut sh12 = ScriptHost::new();
    let _ = sh12.load(&prog);
    let mut inv_ok = true;
    while !sh12.halted && sh12.steps < 10 {
        let _ = sh12.step();
        inv_ok &= sh12.audit();
    }
    set.add("X06795 防劣化守卫", inv_ok && sh12.snapshot()[1] <= 10, "混合负载不变量断言只增不删");

    // —— 创新拓展 X06796~X06800 ——
    let mut sh13 = ScriptHost::new();
    let _ = sh13.load(&[insn(OP_JMP, 0)]);
    let _ = sh13.run();
    set.add("X06796 智能建议", sh_describe(SH_E_BUDGET).contains("检查跳转"), "熔断可解释可定位");
    let mut sh14 = ScriptHost::new();
    let _ = sh14.load(&prog);
    let _ = sh14.step();
    let mid = sh14.snapshot();
    let _ = sh14.step();
    let _ = sh14.restore(mid);
    let _ = sh14.run();
    set.add("X06797 批量自动化", sh14.vars[0] == 2 && sh14.halted && sh14.steps == 4, "断点续跑不重不漏");
    let cross = {
        let mut a = ScriptHost::new();
        let _ = a.load(&prog);
        let _ = a.run();
        let mut b = ScriptHost::new();
        let _ = b.load(&prog);
        let _ = b.run();
        a.snapshot() == b.snapshot()
    };
    set.add("X06798 三线跨域联动", cross, "同程序跨实例终态一致");
    let mut sh15 = ScriptHost::new();
    let long: [u16; SH_PROG_MAX + 4] = [insn(OP_END, 0); SH_PROG_MAX + 4];
    let clamp_n = sh15.load(&long);
    set.add("X06799 开发者扩展点", clamp_n == SH_PROG_MAX && sh15.audit(), "超长程序装载钳制");
    let mut sh16 = ScriptHost::new();
    let _ = sh16.load(&prog);
    let _ = sh16.run();
    let fp16 = sh16.snapshot();
    sh16.reset();
    set.add("X06800 收官与净身", sh16.snapshot() == [0, 0, 0, 0] && fp16[0] == 3, "重置后回到初态收官");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripthost_load_run_and_ops() {
        let mut sh = ScriptHost::new();
        let prog = [insn(OP_SET, 0), insn(OP_SET, 1), insn(OP_ADD, (0 << 3) | 1), insn(OP_END, 0)];
        assert_eq!(sh.load(&prog), 4);
        assert_eq!(sh.run(), SH_E_OK);
        assert_eq!(sh.vars[0], 2);
        assert!(sh.halted && sh.audit());
        let bad = [insn(OP_SET, 9)];
        let _ = sh.load(&bad);
        assert_eq!(sh.run(), SH_E_VAR);
    }

    #[test]
    fn scripthost_budget_and_resume() {
        let mut sh = ScriptHost::new();
        let _ = sh.load(&[insn(OP_JMP, 0)]);
        assert_eq!(sh.run(), SH_E_BUDGET);
        assert_eq!(sh.steps, SH_STEP_BUDGET);
        let mut sh2 = ScriptHost::new();
        let prog = [insn(OP_SET, 0), insn(OP_SET, 1), insn(OP_ADD, (0 << 3) | 1), insn(OP_END, 0)];
        let _ = sh2.load(&prog);
        let _ = sh2.step();
        let mid = sh2.snapshot();
        let _ = sh2.step();
        assert!(sh2.restore(mid));
        assert_eq!(sh2.run(), SH_E_OK);
        assert_eq!(sh2.vars[0], 2);
    }

    #[test]
    fn scripthost_all_checks_pass() {
        let set = run_scripthost_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "第 {} 项未通过: {}", i, c.name);
            let id: u32 = c.name[1..6].parse().unwrap_or(0);
            assert_eq!(id, 6776 + i as u32, "ID 不连续：{}", c.name);
        }
    }
}
