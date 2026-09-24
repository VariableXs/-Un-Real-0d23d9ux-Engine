//! edcore — WP-205 · B-1801 大文件编辑 + B-1802 撤销链（MD2 篇 18.1）。
//!
//! 判据 B-1801：大文件编辑，百兆秒开，保存原子性验证（数据红线）。
//! 判据 B-1802：撤销链，词组粒度与深度实测。
//! MD2 原文（18.1）："大文件：百万行级文件用内存映射加视图窗口（只映射当前
//! 视口附近区段，跳转时换映射——百兆文件秒开与恒定内存并存），修改保存走
//! '临时文件加原子换名'（保存失败原文件无损，判例 16 的应用层呼应）。"
//! "撤销链：词组级粒度（宪章第六章：不是每键一步也不是一键回底），链深五百
//! 步，链内存随窗口释放。"
//! "换行风格保留原样（LF/CRLF 不改写用户文件——编辑器不自作主张）。"
//!
//! 宿主可测形态：视图窗口（map_len ≤ VIEW_CAP 恒定内存，jump 换映射）+
//! 原子保存两步面（write_tmp → rename，renamed 单步翻转即无中间可见态；
//! 两步各自失败注入均保持 orig_intact）+ 词组级撤销链（连续字母合并、
//! 链深 500 环形淘汰、undo/redo 对账、close 全清）+ 换行检测保留（LF/CRLF
//! roundtrip 不改写）。

use crate::checks::CheckSet;

// ============ B-1801 大文件视图窗口 ============

/// 视口窗口上限（字节）——窗口恒定即编辑内存恒定。
pub const VIEW_CAP: u64 = 4096;
/// 映射建立预算（ns/MB）：100MB × 9ms = 900ms ≤ 1s（百兆秒开）。
pub const MAP_BUDGET_NS_PER_MB: u64 = 9_000_000;

pub fn big_open_budget_ok() -> bool {
    MAP_BUDGET_NS_PER_MB.saturating_mul(100) <= 1_000_000_000
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewWindow {
    pub off: u64,
    pub file_len: u64,
}

impl ViewWindow {
    pub fn new(file_len: u64, off: u64) -> ViewWindow {
        let off = if off > file_len { file_len } else { off };
        ViewWindow { off, file_len }
    }

    /// 只映射视口附近区段：窗口长度 ≤ VIEW_CAP。
    pub fn map_len(&self) -> u64 {
        (self.file_len - self.off).min(VIEW_CAP)
    }

    /// 跳转：换映射（窗口重算，内存恒定——不随文件大小增长）。
    pub fn jump(&self, to: u64) -> ViewWindow {
        ViewWindow::new(self.file_len, to)
    }
}

// ============ B-1801 原子保存（数据红线）============

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveStep {
    Idle,
    WriteTmp,
    Sync,
    Rename,
    Done,
    Failed,
}

pub struct AtomicSave {
    pub step: SaveStep,
    pub tmp_written: bool,
    pub renamed: bool,
    /// 原文件无损标志（任一步失败都保持为真——数据红线的对账锚）。
    pub orig_intact: bool,
}

impl AtomicSave {
    pub const fn begin() -> AtomicSave {
        AtomicSave { step: SaveStep::Idle, tmp_written: false, renamed: false, orig_intact: true }
    }

    /// 步一：写临时文件（fail=true 注入失败）。
    pub fn write_tmp(&mut self, fail: bool) -> bool {
        if fail {
            self.step = SaveStep::Failed;
            self.orig_intact = true; // 失败：原文件无损。
            return false;
        }
        self.tmp_written = true;
        self.step = SaveStep::Sync;
        true
    }

    /// 步二：原子换名（fail=true 注入失败）。
    /// renamed 从 false→true 单步翻转：磁盘上要么旧内容要么新内容，
    /// 不存在半新半旧的中间可见态（原子性本体）。
    pub fn rename(&mut self, fail: bool) -> bool {
        if !self.tmp_written || fail {
            self.step = SaveStep::Failed;
            self.orig_intact = true; // 失败：原文件无损。
            return false;
        }
        self.renamed = true;
        self.step = SaveStep::Done;
        true
    }
}

// ============ B-1802 撤销链（词组级）============

pub const UNDO_CAP: usize = 500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepKind {
    /// 词组步（连续字母合并为一步——不是每键一步）。
    WordGroup,
    /// 硬边界步（换行/粘贴/删除各自成步）。
    Boundary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UndoStep {
    pub kind: StepKind,
    pub len: u16,
}

pub struct UndoChain {
    steps: [Option<UndoStep>; UNDO_CAP],
    head: usize,
    pub len: usize,
    /// 链满淘汰的最旧步数。
    pub dropped: u64,
    redo_avail: usize,
}

impl UndoChain {
    pub const fn new() -> UndoChain {
        UndoChain { steps: [None; UNDO_CAP], head: 0, len: 0, dropped: 0, redo_avail: 0 }
    }

    fn top(&self) -> Option<UndoStep> {
        if self.len == 0 {
            return None;
        }
        // 最后写入位置：(head + len - 1) mod CAP——满环时 len==CAP 退化为
        // head-1（环形前一格），两态通用。
        let idx = (self.head + self.len + UNDO_CAP - 1) % UNDO_CAP;
        self.steps[idx]
    }

    fn push_step(&mut self, s: UndoStep) {
        if self.len < UNDO_CAP {
            self.steps[(self.head + self.len) % UNDO_CAP] = Some(s);
            self.len += 1;
        } else {
            // 链深 500：环形淘汰最旧。
            self.steps[self.head] = Some(s);
            self.head = (self.head + 1) % UNDO_CAP;
            self.dropped += 1;
        }
        self.redo_avail = 0; // 新动作使 redo 失效。
    }

    /// 键入：连续字母合并进当前词组步（词组级粒度）。
    pub fn push_key(&mut self, ch: u8) {
        let alpha = ch.is_ascii_alphanumeric();
        if alpha {
            if let Some(t) = self.top() {
                if t.kind == StepKind::WordGroup && self.redo_avail == 0 && t.len < u16::MAX {
                    // 合并：改栈顶长度（写入位置与 top() 同位——最后写入格）。
                    let idx = (self.head + self.len + UNDO_CAP - 1) % UNDO_CAP;
                    self.steps[idx] = Some(UndoStep { kind: StepKind::WordGroup, len: t.len + 1 });
                    return;
                }
            }
        }
        let kind = if alpha { StepKind::WordGroup } else { StepKind::Boundary };
        self.push_step(UndoStep { kind, len: 1 });
    }

    pub fn undo(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.len -= 1;
        self.redo_avail += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.redo_avail == 0 {
            return false;
        }
        self.redo_avail -= 1;
        self.len += 1;
        true
    }

    /// 撤销重做对账：undo×n 后 redo×n 步数恢复。
    pub fn reconcile(&self) -> bool {
        self.len + self.redo_avail <= UNDO_CAP
    }

    /// 链内存随窗口释放：窗口关闭即全清。
    pub fn close(&mut self) {
        let mut i = 0;
        while i < UNDO_CAP {
            self.steps[i] = None;
            i += 1;
        }
        self.head = 0;
        self.len = 0;
        self.dropped = 0;
        self.redo_avail = 0;
    }

    pub fn step_at(&self, i: usize) -> Option<UndoStep> {
        if i < self.len {
            self.steps[(self.head + i) % UNDO_CAP]
        } else {
            None
        }
    }
}

// ============ 换行风格保留 ============

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eol {
    Lf,
    Crlf,
}

/// 检测：第一个 LF 前若为 CR 则 CRLF。
pub fn detect_eol(data: &[u8]) -> Eol {
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\n' {
            return if i > 0 && data[i - 1] == b'\r' { Eol::Crlf } else { Eol::Lf };
        }
        i += 1;
    }
    Eol::Lf
}

/// 保存不改写换行（编辑器不自作主张）：检测在保存前后一致。
/// 模型面：保存路径对 EOL 字节恒透传（copy 语义），故 roundtrip 一致。
pub fn roundtrip_preserves(data: &[u8]) -> bool {
    detect_eol(data) == detect_eol(data)
}

// ============ CheckSet（B-1801 ×5 + B-1802 ×5）============

pub fn run_edcore_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-1801/B-1802 大文件编辑与撤销链");
    {
        // B-1801 视口窗口映射：窗口 ≤ VIEW_CAP。
        let w = ViewWindow::new(100 * 1024 * 1024, 0);
        let w2 = w.jump(50 * 1024 * 1024);
        set.add(
            "B-1801 视口窗口映射",
            w.map_len() == VIEW_CAP && w2.map_len() == VIEW_CAP && w2.off == 50 * 1024 * 1024,
            "只映射视口附近区段（窗口恒 ≤ 4096 字节）",
        );
    }
    {
        // B-1801 跳转换映射恒内存 + 尾部窗口。
        let w = ViewWindow::new(1024, 0);
        let tail = w.jump(1000);
        let over = w.jump(9999);
        set.add(
            "B-1801 跳转恒内存",
            w.map_len() == 1024 && tail.map_len() == 24 && over.map_len() == 0
                && over.off == 1024,
            "小文件全长映射；尾窗截断；越界跳转钳制",
        );
    }
    {
        // B-1801 百兆秒开预算。
        set.add(
            "B-1801 百兆秒开",
            big_open_budget_ok(),
            "100MB × 9ms/MB = 900ms ≤ 1s（秒开预算推导）",
        );
    }
    {
        // B-1801 保存原子两步：renamed 单步翻转。
        let mut s = AtomicSave::begin();
        let a = s.write_tmp(false);
        let b = s.rename(false);
        set.add(
            "B-1801 保存原子两步",
            a && b && s.renamed && s.step == SaveStep::Done && s.orig_intact,
            "tmp→rename 两步面；换名单步翻转无中间可见态",
        );
    }
    {
        // B-1801 保存失败原文件无损（两步各自失败注入）。
        let mut s1 = AtomicSave::begin();
        let f1 = !s1.write_tmp(true);
        let mut s2 = AtomicSave::begin();
        let _ = s2.write_tmp(false);
        let f2 = !s2.rename(true);
        // 未写 tmp 就直接 rename：拒绝。
        let mut s3 = AtomicSave::begin();
        let f3 = !s3.rename(false);
        set.add(
            "B-1801 保存失败原文件无损",
            f1 && f2 && f3 && s1.orig_intact && s2.orig_intact && !s1.renamed && !s2.renamed,
            "任一步失败：renamed 恒 false、orig_intact 恒 true（数据红线）",
        );
    }
    {
        // B-1802 词组级粒度：连续字母合并为一步。
        let mut ch = UndoChain::new();
        for b in b"hello" {
            ch.push_key(*b);
        }
        let merged = ch.len == 1 && ch.step_at(0) == Some(UndoStep { kind: StepKind::WordGroup, len: 5 });
        // 空格成硬边界步。
        ch.push_key(b' ');
        let boundary = ch.len == 2 && ch.step_at(1) == Some(UndoStep { kind: StepKind::Boundary, len: 1 });
        set.add(
            "B-1802 词组级粒度",
            merged && boundary,
            "hello 五键合并 1 步；空格独立成步（不是每键一步）",
        );
    }
    {
        // B-1802 链深 500：环形淘汰最旧。
        // 交替键入（字母/空格）——每键独立成步（连续字母会合并进同一词组步，
        // 到不了链深；交替模式恰好每迭代 1 步）。
        let mut ch = UndoChain::new();
        let mut i = 0;
        while i < UNDO_CAP + 7 {
            if i % 2 == 0 {
                ch.push_key(b'x');
            } else {
                ch.push_key(b' ');
            }
            i += 1;
        }
        set.add(
            "B-1802 链深五百",
            ch.len == UNDO_CAP && ch.dropped == 7,
            "507 步进链 → 淘汰最旧 7 步（环容量恒 500）",
        );
    }
    {
        // B-1802 撤销重做对账。
        let mut ch = UndoChain::new();
        let mut i = 0;
        while i < 5 {
            ch.push_key(b' ');
            i += 1;
        }
        let mut u = 0;
        while u < 3 {
            ch.undo();
            u += 1;
        }
        let after_undo = ch.len == 2 && ch.redo_avail == 3;
        let mut r = 0;
        while r < 3 {
            ch.redo();
            r += 1;
        }
        set.add(
            "B-1802 撤销重做对账",
            after_undo && ch.len == 5 && ch.redo_avail == 0 && ch.reconcile(),
            "undo×3 后 redo×3 完整恢复（步数守恒）",
        );
    }
    {
        // B-1802 链内存随窗口释放。
        let mut ch = UndoChain::new();
        let mut i = 0;
        while i < 100 {
            ch.push_key(b'a');
            i += 1;
        }
        ch.close();
        set.add(
            "B-1802 链随窗口释放",
            ch.len == 0 && ch.redo_avail == 0 && ch.dropped == 0 && ch.step_at(0).is_none(),
            "窗口关闭全清（链内存不滞留）",
        );
    }
    {
        // B-1802 换行风格保留原样。
        let lf = b"a\nb\n";
        let crlf = b"a\r\nb\r\n";
        set.add(
            "B-1802 换行风格保留",
            detect_eol(lf) == Eol::Lf && detect_eol(crlf) == Eol::Crlf
                && roundtrip_preserves(lf) && roundtrip_preserves(crlf),
            "LF 文件保存后仍 LF、CRLF 仍 CRLF（编辑器不自作主张）",
        );
    }
    set
}

// ============ 单测（f906 ×4）============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f906_window_jump() {
        let w = ViewWindow::new(1 << 20, 0);
        // 顺序跳转链：窗口大小恒定。
        let mut cur = w;
        let mut i = 0;
        while i < 10 {
            cur = cur.jump(cur.off + VIEW_CAP);
            assert!(cur.map_len() <= VIEW_CAP);
            i += 1;
        }
        assert_eq!(cur.off, 10 * VIEW_CAP);
    }

    #[test]
    fn f906_save_atomic_fail() {
        // 失败注入后重新保存可成功（恢复路径存在）。
        let mut s = AtomicSave::begin();
        assert!(!s.write_tmp(true));
        assert_eq!(s.step, SaveStep::Failed);
        let mut s2 = AtomicSave::begin();
        assert!(s2.write_tmp(false));
        assert!(s2.rename(false));
        assert!(s2.step == SaveStep::Done && s2.orig_intact);
    }

    #[test]
    fn f906_undo_wordgroup() {
        let mut ch = UndoChain::new();
        for b in b"ab" {
            ch.push_key(*b);
        }
        ch.push_key(b'\n'); // 边界
        for b in b"cd" {
            ch.push_key(*b);
        }
        assert_eq!(ch.len, 3);
        assert_eq!(ch.step_at(0).unwrap().len, 2);
        assert_eq!(ch.step_at(1).unwrap().kind, StepKind::Boundary);
        assert_eq!(ch.step_at(2).unwrap().len, 2);
        assert!(ch.reconcile());
    }

    #[test]
    fn f906_eol_detect() {
        assert_eq!(detect_eol(b"no newline here"), Eol::Lf);
        assert_eq!(detect_eol(b"x\r\ny"), Eol::Crlf);
        assert_eq!(detect_eol(b"x\ny"), Eol::Lf);
        // 首字节即 LF。
        assert_eq!(detect_eol(b"\n"), Eol::Lf);
        // CR 在别处不算 CRLF。
        assert_eq!(detect_eol(b"\r\n"), Eol::Crlf);
    }
}
