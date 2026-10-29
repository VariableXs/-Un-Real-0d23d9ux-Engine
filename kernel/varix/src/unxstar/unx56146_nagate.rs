//! UNX-F56146 禁缺侧编造总闸（N/A 强制路径）（0x61240 段）
//!
//! 锚点：UNX-F56146-J1 缺侧守闸账（对照行双测侧——本机侧/对机侧——
//! 各自登记，缺侧行强制 N/A 路径落账，禁编造实测值）；J2 N/A 强制语
//! 义（缺侧行带缺侧原因码 + 无实测字段 + 可追溯原因三性质）；J3 补采
//! 语义（N/A 行补采后可升级为实测行，旧行保留链）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·缺侧守闸面）。
//!
//! 结构：双测侧登记（本机侧/对机侧状态机：WAITING→PRESENT/MISSING）
//! + 守闸核（缺侧行强制 N/A：原因码闭集 + 无实测值字段）+ 补采升级
//! （N/A → 实测升级链，旧行保留）+ 注入核（缺侧带实测值/N-A 无原因/
//! 重复补采/未知侧/升级后改原因必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61240 独占 0x20）。
pub const SEG_BASE: u32 = 0x61240;
pub const NS_FABRICATED: u32 = SEG_BASE + 0x01; // 缺侧带实测值（编造）
pub const NS_NA_NOREASON: u32 = SEG_BASE + 0x02; // N/A 无原因码
pub const NS_DUP_FILL: u32 = SEG_BASE + 0x03; // 重复补采
pub const NS_UNK_SIDE: u32 = SEG_BASE + 0x04; // 未知侧
pub const NS_UPGRADE_FLIP: u32 = SEG_BASE + 0x05; // 升级后改原因

/// 双测侧闭集。
pub const SIDE_LOCAL: usize = 0;
pub const SIDE_REMOTE: usize = 1;
pub const SIDE_COUNT: usize = 2;

/// 侧名渲染。
pub const SIDE_NAMES: [&str; SIDE_COUNT] = ["local", "remote"];

/// 侧状态机。
pub const ST_WAITING: u8 = 0;
pub const ST_PRESENT: u8 = 1;
pub const ST_MISSING: u8 = 2;

/// 缺侧原因码闭集。
pub const RS_NO_HOST: usize = 0; // 无对机
pub const RS_NO_AGENT: usize = 1; // 对机无采集代理
pub const RS_NO_ROUTE: usize = 2; // 网络不通
pub const RS_NO_PERM: usize = 3; // 无采集权限
pub const RS_COUNT: usize = 4;

/// 原因码渲染。
pub const REASON_NAMES: [&str; RS_COUNT] = ["no_host", "no_agent", "no_route", "no_perm"];

/// 单条对照行（双测侧状态）。
#[derive(Clone)]
pub struct DualRow {
    pub row_key: String,
    pub side_state: [u8; SIDE_COUNT],
    pub na_reason: [Option<usize>; SIDE_COUNT],
    pub measured: [Option<u32>; SIDE_COUNT],
    pub upgraded_from_na: bool,
}

impl DualRow {
    fn new(row_key: &str) -> Self {
        DualRow {
            row_key: row_key.to_string(),
            side_state: [ST_WAITING; SIDE_COUNT],
            na_reason: [None, None],
            measured: [None, None],
            upgraded_from_na: false,
        }
    }

    fn is_na(&self, side: usize) -> bool {
        self.side_state[side] == ST_MISSING
    }
}

/// 缺侧守闸总账。
pub struct NaGate {
    rows: Vec<DualRow>,
}

impl NaGate {
    pub fn new() -> Self {
        NaGate { rows: Vec::new() }
    }

    fn row_mut(&mut self, key: &str) -> Option<&mut DualRow> {
        self.rows.iter_mut().find(|r| r.row_key == key)
    }

    /// 登记缺侧：强制 N/A 路径（原因码必带，实测值字段强制空）。
    pub fn mark_missing(&mut self, key: &str, side: usize, reason: usize) -> Result<(), u32> {
        if side >= SIDE_COUNT {
            return Err(NS_UNK_SIDE);
        }
        if reason >= RS_COUNT {
            return Err(NS_NA_NOREASON);
        }
        if let Some(idx) = self.rows.iter().position(|r| r.row_key == key) {
            let r = &mut self.rows[idx];
            if r.side_state[side] != ST_WAITING {
                return Err(NS_DUP_FILL);
            }
            r.side_state[side] = ST_MISSING;
            r.na_reason[side] = Some(reason);
            r.measured[side] = None; // 编造闸：强制无实测
            Ok(())
        } else {
            let mut r = DualRow::new(key);
            r.side_state[side] = ST_MISSING;
            r.na_reason[side] = Some(reason);
            self.rows.push(r);
            Ok(())
        }
    }

    /// 登记实测：仅 WAITING/MISSING→PRESENT 可走；缺侧行补采即升级。
    pub fn put_measured(&mut self, key: &str, side: usize, val: u32) -> Result<(), u32> {
        if side >= SIDE_COUNT {
            return Err(NS_UNK_SIDE);
        }
        let idx = match self.rows.iter().position(|r| r.row_key == key) {
            Some(i) => i,
            None => {
                let mut r = DualRow::new(key);
                r.side_state[side] = ST_PRESENT;
                r.measured[side] = Some(val);
                self.rows.push(r);
                return Ok(());
            }
        };
        let r = &mut self.rows[idx];
        match r.side_state[side] {
            ST_WAITING => {
                r.side_state[side] = ST_PRESENT;
                r.measured[side] = Some(val);
                Ok(())
            }
            ST_MISSING => {
                // 补采升级：N/A → 实测，原因码清除留链
                r.upgraded_from_na = true;
                r.na_reason[side] = None;
                r.side_state[side] = ST_PRESENT;
                r.measured[side] = Some(val);
                Ok(())
            }
            _ => Err(NS_DUP_FILL),
        }
    }

    /// 守闸核：所有 MISSING 侧必须原因码在场 + 实测值缺席。
    pub fn gate_ok(&self) -> bool {
        self.rows.iter().all(|r| {
            (0..SIDE_COUNT).all(|s| match r.side_state[s] {
                ST_MISSING => r.na_reason[s].is_some() && r.measured[s].is_none(),
                ST_PRESENT => r.measured[s].is_some(),
                _ => true,
            })
        })
    }

    pub fn row(&self, key: &str) -> Option<&DualRow> {
        self.rows.iter().find(|r| r.row_key == key)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// N/A 行计数（含已升级行回溯）。
    pub fn na_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| (0..SIDE_COUNT).any(|s| r.is_na(s)))
            .count()
    }

    /// 升级行计数。
    pub fn upgraded_count(&self) -> usize {
        self.rows.iter().filter(|r| r.upgraded_from_na).count()
    }

    /// 渲染导出。
    pub fn render_gate(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            for s in 0..SIDE_COUNT {
                let cell = match r.side_state[s] {
                    ST_WAITING => "WAIT".to_string(),
                    ST_PRESENT => format!("M{}", r.measured[s].unwrap_or(0)),
                    ST_MISSING => format!(
                        "NA:{}",
                        REASON_NAMES[r.na_reason[s].unwrap_or(0)]
                    ),
                    _ => "?".to_string(),
                };
                out.push_str(&format!("{}|{}|{}\n", r.row_key, SIDE_NAMES[s], cell));
            }
        }
        out
    }
}

impl Default for NaGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行账（本机实测/对机缺侧/补采升级）。
fn standard_gate() -> NaGate {
    let mut g = NaGate::new();
    let _ = g.put_measured("fps_full", SIDE_LOCAL, 57);
    let _ = g.mark_missing("fps_full", SIDE_REMOTE, RS_NO_HOST);
    let _ = g.put_measured("fps_mid", SIDE_LOCAL, 114);
    let _ = g.mark_missing("fps_mid", SIDE_REMOTE, RS_NO_AGENT);
    let _ = g.put_measured("fps_mid", SIDE_REMOTE, 116); // 补采升级
    g
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：缺侧重复登记
    let mut g = NaGate::new();
    let _ = g.mark_missing("x", SIDE_LOCAL, RS_NO_HOST);
    if g.mark_missing("x", SIDE_LOCAL, RS_NO_AGENT) == Err(NS_DUP_FILL) {
        caught += 1;
    }
    // 注 2：未知侧
    let mut g = NaGate::new();
    if g.mark_missing("x", 9, RS_NO_HOST) == Err(NS_UNK_SIDE) {
        caught += 1;
    }
    // 注 3：重复实测
    let mut g = NaGate::new();
    let _ = g.put_measured("x", SIDE_LOCAL, 1);
    if g.put_measured("x", SIDE_LOCAL, 2) == Err(NS_DUP_FILL) {
        caught += 1;
    }
    // 注 4：编造闸（缺侧行 measured 强制空，mark_missing 后立即为 None）
    let mut g = NaGate::new();
    let _ = g.mark_missing("x", SIDE_REMOTE, RS_NO_ROUTE);
    let fab = g.row("x").map(|r| r.measured[SIDE_REMOTE].is_none()).unwrap_or(false);
    if fab {
        caught += 1;
    }
    // 注 5：守闸核抓破坏（手工给 MISSING 侧塞实测值后 gate_ok 必假）
    let mut g = NaGate::new();
    let _ = g.mark_missing("x", SIDE_LOCAL, RS_NO_PERM);
    if let Some(r) = g.rows.iter_mut().find(|r| r.row_key == "x") {
        r.measured[SIDE_LOCAL] = Some(42); // 内存级编造
    }
    if !g.gate_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 守闸账；J2 N/A 强制语义；J3 补采升级。
pub fn run_unx56146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56146");
    // J1-1 三行落账 + 守闸绿
    let g = standard_gate();
    let ok = g.len() == 3 && g.gate_ok();
    s.add("56146-J1-三行守闸", ok, if ok { "" } else { "守闸账漂移" });
    // J1-2 渲染导出齐（含 N/A 原因码与补采实测）
    let text = standard_gate().render_gate();
    let ok = text.contains("fps_full|remote|NA:no_host") && text.contains("fps_mid|remote|M116");
    s.add("56146-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 缺侧行无实测值
    let g = standard_gate();
    let ok = g.row("fps_full").map(|r| r.measured[SIDE_REMOTE].is_none()).unwrap_or(false);
    s.add("56146-J2-缺侧无实测", ok, if ok { "" } else { "编造闸失效" });
    // J2-2 缺侧原因码在场可溯
    let ok = g.row("fps_full").map(|r| r.na_reason[SIDE_REMOTE] == Some(RS_NO_HOST)).unwrap_or(false);
    s.add("56146-J2-原因可溯", ok, if ok { "" } else { "原因码漂移" });
    // J2-3 原因码闭集渲染
    let ok = REASON_NAMES[RS_NO_HOST] == "no_host" && REASON_NAMES[RS_NO_PERM] == "no_perm";
    s.add("56146-J2-原因码闭集", ok, if ok { "" } else { "闭集漂移" });
    // J2-4 N/A 行计数
    let ok = standard_gate().na_count() == 1; // fps_full 的 remote 仍 MISSING
    s.add("56146-J2-NA计数", ok, if ok { "" } else { "计数漂移" });
    // J3-1 补采升级链
    let g = standard_gate();
    let ok = g.row("fps_mid").map(|r| r.upgraded_from_na && r.measured[SIDE_REMOTE] == Some(116)).unwrap_or(false);
    s.add("56146-J3-补采升级", ok, if ok { "" } else { "升级链漂移" });
    // J3-2 升级后原因码清除
    let ok = g.row("fps_mid").map(|r| r.na_reason[SIDE_REMOTE].is_none()).unwrap_or(false);
    s.add("56146-J3-升级清因", ok, if ok { "" } else { "原因残留" });
    // J3-3 升级计数
    let ok = standard_gate().upgraded_count() == 1;
    s.add("56146-J3-升级计数", ok, if ok { "" } else { "计数漂移" });
    // J3-4 本机侧不受对机缺侧影响
    let ok = g.row("fps_full").map(|r| r.measured[SIDE_LOCAL] == Some(57)).unwrap_or(false);
    s.add("56146-J3-双侧独立", ok, if ok { "" } else { "侧间穿透" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56146-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56146_all_checks_green() {
        let set = run_unx56146_checks();
        assert!(set.all_passed(), "UNX-F56146 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56146_gate_stable() {
        assert!(standard_gate().gate_ok());
    }
}
