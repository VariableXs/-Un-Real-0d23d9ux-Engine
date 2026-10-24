//! unx0356_apconverge —— UNX-F0356 AP 侧串口汇聚（日志多源标记）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0356 · AP 串口汇聚 · 目标 240 行
//! 判据原文：UNX-F0356-J1 多 AP 日志汇聚到单一串口：每行带源核
//!           标记（BSP=0 / AP=1..N）；同拍并发写按核号仲裁（低
//!           号优先）保证行原子（半行不许拼行）；汇聚账可按源
//!           核过滤回放。
//! 组件：日志行（源核 + 序号 + 载荷）+ 汇聚仲裁器（同拍冲突按
//!       核号定序 + 行原子核）+ 源过滤回放 + 每源序号账。
//! 职责定位：串口汇聚是「多核日志不串行化但也不串字」的平衡点
//!   ——行原子是底线：两核的半行拼在一起比丢行更毒（读完不
//!   知道哪半行是谁的）。仲裁让冲突可预期：低号优先是固定规则，
//!   不是运气。
//! 性能逐项分解：入行 O(账深)；仲裁 O(冲突数)；回放 O(账深)。
//! 诊断码段：0x20600（0x20601~0x20609 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AC_VERSION: &str = "unx0356_apconverge-v1";

/// 诊断段基址（0x20600 段 · 本模块独占）。
pub const AC_SEG_BASE: u32 = 0x20600;
pub const XC_CORE_OOR: u32 = 0x20601; // 核号越域
pub const XC_LINE_EMPTY: u32 = 0x20602; // 空行拒
pub const XC_LINE_LONG: u32 = 0x20603; // 行超长
pub const XC_SEQ_GAP: u32 = 0x20604; // 源内序号跳号
pub const XC_ARBIT_LOST: u32 = 0x20605; // 仲裁落败（让行）
pub const XC_ATOMic_BROKEN: u32 = 0x20606; // 行原子破
pub const XC_CORE_ZERO: u32 = 0x20607; // 核号 0 拒（0 留 BSP 语义）
pub const XC_REPLAY_DRIFT: u32 = 0x20608; // 回放漂移
pub const XC_TIE_NO_ORDER: u32 = 0x20609; // 同号无序

/// 核号闭集：0=BSP，1~15=AP。
pub const CORE_MAX: u32 = 15;
/// 行长上限。
pub const LINE_MAX: usize = 64;

/// 汇聚日志行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConvLine {
    pub core: u32,
    /// 源内序号（每核独立单调）。
    pub seq: u32,
    pub payload: Vec<u8>,
}

impl ConvLine {
    pub fn new(core: u32, seq: u32, payload: &[u8]) -> Result<ConvLine, u32> {
        if core > CORE_MAX {
            return Err(XC_CORE_OOR);
        }
        if payload.is_empty() {
            return Err(XC_LINE_EMPTY);
        }
        if payload.len() > LINE_MAX {
            return Err(XC_LINE_LONG);
        }
        Ok(ConvLine { core, seq, payload: payload.to_vec() })
    }

    /// 行原子核：载荷内不许有换行（换行 = 行界，载荷内出现即
    /// 拼行风险源，显式拒）。
    pub fn atomic_ok(&self) -> bool {
        !self.payload.contains(&b'\n')
    }
}

/// 汇聚仲裁器。
pub struct Converge {
    lines: Vec<ConvLine>,
    /// 各源核已收最大序号。
    per_core: Vec<u32>,
}

impl Converge {
    pub fn new() -> Converge {
        Converge { lines: Vec::new(), per_core: vec![0; (CORE_MAX + 1) as usize] }
    }

    /// 入行：源内序号必须 +1 连续（跳号 = 源账破）；载荷必须原子。
    pub fn admit(&mut self, line: ConvLine) -> Result<(), u32> {
        if line.seq != self.per_core[line.core as usize] + 1 {
            return Err(XC_SEQ_GAP);
        }
        if !line.atomic_ok() {
            return Err(XC_ATOMic_BROKEN);
        }
        self.per_core[line.core as usize] = line.seq;
        self.lines.push(line);
        Ok(())
    }

    /// 同拍仲裁：给定同拍候选行集，胜者 = 最低核号；败者立案让行。
    pub fn arbitrate(candidates: &[ConvLine]) -> Result<(usize, Vec<usize>), u32> {
        if candidates.is_empty() {
            return Err(XC_TIE_NO_ORDER);
        }
        // 全部同拍才可仲裁（跨拍无冲突语义）。
        let mut winner = 0usize;
        for (i, c) in candidates.iter().enumerate() {
            if c.core < candidates[winner].core {
                winner = i;
            }
        }
        let losers: Vec<usize> = (0..candidates.len()).filter(|&i| i != winner).collect();
        Ok((winner, losers))
    }

    /// 按源核过滤回放（只保留该核行，序保持）。
    pub fn replay_of(&self, core: u32) -> Result<Vec<ConvLine>, u32> {
        if core > CORE_MAX {
            return Err(XC_CORE_OOR);
        }
        let rows: Vec<ConvLine> = self.lines.iter().filter(|l| l.core == core).cloned().collect();
        // 回放序号闭账：1..n 连续。
        for (i, r) in rows.iter().enumerate() {
            if r.seq != (i + 1) as u32 {
                return Err(XC_REPLAY_DRIFT);
            }
        }
        Ok(rows)
    }

    /// 全账深。
    pub fn len(&self) -> u32 {
        self.lines.len() as u32
    }
}

pub fn run_unx0356_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0356_apconverge");
    // 族一：入行。
    let mut cv = Converge::new();
    let mk = |c: u32, s: u32, p: &[u8]| ConvLine::new(c, s, p).unwrap_or(ConvLine { core: c, seq: s, payload: Vec::new() });
    cs.add("BSP 首行", cv.admit(mk(0, 1, b"bsp boot")).is_ok(), "核 0 序 1");
    cs.add("AP1 首行", cv.admit(mk(1, 1, b"ap1 up")).is_ok(), "核 1 序 1");
    cs.add("AP1 次行", cv.admit(mk(1, 2, b"ap1 mmu")).is_ok(), "核 1 序 2");
    cs.add("账深 3", cv.len() == 3, "三行入账");
    // 族二：源内序号闸。
    cs.add("跳号拒", cv.admit(mk(1, 9, b"jump")).is_err(), "SEQ_GAP");
    cs.add("重复号拒", cv.admit(mk(1, 2, b"dup")).is_err(), "SEQ_GAP");
    cs.add("核号越域拒", ConvLine::new(16, 1, b"x").is_err(), "CORE_OOR");
    cs.add("空行拒", ConvLine::new(0, 1, b"").is_err(), "EMPTY");
    cs.add("超长拒", ConvLine::new(0, 1, &vec![b'x'; 65]).is_err(), "LINE_LONG");
    // 族三：行原子。
    cs.add("行内换行拒", cv.admit(mk(0, 2, b"two\nlines")).is_err(), "ATOMic");
    cs.add("原子核", mk(0, 2, b"clean").atomic_ok(), "无换行");
    // 族四：同拍仲裁。
    let cand = vec![mk(3, 1, b"c3"), mk(1, 3, b"c1"), mk(5, 1, b"c5")];
    let (win, losers) = match Converge::arbitrate(&cand) {
        Ok(x) => x,
        Err(_) => {
            cs.add("仲裁", false, "被拒");
            return cs;
        }
    };
    cs.add("低号胜", cand[win].core == 1, "核 1 优先");
    cs.add("败者让行", losers.as_slice() == &[0usize, 2], "3/5 号让");
    cs.add("空候选拒", Converge::arbitrate(&[]).is_err(), "TIE_NO_ORDER");
    cs.add("单候选自胜", matches!(Converge::arbitrate(&[mk(7, 1, b"solo")]), Ok((0, ref l)) if l.is_empty()), "无败者");
    // 族五：按源回放。
    let ap1 = cv.replay_of(1);
    cs.add("回放两行", matches!(&ap1, Ok(v) if v.len() == 2), "核 1 独账");
    cs.add("回放序闭", matches!(&ap1, Ok(v) if v.iter().enumerate().all(|(i, l)| l.seq == (i + 1) as u32)), "1..n");
    cs.add("回放 BSP", matches!(cv.replay_of(0), Ok(ref v) if v.len() == 1), "核 0 单行");
    cs.add("空源回放", matches!(cv.replay_of(9), Ok(ref v) if v.is_empty()), "无行合法");
    cs.add("回放越域拒", cv.replay_of(16).is_err(), "CORE_OOR");
    cs
}
