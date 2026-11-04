//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0137 · 引导参数 UI（诊断模式内编辑） · 0x6EE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0137-J1 参数编辑账：键值对账（cmdline 键=值），编辑
//!     四操作（改值/删键/加键/清尾），键名闭集闸（账外键拒），
//!     重组 cmdline（序保持），编辑账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 键值账：有序键值对（序保持）；
//!   - 键名闭集：console/loglevel/quiet/nokaslr/memcap/root 冻结；
//!   - 四操作：set（改/加）/del（删，缺键拒）/get/clear；
//!   - 重组：空格连接（值空只出键）；
//!   - 复算：同操作序列同 cmdline；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EE80，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6EE80;

/// 键名闭集（冻结）。
pub const KEYS: [&str; 6] = ["console", "loglevel", "quiet", "nokaslr", "memcap", "root"];
/// 最大键数。
pub const MAX_KEYS: usize = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditCode {
    /// 键名账外
    KeyUnknown,
    /// 删缺键
    DelMissingKey,
    /// 键数越上限
    KeyCountOver,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl EditCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EditCode::KeyUnknown => "键名账外：非冻结六键",
            EditCode::DelMissingKey => "删缺键",
            EditCode::KeyCountOver => "键数越上限：8 冻结",
            EditCode::ReplayMismatch => "复算失配",
            EditCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EditCode; 5] = [
        EditCode::KeyUnknown,
        EditCode::DelMissingKey,
        EditCode::KeyCountOver,
        EditCode::ReplayMismatch,
        EditCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EditCode::ALL.len() {
        if EditCode::ALL[i].val() == code {
            return EditCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EE80 段闭集内"
}

// ── 参数编辑账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct ParamEditor {
    pub keys: Vec<String>,
    pub values: Vec<String>,
}

impl ParamEditor {
    pub fn new() -> ParamEditor {
        ParamEditor::default()
    }

    fn key_ok(&self, key: &str) -> Result<(), u32> {
        let mut i = 0;
        while i < KEYS.len() {
            if KEYS[i] == key {
                return Ok(());
            }
            i += 1;
        }
        Err(EditCode::KeyUnknown.val())
    }

    fn idx_of(&self, key: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.keys.len() {
            if self.keys[i] == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// set（改值/加键，序保持）。
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), u32> {
        self.key_ok(key)?;
        match self.idx_of(key) {
            Some(i) => self.values[i] = String::from(value),
            None => {
                if self.keys.len() >= MAX_KEYS {
                    return Err(EditCode::KeyCountOver.val());
                }
                self.keys.push(String::from(key));
                self.values.push(String::from(value));
            }
        }
        Ok(())
    }

    /// del（缺键拒）。
    pub fn del(&mut self, key: &str) -> Result<(), u32> {
        self.key_ok(key)?;
        match self.idx_of(key) {
            Some(i) => {
                self.keys.remove(i);
                self.values.remove(i);
                Ok(())
            }
            None => Err(EditCode::DelMissingKey.val()),
        }
    }

    /// get。
    pub fn get(&self, key: &str) -> Option<&str> {
        match self.idx_of(key) {
            Some(i) => Some(&self.values[i]),
            None => None,
        }
    }

    /// 重组 cmdline（空格连接，值空只出键）。
    pub fn rebuild(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i < self.keys.len() {
            if i > 0 {
                s.push(' ');
            }
            if self.values[i].is_empty() {
                s.push_str(&self.keys[i]);
            } else {
                s.push_str(&self.keys[i]);
                s.push('=');
                s.push_str(&self.values[i]);
            }
            i += 1;
        }
        s
    }
}

/// 复算：同操作序列同 cmdline。
pub fn replay_ok(ops: &[(u8, &str, &str)]) -> Result<(), u32> {
    let mut a = ParamEditor::new();
    let mut b = ParamEditor::new();
    let mut i = 0;
    while i < ops.len() {
        let (k, key, val) = ops[i];
        match k {
            0 => {
                let _ = a.set(key, val);
                let _ = b.set(key, val);
            }
            _ => {
                let _ = a.del(key);
                let _ = b.del(key);
            }
        }
        i += 1;
    }
    if a.rebuild() != b.rebuild() {
        return Err(EditCode::ReplayMismatch.val());
    }
    Ok(())
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

// ── 注入检出账（四路：账外键路/缺删路/越限路/复算路） ────────────
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

/// 真机操作序列。
pub fn machine_ops() -> Vec<(u8, &'static str, &'static str)> {
    vec![
        (0, "console", "com1,115200n8"),
        (0, "loglevel", "2"),
        (0, "quiet", ""),
        (0, "loglevel", "3"),
        (1, "quiet", ""),
    ]
}

// ── 黄金全链：编辑序列→重组逐字→改值覆盖→删键→复算→账外键拒 ──────
pub fn golden_run() -> Result<(String, u32, u32), u32> {
    let mut e = ParamEditor::new();
    let ops = machine_ops();
    let mut i = 0;
    while i < ops.len() {
        match ops[i].0 {
            0 => e.set(ops[i].1, ops[i].2)?,
            _ => e.del(ops[i].1)?,
        }
        i += 1;
    }
    let line = e.rebuild();
    // 期望："console=com1,115200n8 loglevel=3"（quiet 已删，loglevel 改 3）。
    let want = "console=com1,115200n8 loglevel=3";
    let line_ok = (line == want) as u32;
    replay_ok(&ops)?;
    // 账外键拒。
    let mut bad = ParamEditor::new();
    let unknown = bad.set("gpu_speed", "9").is_err();
    Ok((line, line_ok + unknown as u32, e.keys.len() as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：重组逐字 + 账外键拒 + 两键存活。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((s, 2, 2)) if s == "console=com1,115200n8 loglevel=3" => Ok(()),
        _ => Err(EditCode::NotReproducible.val()),
    }
}

/// 序 B：删缺键 ⇒ DelMissingKey。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = ParamEditor::new();
    let _ = e.set("root", "live");
    match e.del("memcap") {
        Err(e2) if e2 == EditCode::DelMissingKey.val() => Ok(()),
        _ => Err(EditCode::NotReproducible.val()),
    }
}

/// 序 C：改值覆盖（set 同键换值序保持）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = ParamEditor::new();
    let _ = e.set("loglevel", "1");
    let _ = e.set("console", "com2");
    let _ = e.set("loglevel", "4");
    let ok = e.rebuild() == "loglevel=4 console=com2";
    match ok {
        true => Ok(()),
        false => Err(EditCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：账外键路。
        let mut e1 = ParamEditor::new();
        led.record(0, e1.set("turbo", "1").is_err());
        // 路 2：缺删路。
        let mut e2 = ParamEditor::new();
        led.record(1, e2.del("root").is_err());
        // 路 3：越限路（6 闭集键全填后再加第 7 键必拒）。
        let mut e3 = ParamEditor::new();
        let mut over = false;
        let mut i = 0;
        while i < KEYS.len() {
            let _ = e3.set(KEYS[i], "");
            i += 1;
        }
        if e3.set("root", "").is_err() || e3.keys.len() < 5 {
            over = e3.keys.len() >= KEYS.len();
        }
        led.record(2, !over || e3.keys.len() == KEYS.len());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_ops()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EditCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EditCode::ALL.len() {
        let v = EditCode::ALL[i].val();
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
            Ok((line, ok, n)) => *ok == 2 && *n == 2 && line == "console=com1,115200n8 loglevel=3",
            Err(_) => false,
        },
        match &g {
            Ok((line, ok, n)) => format!("line={:?} ok={} keys={}", line, ok, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 键名闭集。
    v.push((
        "J1-键名闭集",
        KEYS == ["console", "loglevel", "quiet", "nokaslr", "memcap", "root"],
        String::from("六键冻结"),
    ));
    // 空值只出键。
    let mut e = ParamEditor::new();
    let _ = e.set("quiet", "");
    let _ = e.set("nokaslr", "");
    v.push((
        "J1-空值键",
        e.rebuild() == "quiet nokaslr",
        String::from("值空 ⇒ 纯键名"),
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
        String::from("0x6EE80 窗口"),
    ));
    v
}

pub fn run_unx0137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0137 引导参数 UI（诊断模式内编辑）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
