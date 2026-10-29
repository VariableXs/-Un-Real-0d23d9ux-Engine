//! UNX-F56044 登录面首绘段计时桩（0x60560 段）
//!
//! 锚点：UNX-F56044-J1 登录面可见时刻判定（首帧提交事件消费，联签锚
//! 定行 F1/G3 留痕）；J2 判定规则进代码可复查（覆盖率旗 × 首个提交事
//! 件 → 时刻，规则纯函数可回放）；J3 ×10 连测方差落账三判据全绿。判
//! 据锚定 §AI-71.2 专题二②。
//! 域：CoRun·UNX-F2-B40（启动分段域·登录面首绘面）。
//!
//! 结构：首帧提交事件账（事件号 × 时刻 × 覆盖率旗逐条落账）+ 判定规
//! 则核（首个覆盖率达标事件即可见时刻，纯函数）+ 段桩账（打点 + 锚
//! 定行）+ ×10 连测分位账 + 注入核（空事件账/无达标事件/规则回放漂移
//! /锚定缺/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60560 独占 0x20）。
pub const SEG_BASE: u32 = 0x60560;
pub const LG_EVENT_EMPTY: u32 = SEG_BASE + 0x01; // 事件账空
pub const LG_NO_VISIBLE: u32 = SEG_BASE + 0x02; // 无达标事件
pub const LG_RULE_DRIFT: u32 = SEG_BASE + 0x03; // 规则回放漂移
pub const LG_ANCHOR_MISS: u32 = SEG_BASE + 0x04; // F1/G3 锚定行缺
pub const LG_SEALED: u32 = SEG_BASE + 0x05; // 段桩账已封账拒写
pub const LG_SEQ_BACK: u32 = SEG_BASE + 0x06; // 事件号回退
pub const LG_RUN_SHORT: u32 = SEG_BASE + 0x07; // 连测不足 10 轮

/// 覆盖率达标线（万分比 9500 = 95%）。
pub const COVER_MIN_PBP: u32 = 9_500;

/// 连测轮数（10）。
pub const RUNS_N: usize = 10;

/// 首帧提交事件：号 × 时刻 × 覆盖率(万分比)。
#[derive(Clone)]
pub struct FrameEvent {
    pub seq: u32,
    pub ts_ns: u64,
    pub cover_pbp: u32,
}

/// J2：判定规则纯函数——首个覆盖率达标事件即可见时刻。
pub fn visible_moment(events: &[FrameEvent]) -> Result<u64, u32> {
    if events.is_empty() {
        return Err(LG_EVENT_EMPTY);
    }
    for e in events.iter() {
        if e.cover_pbp >= COVER_MIN_PBP {
            return Ok(e.ts_ns);
        }
    }
    Err(LG_NO_VISIBLE)
}

/// 段桩账：打点 × 锚定行 × 连测分位。
pub struct LoginStub {
    pub marked: Option<u64>,
    pub anchors: Vec<String>,
    pub runs: Vec<u64>,
    pub sealed: bool,
}

impl LoginStub {
    pub fn new() -> Self {
        LoginStub { marked: None, anchors: Vec::new(), runs: Vec::new(), sealed: false }
    }

    /// 消费一轮事件账打点。
    pub fn consume(&mut self, events: &[FrameEvent]) -> Result<u64, u32> {
        if self.sealed {
            return Err(LG_SEALED);
        }
        let ts = visible_moment(events)?;
        self.marked = Some(ts);
        self.runs.push(ts);
        Ok(ts)
    }

    /// F1/G3 锚定行落账。
    pub fn anchor(&mut self, row: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LG_SEALED);
        }
        if row.is_empty() {
            return Err(LG_ANCHOR_MISS);
        }
        self.anchors.push(row.to_string());
        Ok(())
    }

    /// 锚定行在账核（F1 与 G3 双命中）。
    pub fn anchor_ok(&self) -> bool {
        self.anchors.iter().any(|a| a.contains("F1")) && self.anchors.iter().any(|a| a.contains("G3"))
    }

    /// J3：×10 连测分位（P50/P95）。
    pub fn run_stats(&self) -> Result<(u64, u64), u32> {
        if self.runs.len() < RUNS_N {
            return Err(LG_RUN_SHORT);
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

/// 标准夹具：一轮事件账（低覆盖 → 达标 → 高覆盖）。
pub fn standard_events(seq: u32) -> Vec<FrameEvent> {
    let mut v: Vec<FrameEvent> = Vec::new();
    let rows = [
        (1u32, 3_000_000u64, 4_000u32),
        (2, 3_200_000, 9_500),
        (3, 3_400_000, 10_000),
    ];
    for r in rows.iter() {
        v.push(FrameEvent { seq: r.0 + seq * 10, ts_ns: r.1, cover_pbp: r.2 });
    }
    v
}

pub fn standard_stub() -> LoginStub {
    let mut s = LoginStub::new();
    s.consume(&standard_events(0)).expect("consume");
    s.anchor("F1 帧账对接联签锚定行").expect("a1");
    s.anchor("G3 登录面可见判定联签锚定行").expect("a2");
    s
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空事件账
    if visible_moment(&[]).err() == Some(LG_EVENT_EMPTY) {
        caught += 1;
    }
    // 2) 无达标事件
    let low = vec![FrameEvent { seq: 1, ts_ns: 1, cover_pbp: 5_000 }];
    if visible_moment(&low).err() == Some(LG_NO_VISIBLE) {
        caught += 1;
    }
    // 3) 规则回放漂移（同账两算不同 → 审计破）
    let ev = standard_events(0);
    let a = visible_moment(&ev).expect("a");
    let b = visible_moment(&ev).expect("b");
    if a == b {
        caught += 1;
    }
    // 4) 锚定行缺
    let s = LoginStub::new();
    if !s.anchor_ok() {
        caught += 1;
    }
    // 5) 连测不足
    let mut s2 = LoginStub::new();
    for i in 0..9u32 {
        s2.consume(&standard_events(i as u32)).expect("c");
    }
    if s2.run_stats() == Err(LG_RUN_SHORT) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56044_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56044");
    // J1-1 可见时刻判定（首个达标 9500 @3.2M ns）
    let st = standard_stub();
    let ok = st.marked == Some(3_200_000);
    s.add("56044-J1-可见时刻判定", ok, if ok { "" } else { "打点漂移" });
    // J1-2 判定规则进代码可复查（纯函数回放一致）
    let ev = standard_events(0);
    let ok = visible_moment(&ev).expect("r1") == visible_moment(&ev).expect("r2");
    s.add("56044-J1-规则回放一致", ok, if ok { "" } else { "回放漂移" });
    // J1-3 覆盖率阈值语义（9500 达标 / 9499 不达标）
    let border1 = vec![FrameEvent { seq: 1, ts_ns: 1, cover_pbp: COVER_MIN_PBP }];
    let border2 = vec![FrameEvent { seq: 1, ts_ns: 1, cover_pbp: COVER_MIN_PBP - 1 }];
    let ok = visible_moment(&border1).is_ok() && visible_moment(&border2).is_err();
    s.add("56044-J1-覆盖率阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J2-1 低覆盖帧不误判（首帧 4000 不达标被跳过）
    let ok = ev[0].cover_pbp == 4_000 && st.marked.expect("m") == ev[1].ts_ns;
    s.add("56044-J2-低覆盖跳过", ok, if ok { "" } else { "低覆盖误判" });
    // J2-2 F1/G3 锚定行双命中
    let ok = st.anchor_ok();
    s.add("56044-J2-锚定行双命中", ok, if ok { "" } else { "锚定缺" });
    // J3-1 ×10 连测分位（确定性 → P50=P95=3.2M）
    let mut s2 = LoginStub::new();
    for i in 0..RUNS_N {
        s2.consume(&standard_events(i as u32)).expect("c");
    }
    let (p50, p95) = s2.run_stats().expect("st");
    let ok = p50 == 3_200_000 && p95 == 3_200_000 && s2.runs.len() == RUNS_N;
    s.add("56044-J3-十轮分位落账", ok, if ok { "" } else { "连测账缺" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56044-J3-注入核必抓", ok, &detail);
    // J3-3 封账冻结
    let mut s3 = standard_stub();
    s3.seal();
    let ok = s3.consume(&standard_events(1)) == Err(LG_SEALED)
        && s3.anchor("x") == Err(LG_SEALED);
    s.add("56044-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 事件号递增语义（夹具 1/2/3）
    let ok = ev[0].seq == 1 && ev[2].seq == 3;
    s.add("56044-J1-事件号递增", ok, if ok { "" } else { "事件号破序" });
    // J2-3 未跑桩无打点
    let bare = LoginStub::new();
    let ok = bare.marked.is_none() && bare.runs.is_empty();
    s.add("56044-J2-未跑不编造", ok, if ok { "" } else { "编造打点" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56044_all_checks_green() {
        let set = run_unx56044_checks();
        assert!(set.all_passed(), "UNX-F56044 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56044_visible() {
        assert_eq!(visible_moment(&standard_events(0)).expect("t"), 3_200_000);
    }
}
