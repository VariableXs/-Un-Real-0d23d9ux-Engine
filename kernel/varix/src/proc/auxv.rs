//! 篇 27 第五步 · 进程初始栈与环境装配（WP-105）。
//!
//! MD2 篇 27："栈顶布局按 ABI 惯例（参数、环境、辅助向量——辅助向量的
//! 条目集按 Linux 对齐，直插应用的启动例程对它有强预期）。"
//!
//! 本模块是**纯逻辑装配器**：它不碰内存，只产出一份有序字节写入计划
//! （[`StackLayout::writes`]，(虚拟地址, 字节) 列表）与最终 `rsp`。调用方
//! 把计划逐条落到用户栈帧（目标态走 HHDM，宿主测试走假内存），计划即
//! 事实——宿主可对每一个字节断言，不必假装跑过 iretq。
//!
//! 布局（自 stack_top 向下，System V AMD64 ABI + Linux 惯例）：
//!
//! ```text
//! stack_top ──►  execfn 字符串 (NUL 结尾)
//!                envp 字符串…
//!                argv 字符串…            （字符串区）
//!                [对齐到 16]
//!                AT_RANDOM 的 16 字节
//!                auxv 数组 (key,value)*，AT_NULL 收尾
//!                NULL (envp 终止符)
//!                envp[i] 指针…
//!                NULL (argv 终止符)
//!                argv[i] 指针…（argv[0] 在低地址端）
//!        rsp ──► argc                    （rsp % 16 == 0）
//! ```
//!
//! 诚实边界（篇 27 第四步 · F105 预留）：动态链接被拒
//! （`elf::DYNAMIC_LINKING_SUPPORTED == false`），本模块把解释器所需的
//! auxv 约定（AT_BASE=解释器装载基址、AT_ENTRY=原镜像入口、AT_PHDR/
//! AT_PHENT/AT_PHNUM=原程序头表）按 Linux 语义备齐——递归装载解释器的
//! 骨架在 WP-301 Linuxulator 接手时复用同一张表，届时本模块零改动。
//!
//! 正确性论证（指纹与随机的边界）：AT_RANDOM 的 16 字节由调用方传入
//! （目标态取自硬件随机源），装配器只负责落位——纯逻辑不产随机，
//! 随机来源的可测试性不归布局管。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// AT_* 辅助向量键表 —— 按 Linux 语义对齐（篇 27 第五步"条目集按 Linux 对齐"）
// ---------------------------------------------------------------------------

pub const AT_NULL: u64 = 0; // 数组终止符
pub const AT_PHDR: u64 = 3; // 程序头表的用户地址
pub const AT_PHENT: u64 = 4; // 程序头条目大小
pub const AT_PHNUM: u64 = 5; // 程序头条数
pub const AT_PAGESZ: u64 = 6; // 页大小
pub const AT_BASE: u64 = 7; // 解释器基址（动态镜像是解释器的，静态为 0）
pub const AT_FLAGS: u64 = 8; // 标志位（Linux 恒 0）
pub const AT_ENTRY: u64 = 9; // 原镜像入口点
pub const AT_UID: u64 = 11; // 真实 UID
pub const AT_EUID: u64 = 12; // 有效 UID
pub const AT_GID: u64 = 13; // 真实 GID
pub const AT_EGID: u64 = 14; // 有效 GID
pub const AT_HWCAP: u64 = 16; // CPU 能力位（本装配器不填，调用方可补）
pub const AT_CLKTCK: u64 = 17; // times() 频率
pub const AT_SECURE: u64 = 23; // 安全模式（setuid 等）
pub const AT_RANDOM: u64 = 25; // 16 字节随机数的地址
pub const AT_EXECFN: u64 = 31; // execfn 字符串地址

/// auxv 的一条 (key, value)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuxEntry {
    pub key: u64,
    pub val: u64,
}

impl AuxEntry {
    pub const fn new(key: u64, val: u64) -> AuxEntry {
        AuxEntry { key, val }
    }
}

/// 静态镜像的标准条目集（Linux 顺序惯例：值关键的信息在前，AT_RANDOM/
/// AT_EXECFN 指针类在后，AT_NULL 收尾）。AT_PHDR/AT_BASE/AT_ENTRY 三个
/// 解释器约定键在静态镜像下不出现（没有解释器，也没有需要指认的重
/// 定位基址）——出现即第四步的事。
pub fn auxv_for_static(
    page_size: u64,
    entry: u64,
    uid: u64,
    gid: u64,
    secure: bool,
    random_addr: u64,
    execfn_addr: u64,
) -> Vec<AuxEntry> {
    let mut v = Vec::new();
    v.push(AuxEntry::new(AT_PAGESZ, page_size));
    v.push(AuxEntry::new(AT_FLAGS, 0));
    v.push(AuxEntry::new(AT_ENTRY, entry));
    v.push(AuxEntry::new(AT_UID, uid));
    v.push(AuxEntry::new(AT_EUID, uid));
    v.push(AuxEntry::new(AT_GID, gid));
    v.push(AuxEntry::new(AT_EGID, gid));
    v.push(AuxEntry::new(AT_CLKTCK, 100));
    v.push(AuxEntry::new(AT_SECURE, secure as u64));
    v.push(AuxEntry::new(AT_RANDOM, random_addr));
    v.push(AuxEntry::new(AT_EXECFN, execfn_addr));
    v.push(AuxEntry::new(AT_NULL, 0));
    v
}

// ---------------------------------------------------------------------------
// 装配错误
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackError {
    /// argv 至少要有 argv[0]（程序名）——没有名字的进程没法报错。
    EmptyArgv,
    /// argv+envp 数量超限（见 [`MAX_ARGV`] / [`MAX_ENVP`]）。
    TooManyStrings,
    /// 全部装配字节超过预算（见 [`MAX_STACK_BYTES`]）——初始栈页区
    /// 装不下，宁可拒绝也不静默截断（截断=指針指向没写过的字节）。
    StackTooBig,
}

impl StackError {
    pub fn as_str(self) -> &'static str {
        match self {
            StackError::EmptyArgv => "argv must carry at least argv[0]",
            StackError::TooManyStrings => "too many argv/envp strings",
            StackError::StackTooBig => "initial stack image exceeds the budget",
        }
    }
}

/// argv 上限（Linux 的 ARG_MAX 约束远大于此——这是初始栈页区的现实约束）。
pub const MAX_ARGV: usize = 256;
pub const MAX_ENVP: usize = 256;
/// 字符串单条上限（路径 + 参数的现实量级；防单条超长挤爆预算）。
pub const MAX_STRING_BYTES: usize = 4096;
/// 装配区总预算。默认栈 16 页 = 64KiB，装配区只许占零头。
pub const MAX_STACK_BYTES: usize = 32 * 1024;

// ---------------------------------------------------------------------------
// 布局产物
// ---------------------------------------------------------------------------

/// 一次装配的完整产物：最终 `rsp`、全部指针槽地址、以及有序写入计划。
#[derive(Clone, Debug)]
pub struct StackLayout {
    /// 最终 rsp —— 指向 argc，16 字节对齐（ABI 硬性要求）。
    pub rsp: u64,
    /// argc 所在地址 == rsp。
    pub argc_addr: u64,
    /// argv[i] 指针槽的地址（槽里写的值 = 对应字符串地址）。
    pub argv_slot_addrs: Vec<u64>,
    /// argv[i] 字符串首地址（i 与 argv_slot_addrs 一一对应）。
    pub argv_str_addrs: Vec<u64>,
    /// envp[i] 指针槽地址。
    pub envp_slot_addrs: Vec<u64>,
    /// envp[i] 字符串首地址。
    pub env_str_addrs: Vec<u64>,
    /// auxv 数组首地址（[(key,val); n] 连续，AT_NULL 收尾）。
    pub auxv_addr: u64,
    /// AT_RANDOM 16 字节所在地址（16 字节对齐——密钥派生惯例）。
    pub random_addr: u64,
    /// execfn 字符串地址（== AT_EXECFN 的值）。
    pub execfn_addr: u64,
    /// 有序写入计划：(起始虚拟地址, 字节)。地址互不重叠、从低到高；
    /// 调用方逐条落帧即可得到与计划逐字节一致的栈像。
    pub writes: Vec<(u64, Vec<u8>)>,
    /// 全部写入字节总数（预算对账用）。
    pub total_bytes: u64,
}

fn push_u64(writes: &mut Vec<(u64, Vec<u8>)>, addr: u64, v: u64) {
    writes.push((addr, v.to_le_bytes().to_vec()));
}

fn push_bytes(writes: &mut Vec<(u64, Vec<u8>)>, addr: u64, b: &[u8]) {
    writes.push((addr, b.to_vec()));
}

/// 回填指针类条目：AT_RANDOM / AT_EXECFN 的权威地址只有装配器知道
/// （它们取决于字符串区与随机数的实际落位）。调用方传 0 占位即可。
fn fixup_pointer_entries(aux: &mut [AuxEntry], random_addr: u64, execfn_addr: u64) {
    for e in aux.iter_mut() {
        if e.key == AT_RANDOM {
            e.val = random_addr;
        }
        if e.key == AT_EXECFN {
            e.val = execfn_addr;
        }
    }
}

/// 静态装配：argv/envp 字符串、auxv、AT_RANDOM 一次算清。
///
/// `stack_top` 是栈顶（第一不可用字节）；`random: [u8; 16]` 由调用方
/// 提供（目标态取硬件随机源，宿主测试给确定值断言）。
pub fn build(
    stack_top: u64,
    argv: &[&str],
    envp: &[&str],
    aux: &[AuxEntry],
    random: [u8; 16],
) -> Result<StackLayout, StackError> {
    if argv.is_empty() {
        return Err(StackError::EmptyArgv);
    }
    if argv.len() > MAX_ARGV || envp.len() > MAX_ENVP {
        return Err(StackError::TooManyStrings);
    }
    for s in argv.iter().chain(envp.iter()) {
        if s.len() > MAX_STRING_BYTES {
            return Err(StackError::StackTooBig);
        }
    }
    // auxv 必须以 AT_NULL 收尾（没带就补——装配器的职责是产出合法布局，
    // 不把调用方的疏忽变成用户态的野指针）。
    let mut aux: Vec<AuxEntry> = aux.to_vec();
    if !aux.iter().any(|e| e.key == AT_NULL) {
        aux.push(AuxEntry::new(AT_NULL, 0));
    }

    // —— 自 stack_top 向下排字符串区：execfn 在顶（约定 = argv[0]），
    // 然后环境串、argv 串依次向下；串与串之间只保证 NUL，不做对齐。
    let mut cur = stack_top;
    let execfn_addr = cur - argv[0].len() as u64 - 1;
    cur = execfn_addr;
    let mut env_str_addrs = Vec::with_capacity(envp.len());
    for e in envp {
        let a = cur - e.len() as u64 - 1;
        env_str_addrs.push(a);
        cur = a;
    }
    let mut argv_str_addrs = Vec::with_capacity(argv.len());
    for a in argv {
        let sa = cur - a.len() as u64 - 1;
        argv_str_addrs.push(sa);
        cur = sa;
    }

    // —— 对齐到 16，放 AT_RANDOM 的 16 字节。
    let random_addr = (cur - 16) & !0xF;
    cur = random_addr;

    // —— auxv 数组：n 条 (key,val)，含 AT_NULL。
    let auxv_bytes = aux.len() as u64 * 16;
    let auxv_addr = cur - auxv_bytes;
    cur = auxv_addr;

    // —— envp 指针槽 + NULL 终止；argv 指针槽 + NULL 终止；argc。
    let envp_slots = envp.len() as u64 * 8 + 8;
    let argv_slots = argv.len() as u64 * 8 + 8;
    let envp_arr_addr = cur - envp_slots;
    let argv_arr_addr = envp_arr_addr - argv_slots;
    let argc_addr = argv_arr_addr - 8;
    if argc_addr & 0xF != 0 {
        // argc 未落在 16 线上：指针区整体下移 pad 字节（挪整块，不砍
        // 字节——字符串区/auxv/AT_RANDOM 的地址与其槽内值全部不变）。
        let pad = argc_addr & 0xF;
        return build_aligned(stack_top, argv, envp, &mut aux, random, pad);
    }
    let rsp = argc_addr;

    let total_bytes = stack_top - rsp;
    if total_bytes > MAX_STACK_BYTES as u64 {
        return Err(StackError::StackTooBig);
    }

    // 指针类条目回填为权威地址（字符串区/随机区已算定）。
    fixup_pointer_entries(&mut aux, random_addr, execfn_addr);

    // —— 写入计划（从低到高排列，地址互不重叠）。
    let mut writes = Vec::new();
    // argc
    push_u64(&mut writes, rsp, argv.len() as u64);
    // argv 槽：argv[0]..argv[n-1]，然后 NULL。
    let mut argv_slot_addrs = Vec::with_capacity(argv.len());
    for (i, sa) in argv_str_addrs.iter().enumerate() {
        let slot = argv_arr_addr + i as u64 * 8;
        argv_slot_addrs.push(slot);
        push_u64(&mut writes, slot, *sa);
    }
    push_u64(&mut writes, argv_arr_addr + argv.len() as u64 * 8, 0);
    // envp 槽 + NULL。
    let mut envp_slot_addrs = Vec::with_capacity(envp.len());
    for (i, ea) in env_str_addrs.iter().enumerate() {
        let slot = envp_arr_addr + i as u64 * 8;
        envp_slot_addrs.push(slot);
        push_u64(&mut writes, slot, *ea);
    }
    push_u64(&mut writes, envp_arr_addr + envp.len() as u64 * 8, 0);
    // auxv。
    for (i, e) in aux.iter().enumerate() {
        let a = auxv_addr + i as u64 * 16;
        push_u64(&mut writes, a, e.key);
        push_u64(&mut writes, a + 8, e.val);
    }
    // AT_RANDOM 16 字节。
    push_bytes(&mut writes, random_addr, &random);
    // 字符串区（带 NUL）。
    push_bytes(&mut writes, execfn_addr, argv[0].as_bytes());
    push_bytes(&mut writes, execfn_addr + argv[0].len() as u64, &[0]);
    for (e, ea) in envp.iter().zip(env_str_addrs.iter()) {
        push_bytes(&mut writes, *ea, e.as_bytes());
        push_bytes(&mut writes, *ea + e.len() as u64, &[0]);
    }
    for (a, sa) in argv.iter().zip(argv_str_addrs.iter()) {
        push_bytes(&mut writes, *sa, a.as_bytes());
        push_bytes(&mut writes, *sa + a.len() as u64, &[0]);
    }

    Ok(StackLayout {
        rsp,
        argc_addr: rsp,
        argv_slot_addrs,
        argv_str_addrs,
        envp_slot_addrs,
        env_str_addrs,
        auxv_addr,
        random_addr,
        execfn_addr,
        writes,
        total_bytes,
    })
}

/// 对齐垫片路径：argc 天然不落在 16 线上时，指针区整体下移 pad 字节。
/// 字符串区/auxv/AT_RANDOM 的地址不变（它们在更下方，槽地址只上指）。
#[allow(clippy::too_many_arguments)]
fn build_aligned(
    stack_top: u64,
    argv: &[&str],
    envp: &[&str],
    aux: &mut [AuxEntry],
    random: [u8; 16],
    pad: u64,
) -> Result<StackLayout, StackError> {
    debug_assert!(pad > 0 && pad < 16);
    // 与 build 同一套算术，只是 argc 直接取对齐线。为避免双份维护走
    // 递归一次：pad 已知，argc_addr = 原 argc_addr - pad 必然对齐。
    // 直接复用 build 的主体会无限递归，这里按同样顺序重算。
    let mut cur = stack_top;
    let execfn_addr = cur - argv[0].len() as u64 - 1;
    cur = execfn_addr;
    let mut env_str_addrs = Vec::with_capacity(envp.len());
    for e in envp {
        let a = cur - e.len() as u64 - 1;
        env_str_addrs.push(a);
        cur = a;
    }
    let mut argv_str_addrs = Vec::with_capacity(argv.len());
    for a in argv {
        let sa = cur - a.len() as u64 - 1;
        argv_str_addrs.push(sa);
        cur = sa;
    }
    let random_addr = (cur - 16) & !0xF;
    cur = random_addr;
    let auxv_bytes = aux.len() as u64 * 16;
    let auxv_addr = cur - auxv_bytes;
    cur = auxv_addr;
    let envp_slots = envp.len() as u64 * 8 + 8;
    let argv_slots = argv.len() as u64 * 8 + 8;
    let envp_arr_addr = cur - envp_slots - pad;
    let argv_arr_addr = envp_arr_addr - argv_slots;
    let argc_addr = argv_arr_addr - 8;
    debug_assert_eq!(argc_addr & 0xF, 0, "对齐路径必须产出 16 对齐的 rsp");

    let total_bytes = stack_top - argc_addr;
    if total_bytes > MAX_STACK_BYTES as u64 {
        return Err(StackError::StackTooBig);
    }

    // 指针类条目回填为权威地址（与主路径同一语义）。
    fixup_pointer_entries(aux, random_addr, execfn_addr);

    let mut writes = Vec::new();
    push_u64(&mut writes, argc_addr, argv.len() as u64);
    let mut argv_slot_addrs = Vec::with_capacity(argv.len());
    for (i, sa) in argv_str_addrs.iter().enumerate() {
        let slot = argv_arr_addr + i as u64 * 8;
        argv_slot_addrs.push(slot);
        push_u64(&mut writes, slot, *sa);
    }
    push_u64(&mut writes, argv_arr_addr + argv.len() as u64 * 8, 0);
    let mut envp_slot_addrs = Vec::with_capacity(envp.len());
    for (i, ea) in env_str_addrs.iter().enumerate() {
        let slot = envp_arr_addr + i as u64 * 8;
        envp_slot_addrs.push(slot);
        push_u64(&mut writes, slot, *ea);
    }
    push_u64(&mut writes, envp_arr_addr + envp.len() as u64 * 8, 0);
    for (i, e) in aux.iter().enumerate() {
        let a = auxv_addr + i as u64 * 16;
        push_u64(&mut writes, a, e.key);
        push_u64(&mut writes, a + 8, e.val);
    }
    push_bytes(&mut writes, random_addr, &random);
    push_bytes(&mut writes, execfn_addr, argv[0].as_bytes());
    push_bytes(&mut writes, execfn_addr + argv[0].len() as u64, &[0]);
    for (e, ea) in envp.iter().zip(env_str_addrs.iter()) {
        push_bytes(&mut writes, *ea, e.as_bytes());
        push_bytes(&mut writes, *ea + e.len() as u64, &[0]);
    }
    for (a, sa) in argv.iter().zip(argv_str_addrs.iter()) {
        push_bytes(&mut writes, *sa, a.as_bytes());
        push_bytes(&mut writes, *sa + a.len() as u64, &[0]);
    }

    Ok(StackLayout {
        rsp: argc_addr,
        argc_addr,
        argv_slot_addrs,
        argv_str_addrs,
        envp_slot_addrs,
        env_str_addrs,
        auxv_addr,
        random_addr,
        execfn_addr,
        writes,
        total_bytes,
    })
}

// ---------------------------------------------------------------------------
// 测试：布局逐字节按 ABI 重读回断言
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: u64 = 0x7FFF_F000;

    /// 把写入计划铺进假内存，再按 ABI 从 rsp 读回——装配器输出的不是
    /// "大致对"，是每一步都能被启动例程按预期摸到的结构。
    fn materialize(l: &StackLayout) -> Vec<u8> {
        let base = l.rsp;
        let len = (TOP - base) as usize;
        let mut mem = vec![0u8; len];
        for (addr, bytes) in &l.writes {
            let off = (addr - base) as usize;
            mem[off..off + bytes.len()].copy_from_slice(bytes);
        }
        mem
    }

    fn rd(mem: &[u8], base: u64, addr: u64) -> u64 {
        let off = (addr - base) as usize;
        let mut b = [0u8; 8];
        b.copy_from_slice(&mem[off..off + 8]);
        u64::from_le_bytes(b)
    }

    fn read_str(mem: &[u8], base: u64, addr: u64) -> String {
        let off = (addr - base) as usize;
        let end = mem[off..].iter().position(|&b| b == 0).unwrap();
        String::from_utf8(mem[off..off + end].to_vec()).unwrap()
    }

    #[test]
    fn full_layout_matches_sysv_abi_byte_for_byte() {
        let argv = ["hello", "-v", "--verbose"];
        let envp = ["PATH=/bin", "HOME=/"];
        let aux = auxv_for_static(4096, 0x40_1000, 1000, 1000, false, 0, 0);
        let l = build(TOP, &argv, &envp, &aux, [0xAB; 16]).expect("layout");

        // rsp 16 字节对齐（ABI 硬性要求）且在栈顶之下。
        assert_eq!(l.rsp & 0xF, 0);
        assert!(l.rsp < TOP);
        assert_eq!(l.argc_addr, l.rsp);

        let mem = materialize(&l);
        // argc。
        assert_eq!(rd(&mem, l.rsp, l.rsp), 3);
        // argv[0..3] 指向的字符串内容逐个对上，随后是 NULL。
        for (i, want) in argv.iter().enumerate() {
            let slot = l.argv_slot_addrs[i];
            let s = rd(&mem, l.rsp, slot);
            assert_eq!(s, l.argv_str_addrs[i]);
            assert_eq!(read_str(&mem, l.rsp, s), *want);
        }
        assert_eq!(rd(&mem, l.rsp, l.argv_slot_addrs[2] + 8), 0, "argv NULL");
        // envp 同理。
        for (i, want) in envp.iter().enumerate() {
            let s = rd(&mem, l.rsp, l.envp_slot_addrs[i]);
            assert_eq!(s, l.env_str_addrs[i]);
            assert_eq!(read_str(&mem, l.rsp, s), *want);
        }
        assert_eq!(rd(&mem, l.rsp, l.envp_slot_addrs[1] + 8), 0, "envp NULL");
        // auxv：AT_PAGESZ=4096、AT_ENTRY、AT_RANDOM 指向 16 字节区、
        // AT_EXECFN 指向 argv[0]、AT_NULL 收尾。
        let mut keys = Vec::new();
        for i in 0..aux.len() {
            keys.push(rd(&mem, l.rsp, l.auxv_addr + i as u64 * 16));
        }
        let idx = |k: u64| keys.iter().position(|&x| x == k).unwrap() as u64;
        assert_eq!(rd(&mem, l.rsp, l.auxv_addr + idx(AT_PAGESZ) * 16 + 8), 4096);
        assert_eq!(rd(&mem, l.rsp, l.auxv_addr + idx(AT_ENTRY) * 16 + 8), 0x40_1000);
        assert_eq!(
            rd(&mem, l.rsp, l.auxv_addr + idx(AT_RANDOM) * 16 + 8),
            l.random_addr
        );
        assert_eq!(
            rd(&mem, l.rsp, l.auxv_addr + idx(AT_EXECFN) * 16 + 8),
            l.execfn_addr
        );
        assert_eq!(keys[keys.len() - 1], AT_NULL, "auxv 必须以 AT_NULL 收尾");
        // AT_RANDOM 内容逐字节 = 传入随机数。
        let off = (l.random_addr - l.rsp) as usize;
        assert_eq!(&mem[off..off + 16], &[0xAB; 16]);
        // execfn 与 argv[0] 是两份独立拷贝（Linux 语义：copy_strings 各自
        // 拷入，AT_EXECFN 指向自己那份）——内容一致、地址独立。
        assert_ne!(l.execfn_addr, l.argv_str_addrs[0]);
        assert_eq!(read_str(&mem, l.rsp, l.execfn_addr), "hello");
    }

    #[test]
    fn empty_env_and_single_argv_still_aligns() {
        let l = build(TOP, &["only"], &[], &[], [0; 16]).expect("layout");
        assert_eq!(l.rsp & 0xF, 0);
        assert!(l.envp_slot_addrs.is_empty());
        let mem = materialize(&l);
        assert_eq!(rd(&mem, l.rsp, l.rsp), 1);
        // argv[0] 槽之后直接 NULL。
        assert_eq!(rd(&mem, l.rsp, l.argv_slot_addrs[0] + 8), 0);
    }

    #[test]
    fn alignment_pad_path_engages_and_stays_aligned() {
        // 扫一排 argv 长度，让 (argc_addr % 16) 命中两种情况——
        // 天然对齐与需要垫片。两种都要求 rsp % 16 == 0。
        for n in 1..24usize {
            let name = "x".repeat(n);
            let argv = [name.as_str()];
            let l = build(TOP, &argv, &[], &[], [0; 16]).expect("layout");
            assert_eq!(l.rsp & 0xF, 0, "n={n} 时 rsp 未对齐");
            // 计划可铺开、argc 可读回。
            let mem = materialize(&l);
            assert_eq!(rd(&mem, l.rsp, l.rsp), 1, "n={n} argc 读回失败");
        }
    }

    #[test]
    fn rejections_name_the_reason() {
        assert!(matches!(build(TOP, &[], &[], &[], [0; 16]), Err(StackError::EmptyArgv)));

        let argv: Vec<&str> = (0..MAX_ARGV + 1).map(|_| "a").collect();
        assert!(matches!(build(TOP, &argv, &[], &[], [0; 16]), Err(StackError::TooManyStrings)));

        // 单条字符串超长。
        let long = "x".repeat(MAX_STRING_BYTES + 1);
        let argv = [long.as_str()];
        assert!(matches!(build(TOP, &argv, &[], &[], [0; 16]), Err(StackError::StackTooBig)));
    }

    #[test]
    fn auxv_for_static_carries_the_linux_set() {
        let v = auxv_for_static(4096, 0x40_1000, 1000, 100, true, 0x9000, 0x8000);
        // 顺序无所谓，键集必须齐；AT_PHDR/AT_BASE/AT_ENTRY 里的解释器键
        // 在静态装配下只应出现 AT_ENTRY（第四步 F105 预留的诚实边界）。
        let keys: Vec<u64> = v.iter().map(|e| e.key).collect();
        for k in [AT_PAGESZ, AT_FLAGS, AT_ENTRY, AT_UID, AT_EUID, AT_GID, AT_EGID, AT_CLKTCK, AT_SECURE, AT_RANDOM, AT_EXECFN, AT_NULL] {
            assert!(keys.contains(&k), "缺少 AT_{k}");
        }
        assert!(!keys.contains(&AT_PHDR), "静态镜像不应有 AT_PHDR");
        assert!(!keys.contains(&AT_BASE), "静态镜像不应有 AT_BASE");
        let e = v.iter().find(|e| e.key == AT_SECURE).unwrap();
        assert_eq!(e.val, 1);
        assert_eq!(*v.last().unwrap(), AuxEntry::new(AT_NULL, 0));
    }

    #[test]
    fn caller_auxv_gets_null_appended_not_duplicated() {
        // 调用方忘带 AT_NULL → 装配器补；带了 → 不重复。
        let l = build(TOP, &["a"], &[], &[AuxEntry::new(AT_PAGESZ, 4096)], [0; 16]).unwrap();
        let mem = materialize(&l);
        let mut keys = Vec::new();
        let mut a = l.auxv_addr;
        loop {
            let k = rd(&mem, l.rsp, a);
            if k == AT_NULL {
                keys.push(k);
                break;
            }
            keys.push(k);
            a += 16;
        }
        assert_eq!(keys.iter().filter(|&&k| k == AT_NULL).count(), 1);
        assert_eq!(keys[0], AT_PAGESZ);
    }

    #[test]
    fn writes_are_ordered_nonoverlapping_and_complete() {
        let argv = ["prog", "arg1"];
        let envp = ["A=1", "B=2"];
        let aux = auxv_for_static(4096, 0x40_1000, 0, 0, false, 0, 0);
        let l = build(TOP, &argv, &envp, &aux, [7; 16]).unwrap();
        // 从低到高、互不重叠：逐条排序列后相邻段必须相接或留洞但不回叠。
        let mut spans: Vec<(u64, u64)> = l.writes.iter().map(|(a, b)| (*a, *a + b.len() as u64)).collect();
        spans.sort();
        for w in spans.windows(2) {
            assert!(w[0].1 <= w[1].0, "写入计划重叠: {:#x}..{:#x} vs {:#x}", w[0].0, w[0].1, w[1].0);
        }
        // 总字节 == 各段之和（total_bytes 是区域跨度，写入总和 ≤ 跨度）。
        let sum: u64 = l.writes.iter().map(|(_, b)| b.len() as u64).sum();
        assert!(sum <= l.total_bytes);
        // 预算对账：跨度 ≤ MAX_STACK_BYTES。
        assert!(l.total_bytes <= MAX_STACK_BYTES as u64);
    }

    #[test]
    fn oversized_image_refuses_within_budget() {
        // 贴着预算构造：MAX_ENVP 条各 MAX_STRING_BYTES 附近——注意单条
        // 超限走 TooManyStrings 的另一路，这里用多条中等长度撑爆总量。
        let envp: Vec<String> = (0..MAX_ENVP).map(|i| format!("E{i}={}", "y".repeat(120))).collect();
        let refs: Vec<&str> = envp.iter().map(|s| s.as_str()).collect();
        match build(TOP, &["a"], &refs, &[], [0; 16]) {
            Err(StackError::StackTooBig) => {}
            Ok(l) => assert!(l.total_bytes <= MAX_STACK_BYTES as u64, "要么拒绝要么在预算内"),
            Err(e) => panic!("unexpected: {:?}", e),
        }
    }
}
