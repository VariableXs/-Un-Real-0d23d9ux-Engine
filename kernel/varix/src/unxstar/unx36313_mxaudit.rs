//! UNX-F36313-J1 矩阵与 J5 审计的事件对账（应拒格事件不漏发）。
//! 判据：① 应拒格审计事件 100% 产生且 ≤10ms（fake 通道）；② 漏
//! 发/多发注入 10 例被对账器检出；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权审计事件通道）：每个拒格在求值时
//! 必须同步产生一条审计事件（事件三要素：格号、归因门、时延），
//! 事件流与拒格账**双向对账**：应发集合（150 拒格）里每格恰有
//! 一条、事件流里每条对应一个应发格——漏发一格 = 拒绝没留痕、
//! 多发一格 = 账面有幽灵，都是审计破。时延账：fake 通道按事件
//! 出口排队深度建模（base 8_000 单位 + 门序号），逐条 ≤ 10ms
//! 预算（10_000 单位），超预算 = 通道拥塞 = 账面破。对账器用注
//! 入负例自检：构造漏发 5 条 + 多发 5 条的坏流，对账器必须精确
//! 报出 10 处差（漏报/误报都是对账器破）——对账器自身也要过账。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB230;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_EMIT: u16 = SEG_BASE + 1; // 事件产生面破（应发未发/三要素缺）
pub const E_LATENCY: u16 = SEG_BASE + 2; // 时延出 10ms 预算
pub const E_RECON: u16 = SEG_BASE + 3; // 对账器破（漏检/误报/计数错）
pub const E_FROZEN: u16 = SEG_BASE + 4; // 事件总数漂移（150）
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 应发事件总数冻结锚（150 拒格 = 240 - 90 许）。
pub const FROZEN_EVENTS: u32 = 150;
/// fake 通道预算：10ms @ 1 单位 = 1µs。
pub const LATENCY_BUDGET: u32 = 10_000;
/// 定容通道容量（超容即拒收——显性不静默）。
pub const CHANNEL_CAP: usize = 160;

/// 审计事件（三要素齐备：格号/归因门/时延）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    pub cell: u32,
    pub gate: u8,
    pub latency: u32,
}

/// 定容事件通道（拒收计数显性记账）。
pub struct EventChannel {
    buf: Vec<AuditEvent>,
    rejected: u32,
}

impl EventChannel {
    pub fn new() -> EventChannel {
        EventChannel { buf: Vec::new(), rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 入通道（超容拒收——账面显性，不静默丢）。
    pub fn push(&mut self, ev: AuditEvent) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if self.buf.len() >= CHANNEL_CAP {
            self.rejected += 1;
            return Err(E_EMIT);
        }
        if ev.cell == 0 || ev.cell as usize > mx::N_CELLS || ev.gate > mx::A_DENY {
            return Err(E_EMIT); // 三要素缺/域错
        }
        self.buf.push(ev);
        Ok(())
    }

    pub fn events(&self) -> &[AuditEvent] {
        &self.buf
    }
}

// ---- 应发集合（应拒格账——从语义书独立推导） -------------------------------------
pub fn expected_cells() -> Result<Vec<u32>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut cells = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        if mx::spec_at(id)? != mx::A_ALLOW {
            cells.push(id);
        }
    }
    if cells.len() as u32 != FROZEN_EVENTS {
        return Err(E_FROZEN);
    }
    Ok(cells)
}

// ---- 事件产生面（判据 ①：应拒格 100% 产生且 ≤10ms） ------------------------------
// 走真引擎逐格：拒格即发事件（时延 = 8_000 + 门序号——fake 出
// 口排队模型），许格不发。事件流入定容通道，逐条时延断言。
pub fn emit_events() -> Result<EventChannel, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut chan = EventChannel::new();
    for id in 1..=mx::N_CELLS as u32 {
        let (s, o, op) = mx::cell_parts(id)?;
        let code = mx::evaluate(s, o, op)?;
        if code == mx::A_ALLOW {
            continue;
        }
        let ev = AuditEvent {
            cell: id,
            gate: code,
            latency: 8_000 + code as u32,
        };
        if ev.latency > LATENCY_BUDGET {
            return Err(E_LATENCY);
        }
        chan.push(ev)?;
    }
    if chan.len() as u32 != FROZEN_EVENTS || chan.rejected() != 0 {
        return Err(E_EMIT);
    }
    Ok(chan)
}

/// 通道全量核：三要素域合法 + 时延全预算内 + 格号升序（产生序
/// 与格号序一致——通道序的确定性）。
pub fn channel_audit(chan: &EventChannel) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut prev = 0u32;
    let mut n = 0u32;
    for ev in chan.events().iter() {
        if ev.cell <= prev {
            return Err(E_EMIT); // 序乱 = 通道确定性破
        }
        prev = ev.cell;
        if ev.latency == 0 || ev.latency > LATENCY_BUDGET {
            return Err(E_LATENCY);
        }
        n += 1;
    }
    if n != FROZEN_EVENTS {
        return Err(E_FROZEN);
    }
    Ok(n)
}

// ---- 对账器（判据 ② 的执行者） ---------------------------------------------------
// 差异计数：漏发（应发格无事件）+ 多发（事件格不在应发集）。按
// 格号有序双指针走——对账本身 O(n)，差异逐处落账。
pub fn reconcile(events: &[AuditEvent], expected: &[u32]) -> Result<Vec<(u32, u8)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut diffs = Vec::new();
    let mut i = 0usize; // 事件游标
    let mut j = 0usize; // 应发游标
    while j < expected.len() {
        let want = expected[j];
        let got = events.get(i).map(|e| e.cell);
        match got {
            Some(cell) if cell == want => {
                i += 1;
                j += 1;
            }
            Some(cell) if cell > want => {
                diffs.push((want, 1)); // 漏发
                j += 1;
            }
            Some(cell) => {
                diffs.push((cell, 2)); // 多发（幽灵事件）
                i += 1;
            }
            None => {
                diffs.push((want, 1)); // 流尽仍欠
                j += 1;
            }
        }
    }
    while i < events.len() {
        diffs.push((events[i].cell, 2));
        i += 1;
    }
    Ok(diffs)
}

/// 漏发/多发注入 10 例（判据 ②：对账器 10/10 检出）。
pub fn reconcile_injections() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let expected = expected_cells()?;
    let chan = emit_events()?;
    // 坏流构造：掐头 5 条（漏发）+ 尾部复读 5 条（多发）。
    let good = chan.events();
    let mut bad: Vec<AuditEvent> = Vec::new();
    for k in 5..good.len() {
        bad.push(good[k]);
    }
    for k in 0..5 {
        bad.push(good[k]);
    }
    let diffs = reconcile(&bad, &expected)?;
    // 精确检出：恰 10 处（漏 5 + 多 5），类型双向齐备。
    let missing = diffs.iter().filter(|(_, t)| *t == 1).count();
    let phantom = diffs.iter().filter(|(_, t)| *t == 2).count();
    if diffs.len() != 10 || missing != 5 || phantom != 5 {
        return Err(E_RECON);
    }
    // 健康流必须零差（对账器不冤枉好人）。
    if !reconcile(good, &expected)?.is_empty() {
        return Err(E_RECON);
    }
    Ok(diffs.len() as u32)
}

/// 事件账行（呈现面：格号 + 门名 + 时延）。
pub fn event_row_text(ev: &AuditEvent) -> String {
    let gate = mx::attribution_text(ev.gate);
    let mut text = String::from("格");
    text.push_str(&ev.cell.to_string());
    text.push(':');
    text.push_str(gate);
    text.push_str(" 时延=");
    text.push_str(&ev.latency.to_string());
    text
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let chan = emit_events()?;
    let n = channel_audit(&chan)?;
    let caught = reconcile_injections()?;
    // 呈现面抽核：首事件账行齐三要素。
    let first = chan.events().first().copied().unwrap_or(AuditEvent { cell: 0, gate: 0, latency: 0 });
    let row = event_row_text(&first);
    if !row.starts_with("格") || !row.contains("时延=") {
        return Err(E_EMIT);
    }
    if n != FROZEN_EVENTS || caught != 10 {
        return Err(E_FROZEN);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(caught))
}

// ---- 事件门分布账增补 -----------------------------------------------------------
/// 事件按归因门分布（拒格分布同构：DACL 69 / IL 13 / 特权 4 /
/// 掩码 24 / 过滤 40——事件流与拒格账在分布级对账，任何门的
/// 事件多算或漏算都在此显影）。
pub fn event_gate_distribution() -> Result<[u32; 5], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 5] = [69, 13, 4, 24, 40];
    let chan = emit_events()?;
    let mut tally = [0u32; 5];
    for ev in chan.events().iter() {
        let slot = match ev.gate {
            mx::A_DACL => 0,
            mx::A_IL => 1,
            mx::A_PRIV => 2,
            mx::A_MASK => 3,
            mx::A_DENY => 4,
            _ => return Err(E_EMIT),
        };
        tally[slot] += 1;
    }
    if tally != frozen {
        return Err(E_FROZEN);
    }
    Ok(tally)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_EMIT => "事件产生面破（应发未发/三要素缺/通道序乱）",
        E_LATENCY => "时延出 10ms 预算（通道拥塞）",
        E_RECON => "对账器破（漏检/误报/计数错）",
        E_FROZEN => "事件总数漂移（150）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36313_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36313_mxaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0xB230, "0xB230~0xB23F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "拒格事件 150/150·时延全预算内·注入 10 例精确检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("预算锚", LATENCY_BUDGET == 10_000, "10ms @ 1 单位 = 1µs（fake 通道）");
    cs.add("定容锚", CHANNEL_CAP == 160, "定容通道 160 槽（超容显性拒收）");
    cs.add("门分布账", event_gate_distribution() == Ok([69, 13, 4, 24, 40]), "事件流与拒格账分布同构 69/13/4/24/40");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36313_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36313_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
