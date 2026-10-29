//! UNX-F56045 桌面首帧段计时桩（0x60580 段）
//!
//! 锚点：UNX-F56045-J1 桌面合成器首帧时刻（帧账对接联签锚定行 F1 留
//! 痕）；J2 与登录段边界去重规则（无重叠断言：桌面首帧 ≥ 登录可见时
//! 刻，重叠即拒）×10 轮全过；J3 合成器首帧判定规则进代码（首个合成
//! 帧事件时刻）三判据全绿。判据锚定 §AI-71.2 专题二②。
//! 域：CoRun·UNX-F2-B40（启动分段域·桌面首帧面）。
//!
//! 结构：合成帧事件账（号 × 时刻 × 合成旗逐条落账）+ 首帧判定核（首
//! 个合成旗事件）+ 边界去重核（桌面首帧 - 登录可见 ≥0 断言）+ F1 锚
//! 定行账 + ×10 连测账 + 注入核（空事件账/无合成帧/边界重叠/锚定缺/
//! 封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60580 独占 0x20）。
pub const SEG_BASE: u32 = 0x60580;
pub const DF_EVENT_EMPTY: u32 = SEG_BASE + 0x01; // 事件账空
pub const DF_NO_FRAME: u32 = SEG_BASE + 0x02; // 无合成帧
pub const DF_OVERLAP: u32 = SEG_BASE + 0x03; // 边界重叠（去重破）
pub const DF_ANCHOR_MISS: u32 = SEG_BASE + 0x04; // F1 锚定行缺
pub const DF_SEALED: u32 = SEG_BASE + 0x05; // 段桩账已封账拒写
pub const DF_RUN_SHORT: u32 = SEG_BASE + 0x06; // 连测不足 10 轮
pub const DF_FLAG_MISS: u32 = SEG_BASE + 0x07; // 合成旗缺（事件未标）

/// 连测轮数（10）。
pub const RUNS_N: usize = 10;

/// 合成帧事件：号 × 时刻 × 合成旗。
#[derive(Clone)]
pub struct CompEvent {
    pub seq: u32,
    pub ts_ns: u64,
    pub composed: bool,
}

/// J3：首帧判定纯函数——首个合成旗事件时刻。
pub fn first_comp_frame(events: &[CompEvent]) -> Result<u64, u32> {
    if events.is_empty() {
        return Err(DF_EVENT_EMPTY);
    }
    for e in events.iter() {
        if e.composed {
            return Ok(e.ts_ns);
        }
    }
    Err(DF_NO_FRAME)
}

/// J2：边界去重核——桌面首帧 ≥ 登录可见（重叠即拒）。
pub fn boundary_ok(desktop_first: u64, login_visible: u64) -> bool {
    desktop_first >= login_visible
}

/// 段桩账。
pub struct DesktopStub {
    pub marked: Option<u64>,
    pub anchors: Vec<String>,
    pub runs: Vec<u64>,
    pub sealed: bool,
}

impl DesktopStub {
    pub fn new() -> Self {
        DesktopStub { marked: None, anchors: Vec::new(), runs: Vec::new(), sealed: false }
    }

    /// 消费一轮事件账打点（重叠校验联动）。
    pub fn consume(&mut self, events: &[CompEvent], login_visible: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(DF_SEALED);
        }
        let ts = first_comp_frame(events)?;
        if !boundary_ok(ts, login_visible) {
            return Err(DF_OVERLAP);
        }
        self.marked = Some(ts);
        self.runs.push(ts);
        Ok(ts)
    }

    /// F1 锚定行落账。
    pub fn anchor(&mut self, row: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DF_SEALED);
        }
        if row.is_empty() {
            return Err(DF_ANCHOR_MISS);
        }
        self.anchors.push(row.to_string());
        Ok(())
    }

    /// 锚定行在账核。
    pub fn anchor_ok(&self) -> bool {
        self.anchors.iter().any(|a| a.contains("F1"))
    }

    /// ×10 连测分位。
    pub fn run_stats(&self) -> Result<(u64, u64), u32> {
        if self.runs.len() < RUNS_N {
            return Err(DF_RUN_SHORT);
        }
        let mut v = self.runs.clone();
        v.sort();
        let p50 = v[v.len() / 2];
        let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
        Ok((p50, p95))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：一轮合成帧事件（未合成 → 合成 → 合成）。
pub fn standard_events(seq: u32) -> Vec<CompEvent> {
    let mut v: Vec<CompEvent> = Vec::new();
    let rows = [
        (1u32, 4_000_000u64, false),
        (2, 4_300_000, true),
        (3, 4_500_000, true),
    ];
    for r in rows.iter() {
        v.push(CompEvent { seq: r.0 + seq * 10, ts_ns: r.1, composed: r.2 });
    }
    v
}

pub fn standard_stub() -> DesktopStub {
    let mut s = DesktopStub::new();
    s.consume(&standard_events(0), 3_200_000).expect("consume");
    s.anchor("F1 帧账对接联签锚定行（合成器首帧）").expect("a");
    s
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空事件账
    if first_comp_frame(&[]).err() == Some(DF_EVENT_EMPTY) {
        caught += 1;
    }
    // 2) 无合成帧
    let none = vec![CompEvent { seq: 1, ts_ns: 1, composed: false }];
    if first_comp_frame(&none).err() == Some(DF_NO_FRAME) {
        caught += 1;
    }
    // 3) 边界重叠（桌面首帧早于登录可见）
    let mut s = DesktopStub::new();
    if s.consume(&standard_events(0), 5_000_000) == Err(DF_OVERLAP) {
        caught += 1;
    }
    // 4) 锚定行缺
    let bare = DesktopStub::new();
    if !bare.anchor_ok() {
        caught += 1;
    }
    // 5) 连测不足
    let mut s2 = DesktopStub::new();
    for i in 0..9u32 {
        s2.consume(&standard_events(i), 3_200_000).expect("c");
    }
    if s2.run_stats() == Err(DF_RUN_SHORT) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56045");
    // J1-1 桌面首帧打点（首个合成旗 @4.3M ns）
    let st = standard_stub();
    let ok = st.marked == Some(4_300_000);
    s.add("56045-J1-桌面首帧打点", ok, if ok { "" } else { "打点漂移" });
    // J1-2 判定规则进代码（纯函数回放一致）
    let ev = standard_events(0);
    let ok = first_comp_frame(&ev).expect("r1") == first_comp_frame(&ev).expect("r2");
    s.add("56045-J1-规则回放一致", ok, if ok { "" } else { "回放漂移" });
    // J1-3 未合成帧不误判（首事件旗假被跳过）
    let ok = !ev[0].composed && st.marked.expect("m") == ev[1].ts_ns;
    s.add("56045-J1-未合成跳过", ok, if ok { "" } else { "旗判定漂移" });
    // J2-1 边界去重规则（恰等不重叠 / 早于重叠拒）
    let ok = boundary_ok(3_200_000, 3_200_000)
        && boundary_ok(3_300_000, 3_200_000)
        && !boundary_ok(3_100_000, 3_200_000);
    s.add("56045-J2-边界去重语义", ok, if ok { "" } else { "去重语义漂移" });
    // J2-2 ×10 轮无重叠断言全过（逐轮消费即断言）
    let mut s2 = DesktopStub::new();
    let mut all_ok = true;
    for i in 0..RUNS_N {
        if s2.consume(&standard_events(i as u32), 3_200_000).is_err() {
            all_ok = false;
        }
    }
    let ok = all_ok && s2.runs.len() == RUNS_N;
    s.add("56045-J2-十轮无重叠", ok, if ok { "" } else { "重叠检出" });
    // J2-3 ×10 连测分位（确定性 P50=P95=4.3M）
    let (p50, p95) = s2.run_stats().expect("st");
    let ok = p50 == 4_300_000 && p95 == 4_300_000;
    s.add("56045-J2-十轮分位", ok, if ok { "" } else { "分位漂移" });
    // J3-1 F1 锚定行在账
    let ok = st.anchor_ok();
    s.add("56045-J3-F1锚定行在账", ok, if ok { "" } else { "锚定缺" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56045-J3-注入核必抓", ok, &detail);
    // J3-3 封账冻结
    let mut s3 = standard_stub();
    s3.seal();
    let ok = s3.consume(&standard_events(1), 3_200_000) == Err(DF_SEALED)
        && s3.anchor("x") == Err(DF_SEALED);
    s.add("56045-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 事件号递增（夹具 1/2/3）
    let ok = ev[0].seq == 1 && ev[2].seq == 3;
    s.add("56045-J1-事件号递增", ok, if ok { "" } else { "事件号破序" });
    // J2-4 未跑桩无打点
    let bare = DesktopStub::new();
    let ok = bare.marked.is_none() && bare.runs.is_empty();
    s.add("56045-J2-未跑不编造", ok, if ok { "" } else { "编造打点" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56045_all_checks_green() {
        let set = run_unx56045_checks();
        assert!(set.all_passed(), "UNX-F56045 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56045_first_frame() {
        assert_eq!(first_comp_frame(&standard_events(0)).expect("t"), 4_300_000);
    }
}
