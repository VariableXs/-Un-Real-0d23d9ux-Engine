//! unx0424_nmiist2 —— UNX-F0424 NMI 门与 IST2 栈。
//! 锚点（CoRun Unxreal 主册）：UNX-F0424 · NMI 门与 IST2 栈 · 目标 220 行
//! 判据原文：UNX-F0424-J1 NMI 走 IST2 专用栈：栈配置核（尺寸/
//!           对齐/哨兵）与 IST1 同规；NMI 嵌套屏蔽核（NMI 期
//!           再来 NMI 被硬件屏蔽模型拦截——计数封顶 1，屏蔽窗
//!           内到达立案）；NMI 与 #DF 栈分离核（IST2 指针 ≠
//!           IST1 指针，共享即立案）；NMI 上下文预留区（定容
//!           快照区，写入不越界）；门属性核（NMI 门 = 中断门
//!           + 屏蔽 IF 位语义模型）。
//! 组件：IST2 栈（复用 IstStack 规）+ NMI 屏蔽窗（计数封顶）+
//!       栈分离核（IST1/IST2 指针对拍）+ 快照区（定容写入）+
//!       门属性账。
//! 职责定位：NMI 是「不可屏蔽的最高优先级中断」——正因为不可
//!   屏蔽，它的处理环境必须绝对可靠：专用栈（IST2）、嵌套屏
//!   蔽（硬件只屏蔽一层的语义模型）、与 #DF 栈物理分离（NMI
//!   打进 #DF 现场就是灾难叠加）。快照区定容是取证纪律：NMI
//!   期采集的现场必须有限度，不许在故障栈上撒野。
//! 性能逐项分解：配置核 O(1)；屏蔽判定 O(1)；分离核 O(1)；
//!   快照写 O(写入量)。
//! 诊断码段：0x21300（0x21301~0x21309 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const N2_VERSION: &str = "unx0424_nmiist2-v1";

/// 诊断段基址（0x21300 段 · 本模块独占）。
pub const N2_SEG_BASE: u32 = 0x21300;
pub const XC_NESTED_NMI: u32 = 0x21301; // 嵌套 NMI 到达
pub const XC_STACK_SHARED: u32 = 0x21302; // IST1/IST2 共享
pub const XC_SNAP_OVF: u32 = 0x21303; // 快照区越界
pub const XC_GATE_BAD: u32 = 0x21304; // 门属性错
pub const XC_SIZE_SHORT: u32 = 0x21305; // 栈尺寸不足
pub const XC_ALIGN_BAD: u32 = 0x21306; // 不对齐
pub const XC_MAGIC_BRK: u32 = 0x21307; // 哨兵破
pub const XC_MAGIC: u32 = 0x21308; // 账魔数破
pub const XC_WINDOW_LEAK: u32 = 0x21309; // 屏蔽窗未关

/// IST 栈模型（与 0423 同规的独立实现）。
#[derive(Clone, Debug)]
pub struct Ist2Stack {
    pub pages: u32,
    pub base: u64,
    pub canary: u64,
}

pub const NMI_CANARY: u64 = 0xFEED_FACE_CAFE_0002;
pub const NMI_MIN_PAGES: u32 = 4;

impl Ist2Stack {
    /// 建。
    pub fn new(pages: u32, base: u64) -> Result<Ist2Stack, u32> {
        if pages < NMI_MIN_PAGES {
            return Err(XC_SIZE_SHORT);
        }
        if base % 16 != 0 {
            return Err(XC_ALIGN_BAD);
        }
        Ok(Ist2Stack { pages, base, canary: NMI_CANARY })
    }

    /// 容量。
    pub fn cap(&self) -> u32 {
        self.pages * 4096
    }

    /// 哨兵核。
    pub fn canary_ok(&self) -> Result<(), u32> {
        if self.canary != NMI_CANARY {
            return Err(XC_MAGIC_BRK);
        }
        Ok(())
    }

    /// 栈顶。
    pub fn top(&self) -> u64 {
        self.base + self.cap() as u64
    }

    /// 踩哨兵（演练）。
    pub fn corrupt(&mut self) {
        self.canary ^= 0xFF;
    }
}

/// NMI 屏蔽窗：进入 NMI 开窗（计数 +1），退出关窗（-1）；
/// 窗内再入 = 嵌套立案。计数封顶 1。
pub struct NmiWindow {
    depth: u32,
    cases: u32,
}

impl NmiWindow {
    /// 建。
    pub fn new() -> NmiWindow {
        NmiWindow { depth: 0, cases: 0 }
    }

    /// NMI 到达：开窗；已在窗内 = 嵌套立案。
    pub fn enter(&mut self) -> Result<(), u32> {
        if self.depth >= 1 {
            self.cases += 1;
            return Err(XC_NESTED_NMI);
        }
        self.depth += 1;
        Ok(())
    }

    /// NMI 返回：关窗。
    pub fn leave(&mut self) -> Result<(), u32> {
        if self.depth == 0 {
            return Err(XC_WINDOW_LEAK);
        }
        self.depth -= 1;
        Ok(())
    }

    /// 窗深。
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// 立案数。
    pub fn case_count(&self) -> u32 {
        self.cases
    }

    /// 窗未关核（收官检查）。
    pub fn closed_ok(&self) -> Result<(), u32> {
        if self.depth != 0 {
            return Err(XC_WINDOW_LEAK);
        }
        Ok(())
    }
}

/// 栈分离核：IST1 与 IST2 指针必须不同（区间不相交更强）。
pub fn stack_disjoint(ist1_top: u64, ist2: &Ist2Stack) -> Result<(), u32> {
    let lo = ist2.base;
    let hi = ist2.top();
    // IST1 栈区间假设 [ist1_top - 64K, ist1_top)（模型）。
    let ist1_lo = ist1_top.saturating_sub(64 * 1024);
    if ist1_top > lo && ist1_lo < hi {
        return Err(XC_STACK_SHARED);
    }
    Ok(())
}

/// NMI 快照区：定容写入。
pub struct SnapZone {
    cap: usize,
    data: Vec<u8>,
}

impl SnapZone {
    /// 建：容量（字节）。
    pub fn new(cap: usize) -> SnapZone {
        SnapZone { cap, data: Vec::new() }
    }

    /// 写入：越界拒。
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, u32> {
        if self.data.len() + bytes.len() > self.cap {
            return Err(XC_SNAP_OVF);
        }
        for &b in bytes.iter() {
            self.data.push(b);
        }
        Ok(self.data.len())
    }

    /// 已用量。
    pub fn used(&self) -> usize {
        self.data.len()
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// 门属性账：NMI 门 = 类型 14（中断门模型）+ 屏蔽 IF。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateAttr {
    /// 类型码（14 = NMI/中断门模型）。
    pub typ: u8,
    /// 屏蔽 IF 位（NMI 门必须置位）。
    pub mask_if: bool,
    /// DPL（0 = 内核态）。
    pub dpl: u8,
}

impl GateAttr {
    /// 属性核。
    pub fn valid(&self) -> Result<(), u32> {
        if self.typ != 14 {
            return Err(XC_GATE_BAD);
        }
        if !self.mask_if {
            return Err(XC_GATE_BAD);
        }
        if self.dpl != 0 {
            return Err(XC_GATE_BAD);
        }
        Ok(())
    }
}

pub fn run_unx0424_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0424_nmiist2");
    // 族一：IST2 标准配置。
    let s2 = match Ist2Stack::new(4, 0x5000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建栈", false, "拒");
            return cs;
        }
    };
    cs.add("容量 16K", s2.cap() == 16 * 1024, "4 页");
    cs.add("哨兵完好", s2.canary_ok().is_ok(), "NMI_CANARY 在");
    cs.add("栈顶 0x5000_4000", s2.top() == 0x5000_4000, "基址 + 容量");
    // 族二：与 IST1 分离。
    let ist1_top: u64 = 0x1000_4000;
    cs.add("IST 分离过", stack_disjoint(ist1_top, &s2).is_ok(), "0x1000 段 vs 0x5000 段");
    // 共享案：IST2 落在 IST1 区间内。
    let shared = match Ist2Stack::new(4, 0x1000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建共享栈", false, "拒");
            return cs;
        }
    };
    cs.add("共享立案", stack_disjoint(ist1_top, &shared) == Err(XC_STACK_SHARED), "同段重叠");
    // 族三：NMI 屏蔽窗。
    let mut w = NmiWindow::new();
    cs.add("首例 NMI 开窗", w.enter().is_ok(), "depth 1");
    cs.add("窗深 1", w.depth() == 1, "封顶语义");
    cs.add("嵌套 NMI 立案", w.enter() == Err(XC_NESTED_NMI), "屏蔽窗内");
    cs.add("立案数 1", w.case_count() == 1, "一次一案");
    cs.add("关窗成", w.leave().is_ok(), "depth 0");
    cs.add("再入窗成", w.enter().is_ok(), "窗可复用");
    let _ = w.leave();
    cs.add("窗全关核", w.closed_ok().is_ok(), "收官检查");
    cs.add("空窗关拒", w.leave() == Err(XC_WINDOW_LEAK), "深度 0");
    // 族四：快照区。
    let mut z = SnapZone::new(64);
    cs.add("写 16 字节", z.write(&[0xABu8; 16]) == Ok(16), "余 48");
    cs.add("再写 48 字节满", z.write(&[0xCDu8; 48]) == Ok(64), "贴满");
    cs.add("越界拒", z.write(&[0x11]) == Err(XC_SNAP_OVF), "65 > 64");
    cs.add("用量 64", z.used() == 64, "满容");
    cs.add("清空复用", {
        z.clear();
        z.write(&[0x22; 8]) == Ok(8)
    }, "清后可写");
    // 族五：门属性。
    let g = GateAttr { typ: 14, mask_if: true, dpl: 0 };
    cs.add("门属性过", g.valid().is_ok(), "14 + 屏 IF + DPL0");
    let g2 = GateAttr { typ: 15, mask_if: true, dpl: 0 };
    cs.add("类型错拒", g2.valid() == Err(XC_GATE_BAD), "typ=15");
    let g3 = GateAttr { typ: 14, mask_if: false, dpl: 0 };
    cs.add("不屏 IF 拒", g3.valid() == Err(XC_GATE_BAD), "NMI 门必屏");
    let g4 = GateAttr { typ: 14, mask_if: true, dpl: 3 };
    cs.add("DPL 非 0 拒", g4.valid() == Err(XC_GATE_BAD), "dpl=3");
    // 族六：配置拒绝与哨兵。
    cs.add("尺寸不足拒", matches!(Ist2Stack::new(3, 0x5000), Err(XC_SIZE_SHORT)), "3 < 4");
    cs.add("不对齐拒", matches!(Ist2Stack::new(4, 0x5004), Err(XC_ALIGN_BAD)), "%16=4");
    let mut brk = match Ist2Stack::new(4, 0x6000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建栈2", false, "拒");
            return cs;
        }
    };
    brk.corrupt();
    cs.add("哨兵踩破检出", brk.canary_ok() == Err(XC_MAGIC_BRK), "异或 0xFF");
    cs
}
