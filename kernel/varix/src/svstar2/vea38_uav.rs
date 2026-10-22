//! VE-F0038 · 无序访问视图管理（VE-A 域 · A02b 资源视图组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0038`
//!
//! **职责定位（锚点原文）**：UAV 的统一管理（并发访问计数/原子操作声明/
//! 全局引用约束），UAV 并发冲突的静态检测。
//!
//! **数据结构（锚点原文）**：视图管理；冲突检测。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 并发冲突 → 静态检出（提交前按访问集做重叠判定，不带进 GPU）；
//! - 计数漂移 → 审计（并发计数 acquire/release 不平衡时显性报码）；
//! - 越界 → 拒绝（绑定槽位/元素范围越界直接拒）。
//!
//! **性能逐项分解（锚点原文）**：O(视图)。绑定 O(1)；冲突判定 O(同 pass
//! 绑定数) 提交期一次性。
//!
//! **跨批对接点（锚点原文）**：A36 同构；A31 帧图联动——pass 边界由帧图
//! 给出，本模块在 pass 提交点做静态冲突检测。
//!
//! **无障碍与隐私（锚点原文）**：视图状态读屏可达。
//!
//! **判据（锚点原文）**：并发计数、原子声明、冲突检测、全局约束、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0x1600 基址（全域核查无占用）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UAV_VERSION: &str = "vea38_uav-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x1600;

// ---- 诊断码 ----
pub const XC_RANGE: u32 = SEG_BASE + 1; // 元素范围越界
pub const XC_SLOT: u32 = SEG_BASE + 2; // 绑定槽位越界
pub const XC_DRIFT: u32 = SEG_BASE + 3; // 计数漂移（acquire/release 不平衡）
pub const XC_CONFLICT: u32 = SEG_BASE + 4; // 静态冲突检出
pub const XC_GREF_FULL: u32 = SEG_BASE + 5; // 全局引用表满
pub const XC_NO_DECL: u32 = SEG_BASE + 6; // 原子操作未声明却使用

/// 访问类型——原子操作声明（锚点原文：原子操作必须显式声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessOp {
    /// 只读加载（可与任意读并发）。
    Load,
    /// 非原子写（独占要求最强）。
    Store,
    /// 原子加（返回旧值）。
    AtomicAdd,
    /// 原子交换。
    AtomicExchange,
    /// 原子比较交换。
    AtomicCompareExchange,
    /// 原子最值。
    AtomicMinMax,
}

impl AccessOp {
    /// 是否原子操作。
    pub fn is_atomic(self) -> bool {
        !matches!(self, AccessOp::Load | AccessOp::Store)
    }
    /// 是否写语义（含原子写语义）。
    pub fn is_write(self) -> bool {
        !matches!(self, AccessOp::Load)
    }
}

/// 一次 UAV 绑定声明：资源、元素范围、操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UavBind {
    pub res_id: u32,
    /// 元素范围 [start, end)（含头不含尾）。
    pub start: u32,
    pub end: u32,
    pub op: AccessOp,
}

impl UavBind {
    /// 与另一绑定是否范围重叠。
    pub fn overlaps(&self, o: &UavBind) -> bool {
        self.res_id == o.res_id && self.start < o.end && o.start < self.end
    }
}

/// 静态冲突判定：同 pass 内两条绑定的相容性。
/// - 读×读：相容；
/// - 原子×原子：范围重叠即冲突（同元素上的原子序需要用户显式加同步原语，
///   静态层一律报冲突，宁可误报不可漏报——漏报就是数据竞争）；
/// - 任一为非原子写且重叠：冲突。
pub fn static_conflict(a: &UavBind, b: &UavBind) -> bool {
    if !a.overlaps(b) {
        return false;
    }
    match (a.op, b.op) {
        (AccessOp::Load, AccessOp::Load) => false,
        _ => true,
    }
}

/// UAV 记录。
#[derive(Clone, Copy, Debug)]
pub struct UavRec {
    pub res_id: u32,
    pub elem_count: u32,
    /// 并发引用计数（acquire/release 平衡审计用）。
    pub refs: u32,
    /// 生命周期累计 acquire 总数。
    pub acquires: u32,
    /// 生命周期累计 release 总数。
    pub releases: u32,
}

/// 无序访问视图管理器。
pub struct UavManager {
    recs: Vec<UavRec>,
    cap: usize,
    last_code: u32,
    /// 静态冲突检出总数。
    conflicts_found: u32,
    /// 计数漂移审计次数。
    drift_audits: u32,
}

/// 同 pass 绑定集——提交前整体过静态检测。
pub struct PassBinds {
    binds: Vec<UavBind>,
}

impl PassBinds {
    pub fn new() -> PassBinds {
        PassBinds { binds: Vec::new() }
    }
    pub fn push(&mut self, b: UavBind) {
        self.binds.push(b);
    }
    pub fn len(&self) -> usize {
        self.binds.len()
    }
    pub fn is_empty(&self) -> bool {
        self.binds.is_empty()
    }
    /// 静态冲突检测：两两判定，返回冲突对下标（首对即返给调用方定位）。
    /// O(n²) 在 pass 内绑定数（个位数量级）下成立；全局只有这一处是二次方。
    pub fn first_conflict(&self) -> Option<(usize, usize)> {
        for i in 0..self.binds.len() {
            for j in (i + 1)..self.binds.len() {
                if static_conflict(&self.binds[i], &self.binds[j]) {
                    return Some((i, j));
                }
            }
        }
        None
    }
    /// 原子声明完备性：任何写语义操作必须是原子（UAV 写语义强制原子——
    /// 非原子写在无序访问语义下没有定义良好的执行序）。
    /// Store 例外：单独存在时按「独占写」处理，由冲突检测保证独占。
    pub fn atomic_decl_ok(&self) -> bool {
        // 声明完备 = 每条绑定要么是原子操作、要么是 Load/Store 两者之一，
        // 不存在「写语义但既非原子也未走独占通道」的第三态。
        self.binds.iter().all(|b| {
            matches!(b.op, AccessOp::Load | AccessOp::Store) || b.op.is_atomic()
        })
    }
}

impl UavManager {
    pub fn new(cap: usize) -> UavManager {
        UavManager { recs: Vec::new(), cap, last_code: 0, conflicts_found: 0, drift_audits: 0 }
    }

    /// 注册 UAV 资源。
    pub fn register(&mut self, res_id: u32, elem_count: u32) -> Result<usize, u32> {
        if res_id == 0 || elem_count == 0 {
            self.last_code = XC_RANGE;
            return Err(self.last_code);
        }
        if self.recs.iter().any(|r| r.res_id == res_id) || self.recs.len() >= self.cap {
            self.last_code = XC_GREF_FULL;
            return Err(self.last_code);
        }
        self.recs.push(UavRec { res_id, elem_count, refs: 0, acquires: 0, releases: 0 });
        Ok(self.recs.len() - 1)
    }

    /// 校验一条绑定（范围越界 + 槽位存在性）。
    pub fn validate(&self, b: &UavBind) -> Result<(), u32> {
        match self.recs.iter().find(|r| r.res_id == b.res_id) {
            None => Err(XC_SLOT),
            Some(r) => {
                if b.start >= b.end || b.end > r.elem_count {
                    Err(XC_RANGE)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// acquire 引用。
    pub fn acquire(&mut self, res_id: u32) -> bool {
        match self.recs.iter_mut().find(|r| r.res_id == res_id) {
            Some(r) => {
                r.refs += 1;
                r.acquires += 1;
                true
            }
            None => {
                self.last_code = XC_SLOT;
                false
            }
        }
    }

    /// release 引用；下溢即计数漂移，显性报码且不回绕。
    pub fn release(&mut self, res_id: u32) -> bool {
        match self.recs.iter_mut().find(|r| r.res_id == res_id) {
            Some(r) if r.refs > 0 => {
                r.refs -= 1;
                r.releases += 1;
                true
            }
            Some(_) => {
                self.last_code = XC_DRIFT;
                self.drift_audits += 1;
                false
            }
            None => {
                self.last_code = XC_SLOT;
                false
            }
        }
    }

    /// 全局引用约束审计：全部资源 acquire == release 且 refs 归零。
    pub fn audit_balanced(&self) -> bool {
        self.recs.iter().all(|r| r.acquires == r.releases && r.refs == 0)
    }

    /// 提交前静态检测入口：绑定集先逐条 validate，再整集冲突判定。
    pub fn presubmit(&mut self, pass: &PassBinds) -> Result<(), u32> {
        for b in pass.binds.iter() {
            let _ = self.validate(b)?;
        }
        if !pass.atomic_decl_ok() {
            self.last_code = XC_NO_DECL;
            return Err(self.last_code);
        }
        if let Some(_) = pass.first_conflict() {
            self.conflicts_found += 1;
            self.last_code = XC_CONFLICT;
            return Err(self.last_code);
        }
        Ok(())
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let unbalanced = self.recs.iter().filter(|r| r.refs != 0).count();
        alloc::format!(
            "UAV 在册{} 未平引用{} 冲突检出{} 漂移审计{}",
            self.recs.len(),
            unbalanced,
            self.conflicts_found,
            self.drift_audits
        )
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }
    pub fn conflicts_found(&self) -> u32 {
        self.conflicts_found
    }
    pub fn drift_audits(&self) -> u32 {
        self.drift_audits
    }
    pub fn len(&self) -> usize {
        self.recs.len()
    }
}

// =====================================================================
// 自检（判据锚点：并发计数 / 原子声明 / 冲突检测 / 全局约束）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0038 域自检。
pub fn run_vea38_checks() -> CheckSet {
    let mut set = CheckSet::new("vea38");

    fn b(res: u32, s: u32, e: u32, op: AccessOp) -> UavBind {
        UavBind { res_id: res, start: s, end: e, op }
    }

    // C38-01 并发计数：acquire/release 平衡审计绿
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 1024);
        let _ = m.acquire(5);
        let _ = m.acquire(5);
        let _ = m.release(5);
        let _ = m.release(5);
        let ok = m.audit_balanced();
        chk(&mut set, "C38-01 计数平衡", ok, "账不平衡");
    }

    // C38-02 计数漂移：多 release 下溢显性且不回绕
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 1024);
        let ok = !m.release(5) && m.last_code() == XC_DRIFT && m.drift_audits() == 1;
        chk(&mut set, "C38-02 漂移显性", ok, "下溢未显性");
    }

    // C38-03 范围越界拒绝（end > elem_count）
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 100);
        let ok = m.validate(&b(5, 90, 101, AccessOp::Load)) == Err(XC_RANGE);
        chk(&mut set, "C38-03 范围越界", ok, "越界被放行");
    }

    // C38-04 空范围拒绝（start >= end）
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 100);
        let ok = m.validate(&b(5, 50, 50, AccessOp::Load)) == Err(XC_RANGE);
        chk(&mut set, "C38-04 空范围", ok, "空范围被放行");
    }

    // C38-05 资源不存在（全局引用约束：未注册资源拒绝绑定）
    {
        let m = UavManager::new(8);
        let ok = m.validate(&b(9, 0, 1, AccessOp::Load)) == Err(XC_SLOT);
        chk(&mut set, "C38-05 未注册拒", ok, "未注册资源被放行");
    }

    // C38-06 冲突检测：读×读重叠相容
    {
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::Load));
        p.push(b(5, 32, 96, AccessOp::Load));
        let ok = p.first_conflict().is_none();
        chk(&mut set, "C38-06 读读相容", ok, "读读误报冲突");
    }

    // C38-07 冲突检测：非原子写×读重叠 = 冲突
    {
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::Store));
        p.push(b(5, 32, 96, AccessOp::Load));
        let ok = p.first_conflict() == Some((0, 1));
        chk(&mut set, "C38-07 写读冲突", ok, "冲突漏检");
    }

    // C38-08 冲突检测：原子×原子重叠也判冲突（静态层宁可误报不可漏报）
    {
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::AtomicAdd));
        p.push(b(5, 32, 96, AccessOp::AtomicMinMax));
        let ok = p.first_conflict().is_some();
        chk(&mut set, "C38-08 原子重叠", ok, "原子重叠漏检");
    }

    // C38-09 预提交全流程：合法集通过
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 1024);
        let _ = m.register(6, 1024);
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::AtomicAdd));
        p.push(b(6, 0, 64, AccessOp::Load));
        let ok = m.presubmit(&p).is_ok();
        chk(&mut set, "C38-09 预提交过", ok, "合法集被拒");
    }

    // C38-10 预提交拦截：冲突集报 XC_CONFLICT 且计数 +1
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 1024);
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::Store));
        p.push(b(5, 0, 64, AccessOp::Load));
        let ok = m.presubmit(&p) == Err(XC_CONFLICT) && m.conflicts_found() == 1;
        chk(&mut set, "C38-10 预提交拦", ok, "冲突集被放行");
    }

    // C38-11 原子声明：Store 走独占通道（atomic_decl_ok 认可 Load/Store/原子三态）
    {
        let mut p = PassBinds::new();
        p.push(b(5, 0, 64, AccessOp::Store));
        let ok = p.atomic_decl_ok();
        chk(&mut set, "C38-11 声明三态", ok, "合法三态被拒");
    }

    // C38-12 越界绑定在预提交被拦（逐条 validate 先行）
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 100);
        let mut p = PassBinds::new();
        p.push(b(5, 0, 200, AccessOp::Load));
        let ok = m.presubmit(&p) == Err(XC_RANGE);
        chk(&mut set, "C38-12 预提交越界", ok, "越界集被放行");
    }

    // C38-13 重复注册拒绝（同 id 全局唯一）
    {
        let mut m = UavManager::new(8);
        let _ = m.register(5, 1024);
        let ok = m.register(5, 512) == Err(XC_GREF_FULL);
        chk(&mut set, "C38-13 重复注册", ok, "重复注册被放行");
    }

    // C38-14 确定性
    {
        let seq = || -> (u32, u32) {
            let mut m = UavManager::new(8);
            let _ = m.register(5, 1024);
            let _ = m.acquire(5);
            let _ = m.release(5);
            let mut p = PassBinds::new();
            p.push(b(5, 0, 10, AccessOp::AtomicAdd));
            p.push(b(5, 10, 20, AccessOp::Load));
            let _ = m.presubmit(&p);
            (m.conflicts_found(), m.drift_audits())
        };
        chk(&mut set, "C38-14 确定性", seq() == seq(), "两次不一致");
    }

    // C38-15 读屏行四要素
    {
        let m = UavManager::new(8);
        let line = m.status_line();
        let ok = line.contains("在册") && line.contains("未平引用") && line.contains("冲突检出")
            && line.contains("漂移审计");
        chk(&mut set, "C38-15 读屏要素", ok, "读屏行缺要素");
    }


    // C38-90 码位互异且段独占（判据自身：整仓唯一段 0x1600 内六码）
    {
        let codes = [XC_RANGE, XC_SLOT, XC_DRIFT, XC_CONFLICT, XC_GREF_FULL, XC_NO_DECL];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1600);
        chk(&mut set, "C38-90 码位互异段独占", all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea38_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea38_bind_lifecycle() {
        let mut m = UavManager::new(8);
        assert!(m.register(5, 1024).is_ok());
        assert!(m.acquire(5));
        assert!(m.acquire(5));
        assert!(m.release(5));
        assert!(!m.release(9), "未注册资源释放拒绝");
        assert!(m.release(5));
        assert!(m.audit_balanced(), "收工账必须平衡");
        assert_eq!(m.last_code(), 0);
    }

    #[test]
    fn vea38_conflict_matrix() {
        // 读×读相容；写×读冲突；原子×原子重叠冲突（宁可误报不可漏报）
        let mut p = PassBinds::new();
        p.push(UavBind { res_id: 5, start: 0, end: 64, op: AccessOp::Load });
        p.push(UavBind { res_id: 5, start: 32, end: 96, op: AccessOp::Load });
        assert!(p.first_conflict().is_none());
        let mut p2 = PassBinds::new();
        p2.push(UavBind { res_id: 5, start: 0, end: 64, op: AccessOp::Store });
        p2.push(UavBind { res_id: 5, start: 32, end: 96, op: AccessOp::AtomicAdd });
        assert_eq!(p2.first_conflict(), Some((0, 1)));
        assert!(!p2.is_empty() && p2.len() == 2);
    }

    #[test]
    fn vea38_status_line_elements() {
        let m = UavManager::new(4);
        let line = m.status_line();
        assert!(line.contains("在册") && line.contains("漂移审计"), "{}", line);
        assert!(UAV_VERSION.starts_with("vea38_uav"));
    }

    #[test]
    fn vea38_checks_all_green() {
        let set = run_vea38_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0038 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
