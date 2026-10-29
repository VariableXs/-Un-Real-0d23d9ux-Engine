//! UNX-F0691 · 矩阵环境一致性自检（镜像/工具链/负载）· 诊断段 0x56e0（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0691-J1 三机环境四查（镜像哈希/写入工具链/宿主负载基线/采集器
//! 版本）全过才启动矩阵，任一不符拦截 100%。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 四查闭集：镜像哈希/写入工具链版本/宿主负载基线/采集器版本，逐机逐查。
//!  3. 一致性口径：三机同镜像同工具链同采集器；负载基线机间差 ≤ 10%。
//!  4. 门控单向门：四查全过才启动矩阵（矩阵启动旗标落账）；任一不符 → 拦截 100%。
//!  5. 拦截零静默：每次拦截留（机/查项/不符值），逐条可解引。
//!  6. 诊断段 0x56e0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x56e0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_MIRROR: u16 = SEG_BASE + 1;   // 查一：镜像哈希不符
pub const E_TOOL: u16 = SEG_BASE + 2;     // 查二：写入工具链不符
pub const E_LOAD: u16 = SEG_BASE + 3;     // 查三：宿主负载基线超差
pub const E_PROBE: u16 = SEG_BASE + 4;    // 查四：采集器版本不符
pub const E_GATE: u16 = SEG_BASE + 5;     // 四查未全过先启动（门控拦截）
pub const E_MACH: u16 = SEG_BASE + 6;     // 机型越界
pub const E_SEAL: u16 = SEG_BASE + 7;     // 矩阵已启动，重门控拦截

/// 负载基线机间差硬线（千分比）。
pub const LOAD_TOL_PM: u32 = 100;
/// 写入工具链版本锚点。
pub const TOOLCHAIN_VER: u16 = 0x0207;
/// 采集器版本锚点。
pub const PROBE_VER: u16 = 0x0015;
/// 镜像哈希锚点。
pub const MIRROR_ANCHOR: u64 = 0x0691_5A17_BEEF_0001;

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 四查项闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckItem {
    /// 查一：镜像哈希。
    Mirror,
    /// 查二：写入工具链。
    Toolchain,
    /// 查三：宿主负载基线。
    LoadBase,
    /// 查四：采集器版本。
    ProbeVer,
}

impl CheckItem {
    pub const ALL: [CheckItem; 4] = [
        CheckItem::Mirror,
        CheckItem::Toolchain,
        CheckItem::LoadBase,
        CheckItem::ProbeVer,
    ];

    pub fn no(self) -> u8 {
        match self {
            CheckItem::Mirror => 1,
            CheckItem::Toolchain => 2,
            CheckItem::LoadBase => 3,
            CheckItem::ProbeVer => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CheckItem::Mirror => "镜像哈希",
            CheckItem::Toolchain => "写入工具链",
            CheckItem::LoadBase => "宿主负载基线",
            CheckItem::ProbeVer => "采集器版本",
        }
    }
}

/// 三机闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// 机一。
    Ma,
    /// 机二。
    Mb,
    /// 机三。
    Mc,
}

impl Machine {
    pub const ALL: [Machine; 3] = [Machine::Ma, Machine::Mb, Machine::Mc];

    pub fn idx(self) -> usize {
        match self {
            Machine::Ma => 0,
            Machine::Mb => 1,
            Machine::Mc => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Machine::Ma => "M-A",
            Machine::Mb => "M-B",
            Machine::Mc => "M-C",
        }
    }
}

/// 单机环境四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvQuad {
    pub mach: Machine,
    pub mirror_hash: u64,
    pub toolchain: u16,
    /// 宿主负载基线（千分负载单位）。
    pub load_base: u32,
    pub probe_ver: u16,
}

impl EnvQuad {
    pub fn audit(&self) -> Result<(), u16> {
        if self.mirror_hash == 0 || self.load_base == 0 {
            return Err(E_MACH);
        }
        Ok(())
    }
}

/// 确定性环境生成（锚点口径内微抖负载）。
pub fn synth_env(m: Machine, seed: u64) -> Result<EnvQuad, u16> {
    let mut rng = Lcg::new(seed ^ (m.idx() as u64 + 1).wrapping_mul(0x2545_0001));
    let load = 300 + (rng.next_u32() % 25) as u32; // 基线 300‰ ± 25
    Ok(EnvQuad {
        mach: m,
        mirror_hash: MIRROR_ANCHOR,
        toolchain: TOOLCHAIN_VER,
        load_base: load,
        probe_ver: PROBE_VER,
    })
}

/// 单机四查：逐项判定，返回首个不符项。
pub fn check_one(env: &EnvQuad, ref_load: u32) -> Result<(), u16> {
    env.audit()?;
    // 查一：镜像哈希
    if env.mirror_hash != MIRROR_ANCHOR {
        return Err(E_MIRROR);
    }
    // 查二：工具链
    if env.toolchain != TOOLCHAIN_VER {
        return Err(E_TOOL);
    }
    // 查三：负载基线（相对基准机）
    if ref_load == 0 {
        return Err(E_LOAD);
    }
    let d = if env.load_base > ref_load {
        ((env.load_base - ref_load) * 1000 / ref_load) as u32
    } else {
        ((ref_load - env.load_base) * 1000 / ref_load) as u32
    };
    if d > LOAD_TOL_PM {
        return Err(E_LOAD);
    }
    // 查四：采集器
    if env.probe_ver != PROBE_VER {
        return Err(E_PROBE);
    }
    Ok(())
}

/// 拦截立案行。
#[derive(Clone, Debug)]
pub struct InterceptRow {
    pub mach: Machine,
    pub item: CheckItem,
    /// 不符观测值。
    pub observed: u64,
    /// 期望锚点。
    pub expect: u64,
}

impl InterceptRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.observed == self.expect {
            return Err(E_GATE);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from(self.mach.name());
        s.push(' ');
        s.push_str(self.item.name());
        s.push_str(" 观测=");
        s.push_str(&self.observed.to_string());
        s.push_str(" 期望=");
        s.push_str(&self.expect.to_string());
        s
    }
}

/// 环境门控。
pub struct EnvGate {
    pub envs: Vec<EnvQuad>,
    pub intercepts: Vec<InterceptRow>,
    pub started: bool,
    /// 四查通过机数。
    pub passed: u32,
}

impl EnvGate {
    pub fn new() -> EnvGate {
        EnvGate { envs: Vec::new(), intercepts: Vec::new(), started: false, passed: 0 }
    }

    /// 装载三机环境。
    pub fn load(&mut self, seed: u64) -> Result<(), u16> {
        if self.started {
            return Err(E_SEAL);
        }
        self.envs.clear();
        for m in Machine::ALL {
            self.envs.push(synth_env(m, seed)?);
        }
        if self.envs.len() != 3 {
            return Err(E_MACH);
        }
        Ok(())
    }

    /// 四查全跑：以基准机负载为参考，逐机四查，不符逐条立案。
    pub fn check_all(&mut self) -> Result<(), u16> {
        if self.started {
            return Err(E_SEAL);
        }
        if self.envs.len() != 3 {
            return Err(E_MACH);
        }
        let ref_load = self.envs[0].load_base;
        self.intercepts.clear();
        self.passed = 0;
        for env in &self.envs {
            let r = check_one(env, ref_load);
            if r.is_ok() {
                self.passed += 1;
                continue;
            }
            let code = match r {
                Err(c) => c,
                Ok(()) => continue,
            };
            let (item, observed, expect) = match code {
                E_MIRROR => (CheckItem::Mirror, env.mirror_hash, MIRROR_ANCHOR),
                E_TOOL => (CheckItem::Toolchain, env.toolchain as u64, TOOLCHAIN_VER as u64),
                E_LOAD => (CheckItem::LoadBase, env.load_base as u64, ref_load as u64),
                E_PROBE => (CheckItem::ProbeVer, env.probe_ver as u64, PROBE_VER as u64),
                _ => return Err(E_GATE),
            };
            let row = InterceptRow { mach: env.mach, item, observed, expect };
            row.audit()?;
            self.intercepts.push(row);
        }
        Ok(())
    }

    /// 门控：四查全过（3 机 × 4 查，拦截 0）才允许启动。
    pub fn gate(&mut self) -> Result<(), u16> {
        if self.started {
            return Err(E_SEAL);
        }
        if self.passed != 3 || !self.intercepts.is_empty() {
            return Err(E_GATE);
        }
        self.started = true;
        Ok(())
    }

    /// 拦截检出率核：注入的不符机数 = 拦截立案数（100%）。
    pub fn assert_intercepted(&self, want: usize) -> Result<(), u16> {
        if self.intercepts.len() != want {
            return Err(E_GATE);
        }
        for r in &self.intercepts {
            r.audit()?;
        }
        Ok(())
    }

    /// 门控前启动拦截核：未全过先启动 → E_GATE。
    pub fn assert_not_started(&self) -> Result<(), u16> {
        if self.started {
            return Err(E_SEAL);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("环境门控: 过=");
        s.push_str(&self.passed.to_string());
        s.push_str("/3 拦截=");
        s.push_str(&self.intercepts.len().to_string());
        s.push_str(" 矩阵=");
        s.push_str(if self.started { "已启动" } else { "未启动" });
        s
    }
}

/// 四查项闭集自检。
pub fn item_closure_audit() -> Result<(), u16> {
    for (i, item) in CheckItem::ALL.iter().enumerate() {
        if item.no() as usize != i + 1 {
            return Err(E_MACH);
        }
        if item.name().is_empty() {
            return Err(E_MACH);
        }
    }
    Ok(())
}

/// 环境确定性自检。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_env(Machine::Mb, 0x0691_0002)?;
    let b = synth_env(Machine::Mb, 0x0691_0002)?;
    if a != b {
        return Err(E_MACH);
    }
    a.audit()?;
    Ok(())
}

/// 镜像不符注入核：改一机镜像 → 四查拦截 + 门控拒启。
pub fn inject_mirror_must_intercept() -> Result<(), u16> {
    let mut g = EnvGate::new();
    g.load(0x0691_0003)?;
    g.envs[1].mirror_hash ^= 0xFF;
    g.check_all()?;
    g.assert_intercepted(1)?;
    if g.intercepts[0].item != CheckItem::Mirror {
        return Err(E_MIRROR);
    }
    if g.gate() != Err(E_GATE) {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 工具链不符注入核。
pub fn inject_tool_must_intercept() -> Result<(), u16> {
    let mut g = EnvGate::new();
    g.load(0x0691_0004)?;
    g.envs[2].toolchain = 0x0208;
    g.check_all()?;
    g.assert_intercepted(1)?;
    if g.intercepts[0].item != CheckItem::Toolchain {
        return Err(E_TOOL);
    }
    Ok(())
}

/// 采集器不符注入核。
pub fn inject_probe_must_intercept() -> Result<(), u16> {
    let mut g = EnvGate::new();
    g.load(0x0691_0005)?;
    g.envs[0].probe_ver = 0x0016;
    g.check_all()?;
    g.assert_intercepted(1)?;
    if g.intercepts[0].item != CheckItem::ProbeVer {
        return Err(E_PROBE);
    }
    Ok(())
}

/// 负载超差注入核：一机负载 +20% → 查三拦截。
pub fn inject_load_must_intercept() -> Result<(), u16> {
    let mut g = EnvGate::new();
    g.load(0x0691_0006)?;
    g.envs[2].load_base = g.envs[0].load_base * 12 / 10; // +20%
    g.check_all()?;
    g.assert_intercepted(1)?;
    if g.intercepts[0].item != CheckItem::LoadBase {
        return Err(E_LOAD);
    }
    Ok(())
}

/// 多机不符注入核：两机不符 → 拦截 2 条（100%）。
pub fn inject_two_mach_must_intercept() -> Result<(), u16> {
    let mut g = EnvGate::new();
    g.load(0x0691_2)?;
    g.envs[0].mirror_hash ^= 1;
    g.envs[1].probe_ver = 0x0099;
    g.check_all()?;
    g.assert_intercepted(2)?;
    if g.gate() != Err(E_GATE) {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 拦截立案篡改注入核：观测=期望 → 行审计检出（非差异行非法）。
pub fn inject_row_noop_must_fail() -> Result<(), u16> {
    let r = InterceptRow {
        mach: Machine::Ma,
        item: CheckItem::Mirror,
        observed: MIRROR_ANCHOR,
        expect: MIRROR_ANCHOR,
    };
    if r.audit() != Err(E_GATE) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 锚点常量自检。
pub fn anchor_audit() -> Result<(), u16> {
    if TOOLCHAIN_VER != 0x0207 || PROBE_VER != 0x0015 || MIRROR_ANCHOR == 0 {
        return Err(E_MACH);
    }
    Ok(())
}

/// 环境四元组零值注入核：镜像 0 / 负载 0 → 审计拦截。
pub fn inject_zero_env_must_fail() -> Result<(), u16> {
    let mut e = synth_env(Machine::Ma, 1)?;
    e.mirror_hash = 0;
    if e.audit() != Err(E_MACH) {
        return Err(SEG_BASE + 0x38);
    }
    let mut e2 = synth_env(Machine::Ma, 1)?;
    e2.load_base = 0;
    if e2.audit() != Err(E_MACH) {
        return Err(SEG_BASE + 0x39);
    }
    Ok(())
}

/// 未装环境先四查注入核：空门控 check_all → 拦截。
pub fn inject_empty_gate_must_fail() -> Result<(), u16> {
    let mut g = EnvGate::new();
    if g.check_all().is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    if g.gate() != Err(E_GATE) {
        return Err(SEG_BASE + 0x3B);
    }
    Ok(())
}

/// 环境自检报告。
pub struct EnvReport {
    pub lines: Vec<String>,
}

impl EnvReport {
    pub fn generate(g: &EnvGate) -> Result<EnvReport, u16> {
        if g.started && (g.passed != 3 || !g.intercepts.is_empty()) {
            return Err(E_GATE);
        }
        let mut lines = Vec::new();
        lines.push(g.summary());
        for env in &g.envs {
            let mut l = String::from(env.mach.name());
            l.push_str(" 镜像=");
            l.push_str(&(env.mirror_hash % 100_000).to_string());
            l.push_str(" 工具链=0x");
            l.push_str(&fmt_hex16(env.toolchain));
            l.push_str(" 负载=");
            l.push_str(&env.load_base.to_string());
            l.push_str(" 采集器=0x");
            l.push_str(&fmt_hex16(env.probe_ver));
            lines.push(l);
        }
        for r in &g.intercepts {
            lines.push(r.summary());
        }
        Ok(EnvReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 4 {
            return Err(E_MACH);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_MACH);
            }
        }
        Ok(())
    }
}

/// 16 位十六进制渲染。
fn fmt_hex16(v: u16) -> String {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    let mut s = String::new();
    for k in 0..4u8 {
        let nib = (v >> (12 - k * 4)) & 0xF;
        s.push(HEX[nib as usize] as char);
    }
    s
}

/// 十六进制渲染自检。
pub fn hex_audit() -> Result<(), u16> {
    if fmt_hex16(0x0207) != "0207" {
        return Err(SEG_BASE + 0x33);
    }
    if fmt_hex16(0x0015) != "0015" {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 自例程：三机四查全过 → 矩阵启动 + 四类不符注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    anchor_audit()?;
    hex_audit()?;
    item_closure_audit()?;
    synth_determinism_audit()?;
    inject_mirror_must_intercept()?;
    inject_tool_must_intercept()?;
    inject_probe_must_intercept()?;
    inject_load_must_intercept()?;
    inject_two_mach_must_intercept()?;
    inject_row_noop_must_fail()?;
    inject_zero_env_must_fail()?;
    inject_empty_gate_must_fail()?;

    // 主路径：三机四查全过 → 启动
    let mut g = EnvGate::new();
    g.load(0x0691_0007)?;
    g.check_all()?;
    if g.passed != 3 || !g.intercepts.is_empty() {
        return Err(E_GATE);
    }
    g.gate()?;
    if g.summary().is_empty() {
        return Err(SEG_BASE + 0x35);
    }
    // 注入：已启动重门控 → 拦截
    if g.gate() != Err(E_SEAL) {
        return Err(SEG_BASE + 0x36);
    }
    if g.load(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    // 报告
    let rep = EnvReport::generate(&g)?;
    rep.assert_complete()?;
    Ok(0)
}
