//! UNX-F56139 帧率采集器 O2 深化标定预约件（0x61160 段）
//!
//! 锚点：UNX-F56139-J1 预约登记账（O2 深化标定预约单逐条落账：维度 ×
//! 预约波次 × 优先级 × 状态四元组）；J2 预约状态机（OPEN→LOCKED→DONE
//! 单向流转，回退必拒）；J3 预约唯一性（同维度同时段只允许一个 OPEN
//! 预约，重复必拒）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·O2 预约面）。
//!
//! 结构：预约登记册（维度 × 状态机 × 波次链）+ 状态机核（单向流转
//! 语义）+ 唯一性闸（同维度 OPEN 互斥）+ 优先级仲裁（同波次多预约按
//! 优先级排序导出）+ 注入核（未知维度/回退流转/重复 OPEN/LOCKED 后改
//! 优先级/空预约完成必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61160 独占 0x20）。
pub const SEG_BASE: u32 = 0x61160;
pub const RSV_UNK_DIM: u32 = SEG_BASE + 0x01; // 未知标定维度
pub const RSV_STATE_BACK: u32 = SEG_BASE + 0x02; // 状态回退流转
pub const RSV_DUP_OPEN: u32 = SEG_BASE + 0x03; // 同维度重复 OPEN
pub const RSV_PRIO_LOCKED: u32 = SEG_BASE + 0x04; // LOCKED 后改优先级
pub const RSV_DONE_EMPTY: u32 = SEG_BASE + 0x05; // 空预约完成（无数据完成）

/// 标定维度闭集（帧率 O2 深化四维）。
pub const DIM_NOISE: usize = 0;
pub const DIM_BAND: usize = 1;
pub const DIM_WINDOW: usize = 2;
pub const DIM_QUANT: usize = 3;
pub const DIM_COUNT: usize = 4;

/// 维度名渲染。
pub const DIM_NAMES: [&str; DIM_COUNT] = ["noise", "band", "window", "quant"];

/// 预约状态机（三态单向）。
pub const ST_OPEN: u8 = 1;
pub const ST_LOCKED: u8 = 2;
pub const ST_DONE: u8 = 3;

/// 状态名渲染。
pub fn state_name(st: u8) -> &'static str {
    match st {
        ST_OPEN => "OPEN",
        ST_LOCKED => "LOCKED",
        ST_DONE => "DONE",
        _ => "?",
    }
}

/// 状态流转核：只允许 +1 单向（OPEN→LOCKED→DONE），其余一律拒。
pub fn transition_ok(from: u8, to: u8) -> bool {
    to == from + 1 && from >= ST_OPEN && to <= ST_DONE
}

/// 单条预约：维度 × 波次 × 优先级 × 状态 四元组。
#[derive(Clone)]
pub struct Reservation {
    pub dim: usize,
    pub wave: u32,
    pub prio: u32,
    pub state: u8,
}

impl Reservation {
    fn render_line(&self) -> String {
        format!(
            "{}|w{}|p{}|{}",
            DIM_NAMES[self.dim],
            self.wave,
            self.prio,
            state_name(self.state)
        )
    }
}

/// 预约登记册。
pub struct ReservationBook {
    items: Vec<Reservation>,
}

impl ReservationBook {
    pub fn new() -> Self {
        ReservationBook { items: Vec::new() }
    }

    /// 开预约：维度闭集 + 同维度无 OPEN 互斥双闸。
    pub fn open(&mut self, dim: usize, wave: u32, prio: u32) -> Result<usize, u32> {
        if dim >= DIM_COUNT {
            return Err(RSV_UNK_DIM);
        }
        if self.items.iter().any(|r| r.dim == dim && r.state == ST_OPEN) {
            return Err(RSV_DUP_OPEN);
        }
        self.items.push(Reservation {
            dim,
            wave,
            prio,
            state: ST_OPEN,
        });
        Ok(self.items.len() - 1)
    }

    /// 流转：单向状态机核 + LOCKED 后禁改优先级。
    pub fn advance(&mut self, idx: usize) -> Result<u8, u32> {
        let cur = self.items.get(idx).map(|r| r.state).unwrap_or(0);
        if !transition_ok(cur, cur + 1) {
            return Err(RSV_STATE_BACK);
        }
        let next = cur + 1;
        self.items[idx].state = next;
        Ok(next)
    }

    /// 改优先级：仅 OPEN 态可改。
    pub fn set_prio(&mut self, idx: usize, prio: u32) -> Result<(), u32> {
        let st = self.items.get(idx).map(|r| r.state).unwrap_or(0);
        if st != ST_OPEN {
            return Err(RSV_PRIO_LOCKED);
        }
        self.items[idx].prio = prio;
        Ok(())
    }

    /// 完成预约：DONE 态要求有采样数据量佐证（data_n > 0）。
    pub fn complete(&mut self, idx: usize, data_n: u32) -> Result<(), u32> {
        let st = self.items.get(idx).map(|r| r.state).unwrap_or(0);
        if st != ST_LOCKED {
            return Err(RSV_STATE_BACK);
        }
        if data_n == 0 {
            return Err(RSV_DONE_EMPTY);
        }
        self.items[idx].state = ST_DONE;
        Ok(())
    }

    pub fn item(&self, idx: usize) -> Option<&Reservation> {
        self.items.get(idx)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 同波次 OPEN 预约按优先级降序导出（仲裁视图）。
    pub fn open_by_prio(&self, wave: u32) -> Vec<usize> {
        let mut idxs: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, r)| r.wave == wave && r.state == ST_OPEN)
            .map(|(i, _)| i)
            .collect();
        idxs.sort_by(|a, b| {
            let pa = self.items[*a].prio;
            let pb = self.items[*b].prio;
            pb.cmp(&pa).then(a.cmp(b))
        });
        idxs
    }

    /// 全量核：状态合法 + 无同维度双 OPEN 残留。
    pub fn book_ok(&self) -> bool {
        self.items.iter().all(|r| (ST_OPEN..=ST_DONE).contains(&r.state))
            && !(0..DIM_COUNT).any(|d| {
                self.items.iter().filter(|r| r.dim == d && r.state == ST_OPEN).count() > 1
            })
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for r in &self.items {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        out
    }
}

impl Default for ReservationBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四维各开一预约，前三维流转到 DONE，第四维保持 OPEN。
fn standard_book() -> ReservationBook {
    let mut book = ReservationBook::new();
    let _ = book.open(DIM_NOISE, 1, 10);
    let _ = book.open(DIM_BAND, 1, 20);
    let _ = book.open(DIM_WINDOW, 2, 30);
    let _ = book.open(DIM_QUANT, 2, 5);
    // 前三维走完状态机
    for idx in 0..3 {
        let _ = book.advance(idx); // OPEN→LOCKED
        let _ = book.complete(idx, 42);
    }
    book
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知维度
    let mut book = ReservationBook::new();
    if book.open(9, 1, 1) == Err(RSV_UNK_DIM) {
        caught += 1;
    }
    // 注 2：同维度重复 OPEN
    let mut book = ReservationBook::new();
    let _ = book.open(DIM_NOISE, 1, 1);
    if book.open(DIM_NOISE, 1, 2) == Err(RSV_DUP_OPEN) {
        caught += 1;
    }
    // 注 3：状态回退（对 DONE 再 advance）
    let mut book = standard_book();
    if book.advance(0) == Err(RSV_STATE_BACK) {
        caught += 1;
    }
    // 注 4：LOCKED 后改优先级
    let mut book = ReservationBook::new();
    let idx = book.open(DIM_BAND, 1, 5).unwrap_or(0);
    let _ = book.advance(idx); // → LOCKED
    if book.set_prio(idx, 99) == Err(RSV_PRIO_LOCKED) {
        caught += 1;
    }
    // 注 5：空数据完成
    let mut book = ReservationBook::new();
    let idx = book.open(DIM_WINDOW, 1, 1).unwrap_or(0);
    let _ = book.advance(idx);
    if book.complete(idx, 0) == Err(RSV_DONE_EMPTY) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 预约登记账；J2 状态机；J3 唯一性与仲裁。
pub fn run_unx56139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56139");
    // J1-1 四维预约全落账
    let book = standard_book();
    let ok = book.len() == 4 && book.book_ok();
    s.add("56139-J1-四维全落账", ok, if ok { "" } else { "预约账不全" });
    // J1-2 前三维 DONE、第四维 OPEN
    let ok = (0..3).all(|i| book.item(i).map(|r| r.state == ST_DONE).unwrap_or(false))
        && book.item(3).map(|r| r.state == ST_OPEN).unwrap_or(false);
    s.add("56139-J1-状态逐条", ok, if ok { "" } else { "状态面漂移" });
    // J1-3 渲染导出行齐
    let text = standard_book().render_book();
    let ok = text.contains("noise|w1|p10|DONE") && text.contains("quant|w2|p5|OPEN");
    s.add("56139-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 状态机单向：合法流转三步
    let ok = transition_ok(ST_OPEN, ST_LOCKED) && transition_ok(ST_LOCKED, ST_DONE);
    s.add("56139-J2-合法流转", ok, if ok { "" } else { "状态机漂移" });
    // J2-2 状态机拒绝：跳级与回退
    let ok = !transition_ok(ST_OPEN, ST_DONE) && !transition_ok(ST_DONE, ST_LOCKED)
        && !transition_ok(ST_LOCKED, ST_OPEN);
    s.add("56139-J2-跳回皆拒", ok, if ok { "" } else { "状态机失效" });
    // J2-3 完成前置：OPEN 直完成拒
    let mut book2 = ReservationBook::new();
    let idx = book2.open(DIM_NOISE, 1, 1).unwrap_or(0);
    let ok = book2.complete(idx, 42) == Err(RSV_STATE_BACK);
    s.add("56139-J2-OPEN直完成拒", ok, if ok { "" } else { "完成闸失效" });
    // J3-1 同维度 OPEN 互斥
    let mut book2 = ReservationBook::new();
    let _ = book2.open(DIM_NOISE, 1, 1);
    let ok = book2.open(DIM_NOISE, 2, 2) == Err(RSV_DUP_OPEN);
    s.add("56139-J3-同维互斥", ok, if ok { "" } else { "唯一性闸失效" });
    // J3-2 DONE 后可再开（互斥只对 OPEN）
    let mut book2 = standard_book();
    let ok = book2.open(DIM_NOISE, 3, 7).is_ok();
    s.add("56139-J3-DONE后再开", ok, if ok { "" } else { "互斥过宽" });
    // J3-3 优先级仲裁：同波次 OPEN 降序（p30 > p5）
    let book3 = standard_book();
    let open = book3.open_by_prio(2);
    let ok = open.len() == 1 && open[0] == 2;
    s.add("56139-J3-仲裁导出", ok, if ok { "" } else { "仲裁漂移" });
    // J3-4 优先级仲裁多路：两 OPEN 同波次按 prio 降序
    let mut book3 = ReservationBook::new();
    let a = book3.open(DIM_NOISE, 1, 5).unwrap_or(0);
    let b = book3.open(DIM_BAND, 1, 50).unwrap_or(1);
    let open = book3.open_by_prio(1);
    let ok = open.len() == 2 && open[0] == b && open[1] == a;
    s.add("56139-J3-多路仲裁", ok, if ok { "" } else { "仲裁排序漂移" });
    // J3-5 OPEN 可改优先级
    let mut book3 = ReservationBook::new();
    let idx = book3.open(DIM_NOISE, 1, 1).unwrap_or(0);
    let ok = book3.set_prio(idx, 42).is_ok()
        && book3.item(idx).map(|r| r.prio == 42).unwrap_or(false);
    s.add("56139-J3-OPEN可改prio", ok, if ok { "" } else { "优先级面漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56139-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56139_all_checks_green() {
        let set = run_unx56139_checks();
        assert!(set.all_passed(), "UNX-F56139 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56139_state_machine_stable() {
        assert!(transition_ok(ST_OPEN, ST_LOCKED));
        assert!(!transition_ok(ST_DONE, ST_DONE));
    }
}
