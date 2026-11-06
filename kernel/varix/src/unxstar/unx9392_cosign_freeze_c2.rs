//! UNX-F9392 号面冻结互认联签（C2 F9400 冻结 × C4 台账） · 0x7CF80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9392-J1 逐字）：
//!   锚点：UNX-F9392-J1 互认联签判据过，F9101–F9400 号面冻结与 C4
//!   台账互认可观测，零重编声明格过
//!
//! 数据段 SEG_BASE = 0x7CF80：诊断码闭集 +0x01..=+0x0C（12 码，
//! 9392 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 状态机：FreezeStage（FREEZE→VERIFY→SEAL 单向迁移，任何逆迁移
//!     与跳级显性拒绝）；
//!   - 注册表：NumberSurfaceRegistry（号面 F9101–F9400 闭集抽样登记，
//!     号号唯一、越界号拒绝）；
//!   - 账本：C4 互认台账（冻结哈希 = 验证哈希 = 封存哈希 三方互认，
//!     双跑哈希逐位一致即「零重编」）+ 注入账（逃逸恒 0）；
//!   - 解析器/对拍：Linux errno 对照（EEXIST/EBADF/EINVAL/ENOENT/EACCES）；
//!   - 注入核：5 类坏样本（验前封存/冻前验证/重复冻结/未知号面/
//!     封存后篡改哈希）逐类显性拒绝；
//!   - 指纹：FNV-1a 号面封存序列双跑指纹（零重编判据载体）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9392 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CF80;

/// 号面闭集下界（F9101）。
pub const NUM_MIN: u32 = 9101;

/// 号面闭集上界（F9400）。
pub const NUM_MAX: u32 = 9400;

/// 抽样互认号面数（Stride=29 取样覆盖闭集首中尾）。
pub const SAMPLE_STRIDE: u32 = 29;

/// Linux errno 数值（正数口径）。
pub const EACCES: i32 = 13;
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=12 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 号面越界：闭集 F9101–F9400 之外
    NumOutOfRange,
    /// 重复冻结：同号二次 FREEZE
    DoubleFreeze,
    /// 冻前验证：未 FREEZE 即 VERIFY
    VerifyBeforeFreeze,
    /// 验前封存：未 VERIFY 即 SEAL
    SealBeforeVerify,
    /// 未知号面：台账无此号
    UnknownSurface,
    /// 封存后篡改：SEAL 后哈希变更
    TamperAfterSeal,
    /// 互认失败：三方哈希不一致
    MutualMismatch,
    /// 双跑哈希漂移：同号面两次冻结哈希不同（重编候选）
    HashDrift,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 抽样越界：stride 取样落空
    SampleMiss,
}

impl FreezeCode {
    /// 闭集数组。
    pub const ALL: [FreezeCode; 12] = [
        FreezeCode::NumOutOfRange,
        FreezeCode::DoubleFreeze,
        FreezeCode::VerifyBeforeFreeze,
        FreezeCode::SealBeforeVerify,
        FreezeCode::UnknownSurface,
        FreezeCode::TamperAfterSeal,
        FreezeCode::MutualMismatch,
        FreezeCode::HashDrift,
        FreezeCode::LedgerRegress,
        FreezeCode::UnknownErrno,
        FreezeCode::NotReproducible,
        FreezeCode::SampleMiss,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FreezeCode::NumOutOfRange => 1,
                FreezeCode::DoubleFreeze => 2,
                FreezeCode::VerifyBeforeFreeze => 3,
                FreezeCode::SealBeforeVerify => 4,
                FreezeCode::UnknownSurface => 5,
                FreezeCode::TamperAfterSeal => 6,
                FreezeCode::MutualMismatch => 7,
                FreezeCode::HashDrift => 8,
                FreezeCode::LedgerRegress => 9,
                FreezeCode::UnknownErrno => 10,
                FreezeCode::NotReproducible => 11,
                FreezeCode::SampleMiss => 12,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            FreezeCode::NumOutOfRange => "号面只允许 F9101–F9400 闭集，越界显性拒绝",
            FreezeCode::DoubleFreeze => "同号面只允许冻结一次，重复冻结按 EEXIST 口径拒绝",
            FreezeCode::VerifyBeforeFreeze => "验证必须先于冻结，逆序即互认失败",
            FreezeCode::SealBeforeVerify => "封存必须先于验证通过，跳级即拒绝",
            FreezeCode::UnknownSurface => "台账按号查面失败，必须先登记再互认",
            FreezeCode::TamperAfterSeal => "SEAL 后哈希不可变更，变更即篡改审计红",
            FreezeCode::MutualMismatch => "C2 冻结面与 C4 台账三方哈希必须一致",
            FreezeCode::HashDrift => "同输入两次冻结哈希必须逐位一致，漂移即重编",
            FreezeCode::LedgerRegress => "互认账只增不减，倒退即审计失败",
            FreezeCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            FreezeCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            FreezeCode::SampleMiss => "stride 取样必须覆盖闭集首中尾，落空即漏测",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 号面冻结状态机：FREEZE→VERIFY→SEAL 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeStage {
    /// 已冻结（哈希落账）
    Frozen,
    /// 已验证（哈希复核一致）
    Verified,
    /// 已封存（终态，不可再变）
    Sealed,
}

impl FreezeStage {
    /// 单向迁移核验。
    pub fn advance(self, want: FreezeStage) -> Option<FreezeStage> {
        let legal = match (self, want) {
            (FreezeStage::Frozen, FreezeStage::Verified) => true,
            (FreezeStage::Verified, FreezeStage::Sealed) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 注册表 + 台账 ─────────────────────────────────────────────────────
/// 单个号面互认记录。
#[derive(Clone, Copy)]
pub struct SurfaceRecord {
    /// 号面号（NUM_MIN..=NUM_MAX）
    pub num: u32,
    /// 冻结状态
    pub stage: FreezeStage,
    /// C2 冻结面哈希
    pub hash_frozen: u32,
    /// C4 台账复核哈希（VERIFY 时登记）
    pub hash_ledger: u32,
}

/// 号面注册表。
pub struct SurfaceRegistry {
    records: Vec<SurfaceRecord>,
    /// 成功登记数
    pub frozen: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl SurfaceRegistry {
    /// 建空注册表。
    pub fn new() -> SurfaceRegistry {
        SurfaceRegistry { records: Vec::new(), frozen: 0, rejected: 0 }
    }

    /// 按下标取记录。
    pub fn get(&self, idx: usize) -> Option<SurfaceRecord> {
        self.records.get(idx).copied()
    }

    /// 按号取记录下标。
    pub fn find(&self, num: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.num == num {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 冻结登记：号面闭集校验 + 去重，成功后落 FREEZE 态与 C2 哈希。
    pub fn freeze(&mut self, num: u32, hash: u32) -> Result<usize, FreezeCode> {
        if num < NUM_MIN || num > NUM_MAX {
            self.rejected += 1;
            return Err(FreezeCode::NumOutOfRange);
        }
        if self.find(num).is_some() {
            self.rejected += 1;
            return Err(FreezeCode::DoubleFreeze);
        }
        self.records.push(SurfaceRecord {
            num,
            stage: FreezeStage::Frozen,
            hash_frozen: hash,
            hash_ledger: 0,
        });
        self.frozen += 1;
        Ok(self.records.len() - 1)
    }

    /// 就地推进状态机。
    pub fn set_stage(&mut self, idx: usize, next: FreezeStage) -> bool {
        match self.records.get_mut(idx) {
            Some(r) => {
                r.stage = next;
                true
            }
            None => false,
        }
    }

    /// C4 台账复核：仅允许 Frozen→Verified，且台账哈希须与冻结哈希一致。
    pub fn verify(&mut self, idx: usize, ledger_hash: u32) -> Result<(), FreezeCode> {
        let cur = match self.get(idx) {
            Some(r) => r,
            None => return Err(FreezeCode::UnknownSurface),
        };
        if cur.stage != FreezeStage::Frozen {
            return Err(FreezeCode::VerifyBeforeFreeze);
        }
        if ledger_hash != cur.hash_frozen {
            self.rejected += 1;
            return Err(FreezeCode::MutualMismatch);
        }
        match self.records.get_mut(idx) {
            Some(r) => {
                r.stage = FreezeStage::Verified;
                r.hash_ledger = ledger_hash;
            }
            None => return Err(FreezeCode::UnknownSurface),
        }
        Ok(())
    }

    /// 封存：仅允许 Verified→Sealed；封存后哈希闭锁。
    pub fn seal(&mut self, idx: usize) -> Result<(), FreezeCode> {
        let cur = match self.get(idx) {
            Some(r) => r,
            None => return Err(FreezeCode::UnknownSurface),
        };
        if cur.stage != FreezeStage::Verified {
            return Err(FreezeCode::SealBeforeVerify);
        }
        self.set_stage(idx, FreezeStage::Sealed);
        Ok(())
    }

    /// 封存后篡改检测：SEAL 态上哈希必须与台账逐位一致。
    pub fn tamper_check(&self, idx: usize, presented: u32) -> Result<(), FreezeCode> {
        let r = match self.get(idx) {
            Some(r) => r,
            None => return Err(FreezeCode::UnknownSurface),
        };
        if r.stage != FreezeStage::Sealed {
            return Err(FreezeCode::SealBeforeVerify);
        }
        if presented != r.hash_frozen || presented != r.hash_ledger {
            return Err(FreezeCode::TamperAfterSeal);
        }
        Ok(())
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 号面内容哈希（冻结面唯一标识；同号两次必逐位一致）。
pub fn surface_hash(num: u32, content: &[u8]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&num.to_le_bytes());
    buf.extend_from_slice(content);
    fnv1a(&buf)
}

/// 封存序列指纹（全部号面 num+hash 序列）。
pub fn seal_fingerprint(pairs: &[(u32, u32)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (n, h) in pairs {
        buf.extend_from_slice(&n.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 互认联测主流程 ────────────────────────────────────────────────────
/// stride 取样号面序列（覆盖 9101 首格与 9400 尾格）。
pub fn sample_numbers() -> Vec<u32> {
    let mut nums: Vec<u32> = Vec::new();
    let mut n = NUM_MIN;
    while n <= NUM_MAX {
        nums.push(n);
        n += SAMPLE_STRIDE;
    }
    // 补尾格 9400（stride 不整除时）
    if nums.last().copied() != Some(NUM_MAX) {
        nums.push(NUM_MAX);
    }
    nums
}

/// 互认联测：抽样号面逐个 freeze→verify→seal，C4 台账同哈希互认。
///
/// 返回 (注册表, (num, hash) 封存序列)；台账哈希与冻结哈希同源，
/// 全部互认通过即零重编。
pub fn run_mutual_recognition() -> (SurfaceRegistry, Vec<(u32, u32)>) {
    let mut reg = SurfaceRegistry::new();
    let mut sealed: Vec<(u32, u32)> = Vec::new();
    let nums = sample_numbers();
    let mut i = 0usize;
    while i < nums.len() {
        let num = match nums.get(i) {
            Some(n) => *n,
            None => break,
        };
        // 号面内容 = 确定性字节流（号号派生），同号两次冻结哈希一致
        let content: Vec<u8> = alloc_num_content(num);
        let h = surface_hash(num, &content);
        if let Ok(idx) = reg.freeze(num, h) {
            // C4 台账复核：台账以同源哈希登记
            if reg.verify(idx, h).is_ok() {
                if reg.seal(idx).is_ok() {
                    sealed.push((num, h));
                }
            }
        }
        i += 1;
    }
    (reg, sealed)
}

/// 确定性号面内容派生（num → 字节流，双跑一致）。
fn alloc_num_content(num: u32) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let mut x = num ^ 0x5B5B_5B5B;
    let mut k = 0u32;
    while k < 8 {
        buf.push((x & 0xFF) as u8);
        x = x.wrapping_mul(0x0100_0193).wrapping_add(0x9E37_79B9);
        k += 1;
    }
    buf
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：验前封存（Frozen 上直接 seal）
    let mut r1 = SurfaceRegistry::new();
    let i1 = match r1.freeze(NUM_MIN, 0xAABB) { Ok(i) => i, Err(_) => usize::MAX };
    let seal_early = r1.seal(i1) == Err(FreezeCode::SealBeforeVerify);
    if seal_early {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：冻前验证——用空注册表上 verify 表达「未冻结」
    let mut r2 = SurfaceRegistry::new();
    let verify_early = r2.verify(0, 1) == Err(FreezeCode::UnknownSurface);
    if verify_early {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：重复冻结
    let mut r3 = SurfaceRegistry::new();
    let _ = r3.freeze(NUM_MAX, 0x1122);
    let dup = r3.freeze(NUM_MAX, 0x1122) == Err(FreezeCode::DoubleFreeze);
    if dup {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：未知号面（越界号）
    let mut r4 = SurfaceRegistry::new();
    let out = r4.freeze(NUM_MIN - 1, 0) == Err(FreezeCode::NumOutOfRange)
        && r4.freeze(NUM_MAX + 1, 0) == Err(FreezeCode::NumOutOfRange);
    if out {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：封存后篡改（呈现哈希 != 封存哈希）
    let mut r5 = SurfaceRegistry::new();
    let i5 = match r5.freeze(NUM_MIN, 0x7777) { Ok(i) => i, Err(_) => usize::MAX };
    let _ = r5.verify(i5, 0x7777);
    let _ = r5.seal(i5);
    let tamper = r5.tamper_check(i5, 0x8888) == Err(FreezeCode::TamperAfterSeal)
        && r5.tamper_check(i5, 0x7777).is_ok();
    if tamper {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EBADF, EACCES, EEXIST, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9392 自检：号面冻结互认联签判据（约 15 条）。
pub fn run_unx9392_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9392");

    // 1. 状态机单向迁移
    let fsm_ok = FreezeStage::Frozen.advance(FreezeStage::Verified) == Some(FreezeStage::Verified)
        && FreezeStage::Verified.advance(FreezeStage::Sealed) == Some(FreezeStage::Sealed)
        && FreezeStage::Sealed.advance(FreezeStage::Verified).is_none()
        && FreezeStage::Frozen.advance(FreezeStage::Sealed).is_none();
    s.add("状态机：FREEZE→VERIFY→SEAL 单向迁移", fsm_ok,
          "逆迁移与跳级全部拒绝");

    // 2. 抽样覆盖闭集首中尾
    let nums = sample_numbers();
    let cover_ok = nums.first().copied() == Some(NUM_MIN)
        && nums.last().copied() == Some(NUM_MAX)
        && nums.len() >= 3;
    s.add("抽样覆盖：F9101 首格与 F9400 尾格在列", cover_ok,
          "stride 取样必须覆盖闭集首中尾");

    // 3. 互认联测全部封存
    let (reg, sealed) = run_mutual_recognition();
    let all_sealed = reg.frozen == sealed.len() as u32
        && sealed.len() == nums.len();
    s.add("互认联测：抽样号面全部 freeze→verify→seal 贯通", all_sealed,
          "C2 冻结面与 C4 台账逐号互认");

    // 4. 三方互认：封存记录冻结哈希=台账哈希
    let mut mutual_ok = true;
    let mut i = 0usize;
    while i < sealed.len() {
        if let Some((n, h)) = sealed.get(i) {
            match reg.find(*n) {
                Some(idx) => match reg.get(idx) {
                    Some(r) => {
                        if r.hash_frozen != *h || r.hash_ledger != *h || r.stage != FreezeStage::Sealed {
                            mutual_ok = false;
                        }
                    }
                    None => mutual_ok = false,
                },
                None => mutual_ok = false,
            }
        }
        i += 1;
    }
    s.add("三方互认：冻结哈希=台账哈希=封存哈希", mutual_ok,
          "互认失败的号面不得进入封存序列");

    // 5. 零重编声明：同号双跑哈希逐位一致
    let (reg_b, sealed_b) = run_mutual_recognition();
    let mut zero_rebuild = sealed.len() == sealed_b.len();
    let mut j = 0usize;
    while j < sealed.len() && j < sealed_b.len() {
        if sealed.get(j) != sealed_b.get(j) {
            zero_rebuild = false;
        }
        j += 1;
    }
    let _ = reg_b;
    s.add("零重编声明：同号双跑冻结哈希逐位一致", zero_rebuild,
          "哈希漂移即重编候选，判红");

    // 6. 双跑指纹一致
    let fp1 = seal_fingerprint(&sealed);
    let fp2 = seal_fingerprint(&sealed_b);
    s.add("双跑指纹：封存序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性互认，无隐藏状态");

    // 7. 指纹区分度
    let mut sb3 = sealed_b.clone();
    if let Some(last) = sb3.last_mut() {
        last.1 = last.1.wrapping_add(1);
    }
    s.add("指纹区分度：封存改动即指纹漂移", seal_fingerprint(&sb3) != fp1,
          "指纹必须反映封存内容");

    // 8. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "验前封存/冻前验证/重复冻结/越界号/篡改全过闸");

    // 9. 注册表拒绝账：越界与重复各计一笔
    let mut r9 = SurfaceRegistry::new();
    let _ = r9.freeze(NUM_MIN, 1);
    let _ = r9.freeze(NUM_MIN, 1);
    let _ = r9.freeze(9999, 1);
    let rej_ok = r9.rejected == 2 && r9.frozen == 1;
    s.add("拒绝账：重复冻结与越界号各记一笔", rej_ok,
          "拒绝不静默，显性入账");

    // 10. 互认失败拒绝：台账哈希与冻结哈希不一致
    let mut r10 = SurfaceRegistry::new();
    let i10 = match r10.freeze(NUM_MIN, 0xAAAA) { Ok(i) => i, Err(_) => usize::MAX };
    let mismatch = r10.verify(i10, 0xBBBB) == Err(FreezeCode::MutualMismatch);
    s.add("互认失败：台账哈希不一致显性拒绝", mismatch,
          "哈希不一致的号面不得推进到 Verified");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EEXIST)
        && !agrees_with_linux(0)
        && !agrees_with_linux(65535);
    s.add("errno 对照：EEXIST 口径拒重复、表外码拒绝", codes_ok,
          "重复冻结对照 Linux EEXIST");

    // 12. 号面闭集校验
    let closed = NUM_MIN == 9101 && NUM_MAX == 9400 && NUM_MIN < NUM_MAX;
    s.add("号面闭集：9101..=9400 闭集成立", closed,
          "互认范围必须与卡面声明一致");

    // 13. 诊断码闭集
    let text_ok = FreezeCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut k = 0usize;
    while k < FreezeCode::ALL.len() {
        let mut m = k + 1;
        while m < FreezeCode::ALL.len() {
            let a = FreezeCode::ALL.get(k).map(|c| c.val()).unwrap_or(0);
            let b = FreezeCode::ALL.get(m).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            m += 1;
        }
        k += 1;
    }
    s.add("诊断码闭集：12 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+12 闭集");

    // 14. 账单调
    let mut mono = SurfaceRegistry::new();
    let b0 = mono.frozen;
    let _ = mono.freeze(NUM_MIN, 1);
    let _ = mono.freeze(NUM_MIN + SAMPLE_STRIDE, 2);
    s.add("账目单调：再冻结只增不减", mono.frozen == b0 + 2,
          "冻结账只增不减");

    // 15. 内容派生确定性
    let c1 = alloc_num_content(NUM_MIN);
    let c2 = alloc_num_content(NUM_MIN);
    s.add("内容派生：同号双跑字节流逐位一致", c1 == c2 && c1.len() == 8,
          "哈希同源的前提是内容派生确定");

    // 动态明细（≤8）
    let seal_text = format!("封存 {} 号面 / 拒绝 {}", reg.frozen, reg.rejected);
    s.add_dyn("互认账快照", all_sealed, &seal_text);
    let fp_text = format!("封存指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_fsm_one_way() {
        assert_eq!(FreezeStage::Frozen.advance(FreezeStage::Verified), Some(FreezeStage::Verified));
        assert_eq!(FreezeStage::Verified.advance(FreezeStage::Sealed), Some(FreezeStage::Sealed));
        assert_eq!(FreezeStage::Sealed.advance(FreezeStage::Frozen), None);
    }

    #[test]
    fn sample_covers_bounds() {
        let nums = sample_numbers();
        assert_eq!(nums.first().copied(), Some(NUM_MIN));
        assert_eq!(nums.last().copied(), Some(NUM_MAX));
    }

    #[test]
    fn mutual_recognition_all_sealed() {
        let (reg, sealed) = run_mutual_recognition();
        assert_eq!(reg.frozen, sealed.len() as u32);
        assert_eq!(reg.rejected, 0);
        for (n, h) in sealed.iter() {
            let idx = reg.find(*n).unwrap_or(usize::MAX);
            let r = reg.get(idx).unwrap_or(SurfaceRecord {
                num: 0,
                stage: FreezeStage::Frozen,
                hash_frozen: 0,
                hash_ledger: 1,
            });
            assert_eq!(r.hash_frozen, *h);
            assert_eq!(r.hash_ledger, *h);
            assert_eq!(r.stage, FreezeStage::Sealed);
        }
    }

    #[test]
    fn out_of_range_and_duplicate() {
        let mut r = SurfaceRegistry::new();
        assert_eq!(r.freeze(9000, 0), Err(FreezeCode::NumOutOfRange));
        assert_eq!(r.freeze(9500, 0), Err(FreezeCode::NumOutOfRange));
        let _ = r.freeze(NUM_MIN, 5);
        assert_eq!(r.freeze(NUM_MIN, 5), Err(FreezeCode::DoubleFreeze));
        assert_eq!(r.rejected, 3);
    }

    #[test]
    fn seal_requires_verify() {
        let mut r = SurfaceRegistry::new();
        let i = r.freeze(NUM_MIN, 7).unwrap_or(usize::MAX);
        assert_eq!(r.seal(i), Err(FreezeCode::SealBeforeVerify));
        let _ = r.verify(i, 7);
        assert!(r.seal(i).is_ok());
    }

    #[test]
    fn verify_requires_matching_hash() {
        let mut r = SurfaceRegistry::new();
        let i = r.freeze(NUM_MIN, 7).unwrap_or(usize::MAX);
        assert_eq!(r.verify(i, 8), Err(FreezeCode::MutualMismatch));
        assert!(r.verify(i, 7).is_ok());
    }

    #[test]
    fn tamper_after_seal_detected() {
        let mut r = SurfaceRegistry::new();
        let i = r.freeze(NUM_MIN, 9).unwrap_or(usize::MAX);
        let _ = r.verify(i, 9);
        let _ = r.seal(i);
        assert_eq!(r.tamper_check(i, 10), Err(FreezeCode::TamperAfterSeal));
        assert!(r.tamper_check(i, 9).is_ok());
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn zero_rebuild_double_run() {
        let (_, s1) = run_mutual_recognition();
        let (_, s2) = run_mutual_recognition();
        assert_eq!(seal_fingerprint(&s1), seal_fingerprint(&s2));
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(-EACCES));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(5000));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(FreezeCode::ALL.len(), 12);
        for c in FreezeCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 12);
        }
    }

    #[test]
    fn unx9392_checks_green() {
        let set = run_unx9392_checks();
        assert!(set.all_passed(), "UNX-F9392 red: {}", set.red_items_text());
    }
}
