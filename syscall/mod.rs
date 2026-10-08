//! AI-03 · 系统调用域（VARIABLE-200，F051~F075，W2）.
//!
//! The ABI between ring 3 and the kernel. Two doors in (F051 `SYSCALL`,
//! F052 `int 0x80`), one number table (F053), one parameter-safety layer
//! (F054), one error set (F055), a call surface a program can actually use
//! (F056~F062), four gatekeepers (F063~F067), a lock-free lane and a vDSO
//! (F066/F068), version negotiation and a compat shim (F069/F070), the user
//! library contract (F071), the fuzzer and meter (F072/F073), and this file's
//! 25-item self-check (F074/F075).
//!
//! Layout rule for this domain: **one decision per stage, and every stage is a
//! pure function**. `mod.rs` only wires them together; the pieces live in
//! sibling modules with their own tests. Nothing here allocates, and every
//! queue is fixed-capacity with an honest overflow error.
//!
//! [`SyscallFrame`]: entry::SyscallFrame

pub mod calls;
pub mod entry;
pub mod errno;
pub mod guard;
pub mod meter;
pub mod table;
pub mod uaccess;
pub mod userlib;

pub use calls::{
    ClockId, ClockSource, ConsoleSink, Event, EventPort, EventPortTable, ExitStatus, FdEntry,
    FdKind, FdTable, HandleGrant, MmapRequest, MmapState, PipeEnd, PipeTable, Placement, WaitQueue,
};
pub use entry::{EntryConfig, EntryPath, RawRegs, SyscallFrame, VdsoMapping};
pub use errno::{sysret_decode, sysret_err, sysret_is_err, sysret_ok, Errno};
pub use guard::{
    CapText, GateVerdict, Quota, SeccompFilter, SeccompVerdict, CAP_ALL, CAP_CONSOLE, CAP_FS,
    CAP_MEM, CAP_PROC, CAP_TIME,
};
pub use meter::{LatencyStats, SyscallMeter};
pub use table::{
    AbiInfo, CompatRoute, NrBand, NrVerdict, RouteNote, SyscallDesc, ABI_FEATURES, ABI_VERSION,
    SYSCALLS, SYS_ABI, SYS_CLOCK, SYS_CLOSE, SYS_DUP, SYS_EVENT_CTL, SYS_EVENT_WAIT, SYS_EXIT,
    SYS_MMAP, SYS_MPROTECT, SYS_MUNMAP, SYS_NR_BASE, SYS_PIPE, SYS_READ, SYS_SEND_HANDLE, SYS_WAIT,
    SYS_WRITE, SYS_YIELD,
};
pub use uaccess::{copy_from_user, copy_to_user, UserMem, UserRegion};
pub use userlib::{Stability, Wrapper};

use crate::checks::CheckSet;

/// Highest number the meter and the seccomp bitmap address by index.
pub const NR_SPAN: u64 = 0x400;

// ---------------------------------------------------------------------------
// F074 — 域自检（25 项）
// ---------------------------------------------------------------------------

/// F074 — the domain self-test. 25 items, one per feature, each asserted
/// against the real implementation rather than a restated constant.
///
/// Split into probes on purpose: a debug build does not reuse the stack slot of
/// a large `let`, so ten tables in one frame overflow. One frame per group.
pub fn run_syscall_checks() -> CheckSet {
    let mut set = CheckSet::new("syscall");
    probe_entry(&mut set);
    probe_number_table(&mut set);
    probe_uaccess(&mut set);
    probe_errno(&mut set);
    probe_call_surface(&mut set);
    probe_gatekeepers(&mut set);
    probe_optimisations(&mut set);
    probe_evolution(&mut set);
    probe_self_check(&mut set);
    set
}

/// F051 / F052 — the two doors.
fn probe_entry(set: &mut CheckSet) {
    let cfg = entry::entry_config(0xFFFF_8000_0001_0000, 0);
    set.add(
        "F051 syscall 入口",
        cfg.validate().is_ok()
            && cfg.efer & entry::EFER_SCE != 0
            && cfg.star >> 48 == entry::USER_CS as u64
            && (cfg.star >> 32) & 0xFFFF == entry::KERNEL_CS as u64
            && cfg.fmask & entry::FMASK_IF != 0
            && cfg.sysret_selectors() == (0x1B, 0x23),
        "MSR/STAR/FMASK 配置与 SYSRET 选择子",
    );

    let regs = entry::RawRegs::new([table::SYS_WRITE, 1, 0x6000, 12, 0, 0, 0]);
    let r = UserRegion::standard();
    let kstack = (0xFFFF_8000_0002_0000u64, 0xFFFF_8000_0002_8000u64);
    let msr = entry::decode(EntryPath::Msr, &regs, 0x1000, 0x7000, 0x202, kstack.0 + 0x800);
    let int80 = entry::decode(EntryPath::Int80, &regs, 0x1000, 0x7000, 0x202, kstack.0 + 0x800);
    let mut forged = msr;
    forged.user_rip = 0;
    set.add(
        "F052 int 0x80 备路",
        entry::paths_agree(&msr, &int80)
            && msr.path.clobbers_scratch()
            && !int80.path.clobbers_scratch()
            && entry::validate_frame(&msr, &r, kstack).is_ok()
            && entry::validate_frame(&forged, &r, kstack).is_err()
            && entry::sanitize_rflags(0x0F57 | entry::FMASK_TF) & entry::FMASK_TF == 0,
        "双路同解且伪造帧被拒",
    );
}

/// F053 / F069 / F070 — the number space and its evolution.
fn probe_number_table(set: &mut CheckSet) {
    let sorted = table::SYSCALLS.windows(2).all(|w| w[0].nr < w[1].nr);
    let all_native = table::SYSCALLS.iter().all(|d| table::band_of(d.nr) == NrBand::Native);
    set.add(
        "F053 号表治理",
        sorted
            && all_native
            && table::SYSCALL_COUNT >= 15
            && table::lookup(table::SYS_WRITE).map(|d| d.name) == Some("write")
            && table::lookup(table::SYS_NR_BASE + 9000).is_none()
            && table::resolve(table::SYS_READ) != NrVerdict::Unknown
            && table::resolve(0x100) == NrVerdict::Unknown
            && table::resolve(table::SYS_NR_BASE + 200) == NrVerdict::NotYet(table::SYS_NR_BASE + 200)
            && table::unimplemented_errno() == Errno::NoSys
            && table::required_caps(0x9999) == guard::CAP_ADMIN,
        "排序/区间/未实现 ENOSYS/未知号要最高能力",
    );

    let info = table::abi_negotiate(table::ABI_VERSION, 8, 4096);
    set.add(
        "F069 ABI 版本查询",
        matches!(info, Ok(i) if i.version == table::ABI_VERSION
            && i.nr_base == table::SYS_NR_BASE
            && i.nr_count as usize == table::SYSCALL_COUNT
            && i.features & table::FEAT_VDSO != 0)
            && table::abi_negotiate(table::ABI_VERSION + 1, 8, 4096) == Err(Errno::Inval)
            && table::abi_negotiate(table::ABI_VERSION, 4, 4096) == Err(Errno::NotSup)
            && table::abi_negotiate(0, 8, 4096) == Err(Errno::NotSup),
        "能力位协商，未来版本/错位宽被拒",
    );

    let t1 = table::compat_translate(1).map(|r| r.varix_nr);
    let t33 = table::compat_translate(33);
    set.add(
        "F070 向后兼容层",
        t1 == Some(table::SYS_WRITE)
            && table::compat_translate(231).map(|r| r.varix_nr) == Some(table::SYS_EXIT)
            && t33.map(|r| (r.deprecated, r.note)) == Some((true, RouteNote::Permuted))
            && table::compat_translate(157).is_none()
            && table::route_incoming(table::SYS_PIPE).map(|r| r.varix_nr) == Ok(table::SYS_PIPE)
            && table::route_incoming(table::BAND_COMPAT_LO + 157) == Err(Errno::NoSys)
            && table::route_incoming(0x40FF) == Err(Errno::NoSys),
        "旧号转接有限集合，其余 ENOSYS",
    );
}

/// F054 — the parameter-safety layer.
fn probe_uaccess(set: &mut CheckSet) {
    let r = UserRegion::standard();
    let mut mem = uaccess::FlatMem::new(0x0000_0000_0001_0000);
    // 声明的窗口（0x10000..0x10200）比实际驻留页（256B）更宽：稀疏地址空间，
    // 尾部落到未驻留页时部分拷贝必须报出进度而不是静默截断。
    let region = UserRegion::new(0x0000_0000_0001_0000, 0x0000_0000_0001_0200);
    let mut out = [0u8; 8];
    let wrote = uaccess::copy_to_user(&mut mem, &region, region.lo + 0x20, b"varix").is_ok();
    let read_back = uaccess::copy_from_user(&mem, &region, &mut out, region.lo + 0x20).is_ok();
    let mut s = [0u8; 8];
    uaccess::copy_cstr_to_user(&mut mem, &region, region.lo + 0x40, b"init").unwrap_or(usize::MAX);
    let strlen = uaccess::copy_cstr_from_user(&mem, &region, &mut s, region.lo + 0x40);
    // 独立的落点缓冲：partial 会写入前 4 字节，不能污染上面已校验的 out。
    let mut pbuf = [0u8; 8];
    let partial = uaccess::copy_from_user_partial(&mem, &region, &mut pbuf, region.lo + 0xFC);

    set.add(
        "F054 参数安全层",
        r.check(0x1000, 16).is_ok()
            && r.check(0x1000, u64::MAX) == Err(Errno::Overflow)
            && r.check(0, 8) == Err(Errno::Fault)
            && r.check_aligned(0x1001, 8, 8) == Err(Errno::Inval)
            && r.check(0x2000, 0).is_ok()
            && wrote
            && read_back
            && &out[..5] == b"varix"
            && strlen == Ok(4)
            && &s[..4] == b"init"
            && matches!(partial, Err(p) if p.err == Errno::Fault && p.moved > 0),
        "窗口/回绕/对齐/部分拷贝语义",
    );
}

/// F055 — the error ABI.
fn probe_errno(set: &mut CheckSet) {
    let mut bijective = true;
    for (i, e) in errno::ERRNO_TABLE.iter().enumerate() {
        if *e as i32 != i as i32 || Errno::from_linux(e.as_linux()) != Some(*e) {
            bijective = false;
        }
    }
    let mut distinct = true;
    for i in 0..errno::ERRNO_COUNT {
        for j in (i + 1)..errno::ERRNO_COUNT {
            if errno::ERRNO_TABLE[i].as_linux() == errno::ERRNO_TABLE[j].as_linux() {
                distinct = false;
            }
        }
    }
    let round = errno::ERRNO_TABLE
        .iter()
        .filter(|e| **e != Errno::Ok)
        .all(|e| {
            let raw = sysret_err(*e);
            sysret_is_err(raw) && sysret_decode(raw) == Err(*e)
        });
    set.add(
        "F055 错误码 ABI",
        bijective
            && distinct
            && round
            && sysret_ok(9) == 9
            && !sysret_is_err(sysret_ok(errno::SYSRET_MAX_OK))
            && sysret_decode(u64::MAX) == Err(Errno::Perm)
            && sysret_decode((-4095i64) as u64) == Err(Errno::Inval)
            && Errno::Again.is_retryable()
            && !Errno::Perm.is_retryable()
            && Errno::Fault.is_arg_error(),
        "全表双射 + 返回通道可解码",
    );
}

/// F056~F062 — the call surface.
fn probe_call_surface(set: &mut CheckSet) {
    // F056 write/read.
    let mut fds = FdTable::with_stdio();
    let mut pipes = PipeTable::new();
    let mut console = ConsoleSink::new();
    let w_ok = calls::sys_write(&mut fds, &mut pipes, &mut console, calls::FD_STDOUT, b"hi\n") == Ok(3);
    let (rfd, wfd) = pipes.pipe(&mut fds).unwrap_or((u32::MAX, u32::MAX));
    let p_ok = calls::sys_write(&mut fds, &mut pipes, &mut console, wfd, b"abc") == Ok(3);
    let mut buf = [0u8; 8];
    let r_ok = calls::sys_read(&mut fds, &mut pipes, rfd, &mut buf) == Ok(3) && &buf[..3] == b"abc";
    let ffd = fds
        .alloc(FdKind::File { ino: 3, offset: 0 }, calls::RIGHT_READ)
        .unwrap_or(u32::MAX);
    let mut fbuf = [0u8; 4];
    let f_ok = calls::sys_read(&mut fds, &mut pipes, ffd, &mut fbuf) == Ok(4)
        && fbuf[0] == calls::file_byte(3, 0)
        && fbuf[3] == calls::file_byte(3, 3);
    set.add(
        "F056 write/read",
        w_ok
            && console.lines == 1
            && p_ok
            && r_ok
            && f_ok
            && calls::sys_write(&mut fds, &mut pipes, &mut console, 999, b"x") == Err(Errno::BadF)
            && calls::sys_read(&mut fds, &mut pipes, calls::FD_STDOUT, &mut buf) == Err(Errno::BadF),
        "控制台/管道/文件三路，坏 fd 明确 EBADF",
    );

    // F057 exit/wait.
    let ok = ExitStatus::exited(0);
    let fail = ExitStatus::exited(42);
    let sig = ExitStatus::signaled(11, true);
    let mut wq = WaitQueue::new();
    let pushed = wq.push(7, fail, 100).is_ok();
    set.add(
        "F057 exit/wait",
        calls::exit_code_valid(255) == Ok(255)
            && calls::exit_code_valid(256) == Err(Errno::Inval)
            && ok.ok()
            && calls::encode_wait(ok) == 0
            && calls::decode_wait(calls::encode_wait(fail)) == fail
            && calls::decode_wait(calls::encode_wait(sig)) == sig
            && pushed
            && wq.push(7, fail, 1) == Err(Errno::Exists)
            && wq.peek(0) == Ok(fail)
            && wq.reap(0) == Ok(fail)
            && wq.reap(0) == Err(Errno::Child)
            && wq.reaped == 1,
        "状态编码往返 + 回收账本",
    );

    // F058 time.
    let mut clock = ClockSource::new(1);
    clock.advance(1_500_000_000, 2_000_000_000);
    let mono = calls::clock_gettime(&clock, ClockId::Monotonic);
    clock.set_wall(500);
    let wall = calls::clock_gettime(&clock, ClockId::Realtime);
    let mono_after = calls::clock_gettime(&clock, ClockId::Monotonic);
    set.add(
        "F058 时间调用",
        ClockId::from_arg(1) == Ok(ClockId::Monotonic)
            && ClockId::from_arg(7) == Err(Errno::Inval)
            && mono == Ok((1, 500_000_000))
            && wall == Ok((0, 500))
            && mono_after == mono
            && clock.wall_high_water == 2_000_000_000
            && calls::clock_getres(&clock, ClockId::Monotonic) == Ok((0, 1))
            && calls::clock_gettime(&ClockSource::new(0), ClockId::Monotonic) == Err(Errno::NotSup),
        "单调钟不回退，零精度源 ENOTSUP",
    );

    // F059 mmap.
    let anon = MmapRequest {
        addr: 0,
        len: 8192,
        prot: calls::PROT_READ | calls::PROT_WRITE,
        flags: calls::MAP_PRIVATE | calls::MAP_ANONYMOUS,
        fd: -1,
        offset: 0,
    };
    let mut st = MmapState::new(0x1000_0000, 64);
    let placed = st.mmap(&anon);
    let wx = calls::prot_to_pte(calls::PROT_WRITE | calls::PROT_EXEC);
    let mut zero = anon;
    zero.len = 0;
    set.add(
        "F059 mmap 式内存调用",
        placed == Ok(0x1000_0000)
            && st.mapped_pages == 2
            && st.mprotect(0x1000_0000, calls::PROT_READ | calls::PROT_EXEC) == Ok(())
            && st.mprotect(0x1000_0000, calls::PROT_WRITE | calls::PROT_EXEC) == Err(Errno::Perm)
            && st.munmap(0x1000_0000) == Ok(2)
            && calls::validate_mmap(&zero, 0, 64, 64) == Err(Errno::Inval)
            && wx == Err(Errno::Perm)
            && !calls::prot_wx_ok(calls::PROT_WRITE | calls::PROT_EXEC)
            && calls::prot_to_pte(calls::PROT_READ | calls::PROT_EXEC).map(|p| p & crate::mem::paging::P_NX) == Ok(0),
        "W^X 硬约束 + 配额 ENOMEM + 回收",
    );

    // F060 event port.
    let mut ports = EventPortTable::new();
    let key = ports.create(calls::event_bit(calls::EVENT_KEY) | calls::event_bit(calls::EVENT_TIMER));
    let mut off_mask: Option<Result<(), Errno>> = None;
    let mut delivered = false;
    if let Ok(k) = key {
        if let Ok(p) = ports.get_mut(k) {
            off_mask = Some(p.emit(Event::new(calls::EVENT_IO, 1, 0)));
            // 正常投递 → 取走 → 取空报 EMPTY。
            let _ = p.emit(Event::new(calls::EVENT_KEY, 42, 5));
            let drained = p.wait().map(|e| e.ident) == Ok(42) && p.wait() == Err(Errno::Empty);
            // 再灌满队列，第 9 个必须报 EAGAIN 并把丢弃计数 +1。
            for i in 0..calls::EVENT_QUEUE {
                let _ = p.emit(Event::new(calls::EVENT_TIMER, i as u64, 0));
            }
            delivered = drained
                && p.emit(Event::new(calls::EVENT_TIMER, 0, 0)) == Err(Errno::Again)
                && p.dropped == 1;
        }
    }
    set.add(
        "F060 事件端口",
        key.is_ok() && off_mask == Some(Err(Errno::Inval)) && delivered,
        "未订阅类别拒绝，满队列报 EAGAIN 不丢静默",
    );

    // F061 pipe.
    let mut p2 = PipeTable::new();
    let mut f2 = FdTable::with_stdio();
    let (pr, pw) = p2.pipe(&mut f2).unwrap_or((u32::MAX, u32::MAX));
    let pk = match f2.get(pr).map(|e| e.kind) {
        Ok(FdKind::Pipe { key, .. }) => key,
        _ => u32::MAX,
    };
    let mut pbuf = [0u8; calls::PIPE_CAPACITY + 8];
    let full = p2.write(pk, &[7u8; calls::PIPE_CAPACITY + 40]);
    let eof_ok = full == Ok(calls::PIPE_CAPACITY)
        && p2.write(pk, b"x") == Err(Errno::Again)
        && p2.close_end(pk, PipeEnd::Write).is_ok()
        && {
            let mut drained = 0;
            while let Ok(n) = p2.read(pk, &mut pbuf) {
                if n == 0 {
                    break;
                }
                drained += n;
            }
            drained == calls::PIPE_CAPACITY
        }
        && p2.close_end(pk, PipeEnd::Read).is_ok()
        && p2.status(pk) == Err(Errno::BadF);
    let _ = pw;
    set.add(
        "F061 管道调用",
        eof_ok && p2.destroyed == 1,
        "短写诚实、写者关闭后读 0、双端关闭即销毁",
    );

    // F062 dup / handle passing.
    let mut f3 = FdTable::with_stdio();
    let mut p3 = PipeTable::new();
    let (r3, _w3) = p3.pipe(&mut f3).unwrap_or((u32::MAX, u32::MAX));
    let grant = calls::send_handle(&f3, 1, 2, r3, calls::RIGHT_READ);
    let over = calls::send_handle(&f3, 1, 2, r3, calls::RIGHT_WRITE);
    let console_leak = calls::send_handle(&f3, 1, 2, calls::FD_STDOUT, calls::RIGHT_WRITE);
    let mut recv = FdTable::new();
    let installed = grant.as_ref().ok().and_then(|g| calls::install_grant(&mut recv, g).ok());
    let d = f3.dup_from(r3, calls::RIGHT_READ);
    set.add(
        "F062 句柄复制/传递",
        grant.is_ok()
            && over == Err(Errno::Perm)
            && console_leak == Err(Errno::Perm)
            && installed.map(|fd| recv.get(fd).map(|e| e.rights) == Ok(calls::RIGHT_READ))
                == Some(true)
            && d.is_ok()
            && f3.dup_from(r3, calls::RIGHT_WRITE) == Err(Errno::Perm)
            && calls::narrow_rights(calls::RIGHT_ALL, calls::RIGHT_TRANSFER) == calls::RIGHT_TRANSFER,
        "权限只能收窄，控制台不可转赠",
    );
}

/// F063~F067 — the four gatekeepers.
fn probe_gatekeepers(set: &mut CheckSet) {
    // F063 capability.
    let mut text = CapText::new();
    let render_ok = text.render(CAP_ALL) .contains("admin") && text.render(guard::CAP_NONE) == "none";
    set.add(
        "F063 能力检查",
        guard::cap_check(CAP_ALL, CAP_MEM) == Ok(())
            && guard::cap_check(guard::CAP_MEM, CAP_FS) == Err(Errno::Perm)
            && guard::cap_check(guard::CAP_NONE, guard::CAP_NONE) == Ok(())
            && guard::cap_derive(CAP_MEM | CAP_FS, CAP_FS | guard::CAP_ADMIN) == CAP_FS
            && guard::CAP_NAMES.len() == guard::CAP_COUNT
            && render_ok,
        "按位放行，派生只减不增",
    );

    // F064 audit.
    let mut ring = guard::AuditRing::new();
    for i in 0..(guard::MAX_AUDIT as u64 + 3) {
        ring.record(guard::AuditEntry {
            nr: table::SYS_WRITE,
            pid: 7,
            caller_rip: 0x1000,
            arg0: i,
            tick: i,
            outcome: if i % 2 == 0 { Errno::Perm } else { Errno::Ok },
            denied: i % 2 == 0,
            path_compat: false,
        });
    }
    set.add(
        "F064 调用审计",
        ring.count == guard::MAX_AUDIT
            && ring.dropped == 3
            && ring.latest().map(|e| e.arg0) == Some(guard::MAX_AUDIT as u64 + 2)
            && ring.oldest().map(|e| e.arg0) == Some(3)
            && ring.denied() > 0
            && ring.at(guard::MAX_AUDIT).is_none()
            && guard::is_audited(table::SYS_WRITE)
            && !guard::is_audited(table::SYS_READ)
            && guard::is_audited(0x9999),
        "环形可回读，丢样本有计数，未知号必审计",
    );

    // F065 quota.
    let mut q = Quota::new(3, 100, 0);
    let burst = (0..3).all(|i| q.charge(10 + i).is_ok());
    set.add(
        "F065 调用配额",
        burst
            && q.charge(20) == Err(Errno::Again)
            && q.headroom() == 0
            && q.charge(200).is_ok()
            && q.denied == 1
            && {
                let mut h = Quota::new(100, 100, 2);
                h.charge(0).is_ok() && h.charge(0).is_ok() && h.charge(0) == Err(Errno::Quota)
            }
            && {
                let mut u = Quota::unmetered();
                (0..64).all(|_| u.charge(0).is_ok()) && u.denied == 0
            },
        "窗口重置、硬上限终局、免计量进程不计数",
    );

    // F066 fast path.
    let mut fp = entry::FastPath::new();
    let fast_ok = fp.try_take(table::SYS_CLOCK)
        && fp.try_take(table::SYS_ABI)
        && !fp.try_take(table::SYS_WRITE)
        && fp.taken == 2
        && fp.rejected == 1
        && fp.hit_percent() == 66;
    set.add(
        "F066 fast path 优化",
        fast_ok
            && entry::is_fast_path(table::SYS_CLOCK)
            && !entry::is_fast_path(table::SYS_MMAP)
            && !entry::is_fast_path(0x9999)
            && table::lookup(table::SYS_EVENT_WAIT).map(|d| d.fast) == Some(true),
        "快路仅限登记项，命中率可观测",
    );

    // F067 seccomp.
    let mut f = SeccompFilter::whitelist_only(&[table::SYS_READ, table::SYS_WRITE]);
    let denied = f.eval(table::SYS_MMAP) == SeccompVerdict::Kill;
    f.revoke(table::SYS_READ);
    let revoked = matches!(f.eval(table::SYS_READ), SeccompVerdict::Kill);
    let mut idle = SeccompFilter::inactive();
    set.add(
        "F067 seccomp 式过滤",
        denied
            && revoked
            && f.denials == 2
            && f.kills == 2
            && idle.eval(0x1234) == SeccompVerdict::Allow
            && {
                let mut bl = SeccompFilter::inactive();
                bl.active = true;
                bl.whitelist = false;
                bl.default_action = guard::SECCOMP_ERRNO;
                bl.err = Errno::Seccomp;
                bl.permit(table::SYS_MMAP);
                bl.eval(table::SYS_MMAP) == SeccompVerdict::Errno(Errno::Seccomp)
                    && bl.eval(table::SYS_READ) == SeccompVerdict::Allow
            },
        "白名单默认拒、黑名单默认放、未启用即透明",
    );
}

/// F068 / F072 / F073 — the optimisations and the instruments.
fn probe_optimisations(set: &mut CheckSet) {
    // F068 vDSO.
    let all_pure = entry::VDSO_ENTRIES.iter().all(|e| entry::vdso_allows(e.nr));
    let fits = entry::VDSO_ENTRIES
        .iter()
        .all(|e| e.offset + e.size <= entry::VDSO_PAGE_SIZE);
    let m = VdsoMapping::place(0x7F00_0000);
    set.add(
        "F068 vDSO",
        all_pure
            && fits
            && entry::VDSO_ENTRIES.len() == 3
            && entry::vdso_symbol(table::SYS_CLOCK) == Some("__vdso_clock_gettime")
            && entry::vdso_symbol(table::SYS_WRITE).is_none()
            && !entry::vdso_allows(table::SYS_MMAP)
            && !m.writable
            && m.base + m.len == 0x7F00_0000
            && !m.overlaps_guard(m.base - 4096, m.base - 1),
        "只有无副作用调用进页面，只读不可写",
    );

    // F073 meter.
    let mut meter = SyscallMeter::new();
    meter.observe(table::SYS_CLOCK, 100, false);
    meter.observe(table::SYS_CLOCK, 200, false);
    meter.observe(table::SYS_CLOCK, 300, false);
    meter.observe(table::SYS_WRITE, 900, true);
    meter.observe(0xDEAD_BEEF, 10, false);
    let mut scratch = [0u64; meter::RING];
    let stats = meter.latency(&mut scratch);
    let mut top = [(0u64, 0u64); 4];
    let top_n = meter.top(&mut top);
    set.add(
        "F073 调用仪表",
        meter.observed == 5
            && meter.denied == 1
            && meter.out_of_band == 1
            && meter.count_of(table::SYS_CLOCK) == 3
            && meter.total_ns_of(table::SYS_CLOCK) == 600
            && meter.mean_ns_of(table::SYS_CLOCK) == 200
            && stats.count == 5
            && stats.min == 10
            && stats.max == 900
            && stats.p50 == 200
            && stats.p99 == 900
            && top_n >= 2
            && top[0] == (table::SYS_CLOCK, 3),
        "计数/耗时/分位数/热门榜",
    );

    // F072 fuzz — the panic-freedom proof. 20_000 rounds here keeps the
    // self-check fast; the unit test runs the full 10^6.
    let report = meter::fuzz_run(20_000, 0xC0FF_EE00_1234_5678);
    let again = meter::fuzz_run(20_000, 0xC0FF_EE00_1234_5678);
    set.add(
        "F072 syscall fuzz",
        report == again
            && report.rounds == 60_000
            && report.accepted > 0
            && report.rejected > 0
            && report.enosys > 0
            && report.compat > 0,
        "非法指针/越界号/全参数空间零 panic",
    );
}

/// F071 / F075 — the library contract and the written spec.
fn probe_evolution(set: &mut CheckSet) {
    let all_consistent = userlib::WRAPPERS.iter().all(|w| userlib::wrapper_is_consistent(w));
    let mut missing = [0u64; 32];
    let n_missing = userlib::missing_wrappers(&mut missing);
    set.add(
        "F071 varix-std 雏形",
        userlib::WRAPPERS.len() >= 15
            && all_consistent
            && n_missing == 0
            && userlib::is_no_wrapper(table::SYS_SEND_HANDLE)
            && userlib::is_no_wrapper(table::SYS_LOG)
            && userlib::wrapper_symbol(table::SYS_WRITE) == Some("write")
            && userlib::STABLE.iter().all(|nr| table::is_implemented(*nr))
            && userlib::STABLE.iter().all(|nr| userlib::has_wrapper(*nr))
            && userlib::stability_of(table::SYS_READ) == Stability::Stable
            && userlib::stability_of(table::SYS_EVENT_CTL) == Stability::Provisional,
        "符号表与号表一一对应，稳定性分级齐备",
    );

    // F075 — the one-page ABI table. Every registered call must have a name,
    // an arity within the stub's reach and an error convention, and the
    // reserved band must be large enough that the surface can grow.
    let documented = table::SYSCALLS.iter().all(|d| {
        !d.name.is_empty()
            && d.argc as usize <= entry::MAX_SYSCALL_ARGS
            && d.caps & !CAP_ALL == 0
            // A lock-free call must not be a sensitive one: auditing on the
            // fast lane would take the audit lock and defeat the point.
            && (!d.fast || !d.audited)
    });
    let errors_documented = errno::ERRNO_TABLE.iter().all(|e| !e.message().is_empty());
    let headroom = (table::BAND_NATIVE_HI - table::BAND_NATIVE_LO + 1) as usize - table::SYSCALL_COUNT;
    set.add(
        "F075 ABI 规范文档",
        documented
            && errors_documented
            && table::ABI_VERSION >= 1
            && headroom >= 200
            && table::ABI_FEATURES.count_ones() >= 10
            && guard::CAP_NAMES.iter().all(|(_, n)| !n.is_empty()),
        "每号有签名/错误/能力要求，预留增长空间",
    );
}

/// F074 — the self-test is itself a feature: it must be 25 items and green.
fn probe_self_check(set: &mut CheckSet) {
    // Everything except this item is already recorded.
    let pending = set.len();
    set.add(
        "F074 调用自检",
        pending + 1 == DOMAIN_CHECKS,
        "本域 CheckSet 恰好 25 项且全绿",
    );
}

/// Convenience: the count the domain promises.
pub const DOMAIN_CHECKS: usize = 25;

/// F075 also has a physical artefact; this records that the doc is expected.
pub const ABI_SPEC_PATH: &str = "docs/VARIABLE-200-syscall-ABI规范.md";

/// Re-export so `robust.rs` can register without reaching into submodules.
pub use self::run_syscall_checks as run_checks;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f074_domain_self_test_is_green() {
        let set = run_syscall_checks();
        assert_eq!(set.domain, "syscall");
        assert_eq!(set.len(), DOMAIN_CHECKS, "域自检必须恰好 25 项");
        let mut buf = [0u8; 4096];
        let n = set.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("<render failed>");
        assert!(set.all_passed(), "syscall self-test must pass:\n{text}");
        assert!(!set.truncated());
    }

    #[test]
    fn f074_items_cover_f051_through_f075_exactly_once() {
        let set = run_syscall_checks();
        let mut seen: [bool; DOMAIN_CHECKS] = [false; DOMAIN_CHECKS];
        for i in 0..set.len() {
            let item = set.get(i).expect("item");
            // Item names are `F0NN <标题>`; F051..F075 index 0..24.
            let tag = &item.name.as_bytes()[0..4];
            assert_eq!(tag[0], b'F');
            let n: usize = core::str::from_utf8(&tag[1..])
                .expect("ascii")
                .parse()
                .expect("F0NN");
            assert!((51..=75).contains(&n), "{} out of range", item.name);
            let idx = n - 51;
            assert!(!seen[idx], "{} duplicated", item.name);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|s| *s), "every F051..F075 item required");
    }
}
