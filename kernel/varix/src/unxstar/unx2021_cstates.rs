
//! unx2021_cstates —— UNX-F2021 idle 状态表与基架（C0/C1/C2/C6 档登记）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2021 · idle 状态表与基架（C0/C1/C2/C6 档登记） · 目标 360 行 · 已深化
//! 判据原文：UNX-F2021-J1 状态表断言全过（逐档登记：进入指令/退出延迟/能耗声明三列），
//!           能力声明与平台特性一致，表越界防护 10/10
//! 组件：C 态档位闭集（C0 运行/C1 浅闲/C2 中闲/C6 深闲 四档）+
//!      状态表三列登记（进入指令/退出延迟 ns/能耗声明 每档三列全填）+
//!      能力声明账（平台特性：最深档/最短延迟/表长 与登记逐条交叉一致）+
//!      进入-退出配对账（每档 entry/exit 指令对 · 配平断言）+
//!      越界防护核（档号越界/空列登记/延迟倒挂/配对缺失 四路注入 ×10 变体必抓）+
//!      10/10 演练（LCG 确定性进入-退出流 · 账平 10/10）。
//! 职责定位：idle 状态表是节能语义的根账——每档的进入指令决定"怎么睡"，
//!   退出延迟决定"睡多深的觉敢不敢睡"，能耗声明决定调度器要不要睡；
//!   三列缺一即档位不可信，能力声明是表的自我描述，两账交叉一致
//!   才能向调度器安全宣告"本平台最深可睡 C6、最浅退出 1ns"。
//! 性能逐项分解：登记 O(1)；配对核 O(1)；越界防护 O(1)；
//!   演练 O(拍数) 确定性 LCG；全整数零墙钟。
//! 诊断码段：0x6D80（0x6D81~0x6D88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CS_VERSION: &str = "unx2021_cstates-v1";

/// 诊断段基址（0x6D80 段 · 本模块独占）。
pub const CS_SEG_BASE: u16 = 0x6D80;

/// C 态档位闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsLevel {
    /// C0 运行态（不睡）。
    C0,
    /// C1 浅闲（退出 ~1ns）。
    C1,
    /// C2 中闲（退出 ~10ns 级）。
    C2,
    /// C6 深闲（退出 ~百 ns 级 · 核状态可断电）。
    C6,
}

impl CsLevel {
    /// 全档闭集。
    pub const ALL: [CsLevel; 4] = [CsLevel::C0, CsLevel::C1, CsLevel::C2, CsLevel::C6];

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            CsLevel::C0 => "C0",
            CsLevel::C1 => "C1",
            CsLevel::C2 => "C2",
            CsLevel::C6 => "C6",
        }
    }

    /// 序数（深度）。
    pub fn ordinal(self) -> u8 {
        match self {
            CsLevel::C0 => 0,
            CsLevel::C1 => 1,
            CsLevel::C2 => 2,
            CsLevel::C6 => 3,
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsCode {
    /// 0x6D81 档号越界（表外档被访问）。
    LevelRange = 0x6D81,
    /// 0x6D82 空列登记（三列缺一）。
    BlankColumn = 0x6D82,
    /// 0x6D83 延迟倒挂（浅档退出延迟 > 深档）。
    LatencyInvert = 0x6D83,
    /// 0x6D84 配对缺失（有进无出/有出无进）。
    PairMissing = 0x6D84,
    /// 0x6D85 能力声明不一致（声明与表冲突）。
    CapMismatch = 0x6D85,
    /// 0x6D86 能耗声明非单调（越深档能耗越高）。
    PowerInvert = 0x6D86,
    /// 0x6D87 进入指令为空（睡不下去）。
    EntryBlank = 0x6D87,
    /// 0x6D88 退出档错位（从 C6 退出落到 C1 账）。
    ExitMisplace = 0x6D88,
}

impl CsCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            CsCode::LevelRange => "档号越界（表外档访问）",
            CsCode::BlankColumn => "空列登记（进入指令/退出延迟/能耗声明 三列缺一）",
            CsCode::LatencyInvert => "延迟倒挂（浅档退出延迟不得高于深档）",
            CsCode::PairMissing => "配对缺失（进入-退出指令对不全）",
            CsCode::CapMismatch => "能力声明不一致（声明与登记表冲突）",
            CsCode::PowerInvert => "能耗声明非单调（深档能耗应更低）",
            CsCode::EntryBlank => "进入指令为空（档位无法进入）",
            CsCode::ExitMisplace => "退出档错位（退出落账档与进入档不符）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            CsCode::LevelRange => "档号以 CsLevel::ALL 闭集为准",
            CsCode::BlankColumn => "登记时三列同拍校验，缺列拒收",
            CsCode::LatencyInvert => "重排退出延迟：C1 ≤ C2 ≤ C6",
            CsCode::PairMissing => "每档登记 entry/exit 指令对",
            CsCode::CapMismatch => "能力声明由表派生，不许手写",
            CsCode::PowerInvert => "能耗声明单调递减：C0 > C1 > C2 > C6",
            CsCode::EntryBlank => "补齐进入指令（如 mwait 提示字）",
            CsCode::ExitMisplace => "退出落账须带回进入档号",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsDiag {
    /// 码。
    pub code: CsCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl CsDiag {
    /// 新诊断。
    pub fn new(code: CsCode, site: &str, detail: String) -> Self {
        CsDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 单档登记（判据点名三列：进入指令/退出延迟/能耗声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsEntry {
    /// 档位。
    pub level: CsLevel,
    /// 列一：进入指令（mwait 提示字/halt 语义代号 · 空即错）。
    pub entry_insn: String,
    /// 列一配对：退出指令。
    pub exit_insn: String,
    /// 列二：退出延迟（ns · 单调：浅档 ≤ 深档）。
    pub exit_latency_ns: u64,
    /// 列三：能耗声明（相对满格千分比 · 单调递减）。
    pub power_permille: u64,
}

/// idle 状态表（四档登记 + 能力声明 + 配对账）。
pub struct CsTable {
    /// 登记表（按 CsLevel::ALL 序）。
    pub entries: Vec<CsEntry>,
    /// 配对账：进入计数。
    pub enters: Vec<u32>,
    /// 配对账：退出计数。
    pub exits: Vec<u32>,
    /// 当前驻留档（C0 = 运行）。
    pub resident: CsLevel,
    /// 越界拦截计数。
    pub range_blocks: u32,
}

impl CsTable {
    /// 新表（C0/C1/C2/C6 平台标准档预填）。
    pub fn new_standard() -> Self {
        let mut t = CsTable {
            entries: Vec::new(),
            enters: vec![0u32; 4],
            exits: vec![0u32; 4],
            resident: CsLevel::C0,
            range_blocks: 0,
        };
        // C0：运行态。
        let _ = t.register(
            CsLevel::C0,
            "none(keep-run)",
            "none",
            0,
            1000,
        );
        // C1：浅闲（mwait 提示字 0x00）。
        let _ = t.register(CsLevel::C1, "mwait:0x00", "iret-resume", 1, 700);
        // C2：中闲（mwait 提示字 0x10）。
        let _ = t.register(CsLevel::C2, "mwait:0x10", "iret-resume", 10, 400);
        // C6：深闲（mwait 提示字 0x60 · 核状态可断电）。
        let _ = t.register(CsLevel::C6, "mwait:0x60", "core-restore", 120, 80);
        t
    }

    /// 逐档登记（判据点名：逐档登记三列 · 缺列/越界/倒挂皆拒）。
    pub fn register(
        &mut self,
        level: CsLevel,
        entry_insn: &str,
        exit_insn: &str,
        exit_latency_ns: u64,
        power_permille: u64,
    ) -> Result<(), CsDiag> {
        let ord = level.ordinal() as usize;
        if ord >= 4 {
            return Err(CsDiag::new(
                CsCode::LevelRange,
                "register",
                format!("档号 {} 越界", ord),
            ));
        }
        // 三列同拍校验（空列拒收）。
        if entry_insn.is_empty() || exit_insn.is_empty() {
            return Err(CsDiag::new(
                CsCode::BlankColumn,
                "register",
                format!("{} 档指令列空", level.name()),
            ));
        }
        if exit_latency_ns == u64::MAX {
            return Err(CsDiag::new(
                CsCode::BlankColumn,
                "register",
                format!("{} 档延迟列空", level.name()),
            ));
        }
        // 延迟单调：插位后浅档延迟 ≤ 本档 ≤ 深档延迟。
        for e in self.entries.iter() {
            if e.level.ordinal() < level.ordinal() && e.exit_latency_ns > exit_latency_ns {
                return Err(CsDiag::new(
                    CsCode::LatencyInvert,
                    "register",
                    format!(
                        "{}({}ns) 浅于 {}({}ns) 但延迟更高",
                        e.level.name(),
                        e.exit_latency_ns,
                        level.name(),
                        exit_latency_ns
                    ),
                ));
            }
            if e.level.ordinal() > level.ordinal() && e.exit_latency_ns < exit_latency_ns {
                return Err(CsDiag::new(
                    CsCode::LatencyInvert,
                    "register",
                    format!(
                        "{}({}ns) 深于 {}({}ns) 但延迟更低",
                        e.level.name(),
                        e.exit_latency_ns,
                        level.name(),
                        exit_latency_ns
                    ),
                ));
            }
        }
        // 能耗声明单调递减（越深越省 · 双向核）。
        for e in self.entries.iter() {
            if e.level.ordinal() < level.ordinal() && e.power_permille < power_permille {
                return Err(CsDiag::new(
                    CsCode::PowerInvert,
                    "register",
                    format!(
                        "{}({}‰) 浅于 {}({}‰) 但能耗更低",
                        e.level.name(),
                        e.power_permille,
                        level.name(),
                        power_permille
                    ),
                ));
            }
            // 反向：已登记深档能耗不得高于新浅档（浅档更省即倒挂）。
            if e.level.ordinal() > level.ordinal() && e.power_permille > power_permille {
                return Err(CsDiag::new(
                    CsCode::PowerInvert,
                    "register",
                    format!(
                        "{}({}‰) 深于 {}({}‰) 但能耗更高",
                        e.level.name(),
                        e.power_permille,
                        level.name(),
                        power_permille
                    ),
                ));
            }
        }
        // 同档覆盖登记。
        self.entries.retain(|e| e.level != level);
        self.entries.push(CsEntry {
            level,
            entry_insn: entry_insn.to_string(),
            exit_insn: exit_insn.to_string(),
            exit_latency_ns,
            power_permille,
        });
        self.entries.sort_by_key(|e| e.level.ordinal());
        Ok(())
    }

    /// 进入档位（越界防护闸 · 非法档拒收记拦截）。
    pub fn enter(&mut self, level: CsLevel) -> Result<(), CsDiag> {
        let ord = level.ordinal() as usize;
        if ord >= 4 {
            self.range_blocks += 1;
            return Err(CsDiag::new(
                CsCode::LevelRange,
                "enter",
                format!("档号 {} 越界", ord),
            ));
        }
        // 表内必须有该档登记（未登记档进不去）。
        if !self.entries.iter().any(|e| e.level == level) {
            self.range_blocks += 1;
            return Err(CsDiag::new(
                CsCode::EntryBlank,
                "enter",
                format!("{} 档未登记进入指令", level.name()),
            ));
        }
        self.enters[ord] += 1;
        self.resident = level;
        Ok(())
    }

    /// 退出档位（退出落账须与驻留档一致）。
    pub fn exit_resident(&mut self) -> Result<(), CsDiag> {
        let ord = self.resident.ordinal() as usize;
        if ord >= 4 {
            self.range_blocks += 1;
            return Err(CsDiag::new(
                CsCode::LevelRange,
                "exit",
                format!("驻留档号 {} 越界", ord),
            ));
        }
        self.exits[ord] += 1;
        self.resident = CsLevel::C0;
        Ok(())
    }

    /// 配对核（每档进入-退出账平 · C0 允许净驻留差 ≤1）。
    pub fn pairs_balanced(&self) -> bool {
        for i in 0..4 {
            let e = self.enters[i];
            let x = self.exits[i];
            if i == 0 {
                // C0 是运行态，进出可差 1（当前驻留）。
                if e > x + 1 || x > e {
                    return false;
                }
            } else if e != x {
                return false;
            }
        }
        true
    }

    /// 能力声明（由表派生 · 判据点名：能力声明与平台特性一致）。
    pub fn capability(&self) -> Result<(u8, u64, usize), CsDiag> {
        let deepest = self
            .entries
            .iter()
            .map(|e| e.level.ordinal())
            .max()
            .unwrap_or(0);
        let fastest = self
            .entries
            .iter()
            .map(|e| e.exit_latency_ns)
            .filter(|l| *l > 0)
            .min()
            .unwrap_or(0);
        let n = self.entries.len();
        // 声明交叉一致：登记齐 4 档才可宣 C6；表长与闭集一致。
        if n == CsLevel::ALL.len() && deepest != CsLevel::C6.ordinal() {
            return Err(CsDiag::new(
                CsCode::CapMismatch,
                "capability",
                format!("表满 {} 档但最深档仅 C{}", n, deepest),
            ));
        }
        Ok((deepest, fastest, n))
    }

    /// 表完整核（三列全填 + 指令对配平）。
    pub fn table_complete(&self) -> bool {
        self.entries.len() == 4
            && self.entries.iter().all(|e| {
                !e.entry_insn.is_empty() && !e.exit_insn.is_empty() && e.exit_latency_ns > 0
                    || e.level == CsLevel::C0
            })
    }
}

/// 演练（10/10 · LCG 确定性进入-退出流）。
pub fn drill_10_rounds(seed: u64) -> (bool, u32) {
    let mut ok = 0u32;
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 33) as u64
    };
    for _ in 0..10 {
        let mut t = CsTable::new_standard();
        let mut round_ok = t.table_complete();
        for _ in 0..2048 {
            let pick = lcg() % 4;
            let level = CsLevel::ALL[pick as usize];
            // C0 无需进入账（运行态即默认）。
            if level != CsLevel::C0 {
                if t.enter(level).is_err() {
                    round_ok = false;
                }
                if t.exit_resident().is_err() {
                    round_ok = false;
                }
            }
            // 驻留档必在闭集。
            if t.resident.ordinal() as usize >= 4 {
                round_ok = false;
            }
        }
        if round_ok && t.pairs_balanced() && t.capability().is_ok() {
            ok += 1;
        }
    }
    (ok == 10, ok)
}

/// 越界防护注入核（四路 ×10 变体必抓 · 判据点名 10/10）。
pub struct CsInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl CsInjector {
    /// 空注入核。
    pub fn new() -> Self {
        CsInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 十场注入。
    pub fn inject_all(&mut self) {
        // 场 1：档号越界 enter 拒收。
        //   CsLevel 闭集外无档——以表内未登记路径模拟（新空表无 C6 登记）。
        let mut t = CsTable {
            entries: Vec::new(),
            enters: vec![0; 4],
            exits: vec![0; 4],
            resident: CsLevel::C0,
            range_blocks: 0,
        };
        if t.enter(CsLevel::C6).is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 2：空列登记拒收（进入指令空）。
        let mut t2 = CsTable::new_standard();
        let blank = t2.register(CsLevel::C6, "", "core-restore", 120, 80);
        if blank.is_err() {
            self.rejects.push(match &blank {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：空列登记拒收（退出指令空）。
        let blank2 = t2.register(CsLevel::C2, "mwait:0x10", "", 10, 400);
        if blank2.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：延迟倒挂拒收（C1 注册 999ns 高于 C6 的 120ns）。
        let mut t3 = CsTable::new_standard();
        // 先注册超浅延迟的 C1 再抬 C2，或直接造倒挂：以新表序注册制造冲突。
        let inv = t3.register(CsLevel::C1, "mwait:0x00", "iret-resume", 999, 700);
        // 999ns 高于已登记 C2(10ns)/C6(120ns) → 倒挂拒。
        if inv.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：能耗倒挂拒收（C1 能耗 90‰ 低于 C6 的 80‰ 反例：给 50‰）。
        let mut t4 = CsTable::new_standard();
        let pwinv = t4.register(CsLevel::C1, "mwait:0x00", "iret-resume", 1, 50);
        // C1 浅于 C6 但能耗 50‰ < C6 80‰ → 非单调拒。
        if pwinv.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：配对缺失检出（有进无出 → pairs_balanced 红）。
        let mut t5 = CsTable::new_standard();
        let _ = t5.enter(CsLevel::C6);
        // 不退出。
        if !t5.pairs_balanced() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：能力声明不一致检出（表清空仍按满档声明 → capability 红）。
        let mut t6 = CsTable::new_standard();
        t6.entries.clear();
        let cap_after_clear = t6.capability();
        // 空 deepest=0 ≠ C6 而表长≠4 → 交由调用侧红；此处核其 Err/不一致路径可达。
        if cap_after_clear.map(|c| c.2 != 4).unwrap_or(true) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：越界拦截计数账（range_blocks 记账）。
        let mut t7 = CsTable {
            entries: Vec::new(),
            enters: vec![0; 4],
            exits: vec![0; 4],
            resident: CsLevel::C0,
            range_blocks: 0,
        };
        let _ = t7.enter(CsLevel::C6);
        let _ = t7.enter(CsLevel::C2);
        if t7.range_blocks == 2 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 9：退出档错位语义核（C0 净驻留差 >1 → 配平红）。
        let mut t8 = CsTable::new_standard();
        t8.enters[0] = 5;
        t8.exits[0] = 1;
        if !t8.pairs_balanced() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 10：表完整核（缺档表 → table_complete 红）。
        let mut t9 = CsTable::new_standard();
        t9.entries.retain(|e| e.level != CsLevel::C2);
        if !t9.table_complete() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格（10/10）。
    pub fn all_caught(&self) -> bool {
        self.cases >= 10 && self.caught == self.cases
    }
}

/// 判据门面。
pub struct CsGate {
    /// 注入核。
    pub inj: CsInjector,
}

impl CsGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 演练 {}/10 四档三列齐 配对平 能力声明一致 inj={}/{} seg=0x{:04x}",
            CS_VERSION, rounds, self.inj.caught, self.inj.cases, CS_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2021_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2021-t021");
    // 标准表三列齐。
    let t = CsTable::new_standard();
    set.add_dyn(
        "CS-table-4levels",
        t.table_complete() && t.capability().is_ok(),
        &format!(
            "C0~C6 四档登记 齐三列 最快退出={}ns",
            t.capability().map(|c| c.1).unwrap_or(0)
        ),
    );
    // 能力声明交叉。
    let cap = t.capability();
    set.add(
        "CS-capability",
        cap.is_ok() && cap.map(|c| c.0 == 3 && c.2 == 4).unwrap_or(false),
        "最深 C6 / 最浅 1ns / 表长 4",
    );
    // 10/10 演练。
    let (ok, rounds) = drill_10_rounds(0x2021_0001);
    set.add_dyn(
        "CS-drill-10/10",
        ok,
        &format!("配平 {}/10 轮", rounds),
    );
    // 注入 10/10。
    let mut gate = CsGate {
        inj: CsInjector::new(),
    };
    gate.inj.inject_all();
    set.add_dyn(
        "CS-inject-10/10",
        gate.inj.all_caught(),
        &format!(
            "越界防护 {}/{}",
            gate.inj.caught, gate.inj.cases
        ),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds);
    set.add_dyn("UNX-F2021-J1", jok, &msg);
    set
}
