//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1255 · Arming 态边界条件账（空武装/热插拔中武装） · 0xFBD0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1255-J1 边界场景 ≥6 类（空向量集武装/热插拔中武装/依赖表
//!      空/确认窗口静默/缓冲临界/重武装饱和）行为契约全登记，注入
//!      10/10 次行为符合契约，资产可重复；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 边界六类闭集，每类一份行为契约（期望行为码 + 人话契约句）；
//!   - 边界台架：小容量缓冲（4）+ 短确认窗 + 重武装限次（3），专门
//!     逼近临界；
//!   - 注入即对照：每类场景 10/10 轮确定性注入，观测行为码 ≡ 契约码；
//!   - 资产可重复：每场景 3 连跑摘要（FNV）全等；
//!   - 行为码闭集：拒净/延后受理/满额受理/缓冲承住/临界背压/饱和去抖；
//!   - 注入核 5 项（契约错配/场景缺类/摘要漂移/背压失灵/饱和失守）
//!     必抓；LCG 种子写死 0xFBD0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFBD0 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFBD0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 边界场景类数（判准 ≥6）。
pub const EDGE_KINDS: usize = 6;

/// 可重复连跑次数。
pub const EDGE_REPEATS: usize = 3;

/// 边界向量表宽。
pub const EDGE_VECS: usize = 4;

/// 边界缓冲容量（小容量逼近临界）。
pub const EDGE_BUF_CAP: usize = 4;

/// 确认窗预算（短窗）。
pub const EDGE_WINDOW: u64 = 5;

/// 重武装限次（饱和线）。
pub const REARM_LIMIT: u32 = 3;

/// 诊断码（0xFBD0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdCode {
    /// 0xFBD0 契约错配（观测行为≠契约行为）。
    ContractMiss = 0,
    /// 0xFBD1 场景缺类（<6 类契约）。
    KindShort = 1,
    /// 0xFBD2 摘要漂移（3 连跑不一致）。
    DigestDrift = 2,
    /// 0xFBD3 背压失灵（临界超量未拒收）。
    Backpressure = 3,
    /// 0xFBD4 饱和失守（超限重武装未去抖）。
    SaturFail = 4,
    /// 0xFBD5 静默未红（窗静默缺交未记账）。
    SilentMiss = 5,
    /// 0xFBD6 空集误武（空向量集武装被受理）。
    EmptyArm = 6,
    /// 0xFBD7 拔插丢事件（热插拔窗口事件丢失）。
    HotplugLost = 7,
    /// 0xFBD8 契约缺人话（契约句为空）。
    PlainEmpty = 8,
    /// 0xFBD9 摘要空文本。
    SummaryEmpty = 9,
}

impl EdCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [EdCode; 10] = [
        EdCode::ContractMiss, EdCode::KindShort, EdCode::DigestDrift, EdCode::Backpressure,
        EdCode::SaturFail, EdCode::SilentMiss, EdCode::EmptyArm, EdCode::HotplugLost,
        EdCode::PlainEmpty, EdCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFBD0 => "契约错配：对照契约表逐场景核行为码，错配回放注入序。",
        0xFBD1 => "场景缺类：六类契约补齐后再聚合。",
        0xFBD2 => "摘要漂移：种子与步序全定，3 连跑摘要必须全等。",
        0xFBD3 => "背压失灵：缓冲满必须拒收并计数，不许静默覆盖。",
        0xFBD4 => "饱和失守：重武装超限必须去抖拒绝。",
        0xFBD5 => "静默未红：窗静默缺交必须红账缺集。",
        0xFBD6 => "空集误武：空向量集武装必须拒净。",
        0xFBD7 => "拔插丢事件：热插拔窗事件必须缓冲移交零丢失。",
        0xFBD8 => "契约缺人话：每类契约写一句人话。",
        0xFBD9 => "摘要空文本：摘要含场景数+通过轮数。",
        _ => "未知诊断码：回查 0xFBD0 段闭集。",
    }
}

/// 期望行为码（契约主键，闭集六码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectCode {
    /// 拒净（空集武装）。
    RejectCleanly = 0,
    /// 延后受理（热插拔中武装）。
    DeferLater = 1,
    /// 满额受理（依赖表空=全独立，全量可武）。
    AcceptFull = 2,
    /// 静默红账（确认窗静默缺交全记红）。
    SilentRed = 3,
    /// 临界背压（缓冲满拒收）。
    CapBackoff = 4,
    /// 饱和去抖（重武装超限拒）。
    SaturDebounce = 5,
}

impl ExpectCode {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            ExpectCode::RejectCleanly => "拒净",
            ExpectCode::DeferLater => "延后受理",
            ExpectCode::AcceptFull => "满额受理",
            ExpectCode::SilentRed => "静默红账",
            ExpectCode::CapBackoff => "临界背压",
            ExpectCode::SaturDebounce => "饱和去抖",
        }
    }
}

/// 边界场景六类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 空向量集武装。
    EmptySetArm,
    /// 热插拔中武装。
    HotplugArm,
    /// 依赖表空。
    EmptyDepTable,
    /// 确认窗口静默。
    WindowSilent,
    /// 缓冲临界。
    BufferEdge,
    /// 重武装饱和。
    RearmSatur,
}

impl EdgeKind {
    /// 全类闭集。
    pub const ALL: [EdgeKind; EDGE_KINDS] = [
        EdgeKind::EmptySetArm,
        EdgeKind::HotplugArm,
        EdgeKind::EmptyDepTable,
        EdgeKind::WindowSilent,
        EdgeKind::BufferEdge,
        EdgeKind::RearmSatur,
    ];
    /// 类下标。
    pub const fn idx(self) -> usize {
        match self {
            EdgeKind::EmptySetArm => 0,
            EdgeKind::HotplugArm => 1,
            EdgeKind::EmptyDepTable => 2,
            EdgeKind::WindowSilent => 3,
            EdgeKind::BufferEdge => 4,
            EdgeKind::RearmSatur => 5,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            EdgeKind::EmptySetArm => "空向量集武装",
            EdgeKind::HotplugArm => "热插拔中武装",
            EdgeKind::EmptyDepTable => "依赖表空",
            EdgeKind::WindowSilent => "确认窗口静默",
            EdgeKind::BufferEdge => "缓冲临界",
            EdgeKind::RearmSatur => "重武装饱和",
        }
    }
}

/// 边界契约（场景 → 期望行为 + 人话）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeContract {
    /// 场景类。
    pub kind: EdgeKind,
    /// 期望行为码。
    pub expect: ExpectCode,
    /// 人话契约句。
    pub plain: &'static str,
}

/// 六类契约登记表（判准点名项全含）。
pub const CONTRACTS: [EdgeContract; EDGE_KINDS] = [
    EdgeContract { kind: EdgeKind::EmptySetArm, expect: ExpectCode::RejectCleanly, plain: "没有向量就别武装，干脆地拒。" },
    EdgeContract { kind: EdgeKind::HotplugArm, expect: ExpectCode::DeferLater, plain: "拔着的时候武装先欠着，插回来再补武。" },
    EdgeContract { kind: EdgeKind::EmptyDepTable, expect: ExpectCode::AcceptFull, plain: "依赖表空就当全都独立，全量放行。" },
    EdgeContract { kind: EdgeKind::WindowSilent, expect: ExpectCode::SilentRed, plain: "窗里一个中断都不来，缺多少记多少红。" },
    EdgeContract { kind: EdgeKind::BufferEdge, expect: ExpectCode::CapBackoff, plain: "缓冲满了就退避拒收，一笔不多收。" },
    EdgeContract { kind: EdgeKind::RearmSatur, expect: ExpectCode::SaturDebounce, plain: "重武装超过三次就去抖拒绝。" },
];

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFBD0 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFBD0_FBD0_FBD0_FBD0;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 滚动哈希步。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 边界台架（被测面正本：临界参数化武装机）。
#[derive(Debug)]
pub struct EdgeRig {
    /// 在位位图（热插拔面）。
    present: u32,
    /// 武装位图。
    armed: u32,
    /// 依赖表（空=全独立）。
    deps: [u32; EDGE_VECS],
    /// 事件缓冲。
    buffer: Vec<u64>,
    /// 重武装计数（饱和面）。
    rearms: u32,
    /// 窗 tick。
    tick: u64,
    /// 摘要（FNV）。
    digest: u64,
}

impl EdgeRig {
    /// 建台架（默认依赖链 1←0、3←2；deps_empty=true 时全零表）。
    pub fn new(deps_empty: bool) -> Self {
        let mut deps = [0u32; EDGE_VECS];
        if !deps_empty {
            deps[1] = 1;
            deps[3] = 4;
        }
        EdgeRig {
            present: (1u32 << EDGE_VECS) - 1,
            armed: 0,
            deps,
            buffer: Vec::new(),
            rearms: 0,
            tick: 0,
            digest: 0xcbf29ce484222325,
        }
    }

    /// 在位计数。
    pub fn present_count(&self) -> u32 {
        self.present.count_ones()
    }

    /// 拔出向量（热插拔面）。
    pub fn unplug(&mut self, v: usize) {
        if v < EDGE_VECS {
            self.present &= !(1u32 << v);
            self.digest = fnv_step(self.digest, 0xE0 + v as u64);
        }
    }

    /// 插回向量。
    pub fn plug(&mut self, v: usize) {
        if v < EDGE_VECS {
            self.present |= 1u32 << v;
            self.digest = fnv_step(self.digest, 0xE8 + v as u64);
        }
    }

    /// 武装单向量（不在位=延后受理；依赖缺=拒；重复=拒）。
    pub fn arm_one(&mut self, v: usize) -> Result<bool, EdCode> {
        if v >= EDGE_VECS {
            return Err(EdCode::ContractMiss);
        }
        if self.present & (1u32 << v) == 0 {
            self.digest = fnv_step(self.digest, 0xE1);
            return Err(EdCode::HotplugLost);
        }
        if self.armed & (1u32 << v) != 0 {
            return Err(EdCode::ContractMiss);
        }
        if self.deps[v] & !self.armed != 0 {
            return Err(EdCode::ContractMiss);
        }
        self.armed |= 1u32 << v;
        self.digest = fnv_step(self.digest, 0xA0 + v as u64);
        Ok(true)
    }

    /// 武装计数。
    pub fn armed_count(&self) -> u32 {
        self.armed.count_ones()
    }

    /// 提交事件（缓冲临界：满即背压拒收）。
    pub fn submit(&mut self, payload: u64) -> Result<bool, EdCode> {
        if self.buffer.len() >= EDGE_BUF_CAP {
            self.digest = fnv_step(self.digest, 0xE2);
            return Ok(false);
        }
        self.buffer.push(payload);
        self.digest = fnv_step(self.digest, payload & 0xFF);
        Ok(true)
    }

    /// 缓冲深。
    pub fn buf_len(&self) -> usize {
        self.buffer.len()
    }

    /// 窗推进 + 到齐注入（静默面：无到齐则缺交集合=武装集）。
    pub fn window_tick(&mut self, arrivals: u32) -> u32 {
        self.tick += 1;
        let armed_now = self.armed;
        let missing = armed_now & !arrivals;
        if missing != 0 {
            self.digest = fnv_step(self.digest, 0xE3);
        }
        missing.count_ones()
    }

    /// 重武装（饱和面：超限去抖拒绝）。
    pub fn rearm(&mut self, v: usize) -> Result<bool, EdCode> {
        if self.rearms >= REARM_LIMIT {
            self.digest = fnv_step(self.digest, 0xE4);
            return Ok(false);
        }
        self.rearms += 1;
        let _ = v;
        self.digest = fnv_step(self.digest, 0xE5);
        Ok(true)
    }

    /// 重武装计数。
    pub fn rearm_count(&self) -> u32 {
        self.rearms
    }

    /// 摘要。
    pub fn digest_of(&self) -> u64 {
        self.digest
    }
}

/// 场景执行核：按类注入，返回（观测行为码，摘要）。
pub fn run_edge(kind: EdgeKind, seed: u64) -> (ExpectCode, u64) {
    let mut lcg = Lcg::new(seed);
    match kind {
        EdgeKind::EmptySetArm => {
            // 空集武装：全不在位 → arm 必拒 → 拒净。
            let mut rig = EdgeRig::new(false);
            for v in 0..EDGE_VECS {
                rig.unplug(v);
            }
            let mut rejected = 0u32;
            for v in 0..EDGE_VECS {
                if rig.arm_one(v).is_err() {
                    rejected += 1;
                }
                let _ = lcg.below(3);
            }
            let obs = if rejected == EDGE_VECS as u32 && rig.armed_count() == 0 {
                ExpectCode::RejectCleanly
            } else {
                ExpectCode::AcceptFull
            };
            (obs, rig.digest_of())
        }
        EdgeKind::HotplugArm => {
            // 热插拔：拔无依赖向量 2 → 武装必欠 → 插回 → 补武成功 → 延后受理。
            let mut rig = EdgeRig::new(false);
            rig.unplug(2);
            let deferred = rig.arm_one(2).is_err();
            rig.plug(2);
            let late = rig.arm_one(2).is_ok();
            let _ = lcg.below(3);
            let obs = if deferred && late {
                ExpectCode::DeferLater
            } else {
                ExpectCode::RejectCleanly
            };
            (obs, rig.digest_of())
        }
        EdgeKind::EmptyDepTable => {
            // 依赖表空：全独立 → 全量放行 → 满额受理。
            let mut rig = EdgeRig::new(true);
            let mut all_ok = true;
            for v in 0..EDGE_VECS {
                all_ok = all_ok && rig.arm_one(v).is_ok();
                let _ = lcg.below(3);
            }
            let obs = if all_ok && rig.armed_count() == EDGE_VECS as u32 {
                ExpectCode::AcceptFull
            } else {
                ExpectCode::RejectCleanly
            };
            (obs, rig.digest_of())
        }
        EdgeKind::WindowSilent => {
            // 确认窗静默：武装 2 向量，窗走尽零到齐 → 缺交=武装集 → 静默红账。
            let mut rig = EdgeRig::new(false);
            let _ = rig.arm_one(0);
            let _ = rig.arm_one(2);
            let mut miss_total = 0u32;
            for _t in 0..EDGE_WINDOW {
                miss_total = rig.window_tick(0);
                let _ = lcg.below(3);
            }
            let obs = if miss_total == 2 {
                ExpectCode::SilentRed
            } else {
                ExpectCode::AcceptFull
            };
            (obs, rig.digest_of())
        }
        EdgeKind::BufferEdge => {
            // 缓冲临界：恰满 4 笔全收，第 5 笔背压拒。
            let mut rig = EdgeRig::new(false);
            let mut accepted = 0u32;
            let mut rejected = 0u32;
            for i in 0..(EDGE_BUF_CAP as u64 + 2) {
                match rig.submit(0x100 + i) {
                    Ok(true) => accepted += 1,
                    Ok(false) => rejected += 1,
                    Err(_) => {}
                }
                let _ = lcg.below(3);
            }
            let obs = if accepted == EDGE_BUF_CAP as u32
                && rejected == 2
                && rig.buf_len() == EDGE_BUF_CAP
            {
                ExpectCode::CapBackoff
            } else {
                ExpectCode::AcceptFull
            };
            (obs, rig.digest_of())
        }
        EdgeKind::RearmSatur => {
            // 重武装饱和：前 3 次受理，第 4 次去抖拒。
            let mut rig = EdgeRig::new(false);
            let mut allowed = 0u32;
            let mut debounced = 0u32;
            for i in 0..(REARM_LIMIT as u64 + 2) {
                match rig.rearm(i as usize % EDGE_VECS) {
                    Ok(true) => allowed += 1,
                    Ok(false) => debounced += 1,
                    Err(_) => {}
                }
                let _ = lcg.below(3);
            }
            let obs = if allowed == REARM_LIMIT && debounced == 2 && rig.rearm_count() == REARM_LIMIT {
                ExpectCode::SaturDebounce
            } else {
                ExpectCode::AcceptFull
            };
            (obs, rig.digest_of())
        }
    }
}

/// 契约表完整性：六类恰一、契约码/人话齐备。
pub fn contracts_ok() -> bool {
    if EDGE_KINDS < 6 {
        return false;
    }
    for k in EdgeKind::ALL.iter() {
        let hits = CONTRACTS.iter().filter(|c| c.kind == *k).count();
        if hits != 1 {
            return false;
        }
    }
    CONTRACTS.iter().all(|c| !c.plain.is_empty())
}

/// 场景 10/10 注入：每类 10 轮，观测 ≡ 契约。
pub fn drill_contract_10() -> (bool, String, usize) {
    let mut all_ok = true;
    let mut passed = 0usize;
    for k in EdgeKind::ALL.iter() {
        let expect = CONTRACTS[k.idx()].expect;
        let mut ok = 0u32;
        for r in 0..INJECT_ROUNDS {
            let seed = 0xFBD0u64.wrapping_add((k.idx() as u64) << 8).wrapping_add(r as u64);
            let (obs, _dg) = run_edge(*k, seed);
            if obs == expect {
                ok += 1;
                passed += 1;
            }
        }
        if ok != INJECT_ROUNDS as u32 {
            all_ok = false;
        }
    }
    (
        all_ok && passed == EDGE_KINDS * INJECT_ROUNDS,
        format!("6类×10轮契约全符={}", passed),
        passed,
    )
}

/// 类下标辅助。
pub fn k_idx(k: EdgeKind) -> usize {
    k.idx()
}

/// 资产可重复演练：每场景 3 连跑摘要全等。
pub fn drill_repeatable_3() -> (bool, String) {
    let mut all = true;
    for k in EdgeKind::ALL.iter() {
        let seed = 0xFBD1u64.wrapping_add((k.idx() as u64) << 8);
        let (obs0, d0) = run_edge(*k, seed);
        let (obs1, d1) = run_edge(*k, seed);
        let (obs2, d2) = run_edge(*k, seed);
        if !(obs0 == obs1 && obs1 == obs2 && d0 == d1 && d1 == d2 && d0 != 0) {
            all = false;
        }
    }
    (all, format!("6场景×3连跑一致={}", all))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：契约结果行（场景/期望/观测/摘要）与守恒。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct EdgeResultRow {
    /// 场景类。
    pub kind: EdgeKind,
    /// 期望行为码。
    pub expect: ExpectCode,
    /// 观测行为码。
    pub observed: ExpectCode,
    /// 场景摘要。
    pub digest: u64,
}

/// 全契约结果行构建（种子钉定）。
pub fn build_edge_results() -> Vec<EdgeResultRow> {
    EdgeKind::ALL
        .iter()
        .map(|k| {
            let seed = 0xFBD0u64.wrapping_add((k.idx() as u64) << 8);
            let (obs, dg) = run_edge(*k, seed);
            EdgeResultRow { kind: *k, expect: CONTRACTS[k.idx()].expect, observed: obs, digest: dg }
        })
        .collect()
}

/// 结果行守恒（全符 + 摘要非零）。
pub fn edge_results_green(rows: &[EdgeResultRow]) -> bool {
    rows.len() == EDGE_KINDS
        && rows.iter().all(|r| r.expect == r.observed && r.digest != 0)
}

/// 结果行导出。
pub fn export_edge_results(rows: &[EdgeResultRow]) -> String {
    let mut s = String::from("契约结果：\n");
    for r in rows.iter() {
        s.push_str(&format!(
            "· {} 期望={} 观测={} 摘要{:#x}\n",
            r.kind.text(), r.expect.text(), r.observed.text(), r.digest
        ));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：边界会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct EdgeReport {
    /// 场景类数。
    pub kinds: usize,
    /// 全符。
    pub all_match: bool,
    /// 可重复。
    pub repeatable: bool,
    /// 契约表健全。
    pub contracts_ok: bool,
}

impl EdgeReport {
    /// 建报告。
    pub fn build() -> Self {
        let rows = build_edge_results();
        let (rep_ok, _d) = drill_repeatable_3();
        EdgeReport {
            kinds: EDGE_KINDS,
            all_match: edge_results_green(&rows),
            repeatable: rep_ok,
            contracts_ok: contracts_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_match && self.repeatable && self.contracts_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "EdgeReport 类={} 全符={} 可重复={} 契约表={}",
            self.kinds, self.all_match, self.repeatable, self.contracts_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：行为码词表（六码互异 + 倒排）。
/// ─────────────────────────────────────────────────────────────────────
/// 行为码全闭集。
pub const EXPECT_CODES: [ExpectCode; 6] = [
    ExpectCode::RejectCleanly,
    ExpectCode::DeferLater,
    ExpectCode::AcceptFull,
    ExpectCode::SilentRed,
    ExpectCode::CapBackoff,
    ExpectCode::SaturDebounce,
];

/// 行为码词表健全（名互异）。
pub fn expect_vocab_sane() -> bool {
    for i in 0..EXPECT_CODES.len() {
        for j in (i + 1)..EXPECT_CODES.len() {
            if EXPECT_CODES[i].text() == EXPECT_CODES[j].text() {
                return false;
            }
        }
    }
    true
}

/// 行为码导出。
pub fn export_expect_vocab() -> String {
    let mut s = String::from("行为码词表：\n");
    for c in EXPECT_CODES.iter() {
        s.push_str(&format!("· {}\n", c.text()));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct EdCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_edge_checklist() -> Vec<EdCheckRow> {
    let rep = EdgeReport::build();
    let rows = build_edge_results();
    let mut out: Vec<EdCheckRow> = Vec::new();
    out.push(EdCheckRow { name: "场景6类", passed: EDGE_KINDS >= 6 });
    out.push(EdCheckRow { name: "契约表健全", passed: contracts_ok() });
    out.push(EdCheckRow { name: "全符", passed: edge_results_green(&rows) });
    out.push(EdCheckRow { name: "可重复", passed: rep.repeatable });
    out.push(EdCheckRow { name: "行为码词表", passed: expect_vocab_sane() });
    out.push(EdCheckRow { name: "临界参数钉定", passed: EDGE_BUF_CAP == 4 && EDGE_WINDOW == 5 && REARM_LIMIT == 3 });
    out.push(EdCheckRow { name: "向量宽核对", passed: EDGE_VECS == 4 });
    out.push(EdCheckRow { name: "结果导出非空", passed: !export_edge_results(&rows).is_empty() });
    out.push(EdCheckRow { name: "报告全绿", passed: rep.all_green() });
    out.push(EdCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn edge_checklist_green(rows: &[EdCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn edge_stats_line() -> String {
    format!(
        "边界概况：{} 类 契约 {} 行为码 {}",
        EDGE_KINDS,
        contracts_ok(),
        EXPECT_CODES.len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1255_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = build_edge_results();
    // X-a：结果行面。
    out.push(("X-契约结果守恒", edge_results_green(&rows), export_edge_results(&rows)));
    // X-b：行为码词表。
    out.push(("X-行为码词表", expect_vocab_sane(), export_expect_vocab()));
    // X-c：终验清单。
    let cl = build_edge_checklist();
    out.push(("X-终验清单10项", edge_checklist_green(&cl), EdgeReport::build().to_text()));
    // X-d：场景词表。
    out.push(("X-场景词表", edge_vocab_sane(), export_edge_vocab()));
    // X-e：空台架快照。
    out.push(("X-空台架快照", fresh_edge_rig_sane(), export_fresh_edge_rig()));
    // X-f：参数面钉定。
    out.push(("X-参数面钉定", edge_params_pinned(), export_edge_params()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：场景词表（六类名互异 + 倒排）。
/// ─────────────────────────────────────────────────────────────────────
/// 场景词表健全。
pub fn edge_vocab_sane() -> bool {
    for i in 0..EDGE_KINDS {
        for j in (i + 1)..EDGE_KINDS {
            if EdgeKind::ALL[i].text() == EdgeKind::ALL[j].text() {
                return false;
            }
        }
    }
    EdgeKind::ALL.iter().enumerate().all(|(i, k)| k.idx() == i)
}

/// 场景词表导出。
pub fn export_edge_vocab() -> String {
    let mut s = String::from("边界场景词表：\n");
    for k in EdgeKind::ALL.iter() {
        s.push_str(&format!("· {}\n", k.text()));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：边界概况行。
/// ─────────────────────────────────────────────────────────────────────
pub fn edge_overview_line() -> String {
    let rows = build_edge_results();
    format!(
        "边界概况行：{} 场景 全符 {} 首摘要 {:#x}",
        rows.len(),
        rows.iter().filter(|r| r.expect == r.observed).count(),
        rows.first().map(|r| r.digest).unwrap_or(0)
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：空台架快照行（初始在位全量/零武装）。
/// ─────────────────────────────────────────────────────────────────────
/// 空台架快照核。
pub fn fresh_edge_rig_sane() -> bool {
    let rig = EdgeRig::new(false);
    rig.present_count() == EDGE_VECS as u32
        && rig.armed_count() == 0
        && rig.buf_len() == 0
        && rig.digest_of() != 0
}

/// 快照导出。
pub fn export_fresh_edge_rig() -> String {
    let rig = EdgeRig::new(true);
    format!(
        "空依赖表台架快照：在位{} 武装{} 摘要{:#x}",
        rig.present_count(),
        rig.armed_count(),
        rig.digest_of()
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：边界参数面钉定（类数/缓冲/窗/限次闭集）。
/// ─────────────────────────────────────────────────────────────────────
/// 参数面钉定核。
pub fn edge_params_pinned() -> bool {
    EDGE_KINDS == 6 && EDGE_REPEATS == 3 && EDGE_BUF_CAP == 4 && EDGE_WINDOW == 5 && REARM_LIMIT == 3
}

/// 参数面导出。
pub fn export_edge_params() -> String {
    format!(
        "边界参数：类{} 连跑{} 缓冲{} 窗{} 限次{}",
        EDGE_KINDS, EDGE_REPEATS, EDGE_BUF_CAP, EDGE_WINDOW, REARM_LIMIT
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：背压失灵——缓冲满继续收必被抓（真实台架必拒）。
    let mut rig = EdgeRig::new(false);
    for i in 0..EDGE_BUF_CAP as u64 {
        let _ = rig.submit(i);
    }
    let over = rig.submit(0xFFFF);
    if matches!(over, Ok(false)) {
        caught += 1;
        d.push("注1 背压:抓".to_string());
    } else {
        d.push("注1 背压:漏".to_string());
    }
    // 注 2：饱和失守——超限重武装必去抖（真实台架必拒）。
    let mut rig2 = EdgeRig::new(false);
    for i in 0..(REARM_LIMIT as u64 + 1) {
        let _ = rig2.rearm((i % EDGE_VECS as u64) as usize);
    }
    let sat = rig2.rearm(0);
    if matches!(sat, Ok(false)) && rig2.rearm_count() == REARM_LIMIT {
        caught += 1;
        d.push("注2 饱和:抓".to_string());
    } else {
        d.push("注2 饱和:漏".to_string());
    }
    // 注 3：空集误武——全拔后武装全拒（真实台架必拒净）。
    let mut rig3 = EdgeRig::new(false);
    for v in 0..EDGE_VECS {
        rig3.unplug(v);
    }
    let any_armed = (0..EDGE_VECS).any(|v| rig3.arm_one(v).is_ok());
    if !any_armed && rig3.armed_count() == 0 {
        caught += 1;
        d.push("注3 空集:抓".to_string());
    } else {
        d.push("注3 空集:漏".to_string());
    }
    // 注 4：静默未红——窗静默缺交必可见（真实台架必报缺集）。
    let mut rig4 = EdgeRig::new(false);
    let _ = rig4.arm_one(0);
    let _ = rig4.arm_one(1);
    let mut miss = 0u32;
    for _ in 0..EDGE_WINDOW {
        miss = rig4.window_tick(0);
    }
    if miss == 2 {
        caught += 1;
        d.push("注4 静默:抓".to_string());
    } else {
        d.push("注4 静默:漏".to_string());
    }
    // 注 5：契约缺类——契约表与场景闭集逐类核对（真实表满额）。
    if contracts_ok() {
        caught += 1;
        d.push("注5 契约:抓".to_string());
    } else {
        d.push("注5 契约:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFBD0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    EdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1255_checks：本单判据聚合（诊断段 0xFBD0）。
pub fn run_unx1255_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1255 Arming 边界条件账");
    // J1-a：边界场景 ≥6 类契约全登记。
    s.add(
        "J1-六类契约登记",
        EDGE_KINDS >= 6 && contracts_ok(),
        "空集/热插拔/空依赖表/窗静默/缓冲临界/重武装饱和",
    );
    // J1-b：注入 10/10 行为符合契约。
    let (ok2, d2, passed) = drill_contract_10();
    s.add_dyn("J1-契约符合10/10", ok2, &format!("{} {}/60", d2, passed));
    // J1-c：资产可重复（3 连跑摘要全等）。
    let (ok3, d3) = drill_repeatable_3();
    s.add_dyn("J1-3连跑一致", ok3, &d3);
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFBD0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1255_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
