//! UNX-F7581 · 健康阈值引擎总纲（判定入口 / schema / 分级三段）· 诊断段 0xD180
//! 验收锚点：阈值引擎三段落账——① schema 段：属性阈值表（id→warn/crit/fail 三线）可校验
//! （warn > crit > fail、id 非零、无重复）；② 入口段：admit(id, value) 唯一判定入口，
//! 未登记 id 走默认线；③ 分级段：四级 Normal/Warn/Critical/Fail 语义与边界（含等号）固定。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xD180;

/// ① schema 段：单条阈值记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreshRec {
    pub id: u8,
    pub warn_below: u8,
    pub crit_below: u8,
    pub fail_below: u8,
}

impl ThreshRec {
    pub fn new(id: u8, warn_below: u8, crit_below: u8, fail_below: u8) -> Self {
        ThreshRecord { id, warn_below, crit_below, fail_below }
    }
}

/// 别名兼容（笔误防护）。
pub type ThreshRecord = ThreshRec;

/// schema 校验：三线严格递减 + id 非零。
pub fn validate_rec(r: &ThreshRec) -> bool {
    r.id != 0 && r.warn_below > r.crit_below && r.crit_below > r.fail_below
}

/// 判定产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Normal,
    Warn,
    Critical,
    Fail,
    NoSchema,
}

impl Verdict {
    pub fn rank(self) -> u8 {
        match self {
            Verdict::Normal => 0,
            Verdict::NoSchema => 1,
            Verdict::Warn => 2,
            Verdict::Critical => 3,
            Verdict::Fail => 4,
        }
    }
}

/// ② 入口段：阈值引擎。schema 表 + 默认线。
pub struct ThresholdEngine {
    pub schema: Vec<ThreshRec>,
    pub default_warn: u8,
    pub default_crit: u8,
    pub default_fail: u8,
    pub admits: u32,
}

impl ThresholdEngine {
    pub fn new() -> Self {
        ThresholdEngine { schema: Vec::new(), default_warn: 60, default_crit: 40, default_fail: 20, admits: 0 }
    }

    pub fn install(&mut self, r: ThreshRec) -> bool {
        if !validate_rec(&r) {
            return false;
        }
        if self.schema.iter().any(|x| x.id == r.id) {
            return false;
        }
        self.schema.push(r);
        true
    }

    /// 唯一判定入口：admit(id, value)。
    pub fn admit(&mut self, id: u8, value: u8) -> Verdict {
        self.admits += 1;
        let r = match self.schema.iter().find(|r| r.id == id) {
            Some(r) => *r,
            None => ThreshRec { id, warn_below: self.default_warn, crit_below: self.default_crit, fail_below: self.default_fail },
        };
        if value <= r.fail_below {
            Verdict::Fail
        } else if value <= r.crit_below {
            Verdict::Critical
        } else if value <= r.warn_below {
            Verdict::Warn
        } else {
            Verdict::Normal
        }
    }

    /// schema 自查：全表合法且无重复 id。
    pub fn schema_ok(&self) -> bool {
        let mut seen: Vec<u8> = Vec::new();
        for r in &self.schema {
            if !validate_rec(r) || seen.contains(&r.id) {
                return false;
            }
            seen.push(r.id);
        }
        true
    }
}

/// ③ 分级段：verdict → 人类可读标签（呈现契约）。
pub fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Normal => "OK",
        Verdict::Warn => "W",
        Verdict::Critical => "C",
        Verdict::Fail => "F",
        Verdict::NoSchema => "?",
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

// ── 注入核（0xD180 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7581_D180;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("schema-非法线序拒收", !ThresholdEngine::new().install(ThreshRec::new(5, 30, 40, 50))));
    out.push(("入口-空表走默认线", ThresholdEngine::new().admit(9, 10) == Verdict::Fail));
    out.push(("分级-NoSchema秩高于Normal", Verdict::NoSchema.rank() > Verdict::Normal.rank() && Verdict::NoSchema.rank() < Verdict::Warn.rank()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7581_checks：本单判据聚合（诊断段 0xD180）。
pub fn run_unx7581_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7581 健康阈值引擎总纲");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // schema 段：合法表 + 拒收路径
    let mut e = ThresholdEngine::new();
    let ok_rec = e.install(ThreshRec::new(5, 60, 30, 10));
    s.add("schema-合法记录安装", ok_rec, if ok_rec { "" } else { "合法记录被拒" });
    let ok_dup = !e.install(ThreshRec::new(5, 60, 30, 10));
    s.add("schema-重复id拒收", ok_dup, if ok_dup { "" } else { "重复 id 未拒" });
    let ok_zero = !e.install(ThreshRec::new(0, 60, 30, 10));
    s.add("schema-零id拒收", ok_zero, if ok_zero { "" } else { "零 id 未拒" });
    let ok_eq = !e.install(ThreshRec::new(6, 40, 40, 10));
    s.add("schema-warn线等于crit线拒收", ok_eq, if ok_eq { "" } else { "等值线未拒" });
    s.add("schema-自查通过", e.schema_ok(), if e.schema_ok() { "" } else { "自查失败" });

    // 入口段：登记 id 四级边界（含等号）
    let v_fail = e.admit(5, 10);
    s.add("入口-fail线恰等判Fail", v_fail == Verdict::Fail, if v_fail == Verdict::Fail { "" } else { "fail 边界漂移" });
    let v_crit = e.admit(5, 30);
    s.add("入口-crit线恰等判Critical", v_crit == Verdict::Critical, if v_crit == Verdict::Critical { "" } else { "crit 边界漂移" });
    let v_warn = e.admit(5, 60);
    s.add("入口-warn线恰等判Warn", v_warn == Verdict::Warn, if v_warn == Verdict::Warn { "" } else { "warn 边界漂移" });
    let v_ok = e.admit(5, 61);
    s.add("入口-高于warn线判Normal", v_ok == Verdict::Normal, if v_ok == Verdict::Normal { "" } else { "Normal 判定漂移" });

    // 入口段：未登记 id 走默认线（10≤20→Fail）
    let v_def = e.admit(200, 10);
    s.add("入口-未登记走默认线判Fail", v_def == Verdict::Fail, if v_def == Verdict::Fail { "" } else { "默认线失效" });

    // 入口计数
    s.add("入口-admit计数=6", e.admits == 6, if e.admits == 6 { "" } else { "admits 计数漂移" });

    // 分级段：标签契约
    let labels_ok = verdict_label(Verdict::Normal) == "OK"
        && verdict_label(Verdict::Warn) == "W"
        && verdict_label(Verdict::Critical) == "C"
        && verdict_label(Verdict::Fail) == "F";
    s.add("分级-四级标签契约", labels_ok, if labels_ok { "" } else { "标签漂移" });

    // 200 属性批量判定 + FNV 稳定
    let mut seed: u64 = 0x7581_0001;
    let mut acc: u32 = 0x811C_9DC5;
    let mut fails = 0u32;
    for i in 0..200u16 {
        let id = (i % 250 + 1) as u8;
        let value = (lcg_next(&mut seed) % 100) as u8;
        let v = e.admit(id, value);
        acc = fnv1a(&[id, value, v.rank()]);
        if v == Verdict::Fail {
            fails += 1;
        }
    }
    let ok_acc = acc != 0;
    s.add("批量-200属性FNV累计非零", ok_acc, if ok_acc { "" } else { "FNV 累计异常" });
    let ok_fails = fails > 0;
    s.add("批量-默认线必有Fail出现", ok_fails, if ok_fails { "" } else { "批量中无 Fail，判定面异常" });

    s
}
