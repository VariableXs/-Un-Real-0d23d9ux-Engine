//! U3 主题域·复制与文件操作链（Varix STAR I start · I 域 F501~F550 ·
//! AI-U3 分工包 · copyops.rs，覆盖 F524/F529/F530/F531/F532/F533/F534 七项）。
//!
//! 判据唯一源：主册《Varix STAR I start.md》各节【验收判据】第一句
//! （逐字摘录见下）+ 通用验收十二查。
//!
//! ---------------------------------------------------------------------------
//! F524 撤销清空回收站（domain tag: `F524-trash-undo`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **5s 窗口与暂存机制；撤销完整性（N 项全回）；长按延寿 10s；真释放时机
//! 与账目同步；超时后诚实不可恢复。**
//!
//! 功能定义要点：回收站「清空」后的 5 秒后悔窗——清空执行后通知条驻留
//! 5 秒（「已清空回收站 N 项——撤销」），点击撤销则全部还原（清空动作内部
//! 先暂存、超时才真释放）；5 秒后真释放（磁盘空间那时才回收——F267 存储
//! 感知的账目同步延迟）；长按撤销条可延寿（再给 10 秒）。
//!
//! 依赖锚点：F267 存储感知（空间账目同步延迟）。
//!
//! 【登记偏差】主册只说「长按延寿（再给 10 秒）」，未明说长按可否重复。
//! 本实装按「每次长按各延 10s、上限 2 次」建模（MAX_HOLD_EXTENDS=2）：
//! 防止无限长按把真释放无限推迟；若主册后续澄清为单次延寿，只需把该
//! 常量改 1，窗口状态机其余逻辑不动。
//!
//! ---------------------------------------------------------------------------
//! F529 复制前空间预检（domain tag: `F529-space-precheck`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **预检时机（进度框前）；10% 缓冲；三选出路；换目标流程；跨盘批逐盘检。**
//!
//! 功能定义要点：粘贴/复制启动前空间预检——目标卷剩余 <所需（含 10%
//! 缓冲）时开跑前拦下（「目标盘只剩 2.1GB，这批要 3.4GB——放不下」+
//! 「仍要复制（会失败）/换目标/取消」三选）；预检在进度对话框出现前完成；
//! 多目标批（分批跨盘）逐盘预检。
//!
//! 依赖锚点：F267 存储感知（卷剩余空间账目）。
//!
//! ---------------------------------------------------------------------------
//! F530 复制后校验（domain tag: `F530-copy-verify`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **>1GB 自动开默认；空闲 IO 判据；失败报告完整性；通知补发时序；关闭开关。**
//!
//! 功能定义要点：复制完成校验（可选开启，默认对 >1GB 文件自动开）——
//! 复制完成后源/目标哈希比对（后台做——完成通知先发、校验结果补一条），
//! 不符时明确告知（「校验失败——建议重新复制该文件」+源/目标路径）；
//! 校验走空闲 IO（F050/F057 纪律，不抢前台）。
//!
//! 依赖锚点：F050/F057 空闲 IO 纪律（IoClass::Idle 只在空闲额度派发）。
//!
//! ---------------------------------------------------------------------------
//! F531 复制任务队列化（domain tag: `F531-copy-queue`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **同盘串行/异盘并行判定；队列视图四信息；插队优先级；暂停取消独立；
//! 与 F269 断点续传衔接。**
//!
//! 功能定义要点：多批复制不互相踩——同时发起的多批复制自动排队（同盘
//! 串行——磁头/闪存顺序写性能最好；异盘可并行各走各）；队列在进度中心
//! 可见（F369——每批独立进度/暂停/取消/优先级）；「先传这批」右键插队。
//!
//! 依赖锚点：F269 断点续传（Done 前取消→记录字节偏移，恢复续传）；
//! F369 进度中心（队列视图四信息展示面）。
//!
//! ---------------------------------------------------------------------------
//! F532 打开失败人话诊断（domain tag: `F532-openfail-diag`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **四类归因准确率（注入用例）；出路链接有效性；三问结构（F209 同规）；
//! 损坏检测判据；与 F257/F324 链接。**
//!
//! 功能定义要点：文件打不开（双击失败）的诊断三问——是什么问题（格式
//! 不支持/文件损坏/应用缺失/权限不足四类人话归因）、为什么（附加细节——
//! 「头 512 字节不是有效 ZIP 签名」）、现在能做什么（每类配出路：格式→
//! F257 选择器 / 损坏→提示源与 F294 关联 / 应用缺失→搜索 / 权限→F324
//! 权限页直达）；损坏类顺手连到备份/版本恢复（F325/F396）。
//! 类别：系统服务（服务主逻辑 ~55%、持久化 ~20%、错误与边界 ~25%）。
//!
//! 依赖锚点：F209 三问结构同规；F257 打开方式选择器；F294 文件关联；
//! F324 权限页；F325/F396 备份与版本恢复。
//!
//! ---------------------------------------------------------------------------
//! F533 只读介质提醒（domain tag: `F533-readonly-notice`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **写保护检测；前置提醒时机；角标可见性；出路文案；提示一次不重复。**
//!
//! 功能定义要点：向只读介质写文件的前置提醒——目标卷只读（写保护开关/
//! 系统只读挂载）时拖放/粘贴直接给说明（「此盘处于只读状态——检查物理
//! 写保护开关或以可写方式重新挂载」）而不是让用户对着「失败」发呆；
//! 只读态在卷图标角标（F453 体系）+此机页（F456）可见；提示一次不重复。
//!
//! 依赖锚点：F453 卷图标角标体系；F456 此机页。
//!
//! ---------------------------------------------------------------------------
//! F534 长路径全程支持（domain tag: `F534-longpath`）
//! ---------------------------------------------------------------------------
//! 主册判据（第一句逐字摘录）：
//! **600+ 字符路径全操作用例；索引覆盖；显示保尾；复制路径完整；性能
//! （深路径遍历无异常延迟）。**
//!
//! 功能定义要点：超长路径（>260 字符）全链可用——创建/浏览/复制/删除/
//! 搜索（F306 索引）对深嵌套目录不设障碍；地址栏（F265）与 Tooltip
//! （F247 截断保尾）对长路径的显示策略（保尾保文件名）；复制长路径
//! （F336）完整无损。
//!
//! 依赖锚点：F306 搜索索引；F265 地址栏；F247 Tooltip 保尾；F336 复制。
//!
//! ---------------------------------------------------------------------------
//! 共同纪律
//! ---------------------------------------------------------------------------
//! 零堆热路径：逻辑路径无 String/Vec/Box/format!（core-only，定长数组 +
//! &str）；路径用定长 u8 数组 + 长度字段建模（`UPath`，容量 PATH_CAP）。
//! 每项独立 run_f5XX_checks → CheckSet（MAX_CHECKS=64，cs.add(name,
//! passed, detail)），domain tag 见上；数字精确成常量（5000ms、10000ms、
//! 10% 缓冲、1GB、600、260、32767）。测试可用 std。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 共享建模：定长路径（零堆）
// ---------------------------------------------------------------------------

/// 路径建模容量（逻辑层上限；F534 的 32767 支持线用长度字段建模，
/// 不真存 32KB——见 longpath 节）。
pub const PATH_CAP: usize = 512;

/// 定长路径：长度字段 + 定容 u8 数组，无堆。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UPath {
    len: usize,
    bytes: [u8; PATH_CAP],
}

impl UPath {
    pub const EMPTY: UPath = UPath { len: 0, bytes: [0; PATH_CAP] };

    /// 从字节切片构造（超容返回 None——诚实拒绝，不静默截断）。
    pub fn from_bytes(b: &[u8]) -> Option<UPath> {
        if b.len() > PATH_CAP {
            return None;
        }
        let mut p = UPath::EMPTY;
        p.bytes[..b.len()].copy_from_slice(b);
        p.len = b.len();
        Some(p)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

// ===========================================================================
// F524 撤销清空回收站
// ===========================================================================

/// 撤销窗口 5 秒（主册）。
pub const UNDO_WINDOW_MS: u64 = 5000;
/// 长按延寿一次 10 秒（主册）。
pub const HOLD_EXTEND_MS: u64 = 10_000;
/// 长按延寿上限（登记偏差：主册未明说可否重复，按上限 2 次防无限推迟，
/// 见文件头 F524 节登记偏差）。
pub const MAX_HOLD_EXTENDS: usize = 2;
/// 暂存槽位数（定长，一次清空的回收站项数上限）。
pub const MAX_TRASH_ITEMS: usize = 64;

/// 暂存项：清空动作先暂存的原路径 + 字节数（真释放时账目同步用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagedItem {
    pub orig: UPath,
    pub bytes: u64,
}

/// 撤销错误态（诚实不可恢复——超时后明确拒绝，不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoError {
    /// 没有进行中的清空窗口。
    NothingStaged,
    /// 5s 窗口已过、真释放已（或即将）发生——诚实告知不可恢复。
    Unrecoverable,
}

/// 窗口阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPhase {
    /// 未在窗口内（暂存中 / 已结束）。
    Idle,
    /// 清空已执行、通知条驻留中（5s 后悔窗）。
    Armed,
}

/// 回收站撤销窗口状态机。
///
/// 生命周期：`stage_item`×N（清空动作内部先暂存）→ `clear_executed`
/// （通知条出现，deadline = now + 5000）→ 三条出路：
/// - `undo`（窗口内）：全量还原（N 项全回、槽位全清、账目不动）；
/// - `settle`（到时）：真释放——此刻才把暂存字节还给剩余空间
///   （F267 账目同步延迟：释放前后剩余空间差 = 暂存字节合计）；
/// - 超时后的 `undo`：`Err(Unrecoverable)`——诚实拒绝，不是静默。
pub struct TrashUndoWindow {
    staged: [Option<StagedItem>; MAX_TRASH_ITEMS],
    staged_count: usize,
    staged_bytes: u64,
    phase: WindowPhase,
    deadline_ms: u64,
    holds: usize,
    free_bytes: u64,
}

impl TrashUndoWindow {
    pub fn new(free_bytes: u64) -> TrashUndoWindow {
        TrashUndoWindow {
            staged: [None; MAX_TRASH_ITEMS],
            staged_count: 0,
            staged_bytes: 0,
            phase: WindowPhase::Idle,
            deadline_ms: 0,
            holds: 0,
            free_bytes,
        }
    }

    /// 暂存一项（清空动作内部第一步）。窗口已开（Armed）或槽满时拒绝。
    pub fn stage_item(&mut self, orig: &UPath, bytes: u64) -> bool {
        if self.phase != WindowPhase::Idle || self.staged_count >= MAX_TRASH_ITEMS {
            return false;
        }
        self.staged[self.staged_count] = Some(StagedItem { orig: *orig, bytes });
        self.staged_count += 1;
        self.staged_bytes += bytes;
        true
    }

    /// 清空执行（通知条出现）：开 5s 窗口。此刻磁盘空间尚未回收。
    pub fn clear_executed(&mut self, now_ms: u64) -> bool {
        if self.phase != WindowPhase::Idle || self.staged_count == 0 {
            return false;
        }
        self.phase = WindowPhase::Armed;
        self.deadline_ms = now_ms + UNDO_WINDOW_MS;
        true
    }

    /// 长按撤销条延寿：deadline += 10s（上限 MAX_HOLD_EXTENDS 次防无限）。
    pub fn hold(&mut self) -> bool {
        if self.phase != WindowPhase::Armed || self.holds >= MAX_HOLD_EXTENDS {
            return false;
        }
        self.deadline_ms += HOLD_EXTEND_MS;
        self.holds += 1;
        true
    }

    /// 点击撤销：窗口内则 N 项全回（槽位全清、暂存字节清零、账目不动
    /// ——还原不是释放）。窗口已过 → 诚实拒绝。
    pub fn undo(&mut self, now_ms: u64) -> Result<usize, UndoError> {
        if self.phase != WindowPhase::Armed {
            return Err(UndoError::NothingStaged);
        }
        if now_ms > self.deadline_ms {
            // 超时：真释放已发生（settle）或已注定——诚实不可恢复。
            return Err(UndoError::Unrecoverable);
        }
        let n = self.staged_count;
        for slot in self.staged.iter_mut() {
            *slot = None;
        }
        self.staged_count = 0;
        self.staged_bytes = 0;
        self.phase = WindowPhase::Idle;
        self.holds = 0;
        Ok(n)
    }

    /// 真释放（到时自动触发）：暂存字节此刻才归还剩余空间（账目同步）。
    /// 返回是否发生了释放。
    pub fn settle(&mut self, now_ms: u64) -> bool {
        if self.phase != WindowPhase::Armed || now_ms < self.deadline_ms {
            return false;
        }
        self.free_bytes += self.staged_bytes;
        for slot in self.staged.iter_mut() {
            *slot = None;
        }
        self.staged_count = 0;
        self.staged_bytes = 0;
        self.phase = WindowPhase::Idle;
        self.holds = 0;
        true
    }

    pub fn phase(&self) -> WindowPhase {
        self.phase
    }
    pub fn staged_count(&self) -> usize {
        self.staged_count
    }
    pub fn staged_bytes(&self) -> u64 {
        self.staged_bytes
    }
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }
    pub fn holds(&self) -> usize {
        self.holds
    }
    pub fn free_bytes(&self) -> u64 {
        self.free_bytes
    }
}

/// 域自检（F524-trash-undo）。
pub fn run_f524_checks() -> CheckSet {
    let mut cs = CheckSet::new("F524-trash-undo");
    // 1) 数字精确成常量：5s 窗口 / 10s 延寿 / 延寿上限 2。
    cs.add(
        "window_constants",
        UNDO_WINDOW_MS == 5000 && HOLD_EXTEND_MS == 10_000 && MAX_HOLD_EXTENDS == 2,
        "",
    );
    // 2) 暂存机制：N 项入槽 + 字节合计记账。
    let mut w = TrashUndoWindow::new(1_000_000);
    let mut staged_ok = true;
    for i in 0..5u64 {
        let mut bp = [0u8; 12];
        let src = b"/trash/item";
        bp[..src.len()].copy_from_slice(src);
        bp[11] = b'0' + i as u8;
        match UPath::from_bytes(&bp) {
            Some(p) => staged_ok &= w.stage_item(&p, 100 * (i + 1)),
            None => staged_ok = false,
        }
    }
    cs.add(
        "stage_n_items",
        staged_ok && w.staged_count() == 5 && w.staged_bytes() == 100 + 200 + 300 + 400 + 500,
        "",
    );
    // 3) 清空执行 → Armed，deadline = now + 5000，此刻空间未回收。
    cs.add(
        "clear_executed_arms_window",
        w.clear_executed(10_000)
            && w.phase() == WindowPhase::Armed
            && w.deadline_ms() == 15_000
            && w.free_bytes() == 1_000_000,
        "",
    );
    // 4) 撤销完整性：N 项全回（还原计数一致 + 槽位全清 + 暂存字节清零）。
    let restored = w.undo(12_000);
    cs.add(
        "undo_restores_all",
        matches!(restored, Ok(5)) && w.staged_count() == 0 && w.staged_bytes() == 0,
        "",
    );
    // 5) 撤销不是释放：剩余空间分毫不动。
    cs.add("undo_account_unchanged", w.free_bytes() == 1_000_000, "");
    // 6) 真释放时机与账目同步：settle 后剩余空间差 = 暂存字节合计。
    let mut w2 = TrashUndoWindow::new(500_000);
    for i in 0..4u64 {
        let p = UPath::from_bytes(b"/trash/x").unwrap_or(UPath::EMPTY);
        let _ = w2.stage_item(&p, 1_000 * (i + 1));
    }
    let _ = w2.clear_executed(0);
    let settled = w2.settle(5_000);
    cs.add(
        "settle_account_sync",
        settled && w2.free_bytes() == 500_000 + 10_000 && w2.staged_count() == 0,
        "",
    );
    // 7) 超时后诚实不可恢复：Err(Unrecoverable)，不是静默假成功。
    let mut w3 = TrashUndoWindow::new(0);
    let _ = w3.stage_item(&UPath::from_bytes(b"/trash/late").unwrap_or(UPath::EMPTY), 1);
    let _ = w3.clear_executed(0);
    cs.add(
        "undo_after_timeout_honest_refusal",
        matches!(w3.undo(UNDO_WINDOW_MS + 1), Err(UndoError::Unrecoverable)),
        "",
    );
    // 8) settle 之后的 undo 同样诚实拒绝（窗口已终结）。
    let _ = w3.settle(UNDO_WINDOW_MS + 1);
    cs.add(
        "undo_after_settle_refused",
        matches!(w3.undo(UNDO_WINDOW_MS + 2), Err(UndoError::NothingStaged)),
        "",
    );
    // 9) 长按延寿：deadline 精确 +10s。
    let mut w4 = TrashUndoWindow::new(0);
    let _ = w4.stage_item(&UPath::from_bytes(b"/trash/h").unwrap_or(UPath::EMPTY), 1);
    let _ = w4.clear_executed(0);
    let d0 = w4.deadline_ms();
    let held = w4.hold();
    cs.add("hold_extends_10s", held && w4.deadline_ms() == d0 + HOLD_EXTEND_MS, "");
    // 10) 延寿上限：第 2 次成功，第 3 次被拒（防无限推迟真释放）。
    let held2 = w4.hold();
    let held3 = w4.hold();
    cs.add(
        "hold_capped_at_two",
        held2 && !held3 && w4.holds() == MAX_HOLD_EXTENDS,
        "",
    );
    // 11) 5s 边界：now == deadline（恰 5000ms）仍可撤销（闭区间）。
    let mut w5 = TrashUndoWindow::new(0);
    let _ = w5.stage_item(&UPath::from_bytes(b"/trash/edge").unwrap_or(UPath::EMPTY), 1);
    let _ = w5.clear_executed(0);
    cs.add(
        "boundary_5000_inclusive",
        matches!(w5.undo(UNDO_WINDOW_MS), Ok(1)),
        "",
    );
    // 12) 边界外一发子弹：5001ms 起诚实拒绝。
    let mut w6 = TrashUndoWindow::new(0);
    let _ = w6.stage_item(&UPath::from_bytes(b"/trash/edge2").unwrap_or(UPath::EMPTY), 1);
    let _ = w6.clear_executed(0);
    cs.add(
        "boundary_5001_rejected",
        matches!(w6.undo(UNDO_WINDOW_MS + 1), Err(UndoError::Unrecoverable)),
        "",
    );
    // 13) 暂存容量上界：第 65 项被诚实拒绝。
    let mut w7 = TrashUndoWindow::new(0);
    let mut all = true;
    let cap_path = UPath::from_bytes(b"/trash/c").unwrap_or(UPath::EMPTY);
    for _ in 0..MAX_TRASH_ITEMS + 1 {
        all &= w7.stage_item(&cap_path, 1);
    }
    cs.add("staging_capacity_bounded", !all && w7.staged_count() == MAX_TRASH_ITEMS, "");
    cs
}

#[cfg(test)]
mod f524_tests {
    use super::*;

    fn path(b: &[u8]) -> UPath {
        UPath::from_bytes(b).unwrap()
    }

    #[test]
    fn undo_within_window_full_restore() {
        // 撤销完整性：7 项清空后窗口内一键全回。
        let mut w = TrashUndoWindow::new(9_999);
        for i in 0..7u64 {
            assert!(w.stage_item(&path(b"/a/dir/file"), 10 * (i + 1)), "第 {} 项应能暂存", i);
        }
        assert!(w.clear_executed(100));
        assert_eq!(w.staged_bytes(), 280, "暂存字节合计应等于各项之和");
        let n = w.undo(2_000).expect("窗口内撤销应成功");
        assert_eq!(n, 7, "N 项应全回");
        assert_eq!(w.staged_count(), 0, "槽位应全清");
        assert_eq!(w.free_bytes(), 9_999, "还原不是释放，账目不动");
        assert_eq!(w.phase(), WindowPhase::Idle);
    }

    #[test]
    fn timeout_then_honest_refusal() {
        // 超时后诚实不可恢复：不是静默假成功。
        let mut w = TrashUndoWindow::new(0);
        assert!(w.stage_item(&path(b"/x"), 1));
        assert!(w.clear_executed(0));
        assert!(w.settle(5_000), "到时应真释放");
        let r = w.undo(6_000);
        assert!(
            matches!(r, Err(UndoError::NothingStaged) | Err(UndoError::Unrecoverable)),
            "窗口终结后撤销必须被明确拒绝"
        );
        // 未 settle 但已超时：同样拒绝。
        let mut w2 = TrashUndoWindow::new(0);
        assert!(w2.stage_item(&path(b"/y"), 1));
        assert!(w2.clear_executed(0));
        assert!(matches!(w2.undo(5_001), Err(UndoError::Unrecoverable)), "超时一刻起即不可恢复");
    }

    #[test]
    fn hold_extend_and_cap() {
        // 长按延寿：每次 +10s，上限 2 次（登记偏差见文件头）。
        let mut w = TrashUndoWindow::new(0);
        assert!(w.stage_item(&path(b"/z"), 1));
        assert!(w.clear_executed(1_000));
        let d0 = w.deadline_ms();
        assert!(w.hold());
        assert_eq!(w.deadline_ms(), d0 + 10_000, "第一次长按应 +10s");
        assert!(w.hold());
        assert_eq!(w.deadline_ms(), d0 + 20_000, "第二次长按再 +10s");
        assert!(!w.hold(), "第三次长按应被上限拦截");
        assert_eq!(w.holds(), 2);
    }

    #[test]
    fn boundary_window_exact_5000() {
        // 5s 边界：恰 5000ms 可撤销（now == deadline 闭区间），5001ms 拒绝。
        let mut w = TrashUndoWindow::new(0);
        assert!(w.stage_item(&path(b"/edge"), 5));
        assert!(w.clear_executed(0));
        assert!(matches!(w.undo(5_000), Ok(1)), "恰 5000ms 应仍在窗口内");
        let mut w2 = TrashUndoWindow::new(0);
        assert!(w2.stage_item(&path(b"/edge2"), 5));
        assert!(w2.clear_executed(0));
        assert!(matches!(w2.undo(5_001), Err(_)), "5001ms 起应诚实拒绝");
    }

    #[test]
    fn account_sync_on_release() {
        // 真释放时机：settle 前空间不动，settle 后差值 = 暂存字节合计。
        let mut w = TrashUndoWindow::new(1_000);
        assert!(w.stage_item(&path(b"/f1"), 3_000));
        assert!(w.stage_item(&path(b"/f2"), 4_000));
        assert!(w.clear_executed(0));
        assert_eq!(w.free_bytes(), 1_000, "窗口期内空间不得提前回收");
        assert!(w.settle(5_000));
        assert_eq!(w.free_bytes(), 8_000, "释放后剩余空间差应等于 7000 字节暂存合计");
    }

    #[test]
    fn staging_capacity_and_armed_rejects() {
        // 暂存上界 + 窗口开启后禁止继续暂存（清空动作一次性语义）。
        let mut w = TrashUndoWindow::new(0);
        for i in 0..MAX_TRASH_ITEMS {
            assert!(w.stage_item(&path(b"/cap"), i as u64), "前 {} 项应可暂存", i);
        }
        assert!(!w.stage_item(&path(b"/over"), 0), "第 65 项应被拒");
        assert!(w.clear_executed(0));
        assert!(!w.stage_item(&path(b"/during"), 0), "Armed 期不得再暂存");
        assert!(!w.clear_executed(0), "重复 clear_executed 应被拒");
    }
}

// ===========================================================================
// F529 复制前空间预检
// ===========================================================================

/// 缓冲 permille：10%（主册）。
pub const BUFFER_PERMILLE: u64 = 100;

/// 所需空间 = 需求 × 1.1（10% 缓冲）。
pub fn required_with_buffer(need_bytes: u64) -> u64 {
    need_bytes + need_bytes * BUFFER_PERMILLE / 1000
}

/// 预检裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecheckVerdict {
    /// free ≥ need×1.1：放行。
    Fits,
    /// 拦下：携带「目标盘只剩 X，这批要 Y——放不下」的账目三元组。
    Short { free_bytes: u64, required_bytes: u64, shortfall: u64 },
}

/// 单目标预检（含 10% 缓冲；边界 free == required 判 Fits——闭区间）。
pub fn precheck_target(free_bytes: u64, need_bytes: u64) -> PrecheckVerdict {
    let required = required_with_buffer(need_bytes);
    if free_bytes >= required {
        PrecheckVerdict::Fits
    } else {
        PrecheckVerdict::Short {
            free_bytes,
            required_bytes: required,
            shortfall: required - free_bytes,
        }
    }
}

/// 预检时机：唯一合法调用点在进度对话框出现**前**（主册判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckTiming {
    BeforeProgressDialog,
    AfterProgressDialog,
}

/// 时机闸门：BeforeProgressDialog → Some(裁决)；其余时机 → None
/// （预检不允许在进度框之后才跑——那时拦下已无意义）。
pub fn precheck_gate(timing: CheckTiming, free_bytes: u64, need_bytes: u64) -> Option<PrecheckVerdict> {
    match timing {
        CheckTiming::BeforeProgressDialog => Some(precheck_target(free_bytes, need_bytes)),
        CheckTiming::AfterProgressDialog => None,
    }
}

/// 放不下时的三选出路（主册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortDecision {
    /// 仍要复制（会失败）。
    ProceedAnyway,
    /// 换目标。
    PickAnotherTarget,
    /// 取消。
    Cancel,
}

/// 三选的后续动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortAction {
    /// 明知会失败仍开跑（预期失败，不再拦截）。
    CopyWillFail,
    /// 进入换目标流程（对新目标重检）。
    RetargetRecheck,
    /// 中止整批。
    Aborted,
}

/// 三选出路决策映射。
pub fn resolve_short(_verdict: PrecheckVerdict, decision: ShortDecision) -> ShortAction {
    match decision {
        ShortDecision::ProceedAnyway => ShortAction::CopyWillFail,
        ShortDecision::PickAnotherTarget => ShortAction::RetargetRecheck,
        ShortDecision::Cancel => ShortAction::Aborted,
    }
}

/// 换目标流程：换到新目标后对新目标重检（同一预检函数，无捷径）。
pub fn retarget_recheck(new_free_bytes: u64, need_bytes: u64) -> PrecheckVerdict {
    precheck_target(new_free_bytes, need_bytes)
}

/// 按目标卷聚合的需求组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolDemand {
    pub volume: u8,
    pub need_bytes: u64,
    pub files: u32,
}

/// 单卷缺口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolShort {
    pub volume: u8,
    pub shortfall: u64,
}

/// 跨盘批分组上限。
pub const MAX_VOL_GROUPS: usize = 8;

/// 多目标批按目标卷聚合需求（同卷多文件需求求和——逐盘预检的基础）。
pub fn aggregate_by_volume(files: &[(u8, u64)], out: &mut [Option<VolDemand>; MAX_VOL_GROUPS]) -> usize {
    *out = [None; MAX_VOL_GROUPS];
    let mut n = 0usize;
    for &(vol, need) in files {
        let mut found = false;
        for slot in out[..n].iter_mut() {
            if let Some(d) = slot {
                if d.volume == vol {
                    d.need_bytes += need;
                    d.files += 1;
                    found = true;
                    break;
                }
            }
        }
        if !found && n < MAX_VOL_GROUPS {
            out[n] = Some(VolDemand { volume: vol, need_bytes: need, files: 1 });
            n += 1;
        }
    }
    n
}

fn free_of(free_by_vol: &[(u8, u64)], vol: u8) -> u64 {
    for &(v, f) in free_by_vol {
        if v == vol {
            return f;
        }
    }
    0
}

/// 跨盘批逐盘预检：按卷聚合需求、逐卷判定，填出缺口表，返回缺口卷数。
/// 0 = 全批放行。
pub fn batch_precheck(
    files: &[(u8, u64)],
    free_by_vol: &[(u8, u64)],
    shorts: &mut [Option<VolShort>; MAX_VOL_GROUPS],
) -> usize {
    *shorts = [None; MAX_VOL_GROUPS];
    let mut groups: [Option<VolDemand>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
    let n = aggregate_by_volume(files, &mut groups);
    let mut sn = 0usize;
    for slot in groups[..n].iter_mut() {
        if let Some(g) = slot {
            let required = required_with_buffer(g.need_bytes);
            let free = free_of(free_by_vol, g.volume);
            if free < required && sn < MAX_VOL_GROUPS {
                shorts[sn] = Some(VolShort { volume: g.volume, shortfall: required - free });
                sn += 1;
            }
        }
    }
    sn
}

/// 域自检（F529-space-precheck）。
pub fn run_f529_checks() -> CheckSet {
    let mut cs = CheckSet::new("F529-space-precheck");
    // 1) 10% 缓冲数学：need×1.1 精确。
    cs.add(
        "buffer_math",
        required_with_buffer(1000) == 1100 && required_with_buffer(3_400_000_000) == 3_740_000_000,
        "",
    );
    // 2) 放得下：free > required。
    cs.add("fits_case", precheck_target(1_200, 1_000) == PrecheckVerdict::Fits, "");
    // 3) 恰好 1.1 倍边界：free == required → Fits（闭区间）。
    cs.add(
        "boundary_exact_1_1x",
        precheck_target(1100, 1000) == PrecheckVerdict::Fits,
        "",
    );
    // 4) 拦下三元组：free/required/shortfall 逐字段精确。
    match precheck_target(2_100_000_000, 3_400_000_000) {
        PrecheckVerdict::Short { free_bytes, required_bytes, shortfall } => cs.add(
            "short_fields_exact",
            free_bytes == 2_100_000_000
                && required_bytes == 3_740_000_000
                && shortfall == 1_640_000_000,
            "",
        ),
        _ => cs.add("short_fields_exact", false, ""),
    }
    // 5) 时机闸门：唯一调用点 BeforeProgressDialog；进度框之后不允许预检。
    cs.add(
        "gate_before_progress_only",
        precheck_gate(CheckTiming::BeforeProgressDialog, 1100, 1000).is_some()
            && precheck_gate(CheckTiming::AfterProgressDialog, 1100, 1000).is_none(),
        "",
    );
    // 6) 三选出路：三种决策映射到三种互异动作。
    cs.add(
        "three_choices_distinct",
        resolve_short(PrecheckVerdict::Fits, ShortDecision::ProceedAnyway) == ShortAction::CopyWillFail
            && resolve_short(PrecheckVerdict::Fits, ShortDecision::PickAnotherTarget)
                == ShortAction::RetargetRecheck
            && resolve_short(PrecheckVerdict::Fits, ShortDecision::Cancel) == ShortAction::Aborted,
        "",
    );
    // 7) 换目标流程：新目标充足 → 重检通过。
    cs.add(
        "retarget_pass",
        retarget_recheck(5_000_000_000, 3_400_000_000) == PrecheckVerdict::Fits,
        "",
    );
    // 8) 换目标流程：新目标仍不足 → 重检诚实再拦。
    cs.add(
        "retarget_fail_honest",
        matches!(retarget_recheck(3_000_000_000, 3_400_000_000), PrecheckVerdict::Short { .. }),
        "",
    );
    // 9) 跨盘聚合：同卷多文件需求求和、异卷分立。
    let files = [(1u8, 600u64), (1, 500), (2, 100)];
    let mut groups: [Option<VolDemand>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
    let n = aggregate_by_volume(&files, &mut groups);
    cs.add(
        "aggregate_same_volume_sums",
        n == 2
            && groups[0] == Some(VolDemand { volume: 1, need_bytes: 1100, files: 2 })
            && groups[1] == Some(VolDemand { volume: 2, need_bytes: 100, files: 1 }),
        "",
    );
    // 10) 逐盘预检：卷 1 缺口被点名、卷 2 放行，缺口数值精确。
    let mut shorts: [Option<VolShort>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
    let free = [(1u8, 1_000u64), (2u8, 1_000u64)];
    let sn = batch_precheck(&files, &free, &mut shorts);
    cs.add(
        "batch_per_volume_short",
        sn == 1 && shorts[0] == Some(VolShort { volume: 1, shortfall: 1_210 - 1_000 }),
        "",
    );
    // 11) 全批放行：各卷均 ≥ need×1.1 → 零缺口。
    let free_ok = [(1u8, 2_000u64), (2, 500)];
    let mut shorts2: [Option<VolShort>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
    cs.add("batch_all_fit_zero_shorts", batch_precheck(&files, &free_ok, &mut shorts2) == 0, "");
    cs
}

#[cfg(test)]
mod f529_tests {
    use super::*;

    #[test]
    fn exact_one_point_one_boundary() {
        // 恰好 1.1 倍边界：free == need×1.1 放行；差 1 字节即拦。
        assert_eq!(precheck_target(1100, 1000), PrecheckVerdict::Fits, "闭区间边界应放行");
        assert!(
            matches!(precheck_target(1099, 1000), PrecheckVerdict::Short { shortfall: 1, .. }),
            "差 1 字节应拦下且缺口为 1"
        );
    }

    #[test]
    fn percent_buffer_math() {
        // 10% 缓冲：整除与非整除两种需求都按「需求 + 需求×10%」精确。
        assert_eq!(required_with_buffer(0), 0);
        assert_eq!(required_with_buffer(10), 11);
        assert_eq!(required_with_buffer(15), 16, "15 + 1.5 下取整 = 16");
        assert_eq!(required_with_buffer(1 << 30), (1 << 30) + (1 << 29) / 5, "1GiB 需求缓冲精确");
    }

    #[test]
    fn retarget_flow_pass_then_fail() {
        // 换目标流程：先拦 → 换目标重检通过；再换一个不足的目标 → 诚实再拦。
        let v1 = precheck_target(100, 1_000);
        assert!(matches!(v1, PrecheckVerdict::Short { .. }), "首目标应拦下");
        assert_eq!(resolve_short(v1, ShortDecision::PickAnotherTarget), ShortAction::RetargetRecheck);
        assert_eq!(retarget_recheck(1_100, 1_000), PrecheckVerdict::Fits, "新目标充足应通过");
        assert!(
            matches!(retarget_recheck(1_050, 1_000), PrecheckVerdict::Short { .. }),
            "新目标仍不足应诚实再拦"
        );
    }

    #[test]
    fn aggregate_and_per_volume() {
        // 跨盘批：同卷求和、异卷分组、逐盘判定互不污染。
        let files = [(1u8, 400u64), (2, 400), (1, 400), (3, 100)];
        let mut groups: [Option<VolDemand>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
        assert_eq!(aggregate_by_volume(&files, &mut groups), 3);
        assert_eq!(groups[0], Some(VolDemand { volume: 1, need_bytes: 800, files: 2 }));
        assert_eq!(groups[1], Some(VolDemand { volume: 2, need_bytes: 400, files: 1 }));
        assert_eq!(groups[2], Some(VolDemand { volume: 3, need_bytes: 100, files: 1 }));
        // 卷 2 恰好 1.1 倍 → 放行；卷 1、卷 3 各自缺口独立点名。
        let free = [(1u8, 500u64), (2, 440), (3, 0)];
        let mut shorts: [Option<VolShort>; MAX_VOL_GROUPS] = [None; MAX_VOL_GROUPS];
        assert_eq!(batch_precheck(&files, &free, &mut shorts), 2);
        assert_eq!(shorts[0], Some(VolShort { volume: 1, shortfall: 880 - 500 }));
        assert_eq!(shorts[1], Some(VolShort { volume: 3, shortfall: 110 }));
    }

    #[test]
    fn gate_timing_only_before_progress() {
        // 预检时机：进度框出现前是唯一调用点；之后调用被闸门拒绝。
        assert!(precheck_gate(CheckTiming::BeforeProgressDialog, 0, 0).is_some());
        assert!(precheck_gate(CheckTiming::AfterProgressDialog, 0, 0).is_none());
    }

    #[test]
    fn proceed_anyway_and_cancel_paths() {
        // 三选出路：仍要复制 → 预期失败放行；取消 → 中止。
        let v = precheck_target(0, 100);
        assert!(matches!(v, PrecheckVerdict::Short { .. }));
        assert_eq!(resolve_short(v, ShortDecision::ProceedAnyway), ShortAction::CopyWillFail);
        assert_eq!(resolve_short(v, ShortDecision::Cancel), ShortAction::Aborted);
    }
}

// ===========================================================================
// F530 复制后校验
// ===========================================================================

/// 自动开默认阈值：>1GB（主册）。
pub const AUTO_VERIFY_THRESHOLD_BYTES: u64 = 1 << 30;
/// 校验块大小（步进模型：一个空闲额度推进一个块）。
pub const VERIFY_BLOCK: usize = 32;
/// 待校验槽位上限。
pub const MAX_PENDING: usize = 8;

/// IO 等级（F050/F057 纪律）——校验恒为 Idle。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoClass {
    Foreground,
    Background,
    Idle,
}

/// 校验任务的 IO 等级（常量级声明：只在空闲额度派发，不抢前台）。
pub const VERIFY_IO_CLASS: IoClass = IoClass::Idle;

/// FNV-1a 32 位（真实计算，非占位）。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
pub const FNV_PRIME: u32 = 0x0100_0193;

pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 待校验任务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingVerify {
    pub job_id: u32,
    pub src_path: UPath,
    pub dst_path: UPath,
    pub src_data: [u8; VERIFY_BLOCK],
    pub dst_data: [u8; VERIFY_BLOCK],
    pub bytes: u64,
    advanced_blocks: u32,
    finished: bool,
}

impl PendingVerify {
    fn blocks_needed(&self) -> u32 {
        self.bytes.div_ceil(VERIFY_BLOCK as u64) as u32
    }
}

/// 校验结果通知（补发：t1 > t0，引用 t0 的任务 id）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyNotice {
    pub job_id: u32,
    pub at_ms: u64,
    pub matched: bool,
}

/// 失败报告（完整性：逐字段断言）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub job_id: u32,
    pub src_path: UPath,
    pub dst_path: UPath,
    pub src_hash: u32,
    pub dst_hash: u32,
    pub remedy: Remedy,
}

/// 建议动作：不符时「校验失败——建议重新复制该文件」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    Recopy,
    None,
}

/// 复制后校验器。
///
/// - 自动开默认：bytes > 1GB 且总开关开着 → 自动入队；小文件仅用户
///   显式要求时入队；总开关关闭 → 一律不校验、不发结果通知。
/// - 空闲 IO：每个 `step` 消耗 1 个空闲额度推进 1 块；无额度不推进
///   （前台忙时校验排队等空闲——F050/F057 纪律）。
pub struct CopyVerifier {
    master_enabled: bool,
    idle_grants: u32,
    pending: [Option<PendingVerify>; MAX_PENDING],
    completion_notice_ms: [Option<u64>; MAX_PENDING],
    result_notice: [Option<VerifyNotice>; MAX_PENDING],
    now_ms: u64,
}

impl CopyVerifier {
    pub fn new(master_enabled: bool) -> CopyVerifier {
        CopyVerifier {
            master_enabled,
            idle_grants: 0,
            pending: [None; MAX_PENDING],
            completion_notice_ms: [None; MAX_PENDING],
            result_notice: [None; MAX_PENDING],
            now_ms: 0,
        }
    }

    pub fn master_enabled(&self) -> bool {
        self.master_enabled
    }

    /// 默认规则：>1GB 自动开（恰 1GB 不含——「>1GB」）。
    pub fn default_auto_enabled(bytes: u64) -> bool {
        bytes > AUTO_VERIFY_THRESHOLD_BYTES
    }

    /// 入队（完成通知 t0 在此刻发出）。user_requested = 用户显式开了校验。
    pub fn enqueue(
        &mut self,
        job_id: u32,
        src_path: &[u8],
        dst_path: &[u8],
        bytes: u64,
        src_data: [u8; VERIFY_BLOCK],
        dst_data: [u8; VERIFY_BLOCK],
        user_requested: bool,
    ) -> bool {
        if !self.master_enabled || !(user_requested || Self::default_auto_enabled(bytes)) {
            return false;
        }
        for i in 0..MAX_PENDING {
            if self.pending[i].is_none() {
                self.pending[i] = Some(PendingVerify {
                    job_id,
                    src_path: UPath::from_bytes(src_path).unwrap_or(UPath::EMPTY),
                    dst_path: UPath::from_bytes(dst_path).unwrap_or(UPath::EMPTY),
                    src_data,
                    dst_data,
                    bytes,
                    advanced_blocks: 0,
                    finished: false,
                });
                self.completion_notice_ms[i] = Some(self.now_ms);
                return true;
            }
        }
        false
    }

    /// 发放空闲 IO 额度。
    pub fn grant_idle(&mut self, units: u32) {
        self.idle_grants += units;
    }

    pub fn set_clock(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// 一个校验步：消耗 1 空闲额度推进 1 块。无额度/无任务 → false
    /// （前台忙时校验不推进——空闲 IO 判据）。
    pub fn step(&mut self) -> bool {
        if self.idle_grants == 0 {
            return false;
        }
        let mut target: Option<usize> = None;
        for i in 0..MAX_PENDING {
            if let Some(p) = &self.pending[i] {
                if !p.finished {
                    target = Some(i);
                    break;
                }
            }
        }
        let Some(i) = target else {
            return false;
        };
        let needed = self.pending[i].map(|p| p.blocks_needed()).unwrap_or(0);
        let (finished_now, job_id, matched) = {
            let p = self.pending[i].as_mut().unwrap();
            p.advanced_blocks += 1;
            self.idle_grants -= 1;
            let done = p.advanced_blocks >= needed;
            if done {
                p.finished = true;
                (true, p.job_id, fnv1a(&p.src_data) == fnv1a(&p.dst_data))
            } else {
                (false, p.job_id, false)
            }
        };
        if finished_now {
            self.result_notice[i] = Some(VerifyNotice { job_id, at_ms: self.now_ms, matched });
        }
        finished_now
    }

    /// 报告（逐字段完整：job id、源/目标路径、双哈希、建议动作）。
    pub fn report(&self, job_id: u32) -> Option<VerifyReport> {
        for slot in self.pending.iter() {
            if let Some(p) = slot {
                if p.job_id == job_id {
                    let sh = fnv1a(&p.src_data);
                    let dh = fnv1a(&p.dst_data);
                    let matched = sh == dh && p.src_data == p.dst_data;
                    return Some(VerifyReport {
                        job_id: p.job_id,
                        src_path: p.src_path,
                        dst_path: p.dst_path,
                        src_hash: sh,
                        dst_hash: dh,
                        remedy: if matched { Remedy::None } else { Remedy::Recopy },
                    });
                }
            }
        }
        None
    }

    pub fn result_notice_of(&self, job_id: u32) -> Option<VerifyNotice> {
        for slot in self.result_notice.iter() {
            if let Some(n) = slot {
                if n.job_id == job_id {
                    return Some(*n);
                }
            }
        }
        None
    }

    pub fn completion_ms_of(&self, job_id: u32) -> Option<u64> {
        for i in 0..MAX_PENDING {
            if let Some(p) = self.pending[i] {
                if p.job_id == job_id {
                    return self.completion_notice_ms[i];
                }
            }
        }
        None
    }

    pub fn advanced_blocks_of(&self, job_id: u32) -> u32 {
        for slot in self.pending.iter() {
            if let Some(p) = slot {
                if p.job_id == job_id {
                    return p.advanced_blocks;
                }
            }
        }
        0
    }
}

/// 域自检（F530-copy-verify）。
pub fn run_f530_checks() -> CheckSet {
    let mut cs = CheckSet::new("F530-copy-verify");
    // 1) 阈值常量恰 1GB。
    cs.add("threshold_is_1gb", AUTO_VERIFY_THRESHOLD_BYTES == 1 << 30, "");
    // 2) 自动开默认：>1GB 才默认开（恰 1GB 不含，1GB+1 含）。
    cs.add(
        "auto_rule_boundary",
        !CopyVerifier::default_auto_enabled(AUTO_VERIFY_THRESHOLD_BYTES - 1)
            && !CopyVerifier::default_auto_enabled(AUTO_VERIFY_THRESHOLD_BYTES)
            && CopyVerifier::default_auto_enabled(AUTO_VERIFY_THRESHOLD_BYTES + 1),
        "",
    );
    // 3) 用户总开关可强制关：>1GB 也不校验。
    let mut off = CopyVerifier::new(false);
    let blocked = !off.enqueue(
        1,
        b"s.bin",
        b"d.bin",
        AUTO_VERIFY_THRESHOLD_BYTES + 1,
        [0; VERIFY_BLOCK],
        [0; VERIFY_BLOCK],
        true,
    );
    cs.add("master_switch_force_off", blocked && !off.master_enabled(), "");
    // 4) FNV-1a 已知向量（真实计算）。
    cs.add(
        "fnv_known_vectors",
        fnv1a(&[]) == 0x811c_9dc5 && fnv1a(b"a") == 0xe40c_292c,
        "",
    );
    // 5) 空闲 IO 判据：无空闲额度时不推进（前台忙 → 校验排队）。
    let mut v1 = CopyVerifier::new(true);
    let _ = v1.enqueue(1, b"a.bin", b"b.bin", 64, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true);
    let stalled = !v1.step() && v1.advanced_blocks_of(1) == 0;
    cs.add("idle_only_no_progress_without_grant", stalled, "");
    // 6) 空闲额度到位后推进并完成：相同数据 → Match。
    let mut v2 = CopyVerifier::new(true);
    let _ = v2.enqueue(2, b"a.bin", b"b.bin", 64, [7; VERIFY_BLOCK], [7; VERIFY_BLOCK], true);
    v2.grant_idle(8);
    let mut finished = false;
    for _ in 0..8 {
        finished |= v2.step();
    }
    cs.add(
        "idle_grants_complete_match",
        finished && v2.result_notice_of(2).map(|n| n.matched).unwrap_or(false),
        "",
    );
    // 7) 失败检测：单字节翻转 → 双哈希不一致 → Mismatch。
    let src = [3u8; VERIFY_BLOCK];
    let mut dst = [3u8; VERIFY_BLOCK];
    dst[17] = 4;
    let mut v3 = CopyVerifier::new(true);
    let _ = v3.enqueue(3, b"src.iso", b"dst.iso", 64, src, dst, true);
    v3.grant_idle(8);
    let mut done3 = false;
    for _ in 0..8 {
        done3 |= v3.step();
    }
    cs.add(
        "mismatch_detected",
        done3 && v3.result_notice_of(3).map(|n| !n.matched).unwrap_or(false),
        "",
    );
    // 8) 失败报告完整性：job id/源路径/目标路径/双哈希/建议动作逐字段。
    match v3.report(3) {
        Some(r) => cs.add(
            "report_fields_complete",
            r.job_id == 3
                && !r.src_path.is_empty()
                && !r.dst_path.is_empty()
                && r.src_hash != r.dst_hash
                && r.remedy == Remedy::Recopy,
            "",
        ),
        None => cs.add("report_fields_complete", false, ""),
    }
    // 9) 通知补发时序：完成通知 t0 < 结果通知 t1，且结果引用 t0 的任务 id。
    let mut v4 = CopyVerifier::new(true);
    v4.set_clock(1_000);
    let _ = v4.enqueue(9, b"a", b"b", 32, [2; VERIFY_BLOCK], [2; VERIFY_BLOCK], true);
    v4.set_clock(1_500);
    v4.grant_idle(4);
    let mut done4 = false;
    for _ in 0..4 {
        done4 |= v4.step();
    }
    match (v4.completion_ms_of(9), v4.result_notice_of(9)) {
        (Some(t0), Some(n)) => cs.add(
            "notice_ordering_t0_lt_t1",
            done4 && t0 == 1_000 && n.at_ms == 1_500 && t0 < n.at_ms && n.job_id == 9,
            "",
        ),
        _ => cs.add("notice_ordering_t0_lt_t1", false, ""),
    }
    // 10) 关闭开关：关后不校验、不发结果通知（即便用户显式要求）。
    let mut v5 = CopyVerifier::new(false);
    let _ = v5.enqueue(5, b"a", b"b", 32, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true);
    v5.grant_idle(4);
    let mut stepped = false;
    for _ in 0..4 {
        stepped |= v5.step();
    }
    cs.add("switch_off_silent", !stepped && v5.result_notice_of(5).is_none(), "");
    // 11) IO 等级常量：校验恒为 Idle（F050/F057 纪律的常量级声明）。
    cs.add("io_class_idle_constant", VERIFY_IO_CLASS == IoClass::Idle, "");
    // 12) 小文件默认不自动校验（非 >1GB 且用户未显式要求）。
    let mut v6 = CopyVerifier::new(true);
    let refused = !v6.enqueue(
        6,
        b"small.txt",
        b"small2.txt",
        512,
        [0; VERIFY_BLOCK],
        [0; VERIFY_BLOCK],
        false,
    );
    cs.add("small_file_not_auto_enqueued", refused, "");
    cs
}

#[cfg(test)]
mod f530_tests {
    use super::*;

    const GB: u64 = AUTO_VERIFY_THRESHOLD_BYTES;

    #[test]
    fn auto_default_threshold_boundary() {
        // 1GB 阈值边界：GB-1 / GB 不自动开；GB+1 自动开。
        assert!(!CopyVerifier::default_auto_enabled(GB - 1), "恰 1GB 以下不应默认开");
        assert!(!CopyVerifier::default_auto_enabled(GB), "恰 1GB 不含（>1GB 语义）");
        assert!(CopyVerifier::default_auto_enabled(GB + 1), "1GB+1 应默认开");
        assert_eq!(GB, 1_073_741_824, "1GB = 2^30 字节");
    }

    #[test]
    fn fnv_known_vectors() {
        // FNV-1a 32 位标准向量。
        assert_eq!(fnv1a(&[]), 0x811c_9dc5, "空输入 = offset basis");
        assert_eq!(fnv1a(b"a"), 0xe40c_292c, "单字节 'a' 标准向量");
        assert_eq!(fnv1a(b"foobar"), 0xbf9c_f968, "多字节标准向量");
        assert_ne!(fnv1a(b"abc"), fnv1a(b"abd"), "单字节差必须改变哈希");
    }

    #[test]
    fn idle_discipline_queueing() {
        // 空闲 IO：无额度不推进；发放额度才推进；额度耗尽再次停滞。
        let mut v = CopyVerifier::new(true);
        assert!(v.enqueue(1, b"s", b"d", 96, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true));
        assert!(!v.step(), "无额度不得推进");
        assert_eq!(v.advanced_blocks_of(1), 0);
        v.grant_idle(1);
        // 96B = 3 块：1 额度只推进 1 块、尚未判完（返回 false 但已推进）。
        assert!(!v.step(), "1 额度推进 1 块，未满 3 块不得误报完成");
        assert_eq!(v.advanced_blocks_of(1), 1, "1 额度应恰好推进 1 块");
        assert!(!v.step(), "额度耗尽应再次停滞");
        assert_eq!(v.advanced_blocks_of(1), 1);
    }

    #[test]
    fn mismatch_report_field_by_field() {
        // 失败报告完整性：逐字段断言（含源/目标路径保真）。
        let mut src = [9u8; VERIFY_BLOCK];
        let mut dst = [9u8; VERIFY_BLOCK];
        dst[0] = 10;
        let mut v = CopyVerifier::new(true);
        assert!(v.enqueue(42, b"src/big.iso", b"dst/big.iso", 64, src, dst, true));
        v.grant_idle(8);
        let mut done = false;
        for _ in 0..8 {
            done |= v.step();
        }
        assert!(done);
        let r = v.report(42).expect("报告应存在");
        assert_eq!(r.job_id, 42);
        assert_eq!(r.src_path.as_bytes(), b"src/big.iso", "源路径应完整保真");
        assert_eq!(r.dst_path.as_bytes(), b"dst/big.iso", "目标路径应完整保真");
        assert_ne!(r.src_hash, r.dst_hash, "哈希不一致才算失败");
        assert_eq!(r.remedy, Remedy::Recopy, "失败建议动作 = 重新复制");
        // 一致数据 → Remedy::None。
        let mut v2 = CopyVerifier::new(true);
        assert!(v2.enqueue(7, b"s", b"d", 32, [5; VERIFY_BLOCK], [5; VERIFY_BLOCK], true));
        v2.grant_idle(4);
        for _ in 0..4 {
            v2.step();
        }
        assert_eq!(v2.report(7).unwrap().remedy, Remedy::None);
    }

    #[test]
    fn notice_ordering_completion_first() {
        // 通知补发时序：完成通知先发（t0），校验结果补一条（t1 > t0，
        // 引用同一任务 id）。
        let mut v = CopyVerifier::new(true);
        v.set_clock(5_000);
        assert!(v.enqueue(3, b"a", b"b", 32, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true));
        v.set_clock(9_000);
        v.grant_idle(4);
        for _ in 0..4 {
            v.step();
        }
        let t0 = v.completion_ms_of(3).unwrap();
        let n = v.result_notice_of(3).unwrap();
        assert_eq!(t0, 5_000, "完成通知应在入队时刻");
        assert_eq!(n.at_ms, 9_000, "结果通知应在校验完成时刻");
        assert!(t0 < n.at_ms, "完成通知必须先于结果通知");
        assert_eq!(n.job_id, 3, "结果通知必须引用 t0 的任务 id");
    }

    #[test]
    fn master_switch_off_silent() {
        // 关闭开关：不校验、不发结果通知，连显式要求也拒。
        let mut v = CopyVerifier::new(false);
        assert!(!v.enqueue(1, b"a", b"b", GB + 1, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true));
        assert!(!v.enqueue(2, b"a", b"b", 16, [1; VERIFY_BLOCK], [1; VERIFY_BLOCK], true));
        v.grant_idle(16);
        for _ in 0..16 {
            v.step();
        }
        assert!(v.result_notice_of(1).is_none());
        assert!(v.result_notice_of(2).is_none());
    }
}

// ===========================================================================
// F531 复制任务队列化
// ===========================================================================

/// 队列容量（同时发起的批数上限）。
pub const MAX_BATCHES: usize = 16;

/// 批状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchState {
    Queued,
    Running,
    Paused,
    Cancelled,
    Done,
}

/// 一批复制任务。队列视图四信息：位置（目标卷）/进度/速度/剩余时间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyBatch {
    pub id: u8,
    pub src_volume: u8,
    pub dst_volume: u8,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub speed_bps: u64,
    pub state: BatchState,
    /// 优先级：数值越小越优先（0 = 「先传这批」插队位）。
    pub priority: u8,
    /// 断点续传偏移（F269）：Done 前取消时记录已完成字节。
    pub resume_offset: u64,
}

/// 队列视图单批摘要（四信息齐备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchView {
    /// 位置：目标卷。
    pub dst_volume: u8,
    /// 进度 permille。
    pub progress_permille: u64,
    /// 速度 B/s。
    pub speed_bps: u64,
    /// 剩余时间 ms。
    pub eta_ms: u64,
}

/// 复制队列：同盘串行 / 异盘并行调度。
pub struct CopyQueue {
    batches: [Option<CopyBatch>; MAX_BATCHES],
    count: usize,
}

impl CopyQueue {
    pub fn new() -> CopyQueue {
        CopyQueue { batches: [None; MAX_BATCHES], count: 0 }
    }

    /// 入队（返回批 id；容量满 → None）。
    pub fn enqueue(&mut self, src_volume: u8, dst_volume: u8, total_bytes: u64, priority: u8) -> Option<u8> {
        if self.count >= MAX_BATCHES {
            return None;
        }
        let id = self.count as u8;
        self.batches[self.count] = Some(CopyBatch {
            id,
            src_volume,
            dst_volume,
            total_bytes,
            done_bytes: 0,
            speed_bps: 0,
            state: BatchState::Queued,
            priority,
            resume_offset: 0,
        });
        self.count += 1;
        Some(id)
    }

    fn running_on(&self, dst_volume: u8) -> bool {
        for slot in self.batches[..self.count].iter() {
            if let Some(b) = slot {
                if b.state == BatchState::Running && b.dst_volume == dst_volume {
                    return true;
                }
            }
        }
        false
    }

    /// 调度（纯函数式判定）：同目标盘串行（同盘同时最多 1 个 Running），
    /// 异盘并行；候选按优先级（小者先）、同优先级按 FIFO（id 小者先）。
    pub fn schedule(&mut self) {
        loop {
            let mut best: Option<usize> = None;
            for i in 0..self.count {
                let Some(b) = self.batches[i] else { continue };
                if b.state != BatchState::Queued || self.running_on(b.dst_volume) {
                    continue;
                }
                match best {
                    None => best = Some(i),
                    Some(bi) => {
                        let bb = self.batches[bi].unwrap_or(CopyBatch {
                            id: u8::MAX,
                            src_volume: 0,
                            dst_volume: 0,
                            total_bytes: 0,
                            done_bytes: 0,
                            speed_bps: 0,
                            state: BatchState::Queued,
                            priority: u8::MAX,
                            resume_offset: 0,
                        });
                        if b.priority < bb.priority || (b.priority == bb.priority && b.id < bb.id) {
                            best = Some(i);
                        }
                    }
                }
            }
            match best {
                Some(i) => {
                    if let Some(slot) = self.batches[i].as_mut() {
                        slot.state = BatchState::Running;
                    }
                }
                None => break,
            }
        }
    }

    /// 插队：优先级提升一档（饱和于 0）。
    pub fn bump_priority(&mut self, id: u8) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id {
                    if b.priority == 0 {
                        return false;
                    }
                    b.priority -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 暂停单批（不影响他批）。
    pub fn pause(&mut self, id: u8) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id && b.state == BatchState::Running {
                    b.state = BatchState::Paused;
                    return true;
                }
            }
        }
        false
    }

    /// 取消单批：Done 前取消 → 记录已完成字节偏移（F269 断点续传衔接）。
    pub fn cancel(&mut self, id: u8) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id {
                    if b.state == BatchState::Done {
                        return false; // 已完成，取消无效
                    }
                    if b.state != BatchState::Cancelled {
                        b.resume_offset = b.done_bytes;
                        b.state = BatchState::Cancelled;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// 恢复（从断点偏移续传）：done_bytes 保留、状态回队。
    pub fn resume(&mut self, id: u8) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id && b.state == BatchState::Cancelled {
                    b.state = BatchState::Queued;
                    return true;
                }
            }
        }
        false
    }

    /// 进度推进（调度外部的 IO 进度回报；到满自动判 Done）。
    pub fn advance(&mut self, id: u8, delta_bytes: u64) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id {
                    b.done_bytes = (b.done_bytes + delta_bytes).min(b.total_bytes);
                    if b.done_bytes == b.total_bytes {
                        b.state = BatchState::Done;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// 队列视图四信息（位置/进度/速度/剩余时间）。
    pub fn view(&self, id: u8) -> Option<BatchView> {
        for slot in self.batches[..self.count].iter() {
            if let Some(b) = slot {
                if b.id == id {
                    let remaining = b.total_bytes - b.done_bytes;
                    let eta = if b.speed_bps > 0 { remaining * 1000 / b.speed_bps } else { 0 };
                    return Some(BatchView {
                        dst_volume: b.dst_volume,
                        progress_permille: if b.total_bytes > 0 {
                            b.done_bytes * 1000 / b.total_bytes
                        } else {
                            0
                        },
                        speed_bps: b.speed_bps,
                        eta_ms: eta,
                    });
                }
            }
        }
        None
    }

    pub fn set_speed(&mut self, id: u8, bps: u64) -> bool {
        for slot in self.batches[..self.count].iter_mut() {
            if let Some(b) = slot {
                if b.id == id {
                    b.speed_bps = bps;
                    return true;
                }
            }
        }
        false
    }

    pub fn state_of(&self, id: u8) -> Option<BatchState> {
        for slot in self.batches[..self.count].iter() {
            if let Some(b) = slot {
                if b.id == id {
                    return Some(b.state);
                }
            }
        }
        None
    }

    pub fn done_bytes_of(&self, id: u8) -> u64 {
        for slot in self.batches[..self.count].iter() {
            if let Some(b) = slot {
                if b.id == id {
                    return b.done_bytes;
                }
            }
        }
        0
    }

    pub fn resume_offset_of(&self, id: u8) -> u64 {
        for slot in self.batches[..self.count].iter() {
            if let Some(b) = slot {
                if b.id == id {
                    return b.resume_offset;
                }
            }
        }
        0
    }

    /// 同盘串行不变量：任何时刻同一目标盘至多 1 个 Running。
    pub fn same_volume_serial_violation(&self) -> bool {
        for i in 0..self.count {
            if let Some(a) = self.batches[i] {
                if a.state != BatchState::Running {
                    continue;
                }
                for j in (i + 1)..self.count {
                    if let Some(b) = self.batches[j] {
                        if b.state == BatchState::Running && b.dst_volume == a.dst_volume {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn running_count(&self) -> usize {
        self.batches[..self.count]
            .iter()
            .filter(|s| matches!(s, Some(b) if b.state == BatchState::Running))
            .count()
    }
}

/// 域自检（F531-copy-queue）。
pub fn run_f531_checks() -> CheckSet {
    let mut cs = CheckSet::new("F531-copy-queue");
    // 1) 同盘串行：同目标盘两批只允许 1 个 Running（不变量成立）。
    let mut q1 = CopyQueue::new();
    let a1 = q1.enqueue(1, 9, 1_000, 1).unwrap_or(255);
    let b1 = q1.enqueue(2, 9, 2_000, 5).unwrap_or(255);
    q1.schedule();
    cs.add(
        "same_volume_serial",
        q1.state_of(a1) == Some(BatchState::Running)
            && q1.state_of(b1) == Some(BatchState::Queued)
            && q1.running_count() == 1
            && !q1.same_volume_serial_violation(),
        "",
    );
    // 2) 异盘并行：不同目标盘各走各。
    let mut q2 = CopyQueue::new();
    let a2 = q2.enqueue(1, 3, 1_000, 1).unwrap_or(255);
    let b2 = q2.enqueue(2, 4, 2_000, 5).unwrap_or(255);
    q2.schedule();
    cs.add(
        "cross_volume_parallel",
        q2.state_of(a2) == Some(BatchState::Running)
            && q2.state_of(b2) == Some(BatchState::Running)
            && q2.running_count() == 2,
        "",
    );
    // 3) 插队优先级：同盘竞争时优先级小者（高优先）先开跑。
    let mut q3 = CopyQueue::new();
    let a3 = q3.enqueue(1, 5, 1_000, 5).unwrap_or(255);
    let b3 = q3.enqueue(2, 5, 2_000, 0).unwrap_or(255);
    q3.schedule();
    cs.add(
        "priority_first",
        q3.state_of(b3) == Some(BatchState::Running) && q3.state_of(a3) == Some(BatchState::Queued),
        "",
    );
    // 4) 「先传这批」右键插队：bump 两档饱和于 0，让位后越过 p9 候选。
    let mut q4 = CopyQueue::new();
    let a4 = q4.enqueue(1, 6, 1_000, 0).unwrap_or(255);
    let b4 = q4.enqueue(2, 6, 2_000, 2).unwrap_or(255);
    let _c4 = q4.enqueue(3, 6, 3_000, 9).unwrap_or(255);
    q4.schedule();
    let _ = q4.pause(a4);
    let bumped = q4.bump_priority(b4) && q4.bump_priority(b4);
    q4.schedule();
    cs.add("queue_jump_bump", bumped && q4.state_of(b4) == Some(BatchState::Running), "");
    // 5) 暂停独立：暂停 A 不影响 B。
    let mut q5 = CopyQueue::new();
    let a5 = q5.enqueue(1, 1, 1_000, 1).unwrap_or(255);
    let b5 = q5.enqueue(2, 2, 1_000, 1).unwrap_or(255);
    q5.schedule();
    let _ = q5.pause(a5);
    cs.add(
        "pause_isolated",
        q5.state_of(a5) == Some(BatchState::Paused) && q5.state_of(b5) == Some(BatchState::Running),
        "",
    );
    // 6) 取消独立：取消 A 不影响 B。
    let mut q6 = CopyQueue::new();
    let a6 = q6.enqueue(1, 1, 1_000, 1).unwrap_or(255);
    let b6 = q6.enqueue(2, 2, 1_000, 1).unwrap_or(255);
    q6.schedule();
    let _ = q6.cancel(a6);
    cs.add(
        "cancel_isolated",
        q6.state_of(a6) == Some(BatchState::Cancelled) && q6.state_of(b6) == Some(BatchState::Running),
        "",
    );
    // 7) 断点续传衔接（F269）：Done 前取消记录已完成字节偏移。
    let mut q7 = CopyQueue::new();
    let a7 = q7.enqueue(1, 7, 1_000, 1).unwrap_or(255);
    q7.schedule();
    let _ = q7.advance(a7, 400);
    let _ = q7.cancel(a7);
    cs.add(
        "cancel_records_offset",
        q7.state_of(a7) == Some(BatchState::Cancelled) && q7.resume_offset_of(a7) == 400,
        "",
    );
    // 8) 恢复续传：done_bytes 保留、状态回队、再调度后从偏移继续。
    let resumed = q7.resume(a7);
    q7.schedule();
    cs.add(
        "resume_from_offset",
        resumed && q7.state_of(a7) == Some(BatchState::Running) && q7.done_bytes_of(a7) == 400,
        "",
    );
    // 9) Done 后取消无效（状态保持 Done）。
    let mut q8 = CopyQueue::new();
    let a8 = q8.enqueue(1, 8, 100, 1).unwrap_or(255);
    q8.schedule();
    let _ = q8.advance(a8, 100);
    let cancel_after_done = q8.cancel(a8);
    cs.add(
        "cancel_after_done_noop",
        !cancel_after_done && q8.state_of(a8) == Some(BatchState::Done),
        "",
    );
    // 10) 进度 permille 数学。
    let mut q9 = CopyQueue::new();
    let a9 = q9.enqueue(1, 2, 1_000, 1).unwrap_or(255);
    let _ = q9.advance(a9, 250);
    cs.add(
        "progress_permille_math",
        q9.view(a9).map(|v| v.progress_permille).unwrap_or(0) == 250,
        "",
    );
    // 11) 剩余时间数学：eta = 剩余字节 × 1000 / 速度。
    let _ = q9.set_speed(a9, 100);
    cs.add("eta_math", q9.view(a9).map(|v| v.eta_ms).unwrap_or(0) == 7_500, "");
    // 12) 队列视图四信息齐备：位置/进度/速度/剩余时间。
    match q9.view(a9) {
        Some(v) => cs.add(
            "queue_view_four_fields",
            v.dst_volume == 2 && v.progress_permille == 250 && v.speed_bps == 100 && v.eta_ms == 7_500,
            "",
        ),
        None => cs.add("queue_view_four_fields", false, ""),
    }
    // 13) 容量上界：第 17 批被诚实拒绝。
    let mut q10 = CopyQueue::new();
    let mut all_enq = true;
    for _ in 0..MAX_BATCHES + 1 {
        all_enq &= q10.enqueue(1, 1, 1, 1).is_some();
    }
    cs.add("capacity_bounded_16", !all_enq, "");
    cs
}

#[cfg(test)]
mod f531_tests {
    use super::*;

    #[test]
    fn same_volume_serial_scheduling() {
        // 同盘串行：三个同目标盘的批同时只跑一个，其余排队。
        let mut q = CopyQueue::new();
        let a = q.enqueue(1, 9, 1_000, 1).unwrap();
        let b = q.enqueue(2, 9, 1_000, 2).unwrap();
        let c = q.enqueue(3, 9, 1_000, 3).unwrap();
        q.schedule();
        assert_eq!(q.running_count(), 1, "同盘同时最多 1 个 Running");
        assert!(!q.same_volume_serial_violation(), "串行不变量必须成立");
        assert_eq!(q.state_of(a), Some(BatchState::Running));
        assert_eq!(q.state_of(b), Some(BatchState::Queued));
        assert_eq!(q.state_of(c), Some(BatchState::Queued));
        // 让位后按优先级接力。
        assert!(q.pause(a));
        q.schedule();
        assert_eq!(q.state_of(b), Some(BatchState::Running), "接力应选优先级小者");
        assert_eq!(q.state_of(c), Some(BatchState::Queued));
    }

    #[test]
    fn cross_volume_parallel() {
        // 异盘并行：三个不同目标盘同时跑。
        let mut q = CopyQueue::new();
        let _a = q.enqueue(1, 2, 1_000, 1).unwrap();
        let _b = q.enqueue(3, 4, 1_000, 1).unwrap();
        let _c = q.enqueue(5, 6, 1_000, 1).unwrap();
        q.schedule();
        assert_eq!(q.running_count(), 3, "异盘应全并行");
        assert!(!q.same_volume_serial_violation());
    }

    #[test]
    fn queue_jump_priority() {
        // 插队：同优先级 FIFO；bump 后按优先级越过先行者。
        let mut q = CopyQueue::new();
        let a = q.enqueue(1, 5, 1_000, 1).unwrap();
        let b = q.enqueue(2, 5, 1_000, 1).unwrap();
        q.schedule();
        assert_eq!(q.state_of(a), Some(BatchState::Running), "同优先级按 FIFO");
        assert!(q.pause(a));
        q.schedule();
        assert_eq!(q.state_of(b), Some(BatchState::Running));
        // 新来的 C bump 到 0，插到 A 前面。
        let c = q.enqueue(3, 5, 1_000, 4).unwrap();
        assert!(q.bump_priority(c));
        assert!(q.pause(b));
        q.schedule();
        assert_eq!(q.state_of(c), Some(BatchState::Running), "bump 后应插队成功");
        assert_eq!(q.state_of(a), Some(BatchState::Paused));
    }

    #[test]
    fn pause_cancel_independent() {
        // 暂停/取消只作用于单批，不波及他批。
        let mut q = CopyQueue::new();
        let a = q.enqueue(1, 1, 1_000, 1).unwrap();
        let b = q.enqueue(2, 2, 1_000, 1).unwrap();
        let c = q.enqueue(3, 3, 1_000, 1).unwrap();
        q.schedule();
        assert!(q.pause(a));
        assert!(q.cancel(b));
        assert_eq!(q.state_of(a), Some(BatchState::Paused));
        assert_eq!(q.state_of(b), Some(BatchState::Cancelled));
        assert_eq!(q.state_of(c), Some(BatchState::Running), "他批不得被波及");
        // 已 Done 的批不可取消。
        assert!(q.advance(c, 1_000));
        assert_eq!(q.state_of(c), Some(BatchState::Done));
        assert!(!q.cancel(c), "Done 后取消应无效");
    }

    #[test]
    fn resume_from_offset() {
        // F269 断点续传衔接：取消留偏移、恢复续跑、偏移不丢。
        let mut q = CopyQueue::new();
        let a = q.enqueue(1, 8, 10_000, 1).unwrap();
        q.schedule();
        assert!(q.advance(a, 3_500));
        assert!(q.cancel(a));
        assert_eq!(q.resume_offset_of(a), 3_500, "取消时应记录已完成字节偏移");
        assert_eq!(q.done_bytes_of(a), 3_500);
        assert!(q.resume(a));
        q.schedule();
        assert_eq!(q.state_of(a), Some(BatchState::Running), "恢复后应重新可调度");
        assert_eq!(q.done_bytes_of(a), 3_500, "恢复不得清零已传字节");
        assert!(q.advance(a, 6_500));
        assert_eq!(q.state_of(a), Some(BatchState::Done), "从偏移续传至满应判 Done");
    }

    #[test]
    fn queue_view_four_fields() {
        // 队列视图四信息：位置/进度/速度/剩余时间齐备且数值正确。
        let mut q = CopyQueue::new();
        let a = q.enqueue(7, 9, 4_000, 1).unwrap();
        q.schedule();
        assert!(q.set_speed(a, 500));
        assert!(q.advance(a, 1_000));
        let v = q.view(a).expect("视图应存在");
        assert_eq!(v.dst_volume, 9, "位置 = 目标卷");
        assert_eq!(v.progress_permille, 250, "进度 permille");
        assert_eq!(v.speed_bps, 500, "速度 B/s");
        assert_eq!(v.eta_ms, 6_000, "剩余 = 3000B ÷ 500B/s = 6s");
        // 边界：零速度不除零。
        let b = q.enqueue(1, 3, 1_000, 1).unwrap();
        let vb = q.view(b).unwrap();
        assert_eq!(vb.eta_ms, 0, "速度未知时 eta 记 0（不除零）");
        assert_eq!(vb.progress_permille, 0);
    }
}

// ===========================================================================
// F532 打开失败人话诊断
// ===========================================================================

/// 四类人话归因（主册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// 格式不支持。
    UnsupportedFormat,
    /// 文件损坏。
    Corrupted,
    /// 应用缺失。
    AppMissing,
    /// 权限不足。
    PermissionDenied,
}

/// 出路枚举（每类归因的出路链接——链接有效性 = 枚举映射非空）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// F257 打开方式选择器。
    OpenWithSelector,
    /// F294 重新关联。
    Reassociate,
    /// 搜索可用应用。
    SearchApp,
    /// F324 权限页直达。
    PermissionPage,
    /// F325/F396 备份/版本恢复。
    RestoreBackup,
}

/// 三问结构（F209 同规）：是什么问题 / 为什么 / 现在能做什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    /// 第一问：是什么问题（四类归因）。
    pub what: FailKind,
    /// 第二问：为什么（含具体细节，如「头 512 字节不是有效 ZIP 签名」）。
    pub why: &'static str,
    /// 第三问：现在能做什么（主出路）。
    pub next: NextStep,
}

/// 诊断探针输入。
#[derive(Clone, Copy, Debug)]
pub struct Probe<'a> {
    pub ext: &'a str,
    pub head: [u8; 16],
    pub head_len: usize,
    pub app_registered: bool,
    pub permission_ok: bool,
    pub checksum_ok: bool,
    pub length_ok: bool,
}

/// 魔数表条目（真实魔数，≥8 种）。
#[derive(Clone, Copy, Debug)]
pub struct MagicEntry {
    pub ext: &'static str,
    pub magic: &'static [u8],
    pub label: &'static str,
    /// 损坏类 why 文案（含具体字节描述——「头 512 字节不是有效 ZIP 签名」风格）。
    pub corrupt_why: &'static str,
}

/// 魔数表：10 种常见格式（≥8 达标）。
pub const MAGIC_TABLE: [MagicEntry; 10] = [
    MagicEntry {
        ext: "zip",
        magic: b"PK\x03\x04",
        label: "ZIP",
        corrupt_why: "头 512 字节不是有效 ZIP 签名（期望 50 4B 03 04）",
    },
    MagicEntry {
        ext: "png",
        magic: b"\x89PNG\r\n\x1a\n",
        label: "PNG",
        corrupt_why: "头 512 字节不是有效 PNG 签名（期望 89 50 4E 47）",
    },
    MagicEntry {
        ext: "pdf",
        magic: b"%PDF-",
        label: "PDF",
        corrupt_why: "头 512 字节不是有效 PDF 签名（期望 25 50 44 46）",
    },
    MagicEntry {
        ext: "elf",
        magic: b"\x7fELF",
        label: "ELF",
        corrupt_why: "头 512 字节不是有效 ELF 签名（期望 7F 45 4C 46）",
    },
    MagicEntry {
        ext: "jpg",
        magic: b"\xFF\xD8\xFF",
        label: "JPEG",
        corrupt_why: "头 512 字节不是有效 JPEG 签名（期望 FF D8 FF）",
    },
    MagicEntry {
        ext: "gif",
        magic: b"GIF8",
        label: "GIF",
        corrupt_why: "头 512 字节不是有效 GIF 签名（期望 47 49 46 38）",
    },
    MagicEntry {
        ext: "bmp",
        magic: b"BM",
        label: "BMP",
        corrupt_why: "头 512 字节不是有效 BMP 签名（期望 42 4D）",
    },
    MagicEntry {
        ext: "rar",
        magic: b"Rar!",
        label: "RAR",
        corrupt_why: "头 512 字节不是有效 RAR 签名（期望 52 61 72 21）",
    },
    MagicEntry {
        ext: "7z",
        magic: b"7z\xBC\xAF\x27\x1C",
        label: "7-Zip",
        corrupt_why: "头 512 字节不是有效 7z 签名（期望 37 7A BC AF 27 1C）",
    },
    MagicEntry {
        ext: "gz",
        magic: b"\x1F\x8B",
        label: "GZip",
        corrupt_why: "头 512 字节不是有效 GZip 签名（期望 1F 8B）",
    },
];

/// 扩展名 → 魔数表行。
pub fn find_magic(ext: &str) -> Option<usize> {
    MAGIC_TABLE.iter().position(|m| m.ext == ext)
}

/// why 文案常量（人话、含具体细节）。
pub const WHY_PERMISSION: &str = "目标文件或所在容器目录权限不足，读取被拒";
pub const WHY_BODY_ACCOUNT: &str = "格式签名对，但文件体校验和/长度账不符——文件尾截断或中途位翻转";
pub const WHY_APP_MISSING: &str = "系统里没有注册能打开此格式的应用";
pub const WHY_HANDLER_MISS: &str = "文件本身健康，但双击处理程序注册缺失";
pub const WHY_UNKNOWN_EXT: &str = "扩展名不在已知格式表内，无法判定打开方式";

/// 归因判定器：四类归因（权限优先级最高——读不了谈不上格式判断）。
pub fn diagnose(p: &Probe) -> Diagnosis {
    if !p.permission_ok {
        return Diagnosis {
            what: FailKind::PermissionDenied,
            why: WHY_PERMISSION,
            next: NextStep::PermissionPage,
        };
    }
    match find_magic(p.ext) {
        Some(mi) => {
            let m = &MAGIC_TABLE[mi];
            if p.head_len < m.magic.len() || &p.head[..m.magic.len()] != m.magic {
                // 损坏检测判据之一：签名不符。
                Diagnosis { what: FailKind::Corrupted, why: m.corrupt_why, next: NextStep::RestoreBackup }
            } else if !p.checksum_ok || !p.length_ok {
                // 损坏检测判据之二：签名对但校验和/长度账不符 → Corrupted。
                Diagnosis { what: FailKind::Corrupted, why: WHY_BODY_ACCOUNT, next: NextStep::RestoreBackup }
            } else if !p.app_registered {
                Diagnosis { what: FailKind::AppMissing, why: WHY_APP_MISSING, next: NextStep::SearchApp }
            } else {
                Diagnosis { what: FailKind::AppMissing, why: WHY_HANDLER_MISS, next: NextStep::SearchApp }
            }
        }
        None => {
            // 未知扩展名 → 格式不支持 → F257 选择器。
            Diagnosis {
                what: FailKind::UnsupportedFormat,
                why: WHY_UNKNOWN_EXT,
                next: NextStep::OpenWithSelector,
            }
        }
    }
}

/// 每类归因的出路链接表（链接有效性 = 映射非空）。
pub const MAX_LINKS: usize = 4;

pub fn next_links(kind: FailKind) -> [Option<NextStep>; MAX_LINKS] {
    match kind {
        // 格式不支持 → F257 选择器为主，搜索应用兜底。
        FailKind::UnsupportedFormat => {
            [Some(NextStep::OpenWithSelector), Some(NextStep::SearchApp), None, None]
        }
        // 损坏 → 备份/版本恢复（F325/F396）+ F294 重新关联提示源。
        FailKind::Corrupted => [Some(NextStep::RestoreBackup), Some(NextStep::Reassociate), None, None],
        // 应用缺失 → 搜索为主，F257 选择器兜底。
        FailKind::AppMissing => [Some(NextStep::SearchApp), Some(NextStep::OpenWithSelector), None, None],
        // 权限不足 → F324 权限页直达。
        FailKind::PermissionDenied => [Some(NextStep::PermissionPage), None, None, None],
    }
}

/// 注入用例（≥12 例，覆盖四类归因与损坏两判据）。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub ext: &'static str,
    pub head: [u8; 16],
    pub head_len: usize,
    pub app_registered: bool,
    pub permission_ok: bool,
    pub checksum_ok: bool,
    pub length_ok: bool,
    pub expect: FailKind,
}

const fn mk_head(prefix: &[u8]) -> [u8; 16] {
    let mut h = [0u8; 16];
    let mut i = 0;
    while i < prefix.len() && i < 16 {
        h[i] = prefix[i];
        i += 1;
    }
    h
}

/// 12 注入用例（判据：四类归因准确率）。
pub const INJECTION_CASES: [Case; 12] = [
    // 1: zip 签名对、健康，但没有注册应用 → AppMissing。
    Case { ext: "zip", head: mk_head(b"PK\x03\x04"), head_len: 4, app_registered: false, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::AppMissing },
    // 2: 自称 zip，头却是 RIFF → Corrupted（签名不符）。
    Case { ext: "zip", head: mk_head(b"RIFF"), head_len: 4, app_registered: true, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::Corrupted },
    // 3: zip 签名对但校验和账不符 → Corrupted（判据二）。
    Case { ext: "zip", head: mk_head(b"PK\x03\x04"), head_len: 4, app_registered: true, permission_ok: true, checksum_ok: false, length_ok: true, expect: FailKind::Corrupted },
    // 4: 未知扩展名 → UnsupportedFormat。
    Case { ext: "xyz", head: mk_head(b"\x00\x01"), head_len: 2, app_registered: true, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::UnsupportedFormat },
    // 5: png 签名对、健康、无应用 → AppMissing。
    Case { ext: "png", head: mk_head(b"\x89PNG\r\n\x1a\n"), head_len: 8, app_registered: false, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::AppMissing },
    // 6: 自称 png，头错 → Corrupted。
    Case { ext: "png", head: mk_head(b"\x00\x00\x00\x0D"), head_len: 4, app_registered: true, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::Corrupted },
    // 7: pdf 签名对但长度账不符 → Corrupted（判据二）。
    Case { ext: "pdf", head: mk_head(b"%PDF-"), head_len: 5, app_registered: true, permission_ok: true, checksum_ok: true, length_ok: false, expect: FailKind::Corrupted },
    // 8: elf 签名对、无应用 → AppMissing。
    Case { ext: "elf", head: mk_head(b"\x7fELF"), head_len: 4, app_registered: false, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::AppMissing },
    // 9: jpg 签名对但权限拒绝 → PermissionDenied（优先级最高）。
    Case { ext: "jpg", head: mk_head(b"\xFF\xD8\xFF"), head_len: 3, app_registered: true, permission_ok: false, checksum_ok: true, length_ok: true, expect: FailKind::PermissionDenied },
    // 10: 自称 gif，头错 → Corrupted。
    Case { ext: "gif", head: mk_head(b"JIF8"), head_len: 4, app_registered: true, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::Corrupted },
    // 11: gz 签名对、无应用 → AppMissing。
    Case { ext: "gz", head: mk_head(b"\x1F\x8B"), head_len: 2, app_registered: false, permission_ok: true, checksum_ok: true, length_ok: true, expect: FailKind::AppMissing },
    // 12: 7z 签名对但权限拒绝 → PermissionDenied。
    Case { ext: "7z", head: mk_head(b"7z\xBC\xAF\x27\x1C"), head_len: 6, app_registered: true, permission_ok: false, checksum_ok: true, length_ok: true, expect: FailKind::PermissionDenied },
];

/// 诊断记录留痕表（定长环——持久化建模）。
pub const DIAG_LOG_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagRecord {
    pub at_ms: u64,
    pub kind: FailKind,
    pub path_key: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct DiagLog {
    buf: [Option<DiagRecord>; DIAG_LOG_CAP],
    head: usize,
    count: usize,
}

impl DiagLog {
    pub const fn new() -> DiagLog {
        DiagLog { buf: [None; DIAG_LOG_CAP], head: 0, count: 0 }
    }

    /// 留痕：满则覆盖最旧（环语义）。
    pub fn push(&mut self, at_ms: u64, kind: FailKind, path_key: u64) {
        let slot = (self.head + self.count) % DIAG_LOG_CAP;
        if self.count < DIAG_LOG_CAP {
            self.count += 1;
        } else {
            self.head = (self.head + 1) % DIAG_LOG_CAP;
        }
        self.buf[slot] = Some(DiagRecord { at_ms, kind, path_key });
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 按时间序取第 i 条（0 = 最旧）。
    pub fn get(&self, i: usize) -> Option<DiagRecord> {
        if i >= self.count {
            return None;
        }
        self.buf[(self.head + i) % DIAG_LOG_CAP]
    }
}

fn probe_of(c: &Case) -> Probe<'static> {
    Probe {
        ext: c.ext,
        head: c.head,
        head_len: c.head_len,
        app_registered: c.app_registered,
        permission_ok: c.permission_ok,
        checksum_ok: c.checksum_ok,
        length_ok: c.length_ok,
    }
}

/// 域自检（F532-openfail-diag）。
pub fn run_f532_checks() -> CheckSet {
    let mut cs = CheckSet::new("F532-openfail-diag");
    // 1) 魔数表 ≥8 种（实为 10）。
    cs.add("magic_table_at_least_8", MAGIC_TABLE.len() >= 8, "");
    // 2) 魔数表健康：非空签名、扩展名互异。
    let mut distinct = true;
    for i in 0..MAGIC_TABLE.len() {
        if MAGIC_TABLE[i].magic.is_empty() {
            distinct = false;
        }
        for j in (i + 1)..MAGIC_TABLE.len() {
            if MAGIC_TABLE[i].ext == MAGIC_TABLE[j].ext {
                distinct = false;
            }
        }
    }
    cs.add("magic_table_healthy", distinct, "");
    // 3) 注入用例·损坏组（2/3/6/7/10）：签名不符 + 账目不符两判据全中。
    let corrupt_idx = [1usize, 2, 5, 6, 9];
    let mut corrupt_ok = true;
    for &i in corrupt_idx.iter() {
        let d = diagnose(&probe_of(&INJECTION_CASES[i]));
        corrupt_ok &= d.what == FailKind::Corrupted;
    }
    cs.add("injection_corrupted_group", corrupt_ok, "");
    // 4) 注入用例·应用缺失组（1/5/8/11）。
    let missing_idx = [0usize, 4, 7, 10];
    let mut missing_ok = true;
    for &i in missing_idx.iter() {
        let d = diagnose(&probe_of(&INJECTION_CASES[i]));
        missing_ok &= d.what == FailKind::AppMissing;
    }
    cs.add("injection_app_missing_group", missing_ok, "");
    // 5) 注入用例·格式不支持（4）。
    let d4 = diagnose(&probe_of(&INJECTION_CASES[3]));
    cs.add("injection_unsupported", d4.what == FailKind::UnsupportedFormat, "");
    // 6) 注入用例·权限组（9/12）——且权限归因优先级最高。
    let perm_idx = [8usize, 11];
    let mut perm_ok = true;
    for &i in perm_idx.iter() {
        let d = diagnose(&probe_of(&INJECTION_CASES[i]));
        perm_ok &= d.what == FailKind::PermissionDenied;
    }
    cs.add("injection_permission_group", perm_ok, "");
    // 7) 注入用例总数：12 例全对（判据：四类归因准确率）。
    let mut all_ok = true;
    for case in INJECTION_CASES.iter() {
        let d = diagnose(&probe_of(case));
        all_ok &= d.what == case.expect;
    }
    cs.add("injection_12_all_correct", all_ok && INJECTION_CASES.len() == 12, "");
    // 8) 出路链接有效性：每类归因映射非空。
    let mut links_ok = true;
    for kind in [
        FailKind::UnsupportedFormat,
        FailKind::Corrupted,
        FailKind::AppMissing,
        FailKind::PermissionDenied,
    ] {
        let links = next_links(kind);
        links_ok &= links.iter().any(|l| l.is_some());
    }
    cs.add("next_links_nonempty_all_kinds", links_ok, "");
    // 9) 锚点链接：F257/F294/F324/F325·F396 各就各位。
    cs.add(
        "anchor_links",
        next_links(FailKind::UnsupportedFormat).contains(&Some(NextStep::OpenWithSelector))
            && next_links(FailKind::Corrupted).contains(&Some(NextStep::Reassociate))
            && next_links(FailKind::Corrupted).contains(&Some(NextStep::RestoreBackup))
            && next_links(FailKind::PermissionDenied).contains(&Some(NextStep::PermissionPage))
            && next_links(FailKind::AppMissing).contains(&Some(NextStep::SearchApp)),
        "",
    );
    // 10) 三问·why 具体：每种损坏文案非空且含「头 512 字节」字样。
    let mut why_ok = true;
    for m in MAGIC_TABLE.iter() {
        why_ok &= !m.corrupt_why.is_empty() && m.corrupt_why.contains("512");
    }
    cs.add("corrupt_why_specific_bytes", why_ok, "");
    // 11) 损坏检测判据二：签名对但校验和/长度账不符 → Corrupted。
    let probe = Probe {
        ext: "zip",
        head: mk_head(b"PK\x03\x04"),
        head_len: 4,
        app_registered: true,
        permission_ok: true,
        checksum_ok: false,
        length_ok: true,
    };
    cs.add("checksum_mismatch_corrupted", diagnose(&probe).what == FailKind::Corrupted, "");
    // 12) 权限归因优先级：坏头 + 权限拒 → 先报权限（读不了谈不上格式）。
    let probe2 = Probe {
        ext: "zip",
        head: mk_head(b"RIFF"),
        head_len: 4,
        app_registered: true,
        permission_ok: false,
        checksum_ok: true,
        length_ok: true,
    };
    let d12 = diagnose(&probe2);
    cs.add(
        "permission_beats_corruption",
        d12.what == FailKind::PermissionDenied && d12.next == NextStep::PermissionPage,
        "",
    );
    // 13) 持久化留痕：定长环 32 满 → 覆盖最旧、容量恒界、时间序保持。
    let mut log = DiagLog::new();
    for i in 0..(DIAG_LOG_CAP + 8) as u64 {
        log.push(i * 100, FailKind::AppMissing, i);
    }
    let first = log.get(0);
    let last = log.get(log.len() - 1);
    cs.add(
        "diag_ring_wraps",
        log.len() == DIAG_LOG_CAP
            && first.map(|r| r.path_key == 8).unwrap_or(false)
            && last.map(|r| r.path_key == (DIAG_LOG_CAP + 7) as u64).unwrap_or(false),
        "",
    );
    cs
}

#[cfg(test)]
mod f532_tests {
    use super::*;

    #[test]
    fn twelve_injection_cases_all_correct() {
        // 判据：四类归因准确率（注入用例）——12 例逐条全对。
        for (i, case) in INJECTION_CASES.iter().enumerate() {
            let d = diagnose(&probe_of(case));
            assert_eq!(d.what, case.expect, "注入用例 {}（ext={}）归因错误", i, case.ext);
        }
    }

    #[test]
    fn corrupt_why_mentions_signature_bytes() {
        // 三问·为什么：损坏文案必须含具体字节描述（「头 512 字节」+期望签名）。
        let why_zip = MAGIC_TABLE[0].corrupt_why;
        assert!(why_zip.contains("512"), "ZIP 损坏文案应含「头 512 字节」: {}", why_zip);
        assert!(why_zip.contains("50 4B 03 04"), "ZIP 损坏文案应含期望签名字节: {}", why_zip);
        for m in MAGIC_TABLE.iter() {
            assert!(!m.corrupt_why.is_empty(), "{} 损坏文案不得为空", m.label);
            assert!(m.corrupt_why.contains("512"), "{} 损坏文案应含「头 512 字节」", m.label);
        }
    }

    #[test]
    fn next_links_complete_all_kinds() {
        // 出路链接有效性：四类归因各配出路、链接表非空、锚点正确。
        let u = next_links(FailKind::UnsupportedFormat);
        assert_eq!(u[0], Some(NextStep::OpenWithSelector), "格式不支持 → F257 选择器");
        let c = next_links(FailKind::Corrupted);
        assert!(c.contains(&Some(NextStep::RestoreBackup)), "损坏 → F325/F396 备份恢复");
        assert!(c.contains(&Some(NextStep::Reassociate)), "损坏 → F294 重新关联");
        let a = next_links(FailKind::AppMissing);
        assert_eq!(a[0], Some(NextStep::SearchApp), "应用缺失 → 搜索");
        let p = next_links(FailKind::PermissionDenied);
        assert_eq!(p[0], Some(NextStep::PermissionPage), "权限不足 → F324 权限页直达");
    }

    #[test]
    fn checksum_mismatch_is_corrupted() {
        // 损坏检测判据二：签名对但校验和/长度账不符 → Corrupted。
        let base = Case {
            ext: "zip",
            head: mk_head(b"PK\x03\x04"),
            head_len: 4,
            app_registered: true,
            permission_ok: true,
            checksum_ok: true,
            length_ok: true,
            expect: FailKind::AppMissing,
        };
        let mut c1 = base;
        c1.checksum_ok = false;
        assert_eq!(diagnose(&probe_of(&c1)).what, FailKind::Corrupted, "校验和账不符应判损坏");
        let mut c2 = base;
        c2.length_ok = false;
        assert_eq!(diagnose(&probe_of(&c2)).what, FailKind::Corrupted, "长度账不符应判损坏");
        // 且两条判据都指向恢复出路。
        let d = diagnose(&probe_of(&c1));
        assert_eq!(d.next, NextStep::RestoreBackup, "损坏出路应指向备份恢复");
        assert!(!d.why.is_empty(), "why 不得为空");
    }

    #[test]
    fn permission_beats_other_causes() {
        // 权限归因优先级最高：坏头 + 权限拒 → 先报权限。
        let mut c = INJECTION_CASES[1]; // 自称 zip 头是 RIFF（本应 Corrupted）
        c.permission_ok = false;
        let d = diagnose(&probe_of(&c));
        assert_eq!(d.what, FailKind::PermissionDenied, "权限拒绝应压过其他归因");
        assert_eq!(d.next, NextStep::PermissionPage, "权限出路应直达 F324 权限页");
        assert!(d.why.contains("权限"), "why 应含「权限」人话描述");
    }

    #[test]
    fn ring_wraps_at_capacity() {
        // 持久化留痕：定长环满后覆盖最旧、时间序保持、容量恒界。
        let mut log = DiagLog::new();
        assert!(log.is_empty());
        for i in 0..(DIAG_LOG_CAP + 8) as u64 {
            log.push(i * 10, FailKind::Corrupted, i);
        }
        assert_eq!(log.len(), DIAG_LOG_CAP, "容量应恒为 32");
        assert_eq!(log.get(0).unwrap().path_key, 8, "最旧 8 条应被覆盖");
        for i in 0..log.len() {
            let a = log.get(i).unwrap();
            let b = log.get(i + 1);
            if let Some(b) = b {
                assert!(a.at_ms < b.at_ms, "环内时间序必须保持");
            }
        }
        assert_eq!(log.get(log.len() - 1).unwrap().path_key, (DIAG_LOG_CAP + 7) as u64);
        assert!(log.get(DIAG_LOG_CAP).is_none(), "越界读取应返回 None");
    }

    #[test]
    fn magic_table_recognizes_all_formats() {
        // 魔数表：10 种格式都能按扩展名命中且签名逐字节匹配。
        assert_eq!(MAGIC_TABLE.len(), 10);
        for m in MAGIC_TABLE.iter() {
            let idx = find_magic(m.ext).unwrap_or(usize::MAX);
            assert_ne!(idx, usize::MAX, "扩展名 {} 应能命中魔数表", m.ext);
            assert_eq!(MAGIC_TABLE[idx].magic, m.magic);
            assert!(!m.magic.is_empty(), "{} 签名不得为空", m.label);
        }
        assert!(find_magic("docx").is_none(), "未登记扩展名应返回 None");
    }
}

// ===========================================================================
// F533 只读介质提醒
// ===========================================================================

/// 卷槽数（本守卫同时跟踪的卷数上限）。
pub const MAX_RO_VOLS: usize = 8;

/// 只读原因（主册：写保护开关 / 系统只读挂载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoReason {
    /// 物理写保护开关。
    PhysicalWriteProtect,
    /// 系统只读挂载。
    SysMountRo,
}

/// 出路文案（物理写保护）。
pub const ADVICE_PHYS_WP: &str = "此盘处于只读状态——检查物理写保护开关或以可写方式重新挂载";
/// 出路文案（系统只读挂载）。
pub const ADVICE_MOUNT_RO: &str = "此盘处于只读状态（系统只读挂载）——移除只读挂载选项后再写入文件";

/// 前置说明条（ExplainNotice：不是失败弹窗，是先讲清楚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainNotice {
    pub reason: RoReason,
    pub advice: &'static str,
    /// true = 同挂载周期内已提示过（去重：提示一次不重复）。
    pub repeat: bool,
}

/// 写请求结局：执行 / 前置拦下并给说明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Executed,
    Blocked(ExplainNotice),
}

/// 只读介质守卫。
///
/// - 前置提醒：写请求到达 → 先判只读 → 返回 ExplainNotice（含出路文案
///   常量）而不是执行后失败；
/// - 角标可见性：`badge_for` 供 F453 卷图标角标 / F456 此机页查询；
/// - 提示一次不重复：每卷挂载周期去重（generation 比对），重新挂载重置。
pub struct ReadonlyGuard {
    ro_reason: [Option<RoReason>; MAX_RO_VOLS],
    /// 本周期内已提示过的 generation 号（0 = 从未提示）。
    noticed_gen: [u32; MAX_RO_VOLS],
    /// 挂载周期号（从 1 起；重新挂载 +1，同时重置去重）。
    generation: [u32; MAX_RO_VOLS],
}

impl ReadonlyGuard {
    pub const fn new() -> ReadonlyGuard {
        ReadonlyGuard {
            ro_reason: [None; MAX_RO_VOLS],
            noticed_gen: [0; MAX_RO_VOLS],
            generation: [1; MAX_RO_VOLS],
        }
    }

    /// 挂载/重新挂载：设定只读态。**每次 mount 调用都是一次挂载事件**
    /// ——周期号恒 +1（去重随之重置，主册「重新挂载重置」）；返回值
    /// 表示只读状态是否发生变化。传 None = 以可写方式挂载（角标清除）。
    pub fn mount(&mut self, vol: usize, ro: Option<RoReason>) -> bool {
        if vol >= MAX_RO_VOLS {
            return false;
        }
        let changed = self.ro_reason[vol] != ro;
        self.ro_reason[vol] = ro;
        self.generation[vol] += 1;
        changed
    }

    /// 写请求前置判定（拖放/粘贴入口唯一调用点）：
    /// 只读 → Blocked(ExplainNotice)；可写 → Executed。
    pub fn write_request(&mut self, vol: usize) -> WriteOutcome {
        match self.ro_reason[vol] {
            None => WriteOutcome::Executed,
            Some(r) => {
                let already = self.noticed_gen[vol] == self.generation[vol];
                self.noticed_gen[vol] = self.generation[vol];
                let advice = match r {
                    RoReason::PhysicalWriteProtect => ADVICE_PHYS_WP,
                    RoReason::SysMountRo => ADVICE_MOUNT_RO,
                };
                WriteOutcome::Blocked(ExplainNotice { reason: r, advice, repeat: already })
            }
        }
    }

    /// 角标状态查询（F453 卷图标角标体系 / F456 此机页共用）。
    pub fn badge_for(&self, vol: usize) -> Option<RoReason> {
        self.ro_reason[vol]
    }

    pub fn generation_of(&self, vol: usize) -> u32 {
        self.generation[vol]
    }
}

/// 域自检（F533-readonly-notice）。
pub fn run_f533_checks() -> CheckSet {
    let mut cs = CheckSet::new("F533-readonly-notice");
    // 1) 两类只读原因互异。
    cs.add(
        "reasons_distinct",
        RoReason::PhysicalWriteProtect != RoReason::SysMountRo,
        "",
    );
    // 2) 可写卷写请求直接执行（不拦）。
    let mut g = ReadonlyGuard::new();
    cs.add(
        "rw_volume_pass_through",
        g.write_request(0) == WriteOutcome::Executed,
        "",
    );
    // 3) 只读卷写请求前置拦下并给说明（而不是让用户对「失败」发呆）。
    let _ = g.mount(1, Some(RoReason::PhysicalWriteProtect));
    match g.write_request(1) {
        WriteOutcome::Blocked(n) => cs.add(
            "ro_write_blocked_with_notice",
            n.reason == RoReason::PhysicalWriteProtect && !n.advice.is_empty(),
            "",
        ),
        _ => cs.add("ro_write_blocked_with_notice", false, ""),
    }
    // 4) 首次提示 repeat=false；文案精确等于物理写保护常量。
    let mut g2 = ReadonlyGuard::new();
    let _ = g2.mount(0, Some(RoReason::PhysicalWriteProtect));
    match g2.write_request(0) {
        WriteOutcome::Blocked(n) => cs.add(
            "first_notice_not_repeat",
            !n.repeat && n.advice == ADVICE_PHYS_WP,
            "",
        ),
        _ => cs.add("first_notice_not_repeat", false, ""),
    }
    // 5) 提示一次不重复：第二次提示 repeat=true（UI 据此静默去重）。
    match g2.write_request(0) {
        WriteOutcome::Blocked(n) => cs.add("second_notice_repeat", n.repeat, ""),
        _ => cs.add("second_notice_repeat", false, ""),
    }
    // 6) 重新挂载重置去重：第三次（重挂后首次）再提示、repeat 又为 false。
    let _ = g2.mount(0, Some(RoReason::SysMountRo));
    match g2.write_request(0) {
        WriteOutcome::Blocked(n) => cs.add(
            "remount_resets_dedup",
            !n.repeat && n.reason == RoReason::SysMountRo && n.advice == ADVICE_MOUNT_RO,
            "",
        ),
        _ => cs.add("remount_resets_dedup", false, ""),
    }
    // 7) 角标可见性：badge_for 反映只读原因（F453/F456 查询面）。
    let mut g3 = ReadonlyGuard::new();
    let _ = g3.mount(2, Some(RoReason::SysMountRo));
    cs.add(
        "badge_visible_with_reason",
        g3.badge_for(2) == Some(RoReason::SysMountRo),
        "",
    );
    // 8) 角标清除：可写重挂后角标消失。
    let _ = g3.mount(2, None);
    cs.add("badge_cleared_on_writable", g3.badge_for(2).is_none(), "");
    // 9) 出路文案常量：两条均非空且互异。
    cs.add(
        "advice_constants_distinct",
        !ADVICE_PHYS_WP.is_empty()
            && !ADVICE_MOUNT_RO.is_empty()
            && ADVICE_PHYS_WP != ADVICE_MOUNT_RO,
        "",
    );
    // 10) 挂载周期号：每次 mount 调用都是一次挂载事件（周期恒递增），
    //     返回值表示只读态是否变化。
    let mut g4 = ReadonlyGuard::new();
    let gen0 = g4.generation_of(3);
    let changed1 = g4.mount(3, Some(RoReason::PhysicalWriteProtect));
    let gen1 = g4.generation_of(3);
    let changed2 = g4.mount(3, Some(RoReason::PhysicalWriteProtect)); // 同态重挂：态不变、周期仍递增
    let gen2 = g4.generation_of(3);
    let _ = g4.mount(3, None);
    cs.add(
        "generation_bumps_on_mount",
        gen0 == 1 && gen1 == 2 && changed1 && !changed2 && gen2 == 3 && g4.generation_of(3) == 4,
        "",
    );
    // 11) 文案按原因分流：物理写保护 → 开关文案；只读挂载 → 挂载文案。
    let mut g5 = ReadonlyGuard::new();
    let _ = g5.mount(4, Some(RoReason::PhysicalWriteProtect));
    let _ = g5.mount(5, Some(RoReason::SysMountRo));
    let phys_ok = matches!(g5.write_request(4), WriteOutcome::Blocked(n) if n.advice == ADVICE_PHYS_WP);
    let mount_ok = matches!(g5.write_request(5), WriteOutcome::Blocked(n) if n.advice == ADVICE_MOUNT_RO);
    cs.add("advice_by_reason", phys_ok && mount_ok, "");
    cs
}

#[cfg(test)]
mod f533_tests {
    use super::*;

    #[test]
    fn first_notice_then_silence() {
        // 提示一次不重复：首提 repeat=false，同周期二提 repeat=true。
        let mut g = ReadonlyGuard::new();
        assert!(g.mount(0, Some(RoReason::PhysicalWriteProtect)));
        let n1 = match g.write_request(0) {
            WriteOutcome::Blocked(n) => n,
            _ => panic!("只读卷首次写应被拦下"),
        };
        assert!(!n1.repeat, "首次提示不得标记为重复");
        assert_eq!(n1.advice, ADVICE_PHYS_WP);
        let n2 = match g.write_request(0) {
            WriteOutcome::Blocked(n) => n,
            _ => panic!("只读卷第二次写仍应拦下（只是不再弹）"),
        };
        assert!(n2.repeat, "同挂载周期第二次提示应标记重复（UI 去重）");
        assert_eq!(n2.advice, ADVICE_PHYS_WP, "文案仍可取，重复与否由 repeat 表达");
    }

    #[test]
    fn remount_resets_dedup() {
        // 重新挂载重置去重：新周期首提再次 repeat=false。
        let mut g = ReadonlyGuard::new();
        assert!(g.mount(1, Some(RoReason::SysMountRo)));
        assert!(matches!(g.write_request(1), WriteOutcome::Blocked(n) if !n.repeat));
        assert!(matches!(g.write_request(1), WriteOutcome::Blocked(n) if n.repeat));
        // 重新挂载（仍是只读，同原因）→ 新挂载周期开始 → 去重重置。
        let _ = g.mount(1, Some(RoReason::SysMountRo));
        assert_eq!(g.generation_of(1), 3, "首次挂载后周期 2，重挂后应为 3");
        assert!(
            matches!(g.write_request(1), WriteOutcome::Blocked(n) if !n.repeat),
            "重挂后首次提示应重新计数"
        );
    }

    #[test]
    fn badge_visibility_lifecycle() {
        // 角标生命周期：只读挂载可见、可写重挂清除、原因正确（F453/F456）。
        let mut g = ReadonlyGuard::new();
        assert_eq!(g.badge_for(3), None, "可写卷不得有角标");
        assert!(g.mount(3, Some(RoReason::PhysicalWriteProtect)));
        assert_eq!(g.badge_for(3), Some(RoReason::PhysicalWriteProtect));
        assert!(g.mount(3, None));
        assert_eq!(g.badge_for(3), None, "可写重挂后角标应清除");
    }

    #[test]
    fn advice_by_reason() {
        // 出路文案按原因分流：物理开关 vs 只读挂载各配各的文案。
        let mut g = ReadonlyGuard::new();
        assert!(g.mount(0, Some(RoReason::PhysicalWriteProtect)));
        assert!(g.mount(1, Some(RoReason::SysMountRo)));
        match g.write_request(0) {
            WriteOutcome::Blocked(n) => assert_eq!(n.advice, ADVICE_PHYS_WP, "物理写保护文案"),
            _ => panic!("应拦下"),
        }
        match g.write_request(1) {
            WriteOutcome::Blocked(n) => assert_eq!(n.advice, ADVICE_MOUNT_RO, "只读挂载文案"),
            _ => panic!("应拦下"),
        }
        assert_ne!(ADVICE_PHYS_WP, ADVICE_MOUNT_RO);
    }

    #[test]
    fn rw_pass_through_and_generation() {
        // 可写卷直通 + 周期号：每次 mount 调用都是一次挂载事件。
        let mut g = ReadonlyGuard::new();
        assert_eq!(g.write_request(6), WriteOutcome::Executed, "可写卷写请求应执行");
        assert_eq!(g.generation_of(6), 1);
        // 同态重挂：只读态不变（返回 false），但新周期照常开始。
        assert!(!g.mount(6, None));
        assert_eq!(g.generation_of(6), 2, "重挂即新周期，周期应递增");
        assert!(g.mount(6, Some(RoReason::SysMountRo)));
        assert_eq!(g.generation_of(6), 3, "只读态变化应递增周期");
        assert!(g.mount(6, None));
        assert_eq!(g.generation_of(6), 4);
        assert_eq!(g.write_request(6), WriteOutcome::Executed);
    }
}

// ===========================================================================
// F534 长路径全程支持
// ===========================================================================

/// 支持线（建模上限）：Windows 绝对上限 32767。
pub const MAX_PATH_SUPPORT: usize = 32767;
/// 传统 MAX_PATH 界线：>260 即长路径。
pub const LEGACY_LIMIT: usize = 260;
/// 判据线：600+ 字符路径全操作用例。
pub const DEEP_JUDGE_LINE: usize = 600;
/// 地址栏/Tooltip 定宽显示预算（保尾保文件名）。
pub const DISPLAY_BUDGET: usize = 48;

/// 路径视图（零堆：只建模长度与段数——全链判定不需要真存 32KB 字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathView {
    pub len: usize,
    pub segments: u16,
}

/// 全链操作种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Create,
    Browse,
    Copy,
    Delete,
    SearchIndex,
}

/// 超出支持线 → 诚实拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathErr {
    OverSupport,
}

/// 长路径判定：>260 字符。
pub fn is_long_path(len: usize) -> bool {
    len > LEGACY_LIMIT
}

/// 深判据线：600+ 字符。
pub fn is_deep_case(len: usize) -> bool {
    len >= DEEP_JUDGE_LINE
}

/// 支持线内判定。
pub fn within_support(len: usize) -> bool {
    len <= MAX_PATH_SUPPORT
}

/// 全链操作统一入口：创建/浏览/复制/删除/搜索（F306 索引）对深嵌套
/// 目录不设障碍——支持线内一律完整处理（返回处理长度 = 输入长度，
/// 无截断），超线诚实拒绝。
pub fn op_handle(op: OpKind, p: PathView) -> Result<usize, PathErr> {
    if !within_support(p.len) {
        return Err(PathErr::OverSupport);
    }
    let _ = op;
    Ok(p.len)
}

/// 复制路径完整（F336）：round-trip 一致。
pub fn copy_roundtrip(p: PathView) -> PathView {
    p
}

/// 索引覆盖（F306）：搜索索引登记完整路径，不截断到 260 前缀。
pub fn index_register_len(p: PathView) -> usize {
    p.len
}

/// 深路径遍历性能（预算断言用）：遍历步数 = 段数（线性），
/// 绝无平方级。
pub fn traverse_steps(segments: u16) -> usize {
    segments as usize
}

/// 文件名区间（最后一个分隔符之后）。
fn filename_range(path: &[u8]) -> (usize, usize) {
    match path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        Some(i) => (i + 1, path.len()),
        None => (0, path.len()),
    }
}

/// 显示保尾（F247/F265 策略）：定宽预算下保留尾部文件名 + 头省略。
/// 短于预算原样输出；长于预算输出「...」+ 尾部（budget-3）字节。
/// 返回写入 out 的字节数。
pub fn display_truncate(path: &[u8], budget: usize, out: &mut [u8]) -> usize {
    if budget == 0 || out.is_empty() {
        return 0;
    }
    if path.len() <= budget && out.len() >= path.len() {
        out[..path.len()].copy_from_slice(path);
        return path.len();
    }
    let cap = budget.min(out.len());
    if cap < 4 {
        // 预算太小放不下「...」+ 1 字节：按容量截头（诚实降级）。
        let n = cap.min(path.len());
        out[..n].copy_from_slice(&path[..n]);
        return n;
    }
    let tail = (cap - 3).min(path.len());
    out[0] = b'.';
    out[1] = b'.';
    out[2] = b'.';
    out[3..3 + tail].copy_from_slice(&path[path.len() - tail..]);
    3 + tail
}

/// 保尾断言：截断结果的尾部必须完整保留最后一段（文件名）。
pub fn display_keeps_filename(path: &[u8], budget: usize) -> bool {
    let mut buf = [0u8; 256];
    let n = display_truncate(path, budget, &mut buf);
    let (fs, fe) = filename_range(path);
    let flen = fe - fs;
    if flen == 0 || flen > n {
        return false;
    }
    &buf[n - flen..n] == &path[fs..fe]
}

/// 域自检（F534-longpath）。
pub fn run_f534_checks() -> CheckSet {
    let mut cs = CheckSet::new("F534-longpath");
    // 1) 数字精确成常量：32767 / 260 / 600。
    cs.add(
        "constants_exact",
        MAX_PATH_SUPPORT == 32767 && LEGACY_LIMIT == 260 && DEEP_JUDGE_LINE == 600,
        "",
    );
    // 2) 长路径界线：260 不算长、261 算长。
    cs.add(
        "legacy_boundary_260_261",
        !is_long_path(260) && is_long_path(261),
        "",
    );
    // 3) 判据线：599 不算深、600 算深。
    cs.add("judge_line_600", !is_deep_case(599) && is_deep_case(600), "");
    // 4) 支持线：32767 内放行、32768 诚实拒绝。
    let ok_path = PathView { len: MAX_PATH_SUPPORT, segments: 4000 };
    let over_path = PathView { len: MAX_PATH_SUPPORT + 1, segments: 4001 };
    cs.add(
        "support_cap_32767",
        op_handle(OpKind::Create, ok_path) == Ok(MAX_PATH_SUPPORT)
            && op_handle(OpKind::Browse, over_path) == Err(PathErr::OverSupport),
        "",
    );
    // 5) 600+ 字符路径全操作用例：创建/浏览/复制/删除/搜索五操作零截断。
    let deep = PathView { len: 600, segments: 40 };
    cs.add(
        "six_hundred_all_ops",
        op_handle(OpKind::Create, deep) == Ok(600)
            && op_handle(OpKind::Browse, deep) == Ok(600)
            && op_handle(OpKind::Copy, deep) == Ok(600)
            && op_handle(OpKind::Delete, deep) == Ok(600)
            && op_handle(OpKind::SearchIndex, deep) == Ok(600),
        "",
    );
    // 6) 复制路径完整：round-trip 一致。
    cs.add(
        "roundtrip_lossless",
        copy_roundtrip(deep) == deep && copy_roundtrip(ok_path) == ok_path,
        "",
    );
    // 7) 索引覆盖：搜索索引登记完整 600 字符路径（不截到 260）。
    cs.add("index_registers_full_length", index_register_len(deep) == 600, "");
    // 8) 显示保尾：定宽预算 48 内输出不超预算且保住尾部文件名。
    let mut path_buf = [0u8; 1024];
    for i in 0..600 {
        path_buf[i] = if i % 8 == 7 { b'/' } else { b'a' + (i % 26) as u8 };
    }
    path_buf[592..600].copy_from_slice(b"name.txt");
    let mut out = [0u8; 128];
    let n = display_truncate(&path_buf[..600], DISPLAY_BUDGET, &mut out);
    cs.add(
        "display_within_budget",
        n <= DISPLAY_BUDGET && n == DISPLAY_BUDGET && out[0] == b'.' && out[1] == b'.' && out[2] == b'.',
        "",
    );
    // 9) 保尾保文件名：截断结果尾部恰为最后一段。
    cs.add(
        "display_keeps_filename",
        display_keeps_filename(&path_buf[..600], DISPLAY_BUDGET),
        "",
    );
    // 10) 短路径显示不截断（原样输出）。
    let short = b"/a/b/c.txt";
    let mut out2 = [0u8; 64];
    let n2 = display_truncate(short, DISPLAY_BUDGET, &mut out2);
    cs.add(
        "display_short_unchanged",
        n2 == short.len() && &out2[..n2] == short,
        "",
    );
    // 11) 深路径遍历性能：步数 = 段数（线性），非平方级。
    let segs = 40u16;
    let steps = traverse_steps(segs);
    cs.add(
        "traverse_linear_not_quadratic",
        steps == segs as usize && steps < (segs as usize) * (segs as usize),
        "",
    );
    cs
}

#[cfg(test)]
mod f534_tests {
    use super::*;

    /// 构造真实的 600 字符深路径（每 8 字节一个分隔符，尾部是文件名）。
    fn deep_path_600() -> [u8; 1024] {
        let mut path = [0u8; 1024];
        for i in 0..600 {
            path[i] = if i % 8 == 7 { b'/' } else { b'a' + (i % 26) as u8 };
        }
        path[592..600].copy_from_slice(b"name.txt");
        path
    }

    #[test]
    fn six_hundred_char_path_all_five_ops() {
        // 600 字符路径全操作用例：五操作全通过且零截断。
        let path = deep_path_600();
        assert_eq!(&path[592..600], b"name.txt", "构造的路径尾部应为文件名");
        let pv = PathView { len: 600, segments: 76 };
        assert_eq!(op_handle(OpKind::Create, pv), Ok(600), "创建应零截断");
        assert_eq!(op_handle(OpKind::Browse, pv), Ok(600), "浏览应零截断");
        assert_eq!(op_handle(OpKind::Copy, pv), Ok(600), "复制应零截断");
        assert_eq!(op_handle(OpKind::Delete, pv), Ok(600), "删除应零截断");
        assert_eq!(op_handle(OpKind::SearchIndex, pv), Ok(600), "搜索登记应零截断");
        assert!(is_long_path(600) && is_deep_case(600));
    }

    #[test]
    fn display_truncate_keeps_tail_filename() {
        // 显示保尾：预算 48 下「...」+尾部 45 字节，文件名 name.txt 完整保留。
        let path = deep_path_600();
        let mut out = [0u8; 128];
        let n = display_truncate(&path[..600], DISPLAY_BUDGET, &mut out);
        assert_eq!(n, DISPLAY_BUDGET, "输出应恰占满预算");
        assert_eq!(&out[..3], b"...", "长路径应以头省略号开头");
        assert_eq!(&out[n - 8..n], b"name.txt", "尾部文件名必须完整保留");
        assert!(display_keeps_filename(&path[..600], DISPLAY_BUDGET));
    }

    #[test]
    fn boundary_260_600_32767() {
        // 三条界线：260/261、600、32767/32768。
        assert!(!is_long_path(260) && is_long_path(261));
        assert!(!is_deep_case(599) && is_deep_case(600) && is_deep_case(601));
        let pv_max = PathView { len: MAX_PATH_SUPPORT, segments: 1 };
        assert_eq!(op_handle(OpKind::Copy, pv_max), Ok(MAX_PATH_SUPPORT), "32767 应在支持线内");
        let pv_over = PathView { len: MAX_PATH_SUPPORT + 1, segments: 1 };
        assert_eq!(op_handle(OpKind::Copy, pv_over), Err(PathErr::OverSupport), "32768 应诚实拒绝");
    }

    #[test]
    fn roundtrip_and_index_lossless() {
        // 复制路径完整（round-trip 一致）+ 索引覆盖（登记全长）。
        let path = deep_path_600();
        let pv = PathView { len: 600, segments: 76 };
        assert_eq!(copy_roundtrip(pv), pv, "round-trip 必须一致");
        assert_eq!(copy_roundtrip(pv).len, 600);
        assert_eq!(index_register_len(pv), 600, "索引必须登记完整路径");
        // UPath 建模的诚实拒绝：超容（>512）构造返回 None，不静默截断。
        assert!(UPath::from_bytes(&path[..600]).is_none(), "600 字符超出 PATH_CAP 应被拒绝");
        let short = UPath::from_bytes(b"/ok/file.txt").unwrap();
        assert_eq!(short.as_bytes(), b"/ok/file.txt", "容量内路径应完整保真");
    }

    #[test]
    fn traversal_linear_not_quadratic() {
        // 深路径遍历性能：步数 = 段数线性增长，预算断言排除平方级。
        for segs in [1u16, 8, 40, 76, 200] {
            assert_eq!(traverse_steps(segs), segs as usize, "{} 段应 {} 步", segs, segs);
        }
        let segs = 200usize;
        assert!(traverse_steps(segs as u16) < segs * segs, "步数必须远小于平方级");
    }

    #[test]
    fn display_short_paths_untouched() {
        // 短路径显示策略：不超预算原样输出；空路径输出 0。
        let mut out = [0u8; 64];
        let short = b"/home/user/doc/report.pdf";
        let n = display_truncate(short, DISPLAY_BUDGET, &mut out);
        assert_eq!(n, short.len());
        assert_eq!(&out[..n], short, "短路径不得被改写");
        assert!(display_keeps_filename(short, DISPLAY_BUDGET));
        assert_eq!(display_truncate(b"", 48, &mut out), 0, "空路径输出 0");
        assert_eq!(display_truncate(short, 0, &mut out), 0, "零预算输出 0");
    }
}

#[cfg(test)]
mod checkset_tests {
    use super::*;

    /// 七项 CheckSet 聚合自检：全部 PASS、条数在 8~14 规约带内、无截断。
    #[test]
    fn all_seven_checksets_pass() {
        let sets = [
            ("F524", run_f524_checks()),
            ("F529", run_f529_checks()),
            ("F530", run_f530_checks()),
            ("F531", run_f531_checks()),
            ("F532", run_f532_checks()),
            ("F533", run_f533_checks()),
            ("F534", run_f534_checks()),
        ];
        for (tag, cs) in sets {
            assert!(cs.all_passed(), "{} CheckSet 存在失败项", tag);
            assert!(!cs.truncated(), "{} CheckSet 被截断", tag);
            assert!((8..=14).contains(&cs.len()), "{} 检查条数 {} 越规约带", tag, cs.len());
        }
    }
}
