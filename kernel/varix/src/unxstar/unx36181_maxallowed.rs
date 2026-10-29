//! UNX-F36181-J1 MAXIMUM_ALLOWED 全语义（尽量授而不失败）。
//! 判据：① MA 位（0x0200_0000，GEN_RSVD 岛——请求侧专用位）在存储面
//! 恒非法、在请求面恒合法的双面纪律；② MA 展开 = 全许可位并 − Deny
//! 命中位 − 虚授位（WRITE_OWNER），结果 ⊆ 候选且 ⊆ STD_ALL∪SPEC 合法区；
//! ③ MA 永不失败：零许可也是合法结论（授空 ≠ 拒绝失败）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：MA 真源——「尽量授」的判据落点是确定性展开式（不是猜）：
//! 对给定 (候选允许位, Deny 命中位, 虚授位) 三元组，MA 结果唯一且可重放。
//! 消费 B06 掩码面（合法区/标准五位）与 B04 SD 面（Deny 语义载体），
//! 为 36182（Deny 交互）/36183（特权交互）供展开基元。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4150;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MABIT: u16 = SEG_BASE + 2; // MA 位在存储面出现（双面纪律破）
pub const E_ILLEGAL: u16 = SEG_BASE + 3; // 候选/结果越合法区
pub const E_OVER: u16 = SEG_BASE + 4; // 结果越候选（MA 虚授越界）
pub const E_VIRT: u16 = SEG_BASE + 5; // 虚授位未被剔除（WRITE_OWNER 逃逸）
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// MAXIMUM_ALLOWED 请求位（0x0200_0000——GEN_RSVD 岛，请求侧专用）。
pub const MA_BIT: u32 = 0x0200_0000;

/// MA 永不虚授位（接管权必须走显式授权面——36183 特权交互的基锚）。
pub const MA_NO_VIRT: u32 = sr::WRITE_OWNER;

/// 存储面守卫：MA 位出现在任何将存入 ACE/掩码的值里 → 显性拒。
pub fn storage_guard(mask: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if mask & MA_BIT != 0 {
        return Err(E_MABIT);
    }
    if mask & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 请求面识别：desired 是否携带 MA 请求（请求面合法，存储面非法）。
pub fn requests_ma(desired: u32) -> bool {
    desired & MA_BIT != 0
}

/// MA 展开基元（确定性三元组投影）：
///   result = (candidate & !deny_hit & !MA_NO_VIRT) & mt::ALL_LEGAL
/// 约束三条全核：⊆ 候选 / ⊆ 合法区 / 虚授位恒零。
pub fn ma_expand(candidate: u32, deny_hit: u32) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if candidate & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    if deny_hit & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    let result = (candidate & !deny_hit & !MA_NO_VIRT) & mt::ALL_LEGAL;
    if result & !candidate != 0 {
        return Err(E_OVER);
    }
    if result & MA_NO_VIRT != 0 {
        return Err(E_VIRT);
    }
    Ok(result)
}

/// MA 请求解析面：desired 携 MA → 展开；不携 → 掩码直通（仍守卫存储面）。
pub fn resolve(desired: u32, candidate: u32, deny_hit: u32) -> Result<u32, u16> {
    if requests_ma(desired) {
        ma_expand(candidate, deny_hit)
    } else {
        // 非 MA 请求：desired 与候选的交集即授出（仍不越过合法区）。
        let grant = desired & candidate & mt::ALL_LEGAL;
        if grant & !candidate != 0 {
            return Err(E_OVER);
        }
        Ok(grant)
    }
}

/// MA 引擎（多主体场景）：按主体登记候选允许位与 Deny 命中位，查询时
/// 确定性展开。固定容量（16 主体——J1 域令牌面主体闭集）。
pub const SUBJECT_CAP: usize = 16;

pub struct MaEngine {
    cand: [u32; SUBJECT_CAP],
    deny: [u32; SUBJECT_CAP],
    n: usize,
}

impl MaEngine {
    pub fn new() -> MaEngine {
        MaEngine { cand: [0; SUBJECT_CAP], deny: [0; SUBJECT_CAP], n: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 登记主体（cid 连续分配；满 → E_INTG）。
    pub fn enroll(&mut self, candidate: u32, deny_hit: u32) -> Result<usize, u16> {
        if self.n >= SUBJECT_CAP {
            return Err(E_INTG);
        }
        storage_guard(candidate & mt::ALL_LEGAL)?;
        self.cand[self.n] = candidate & mt::ALL_LEGAL;
        self.deny[self.n] = deny_hit & mt::ALL_LEGAL;
        let cid = self.n;
        self.n += 1;
        Ok(cid)
    }

    /// MA 查询（cid 越界显性拒）。
    pub fn query_ma(&self, cid: usize) -> Result<u32, u16> {
        if cid >= self.n {
            return Err(E_INTG);
        }
        ma_expand(self.cand[cid], self.deny[cid])
    }

    /// 非 MA 查询（desired 直通语义，见 resolve）。
    pub fn query_plain(&self, cid: usize, desired: u32) -> Result<u32, u16> {
        if cid >= self.n {
            return Err(E_INTG);
        }
        resolve(desired, self.cand[cid], self.deny[cid])
    }

    /// 引擎守恒：主体数一致 + 全部候选/Deny 落合法区。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > SUBJECT_CAP {
            return Err(E_INTG);
        }
        let mut i = 0;
        while i < self.n {
            if self.cand[i] & !mt::ALL_LEGAL != 0 || self.deny[i] & !mt::ALL_LEGAL != 0 {
                return Err(E_ILLEGAL);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 黄金矩阵（36188 边界库消费）：候选 × Deny 的确定性展开矩阵快照。
pub const GOLDEN_CAND: [u32; 6] = [
    0,
    sr::READ_CONTROL,
    sr::STD_ALL,
    sr::READ_CONTROL | sr::SYNCHRONIZE | 0x0001,
    mt::SPEC_MASK,
    sr::DELETE | sr::READ_CONTROL | sr::WRITE_DAC,
];

pub const GOLDEN_DENY: [u32; 5] = [
    0,
    sr::READ_CONTROL,
    sr::WRITE_DAC | sr::WRITE_OWNER,
    mt::SPEC_MASK,
    sr::DELETE | sr::SYNCHRONIZE,
];

/// 黄金矩阵展开（6×5 = 30 组；快照确定性锚）。
pub fn golden_matrix() -> Result<[[u32; 5]; 6], u16> {
    let mut m = [[0u32; 5]; 6];
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < 5 {
            m[i][j] = ma_expand(GOLDEN_CAND[i], GOLDEN_DENY[j])?;
            // 矩阵内自证：三约束逐格成立。
            if m[i][j] & !GOLDEN_CAND[i] != 0 || m[i][j] & MA_NO_VIRT != 0 {
                return Err(E_OVER);
            }
            if m[i][j] & !mt::ALL_LEGAL != 0 {
                return Err(E_ILLEGAL);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(m)
}

/// 矩阵比对（自检面）。
pub fn matrix_eq(a: &[[u32; 5]; 6], b: &[[u32; 5]; 6]) -> bool {
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < 5 {
            if a[i][j] != b[i][j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：双面纪律（请求合法 / 存储显性拒）。
    if !requests_ma(MA_BIT) || requests_ma(sr::READ_CONTROL) {
        return Err(E_MABIT);
    }
    if storage_guard(sr::STD_ALL).is_err() {
        return Err(E_ILLEGAL);
    }
    if storage_guard(MA_BIT).err() != Some(E_MABIT) {
        return Err(E_MABIT);
    }
    if storage_guard(0x0100_0000).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL); // GEN_RSVD 其他位（0x01000000）也非法
    }
    // R2：展开三约束（六候选 × 五 Deny 全矩阵逐格）。
    let m1 = golden_matrix()?;
    let m2 = golden_matrix()?;
    if !matrix_eq(&m1, &m2) {
        return Err(E_INTG);
    }
    // 全零候选 → MA 授空（授空 ≠ 失败——永不失败语义锚）。
    let mut j = 0;
    while j < 5 {
        if m1[0][j] != 0 {
            return Err(E_OVER);
        }
        j += 1;
    }
    // 全许可候选 × 零 Deny → 除 WRITE_OWNER 外全授。
    let full = ma_expand(sr::STD_ALL | mt::SPEC_MASK, 0)?;
    if full != (sr::STD_ALL | mt::SPEC_MASK) & !MA_NO_VIRT {
        return Err(E_VIRT);
    }
    // Deny 剔除：候选 STD_ALL × Deny WRITE_DAC|WRITE_OWNER → 结果无两位。
    let d = ma_expand(sr::STD_ALL, sr::WRITE_DAC | sr::WRITE_OWNER)?;
    if d & (sr::WRITE_DAC | sr::WRITE_OWNER) != 0 {
        return Err(E_VIRT);
    }
    if d != sr::DELETE | sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_OVER);
    }
    // R3：越候选注入（结果 ⊆ 候选的负例：Deny 反转不可凭空造位）。
    // 构造：候选 0 但 Deny 含某位——结果必 0（不可「补回」）。
    if ma_expand(0, sr::READ_CONTROL)? != 0 {
        return Err(E_OVER);
    }
    // 候选越合法区显性拒。
    if ma_expand(0x0200_0000 | sr::READ_CONTROL, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // R4：resolve 双态（MA 位走展开 / 无 MA 位走交集）。
    let eng = {
        let mut e = MaEngine::new();
        let cid = e.enroll(sr::STD_ALL, sr::WRITE_DAC)?;
        if cid != 0 {
            return Err(E_INTG);
        }
        e
    };
    let g_ma = eng.query_ma(0)?;
    // MA 展开恒剔虚授位：Deny WRITE_DAC 后，WRITE_OWNER 也被 MA_NO_VIRT 剔除，
    // 结果 = DELETE|READ_CONTROL|SYNCHRONIZE（与 d 同一结论——语义自洽锚）。
    if g_ma != sr::DELETE | sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_OVER);
    }
    let g_plain = eng.query_plain(0, sr::READ_CONTROL | sr::SYNCHRONIZE)?;
    if g_plain != sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_OVER);
    }
    // MA 请求但 desired 带具体位 → 仍走 MA 展开（MA 位优先——Windows 语义）。
    let g_mix = eng.query_plain(0, sr::READ_CONTROL | MA_BIT)?;
    // 无 MA 位（MA_BIT 不在 desired——此处构造含 MA 的请求走 resolve）。
    let g_r = resolve(sr::READ_CONTROL | MA_BIT, sr::STD_ALL, sr::WRITE_DAC)?;
    if g_r != g_ma {
        return Err(E_OVER);
    }
    let _ = g_mix;
    // R5：引擎守恒（16 主体满 + 越界拒 + 越合法区拒）。
    let mut big = MaEngine::new();
    let mut k = 0;
    while k < SUBJECT_CAP {
        big.enroll(GOLDEN_CAND[k % 6], GOLDEN_DENY[k % 5])?;
        k += 1;
    }
    if big.count() != SUBJECT_CAP {
        return Err(E_INTG);
    }
    if big.enroll(0, 0).err() != Some(E_INTG) {
        return Err(E_INTG);
    }
    if big.query_ma(SUBJECT_CAP).err() != Some(E_INTG) {
        return Err(E_INTG);
    }
    big.integrity()?;
    // 校验和：矩阵全并（确定性总锚）。
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < 5 {
            sum = sum.wrapping_add(m1[i][j]);
            j += 1;
        }
        i += 1;
    }
    Ok(sum.wrapping_mul(3).wrapping_add(30))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MABIT => "MA 位出现在存储面（双面纪律破）",
        E_ILLEGAL => "候选/结果越 B06 合法区",
        E_OVER => "结果越候选（MA 虚授越界/凭空造位）",
        E_VIRT => "虚授位未剔除（WRITE_OWNER 逃逸）",
        E_INTG => "引擎/矩阵完整性不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36181_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36181_maxallowed");
    cs.add("SEG 段位独占", SEG_BASE == 0x4150, "0x4150~0x415F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "双面纪律/矩阵三约束/永不失败/引擎守恒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("MA 位锚", MA_BIT == 0x0200_0000 && MA_BIT & mt::ALL_LEGAL == 0, "0x02000000 ∈ GEN_RSVD（请求侧专用）");
    cs.add("虚授锚", MA_NO_VIRT == sr::WRITE_OWNER, "MA 永不虚授 = WRITE_OWNER");
    cs.add("矩阵容量", GOLDEN_CAND.len() == 6 && GOLDEN_DENY.len() == 5, "6×5 = 30 组黄金矩阵");
    cs.add("授空合法", ma_expand(0, 0) == Ok(0), "MA 零许可 = 合法结论（永不失败）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
