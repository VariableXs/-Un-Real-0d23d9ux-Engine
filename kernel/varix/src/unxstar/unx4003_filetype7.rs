//! unx4003_filetype7 — UNX-F4003 文件类型系统七类（REG/DIR/LNK/CHR/BLK/FIFO/SOCK）（诊断段 0x8320）。
//!
//! 判据原文：
//!   J1 七类类型枚举与 i_mode S_IFMT 位段编码逐位对照 POSIX.1-2017 表一致。
//!   J2 类型编解码往返 10,000 次零错，计数账可导出。
//!   J3 S_IFMT 掩码外位（权限九位 + 三特殊位）在类型编解码中不被扰动。
//!   J4 非法 i_mode（S_IFMT 位段落入七类之外的组合）解码被拒并入账。
//! 设计纪律：
//!   1. S_IFMT 位段锚定 POSIX.1-2017：<sys/stat.h> 高四位 0xF000：
//!      FIFO=0o010000(0x1000)、CHR=0o020000(0x2000)、DIR=0o040000(0x4000)、
//!      BLK=0o060000(0x6000)、REG=0o100000(0x8000)、LNK=0o120000(0xA000)、
//!      SOCK=0o140000(0xC000)；0o060000 与 0o100000 之外的 0xE000 组合非法。
//!   2. 编解码往返守恒：encode(decode(m)) == m & S_IFMT 对合法输入恒成立；
//!      权限低位掩码 0o7777 与类型高位互不串位。
//!   3. 往返账：每次成功往返入往返账，非法解码入拒绝账，两账之和等于
//!      总请求账；10,000 次演练后零错可对账导出。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8320;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_FMT: u16 = SEG_BASE + 1;  // S_IFMT 位段非法组合（七类之外）
const E_TYPE_MIX: u16 = SEG_BASE + 2; // 编解码往返不等（类型串位）
const E_PERM_TOUCH: u16 = SEG_BASE + 3; // 类型操作扰动权限低位
const E_BAL: u16 = SEG_BASE + 4;      // 往返账与总请求账不平
const E_ROUND_LOST: u16 = SEG_BASE + 5; // 10,000 次往返出现错账
const E_KIND_NONE: u16 = SEG_BASE + 6; // 对未知类查询被拒

/// S_IFMT 类型位段掩码（高四位，POSIX.1-2017 <sys/stat.h>）。
pub const S_IFMT: u32 = 0o170000;
/// 权限与特殊位低位掩码（九位 rwx + setuid/setgid/sticky）。
pub const S_PERM_MASK: u32 = 0o7777;

/// 七类文件类型（判别值即 S_IFMT 位段值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType7 {
    Fifo = 0o010000,  // 命名管道
    Chr = 0o020000,   // 字符设备
    Dir = 0o040000,   // 目录
    Blk = 0o060000,   // 块设备
    Reg = 0o100000,   // 普通文件
    Lnk = 0o120000,   // 符号链接
    Sock = 0o140000,  // 套接字
}

pub const KIND_COUNT: usize = 7;

/// 七类全集常量表：(类型, 助记名)。逐位对照 POSIX.1-2017 表写死。
pub const KIND_TAB: [(u32, &'static str); KIND_COUNT] = [
    (0o010000, "FIFO"), (0o020000, "CHR"), (0o040000, "DIR"),
    (0o060000, "BLK"), (0o100000, "REG"), (0o120000, "LNK"),
    (0o140000, "SOCK"),
];

/// 类型解码：i_mode → 七类枚举，S_IFMT 位段非法组合返回 Err(E_BAD_FMT)。
pub fn decode(i_mode: u32) -> Result<FileType7, u16> {
    let fmt = i_mode & S_IFMT;
    match fmt {
        0o010000 => Ok(FileType7::Fifo),
        0o020000 => Ok(FileType7::Chr),
        0o040000 => Ok(FileType7::Dir),
        0o060000 => Ok(FileType7::Blk),
        0o100000 => Ok(FileType7::Reg),
        0o120000 => Ok(FileType7::Lnk),
        0o140000 => Ok(FileType7::Sock),
        _ => Err(E_BAD_FMT),
    }
}

/// 类型编码：七类枚举 → S_IFMT 位段值（纯函数）。
pub fn encode(t: FileType7) -> u32 {
    t as u32
}

/// 合成 i_mode：类型位段 + 权限低位（互不串位）。
pub fn synth(t: FileType7, perm: u32) -> Result<u32, u16> {
    if perm & !S_PERM_MASK != 0 {
        return Err(E_PERM_TOUCH);
    }
    Ok(encode(t) | perm)
}

/// 单槽类型登记状态机：Empty → Encoded → Verified。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotPhase {
    Empty = 0,    // 仅构造，未编码
    Encoded = 1,  // 类型位段已编码
    Verified = 2, // 编解码往返已过
}

/// 类型化 i_node 记账单元：绑定类型 + 往返账。
pub struct TypeEntry {
    pub kind: FileType7,   // 本槽文件类型
    pub mode_bits: u32,    // 本槽 i_mode 全量位（类型 + 权限）
    pub rounds: u32,       // 本槽编解码往返成功数
    pub phase: SlotPhase,  // 槽登记状态机
}

impl TypeEntry {
    fn new(kind: FileType7) -> TypeEntry {
        TypeEntry {
            kind,
            mode_bits: encode(kind),
            rounds: 0,
            phase: SlotPhase::Encoded,
        }
    }
}

/// 类型系统基座：七槽账 + 往返总账 + 拒绝账。
pub struct TypeFabric {
    pub slots: [TypeEntry; KIND_COUNT],
    pub rounds_total: u32,  // 全局往返成功账
    pub rej_total: u32,     // 非法解码拒绝账
    pub req_total: u32,     // 总请求账（往返 + 拒绝）
}

impl TypeFabric {
    pub fn new() -> TypeFabric {
        TypeFabric {
            slots: [
                TypeEntry::new(FileType7::Reg),
                TypeEntry::new(FileType7::Dir),
                TypeEntry::new(FileType7::Lnk),
                TypeEntry::new(FileType7::Chr),
                TypeEntry::new(FileType7::Blk),
                TypeEntry::new(FileType7::Fifo),
                TypeEntry::new(FileType7::Sock),
            ],
            rounds_total: 0,
            rej_total: 0,
            req_total: 0,
        }
    }

    fn slot_of(&self, kind: FileType7) -> usize {
        match kind {
            FileType7::Reg => 0,
            FileType7::Dir => 1,
            FileType7::Lnk => 2,
            FileType7::Chr => 3,
            FileType7::Blk => 4,
            FileType7::Fifo => 5,
            FileType7::Sock => 6,
        }
    }

    /// 单槽往返一次：mode → decode → encode → 与原类型比对，成功入账。
    pub fn roundtrip(&mut self, kind: FileType7, mode: u32) -> Result<u32, u16> {
        self.req_total += 1;
        let d = decode(mode)?;
        if d != kind {
            self.rej_total += 1;
            return Err(E_TYPE_MIX);
        }
        // 权限低位必须原样保留（判据 J3）。
        let rebuilt = synth(kind, mode & S_PERM_MASK)?;
        if rebuilt != mode {
            self.rej_total += 1;
            return Err(E_PERM_TOUCH);
        }
        let i = self.slot_of(kind);
        self.slots[i].rounds += 1;
        self.slots[i].mode_bits = mode;
        self.slots[i].phase = SlotPhase::Verified;
        self.rounds_total += 1;
        Ok(self.rounds_total)
    }

    /// 非法 i_mode 注入：必须被拒并入拒绝账（判据 J4）。
    pub fn inject_bad(&mut self, mode: u32) -> Result<(), u16> {
        self.req_total += 1;
        match decode(mode) {
            Ok(_) => Err(E_BAD_FMT), // 非法值竟被接受——判据失守
            Err(_) => {
                self.rej_total += 1;
                Ok(())
            }
        }
    }

    /// 守恒核对：往返账 + 拒绝账 == 总请求账。
    pub fn conserve(&self) -> bool {
        self.rounds_total + self.rej_total == self.req_total
    }

    /// 按类型查询槽账（未知类返回 E_KIND_NONE）。
    pub fn slot_rounds(&self, kind: FileType7) -> Result<u32, u16> {
        let i = self.slot_of(kind);
        if i >= KIND_COUNT {
            return Err(E_KIND_NONE);
        }
        Ok(self.slots[i].rounds)
    }

    /// 槽状态导出（状态机读面）。
    pub fn slot_phase(&self, kind: FileType7) -> Result<SlotPhase, u16> {
        let i = self.slot_of(kind);
        if i >= KIND_COUNT {
            return Err(E_KIND_NONE);
        }
        Ok(self.slots[i].phase)
    }

    /// 七类类型值互异复核（判据 J1 唯一性面）。
    pub fn kinds_unique(&self) -> bool {        for a in 0..KIND_COUNT {
            for b in (a + 1)..KIND_COUNT {
                if KIND_TAB[a].0 == KIND_TAB[b].0 {
                    return false;
                }
            }
        }
        true
    }

    /// 表枚举值与枚举判别值逐位对照（判据 J1 主面）。
    pub fn tab_matches_enum(&self) -> bool {
        let all = [
            FileType7::Fifo, FileType7::Chr, FileType7::Dir,
            FileType7::Blk, FileType7::Reg, FileType7::Lnk,
            FileType7::Sock,
        ];
        for (v, _) in KIND_TAB.iter() {
            let mut found = false;
            for k in all.iter() {
                if encode(*k) == *v {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BAD_FMT => "S_IFMT 位段落入七类之外（0x0000/0xE000 区）",
        E_TYPE_MIX => "编解码往返类型串位",
        E_PERM_TOUCH => "类型操作扰动权限低位 0o7777",
        E_BAL => "往返账与总请求账不平",
        E_ROUND_LOST => "10,000 次往返出现错账",
        E_KIND_NONE => "对未知类查询被拒",
        _ => "未知诊断码",
    }
}

/// 确定性演练：逐位对照 → 万次往返 → 非法位段注入 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut fab = TypeFabric::new();

    // R1：七类类型表与枚举判别值逐位对照一致（判据 J1）。
    if !fab.tab_matches_enum() || !fab.kinds_unique() {
        return Err(E_TYPE_MIX);
    }
    hits += 1;

    // R2：七类各一次权限合成往返——低位 0o644 不被扰动（判据 J3）。
    let kinds = [
        FileType7::Reg, FileType7::Dir, FileType7::Lnk,
        FileType7::Chr, FileType7::Blk, FileType7::Fifo,
        FileType7::Sock,
    ];
    for k in kinds.iter() {
        let m = synth(*k, 0o644)?;
        if fab.roundtrip(*k, m).is_err() {
            return Err(E_TYPE_MIX);
        }
    }
    if fab.rounds_total != 7 {
        return Err(E_ROUND_LOST);
    }
    // 七槽状态机全部推进到 Verified。
    if fab.slot_phase(FileType7::Reg)? != SlotPhase::Verified {
        return Err(E_TYPE_MIX);
    }
    hits += 1;

    // R3：10,000 次编解码往返零错（判据 J2 原文）——确定性轮转七类。
    for n in 0..10_000u32 {
        let k = kinds[(n % 7) as usize];
        // 权限位在 0..0o777 内确定性步进，类型位段保持本类。
        let perm = (n * 3) % 0o1000;
        let m = synth(k, perm)?;
        fab.roundtrip(k, m)?;
    }
    if fab.rounds_total != 7 + 10_000 {
        return Err(E_ROUND_LOST);
    }
    hits += 1;

    // R4：非法 S_IFMT 位段注入——0x0000（无类型）与 0xE000 组合全拒。
    fab.inject_bad(0o000644)?; // 0o000000 类型段非法
    fab.inject_bad(0o160000 | 0o644)?; // 0o160000 非法组合
    fab.inject_bad(0o000000)?; // 全零 i_mode 非法
    if fab.rej_total != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：类型不匹配注入——REG 位段按 DIR 类申报往返被拒。
    if fab.roundtrip(FileType7::Dir, 0o100644) != Err(E_TYPE_MIX) {
        return Err(E_TYPE_MIX);
    }
    hits += 1;

    // R6：槽账核对——七槽各得 1 + 10_000/7 余账按轮转分布，总量守恒。
    let mut sum = 0u32;
    for k in kinds.iter() {
        sum += fab.slot_rounds(*k)?;
    }
    if sum != fab.rounds_total {
        return Err(E_BAL);
    }
    if !fab.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：超界权限位合成（0o10000 高于低位掩码）被拒。
    if synth(FileType7::Reg, 0o10000) != Err(E_PERM_TOUCH) {
        return Err(E_PERM_TOUCH);
    }
    hits += 1;

    // I2：非法位段重复注入确认拦截稳定（decode 直呼路径）。
    if decode(0o170000) != Err(E_BAD_FMT) {
        return Err(E_BAD_FMT);
    }
    if decode(0o200000) != Err(E_BAD_FMT) {
        return Err(E_BAD_FMT);
    }
    hits += 1;

    Ok(hits)
}
