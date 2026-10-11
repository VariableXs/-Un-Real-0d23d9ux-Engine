//! cgc05_framecontract — CGPU-F0006 80 帧合同与承诺报告（把稳定 80 帧从口号
//! 变成合同文本）。
//!
//! 判据原文（施工书锚点 CGPU-F0006）：合同定义基准场景三档：基准 A（30 网页
//! 静态）、基准 B（30 网页 + 4 路 4K 动图）、基准 C（30 网页 + 8 路 4K 动图 +
//! 视频墙），每档帧率承诺（P95/P99/最大卡顿）、画质档位、硬件下限。合同执
//! 行器每帧记账：实际帧间隔、各账户耗时、降质动作；持续 10 秒聚合一次出"承
//! 诺兑现率"。不兑现的处置分级：单帧超时→帧内降质；连续 5 秒 P95 不达标→
//! 自动降质链；连续 60 秒仍不达标→判定硬件低于合同下限，出书面报告（当前画
//! 质档、瓶颈归因、建议）。合同文本随版本冻结，改动走 ADR。判据：QEMU 基准
//! B 场景 P95 ≥80fps 实测、三档合同文本冻结、兑现率报告逐 10 秒出、降质链联
//! 动正确、书面报告归因可定位到账户。
//!
//! 职责定位：三档合同文本冻结+每帧记账+10 秒兑现率聚合+三级处置分级状态机+
//! 书面报告（归因定位到账户）。与 cgd05_ratecontract（F0485）分工——彼是通
//! 用帧率合同校验器（三条款滚动窗），此是 80 帧承诺的专用合同与处置分级（场
//! 景三档+画质档位+硬件下限+书面报告）。账户耗时记账口径对齐 cgc04
//! （AcctKind 五账户 ordinal 同表）。诊断码段 0x42xx。

use alloc::vec::Vec;

/// 合同版本（合同文本随版本冻结——版本号即冻结标识，改动走 ADR）。
pub const FRAME_CONTRACT_VERSION: &str = "C05-framecontract-v1";

/// 80fps 承诺帧间隔 12.5ms。
pub const PROMISE_GAP_US: u32 = 12_500;
/// P99 承诺 25ms（两倍）。
pub const PROMISE_P99_US: u32 = 25_000;
/// 最大卡顿承诺 50ms。
pub const PROMISE_JANK_US: u32 = 50_000;

/// 10 秒聚合窗（80fps 基准 = 800 帧；判据"持续 10 秒聚合一次"）。
pub const AGG_WINDOW_FRAMES: u32 = 800;
/// 连续 5 秒 P95 不达标 → 自动降质链（400 帧 @80fps）。
pub const DEGRADE_CHAIN_FRAMES: u32 = 400;
/// 连续 60 秒仍不达标 → 硬件低于合同下限书面报告（4800 帧）。
pub const BELOW_FLOOR_FRAMES: u32 = 4800;

/// 画质档位闭集（合同文本的画质承诺载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityTier {
    High,
    Medium,
    Low,
}

impl QualityTier {
    pub const fn zh(self) -> &'static str {
        match self {
            QualityTier::High => "高画质",
            QualityTier::Medium => "中画质",
            QualityTier::Low => "低画质",
        }
    }
}

/// 基准场景三档（对齐 cgd05_ratecontract::Scene 的 A/B/C——本模块聚焦三档
/// 合同文本，Free 场景不入合同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchScene {
    A,
    B,
    C,
}

impl BenchScene {
    pub const ALL: [BenchScene; 3] = [BenchScene::A, BenchScene::B, BenchScene::C];

    pub const fn zh(self) -> &'static str {
        match self {
            BenchScene::A => "基准A（30网页静态）",
            BenchScene::B => "基准B（30网页+4路4K动图）",
            BenchScene::C => "基准C（30网页+8路4K动图+视频墙）",
        }
    }

    /// 档位序（归因报告引用）。
    pub const fn ordinal(self) -> usize {
        match self {
            BenchScene::A => 0,
            BenchScene::B => 1,
            BenchScene::C => 2,
        }
    }
}

/// 合同条款行（单档文本——三档结构同形，阈值可按档收紧或放宽）。
#[derive(Clone, Copy, Debug)]
pub struct ContractClause {
    /// 适用档。
    pub scene: BenchScene,
    /// P95 承诺（µs；≤ 此值记达标帧）。
    pub p95_us: u32,
    /// P99 承诺（µs）。
    pub p99_us: u32,
    /// 最大卡顿承诺（µs；单帧超此值即单帧违约）。
    pub max_jank_us: u32,
    /// 承诺画质档位。
    pub quality: QualityTier,
    /// 硬件下限（抽象算力分——低于此分不签该档合同）。
    pub hw_floor_score: u32,
}

/// 三档合同文本（构造即冻结——判据"合同文本随版本冻结"）。
pub struct FrameContract {
    /// 三档条款（下标对齐 BenchScene::ordinal）。
    clauses: [ContractClause; 3],
    /// 冻结指纹（FNV-1a——文本篡改可检出）。
    fingerprint: u64,
}

impl FrameContract {
    /// 默认三档合同（阈值三档同口径：P95 12.5ms / P99 25ms / 卡顿 50ms——
    /// 承诺不因场景重而放水，放水走画质档位；硬件下限按档递增）。
    pub fn standard() -> Self {
        let clauses = [
            ContractClause {
                scene: BenchScene::A,
                p95_us: PROMISE_GAP_US,
                p99_us: PROMISE_P99_US,
                max_jank_us: PROMISE_JANK_US,
                quality: QualityTier::High,
                hw_floor_score: 100,
            },
            ContractClause {
                scene: BenchScene::B,
                p95_us: PROMISE_GAP_US,
                p99_us: PROMISE_P99_US,
                max_jank_us: PROMISE_JANK_US,
                quality: QualityTier::High,
                hw_floor_score: 160,
            },
            ContractClause {
                scene: BenchScene::C,
                p95_us: PROMISE_GAP_US,
                p99_us: PROMISE_P99_US,
                max_jank_us: PROMISE_JANK_US,
                quality: QualityTier::Medium,
                hw_floor_score: 220,
            },
        ];
        let mut c = FrameContract { clauses, fingerprint: 0 };
        c.fingerprint = c.compute_fingerprint();
        c
    }

    /// 条款只读。
    pub fn clause(&self, scene: BenchScene) -> &ContractClause {
        &self.clauses[scene.ordinal()]
    }

    /// 合同冻结指纹（场景+阈值+画质+下限逐字节 FNV）。
    pub fn compute_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for cl in &self.clauses {
            for v in [
                cl.scene.ordinal() as u64,
                cl.p95_us as u64,
                cl.p99_us as u64,
                cl.max_jank_us as u64,
                cl.quality as u8 as u64,
                cl.hw_floor_score as u64,
            ] {
                let bytes = v.to_le_bytes();
                for b in bytes {
                    h = (h ^ b as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
        h
    }

    /// 冻结指纹只读（消费方对账）。
    pub const fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// 签约资格：硬件分低于该档下限即拒签（判据"硬件下限"）。
    pub const fn signable(&self, scene: BenchScene, hw_score: u32) -> bool {
        hw_score >= self.clauses[scene.ordinal()].hw_floor_score
    }
}

/// 每帧记账行（判据"每帧记账：实际帧间隔、各账户耗时、降质动作"）。
#[derive(Clone, Copy, Debug)]
pub struct FrameRow {
    /// 帧号。
    pub frame_no: u64,
    /// 实际帧间隔（µs）。
    pub gap_us: u32,
    /// 五账户耗时（µs；下标对齐 cgc04 AcctKind ordinal——口径对齐声明）。
    pub acct_us: [u64; 5],
    /// 本帧降质动作档（0=无；1..=轻中重——帧内降质信号由 F0008 消费）。
    pub degrade_action: u8,
}

/// 诊断码（0x42xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4201 单帧超时——帧内降质信号（F0008 消费）。
    FcFrameOver,
    /// 0x4202 连续 5 秒 P95 不达标——自动降质链信号。
    FcChainFire,
    /// 0x4203 连续 60 秒不达标——硬件低于合同下限，出书面报告。
    FcBelowFloor,
    /// 0x4204 聚合窗未满不给兑现率（样本不足显性）。
    FcWindowNotFull,
    /// 0x4205 场景不在合同三档内（Free 场景不入合同）。
    FcSceneOffContract,
    /// 0x4206 硬件分低于合同下限——拒签。
    FcHwBelowFloor,
    /// 0x4207 合同指纹对不上（文本被篡改）。
    FcFingerprintMismatch,
    /// 0x4208 记账行序断裂（frame_no 不单调）。
    FcLedgerBroken,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::FcFrameOver => "单帧超时：帧内降质信号已发（先走任务自身降级档——F0008 消费）",
            XcCode::FcChainFire => "连续 5 秒 P95 不达标：自动降质链信号已发",
            XcCode::FcBelowFloor => "连续 60 秒不达标：判定硬件低于合同下限，出书面报告",
            XcCode::FcWindowNotFull => "聚合窗未满：样本不足不下兑现率结论",
            XcCode::FcSceneOffContract => "场景不在合同三档内：自由负载不走合同处置",
            XcCode::FcHwBelowFloor => "硬件分低于该档合同下限：拒签",
            XcCode::FcFingerprintMismatch => "合同指纹不符：合同文本被篡改",
            XcCode::FcLedgerBroken => "记账行序断裂：frame_no 必须单调",
        }
    }
}

/// 处置分级状态（判据三级处置的当前态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposal {
    /// 正常（窗内达标）。
    Nominal,
    /// 单帧降质（瞬时态，帧内消化）。
    FrameDegrade,
    /// 降质链已触发（连续 5 秒 P95 失守）。
    ChainFired,
    /// 低于合同下限（连续 60 秒——书面报告态）。
    BelowFloor,
}

/// 10 秒兑现率报告（判据"持续 10 秒聚合一次出承诺兑现率"）。
#[derive(Clone, Debug)]
pub struct FulfillReport {
    /// 报告序（每聚合窗 +1）。
    pub seq: u64,
    /// 场景档。
    pub scene: BenchScene,
    /// 窗口帧数。
    pub frames: u32,
    /// 兑现帧数（P95 内帧）。
    pub ok_frames: u32,
    /// 兑现率（ppm）。
    pub fulfill_ppm: u64,
    /// P95 实测（µs）。
    pub p95_us: u32,
    /// P99 实测（µs）。
    pub p99_us: u32,
    /// 超卡顿线帧数。
    pub jank_frames: u32,
    /// 当前处置态。
    pub disposal: Disposal,
}

/// 书面报告（判据"连续 60 秒仍不达标→出书面报告"——归因可定位到账户）。
#[derive(Clone, Debug)]
pub struct WrittenReport {
    /// 报告序。
    pub seq: u64,
    /// 场景档。
    pub scene: BenchScene,
    /// 当前画质档。
    pub quality: QualityTier,
    /// 瓶颈账户归因（五账户 ordinal——判据"归因可定位到账户"）。
    pub bottleneck_acct_ordinal: usize,
    /// 瓶颈账户累计超支量（µs）。
    pub bottleneck_over_us: u64,
    /// 建议文本（闭集建议表）。
    pub advice: &'static str,
}

/// 归因建议闭集（按瓶颈账户映射——单值映射不静默）。
pub fn advice_for(acct_ordinal: usize) -> &'static str {
    match acct_ordinal {
        0 => "合成器保底超支：检查合成器任务队列与表面数量",
        1 => "交互响应超支：检查输入路径任务与事件风暴合批",
        2 => "网页表面池超支：触发表面池预算重排与后台冻结",
        3 => "动态画面池超支：降动图刷新率或合批交错渲染",
        _ => "机动池超支：机动预算耗尽，禁止再接纳机动任务",
    }
}

/// 80 帧合同执行器（判据主路径本体）。
pub struct FrameContractRunner {
    /// 冻结合同。
    contract: FrameContract,
    /// 当前场景（仅合同三档）。
    scene: Option<BenchScene>,
    /// 记账账本。
    ledger: Vec<FrameRow>,
    /// 聚合窗内帧间隔缓冲（分位计算用）。
    win_gaps: Vec<u32>,
    /// 聚合窗内各账户累计耗时（归因用）。
    win_acct_us: [u64; 5],
    /// 窗内降质动作帧计数。
    win_degrade_frames: u32,
    /// 窗口起始帧号。
    win_start: u64,
    /// 报告序发生器。
    report_seq: u64,
    /// 连续 P95 失守帧计数（降质链判据）。
    chain_miss: u32,
    /// 连续失守帧计数（低于下限判据——跨窗累计）。
    below_miss: u32,
    /// 处置态。
    pub disposal: Disposal,
    /// 书面报告账。
    pub reports: Vec<WrittenReport>,
    /// 立案计数。
    pub cases: u32,
}

impl FrameContractRunner {
    /// 构造（带标准三档合同）。
    pub fn new() -> Self {
        FrameContractRunner {
            contract: FrameContract::standard(),
            scene: None,
            ledger: Vec::new(),
            win_gaps: Vec::new(),
            win_acct_us: [0; 5],
            win_degrade_frames: 0,
            win_start: 0,
            report_seq: 0,
            chain_miss: 0,
            below_miss: 0,
            disposal: Disposal::Nominal,
            reports: Vec::new(),
            cases: 0,
        }
    }

    /// 签约（场景必须三档内+硬件分达标——否则显性拒）。
    pub fn sign(&mut self, scene: BenchScene, hw_score: u32) -> Result<(), XcCode> {
        if !self.contract.signable(scene, hw_score) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::FcHwBelowFloor);
        }
        self.scene = Some(scene);
        Ok(())
    }

    /// 合同文本只读（消费方展示合同）。
    pub const fn contract(&self) -> &FrameContract {
        &self.contract
    }

    /// 当前场景。
    pub const fn scene(&self) -> Option<BenchScene> {
        self.scene
    }

    /// 逐帧记账（判据主路径入口）：入账 + 三级处置判定。
    pub fn feed_frame(
        &mut self,
        frame_no: u64,
        gap_us: u32,
        acct_us: [u64; 5],
        degrade_action: u8,
    ) -> Result<Disposal, XcCode> {
        if self.scene.is_none() {
            return Err(XcCode::FcSceneOffContract);
        }
        // 记账
        self.ledger.push(FrameRow { frame_no, gap_us, acct_us, degrade_action });
        // 聚合窗累计
        self.win_gaps.push(gap_us);
        for i in 0..5 {
            self.win_acct_us[i] = self.win_acct_us[i].saturating_add(acct_us[i]);
        }
        if degrade_action > 0 {
            self.win_degrade_frames = self.win_degrade_frames.saturating_add(1);
        }
        // 三级处置判定（阈值取当前档条款）
        let cl = self.contract.clause(self.scene.unwrap());
        // 一级：单帧超时（超最大卡顿承诺）
        if gap_us > cl.max_jank_us {
            self.disposal = Disposal::FrameDegrade;
            self.cases = self.cases.saturating_add(1);
            // 单帧超时计入 P95 失守序列（超卡顿必然超 P95）
            self.chain_miss = self.chain_miss.saturating_add(1);
            self.below_miss = self.below_miss.saturating_add(1);
        } else {
            // 未超卡顿线：按 P95 承诺线判失守（超 12.5ms 即失守一帧——
            // 保守计数：严格按逐帧 P95 代理=逐帧超承诺线计数）
            if gap_us > cl.p95_us {
                self.chain_miss = self.chain_miss.saturating_add(1);
                self.below_miss = self.below_miss.saturating_add(1);
            } else {
                self.chain_miss = 0;
                self.below_miss = self.below_miss.saturating_sub(1.min(self.below_miss));
            }
        }
        // 二级：连续 5 秒 P95 不达标 → 自动降质链
        if self.chain_miss >= DEGRADE_CHAIN_FRAMES {
            self.disposal = Disposal::ChainFired;
            self.cases = self.cases.saturating_add(1);
        }
        // 三级：连续 60 秒仍不达标 → 低于合同下限书面报告
        if self.below_miss >= BELOW_FLOOR_FRAMES {
            self.disposal = Disposal::BelowFloor;
            self.cases = self.cases.saturating_add(1);
            let rep = self.build_written_report();
            self.reports.push(rep);
            self.below_miss = 0; // 报告后重计（下一轮 60s 窗）
        }
        Ok(self.disposal)
    }

    /// 10 秒聚合出兑现率报告（判据"逐 10 秒出"——调用方按窗满调）。
    pub fn fulfill_report(&mut self) -> Result<FulfillReport, XcCode> {
        if self.win_gaps.len() < AGG_WINDOW_FRAMES as usize {
            return Err(XcCode::FcWindowNotFull);
        }
        // 桶计数分位（0.5ms 桶，0~100ms 量程——与 cgd05 同思想独立实现）
        let buckets_n = 200usize;
        let mut buckets = [0u32; 200];
        let bucket_us = 500u32;
        let mut jank_frames = 0u32;
        for &g in &self.win_gaps {
            let bi = (g / bucket_us) as usize;
            if bi >= buckets_n {
                buckets[buckets_n - 1] += 1;
            } else {
                buckets[bi] += 1;
            }
            if g > PROMISE_JANK_US {
                jank_frames += 1;
            }
        }
        let p95 = percentile(&buckets, bucket_us, self.win_gaps.len() as u64, 950_000);
        let p99 = percentile(&buckets, bucket_us, self.win_gaps.len() as u64, 990_000);
        let ok = self
            .win_gaps
            .iter()
            .filter(|&&g| g <= PROMISE_GAP_US)
            .count() as u32;
        let fulfill = (ok as u64) * 1_000_000 / self.win_gaps.len() as u64;
        self.report_seq = self.report_seq.saturating_add(1);
        let rep = FulfillReport {
            seq: self.report_seq,
            scene: self.scene.unwrap_or(BenchScene::A),
            frames: self.win_gaps.len() as u32,
            ok_frames: ok,
            fulfill_ppm: fulfill,
            p95_us: p95,
            p99_us: p99,
            jank_frames,
            disposal: self.disposal,
        };
        // 窗清空（下一聚合窗）
        self.win_gaps.clear();
        self.win_acct_us = [0; 5];
        self.win_degrade_frames = 0;
        self.win_start = self.ledger.last().map(|r| r.frame_no).unwrap_or(0);
        Ok(rep)
    }

    /// 书面报告构造（归因定位到账户：窗内各账户累计耗时最大者即瓶颈）。
    fn build_written_report(&self) -> WrittenReport {
        let mut bottleneck = 0usize;
        let mut max_us = 0u64;
        for i in 0..5 {
            if self.win_acct_us[i] > max_us {
                max_us = self.win_acct_us[i];
                bottleneck = i;
            }
        }
        let scene = self.scene.unwrap_or(BenchScene::A);
        WrittenReport {
            seq: self.report_seq + 1,
            scene,
            quality: self.contract.clause(scene).quality,
            bottleneck_acct_ordinal: bottleneck,
            bottleneck_over_us: max_us,
            advice: advice_for(bottleneck),
        }
    }

    /// 记账行序核验（frame_no 单调；断裂=F cLedgerBroken）。
    pub fn ledger_sequential(&mut self) -> Result<(), XcCode> {
        let mut prev = 0u64;
        for r in &self.ledger {
            if r.frame_no <= prev {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::FcLedgerBroken);
            }
            prev = r.frame_no;
        }
        Ok(())
    }

    /// 记账账本只读。
    pub fn ledger(&self) -> &[FrameRow] {
        &self.ledger
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        let scene_s = match self.scene {
            Some(s) => s.zh(),
            None => "未签约",
        };
        alloc::format!(
            "合同{} {} 处置{:?} 链计{}/{} 低于下限计{}/{} 书面报告{} 立案{}",
            FRAME_CONTRACT_VERSION,
            scene_s,
            self.disposal as u8,
            self.chain_miss,
            DEGRADE_CHAIN_FRAMES,
            self.below_miss,
            BELOW_FLOOR_FRAMES,
            self.reports.len(),
            self.cases,
        )
    }
}

/// 桶计数分位（上沿即代表值——0.5ms 粒度）。
fn percentile(buckets: &[u32; 200], bucket_us: u32, total: u64, ppm: u64) -> u32 {
    let target = (total.saturating_mul(ppm) / 1_000_000).max(1);
    let mut acc = 0u64;
    for (i, &c) in buckets.iter().enumerate() {
        acc += c as u64;
        if acc >= target {
            return ((i as u32) * bucket_us).min(100_000);
        }
    }
    100_000
}
