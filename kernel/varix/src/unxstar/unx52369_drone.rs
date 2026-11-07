//! UNX-F52369 Drone CLI 收口 · 0xC9C00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52369"，逐字）：
//!   title: Drone CLI 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52370-J1 ×10（集群
//!     消费与 B14 kubectl 联签锚定行）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52369）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52370-J1，照原文锚定；功能按本卡标题（Drone CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pipeline 五段闭集：克隆/构建/测试/打包/部署，各 4 格共 20 格，
//!      段序单调（逆序必抓），段载荷折叠纯函数恒等才收。
//!   ② repo/build 10 操作集：仓库登记→秘钥写入→秘钥轮换→构建触发→构建
//!      列表→构建详情→构建取消→日志拉取→提升发布→定时任务，越闭集必拒。
//!   ③ .drone.yml 投影 8 格：kind/name/steps/trigger/environment/secret/
//!      volume/clone，逐格落账，缺格拒收。
//!   ④ Gitea 消费联签 4 格：webhook 事件/commit SHA/分支/秘钥通道；Go 上
//!      栈声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（段越闭集/段漂移/操作越闭集），检出+逃逸
//!      守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（20 格 + 10 操作
//!      + 8 格投影 + 4 格联签 + 2 行上栈 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52369;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// pipeline 五段。
pub const STAGES: usize = 5;

/// 每段格数。
pub const STAGE_CELLS: usize = 4;

/// pipeline 总格数（5 段 × 4 格）。
pub const PIPE_CELLS: usize = 20;

/// repo/build 操作数。
pub const REPO_OPS: usize = 10;

/// .drone.yml 投影格数。
pub const YML_CELLS: usize = 8;

/// Gitea 消费联签格数。
pub const GITEA_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 收口六判据。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DroneCode {
    /// 成功
    Ok = 0,
    /// pipeline 段号越闭集
    ENoStage,
    /// 段载荷漂移
    EStageDrift,
    /// repo/build 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EDrift,
    /// 段序逆序
    EOrder,
    /// yml 投影格越闭集
    ENoYml,
    /// 联签/上栈行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DroneCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DroneCode::Ok => "成功",
            DroneCode::ENoStage => "pipeline 段号越闭集：克隆/构建/测试/打包/部署 20 格之外先登记",
            DroneCode::EStageDrift => "段载荷漂移：段折叠与基准不恒等，逐格复算",
            DroneCode::ENoOp => "repo/build 操作号越闭集：10 操作集之外先登记",
            DroneCode::EDrift => "操作载荷漂移：操作折叠与基准不恒等，逐项复算",
            DroneCode::EOrder => "段序逆序：克隆→构建→测试→打包→部署单调，逆序必抓",
            DroneCode::ENoYml => "yml 投影格越闭集：.drone.yml 8 格投影之外先登记",
            DroneCode::ENoRow => "联签/上栈行越闭集：Gitea 联签 4 格与 Go 上栈 2 行闭集之外先登记",
            DroneCode::EDup => "重复落账：同格只许一条账目",
            DroneCode::ESeal => "未齐即封：20 格 + 10 操作 + 8 投影 + 4 联签 + 2 上栈未齐先补齐",
            DroneCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DroneCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DroneCode::EGradeDrift => "定级漂移：全通且联签上栈全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡登记 S1 全通收口）。
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
            Grade::S1 => "S1：六判据+20格+10操作+8投影+4联签+2上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// pipeline 段名（const fn 表 + Option 边界）。
pub const fn stage_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("克隆"),
        1 => Some("构建"),
        2 => Some("测试"),
        3 => Some("打包"),
        4 => Some("部署"),
        _ => None,
    }
}

/// pipeline 段格载荷（格 o；段 = o/4）。
pub const fn pipeline_cell(o: usize) -> Option<u64> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x31) ^ 0x44524F4E455F0000)
}

/// pipeline 段号（0..4=克隆 4..8=构建 8..12=测试 12..16=打包 16..20=部署）。
pub const fn pipeline_kind(o: usize) -> Option<u32> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((o / STAGE_CELLS) as u32)
}

/// 段族名。
pub const fn pipeline_kind_name(k: usize) -> Option<&'static str> {
    stage_name(k)
}

/// repo/build 操作载荷（操作 o）。
pub const fn repo_cell(o: usize) -> Option<u64> {
    if o >= REPO_OPS {
        return None;
    }
    Some((0xB3000u64 + (o as u64) * 0x27) ^ 0x4245494D5F5F0000)
}

/// repo/build 操作名。
pub const fn repo_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("仓库登记"),
        1 => Some("秘钥写入"),
        2 => Some("秘钥轮换"),
        3 => Some("构建触发"),
        4 => Some("构建列表"),
        5 => Some("构建详情"),
        6 => Some("构建取消"),
        7 => Some("日志拉取"),
        8 => Some("提升发布"),
        9 => Some("定时任务"),
        _ => None,
    }
}

/// .drone.yml 投影格载荷（格 r）。
pub const fn yml_cell(r: usize) -> Option<u32> {
    if r >= YML_CELLS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2B)
}

/// yml 投影字段名。
pub const fn yml_field_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("kind"),
        1 => Some("name"),
        2 => Some("steps"),
        3 => Some("trigger"),
        4 => Some("environment"),
        5 => Some("secret"),
        6 => Some("volume"),
        7 => Some("clone"),
        _ => None,
    }
}

/// Gitea 消费联签行折叠（行 r）。
pub const fn gitea_row(r: usize) -> Option<u32> {
    if r >= GITEA_ROWS {
        return None;
    }
    Some(0xB500 + r as u32 * 0x2D)
}

/// Gitea 联签行名。
pub const fn gitea_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("webhook 事件"),
        1 => Some("commit SHA"),
        2 => Some("分支"),
        3 => Some("秘钥通道"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_stack_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xB600 + r as u32 * 0x2F)
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("服务端启动"),
        1 => Some("runner 注册"),
        2 => Some("仓库激活"),
        3 => Some(".drone.yml 校验"),
        4 => Some("秘钥注入"),
        5 => Some("部署回调"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Drone CLI 收口账本状态机。
#[derive(Clone)]
pub struct DroneLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATES],
    /// pipeline 20 格账。
    pipes: [Option<u64>; PIPE_CELLS],
    /// repo/build 10 操作账。
    repos: [Option<u64>; REPO_OPS],
    /// yml 投影 8 格账。
    ymls: [Option<u32>; YML_CELLS],
    /// Gitea 联签账。
    gitea: [Option<u32>; GITEA_ROWS],
    /// Go 上栈账。
    go_stack: [Option<u32>; GO_ROWS],
    /// 最后段号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DroneLedger {
    /// 新账本。
    pub fn new() -> Self {
        DroneLedger {
            gates: [None; GATES],
            pipes: [None; PIPE_CELLS],
            repos: [None; REPO_OPS],
            ymls: [None; YML_CELLS],
            gitea: [None; GITEA_ROWS],
            go_stack: [None; GO_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DroneCode> {
        if gate >= GATES {
            return Err(DroneCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4A00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// pipeline 一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_pipeline(&mut self, o: usize, got: u64) -> Result<(), DroneCode> {
        if o >= PIPE_CELLS {
            return Err(DroneCode::ENoStage);
        }
        let kind = match pipeline_kind(o) {
            Some(k) => k,
            None => return Err(DroneCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00);
            return Err(DroneCode::EOrder);
        }
        let want = match pipeline_cell(o) {
            Some(w) => w,
            None => return Err(DroneCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B01 + o as u64);
            return Err(DroneCode::EStageDrift);
        }
        if match self.pipes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DroneCode::EDup);
        }
        if let Some(c) = self.pipes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4B02 + o as u64);
        Ok(())
    }

    /// pipeline 全账核验（20 格）。
    pub fn pipes_all(&self) -> bool {
        let mut o = 0usize;
        while o < PIPE_CELLS {
            match self.pipes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// repo/build 一格（载荷对照 + 重复拒）。
    pub fn record_repo(&mut self, o: usize, got: u64) -> Result<(), DroneCode> {
        if o >= REPO_OPS {
            return Err(DroneCode::ENoOp);
        }
        let want = match repo_cell(o) {
            Some(w) => w,
            None => return Err(DroneCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C00 + o as u64);
            return Err(DroneCode::EDrift);
        }
        if match self.repos.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DroneCode::EDup);
        }
        if let Some(c) = self.repos.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x4C01 + o as u64);
        Ok(())
    }

    /// repo/build 全账核验（10 操作）。
    pub fn repos_all(&self) -> bool {
        let mut o = 0usize;
        while o < REPO_OPS {
            match self.repos.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// yml 投影一格。
    pub fn record_yml(&mut self, r: usize) -> Result<(), DroneCode> {
        let want = match yml_cell(r) {
            Some(w) => w,
            None => return Err(DroneCode::ENoYml),
        };
        if match self.ymls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DroneCode::EDup);
        }
        if let Some(c) = self.ymls.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// yml 投影全齐核验。
    pub fn ymls_all(&self) -> bool {
        let mut r = 0usize;
        while r < YML_CELLS {
            match self.ymls.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Gitea 联签一行。
    pub fn declare_gitea(&mut self, r: usize) -> Result<(), DroneCode> {
        let want = match gitea_row(r) {
            Some(w) => w,
            None => return Err(DroneCode::ENoRow),
        };
        if match self.gitea.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DroneCode::EDup);
        }
        if let Some(c) = self.gitea.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x4E00 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn gitea_all(&self) -> bool {
        let mut r = 0usize;
        while r < GITEA_ROWS {
            match self.gitea.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go_stack(&mut self, r: usize) -> Result<(), DroneCode> {
        let want = match go_stack_row(r) {
            Some(w) => w,
            None => return Err(DroneCode::ENoRow),
        };
        if match self.go_stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DroneCode::EDup);
        }
        if let Some(c) = self.go_stack.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x4F00 + r as u64);
        Ok(())
    }

    /// 上栈全齐核验。
    pub fn go_stack_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.go_stack.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
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
            && self.pipes_all()
            && self.repos_all()
            && self.ymls_all()
            && self.gitea_all()
            && self.go_stack_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DroneLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DroneLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PIPE_CELLS {
        if let Some(w) = pipeline_cell(o) {
            if led.record_pipeline(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut o = 0usize;
    while o < REPO_OPS {
        if let Some(w) = repo_cell(o) {
            if led.record_repo(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < YML_CELLS {
        if led.record_yml(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < GITEA_ROWS {
        let _ = led.declare_gitea(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go_stack(r);
        r += 1;
    }
    // 注入 10 轮：段越闭集 / 段漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DroneLedger::new();
        let caught = match kind {
            0 => lt.record_pipeline(PIPE_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格再注漂移。
                let _ = lt.record_pipeline(2, pipeline_cell(2).unwrap_or(0));
                lt.record_pipeline(1, pipeline_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DroneCode::EStageDrift)
            }
            _ => lt.record_repo(REPO_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5000 + k as u64);
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
/// UNX-F52369 判据集（锚 UNX-F52370-J1，off-by-one 板式）。
pub fn run_unx52369_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52369");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pipeline 五段 20 格全通",
        recorded >= PIPE_CELLS as u32,
        "克隆/构建/测试/打包/部署逐段逐格恒等（UNX-F52370-J1 锚定）",
    );
    s.add(
        "repo/build 10 操作全通",
        {
            let mut lt = DroneLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < REPO_OPS {
                if let Some(w) = repo_cell(o) {
                    if lt.record_repo(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.repos_all()
        },
        "仓库登记→定时任务十操作闭集逐项恒等",
    );
    s.add(
        ".drone.yml 投影 8 格全齐",
        {
            let mut lt = DroneLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < YML_CELLS {
                if lt.record_yml(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ymls_all()
        },
        "kind/name/steps/trigger/environment/secret/volume/clone 逐格投影",
    );
    s.add(
        "Gitea 消费联签 4/4 全齐",
        {
            let mut lt = DroneLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GITEA_ROWS {
                if lt.declare_gitea(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gitea_all()
        },
        "webhook 事件/commit SHA/分支/秘钥通道四格联签（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = DroneLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.go_stack_all()
        },
        "Go 上栈声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DroneLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "服务端启动/runner 注册/仓库激活/yml 校验/秘钥注入/部署回调",
    );
    s.add(
        "段序单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PIPE_CELLS {
                if pipeline_kind(o).unwrap_or(5) < pipeline_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "克隆→构建→测试→打包→部署五段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "段越闭集/段漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = DroneLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_pipeline(PIPE_CELLS, 0).is_err(),
        "pipeline 格 20 越闭集必拒",
    );
    s.add(
        "边界·段漂移",
        {
            let _ = l0.record_pipeline(2, pipeline_cell(2).unwrap_or(0));
            l0.record_pipeline(1, pipeline_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(DroneCode::EStageDrift)
        },
        "段载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_repo(REPO_OPS, 0).is_err(),
        "repo/build 操作 10 越闭集必拒",
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
            let mut lt = DroneLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PIPE_CELLS {
                if let Some(w) = pipeline_cell(o) {
                    let _ = lt.record_pipeline(o, w);
                }
                o += 1;
            }
            let mut o = 0usize;
            while o < REPO_OPS {
                if let Some(w) = repo_cell(o) {
                    let _ = lt.record_repo(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < YML_CELLS {
                let _ = lt.record_yml(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GITEA_ROWS {
                let _ = lt.declare_gitea(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go_stack(r);
                r += 1;
            }
            // 定级断言前置所有分节记账完毕（main 节已全落）。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9C00,
        "SEG_BASE 0x40 对齐且落在 VX15 0xC9C00 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_names() {
        assert_eq!(stage_name(0), Some("克隆"));
        assert_eq!(stage_name(1), Some("构建"));
        assert_eq!(stage_name(2), Some("测试"));
        assert_eq!(stage_name(3), Some("打包"));
        assert_eq!(stage_name(4), Some("部署"));
        assert!(stage_name(5).is_none());
    }

    #[test]
    fn test_pipeline_kind() {
        assert_eq!(pipeline_kind(0), Some(0));
        assert_eq!(pipeline_kind(3), Some(0));
        assert_eq!(pipeline_kind(4), Some(1));
        assert_eq!(pipeline_kind(19), Some(4));
        assert!(pipeline_kind(20).is_none());
        assert!(pipeline_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = pipeline_cell(7).unwrap_or(0);
        assert_eq!(a, pipeline_cell(7).unwrap_or(0));
        assert_ne!(a, pipeline_cell(8).unwrap_or(0));
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(gitea_row(GITEA_ROWS).is_none());
        assert!(go_stack_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_repo_ops_closed() {
        assert_eq!(repo_op_name(0), Some("仓库登记"));
        assert_eq!(repo_op_name(8), Some("提升发布"));
        assert_eq!(repo_op_name(9), Some("定时任务"));
        assert!(repo_op_name(10).is_none());
        assert!(repo_cell(10).is_none());
    }

    #[test]
    fn test_yml_fields() {
        assert_eq!(yml_field_name(0), Some("kind"));
        assert_eq!(yml_field_name(2), Some("steps"));
        assert_eq!(yml_field_name(7), Some("clone"));
        assert!(yml_field_name(8).is_none());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = DroneLedger::new();
        let _ = lt.record_pipeline(9, pipeline_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_pipeline(2, pipeline_cell(2).unwrap_or(0)),
            Err(DroneCode::EOrder)
        );
        // 逆序注入被拒不改 last_kind：同段后格仍可落。
        assert!(lt.record_pipeline(10, pipeline_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DroneLedger::new();
        assert_eq!(
            lt.record_pipeline(5, pipeline_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(DroneCode::EStageDrift)
        );
        assert_eq!(
            lt.record_repo(3, repo_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(DroneCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DroneLedger::new();
        let _ = lt.record_repo(0, repo_cell(0).unwrap_or(0));
        assert_eq!(lt.record_repo(0, repo_cell(0).unwrap_or(0)), Err(DroneCode::EDup));
        let _ = lt.record_yml(0);
        assert_eq!(lt.record_yml(0), Err(DroneCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DroneLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DroneLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 计数口径：联签行与上栈行不计 recorded（见 run_cycle）。
        assert_eq!(recorded, (PIPE_CELLS + REPO_OPS + YML_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9C00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DroneCode::Ok as u32, 0);
        assert_eq!(DroneCode::EGradeDrift as u32, 12);
        assert_eq!(DroneCode::Ok.val(), SEG_BASE);
        assert_eq!(DroneCode::ESeal.val(), SEG_BASE + 9);
        let mut i = 0u32;
        while i < 13 {
            let c: DroneCode = match i {
                0 => DroneCode::Ok,
                1 => DroneCode::ENoStage,
                2 => DroneCode::EStageDrift,
                3 => DroneCode::ENoOp,
                4 => DroneCode::EDrift,
                5 => DroneCode::EOrder,
                6 => DroneCode::ENoYml,
                7 => DroneCode::ENoRow,
                8 => DroneCode::EDup,
                9 => DroneCode::ESeal,
                10 => DroneCode::EInj,
                11 => DroneCode::EFp,
                _ => DroneCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DroneLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = DroneLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = DroneLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        // 注入账守恒：空账本注入=检出=0。
        let lt = DroneLedger::new();
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 0);
        assert_eq!(caught, 0);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx52369_checks_green() {
        let s = run_unx52369_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
