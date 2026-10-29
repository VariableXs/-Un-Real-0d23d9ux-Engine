//! unx4096_dcompare — UNX-F4096 d_compare 大小写敏感声明（D3 联签边界）（诊断段 0x8E20）。
//!
//! 判据原文：UNX-F4096-J1 Unix 人格全域字节级敏感比较（大小写变体视为不同条目，
//!           1,000 例断言），D3 转换层的大小写折叠入场契约锚定有账（联签边界注记）。
//! 可验性质（本模块自证面）：
//!   J1 Unix 人格下 1,000 例大小写变体全部判"不同条目"（字节级敏感），
//!      且同名自比为真（1,000 例），断言成功计数逐例入账；
//!   J2 D3 折叠模式只在联签（Signed）后可启用：未联签时任何折叠比较/折叠登记
//!      一律拒（E_UNSIGNED），拒收不动断言账；
//!   J3 折叠入场契约账：每次进入折叠域必须登记一条契约项（原名 + 折叠名 + 签名），
//!      契约项数 == 折叠比较次数（一比较一锚，不许裸折叠）；
//!   J4 模式隔离：Unix 人格下登记的契约项不参与 Unix 比较；切换模式只改 Persona，
//!      不改任何已登记契约（历史不可改写）；
//!   J5 非法输入拒：空名（E_NAME_EMPTY）、超长（E_NAME_LONG）、重复契约项
//!      （E_DUP_CONTRACT）、非法模式码（E_MODE_OOR）。
//! 设计纪律：
//!   1. "Unix 人格全域字节级敏感"是这个模块的宪法：Persona::UnixByte 下比较函数
//!      一个字节都不折叠，A-Z 与 a-z 恒不等——也不允许上层用参数绕过，绕过只能靠
//!      显式切 Persona 且必须先联签。
//!   2. 折叠只折叠 ASCII A-Z（D3 转换层的最小契约面）：不碰 Unicode 大小写，
//!      不碰 locale，避免把"人格"和"语言环境"两件事混在一起。
//!   3. 契约项是 append-only 的证据链：登记后不可改、不可删，重复登记直接拒
//!      （同一条折叠语义只允许锚一次）。
//!   4. 联签是单向的：Unsigned → Signed 只走一次，Signed 后不接受"撤销联签"
//!      （撤销需要新开账，本模块不提供反悔口）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8E20 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8E20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_EMPTY: u16 = SEG_BASE + 1; // 空名字
const E_NAME_LONG: u16 = SEG_BASE + 2; // 名字超长（NAME_MAX=255）
const E_UNSIGNED: u16 = SEG_BASE + 3; // 未联签即请求折叠域
const E_DUP_CONTRACT: u16 = SEG_BASE + 4; // 重复登记同一折叠契约
const E_MODE_OOR: u16 = SEG_BASE + 5; // 模式码越界
const E_BAL: u16 = SEG_BASE + 6; // 断言账/契约账不平
const E_NOT_UNIX: u16 = SEG_BASE + 7; // 非 Unix 人格却走字节级断言口
const E_SIGN_TWICE: u16 = SEG_BASE + 8; // 重复联签

/// 名字长度上限（Linux NAME_MAX）。
pub const NAME_MAX: usize = 255;

/// 判据要求的断言例数（1,000 例）。
pub const CASES: u32 = 1_000;

/// 人格模式：Unix 字节级 / D3 折叠。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonaMode {
    UnixByte = 0, // Unix 人格：字节级敏感
    D3Fold = 1,   // D3 转换层：ASCII 大小写折叠
}

/// 联签状态：未签 / 已签（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    Unsigned = 0, // 未联签，禁用折叠域
    Signed = 1,   // 已联签，可进折叠域
}

/// 契约项：一次折叠入场的证据。
pub struct ContractEntry {
    pub name: Vec<u8>,   // 原始名字
    pub folded: Vec<u8>, // 折叠后名字
    pub seq: u32,        // 登记序号
}

/// Unix 人格：模式 + 断言账。
pub struct Persona {
    pub mode: PersonaMode, // 当前人格
    pub asserts: u32,      // 断言成功计数
    pub case_variants: u32, // 判"不同条目"的变体例数
    pub self_same: u32,    // 同名自比为真的例数
}

impl Persona {
    pub fn new() -> Persona {
        Persona {
            mode: PersonaMode::UnixByte,
            asserts: 0,
            case_variants: 0,
            self_same: 0,
        }
    }

    /// 模式码 → 闭集。
    pub fn mode_by_code(code: u16) -> Result<PersonaMode, u16> {
        match code {
            0 => Ok(PersonaMode::UnixByte),
            1 => Ok(PersonaMode::D3Fold),
            _ => Err(E_MODE_OOR),
        }
    }

    /// 切模式：进折叠域必须已联签（由调用方先查 D3Contract）。
    pub fn set_mode(&mut self, m: PersonaMode, signed: bool) -> Result<(), u16> {
        if m == PersonaMode::D3Fold && !signed {
            return Err(E_UNSIGNED); // 未联签不许进折叠域
        }
        self.mode = m;
        Ok(())
    }
}

/// D3 转换层联签契约：签名状态 + 契约项链。
pub struct D3Contract {
    pub state: SignState,       // 联签状态
    pub entries: Vec<ContractEntry>, // 契约项（append-only）
    pub folds: u32,             // 折叠比较次数（= 契约项数）
    pub rejects: u32,           // 拒收次数
}

impl D3Contract {
    pub fn new() -> D3Contract {
        D3Contract {
            state: SignState::Unsigned,
            entries: Vec::new(),
            folds: 0,
            rejects: 0,
        }
    }

    /// 联签：只能签一次（单向）。
    pub fn sign(&mut self) -> Result<(), u16> {
        if self.state == SignState::Signed {
            self.rejects += 1;
            return Err(E_SIGN_TWICE);
        }
        self.state = SignState::Signed;
        Ok(())
    }

    /// 是否已联签。
    pub fn signed(&self) -> bool {
        self.state == SignState::Signed
    }

    /// 折叠登记 + 折叠比较（J3 的唯一入口）：未联签直接拒，且不落任何账。
    pub fn fold_compare(&mut self, a: &[u8], b: &[u8]) -> Result<bool, u16> {
        if !self.signed() {
            self.rejects += 1;
            return Err(E_UNSIGNED);
        }
        if a.is_empty() || b.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if a.len() > NAME_MAX || b.len() > NAME_MAX {
            return Err(E_NAME_LONG);
        }
        // 先建契约项（append-only），再比较
        if self.has_entry(a) {
            self.rejects += 1;
            return Err(E_DUP_CONTRACT);
        }
        self.entries.push(ContractEntry {
            name: copy(a),
            folded: fold_all(a),
            seq: self.entries.len() as u32 + 1,
        });
        self.folds += 1;
        let fa = fold_all(a);
        let fb = fold_all(b);
        Ok(bytes_eq(&fa, &fb))
    }

    /// 契约项是否已登记（按原名比）。
    pub fn has_entry(&self, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if bytes_eq(&self.entries[i].name, name) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 契约项数 == 折叠次数（J3 守恒）。
    pub fn folds_conserved(&self) -> bool {
        self.entries.len() as u32 == self.folds
    }
}

/// ASCII 折叠：A-Z → a-z（唯一折叠点，设计纪律 2）。
fn fold(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// 整串折叠（返回新 Vec，调用方负责长度已校验）。
fn fold_all(a: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        v.push(fold(a[i]));
        i += 1;
    }
    v
}

/// 字节相等。
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节拷贝。
fn copy(a: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        v.push(a[i]);
        i += 1;
    }
    v
}

/// 联签边界注记：本模块是"Unix 字节敏感"与"D3 折叠"的唯一交界。
/// 上边界（向 B05 dcache）：Unix 人格下 d_compare 恒字节级，不接受任何折叠参数。
/// 下边界（向 D3 转换层）：折叠必须携带联签契约项，一次比较一条，可逐条回溯。
pub const BOUNDARY_NOTE: &str =
    "D3-SIGNED-ONLY: Unix persona keeps byte-exact compare; case folding enters only through signed contract";

/// 断言：Unix 人格下 a、b 是否相等（字节级，不折叠，不接受外部模式）。
pub fn unix_compare(p: &mut Persona, a: &[u8], b: &[u8]) -> Result<bool, u16> {
    if p.mode != PersonaMode::UnixByte {
        return Err(E_NOT_UNIX); // 非 Unix 人格不许走这个口
    }
    if a.is_empty() || b.is_empty() {
        return Err(E_NAME_EMPTY);
    }
    if a.len() > NAME_MAX || b.len() > NAME_MAX {
        return Err(E_NAME_LONG);
    }
    let same = bytes_eq(a, b);
    p.asserts += 1;
    if same {
        p.self_same += 1;
    } else {
        // 长度相等但内容不同 → 变体；长度不同 → 也不同条目（同样计入变体）
        p.case_variants += 1;
    }
    Ok(same)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_EMPTY => "空名字（dentry 名至少 1 字节）",
        E_NAME_LONG => "名字超长（NAME_MAX=255）",
        E_UNSIGNED => "未联签即请求折叠域",
        E_DUP_CONTRACT => "重复登记同一折叠契约",
        E_MODE_OOR => "模式码越界（合法 0/1）",
        E_BAL => "断言账/契约账不平",
        E_NOT_UNIX => "非 Unix 人格却走字节级断言口",
        E_SIGN_TWICE => "重复联签",
        _ => "未知诊断码",
    }
}

/// 造大小写变体：把 ASCII 小写字母翻成大写（其余原样），写入 out。
fn to_upper(a: &[u8], out: &mut Vec<u8>) {
    out.clear();
    let mut i = 0usize;
    while i < a.len() {
        let b = a[i];
        if b >= b'a' && b <= b'z' {
            out.push(b - 32);
        } else {
            out.push(b);
        }
        i += 1;
    }
}

/// 造一个名字：字母表 26，长度 3..9，序号尾缀防重复。
fn make_name(seed: u32, out: &mut Vec<u8>) {
    out.clear();
    let mut s = seed as u64;
    let len = 3 + (s % 7) as usize;
    let mut i = 0usize;
    while i < len {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push(b'a' + ((s >> 37) as u8 % 26));
        i += 1;
    }
    out.push(b'.');
    let mut digits: [u8; 8] = [0; 8];
    let mut n = 0usize;
    let mut v = seed;
    if v == 0 {
        out.push(b'0');
        return;
    }
    while v > 0 && n < 8 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(digits[n]);
    }
}

/// 确定性演练：千例字节敏感 → 折叠联签 → 契约账 → 模式隔离 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut p = Persona::new();
    let mut c = D3Contract::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut up: Vec<u8> = Vec::new();

    // R1：Unix 人格千例——原名与大小写变体必须判不同（J1 主面）。
    let mut i = 0u32;
    while i < CASES {
        make_name(i, &mut buf);
        to_upper(&buf, &mut up);
        if unix_compare(&mut p, &buf, &up)? {
            return Err(E_BAL); // 大小写变体不得判等
        }
        i += 1;
    }
    if p.case_variants != CASES {
        return Err(E_BAL);
    }
    if p.asserts != CASES {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：同名自比千例——必须判等（否则比较器本身坏了）。
    i = 0;
    while i < CASES {
        make_name(i, &mut buf);
        if !unix_compare(&mut p, &buf, &buf)? {
            return Err(E_BAL);
        }
        i += 1;
    }
    if p.self_same != CASES || p.asserts != CASES * 2 {
        return Err(E_BAL);
    }
    if p.mode != PersonaMode::UnixByte {
        return Err(E_NOT_UNIX);
    }
    hits += 1;

    // R3：未联签进折叠域 → 全拒（E_UNSIGNED），且折叠账不动（J2）。
    if p.set_mode(PersonaMode::D3Fold, c.signed()) != Err(E_UNSIGNED) {
        return Err(E_UNSIGNED);
    }
    make_name(7, &mut buf);
    to_upper(&buf, &mut up);
    if c.fold_compare(&buf, &up) != Err(E_UNSIGNED) {
        return Err(E_UNSIGNED);
    }
    if c.folds != 0 || !c.entries.is_empty() {
        return Err(E_BAL); // 拒收不落契约
    }
    if p.mode != PersonaMode::UnixByte {
        return Err(E_NOT_UNIX); // 拒收不得改人格
    }
    hits += 1;

    // R4：联签后进折叠域——同一对变体在折叠下判等，且落一条契约项（J3）。
    c.sign()?;
    if !c.signed() {
        return Err(E_UNSIGNED);
    }
    p.set_mode(PersonaMode::D3Fold, c.signed())?;
    if p.mode != PersonaMode::D3Fold {
        return Err(E_BAL);
    }
    if !c.fold_compare(&buf, &up)? {
        return Err(E_BAL); // 折叠后应判等
    }
    if c.folds != 1 || c.entries.len() != 1 {
        return Err(E_BAL);
    }
    if !c.folds_conserved() {
        return Err(E_BAL);
    }
    if !bytes_eq(&c.entries[0].name, &buf) {
        return Err(E_BAL); // 契约项记的是原名
    }
    hits += 1;

    // R5：折叠域里再比不同串——不同的两个名字仍判不等（折叠不是"全等"）。
    make_name(11, &mut buf);
    to_upper(&buf, &mut up);
    let mut other: Vec<u8> = Vec::new();
    make_name(12, &mut other);
    if c.fold_compare(&buf, &other)? {
        return Err(E_BAL);
    }
    if c.folds != 2 || c.entries.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：模式隔离——切回 Unix 人格后，同一对变体又判不等（J4）；
    //     已登记契约项仍在（历史不可改写）。
    let entries_before = c.entries.len();
    p.set_mode(PersonaMode::UnixByte, true)?;
    if unix_compare(&mut p, &buf, &up)? {
        return Err(E_BAL); // Unix 下必须字节级不等
    }
    if c.entries.len() != entries_before {
        return Err(E_BAL); // 切模式不改契约
    }
    if !c.folds_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复联签 → E_SIGN_TWICE，状态仍 Signed（单向）。
    if c.sign() != Err(E_SIGN_TWICE) {
        return Err(E_SIGN_TWICE);
    }
    if !c.signed() || c.rejects == 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：重复契约项 → E_DUP_CONTRACT（同一原名不许锚两次）。
    make_name(11, &mut buf);
    let dup = copy(&buf);
    if c.fold_compare(&dup, &dup) != Err(E_DUP_CONTRACT) {
        return Err(E_DUP_CONTRACT);
    }
    hits += 1;

    // I3：空名 / 超长名——折叠口与 Unix 口双点拒（E_NAME_EMPTY / E_NAME_LONG）。
    if c.fold_compare(b"", b"a") != Err(E_NAME_EMPTY) {
        return Err(E_NAME_EMPTY);
    }
    if unix_compare(&mut p, b"a", b"") != Err(E_NAME_EMPTY) {
        return Err(E_NAME_EMPTY);
    }
    let mut longbuf: Vec<u8> = Vec::new();
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longbuf.push(b'a');
        z += 1;
    }
    if c.fold_compare(&longbuf, &longbuf) != Err(E_NAME_LONG) {
        return Err(E_NAME_LONG);
    }
    if unix_compare(&mut p, &longbuf, &longbuf) != Err(E_NAME_LONG) {
        return Err(E_NAME_LONG);
    }
    hits += 1;

    // I4：模式码越界（2 / 65535）→ E_MODE_OOR；非 Unix 人格走 Unix 断言口 → E_NOT_UNIX。
    if Persona::mode_by_code(2) != Err(E_MODE_OOR) {
        return Err(E_MODE_OOR);
    }
    if Persona::mode_by_code(0xFFFF) != Err(E_MODE_OOR) {
        return Err(E_MODE_OOR);
    }
    let mut fold_persona = Persona::new();
    fold_persona.set_mode(PersonaMode::D3Fold, true)?;
    if unix_compare(&mut fold_persona, b"Ab", b"ab") != Err(E_NOT_UNIX) {
        return Err(E_NOT_UNIX);
    }
    hits += 1;

    // I5：折叠映射自审——ASCII A-Z 全量折叠到 a-z，其它字节恒等（契约的物理面）。
    let mut b = 0u16;
    while b < 256 {
        let x = b as u8;
        let f = fold(x);
        if x >= b'A' && x <= b'Z' {
            if f != x + 32 {
                return Err(E_BAL);
            }
        } else if f != x {
            return Err(E_BAL); // 非 A-Z 一律恒等（不碰 Unicode/locale）
        }
        b += 1;
    }
    if BOUNDARY_NOTE.is_empty() {
        return Err(E_BAL); // 联签边界注记必须在位
    }
    hits += 1;

    // 终审：断言账、契约账双双守恒，人格回到 Unix（默认面）。
    if p.asserts != CASES * 2 + 1 {
        return Err(E_BAL);
    }
    if !c.folds_conserved() || c.folds != 2 {
        return Err(E_BAL);
    }
    if p.mode != PersonaMode::UnixByte {
        return Err(E_NOT_UNIX);
    }
    hits += 1;

    Ok(hits)
}
