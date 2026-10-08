//! Wine 组透明（WP-206 · B-2003）：组聚合与展开正确——兼容不等于黑箱。
//!
//! MD2 篇 20.1：Wine 组的展示按组聚合（wineserver 加组内进程一卡显示），
//! 组内展开看单进程——Windows 应用的资源占用在 VARIX 的账本里同样透明，
//! 兼容不等于黑箱。
//!
//! 透明语义三条：①聚合==成员之和恒等式（wineserver 自身读数计入）；
//! ②组内展开每成员独立可见（聚合不隐藏个体）；③非 Wine 进程绝不进组。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 进程行与组模型
// ---------------------------------------------------------------------------

pub const MEMBER_CAP: usize = 16; // 组内成员上限（wineserver 之外）
pub const GROUP_CAP: usize = 4; // Wine 组上限
pub const NAME_LEN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    pub name: [u8; NAME_LEN],
    pub name_len: usize,
    pub cpu_permille: u64,
    pub mem_kb: u64,
    pub is_wine: bool,
}

impl ProcRow {
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len.min(NAME_LEN)]
    }
}

/// 一张组卡：wineserver 一行 + 组内成员若干行，聚合读数=sum(server, members)。
#[derive(Clone, Copy)]
pub struct WineGroup {
    pub used: bool,
    pub server: Option<ProcRow>,
    pub members: [Option<ProcRow>; MEMBER_CAP],
    pub member_cnt: usize,
    /// 拒绝留痕：成员超限被拒的累计次数（不静默丢）。
    pub rejected: u64,
}

impl WineGroup {
    pub const fn new() -> Self {
        WineGroup { used: false, server: None, members: [None; MEMBER_CAP], member_cnt: 0, rejected: 0 }
    }

    pub fn server_pid(&self) -> u32 {
        match self.server {
            Some(s) => s.pid,
            None => 0,
        }
    }

    /// 成员入组：满员拒绝并留痕（资源面守恒——拒绝可见）。
    pub fn add_member(&mut self, row: ProcRow) -> bool {
        if !row.is_wine {
            self.rejected += 1;
            return false;
        }
        if self.member_cnt >= MEMBER_CAP {
            self.rejected += 1;
            return false;
        }
        let mut i = 0;
        while i < MEMBER_CAP {
            if self.members[i].is_none() {
                self.members[i] = Some(row);
                self.member_cnt += 1;
                return true;
            }
            i += 1;
        }
        self.rejected += 1;
        false
    }

    /// 聚合 CPU：wineserver + Σ成员（千分比整数口径）。
    pub fn agg_cpu_permille(&self) -> u64 {
        let mut sum = self.server.map(|s| s.cpu_permille).unwrap_or(0);
        let mut i = 0;
        while i < MEMBER_CAP {
            if let Some(m) = self.members[i] {
                sum += m.cpu_permille;
            }
            i += 1;
        }
        sum
    }

    /// 聚合内存：wineserver + Σ成员（KB 整数口径）。
    pub fn agg_mem_kb(&self) -> u64 {
        let mut sum = self.server.map(|s| s.mem_kb).unwrap_or(0);
        let mut i = 0;
        while i < MEMBER_CAP {
            if let Some(m) = self.members[i] {
                sum += m.mem_kb;
            }
            i += 1;
        }
        sum
    }

    /// 展开行数：wineserver 一行 + 成员各行（聚合卡上的全部个体）。
    pub fn expanded_rows(&self) -> usize {
        (if self.server.is_some() { 1 } else { 0 }) + self.member_cnt
    }
}

// ---------------------------------------------------------------------------
// 组归并：进程表 → Wine 组卡
// ---------------------------------------------------------------------------

pub const PROC_CAP: usize = 64;

pub struct ProcTable {
    rows: [Option<ProcRow>; PROC_CAP],
    pub row_cnt: usize,
}

impl ProcTable {
    pub fn new() -> Self {
        ProcTable { rows: [None; PROC_CAP], row_cnt: 0 }
    }

    pub fn add(&mut self, row: ProcRow) -> bool {
        if self.row_cnt >= PROC_CAP {
            return false;
        }
        self.rows[self.row_cnt] = Some(row);
        self.row_cnt += 1;
        true
    }

    pub fn row(&self, i: usize) -> Option<ProcRow> {
        if i < self.row_cnt {
            self.rows[i]
        } else {
            None
        }
    }

    /// 组归并：wineserver 行（名字含 "wineserver"）立卡，is_wine 的其余行
    /// 入最近一张卡（无卡则先立卡再入）；非 Wine 行不进任何组。
    /// 归并后组卡聚合==成员之和的恒等式由调用方对账（CheckSet 项 2）。
    pub fn merge_groups(&self) -> ([WineGroup; GROUP_CAP], usize) {
        let mut groups = [WineGroup::new(); GROUP_CAP];
        let mut gcnt = 0;
        let mut i = 0;
        while i < self.row_cnt {
            let row = match self.rows[i] {
                Some(r) => r,
                None => {
                    i += 1;
                    continue;
                }
            };
            if !row.is_wine {
                i += 1;
                continue;
            }
            let is_server = row.name_bytes() == b"wineserver";
            if is_server {
                // wineserver 行立新卡（server 槽位）。
                if gcnt < GROUP_CAP {
                    groups[gcnt].used = true;
                    groups[gcnt].server = Some(row);
                    gcnt += 1;
                }
            } else {
                // 成员行：入最近一张卡；无卡则先立空卡（server 稍后到也合法）。
                if gcnt == 0 || groups[gcnt - 1].member_cnt >= MEMBER_CAP {
                    if gcnt < GROUP_CAP {
                        groups[gcnt].used = true;
                        gcnt += 1;
                    }
                }
                if gcnt > 0 {
                    groups[gcnt - 1].add_member(row);
                }
            }
            i += 1;
        }
        (groups, gcnt)
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-2003 · 8 项）
// ---------------------------------------------------------------------------

fn mkrow(pid: u32, name: &[u8], cpu: u64, mem: u64, wine: bool) -> ProcRow {
    let mut n = [0u8; NAME_LEN];
    let l = name.len().min(NAME_LEN);
    n[..l].copy_from_slice(&name[..l]);
    ProcRow { pid, name: n, name_len: l, cpu_permille: cpu, mem_kb: mem, is_wine: wine }
}

pub fn run_winegrp_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2003 Wine 组透明");
    // 1. 组聚合形成：wineserver+成员归并为一卡。
    let mut t1 = ProcTable::new();
    assert!(t1.add(mkrow(100, b"wineserver", 40, 51_200, true)));
    assert!(t1.add(mkrow(101, b"app-win", 120, 102_400, true)));
    assert!(t1.add(mkrow(102, b"vxedit", 5, 8_192, false)));
    let (g1, n1) = t1.merge_groups();
    set.add(
        "B-2003 组聚合形成",
        n1 == 1 && g1[0].used && g1[0].server_pid() == 100 && g1[0].member_cnt == 1,
        "wineserver 与组内 Windows 进程归并一卡，非 Wine 行不入组",
    );
    // 2. 聚合==成员之和恒等式（CPU 与内存两面，server 自身计入）。
    let agg_cpu_ok = g1[0].agg_cpu_permille() == 40 + 120;
    let agg_mem_ok = g1[0].agg_mem_kb() == 51_200 + 102_400;
    set.add(
        "B-2003 聚合恒等式",
        agg_cpu_ok && agg_mem_ok,
        "组卡聚合值==wineserver+Σ成员，逐面可对账",
    );
    // 3. 展开正确：行数==server+成员数，成员独立可见。
    let rows3 = g1[0].expanded_rows();
    let m3 = g1[0].members[0];
    set.add(
        "B-2003 组内展开正确",
        rows3 == 2 && matches!(m3, Some(r) if r.pid == 101 && r.cpu_permille == 120),
        "展开看单进程：每成员独立读数，聚合不隐藏个体",
    );
    // 4. 无 Wine 进程时零卡。
    let mut t4 = ProcTable::new();
    assert!(t4.add(mkrow(1, b"vxedit", 5, 8_192, false)));
    assert!(t4.add(mkrow(2, b"vxterm", 3, 4_096, false)));
    let (_, n4) = t4.merge_groups();
    set.add(
        "B-2003 无 Wine 零卡",
        n4 == 0,
        "没有 Wine 进程就没有组卡（不出现空壳卡）",
    );
    // 5. 非 Wine 进程绝不进组（入组面直接拒绝）。
    let mut g5 = WineGroup::new();
    g5.used = true;
    let native5 = mkrow(9, b"vxedit", 1, 1, false);
    let add5 = g5.add_member(native5);
    set.add(
        "B-2003 非 Wine 不进组",
        !add5 && g5.member_cnt == 0 && g5.rejected == 1,
        "is_wine==false 的行在入组口被拒并留痕",
    );
    // 6. 成员超限拒绝留痕（不静默丢）。
    let mut g6 = WineGroup::new();
    g6.used = true;
    let mut i6 = 0usize;
    while i6 < MEMBER_CAP {
        assert!(g6.add_member(mkrow(1000 + i6 as u32, b"win-app", 1, 1, true)));
        i6 += 1;
    }
    let over6 = g6.add_member(mkrow(9999, b"win-app", 1, 1, true));
    set.add(
        "B-2003 成员超限留痕",
        !over6 && g6.member_cnt == MEMBER_CAP && g6.rejected == 1,
        "满员后的成员行被拒且 rejected 计数上升——拒绝可见",
    );
    // 7. 多组并存：两组各自聚合互不串账。
    let mut t7 = ProcTable::new();
    assert!(t7.add(mkrow(200, b"wineserver", 10, 20_480, true)));
    assert!(t7.add(mkrow(201, b"win-a", 30, 30_720, true)));
    assert!(t7.add(mkrow(300, b"wineserver", 20, 40_960, true)));
    assert!(t7.add(mkrow(301, b"win-b", 60, 61_440, true)));
    let (g7, n7) = t7.merge_groups();
    let g0 = &g7[0];
    let g1x = &g7[1];
    set.add(
        "B-2003 多组不串账",
        n7 == 2
            && g0.server_pid() == 200
            && g0.agg_cpu_permille() == 40
            && g1x.server_pid() == 300
            && g1x.agg_cpu_permille() == 80,
        "两组各自聚合：40=10+30、80=20+60，组间零混线",
    );
    // 8. 展开读数与录入同源（成员行值==录入值，无加工）。
    let m8 = g7[1].members[0];
    set.add(
        "B-2003 展开与录入同源",
        matches!(m8, Some(r) if r.pid == 301 && r.mem_kb == 61_440 && r.is_wine),
        "展开行原样回放录入值——透明即无中间加工",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fa03 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fa03_agg_identity() {
        let mut g = WineGroup::new();
        g.used = true;
        g.server = Some(mkrow(1, b"wineserver", 50, 65_536, true));
        assert!(g.add_member(mkrow(2, b"win-a", 100, 131_072, true)));
        assert!(g.add_member(mkrow(3, b"win-b", 200, 262_144, true)));
        assert_eq!(g.agg_cpu_permille(), 350);
        assert_eq!(g.agg_mem_kb(), 65_536 + 131_072 + 262_144);
        assert_eq!(g.expanded_rows(), 3);
    }

    #[test]
    fn fa03_merge_and_expand() {
        let mut t = ProcTable::new();
        assert!(t.add(mkrow(1, b"vxinit", 1, 1, false)));
        assert!(t.add(mkrow(10, b"wineserver", 10, 10_240, true)));
        assert!(t.add(mkrow(11, b"win-x", 20, 20_480, true)));
        assert!(t.add(mkrow(12, b"win-y", 30, 30_720, true)));
        let (groups, n) = t.merge_groups();
        assert_eq!(n, 1);
        assert_eq!(groups[0].member_cnt, 2);
        assert_eq!(groups[0].agg_cpu_permille(), 60);
        assert_eq!(groups[0].expanded_rows(), 3);
    }

    #[test]
    fn fa03_reject_and_conserve() {
        let mut g = WineGroup::new();
        g.used = true;
        assert!(!g.add_member(mkrow(1, b"native", 1, 1, false)), "非 Wine 拒");
        assert_eq!(g.rejected, 1);
        let mut i = 0usize;
        while i < MEMBER_CAP {
            assert!(g.add_member(mkrow(10 + i as u32, b"win", 1, 1, true)));
            i += 1;
        }
        assert!(!g.add_member(mkrow(500, b"win", 1, 1, true)), "满员拒");
        assert_eq!(g.rejected, 2, "两次拒绝都留痕");
        assert_eq!(g.member_cnt, MEMBER_CAP);
    }

    #[test]
    fn fa03_zero_group_when_no_wine() {
        let mut t = ProcTable::new();
        assert!(t.add(mkrow(1, b"vxedit", 5, 8_192, false)));
        assert!(t.add(mkrow(2, b"vxterm", 3, 4_096, false)));
        let (_, n) = t.merge_groups();
        assert_eq!(n, 0);
    }
}
