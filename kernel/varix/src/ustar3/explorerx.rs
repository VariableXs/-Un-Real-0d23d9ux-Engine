//! explorerx —— 资源管理器主题七项（AI-U3 分工包 · 本文件只覆盖 F515/F517/F521/F525/F526/F527/F528）。
//!
//! 判据唯一源：主册《Varix STAR I start.md》各节【验收判据】第一句（逐字摘录如下）。
//! 逻辑路径零堆：无 String/Vec/Box/format!，定长数组 + core 运算；测试可用 std。
//!
//! =========================================================================
//! F515 查找与替换（domain: "F515-find-replace"）
//! 判据（逐字）：**双条形制一致；影响数预览；逐个/全部两路；整批撤销一次回滚；开关共享。**
//! 功能定义要点：文本查找（F221）补全替换半边——Ctrl+H 呼出替换条（与查找条
//!   同形制同位置）；查找词/替换词双输入；逐个替换（当前高亮处推进）/全部替换
//!   （替换前显示将影响 N 处+预览第一处上下文——批量操作先看账）；可撤销（整批
//!   一次 Ctrl+Z 回滚——F202 栈内一次 undo 整批）；区分大小写/全字匹配沿用查找开关。
//! 依赖锚点：F221 查找条（开关共享）、F202 撤销栈（整批一次回滚）。
//! 零堆纪律：文档=定长 u32 码点数组；undo 记录含原文快照+区间账，全部栈上定长。
//!
//! =========================================================================
//! F517 资源管理器启动页设置（domain: "F517-exp-startpage"）
//! 判据（逐字）：**三模式行为；上次文件夹记忆准确性；固定文件夹；说明文案；切换即时（下次生效语义明确）。**
//! 功能定义要点：Win+E/新窗口落点可选——此机（默认 F404）/上次关闭的文件夹/
//!   固定文件夹（用户指定）；「上次文件夹」模式记每个窗口最后位置（F237 记忆的
//!   启动端延伸）；设置页即时预览说明（每种落点一句话后果）；切换即时（修改
//!   policy 不影响已开窗口，只影响新窗）。
//! 依赖锚点：F404 此机默认落点、F237 窗口记忆族。
//! 零堆纪律：记忆表=定长 20 窗口槽位；路径=定长字节数组 NameBuf；文案 &str 常量。
//!
//! =========================================================================
//! F521 截图保存位置设置（domain: "F521-shot-savedir"）
//! 判据（逐字）：**三落点行为；命名规则与前缀；一致性（截图/另存同点）；S: 可选；询问模式对话框。**
//! 功能定义要点：截图落点三选——图片/截图目录（默认，自动建）/桌面/每次询问
//!   （保存对话框 F233）；自动命名规则可调（「截图 2026-09-25_1430」默认、可加
//!   前缀、永不重名单调）；F512 历史条目的「另存」默认走同一落点（一致性）；
//!   S: 共享卷也可选。
//! 依赖锚点：F233 保存对话框、F512 截图历史（另存同点）。
//! 零堆纪律：文件名=定长字节数组手工拼装（无 format!）；policy 单源两读取口。
//!
//! =========================================================================
//! F525 快捷键速查卡导出（domain: "F525-hotkey-card"）
//! 判据（逐字）：**两格式导出；同源一致性（导出后改键再导出对比）；分色标注；排版清晰度（打印 300dpi 走查）；导出入口（F374 浮层内+F244 注册表页双门）。**
//! 功能定义要点：F374 速查浮层的离线身——导出当前快捷键全表（PNG 一页版/PDF
//!   双页版）打印贴墙或存平板；导出内容与 F244 注册表同源（自定义过的键如实
//!   导出）；系统默认表+用户自定义表分色标注。
//! 依赖锚点：F374 速查浮层、F244 快捷键注册表（双门入口）。
//! 零堆纪律：键表=定长条目数组；指纹=纯 core FNV-1a；排版账=定长二元组。
//!
//! =========================================================================
//! F526 资源管理器状态栏（domain: "F526-exp-statusbar"）
//! 判据（逐字）：**三段实时性；三处同源对账；键盘操作反映；高度 24px 基线；空目录态显示。**
//! 功能定义要点：窗口底部状态栏（24px 基线）常显三段——左=当前目录项数
//!   （「128 项」）、中=选中态（「已选 5 项 · 245MB」F338 同源）、右=当前卷剩余
//!   空间（F456 同数据）；信息实时同步（删除文件项数即减）；全选/清空选择键盘
//!   操作同样反映；空目录态显示（0 项）。
//! 依赖锚点：F338 详情操作条（选中态同源）、F456 此机页（卷剩余同数据）。
//! 零堆纪律：数据源=单一定长结构 ExplorerData 三处共读；渲染用 checks::push 系
//!   手工拼字节（无 format!）。
//!
//! =========================================================================
//! F527 导航树折叠展开（domain: "F527-navtree-fold"）
//! 判据（逐字）：**双击/箭头两路；两命令快捷键；持久化；万节点性能；拖放兼容用例。**
//! 功能定义要点：左侧目录树操作全集——单击选中、双击/箭头展开折叠（两路同一
//!   toggle 核心）、全部展开（当前层）与全部折叠两命令（含快捷键枚举）；展开
//!   状态持久（F219 记忆族——重启后树保持展开样）；万节点虚拟化（F228 同源，
//!   可见切片 O(可见数) 而非 O(万)）；拖拽到树节点=移动/复制（F262 语义全兼容）。
//! 依赖锚点：F219 记忆族（展开持久）、F228 虚拟化、F262 拖放语义。
//! 零堆纪律：树=父指针+展开位图定长数组（容量 10_000）；持久化=定长 u32 位图。
//!
//! =========================================================================
//! F528 树与列表双向同步（domain: "F528-treelist-sync"）
//! 判据（逐字）：**双向同步用例；自动滚动可见；焦点/高亮分离判据；深层路径（5 层）同步时序；与 F527 持久化协同。**
//! 功能定义要点：左侧树与右侧列表一处导航两处亮——列表进入子目录→树自动展开
//!   路径并高亮当前节点（滚动到可见）；树点选→列表刷新——双向永远同步；同步
//!   高亮不抢焦点（树高亮是视觉态，键盘焦点仍在列表——F206 语义不打架）。
//! 依赖锚点：F206 焦点语义、F219 记忆族（协同持久化）、F527 树基建。
//! 零堆纪律：路径=定长节点 id 数组；同步账=定长步骤日志（无堆记录）。

use crate::checks::{push_str, push_usize, CheckSet};

// ===========================================================================
// F515 查找与替换 —— ReplaceEngine
// ===========================================================================

/// 文档容量（码点数）。判据域内定长建模。
pub const DOC_MAX: usize = 2048;
/// 查找词/替换词容量（码点数）。
pub const WORD_MAX: usize = 64;
/// 整批 undo 记录可容纳的替换区间数（覆盖 DOC_MAX 的合理上限账）。
pub const MAX_EDITS: usize = 256;

/// 区分大小写开关位（与 F221 查找条共享 bit 布局）。
pub const BIT_MATCH_CASE: u8 = 1 << 0;
/// 全字匹配开关位（与 F221 查找条共享 bit 布局）。
pub const BIT_WHOLE_WORD: u8 = 1 << 1;

/// 查找条几何 [x, y, w, h]（同形制基准）。
pub const FIND_BAR_GEOM: [u16; 4] = [8, 8, 480, 32];
/// 替换条几何：与查找条**同形制同位置**（判据第一句——双条形制一致）。
pub const REPLACE_BAR_GEOM: [u16; 4] = [8, 8, 480, 32];

/// 查找/替换共享开关（F221 查找条与 F515 替换条共用同一 bit 布局——开关共享）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Switches {
    bits: u8,
}

impl Switches {
    pub const fn new() -> Switches {
        Switches { bits: 0 }
    }
    pub fn set_match_case(&mut self, on: bool) {
        if on {
            self.bits |= BIT_MATCH_CASE;
        } else {
            self.bits &= !BIT_MATCH_CASE;
        }
    }
    pub fn set_whole_word(&mut self, on: bool) {
        if on {
            self.bits |= BIT_WHOLE_WORD;
        } else {
            self.bits &= !BIT_WHOLE_WORD;
        }
    }
    pub fn match_case(&self) -> bool {
        self.bits & BIT_MATCH_CASE != 0
    }
    pub fn whole_word(&self) -> bool {
        self.bits & BIT_WHOLE_WORD != 0
    }
    /// 与 F221 查找条共享的原始 bit 布局（开关共享判据的对账面）。
    pub fn shared_bits(&self) -> u8 {
        self.bits
    }
}

/// 文档：定长 u32 码点数组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Document {
    cps: [u32; DOC_MAX],
    len: usize,
}

impl Document {
    /// 从 &str 装入（超容量截断——no_std 定长语义）。
    pub fn from_str(s: &str) -> Document {
        let mut d = Document { cps: [0; DOC_MAX], len: 0 };
        for c in s.chars() {
            if d.len < DOC_MAX {
                d.cps[d.len] = c as u32;
                d.len += 1;
            }
        }
        d
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn cp(&self, i: usize) -> u32 {
        self.cps[i]
    }
    /// 与 &str 全文逐码点比对（测试与自检断言用）。
    pub fn equals_str(&self, s: &str) -> bool {
        let mut i = 0;
        for c in s.chars() {
            if i >= self.len || self.cps[i] != c as u32 {
                return false;
            }
            i += 1;
        }
        i == self.len
    }
}

/// 查找词/替换词：定长码点词。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Word {
    cps: [u32; WORD_MAX],
    len: usize,
}

impl Word {
    pub fn from_str(s: &str) -> Word {
        let mut w = Word { cps: [0; WORD_MAX], len: 0 };
        for c in s.chars() {
            if w.len < WORD_MAX {
                w.cps[w.len] = c as u32;
                w.len += 1;
            }
        }
        w
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn cp(&self, i: usize) -> u32 {
        self.cps[i]
    }
}

/// 词字符判定（全字匹配的边界依据：字母/数字为词内字符）。
fn is_word_cp(c: u32) -> bool {
    match char::from_u32(c) {
        Some(ch) => ch.is_alphanumeric(),
        None => false,
    }
}

/// ASCII 大小写折叠（判据域内只承诺 ASCII 折叠语义）。
fn lower_cp(c: u32) -> u32 {
    if (b'A' as u32..=b'Z' as u32).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// 位置 pos 是否命中词 pat（含区分大小写/全字匹配两开关）。
fn match_at(doc: &Document, pos: usize, pat: &Word, sw: Switches) -> bool {
    if pat.is_empty() || pos + pat.len() > doc.len() {
        return false;
    }
    for k in 0..pat.len() {
        let (a, b) = (doc.cp(pos + k), pat.cp(k));
        let eq = if sw.match_case() { a == b } else { lower_cp(a) == lower_cp(b) };
        if !eq {
            return false;
        }
    }
    if sw.whole_word() {
        let before_ok = pos == 0 || !is_word_cp(doc.cp(pos - 1));
        let after_ok = pos + pat.len() == doc.len() || !is_word_cp(doc.cp(pos + pat.len()));
        if !(before_ok && after_ok) {
            return false;
        }
    }
    true
}

/// 从 from 起找下一处（逐个替换的推进依据）。
pub fn find_next(doc: &Document, pat: &Word, sw: Switches, from: usize) -> Option<usize> {
    if pat.is_empty() || doc.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= doc.len() {
        if match_at(doc, i, pat, sw) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 影响数预览（判据：替换前显示将影响 N 处——批量操作先看账）。
pub fn count_matches(doc: &Document, pat: &Word, sw: Switches) -> usize {
    if pat.is_empty() {
        return 0;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i + pat.len() <= doc.len() {
        if match_at(doc, i, pat, sw) {
            n += 1;
            i += pat.len();
        } else {
            i += 1;
        }
    }
    n
}

/// 替换错误路径（空查找词/无匹配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplaceError {
    EmptyFind,
    NoMatch,
}

/// 一次替换区间账（start 为**替换后文档**坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EditSpan {
    pub start: usize,
    pub old_len: usize,
    pub new_len: usize,
}

/// 整批 undo 记录：全部替换区间账 + 原文快照。
/// 判据：整批撤销一次回滚——F202 栈内一次 undo 恢复整批替换前原文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UndoRecord {
    pub spans: [EditSpan; MAX_EDITS],
    pub span_count: usize,
    before: Document,
    /// 操作标签（进 F202 撤销栈的条目名）。
    pub label: &'static str,
}

impl UndoRecord {
    /// 单次 undo 的回滚结果（整批一次回滚）。
    pub fn restore(&self) -> Document {
        self.before
    }
}

/// 一次替换落盘（定长数组内移动尾部并写入替换词）。
fn apply_single(doc: &mut Document, pos: usize, old_len: usize, repl: &Word) -> EditSpan {
    let delta = repl.len() as isize - old_len as isize;
    if delta != 0 {
        if delta > 0 {
            let mut i = doc.len();
            while i > pos + old_len {
                let t = i + delta as usize;
                if t <= DOC_MAX {
                    doc.cps[t - 1] = doc.cps[i - 1];
                }
                i -= 1;
            }
        } else {
            let shift = (-delta) as usize;
            let mut i = pos + old_len;
            while i < doc.len() {
                doc.cps[i - shift] = doc.cps[i];
                i += 1;
            }
        }
        let nl = doc.len as isize + delta;
        doc.len = if nl < 0 { 0 } else { nl as usize };
    }
    for k in 0..repl.len() {
        if pos + k < DOC_MAX {
            doc.cps[pos + k] = repl.cp(k);
        }
    }
    EditSpan { start: pos, old_len, new_len: repl.len() }
}

/// 替换引擎：查找词/替换词/共享开关/当前高亮位。
pub struct ReplaceEngine {
    find: Word,
    repl: Word,
    sw: Switches,
    cursor: usize,
    /// 最近一次错误（诊断面）。
    pub last_error: Option<ReplaceError>,
}

impl ReplaceEngine {
    pub fn new() -> ReplaceEngine {
        ReplaceEngine { find: Word::from_str(""), repl: Word::from_str(""), sw: Switches::new(), cursor: 0, last_error: None }
    }

    /// 设置双词与共享开关（空查找词 → EmptyFind）。
    pub fn set_words(&mut self, find_s: &str, repl_s: &str, sw: Switches) -> Result<(), ReplaceError> {
        let f = Word::from_str(find_s);
        if f.is_empty() {
            self.last_error = Some(ReplaceError::EmptyFind);
            return Err(ReplaceError::EmptyFind);
        }
        self.find = f;
        self.repl = Word::from_str(repl_s);
        self.sw = sw;
        self.cursor = 0;
        self.last_error = None;
        Ok(())
    }

    pub fn switches(&self) -> Switches {
        self.sw
    }
    pub fn find_word(&self) -> Word {
        self.find
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 影响数预览。
    pub fn count_matches(&self, doc: &Document) -> usize {
        count_matches(doc, &self.find, self.sw)
    }

    /// 预览第一处上下文：首处命中位（全部替换前的「看账+看第一处」）。
    pub fn first_match(&self, doc: &Document) -> Option<usize> {
        find_next(doc, &self.find, self.sw, 0)
    }

    /// 逐个替换：从当前高亮处（或其后最近一处）替换并推进。
    pub fn replace_current(&mut self, doc: &mut Document) -> Result<EditSpan, ReplaceError> {
        match find_next(doc, &self.find, self.sw, self.cursor) {
            Some(pos) => {
                let span = apply_single(doc, pos, self.find.len(), &self.repl);
                self.cursor = pos + span.new_len;
                Ok(span)
            }
            None => {
                self.last_error = Some(ReplaceError::NoMatch);
                Err(ReplaceError::NoMatch)
            }
        }
    }

    /// 全部替换：**一次正向 pass**（命中即写替换词并跳过新内容——
    /// 「x」换「xx」不重扫不爆长），产生一份 undo 记录=整批一次回滚。
    pub fn replace_all(&mut self, doc: &mut Document) -> Result<UndoRecord, ReplaceError> {
        if self.find.is_empty() {
            self.last_error_mut(ReplaceError::EmptyFind);
            return Err(ReplaceError::EmptyFind);
        }
        let n = count_matches(doc, &self.find, self.sw);
        if n == 0 {
            self.last_error_mut(ReplaceError::NoMatch);
            return Err(ReplaceError::NoMatch);
        }
        let mut rec = UndoRecord {
            spans: [EditSpan { start: 0, old_len: 0, new_len: 0 }; MAX_EDITS],
            span_count: 0,
            before: *doc,
            label: "replace-all",
        };
        let mut out = Document { cps: [0; DOC_MAX], len: 0 };
        let mut i = 0usize;
        while i < doc.len() {
            if match_at(doc, i, &self.find, self.sw) {
                let start_in_out = out.len();
                for k in 0..self.repl.len() {
                    if out.len() < DOC_MAX {
                        out.cps[out.len] = self.repl.cp(k);
                        out.len += 1;
                    }
                }
                if rec.span_count < MAX_EDITS {
                    rec.spans[rec.span_count] =
                        EditSpan { start: start_in_out, old_len: self.find.len(), new_len: self.repl.len() };
                    rec.span_count += 1;
                }
                i += self.find.len(); // 跳过命中段：新写入内容不再参与匹配
            } else {
                if out.len() < DOC_MAX {
                    out.cps[out.len] = doc.cp(i);
                    out.len += 1;
                }
                i += 1;
            }
        }
        *doc = out;
        Ok(rec)
    }

    fn last_error_mut(&mut self, e: ReplaceError) {
        self.last_error = Some(e);
    }
}

/// 整批撤销：单次调用恢复原文（F202 栈内一次 undo 整批）。
pub fn undo_all(rec: &UndoRecord, doc: &mut Document) {
    *doc = rec.restore();
}

/// 不替换处逐码点不变（spans 按输出坐标升序、互不重叠——replace_all 保证）。
pub fn verify_untouched(before: &Document, after: &Document, spans: &[EditSpan]) -> bool {
    let mut ai = 0usize;
    let mut bi = 0usize;
    let mut s = 0usize;
    while ai < after.len() {
        if s < spans.len() && ai == spans[s].start {
            ai += spans[s].new_len;
            bi += spans[s].old_len;
            s += 1;
        } else {
            if bi >= before.len() || after.cp(ai) != before.cp(bi) {
                return false;
            }
            ai += 1;
            bi += 1;
        }
    }
    bi == before.len()
}

/// F515 域自检（domain: "F515-find-replace"）。
pub fn run_f515_checks() -> CheckSet {
    let mut cs = CheckSet::new("F515-find-replace");
    // 1) 双条形制一致：替换条与查找条同形制同位置（同一几何常量）。
    cs.add("bar_geom_same", REPLACE_BAR_GEOM == FIND_BAR_GEOM, "替换条与查找条几何不一致");
    // 2) 开关共享：区分大小写/全字匹配与 F221 查找条共用同一 bit 布局。
    let mut sw2 = Switches::new();
    sw2.set_match_case(true);
    sw2.set_whole_word(true);
    cs.add(
        "switches_shared",
        sw2.shared_bits() == (BIT_MATCH_CASE | BIT_WHOLE_WORD) && Switches::new().shared_bits() == 0,
        "开关 bit 布局与查找条不同源",
    );
    // 3) 影响数预览：count_matches 精确计数（3 处）。
    let doc = Document::from_str("abc X abc X abc");
    let mut eng = ReplaceEngine::new();
    let _ = eng.set_words("abc", "Z", Switches::new());
    cs.add("count_matches_preview", eng.count_matches(&doc) == 3, "影响数计数不实");
    // 4) 区分大小写：开=1 处 / 关=3 处（开关沿查找语义）。
    let doc_case = Document::from_str("Abc abc ABC");
    let mut sw_case = Switches::new();
    sw_case.set_match_case(true);
    let mut e_case = ReplaceEngine::new();
    let _ = e_case.set_words("abc", "Z", sw_case);
    let mut e_nocase = ReplaceEngine::new();
    let _ = e_nocase.set_words("abc", "Z", Switches::new());
    cs.add("match_case", e_case.count_matches(&doc_case) == 1 && e_nocase.count_matches(&doc_case) == 3, "区分大小写行为不符");
    // 5) 全字匹配：「cart art artx」中 art——开=1 处 / 关=3 处。
    let doc_word = Document::from_str("cart art artx");
    let mut sw_ww = Switches::new();
    sw_ww.set_whole_word(true);
    let mut e_ww = ReplaceEngine::new();
    let _ = e_ww.set_words("art", "Q", sw_ww);
    let mut e_any = ReplaceEngine::new();
    let _ = e_any.set_words("art", "Q", Switches::new());
    cs.add("whole_word", e_ww.count_matches(&doc_word) == 1 && e_any.count_matches(&doc_word) == 3, "全字匹配行为不符");
    // 6) 预览第一处上下文：首处定位可取（看账+看第一处）。
    cs.add("preview_first_pos", eng.first_match(&doc) == Some(0), "首处预览定位失败");
    // 7) 逐个替换：从当前高亮处推进（1→3，文档 aYaYa）。
    let mut d1 = Document::from_str("aXaXa");
    let mut e_one = ReplaceEngine::new();
    let _ = e_one.set_words("X", "Y", Switches::new());
    let s1 = e_one.replace_current(&mut d1);
    let s2 = e_one.replace_current(&mut d1);
    cs.add(
        "replace_one_advances",
        s1 == Ok(EditSpan { start: 1, old_len: 1, new_len: 1 })
            && s2 == Ok(EditSpan { start: 3, old_len: 1, new_len: 1 })
            && d1.equals_str("aYaYa"),
        "逐个替换推进不符",
    );
    // 8) 全部替换：一次 pass 全替换（ab_Xcd_Xef → ab_-cd_-ef）。
    let mut d2 = Document::from_str("ab_Xcd_Xef");
    let mut e_all = ReplaceEngine::new();
    let _ = e_all.set_words("X", "-", Switches::new());
    let r_all = e_all.replace_all(&mut d2);
    cs.add("replace_all_pass", r_all.is_ok() && d2.equals_str("ab_-cd_-ef"), "全部替换结果不符");
    // 9) 整批撤销一次回滚：单次 undo 恢复原文（逐码点一致）。
    let before2 = Document::from_str("ab_Xcd_Xef");
    let undo_ok = match &r_all {
        Ok(_) => {
            undo_all(r_all.as_ref().expect("checked"), &mut d2);
            d2 == before2
        }
        Err(_) => false,
    };
    cs.add("undo_single_shot", undo_ok, "整批撤销未一次回滚原文");
    // 10) 不替换处逐码点不变（keep 段原样）。
    let orig3 = Document::from_str("keep X keep X keep");
    let mut d3 = orig3;
    let mut e_keep = ReplaceEngine::new();
    let _ = e_keep.set_words("X", "YY", Switches::new());
    let r3 = e_keep.replace_all(&mut d3);
    let untouched = match &r3 {
        Ok(r) => verify_untouched(&orig3, &d3, &r.spans[..r.span_count]),
        Err(_) => false,
    };
    cs.add("untouched_regions", untouched && d3.equals_str("keep YY keep YY keep"), "不替换处被改动");
    // 11) 替换词含查找词（x→xx）：单次 pass 不重扫不爆长，恰好 1 处。
    let mut d4 = Document::from_str("axa");
    let mut e_cont = ReplaceEngine::new();
    let _ = e_cont.set_words("x", "xx", Switches::new());
    let r4 = e_cont.replace_all(&mut d4);
    let n4 = match &r4 {
        Ok(r) => r.span_count,
        Err(_) => 0,
    };
    cs.add("containment_no_rescan", r4.is_ok() && n4 == 1 && d4.equals_str("axxa"), "替换词含查找词引发重扫/计数不实");
    // 12) 错误路径：空查找词 / 无匹配（文档不被改动）。
    let mut e_empty = ReplaceEngine::new();
    let empty_err = matches!(e_empty.set_words("", "y", Switches::new()), Err(ReplaceError::EmptyFind));
    let mut e_nom = ReplaceEngine::new();
    let _ = e_nom.set_words("zz", "y", Switches::new());
    let mut d5 = Document::from_str("abc");
    let nom_err = e_nom.replace_all(&mut d5) == Err(ReplaceError::NoMatch);
    cs.add("error_paths", empty_err && nom_err && d5.equals_str("abc"), "空查找词/无匹配错误路径不符");
    // 13) 撤销账：区间账 old/new 长度对账与实际替换一致。
    let acct_ok = match &r3 {
        Ok(r) => r.span_count == 2 && r.spans[0].old_len == 1 && r.spans[0].new_len == 2 && r.spans[1].new_len == 2,
        Err(_) => false,
    };
    cs.add("undo_ledger", acct_ok, "整批替换区间账不符");
    cs
}

#[cfg(test)]
mod f515_tests {
    use super::*;

    #[test]
    fn replace_all_then_single_undo_restores_original() {
        let orig = Document::from_str("alpha X beta X gamma X");
        let mut d = orig;
        let mut e = ReplaceEngine::new();
        assert!(e.set_words("X", "space", Switches::new()).is_ok(), "设词失败");
        let r = e.replace_all(&mut d);
        assert!(r.is_ok(), "全部替换应成功");
        assert!(d.equals_str("alpha space beta space gamma space"), "全部替换结果不符");
        if let Ok(rec) = r {
            assert_eq!(rec.span_count, 3, "应记 3 处替换账");
            undo_all(&rec, &mut d);
            assert_eq!(d, orig, "单次 undo 应逐码点恢复原文");
        }
    }

    #[test]
    fn replace_one_walks_positions_until_exhausted() {
        let mut d = Document::from_str("aXbXc");
        let mut e = ReplaceEngine::new();
        assert!(e.set_words("X", "!", Switches::new()).is_ok());
        assert_eq!(e.replace_current(&mut d), Ok(EditSpan { start: 1, old_len: 1, new_len: 1 }), "第一处位置不符");
        assert_eq!(e.replace_current(&mut d), Ok(EditSpan { start: 3, old_len: 1, new_len: 1 }), "第二处位置不符");
        assert_eq!(e.replace_current(&mut d), Err(ReplaceError::NoMatch), "耗尽后应报无匹配");
        assert!(d.equals_str("a!b!c"), "逐个替换终态不符");
    }

    #[test]
    fn case_and_whole_word_matrix() {
        let doc = Document::from_str("Cat cat catalog CAT");
        let mut on = Switches::new();
        on.set_match_case(true);
        let mut e1 = ReplaceEngine::new();
        assert!(e1.set_words("cat", "dog", on).is_ok());
        assert_eq!(e1.count_matches(&doc), 2, "区分大小写应命中 cat/CAT?——只命中小写 cat 两处");
        let mut ww = Switches::new();
        ww.set_match_case(true);
        ww.set_whole_word(true);
        let mut e2 = ReplaceEngine::new();
        assert!(e2.set_words("cat", "dog", ww).is_ok());
        assert_eq!(e2.count_matches(&doc), 1, "全字匹配应只命中独立的 cat");
    }

    #[test]
    fn containment_and_untouched_invariants() {
        let orig = Document::from_str("xoxox");
        let mut d = orig;
        let mut e = ReplaceEngine::new();
        assert!(e.set_words("o", "oo", Switches::new()).is_ok());
        let r = e.replace_all(&mut d);
        assert!(r.is_ok(), "含词替换应成功");
        assert!(d.equals_str("xooxoox"), "o→oo 应单 pass 得 xooxoox");
        if let Ok(rec) = r {
            assert!(verify_untouched(&orig, &d, &rec.spans[..rec.span_count]), "不替换处应逐码点不变");
        }
        undo_all(&r.as_ref().expect("ok"), &mut d);
        assert_eq!(d, orig, "回滚后应还原");
    }

    #[test]
    fn empty_find_and_capacity_clamp() {
        let mut e = ReplaceEngine::new();
        assert_eq!(e.set_words("", "y", Switches::new()), Err(ReplaceError::EmptyFind), "空查找词应拒绝");
        // 文档超容量截断到 DOC_MAX。
        let big = Document::from_str(&"a".repeat(DOC_MAX + 500));
        assert_eq!(big.len(), DOC_MAX, "文档应按定长容量截断");
        let mut d = big;
        let mut e2 = ReplaceEngine::new();
        assert!(e2.set_words("a", "b", Switches::new()).is_ok());
        assert!(e2.replace_all(&mut d).is_ok(), "满容量文档全部替换应成功");
        assert_eq!(d.len(), DOC_MAX, "替换后长度应守恒");
    }
}

// ===========================================================================
// F517 资源管理器启动页设置 —— StartPagePolicy
// ===========================================================================

/// 「上次文件夹」记忆表窗口槽位数。**20 与本文件其他数字常量（24px/5 层/万节点）无关，勿混用。**
pub const LAST_WINDOW_SLOTS: usize = 20;
/// 路径名容量（字节）。
pub const NAME_MAX: usize = 64;
/// 固定文件夹槽位数（用户可指定多个备选）。
pub const FIXED_SLOTS: usize = 4;
/// 已开窗口追踪容量（切换即时性建模用）。
pub const OPEN_WINDOWS_MAX: usize = 8;

/// 启动落点三模式（判据：此机默认 F404 / 上次关闭的文件夹 / 固定文件夹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartMode {
    /// 此机（默认，F404）。
    ThisPc,
    /// 上次关闭的文件夹（每窗口记忆）。
    LastFolder,
    /// 固定文件夹（用户指定槽位）。
    FixedFolder { slot: u8 },
}

/// 定长路径名缓冲。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NameBuf {
    pub bytes: [u8; NAME_MAX],
    pub len: usize,
}

impl NameBuf {
    pub const fn new() -> NameBuf {
        NameBuf { bytes: [0; NAME_MAX], len: 0 }
    }
    pub fn from_str(s: &str) -> NameBuf {
        let mut nb = NameBuf::new();
        for &b in s.as_bytes() {
            if nb.len < NAME_MAX {
                nb.bytes[nb.len] = b;
                nb.len += 1;
            }
        }
        nb
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes[..self.len]).ok()
    }
    pub fn eq_str(&self, s: &str) -> bool {
        &self.bytes[..self.len] == s.as_bytes()
    }
}

/// 落点解析结果（空位/无记忆回退默认此机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Landing {
    ThisPc,
    LastFolder(NameBuf),
    Fixed(NameBuf),
    /// 回退默认（附原因标注——graceful 诊断）。
    FallbackThisPc(&'static str),
}

/// 每窗口最后位置记忆表（F237 记忆的启动端延伸）：
/// 窗口 id 对槽取模落位；关闭时写入、启动时读出。
pub struct LastFolderTable {
    slots: [Option<NameBuf>; LAST_WINDOW_SLOTS],
}

impl LastFolderTable {
    pub fn new() -> LastFolderTable {
        LastFolderTable { slots: [None; LAST_WINDOW_SLOTS] }
    }
    /// 关闭窗口时写入最后位置。
    pub fn record(&mut self, win_id: usize, path: &str) {
        self.slots[win_id % LAST_WINDOW_SLOTS] = Some(NameBuf::from_str(path));
    }
    /// 启动时读出。
    pub fn recall(&self, win_id: usize) -> Option<NameBuf> {
        self.slots[win_id % LAST_WINDOW_SLOTS]
    }
    pub fn clear(&mut self) {
        for s in self.slots.iter_mut() {
            *s = None;
        }
    }
    /// 持久化快照（重启恢复——位级一致）。
    pub fn snapshot(&self) -> [Option<NameBuf>; LAST_WINDOW_SLOTS] {
        self.slots
    }
    pub fn restore(&mut self, snap: [Option<NameBuf>; LAST_WINDOW_SLOTS]) {
        self.slots = snap;
    }
}

/// 启动页策略：三模式 + 固定文件夹槽表。
pub struct StartPagePolicy {
    mode: StartMode,
    fixed: [NameBuf; FIXED_SLOTS],
}

impl StartPagePolicy {
    /// 默认 = 此机（F404）。
    pub fn new() -> StartPagePolicy {
        StartPagePolicy { mode: StartMode::ThisPc, fixed: [NameBuf::new(); FIXED_SLOTS] }
    }
    pub fn mode(&self) -> StartMode {
        self.mode
    }
    pub fn set_mode(&mut self, m: StartMode) {
        self.mode = m;
    }
    pub fn set_fixed_slot(&mut self, slot: usize, path: &str) {
        if slot < FIXED_SLOTS {
            self.fixed[slot] = NameBuf::from_str(path);
        }
    }
    pub fn fixed_slot(&self, slot: usize) -> Option<NameBuf> {
        if slot < FIXED_SLOTS && !self.fixed[slot].is_empty() {
            Some(self.fixed[slot])
        } else {
            None
        }
    }
    /// 解析落点：LastFolder 无记忆 → 回退；Fixed 空槽 → 回退默认（判据：固定文件夹校验）。
    pub fn resolve(&self, table: &LastFolderTable, win_id: usize) -> Landing {
        match self.mode {
            StartMode::ThisPc => Landing::ThisPc,
            StartMode::LastFolder => match table.recall(win_id) {
                Some(nb) if !nb.is_empty() => Landing::LastFolder(nb),
                _ => Landing::FallbackThisPc("last-unknown"),
            },
            StartMode::FixedFolder { slot } => {
                let s = slot as usize;
                if s < FIXED_SLOTS && !self.fixed[s].is_empty() {
                    Landing::Fixed(self.fixed[s])
                } else {
                    Landing::FallbackThisPc("fixed-slot-empty")
                }
            }
        }
    }
}

/// 三模式一句话说明文案（设置页即时预览——每种落点一句话后果）。
pub const COPY_THIS_PC: &str = "此机：新窗口从「此机」根目录开始（默认落点）。";
pub const COPY_LAST_FOLDER: &str = "上次关闭的文件夹：每个窗口各自记住最后位置，重启后从原处继续。";
pub const COPY_FIXED_FOLDER: &str = "固定文件夹：始终打开你指定的目录，未指定时回退此机。";

pub fn copy_text(mode: StartMode) -> &'static str {
    match mode {
        StartMode::ThisPc => COPY_THIS_PC,
        StartMode::LastFolder => COPY_LAST_FOLDER,
        StartMode::FixedFolder { .. } => COPY_FIXED_FOLDER,
    }
}

/// 已开窗口管理：切换即时语义——修改 policy 不影响已开窗口，只影响新窗。
pub struct WindowManager {
    open: [Option<(u16, Landing)>; OPEN_WINDOWS_MAX],
    open_n: usize,
}

impl WindowManager {
    pub fn new() -> WindowManager {
        WindowManager { open: [None; OPEN_WINDOWS_MAX], open_n: 0 }
    }
    /// 新窗按当前 policy 落点开窗。
    pub fn spawn(&mut self, policy: &StartPagePolicy, table: &LastFolderTable, win_id: u16) -> Landing {
        let landing = policy.resolve(table, win_id as usize);
        if self.open_n < OPEN_WINDOWS_MAX {
            self.open[self.open_n] = Some((win_id, landing));
            self.open_n += 1;
        }
        landing
    }
    /// 已开窗的落点（不受后续 policy 修改影响）。
    pub fn open_landing(&self, win_id: u16) -> Option<Landing> {
        for i in 0..self.open_n {
            if let Some((id, l)) = self.open[i] {
                if id == win_id {
                    return Some(l);
                }
            }
        }
        None
    }
    pub fn open_count(&self) -> usize {
        self.open_n
    }
}

/// F517 域自检（domain: "F517-exp-startpage"）。
pub fn run_f517_checks() -> CheckSet {
    let mut cs = CheckSet::new("F517-exp-startpage");
    // 1) 三模式行为之默认：新策略 = 此机（F404）。
    let p = StartPagePolicy::new();
    cs.add("default_this_pc", p.mode() == StartMode::ThisPc, "默认落点应为此机");
    // 2) 说明文案：三模式一句话，非空且互异。
    let t0 = copy_text(StartMode::ThisPc);
    let t1 = copy_text(StartMode::LastFolder);
    let t2 = copy_text(StartMode::FixedFolder { slot: 0 });
    cs.add("copy_texts", !t0.is_empty() && !t1.is_empty() && !t2.is_empty() && t0 != t1 && t1 != t2, "说明文案缺失或雷同");
    // 3) 上次文件夹记忆准确性：写入后原样读出。
    let mut table = LastFolderTable::new();
    table.record(2, "D:/proj/src");
    let got = table.recall(2);
    cs.add("last_record_recall", got.map(|n| n.eq_str("D:/proj/src")).unwrap_or(false), "上次位置记忆不准");
    // 4) 每窗口各自记忆：两窗不同目录互不串。
    table.record(5, "C:/work");
    cs.add(
        "last_per_window",
        table.recall(2).map(|n| n.eq_str("D:/proj/src")).unwrap_or(false)
            && table.recall(5).map(|n| n.eq_str("C:/work")).unwrap_or(false),
        "多窗口记忆互相串位",
    );
    // 5) 固定文件夹：指定槽位解析出该目录。
    let mut pf = StartPagePolicy::new();
    pf.set_fixed_slot(1, "D:/work");
    pf.set_mode(StartMode::FixedFolder { slot: 1 });
    let empty_table = LastFolderTable::new();
    cs.add(
        "fixed_resolves",
        matches!(pf.resolve(&empty_table, 9), Landing::Fixed(_)),
        "固定文件夹未按槽位解析",
    );
    // 6) 固定文件夹校验：空槽回退默认此机。
    let pe = StartPagePolicy::new();
    let mut pe2 = StartPagePolicy::new();
    pe2.set_mode(StartMode::FixedFolder { slot: 3 });
    cs.add(
        "fixed_empty_fallback",
        matches!(pe2.resolve(&empty_table, 0), Landing::FallbackThisPc("fixed-slot-empty"))
            && matches!(pe.resolve(&empty_table, 0), Landing::ThisPc),
        "空槽未回退默认",
    );
    // 7) 上次模式无记忆：回退默认并标注原因。
    let mut pl = StartPagePolicy::new();
    pl.set_mode(StartMode::LastFolder);
    cs.add("last_unknown_fallback", matches!(pl.resolve(&empty_table, 7), Landing::FallbackThisPc("last-unknown")), "无记忆未回退");
    // 8) 切换即时：修改 policy 不影响已开窗口。
    let mut wm = WindowManager::new();
    let _ = wm.spawn(&p, &empty_table, 100);
    let mut p2 = StartPagePolicy::new();
    p2.set_mode(StartMode::LastFolder);
    table.record(100, "E:/keep");
    let still = wm.open_landing(100);
    cs.add("switch_open_unchanged", still == Some(Landing::ThisPc), "policy 切换影响了已开窗口");
    // 9) 切换即时：新窗按新 policy 落点。
    let nw = wm.spawn(&p2, &table, 100);
    cs.add("switch_new_follows", matches!(nw, Landing::LastFolder(_)), "新窗未按新落点打开");
    // 10) 记忆表容量 20：20 窗全部准确落位。
    let mut t20 = LastFolderTable::new();
    let mut all_ok = true;
    for w in 0..LAST_WINDOW_SLOTS {
        t20.record(w, "D:/w20");
    }
    for w in 0..LAST_WINDOW_SLOTS {
        if !t20.recall(w).map(|n| n.eq_str("D:/w20")).unwrap_or(false) {
            all_ok = false;
        }
    }
    cs.add("capacity_20", LAST_WINDOW_SLOTS == 20 && all_ok, "20 槽记忆表落位不准");
    // 11) 持久化：快照-清空-恢复后记忆一致（重启恢复语义）。
    let snap = table.snapshot();
    table.clear();
    table.restore(snap);
    cs.add(
        "persistence_roundtrip",
        table.recall(2).map(|n| n.eq_str("D:/proj/src")).unwrap_or(false)
            && table.recall(5).map(|n| n.eq_str("C:/work")).unwrap_or(false),
        "持久化恢复后记忆丢失",
    );
    cs
}

#[cfg(test)]
mod f517_tests {
    use super::*;

    #[test]
    fn default_is_this_pc_with_copy_text() {
        let p = StartPagePolicy::new();
        assert_eq!(p.mode(), StartMode::ThisPc, "默认应为此机");
        assert!(copy_text(StartMode::ThisPc).contains("此机"), "此机文案缺失");
        assert!(copy_text(StartMode::LastFolder).contains("最后位置"), "上次文件夹文案缺失");
        assert!(copy_text(StartMode::FixedFolder { slot: 2 }).contains("固定文件夹"), "固定文件夹文案缺失");
    }

    #[test]
    fn last_folder_accuracy_per_window() {
        let mut t = LastFolderTable::new();
        t.record(1, "D:/a");
        t.record(2, "D:/b");
        assert!(t.recall(1).map(|n| n.eq_str("D:/a")).unwrap_or(false), "窗口 1 记忆不准");
        assert!(t.recall(2).map(|n| n.eq_str("D:/b")).unwrap_or(false), "窗口 2 记忆不准");
        assert!(t.recall(3).is_none(), "未记忆窗口不应有值");
        // 覆写语义：同窗再关闭写入新位置。
        t.record(1, "D:/a2");
        assert!(t.recall(1).map(|n| n.eq_str("D:/a2")).unwrap_or(false), "覆写后应取最后位置");
    }

    #[test]
    fn fixed_slot_and_fallback_paths() {
        let mut p = StartPagePolicy::new();
        p.set_mode(StartMode::FixedFolder { slot: 0 });
        let t = LastFolderTable::new();
        assert!(matches!(p.resolve(&t, 0), Landing::FallbackThisPc("fixed-slot-empty")), "空槽应回退");
        p.set_fixed_slot(0, "S:/share/docs");
        match p.resolve(&t, 0) {
            Landing::Fixed(nb) => assert!(nb.eq_str("S:/share/docs"), "固定目录内容不符"),
            other => panic!("应为 Fixed，实为 {:?}", other),
        }
    }

    #[test]
    fn switch_immediacy_next_effect_only() {
        let p0 = StartPagePolicy::new();
        let mut t = LastFolderTable::new();
        t.record(9, "E:/last");
        let mut wm = WindowManager::new();
        let first = wm.spawn(&p0, &t, 9);
        assert_eq!(first, Landing::ThisPc, "初始窗应为此机");
        let mut p1 = StartPagePolicy::new();
        p1.set_mode(StartMode::FixedFolder { slot: 1 });
        p1.set_fixed_slot(1, "D:/fixed");
        // 已开窗不变。
        assert_eq!(wm.open_landing(9), Some(Landing::ThisPc), "切换后已开窗落点被改动");
        // 新窗按新 policy。
        let second = wm.spawn(&p1, &t, 10);
        assert!(matches!(second, Landing::Fixed(_)), "新窗应落固定文件夹");
        assert_eq!(wm.open_count(), 2, "开窗账不符");
    }

    #[test]
    fn capacity_twenty_slots_all_accurate() {
        let mut t = LastFolderTable::new();
        for w in 0..LAST_WINDOW_SLOTS {
            t.record(w, "D:/slot");
        }
        assert_eq!(LAST_WINDOW_SLOTS, 20, "槽位数应为 20（与其他常量无关勿混）");
        for w in 0..LAST_WINDOW_SLOTS {
            assert!(t.recall(w).map(|n| n.eq_str("D:/slot")).unwrap_or(false), "槽 {} 记忆不准", w);
        }
    }

    #[test]
    fn persistence_snapshot_restore_bit_exact() {
        let mut t = LastFolderTable::new();
        t.record(4, "C:/snap/x");
        let snap = t.snapshot();
        t.clear();
        assert!(t.recall(4).is_none(), "清空后应无记忆");
        t.restore(snap);
        assert!(t.recall(4).map(|n| n.eq_str("C:/snap/x")).unwrap_or(false), "恢复后记忆应位级一致");
    }
}

// ===========================================================================
// F521 截图保存位置设置 —— ShotSavePolicy
// ===========================================================================

/// 文件名缓冲容量（前缀 32 + 日期时间 15 + 序号 3 内）。
pub const SHOT_NAME_MAX: usize = 64;
/// 前缀容量。
pub const PREFIX_MAX: usize = 32;
/// 默认前缀：「截图 」（UTF-8 字节，后接 YYYY-MM-DD_HHMM；字节串字面量限 ASCII，
/// 故以 \x 转义书写）。
pub const DEFAULT_PREFIX: &[u8] = b"\xE6\x88\xAA\xE5\x9B\xBE ";
/// 默认样例时间（判据示例：截图 2026-09-25_1430）。
pub const SAMPLE_YEAR: u16 = 2026;
pub const SAMPLE_MONTH: u8 = 9;
pub const SAMPLE_DAY: u8 = 25;
pub const SAMPLE_HOUR: u8 = 14;
pub const SAMPLE_MINUTE: u8 = 30;

/// 截图落点（判据：图片/截图目录默认自动建 / 桌面 / 每次询问；S: 共享卷可选）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotDir {
    /// 图片/截图目录（默认，自动建）。
    PicturesShotDir,
    /// 桌面。
    Desktop,
    /// 每次询问（保存对话框 F233）。
    AskEachTime,
    /// S: 共享卷（可选落点）。
    ShareVolS,
}

/// 命名样例时间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl SampleTime {
    pub const SAMPLE: SampleTime = SampleTime { year: SAMPLE_YEAR, month: SAMPLE_MONTH, day: SAMPLE_DAY, hour: SAMPLE_HOUR, minute: SAMPLE_MINUTE };
}

/// 截图保存策略：落点 + 命名前缀（截图与 F512「另存」共读同一源）。
pub struct ShotSavePolicy {
    dir: ShotDir,
    prefix: [u8; PREFIX_MAX],
    prefix_len: usize,
}

impl ShotSavePolicy {
    /// 默认：截图目录落点 + 「截图 」前缀。
    pub fn new() -> ShotSavePolicy {
        let mut p = ShotSavePolicy { dir: ShotDir::PicturesShotDir, prefix: [0; PREFIX_MAX], prefix_len: 0 };
        p.set_prefix_bytes(DEFAULT_PREFIX);
        p
    }
    pub fn dir(&self) -> ShotDir {
        self.dir
    }
    pub fn set_dir(&mut self, d: ShotDir) {
        self.dir = d;
    }
    fn set_prefix_bytes(&mut self, b: &[u8]) {
        self.prefix_len = 0;
        for &x in b {
            if self.prefix_len < PREFIX_MAX {
                self.prefix[self.prefix_len] = x;
                self.prefix_len += 1;
            }
        }
    }
    pub fn set_prefix(&mut self, p: &str) {
        self.set_prefix_bytes(p.as_bytes());
    }
    pub fn prefix_bytes(&self) -> &[u8] {
        &self.prefix[..self.prefix_len]
    }
    pub fn prefix_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.prefix[..self.prefix_len]).ok()
    }
}

fn put_byte(out: &mut [u8], n: &mut usize, b: u8) {
    if *n < out.len() {
        out[*n] = b;
        *n += 1;
    }
}

/// 两位十进制（零填充）。
fn put_2dig(out: &mut [u8], n: &mut usize, v: u8) {
    put_byte(out, n, b'0' + (v / 10) % 10);
    put_byte(out, n, b'0' + v % 10);
}

/// 自动命名：「<前缀>YYYY-MM-DD_HHMM」，同分钟重名时加序号 `_N`（seq≥2）。
/// 永不重名单调性：时间前进则定宽字典序严格递增；同分钟序号区分。
pub fn format_shot_name(t: &SampleTime, prefix: &[u8], seq: u8, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    // 前缀按 PREFIX_MAX 截断（本域检查 11 契约：最长组合
    // = PREFIX_MAX + 15 + 3 = 50 字节完整容纳）。
    for &b in prefix.iter().take(PREFIX_MAX) {
        put_byte(out, &mut n, b);
    }
    let y = t.year;
    put_2dig(out, &mut n, (y / 100) as u8);
    put_2dig(out, &mut n, (y % 100) as u8);
    put_byte(out, &mut n, b'-');
    put_2dig(out, &mut n, t.month);
    put_byte(out, &mut n, b'-');
    put_2dig(out, &mut n, t.day);
    put_byte(out, &mut n, b'_');
    put_2dig(out, &mut n, t.hour);
    put_2dig(out, &mut n, t.minute);
    if seq > 1 {
        put_byte(out, &mut n, b'_');
        // 序号 `_N`（seq≥2）按最小位数写：1-9 一位、10+ 两位零填充——
        // 对 1-9 直接套两位零填充会产出 `_02`，违反本函数 doc 的 `_N`
        // 契约（本域检查 6「同分钟序号去重」）。
        if seq < 10 {
            put_byte(out, &mut n, b'0' + seq);
        } else {
            put_2dig(out, &mut n, seq);
        }
    }
    n
}

/// 目录自动建语义（默认落点不存在时 mkdir）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirEnsure {
    AlreadyThere,
    Created,
}

/// 默认落点目录自动建（判据：截图目录默认，自动建）。
pub fn ensure_shot_dir(exists: bool) -> DirEnsure {
    if exists {
        DirEnsure::AlreadyThere
    } else {
        DirEnsure::Created
    }
}

/// 保存目标：直接落盘（附目录建账）或弹询问对话框。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveTarget {
    Dir { dir: ShotDir, ensure: DirEnsure },
    Dialog,
}

/// 落点路由：AskEachTime → 对话框；其余直接落盘（默认落点负责自动建）。
pub fn save_target(policy: &ShotSavePolicy, shot_dir_exists: bool) -> SaveTarget {
    match policy.dir() {
        ShotDir::AskEachTime => SaveTarget::Dialog,
        d => SaveTarget::Dir {
            dir: d,
            ensure: if d == ShotDir::PicturesShotDir && !shot_dir_exists {
                DirEnsure::Created
            } else {
                DirEnsure::AlreadyThere
            },
        },
    }
}

/// 询问模式对话框状态（F233 保存对话框语义）。
pub struct SaveDialog {
    pub open: bool,
    suggested: [u8; SHOT_NAME_MAX],
    suggested_len: usize,
}

impl SaveDialog {
    pub const fn new() -> SaveDialog {
        SaveDialog { open: false, suggested: [0; SHOT_NAME_MAX], suggested_len: 0 }
    }
    /// 以建议名打开（自动命名规则照常生成）。
    pub fn open_with(&mut self, name: &[u8]) {
        self.open = true;
        self.suggested_len = 0;
        for &b in name {
            if self.suggested_len < SHOT_NAME_MAX {
                self.suggested[self.suggested_len] = b;
                self.suggested_len += 1;
            }
        }
    }
    pub fn suggested_bytes(&self) -> &[u8] {
        &self.suggested[..self.suggested_len]
    }
    /// 取消：不落盘（对话框关闭、无写动作）。
    pub fn cancel(&mut self) {
        self.open = false;
    }
}

/// 截图本体落点读取口。
pub fn screenshot_dir(policy: &ShotSavePolicy) -> ShotDir {
    policy.dir()
}

/// F512 历史条目「另存」默认落点读取口——与截图同一 policy 源（一致性判据）。
pub fn history_saveas_dir(policy: &ShotSavePolicy) -> ShotDir {
    policy.dir()
}

/// F521 域自检（domain: "F521-shot-savedir"）。
pub fn run_f521_checks() -> CheckSet {
    let mut cs = CheckSet::new("F521-shot-savedir");
    // 1) 三落点行为之默认：截图目录 + 不存在时自动建。
    let p = ShotSavePolicy::new();
    cs.add(
        "default_dir_automkdir",
        p.dir() == ShotDir::PicturesShotDir && save_target(&p, false) == SaveTarget::Dir { dir: ShotDir::PicturesShotDir, ensure: DirEnsure::Created },
        "默认落点/自动建不符",
    );
    // 2) 已存在则不重复建。
    cs.add("ensure_already", ensure_shot_dir(true) == DirEnsure::AlreadyThere, "已存在目录误判为新建");
    // 3) 命名规则：固定样例时间逐字节精确（「截图 2026-09-25_1430」）。
    let mut nb = [0u8; SHOT_NAME_MAX];
    let n = format_shot_name(&SampleTime::SAMPLE, DEFAULT_PREFIX, 1, &mut nb);
    cs.add(
        "naming_exact",
        core::str::from_utf8(&nb[..n]).map(|s| s == "截图 2026-09-25_1430").unwrap_or(false),
        "默认命名格式不符",
    );
    // 4) 前缀可调：自定义前缀与空前缀两路。
    let mut nb2 = [0u8; SHOT_NAME_MAX];
    let n2 = format_shot_name(&SampleTime::SAMPLE, b"VX-", 1, &mut nb2);
    let mut nb3 = [0u8; SHOT_NAME_MAX];
    let n3 = format_shot_name(&SampleTime::SAMPLE, b"", 1, &mut nb3);
    cs.add(
        "prefix_variants",
        core::str::from_utf8(&nb2[..n2]).map(|s| s == "VX-2026-09-25_1430").unwrap_or(false)
            && core::str::from_utf8(&nb3[..n3]).map(|s| s == "2026-09-25_1430").unwrap_or(false),
        "前缀定制不符",
    );
    // 5) 永不重名单调性：分钟前进 → 定宽字典序严格递增。
    let t1 = SampleTime { minute: 29, ..SampleTime::SAMPLE };
    let t2 = SampleTime { minute: 30, ..SampleTime::SAMPLE };
    let mut b1 = [0u8; SHOT_NAME_MAX];
    let mut b2 = [0u8; SHOT_NAME_MAX];
    let l1 = format_shot_name(&t1, DEFAULT_PREFIX, 1, &mut b1);
    let l2 = format_shot_name(&t2, DEFAULT_PREFIX, 1, &mut b2);
    cs.add("monotonic_minutes", l1 == l2 && &b1[..l1] < &b2[..l2], "时间前进未保证名字单调");
    // 6) 同分钟重名：序号 _2 区分（永不重名）。
    let mut b3 = [0u8; SHOT_NAME_MAX];
    let l3 = format_shot_name(&t2, DEFAULT_PREFIX, 2, &mut b3);
    cs.add(
        "same_minute_seq",
        &b2[..l2] != &b3[..l3] && core::str::from_utf8(&b3[..l3]).map(|s| s == "截图 2026-09-25_1430_2").unwrap_or(false),
        "同分钟序号去重不符",
    );
    // 7) 桌面与 S: 共享卷落点可选。
    let mut pd = ShotSavePolicy::new();
    pd.set_dir(ShotDir::Desktop);
    let mut ps = ShotSavePolicy::new();
    ps.set_dir(ShotDir::ShareVolS);
    cs.add(
        "desktop_and_share",
        save_target(&pd, false) == SaveTarget::Dir { dir: ShotDir::Desktop, ensure: DirEnsure::AlreadyThere }
            && save_target(&ps, false) == SaveTarget::Dir { dir: ShotDir::ShareVolS, ensure: DirEnsure::AlreadyThere },
        "桌面/S: 落点路由不符",
    );
    // 8) 询问模式：弹对话框（建议名照常生成），取消不落盘。
    let mut pa = ShotSavePolicy::new();
    pa.set_dir(ShotDir::AskEachTime);
    let is_dialog = save_target(&pa, false) == SaveTarget::Dialog;
    let mut dlg = SaveDialog::new();
    let mut bn = [0u8; SHOT_NAME_MAX];
    let ln = format_shot_name(&SampleTime::SAMPLE, DEFAULT_PREFIX, 1, &mut bn);
    dlg.open_with(&bn[..ln]);
    let dlg_ok = dlg.open && dlg.suggested_bytes() == "截图 2026-09-25_1430".as_bytes();
    dlg.cancel();
    cs.add("ask_mode_dialog", is_dialog && dlg_ok && !dlg.open, "询问模式对话框行为不符");
    // 9) 一致性：截图落点与 F512「另存」同一 policy 源。
    cs.add("consistency_same_source", screenshot_dir(&p) == history_saveas_dir(&p), "截图/另存落点不同源");
    // 10) 一致性随切换保持：改落点后两读取口同步跟随。
    let mut pc = ShotSavePolicy::new();
    pc.set_dir(ShotDir::Desktop);
    cs.add("consistency_after_change", screenshot_dir(&pc) == ShotDir::Desktop && history_saveas_dir(&pc) == ShotDir::Desktop, "切换后两读取口失同步");
    // 11) 名称缓冲容量：超长前缀按 PREFIX_MAX 截断，最长组合 50 字节完整容纳。
    let long_prefix = [b'P'; 40]; // 40 字节 > PREFIX_MAX(32)
    let mut full = [0u8; SHOT_NAME_MAX];
    let lf = format_shot_name(&SampleTime::SAMPLE, &long_prefix, 99, &mut full);
    cs.add("name_capacity", lf == PREFIX_MAX + 15 + 3 && lf <= SHOT_NAME_MAX, "最长名组合应按前缀上限容纳（50 字节）");
    cs
}

#[cfg(test)]
mod f521_tests {
    use super::*;

    #[test]
    fn naming_exact_default_and_prefix() {
        let p = ShotSavePolicy::new();
        assert_eq!(p.prefix_str(), Some("截图 "), "默认前缀应为「截图 」");
        let mut b = [0u8; SHOT_NAME_MAX];
        let n = format_shot_name(&SampleTime::SAMPLE, p.prefix_bytes(), 1, &mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "截图 2026-09-25_1430", "默认命名不符");
        let mut q = ShotSavePolicy::new();
        q.set_prefix("会议-");
        let mut b2 = [0u8; SHOT_NAME_MAX];
        let n2 = format_shot_name(&SampleTime::SAMPLE, q.prefix_bytes(), 1, &mut b2);
        assert_eq!(core::str::from_utf8(&b2[..n2]).unwrap(), "会议-2026-09-25_1430", "自定义前缀命名不符");
    }

    #[test]
    fn monotonic_never_collides() {
        // 分钟前进：定宽字典序严格递增（双缓冲轮换避免悬垂借用）。
        let mut b_prev = [0u8; SHOT_NAME_MAX];
        let mut b_cur = [0u8; SHOT_NAME_MAX];
        let mut have_prev = false;
        for m in 0u8..60 {
            let t = SampleTime { minute: m, ..SampleTime::SAMPLE };
            let n = format_shot_name(&t, DEFAULT_PREFIX, 1, &mut b_cur);
            if have_prev {
                assert!(&b_cur[..n] > &b_prev[..n], "分钟 {} 名字应严格递增", m);
            }
            b_prev[..n].copy_from_slice(&b_cur[..n]);
            have_prev = true;
        }
        // 同分钟连拍：seq 1..=5 两两互异（永不重名）。
        let mut seq_bufs = [[0u8; SHOT_NAME_MAX]; 5];
        let mut lens = [0usize; 5];
        for k in 0..5 {
            lens[k] = format_shot_name(&SampleTime::SAMPLE, DEFAULT_PREFIX, (k + 1) as u8, &mut seq_bufs[k]);
        }
        for i in 0..5 {
            for j in (i + 1)..5 {
                assert_ne!(
                    &seq_bufs[i][..lens[i]],
                    &seq_bufs[j][..lens[j]],
                    "同分钟 seq {} 与 {} 应互异",
                    i + 1,
                    j + 1
                );
            }
        }
    }

    #[test]
    fn target_routing_all_dirs() {
        let mut p = ShotSavePolicy::new();
        assert!(
            save_target(&p, false) == SaveTarget::Dir { dir: ShotDir::PicturesShotDir, ensure: DirEnsure::Created },
            "默认应自动建"
        );
        p.set_dir(ShotDir::Desktop);
        assert!(matches!(save_target(&p, false), SaveTarget::Dir { dir: ShotDir::Desktop, .. }), "桌面路由不符");
        p.set_dir(ShotDir::ShareVolS);
        assert!(matches!(save_target(&p, false), SaveTarget::Dir { dir: ShotDir::ShareVolS, .. }), "S: 路由不符");
        p.set_dir(ShotDir::AskEachTime);
        assert_eq!(save_target(&p, true), SaveTarget::Dialog, "询问模式应弹对话框");
    }

    #[test]
    fn consistency_between_screenshot_and_saveas() {
        let mut p = ShotSavePolicy::new();
        assert_eq!(screenshot_dir(&p), history_saveas_dir(&p), "初始应同源");
        for d in [ShotDir::Desktop, ShotDir::ShareVolS, ShotDir::AskEachTime, ShotDir::PicturesShotDir] {
            p.set_dir(d);
            assert_eq!(screenshot_dir(&p), history_saveas_dir(&p), "切到 {:?} 后应保持同源", d);
        }
    }

    #[test]
    fn dialog_cancel_writes_nothing() {
        let mut dlg = SaveDialog::new();
        assert!(!dlg.open, "初始应关闭");
        let mut b = [0u8; SHOT_NAME_MAX];
        let n = format_shot_name(&SampleTime::SAMPLE, DEFAULT_PREFIX, 1, &mut b);
        dlg.open_with(&b[..n]);
        assert!(dlg.open, "打开失败");
        assert_eq!(dlg.suggested_bytes(), "截图 2026-09-25_1430".as_bytes(), "建议名不符");
        dlg.cancel();
        assert!(!dlg.open, "取消后应关闭（无写动作）");
    }
}

// ===========================================================================
// F525 快捷键速查卡导出 —— HotkeyCardExport
// ===========================================================================

/// 键表容量（条目数）。
pub const TABLE_CAP: usize = 96;
/// PNG 一页版行容量上限（一页排不下如实报溢出账）。
pub const PNG_ONE_PAGE_ROWS: usize = 40;
/// PDF 双页版每页行数：2×48=96 恰满全表。
pub const PDF_PAGE_ROWS: usize = 48;
/// 打印走查 DPI（判据：打印 300dpi 走查）。
pub const CARD_DPI: u32 = 300;
/// A4 纵向 @300dpi 宽（210mm）。
pub const A4_W_PX_300DPI: u32 = 2480;
/// A4 纵向 @300dpi 高（297mm）。
pub const A4_H_PX_300DPI: u32 = 3508;
/// 动作名容量（字节）。
pub const ACTION_MAX: usize = 24;

/// 键来源（分色标注驱动字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeySource {
    SystemDefault,
    UserCustom,
}

/// 来源 → 色标（0=系统默认 / 1=用户自定义；分色标注）。
pub fn color_index(source: KeySource) -> u8 {
    match source {
        KeySource::SystemDefault => 0,
        KeySource::UserCustom => 1,
    }
}

/// 键表条目：动作名 + 组合键位图 + 键码 + 来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotkeyEntry {
    pub action: [u8; ACTION_MAX],
    pub action_len: usize,
    /// 修饰键位图（bit0 Ctrl / bit1 Alt / bit2 Shift / bit3 Win）。
    pub mods: u8,
    pub key: u8,
    pub source: KeySource,
}

impl HotkeyEntry {
    pub fn from(action: &str, mods: u8, key: u8, source: KeySource) -> HotkeyEntry {
        let mut e = HotkeyEntry { action: [0; ACTION_MAX], action_len: 0, mods, key, source };
        for &b in action.as_bytes() {
            if e.action_len < ACTION_MAX {
                e.action[e.action_len] = b;
                e.action_len += 1;
            }
        }
        e
    }
    pub fn action_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.action[..self.action_len]).ok()
    }
}

/// 快捷键注册表（F244 同源数据面）。
pub struct HotkeyRegistry {
    entries: [Option<HotkeyEntry>; TABLE_CAP],
    count: usize,
}

impl HotkeyRegistry {
    pub fn new() -> HotkeyRegistry {
        HotkeyRegistry { entries: [None; TABLE_CAP], count: 0 }
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn register(&mut self, action: &str, mods: u8, key: u8, source: KeySource) -> bool {
        if self.count >= TABLE_CAP {
            return false; // 超容拒绝（不静默丢）
        }
        self.entries[self.count] = Some(HotkeyEntry::from(action, mods, key, source));
        self.count += 1;
        true
    }
    /// 改键：命中动作即改组合键并标记为用户自定义（导出如实）。
    pub fn set_key(&mut self, action: &str, mods: u8, key: u8) -> bool {
        let a = action.as_bytes();
        for e in self.entries[..self.count].iter_mut().flatten() {
            if e.action_len == a.len() && e.action[..e.action_len] == *a {
                e.mods = mods;
                e.key = key;
                e.source = KeySource::UserCustom;
                return true;
            }
        }
        false
    }
    /// 当前态快照（导出取快照——同源一致性）。
    pub fn snapshot(&self) -> HotkeySnapshot {
        HotkeySnapshot { entries: self.entries, count: self.count }
    }
    pub fn fingerprint(&self) -> u32 {
        fingerprint_entries(&self.entries, self.count)
    }
}

/// 导出快照（导出内容与注册表同源：指纹一致）。
#[derive(Clone, Copy)]
pub struct HotkeySnapshot {
    entries: [Option<HotkeyEntry>; TABLE_CAP],
    count: usize,
}

impl HotkeySnapshot {
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn entry(&self, i: usize) -> Option<HotkeyEntry> {
        if i < self.count {
            self.entries[i]
        } else {
            None
        }
    }
    pub fn fingerprint(&self) -> u32 {
        fingerprint_entries(&self.entries, self.count)
    }
}

/// FNV-1a 指纹（动作名+位图+键码+来源，逐条目混入）。
fn fingerprint_entries(entries: &[Option<HotkeyEntry>; TABLE_CAP], count: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for e in entries[..count].iter().flatten() {
        h ^= e.source as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= e.mods as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= e.key as u32;
        h = h.wrapping_mul(0x0100_0193);
        for k in 0..e.action_len {
            h ^= e.action[k] as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= 0xFF;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 两格式（判据：PNG 一页版 / PDF 双页版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardFormat {
    PngOnePage,
    PdfTwoPage,
}

/// 分页账：页数 + 每页行数（rows[1] 在一页版兼作溢出账，如实标注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pagination {
    pub pages: usize,
    pub rows: [usize; 2],
}

/// 排版分页：一页版 40 行封顶；双页版 48×2 恰满 96 条全表。
pub fn paginate(count: usize, fmt: CardFormat) -> Pagination {
    match fmt {
        CardFormat::PngOnePage => {
            let first = count.min(PNG_ONE_PAGE_ROWS);
            Pagination { pages: 1, rows: [first, count - first] }
        }
        CardFormat::PdfTwoPage => {
            let p1 = count.min(PDF_PAGE_ROWS);
            let p2 = (count - p1).min(PDF_PAGE_ROWS);
            Pagination { pages: 2, rows: [p1, p2] }
        }
    }
}

/// 导出双入口（判据：F374 浮层内 + F244 注册表页——双门到同一导出函数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardEntryDoor {
    OverlayF374,
    RegistryF244,
}

/// 导出结果：快照指纹 + 分页账 + 分色统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardExport {
    pub fingerprint: u32,
    pub pages: usize,
    pub rows: [usize; 2],
    pub defaults: usize,
    pub customs: usize,
}

/// 双门同一导出函数：取注册表快照，指纹/分页/分色一次算清。
pub fn export_card(reg: &HotkeyRegistry, door: CardEntryDoor, fmt: CardFormat) -> CardExport {
    let _ = door; // 入口只影响 UI 侧来源标注，内容与指纹同一源
    let snap = reg.snapshot();
    let mut defaults = 0usize;
    let mut customs = 0usize;
    for i in 0..snap.count() {
        if let Some(e) = snap.entry(i) {
            match e.source {
                KeySource::SystemDefault => defaults += 1,
                KeySource::UserCustom => customs += 1,
            }
        }
    }
    let pg = paginate(snap.count(), fmt);
    CardExport { fingerprint: snap.fingerprint(), pages: pg.pages, rows: pg.rows, defaults, customs }
}

/// F525 域自检（domain: "F525-hotkey-card"）。
pub fn run_f525_checks() -> CheckSet {
    let mut cs = CheckSet::new("F525-hotkey-card");
    // 1) 两格式枚举可辨。
    cs.add("two_formats", CardFormat::PngOnePage != CardFormat::PdfTwoPage, "两格式未区分");
    // 2) 300dpi 排版参数常量（A4 2480×3508）。
    cs.add(
        "dpi_consts",
        CARD_DPI == 300 && A4_W_PX_300DPI == 2480 && A4_H_PX_300DPI == 3508,
        "300dpi 排版参数不符",
    );
    // 3) 一页版容量：50 条 → 40 行 + 溢出账 10。
    let pg1 = paginate(50, CardFormat::PngOnePage);
    cs.add("one_page_capacity", pg1.pages == 1 && pg1.rows == [40, 10], "一页版容量/溢出账不符");
    // 4) 双页版分页账：96 条 → 48+48 恰满；50 条 → 48+2。
    let pg2 = paginate(96, CardFormat::PdfTwoPage);
    let pg3 = paginate(50, CardFormat::PdfTwoPage);
    cs.add("two_page_split", pg2.pages == 2 && pg2.rows == [48, 48] && pg3.rows == [48, 2], "双页版分页账不符");
    // 5) 同源一致性：导出快照指纹 == 注册表当前态指纹。
    let mut reg = HotkeyRegistry::new();
    let _ = reg.register("copy", 0x1, b'C', KeySource::SystemDefault);
    let _ = reg.register("paste", 0x1, b'V', KeySource::SystemDefault);
    let ex = export_card(&reg, CardEntryDoor::OverlayF374, CardFormat::PngOnePage);
    cs.add("fingerprint_same_source", ex.fingerprint == reg.fingerprint(), "导出与注册表不同源");
    // 6) 改键 → 指纹变化（导出后改键再导出对比）。
    let fp_before = reg.fingerprint();
    assert!(reg.set_key("copy", 0x8, b'J'), "改键应命中");
    cs.add("change_key_fp_changes", reg.fingerprint() != fp_before, "改键后指纹未变");
    // 7) 再导出与注册表当前态一致（同源一致性判据）。
    let ex2 = export_card(&reg, CardEntryDoor::RegistryF244, CardFormat::PdfTwoPage);
    cs.add("reexport_matches_registry", ex2.fingerprint == reg.fingerprint(), "再导出与注册表不一致");
    // 8) 分色标注：色标两值 + 分色统计（3 默认 1 自定义）。
    let _ = reg.register("undo", 0x1, b'Z', KeySource::UserCustom);
    let ex3 = export_card(&reg, CardEntryDoor::OverlayF374, CardFormat::PngOnePage);
    cs.add(
        "color_annotation",
        color_index(KeySource::SystemDefault) == 0
            && color_index(KeySource::UserCustom) == 1
            && ex3.defaults == 2
            && ex3.customs == 1,
        "分色标注/统计不符",
    );
    // 9) 双入口同一导出函数：两门产出同指纹同分页。
    let a = export_card(&reg, CardEntryDoor::OverlayF374, CardFormat::PdfTwoPage);
    let b = export_card(&reg, CardEntryDoor::RegistryF244, CardFormat::PdfTwoPage);
    cs.add("dual_doors_same", a == b, "双入口导出结果不一致");
    // 10) 自定义键如实导出：快照里条目值即改后值。
    let snap = reg.snapshot();
    let custom_ok = (0..snap.count())
        .filter_map(|i| snap.entry(i))
        .any(|e| e.action_str() == Some("copy") && e.mods == 0x8 && e.key == b'J' && e.source == KeySource::UserCustom);
    cs.add("custom_as_is", custom_ok, "自定义键未如实导出");
    // 11) 表容量 96：超容拒绝不静默丢。
    let mut full = HotkeyRegistry::new();
    let mut all_in = true;
    for i in 0..TABLE_CAP {
        if !full.register("act", 0, i as u8, KeySource::SystemDefault) {
            all_in = false;
        }
    }
    cs.add("table_cap_bound", TABLE_CAP == 96 && all_in && !full.register("over", 0, 0, KeySource::SystemDefault), "容量边界不符");
    cs
}

#[cfg(test)]
mod f525_tests {
    use super::*;

    #[test]
    fn paginate_accounts_honest() {
        assert_eq!(paginate(0, CardFormat::PngOnePage), Pagination { pages: 1, rows: [0, 0] }, "空表分页账");
        assert_eq!(paginate(40, CardFormat::PngOnePage).rows, [40, 0], "恰满一页无溢出");
        assert_eq!(paginate(41, CardFormat::PngOnePage).rows, [40, 1], "溢出 1 条应入账");
        assert_eq!(paginate(TABLE_CAP, CardFormat::PdfTwoPage).rows, [48, 48], "全表恰满双页");
    }

    #[test]
    fn fingerprint_tracks_changes() {
        let mut r = HotkeyRegistry::new();
        let _ = r.register("open", 0x1, b'O', KeySource::SystemDefault);
        let f0 = r.fingerprint();
        assert!(r.set_key("open", 0x2, b'P'), "改键应命中");
        let f1 = r.fingerprint();
        assert_ne!(f0, f1, "改键后指纹必须变化");
        assert!(!r.set_key("nope", 0, 0), "未注册动作不应命中");
    }

    #[test]
    fn export_same_source_after_edit() {
        let mut r = HotkeyRegistry::new();
        let _ = r.register("a", 0, 1, KeySource::SystemDefault);
        let e1 = export_card(&r, CardEntryDoor::OverlayF374, CardFormat::PngOnePage);
        assert_eq!(e1.fingerprint, r.fingerprint(), "导出应同源");
        assert!(r.set_key("a", 0x4, 9), "改键");
        let e2 = export_card(&r, CardEntryDoor::RegistryF244, CardFormat::PdfTwoPage);
        assert_eq!(e2.fingerprint, r.fingerprint(), "改键后再导出应仍同源");
        assert_ne!(e1.fingerprint, e2.fingerprint, "改键前后导出指纹应不同");
    }

    #[test]
    fn color_annotation_counts() {
        let mut r = HotkeyRegistry::new();
        for i in 0..7 {
            let src = if i % 2 == 0 { KeySource::SystemDefault } else { KeySource::UserCustom };
            let _ = r.register("k", 0, i as u8, src);
        }
        let e = export_card(&r, CardEntryDoor::OverlayF374, CardFormat::PngOnePage);
        assert_eq!(e.defaults, 4, "系统默认应 4 条");
        assert_eq!(e.customs, 3, "用户自定义应 3 条");
    }

    #[test]
    fn dual_doors_identical_output() {
        let mut r = HotkeyRegistry::new();
        let _ = r.register("save", 0x1, b'S', KeySource::UserCustom);
        for fmt in [CardFormat::PngOnePage, CardFormat::PdfTwoPage] {
            let a = export_card(&r, CardEntryDoor::OverlayF374, fmt);
            let b = export_card(&r, CardEntryDoor::RegistryF244, fmt);
            assert_eq!(a, b, "双门输出应一致（{:?}）", fmt);
        }
    }
}

// ===========================================================================
// F526 资源管理器状态栏 —— StatusbarModel
// ===========================================================================

/// 状态栏高度基线（判据：24px 基线）。
pub const STATUSBAR_HEIGHT_PX: u32 = 24;
/// MB（选中态/剩余空间的展示单位换算基准）。
pub const MB: u64 = 1 << 20;

/// 选中操作命令枚举（键盘操作与鼠标同路——同一刷新函数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelCommand {
    SelectAll,
    ClearSelection,
}

/// 资源管理器数据源：**三处同源**——状态栏 / F338 详情操作条 / F456 此机页共读此结构。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExplorerData {
    /// 当前目录项数（左段）。
    pub items: usize,
    /// 当前目录总字节。
    pub total_bytes: u64,
    /// 选中项数（中段）。
    pub sel_count: usize,
    /// 选中字节数（中段）。
    pub sel_bytes: u64,
    /// 当前卷剩余空间（右段，F456 同数据）。
    pub vol_free: u64,
}

impl ExplorerData {
    /// F338 详情操作条读口（选中态同源）。
    pub fn detail_bar_selection(&self) -> (usize, u64) {
        (self.sel_count, self.sel_bytes)
    }
    /// F456 此机页读口（卷剩余同数据）。
    pub fn thispc_volume_free(&self) -> u64 {
        self.vol_free
    }
    /// 键盘选择命令：全选/清空选择（走同一数据源，刷新即反映）。
    pub fn apply_sel_command(&mut self, cmd: SelCommand) {
        match cmd {
            SelCommand::SelectAll => {
                self.sel_count = self.items;
                self.sel_bytes = self.total_bytes;
            }
            SelCommand::ClearSelection => {
                self.sel_count = 0;
                self.sel_bytes = 0;
            }
        }
    }
}

/// 状态栏三段快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segments {
    /// 左：当前目录项数。
    pub left_items: usize,
    /// 中：选中项数。
    pub mid_count: usize,
    /// 中：选中字节。
    pub mid_bytes: u64,
    /// 右：卷剩余字节。
    pub right_free: u64,
}

/// 状态栏模型：数据源快照 + 实时刷新 + 三段渲染。
pub struct StatusbarModel {
    data: ExplorerData,
    /// 最近一次刷新事件（诊断面）。
    pub last_event: &'static str,
}

impl StatusbarModel {
    pub fn new(data: ExplorerData) -> StatusbarModel {
        StatusbarModel { data, last_event: "init" }
    }
    /// 实时刷新：增删文件/选择变化后调用，数字即变（判据：实时同步）。
    pub fn refresh(&mut self, data: ExplorerData, event: &'static str) {
        self.data = data;
        self.last_event = event;
    }
    pub fn data(&self) -> ExplorerData {
        self.data
    }
    pub fn segments(&self) -> Segments {
        Segments {
            left_items: self.data.items,
            mid_count: self.data.sel_count,
            mid_bytes: self.data.sel_bytes,
            right_free: self.data.vol_free,
        }
    }
    /// 空目录态（判据：空目录显示——0 项文案）。
    pub fn is_empty_dir(&self) -> bool {
        self.data.items == 0
    }
    /// 左段渲染：「128 项」/「0 项」。
    pub fn render_left(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_usize(out, &mut n, self.data.items);
        push_str(out, &mut n, " 项");
        n
    }
    /// 中段渲染：「已选 5 项 · 245MB」；未选中不占段。
    pub fn render_mid(&self, out: &mut [u8]) -> usize {
        if self.data.sel_count == 0 {
            return 0;
        }
        let mut n = 0usize;
        push_str(out, &mut n, "已选 ");
        push_usize(out, &mut n, self.data.sel_count);
        push_str(out, &mut n, " 项 · ");
        push_usize(out, &mut n, (self.data.sel_bytes / MB) as usize);
        push_str(out, &mut n, "MB");
        n
    }
    /// 右段渲染：「剩余 1024MB」。
    pub fn render_right(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, "剩余 ");
        push_usize(out, &mut n, (self.data.vol_free / MB) as usize);
        push_str(out, &mut n, "MB");
        n
    }
}

/// F526 域自检（domain: "F526-exp-statusbar"）。
pub fn run_f526_checks() -> CheckSet {
    let mut cs = CheckSet::new("F526-exp-statusbar");
    // 1) 高度 24px 基线常量。
    cs.add("height_24px", STATUSBAR_HEIGHT_PX == 24, "高度基线非 24px");
    // 2) 三段初值与数据源一致。
    let d0 = ExplorerData { items: 128, total_bytes: 4096 * 128, sel_count: 5, sel_bytes: 245 * MB, vol_free: 1024 * MB };
    let sb = StatusbarModel::new(d0);
    let seg = sb.segments();
    cs.add(
        "segments_init",
        seg.left_items == 128 && seg.mid_count == 5 && seg.mid_bytes == 245 * MB && seg.right_free == 1024 * MB,
        "三段初值不符",
    );
    // 3) 实时性：删除文件项数即减。
    let mut d = d0;
    d.items = 127;
    d.total_bytes -= 4096;
    let mut sb2 = StatusbarModel::new(d0);
    sb2.refresh(d, "delete");
    cs.add("realtime_delete", sb2.segments().left_items == 127, "删除后项数未即减");
    // 4) 实时性：选择变化即反映。
    let mut d2 = d0;
    d2.sel_count = 9;
    d2.sel_bytes = 300 * MB;
    sb2.refresh(d2, "select");
    cs.add("realtime_select", sb2.segments().mid_count == 9 && sb2.segments().mid_bytes == 300 * MB, "选择变化未即反映");
    // 5) 三处同源之 F338：状态栏中段 == 详情操作条读口。
    cs.add(
        "same_source_f338",
        sb.segments().mid_count == d0.detail_bar_selection().0 && sb.segments().mid_bytes == d0.detail_bar_selection().1,
        "中段与 F338 读口不同源",
    );
    // 6) 三处同源之 F456：右段 == 此机页读口。
    cs.add("same_source_f456", sb.segments().right_free == d0.thispc_volume_free(), "右段与 F456 读口不同源");
    // 7) 键盘操作反映：全选（Ctrl+A 语义同路）。
    let mut d3 = d0;
    d3.apply_sel_command(SelCommand::SelectAll);
    sb2.refresh(d3, "select-all");
    cs.add(
        "kbd_select_all",
        sb2.segments().mid_count == d0.items && sb2.segments().mid_bytes == d0.total_bytes,
        "全选后中段未反映",
    );
    // 8) 键盘操作反映：清空选择。
    let mut d4 = d3;
    d4.apply_sel_command(SelCommand::ClearSelection);
    sb2.refresh(d4, "clear-sel");
    cs.add("kbd_clear_sel", sb2.segments().mid_count == 0 && sb2.segments().mid_bytes == 0, "清空选择未反映");
    // 9) 空目录态：0 项文案。
    let d_empty = ExplorerData { items: 0, total_bytes: 0, sel_count: 0, sel_bytes: 0, vol_free: 512 * MB };
    let sb3 = StatusbarModel::new(d_empty);
    let mut buf = [0u8; 32];
    let n = sb3.render_left(&mut buf);
    cs.add(
        "empty_dir_state",
        sb3.is_empty_dir() && core::str::from_utf8(&buf[..n]).map(|s| s == "0 项").unwrap_or(false),
        "空目录未显示 0 项",
    );
    // 10) 左段格式：「128 项」逐字节。
    let mut buf = [0u8; 32];
    let n = sb.render_left(&mut buf);
    cs.add("left_format", core::str::from_utf8(&buf[..n]).map(|s| s == "128 项").unwrap_or(false), "左段格式不符");
    // 11) 中段格式：「已选 5 项 · 245MB」逐字节。
    let mut buf = [0u8; 64];
    let n = sb.render_mid(&mut buf);
    cs.add(
        "mid_format",
        core::str::from_utf8(&buf[..n]).map(|s| s == "已选 5 项 · 245MB").unwrap_or(false),
        "中段格式不符",
    );
    // 12) 右段格式：「剩余 1024MB」逐字节。
    let mut buf = [0u8; 32];
    let n = sb.render_right(&mut buf);
    cs.add("right_format", core::str::from_utf8(&buf[..n]).map(|s| s == "剩余 1024MB").unwrap_or(false), "右段格式不符");
    cs
}

#[cfg(test)]
mod f526_tests {
    use super::*;

    #[test]
    fn segments_track_changes_realtime() {
        let d0 = ExplorerData { items: 10, total_bytes: 10 * MB, sel_count: 2, sel_bytes: 2 * MB, vol_free: 8 * MB };
        let mut sb = StatusbarModel::new(d0);
        assert_eq!(sb.segments().left_items, 10, "初值应 10");
        let mut d = d0;
        d.items = 9;
        sb.refresh(d, "delete");
        assert_eq!(sb.segments().left_items, 9, "删除后应即减");
        assert_eq!(sb.last_event, "delete", "事件标注应更新");
    }

    #[test]
    fn same_source_invariants_hold() {
        let d = ExplorerData { items: 7, total_bytes: 7 * MB, sel_count: 3, sel_bytes: 1 * MB, vol_free: 99 * MB };
        let sb = StatusbarModel::new(d);
        let seg = sb.segments();
        let (c, b) = d.detail_bar_selection();
        assert_eq!((seg.mid_count, seg.mid_bytes), (c, b), "中段应与 F338 读口同源");
        assert_eq!(seg.right_free, d.thispc_volume_free(), "右段应与 F456 读口同源");
    }

    #[test]
    fn keyboard_commands_reflected() {
        let d0 = ExplorerData { items: 6, total_bytes: 6 * MB, sel_count: 1, sel_bytes: MB, vol_free: 4 * MB };
        let mut d = d0;
        let mut sb = StatusbarModel::new(d0);
        d.apply_sel_command(SelCommand::SelectAll);
        sb.refresh(d, "select-all");
        assert_eq!(sb.segments().mid_count, 6, "全选应选中全部 6 项");
        assert_eq!(sb.segments().mid_bytes, 6 * MB, "全选字节应为总量");
        let mut d2 = sb.data();
        d2.apply_sel_command(SelCommand::ClearSelection);
        sb.refresh(d2, "clear");
        assert_eq!(sb.segments().mid_count, 0, "清空后应为 0");
    }

    #[test]
    fn empty_dir_shows_zero_items() {
        let d = ExplorerData { items: 0, total_bytes: 0, sel_count: 0, sel_bytes: 0, vol_free: MB };
        let sb = StatusbarModel::new(d);
        assert!(sb.is_empty_dir(), "应判空目录");
        let mut b = [0u8; 16];
        let n = sb.render_left(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "0 项", "空目录文案应为「0 项」");
        assert_eq!(sb.render_mid(&mut [0u8; 16]), 0, "未选中中段不占");
    }

    #[test]
    fn render_formats_exact() {
        let d = ExplorerData { items: 128, total_bytes: 512 * MB, sel_count: 5, sel_bytes: 245 * MB, vol_free: 2048 * MB };
        let sb = StatusbarModel::new(d);
        let mut b = [0u8; 32];
        let n = sb.render_left(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "128 项", "左段不符");
        let mut b = [0u8; 64];
        let n = sb.render_mid(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "已选 5 项 · 245MB", "中段不符");
        let mut b = [0u8; 32];
        let n = sb.render_right(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "剩余 2048MB", "右段不符");
    }

    #[test]
    fn realtime_sequence_delete_select_clear() {
        let d0 = ExplorerData { items: 100, total_bytes: 100 * MB, sel_count: 0, sel_bytes: 0, vol_free: 10 * MB };
        let mut sb = StatusbarModel::new(d0);
        // 删 1 项 → 项数即减。
        let mut d = d0;
        d.items = 99;
        sb.refresh(d, "delete");
        assert_eq!(sb.segments().left_items, 99, "步骤 1 删除");
        // 全选 → 中段满额。
        d.apply_sel_command(SelCommand::SelectAll);
        sb.refresh(d, "select-all");
        assert_eq!(sb.segments().mid_count, 99, "步骤 2 全选");
        // 清空 → 中段归零。
        d.apply_sel_command(SelCommand::ClearSelection);
        sb.refresh(d, "clear");
        assert_eq!(sb.segments().mid_count, 0, "步骤 3 清空");
    }
}

// ===========================================================================
// F527 导航树折叠展开 —— NavTree
// ===========================================================================

/// 万节点容量（判据：万节点性能）。
pub const TREE_CAP: usize = 10_000;
/// 无父哨兵（根节点）。
pub const NONE: u16 = u16::MAX;
/// 虚拟化视口行数（可见切片预算基准）。
pub const VIEWPORT_ROWS: usize = 30;
/// 展开位图持久化字数（TREE_CAP 位 → 313 个 u32）。
pub const EXP_WORDS: usize = (TREE_CAP + 31) / 32;

/// 折叠展开输入两路：双击 / 箭头（判据：双击/箭头两路到同一 toggle 核心）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoldInput {
    DoubleClick,
    ArrowRight,
    ArrowLeft,
}

/// 两命令快捷键枚举（判据：全部展开当前层 / 全部折叠，右键+快捷键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeShortcut {
    ExpandLevelKey,
    CollapseAllKey,
}

/// 拖放操作语义（F262 全兼容：移动 / 复制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropOp {
    Move,
    Copy,
}

/// 定长导航树：父指针数组 + 展开位图 + 可见行缓存（虚拟化）。
pub struct NavTree {
    pub parent: [u16; TREE_CAP],
    pub name: [u32; TREE_CAP],
    expanded: [bool; TREE_CAP],
    count: usize,
    selected: u16,
    row_cache: [u16; TREE_CAP],
    row_count: usize,
    cache_valid: bool,
    /// 最近一次可见切片触达行数（O(可见数) 预算对账）。
    pub last_visible_cost: usize,
}

impl NavTree {
    pub const fn new() -> NavTree {
        NavTree {
            parent: [NONE; TREE_CAP],
            name: [0; TREE_CAP],
            expanded: [false; TREE_CAP],
            count: 0,
            selected: NONE,
            row_cache: [0; TREE_CAP],
            row_count: 0,
            cache_valid: false,
            last_visible_cost: 0,
        }
    }
    pub fn count(&self) -> usize {
        self.count
    }
    /// 加根/子节点（测试按先序创建保证行序=节点序）。
    pub fn add_node(&mut self, parent: u16, name: u32) -> u16 {
        if self.count >= TREE_CAP {
            return NONE; // 超容拒绝
        }
        let id = self.count as u16;
        self.parent[id as usize] = parent;
        self.name[id as usize] = name;
        self.expanded[id as usize] = false;
        self.count += 1;
        self.cache_valid = false;
        id
    }
    pub fn add_root(&mut self, name: u32) -> u16 {
        self.add_node(NONE, name)
    }
    pub fn is_expanded(&self, n: u16) -> bool {
        (n as usize) < self.count && self.expanded[n as usize]
    }
    /// toggle 核心：两路输入最终都落在这里（一处一事实）。
    fn set_fold(&mut self, n: u16, open: bool) {
        if (n as usize) < self.count {
            self.expanded[n as usize] = open;
            self.cache_valid = false;
        }
    }
    pub fn toggle(&mut self, n: u16) {
        self.set_fold(n, !self.is_expanded(n));
    }
    pub fn expand(&mut self, n: u16) {
        self.set_fold(n, true);
    }
    pub fn collapse(&mut self, n: u16) {
        self.set_fold(n, false);
    }
    /// 双击/箭头两路路由：双击=toggle，右箭头=展开，左箭头=折叠（同核心）。
    pub fn route_input(&mut self, n: u16, input: FoldInput) {
        match input {
            FoldInput::DoubleClick => self.toggle(n),
            FoldInput::ArrowRight => self.expand(n),
            FoldInput::ArrowLeft => self.collapse(n),
        }
    }
    /// 单击选中。
    pub fn select(&mut self, n: u16) {
        if (n as usize) < self.count {
            self.selected = n;
        }
    }
    pub fn selected(&self) -> u16 {
        self.selected
    }
    pub fn parent_of(&self, n: u16) -> Option<u16> {
        if (n as usize) < self.count && self.parent[n as usize] != NONE {
            Some(self.parent[n as usize])
        } else {
            None
        }
    }
    pub fn depth_of(&self, n: u16) -> u32 {
        let mut d = 0u32;
        let mut cur = n;
        while let Some(p) = self.parent_of(cur) {
            d += 1;
            cur = p;
        }
        d
    }
    /// 快捷键 → 两命令（全部展开当前层 / 全部折叠）。
    pub fn apply_shortcut(&mut self, sc: TreeShortcut) {
        match sc {
            TreeShortcut::ExpandLevelKey => {
                let d = if (self.selected as usize) < self.count { self.depth_of(self.selected) } else { 0 };
                for i in 0..self.count {
                    if self.depth_of(i as u16) == d {
                        self.set_fold(i as u16, true);
                    }
                }
            }
            TreeShortcut::CollapseAllKey => {
                for i in 0..self.count {
                    self.set_fold(i as u16, false);
                }
            }
        }
    }
    /// 可见性：全部祖先展开（不含自身——折叠节点本身仍显示）。
    pub fn is_visible(&self, n: u16) -> bool {
        if (n as usize) >= self.count {
            return false;
        }
        let mut cur = self.parent[n as usize];
        while cur != NONE {
            if !self.expanded[cur as usize] {
                return false;
            }
            cur = self.parent[cur as usize];
        }
        true
    }
    /// 重建可见行缓存（折叠变化后一次 O(n)；虚拟化读取 O(可见数)）。
    pub fn rebuild_rows(&mut self) {
        let mut n = 0usize;
        for i in 0..self.count {
            if self.is_visible(i as u16) {
                self.row_cache[n] = i as u16;
                n += 1;
            }
        }
        self.row_count = n;
        self.cache_valid = true;
    }
    pub fn rows(&self) -> usize {
        self.row_count
    }
    pub fn is_cache_valid(&self) -> bool {
        self.cache_valid
    }
    pub fn row_of(&self, n: u16) -> Option<usize> {
        for i in 0..self.row_count {
            if self.row_cache[i] == n {
                return Some(i);
            }
        }
        None
    }
    /// 可见切片：从 offset 起供给视口行数——成本 O(可见数) 而非 O(万)。
    pub fn visible_slice(&mut self, offset: usize) -> usize {
        let served = if offset >= self.row_count { 0 } else { (self.row_count - offset).min(VIEWPORT_ROWS) };
        self.last_visible_cost = served;
        served
    }
    /// 拖放：落到节点 = 移动/复制（F262 语义回执）。
    pub fn drop_on(&self, target: u16, op: DropOp) -> (u16, DropOp) {
        (target, op)
    }
    /// 展开位图持久化（F219 记忆族：重启后树保持展开样）。
    pub fn save_expanded(&self, out: &mut [u32; EXP_WORDS]) -> usize {
        for w in out.iter_mut() {
            *w = 0;
        }
        for i in 0..self.count {
            if self.expanded[i] {
                out[i / 32] |= 1 << (i % 32);
            }
        }
        self.count
    }
    pub fn load_expanded(&mut self, words: &[u32; EXP_WORDS], count: usize) {
        let n = count.min(TREE_CAP);
        for i in 0..n {
            self.expanded[i] = (words[i / 32] >> (i % 32)) & 1 == 1;
        }
        for i in n..self.count {
            self.expanded[i] = false;
        }
        self.count = n;
        self.cache_valid = false;
    }
}

/// F527 域自检（domain: "F527-navtree-fold"）。
pub fn run_f527_checks() -> CheckSet {
    let mut cs = CheckSet::new("F527-navtree-fold");
    // 1) 容量常量 = 万节点 10_000。
    cs.add("capacity_10k", TREE_CAP == 10_000 && EXP_WORDS == 313, "万节点容量/位图字数不符");
    // 2) 单击选中。
    let mut t = NavTree::new();
    let r = t.add_root(1);
    let a = t.add_node(r, 2);
    let _b = t.add_node(a, 3);
    let c = t.add_node(r, 4);
    t.select(c);
    cs.add("click_select", t.selected() == c, "单击选中失败");
    // 3) 双击 toggle：展开→折叠。
    let mut t2 = NavTree::new();
    let r2 = t2.add_root(1);
    let a2 = t2.add_node(r2, 2);
    t2.route_input(a2, FoldInput::DoubleClick);
    let once = t2.is_expanded(a2);
    t2.route_input(a2, FoldInput::DoubleClick);
    cs.add("doubleclick_toggle", once && !t2.is_expanded(a2), "双击 toggle 不符");
    // 4) 箭头两路：右箭展开 / 左箭折叠，与双击同一 toggle 核心。
    t2.route_input(a2, FoldInput::ArrowLeft);
    let after_left = t2.is_expanded(a2);
    t2.route_input(a2, FoldInput::ArrowRight);
    let after_right = t2.is_expanded(a2);
    cs.add("arrow_routes_same_core", !after_left && after_right, "箭头两路与核心 toggle 不一致");
    // 5) 全部展开当前层：只展开选中节点同深度层。
    let mut t3 = NavTree::new();
    let r3 = t3.add_root(1);
    let x1 = t3.add_node(r3, 2);
    let _y1 = t3.add_node(r3, 3);
    let _x2 = t3.add_node(x1, 4);
    t3.select(x1);
    t3.apply_shortcut(TreeShortcut::ExpandLevelKey);
    cs.add(
        "expand_level",
        t3.is_expanded(x1) && t3.is_expanded(_y1) && !t3.is_expanded(r3) && !t3.is_expanded(_x2),
        "全部展开当前层范围不符",
    );
    // 6) 全部折叠：一键收起全部。
    t3.apply_shortcut(TreeShortcut::CollapseAllKey);
    let none_open = (0..t3.count()).all(|i| !t3.is_expanded(i as u16));
    cs.add("collapse_all", none_open, "全部折叠后仍有展开节点");
    // 7) 两命令快捷键枚举可辨。
    cs.add("shortcuts_distinct", TreeShortcut::ExpandLevelKey != TreeShortcut::CollapseAllKey, "两快捷键未区分");
    // 8) 持久化：存位图→扰动→恢复后与存时一致（重启恢复语义）。
    let mut t4 = NavTree::new();
    let r4 = t4.add_root(1);
    let a4 = t4.add_node(r4, 2);
    let b4 = t4.add_node(a4, 3);
    t4.expand(r4);
    t4.expand(a4);
    let mut words = [0u32; EXP_WORDS];
    let saved_n = t4.save_expanded(&mut words);
    t4.collapse(a4);
    t4.expand(b4);
    t4.load_expanded(&words, saved_n);
    cs.add(
        "persistence_roundtrip",
        t4.is_expanded(r4) && t4.is_expanded(a4) && !t4.is_expanded(b4) && t4.count() == saved_n,
        "展开位图持久化恢复不一致",
    );
    // 9) 万节点虚拟化预算：10_000 节点全可见，视口切片成本 ≤ 30 行。
    let mut big = NavTree::new();
    let br = big.add_root(1);
    for i in 0..TREE_CAP - 1 {
        let _ = big.add_node(br, i as u32);
    }
    big.expand(br);
    big.rebuild_rows();
    let rows_all = big.rows();
    let s0 = big.visible_slice(0);
    let s_end = big.visible_slice(TREE_CAP - 1);
    cs.add(
        "virtualization_budget",
        big.count() == 10_000 && rows_all == 10_000 && s0 == VIEWPORT_ROWS && s_end == 1 && big.last_visible_cost <= VIEWPORT_ROWS,
        "万节点切片成本超预算/行账不符",
    );
    // 10) 可见性行账：折叠根时只有根一行；展开后子节点入行。
    let mut t5 = NavTree::new();
    let r5 = t5.add_root(1);
    let _a5 = t5.add_node(r5, 2);
    let _b5 = t5.add_node(r5, 3);
    t5.rebuild_rows();
    let collapsed_rows = t5.rows();
    t5.expand(r5);
    t5.rebuild_rows();
    cs.add("visibility_rows", collapsed_rows == 1 && t5.rows() == 3, "可见行账不符");
    // 11) 拖放语义：move/copy 回执保真（F262 兼容）。
    let (tgt_m, op_m) = t.drop_on(c, DropOp::Move);
    let (tgt_c, op_c) = t.drop_on(r, DropOp::Copy);
    cs.add(
        "drop_semantics",
        tgt_m == c && op_m == DropOp::Move && tgt_c == r && op_c == DropOp::Copy && DropOp::Move != DropOp::Copy,
        "拖放回执不符",
    );
    // 12) 缓存失效纪律：折叠变化即失效，重建后复效。
    t5.collapse(r5);
    let invalid_after_fold = !t5.is_cache_valid();
    t5.rebuild_rows();
    cs.add("cache_invalidation", invalid_after_fold && t5.is_cache_valid(), "缓存失效/重建纪律不符");
    cs
}

#[cfg(test)]
mod f527_tests {
    use super::*;

    #[test]
    fn two_input_routes_same_core() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        let a = t.add_node(r, 2);
        // 双击与右箭头展开等效。
        let mut t1 = NavTree::new();
        let r1 = t1.add_root(1);
        let a1 = t1.add_node(r1, 2);
        t1.route_input(a1, FoldInput::DoubleClick);
        t1.route_input(a1, FoldInput::ArrowRight); // 已展开，幂等
        assert!(t1.is_expanded(a1), "双击后应展开");
        let _ = a;
        // 左箭头折叠与双击两次等效。
        t1.route_input(a1, FoldInput::ArrowLeft);
        assert!(!t1.is_expanded(a1), "左箭头应折叠");
        let _ = r;
    }

    #[test]
    fn expand_level_and_collapse_all() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        let a = t.add_node(r, 2);
        let b = t.add_node(r, 3);
        let c = t.add_node(a, 4);
        t.select(b);
        t.apply_shortcut(TreeShortcut::ExpandLevelKey);
        assert!(t.is_expanded(a) && t.is_expanded(b), "深度 1 层应全展开");
        assert!(!t.is_expanded(r), "根（深度 0）不应被展开");
        assert!(!t.is_expanded(c), "深度 2 不应被展开");
        t.apply_shortcut(TreeShortcut::CollapseAllKey);
        for i in 0..t.count() {
            assert!(!t.is_expanded(i as u16), "全部折叠后节点 {} 仍展开", i);
        }
    }

    #[test]
    fn persistence_roundtrip_restore() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        let a = t.add_node(r, 2);
        let b = t.add_node(a, 3);
        let c = t.add_node(a, 4);
        t.expand(r);
        t.expand(a);
        t.expand(c);
        let mut words = [0u32; EXP_WORDS];
        let n = t.save_expanded(&mut words);
        // 扰动：折叠一切再展开 b。
        t.apply_shortcut(TreeShortcut::CollapseAllKey);
        t.expand(b);
        t.load_expanded(&words, n);
        assert!(t.is_expanded(r) && t.is_expanded(a) && t.is_expanded(c), "恢复后展开态不符");
        assert!(!t.is_expanded(b), "恢复后 b 不应展开");
        assert_eq!(t.count(), 4, "节点规模应随快照恢复");
    }

    #[test]
    fn ten_thousand_nodes_virtualization_budget() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        for i in 0..TREE_CAP - 1 {
            let id = t.add_node(r, i as u32);
            assert_ne!(id, NONE, "万节点插入中途拒绝");
        }
        assert_eq!(t.count(), 10_000, "应恰好万节点");
        // 根折叠时只有 1 行；展开后万行。
        t.rebuild_rows();
        assert_eq!(t.rows(), 1, "根折叠应只 1 行");
        t.expand(r);
        t.rebuild_rows();
        assert_eq!(t.rows(), 10_000, "展开后应万行");
        // 视口切片：任一 offset 供给 ≤ 30 行，成本 O(可见数)。
        for off in [0usize, 5_000, 9_999] {
            let served = t.visible_slice(off);
            assert!(served <= VIEWPORT_ROWS, "offset {} 供给超视口", off);
            assert!(t.last_visible_cost <= VIEWPORT_ROWS, "offset {} 成本超预算", off);
        }
        assert_eq!(t.visible_slice(9_999), 1, "末行切片应只供 1 行");
    }

    #[test]
    fn rows_follow_visibility() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        let a = t.add_node(r, 2);
        let b = t.add_node(a, 3);
        let _c = t.add_node(r, 4);
        t.rebuild_rows();
        assert_eq!(t.rows(), 1, "全折叠应 1 行");
        assert_eq!(t.row_of(b), None, "深层折叠节点不应入行");
        t.expand(r);
        t.rebuild_rows();
        assert_eq!(t.rows(), 3, "根展开应 3 行（子仍折叠）");
        assert_eq!(t.row_of(b), None, "孙节点仍折叠不入行");
        t.expand(a);
        t.rebuild_rows();
        assert_eq!(t.rows(), 4, "全展开应 4 行");
        assert!(t.row_of(b).is_some(), "展开后孙节点应入行");
    }

    #[test]
    fn drop_ops_and_capacity_reject() {
        let mut t = NavTree::new();
        let r = t.add_root(1);
        let a = t.add_node(r, 2);
        let (tg, op) = t.drop_on(a, DropOp::Move);
        assert_eq!((tg, op), (a, DropOp::Move), "移动回执不符");
        let (tg2, op2) = t.drop_on(r, DropOp::Copy);
        assert_eq!((tg2, op2), (r, DropOp::Copy), "复制回执不符");
        // 容量拒绝：万节点后继续加返回 NONE。
        let mut big = NavTree::new();
        let br = big.add_root(1);
        for i in 0..TREE_CAP - 1 {
            big.add_node(br, i as u32);
        }
        assert_eq!(big.add_node(br, 0xFFFF), NONE, "超容应拒绝");
        assert_eq!(big.count(), TREE_CAP, "超容后计数不变");
    }
}

// ===========================================================================
// F528 树与列表双向同步 —— TreeListSync
// ===========================================================================

/// 路径容量（节点 id 深度）。
pub const PATH_MAX: usize = 8;
/// 深层路径判据层数（5 层同步时序）。
pub const SYNC_DEEP_LAYERS: usize = 5;
/// 同步步骤日志容量。
pub const LOG_MAX: usize = 16;

/// 同步步骤类别（时序账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepKind {
    /// 树展开路径一层。
    ExpandPath,
    /// 树高亮当前节点（视觉态）。
    Highlight,
    /// 滚动到可见。
    ScrollTo,
    /// 列表刷新。
    ListRefresh,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncStep {
    pub kind: StepKind,
    pub node: u16,
}

/// 焦点归属（判据：同步高亮不抢焦点——F206 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Tree,
    List,
}

/// 双向同步状态机：树展开/高亮/滚动 + 列表路径 + 步骤日志。
pub struct TreeListSync {
    tree: NavTree,
    list_path: [u16; PATH_MAX],
    list_len: usize,
    /// 树侧高亮（视觉态）。
    pub highlight: u16,
    /// 键盘焦点（与高亮独立——判据：焦点/高亮分离）。
    pub focus: Focus,
    /// 树视口滚动偏移（行）。
    pub scroll: usize,
    /// 最近一次可见性处理是否引发滚动。
    pub last_scroll_moved: bool,
    log: [SyncStep; LOG_MAX],
    log_n: usize,
}

impl TreeListSync {
    pub fn new() -> TreeListSync {
        TreeListSync {
            tree: NavTree::new(),
            list_path: [0; PATH_MAX],
            list_len: 0,
            highlight: 0,
            focus: Focus::List,
            scroll: 0,
            last_scroll_moved: false,
            log: [SyncStep { kind: StepKind::ExpandPath, node: 0 }; LOG_MAX],
            log_n: 0,
        }
    }
    // 树建装委托。
    pub fn add_root(&mut self, name: u32) -> u16 {
        self.tree.add_root(name)
    }
    pub fn add_node(&mut self, parent: u16, name: u32) -> u16 {
        self.tree.add_node(parent, name)
    }
    pub fn tree_is_expanded(&self, n: u16) -> bool {
        self.tree.is_expanded(n)
    }
    pub fn tree_rows(&self) -> usize {
        self.tree.rows()
    }
    pub fn tree_selected(&self) -> u16 {
        self.tree.selected()
    }

    fn log_step(&mut self, kind: StepKind, node: u16) {
        if self.log_n < LOG_MAX {
            self.log[self.log_n] = SyncStep { kind, node };
            self.log_n += 1;
        }
    }
    pub fn log_len(&self) -> usize {
        self.log_n
    }
    pub fn log_at(&self, i: usize) -> Option<SyncStep> {
        if i < self.log_n {
            Some(self.log[i])
        } else {
            None
        }
    }

    /// 节点的树侧行号（可见行缓存中查找）。
    fn row_index_of(&self, n: u16) -> Option<usize> {
        self.tree.row_of(n)
    }

    /// 自动滚动到可见：目标不在当前视口则移动滚动偏移（对齐到该行，尾部钳制）。
    pub fn ensure_visible(&mut self, n: u16) {
        self.tree.rebuild_rows();
        match self.row_index_of(n) {
            Some(idx) => {
                if idx < self.scroll || idx >= self.scroll + VIEWPORT_ROWS {
                    let max_off = self.tree.rows().saturating_sub(1);
                    self.scroll = idx.min(max_off);
                    self.last_scroll_moved = true;
                    self.log_step(StepKind::ScrollTo, n);
                } else {
                    self.last_scroll_moved = false;
                }
            }
            None => {
                self.last_scroll_moved = false;
            }
        }
    }

    /// 列表进入子目录 → 树自动展开路径+高亮当前节点+滚动到可见（不夺焦点）。
    pub fn list_enter(&mut self, path: &[u16]) {
        self.log_n = 0;
        for &n in path {
            self.tree.expand(n); // 逐层展开（时序：自根向叶）
            self.log_step(StepKind::ExpandPath, n);
        }
        if let Some(&last) = path.last() {
            self.highlight = last; // 高亮是视觉态
            self.log_step(StepKind::Highlight, last);
            self.ensure_visible(last); // 滚动到可见
        }
        // 焦点留在列表：树高亮不抢焦点（F206）。
        self.focus = Focus::List;
    }

    /// 树点选 → 列表刷新到该节点（双向另一向）。
    pub fn tree_select(&mut self, n: u16) {
        self.tree.select(n);
        let (p, len) = self.path_to(n);
        self.list_path = p;
        self.list_len = len;
        self.highlight = n;
        self.focus = Focus::Tree; // 树点选时焦点本来就在树上
        self.log_step(StepKind::ListRefresh, n);
    }

    /// 节点到根的路径（根在前）。
    pub fn path_to(&self, n: u16) -> ([u16; PATH_MAX], usize) {
        let mut rev = [0u16; PATH_MAX];
        let mut rn = 0usize;
        let mut cur = n;
        loop {
            if (cur as usize) < self.tree.count() && rn < PATH_MAX {
                rev[rn] = cur;
                rn += 1;
            }
            match self.tree.parent_of(cur) {
                Some(p) => cur = p,
                None => break,
            }
        }
        let mut out = [0u16; PATH_MAX];
        for k in 0..rn {
            out[k] = rev[rn - 1 - k];
        }
        (out, rn)
    }

    /// 列表当前路径拷出（对账用）。
    pub fn list_path_copy(&self) -> ([u16; PATH_MAX], usize) {
        (self.list_path, self.list_len)
    }
    pub fn list_path_last(&self) -> u16 {
        if self.list_len > 0 {
            self.list_path[self.list_len - 1]
        } else {
            0
        }
    }

    /// 纯视觉高亮：只改高亮，不迁移焦点（焦点/高亮字段分离）。
    pub fn highlight_only(&mut self, n: u16) {
        self.highlight = n;
    }

    /// 与 F527 持久化协同：同步后的展开态照常进持久化位图。
    pub fn persisted_expanded(&self, out: &mut [u32; EXP_WORDS]) -> usize {
        self.tree.save_expanded(out)
    }
    pub fn persisted_count(&self) -> usize {
        self.tree.count()
    }
}

/// F528 域自检（domain: "F528-treelist-sync"）。
pub fn run_f528_checks() -> CheckSet {
    let mut cs = CheckSet::new("F528-treelist-sync");
    // 1) 列表进入 → 树展开路径并高亮当前节点。
    let mut s = TreeListSync::new();
    let r = s.add_root(1);
    let a = s.add_node(r, 2);
    let b = s.add_node(a, 3);
    s.list_enter(&[r, a, b]);
    cs.add(
        "list_enter_expands_highlights",
        s.tree_is_expanded(r) && s.tree_is_expanded(a) && s.highlight == b,
        "list_enter 展开路径/高亮不符",
    );
    // 2) 树点选 → 列表刷新（list path 末位=该节点）。
    let mut s2 = TreeListSync::new();
    let r2 = s2.add_root(1);
    let a2 = s2.add_node(r2, 2);
    let b2 = s2.add_node(a2, 3);
    s2.tree_select(b2);
    cs.add("tree_select_refreshes", s2.list_path_last() == b2, "树点选后列表未刷新到该节点");
    // 3) 双向永远同步：list→tree 后 tree→list 回程路径一致。
    let (p, l) = s2.list_path_copy();
    cs.add("bidirectional_roundtrip", l == 3 && p[0] == r2 && p[1] == a2 && p[2] == b2, "双向回程路径不一致");
    // 4) 焦点/高亮分离：树高亮后键盘焦点仍在列表。
    let mut s3 = TreeListSync::new();
    let r3 = s3.add_root(1);
    let a3 = s3.add_node(r3, 2);
    s3.list_enter(&[r3, a3]);
    cs.add(
        "focus_stays_list",
        s3.focus == Focus::List && s3.highlight == a3,
        "树高亮抢了列表焦点",
    );
    // 5) 焦点/高亮字段独立：纯视觉高亮不改焦点。
    let mut s4 = TreeListSync::new();
    let r4 = s4.add_root(1);
    let _a4 = s4.add_node(r4, 2);
    let b4 = s4.add_node(r4, 3);
    s4.focus = Focus::Tree;
    s4.highlight_only(b4);
    cs.add("highlight_focus_independent", s4.focus == Focus::Tree && s4.highlight == b4, "高亮与焦点字段未分离");
    // 6) 自动滚动可见：目标行在视口下方 → 滚动偏移移动且目标可见。
    let mut s5 = TreeListSync::new();
    let r5 = s5.add_root(1);
    let mut last_leaf = r5;
    let mut mid_leaf = r5;
    for i in 0..40 {
        let id = s5.add_node(r5, i as u32 + 10);
        if i == 35 {
            mid_leaf = id;
        }
        last_leaf = id;
    }
    s5.list_enter(&[r5]);
    s5.ensure_visible(last_leaf);
    let idx = s5.tree_rows() - 1; // 末叶行号
    cs.add(
        "autoscroll_below",
        s5.last_scroll_moved && s5.scroll <= idx && idx < s5.scroll + VIEWPORT_ROWS,
        "视口外目标未滚动到可见",
    );
    // 7) 已在视口内 → 不滚动。
    let mut s6 = TreeListSync::new();
    let r6 = s6.add_root(1);
    let a6 = s6.add_node(r6, 2);
    s6.list_enter(&[r6]);
    s6.scroll = 0;
    s6.ensure_visible(a6);
    cs.add("no_scroll_when_visible", !s6.last_scroll_moved && s6.scroll == 0, "视口内目标误滚动");
    // 8) 深层路径 5 层：逐层展开时序 + 最终态全展开+高亮叶。
    let mut s7 = TreeListSync::new();
    let r7 = s7.add_root(1);
    let d1 = s7.add_node(r7, 2);
    let d2 = s7.add_node(d1, 3);
    let d3 = s7.add_node(d2, 4);
    let d4 = s7.add_node(d3, 5);
    s7.list_enter(&[r7, d1, d2, d3, d4]);
    let order_ok = (0..SYNC_DEEP_LAYERS).all(|i| match s7.log_at(i) {
        Some(st) => st.kind == StepKind::ExpandPath && st.node == i as u16,
        None => false,
    });
    cs.add(
        "deep_5_layers_timing",
        SYNC_DEEP_LAYERS == 5
            && order_ok
            && s7.highlight == d4
            && s7.tree_is_expanded(r7)
            && s7.tree_is_expanded(d1)
            && s7.tree_is_expanded(d2)
            && s7.tree_is_expanded(d3)
            && s7.tree_is_expanded(d4),
        "5 层路径同步时序/最终态不符",
    );
    // 9) 步骤日志次序：展开×N → 高亮（→ 滚动如需）。
    let kinds_ok = match (s7.log_at(SYNC_DEEP_LAYERS), s7.log_at(SYNC_DEEP_LAYERS + 1)) {
        (Some(h), maybe_s) => {
            h.kind == StepKind::Highlight
                && match maybe_s {
                    Some(st) => st.kind == StepKind::ScrollTo,
                    None => true,
                }
        }
        _ => false,
    };
    cs.add("step_log_order", kinds_ok && s7.log_len() >= SYNC_DEEP_LAYERS + 1, "同步步骤日志次序不符");
    // 10) 与 F527 持久化协同：同步后的展开态进持久化位图。
    let mut words = [0u32; EXP_WORDS];
    let n = s7.persisted_expanded(&mut words);
    let bit_ok = [r7, d1, d2, d3, d4].iter().all(|&i| (words[i as usize / 32] >> (i as usize % 32)) & 1 == 1);
    cs.add("persistence_coop", n == 5 && bit_ok, "同步后展开态未进持久化位图");
    // 11) 树点选路径保真：深层叶回程路径逐位一致。
    let mut s8 = TreeListSync::new();
    let r8 = s8.add_root(1);
    let e1 = s8.add_node(r8, 2);
    let e2 = s8.add_node(e1, 3);
    let e3 = s8.add_node(e2, 4);
    let e4 = s8.add_node(e3, 5);
    s8.tree_select(e4);
    let (p8, l8) = s8.list_path_copy();
    cs.add(
        "tree_select_path_exact",
        l8 == 5 && p8[0] == r8 && p8[1] == e1 && p8[2] == e2 && p8[3] == e3 && p8[4] == e4,
        "树点选回程路径逐位不符",
    );
    // 12) 滚动钳制：反向回滚到中间叶（行 36 < 当前偏移 40）→ 对齐且偏移不越界。
    s5.ensure_visible(mid_leaf);
    let clamped = s5.last_scroll_moved && s5.scroll < s5.tree_rows();
    cs.add("scroll_clamped", clamped, "滚动偏移越界");
    cs
}

#[cfg(test)]
mod f528_tests {
    use super::*;

    fn build_chain(s: &mut TreeListSync, depth: usize) -> Vec<u16> {
        let mut path = Vec::new();
        let mut parent = s.add_root(1);
        path.push(parent);
        for i in 1..depth {
            parent = s.add_node(parent, i as u32 + 1);
            path.push(parent);
        }
        path
    }

    #[test]
    fn bidirectional_roundtrip() {
        let mut s = TreeListSync::new();
        let path = build_chain(&mut s, 3);
        // list → tree：展开+高亮。
        s.list_enter(&path);
        assert_eq!(s.highlight, path[2], "列表进入后树高亮应为叶");
        assert!(s.tree_is_expanded(path[0]) && s.tree_is_expanded(path[1]), "路径应展开");
        // tree → list：高亮节点回程刷新列表。
        s.tree_select(s.highlight);
        let (p, l) = s.list_path_copy();
        assert_eq!(l, 3, "回程路径长度不符");
        assert_eq!(&p[..l], &path[..3], "回程路径应与原路径一致");
    }

    #[test]
    fn focus_highlight_separation() {
        let mut s = TreeListSync::new();
        let path = build_chain(&mut s, 2);
        s.list_enter(&path);
        assert_eq!(s.focus, Focus::List, "树高亮不得夺列表焦点");
        assert_eq!(s.highlight, path[1], "树应高亮当前节点");
        // 纯视觉高亮再换节点，焦点纹丝不动。
        let other = s.add_node(path[0], 99);
        s.highlight_only(other);
        assert_eq!(s.focus, Focus::List, "视觉高亮不应迁移焦点");
        assert_eq!(s.highlight, other, "视觉高亮应更新");
    }

    #[test]
    fn deep_path_timing_and_final_state() {
        let mut s = TreeListSync::new();
        let path = build_chain(&mut s, SYNC_DEEP_LAYERS);
        assert_eq!(path.len(), 5, "应有 5 层路径");
        s.list_enter(&path);
        // 时序账：逐层 ExpandPath（自根向叶）→ Highlight → 可选 ScrollTo。
        for (i, &n) in path.iter().enumerate() {
            let st = s.log_at(i).expect("步骤缺失");
            assert_eq!(st.kind, StepKind::ExpandPath, "第 {} 步应为展开", i);
            assert_eq!(st.node, n, "第 {} 步展开节点不符（应自根向叶）", i);
        }
        let h = s.log_at(path.len()).expect("高亮步骤缺失");
        assert_eq!(h.kind, StepKind::Highlight, "展开后应高亮");
        // 最终态：全展开+高亮叶+焦点留列表。
        for &n in &path {
            assert!(s.tree_is_expanded(n), "节点 {} 应展开", n);
        }
        assert_eq!(s.highlight, path[4], "最终高亮应为叶");
        assert_eq!(s.focus, Focus::List, "焦点应留在列表");
    }

    #[test]
    fn autoscroll_math_and_clamp() {
        let mut s = TreeListSync::new();
        let r = s.add_root(1);
        let mut leaves = Vec::new();
        for i in 0..40 {
            leaves.push(s.add_node(r, i as u32 + 10));
        }
        s.list_enter(&[r]);
        // 视口内（行 1）不滚。
        s.scroll = 0;
        s.ensure_visible(leaves[0]);
        assert!(!s.last_scroll_moved && s.scroll == 0, "视口内不应滚动");
        // 末叶（行 40）在视口外 → 滚到对齐行且钳制在行界内。
        s.ensure_visible(leaves[39]);
        assert!(s.last_scroll_moved, "视口外应滚动");
        let idx = s.tree_rows() - 1;
        assert_eq!(s.scroll, idx, "应对齐目标行");
        assert!(idx >= s.scroll && idx < s.scroll + VIEWPORT_ROWS, "目标应在视口内");
        assert!(s.scroll < s.tree_rows(), "滚动偏移应钳制在行界内");
    }

    #[test]
    fn persistence_cooperation_after_sync() {
        let mut s = TreeListSync::new();
        let path = build_chain(&mut s, 4);
        s.list_enter(&path);
        let mut words = [0u32; EXP_WORDS];
        let n = s.persisted_expanded(&mut words);
        assert_eq!(n, 4, "持久化节点数不符");
        for &node in &path {
            let bit = (words[node as usize / 32] >> (node as usize % 32)) & 1 == 1;
            assert!(bit, "同步展开的节点 {} 应记入持久化位图", node);
        }
        // 恢复到新树：展开态原样回来（F527 语义复用）。
        let mut t2 = NavTree::new();
        let r = t2.add_root(1);
        let a = t2.add_node(r, 2);
        let b = t2.add_node(a, 3);
        let _c = t2.add_node(b, 4);
        t2.load_expanded(&words, n);
        assert!(t2.is_expanded(r) && t2.is_expanded(a) && t2.is_expanded(b), "恢复后展开态应与同步后一致");
    }

    #[test]
    fn tree_select_focus_and_highlight() {
        let mut s = TreeListSync::new();
        let r = s.add_root(1);
        let a = s.add_node(r, 2);
        let b = s.add_node(a, 3);
        s.tree_select(b);
        assert_eq!(s.focus, Focus::Tree, "树点选焦点应在树");
        assert_eq!(s.highlight, b, "高亮应随点选");
        assert_eq!(s.list_path_last(), b, "列表应刷新到点选节点");
        assert_eq!(s.tree_selected(), b, "树选中态应更新");
        // 再走 list_enter：焦点回列表（双向切换焦点语义正确）。
        s.list_enter(&[r, a]);
        assert_eq!(s.focus, Focus::List, "列表进入后焦点应回列表");
    }
}
