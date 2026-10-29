//! UNX-F56037 采集失败机器可读痕迹件（0x60480 段）
//!
//! 锚点：UNX-F56037-J1 采集器崩溃/超时/样本不足三种失败路径统一落失
//! 败账（禁静默丢弃——每次失败必有痕迹行）；J2 失败账字段齐（时间序 ×
//! 指标 × 阶段 × errno 类码 × 诊断摘要五元组逐条可读）×3 注入全捕获；
//! J3 阶段闭集（预热/采样/聚合三阶段，越段即拒）三判据全绿。判据锚
//! 定 §AI-71.2 专题三错误路径。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·失败痕迹面）。
//!
//! 结构：失败类闭集（崩溃/超时/样本不足）+ 阶段闭集（预热/采样/聚合）
//! + 统一失败账（五元组逐条落账只追加）+ 静默丢弃审计钩子（账外失败
//! 计数恒零）+ 注入核（越闭集类/越闭集段/空摘要/静默丢弃/封账改写必
//! 抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60480 独占 0x20）。
pub const SEG_BASE: u32 = 0x60480;
pub const FT_KIND_UNK: u32 = SEG_BASE + 0x01; // 越闭集失败类
pub const FT_STAGE_UNK: u32 = SEG_BASE + 0x02; // 越闭集阶段
pub const FT_MSG_EMPTY: u32 = SEG_BASE + 0x03; // 诊断摘要空
pub const FT_SILENT: u32 = SEG_BASE + 0x04; // 静默丢弃（账外失败）
pub const FT_SEALED: u32 = SEG_BASE + 0x05; // 失败账已封账拒写
pub const FT_SEQ_BACK: u32 = SEG_BASE + 0x06; // 时间序回退
pub const FT_METRIC_EMPTY: u32 = SEG_BASE + 0x07; // 指标名空

/// 失败类闭集。
pub const K_CRASH: usize = 0;
pub const K_TIMEOUT: usize = 1;
pub const K_SHORT: usize = 2;
pub const K_COUNT: usize = 3;
pub const K_NAMES: [&str; K_COUNT] = ["crash", "timeout", "sample_short"];

/// 阶段闭集。
pub const S_WARMUP: usize = 0;
pub const S_SAMPLE: usize = 1;
pub const S_AGGREGATE: usize = 2;
pub const S_COUNT: usize = 3;
pub const S_NAMES: [&str; S_COUNT] = ["warmup", "sample", "aggregate"];

pub fn kind_ok(k: usize) -> bool {
    k < K_COUNT
}
pub fn stage_ok(s: usize) -> bool {
    s < S_COUNT
}

/// 失败痕迹：序 × 类 × 指标 × 阶段 × 码 × 摘要。
#[derive(Clone)]
pub struct FailRow {
    pub seq: u32,
    pub kind: usize,
    pub metric: String,
    pub stage: usize,
    pub err_code: u32,
    pub msg: String,
}

impl FailRow {
    pub fn new(seq: u32, kind: usize, metric: &str, stage: usize, err_code: u32, msg: &str) -> Result<Self, u32> {
        if !kind_ok(kind) {
            return Err(FT_KIND_UNK);
        }
        if !stage_ok(stage) {
            return Err(FT_STAGE_UNK);
        }
        if metric.is_empty() {
            return Err(FT_METRIC_EMPTY);
        }
        if msg.is_empty() {
            return Err(FT_MSG_EMPTY);
        }
        if seq == 0 {
            return Err(FT_SEQ_BACK);
        }
        Ok(FailRow {
            seq,
            kind,
            metric: metric.to_string(),
            stage,
            err_code,
            msg: msg.to_string(),
        })
    }

    /// 机读行（FAIL|序|类|指标|段|码|摘要 七段）。
    pub fn render(&self) -> String {
        format!(
            "FAIL|{}|{}|{}|{}|{}|{}",
            self.seq, K_NAMES[self.kind], self.metric, S_NAMES[self.stage],
            self.err_code, self.msg
        )
    }
}

/// 统一失败账：只追加 + 静默审计。
pub struct FailBook {
    pub rows: Vec<FailRow>,
    pub silent_drops: usize,
    pub sealed: bool,
}

impl FailBook {
    pub fn new() -> Self {
        FailBook { rows: Vec::new(), silent_drops: 0, sealed: false }
    }

    /// 落失败行（序号严格递增 + 封账拒）。
    pub fn record(&mut self, row: FailRow) -> Result<(), u32> {
        if self.sealed {
            self.silent_drops += 1;
            return Err(FT_SEALED);
        }
        if row.seq == 0 {
            self.silent_drops += 1;
            return Err(FT_SEQ_BACK);
        }
        if let Some(prev) = self.rows.last() {
            if row.seq <= prev.seq {
                self.silent_drops += 1;
                return Err(FT_SEQ_BACK);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    /// J1：禁静默丢弃核（账外计数恒零）。
    pub fn no_silent(&self) -> bool {
        self.silent_drops == 0
    }

    /// J2：×3 注入捕获核（三类失败各恰一）。
    pub fn captured3(&self) -> bool {
        if self.rows.len() != 3 {
            return false;
        }
        self.rows[0].kind == K_CRASH
            && self.rows[1].kind == K_TIMEOUT
            && self.rows[2].kind == K_SHORT
    }

    /// 按类计数。
    pub fn count_by_kind(&self, k: usize) -> usize {
        let mut n = 0usize;
        for r in self.rows.iter() {
            if r.kind == k {
                n += 1;
            }
        }
        n
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三类失败各一条（syscall 指标 × 三段）。
pub fn standard_fails() -> FailBook {
    let mut b = FailBook::new();
    b.record(FailRow::new(1, K_CRASH, "syscall", S_WARMUP, SEG_BASE + 0x10, "预热段空指针").expect("r1"))
        .expect("rec1");
    b.record(FailRow::new(2, K_TIMEOUT, "syscall", S_SAMPLE, SEG_BASE + 0x11, "采样窗超时 30s").expect("r2"))
        .expect("rec2");
    b.record(FailRow::new(3, K_SHORT, "syscall", S_AGGREGATE, SEG_BASE + 0x12, "样本 29 < 30").expect("r3"))
        .expect("rec3");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集失败类
    if FailRow::new(1, 9, "m", S_SAMPLE, 1, "x").err() == Some(FT_KIND_UNK) {
        caught += 1;
    }
    // 2) 越闭集阶段
    if FailRow::new(1, K_CRASH, "m", 7, 1, "x").err() == Some(FT_STAGE_UNK) {
        caught += 1;
    }
    // 3) 空摘要
    if FailRow::new(1, K_CRASH, "m", S_SAMPLE, 1, "").err() == Some(FT_MSG_EMPTY) {
        caught += 1;
    }
    // 4) 序号回退
    let mut b = standard_fails();
    if b.record(FailRow::new(2, K_CRASH, "m", S_SAMPLE, 1, "y").expect("back"))
        == Err(FT_SEQ_BACK)
        && b.silent_drops == 1
    {
        caught += 1;
    }
    // 5) 封账改写（落账拒且静默计数 +1——封账也留痕不静默）
    let mut b2 = standard_fails();
    b2.seal();
    if b2.record(FailRow::new(9, K_CRASH, "m", S_SAMPLE, 1, "z").expect("sealed"))
        == Err(FT_SEALED)
        && b2.silent_drops == 1
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56037");
    // J1-1 三类失败 ×3 注入全捕获
    let b = standard_fails();
    let ok = b.captured3()
        && b.count_by_kind(K_CRASH) == 1
        && b.count_by_kind(K_TIMEOUT) == 1
        && b.count_by_kind(K_SHORT) == 1;
    s.add("56037-J1-三路径全捕获", ok, if ok { "" } else { "失败路径漏捕" });
    // J1-2 五元组齐（机读行七段）
    let line = b.rows[0].render();
    let ok = line.split('|').count() == 7
        && line.starts_with("FAIL|1|crash|syscall|warmup|");
    s.add("56037-J1-五元组机读", ok, if ok { "" } else { "字段缺" });
    // J1-3 禁静默丢弃（账外计数恒零）
    let ok = b.no_silent() && b.silent_drops == 0;
    s.add("56037-J1-禁静默丢弃", ok, if ok { "" } else { "静默丢弃发生" });
    // J2-1 阶段闭集三段（预热/采样/聚合各一段）
    let ok = b.rows[0].stage == S_WARMUP
        && b.rows[1].stage == S_SAMPLE
        && b.rows[2].stage == S_AGGREGATE;
    s.add("56037-J2-阶段闭集三段", ok, if ok { "" } else { "阶段漂移" });
    // J2-2 闭集核函数语义
    let ok = kind_ok(K_SHORT) && !kind_ok(3) && stage_ok(S_AGGREGATE) && !stage_ok(3);
    s.add("56037-J2-闭集核语义", ok, if ok { "" } else { "闭集核松" });
    // J2-3 errno 类码逐条递增段基（0x10/0x11/0x12）
    let ok = b.rows[0].err_code == SEG_BASE + 0x10
        && b.rows[1].err_code == SEG_BASE + 0x11
        && b.rows[2].err_code == SEG_BASE + 0x12;
    s.add("56037-J2-errno段基", ok, if ok { "" } else { "码漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56037-J3-注入核必抓", ok, &detail);
    // J3-2 封账也留痕（封账拒写落静默计数——不丢不吞）
    let mut b2 = standard_fails();
    b2.seal();
    let _ = b2.record(FailRow::new(9, K_CRASH, "m", S_SAMPLE, 1, "z").expect("s"));
    let ok = b2.silent_drops == 1;
    s.add("56037-J3-封账留痕", ok, if ok { "" } else { "封账吞账" });
    // J3-3 类名/段名闭集同源
    let ok = K_NAMES[0] == "crash" && S_NAMES[2] == "aggregate";
    s.add("56037-J3-命名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-4 序号单调（1..3）
    let mut mono = true;
    for w in b.rows.windows(2) {
        if w[0].seq >= w[1].seq {
            mono = false;
        }
    }
    let ok = mono && b.rows.len() == 3;
    s.add("56037-J1-序号单调", ok, if ok { "" } else { "序号破" });
    // J2-4 指标名空拒
    let ok = FailRow::new(1, K_CRASH, "", S_SAMPLE, 1, "x").err() == Some(FT_METRIC_EMPTY);
    s.add("56037-J2-指标名必填", ok, if ok { "" } else { "指标名缺" });
    // J1-5 渲染逐行齐
    let mut all_lines = String::new();
    for r in b.rows.iter() {
        all_lines.push_str(r.render().as_str());
        all_lines.push('\n');
    }
    let ok = all_lines.lines().count() == 3 && all_lines.contains("sample_short|syscall|aggregate");
    s.add("56037-J1-渲染逐行齐", ok, if ok { "" } else { "渲染面漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56037_all_checks_green() {
        let set = run_unx56037_checks();
        assert!(set.all_passed(), "UNX-F56037 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56037_no_silent() {
        let b = standard_fails();
        assert!(b.no_silent());
        assert_eq!(b.rows.len(), 3);
    }
}
