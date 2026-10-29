//! unx0648_takeover —— UNX-F0648 A2 接管演练（交接包实读三步）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0648 · A2 接管演练（交接包实读三步） · 目标 ≥240 行
//! 判据原文摘录：UNX-F0648-J1 交接包实读三步（定位/校验/取用）全过，
//!   12 包读取成功率 12/12。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 实读三步状态机：定位（Locate）→ 校验（Verify）→ 取用
//!      （Consume），边闭集顺序推进，跳步/回退拒收。
//!   2. 12 包定容：包号 0..12 闭集，每包独立走三步；12/12 全过才
//!      接管成功。
//!   3. 校验步：包内容校验和（FNV-1a 64）复算必须 = 登记校验和，
//!      校验不过 = 定位到了坏包，红。
//!   4. 读取成功率：成功包数 / 12 = 12/12 判据（整数分数面：
//!      成功数 == 12）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5280 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5280 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5280;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x528F;

/// XcCode：包号越界。
pub const XC_BAD_PKG: u16 = SEG_BASE + 1; // 0x5281 包号 0..12 闭集外
/// XcCode：非法状态边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 2; // 0x5282 三步跳步/回退
/// XcCode：包未定位。
pub const XC_NOT_LOCATED: u16 = SEG_BASE + 3; // 0x5283 未定位先校验/取用
/// XcCode：校验不过。
pub const XC_VERIFY_FAIL: u16 = SEG_BASE + 4; // 0x5284 坏包
/// XcCode：包缺。
pub const XC_PKG_MISSING: u16 = SEG_BASE + 5; // 0x5285 包架缺包
/// XcCode：接管未完成。
pub const XC_TAKEOVER_SHORT: u16 = SEG_BASE + 6; // 0x5286 12/12 未齐

/// 包数（判据原文 12）。
pub const PKG_N: usize = 12;

/// FNV-1a 64。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 实读三步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadStage {
    /// 定位。
    Locate = 0,
    /// 校验。
    Verify = 1,
    /// 取用。
    Consume = 2,
}

impl ReadStage {
    /// 步名。
    pub fn name(self) -> &'static str {
        match self {
            ReadStage::Locate => "定位",
            ReadStage::Verify => "校验",
            ReadStage::Consume => "取用",
        }
    }
}

/// 交接包（包架登记面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfPkg {
    /// 包号。
    pub pkg_no: usize,
    /// 包名。
    pub name: String,
    /// 载荷。
    pub payload: String,
    /// 登记校验和。
    pub csum: u64,
}

impl ShelfPkg {
    /// 构包（校验和即算即登）。
    pub fn build(pkg_no: usize, name: &str, payload: &str) -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(payload.as_bytes());
        ShelfPkg {
            pkg_no,
            name: name.to_string(),
            payload: payload.to_string(),
            csum: fnv64(&buf),
        }
    }

    /// 校验和自证。
    pub fn csum_ok(&self) -> bool {
        let mut buf = Vec::new();
        buf.extend_from_slice(self.name.as_bytes());
        buf.extend_from_slice(self.payload.as_bytes());
        self.csum == fnv64(&buf)
    }
}

/// 单包实读进度（三步状态机）。
#[derive(Clone, Debug)]
pub struct PkgReadState {
    /// 包号。
    pub pkg_no: usize,
    /// 当前步（None = 未开始）。
    pub stage: Option<ReadStage>,
    /// 已取用载荷（三步走完才落）。
    pub taken: Option<String>,
}

/// A2 接管演练台。
#[derive(Clone, Debug)]
pub struct TakeoverRig {
    /// 包架（12 槽）。
    shelf: [Option<ShelfPkg>; PKG_N],
    /// 逐包读态。
    states: [PkgReadState; PKG_N],
}

impl TakeoverRig {
    /// 空演练台。
    pub fn new() -> Self {
        TakeoverRig {
            shelf: [
                None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            states: [
                PkgReadState { pkg_no: 0, stage: None, taken: None },
                PkgReadState { pkg_no: 1, stage: None, taken: None },
                PkgReadState { pkg_no: 2, stage: None, taken: None },
                PkgReadState { pkg_no: 3, stage: None, taken: None },
                PkgReadState { pkg_no: 4, stage: None, taken: None },
                PkgReadState { pkg_no: 5, stage: None, taken: None },
                PkgReadState { pkg_no: 6, stage: None, taken: None },
                PkgReadState { pkg_no: 7, stage: None, taken: None },
                PkgReadState { pkg_no: 8, stage: None, taken: None },
                PkgReadState { pkg_no: 9, stage: None, taken: None },
                PkgReadState { pkg_no: 10, stage: None, taken: None },
                PkgReadState { pkg_no: 11, stage: None, taken: None },
            ],
        }
    }

    /// 包架上包（去重）。
    pub fn shelve(&mut self, pkg: ShelfPkg) -> Result<(), u16> {
        if pkg.pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if self.shelf[pkg.pkg_no].is_some() {
            return Err(XC_PKG_MISSING);
        }
        let no = pkg.pkg_no;
        self.shelf[no] = Some(pkg);
        Ok(())
    }

    /// 定位步。
    pub fn locate(&mut self, pkg_no: usize) -> Result<(), u16> {
        if pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if self.shelf[pkg_no].is_none() {
            return Err(XC_PKG_MISSING);
        }
        match self.states[pkg_no].stage {
            None => {
                self.states[pkg_no].stage = Some(ReadStage::Locate);
                Ok(())
            }
            Some(s) => Err(if s as usize >= ReadStage::Locate as usize {
                XC_BAD_EDGE
            } else {
                XC_BAD_EDGE
            }),
        }
    }

    /// 校验步（必须已定位；复算校验和）。
    pub fn verify(&mut self, pkg_no: usize) -> Result<(), u16> {
        if pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if self.states[pkg_no].stage != Some(ReadStage::Locate) {
            return Err(XC_NOT_LOCATED);
        }
        let pkg = match &self.shelf[pkg_no] {
            Some(p) => p,
            None => return Err(XC_PKG_MISSING),
        };
        if !pkg.csum_ok() {
            return Err(XC_VERIFY_FAIL);
        }
        self.states[pkg_no].stage = Some(ReadStage::Verify);
        Ok(())
    }

    /// 取用步（必须已校验）。
    pub fn consume(&mut self, pkg_no: usize) -> Result<String, u16> {
        if pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if self.states[pkg_no].stage != Some(ReadStage::Verify) {
            return Err(XC_BAD_EDGE);
        }
        let payload = match &self.shelf[pkg_no] {
            Some(p) => p.payload.clone(),
            None => return Err(XC_PKG_MISSING),
        };
        self.states[pkg_no].stage = Some(ReadStage::Consume);
        self.states[pkg_no].taken = Some(payload.clone());
        Ok(payload)
    }

    /// 成功包数（三步走完的包数）。
    pub fn consumed_n(&self) -> usize {
        self.states
            .iter()
            .filter(|s| s.stage == Some(ReadStage::Consume))
            .count()
    }

    /// 读取成功率：12/12 判据。
    pub fn success_rate_full(&self) -> Result<usize, u16> {
        if self.consumed_n() != PKG_N {
            return Err(XC_TAKEOVER_SHORT);
        }
        Ok(PKG_N)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "A2 接管演练 · 架上包 {}/{} · 已取用 {}",
            self.shelf.iter().filter(|p| p.is_some()).count(),
            PKG_N,
            self.consumed_n()
        );
        for st in &self.states {
            let sn = match st.stage {
                Some(s) => s.name(),
                None => "未开始",
            };
            s.push_str(&alloc::format!("\n  包{} {} 取用={}", st.pkg_no, sn, st.taken.is_some()));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 12 包全量实读演练：上架 12 包 → 逐包三步 → 成功率 12/12。
pub fn run_takeover() -> Result<(TakeoverRig, usize), u16> {
    let names = [
        "时钟", "内存", "计时", "中断", "ACPI", "PCI",
        "显示", "输入", "ktest", "日志", "参数", "initrd",
    ];
    let mut rig = TakeoverRig::new();
    for i in 0..PKG_N {
        rig.shelve(ShelfPkg::build(
            i,
            names[i],
            &alloc::format!("包{}载荷{}", i, i * 13),
        ))?;
    }
    for i in 0..PKG_N {
        rig.locate(i)?;
        rig.verify(i)?;
        rig.consume(i)?;
    }
    let n = rig.success_rate_full()?;
    Ok((rig, n))
}

/// 自例程：主路径（12/12）+ 错误路径（缺包/跳步/坏包/越界/重上架）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (rig, n) = run_takeover()?;
    if n != PKG_N {
        return Err(XC_TAKEOVER_SHORT);
    }
    if rig.fingerprint() == 0 {
        return Err(XC_TAKEOVER_SHORT);
    }

    // 错误路径 1：缺包定位。
    let mut r1 = TakeoverRig::new();
    if r1.locate(3) != Err(XC_PKG_MISSING) {
        return Err(XC_PKG_MISSING);
    }
    // 错误路径 2：跳步（未定位先校验）。
    let mut r2 = TakeoverRig::new();
    r2.shelve(ShelfPkg::build(0, "包0", "载荷"))?;
    if r2.verify(0) != Err(XC_NOT_LOCATED) {
        return Err(XC_NOT_LOCATED);
    }
    // 错误路径 3：未校验先取用。
    r2.locate(0)?;
    if r2.consume(0) != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }
    // 错误路径 4：坏包（登记后篡改载荷 → 校验红）。
    let mut r4 = TakeoverRig::new();
    r4.shelve(ShelfPkg::build(0, "包0", "载荷"))?;
    r4.locate(0)?;
    if let Some(p) = &mut r4.shelf[0] {
        p.payload.push('x');
    }
    if r4.verify(0) != Err(XC_VERIFY_FAIL) {
        return Err(XC_VERIFY_FAIL);
    }
    // 错误路径 5：包号越界。
    let mut r5 = TakeoverRig::new();
    if r5.locate(PKG_N) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    if r5.shelve(ShelfPkg::build(PKG_N, "越界", "x")) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    // 错误路径 6：重上架。
    let mut r6 = TakeoverRig::new();
    r6.shelve(ShelfPkg::build(0, "包0", "载荷"))?;
    if r6.shelve(ShelfPkg::build(0, "包0", "载荷2")) != Err(XC_PKG_MISSING) {
        return Err(XC_PKG_MISSING);
    }
    // 错误路径 7：缺包收口（只读 11 包禁 12/12）。
    let mut r7 = TakeoverRig::new();
    let names = [
        "时钟", "内存", "计时", "中断", "ACPI", "PCI",
        "显示", "输入", "ktest", "日志", "参数", "initrd",
    ];
    for i in 0..PKG_N - 1 {
        r7.shelve(ShelfPkg::build(i, names[i], &alloc::format!("载荷{}", i)))?;
        r7.locate(i)?;
        r7.verify(i)?;
        r7.consume(i)?;
    }
    if r7.success_rate_full() != Err(XC_TAKEOVER_SHORT) {
        return Err(XC_TAKEOVER_SHORT);
    }
    Ok(0)
}
