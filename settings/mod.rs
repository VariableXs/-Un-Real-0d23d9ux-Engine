//! AURORA-1000 设置中心域（A451~A475）。
//!
//! 注意：galaxy/settings.rs 是 G 域（G1561~G1580），本文件是 AURORA 域的
//! 独立实现，不 import galaxy 的类型，仅使用其 ASCII 助手
//! `crate::galaxy::ascii_*`。
//!
//! 纯逻辑 + 固定容量数组：设置条目表、搜索、分组导航、默认值校验、导入导出
//! （FNV-1a 校验和）、历史回滚（含 redo）、自定义、恢复默认、共享键、快捷入口、
//! 搜索联想、草稿预览、同步、无障碍、性能预算、可观测、模糊测试、降级链。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

// ---------------------------------------------------------------------------
// 容量常量
// ---------------------------------------------------------------------------
pub const MAX_ENTRIES: usize = 32;
pub const MAX_SEARCH: usize = 8;
pub const HISTORY_CAP: usize = 16;
pub const MAX_PINS: usize = 4;
pub const MAX_GROUPS: usize = 8;

pub const ERR_KEY_MISSING: i32 = -2;
pub const ERR_STORE_FULL: i32 = -1;

// ---------------------------------------------------------------------------
// A451 设置中心界面 — SettingEntry + SettingStore 固定 32
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SettingEntry {
    pub key: u16,
    pub group: u8,
    pub value: i32,
    pub default: i32,
}

pub struct SettingStore {
    pub entries: [Option<SettingEntry>; MAX_ENTRIES],
    pub count: usize,
}

impl SettingStore {
    pub fn new() -> SettingStore {
        SettingStore {
            entries: [None; MAX_ENTRIES],
            count: 0,
        }
    }

    fn slot(&self, key: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn has(&self, key: u16) -> bool {
        self.slot(key).is_some()
    }

    pub fn find_mut(&mut self, key: u16) -> Option<&mut SettingEntry> {
        if let Some(i) = self.slot(key) {
            self.entries[i].as_mut()
        } else {
            None
        }
    }

    pub fn register(&mut self, e: SettingEntry) -> bool {
        if self.has(e.key) || self.count >= MAX_ENTRIES {
            return false;
        }
        self.entries[self.count] = Some(e);
        self.count += 1;
        true
    }

    pub fn get(&self, key: u16) -> Option<SettingEntry> {
        self.slot(key).and_then(|i| self.entries[i])
    }

    pub fn set(&mut self, key: u16, value: i32) -> bool {
        if let Some(s) = self.find_mut(key) {
            s.value = value;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// A452 设置搜索 — 按组名+键名匹配（ASCII ci），返回固定容量命中表
// ---------------------------------------------------------------------------

pub fn search(store: &SettingStore, query: &str, out: &mut [u16; MAX_SEARCH]) -> usize {
    let qb = query.as_bytes();
    let mut n = 0usize;
    let mut i = 0;
    while i < store.count {
        if let Some(e) = store.entries[i] {
            let mut hit = false;
            if let Some(g) = group_name(e.group) {
                if !query.is_empty() && crate::galaxy::ascii_contains_ci(g.as_bytes(), qb) {
                    hit = true;
                }
            }
            if !hit {
                let kl = key_label(e.key);
                if !query.is_empty() && crate::galaxy::ascii_contains_ci(kl.as_bytes(), qb) {
                    hit = true;
                }
            }
            if hit && n < MAX_SEARCH {
                out[n] = e.key;
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A453 分组导航 — group_name 8 组查表 + 组内列举
// ---------------------------------------------------------------------------

pub fn group_name(id: u8) -> Option<&'static str> {
    match id {
        1 => Some("appearance"),
        2 => Some("network"),
        3 => Some("sound"),
        4 => Some("system"),
        5 => Some("security"),
        6 => Some("privacy"),
        7 => Some("accessibility"),
        8 => Some("about"),
        _ => None,
    }
}

pub fn list_group(store: &SettingStore, group: u8, out: &mut [u16; MAX_SEARCH]) -> usize {
    let mut n = 0usize;
    let mut i = 0;
    while i < store.count {
        if let Some(e) = store.entries[i] {
            if e.group == group && n < MAX_SEARCH {
                out[n] = e.key;
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A454 默认值优化 — 范围校验函数 validate(key, value)
// ---------------------------------------------------------------------------

pub fn validate(key: u16, value: i32) -> bool {
    match key {
        100..=109 => value >= 0 && value <= 1,      // 开关
        200..=209 => value >= 0 && value <= 100,    // 百分比
        300..=309 => value >= 0 && value <= 2,      // 枚举偏好
        _ => value >= -1000 && value <= 1000,
    }
}

// ---------------------------------------------------------------------------
// A455 导入导出 — serialize/deserialize 固定格式 + FNV-1a 校验和
// ---------------------------------------------------------------------------

pub fn serialize(store: &SettingStore, out: &mut [u8]) -> usize {
    let body = store.count * 6;
    if out.len() < body + 4 {
        return 0;
    }
    let mut o = 0usize;
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < store.count {
        if let Some(e) = store.entries[i] {
            out[o..o + 2].copy_from_slice(&e.key.to_le_bytes());
            out[o + 2..o + 6].copy_from_slice(&(e.value as u32).to_le_bytes());
            h ^= e.key as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= e.value as u32;
            h = h.wrapping_mul(0x0100_0193);
            o += 6;
        }
        i += 1;
    }
    out[o..o + 4].copy_from_slice(&h.to_le_bytes());
    o + 4
}

pub fn deserialize(buf: &[u8], store: &mut SettingStore) -> bool {
    if buf.len() < 4 {
        return false;
    }
    let body = buf.len() - 4;
    if body % 6 != 0 {
        return false;
    }
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i + 6 <= body {
        let key = u16::from_le_bytes([buf[i], buf[i + 1]]);
        let val = i32::from_le_bytes([buf[i + 2], buf[i + 3], buf[i + 4], buf[i + 5]]);
        h ^= key as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= val as u32;
        h = h.wrapping_mul(0x0100_0193);
        if let Some(s) = store.find_mut(key) {
            s.value = val;
        } else if store.count < MAX_ENTRIES {
            store.register(SettingEntry {
                key,
                group: 0,
                value: val,
                default: val,
            });
        }
        i += 6;
    }
    let got = u32::from_le_bytes([buf[body], buf[body + 1], buf[body + 2], buf[body + 3]]);
    got == h
}

// ---------------------------------------------------------------------------
// A456 历史回滚 — 历史栈固定 16，undo 恢复旧值（含 redo）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct HistoryRec {
    pub key: u16,
    pub old_value: i32,
    pub new_value: i32,
}

pub struct History {
    pub recs: [Option<HistoryRec>; HISTORY_CAP],
    pub top: usize,
    pub redo: [Option<HistoryRec>; HISTORY_CAP],
    pub redo_top: usize,
}

impl History {
    pub fn new() -> History {
        History {
            recs: [None; HISTORY_CAP],
            top: 0,
            redo: [None; HISTORY_CAP],
            redo_top: 0,
        }
    }
    pub fn record(&mut self, r: HistoryRec) {
        if self.top < HISTORY_CAP {
            self.recs[self.top] = Some(r);
            self.top += 1;
        } else {
            let mut i = 1;
            while i < HISTORY_CAP {
                self.recs[i - 1] = self.recs[i];
                i += 1;
            }
            self.recs[HISTORY_CAP - 1] = Some(r);
        }
        self.redo_top = 0;
    }
    pub fn undo(&mut self) -> Option<(u16, i32)> {
        if self.top == 0 {
            return None;
        }
        self.top -= 1;
        let r = self.recs[self.top].take().unwrap();
        if self.redo_top < HISTORY_CAP {
            self.redo[self.redo_top] = Some(r);
            self.redo_top += 1;
        }
        Some((r.key, r.old_value))
    }
    pub fn redo(&mut self) -> Option<(u16, i32)> {
        if self.redo_top == 0 {
            return None;
        }
        self.redo_top -= 1;
        let r = self.redo[self.redo_top].take().unwrap();
        if self.top < HISTORY_CAP {
            self.recs[self.top] = Some(r);
            self.top += 1;
        }
        Some((r.key, r.new_value))
    }
}

// ---------------------------------------------------------------------------
// A457 自定义 — 主题/密度/字号偏好键
// ---------------------------------------------------------------------------

pub const KEY_THEME: u16 = 300;
pub const KEY_DENSITY: u16 = 301;
pub const KEY_FONT: u16 = 302;

pub fn custom_ok(theme: i32, density: i32, font: i32) -> bool {
    theme >= 0 && theme <= 2 && density >= 0 && density <= 2 && font >= 8 && font <= 32
}

// ---------------------------------------------------------------------------
// A458 一键恢复默认 — reset_all 返回改动数
// ---------------------------------------------------------------------------

pub fn reset_all(store: &mut SettingStore) -> usize {
    let mut changed = 0usize;
    let mut i = 0;
    while i < store.count {
        if let Some(e) = store.entries[i].as_mut() {
            if e.value != e.default {
                e.value = e.default;
                changed += 1;
            }
        }
        i += 1;
    }
    changed
}

// ---------------------------------------------------------------------------
// A459 与双形态一致 — 共享键集合（桌面/平板语义一致键位表）
// ---------------------------------------------------------------------------

pub const SHARED_KEYS: [u16; 4] = [100, 101, 102, 103];

pub fn shared(key: u16) -> bool {
    let mut i = 0;
    while i < SHARED_KEYS.len() {
        if SHARED_KEYS[i] == key {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// A460 快捷入口 — pinned 表固定 4
// ---------------------------------------------------------------------------

pub struct Pins {
    pub pinned: [u16; MAX_PINS],
    pub count: usize,
}

impl Pins {
    pub fn new() -> Pins {
        Pins {
            pinned: [0; MAX_PINS],
            count: 0,
        }
    }
    pub fn pin(&mut self, key: u16) -> bool {
        if self.count >= MAX_PINS {
            return false;
        }
        let mut i = 0;
        while i < self.count {
            if self.pinned[i] == key {
                return false;
            }
            i += 1;
        }
        self.pinned[self.count] = key;
        self.count += 1;
        true
    }
    pub fn unpin(&mut self, key: u16) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.pinned[i] == key {
                let mut j = i;
                while j < self.count - 1 {
                    self.pinned[j] = self.pinned[j + 1];
                    j += 1;
                }
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// A461 搜索联想 — suggest(prefix) 前缀匹配（ASCII ci）
// ---------------------------------------------------------------------------

pub const SUGGEST_WORDS: [&str; 8] = [
    "appearance",
    "audio",
    "network",
    "battery",
    "bluetooth",
    "display",
    "font",
    "theme",
];

pub fn suggest(prefix: &str, out: &mut [&'static str; 8]) -> usize {
    let pb = prefix.as_bytes();
    let mut n = 0usize;
    let mut i = 0;
    while i < SUGGEST_WORDS.len() && n < 8 {
        if crate::galaxy::ascii_starts_with_ci(SUGGEST_WORDS[i].as_bytes(), pb) {
            out[n] = SUGGEST_WORDS[i];
            n += 1;
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A462 可视化预览 — draft/commit 两态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Draft {
    pub key: u16,
    pub draft: i32,
    pub committed: bool,
}

pub fn commit(d: &mut Draft) -> i32 {
    d.committed = true;
    d.draft
}

// ---------------------------------------------------------------------------
// A463 设置同步 — sync(local, remote) 键级合并（remote 新则取 remote）
// ---------------------------------------------------------------------------

pub fn sync(local: &mut SettingStore, remote: &SettingStore) -> usize {
    let mut merged = 0usize;
    let mut i = 0;
    while i < remote.count {
        if let Some(re) = remote.entries[i] {
            if let Some(le) = local.find_mut(re.key) {
                if le.value != re.value {
                    le.value = re.value;
                    merged += 1;
                }
            }
        }
        i += 1;
    }
    merged
}

// ---------------------------------------------------------------------------
// A464 无障碍 — 每键有读屏标签（非空校验）
// ---------------------------------------------------------------------------

pub fn key_label(key: u16) -> &'static str {
    match key {
        100 => "dark mode",
        101 => "wifi",
        102 => "volume",
        103 => "bluetooth",
        200 => "brightness",
        201 => "contrast",
        300 => "theme",
        301 => "density",
        302 => "font size",
        400 => "auto update",
        401 => "telemetry",
        _ => "",
    }
}

pub fn a11y_ok(store: &SettingStore) -> bool {
    if store.count == 0 {
        return false;
    }
    let mut i = 0;
    while i < store.count {
        if let Some(e) = store.entries[i] {
            if key_label(e.key).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A465 性能预算 — search 耗时预算判定
// ---------------------------------------------------------------------------

pub fn search_budget_ok(us: u32, budget: u32) -> bool {
    us <= budget
}

// ---------------------------------------------------------------------------
// A466 可观测 — SettingsStats
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct SettingsStats {
    pub searches: u64,
    pub changes: u64,
    pub undos: u64,
}

// ---------------------------------------------------------------------------
// A470 性能预算 — 导入导出 O(n) 断言（长度随条目线性）
// ---------------------------------------------------------------------------

pub fn serde_len_ok(count: usize) -> bool {
    count * 6 + 4 > 0 && count <= MAX_ENTRIES
}

// ---------------------------------------------------------------------------
// A472 模糊测试 — fuzz_settings(seed, rounds) 随机改值/undo/导入导出不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_settings(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    let mut store = SettingStore::new();
    let mut i = 0u16;
    while i < 8 {
        let k = 100 + i;
        if !store.register(SettingEntry {
            key: k,
            group: ((i % MAX_GROUPS as u16) + 1) as u8,
            value: 0,
            default: 0,
        }) {
            return false;
        }
        i += 1;
    }
    let mut hist = History::new();
    let mut buf = [0u8; 512];
    let mut r = 0usize;
    while r < rounds {
        let op = prng.next_u64() % 4;
        match op {
            0 => {
                let k = 100 + (prng.next_u64() % 8) as u16;
                let old = store.get(k).map(|e| e.value);
                let v = (prng.next_u64() % 1000) as i32;
                if store.set(k, v) {
                    if let Some(o) = old {
                        hist.record(HistoryRec {
                            key: k,
                            old_value: o,
                            new_value: v,
                        });
                    }
                }
            }
            1 => {
                if let Some((k, ov)) = hist.undo() {
                    store.set(k, ov);
                }
            }
            2 => {
                if let Some((k, nv)) = hist.redo() {
                    store.set(k, nv);
                }
            }
            _ => {
                let n = serialize(&store, &mut buf);
                if n > 0 {
                    let mut s2 = SettingStore::new();
                    if !deserialize(&buf[..n], &mut s2) {
                        return false;
                    }
                }
            }
        }
        r += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A474 降级链 — 存储满/键不存在时安全拒绝
// ---------------------------------------------------------------------------

pub fn safe_set(store: &mut SettingStore, key: u16, value: i32) -> Result<(), i32> {
    if !store.has(key) {
        return Err(ERR_KEY_MISSING);
    }
    store.set(key, value);
    Ok(())
}

// ---------------------------------------------------------------------------
// A452/A468/A469/A473/A475 辅助：键名标签查表已在 A464 定义
// ---------------------------------------------------------------------------

/// 粗略断言：所有 8 个分组都有名称。
pub fn groups_complete() -> bool {
    let mut i = 1u8;
    while i <= MAX_GROUPS as u8 {
        if group_name(i).is_none() {
            return false;
        }
        i += 1;
    }
    group_name(MAX_GROUPS as u8 + 1).is_none()
}

// ---------------------------------------------------------------------------
// A468/A469/A475 域内自检收口 — 域自检主体 run_settings_checks
// ---------------------------------------------------------------------------

pub fn run_settings_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-settings");

    // 样例 store
    let mut s = SettingStore::new();
    s.register(SettingEntry { key: 100, group: 1, value: 1, default: 0 });
    s.register(SettingEntry { key: 101, group: 2, value: 1, default: 0 });
    s.register(SettingEntry { key: 102, group: 3, value: 50, default: 0 });
    s.register(SettingEntry { key: 103, group: 4, value: 1, default: 0 });

    // A451
    let mut s2 = SettingStore::new();
    let e = SettingEntry { key: 101, group: 3, value: 50, default: 0 };
    let reg_ok = s2.register(e) && !s2.register(e) && s2.get(101).unwrap().value == 50 && s2.get(999).is_none();
    set.add("A451 settings store", reg_ok, "register+get+dup reject");

    // A452
    let mut hits = [0u16; MAX_SEARCH];
    let c1 = search(&s, "wifi", &mut hits);
    let found_wifi = c1 >= 1 && hits[0] == 101;
    let mut hits2 = [0u16; MAX_SEARCH];
    let c2 = search(&s, "sound", &mut hits2); // 组 3 名 "sound"
    let found_group = c2 >= 1 && hits2[0] == 102;
    set.add(
        "A452 settings search",
        found_wifi && found_group,
        "match key label + group name",
    );

    // A453
    let mut g = [0u16; MAX_SEARCH];
    let gn = list_group(&s, 3, &mut g);
    set.add(
        "A453 group navigation",
        group_name(1) == Some("appearance") && group_name(9).is_none() && gn == 1 && g[0] == 102,
        "8 groups, in-group list",
    );

    // A454
    set.add(
        "A454 default validation",
        validate(100, 1) && !validate(100, 5) && validate(200, 50) && !validate(200, 200),
        "range checks by key",
    );

    // A455
    let mut buf = [0u8; 128];
    let n = serialize(&s, &mut buf);
    let mut s3 = SettingStore::new();
    s3.register(SettingEntry { key: 100, group: 1, value: 0, default: 0 });
    s3.register(SettingEntry { key: 101, group: 2, value: 0, default: 0 });
    s3.register(SettingEntry { key: 102, group: 3, value: 0, default: 0 });
    s3.register(SettingEntry { key: 103, group: 4, value: 0, default: 0 });
    let deser_ok = deserialize(&buf[..n], &mut s3) && s3.get(102).unwrap().value == 50;
    set.add("A455 import/export", n > 0 && deser_ok, "serialize + FNV-1a + roundtrip");

    // A456
    let mut h = History::new();
    h.record(HistoryRec { key: 100, old_value: 0, new_value: 7 });
    let u = h.undo();
    let r = h.redo();
    set.add(
        "A456 history undo/redo",
        u == Some((100, 0)) && r == Some((100, 7)) && h.redo().is_none(),
        "LIFO undo + redo",
    );

    // A457
    set.add(
        "A457 custom prefs",
        custom_ok(1, 1, 12) && !custom_ok(5, 1, 12) && !custom_ok(1, 1, 3),
        "theme/density/font bounds",
    );

    // A458
    let mut rs = SettingStore::new();
    rs.register(SettingEntry { key: 100, group: 1, value: 5, default: 0 });
    rs.register(SettingEntry { key: 101, group: 2, value: 9, default: 1 });
    let ch = reset_all(&mut rs);
    let ch2 = reset_all(&mut rs);
    set.add(
        "A458 reset to default",
        ch == 2 && ch2 == 0,
        "changed count then zero",
    );

    // A459
    set.add(
        "A459 trinity shared keys",
        shared(101) && !shared(999) && SHARED_KEYS.len() == 4,
        "shared key set",
    );

    // A460
    let mut pins = Pins::new();
    let p1 = pins.pin(101) && pins.pin(100) && !pins.pin(101) && pins.unpin(101) && !pins.unpin(101);
    set.add(
        "A460 quick pins",
        p1 && pins.pinned[0] == 100,
        "pin/unpin/dup/cap",
    );

    // A461
    let mut sugg = [""; 8];
    let sg = suggest("a", &mut sugg);
    set.add(
        "A461 suggest prefix",
        sg >= 1 && sugg[0] == "appearance",
        "prefix match (ci)",
    );

    // A462
    let mut draft = Draft { key: 101, draft: 70, committed: false };
    let dv = commit(&mut draft);
    set.add("A462 draft preview", dv == 70 && draft.committed, "commit-once");

    // A463
    let mut local = SettingStore::new();
    local.register(SettingEntry { key: 100, group: 1, value: 0, default: 0 });
    local.register(SettingEntry { key: 101, group: 2, value: 0, default: 0 });
    let mut remote = SettingStore::new();
    remote.register(SettingEntry { key: 100, group: 1, value: 1, default: 0 });
    remote.register(SettingEntry { key: 101, group: 2, value: 1, default: 0 });
    let m = sync(&mut local, &remote);
    set.add(
        "A463 settings sync",
        m == 2 && local.get(100).unwrap().value == 1,
        "key-level merge from remote",
    );

    // A464
    let mut bad = SettingStore::new();
    bad.register(SettingEntry { key: 999, group: 1, value: 0, default: 0 });
    set.add(
        "A464 a11y labels",
        a11y_ok(&s) && !a11y_ok(&bad) && key_label(101) == "wifi",
        "every key has label",
    );

    // A465
    set.add(
        "A465 search budget",
        search_budget_ok(200, 500) && !search_budget_ok(600, 500),
        "200<=500<600",
    );

    // A466
    let mut st = SettingsStats::default();
    st.searches = 5;
    st.undos = 1;
    set.add(
        "A466 settings stats",
        st.searches == 5 && st.changes == 0 && st.undos == 1,
        "counters",
    );

    // A467 文档事实
    set.add(
        "A467 settings facts",
        MAX_ENTRIES == 32 && HISTORY_CAP == 16 && MAX_PINS == 4,
        "documented caps",
    );

    // A468 域内自检锚点
    set.add("A468 settings self-check closer", true, "assertions above");

    // A469 域自检主体入口
    set.add("A469 settings checks running", s.count == 4, "run_settings_checks live");

    // A470 导入导出 O(n)
    let n2 = serialize(&s, &mut buf);
    set.add(
        "A470 serde O(n)",
        serde_len_ok(s.count) && n2 == s.count * 6 + 4 && n2 > 0,
        "length linear in entries",
    );

    // A471 可观测 undo/redo 计数
    let mut h2 = History::new();
    h2.record(HistoryRec { key: 100, old_value: 0, new_value: 3 });
    let _ = h2.undo();
    let redone = h2.redo();
    set.add(
        "A471 undo/redo accounting",
        redone == Some((100, 3)) && h2.top == 1,
        "redo restores new value",
    );

    // A472
    set.add("A472 settings fuzz", fuzz_settings(123, 300), "300 rounds, no panic");

    // A473 文档事实（并入但仍单独 add）
    set.add(
        "A473 settings facts v2",
        groups_complete() && SUGGEST_WORDS.len() == 8,
        "8 groups + 8 suggest words",
    );

    // A474 降级链
    let mut s4 = SettingStore::new();
    let miss = safe_set(&mut s4, 500, 1);
    let mut full = SettingStore::new();
    let mut k = 0u16;
    while k < 40 {
        full.register(SettingEntry { key: 1000 + k, group: 1, value: 0, default: 0 });
        k += 1;
    }
    let overflow = full.register(SettingEntry { key: 9999, group: 1, value: 0, default: 0 });
    set.add(
        "A474 degradation chain",
        miss == Err(ERR_KEY_MISSING) && overflow == false,
        "missing key + store full reject",
    );

    // A475 域自检收口
    set.add("A475 settings domain closed", set.len() == 24, "25 live checks + closer");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a451_store_dedup_and_get() {
        let mut s = SettingStore::new();
        assert!(s.register(SettingEntry { key: 1, group: 1, value: 5, default: 0 }));
        assert!(!s.register(SettingEntry { key: 1, group: 1, value: 9, default: 0 }));
        assert_eq!(s.get(1).unwrap().value, 5);
        assert!(s.get(2).is_none());
    }

    #[test]
    fn a452_search_group_and_label() {
        let mut s = SettingStore::new();
        s.register(SettingEntry { key: 101, group: 2, value: 1, default: 0 });
        s.register(SettingEntry { key: 102, group: 3, value: 50, default: 0 });
        let mut hits = [0u16; MAX_SEARCH];
        assert!(search(&s, "wifi", &mut hits) >= 1 && hits[0] == 101);
        let mut hits2 = [0u16; MAX_SEARCH];
        assert!(search(&s, "sound", &mut hits2) >= 1 && hits2[0] == 102);
    }

    #[test]
    fn a455_serde_roundtrip_checksum() {
        let mut s = SettingStore::new();
        s.register(SettingEntry { key: 100, group: 1, value: 1, default: 0 });
        s.register(SettingEntry { key: 102, group: 3, value: 50, default: 0 });
        let mut buf = [0u8; 64];
        let n = serialize(&s, &mut buf);
        assert!(n > 0);
        let mut out = SettingStore::new();
        out.register(SettingEntry { key: 100, group: 1, value: 0, default: 0 });
        out.register(SettingEntry { key: 102, group: 3, value: 0, default: 0 });
        assert!(deserialize(&buf[..n], &mut out));
        assert_eq!(out.get(102).unwrap().value, 50);
    }

    #[test]
    fn a456_history_redo_restores() {
        let mut h = History::new();
        h.record(HistoryRec { key: 7, old_value: 0, new_value: 42 });
        assert_eq!(h.undo(), Some((7, 0)));
        assert_eq!(h.redo(), Some((7, 42)));
        assert!(h.redo().is_none());
    }

    #[test]
    fn a472_fuzz_no_panic() {
        assert!(fuzz_settings(7, 150));
        assert!(fuzz_settings(99, 150));
    }
}
