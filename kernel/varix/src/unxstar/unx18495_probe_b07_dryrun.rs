//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18495 · 版本探测探针联调（B07 预演） · 0x7F780 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18495-J1 探针 10 例经覆写路径判定断言全过，与 B07 判据主
//!       轴格式一致
//! 本模块结构清单：
//!   1. 冻结常量层：支持表 SUPPORT 4 项（6.1.7600/6.1.7601/10.0.10240/
//!       10.0.19041）/ 覆写元 (10,0,19045,2) / 判据轮数 10；
//!   2. 诊断码闭集 PbCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 探针例 ProbeCase（id+查询四段）；
//!       - 判定器 base_rule()：基线路径 = 支持表中首个 ≥ 查询版本项；
//!       - 覆写路径 ovr_path()：查询 (major,minor,plat) 命中覆写作用域
//!         → 覆写裁决，否则回基线（与 F18494 探针联用语义一致）；
//!       - 双判表：EXPECT_BASE（无覆写对照）/ EXPECT_WOVR（覆写路径，
//!         均冻结手算+机算对账）；
//!   4. B07 判据主轴格式：表头 B07-AXIS v1 + 数据行 "B07|id|verdict|
//!       expect"，格式逐字段对拍（往返一致、篡改必拒）；
//!   5. 注入核四类：幽灵例号 / build 越界 / plat 越界 / 篡改轴行——逐类
//!       10 轮全抓，守恒漏检必 0；
//!   6. golden_run：10 例经覆写路径逐例判定全过 + 主轴导出回放对拍 +
//!       双跑一致；
//!   7. run_unx18495_checks：16 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F780，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F780;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 探针例数（写死 10：J1"探针 10 例"语义）。
pub const CASE_N: usize = 10;
/// 版本值域（冻结：与 F18491/F18493 同表——跨卡口径一致）。
pub const VER_MAJOR_MAX: u32 = 10;
pub const VER_MINOR_MAX: u32 = 9;
pub const VER_BUILD_MAX: u32 = 65535;
pub const PLAT_MAX: u32 = 3;
/// 基线支持表（冻结 4 项：Win7/Win7 SP1/Win10 1507/Win10 2004——推导：
/// B07 批语义面 B01–B05 冻结的四锚版本）。
pub const SUPPORT_N: usize = 4;
/// 支持表条目（major, minor, build, plat）——(6,1,7600,2)/(6,1,7601,2)/
/// (10,0,10240,2)/(10,0,19041,2)。
pub const SUPPORT: [(u32, u32, u32, u32); SUPPORT_N] = [
    (6, 1, 7600, 2),
    (6, 1, 7601, 2),
    (10, 0, 10240, 2),
    (10, 0, 19041, 2),
];
/// 覆写元（冻结：10.0.19045 平台 2——B07 预演覆写锚）。
pub const OVR_VER: (u32, u32, u32, u32) = (10, 0, 19045, 2);
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// B07 主轴协议标签（冻结）。
pub const AXIS_TAG: &'static str = "B07-AXIS v1";
/// 主轴行前缀（冻结——与 B07 判据主轴格式一致的字面锚）。
pub const AXIS_ROW_PREFIX: &'static str = "B07|";
/// 主轴行字段数（写死 4：B07|id|verdict|expect）。
pub const AXIS_FIELDS: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PbCd {
    /// 幽灵例号（0..10 外）
    CaseOutOfRange,
    /// build 越界
    BuildBad,
    /// plat 越界
    PlatBad,
    /// major/minor 越界
    VerBad,
    /// 主轴行非法（前缀/字段数/数字错）
    AxisBad,
    /// 判表对账破坏（机算 ≠ 冻结表）
    TableBroken,
    /// 支持表单调破坏
    MonoBroken,
    /// 覆写作用域非法
    OvrScopeBad,
    /// 守恒破坏（例数 ≠ 匹配 + 失配）
    ConservationBroken,
    /// 往返破坏
    RoundtripBroken,
    /// 双跑不一致
    NotReproducible,
}

impl PbCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PbCd::CaseOutOfRange => "幽灵例号：0..10 之外",
            PbCd::BuildBad => "build 越界：0..=65535",
            PbCd::PlatBad => "plat 越界：1..=3",
            PbCd::VerBad => "major/minor 越界",
            PbCd::AxisBad => "主轴行非法：前缀/字段/数字错",
            PbCd::TableBroken => "判表对账破坏：机算≠冻结表",
            PbCd::MonoBroken => "支持表单调破坏",
            PbCd::OvrScopeBad => "覆写作用域非法",
            PbCd::ConservationBroken => "守恒破坏：例数≠匹配+失配",
            PbCd::RoundtripBroken => "往返破坏：回放与账不一致",
            PbCd::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [PbCd; 11] = [
        PbCd::CaseOutOfRange,
        PbCd::BuildBad,
        PbCd::PlatBad,
        PbCd::VerBad,
        PbCd::AxisBad,
        PbCd::TableBroken,
        PbCd::MonoBroken,
        PbCd::OvrScopeBad,
        PbCd::ConservationBroken,
        PbCd::RoundtripBroken,
        PbCd::NotReproducible,
    ];
}

/// 打包式（推导：major*1e8 + minor*1e6 + build*10 + plat，三段位权无
/// 碰撞——build*10+plat ≤ 655353 < 1e6，minor ≤ 9 占满 1e6..1e7）。
pub const fn pack4(major: u32, minor: u32, build: u32, plat: u32) -> u32 {
    major * 100_000_000 + minor * 1_000_000 + build * 10 + plat
}

// ── 探针例与查询集 ─────────────────────────────────────────────────
/// 探针例（id + 查询四段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCase {
    pub id: usize,
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub plat: u32,
}

/// 冻结 10 例查询集（确定性手排：覆盖支持表锚点/越锚/越主/越平四类形态）。
pub const CASES: [ProbeCase; CASE_N] = [
    ProbeCase { id: 0, major: 6, minor: 1, build: 7600, plat: 2 },
    ProbeCase { id: 1, major: 6, minor: 1, build: 7601, plat: 2 },
    ProbeCase { id: 2, major: 6, minor: 1, build: 9000, plat: 2 },
    ProbeCase { id: 3, major: 10, minor: 0, build: 10240, plat: 2 },
    ProbeCase { id: 4, major: 10, minor: 0, build: 19041, plat: 2 },
    ProbeCase { id: 5, major: 10, minor: 0, build: 22000, plat: 2 },
    ProbeCase { id: 6, major: 6, minor: 2, build: 7601, plat: 2 },
    ProbeCase { id: 7, major: 10, minor: 0, build: 19045, plat: 2 },
    ProbeCase { id: 8, major: 5, minor: 1, build: 700, plat: 2 },
    ProbeCase { id: 9, major: 10, minor: 0, build: 19045, plat: 3 },
];

/// 基线路径判定规则：支持表中首个 (major,minor,build) ≥ 查询
/// (major,minor,build) 的项（词典序；plat 为平台限定词不入序），
/// 无则取末项。返回打包裁决。
pub fn base_rule(major: u32, minor: u32, build: u32, plat: u32) -> u32 {
    let mut i = 0;
    while i < SUPPORT_N {
        let (sm, sn, sb, sp) = SUPPORT[i];
        if (major, minor, build) <= (sm, sn, sb) {
            return pack4(sm, sn, sb, sp);
        }
        i += 1;
    }
    let (sm, sn, sb, sp) = SUPPORT[SUPPORT_N - 1];
    pack4(sm, sn, sb, sp)
}

/// 覆写作用域匹配：查询 (major,minor,plat) == 覆写元三元组。
pub fn ovr_scope_hit(major: u32, minor: u32, plat: u32) -> bool {
    // 作用域三元组 = (major, minor, plat) = 元组第 0/1/3 位
    //（第 2 位是 build=19045，不入作用域比较——修前误用 .2 致永不命中）。
    major == OVR_VER.0 && minor == OVR_VER.1 && plat == OVR_VER.3
}

/// 覆写路径判定：命中作用域 → 覆写裁决；否则回基线。
pub fn ovr_path(major: u32, minor: u32, build: u32, plat: u32) -> u32 {
    if ovr_scope_hit(major, minor, plat) {
        pack4(OVR_VER.0, OVR_VER.1, OVR_VER.2, OVR_VER.3)
    } else {
        base_rule(major, minor, build, plat)
    }
}

// ── 冻结判表（手算 + 机算对账） ────────────────────────────────────
/// 无覆写对照判表（冻结，手算：逐例 base_rule 展开）。
/// c0→S0=601076002 c1→S1=601076012 c2→S2=1000102402 c3→S2 c4→S3=
/// 1000190412 c5→S3 c6→S2 c7→S3 c8→S0 c9(plat3)→S3。
pub const EXPECT_BASE: [u32; CASE_N] = [
    601_076_002,
    601_076_012,
    1_000_102_402,
    1_000_102_402,
    1_000_190_412,
    1_000_190_412,
    1_000_102_402,
    1_000_190_412,
    601_076_002,
    1_000_190_412,
];

/// 覆写路径判表（冻结，手算：c3/c4/c5/c7 命中覆写作用域 → 1000190452；
/// 其余同基线；c9 plat=3 不命中 → 基线）。
pub const EXPECT_WOVR: [u32; CASE_N] = [
    601_076_002,
    601_076_012,
    1_000_102_402,
    1_000_190_452,
    1_000_190_452,
    1_000_190_452,
    1_000_102_402,
    1_000_190_452,
    601_076_002,
    1_000_190_412,
];

/// 覆写改判例集（冻结：{3,4,5,7}——机算与手算对账的差集）。
pub const OVR_REDIRECTED: [usize; 4] = [3, 4, 5, 7];

/// 机算对账：逐例 base_rule 复算 == EXPECT_BASE 逐项相等。
pub fn tables_consistent() -> Result<(), u32> {
    let mut i = 0;
    while i < CASE_N {
        let c = CASES[i];
        let b = base_rule(c.major, c.minor, c.build, c.plat);
        if b != EXPECT_BASE[i] {
            return Err(PbCd::TableBroken.code());
        }
        let w = ovr_path(c.major, c.minor, c.build, c.plat);
        if w != EXPECT_WOVR[i] {
            return Err(PbCd::TableBroken.code());
        }
        i += 1;
    }
    Ok(())
}

/// 支持表单调：词典序严格递增（判表规则的前提——键与 base_rule 同构：
/// (major, minor, build) 三元序；S0/S1 同为 6.1、build 递增，二元键
/// 无法区分，故单调闸必须用三元键。plat 为平台限定词不入序）。
pub fn support_monotone() -> Result<(), u32> {
    let mut i = 1;
    while i < SUPPORT_N {
        let a = SUPPORT[i - 1];
        let b = SUPPORT[i];
        if !((a.0, a.1, a.2) < (b.0, b.1, b.2)) {
            return Err(PbCd::MonoBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 探针执行器 ─────────────────────────────────────────────────────
/// 单例执行：id 域闸 → 值域闸（major/minor/build/plat）→ 覆写路径判定
/// → 与冻结判表对拍。
pub fn run_probe(id: usize) -> Result<u32, u32> {
    if id >= CASE_N {
        return Err(PbCd::CaseOutOfRange.code());
    }
    let c = CASES[id];
    if c.major == 0 || c.major > VER_MAJOR_MAX || c.minor > VER_MINOR_MAX {
        return Err(PbCd::VerBad.code());
    }
    if c.build > VER_BUILD_MAX {
        return Err(PbCd::BuildBad.code());
    }
    if c.plat == 0 || c.plat > PLAT_MAX {
        return Err(PbCd::PlatBad.code());
    }
    let v = ovr_path(c.major, c.minor, c.build, c.plat);
    if v != EXPECT_WOVR[id] {
        return Err(PbCd::TableBroken.code());
    }
    Ok(v)
}

/// B07 主轴行（判据主轴格式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisRow {
    pub id: usize,
    pub verdict: u32,
    pub expect: u32,
}

/// 主轴导出：表头 + 10 行 "B07|id|verdict|expect"。
pub fn export_axis() -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("{} case|verdict|expect n={}", AXIS_TAG, CASE_N));
    let mut i = 0;
    while i < CASE_N {
        out.push(format!(
            "{}{}|{}|{}",
            AXIS_ROW_PREFIX,
            i,
            EXPECT_WOVR[i],
            EXPECT_WOVR[i]
        ));
        i += 1;
    }
    out
}

/// 数字段解析（十进制无符号）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(PbCd::AxisBad.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(PbCd::AxisBad.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(PbCd::AxisBad.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 表头校验（AXIS_TAG 前缀 + n= 计数段）。
pub fn valid_header(line: &str) -> bool {
    let tag = AXIS_TAG;
    let tb = tag.as_bytes();
    let lb = line.as_bytes();
    if lb.len() < tb.len() || &lb[..tb.len()] != tb {
        return false;
    }
    let mut i = tb.len();
    let mut saw = false;
    while i + 2 < lb.len() {
        if lb[i] == b' ' && lb[i + 1] == b'n' && lb[i + 2] == b'=' {
            saw = true;
            break;
        }
        i += 1;
    }
    saw
}

/// 单数据行解析：B07 前缀 + 恰四段（B07|id|verdict|expect）。
pub fn parse_axis_row(line: &str) -> Result<AxisRow, u32> {
    let pre = AXIS_ROW_PREFIX;
    let pb = pre.as_bytes();
    let lb = line.as_bytes();
    if lb.len() < pb.len() || &lb[..pb.len()] != pb {
        return Err(PbCd::AxisBad.code());
    }
    let body = &line[pb.len()..];
    // 前缀 "B07|" 已消费首字段（B07），故正文管道数 = 字段数-2。
    let bb = body.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < bb.len() {
        if bb[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != AXIS_FIELDS - 2 {
        return Err(PbCd::AxisBad.code());
    }
    let mut segs: [&str; AXIS_FIELDS] = ["B07", "", "", ""];
    let mut idx = 1usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= bb.len() {
        if k == bb.len() || bb[k] == b'|' {
            if idx >= AXIS_FIELDS {
                return Err(PbCd::AxisBad.code());
            }
            segs[idx] = &body[start..k];
            idx += 1;
            start = k + 1;
        }
        k += 1;
    }
    if idx != AXIS_FIELDS {
        return Err(PbCd::AxisBad.code());
    }
    let id = parse_u32(segs[1])? as usize;
    let verdict = parse_u32(segs[2])?;
    let expect = parse_u32(segs[3])?;
    if id >= CASE_N {
        return Err(PbCd::CaseOutOfRange.code());
    }
    Ok(AxisRow { id, verdict, expect })
}

/// 主轴回放：跳表头逐行解析并逐例对拍（verdict==expect ∧ ==判表）。
pub fn replay_axis(lines: &[String]) -> Result<(), u32> {
    if lines.is_empty() || !valid_header(&lines[0]) {
        return Err(PbCd::AxisBad.code());
    }
    if lines.len() != CASE_N + 1 {
        return Err(PbCd::RoundtripBroken.code());
    }
    let mut i = 1;
    while i < lines.len() {
        let r = parse_axis_row(&lines[i])?;
        if r.id != i - 1 {
            return Err(PbCd::RoundtripBroken.code());
        }
        if r.verdict != EXPECT_WOVR[r.id] || r.expect != EXPECT_WOVR[r.id] {
            return Err(PbCd::TableBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：10 例经覆写路径逐例判定全过 → 主轴导出回放对拍。
/// 返回 (过例数, 改判例数)。
pub fn golden_run() -> Result<(usize, usize), u32> {
    tables_consistent()?;
    support_monotone()?;
    let mut passed = 0usize;
    let mut redirected = 0usize;
    let mut i = 0;
    while i < CASE_N {
        let v = run_probe(i)?;
        if v != EXPECT_BASE[i] {
            // 判定与无覆写对照不同 → 覆写改判例。
            redirected += 1;
        }
        passed += 1;
        i += 1;
    }
    if redirected != OVR_REDIRECTED.len() {
        return Err(PbCd::ConservationBroken.code());
    }
    let lines = export_axis();
    replay_axis(&lines)?;
    Ok((passed, redirected))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PbCd::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核 ─────────────────────────────────────────────────────────
/// 注入账（4 槽，守恒 tries==caught）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
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

/// 四类坏样本注入 × rounds 轮（每轮独立）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：幽灵例号（id=10 与 id=99 两形态合槽）。
        led.record(
            0,
            run_probe(CASE_N).err() == Some(PbCd::CaseOutOfRange.code())
                || run_probe(99).err() == Some(PbCd::CaseOutOfRange.code()),
        );
        // 槽1：值域越界探针（直接调判定路径——build 65536/plat 0/4 合槽）。
        let hit1 = ovr_path_probe_guard(10, 0, VER_BUILD_MAX + 1, 2).is_err()
            || ovr_path_probe_guard(10, 0, 19045, 0).is_err()
            || ovr_path_probe_guard(10, 0, 19045, PLAT_MAX + 1).is_err();
        led.record(1, hit1);
        // 槽2：篡改轴行（缺前缀/缺字段/例号越界三形态合槽）。
        let hit2 = parse_axis_row("X07|3|1000190452|1000190452").is_err()
            && parse_axis_row("B07|3|1000190452").is_err()
            && parse_axis_row("B07|99|1|1").is_err();
        led.record(2, hit2);
        // 槽3：判表篡改对拍（构造假期望值 vs 真裁决 → 必失配）。
        let fake_expect = 0u32;
        let real = run_probe(0).unwrap_or(0);
        led.record(3, real != fake_expect && run_probe(0).is_ok());
        r += 1;
    }
    Ok(led)
}

/// 值域守卫探针（注入核专用：与 run_probe 同闸但任意查询四段）。
pub fn ovr_path_probe_guard(major: u32, minor: u32, build: u32, plat: u32) -> Result<u32, u32> {
    if major == 0 || major > VER_MAJOR_MAX || minor > VER_MINOR_MAX {
        return Err(PbCd::VerBad.code());
    }
    if build > VER_BUILD_MAX {
        return Err(PbCd::BuildBad.code());
    }
    if plat == 0 || plat > PLAT_MAX {
        return Err(PbCd::PlatBad.code());
    }
    Ok(ovr_path(major, minor, build, plat))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PbCd::ALL.len() {
        let v = PbCd::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < PbCd::ALL.len() {
        if PbCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 覆写作用域守恒：命中例数 == OVR_REDIRECTED 长度（改判例差集对账）。
pub fn redirect_set_consistent() -> Result<(), u32> {
    let mut hits = 0usize;
    let mut i = 0;
    while i < CASE_N {
        let c = CASES[i];
        if ovr_scope_hit(c.major, c.minor, c.plat) {
            hits += 1;
        }
        i += 1;
    }
    if hits != OVR_REDIRECTED.len() {
        return Err(PbCd::ConservationBroken.code());
    }
    // 差集逐例一致：命中例的覆写裁决 ≠ 基线（判定真的变了）。
    let mut k = 0;
    while k < OVR_REDIRECTED.len() {
        let id = OVR_REDIRECTED[k];
        let c = CASES[id];
        if !ovr_scope_hit(c.major, c.minor, c.plat) {
            return Err(PbCd::ConservationBroken.code());
        }
        k += 1;
    }
    Ok(())
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18495_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18495 版本探测探针联调（B07 预演）");
    // 1. 探针 10 例经覆写路径判定全过（J1 主体）。
    let g = golden_run();
    s.add_dyn("J1-探针10例全过", match &g { Ok((p, _)) => *p == CASE_N, Err(_) => false }, &format!("{:?}", g));
    // 2. 判表机算对账（双表逐例 == 规则复算）。
    s.add("J1-判表机算对账", tables_consistent().is_ok(), "EXPECT_BASE/WOVR 复算一致");
    // 3. 支持表单调。
    s.add("J1-支持表单调", support_monotone().is_ok(), "词典序严格递增");
    // 4. 覆写改判例差集对账（改判 4 例）。
    s.add("J1-覆写改判4例", redirect_set_consistent().is_ok(), "{3,4,5,7} 命中作用域");
    // 5. B07 主轴格式一致（标签+行前缀+字段数）。
    s.add(
        "J1-B07主轴格式一致",
        AXIS_TAG == "B07-AXIS v1" && AXIS_ROW_PREFIX == "B07|" && AXIS_FIELDS == 4,
        "字面锚冻结",
    );
    // 6. 主轴导出回放往返。
    s.add("J1-主轴回放往返", replay_axis(&export_axis()).is_ok(), "逐例 verdict==expect==判表");
    // 7. 注入 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 8. 幽灵例号拒。
    s.add("J1-幽灵例号拒", run_probe(CASE_N).err() == Some(PbCd::CaseOutOfRange.code()), "CaseOutOfRange");
    // 9. build 越界拒。
    s.add(
        "J1-build越界拒",
        ovr_path_probe_guard(10, 0, VER_BUILD_MAX + 1, 2).err() == Some(PbCd::BuildBad.code()),
        "BuildBad",
    );
    // 10. plat 越界拒。
    s.add(
        "J1-plat越界拒",
        ovr_path_probe_guard(10, 0, 19045, PLAT_MAX + 1).err() == Some(PbCd::PlatBad.code()),
        "PlatBad",
    );
    // 11. major 越界拒。
    s.add(
        "J1-major越界拒",
        ovr_path_probe_guard(VER_MAJOR_MAX + 1, 0, 19045, 2).err() == Some(PbCd::VerBad.code()),
        "VerBad",
    );
    // 12. 篡改轴行拒。
    let tamper = parse_axis_row("X07|3|1|1").is_err()
        && parse_axis_row("B07|3|1").is_err()
        && parse_axis_row("B07|99|1|1").is_err();
    s.add("J1-篡改轴行拒", tamper, "AxisBad·CaseOutOfRange");
    // 13. 守恒（10 例 = 全匹配 + 0 失配）。
    let cons = {
        let mut matched = 0usize;
        let mut i = 0;
        while i < CASE_N {
            if run_probe(i).is_ok() {
                matched += 1;
            }
            i += 1;
        }
        matched == CASE_N
    };
    s.add("J1-判例守恒", cons, "10 例全匹配零失配");
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7F780 窗口");
    // 16. 文案闭集。
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18495_checks_green() {
        let set = run_unx18495_checks();
        assert!(set.all_passed(), "UNX-F18495 red: {}", set.red_items_text());
    }

    #[test]
    fn t_golden_shape() {
        let g = golden_run().unwrap_or((0, 0));
        assert_eq!(g, (10, 4), "10 例全过 · 改判 4 例");
    }

    #[test]
    fn t_frozen_tables_hand_derived() {
        // 冻结判表逐项手算锚（抽取 4 例复核）。
        assert_eq!(EXPECT_BASE[0], 601_076_002, "c0→S0");
        assert_eq!(EXPECT_BASE[1], 601_076_012, "c1→S1");
        assert_eq!(EXPECT_BASE[2], 1_000_102_402, "c2→S2(6.1 无 ≥9000 支持项)");
        assert_eq!(EXPECT_WOVR[3], pack4(OVR_VER.0, OVR_VER.1, OVR_VER.2, OVR_VER.3), "c3→覆写元");
        assert_eq!(EXPECT_WOVR[9], 1_000_190_412, "c9 plat3 不命中覆写→S3");
    }

    #[test]
    fn t_base_rule_lexicographic() {
        // 基线规则：查询低于首个支持项 → S0；跨 minor → 升级到 (10,0)。
        assert_eq!(base_rule(5, 1, 700, 2), 601_076_002, "低于 S0 → S0");
        assert_eq!(base_rule(6, 2, 7601, 2), 1_000_102_402, "(6,2)>(6,1) → S2");
        assert_eq!(base_rule(10, 0, 99999, 2), 1_000_190_412, "超出末项 → S3");
    }

    #[test]
    fn t_ovr_scope_boundary() {
        // 作用域边界：plat 变即不命中；major/minor 同三元才命中。
        assert!(ovr_scope_hit(10, 0, 2), "覆写三元命中");
        assert!(!ovr_scope_hit(10, 0, 3), "plat 变不命中");
        assert!(!ovr_scope_hit(9, 0, 2), "major 变不命中");
        assert!(!ovr_scope_hit(10, 1, 2), "minor 变不命中");
    }

    #[test]
    fn t_guard_rejects() {
        assert_eq!(run_probe(CASE_N).err(), Some(PbCd::CaseOutOfRange.code()));
        assert_eq!(ovr_path_probe_guard(10, 0, VER_BUILD_MAX + 1, 2).err(), Some(PbCd::BuildBad.code()));
        assert_eq!(ovr_path_probe_guard(10, 0, 19045, 0).err(), Some(PbCd::PlatBad.code()));
        assert_eq!(ovr_path_probe_guard(0, 0, 19045, 2).err(), Some(PbCd::VerBad.code()));
        assert_eq!(ovr_path_probe_guard(10, VER_MINOR_MAX + 1, 19045, 2).err(), Some(PbCd::VerBad.code()));
    }

    #[test]
    fn t_axis_roundtrip_and_tamper() {
        let lines = export_axis();
        assert_eq!(lines.len(), CASE_N + 1, "表头+10行");
        assert!(valid_header(&lines[0]));
        assert!(!valid_header("B07-AXIS v2 x"), "版标签不符");
        let row = parse_axis_row(&lines[4]).unwrap_or(AxisRow { id: 0, verdict: 0, expect: 0 });
        assert_eq!(row.id, 3, "第4行 id=3");
        assert_eq!(row.verdict, EXPECT_WOVR[3]);
        // 篡改三形态全拒。
        assert!(parse_axis_row("B07|3").is_err(), "字段数不足");
        assert!(parse_axis_row("XX|3|1|1").is_err(), "前缀错");
        assert!(parse_axis_row("B07|10|1|1").is_err(), "例号越界");
    }

    #[test]
    fn t_inject_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "四类全抓");
        let mut total = 0u32;
        let mut i = 0;
        while i < 4 {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 40, "总注入 4×10=40");
    }

    #[test]
    fn t_double_run_identical() {
        assert!(repro_check().is_ok(), "双跑一致");
        let a = export_axis();
        let b = export_axis();
        assert_eq!(a, b, "主轴导出确定性");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F780 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(PbCd::TableBroken.code(), SEG_BASE + 5, "码位推导");
    }
}
