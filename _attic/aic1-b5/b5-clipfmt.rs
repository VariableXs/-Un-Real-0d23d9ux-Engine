
// ---------------------------------------------------------------------------
// F017 · 深化批次五：剪贴板变更序号（Windows 同语义——内容变更单调计数）
//
// 主册依据（G-A-17【功能定义】）Windows 剪贴板语义对齐——GetClipboardSequence
// Number 同语义：每次内容更替序号 +1，消费方据此识别「剪贴板还是老内容」。
// ---------------------------------------------------------------------------

/// 剪贴板变更序号（会话级单调计数；初始 0 = 会话内尚无变更）。
#[derive(Clone, Copy, Debug)]
pub struct ClipboardSequence {
    seq: u64,
}

impl ClipboardSequence {
    pub const fn new() -> ClipboardSequence {
        ClipboardSequence { seq: 0 }
    }

    pub fn current(&self) -> u64 {
        self.seq
    }

    /// 内容更替（set/替换所有权都算——返回新序号）。
    pub fn bump(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }
}

/// F017 深化批次五自检。
pub fn run_clipfmt_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep4");
    // 1) 单调递增：三次更替 → 1/2/3（消费方可判「有没有新内容」）。
    let mut sq = ClipboardSequence::new();
    let a = sq.bump();
    let b = sq.bump();
    let c = sq.bump();
    cs.add(
        "clipboard_seq_monotonic",
        a == 1 && b == 2 && c == 3 && sq.current() == 3,
        "",
    );
    // 2) 未变更不递增（current 是读不是写——序号不虚涨）。
    let cur = sq.current();
    cs.add(
        "clipboard_seq_read_only",
        cur == 3 && sq.current() == 3,
        "",
    );
    cs
}
