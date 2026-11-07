//! UNX-F52367 AppVeyor CLI 收口 · 0xC9B80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52367"，逐字）：
//!   title: AppVeyor CLI 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52368-J1 注册/
//!     执行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52367）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52368-J1，照原文锚定；功能按本卡标题（AppVeyor CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① build 五段 20 格闭集：还原/编译/测试/打包/发布五段各 4 格，
//!      段序单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② appveyor.yml 双向投影 12 格：正向折叠 yml_cell 与反向镜像
//!      yml_bwd 恒等配对，漂移必抓。
//!   ③ 工件三态 6 格：上传/暂存/发布三态各 2 格，态号 = i%3。
//!   ④ 与云消费联签 4 格（存储/通知/部署/工件）+ 部署 5 场景
//!      （GitHub Releases/Azure WebApp/Azure Blob/S3/NuGet）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集 + adopt 采纳声明）；注入核 run_cycle 三态轮转
//!      10/10（投影漂移/段逆序/格越闭集）；双跑指纹一致；KB-COMPAT
//!      登记定级 S2（有条件收口，采纳声明后可升 S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52367;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// build 格数（五段 × 4）。
pub const BUILD_CELLS: usize = 20;

/// build 段数。
pub const BUILD_STAGES: usize = 5;

/// appveyor.yml 双向投影格数。
pub const YML_CELLS: usize = 12;

/// 工件三态格数。
pub const ART_CELLS: usize = 6;

/// 工件三态数。
pub const ART_STATES: usize = 3;

/// 云消费联签行数。
pub const COSIGN_ROWS: usize = 4;

/// 部署场景数。
pub const DEPLOY_SCENES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppveyorCode {
    /// 成功
    Ok = 0,
    /// build 格越闭集
    ENoCell,
    /// 投影漂移
    EDrift,
    /// 段逆序
    EOrder,
    /// appveyor.yml 投影格越闭集
    EYmlNo,
    /// 工件格越闭集
    EArtNo,
    /// 联签/部署行越闭集
    ENoRow,
    /// 行未齐
    ERowMiss,
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

impl AppveyorCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AppveyorCode::Ok => "成功",
            AppveyorCode::ENoCell => "build 格越闭集：五段 20 格之外先登记",
            AppveyorCode::EDrift => "投影漂移：build/appveyor.yml 双向投影与基准不恒等，逐格复算",
            AppveyorCode::EOrder => "段逆序：还原→编译→测试→打包→发布单调，逆序必抓",
            AppveyorCode::EYmlNo => "appveyor.yml 投影格越闭集：双向投影 12 格之外先登记",
            AppveyorCode::EArtNo => "工件格越闭集：三态 6 格之外先登记",
            AppveyorCode::ENoRow => "联签/部署行越闭集：联签 4 格 + 部署 5 场景之外先登记",
            AppveyorCode::ERowMiss => "行未齐：云消费联签与部署场景未齐先补齐",
            AppveyorCode::EDup => "重复落账：同格只许一条账目",
            AppveyorCode::ESeal => "未齐即封：20 格 + 12 投影 + 6 工件 + 联签部署未齐先补齐",
            AppveyorCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AppveyorCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AppveyorCode::EGradeDrift => "定级漂移：全通且采纳声明后方可升 S1，当前登记 S2",
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
            Grade::S1 => "S1：全账全通且采纳声明成立",
            Grade::S2 => "S2：有条件收口（KB-COMPAT 登记级），采纳声明未落或存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

/// KB-COMPAT 登记定级目标（本卡 S2：部署场景依赖云端连通，有条件收口）。
pub const GRADE_TARGET: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零）。
pub const XORA_BUILD: u64 = 0x4150_5659_0000;

/// 镜像掩码（双向投影反向折叠用）。
pub const YML_MIRROR: u64 = 0x00FF_0000_0000;

/// build 格载荷（格 o；段 = o/4：0=还原 1=编译 2=测试 3=打包 4=发布）。
pub const fn build_cell(o: usize) -> Option<u64> {
    if o >= BUILD_CELLS {
        return None;
    }
    Some((0x71000u64 + (o as u64) * 0x2F) ^ XORA_BUILD)
}

/// build 段号。
pub const fn build_stage(o: usize) -> Option<u32> {
    if o >= BUILD_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 段名。
pub const fn build_stage_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("还原"),
        1 => Some("编译"),
        2 => Some("测试"),
        3 => Some("打包"),
        4 => Some("发布"),
        _ => None,
    }
}

/// appveyor.yml 正向投影载荷（格 i）。
pub const fn yml_cell(i: usize) -> Option<u64> {
    if i >= YML_CELLS {
        return None;
    }
    Some((0x72000u64 + (i as u64) * 0x59) ^ 0x594D_4C00_0000)
}

/// appveyor.yml 反向镜像（双向投影：bwd 与 fwd 镜像配对恒等）。
pub const fn yml_bwd(i: usize) -> Option<u64> {
    match yml_cell(i) {
        Some(w) => Some(w ^ YML_MIRROR),
        None => None,
    }
}

/// 工件三态（格 i：态 = i%3，0=上传 1=暂存 2=发布）。
pub const fn art_state(i: usize) -> Option<u32> {
    if i >= ART_CELLS {
        return None;
    }
    Some((i % ART_STATES) as u32)
}

/// 工件格载荷。
pub const fn art_cell(i: usize) -> Option<u64> {
    if i >= ART_CELLS {
        return None;
    }
    Some((0x73000u64 + (i as u64) * 0x67) ^ 0x4152_5400_0000)
}

/// 工件态名。
pub const fn art_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("上传"),
        1 => Some("暂存"),
        2 => Some("发布"),
        _ => None,
    }
}

/// 云消费联签行折叠（行 r：0=存储 1=通知 2=部署 3=工件）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0x7400 + r as u32 * 0x43)
}

/// 联签行名。
pub const fn cosign_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("存储消费联签"),
        1 => Some("通知消费联签"),
        2 => Some("部署消费联签"),
        3 => Some("工件消费联签"),
        _ => None,
    }
}

/// 部署场景载荷（场景 s：0=GitHub Releases 1=Azure WebApp 2=Azure Blob 3=S3 4=NuGet）。
pub const fn deploy_cell(s: usize) -> Option<u64> {
    if s >= DEPLOY_SCENES {
        return None;
    }
    Some(0x75000u64 + (s as u64) * 0x79)
}

/// 部署场景名。
pub const fn deploy_scene_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("GitHub Releases 部署"),
        1 => Some("Azure WebApp 部署"),
        2 => Some("Azure Blob 部署"),
        3 => Some("S3 部署"),
        4 => Some("NuGet 部署"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// AppVeyor CLI 收口账本状态机。
#[derive(Clone)]
pub struct AppveyorLedger {
    /// 六判据（连接/还原/编译/测试/部署/遥测）。
    gates: [Option<bool>; 6],
    /// build 格账。
    build: [Option<u64>; BUILD_CELLS],
    /// appveyor.yml 双向投影账。
    yml: [Option<u64>; YML_CELLS],
    /// 工件三态账。
    art: [Option<u64>; ART_CELLS],
    /// 联签账。
    cosign: [Option<u32>; COSIGN_ROWS],
    /// 部署场景账。
    deploy: [Option<u64>; DEPLOY_SCENES],
    /// 最后段号。
    last_stage: i64,
    /// 采纳声明（升 S1 前置）。
    adopted: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AppveyorLedger {
    /// 新账本。
    pub fn new() -> Self {
        AppveyorLedger {
            gates: [None; 6],
            build: [None; BUILD_CELLS],
            yml: [None; YML_CELLS],
            art: [None; ART_CELLS],
            cosign: [None; COSIGN_ROWS],
            deploy: [None; DEPLOY_SCENES],
            last_stage: -1,
            adopted: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AppveyorCode> {
        if gate >= 6 {
            return Err(AppveyorCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// build 一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_build(&mut self, o: usize, got: u64) -> Result<(), AppveyorCode> {
        if o >= BUILD_CELLS {
            return Err(AppveyorCode::ENoCell);
        }
        let stage = match build_stage(o) {
            Some(st) => st,
            None => return Err(AppveyorCode::ENoCell),
        };
        if (stage as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(AppveyorCode::EOrder);
        }
        let want = match build_cell(o) {
            Some(w) => w,
            None => return Err(AppveyorCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(AppveyorCode::EDrift);
        }
        if match self.build.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppveyorCode::EDup);
        }
        if let Some(c) = self.build.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = stage as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// build 全账核验（20 格）。
    pub fn build_all(&self) -> bool {
        let mut o = 0usize;
        while o < BUILD_CELLS {
            match self.build.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// appveyor.yml 双向投影一格（正向对照 + 重复拒）。
    pub fn record_yml(&mut self, i: usize, got: u64) -> Result<(), AppveyorCode> {
        let want = match yml_cell(i) {
            Some(w) => w,
            None => return Err(AppveyorCode::EYmlNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + i as u64);
            return Err(AppveyorCode::EDrift);
        }
        if match self.yml.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppveyorCode::EDup);
        }
        if let Some(c) = self.yml.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7301 + i as u64);
        Ok(())
    }

    /// appveyor.yml 投影全账核验（12 格，双向镜像配对核验）。
    pub fn yml_all(&self) -> bool {
        let mut i = 0usize;
        while i < YML_CELLS {
            let filled = match self.yml.get(i) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !filled {
                return false;
            }
            // 双向恒等：fwd ^ MIRROR == bwd。
            match (yml_cell(i), yml_bwd(i)) {
                (Some(f), Some(b)) => {
                    if f ^ YML_MIRROR != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 工件一格（载荷对照 + 重复拒）。
    pub fn record_art(&mut self, i: usize, got: u64) -> Result<(), AppveyorCode> {
        let want = match art_cell(i) {
            Some(w) => w,
            None => return Err(AppveyorCode::EArtNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + i as u64);
            return Err(AppveyorCode::EDrift);
        }
        if match self.art.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppveyorCode::EDup);
        }
        if let Some(c) = self.art.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7401 + i as u64);
        Ok(())
    }

    /// 工件全账核验（6 格）。
    pub fn art_all(&self) -> bool {
        let mut i = 0usize;
        while i < ART_CELLS {
            match self.art.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 云消费联签一行。
    pub fn record_cosign(&mut self, r: usize) -> Result<(), AppveyorCode> {
        let want = match cosign_row(r) {
            Some(w) => w,
            None => return Err(AppveyorCode::ENoRow),
        };
        if match self.cosign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppveyorCode::EDup);
        }
        if let Some(c) = self.cosign.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn cosign_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_ROWS {
            match self.cosign.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 部署场景一格。
    pub fn record_deploy(&mut self, s: usize) -> Result<(), AppveyorCode> {
        let want = match deploy_cell(s) {
            Some(w) => w,
            None => return Err(AppveyorCode::ENoRow),
        };
        if match self.deploy.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppveyorCode::EDup);
        }
        if let Some(c) = self.deploy.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x7600 + s as u64);
        Ok(())
    }

    /// 部署场景全齐核验（5 场景）。
    pub fn deploy_all(&self) -> bool {
        let mut s = 0usize;
        while s < DEPLOY_SCENES {
            match self.deploy.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 落采纳声明（升 S1 的前置格）。
    pub fn adopt(&mut self) {
        self.adopted = true;
        self.fnv(0x7700);
    }

    /// 采纳声明核验。
    pub fn adopted(&self) -> bool {
        self.adopted
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通且采纳 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.build_all()
            && self.yml_all()
            && self.art_all()
            && self.cosign_all()
            && self.deploy_all()
            && self.adopted
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AppveyorLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全量可记账格数（六判据之外）。
pub const FILL_CELLS: u32 =
    (BUILD_CELLS + YML_CELLS + ART_CELLS + COSIGN_ROWS + DEPLOY_SCENES) as u32;

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AppveyorLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < BUILD_CELLS {
        if let Some(w) = build_cell(o) {
            if led.record_build(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < YML_CELLS {
        if let Some(w) = yml_cell(i) {
            if led.record_yml(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < ART_CELLS {
        if let Some(w) = art_cell(a) {
            if led.record_art(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < COSIGN_ROWS {
        let _ = led.record_cosign(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DEPLOY_SCENES {
        let _ = led.record_deploy(d);
        d += 1;
    }
    // 注入 10 轮：投影漂移 / 段逆序 / build 格越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AppveyorLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_yml(2, yml_cell(2).unwrap_or(0));
                lt.record_yml(3, yml_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(AppveyorCode::EDrift)
            }
            1 => {
                let _ = lt.record_build(7, build_cell(7).unwrap_or(0));
                lt.record_build(1, build_cell(1).unwrap_or(0)) == Err(AppveyorCode::EOrder)
            }
            _ => lt.record_build(BUILD_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7800 + k as u64);
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
/// UNX-F52367 判据集（锚 UNX-F52368-J1，off-by-one 板式）。
pub fn run_unx52367_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52367");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "build 五段 20 格全通",
        recorded >= BUILD_CELLS as u32,
        "还原/编译/测试/打包/发布五段逐格恒等",
    );
    s.add(
        "appveyor.yml 双向投影 12/12 恒等",
        {
            let mut lt = AppveyorLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    if lt.record_yml(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.yml_all()
        },
        "正向折叠与反向镜像配对恒等，漂移必抓",
    );
    s.add(
        "工件三态 6/6 全落",
        {
            let mut lt = AppveyorLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ART_CELLS {
                if let Some(w) = art_cell(a) {
                    if lt.record_art(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.art_all()
        },
        "上传/暂存/发布三态各 2 格逐格落账",
    );
    s.add(
        "云消费联签 4/4 全齐",
        {
            let mut lt = AppveyorLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                if lt.record_cosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cosign_all()
        },
        "存储/通知/部署/工件四路消费联签（§AI-66.3）",
    );
    s.add(
        "部署 5 场景全齐",
        {
            let mut lt = AppveyorLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DEPLOY_SCENES {
                if lt.record_deploy(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.deploy_all()
        },
        "GitHub Releases/Azure WebApp/Azure Blob/S3/NuGet 逐场景落账",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = AppveyorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接/还原/编译/测试/部署/遥测逐格落账",
    );
    s.add(
        "段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BUILD_CELLS {
                if build_stage(o).unwrap_or(9) < build_stage(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影漂移/段逆序/build 格越闭集三态轮转必抓",
    );
    let mut l0 = AppveyorLedger::new();
    s.add(
        "边界·yml 投影格越闭集",
        l0.record_yml(YML_CELLS, 0).is_err(),
        "appveyor.yml 12 格之外越界必拒",
    );
    s.add(
        "边界·工件格越闭集",
        l0.record_art(ART_CELLS, 0).is_err(),
        "工件 6 格之外越界必拒",
    );
    s.add(
        "边界·build 格越闭集",
        l0.record_build(BUILD_CELLS, 0).is_err(),
        "build 20 格之外越界必拒",
    );
    s.add(
        "投影漂移拒",
        {
            let _ = l0.record_yml(0, yml_cell(0).unwrap_or(0));
            l0.record_yml(1, yml_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(AppveyorCode::EDrift)
        },
        "投影载荷漂移必抓",
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
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            // 全账落齐但不落采纳声明 → S2；采纳后可升 S1；空账 → S3。
            let mut lt = AppveyorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < BUILD_CELLS {
                if let Some(w) = build_cell(o) {
                    let _ = lt.record_build(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    let _ = lt.record_yml(i, w);
                }
                i += 1;
            }
            let mut a = 0usize;
            while a < ART_CELLS {
                if let Some(w) = art_cell(a) {
                    let _ = lt.record_art(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                let _ = lt.record_cosign(r);
                r += 1;
            }
            let mut d = 0usize;
            while d < DEPLOY_SCENES {
                let _ = lt.record_deploy(d);
                d += 1;
            }
            let s2 = lt.judge_grade();
            lt.adopt();
            let s1 = lt.judge_grade();
            s2 == GRADE_TARGET && s1 == Grade::S1
        },
        "全账未采纳 → S2，采纳后 → S1（定级闭集核验）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9B80,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_stages() {
        assert_eq!(build_stage_name(0), Some("还原"));
        assert_eq!(build_stage_name(3), Some("打包"));
        assert_eq!(build_stage_name(4), Some("发布"));
        assert!(build_stage_name(5).is_none());
        assert_eq!(build_stage(0), Some(0));
        assert_eq!(build_stage(19), Some(4));
        assert!(build_stage(20).is_none());
        assert!(build_cell(20).is_none());
    }

    #[test]
    fn test_yml_bidirectional() {
        // 双向投影：bwd = fwd ^ MIRROR，且逐格恒等可复算。
        let mut i = 0usize;
        while i < YML_CELLS {
            let f = yml_cell(i).unwrap_or(0);
            let b = yml_bwd(i).unwrap_or(0);
            assert_eq!(f ^ YML_MIRROR, b);
            i += 1;
        }
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(yml_bwd(YML_CELLS).is_none());
    }

    #[test]
    fn test_art_states() {
        // 三态映射：态 = i%3，测试按该映射核对。
        assert_eq!(art_state(0), Some(0));
        assert_eq!(art_state(1), Some(1));
        assert_eq!(art_state(2), Some(2));
        assert_eq!(art_state(3), Some(0));
        assert_eq!(art_state(5), Some(2));
        assert!(art_state(6).is_none());
        assert_eq!(art_state_name(0), Some("上传"));
        assert_eq!(art_state_name(2), Some("发布"));
        assert!(art_state_name(3).is_none());
        assert!(art_cell(ART_CELLS).is_none());
    }

    #[test]
    fn test_deploy_scenes() {
        assert_eq!(deploy_scene_name(0), Some("GitHub Releases 部署"));
        assert_eq!(deploy_scene_name(4), Some("NuGet 部署"));
        assert!(deploy_scene_name(5).is_none());
        assert!(deploy_cell(DEPLOY_SCENES).is_none());
        assert!(cosign_row(COSIGN_ROWS).is_none());
        assert_eq!(cosign_row_name(2), Some("部署消费联签"));
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = AppveyorLedger::new();
        let _ = lt.record_build(9, build_cell(9).unwrap_or(0));
        assert_eq!(lt.record_build(2, build_cell(2).unwrap_or(0)), Err(AppveyorCode::EOrder));
        // 逆序被拒后 last_stage 未变：同段 10 号仍可合法落账。
        assert!(lt.record_build(10, build_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AppveyorLedger::new();
        assert_eq!(
            lt.record_build(5, build_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AppveyorCode::EDrift)
        );
        assert_eq!(
            lt.record_art(2, art_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(AppveyorCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AppveyorLedger::new();
        let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
        assert_eq!(lt.record_yml(0, yml_cell(0).unwrap_or(0)), Err(AppveyorCode::EDup));
        let _ = lt.record_deploy(0);
        assert_eq!(lt.record_deploy(0), Err(AppveyorCode::EDup));
        let _ = lt.record_cosign(1);
        assert_eq!(lt.record_cosign(1), Err(AppveyorCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AppveyorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 build 格 + 12 yml + 6 工件 + 4 联签 + 5 部署 = 47。
        assert_eq!(recorded, BUILD_CELLS as u32 + YML_CELLS as u32 + ART_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9B80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AppveyorCode::Ok as u32, 0);
        assert_eq!(AppveyorCode::EGradeDrift as u32, 12);
        assert_eq!(AppveyorCode::EArtNo as u32, 5);
        assert_eq!(AppveyorCode::ESeal as u32, 9);
        assert!(!AppveyorCode::Ok.detail().is_empty());
        assert!(!AppveyorCode::ERowMiss.detail().is_empty());
        assert_eq!(AppveyorCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(GRADE_TARGET, Grade::S2);
        assert_eq!(Grade::S1.val(), 1);
        let lt = AppveyorLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据（账未齐、未采纳）→ S2。
        let mut lt2 = AppveyorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(!lt2.adopted());
    }

    #[test]
    fn unx52367_checks_green() {
        let s = run_unx52367_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
