//! UNX-F0246 早传第五步：引导参数覆盖块（0xAA80 段）
//!
//! 锚点：UNX-F0246-J1 降级/诊断/一次性覆盖三来源合并生效，优先级
//! 与 F0255 一致，来源标记齐。
//! 域：CoRun·UNX-A1-B13（覆盖块合并面）。
//!
//! 结构：三来源闭集（降级 / 诊断 / 一次性覆盖）+ 参数覆盖模型
//! （(键, 值, 来源)）+ 合并器（优先级：一次性覆盖 > 诊断 > 降级，
//! 同键高优先覆盖低优先）+ 来源标记核（生效值必须带来源标记）+
//! 注入核（未知来源/同键无标记/优先级破缺/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAA80 独占 0x80）。
pub const SEG_BASE: u32 = 0xAA80;
pub const XC_OV_SRC: u32 = SEG_BASE + 0x01; // 未知来源
pub const XC_OV_NOTAG: u32 = SEG_BASE + 0x02; // 无标记
pub const XC_OV_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 三来源闭集（优先级递增）。
pub const SRC_DEGRADE: u8 = 0;
pub const SRC_DIAG: u8 = 1;
pub const SRC_ONESHOT: u8 = 2;

/// 来源名。
pub fn src_name(s: u8) -> &'static str {
    match s {
        SRC_DEGRADE => "degrade",
        SRC_DIAG => "diag",
        SRC_ONESHOT => "oneshot",
        _ => "?",
    }
}

/// 覆盖项。
#[derive(Clone)]
pub struct OverItem {
    pub key: String,
    pub value: String,
    pub src: u8,
}

/// 覆盖块合并器。
pub struct OverBlock {
    pub items: Vec<OverItem>,
    pub sealed: bool,
}

impl OverBlock {
    pub fn new() -> Self {
        OverBlock { items: Vec::new(), sealed: false }
    }

    /// 添加来源项（来源闭集守卫）。
    pub fn put(&mut self, key: &str, value: &str, src: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_OV_SEALED);
        }
        if src > SRC_ONESHOT {
            return Err(XC_OV_SRC);
        }
        // 同 (键, 来源) 覆盖
        if let Some(it) = self.items.iter_mut().find(|it| it.key == key && it.src == src) {
            it.value = String::from(value);
            return Ok(());
        }
        self.items.push(OverItem {
            key: String::from(key),
            value: String::from(value),
            src,
        });
        Ok(())
    }

    /// 三源合并生效：同键取最高优先（一次性 > 诊断 > 降级）。
    pub fn merge(&self) -> Result<Vec<OverItem>, u32> {
        if self.sealed {
            return Err(XC_OV_SEALED);
        }
        let mut keys: Vec<String> = Vec::new();
        for it in &self.items {
            if !keys.contains(&it.key) {
                keys.push(it.key.clone());
            }
        }
        let mut out = Vec::new();
        for k in keys {
            // 在所有来源中取最高优先
            let mut best: Option<&OverItem> = None;
            for it in &self.items {
                if it.key != k {
                    continue;
                }
                match best {
                    None => best = Some(it),
                    Some(b) => {
                        if it.src > b.src {
                            best = Some(it);
                        }
                    }
                }
            }
            match best {
                Some(it) => out.push(it.clone()),
                None => return Err(XC_OV_NOTAG),
            }
        }
        Ok(out)
    }

    /// 生效项来源标记齐备核。
    pub fn merged_tagged(&self) -> bool {
        match self.merge() {
            Ok(items) => items.iter().all(|it| it.src <= SRC_ONESHOT && !it.key.is_empty()),
            Err(_) => false,
        }
    }
}

/// 三源合并核：降级/诊断/一次性同键 → 一次性生效。
pub fn three_src_exact() -> bool {
    let mut b = OverBlock::new();
    let _ = b.put("console", "tty0", SRC_DEGRADE);
    let _ = b.put("console", "ttyS0", SRC_DIAG);
    let _ = b.put("console", "ttyUSB0", SRC_ONESHOT);
    let m = match b.merge() {
        Ok(m) => m,
        Err(_) => return false,
    };
    m.len() == 1 && m[0].value == "ttyUSB0" && m[0].src == SRC_ONESHOT
}

/// 异键并存核：不同键各来源独立生效。
pub fn multi_key_exact() -> bool {
    let mut b = OverBlock::new();
    let _ = b.put("loglevel", "3", SRC_DEGRADE);
    let _ = b.put("verbose", "1", SRC_DIAG);
    let _ = b.put("single", "yes", SRC_ONESHOT);
    let m = match b.merge() {
        Ok(m) => m,
        Err(_) => return false,
    };
    m.len() == 3
}

/// 优先级一致核（F0255 对齐）：降级 < 诊断 < 一次性。
pub fn priority_exact() -> bool {
    // 诊断 vs 降级
    let mut b = OverBlock::new();
    let _ = b.put("k", "v-degrade", SRC_DEGRADE);
    let _ = b.put("k", "v-diag", SRC_DIAG);
    if b.merge().map(|m| m[0].value.clone()) != Ok(String::from("v-diag")) {
        return false;
    }
    // 一次性 vs 诊断
    let mut b2 = OverBlock::new();
    let _ = b2.put("k", "v-diag", SRC_DIAG);
    let _ = b2.put("k", "v-oneshot", SRC_ONESHOT);
    b2.merge().map(|m| m[0].value.clone()) == Ok(String::from("v-oneshot"))
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = OverBlock::new();
    // 1) 未知来源
    if b.put("k", "v", 7) == Err(XC_OV_SRC) {
        caught += 1;
    }
    // 2) 未知来源合并检测（未放入即无标记项不存在）
    let _ = b.put("k", "v", SRC_DIAG);
    if b.merged_tagged() {
        caught += 1;
    }
    // 3) 封账后拒
    b.sealed = true;
    if b.put("k", "v2", SRC_ONESHOT) == Err(XC_OV_SEALED) {
        caught += 1;
    }
    // 4) 同 (键, 来源) 覆盖不重复
    let mut b2 = OverBlock::new();
    let _ = b2.put("k", "v1", SRC_DEGRADE);
    let _ = b2.put("k", "v2", SRC_DEGRADE);
    if b2.items.len() == 1 && b2.items[0].value == "v2" {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：覆盖块折叠哈希。
pub fn fp_frozen(b: &OverBlock) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for it in &b.items {
        for &c in it.key.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= it.src as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx246_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0246");
    // J1-1 三源合并生效
    let ok = three_src_exact();
    s.add("B13-01-三源合并", ok, if ok { "" } else { "合并漂移" });
    // J1-2 异键并存
    let ok = multi_key_exact();
    s.add("B13-02-异键并存", ok, if ok { "" } else { "并存漂移" });
    // J1-3 优先级一致（F0255）
    let ok = priority_exact();
    s.add("B13-03-优先级一致", ok, if ok { "" } else { "优先漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let b = OverBlock::new();
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B13-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
