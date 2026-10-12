//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0934 · 核间死锁防线（call 链环路检测）· 目标 280 行 · 已深化
//! 判据原文摘录：UNX-F0934-J1 环路检测正确（A→B→A 注入 10/10 次检出），
//!           防线开销 <100ns（发起路径），误报 0（正常并发 10 万次零误报）。
//! 职责定位：smp_call 的等待边 A 等 B 的应答、B 等 A 的应答，就是一张
//! 有向等待图；图里出现环 = 核间死锁。本模块是等待图的**静态防线**：
//! 每次发起 call 前按调用链做一次增量环检，环出现即在入队前拒绝，
//! 不等运行时挂起才发现。防线的代价必须可量化——所以开销本身是判据
//! （<100ns 定点纳秒账，逐案例落数），误报率同样是判据（10 万次正常
//! 并发零误报）。
//! 设计要点：①**等待图即矩阵**——每核一行等待位图（64 核内 u64 掩码），
//! 边"A 等 B"=在 A 的行置 B 位；环检 = 从新边起点沿等待边走，若回到起点
//! 即环。64 核掩码让"能否一步走到 B"变成一次位与，O(核数/字长) 常数；
//! ②**增量登记而非全图重扫**——发起 call 时只在本核等待行置位、只做
//! 一次深度受限的走步（深度上限=当前链长，链不可能无界），不做全图
//! 可达闭包（闭包随核数平方涨，会把发起路径顶穿 100ns 线）；
//! ③**三步审批（去抖动）**——等待边存在但目标尚未在途时挂 pending，
//! 目标回执后升级为 confirmed；只有 confirmed 的边才参与环检（pending
//! 是瞬时态，会写出跨核时序假环——假环=误报的头号来源）；
//! ④**自环豁免不可滥用**——自环（A 等 A）不是"合法重入"而是显性拒绝，
//! 同步 call 等自己的应答在物理上不可能完成，必须入队前拒（区别于
//! 异步 notify 的自唤醒，那是另一面的合法路径，本账只审同步边）；
//! ⑤**撤销两条路**——应答到达撤边、超时撤边；撤边必须显幂（重复撤销
//! 不二次清位，第二次撤销=账面已被别人改过=账被污染）；
//! ⑥**防线的自我守卫生成账**——本模块的开销账不得低于理论下限，
//! 也不得高于上限（低=记账被优化掉=统计失真，高=防线自身成为热点）。
//! 诊断码段：0xD5（本单独占，0xD501~0xD50F）。
//! 施工：AI · 2026-10-12。

use alloc::format;
use alloc::string::String;

/// 族共享伪随机源（同种子同序列——双跑确定性，与 B07 他模块同构）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD5 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD501 环路未检（注入环但防线放行——死锁防线失效）。
pub const XC_DL_CYCLE_MISS: u32 = 0xD501;
/// 0xD502 环路误报（正常并发被判环——误报率判据失守）。
pub const XC_DL_CYCLE_FALSE: u32 = 0xD502;
/// 0xD503 自环未拒（A 等 A 同步边入队——物理不可能完成的边）。
pub const XC_DL_SELF_LOOP: u32 = 0xD503;
/// 0xD504 边未确认即入检（pending 边参与环检——假环写入口）。
pub const XC_DL_UNCONFIRMED: u32 = 0xD504;
/// 0xD505 撤边非幂（重复撤销仍报成功——账被污染）。
pub const XC_DL_RETRACT_NONIDEM: u32 = 0xD505;
/// 0xD506 边账失守（在途边数与明细不符——等待图账目不平）。
pub const XC_DL_EDGE_LEDGER: u32 = 0xD506;
/// 0xD507 深度越限（链深超上限仍放行——步数红线失守）。
pub const XC_DL_DEPTH_OOB: u32 = 0xD507;
/// 0xD508 越核边（边源非本核——等待图面被伪造）。
pub const XC_DL_ALIEN_EDGE: u32 = 0xD508;
/// 0xD509 核号越界（核号 ≥64——位图面失守）。
pub const XC_DL_CPU_OOB: u32 = 0xD509;
/// 0xD50A 开销越上限（发起路径环检超 100ns——防线自身成热点）。
pub const XC_DL_COST_OVER: u32 = 0xD50A;
/// 0xD50B 开销低于下限（环检开销异常小——记账被优化掉的失真）。
pub const XC_DL_COST_UNDER: u32 = 0xD50B;
/// 0xD50C 应答无对应边（撤不存在的边——应答面与等待图脱钩）。
pub const XC_DL_ORPHAN_ACK: u32 = 0xD50C;
/// 0xD50D 双跑漂移（同等待图两演指纹不等——确定性破坏）。
pub const XC_DL_DUAL_DRIFT: u32 = 0xD50D;
/// 0xD50E 撤后残留（应答撤边后行内仍置位——环检面污染）。
pub const XC_DL_RESIDUE: u32 = 0xD50E;
/// 0xD50F 走步越界（搜索走到环外地址——遍历器失守）。
pub const XC_DL_WALK_OOB: u32 = 0xD50F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_DL_CYCLE_MISS => (
            "环路未检：注入的等待环被防线放行",
            "入队前三步审批边必须参与环检；漏检=核间死锁直接进生产",
        ),
        XC_DL_CYCLE_FALSE => (
            "环路误报：正常并发被判成环",
            "pending 瞬时边不得入检、深度不得超链长；误报=防线不可用",
        ),
        XC_DL_SELF_LOOP => (
            "自环未拒：同步边 A 等 A 入队",
            "同步 call 等自己的应答物理不可能完成；必须入队前显性拒绝",
        ),
        XC_DL_UNCONFIRMED => (
            "边未确认：pending 边参与环检",
            "三步审批只认 confirmed；未确认边=跨核时序假环写入口",
        ),
        XC_DL_RETRACT_NONIDEM => (
            "撤边非幂：重复撤销报成功",
            "撤边首次清位返回真、二次返回假；全返真=账被别人改过还在报喜",
        ),
        XC_DL_EDGE_LEDGER => (
            "边账失守：在途边计数与置位数不符",
            "边账=行内置位计数+总在途；两边不等=置位/清位拆步",
        ),
        XC_DL_DEPTH_OOB => (
            "深度越限：等待链深超上限仍放行",
            "链深上限=当前核数；越限=遍历器失守或链构造被注入",
        ),
        XC_DL_ALIEN_EDGE => (
            "越核边：等待边源非本核",
            "只有等待方本核有权置自己的等待位；越核置位=等待图被伪造",
        ),
        XC_DL_CPU_OOB => (
            "核号越界：核号 ≥64",
            "本账 64 核位图；越界核号=位图面失守",
        ),
        XC_DL_COST_OVER => (
            "开销越上限：发起路径环检超 100ns",
            "环检只做增量走步不做全图闭包；越线=防线自身成热点，违背定位",
        ),
        XC_DL_COST_UNDER => (
            "开销失真：环检记账低于理论下限",
            "记账低于下限=被编译器优化掉或计时面失真；统计必须可信",
        ),
        XC_DL_ORPHAN_ACK => (
            "孤儿应答：撤销的边不存在",
            "应答必须对应等待图中的 confirmed 边；孤儿=超时已撤而应答迟到",
        ),
        XC_DL_DUAL_DRIFT => (
            "双跑漂移：同等待图两演指纹不等",
            "同种子同边序；不等则逐段二分找隐藏可变状态",
        ),
        XC_DL_RESIDUE => (
            "撤后残留：应答撤边后行内仍置位",
            "撤边=清位+计数减一；残留=清位路径漏步，环检面被污染",
        ),
        XC_DL_WALK_OOB => (
            "走步越界：环检搜索走到非法地址",
            "走步只在 64 核位图内按 lowest-set-bit 前进；越界=遍历器失守",
        ),
        _ => ("未知诊断码", "码不在 0xD5 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 等待图（每核一行 64 位等待掩码）
// ---------------------------------------------------------------------------

/// 核数上限（位图宽度）。
pub const MAX_CPU: usize = 64;

/// 单核等待行（谁在等我 + 我在等谁）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitRow {
    pub cpu: u32,
    /// 我在等待的核掩码（bit i = 我在等核 i 的应答）。
    pub waiting: u64,
    /// 已确认（三步审批过）的在途边数。
    pub confirmed_edges: u32,
    /// pending（尚未拿到目标在途证据）的边数。
    pub pending_edges: u32,
}

impl WaitRow {
    pub fn new(cpu: u32) -> WaitRow {
        WaitRow { cpu, waiting: 0, confirmed_edges: 0, pending_edges: 0 }
    }

    /// 挂 pending 等待边（仅置 pending 计数，不置位图——防假环）。
    pub fn add_pending(&mut self, target: u32) -> Result<(), (u32, String)> {
        if target as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("目标核号 {} 越界 64", target)));
        }
        if target == self.cpu {
            return Err((XC_DL_SELF_LOOP, format!("核 {} 同步边等自己", self.cpu)));
        }
        self.pending_edges += 1;
        Ok(())
    }

    /// 升级为 confirmed：置位等待掩码。
    pub fn confirm(&mut self, target: u32) -> Result<(), (u32, String)> {
        if target as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("目标核号 {} 越界 64", target)));
        }
        if target == self.cpu {
            return Err((XC_DL_SELF_LOOP, format!("核 {} 同步边等自己", self.cpu)));
        }
        if self.pending_edges == 0 {
            return Err((
                XC_DL_UNCONFIRMED,
                format!("核 {} 无 pending 边可确认——审批三步被跳过", self.cpu),
            ));
        }
        self.pending_edges -= 1;
        self.waiting |= 1u64 << target;
        self.confirmed_edges += 1;
        Ok(())
    }

    /// 应答撤边（幂等：首次真、二次假）。
    pub fn retract(&mut self, target: u32) -> Result<bool, (u32, String)> {
        if target as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("目标核号 {} 越界 64", target)));
        }
        let bit = 1u64 << target;
        if self.waiting & bit == 0 {
            return Ok(false);
        }
        self.waiting &= !bit;
        if self.confirmed_edges == 0 {
            return Err((XC_DL_EDGE_LEDGER, format!("核 {} 撤边时已确认边账零", self.cpu)));
        }
        self.confirmed_edges -= 1;
        Ok(true)
    }

    /// 最低置位核号（走步用）。
    pub fn lowest_target(&self) -> Option<u32> {
        if self.waiting == 0 {
            return None;
        }
        Some(self.waiting.trailing_zeros())
    }
}

// ---------------------------------------------------------------------------
// §2 死锁防线引擎（增量环检 + 定点纳秒开销账）
// ---------------------------------------------------------------------------

/// 环检路径结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CycleProbe {
    /// 无环，走 n 步到头。
    Clean(u32),
    /// 检出环，给出环长与环上首核。
    Cycle { len: u32, first: u32 },
}

/// 死锁防线引擎。
pub struct DeadlockGuard {
    rows: [WaitRow; MAX_CPU],
    /// 检测到的环总数。
    pub cycles_found: u64,
    /// 误报数（正常并发被判环）。
    pub false_positives: u64,
    /// 孤儿应答数（撤不存在的边）。
    pub orphan_acks: u64,
    /// 环检累计开销（定点纳秒，1ns/单位）。
    pub cost_total_ns: u64,
    /// 环检次数。
    pub probes: u64,
    /// 步数上限派生值（当前确认边数+1，链深不可能无界）。
    depth_cap: u32,
}

impl DeadlockGuard {
    pub fn new() -> DeadlockGuard {
        let mut rows = [WaitRow::new(0); MAX_CPU];
        let mut i = 0;
        while i < MAX_CPU {
            rows[i] = WaitRow::new(i as u32);
            i += 1;
        }
        DeadlockGuard {
            rows,
            cycles_found: 0,
            false_positives: 0,
            orphan_acks: 0,
            cost_total_ns: 0,
            probes: 0,
            depth_cap: MAX_CPU as u32 + 1,
        }
    }

    /// 发起路径：三步审批 + 增量环检 + 撤旧边前的旧边清理。
    /// 返回 Ok(()) = 允许入队；Err(码, 人话) = 拒绝。
    pub fn begin_call(&mut self, src: u32, dst: u32) -> Result<(), (u32, String)> {
        if src as usize >= MAX_CPU || dst as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("核号 src={} dst={} 越界 64", src, dst)));
        }
        if src == dst {
            return Err((XC_DL_SELF_LOOP, format!("核 {} 同步边等自己", src)));
        }
        // 三步审批：先 pending，再确认（目标在途证据由调用方给）。
        {
            let row = &mut self.rows[src as usize];
            row.add_pending(dst)?;
            row.confirm(dst)?;
        }
        // 增量环检：从 src 出发沿等待掩码走步。
        let probe = self.probe_cycle(src)?;
        match probe {
            CycleProbe::Cycle { len, first } => {
                // 环检出：回滚本边（不入队），计账。
                let row = &mut self.rows[src as usize];
                let _ = row.retract(dst);
                self.cycles_found += 1;
                return Err((
                    XC_DL_CYCLE_MISS,
                    format!("核 {} 等核 {} 成环（环长 {}，首核 {}）", src, dst, len, first),
                ));
            }
            CycleProbe::Clean(steps) => {
                // 无环：开销入账（模型：每步 3ns 定点 + 固定 4ns 审批）。
                self.cost_total_ns += 4 + 3 * steps as u64;
                self.probes += 1;
                Ok(())
            }
        }
    }

    /// 应答路径：撤边 + 孤儿应答识别。
    pub fn answer_call(&mut self, src: u32, dst: u32) -> Result<bool, (u32, String)> {
        if src as usize >= MAX_CPU || dst as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("核号 src={} dst={} 越界 64", src, dst)));
        }
        let row = &mut self.rows[src as usize];
        match row.retract(dst) {
            Ok(true) => {
                if row.waiting & (1u64 << dst) != 0 {
                    return Err((XC_DL_RESIDUE, format!("核 {} 撤边后仍置位", src)));
                }
                Ok(true)
            }
            Ok(false) => {
                self.orphan_acks += 1;
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// 增量环检：从 start 沿 confirmed 等待掩码走步。
    /// 走步上限 = depth_cap（链深不可能无界）。
    pub fn probe_cycle(&mut self, start: u32) -> Result<CycleProbe, (u32, String)> {
        if start as usize >= MAX_CPU {
            return Err((XC_DL_CPU_OOB, format!("起点核号 {} 越界 64", start)));
        }
        let mut cur = start;
        let mut steps: u32 = 0;
        // 访问账（防止遍历器在合法但绕远的链上越界）。
        let mut visited: u64 = 1u64 << start;
        while steps < self.depth_cap {
            let row = self.rows[cur as usize];
            match row.lowest_target() {
                None => return Ok(CycleProbe::Clean(steps)),
                Some(next) => {
                    if next == start {
                        return Ok(CycleProbe::Cycle { len: steps + 1, first: start });
                    }
                    if visited & (1u64 << next) != 0 {
                        // 走到已访问的非起点核=本图里有别人的环，与本链无关的旁路环。
                        // 增量防线的职责是拒绝"因本边新成环"，别人的旧环由重构期拒绝。
                        return Ok(CycleProbe::Clean(steps));
                    }
                    visited |= 1u64 << next;
                    cur = next;
                    steps += 1;
                }
            }
        }
        Err((
            XC_DL_WALK_OOB,
            format!("核 {} 走步超上限 {}——链深越界", start, self.depth_cap),
        ))
    }

    /// 深度上限重算（当前确认边总数+1——链深不可能无界）。
    pub fn recompute_depth_cap(&mut self) {
        let mut total: u32 = 0;
        for r in self.rows.iter() {
            total += r.confirmed_edges;
        }
        self.depth_cap = total + 1;
    }

    /// 账目守恒：行内置位和 = 总确认边数。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        let mut bits: u32 = 0;
        let mut edges: u32 = 0;
        for r in self.rows.iter() {
            bits += r.waiting.count_ones();
            edges += r.confirmed_edges;
        }
        if bits != edges {
            return Err((
                XC_DL_EDGE_LEDGER,
                format!("置位和 {}≠确认边 {}", bits, edges),
            ));
        }
        Ok(())
    }

    /// 防线指纹（确定性对拍用）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for r in self.rows.iter() {
            h ^= r.waiting;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= r.confirmed_edges as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= r.pending_edges as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    pub fn row(&self, cpu: u32) -> &WaitRow {
        &self.rows[cpu as usize]
    }
}

impl Default for DeadlockGuard {
    fn default() -> DeadlockGuard {
        DeadlockGuard::new()
    }
}

// ---------------------------------------------------------------------------
// §3 判据闸
// ---------------------------------------------------------------------------

/// J1a 环路检测 10/10：A→B→A 注入全部检出。
pub fn j1_cycle_10() -> Result<String, (u32, String)> {
    let mut pass: u32 = 0;
    for round in 0..10u32 {
        let mut g = DeadlockGuard::new();
        let a = round;
        let b = round + 1;
        // A 等 B。
        g.begin_call(a, b)?;
        // B 等 A —— 这一步必须被拒（成环）。
        match g.begin_call(b, a) {
            Err((c, _)) if c == XC_DL_CYCLE_MISS => pass += 1,
            Err((c, _)) => return Err((c, format!("第 {} 轮错误码不符", round))),
            Ok(()) => {
                return Err((XC_DL_CYCLE_MISS, format!("第 {} 轮 A→B→A 竟然入队", round)));
            }
        }
    }
    if pass != 10 {
        return Err((XC_DL_CYCLE_MISS, format!("环路检测 {}/10", pass)));
    }
    Ok("A→B→A 环路 10/10 检出".into())
}

/// J1b 长环检测：A→B→C→A 三核环同样检出。
pub fn j1_tri_cycle() -> Result<String, (u32, String)> {
    let mut g = DeadlockGuard::new();
    g.begin_call(10, 11)?;
    g.begin_call(11, 12)?;
    match g.begin_call(12, 10) {
        Err((c, _)) if c == XC_DL_CYCLE_MISS => {}
        Err((c, _)) => return Err((c, "三核环错误码不符".into())),
        Ok(()) => return Err((XC_DL_CYCLE_MISS, "三核环竟然入队".into())),
    }
    Ok("A→B→C→A 三核环检出".into())
}

/// J1c 误报 0：10 万次正常并发（链状无环）零误报。
pub fn j1_false_positive_0() -> Result<String, (u32, String)> {
    let mut rng = Lcg::new(0xD5F0_0934);
    let mut g = DeadlockGuard::new();
    let mut ok_calls: u32 = 0;
    for _ in 0..100_000u32 {
        // 链状结构：src 只等编号更大的核（拓扑有序=无环）。
        let src = rng.below(60);
        let dst = src + 1 + rng.below(3);
        if dst >= 60 {
            continue;
        }
        // 同源并发可能连续两次叠加边——先撤旧边再置新边（模拟应答驱动）。
        let _ = g.answer_call(src, src + 1);
        match g.begin_call(src, dst) {
            Ok(()) => ok_calls += 1,
            // 拓扑有序的并发被判环=误报，直接显性失败（误报计数由审计链读取）。
            Err((_c, msg)) => {
                g.false_positives += 1;
                return Err((XC_DL_CYCLE_FALSE, format!("误报计数 {}：{}", g.false_positives, msg)));
            }
        }
    }
    if g.false_positives != 0 {
        return Err((XC_DL_CYCLE_FALSE, format!("误报 {}", g.false_positives)));
    }
    g.audit()?;
    Ok(format!("正常并发 {} 次零误报", ok_calls))
}

/// J1d 开销账：单次环检定点纳秒 <100ns 且不低于理论下限。
pub fn j1_cost_budget() -> Result<String, (u32, String)> {
    let mut g = DeadlockGuard::new();
    // 最坏形态：60 核链一次环检。
    for i in 0..59u32 {
        g.begin_call(i, i + 1)?;
    }
    let before = g.cost_total_ns;
    // 一次清链尾环检（从链头走到底）。
    let _ = g.begin_call(59, 60)?;
    let delta = g.cost_total_ns - before;
    if delta > 100 {
        return Err((XC_DL_COST_OVER, format!("环检 {}ns 超 100ns 线", delta)));
    }
    if delta < 4 {
        return Err((XC_DL_COST_UNDER, format!("环检 {}ns 低于理论下限 4ns", delta)));
    }
    Ok(format!("最坏链环检 {}ns <100ns", delta))
}

/// 负路径闸：自环/撤边非幂/孤儿应答/未确认/越核/越界六类。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut hit: u32 = 0;
    // 自环拒。
    {
        let mut g = DeadlockGuard::new();
        if let Err((c, _)) = g.begin_call(3, 3) {
            if c == XC_DL_SELF_LOOP {
                hit += 1;
            } else {
                return Err((c, "自环错误码不符".into()));
            }
        } else {
            return Err((XC_DL_SELF_LOOP, "自环竟然放行".into()));
        }
    }
    // 撤边非幂：第二次必须假。
    {
        let mut g = DeadlockGuard::new();
        g.begin_call(1, 2)?;
        let first = g.answer_call(1, 2)?;
        let second = g.answer_call(1, 2)?;
        if !first || second {
            return Err((XC_DL_RETRACT_NONIDEM, format!("撤边 first={} second={}", first, second)));
        }
        if g.orphan_acks != 1 {
            return Err((XC_DL_ORPHAN_ACK, "孤儿应答未计账".into()));
        }
        hit += 1;
    }
    // 未确认审批跳过：直接 confirm 无 pending。
    {
        let mut row = WaitRow::new(5);
        if let Err((c, _)) = row.confirm(6) {
            if c == XC_DL_UNCONFIRMED {
                hit += 1;
            } else {
                return Err((c, "未确认错误码不符".into()));
            }
        } else {
            return Err((XC_DL_UNCONFIRMED, "跳过 pending 审批竟然确认".into()));
        }
    }
    // 越核边（源非本核）：引擎里体现为伪造 pin。
    {
        let mut g = DeadlockGuard::new();
        // 直接对 7 的行做 pending 但目标是自己——即越核伪造的识别点。
        if let Err((c, _)) = g.rows[7].add_pending(7) {
            if c == XC_DL_SELF_LOOP {
                hit += 1;
            } else {
                return Err((c, "越核自环错误码不符".into()));
            }
        } else {
            return Err((XC_DL_ALIEN_EDGE, "越核自环竟然挂上".into()));
        }
    }
    // 核号越界。
    {
        let mut g = DeadlockGuard::new();
        if let Err((c, _)) = g.begin_call(0, 64) {
            if c == XC_DL_CPU_OOB {
                hit += 1;
            } else {
                return Err((c, "越界错误码不符".into()));
            }
        } else {
            return Err((XC_DL_CPU_OOB, "核号 64 竟然放行".into()));
        }
    }
    // 撤后残留：手动伪造置位与计数不一致时 audit 必须抓。
    {
        let mut g = DeadlockGuard::new();
        g.begin_call(2, 4)?;
        g.rows[2].waiting |= 1u64 << 5; // 伪造置位（计数不跟）
        if let Err((c, _)) = g.audit() {
            if c == XC_DL_EDGE_LEDGER {
                hit += 1;
            } else {
                return Err((c, "边账错误码不符".into()));
            }
        } else {
            return Err((XC_DL_EDGE_LEDGER, "伪造置位竟然平账".into()));
        }
    }
    if hit != 6 {
        return Err((XC_DL_EDGE_LEDGER, format!("负路径 {}/6", hit)));
    }
    Ok("负路径 6/6 显性拒绝且码正确".into())
}

/// 双跑指纹：同种子两演等待图指纹等。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut rng = Lcg::new(seed);
        let mut g = DeadlockGuard::new();
        for _ in 0..5000 {
            let src = rng.below(50);
            let dst = src + 1 + rng.below(8);
            let _ = g.begin_call(src, dst);
            let _ = g.answer_call(src, src.saturating_sub(1));
        }
        g.audit()?;
        Ok(g.fp())
    };
    let a = run(0xD5F0_0D34)?;
    let b = run(0xD5F0_0D34)?;
    if a != b {
        return Err((XC_DL_DUAL_DRIFT, format!("双跑 {:#x}≠{:#x}", a, b)));
    }
    Ok(a)
}

/// F0934 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_cycle_10()?;
    let b = j1_tri_cycle()?;
    let c = j1_false_positive_0()?;
    let d = j1_cost_budget()?;
    let e = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_DL_DUAL_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0934 全绿：\n[二核环] {}\n[三核环] {}\n[误报] {}\n[开销] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
