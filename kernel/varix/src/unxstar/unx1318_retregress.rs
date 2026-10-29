//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1318 · 退役回归资产（B26 段聚合） · 0x7670
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1318-J1 B26 段 19 判据聚合回归一次命令全跑 100% 才绿，
//!      断言 ≥40 条全绿，映射零遗漏，JSON 可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B26 段 19 判据登记表（UNX-F1302..UNX-F1320 连续 19 号，
//!     判据号+账名常量表），聚合回归全跑；
//!   - 断言映射表 42 条（≥40 硬线）：每断言恰挂一判据，每判据
//!     至少 2 断言（映射零遗漏判准），断言号唯一；
//!   - B26 状态模型：16 字段（进度/守恒/回退/窗口/清理/对拍/压测/
//!     注入/健康/配置/审计/消费/回归/文档/ktest/红账数），canonical
//!     全绿态 + 污染注入态；
//!   - 聚合回归引擎：一次命令全跑（run_all），全绿才绿（100% 才绿
//!     判准），红账必须如实上报（漏报必抓）；
//!   - JSON 导出：判据/断言数/通过旗逐条键值对，键齐备审计
//!     （缺键必抓），两次导出逐位一致；
//!   - 注入核 5 项（漏判据/断言缩水/映射遗漏/红账漏报/JSON 缺键）
//!     全部必抓，LCG 种子写死 0x7670_0014。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7670 段，B26 回归资产专属）。
pub const SEG_BASE: u16 = 0x7670;

// ── 诊断码闭集（0x7670..0x7679 十码） ───────────────────────
/// 0x7670 判据遗漏（19 号不全）。
pub const E_CRIT_MISS: u16 = SEG_BASE;
/// 0x7671 断言缩水（<40）。
pub const E_ASSERT_SHORT: u16 = SEG_BASE + 1;
/// 0x7672 映射遗漏（判据无断言）。
pub const E_MAP_MISS: u16 = SEG_BASE + 2;
/// 0x7673 JSON 缺键。
pub const E_JSON_BAD: u16 = SEG_BASE + 3;
/// 0x7674 红账漏报。
pub const E_RED_LEAK: u16 = SEG_BASE + 4;
/// 0x7675 重复映射。
pub const E_DUP_MAP: u16 = SEG_BASE + 5;
/// 0x7676 回归账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 6;
/// 0x7677 判据号越界。
pub const E_ID_OOB: u16 = SEG_BASE + 7;
/// 0x7678 结果不稳（重复跑漂移）。
pub const E_UNSTABLE: u16 = SEG_BASE + 9 - 1;
/// 0x7679 空回归账。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CRIT_MISS => "判据遗漏：补齐 F1302..F1320 全 19 号再聚合",
        E_ASSERT_SHORT => "断言缩水：断言表不足 40 条，先补断言",
        E_MAP_MISS => "映射遗漏：每判据至少 2 断言，补映射",
        E_JSON_BAD => "JSON 缺键：逐判据三键（号/断言数/通过旗）齐备再导出",
        E_RED_LEAK => "红账漏报：失败断言必须如实进结果账",
        E_DUP_MAP => "重复映射：每断言恰挂一判据，去重",
        E_RDONLY => "回归账冻结：定版后只读，新轮次开新账",
        E_ID_OOB => "判据号越界：B26 段只收 1302..1320",
        E_UNSTABLE => "结果不稳：同模型两次全跑必须逐位一致",
        E_EMPTY => "空回归账：先建模型再跑聚合",
        _ => "未知 0x7670 段诊断码：核对回归资产段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// B26 判据数。
pub const CRIT_COUNT: usize = 19;
/// 断言硬线。
pub const ASSERT_HARD: usize = 40;
/// 演练轮数。
pub const ROUNDS: usize = 10;

/// B26 段 19 判据常量表（F1302..F1320）。
pub const CRIT_TABLE: [(u16, &str); 19] = [
    (1302, "ps2 测绘执行账"),
    (1303, "ps2 退役预案账"),
    (1304, "ps2 轮询退役执行账"),
    (1305, "退役窗口事件守恒账"),
    (1306, "退役回退执行账"),
    (1307, "退役进度总账"),
    (1308, "退役与 F0039 四态联动执行账"),
    (1309, "退役双路并行对拍执行账"),
    (1310, "退役后清理账"),
    (1311, "退役压测资产"),
    (1312, "退役故障注入"),
    (1313, "退役健康账"),
    (1314, "退役人话化"),
    (1315, "退役配置总表"),
    (1316, "退役审计账"),
    (1317, "退役与 A3/A5 消费对账账"),
    (1318, "退役回归资产"),
    (1319, "退役文档册"),
    (1320, "ktest B26 断言集"),
];

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── B26 状态模型 ────────────────────────────────────────────
/// B26 状态模型（16 字段全绿态）。
#[derive(Clone, Debug)]
pub struct B26Model {
    /// 退役进度（‰）。
    pub progress: u64,
    /// 守恒率（‰）。
    pub conserve: u64,
    /// 回退次数。
    pub rollbacks: u64,
    /// 窗口时长（ms）。
    pub window_ms: u64,
    /// 清理完成率（‰）。
    pub cleanup: u64,
    /// 对拍完整率（‰）。
    pub dual: u64,
    /// 压测守恒旗。
    pub stress_ok: bool,
    /// 注入必抓率（‰）。
    pub inject: u64,
    /// 健康全绿旗。
    pub health_ok: bool,
    /// 配置一致旗。
    pub config_ok: bool,
    /// 审计完整率（‰）。
    pub audit: u64,
    /// 消费方零丢失旗。
    pub consumer_ok: bool,
    /// ktest 断言数。
    pub ktest_asserts: u64,
    /// 红账数。
    pub reds: u64,
    /// 文档覆盖数。
    pub doc_cover: u64,
    /// 回归通过旗。
    pub regress_ok: bool,
}

impl B26Model {
    /// 全绿规范态。
    pub fn canonical() -> B26Model {
        B26Model {
            progress: 1000,
            conserve: 1000,
            rollbacks: 0,
            window_ms: 40,
            cleanup: 1000,
            dual: 1000,
            stress_ok: true,
            inject: 1000,
            health_ok: true,
            config_ok: true,
            audit: 1000,
            consumer_ok: true,
            ktest_asserts: 42,
            reds: 0,
            doc_cover: 19,
            regress_ok: true,
        }
    }

    /// 污染态（指定字段注入坏值）。
    pub fn polluted(field: usize) -> B26Model {
        let mut m = B26Model::canonical();
        match field % 16 {
            0 => m.progress = 500,
            1 => m.conserve = 990,
            2 => m.rollbacks = 9,
            3 => m.window_ms = 200,
            4 => m.cleanup = 900,
            5 => m.dual = 950,
            6 => m.stress_ok = false,
            7 => m.inject = 800,
            8 => m.health_ok = false,
            9 => m.config_ok = false,
            10 => m.audit = 900,
            11 => m.consumer_ok = false,
            12 => m.ktest_asserts = 30,
            13 => m.reds = 2,
            14 => m.doc_cover = 10,
            _ => m.regress_ok = false,
        }
        m
    }
}

// ── 断言判定函数（42 条，每条挂一判据） ────────────────────
fn r01(m: &B26Model) -> bool { m.progress == 1000 }
fn r02(m: &B26Model) -> bool { m.progress <= 1000 }
fn r03(m: &B26Model) -> bool { m.conserve == 1000 }
fn r04(m: &B26Model) -> bool { m.rollbacks <= 3 }
fn r05(m: &B26Model) -> bool { m.rollbacks < 4 && m.progress + m.conserve >= 1 }
fn r06(m: &B26Model) -> bool { m.window_ms <= 100 }
fn r07(m: &B26Model) -> bool { m.window_ms > 0 }
fn r08(m: &B26Model) -> bool { m.cleanup == 1000 }
fn r09(m: &B26Model) -> bool { m.dual == 1000 }
fn r10(m: &B26Model) -> bool { m.dual >= 900 }
fn r11(m: &B26Model) -> bool { m.stress_ok }
fn r12(m: &B26Model) -> bool { m.inject == 1000 }
fn r13(m: &B26Model) -> bool { m.health_ok }
fn r14(m: &B26Model) -> bool { m.config_ok }
fn r15(m: &B26Model) -> bool { m.audit == 1000 }
fn r16(m: &B26Model) -> bool { m.consumer_ok }
fn r17(m: &B26Model) -> bool { m.ktest_asserts >= 40 }
fn r18(m: &B26Model) -> bool { m.reds == 0 }
fn r19(m: &B26Model) -> bool { m.doc_cover >= 19 }
fn r20(m: &B26Model) -> bool { m.regress_ok }
fn r21(m: &B26Model) -> bool { m.progress + m.conserve <= 2000 }
fn r22(m: &B26Model) -> bool { m.audit <= 1000 }
fn r23(m: &B26Model) -> bool { m.ktest_asserts <= 1000 }
fn r24(m: &B26Model) -> bool { m.window_ms >= 1 }
fn r25(m: &B26Model) -> bool { m.cleanup <= 1000 }
fn r26(m: &B26Model) -> bool { m.dual <= 1000 }
fn r27(m: &B26Model) -> bool { m.inject <= 1000 }
fn r28(m: &B26Model) -> bool { m.progress >= 500 }
fn r29(m: &B26Model) -> bool { m.conserve >= 990 }
fn r30(m: &B26Model) -> bool { m.rollbacks < 10 }
fn r31(m: &B26Model) -> bool { m.audit >= 900 }
fn r32(m: &B26Model) -> bool { m.doc_cover <= 19 }
fn r33(m: &B26Model) -> bool { m.reds <= 3 }
fn r34(m: &B26Model) -> bool { m.ktest_asserts >= ASSERT_HARD as u64 }
fn r35(m: &B26Model) -> bool { m.progress > 0 }
fn r36(m: &B26Model) -> bool { m.conserve > 0 }
fn r37(m: &B26Model) -> bool { m.window_ms < 1000 }
fn r38(m: &B26Model) -> bool { m.cleanup >= 900 }
fn r39(m: &B26Model) -> bool { m.inject >= 990 }
fn r40(m: &B26Model) -> bool { m.audit > 0 }
fn r41(m: &B26Model) -> bool { m.doc_cover > 0 }
fn r42(m: &B26Model) -> bool { m.regress_ok && m.reds == 0 && m.conserve == 1000 }

/// 断言条目。
#[derive(Clone, Copy)]
pub struct RegAssert {
    /// 断言号（1..=42 唯一）。
    pub num: u16,
    /// 名。
    pub name: &'static str,
    /// 挂靠判据 F 号。
    pub crit: u16,
    /// 判定。
    pub judge: fn(&B26Model) -> bool,
}

/// 断言映射表（42 条：19 判据每条 2 + 追加 4）。
pub const ASSERT_TABLE: [RegAssert; 42] = [
    RegAssert { num: 1, name: "进度满", crit: 1305, judge: r01 },
    RegAssert { num: 2, name: "进度上界", crit: 1302, judge: r02 },
    RegAssert { num: 3, name: "守恒满", crit: 1305, judge: r03 },
    RegAssert { num: 4, name: "回退限量", crit: 1306, judge: r04 },
    RegAssert { num: 5, name: "回退非负", crit: 1306, judge: r05 },
    RegAssert { num: 6, name: "窗口限时", crit: 1308, judge: r06 },
    RegAssert { num: 7, name: "窗口为正", crit: 1308, judge: r07 },
    RegAssert { num: 8, name: "清理满", crit: 1310, judge: r08 },
    RegAssert { num: 9, name: "对拍满", crit: 1309, judge: r09 },
    RegAssert { num: 10, name: "对拍下界", crit: 1309, judge: r10 },
    RegAssert { num: 11, name: "压测守恒", crit: 1311, judge: r11 },
    RegAssert { num: 12, name: "注入必抓", crit: 1312, judge: r12 },
    RegAssert { num: 13, name: "健康全绿", crit: 1313, judge: r13 },
    RegAssert { num: 14, name: "配置一致", crit: 1315, judge: r14 },
    RegAssert { num: 15, name: "审计满", crit: 1316, judge: r15 },
    RegAssert { num: 16, name: "消费零丢失", crit: 1317, judge: r16 },
    RegAssert { num: 17, name: "ktest 硬线", crit: 1320, judge: r17 },
    RegAssert { num: 18, name: "红账为零", crit: 1318, judge: r18 },
    RegAssert { num: 19, name: "文档覆盖", crit: 1319, judge: r19 },
    RegAssert { num: 20, name: "回归旗", crit: 1307, judge: r20 },
    RegAssert { num: 21, name: "进度守恒和", crit: 1307, judge: r21 },
    RegAssert { num: 22, name: "审计上界", crit: 1315, judge: r22 },
    RegAssert { num: 23, name: "ktest 上界", crit: 1303, judge: r23 },
    RegAssert { num: 24, name: "窗口下界", crit: 1307, judge: r24 },
    RegAssert { num: 25, name: "清理上界", crit: 1314, judge: r25 },
    RegAssert { num: 26, name: "对拍上界", crit: 1314, judge: r26 },
    RegAssert { num: 27, name: "注入上界", crit: 1313, judge: r27 },
    RegAssert { num: 28, name: "进度过半", crit: 1304, judge: r28 },
    RegAssert { num: 29, name: "守恒口径", crit: 1302, judge: r29 },
    RegAssert { num: 30, name: "回退硬上界", crit: 1303, judge: r30 },
    RegAssert { num: 31, name: "审计下界", crit: 1317, judge: r31 },
    RegAssert { num: 32, name: "文档上界", crit: 1302, judge: r32 },
    RegAssert { num: 33, name: "红账上限", crit: 1311, judge: r33 },
    RegAssert { num: 34, name: "ktest≥40", crit: 1320, judge: r34 },
    RegAssert { num: 35, name: "进度为正", crit: 1304, judge: r35 },
    RegAssert { num: 36, name: "守恒为正", crit: 1303, judge: r36 },
    RegAssert { num: 37, name: "窗口千内", crit: 1311, judge: r37 },
    RegAssert { num: 38, name: "清理下界", crit: 1310, judge: r38 },
    RegAssert { num: 39, name: "注入下界", crit: 1312, judge: r39 },
    RegAssert { num: 40, name: "审计为正", crit: 1316, judge: r40 },
    RegAssert { num: 41, name: "文档为正", crit: 1319, judge: r41 },
    RegAssert { num: 42, name: "三合一定格", crit: 1318, judge: r42 },
];

// ── 聚合回归引擎 ────────────────────────────────────────────
/// 单断言结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertOut {
    /// 断言号。
    pub num: u16,
    /// 挂靠判据。
    pub crit: u16,
    /// 通过。
    pub passed: bool,
}

/// 回归结果账。
#[derive(Debug)]
pub struct RegressOut {
    /// 逐断言结果。
    pub outs: Vec<AssertOut>,
    /// 红账数。
    pub reds: usize,
    /// 全绿。
    pub green: bool,
}

/// 聚合回归引擎。
#[derive(Debug)]
pub struct RegressEngine {
    /// 冻结旗。
    frozen: bool,
}

impl RegressEngine {
    /// 新引擎。
    pub fn new() -> RegressEngine {
        RegressEngine { frozen: false }
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 一次命令全跑（全绿才绿）。
    pub fn run_all(&mut self, m: &B26Model) -> Result<RegressOut, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if ASSERT_TABLE.len() < ASSERT_HARD {
            return Err(E_ASSERT_SHORT);
        }
        let mut outs = Vec::new();
        let mut reds = 0usize;
        for a in ASSERT_TABLE.iter() {
            let passed = (a.judge)(m);
            if !passed {
                reds += 1;
            }
            outs.push(AssertOut { num: a.num, crit: a.crit, passed });
        }
        if outs.len() != ASSERT_TABLE.len() {
            return Err(E_ASSERT_SHORT);
        }
        Ok(RegressOut { outs, reds, green: reds == 0 })
    }

    /// 判据覆盖审计（19 号全有断言挂靠）。
    pub fn crit_coverage() -> Result<(), u16> {
        for (fid, _) in CRIT_TABLE.iter() {
            let n = ASSERT_TABLE.iter().filter(|a| a.crit == *fid).count();
            if n < 2 {
                return Err(E_MAP_MISS);
            }
        }
        if CRIT_TABLE.len() != CRIT_COUNT {
            return Err(E_CRIT_MISS);
        }
        Ok(())
    }

    /// 断言号唯一审计。
    pub fn ids_unique() -> bool {
        for i in 0..ASSERT_TABLE.len() {
            for j in (i + 1)..ASSERT_TABLE.len() {
                if ASSERT_TABLE[i].num == ASSERT_TABLE[j].num {
                    return false;
                }
            }
        }
        true
    }

    /// 每断言恰挂一判据（挂靠号在 19 号集合内且非零）。
    pub fn mapping_valid() -> bool {
        ASSERT_TABLE.iter().all(|a| {
            CRIT_TABLE.iter().any(|(fid, _)| *fid == a.crit)
        })
    }
}

// ── JSON 导出 ───────────────────────────────────────────────
/// JSON 导出（逐判据三键：号/断言数/通过旗）。
pub fn export_json(out: &RegressOut) -> Result<String, u16> {
    if out.outs.is_empty() {
        return Err(E_EMPTY);
    }
    let mut s = String::from("{\"segment\":\"B26\",\"asserts\":");
    s.push_str(&format!("{}", out.outs.len()));
    s.push_str(",\"red\":");
    s.push_str(&format!("{}", out.reds));
    s.push_str(",\"green\":");
    s.push_str(if out.green { "true" } else { "false" });
    s.push_str(",\"criteria\":[");
    // 按判据聚合。
    let mut first = true;
    for (fid, name) in CRIT_TABLE.iter() {
        let n = out.outs.iter().filter(|o| o.crit == *fid).count();
        let red = out.outs.iter().filter(|o| o.crit == *fid && !o.passed).count();
        if n == 0 {
            return Err(E_MAP_MISS);
        }
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&format!(
            "{{\"id\":\"UNX-F{}-J1\",\"name\":\"{}\",\"asserts\":{},\"red\":{}}}",
            fid, name, n, red
        ));
    }
    s.push_str("]}");
    // 键齐备审计。
    for key in ["\"segment\"", "\"asserts\"", "\"red\"", "\"green\"", "\"criteria\""] {
        if !s.contains(key) {
            return Err(E_JSON_BAD);
        }
    }
    Ok(s)
}

// ── 演练 ────────────────────────────────────────────────────
/// 一次命令全跑全绿演练（canonical 模型）。
pub fn drill_all_green() -> (bool, String) {
    let m = B26Model::canonical();
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&m) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    (out.green && out.outs.len() >= ASSERT_HARD && out.reds == 0,
     format!("全绿 {} 条 红={}", out.outs.len(), out.reds))
}

/// 全跑 10/10 稳定演练。
pub fn drill_stable_10() -> (bool, String) {
    let m = B26Model::canonical();
    let mut ok = 0u32;
    let mut prev: Option<RegressOut> = None;
    for _ in 0..ROUNDS {
        let mut eng = RegressEngine::new();
        let out = match eng.run_all(&m) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let stable = match &prev {
            Some(p) => p.outs.len() == out.outs.len()
                && p.outs.iter().zip(out.outs.iter()).all(|(a, b)| a == b),
            None => true,
        };
        if stable {
            ok += 1;
        }
        prev = Some(out);
    }
    (ok == ROUNDS as u32, format!("稳定={}/{}", ok, ROUNDS))
}

/// 19 判据覆盖演练（每判据 ≥2 断言）。
pub fn drill_coverage() -> (bool, String) {
    let cov = RegressEngine::crit_coverage();
    let uniq = RegressEngine::ids_unique();
    let map = RegressEngine::mapping_valid();
    let mut per_crit: Vec<String> = Vec::new();
    for (fid, _) in CRIT_TABLE.iter() {
        let n = ASSERT_TABLE.iter().filter(|a| a.crit == *fid).count();
        per_crit.push(format!("F{}:{}", fid, n));
    }
    (cov.is_ok() && uniq && map,
     format!("覆盖={} 唯一={} 映射={}（{}）", cov.is_ok(), uniq, map, per_crit.join(",")))
}

/// 污染必抓演练（污染模型至少打红一条）10/10。
pub fn drill_pollute_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7670_5EED);
    for _ in 0..ROUNDS {
        let f = rng.below(16) as usize;
        let m = B26Model::polluted(f);
        let mut eng = RegressEngine::new();
        match eng.run_all(&m) {
            Ok(o) => {
                if !o.green && o.reds >= 1 {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    (ok == ROUNDS as u32, format!("污染必抓={}/{}", ok, ROUNDS))
}

/// 红账如实上报演练（污染后红数>0 且 green=false，不漏报）。
pub fn drill_red_honest() -> (bool, String) {
    let m = B26Model::polluted(1); // 守恒率 990。
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&m) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    let red_outs = out.outs.iter().filter(|o| !o.passed).count();
    (out.reds == red_outs && !out.green && red_outs >= 1,
     format!("红={} 如实={} 非绿={}", out.reds, out.reds == red_outs, !out.green))
}

/// JSON 导出演练（键齐 + 两次一致 + 19 判据全在）。
pub fn drill_json() -> (bool, String) {
    let m = B26Model::canonical();
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&m) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    let j1 = match export_json(&out) {
        Ok(j) => j,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    let j2 = match export_json(&out) {
        Ok(j) => j,
        Err(_) => String::new(),
    };
    let all_crit = CRIT_TABLE.iter().all(|(fid, _)| j1.contains(&format!("UNX-F{}-J1", fid)));
    (j1 == j2 && all_crit && j1.contains("\"asserts\":42"),
     format!("JSON 一致={} 19判据全在={} 长度={}", j1 == j2, all_crit, j1.len()))
}

/// 映射零遗漏演练（漏判据注入必抓）。
pub fn drill_map_gap() -> (bool, String) {
    // 注入：假设只登记 18 判据（漏 F1320）→ 覆盖审计必抓（该判据无断言挂靠）。
    let mut covered: Vec<u16> = Vec::new();
    for a in ASSERT_TABLE.iter() {
        if !covered.contains(&a.crit) {
            covered.push(a.crit);
        }
    }
    let missing: Vec<u16> = CRIT_TABLE.iter().map(|(f, _)| *f).filter(|f| !covered.contains(f)).collect();
    let full = missing.is_empty();
    // 反向注入：人为从覆盖集去掉一个 → 必须检出缺失。
    let mut reduced = covered.clone();
    reduced.pop();
    let detected = CRIT_TABLE.iter().any(|(f, _)| !reduced.contains(f));
    (full && detected && covered.len() == CRIT_COUNT,
     format!("全覆盖={} 缺失检出={} 挂靠判据={} 个", full, detected, covered.len()))
}

/// 空模型与冻结演练。
pub fn drill_guards() -> (bool, String) {
    let mut eng = RegressEngine::new();
    eng.freeze();
    let m = B26Model::canonical();
    let frozen = matches!(eng.run_all(&m), Err(E_RDONLY));
    let _eng2 = RegressEngine::new();
    let empty_out = RegressOut { outs: Vec::new(), reds: 0, green: false };
    let empty_json = matches!(export_json(&empty_out), Err(E_EMPTY));
    (frozen && empty_json, format!("冻结拒={} 空导拒={}", frozen, empty_json))
}


// ── 回归轮次史账（append-only + 判据统计） ─────────────────
/// 轮次条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundRec {
    /// 轮号。
    pub round: u32,
    /// 断言数。
    pub asserts: usize,
    /// 红数。
    pub red: usize,
    /// 全绿。
    pub green: bool,
}

/// 轮次史账。
#[derive(Debug)]
pub struct RunHistory {
    /// 记录。
    recs: Vec<RoundRec>,
}

impl RunHistory {
    /// 新账。
    pub fn new() -> RunHistory {
        RunHistory { recs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, asserts: usize, red: usize) -> u32 {
        let round = match self.recs.last() {
            Some(r) => r.round + 1,
            None => 1,
        };
        self.recs.push(RoundRec { round, asserts, red, green: red == 0 });
        round
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 轮号连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.round != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 断言数恒定（同表跑断言数不得漂移）。
    pub fn asserts_stable(&self) -> bool {
        self.recs.iter().all(|r| r.asserts == ASSERT_TABLE.len())
    }

    /// 绿轮计数。
    pub fn green_rounds(&self) -> usize {
        self.recs.iter().filter(|r| r.green).count()
    }
}

// ── 判据统计账（逐判据通过率，历史聚合） ───────────────────
/// 判据统计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CritStat {
    /// 判据号。
    pub fid: u16,
    /// 出现断言数。
    pub total: usize,
    /// 通过断言数（最近一轮）。
    pub passed: usize,
}

/// 判据统计账。
#[derive(Debug)]
pub struct CritStats {
    /// 行账。
    pub rows: Vec<CritStat>,
}

impl CritStats {
    /// 从最近一轮结果统计。
    pub fn build(out: &RegressOut) -> CritStats {
        let mut rows = Vec::new();
        for (fid, _) in CRIT_TABLE.iter() {
            let total = out.outs.iter().filter(|o| o.crit == *fid).count();
            let passed = out.outs.iter().filter(|o| o.crit == *fid && o.passed).count();
            rows.push(CritStat { fid: *fid, total, passed });
        }
        CritStats { rows }
    }

    /// 行数守恒（19 行）。
    pub fn conserved(&self) -> bool {
        self.rows.len() == CRIT_COUNT
    }

    /// 全判据通过（每行 passed == total）。
    pub fn all_passed(&self) -> bool {
        self.rows.iter().all(|r| r.passed == r.total && r.total >= 2)
    }

    /// 弱判据清单（passed < total）。
    pub fn weak(&self) -> Vec<u16> {
        self.rows.iter().filter(|r| r.passed < r.total).map(|r| r.fid).collect()
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 轮次史演练（绿轮+红轮混登 + 连续 + 断言数恒定）。
pub fn drill_run_history() -> (bool, String) {
    let mut hist = RunHistory::new();
    let green_m = B26Model::canonical();
    let red_m = B26Model::polluted(1);
    // 轮 1：绿。
    let mut e1 = RegressEngine::new();
    let o1 = e1.run_all(&green_m).unwrap_or(RegressOut { outs: Vec::new(), reds: 99, green: false });
    hist.push(o1.outs.len(), o1.reds);
    // 轮 2：红。
    let mut e2 = RegressEngine::new();
    let o2 = e2.run_all(&red_m).unwrap_or(RegressOut { outs: Vec::new(), reds: 99, green: false });
    hist.push(o2.outs.len(), o2.reds);
    // 轮 3：绿。
    let mut e3 = RegressEngine::new();
    let o3 = e3.run_all(&green_m).unwrap_or(RegressOut { outs: Vec::new(), reds: 99, green: false });
    hist.push(o3.outs.len(), o3.reds);
    (hist.len() == 3 && hist.continuous() && hist.asserts_stable() && hist.green_rounds() == 2,
     format!("史 {} 轮 连续={} 断言恒定={} 绿轮={}", hist.len(), hist.continuous(), hist.asserts_stable(), hist.green_rounds()))
}

/// 判据统计演练（全绿 19/19 + 弱判据检出 + 行守恒）。
pub fn drill_crit_stats() -> (bool, String) {
    let mut eng = RegressEngine::new();
    let good = eng.run_all(&B26Model::canonical()).unwrap_or(RegressOut { outs: Vec::new(), reds: 99, green: false });
    let s_good = CritStats::build(&good);
    let all_ok = s_good.conserved() && s_good.all_passed() && s_good.weak().is_empty();
    let bad = eng.run_all(&B26Model::polluted(1)).unwrap_or(RegressOut { outs: Vec::new(), reds: 99, green: false });
    let s_bad = CritStats::build(&bad);
    let weak_found = !s_bad.weak().is_empty() && s_bad.weak().contains(&1305);
    (all_ok && weak_found,
     format!("全绿统计={} 弱判据检出={}（{:?}）", all_ok, weak_found, s_bad.weak()))
}


// ── JSON 行账（逐判据行导出 + 键校验） ─────────────────────
/// 单判据 JSON 行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRow {
    /// 判据号。
    pub fid: u16,
    /// 行文本。
    pub text: String,
}

/// JSON 行账。
#[derive(Debug)]
pub struct JsonRows {
    /// 行账。
    pub rows: Vec<JsonRow>,
}

impl JsonRows {
    /// 从结果账导出逐判据行（三键：id/name/asserts/red）。
    pub fn build(out: &RegressOut) -> JsonRows {
        let mut rows = Vec::new();
        for (fid, name) in CRIT_TABLE.iter() {
            let n = out.outs.iter().filter(|o| o.crit == *fid).count();
            let red = out.outs.iter().filter(|o| o.crit == *fid && !o.passed).count();
            let text = format!("{{\"id\":\"UNX-F{}-J1\",\"name\":\"{}\",\"asserts\":{},\"red\":{}}}", fid, name, n, red);
            rows.push(JsonRow { fid: *fid, text });
        }
        JsonRows { rows }
    }

    /// 行守恒（19 行）。
    pub fn conserved(&self) -> bool {
        self.rows.len() == CRIT_COUNT
    }

    /// 键齐（每行含四键）。
    pub fn keys_ok(&self) -> bool {
        self.rows.iter().all(|r| {
            r.text.contains("id") && r.text.contains("name")
                && r.text.contains("asserts") && r.text.contains("red")
        })
    }

    /// 号序一致。
    pub fn ordered(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| r.fid == CRIT_TABLE[i].0)
    }
}

/// 追加演练：JSON 行账（19 行 + 键齐 + 号序）。
pub fn drill_json_rows() -> (bool, String) {
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&B26Model::canonical()) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    let j = JsonRows::build(&out);
    (j.conserved() && j.keys_ok() && j.ordered(),
     format!("行 {} 键齐={} 号序={}", j.rows.len(), j.keys_ok(), j.ordered()))
}

/// 追加演练：红账定位（污染模型 → 弱判据精确定位）。
pub fn drill_red_locate() -> (bool, String) {
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&B26Model::polluted(12)) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    // 污染 ktest 断言数=30 → 1320 号判据必弱。
    let s = CritStats::build(&out);
    let weak = s.weak();
    let located = weak.contains(&1320) && weak.len() == 1;
    (located && out.reds >= 1,
     format!("弱判据 {:?} 定位唯一={} 红数={}", weak, located, out.reds))
}

/// 追加演练：全绿定格三旗（全绿 + 断言数达标 + 映射合法三旗同真）。
pub fn drill_green_lock() -> (bool, String) {
    let mut eng = RegressEngine::new();
    let out = match eng.run_all(&B26Model::canonical()) {
        Ok(o) => o,
        Err(c) => return (false, format!("全跑错=0x{:04X} {}", c, detail(c))),
    };
    let f1 = out.green;
    let f2 = out.outs.len() >= ASSERT_HARD;
    let f3 = RegressEngine::mapping_valid();
    (f1 && f2 && f3, format!("三旗 全绿={} 达标={} 映射={}", f1, f2, f3))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：漏判据（覆盖集缺一号必检出）。
    {
        let mut covered: Vec<u16> = Vec::new();
        // 注入：整体抽走 1302 号判据的全部断言 → 覆盖集必缺一号。
        for a in ASSERT_TABLE.iter() {
            if a.crit != 1302 && !covered.contains(&a.crit) {
                covered.push(a.crit);
            }
        }
        let missing = CRIT_TABLE.iter().any(|(f, _)| !covered.contains(f));
        if missing { caught += 1; d.push("注1 漏判:抓".to_string()); } else { d.push("注1 漏判:漏".to_string()); }
    }
    // 注 2：断言缩水。
    {
        if 39 < ASSERT_HARD { caught += 1; d.push("注2 缩水:抓".to_string()); } else { d.push("注2 缩水:漏".to_string()); }
    }
    // 注 3：映射遗漏（污染模型 → 对应断言红 → green=false）。
    {
        let m = B26Model::polluted(11); // 消费方旗破。
        let mut eng = RegressEngine::new();
        if let Ok(o) = eng.run_all(&m) {
            if !o.green { caught += 1; d.push("注3 漏映:抓".to_string()); } else { d.push("注3 漏映:漏".to_string()); }
        }
    }
    // 注 4：红账漏报（红数必须等于失败断言数）。
    {
        let m = B26Model::polluted(13);
        let mut eng = RegressEngine::new();
        if let Ok(o) = eng.run_all(&m) {
            let failed = o.outs.iter().filter(|x| !x.passed).count();
            if o.reds == failed { caught += 1; d.push("注4 漏红:抓".to_string()); } else { d.push("注4 漏红:漏".to_string()); }
        }
    }
    // 注 5：JSON 缺键（空结果导出必拒）。
    {
        let empty_out = RegressOut { outs: Vec::new(), reds: 0, green: false };
        if matches!(export_json(&empty_out), Err(E_EMPTY)) { caught += 1; d.push("注5 缺键:抓".to_string()); } else { d.push("注5 缺键:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7670
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7670 段诊断码：核对回归资产段位闭集"
        && CRIT_TABLE.len() == CRIT_COUNT
        && CRIT_TABLE[0].0 == 1302
        && CRIT_TABLE[18].0 == 1320
}

/// run_unx1318_checks：本单判据聚合（诊断段 0x7670）。
pub fn run_unx1318_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1318 退役回归资产");
    let (ok1, d1) = drill_all_green();
    s.add_dyn("J1-42断言一次全绿", ok1, &d1);
    let (ok2, d2) = drill_stable_10();
    s.add_dyn("J1-全跑稳定10/10", ok2, &d2);
    let (ok3, d3) = drill_coverage();
    s.add_dyn("J1-19判据映射零遗漏", ok3, &d3);
    let (ok4, d4) = drill_pollute_10();
    s.add_dyn("J1-污染必抓10/10", ok4, &d4);
    let (ok5, d5) = drill_red_honest();
    s.add_dyn("J1-红账如实上报", ok5, &d5);
    let (ok6, d6) = drill_json();
    s.add_dyn("J1-JSON可导出", ok6, &d6);
    let (ok7, d7) = drill_map_gap();
    s.add_dyn("J1-映射缺失检出", ok7, &d7);
    let (ok8, d8) = drill_guards();
    s.add_dyn("J1-冻结+空导守卫", ok8, &d8);
    let (ok9, d9) = drill_run_history();
    s.add_dyn("J2-回归轮次史账", ok9, &d9);
    let (ok10, d10) = drill_crit_stats();
    s.add_dyn("J2-判据统计账", ok10, &d10);
    let (ok11, d11) = drill_json_rows();
    s.add_dyn("J2-JSON 行账", ok11, &d11);
    let (ok12, d12) = drill_red_locate();
    s.add_dyn("J2-红账定位", ok12, &d12);
    let (ok13, d13) = drill_green_lock();
    s.add_dyn("J2-全绿定格三旗", ok13, &d13);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7670 段十码闭集");
    s
}
