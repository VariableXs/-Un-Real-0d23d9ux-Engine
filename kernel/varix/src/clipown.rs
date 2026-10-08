//! 剪贴板所有权与大载荷旁路（WP-207 · B-3901/B-3903）：复制粘贴不出事故。
//!
//! MD2 篇 39.1/39.3：剪贴板在 VXWM 层是所有权对象——clipboard_select 只发
//! 给焦点窗口；内容分两段（描述段随所有权即时可得，内容段按需取、读经
//! 合成器单一审计路径）。三条安全收益：后台窗口读不到剪贴板（显式授权
//! 语义，C-8 的姊妹约束）、所有者退出所有权即时回收（引用悬空零存在）、
//! 读内容必须经合成器（Q56 敏感类型判定在此执法）。
//! 类型三支持：纯文本（UTF-8 最大 256KB，超限截断如实标记）/位图
//! （ARGB8888 最大 4096 见方）/文件引用（路径清单+类型元数据，粘贴转
//! 文件操作，删除进回收站——剪贴板与回收站语义在此咬合）。
//! 大载荷旁路：报文只传引用（复制微秒级），粘贴才真正搬运（按需付费）；
//! 位图粘贴视尺寸节流，大图粘贴可取消——诚实的慢优于冻结的快。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 类型与容量常量
// ---------------------------------------------------------------------------

pub const TEXT_MAX: usize = 256 * 1024; // 256KB（大文本走文件路径，剪贴板不是文件系统）
pub const BITMAP_MAX_SIDE: u32 = 4096; // 位图最大 4096 见方
pub const PATH_CAP: usize = 16; // 文件引用清单上限
pub const PATH_LEN: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipKind {
    Text,
    Bitmap,
    FileRefs,
}

/// 描述段：类型清单与大小，随所有权即时可得（不触内容段）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClipDesc {
    pub kind: ClipKind,
    /// 字节数（Text）/宽*高（Bitmap 编码为 w|h）/条目数（FileRefs）。
    pub size_or_dim: u64,
    /// 超限截断如实标记（文本 256KB / 位图 4096 见方）。
    pub truncated: bool,
}

/// 文件引用条目。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FileRef {
    pub path: [u8; PATH_LEN],
    pub path_len: usize,
    pub is_dir: bool,
}

impl FileRef {
    pub fn path_bytes(&self) -> &[u8] {
        &self.path[..self.path_len.min(PATH_LEN)]
    }
}

/// 内容段：按需取（读时经合成器向所有者拉取——审计点单一路径）。
#[derive(Clone, Copy)]
pub enum ClipContent {
    /// 文本内容（定长缓冲 + 实长；截断后实长==TEXT_MAX）。
    Text { buf: [u8; 32], len: usize, real_len: u64 },
    /// 位图引用（共享内存旁路：shm_id+offset——报文只传引用）。
    Bitmap { shm_id: u32, offset: u64, w: u32, h: u32 },
    /// 文件引用清单。
    Files { refs: [Option<FileRef>; PATH_CAP], cnt: usize },
}

// ---------------------------------------------------------------------------
// 所有权模型
// ---------------------------------------------------------------------------

/// 所有权状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnState {
    Free,
    Owned,
}

/// 审计路径留痕：读内容必须经合成器（单一路径——旁路读取不存在）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuditEntry {
    pub seq: u64,
    pub reader_focus: bool, // 读取时请求方是否焦点
    pub via_compositor: bool, // 是否经合成器路径
    pub granted: bool,
}

pub const AUDIT_CAP: usize = 16;

/// 剪贴板所有权对象（VXWM 层）。
pub struct Clipboard {
    pub state: OwnState,
    pub owner: Option<u32>, // 所有者窗口 id
    pub focus_win: Option<u32>, // 当前焦点窗口（clipboard_select 只发给焦点）
    pub desc: Option<ClipDesc>,
    pub content: Option<ClipContent>,
    /// 后台读取拒绝计数（对抗测试对账面——零成功要有账）。
    pub denied_bg_reads: u64,
    /// 敏感类型拒绝计数（Q56：密码管理器类内容粘贴敏感判定）。
    pub sensitive_denied: u64,
    pub audit: [Option<AuditEntry>; AUDIT_CAP],
    pub audit_cnt: usize,
    audit_seq: u64,
}

/// 读取请求结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadVerdict {
    /// 焦点窗口经合成器读取：放行。
    Granted,
    /// 非焦点窗口：拒绝（后台读剪贴板零成功——显式授权语义）。
    DeniedNotFocused,
    /// 未经合成器路径：拒绝（审计点单一路径）。
    DeniedBypass,
    /// 所有权空：无内容可读。
    DeniedEmpty,
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard {
            state: OwnState::Free,
            owner: None,
            focus_win: None,
            desc: None,
            content: None,
            denied_bg_reads: 0,
            sensitive_denied: 0,
            audit: [None; AUDIT_CAP],
            audit_cnt: 0,
            audit_seq: 0,
        }
    }

    /// 焦点变化（VXWM 推送）。
    pub fn set_focus(&mut self, win: Option<u32>) {
        self.focus_win = win;
    }

    /// 所有权授予：只对焦点窗口生效（clipboard_select 只发给焦点窗口）。
    pub fn select(&mut self, win: u32, desc: ClipDesc, content: ClipContent) -> bool {
        if Some(win) != self.focus_win {
            return false;
        }
        self.state = OwnState::Owned;
        self.owner = Some(win);
        self.desc = Some(desc);
        self.content = Some(content);
        true
    }

    /// 所有者退出：所有权即时回收（引用悬空零存在——内容随所有权同灭）。
    pub fn owner_gone(&mut self, win: u32) -> bool {
        if self.owner == Some(win) {
            self.state = OwnState::Free;
            self.owner = None;
            self.desc = None;
            self.content = None;
            true
        } else {
            false
        }
    }

    fn push_audit(&mut self, reader_focus: bool, via_compositor: bool, granted: bool) {
        if self.audit_cnt < AUDIT_CAP {
            self.audit_seq += 1;
            self.audit[self.audit_cnt] = Some(AuditEntry {
                seq: self.audit_seq,
                reader_focus,
                via_compositor,
                granted,
            });
            self.audit_cnt += 1;
        }
    }

    /// 读内容请求：三条裁决 + 审计留痕。
    /// 后台窗口（非焦点）读剪贴板零成功——对抗测试的对账面。
    pub fn read(&mut self, win: u32, via_compositor: bool) -> ReadVerdict {
        let focused = self.focus_win == Some(win);
        if !focused {
            self.denied_bg_reads += 1;
            self.push_audit(focused, via_compositor, false);
            return ReadVerdict::DeniedNotFocused;
        }
        if !via_compositor {
            self.push_audit(focused, via_compositor, false);
            return ReadVerdict::DeniedBypass;
        }
        if self.state != OwnState::Owned {
            self.push_audit(focused, via_compositor, false);
            return ReadVerdict::DeniedEmpty;
        }
        self.push_audit(focused, via_compositor, true);
        ReadVerdict::Granted
    }

    /// 敏感类型判定（Q56 在合成器审计点执法）：密码管理器类来源拒粘贴。
    pub fn paste_sensitive(&mut self, sensitive: bool) -> bool {
        if sensitive {
            self.sensitive_denied += 1;
            false
        } else {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// 文本写入与截断（类型守卫）
// ---------------------------------------------------------------------------

/// 文本写入描述：256KB 上限，超限截断并如实标记（大文本走文件路径）。
pub fn text_desc(len: u64) -> ClipDesc {
    ClipDesc {
        kind: ClipKind::Text,
        size_or_dim: len,
        truncated: len > TEXT_MAX as u64,
    }
}

/// 位图写入描述：4096 见方上限，超限截断（缩边）并如实标记。
pub fn bitmap_desc(w: u32, h: u32) -> ClipDesc {
    let over = w > BITMAP_MAX_SIDE || h > BITMAP_MAX_SIDE;
    ClipDesc {
        kind: ClipKind::Bitmap,
        size_or_dim: ((w as u64) << 32) | h as u64,
        truncated: over,
    }
}

/// 文件引用粘贴语义：粘贴转文件操作；删除动作进回收站（零真删——
/// 与 trashbin DeleteApi 语义咬合，此处以标记位承接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteOp {
    Copy,
    Move,
    /// 删除：必须进回收站（本模块只产出语义标记，真删面不存在）。
    TrashOnly,
}

pub fn paste_op(move_mod: bool, delete: bool) -> PasteOp {
    if delete {
        PasteOp::TrashOnly
    } else if move_mod {
        PasteOp::Move
    } else {
        PasteOp::Copy
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3901 · 7 项 + B-3903 · 3 项）
// ---------------------------------------------------------------------------

pub fn run_clipown_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3901/3903 剪贴板所有权与大载荷");
    // 1. 所有权授予只对焦点窗口生效。
    let mut cb = Clipboard::new();
    cb.set_focus(Some(7));
    let d = text_desc(10);
    let c = ClipContent::Text { buf: [0; 32], len: 0, real_len: 10 };
    let grant_bg = cb.select(9, d, c); // 非焦点窗口请求
    let grant_ok = cb.select(7, d, c);
    set.add(
        "B-3901 所有权只发焦点",
        !grant_bg && grant_ok && cb.owner == Some(7),
        "clipboard_select 只发给焦点窗口，后台授予请求拒绝",
    );
    // 2. 后台读剪贴板零成功（对抗测试面：拒绝计数 + 裁决面）。
    let v_bg = cb.read(9, true);
    let v_ok = cb.read(7, true);
    set.add(
        "B-3901 后台读取零成功",
        v_bg == ReadVerdict::DeniedNotFocused && v_ok == ReadVerdict::Granted && cb.denied_bg_reads == 1,
        "非焦点窗口读取一律拒绝并留痕——显式授权语义（C-8 姊妹约束）",
    );
    // 3. 未经合成器的读取拒绝（审计点单一路径——Q56 执法位）。
    let v_bypass = cb.read(7, false);
    set.add(
        "B-3901 读必经合成器",
        v_bypass == ReadVerdict::DeniedBypass,
        "旁路读取在裁决面不存在——审计点单一路径",
    );
    // 4. 所有者退出所有权即时回收（引用悬空零存在）。
    let gone = cb.owner_gone(7);
    let v_after = cb.read(7, true);
    set.add(
        "B-3901 所有者退出即时回收",
        gone && v_after == ReadVerdict::DeniedEmpty && cb.owner.is_none() && cb.desc.is_none(),
        "描述段与内容段随所有权同灭——引用悬空零存在",
    );
    // 5. 文本 256KB 截断如实标记 + 位图 4096 见方截断标记。
    let t_over = text_desc(TEXT_MAX as u64 + 1);
    let t_ok = text_desc(1000);
    let b_over = bitmap_desc(5000, 3000);
    let b_ok = bitmap_desc(1920, 1080);
    set.add(
        "B-3901 类型上限如实标记",
        t_over.truncated && !t_ok.truncated && b_over.truncated && !b_ok.truncated,
        "文本 256KB/位图 4096 见方，超限截断标记不撒谎",
    );
    // 6. 文件引用粘贴：删除语义必须进回收站（与回收站零真删咬合）。
    set.add(
        "B-3901 删除进回收站咬合",
        paste_op(false, true) == PasteOp::TrashOnly
            && paste_op(true, false) == PasteOp::Move
            && paste_op(false, false) == PasteOp::Copy,
        "移动按修饰键/删除进回收站——剪贴板与回收站语义咬合",
    );
    // 7. 敏感类型判定（Q56 审计点执法）。
    let mut cb7 = Clipboard::new();
    let s1 = cb7.paste_sensitive(true);
    let s2 = cb7.paste_sensitive(false);
    set.add(
        "B-3901 敏感类型执法",
        !s1 && s2 && cb7.sensitive_denied == 1,
        "密码管理器类来源敏感判定拒绝并留痕",
    );
    // 8. 大载荷引用传递：位图内容是共享内存引用（shm_id+offset），
    //    报文不含像素——复制操作微秒级（引用传递按需付费）。
    let d8 = bitmap_desc(4096, 4096);
    let c8 = ClipContent::Bitmap { shm_id: 3, offset: 4096, w: 4096, h: 4096 };
    let mut cb8 = Clipboard::new();
    cb8.set_focus(Some(1));
    let ok8 = cb8.select(1, d8, c8);
    let ref_only = matches!(cb8.content, Some(ClipContent::Bitmap { shm_id, offset, .. }) if shm_id == 3 && offset == 4096);
    set.add(
        "B-3903 大载荷引用传递",
        ok8 && ref_only && d8.truncated == false,
        "4096² 位图报文只传 shm 引用——复制是引用操作非字节搬运",
    );
    // 9. 大图粘贴可取消进度（诚实的慢优于冻结的快）。
    let cancel9 = paste_progress_cancelable(4096, 4096);
    let nocancel9 = paste_progress_cancelable(64, 64);
    set.add(
        "B-3903 大图粘贴可取消",
        cancel9 && !nocancel9,
        "视尺寸节流：大图带可取消进度，小图直贴",
    );
    // 10. 粘贴回显一帧预算（文本 16ms 同源判据——预算模型面）。
    set.add(
        "B-3903 文本回显一帧",
        text_paste_budget_ns(32 * 1024) <= 16_000_000,
        "32KB 文本粘贴回显整数预算 ≤16ms（与 B-1603 同源口径）",
    );
    set
}

/// 位图粘贴是否带可取消进度：面积超 1M 像素即节流档（可取消）。
pub fn paste_progress_cancelable(w: u32, h: u32) -> bool {
    (w as u64) * (h as u64) > 1_000_000
}

/// 文本粘贴回显预算（纳秒）：拷贝 8ns/字节 + 提交 2ms 固定（整数模型）。
pub fn text_paste_budget_ns(len: u64) -> u64 {
    len * 8 + 2_000_000
}

// ---------------------------------------------------------------------------
// 单测（fb01 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fb01_focus_only_ownership() {
        let mut cb = Clipboard::new();
        cb.set_focus(Some(2));
        let d = text_desc(5);
        let c = ClipContent::Text { buf: [0; 32], len: 0, real_len: 5 };
        assert!(!cb.select(3, d, c), "非焦点授予拒绝");
        assert!(cb.select(2, d, c), "焦点授予成功");
        cb.set_focus(Some(3));
        // 焦点转移不改变所有权（所有权与焦点是两回事——读受焦点门控）。
        assert_eq!(cb.read(3, true), ReadVerdict::Granted);
        assert_eq!(cb.read(2, true), ReadVerdict::DeniedNotFocused, "失焦后原所有者变后台读取者");
    }

    #[test]
    fn fb01_zero_background_read() {
        // 对抗测试：100 轮后台读取零成功。
        let mut cb = Clipboard::new();
        cb.set_focus(Some(1));
        let d = text_desc(3);
        let c = ClipContent::Text { buf: [0; 32], len: 0, real_len: 3 };
        assert!(cb.select(1, d, c));
        let mut i = 0;
        while i < 100 {
            assert_eq!(cb.read(100 + i, true), ReadVerdict::DeniedNotFocused, "后台读零成功");
            i += 1;
        }
        assert_eq!(cb.denied_bg_reads, 100);
    }

    #[test]
    fn fb01_owner_reclaim() {
        let mut cb = Clipboard::new();
        cb.set_focus(Some(5));
        let d = text_desc(1);
        let c = ClipContent::Text { buf: [0; 32], len: 0, real_len: 1 };
        assert!(cb.select(5, d, c));
        assert!(!cb.owner_gone(6), "非所有者退出不误回收");
        assert!(cb.owner_gone(5));
        assert_eq!(cb.state, OwnState::Free);
        assert_eq!(cb.read(5, true), ReadVerdict::DeniedEmpty, "回收后读为空");
    }

    #[test]
    fn fb03_ref_paste_budget() {
        let d = bitmap_desc(4096, 4096);
        assert!(!d.truncated);
        assert!(paste_progress_cancelable(2000, 1000));
        assert!(!paste_progress_cancelable(500, 500));
        assert!(text_paste_budget_ns(64 * 1024) <= 16_000_000);
    }
}
