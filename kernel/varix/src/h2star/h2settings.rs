//! H2 域设置存储 · 深化批次三·二波（五十项里所有「重启要保持」的
//! 设置项统一存储层——版本化 + 迁移 + 导入导出 round-trip）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F279 逐条改禁持久化**、**F286 壁纸配置**、**F287 附加时钟
//!   配置**、**F296 区域格式**、**F298 磁贴布局**——这些「设置类
//!   判据」的落盘全部走本层：条目化存储（键→值→校验和），写入经
//!   [`crate::h2star::h2persist`] 语义（三段式在 persist，本层只管
//!   条目模型与迁移）；
//! - **升级不破坏旧数据**（十二章）：版本化封包 + 迁移链——旧版本
//!   读入自动迁到当前版，未知未来版本**拒绝并自首**（不猜格式）；
//! - **导入导出 round-trip**（F161 车道、十二章「数据开放」）：导出
//!   →导入还原逐条等值；损坏段降级默认值并自首（不炸、不静默）。
//!
//! 存储模型：`键（静态串）→ 值（字节）→ CRC`。键集封闭（表驱动
//! ——五十项设置键全部登记在案，表外键拒绝写入）。

use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 设置条目模型
// ---------------------------------------------------------------------------

/// 当前封包版本（迁移链终点）。
pub const SCHEMA_VERSION: u16 = 3;

/// 值类型（解析与校验的唯一依据——类型不符拒收，不猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    U32,
    Bool,
    Text,
    /// 字节块（磁贴布局/手势表等复合体）。
    Blob,
}

/// 键登记表条目：键名 + 值类型 + 默认值（损坏时降级的落点）。
#[derive(Clone, Copy, Debug)]
pub struct KeySpec {
    pub key: &'static str,
    pub ty: ValType,
    pub default: &'static [u8],
}

/// 域设置键表（五十项设置类判据的登记处——封闭键集）。
pub const KEYS: [KeySpec; 12] = [
    KeySpec { key: "f279.disabled_gestures", ty: ValType::Blob, default: &[] },
    KeySpec { key: "f286.mode", ty: ValType::U32, default: &[0] },
    KeySpec { key: "f287.clocks", ty: ValType::Blob, default: &[] },
    KeySpec { key: "f296.profile", ty: ValType::U32, default: &[0] },
    KeySpec { key: "f298.tiles", ty: ValType::Blob, default: &[] },
    KeySpec { key: "f281.banner_ms", ty: ValType::U32, default: &[136, 19, 0, 0] },
    KeySpec { key: "f271.tab_count", ty: ValType::U32, default: &[1, 0, 0, 0] },
    KeySpec { key: "f293.remember", ty: ValType::Bool, default: &[0] },
    KeySpec { key: "f267.enabled", ty: ValType::Bool, default: &[1] },
    KeySpec { key: "f285.budget_mb", ty: ValType::U32, default: &[64, 0, 0, 0] },
    KeySpec { key: "f253.pins", ty: ValType::Blob, default: &[] },
    KeySpec { key: "f292.repair_on", ty: ValType::Bool, default: &[1] },
];

/// 键查表（封闭键集的表外键 = 拒绝写入）。
pub fn spec_of(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.key == key)
}

/// 值合法性：类型符 + 长度口径（U32 恰 4 字节、Bool 恰 1 字节 0/1、
/// Text 必须 UTF-8、Blob 任意——类型即契约）。
pub fn validate_value(spec: &KeySpec, val: &[u8]) -> bool {
    match spec.ty {
        ValType::U32 => val.len() == 4,
        ValType::Bool => val.len() == 1 && (val[0] == 0 || val[0] == 1),
        ValType::Text => core::str::from_utf8(val).is_ok(),
        ValType::Blob => true,
    }
}

// ---------------------------------------------------------------------------
// 存储（键 → 值 + 校验和）
// ---------------------------------------------------------------------------

/// FNV-1a（与 h2screen 指纹同族——域内唯一哈希实现归 h2ledger，
/// 此处 32 位变体用于短校验和，注明用途）。
fn crc32ish(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 条目存储：封闭键集 + 校验和账。
pub struct SettingsStore {
    slots: Vec<(&'static str, Vec<u8>, u32)>,
    /// 损坏自首账（读出校验不符的键——降级默认值并登记）。
    pub corrupted: Vec<&'static str>,
    /// 被拒写入账（表外键/类型不符——拒绝可见）。
    pub rejected: Vec<&'static str>,
}

impl SettingsStore {
    pub fn new() -> SettingsStore {
        SettingsStore { slots: Vec::new(), corrupted: Vec::new(), rejected: Vec::new() }
    }

    /// 写入（表外键与非法值拒收——留账不静默）。
    pub fn set(&mut self, key: &'static str, val: Vec<u8>) -> bool {
        let Some(spec) = spec_of(key) else {
            self.rejected.push(key);
            return false;
        };
        if !validate_value(spec, &val) {
            self.rejected.push(key);
            return false;
        }
        let sum = crc32ish(&val);
        match self.slots.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, v, s)) => {
                *v = val;
                *s = sum;
            }
            None => self.slots.push((key, val, sum)),
        }
        true
    }

    /// 读取：校验和不符 → 降级默认值 + 自首；未设置 → 默认值
    /// （默认值也算「读到了」——空值不存在，行为恒定）。
    pub fn get(&mut self, key: &'static str) -> Vec<u8> {
        let Some(spec) = spec_of(key) else {
            self.rejected.push(key);
            return Vec::new();
        };
        match self.slots.iter().find(|(k, _, _)| *k == key) {
            Some((_, v, s)) if *s == crc32ish(v) => v.clone(),
            Some((k, _, _)) => {
                self.corrupted.push(k);
                spec.default.to_vec()
            }
            None => spec.default.to_vec(),
        }
    }

    /// 条目数（诊断口径）。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 封包与迁移（版本化 + 导入导出）
// ---------------------------------------------------------------------------

/// 封包：`[版本 u16LE][条目数 u16LE][条目…]`，每条
/// `[键长 u8][键][值长 u16LE][值][校验和 u32LE]`。
/// 未知未来版本拒绝（返回 None——不猜格式，自首交给调用方呈现）。
pub fn export_pack(store: &SettingsStore, version: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(store.slots.len() as u16).to_le_bytes());
    for (k, v, s) in &store.slots {
        out.push(k.len() as u8);
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(&(v.len() as u16).to_le_bytes());
        out.extend_from_slice(v);
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// 解包（迁移链：v1/v2 → 当前版逐条迁移；未知版本 None）。
/// 迁移规则（一处一事实）：
/// - v1→v2：f281.banner_ms 从 5000ms 口径迁到阈值+淡出口径（+200）；
/// - v2→v3：f285.budget_mb 字节口径 → MB 口径（÷1MB）；
/// - 校验和不符条目丢弃并降默认（调用方查 corrupted 账）。
pub fn import_pack(
    pack: &[u8],
    corrupted: &mut Vec<&'static str>,
) -> Option<Vec<(&'static str, Vec<u8>)>> {
    if pack.len() < 4 {
        return None;
    }
    let version = u16::from_le_bytes([pack[0], pack[1]]);
    let count = u16::from_le_bytes([pack[2], pack[3]]) as usize;
    match version {
        1 | 2 | 3 => {}
        _ => return None,
    }
    let mut items = Vec::new();
    let mut off = 4usize;
    for _ in 0..count {
        if off + 1 > pack.len() {
            return None;
        }
        let klen = pack[off] as usize;
        off += 1;
        if off + klen > pack.len() {
            return None;
        }
        let key = core::str::from_utf8(&pack[off..off + klen]).ok()?;
        off += klen;
        if off + 2 > pack.len() {
            return None;
        }
        let vlen = u16::from_le_bytes([pack[off], pack[off + 1]]) as usize;
        off += 2;
        if off + vlen + 4 > pack.len() {
            return None;
        }
        let val = pack[off..off + vlen].to_vec();
        off += vlen;
        let stored = u32::from_le_bytes([
            pack[off],
            pack[off + 1],
            pack[off + 2],
            pack[off + 3],
        ]);
        off += 4;
        if crc32ish(&val) != stored {
            if let Some(spec) = spec_of(key) {
                corrupted.push(spec.key);
                items.push((spec.key, spec.default.to_vec()));
            }
            continue;
        }
        match spec_of(key) {
            Some(spec) => items.push((spec.key, val)),
            None => continue, // 表外键弃置（封闭键集纪律）
        }
    }
    // 迁移链（版本升序执行——每步一事实，可逐条对账）。
    let mut cur = items;
    if version < 2 {
        for (k, v) in cur.iter_mut() {
            if *k == "f281.banner_ms" && v.len() == 4 {
                let ms = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
                *v = (ms + 200).to_le_bytes().to_vec();
            }
        }
    }
    if version < 3 {
        for (k, v) in cur.iter_mut() {
            if *k == "f285.budget_mb" && v.len() == 4 {
                let bytes = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
                *v = (bytes / (1024 * 1024)).max(1).to_le_bytes().to_vec();
            }
        }
    }
    Some(cur)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2settings_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2settings");
    // 键表封闭：表外键拒写、拒读（留账）。
    let mut st = SettingsStore::new();
    set.add(
        "h2settings closed keyset",
        !st.set("f999.evil", vec![1])
            && st.rejected.contains(&"f999.evil")
            && st.get("f999.evil").is_empty(),
        "wild key refused",
    );
    // 类型契约：U32 恰 4 字节、Bool 恰 1 字节 0/1。
    set.add(
        "h2settings type gate",
        !st.set("f286.mode", vec![1, 2])
            && !st.set("f293.remember", vec![7])
            && st.set("f293.remember", vec![1])
            && st.len() == 1,
        "shape checked",
    );
    // 读写回等 + 覆盖。
    set.add(
        "h2settings write read",
        st.set("f286.mode", vec![2, 0, 0, 0])
            && st.get("f286.mode") == vec![2, 0, 0, 0]
            && st.set("f286.mode", vec![1, 0, 0, 0])
            && st.get("f286.mode") == vec![1, 0, 0, 0]
            && st.len() == 2,
        "overwrite ok",
    );
    // 默认值降级：未设置项给默认（行为恒定）；损坏项给默认+自首。
    let mut st2 = SettingsStore::new();
    set.add(
        "h2settings default fallback",
        st2.get("f281.banner_ms") == vec![136, 19, 0, 0],
        "unset → default (5000ms)",
    );
    st2.set("f292.repair_on", vec![1]);
    st2.slots[0].2 ^= 0xffff; // 翻转校验和——模拟位腐
    let got = st2.get("f292.repair_on");
    set.add(
        "h2settings corruption self-report",
        got == vec![1] && st2.corrupted.contains(&"f292.repair_on"),
        "degrade + confess",
    );
    // 导入导出 round-trip：当前版全量往返逐条等值。
    let mut st3 = SettingsStore::new();
    st3.set("f286.mode", vec![3, 0, 0, 0]);
    st3.set("f253.pins", vec![1, 2, 3]);
    st3.set("f296.profile", vec![1, 0, 0, 0]);
    let pack = export_pack(&st3, SCHEMA_VERSION);
    let mut corr = Vec::new();
    let back = import_pack(&pack, &mut corr).unwrap();
    set.add(
        "h2settings roundtrip",
        back.len() == 3
            && back.iter().find(|(k, _)| *k == "f286.mode").unwrap().1 == vec![3, 0, 0, 0]
            && back.iter().find(|(k, _)| *k == "f253.pins").unwrap().1 == vec![1, 2, 3]
            && corr.is_empty(),
        "lossless restore",
    );
    // 未知未来版本拒绝（不猜格式）。
    set.add(
        "h2settings future refused",
        import_pack(&export_pack(&st3, 99), &mut corr).is_none(),
        "no format guessing",
    );
    // 迁移链 v1→v3：banner +200、budget 字节口径 ÷1MB。
    let mut old = SettingsStore::new();
    old.set("f281.banner_ms", vec![136, 19, 0, 0]); // 5000ms（v1 口径）
    old.set("f285.budget_mb", vec![0, 0, 64, 0]); // 0x400000 字节（v2 口径）
    let old_pack = export_pack(&old, 1);
    let migrated = import_pack(&old_pack, &mut corr).unwrap();
    let banner = migrated.iter().find(|(k, _)| *k == "f281.banner_ms").unwrap().1.clone();
    let budget = migrated.iter().find(|(k, _)| *k == "f285.budget_mb").unwrap().1.clone();
    set.add(
        "h2settings migrate v1→v3",
        u32::from_le_bytes([banner[0], banner[1], banner[2], banner[3]]) == 5200
            && u32::from_le_bytes([budget[0], budget[1], budget[2], budget[3]]) == 4,
        "5000+200；4MB",
    );
    // 截断包：解析安全失败（不 panic、None）。
    set.add(
        "h2settings truncated safe",
        import_pack(&pack[..pack.len() / 2], &mut corr).is_none(),
        "no panic on cut",
    );
    // 键表登记数（对账：封闭键集 = 12 键——表外即缺陷）。
    set.add("h2settings key registry", KEYS.len() == 12 && spec_of("f298.tiles").is_some(), "12 keys");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2settings_all_green() {
        let set = run_h2settings_checks();
        assert!(set.all_passed(), "h2settings 自检有红项");
        assert!(!set.truncated(), "h2settings 自检溢出");
    }

    #[test]
    fn fuzz_packs_never_panic() {
        // 畸形包 fuzz：随机字节串 500 发——解析器只许 None/Some，不许炸。
        let mut seed: u64 = 0x5eed;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut corr = Vec::new();
        for _ in 0..500 {
            let n = (rnd() % 64) as usize;
            let pack: Vec<u8> = (0..n).map(|_| (rnd() & 0xff) as u8).collect();
            let _ = import_pack(&pack, &mut corr);
        }
    }
}
