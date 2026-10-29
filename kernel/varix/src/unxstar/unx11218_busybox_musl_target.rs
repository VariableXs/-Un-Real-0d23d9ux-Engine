//! UNX-F11218 busybox-musl 最小系统构建目标登记（0x1f818 段）
//!
//! 锚点：UNX-F11218-J1 最小系统形态（busybox-musl 组合）构建目标与前置
//! 条件清单落账，作为 B29 自举集成批的输入冻结。
//! 域：CoRun·UNX-C5-B01——目标形态三列（三元组/库形态/applet 数）与八条
//! 前置一并立账；前置未齐不得冻结为 B29 输入，冻结后任何改写一律拒。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f818;
pub const BT_REQ: u32 = SEG_BASE + 0x01; // 前置名空
pub const BT_DUP: u32 = SEG_BASE + 0x02; // 前置重录
pub const BT_UNMET: u32 = SEG_BASE + 0x03; // 前置未满足却定稿
pub const BT_SHAPE: u32 = SEG_BASE + 0x04; // 形态三列非法
pub const BT_COUNT: u32 = SEG_BASE + 0x05; // 前置条数不足
pub const BT_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const BT_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 最小前置条数。
pub const MIN_REQS: usize = 8;
pub const CAP: usize = 12;
pub const BT_TAG: u64 = 0x1f818c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 构建目标形态：名、三元组、库形态、applet 数。
#[derive(Clone, Debug)]
pub struct Shape {
    pub name: &'static str,
    pub triple: &'static str,
    pub libc: &'static str,
    pub applets: u32,
}

impl Shape {
    pub fn legal(&self) -> bool {
        !self.name.is_empty() && !self.triple.is_empty() && self.libc == "musl" && self.applets == 40
    }

    pub fn render(&self) -> String {
        format!("{}|{}|{}|{}", self.name, self.triple, self.libc, self.applets)
    }
}

/// 单条前置条件。
#[derive(Clone, Debug)]
pub struct Req {
    pub name: &'static str,
    pub met: bool,
    pub proof: &'static str,
}

impl Req {
    pub fn closed(&self) -> bool {
        self.met && !self.proof.is_empty()
    }
}

/// 目标账。
#[derive(Clone, Debug)]
pub struct BuildTarget {
    pub shape: Shape,
    pub reqs: Vec<Req>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl BuildTarget {
    pub fn new(name: &'static str, triple: &'static str, libc: &'static str, applets: u32) -> BuildTarget {
        BuildTarget {
            shape: Shape { name, triple, libc, applets },
            reqs: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    /// 加前置：冻结/空名/重录/超容四路拒。
    pub fn add_req(&mut self, name: &'static str, met: bool, proof: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(BT_FROZEN);
        }
        if name.is_empty() {
            return Err(BT_REQ);
        }
        if self.reqs.iter().any(|r| r.name == name) {
            return Err(BT_DUP);
        }
        if self.reqs.len() >= CAP {
            return Err(BT_OVER);
        }
        self.reqs.push(Req { name, met, proof });
        Ok(())
    }

    /// 补证：把某前置置满足并落证据。
    pub fn prove(&mut self, name: &'static str, proof: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(BT_FROZEN);
        }
        if proof.is_empty() {
            return Err(BT_REQ);
        }
        let mut k = 0usize;
        while k < self.reqs.len() {
            if self.reqs[k].name == name {
                self.reqs[k].met = true;
                self.reqs[k].proof = proof;
                return Ok(());
            }
            k += 1;
        }
        Err(BT_REQ)
    }

    /// 未满足前置名。
    pub fn unmet(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.reqs.len() {
            if !self.reqs[k].closed() {
                out.push(self.reqs[k].name);
            }
            k += 1;
        }
        out
    }

    /// 就绪：形态合法、前置达最低数、全部满足。
    pub fn ready(&self) -> bool {
        self.shape.legal() && self.reqs.len() >= MIN_REQS && self.unmet().is_empty()
    }

    /// 冻结为 B29 输入：未就绪必拒。
    pub fn freeze_input(&mut self) -> Result<u64, u32> {
        if !self.ready() {
            return Err(BT_UNMET);
        }
        if self.frozen {
            return Err(BT_FROZEN);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = BT_TAG;
        let nb = self.shape.name.as_bytes();
        let mut b = 0usize;
        while b < nb.len() {
            h = (h ^ nb[b] as u64).wrapping_mul(FP_STEP);
            b += 1;
        }
        let tb = self.shape.triple.as_bytes();
        b = 0;
        while b < tb.len() {
            h = (h ^ tb[b] as u64).wrapping_mul(FP_STEP);
            b += 1;
        }
        h = (h ^ self.shape.applets as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.reqs.len() {
            let rb = self.reqs[k].name.as_bytes();
            b = 0;
            while b < rb.len() {
                h = (h ^ rb[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.reqs[k].met as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 八条前置：名、证据。
pub const REQS: [(&str, &str); 8] = [
    ("toolchain", "x86_64-linux-musl-gcc 13.2 可复现"),
    ("libc-headers", "musl 1.2.5 头文件树钉定"),
    ("initramfs", "cpio 新式格式 4KiB 对齐"),
    ("applet-manifest", "40 个 applet 清单与开关一致"),
    ("tls-helper", "B06 五个钩子全接线"),
    ("dynlink", "ld-musl-x86_64.so.1 路径固定"),
    ("probe-suite", "C1 探针 12 键全读"),
    ("ab-label", "A/B 标签 ab-11218 写入引导参数"),
];

/// 组一张就绪的目标账。
pub fn build() -> BuildTarget {
    let mut t = BuildTarget::new("busybox-musl", "x86_64-linux-musl", "musl", 40);
    let mut k = 0usize;
    while k < REQS.len() {
        let (n, p) = REQS[k];
        let _ = t.add_req(n, true, p);
        k += 1;
    }
    t
}

/// 四路对抗：空名、重录、未就绪冻结、冻结后改写；全拒返回真。
pub fn inject_four(t: &mut BuildTarget) -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = t.add_req("", true, "空名") == Err(BT_REQ);
    out[1] = t.add_req("toolchain", true, "重录") == Err(BT_DUP);
    let mut thin = BuildTarget::new("busybox-musl", "x86_64-linux-musl", "musl", 40);
    let _ = thin.add_req("toolchain", true, "仅一条");
    out[2] = thin.freeze_input() == Err(BT_UNMET);
    let _ = t.freeze_input();
    out[3] = t.add_req("late", true, "冻结后加") == Err(BT_FROZEN);
    out
}

/// 判据入口：J1–J15。
pub fn run_unx11218_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11218");
    let mut t = build();
    let root = t.freeze_input().unwrap_or(0);

    s.add("J1-形态三列", t.shape.legal(), "三元组/库形态/applet 数三列齐全且合法");
    s.add("J2-前置八条", t.reqs.len() == MIN_REQS, "前置恰为八条，达最低条数");
    s.add("J3-前置闭合", t.unmet().is_empty(), "八条前置均有证据且置满足");
    s.add("J4-就绪", t.ready(), "形态合法、条数达线、无未满足项");
    s.add("J5-冻结根", root != 0 && t.root == Some(root), "冻结产出非零根并留在账上");
    s.add("J6-冻结后加拒", t.add_req("x", true, "y") == Err(BT_FROZEN), "冻结后再加前置一律拒为 BT_FROZEN");
    s.add("J7-冻结后补证拒", t.prove("toolchain", "z") == Err(BT_FROZEN), "冻结后再补证一律拒为 BT_FROZEN");
    s.add("J8-二次冻结拒", t.freeze_input() == Err(BT_FROZEN), "已冻结账再次冻结拒为 BT_FROZEN");

    let mut u = BuildTarget::new("busybox-musl", "x86_64-linux-musl", "musl", 40);
    s.add("J9-空名拒", u.add_req("", true, "p") == Err(BT_REQ), "空前置名拒为 BT_REQ");
    let _ = u.add_req("a", true, "p");
    s.add("J10-重录拒", u.add_req("a", true, "q") == Err(BT_DUP), "同名前置重录拒为 BT_DUP");
    s.add("J11-未就绪冻结拒", u.freeze_input() == Err(BT_UNMET), "未就绪账冻结拒为 BT_UNMET");

    let bad = Shape { name: "busybox-musl", triple: "x86_64-linux-musl", libc: "musl", applets: 39 };
    s.add("J12-非法形态", !bad.legal(), "applet 数 39 不等 40，形态判非法");
    let mut inv = build();
    let four = inject_four(&mut inv);
    s.add("J13-四路注入", four.iter().all(|x| *x), "空名/重录/未就绪冻结/冻结后改写四路全拒");

    let mut d = build();
    let r2 = d.freeze_input().unwrap_or(1);
    s.add_dyn(
        "J14-根复现",
        r2 == root,
        &format!("同构账两次冻结根一致：0x{:x} vs 0x{:x}", root, r2),
    );
    s.add_dyn(
        "J15-明细",
        t.reqs.len() == 8 && root == t.fold(),
        &format!("前置 {} 条，根 0x{:x}，重折叠一致", t.reqs.len(), root),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11218_busybox_musl_flow() {
        let mut t = build();
        assert!(t.ready());
        assert!(t.frozen == false);
        let r = t.freeze_input().unwrap_or(0);
        assert!(r != 0);
        assert!(t.frozen);
        assert_eq!(t.unmet().len(), 0);
        let mut g = build();
        assert!(inject_four(&mut g).iter().all(|x| *x));
        assert_eq!(g.add_req("after", true, "p"), Err(BT_FROZEN));
    }

    #[test]
    fn unx11218_checks_green() {
        let s = run_unx11218_checks();
        assert!(s.all_passed(), "UNX-F11218 判据未全绿");
    }
}
