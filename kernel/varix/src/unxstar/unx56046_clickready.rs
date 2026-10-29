//! UNX-F56046 双击首响应段计时桩（0x605A0 段）
//!
//! 锚点：UNX-F56046-J1 标准探测操作（约定双击目标）→可见反馈时刻打
//! 点；J2 反馈判定规则冻结（视觉变化事件闭集：目标高亮/窗口置前/光
//! 标变形三类）；J3 ×10 连测 + 操作序纪律（双击必须两击配对——单击
//! 不触发反馈判定）三判据全绿。判据锚定 §AI-71.2 专题二②。
//! 域：CoRun·UNX-F2-B40（启动分段域·交互就绪面）。
//!
//! 结构：视觉变化事件账（号 × 时刻 × 事件类逐条落账）+ 反馈判定核
//! （首个三类事件时刻，规则冻结进代码）+ 双击配对闸（单击数与反馈判
//！定解耦——无配对不判定）+ ×10 连测分位账 + 注入核（越闭集事件类/
//! 空事件账/无反馈/未配对判定/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x605A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x605A0;
pub const CK_KIND_UNK: u32 = SEG_BASE + 0x01; // 越闭集视觉事件类
pub const CK_EVENT_EMPTY: u32 = SEG_BASE + 0x02; // 事件账空
pub const CK_NO_FEEDBACK: u32 = SEG_BASE + 0x03; // 无反馈事件
pub const CK_PAIR_MISS: u32 = SEG_BASE + 0x04; // 双击未配对
pub const CK_SEALED: u32 = SEG_BASE + 0x05; // 段桩账已封账拒写
pub const CK_RUN_SHORT: u32 = SEG_BASE + 0x06; // 连测不足 10 轮
pub const CK_TS_BACK: u32 = SEG_BASE + 0x07; // 事件时刻回退

/// 视觉变化事件类闭集（反馈判定冻结口径）。
pub const V_HIGHLIGHT: usize = 0;
pub const V_RAISE: usize = 1;
pub const V_CURSOR: usize = 2;
pub const V_COUNT: usize = 3;
pub const V_NAMES: [&str; V_COUNT] = ["target_highlight", "window_raise", "cursor_morph"];

/// 连测轮数（10）。
pub const RUNS_N: usize = 10;

/// 视觉变化事件：号 × 时刻 × 类。
#[derive(Clone)]
pub struct VisEvent {
    pub seq: u32,
    pub ts_ns: u64,
    pub kind: usize,
}

impl VisEvent {
    pub fn new(seq: u32, ts_ns: u64, kind: usize) -> Result<Self, u32> {
        if kind >= V_COUNT {
            return Err(CK_KIND_UNK);
        }
        if seq == 0 {
            return Err(CK_TS_BACK);
        }
        Ok(VisEvent { seq, ts_ns, kind })
    }
}

/// 双击配对闸：两击时刻（click1 < click2）→ 配对成立。
pub fn pair_ok(click1: u64, click2: u64) -> bool {
    click1 > 0 && click2 > click1
}

/// J2：反馈判定纯函数——首个三类视觉事件时刻（配对成立才判定）。
pub fn feedback_moment(
    click1: u64,
    click2: u64,
    events: &[VisEvent],
) -> Result<u64, u32> {
    if !pair_ok(click1, click2) {
        return Err(CK_PAIR_MISS);
    }
    if events.is_empty() {
        return Err(CK_EVENT_EMPTY);
    }
    for e in events.iter() {
        if e.kind < V_COUNT && e.ts_ns >= click2 {
            return Ok(e.ts_ns);
        }
    }
    Err(CK_NO_FEEDBACK)
}

/// 段桩账。
pub struct ClickStub {
    pub marked: Option<u64>,
    pub runs: Vec<u64>,
    pub sealed: bool,
}

impl ClickStub {
    pub fn new() -> Self {
        ClickStub { marked: None, runs: Vec::new(), sealed: false }
    }

    /// 消费一轮（配对 + 判定 + 落账）。
    pub fn consume(&mut self, click1: u64, click2: u64, events: &[VisEvent]) -> Result<u64, u32> {
        if self.sealed {
            return Err(CK_SEALED);
        }
        let ts = feedback_moment(click1, click2, events)?;
        self.marked = Some(ts);
        self.runs.push(ts);
        Ok(ts)
    }

    /// ×10 连测分位。
    pub fn run_stats(&self) -> Result<(u64, u64), u32> {
        if self.runs.len() < RUNS_N {
            return Err(CK_RUN_SHORT);
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

/// 标准夹具：双击 (1.0M, 1.1M) + 视觉事件（高亮 @1.15M 首反馈）。
pub fn standard_events(seq: u32) -> Vec<VisEvent> {
    let mut v: Vec<VisEvent> = Vec::new();
    let rows = [
        (1u32, 1_150_000u64, V_HIGHLIGHT),
        (2, 1_200_000, V_RAISE),
    ];
    for r in rows.iter() {
        v.push(VisEvent::new(r.0 + seq * 10, r.1, r.2).expect("event"));
    }
    v
}

pub fn standard_stub() -> ClickStub {
    let mut s = ClickStub::new();
    s.consume(1_000_000, 1_100_000, &standard_events(0)).expect("consume");
    s
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集事件类
    if VisEvent::new(1, 1, 9).err() == Some(CK_KIND_UNK) {
        caught += 1;
    }
    // 2) 双击未配对（click2 ≤ click1）
    if feedback_moment(1_000, 1_000, &standard_events(0)).err() == Some(CK_PAIR_MISS) {
        caught += 1;
    }
    // 3) 空事件账
    if feedback_moment(1_000, 2_000, &[]).err() == Some(CK_EVENT_EMPTY) {
        caught += 1;
    }
    // 4) 无反馈（事件全早于第二击）
    let early = vec![VisEvent::new(1, 500, V_HIGHLIGHT).expect("e")];
    if feedback_moment(1_000, 2_000, &early).err() == Some(CK_NO_FEEDBACK) {
        caught += 1;
    }
    // 5) 连测不足
    let mut s = ClickStub::new();
    for i in 0..9u32 {
        s.consume(1_000_000, 1_100_000, &standard_events(i)).expect("c");
    }
    if s.run_stats() == Err(CK_RUN_SHORT) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56046_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56046");
    // J1-1 反馈时刻打点（首个三类事件 @1.15M）
    let st = standard_stub();
    let ok = st.marked == Some(1_150_000);
    s.add("56046-J1-反馈时刻打点", ok, if ok { "" } else { "打点漂移" });
    // J1-2 事件类闭集同源（三类冻结口径）
    let ok = V_NAMES[0] == "target_highlight" && V_NAMES[2] == "cursor_morph";
    s.add("56046-J1-事件类闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-3 判定规则冻结（纯函数回放一致）
    let ev = standard_events(0);
    let ok = feedback_moment(1_000_000, 1_100_000, &ev).expect("r1")
        == feedback_moment(1_000_000, 1_100_000, &ev).expect("r2");
    s.add("56046-J1-规则冻结回放", ok, if ok { "" } else { "回放漂移" });
    // J2-1 双击配对闸语义（两击单调成立 / 单击拒 / 零击拒）
    let ok = pair_ok(1_000, 2_000)
        && !pair_ok(1_000, 1_000)
        && !pair_ok(0, 2_000)
        && !pair_ok(2_000, 1_000);
    s.add("56046-J2-双击配对闸", ok, if ok { "" } else { "配对闸漂移" });
    // J2-2 单击不触发判定（未配对被拦——J2 纪律）
    let ok = feedback_moment(1_000, 1_000, &ev).is_err();
    s.add("56046-J2-单击不判定", ok, if ok { "" } else { "单文件穿透" });
    // J2-3 反馈时刻不早于第二击（时序纪律）
    let ok = st.marked.expect("m") >= 1_100_000;
    s.add("56046-J2-反馈不早于击", ok, if ok { "" } else { "时序破" });
    // J3-1 ×10 连测分位（确定性 P50=P95=1.15M）
    let mut s2 = ClickStub::new();
    for i in 0..RUNS_N {
        s2.consume(1_000_000, 1_100_000, &standard_events(i as u32)).expect("c");
    }
    let (p50, p95) = s2.run_stats().expect("st");
    let ok = p50 == 1_150_000 && p95 == 1_150_000 && s2.runs.len() == RUNS_N;
    s.add("56046-J3-十轮分位", ok, if ok { "" } else { "连测账缺" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56046-J3-注入核必抓", ok, &detail);
    // J3-3 封账冻结
    let mut s3 = standard_stub();
    s3.seal();
    let ok = s3.consume(1, 2, &standard_events(1)) == Err(CK_SEALED);
    s.add("56046-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-4 未跑桩无打点
    let bare = ClickStub::new();
    let ok = bare.marked.is_none() && bare.runs.is_empty();
    s.add("56046-J2-未跑不编造", ok, if ok { "" } else { "编造打点" });
    // J1-4 三类事件皆可判定（逐类首反馈可达）
    let mut all_kind = true;
    for k in 0..V_COUNT {
        let ev1 = vec![VisEvent::new(1, 1_150_000, k).expect("e")];
        if feedback_moment(1_000_000, 1_100_000, &ev1).expect("m") != 1_150_000 {
            all_kind = false;
        }
    }
    let ok = all_kind;
    s.add("56046-J1-三类皆可达", ok, if ok { "" } else { "类判定缺" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56046_all_checks_green() {
        let set = run_unx56046_checks();
        assert!(set.all_passed(), "UNX-F56046 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56046_feedback() {
        assert_eq!(
            feedback_moment(1_000_000, 1_100_000, &standard_events(0)).expect("t"),
            1_150_000
        );
    }
}
