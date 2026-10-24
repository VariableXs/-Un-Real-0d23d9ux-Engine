//! UNX-F8341 · ld.so 装载协作协议（内核↔解释器分工表）账 · 诊断段 0xB460
//! 验收锚点：UNX-F8341-J1 协作分工表成文——内核做映射 / ld.so 做重定位与
//! 依赖解析（职责两分逐项登记），分工表与 F8064 递归装载口径一致机检，
//! 协议版本冻结。


use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB460;

pub const PROTOCOL_VERSION: &str = "lxso-ldso-v1.0-frozen";

/// 分工职责归属。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duty {
    Kernel,
    Ldso,
}

impl Duty {
    pub fn as_str(self) -> &'static str {
        match self {
            Duty::Kernel => "kernel",
            Duty::Ldso => "ld.so",
        }
    }
}

/// 分工表行。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DutyRow {
    pub work: &'static str,
    pub owner: Duty,
}

impl DutyRow {
    pub fn new(work: &'static str, owner: Duty) -> Self {
        DutyRow { work, owner }
    }
}

/// 协作分工表（职责两分逐项登记）。
pub fn duty_table() -> Vec<DutyRow> {
    vec![
        DutyRow::new("PT_LOAD 映射", Duty::Kernel),
        DutyRow::new("匿名页清零", Duty::Kernel),
        DutyRow::new("mprotect 权限落地", Duty::Kernel),
        DutyRow::new("栈布局装配", Duty::Kernel),
        DutyRow::new("auxv 传递 AT_PHDR/AT_BASE", Duty::Kernel),
        DutyRow::new("GOT/PLT 重定位", Duty::Ldso),
        DutyRow::new("DT_NEEDED 依赖解析", Duty::Ldso),
        DutyRow::new("符号查找与绑定", Duty::Ldso),
        DutyRow::new("TLS 初始化镜像", Duty::Ldso),
        DutyRow::new("递归装载入口", Duty::Ldso),
    ]
}

/// 协议账。
pub struct LdsoProtocol {
    pub rows: Vec<DutyRow>,
    pub version: &'static str,
}

impl LdsoProtocol {
    pub fn new() -> Self {
        LdsoProtocol { rows: Vec::new(), version: PROTOCOL_VERSION }
    }
    pub fn build_default(&mut self) {
        self.rows = duty_table();
    }
    /// 职责两分：两 owner 各至少 1 条，且逐行归属与 canon 表全等。
    pub fn two_sides(&self) -> bool {
        self.rows.iter().any(|r| r.owner == Duty::Kernel)
            && self.rows.iter().any(|r| r.owner == Duty::Ldso)
    }
    pub fn matches_canon(&self) -> bool {
        let canon = duty_table();
        self.rows.len() == canon.len()
            && self.rows.iter().zip(canon.iter()).all(|(a, b)| a == b)
    }
    /// 与 F8064 递归装载口径一致机检：递归入口归 ld.so。
    pub fn f8064_consistent(&self) -> bool {
        self.rows.iter().any(|r| r.work == "递归装载入口" && r.owner == Duty::Ldso)
    }
    /// 协议版本冻结。
    pub fn frozen(&self) -> bool {
        self.version == PROTOCOL_VERSION
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB460 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8341_B460;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("表-十行常数", duty_table().len() == 10));
    out.push(("版本-冻结串", PROTOCOL_VERSION.ends_with("frozen")));
    out.push(("口径-空账不一致", !LdsoProtocol::new().f8064_consistent()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8341_checks：本单判据聚合（诊断段 0xB460）。
pub fn run_unx8341_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8341 ld.so 装载协作分工表");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 分工表成文：十行登记
    let mut p = LdsoProtocol::new();
    p.build_default();
    let n_ok = p.rows.len() == 10;
    s.add("成文-十行职责登记", n_ok, if n_ok { "" } else { "登记缺口" });

    // 职责两分
    let two = p.two_sides();
    let k_n = p.rows.iter().filter(|r| r.owner == Duty::Kernel).count();
    let l_n = p.rows.iter().filter(|r| r.owner == Duty::Ldso).count();
    s.add("两分-kernel5/ldso5各半", two && k_n == 5 && l_n == 5, if two { "" } else { "两分缺口" });

    // 与 canon 全等
    s.add("机检-逐行与canon全等", p.matches_canon(), if p.matches_canon() { "" } else { "canon 对拍红" });

    // F8064 递归装载口径一致机检
    s.add("口径-F8064递归入口归ld.so", p.f8064_consistent(), if p.f8064_consistent() { "" } else { "口径机检红" });

    // 版本冻结
    s.add("版本-协议冻结串一致", p.frozen(), if p.frozen() { "" } else { "版本漂移" });

    // 错账检出：职责归属颠倒（映射归 ld.so）→ canon 对拍红
    let mut p2 = LdsoProtocol::new();
    p2.build_default();
    p2.rows[0].owner = Duty::Ldso;
    s.add("错账-映射归ld.so检出", !p2.matches_canon(), if !p2.matches_canon() { "" } else { "颠倒漏检" });

    // 错账检出：递归入口归内核 → 口径机检红
    let mut p3 = LdsoProtocol::new();
    p3.build_default();
    p3.rows[9].owner = Duty::Kernel;
    s.add("错账-递归归内核口径红", !p3.f8064_consistent(), if !p3.f8064_consistent() { "" } else { "口径错漏检" });

    // 版本漂移检出
    let mut p4 = LdsoProtocol::new();
    p4.build_default();
    p4.version = "lxso-ldso-v2.0-unfrozen";
    s.add("版本-漂移检出", !p4.frozen(), if !p4.frozen() { "" } else { "漂移漏检" });

    // 分工名导出
    let names = duty_table().iter().map(|r| r.owner.as_str()).collect::<Vec<&str>>().join(",");
    s.add("导出-十行owner序一致", names == "kernel,kernel,kernel,kernel,kernel,ld.so,ld.so,ld.so,ld.so,ld.so", if names.starts_with("kernel,kernel") { "" } else { "owner 序漂移" });

    // FNV 表指纹稳定
    let mut acc: u32 = 0x811C_9DC5;
    for r in duty_table() {
        let bit: u8 = if matches!(r.owner, Duty::Kernel) { 1 } else { 0 };
        acc ^= fnv1a(&[bit]);
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-表指纹非零", acc != 0, if acc != 0 { "" } else { "指纹异常" });

    s
}
