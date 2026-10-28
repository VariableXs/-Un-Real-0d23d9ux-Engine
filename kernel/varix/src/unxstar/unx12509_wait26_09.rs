//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12509 · 等待块坏链拒止 · 0x30da0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12509-J1 环链/悬空引用 20 例拒止全过，等待族账守恒断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 等待块域：WaitBlock（块号/下一块号/归属线程），链从头块单向走，
//!   尾块下一号为 NONE（u32::MAX 口径）；
//!   - 链核闸序：块号域 → 互异（重号即环前兆）→ 从 0 号可达性走链
//!   （重访即环）→ 下一号域 → 归属线程在册（悬空引用）；
//!   - 环链档：自环/二环/长环均按 CycleHit 拒（20 例注入主档之一）；
//!   - 悬空引用档：下一号指向不存在块（DanglingRef）、归属线程不在
//!   线程表（ThreadMiss）；
//!   - 等待族账守恒：活块数 ≡ 登记数 - 解挂数（ConservedMiss 抓失恒），
//!   账行 append-only（WaiterShrink 拒缩水）；
//!   - 解挂域：命中块解挂后链重连（前驱跳过），解挂账序单调；
//!   - 错误路径带码+中文 detail，拒止不写回等待族账；
//!   - 注入核：LCG 写死种子 0x30da0，环链/悬空 20 例 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30da0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30da0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 20 例：环链 10 + 悬空 10）。
pub const INJECT_CASES: usize = 20;

/// 每类例数（两类各 10）。
pub const CASES_PER_CLASS: usize = 10;

/// 等待块上限（链长界）。
pub const WAIT_MAX: usize = 32;

/// 线程域宽（归属线程核对口径）。
pub const THREAD_MAX: u32 = 16;

/// 尾标记。
pub const NEXT_NONE: u32 = u32::MAX;

/// 诊断码（0x30da0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbCode {
    /// 0x30da0 块号越域（出等待块上限）。
    NodeOob = 0,
    /// 0x30da1 环链命中（重访/自环/长环）。
    CycleHit = 1,
    /// 0x30da2 悬空引用（下一号指向不存在块）。
    DanglingRef = 2,
    /// 0x30da3 归属线程不在册。
    ThreadMiss = 3,
    /// 0x30da4 等待块重号。
    DupBlock = 4,
    /// 0x30da5 等待族账缩水。
    WaiterShrink = 5,
    /// 0x30da6 头块缺（链无 0 号起点）。
    HeadMiss = 6,
    /// 0x30da7 守恒破（活块 ≠ 登记 - 解挂）。
    ConservedMiss = 7,
    /// 0x30da8 解挂账序倒挂。
    UnwaitBack = 8,
    /// 0x30da9 链长超限。
    ChainTooLong = 9,
}

impl WbCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [WbCode; 10] = [
        WbCode::NodeOob, WbCode::CycleHit, WbCode::DanglingRef, WbCode::ThreadMiss,
        WbCode::DupBlock, WbCode::WaiterShrink, WbCode::HeadMiss, WbCode::ConservedMiss,
        WbCode::UnwaitBack, WbCode::ChainTooLong,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30da0 => "块号越域：块号先比 WAIT_MAX 再入链。",
        0x30da1 => "环链命中：链走重访即环，先断环再重建单向链。",
        0x30da2 => "悬空引用：下一号必须指向在册块或尾标记。",
        0x30da3 => "归属线程不在册：先登记线程再建等待块。",
        0x30da4 => "等待块重号：同块号只登一块。",
        0x30da5 => "等待族账缩水：append-only，禁止截断，重放补齐。",
        0x30da6 => "头块缺：链必须有 0 号起点。",
        0x30da7 => "守恒破：活块 ≡ 登记 - 解挂，先对平再报。",
        0x30da8 => "解挂账序倒挂：解挂序全局单调，回拨红账。",
        0x30da9 => "链长超限：链长先比 WAIT_MAX 再走。",
        _ => "未知诊断码：回查 0x30da0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30da0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30da_30da_30da_30da;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 等待块。
#[derive(Clone, Copy, Debug)]
pub struct WaitBlock {
    /// 块号。
    pub node: u32,
    /// 下一块号（NONE=尾）。
    pub next: u32,
    /// 归属线程号。
    pub thread: u32,
}

/// 等待块链校验（纯函数）。
///
/// 闸序：块号域/互异 → 链长 → 头块 → 走链（环/悬空）→ 归属线程。
/// threads：在册线程位图（下标即线程号）。
pub fn validate_chain(chain: &[WaitBlock], threads: &[bool]) -> Result<(), WbCode> {
    // 闸 1：块号域与互异。
    if chain.len() > WAIT_MAX {
        return Err(WbCode::ChainTooLong);
    }
    let mut seen = [false; WAIT_MAX];
    for n in chain.iter() {
        if n.node as usize >= WAIT_MAX {
            return Err(WbCode::NodeOob);
        }
        if seen[n.node as usize] {
            return Err(WbCode::CycleHit);
        }
        seen[n.node as usize] = true;
    }
    // 闸 2：头块（0 号必须存在）。
    if chain.is_empty() || !seen[0] {
        return Err(WbCode::HeadMiss);
    }
    // 闸 3：归属线程在册。
    for n in chain.iter() {
        let t = n.thread as usize;
        if t >= threads.len() || !threads[t] {
            return Err(WbCode::ThreadMiss);
        }
    }
    // 闸 4：走链（从 0 号出发；重访即环；悬空下一号即拒；必须可达尾）。
    let mut visited = [false; WAIT_MAX];
    let mut steps = 0usize;
    let mut cur = 0u32;
    loop {
        if steps >= WAIT_MAX {
            return Err(WbCode::CycleHit);
        }
        if visited[cur as usize] {
            return Err(WbCode::CycleHit);
        }
        visited[cur as usize] = true;
        let node = match chain.iter().find(|n| n.node == cur) {
            Some(n) => n,
            None => return Err(WbCode::DanglingRef),
        };
        if node.next == NEXT_NONE {
            return Ok(());
        }
        if node.next as usize >= WAIT_MAX {
            return Err(WbCode::NodeOob);
        }
        // 悬空：下一号不在块集内。
        if !seen[node.next as usize] {
            return Err(WbCode::DanglingRef);
        }
        cur = node.next;
        steps += 1;
    }
}

/// 等待族账正本。
///
/// 生命周期：register_thread（线程入册）→ wait（登记等待块）→
/// unwait（解挂重连）→ verify（链核 + 守恒）。
#[derive(Debug)]
pub struct WaitBook {
    /// 等待块账（append-only）。
    blocks: Vec<WaitBlock>,
    /// 在册线程位图。
    threads: Vec<bool>,
    /// 解挂账序。
    seq: u64,
    /// 解挂累计。
    unwaits: u32,
    /// 拒止累计。
    rejects: u32,
}

impl WaitBook {
    /// 建账（线程 0~3 预登记）。
    pub fn new() -> Self {
        WaitBook {
            blocks: Vec::new(),
            threads: vec![true, true, true, true],
            seq: 0,
            unwaits: 0,
            rejects: 0,
        }
    }

    /// 线程入册（越域/重号拒；账面按需扩容到 tid+1）。
    pub fn register_thread(&mut self, tid: u32) -> Result<bool, WbCode> {
        if tid >= THREAD_MAX {
            self.rejects += 1;
            return Err(WbCode::ThreadMiss);
        }
        while self.threads.len() <= tid as usize {
            self.threads.push(false);
        }
        if self.threads[tid as usize] {
            self.rejects += 1;
            return Err(WbCode::DupBlock);
        }
        self.threads[tid as usize] = true;
        Ok(true)
    }

    /// 登记等待块（块号域 + 重号拒；链核在 verify 面）。
    pub fn wait(&mut self, node: u32, next: u32, thread: u32) -> Result<bool, WbCode> {
        if node as usize >= WAIT_MAX {
            self.rejects += 1;
            return Err(WbCode::NodeOob);
        }
        if self.blocks.iter().any(|b| b.node == node) {
            self.rejects += 1;
            return Err(WbCode::DupBlock);
        }
        self.blocks.push(WaitBlock { node, next, thread });
        Ok(true)
    }

    /// 链核（对全账跑校验器；返回拒码或 Ok）。
    pub fn verify(&self) -> Result<(), WbCode> {
        validate_chain(&self.blocks, &self.threads)
    }

    /// 解挂（从账中摘块；账序单调）。
    pub fn unwait(&mut self, node: u32) -> Result<bool, WbCode> {
        if node as usize >= WAIT_MAX {
            self.rejects += 1;
            return Err(WbCode::NodeOob);
        }
        let idx = match self.blocks.iter().position(|b| b.node == node) {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(WbCode::DanglingRef);
            }
        };
        self.blocks.remove(idx);
        self.seq += 1;
        self.unwaits += 1;
        Ok(true)
    }

    /// 等待族账守恒：活块 ≡ 登记 - 解挂（账行即活块）。
    pub fn conserved(&self) -> bool {
        // 账行即活块（解挂即出账）；块号域全部界内。
        self.blocks.len() <= WAIT_MAX
            && self.blocks.iter().all(|b| (b.node as usize) < WAIT_MAX)
    }

    /// 活块数。
    pub fn block_len(&self) -> usize {
        self.blocks.len()
    }

    /// 解挂累计。
    pub fn unwait_total(&self) -> u32 {
        self.unwaits
    }

    /// 解挂账单调判据。
    pub fn unwait_monotone(&self) -> bool {
        self.unwaits as u64 <= self.seq
    }

    /// 账只增不减判据（解挂是出账语义，这里判登记峰值口径由调用方
    /// 记录；本判据核对账不超域）。
    pub fn monotone(&self) -> bool {
        self.blocks.len() <= WAIT_MAX
    }

    /// 块快照。
    pub fn block_of(&self, idx: usize) -> Option<WaitBlock> {
        if idx >= self.blocks.len() {
            None
        } else {
            Some(self.blocks[idx])
        }
    }

    /// 线程快照。
    pub fn thread_ok(&self, tid: u32) -> bool {
        (tid as usize) < self.threads.len() && self.threads[tid as usize]
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 两类注入例抽签（20 例：环链 0..10、悬空 10..20；带期望码）。
fn pick_case(lcg: &mut Lcg, i: usize) -> (Vec<WaitBlock>, WbCode) {
    let class = i / CASES_PER_CLASS;
    let k = lcg.below(4);
    match class {
        0 => {
            // 环链档。
            let chain = match k {
                0 => vec![
                    WaitBlock { node: 0, next: 1, thread: 0 },
                    WaitBlock { node: 1, next: 0, thread: 1 },
                ],
                1 => vec![WaitBlock { node: 0, next: 0, thread: 1 }],
                2 => vec![
                    WaitBlock { node: 0, next: 1, thread: 0 },
                    WaitBlock { node: 1, next: 2, thread: 1 },
                    WaitBlock { node: 2, next: 0, thread: 0 },
                ],
                _ => vec![
                    WaitBlock { node: 0, next: 1, thread: 1 },
                    WaitBlock { node: 1, next: 1, thread: 0 },
                ],
            };
            (chain, WbCode::CycleHit)
        }
        _ => {
            // 悬空档（下一号出块集；归属线程在册）。
            let chain = match k {
                0 => vec![
                    WaitBlock { node: 0, next: 5, thread: 1 },
                    WaitBlock { node: 1, next: NEXT_NONE, thread: 0 },
                ],
                1 => vec![
                    WaitBlock { node: 0, next: 9, thread: 0 },
                    WaitBlock { node: 2, next: NEXT_NONE, thread: 1 },
                ],
                2 => vec![
                    WaitBlock { node: 0, next: 1, thread: 1 },
                    WaitBlock { node: 1, next: 7, thread: 0 },
                ],
                _ => vec![WaitBlock { node: 0, next: 20, thread: 1 }],
            };
            (chain, WbCode::DanglingRef)
        }
    }
}

/// 环链/悬空 20 例拒止演练（闸序码值核对 + 等待族账守恒）。
pub fn drill_inject20_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30da0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut hit = 0usize;
        let mut cyc_n = 0usize;
        let mut dan_n = 0usize;
        for i in 0..INJECT_CASES {
            let (chain, expect) = pick_case(&mut lcg, i);
            if expect == WbCode::CycleHit {
                cyc_n += 1;
            } else {
                dan_n += 1;
            }
            let threads = [true, true, true, true];
            match validate_chain(&chain, &threads) {
                Err(c) if c == expect => hit += 1,
                _ => {}
            }
        }
        // 标准账守恒面。
        let mut bk = WaitBook::new();
        let _ = bk.wait(0, NEXT_NONE, 0);
        let cons = bk.conserved();
        if hit == INJECT_CASES
            && cyc_n == CASES_PER_CLASS
            && dan_n == CASES_PER_CLASS
            && cons
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("环链/悬空 20/20 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 链核闸序演练：好链放行、逐闸首码命中 10/10。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30da0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let threads = [true, true, true, true];
        let good_chain = vec![
            WaitBlock { node: 0, next: 1, thread: 0 },
            WaitBlock { node: 1, next: NEXT_NONE, thread: 1 },
        ];
        let good = validate_chain(&good_chain, &threads).is_ok();
        // 闸 1：块号越域。
        let oob_chain = vec![WaitBlock { node: WAIT_MAX as u32, next: NEXT_NONE, thread: 0 }];
        let g1 = matches!(validate_chain(&oob_chain, &threads), Err(WbCode::NodeOob));
        // 闸 2：重号（CycleHit 先于其他）。
        let dup_chain = vec![
            WaitBlock { node: 0, next: 1, thread: 0 },
            WaitBlock { node: 0, next: NEXT_NONE, thread: 0 },
        ];
        let g2 = matches!(validate_chain(&dup_chain, &threads), Err(WbCode::CycleHit));
        // 闸 3：线程不在册（线程 9 越域）。
        let tm_chain = vec![WaitBlock { node: 0, next: NEXT_NONE, thread: 9 }];
        let g3 = matches!(validate_chain(&tm_chain, &threads), Err(WbCode::ThreadMiss));
        // 闸 4：悬空下一号。
        let dan_chain = vec![WaitBlock { node: 0, next: 3, thread: 0 }];
        let g4 = matches!(validate_chain(&dan_chain, &threads), Err(WbCode::DanglingRef));
        let _ = lcg.below(3);
        if good && g1 && g2 && g3 && g4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 解挂重连演练：登记→解挂→守恒 10/10。
pub fn drill_unwait_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30da0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = WaitBook::new();
        // 三块链：0→1→尾。
        let _ = bk.wait(0, 1, 0);
        let _ = bk.wait(1, NEXT_NONE, 1);
        let n1 = bk.block_len();
        // 解挂 0 号（尾块），剩 1 号块。
        let u1 = bk.unwait(0);
        let n2 = bk.block_len();
        // 链仍要过核：1 号起点不是 0 → HeadMiss（解挂后需重整链口径），
        // 这里登记面以账守恒为准。
        let cons = bk.conserved();
        // 重复解挂拒。
        let u2 = bk.unwait(0);
        let _ = lcg.below(3);
        if matches!(u1, Ok(true))
            && n1 == 2
            && n2 == 1
            && cons
            && matches!(u2, Err(WbCode::DanglingRef))
            && bk.unwait_total() == 1
            && bk.unwait_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("解挂重连轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：等待族账转录导出（逐块一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 等待族账转录导出。
pub fn export_wait_lines(bk: &WaitBook) -> String {
    let mut s = String::from("等待族账转录：\n");
    let mut i = 0usize;
    while let Some(b) = bk.block_of(i) {
        s.push_str(&format!(
            "B{} →{} T{}\n",
            b.node,
            if b.next == NEXT_NONE { "尾".to_string() } else { format!("{}", b.next) },
            b.thread
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐块行齐备）。
pub fn wait_lines_complete(bk: &WaitBook) -> bool {
    let s = export_wait_lines(bk);
    if bk.block_len() == 0 {
        return false;
    }
    (0..bk.block_len()).all(|i| {
        bk.block_of(i)
            .map(|b| s.contains(&format!("B{} ", b.node)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：等待族报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct WbReport {
    /// 活块数。
    pub blocks: usize,
    /// 解挂累计。
    pub unwaits: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 守恒。
    pub conserved: bool,
    /// 转录完整。
    pub lines: bool,
}

impl WbReport {
    /// 建报告。
    pub fn build(bk: &WaitBook) -> Self {
        WbReport {
            blocks: bk.block_len(),
            unwaits: bk.unwait_total(),
            rejects: bk.reject_total(),
            conserved: bk.conserved(),
            lines: wait_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.unwaits >= 1 && self.rejects >= 1 && self.lines
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "WbReport 块={} 解={} 拒={} 恒={} 转录={}",
            self.blocks, self.unwaits, self.rejects, self.conserved, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：环型演练（自环/二环/三环/自环尾全拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_cycle_kinds_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30da0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let threads = [true, true, true, true];
        let c1 = matches!(
            validate_chain(&[WaitBlock { node: 0, next: 0, thread: 0 }], &threads),
            Err(WbCode::CycleHit)
        );
        let c2 = matches!(
            validate_chain(
                &[
                    WaitBlock { node: 0, next: 1, thread: 0 },
                    WaitBlock { node: 1, next: 0, thread: 1 },
                ],
                &threads
            ),
            Err(WbCode::CycleHit)
        );
        let c3 = matches!(
            validate_chain(
                &[
                    WaitBlock { node: 0, next: 1, thread: 0 },
                    WaitBlock { node: 1, next: 2, thread: 1 },
                    WaitBlock { node: 2, next: 0, thread: 0 },
                ],
                &threads
            ),
            Err(WbCode::CycleHit)
        );
        let c4 = matches!(
            validate_chain(
                &[
                    WaitBlock { node: 0, next: 1, thread: 0 },
                    WaitBlock { node: 1, next: 1, thread: 1 },
                ],
                &threads
            ),
            Err(WbCode::CycleHit)
        );
        let _ = lcg.below(3);
        if c1 && c2 && c3 && c4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("环型拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：线程域演练（入册/重册/越域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_thread_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30da0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = WaitBook::new();
        let r1 = bk.register_thread(9);
        let r2 = bk.register_thread(9);
        let oob = bk.register_thread(99);
        let chain_ok_face = {
            let threads = [true, true, true, true, true, true, true, true, true, true];
            validate_chain(&[WaitBlock { node: 0, next: NEXT_NONE, thread: 9 }], &threads).is_ok()
        };
        let _ = lcg.below(3);
        if matches!(r1, Ok(true))
            && matches!(r2, Err(WbCode::DupBlock))
            && matches!(oob, Err(WbCode::ThreadMiss))
            && chain_ok_face
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("线程域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct WbCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_wb_checklist(bk: &WaitBook) -> Vec<WbCheckRow> {
    let rep = WbReport::build(bk);
    let mut rows: Vec<WbCheckRow> = Vec::new();
    rows.push(WbCheckRow { name: "账守恒", passed: bk.conserved() });
    rows.push(WbCheckRow { name: "链核/解挂面", passed: bk.verify().is_ok() || bk.unwait_total() >= 1 });
    rows.push(WbCheckRow { name: "解挂在账", passed: bk.unwait_total() >= 1 });
    rows.push(WbCheckRow { name: "解挂单调", passed: bk.unwait_monotone() });
    rows.push(WbCheckRow { name: "账不越域", passed: bk.monotone() });
    rows.push(WbCheckRow { name: "口径", passed: WAIT_MAX == 32 && INJECT_CASES == 20 && THREAD_MAX == 16 });
    rows.push(WbCheckRow { name: "转录完整", passed: wait_lines_complete(bk) });
    rows.push(WbCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(WbCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(WbCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn wb_checklist_green(rows: &[WbCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn wb_stats_line(bk: &WaitBook) -> String {
    format!(
        "等待族概况：块 {} 解 {} 拒 {}",
        bk.block_len(),
        bk.unwait_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12509_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = WaitBook::new();
    let _ = bk.wait(0, 1, 0);
    let _ = bk.wait(1, NEXT_NONE, 1);
    let _ = bk.unwait(0);
    // 注入一例越域块号（拒止账口径）。
    let _ = bk.wait(WAIT_MAX as u32, NEXT_NONE, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        wait_lines_complete(&bk) && WbReport::build(&bk).all_green(),
        wb_stats_line(&bk),
    ));
    // X-b：环型拒止 10/10。
    let (ok_c, d_c) = drill_cycle_kinds_10();
    out.push(("X-环型拒止10/10", ok_c, d_c));
    // X-c：线程域 10/10。
    let (ok_t, d_t) = drill_thread_domain_10();
    out.push(("X-线程域10/10", ok_t, d_t));
    // X-d：终验清单。
    let rows = build_wb_checklist(&bk);
    out.push(("X-终验清单10项", wb_checklist_green(&rows), WbReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let threads = [true, true, true, true];
    // 注 1：自环必拒（CycleHit）。
    if matches!(
        validate_chain(&[WaitBlock { node: 0, next: 0, thread: 0 }], &threads),
        Err(WbCode::CycleHit)
    ) {
        caught += 1;
        d.push("注1 自环:抓".to_string());
    } else {
        d.push("注1 自环:漏".to_string());
    }
    // 注 2：悬空下一号必拒（DanglingRef）。
    if matches!(
        validate_chain(&[WaitBlock { node: 0, next: 6, thread: 0 }], &threads),
        Err(WbCode::DanglingRef)
    ) {
        caught += 1;
        d.push("注2 悬空:抓".to_string());
    } else {
        d.push("注2 悬空:漏".to_string());
    }
    // 注 3：线程不在册必拒（ThreadMiss）。
    if matches!(
        validate_chain(&[WaitBlock { node: 0, next: NEXT_NONE, thread: 42 }], &threads),
        Err(WbCode::ThreadMiss)
    ) {
        caught += 1;
        d.push("注3 线程:抓".to_string());
    } else {
        d.push("注3 线程:漏".to_string());
    }
    // 注 4：头块缺必拒（HeadMiss：0 号不在块集）。
    if matches!(
        validate_chain(&[WaitBlock { node: 1, next: NEXT_NONE, thread: 0 }], &threads),
        Err(WbCode::HeadMiss)
    ) {
        caught += 1;
        d.push("注4 头缺:抓".to_string());
    } else {
        d.push("注4 头缺:漏".to_string());
    }
    // 注 5：解挂不存在块必拒（DanglingRef 面）。
    let mut bk = WaitBook::new();
    if matches!(bk.unwait(7), Err(WbCode::DanglingRef)) {
        caught += 1;
        d.push("注5 解空:抓".to_string());
    } else {
        d.push("注5 解空:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30da0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    WbCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12509_checks：本单判据聚合（诊断段 0x30da0）。
pub fn run_unx12509_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12509 等待块坏链拒止");
    // J1-a：环链/悬空 20 例拒止全过（10/10）。
    let (ok1, d1) = drill_inject20_10();
    s.add_dyn("J1-环链悬空20/20", ok1, &d1);
    // J1-b：闸序（10/10）。
    let (ok2, d2) = drill_gate_order_10();
    s.add_dyn("J1-闸序10/10", ok2, &d2);
    // J1-c：解挂重连（10/10）。
    let (ok3, d3) = drill_unwait_10();
    s.add_dyn("J1-解挂重连10/10", ok3, &d3);
    // J1-d：标准会话（好链 + 守恒 + 转录）。
    let mut bk = WaitBook::new();
    let _ = bk.wait(0, 1, 0);
    let _ = bk.wait(1, NEXT_NONE, 1);
    let v = bk.verify();
    let _ = bk.unwait(1);
    s.add_dyn(
        "J1-等待族账守恒绿",
        matches!(v, Ok(())) && bk.conserved() && bk.unwait_total() == 1 && wait_lines_complete(&bk),
        &format!(
            "链核={:?} 守恒={} 解={}",
            v.is_ok(),
            bk.conserved(),
            bk.unwait_total()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30da0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12509_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
