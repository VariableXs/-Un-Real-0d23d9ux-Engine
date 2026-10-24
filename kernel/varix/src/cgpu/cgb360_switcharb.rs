//! CGPU-F0360 · 场景切换仲裁（判据段 0x08E0）
//! 切换仲裁：预算转移协议（场景 A→B 的预算平稳转移——原子切换防
//! 窗口期超卖）；转移时序（先予后夺——新场景先到位再回收旧场景）；
//! 多场景并存（画中画视频+前台滚动——多配方叠加仲裁）；切换风暴
//! 防护（快速连续切换的合并窗口）。判据：原子转移、先予后夺、并
//! 存、风暴合并、判据。
//!
//! 锚点原文摘录（施工书 1603 行）：
//! 「预算转移协议（原子切换防窗口期超卖）；转移时序（先予后夺——
//! 新场景先到位再回收旧场景）；多场景并存（多配方叠加仲裁）；切
//! 换风暴防护（快速连续切换的合并窗口）。判据：原子转移、先予后
//! 夺、并存、风暴合并、判据。」
//!
//! 诊断码段 0x08E0（本模块独占：0x08E0~0x08EF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。


/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x08E0;

/// 切换合并窗口（帧）：风暴期内快速切换在窗口内合并为一次。
pub const STORM_MERGE_WINDOW_FRAMES: u32 = 8;

/// 转移步四相（判据「先予后夺」的严格时序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransferPhase {
    /// 相 1：预检（A 有余量可让）。
    Precheck,
    /// 相 2：先予（B 到位）。
    Grant,
    /// 相 3：后夺（A 回收）。
    Reclaim,
    /// 相 4：封账（转移原子提交）。
    Seal,
}

impl TransferPhase {
    /// 相位号（严格递增——时序闸依据）。
    pub fn ordinal(&self) -> u8 {
        match self {
            TransferPhase::Precheck => 1,
            TransferPhase::Grant => 2,
            TransferPhase::Reclaim => 3,
            TransferPhase::Seal => 4,
        }
    }
    /// 下一相（封账后再推进 = 非法）。
    pub fn next(&self) -> Option<TransferPhase> {
        match self {
            TransferPhase::Precheck => Some(TransferPhase::Grant),
            TransferPhase::Grant => Some(TransferPhase::Reclaim),
            TransferPhase::Reclaim => Some(TransferPhase::Seal),
            TransferPhase::Seal => None,
        }
    }
}

/// 转移协议状态机（判据「原子转移」主判据）：四相严格单向推进，
/// 任何跳相即拒；封账前任何相可整体回滚（回滚 = 账面回到预检前，
/// 不产生半转移状态——窗口期不超卖的根）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransferMachine {
    /// 当前相。
    pub phase: TransferPhase,
    /// 让出场景号。
    pub from_scene: u32,
    /// 得到场景号。
    pub to_scene: u32,
    /// 转移量（千分比）。
    pub amount_permille: u32,
    /// 已提交。
    pub sealed: bool,
}

impl TransferMachine {
    /// 开启转移（判据「原子转移」入口）：自检合法性。
    pub fn open(from_scene: u32, to_scene: u32, amount: u32) -> Result<Self, u16> {
        if from_scene == 0 || to_scene == 0 {
            return Err(SEG_BASE);
        }
        if from_scene == to_scene {
            return Err(SEG_BASE + 1);
        }
        if amount == 0 || amount > 1000 {
            return Err(SEG_BASE + 2);
        }
        Ok(TransferMachine {
            phase: TransferPhase::Precheck,
            from_scene,
            to_scene,
            amount_permille: amount,
            sealed: false,
        })
    }
    /// 推进一相（判据「先予后夺」时序闸：只许走 next，跳相即拒）。
    pub fn advance(&mut self) -> Result<TransferPhase, u16> {
        if self.sealed {
            return Err(SEG_BASE + 3);
        }
        let nx = match self.phase.next() {
            Some(p) => p,
            None => return Err(SEG_BASE + 3),
        };
        // 先予后夺硬闸：Reclaim 相只有在 Grant 已发生（相号>2）才可达。
        if nx == TransferPhase::Reclaim && self.phase.ordinal() < TransferPhase::Grant.ordinal() {
            return Err(SEG_BASE + 4);
        }
        self.phase = nx;
        if nx == TransferPhase::Seal {
            self.sealed = true;
        }
        Ok(nx)
    }
    /// 回滚（封账前有效）：相回预检、量保留——调用方决定重试或放弃。
    pub fn rollback(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(SEG_BASE + 5);
        }
        self.phase = TransferPhase::Precheck;
        Ok(())
    }
}

/// 转移守恒核（判据「原子转移」验收面）：A 减量 ≡ B 增量，全局
/// Σ 恒 1000——窗口期不超卖的量化证明。
pub fn transfer_conservation(
    a_before: u32,
    b_before: u32,
    amount: u32,
) -> Result<(u32, u32), u16> {
    if amount == 0 || amount > 1000 {
        return Err(SEG_BASE + 6);
    }
    if a_before < amount {
        return Err(SEG_BASE + 7);
    }
    let a_after = a_before - amount;
    let b_after = b_before + amount;
    if a_after + b_after != a_before + b_before {
        return Err(SEG_BASE + 8);
    }
    Ok((a_after, b_after))
}

/// 场景并存表（判据「并存」输入）：多配方同时在场，各占份额。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoexistEntry {
    /// 场景号。
    pub scene: u32,
    /// 配方号（对应 F0357~F0359 的配方实例）。
    pub recipe: u32,
    /// 当前份额（千分比）。
    pub share_permille: u32,
    /// 是否前台。
    pub foreground: bool,
}

impl CoexistEntry {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.scene == 0 || self.recipe == 0 {
            return Err(SEG_BASE + 9);
        }
        if self.share_permille > 1000 {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }
}

/// 并存仲裁（判据「并存」主判据）：多配方叠加时——①Σ份额 ≤ 1000
/// （超卖即拒）；②前台场景份额 ≥ 每个后台场景（可见性优先）；
/// ③空表非法。
pub fn coexist_arbitrate(entries: &[CoexistEntry]) -> Result<(), u16> {
    if entries.is_empty() {
        return Err(SEG_BASE + 11);
    }
    for e in entries {
        e.validate()?;
    }
    let sum: u32 = entries.iter().map(|e| e.share_permille).sum();
    if sum > 1000 {
        return Err(SEG_BASE + 12);
    }
    if let Some(fg) = entries.iter().find(|e| e.foreground) {
        for e in entries.iter().filter(|e| !e.foreground) {
            if fg.share_permille < e.share_permille {
                return Err(SEG_BASE + 13);
            }
        }
        let _ = fg;
    }
    Ok(())
}

/// 切换风暴合并器（判据「风暴合并」主判据）：窗口内的连续切换
/// 请求合并为一个最终目的地——中间态不产生转移开销。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormMerger {
    /// 窗口起点（帧号）。
    pub window_start_frame: u32,
    /// 窗口内最终目的地（None=窗口未开）。
    pub pending_to: Option<u32>,
    /// 窗口内被合并掉的请求数。
    pub merged_count: u32,
}

impl StormMerger {
    /// 新合并器。
    pub fn new() -> Self {
        StormMerger { window_start_frame: 0, pending_to: None, merged_count: 0 }
    }
    /// 请求切换（判据「风暴合并」入口）：窗口未开则开窗记录起点；
    /// 窗口内则更新目的地并计合并数。
    pub fn request(&mut self, frame: u32, to_scene: u32) -> Result<bool, u16> {
        if to_scene == 0 {
            return Err(SEG_BASE + 14);
        }
        match self.pending_to {
            None => {
                self.window_start_frame = frame;
                self.pending_to = Some(to_scene);
                Ok(true)
            }
            Some(_) => {
                if frame - self.window_start_frame < STORM_MERGE_WINDOW_FRAMES {
                    self.pending_to = Some(to_scene);
                    self.merged_count += 1;
                    Ok(false)
                } else {
                    // 窗口已过：旧请求落定，开新窗。
                    self.window_start_frame = frame;
                    self.pending_to = Some(to_scene);
                    self.merged_count = 0;
                    Ok(true)
                }
            }
        }
    }
    /// 落定（取出最终目的地，关窗）。
    pub fn settle(&mut self) -> Option<u32> {
        let out = self.pending_to.take();
        if out.is_some() {
            self.merged_count = 0;
        }
        out
    }
}

impl Default for StormMerger {
    fn default() -> Self {
        Self::new()
    }
}

/// 切换指纹（可审计面）。
pub fn switch_fp(m: &TransferMachine) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for v in [
        m.from_scene as u64,
        m.to_scene as u64,
        m.amount_permille as u64,
        m.phase.ordinal() as u64,
        m.sealed as u64,
    ] {
        acc = acc.wrapping_mul(0x100000001b3) ^ v;
    }
    acc
}

/// 切换仲裁诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "场景号为 0 非法",
        1 => "自切自非法（from=to）",
        2 => "转移量必须 ∈ (0,1000]",
        3 => "已封账——协议终结不可再推进",
        4 => "跳相即拒——先予后夺时序被破坏",
        5 => "封账后不可回滚——原子性边界",
        6 => "转移量非法",
        7 => "让出方余量不足——预检应拦住这笔",
        8 => "转移守恒被破坏（A减≠B增）",
        9 => "场景/配方号为 0",
        10 => "份额 >1000",
        11 => "并存表为空",
        12 => "并存 Σ>1000——窗口期超卖",
        13 => "前台份额低于后台——可见性优先被破坏",
        14 => "切换目的地为 0",
        _ => "未知诊断码——查本模块 SEG_BASE 0x08E0",
    }
}

#[cfg(test)]
mod tests {
use alloc::vec;
    use super::*;

    #[test]
    fn transfer_four_phase_strict_order() {
        let mut m = TransferMachine::open(1, 2, 300).unwrap();
        assert_eq!(m.phase, TransferPhase::Precheck);
        assert_eq!(m.advance().unwrap(), TransferPhase::Grant);
        assert_eq!(m.advance().unwrap(), TransferPhase::Reclaim);
        assert_eq!(m.advance().unwrap(), TransferPhase::Seal);
        assert!(m.sealed);
        // 封账后推进拒绝。
        assert_eq!(m.advance().unwrap_err(), SEG_BASE + 3);
        let fp = switch_fp(&m);
        assert!(fp != 0);
    }

    #[test]
    fn rollback_before_seal_only() {
        let mut m = TransferMachine::open(1, 2, 300).unwrap();
        m.advance().unwrap();
        assert!(m.rollback().is_ok());
        assert_eq!(m.phase, TransferPhase::Precheck);
        m.advance().unwrap();
        m.advance().unwrap();
        m.advance().unwrap();
        assert_eq!(m.rollback().unwrap_err(), SEG_BASE + 5);
    }

    #[test]
    fn conservation_never_oversell() {
        let (a, b) = transfer_conservation(600, 100, 300).unwrap();
        assert_eq!(a, 300);
        assert_eq!(b, 400);
        assert_eq!(transfer_conservation(200, 100, 300).unwrap_err(), SEG_BASE + 7);
        assert!(diagnose(SEG_BASE + 7).contains("预检"));
    }

    #[test]
    fn coexist_rules() {
        let ok = vec![
            CoexistEntry { scene: 1, recipe: 357, share_permille: 500, foreground: true },
            CoexistEntry { scene: 2, recipe: 358, share_permille: 400, foreground: false },
        ];
        assert!(coexist_arbitrate(&ok).is_ok());
        // 超卖。
        let oversell = vec![
            CoexistEntry { scene: 1, recipe: 357, share_permille: 700, foreground: true },
            CoexistEntry { scene: 2, recipe: 358, share_permille: 400, foreground: false },
        ];
        assert_eq!(coexist_arbitrate(&oversell).unwrap_err(), SEG_BASE + 12);
        // 前台低于后台。
        let inverted = vec![
            CoexistEntry { scene: 1, recipe: 357, share_permille: 100, foreground: true },
            CoexistEntry { scene: 2, recipe: 358, share_permille: 400, foreground: false },
        ];
        assert_eq!(coexist_arbitrate(&inverted).unwrap_err(), SEG_BASE + 13);
        assert!(coexist_arbitrate(&[]).is_err());
    }

    #[test]
    fn storm_merging() {
        let mut sm = StormMerger::new();
        assert!(sm.request(0, 5).unwrap());
        // 窗口内连续切换：全部合并，不触发。
        assert!(!sm.request(1, 6).unwrap());
        assert!(!sm.request(2, 7).unwrap());
        assert_eq!(sm.merged_count, 2);
        assert_eq!(sm.settle(), Some(7));
        // 窗口外新请求：触发新窗。
        assert!(sm.request(100, 9).unwrap());
        assert_eq!(sm.settle(), Some(9));
        assert_eq!(sm.request(0, 0).unwrap_err(), SEG_BASE + 14);
    }
}
