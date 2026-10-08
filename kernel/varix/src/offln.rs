//! offln — WP-204 · B-606 离线态（MD2 篇 6.5）。
//!
//! 判据 B-606：全系统离线呈现一致，无黑箱等待。
//! MD2 原文（6.5）："离线态是头等公民：网络服务降级（MD1 第 25.1 节）时
//! 桌面明确标注'离线'，星图的应用更新按钮变灰并说明原因，vscode 等直插
//! 应用的网络失败得到三要素报错——绝不出现'转圈十分钟后超时'的黑箱。
//! 弱网（高丢包）场景：栈参数（重传、窗口）按 smoltcp 默认起步，判例
//! 复测时如遇弱网病再调——先测量后调参，不凭感觉动旋钮。"
//!
//! 宿主可测形态：单一网络状态源 + 三呈现面派生（桌面标注 / 更新按钮
//! 变灰带原因 / 直插应用三要素报错）+ 一致性对账（状态一翻三面同步）+
//! 无黑箱防线（离线态任何网络操作即时返回三要素，操作延迟恒 0——
//! "转圈十分钟后超时"在模型面不可能）+ 弱网调参纪律（先测量后调参，
//! auto_tune 默认关）。

use crate::checks::CheckSet;

/// 网络状态（单一状态源；MD1 25.1 网络服务降级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetState {
    Online,
    Offline,
}

/// 网络失败三要素（离线态直插应用报错文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NetFailTriple {
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 离线三要素文案（常量锁）。
pub const OFFLINE_TRIPLE: NetFailTriple = NetFailTriple {
    what: "网络操作失败",
    why: "网络处于离线状态",
    next: "请检查网线或 WiFi 后重试",
};

/// 直插应用的网络操作类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetOp {
    Fetch,
    Sync,
    Update,
}

/// 呈现模型：单一状态源，三呈现面全部派生（一致性由构造保证）。
pub struct PresentModel {
    pub state: NetState,
}

impl PresentModel {
    pub const fn new(state: NetState) -> PresentModel {
        PresentModel { state }
    }

    /// 呈现面一：桌面网络标注（明确标注"离线"——不静默）。
    pub const fn desktop_label(&self) -> &'static str {
        match self.state {
            NetState::Online => "在线",
            NetState::Offline => "离线",
        }
    }

    /// 呈现面二：星图应用更新按钮（enabled, 原因说明）。
    pub const fn update_button(&self) -> (bool, &'static str) {
        match self.state {
            NetState::Online => (true, ""),
            NetState::Offline => (false, "网络离线，暂时无法检查更新"),
        }
    }

    /// 呈现面三：直插应用网络操作（vscode 等）。
    /// 离线态：**即时**返回三要素——延迟恒 0（无黑箱防线的模型面）。
    pub fn app_net_op(&mut self, op: NetOp) -> Result<&'static str, NetFailTriple> {
        match self.state {
            NetState::Online => Ok("ok"),
            NetState::Offline => {
                let _ = op; // 操作类型只影响 what 的措辞面；宿主模型统一文案
                Err(OFFLINE_TRIPLE)
            }
        }
    }

    /// 无黑箱防线：离线态操作延迟恒 0（即时报错，绝不转圈等超时）。
    pub const fn op_latency_ms(&self, op: NetOp) -> u64 {
        match self.state {
            NetState::Online => 0, // 在线态延迟由真实网络决定，模型面不管
            NetState::Offline => 0, // **离线态恒 0**——即时三要素，无黑箱
        }
    }
}

/// 弱网调参纪律（先测量后调参，不凭感觉动旋钮）。
pub struct TuningPolicy {
    /// 自动调参默认关（smoltcp 默认参数起步）。
    pub auto_tune: bool,
    pub measurements: u64,
    pub tunes: u64,
}

impl TuningPolicy {
    pub const fn new() -> TuningPolicy {
        TuningPolicy { auto_tune: false, measurements: 0, tunes: 0 }
    }

    /// 测量记录（判例复测取证）。
    pub fn measure(&mut self) {
        self.measurements += 1;
    }

    /// 调参请求：**必须**先有测量记录（先测量后调参——不带测量不批）。
    pub fn request_tune(&mut self, with_measurement: bool) -> bool {
        if self.auto_tune {
            return false; // 自动调参被纪律禁止
        }
        if !with_measurement || self.measurements == 0 {
            return false; // 无测量不批
        }
        self.tunes += 1;
        true
    }
}

// ---------------------------------------------------------------- 对练

/// 离线态对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct OffDrillSummary {
    pub rounds: u32,
    pub flips: u64,
    /// 三呈现面始终与状态源一致
    pub consistent: bool,
    /// 离线态操作即时报错（延迟 0 + 三要素）
    pub no_black_box: bool,
}

/// 随机状态翻转序列 × 随机操作对练：一致性 + 无黑箱闭环。
pub fn run_off_drills(seed: u64, rounds: u32) -> OffDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = OffDrillSummary::default();
    sum.rounds = rounds;
    sum.consistent = true;
    sum.no_black_box = true;
    let mut m = PresentModel::new(NetState::Online);
    for _ in 0..rounds {
        // 随机翻转
        let next = if g.next() % 2 == 0 { NetState::Online } else { NetState::Offline };
        if m.state != next {
            m.state = next;
            sum.flips += 1;
        }
        // 一致性对账：三呈现面从同一状态派生，必须同口径
        let label = m.desktop_label();
        let (btn_on, btn_why) = m.update_button();
        let want_label = match m.state {
            NetState::Online => "在线",
            NetState::Offline => "离线",
        };
        let want_btn = m.state == NetState::Online;
        let why_ok = match m.state {
            NetState::Offline => !btn_why.is_empty(),
            NetState::Online => btn_why.is_empty(),
        };
        if label != want_label || btn_on != want_btn || !why_ok {
            sum.consistent = false;
        }
        // 无黑箱对账：离线态任何操作即时三要素、延迟 0
        let ops = [NetOp::Fetch, NetOp::Sync, NetOp::Update];
        let op = ops[(g.next() % 3) as usize];
        match m.app_net_op(op) {
            Ok(_) => {
                if m.state == NetState::Offline {
                    sum.no_black_box = false; // 离线却"成功"——黑箱
                }
            }
            Err(t) => {
                if m.state != NetState::Offline || m.op_latency_ms(op) != 0
                    || t.what.is_empty() || t.why.is_empty() || t.next.is_empty()
                {
                    sum.no_black_box = false;
                }
            }
        }
        // 在线态延迟模型不管（真实网络面）；离线态恒 0 已在 Err 分支锁
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_offln_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-606 离线态");
    {
        // 桌面明确标注"离线"
        let m = PresentModel::new(NetState::Offline);
        set.add(
            "B-606 桌面标注离线",
            m.desktop_label() == "离线",
            "MD2 6.5：桌面明确标注'离线'",
        );
    }
    {
        // 更新按钮变灰并说明原因
        let m = PresentModel::new(NetState::Offline);
        let (enabled, why) = m.update_button();
        set.add(
            "B-606 更新按钮变灰带原因",
            !enabled && !why.is_empty(),
            "星图应用更新按钮变灰并说明原因",
        );
    }
    {
        // 在线态恢复一致
        let m = PresentModel::new(NetState::Online);
        let (enabled, why) = m.update_button();
        set.add(
            "B-606 在线态恢复",
            m.desktop_label() == "在线" && enabled && why.is_empty(),
            "状态回在线三面同步恢复",
        );
    }
    {
        // 离线态直插应用三要素报错
        let mut m = PresentModel::new(NetState::Offline);
        set.add(
            "B-606 离线三要素报错",
            m.app_net_op(NetOp::Fetch) == Err(OFFLINE_TRIPLE)
                && m.app_net_op(NetOp::Update) == Err(OFFLINE_TRIPLE),
            "vscode 等直插应用的网络失败得到三要素报错",
        );
    }
    {
        // 三要素文案锁
        set.add(
            "B-606 三要素文案锁",
            !OFFLINE_TRIPLE.what.is_empty()
                && !OFFLINE_TRIPLE.why.is_empty()
                && !OFFLINE_TRIPLE.next.is_empty(),
            "WHAT/WHY/NEXT 全非空",
        );
    }
    {
        // 无黑箱防线：离线操作延迟恒 0（绝不转圈十分钟后超时）
        let mut m = PresentModel::new(NetState::Offline);
        let ops = [NetOp::Fetch, NetOp::Sync, NetOp::Update];
        let zero = ops.iter().all(|op| m.app_net_op(*op).is_err() && m.op_latency_ms(*op) == 0);
        set.add(
            "B-606 无黑箱防线（延迟恒 0）",
            zero,
            "绝不出现'转圈十分钟后超时'的黑箱",
        );
    }
    {
        // 弱网纪律：先测量后调参
        let mut tp = TuningPolicy::new();
        let no_meas = !tp.request_tune(false);
        let meas_but_none = {
            tp.measurements = 0;
            !tp.request_tune(true)
        };
        tp.measure();
        let with_meas = tp.request_tune(true);
        set.add(
            "B-606 先测量后调参",
            !tp.auto_tune && no_meas && meas_but_none && with_meas,
            "smoltcp 默认起步；不带测量不批调参",
        );
    }
    {
        // 离线态对练
        let sum = run_off_drills(0xB606, 80);
        set.add(
            "B-606 离线态对练",
            sum.rounds == 80 && sum.consistent && sum.no_black_box,
            "全系统离线呈现一致，无黑箱等待（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f706_triple_face_consistent() {
        // 三呈现面从同一状态派生：离线
        let m = PresentModel::new(NetState::Offline);
        assert_eq!(m.desktop_label(), "离线");
        let (enabled, why) = m.update_button();
        assert!(!enabled && !why.is_empty());
        // 在线
        let m = PresentModel::new(NetState::Online);
        assert_eq!(m.desktop_label(), "在线");
        let (enabled, why) = m.update_button();
        assert!(enabled && why.is_empty());
    }

    #[test]
    fn f706_no_black_box() {
        let mut m = PresentModel::new(NetState::Offline);
        for op in [NetOp::Fetch, NetOp::Sync, NetOp::Update] {
            let r = m.app_net_op(op);
            assert!(r.is_err(), "离线态操作必须即时失败");
            assert_eq!(m.op_latency_ms(op), 0, "离线态延迟恒 0");
            let t = r.unwrap_err();
            assert!(!t.what.is_empty() && !t.why.is_empty() && !t.next.is_empty());
        }
    }

    #[test]
    fn f706_online_ok() {
        let mut m = PresentModel::new(NetState::Online);
        assert!(m.app_net_op(NetOp::Fetch).is_ok());
        assert!(m.app_net_op(NetOp::Sync).is_ok());
    }

    #[test]
    fn f706_drill_deterministic() {
        let a = run_off_drills(3, 40);
        let b = run_off_drills(3, 40);
        assert_eq!(a, b);
        assert!(a.consistent && a.no_black_box);
        assert!(a.flips > 0, "40 轮随机翻转必有翻转");
    }
}
