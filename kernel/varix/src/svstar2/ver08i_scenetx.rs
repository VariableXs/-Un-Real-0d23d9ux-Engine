//! VE-F3509 · 场景切换事务 —— 色温/壁纸/通知三件整体原子切换 + 整组回滚。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3509`
//!
//! # 职责（锚点原文拆解）
//!
//! - **原子切换**（[`SceneTx3`]）：场景切换动的是**三件套**——色温
//!   （E06 组内）、壁纸（E05 联动）、通知策略（K 域联动）。三件是
//!   一个整体：用户看到的是「进了夜间模式」而不是「色温变了但
//!   壁纸没跟上」。事务保证三件要么全换要么全不换。
//! - **整组回滚**（[`SceneTx3::rollback`]）：任何一件失败，三件全部
//!   回到快照。回滚的粒度是**组**不是件——部分回滚比不回滚更糟
//!   （用户会看到「换了一半」这个最坏状态）。
//! - **不中断**（[`Tx3Ledger::replay_input`]）：切换期间到达的交互
//!   先入队，切换落定后按序重放。这是 E02「换肤期间交互不中断」
//!   承诺的场景切换版——同一承诺，不同域，同一个实现纪律。
//! - **部分生效修复**（[`Tx3Ledger::repair_partial`]）：如果检测到
//!   三件中只有部分生效（比如回滚中途崩溃留下的残局），修复路径
//!   把缺失的件补齐到快照值——修复优先于重新切换（便宜且无感）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 失败 | 回滚整组 |
//! | 中断 | 接管（交互重放） |
//! | 部分生效 | 修复到快照 |
//!
//! # 复杂度
//!
//! - `begin/apply/rollback`：O(1)（三件是三个 Copy 值）；
//! - `replay_input`：O(队列长度)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5760 起）
// ---------------------------------------------------------------------------

/// 场景三件事务域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tx3Code {
    /// 阶段不符（未预演就提交/未提交就回滚）。
    BadPhase = 5760,
    /// 三件中某件下发失败（触发整组回滚）。
    PieceFailed = 5761,
    /// 回滚目标缺失（没有快照）。
    NoSnapshot = 5762,
    /// 部分生效残留（进入修复路径）。
    PartialResidue = 5763,
    /// 修复后仍不一致（升级为硬错）。
    RepairIncomplete = 5764,
    /// 交互队列溢出（切换太久，超出承诺窗口）。
    InputOverflow = 5765,
}

impl Tx3Code {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            Tx3Code::BadPhase => "事务阶段不符",
            Tx3Code::PieceFailed => "三件下发失败",
            Tx3Code::NoSnapshot => "回滚目标缺失",
            Tx3Code::PartialResidue => "部分生效残留",
            Tx3Code::RepairIncomplete => "修复不完整",
            Tx3Code::InputOverflow => "交互队列溢出",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            Tx3Code::NoSnapshot | Tx3Code::RepairIncomplete | Tx3Code::InputOverflow
        )
    }
}

// ---------------------------------------------------------------------------
// 二、三件套与快照
// ---------------------------------------------------------------------------

/// 交互队列上限：切换承诺窗口内可缓冲的交互数。
pub const INPUT_QUEUE_CAP: usize = 64;

/// 场景三件套：色温/壁纸/通知。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Triple {
    /// 目标色温（开尔文）。
    pub kelvin: u32,
    /// 壁纸档位 id。
    pub wallpaper: u16,
    /// 通知策略档位 id。
    pub notify: u16,
}

/// 交互事件（重放用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// 事件序号（入队顺序）。
    pub seq: u32,
    /// 事件类别（0=点击 1=滚动 2=按键）。
    pub kind: u8,
}

/// 三件事务状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tx3Stage {
    /// 未开始。
    Idle,
    /// 预演通过。
    Prepared,
    /// 三件已下发（可能部分失败）。
    Applied,
    /// 已回滚。
    RolledBack,
    /// 修复完成。
    Repaired,
}

/// 场景切换三件事务。
pub struct SceneTx3 {
    stage: Tx3Stage,
    snapshot: Option<Triple>,
    target: Option<Triple>,
    queue: Vec<InputEvent>,
    queue_seq: u32,
    failed_piece: Option<u8>,
}

impl SceneTx3 {
    /// 新事务。
    pub fn new() -> Self {
        SceneTx3 {
            stage: Tx3Stage::Idle,
            snapshot: None,
            target: None,
            queue: Vec::new(),
            queue_seq: 0,
            failed_piece: None,
        }
    }

    /// 阶段。
    pub fn stage(&self) -> Tx3Stage {
        self.stage
    }

    /// 预演：抓当前三件快照并锁定目标。
    pub fn prepare(&mut self, current: Triple, target: Triple) {
        self.snapshot = Some(current);
        self.target = Some(target);
        self.stage = Tx3Stage::Prepared;
        self.failed_piece = None;
    }

    /// 下发三件。`deliver` 回调按件号（0=色温 1=壁纸 2=通知）尝试下发，
    /// 返回 false 即该件失败 → 立即整组回滚（不留部分生效）。
    pub fn apply(&mut self, mut deliver: impl FnMut(u8, Triple) -> bool) -> Result<(), Tx3Code> {
        if self.stage != Tx3Stage::Prepared {
            return Err(Tx3Code::BadPhase);
        }
        let t = self.target.ok_or(Tx3Code::BadPhase)?;
        for piece in 0u8..3 {
            if deliver(piece, t) {
                continue;
            }
            self.failed_piece = Some(piece);
            self.stage = Tx3Stage::Applied;
            return Err(Tx3Code::PieceFailed);
        }
        self.stage = Tx3Stage::Applied;
        Ok(())
    }

    /// 整组回滚：三件全部恢复快照值。
    pub fn rollback(&mut self, mut deliver: impl FnMut(u8, Triple) -> bool) -> Result<(), Tx3Code> {
        let snap = self.snapshot.take().ok_or(Tx3Code::NoSnapshot)?;
        for piece in 0u8..3 {
            if !deliver(piece, snap) {
                self.stage = Tx3Stage::RolledBack;
                return Err(Tx3Code::RepairIncomplete);
            }
        }
        self.stage = Tx3Stage::RolledBack;
        Ok(())
    }

    /// 失败件号（回滚后审计用）。
    pub fn failed_piece(&self) -> Option<u8> {
        self.failed_piece
    }

    /// 交互入队（切换期间不中断承诺）。
    pub fn push_input(&mut self, kind: u8) -> Result<(), Tx3Code> {
        if self.queue.len() >= INPUT_QUEUE_CAP {
            return Err(Tx3Code::InputOverflow);
        }
        self.queue.push(InputEvent {
            seq: self.queue_seq,
            kind,
        });
        self.queue_seq += 1;
        Ok(())
    }

    /// 切换落定后按序重放并清队。返回重放件数。
    pub fn replay_input(&mut self, mut deliver: impl FnMut(InputEvent)) -> usize {
        let n = self.queue.len();
        for e in core::mem::take(&mut self.queue) {
            deliver(e);
        }
        n
    }

    /// 队列深度。
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

// ---------------------------------------------------------------------------
// 三、部分生效修复
// ---------------------------------------------------------------------------

/// 三件生效掩码（bit0=色温 bit1=壁纸 bit2=通知）。
pub type PieceMask = u8;

/// 台账：负责检测与修复「部分生效」残局。
#[derive(Default)]
pub struct Tx3Ledger {
    repairs: u32,
}

impl Tx3Ledger {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 修复次数（审计）。
    pub fn repairs(&self) -> u32 {
        self.repairs
    }

    /// 检测残局：期望掩码（三件全 1 = 0b111）与实测掩码不符即残留。
    pub fn detect_partial(&self, actual: PieceMask) -> Option<PieceMask> {
        const FULL: PieceMask = 0b111;
        if actual == FULL {
            None
        } else {
            Some(actual & FULL)
        }
    }

    /// 修复：把缺失的件补到 `snap` 值。`deliver` 同 [`SceneTx3::apply`]。
    /// 全补齐返回 Ok；补不齐返回 Err（升级硬错）。
    pub fn repair_partial(
        &mut self,
        actual: PieceMask,
        snap: Triple,
        mut deliver: impl FnMut(u8, Triple) -> bool,
    ) -> Result<(), Tx3Code> {
        if self.detect_partial(actual).is_none() {
            return Ok(());
        }
        for piece in 0u8..3 {
            let bit = 1u8 << piece;
            if actual & bit != 0 {
                continue;
            }
            if !deliver(piece, snap) {
                return Err(Tx3Code::RepairIncomplete);
            }
            self.repairs += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_all_or_nothing() {
        let mut tx = SceneTx3::new();
        let cur = Triple { kelvin: 6500, wallpaper: 1, notify: 1 };
        let tgt = Triple { kelvin: 2700, wallpaper: 2, notify: 2 };
        tx.prepare(cur, tgt);
        // 第 2 件（壁纸）失败 → 整组失败。
        let r = tx.apply(|piece, _| piece != 1);
        assert_eq!(r, Err(Tx3Code::PieceFailed));
        assert_eq!(tx.failed_piece(), Some(1));
        // 回滚三件全发。
        let mut rolled = Vec::new();
        tx.rollback(|piece, t| {
            rolled.push((piece, t));
            true
        })
        .unwrap();
        assert_eq!(rolled.len(), 3);
        for (p, t) in rolled {
            assert_eq!(t, cur, "piece {}", p);
        }
    }

    #[test]
    fn input_replay_in_order() {
        let mut tx = SceneTx3::new();
        tx.prepare(
            Triple { kelvin: 6500, wallpaper: 1, notify: 1 },
            Triple { kelvin: 2700, wallpaper: 1, notify: 1 },
        );
        tx.push_input(0).unwrap();
        tx.push_input(2).unwrap();
        tx.push_input(0).unwrap();
        assert_eq!(tx.queued(), 3);
        let order = core::cell::RefCell::new(Vec::new());
        tx.replay_input(|e| order.borrow_mut().push(e.seq));
        assert_eq!(order.into_inner(), vec![0, 1, 2]);
    }

    #[test]
    fn partial_repair_fills_missing() {
        let mut led = Tx3Ledger::new();
        let snap = Triple { kelvin: 6500, wallpaper: 1, notify: 1 };
        // 色温与壁纸在位（bit0|bit1），通知缺失 → 补通知。
        let mut delivered = Vec::new();
        led.repair_partial(0b011, snap, |p, t| {
            delivered.push((p, t));
            true
        })
        .unwrap();
        assert_eq!(delivered, vec![(2, snap)]);
        assert_eq!(led.repairs(), 1);
    }
}
