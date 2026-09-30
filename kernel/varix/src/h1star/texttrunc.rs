//! F247 长文本截断规范 · 判据实装。
//!
//! **判据锚**：主册 F247「长文本截断规范」。
//!
//! **验收标准第一句（任务包原文）**：三式适用场景清单审计。
//!
//! **判据（主册原文摘录）**：截断三式全系统统一：单行截断用省略号（…）
//! 且悬停 Tooltip 显示全文（F205 联动）、多行截断用「展开」链接（点击
//! 原位展开，收起入口保留）、路径截断保尾不保头（C:\very\long\path\file.txt
//! 显示成 …\path\file.txt——文件名永远完整可见，这是和 Windows 中段
//! 省略的关键差异，理由：用户认的是文件名不是前缀）。
//!
//! **设计要点**：
//! - 三式枚举 [`TruncMode`] 与内容种类一一对应，「适用场景清单」即
//!   [`mode_for`] 映射本身——一处一事实，加第四式必须改这里；
//! - 路径保尾：从文件名整段出发向左逐段装填，宽度预算耗尽即止。
//!   文件名完整是**结构性保证**（先取文件名再谈预算），不是事后修补；
//!   与 Windows 中段省略（保头掐尾）的关键差异落在这里；
//! - 全部字节域 UTF-8 安全：只在字符边界与路径分隔符处截断，续字节
//!   （0b10xxxxxx）永不成为切点。宽度走查口径：ASCII 6px / 宽字符 12px；
//! - Tooltip 联动是**结构性**的：截断发生即产出「需要 Tooltip 全文」
//!   信号并登记全文（定容 32 条，满则淘汰最旧），F205 消费方只读；
//! - 展开/收起状态机记录锚点几何：原位不变式=锚点/宽度不因展开改变
//!   （只有高度向下生长），跳变整数域为 0px，满足判据「<1px 位移」。
//!
//! **依赖锚点**：F205（Tooltip 全文消费方）、F151（字号令牌——行高
//! 由调用方注入，本模块不持字体表）；零堆热路径：截断/装填全部在
//! 调用方给定的定长缓冲内完成，堆只出现在 Tooltip 登记表的定容环里
//! （实际上本实现连堆都不用——纯定长数组）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 省略号「…」的 UTF-8 编码（U+2026，3 字节）——主册「单行截断用省略号（…）」。
pub const ELLIPSIS: [u8; 3] = [0xE2, 0x80, 0xA6];

/// 省略号显示宽度（px，走查口径：与宽字符同档）。
pub const ELLIPSIS_PX: u32 = 12;

/// ASCII 字符走查宽度（px）。
pub const ASCII_PX: u32 = 6;

/// 宽字符（CJK 等）走查宽度（px）。
pub const WIDE_PX: u32 = 12;

/// 原位展开跳变容忍（px）——主册「展开/收起原位无跳动（<1px 位移）」；
/// 整数像素域里 <1px 即 0px。
pub const EXPAND_JUMP_TOL_PX: u32 = 1;

/// Tooltip 登记表容量——F205 消费面，满则淘汰最旧（定容纪律）。
pub const TOOLTIP_CAP: usize = 32;

/// Tooltip 全文缓存字节数/条（超长全文按 48 字节截断登记——登记目的是
/// 提示消费方「有全文可取」，不是替代数据源）。
pub const TOOLTIP_TEXT_CAP: usize = 96;

/// 主册验收走查用例数——「路径保尾规则用例（20 条长路径）」。
pub const PATH_CASE_N: usize = 20;

/// 单条路径用例缓冲上限（用例集内最长路径 96 字节，留余量）。
const PATH_BUF: usize = 128;

// ---------------------------------------------------------------------------
// 字节域 UTF-8 安全工具
// ---------------------------------------------------------------------------

/// `s[i]` 起首字符的字节长度（畸形序列按 1 字节消费——不 panic、不静默卡死）。
fn char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// 片段显示宽度（px）。续字节不计宽（宽度记在首字节上）。
pub fn slice_width_px(s: &[u8]) -> u32 {
    let mut w = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let lead = s[i];
        if lead < 0x80 {
            w += ASCII_PX;
            i += 1;
        } else if lead & 0xC0 == 0x80 {
            i += 1; // 畸形续字节：不炸、不计宽
        } else {
            w += WIDE_PX;
            i += char_len(lead).min(s.len() - i);
        }
    }
    w
}

/// `b` 是否为 UTF-8 续字节（切点合法性判定用）。
pub fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

// ---------------------------------------------------------------------------
// 第一式：单行省略号
// ---------------------------------------------------------------------------

/// 内容种类（三式适用场景清单的键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    /// 单行文本（列表行、状态栏、按钮）。
    SingleLine,
    /// 多行正文（卡片摘要、说明段）。
    MultiLine,
    /// 文件系统路径。
    Path,
}

/// 截断三式——主册「截断三式全系统统一」的唯一取值点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncMode {
    /// 单行省略号 + Tooltip 全文（F205 联动）。
    SingleLineEllipsis,
    /// 多行「展开」链接（原位展开，收起入口保留）。
    MultiLineExpand,
    /// 路径保尾不保头（文件名永远完整）。
    PathTailKeep,
}

impl TruncMode {
    /// 本式是否结构联动 Tooltip（只有单行省略式）。
    pub fn tooltip_linked(&self) -> bool {
        matches!(self, TruncMode::SingleLineEllipsis)
    }

    /// 本式是否带展开/收起状态机（只有多行展开式）。
    pub fn expandable(&self) -> bool {
        matches!(self, TruncMode::MultiLineExpand)
    }

    /// 本式是否结构性保尾（只有路径式）。
    pub fn tail_keep(&self) -> bool {
        matches!(self, TruncMode::PathTailKeep)
    }
}

/// 三式适用场景清单：内容种类 → 截断式（一处一事实）。
pub fn mode_for(kind: ContentKind) -> TruncMode {
    match kind {
        ContentKind::SingleLine => TruncMode::SingleLineEllipsis,
        ContentKind::MultiLine => TruncMode::MultiLineExpand,
        ContentKind::Path => TruncMode::PathTailKeep,
    }
}

/// 单行截断结果：保留前缀字节数与是否发生截断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCut {
    /// 保留前缀字节数（UTF-8 字符边界对齐）。
    pub keep_bytes: usize,
    /// true = 发生截断，渲染面应追加省略号并登记 Tooltip 全文。
    pub truncated: bool,
}

/// 单行省略式截断：装得下原样返回；装不下给省略号留宽度后逐字符装填。
pub fn single_line(text: &[u8], budget_px: u32) -> LineCut {
    if slice_width_px(text) <= budget_px {
        return LineCut { keep_bytes: text.len(), truncated: false };
    }
    let limit = budget_px.saturating_sub(ELLIPSIS_PX);
    let mut w = 0u32;
    let mut keep = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        let l = char_len(text[i]).min(text.len() - i);
        let cw = if text[i] < 0x80 { ASCII_PX } else { WIDE_PX };
        if w + cw > limit {
            break;
        }
        w += cw;
        keep = i + l;
        i += l;
    }
    LineCut { keep_bytes: keep, truncated: true }
}

// ---------------------------------------------------------------------------
// 第三式：路径保尾（文件名永远完整的结构性保证）
// ---------------------------------------------------------------------------

/// `path` 中最后一个分隔符之后的下标（文件名起点；无分隔符为 0）。
fn name_start(path: &[u8]) -> usize {
    let mut sep = None;
    for (i, &b) in path.iter().enumerate() {
        if b == b'\\' || b == b'/' {
            sep = Some(i);
        }
    }
    match sep {
        Some(i) => i + 1,
        None => 0,
    }
}

/// `path[..seg_start-1]` 内最后一个分隔符的下一位（上一段起点；无则 0）。
/// `seg_start` 必须已由 [`name_start`] 或本函数产出。
fn prev_seg_start(path: &[u8], seg_start: usize) -> usize {
    if seg_start == 0 {
        return 0;
    }
    // seg_start-1 是紧邻当前段的分隔符；在它之前再找一个分隔符。
    let mut i = seg_start - 1;
    while i > 0 {
        i -= 1;
        if path[i] == b'\\' || path[i] == b'/' {
            return i + 1;
        }
    }
    0
}

/// 保留段起点为 `keep_start` 时的展示宽度（keep_start=0 表示整条路径）。
fn display_width(path: &[u8], keep_start: usize) -> u32 {
    if keep_start == 0 {
        slice_width_px(path)
    } else {
        ELLIPSIS_PX + slice_width_px(&path[keep_start - 1..])
    }
}

/// 拷贝 `src` 到 `dst`，截断安全；返回写入字节数。
fn blit(dst: &mut [u8], src: &[u8]) -> usize {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

/// 路径保尾截断：输出 `…\path\file.txt` 式结果到 `out`，返回写入字节数。
///
/// 算法：文件名整段先取（结构性保证），随后向左逐段装填直到宽度预算
/// 耗尽；只要发生丢段就前置省略号 + 紧邻保留段的分隔符。文件名本身
/// 超预算也完整输出——判据「文件名永远完整可见」优先于宽度预算。
pub fn path_tail(path: &[u8], budget_px: u32, out: &mut [u8]) -> usize {
    let ns = name_start(path);
    let mut keep_start = ns;
    if keep_start > 0 {
        loop {
            let cand = prev_seg_start(path, keep_start);
            // 修障登记（单测 path_tail_reference_and_extremes 红）：
            // cand == 0 是「没有上一段、候选即整条路径」，不是「装不下」
            // ——旧实现先 break 后判宽，整条路径明明装得下也丢头加省略号
            // （budget=1000 时 "C:\very\…" 被截成 "…\very\…"）。改为照常
            // 过宽度门：装得下取全路径并终止，装不下维持现保留段终止。
            if display_width(path, cand) <= budget_px {
                keep_start = cand; // 接受更长的保留段
                if cand == 0 {
                    break; // 全路径已装下
                }
            } else {
                // 宽度随保留前缀变长单调递增：装不进预算就到头了，
                // 显性终止（v2 批修复：修复前不前进也不终止，预算小于
                // 中段宽度时在此永久空转——F247 自检悬挂实锤）。
                break;
            }
        }
    }
    if keep_start == 0 {
        blit(out, path)
    } else {
        let mut n = blit(out, &ELLIPSIS);
        n += blit(&mut out[n..], &path[keep_start - 1..]);
        n
    }
}

/// 主册 20 条长路径用例集（覆盖盘符/多级目录/混合分隔符/宽字符文件名）。
pub const PATH_CASES: [&str; PATH_CASE_N] = [
    "C:\\very\\long\\path\\file.txt",
    "C:\\Users\\varix\\Documents\\Projects\\varix-os\\kernel\\main.rs",
    "D:\\工作\\设计稿\\2026 季度\\首页改版\\banner\\final\\hero.png",
    "C:\\Program Files\\Varix OS\\runtime\\plugins\\editor\\layout.dll",
    "E:\\backups\\2026-09\\weekly\\snapshot-0926\\system\\registry.hiv",
    "/usr/local/varix/libexec/compositor/varix-comp",
    "/home/varia/文档/论文/审稿意见/第二轮/reviewer-b/comments.md",
    "C:\\Windows\\System32\\driverstore\\filerepository\\display.inf",
    "D:\\media\\movies\\2026\\纪录片\\城市脉动\\city.pulse.2160p.mkv",
    "C:\\Users\\varia\\AppData\\Local\\Varix\\cache\\thumbs\\a1b2c3.png",
    "Z:\\net\\mount\\team-share\\规范\\H 基础通用域\\判据实装\\readme.md",
    "C:\\very\\deep\\a\\b\\c\\d\\e\\f\\g\\h\\i\\j\\leaf.bin",
    "D:\\tools\\sdk\\ndk\\25.2.9579313\\toolchains\\llvm\\prebuilt\\bin\\clang.exe",
    "F:\\photo\\2026 春\\西湖\\DSC_0001.JPG",
    "C:\\varix\\logs\\crash\\2026-09-26\\143255\\kernel\\panic.dmp",
    "E:\\vm\\varix-dev\\snapshots\\clean-boot\\disk0.vhd",
    "D:\\音乐\\无损\\巴赫\\哥德堡变奏曲\\1981\\variation-01.flac",
    "C:\\Users\\varia\\Desktop\\未命名文件夹\\新建文本文档.txt",
    "M:\\mount-point-with-a-really-long-name\\sub\\data.csv",
    "/var/log/varix/input-probe/2026/09/26/session-0007.log",
];

/// 20 条长路径用例审计：任意预算下输出必须以完整文件名收尾。
///
/// 预算 120px 走查（判据口径）+ 极小预算 10px（结构性保证压力例）。
pub fn path_case_audit(budget_px: u32) -> bool {
    for case in PATH_CASES.iter() {
        let p = case.as_bytes();
        let fname = &p[name_start(p)..];
        if fname.is_empty() {
            return false;
        }
        let mut buf = [0u8; PATH_BUF];
        let n = path_tail(p, budget_px, &mut buf);
        if n < fname.len() || &buf[n - fname.len()..n] != fname {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F205 Tooltip 联动（结构性：截断即登记全文）
// ---------------------------------------------------------------------------

/// Tooltip 登记槽（定长，零堆）。
#[derive(Clone, Copy)]
struct TipSlot {
    key: u32,
    used: bool,
    len: usize,
    text: [u8; TOOLTIP_TEXT_CAP],
}

/// Tooltip 全文登记表：截断发生即登记，满则淘汰最旧（环形）。
pub struct TooltipReg {
    slots: [TipSlot; TOOLTIP_CAP],
    head: usize,
    /// 因容量淘汰被挤出的登记次数（诊断面如实呈现）。
    pub evicted: u32,
}

impl TooltipReg {
    pub fn new() -> TooltipReg {
        TooltipReg {
            slots: [const {
                TipSlot { key: 0, used: false, len: 0, text: [0u8; TOOLTIP_TEXT_CAP] }
            }; TOOLTIP_CAP],
            head: 0,
            evicted: 0,
        }
    }

    /// 登记全文（已登记同 key 则原位更新）。
    pub fn register(&mut self, key: u32, full: &[u8]) -> bool {
        for slot in self.slots.iter_mut() {
            if slot.used && slot.key == key {
                slot.len = blit(&mut slot.text, full);
                return true;
            }
        }
        let slot = &mut self.slots[self.head];
        if slot.used {
            self.evicted += 1;
        }
        slot.key = key;
        slot.used = true;
        slot.len = blit(&mut slot.text, full);
        self.head = (self.head + 1) % TOOLTIP_CAP;
        true
    }

    /// 读回全文（F205 悬停面消费）。
    pub fn lookup(&self, key: u32) -> Option<&[u8]> {
        self.slots
            .iter()
            .find(|s| s.used && s.key == key)
            .map(|s| &s.text[..s.len])
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 结构性联动入口：单行截断发生 → 登记全文；未截断不登记。
    /// 返回是否登记（与 cut.truncated 严格一致——联动可审计）。
    pub fn link_from_cut(&mut self, key: u32, text: &[u8], cut: LineCut) -> bool {
        if cut.truncated {
            self.register(key, text)
        } else {
            false
        }
    }
}

impl Default for TooltipReg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 第二式：多行展开/收起（原位无跳动）
// ---------------------------------------------------------------------------

/// 多行展开状态机：锚点几何 + 展开/收起位。
///
/// 原位不变式：展开只向下生长高度，锚点 (x,y) 与宽度 w 由调用方在
/// 切换前后各传一次；任何偏移都被记为跳变——判据「<1px 位移」在
/// 整数像素域即 0px（正确实现恒 0， nonzero 即调用方布局缺陷）。
#[derive(Clone, Copy, Debug)]
pub struct ExpandState {
    pub expanded: bool,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub width: i32,
    pub last_jump_px: u32,
    toggle_count: u32,
}

impl ExpandState {
    pub fn new(x: i32, y: i32, w: i32) -> ExpandState {
        ExpandState { expanded: false, anchor_x: x, anchor_y: y, width: w, last_jump_px: 0, toggle_count: 0 }
    }

    /// 展开/收起切换。
    pub fn toggle(&mut self, x: i32, y: i32, w: i32) {
        let jump = (self.anchor_x - x).abs() + (self.anchor_y - y).abs();
        self.last_jump_px = jump.max(0) as u32;
        self.width = w;
        self.expanded = !self.expanded;
        self.toggle_count = self.toggle_count.wrapping_add(1);
    }

    /// 原位无跳动判定（<1px 容忍）。
    pub fn in_place_ok(&self) -> bool {
        self.last_jump_px < EXPAND_JUMP_TOL_PX
    }

    /// 「展开」链接可见（收起态）。
    pub fn expand_link_visible(&self) -> bool {
        !self.expanded
    }

    /// 「收起」入口可见（展开态）——判据「收起入口保留」。
    pub fn collapse_link_visible(&self) -> bool {
        self.expanded
    }

    pub fn toggles(&self) -> u32 {
        self.toggle_count
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F247 自检（判据：三式适用场景清单审计；≥1 条 xors32 fuzz）。
pub fn run_texttrunc_checks() -> CheckSet {
    let mut set = CheckSet::new("F247-texttrunc");

    // 1. 三式适用场景清单：内容种类 → 截断式映射逐一核对。
    set.add(
        "mode map single/multi/path",
        mode_for(ContentKind::SingleLine) == TruncMode::SingleLineEllipsis
            && mode_for(ContentKind::MultiLine) == TruncMode::MultiLineExpand
            && mode_for(ContentKind::Path) == TruncMode::PathTailKeep,
        "",
    );
    set.add(
        "mode capability flags",
        TruncMode::SingleLineEllipsis.tooltip_linked()
            && !TruncMode::SingleLineEllipsis.expandable()
            && TruncMode::MultiLineExpand.expandable()
            && TruncMode::PathTailKeep.tail_keep(),
        "",
    );

    // 2. 单行放得下不截断。
    let cut = single_line(b"short text", 200);
    set.add("single line fits untouched", !cut.truncated && cut.keep_bytes == 10, "");

    // 3. 单行超宽截断：保留前缀宽度 ≤ 预算-省略号宽。
    let long = b"the quick brown fox jumps over the lazy dog";
    let cut_long = single_line(long, 100);
    let fits =
        !cut_long.truncated || slice_width_px(&long[..cut_long.keep_bytes]) + ELLIPSIS_PX <= 100;
    set.add("single line cut within budget", cut_long.truncated && fits, "");

    // 4. UTF-8 边界：中文串截断点不落在续字节上。
    let zh = "中文内容超长需要截断的测试样例文本".as_bytes();
    let cut = single_line(zh, 60);
    let boundary_ok = cut.keep_bytes == zh.len() || !is_cont(zh[cut.keep_bytes]);
    set.add("utf8 boundary kept", boundary_ok, "");

    // 5. 路径保尾参考例：…\path\file.txt（主册原文样例）。
    let p = b"C:\\very\\long\\path\\file.txt";
    let mut buf = [0u8; PATH_BUF];
    let n = path_tail(p, 100, &mut buf);
    set.add(
        "path tail reference case",
        core::str::from_utf8(&buf[..n]) == Ok("…\\path\\file.txt"),
        "",
    );

    // 6. 20 条长路径用例：文件名永远完整（120px 与 10px 双预算）。
    set.add("path cases filename kept @120px", path_case_audit(120), "");
    set.add("path cases filename kept @10px", path_case_audit(10), "");

    // 7. Tooltip 结构性联动：截断才登记、可读回；未截断不登记。
    let mut tips = TooltipReg::new();
    let linked = tips.link_from_cut(7, long, cut_long);
    let unlinked = !tips.link_from_cut(8, b"short", single_line(b"short", 200));
    set.add(
        "tooltip linked iff truncated",
        linked && unlinked && tips.lookup(7) == Some(&long[..]),
        "",
    );

    // 8. Tooltip 环形淘汰：登记 33 条后容量恒 32 且最旧被挤出。
    let mut tips2 = TooltipReg::new();
    for k in 0..(TOOLTIP_CAP + 1) as u32 {
        tips2.register(k, b"full text");
    }
    set.add(
        "tooltip ring evicts oldest",
        tips2.len() == TOOLTIP_CAP && tips2.lookup(0).is_none() && tips2.evicted == 1,
        "",
    );

    // 9. 展开/收起原位无跳动 + 收起入口保留。
    let mut ex = ExpandState::new(40, 100, 320);
    ex.toggle(40, 100, 320); // 原位展开
    let expand_ok = ex.expanded && ex.in_place_ok() && ex.collapse_link_visible();
    ex.toggle(40, 100, 320); // 收起
    set.add(
        "expand/collapse in place, links kept",
        expand_ok && !ex.expanded && ex.expand_link_visible() && ex.toggles() == 2,
        "",
    );

    // 10. xors32 fuzz：随机分段路径保尾——不 panic、输出以完整文件名收尾。
    let mut x: u32 = 0x243F_6A88;
    let mut survived = true;
    let mut buf = [0u8; PATH_BUF];
    for _ in 0..1000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let segs = (x % 5 + 1) as usize;
        let mut path = Vec::new();
        for s in 0..segs {
            if s > 0 {
                path.push(b'\\');
            }
            let seglen = ((x >> 8) % 8 + 1) as usize;
            for _ in 0..seglen {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                path.push(b'a' + (x % 26) as u8);
            }
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let budget = (x % 200) as u32;
        let fname = &path[name_start(&path)..];
        let n = path_tail(&path, budget, &mut buf);
        if n < fname.len() || &buf[n - fname.len()..n] != fname {
            survived = false;
        }
    }
    set.add("path fuzz 1000 rounds filename kept", survived, "");

    // 11. xors32 fuzz：随机字节流单行截断——不 panic、截断时预算守恒。
    let mut survived2 = true;
    let mut raw = [0u8; 40];
    for _ in 0..1000u32 {
        for b in raw.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let budget = (x % 300) as u32;
        let c = single_line(&raw, budget);
        // 修障登记（check 11 红）：预算不变式的文档契约式是
        // 「前缀宽 ≤ 预算-省略号宽（饱和减法）」。旧断言把省略号宽
        // 移到等式另一侧（+ELLIPSIS ≤ budget），budget < ELLIPSIS_PX 时
        // 连省略号本身都放不下，断言物理不可满足——按契约式判定，
        // budget ≥ ELLIPSIS_PX 域与旧式逐点等价，小预算域判 keep=0。
        if c.truncated && slice_width_px(&raw[..c.keep_bytes]) > budget.saturating_sub(ELLIPSIS_PX) {
            survived2 = false;
        }
    }
    set.add("single-line fuzz 1000 rounds budget holds", survived2, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use alloc::vec::Vec;

    #[test]
    fn path_tail_reference_and_extremes() {
        // 主册参考例。
        let p = b"C:\\very\\long\\path\\file.txt";
        let mut buf = [0u8; 64];
        let n = path_tail(p, 100, &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("…\\path\\file.txt"));
        // 预算充裕：整条路径原样。
        let n2 = path_tail(p, 1000, &mut buf);
        assert_eq!(&buf[..n2], &p[..]);
        // 预算极小：文件名仍完整（"…" + "\file.txt" 共 12 字节）。
        let n3 = path_tail(p, 4, &mut buf);
        assert_eq!(n3, 12);
        assert_eq!(&buf[n3 - 8..n3], b"file.txt");
        // 裸文件名无分隔符：原样输出。
        let n4 = path_tail(b"readme.md", 10, &mut buf);
        assert_eq!(&buf[..n4], b"readme.md");
    }

    #[test]
    fn single_line_utf8_never_splits_char() {
        let text = "把一段中文长句放进窄容器里必然触发单行截断".as_bytes();
        for budget in [30u32, 48, 66, 90, 120] {
            let c = single_line(text, budget);
            if c.keep_bytes < text.len() {
                assert!(!is_cont(text[c.keep_bytes]), "切点落在续字节 budget={budget}");
            }
        }
    }

    #[test]
    fn tooltip_ring_wraparound() {
        let mut tips = TooltipReg::new();
        for k in 0..(TOOLTIP_CAP * 2) as u32 {
            tips.register(k, b"x");
        }
        assert_eq!(tips.len(), TOOLTIP_CAP);
        assert_eq!(tips.evicted, TOOLTIP_CAP as u32);
        assert!(tips.lookup(0).is_none());
        assert!(tips.lookup((TOOLTIP_CAP * 2 - 1) as u32).is_some());
        // 同 key 原位更新不占新槽。
        let before = tips.len();
        tips.register(TOOLTIP_CAP as u32 * 2 - 1, b"updated");
        assert_eq!(tips.len(), before);
    }

    #[test]
    fn expand_state_machine_invariants() {
        let mut ex = ExpandState::new(10, 20, 200);
        assert!(!ex.expanded && ex.expand_link_visible());
        ex.toggle(10, 20, 200);
        assert!(ex.expanded && ex.collapse_link_visible() && ex.in_place_ok());
        // 锚点被移动：跳变如实记录为缺陷信号。
        ex.toggle(12, 20, 200);
        assert!(!ex.in_place_ok() && ex.last_jump_px >= EXPAND_JUMP_TOL_PX);
    }

    #[test]
    fn path_cases_all_end_with_filename() {
        assert!(path_case_audit(120), "120px 走查预算下 20 条用例全过");
        assert!(path_case_audit(10), "10px 极端预算下文件名仍完整");
        for case in PATH_CASES.iter() {
            let p = case.as_bytes();
            let has_sep = p.iter().any(|&b| b == b'\\' || b == b'/');
            assert_eq!(name_start(p) > 0, has_sep);
        }
    }

    #[test]
    fn fuzz_path_tail_never_panics_and_keeps_name() {
        // 随机原始字节当路径（畸形序列也要活），文件名不变式只在
        // 存在分隔符时检查。
        let mut x: u32 = 0xDEAD_BEEF;
        let mut buf = [0u8; PATH_BUF];
        for _ in 0..3000u32 {
            let mut raw = Vec::new();
            let len = (x % 90) as usize;
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                raw.push((x >> 3) as u8);
                if raw.last() == Some(&b'\\') || raw.last() == Some(&b'/') {
                    continue;
                }
            }
            let n = path_tail(&raw, (x % 240) as u32, &mut buf);
            assert!(n <= buf.len());
            if let Some(pos) = raw.iter().rposition(|&b| b == b'\\' || b == b'/') {
                let fname = &raw[pos + 1..];
                if !fname.is_empty() {
                    assert_eq!(&buf[n - fname.len()..n], &fname[..]);
                }
            }
        }
    }

    #[test]
    fn texttrunc_selfcheck_all_green() {
        let set = run_texttrunc_checks();
        assert!(set.all_passed(), "F247 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F247（长文本截断规范）。v2 三件事：
// 1) 持久化 I/O：截断预算册（单行/路径两路预算 px）v2 定长容器序列化
//    ——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验和，四类损坏
//    显性拒绝；
// 2) UI 壳接线：列表行截断绘制清单（逐行 LineCut + Tooltip 结构性联动
//    登记 + 悬停 key 命中）——「悬停 Tooltip 显示全文」的行级承载；
// 3) 判定面扩展：run_texttrunc_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：单行预算 u32 + 路径预算 u32 + 保留 u32。
pub const VX2_TT_PAYLOAD: usize = 12;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_TT_BLOB: usize = 9 + VX2_TT_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 截断预算册（列表/面包屑等消费面的预算持久化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TruncBudgetBook {
    /// 单行省略式预算（px，含省略号）。
    pub single_budget_px: u32,
    /// 路径保尾式预算（px）。
    pub path_budget_px: u32,
}

impl TruncBudgetBook {
    pub const fn new() -> TruncBudgetBook {
        TruncBudgetBook { single_budget_px: 200, path_budget_px: 120 }
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_TT_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5..9].copy_from_slice(&self.single_budget_px.to_le_bytes());
        out[9..13].copy_from_slice(&self.path_budget_px.to_le_bytes());
        out[13..17].copy_from_slice(&0u32.to_le_bytes());
        let crc = vx2_fnv(&out[..9 + VX2_TT_PAYLOAD - 4]);
        out[9 + VX2_TT_PAYLOAD - 4..9 + VX2_TT_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        VX2_TT_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<TruncBudgetBook, Vx2Error> {
        if blob.len() != VX2_TT_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_TT_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        Ok(TruncBudgetBook {
            single_budget_px: u32::from_le_bytes([blob[5], blob[6], blob[7], blob[8]]),
            path_budget_px: u32::from_le_bytes([blob[9], blob[10], blob[11], blob[12]]),
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 列表行高（px）——v2 布局常量：F247 列表行 28px。
pub const VX2_ROW_H_PX: i32 = 28;
/// 列表行清单容量。
pub const VX2_ROWS_CAP: usize = 16;

/// 截断行绘制条目：行矩形 + 保留字节数 + 截断徽标（渲染面据此追加
/// 省略号；Tooltip 登记已由管线完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TruncRow {
    pub key: u32,
    pub y: i32,
    pub h: i32,
    pub keep_bytes: usize,
    pub truncated: bool,
}

/// 生成列表行截断绘制清单：逐行单行省略式截断 → 截断即登记 Tooltip
/// 全文（结构性联动，与 [`TooltipReg::link_from_cut`] 同门）。
pub fn trunc_rows(reg: &mut TooltipReg, texts: &[&[u8]], budget_px: u32, out: &mut [TruncRow]) -> usize {
    let m = texts.len().min(out.len()).min(VX2_ROWS_CAP);
    for k in 0..m {
        let cut = single_line(texts[k], budget_px);
        let _ = reg.link_from_cut(k as u32, texts[k], cut);
        out[k] = TruncRow {
            key: k as u32,
            y: k as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            keep_bytes: cut.keep_bytes,
            truncated: cut.truncated,
        };
    }
    m
}

/// 行命中测试 → 悬停 key（F205 悬停面据此 lookup 全文；界外 None）。
pub fn hover_key(rows: &[TruncRow], n: usize, px: i32, py: i32, w: i32) -> Option<u32> {
    (0..n.min(rows.len()))
        .find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
        .map(|k| rows[k].key)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F247 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_texttrunc_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F247-texttrunc-v2");

    // 1. 持久化 round-trip：预算册编→解→逐字段相等→新预算下行为同源。
    let book = TruncBudgetBook { single_budget_px: 150, path_budget_px: 90 };
    let mut buf = [0u8; VX2_TT_BLOB];
    let len = book.to_bytes(&mut buf);
    match TruncBudgetBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let long = b"the quick brown fox jumps over the lazy dog";
            let a = single_line(long, book.single_budget_px);
            let b = single_line(long, b2.single_budget_px);
            set.add(
                "v2 persistence round-trip",
                b2 == book && a == b && a.truncated,
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 5;
    let mut c = buf;
    c[10] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        TruncBudgetBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && TruncBudgetBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && TruncBudgetBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && TruncBudgetBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 行清单联动：长行截断且全文登记、短行原样不登记、悬停 key 命中
    //    （判据「悬停 Tooltip 显示全文」的行级承载）。
    let mut tips = TooltipReg::new();
    let texts: [&[u8]; 3] = [
        b"the quick brown fox jumps over the lazy dog",
        b"short",
        "把一段中文长句放进窄容器里必然触发单行截断".as_bytes(),
    ];
    let mut rows = [TruncRow { key: 0, y: 0, h: 0, keep_bytes: 0, truncated: false }; VX2_ROWS_CAP];
    let rn = trunc_rows(&mut tips, &texts, 100, &mut rows);
    let zh_keep_ok = rows[2].truncated
        && !is_cont(texts[2][rows[2].keep_bytes.min(texts[2].len()).min(texts[2].len() - 1)]);
    set.add(
        "v2 trunc rows & tooltip linkage",
        rn == 3
            && rows[0].truncated && rows[1].truncated == false
            && tips.lookup(0).is_some() && tips.lookup(1).is_none()
            && tips.lookup(2).is_some()
            && zh_keep_ok
            && hover_key(&rows, rn, 50, VX2_ROW_H_PX + 4, 400) == Some(1)
            && hover_key(&rows, rn, 50, -1, 400).is_none(),
        "",
    );

    // 4. 预算册驱动路径保尾：册内预算下 20 条用例文件名仍完整（预算
    //    持久化不改判据——文件名完整是结构性保证）。
    set.add("v2 budget book keeps path tails", path_case_audit(book.path_budget_px), "");

    // 5. xors32 fuzz 500 轮：随机预算册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和捕获。
    let mut x: u32 = 0x2477_C3E9;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let b = TruncBudgetBook {
            single_budget_px: x % 500,
            path_budget_px: (x >> 5) % 500,
        };
        let mut tbuf = [0u8; VX2_TT_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_TT_BLOB && TruncBudgetBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_TT_PAYLOAD] ^= 0x11;
        ok &= TruncBudgetBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_budget_book_roundtrip_and_reject() {
        let b = TruncBudgetBook { single_budget_px: 88, path_budget_px: 60 };
        let mut buf = [0u8; VX2_TT_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_TT_BLOB);
        assert_eq!(TruncBudgetBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[6] ^= 0x02;
        assert_eq!(TruncBudgetBook::from_bytes(&bad), Err(Vx2Error::BadChecksum));
        assert_eq!(TruncBudgetBook::from_bytes(&buf[..3]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_rows_layout_never_overlap() {
        let mut tips = TooltipReg::new();
        let texts: [&[u8]; 4] = [b"aaaa", b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", b"c", b"dddd"];
        let mut rows = [TruncRow { key: 0, y: 0, h: 0, keep_bytes: 0, truncated: false }; VX2_ROWS_CAP];
        let n = trunc_rows(&mut tips, &texts, 60, &mut rows);
        assert_eq!(n, 4);
        for k in 1..n {
            assert!(rows[k].y >= rows[k - 1].y + rows[k - 1].h, "行矩形不得重叠");
        }
        assert_eq!(rows[2].truncated, false);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_texttrunc_v2_checks();
        assert!(set.all_passed(), "F247 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
