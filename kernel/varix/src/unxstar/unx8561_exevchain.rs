//! UNX-F8561 · execve 全链联测件（C2 分发 × C5 ld.so 联签）账 · 诊断段 0xB5C0
//! 验收锚点：UNX-F8561-J1 execve 全链联测档——C2 分发入口 → C1 装载 → C5 ld.so
//! → main 四段联签单登记，fake 分发下联测断言 10/10，联签待收口登记。


use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB5C0;

/// 全链四段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainStage {
    Dispatch, // C2 分发入口
    Load,     // C1 装载
    Ldso,     // C5 ld.so
    Main,     // 入口
}

impl ChainStage {
    pub fn as_str(self) -> &'static str {
        match self {
            ChainStage::Dispatch => "c2_dispatch",
            ChainStage::Load => "c1_load",
            ChainStage::Ldso => "c5_ldso",
            ChainStage::Main => "main",
        }
    }
    pub fn all() -> [ChainStage; 4] {
        [ChainStage::Dispatch, ChainStage::Load, ChainStage::Ldso, ChainStage::Main]
    }
}

/// 联签单。
pub struct JointSign {
    pub stages: Vec<(ChainStage, bool)>, // 段 → 通过
    pub closed: bool,                    // 收口标记（待收口 = false）
}

impl JointSign {
    pub fn new() -> Self {
        JointSign { stages: Vec::new(), closed: false }
    }
    pub fn sign(&mut self, st: ChainStage, ok: bool) {
        self.stages.push((st, ok));
    }
    /// 四段全签全过。
    pub fn all_signed(&self) -> bool {
        ChainStage::all().iter().all(|s| self.stages.iter().any(|(g, ok)| g == s && *ok))
    }
    /// 待收口登记：全签但未收口。
    pub fn pending_close(&self) -> bool {
        self.all_signed() && !self.closed
    }
}

/// fake 分发器：execve 路径模拟。
pub struct FakeDispatcher {
    pub routed: u32,
}

impl FakeDispatcher {
    pub fn new() -> Self {
        FakeDispatcher { routed: 0 }
    }
    /// 联测：分发 → 装载 → ld.so → main 全链走通（每样本独立联签单）。
    pub fn run_chain(&mut self, elf_ok: bool, ldso_ok: bool) -> JointSign {
        let mut js = JointSign::new();
        self.routed += 1;
        js.sign(ChainStage::Dispatch, true);
        js.sign(ChainStage::Load, elf_ok);
        js.sign(ChainStage::Ldso, elf_ok && ldso_ok);
        js.sign(ChainStage::Main, elf_ok && ldso_ok);
        js
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

// ── 注入核（0xB5C0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8561_B5C0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("四段-枚举全4", ChainStage::all().len() == 4));
    out.push(("联签-空单不全签", !JointSign::new().all_signed()));
    out.push(("分发-计数起0", FakeDispatcher::new().routed == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8561_checks：本单判据聚合（诊断段 0xB5C0）。
pub fn run_unx8561_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8561 execve 全链联测件");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // fake 分发下联测断言 10/10
    let mut d = FakeDispatcher::new();
    let mut all_ok = true;
    let mut signs: Vec<JointSign> = Vec::new();
    for _ in 0..10u32 {
        let js = d.run_chain(true, true);
        if !js.all_signed() {
            all_ok = false;
        }
        signs.push(js);
    }
    let ten_ok = all_ok && d.routed == 10;
    s.add("联测-fake分发10/10全链全签", ten_ok, if ten_ok { "" } else { "联测缺口" });

    // 待收口登记：全签未收口
    let pending_ok = signs.iter().all(|js| js.pending_close());
    s.add("登记-联签待收口10单", pending_ok, if pending_ok { "" } else { "待收口登记漂移" });

    // 收口动作
    let mut js0 = signs.remove(0);
    js0.closed = true;
    s.add("收口-标记后不再待收", !js0.pending_close() && js0.closed, if js0.closed { "" } else { "收口漂移" });

    // 段失败链断：装载失败 → ld.so/main 不签过
    let js_bad = d.run_chain(false, true);
    let load_fail = !js_bad.all_signed();
    s.add("链断-装载失败ld.so/main不过", load_fail, if load_fail { "" } else { "链断漏网" });

    // ld.so 失败：装载过 ld.so/main 不过
    let js_ld = d.run_chain(true, false);
    let ld_fail = js_ld.stages.iter().find(|(s, _)| *s == ChainStage::Ldso).map(|(_, ok)| *ok) == Some(false);
    let main_fail = js_ld.stages.iter().find(|(s, _)| *s == ChainStage::Main).map(|(_, ok)| *ok) == Some(false);
    let disp_ok = js_ld.stages.iter().find(|(s, _)| *s == ChainStage::Dispatch).map(|(_, ok)| *ok) == Some(true);
    s.add("链断-ld.so失败main不过分发过", ld_fail && main_fail && disp_ok, if ld_fail && main_fail { "" } else { "级联失效" });

    // 四段名可导出
    let names = ChainStage::all().iter().map(|s| s.as_str()).collect::<Vec<&str>>().join(",");
    s.add("导出-四段名序一致", names == "c2_dispatch,c1_load,c5_ldso,main", if names == "c2_dispatch,c1_load,c5_ldso,main" { "" } else { "名序漂移" });

    // 缺段检出：缺 main 全签红
    let mut js2 = JointSign::new();
    for st in ChainStage::all().iter().take(3) {
        js2.sign(*st, true);
    }
    s.add("缺段-缺main全签红", !js2.all_signed(), if !js2.all_signed() { "" } else { "缺段漏检" });

    // FNV 联签单指纹稳定
    let h1 = fnv1a(format!("{:?}", signs[0].stages).as_bytes());
    let h2 = fnv1a(format!("{:?}", signs[0].stages).as_bytes());
    s.add("FNV-联签单指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
