//! F243 隐藏文件与受保护文件显示 · 判据实装。
//!
//! **判据锚**：主册 F243「隐藏文件与受保护文件显示」。
//!
//! **验收标准第一句（任务包原文）**：双开关独立用例。
//!
//! **判据（主册原文摘录）**：资源管理器查看菜单双开关：显示隐藏项
//! （点名的 `.` 开头/隐藏属性文件，显示时图标半透明 60% 区分）、显示
//! 系统保护项（默认关，开启需确认对话框说明风险——用户点了确认就给
//! 看，不搞祖传的「连提示都不给」）；隐藏项参与搜索但结果里标注「隐藏」
//! 徽标；系统保护项即使开启显示也不出现在默认搜索（防误删，明确文档
//! 化）。验收：半透明 60% 视觉走查；开启保护项确认对话框文案审查；
//! 搜索标注徽标判据；保护项搜索排除验证。
//!
//! **设计要点**：
//! - [`FileVisibility`] 双开关状态机：隐藏项开关一键直切；保护项开关
//!   走**确认对话框路径**——`request`（开对话框）→ `confirm`（用户点了
//!   确认才置位）或 `cancel`（不留痕不置位），两级独立、互不影响；
//! - 可见性判定三级分类（[`FileClass`]）：`.` 开头或隐藏属性位 →
//!   Hidden；系统属性位 → Protected（优先级最高——一个文件同时带
//!   隐藏+系统位时按保护项处理，宁可多保护）；判定函数输出渲染面
//!   四元组（可见性/不透明度/徽标/搜索资格），半透明 60% 从这里取；
//! - 搜索管线是**结构性**分流：Hidden 无条件参与搜索并带「隐藏」徽标
//!   （搜索面与浏览面独立——浏览跟随 show_hidden，搜索始终参与）；
//!   Protected 在查询入口处直接拒绝（`ExcludedProtected`），即使
//!   show_protected 开启也不进结果集——防误删是架构约束不是开关位，
//!   文档锚 [`PROTECTED_SEARCH_DOC`]；
//! - 确认对话框文案审查接口（[`review_dialog_text`]）：正文必须说明
//!   风险并给退路（含「可随时关闭」）——「连提示都不给」的祖传行为
//!   在文案审查面就被拦下；
//! - 搜索结果面用 alloc Vec 但定容 128 + 最旧淘汰；开关账本定容 64；
//!   时间一律注入（毫秒戳），模块不持时钟。
//!
//! **依赖锚点**：F205（徽标/Tooltip 渲染面）、F233（文件对话框同源
//! 属性位）、F219（排序视图记忆——浏览面开关随视图持久化）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 隐藏项图标不透明度（%）——判据「显示时图标半透明 60% 区分」。
pub const SEMI_OPACITY_PCT: u32 = 60;

/// 不透明档（%）——普通文件与已可见保护项。
pub const FULL_OPACITY_PCT: u32 = 100;

/// DOS 目录项隐藏属性位（0x02，F233 文件对话框同源属性域）。
pub const ATTR_HIDDEN: u8 = 0x02;

/// DOS 目录项系统属性位（0x04——保护项判定依据）。
pub const ATTR_SYSTEM: u8 = 0x04;

/// 点名前缀：`.` 开头的文件按隐藏项处理（判据「点名的 . 开头」）。
pub const DOT_PREFIX: u8 = b'.';

/// DOS 目录项只读属性位（F233 文件对话框同源属性域——属性页展示面）。
pub const ATTR_READONLY: u8 = 0x01;

/// DOS 目录项归档属性位（同源属性域）。
pub const ATTR_ARCHIVE: u8 = 0x20;

/// 「隐藏」徽标文案（判据「结果里标注『隐藏』徽标」的文案锚——
/// 搜索结果面与浏览面共用同一字串，杜绝双源漂移）。
pub const BADGE_HIDDEN_TEXT: &str = "隐藏";

/// 浏览列表容量（目录枚举定容纪律）。
pub const BROWSE_CAP: usize = 64;

/// 文件名缓存上限（字节）。
pub const NAME_CAP: usize = 64;

/// 搜索结果面容量（超出淘汰最旧——定容纪律）。
pub const RESULTS_CAP: usize = 128;

/// 开关事件账本容量。
pub const LEDGER_CAP: usize = 64;

/// 保护项搜索排除的文档锚（判据「防误删，明确文档化」）。
pub const PROTECTED_SEARCH_DOC: &str = "系统保护项（system 属性位）不参与默认搜索：查询入口结构性排除，即使「显示系统保护项」开启也不入结果集——防误删是架构约束而非开关位；高级检索工具需显式声明 include_protected 且逐次留痕（不在本模块实现面）。";

/// 确认对话框标题（文案审查接口的第一半）。
pub const PROTECT_CONFIRM_TITLE: &str = "显示系统保护文件";

/// 确认对话框正文（文案审查接口的第二半）——判据「说明风险」+ 给退路。
pub const PROTECT_CONFIRM_BODY: &str = "系统保护文件是操作系统与驱动赖以运行的关键文件。显示后它们与普通文件混排，误删或误改可能导致系统无法启动。这些文件仍不会出现在默认搜索结果中。风险自担，可随时关闭本开关恢复默认。";

// ---------------------------------------------------------------------------
// 三级分类与可见性判定
// ---------------------------------------------------------------------------

/// 文件三级分类（判据「. 开头/隐藏属性/保护属性三级分类」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileClass {
    /// 普通文件。
    Normal,
    /// 隐藏项：`.` 开头或带隐藏属性位。
    Hidden,
    /// 系统保护项：带系统属性位（优先级最高）。
    Protected,
}

/// 三级分类：属性位优先于点名前缀（系统+隐藏位并存按保护项处理）。
pub fn classify(name: &[u8], attrs: u8) -> FileClass {
    if attrs & ATTR_SYSTEM != 0 {
        return FileClass::Protected;
    }
    if (!name.is_empty() && name[0] == DOT_PREFIX) || attrs & ATTR_HIDDEN != 0 {
        return FileClass::Hidden;
    }
    FileClass::Normal
}

/// 分类的人类可读理由（设置中心/属性页 tooltip 消费）。
pub fn class_reason(c: FileClass) -> &'static str {
    match c {
        FileClass::Normal => "常规文件",
        FileClass::Hidden => "隐藏项（. 开头或带隐藏属性）",
        FileClass::Protected => "系统保护项（带系统属性，不参与默认搜索）",
    }
}

/// 只读位判定（属性页锁定「只读」复选框的依据）。
pub fn is_readonly(attrs: u8) -> bool {
    attrs & ATTR_READONLY != 0
}

/// 归档位判定。
pub fn is_archive(attrs: u8) -> bool {
    attrs & ATTR_ARCHIVE != 0
}

/// 属性位串格式化（R/H/S/A 次序恒定——属性页展示与测试对账共用）。
/// 返回写入 `out` 的字节数；out 过小则截断（痕迹由返回值 < 应有长度体现）。
pub fn attrs_str(attrs: u8, out: &mut [u8]) -> usize {
    const LETTERS: [(u8, u8); 4] = [
        (ATTR_READONLY, b'R'),
        (ATTR_HIDDEN, b'H'),
        (ATTR_SYSTEM, b'S'),
        (ATTR_ARCHIVE, b'A'),
    ];
    let mut n = 0;
    for &(bit, ch) in LETTERS.iter() {
        if attrs & bit != 0 {
            if n < out.len() {
                out[n] = ch;
            }
            n += 1;
        }
    }
    n
}

/// 属性位教学文案（属性页/设置中心说明共用锚）。
pub const ATTR_DOC: &str = "R=只读 H=隐藏 S=系统（保护项，搜索排除） A=归档；系统位优先于隐藏位判定。";

/// 定长文件名缓存（零堆分类/搜索热路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameBuf {
    len: u8,
    bytes: [u8; NAME_CAP],
}

impl NameBuf {
    /// 从字节串构造（超长截断——截断痕迹由 len 与源长差体现）。
    pub fn from_bytes(src: &[u8]) -> NameBuf {
        let mut out = NameBuf { len: 0, bytes: [0u8; NAME_CAP] };
        let n = src.len().min(NAME_CAP);
        out.bytes[..n].copy_from_slice(&src[..n]);
        out.len = n as u8;
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub fn is_dot_prefix(&self) -> bool {
        self.len > 0 && self.bytes[0] == DOT_PREFIX
    }

    /// 名字字节数（截断痕迹核对面：源长超过 NAME_CAP 时 len < 源长）。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否发生截断（超长名入库时如实标注）。
    pub fn was_truncated(&self, src_len: usize) -> bool {
        src_len > NAME_CAP
    }

    /// 扩展名（最后一个 `.` 之后的部分；无名点/点首名返回空——
    /// `.gitignore` 的扩展名按惯例视为空而非 "gitignore"）。
    pub fn extension(&self) -> &[u8] {
        let hay = self.as_bytes();
        if hay.is_empty() {
            return &[];
        }
        match hay[1..].iter().rposition(|&b| b == b'.') {
            Some(pos) => &hay[pos + 2..],
            None => &[],
        }
    }

    /// 主名（最后一个 `.` 之前的部分——改名/重命名面对）。
    pub fn stem(&self) -> &[u8] {
        let hay = self.as_bytes();
        if hay.is_empty() {
            return &[];
        }
        match hay[1..].iter().rposition(|&b| b == b'.') {
            Some(pos) => &hay[..pos + 1],
            None => hay,
        }
    }

    /// 扩展名匹配（ASCII 大小写不敏感——"MD"/"md" 同判）。
    pub fn has_extension(&self, ext: &[u8]) -> bool {
        self.extension().eq_ignore_ascii_case(ext)
    }

    /// 查询匹配：ASCII 大小写不敏感的子串匹配；空查询匹配一切。
    pub fn matches_query(&self, query: &[u8]) -> bool {
        if query.is_empty() {
            return true;
        }
        let hay = self.as_bytes();
        if hay.len() < query.len() {
            return false;
        }
        for i in 0..=hay.len() - query.len() {
            let mut hit = true;
            for (j, &q) in query.iter().enumerate() {
                if hay[i + j].to_ascii_lowercase() != q.to_ascii_lowercase() {
                    hit = false;
                    break;
                }
            }
            if hit {
                return true;
            }
        }
        false
    }
}

/// 可见性判定四元组（渲染面/搜索面的唯一事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisVerdict {
    /// 浏览面是否显示。
    pub visible: bool,
    /// 图标不透明度（%）——隐藏项可见时 60%，其余 100/0。
    pub opacity_pct: u32,
    /// 「隐藏」徽标（搜索结果标注判据）。
    pub badge_hidden: bool,
    /// 默认搜索资格——保护项恒 false（结构性排除）。
    pub search_eligible: bool,
}

// ---------------------------------------------------------------------------
// 双开关状态机
// ---------------------------------------------------------------------------

/// 开关事件账本条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisEvent {
    /// 事件时刻（ms，注入式）。
    pub ts: u64,
    /// 1=切隐藏开关 2=保护确认框开启 3=保护确认（置位） 4=保护取消。
    pub kind: u8,
    /// 开关终态（kind=1 时有效）。
    pub to: bool,
}

/// 隐藏/保护双开关状态机（查看菜单两个独立开关的唯一事实源）。
pub struct FileVisibility {
    show_hidden: bool,
    show_protected: bool,
    /// 确认对话框是否开着（临时态，不入持久化）。
    dialog_open: bool,
    pub version: u32,
    /// 用户点了确认的次数（文案审查→行为留痕对账）。
    pub confirmed_count: u32,
    /// 用户取消的次数（提示有效的证据面）。
    pub cancelled_count: u32,
    ledger: RingLog<VisEvent, LEDGER_CAP>,
}

impl FileVisibility {
    pub fn new() -> FileVisibility {
        FileVisibility {
            show_hidden: false,
            show_protected: false,
            dialog_open: false,
            version: 0,
            confirmed_count: 0,
            cancelled_count: 0,
            ledger: RingLog::new(),
        }
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn show_protected(&self) -> bool {
        self.show_protected
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    /// 开关一：显示隐藏项（一键直切，无需确认——判据只对保护项设卡）。
    pub fn toggle_hidden(&mut self, ts: u64) -> bool {
        self.show_hidden = !self.show_hidden;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(VisEvent { ts, kind: 1, to: self.show_hidden });
        self.show_hidden
    }

    /// 开关二第一段：请求显示保护项 → 开确认对话框（默认关，不直接置位）。
    pub fn request_show_protected(&mut self, ts: u64) -> bool {
        if self.show_protected || self.dialog_open {
            return false;
        }
        self.dialog_open = true;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(VisEvent { ts, kind: 2, to: false });
        true
    }

    /// 开关二第二段：用户点了确认 → 才置位显示保护项。
    pub fn confirm_protect(&mut self, ts: u64) -> bool {
        if !self.dialog_open {
            return false;
        }
        self.dialog_open = false;
        self.show_protected = true;
        self.confirmed_count += 1;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(VisEvent { ts, kind: 3, to: true });
        true
    }

    /// 开关二取消路径：不置位、留取消痕（提示有效性的证据）。
    pub fn cancel_protect(&mut self, ts: u64) -> bool {
        if !self.dialog_open {
            return false;
        }
        self.dialog_open = false;
        self.cancelled_count += 1;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(VisEvent { ts, kind: 4, to: false });
        true
    }

    /// 关闭保护项显示（判据文案「可随时关闭本开关恢复默认」的行为面）。
    pub fn hide_protected(&mut self, ts: u64) -> bool {
        if !self.show_protected {
            return false;
        }
        self.show_protected = false;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(VisEvent { ts, kind: 1, to: false });
        true
    }

    /// 可见性判定：三级分类 → 渲染/搜索四元组。
    ///
    /// - Hidden：浏览跟随 show_hidden，可见时 60% 半透明；搜索无条件
    ///   参与（徽标标注——搜索面与浏览面独立）；
    /// - Protected：浏览跟随 show_protected（须确认后开启），不透明
    ///   100%；搜索**无条件排除**（结构性，见 [`PROTECTED_SEARCH_DOC`]）。
    pub fn verdict(&self, name: &[u8], attrs: u8) -> VisVerdict {
        match classify(name, attrs) {
            FileClass::Protected => VisVerdict {
                visible: self.show_protected,
                opacity_pct: if self.show_protected { FULL_OPACITY_PCT } else { 0 },
                badge_hidden: false,
                search_eligible: false,
            },
            FileClass::Hidden => VisVerdict {
                visible: self.show_hidden,
                opacity_pct: if self.show_hidden { SEMI_OPACITY_PCT } else { 0 },
                badge_hidden: true,
                search_eligible: true,
            },
            FileClass::Normal => VisVerdict {
                visible: true,
                opacity_pct: FULL_OPACITY_PCT,
                badge_hidden: false,
                search_eligible: true,
            },
        }
    }

    /// 默认态审计：双开关全关（判据「显示系统保护项默认关」）。
    pub fn is_default(&self) -> bool {
        !self.show_hidden && !self.show_protected && !self.dialog_open
    }

    /// 账本读出（新→旧）。
    pub fn recent_events(&self) -> [Option<VisEvent>; LEDGER_CAP] {
        let mut out = [const { None }; LEDGER_CAP];
        for (k, ev) in self.ledger.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }

    /// 账本条数（容量有界性核对）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 事件种类分布统计（kind 1..=4 → 槽 1..=4；审计面：确认/取消
    /// 比例是「提示有效性」的量化证据）。
    pub fn event_counts(&self) -> [u32; 5] {
        let mut counts = [0u32; 5];
        for ev in self.ledger.newest_first().iter() {
            let slot = (ev.kind as usize).min(4);
            counts[slot] += 1;
        }
        counts
    }

    /// 持久化导出（差异面）：8 字节 = "VFIS" 头 + 版本号 LE + 开关位。
    pub fn export_state(&self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0..4].copy_from_slice(b"VFIS");
        out[4..8].copy_from_slice(&self.version.to_le_bytes());
        out[7] = self.encode();
        out
    }

    /// 持久化导入：头不符显性拒绝；开关位经 decode 正规路径应用
    /// （账本不缺环）；版本号只增不减（重放旧快照不倒退审计面）。
    pub fn import_state(&mut self, blob: &[u8; 8], ts: u64) -> bool {
        if &blob[0..4] != b"VFIS" {
            return false;
        }
        let mut vb = [0u8; 4];
        vb.copy_from_slice(&blob[4..8]);
        let ver = u32::from_le_bytes(vb);
        if !self.decode(blob[7], ts) {
            return false;
        }
        if ver > self.version {
            self.version = ver;
        }
        true
    }

    /// 持久化编码：bit0=显示隐藏项 bit1=显示保护项（对话框临时态不入）。
    pub fn encode(&self) -> u8 {
        self.show_hidden as u8 | ((self.show_protected as u8) << 1)
    }

    /// 持久化解码：非法字节（>3）显性拒绝并保持现状。
    pub fn decode(&mut self, byte: u8, ts: u64) -> bool {
        if byte > 3 {
            return false;
        }
        let h = byte & 1 == 1;
        let p = byte & 2 != 0;
        if h != self.show_hidden || p != self.show_protected {
            self.show_hidden = h;
            self.show_protected = p;
            self.dialog_open = false;
            self.version = self.version.wrapping_add(1);
            self.ledger.push(VisEvent { ts, kind: 1, to: h });
        }
        true
    }
}

impl Default for FileVisibility {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 确认对话框文案审查接口
// ---------------------------------------------------------------------------

/// 文案审查结论（判据「确认对话框文案审查」的机器化形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogReview {
    /// 标题非空。
    pub has_title: bool,
    /// 正文非空。
    pub has_body: bool,
    /// 正文说明了风险。
    pub mentions_risk: bool,
    /// 正文给了退路（可随时关闭/恢复默认）。
    pub mentions_reversible: bool,
    /// 正文交代了搜索排除行为（保护项仍不进默认搜索）。
    pub mentions_search_excluded: bool,
}

impl DialogReview {
    pub fn all_ok(&self) -> bool {
        self.has_title && self.has_body && self.mentions_risk && self.mentions_reversible && self.mentions_search_excluded
    }
}

/// 字节级子串判定（'static 文案审查专用，无堆）。
fn str_contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if h.len() < n.len() {
        return false;
    }
    for i in 0..=h.len() - n.len() {
        if &h[i..i + n.len()] == n {
            return true;
        }
    }
    false
}

/// 确认对话框文案审查：逐要素核对（判据「文案审查」判据的落点）。
pub fn review_dialog_text() -> DialogReview {
    let body = PROTECT_CONFIRM_BODY;
    DialogReview {
        has_title: !PROTECT_CONFIRM_TITLE.is_empty(),
        has_body: !body.is_empty(),
        mentions_risk: str_contains(body, "风险"),
        mentions_reversible: str_contains(body, "可随时关闭"),
        mentions_search_excluded: str_contains(body, "默认搜索"),
    }
}

// ---------------------------------------------------------------------------
// 目录浏览面（批量分类扫描器 + 过滤器）
// ---------------------------------------------------------------------------

/// 浏览过滤器（资源管理器视图面的三种口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseFilter {
    /// 全量（内部审计用）。
    Everything,
    /// 浏览口径：只收 `verdict.visible` 为真的条目（跟随双开关）。
    VisibleOnly,
    /// 隐藏项专项视图（设置页「隐藏项预览」用）。
    HiddenOnly,
}

/// 浏览条目（分类 + 渲染四元组的落地面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowseItem {
    pub name: NameBuf,
    pub class: FileClass,
    pub opacity_pct: u32,
    pub badge_hidden: bool,
    /// 浏览面是否可见（过滤器 VisibleOnly 的判据）。
    pub visible: bool,
}

/// 目录批量扫描结果（定容 Vec + 最旧淘汰——账本面允许 alloc）。
pub struct BrowseList {
    items: Vec<BrowseItem>,
    /// 因容量被丢弃的条目数。
    pub dropped: u32,
}

impl BrowseList {
    pub fn new() -> BrowseList {
        BrowseList { items: Vec::new(), dropped: 0 }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[BrowseItem] {
        &self.items
    }

    /// 过滤判定：过滤器 × 条目四元组。
    pub fn accepts(filter: BrowseFilter, item: &BrowseItem) -> bool {
        match filter {
            BrowseFilter::Everything => true,
            BrowseFilter::VisibleOnly => item.visible,
            BrowseFilter::HiddenOnly => item.class == FileClass::Hidden,
        }
    }

    /// 单条扫描入列（分类 → 四元组 → 过滤 → 定容淘汰最旧）。
    pub fn scan(&mut self, name: &[u8], attrs: u8, vis: &FileVisibility, filter: BrowseFilter) -> bool {
        let class = classify(name, attrs);
        let v = vis.verdict(name, attrs);
        let item = BrowseItem {
            name: NameBuf::from_bytes(name),
            class,
            opacity_pct: v.opacity_pct,
            badge_hidden: v.badge_hidden,
            visible: v.visible,
        };
        if !Self::accepts(filter, &item) {
            return false;
        }
        if self.items.len() >= BROWSE_CAP {
            self.items.remove(0);
            self.dropped += 1;
        }
        self.items.push(item);
        true
    }

    /// 批量扫描器（目录枚举主入口）：一批 (名字， 属性) 逐条入列，
    /// 返回实际入列数——「抽 20 处实测」走查可以喂 20 条混合目录。
    pub fn scan_batch(&mut self, batch: &[(&[u8], u8)], vis: &FileVisibility, filter: BrowseFilter) -> usize {
        let mut accepted = 0;
        for &(name, attrs) in batch.iter() {
            if self.scan(name, attrs, vis, filter) {
                accepted += 1;
            }
        }
        accepted
    }

    /// 三级分类计数：(普通， 隐藏， 保护)——目录体检面。
    pub fn class_counts(&self) -> (usize, usize, usize) {
        let (mut n, mut h, mut p) = (0, 0, 0);
        for item in self.items.iter() {
            match item.class {
                FileClass::Normal => n += 1,
                FileClass::Hidden => h += 1,
                FileClass::Protected => p += 1,
            }
        }
        (n, h, p)
    }

    /// 浏览渲染审计：可见条目的不透明度只许 ∈ {60, 100}，且隐藏项可见
    /// 时恰为 60（判据「半透明 60% 区分」在浏览面的机器形态）。
    pub fn audit_opacities(&self) -> bool {
        self.items.iter().all(|it| {
            if !it.visible {
                return it.opacity_pct == 0;
            }
            if it.class == FileClass::Hidden {
                it.opacity_pct == SEMI_OPACITY_PCT && it.badge_hidden
            } else {
                it.opacity_pct == FULL_OPACITY_PCT
            }
        })
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Default for BrowseList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 搜索管线（隐藏项参与+徽标；保护项结构性排除）
// ---------------------------------------------------------------------------

/// 搜索结果条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub name: NameBuf,
    pub class: FileClass,
    /// 「隐藏」徽标（Hidden 类恒 true）。
    pub badge_hidden: bool,
    /// 建议不透明度（%）（浏览渲染同源）。
    pub opacity_pct: u32,
}

/// 查询入口的显性结论（无静默：进/滤/排都留痕可查）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// 命中入结果（普通项）。
    Accepted,
    /// 命中入结果且带「隐藏」徽标。
    AcceptedHiddenBadge,
    /// 未命中查询词（普通过滤，非排除）。
    FilteredNoMatch,
    /// 保护项结构性排除（无条件，与开关无关）。
    ExcludedProtected,
    /// 结果面满（丢弃新条目——保守不挤旧）。
    Full,
}

/// 搜索结果面：定容 Vec（容量 128 + 最旧淘汰——账本面允许 alloc）。
pub struct SearchPipe {
    results: Vec<SearchHit>,
    /// 因容量淘汰的最旧条目数。
    pub evicted: u32,
    /// 被结构性排除的保护项计数（防误删架构的留痕面）。
    pub excluded_protected: u32,
}

impl SearchPipe {
    pub fn new() -> SearchPipe {
        SearchPipe { results: Vec::new(), evicted: 0, excluded_protected: 0 }
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    pub fn hits(&self) -> &[SearchHit] {
        &self.results
    }

    /// 查询入口：三级分类 → 分流（判据「搜索管线集成」的落点）。
    pub fn offer(&mut self, name: &[u8], attrs: u8, vis: &FileVisibility, query: &[u8]) -> Offer {
        match classify(name, attrs) {
            FileClass::Protected => {
                // 结构性排除：不看 show_protected、不看查询词——
                // 防误删是架构约束（判据原文「即使开启显示也不出现」）。
                self.excluded_protected += 1;
                Offer::ExcludedProtected
            }
            cls => {
                let nb = NameBuf::from_bytes(name);
                if !nb.matches_query(query) {
                    return Offer::FilteredNoMatch;
                }
                let v = vis.verdict(name, attrs);
                let hit = SearchHit {
                    name: nb,
                    class: cls,
                    badge_hidden: v.badge_hidden,
                    opacity_pct: v.opacity_pct.max(SEMI_OPACITY_PCT), // 搜索面隐藏项始终按 60% 徽标语义渲染
                };
                if self.results.len() >= RESULTS_CAP {
                    self.results.remove(0);
                    self.evicted += 1;
                }
                self.results.push(hit);
                if cls == FileClass::Hidden {
                    Offer::AcceptedHiddenBadge
                } else {
                    Offer::Accepted
                }
            }
        }
    }

    /// 排除判据审计：结果集中不存在任何保护项（判据「保护项搜索排除
    /// 验证」的机器形态——对账 [`Self::excluded_protected`] 留痕）。
    pub fn audit_no_protected(&self) -> bool {
        self.results.iter().all(|h| h.class != FileClass::Protected)
    }

    /// 徽标判据审计：Hidden 类条目全部带徽标，非 Hidden 全不带。
    pub fn audit_badges(&self) -> bool {
        self.results
            .iter()
            .all(|h| h.badge_hidden == (h.class == FileClass::Hidden))
    }

    /// 徽标文案（渲染面唯一来源）：Hidden →「隐藏」，其余空串——
    /// 结果面标注判据的文案锚（与浏览面共用 [`BADGE_HIDDEN_TEXT`]）。
    pub fn badge_text(class: FileClass) -> &'static str {
        if class == FileClass::Hidden {
            BADGE_HIDDEN_TEXT
        } else {
            ""
        }
    }

    /// 批量查询入口：一批 (名字， 属性) 逐条过管线，返回实际入列数。
    pub fn offer_batch(&mut self, batch: &[(&[u8], u8)], vis: &FileVisibility, query: &[u8]) -> usize {
        let mut accepted = 0;
        for &(name, attrs) in batch.iter() {
            match self.offer(name, attrs, vis, query) {
                Offer::Accepted | Offer::AcceptedHiddenBadge => accepted += 1,
                _ => {}
            }
        }
        accepted
    }

    /// 按类稳定导出（普通在前、隐藏在后——结果页「隐藏项沉底」惯例；
    /// 保护项结构性不存在于结果集，无需档位）。
    pub fn hits_hidden_last(&self) -> Vec<SearchHit> {
        let mut normal: Vec<SearchHit> = Vec::new();
        let mut hidden: Vec<SearchHit> = Vec::new();
        for h in self.results.iter().copied() {
            if h.class == FileClass::Hidden {
                hidden.push(h);
            } else {
                normal.push(h);
            }
        }
        normal.extend_from_slice(&hidden);
        normal
    }

    /// 结果面不透明度审计：全部 ∈ {60, 100}（搜索面隐藏项恒按 60% 徽标
    /// 语义渲染——[`Self::offer`] 的钳制面逐条复核）。
    pub fn audit_opacities(&self) -> bool {
        self.results
            .iter()
            .all(|h| h.opacity_pct == SEMI_OPACITY_PCT || h.opacity_pct == FULL_OPACITY_PCT)
    }

    pub fn clear(&mut self) {
        self.results.clear();
    }
}

impl Default for SearchPipe {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F243 自检（判据：双开关独立、60% 半透明、文案审查、徽标、保护项
/// 搜索排除；含 xors32 fuzz）。
pub fn run_hidfiles_checks() -> CheckSet {
    let mut set = CheckSet::new("F243-hidfiles");

    // 1. 默认态审计：双开关全关、对话框闭合（判据「默认关」）。
    let mut vis = FileVisibility::new();
    set.add(
        "defaults: both switches off",
        vis.is_default() && !vis.show_hidden() && !vis.show_protected(),
        "",
    );

    // 2. 双开关独立用例（判据本体）：切隐藏开关不动保护开关，反之亦然。
    let v0 = vis.version;
    let _ = vis.toggle_hidden(10);
    let after_hide = (vis.show_hidden(), vis.show_protected(), vis.dialog_open());
    let _ = vis.request_show_protected(20);
    let after_req = (vis.show_hidden(), vis.show_protected(), vis.dialog_open());
    set.add(
        "two switches independent",
        v0 + 1 == vis.version - 1
            && after_hide == (true, false, false)
            && after_req == (true, false, true),
        "",
    );

    // 3. 三级分类：点名前缀 / 隐藏属性位 / 系统属性位 / 普通。
    set.add(
        "classify three tiers",
        classify(b".gitignore", 0) == FileClass::Hidden
            && classify(b"notes.txt", ATTR_HIDDEN) == FileClass::Hidden
            && classify(b"kernel.sys", ATTR_SYSTEM) == FileClass::Protected
            && classify(b"report.md", 0) == FileClass::Normal,
        "",
    );

    // 4. 分类优先级：系统+隐藏位并存按保护项（宁可多保护）。
    set.add(
        "system attr outranks hidden",
        classify(b"winload.sys", ATTR_SYSTEM | ATTR_HIDDEN) == FileClass::Protected,
        "",
    );

    // 5. 半透明 60% 视觉判据：开 → 可见且 60%；关 → 不可见且 0。
    let v_on = vis.verdict(b".config", 0);
    let _ = vis.toggle_hidden(30);
    let v_off = vis.verdict(b".config", 0);
    set.add(
        "hidden shows at 60% opacity",
        v_on.visible && v_on.opacity_pct == SEMI_OPACITY_PCT && v_on.badge_hidden
            && v_off.opacity_pct == 0
            && !v_off.visible,
        "",
    );

    // 6. 保护项确认路径：未确认不置位；确认置位；取消路径留痕不置位。
    let _ = vis.confirm_protect(40); // 对话框开着（check 2 request 过）→ 置位
    let confirmed = vis.show_protected() && vis.confirmed_count == 1;
    let mut vis2 = FileVisibility::new();
    let _ = vis2.request_show_protected(10);
    let _ = vis2.cancel_protect(20);
    set.add(
        "protect needs explicit confirm",
        confirmed
            && !vis2.show_protected()
            && vis2.cancelled_count == 1
            && !vis2.dialog_open(),
        "",
    );

    // 7. 确认对话框文案审查（判据本体）：标题/正文/风险/退路/搜索排除。
    let rev = review_dialog_text();
    set.add(
        "confirm dialog copy review",
        rev.all_ok() && PROTECT_CONFIRM_BODY.len() > 20,
        "",
    );

    // 8. 保护项搜索结构性排除（判据本体）：即使 show_protected 开启，
    //    offer 保护文件 → 排除留痕、结果集零保护项。
    let mut pipe = SearchPipe::new();
    let o1 = pipe.offer(b"winload.sys", ATTR_SYSTEM, &vis, b"");
    set.add(
        "protected excluded from search structurally",
        o1 == Offer::ExcludedProtected
            && vis.show_protected()
            && pipe.excluded_protected == 1
            && pipe.audit_no_protected()
            && PROTECTED_SEARCH_DOC.contains("结构性排除"),
        "",
    );

    // 9. 隐藏项参与搜索 + 徽标（判据本体）：搜索面无视 show_hidden。
    let mut vis3 = FileVisibility::new(); // 双关
    let mut pipe2 = SearchPipe::new();
    let o2 = pipe2.offer(b".env.local", 0, &vis3, b"env");
    set.add(
        "hidden joins search with badge",
        o2 == Offer::AcceptedHiddenBadge
            && pipe2.len() == 1
            && pipe2.hits()[0].badge_hidden
            && pipe2.hits()[0].class == FileClass::Hidden
            && pipe2.audit_badges(),
        "",
    );

    // 10. 查询过滤与大小写不敏感：不命中显性 FilteredNoMatch。
    let mut pipe3 = SearchPipe::new();
    let miss = pipe3.offer(b"Report_Final.MD", 0, &vis3, b"draft");
    let hit = pipe3.offer(b"Report_Final.MD", 0, &vis3, b"final.md");
    set.add(
        "query filter case-insensitive",
        miss == Offer::FilteredNoMatch && hit == Offer::Accepted && pipe3.len() == 1,
        "",
    );

    // 11. 结果面容量淘汰：129 条 → 128 条、evicted=1、最旧被挤。
    let mut pipe4 = SearchPipe::new();
    for i in 0..(RESULTS_CAP + 1) as u32 {
        let name = [b'f', b'i', b'l', b'e', b'0' + (i % 10) as u8];
        let _ = pipe4.offer(&name, 0, &vis3, b"");
    }
    set.add(
        "results cap evicts oldest",
        pipe4.len() == RESULTS_CAP
            && pipe4.evicted == 1
            && pipe4.hits()[0].name.as_bytes()[4] == b'1',
        "",
    );

    // 12. 持久化 round-trip + 非法字节拒绝。
    let _ = vis3.toggle_hidden(50);
    let byte = vis3.encode();
    let mut vis4 = FileVisibility::new();
    let ok = vis4.decode(byte, 60);
    set.add(
        "settings round-trip + reject",
        ok && vis4.show_hidden() && !vis4.show_protected() && !vis4.decode(0x04, 70),
        "",
    );

    // 13. xors32 fuzz：随机名/属性/查询/开关流——保护项永不入结果集、
    //     徽标↔类别严格一致、不透明度 ∈ {0,60,100}、无 panic。
    let mut x: u32 = 0x9E37_79B9;
    let mut fz_vis = FileVisibility::new();
    let mut fz_pipe = SearchPipe::new();
    let mut survived = true;
    let mut name_buf = [0u8; 16];
    for i in 0..1500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let nlen = (x % 12 + 1) as usize;
        for b in name_buf.iter_mut().take(nlen) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // 名字字符池带 '.' 概率，制造点名前缀流。
            *b = if x % 11 == 0 { b'.' } else { b'a' + (x % 26) as u8 };
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let attrs = (x & 0x07) as u8;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let qlen = (x % 3) as usize;
        let query = &name_buf[..qlen];
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        // 随机拨开关流（含确认/取消路径）。
        match x % 6 {
            0 => {
                let _ = fz_vis.toggle_hidden(i as u64);
            }
            1 => {
                let _ = fz_vis.request_show_protected(i as u64);
            }
            2 => {
                let _ = fz_vis.confirm_protect(i as u64);
            }
            3 => {
                let _ = fz_vis.cancel_protect(i as u64);
            }
            _ => {}
        }
        let v = fz_vis.verdict(&name_buf[..nlen], attrs);
        if v.opacity_pct != 0 && v.opacity_pct != SEMI_OPACITY_PCT && v.opacity_pct != FULL_OPACITY_PCT {
            survived = false;
        }
        let offer = fz_pipe.offer(&name_buf[..nlen], attrs, &fz_vis, query);
        if offer == Offer::ExcludedProtected {
            // 排除面必须与分类器一致：系统位判定。
            if attrs & ATTR_SYSTEM == 0 {
                survived = false;
            }
        } else if offer != Offer::FilteredNoMatch && attrs & ATTR_SYSTEM != 0 {
            survived = false; // 保护项混入结果
        }
        if !fz_pipe.audit_no_protected() || !fz_pipe.audit_badges() {
            survived = false;
        }
    }
    set.add(
        "fuzz 1500 offers invariants hold",
        survived && fz_pipe.audit_no_protected(),
        "",
    );

    // 14. 浏览面批量扫描（可见性过滤）+ 属性位串 + 扩展名解析 +
    //     状态导出/导入 + 徽标文案锚。
    let mut vis5 = FileVisibility::new();
    let _ = vis5.toggle_hidden(100);
    let mut bl = BrowseList::new();
    let batch: [(&[u8], u8); 5] = [
        (b"readme.txt", 0),
        (b".env", 0),
        (b"kernel.sys", ATTR_SYSTEM),
        (b"archive.dat", ATTR_ARCHIVE),
        (b"locked.ini", ATTR_READONLY),
    ];
    let took = bl.scan_batch(&batch, &vis5, BrowseFilter::VisibleOnly);
    let (n_cnt, h_cnt, p_cnt) = bl.class_counts();
    let mut abuf = [0u8; 8];
    let an = attrs_str(ATTR_READONLY | ATTR_HIDDEN | ATTR_SYSTEM, &mut abuf);
    let multi = NameBuf::from_bytes(b"archive.tar.gz");
    let blob = vis5.export_state();
    let mut vis6 = FileVisibility::new();
    let imp = vis6.import_state(&blob, 200);
    set.add(
        "browse batch scan & attrs str & extension & state io",
        took == 4
            && n_cnt == 3 && h_cnt == 1 && p_cnt == 0
            && bl.audit_opacities()
            && an == 3
            && &abuf[..3] == b"RHS"
            && multi.extension() == b"gz"
            && multi.stem() == b"archive.tar"
            && imp
            && vis6.show_hidden()
            && SearchPipe::badge_text(FileClass::Hidden) == BADGE_HIDDEN_TEXT
            && SearchPipe::badge_text(FileClass::Normal) == ""
            && is_readonly(ATTR_READONLY)
            && !is_archive(ATTR_SYSTEM),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_names_and_edge_cases() {
        assert_eq!(classify(b".", 0), FileClass::Hidden);
        assert_eq!(classify(b"..", 0), FileClass::Hidden);
        assert_eq!(classify(b"", 0), FileClass::Normal); // 空名非隐藏
        assert_eq!(classify(b"dir.hidden", 0), FileClass::Normal); // 只有前缀算
        assert_eq!(classify(b".hidden.sys", ATTR_SYSTEM), FileClass::Protected);
    }

    #[test]
    fn confirm_flow_state_machine() {
        let mut vis = FileVisibility::new();
        // 没开对话框就确认 → 拒绝。
        assert!(!vis.confirm_protect(1));
        assert!(!vis.cancel_protect(2));
        // 开 → 取消 → 重开 → 确认。
        assert!(vis.request_show_protected(3));
        assert!(vis.cancel_protect(4));
        assert!(vis.request_show_protected(5));
        assert!(vis.confirm_protect(6));
        assert!(vis.show_protected());
        assert_eq!(vis.cancelled_count, 1);
        assert_eq!(vis.confirmed_count, 1);
        // 已开启后再请求 → 拒绝（无需再确认）。
        assert!(!vis.request_show_protected(7));
        // 可随时关闭（文案承诺的行为面）。
        assert!(vis.hide_protected(8));
        assert!(!vis.show_protected());
    }

    #[test]
    fn search_pipe_protected_never_lands() {
        let mut vis = FileVisibility::new();
        let _ = vis.request_show_protected(1);
        let _ = vis.confirm_protect(2);
        assert!(vis.show_protected());
        let mut pipe = SearchPipe::new();
        // 混合流：保护/隐藏/普通文件 + 命中与不命中查询。
        let _ = pipe.offer(b"bootmgfw.efi", ATTR_SYSTEM, &vis, b"");
        let _ = pipe.offer(b".gitconfig", 0, &vis, b"");
        let _ = pipe.offer(b"hello.txt", 0, &vis, b"zzz");
        let _ = pipe.offer(b"hello.log", 0, &vis, b"hello");
        assert_eq!(pipe.excluded_protected, 1);
        assert!(pipe.audit_no_protected());
        assert_eq!(pipe.len(), 2);
        // 命中的两条：隐藏带徽标、普通不带。
        assert!(pipe.hits().iter().all(|h| h.badge_hidden == (h.class == FileClass::Hidden)));
    }

    #[test]
    fn verdict_matrix_all_classes() {
        let mut vis = FileVisibility::new();
        let _ = vis.toggle_hidden(1);
        let _ = vis.request_show_protected(2);
        let _ = vis.confirm_protect(3);
        // 双开状态下的完整判定矩阵。
        let h = vis.verdict(b".x", 0);
        let p = vis.verdict(b"y", ATTR_SYSTEM);
        let n = vis.verdict(b"z", 0);
        assert!(h.visible && h.opacity_pct == 60 && h.badge_hidden && h.search_eligible);
        assert!(p.visible && p.opacity_pct == 100 && !p.badge_hidden && !p.search_eligible);
        assert!(n.visible && n.opacity_pct == 100 && !n.badge_hidden && n.search_eligible);
        // 关闭隐藏后：隐藏项消失但搜索资格仍在（搜索面独立）。
        let _ = vis.toggle_hidden(4);
        let h2 = vis.verdict(b".x", 0);
        assert!(!h2.visible && h2.opacity_pct == 0 && h2.search_eligible);
    }

    #[test]
    fn ledger_ring_wraparound() {
        let mut vis = FileVisibility::new();
        for i in 0..70u64 {
            let _ = vis.toggle_hidden(i * 10);
        }
        let events = vis.recent_events();
        let n = events.iter().flatten().count();
        assert_eq!(n, LEDGER_CAP);
        assert_eq!(events[0].unwrap().ts, 690);
    }

    #[test]
    fn attrs_ext_and_state_io() {
        // —— 扩展名/主名解析（多重点名、点名前缀、无点名三态）——
        let f = NameBuf::from_bytes(b"report.final.docx");
        assert_eq!(f.extension(), b"docx");
        assert_eq!(f.stem(), b"report.final");
        let dot = NameBuf::from_bytes(b".gitignore");
        assert!(dot.extension().is_empty(), "点名文件的扩展名按惯例为空");
        assert_eq!(dot.stem(), b".gitignore");
        let none = NameBuf::from_bytes(b"Makefile");
        assert!(none.extension().is_empty());
        assert_eq!(none.stem(), b"Makefile");
        // —— 属性位串：次序恒定 R→H→S→A；空位得空串 ——
        let mut buf = [0u8; 4];
        assert_eq!(attrs_str(0, &mut buf), 0);
        assert_eq!(attrs_str(ATTR_ARCHIVE | ATTR_SYSTEM, &mut buf), 2);
        assert_eq!(&buf[..2], b"SA");
        // 超短缓冲截断留痕：应写 4 字节，实际写满 2。
        let mut tiny = [0u8; 2];
        assert_eq!(attrs_str(0x2F, &mut tiny), 4);
        // —— 状态导出/导入 round-trip + 头不符显性拒绝 + 版本不倒退 ——
        let mut vis = FileVisibility::new();
        let _ = vis.toggle_hidden(10);
        let _ = vis.request_show_protected(20);
        let _ = vis.confirm_protect(30);
        let blob = vis.export_state();
        assert_eq!(&blob[0..4], b"VFIS");
        let mut vis2 = FileVisibility::new();
        assert!(vis2.import_state(&blob, 40));
        assert!(vis2.show_hidden() && vis2.show_protected());
        let mut bad = blob;
        bad[0] = b'X';
        assert!(!vis2.import_state(&bad, 50));
        let ver_before = vis2.version;
        let old = vis.export_state();
        assert!(vis2.import_state(&old, 60));
        assert!(vis2.version >= ver_before);
    }

    #[test]
    fn browse_list_filter_and_audit() {
        let mut vis = FileVisibility::new();
        let mut bl = BrowseList::new();
        let batch: [(&[u8], u8); 4] = [
            (b"a.txt", 0),
            (b".b", 0),
            (b"c.sys", ATTR_SYSTEM),
            (b"d.log", ATTR_HIDDEN),
        ];
        // 默认双关：可见面只收普通项。
        assert_eq!(bl.scan_batch(&batch, &vis, BrowseFilter::VisibleOnly), 1);
        assert_eq!(bl.len(), 1);
        // Everything 全量；HiddenOnly 只收隐藏类（含属性位隐藏）。
        bl.clear();
        assert_eq!(bl.scan_batch(&batch, &vis, BrowseFilter::Everything), 4);
        let (n, h, p) = bl.class_counts();
        assert_eq!((n, h, p), (1, 2, 1));
        // 计数含不可见项；可见性审计在默认态下：普通 100、其余 0。
        assert!(bl.audit_opacities());
        // 开隐藏后可见面收 3（保护仍关）。
        let _ = vis.toggle_hidden(1);
        bl.clear();
        assert_eq!(bl.scan_batch(&batch, &vis, BrowseFilter::VisibleOnly), 3);
        assert!(bl.audit_opacities());
        // 定容淘汰：塞满 + 1 → dropped=1（名字用运行期池，条条不同）。
        bl.clear();
        let mut names: Vec<[u8; 8]> = Vec::new();
        for i in 0..(BROWSE_CAP + 1) {
            names.push([b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8, b'.', b't', b'x', b't', 0]);
        }
        for k in 0..(BROWSE_CAP + 1) {
            let n8 = names[k];
            let _ = bl.scan(&n8[..7], 0, &vis, BrowseFilter::VisibleOnly);
        }
        assert_eq!(bl.len(), BROWSE_CAP);
        assert_eq!(bl.dropped, 1);
    }

    #[test]
    fn hidfiles_selfcheck_all_green() {
        let set = run_hidfiles_checks();
        assert!(set.all_passed(), "F243 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F243（隐藏文件与受保护文件显示）。v2 三件事：
// 1) 持久化 I/O：查看偏好册（双开关 + 隐藏项沉底视图位）v2 定长容器
//    序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验和，
//    四类损坏显性拒绝（与既有 VFIS 8 字节位包并存于追加段）；
// 2) UI 壳接线：浏览列表行绘制清单（行矩形 + 不透明度 + 「隐藏」徽标）
//    + 行命中测试 + 键盘遍历——「半透明 60% 视觉走查」的几何承载；
// 3) 判定面扩展：run_hidfiles_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：byte0 = bit0 隐藏开关 / bit1 保护开关 /
/// bit2 隐藏项沉底，byte1..4 保留清零。
pub const VX2_HF_PAYLOAD: usize = 4;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_HF_BLOB: usize = 9 + VX2_HF_PAYLOAD;

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

/// 查看偏好册（资源管理器查看菜单的持久化数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewPrefsBook {
    pub show_hidden: bool,
    pub show_protected: bool,
    /// 隐藏项沉底视图位（结果页排序惯例的开关记忆，F219 同源）。
    pub hidden_last: bool,
}

impl ViewPrefsBook {
    pub const fn new() -> ViewPrefsBook {
        ViewPrefsBook { show_hidden: false, show_protected: false, hidden_last: true }
    }

    /// 从状态机读出（开关位唯一事实源——不旁路 decode 纪律）。
    pub fn from_vis(vis: &FileVisibility) -> ViewPrefsBook {
        ViewPrefsBook {
            show_hidden: vis.show_hidden(),
            show_protected: vis.show_protected(),
            hidden_last: true,
        }
    }

    /// 推到状态机：走既有 decode 正规路径（账本不缺环、非法位被拒）。
    pub fn apply_to(&self, vis: &mut FileVisibility, ts: u64) -> bool {
        vis.decode(self.show_hidden as u8 | ((self.show_protected as u8) << 1), ts)
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_HF_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5] = self.show_hidden as u8 | ((self.show_protected as u8) << 1) | ((self.hidden_last as u8) << 2);
        out[6] = 0;
        out[7] = 0;
        out[8] = 0;
        let crc = vx2_fnv(&out[..9 + VX2_HF_PAYLOAD - 4]);
        out[9 + VX2_HF_PAYLOAD - 4..9 + VX2_HF_PAYLOAD]
            .copy_from_slice(&crc.to_le_bytes());
        VX2_HF_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<ViewPrefsBook, Vx2Error> {
        if blob.len() != VX2_HF_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_HF_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        Ok(ViewPrefsBook {
            show_hidden: blob[5] & 1 == 1,
            show_protected: blob[5] & 2 != 0,
            hidden_last: blob[5] & 4 != 0,
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 浏览列表行高（px）——v2 布局常量：F243 资源管理器行 24px。
pub const VX2_ROW_H_PX: i32 = 24;
/// 键盘遍历键码（与 F244 VK_UP/VK_DOWN/VK_RETURN 同码）。
pub const VX2_KEY_UP: u8 = 0x26;
pub const VX2_KEY_DOWN: u8 = 0x27;
pub const VX2_KEY_ENTER: u8 = 0x0D;

/// 浏览行绘制条目：行矩形 + 不透明度（60/100）+ 「隐藏」徽标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub name_len: usize,
    pub y: i32,
    pub h: i32,
    pub opacity_pct: u32,
    pub badge_hidden: bool,
}

/// 生成浏览行绘制清单（清单序 = 列表序；不透明度/徽标直取 BrowseItem
/// ——渲染面不自行判定，四处一事实）。
pub fn browse_rows(bl: &BrowseList, out: &mut [FileRow]) -> usize {
    let items = bl.items();
    let m = items.len().min(out.len());
    for k in 0..m {
        out[k] = FileRow {
            name_len: items[k].name.len(),
            y: k as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            opacity_pct: items[k].opacity_pct,
            badge_hidden: items[k].badge_hidden,
        };
    }
    m
}

/// 行命中测试（列表内坐标；x ∈ [0, w) 且落在行内）。
pub fn browse_row_hit(rows: &[FileRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 键盘遍历结论：不动 / 移高亮（夹取）/ 激活当前行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListNav {
    Stay,
    Moved(usize),
    Activate(usize),
}

/// 键盘遍历：Up/Down 移高亮（首末行夹取）、Enter 激活。
pub fn browse_list_nav(hl: usize, n: usize, key: u8) -> ListNav {
    if n == 0 {
        return ListNav::Stay;
    }
    match key {
        VX2_KEY_UP => ListNav::Moved(hl.saturating_sub(1)),
        VX2_KEY_DOWN => ListNav::Moved((hl + 1).min(n - 1)),
        VX2_KEY_ENTER => ListNav::Activate(hl.min(n - 1)),
        _ => ListNav::Stay,
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// F243 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_hidfiles_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F243-hidfiles-v2");

    // 1. 持久化 round-trip：状态机→册→编→解→推新状态机→双开关一致。
    let mut src = FileVisibility::new();
    let _ = src.toggle_hidden(10);
    let _ = src.request_show_protected(20);
    let _ = src.confirm_protect(30);
    let book = ViewPrefsBook::from_vis(&src);
    let mut buf = [0u8; VX2_HF_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = FileVisibility::new();
    match ViewPrefsBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let ok = b2 == book && b2.apply_to(&mut dst, 40);
            set.add(
                "v2 persistence round-trip",
                ok && dst.show_hidden() && dst.show_protected() && !dst.dialog_open(),
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 2;
    let mut c = buf;
    c[6] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        ViewPrefsBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && ViewPrefsBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && ViewPrefsBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && ViewPrefsBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 浏览行清单：不透明度契约（隐藏可见=60、普通=100）+ 行距铺排
    //    + 命中测试（界内命中、界外不命中）。
    let mut vis = FileVisibility::new();
    let _ = vis.toggle_hidden(1);
    let mut bl = BrowseList::new();
    let batch: [(&[u8], u8); 3] =
        [(b"readme.txt", 0), (b".env", 0), (b"notes.md", 0)];
    let _ = bl.scan_batch(&batch, &vis, BrowseFilter::VisibleOnly);
    let mut rows = [FileRow { name_len: 0, y: 0, h: 0, opacity_pct: 0, badge_hidden: false }; 4];
    let rn = browse_rows(&bl, &mut rows);
    set.add(
        "v2 browse rows opacity & hit",
        rn == 3
            && rows[1].opacity_pct == SEMI_OPACITY_PCT && rows[1].badge_hidden
            && rows[0].opacity_pct == FULL_OPACITY_PCT && !rows[0].badge_hidden
            && rows[2].y == 2 * VX2_ROW_H_PX
            && browse_row_hit(&rows, rn, 50, VX2_ROW_H_PX + 3, 400) == Some(1)
            && browse_row_hit(&rows, rn, 50, -1, 400).is_none()
            && browse_row_hit(&rows, rn, 400, 3, 400).is_none(),
        "",
    );

    // 4. 键盘遍历：Down 步进夹取底行、Up 回顶、Enter 激活。
    let mut hl = 0usize;
    for _ in 0..5 {
        if let ListNav::Moved(k) = browse_list_nav(hl, 3, VX2_KEY_DOWN) {
            hl = k;
        }
    }
    let up_top = matches!(browse_list_nav(0, 3, VX2_KEY_UP), ListNav::Moved(0));
    set.add(
        "v2 list keyboard nav clamped",
        hl == 2 && up_top && browse_list_nav(2, 3, VX2_KEY_ENTER) == ListNav::Activate(2),
        "",
    );

    // 5. xors32 fuzz 500 轮：随机开关流读出→编→解→推新状态机逐位一致、
    //    payload 任一字节翻位必被校验和捕获。
    let mut x: u32 = 0x2433_77AA;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut visf = FileVisibility::new();
        match x % 4 {
            0 => {
                let _ = visf.toggle_hidden(1);
            }
            1 => {
                let _ = visf.request_show_protected(1);
                let _ = visf.confirm_protect(2);
            }
            2 => {
                let _ = visf.toggle_hidden(1);
                let _ = visf.request_show_protected(2);
                let _ = visf.cancel_protect(3);
            }
            _ => {}
        }
        let b = ViewPrefsBook::from_vis(&visf);
        let mut tbuf = [0u8; VX2_HF_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_HF_BLOB && ViewPrefsBook::from_bytes(&tbuf) == Ok(b);
        let mut visg = FileVisibility::new();
        ok &= b.apply_to(&mut visg, 5)
            && visg.show_hidden() == b.show_hidden
            && visg.show_protected() == b.show_protected;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_HF_PAYLOAD] ^= 0x10;
        ok &= ViewPrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
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
    fn v2_prefs_roundtrip_and_reject() {
        let mut vis = FileVisibility::new();
        let _ = vis.toggle_hidden(1);
        let b = ViewPrefsBook::from_vis(&vis);
        let mut buf = [0u8; VX2_HF_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_HF_BLOB);
        assert_eq!(ViewPrefsBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[7] ^= 0x01;
        assert_eq!(ViewPrefsBook::from_bytes(&bad), Err(Vx2Error::BadChecksum));
        assert_eq!(ViewPrefsBook::from_bytes(&buf[..5]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_rows_never_overlap() {
        let mut vis = FileVisibility::new();
        let _ = vis.toggle_hidden(1);
        let mut bl = BrowseList::new();
        let batch: [(&[u8], u8); 4] =
            [(b"a", 0), (b".b", 0), (b"c", 0), (b"d", 0)];
        let _ = bl.scan_batch(&batch, &vis, BrowseFilter::VisibleOnly);
        let mut rows = [FileRow { name_len: 0, y: 0, h: 0, opacity_pct: 0, badge_hidden: false }; 4];
        let n = browse_rows(&bl, &mut rows);
        for k in 1..n {
            assert!(rows[k].y >= rows[k - 1].y + rows[k - 1].h, "行矩形不得重叠");
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_hidfiles_v2_checks();
        assert!(set.all_passed(), "F243 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
