//! unx48214_xdropkey — UNX-F48214 Xwayland 丢键账本 X 侧分账（与桥总账合流对账）（诊断段 0xBBA0）。
//!
//! 判据原文：
//!   J1 对账 ×10 周期账实一致；判据锚定 §AI-61 §2 专题二丢键账本。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 分账定位：桥总账（BridgeMaster）记键事件的全量收发，X 侧分账
//!      （XSideLedger）只记 X 侧转发与未获回执的键；分账不是总账的副本，
//!      而是守恒恒等式的 X 侧半边：转发量 = 桥收讫量 + 且回账量。
//!   2. 回执模型：X 侧每转发一键即候回执；本轮内未获回执者按
//!      E_ACK_MISS 落且回账（reason 闭集），绝不静默吞账。
//!   3. 对账周期：每周期末跑三层对账——总量守恒（转发=收讫+丢失）、
//!      分窗 tally（每窗收发键数与键码和逐窗相符）、总账单调（桥侧
//!      mseq 严格递增防重放）；三层全过才算账实一致。
//!   4. ×10 周期口径：10 个对账周期（含无损周期与有损周期）逐周期
//!      账实一致，任何一层失守立即带诊断码返回，不留跨期烂账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBBA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ACK_MISS: u16 = SEG_BASE + 1; // 转发未获回执（丢键原因，闭集成员）
const E_TALLY_OFF: u16 = SEG_BASE + 2; // 分窗 tally 不符
const E_MASTER_MONO: u16 = SEG_BASE + 3; // 桥总账 mseq 非单调
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // X 侧分账守恒破
const E_CYCLE_MISS: u16 = SEG_BASE + 5; // 对账周期不足
const E_STATE_BAD: u16 = SEG_BASE + 6; // 对账器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// X 侧转发的键事件。
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub xseq: u64,
    pub keycode: u8,
    pub window: u32,
}

/// 桥总账条目：桥侧收讫编号 mseq 全局单调。
#[derive(Clone, Copy, Debug)]
pub struct MasterEntry {
    pub mseq: u64,
    pub xseq: u64,
    pub keycode: u8,
    pub window: u32,
}

/// X 侧且回账一条：未获回执的键，reason 只出自闭集。
#[derive(Clone, Copy, Debug)]
pub struct DropSub {
    pub xseq: u64,
    pub keycode: u8,
    pub window: u32,
    pub reason: u16,
}

/// 桥总账：收讫入账 + 丢包登记（丢包事实由桥如实自记，供对账复核）。
pub struct BridgeMaster {
    next_mseq: u64,
    pub accepted: Vec<MasterEntry>,
    pub lost: Vec<u64>, // 丢弃的 xseq
}

impl BridgeMaster {
    pub fn new() -> BridgeMaster {
        BridgeMaster { next_mseq: 1, accepted: Vec::new(), lost: Vec::new() }
    }

    /// 收键入账；deliver=false 模拟桥内丢包（键未达总账）。
    pub fn accept(&mut self, ev: &KeyEvent, deliver: bool) -> bool {
        if deliver {
            self.accepted.push(MasterEntry {
                mseq: self.next_mseq,
                xseq: ev.xseq,
                keycode: ev.keycode,
                window: ev.window,
            });
            self.next_mseq += 1;
            true
        } else {
            self.lost.push(ev.xseq);
            false
        }
    }

    /// 总账单调审计：mseq 严格递增且与条目数闭合。
    pub fn audit_monotonic(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.accepted.len() {
            if self.accepted[i].mseq != (i as u64) + 1 {
                return Err(E_MASTER_MONO);
            }
            i += 1;
        }
        if self.next_mseq != (self.accepted.len() as u64) + 1 {
            return Err(E_MASTER_MONO);
        }
        Ok(())
    }

    pub fn accepted_len(&self) -> usize {
        self.accepted.len()
    }
}

/// X 侧分账：转发流水 + 且回账。
pub struct XSideLedger {
    pub issued: Vec<KeyEvent>,
    pub drops: Vec<DropSub>,
}

impl XSideLedger {
    pub fn new() -> XSideLedger {
        XSideLedger { issued: Vec::new(), drops: Vec::new() }
    }

    /// 且回账 reason 闭集审计。
    pub fn audit_reasons(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.drops.len() {
            if self.drops[i].reason != E_ACK_MISS {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 对账器：X 侧分账 ↔ 桥总账的周期合流枢纽。
pub struct Reconciler {
    pub x: XSideLedger,
    pub master: BridgeMaster,
    pub cycle_total: u32,
    pub matched_cycles: u32,
    pub last_reconciled: bool,
}

impl Reconciler {
    pub fn new() -> Reconciler {
        Reconciler {
            x: XSideLedger::new(),
            master: BridgeMaster::new(),
            cycle_total: 0,
            matched_cycles: 0,
            last_reconciled: false,
        }
    }

    /// 一个对账周期：X 侧整流转发 → 且回落账 → 三层对账。
    pub fn cycle(&mut self, stream: &[KeyEvent], lossy: bool) -> Result<(), u16> {
        let mut i = 0usize;
        while i < stream.len() {
            let ev = stream[i];
            self.x.issued.push(ev);
            // 有损模式：键码为 13 倍数的键在桥内丢失（确定性注入）。
            let deliver = !(lossy && ev.keycode % 13 == 0);
            if !self.master.accept(&ev, deliver) {
                self.x.drops.push(DropSub {
                    xseq: ev.xseq,
                    keycode: ev.keycode,
                    window: ev.window,
                    reason: E_ACK_MISS,
                });
            }
            i += 1;
        }
        self.reconcile()?;
        self.cycle_total += 1;
        self.matched_cycles += 1;
        self.last_reconciled = true;
        Ok(())
    }

    /// 三层对账（纪律 3）：总量守恒、分窗 tally、总账单调。
    pub fn reconcile(&self) -> Result<(), u16> {
        // 层一：总量守恒 转发 = 收讫 + 且回。
        if self.x.issued.len() != self.master.accepted_len() + self.x.drops.len() {
            return Err(E_LEDGER_BAD);
        }
        // 层二：分窗 tally——逐窗比对（键数、键码和双口径）。
        let mut w = 0usize;
        while w < self.x.issued.len() {
            let win = self.x.issued[w].window;
            // 只对窗口首次出现的位置做全窗对账（去重）。
            let mut first = true;
            let mut k = 0usize;
            while k < w {
                if self.x.issued[k].window == win {
                    first = false;
                    break;
                }
                k += 1;
            }
            if first {
                self.reconcile_window(win)?;
            }
            w += 1;
        }
        // 层三：桥总账单调。
        self.master.audit_monotonic()?;
        // 且回账 reason 闭集。
        self.x.audit_reasons()?;
        Ok(())
    }

    /// 单窗对账：issued 与 (accepted + drops) 在该窗的键数与键码和必须逐位相等。
    fn reconcile_window(&self, win: u32) -> Result<(), u16> {
        let mut iss_n: u32 = 0;
        let mut iss_sum: u64 = 0;
        let mut i = 0usize;
        while i < self.x.issued.len() {
            if self.x.issued[i].window == win {
                iss_n += 1;
                iss_sum += self.x.issued[i].keycode as u64;
            }
            i += 1;
        }
        let mut acc_n: u32 = 0;
        let mut acc_sum: u64 = 0;
        i = 0;
        while i < self.master.accepted.len() {
            if self.master.accepted[i].window == win {
                acc_n += 1;
                acc_sum += self.master.accepted[i].keycode as u64;
            }
            i += 1;
        }
        let mut drp_n: u32 = 0;
        let mut drp_sum: u64 = 0;
        i = 0;
        while i < self.x.drops.len() {
            if self.x.drops[i].window == win {
                drp_n += 1;
                drp_sum += self.x.drops[i].keycode as u64;
            }
            i += 1;
        }
        if iss_n != acc_n + drp_n || iss_sum != acc_sum + drp_sum {
            return Err(E_TALLY_OFF);
        }
        Ok(())
    }

    pub fn issued_total(&self) -> usize {
        self.x.issued.len()
    }
    pub fn drop_total(&self) -> usize {
        self.x.drops.len()
    }

    /// ×N 周期覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.matched_cycles < want {
            return Err(E_CYCLE_MISS);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：对账态闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.last_reconciled {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（对账 ×10 周期账实一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10 周期：周期 3、7 有损（确定性丢包注入），其余无损；
    //    逐周期三层对账全过，账实一致 ×10 达标。
    let mut rc = Reconciler::new();
    let mut c = 0u32;
    while c < 10 {
        let n = 12 + c;
        let mut stream: Vec<KeyEvent> = Vec::new();
        let mut j = 0u32;
        while j < n {
            stream.push(KeyEvent {
                xseq: (c * 100 + j) as u64,
                keycode: ((c * 37 + j * 11) % 248 + 8) as u8,
                window: 0xA000 + (j % 3),
            });
            j += 1;
        }
        rc.cycle(&stream, c == 3 || c == 7)?;
        c += 1;
    }
    rc.coverage(10)?;
    hits += 1;

    // R2 守恒恒等式终判：转发 = 收讫 + 且回；有损周期丢包全部落账，零静默。
    if rc.issued_total() != rc.master.accepted_len() + rc.drop_total() {
        return Err(E_LEDGER_BAD);
    }
    if rc.drop_total() == 0 {
        return Err(E_LEDGER_BAD); // 有损周期必须产生且回账
    }
    rc.x.audit_reasons()?;
    hits += 1;

    // R3 分窗 tally 终判：每窗键数、键码和两侧相等（对账器内层二复核）。
    rc.reconcile()?;
    hits += 1;

    // R4 桥总账单调终判：mseq 从 1 起严格递增、与条目数闭合。
    rc.master.audit_monotonic()?;
    if rc.master.accepted[0].mseq != 1 {
        return Err(E_MASTER_MONO);
    }
    hits += 1;

    // R5 无损周期零丢账：非 3/7 周期（如周期 0 结束时点）且回账只增于有损期。
    let mut rc2 = Reconciler::new();
    let mut stream: Vec<KeyEvent> = Vec::new();
    let mut j = 0u32;
    while j < 20 {
        stream.push(KeyEvent { xseq: j as u64, keycode: (j % 200 + 8) as u8, window: 0xB000 });
        j += 1;
    }
    rc2.cycle(&stream, false)?;
    if rc2.drop_total() != 0 {
        return Err(E_LEDGER_BAD); // 无损周期不得产生且回账
    }
    if rc2.issued_total() != rc2.master.accepted_len() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 烂账即报：人为塞一条假且回账（账实不符），对账立即失守（E_TALLY_OFF
    //    /E_LEDGER_BAD 族），不留跨期烂账。
    let mut rc3 = Reconciler::new();
    let mut stream: Vec<KeyEvent> = Vec::new();
    let mut j = 0u32;
    while j < 4 {
        stream.push(KeyEvent { xseq: j as u64, keycode: 100 + j as u8, window: 0xC000 });
        j += 1;
    }
    rc3.cycle(&stream, false)?;
    rc3.x.drops.push(DropSub {
        xseq: 999,
        keycode: 1,
        window: 0xC000,
        reason: E_ACK_MISS,
    });
    if rc3.reconcile().is_ok() {
        return Err(E_LEDGER_BAD); // 守恒被破坏必须报错
    }
    hits += 1;

    // R7 reason 闭集 + 状态机闭合：假账 reason 越界同报；对账器状态位闭合。
    let mut rc4 = Reconciler::new();
    rc4.x.drops.push(DropSub { xseq: 1, keycode: 1, window: 1, reason: SEG_BASE + 0x1F });
    if rc4.x.audit_reasons().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if !rc.last_reconciled {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：未对账不写回、空摘要拒、对账闭合+非空摘要放行。
    let mut rc5 = Reconciler::new();
    if rc5.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut stream: Vec<KeyEvent> = Vec::new();
    stream.push(KeyEvent { xseq: 1, keycode: 42, window: 0xD000 });
    rc5.cycle(&stream, false)?;
    if rc5.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    rc5.writeback(true, 0x30)?;
    hits += 1;

    Ok(hits)
}
