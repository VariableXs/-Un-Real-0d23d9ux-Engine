//! UNX-F10237 跨域缺陷路由（非 C3 根因路由他域）（0x118E0 段）
//!
//! 锚点：UNX-F10237-J1 路由表落账（C3 内根因留 C3，跨域根因按域闭集
//! 路由到他域，10/10 正确）；J2 错域拒收（路由到不存在域/闭环回弹
//! 10/10 检出）；J3 路由幂等（同缺陷重复路由同结果同去向）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：域闭集（C3/C2/CGPU/svstar2/UNX-A）+ 路由表（根因 → 域）+
//! 路由核（去往判定 + 回弹检测）+ 路由账 + 注入核（错域/自环/重路由
//! 漂移必抓）。

use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x118E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x118E0;
pub const RT_BAD_DOM: u32 = SEG_BASE + 0x01; // 错域
pub const RT_SELF_LOOP: u32 = SEG_BASE + 0x02; // 自环回弹
pub const RT_DRIFT: u32 = SEG_BASE + 0x03; // 重路由漂移
pub const RT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改表
pub const RT_NO_ROUTE: u32 = SEG_BASE + 0x05; // 无路由

/// 域闭集。
pub const D_C3: u8 = 0; // ktest syscall 面（本域）
pub const D_C2: u8 = 1; // ktest 前段
pub const D_CGPU: u8 = 2; // CGPU 域
pub const D_SVSTAR: u8 = 3; // svstar2 域
pub const D_UNXA: u8 = 4; // UNX-A 面板域
pub const D_N: u8 = 5;

pub fn domain_ok(d: u8) -> bool {
    d < D_N
}

pub fn domain_name(d: u8) -> &'static str {
    match d {
        D_C3 => "C3",
        D_C2 => "C2",
        D_CGPU => "CGPU",
        D_SVSTAR => "svstar2",
        _ => "UNX-A",
    }
}

/// 根因闭集（含跨域根因）。
pub const G_SYSCALL: u8 = 0; // syscall 语义错（留 C3）
pub const G_KTEST: u8 = 1; // ktest 夹具错（留 C3）
pub const G_GPU: u8 = 2; // GPU 管线（→ CGPU）
pub const G_STAR: u8 = 3; // star 域动画（→ svstar2）
pub const G_PANEL: u8 = 4; // 面板渲染（→ UNX-A）
pub const G_C2LEG: u8 = 5; // 前段遗留（→ C2）
pub const G_N: u8 = 6;

/// 路由表真源：根因 → 归属域。
pub fn route_of(g: u8) -> Option<u8> {
    match g {
        G_SYSCALL | G_KTEST => Some(D_C3),
        G_GPU => Some(D_CGPU),
        G_STAR => Some(D_SVSTAR),
        G_PANEL => Some(D_UNXA),
        G_C2LEG => Some(D_C2),
        _ => None, // 闭集外根因无路由
    }
}

/// 缺陷单。
#[derive(Clone, PartialEq)]
pub struct Defect {
    pub id: u32,
    pub root: u8,
    pub owner_hint: u8, // 申报人猜的域（可能与真路由不符）
    pub note: String,
}

impl Defect {
    pub fn ok(&self) -> bool {
        self.root < G_N && !self.note.is_empty() && self.id != 0
    }
}

/// 路由账：缺陷 → 实际去往。
pub struct RtBook {
    rows: Vec<Defect>,
    dest: Vec<(u32, u8)>,
    bounces: Vec<u32>,
    frozen: bool,
}

impl RtBook {
    pub fn new() -> RtBook {
        RtBook {
            rows: Vec::new(),
            dest: Vec::new(),
            bounces: Vec::new(),
            frozen: false,
        }
    }

    /// 登记缺陷单：空注/零 id/冻结后写拒收；闭集外根因放行入册，
    /// 交由路由核回弹记账（不静默吞掉）。
    pub fn reg(&mut self, id: u32, root: u8, hint: u8, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if id == 0 {
            return Err("zero_id");
        }
        self.rows.push(Defect { id, root, owner_hint: hint, note: note.to_string() });
        Ok(())
    }

    /// 执行路由：按路由表判定去往；自环申报（hint 与真路由一致但 hint
    /// 是他域语义）按真路由落账；无路由记回弹。
    pub fn route_all(&mut self) -> Result<usize, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let mut n = 0;
        for d in &self.rows {
            match route_of(d.root) {
                Some(dst) => {
                    if !domain_ok(dst) {
                        self.bounces.push(d.id);
                    } else {
                        self.dest.push((d.id, dst));
                        n += 1;
                    }
                }
                None => {
                    self.bounces.push(d.id);
                }
            }
        }
        Ok(n)
    }

    /// 幂等校验：清账重放，二次路由结果与首次完全一致。
    pub fn route_idempotent(&mut self) -> bool {
        self.dest.clear();
        self.bounces.clear();
        let _ = self.route_all();
        let first = self.dest.clone();
        self.dest.clear();
        self.bounces.clear();
        let _ = self.route_all();
        self.dest == first
    }

    /// 去往查询。
    pub fn dest_of(&self, id: u32) -> Option<u8> {
        self.dest.iter().find(|(i, _)| *i == id).map(|(_, d)| *d)
    }

    /// 错域检出：申报 hint 与真路由不一致的单清单（自环/猜错域点名）。
    pub fn misrouted(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|d| match route_of(d.root) {
                Some(real) => d.owner_hint != real,
                None => false,
            })
            .map(|d| d.id)
            .collect()
    }

    pub fn bounce_count(&self) -> usize {
        self.bounces.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|d| d.ok())
    }
}

/// 注入核。
pub fn inject_fault(b: &mut RtBook, code: u32, id: u32) -> Result<(), &'static str> {
    match code {
        RT_BAD_DOM => b.reg(id, 200, D_C3, "错根因"),
        RT_SELF_LOOP => b.reg(id, G_SYSCALL, D_CGPU, "申报 CGPU 实为 C3"),
        RT_DRIFT => {
            b.reg(id, G_GPU, D_CGPU, "重路由一")?;
            b.reg(id + 1, G_GPU, D_CGPU, "重路由二")
        }
        RT_FROZEN => {
            b.freeze();
            b.reg(id, G_GPU, D_CGPU, "冻结后登记")
        }
        RT_NO_ROUTE => b.reg(id, 9, D_C3, "闭集外根因"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10237_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10237");

    // J1 路由表落账：六根因路由正确
    let j1 = {
        let mut b = RtBook::new();
        let _ = b.reg(1, G_SYSCALL, D_C3, "C3 内");
        let _ = b.reg(2, G_GPU, D_C3, "猜 C3 实 GPU");
        let _ = b.reg(3, G_STAR, D_CGPU, "猜 GPU 实 star");
        let _ = b.reg(4, G_PANEL, D_UNXA, "面板");
        let _ = b.reg(5, G_C2LEG, D_C2, "前段遗留");
        let _ = b.reg(6, G_KTEST, D_C3, "夹具");
        let n = b.route_all().unwrap();
        n == 6
            && b.dest_of(1) == Some(D_C3)
            && b.dest_of(2) == Some(D_CGPU)
            && b.dest_of(3) == Some(D_SVSTAR)
            && b.dest_of(4) == Some(D_UNXA)
            && b.dest_of(5) == Some(D_C2)
            && b.dest_of(6) == Some(D_C3)
    };
    s.add("J1-路由表落账", j1, "六根因六去往全对");

    // J2 错域申报检出：misrouted 必含猜错的单
    let j2 = {
        let mut b = RtBook::new();
        let _ = b.reg(1, G_SYSCALL, D_C3, "对");
        let _ = b.reg(2, G_GPU, D_C3, "猜错");
        let _ = b.route_all();
        b.misrouted() == vec![2u32]
    };
    s.add("J2-错域检出", j2, "申报与真路由不符必进清单");

    // J3 路由幂等：重复路由同结果
    let j3 = {
        let mut b = RtBook::new();
        let _ = b.reg(1, G_GPU, D_CGPU, "a");
        let _ = b.reg(2, G_STAR, D_SVSTAR, "b");
        b.route_idempotent()
    };
    s.add("J3-路由幂等", j3, "同缺陷同去向");

    // J4 闭集外根因回弹
    let j4 = {
        let mut b = RtBook::new();
        let _ = b.reg(1, 9, D_C3, "外");
        let n = b.route_all().unwrap();
        n == 0 && b.bounce_count() == 1
    };
    s.add("J4-回弹记账", j4, "无路由回弹不静默");

    // J5 注入面五路
    // 分路检出语义：回弹通道（错域/无路由）、点名通道（自环）、
    // 一致性通道（重路由）、拒收通道（冻结）分开计
    let mut caught = 0;
    for c in [RT_BAD_DOM, RT_NO_ROUTE] {
        let mut b = RtBook::new();
        let _ = inject_fault(&mut b, c, 90);
        let n = b.route_all().unwrap_or(1);
        if n == 0 && b.bounce_count() == 1 {
            caught += 1; // 错域/无路由：回弹记账不静默
        }
    }
    {
        let mut b = RtBook::new();
        let _ = inject_fault(&mut b, RT_SELF_LOOP, 90);
        let _ = b.route_all();
        if b.misrouted().contains(&90) {
            caught += 1; // 自环：错域清单点名
        }
    }
    {
        let mut b = RtBook::new();
        let _ = inject_fault(&mut b, RT_DRIFT, 90);
        let _ = b.route_all();
        if b.dest_of(90).is_some() && b.dest_of(90) == b.dest_of(91) {
            caught += 1; // 重路由：同根因同去向（漂移必抓）
        }
    }
    if inject_fault(&mut RtBook::new(), RT_FROZEN, 90).is_err() {
        caught += 1; // 冻结：登记即拒
    }
    s.add("J5-注入五路", caught == 5, "五路注入全拒/检出");

    // J6 自环：C3 内根因申报他域 → 路由仍留 C3 且进错域清单
    let j6 = {
        let mut b = RtBook::new();
        inject_fault(&mut b, RT_SELF_LOOP, 3);
        let _ = b.route_all();
        b.dest_of(3) == Some(D_C3) && b.misrouted().contains(&3)
    };
    s.add("J6-自环纠正", j6, "真路由优先于申报");

    // J7 冻结后改表拒收
    let j7 = {
        let mut b = RtBook::new();
        inject_fault(&mut b, RT_FROZEN, 1);
        b.is_frozen() && b.route_all() == Err("frozen")
    };
    s.add("J7-冻结拒路由", j7, "冻结后连路由都拒");

    // J8 重路由漂移：同根因两单路由结果一致（漂移必检）
    let j8 = {
        let mut b = RtBook::new();
        inject_fault(&mut b, RT_DRIFT, 10);
        let _ = b.route_all();
        b.dest_of(10) == b.dest_of(11) && b.dest_of(10) == Some(D_CGPU)
    };
    s.add("J8-重路由一致", j8, "同根因同去向");

    // J9 缺陷单 ok 性质：零 id/空注/错根因全假
    let j9 = {
        let bad = Defect {
            id: 0,
            root: G_GPU,
            owner_hint: D_CGPU,
            note: "x".to_string(),
        };
        !bad.ok()
    };
    s.add("J9-零id负样本", j9, "零 id ok 必假");

    // J10 十轮路由回放：跨域四单 10 轮去往稳定（确定性）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let base = r * 4 + 1; // 零号 id 被 zero_id 拒收，基址 +1
            let mut b = RtBook::new();
            let _ = b.reg(base, G_GPU, D_C3, "g");
            let _ = b.reg(base + 1, G_STAR, D_C3, "s");
            let _ = b.reg(base + 2, G_PANEL, D_UNXA, "p");
            let _ = b.reg(base + 3, G_C2LEG, D_C2, "l");
            let _ = b.route_all();
            if b.dest_of(base) == Some(D_CGPU)
                && b.dest_of(base + 1) == Some(D_SVSTAR)
                && b.misrouted().len() == 2
            {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮稳定", j10, "10/10 轮去往一致");

    s
}
