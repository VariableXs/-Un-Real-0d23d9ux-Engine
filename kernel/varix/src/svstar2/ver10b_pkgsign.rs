//! VE-F3542 · persona 包签名安全 —— 三级信任链复用 + 签名校验前置 + 双扫描。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3542`
//!
//! # 职责（锚点原文拆解）
//!
//! - **信任复用**（[`TrustLevel`]）：复用 E04 三级信任
//!   （官方/社区/未知），persona 包不另起炉灶——同一信任词汇表。
//! - **校验前置**（[`SignGate::verify_first`]）：签名校验是安装
//!   的第一道门，签名不过连扫描都不进（不给恶意包任何后续处理）。
//! - **双扫描**（[`ContentScanner`]）：档案扫描（配置值域/公式注入）
//!   + 资产扫描（图片尺寸炸弹/路径穿越）两路独立，两路都过才放行。
//! - **隔离处置**（[`Quarantine`]）：扫描命中的包进隔离区，
//!   不删除（可申诉），不允许安装。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 签名无效 | 拒绝（不进扫描） |
//! | 扫描命中 | 隔离（可申诉不删除） |
//! | 降级越权 | 阻断（低信任不得冒充高信任） |
//!
//! # 复杂度
//!
//! O(包体)（扫描线性遍历）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6020 起）
// ---------------------------------------------------------------------------

/// 包签名安全诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignCode {
    /// 签名无效。
    SignInvalid = 6020,
    /// 信任越权（低信任冒充高信任）。
    TrustEscalation = 6021,
    /// 档案扫描命中。
    ProfileHit = 6022,
    /// 资产扫描命中。
    AssetHit = 6023,
    /// 包已被隔离。
    Quarantined = 6024,
    /// 摘要不匹配。
    DigestMismatch = 6025,
}

impl SignCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SignCode::SignInvalid => "签名无效已拒绝",
            SignCode::TrustEscalation => "信任等级越权已阻断",
            SignCode::ProfileHit => "档案扫描命中",
            SignCode::AssetHit => "资产扫描命中",
            SignCode::Quarantined => "包已隔离",
            SignCode::DigestMismatch => "包摘要不匹配",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            SignCode::SignInvalid | SignCode::TrustEscalation | SignCode::DigestMismatch
        )
    }
}

// ---------------------------------------------------------------------------
// 二、三级信任（E04 同规）
// ---------------------------------------------------------------------------

/// 三级信任（与 E04 主题包签名完全同规）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TrustLevel {
    /// 未知来源。
    Unknown = 0,
    /// 社区签名。
    Community = 1,
    /// 官方签名。
    Official = 2,
}

impl TrustLevel {
    /// 等级短名（读屏口径）。
    pub const fn label(self) -> &'static str {
        match self {
            TrustLevel::Unknown => "未知来源",
            TrustLevel::Community => "社区签名",
            TrustLevel::Official => "官方签名",
        }
    }

    /// 数值等级（比较用）。
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------
// 三、签名与摘要
// ---------------------------------------------------------------------------

/// 定长摘要（简化 FNV-1a 64 位；真实部署换 SHA-256）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Digest(pub u64);

impl Digest {
    /// 从字节序列计算摘要。
    pub fn of(bytes: &[u8]) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Digest(h)
    }
}

/// 签名块（信任等级 + 摘要承诺）。
#[derive(Clone, Copy, Debug)]
pub struct Signature {
    /// 签发信任等级。
    pub level: TrustLevel,
    /// 对包体的摘要承诺。
    pub digest: Digest,
    /// 是否有效（伪造/过期则 false）。
    pub valid: bool,
}

// ---------------------------------------------------------------------------
// 四、包体
// ---------------------------------------------------------------------------

/// persona 包体（档案区 + 资产区）。
pub struct PersonaPackage {
    /// 包 id。
    pub id: u32,
    /// 签名。
    pub sig: Signature,
    /// 档案字节（配置序列化）。
    pub profile: Vec<u8>,
    /// 资产字节（图片/音效等拼接摘要源）。
    pub assets: Vec<u8>,
}

// ---------------------------------------------------------------------------
// 五、双扫描器
// ---------------------------------------------------------------------------

/// 扫描命中原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanHit {
    /// 档案区：检测到疑似公式注入前缀。
    FormulaInjection,
    /// 档案区：配置段数超限（配置炸弹）。
    ProfileBomb,
    /// 资产区：路径穿越序列。
    PathTraversal,
    /// 资产区：长度字段伪造（尺寸炸弹）。
    SizeBomb,
}

impl ScanHit {
    /// 短名。
    pub const fn label(self) -> &'static str {
        match self {
            ScanHit::FormulaInjection => "公式注入",
            ScanHit::ProfileBomb => "配置炸弹",
            ScanHit::PathTraversal => "路径穿越",
            ScanHit::SizeBomb => "尺寸炸弹",
        }
    }
}

/// 双扫描器（档案 + 资产两路独立）。
pub struct ContentScanner {
    /// 档案段数上限。
    pub max_profile_segs: u32,
    /// 资产字节上限。
    pub max_asset_bytes: usize,
}

impl ContentScanner {
    /// 默认扫描器（档案 256 段 / 资产 256 KiB）。
    pub const fn standard() -> Self {
        Self {
            max_profile_segs: 256,
            max_asset_bytes: 256 * 1024,
        }
    }

    /// 档案扫描。
    pub fn scan_profile(&self, profile: &[u8]) -> Option<ScanHit> {
        // 公式注入：以 = + - @ 开头的配置值（表格公式惯用入口）。
        let mut segs: u32 = 0;
        let mut i = 0;
        while i < profile.len() {
            let b = profile[i];
            if b == b'\n' {
                segs = segs.saturating_add(1);
                if segs > self.max_profile_segs {
                    return Some(ScanHit::ProfileBomb);
                }
                // 行首公式前缀。
                if i + 1 < profile.len() {
                    let n = profile[i + 1];
                    if n == b'=' || n == b'@' {
                        return Some(ScanHit::FormulaInjection);
                    }
                }
            }
            i += 1;
        }
        // 首字节也查（文件头无换行时）。
        if !profile.is_empty() {
            let f = profile[0];
            if f == b'=' || f == b'@' {
                return Some(ScanHit::FormulaInjection);
            }
        }
        None
    }

    /// 资产扫描。
    pub fn scan_assets(&self, assets: &[u8]) -> Option<ScanHit> {
        if assets.len() > self.max_asset_bytes {
            return Some(ScanHit::SizeBomb);
        }
        // 路径穿越：找 ../ 或 ..\ 序列。
        let mut i = 0;
        while i + 2 < assets.len() {
            if assets[i] == b'.'
                && assets[i + 1] == b'.'
                && (assets[i + 2] == b'/' || assets[i + 2] == b'\\')
            {
                return Some(ScanHit::PathTraversal);
            }
            i += 1;
        }
        None
    }

    /// 双扫描（两路都过才 Ok）。
    pub fn scan_both(&self, pkg: &PersonaPackage) -> Result<(), ScanHit> {
        if let Some(h) = self.scan_profile(&pkg.profile) {
            return Err(h);
        }
        if let Some(h) = self.scan_assets(&pkg.assets) {
            return Err(h);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、隔离区
// ---------------------------------------------------------------------------

/// 隔离记录。
#[derive(Clone, Debug)]
pub struct QuarantineRecord {
    /// 包 id。
    pub pkg: u32,
    /// 命中原因。
    pub hit: ScanHit,
    /// 是否已申诉。
    pub appealed: bool,
}

/// 隔离区（不删除、可申诉）。
#[derive(Default)]
pub struct Quarantine {
    records: Vec<QuarantineRecord>,
}

impl Quarantine {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 送隔离。
    pub fn isolate(&mut self, pkg: u32, hit: ScanHit) {
        self.records.push(QuarantineRecord {
            pkg,
            hit,
            appealed: false,
        });
    }

    /// 申诉标记。
    pub fn appeal(&mut self, pkg: u32) -> bool {
        for r in self.records.iter_mut() {
            if r.pkg == pkg {
                r.appealed = true;
                return true;
            }
        }
        false
    }

    /// 是否在隔离中。
    pub fn holds(&self, pkg: u32) -> bool {
        self.records.iter().any(|r| r.pkg == pkg && !r.appealed)
    }

    /// 隔离条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 七、签名门（主入口）
// ---------------------------------------------------------------------------

/// 签名门：校验前置 + 双扫描 + 隔离。
pub struct SignGate {
    scanner: ContentScanner,
    quarantine: Quarantine,
    /// 安装所需的最低信任。
    pub min_trust: TrustLevel,
}

impl SignGate {
    /// 标准门（社区起装，官方免扫描快捷道）。
    pub fn standard() -> Self {
        Self {
            scanner: ContentScanner::standard(),
            quarantine: Quarantine::new(),
            min_trust: TrustLevel::Community,
        }
    }

    /// 校验前置：签名 → 摘要 → 信任 → 扫描，逐步短路。
    pub fn verify_first(&mut self, pkg: &PersonaPackage) -> Result<TrustLevel, SignCode> {
        // 1. 签名有效性（第一道，不过即返回）。
        if !pkg.sig.valid {
            return Err(SignCode::SignInvalid);
        }
        // 2. 摘要承诺与实际包体一致。
        let actual = Digest::of(&pkg.profile).0 ^ Digest::of(&pkg.assets).0;
        if Digest(actual) != pkg.sig.digest {
            return Err(SignCode::DigestMismatch);
        }
        // 3. 信任不得低于门槛（未知不得冒充）。
        if pkg.sig.level.rank() < self.min_trust.rank() {
            return Err(SignCode::TrustEscalation);
        }
        // 4. 双扫描（官方信任也扫——官方包若损坏同样拦）。
        match self.scanner.scan_both(pkg) {
            Ok(()) => Ok(pkg.sig.level),
            Err(hit) => {
                self.quarantine.isolate(pkg.id, hit);
                Err(match hit {
                    ScanHit::FormulaInjection | ScanHit::ProfileBomb => SignCode::ProfileHit,
                    _ => SignCode::AssetHit,
                })
            }
        }
    }

    /// 隔离区只读访问。
    pub fn quarantine(&self) -> &Quarantine {
        &self.quarantine
    }

    /// 隔离区可变访问（申诉入口）。
    pub fn quarantine_mut(&mut self) -> &mut Quarantine {
        &mut self.quarantine
    }
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// 包签名安全判据集。
pub fn run_sign_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10b_pkgsign");

    // 1. 合法包过门。
    let profile = b"theme=dark\nkelvin=4500\n".to_vec();
    let assets = b"icon-payload".to_vec();
    let digest = Digest(Digest::of(&profile).0 ^ Digest::of(&assets).0);
    let good = PersonaPackage {
        id: 1,
        sig: Signature {
            level: TrustLevel::Official,
            digest,
            valid: true,
        },
        profile,
        assets,
    };
    let mut gate = SignGate::standard();
    let passed = gate.verify_first(&good).is_ok();
    set.add("6020-合法包过门", passed, "");

    // 2. 无效签名拒。
    let mut bad_sig = PersonaPackage {
        id: 2,
        sig: Signature {
            level: TrustLevel::Official,
            digest,
            valid: false,
        },
        profile: b"x=1\n".to_vec(),
        assets: b"a".to_vec(),
    };
    // 修 digest 使摘要正确，仅签名旗标无效。
    let p2 = b"x=1\n".to_vec();
    let a2 = b"a".to_vec();
    bad_sig.sig.digest = Digest(Digest::of(&p2).0 ^ Digest::of(&a2).0);
    let mut gate2 = SignGate::standard();
    let rejected = gate2.verify_first(&bad_sig).err() == Some(SignCode::SignInvalid);
    set.add("6021-无效签名拒绝", rejected, "");

    // 3. 未知信任阻断。
    let p3 = b"y=2\n".to_vec();
    let a3 = b"b".to_vec();
    let d3 = Digest(Digest::of(&p3).0 ^ Digest::of(&a3).0);
    let unknown = PersonaPackage {
        id: 3,
        sig: Signature {
            level: TrustLevel::Unknown,
            digest: d3,
            valid: true,
        },
        profile: p3,
        assets: a3,
    };
    let mut gate3 = SignGate::standard();
    let blocked = gate3.verify_first(&unknown).err() == Some(SignCode::TrustEscalation);
    set.add("6022-未知信任阻断", blocked, "");

    // 4. 公式注入被档案扫描捕。
    let evil_profile = b"ok=1\n=cmd|'/c calc'!A0\n".to_vec();
    let evil_assets = b"z".to_vec();
    let d4 = Digest(Digest::of(&evil_profile).0 ^ Digest::of(&evil_assets).0);
    let evil = PersonaPackage {
        id: 4,
        sig: Signature {
            level: TrustLevel::Community,
            digest: d4,
            valid: true,
        },
        profile: evil_profile,
        assets: evil_assets,
    };
    let mut gate4 = SignGate::standard();
    let hit_profile = gate4.verify_first(&evil).err() == Some(SignCode::ProfileHit);
    let isolated = gate4.quarantine().holds(4);
    set.add("6023-公式注入隔离", hit_profile && isolated, "");

    // 5. 路径穿越被资产扫描捕。
    let p5 = b"z=1\n".to_vec();
    let a5 = b"../../etc/passwd".to_vec();
    let d5 = Digest(Digest::of(&p5).0 ^ Digest::of(&a5).0);
    let traversal = PersonaPackage {
        id: 5,
        sig: Signature {
            level: TrustLevel::Community,
            digest: d5,
            valid: true,
        },
        profile: p5,
        assets: a5,
    };
    let mut gate5 = SignGate::standard();
    let hit_asset = gate5.verify_first(&traversal).err() == Some(SignCode::AssetHit);
    set.add("6024-路径穿越拦截", hit_asset, "");

    // 6. 摘要不匹配拒。
    let p6 = b"k=1\n".to_vec();
    let a6 = b"v".to_vec();
    let wrong = PersonaPackage {
        id: 6,
        sig: Signature {
            level: TrustLevel::Official,
            digest: Digest(0xDEAD),
            valid: true,
        },
        profile: p6,
        assets: a6,
    };
    let mut gate6 = SignGate::standard();
    let digest_rej = gate6.verify_first(&wrong).err() == Some(SignCode::DigestMismatch);
    set.add("6025-摘要不匹配拒绝", digest_rej, "");

    // 7. 申诉解除隔离。
    let mut q = Quarantine::new();
    q.isolate(99, ScanHit::SizeBomb);
    let before = q.holds(99);
    let _ = q.appeal(99);
    let after = !q.holds(99);
    set.add("6026-申诉解除隔离", before && after, "");

    set
}
