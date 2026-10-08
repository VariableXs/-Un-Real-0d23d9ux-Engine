//! CGPU-F1281 · I 域开工与带宽总架构域自检（判据五条逐条映射 + 反向语料钉门禁）。
//!
//! **判据（锚点原文）**：五主题、账户化、边界、十组、判据。
//!
//! 自检纪律：判据区零 panic 面（下标走 `get`/`Option`）；判据侧独立
//! 重算（十组表与五主题字面量独立重排逐条全等、守恒式判据侧独立重算、
//! 反向语料证明门禁不恒绿——超预算必拒、零预算必拒、退款超消费必拒）。

use super::cgi01_bandwidth::{
    ten_group_line, topics_present, BandwidthTopic, BudgetLedger, BANDWIDTH_ARCH_VERSION,
    BOUNDARY_DOC, Grant, I_GROUPS, TRANSFER_KINDS,
};

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["五主题", "账户化", "边界", "十组", "判据"];

/// 判据侧独立重算的十组规划表（与本体 [`I_GROUPS`] 逐条全等——被误改先红）。
const GROUPS_RECHECK: [(&str, u32, u32); 10] = [
    ("带宽总架构与 PCIe 仲裁组", 1281, 1296),
    ("上传下载调度组", 1297, 1312),
    ("共享内存带宽组", 1313, 1328),
    ("带宽与 G 域协同组", 1329, 1344),
    ("传输安全与诊断组", 1345, 1360),
    ("混合场景带宽组", 1361, 1376),
    ("带宽与场景适配组", 1377, 1392),
    ("I 域预备与自查组", 1393, 1408),
    ("带宽场景扩展组", 1409, 1424),
    ("I 域收口组", 1425, 1440),
];

/// 判据侧独立重算的五主题标签表。
const TOPIC_LABELS_RECHECK: [&str; 5] = [
    "PCIe带宽仲裁",
    "共享内存带宽",
    "上传下载调度",
    "带宽遥测",
    "瓶颈归因",
];

/// CGPU-F1281 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgi01_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgi01_bandwidth");

    // —— 判据一 · 五主题：闭集齐备 + 标签独立重排全等 ——
    let topics = topics_present();
    let mut topics_ok = topics.len() == 5 && CRITERIA_RECHECK.len() == 5;
    let mut ti = 0usize;
    while ti < topics.len() {
        let t = match topics.get(ti) {
            Some(t) => *t,
            None => break,
        };
        let label = t.label();
        let expect = match TOPIC_LABELS_RECHECK.get(ti) {
            Some(l) => *l,
            None => "",
        };
        if label != expect {
            topics_ok = false;
        }
        ti += 1;
    }
    s.add(
        "I01-五主题-闭集齐备标签全等",
        topics_ok
            && BandwidthTopic::PcieArbitration.label() != BandwidthTopic::SharedMemBandwidth.label(),
        "官方五主题（PCIe仲裁/共享内存/上传下载调度/遥测/归因）闭集冻结；标签与判据侧独立重排逐条全等",
    );

    // —— 判据二 · 账户化：开立/申请/拒绝/退款/守恒 ——
    let mut ledger = BudgetLedger::new();
    let up = ledger.allocate(TRANSFER_KINDS[0], 1_000_000);
    let zero = ledger.allocate(TRANSFER_KINDS[1], 0);
    let g1 = ledger.request(TRANSFER_KINDS[0], 400_000);
    let g2 = ledger.request(TRANSFER_KINDS[0], 700_000); // 超余额（余 60 万）
    let g3 = ledger.request(TRANSFER_KINDS[0], 0); // 零字节也拒
    let r1 = ledger.refund(TRANSFER_KINDS[0], 100_000);
    let r_bad = ledger.refund(TRANSFER_KINDS[0], 999_999); // 超净消费（30 万）
    let granted_ok = g1 == Grant::Granted { remaining: 600_000 };
    let denied_ok = match g2 {
        Grant::Denied { available } => available == 600_000,
        _ => false,
    };
    s.add(
        "I01-账户化-申请拒绝退款全链",
        up.is_ok()
            && zero.is_err()
            && granted_ok
            && denied_ok
            && matches!(g3, Grant::Denied { .. })
            && r1
            && !r_bad
            && ledger.is_consistent(TRANSFER_KINDS[0])
            && ledger.utilization_ppm(TRANSFER_KINDS[0]) == Some(300_000),
        "开账户→申请40万批（余60万）→超支70万拒+零字节拒（拒绝留痕）→退10万（净消费30万）→退款超消费拒；守恒与使用率30%判据侧独立重算",
    );

    // —— 反向：未开账户请求必拒（门禁不恒绿） ——
    let ghost = ledger.request(TRANSFER_KINDS[3], 1);
    let ghost_denied = match ghost {
        Grant::Denied { available } => available == 0,
        _ => false,
    };
    let all_open_after = ledger.allocate(TRANSFER_KINDS[3], 500_000).is_ok()
        && ledger.allocate(TRANSFER_KINDS[1], 500_000).is_ok()
        && ledger.allocate(TRANSFER_KINDS[2], 500_000).is_ok();
    s.add(
        "I01-反向-未开账户必拒",
        ghost_denied && !ledger.all_consistent() && all_open_after,
        "未开立账户的请求拒绝且可用为 0（不报幻觉额度）；四账户补齐后 all_consistent 才成立",
    );

    // —— 四账户全开守恒 + 摘要行 ——
    let all_ok = ledger.all_consistent();
    let line = ledger.summary_line(TRANSFER_KINDS[0]);
    let dline = ledger.denied_line(TRANSFER_KINDS[0], 600_000);
    s.add(
        "I01-账本-四账户守恒+遥测摘要",
        all_ok
            && TRANSFER_KINDS.len() == 4
            && line.contains("纹理上传") && line.contains("拒绝2次")
            && dline.contains("余额不足"),
        "四类传输（上传/下载/交换/流送）账户全开全守恒；摘要行含消费与拒绝计数（遥测主题的开工面）",
    );

    // —— 判据四 · 边界声明：三域分工逐句可 grep ——
    s.add(
        "I01-边界-三域分工声明",
        BOUNDARY_DOC.contains("H 域管显存驻留")
            && BOUNDARY_DOC.contains("本 I 域管传输带宽")
            && BOUNDARY_DOC.contains("F0405")
            && BOUNDARY_DOC.contains("F5801")
            && BOUNDARY_DOC.contains("存储流管数据从盘到内存的到达")
            && BANDWIDTH_ARCH_VERSION.starts_with("I01-"),
        "H 管驻留/I 管搬运/AD 管到达三段各管一段；接口复用 F0405、存储流分工 F5801+ 逐句点名",
    );

    // —— 判据五 · 十组：规划表与判据侧独立重排逐条全等 + 起止号连续 ——
    let mut groups_ok = I_GROUPS.len() == 10 && GROUPS_RECHECK.len() == 10;
    let mut gi = 0usize;
    while gi < I_GROUPS.len() {
        let g = match I_GROUPS.get(gi) {
            Some(g) => *g,
            None => break,
        };
        let r = match GROUPS_RECHECK.get(gi) {
            Some(r) => *r,
            None => break,
        };
        if g.0 != r.0 || g.1 != r.1 || g.2 != r.2 {
            groups_ok = false;
        }
        if gi > 0 {
            let prev = match I_GROUPS.get(gi - 1) {
                Some(p) => *p,
                None => break,
            };
            if g.1 != prev.2 + 1 {
                groups_ok = false; // 起止号必须首尾连续（160 项无缺口）
            }
        }
        gi += 1;
    }
    let total_span = I_GROUPS[0].1 == 1281 && I_GROUPS[9].2 == 1440;
    let line10 = ten_group_line();
    s.add(
        "I01-十组-规划表独立对账+读屏行",
        groups_ok
            && total_span
            && line10.contains("I01")
            && line10.contains("I10")
            && line10.contains("带宽总架构与 PCIe 仲裁组"),
        "十组组名+起止单号与判据侧独立重排逐条全等；组间起止首尾连续（F1281-F1440 无缺口）；宣告行读屏可查",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["五主题", "账户化", "边界", "十组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "I01-判据stamp-五条独立重排全等",
        stamp_ok && I_GROUPS.len() == 10 && topics_present().len() == 5,
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）；五主题×十组闭集尺寸钉死",
    );

    s
}
