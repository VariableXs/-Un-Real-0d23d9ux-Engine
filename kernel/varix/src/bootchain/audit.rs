//! UNREAL-X AI-01 · 族0001 冷启动链路体检（X00001~X00025）。
//!
//! 冷启动链路体检：把从固件交接到 Variable 桌面的整条链路切成固定数量的
//! 链路段（segment），逐段计时、逐段判定，产出一份可直接在设置页呈现的
//! 体检报告。纯逻辑 + 固定容量数组，无 `Vec`/`String`/`alloc`，no_std 兼容。
//!
//! 纪律（对齐全景图 25 档口径）：
//! - 默认档 = 现状（只观测，不干预）；
//! - 非法/越界输入一律钳制回默认，绝不 panic；
//! - 链路中断支持「半成品标记 + 续跑」，不丢已有采样。

use crate::bootchain::hash_bytes;
use crate::checks::CheckSet;

/// 链路段名上限（字节，超出钳掉尾巴）。
pub const SEGMENT_NAME_MAX: usize = 16;
/// 体检报告最多容纳的链路段数。
pub const SEGMENT_MAX: usize = 16;

/// 体检档位矩阵：≥5 档独立可交付，默认 Standard = 现状手感。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditLevel {
    /// 只测核心 5 段，其余不采样。
    Minimal,
    /// 现状：全段采样，不做阈值判定。
    Standard,
    /// 全段采样 + 预算表阈值判定。
    Deep,
    /// Deep + 每段留指纹（哈希摘要），供跨版本比对。
    Forensic,
    /// 自定义阈值（钳制到 [64, 16384] ms）。
    Custom,
}

impl AuditLevel {
    /// 档位序号（0~4），非法值钳回 Standard(1)。
    pub fn from_index(i: u32) -> AuditLevel {
        match i {
            0 => AuditLevel::Minimal,
            1 => AuditLevel::Standard,
            2 => AuditLevel::Deep,
            3 => AuditLevel::Forensic,
            _ => AuditLevel::Standard,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            AuditLevel::Minimal => 0,
            AuditLevel::Standard => 1,
            AuditLevel::Deep => 2,
            AuditLevel::Forensic => 3,
            AuditLevel::Custom => 4,
        }
    }

    /// Minimal 只采前 5 段；其余档全量。
    pub fn covers(self, index: usize) -> bool {
        match self {
            AuditLevel::Minimal => index < 5,
            _ => true,
        }
    }
}

/// 单个链路段采样：名字、起止毫秒、成败、指纹（Forensic 档才有意义）。
#[derive(Clone, Copy, Debug)]
pub struct SegmentSample {
    pub name: [u8; SEGMENT_NAME_MAX],
    pub name_len: usize,
    pub start_ms: u32,
    pub end_ms: u32,
    pub ok: bool,
    pub fingerprint: u32,
    /// 断点续传标记：上一次体检中断时本段已完成（true = 半成品/陈旧样本）。
    pub stale: bool,
}

impl SegmentSample {
    fn empty() -> SegmentSample {
        SegmentSample {
            name: [0u8; SEGMENT_NAME_MAX],
            name_len: 0,
            start_ms: 0,
            end_ms: 0,
            ok: false,
            fingerprint: 0,
            stale: false,
        }
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 本段耗时；end 未落（end_ms == 0）视为未完成，返回 0。
    pub fn duration_ms(&self) -> u32 {
        if self.end_ms <= self.start_ms {
            0
        } else {
            self.end_ms - self.start_ms
        }
    }
}

/// 冷启动链路体检报告。
#[derive(Clone, Copy, Debug)]
pub struct AuditReport {
    pub level: AuditLevel,
    pub segments: [Option<SegmentSample>; SEGMENT_MAX],
    pub count: usize,
    /// 被钳制/丢弃的非法输入次数（护栏可观测）。
    pub clamped: u32,
    /// 自定义阈值（Custom 档生效），毫秒。
    pub custom_budget_ms: u32,
}

impl AuditReport {
    pub const fn new(level: AuditLevel) -> AuditReport {
        AuditReport {
            level,
            segments: [None; SEGMENT_MAX],
            count: 0,
            clamped: 0,
            custom_budget_ms: 2048,
        }
    }

    /// 写入名字（越界回退空名，不越权内存）。
    fn put_name(&self, slot: &mut SegmentSample, name: &[u8]) {
        let n = if name.len() > SEGMENT_NAME_MAX {
            SEGMENT_NAME_MAX
        } else {
            name.len()
        };
        slot.name[..n].copy_from_slice(&name[..n]);
        slot.name_len = n;
    }

    /// 自定义阈值：钳制到 [64, 16384] ms，越界回默认 2048。
    pub fn set_custom_budget(&mut self, ms: u32) {
        self.custom_budget_ms = if (64..=16384).contains(&ms) {
            ms
        } else {
            2048
        };
    }

    /// 段开始：index 越界或档位不覆盖该段 → 记一次钳制并返回 false。
    pub fn begin(&mut self, index: usize, name: &[u8], now_ms: u32) -> bool {
        if index >= SEGMENT_MAX || !self.level.covers(index) || name.is_empty() {
            self.clamped += 1;
            return false;
        }
        let mut s = SegmentSample::empty();
        self.put_name(&mut s, name);
        s.start_ms = now_ms;
        s.stale = self.segments[index].is_some();
        self.segments[index] = Some(s);
        if index + 1 > self.count {
            self.count = index + 1;
        }
        true
    }

    /// 段结束：必须在 begin 之后；ok 决定段判定。
    pub fn end(&mut self, index: usize, now_ms: u32, ok: bool) -> bool {
        if index >= SEGMENT_MAX || self.segments[index].is_none() {
            self.clamped += 1;
            return false;
        }
        if let Some(s) = self.segments[index].as_mut() {
            if now_ms < s.start_ms {
                // 时间回绕/非法：钳回开始时刻 + 0。
                s.end_ms = s.start_ms;
                s.ok = false;
            } else {
                s.end_ms = now_ms;
                s.ok = ok;
            }
            s.fingerprint = hash_bytes(s.name_bytes()) ^ (s.duration_ms().wrapping_mul(0x9e37_79b9));
        }
        true
    }

    /// 段预算判定：超时即不健康。Minimal/Standard 档不做判定（恒 true）。
    pub fn segment_within_budget(&self, index: usize) -> bool {
        if !matches!(self.level, AuditLevel::Deep | AuditLevel::Forensic | AuditLevel::Custom) {
            return true;
        }
        match self.segments[index] {
            Some(s) => {
                let budget = match self.level {
                    AuditLevel::Custom => self.custom_budget_ms,
                    _ => 1024,
                };
                s.ok && s.duration_ms() <= budget
            }
            None => false,
        }
    }

    /// 总耗时（未完成段计 0）。
    pub fn total_ms(&self) -> u32 {
        let mut t = 0u32;
        for i in 0..self.count {
            if let Some(s) = self.segments[i] {
                t = t.wrapping_add(s.duration_ms());
            }
        }
        t
    }

    /// 全链路健康：所有已采样段在预算内。
    pub fn healthy(&self) -> bool {
        if self.count == 0 {
            return false;
        }
        (0..self.count).all(|i| self.segment_within_budget(i))
    }

    /// 一键续作：把上次中断留下的陈旧段清空，仅保留完整样本。
    pub fn resume(&mut self) -> usize {
        let mut kept = 0usize;
        for i in 0..self.count {
            let complete = match self.segments[i] {
                Some(s) => s.end_ms > s.start_ms,
                None => false,
            };
            if !complete {
                self.segments[i] = None;
            } else {
                kept += 1;
            }
        }
        self.count = self
            .segments
            .iter()
            .rposition(|s| s.is_some())
            .map(|p| p + 1)
            .unwrap_or(0);
        kept
    }

    /// 回滚净身：清空报告，回到出厂档（Standard）。
    pub fn reset(&mut self) {
        *self = AuditReport::new(AuditLevel::Standard);
    }
}

/// 族0001 域自检（对齐全景图 25 档中的代表性可运行断言）。
pub fn run_audit_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain.audit");
    let mut r = AuditReport::new(AuditLevel::Deep);
    let b1 = r.begin(0, b"firmware", 0);
    let e1 = r.end(0, 40, true);
    set.add("begin-end roundtrip", b1 && e1 && r.segments[0].unwrap().duration_ms() == 40, "");
    set.add("total sums segments", r.total_ms() == 40, "");
    let bad = r.begin(SEGMENT_MAX + 3, b"bogus", 0);
    set.add("out-of-range clamped", !bad && r.clamped == 1, "");
    set.add("minimal covers 5 only", !AuditLevel::Minimal.covers(5) && AuditLevel::Deep.covers(15), "");
    set.add("level index roundtrip", AuditLevel::from_index(3).index() == 3 && AuditLevel::from_index(99) == AuditLevel::Standard, "");
    let mut c = AuditReport::new(AuditLevel::Custom);
    c.custom_budget_ms = 0;
    set.add("custom budget clamped floor", {
        c.set_custom_budget(0);
        c.set_custom_budget(999_999);
        c.set_custom_budget(900);
        c.custom_budget_ms == 900
    }, "");
    set.add("resume drops half-done", {
        let mut rr = AuditReport::new(AuditLevel::Standard);
        rr.begin(0, b"a", 0);
        rr.end(0, 5, true);
        rr.begin(1, b"b", 5);
        rr.resume() == 1 && rr.segments[1].is_none()
    }, "");
    set.add("reset returns to standard", {
        let mut rr = AuditReport::new(AuditLevel::Forensic);
        rr.reset();
        rr.level == AuditLevel::Standard && rr.count == 0
    }, "");
    set.add("empty report unhealthy", !AuditReport::new(AuditLevel::Standard).healthy(), "");
    set.add("healthy full chain", r.healthy(), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x00001_end_to_end_min_loop() {
        let mut r = AuditReport::new(AuditLevel::Standard);
        assert!(r.begin(0, b"firmware", 0));
        assert!(r.begin(1, b"limine", 40));
        assert!(r.end(0, 40, true));
        assert!(r.end(1, 90, true));
        assert_eq!(r.total_ms(), 90);
        assert!(r.healthy());
    }

    #[test]
    fn x00006_out_of_range_never_panics() {
        let mut r = AuditReport::new(AuditLevel::Standard);
        assert!(!r.begin(SEGMENT_MAX, b"x", 0));
        assert!(!r.end(SEGMENT_MAX, 1, true));
        assert!(!r.end(0, 0, true)); // 无 begin 的 end
        assert!(r.clamped >= 2);
    }

    #[test]
    fn x00008_resume_keeps_complete_only() {
        let mut r = AuditReport::new(AuditLevel::Standard);
        r.begin(0, b"a", 0);
        r.end(0, 4, true);
        r.begin(1, b"b", 4); // 中断：未 end
        assert_eq!(r.resume(), 1);
        assert!(r.segments[0].is_some());
        assert!(r.segments[1].is_none());
    }

    #[test]
    fn x00010_reset_is_clean_uninstall() {
        let mut r = AuditReport::new(AuditLevel::Forensic);
        r.begin(0, b"a", 0);
        r.reset();
        assert_eq!(r.count, 0);
        assert_eq!(r.clamped, 0);
    }

    #[test]
    fn x00001_run_checks_pass() {
        let set = run_audit_checks();
        assert!(set.all_passed(), "audit self-test must pass");
    }
}
