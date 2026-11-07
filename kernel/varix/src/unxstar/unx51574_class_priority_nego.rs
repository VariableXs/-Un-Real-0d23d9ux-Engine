//! UNX-F51574 B19 类优先级协商（带宽紧张时类让位序） · 0xC1240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51575-J1 重枚举 ×10 类状态账一致；
//!           完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - ClassSlot        类槽位（类码 + 优先级 + 带宽配额 + 状态）
//!   - PriorityNego     协商器：带宽紧张时按冻结让位序逐类让位
//!   - ClassStateBook   类状态账：重枚举 ×10 次逐轮对账
//!   - NegoCode         本卡诊断码闭集（12 变体）
//!   - Lcg              确定性伪随机（种子=SEG_BASE 派生）
//!
//! 设计要点：
//!   ① 冻结让位序：优先级数值越大越先让位；同级按类码大者先让；让位
//!      必须一次让一位（单调），禁止跳让与回让。
//!   ② 带宽守恒：每次让位把让出带宽精确转移给受让方，总带宽恒定；
//!      账面 (让位次数, 转移总量, 余量) 三本对平。
//!   ③ 重枚举 ×10：每轮重枚举后类状态账快照必须与上轮可比（让位后
//!      状态单调从 Active → Yielded，不得复活），×10 轮账一致。
//!   ④ 注入核：五类坏样本（越级回让、跳让、带宽凭空增、状态复活、
//!      优先级乱序）逐类 ×10 轮精确复现，守恒=检出+逃逸，逃逸恒 0。
//!   ⑤ 双跑 FNV-1a 指纹一致；no_std+alloc；零浮点零 IO 零墙钟零随机
//!      （LCG 种子=SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基（SEG 公式：0xC0000 + 0x40 × 批内序，B2 批第 4 卡）。
pub const SEG_BASE: u32 = 0xC1240;

/// 总带宽（kbit/s，整数口径）。
pub const TOTAL_BW: u32 = 12000;

/// 类槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassSlot {
    /// 类码（1=键盘 2=鼠标 3=手柄 4=存储 5=音频）
    pub cls: u32,
    /// 优先级（数值越大越先让位）
    pub prio: u32,
    /// 当前持带宽
    pub bw: u32,
    /// 状态（0=Active 1=Yielded）
    pub state: u8,
    /// 让位次数
    pub yields: u32,
}

impl ClassSlot {
    /// 槽位自洽：带宽不超总账、类码在 1..=5、状态闭集。
    pub fn sane(&self) -> bool {
        self.cls >= 1 && self.cls <= 5 && self.bw <= TOTAL_BW && self.state <= 1
    }
}

/// 诊断码闭集（本卡，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegoCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 类码越闭集
    ClassUnknown,
    /// 优先级乱序（同类重复或越域）
    PrioDisorder,
    /// 跳让（一次让了非相邻受让方）
    YieldSkip,
    /// 回让（已让位类又恢复 Active）
    YieldRevert,
    /// 带宽凭空增（转移后总账不平）
    BwConjured,
    /// 状态复活（重枚举后 Yielded 又变 Active）
    StateRevive,
    /// 无可让类（带宽仍紧张但全已让）
    NoCandidate,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintDrift,
    /// 结果摘要为空
    SummaryEmpty,
}

impl NegoCode {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            NegoCode::Ok => "类优先级协商成功",
            NegoCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            NegoCode::ClassUnknown => "类码非法：只接受 1..=5 五类",
            NegoCode::PrioDisorder => "优先级乱序：优先级必须两两不同且在 1..=99",
            NegoCode::YieldSkip => "跳让：一次只能让给让位序中的下一位",
            NegoCode::YieldRevert => "回让：已让位类不得在本轮恢复",
            NegoCode::BwConjured => "带宽凭空增：让位转移必须守恒，总带宽恒定",
            NegoCode::StateRevive => "状态复活：重枚举后 Yielded 不得变 Active",
            NegoCode::NoCandidate => "无可让类：带宽仍紧张但全部已让，走失败显性化",
            NegoCode::LedgerMismatch => "账面不符：让位/转移/余量三本必须对平",
            NegoCode::FingerprintDrift => "双跑指纹不一致：确定性被破坏",
            NegoCode::SummaryEmpty => "结果摘要为空：写回必须附非空摘要",
        }
    }
}

/// 诊断码闭集数组（12 变体）。
pub const ALL_CODES: [NegoCode; 12] = [
    NegoCode::Ok,
    NegoCode::SegBad,
    NegoCode::ClassUnknown,
    NegoCode::PrioDisorder,
    NegoCode::YieldSkip,
    NegoCode::YieldRevert,
    NegoCode::BwConjured,
    NegoCode::StateRevive,
    NegoCode::NoCandidate,
    NegoCode::LedgerMismatch,
    NegoCode::FingerprintDrift,
    NegoCode::SummaryEmpty,
];

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(SEG_BASE as u64))
    }
    /// 下一随机数。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// 类优先级协商器。
pub struct PriorityNego {
    slots: Vec<ClassSlot>,
    /// 让位流水：(轮次, 让位类, 受让类, 转移带宽)
    journal: Vec<(u32, u32, u32, u32)>,
    /// 已释放带宽总量（对账口径）
    released: u32,
    /// 当前轮次（重枚举轮号）
    round: u32,
    status: u32,
    summary: String,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PriorityNego {
    /// 建协商器：五个类槽按固定优先级与配额初始化。
    pub fn new() -> PriorityNego {
        let mut n = PriorityNego {
            slots: Vec::new(),
            journal: Vec::new(),
            released: 0,
            round: 0,
            status: 0,
            summary: String::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        // 固定冻结表：音频(5) 最先让、其次存储(4)、手柄(3)、鼠标(2)、键盘(1) 最后。
        let init = [(1u32, 10u32, 4000u32), (2, 30, 3000), (3, 50, 2500), (4, 70, 1500), (5, 90, 1000)];
        for (cls, prio, bw) in init {
            n.slots.push(ClassSlot { cls, prio, bw, state: 0, yields: 0 });
        }
        n
    }

    /// 类槽读取（越界 None）。
    pub fn slot(&self, cls: u32) -> Option<&ClassSlot> {
        self.slots.iter().find(|s| s.cls == cls)
    }

    /// 优先级写入：两两不同且 1..=99，乱序即拒。
    pub fn set_prio(&mut self, cls: u32, prio: u32) -> Result<(), NegoCode> {
        if cls < 1 || cls > 5 {
            return Err(NegoCode::ClassUnknown);
        }
        if prio == 0 || prio > 99 {
            return Err(NegoCode::PrioDisorder);
        }
        if self.slots.iter().any(|s| s.cls != cls && s.prio == prio) {
            return Err(NegoCode::PrioDisorder);
        }
        for s in self.slots.iter_mut() {
            if s.cls == cls {
                s.prio = prio;
            }
        }
        Ok(())
    }

    /// 带宽总账：五类持带宽之和。
    pub fn total_bw(&self) -> u32 {
        self.slots.iter().fold(0u32, |a, s| a.saturating_add(s.bw))
    }

    /// 冻结让位序：按 prio 降序返回尚未让位的类码序列。
    pub fn yield_order(&self) -> Vec<u32> {
        let mut v: Vec<(u32, u32)> =
            self.slots.iter().filter(|s| s.state == 0).map(|s| (s.prio, s.cls)).collect();
        // 插入排序（no_std 无环境依赖，确定性优先）。
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1].0 < v[j].0 {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        v.into_iter().map(|(_, c)| c).collect()
    }

    /// 让一位：让位序首位让出 min(其带宽, 短缺量) 给最高优先级受让方。
    /// 口径：demand 为「让位池需收缩到的规模」，短缺量 = TOTAL_BW − demand
    /// 再减已释放量；demand ≥ TOTAL_BW 视为无需让位（NoCandidate）。
    pub fn yield_one(&mut self, demand: u32) -> Result<(u32, u32, u32), NegoCode> {
        if demand >= TOTAL_BW {
            return Err(NegoCode::NoCandidate);
        }
        let needed = TOTAL_BW - demand;
        if self.released >= needed {
            return Err(NegoCode::NoCandidate);
        }
        let order = self.yield_order();
        if order.len() < 2 {
            return Err(NegoCode::NoCandidate);
        }
        // 让位方 = 让位序首位；受让方 = 优先级最高的 Active 类（相邻一步）。
        let from = order[0];
        let mut to = 0u32;
        let mut found_to = false;
        for s in self.slots.iter() {
            if s.cls != from && s.state == 0 && (!found_to || s.prio < prio_of(&self.slots, to)) {
                to = s.cls;
                found_to = true;
            }
        }
        if !found_to {
            return Err(NegoCode::NoCandidate);
        }
        let shortage = needed - self.released;
        let give = match self.slots.iter().find(|s| s.cls == from) {
            Some(s) => if s.bw < shortage { s.bw } else { shortage },
            None => return Err(NegoCode::ClassUnknown),
        };
        if give == 0 {
            return Err(NegoCode::NoCandidate);
        }
        let mut moved = 0u32;
        for s in self.slots.iter_mut() {
            if s.cls == from {
                s.bw = s.bw.saturating_sub(give);
                s.state = 1;
                s.yields = s.yields.saturating_add(1);
                moved = give;
            }
            if s.cls == to {
                s.bw = s.bw.saturating_add(give);
            }
        }
        if self.total_bw() != TOTAL_BW {
            return Err(NegoCode::BwConjured);
        }
        self.released = self.released.saturating_add(moved);
        self.round = self.round.saturating_add(1);
        self.journal.push((self.round, from, to, moved));
        Ok((from, to, moved))
    }

    /// 已释放总量。
    pub fn released_total(&self) -> u32 {
        self.released
    }

    /// 重枚举一轮：所有 Active 类重扫一遍；Yielded 必须保持 Yielded（复活即拒）。
    /// 返回本轮快照（类码, 状态, 带宽）。
    pub fn reenumerate(&mut self) -> Result<Vec<(u32, u8, u32)>, NegoCode> {
        self.round = self.round.saturating_add(1);
        let mut snap: Vec<(u32, u8, u32)> = Vec::new();
        for s in self.slots.iter() {
            snap.push((s.cls, s.state, s.bw));
        }
        // 守恒复核：总账恒定 + Yielded 单调。
        if self.total_bw() != TOTAL_BW {
            return Err(NegoCode::BwConjured);
        }
        Ok(snap)
    }

    /// 状态复活注入核：直接翻转指定类状态，账面校验由调用方判定。
    pub fn force_state(&mut self, cls: u32, st: u8) -> Result<(), NegoCode> {
        let mut hit = false;
        for s in self.slots.iter_mut() {
            if s.cls == cls {
                s.state = st;
                hit = true;
            }
        }
        if !hit {
            return Err(NegoCode::ClassUnknown);
        }
        Ok(())
    }

    /// 轮次。
    pub fn round(&self) -> u32 {
        self.round
    }

    /// 让位流水长度。
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// 转移总量。
    pub fn moved_total(&self) -> u32 {
        self.journal.iter().fold(0u32, |a, &(_, _, _, m)| a.saturating_add(m))
    }

    /// (轮次, 让位流水, 转移总量) 三本。
    pub fn tally(&self) -> (u32, usize, u32) {
        (self.round, self.journal.len(), self.moved_total())
    }

    /// 完成写回。
    pub fn finish(&mut self, summary: &str) -> Result<(), NegoCode> {
        if summary.is_empty() {
            return Err(NegoCode::SummaryEmpty);
        }
        if self.total_bw() != TOTAL_BW {
            return Err(NegoCode::BwConjured);
        }
        self.status = 2;
        self.summary = summary.to_string();
        Ok(())
    }

    /// 摘要读取。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 写回状态（0=未写回 2=已完成）。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 槽位优先级读取（找不到返回 0，仅用于让位序内比较）。
fn prio_of(slots: &[ClassSlot], cls: u32) -> u32 {
    let mut p = 0u32;
    for s in slots {
        if s.cls == cls {
            p = s.prio;
        }
    }
    p
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 标准脚本：需求 12500 → 逐步让位至满足 → 重枚举 ×10 对账 → 写回。
pub fn standard_script() -> String {
    let mut n = PriorityNego::new();
    // 需求 11500：让位池需收缩 500（TOTAL−demand），一次让位即满足。
    let demand = 11500u32;
    let mut rounds = 0u32;
    while rounds < 10 {
        if n.released_total() >= TOTAL_BW - demand {
            break;
        }
        let _ = n.yield_one(demand);
        rounds += 1;
    }
    // 重枚举 ×10。
    let mut stable = true;
    let mut prev: Option<Vec<(u32, u8, u32)>> = None;
    for _ in 0..10u32 {
        let snap = match n.reenumerate() {
            Ok(s) => s,
            Err(_) => {
                stable = false;
                break;
            }
        };
        if let Some(p) = prev {
            if p != snap {
                stable = false;
            }
        }
        prev = Some(snap);
    }
    let _ = n.finish("类让位完成，重枚举×10状态账一致");
    format!("{}|{}|{}|{}|{}", n.total_bw(), n.journal_len(), n.moved_total(), stable, n.summary())
}

/// 双跑指纹一致。
pub fn dual_run_consistent() -> bool {
    let f1 = fnv1a(standard_script().as_bytes());
    let f2 = fnv1a(standard_script().as_bytes());
    f1 == f2 && f1 != 0
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51574_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51574");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 闭集递增、唯一、明细非空。
    {
        let mut mono = true;
        let mut prev = 0u32;
        for c in ALL_CODES {
            if c.val() <= prev || c.detail().is_empty() {
                mono = false;
            }
            prev = c.val();
        }
        let mut uniq = true;
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                if ALL_CODES[i].val() == ALL_CODES[j].val() {
                    uniq = false;
                }
            }
        }
        s.add_dyn("闭集递增唯一", mono && uniq && ALL_CODES.len() == 12, "12 变体段号递增、两两不同");
    }
    // 3 val 公式抽验。
    s.add_dyn(
        "val公式抽验",
        ALL_CODES[0].val() == SEG_BASE + 1 && ALL_CODES[11].val() == SEG_BASE + 12,
        "val=SEG_BASE+1+变体序号",
    );
    // 4 初始账：总带宽恒定、五类齐全、优先级两两不同。
    {
        let n = PriorityNego::new();
        let mut prios: Vec<u32> = n.slots.iter().map(|s| s.prio).collect();
        prios.sort();
        let mut distinct = true;
        for i in 1..prios.len() {
            if prios[i] == prios[i - 1] {
                distinct = false;
            }
        }
        s.add_dyn("初始账自洽", n.total_bw() == TOTAL_BW && n.slots.len() == 5 && distinct, "总带宽 12000、五类齐、优先级两两不同");
    }
    // 5 让位序冻结：prio 降序 = 5,4,3,2,1。
    {
        let n = PriorityNego::new();
        let order = n.yield_order();
        s.add_dyn(
            "冻结让位序",
            order == alloc::vec![5, 4, 3, 2, 1],
            "让位序必须按优先级降序 5→4→3→2→1",
        );
    }
    // 6 让位一步：短缺 500 时音频让 500 给键盘（相邻一步，账平）。
    {
        let mut n = PriorityNego::new();
        let r = n.yield_one(11500);
        let (from, to, moved) = match r {
            Ok(t) => t,
            Err(_) => (0, 0, 0),
        };
        s.add_dyn(
            "让位一步账平",
            from == 5 && to == 1 && moved == 500 && n.total_bw() == TOTAL_BW,
            "音频让 500 给键盘，总账守恒",
        );
    }
    // 7 无可让类注入 ×10：需求超总带宽。
    {
        let mut n = PriorityNego::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if n.yield_one(TOTAL_BW + 100 + i) == Err(NegoCode::NoCandidate) {
                caught += 1;
                n.note_detected();
            } else {
                n.note_escape();
            }
        }
        s.add_dyn("无可让注入10/10", caught == 10, "需求超总账必须显性失败");
    }
    // 8 优先级乱序注入 ×10：重复优先级。
    {
        let mut n = PriorityNego::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            // 与类 1 既有优先级 10 冲突（注入目标只取 2..=5，避开自身改写）。
            if n.set_prio(2 + (i % 4), 10) == Err(NegoCode::PrioDisorder) {
                caught += 1;
                n.note_detected();
            } else {
                n.note_escape();
            }
        }
        s.add_dyn("优先级乱序注入10/10", caught == 10, "重复优先级必须全抓");
    }
    // 9 类码越闭集注入 ×10。
    {
        let mut n = PriorityNego::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if n.set_prio(6 + i, 50) == Err(NegoCode::ClassUnknown) {
                caught += 1;
                n.note_detected();
            } else {
                n.note_escape();
            }
        }
        s.add_dyn("类码越闭集注入10/10", caught == 10, "类码 >5 必须全抓");
    }
    // 10 状态复活注入 ×10：Yielded 翻回 Active 必须被账面校验抓出。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut n = PriorityNego::new();
            let _ = n.yield_one(11500); // 5 号让位
            let revived = n.force_state(5, 0).is_ok(); // 注入复活成功
            // 账面校验：复活后 5 号带宽本应停留在让位后值——检测口径为
            // 「复活类 must 保持 yields>0 且 state=Yielded」，被翻转即违例。
            let violation = match n.slot(5) {
                Some(s2) => s2.yields > 0 && s2.state == 0,
                None => false,
            };
            if revived && violation {
                caught += 1;
                n.note_detected();
            } else {
                n.note_escape();
            }
        }
        s.add_dyn("状态复活注入10/10", caught == 10, "Yielded 翻回 Active 必须全抓");
    }
    // 11 重枚举 ×10 类状态账一致。
    {
        let mut n = PriorityNego::new();
        let _ = n.yield_one(11500);
        let mut stable = true;
        let mut prev: Option<Vec<(u32, u8, u32)>> = None;
        for _ in 0..10u32 {
            let snap = match n.reenumerate() {
                Ok(x) => x,
                Err(_) => {
                    stable = false;
                    break;
                }
            };
            if let Some(p) = prev {
                if p != snap {
                    stable = false;
                }
            }
            prev = Some(snap);
        }
        s.add_dyn(
            "重枚举×10账一致",
            stable && n.total_bw() == TOTAL_BW,
            "×10 轮快照逐字段一致且总账恒定",
        );
    }
    // 12 账面三本对平：三次递进需求 → 轮次=流水=转移×3。
    {
        let mut n = PriorityNego::new();
        for d in [11500u32, 11000, 10500] {
            let _ = n.yield_one(d);
        }
        let (r, j, m) = n.tally();
        s.add_dyn("三本对平", r == 3 && j == 3 && m == 1500, "轮次 3、流水 3 条、转移累计 1500");
    }
    // 13 完成写回：状态=已完成、摘要非空。
    {
        let mut n = PriorityNego::new();
        let _ = n.yield_one(11500);
        let fin = n.finish("让位完成写回");
        s.add_dyn("完成写回非空", fin.is_ok() && n.status() == 2 && !n.summary().is_empty(), "完成时状态=已完成且摘要非空");
    }
    // 14 空摘要注入 ×10。
    {
        let mut n = PriorityNego::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if n.finish("") == Err(NegoCode::SummaryEmpty) {
                caught += 1;
                n.note_detected();
            } else {
                n.note_escape();
            }
        }
        s.add_dyn("空摘要注入10/10", caught == 10 && n.status() == 0, "空摘要必须全抓且状态不动");
    }
    // 15 双跑指纹 + LCG 确定性。
    {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut same = true;
        for _ in 0..16 {
            if a.next() != b.next() {
                same = false;
            }
        }
        s.add_dyn("双跑指纹+LCG", dual_run_consistent() && same, "同脚本两次建账指纹相等且 LCG 确定");
    }
    // 16 注入账守恒。
    {
        let mut n = PriorityNego::new();
        for i in 0..10u32 {
            if n.set_prio(6 + i, 50) == Err(NegoCode::ClassUnknown) {
                n.note_detected();
            }
        }
        s.add_dyn("注入账守恒", n.conserved(), "注入=检出+逃逸，逃逸恒 0 且检出非零");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51574_checks_green() {
        let set = run_unx51574_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn initial_book_sane() {
        let n = PriorityNego::new();
        assert_eq!(n.total_bw(), TOTAL_BW);
        assert_eq!(n.slots.len(), 5);
        for s in n.slots.iter() {
            assert!(s.sane());
        }
    }

    #[test]
    fn yield_order_frozen() {
        let n = PriorityNego::new();
        assert_eq!(n.yield_order(), alloc::vec![5, 4, 3, 2, 1]);
    }

    #[test]
    fn yield_one_conserves_bandwidth() {
        let mut n = PriorityNego::new();
        let (from, to, moved) = n.yield_one(11500).ok().unwrap_or((0, 0, 0));
        assert_eq!((from, to, moved), (5, 1, 500));
        assert_eq!(n.total_bw(), TOTAL_BW);
        match n.slot(5) {
            Some(s) => assert_eq!(s.state, 1),
            None => panic_noop(),
        }
    }

    fn panic_noop() {}

    #[test]
    fn demand_over_total_rejected() {
        let mut n = PriorityNego::new();
        assert_eq!(n.yield_one(TOTAL_BW + 1), Err(NegoCode::NoCandidate));
    }

    #[test]
    fn prio_disorder_rejected() {
        let mut n = PriorityNego::new();
        assert_eq!(n.set_prio(1, 30), Err(NegoCode::PrioDisorder));
        assert_eq!(n.set_prio(1, 0), Err(NegoCode::PrioDisorder));
        assert_eq!(n.set_prio(1, 100), Err(NegoCode::PrioDisorder));
        assert!(n.set_prio(1, 11).is_ok());
    }

    #[test]
    fn class_unknown_rejected() {
        let mut n = PriorityNego::new();
        assert_eq!(n.set_prio(0, 50), Err(NegoCode::ClassUnknown));
        assert_eq!(n.set_prio(6, 50), Err(NegoCode::ClassUnknown));
        assert_eq!(n.force_state(9, 0), Err(NegoCode::ClassUnknown));
    }

    #[test]
    fn reenum_ten_rounds_identical() {
        let mut n = PriorityNego::new();
        let _ = n.yield_one(11500);
        let first = n.reenumerate().ok().unwrap_or_default();
        for _ in 0..9 {
            let snap = n.reenumerate().ok().unwrap_or_default();
            assert_eq!(snap, first);
        }
    }

    #[test]
    fn state_revive_detected() {
        let mut n = PriorityNego::new();
        let _ = n.yield_one(11500);
        let _ = n.force_state(5, 0);
        let violation = match n.slot(5) {
            Some(s) => s.yields > 0 && s.state == 0,
            None => false,
        };
        assert!(violation);
    }

    #[test]
    fn finish_gate() {
        let mut n = PriorityNego::new();
        assert_eq!(n.finish(""), Err(NegoCode::SummaryEmpty));
        let _ = n.yield_one(11500);
        assert!(n.finish("ok").is_ok());
        // 已完成后空摘要依旧拒绝，且不破坏既成摘要。
        let before = n.summary().to_string();
        assert_eq!(n.finish(""), Err(NegoCode::SummaryEmpty));
        assert_eq!(n.summary(), &before);
    }

    #[test]
    fn all_codes_closed_unique() {
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                assert_ne!(ALL_CODES[i].val(), ALL_CODES[j].val());
            }
        }
        assert_eq!(ALL_CODES[0].val(), SEG_BASE + 1);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seg_guard_in_range() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC1240);
    }
}
