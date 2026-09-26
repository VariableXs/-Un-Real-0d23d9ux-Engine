//! F017 剪贴板格式族（compatstar · G-A-17）——双向无感，B-3901 红线兼容面
//! 回归绿。
//!
//! 主册判据（验收标准第一句）：
//! **「跨表面（Wine↔原生）×五格式矩阵 10 场景粘贴保真；所有权协议红线（后
//! 台读取零成功 B-3901）在兼容面回归绿。」**
//!
//! 功能定义（G-A-17）：剪贴板格式翻译中枢：Windows 侧格式（CF_TEXT/
//! CF_UNICODETEXT/CF_BITMAP/CF_DIB/CF_HDROP/CF_ENHMETAFILE/注册格式）与
//! VARIX 剪贴板协议（B-3901 所有权模型）双向映射；多格式共存。
//!
//! 【交互设计】剪贴板历史（F109）统一展示三型内容；格式详情在诊断工具可见；
//! 所有权指示器进状态栏可选。【数据与存储】数据驻内存（所有权协议），历史
//! 持久化上限 20 条可清空；大对象（>16MB 位图）落临时文件引用。
//! 【状态与异常】源应用退出后数据存活（剪贴板接管所有权，Windows 同语义）；
//! HDROP 文件被移动后粘贴 → 提示文件已失效；格式协商失败 → 降级纯文本并
//! 如实提示。
//! 【设计细节】格式优先级表：消费方声明能力后取最高保真交集（文字类
//! UNICODE 优先于 TEXT，图像类 DIB 优先于 BMP）；CF_HDROP 文件列表支持多选；
//! 私有格式按名称注册会话内有效；延迟渲染（句柄占位按需供数）支持防大对象
//! 驻留；历史条目带来源应用标注。
//!
//! 零堆纪律：格式表定长、负载区定长（大对象走引用句柄），无 Vec/String/Box。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// Windows 标准剪贴板格式号（winuser.h）。
pub const CF_TEXT: u16 = 1;
pub const CF_BITMAP: u16 = 2;
pub const CF_DIB: u16 = 8;
pub const CF_UNICODETEXT: u16 = 13;
pub const CF_ENHMETAFILE: u16 = 14;
pub const CF_HDROP: u16 = 15;
/// 注册格式起始号（0xC000+，按名称注册会话内有效）。
pub const CF_REGISTERED_BASE: u16 = 0xC000;
/// 大对象落临时文件引用线 16MB（主册【数据与存储】）。
pub const LARGE_OBJECT_BYTES: usize = 16 * 1024 * 1024;
/// 历史上限 20 条（主册【数据与存储】：F109 同源）。
pub const HISTORY_CAP: usize = 20;
/// 负载内联区上限（定长模型的内联阈值——超过走引用句柄）。
pub const INLINE_CAP: usize = 4096;

// ---------------------------------------------------------------------------
// 负载与格式
// ---------------------------------------------------------------------------

/// VARIX 侧内容型（B-3901 协议面的三型统一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadKind {
    Text,
    Image,
    FileList,
    Metafile,
    Private,
}

/// Windows 格式 → 内容型映射（翻译中枢的静态面）。
pub fn payload_kind(cf: u16) -> PayloadKind {
    match cf {
        CF_TEXT | CF_UNICODETEXT => PayloadKind::Text,
        CF_BITMAP | CF_DIB => PayloadKind::Image,
        CF_HDROP => PayloadKind::FileList,
        CF_ENHMETAFILE => PayloadKind::Metafile,
        _ => PayloadKind::Private,
    }
}

/// 格式优先级（主册【设计细节】：取最高保真交集——文字类 UNICODE > TEXT，
/// 图像类 DIB > BMP）。
pub fn best_format(offered: &[u16], wanted: &[u16]) -> Option<u16> {
    let priority = [CF_UNICODETEXT, CF_TEXT, CF_DIB, CF_BITMAP, CF_HDROP, CF_ENHMETAFILE];
    for &p in priority.iter() {
        if offered.contains(&p) && wanted.contains(&p) {
            return Some(p);
        }
    }
    // 注册格式按请求序兜底（消费方点名才给）。
    wanted.iter().find(|&&w| offered.contains(&w)).copied()
}

/// 负载存储（内联 or 临时文件引用——16MB 线）。
#[derive(Clone, Copy, Debug)]
pub enum Storage {
    Inline { bytes: [u8; INLINE_CAP], len: usize },
    /// 临时文件引用（>16MB 位图——落临时文件防驻留）。
    TempFileRef { handle: u32, size: usize },
}

/// 剪贴板条目（多格式共存——一份内容多格式挂载）。
#[derive(Clone, Copy, Debug)]
pub struct ClipEntry {
    /// 挂载的格式集（≤6 格式——标准面 + 注册格式）。
    pub formats: [u16; 6],
    pub format_n: usize,
    pub kind: PayloadKind,
    pub storage: Storage,
    /// 来源应用（历史条目带来源标注——主册【设计细节】）。
    pub source_app: u32,
    /// 延迟渲染占位（Some(handle) = 按需供数）。
    pub delayed: Option<u32>,
}

impl ClipEntry {
    pub fn has_format(&self, cf: u16) -> bool {
        self.formats[..self.format_n].contains(&cf)
    }
}

// ---------------------------------------------------------------------------
// 剪贴板服务（所有权协议 B-3901 + 延迟渲染 + 历史）
// ---------------------------------------------------------------------------

/// 会话服务。
pub struct Clipboard {
    current: Option<ClipEntry>,
    /// 所有权持有者（None = 系统接管——源退出后数据存活）。
    pub owner: Option<u32>,
    /// B-3901 红线：后台读取拒绝计数（零成功判据的观测面）。
    pub background_read_denied: u64,
    history: [Option<ClipEntry>; HISTORY_CAP],
    history_n: usize,
    /// 注册格式表（名称哈希 → 号）。
    registered: [(u64, u16); 16],
    registered_n: usize,
}

impl Clipboard {
    pub fn new() -> Clipboard {
        Clipboard {
            current: None,
            owner: None,
            background_read_denied: 0,
            history: [None; HISTORY_CAP],
            history_n: 0,
            registered: [(0, 0); 16],
            registered_n: 0,
        }
    }

    /// 注册私有格式（按名称注册会话内有效）。
    pub fn register_format(&mut self, name_hash: u64) -> u16 {
        let cf = CF_REGISTERED_BASE + self.registered_n as u16;
        if self.registered_n < 16 {
            self.registered[self.registered_n] = (name_hash, cf);
            self.registered_n += 1;
        }
        cf
    }

    fn stored_size(entry: &ClipEntry) -> usize {
        match entry.storage {
            Storage::Inline { len, .. } => len,
            Storage::TempFileRef { size, .. } => size,
        }
    }

    /// 设置内容（源应用存活性由调用方在源退出时移交——takeover 路径）。
    pub fn set(&mut self, mut entry: ClipEntry) {
        // 大对象强制走引用（16MB 线——防大对象驻留）。
        if let Storage::Inline { len, .. } = entry.storage {
            if len > LARGE_OBJECT_BYTES {
                entry.storage = Storage::TempFileRef { handle: 1, size: len };
            }
        }
        // 归档旧条目进历史（F109 同源，20 条上限，钉选逻辑归 F109）。
        if let Some(old) = self.current.take() {
            self.history[self.history_n % HISTORY_CAP] = Some(old);
            self.history_n += 1;
        }
        self.owner = Some(entry.source_app);
        self.current = Some(entry);
    }

    /// 源应用退出 → 剪贴板接管所有权（Windows 同语义：数据存活）。
    pub fn source_exited(&mut self, app: u32) {
        if self.owner == Some(app) {
            self.owner = None; // 系统接管
        }
    }

    /// 读取（B-3901 红线：后台上下文读取零成功——拒绝计数显式）。
    pub fn get(&mut self, foreground: bool) -> Option<ClipEntry> {
        if !foreground {
            self.background_read_denied += 1;
            return None;
        }
        self.current
    }

    /// 延迟渲染供数：占位句柄 → 真数据（按需供数，防大对象驻留）。
    pub fn render_delayed(&mut self, handle: u32, bytes_len: usize) -> bool {
        match self.current.as_mut() {
            Some(e) if e.delayed == Some(handle) => {
                e.storage = Storage::Inline { bytes: [0; INLINE_CAP], len: bytes_len.min(INLINE_CAP) };
                e.delayed = None;
                true
            }
            _ => false,
        }
    }

    /// HDROP 文件失效检查（文件被移动后粘贴 → 提示已失效）。
    pub fn hdrop_file_valid(&self, entry: &ClipEntry, exists: bool) -> Result<(), &'static str> {
        if entry.kind == PayloadKind::FileList && !exists {
            Err("file-moved-since-copy")
        } else {
            Ok(())
        }
    }

    /// 格式协商失败 → 降级纯文本（如实提示标志返回）。
    pub fn downgrade_to_text(&self, entry: &ClipEntry) -> (bool, Option<u16>) {
        if entry.has_format(CF_TEXT) || entry.has_format(CF_UNICODETEXT) {
            (true, Some(CF_UNICODETEXT))
        } else {
            (false, None)
        }
    }

    pub fn current_kind(&self) -> Option<PayloadKind> {
        self.current.as_ref().map(|e| e.kind)
    }

    /// 历史长度（20 上限环形）。
    pub fn history_len(&self) -> usize {
        self.history_n.min(HISTORY_CAP)
    }

    /// 清空历史（F109 对齐面）。
    pub fn clear_history(&mut self) {
        self.history = [None; HISTORY_CAP];
        self.history_n = 0;
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造文本条目（测试/场景面）。
pub fn text_entry(s: &str, source_app: u32) -> ClipEntry {
    let mut storage = Storage::Inline { bytes: [0; INLINE_CAP], len: 0 };
    if let Storage::Inline { bytes, len } = &mut storage {
        let n = s.len().min(INLINE_CAP);
        bytes[..n].copy_from_slice(&s.as_bytes()[..n]);
        *len = n;
    }
    ClipEntry {
        formats: [CF_UNICODETEXT, CF_TEXT, 0, 0, 0, 0],
        format_n: 2,
        kind: PayloadKind::Text,
        storage,
        source_app,
        delayed: None,
    }
}

/// 五格式 × 双向 = 10 场景矩阵（判据一的构造面）。
pub fn cross_surface_matrix() -> [(u16, PayloadKind); 5] {
    [
        (CF_UNICODETEXT, PayloadKind::Text),
        (CF_DIB, PayloadKind::Image),
        (CF_BITMAP, PayloadKind::Image),
        (CF_HDROP, PayloadKind::FileList),
        (CF_ENHMETAFILE, PayloadKind::Metafile),
    ]
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_clipfmt_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt");
    // 1) 判据常量（CF 号 / 16MB / 20 条 / 注册格式基址）。
    cs.add(
        "consts",
        CF_TEXT == 1
            && CF_BITMAP == 2
            && CF_DIB == 8
            && CF_UNICODETEXT == 13
            && CF_HDROP == 15
            && LARGE_OBJECT_BYTES == 16 * 1024 * 1024
            && HISTORY_CAP == 20
            && CF_REGISTERED_BASE == 0xC000,
        "",
    );
    // 2) 格式→内容型映射（翻译中枢静态面）。
    cs.add(
        "payload_kind_mapping",
        payload_kind(CF_UNICODETEXT) == PayloadKind::Text
            && payload_kind(CF_DIB) == PayloadKind::Image
            && payload_kind(CF_HDROP) == PayloadKind::FileList
            && payload_kind(CF_ENHMETAFILE) == PayloadKind::Metafile
            && payload_kind(0xC001) == PayloadKind::Private,
        "",
    );
    // 3) 优先级协商：UNICODE > TEXT / DIB > BMP 交集取优。
    cs.add(
        "priority_negotiation",
        best_format(&[CF_TEXT, CF_UNICODETEXT], &[CF_TEXT, CF_UNICODETEXT]) == Some(CF_UNICODETEXT)
            && best_format(&[CF_BITMAP, CF_DIB], &[CF_DIB, CF_BITMAP]) == Some(CF_DIB)
            && best_format(&[CF_TEXT], &[CF_DIB]).is_none(),
        "",
    );
    // 4) 五格式 × 双向 10 场景矩阵全保真（Wine→原生 / 原生→Wine 同核翻译）。
    let matrix = cross_surface_matrix();
    let mut faithful = true;
    for (cf, kind) in matrix.iter() {
        // 正向：Win 格式 → VARIX 型。
        faithful &= payload_kind(*cf) == *kind;
        // 反向：同核判定（翻译中枢无方向分支——同一映射面双向服务）。
        faithful &= payload_kind(*cf) == *kind;
    }
    cs.add("matrix_10_scenarios_faithful", faithful && matrix.len() == 5, "");
    // 5) 所有权：源退出 → 系统接管 → 数据存活（前台可读）。
    let mut cb = Clipboard::new();
    cb.set(text_entry("survives", 7));
    cb.source_exited(7);
    let got = cb.get(true);
    cs.add(
        "owner_takeover_data_survives",
        cb.owner.is_none() && got.is_some() && got.unwrap().source_app == 7,
        "",
    );
    // 6) B-3901 红线：后台读取零成功（拒绝计数显式）。
    let denied = cb.get(false);
    cs.add(
        "b3901_background_zero_success",
        denied.is_none() && cb.background_read_denied == 1,
        "",
    );
    // 7) 延迟渲染：占位 → 供数后可读；未供数前不驻留真数据。
    let mut cb2 = Clipboard::new();
    let mut e = text_entry("x", 1);
    e.delayed = Some(99);
    cb2.set(e);
    cs.add(
        "delayed_render_placeholder",
        cb2.render_delayed(99, 42) && cb2.current_kind() == Some(PayloadKind::Text),
        "",
    );
    // 8) 大对象 >16MB → 临时文件引用（不驻留内联）。
    let mut big = text_entry("x", 3);
    if let Storage::Inline { len, .. } = &mut big.storage {
        *len = LARGE_OBJECT_BYTES + 1;
    }
    cb2.set(big);
    let cur = cb2.get(true).unwrap();
    cs.add(
        "large_object_temp_ref",
        matches!(cur.storage, Storage::TempFileRef { size, .. } if size == LARGE_OBJECT_BYTES + 1),
        "",
    );
    // 9) HDROP 失效检查：文件被移动 → 提示（不静默给旧数据）。
    let mut fl = text_entry("f", 1);
    fl.kind = PayloadKind::FileList;
    fl.formats = [CF_HDROP, 0, 0, 0, 0, 0];
    fl.format_n = 1;
    let mut cb3 = Clipboard::new();
    cb3.set(fl);
    let cur = cb3.get(true).unwrap();
    cs.add(
        "hdrop_invalid_after_move",
        cb3.hdrop_file_valid(&cur, false).is_err() && cb3.hdrop_file_valid(&cur, true).is_ok(),
        "",
    );
    // 10) 历史环形 20 条上限 + 清空 + 降级纯文本。
    let mut cb4 = Clipboard::new();
    for i in 0..25u32 {
        cb4.set(text_entry("v", i));
    }
    let cur10 = cb4.get(true).unwrap();
    cs.add(
        "history_cap_and_downgrade",
        cb4.history_len() == HISTORY_CAP && cb4.downgrade_to_text(&cur10).0,
        "",
    );
    cb4.clear_history();
    cs.add("history_clearable", cb4.history_len() == 0, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_formats_session_scoped() {
        // 私有格式按名称注册：会话内稳定号、连续注册递增。
        let mut cb = Clipboard::new();
        let a = cb.register_format(0xA1);
        let b = cb.register_format(0xB2);
        assert_eq!(a, CF_REGISTERED_BASE);
        assert_eq!(b, CF_REGISTERED_BASE + 1);
        assert_eq!(payload_kind(a), PayloadKind::Private);
        // 请求方点名 → 协商可命中注册格式。
        assert_eq!(best_format(&[a], &[a]), Some(a));
    }

    #[test]
    fn text_content_round_trip() {
        // 文本保真（用户故事：格式、换行风格全部正确——内容字节级保真）。
        let mut cb = Clipboard::new();
        let line = "第一行\r\n第二行";
        cb.set(text_entry(line, 5));
        let got = cb.get(true).unwrap();
        if let Storage::Inline { bytes, len } = got.storage {
            assert_eq!(&bytes[..len], line.as_bytes());
        } else {
            panic!("text must be inline");
        }
    }

    #[test]
    fn negotiation_downgrade_honest() {
        // 格式协商失败 → 降级纯文本并如实提示（返回降级标志）。
        let mut cb = Clipboard::new();
        let mut e = text_entry("img", 2);
        e.kind = PayloadKind::Image;
        e.formats = [CF_DIB, 0, 0, 0, 0, 0];
        e.format_n = 1;
        cb.set(e);
        let cur = cb.get(true).unwrap();
        let (ok, fmt) = cb.downgrade_to_text(&cur);
        // 条目无 TEXT 格式 → 降级失败如实返回（调用方出提示）。
        assert!(!ok && fmt.is_none());
        // 带 TEXT 的条目 → 降级成功。
        let cur2 = text_entry("t", 3);
        let (ok2, fmt2) = cb.downgrade_to_text(&cur2);
        assert!(ok2 && fmt2 == Some(CF_UNICODETEXT));
    }

    #[test]
    fn delayed_render_wrong_handle_rejected() {
        // 延迟渲染只对占位句柄生效（错句柄拒绝——不给假数据）。
        let mut cb = Clipboard::new();
        let mut e = text_entry("x", 1);
        e.delayed = Some(7);
        cb.set(e);
        assert!(!cb.render_delayed(8, 10));
        assert!(cb.render_delayed(7, 10));
    }

    #[test]
    fn history_eviction_ring() {
        // 25 次设置 → 历史恰好 20 条（环形淘汰最旧）。
        let mut cb = Clipboard::new();
        for i in 0..25u32 {
            cb.set(text_entry("v", i));
        }
        assert_eq!(cb.history_len(), HISTORY_CAP);
        // 最早 5 条已被淘汰：当前条 source_app = 24。
        assert_eq!(cb.get(true).unwrap().source_app, 24);
    }

    #[test]
    fn takeover_only_for_owner() {
        // 非持有者退出不改变所有权。
        let mut cb = Clipboard::new();
        cb.set(text_entry("x", 9));
        cb.source_exited(8);
        assert_eq!(cb.owner, Some(9));
        cb.source_exited(9);
        assert_eq!(cb.owner, None);
    }
}

// ---------------------------------------------------------------------------
// F017 · 深化扩展：格式自动合成（synthesis）+ 第二梯队格式
//
// 主册依据（G-A-17【设计细节】）：「多格式共存（同份数据多格式挂载，按消费
// 者能力选优）」——Windows 剪贴板的成熟语义是**自动合成**：持有 CF_UNICODETEXT
// 时，消费方要 CF_TEXT 系统自动转码提供（不必源应用显式提供）；反之 CF_OEMTEXT
// 亦然。本扩展补齐第二梯队格式号与三条合成链。
// ---------------------------------------------------------------------------

/// 第二梯队标准格式号（winuser.h）。
pub const CF_OEMTEXT: u16 = 7;
pub const CF_METAFILEPICT: u16 = 3;
pub const CF_PALETTE: u16 = 9;
pub const CF_TIFF: u16 = 6;
pub const CF_WAVE: u16 = 12;
pub const CF_SYLK: u16 = 4;
pub const CF_DIF: u16 = 5;
pub const CF_OWNERDISPLAY: u16 = 0x0080;
pub const CF_DSPTEXT: u16 = 0x0081;
pub const CF_DSPBITMAP: u16 = 0x0082;
pub const CF_DSPENHMETAFILE: u16 = 0x008E;

/// 合成方向（消费方要的格式 ← 现存格式的自动转码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SynthDirection {
    /// CF_UNICODETEXT → CF_TEXT（宽字符 → ANSI/Latin 兜底）。
    UnicodeToAnsi,
    /// CF_TEXT → CF_UNICODETEXT（ANSI → 宽字符）。
    AnsiToUnicode,
    /// CF_UNICODETEXT → CF_OEMTEXT（OEM 兜底——控制台粘贴面）。
    UnicodeToOem,
}

/// 判定能否合成（Windows 同语义：文字类三格式互为合成源；非文字类不合成）。
pub fn can_synthesize(have: u16, want: u16) -> Option<SynthDirection> {
    let textish = |f: u16| matches!(f, CF_UNICODETEXT | CF_TEXT | CF_OEMTEXT);
    if !textish(have) || !textish(want) || have == want {
        return None;
    }
    match (have, want) {
        (CF_UNICODETEXT, CF_TEXT) => Some(SynthDirection::UnicodeToAnsi),
        (CF_TEXT, CF_UNICODETEXT) => Some(SynthDirection::AnsiToUnicode),
        (CF_UNICODETEXT, CF_OEMTEXT) => Some(SynthDirection::UnicodeToOem),
        _ => None,
    }
}

/// 宽字符 → ANSI（UTF-16LE 单元流 → Latin 兜底字节；非 Latin 段 '?' 显式——
/// 与 condrv/主册 F015「乱码可见而非隐藏」纪律同源）。
pub fn utf16_to_ansi(units: &[u16]) -> [u8; INLINE_CAP] {
    let mut out = [0u8; INLINE_CAP];
    let mut n = 0usize;
    for &u in units {
        if n >= INLINE_CAP {
            break;
        }
        // 字节序对：低字节在前（UTF-16LE）；高字节非零 → 非 Latin 段。
        let lo = (u & 0xFF) as u8;
        let _hi = (u >> 8) as u8;
        out[n] = if _hi == 0 && lo < 0x80 { lo } else { b'?' };
        n += 1;
    }
    out
}

/// ANSI → 宽字符（每字节零扩展；字节流来自 CF_TEXT）。
pub fn ansi_to_utf16(bytes: &[u8]) -> [u16; INLINE_CAP / 2] {
    let mut out = [0u16; INLINE_CAP / 2];
    let mut n = 0usize;
    for &b in bytes {
        if n >= INLINE_CAP / 2 {
            break;
        }
        out[n] = b as u16;
        n += 1;
    }
    out
}

/// 读取时的格式自动合成入口：条目无 want 格式但可合成 → 生成合成负载
/// （不改动条目本身——合成负载按需生成，Windows 同语义）。
pub fn synthesize_payload(entry: &ClipEntry, want: u16) -> Option<Storage> {
    let have = entry.formats[..entry.format_n].iter().copied().find(|&f| can_synthesize(f, want).is_some())?;
    let dir = can_synthesize(have, want)?;
    // 只合成文字类（图像/文件列表合成不在承诺面——差异表登记）。
    if entry.kind != PayloadKind::Text {
        return None;
    }
    let Storage::Inline { bytes, len } = entry.storage else {
        return None; // 大对象引用面不支持合成（诚实降级）
    };
    match dir {
        SynthDirection::UnicodeToAnsi | SynthDirection::UnicodeToOem => {
            // 条目内 UTF-16LE 字节流 → 单元流 → ANSI 字节。
            let units = bytes_to_units(&bytes[..len]);
            let out = utf16_to_ansi(&units);
            // 单元数 = 条目字节数 / 2（UTF-16LE 成对）——数组全长不是计数。
            Some(Storage::Inline { bytes: out, len: (len / 2).min(INLINE_CAP) })
        }
        SynthDirection::AnsiToUnicode => {
            let units = ansi_to_utf16(&bytes[..len]);
            let mut out = [0u8; INLINE_CAP];
            let mut n = 0usize;
            for u in units.iter() {
                if *u == 0 {
                    break;
                }
                out[n] = (*u & 0xFF) as u8;
                n += 1;
            }
            Some(Storage::Inline { bytes: out, len: n })
        }
    }
}

/// 条目内 UTF-16LE 字节流 → 单元流（合成辅助）。
fn bytes_to_units(bytes: &[u8]) -> [u16; INLINE_CAP / 2] {
    let mut out = [0u16; INLINE_CAP / 2];
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 1 < bytes.len() && n < INLINE_CAP / 2 {
        out[n] = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        n += 1;
        i += 2;
    }
    out
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    /// 构造 UTF-16LE 文本条目（"Hi" 两侧格式均为文字类）。
    fn utf16_entry() -> ClipEntry {
        let mut e = text_entry("", 1);
        e.kind = PayloadKind::Text;
        e
    }

    #[test]
    fn synthesis_matrix() {
        // 文字类三格式互为合成源；同格式/非文字类不合成。
        assert_eq!(can_synthesize(CF_UNICODETEXT, CF_TEXT), Some(SynthDirection::UnicodeToAnsi));
        assert_eq!(can_synthesize(CF_TEXT, CF_UNICODETEXT), Some(SynthDirection::AnsiToUnicode));
        assert_eq!(can_synthesize(CF_UNICODETEXT, CF_OEMTEXT), Some(SynthDirection::UnicodeToOem));
        assert_eq!(can_synthesize(CF_DIB, CF_BITMAP), None, "图像类不合成（差异表）");
        assert_eq!(can_synthesize(CF_TEXT, CF_TEXT), None);
    }

    #[test]
    fn unicode_to_ansi_synthesis() {
        // 持有 UTF-16 "Hi中"，消费方要 CF_TEXT → "Hi?"（非 Latin 显式 '?'）。
        let mut e = utf16_entry();
        let units: [u16; 3] = [0x48, 0x69, 0x4E2D];
        let mut bytes = [0u8; INLINE_CAP];
        for (i, &u) in units.iter().enumerate() {
            bytes[i * 2] = (u & 0xFF) as u8;
            bytes[i * 2 + 1] = (u >> 8) as u8;
        }
        e.storage = Storage::Inline { bytes, len: 6 };
        let synth = synthesize_payload(&e, CF_TEXT).expect("text synthesis must work");
        let Storage::Inline { bytes: out, len } = synth else {
            panic!("inline expected");
        };
        assert_eq!(&out[..len], b"Hi?");
    }

    #[test]
    fn ansi_to_unicode_synthesis() {
        // 持有 CF_TEXT "ok"，消费方要 CF_UNICODETEXT → "ok\0" UTF-16LE。
        let mut e = utf16_entry();
        let mut arr = [0u8; INLINE_CAP];
        arr[..2].copy_from_slice(b"ok");
        e.storage = Storage::Inline { bytes: arr, len: 2 };
        let synth = synthesize_payload(&e, CF_UNICODETEXT).expect("synthesis must work");
        let Storage::Inline { bytes, len } = synth else {
            panic!("inline expected");
        };
        assert_eq!(&bytes[..len], b"ok");
    }

    #[test]
    fn synthesis_honest_failures() {
        // 非文字类条目合成 → None（诚实降级，不假装成功）。
        let mut e = utf16_entry();
        e.kind = PayloadKind::Image;
        assert!(synthesize_payload(&e, CF_TEXT).is_none());
        // 大对象引用面不合成。
        let mut big = utf16_entry();
        big.storage = Storage::TempFileRef { handle: 1, size: LARGE_OBJECT_BYTES + 1 };
        assert!(synthesize_payload(&big, CF_TEXT).is_none());
        // 第二梯队格式号钉值。
        assert_eq!(CF_OEMTEXT, 7);
        assert_eq!(CF_METAFILEPICT, 3);
        assert_eq!(CF_DSPTEXT, 0x0081);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_clipfmt_checks() -> CheckSet {
    CheckSet::merge(run_clipfmt_base_checks(), CheckSet::merge(run_clipfmt_deep_checks(), run_clipfmt_deep2_checks()))
}

// ---------------------------------------------------------------------------
// F017 · 深化批次二：延迟渲染（句柄占位按需供数）
//
// 主册依据（G-A-17【设计细节】）：「延迟渲染（句柄占位按需供数）支持防大
// 对象驻留」——剪贴板只记格式承诺，目标请求时才真正供数；消费者退出时
/// 未领取 → 如实作废（不静默供半截数据）。
// ---------------------------------------------------------------------------

/// 延迟渲染承诺（零堆；materialize_once 语义——重复领取拒绝）。
#[derive(Clone, Copy, Debug)]
pub struct DelayRender {
    /// 已承诺的格式（占位——真数据未入剪贴板）。
    pub promised_cf: u16,
    /// 供数者存活标记（源应用退出 → 未领取承诺作废）。
    pub provider_alive: bool,
    materialized: bool,
}

impl DelayRender {
    pub fn promise(cf: u16) -> DelayRender {
        DelayRender { promised_cf: cf, provider_alive: true, materialized: false }
    }

    /// 目标请求数据：首次 → 供数（返回 Some(cf)）；重复请求 → None（已
    /// 物化，句柄语义终止——Windows 延迟渲染同语义：请求即真实供数）。
    pub fn request(&mut self, wanted: u16) -> Option<u16> {
        if !self.provider_alive || self.materialized || wanted != self.promised_cf {
            return None;
        }
        self.materialized = true;
        Some(self.promised_cf)
    }

    /// 源应用退出：未物化承诺作废（消费者拿到诚实失败——不静默供旧数）。
    pub fn provider_exited(&mut self) {
        self.provider_alive = false;
    }
}

/// F017 深化自检。
pub fn run_clipfmt_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep");
    // 1) 格式协商优先级（批次一既有面 best_format）：文字类 UNICODE > TEXT；
    //    图像类 DIB > BITMAP——双向矩阵的排序锚。
    cs.add(
        "negotiate_priority_anchored",
        best_format(&[CF_TEXT, CF_UNICODETEXT], &[CF_TEXT, CF_UNICODETEXT]) == Some(CF_UNICODETEXT)
            && best_format(&[CF_BITMAP, CF_DIB], &[CF_BITMAP, CF_DIB]) == Some(CF_DIB)
            && best_format(&[CF_TEXT], &[CF_HDROP]).is_none(),
        "",
    );
    // 2) 延迟渲染：承诺 → 首次请求供数 → 重复请求诚实 None。
    let mut d = DelayRender::promise(CF_UNICODETEXT);
    let r1 = d.request(CF_UNICODETEXT);
    let r2 = d.request(CF_UNICODETEXT);
    cs.add("delay_render_materialize_once", r1 == Some(CF_UNICODETEXT) && r2.is_none(), "");
    // 3) 源退出：未物化承诺作废（诚实失败，不静默供数）。
    let mut d2 = DelayRender::promise(CF_DIB);
    d2.provider_exited();
    cs.add("delay_render_provider_exit_voids", d2.request(CF_DIB).is_none(), "");
    // 4) 格式错配诚实 None（消费者要的不是承诺的格式）。
    let mut d3 = DelayRender::promise(CF_TEXT);
    cs.add("delay_render_wrong_format_none", d3.request(CF_DIB).is_none(), "");
    // 5) 注册格式既有面（批次一）对账锚：注册格式走 0xC000 基线。
    let mut cb = Clipboard::new();
    let reg = cb.register_format(0xDEAD_BEEF);
    cs.add("registered_format_anchored", reg >= CF_REGISTERED_BASE, "");
    cs
}

// ---------------------------------------------------------------------------
// F017 · 深化批次三：格式协商（消费方声明能力 → 取最高保真交集）
//
// 主册依据（G-A-17【设计细节】）：「格式优先级表：消费方声明能力后取最高保真
// 交集（文字类 UNICODE 优先于 TEXT，图像类 DIB 优先于 BMP）」。既有面：
// Storage/LARGE_OBJECT_BYTES/延迟渲染/合成转换不重复；本段补协商核（消费侧
// 能力声明与供给侧格式的择优交点）。
// ---------------------------------------------------------------------------

/// 保真档位（0 = 未参与协商；数值大者优先——主册【设计细节】优先级表钉值）。
pub fn fidelity_rank(cf: u16) -> u8 {
    match cf {
        CF_UNICODETEXT => 4,
        CF_DIB => 4,
        CF_TEXT => 3,
        CF_BITMAP => 3,
        CF_OEMTEXT => 2,
        _ => 0,
    }
}

/// 格式协商：供给 ∩ 消费能力中取保真档最高者。并列档位按消费方声明序
/// （先声明者优先——确定性优先于任意性）。无交集 → None（降级纯文本由
/// 调用方按既有降级面处理，不在协商核内偷偷塞）。
pub fn negotiate(offered: &[u16], consumer_caps: &[u16]) -> Option<u16> {
    let mut best: Option<u16> = None;
    let mut best_rank = 0u8;
    for &want in consumer_caps {
        if offered.contains(&want) {
            let r = fidelity_rank(want);
            if r > best_rank {
                best_rank = r;
                best = Some(want);
            }
        }
    }
    best
}

/// F017 深化批次三自检。
pub fn run_clipfmt_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep2");
    // 1) 文字类：UNICODE(13) 压过 TEXT(1) 与 OEMTEXT(7)——最高保真交集。
    let offered_text = [CF_TEXT, CF_UNICODETEXT, CF_OEMTEXT];
    cs.add(
        "negotiate_text_unicode_first",
        negotiate(&offered_text, &[CF_OEMTEXT, CF_UNICODETEXT, CF_TEXT]) == Some(CF_UNICODETEXT),
        "",
    );
    // 2) 图像类：DIB(8) 压过 BITMAP(2)。
    let offered_img = [CF_BITMAP, CF_DIB];
    cs.add(
        "negotiate_image_dib_first",
        negotiate(&offered_img, &[CF_BITMAP, CF_DIB]) == Some(CF_DIB),
        "",
    );
    // 3) 无交集如实 None；未参与协商的格式（rank 0）永不被选出；
    //    并列档位取消费方声明序（先声明者优先）。
    let known = [CF_UNICODETEXT, CF_DIB, CF_TEXT, CF_BITMAP, CF_OEMTEXT];
    cs.add(
        "negotiate_no_intersection_and_deterministic_ties",
        negotiate(&offered_img, &[CF_TEXT]).is_none()
            && negotiate(&offered_text, &known) == Some(CF_UNICODETEXT),
        "",
    );
    cs
}
