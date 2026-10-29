// unx1235_u1235 —— UNX-F1235 Draining 态审计账（窗口与切换飞行记录）
// 判据原文 UNX-F1235-J1：审计账全过（进入/退出 Draining 四元记录、驻留账
// 配对重算一致、回放对拍零差异、守恒（进=出+在态）、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb8f0 独占（0xb8f0~0xb92f）。
//
// 设计（纯功能 no_std）：
//   * 飞行记录：进入/退出 Draining 各一行（tick、对侧态、原因码）；
//   * 驻留账：进入-退出配对重算 == 累计驻留（双算对拍）；
//   * 回放：逐行重放重建在态结论，与账目侧对拍零差异；
//   * 纪律：tick 单调、Draining→Draining 环回拒、未进先退拒、进中再进拒；
//   * FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb8f0 独占） ─────────────────────────────────────────────
pub const XC_DA_SEALED: u32 = 0xB8F1; // 封账后记录
pub const XC_DA_BACKTICK: u32 = 0xB8F2; // tick 回退
pub const XC_DA_LOOP: u32 = 0xB8F3; // Draining 环回
pub const XC_DA_NOTIN: u32 = 0xB8F4; // 未在态退出
pub const XC_DA_ALREADY: u32 = 0xB8F5; // 已在态再进
pub const XC_DA_BADSTATE: u32 = 0xB8F6; // 非法对侧态

/// 飞行行。
#[derive(Clone, Copy)]
pub struct DFlightRow {
    pub tick: u64,
    /// true=进入 Draining。
    pub enter: bool,
    /// 对侧态编码 0~3。
    pub peer: u8,
    pub cause: u16,
}

/// 审计账。
pub struct DrainAudit {
    cap: usize,
    rows: Vec<DFlightRow>,
    in_state: bool,
    enters: u32,
    exits: u32,
    dwell: u64,
    last_tick: u64,
    enter_tick: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DrainAudit {
    pub fn new(cap: usize) -> DrainAudit {
        DrainAudit {
            cap,
            rows: vec![],
            in_state: false,
            enters: 0,
            exits: 0,
            dwell: 0,
            last_tick: 0,
            enter_tick: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn peer_ok(p: u8) -> bool {
        p <= 3
    }

    /// 进入 Draining。
    pub fn enter(&mut self, tick: u64, peer: u8, cause: u16) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DA_SEALED, "封账后记录"));
            return Err(XC_DA_SEALED);
        }
        if !Self::peer_ok(peer) {
            self.rejects.push((XC_DA_BADSTATE, "非法对侧态"));
            return Err(XC_DA_BADSTATE);
        }
        if self.in_state {
            self.rejects.push((XC_DA_ALREADY, "已在态再进"));
            return Err(XC_DA_ALREADY);
        }
        if !self.rows.is_empty() {
            if tick < self.last_tick {
                self.rejects.push((XC_DA_BACKTICK, "tick 回退"));
                return Err(XC_DA_BACKTICK);
            }
            if peer == 1 {
                self.rejects.push((XC_DA_LOOP, "Draining 环回"));
                return Err(XC_DA_LOOP);
            }
        }
        self.rows.push(DFlightRow { tick, enter: true, peer, cause });
        self.in_state = true;
        self.enters = self.enters.saturating_add(1);
        self.enter_tick = tick;
        self.last_tick = tick;
        Ok(())
    }

    /// 退出 Draining。
    pub fn exit(&mut self, tick: u64, peer: u8, cause: u16) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DA_SEALED, "封账后记录"));
            return Err(XC_DA_SEALED);
        }
        if !Self::peer_ok(peer) {
            self.rejects.push((XC_DA_BADSTATE, "非法对侧态"));
            return Err(XC_DA_BADSTATE);
        }
        if !self.in_state {
            self.rejects.push((XC_DA_NOTIN, "未在态退出"));
            return Err(XC_DA_NOTIN);
        }
        if tick < self.last_tick {
            self.rejects.push((XC_DA_BACKTICK, "tick 回退"));
            return Err(XC_DA_BACKTICK);
        }
        if peer == 1 {
            self.rejects.push((XC_DA_LOOP, "Draining 环回"));
            return Err(XC_DA_LOOP);
        }
        self.dwell = self.dwell.saturating_add(tick.saturating_sub(self.enter_tick));
        self.rows.push(DFlightRow { tick, enter: false, peer, cause });
        self.in_state = false;
        self.exits = self.exits.saturating_add(1);
        self.last_tick = tick;
        Ok(())
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.enters == self.exits + if self.in_state { 1 } else { 0 }
    }

    /// 驻留双算对拍。
    pub fn dwell_recheck(&self) -> bool {
        let mut total: u64 = 0;
        let mut open: Option<u64> = None;
        for r in &self.rows {
            if r.enter {
                if open.is_some() {
                    return false;
                }
                open = Some(r.tick);
            } else {
                match open {
                    Some(t0) => {
                        total = total.saturating_add(r.tick.saturating_sub(t0));
                        open = None;
                    }
                    None => return false,
                }
            }
        }
        total == self.dwell
    }

    /// 回放对拍。
    pub fn replay(&self) -> Result<bool, u32> {
        let mut st = false;
        let mut prev = 0u64;
        let mut first = true;
        for r in &self.rows {
            if !first {
                if r.tick < prev {
                    return Err(XC_DA_BACKTICK);
                }
                if r.enter && st {
                    return Err(XC_DA_ALREADY);
                }
                if !r.enter && !st {
                    return Err(XC_DA_NOTIN);
                }
            }
            st = r.enter;
            prev = r.tick;
            first = false;
        }
        Ok(st == self.in_state && self.enters == self.exits + if st { 1 } else { 0 })
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DA_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.enters as u64;
        h = h.wrapping_mul(0x0100_0193) ^ self.exits as u64;
        h = h.wrapping_mul(0x0100_0193) ^ self.dwell;
        for r in &self.rows {
            let dir: u64 = if r.enter { 1 } else { 0 };
            h ^= r.tick ^ (dir << 56) ^ ((r.peer as u64) << 48) ^ ((r.cause as u64) << 32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：三轮巡游 + 守恒 + 驻留双算 + 回放零差异。
pub fn audit_scenario() -> Result<(usize, u64, u64), u32> {
    let mut a = DrainAudit::new(32);
    a.enter(100, 0, 1)?;  // Polling → Draining
    a.exit(220, 2, 2)?;   // Draining → Arming
    a.enter(300, 3, 3)?;  // Irq → Draining
    a.exit(455, 0, 4)?;   // Draining → Polling
    a.enter(600, 2, 5)?;  // Arming → Draining
    a.exit(777, 3, 6)?;   // Draining → Irq
    if !a.conserved() || !a.dwell_recheck() {
        return Err(XC_DA_NOTIN);
    }
    if a.replay()? != true {
        return Err(XC_DA_ALREADY);
    }
    // 驻留：120 + 155 + 177 = 452
    if a.dwell != 452 {
        return Err(XC_DA_BACKTICK);
    }
    let fp = a.seal()?;
    Ok((a.rows_len(), a.dwell, fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）未在态退出必拒
    {
        let mut a = DrainAudit::new(8);
        if a.exit(10, 0, 1) == Err(XC_DA_NOTIN) {
            caught += 1;
        }
    }
    // 2）已在态再进必拒
    {
        let mut a = DrainAudit::new(8);
        let _ = a.enter(10, 0, 1);
        if a.enter(20, 0, 1) == Err(XC_DA_ALREADY) {
            caught += 1;
        }
    }
    // 3）Draining 环回必拒（退出到自身）
    {
        let mut a = DrainAudit::new(8);
        let _ = a.enter(10, 0, 1);
        if a.exit(20, 1, 2) == Err(XC_DA_LOOP) {
            caught += 1;
        }
    }
    // 4）非法对侧态必拒
    {
        let mut a = DrainAudit::new(8);
        if a.enter(10, 9, 1) == Err(XC_DA_BADSTATE) {
            caught += 1;
        }
    }
    // 5）tick 回退必拒
    {
        let mut a = DrainAudit::new(8);
        let _ = a.enter(100, 0, 1);
        if a.exit(50, 0, 2) == Err(XC_DA_BACKTICK) {
            caught += 1;
        }
    }
    // 6）封账后记录必拒
    {
        let mut a = DrainAudit::new(8);
        let _ = a.enter(10, 0, 1);
        let _ = a.seal();
        if a.exit(20, 0, 2) == Err(XC_DA_SEALED) {
            caught += 1;
        }
    }
    (caught, N)
}
