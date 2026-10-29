//! unx48192_wobs4pt — UNX-F48192 Wine 窗观测埋点（START/COMP/END/CAND 四类消息四点）（诊断段 0xB8E0）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四点定义：一次输入会话的生命周期被四个观测点完整覆盖——
//!      START（会话开：焦点获得后首键）、COMP（组串更新，可多次）、
//!      CAND（候选窗变化，可多次）、END（会话闭：上屏或取消）。
//!   2. 四点 100% 口径（J1）：每条会话账必须同时含 START 与 END 各恰一次、
//!      COMP ≥1 次、CAND ≥1 次，且事件序合法（START 最先、END 最后、
//!      中间点不越会话界）；满足者记「全覆盖」，逐会话核销。
//!   3. 点序状态机：Closed → Started →（Comp/Cand 往复）→ Ended → Closed；
//!      非法迁移（未 START 先 COMP、END 后再 COMP、重 START、重 END）
//!      一律拒收并落违序账，不污染会话账。
//!   4. 埋点载荷：每点带 (session_id, tick, payload_len) 三元组，tick
//!      单调递增；载荷长度只记账不存内容（观测面零拷贝红线）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB8E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ORDER_BAD: u16 = SEG_BASE + 1;   // 点序非法迁移
const E_PARTIAL: u16 = SEG_BASE + 2;     // 会话四点缺一
const E_SESSION_MISS: u16 = SEG_BASE + 3;// 会话账缺失/越界
const E_TICK_BACK: u16 = SEG_BASE + 4;   // tick 回退
const E_COVER_MISS: u16 = SEG_BASE + 5;  // 四点 100% 未达
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 账目不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 观测点四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    Start,
    Comp,
    Cand,
    End,
}

/// 会话内状态机（纪律 3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessState {
    Closed,
    Started,
    Ended,
}

/// 一条埋点账：点 + 会话号 + tick + 载荷长度。
#[derive(Clone, Copy, Debug)]
pub struct ObsRecord {
    pub point: ObsPoint,
    pub session: u32,
    pub tick: u64,
    pub payload_len: u32,
}

/// 会话覆盖账：一次会话四点的到点情况。
#[derive(Clone, Copy, Debug)]
pub struct SessCover {
    pub session: u32,
    pub has_start: bool,
    pub has_comp: bool,
    pub has_cand: bool,
    pub has_end: bool,
    pub comp_n: u32,
    pub cand_n: u32,
    pub full: bool,
}

/// 违序账：被拒收的非法点。
#[derive(Clone, Copy, Debug)]
pub struct ViolationRecord {
    pub point: ObsPoint,
    pub session: u32,
    pub reason: u16,
}

/// 观测埋点枢纽：点序状态机 + 会话覆盖账 + 违序账。
pub struct WObs4Pt {
    pub state: SessState,
    pub cur_session: u32,
    pub last_tick: u64,
    records: Vec<ObsRecord>,
    covers: Vec<SessCover>,
    violations: Vec<ViolationRecord>,
    pub session_total: u32,
    pub emit_total: u32,
    pub reject_total: u32,
}

impl WObs4Pt {
    pub fn new() -> WObs4Pt {
        WObs4Pt {
            state: SessState::Closed,
            cur_session: 0,
            last_tick: 0,
            records: Vec::new(),
            covers: Vec::new(),
            violations: Vec::new(),
            session_total: 0,
            emit_total: 0,
            reject_total: 0,
        }
    }

    /// 会话开启（START 点）：Closed→Started，新会话入覆盖账。
    pub fn emit_start(&mut self, session: u32, tick: u64, payload_len: u32) -> Result<(), u16> {
        if self.state != SessState::Closed {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        self.state = SessState::Started;
        self.cur_session = session;
        self.records.push(ObsRecord {
            point: ObsPoint::Start,
            session,
            tick,
            payload_len,
        });
        self.covers.push(SessCover {
            session,
            has_start: true,
            has_comp: false,
            has_cand: false,
            has_end: false,
            comp_n: 0,
            cand_n: 0,
            full: false,
        });
        self.last_tick = tick;
        self.session_total += 1;
        self.emit_total += 1;
        Ok(())
    }

    /// 中间点（COMP/CAND）：仅 Started 态可收，同会话内往复无限制。
    pub fn emit_mid(&mut self, point: ObsPoint, tick: u64, payload_len: u32) -> Result<(), u16> {
        if self.state != SessState::Started {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        let sess = self.cur_session;
        let rec = ObsRecord { point, session: sess, tick, payload_len };
        self.records.push(rec);
        self.last_tick = tick;
        self.emit_total += 1;
        // 覆盖账落格。
        let mut i = self.covers.len();
        let mut hit = false;
        while i > 0 {
            i -= 1;
            if self.covers[i].session == sess {
                match point {
                    ObsPoint::Comp => {
                        self.covers[i].has_comp = true;
                        self.covers[i].comp_n += 1;
                    }
                    ObsPoint::Cand => {
                        self.covers[i].has_cand = true;
                        self.covers[i].cand_n += 1;
                    }
                    _ => {}
                }
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(E_SESSION_MISS);
        }
        Ok(())
    }

    /// 会话关闭（END 点）：Started→Ended→Closed，覆盖账终判四点齐缺。
    pub fn emit_end(&mut self, tick: u64, payload_len: u32) -> Result<(), u16> {
        if self.state != SessState::Started {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        let sess = self.cur_session;
        self.records.push(ObsRecord {
            point: ObsPoint::End,
            session: sess,
            tick,
            payload_len,
        });
        self.last_tick = tick;
        self.emit_total += 1;
        self.state = SessState::Ended;
        // 终判：四点齐才记 full（纪律 2）。
        let mut i = self.covers.len();
        while i > 0 {
            i -= 1;
            if self.covers[i].session == sess {
                let c = &mut self.covers[i];
                c.has_end = true;
                c.full = c.has_start && c.has_comp && c.has_cand && c.has_end;
                break;
            }
        }
        self.state = SessState::Closed;
        self.cur_session = 0;
        Ok(())
    }

    /// 非法点挂账（供调用方在 emit 系列返回 Err 时显式落违序账）。
    pub fn note_violation(&mut self, point: ObsPoint, session: u32, reason: u16) {
        self.violations.push(ViolationRecord { point, session, reason });
        self.reject_total += 1;
    }

    /// 四点 100% 审计（J1）：全会话 full 全真 + 账目守恒。
    pub fn audit_full_coverage(&self) -> Result<(), u16> {
        if self.covers.len() as u32 != self.session_total {
            return Err(E_SESSION_MISS);
        }
        let mut i = 0usize;
        while i < self.covers.len() {
            if !self.covers[i].full {
                return Err(E_PARTIAL);
            }
            i += 1;
        }
        if self.records.len() as u32 != self.emit_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// ×N 会话覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.session_total < want {
            return Err(E_COVER_MISS);
        }
        Ok(())
    }

    /// tick 单调审计：账内 tick 逐条不回退。
    pub fn audit_tick(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.records.len() {
            if self.records[i].tick < self.records[i - 1].tick {
                return Err(E_TICK_BACK);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn records_len(&self) -> usize {
        self.records.len()
    }
    pub fn covers_len(&self) -> usize {
        self.covers.len()
    }
    pub fn violations_len(&self) -> usize {
        self.violations.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_ORDER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（四点 100%），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 标准会话 ×10：每会话 START+2×COMP+2×CAND+END，逐会话覆盖账
    //    全真，四点 100%，tick 单调。
    let mut ob = WObs4Pt::new();
    let mut s = 0u32;
    let mut tick = 0u64;
    while s < 10 {
        tick += 1;
        ob.emit_start(s, tick, 8)?;
        let mut k = 0u32;
        while k < 2 {
            tick += 1;
            ob.emit_mid(ObsPoint::Comp, tick, 16)?;
            tick += 1;
            ob.emit_mid(ObsPoint::Cand, tick, 4)?;
            k += 1;
        }
        tick += 1;
        ob.emit_end(tick, 8)?;
        s += 1;
    }
    ob.coverage(10)?;
    ob.audit_full_coverage()?;
    ob.audit_tick()?;
    if ob.records_len() != 10 * 6 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 ×100 会话主扫：100 会话（每会话 1+1+1+1 四点），100% 覆盖。
    let mut ob = WObs4Pt::new();
    let mut s = 0u32;
    let mut tick = 100u64;
    while s < 100 {
        tick += 1;
        ob.emit_start(s, tick, 1)?;
        tick += 1;
        ob.emit_mid(ObsPoint::Comp, tick, 2)?;
        tick += 1;
        ob.emit_mid(ObsPoint::Cand, tick, 3)?;
        tick += 1;
        ob.emit_end(tick, 4)?;
        s += 1;
    }
    ob.coverage(100)?;
    ob.audit_full_coverage()?;
    ob.audit_tick()?;
    if ob.covers_len() != 100 {
        return Err(E_SESSION_MISS);
    }
    hits += 1;

    // R3 违序拒收×4：未 START 先 COMP、重 START、END 后再 COMP、
    //    重 END——全部拒收并挂违序账，会话账不污染。
    let mut ob = WObs4Pt::new();
    if ob.emit_mid(ObsPoint::Comp, 1, 0).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_violation(ObsPoint::Comp, 0, E_ORDER_BAD);
    ob.emit_start(1, 2, 8)?;
    if ob.emit_start(2, 3, 8).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_violation(ObsPoint::Start, 2, E_ORDER_BAD);
    ob.emit_mid(ObsPoint::Comp, 4, 8)?;
    ob.emit_end(5, 8)?;
    if ob.emit_mid(ObsPoint::Cand, 6, 8).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_violation(ObsPoint::Cand, 1, E_ORDER_BAD);
    if ob.emit_end(7, 8).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_violation(ObsPoint::End, 1, E_ORDER_BAD);
    if ob.reject_total != 4 || ob.violations_len() != 4 {
        return Err(E_LEDGER_BAD);
    }
    // 会话 1 仅有 START+COMP+END：缺 CAND，覆盖审计必红。
    if ob.audit_full_coverage().is_ok() {
        return Err(E_PARTIAL);
    }
    hits += 1;

    // R4 缺点补齐路径：R3 残局无法回补（会话已闭），新会话补齐后整体
    //    仍按逐会话口径审计——旧会话缺点不连坐新会话，但整体 100% 需
    //    全量会话全真，故残局只能以「已如实挂账」收口。
    let mut ob2 = WObs4Pt::new();
    ob2.emit_start(9, 10, 8)?;
    ob2.emit_mid(ObsPoint::Comp, 11, 8)?;
    ob2.emit_mid(ObsPoint::Cand, 12, 8)?;
    ob2.emit_end(13, 8)?;
    ob2.audit_full_coverage()?; // 单会话全真 → 100%
    if ob2.covers_len() != 1 {
        return Err(E_SESSION_MISS);
    }
    hits += 1;

    // R5 tick 回退拒收：乱序 tick 的点被拒且不落账。
    let mut ob = WObs4Pt::new();
    ob.emit_start(1, 50, 8)?;
    ob.emit_mid(ObsPoint::Comp, 60, 8)?;
    if ob.emit_mid(ObsPoint::Cand, 55, 8).is_ok() {
        return Err(E_TICK_BACK);
    }
    if ob.emit_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    // 合法 tick 续走，会话仍可闭全。
    ob.emit_mid(ObsPoint::Cand, 70, 8)?;
    ob.emit_end(80, 8)?;
    ob.audit_full_coverage()?;
    ob.audit_tick()?;
    hits += 1;

    // R6 多次 COMP/CAND：会话内 COMP×5、CAND×3 皆合法，计数入账。
    let mut ob = WObs4Pt::new();
    ob.emit_start(1, 1, 8)?;
    let mut k = 0u32;
    while k < 5 {
        ob.emit_mid(ObsPoint::Comp, 2 + k as u64, 16)?;
        k += 1;
    }
    let mut k = 0u32;
    while k < 3 {
        ob.emit_mid(ObsPoint::Cand, 10 + k as u64, 4)?;
        k += 1;
    }
    ob.emit_end(20, 8)?;
    ob.audit_full_coverage()?;
    let c = &ob.covers[0];
    if c.comp_n != 5 || c.cand_n != 3 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 载荷长度只记账：records 内 payload_len 与 emit 入参逐条相等
    //    （观测面零拷贝——账里只有长度，无内容字段）。
    let mut ob = WObs4Pt::new();
    ob.emit_start(1, 1, 7)?;
    ob.emit_mid(ObsPoint::Comp, 2, 33)?;
    ob.emit_mid(ObsPoint::Cand, 3, 9)?;
    ob.emit_end(4, 21)?;
    if ob.records_len() != 4 {
        return Err(E_LEDGER_BAD);
    }
    let want: [u32; 4] = [7, 33, 9, 21];
    let mut i = 0usize;
    while i < 4 {
        if ob.records[i].payload_len != want[i] {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    ob.audit_full_coverage()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let mut ob = WObs4Pt::new();
    if ob.writeback(true, 0x10).is_ok() {
        return Err(E_ORDER_BAD); // 无任何会话不写回
    }
    ob.emit_start(1, 1, 8)?;
    ob.emit_mid(ObsPoint::Comp, 2, 8)?;
    ob.emit_mid(ObsPoint::Cand, 3, 8)?;
    ob.emit_end(4, 8)?;
    if ob.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ob.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
