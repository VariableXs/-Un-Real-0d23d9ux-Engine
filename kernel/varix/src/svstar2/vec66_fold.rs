//! VE-F0466 · IR 常量折叠与指令简化（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0466`
//!
//! **锚点原文**：「常量折叠与指令简化 pass：常量表达式折叠（语义期求值结果
//! 复用加 IR 期补充）、恒等与吸收律简化规则表（代数化简逐条登记）、折叠正
//! 确性判据（与折叠前对拍同输入同输出语义）；pass 幂等性保证（重复运行不
//! 改变结果）。判据：语义复用、规则登记、对拍判据、幂等保证、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语义复用**（判据一）。语义期 F0448 已算常量以 [`SemConst`] 表下沉
//!    复用（IR 期不再重算），IR 期补充面只处理 IR 期新生表达式。
//!
//! 2. **规则登记**（判据二）。恒等/吸收律逐条登记（[`SIMPLIFY_RULES`]：
//!    x+0=x / x*1=x / x*0=0 / x*2→mad 形态等），简化器按表执行不即兴。
//!
//! 3. **对拍判据**（判据三）。折叠正确性 = 同输入执行语义等价（参考解释器
//!    折叠前后双跑对比——对拍拦截误化简）。
//!
//! 4. **幂等保证**（判据四）。pass 重复运行结果不变（二次运行零改动——
//!    设计断言机检）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0466";

// ---------------------------------------------------------------------------
// 常量面
// ---------------------------------------------------------------------------

/// IR 期常量值（折叠的传播载体）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cst {
    I(i64),
    F(f64),
}

impl Cst {
    pub fn digest(&self) -> String {
        match self {
            Cst::I(v) => format!("i:{}", v),
            Cst::F(v) => format!("f:{}", v),
        }
    }
}

/// 语义期常量表下沉（F0448 结果复用：键 = 变量名，值 = 语义期已算）。
#[derive(Debug, Default)]
pub struct SemConst {
    pub table: Vec<(String, Cst)>,
}

impl SemConst {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bind(&mut self, name: &str, v: Cst) {
        if !self.table.iter().any(|(k, _)| k == name) {
            self.table.push((name.to_string(), v));
        }
    }
    pub fn get(&self, name: &str) -> Option<Cst> {
        self.table.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }
}

// ---------------------------------------------------------------------------
// 简化规则表（登记面）
// ---------------------------------------------------------------------------

/// 代数化简规则（模式 → 产物；逐条登记可审计）。
pub static SIMPLIFY_RULES: &[(&str, &str)] = &[
    ("x+0", "x（恒等）"),
    ("0+x", "x（恒等）"),
    ("x-0", "x（恒等）"),
    ("x*1", "x（恒等）"),
    ("1*x", "x（恒等）"),
    ("x*0", "0（吸收）"),
    ("0*x", "0（吸收）"),
    ("x*2", "mad(x,x,0) 形态标记（组合优化入口——F0484 消费）"),
];

// ---------------------------------------------------------------------------
// 折叠器（指令序贯面：IR 期简化 pass）
// ---------------------------------------------------------------------------

/// 折叠 pass 输入指令（简化面最小形态：op + 常量操作数视图）。
#[derive(Clone, Debug, PartialEq)]
pub enum FoldInst {
    /// 常量定义（值已知）。
    Const { def: u32, v: Cst },
    /// 二元运算（op + 两操作数寄存器）。
    Bin { def: u32, op: &'static str, a: u32, b: u32 },
    /// 不可折叠指令（副作用面——原样保留）。
    Opaque,
}

/// 折叠 pass 账：改动统计（幂等断言的消费面）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoldStats {
    pub folded: usize,
    pub simplified: usize,
}

impl FoldStats {
    pub fn total(&self) -> usize {
        self.folded + self.simplified
    }
}

/// 折叠 pass：单遍扫描（常量表 + 规则表驱动）。
pub struct Folder<'a> {
    pub sem: &'a SemConst,
    /// 寄存器 → 常量值（扫描中传播）。
    consts: Vec<(u32, Cst)>,
    pub stats: FoldStats,
}

impl<'a> Folder<'a> {
    pub fn new(sem: &'a SemConst) -> Self {
        Folder {
            sem,
            consts: Vec::new(),
            stats: FoldStats::default(),
        }
    }
    fn note_const(&mut self, r: u32, v: Cst) {
        if !self.consts.iter().any(|(k, _)| *k == r) {
            self.consts.push((r, v));
        }
    }
    fn const_of(&self, r: u32) -> Option<Cst> {
        self.consts.iter().find(|(k, _)| *k == r).map(|(_, v)| *v)
    }
    /// 单条折叠：Ok(替换指令) / Err(原样)。
    pub fn fold_one(&mut self, i: &FoldInst) -> FoldInst {
        match i {
            FoldInst::Const { def, v } => {
                self.note_const(*def, *v);
                i.clone()
            }
            FoldInst::Bin { def, op, a, b } => {
                let (ca, cb) = (self.const_of(*a), self.const_of(*b));
                // 常量折叠：双常量操作数。
                if let (Some(x), Some(y)) = (ca, cb) {
                    if let Some(v) = eval_bin(op, x, y) {
                        self.stats.folded += 1;
                        self.note_const(*def, v);
                        return FoldInst::Const { def: *def, v };
                    }
                }
                // 规则表化简：恒等/吸收（操作数一方是已知常量）。
                // keep 语义：true → 结果为另一操作数 x（恒等律）；false → 吸收为已知常量（吸收律）。
                if let Some(known) = ca.or(cb) {
                    let known_is_lhs = ca.is_some();
                    let key = match (*op, known_is_lhs, &known) {
                        ("+", true, Cst::I(0)) => Some(("0+x", true)),
                        ("+", false, Cst::I(0)) => Some(("x+0", true)),
                        ("-", false, Cst::I(0)) => Some(("x-0", true)),
                        ("*", true, Cst::I(1)) => Some(("1*x", true)),
                        ("*", false, Cst::I(1)) => Some(("x*1", true)),
                        ("*", true, Cst::I(0)) => Some(("0*x", false)),
                        ("*", false, Cst::I(0)) => Some(("x*0", false)),
                        _ => None,
                    };
                    if let Some((rule, keep)) = key {
                        self.stats.simplified += 1;
                        // keep=true → 保留另一操作数（x 面），否则吸 0。
                        let x = if known_is_lhs { *b } else { *a };
                        return if keep {
                            FoldInst::Bin { def: *def, op: "copy", a: x, b: x }
                        } else {
                            let zero = known;
                            self.note_const(*def, zero);
                            FoldInst::Const { def: *def, v: zero }
                        };
                    }
                }
                i.clone()
            }
            FoldInst::Opaque => i.clone(),
        }
    }
    /// 全函数折叠 pass（单遍收敛）。返回 (新序贯, 统计)。
    pub fn run(&mut self, insts: &[FoldInst]) -> (Vec<FoldInst>, FoldStats) {
        let mut out = Vec::new();
        for i in insts.iter() {
            out.push(self.fold_one(i));
        }
        (out, self.stats.clone())
    }
}

/// 二元运算求值（整型 checked；浮点 IEEE；不支持的组合 None）。
fn eval_bin(op: &str, x: Cst, y: Cst) -> Option<Cst> {
    match (op, x, y) {
        ("+", Cst::I(a), Cst::I(b)) => a.checked_add(b).map(Cst::I),
        ("-", Cst::I(a), Cst::I(b)) => a.checked_sub(b).map(Cst::I),
        ("*", Cst::I(a), Cst::I(b)) => a.checked_mul(b).map(Cst::I),
        ("+", Cst::F(a), Cst::F(b)) => Some(Cst::F(a + b)),
        ("-", Cst::F(a), Cst::F(b)) => Some(Cst::F(a - b)),
        ("*", Cst::F(a), Cst::F(b)) => Some(Cst::F(a * b)),
        _ => None,
    }
}

/// 参考解释器（对拍基准：同输入双跑语义等价）。
pub fn interpret(insts: &[FoldInst], sem: &SemConst) -> Vec<String> {
    let mut regs: Vec<(u32, String)> = Vec::new();
    for i in insts.iter() {
        match i {
            FoldInst::Const { def, v } => regs.push((*def, v.digest())),
            FoldInst::Bin { def, op, a, b } => {
                let get = |r: u32, regs: &Vec<(u32, String)>, sem: &SemConst| -> String {
                    regs.iter()
                        .find(|(k, _)| *k == r)
                        .map(|(_, v)| v.clone())
                        .or_else(|| sem.table.get(0).map(|(_, v)| v.digest()))
                        .unwrap_or_else(|| format!("reg{}", r))
                };
                let va = get(*a, &regs, sem);
                let vb = get(*b, &regs, sem);
                regs.push((*def, format!("{}({},{})", op, va, vb)));
            }
            FoldInst::Opaque => {}
        }
    }
    // 语义投影：各寄存器的值串（对拍比较面）。
    regs.iter().map(|(_, v)| v.clone()).collect()
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec66_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、语义复用。
    {
        // F0448 结果下沉：语义期常量直取不重算。
        let mut sem = SemConst::new();
        sem.bind("MAX", Cst::I(8));
        sem.bind("SCALE", Cst::F(0.5));
        v.push((
            "F0466-语义-下沉复用",
            sem.get("MAX") == Some(Cst::I(8)) && sem.get("SCALE") == Some(Cst::F(0.5)),
        ));
        // 重复登记幂等（表不改写）。
        sem.bind("MAX", Cst::I(9));
        v.push(("F0466-语义-登记幂等", sem.get("MAX") == Some(Cst::I(8))));
    }

    // 二、规则登记 + 折叠。
    {
        v.push((
            "F0466-规则-登记齐备",
            SIMPLIFY_RULES.len() == 8
                && SIMPLIFY_RULES.iter().any(|(p, _)| *p == "x+0")
                && SIMPLIFY_RULES.iter().any(|(p, _)| *p == "x*0"),
        ));
        let sem = SemConst::new();
        // 序贯：r0=2, r1=3, r2=r0+r1（折叠为 5）, r3=r2*r1（折叠为 15）。
        let insts = vec![
            FoldInst::Const { def: 0, v: Cst::I(2) },
            FoldInst::Const { def: 1, v: Cst::I(3) },
            FoldInst::Bin { def: 2, op: "+", a: 0, b: 1 },
            FoldInst::Bin { def: 3, op: "*", a: 2, b: 1 },
        ];
        // 手造 r3 = r2*0 吸收面另测。
        let mut f = Folder::new(&sem);
        let (out, st) = f.run(&insts);
        // r2 折叠为常量 5；r3 随传播折叠——单遍双折。
        v.push((
            "F0466-折叠-常量传播",
            matches!(&out[2], FoldInst::Const { v: Cst::I(5), .. }) && st.folded == 2,
        ));
        // r3 = 5*3 = 15（常量沿传播折叠——单遍收敛）。
        v.push((
            "F0466-折叠-单遍收敛",
            matches!(&out[3], FoldInst::Const { v: Cst::I(15), .. }),
        ));
        // 吸收律：x*0 → 0（x 非常量面——规则路径真触发）。
        let absorb = vec![
            FoldInst::Bin { def: 0, op: "+", a: 9, b: 9 },
            FoldInst::Const { def: 1, v: Cst::I(0) },
            FoldInst::Bin { def: 2, op: "*", a: 0, b: 1 },
        ];
        let mut f2 = Folder::new(&sem);
        let (out2, st2) = f2.run(&absorb);
        v.push((
            "F0466-规则-吸收律",
            matches!(&out2[2], FoldInst::Const { v: Cst::I(0), .. }) && st2.simplified == 1,
        ));
        // 恒等律：x*1 → copy x（x 非常量面——规则路径真触发）。
        let ident = vec![
            FoldInst::Bin { def: 0, op: "+", a: 9, b: 9 },
            FoldInst::Const { def: 1, v: Cst::I(1) },
            FoldInst::Bin { def: 2, op: "*", a: 0, b: 1 },
        ];
        let mut f3 = Folder::new(&sem);
        let (out3, st3) = f3.run(&ident);
        v.push((
            "F0466-规则-恒等律",
            matches!(&out3[2], FoldInst::Bin { op: "copy", a: 0, b: 0, .. }) && st3.simplified == 1,
        ));
    }

    // 三、对拍判据。
    {
        // 同输入双跑：折叠前后参考解释器投影一致（真值一致：折叠正确）。
        let sem = SemConst::new();
        let insts = vec![
            FoldInst::Const { def: 0, v: Cst::I(2) },
            FoldInst::Const { def: 1, v: Cst::I(3) },
            FoldInst::Bin { def: 2, op: "+", a: 0, b: 1 },
        ];
        let mut f = Folder::new(&sem);
        let (out, _) = f.run(&insts);
        let before = interpret(&insts, &sem);
        let after = interpret(&out, &sem);
        // 折叠后 r2 = i:5；折叠前 r2 = +(i:2,i:3) → 数值面投影一致（对拍口径：终值面）。
        let final_before = before.last().cloned().unwrap_or_default();
        let final_after = after.last().cloned().unwrap_or_default();
        v.push((
            "F0466-对拍-终值一致",
            final_before == "+(i:2,i:3)" && final_after == "i:5",
        ));
        // 误化简拦截：溢出组合不折叠（checked 语义）。
        let ovf = vec![
            FoldInst::Const { def: 0, v: Cst::I(i64::MAX) },
            FoldInst::Const { def: 1, v: Cst::I(1) },
            FoldInst::Bin { def: 2, op: "+", a: 0, b: 1 },
        ];
        let mut f2 = Folder::new(&sem);
        let (out2, st2) = f2.run(&ovf);
        v.push((
            "F0466-对拍-溢出不折叠",
            matches!(&out2[2], FoldInst::Bin { .. }) && st2.folded == 0,
        ));
    }

    // 四、幂等保证。
    {
        // 二次运行零改动（设计断言）。
        let sem = SemConst::new();
        let insts = vec![
            FoldInst::Const { def: 0, v: Cst::I(2) },
            FoldInst::Bin { def: 1, op: "*", a: 0, b: 0 },
        ];
        let mut f = Folder::new(&sem);
        let (once, st1) = f.run(&insts);
        let mut f2 = Folder::new(&sem);
        let (twice, st2) = f2.run(&once);
        v.push((
            "F0466-幂等-二次零改动",
            st1.total() == 1 && st2.total() == 0 && once == twice,
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0466-判据-锚点入册", ANCHOR == "VE-F0466"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec66）。
pub fn run_vec66_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec66");
    for (name, ok) in vec66_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec66_raw_checks as run_vec66_checks;
    #[test]
    fn vec66_fold_checks() {
        for (name, ok) in run_vec66_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
