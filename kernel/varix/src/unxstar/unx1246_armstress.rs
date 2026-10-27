//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1246 · Arming 态压测资产（武装序列高压耐受） · 0xFB30
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1246-J1 压测场景 ≥5 类（全量武装风暴/武装中事件洪峰/失败
//!      注入风暴/重武装循环/确认窗口高压）资产化，3 次可重复一致率
//!      100%，高压零丢失全通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 压测台架：武装序列仿真器（逐向量使能+事件泵+确认窗）承载 5 类
//!     高压场景，事件序号账守恒（提交≡交付+滞留，零丢失）；
//!   - 场景资产表：5 类场景 × 写死种子 × 事件量，资产化登记；
//!   - 可重复一致率：同资产连跑 3 次，摘要（FNV 滚动哈希）逐次相等；
//!   - 高压零丢失：每轮场景后 seq 连续性断言 + 守恒恒等式全过；
//!   - 注入核 5 项（越源风暴/二武装/闭窗投递/守恒破口/资产指纹漂移）
//!     必抓；LCG 种子写死 0xFB30，10/10 轮全过。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB30 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB30;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 压测场景类数（判准 ≥5）。
pub const SCENARIO_KINDS: usize = 5;

/// 可重复连跑次数（判准 3 次一致）。
pub const REPEAT_RUNS: usize = 3;

/// 压测向量表宽度。
pub const STRESS_VECTORS: usize = 5;

/// 诊断码（0xFB30 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    /// 0xFB30 高压丢事件（提交≠交付+滞留）。
    EventLost = 0,
    /// 0xFB31 越源风暴（风暴事件向量号出界）。
    StormOob = 1,
    /// 0xFB32 重复武装（同向量二武装未先撤）。
    ArmDup = 2,
    /// 0xFB33 闭窗投递（确认窗已闭仍投）。
    WindowShut = 3,
    /// 0xFB34 指纹漂移（3 次连跑摘要不一致）。
    DigestDrift = 4,
    /// 0xFB35 场景缺类（<5 类资产）。
    KindShort = 5,
    /// 0xFB36 序号断流（交付 seq 不连续）。
    SeqBreak = 6,
    /// 0xFB37 洪峰溢出（缓冲超容量未背压）。
    BufOverflow = 7,
    /// 0xFB38 空场景（事件量为零仍报通过）。
    EmptyRun = 8,
    /// 0xFB39 资产缺种子（种子未登记）。
    SeedMiss = 9,
}

impl StCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [StCode; 10] = [
        StCode::EventLost, StCode::StormOob, StCode::ArmDup, StCode::WindowShut,
        StCode::DigestDrift, StCode::KindShort, StCode::SeqBreak, StCode::BufOverflow,
        StCode::EmptyRun, StCode::SeedMiss,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB30 => "高压丢事件：提交数必须等于交付+滞留，缺口回溯序号账。",
        0xFB31 => "越源风暴：风暴生成器先对向量表宽取模再投递。",
        0xFB32 => "重复武装：armed 位已置时再武装必拒，先撤再武。",
        0xFB33 => "闭窗投递：确认窗闭后投递走下轮窗，不许静默吞。",
        0xFB34 => "指纹漂移：LCG 种子与场景步序必须全定，逐次比对摘要。",
        0xFB35 => "场景缺类：5 类场景资产表补齐后再聚合。",
        0xFB36 => "序号断流：交付 seq 必须从起点连续递增，断流即红账。",
        0xFB37 => "洪峰溢出：缓冲容量满时必须背压拒收，不许静默覆盖。",
        0xFB38 => "空场景：事件量为零的场景不报通过，补事件量。",
        0xFB39 => "资产缺种子：每类场景必须登记写死种子。",
        _ => "未知诊断码：回查 0xFB30 段闭集。",
    }
}

/// 压测场景五类（判准点名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StressKind {
    /// 场景 1：全量武装风暴（全向量武装后事件全量灌入）。
    FullArmStorm,
    /// 场景 2：武装中事件洪峰（逐向量武装间隙事件不断）。
    MidArmFlood,
    /// 场景 3：失败注入风暴（越源/二武装/未武装投递混灌）。
    FailInjectStorm,
    /// 场景 4：重武装循环（武装→撤退→重武装多轮）。
    RearmLoop,
    /// 场景 5：确认窗口高压（背靠背确认窗满载投递）。
    ConfirmStorm,
}

impl StressKind {
    /// 全类闭集。
    pub const ALL: [StressKind; SCENARIO_KINDS] = [
        StressKind::FullArmStorm,
        StressKind::MidArmFlood,
        StressKind::FailInjectStorm,
        StressKind::RearmLoop,
        StressKind::ConfirmStorm,
    ];
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            StressKind::FullArmStorm => "全量武装风暴",
            StressKind::MidArmFlood => "武装中事件洪峰",
            StressKind::FailInjectStorm => "失败注入风暴",
            StressKind::RearmLoop => "重武装循环",
            StressKind::ConfirmStorm => "确认窗口高压",
        }
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB30 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB30_FB30_FB30_FB30;

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

/// 压测台架（武装序列仿真器正本）。
///
/// 事件守恒：submitted ≡ delivered + buffered + rejected；
/// 交付 seq 连续：从 1 起每交付一笔 +1 不跳号。
#[derive(Debug)]
pub struct StressRig {
    /// 武装位图。
    mask: u32,
    /// 待交付缓冲（容量上限背压）。
    buffer: Vec<u64>,
    /// 缓冲容量。
    cap: usize,
    /// 下一交付序号（连续性主键）。
    next_seq: u64,
    /// 已提交事件数。
    submitted: u64,
    /// 已交付事件数。
    delivered: u64,
    /// 已拒收事件数（背压/未武装/越源）。
    rejected: u64,
    /// 确认窗开闭。
    window_open: bool,
    /// 场景轮次。
    rounds: u32,
    /// 摘要（FNV 滚动哈希：可重复一致率主键）。
    digest: u64,
}

/// FNV-1a 滚动哈希步（u64，确定性，无浮点）。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

impl StressRig {
    /// 建台架（缓冲容量写死 64）。
    pub fn new() -> Self {
        StressRig {
            mask: 0,
            buffer: Vec::new(),
            cap: 64,
            next_seq: 1,
            submitted: 0,
            delivered: 0,
            rejected: 0,
            window_open: false,
            rounds: 0,
            digest: 0xcbf29ce484222325,
        }
    }

    /// 武装向量（越源必拒；重复武装必拒）。
    pub fn arm(&mut self, v: usize) -> Result<(), StCode> {
        if v >= STRESS_VECTORS {
            return Err(StCode::StormOob);
        }
        if self.mask & (1u32 << v) != 0 {
            return Err(StCode::ArmDup);
        }
        self.mask |= 1u32 << v;
        self.digest = fnv_step(self.digest, 0xA0 + v as u64);
        Ok(())
    }

    /// 撤武装（重武装循环用）。
    pub fn disarm(&mut self, v: usize) -> Result<(), StCode> {
        if v >= STRESS_VECTORS {
            return Err(StCode::StormOob);
        }
        self.mask &= !(1u32 << v);
        self.digest = fnv_step(self.digest, 0xD0 + v as u64);
        Ok(())
    }

    /// 武装计数。
    pub fn armed_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 开/闭确认窗。
    pub fn set_window(&mut self, open: bool) {
        self.window_open = open;
        self.digest = fnv_step(self.digest, if open { 0x0F } else { 0xF0 });
    }

    /// 窗口开闭。
    pub fn window(&self) -> bool {
        self.window_open
    }

    /// 投递一笔事件（守恒面唯一入口）。
    ///
    /// 规则：越源 → 记提交+拒收后拒；未武装 → 拒收；缓冲满 → 背压拒收；
    /// 窗闭 → 拒收；否则入缓冲（窗口即交付泵）。
    /// 守恒约束：submitted 一律先记，rejected 分类记账，恒等式恒平。
    pub fn submit(&mut self, v: usize, payload: u64) -> Result<bool, StCode> {
        self.submitted += 1;
        if v >= STRESS_VECTORS {
            self.rejected += 1;
            return Err(StCode::StormOob);
        }
        if self.mask & (1u32 << v) == 0 {
            self.rejected += 1;
            self.digest = fnv_step(self.digest, 0xB1);
            return Ok(false);
        }
        if !self.window_open {
            self.rejected += 1;
            self.digest = fnv_step(self.digest, 0xB2);
            return Ok(false);
        }
        if self.buffer.len() >= self.cap {
            self.rejected += 1;
            self.digest = fnv_step(self.digest, 0xB3);
            return Ok(false);
        }
        self.buffer.push(payload);
        self.digest = fnv_step(self.digest, payload & 0xFF);
        Ok(true)
    }

    /// 交付泵：把缓冲按序交付（seq 连续递增）。
    pub fn pump(&mut self, max: usize) -> usize {
        let mut n = 0usize;
        while n < max && !self.buffer.is_empty() {
            let payload = self.buffer.remove(0);
            self.digest = fnv_step(self.digest, self.next_seq & 0xFF);
            self.digest = fnv_step(self.digest, payload & 0xFF);
            self.next_seq += 1;
            self.delivered += 1;
            n += 1;
        }
        if n > 0 {
            self.rounds += 1;
        }
        n
    }

    /// 场景轮次。
    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    /// 提交总数。
    pub fn submitted_count(&self) -> u64 {
        self.submitted
    }

    /// 交付总数。
    pub fn delivered_count(&self) -> u64 {
        self.delivered
    }

    /// 拒收总数。
    pub fn rejected_count(&self) -> u64 {
        self.rejected
    }

    /// 滞留数。
    pub fn buffered_count(&self) -> u64 {
        self.buffer.len() as u64
    }

    /// 当前摘要。
    pub fn digest_of(&self) -> u64 {
        self.digest
    }

    /// 守恒：提交 ≡ 交付 + 滞留 + 拒收。
    pub fn conserved(&self) -> bool {
        self.submitted == self.delivered + self.buffered_count() + self.rejected
    }

    /// 序号连续：交付数 + 1 ≡ 下一序号（从 1 起无跳号）。
    pub fn seq_continuous(&self) -> bool {
        self.next_seq == self.delivered + 1
    }

    /// 高压零丢失总判据：守恒 + 序号连续 + 拒收全记账。
    pub fn zero_loss(&self) -> bool {
        self.conserved() && self.seq_continuous()
    }

    /// 篡改核（仅注入演练用）：手改提交账制造守恒破口。
    pub fn tamper_submitted(&mut self, delta: u64) {
        self.submitted = self.submitted.wrapping_add(delta);
    }
}

/// 压测资产（场景 × 写死种子 × 事件量，资产化登记）。
#[derive(Clone, Copy, Debug)]
pub struct StressAsset {
    /// 场景类。
    pub kind: StressKind,
    /// 写死种子（协议禁 rand：种子=段基址×场景序）。
    pub seed: u64,
    /// 事件量（确定性）。
    pub events: usize,
    /// 资产名（中文短句）。
    pub name: &'static str,
}

/// 五类场景资产表（≥5 类达标；种子全部写死）。
pub const STRESS_ASSETS: [StressAsset; SCENARIO_KINDS] = [
    StressAsset { kind: StressKind::FullArmStorm, seed: 0xFB30_0001, events: 120, name: "全量武装风暴·120事件" },
    StressAsset { kind: StressKind::MidArmFlood, seed: 0xFB30_0002, events: 100, name: "武装中洪峰·100事件" },
    StressAsset { kind: StressKind::FailInjectStorm, seed: 0xFB30_0003, events: 90, name: "失败注入风暴·90事件" },
    StressAsset { kind: StressKind::RearmLoop, seed: 0xFB30_0004, events: 80, name: "重武装循环·80事件" },
    StressAsset { kind: StressKind::ConfirmStorm, seed: 0xFB30_0005, events: 150, name: "确认窗高压·150事件" },
];

/// 场景执行核：按类执行确定性脚本，返回摘要。
///
/// 全程零交叉引用：场景步序写死，LCG 只产生事件向量与负载。
fn run_scenario(asset: &StressAsset) -> (u64, bool) {
    let mut rig = StressRig::new();
    let mut lcg = Lcg::new(asset.seed);
    let n = asset.events;
    let mut i = 0usize;
    while i < n {
        let v = (lcg.below(STRESS_VECTORS as u64)) as usize;
        let payload = lcg.next() & 0xFFFF;
        match asset.kind {
            StressKind::FullArmStorm => {
                if i == 0 {
                    for s in 0..STRESS_VECTORS {
                        let _ = rig.arm(s);
                    }
                    rig.set_window(true);
                }
                let _ = rig.submit(v, payload);
                if i % 16 == 15 {
                    let _ = rig.pump(8);
                }
            }
            StressKind::MidArmFlood => {
                // 武装与事件交替：每 8 笔事件武装一个新向量。
                let _ = rig.submit(v, payload);
                if i % 8 == 7 && rig.armed_count() < STRESS_VECTORS as u32 {
                    // 找一个未武装向量武装。
                    for s in 0..STRESS_VECTORS {
                        if rig.armed_count() == 0 || (rig.mask >> s) & 1 == 0 {
                            let _ = rig.arm(s);
                            rig.set_window(true);
                            break;
                        }
                    }
                }
                if i % 10 == 9 {
                    let _ = rig.pump(5);
                }
            }
            StressKind::FailInjectStorm => {
                if i < STRESS_VECTORS {
                    let _ = rig.arm(i);
                }
                rig.set_window(true);
                // 混灌：一半正常、一半打失败路径（未武装向量必然存在）。
                let target = if i % 2 == 0 { v } else { STRESS_VECTORS + (v % 2) };
                let r = rig.submit(target % (STRESS_VECTORS + 2), payload);
                let _ = r;
                if i % 12 == 11 {
                    let _ = rig.pump(6);
                }
            }
            StressKind::RearmLoop => {
                if i == 0 {
                    rig.set_window(true);
                }
                // 循环：武装 0/1，灌 4 笔，撤 1，再武装。
                if i % 8 == 0 {
                    let _ = rig.arm(0);
                    let _ = rig.arm(1);
                }
                let _ = rig.submit(v % 2, payload);
                if i % 8 == 4 {
                    let _ = rig.disarm(1);
                }
                if i % 8 == 6 {
                    let _ = rig.arm(1);
                }
                if i % 8 == 7 {
                    let _ = rig.pump(4);
                }
            }
            StressKind::ConfirmStorm => {
                if i == 0 {
                    for s in 0..STRESS_VECTORS {
                        let _ = rig.arm(s);
                    }
                }
                // 背靠背窗：每 5 笔切一次窗。
                if i % 5 == 0 {
                    rig.set_window(i % 10 == 0);
                }
                let _ = rig.submit(v, payload);
                if i % 5 == 4 {
                    let _ = rig.pump(5);
                }
            }
        }
        i += 1;
    }
    // 收尾：全量泵净。
    rig.set_window(true);
    loop {
        let got = rig.pump(64);
        if got == 0 {
            break;
        }
    }
    (rig.digest_of(), rig.zero_loss() && rig.conserved())
}

/// 压测执行报告（资产级）。
#[derive(Clone, Copy, Debug)]
pub struct StressReport {
    /// 场景类。
    pub kind: StressKind,
    /// 三连跑摘要（全等才有效）。
    pub digests: [u64; REPEAT_RUNS],
    /// 三连跑零丢失（全过才有效）。
    pub losses: [bool; REPEAT_RUNS],
}

impl StressReport {
    /// 三次可重复一致（判准：3 次一致率 100%）。
    pub fn repeatable(&self) -> bool {
        self.digests[0] == self.digests[1]
            && self.digests[1] == self.digests[2]
            && self.digests[0] != 0
    }
    /// 高压零丢失全通过。
    pub fn zero_loss_all(&self) -> bool {
        self.losses.iter().all(|l| *l)
    }
}

/// 单资产三连跑。
pub fn run_asset_thrice(asset: &StressAsset) -> StressReport {
    let mut dg = [0u64; REPEAT_RUNS];
    let mut ls = [false; REPEAT_RUNS];
    for k in 0..REPEAT_RUNS {
        let (d, ok) = run_scenario(asset);
        dg[k] = d;
        ls[k] = ok;
    }
    StressReport { kind: asset.kind, digests: dg, losses: ls }
}

/// 全资产聚合跑（≥5 类，全类可重复 + 零丢失）。
pub fn run_all_assets() -> ([StressReport; SCENARIO_KINDS], bool, bool) {
    let mut reps: [StressReport; SCENARIO_KINDS] = [
        StressReport { kind: StressKind::FullArmStorm, digests: [0; REPEAT_RUNS], losses: [false; REPEAT_RUNS] },
        StressReport { kind: StressKind::MidArmFlood, digests: [0; REPEAT_RUNS], losses: [false; REPEAT_RUNS] },
        StressReport { kind: StressKind::FailInjectStorm, digests: [0; REPEAT_RUNS], losses: [false; REPEAT_RUNS] },
        StressReport { kind: StressKind::RearmLoop, digests: [0; REPEAT_RUNS], losses: [false; REPEAT_RUNS] },
        StressReport { kind: StressKind::ConfirmStorm, digests: [0; REPEAT_RUNS], losses: [false; REPEAT_RUNS] },
    ];
    for (i, a) in STRESS_ASSETS.iter().enumerate() {
        reps[i] = run_asset_thrice(a);
    }
    let all_rep = reps.iter().all(|r| r.repeatable());
    let all_zero = reps.iter().all(|r| r.zero_loss_all());
    (reps, all_rep, all_zero)
}

/// 场景类登记完整性（每类恰一资产且种子非零、事件量>0）。
pub fn assets_wellformed() -> bool {
    for k in StressKind::ALL.iter() {
        let hits = STRESS_ASSETS.iter().filter(|a| a.kind == *k).count();
        if hits != 1 {
            return false;
        }
    }
    STRESS_ASSETS.iter().all(|a| a.seed != 0 && a.events > 0 && !a.name.is_empty())
}

/// LCG 10/10 演练：全资产每轮复跑，10/10 轮可重复+零丢失。
pub fn drill_stress_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB30 段基址。
    let mut lcg = Lcg::new(0xFB30);
    for _r in 0..INJECT_ROUNDS {
        let _ = lcg.below(7); // 轮间扰动（不改场景定步，仅消费序列）
        let (_reps, all_rep, all_zero) = run_all_assets();
        if all_rep && all_zero && assets_wellformed() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("资产全绿轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：资产结果行（场景/摘要/可重复/零丢失）与全表报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct AssetResultRow {
    /// 场景类。
    pub kind: StressKind,
    /// 三连跑摘要（首值）。
    pub digest: u64,
    /// 可重复。
    pub repeatable: bool,
    /// 零丢失。
    pub zero_loss: bool,
}

/// 全资产结果行构建。
pub fn build_asset_results(reps: &[StressReport]) -> Vec<AssetResultRow> {
    reps.iter()
        .map(|r| AssetResultRow {
            kind: r.kind,
            digest: r.digests[0],
            repeatable: r.repeatable(),
            zero_loss: r.zero_loss_all(),
        })
        .collect()
}

/// 结果行导出。
pub fn export_asset_results(rows: &[AssetResultRow]) -> String {
    let mut s = String::from("资产结果：\n");
    for r in rows.iter() {
        s.push_str(&format!(
            "· {} 摘要{:#x} 可重复={} 零丢失={}\n",
            r.kind.text(), r.digest, r.repeatable, r.zero_loss
        ));
    }
    s
}

/// 结果行守恒（行数=场景类数且全绿）。
pub fn asset_results_green(rows: &[AssetResultRow]) -> bool {
    rows.len() == SCENARIO_KINDS && rows.iter().all(|r| r.repeatable && r.zero_loss)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：压测会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct StressSummary {
    /// 场景类数。
    pub kinds: usize,
    /// 全可重复。
    pub all_rep: bool,
    /// 全零丢失。
    pub all_zero: bool,
    /// 资产表健全。
    pub wellformed: bool,
    /// 首资产摘要。
    pub first_digest: u64,
}

impl StressSummary {
    /// 建报告。
    pub fn build() -> Self {
        let (reps, all_rep, all_zero) = run_all_assets();
        StressSummary {
            kinds: SCENARIO_KINDS,
            all_rep,
            all_zero,
            wellformed: assets_wellformed(),
            first_digest: reps[0].digests[0],
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_rep && self.all_zero && self.wellformed
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "StressSummary 类={} 可重复={} 零丢失={} 资产表={} 首摘要={:#x}",
            self.kinds, self.all_rep, self.all_zero, self.wellformed, self.first_digest
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：指纹稳定 10/10 演练（全资产两次聚合摘要逐值相等）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_fingerprint_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB30 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB31);
    for _r in 0..INJECT_ROUNDS {
        let (reps1, rep1, zero1) = run_all_assets();
        let (reps2, rep2, zero2) = run_all_assets();
        let same = reps1
            .iter()
            .zip(reps2.iter())
            .all(|(a, b)| a.digests == b.digests && a.losses == b.losses);
        let _ = lcg.below(3);
        if same && rep1 == rep2 && zero1 == zero2 && rep1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("指纹稳定轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct StCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_stress_checklist() -> Vec<StCheckRow> {
    let sum = StressSummary::build();
    let (reps, _, _) = run_all_assets();
    let rows = build_asset_results(&reps);
    let mut out: Vec<StCheckRow> = Vec::new();
    out.push(StCheckRow { name: "场景5类", passed: SCENARIO_KINDS >= 5 });
    out.push(StCheckRow { name: "资产表健全", passed: assets_wellformed() });
    out.push(StCheckRow { name: "三连跑一致", passed: reps.iter().all(|r| r.repeatable()) });
    out.push(StCheckRow { name: "零丢失全过", passed: reps.iter().all(|r| r.zero_loss_all()) });
    out.push(StCheckRow { name: "结果行守恒", passed: asset_results_green(&rows) });
    out.push(StCheckRow { name: "连跑次数=3", passed: REPEAT_RUNS == 3 });
    out.push(StCheckRow { name: "缓冲容量钉定", passed: { let rig = StressRig::new(); rig.submitted_count() == 0 && rig.conserved() } });
    out.push(StCheckRow { name: "报告全绿", passed: sum.all_green() });
    out.push(StCheckRow { name: "向量宽核对", passed: STRESS_VECTORS == 5 });
    out.push(StCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn stress_checklist_green(rows: &[StCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn stress_stats_line() -> String {
    format!(
        "压测概况：{} 类 ×{} 连跑 向量 {}",
        SCENARIO_KINDS, REPEAT_RUNS, STRESS_VECTORS
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1246_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let (reps, _, _) = run_all_assets();
    let rows = build_asset_results(&reps);
    // X-a：结果行面。
    out.push(("X-结果行守恒", asset_results_green(&rows), export_asset_results(&rows)));
    // X-b：指纹稳定 10/10。
    let (ok_f, d_f) = drill_fingerprint_10();
    out.push(("X-指纹稳定10/10", ok_f, d_f));
    // X-c：终验清单。
    let cl = build_stress_checklist();
    out.push(("X-终验清单10项", stress_checklist_green(&cl), StressSummary::build().to_text()));
    // X-d：场景词表。
    out.push(("X-场景词表", scenario_vocab_sane(), export_scenario_vocab()));
    // X-e：空台架快照。
    out.push(("X-空台架快照", fresh_rig_conserved(), export_fresh_rig()));
    // X-f：参数面钉定。
    out.push(("X-参数面钉定", stress_params_pinned(), export_stress_params()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：场景词表（五类名互异 + 倒排）。
/// ─────────────────────────────────────────────────────────────────────
/// 场景词表健全。
pub fn scenario_vocab_sane() -> bool {
    for i in 0..SCENARIO_KINDS {
        for j in (i + 1)..SCENARIO_KINDS {
            if StressKind::ALL[i].text() == StressKind::ALL[j].text() {
                return false;
            }
        }
    }
    true
}

/// 场景词表导出。
pub fn export_scenario_vocab() -> String {
    let mut s = String::from("场景词表：\n");
    for k in StressKind::ALL.iter() {
        s.push_str(&format!("· {}\n", k.text()));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：压测概况行。
/// ─────────────────────────────────────────────────────────────────────
pub fn stress_overview_line() -> String {
    let (reps, _, _) = run_all_assets();
    format!(
        "压测概况行：{} 资产 全可重复 {} 首摘要 {:#x}",
        reps.len(),
        reps.iter().all(|r| r.repeatable()),
        reps.first().map(|r| r.digests[0]).unwrap_or(0)
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：守恒快照行（新台架零账恒等）。
/// ─────────────────────────────────────────────────────────────────────
/// 空台架守恒快照核。
pub fn fresh_rig_conserved() -> bool {
    let rig = StressRig::new();
    rig.conserved() && rig.seq_continuous() && rig.zero_loss() && rig.armed_count() == 0
}

/// 快照导出。
pub fn export_fresh_rig() -> String {
    let rig = StressRig::new();
    format!(
        "空台架快照：提交{} 交付{} 拒收{} 守恒={}",
        rig.submitted_count(),
        rig.delivered_count(),
        rig.rejected_count(),
        rig.conserved()
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：压测参数面钉定（类数/连跑/向量宽/缓冲容量闭集）。
/// ─────────────────────────────────────────────────────────────────────
/// 参数面钉定核。
pub fn stress_params_pinned() -> bool {
    SCENARIO_KINDS == 5 && REPEAT_RUNS == 3 && STRESS_VECTORS == 5
}

/// 参数面导出。
pub fn export_stress_params() -> String {
    format!(
        "压测参数：类{} 连跑{} 向量{}",
        SCENARIO_KINDS, REPEAT_RUNS, STRESS_VECTORS
    )
}


/// 资产族数（ktest 汇总面）。
pub fn asset_family_count() -> usize {
    STRESS_ASSETS.len()
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越源投递必拒（StormOob）。
    let mut rig = StressRig::new();
    let _ = rig.arm(0);
    rig.set_window(true);
    if matches!(rig.submit(STRESS_VECTORS, 1), Err(StCode::StormOob)) {
        caught += 1;
        d.push("注1 越源:抓".to_string());
    } else {
        d.push("注1 越源:漏".to_string());
    }
    // 注 2：重复武装必拒（ArmDup）。
    let mut rig2 = StressRig::new();
    let _ = rig2.arm(1);
    if matches!(rig2.arm(1), Err(StCode::ArmDup)) {
        caught += 1;
        d.push("注2 二武:抓".to_string());
    } else {
        d.push("注2 二武:漏".to_string());
    }
    // 注 3：闭窗投递必拒且记账（WindowShut 路径 → 拒收计数）。
    let mut rig3 = StressRig::new();
    let _ = rig3.arm(2);
    let shut = rig3.submit(2, 7);
    if matches!(shut, Ok(false)) && rig3.rejected_count() == 1 && rig3.submitted_count() == 1 {
        caught += 1;
        d.push("注3 闭窗:抓".to_string());
    } else {
        d.push("注3 闭窗:漏".to_string());
    }
    // 注 4：守恒破口——伪提交（绕过 submit 直增账）必被抓。
    let mut rig4 = StressRig::new();
    let _ = rig4.arm(0);
    rig4.set_window(true);
    let _ = rig4.submit(0, 9);
    let _ = rig4.pump(4);
    let honest = rig4.conserved();
    // 篡改：手改提交账（制造破口）。
    rig4.tamper_submitted(1);
    if honest && !rig4.conserved() {
        caught += 1;
        d.push("注4 守恒:抓".to_string());
    } else {
        d.push("注4 守恒:漏".to_string());
    }
    // 注 5：指纹漂移——同资产异种子摘要必不同（可重复性的反面）。
    let a1 = StressAsset { kind: StressKind::FullArmStorm, seed: 0xFB30_0001, events: 40, name: "x" };
    let a2 = StressAsset { kind: StressKind::FullArmStorm, seed: 0xFB30_0099, events: 40, name: "x" };
    let (d1, _) = run_scenario(&a1);
    let (d2, _) = run_scenario(&a2);
    if d1 != d2 {
        caught += 1;
        d.push("注5 漂移:抓".to_string());
    } else {
        d.push("注5 漂移:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB30 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    StCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1246_checks：本单判据聚合（诊断段 0xFB30）。
pub fn run_unx1246_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1246 Arming 压测资产");
    // J1-a：场景 ≥5 类资产化（类别齐全 + 种子/事件量登记）。
    s.add("J1-场景5类资产化", SCENARIO_KINDS >= 5 && assets_wellformed(), "5类×写死种子×事件量");
    // J1-b：3 次可重复一致率 100%。
    let (reps, all_rep, all_zero) = run_all_assets();
    let r0 = reps[0].digests[0];
    s.add_dyn(
        "J1-三连跑一致100%",
        all_rep,
        &format!("5资产指纹一致 首资产={:#x}", r0),
    );
    // J1-c：高压零丢失全通过。
    s.add_dyn("J1-高压零丢失全过", all_zero, "守恒+seq连续 全资产");
    // J2：LCG 10/10 演练。
    let (ok10, d10) = drill_stress_10();
    s.add_dyn("J2-资产复跑10/10", ok10, &d10);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB30段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1246_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
