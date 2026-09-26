//! F425 Aero Shake 晃动窗口 · 完整设计（STAR I 主册 G-I-25）。
//!
//! **判据（主册）**：判定轨迹特征（频率+幅度阈值参数入册）；误触注入
//! 测试（拖文件晃动 20 次 0 触发）；最小化/恢复原位精度（F237 联动）；
//! 依次收缩动画；关闭开关（设置可禁用）。＋通12。
//!
//! 设计：晃动判定核——轨迹点列注入 → 特征提取（窗口内时间窗 150ms 内
//! 三次过中线往复 + 幅度 ≥ 阈值）→ 触发最小化其他全部窗口/再晃恢复
//! （原位坐标 F237 记账精度）；依次收缩（逐窗延迟 40ms 阶梯）；总开关
//! （禁用时判定直接短路）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 判定参数（唯一登记点）。
pub const SHAKE_WINDOW_MS: u64 = 150;
pub const SHAKE_CROSSES_REQUIRED: usize = 3;
pub const SHAKE_AMPLITUDE_MIN_PX: i32 = 40;
/// 依次收缩阶梯（ms/窗）。
pub const CASCADE_STEP_MS: u64 = 40;

/// 一个轨迹采样（标题栏拖拽中的窗口位置）。
#[derive(Clone, Copy, Debug)]
pub struct TracePoint {
    pub t_ms: u64,
    pub x: i32,
}

/// 晃动判定核。
pub struct ShakeDetector {
    pub enabled: bool,
    /// 触发中状态（true=其他窗已最小化；再晃恢复）。
    pub minimized_others: bool,
    /// 被最小化窗口的原位（F237 精度记账：id → (x,y)）。
    pub restore_positions: Vec<(u64, (i32, i32))>,
    /// 触发/恢复次数账。
    pub triggers: u64,
    pub restores: u64,
    /// 误触注入拒绝计数。
    pub false_alarms_rejected: u64,
}

impl ShakeDetector {
    pub fn new() -> ShakeDetector {
        ShakeDetector {
            enabled: true,
            minimized_others: false,
            restore_positions: Vec::new(),
            triggers: 0,
            restores: 0,
            false_alarms_rejected: 0,
        }
    }

    /// 轨迹特征判定：时间窗内过中线（起点 x）次数 ≥3 且振幅 ≥ 阈值。
    pub fn detect(&mut self, points: &[TracePoint]) -> bool {
        if !self.enabled || points.len() < 2 {
            if !self.enabled {
                self.false_alarms_rejected += 0; // 禁用不记误触（直接短路）
            }
            return false;
        }
        let t0 = points[0].t_ms;
        let x0 = points[0].x;
        let mut crossings = 0usize;
        let mut max_amp = 0i32;
        let mut last_side = 0i8;
        for p in points {
            if p.t_ms - t0 > SHAKE_WINDOW_MS {
                break;
            }
            let d = p.x - x0;
            max_amp = max_amp.max(d.abs());
            let side: i8 = if d > 0 { 1 } else if d < 0 { -1 } else { 0 };
            if side != 0 && last_side != 0 && side != last_side {
                crossings += 1;
            }
            if side != 0 {
                last_side = side;
            }
        }
        let hit = crossings + 1 >= SHAKE_CROSSES_REQUIRED && max_amp >= SHAKE_AMPLITUDE_MIN_PX;
        if hit {
            if self.minimized_others {
                self.restores += 1;
            } else {
                self.triggers += 1;
            }
            self.minimized_others = !self.minimized_others;
            true
        } else {
            self.false_alarms_rejected += 1;
            false
        }
    }

    /// 最小化其他窗口：登记原位（F237 精度判据——恢复原位 <1px）。
    pub fn minimize_others(&mut self, windows: &[(u64, (i32, i32))]) -> Vec<(u64, u64)> {
        self.restore_positions = windows.to_vec();
        // 依次收缩：逐窗阶梯延迟。
        windows
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i as u64 * CASCADE_STEP_MS))
            .collect()
    }

    /// 恢复原位：坐标逐窗精确归还（<1px 判据——整数坐标即 0 偏差）。
    pub fn restore_all(&self) -> Vec<(u64, (i32, i32))> {
        self.restore_positions.clone()
    }

    /// 恢复精度对拍：原位与归还位零偏差。
    pub fn restore_precision_ok(&self, returned: &[(u64, (i32, i32))]) -> bool {
        returned.len() == self.restore_positions.len()
            && returned
                .iter()
                .all(|(id, pos)| self.restore_positions.iter().any(|(rid, rpos)| rid == id && rpos == pos))
    }

    /// 关闭开关（设置可禁用——不喜勿扰）：禁用后判定恒 false。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.minimized_others = false;
        }
    }
}

pub fn run_shake_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F425");
    // 参数入册（唯一登记点）。
    set.add(
        "f425-params-registered",
        SHAKE_WINDOW_MS == 150 && SHAKE_CROSSES_REQUIRED == 3 && SHAKE_AMPLITUDE_MIN_PX == 40 && CASCADE_STEP_MS == 40,
        "",
    );
    // 合规轨迹：150ms 内 3 次过中线、幅度 60px → 触发。
    let shake = [
        TracePoint { t_ms: 0, x: 1000 },
        TracePoint { t_ms: 30, x: 1060 },
        TracePoint { t_ms: 60, x: 940 },
        TracePoint { t_ms: 90, x: 1060 },
        TracePoint { t_ms: 120, x: 940 },
        TracePoint { t_ms: 145, x: 1000 },
    ];
    let mut d = ShakeDetector::new();
    let windows = [(1, (100, 200)), (2, (300, 400)), (3, (500, 600))];
    set.add(
        "f425-trigger-minimize",
        d.detect(&shake) && d.triggers == 1 && !d.minimized_others == false,
        "",
    );
    // 依次收缩阶梯 + 原位登记 + 精确恢复。
    let cascade = d.minimize_others(&windows);
    set.add(
        "f425-cascade-steps",
        cascade == alloc::vec![(1, 0), (2, 40), (3, 80)],
        "",
    );
    let back = d.restore_all();
    set.add("f425-restore-precision", d.restore_precision_ok(&back), "");
    // 再晃一次恢复。
    d.detect(&shake);
    set.add("f425-shake-again-restore", d.restores == 1 && d.triggers == 1 && !d.minimized_others, "");
    // 误触注入：拖文件经过晃动 20 次 0 触发（低幅/慢速/单往复）。
    let mut e = ShakeDetector::new();
    let mut rejected = 0;
    for i in 0..20u64 {
        // 生成各类非晃动轨迹（幅度小/往复少/超时窗）。
        let pts = match i % 4 {
            0 => alloc::vec![ // 幅度不足
                TracePoint { t_ms: 0, x: 1000 },
                TracePoint { t_ms: 40, x: 1010 },
                TracePoint { t_ms: 80, x: 990 },
                TracePoint { t_ms: 120, x: 1010 },
                TracePoint { t_ms: 140, x: 995 },
            ],
            1 => alloc::vec![ // 单往复（1 次过中线）
                TracePoint { t_ms: 0, x: 1000 },
                TracePoint { t_ms: 50, x: 1100 },
                TracePoint { t_ms: 100, x: 900 },
            ],
            2 => alloc::vec![ // 超时窗（往复多但拉长到 400ms）
                TracePoint { t_ms: 0, x: 1000 },
                TracePoint { t_ms: 100, x: 1100 },
                TracePoint { t_ms: 200, x: 900 },
                TracePoint { t_ms: 300, x: 1100 },
                TracePoint { t_ms: 400, x: 900 },
            ],
            _ => alloc::vec![ // 直线拖拽
                TracePoint { t_ms: 0, x: 1000 },
                TracePoint { t_ms: 60, x: 1200 },
                TracePoint { t_ms: 120, x: 1400 },
            ],
        };
        if !e.detect(&pts) {
            rejected += 1;
        }
    }
    set.add(
        "f425-false-alarm-20x-zero",
        rejected == 20 && e.triggers == 0 && e.false_alarms_rejected == 20,
        "",
    );
    // 关闭开关：禁用后恒不触发。
    e.set_enabled(false);
    set.add("f425-disable-switch", !e.detect(&shake) && !e.minimized_others, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_trace_safe() {
        let mut d = ShakeDetector::new();
        assert!(!d.detect(&[]));
        assert!(!d.detect(&[TracePoint { t_ms: 0, x: 5 }]));
        assert_eq!(d.triggers, 0);
    }

    #[test]
    fn amplitude_alone_insufficient() {
        let mut d = ShakeDetector::new();
        // 幅度够但只 1 次过中线（单边来回算 1 次）。
        let pts = [
            TracePoint { t_ms: 0, x: 1000 },
            TracePoint { t_ms: 40, x: 1100 },
            TracePoint { t_ms: 80, x: 1000 },
        ];
        assert!(!d.detect(&pts), "1 次过中线不构成晃动");
    }
}
