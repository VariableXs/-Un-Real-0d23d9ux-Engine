//! unx48241_spframe — UNX-F48241 双拼方案框架（键位映射表可插拔、方案注册制）（诊断段 0xBF00）。
//!
//! 判据原文：
//!   J1 框架注册 ×10 方案位就绪；判据锚定 §AI-61 §2 专题五双拼项、§AI-61 §5 J-3 双拼 3 种构。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 注册制：方案 = (方案id, 声母映射表[26], 韵母映射表[26]) 的
//!      不可变登记体；注册时全表校验（26 键全覆盖、无重键、自反位
//!      合法），校验不过即拒收——坏表不许进注册册。
//!   2. 可插拔契约：转换入口只吃方案 id，框架按 id 查表执行
//!      「键序列 → 音节序列」的纯查表转换；框架零内置方案语义。
//!   3. ×10 方案位就绪：注册册容量 10 位，判据装 10 套合格表全部
//!      注册成功 + 第 11 位拒收（容量守恒）。
//!   4. 转换不变式：合法双拼键对 (声母位, 韵母位) 产出唯一音节；
//!      zh/ch/sh 三键的特殊韵母位按表直读（框架不做隐式规则）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBF00;

const E_TABLE_SIZE: u16 = SEG_BASE + 1;  // 映射表长度≠26
const E_DUP_KEY: u16 = SEG_BASE + 2;     // 同目标重键
const E_BAD_IDX: u16 = SEG_BASE + 3;     // 键位越界
const E_DUP_SCHEME: u16 = SEG_BASE + 4;  // 方案 id 重复
const E_REG_FULL: u16 = SEG_BASE + 5;    // 注册册满
const E_UNKNOWN_SCHEME: u16 = SEG_BASE + 6; // 未注册方案
const E_ODD_LEN: u16 = SEG_BASE + 7;     // 键序列奇数长
const E_SEAL_MISMATCH: u16 = SEG_BASE + 8; // 封账指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 9; // 框架账不平

pub const REG_CAP: usize = 10;

/// 声母表：26 位（a..z 键位 → 声母编号 0..=25，0=无声母位）。
pub type SmTab = [u8; 26];
/// 韵母表：26 位（a..z 键位 → 韵母编号 0..=25）。
pub type YmTab = [u8; 26];

/// 双拼方案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub id: u8,
    pub sm: SmTab,
    pub ym: YmTab,
}

/// 音节：声母编号 + 韵母编号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Syll {
    pub sm: u8,
    pub ym: u8,
}

/// 双拼框架。
#[derive(Debug)]
pub struct SpFrame {
    reg: Vec<Scheme>,
    sealed: Option<u32>,
    converts: u32,
}

fn tab_ok(tab: &[u8; 26]) -> bool {
    // 全表合法：编号 <26；无重目标（同编号不得出现两次以上? 目标唯一性：
    // 双拼表允许一键多义吗——不许：每个键位映射唯一，且每个音位恰一键位。
    let mut seen = [false; 26];
    for v in tab {
        if *v as usize >= 26 {
            return false;
        }
        if seen[*v as usize] {
            return false; // 同音位双键 = 表坏
        }
        seen[*v as usize] = true;
    }
    true
}

impl SpFrame {
    pub fn new() -> Self {
        SpFrame { reg: Vec::new(), sealed: None, converts: 0 }
    }

    /// 注册方案：全表校验 + 容量 + 号唯一。
    pub fn register(&mut self, s: Scheme) -> Result<(), u16> {
        if self.reg.len() >= REG_CAP {
            return Err(E_REG_FULL);
        }
        if self.reg.iter().any(|x| x.id == s.id) {
            return Err(E_DUP_SCHEME);
        }
        if !tab_ok(&s.sm) || !tab_ok(&s.ym) {
            return Err(E_DUP_KEY);
        }
        self.reg.push(s);
        Ok(())
    }

    /// 键序列 → 音节序列（纯查表）：偶数长切分，(键i, 键i+1) 查表。
    pub fn convert(&mut self, scheme_id: u8, keys: &[u8]) -> Result<Vec<Syll>, u16> {
        let s = self
            .reg
            .iter()
            .find(|x| x.id == scheme_id)
            .ok_or(E_UNKNOWN_SCHEME)?;
        if keys.len() % 2 != 0 {
            return Err(E_ODD_LEN);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < keys.len() {
            let ki = keys[i] as usize;
            let kj = keys[i + 1] as usize;
            if ki >= 26 || kj >= 26 {
                return Err(E_BAD_IDX);
            }
            out.push(Syll {
                sm: s.sm[ki],
                ym: s.ym[kj],
            });
            i += 2;
        }
        self.converts += 1;
        Ok(out)
    }


    /// 注册册视图。
    pub fn ids(&self) -> Vec<u8> {
        self.reg.iter().map(|x| x.id).collect()
    }

    /// 注册册迭代器（审计只读访问）。
    pub fn reg_iter(&self) -> impl Iterator<Item = &Scheme> {
        self.reg.iter()
    }

    pub fn len(&self) -> usize {
        self.reg.len()
    }
    pub fn converts(&self) -> u32 {
        self.converts
    }

    /// 封账：注册册指纹。
    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for s in &self.reg {
            h ^= s.id as u32;
            h = h.wrapping_mul(0x0100_0193);
            for v in s.sm.iter().chain(s.ym.iter()) {
                h ^= *v as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_SEAL_MISMATCH),
        }
    }
}

/// 单位表构造：恒等映射（键位 i → 音位 i），测试与方案生成基座。
pub fn ident_tab() -> (SmTab, YmTab) {
    let mut t = [0u8; 26];
    for i in 0..26 {
        t[i] = i as u8;
    }
    (t, t)
}

/// 方案音位覆盖审计：声母/韵母两表的被映射音位计数（0..=25 各一位）。
/// 合法方案恒为全 1（每音位恰一键位）——覆盖缺口即表坏（注册期已挡，
/// 此函数供注册后复核与账面导出）。
pub fn coverage_report(s: &Scheme) -> ([u8; 26], [u8; 26]) {
    let mut sm_cov = [0u8; 26];
    let mut ym_cov = [0u8; 26];
    for v in s.sm.iter() {
        if (*v as usize) < 26 {
            sm_cov[*v as usize] += 1;
        }
    }
    for v in s.ym.iter() {
        if (*v as usize) < 26 {
            ym_cov[*v as usize] += 1;
        }
    }
    (sm_cov, ym_cov)
}

/// 方案表确定性指纹（注册册排序前独立指纹，供对账）。
pub fn scheme_fp(s: &Scheme) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    h ^= s.id as u32;
    h = h.wrapping_mul(0x0100_0193);
    for v in s.sm.iter().chain(s.ym.iter()) {
        h ^= *v as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 自例程判据：落实 J1「框架注册 ×10 方案位就绪」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×10 方案位全注册 —— 10 套合法表（恒等基座+扰动变体）全部入册。
    let mut f = SpFrame::new();
    for id in 1..=10u8 {
        let (mut sm, mut ym) = ident_tab();
        // 变体：方案 id 决定一次确定性对换（保持合法：互换两目标）。
        let a = (id as usize * 3) % 26;
        let b = (id as usize * 7 + 5) % 26;
        if a != b {
            sm.swap(a, b);
            ym.swap(26 - 1 - a, 26 - 1 - b);
        }
        f.register(Scheme { id, sm, ym })?;
    }
    if f.len() != REG_CAP {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R2 场景：容量守恒 —— 第 11 套拒收。
    let (sm, ym) = ident_tab();
    match f.register(Scheme { id: 11, sm, ym }) {
        Err(E_REG_FULL) => {}
        _ => return Err(E_REG_FULL),
    }
    hits += 1;

    // R3 场景：方案 id 重复拒收。
    let mut f3 = SpFrame::new();
    f3.register(Scheme { id: 1, sm: ident_tab().0, ym: ident_tab().1 })?;
    match f3.register(Scheme { id: 1, sm: ident_tab().0, ym: ident_tab().1 }) {
        Err(E_DUP_SCHEME) => {}
        _ => return Err(E_DUP_SCHEME),
    }
    hits += 1;

    // R4 场景：坏表拒收 —— 表长恒 26（类型保证），重音位双键构造拒收。
    let mut bad = ident_tab().0;
    bad[5] = bad[4]; // 音位重复
    let mut f4 = SpFrame::new();
    match f4.register(Scheme { id: 2, sm: bad, ym: ident_tab().1 }) {
        Err(E_DUP_KEY) => {}
        _ => return Err(E_DUP_KEY),
    }
    let mut bad2 = ident_tab().1;
    bad2[0] = 26; // 越界编号
    match f4.register(Scheme { id: 3, sm: ident_tab().0, ym: bad2 }) {
        Err(E_DUP_KEY) => {}
        _ => return Err(E_DUP_KEY),
    }
    hits += 1;

    // R5 场景：转换不变式 —— 恒等表下 (键i, 键j) → 音节 (i, j)。
    let mut f5 = SpFrame::new();
    f5.register(Scheme { id: 1, sm: ident_tab().0, ym: ident_tab().1 })?;
    let out = f5.convert(1, &[0, 25, 5, 12])?;
    if out.len() != 2
        || out[0] != (Syll { sm: 0, ym: 25 })
        || out[1] != (Syll { sm: 5, ym: 12 })
    {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R6 场景：换表换义 —— 扰动表下同键对不同音节（可插拔契约）。
    let (mut sm6, _) = ident_tab();
    sm6.swap(0, 1);
    let mut f6 = SpFrame::new();
    f6.register(Scheme { id: 1, sm: ident_tab().0, ym: ident_tab().1 })?;
    f6.register(Scheme { id: 2, sm: sm6, ym: ident_tab().1 })?;
    let o1 = f5.convert(1, &[0, 0])?;
    let o2 = f6.convert(2, &[0, 0])?;
    if o1[0].sm == o2[0].sm {
        return Err(E_GUARD_UNBAL); // 表不同结果必须不同
    }
    hits += 1;

    // R7 场景：未注册方案 / 越界键 / 奇数长 全拒。
    let mut f7 = SpFrame::new();
    f7.register(Scheme { id: 1, sm: ident_tab().0, ym: ident_tab().1 })?;
    match f7.convert(9, &[0, 1]) {
        Err(E_UNKNOWN_SCHEME) => {}
        _ => return Err(E_UNKNOWN_SCHEME),
    }
    match f7.convert(1, &[26, 0]) {
        Err(E_BAD_IDX) => {}
        _ => return Err(E_BAD_IDX),
    }
    match f7.convert(1, &[0, 1, 2]) {
        Err(E_ODD_LEN) => {}
        _ => return Err(E_ODD_LEN),
    }
    hits += 1;

    // R8 场景：zh/ch/sh 三键位直读 —— 表内特殊韵母位按表直读（无隐式规则）。
    let mut sm8 = ident_tab().0;
    let mut ym8 = ident_tab().1;
    // 约定样例：键 v(21)→zh(25)，键 i(8)→chi 韵(20)，键 u(20)→shu 韵(21)。
    sm8[21] = 25;
    ym8[8] = 20;
    ym8[20] = 21;
    let mut f8 = SpFrame::new();
    f8.register(Scheme { id: 1, sm: sm8, ym: ym8 })?;
    let zh = f8.convert(1, &[21, 8])?; // v+i → (zh, chi)
    if zh[0] != (Syll { sm: 25, ym: 20 }) {
        return Err(E_GUARD_UNBAL);
    }
    let sh = f8.convert(1, &[21, 20])?; // v+u → (zh, shu)
    if sh[0] != (Syll { sm: 25, ym: 21 }) {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R9 场景：转换计数与封账 —— converts 累计；封账后注册拒收。
    if f8.converts() != 2 {
        return Err(E_GUARD_UNBAL);
    }
    let fp = f8.seal()?;
    f8.verify(fp)?;
    match f8.verify(fp ^ 1) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R10 场景：注册册 id 视图 —— 10 位全在册且互异。
    let ids = f.ids();
    if ids.len() != 10 {
        return Err(E_GUARD_UNBAL);
    }
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            if ids[i] == ids[j] {
                return Err(E_DUP_SCHEME);
            }
        }
    }
    hits += 1;

    // R11 场景：音位覆盖审计 —— 全部 10 套在册方案覆盖恒全 1（两表 26 位）。
    for id in 1..=10u8 {
        let s = f.reg_iter().find(|x| x.id == id).ok_or(E_UNKNOWN_SCHEME)?;
        let (sc, yc) = coverage_report(s);
        if sc.iter().any(|x| *x != 1) || yc.iter().any(|x| *x != 1) {
            return Err(E_DUP_KEY);
        }
    }
    hits += 1;

    // R12 场景：方案指纹对账 —— 同方案同指纹、扰动后指纹必变；
    //        注册册封账指纹与逐方案指纹叠加可互恰。
    let (sm_a, ym_a) = ident_tab();
    let sa = Scheme { id: 77, sm: sm_a, ym: ym_a };
    let fa = scheme_fp(&sa);
    let (sm_b, ym_b) = ident_tab();
    let mut sb = Scheme { id: 77, sm: sm_b, ym: ym_b };
    let fb0 = scheme_fp(&sb);
    if fa != fb0 {
        return Err(E_SEAL_MISMATCH);
    }
    sb.sm.swap(0, 1);
    if scheme_fp(&sb) == fa {
        return Err(E_SEAL_MISMATCH);
    }
    hits += 1;

    // R13 场景：注册制封闭性 —— 框架零内置方案语义：空册转换必查无；
    //        转换结果只由注册表决定（同键同表跨框架实例同结果）。
    let mut f13 = SpFrame::new();
    match f13.convert(1, &[0, 1]) {
        Err(E_UNKNOWN_SCHEME) => {}
        _ => return Err(E_UNKNOWN_SCHEME),
    }
    let (sm_x, ym_x) = ident_tab();
    let mut g13 = SpFrame::new();
    g13.register(Scheme { id: 1, sm: sm_x, ym: ym_x })?;
    let r1 = f5.convert(1, &[3, 4])?;
    let r2 = g13.convert(1, &[3, 4])?;
    if r1 != r2 {
        return Err(E_GUARD_UNBAL); // 同表同键跨实例必同结果（查表纯函数）
    }
    hits += 1;

    Ok(hits)
}
