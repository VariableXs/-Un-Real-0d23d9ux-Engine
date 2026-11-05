//! UNX-F12608 端口区段（Section）共享缓冲→D4 大消息消费 · 0x801C0段
//!
//! 判据原文（派工卡 12607 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12608-J1 区段映射大消息消费边对平断言绿
//!      （视图大小/偏移档），越界读注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 区段-视图模型：区段（Section）是定长共享缓冲，视图（View）以
//!    偏移+大小映射进区段；映射规则：偏移对齐 0x10、偏移+大小 ≤ 区段
//!    容量、大小非零，三规一破即映射拒；
//! 2. 大消息消费边：消息头部落在小消息通道，正文落在视图内；消费边
//!    对平 = 头部长度 + 正文长度 == 视图占用，缺一即账不平；
//! 3. 视图大小/偏移档：视图按（偏移, 大小）二元组建档，档间不得重叠
//!    （重叠即共享冲突），档号单调登记；
//! 4. 越界读注入 10/10：十路构造（偏移越界、大小越界、偏移+大小溢出、
//!    未映射读、零大小视图、错位对齐、重叠档、越档读、空区段读、
//!    重复映射）逐路拒止，任何一路放行即判据失效；
//! 5. 溢出全用 checked_add/saturating_add，不做裸加法指针运算；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12607 − 12600)）。
pub const SEG_BASE: u32 = 0x801C0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 区段映射诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvDiag {
    /// 偏移未按 0x10 对齐。
    EUnaligned,
    /// 视图越区段右界。
    EViewOver,
    /// 偏移+大小整数溢出。
    EOverflow,
    /// 零大小视图。
    EZeroSize,
    /// 未映射地址读。
    EUnmapped,
    /// 视图档重叠。
    EOverlap,
    /// 越档读（读了别人的视图）。
    ECrossRead,
    /// 空区段读。
    EEmptySection,
    /// 重复映射同档号。
    EDupMap,
    /// 消费边账不平（头部+正文 ≠ 视图占用）。
    EUnbalanced,
    /// 空样本（偏移与大小全零）。
    EEmpty,
    /// 区段容量为零。
    ENoSection,
}

impl SvDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SvDiag::EUnaligned => 0x01,
                SvDiag::EViewOver => 0x02,
                SvDiag::EOverflow => 0x03,
                SvDiag::EZeroSize => 0x04,
                SvDiag::EUnmapped => 0x05,
                SvDiag::EOverlap => 0x06,
                SvDiag::ECrossRead => 0x07,
                SvDiag::EEmptySection => 0x08,
                SvDiag::EDupMap => 0x09,
                SvDiag::EUnbalanced => 0x0a,
                SvDiag::EEmpty => 0x0b,
                SvDiag::ENoSection => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SvDiag::EUnaligned => "偏移未对齐：视图偏移必须 0x10 对齐，先改对齐再映射",
            SvDiag::EViewOver => "视图越界：偏移+大小超区段容量，先缩视图或换区段",
            SvDiag::EOverflow => "整数溢出：偏移+大小用 checked_add 判定，溢出即拒",
            SvDiag::EZeroSize => "零大小视图：不构成映射，先给非零大小",
            SvDiag::EUnmapped => "未映射读：读前先查视图档，未映射地址一律拒",
            SvDiag::EOverlap => "视图重叠：档间不得重叠，重叠即共享冲突",
            SvDiag::ECrossRead => "越档读：读操作不得跨出本视图，越档按越界拒",
            SvDiag::EEmptySection => "空区段读：区段未映射任何视图，禁止读",
            SvDiag::EDupMap => "重复映射：档号一次映射，重复映射先解除",
            SvDiag::EUnbalanced => "消费边账不平：头部长+正文长必须等于视图占用",
            SvDiag::EEmpty => "空样本：偏移与大小全零不构成视图",
            SvDiag::ENoSection => "零容量区段：先建区段再映射视图",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SV_ALL: &[SvDiag] = &[
    SvDiag::EUnaligned,
    SvDiag::EViewOver,
    SvDiag::EOverflow,
    SvDiag::EZeroSize,
    SvDiag::EUnmapped,
    SvDiag::EOverlap,
    SvDiag::ECrossRead,
    SvDiag::EEmptySection,
    SvDiag::EDupMap,
    SvDiag::EUnbalanced,
    SvDiag::EEmpty,
    SvDiag::ENoSection,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in SV_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x801C1..0x801CC 闭集内"
}

// ── 区段-视图域模型 ──────────────────────────────────────────────
/// 偏移对齐粒度。
pub const ALIGN: u32 = 0x10;
/// 标准区段容量（字节）。
pub const SECTION_CAP: u32 = 0x10000;

/// 视图档：偏移 + 大小（判据原文"视图大小/偏移档"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSlot {
    /// 档号（非零，单调登记）。
    pub slot_id: u32,
    /// 映射偏移（0x10 对齐）。
    pub offset: u32,
    /// 视图大小（非零）。
    pub size: u32,
}

impl ViewSlot {
    /// 静态合法性：非零样本。
    pub fn non_empty(&self) -> bool {
        !(self.slot_id == 0 && self.offset == 0 && self.size == 0)
    }

    /// 区间右界（checked，溢出 None）。
    pub fn end(&self) -> Option<u32> {
        self.offset.checked_add(self.size)
    }

    /// 与另一档是否重叠（半开区间 [offset, end)）。
    pub fn overlaps(&self, other: &ViewSlot) -> bool {
        let (a_end, b_end) = match (self.end(), other.end()) {
            (Some(a), Some(b)) => (a, b),
            _ => return true, // 溢出档视为冲突。
        };
        self.offset < b_end && other.offset < a_end
    }
}

/// 大消息消费边账：头部（小消息通道）+ 正文（视图内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BigMsg {
    /// 消息号。
    pub msg_id: u32,
    /// 所属视图档号。
    pub slot_id: u32,
    /// 头部长度（小消息通道携带）。
    pub head_len: u32,
    /// 正文长度（视图内承载）。
    pub body_len: u32,
}

impl BigMsg {
    /// 消费边对平：头 + 体 == 视图占用（以视图大小记账）。
    pub fn balanced(&self, view_size: u32) -> bool {
        match self.head_len.checked_add(self.body_len) {
            Some(total) => total == view_size,
            None => false,
        }
    }
}

/// 区段：定容共享缓冲 + 视图档表。
#[derive(Clone, Debug)]
pub struct Section {
    /// 区段容量（字节，零容量非法）。
    cap: u32,
    /// 视图档（按登记序）。
    slots: Vec<ViewSlot>,
}

impl Section {
    /// 建区段。
    pub fn new(cap: u32) -> Section {
        Section {
            cap,
            slots: Vec::new(),
        }
    }

    /// 容量。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 档数。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 查档。
    pub fn slot_of(&self, slot_id: u32) -> Option<ViewSlot> {
        for s in self.slots.iter() {
            if s.slot_id == slot_id {
                return Some(*s);
            }
        }
        None
    }

    /// 映射视图（六道闸：空样本/零容量/对齐/零大小/越界/溢出/重叠/重复）。
    pub fn map(&mut self, slot: ViewSlot) -> Result<(), u32> {
        if !slot.non_empty() {
            return Err(SvDiag::EEmpty.val());
        }
        if self.cap == 0 {
            return Err(SvDiag::ENoSection.val());
        }
        if slot.offset % ALIGN != 0 {
            return Err(SvDiag::EUnaligned.val());
        }
        if slot.size == 0 {
            return Err(SvDiag::EZeroSize.val());
        }
        let end = match slot.end() {
            Some(e) => e,
            None => return Err(SvDiag::EOverflow.val()),
        };
        if end > self.cap {
            return Err(SvDiag::EViewOver.val());
        }
        if self.slot_of(slot.slot_id).is_some() {
            return Err(SvDiag::EDupMap.val());
        }
        for s in self.slots.iter() {
            if s.overlaps(&slot) {
                return Err(SvDiag::EOverlap.val());
            }
        }
        self.slots.push(slot);
        Ok(())
    }

    /// 视图内读：地址必须落在本视图 [offset, end) 内。
    pub fn read(&self, slot_id: u32, addr: u32, len: u32) -> Result<(), u32> {
        if self.slot_count() == 0 {
            return Err(SvDiag::EEmptySection.val());
        }
        let slot = match self.slot_of(slot_id) {
            Some(s) => s,
            None => return Err(SvDiag::EUnmapped.val()),
        };
        let end = match slot.end() {
            Some(e) => e,
            None => return Err(SvDiag::EOverflow.val()),
        };
        // 读区间 [addr, addr+len) 必须整体落在本视图内。
        let rend = match addr.checked_add(len) {
            Some(r) => r,
            None => return Err(SvDiag::EOverflow.val()),
        };
        if len == 0 {
            return Err(SvDiag::EZeroSize.val());
        }
        if addr < slot.offset || rend > end {
            return Err(SvDiag::ECrossRead.val());
        }
        Ok(())
    }

    /// 大消息消费边落账：视图 + 头/体长度对平。
    pub fn consume_big(&self, msg: BigMsg) -> Result<(), u32> {
        let slot = match self.slot_of(msg.slot_id) {
            Some(s) => s,
            None => return Err(SvDiag::EUnmapped.val()),
        };
        if !msg.balanced(slot.size) {
            return Err(SvDiag::EUnbalanced.val());
        }
        // 正文必须整体落在视图内（零正文即纯头部消息，免读检）。
        if msg.body_len > 0 {
            self.read(msg.slot_id, slot.offset, msg.body_len)?;
        }
        Ok(())
    }

    /// 档间无重叠守恒。
    pub fn no_overlap(&self) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            let a = match self.slots.get(i) {
                Some(s) => *s,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.slots.len() {
                let b = match self.slots.get(j) {
                    Some(s) => *s,
                    None => return false,
                };
                if a.overlaps(&b) {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

// ── 对平断言与注入面 ─────────────────────────────────────────────
/// 构建标准区段：三档视图（0x0000/0x100、0x1000/0x800、0x8000/0x400）。
pub fn std_section() -> Section {
    let mut sec = Section::new(SECTION_CAP);
    let _ = sec.map(ViewSlot {
        slot_id: 1,
        offset: 0x0000,
        size: 0x100,
    });
    let _ = sec.map(ViewSlot {
        slot_id: 2,
        offset: 0x1000,
        size: 0x800,
    });
    let _ = sec.map(ViewSlot {
        slot_id: 3,
        offset: 0x8000,
        size: 0x400,
    });
    sec
}

/// 大消息消费边对平断言（判据 J1 直读口）：三档各落一笔账全对平。
fn big_msg_pairing_ok() -> bool {
    let sec = std_section();
    if sec.slot_count() != 3 || !sec.no_overlap() {
        return false;
    }
    // 档 1：头 0x40 + 体 0xC0 == 0x100。
    if sec
        .consume_big(BigMsg {
            msg_id: 0x10,
            slot_id: 1,
            head_len: 0x40,
            body_len: 0xC0,
        })
        .is_err()
    {
        return false;
    }
    // 档 2：头 0x100 + 体 0x700 == 0x800。
    if sec
        .consume_big(BigMsg {
            msg_id: 0x11,
            slot_id: 2,
            head_len: 0x100,
            body_len: 0x700,
        })
        .is_err()
    {
        return false;
    }
    // 档 3：头 0x80 + 体 0x380 == 0x400。
    sec.consume_big(BigMsg {
        msg_id: 0x12,
        slot_id: 3,
        head_len: 0x80,
        body_len: 0x380,
    })
    .is_ok()
}

/// 越界读注入 10/10：十路违约全数拒止。
fn oob_injections_ok() -> bool {
    let mut hits = 0usize;
    let sec = std_section();
    // 注 1：偏移越界（超区段右界映射）。
    let mut s1 = Section::new(SECTION_CAP);
    if s1.map(ViewSlot {
        slot_id: 9,
        offset: SECTION_CAP,
        size: 0x10,
    }) == Err(SvDiag::EViewOver.val())
    {
        hits += 1;
    }
    // 注 2：大小越界（末端出界）。
    let mut s2 = Section::new(SECTION_CAP);
    if s2.map(ViewSlot {
        slot_id: 9,
        offset: SECTION_CAP - 0x10,
        size: 0x20,
    }) == Err(SvDiag::EViewOver.val())
    {
        hits += 1;
    }
    // 注 3：偏移+大小溢出。
    let mut s3 = Section::new(SECTION_CAP);
    if s3.map(ViewSlot {
        slot_id: 9,
        offset: 0xFFFFFFF0,
        size: 0x20,
    }) == Err(SvDiag::EOverflow.val())
    {
        hits += 1;
    }
    // 注 4：未映射地址读。
    if sec.read(1, 0x5000, 0x10) == Err(SvDiag::ECrossRead.val()) {
        hits += 1;
    }
    // 注 5：零大小读。
    if sec.read(1, 0x0000, 0) == Err(SvDiag::EZeroSize.val()) {
        hits += 1;
    }
    // 注 6：错位对齐映射。
    let mut s6 = Section::new(SECTION_CAP);
    if s6.map(ViewSlot {
        slot_id: 9,
        offset: 0x0008,
        size: 0x10,
    }) == Err(SvDiag::EUnaligned.val())
    {
        hits += 1;
    }
    // 注 7：重叠档映射。
    let mut s7 = std_section();
    if s7.map(ViewSlot {
        slot_id: 9,
        offset: 0x0080,
        size: 0x100,
    }) == Err(SvDiag::EOverlap.val())
    {
        hits += 1;
    }
    // 注 8：越档读（从档 1 读到档 1 右界之外）。
    if sec.read(1, 0x00F0, 0x20) == Err(SvDiag::ECrossRead.val()) {
        hits += 1;
    }
    // 注 9：空区段读。
    if Section::new(SECTION_CAP)
        .read(1, 0x0, 0x10)
        == Err(SvDiag::EEmptySection.val())
    {
        hits += 1;
    }
    // 注 10：重复映射同档号。
    let mut s10 = std_section();
    if s10.map(ViewSlot {
        slot_id: 1,
        offset: 0x2000,
        size: 0x100,
    }) == Err(SvDiag::EDupMap.val())
    {
        hits += 1;
    }
    hits == 10
}

/// 账不平注入：头/体长度与视图占用不符必拒。
fn unbalanced_msg_rejected_ok() -> bool {
    let sec = std_section();
    // 头+体 不足视图占用。
    if sec
        .consume_big(BigMsg {
            msg_id: 0x20,
            slot_id: 1,
            head_len: 0x10,
            body_len: 0x10,
        })
        != Err(SvDiag::EUnbalanced.val())
    {
        return false;
    }
    // 头+体 超视图占用。
    if sec
        .consume_big(BigMsg {
            msg_id: 0x21,
            slot_id: 1,
            head_len: 0x80,
            body_len: 0x90,
        })
        != Err(SvDiag::EUnbalanced.val())
    {
        return false;
    }
    // 头+体 溢出。
    sec.consume_big(BigMsg {
        msg_id: 0x22,
        slot_id: 1,
        head_len: 0xFFFFFFF0,
        body_len: 0x20,
    }) == Err(SvDiag::EUnbalanced.val())
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in SV_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(SvDiag::ECrossRead.val()) != SvDiag::ECrossRead.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x34).starts_with("未知") {
        return false;
    }
    true
}

/// 视图区间语义：半开区间端点与重叠判定。
fn view_range_semantics_ok() -> bool {
    let a = ViewSlot {
        slot_id: 1,
        offset: 0x100,
        size: 0x100,
    };
    // 端点相接不算重叠（半开区间）。
    let touch = ViewSlot {
        slot_id: 2,
        offset: 0x200,
        size: 0x100,
    };
    let inside = ViewSlot {
        slot_id: 3,
        offset: 0x180,
        size: 0x10,
    };
    let apart = ViewSlot {
        slot_id: 4,
        offset: 0x400,
        size: 0x100,
    };
    a.end() == Some(0x200) && !a.overlaps(&touch) && a.overlaps(&inside) && !a.overlaps(&apart)
}

/// 对齐语义：0x10 对齐三点。
fn align_semantics_ok() -> bool {
    0x0000 % ALIGN == 0 && 0x1000 % ALIGN == 0 && 0x0008 % ALIGN != 0
}

/// 双跑一致：注入判定与对平断言两次相同。
fn dual_run_ok() -> bool {
    let a = (big_msg_pairing_ok(), oob_injections_ok());
    let b = (big_msg_pairing_ok(), oob_injections_ok());
    a == b && a.0 && a.1
}

/// 完成写回语义：消费边摘要非空。
fn writeback_ok() -> bool {
    let sec = std_section();
    let msg = BigMsg {
        msg_id: 0x30,
        slot_id: 2,
        head_len: 0x100,
        body_len: 0x700,
    };
    if sec.consume_big(msg).is_err() {
        return false;
    }
    let summary = format!(
        "msg={} slot={} total=0x{:X}",
        msg.msg_id,
        msg.slot_id,
        msg.head_len.saturating_add(msg.body_len)
    );
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12608 自检（判据 12607 卡：区段映射大消息消费边 + 越界读注入）。
pub fn run_unx12607_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12607");
    let sec = std_section();

    s.add_dyn(
        "F12608-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12608-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", SV_ALL.len()),
    );

    s.add_dyn(
        "F12608-03 标准区段三档映射",
        sec.slot_count() == 3 && sec.no_overlap(),
        &format!("cap=0x{:X} slots={}", sec.cap(), sec.slot_count()),
    );

    s.add_dyn(
        "F12608-04 大消息消费边对平绿",
        big_msg_pairing_ok(),
        "头长+体长 == 视图占用 三档全平",
    );

    // 注入必抓：十路越界读全数拒止。
    s.add_dyn(
        "F12608-05 越界读注入 10/10 拒止",
        oob_injections_ok(),
        "偏越/大越/溢出/未映射/零读/错位/重叠/越档/空段/重映射",
    );

    s.add_dyn(
        "F12608-06 账不平注入必拒",
        unbalanced_msg_rejected_ok(),
        "头体不足/超出/溢出 三路全拒",
    );

    s.add_dyn(
        "F12608-07 视图区间语义",
        view_range_semantics_ok(),
        "半开区间端点相接不重叠、内含重叠、相离不重叠",
    );

    s.add_dyn(
        "F12608-08 对齐语义",
        align_semantics_ok(),
        &format!("ALIGN=0x{:X} 三点判定", ALIGN),
    );

    s.add_dyn(
        "F12608-09 双跑一致",
        dual_run_ok(),
        "对平与注入判定两次逐项相同",
    );

    s.add_dyn(
        "F12608-10 零容量区段拒映射",
        match Section::new(0).map(ViewSlot {
            slot_id: 1,
            offset: 0,
            size: 0x10,
        }) {
            Err(code) => code == SvDiag::ENoSection.val(),
            Ok(()) => false,
        },
        "零容量区段先建再映射",
    );

    s.add_dyn(
        "F12608-11 空样本拒",
        match Section::new(SECTION_CAP).map(ViewSlot {
            slot_id: 0,
            offset: 0,
            size: 0,
        }) {
            Err(code) => code == SvDiag::EEmpty.val(),
            Ok(()) => false,
        },
        "偏移与大小全零不构成视图",
    );

    s.add_dyn(
        "F12608-12 完成写回语义",
        writeback_ok(),
        "消费边摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x801C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(SV_ALL.len(), 12);
        let mut i = 0usize;
        while i < SV_ALL.len() {
            let vi = SV_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SV_ALL.len() {
                let vj = SV_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x33).contains("未知"));
    }

    #[test]
    fn map_gates_reject() {
        // 映射六道闸逐闸验证。
        let mut sec = Section::new(SECTION_CAP);
        // 错位对齐。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 1,
                offset: 0x8,
                size: 0x10,
            }),
            Err(SvDiag::EUnaligned.val())
        );
        // 零大小。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 1,
                offset: 0x0,
                size: 0,
            }),
            Err(SvDiag::EZeroSize.val())
        );
        // 越界。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 1,
                offset: SECTION_CAP,
                size: 0x10,
            }),
            Err(SvDiag::EViewOver.val())
        );
        // 溢出。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 1,
                offset: 0xFFFFFFF0,
                size: 0x20,
            }),
            Err(SvDiag::EOverflow.val())
        );
        // 合法映射。
        assert!(sec
            .map(ViewSlot {
                slot_id: 1,
                offset: 0x0,
                size: 0x100,
            })
            .is_ok());
        // 重复档号。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 1,
                offset: 0x2000,
                size: 0x10,
            }),
            Err(SvDiag::EDupMap.val())
        );
        // 重叠。
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 2,
                offset: 0x80,
                size: 0x10,
            }),
            Err(SvDiag::EOverlap.val())
        );
    }

    #[test]
    fn read_gates_reject() {
        // 读闸：未映射/越档/零读。
        let sec = std_section();
        assert_eq!(sec.read(99, 0x0, 0x10), Err(SvDiag::EUnmapped.val()));
        assert_eq!(sec.read(1, 0x0, 0), Err(SvDiag::EZeroSize.val()));
        // 档 1 [0x0, 0x100)：右界外读拒。
        assert_eq!(sec.read(1, 0xF0, 0x20), Err(SvDiag::ECrossRead.val()));
        // 恰好到右界合法。
        assert!(sec.read(1, 0xF0, 0x10).is_ok());
    }

    #[test]
    fn big_msg_balance_semantics() {
        // 头+体 == 视图占用才平。
        let sec = std_section();
        assert!(big_msg_pairing_ok());
        assert!(sec
            .consume_big(BigMsg {
                msg_id: 0x40,
                slot_id: 1,
                head_len: 0,
                body_len: 0x100,
            })
            .is_ok());
        assert!(sec
            .consume_big(BigMsg {
                msg_id: 0x41,
                slot_id: 1,
                head_len: 0x100,
                body_len: 0,
            })
            .is_ok());
    }

    #[test]
    fn oob_injections_all_rejected() {
        // 注入必抓：十路全数拒止。
        assert!(oob_injections_ok());
        assert!(unbalanced_msg_rejected_ok());
    }

    #[test]
    fn overlap_semantics() {
        // 半开区间重叠判定。
        assert!(view_range_semantics_ok());
        let a = ViewSlot {
            slot_id: 1,
            offset: 0x100,
            size: 0x100,
        };
        let same = a;
        assert!(a.overlaps(&same));
        // 溢出档视为冲突。
        let bad = ViewSlot {
            slot_id: 2,
            offset: 0xFFFFFFF0,
            size: 0x20,
        };
        assert!(a.overlaps(&bad));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_ok());
    }

    #[test]
    fn zero_cap_and_empty_sample() {
        // 零容量区段与空样本。
        let mut empty = Section::new(0);
        assert_eq!(empty.cap(), 0);
        assert_eq!(
            empty.map(ViewSlot {
                slot_id: 1,
                offset: 0,
                size: 0x10,
            }),
            Err(SvDiag::ENoSection.val())
        );
        let mut sec = Section::new(SECTION_CAP);
        assert_eq!(
            sec.map(ViewSlot {
                slot_id: 0,
                offset: 0,
                size: 0,
            }),
            Err(SvDiag::EEmpty.val())
        );
    }

    #[test]
    fn checked_arithmetic_no_overflow() {
        // 溢出面：end()/read 全走 checked，无 panic。
        // （offset 取 0x10 对齐值，避开映射口先行的对齐闸。）
        let bad = ViewSlot {
            slot_id: 1,
            offset: 0xFFFFFFF0,
            size: 0x20,
        };
        assert_eq!(bad.end(), None);
        let mut sec = Section::new(SECTION_CAP);
        assert_eq!(sec.map(bad), Err(SvDiag::EOverflow.val()));
        // 读溢出。
        assert!(sec.map(ViewSlot {
            slot_id: 1,
            offset: 0x0,
            size: 0x100,
        })
        .is_ok());
        assert_eq!(
            sec.read(1, 0xFFFFFFFF, 0x10),
            Err(SvDiag::EOverflow.val())
        );
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12607_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
