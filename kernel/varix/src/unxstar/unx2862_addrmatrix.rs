// unx2862 —— UNX-F2862 地址空间回归矩阵冻结（十面压测→用例映射 30 例）
// 判据原文 UNX-F2862-J1：回归用例矩阵（十面压测→用例映射 30 例）冻结哈希入册，
// 矩阵字段完整性零缺失。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF980 独占（0xF980~0xF9FF）。
//
// 设计（纯功能 no_std）：
//   * 十面压测闭集：栈/堆/共享/文件映射/guard/对齐/边界/权限/碎片/重映射——
//     每面挂 3 条用例（面 ID/用例键/期望三字段），共 30 例；
//   * 矩阵字段完整性：每行四字段（面/键/期望/优先级）逐行扫描零缺失；
//   * 冻结哈希入册：矩阵全量 FNV-1a 32 位指纹一次冻结，复算恒等；
//   * 注入核：缺字段/缺面/缺例/哈希漂移四类注入必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF980 独占） ─────────────────────────────────────────────
pub const XC_MTX_FIELD: u32 = 0xF981; // 矩阵字段缺失
pub const XC_MTX_FACE: u32 = 0xF982; // 压测面缺失
pub const XC_MTX_CASE: u32 = 0xF983; // 用例缺失
pub const XC_MTX_HASH: u32 = 0xF984; // 指纹漂移
pub const XC_MTX_DUP: u32 = 0xF985; // 用例键重复

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

// ── 十面压测闭集 ──────────────────────────────────────────────────────
pub const FACE_NAMES: [&str; 10] = [
    "stack", "heap", "shared", "filemap", "guard", "align", "bounds", "perm", "frag", "remap",
];

/// 用例优先级（P0 冒烟 / P1 常规 / P2 深钻）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prio {
    P0,
    P1,
    P2,
}

impl Prio {
    pub fn code(self) -> u8 {
        match self {
            Prio::P0 => 0,
            Prio::P1 => 1,
            Prio::P2 => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Prio::P0 => "P0",
            Prio::P1 => "P1",
            Prio::P2 => "P2",
        }
    }
}

/// 矩阵行：面/键/期望/优先级 四字段。
#[derive(Clone)]
pub struct MtxRow {
    pub face: usize, // 0..10
    pub case_key: &'static str,
    pub expect: &'static str,
    pub prio: Prio,
}

impl MtxRow {
    pub fn field_count(&self) -> usize {
        let mut n = 0;
        if self.face < 10 {
            n += 1;
        }
        if !self.case_key.is_empty() {
            n += 1;
        }
        if !self.expect.is_empty() {
            n += 1;
        }
        if self.prio.code() <= 2 {
            n += 1;
        }
        n
    }
    pub fn row_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(self.face as u32).to_le_bytes());
        b.extend_from_slice(self.case_key.as_bytes());
        b.push(0xFF);
        b.extend_from_slice(self.expect.as_bytes());
        b.push(0xFE);
        b.push(self.prio.code());
        b
    }
}

/// 回归矩阵（30 例全集）。
pub struct RegressMatrix {
    rows: Vec<MtxRow>,
    frozen: Option<u32>,
    rejects: Vec<(u32, &'static str)>,
}

impl RegressMatrix {
    pub fn new() -> Self {
        RegressMatrix {
            rows: Vec::new(),
            frozen: None,
            rejects: Vec::new(),
        }
    }

    pub fn add_row(&mut self, row: MtxRow) -> Result<(), u32> {
        if self.frozen.is_some() {
            self.rejects.push((XC_MTX_HASH, "矩阵已冻结拒加行"));
            return Err(XC_MTX_HASH);
        }
        if row.face >= 10 {
            self.rejects.push((XC_MTX_FACE, "面号越界"));
            return Err(XC_MTX_FACE);
        }
        if row.case_key.is_empty() || row.expect.is_empty() {
            self.rejects.push((XC_MTX_FIELD, "键或期望空"));
            return Err(XC_MTX_FIELD);
        }
        for r in &self.rows {
            if r.case_key == row.case_key {
                self.rejects.push((XC_MTX_DUP, "用例键重复"));
                return Err(XC_MTX_DUP);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    /// 冻结：全量指纹一次刻印。
    pub fn freeze(&mut self) -> u32 {
        let mut bytes = Vec::new();
        for r in &self.rows {
            bytes.extend_from_slice(&r.row_bytes());
        }
        let h = fnv1a32(&bytes);
        self.frozen = Some(h);
        h
    }

    pub fn fingerprint(&self) -> u32 {
        self.frozen.unwrap_or(0)
    }

    /// 字段完整性零缺失扫描。
    pub fn fields_complete(&self) -> bool {
        self.rows.iter().all(|r| r.field_count() == 4)
    }

    /// 十面覆盖核：每面恰 3 例。
    pub fn faces_covered(&self) -> bool {
        if self.rows.len() != 30 {
            return false;
        }
        for f in 0..10 {
            let c = self.rows.iter().filter(|r| r.face == f).count();
            if c != 3 {
                return false;
            }
        }
        true
    }

    pub fn rows(&self) -> &[MtxRow] {
        &self.rows
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准矩阵：十面 × 3 例 = 30 例（键名与期望逐字冻结）。
pub fn standard_matrix() -> RegressMatrix {
    let mut m = RegressMatrix::new();
    let spec: [(usize, [&str; 3], [&str; 3]); 10] = [
        (0, ["stk-ovf", "stk-align", "stk-guard"], ["拒写", "16B 对齐", "缺页断"]),
        (1, ["heap-grow", "heap-exhaust", "heap-free"], ["扩容", "返回空", "归还"]),
        (2, ["shm-create", "shm-mapdup", "shm-rm"], ["键唯一", "同址", "解除"]),
        (3, ["file-rd", "file-wr", "file-rw"], ["按需读", "回流", "双账平"]),
        (4, ["guard-touch", "guard-skip", "guard-unmap"], ["缺页", "越页合法", "解除"]),
        (5, ["align-4k", "align-2m", "align-mis"], ["4K 齐", "2M 大页", "拒绝"]),
        (6, ["bound-lo", "bound-hi", "bound-cross"], ["下界拒", "上界拒", "跨页断"]),
        (7, ["perm-rx", "perm-rw", "perm-none"], ["拒写", "可写", "全拒"]),
        (8, ["frag-frag", "frag-defrag", "frag-scan"], ["碎片升", "归并", "扫描账"]),
        (9, ["remap-grow", "remap-move", "remap-shrink"], ["原址扩", "搬移", "收缩"]),
    ];
    let prios = [Prio::P0, Prio::P1, Prio::P2];
    for (f, keys, exps) in spec {
        for i in 0..3 {
            let _ = m.add_row(MtxRow {
                face: f,
                case_key: keys[i],
                expect: exps[i],
                prio: prios[i],
            });
        }
    }
    m
}

/// 注入核：四类注入必抓（缺字段/缺面/缺例/哈希漂移）。
pub struct MtxInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl MtxInjectCore {
    pub fn new() -> Self {
        MtxInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_field_missing(&mut self) {
        self.total += 1;
        let mut m = RegressMatrix::new();
        let _ = m.add_row(MtxRow {
            face: 3,
            case_key: "x",
            expect: "",
            prio: Prio::P0,
        });
        if !m.fields_complete() && m.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_face_oob(&mut self) {
        self.total += 1;
        let mut m = RegressMatrix::new();
        let r = m.add_row(MtxRow {
            face: 10,
            case_key: "y",
            expect: "e",
            prio: Prio::P0,
        });
        if r.is_err() && m.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_case_missing(&mut self) {
        self.total += 1;
        let mut m = standard_matrix();
        // 移走一例的模拟：只加 29 例的矩阵补不齐
        let mut m2 = RegressMatrix::new();
        for r in m.rows().iter().take(29) {
            let _ = m2.add_row(r.clone());
        }
        m.freeze();
        if !m2.faces_covered() {
            self.caught += 1;
        }
    }
    pub fn inject_hash_drift(&mut self) {
        self.total += 1;
        let mut m = standard_matrix();
        let h1 = m.freeze();
        // 冻结后追加被拒 → 指纹不变
        let _ = m.add_row(MtxRow {
            face: 0,
            case_key: "late",
            expect: "e",
            prio: Prio::P0,
        });
        if m.fingerprint() == h1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx2862_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2862");

    // C62-01 标准矩阵 30 例且十面齐
    {
        let m = standard_matrix();
        let ok = m.rows().len() == 30 && m.faces_covered();
        s.add("C62-01-三十例十面齐", ok, if ok { "" } else { "矩阵不齐" });
    }
    // C62-02 字段完整性零缺失
    {
        let m = standard_matrix();
        let ok = m.fields_complete();
        s.add(
            "C62-02-字段零缺失",
            ok,
            if ok { "" } else { "存在缺失字段行" },
        );
    }
    // C62-03 冻结哈希入册恒等
    {
        let mut m = standard_matrix();
        let h1 = m.freeze();
        let h2 = m.fingerprint();
        let ok = h1 != 0 && h1 == h2;
        s.add("C62-03-哈希入册", ok, if ok { "" } else { "指纹漂移" });
    }
    // C62-04 双构造同指纹（确定性）
    {
        let mut a = standard_matrix();
        let mut b = standard_matrix();
        let ok = a.freeze() == b.freeze();
        s.add("C62-04-确定性指纹", ok, if ok { "" } else { "构造非确定" });
    }
    // C62-05 重复键拒绝
    {
        let mut m = RegressMatrix::new();
        let _ = m.add_row(MtxRow {
            face: 0,
            case_key: "k",
            expect: "e",
            prio: Prio::P0,
        });
        let dup = m.add_row(MtxRow {
            face: 0,
            case_key: "k",
            expect: "e2",
            prio: Prio::P1,
        });
        let ok = dup.is_err() && m.reject_count() == 1;
        s.add("C62-05-重复键拒", ok, if ok { "" } else { "重复键放行" });
    }
    // C62-06 注入核 4/4
    {
        let mut inj = MtxInjectCore::new();
        inj.inject_field_missing();
        inj.inject_face_oob();
        inj.inject_case_missing();
        inj.inject_hash_drift();
        s.add(
            "C62-06-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    // C62-07 优先级三态闭集
    {
        let m = standard_matrix();
        let ok = m.rows().iter().all(|r| r.prio.code() <= 2);
        s.add("C62-07-优先级闭集", ok, if ok { "" } else { "优先级越界" });
    }
    s
}
