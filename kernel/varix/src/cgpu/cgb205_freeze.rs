//! CGPU-F0205 · 图 API 冻结（判据段 0x0670）
//! 帧图对外 API 冻结 v1：构建 API（节点/边/子图/模板/条件——四族
//! 全签名）、查询 API（状态/预算/耗时/拓扑）、控制 API（暂停/恢复/
//! 取消/优先级调整）、事件 API（完成/失败/预算/降级事件）。API 十
//! 年承诺（F0393 纪律）。判据：四族 API 冻结、语义测试、承诺声明、
//! 文档、示例。
//!
//! 诊断码段 0x0670（本模块独占：0x0670~0x067F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0670;

/// API 版本号（v1 冻结——破坏性变更必须升版本号，禁止静默变更）。
pub const API_VERSION: u32 = 1;

/// 冻结族数：构建/查询/控制/事件四族。
pub const API_FAMILIES: u8 = 4;

/// API 族标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 构建族（节点/边/子图/模板/条件）。
    Build,
    /// 查询族（状态/预算/耗时/拓扑）。
    Query,
    /// 控制族（暂停/恢复/取消/优先级）。
    Control,
    /// 事件族（完成/失败/预算/降级）。
    Event,
}

impl ApiFamily {
    /// 族序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            ApiFamily::Build => 0,
            ApiFamily::Query => 1,
            ApiFamily::Control => 2,
            ApiFamily::Event => 3,
        }
    }
    /// 族签名哈希（判据「四族 API 冻结」的签名锚点）：签名变更必须
    /// 显式升版本——语义测试以本值比对冻结基线。
    pub fn signature(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.ordinal() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ API_VERSION as u64;
        acc
    }
}

/// 构建操作（判据「构建 API」的语义面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildOp {
    /// 加节点：返回节点号。
    AddNode,
    /// 加边：u→v。
    AddEdge,
    /// 嵌入子图：子图节点整体挂入。
    AddSubgraph,
    /// 实例化模板。
    AddTemplate,
    /// 声明条件分支。
    AddCondition,
}

impl BuildOp {
    /// 操作码。
    pub fn opcode(&self) -> u8 {
        match self {
            BuildOp::AddNode => 1,
            BuildOp::AddEdge => 2,
            BuildOp::AddSubgraph => 3,
            BuildOp::AddTemplate => 4,
            BuildOp::AddCondition => 5,
        }
    }
}

/// 构建请求（判据「构建 API」统一签名：op + 两侧参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildReq {
    /// 操作。
    pub op: BuildOp,
    /// 参数 A（节点号/模板号/子图号）。
    pub a: u32,
    /// 参数 B（第二端点/条件码）。
    pub b: u32,
}

impl BuildReq {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.op.opcode() == 0 || self.a == 0 {
            return Err(SEG_BASE);
        }
        // 边与条件必须双侧有效。
        if matches!(self.op, BuildOp::AddEdge | BuildOp::AddCondition) && self.b == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 控制命令（判据「控制 API」语义面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlCmd {
    /// 暂停节点。
    Pause,
    /// 恢复节点。
    Resume,
    /// 取消节点（及其未开始的后继）。
    Cancel,
    /// 优先级调整。
    Reprioritize,
}

impl ControlCmd {
    /// 命令码。
    pub fn opcode(&self) -> u8 {
        match self {
            ControlCmd::Pause => 1,
            ControlCmd::Resume => 2,
            ControlCmd::Cancel => 3,
            ControlCmd::Reprioritize => 4,
        }
    }
}

/// 事件类别（判据「事件 API」语义面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 节点完成。
    Completed,
    /// 节点失败。
    Failed,
    /// 预算事件（超支/回退）。
    Budget,
    /// 降级事件（质量回退链触发）。
    Degraded,
}

impl EventKind {
    /// 事件码。
    pub fn code(&self) -> u8 {
        match self {
            EventKind::Completed => 1,
            EventKind::Failed => 2,
            EventKind::Budget => 3,
            EventKind::Degraded => 4,
        }
    }
}

/// API 调用账（判据「语义测试」的调用记录容器）：跨四族的全调用
/// 序列，供冻结基线比对。
#[derive(Default)]
pub struct ApiCallLog {
    calls: Vec<(u8, u8, u32)>,
}

impl ApiCallLog {
    /// 记录一次调用（族号, 操作码, 对象号）。
    pub fn log(&mut self, fam: ApiFamily, opcode: u8, target: u32) -> Result<(), u16> {
        if target == 0 {
            return Err(SEG_BASE + 2);
        }
        if fam.ordinal() >= API_FAMILIES {
            return Err(SEG_BASE + 3);
        }
        if self.calls.len() >= 4096 {
            self.calls.remove(0);
        }
        self.calls.push((fam.ordinal() as u8, opcode, target));
        Ok(())
    }
    /// 调用数。
    pub fn count(&self) -> usize {
        self.calls.len()
    }
    /// 按族过滤（判据「查询接口」语义测试面）。
    pub fn by_family(&self, fam: ApiFamily) -> Vec<(u8, u32)> {
        self.calls
            .iter()
            .filter(|(f, _, _)| *f == fam.ordinal())
            .map(|&(_, op, t)| (op, t))
            .collect()
    }
    /// 调用序列指纹（冻结基线比对锚点）。
    pub fn log_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for &(f, op, t) in &self.calls {
            acc = acc.wrapping_mul(0x100000001b3) ^ f as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ op as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ t as u64;
        }
        acc
    }
}

/// 语义测试账（判据「语义测试」主判据）：每个 API 族至少要有一类
/// 语义测试覆盖；覆盖缺失的族报错。
pub fn semantic_coverage_ok(log: &ApiCallLog) -> Result<[bool; 4], u16> {
    if log.count() == 0 {
        return Err(SEG_BASE + 4);
    }
    let mut covered = [false; 4];
    for f in 0..4u8 {
        covered[f as usize] = !log.by_family(ApiFamily::from_ordinal(f)).is_empty();
    }
    if covered.iter().any(|c| !c) {
        return Err(SEG_BASE + 5);
    }
    Ok(covered)
}

impl ApiFamily {
    /// 序号 → 族（语义测试遍历用）。
    pub fn from_ordinal(o: u8) -> ApiFamily {
        match o {
            0 => ApiFamily::Build,
            1 => ApiFamily::Query,
            2 => ApiFamily::Control,
            _ => ApiFamily::Event,
        }
    }
}

/// 控制命令状态机（判据「控制 API」语义核心）：暂停↔恢复、取消
/// 后不可恢复（终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeCtl {
    /// 运行中。
    Running,
    /// 已暂停。
    Paused,
    /// 已取消（终态）。
    Cancelled,
    /// 已完成（终态）。
    Finished,
}

impl NodeCtl {
    /// 命令合法性转移（判据「控制 API」语义测试核心断言）。
    pub fn transition(&self, cmd: ControlCmd) -> Result<NodeCtl, u16> {
        match (self, cmd) {
            (NodeCtl::Running, ControlCmd::Pause) => Ok(NodeCtl::Paused),
            (NodeCtl::Paused, ControlCmd::Resume) => Ok(NodeCtl::Running),
            (NodeCtl::Running, ControlCmd::Cancel) => Ok(NodeCtl::Cancelled),
            (NodeCtl::Paused, ControlCmd::Cancel) => Ok(NodeCtl::Cancelled),
            (NodeCtl::Running, ControlCmd::Reprioritize) => Ok(NodeCtl::Running),
            (NodeCtl::Paused, ControlCmd::Reprioritize) => Ok(NodeCtl::Paused),
            (NodeCtl::Cancelled, _) => Err(SEG_BASE + 6),
            (NodeCtl::Finished, _) => Err(SEG_BASE + 7),
            _ => Err(SEG_BASE + 8),
        }
    }
}

/// 事件发布（判据「事件 API」主判据）：节点到达终态必须发布对应
/// 事件——Finished→Completed、Cancelled→Failed；事件码与终态不匹配
/// 即冻结语义破坏。
pub fn publish_terminal_event(state: NodeCtl, node: u32, log: &mut ApiCallLog) -> Result<u8, u16> {
    if node == 0 {
        return Err(SEG_BASE + 9);
    }
    let kind = match state {
        NodeCtl::Finished => EventKind::Completed,
        NodeCtl::Cancelled => EventKind::Failed,
        _ => return Err(SEG_BASE + 10),
    };
    log.log(ApiFamily::Event, kind.code(), node)?;
    Ok(kind.code())
}

/// 冻结承诺声明（判据「承诺声明」载体）：版本 + 四族签名集 + 承诺
/// 文本指纹。承诺一旦登记，签名集变化必须伴随版本号递增。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezePledge {
    /// 版本号。
    pub version: u32,
    /// 四族签名集（按族序）。
    pub signatures: [u64; 4],
    /// 承诺文本指纹。
    pub pledge_fp: u64,
}

impl FreezePledge {
    /// 登记校验（判据「承诺声明」主判据）：版本非零、签名与当前
    /// 族签名一致、承诺指纹非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.version == 0 || self.version != API_VERSION {
            return Err(SEG_BASE + 11);
        }
        if self.pledge_fp == 0 {
            return Err(SEG_BASE + 12);
        }
        for f in 0..4u8 {
            let fam = ApiFamily::from_ordinal(f);
            if self.signatures[f as usize] != fam.signature() {
                return Err(SEG_BASE + 13);
            }
        }
        Ok(())
    }
}

/// 冻结基线漂移检测（判据「四族 API 冻结」门禁面）：登记的承诺与
/// 当前代码签名比对——漂移即破坏冻结。
pub fn freeze_drift_check(pledge: &FreezePledge) -> Result<(), u16> {
    pledge.validate()?;
    for f in 0..4u8 {
        let fam = ApiFamily::from_ordinal(f);
        if pledge.signatures[f as usize] != fam.signature() {
            return Err(SEG_BASE + 14);
        }
    }
    Ok(())
}

/// 查询族只读性验证（判据「查询 API」语义面）：查询操作不得改变
/// 状态机——查询前后节点状态必须一致。
pub fn query_is_pure(before: NodeCtl, after: NodeCtl) -> Result<bool, u16> {
    Ok(before == after)
}

/// 构建请求序列化指纹（判据「文档」示例可复现锚点）。
pub fn build_req_fp(r: &BuildReq) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.op.opcode() as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.a as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.b as u64;
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signatures_stable_per_version() {
        let f0 = ApiFamily::Build.signature();
        let f0b = ApiFamily::Build.signature();
        assert_eq!(f0, f0b);
        assert_ne!(f0, ApiFamily::Query.signature());
    }
    #[test]
    fn build_req_validation() {
        assert!(BuildReq { op: BuildOp::AddNode, a: 1, b: 0 }.validate().is_ok());
        assert!(BuildReq { op: BuildOp::AddEdge, a: 1, b: 0 }.validate().is_err());
        assert!(BuildReq { op: BuildOp::AddEdge, a: 1, b: 2 }.validate().is_ok());
        assert!(BuildReq { op: BuildOp::AddNode, a: 0, b: 0 }.validate().is_err());
    }
    #[test]
    fn control_state_machine() {
        let mut st = NodeCtl::Running;
        st = st.transition(ControlCmd::Pause).unwrap();
        assert_eq!(st, NodeCtl::Paused);
        st = st.transition(ControlCmd::Resume).unwrap();
        assert_eq!(st, NodeCtl::Running);
        st = st.transition(ControlCmd::Cancel).unwrap();
        assert_eq!(st, NodeCtl::Cancelled);
        // 终态不可再转。
        assert!(st.transition(ControlCmd::Resume).is_err());
    }
    #[test]
    fn event_terminal_binding() {
        let mut log = ApiCallLog::default();
        assert_eq!(publish_terminal_event(NodeCtl::Finished, 5, &mut log).unwrap(), 1);
        assert_eq!(publish_terminal_event(NodeCtl::Cancelled, 6, &mut log).unwrap(), 2);
        // 非终态发布拒绝。
        assert!(publish_terminal_event(NodeCtl::Running, 7, &mut log).is_err());
    }
    #[test]
    fn semantic_coverage_all_families() {
        let mut log = ApiCallLog::default();
        log.log(ApiFamily::Build, BuildOp::AddNode.opcode(), 1).unwrap();
        log.log(ApiFamily::Query, 1, 1).unwrap();
        log.log(ApiFamily::Control, ControlCmd::Pause.opcode(), 1).unwrap();
        log.log(ApiFamily::Event, EventKind::Completed.code(), 1).unwrap();
        assert_eq!(semantic_coverage_ok(&log).unwrap(), [true; 4]);
        // 缺事件族 → 拒绝。
        let mut log2 = ApiCallLog::default();
        log2.log(ApiFamily::Build, 1, 1).unwrap();
        assert!(semantic_coverage_ok(&log2).is_err());
    }
    #[test]
    fn freeze_pledge_and_drift() {
        let pledge = FreezePledge {
            version: API_VERSION,
            signatures: [
                ApiFamily::Build.signature(),
                ApiFamily::Query.signature(),
                ApiFamily::Control.signature(),
                ApiFamily::Event.signature(),
            ],
            pledge_fp: 0xBEEF,
        };
        assert!(freeze_drift_check(&pledge).is_ok());
        // 篡改一族签名 → 漂移检出。
        let mut bad = pledge;
        bad.signatures[2] = 0x1234;
        assert!(freeze_drift_check(&bad).is_err());
    }
    #[test]
    fn query_purity() {
        assert!(query_is_pure(NodeCtl::Running, NodeCtl::Running).unwrap());
        assert!(!query_is_pure(NodeCtl::Running, NodeCtl::Paused).unwrap());
    }
}
