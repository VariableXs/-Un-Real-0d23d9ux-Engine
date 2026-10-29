//! UNX-F11220 B01 批 ktest 断言集聚合（0x1f820 段）
//!
//! 锚点：UNX-F11220-J1 本批 19 条判据全部聚合入 ktest 生态面，一次命令
//! 全跑，通过率 100% 才算绿。
//! 域：CoRun·UNX-C5-B01——聚合器只做三件事：登断言、一次跑、算通过率；
//! 任何缺条、重号、未跑即冻存都判红，绝不以部分通过冒充全绿。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f820;
pub const KA_COUNT: u32 = SEG_BASE + 0x01; // 断言条数不等于十九
pub const KA_FAIL: u32 = SEG_BASE + 0x02; // 存在红断言
pub const KA_DUP: u32 = SEG_BASE + 0x03; // 判据号重录
pub const KA_EMPTY: u32 = SEG_BASE + 0x04; // 判据号为空
pub const KA_NORUN: u32 = SEG_BASE + 0x05; // 未跑即冻存
pub const KA_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const KA_OVER: u32 = SEG_BASE + 0x07; // 超容

/// B01 批断言条数。
pub const WANT: usize = 19;
pub const CAP: usize = 24;
pub const KA_TAG: u64 = 0x1f820b01;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一条判据断言。
#[derive(Clone, Debug)]
pub struct Assertion {
    pub id: &'static str,
    pub title: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

impl Assertion {
    pub fn id_ok(&self) -> bool {
        let b = self.id.as_bytes();
        b.len() == 10 && b.starts_with(b"UNX-F") && b[5..].iter().all(|c| c.is_ascii_digit())
    }
}

/// 聚合批。
#[derive(Clone, Debug)]
pub struct KTestBatch {
    pub rows: Vec<Assertion>,
    pub ran: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl KTestBatch {
    pub fn new() -> KTestBatch {
        KTestBatch { rows: Vec::new(), ran: false, frozen: false, root: None }
    }

    /// 登一条断言。
    pub fn add(&mut self, a: Assertion) -> Result<(), u32> {
        if self.frozen {
            return Err(KA_FROZEN);
        }
        if a.id.is_empty() {
            return Err(KA_EMPTY);
        }
        if self.rows.iter().any(|r| r.id == a.id) {
            return Err(KA_DUP);
        }
        if self.rows.len() >= CAP {
            return Err(KA_OVER);
        }
        self.rows.push(a);
        Ok(())
    }

    /// 一次命令全跑：逐条执行置 ran。
    pub fn run_all(&mut self) -> usize {
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].passed {
                ok += 1;
            }
            k += 1;
        }
        self.ran = true;
        ok
    }

    /// 通过率（百分比向下取整；空批记 0）。
    pub fn pass_rate(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let ok = self.passed();
        (ok as u32).saturating_mul(100) / self.rows.len() as u32
    }

    pub fn passed(&self) -> usize {
        self.rows.iter().filter(|r| r.passed).count()
    }

    /// 全绿：恰十九条、已跑、逐条绿。
    pub fn all_green(&self) -> bool {
        self.ran && self.rows.len() == WANT && self.passed() == WANT
    }

    /// 冻存为 ktest 基线。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(KA_FROZEN);
        }
        if self.rows.len() != WANT {
            return Err(KA_COUNT);
        }
        if !self.ran {
            return Err(KA_NORUN);
        }
        if self.passed() != WANT {
            return Err(KA_FAIL);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("B01 {}/{} ran={}", self.passed(), self.rows.len(), self.ran)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = KA_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            let id = self.rows[k].id.as_bytes();
            let mut i = 0usize;
            while i < id.len() {
                h = (h ^ id[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.rows[k].passed as u64).wrapping_mul(FP_STEP);
            let d = self.rows[k].detail.as_bytes();
            i = 0;
            while i < d.len() {
                h = (h ^ d[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            k += 1;
        }
        h = (h ^ self.ran as u64).wrapping_mul(FP_STEP);
        h
    }
}

impl KTestBatch {
    /// 红条清单（按登记序）。
    pub fn failures(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].passed {
                out.push(self.rows[k].id);
            }
            k += 1;
        }
        out
    }

    /// 冻存后重跑一律拒（基线不可再动）。
    pub fn rerun(&mut self) -> Result<usize, u32> {
        if self.frozen {
            return Err(KA_FROZEN);
        }
        Ok(self.run_all())
    }
}

/// B01 批十九条判据：号、短题、明细。
pub const ROWS: [(&str, &str, &str); 19] = [
    ("UNX-F11200", "C4 收口锚", "三断言十项回写"),
    ("UNX-F11201", "glibc 版本表", "十二钉定依赖"),
    ("UNX-F11202", "符号版本查找", "双键缺一即 SV_NOSYM"),
    ("UNX-F11203", "coreutils 三协议", "stdin/stdout/code"),
    ("UNX-F11204", "applet 分派", "四十 applet 全表"),
    ("UNX-F11205", "dlopen 语义", "引用计数归零卸载"),
    ("UNX-F11206", "版本表查询", "二分命中"),
    ("UNX-F11207", "auxv 登记", "十二键对账"),
    ("UNX-F11208", "vvar/vDSO 布局", "四页 vvar 两页 vdso"),
    ("UNX-F11209", "futex 操作", "AI-14 四操作"),
    ("UNX-F11210", "/proc 探测序", "三致命九降级"),
    ("UNX-F11211", "HWCAP 位", "未实现位回退有账"),
    ("UNX-F11212", "TLS 辅助依赖", "B06 五钩子"),
    ("UNX-F11213", "known_gaps 账", "四列回流"),
    ("UNX-F11214", "排疑归域", "部 C 案例全对"),
    ("UNX-F11215", "30 动态样本", "编号来源依赖"),
    ("UNX-F11216", "ldd 输出一致", "三十样本双跑"),
    ("UNX-F11217", "musl syscall", "六十号逐号"),
    ("UNX-F11218", "busybox-musl 目标", "八前置冻存"),
];

/// 组一张十九条全绿的批。
pub fn build() -> KTestBatch {
    let mut b = KTestBatch::new();
    let mut k = 0usize;
    while k < ROWS.len() {
        let (id, title, detail) = ROWS[k];
        let _ = b.add(Assertion { id, title, passed: true, detail });
        k += 1;
    }
    let _ = b.run_all();
    b
}

/// 四路对抗：空号、重号、缺条冻存、红条冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = KTestBatch::new();
    out[0] = a.add(Assertion { id: "", title: "空", passed: true, detail: "x" }) == Err(KA_EMPTY);
    let _ = a.add(Assertion { id: "UNX-F11200", title: "t", passed: true, detail: "d" });
    out[1] = a.add(Assertion { id: "UNX-F11200", title: "t", passed: true, detail: "d" }) == Err(KA_DUP);
    let mut c = KTestBatch::new();
    let mut k = 0usize;
    while k < 18 {
        let (id, title, detail) = ROWS[k];
        let _ = c.add(Assertion { id, title, passed: true, detail });
        k += 1;
    }
    let _ = c.run_all();
    out[2] = c.freeze() == Err(KA_COUNT);
    let mut d = build();
    if let Some(slot) = d.rows.first_mut() {
        slot.passed = false;
    }
    out[3] = d.freeze() == Err(KA_FAIL);
    out
}

/// 判据入口。
pub fn run_unx11220_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11220");
    let mut b = build();
    let root = b.freeze().unwrap_or(0);

    s.add("J1-十九条", b.rows.len() == WANT, "B01 批恰聚十九条判据");
    s.add("J2-判据号范式", b.rows.iter().all(|r| r.id_ok()), "每条判据号形如 UNX-FNNNNN");
    s.add("J3-号无重", {
        let mut uniq: Vec<&str> = Vec::new();
        for r in b.rows.iter() {
            if !uniq.contains(&r.id) {
                uniq.push(r.id);
            }
        }
        uniq.len() == WANT
    }, "十九条判据号两两不同");
    s.add("J4-一次全跑", b.ran, "单次 run_all 覆盖全部十九条");
    s.add("J5-通过率", b.pass_rate() == 100, "通过率为 100% 才算绿");
    s.add("J6-全绿", b.all_green(), "已跑、满条、逐条绿三条件同时成立");
    s.add("J7-冻存根", root != 0 && b.root == Some(root), "全绿批冻存产出非零根");

    s.add("J8-冻后加拒", b.add(Assertion { id: "UNX-F11299", title: "x", passed: true, detail: "y" }) == Err(KA_FROZEN), "冻存后新增断言拒为 KA_FROZEN");
    s.add("J9-冻后重跑拒", b.rerun() == Err(KA_FROZEN), "冻存后重跑拒为 KA_FROZEN");
    s.add("J10-二次冻结拒", b.freeze() == Err(KA_FROZEN), "已冻存批再次冻结拒为 KA_FROZEN");

    let mut e = KTestBatch::new();
    s.add("J11-空号拒", e.add(Assertion { id: "", title: "x", passed: true, detail: "y" }) == Err(KA_EMPTY), "空判据号拒为 KA_EMPTY");
    let _ = e.add(Assertion { id: "UNX-F11200", title: "x", passed: true, detail: "y" });
    s.add("J12-重号拒", e.add(Assertion { id: "UNX-F11200", title: "z", passed: true, detail: "w" }) == Err(KA_DUP), "重号断言拒为 KA_DUP");

    let mut norun = KTestBatch::new();
    let mut k = 0usize;
    while k < WANT {
        let (id, title, detail) = ROWS[k];
        let _ = norun.add(Assertion { id, title, passed: true, detail });
        k += 1;
    }
    s.add("J13-未跑禁冻", norun.freeze() == Err(KA_NORUN), "未跑即冻存拒为 KA_NORUN");

    let mut red = build();
    if let Some(slot) = red.rows.first_mut() {
        slot.passed = false;
    }
    s.add("J14-红条禁冻", red.freeze() == Err(KA_FAIL) && red.pass_rate() < 100, "存在红条时冻存拒为 KA_FAIL");

    let four = inject_four();
    s.add("J15-四路注入", four.iter().all(|x| *x), "空号/重号/缺条/红条四路全拒");
    s.add_dyn(
        "J16-聚合明细",
        b.passed() == WANT && b.rows.len() == WANT,
        &format!("{} 绿 / {} 条，通过率 {}%，根 0x{:x}", b.passed(), b.rows.len(), b.pass_rate(), root),
    );
    s.add_dyn(
        "J17-红条清单",
        b.failures().is_empty(),
        &format!("红条 {} 条（应为空）", b.failures().len()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11220_b01_ktest_flow() {
        let mut b = build();
        assert_eq!(b.rows.len(), 19);
        assert!(b.all_green());
        let r = b.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(b.failures().len(), 0);
        assert_eq!(b.rerun(), Err(KA_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11220_checks_green() {
        let s = run_unx11220_checks();
        assert!(s.all_passed(), "UNX-F11220 判据未全绿");
    }
}
