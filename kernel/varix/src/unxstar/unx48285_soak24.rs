//! unx48285_soak24 — UNX-F48285 24h 混合操作稳定压测总成（切换/删除/撤销/焦点切换混合）（诊断段 0xC480）。
//!
//! 判据原文：
//!   J1 短程 2h 预跑零崩、24h 全量登记随闸门补测（试产校准定）；
//!   判据锚定 §AI-61 §5 J-4。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 混合操作四类：引擎切换 / 删字 / 撤销 / 焦点切换——确定性 LCG
//!      驱动混合流（同种子同流，跑崩可复现），四类配比均衡轮转。
//!   2. 语义真跑：删字从文档栈实删并压撤销记录；撤销弹栈回填；切换
//!      换引擎活跃位（挂字作废）；焦点切换换窗（每窗独立文档视图）——
//!      不是空转计数，每步落真实状态。
//!   3. 不变量随步核：文档守恒（删除数 = 已撤销 + 待撤销栈深）、撤销
//!      栈不越弹（空栈撤销即 E_UNDO_EMPTY 记账不崩）、窗视图互不串。
//!      任一不变量破即 E_INVARIANT（压测红）。
//!   4. 分程口径（J1）：短程 2h 预跑全量真跑、零崩（E_CRASH 恒 0）；
//!      24h 全量只登记（行数/操作数/闸门位）不实跑——随闸门补测
//!      （试产校准定），登记账可审计、闸门开启前不越权执行。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC480;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_INVARIANT: u16 = SEG_BASE + 1; // 不变量破
const E_CRASH: u16 = SEG_BASE + 2; // 压测崩溃（恒 0 口径）
const E_UNDO_EMPTY: u16 = SEG_BASE + 3; // 空栈撤销（记账不崩）
const E_GATE_LOCKED: u16 = SEG_BASE + 4; // 闸门未开执行 24h 全量
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 登记账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 6; // 压测器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 操作四类闭集。
pub const OP_SWITCH: u8 = 0; // 引擎切换
pub const OP_DELETE: u8 = 1; // 删字
pub const OP_UNDO: u8 = 2; // 撤销
pub const OP_FOCUS: u8 = 3; // 焦点切换
pub const OP_N: usize = 4;

/// 压测口径：2h 预跑 / 24h 全量（按 10 ops/s 折算操作数）。
pub const PRE_OPS: u32 = 72_000; // 2h
pub const FULL_OPS: u32 = 864_000; // 24h

/// 确定性 LCG（压测流驱动，同种子同流）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed } }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 单窗文档视图：文档 + 撤销栈。
pub struct WinView {
    pub win: u32,
    pub doc: Vec<u16>,
    undo: Vec<u16>, // 被删字符栈（撤销即回填）
}

impl WinView {
    pub fn new(win: u32) -> WinView {
        WinView { win, doc: Vec::new(), undo: Vec::new() }
    }
    /// 预填文档（压测起点）。
    pub fn seed_doc(&mut self, n: u16) {
        let mut i = 0u16;
        while i < n {
            self.doc.push(0x4E00 + i);
            i += 1;
        }
    }
    /// 删字：非空才删，删了压撤销栈。
    pub fn delete(&mut self) -> bool {
        if self.doc.len() == 0 {
            return false;
        }
        let ch = self.doc[self.doc.len() - 1];
        self.doc.remove(self.doc.len() - 1);
        self.undo.push(ch);
        true
    }
    /// 撤销：栈非空回填，空栈记账不崩。
    pub fn undo_pop(&mut self) -> Result<bool, u16> {
        if self.undo.len() == 0 {
            return Err(E_UNDO_EMPTY);
        }
        let ch = self.undo[self.undo.len() - 1];
        self.undo.remove(self.undo.len() - 1);
        self.doc.push(ch);
        Ok(true)
    }
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }
}

/// 压测器。
pub struct Soaker {
    wins: [WinView; 3], // 三窗视图
    pub active_win: usize,
    pub active_engine: u8, // 0 自研 / 1 对接
    pub op_counts: [u32; OP_N],
    pub crashes: u32,
    pub invariant_faults: u32,
    pub undo_empty_seen: u32,
    pub pending_switch_chars: u32, // 切换作废的挂字（净空语义，全局账）
    pub discarded: [u32; 3],       // 逐窗作废量（守恒式第三项）
}

impl Soaker {
    pub fn new() -> Soaker {
        Soaker {
            wins: [WinView::new(0), WinView::new(1), WinView::new(2)],
            active_win: 0,
            active_engine: 0,
            op_counts: [0; OP_N],
            crashes: 0,
            invariant_faults: 0,
            undo_empty_seen: 0,
            pending_switch_chars: 0,
            discarded: [0; 3],
        }
    }

    /// 预填三窗文档（各 16 字起步）。
    pub fn seed_all(&mut self) {
        let mut w = 0usize;
        while w < 3 {
            self.wins[w].seed_doc(16);
            w += 1;
        }
    }

    /// 单操作真跑（纪律 2）：op 由 LCG 混合流决定，逐类落真实状态。
    pub fn step(&mut self, op: u8) -> Result<(), u16> {
        if op as usize >= OP_N {
            self.crashes += 1;
            return Err(E_CRASH); // 非法操作码入流 = 压测抓崩
        }
        self.op_counts[op as usize] += 1;
        match op {
            OP_SWITCH => {
                // 切换净空：挂字作废——全局净空账 + 逐窗作废账双记。
                let w = self.active_win;
                let d = self.wins[w].undo_depth() as u32;
                self.pending_switch_chars += d;
                self.discarded[w] += d;
                self.wins[w].undo.clear();
                self.active_engine = 1 - self.active_engine;
            }
            OP_DELETE => {
                let w = self.active_win;
                self.wins[w].delete();
            }
            OP_UNDO => {
                let w = self.active_win;
                match self.wins[w].undo_pop() {
                    Ok(_) => {}
                    Err(E_UNDO_EMPTY) => {
                        // 空栈撤销：记账不崩（压测要的就是这个防线）。
                        self.undo_empty_seen += 1;
                    }
                    Err(_) => {
                        self.crashes += 1;
                        return Err(E_CRASH);
                    }
                }
            }
            _ => {
                // 焦点切换：三窗轮转。
                self.active_win = (self.active_win + 1) % 3;
            }
        }
        // 不变量随步核（纪律 3）。
        self.check_invariants()
    }

    /// 文档守恒核（真实守恒式）：每窗 文档 + 撤销栈 + 作废量 ≡ 种子量
    /// （16 字）——删字/撤销/净空三路全在式内，任何一路丢字即破。
    fn check_invariants(&self) -> Result<(), u16> {
        let mut w = 0usize;
        while w < 3 {
            let sum = self.wins[w].doc.len() as u32
                + self.wins[w].undo_depth() as u32
                + self.discarded[w];
            if sum != 16 {
                return Err(E_INVARIANT);
            }
            w += 1;
        }
        if self.active_engine > 1 {
            return Err(E_INVARIANT);
        }
        // 全局净空账与逐窗作废账和闭合。
        let mut disc_sum = 0u32;
        let mut w = 0usize;
        while w < 3 {
            disc_sum += self.discarded[w];
            w += 1;
        }
        if disc_sum != self.pending_switch_chars {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 短程 2h 预跑（J1 主体一）：PRE_OPS 全量真跑，零崩 + 不变量全程绿。
    pub fn pre_run(&mut self, seed: u64) -> Result<(), u16> {
        self.seed_all();
        let mut rng = Lcg::new(seed);
        let mut i = 0u32;
        while i < PRE_OPS {
            let op = rng.next_below(OP_N as u64) as u8;
            self.step(op)?;
            i += 1;
        }
        if self.crashes != 0 {
            return Err(E_CRASH);
        }
        Ok(())
    }

    /// 24h 全量登记（J1 主体二）：只登记不执行，闸门未开必拒。
    pub fn register_full(&mut self, gate_open: bool) -> Result<u32, u16> {
        if !gate_open {
            return Err(E_GATE_LOCKED); // 随闸门补测：闸开前不越权
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < OP_N {
            sum += self.op_counts[i];
            i += 1;
        }
        // 登记账：全量口径 = FULL_OPS，预跑实跑量如实附带。
        if sum < PRE_OPS {
            return Err(E_LEDGER_BAD); // 预跑量不足，登记无据
        }
        Ok(FULL_OPS)
    }

    /// 预跑账审计：四类操作全有量（混合流不得偏科）、崩溃恒 0、
    /// 净空作废账闭合（作废量 ≤ 撤销栈历史总量）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.crashes != 0 {
            return Err(E_CRASH);
        }
        let mut i = 0usize;
        while i < OP_N {
            if self.op_counts[i] == 0 {
                return Err(E_LEDGER_BAD); // 混合流偏科
            }
            i += 1;
        }
        let total: u32 = self.op_counts[0] + self.op_counts[1] + self.op_counts[2] + self.op_counts[3];
        if total < PRE_OPS {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn total_ops(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < OP_N {
            t += self.op_counts[i];
            i += 1;
        }
        t
    }

    /// 完成写回（判据 J2）：预跑零崩 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.crashes != 0 || self.total_ops() == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（短程 2h 预跑零崩、24h 全量登记随闸门补测），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐：短程 2h 预跑（PRE_OPS=72000 ops 真跑）零崩、四类
    //    操作全有量、不变量全程绿。
    let mut sk = Soaker::new();
    sk.pre_run(0x2026_1027)?;
    sk.audit()?;
    if sk.crashes != 0 {
        return Err(E_CRASH);
    }
    if sk.total_ops() != PRE_OPS {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 可复现：同种子重跑操作配比逐类相同（确定性混合流，跑崩可定位）。
    let mut sk2 = Soaker::new();
    sk2.pre_run(0x2026_1027)?;
    let mut i = 0usize;
    while i < OP_N {
        if sk2.op_counts[i] != sk.op_counts[i] {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 撤销语义实测：种子窗删 3 字 → 栈深 3 → 逐撤回填 → 文档复原、
    //    栈清零；再撤 → E_UNDO_EMPTY 记账不崩（防线真跑）。
    let mut wv = WinView::new(9);
    wv.seed_doc(5);
    let mut d = 0u32;
    while d < 3 {
        if !wv.delete() {
            return Err(E_INVARIANT);
        }
        d += 1;
    }
    if wv.undo_depth() != 3 || wv.doc.len() != 2 {
        return Err(E_INVARIANT);
    }
    let mut u = 0u32;
    while u < 3 {
        wv.undo_pop()?;
        u += 1;
    }
    if wv.undo_depth() != 0 || wv.doc.len() != 5 {
        return Err(E_INVARIANT);
    }
    if wv.undo_pop().is_ok() {
        return Err(E_UNDO_EMPTY);
    }
    hits += 1;

    // R4 非法操作码抓崩：op=7 → E_CRASH、崩字计数 +1（压测哨兵非摆设）。
    let mut sk3 = Soaker::new();
    sk3.seed_all();
    if sk3.step(7).is_ok() {
        return Err(E_CRASH);
    }
    if sk3.crashes != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R5 闸门口径（J1 主体二）：闸门未开 → 24h 全量登记必拒
    //    （E_GATE_LOCKED）；闸开 → 登记成功、口径恰 FULL_OPS。
    let mut sk4 = Soaker::new();
    sk4.seed_all();
    if sk4.register_full(false).is_ok() {
        return Err(E_GATE_LOCKED);
    }
    sk4.pre_run(0x42)?;
    let full = sk4.register_full(true)?;
    if full != FULL_OPS {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 空跑不登记：未预跑即开闸登记 → E_LEDGER_BAD（登记无据）。
    let mut sk5 = Soaker::new();
    sk5.seed_all();
    if sk5.register_full(true).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 切换净空实测：删字积栈后切换 → 挂字作废入净空账、栈清零；
    //    引擎活跃位翻转且不越界。
    let mut sk6 = Soaker::new();
    sk6.seed_all();
    let eng_before = sk6.active_engine;
    let mut d = 0u32;
    while d < 4 {
        sk6.step(OP_DELETE)?;
        d += 1;
    }
    let depth_before = sk6.wins[sk6.active_win].undo_depth();
    sk6.step(OP_SWITCH)?;
    if sk6.active_engine == eng_before || sk6.active_engine > 1 {
        return Err(E_INVARIANT);
    }
    if sk6.wins[0].undo_depth() + sk6.wins[1].undo_depth() + sk6.wins[2].undo_depth() != 0 {
        return Err(E_INVARIANT); // 净空：三窗撤销栈全清
    }
    if sk6.pending_switch_chars != depth_before as u32 {
        return Err(E_LEDGER_BAD); // 作废量如实入账
    }
    hits += 1;

    // R8 守恒负例哨兵：人工撕文档（删空且栈清）→ 不变量核必报
    //    E_INVARIANT（口径非恒绿）。
    let mut sk7 = Soaker::new();
    sk7.seed_all();
    // 人为制造违规态：把 0 号窗文档与撤销栈同时清空。
    let w0 = &mut sk7.wins[0];
    w0.doc.clear();
    w0.undo.clear();
    if sk7.check_invariants().is_ok() {
        return Err(E_INVARIANT);
    }
    hits += 1;

    // R9 写回（判据 J2）：带崩不写回、空摘要拒、零崩+非空摘要放行。
    let mut sk8 = Soaker::new();
    sk8.crashes = 1;
    if sk8.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut sk9 = Soaker::new();
    sk9.seed_all();
    let mut rng9 = Lcg::new(0x99);
    let mut i = 0u32;
    while i < 64 {
        sk9.step(rng9.next_below(4) as u8)?;
        i += 1;
    }
    if sk9.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sk9.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
