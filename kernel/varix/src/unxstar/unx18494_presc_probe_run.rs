//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18494 · 排障处方模式（覆写+探针联用） · 0x7F740 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18494-J1 处方模板（覆写→查询→撤销）跑批一次命令完成，结
//!       果可导出
//! 本模块结构清单：
//!   1. 冻结常量层：步数上限 16 / 覆写键域 0..8 / 判据轮数 10 / 导出协议
//!       RX-RUN v1；
//!   2. 诊断码闭集 RxCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 处方步 Step（op/pid/key/版本四段/探针面 七元）；
//!       - 步结果 StepResult（步号/操作码/过拒/码/值 五元）；
//!       - 引擎 RxEngine：状态机 Idle→Running→Done（单程，重入显拒）
//!         + 键态账 [KeyState; 8] + 位面 plane；
//!       - 处方账 RxBatch：一次跑批 = 全步执行 + 守恒三账；
//!   4. 模板 canonical_template()：覆写→查询→撤销 ×2 键（6 步冻结序）；
//!   5. 一次命令完成：run_rx() 单入口跑完整批——不暴露逐步 API 的语义
//!       是"半程处方不可见"（引擎单程状态机，重入即 EngineStateBad）；
//!   6. 结果可导出：Report::export() → RX-RUN v1 表头+数据行，回放面
//!       逐行解析与账本对拍（往返一致）；
//!   7. 注入核四类：撤销先于创建 / 未知操作码 / 活跃键重复创建 / 幽灵键
//!       ——逐类 10 轮全抓，守恒漏检必 0；
//!   8. 边界：空处方拒 / 超长处方拒 / 全键遍历（0..8）合法；
//!   9. run_unx18494_checks：19 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F740，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F740;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 处方步数上限（写死 16：模板 6 步 × 2 倍余量，2 的幂）。
pub const STEPS_MAX: usize = 16;
/// 覆写键域（冻结 0..8，与 F18491/F18492 同容量——跨卡口径一致）。
pub const KEY_CAP: usize = 8;
/// 进程号域上界（冻结 1..=999）。
pub const PID_MAX: u32 = 999;
/// 版本值域（冻结：与 F18491 同表）。
pub const VER_MAJOR_MAX: u32 = 10;
pub const VER_MINOR_MAX: u32 = 9;
pub const VER_BUILD_MAX: u32 = 65535;
pub const PLAT_MAX: u32 = 3;
/// 操作码闭集：1 创建 / 2 查询 / 3 撤销 / 4 探针。
pub const OP_CREATE: u8 = 1;
pub const OP_QUERY: u8 = 2;
pub const OP_UNDO: u8 = 3;
pub const OP_PROBE: u8 = 4;
/// 探针面闭集（冻结 1..=4：1 矩阵面 / 2 声明面 / 3 兼容层面 / 4 覆写面）。
pub const FACE_MAX: u32 = 4;
/// 兜底裁决（推导：6.1 build 7650 平台 2 打包 = 601076502，与 F18491 同式）。
pub const BASE_VERDICT: u32 = 601_076_502;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 导出协议标签（冻结）。
pub const RX_FMT_TAG: &'static str = "RX-RUN v1";
/// 导出行字段数（写死 5：step|op|ok|code|value）。
pub const RX_FIELDS: usize = 5;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RxCd {
    /// 空处方（0 步）
    EmptyRx,
    /// 超长处方（>16 步）
    RxTooLong,
    /// 未知操作码（闭集 1..4 外）
    StepOpcodeBad,
    /// 幽灵键（0..8 外）
    GhostKey,
    /// 撤销先于创建（Free 键撤销）
    UndoBeforeCreate,
    /// 活跃键重复创建
    DoubleCreate,
    /// 版本域非法
    VerIllegal,
    /// 进程域非法
    PidBad,
    /// 探针面越界
    FaceBad,
    /// 引擎重入（非 Idle 状态开跑）
    EngineStateBad,
    /// 引擎未收口（Running 状态出结果）
    EngineNotDone,
    /// 守恒破坏（步数 ≠ 过+拒）
    ConservationBroken,
    /// 导出行非法
    ExportBad,
    /// 往返破坏
    RoundtripBroken,
    /// 双跑不一致
    NotReproducible,
}

impl RxCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RxCd::EmptyRx => "空处方：0 步不可执行",
            RxCd::RxTooLong => "超长处方：16 步上限",
            RxCd::StepOpcodeBad => "未知操作码：1..4 闭集外",
            RxCd::GhostKey => "幽灵键：0..8 之外",
            RxCd::UndoBeforeCreate => "撤销先于创建：Free 键不可撤销",
            RxCd::DoubleCreate => "重复创建：活跃键不可再建",
            RxCd::VerIllegal => "版本域非法：四段越界",
            RxCd::PidBad => "进程域非法：须 1..=999",
            RxCd::FaceBad => "探针面越界：1..=4",
            RxCd::EngineStateBad => "引擎重入：处方单程执行",
            RxCd::EngineNotDone => "引擎未收口：Running 出结果",
            RxCd::ConservationBroken => "守恒破坏：步数≠过+拒",
            RxCd::ExportBad => "导出行非法：字段/数字错",
            RxCd::RoundtripBroken => "往返破坏：回放与账不一致",
            RxCd::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [RxCd; 15] = [
        RxCd::EmptyRx,
        RxCd::RxTooLong,
        RxCd::StepOpcodeBad,
        RxCd::GhostKey,
        RxCd::UndoBeforeCreate,
        RxCd::DoubleCreate,
        RxCd::VerIllegal,
        RxCd::PidBad,
        RxCd::FaceBad,
        RxCd::EngineStateBad,
        RxCd::EngineNotDone,
        RxCd::ConservationBroken,
        RxCd::ExportBad,
        RxCd::RoundtripBroken,
        RxCd::NotReproducible,
    ];
}

/// FNV-1a 32（步摘要基元）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 处方步 ─────────────────────────────────────────────────────────
/// 处方步（操作码 + 载荷七元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub op: u8,
    pub pid: u32,
    pub key: usize,
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub plat: u32,
    pub face: u32,
}

impl Step {
    /// 创建步构造。
    pub fn create(pid: u32, key: usize, major: u32, minor: u32, build: u32, plat: u32) -> Step {
        Step { op: OP_CREATE, pid, key, major, minor, build, plat, face: 0 }
    }
    /// 查询步构造。
    pub fn query(pid: u32, key: usize) -> Step {
        Step { op: OP_QUERY, pid, key, major: 0, minor: 0, build: 0, plat: 0, face: 0 }
    }
    /// 撤销步构造。
    pub fn undo(pid: u32, key: usize) -> Step {
        Step { op: OP_UNDO, pid, key, major: 0, minor: 0, build: 0, plat: 0, face: 0 }
    }
    /// 探针步构造（face 1..=4）。
    pub fn probe(pid: u32, key: usize, face: u32) -> Step {
        Step { op: OP_PROBE, pid, key, major: 0, minor: 0, build: 0, plat: 0, face }
    }
}

/// 步结果（步号/操作码/过拒/码/值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepResult {
    pub no: usize,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl StepResult {
    /// 零结果（const：回放占位）。
    pub const fn zero() -> StepResult {
        StepResult { no: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

// ── 引擎（状态机 + 键态账 + 位面） ────────────────────────────────
/// 引擎状态机：Idle→Running（run_rx 开跑），Running→Done（收口）。
/// 单程不可重入（Running/Done 上 run_rx 显拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RxState {
    /// 待跑
    Idle,
    /// 跑批中
    Running,
    /// 已收口
    Done,
}

/// 键态：Free→Active（创建），Active→Free（撤销）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyState {
    /// 无覆写
    Free,
    /// 覆写在场
    Active,
}

/// 排障引擎。
pub struct RxEngine {
    pub state: RxState,
    /// 键态账（0..8）。
    pub keys: [KeyState; KEY_CAP],
    /// 活跃位面：第 i 位=键 i 覆写在场。
    pub plane: u8,
    /// 覆写裁决缓存（键→打包版本）。
    pub verdicts: [u32; KEY_CAP],
    pub log: Vec<StepResult>,
    pub oks: u32,
    pub fails: u32,
}

impl RxEngine {
    /// 空引擎（Idle，全键 Free）。
    pub fn new() -> RxEngine {
        RxEngine {
            state: RxState::Idle,
            keys: [KeyState::Free; KEY_CAP],
            plane: 0,
            verdicts: [0u32; KEY_CAP],
            log: Vec::new(),
            oks: 0,
            fails: 0,
        }
    }
    /// 位面查询。
    pub fn active(&self, i: usize) -> bool {
        if i >= KEY_CAP {
            return false;
        }
        (self.plane >> i) & 1 == 1
    }
    /// 位面一致性：位面位 == 键态。
    pub fn plane_consistent(&self) -> bool {
        let mut i = 0;
        while i < KEY_CAP {
            if self.active(i) != (self.keys[i] == KeyState::Active) {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 活跃键计数。
    pub fn active_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0;
        while i < KEY_CAP {
            if self.active(i) {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 单步执行（私有：域闸 + 键态闸 + 落账）。次序：pid → 键 → 操作码
    /// 专属闸（创建：版本域+重复闸；撤销：状态闸；探针：面闸）。
    fn run_step(&mut self, no: usize, st: &Step) -> StepResult {
        // pid 域闸。
        if st.pid == 0 || st.pid > PID_MAX {
            return StepResult { no, op: st.op, ok: false, code: RxCd::PidBad.code(), value: 0 };
        }
        // 键域闸。
        if st.key >= KEY_CAP {
            return StepResult { no, op: st.op, ok: false, code: RxCd::GhostKey.code(), value: 0 };
        }
        match st.op {
            o if o == OP_CREATE => {
                // 版本域闸。
                if st.major == 0
                    || st.major > VER_MAJOR_MAX
                    || st.minor > VER_MINOR_MAX
                    || st.build > VER_BUILD_MAX
                    || st.plat == 0
                    || st.plat > PLAT_MAX
                {
                    return StepResult {
                        no,
                        op: st.op,
                        ok: false,
                        code: RxCd::VerIllegal.code(),
                        value: 0,
                    };
                }
                // 重复创建闸。
                if self.keys[st.key] == KeyState::Active {
                    return StepResult {
                        no,
                        op: st.op,
                        ok: false,
                        code: RxCd::DoubleCreate.code(),
                        value: 0,
                    };
                }
                // 落账：键态/位面/裁决缓存一次写齐。
                self.keys[st.key] = KeyState::Active;
                self.plane |= 1u8 << st.key;
                let packed = st.major * 100_000_000 + st.minor * 1_000_000 + st.build * 10 + st.plat;
                self.verdicts[st.key] = packed;
                StepResult { no, op: st.op, ok: true, code: 0, value: packed }
            }
            o if o == OP_UNDO => {
                // 撤销先于创建闸（Free 键撤销即显拒——处方次序语义）。
                if self.keys[st.key] != KeyState::Active {
                    return StepResult {
                        no,
                        op: st.op,
                        ok: false,
                        code: RxCd::UndoBeforeCreate.code(),
                        value: 0,
                    };
                }
                self.keys[st.key] = KeyState::Free;
                self.plane &= !(1u8 << st.key);
                self.verdicts[st.key] = 0;
                StepResult { no, op: st.op, ok: true, code: 0, value: 0 }
            }
            o if o == OP_QUERY || o == OP_PROBE => {
                // 探针面闸（仅 OP_PROBE 校验）。
                if o == OP_PROBE && (st.face == 0 || st.face > FACE_MAX) {
                    return StepResult {
                        no,
                        op: st.op,
                        ok: false,
                        code: RxCd::FaceBad.code(),
                        value: 0,
                    };
                }
                // 查询/探针恒不落错：活跃键回覆写裁决，空闲键回兜底裁决。
                let v = if self.keys[st.key] == KeyState::Active {
                    self.verdicts[st.key]
                } else {
                    BASE_VERDICT
                };
                StepResult { no, op: st.op, ok: true, code: 0, value: v }
            }
            _ => StepResult { no, op: st.op, ok: false, code: RxCd::StepOpcodeBad.code(), value: 0 },
        }
    }

    /// 一次命令跑完整处方：Idle 开跑→逐步执行→守恒复核→Done 收口。
    /// 返回 Err(码)=处方级拒（空/超长/重入）；步级失败不中断跑批，
    /// 落账后继续（排障语义：收集全部证据一次回报）。
    pub fn run_rx(&mut self, steps: &[Step]) -> Result<(), u32> {
        // 处方级闸：空 / 超长 / 重入。
        if steps.is_empty() {
            return Err(RxCd::EmptyRx.code());
        }
        if steps.len() > STEPS_MAX {
            return Err(RxCd::RxTooLong.code());
        }
        if self.state != RxState::Idle {
            return Err(RxCd::EngineStateBad.code());
        }
        self.state = RxState::Running;
        let mut i = 0;
        while i < steps.len() {
            let r = self.run_step(i, &steps[i]);
            if r.ok {
                self.oks = self.oks.saturating_add(1);
            } else {
                self.fails = self.fails.saturating_add(1);
            }
            self.log.push(r);
            i += 1;
        }
        // 守恒：步数 = 过 + 拒。
        if (self.oks + self.fails) as usize != self.log.len() {
            self.state = RxState::Done;
            return Err(RxCd::ConservationBroken.code());
        }
        if !self.plane_consistent() {
            self.state = RxState::Done;
            return Err(RxCd::ConservationBroken.code());
        }
        self.state = RxState::Done;
        Ok(())
    }

    /// 结果可导出：RX-RUN v1 表头 + 数据行（step|op|ok|code|value）。
    pub fn export(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("{} step|op|ok|code|value n={}", RX_FMT_TAG, self.log.len()));
        let mut i = 0;
        while i < self.log.len() {
            let r = self.log[i];
            out.push(format!(
                "{}|{}|{}|{}|{}",
                r.no,
                r.op,
                if r.ok { 1u32 } else { 0u32 },
                r.code,
                r.value
            ));
            i += 1;
        }
        out
    }
}

// ── 导出行解析（回放面） ───────────────────────────────────────────
/// 数字段解析（十进制无符号，溢出/非数字显拒）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(RxCd::ExportBad.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(RxCd::ExportBad.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(RxCd::ExportBad.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 表头校验（RX-RUN v1 前缀 + n= 计数段）。
pub fn valid_header(line: &str) -> bool {
    let tag = RX_FMT_TAG;
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

/// 单数据行解析：恰五段全数字；ok 段 ∈ {0,1}。
pub fn parse_line(line: &str) -> Result<StepResult, u32> {
    let b = line.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != RX_FIELDS - 1 {
        return Err(RxCd::ExportBad.code());
    }
    let mut segs: [&str; RX_FIELDS] = ["", "", "", "", ""];
    let mut idx = 0usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= b.len() {
        if k == b.len() || b[k] == b'|' {
            segs[idx] = &line[start..k];
            idx += 1;
            start = k + 1;
            if idx > RX_FIELDS {
                return Err(RxCd::ExportBad.code());
            }
        }
        k += 1;
    }
    if idx != RX_FIELDS {
        return Err(RxCd::ExportBad.code());
    }
    let no = parse_u32(segs[0])? as usize;
    let op = parse_u32(segs[1])? as u8;
    let ok = parse_u32(segs[2])?;
    let code = parse_u32(segs[3])?;
    let value = parse_u32(segs[4])?;
    if ok > 1 {
        return Err(RxCd::ExportBad.code());
    }
    Ok(StepResult { no, op, ok: ok == 1, code, value })
}

/// 回放面：跳表头逐行解析。
pub fn replay(lines: &[String]) -> Result<Vec<StepResult>, u32> {
    if lines.is_empty() || !valid_header(&lines[0]) {
        return Err(RxCd::ExportBad.code());
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        out.push(parse_line(&lines[i])?);
        i += 1;
    }
    Ok(out)
}

// ── 冻结模板与黄金链 ───────────────────────────────────────────────
/// 按键派生样板版本（确定性）。
pub fn sample_ver(key: usize) -> (u32, u32, u32, u32) {
    (
        1 + (key % 3) as u32,
        (key % 5) as u32,
        ((key * 977 + 13) % (VER_BUILD_MAX as usize + 1)) as u32,
        1 + (key % 3) as u32,
    )
}

/// 冻结模板：覆写→查询→撤销 ×2 键（6 步，pid 确定性派生）。
pub fn canonical_template() -> [Step; 6] {
    let v0 = sample_ver(0);
    let v1 = sample_ver(1);
    [
        Step::create(101, 0, v0.0, v0.1, v0.2, v0.3),
        Step::query(101, 0),
        Step::undo(101, 0),
        Step::create(202, 1, v1.0, v1.1, v1.2, v1.3),
        Step::query(202, 1),
        Step::undo(202, 1),
    ]
}

/// 黄金全链：冻结模板一次跑批 → 6/6 全过 → 导出 → 回放对拍。
/// 返回 (步数, 过, 拒)。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut eng = RxEngine::new();
    let steps = canonical_template();
    eng.run_rx(&steps)?;
    if eng.state != RxState::Done {
        return Err(RxCd::EngineNotDone.code());
    }
    // 6 步全过（模板是合法序——零拒）。
    if eng.fails != 0 || eng.oks != steps.len() as u32 {
        return Err(RxCd::ConservationBroken.code());
    }
    // 导出 → 回放 → 往返逐字段对拍。
    let lines = eng.export();
    let back = replay(&lines)?;
    if back.len() != eng.log.len() {
        return Err(RxCd::RoundtripBroken.code());
    }
    let mut i = 0;
    while i < eng.log.len() {
        let a = eng.log[i];
        let b = back[i];
        if a.no != b.no || a.op != b.op || a.ok != b.ok || a.code != b.code || a.value != b.value {
            return Err(RxCd::RoundtripBroken.code());
        }
        i += 1;
    }
    Ok((eng.log.len(), eng.oks, eng.fails))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RxCd::NotReproducible.code());
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

/// 四类坏处方注入 × rounds 轮（每轮全新引擎）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：撤销先于创建（Free 键撤销步）。
        let rx0 = [Step::undo(101, 0)];
        let mut e0 = RxEngine::new();
        let hit0 = e0.run_rx(&rx0).is_ok()
            && e0.fails == 1
            && e0.log[0].code == RxCd::UndoBeforeCreate.code();
        led.record(0, hit0);
        // 槽1：未知操作码（op=9）。
        let rx1 = [Step { op: 9, pid: 101, key: 0, major: 0, minor: 0, build: 0, plat: 0, face: 0 }];
        let mut e1 = RxEngine::new();
        let hit1 = e1.run_rx(&rx1).is_ok()
            && e1.fails == 1
            && e1.log[0].code == RxCd::StepOpcodeBad.code();
        led.record(1, hit1);
        // 槽2：活跃键重复创建。
        let v = sample_ver(2);
        let rx2 = [
            Step::create(101, 2, v.0, v.1, v.2, v.3),
            Step::create(101, 2, v.0, v.1, v.2, v.3),
        ];
        let mut e2 = RxEngine::new();
        let hit2 = e2.run_rx(&rx2).is_ok()
            && e2.fails == 1
            && e2.log[1].code == RxCd::DoubleCreate.code();
        led.record(2, hit2);
        // 槽3：幽灵键（key=8）。
        let rx3 = [Step::query(101, KEY_CAP)];
        let mut e3 = RxEngine::new();
        let hit3 = e3.run_rx(&rx3).is_ok()
            && e3.fails == 1
            && e3.log[0].code == RxCd::GhostKey.code();
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 探针联用断言：活跃键探针回覆写裁决，撤销后回兜底（覆写+探针联用语义）。
pub fn probe_union_check() -> Result<(), u32> {
    let v = sample_ver(3);
    let rx = [
        Step::create(303, 3, v.0, v.1, v.2, v.3),
        Step::probe(303, 3, 4),
        Step::undo(303, 3),
        Step::probe(303, 3, 4),
    ];
    let mut eng = RxEngine::new();
    eng.run_rx(&rx)?;
    if eng.fails != 0 {
        return Err(RxCd::ConservationBroken.code());
    }
    // 步1（活跃）覆写裁决 ≠ 兜底；步3（撤销后）== 兜底。
    let packed = v.0 * 100_000_000 + v.1 * 1_000_000 + v.2 * 10 + v.3;
    if eng.log[1].value != packed || eng.log[1].value == BASE_VERDICT {
        return Err(RxCd::ConservationBroken.code());
    }
    if eng.log[3].value != BASE_VERDICT {
        return Err(RxCd::ConservationBroken.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RxCd::ALL.len() {
        let v = RxCd::ALL[i].code();
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
    while i < RxCd::ALL.len() {
        if RxCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 篡改行注入：缺管道/非数字/ok 域非法行必拒。
pub fn inject_bad_line() -> Result<(), u32> {
    let a = parse_line("0|1|1|0|123");
    let b = parse_line("0|1");
    let c = parse_line("0|1|2|0|123");
    if a.is_err() || b.is_ok() || c.is_ok() {
        return Err(RxCd::ExportBad.code());
    }
    Ok(())
}

// ── 判据收口（17 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18494_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18494 排障处方模式（覆写+探针联用）");
    // 1. 黄金全链（冻结模板一次跑批 + 导出回放）。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("steps/ok/fail={:?}", g));
    // 2. 一次命令完成（单入口 run_rx 无逐步 API——引擎单程）。
    let once = {
        let mut eng = RxEngine::new();
        let steps = canonical_template();
        let ok = eng.run_rx(&steps).is_ok() && eng.state == RxState::Done;
        // 重入显拒（单程语义）。
        ok && eng.run_rx(&steps).err() == Some(RxCd::EngineStateBad.code())
    };
    s.add("J1-一次命令完成且单程", once, "run_rx 单入口·重入拒");
    // 3. 模板 6/6 全过。
    let six_ok = match golden_run() {
        Ok((n, ok, fail)) => n == 6 && ok == 6 && fail == 0,
        Err(_) => false,
    };
    s.add("J1-模板6步全过", six_ok, "覆写→查询→撤销 ×2");
    // 4. 覆写+探针联用（活跃回覆写值/撤销后回兜底）。
    s.add("J1-覆写探针联用", probe_union_check().is_ok(), "活跃≠兜底 撤销后=兜底");
    // 5. 结果可导出（表头+行数=n）。
    let exp_ok = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&canonical_template());
        let lines = eng.export();
        lines.len() == 7 && valid_header(&lines[0])
    };
    s.add("J1-结果可导出", exp_ok, "RX-RUN v1 表头+6行");
    // 6. 回放往返。
    let rt = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&canonical_template());
        let lines = eng.export();
        match replay(&lines) {
            Ok(back) => back.len() == 6 && back[0].ok && back[2].op == OP_UNDO,
            Err(_) => false,
        }
    };
    s.add("J1-回放往返", rt, "逐字段对拍");
    // 7. 注入 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 8. 撤销先于创建逐码。
    let u0 = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&[Step::undo(101, 0)]);
        eng.log[0].code == RxCd::UndoBeforeCreate.code()
    };
    s.add("J1-撤销先于创建拒", u0, "UndoBeforeCreate");
    // 9. 未知操作码逐码。
    let opbad = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&[Step { op: 9, pid: 101, key: 0, major: 0, minor: 0, build: 0, plat: 0, face: 0 }]);
        eng.log[0].code == RxCd::StepOpcodeBad.code()
    };
    s.add("J1-未知操作码拒", opbad, "StepOpcodeBad");
    // 10. 重复创建逐码。
    let dup = {
        let v = sample_ver(2);
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&[
            Step::create(101, 2, v.0, v.1, v.2, v.3),
            Step::create(101, 2, v.0, v.1, v.2, v.3),
        ]);
        eng.log[1].code == RxCd::DoubleCreate.code()
    };
    s.add("J1-重复创建拒", dup, "DoubleCreate");
    // 11. 幽灵键逐码。
    let ghost = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&[Step::query(101, KEY_CAP)]);
        eng.log[0].code == RxCd::GhostKey.code()
    };
    s.add("J1-幽灵键拒", ghost, "GhostKey");
    // 12. 空处方拒 / 超长处方拒（边界）。
    let bounds = {
        let mut eng = RxEngine::new();
        let e0 = eng.run_rx(&[]).err() == Some(RxCd::EmptyRx.code());
        let mut eng2 = RxEngine::new();
        let mut long: Vec<Step> = Vec::new();
        let mut k = 0;
        while k <= STEPS_MAX {
            long.push(Step::query(101, 0));
            k += 1;
        }
        e0 && eng2.run_rx(&long).err() == Some(RxCd::RxTooLong.code())
    };
    s.add("J1-空/超长处方拒", bounds, "EmptyRx·RxTooLong");
    // 13. 全键遍历合法（0..8 建撤一轮零拒）。
    let allkeys = {
        let mut eng = RxEngine::new();
        let mut rx: Vec<Step> = Vec::new();
        let mut k = 0;
        while k < KEY_CAP {
            let v = sample_ver(k);
            rx.push(Step::create(101, k, v.0, v.1, v.2, v.3));
            k += 1;
        }
        let mut k2 = 0;
        while k2 < KEY_CAP {
            rx.push(Step::undo(101, k2));
            k2 += 1;
        }
        eng.run_rx(&rx).is_ok() && eng.fails == 0 && eng.active_count() == 0
    };
    s.add("J1-全键遍历合法", allkeys, "0..8 建撤一轮");
    // 14. 步级失败不中断（收集全部证据）。
    let keep = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&[Step::undo(101, 0), Step::undo(101, 0)]);
        eng.log.len() == 2 && eng.fails == 2 && eng.state == RxState::Done
    };
    s.add("J1-步级失败不中断", keep, "全量证据一次回报");
    // 15. 守恒（步数=过+拒 ∧ 位面一致）。
    let cons = {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&canonical_template());
        (eng.oks + eng.fails) as usize == eng.log.len() && eng.plane_consistent()
    };
    s.add("J1-守恒", cons, "steps==ok+fail");
    // 16. 双跑一致 + 篡改行拒。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    s.add("J1-篡改行拒", inject_bad_line().is_ok(), "ExportBad");
    // 17. 段位守卫 + 文案闭集。
    s.add("J1-段位守卫", segment_guard(), "0x7F740 窗口");
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18494_checks_green() {
        let set = run_unx18494_checks();
        assert!(set.all_passed(), "UNX-F18494 red: {}", set.red_items_text());
    }

    #[test]
    fn t_golden_shape() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g, (6, 6, 0), "模板 6 步全过");
    }

    #[test]
    fn t_engine_single_shot() {
        let mut eng = RxEngine::new();
        let steps = canonical_template();
        assert!(eng.run_rx(&steps).is_ok());
        // Running 状态已收口为 Done；重入必拒。
        assert_eq!(eng.state, RxState::Done);
        assert_eq!(eng.run_rx(&steps).err(), Some(RxCd::EngineStateBad.code()));
    }

    #[test]
    fn t_expected_reject_codes() {
        // 撤销先于创建。
        let mut e = RxEngine::new();
        let _ = e.run_rx(&[Step::undo(101, 0)]);
        assert_eq!(e.log[0].code, RxCd::UndoBeforeCreate.code());
        // 未知操作码。
        let mut e2 = RxEngine::new();
        let _ = e2.run_rx(&[Step { op: 9, pid: 101, key: 0, major: 0, minor: 0, build: 0, plat: 0, face: 0 }]);
        assert_eq!(e2.log[0].code, RxCd::StepOpcodeBad.code());
        // 幽灵键。
        let mut e3 = RxEngine::new();
        let _ = e3.run_rx(&[Step::query(101, KEY_CAP)]);
        assert_eq!(e3.log[0].code, RxCd::GhostKey.code());
        // 版本域非法（major=0）。
        let mut e4 = RxEngine::new();
        let _ = e4.run_rx(&[Step::create(101, 0, 0, 1, 100, 1)]);
        assert_eq!(e4.log[0].code, RxCd::VerIllegal.code());
        // 进程域非法。
        let mut e5 = RxEngine::new();
        let _ = e5.run_rx(&[Step::query(0, 0)]);
        assert_eq!(e5.log[0].code, RxCd::PidBad.code());
        // 探针面越界。
        let mut e6 = RxEngine::new();
        let _ = e6.run_rx(&[Step::probe(101, 0, FACE_MAX + 1)]);
        assert_eq!(e6.log[0].code, RxCd::FaceBad.code());
    }

    #[test]
    fn t_ten_rounds_all_caught() {
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
    fn t_probe_union_semantics() {
        // 覆写+探针联用三态：建前兜底 / 建后覆写 / 撤后兜底。
        let v = sample_ver(5);
        let rx = [
            Step::probe(303, 5, 4),
            Step::create(303, 5, v.0, v.1, v.2, v.3),
            Step::probe(303, 5, 4),
            Step::undo(303, 5),
            Step::probe(303, 5, 4),
        ];
        let mut eng = RxEngine::new();
        assert!(eng.run_rx(&rx).is_ok());
        assert_eq!(eng.log[0].value, BASE_VERDICT, "建前兜底");
        let packed = v.0 * 100_000_000 + v.1 * 1_000_000 + v.2 * 10 + v.3;
        assert_eq!(eng.log[2].value, packed, "建后覆写值");
        assert_eq!(eng.log[4].value, BASE_VERDICT, "撤后兜底");
    }

    #[test]
    fn t_export_roundtrip_and_tamper() {
        let mut eng = RxEngine::new();
        let _ = eng.run_rx(&canonical_template());
        let lines = eng.export();
        assert_eq!(lines.len(), 7, "表头+6行");
        assert!(valid_header(&lines[0]));
        assert!(!valid_header("RX-RUN v2 x"), "版标签不符");
        let back = replay(&lines).unwrap_or(Vec::new());
        assert_eq!(back.len(), 6, "回放行数");
        assert_eq!(back[3].op, OP_CREATE, "步3 创建");
        // 篡改行。
        assert!(parse_line("0|1|1|0").is_err(), "管道数不足");
        assert!(parse_line("0|1|x|0|1").is_err(), "非数字");
        assert!(parse_line("0|1|2|0|1").is_err(), "ok 域非法");
    }

    #[test]
    fn t_plane_consistency_after_all() {
        // 全键建→撤一轮后位面零、键态全 Free、位面一致。
        let mut eng = RxEngine::new();
        let mut rx: Vec<Step> = Vec::new();
        let mut k = 0;
        while k < KEY_CAP {
            let v = sample_ver(k);
            rx.push(Step::create(101, k, v.0, v.1, v.2, v.3));
            k += 1;
        }
        let mut k2 = 0;
        while k2 < KEY_CAP {
            rx.push(Step::undo(101, k2));
            k2 += 1;
        }
        assert!(eng.run_rx(&rx).is_ok());
        assert_eq!(eng.plane, 0, "位面清零");
        assert!(eng.plane_consistent());
        assert_eq!(eng.active_count(), 0);
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F740 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(RxCd::UndoBeforeCreate.code(), SEG_BASE + 4, "码位推导");
    }
}
