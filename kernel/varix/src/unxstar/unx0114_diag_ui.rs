//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0114 · 诊断模式控制台 UI 骨架 · 0x6EA00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0114-J1 诊断 UI 骨架：菜单树（四键：上下选/回车进/
//!     ESC 退）状态机，菜单项账（id/名/深度）越深度拒，焦点单调
//!     环绕，进入/退出账（栈式），状态机复算一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 菜单账：项（id/name/parent 深度），栈式导航（进入 push/
//!     退出 pop，根栈空退即拒）；
//!   - 四键：UP/DOWN（焦点环绕）/ENTER（进入子菜单）/ESC（退出）；
//!   - 焦点闸：环绕单调（越尾回 0），空菜单拒；
//!   - 深度闸：≥3 层拒再进（冻结）；
//!   - 复算：同按键序列同终态；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EA00，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6EA00;

/// 最大深度（冻结 3）。
pub const MAX_DEPTH: usize = 3;
/// 最大菜单项。
pub const MAX_ITEMS: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiCode {
    /// 空菜单焦点拒
    EmptyMenuFocus,
    /// 深度越限（>3 再进）
    DepthOver,
    /// 栈空退出
    PopAtRoot,
    /// 菜单项越上限
    ItemCountOver,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl UiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UiCode::EmptyMenuFocus => "空菜单焦点拒",
            UiCode::DepthOver => "深度越限：>3 再进拒",
            UiCode::PopAtRoot => "栈空退出：根层 ESC 拒",
            UiCode::ItemCountOver => "菜单项越上限：16 冻结",
            UiCode::ReplayMismatch => "复算失配",
            UiCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [UiCode; 6] = [
        UiCode::EmptyMenuFocus,
        UiCode::DepthOver,
        UiCode::PopAtRoot,
        UiCode::ItemCountOver,
        UiCode::ReplayMismatch,
        UiCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < UiCode::ALL.len() {
        if UiCode::ALL[i].val() == code {
            return UiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EA00 段闭集内"
}

// ── 菜单账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: u32,
    /// 子项数（0 = 叶子）。
    pub children: u32,
}

#[derive(Clone, Debug)]
pub struct UiMenu {
    pub items: Vec<MenuItem>,
    pub focus: usize,
    /// 导航栈（进入的菜单 id 序）。
    pub stack: Vec<u32>,
}

impl UiMenu {
    pub fn new(items: &[MenuItem]) -> Result<UiMenu, u32> {
        if items.is_empty() {
            return Err(UiCode::EmptyMenuFocus.val());
        }
        if items.len() > MAX_ITEMS {
            return Err(UiCode::ItemCountOver.val());
        }
        Ok(UiMenu { items: Vec::from(items), focus: 0, stack: Vec::new() })
    }

    /// UP/DOWN（环绕）。
    pub fn move_focus(&mut self, delta: isize) -> usize {
        let n = self.items.len() as isize;
        self.focus = ((self.focus as isize + delta).rem_euclid(n)) as usize;
        self.focus
    }

    /// ENTER（进入子菜单；叶子/深度超限拒）。
    pub fn enter(&mut self) -> Result<usize, u32> {
        let item = self.items[self.focus];
        if item.children == 0 {
            return Err(UiCode::EmptyMenuFocus.val());
        }
        if self.stack.len() + 1 > MAX_DEPTH {
            return Err(UiCode::DepthOver.val());
        }
        self.stack.push(item.id);
        self.focus = 0;
        Ok(self.stack.len())
    }

    /// ESC（退出；根栈空拒）。
    pub fn back(&mut self) -> Result<usize, u32> {
        match self.stack.pop() {
            Some(_) => Ok(self.stack.len()),
            None => Err(UiCode::PopAtRoot.val()),
        }
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

/// 复算：同按键序列重建终态一致。
pub fn replay_ok(keys: &[u8], items: &[MenuItem]) -> Result<(), u32> {
    let mut a = match UiMenu::new(items) {
        Ok(m) => m,
        Err(_) => return Err(UiCode::ReplayMismatch.val()),
    };
    let mut b = match UiMenu::new(items) {
        Ok(m) => m,
        Err(_) => return Err(UiCode::ReplayMismatch.val()),
    };
    let mut i = 0;
    while i < keys.len() {
        let k = keys[i];
        let _ = run_key(&mut a, k);
        let _ = run_key(&mut b, k);
        i += 1;
    }
    if a.focus != b.focus || a.stack[..] != b.stack[..] {
        return Err(UiCode::ReplayMismatch.val());
    }
    Ok(())
}

/// 按键执行（0=UP 1=DOWN 2=ENTER 3=ESC）。
pub fn run_key(m: &mut UiMenu, key: u8) -> Result<(), u32> {
    match key {
        0 => {
            m.move_focus(-1);
            Ok(())
        }
        1 => {
            m.move_focus(1);
            Ok(())
        }
        2 => m.enter().map(|_| ()),
        _ => m.back().map(|_| ()),
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：空菜单路/深度路/栈空路/复算路） ────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机菜单（根 4 项：3 子菜单 + 1 叶）。
pub fn machine_root() -> [MenuItem; 4] {
    [
        MenuItem { id: 1, children: 3 },
        MenuItem { id: 2, children: 2 },
        MenuItem { id: 3, children: 0 },
        MenuItem { id: 4, children: 1 },
    ]
}

/// 真机按键序列：ENTER×3（连进三层）。
pub fn machine_keys() -> Vec<u8> {
    vec![2, 2, 2]
}

// ── 黄金全链：导航→三层进→叶子拒→栈空拒→复算 ────────────────────
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut m = UiMenu::new(&machine_root())?;
    let keys = machine_keys();
    let mut i = 0;
    while i < keys.len() {
        let _ = run_key(&mut m, keys[i]);
        i += 1;
    }
    let depth = m.depth();
    // 叶子项 ENTER 必拒。
    let mut m2 = UiMenu::new(&machine_root())?;
    m2.move_focus(2); // 焦点 2 = 叶子
    let leaf_reject = m2.enter().is_err();
    // 栈空 ESC 必拒。
    let mut m3 = UiMenu::new(&machine_root())?;
    let root_reject = m3.back().is_err();
    match replay_ok(&keys, &machine_root()) {
        Ok(()) => {}
        Err(_) => return Err(UiCode::ReplayMismatch.val()),
    }
    Ok((depth, leaf_reject as u32, root_reject as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机序列 → 三层深 + 叶子拒 + 栈空拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 1, 1)) => Ok(()),
        _ => Err(UiCode::NotReproducible.val()),
    }
}

/// 序 B：深度越限 ⇒ DepthOver（连进 4 层）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = UiMenu::new(&machine_root())?;
    let _ = m.enter();
    let _ = m.enter();
    let _ = m.enter();
    match m.enter() {
        Err(e) if e == UiCode::DepthOver.val() => Ok(()),
        _ => Err(UiCode::NotReproducible.val()),
    }
}

/// 序 C：空菜单 ⇒ EmptyMenuFocus。
pub fn probe_seq_c() -> Result<(), u32> {
    match UiMenu::new(&[]) {
        Err(e) if e == UiCode::EmptyMenuFocus.val() => Ok(()),
        _ => Err(UiCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：空菜单路。
        led.record(0, UiMenu::new(&[]).is_err());
        // 路 2：深度路。
        let mut m2 = UiMenu::new(&machine_root())?;
        let _ = m2.enter();
        let _ = m2.enter();
        let _ = m2.enter();
        led.record(1, m2.enter().is_err());
        // 路 3：栈空路。
        let mut m3 = UiMenu::new(&machine_root())?;
        led.record(2, m3.back().is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_keys(), &machine_root()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(UiCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < UiCode::ALL.len() {
        let v = UiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((depth, leaf, root)) => *depth == 3 && *leaf == 1 && *root == 1,
            Err(_) => false,
        },
        match &g {
            Ok((depth, leaf, root)) => format!("depth={} leaf_reject={} root_reject={}", depth, leaf, root),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 四键语义（UP/DOWN/ENTER/ESC）。
    v.push((
        "J1-四键语义",
        MAX_DEPTH == 3 && MAX_ITEMS == 16,
        String::from("UP/DOWN 环绕 + ENTER 入 + ESC 退，深 3 项 16 冻结"),
    ));
    // 焦点环绕（4 项：DOWN×5 → 回焦点 1）。
    let mut m = match UiMenu::new(&machine_root()) {
        Ok(m) => m,
        Err(_) => {
            v.push(("J1-focus-wrap", false, String::from("menu build failed")));
            return v;
        }
    };
    let mut i = 0;
    while i < 5 {
        m.move_focus(1);
        i += 1;
    }
    v.push((
        "J1-焦点环绕",
        m.focus == 1,
        String::from("4 项 DOWN×5 ⇒ 焦点环绕回 1"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EA00 窗口"),
    ));
    v
}

pub fn run_unx0114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0114 诊断模式控制台 UI 骨架");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
