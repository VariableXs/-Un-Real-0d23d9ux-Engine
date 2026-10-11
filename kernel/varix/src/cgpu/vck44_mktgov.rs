//! CGPU-F1725 · 异构市场与生态治理
//!
//! 判据锚点（施工书原文摘录）：
//! 市场治理：模型市场（NPU 模型分发（模型市场（签名/审核——市场接口（安全复用
//! F1626——安全复用；治理（生态治理（恶意模型下架——治理复用 E04 模式；分成
//! （创作者激励预留——激励预留复用；隐私（模型使用统计（匿名——隐私复用；测试
//! （市场/安全/治理/激励/隐私五组）。判据：市场接口、安全、治理、激励、隐私、
//! 五组、判据。
//!
//! 诊断码段 0xA801~0xA805（独占）：
//!   0xA801 市场闸缺（分发/签名/审核三道闸任一缺）
//!   0xA802 签名无效（签名校验不过——复用 F1626 口径）
//!   0xA803 治理迟滞（下架四步越序/超时限/未公示）
//!   0xA804 隐私泄漏（使用统计含未剥离字段）
//!   0xA805 段越界（判码落 0xA800..0xA900 区间外；激励预留并入市场组判 0xA801）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const MKT_GOV_VERSION: &str = "K44-mktgov-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA801~0xA805）
// ---------------------------------------------------------------------------

pub const XC_MK_GATE_MISSING: u32 = 0xA801;
pub const XC_MK_SIG_INVALID: u32 = 0xA802;
pub const XC_MK_GOVERN_LATE: u32 = 0xA803;
pub const XC_MK_PRIVACY_LEAK: u32 = 0xA804;
pub const XC_MK_SEG_OOR: u32 = 0xA805;

// ---------------------------------------------------------------------------
// 一、市场接口（分发/签名/审核 三道闸）
// ---------------------------------------------------------------------------

/// 市场资产：模型条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModelAsset {
    pub id: u64,
    /// 签名指纹（FNV-1a 64——复用 F1626 签名口径）。
    pub sig: u64,
    /// 审核态（0=待审 1=过审 2=拒审）。
    pub review: u8,
    /// 分发开关（过审才可分发）。
    pub distributed: bool,
}

impl ModelAsset {
    /// 三道闸齐判：签名非零 + 审核态在册 + 分发必以过审为前提。
    pub fn gated(&self) -> Result<(), u32> {
        if self.sig == 0 {
            return Err(XC_MK_SIG_INVALID);
        }
        if self.review > 2 {
            return Err(XC_MK_GATE_MISSING);
        }
        if self.distributed && self.review != 1 {
            return Err(XC_MK_GATE_MISSING);
        }
        Ok(())
    }
}

/// 市场本体：资产在册 + 激励预留位。
pub struct ModelMarket {
    pub assets: [Option<ModelAsset>; 8],
    /// 创作者激励分成千分比（预留位——不实装结算，仅占位冻结）。
    pub creator_share_permille: u16,
}

impl ModelMarket {
    pub fn build() -> Self {
        ModelMarket { assets: [None; 8], creator_share_permille: 300 }
    }

    /// 激励预留合法性：分成占比在册区间（0<share<500‰——预留不实装但占位须合理）。
    pub fn incentive_stub_ok(&self) -> Result<(), u32> {
        if self.creator_share_permille == 0 || self.creator_share_permille >= 500 {
            return Err(XC_MK_GATE_MISSING);
        }
        Ok(())
    }

    pub fn list(&mut self, asset: ModelAsset) -> Result<(), u32> {
        asset.gated()?;
        let mut i = 0;
        while i < self.assets.len() {
            if let Some(a) = &self.assets[i] {
                if a.id == asset.id {
                    return Err(XC_MK_GATE_MISSING);
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < self.assets.len() {
            if self.assets[i].is_none() {
                self.assets[i] = Some(asset);
                return Ok(());
            }
            i += 1;
        }
        Err(XC_MK_GATE_MISSING)
    }
}

// ---------------------------------------------------------------------------
// 二、签名校验（安全复用 F1626）
// ---------------------------------------------------------------------------

/// FNV-1a 64（签名指纹口径——与库内既有 fnv 惯例一致）。
pub const fn fnv64(buf: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < buf.len() {
        hash ^= buf[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

/// 签名校验：重算指纹对拍。
pub fn sig_verify(model_bytes: &[u8], claimed_sig: u64) -> Result<(), u32> {
    if claimed_sig == 0 {
        return Err(XC_MK_SIG_INVALID);
    }
    if fnv64(model_bytes) != claimed_sig {
        return Err(XC_MK_SIG_INVALID);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、治理（恶意模型下架——E04 模式四步单调）
// ---------------------------------------------------------------------------

/// 下架四步闭集：举报 → 核验 → 下架 → 公示。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakedownStage {
    Report,
    Verify,
    Remove,
    Announce,
}

impl TakedownStage {
    pub const ALL: [TakedownStage; 4] = [
        TakedownStage::Report,
        TakedownStage::Verify,
        TakedownStage::Remove,
        TakedownStage::Announce,
    ];

    pub const fn ord(self) -> u8 {
        match self {
            TakedownStage::Report => 0,
            TakedownStage::Verify => 1,
            TakedownStage::Remove => 2,
            TakedownStage::Announce => 3,
        }
    }
}

/// 下架状态机：单调四步 + 时限（举报到下架 ≤ 3 站内步）。
pub struct Takedown {
    pub stage: TakedownStage,
    /// 步计数（单调推进累计）。
    pub steps: u8,
}

impl Takedown {
    pub fn begin() -> Self {
        Takedown { stage: TakedownStage::Report, steps: 0 }
    }

    pub fn advance(&mut self, next: TakedownStage) -> Result<(), u32> {
        if next.ord() != self.stage.ord() + 1 {
            return Err(XC_MK_GOVERN_LATE);
        }
        self.steps += 1;
        self.stage = next;
        // 治理时限：到 Remove（ord=2）时步数不得超过 3。
        if self.stage.ord() == TakedownStage::Remove.ord() && self.steps > 3 {
            return Err(XC_MK_GOVERN_LATE);
        }
        Ok(())
    }

    /// 闭环判：公示后闭环（未公示即治理迟滞）。
    pub fn closed(&self) -> Result<(), u32> {
        if self.stage != TakedownStage::Announce {
            return Err(XC_MK_GOVERN_LATE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、使用统计匿名（隐私复用 F1707 白名单剥离）
// ---------------------------------------------------------------------------

/// 使用统计白名单字段闭集（之外字段剥离）。
pub const STAT_ALLOWLIST: [&str; 4] = ["model_id", "run_count", "duration_ms", "success"];

/// 匿名剥离判：仅白名单字段可入统计账。
pub fn stat_anonymous(fields: &[&str]) -> Result<(), u32> {
    for f in fields {
        let mut ok = false;
        let mut i = 0;
        while i < STAT_ALLOWLIST.len() {
            if STAT_ALLOWLIST[i] == *f {
                ok = true;
                break;
            }
            i += 1;
        }
        if !ok {
            return Err(XC_MK_PRIVACY_LEAK);
        }
    }
    if fields.is_empty() {
        return Err(XC_MK_PRIVACY_LEAK);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（五组：市场/安全/治理/激励/隐私）
// ---------------------------------------------------------------------------

/// 市场治理引擎。
pub struct MktGovEngine;

impl MktGovEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一/四（激励并入）：市场三道闸 + 激励预留位。
        let mut mkt = ModelMarket::build();
        mkt.incentive_stub_ok()?;
        mkt.list(ModelAsset { id: 1, sig: 111, review: 1, distributed: true })?;
        mkt.list(ModelAsset { id: 2, sig: 222, review: 0, distributed: false })?;
        // 反例：未过审就分发 / 拒审态越界 / 重复上架 / 分成占位越界。
        if mkt.list(ModelAsset { id: 3, sig: 333, review: 2, distributed: true }).is_ok() {
            return Err(XC_MK_GATE_MISSING);
        }
        if mkt.list(ModelAsset { id: 4, sig: 444, review: 7, distributed: false }).is_ok() {
            return Err(XC_MK_GATE_MISSING);
        }
        if mkt.list(ModelAsset { id: 1, sig: 111, review: 1, distributed: true }).is_ok() {
            return Err(XC_MK_GATE_MISSING);
        }
        let mut bad_share = ModelMarket::build();
        bad_share.creator_share_permille = 700;
        if bad_share.incentive_stub_ok().is_ok() {
            return Err(XC_MK_GATE_MISSING);
        }
        // 组二：签名——对拍过 + 篡改拒 + 零签名拒。
        let payload: [u8; 8] = [0x76, 0x61, 0x72, 0x69, 0x78, 0x00, 0x01, 0x02];
        let sig = fnv64(&payload);
        sig_verify(&payload, sig)?;
        if sig_verify(&payload, sig ^ 1).is_ok() {
            return Err(XC_MK_SIG_INVALID);
        }
        if sig_verify(&payload, 0).is_ok() {
            return Err(XC_MK_SIG_INVALID);
        }
        // 组三：治理——四步单调闭环 + 越序拒 + 迟滞拒。
        let mut td = Takedown::begin();
        td.advance(TakedownStage::Verify)?;
        td.advance(TakedownStage::Remove)?;
        td.advance(TakedownStage::Announce)?;
        td.closed()?;
        let mut jump = Takedown::begin();
        if jump.advance(TakedownStage::Remove).is_ok() {
            return Err(XC_MK_GOVERN_LATE);
        }
        let mut stall = Takedown::begin();
        stall.advance(TakedownStage::Verify)?;
        if stall.closed().is_ok() {
            return Err(XC_MK_GOVERN_LATE);
        }
        // 组五：隐私——白名单内过 + 越白名单拒 + 空账拒。
        stat_anonymous(&["model_id", "run_count"])?;
        if stat_anonymous(&["model_id", "user_id"]).is_ok() {
            return Err(XC_MK_PRIVACY_LEAK);
        }
        if stat_anonymous(&[]).is_ok() {
            return Err(XC_MK_PRIVACY_LEAK);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xA800..0xA900 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_MK_GATE_MISSING,
            XC_MK_SIG_INVALID,
            XC_MK_GOVERN_LATE,
            XC_MK_PRIVACY_LEAK,
            XC_MK_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA800 || codes[i] >= 0xA900 {
                return Err(XC_MK_GATE_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_MK_GATE_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
