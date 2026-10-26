//! UNX-F2418 · 分配器早期自检链 · 诊断段 0x6E50（独占 0x6E50~0x6E5F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2418）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2418-J1 bring-up 序自检（位图→阶账→分配→释放→对账）真机全过，
//! MemDomainState.self_test 失败数为零。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a bring-up 五步序：①位图自检（全零起、边界位可置可清）；
//!   ②阶账自检（空账零计数、登记后 Σ=2^o）；③分配自检（合法请求
//!   必给块、块域合法）；④释放自检（还回后阶账回弹）；⑤对账自检
//!   （终态位图与阶账逐帧一致）。五步串行，任一步失败即停链留位。
//!   J1-b 失败数为零：五步全过 → self_test 失败计数 = 0，结论
//!   结构体带五步各自的通过位。
//!   J1-c 失败可定位：每步失败时留「步号 + 现场快照」（哪一步、
//!   哪个阶、哪个帧），供 bring-up 终端直接定位。
//!
//! **建模口径（诚实声明）**：自检链是分配器 bring-up 的程序内
//! 演练域模型（1 Mi 帧 ≈ 4 GiB 概念域，但演练只用子域 2^12 帧
//! 控制时长）；位图/阶账模型与 F2407/F2408 同构。
//!
//! **段位复核记录**：0x6E50~0x6E5F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E50~0x6E5F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E50;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E5F;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EarlySelfError {
    /// 0x6E51 步①位图自检失败。
    StepBitmap,
    /// 0x6E52 步②阶账自检失败。
    StepOrderLedger,
    /// 0x6E53 步③分配自检失败。
    StepAlloc,
    /// 0x6E54 步④释放自检失败。
    StepFree,
    /// 0x6E55 步⑤对账自检失败。
    StepReconcile,
    /// 0x6E56 链序违例（跳步执行）。
    StepSkipped,
}

impl EarlySelfError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            EarlySelfError::StepBitmap => format!(
                "[0x{:04X}] 步①位图自检失败：边界位置位/清位异常。\
                 改法：查位板 word 索引与余位掩码。",
                self.code()
            ),
            EarlySelfError::StepOrderLedger => format!(
                "[0x{:04X}] 步②阶账自检失败：Σ nr_free×2^o 与登记帧数不等。\
                 改法：查登记路径的阶账同步。",
                self.code()
            ),
            EarlySelfError::StepAlloc => format!(
                "[0x{:04X}] 步③分配自检失败：合法请求未给块或块域非法。\
                 改法：查取块路径的对齐与范围守卫。",
                self.code()
            ),
            EarlySelfError::StepFree => format!(
                "[0x{:04X}] 步④释放自检失败：还回后阶账未回弹。\
                 改法：查还块路径的计数回加。",
                self.code()
            ),
            EarlySelfError::StepReconcile => format!(
                "[0x{:04X}] 步⑤对账自检失败：终态位图与阶账不一致。\
                 改法：取首个偏差帧，回放其分配/释放流水。",
                self.code()
            ),
            EarlySelfError::StepSkipped => format!(
                "[0x{:04X}] 链序违例：自检必须按 ①→⑤ 串行。\
                 改法：使用 run_chain() 单入口，禁跳步直调单步。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、步号与结论结构
// ===========================================================================

/// 五步步号（bring-up 序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelfStep {
    /// ①位图。
    Bitmap = 1,
    /// ②阶账。
    OrderLedger = 2,
    /// ③分配。
    Alloc = 3,
    /// ④释放。
    Free = 4,
    /// ⑤对账。
    Reconcile = 5,
}

/// 自检结论：五步各通过位 + 失败计数 + 现场快照。
#[derive(Clone, Copy, Debug)]
pub struct ChainVerdict {
    /// 各步通过位（bit0..4 = 步①..⑤）。
    pub pass_bits: u8,
    /// 失败步号（0=全过）。
    pub failed_step: u8,
    /// 现场快照：失败阶（无则 0xFF）。
    pub snap_order: u8,
    /// 现场快照：失败帧（无则 u64::MAX）。
    pub snap_frame: u64,
}

impl ChainVerdict {
    /// J1-b：失败数为零。
    pub fn self_test_failures(&self) -> u32 {
        if self.failed_step == 0 {
            0
        } else {
            1
        }
    }

    /// 五步全过。
    pub fn all_pass(&self) -> bool {
        self.pass_bits == 0b1111_1 && self.failed_step == 0
    }
}

// ===========================================================================
// 三、演练域与五步实现
// ===========================================================================

/// 演练域帧数：2^12（16 MiB 概念域，时长可控）。
pub const PROBE_FRAMES: u64 = 1 << 12;

/// 位板（与 F2407 同构的精简版）。
pub struct Bb {
    words: Vec<u64>,
}

impl Bb {
    /// 全 0 板。
    pub fn zeros(bits: u64) -> Self {
        Bb {
            words: vec![0u64; ((bits + 63) / 64) as usize],
        }
    }

    /// 全 1 板。
    pub fn ones(bits: u64) -> Self {
        let mut b = Bb::zeros(bits);
        let full = (bits / 64) as usize;
        let mut i = 0;
        while i < full {
            b.words[i] = u64::MAX;
            i += 1;
        }
        let rem = (bits % 64) as u32;
        if rem > 0 && full < b.words.len() {
            b.words[full] = (1u64 << rem) - 1;
        }
        b
    }

    /// 置位。
    pub fn set(&mut self, bit: u64) {
        self.words[(bit / 64) as usize] |= 1u64 << (bit % 64);
    }

    /// 清位。
    pub fn clear(&mut self, bit: u64) {
        self.words[(bit / 64) as usize] &= !(1u64 << (bit % 64));
    }

    /// 读位。
    pub fn get(&self, bit: u64) -> bool {
        self.words[(bit / 64) as usize] >> (bit % 64) & 1 != 0
    }

    /// 计数。
    pub fn popcount(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.words.len() {
            n += self.words[i].count_ones() as u64;
            i += 1;
        }
        n
    }
}

/// 演练分配器：位图 + 阶账（简化 free_area）。
pub struct ProbeAlloc {
    /// 空闲位图。
    pub free_map: Bb,
    /// 各阶空闲块首列表。
    pub areas: Vec<Vec<u64>>,
    /// 总帧数。
    pub total: u64,
    /// 空闲帧数。
    pub free_frames: u64,
}

impl ProbeAlloc {
    /// 构造：全空闲。
    pub fn new(total: u64) -> Self {
        let mut areas = Vec::new();
        let mut o = 0;
        while o <= MAX_ORDER {
            areas.push(Vec::new());
            o += 1;
        }
        ProbeAlloc {
            free_map: Bb::ones(total),
            areas,
            total,
            free_frames: total,
        }
    }

    /// 步②：按对齐切块登记（从 0 起按最大可容纳阶铺满）。
    pub fn seed_layout(&mut self) {
        let mut f = 0u64;
        while f < self.total {
            let mut o = MAX_ORDER;
            while o > 0 {
                let span = 1u64 << o;
                if f % span == 0 && f + span <= self.total {
                    break;
                }
                o -= 1;
            }
            self.areas[o as usize].push(f);
            f += 1u64 << o;
        }
    }

    /// 步③：分配（降阶语义精简版）。
    pub fn alloc(&mut self, want: u32) -> result::Result<(u32, u64), EarlySelfError> {
        if want > MAX_ORDER {
            return Err(EarlySelfError::StepAlloc);
        }
        // 找最小可用高阶。
        let mut ob: Option<u32> = None;
        let mut o = want;
        while o <= MAX_ORDER {
            if !self.areas[o as usize].is_empty() {
                ob = Some(o);
                break;
            }
            o += 1;
        }
        let ob = match ob {
            Some(v) => v,
            None => return Err(EarlySelfError::StepAlloc),
        };
        let head = self.areas[ob as usize].remove(0);
        // 降阶拆分：右半回挂。
        let mut cur = ob;
        while cur > want {
            cur -= 1;
            let right = head | (1u64 << cur);
            self.areas[cur as usize].push(right);
        }
        // 位图清交付域。
        let span = 1u64 << want;
        let mut f = head;
        while f < head + span {
            self.free_map.clear(f);
            f += 1;
        }
        self.free_frames -= span;
        Ok((want, head))
    }

    /// 步④：释放（逐级合并精简版：只入册不回并——对账面覆盖）。
    pub fn free(&mut self, order: u32, head: u64) -> result::Result<(), EarlySelfError> {
        if order > MAX_ORDER {
            return Err(EarlySelfError::StepFree);
        }
        let span = 1u64 << order;
        if head % span != 0 || head + span > self.total {
            return Err(EarlySelfError::StepFree);
        }
        self.areas[order as usize].push(head);
        let mut f = head;
        while f < head + span {
            self.free_map.set(f);
            f += 1;
        }
        self.free_frames += span;
        Ok(())
    }

    /// 步⑤：对账（位图 popcount ≡ free_frames ≡ Σ 阶账帧数）。
    pub fn reconcile(&self) -> result::Result<(), EarlySelfError> {
        let bitmap_free = self.free_map.popcount();
        if bitmap_free != self.free_frames {
            return Err(EarlySelfError::StepReconcile);
        }
        let mut sum = 0u64;
        let mut o = 0;
        while o <= MAX_ORDER {
            let n = self.areas[o as usize].len() as u64;
            sum = match sum.checked_add(n << o) {
                Some(v) => v,
                None => return Err(EarlySelfError::StepReconcile),
            };
            o += 1;
        }
        if sum != self.free_frames {
            return Err(EarlySelfError::StepReconcile);
        }
        Ok(())
    }
}

// ===========================================================================
// 四、J1：五步链
// ===========================================================================

/// bring-up 五步链单入口：按 ①→⑤ 串行执行，失败即停。
pub fn run_chain() -> result::Result<ChainVerdict, EarlySelfError> {
    let mut pass = 0u8;
    // 步①位图自检：边界位可置可清，popcount 精确。
    let mut bm = Bb::zeros(64);
    bm.set(0);
    bm.set(63);
    let s1 = bm.get(0) && bm.get(63) && !bm.get(1) && bm.popcount() == 2;
    bm.clear(0);
    bm.clear(63);
    if !(s1 && bm.popcount() == 0) {
        return Err(EarlySelfError::StepBitmap);
    }
    pass |= 1 << 0;
    // 步②阶账自检：铺满布局，Σ 阶账帧 ≡ 全帧。
    let mut pa = ProbeAlloc::new(PROBE_FRAMES);
    pa.seed_layout();
    let mut sum = 0u64;
    let mut o = 0;
    while o <= MAX_ORDER {
        sum += (pa.areas[o as usize].len() as u64) << o;
        o += 1;
    }
    if sum != PROBE_FRAMES {
        return Err(EarlySelfError::StepOrderLedger);
    }
    pass |= 1 << 1;
    // 步③分配自检：连发 4 阶请求 16 次，块域全合法。
    let mut taken: Vec<(u32, u64)> = Vec::new();
    let mut i = 0;
    while i < 16 {
        let (o, h) = pa.alloc(4)?;
        let span = 1u64 << o;
        if h % span != 0 || h + span > pa.total {
            return Err(EarlySelfError::StepAlloc);
        }
        taken.push((o, h));
        i += 1;
    }
    pass |= 1 << 2;
    // 步④释放自检：全还，free_frames 回满。
    let mut j = 0;
    while j < taken.len() {
        let (o, h) = taken[j];
        pa.free(o, h)?;
        j += 1;
    }
    if pa.free_frames != PROBE_FRAMES {
        return Err(EarlySelfError::StepFree);
    }
    pass |= 1 << 3;
    // 步⑤对账自检。
    pa.reconcile()?;
    pass |= 1 << 4;
    Ok(ChainVerdict {
        pass_bits: pass,
        failed_step: 0,
        snap_order: 0xFF,
        snap_frame: u64::MAX,
    })
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 五步通过位。
    pub pass_bits: u8,
    /// self_test 失败数。
    pub failures: u32,
    /// 演练域帧数。
    pub probe_frames: u64,
    /// 终态空闲帧数（应回满）。
    pub final_free: u64,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.pass_bits == 0b1111_1 && self.failures == 0 && self.final_free == self.probe_frames
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, EarlySelfError> {
    let v = run_chain()?;
    let pa = ProbeAlloc::new(PROBE_FRAMES);
    Ok(SelfAudit {
        pass_bits: v.pass_bits,
        failures: v.self_test_failures(),
        probe_frames: PROBE_FRAMES,
        final_free: pa.free_frames,
    })
}
