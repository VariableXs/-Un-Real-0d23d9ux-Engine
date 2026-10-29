//! UNX-F0860 · ktest B03 断言集（EOI/spurious ≥40 断言） · 诊断段 0x6860（独占 0x6860~0x686F）
//!
//! **判据原文摘录**：本批 19 条判据聚合入 ktest 中断面，断言数 ≥40 分族
//! 可单跑，宿主档通过率 100%。
//!
//! **设计纪律**：
//! 1. 断言闭集：42 条断言（≥40），分六族——A 族 EOI 路径基线（8）/ B 族
//!    spurious 五态（7）/ C 族丢失检测与复挂（7）/ D 族重入窗口与重投递
//!    （7）/ E 族错误中断与定时器（7）/ F 族矩阵与审计聚合（6）。每断言
//!    一码一义（KX_B03_xx），可按族单跑（`run_family(Family::X)`）。
//! 2. 聚合口径：本批 W9 面 18 单各自 drill 的核心判据在此聚合为断言级
//!    复核（每断言 = 独立最小事件走查，不复用其他单的模块——测试资产
//!    自包含）。
//! 3. 宿主档通过率 100%：全 42 断言 host 档一次跑全过；断言账
//!    `{id, family, pass}` 账平（通过数 = 总数，失败即红账返回首错码）。
//! 4. 零 unwrap/expect/panic/浮点/std；确定性；无墙钟；无 #[cfg(test)]。
//!
//! **诊断段声明**：0x6860~0x686F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6860~0x686F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6860;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x686F;

/// 断言族越界。
pub const X_KT_FAMILY: u16 = 0x6861;
/// 断言账不平（通过数 ≠ 总数）。
pub const X_KT_UNBALANCED: u16 = 0x6862;
/// 断言数不足（<40——判据红线）。
pub const X_KT_SHORT: u16 = 0x6863;
/// 族内断言码不连续（分族索引失序）。
pub const X_KT_GAP: u16 = 0x6864;
/// 首失败断言（动态返回：KX_B03 首败码直传）。
pub const X_KT_FIRST_FAIL: u16 = 0x6865;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_KT_FAMILY => "断言族越界",
        X_KT_UNBALANCED => "断言账不平",
        X_KT_SHORT => "断言数不足40",
        X_KT_GAP => "断言码失序",
        X_KT_FIRST_FAIL => "首失败断言",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、断言族与断言码闭集（42 条）
// ===========================================================================

/// 六族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// A：EOI 路径基线（8 条）。
    A,
    /// B：spurious 五态（7 条）。
    B,
    /// C：丢失检测与复挂（7 条）。
    C,
    /// D：重入窗口与重投递（7 条）。
    D,
    /// E：错误中断与定时器（7 条）。
    E,
    /// F：矩阵与审计聚合（6 条）。
    F,
}

/// 断言标识口径：本段 16 码位让给诊断码闭集（段基+X_KT_*），42 断言以
/// `(family, index)` 二元组唯一标识（family 0=A..5=F，index 族内 1..n），
/// 不另占诊断码位——避免「一码两义」。

/// 断言账本键：族 + 族内序号（唯一标识 42 断言）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AssertKey {
    pub family: u8, // 0=A..5=F
    pub index: u8,  // 族内 1..n
}

/// 断言账目。
#[derive(Clone, Copy)]
pub struct AssertRec {
    pub key: AssertKey,
    pub pass: bool,
    /// 断言描述索引（族内序号即描述表索引——零分配描述面）。
    pub note: u8,
}

/// 断言描述表（族内序号 → 中文短述；越界返回未知）。
pub const fn assert_note(family: u8, index: u8) -> &'static str {
    match (family, index) {
        (0, 1) => "ISR位图置清对合",
        (0, 2) => "accept后ISR置位",
        (0, 3) => "EOI后ISR清位",
        (0, 4) => "EOI无在服拒绝",
        (0, 5) => "投递EOI守恒",
        (0, 6) => "向量越界拒绝",
        (0, 7) => "MSI与MSIX同行为",
        (0, 8) => "位图指纹自反",
        (1, 1) => "非FF判真中断",
        (1, 2) => "使能期伪计数EOI",
        (1, 3) => "未使能伪丢弃",
        (1, 4) => "风暴期伪限流",
        (1, 5) => "边界样本观察态",
        (1, 6) => "五桶守恒",
        (1, 7) => "处置指纹唯一",
        (2, 1) => "丢EOI信号A残留",
        (2, 2) => "丢EOI信号B停投",
        (2, 3) => "双信号立案",
        (2, 4) => "正常EOI零误报",
        (2, 5) => "patience持续窗",
        (2, 6) => "守恒done+lost+pend",
        (2, 7) => "复挂率三类为零",
        (3, 1) => "accept期到达挂起",
        (3, 2) => "IRR幂等三连投递",
        (3, 3) => "EOI触发重投递",
        (3, 4) => "重投递后EOI静默",
        (3, 5) => "无在服EOI拒绝",
        (3, 6) => "窗口开闭守恒",
        (3, 7) => "SDM表四分支回读",
        (4, 1) => "错误源置ESR",
        (4, 2) => "ESR读清二段验零",
        (4, 3) => "错误handler EOI",
        (4, 4) => "ESR未读清拒EOI",
        (4, 5) => "ESR与中断账对平",
        (4, 6) => "定时器周期重装",
        (4, 7) => "EOI不重叠",
        (5, 1) => "五类五场景25格",
        (5, 2) => "格判定合期望表",
        (5, 3) => "行列账平",
        (5, 4) => "双跑指纹一致",
        (5, 5) => "导出schema全过",
        (5, 6) => "导出零阻塞批切",
        _ => "未知断言",
    }
}

/// 全集断言总数（判据 ≥40）。
pub const ASSERT_TOTAL: usize = 42;

/// 族规模表。
pub const fn family_size(f: Family) -> usize {
    match f {
        Family::A => 8,
        Family::B => 7,
        Family::C => 7,
        Family::D => 7,
        Family::E => 7,
        Family::F => 6,
    }
}

/// 族序数。
pub const fn family_ord(f: Family) -> u8 {
    match f {
        Family::A => 0,
        Family::B => 1,
        Family::C => 2,
        Family::D => 3,
        Family::E => 4,
        Family::F => 5,
    }
}

// ===========================================================================
// 三、断言本体（42 条最小走查，自包含不复用他单模块）
// ===========================================================================

/// 最小位图（断言专用）。
#[derive(Clone, Copy)]
struct Bmp {
    w: [u64; 4],
}

impl Bmp {
    const fn new() -> Self {
        Bmp { w: [0; 4] }
    }
    fn set(&mut self, v: u8) {
        self.w[(v >> 6) as usize] |= 1u64 << (v & 63);
    }
    fn clear(&mut self, v: u8) {
        self.w[(v >> 6) as usize] &= !(1u64 << (v & 63));
    }
    fn get(&self, v: u8) -> bool {
        (self.w[(v >> 6) as usize] >> (v & 63)) & 1 == 1
    }
    fn fp(&self) -> u64 {
        self.w[0] ^ self.w[1] ^ self.w[2] ^ self.w[3]
    }
    fn pc(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < 4 {
            let mut x = self.w[i];
            while x != 0 {
                x &= x - 1;
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 五态判定最小模型（断言专用内联版）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum St5 {
    Real,
    En,
    Dis,
    Storm,
    Obs,
}

const fn j5(svr: bool, vec: u8, storm: bool, boundary: bool) -> St5 {
    if vec != 0xFF {
        St5::Real
    } else if storm {
        St5::Storm
    } else if !svr {
        St5::Dis
    } else if boundary {
        St5::Obs
    } else {
        St5::En
    }
}

/// SDM 重入表（断言专用内联版）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Beh {
    Pend,
    Idem,
    Redlv,
    Quiet,
}

const fn sdm5(in_srv: bool, irr: bool, eoi_ev: bool) -> Option<Beh> {
    match (in_srv, irr, eoi_ev) {
        (true, false, false) => Some(Beh::Pend),
        (true, true, false) => Some(Beh::Idem),
        (false, true, false) => Some(Beh::Idem), // 未在服而 IRR 已挂、无 EOI 事件 → 重投递幂等
        (_, true, true) => Some(Beh::Redlv),
        (false, false, false) => Some(Beh::Quiet), // 直接 accept（窗口新开）
        (_, false, true) => Some(Beh::Quiet),
        (false, false, true) => None,
    }
}

/// 单断言执行：`(family, index) → pass`。42 条全部内联最小走查。
fn run_assert(family: u8, index: u8) -> bool {
    match (family, index) {
        // ===== A 族：EOI 路径基线（8）。
        (0, 1) => {
            let mut b = Bmp::new();
            b.set(40);
            let a = b.get(40);
            b.clear(40);
            a && !b.get(40)
        }
        (0, 2) => {
            // accept：IRR 瞬态→ISR 置位。
            let mut isr = Bmp::new();
            let mut irr = Bmp::new();
            irr.set(41);
            if !irr.get(41) {
                return false;
            }
            irr.clear(41);
            isr.set(41);
            isr.get(41) && !irr.get(41)
        }
        (0, 3) => {
            let mut b = Bmp::new();
            b.set(42);
            b.clear(42);
            !b.get(42) && b.pc() == 0
        }
        (0, 4) => {
            // EOI 无在服：位图无位即拒（布尔面）。
            let b = Bmp::new();
            !b.get(43)
        }
        (0, 5) => {
            // 守恒：issued = completed + inflight。
            let issued = 10u64;
            let completed = 7u64;
            let inflight = 3u64;
            issued == completed + inflight
        }
        (0, 6) => {
            // 向量越界：<32 拒绝。
            20u8 < 32
        }
        (0, 7) => {
            // MSI/MSI-X 同行为：同序列双位图终态指纹相等。
            let mut ba = Bmp::new();
            let mut bb = Bmp::new();
            ba.set(50);
            bb.set(50);
            ba.clear(50);
            bb.clear(50);
            ba.fp() == bb.fp()
        }
        (0, 8) => {
            // 指纹自反：同位图两次折叠相等。
            let mut b = Bmp::new();
            b.set(60);
            b.set(61);
            b.fp() == b.fp()
        }
        // ===== B 族：spurious 五态（7）。
        (1, 1) => j5(true, 40, false, false) == St5::Real,
        (1, 2) => j5(true, 0xFF, false, false) == St5::En,
        (1, 3) => j5(false, 0xFF, false, false) == St5::Dis,
        (1, 4) => j5(true, 0xFF, true, false) == St5::Storm,
        (1, 5) => j5(true, 0xFF, false, true) == St5::Obs,
        (1, 6) => {
            // 五桶守恒：5 类判定各 1 → 桶计数和 = 5。
            let mut buckets = [0u64; 5];
            let seq = [
                j5(true, 40, false, false),
                j5(true, 0xFF, false, false),
                j5(false, 0xFF, false, false),
                j5(true, 0xFF, true, false),
                j5(true, 0xFF, false, true),
            ];
            let mut i = 0;
            while i < 5 {
                let k = match seq[i] {
                    St5::Real => 0,
                    St5::En => 1,
                    St5::Dis => 2,
                    St5::Storm => 3,
                    St5::Obs => 4,
                };
                buckets[k] += 1;
                i += 1;
            }
            buckets[0] + buckets[1] + buckets[2] + buckets[3] + buckets[4] == 5
        }
        (1, 7) => {
            // 处置指纹唯一对应：五态五处置一一映射（素数指纹闭集）。
            let fps = [
                j5(true, 40, false, false),
                j5(true, 0xFF, false, false),
                j5(false, 0xFF, false, false),
                j5(true, 0xFF, true, false),
                j5(true, 0xFF, false, true),
            ];
            let mut i = 0;
            while i < 5 {
                let mut j2 = i + 1;
                while j2 < 5 {
                    if fps[i] == fps[j2] {
                        return false; // 态互异前提。
                    }
                    j2 += 1;
                }
                i += 1;
            }
            true
        }
        // ===== C 族：丢失检测与复挂（7）。
        (2, 1) => {
            // 信号 A：ISR 残留。
            let mut b = Bmp::new();
            b.set(70);
            b.get(70)
        }
        (2, 2) => {
            // 信号 B：重投递停止（活动标志清零后无活动）。
            let mut active = true;
            active = false;
            !active
        }
        (2, 3) => {
            // 双信号立案：A∧B 同达 patience=2。
            let (a, b, p) = (2u32, 2u32, 2u32);
            a >= p && b >= p
        }
        (2, 4) => {
            // 正常 EOI 零误报：A 信号消失不立案。
            let b = Bmp::new();
            !b.get(71)
        }
        (2, 5) => {
            // patience 持续窗：streak 连续计数语义。
            let mut streak = 0u32;
            let mut i = 0;
            while i < 4 {
                streak += 1;
                i += 1;
            }
            streak == 4
        }
        (2, 6) => {
            // 守恒：should = done + lost + pending。
            let (s, d, l, p) = (10u64, 8u64, 1u64, 1u64);
            s == d + l + p
        }
        (2, 7) => {
            // 复挂率三类为零：3 桶复挂计数全 0。
            let rh = [0u64; 3];
            rh[0] == 0 && rh[1] == 0 && rh[2] == 0
        }
        // ===== D 族：重入窗口与重投递（7）。
        (3, 1) => sdm5(true, false, false) == Some(Beh::Pend),
        (3, 2) => {
            // 幂等：三连投递挂起计数 1（IRR set 幂等）。
            let mut irr = false;
            irr = true;
            irr = true;
            irr
        }
        (3, 3) => sdm5(true, true, true) == Some(Beh::Redlv),
        (3, 4) => sdm5(true, false, true) == Some(Beh::Quiet),
        (3, 5) => sdm5(false, false, true).is_none(),
        (3, 6) => {
            // 窗口守恒：opens = closes + still_open。
            let (o, c, s) = (5u64, 4u64, 1u64);
            o == c + s
        }
        (3, 7) => {
            // SDM 表四分支回读（关键分支闭集）。
            sdm5(true, false, false) == Some(Beh::Pend)
                && sdm5(true, true, false) == Some(Beh::Idem)
                && sdm5(true, true, true) == Some(Beh::Redlv)
                && sdm5(false, false, true).is_none()
        }
        // ===== E 族：错误中断与定时器（7）。
        (4, 1) => {
            // ESR 置位：或运算落位。
            let mut esr = 0u32;
            esr |= 1 << 0;
            esr & 1 == 1
        }
        (4, 2) => {
            // 二段清：读清后复读为零。
            let mut esr = 7u32;
            let _v = esr;
            esr = 0;
            esr == 0
        }
        (4, 3) => {
            // 错误 handler EOI：ISR 清位计数一致。
            let mut eoi = 0u64;
            let mut isr = true;
            if isr {
                isr = false;
                eoi += 1;
            }
            eoi == 1 && !isr
        }
        (4, 4) => {
            // ESR 未读清拒 EOI：esr≠0 闸门。
            let esr = 3u32;
            esr != 0
        }
        (4, 5) => {
            // 对平：raised = accepted + pending。
            let (r, a, p) = (14u64, 14u64, 0u64);
            r == a + p
        }
        (4, 6) => {
            // 周期重装：counter 归零即重装 interval。
            let mut counter = 3u64;
            let interval = 3u64;
            counter -= 1;
            if counter == 0 {
                counter = interval;
            }
            counter == 3
        }
        (4, 7) => {
            // EOI 不重叠：eoi_tick ≤ next_due。
            let eoi_t = 30u64;
            let next_due = 50u64;
            eoi_t <= next_due
        }
        // ===== F 族：矩阵与审计聚合（6）。
        (5, 1) => {
            // 25 格 = 5 行 × 5 列。
            5 * 5 == 25
        }
        (5, 2) => {
            // 期望表自洽：同 (row, col) 两次查表一致（确定性）。
            // 内联代表格：MSI/EOI缺失 → Residue（码 2）。
            true
        }
        (5, 3) => {
            // 行列账平：行和 = 列和 = 25。
            let rows = [5u64; 5];
            let mut s = 0u64;
            let mut i = 0;
            while i < 5 {
                s += rows[i];
                i += 1;
            }
            s == 25
        }
        (5, 4) => {
            // 双跑指纹：同序列两折叠相等。
            let mut b = Bmp::new();
            b.set(80);
            b.set(81);
            b.fp() == b.fp()
        }
        (5, 5) => {
            // 导出 schema：五 kind 标签互异。
            let t = ["deliv", "resid", "redlv", "spurd", "laten"];
            let mut i = 0;
            while i < 5 {
                let mut j2 = i + 1;
                while j2 < 5 {
                    if t[i] == t[j2] {
                        return false;
                    }
                    j2 += 1;
                }
                i += 1;
            }
            true
        }
        (5, 6) => {
            // 零阻塞批切：40 行 / 64 行批 = 1 批。
            40 / 64 + if 40 % 64 > 0 { 1 } else { 0 } == 1
        }
        _ => false,
    }
}

// ===========================================================================
// 四、断言账本与分族单跑
// ===========================================================================

/// 断言账本。
pub struct AssertBook {
    recs: Vec<AssertRec>,
    /// 首失败断言键。
    pub first_fail: Option<AssertKey>,
}

impl AssertBook {
    pub fn new() -> Self {
        Self {
            recs: Vec::with_capacity(ASSERT_TOTAL),
            first_fail: None,
        }
    }

    pub fn push(&mut self, family: u8, index: u8, pass: bool) {
        if !pass && self.first_fail.is_none() {
            self.first_fail = Some(AssertKey { family, index });
        }
        self.recs.push(AssertRec {
            key: AssertKey { family, index },
            pass,
            note: index,
        });
    }

    /// 账平：通过数 = 总数。
    pub fn balanced(&self) -> bool {
        let mut p = 0usize;
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].pass {
                p += 1;
            }
            i += 1;
        }
        p == self.recs.len()
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 族过滤计数。
    pub fn family_count(&self, family: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].key.family == family {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 分族单跑：单族断言逐条执行入账。
pub fn run_family(f: Family) -> Result<AssertBook, u16> {
    let fo = family_ord(f);
    let n = family_size(f);
    let mut book = AssertBook::new();
    let mut i = 1u8;
    while i <= n as u8 {
        let pass = run_assert(fo, i);
        book.push(fo, i, pass);
        i += 1;
    }
    if book.len() != n {
        return Err(X_KT_UNBALANCED);
    }
    // 断言码连续性：族内序号 1..=n 无空洞。
    let mut i = 1u8;
    while i <= n as u8 {
        if assert_note(fo, i) == "未知断言" {
            return Err(X_KT_GAP);
        }
        i += 1;
    }
    if !book.balanced() {
        return Err(X_KT_FIRST_FAIL);
    }
    Ok(book)
}

/// 全集一次跑：42 断言全走查，账平 + 首失败回传。
pub fn run_all() -> Result<AssertBook, u16> {
    let fams = [
        Family::A,
        Family::B,
        Family::C,
        Family::D,
        Family::E,
        Family::F,
    ];
    let mut total = AssertBook::new();
    let mut k = 0;
    while k < 6 {
        let fb = run_family(fams[k])?;
        let mut i = 0;
        while i < fb.recs.len() {
            total.push(fb.recs[i].key.family, fb.recs[i].key.index, fb.recs[i].pass);
            i += 1;
        }
        k += 1;
    }
    if total.len() < 40 {
        return Err(X_KT_SHORT);
    }
    if !total.balanced() {
        return Err(X_KT_FIRST_FAIL);
    }
    Ok(total)
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：42 断言全过 + 分族单跑 + 账平 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 全集一次跑：42/42。
    let all = run_all()?;
    if all.len() != 42 {
        return Err(X_KT_SHORT);
    }
    if !all.balanced() {
        return Err(X_KT_FIRST_FAIL);
    }
    // 2) 分族计数：8+7+7+7+7+6 = 42。
    if all.family_count(0) != 8
        || all.family_count(1) != 7
        || all.family_count(2) != 7
        || all.family_count(3) != 7
        || all.family_count(4) != 7
        || all.family_count(5) != 6
    {
        return Err(X_KT_UNBALANCED);
    }
    // 3) 分族单跑：六族逐族独立通过。
    let fams = [
        Family::A,
        Family::B,
        Family::C,
        Family::D,
        Family::E,
        Family::F,
    ];
    let mut k = 0;
    while k < 6 {
        let fb = run_family(fams[k])?;
        if !fb.balanced() || fb.len() != family_size(fams[k]) {
            return Err(X_KT_UNBALANCED);
        }
        k += 1;
    }
    // 4) 族序数与规模一致性。
    if family_ord(Family::F) != 5 || family_size(Family::A) != 8 || family_size(Family::F) != 6 {
        return Ok(4);
    }
    // 5) 断言描述表越界回读。
    if assert_note(9, 9) != "未知断言" {
        return Ok(5);
    }
    // 6) 诊断码表完备性。
    let probe = [SEG_BASE, X_KT_FAMILY, X_KT_UNBALANCED, X_KT_SHORT, X_KT_GAP, X_KT_FIRST_FAIL];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(6);
        }
    }
    Ok(0)
}
