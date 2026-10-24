//! VE-F3981 · S 域接口总账与主题映射终审
//!
//! 终审：F3966 清点终版对账（册版本冻结 + 修正留痕 + 对外承诺表回执
//! 状态——复述红线）；主题映射终审（二十项→落条唯一性断言）；五域映射
//! 家族对拍（O/P/Q/R 四域映射同构复核）。
//! 数据结构：总账终版；映射表；唯一性断言。
//! 错误路径：对账非冻结→阻断；映射缺/重→修正；回执未达→催办。
//! 性能：对账 O(版本)；映射 O(1)；催办 O(项)。
//! 跨批对接：F3966 上游；F3995 移交包前提；家族对拍。
//! 无障碍与隐私：总账含语义接口章（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x39C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x39C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T1LCode(pub u16);

impl T1LCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const NOT_FROZEN: Self = Self(SEG_BASE + 1);
    pub const MAP_MISSING: Self = Self(SEG_BASE + 2);
    pub const MAP_DUP: Self = Self(SEG_BASE + 3);
    pub const RECEIPT_LATE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NOT_FROZEN => "对账非冻结→阻断",
            Self::MAP_MISSING => "映射缺→修正",
            Self::MAP_DUP => "映射重→修正",
            Self::RECEIPT_LATE => "回执未达→催办",
            _ => "未知",
        }
    }
}

// ── 总账终版（册版本冻结）────────────────────────────────────────────

/// 总账条目：接口名 + 册版本 + 修正留痕。
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    /// 接口名。
    pub api: &'static str,
    /// 册版本号。
    pub ver: u32,
    /// 修正留痕（空 = 未修正）。
    pub amend_note: String,
    /// 修正计数。
    pub amends: u32,
}

/// 总账终版：版本冻结后对账才有效。
#[derive(Debug, Clone)]
pub struct LedgerFinal {
    /// 条目。
    pub entries: Vec<LedgerEntry>,
    /// 册版本冻结标记。
    pub frozen: bool,
    /// 冻结版本号。
    pub frozen_ver: u32,
}

impl LedgerFinal {
    pub fn new() -> Self {
        Self { entries: Vec::new(), frozen: false, frozen_ver: 0 }
    }

    /// 登记条目（冻结后拒绝——变更走纪律册）。
    pub fn put(&mut self, api: &'static str, ver: u32) -> bool {
        if self.frozen || self.entries.iter().any(|e| e.api == api) {
            return false;
        }
        self.entries.push(LedgerEntry {
            api,
            ver,
            amend_note: String::new(),
            amends: 0,
        });
        true
    }

    /// 修正留痕（留痕制：每次修正计数 + 备注）。
    pub fn amend(&mut self, api: &str, ver: u32, note: &str) -> bool {
        if self.frozen {
            return false;
        }
        match self.entries.iter_mut().find(|e| e.api == api) {
            Some(e) => {
                e.ver = ver;
                e.amend_note = String::from(note);
                e.amends += 1;
                true
            }
            None => false,
        }
    }

    /// 冻结（版本取当前最大条目版本）。
    pub fn freeze(&mut self) -> T1LCode {
        if self.frozen {
            return T1LCode::OK;
        }
        if self.entries.is_empty() {
            return T1LCode::NOT_FROZEN;
        }
        self.frozen_ver = self.entries.iter().map(|e| e.ver).max().unwrap_or(0);
        self.frozen = true;
        T1LCode::OK
    }

    /// 对账：非冻结即阻断（复述红线）。
    pub fn reconcile(&self) -> T1LCode {
        if !self.frozen {
            return T1LCode::NOT_FROZEN;
        }
        T1LCode::OK
    }
}

// ── 主题映射终审（二十项→落条唯一性）────────────────────────────────

pub const THEME_TOTAL: u8 = 20;

/// 映射表：二十项主题 → 落条（接口名）唯一性。
#[derive(Debug, Clone)]
pub struct ThemeMap {
    /// theme_index → 落条 api 名。
    pub landings: Vec<(&'static str, String)>,
}

impl ThemeMap {
    pub fn new() -> Self {
        Self { landings: Vec::new() }
    }

    /// 登记映射（index 0..20）。
    pub fn put(&mut self, theme_index: u8, api: &str) -> bool {
        if theme_index >= THEME_TOTAL {
            return false;
        }
        if self.landings.iter().any(|(i, _)| *i == api_name(theme_index)) {
            return false;
        }
        self.landings.push((api_name(theme_index), String::from(api)));
        true
    }

    /// 唯一性断言：二十项齐备且落条互不重复。
    pub fn unique_ok(&self) -> bool {
        if self.landings.len() != THEME_TOTAL as usize {
            return false;
        }
        for i in 0..self.landings.len() {
            for j in (i + 1)..self.landings.len() {
                if self.landings[i].1 == self.landings[j].1 {
                    return false;
                }
            }
        }
        true
    }

    /// 缺/重诊断：缺项清单。
    pub fn missing(&self) -> Vec<u8> {
        let mut miss = Vec::new();
        for i in 0..THEME_TOTAL {
            if !self.landings.iter().any(|(t, _)| *t == api_name(i)) {
                miss.push(i);
            }
        }
        miss
    }
}

fn api_name(theme_index: u8) -> &'static str {
    match theme_index {
        0 => "主题-读屏",
        1 => "主题-高对比",
        2 => "主题-动效",
        3 => "主题-焦点",
        4 => "主题-朗读",
        5 => "主题-色标",
        6 => "主题-认知",
        7 => "主题-输入",
        8 => "主题-性能",
        9 => "主题-工具",
        10 => "主题-渲染",
        11 => "主题-裁剪",
        12 => "主题-缓存",
        13 => "主题-降级",
        14 => "主题-回退",
        15 => "主题-审计",
        16 => "主题-词典",
        17 => "主题-走查",
        18 => "主题-观测",
        _ => "主题-收官",
    }
}

// ── 对外承诺表回执状态 ───────────────────────────────────────────────

/// 承诺回执状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseState {
    /// 未回。
    Unreceived,
    /// 已回执确认。
    Confirmed,
    /// 回执有异议。
    Disputed,
}

/// 对外双承诺条目。
#[derive(Debug, Clone)]
pub struct PromiseRow {
    /// 承诺名。
    pub name: &'static str,
    /// 对端域。
    pub peer: &'static str,
    /// 回执状态。
    pub state: PromiseState,
}

/// 承诺表。
#[derive(Debug, Clone)]
pub struct PromiseBook {
    pub rows: Vec<PromiseRow>,
}

impl PromiseBook {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn put(&mut self, name: &'static str, peer: &'static str) {
        self.rows.push(PromiseRow { name, peer, state: PromiseState::Unreceived });
    }

    /// 回执登记。
    pub fn receipt(&mut self, name: &str, confirmed: bool) -> bool {
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.state = if confirmed { PromiseState::Confirmed } else { PromiseState::Disputed };
                true
            }
            None => false,
        }
    }

    /// 回执未达清单（催办对象）。
    pub fn pending(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| r.state != PromiseState::Confirmed).map(|r| r.name).collect()
    }
}

// ── 五域映射家族对拍（O/P/Q/R 四域同构）─────────────────────────────

/// 家族对拍：五域（S+O/P/Q/R）映射结构同构复核。
#[derive(Debug, Clone)]
pub struct FamilyPeerCheck {
    /// 各域映射条数。
    pub counts: [u32; 5],
    /// 各域唯一性通过。
    pub unique: [bool; 5],
}

pub const PEER_DOMAINS: [&str; 5] = ["S", "O", "P", "Q", "R"];

impl FamilyPeerCheck {
    pub fn new() -> Self {
        Self { counts: [0; 5], unique: [false; 5] }
    }

    pub fn put(&mut self, slot: usize, count: u32, unique: bool) -> bool {
        if slot >= 5 {
            return false;
        }
        self.counts[slot] = count;
        self.unique[slot] = unique;
        true
    }

    /// 同构复核：五域条数一致且全唯一。
    pub fn isomorphic(&self) -> bool {
        let c = self.counts[0];
        c > 0 && self.counts.iter().all(|&x| x == c) && self.unique.iter().all(|&u| u)
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves235_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves235_s10ledger");

    // C3981-01 诊断码段不越窗。
    chk(&mut set, "C3981-01",
        T1LCode::RECEIPT_LATE.0 < SEG_BASE + 16 && SEG_BASE == 0x39C0, "err");

    // C3981-02 总账登记 + 重名拒绝 + 冻结后拒绝写入。
    let mut l = LedgerFinal::new();
    chk(&mut set, "C3981-02",
        l.put("读屏接口", 3) && !l.put("读屏接口", 4) && l.entries.len() == 1, "err");

    // C3981-03 修正留痕计数。
    l.amend("读屏接口", 5, "版次修正");
    l.amend("读屏接口", 6, "二修");
    let e = &l.entries[0];
    chk(&mut set, "C3981-03", e.ver == 6 && e.amends == 2 && e.amend_note == "二修", "err");

    // C3981-04 非冻结对账 → 阻断；冻结后放行。
    chk(&mut set, "C3981-04", l.reconcile() == T1LCode::NOT_FROZEN, "err");
    chk(&mut set, "C3981-05", l.freeze() == T1LCode::OK && l.frozen_ver == 6, "err");

    // C3981-05 冻结后 amend/put 双拒（留痕制闭环）。
    chk(&mut set, "C3981-05",
        !l.amend("读屏接口", 7, "迟改") && !l.put("新接口", 1), "err");

    // C3981-06 映射唯一性：齐备且不重。
    let mut m = ThemeMap::new();
    for i in 0..THEME_TOTAL {
        m.put(i, "落条A");
    }
    chk(&mut set, "C3981-06", !m.unique_ok() && m.missing().is_empty(), "err"); // 全重 → 断言败

    // C3981-07 逐项唯一落条 → 断言过。
    let mut m2 = ThemeMap::new();
    for i in 0..THEME_TOTAL {
        let api = match i {
            0 => "读屏", 1 => "高对比", 2 => "动效", 3 => "焦点",
            4 => "朗读", 5 => "色标", 6 => "认知", 7 => "输入",
            8 => "性能", 9 => "工具", 10 => "渲染", 11 => "裁剪",
            12 => "缓存", 13 => "降级", 14 => "回退", 15 => "审计",
            16 => "词典", 17 => "走查", 18 => "观测", _ => "收官",
        };
        m2.put(i, api);
    }
    chk(&mut set, "C3981-07", m2.unique_ok(), "err");

    // C3981-08 映射越界 index 拒绝。
    chk(&mut set, "C3981-08", !m2.put(20, "越界"), "err");

    // C3981-09 承诺表回执：未回 → 催办清单非空。
    let mut p = PromiseBook::new();
    p.put("双承诺-读屏措辞", "R");
    p.put("双承诺-色标文化色", "T");
    chk(&mut set, "C3981-09", p.pending().len() == 2, "err");

    // C3981-10 回执确认 + 异议分流。
    p.receipt("双承诺-读屏措辞", true);
    p.receipt("双承诺-色标文化色", false);
    chk(&mut set, "C3981-10", p.pending() == vec!["双承诺-色标文化色"], "err");

    // C3981-11 家族对拍：五域同构（条数一致 + 全唯一）。
    let mut f = FamilyPeerCheck::new();
    for s in 0..5 {
        f.put(s, 20, true);
    }
    chk(&mut set, "C3981-11", f.isomorphic(), "err");

    // C3981-12 某域条数漂移 → 同构破。
    let mut f2 = FamilyPeerCheck::new();
    for s in 0..5 {
        f2.put(s, 20, true);
    }
    f2.put(3, 19, true);
    chk(&mut set, "C3981-12", !f2.isomorphic(), "err");

    // C3981-13 某域唯一性败 → 同构破。
    let mut f3 = FamilyPeerCheck::new();
    for s in 0..5 {
        f3.put(s, 20, true);
    }
    f3.put(4, 20, false);
    chk(&mut set, "C3981-13", !f3.isomorphic(), "err");

    // C3981-14 映射缺项诊断。
    let mut m3 = ThemeMap::new();
    for i in 0..THEME_TOTAL {
        if i != 7 && i != 13 {
            m3.put(i, "落");
        }
    }
    chk(&mut set, "C3981-14", m3.missing() == vec![7u8, 13], "err");

    // C3981-15 空账冻结拒绝。
    let mut l2 = LedgerFinal::new();
    chk(&mut set, "C3981-15", l2.freeze() == T1LCode::NOT_FROZEN, "err");

    // C3981-16 回执越名拒绝。
    let mut p2 = PromiseBook::new();
    p2.put("承诺甲", "O");
    chk(&mut set, "C3981-16", !p2.receipt("承诺乙", true), "err");

    // C3981-17 对拍越槽拒绝。
    let mut f4 = FamilyPeerCheck::new();
    chk(&mut set, "C3981-17", !f4.put(5, 1, true), "err");

    // C3981-18 主题名覆盖全 20 槽。
    chk(&mut set, "C3981-18",
        (0..THEME_TOTAL).all(|i| !api_name(i).is_empty())
            && api_name(0) == "主题-读屏"
            && api_name(19) == "主题-收官", "err");

    // C19 修正留痕冻结即锁（amends 不再增长）。
    let amends_before = l.entries[0].amends;
    let _ = l.amend("读屏接口", 9, "锁后");
    chk(&mut set, "C3981-19", l.entries[0].amends == amends_before, "err");

    // C3981-20 错误码互异。
    let codes = [T1LCode::OK.0, T1LCode::NOT_FROZEN.0, T1LCode::MAP_MISSING.0,
        T1LCode::MAP_DUP.0, T1LCode::RECEIPT_LATE.0];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    chk(&mut set, "C3981-20", uniq, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_gate_blocks_reconcile() {
        let mut l = LedgerFinal::new();
        assert_eq!(l.reconcile(), T1LCode::NOT_FROZEN);
        l.put("a", 1);
        l.freeze();
        assert_eq!(l.reconcile(), T1LCode::OK);
    }

    #[test]
    fn theme_map_uniqueness() {
        let mut m = ThemeMap::new();
        for i in 0..THEME_TOTAL {
            m.put(i, "同一条");
        }
        assert!(!m.unique_ok());
    }

    #[test]
    fn promise_receipt_flow() {
        let mut p = PromiseBook::new();
        p.put("甲", "O");
        assert_eq!(p.pending().len(), 1);
        p.receipt("甲", true);
        assert!(p.pending().is_empty());
    }
}
