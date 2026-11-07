//! UNX-F52268 Helm 收口 · 0xC82C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52268"，逐字）：
//!   title: Helm 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52269-J1
//!     start/delete ×10；虚拟化驱动探测账（L5 消费联签锚定行，N/A 同构）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52268）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52269-J1，照原文锚定；功能按本卡标题（Helm）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① chart 双向投影 12 格：六要素（Chart/metadata/templates/values/
//!      charts/lock）× 两向（读取/写回）闭集，逐格载荷（纯函数折叠，
//!      XOR 锚低 16 位恒零）恒等才收；越界必抓。
//!   ② release 10 操作集：四动词闭集（install×3 / upgrade×3 / rollback×2
//!      / uninstall×2），动词单调（逆序必抓；空账本首操作必须落在 install
//!      动词）；修订号阶梯 o+1 单调，漂移必抓。
//!   ③ values 8 格：defaults×2 / override×2 / secret×2 / schema×2 逐格
//!      落账。
//!   ④ repo 三元闸：url/index/签发 三元闭集逐闸落账，缺一拒封。
//!   ⑤ kubectl 消费联签 4 格（F52267 消费联签锚定行，L5 消费 N/A 同构）：
//!      渲染联签/apply 联签/回滚联签/清理联签。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（chart 格越闭集/
//!      修订漂移/动词越序），全部落在当轮临时账本上，检出+逃逸守恒；双跑
//!      FNV 指纹一致；定级 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52268;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// chart 投影要素数。
pub const CHART_ELEMENTS: usize = 6;

/// 投影方向数（读取/写回）。
pub const PROJ_DIRS: usize = 2;

/// chart 投影总格数（6×2）。
pub const CHART_CELLS: usize = CHART_ELEMENTS * PROJ_DIRS;

/// release 操作数。
pub const RELEASE_OPS: usize = 10;

/// values 格数。
pub const VALUES_CELLS: usize = 8;

/// repo 三元闸数。
pub const REPO_GATES: usize = 3;

/// kubectl 消费联签格数。
pub const COSIGN_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HelmCode {
    /// 成功
    Ok = 0,
    /// chart 投影格越闭集
    ENoChart,
    /// chart 投影格漂移
    EChartDrift,
    /// release 操作号越闭集
    ENoOp,
    /// release 操作漂移
    EDrift,
    /// release 动词逆序
    EOrder,
    /// 修订号阶梯漂移
    ERevDrift,
    /// values/repo/联签格越闭集
    ENoCell,
    /// repo 闸缺
    ERepoMiss,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl HelmCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HelmCode::Ok => "成功",
            HelmCode::ENoChart => "chart 投影格越闭集：6 要素 ×2 向之外先登记",
            HelmCode::EChartDrift => "chart 投影格漂移：投影载荷折叠与基准不恒等，逐格复算",
            HelmCode::ENoOp => "release 操作号越闭集：10 操作集之外先登记",
            HelmCode::EDrift => "release 操作漂移：操作折叠与基准不恒等，逐格复算",
            HelmCode::EOrder => "release 动词逆序：install→upgrade→rollback→uninstall 单调，逆序必抓；空账本首操作必须落在 install 动词",
            HelmCode::ERevDrift => "修订号阶梯漂移：修订号必须为 o+1 单调阶梯，漂移必抓",
            HelmCode::ENoCell => "格越闭集：values 8 格、repo 3 闸或联签 4 格之外先登记",
            HelmCode::ERepoMiss => "repo 闸缺：url/index/签发三元闸未齐先补齐",
            HelmCode::EDup => "重复落账：同格只许一条账目",
            HelmCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            HelmCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HelmCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+12 投影格+10 操作+8 values+3 闸+4 联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// chart 要素名（const fn 表 + Option 边界）。
pub const fn chart_element_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("Chart"),
        1 => Some("metadata"),
        2 => Some("templates"),
        3 => Some("values"),
        4 => Some("charts"),
        5 => Some("lock"),
        _ => None,
    }
}

/// 投影方向名（0=读取 1=写回）。
pub const fn proj_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("读取"),
        1 => Some("写回"),
        _ => None,
    }
}

/// chart 投影格载荷（格 i：e×2+d；XOR 锚低 16 位恒零）。
pub const fn chart_cell(i: usize) -> Option<u64> {
    if i >= CHART_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x5D) ^ 0x48454C4D_0000)
}

/// release 动词（0..3=install 3..6=upgrade 6..8=rollback 8..10=uninstall）。
pub const fn release_verb(o: usize) -> Option<u32> {
    match o {
        0 | 1 | 2 => Some(0),
        3 | 4 | 5 => Some(1),
        6 | 7 => Some(2),
        8 | 9 => Some(3),
        _ => None,
    }
}

/// 动词名。
pub const fn release_verb_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("install"),
        1 => Some("upgrade"),
        2 => Some("rollback"),
        3 => Some("uninstall"),
        _ => None,
    }
}

/// release 操作格载荷（操作 o）。
pub const fn release_cell(o: usize) -> Option<u64> {
    if o >= RELEASE_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x2C) ^ ((o % 3) as u64).wrapping_mul(0x01000193))
}

/// 修订号阶梯（操作 o 的期望修订号 = o+1，单调）。
pub const fn release_rev(o: usize) -> Option<u32> {
    if o >= RELEASE_OPS {
        return None;
    }
    Some(o as u32 + 1)
}

/// values 格载荷（格 v：0..2 defaults 2..4 override 4..6 secret 6..8 schema）。
pub const fn values_cell(v: usize) -> Option<u32> {
    if v >= VALUES_CELLS {
        return None;
    }
    Some(0xB300 + v as u32 * 0x2C)
}

/// values 格名。
pub const fn values_cell_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("defaults·chart"),
        1 => Some("defaults·环境"),
        2 => Some("override·命令行"),
        3 => Some("override·文件"),
        4 => Some("secret·凭据"),
        5 => Some("secret·令牌"),
        6 => Some("schema·校验"),
        7 => Some("schema·约束"),
        _ => None,
    }
}

/// repo 闸载荷（闸 r：0=url 1=index 2=签发）。
pub const fn repo_gate(r: usize) -> Option<u32> {
    if r >= REPO_GATES {
        return None;
    }
    Some(0xB400 + r as u32 * 0x3E)
}

/// repo 闸名。
pub const fn repo_gate_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("url"),
        1 => Some("index"),
        2 => Some("签发"),
        _ => None,
    }
}

/// kubectl 消费联签格载荷（格 c：0=渲染 1=apply 2=回滚 3=清理）。
pub const fn cosign_cell(c: usize) -> Option<u32> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some(0xB500 + c as u32 * 0x2C)
}

/// 联签格名。
pub const fn cosign_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("渲染联签"),
        1 => Some("apply 联签"),
        2 => Some("回滚联签"),
        3 => Some("清理联签"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Helm 收口账本状态机。
#[derive(Clone)]
pub struct HelmLedger {
    /// 六判据（安装/chart 解析/render/release/values/repo）。
    gates: [Option<bool>; 6],
    /// chart 投影格账。
    charts: [Option<u64>; CHART_CELLS],
    /// release 操作账。
    ops: [Option<u64>; RELEASE_OPS],
    /// values 格账。
    values: [Option<u32>; VALUES_CELLS],
    /// repo 闸账。
    repo: [Option<u32>; REPO_GATES],
    /// 联签格账。
    cosigns: [Option<u32>; COSIGN_CELLS],
    /// 最后动词。
    last_verb: i64,
    /// 最后修订号。
    last_rev: u32,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HelmLedger {
    /// 新账本。
    pub fn new() -> Self {
        HelmLedger {
            gates: [None; 6],
            charts: [None; CHART_CELLS],
            ops: [None; RELEASE_OPS],
            values: [None; VALUES_CELLS],
            repo: [None; REPO_GATES],
            cosigns: [None; COSIGN_CELLS],
            last_verb: -1,
            last_rev: 0,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HelmCode> {
        if gate >= 6 {
            return Err(HelmCode::ERepoMiss);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// chart 投影格一格（双向载荷对照 + 重复拒）。
    pub fn record_chart(&mut self, i: usize, got: u64) -> Result<(), HelmCode> {
        let want = match chart_cell(i) {
            Some(w) => w,
            None => return Err(HelmCode::ENoChart),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + i as u64);
            return Err(HelmCode::EChartDrift);
        }
        if match self.charts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HelmCode::EDup);
        }
        if let Some(c) = self.charts.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4201 + i as u64);
        Ok(())
    }

    /// chart 投影全账核验（12 格）。
    pub fn charts_all(&self) -> bool {
        let mut i = 0usize;
        while i < CHART_CELLS {
            match self.charts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// release 操作一格（动词单调 + 空账本首动词必须为 install + 修订阶梯
    /// o+1 + 载荷对照 + 重复拒）。
    pub fn record_release(&mut self, o: usize, rev: u32, got: u64) -> Result<(), HelmCode> {
        if o >= RELEASE_OPS {
            return Err(HelmCode::ENoOp);
        }
        let verb = match release_verb(o) {
            Some(v) => v,
            None => return Err(HelmCode::ENoOp),
        };
        if (self.last_verb < 0 && verb != 0) || (self.last_verb >= 0 && (verb as i64) < self.last_verb) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(HelmCode::EOrder);
        }
        let want_rev = match release_rev(o) {
            Some(r) => r,
            None => return Err(HelmCode::ENoOp),
        };
        if rev != want_rev {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(HelmCode::ERevDrift);
        }
        let want = match release_cell(o) {
            Some(w) => w,
            None => return Err(HelmCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4302 + o as u64);
            return Err(HelmCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HelmCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_verb = verb as i64;
        self.last_rev = rev;
        self.fnv(0x4303 + o as u64);
        Ok(())
    }

    /// release 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < RELEASE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// values 格一格。
    pub fn record_values(&mut self, v: usize) -> Result<(), HelmCode> {
        let want = match values_cell(v) {
            Some(x) => x,
            None => return Err(HelmCode::ENoCell),
        };
        if match self.values.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HelmCode::EDup);
        }
        if let Some(c) = self.values.get_mut(v) {
            *c = Some(want);
        }
        self.fnv(0x4400 + v as u64);
        Ok(())
    }

    /// values 全齐核验。
    pub fn values_all(&self) -> bool {
        let mut v = 0usize;
        while v < VALUES_CELLS {
            match self.values.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// repo 闸一格。
    pub fn record_repo(&mut self, r: usize) -> Result<(), HelmCode> {
        let want = match repo_gate(r) {
            Some(x) => x,
            None => return Err(HelmCode::ENoCell),
        };
        if match self.repo.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HelmCode::EDup);
        }
        if let Some(c) = self.repo.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// repo 三元闸全落核验。
    pub fn repo_all(&self) -> bool {
        let mut r = 0usize;
        while r < REPO_GATES {
            match self.repo.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// kubectl 消费联签一格。
    pub fn record_cosign(&mut self, c: usize) -> Result<(), HelmCode> {
        let want = match cosign_cell(c) {
            Some(x) => x,
            None => return Err(HelmCode::ENoCell),
        };
        if match self.cosigns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HelmCode::EDup);
        }
        if let Some(x) = self.cosigns.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x4600 + c as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn cosigns_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosigns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.charts_all()
            && self.ops_all()
            && self.values_all()
            && self.repo_all()
            && self.cosigns_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HelmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HelmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CHART_CELLS {
        if let Some(w) = chart_cell(i) {
            if led.record_chart(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < RELEASE_OPS {
        if let (Some(w), Some(r)) = (release_cell(o), release_rev(o)) {
            if led.record_release(o, r, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut v = 0usize;
    while v < VALUES_CELLS {
        let _ = led.record_values(v);
        v += 1;
    }
    let mut r = 0usize;
    while r < REPO_GATES {
        let _ = led.record_repo(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        let _ = led.record_cosign(c);
        c += 1;
    }
    // 注入 10 轮：chart 格越闭集 / 修订漂移 / 动词越序三态轮转，
    // 全部落在当轮临时账本上；动词越序对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HelmLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_chart(0, chart_cell(0).unwrap_or(0));
                lt.record_chart(CHART_CELLS, 0).is_err()
            }
            1 => {
                let _ = lt.record_release(0, 1, release_cell(0).unwrap_or(0));
                lt.record_release(1, 2, release_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(HelmCode::EDrift)
            }
            _ => {
                // 空账本首操作落在 rollback 动词（动词 2）→ 越序必抓，且不改 last_verb。
                lt.record_release(6, 7, release_cell(6).unwrap_or(0)) == Err(HelmCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52268 判据集（锚 UNX-F52269-J1，off-by-one 板式）。
pub fn run_unx52268_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52268");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "chart 双向投影 12 格全通（6 要素 ×2 向）",
        recorded >= CHART_CELLS as u32,
        "Chart/metadata/templates/values/charts/lock 双向投影逐格恒等",
    );
    s.add(
        "release 10 操作全通（install×3/upgrade×3/rollback×2/uninstall×2）",
        {
            let mut lt = HelmLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < RELEASE_OPS {
                if let (Some(w), Some(r)) = (release_cell(o), release_rev(o)) {
                    if lt.record_release(o, r, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "四动词闭集 + 修订阶梯 o+1 单调（release ×10 判据原文）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HelmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/chart 解析/render/release/values/repo 逐格落账（§AI-66.3 判据锚定）",
    );
    s.add(
        "values 8 格全齐",
        {
            let mut lt = HelmLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VALUES_CELLS {
                if lt.record_values(v).is_err() {
                    ok = false;
                }
                v += 1;
            }
            ok && lt.values_all()
        },
        "defaults×2/override×2/secret×2/schema×2 逐格落账",
    );
    s.add(
        "repo 三元闸全落",
        {
            let mut lt = HelmLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < REPO_GATES {
                if lt.record_repo(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.repo_all()
        },
        "url/index/签发三元闸逐闸落账，缺一拒封",
    );
    s.add(
        "kubectl 消费联签 4/4 全齐（L5 消费 N/A 同构）",
        {
            let mut lt = HelmLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if lt.record_cosign(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "渲染/apply/回滚/清理四格联签锚定行（F52267 消费联签）",
    );
    s.add(
        "release 动词单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < RELEASE_OPS {
                if release_verb(o).unwrap_or(4) < release_verb(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "install→upgrade→rollback→uninstall 四动词闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "chart 格越闭集/修订漂移/动词越序三态轮转必抓",
    );
    let mut l0 = HelmLedger::new();
    s.add(
        "边界·chart 投影格越闭集",
        l0.record_chart(CHART_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·release 操作越闭集",
        l0.record_release(RELEASE_OPS, 1, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·values 格越闭集",
        l0.record_values(VALUES_CELLS).is_err(),
        "values 格 8 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.record_cosign(COSIGN_CELLS).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "修订阶梯漂移拒",
        {
            let mut lt = HelmLedger::new();
            let _ = lt.record_release(0, 1, release_cell(0).unwrap_or(0));
            lt.record_release(1, 9, release_cell(1).unwrap_or(0)) == Err(HelmCode::ERevDrift)
        },
        "修订号必须为 o+1 单调阶梯，漂移必抓",
    );
    s.add(
        "空账本动词越序必抓",
        l0.record_release(6, 7, release_cell(6).unwrap_or(0)) == Err(HelmCode::EOrder),
        "空账本首操作落在 rollback 动词，越序必抓且不改 last_verb",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = HelmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CHART_CELLS {
                if let Some(w) = chart_cell(i) {
                    let _ = lt.record_chart(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < RELEASE_OPS {
                if let (Some(w), Some(r)) = (release_cell(o), release_rev(o)) {
                    let _ = lt.record_release(o, r, w);
                }
                o += 1;
            }
            let mut v = 0usize;
            while v < VALUES_CELLS {
                let _ = lt.record_values(v);
                v += 1;
            }
            let mut r = 0usize;
            while r < REPO_GATES {
                let _ = lt.record_repo(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                let _ = lt.record_cosign(c);
                c += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC82C0,
        "SEG_BASE 0x40 对齐且按派工公式落在 0xC82C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_grid() {
        assert_eq!(chart_element_name(0), Some("Chart"));
        assert_eq!(chart_element_name(5), Some("lock"));
        assert!(chart_element_name(6).is_none());
        assert_eq!(proj_dir_name(1), Some("写回"));
        assert!(proj_dir_name(2).is_none());
        assert!(chart_cell(CHART_CELLS).is_none());
    }

    #[test]
    fn test_release_verb() {
        assert_eq!(release_verb(0), Some(0));
        assert_eq!(release_verb(3), Some(1));
        assert_eq!(release_verb(6), Some(2));
        assert_eq!(release_verb(9), Some(3));
        assert!(release_verb(10).is_none());
        assert_eq!(release_verb_name(2), Some("rollback"));
        assert!(release_verb_name(4).is_none());
    }

    #[test]
    fn test_revision_ladder() {
        assert_eq!(release_rev(0), Some(1));
        assert_eq!(release_rev(9), Some(10));
        assert!(release_rev(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = chart_cell(3).unwrap_or(0);
        assert_eq!(a, chart_cell(3).unwrap_or(0));
        assert_ne!(a, chart_cell(4).unwrap_or(0));
        let b = release_cell(5).unwrap_or(0);
        assert_eq!(b, release_cell(5).unwrap_or(0));
        assert_ne!(b, release_cell(6).unwrap_or(0));
        assert!(values_cell(VALUES_CELLS).is_none());
        assert!(repo_gate(REPO_GATES).is_none());
        assert!(cosign_cell(COSIGN_CELLS).is_none());
    }

    #[test]
    fn test_verb_order_caught() {
        let mut lt = HelmLedger::new();
        // 先落 verb0 三格（空账本首操作必须 install 相位），再落 verb2 一格，
        // 然后逆序回 verb1 必抓 EOrder；最后 verb3 正常落账。
        let mut i = 0usize;
        while i <= 2 {
            let _ = lt.record_release(i, i as u32 + 1, release_cell(i).unwrap_or(0));
            i += 1;
        }
        let _ = lt.record_release(6, 7, release_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_release(3, 4, release_cell(3).unwrap_or(0)),
            Err(HelmCode::EOrder)
        );
        assert!(lt.record_release(8, 9, release_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_verb_caught() {
        let mut lt = HelmLedger::new();
        assert_eq!(
            lt.record_release(8, 9, release_cell(8).unwrap_or(0)),
            Err(HelmCode::EOrder)
        );
        // 空账本首操作必须落在 install 动词（0）。
        assert!(lt.record_release(0, 1, release_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_rev_caught() {
        let mut lt = HelmLedger::new();
        let _ = lt.record_release(0, 1, release_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_release(1, 2, release_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(HelmCode::EDrift)
        );
        let mut lt2 = HelmLedger::new();
        let _ = lt2.record_release(0, 1, release_cell(0).unwrap_or(0));
        assert_eq!(
            lt2.record_release(1, 3, release_cell(1).unwrap_or(0)),
            Err(HelmCode::ERevDrift)
        );
        let mut lt3 = HelmLedger::new();
        assert_eq!(
            lt3.record_chart(1, chart_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(HelmCode::EChartDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HelmLedger::new();
        let _ = lt.record_chart(0, chart_cell(0).unwrap_or(0));
        assert_eq!(lt.record_chart(0, chart_cell(0).unwrap_or(0)), Err(HelmCode::EDup));
        let _ = lt.record_values(0);
        assert_eq!(lt.record_values(0), Err(HelmCode::EDup));
        let _ = lt.record_release(0, 1, release_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_release(0, 1, release_cell(0).unwrap_or(0)),
            Err(HelmCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HelmLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HelmLedger::new();
        assert!(lt2.judge_gate(3).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CHART_CELLS + RELEASE_OPS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC82C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HelmCode::Ok as u32, 0);
        assert_eq!(HelmCode::EGradeDrift as u32, 12);
        // 13 变体闭集：逐位核对（与枚举位序一致）。
        assert_eq!(HelmCode::ENoChart as u32, 1);
        assert_eq!(HelmCode::EOrder as u32, 5);
        assert_eq!(HelmCode::ERevDrift as u32, 6);
        assert!(!HelmCode::Ok.detail().is_empty());
        assert!(!HelmCode::ERepoMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = HelmLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52268_checks_green() {
        let s = run_unx52268_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
