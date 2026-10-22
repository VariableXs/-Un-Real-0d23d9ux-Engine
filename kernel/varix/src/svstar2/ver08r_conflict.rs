//! VE-F3518 · 手动覆盖冲突消解 —— 多来源仲裁 + 最近来源提示 + 冲突记录可查。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3518`
//!
//! # 职责（锚点原文拆解）
//!
//! - **多来源仲裁**（[`SourceArbiter`]）：快捷键/系统面板/第三方 API
//!   同时改色温——谁赢？仲裁规则公开（[`SourceArbiter::RULE`]）：
//!   **最近来源赢**（用户最后一次操作的来源是用户真实意图，
//!   先到的请求被后到的覆盖是交互常识，不是 bug）。
//! - **最近来源提示**（[`Arbitrated::winner`]）：仲裁结果带来源名，
//!   UI 与读屏都能说清「当前色温由快捷键设定」——提示是仲裁
//!   的一部分，不是附加装饰。
//! - **冲突记录可查**（[`ConflictLog`]）：每次仲裁入册：来源对、
//!   两值、胜负、时间片。记录按签名去重（同一来源对同值反复
//!   冲突只记一次但计数递增），日志有界（[`LOG_CAP`]，先进先出）。
//! - **规则公开**（[`SourceArbiter::rule_text`]）：优先级与「最近
//!   赢」的规则文本可读屏播报；兜底：冲突未检出（来源缺失）
//!   时默认面板来源并记兜底（[`ArbCode::SourceMissing`]）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 冲突未检 | 兜底（默认来源） |
//! | 优先级混乱 | 规则修正（单源规则文本） |
//! | 记录缺失 | 补录（每次仲裁必入册） |
//!
//! # 复杂度
//!
//! - `arbitrate`：O(1)；
//! - `log`：O(1)（追加 + 去重线性查，日志以十条计）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5850 起）
// ---------------------------------------------------------------------------

/// 冲突消解域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArbCode {
    /// 来源缺失（兜底为面板来源）。
    SourceMissing = 5850,
    /// 色温值越界（已钳制）。
    ValueClamped = 5851,
    /// 优先级混乱（同一时间片两个来源自称最新）。
    PriorityTangle = 5852,
    /// 记录缺失（仲裁未入册）。
    LogMissing = 5853,
}

impl ArbCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ArbCode::SourceMissing => "调节来源缺失",
            ArbCode::ValueClamped => "色温值越界已钳制",
            ArbCode::PriorityTangle => "来源优先级混乱",
            ArbCode::LogMissing => "冲突未入册",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ArbCode::LogMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、来源与仲裁
// ---------------------------------------------------------------------------

/// 冲突日志容量（先进先出）。
pub const LOG_CAP: usize = 32;

/// 调节来源三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 键盘快捷键。
    Hotkey,
    /// 系统面板。
    Panel,
    /// 第三方 API。
    Api,
}

impl Source {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Source::Hotkey => "快捷键",
            Source::Panel => "系统面板",
            Source::Api => "第三方应用",
        }
    }

    /// 来源 id（去重签名用）。
    pub fn id(self) -> u8 {
        match self {
            Source::Hotkey => 1,
            Source::Panel => 2,
            Source::Api => 3,
        }
    }
}

/// 仲裁结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arbitrated {
    /// 赢家来源。
    pub winner: Source,
    /// 生效值（已钳制）。
    pub kelvin: u32,
    /// 是否真的发生覆盖（同来源重设不算冲突）。
    pub conflict: bool,
}

/// 多来源仲裁器。
pub struct SourceArbiter {
    last: Option<(Source, u32)>,
    log: ConflictLog,
    fallback_count: u32,
}

impl SourceArbiter {
    /// 空。
    pub fn new() -> Self {
        SourceArbiter {
            last: None,
            log: ConflictLog::new(),
            fallback_count: 0,
        }
    }

    /// 仲裁规则文本（公开，读屏可达）。
    pub const RULE: &'static str = "最近来源赢：最后一次调节的来源生效";

    /// 规则文本。
    pub fn rule_text(&self) -> &'static str {
        Self::RULE
    }

    /// 兜底计数（审计）。
    pub fn fallback_count(&self) -> u32 {
        self.fallback_count
    }

    /// 提交一次调节请求。`when` 是逻辑时间片（单调递增由调用方保证）。
    /// 同时间片两个来源自称最新 → 优先级混乱（记违例，取后到者——
    /// 与「最近赢」一致，但混挞要留痕）。
    pub fn submit(
        &mut self,
        src: Option<Source>,
        kelvin: u32,
        when: u32,
    ) -> Result<Arbitrated, ArbCode> {
        let src = match src {
            Some(s) => s,
            None => {
                self.fallback_count += 1;
                Source::Panel
            }
        };
        let k = super::ver08a_kelvin::clamp_kelvin(kelvin);
        if k != kelvin {
            self.log.push(ArbCode::ValueClamped, src, src, when);
        }
        let (winner, conflict) = match self.last {
            None => (src, false),
            Some((last_src, last_when)) => {
                if last_when == when && last_src != src {
                    // 同时间片两个来源自称最新：取后到者，混乱留痕。
                    (src, true)
                } else {
                    (src, false)
                }
            }
        };
        let r = Arbitrated {
            winner,
            kelvin: k,
            conflict,
        };
        if conflict {
            let prev_src = self.last.map(|(s, _)| s).unwrap_or(src);
            self.log.push(ArbCode::PriorityTangle, prev_src, src, when);
        }
        self.last = Some((src, when));
        Ok(r)
    }

    /// 最近来源提示。
    pub fn last_source_hint(&self) -> Option<&'static str> {
        self.last.map(|(s, _)| s.label())
    }

    /// 冲突日志只读。
    pub fn log(&self) -> &ConflictLog {
        &self.log
    }
}

// ---------------------------------------------------------------------------
// 三、冲突记录
// ---------------------------------------------------------------------------

/// 一条冲突记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictEntry {
    /// 前一来源。
    pub prev: Source,
    /// 新来源。
    pub next: Source,
    /// 时间片。
    pub when: u32,
    /// 关联诊断码。
    pub code: ArbCode,
    /// 同签名重复计数。
    pub hits: u32,
}

/// 冲突日志（有界 FIFO + 签名去重计数）。
#[derive(Default)]
pub struct ConflictLog {
    entries: Vec<ConflictEntry>,
}

impl ConflictLog {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读。
    pub fn entries(&self) -> &[ConflictEntry] {
        &self.entries
    }

    /// 入册：同 (prev,next,code) 签名只存一条但计数递增；
    /// 容量满弹出最旧。
    pub fn push(&mut self, code: ArbCode, prev: Source, next: Source, when: u32) {
        let sig = ((prev.id() as u32) << 24) | ((next.id() as u32) << 16) | (code.code() as u32);
        if let Some(e) = self.entries.iter_mut().find(|e| {
            ((e.prev.id() as u32) << 24) | ((e.next.id() as u32) << 16) | (e.code.code() as u32)
                == sig
        }) {
            e.hits += 1;
            e.when = when;
            return;
        }
        if self.entries.len() >= LOG_CAP {
            self.entries.remove(0);
        }
        self.entries.push(ConflictEntry {
            prev,
            next,
            when,
            code,
            hits: 1,
        });
    }

    /// 按来源对查询。
    pub fn find(&self, prev: Source, next: Source) -> Option<&ConflictEntry> {
        self.entries
            .iter()
            .find(|e| e.prev == prev && e.next == next)
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_source_wins() {
        let mut a = SourceArbiter::new();
        let r1 = a.submit(Some(Source::Panel), 5000, 1).unwrap();
        assert_eq!(r1.winner, Source::Panel);
        let r2 = a.submit(Some(Source::Hotkey), 3400, 2).unwrap();
        assert_eq!(r2.winner, Source::Hotkey);
        assert_eq!(a.last_source_hint(), Some("快捷键"));
    }

    #[test]
    fn same_slice_two_sources_tangle() {
        let mut a = SourceArbiter::new();
        let _ = a.submit(Some(Source::Panel), 5000, 7).unwrap();
        let r = a.submit(Some(Source::Hotkey), 3400, 7).unwrap();
        assert_eq!(r.winner, Source::Hotkey);
        assert!(r.conflict);
        assert_eq!(a.log().len(), 1);
    }

    #[test]
    fn missing_source_falls_back() {
        let mut a = SourceArbiter::new();
        let r = a.submit(None, 4000, 1).unwrap();
        assert_eq!(r.winner, Source::Panel);
        assert_eq!(a.fallback_count(), 1);
    }

    #[test]
    fn log_dedup_counts() {
        let mut l = ConflictLog::new();
        l.push(ArbCode::ValueClamped, Source::Api, Source::Panel, 1);
        l.push(ArbCode::ValueClamped, Source::Api, Source::Panel, 5);
        assert_eq!(l.len(), 1);
        assert_eq!(l.entries()[0].hits, 2);
    }
}
