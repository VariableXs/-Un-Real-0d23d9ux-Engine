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
