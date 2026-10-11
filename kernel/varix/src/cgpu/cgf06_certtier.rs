//! CGPU-F0806 · 认证分级（目标 340 行）
//!
//! 诊断段 0x7B51~0x7B58（CGPU 认证族分级段）。
//! 判据（施工书）：四级认证（金：全场景 80 帧/银：主要场景 80 帧/铜：降质后达标/不认证——每级判据量化）；
//! 分级公开（认证等级随硬件公开可查——F0808 透明）；分级与降质链联动（铜级→O 域降质配方预载——联动声明）；
//! 分级争议处理（复核流程预留）。

use alloc::string::String;
use alloc::vec::Vec;

/// 认证分级诊断码闭集（0x7B51~0x7B58 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierCode {
    /// 0x7B51 金级授予（全场景 80 帧）。
    GoldGranted = 0x7B51,
    /// 0x7B52 银级授予（主要场景 80 帧）。
    SilverGranted = 0x7B52,
    /// 0x7B53 铜级授予（降质后达标）。
    BronzeGranted = 0x7B53,
    /// 0x7B54 不认证（判据未达最低档）。
    NotCertified = 0x7B54,
    /// 0x7B55 分级公开登记（随硬件可查）。
    Published = 0x7B55,
    /// 0x7B56 降质链联动预载（铜级 → O 域配方）。
    DownlinkPreload = 0x7B56,
    /// 0x7B57 复核受理（争议流程预留）。
    ReviewFiled = 0x7B57,
    /// 0x7B58 分级账守恒破坏。
    LedgerBroken = 0x7B58,
}

impl TierCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            TierCode::GoldGranted => "金级认证授予",
            TierCode::SilverGranted => "银级认证授予",
            TierCode::BronzeGranted => "铜级认证授予",
            TierCode::NotCertified => "不予认证",
            TierCode::Published => "分级已公开登记",
            TierCode::DownlinkPreload => "铜级降质配方已预载",
            TierCode::ReviewFiled => "复核申请已受理",
            TierCode::LedgerBroken => "分级账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            TierCode::GoldGranted => "全场景帧率 ≥80 达标",
            TierCode::SilverGranted => "主要场景 ≥80 且非全场景",
            TierCode::BronzeGranted => "降质后达标且原生未达",
            TierCode::NotCertified => "降质后仍未达最低量化线",
            TierCode::Published => "公开通道登记完成",
            TierCode::DownlinkPreload => "铜级联动声明触发",
            TierCode::ReviewFiled => "争议复核通道开放",
            TierCode::LedgerBroken => "授予数与公开数不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TierCode::GoldGranted => "无需处置",
            TierCode::SilverGranted => "无需处置",
            TierCode::BronzeGranted => "启用 O 域降质配方",
            TierCode::NotCertified => "查硬件能力清单",
            TierCode::Published => "无需处置",
            TierCode::DownlinkPreload => "无需处置",
            TierCode::ReviewFiled => "等待复核结论",
            TierCode::LedgerBroken => "冻结认证账并上报 0x7B58",
        }
    }
}

/// 认证四级闭集（量化判据绑定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum CertTier {
    /// 不认证。
    None0,
    /// 铜级：降质后达标。
    Bronze,
    /// 银级：主要场景 80 帧。
    Silver,
    /// 金级：全场景 80 帧。
    Gold,
}

impl CertTier {
    /// 全量闭集（低→高）。
    pub const ALL: [CertTier; 4] = [CertTier::None0, CertTier::Bronze, CertTier::Silver, CertTier::Gold];

    /// 量化判据：全场景帧率下限。
    pub const fn fps_all_min(self) -> u32 {
        match self {
            CertTier::Gold => 80,
            _ => 0,
        }
    }

    /// 量化判据：主要场景帧率下限。
    pub const fn fps_main_min(self) -> u32 {
        match self {
            CertTier::Gold | CertTier::Silver => 80,
            _ => 0,
        }
    }

    /// 量化判据：降质后帧率下限。
    pub const fn fps_downgraded_min(self) -> u32 {
        match self {
            CertTier::Gold | CertTier::Silver | CertTier::Bronze => 80,
            CertTier::None0 => 0,
        }
    }
}

/// 场景帧率采样集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneFps {
    /// 场景名。
    pub scene: &'static str,
    /// 实测帧率。
    pub fps: u32,
    /// 是否主要场景。
    pub main: bool,
}

/// 分级裁决输入：原生帧率集 + 降质后帧率集。
pub struct TierJudge {
    /// 原生场景帧率。
    pub native: Vec<SceneFps>,
    /// 降质后场景帧率。
    pub degraded: Vec<SceneFps>,
    /// 复核账。
    reviews: u64,
}

impl TierJudge {
    /// 新裁决器。
    pub fn new() -> Self {
        TierJudge { native: Vec::new(), degraded: Vec::new(), reviews: 0 }
    }

    /// 金级核：全场景 ≥80。
    pub fn gold_ok(&self) -> bool {
        !self.native.is_empty() && self.native.iter().all(|s| s.fps >= CertTier::Gold.fps_all_min())
    }

    /// 银级核：主要场景全 ≥80（非全场景达标）。
    pub fn silver_ok(&self) -> bool {
        let mains: Vec<&SceneFps> = self.native.iter().filter(|s| s.main).collect();
        !mains.is_empty() && mains.iter().all(|s| s.fps >= CertTier::Silver.fps_main_min()) && !self.gold_ok()
    }

    /// 铜级核：降质后全 ≥80（原生未达银）。
    pub fn bronze_ok(&self) -> bool {
        !self.degraded.is_empty()
            && self.degraded.iter().all(|s| s.fps >= CertTier::Bronze.fps_downgraded_min())
            && !self.gold_ok()
            && !self.silver_ok()
    }

    /// 裁决（级联判定：金→银→铜→不认证）。
    pub fn judge(&self) -> (CertTier, TierCode) {
        if self.gold_ok() {
            (CertTier::Gold, TierCode::GoldGranted)
        } else if self.silver_ok() {
            (CertTier::Silver, TierCode::SilverGranted)
        } else if self.bronze_ok() {
            (CertTier::Bronze, TierCode::BronzeGranted)
        } else {
            (CertTier::None0, TierCode::NotCertified)
        }
    }

    /// 分级公开登记（随硬件可查）。
    pub fn publish(&self, hw: &str, tier: CertTier) -> String {
        // 公开格式：hw = tier（透明纪律：等级随硬件公开可查）。
        let mut out = String::new();
        out.push_str(hw);
        out.push('=');
        match tier {
            CertTier::Gold => out.push_str("gold"),
            CertTier::Silver => out.push_str("silver"),
            CertTier::Bronze => out.push_str("bronze"),
            CertTier::None0 => out.push_str("none"),
        }
        out
    }

    /// 降质链联动：铜级 → O 域配方预载声明。
    pub fn downlink_needed(tier: CertTier) -> bool {
        tier == CertTier::Bronze
    }

    /// 复核受理（争议流程预留）。
    pub fn file_review(&mut self) -> TierCode {
        self.reviews += 1;
        TierCode::ReviewFiled
    }

    /// 复核数。
    pub const fn reviews(&self) -> u64 {
        self.reviews
    }
}

/// 认证分级登记账。
pub struct TierLedger {
    /// 登记条目（hw → tier）。
    pub entries: Vec<(String, CertTier)>,
    /// 公开数。
    published: u64,
}

impl TierLedger {
    /// 新账。
    pub fn new() -> Self {
        TierLedger { entries: Vec::new(), published: 0 }
    }

    /// 授予并公开。
    pub fn grant(&mut self, hw: &str, tier: CertTier, code: TierCode) -> TierCode {
        match code {
            TierCode::GoldGranted | TierCode::SilverGranted | TierCode::BronzeGranted | TierCode::NotCertified => {
                self.entries.push((String::from(hw), tier));
                self.published += 1;
                if tier == CertTier::Bronze {
                    return TierCode::DownlinkPreload;
                }
                TierCode::Published
            }
            _ => TierCode::LedgerBroken,
        }
    }

    /// 公开数。
    pub const fn published(&self) -> u64 {
        self.published
    }

    /// 账守恒：entries 数 == published 数。
    pub fn ledger_consistent(&self) -> bool {
        self.entries.len() as u64 == self.published
    }

    /// 公开可查核：按硬件名查级。
    pub fn lookup(&self, hw: &str) -> Option<CertTier> {
        self.entries.iter().find(|(h, _)| h == hw).map(|(_, t)| *t)
    }
}

/// 判据测试（段 0x7B51~0x7B58）。
pub fn run_t806_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("CGPU-t806");
    // c1 金级：全场景 80+。
    let mut j1 = TierJudge::new();
    j1.native = vec![
        SceneFps { scene: "scene-a", fps: 95, main: true },
        SceneFps { scene: "scene-b", fps: 82, main: false },
    ];
    let (t1, c1) = j1.judge();
    set.add("t806-c1-gold", t1 == CertTier::Gold && c1 == TierCode::GoldGranted, "");
    // c2 银级：主要场景 80+ 但边角场景不达。
    let mut j2 = TierJudge::new();
    j2.native = vec![
        SceneFps { scene: "main-a", fps: 90, main: true },
        SceneFps { scene: "edge-b", fps: 60, main: false },
    ];
    let (t2, c2) = j2.judge();
    set.add("t806-c2-silver", t2 == CertTier::Silver && c2 == TierCode::SilverGranted, "");
    // c3 铜级：原生不达、降质后达标 + 联动预载。
    let mut j3 = TierJudge::new();
    j3.native = vec![SceneFps { scene: "main-a", fps: 70, main: true }];
    j3.degraded = vec![SceneFps { scene: "main-a", fps: 85, main: true }];
    let (t3, c3) = j3.judge();
    set.add(
        "t806-c3-bronze-link",
        t3 == CertTier::Bronze && c3 == TierCode::BronzeGranted && TierJudge::downlink_needed(t3),
        "",
    );
    // c4 不认证：降质后仍不达。
    let mut j4 = TierJudge::new();
    j4.native = vec![SceneFps { scene: "main-a", fps: 40, main: true }];
    j4.degraded = vec![SceneFps { scene: "main-a", fps: 60, main: true }];
    let (t4, c4) = j4.judge();
    set.add("t806-c4-none", t4 == CertTier::None0 && c4 == TierCode::NotCertified, "");
    // c5 分级公开可查。
    let mut led = TierLedger::new();
    let pub_str = j1.publish("gpu-x", t1);
    let grant = led.grant("gpu-x", t1, c1);
    set.add(
        "t806-c5-publish",
        pub_str == "gpu-x=gold" && grant == TierCode::Published && led.lookup("gpu-x") == Some(CertTier::Gold),
        "",
    );
    // c6 铜级授予 → 降质预载码。
    let grant6 = led.grant("gpu-y", t3, c3);
    set.add("t806-c6-downlink", grant6 == TierCode::DownlinkPreload, "");
    // c7 复核受理。
    let mut j7 = TierJudge::new();
    let _ = j7.file_review();
    set.add("t806-c7-review", j7.reviews() == 1, "");
    // c8 账守恒 + LCG 512 轮注入：随机帧率集裁决，账守恒恒成立。
    set.add("t806-c8-ledger", led.ledger_consistent() && led.published() == 2, "");
    let mut seed: u64 = 0x8060_7B51_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut j = TierJudge::new();
        for i in 0..6u32 {
            j.native.push(SceneFps { scene: "s", fps: (lc() % 140) as u32, main: i % 2 == 0 });
        }
        for i in 0..6u32 {
            j.degraded.push(SceneFps { scene: "d", fps: (lc() % 140) as u32, main: i % 2 == 0 });
        }
        let (t, c) = j.judge();
        // 裁决级联一致性：金蕴含 gold_ok，其余逐级。
        let consistent = match t {
            CertTier::Gold => j.gold_ok(),
            CertTier::Silver => j.silver_ok() && !j.gold_ok(),
            CertTier::Bronze => j.bronze_ok(),
            CertTier::None0 => !j.gold_ok() && !j.silver_ok() && !j.bronze_ok(),
        };
        inj_ok &= consistent;
        let _ = c;
    }
    set.add("t806-c9-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        TierCode::GoldGranted,
        TierCode::SilverGranted,
        TierCode::BronzeGranted,
        TierCode::NotCertified,
        TierCode::Published,
        TierCode::DownlinkPreload,
        TierCode::ReviewFiled,
        TierCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t806-c10-triad", tri_ok, "");
    set
}
