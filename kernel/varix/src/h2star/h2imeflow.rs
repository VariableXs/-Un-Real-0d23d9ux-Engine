//! H2 输入法组合流 · 深化批次四（编辑面的组合期闸门——F260 行内
//! 重命名、F265 地址栏、F272 菜单搜索三处共用）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程六章）：
//! - **IME 纪律**：中文输入法组合期不能触发快捷键和误提交——
//!   本层是编辑面的组合闸：组合期 keystroke 全部进入组合缓冲，
//!   Enter/快捷键语义被**冻结**（组合期 Enter = 确认候选，不是
//!   提交编辑框——两语义不串台）；
//! - **F260 行内重命名**：组合提交 = 一次替换编辑（组合区间被
//!   提交串整段替换——字节级边界安全接 h2edit）；非法字符在
//!   **提交点**校验（组合期临时串可以含任意中间态，提交才判——
//!   拼音字母是合法中间态，落盘才判中文成果）；
//! - **F265 地址栏**：组合期补全暂停（候选拼音不参与补全匹配——
//!   补全只在提交后的稳定文本上算）。
//!
//! 状态机：Idle→Composing→Committed/Aborted 每条路径有出口；
//! 焦点丢失 = Aborted（组合丢弃 + 显性化——不静默吞组合）。

use crate::checks::CheckSet;

use alloc::string::String;

use crate::h2star::h2base::has_invalid_char;

// ---------------------------------------------------------------------------
// 组合状态机
// ---------------------------------------------------------------------------

/// 组合会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeState {
    Idle,
    Composing,
    /// 提交完成（一拍终态——下帧回 Idle）。
    Committed,
    /// 丢弃（焦点丢失/Esc——显性化账目）。
    Aborted,
}

/// 编辑框锚点（哪个编辑面在组合——三处共用一闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditFace {
    Rename,
    AddressBar,
    MenuSearch,
}

/// 组合会话。
pub struct ImeSession {
    pub state: ImeState,
    pub face: EditFace,
    /// 组合串（拼音中间态）。
    pub composition: String,
    /// 组合起点（编辑框字节偏移——替换区间左界）。
    pub start_byte: usize,
    /// 丢弃账（Aborted 计数——不静默）。
    pub aborts: u32,
}

impl ImeSession {
    pub fn new(face: EditFace, start_byte: usize) -> ImeSession {
        ImeSession { state: ImeState::Idle, face, start_byte, composition: String::new(), aborts: 0 }
    }

    /// 输入法开始组合（Idle → Composing）。
    pub fn begin(&mut self) {
        if self.state == ImeState::Idle {
            self.state = ImeState::Composing;
            self.composition.clear();
        }
    }

    /// 组合串更新（输入法喂中间态——只进缓冲，不进编辑框、不触发
    /// 补全、不触发快捷键）。
    pub fn update(&mut self, s: &str) -> bool {
        if self.state == ImeState::Composing {
            self.composition = s.into();
            true
        } else {
            false
        }
    }

    /// 组合期快捷键闸：组合期返回 false（冻结——Enter 是候选确认
    /// 不是提交，Ctrl+F 是候选输入不是查找）。这是「组合期零误触」
    /// 的唯一判定函数——三处编辑面共用。
    pub fn shortcuts_frozen(&self) -> bool {
        self.state == ImeState::Composing
    }

    /// 提交：组合串 → 编辑成果。返回 (替换区间, 成果串)；
    /// 非法字符在提交点校验（F260 八字符表——中间态放行、落盘才判）。
    /// 非法 → Err（编辑框抖动拒绝，组合保持——用户能改对）。
    pub fn commit(&mut self, text_len: usize) -> Result<(usize, usize, String), &'static str> {
        if self.state != ImeState::Composing {
            return Err("不在组合期——无组合可提交");
        }
        if has_invalid_char(&self.composition) {
            return Err("文件名含非法字符：\\ / : * ? \" < > |");
        }
        let end = (self.start_byte + self.composition.len()).min(text_len.max(self.start_byte));
        self.state = ImeState::Committed;
        Ok((self.start_byte, end, self.composition.clone()))
    }

    /// 丢弃（焦点丢失/Esc）：组合清空 + 显性化计数。
    pub fn abort(&mut self) -> bool {
        if self.state == ImeState::Composing {
            self.composition.clear();
            self.state = ImeState::Aborted;
            self.aborts += 1;
            true
        } else {
            false
        }
    }

    /// 复位到 Idle（Committed/Aborted 的一拍终态出口）。
    pub fn reset(&mut self) {
        self.state = ImeState::Idle;
        self.composition.clear();
    }
}

// ---------------------------------------------------------------------------
// 组合区间替换（UTF-8 边界安全——与 h2edit 字节纪律同源）
// ---------------------------------------------------------------------------

/// 组合替换：把编辑文本 `[start, end)` 区间替换为成果。
/// 边界不对齐字符边界 → None（调用方钳制后重试——不 panic）。
pub fn splice(text: &str, start: usize, end: usize, inserted: &str) -> Option<String> {
    if start > end || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + inserted.len());
    out.push_str(&text[..start]);
    out.push_str(inserted);
    out.push_str(&text[end..]);
    Some(out)
}

/// 组合期补全暂停判定（F265 车道）：Composing → 补全引擎闭嘴。
pub fn completion_paused(state: ImeState) -> bool {
    state == ImeState::Composing
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2imeflow_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2imeflow");
    // 状态机：Idle→Composing→Committed 一拍回 Idle。
    let mut s = ImeSession::new(EditFace::Rename, 0);
    set.add("h2imeflow idle open", s.commit(10).is_err(), "commit without compose refused");
    s.begin();
    s.update("baogao");
    set.add(
        "h2imeflow compose buffer",
        s.composition == "baogao" && s.shortcuts_frozen(),
        "buffered not applied",
    );
    let r = s.commit(10);
    set.add(
        "h2imeflow commit result",
        r.as_ref().map(|(a, b, t)| *a == 0 && *b == 6 && t == "baogao").unwrap_or(false)
            && s.state == ImeState::Committed,
        "range + text",
    );
    s.reset();
    set.add("h2imeflow reset", s.state == ImeState::Idle && s.composition.is_empty(), "one-beat exit");
    // 快捷键冻结：只冻结组合期（Idle 放行）。
    set.add(
        "h2imeflow freeze gate",
        !s.shortcuts_frozen(),
        "idle unfrozen",
    );
    s.begin();
    set.add("h2imeflow freeze on compose", s.shortcuts_frozen(), "composing frozen");
    // 非法字符提交点拦截：中间态放行、提交才判、组合保持可改。
    s.update("报告:终稿");
    let bad = s.commit(10);
    set.add(
        "h2imeflow illegal at commit",
        bad.is_err() && s.state == ImeState::Composing && s.composition == "报告:终稿",
        "compose state kept for fix",
    );
    s.update("报告终稿");
    set.add("h2imeflow fix and commit", s.commit(10).is_ok(), "corrected commits");
    // 丢弃：焦点丢失路径显性化（复位后重开组合）。
    s.reset();
    s.begin();
    s.update("will drop");
    set.add(
        "h2imeflow abort visible",
        s.abort() && s.state == ImeState::Aborted && s.aborts == 1 && s.composition.is_empty(),
        "abort counted",
    );
    // 组合区间替换：UTF-8 边界（中文正文里插成果——「报告」= 字节 6..12）。
    let text = "我的报告.docx";
    let sp = splice(text, 6, 12, "2026");
    set.add(
        "h2imeflow splice utf8",
        sp.as_deref() == Some("我的2026.docx"),
        "char boundary safe",
    );
    set.add(
        "h2imeflow splice reject",
        splice(text, 1, 3, "x").is_none(),
        "non-boundary refused",
    );
    // F265 车道：组合期补全暂停。
    set.add(
        "h2imeflow completion pause",
        completion_paused(ImeState::Composing) && !completion_paused(ImeState::Idle),
        "suggest only on stable text",
    );
    // 三面同闸：同一判定函数服务三个编辑面。
    set.add(
        "h2imeflow three faces",
        [EditFace::Rename, EditFace::AddressBar, EditFace::MenuSearch]
            .iter()
            .all(|f| ImeSession::new(*f, 0).state == ImeState::Idle),
        "one gate all faces",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2imeflow_all_green() {
        let set = run_h2imeflow_checks();
        assert!(set.all_passed(), "h2imeflow 自检有红项");
        assert!(!set.truncated(), "h2imeflow 自检溢出");
    }

    #[test]
    fn composition_never_leaks_into_text() {
        // 组合 100 次中途全丢：编辑文本零污染（组合不进正文不变式）。
        for i in 0..100u32 {
            let mut s = ImeSession::new(EditFace::Rename, 0);
            s.begin();
            let _ = s.update(&alloc::format!("pin{i}"));
            s.abort();
            assert_eq!(s.composition, "");
        }
    }
}
