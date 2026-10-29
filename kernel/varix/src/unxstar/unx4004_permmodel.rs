//! unx4004_permmodel — UNX-F4004 权限位模型（rwx 九位与 setuid/setgid/sticky）（诊断段 0x8340）。
//!
//! 判据原文：
//!   J1 权限位编解码与 chmod 八进制字面量全部 4,096 组合往返一致。
//!   J2 setuid/setgid/sticky 单独置位后 i_mode 对应位可观测为 1。
//!   J3 九位 rwx 三组（属主/组/其他）编码互不串位，组内读 4 写 2 执行 1。
//!   J4 chmod 越界八进制（高于 0o7777）注入被拒并入账。
//! 设计纪律：
//!   1. i_mode 权限域 0o7777 十二位：setuid=0o4000、setgid=0o2000、
//!      sticky=0o1000（POSIX.1-2017 <sys/stat.h>），九位 rwx 占 0o0777。
//!   2. 编解码为纯位运算：decode 提取十二位域，encode 由三组 rwx 加
//!      三特殊位合成；双向往返 4,096 组合逐一比对零差。
//!   3. 权限检查状态机：按凭据类别（属主/组/其他）选组，组内逐位判定，
//!      无 ANY 短路漏洞——请求 rwx 三位必须全部满足才放行。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8340;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PERM_OVER: u16 = SEG_BASE + 1; // 权限域越界（高于 0o7777）
const E_PERM_MIX: u16 = SEG_BASE + 2;  // 编解码往返不等（位串扰）
const E_STICK_OFF: u16 = SEG_BASE + 3; // 特殊位置位后不可观测
const E_BAL: u16 = SEG_BASE + 4;       // 账目不守恒
const E_NO_RULE: u16 = SEG_BASE + 5;   // 权限检查规则未登记
const E_ARG_BAD: u16 = SEG_BASE + 6;   // 凭据类别非法

/// 十二位权限域掩码（setuid/setgid/sticky + rwx 九位）。
pub const PERM_MASK: u32 = 0o7777;
/// 特殊位：setuid（属主 suid）、setgid（组 sgid）、sticky（粘滞）。
pub const S_ISUID: u32 = 0o4000;
pub const S_ISGID: u32 = 0o2000;
pub const S_ISVTX: u32 = 0o1000;

/// 凭据类别状态机：属主 → 组 → 其他，取首个命中组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermClass {
    Owner = 0,
    Group = 1,
    Other = 2,
}

/// 权限请求三元组：读/写/执行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccessReq {
    pub read: bool,    // 请求读位
    pub write: bool,   // 请求写位
    pub exec: bool,    // 请求执行位
}

impl AccessReq {
    pub fn new(r: bool, w: bool, x: bool) -> AccessReq {
        AccessReq { read: r, write: w, exec: x }
    }

    /// 三元组编码为组内三位八进制权值。
    pub fn bits(&self) -> u32 {
        let mut b = 0u32;
        if self.read {
            b |= 0o4;
        }
        if self.write {
            b |= 0o2;
        }
        if self.exec {
            b |= 0o1;
        }
        b
    }
}

/// 权限集：十二位 i_mode 权限域的可观测包装。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermSet {
    pub raw: u32, // 十二位原始域（0o0000..=0o7777）
}

impl PermSet {
    /// 从 chmod 八进制字面量构造，越界拒收。
    pub fn from_octal(o: u32) -> Result<PermSet, u16> {
        if o & !PERM_MASK != 0 {
            return Err(E_PERM_OVER);
        }
        Ok(PermSet { raw: o })
    }

    /// 属主 rwx 三位（八进制百位组）。
    pub fn owner_bits(&self) -> u32 {
        (self.raw >> 6) & 0o7
    }

    /// 组 rwx 三位（八进制十位组）。
    pub fn group_bits(&self) -> u32 {
        (self.raw >> 3) & 0o7
    }

    /// 其他 rwx 三位（八进制个位组）。
    pub fn other_bits(&self) -> u32 {
        self.raw & 0o7
    }

    /// 特殊位单独可观测（判据 J2）。
    pub fn setuid_on(&self) -> bool {
        self.raw & S_ISUID != 0
    }
    pub fn setgid_on(&self) -> bool {
        self.raw & S_ISGID != 0
    }
    pub fn sticky_on(&self) -> bool {
        self.raw & S_ISVTX != 0
    }

    /// 权限检查状态机：按类别选组，请求三位必须全部满足。
    pub fn check(&self, cls: PermClass, req: AccessReq) -> Result<bool, u16> {
        let have = match cls {
            PermClass::Owner => self.owner_bits(),
            PermClass::Group => self.group_bits(),
            PermClass::Other => self.other_bits(),
        };
        let want = req.bits();
        Ok(want & have == want) // 全满足才放行，无部分授信
    }

    /// chmod 变更：合法八进制域内直接重写。
    pub fn chmod(&mut self, o: u32) -> Result<(), u16> {
        if o & !PERM_MASK != 0 {
            return Err(E_PERM_OVER);
        }
        self.raw = o;
        Ok(())
    }
}

/// 权限模型状态机：Initial → Normalized → Evaluated。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermPhase {
    Initial = 0,    // 刚构造，未做编解码
    Normalized = 1, // 至少完成一次往返
    Evaluated = 2,  // 至少完成一次权限判定
}

/// 权限模型基座：编解码账 + 检查账 + 拒绝账。
pub struct PermModel {
    pub mode: PermSet,      // 当前挂账 i_mode 权限域
    pub enc_count: u32,     // 编码账
    pub dec_count: u32,     // 解码账
    pub round_count: u32,   // 往返一致账
    pub rej_count: u32,     // 越界拒绝账
    pub chk_count: u32,     // 权限检查账
    pub grant_count: u32,   // 放行账
    pub deny_count: u32,    // 拒绝放行账（非注入拒绝）
    pub phase: PermPhase,   // 模型状态机
}

impl PermModel {
    pub fn new(mode: PermSet) -> PermModel {
        PermModel {
            mode,
            enc_count: 0,
            dec_count: 0,
            round_count: 0,
            rej_count: 0,
            chk_count: 0,
            grant_count: 0,
            deny_count: 0,
            phase: PermPhase::Initial,
        }
    }

    /// 状态机读出口。
    pub fn phase(&self) -> PermPhase {
        self.phase
    }

    /// 编码：三组 rwx + 三特殊位 → 十二位域，入编码账。
    pub fn encode(&mut self) -> Result<u32, u16> {
        let o = (self.mode.owner_bits() << 6)
            | (self.mode.group_bits() << 3)
            | self.mode.other_bits()
            | (if self.mode.setuid_on() { S_ISUID } else { 0 })
            | (if self.mode.setgid_on() { S_ISGID } else { 0 })
            | (if self.mode.sticky_on() { S_ISVTX } else { 0 });
        self.enc_count += 1;
        Ok(o)
    }

    /// 解码：十二位域 → 权限集，越界入拒绝账。
    pub fn decode(&mut self, o: u32) -> Result<PermSet, u16> {
        match PermSet::from_octal(o) {
            Ok(p) => {
                self.dec_count += 1;
                Ok(p)
            }
            Err(e) => {
                self.rej_count += 1;
                Err(e)
            }
        }
    }

    /// 一次完整往返：encode → decode → raw 相等则入往返一致账。
    pub fn roundtrip(&mut self) -> Result<u32, u16> {
        let e = self.encode()?;
        let p = self.decode(e)?;
        if p.raw != self.mode.raw {
            return Err(E_PERM_MIX);
        }
        self.round_count += 1;
        self.phase = PermPhase::Normalized;
        Ok(self.round_count)
    }

    /// 带账权限检查：入检查账，按结果分流放行/拒绝放行账。
    pub fn check_logged(&mut self, cls: PermClass, req: AccessReq) -> Result<bool, u16> {
        self.chk_count += 1;
        let ok = self.mode.check(cls, req)?;
        self.phase = PermPhase::Evaluated;
        if ok {
            self.grant_count += 1;
        } else {
            self.deny_count += 1;
        }
        Ok(ok)
    }

    /// 守恒：编码账 == 解码账 == 往返账，且检查账 == 放行 + 拒绝放行。
    pub fn conserve(&self) -> bool {
        self.enc_count == self.dec_count
            && self.dec_count == self.round_count
            && self.chk_count == self.grant_count + self.deny_count
    }

    /// 特殊位单独置位 API（判据 J2 落点）。
    pub fn set_special(&mut self, uid: bool, gid: bool, vtx: bool) -> Result<(), u16> {
        let mut raw = self.mode.raw & !(S_ISUID | S_ISGID | S_ISVTX);
        if uid {
            raw |= S_ISUID;
        }
        if gid {
            raw |= S_ISGID;
        }
        if vtx {
            raw |= S_ISVTX;
        }
        self.mode = PermSet::from_octal(raw)?;
        Ok(())
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_PERM_OVER => "chmod 八进制越界（高于 0o7777）",
        E_PERM_MIX => "编解码往返位串扰",
        E_STICK_OFF => "特殊位置位后不可观测",
        E_BAL => "权限账目不守恒",
        E_NO_RULE => "权限检查规则未登记",
        E_ARG_BAD => "凭据类别非法",
        _ => "未知诊断码",
    }
}

/// 确定性演练：4,096 组合全往返 → 特殊位可观测 → 检查状态机 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全部 4,096 组合（0o0000..=0o7777）编解码往返零差（判据 J1 原文）。
    let mut pm = PermModel::new(PermSet::from_octal(0o644)?);
    let mut n = 0u32;
    for oct in 0..=0o7777u32 {
        pm.mode = PermSet::from_octal(oct)?;
        pm.roundtrip()?;
        n += 1;
    }
    if n != 4096 || pm.round_count != 4096 {
        return Err(E_PERM_MIX);
    }
    if pm.phase() != PermPhase::Normalized {
        return Err(E_PERM_MIX);
    }
    hits += 1;

    // R2：特殊位单独置位后可观测（判据 J2 原文）——三组合一逐一验。
    pm.mode = PermSet::from_octal(0o644)?;
    pm.set_special(true, false, false)?;
    if !pm.mode.setuid_on() || pm.mode.setgid_on() || pm.mode.sticky_on() {
        return Err(E_STICK_OFF);
    }
    pm.set_special(false, true, false)?;
    if pm.mode.setuid_on() || !pm.mode.setgid_on() || pm.mode.sticky_on() {
        return Err(E_STICK_OFF);
    }
    pm.set_special(false, false, true)?;
    if pm.mode.setuid_on() || pm.mode.setgid_on() || !pm.mode.sticky_on() {
        return Err(E_STICK_OFF);
    }
    hits += 1;

    // R3：三组互不串位——0o754 拆组回组值应为 7/5/4（判据 J3）。
    pm.mode = PermSet::from_octal(0o754)?;
    if pm.mode.owner_bits() != 0o7 || pm.mode.group_bits() != 0o5 || pm.mode.other_bits() != 0o4 {
        return Err(E_PERM_MIX);
    }
    hits += 1;

    // R4：检查状态机——属主 rwx 对 0o754：读放行、写放行、执行放行。
    if pm.check_logged(PermClass::Owner, AccessReq::new(true, true, true))? != true {
        return Err(E_BAL);
    }
    // 组：5=r-x——写请求被拒。
    if pm.check_logged(PermClass::Group, AccessReq::new(true, false, true))? != true {
        return Err(E_BAL);
    }
    if pm.check_logged(PermClass::Group, AccessReq::new(false, true, false))? != false {
        return Err(E_BAL);
    }
    // 其他：4=r——写/执行均拒。
    if pm.check_logged(PermClass::Other, AccessReq::new(true, false, false))? != true {
        return Err(E_BAL);
    }
    if pm.check_logged(PermClass::Other, AccessReq::new(true, true, false))? != false {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：守恒对账——编码=解码=往返账，检查账=放行+拒绝放行。
    if !pm.conserve() {
        return Err(E_BAL);
    }
    if pm.chk_count != 5 || pm.grant_count != 3 || pm.deny_count != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：chmod 越界注入（0o10000 高于十二位域）被拒（判据 J4）。
    if pm.mode.chmod(0o10000) != Err(E_PERM_OVER) {
        return Err(E_PERM_OVER);
    }
    hits += 1;

    // I2：decode 越界注入被拒并入拒绝账。
    if pm.decode(0o20000) != Err(E_PERM_OVER) {
        return Err(E_PERM_OVER);
    }
    if pm.rej_count != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I3：PermSet::from_octal 直接越界构造被拒。
    if PermSet::from_octal(0o17777) != Err(E_PERM_OVER) {
        return Err(E_PERM_OVER);
    }
    hits += 1;

    Ok(hits)
}
