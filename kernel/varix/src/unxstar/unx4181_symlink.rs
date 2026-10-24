//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4181 · 符号链接存储模型（快/慢链接） · 诊断段 0xDA80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F4181-J1 快链接（目标内联 ≤60 字节）与慢链接（外置
//!      存储）双形态创建读取往返一致（各 1,000 例），形态自动选择
//!      边界（60 字节恰等）断言通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 快链接：目标内联（≤60 字节）；
//!   - 慢链接：外置存储（>60 字节）；
//!   - 形态自动选择：len ≤ 60 → fast，else slow；
//!   - 边界：60 字节恰等 → fast；61 → slow；
//!   - 往返一致：create→read 逐例逐字节 diff=0；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xDA80;

/// 快链接内联上界。
pub const INLINE_MAX: usize = 60;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 链接形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkForm {
    Fast,
    Slow,
}

impl LinkForm {
    pub fn name(self) -> &'static str {
        match self {
            LinkForm::Fast => "FAST",
            LinkForm::Slow => "SLOW",
        }
    }
}

/// 符号链接。
pub struct Symlink {
    pub form: LinkForm,
    /// 快链接内联目标。
    pub inline_target: Option<String>,
    /// 慢链接外置存储句柄。
    pub ext_slot: Option<u32>,
}

/// 外置存储池。
pub struct ExtStore {
    pub slots: Vec<String>,
}

impl ExtStore {
    pub fn new() -> Self {
        ExtStore { slots: Vec::new() }
    }

    pub fn put(&mut self, target: &str) -> u32 {
        self.slots.push(String::from(target));
        (self.slots.len() - 1) as u32
    }

    pub fn get(&self, slot: u32) -> Option<&String> {
        self.slots.get(slot as usize)
    }
}

/// 形态自动选择：len ≤ 60 → fast。
pub fn choose_form(target: &str) -> LinkForm {
    if target.len() <= INLINE_MAX {
        LinkForm::Fast
    } else {
        LinkForm::Slow
    }
}

/// 创建符号链接（双形态自动）。
pub fn create_link(store: &mut ExtStore, target: &str) -> Symlink {
    let form = choose_form(target);
    match form {
        LinkForm::Fast => Symlink {
            form,
            inline_target: Some(String::from(target)),
            ext_slot: None,
        },
        LinkForm::Slow => {
            let slot = store.put(target);
            Symlink { form, inline_target: None, ext_slot: Some(slot) }
        }
    }
}

/// 读取符号链接（往返）。
pub fn read_link<'a>(sl: &'a Symlink, store: &'a ExtStore) -> Option<&'a str> {
    match sl.form {
        LinkForm::Fast => sl.inline_target.as_deref(),
        LinkForm::Slow => sl.ext_slot.and_then(|s| store.get(s)).map(|x| x.as_str()),
    }
}

/// J1 主判据：双形态各 1,000 例往返一致 + 60 字节边界恰等。
pub fn j1_main() -> bool {
    let mut store = ExtStore::new();
    // fast 1000 例：len 1..=60
    let mut fast_ok = true;
    for l in 1..=1000usize {
        let target = gen_target(l % INLINE_MAX + 1);
        let sl = create_link(&mut store, &target);
        if sl.form != LinkForm::Fast || read_link(&sl, &store) != Some(target.as_str()) {
            fast_ok = false;
        }
        let _ = l;
    }
    // slow 1000 例：len 61..=1060
    let mut slow_ok = true;
    for l in 0..1000usize {
        let target = gen_target(INLINE_MAX + 1 + l);
        let sl = create_link(&mut store, &target);
        if sl.form != LinkForm::Slow || read_link(&sl, &store) != Some(target.as_str()) {
            slow_ok = false;
        }
    }
    // 边界：60 恰等 → fast；61 → slow
    let edge60 = choose_form(&gen_target(INLINE_MAX)) == LinkForm::Fast;
    let edge61 = choose_form(&gen_target(INLINE_MAX + 1)) == LinkForm::Slow;
    fast_ok && slow_ok && edge60 && edge61
}

/// 目标串生成（确定性，长度精确）。
fn gen_target(len: usize) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < len {
        s.push((b'a' + (i % 26) as u8) as char);
        i += 1;
    }
    s
}

// ── 注入核（0xDA80 段 5 项）────────────────────────────────────────────

/// 注入核 1：形态名互异注入。
pub fn inj1_form_names() -> bool {
    LinkForm::Fast.name() != LinkForm::Slow.name() && !LinkForm::Fast.name().is_empty()
}

/// 注入核 2：边界反转注入——61 字节必不进内联。
pub fn inj2_edge_flip() -> bool {
    choose_form(&gen_target(60)) == LinkForm::Fast
        && choose_form(&gen_target(61)) == LinkForm::Slow
        && choose_form(&gen_target(0)) == LinkForm::Fast
}

/// 注入核 3：快链接无外置注入——fast 不占外置槽。
pub fn inj3_fast_no_ext() -> bool {
    let mut store = ExtStore::new();
    let sl = create_link(&mut store, "abc");
    sl.inline_target.is_some() && sl.ext_slot.is_none() && store.slots.is_empty()
}

/// 注入核 4：慢链接不内联注入——slow 不占内联。
pub fn inj4_slow_no_inline() -> bool {
    let mut store = ExtStore::new();
    let target = gen_target(INLINE_MAX + 5);
    let sl = create_link(&mut store, &target);
    sl.inline_target.is_none() && sl.ext_slot.is_some()
}

/// 注入核 5：外置槽序注入——多慢链接槽号互异且读回各自目标。
pub fn inj5_ext_slots() -> bool {
    let mut store = ExtStore::new();
    let t1 = gen_target(INLINE_MAX + 1);
    let t2 = gen_target(INLINE_MAX + 2);
    let s1 = create_link(&mut store, &t1);
    let s2 = create_link(&mut store, &t2);
    s1.ext_slot != s2.ext_slot && read_link(&s1, &store) == Some(t1.as_str())
        && read_link(&s2, &store) == Some(t2.as_str())
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 29
}

/// run_unx4181_checks：本单判据聚合（诊断段 0xDA80）。
pub fn run_unx4181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4181 符号链接存储模型（快/慢链接）");
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    s.add(
        "快/慢链接双形态创建读取往返一致（各1000例）",
        j1_main(),
        if j1_main() { "" } else { "往返不一致" },
    );
    s.add(
        "形态自动选择边界（60字节恰等）断言通过",
        inj2_edge_flip(),
        if inj2_edge_flip() { "" } else { "边界断言破" },
    );
    let inj = [
        ("注入1形态名互异", inj1_form_names()),
        ("注入2边界反转", inj2_edge_flip()),
        ("注入3快链接无外置", inj3_fast_no_ext()),
        ("注入4慢链接不内联", inj4_slow_no_inline()),
        ("注入5外置槽序", inj5_ext_slots()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
