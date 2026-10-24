//! UNX-F0111 · 屏幕滚动与历史回看 · 诊断段 0x2390
//! 验收锚点：UNX-F0111-J1 历史缓冲滚动回看，行账守恒、回看窗
//! 越界全拦。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2390;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_WIN: u16 = SEG_BASE + 1;   // 回看窗越界（超历史深度）
const E_NOLINE: u16 = SEG_BASE + 2;// 行号为零（行账从 1 起）
const E_EMPTY: u16 = SEG_BASE + 3; // 空历史回看
const E_CAP: u16 = SEG_BASE + 4;   // 容量非法（零或超上限）
const E_RDONLY: u16 = SEG_BASE + 5;// 回看态冻结，追加被拒
const E_BAL: u16 = SEG_BASE + 6;   // 行账不平（首号+条数 ≠ 尾号+1）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_WIN => "回看窗越界：请求行超出历史深度",
        E_NOLINE => "行号非法：行账从 1 起",
        E_EMPTY => "空历史：无行可回看",
        E_CAP => "容量非法：历史深度须为正且不超 4096",
        E_RDONLY => "回看冻结：定格态禁止追加",
        E_BAL => "行账不平：首号 + 条数 ≠ 尾号 + 1",
        _ => "未知诊断码",
    }
}

pub const CAP_MAX: usize = 4096;

/// 历史行。
#[derive(Clone)]
pub struct HistLine {
    pub no: u64,   // 全局行号（从 1 起）
    pub text: String,
}

/// 回看账：定深环形历史 + 回看窗。
pub struct ScrollBack {
    pub depth: usize,
    pub lines: Vec<HistLine>,
    pub frozen: bool,
    pub next_no: u64,
}

impl ScrollBack {
    pub fn new(depth: usize) -> Result<ScrollBack, u16> {
        if depth == 0 || depth > CAP_MAX {
            return Err(E_CAP);
        }
        Ok(ScrollBack {
            depth,
            lines: Vec::new(),
            frozen: false,
            next_no: 1,
        })
    }

    /// 追加一行：超深从顶挤掉最老。
    pub fn push(&mut self, text: &str) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.lines.push(HistLine {
            no,
            text: text.to_string(),
        });
        if self.lines.len() > self.depth {
            self.lines.remove(0);
        }
        Ok(no)
    }

    /// 回看窗：取第 k 条（1 = 最新，k = 最老）。
    pub fn look(&self, k: usize) -> Result<&HistLine, u16> {
        if self.lines.is_empty() {
            return Err(E_EMPTY);
        }
        if k == 0 {
            return Err(E_NOLINE);
        }
        if k > self.lines.len() {
            return Err(E_WIN);
        }
        Ok(&self.lines[self.lines.len() - k])
    }

    /// 定格回看：冻结历史，禁止追加（回看态）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if self.lines.is_empty() {
            return Err(E_EMPTY);
        }
        self.frozen = true;
        Ok(())
    }

    /// 行账守恒：首号 + 条数 − 1 = 尾号。
    pub fn audit(&self) -> Result<(), u16> {
        if self.lines.is_empty() {
            return Ok(());
        }
        let first = self.lines[0].no;
        let last = self.lines[self.lines.len() - 1].no;
        if first + self.lines.len() as u64 - 1 != last {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 回看窗渲染：从第 from 条起至多 n 条（k 序：1=最新）。
    pub fn render_window(&self, from: usize, n: usize) -> Result<String, u16> {
        if self.lines.is_empty() {
            return Err(E_EMPTY);
        }
        if from == 0 || from > self.lines.len() {
            return Err(E_WIN);
        }
        let mut s = format!("回看窗 [{}..{}]：", from, (from + n - 1).min(self.lines.len()));
        let mut first = true;
        let end = (from + n - 1).min(self.lines.len());
        for k in from..=end {
            let l = self.look(k)?;
            if !first {
                s.push_str(" | ");
            }
            first = false;
            s.push_str(&format!("#{} {}", l.no, l.text));
        }
        Ok(s)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：深 4 环写 6 行 → 在史 4 行（#3~#6），回看 k=1 得 #6。
    let mut s = ScrollBack::new(4)?;
    for i in 1..=6u64 {
        s.push(&format!("line-{}", i))?;
    }
    if s.lines.len() != 4 {
        return Err(E_BAL);
    }
    let top = s.look(1)?;
    if top.no != 6 || top.text != "line-6" {
        return Err(E_BAL);
    }
    let bot = s.look(4)?;
    if bot.no != 3 {
        return Err(E_BAL);
    }
    s.audit()?;
    hits += 1;

    // R2：回看窗渲染 [1..2] → 最新两行倒序呈现。
    let w = s.render_window(1, 2)?;
    if !w.contains("#6 line-6") || !w.contains("#5 line-5") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：定格回看 → 追加被拒；账审计绿。
    let mut s3 = ScrollBack::new(8)?;
    for i in 1..=8u64 {
        s3.push(&format!("h{}", i))?;
    }
    s3.freeze()?;
    if s3.push("late") != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    s3.audit()?;
    hits += 1;

    // R4：不满环（2/8）行号守恒（#1~#2）+ 空历史审计放行。
    let mut s4 = ScrollBack::new(8)?;
    s4.audit()?;
    s4.push("a")?;
    s4.push("b")?;
    s4.audit()?;
    let bot = s4.look(2)?;
    if bot.no != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：容量非法（零 / 超上限）→ E_CAP。
    if ScrollBack::new(0).is_ok() {
        return Err(E_CAP);
    }
    if ScrollBack::new(CAP_MAX + 1).is_ok() {
        return Err(E_CAP);
    }
    hits += 1;

    // I2：行号零 → E_NOLINE。
    let mut i2 = ScrollBack::new(4)?;
    i2.push("x")?;
    if !matches!(i2.look(0), Err(E_NOLINE)) {
        return Err(E_NOLINE);
    }
    hits += 1;

    // I3：回看窗越界（超深度）→ E_WIN；空历史回看 → E_EMPTY。
    if !matches!(i2.look(5), Err(E_WIN)) {
        return Err(E_WIN);
    }
    let i3 = ScrollBack::new(4)?;
    if !matches!(i3.look(1), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：回看窗渲染越界（from 0 / from 超顶）→ E_WIN。
    if i2.render_window(0, 1) != Err(E_WIN) {
        return Err(E_WIN);
    }
    if i2.render_window(9, 1) != Err(E_WIN) {
        return Err(E_WIN);
    }
    hits += 1;

    // I5：行账破注入 → E_BAL；重复定格 → E_RDONLY。
    let mut i5 = ScrollBack::new(4)?;
    i5.push("p")?;
    i5.push("q")?;
    i5.lines[1].no = 9; // 人为破账：#1,#9
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut i5b = ScrollBack::new(4)?;
    i5b.push("z")?;
    i5b.freeze()?;
    if i5b.freeze() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
