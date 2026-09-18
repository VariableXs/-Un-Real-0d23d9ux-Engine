# -*- coding: utf-8 -*-
"""任务41 ring3.rs 补丁：notepad.pe 实例 + 消息注入 + 闭环校验"""
import io

P = 'kernel/varix/src/proc/ring3.rs'
s = io.open(P, encoding='utf-8').read()

def rep(old, new, tag):
    global s
    assert old in s, 'ANCHOR MISS: ' + tag
    assert s.count(old) == 1, 'ANCHOR DUP: ' + tag
    s = s.replace(old, new, 1)

# ---------- 1) static 样例 + 只跑一轮位 ----------
rep(
"""/// 任务40 带导入 PE 实例只跑一轮。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static IMP_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);""",
"""/// 任务40 带导入 PE 实例只跑一轮。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static IMP_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// 任务41（AI-B）· 记事本闭环 PE64（tools/make-pe-notepad.py 生成，随源入库）：
/// 4 DLL 16 导入（kernel32/user32/gdi32/comdlg32），完整 Win32 消息循环——
/// 注册窗口 → 建窗 → 消息泵 → WM_PAINT(GDI TextOut)/WM_CHAR(编辑)/
/// WM_COMMAND(打开/保存/退出) → WM_QUIT → ExitProcess。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static NOTEPAD_PE: &[u8] = include_bytes!("notepad.pe");

/// 记事本实例只跑一轮。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
static NOTEPAD_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);""",
    'statics')

# ---------- 2) 续体链挂载：IMP_DONE 后进 notepad；notepad 后进压力探针 ----------
rep(
"""    if !IMP_DONE.swap(true, Ordering::Relaxed) {
        spawn_pe_imports();
    }
    pressure_probe_and_finish()
}""",
"""    if !IMP_DONE.swap(true, Ordering::Relaxed) {
        spawn_pe_imports();
    }
    // 任务41（AI-B）：带导入 PE 后运行记事本闭环实例——探针注入
    // 打开/编辑/保存/退出消息序列，PE 侧消息泵全链消费（总案波次验收
    // 「记事本能打开写字保存」）；exit 后回到本续体先校验保存内容再进
    // 压力探针。
    if !NOTEPAD_DONE.swap(true, Ordering::Relaxed) {
        spawn_pe_notepad();
    }
    notepad_verify_and_finish()
}""",
    'chain')

# ---------- 3) spawn_pe_notepad（照 spawn_pe_imports 模式） ----------
rep(
"""/// 压力探针（总案任务15 验收：spawn 压力 64 槽打满/耗尽优雅拒绝）：""",
"""/// 任务41（AI-B）· 装载记事本 PE64 并进入 ring3：任务40 同一条导入绑定
/// 链（plan 全量解析 + install thunk/IAT 补钉），spawn 前预置虚拟文件与
/// 探针消息序列（菜单打开 → 字符 V/X → 菜单保存 → 菜单退出）——PE 侧
/// 消息泵全链消费；exit 后续体 [`notepad_verify_and_finish`] 校验闭环。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn spawn_pe_notepad() -> ! {
    // 预置「文件系统」+ 探针消息序列（先于 PE 运行全部入队）。
    super::winsrv::with_service(|s| {
        s.files.write(b"boot.txt", b"from boot");
        let hwnd_pre = 0u32; // 真实 hwnd 由 CreateWindowExW 运行期分配；探针消息以 0  hwnd 投递（PE switch 只看 message/wparam）。
        s.queue.post(super::winsrv::Msg { hwnd: hwnd_pre, message: super::winsrv::WM_COMMAND, wparam: super::winsrv::IDM_OPEN, lparam: 0 });
        s.queue.post(super::winsrv::Msg { hwnd: hwnd_pre, message: super::winsrv::WM_CHAR, wparam: b'V' as u64, lparam: 0 });
        s.queue.post(super::winsrv::Msg { hwnd: hwnd_pre, message: super::winsrv::WM_CHAR, wparam: b'X' as u64, lparam: 0 });
        s.queue.post(super::winsrv::Msg { hwnd: hwnd_pre, message: super::winsrv::WM_COMMAND, wparam: super::winsrv::IDM_SAVE, lparam: 0 });
        s.queue.post(super::winsrv::Msg { hwnd: hwnd_pre, message: super::winsrv::WM_COMMAND, wparam: super::winsrv::IDM_EXIT, lparam: 0 });
    });

    let img = match super::pe::parse(NOTEPAD_PE) {
        Ok(i) => i,
        Err(e) => {
            crate::kwarn!("ring3: notepad.pe parse failed: {} — halting", e.as_str());
            halt_demo()
        }
    };
    let bind = match super::winapi::plan(&img, NOTEPAD_PE) {
        Ok(t) => t,
        Err(e) => {
            crate::kwarn!("ring3: notepad import bind refused: {} — halting", e.as_str());
            halt_demo()
        }
    };
    let pid = match PROCS.lock().alloc(0, b"notepad") {
        Ok(p) => p,
        Err(e) => {
            crate::kwarn!("ring3: notepad proc alloc failed: {:?} — halting", e);
            halt_demo()
        }
    };
    DEMO_PID.store(pid, core::sync::atomic::Ordering::Relaxed);
    let mut mp = super::loader::KernelMapper::new(crate::mem::pfh::target_ops());
    let src = super::pe::PeSource { img: &img, blob: NOTEPAD_PE };
    let mut loaded = match super::loader::load_into(
        &src,
        &mut mp,
        entry::DEFAULT_STACK_PAGES as u64,
        entry::USER_STACK_TOP,
    ) {
        Ok(l) => l,
        Err(e) => {
            PROCS.lock().exit(pid, -1, 0);
            crate::kwarn!("ring3: notepad load failed ({:?}) — halting", e);
            halt_demo()
        }
    };
    let (full, partial, stub) = match super::winapi::install(&bind.patches, &mut loaded, &mut mp) {
        Ok(s) => s,
        Err(e) => {
            PROCS.lock().exit(pid, -1, 0);
            crate::kwarn!("ring3: notepad bind install failed: {} — halting", e.as_str());
            halt_demo()
        }
    };
    crate::kinfo!(
        "ring3: notepad bound pid={} dlls={} funcs={} full={} partial={} stub={} thunk@{:#x}",
        pid,
        bind.dlls,
        bind.funcs,
        full,
        partial,
        stub,
        super::winapi::WINAPI_THUNK_BASE
    );
    let pages_n = loaded.pages.len();
    let (entry_ip, stack_top) = (loaded.entry, loaded.stack_top);
    CHILD_PAGES.lock().push((pid, loaded.pages));
    crate::kinfo!(
        "ring3: notepad spawned pid={} entry={:#x} pages={} — 任务41 记事本闭环",
        entry_ip,
        pages_n,
        pid = pid,
    );
    let mut plan = super::uspace::EntryPlan::default();
    if let Err(e) = plan_entry(EntryPath::Iret, entry_ip, stack_top, &mut plan) {
        crate::kwarn!("ring3: notepad entry plan rejected: {} — halting", e);
        halt_demo()
    }
    crate::kinfo!(
        "ring3: iretq → notepad user (rip={:#x} rsp={:#x} rflags={:#x})",
        plan.rip,
        plan.rsp,
        plan.rflags
    );
    // SAFETY: 同 spawn_pe_imports——loader 已映射段页/栈页与 thunk 页。
    unsafe { enter_user(plan.rip, plan.rsp, plan.rflags) }
}

/// 任务41 · 记事本闭环校验（PE exit 后续体）：保存目标内容必须等于
/// 「打开内容 + 探针注入字符」＝ b"from bootVX"；轨迹（对话框/画布）落
/// 串口证据行后进压力探针收尾。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn notepad_verify_and_finish() -> ! {
    let (saved, edit_len, trace, pixels) = super::winsrv::with_service(|s| {
        let saved = s.files.read(b"boot.txt").map(|d| d.to_vec());
        (saved, s.edit.len, s.trace.as_slice().to_vec(), s.canvas.buf.iter().filter(|&&p| p != 0).count())
    });
    let ok = saved.as_deref() == Some(b"from bootVX".as_slice());
    crate::kinfo!(
        "notepad: trace=[{}] edit_units={} canvas_px={} saved={}",
        core::str::from_utf8(&trace).unwrap_or("?"),
        edit_len,
        pixels,
        core::str::from_utf8(saved.as_deref().unwrap_or(b"?")).unwrap_or("?")
    );
    if ok {
        crate::kinfo!("notepad: open→edit→save closed loop VERDICT=PASS — 任务41 记事本闭环（总案波次验收）");
    } else {
        crate::kwarn!("notepad: closed loop VERDICT=FAIL — saved content mismatch");
    }
    pressure_probe_and_finish()
}

/// 压力探针（总案任务15 验收：spawn 压力 64 槽打满/耗尽优雅拒绝）：""",
    'spawn_pe_notepad')

io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
print('ring3 patched,', len(s.splitlines()), 'lines')
