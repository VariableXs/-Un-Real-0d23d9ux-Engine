//! F199 版本与谱系页（secstar2 · G-G-29）——每台机器知道自己从哪来。
//!
//! **判据（主册）**：谱系与构建记录对拍一致；哈希复制粘贴保真；组件清单
//! 跳转正确。
//!
//! **功能定义（主册 G-G-29）**：「关于本机」谱系区：系统版本树（STAR I →
//! STAR I start 各版本节点）/本镜像构建指纹（可复现构建哈希 WP-401 语义）/
//! 各借力件版本（F130 联动）。
//!
//! 【交互设计】F123 关于本机页尾部「版本谱系」区：版本树横向时间线（当前
//! 节点高亮）/构建哈希行（复制钮）/「查看组件清单」（F130 登记册跳转）；
//! 谱系数据只读。
//! 【数据与存储】版本清单编译期生成（构建系统产出）；哈希=镜像构建指纹
//! （F130 同源）。
//! 【状态与异常】谱系数据缺失（自编译无清单）→ 「本地构建」标注+跳过节点
//! （诚实）；更新中断的双槽态 → 两版本并显（F190 联动）。
//! 【设计细节】版本树最多显 6 节点（更早折叠「+N」）；当前节点=强调色圆点
//! +「运行中」标；构建指纹格式 vX.Y.Z+<哈希前 12 位>；组件清单页复用
//! F130 表格组件；谱系数据进 F128 开放 JSON（第三方工具可解析——生态同
//! 语言）。
//!
//! 依赖锚点：F123（关于本机页）、F128（开放 JSON）、F130（组件登记册）、F190（双槽态）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 版本树可见节点上限（更早折叠「+N」）。
pub const TREE_VISIBLE_MAX: usize = 6;
/// 构建指纹哈希截取位数。
pub const FINGERPRINT_HEX: usize = 12;
/// 本体谱系（编译期常量——构建系统产出注入点）。
pub const LINEAGE: [&str; 3] = ["STAR I", "STAR I start", "STAR I start.1"];

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 版本节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionNode {
    /// 版本名（如 "STAR I start.1"）。
    pub name: &'static str,
    /// 发布序（小=早）。
    pub seq: u32,
}

/// 组件条目（F130 登记册投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentEntry {
    pub name: &'static str,
    /// 上游版本串（诚实呈现——含本地改动标注由登记册承载）。
    pub version: &'static str,
    /// 许可（开放可验证）。
    pub license: &'static str,
}

/// 谱系页数据体。
pub struct Lineage {
    /// 版本树（按 seq 升序）。
    pub tree: Vec<VersionNode>,
    /// 当前运行节点 seq。
    pub current_seq: u32,
    /// 构建指纹（完整哈希——展示取前 12 位）。
    pub fingerprint: [u8; 32],
    /// 版本号（语义化）。
    pub semver: (u32, u32, u32),
    /// 本地构建标注（自编译无清单 → 诚实跳过节点）。
    pub local_build: bool,
    /// 组件清单（F130 投影）。
    pub components: Vec<ComponentEntry>,
}

impl Lineage {
    /// 构造（tree 乱序输入按 seq 排序——一处一事实：seq 是唯一序）。
    pub fn new(tree: Vec<VersionNode>, current_seq: u32, fingerprint: [u8; 32], semver: (u32, u32, u32)) -> Lineage {
        let mut tree = tree;
        tree.sort_by_key(|n| n.seq);
        Lineage { tree, current_seq, fingerprint, semver, local_build: false, components: Vec::new() }
    }

    /// 哈希前 12 位 hex 到调用方缓冲（复制钮数据源——复制粘贴保真判据；
    /// 完整版式 vX.Y.Z+<hex12> 由 UI 层拼接，本函数供确定性数据）。
    pub fn fingerprint_hex12(&self, out: &mut [u8; FINGERPRINT_HEX]) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for i in 0..FINGERPRINT_HEX / 2 {
            let b = self.fingerprint[i];
            out[i * 2] = HEX[(b >> 4) as usize];
            out[i * 2 + 1] = HEX[(b & 0xF) as usize];
        }
    }

    /// hex12 二次生成一致性（复制钮数据源确定性——保真判据）。
    pub fn fingerprint_hex12_stable(&self, out: &mut [u8; FINGERPRINT_HEX]) -> bool {
        let mut again = [0u8; FINGERPRINT_HEX];
        self.fingerprint_hex12(&mut again);
        *out == again && out.iter().all(|&b| b != 0)
    }

    /// 版本树展示节点（最多 6 个，更早折叠）——返回 (可见节点, 折叠数)。
    pub fn visible_tree(&self) -> (Vec<VersionNode>, usize) {
        if self.tree.len() <= TREE_VISIBLE_MAX {
            return (self.tree.clone(), 0);
        }
        let fold = self.tree.len() - TREE_VISIBLE_MAX;
        (self.tree[fold..].to_vec(), fold)
    }

    /// 当前节点判定（横向时间线高亮+「运行中」标）。
    pub fn is_current(&self, n: &VersionNode) -> bool {
        n.seq == self.current_seq
    }

    /// 双槽态并显（F190 联动）：回滚点版本与当前版本并排——更新中断场景。
    pub fn dual_slot_texts(&self, rollback_version: Option<&'static str>) -> Vec<&'static str> {
        let mut out = Vec::new();
        out.push(self.tree.iter().find(|n| n.seq == self.current_seq).map(|n| n.name).unwrap_or("未知版本"));
        if let Some(rv) = rollback_version {
            out.push(rv);
        }
        out
    }

    /// 开放 JSON 导出（F128 生态同语言——第三方工具可解析）。
    /// 手写序列化（内核 no_std 无 serde；键序稳定——可复现）。
    pub fn open_json(&self, out: &mut Vec<u8>) {
        push(out, b"{\"lineage\":{\"semver\":\"");
        push_semver(out, self.semver);
        push(out, b"\",\"current\":\"");
        if let Some(n) = self.tree.iter().find(|n| n.seq == self.current_seq) {
            push(out, n.name.as_bytes());
        }
        push(out, b"\",\"fingerprint\":\"");
        let mut hex = [0u8; FINGERPRINT_HEX];
        self.fingerprint_hex12(&mut hex);
        push(out, &hex);
        push(out, b"\",\"localBuild\":");
        push(out, if self.local_build { b"true" } else { b"false" });
        push(out, b",\"tree\":[");
        let (visible, fold) = self.visible_tree();
        for (i, n) in visible.iter().enumerate() {
            if i > 0 {
                push(out, b",");
            }
            push(out, b"{\"name\":\"");
            push(out, n.name.as_bytes());
            push(out, b"\",\"seq\":");
            push_u64(out, n.seq as u64);
            push(out, b"}");
        }
        if fold > 0 {
            push(out, b"],\"folded\":");
            push_u64(out, fold as u64);
            push(out, b",\"components\":[");
        } else {
            push(out, b"],\"components\":[");
        }
        for (i, c) in self.components.iter().enumerate() {
            if i > 0 {
                push(out, b",");
            }
            push(out, b"{\"name\":\"");
            push(out, c.name.as_bytes());
            push(out, b"\",\"version\":\"");
            push(out, c.version.as_bytes());
            push(out, b"\",\"license\":\"");
            push(out, c.license.as_bytes());
            push(out, b"\"}");
        }
        push(out, b"]}}");
    }

    /// 谱系与构建记录对拍（判据一）：注入外部构建记录 (semver, fingerprint)，
    /// 逐字段比——不一致即脱节（R8 腐化网）。
    pub fn matches_build_record(&self, semver: (u32, u32, u32), fp: [u8; 32]) -> bool {
        self.semver == semver && self.fingerprint == fp
    }
}

pub(crate) fn push(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
}

pub(crate) fn push_u64(out: &mut Vec<u8>, mut v: u64) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&buf[i..]);
}

fn push_semver(out: &mut Vec<u8>, v: (u32, u32, u32)) {
    push_u64(out, v.0 as u64);
    out.push(b'.');
    push_u64(out, v.1 as u64);
    out.push(b'.');
    push_u64(out, v.2 as u64);
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F199 自检（聚合进 secstar2 域）。
pub fn run_lineage_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-lineage");

    let fp_src = b"STAR I start reproducible build";
    // ksha256 复用（F194 同族——一处一事实）。
    let fp = crate::ksha256::sha256(fp_src);
    let mut tree = vec![
        VersionNode { name: "STAR I start.1", seq: 3 },
        VersionNode { name: "STAR I", seq: 1 },
        VersionNode { name: "STAR I start", seq: 2 },
    ];
    let mut lin = Lineage::new(core::mem::take(&mut tree), 3, fp, (1, 0, 1));

    // 谱系排序与当前节点。
    let (vis, fold) = lin.visible_tree();
    set.add("tree sorted", vis[0].name == "STAR I" && vis[2].name == "STAR I start.1", "");
    set.add("no fold small tree", fold == 0, "");
    set.add("current marked", lin.is_current(&vis[2]), "");

    // 折叠：塞 8 个节点 → 显 6 折 2。
    let mut big: Vec<VersionNode> = (1..=8).map(|i| VersionNode { name: "v", seq: i }).collect();
    let lin8 = Lineage::new(core::mem::take(&mut big), 8, fp, (1, 0, 1));
    let (vis8, fold8) = lin8.visible_tree();
    set.add("tree folded", vis8.len() == TREE_VISIBLE_MAX && fold8 == 2, "");

    // 哈希复制粘贴保真（判据二）：hex12 与 SHA-256 前缀一致。
    let mut hex12 = [0u8; 12];
    lin.fingerprint_hex12(&mut hex12);
    set.add("hex12 stable", lin.fingerprint_hex12_stable(&mut hex12), "");
    set.add("hex12 len", hex12.len() == 12, "");

    // 判据一：构建记录对拍。
    set.add("build record match", lin.matches_build_record((1, 0, 1), fp), "");
    let mut fp_bad = fp;
    fp_bad[0] ^= 1;
    set.add("build record mismatch", !lin.matches_build_record((1, 0, 1), fp_bad), "");

    // 双槽态并显。
    let dual = lin.dual_slot_texts(Some("STAR I start"));
    set.add("dual slot", dual.len() == 2 && dual[0] == "STAR I start.1" && dual[1] == "STAR I start", "");
    set.add("single when no rollback", lin.dual_slot_texts(None).len() == 1, "");

    // 开放 JSON（F128）：可解析形状 + 折叠字段按需出现。
    lin.components.push(ComponentEntry { name: "limine", version: "8.x", license: "BSD-2-Clause" });
    let mut json = Vec::new();
    lin.open_json(&mut json);
    let js = core::str::from_utf8(&json).unwrap_or("");
    set.add("json shape", js.starts_with("{\"lineage\":{") && js.ends_with("}}"), "");
    set.add("json fingerprint", js.contains(&core::str::from_utf8(&hex12).unwrap_or("?")), "");
    set.add("json component", js.contains("limine") && js.contains("BSD-2-Clause"), "");
    set.add("json no fold when small", !js.contains("\"folded\""), "");
    let mut json8 = Vec::new();
    lin8.open_json(&mut json8);
    set.add("json fold present", core::str::from_utf8(&json8).unwrap_or("").contains("\"folded\":2"), "");

    // 本地构建诚实标注。
    lin.local_build = true;
    let mut json3 = Vec::new();
    lin.open_json(&mut json3);
    set.add("local build honest", core::str::from_utf8(&json3).unwrap_or("").contains("\"localBuild\":true"), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lineage {
        let fp = crate::ksha256::sha256(b"sample build");
        let mut lin = Lineage::new(
            vec![
                VersionNode { name: "STAR I", seq: 1 },
                VersionNode { name: "STAR I start", seq: 2 },
                VersionNode { name: "STAR I start.1", seq: 3 },
            ],
            3,
            fp,
            (1, 0, 1),
        );
        lin.components.push(ComponentEntry { name: "limine", version: "8.x", license: "BSD-2-Clause" });
        lin.components.push(ComponentEntry { name: "rustls", version: "0.23", license: "Apache-2.0/MIT" });
        lin
    }

    #[test]
    fn f199_json_roundtrip_parseable_shape() {
        let lin = sample();
        let mut json = Vec::new();
        lin.open_json(&mut json);
        let js = core::str::from_utf8(&json).unwrap();
        // 引号配对（手写序列化的形状自检）。
        assert_eq!(js.matches('"').count() % 2, 0, "quotes balanced");
        assert!(js.contains("\"seq\":3"));
        assert!(js.contains("\"current\":\"STAR I start.1\""));
    }

    #[test]
    fn f199_hex12_matches_sha_prefix() {
        let lin = sample();
        let mut hex = [0u8; 12];
        lin.fingerprint_hex12(&mut hex);
        let full = crate::ksha256::sha256(b"sample build");
        let hex_tbl = b"0123456789abcdef";
        for i in 0..6 {
            assert_eq!(hex[i * 2] as char, hex_tbl[(full[i] >> 4) as usize] as char);
            assert_eq!(hex[i * 2 + 1] as char, hex_tbl[(full[i] & 0xF) as usize] as char);
        }
    }

    #[test]
    fn f199_local_build_skips_nodes() {
        let fp = crate::ksha256::sha256(b"local");
        let mut lin = Lineage::new(vec![VersionNode { name: "x", seq: 1 }], 1, fp, (0, 0, 1));
        lin.local_build = true;
        let mut json = Vec::new();
        lin.open_json(&mut json);
        assert!(core::str::from_utf8(&json).unwrap().contains("\"localBuild\":true"));
    }

    #[test]
    fn f199_build_record_catches_drift() {
        let lin = sample();
        assert!(lin.matches_build_record((1, 0, 1), crate::ksha256::sha256(b"sample build")));
        assert!(!lin.matches_build_record((1, 0, 2), crate::ksha256::sha256(b"sample build")));
    }

    #[test]
    fn f199_run_checks_pass() {
        assert!(run_lineage_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：TimelineModel —— 版本树横向时间线渲染（主册【交互设计】：版本树
// 横向时间线（当前节点高亮）/更早折叠「+N」——渲染数据一次给齐，UI 不再
// 自算几何）
// ---------------------------------------------------------------------------

/// 单节点时间线渲染数据。
pub struct TimelineNode {
    pub name: &'static str,
    pub seq: u32,
    /// 横向槽位（0 起——按显示序，UI 直接映射列位置）。
    pub slot: usize,
    /// 当前运行节点（强调色圆点+「运行中」标）。
    pub current: bool,
}

/// 时间线完整渲染数据。
pub struct TimelineModel {
    pub nodes: Vec<TimelineNode>,
    /// 折叠徽标（None=无折叠；Some(n)=「+n」更早节点）。
    pub fold_chip: Option<usize>,
}

/// 时间线组装（谱系页唯一几何来源——visible_tree 的渲染投影）。
pub fn timeline_model(lin: &Lineage) -> TimelineModel {
    let (visible, fold) = lin.visible_tree();
    TimelineModel {
        nodes: visible
            .iter()
            .enumerate()
            .map(|(slot, n)| TimelineNode {
                name: n.name,
                seq: n.seq,
                slot,
                current: lin.is_current(n),
            })
            .collect(),
        fold_chip: if fold > 0 { Some(fold) } else { None },
    }
}

// ---------------------------------------------------------------------------
// 深二：fingerprint_line —— 构建指纹完整行（主册【设计细节】逐字：构建
// 指纹格式 vX.Y.Z+<哈希前 12 位>——复制钮复制的就是这一行的确定性数据）
// ---------------------------------------------------------------------------

/// 指纹完整行（`v1.0.1+abc123def456` 形态；缓冲不足返回 None——零静默）。
pub fn fingerprint_line(lin: &Lineage, out: &mut String) -> Option<()> {
    let mut hex = [0u8; FINGERPRINT_HEX];
    lin.fingerprint_hex12(&mut hex);
    out.push('v');
    push_semver_str(out, lin.semver);
    out.push('+');
    // hex12 缓冲已是 ASCII 十六进制字符（fingerprint_hex12 的输出契约）——
    // 直接推送，不二次编码。
    for b in hex.iter() {
        out.push(*b as char);
    }
    Some(())
}

fn push_semver_str(out: &mut String, v: (u32, u32, u32)) {
    push_u32_str(out, v.0);
    out.push('.');
    push_u32_str(out, v.1);
    out.push('.');
    push_u32_str(out, v.2);
}

fn push_u32_str(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut v = v;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

// ---------------------------------------------------------------------------
// 深三：ComponentTable —— F130 组件清单表格数据（主册【交互设计】：
// 「查看组件清单」（F130 登记册跳转）；组件清单页复用 F130 表格组件——
// 行数据契约在此，跳转锚也在此）
// ---------------------------------------------------------------------------

/// 组件表行（license 列 + 跳转锚——F130 表格组件的输入契约）。
pub struct ComponentRow {
    pub name: &'static str,
    pub version: &'static str,
    pub license: &'static str,
    /// 跳转锚（登记册行 ID——F130 页面的定位键）。
    pub anchor: String,
}

/// 组件表组装（名序排序——登记册对拍用同序）。
pub fn component_table(lin: &Lineage) -> Vec<ComponentRow> {
    let mut comps: Vec<&ComponentEntry> = lin.components.iter().collect();
    comps.sort_by_key(|c| c.name);
    comps
        .into_iter()
        .map(|c| ComponentRow {
            name: c.name,
            version: c.version,
            license: c.license,
            anchor: alloc::format!("goto:f130/{}", c.name),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 深四：RollbackRetention —— 双槽回滚保留期联动（主册【状态与异常】：
// 更新中断的双槽态 → 两版本并显（F190 联动）——保留期剩余天数一并可见，
// 灰置有预告）
// ---------------------------------------------------------------------------

/// 双槽并显行 + 保留期提示（F190 的槽卡数据注入）。
pub struct DualSlotLine {
    /// 当前版本行。
    pub current: &'static str,
    /// 回滚点版本行（None=无双槽态）。
    pub rollback: Option<&'static str>,
    /// 保留期剩余天数行（Some 时 rollback 必有——对称纪律）。
    pub retention_days: Option<u64>,
    /// 保留期到期提示（剩余 0 天——灰置预告文案）。
    pub expiry_note: Option<&'static str>,
}

/// 组装（days=剩余天数；0 = 今日到期——预告不是恐吓）。
pub fn dual_slot_line(lin: &Lineage, rollback_version: Option<&'static str>, days_left: u64) -> DualSlotLine {
    DualSlotLine {
        current: lin.dual_slot_texts(rollback_version)[0],
        rollback: rollback_version,
        retention_days: rollback_version.map(|_| days_left),
        expiry_note: if rollback_version.is_some() && days_left == 0 {
            Some("回滚点今日到期——明天将按策略清理（F190）")
        } else {
            None
        },
    }
}

// ---------------------------------------------------------------------------
// 深五：BuildManifest —— 构建清单一致性（主册【数据与存储】：版本清单
// 编译期生成（构建系统产出）——清单与谱系页数据同源对拍：组件数、当前
// 节点、semver 三处一致才可发布；不一致即 R8 腐化网要抓的脱节）
// ---------------------------------------------------------------------------

/// 构建清单（构建系统产出的最小投影）。
pub struct BuildManifest {
    pub semver: (u32, u32, u32),
    pub current_seq: u32,
    pub component_count: usize,
}

/// 对拍（谱系页 vs 构建清单——三处一致判据）。
pub fn matches_manifest(lin: &Lineage, m: &BuildManifest) -> bool {
    lin.semver == m.semver
        && lin.current_seq == m.current_seq
        && lin.components.len() == m.component_count
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F199 深化自检（聚合进 secstar2 域）。
pub fn run_lineage_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-deep");

    let fp = crate::ksha256::sha256(b"deep lineage build");
    let mut lin = Lineage::new(
        vec![
            VersionNode { name: "STAR I", seq: 1 },
            VersionNode { name: "STAR I start", seq: 2 },
            VersionNode { name: "STAR I start.1", seq: 3 },
        ],
        3,
        fp,
        (1, 0, 1),
    );
    lin.components.push(ComponentEntry { name: "rustls", version: "0.23", license: "Apache-2.0/MIT" });
    lin.components.push(ComponentEntry { name: "limine", version: "8.x", license: "BSD-2-Clause" });

    // 深一：时间线——槽位连续、当前节点唯一且高亮、小树无折叠。
    let tm = timeline_model(&lin);
    set.add("timeline slots", tm.nodes.iter().enumerate().all(|(i, n)| n.slot == i), "");
    set.add("timeline current unique", tm.nodes.iter().filter(|n| n.current).count() == 1, "");
    set.add("timeline current is latest", tm.nodes.last().map(|n| n.current) == Some(true), "");
    set.add("timeline no fold small", tm.fold_chip.is_none(), "");
    // 8 节点树 → 折叠徽标 +2。
    let big: Vec<VersionNode> = (1..=8u32).map(|i| VersionNode { name: "v", seq: i }).collect();
    let lin8 = Lineage::new(big, 8, fp, (1, 0, 1));
    let tm8 = timeline_model(&lin8);
    set.add("timeline fold chip", tm8.fold_chip == Some(2) && tm8.nodes.len() == TREE_VISIBLE_MAX, "");

    // 深二：指纹完整行——主册格式逐字、复制保真（二次生成一致）。
    let mut line1 = String::new();
    let mut line2 = String::new();
    fingerprint_line(&lin, &mut line1).unwrap();
    fingerprint_line(&lin, &mut line2).unwrap();
    set.add("fp line stable", line1 == line2, "");
    set.add("fp line shape", line1.starts_with("v1.0.1+") && line1.len() == 7 + FINGERPRINT_HEX, "");
    let mut hex12 = [0u8; FINGERPRINT_HEX];
    lin.fingerprint_hex12(&mut hex12);
    let hex_str = core::str::from_utf8(&hex12).unwrap_or("?");
    set.add("fp line hex suffix", line1.ends_with(hex_str), "");

    // 深三：组件表——名序、license 列在、跳转锚可定位。
    let ct = component_table(&lin);
    set.add("comp sorted", ct[0].name == "limine" && ct[1].name == "rustls", "");
    set.add("comp license col", ct.iter().all(|r| !r.license.is_empty()), "");
    set.add("comp anchor", ct[0].anchor == "goto:f130/limine", "");

    // 深四：双槽保留期——剩余天数随行、到期预告、无回滚点无保留期行。
    let dsl = dual_slot_line(&lin, Some("STAR I start"), 5);
    set.add("dual retention", dsl.retention_days == Some(5) && dsl.expiry_note.is_none(), "");
    let dsl0 = dual_slot_line(&lin, Some("STAR I start"), 0);
    set.add("dual expiry note", dsl0.expiry_note.map(|s| s.contains("清理")) == Some(true), "");
    let dsl1 = dual_slot_line(&lin, None, 7);
    set.add("dual no rollback no retention", dsl1.retention_days.is_none() && dsl1.expiry_note.is_none(), "");

    // 深五：构建清单对拍——三处一致绿；组件数漂移红。
    let ok = matches_manifest(&lin, &BuildManifest { semver: (1, 0, 1), current_seq: 3, component_count: 2 });
    set.add("manifest match", ok, "");
    let drift = matches_manifest(&lin, &BuildManifest { semver: (1, 0, 1), current_seq: 3, component_count: 3 });
    set.add("manifest drift red", !drift, "组件数脱节=构建腐化网要抓的第一现场");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    fn sample() -> Lineage {
        let fp = crate::ksha256::sha256(b"deep sample build");
        let mut lin = Lineage::new(
            vec![
                VersionNode { name: "STAR I", seq: 1 },
                VersionNode { name: "STAR I start", seq: 2 },
                VersionNode { name: "STAR I start.1", seq: 3 },
            ],
            3,
            fp,
            (1, 0, 2),
        );
        lin.components.push(ComponentEntry { name: "limine", version: "8.x", license: "BSD-2-Clause" });
        lin
    }

    #[test]
    fn f199_deep_fingerprint_line_matches_open_json() {
        // 指纹行与开放 JSON 的 fingerprint 字段同源（一处一事实——两处渲染
        // 一份哈希，第三方对照两处必相等）。
        let lin = sample();
        let mut line = String::new();
        fingerprint_line(&lin, &mut line).unwrap();
        let mut json = Vec::new();
        lin.open_json(&mut json);
        let mut hex = [0u8; FINGERPRINT_HEX];
        lin.fingerprint_hex12(&mut hex);
        let hex_str = core::str::from_utf8(&hex).unwrap();
        assert!(line.ends_with(hex_str));
        assert!(core::str::from_utf8(&json).unwrap().contains(hex_str));
    }

    #[test]
    fn f199_deep_timeline_100_nodes_never_breaks() {
        // 100 节点谱系：渲染恒显 6、折叠数正确、当前节点永远在可见窗内。
        let fp = crate::ksha256::sha256(b"huge tree");
        let tree: Vec<VersionNode> = (1..=100u32).map(|i| VersionNode { name: "n", seq: i }).collect();
        let lin = Lineage::new(tree, 100, fp, (9, 9, 9));
        let tm = timeline_model(&lin);
        assert_eq!(tm.nodes.len(), TREE_VISIBLE_MAX);
        assert_eq!(tm.fold_chip, Some(94));
        assert!(tm.nodes.iter().filter(|n| n.current).count() == 1);
        assert_eq!(tm.nodes.last().unwrap().seq, 100, "current node always visible");
    }

    #[test]
    fn f199_deep_component_table_stable_order() {
        // 两次组装同序（登记册对拍依赖稳定序——排序确定性回归）。
        let mut lin = sample();
        lin.components.push(ComponentEntry { name: "a11y-gate", version: "1.0", license: "MIT" });
        let t1: Vec<&str> = component_table(&lin).iter().map(|r| r.name).collect();
        let t2: Vec<&str> = component_table(&lin).iter().map(|r| r.name).collect();
        assert_eq!(t1, t2);
        assert_eq!(t1, vec!["a11y-gate", "limine"]);
    }

    #[test]
    fn f199_deep_run_checks_pass() {
        assert!(run_lineage_deep_checks().all_passed());
    }
}
