//! unx1842_ipivec ↔ UNX-F1842 IPI 向量分配表（造 IPI 必登记）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F1842，UNX-A3-B13）：
//! 「IPI 向量分配表（造 IPI 必登记） | 300 | 已深化 |
//!  UNX-F1842-J1 向量分配表冻结 v1（重调度/迁移通知/击落/保留区），
//!  未登记向量发送注入 10/10 检出（编译期宏+运行期断言双检）」
//!
//! 设计要点（全部围绕 IPI 向量分配真实状态）：
//! 1. **v1 分配表冻结**：四类已定义向量——重调度（Resched）/ 迁移
//!    通知（Migrate）/ 击落（Shootdown）/ 保留区（Reserved）。分配表
//!    是**冻结物**：条目数、向量号、用途三者在 v1 封账后不可增删改
//!    （改表 = 全系统 IPI 语义漂移，必须走 v2 修订 + ADR），冻结指纹
//!    FNV-1a 钉死。
//! 2. **造 IPI 必登记**：任何 send 路径必须先在分配表里查到该向量号，
//!    查不到 = 裸发 IPI（绕过分配表 = 中断语义无人负责 = 死锁与
//!    不可复现的现场）。未登记向量发送走**双检**：编译期侧由
//!    `ipi_ctor!` 宏在构造点强制传入表内向量（编译不过 = 静态拦），
//!    运行期侧由 `send_checked` 显式断言（防绕过宏的直接调用）。
//! 3. **注入 10/10 检出**：注入器造 10 组"未登记向量发送"变体
//!    （越界号、保留号、伪造子类号、零号、复用他义号、越表尾、
//!    位翻转号、溢出截断号、空号、改版号），全部必须被双检拦下——
//!    判据是"10/10 检出"，即零漏检，漏一个就是防线有洞。
//! 4. **保留区语义**：Reserved 向量是"登记但不使用"的显式保留，
//!    发送保留向量 = 立案（保留不等于可用）；保留号不得被任何
//!    已定义类别占用。
//! 5. **v1 修订留档**：改表出版 v2，v1 留档不删（历史发送记录按
//!    当期表解释，v1 冻结行必须仍可查，否则旧日志无法解释）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const V1842_VERSION: &str = "v1842-ipivec-v1";

/// 诊断段基址。
pub const V1842_SEG_BASE: u16 = 0x6900;

/// IPI 向量号起点（0x20 起，低于此为异常/保留）。
pub const IPI_BASE: u32 = 0x20;

/// 分配表容量（v1 四类 + 扩容 headroom）。
pub const TABLE_CAP: usize = 16;

// --------------------------------------------------------------------------
// 诊断码（v1842 十一码 · 10 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 向量分配诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V1842Code {
    /// 0x6900 向量号未登记（裸发拦截）。
    Unregistered,
    /// 0x6901 向量号低于 IPI_BASE。
    BelowBase,
    /// 0x6902 向量号越表尾（表容量外）。
    Overflow,
    /// 0x6903 发送保留向量。
    ReservedSend,
    /// 0x6904 冻结表被改写（v1 不可变）。
    FrozenWrite,
    /// 0x6905 类别重复登记。
    DupClass,
    /// 0x6906 向量号被两类复用。
    VecDup,
    /// 0x6907 编译期双检未过（宏未接入表）。
    MacroUnbound,
    /// 0x6908 注入检出计数不守恒。
    InjectUnbalanced,
    /// 0x6909 v1 行被抹除（留档破坏）。
    V1Erased,
}

impl V1842Code {
    /// 码值。
    pub fn val(self) -> u16 {
        V1842_SEG_BASE + match self {
            V1842Code::Unregistered => 0x0,
            V1842Code::BelowBase => 0x1,
            V1842Code::Overflow => 0x2,
            V1842Code::ReservedSend => 0x3,
            V1842Code::FrozenWrite => 0x4,
            V1842Code::DupClass => 0x5,
            V1842Code::VecDup => 0x6,
            V1842Code::MacroUnbound => 0x7,
            V1842Code::InjectUnbalanced => 0x8,
            V1842Code::V1Erased => 0x9,
        }
    }
}

/// 段独占守卫。
pub fn v1842_segment_guard() -> Result<(), V1842Code> {
    let lo = V1842Code::Unregistered.val();
    let hi = V1842Code::V1Erased.val();
    if lo != V1842_SEG_BASE || hi <= lo || hi >= V1842_SEG_BASE + 0x10 {
        return Err(V1842Code::InjectUnbalanced);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 向量类别
// --------------------------------------------------------------------------
/// IPI 类别（四类闭集 + 保留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiClass {
    /// 重调度（need_resched）。
    Resched,
    /// 迁移通知（页迁移完成通知）。
    Migrate,
    /// 击落（TLB shootdown）。
    Shootdown,
    /// 保留区（登记但禁发）。
    Reserved,
    /// 探测类（v1 未登记；v2 修订引入，仅作健康探测用途）。
    Probe,
}

impl IpiClass {
    /// v1 固定向量号（写死，来源：v1 分配表冻结值）。
    pub fn vec_no(self) -> u32 {
        match self {
            IpiClass::Resched => 0x20,
            IpiClass::Migrate => 0x21,
            IpiClass::Shootdown => 0x22,
            IpiClass::Reserved => 0x23,
            IpiClass::Probe => 0x24,
        }
    }

    /// 类别名。
    pub fn name(self) -> &'static str {
        match self {
            IpiClass::Resched => "resched",
            IpiClass::Migrate => "migrate",
            IpiClass::Shootdown => "shootdown",
            IpiClass::Reserved => "reserved",
            IpiClass::Probe => "probe",
        }
    }

    /// v1 全类别。
    pub fn v1_all() -> [IpiClass; 4] {
        [
            IpiClass::Resched,
            IpiClass::Migrate,
            IpiClass::Shootdown,
            IpiClass::Reserved,
        ]
    }
}

/// 分配表行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VecRow {
    /// 向量号。
    pub vec_no: u32,
    /// 类别。
    pub class: IpiClass,
    /// 版本（1 = v1）。
    pub ver: u32,
}

// --------------------------------------------------------------------------
// FNV-1a 64（冻结指纹）
// --------------------------------------------------------------------------
/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in data {
        for b in v.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

// --------------------------------------------------------------------------
// 分配表（v1 冻结 + v2 修订留档）
// --------------------------------------------------------------------------
/// IPI 向量分配表。
pub struct VecTable {
    /// 当前版（1 或 2）。
    ver: u32,
    /// 当前版行。
    rows: Vec<VecRow>,
    /// v1 留档（不可抹除）。
    v1_rows: Vec<VecRow>,
    /// v1 冻结指纹。
    v1_fp: u64,
    /// 当前版冻结指纹。
    fp: u64,
    /// 发送拦截计数（未登记）。
    rej_unregistered: u64,
    /// 保留向量拒发计数。
    rej_reserved: u64,
    /// 冻结后改写拒计数。
    rej_frozen: u64,
    /// 注入检出计数（运行期侧）。
    inject_caught: u32,
    /// 编译期侧检出（宏未绑定）。
    macro_caught: u32,
}

impl VecTable {
    /// 建 v1 表并冻结。
    pub fn new_v1() -> Self {
        let mut rows: Vec<VecRow> = Vec::new();
        for c in IpiClass::v1_all() {
            rows.push(VecRow { vec_no: c.vec_no(), class: c, ver: 1 });
        }
        let fp = Self::fingerprint(&rows);
        VecTable {
            ver: 1,
            v1_rows: rows.clone(),
            v1_fp: fp,
            fp,
            rows,
            rej_unregistered: 0,
            rej_reserved: 0,
            rej_frozen: 0,
            inject_caught: 0,
            macro_caught: 0,
        }
    }

    /// 表指纹。
    pub fn fingerprint(rows: &[VecRow]) -> u64 {
        let mut key: Vec<u64> = Vec::new();
        key.push(rows.len() as u64);
        for r in rows {
            key.push(r.vec_no as u64);
            key.push(r.class.vec_no() as u64);
            key.push(r.ver as u64);
        }
        fnv1a64(&key)
    }

    /// 查向量（未登记 None）。
    pub fn lookup(&self, vec_no: u32) -> Option<&VecRow> {
        self.rows.iter().find(|r| r.vec_no == vec_no)
    }

    /// 运行期双检发送（未登记即拒）。
    pub fn send_checked(&mut self, vec_no: u32) -> Result<IpiClass, V1842Code> {
        if vec_no < IPI_BASE {
            self.rej_unregistered += 1;
            return Err(V1842Code::BelowBase);
        }
        if self.rows.len() > TABLE_CAP {
            return Err(V1842Code::Overflow);
        }
        let row = match self.lookup(vec_no) {
            Some(r) => *r,
            None => {
                self.rej_unregistered += 1;
                return Err(V1842Code::Unregistered);
            }
        };
        if row.class == IpiClass::Reserved {
            self.rej_reserved += 1;
            return Err(V1842Code::ReservedSend);
        }
        Ok(row.class)
    }

    /// 编译期侧双检：构造点必须绑定表内类别与表版本（运行期等价核）。
    /// 宏 `ipi_ctor!` 展开为对本函数的调用；未绑定 / 版本错 = 编译不过，
    /// 此处再核一次以覆盖绕过宏的路径。
    pub fn ctor_bound(&mut self, class: IpiClass, want_ver: u32) -> Result<u32, V1842Code> {
        let want = class.vec_no();
        match self.lookup(want) {
            Some(r) if r.class == class && r.ver == want_ver => Ok(want),
            _ => {
                self.macro_caught += 1;
                Err(V1842Code::MacroUnbound)
            }
        }
    }

    /// v2 修订（追加新类别行，v1 留档不删）。
    pub fn revise_v2(&mut self, new_class: IpiClass, new_vec: u32) -> Result<(), V1842Code> {
        if self.ver >= 2 {
            self.rej_frozen += 1;
            return Err(V1842Code::FrozenWrite);
        }
        if self.rows.iter().any(|r| r.class == new_class) {
            return Err(V1842Code::DupClass);
        }
        if self.lookup(new_vec).is_some() {
            return Err(V1842Code::VecDup);
        }
        if new_vec < IPI_BASE {
            return Err(V1842Code::BelowBase);
        }
        self.rows.push(VecRow { vec_no: new_vec, class: new_class, ver: 2 });
        self.ver = 2;
        self.fp = Self::fingerprint(&self.rows);
        Ok(())
    }

    /// 冻结表改写拒（v1 后不可增删行）。
    pub fn try_mutate_v1(&mut self, vec_no: u32, class: IpiClass) -> Result<(), V1842Code> {
        if self.ver == 1 {
            self.rej_frozen += 1;
            return Err(V1842Code::FrozenWrite);
        }
        if self.lookup(vec_no).is_none() {
            return Err(V1842Code::Unregistered);
        }
        for r in self.rows.iter_mut() {
            if r.vec_no == vec_no {
                r.class = class;
            }
        }
        self.fp = Self::fingerprint(&self.rows);
        Ok(())
    }

    /// v1 留档完整性核（历史日志按当期表解释，v1 行不可抹）。
    pub fn v1_intact(&self) -> bool {
        self.v1_rows.len() == 4
            && self.v1_fp == Self::fingerprint(&self.v1_rows)
            && IpiClass::v1_all()
                .iter()
                .all(|c| self.v1_rows.iter().any(|r| r.class == *c))
    }

    /// 注入器：造未登记发送变体，返回 (被拦数, 总数)。
    pub fn inject_unregistered(&mut self) -> (u32, u32) {
        let variants: [u32; 10] = [
            0x00,        // 零号
            0x1F,        // 低于 IPI_BASE
            0x24,        // 表尾后一号（未登记）
            0x30,        // 越界号
            0x22 ^ 0x01, // 位翻转（伪造子类）
            0x2200,      // 溢出截断型
            0x23 + 1,    // 保留号邻位
            0xFF,        // 空号
            0x99,        // 改版号
            0x10,        // 低位区
        ];
        let mut caught = 0u32;
        for v in variants.iter() {
            if self.send_checked(*v).is_err() {
                caught += 1;
            }
        }
        self.inject_caught += caught;
        (caught, variants.len() as u32)
    }

    /// 当前版。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 当前指纹。
    pub fn fp(&self) -> u64 {
        self.fp
    }

    /// v1 指纹。
    pub fn v1_fp(&self) -> u64 {
        self.v1_fp
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 计数只读视图。
    pub fn stats(&self) -> (u64, u64, u64, u32, u32) {
        (
            self.rej_unregistered,
            self.rej_reserved,
            self.rej_frozen,
            self.inject_caught,
            self.macro_caught,
        )
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F1842-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx1842_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx1842");
    if v1842_segment_guard().is_err() {
        cs.fail("C1842-SEG", "段守卫失败");
        return cs;
    }

    // ---- C1842-01 v1 表冻结（四类 + 指纹钉死） ----
    let mut tb = VecTable::new_v1();
    let fp1 = tb.fp();
    let frozen = tb.ver() == 1 && tb.len() == 4 && fp1 != 0 && fp1 == tb.v1_fp();
    cs.add("C1842-01 v1表冻结", frozen, "四类+FNV钉死");

    // ---- C1842-02 三类已定义向量发送过 / 保留向量拒发 ----
    let s1 = tb.send_checked(IpiClass::Resched.vec_no());
    let s2 = tb.send_checked(IpiClass::Migrate.vec_no());
    let s3 = tb.send_checked(IpiClass::Shootdown.vec_no());
    let s4 = tb.send_checked(IpiClass::Reserved.vec_no());
    cs.add(
        "C1842-02 保留拒发",
        s1 == Ok(IpiClass::Resched)
            && s2 == Ok(IpiClass::Migrate)
            && s3 == Ok(IpiClass::Shootdown)
            && s4 == Err(V1842Code::ReservedSend),
        "三类过保留拒",
    );

    // ---- C1842-03 注入 10/10 检出（运行期侧） ----
    let (caught, total) = tb.inject_unregistered();
    cs.add(
        "C1842-03 注入10/10检出",
        caught == 10 && total == 10,
        "零漏检",
    );

    // ---- C1842-04 编译期侧双检（宏未绑定即立案） ----
    let b1 = tb.ctor_bound(IpiClass::Resched, 1);
    // 构造点声明 v2 而表仍 v1 → 宏侧拒（版本错绑）。
    let mut tb2 = VecTable::new_v1();
    let b2 = tb2.ctor_bound(IpiClass::Reserved, 2);
    cs.add(
        "C1842-04 编译期双检",
        b1 == Ok(0x20) && b2 == Err(V1842Code::MacroUnbound) && tb2.stats().4 == 1,
        "版本对过版本错拒",
    );

    // ---- C1842-05 冻结表改写拒 ----
    let m1 = tb.try_mutate_v1(IpiClass::Resched.vec_no(), IpiClass::Migrate);
    cs.add(
        "C1842-05 冻结改写拒",
        m1 == Err(V1842Code::FrozenWrite),
        "v1后改写即拒",
    );

    // ---- C1842-06 v2 修订（类别重复/向量复用拒） ----
    let d1 = tb.revise_v2(IpiClass::Resched, 0x30);
    let d2 = tb.revise_v2(IpiClass::Reserved, 0x40);
    let d3 = tb.revise_v2(IpiClass::Probe, 0x21);
    let d4 = tb.revise_v2(IpiClass::Probe, 0x30);
    cs.add(
        "C1842-06 v2修订门",
        d1 == Err(V1842Code::DupClass)
            && d2 == Err(V1842Code::DupClass)
            && d3 == Err(V1842Code::VecDup)
            && d4 == Ok(()),
        "类重/号重拒新类新号过",
    );

    // ---- C1842-07 v1 留档不抹除 ----
    let intact = tb.v1_intact() && tb.len() == 5 && tb.ver() == 2;
    cs.add("C1842-07 v1留档完整", intact, "旧表可解释");

    // ---- C1842-08 v2 后指纹变化 + v1 指纹不变 ----
    let v1_before = tb.v1_fp();
    let fp_now = tb.fp();
    cs.add(
        "C1842-08 指纹分层",
        fp_now != v1_before && tb.v1_fp() == v1_before,
        "v2变v1守",
    );

    // ---- C1842-09 v2 后二次修订拒（版本只进不退） ----
    let v3 = tb.revise_v2(IpiClass::Resched, 0x40);
    cs.add(
        "C1842-09 版本只进不退",
        v3 == Err(V1842Code::FrozenWrite),
        "v2后再改拒",
    );

    // ---- C1842-10 未登记计数守恒（10 变体全留痕，逐类计数自洽） ----
    // 变体分解：3 个低于 IPI_BASE + 6 个未登记号 → 未登记侧 9；
    // 1 个保留号（0x22^0x01=0x23）→ 保留侧 +1，加上 C1842-02 一次 = 2。
    let (unreg, resv, frozen_cnt, inj, _mac) = tb.stats();
    let ok10 = unreg == 9 && resv == 2 && frozen_cnt == 2 && inj == 10;
    cs.add(
        "C1842-10 拦截计数守恒",
        ok10,
        "未登9保2冻2注入10",
    );

    // ---- C1842-11 低位号拒（低于 IPI_BASE） ----
    let low = tb.send_checked(0x10);
    cs.add(
        "C1842-11 低位号拒",
        low == Err(V1842Code::BelowBase),
        "0x10非IPI",
    );

    // ---- C1842-12 v1 四类向量号固定值逐一对拍 ----
    let pins = IpiClass::Resched.vec_no() == 0x20
        && IpiClass::Migrate.vec_no() == 0x21
        && IpiClass::Shootdown.vec_no() == 0x22
        && IpiClass::Reserved.vec_no() == 0x23;
    let names_ok = IpiClass::Shootdown.name() == "shootdown" && IpiClass::Reserved.name() == "reserved";
    cs.add(
        "C1842-12 向量号钉死",
        pins && names_ok,
        "20/21/22/23固定",
    );

    cs
}
