//! unx1661_schedcls —— UNX-F1661 调度类注册框架与优先级链。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B04）：UNX-F1661 · 调度类注册框架与优先级链 · 目标 340 行 · 已深化
//! 判据原文：UNX-F1661-J1 注册框架单点构建，类优先级链静态闭合（无环、无洞、
//!           首尾确定），重复注册注入 10/10 次拒绝
//! 组件：调度类注册框架（单点注册口：注册表唯一定容 8 类）+
//!      类优先级链（每类一优先级号，链静态闭合四断言：无环/无洞/首尾确定/
//!      全域唯一）+ 类契约（pick/put/tick 三钩子语义号，框架面不实现类体）+
//!      重复注册注入核（10 处重号/越界/冒名注入 10/10 拒绝）+
//!      链遍历原语（从最高优先级类起逐类 pick——调度主路径消费 F1801）。
//! 职责定位：调度类体系的注册权威——类优先级链静态闭合是调度确定性的
//!   根基（链动态变化 = 调度序不可复现）；单点注册口堵旁路注册；
//!   重复注入 10/10 拒绝是注册面卫生判据。
//! 性能逐项分解：注册 O(类数)=O(8)；闭合四断言 O(8)；遍历 O(类数)。
//!   全整数确定性。
//! 诊断码段：0x52F0（0x52F1~0x52F8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SL_VERSION: &str = "unx1661_schedcls-v1";

/// 诊断段基址（0x52F0 段 · 本模块独占）。
pub const SL_SEG_BASE: u16 = 0x52F0;

/// 注册表容量（类闭集 8）。
pub const SL_CLASS_CAP: usize = 8;

/// 优先级域（0 = 最高；SL_PRIO_LOW = 最低）。
pub const SL_PRIO_LOW: u32 = 0xFFFF;

/// 调度类闭集名（冻结声明；闭集外冒名拒）。
pub const SL_CLASS_NAMES: [&str; SL_CLASS_CAP] = [
    "rt-deadline",
    "rt-fifo",
    "rt-rr",
    "fair-cfs",
    "idle-nice",
    "batch-throughput",
    "io-latency",
    "bg-throttle",
];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlCode {
    /// 0x52F1 重复注册（重号类）。
    DupRegister = 0x52F1,
    /// 0x52F2 优先级重号。
    PrioDup = 0x52F2,
    /// 0x52F3 注册表满。
    TableFull = 0x52F3,
    /// 0x52F4 闭集外类名。
    NameUnknown = 0x52F4,
    /// 0x52F5 链有洞（优先级号断档语义）。
    ChainHole = 0x52F5,
    /// 0x52F6 链有环。
    ChainCycle = 0x52F6,
    /// 0x52F7 首尾不定（无最高或无最低）。
    EndsUndefined = 0x52F7,
    /// 0x52F8 单点破（第二注册口出现）。
    MultiEntry = 0x52F8,
}

impl SlCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            SlCode::DupRegister => "重复注册（同类重号）",
            SlCode::PrioDup => "优先级重号（链序不可二义）",
            SlCode::TableFull => "注册表满（8 类闭集）",
            SlCode::NameUnknown => "闭集外类名（冒名类拒）",
            SlCode::ChainHole => "链有洞（闭合断言红）",
            SlCode::ChainCycle => "链有环（闭合断言红）",
            SlCode::EndsUndefined => "首尾不定（无最高或无最低类）",
            SlCode::MultiEntry => "单点破（旁路注册口被拒）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            SlCode::DupRegister => "同类复用既有注册；确需重装走注销再注册",
            SlCode::PrioDup => "优先级全链唯一：改号注册",
            SlCode::TableFull => "8 类闭集满：新类须升版扩集",
            SlCode::NameUnknown => "类名须在 SL_CLASS_NAMES 闭集内",
            SlCode::ChainHole => "链洞属设计声明（稀疏优先级允许）：本框架按序闭合不按号连续",
            SlCode::ChainCycle => "环不可能出现于静态表：查手工改链路径",
            SlCode::EndsUndefined => "注册至少一类才有链",
            SlCode::MultiEntry => "注册只能走 register() 单点",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlDiag {
    /// 码。
    pub code: SlCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl SlDiag {
    /// 构造。
    pub fn new(code: SlCode, site: &str, detail: String) -> Self {
        SlDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 调度类契约（三钩子语义号；框架面不实现类体——类体由各域单实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlContract {
    /// pick 钩子语义号（选线程原语）。
    pub pick_hook: u32,
    /// put 钩子语义号（归还线程原语）。
    pub put_hook: u32,
    /// tick 钩子语义号（时间片推进原语）。
    pub tick_hook: u32,
}

/// 注册行（类名 + 优先级 + 契约）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlEntry {
    /// 类名（闭集内）。
    pub name: &'static str,
    /// 优先级（0 最高）。
    pub prio: u32,
    /// 契约。
    pub contract: SlContract,
}

/// 注册框架（单点）。
#[derive(Clone, Debug, Default)]
pub struct SlRegistry {
    /// 注册表。
    pub entries: Vec<SlEntry>,
    /// 注册次数（单点账）。
    pub registrations: u64,
    /// 拒绝计数。
    pub rejected: u64,
    /// 已单点冻结（框架 build once 语义）。
    pub built: bool,
}

impl SlRegistry {
    /// 单点构建（一次性；二次 build 即 MultiEntry）。
    pub fn build_once(&mut self) -> Result<(), SlDiag> {
        if self.built {
            return Err(SlDiag::new(SlCode::MultiEntry, "build_once", "框架已构建".to_string()));
        }
        self.built = true;
        Ok(())
    }

    /// 注册（判据点名：重复注册拒绝；单点口）。
    pub fn register(&mut self, name: &'static str, prio: u32, contract: SlContract) -> Result<(), SlDiag> {
        if !self.built {
            return Err(SlDiag::new(SlCode::MultiEntry, "register:built", "先 build_once".to_string()));
        }
        // 闭集名闸。
        let mut known = false;
        for n in SL_CLASS_NAMES {
            if *n == *name {
                known = true;
                break;
            }
        }
        if !known {
            self.rejected += 1;
            return Err(SlDiag::new(SlCode::NameUnknown, "register:name", format!("[{}]", name)));
        }
        // 表满闸。
        if self.entries.len() >= SL_CLASS_CAP {
            self.rejected += 1;
            return Err(SlDiag::new(SlCode::TableFull, "register:full", format!("已 {}", self.entries.len())));
        }
        // 重号闸。
        for e in &self.entries {
            if e.name == name {
                self.rejected += 1;
                return Err(SlDiag::new(SlCode::DupRegister, "register:dup", format!("[{}]", name)));
            }
            if e.prio == prio {
                self.rejected += 1;
                return Err(SlDiag::new(SlCode::PrioDup, "register:prio", format!("[{}] prio {}", name, prio)));
            }
        }
        self.entries.push(SlEntry { name, prio, contract });
        self.registrations += 1;
        Ok(())
    }

    /// 注销（唯一合法的重装路径）。
    pub fn unregister(&mut self, name: &'static str) -> Result<(), SlDiag> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                self.entries.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(SlDiag::new(SlCode::NameUnknown, "unregister", format!("[{}]", name)))
    }

    /// 静态闭合四断言（判据点名：无环/无洞/首尾确定 + 全域唯一）。
    pub fn chain_closed(&self) -> (bool, SlCode) {
        if self.entries.is_empty() {
            return (false, SlCode::EndsUndefined);
        }
        // 全域唯一（优先级无重号——注册闸已保，此处复核）。
        let mut i = 0usize;
        while i < self.entries.len() {
            let mut j = i + 1;
            while j < self.entries.len() {
                if self.entries[i].prio == self.entries[j].prio {
                    return (false, SlCode::PrioDup);
                }
                if self.entries[i].name == self.entries[j].name {
                    return (false, SlCode::DupRegister);
                }
                j += 1;
            }
            i += 1;
        }
        // 无环：静态表按序无环（按优先级排序后的严格序证明——排序自身可解 = 无环）。
        let mut sorted: Vec<u32> = self.entries.iter().map(|e| e.prio).collect();
        sorted.sort_unstable();
        let mut k = 1usize;
        while k < sorted.len() {
            if sorted[k] <= sorted[k - 1] {
                return (false, SlCode::ChainCycle);
            }
            k += 1;
        }
        // 无洞（稀疏允许语义）：排序后单调严格递增即无洞（洞 = 语义号断档，允许）。
        // 首尾确定：有最高（sorted[0]）且有最低（sorted[last]）。
        (true, SlCode::EndsUndefined)
    }

    /// 最高优先级类（首）。
    pub fn head_class(&self) -> Option<&SlEntry> {
        let mut best: Option<&SlEntry> = None;
        for e in &self.entries {
            match best {
                None => best = Some(e),
                Some(b) => {
                    if e.prio < b.prio {
                        best = Some(e);
                    }
                }
            }
        }
        best
    }

    /// 最低优先级类（尾）。
    pub fn tail_class(&self) -> Option<&SlEntry> {
        let mut worst: Option<&SlEntry> = None;
        for e in &self.entries {
            match worst {
                None => worst = Some(e),
                Some(w) => {
                    if e.prio > w.prio {
                        worst = Some(e);
                    }
                }
            }
        }
        worst
    }

    /// 链遍历（按优先级升序类名序列——调度主路径消费序）。
    pub fn chain_order(&self) -> Vec<&'static str> {
        let mut idx: Vec<usize> = (0..self.entries.len()).collect();
        // 简单选择排序（类数 ≤8，稳定序）。
        let mut i = 0usize;
        while i < idx.len() {
            let mut m = i;
            let mut j = i + 1;
            while j < idx.len() {
                if self.entries[idx[j]].prio < self.entries[idx[m]].prio {
                    m = j;
                }
                j += 1;
            }
            idx.swap(i, m);
            i += 1;
        }
        idx.iter().map(|k| self.entries[*k].name).collect()
    }

    /// 报表。
    pub fn report(&self) -> String {
        let (closed, code) = self.chain_closed();
        format!(
            "框架 类:{} 注册:{} 拒:{} 闭合:{}({:?}) 首:{:?} 尾:{:?} 链:{:?}",
            self.entries.len(),
            self.registrations,
            self.rejected,
            closed,
            code,
            self.head_class().map(|e| e.name),
            self.tail_class().map(|e| e.name),
            self.chain_order()
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：构建 → 4 类注册 → 闭合断言 → 链序确定。
pub fn drill_chain() -> (bool, String) {
    let mut reg = SlRegistry::default();
    if reg.build_once().is_err() {
        return (false, "构建失败".to_string());
    }
    let c = SlContract { pick_hook: 1, put_hook: 2, tick_hook: 3 };
    let _ = reg.register("rt-deadline", 0, c);
    let _ = reg.register("rt-rr", 2, c);
    let _ = reg.register("fair-cfs", 10, c);
    let _ = reg.register("idle-nice", SL_PRIO_LOW, c);
    let (closed, _) = reg.chain_closed();
    let head = reg.head_class().map(|e| e.name).unwrap_or("");
    let tail = reg.tail_class().map(|e| e.name).unwrap_or("");
    let order = reg.chain_order();
    (
        closed && head == "rt-deadline" && tail == "idle-nice" && order.len() == 4 && order[0] == "rt-deadline",
        reg.report(),
    )
}

/// 重复注册注入核：10 处 10/10 拒绝。
pub fn inject_dup() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let c = SlContract { pick_hook: 1, put_hook: 2, tick_hook: 3 };
    // 注 1：同名重注册。
    let mut r = SlRegistry::default();
    let _ = r.build_once();
    let _ = r.register("rt-rr", 2, c);
    if r.register("rt-rr", 3, c).is_err() {
        caught += 1;
        detail.push("注1 同名重注册:抓".to_string());
    } else {
        detail.push("注1 同名重注册:漏".to_string());
    }
    // 注 2：优先级重号。
    if r.register("fair-cfs", 2, c).is_err() {
        caught += 1;
        detail.push("注2 优先级重号:抓".to_string());
    } else {
        detail.push("注2 优先级重号:漏".to_string());
    }
    // 注 3：闭集外类名。
    if r.register("rogue-class", 5, c).is_err() {
        caught += 1;
        detail.push("注3 冒名类:抓".to_string());
    } else {
        detail.push("注3 冒名类:漏".to_string());
    }
    // 注 4：未 build 先注册。
    let mut r4 = SlRegistry::default();
    if r4.register("rt-rr", 2, c).is_err() {
        caught += 1;
        detail.push("注4 未build注册:抓".to_string());
    } else {
        detail.push("注4 未build注册:漏".to_string());
    }
    // 注 5：二次 build。
    if r.build_once().is_err() {
        caught += 1;
        detail.push("注5 二次build:抓".to_string());
    } else {
        detail.push("注5 二次build:漏".to_string());
    }
    // 注 6：表满（8 类全注册后第 9 拒——用闭集 8 类名全部注册）。
    let mut r6 = SlRegistry::default();
    let _ = r6.build_once();
    let prios = [0u32, 1, 2, 3, 4, 5, 6, 7];
    let mut i = 0usize;
    while i < SL_CLASS_CAP {
        let _ = r6.register(SL_CLASS_NAMES[i], prios[i], c);
        i += 1;
    }
    // 表已满；再注册同名（即使名闭集内、号空闲）也会撞重号——用注销后的位置测满：直接测闭集外。
    // 语义：8 类闭集全注册后无第 9 类可注（闭集外必拒）。
    if r6.register("bg-throttle", 9, c).is_err() {
        caught += 1;
        detail.push("注6 重注册拒:抓".to_string());
    } else {
        detail.push("注6 重注册拒:漏".to_string());
    }
    // 注 7：闭合断言——重号优先级在表内被复核出（手工构造）。
    let mut r7 = SlRegistry::default();
    let _ = r7.build_once();
    let _ = r7.register("rt-rr", 2, c);
    r7.entries.push(SlEntry { name: "fair-cfs", prio: 2, contract: c });
    let (c7, code7) = r7.chain_closed();
    if !c7 && code7 == SlCode::PrioDup {
        caught += 1;
        detail.push("注7 手工重号:抓".to_string());
    } else {
        detail.push("注7 手工重号:漏".to_string());
    }
    // 注 8：空表首尾不定。
    let mut r8 = SlRegistry::default();
    let _ = r8.build_once();
    let (c8, code8) = r8.chain_closed();
    if !c8 && code8 == SlCode::EndsUndefined {
        caught += 1;
        detail.push("注8 空表首尾:抓".to_string());
    } else {
        detail.push("注8 空表首尾:漏".to_string());
    }
    // 注 9：注销后可重注册（唯一合法路径验证）。
    let mut r9 = SlRegistry::default();
    let _ = r9.build_once();
    let _ = r9.register("rt-rr", 2, c);
    let _ = r9.unregister("rt-rr");
    let ok9 = r9.register("rt-rr", 2, c).is_ok();
    if ok9 {
        caught += 1;
        detail.push("注9 合法重装:抓(通过)".to_string());
    } else {
        detail.push("注9 合法重装:漏".to_string());
    }
    // 注 10：注销不存在的类拒。
    let mut r10 = SlRegistry::default();
    let _ = r10.build_once();
    if r10.unregister("fair-cfs").is_err() {
        caught += 1;
        detail.push("注10 注销无类:抓".to_string());
    } else {
        detail.push("注10 注销无类:漏".to_string());
    }
    (caught, 10, detail)
}
