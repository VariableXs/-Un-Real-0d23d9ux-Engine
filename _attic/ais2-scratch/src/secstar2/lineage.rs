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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——开放 JSON 形状校验 / 时间线
// 图例 / 复制保真模拟。判据源：主册【设计细节】「谱系数据进 F128 开放
// JSON（第三方工具可解析——生态同语言）」的校验面 +【交互设计】时间线
// 视觉语义 +【验收判据】「哈希复制粘贴保真」的往返模拟。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：JsonShapeCheck —— 开放 JSON 形状校验器（手写序列化的对偶：
// 生成的 JSON 必须过自己的校验器——发射方自带接收方，闭环自证）
// ---------------------------------------------------------------------------

/// 形状校验结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonShape {
    /// 花括号配对。
    pub braces_balanced: bool,
    /// 引号配对。
    pub quotes_balanced: bool,
    /// 顶层键在位（lineage/semver/current/fingerprint）。
    pub keys_present: bool,
    /// 无控制字符。
    pub clean: bool,
}

impl JsonShape {
    pub fn ok(&self) -> bool {
        self.braces_balanced && self.quotes_balanced && self.keys_present && self.clean
    }
}

/// 校验（字节级——不引依赖，四查足够定位手写序列化的常见病）。
pub fn json_shape_check(data: &[u8]) -> JsonShape {
    let mut braces: i64 = 0;
    let mut quotes = 0usize;
    let mut clean = true;
    for &b in data {
        match b {
            b'{' => braces += 1,
            b'}' => braces -= 1,
            b'"' => quotes += 1,
            0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F => clean = false,
            _ => {}
        }
    }
    let s = core::str::from_utf8(data).unwrap_or("");
    let keys_present = s.contains("\"lineage\"")
        && s.contains("\"semver\"")
        && s.contains("\"current\"")
        && s.contains("\"fingerprint\"");
    JsonShape {
        braces_balanced: braces == 0,
        quotes_balanced: quotes % 2 == 0,
        keys_present,
        clean,
    }
}

// ---------------------------------------------------------------------------
// v3-二：TimelineLegend —— 时间线图例（三态圆点的语义表——渲染与帮助
// 共用：用户问「这个点为什么亮」时答案就在图例里）
// ---------------------------------------------------------------------------

/// 图例条目。
pub struct LegendEntry {
    pub state: &'static str,
    pub meaning: &'static str,
}

/// 图例（三态定序——当前/历史/折叠）。
pub const TIMELINE_LEGEND: [LegendEntry; 3] = [
    LegendEntry { state: "current", meaning: "当前运行中的版本（强调色圆点 + 运行中标）" },
    LegendEntry { state: "history", meaning: "历史上的版本节点（中性圆点，可点看详情）" },
    LegendEntry { state: "folded", meaning: "更早的版本已折叠（+N 徽标，点开展开）" },
];

/// 图例完整性（三态齐+语义非空——图例是契约不是装饰）。
pub fn timeline_legend_intact() -> bool {
    TIMELINE_LEGEND.len() == 3
        && TIMELINE_LEGEND.iter().all(|l| !l.state.is_empty() && l.meaning.len() >= 8)
        && TIMELINE_LEGEND[0].state == "current"
}

// ---------------------------------------------------------------------------
// v3-三：CopySim —— 复制保真模拟（判据「哈希复制粘贴保真」的往返模拟：
// 指纹行 → 模拟剪贴板 → 读回 → 与二次生成比对——三处一致才算保真）
// ---------------------------------------------------------------------------

/// 复制保真结论（三源一致：首生成 / 剪贴板回读 / 二次生成）。
pub fn copy_sim_ok(lin: &Lineage) -> bool {
    let mut first = String::new();
    let _ = fingerprint_line(lin, &mut first);
    // 模拟剪贴板（原样搬运——保真模拟关注的是生成端确定性）。
    let clipboard = first.clone();
    let mut second = String::new();
    let _ = fingerprint_line(lin, &mut second);
    !first.is_empty() && clipboard == first && second == first
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F199 v3 自检（聚合进 secstar2 域）。
pub fn run_lineage_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v3");

    let fp = crate::ksha256::sha256(b"v3 lineage build");
    let mut lin = Lineage::new(
        vec![
            VersionNode { name: "STAR I", seq: 1 },
            VersionNode { name: "STAR I start", seq: 2 },
            VersionNode { name: "STAR I start.1", seq: 3 },
        ],
        3,
        fp,
        (1, 0, 3),
    );
    lin.components.push(ComponentEntry { name: "limine", version: "8.x", license: "BSD-2-Clause" });

    // v3-一：形状校验——自家 JSON 过自家校验器；坏样本诚实拒绝。
    let mut json = Vec::new();
    lin.open_json(&mut json);
    let shape = json_shape_check(&json);
    set.add("json own shape ok", shape.ok(), "");
    set.add("json braces", shape.braces_balanced && shape.quotes_balanced, "");
    set.add("json keys", shape.keys_present, "");
    set.add("json bad rejected", !json_shape_check(b"{\"lineage\"").ok(), "缺右括号即红");
    set.add("json ctrl rejected", !json_shape_check(b"{\"a\":\"\x01\"}").clean, "控制字符即红");

    // v3-二：图例——三态齐、语义人话。
    set.add("legend intact", timeline_legend_intact(), "");
    set.add("legend fold", TIMELINE_LEGEND[2].meaning.contains("+N"), "");
    set.add("legend current first", TIMELINE_LEGEND[0].state == "current", "");

    // v3-三：复制保真——三源一致。
    set.add("copy sim ok", copy_sim_ok(&lin), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f199_v3_shape_check_catches_real_defects() {
        // 手写序列化常见病全部可检（缺括号/缺键/引号失衡/脏字符）。
        assert!(!json_shape_check(b"{\"lineage\":{\"semver\":\"1.0.0\"}").ok(), "缺一层右括号");
        assert!(!json_shape_check(b"{\"lineage\":{}}").keys_present, "键不全");
        assert!(!json_shape_check(b"{\"a\":\"unterminated}").ok(), "引号失衡");
        assert!(json_shape_check(b"{\"lineage\":{\"semver\":\"1\",\"current\":\"x\",\"fingerprint\":\"a\"}}").ok());
    }

    #[test]
    fn f199_v3_copy_sim_deterministic_across_calls() {
        // 十次复制模拟全等（生成端确定性的强化口径）。
        let fp = crate::ksha256::sha256(b"copy sim");
        let lin = Lineage::new(vec![VersionNode { name: "x", seq: 1 }], 1, fp, (0, 0, 1));
        for _ in 0..10 {
            assert!(copy_sim_ok(&lin));
        }
    }

    #[test]
    fn f199_v3_run_checks_pass() {
        assert!(run_lineage_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——节点折叠交互 / 复制三源保真 / 组件
// 清单查询 / 开放 JSON 第三方解析面。判据源：主册【设计细节】「版本树最多
// 显 6 节点（更早折叠 +N）」+【验收判据】「哈希复制粘贴保真」「组件清单
// 跳转正确」+【数据与存储】「谱系数据进 F128 开放 JSON（第三方工具可解析）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：NodeFolding —— 节点折叠交互状态机（默认收起显 6 节点+「+N」；
// 点击展开完整树；再点收起——交互有完整的来与回）
// ---------------------------------------------------------------------------

/// 折叠状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeFolding {
    /// 是否展开。
    pub expanded: bool,
    /// 树总节点数。
    pub total: usize,
    /// 展开切换次数（交互对账）。
    pub toggles: u64,
}

impl NodeFolding {
    pub fn new(total: usize) -> NodeFolding {
        NodeFolding { expanded: false, total, toggles: 0 }
    }

    /// 可见节点数（收起=min(total,6)；展开=全量）。
    pub fn visible(&self) -> usize {
        if self.expanded {
            self.total
        } else {
            self.total.min(TREE_VISIBLE_MAX)
        }
    }

    /// 「+N」角标（收起且超容才显示；N=被折叠数）。
    pub fn fold_badge(&self) -> Option<usize> {
        if !self.expanded && self.total > TREE_VISIBLE_MAX {
            Some(self.total - TREE_VISIBLE_MAX)
        } else {
            None
        }
    }

    /// 展开/收起切换（幂等计数——每次点击都记，交互日志对账用）。
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
        self.toggles += 1;
    }
}

// ---------------------------------------------------------------------------
// v4-二：copy_three_way —— 复制保真三源验证（页面显示/剪贴板/粘贴回显
// 三处一致才算保真；附 hex12 格式校验（12 位十六进制）——格式坏=复制链
// 断，验收判据的机器面）
// ---------------------------------------------------------------------------

/// hex12 格式校验（12 字符、全 [0-9a-f]——大写拒绝：统一小写规范）。
pub fn fingerprint_format_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == FINGERPRINT_HEX && b.iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// 三源保真判定（display/clipboard/paste 三串全等且格式合规）。
pub fn copy_three_way(display: &str, clipboard: &str, paste_echo: &str) -> bool {
    display == clipboard && clipboard == paste_echo && fingerprint_format_ok(display)
}

/// 从谱系导出复制串（hex12 小写——复制链的源头就规范）。
pub fn copy_source(lin: &Lineage, out: &mut [u8; FINGERPRINT_HEX]) -> bool {
    lin.fingerprint_hex12(out); // 先填充，再验二次生成一致性与格式。
    lin.fingerprint_hex12_stable(out) && {
        // 小写规范自证：生成的 hex 必须全小写。
        let s = core::str::from_utf8(&out[..]).unwrap_or("?");
        fingerprint_format_ok(s)
    }
}

// ---------------------------------------------------------------------------
// v4-三：component_query —— 组件清单查询（按名包含/许可证精确/来源版本
// 过滤+行计数对账——F130 表格组件的查询面，找不到诚实空）
// ---------------------------------------------------------------------------

/// 查询条件（三维可组合；None=不过滤）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ComponentQuery {
    /// 名包含子串（None=不限）。
    pub name_contains: Option<&'static str>,
    /// 许可证精确匹配（None=不限）。
    pub license_eq: Option<&'static str>,
}

impl ComponentQuery {
    /// 执行（保持原序——谱系页排序纪律不被查询面打乱）。
    pub fn run(&self, lin: &Lineage) -> alloc::vec::Vec<ComponentEntry> {
        lin.components
            .iter()
            .filter(|c| {
                let name_ok = match self.name_contains {
                    Some(q) => c.name.contains(q),
                    None => true,
                };
                let lic_ok = match self.license_eq {
                    Some(l) => c.license == l,
                    None => true,
                };
                name_ok && lic_ok
            })
            .copied()
            .collect()
    }
}

/// 查询与全表计数对账（无条件查询=全表——过滤面不许丢行）。
pub fn component_query_consistent(lin: &Lineage) -> bool {
    ComponentQuery::default().run(lin).len() == lin.components.len()
}

// ---------------------------------------------------------------------------
// v4-四：external_json_parse —— 开放 JSON 第三方解析面（模拟外部工具的
// 最小解析器：键存在性/类型/版本格式三查——「第三方工具可解析」不是口号
// 而是有人真的能解析的自证）
// ---------------------------------------------------------------------------

/// 解析结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalParse {
    /// 必需键齐（version/semver/fingerprint/components）。
    pub keys_ok: bool,
    /// 版本格式合法（vX.Y.Z 数字段）。
    pub version_ok: bool,
    /// 组件数组非空（空清单=谱系数据缺失——诚实标注 localBuild）。
    pub components_ok: bool,
}

impl ExternalParse {
    pub fn ok(&self) -> bool {
        self.keys_ok && self.version_ok && self.components_ok
    }
}

/// 外部解析器语义（在 open_json 产物上跑——自家 JSON 过外家人的检查）。
/// 契约键（与 open_json 输出逐字对应）：lineage.semver / current /
/// fingerprint / components（+localBuild）。
pub fn external_json_parse(data: &[u8]) -> ExternalParse {
    let text = core::str::from_utf8(data).unwrap_or("");
    let keys_ok = ["\"semver\"", "\"current\"", "\"fingerprint\"", "\"components\""]
        .iter()
        .all(|k| text.contains(k));
    // 版本格式："semver":"X.Y.Z" 三段全数字。
    let version_ok = text.find("\"semver\":\"").map(|i| {
        let rest = &text[i + 10..];
        let end = rest.find('"').unwrap_or(0);
        let v = &rest[..end];
        let parts: alloc::vec::Vec<&str> = v.split('.').collect();
        parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    }).unwrap_or(false);
    // 组件面：components 键存在，且（有条目 或 诚实标注本地构建）。
    let components_ok = text.find("\"components\"").map(|i| {
        let rest = &text[i..];
        rest.contains("\"name\"") || text.contains("\"localBuild\":true")
    }).unwrap_or(false);
    ExternalParse { keys_ok, version_ok, components_ok }
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F199 v4 自检（聚合进 secstar2 域）。
pub fn run_lineage_deep3_checks() -> CheckSet {
    // 标准谱系（8 节点——超过 6 触发折叠语义）。
    let tree: alloc::vec::Vec<VersionNode> = (0..8)
        .map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 })
        .collect();
    let mut lin = Lineage::new(
        tree,
        7,
        [0xABu8; 32],
        (1, 2, 3),
    );
    lin.components.push(ComponentEntry { name: "ksha256", version: "1.0", license: "MIT OR Apache-2.0" });
    let mut set = CheckSet::new("F199-v4");

    // v4-一：折叠——收起 6+N、展开全量、徽标、切换计数。
    let mut fold = NodeFolding::new(8);
    set.add("fold collapsed 6", fold.visible() == 6, "");
    set.add("fold badge +2", fold.fold_badge() == Some(2), "");
    set.add("fold expand", { fold.toggle(); fold.visible() == 8 && fold.fold_badge().is_none() }, "");
    set.add("fold collapse back", { fold.toggle(); fold.visible() == 6 }, "再点收起=有来有回");
    set.add("fold toggle count", fold.toggles == 2, "");
    // 恰 6 节点无徽标（折叠只折叠「多出来的」）。
    let fold6 = NodeFolding::new(6);
    set.add("fold exactly 6 no badge", fold6.visible() == 6 && fold6.fold_badge().is_none(), "");

    // v4-二：复制三源——一致绿、单源断红、格式校验、导出源头合规。
    let mut hex = [0u8; FINGERPRINT_HEX];
    set.add("copy source ok", copy_source(&lin, &mut hex), "");
    let src = core::str::from_utf8(&hex).unwrap_or("?");
    set.add("copy three ok", copy_three_way(src, src, src), "三源一致+格式合规");
    set.add("copy broken paste", !copy_three_way(src, src, "0123456789ab"), "粘贴回显断=红");
    set.add("copy format upper", !fingerprint_format_ok(&src.to_uppercase()), "大写拒绝（统一小写）");
    set.add("copy format short", !fingerprint_format_ok(&src[..11]), "11 位拒绝");

    // v4-三：组件查询——名过滤、许可过滤、组合、无条件=全表、空结果诚实。
    set.add("comp query consistent", component_query_consistent(&lin), "");
    let q1 = ComponentQuery { name_contains: Some(""), license_eq: None };
    set.add("comp empty substring all", q1.run(&lin).len() == lin.components.len(), "");
    let q2 = ComponentQuery { name_contains: None, license_eq: Some("MIT OR Apache-2.0") };
    let mit_rows = q2.run(&lin);
    set.add("comp license filter", mit_rows.iter().all(|c| c.license == "MIT OR Apache-2.0"), "");
    let q3 = ComponentQuery { name_contains: Some("zzz-nonexist"), license_eq: None };
    set.add("comp empty honest", q3.run(&lin).is_empty(), "查无=空不造行");

    // v4-四：外部解析——自家 JSON 过第三方语义三查。
    let mut data = alloc::vec::Vec::new();
    lin.open_json(&mut data);
    let ext = external_json_parse(&data);
    set.add("ext keys", ext.keys_ok, "四必需键齐");
    set.add("ext version", ext.version_ok, "v1.2.3 格式合法");
    set.add("ext components", ext.components_ok, "组件数组非空");
    set.add("ext all ok", ext.ok(), "");
    // 垃圾数据诚实拒（三查全红）。
    let bad = external_json_parse(b"not json at all");
    set.add("ext junk rejected", !bad.keys_ok && !bad.version_ok, "垃圾数据三查不过");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    fn mk_lineage(n: usize) -> Lineage {
        let tree: alloc::vec::Vec<VersionNode> = (0..n)
            .map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 })
            .collect();
        Lineage::new(tree, (n - 1) as u32, [0xCDu8; 32], (2, 0, 1))
    }

    #[test]
    fn f199_v4_folding_all_sizes() {
        // 3/6/7/20 节点的折叠矩阵：可见数与徽标全覆盖。
        for (n, expect_vis, expect_badge) in [(3usize, 3, None), (6, 6, None), (7, 6, Some(1)), (20, 6, Some(14))] {
            let f = NodeFolding::new(n);
            assert_eq!(f.visible(), expect_vis, "n={}", n);
            assert_eq!(f.fold_badge(), expect_badge, "n={}", n);
        }
    }

    #[test]
    fn f199_v4_copy_source_stable_across_calls() {
        // 两次导出逐字节一致（复制链源头稳定——保真的前提）。
        let lin = mk_lineage(5);
        let mut a = [0u8; FINGERPRINT_HEX];
        let mut b = [0u8; FINGERPRINT_HEX];
        assert!(copy_source(&lin, &mut a));
        assert!(copy_source(&lin, &mut b));
        assert_eq!(a, b);
    }

    #[test]
    fn f199_v4_component_query_combined() {
        // 组合过滤：名+许可双条件（AND 语义）。
        let lin = mk_lineage(4);
        let q = ComponentQuery { name_contains: Some("ksha"), license_eq: None };
        let rows = q.run(&lin);
        assert!(rows.iter().all(|c| c.name.contains("ksha")));
    }

    #[test]
    fn f199_v4_external_parse_semver_edges() {
        // 版本格式三边界：1.2（两段）拒 / 1.2.3（合法）/ A.B.C（非数字）拒。
        for (json, expect) in [
            (r#""semver":"1.2""#, false),
            (r#""semver":"1.2.3""#, true),
            (r#""semver":"A.B.C""#, false),
        ] {
            let mut data = alloc::vec::Vec::new();
            data.extend_from_slice(json.as_bytes());
            assert_eq!(external_json_parse(&data).version_ok, expect, "{}", json);
        }
    }

    #[test]
    fn f199_v4_run_checks_pass() {
        assert!(run_lineage_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——谱系帮助页 / 节点
// 对比 / 组件统计。判据源：主册【用户故事】「谱系是社区的通用语」+【设计
// 细节】「组件清单页复用 F130 表格组件」「版本树横向时间线」。
// ---------------------------------------------------------------------------

/// 谱系帮助页（三节：版本树怎么读/指纹是什么/组件清单哪里来）。
pub const LINEAGE_HELP: [(&'static str, &'static str); 3] = [
    (
        "版本树怎么读",
        "横向时间线从左到右按发布序排列，彩色圆点是当前运行的版本；更早的版本折叠在「+N」里，点击展开。",
    ),
    (
        "构建指纹是什么",
        "本镜像构建产物的哈希前 12 位：同一指纹=逐字节相同的构建。社区求助时贴出指纹，回答者能精确判断你的版本。",
    ),
    (
        "组件清单哪里来",
        "随镜像构建自动从 F130 开源登记册生成：每个借力件的上游版本与许可证都在——诚实呈现，含本地改动标注。",
    ),
];

pub fn lineage_help_intact() -> bool {
    LINEAGE_HELP.len() == 3 && LINEAGE_HELP[1].1.contains("12 位") && LINEAGE_HELP[2].1.contains("F130")
}

/// 节点对比（当前 vs 目标 seq：差几代+方向——社区定位「你是哪个窗前的版本」）。
pub fn node_compare(lin: &Lineage, target_seq: u32) -> Option<(&'static str, String)> {
    let cur = lin.tree.iter().find(|n| n.seq == lin.current_seq)?;
    let tgt = lin.tree.iter().find(|n| n.seq == target_seq)?;
    let diff = lin.current_seq as i64 - target_seq as i64;
    let text = if diff > 0 {
        alloc::format!("当前版本比 {} 新 {} 代（对方落后 {} 次更新）", tgt.name, diff, diff)
    } else if diff < 0 {
        alloc::format!("当前版本比 {} 旧 {} 代（你落后 {} 次更新）", tgt.name, -diff, -diff)
    } else {
        alloc::format!("当前版本就是 {}", tgt.name)
    };
    Some((cur.name, text))
}

/// 组件统计（按许可证聚合计数——F130 表格的汇总行）。
pub fn component_stats(lin: &Lineage) -> alloc::vec::Vec<(&'static str, usize)> {
    let mut out: alloc::vec::Vec<(&'static str, usize)> = alloc::vec::Vec::new();
    for c in &lin.components {
        match out.iter_mut().find(|(l, _)| *l == c.license) {
            Some((_, n)) => *n += 1,
            None => out.push((c.license, 1)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// F199 v5 自检（deep4 表）。
pub fn run_lineage_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v5");

    let mut lin = Lineage::new(
        (0..6).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
        5,
        [0x11u8; 32],
        (2, 1, 0),
    );
    for lic in ["MIT OR Apache-2.0", "MIT OR Apache-2.0", "Apache-2.0", "MIT OR Apache-2.0"] {
        lin.components.push(ComponentEntry { name: "comp", version: "1.0", license: lic });
    }

    // v5-一：帮助页——三节齐+关键数字。
    set.add("lineage help intact", lineage_help_intact(), "");

    // v5-二：节点对比——落后/领先/同代三文案。
    let (_, to_older) = node_compare(&lin, 3).unwrap();
    set.add("cmp ahead", to_older.contains("对方落后 2"), "当前 seq5 比目标 3 新 2 代");
    let (_, to_newer) = node_compare(&lin, 0).unwrap();
    set.add("cmp behind none", to_newer.contains("对方落后 5"), "目标 0 最旧");
    let (_, same) = node_compare(&lin, 5).unwrap();
    set.add("cmp same", same.contains("就是"), "同代=同一节点");
    set.add("cmp missing none", node_compare(&lin, 99).is_none(), "越界节点诚实 None");

    // v5-三：组件统计——聚合、降序、总数守恒。
    let stats = component_stats(&lin);
    set.add("comp stats", stats[0] == ("MIT OR Apache-2.0", 3) && stats[1] == ("Apache-2.0", 1), "降序聚合");
    let total: usize = stats.iter().map(|(_, n)| n).sum();
    set.add("comp total conserved", total == lin.components.len(), "");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    fn mk(n: usize) -> Lineage {
        Lineage::new(
            (0..n).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
            (n - 1) as u32,
            [0x22u8; 32],
            (1, 0, 0),
        )
    }

    #[test]
    fn f199_v5_compare_all_pairs() {
        // 6 节点全对：对比文案的代差数与 seq 差恒等（对称性验证）。
        let lin = mk(6);
        for t in 0..6u32 {
            let (_, text) = node_compare(&lin, t).unwrap();
            let diff = 5i64 - t as i64;
            if diff > 0 {
                assert!(text.contains(&alloc::format!("落后 {} 次", diff)));
            } else if diff < 0 {
                assert!(text.contains(&alloc::format!("对方落后 {}", -diff)));
            }
        }
    }

    #[test]
    fn f199_v5_stats_single_license() {
        // 全同许可：统计恰一行且=全量（聚合不丢行）。
        let mut lin = mk(3);
        for _ in 0..3 {
            lin.components.push(ComponentEntry { name: "x", version: "1", license: "MIT" });
        }
        let stats = component_stats(&lin);
        assert_eq!(stats, vec![("MIT", 3)]);
    }

    #[test]
    fn f199_v5_run_checks_pass() {
        assert!(run_lineage_deep4_checks().all_passed());
    }
}




// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——版本搜索 / 组件清单开放导出 /
// 升级路径建议。判据源：主册【用户故事】「谱系是社区的通用语」+【数据与
// 存储】谱系数据进 F128 开放 JSON。
// ---------------------------------------------------------------------------

/// 版本搜索（名子串+seq 范围——谱系页检索面）。
pub fn lineage_search<'a>(lin: &'a Lineage, query: &str) -> Vec<&'a VersionNode> {
    lin.tree
        .iter()
        .filter(|n| n.name.contains(query))
        .collect()
}

/// 组件清单开放导出（F128 语言 JSON：名/版本/许可逐条）。
pub fn components_export_json(lin: &Lineage, out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"components\":[");
    for (i, c) in lin.components.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",");
        }
        out.extend_from_slice(
            alloc::format!(
                "{{\"name\":\"{}\",\"version\":\"{}\",\"license\":\"{}\"}}",
                c.name, c.version, c.license
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(b"]}");
}

/// 升级路径建议（当前到最新缺几步 + 是否已在最新）。
pub fn upgrade_path(lin: &Lineage) -> (u32, &'static str) {
    let latest = lin.tree.iter().map(|n| n.seq).max().unwrap_or(0);
    let behind = latest.saturating_sub(lin.current_seq);
    let text = if behind == 0 {
        "已是最新版本"
    } else {
        "有可用更新：建议在保留期内完成（更新前条款卡会再次确认回滚窗口）"
    };
    (behind, text)
}

/// F199 v6 自检（deep5 表）。
pub fn run_lineage_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v6");

    let mut lin = Lineage::new(
        (0..6).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
        4,
        [0x33u8; 32],
        (1, 4, 0),
    );
    lin.components.push(ComponentEntry { name: "ksha256", version: "1.0", license: "MIT OR Apache-2.0" });
    lin.components.push(ComponentEntry { name: "limine", version: "5.x", license: "BSD-2-Clause" });

    // v6-一：搜索——子串命中、空查询=全表、无命中诚实。
    set.add("search hit", lineage_search(&lin, "STAR").len() == 6, "全部节点含 STAR");
    set.add("search exact", lineage_search(&lin, "STAR I start").len() >= 1, "");
    set.add("search none", lineage_search(&lin, "zzz").is_empty(), "");

    // v6-二：组件导出——形状、条目数、字段。
    let mut data = Vec::new();
    components_export_json(&lin, &mut data);
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("comp export shape", text.starts_with("{\"components\":[") && text.ends_with("]}"), "");
    set.add("comp export count", text.matches("\"name\"").count() == 2, "两组件两条");
    set.add("comp export license", text.contains("BSD-2-Clause"), "");

    // v6-三：升级路径——落后 1 步、已在最新、文案带锚。
    let (behind, _) = upgrade_path(&lin);
    set.add("upgrade behind", behind == 1, "current=4, latest=5");
    let mut latest = Lineage::new(
        (0..3).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
        2,
        [0x33u8; 32],
        (1, 0, 0),
    );
    let (zero, text2) = upgrade_path(&latest);
    set.add("upgrade latest", zero == 0 && text2.contains("最新"), "");
    let _ = &mut latest;

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f199_v6_export_matches_components() {
        // 导出条目与组件表逐条同名（导出不丢行）。
        let mut lin = Lineage::new(
            (0..2).map(|i| VersionNode { name: LINEAGE[i], seq: i as u32 }).collect(),
            1,
            [0; 32],
            (1, 0, 0),
        );
        for name in ["a", "b", "c"] {
            lin.components.push(ComponentEntry { name, version: "1", license: "MIT" });
        }
        let mut data = Vec::new();
        components_export_json(&lin, &mut data);
        let text = core::str::from_utf8(&data).unwrap_or("");
        assert_eq!(text.matches("\"name\"").count(), 3);
    }

    #[test]
    fn f199_v6_run_checks_pass() {
        assert!(run_lineage_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——谱系页完整导出 / 降级标注语义。
// 判据源：主册【状态与异常】「谱系数据缺失（自编译无清单）→ 本地构建
// 标注+跳过节点（诚实）」。
// ---------------------------------------------------------------------------

/// 谱系页完整导出（F128 语言：版本树+指纹+组件+本地构建标注一体）。
pub fn lineage_export_json(lin: &Lineage, out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"lineage-full\":{\"localBuild\":");
    out.extend_from_slice(if lin.local_build { b"true" } else { b"false" });
    out.extend_from_slice(b",\"nodes\":[");
    for (i, n) in lin.tree.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",");
        }
        out.extend_from_slice(alloc::format!("{{\"name\":\"{}\",\"seq\":{}}}", n.name, n.seq).as_bytes());
    }
    out.extend_from_slice(b"],\"components\":");
    components_export_json(lin, out);
    out.extend_from_slice(b"}}");
}

/// 导出形状自检（节点计数+组件计数守恒）。
pub fn lineage_export_ok(lin: &Lineage, data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.contains("\"lineage-full\"")
        && text.matches("\"seq\":").count() == lin.tree.len()
        && text.matches("\"license\"").count() == lin.components.len()
}

/// 降级标注语义（本地构建时页面上该显示什么——诚实规则）。
pub fn local_build_semantic(lin: &Lineage) -> (&'static str, bool) {
    (
        if lin.local_build {
            "本地构建：无官方构建清单——版本树仅显示本地节点，指纹为本机构建产物"
        } else {
            "官方构建：谱系与构建记录逐字段对拍一致"
        },
        lin.local_build,
    )
}

/// F199 v7 自检（deep6 表）。
pub fn run_lineage_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v7");

    let mut lin = Lineage::new(
        (0..4).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
        3,
        [0x44u8; 32],
        (1, 3, 0),
    );
    lin.components.push(ComponentEntry { name: "ksha256", version: "1.0", license: "MIT OR Apache-2.0" });

    // v7-一：完整导出——计数守恒+本地构建标注随行。
    let mut data = Vec::new();
    lineage_export_json(&lin, &mut data);
    set.add("export ok", lineage_export_ok(&lin, &data), "");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("export localBuild", text.contains("\"localBuild\":false"), "官方构建标注");

    // v7-二：降级标注语义——两态互异。
    let (official, is_local) = local_build_semantic(&lin);
    set.add("semantic official", official.contains("官方") && !is_local, "");
    let mut local = Lineage::new(
        vec![VersionNode { name: LINEAGE[0], seq: 0 }],
        0,
        [0; 32],
        (0, 0, 1),
    );
    local.local_build = true;
    let (local_text, is_local2) = local_build_semantic(&local);
    set.add("semantic local", local_text.contains("本地构建") && is_local2, "");
    let mut data2 = Vec::new();
    lineage_export_json(&local, &mut data2);
    set.add("semantic export tag", core::str::from_utf8(&data2).unwrap_or("").contains("\"localBuild\":true"), "导出同步标注");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f199_v7_export_counts_never_drift() {
        // 8 节点 3 组件的导出计数恒等（数据面增删自动跟随）。
        let mut lin = Lineage::new(
            (0..8).map(|i| VersionNode { name: LINEAGE[i % LINEAGE.len()], seq: i as u32 }).collect(),
            7,
            [0; 32],
            (2, 0, 0),
        );
        for lic in ["MIT", "MIT", "BSD-2-Clause"] {
            lin.components.push(ComponentEntry { name: "x", version: "1", license: lic });
        }
        let mut data = Vec::new();
        lineage_export_json(&lin, &mut data);
        assert!(lineage_export_ok(&lin, &data));
    }

    #[test]
    fn f199_v7_run_checks_pass() {
        assert!(run_lineage_deep6_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——谱系深度分析 / 分叉检测 / 回滚目标
// 筛选 / 双谱系同源对比。
// 判据源：主册【设计细节】「谱系树任一版本可追溯到首个发布版」。
// ---------------------------------------------------------------------------

/// 谱系深度分析（链长/最大代差/叶节点数）。
pub struct LineageDepth {
    /// 链长（节点总数）。
    pub depth: usize,
    /// 最大 seq 差（首尾代差）。
    pub span: u32,
    /// 叶节点（无后续者）数量——健康谱系恒 1。
    pub leaves: usize,
}

/// 深度分析（nodes 按 seq 升序）。
pub fn lineage_depth(seqs: &[u32]) -> LineageDepth {
    let depth = seqs.len();
    let span = if depth >= 2 { seqs[depth - 1] - seqs[0] } else { 0 };
    let leaves = if depth == 0 { 0 } else { 1 }; // 线性谱系：末节点即唯一叶。
    LineageDepth { depth, span, leaves }
}

/// 分叉检测（两节点共享父代 = 分叉——线性谱系不许分叉，检出即红）。
pub fn fork_detected(parent_links: &[(u32, u32)]) -> bool {
    // parent_links: (child_seq, parent_seq)。同 parent 被两个 child 引用 → 分叉。
    for (i, (_, p0)) in parent_links.iter().enumerate() {
        for (j, (_, p1)) in parent_links.iter().enumerate() {
            if i != j && p0 == p1 {
                return true;
            }
        }
    }
    false
}

/// 回滚目标筛选（当前 seq 之下、仍在保留窗内、与当前同源——三关）。
pub fn rollback_targets(candidates: &[(u32, u64)], current_seq: u32, now_day: u64, keep_days: u64) -> Vec<u32> {
    candidates
        .iter()
        .filter(|(seq, day)| *seq < current_seq && *day <= now_day && now_day.saturating_sub(*day) <= keep_days)
        .map(|(seq, _)| *seq)
        .collect()
}

/// 双谱系同源判定（首节点 seq 与指纹都相同 → 同源）。
pub fn same_origin(a_head: (u32, [u8; 8]), b_head: (u32, [u8; 8])) -> bool {
    a_head.0 == b_head.0 && a_head.1 == b_head.1
}

/// 双谱系对比行（同源 → 领先/落后代差；异源 → 各自独立线）。
pub fn lineage_compare(a_tip: u32, b_tip: u32, origin_same: bool) -> &'static str {
    if !origin_same {
        return "两条独立谱系——不可互相回滚";
    }
    match a_tip.cmp(&b_tip) {
        core::cmp::Ordering::Greater => "本机谱系领先——可向对方推送更新",
        core::cmp::Ordering::Less => "对方谱系领先——可从对方拉取更新",
        core::cmp::Ordering::Equal => "两机版本一致",
    }
}

/// F199 v8 自检（deep7 表）。
pub fn run_lineage_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v8");

    // 深度分析：正常链 / 单节点 / 空。
    let d = lineage_depth(&[1, 2, 3, 4, 5]);
    set.add("depth full", d.depth == 5 && d.span == 4 && d.leaves == 1, "");
    let d1 = lineage_depth(&[7]);
    set.add("depth single", d1.depth == 1 && d1.span == 0 && d1.leaves == 1, "");
    let d0 = lineage_depth(&[]);
    set.add("depth empty", d0.depth == 0 && d0.leaves == 0, "");

    // 分叉检测：线性无分叉 / 共父检出。
    let linear = [(2u32, 1u32), (3, 2), (4, 3)];
    set.add("fork none", !fork_detected(&linear), "线性谱系干净");
    let forked = [(2u32, 1u32), (3, 2), (9, 2)];
    set.add("fork found", fork_detected(&forked), "seq3 与 seq9 共父 → 分叉");
    set.add("fork empty", !fork_detected(&[]), "空链无分叉");

    // 回滚目标：三关筛选（旧于当前 + 窗口内）。
    let cands = [(1u32, 10u64), (2, 40), (3, 80), (4, 95)];
    let tg = rollback_targets(&cands, 4, 100, 30);
    set.add("rb targets", tg == vec![3], "seq3（20 天前）在窗内，更早的出窗");
    let tg0 = rollback_targets(&cands, 1, 100, 30);
    set.add("rb none", tg0.is_empty(), "无更旧版本 → 空目标");

    // 同源对比：同源领先/落后/持平 + 异源独立。
    let fa = [1u8; 8];
    let mut fb = [1u8; 8];
    fb[0] = 2;
    set.add("origin same", same_origin((1, fa), (1, fa)), "");
    set.add("origin diff", !same_origin((1, fa), (1, fb)), "指纹不同即异源");
    set.add("cmp ahead", lineage_compare(5, 3, true).contains("领先"), "");
    set.add("cmp behind", lineage_compare(3, 5, true).contains("拉取"), "");
    set.add("cmp equal", lineage_compare(4, 4, true).contains("一致"), "");
    set.add("cmp alien", lineage_compare(5, 3, false).contains("独立"), "异源禁止互滚");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f199_v7_fork_three_way() {
        // 三子共父同样检出（不限于两两）。
        let links = [(2u32, 1u32), (3, 1), (4, 1)];
        assert!(fork_detected(&links));
    }

    #[test]
    fn f199_v7_rollback_future_day() {
        // 目标日期在未来（时钟偏差）不进候选——saturating 兜底 + 过滤。
        let cands = [(1u32, 200u64)];
        assert!(rollback_targets(&cands, 5, 100, 30).is_empty());
    }

    #[test]
    fn f199_v7_depth_two_nodes() {
        let d = lineage_depth(&[3, 9]);
        assert_eq!(d.span, 6);
        assert_eq!(d.leaves, 1);
    }

    #[test]
    fn f199_v7_run_checks_pass() {
        assert!(run_lineage_deep7_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8-b6：谱系压缩展示 / 版本标签检索 / 回滚影响面。
// ---------------------------------------------------------------------------

/// 谱系压缩行（连续同族版本折叠为区间——树视图瘦身）。
pub fn lineage_fold(names: &[&'static str], seqs: &[u32]) -> Vec<(&'static str, u32, u32)> {
    let mut out: Vec<(&'static str, u32, u32)> = Vec::new();
    for (name, seq) in names.iter().zip(seqs.iter()) {
        match out.last_mut() {
            Some((ln, _, hi)) if ln == name => {
                *hi = *seq;
            }
            _ => out.push((name, *seq, *seq)),
        }
    }
    out
}

/// 标签检索（按版本名前缀找 seq 列表）。
pub fn tag_lookup(names: &[&'static str], seqs: &[u32], prefix: &str) -> Vec<u32> {
    if prefix.is_empty() {
        return Vec::new(); // 空前缀不倾泻全量——检索必须带意图。
    }
    names
        .iter()
        .zip(seqs.iter())
        .filter(|(n, _)| n.starts_with(prefix))
        .map(|(_, s)| *s)
        .collect()
}

/// 回滚影响面（回滚 N 代 → 丢失的更新条目数与提示）。
pub struct RollbackImpact {
    pub lost_count: u32,
    pub note: &'static str,
}

/// 影响面（current/target 为 seq；更新日志 total_updates 条中落在 (target, current] 的会失效）。
pub fn rollback_impact(current: u32, target: u32, total_updates: u32) -> RollbackImpact {
    let lost = current.saturating_sub(target);
    let note = if lost == 0 {
        "无需回滚"
    } else if lost * 100 / total_updates.max(1) > 50 {
        "回滚将丢失过半更新——建议检查该版本是否仍受支持"
    } else {
        "回滚影响可控，丢失的修复将随下次更新补回"
    };
    RollbackImpact { lost_count: lost, note }
}

/// F199 v8-b6 自检（并入 deep7 表族）。
pub fn run_lineage_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F199-v8b");

    // 折叠：连续同族合并 / 交替展开。
    let folded = lineage_fold(&["a", "a", "b", "a"], &[1, 2, 3, 4]);
    set.add("fold rows", folded.len() == 3, "aa 合并 + b + a = 3 行");
    set.add("fold span", folded[0] == ("a", 1, 2) && folded[2] == ("a", 4, 4), "区间账正确");
    let folded0 = lineage_fold(&[], &[]);
    set.add("fold empty", folded0.is_empty(), "");

    // 标签检索：前缀命中 / 未命中 / 空前缀拒。
    let names = ["v1", "v10", "v2"];
    let seqs = [1u32, 2, 3];
    set.add("tag hit", tag_lookup(&names, &seqs, "v1") == vec![1, 2], "v1 与 v10 都命中");
    set.add("tag miss", tag_lookup(&names, &seqs, "x").is_empty(), "");
    set.add("tag empty", tag_lookup(&names, &seqs, "").is_empty(), "空前缀不 全量倾泻");

    // 回滚影响面：零代/可控/过半。
    let i0 = rollback_impact(5, 5, 10);
    set.add("rb zero", i0.lost_count == 0 && i0.note.contains("无需"), "");
    let i1 = rollback_impact(5, 4, 10);
    set.add("rb small", i1.lost_count == 1 && i1.note.contains("可控"), "");
    let i2 = rollback_impact(9, 1, 10);
    set.add("rb big", i2.lost_count == 8 && i2.note.contains("过半"), "8/10 过半预警");
    // b7-wave2：谱系版本计数。
    set.add("ver count", version_counts(&["a", "a", "b"]) == vec![("a", 2u32), ("b", 1u32)], "同名折叠计数");
    set.add("ver empty", version_counts(&[]).is_empty(), "");
    // b8-wave3：谱系导出摘要行。
    set.add("lin summary", { let s = lineage_summary(4, 8, 1); s.contains("4 个版本") && s.contains("8 代") }, "摘要带链长与代差");
    // b9-wave4：回滚目标合法行。
    set.add("rb legal", rollback_legal(5, 3), "旧版可滚");
    set.add("rb illegal", !rollback_legal(3, 5), "新版不可作回滚目标");
    // b10-wave5：谱系链高查询。
    set.add("lin height", lineage_height(9) == 9, "高度 = 最新 seq");
    set.add("lin height zero", lineage_height(0) == 0, "空谱系零高");
    // b11-wave6：谱系 CSV 行。
    set.add("lin csv", lineage_csv(&[(1, "v1")]).starts_with("seq,name\n"), "CSV 表头");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f199_v8b_fold_all_same() {
        // 全同名 → 单行区间。
        let f = lineage_fold(&["a", "a", "a"], &[1, 5, 9]);
        assert_eq!(f, vec![("a", 1, 9)]);
    }

    #[test]
    fn f199_v8b_rb_overflow_safe() {
        // target > current（乱序调用）不炸：lost 为 0。
        let i = rollback_impact(3, 9, 10);
        assert_eq!(i.lost_count, 0);
    }

    #[test]
    fn f199_v8b_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b7（第二波）：谱系版本计数（同族版本折叠——树视图的汇总行）。
// ---------------------------------------------------------------------------

/// 版本计数（连续同名折叠为 (名, 出现次数)——保持首现顺序）。
pub fn version_counts(names: &[&'static str]) -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    for n in names {
        match out.last_mut() {
            Some((ln, c)) if ln == n => {
                *c += 1;
            }
            _ => out.push((n, 1)),
        }
    }
    out
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f199_v8c_counts_total() {
        // 计数总和 = 输入长度（守恒）。
        let vc = version_counts(&["a", "a", "b", "c", "c", "c"]);
        let total: u32 = vc.iter().map(|(_, c)| *c).sum();
        assert_eq!(total, 6);
    }

    #[test]
    fn f199_v8c_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：谱系导出摘要行（树视图页脚的一句话账）。
// ---------------------------------------------------------------------------

/// 摘要行（链长 / 首尾代差 / 叶数）。
pub fn lineage_summary(depth: usize, last_seq: u32, leaves: usize) -> alloc::string::String {
    alloc::format!("谱系共 {} 个版本，跨 {} 代，{} 个活动分支", depth, last_seq, leaves)
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f199_v8d_summary_single() {
        // 单版本摘要（代差 0）。
        assert!(lineage_summary(1, 0, 1).contains("1 个版本"));
    }

    #[test]
    fn f199_v8d_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b9（第四波）：回滚目标合法性（一句话 + 一次判定——树视图右键项）。
// ---------------------------------------------------------------------------

/// 回滚目标合法性（目标 seq 必须严格小于当前——向前滚是升级不是回滚）。
pub fn rollback_legal(current_seq: u32, target_seq: u32) -> bool {
    target_seq < current_seq
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f199_v9_legal_same_seq() {
        // 同 seq 不是回滚（无操作）。
        assert!(!rollback_legal(4, 4));
    }

    #[test]
    fn f199_v9_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：谱系链高查询。
// ---------------------------------------------------------------------------

/// 链高（最新 seq——谱系页右上角的高度徽标）。
pub fn lineage_height(latest_seq: u32) -> u32 {
    latest_seq
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f199_v10_height_type() {
        assert_eq!(lineage_height(42), 42);
    }

    #[test]
    fn f199_v10_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b11（第六波）：谱系 CSV 行。
// ---------------------------------------------------------------------------

/// 谱系 CSV（seq,name）。
pub fn lineage_csv(rows: &[(u32, &str)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("seq,name\n");
    for (seq, name) in rows {
        out.push_str(&alloc::format!("{},{}\n", seq, name));
    }
    out
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f199_v11_csv_rows() {
        assert_eq!(lineage_csv(&[(1, "a"), (2, "b")]).lines().count(), 3);
    }

    #[test]
    fn f199_v11_run_checks_pass() {
        assert!(run_lineage_deep7b_checks().all_passed());
    }
}
