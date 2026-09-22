//! Varix kernel image entry point (F001~F025 boot sequence).
//!
//! Limine (base revision 1) calls `_start` as a plain C function on the BSP
//! with a valid stack; every request response is already filled in. The boot
//! sequence below brackets each stage into the timeline (F024), advances the
//! honest progress bar (F014) and finishes with the self-test (F025).

#![no_std]
#![no_main]

use varix::timeline::{Stage, TIMELINE};

extern crate varix;

#[no_mangle]
extern "C" fn _start() -> ! {
    boot();
}

/// Number of timeline stages = progress bar total (F014 truth mapping).
const TOTAL_STAGES: usize = 14;

/// Scratch buffer for the "bootloader name + version" banner line. Kept as a
/// `static` so the banner can borrow a `&'static str` out of it.
const ZERO_U8: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static BL_LINE: [core::sync::atomic::AtomicU8; 64] = [ZERO_U8; 64];

fn boot() -> ! {
    TIMELINE.begin_boot(varix::timeline::read_tsc());

    // --- serial (F005/F006) ------------------------------------------------
    TIMELINE.stage_begin(Stage::Serial, varix::timeline::read_tsc());
    varix::logger::early_init();
    TIMELINE.stage_end(Stage::Serial, varix::timeline::read_tsc());

    // --- cmdline (F017) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Cmdline, varix::timeline::read_tsc());
    let cmdline = varix::cmdline::init();
    varix::logger::apply_cmdline(cmdline.source());
    TIMELINE.stage_end(Stage::Cmdline, varix::timeline::read_tsc());
    varix::kinfo!("cmdline: {}", cmdline.source());

    // bootopt 必须**在菜单读取它之前**装载：`bootopt::options()` 读的是全局
    // `OPTS`，而 `OPTS` 只由 `bootopt::init()` 写入。历史上 init 被放在
    // 第 381 行（boot-select 菜单之后），于是菜单读到的永远是编译期默认值
    // `timeout_secs = 5` —— **cmdline 的 `boot_timeout=` / `boot_default=`
    // 被静默丢弃**（实测：配置 60s 却只倒计时 3~5s 就自动进默认项）。
    // 这直接损害「用户自主选择要进入的系统」：窗口小到真机键盘来不及响应。
    // cmdline 解析完就近装载，语义与 F022 一致（cmdline 覆盖编译期默认），
    // 后续第 381 行的 init() 变成幂等二次调用（OnceLock::set 首次为准）。
    TIMELINE.stage_begin(Stage::BootOpt, varix::timeline::read_tsc());
    let boot_opts_cmdline = varix::bootopt::init();
    TIMELINE.stage_end(Stage::BootOpt, varix::timeline::read_tsc());
    varix::kinfo!(
        "bootopt: default={} timeout={}s (from cmdline)",
        boot_opts_cmdline.default_entry,
        boot_opts_cmdline.timeout_secs
    );

    // --- framebuffer (F003/F004) --------------------------------------------
    TIMELINE.stage_begin(Stage::Framebuffer, varix::timeline::read_tsc());
    let surface = varix::limine::framebuffer().and_then(|fb| {
        varix::fb::Surface::from_limine(fb).ok()
    });
    // Backdrop belongs to framebuffer bring-up: the logo blends into it, so it
    // must exist before the mark is rasterized.
    if let Some(s) = surface.as_ref() {
        varix::banner::paint_backdrop(s);
    }
    TIMELINE.stage_end(Stage::Framebuffer, varix::timeline::read_tsc());
    let surface = match surface {
        Some(s) => s,
        None => {
            varix::kerror!("no usable framebuffer — serial-only boot");
            finish_no_fb();
        }
    };

    // --- 显示服务收编（任务20：Surface 归口；boot 期单核直写模式）-----------
    // boot 链的绘制面唯一来源（draw_surface=前台）；双缓冲+脏矩形提交服务
    // 由 display_probe 用独立 Double 实例实机验证（future shell 接入口）。
    let mut display_svc = varix::displaysrv::DisplayService::new_direct(surface);
    varix::displaysrv::install(&mut display_svc);

    // --- boot select（双域总案·阶段0）--------------------------------------
    // 菜单在帧缓冲就绪后、域初始化前亮出：↑/↓/Enter 实时选择（任务1），
    // 倒计时归零走默认项。选中 windows → 写 UEFI BootNext + ResetSystem
    // （任务2）；选中 uefi → 直接重启进固件设置。
    let mut boot_opts = varix::bootopt::options();
    // 任务4：boot-select.json 配置读取（路径参数注入；当前经引导卷模块
    // 通道读取——limine.conf module_path 挂载；SHARED 分区真盘 FS 于
    // 任务17/18 落地后替换读取实现，解析与容错零改动）。
    let mut cfg_buf = [0u8; 4096];
    let (boot_cfg, cfg_src) = varix::bootcfg::load(
        Some(&mut |p, b| varix::bootcfg::read_via_limine(p, b)),
        varix::bootcfg::SHARED_BOOT_SELECT_PATH,
        &mut cfg_buf,
    );
    if cfg_src == varix::bootcfg::CfgSource::Reset {
        varix::kwarn!("boot-cfg: shared config corrupt — built-in defaults applied (badge shown)");
        varix::bootselect::set_cfg_reset_badge(true);
    }
    boot_opts = varix::bootcfg::effective(boot_opts, &boot_cfg, cfg_src);
    let mut chosen_id: Option<&'static str> = None;
    // 需求 2：A 卡（varix）也要被记下来——它的终点不是内核自绘 ushell，
    // 而是「交接给 Windows 上的 Variable」（见 handoff::plan）。未显示菜单时
    // 按配置默认项判定，避免"纯 B 路"被误判成 A 卡交接。
    let mut chosen_entry: &'static str = boot_opts.default_entry;
    if boot_opts.menu_visible() {
        let tsc_hz = varix::platform::info()
            .map(|p| p.tsc_hz)
            .unwrap_or(varix::platform::FALLBACK_TSC_HZ);
        // 诊断：菜单亮出前把实际生效的超时与 cmdline 打到串口——
        // 「倒计时起始值」是 boot_timeout 是否被解析的硬证据。
        varix::kinfo!(
            "boot-diag: timeout={} default={} customized={} handoff={} cmdline='{}'",
            boot_opts.timeout_secs,
            boot_opts.default_entry,
            boot_opts.customized,
            boot_opts.handoff_to_variable,
            varix::cmdline::init().source()
        );
        // 鼠标 bring-up 必须排在菜单亮出**之前**（需求 1/6「任意鼠标和键盘」）：
        // PS/2 鼠标复位后默认**不上报数据**，不显式开 AUX 门 + 使能上报就
        // 永远收不到包。此前这一步只挂在 QEMU 探针分支里、且探针排在菜单
        // 之后——真机上鼠标从头到尾没初始化，插着也只能当摆设。
        // 三步全程 TSC 限次自旋，无鼠标/控制器异常也只是超时返回 0，不挂引导。
        let mouse_ack = varix::inputsvc::target::mouse_init(tsc_hz);
        varix::kinfo!(
            "boot-diag: mouse bringup set-defaults={} enable-report={}",
            mouse_ack & 0x1 != 0,
            mouse_ack & 0x2 != 0
        );
        let sel = varix::bootselect::run_countdown(&surface, boot_opts.timeout_secs, tsc_hz);
        // 菜单用完就把键源订阅槽**还回去**：`MAX_SUBS` 只有 4 个，菜单留着
        // 不还的话后面的 input_probe 槽位自检填不满、ushell 的输入订阅也会
        // 失败（实测回归：`usrshell: inputsvc subscribe failed — 键盘输入不可达`）。
        // 幂等，交接与降级两条路都安全。
        let released = varix::inputsvc::target::menu_release_key_source();
        varix::kinfo!("boot-select: key-source slot released={}", released);
        let chosen = varix::bootselect::ENTRIES[sel].id;
        chosen_entry = chosen;
        varix::kinfo!("boot-select: entry={}", chosen);
        if chosen == "windows" {
            // BootNext 写入需要可执行的 Runtime Services 映射，推迟到
            // cpu/mem 域初始化之后再执行（见下方 boot_next_action）。
            chosen_id = Some(chosen);
        } else if chosen == "uefi" {
            chosen_id = Some(chosen);
        }
    }

    // --- logo (F023) ----------------------------------------------------------
    // 菜单退出后先重绘背板：清掉选择页残影再落 logo（任务3 视觉收口）。
    TIMELINE.stage_begin(Stage::Logo, varix::timeline::read_tsc());
    varix::banner::paint_backdrop(&surface);
    let (lcx, lcy, lsize) = varix::logo::metrics_for(&surface);
    let logo_bottom = varix::logo::draw(&surface, lcx, lcy, lsize);
    TIMELINE.stage_end(Stage::Logo, varix::timeline::read_tsc());

    // --- banner (F013) --------------------------------------------------------
    TIMELINE.stage_begin(Stage::Banner, varix::timeline::read_tsc());
    let firmware = varix::limine::BootInfo::collect();
    let bl_line = {
        let (name, version) = varix::limine::bootloader_info().unwrap_or(("Limine", "?"));
        let mut m = 0usize;
        let mut push = |b: u8| {
            if m < BL_LINE.len() {
                BL_LINE[m].store(b, core::sync::atomic::Ordering::Relaxed);
                m += 1;
            }
        };
        for &b in name.as_bytes() {
            push(b);
        }
        push(b' ');
        for &b in version.as_bytes() {
            push(b);
        }
        // SAFETY: `BL_LINE` is a `static`, so the view is `'static`; it is
        // written once here on the single-threaded boot path.
        let view: &'static [u8] =
            unsafe { core::slice::from_raw_parts(BL_LINE.as_ptr().cast::<u8>(), m) };
        core::str::from_utf8(view).unwrap_or("Limine")
    };
    let hud_y = varix::banner::draw_boot_banner_at(
        &surface,
        firmware.firmware.as_str(),
        bl_line,
        logo_bottom + 12,
    );
    TIMELINE.stage_end(Stage::Banner, varix::timeline::read_tsc());

    // --- console (F011/F012) -------------------------------------------------
    TIMELINE.stage_begin(Stage::Console, varix::timeline::read_tsc());
    varix::console::init(surface);
    varix::console::enable_mirror();
    TIMELINE.stage_end(Stage::Console, varix::timeline::read_tsc());

    // --- platform (F019/F020) ------------------------------------------------
    TIMELINE.stage_begin(Stage::Platform, varix::timeline::read_tsc());
    let platform = varix::platform::detect();
    TIMELINE.stage_end(Stage::Platform, varix::timeline::read_tsc());
    varix::kinfo!(
        "cpu: {} fam{} mod{} step{} @{}MHz",
        platform.vendor.as_str(),
        platform.signature.family,
        platform.signature.model,
        platform.signature.stepping,
        platform.tsc_hz / 1_000_000
    );

    // --- acpi (F008/F009) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Acpi, varix::timeline::read_tsc());
    let acpi = varix::acpi::init();
    TIMELINE.stage_end(Stage::Acpi, varix::timeline::read_tsc());
    match acpi {
        Some(state) => varix::kinfo!(
            "acpi: rev {} tables {} cpus {} ioapics {}",
            state.revision,
            state.table_count,
            state.enabled_cpus,
            state.ioapics
        ),
        None => varix::kwarn!("acpi: RSDP not found"),
    }

    // --- smbios (F010) ---------------------------------------------------------
    TIMELINE.stage_begin(Stage::Smios, varix::timeline::read_tsc());
    let smbios = varix::smbios::init();
    TIMELINE.stage_end(Stage::Smios, varix::timeline::read_tsc());
    if let Some(info) = smbios {
        varix::kinfo!(
            "smbios: {} {} / {} {}",
            info.manufacturer_str(),
            info.product_str(),
            info.bios_vendor_str(),
            info.bios_version_str()
        );
    }

    // --- memmap (F015/F016) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Memmap, varix::timeline::read_tsc());
    match varix::memmap::init() {
        Some(state) => {
            varix::kinfo!(
                "memmap: {} entries, {} usable",
                state.entry_count,
                state.usable_bytes
            );
            varix::memmap::register_boot_reservations();
        }
        None => varix::kwarn!("memmap: no memory map from bootloader"),
    }
    TIMELINE.stage_end(Stage::Memmap, varix::timeline::read_tsc());

    // --- kaslr (F018) -------------------------------------------------------------
    TIMELINE.stage_begin(Stage::Kaslr, varix::timeline::read_tsc());
    varix::kaslr::init();
    TIMELINE.stage_end(Stage::Kaslr, varix::timeline::read_tsc());
    let (pool, seeded) = varix::kaslr::pool_state();
    varix::kinfo!("kaslr: seeded={} pool={:#x}", seeded, pool);

    // --- cpu / interrupt domain (F026~F050) --------------------------------------------
    // Runs after the platform probe (CPUID), the ACPI/MADT parse and the memory
    // map, because it consumes all three. Interrupts stay disabled here — the
    // scheduler domain owns `sti`.
    let cpu_state = varix::cpu::init();
    varix::cpu::render_to_console(&cpu_state);

    // --- memory management domain (F051~F075) ------------------------------------------
    // Needs the framebuffer reservation (F016), the boot reservations and the
    // platform topology, so it runs after AI-01's boot stages and after the
    // CPU domain has reported its core count.
    let mem_state = varix::mem::init();
    varix::mem::render_to_console(&mem_state);

    // --- 任务12 · #PF handler 内核自检 --------------------------------------
    // 登记 scratch 区后故意踩出真 #PF：MapZero/GrowStack/COW 三路端到端
    // 走通（Guard/Fault 路由由宿主测试+诊断路径覆盖）。idt 已在 cpu::init
    // 装好，vector 14 的决策路径此时开始生效。
    let pf_pass = varix::mem::pfh::target_selftest();
    if pf_pass < 3 {
        varix::kwarn!("pf: selftest below pass bar ({}/4)", pf_pass);
    }

    // --- boot-select 非默认项执行点（双域总案·阶段0 任务2）-----------------
    // 内存域上线后再调 UEFI Runtime Services：SetVariable/ResetSystem 需要
    // 可执行、已映射的运行期区域；此前调用会在部分固件上三重故障复位。
    if let Some(chosen) = chosen_id {
        if chosen == "uefi" {
            // 进固件设置走 UEFI 标准通道 OsIndications（置 BOOT_TO_FW_UI 后冷
            // 重启）；固件不支持该变量时退回普通冷重启并如实告知——绝不把
            // 「重启了」当成「进设置界面了」。
            let _ = varix::bootnext::prepare_runtime_identity_map();
            let blocks = varix::bootnext::identity_map_low_4gib();
            varix::kinfo!("boot-select: low-memory identity-mapped ({} x 2MiB)", blocks);
            varix::kinfo!("boot-select: requesting firmware setup via OsIndications");
            if !varix::bootnext::boot_to_firmware_ui() {
                varix::kwarn!(
                    "boot-select: OsIndications unsupported — falling back to plain cold reset"
                );
                if !varix::bootnext::reset_cold() {
                    varix::kwarn!("firmware reset unavailable on this firmware — continuing varix");
                }
            }
        } else if chosen == "windows" {
            let _ = varix::bootnext::prepare_runtime_identity_map();
            let blocks = varix::bootnext::identity_map_low_4gib();
            varix::kinfo!("boot-select: low-memory identity-mapped ({} x 2MiB)", blocks);
            // 项号**不写死**：按固件 BootOrder 逐个读 Boot#### 匹配 Windows；
            // handoff_target=usb 时只认设备路径含 U 盘 ESP GUID 的项（S1.3
            // 登记制）。GUID 缺失时回退任意 Windows 项——手动选择用户在场，
            // 沿用下方既有 guessing 语义（自动交接路径则是硬拒绝，见 handoff）。
            let usb_guid = if boot_opts.handoff_target == varix::bootopt::HandoffTarget::Usb {
                if boot_opts.usb_windows_esp_guid.is_none() {
                    varix::kwarn!(
                        "boot-select: handoff_target=usb but usb_windows_esp_guid missing — \
                         falling back to any Windows entry (manual selection)"
                    );
                }
                boot_opts.usb_windows_esp_guid.as_ref()
            } else {
                None
            };
            let entry =
                varix::bootnext::resolve_windows_entry_for(varix::cmdline::init().source(), usb_guid);
            if entry.verified() {
                varix::kinfo!(
                    "boot-select: Windows boot option resolved to 0x{:04X}",
                    entry.number()
                );
            } else {
                varix::kwarn!(
                    "boot-select: no Windows boot option found in BootOrder — guessing 0x{:04X} \
                     (pass boot_next=<num> on the kernel cmdline to pin it)",
                    entry.number()
                );
            }
            match varix::bootnext::write_bootnext(entry.number()) {
                varix::bootnext::BootNextOutcome::Written { entry } => {
                    varix::kinfo!(
                        "boot-select: BootNext=0x{:04X} written & verified — resetting",
                        entry
                    );
                    if varix::bootnext::reset_cold() {
                        // ResetSystem 正常不返回；保险停在死循环（中断未开）。
                        loop {
                            core::hint::spin_loop();
                        }
                    }
                    varix::kwarn!("firmware reset unavailable — continuing varix");
                }
                varix::bootnext::BootNextOutcome::NoRuntimeServices => {
                    varix::kwarn!(
                        "boot-select: BIOS boot — UEFI BootNext unavailable, continuing varix"
                    );
                }
                other => {
                    varix::kwarn!("boot-select: BootNext failed ({:?}) — continuing varix", other);
                }
            }
        }
    }

    // --- scheduler domain (F076~F100) --------------------------------------------------
    // Last in the boot chain: it needs CPU vectors, the clock tick and the
    // memory allocators, and it is what finally enables interrupts.
    // F12 逃生门布防：bootselect 三卡已过（菜单即引导界面，期间关闸），
    // 此后任意加载阶段按 F12 = 四级复位阶梯回本菜单。
    varix::ps2::enable_f12_escape();
    let sched_state = varix::sched::init();
    varix::sched::render_to_console(&sched_state);

    // --- process & user-space domain (F101~F125) ---------------------------------------
    // Needs the scheduler (for thread identity) and the memory allocators, and
    // it is what gives the rest of the boot chain a notion of "who".
    let proc_state = varix::proc::init();
    varix::proc::render_to_console(&proc_state);

    // --- storage domain (F126~F150) ----------------------------------------------------
    let storage_state = varix::storage::init();
    varix::storage::render_to_console(&storage_state);

    // --- block stack probe（任务16：块设备抽象 + NVMe 最小栈）------------------------
    // 引导设施红线：init 只读 identify；六连直写探针须 cmdline 显式
    // storage_selftest=1（QEMU 刮擦盘验收用），真机默认绝不写内置盘。
    varix::drivers::nvme::target::probe_and_selftest();

    // --- usb stack probe（S4.1·AI-5：xHCI 最小栈——真机 USB 键鼠，PS/2 增量不替代）----
    varix::drivers::xhci::target::probe_and_selftest();

    // --- sata stack probe（S4 批·AI-5：AHCI 最小栈——默认只读取证；
    //     写回环仅在 cmdline 显式 ahci_selftest=1 时执行，绝不自动写真盘）----
    varix::drivers::ahci::target::probe_and_selftest();

    // --- last_boot 写回（S4.2·AI-5 方案2 根解）-----------------------------------
    // 走到这里的唯一路径 = 用户三卡选中 varix（或倒计时默认）——「进入过
    // VARIX」事实成立，写 SHARED 真相源（MSC 优先/QEMU NVMe 兜底；
    // 尽力而为，失败只记账绝不阻塞引导；windows 侧由 Variable 启动写回）。
    if chosen_entry == "varix" {
        let ok = varix::fs::exfat_rw::record_last_boot("variable");
        varix::kinfo!("boot-select: last_boot=variable recorded ok={}", ok);
    }

    // --- wallpaper 装载（S4·AI-4/6：Limine 模块可选，缺席回退色带）--------------
    varix::wallpaper::init_from_module();

    // --- 实机快进模式（2026-09-22 实机卡死收口）----------------------------------
    // 证据：两代内核（09:14 旧盘 / 18:38 新盘）都在 win_probe 探针带内部
    // 不同位置冻结（QEMU 十余次冒烟从不复现）。重型探针 = 开发诊断，不是
    // 引导功能件——真机（无 CPUID hypervisor 位）默认快进跳过，把 ushell
    // 从探针带的生死里解耦；QEMU（hypervisor 位=1）照常全量，门禁不动。
    // cmdline probes_full=1 可在真机强制全量（排障用）。
    let probes_full = platform.features.hypervisor || varix::cmdline::flag("probes_full");
    varix::kinfo!(
        "boot-trace: probe mode = {} (hypervisor={})",
        if probes_full { "full" } else { "quick" },
        platform.features.hypervisor
    );

    // --- input service probe（任务19：PS/2 键鼠事件服务化）---------------------------
    varix::inputsvc::target::input_probe();

    // --- display service probe（任务20：双缓冲+脏矩形滚动条带撕裂验证）--------------
    if probes_full {
        varix::displaysrv::target::display_probe();
    }

    // --- window surface probe（AI-4 · S2.06/S2.09：窗口面注册+行提交+块链+
    //     Z 序合成+焦点命中全链，serial 断言 win-probe: PASS）--------------------
    if probes_full {
        varix::winsurf::win_probe();
        varix::kinfo!("boot-trace: win-probe done");
    }

    // --- quota service probe（任务56：三方配额矩阵+水位回收+GPU 通道）--------------
    if probes_full {
        varix::kinfo!("boot-trace: quota-probe begin");
        varix::quota::target::quota_probe();
        varix::kinfo!("boot-trace: quota-probe done");
    }

    // --- shm service probe（任务32：授权模型全矩阵）-----------------------------------
    if probes_full {
        varix::kinfo!("boot-trace: shm-probe begin");
        varix::shmsrv::target::shm_probe();
        varix::kinfo!("boot-trace: shm-probe done");
    }

    // --- vfs guard probe（任务30：白名单裁决矩阵，纯计算）---------------------------
    varix::kinfo!("boot-trace: vfs-probe begin");
    varix::vfsguard::target::vfs_decisions_probe();
    varix::kinfo!("boot-trace: vfs-probe done");

    // --- picflow probe（任务48：画面流通道 VM帧源→显示栈 全链直写）-----------------
    if probes_full {
        varix::kinfo!("boot-trace: picflow-probe begin");
        varix::stream::picflow::target::picflow_probe();
        varix::kinfo!("boot-trace: picflow-probe done");
    }



    // --- input domain (F151~F175) ------------------------------------------------------
    varix::kinfo!("boot-trace: input-domain begin");
    let input_state = varix::input::init();
    varix::input::render_to_console(&input_state);
    varix::kinfo!("boot-trace: input-domain done");

    // --- display domain (F176~F200) ----------------------------------------------------
    varix::kinfo!("boot-trace: display-domain begin");
    let display_state = varix::display::init();
    varix::display::render_to_console(&display_state);
    varix::kinfo!("boot-trace: display-domain done");

    // --- network domain (F201~F225) ----------------------------------------------------
    varix::kinfo!("boot-trace: net-domain begin");
    let net_state = varix::net::init();
    varix::net::render_to_console(&net_state);
    varix::kinfo!("boot-trace: net-domain done");

    // --- security domain (F226~F250) ---------------------------------------------------
    varix::kinfo!("boot-trace: sec-domain begin");
    let sec_state = varix::security::init();
    varix::security::render_to_console(&sec_state);
    varix::kinfo!("boot-trace: sec-domain done");

    // --- integrity (F021) ------------------------------------------------------------
    varix::kinfo!("boot-trace: integrity begin");
    TIMELINE.stage_begin(Stage::Integrity, varix::timeline::read_tsc());
    let chain = varix::integrity::init();
    TIMELINE.stage_end(Stage::Integrity, varix::timeline::read_tsc());
    varix::kinfo!("boot-trace: integrity done");
    let prefix = core::str::from_utf8(&chain.digest_prefix).unwrap_or("--------");
    varix::kinfo!(
        "integrity: {} image={}B sha256={}",
        chain.verdict.as_str(),
        chain.image_size,
        prefix
    );

    // --- bootopt (F022) ---------------------------------------------------------------
    // 已在 cmdline 阶段就近装载（菜单必须在它之前拿到真值）；此处再调一次是
    // 幂等确认，顺带保持 timeline 的 BootOpt 阶段语义。
    varix::kinfo!("boot-trace: bootopt begin");
    TIMELINE.stage_begin(Stage::BootOpt, varix::timeline::read_tsc());
    let opts = varix::bootopt::init();
    TIMELINE.stage_end(Stage::BootOpt, varix::timeline::read_tsc());
    varix::kinfo!(
        "bootopt: default={} timeout={}s",
        opts.default_entry,
        opts.timeout_secs
    );

    // --- selftest (F025) — runs its own stage bracket -------------------------------
    varix::kinfo!("boot-trace: selftest begin");
    TIMELINE.stage_begin(Stage::SelfTest, varix::timeline::read_tsc());
    let (_pass, _fail) = varix::selftest::run_boot_checks();
    TIMELINE.stage_end(Stage::SelfTest, varix::timeline::read_tsc());

    // Every domain is armed; only now do interrupts become useful. This is the
    // single point in the boot chain where the machine goes live.
    varix::cpu::enable_interrupts();
    varix::kinfo!("boot-trace: interrupts enabled — system live");

    // --- HUD: progress bar + timeline + self-test (F014/F024/F025) -------------------
    draw_hud(&surface, hud_y);
    varix::selftest::render_to_console();
    varix::timeline::render_to_console();

    // --- AURORA 桌面演示（`desktop=1` cmdline 触发）：界面栈首画帧缓冲 --------------
    if varix::cmdline::init().source().contains("desktop=1") {
        varix::aurora::demo::paint(&surface);
        varix::kinfo!("aurora: desktop demo painted");
    }

    // 任务71 性能门禁口径：boot_ms 由内核时钟源实测（boot-replay/性能基线
    // 消费该行；"boot complete" 前缀保持兼容既有里程碑 grep）。
    varix::kinfo!("boot completed {} ms", varix::proc::usrshell::boot_ms());

    // --- boot-summary（2026-09-22 诊断增强）：引导链异常征兆一站式汇总 ----
    // 真机照片判读行：任何一项非 ok 即是排查入口（usb=0 → xHCI init 失败
    // /键鼠无源；shared=0 → 双 NVMe 缺失或挂载失败；wp=0 → 壁纸模块缺席
    // 回退色带；storage_probes=off 是默认安全态=存储探针门禁）。
    {
        let disp_mode = varix::displaysrv::target::service()
            .map(|s| s.mode_name())
            .unwrap_or("none");
        varix::kinfo!(
            "boot-summary: display={} usb_hid={} shared={} wallpaper={} f12_armed={} storage_probes={} pmm_free={}MiB",
            disp_mode,
            varix::drivers::xhci::target::hid_channel_live(),
            varix::proc::usrshell::mount::available(),
            varix::wallpaper::ready(),
            varix::ps2::f12_armed(),
            if varix::cmdline::flag("storage_selftest") { "on" } else { "off" },
            varix::mem::pmm::free_bytes() >> 20
        );
    }

    // --- 需求 2 · A 卡交接（内核 → Variable）----------------------------------
    // 内核里跑不了 Tauri（要 Windows API + WebView2，结构上不可能），而
    // ExitBootServices 之后也无法跳转到 Windows Boot Manager——"进入 Variable"
    // 只能是：内核加载完把 BootNext 指向 Windows，复位后由那边的 Variable
    // 自启全屏。开关与逐级降级见 varix::handoff（默认开，可在 boot-select.json
    // 的 "handoff" 或 cmdline handoff=0 关掉以保留 ushell）。
    // 交接成功即永不返回；失败/关闭则如实继续走下面的 ushell。
    varix::kinfo!("boot-trace: handoff plan begin");
    if boot_opts.handoff_to_variable {
        match varix::handoff::plan(Some(chosen_entry), boot_opts.handoff_to_variable) {
            varix::handoff::HandoffPlan::ToWindows => {
                varix::kinfo!("handoff: plan=windows (entry={})", chosen_entry);
                varix::kinfo!("boot-trace: handoff run begin (prompt+bootnext)");
                if !varix::handoff::run(
                    &surface,
                    boot_opts.handoff_target,
                    boot_opts.usb_windows_esp_guid,
                ) {
                    varix::kwarn!("handoff: unavailable — continuing into the kernel ushell");
                }
            }
            varix::handoff::HandoffPlan::ToKernelShell => {
                varix::kinfo!("handoff: plan=kernel-shell (entry={})", chosen_entry);
            }
        }
    } else {
        varix::kinfo!("handoff: disabled by config — entering the kernel ushell");
    }

    // --- 任务14 · ring3 演示/ushell 拉起：装 MSR/TSS 后分两路 -------------
    // full（QEMU/probes_full）= 走完整受监护演示带（hello→PE→job→压力探针→
    // spawn_shell）——这是 QEMU-only 的开发自检件。
    // quick（真机）= **直接拉起 ushell**：演示带在真机曾发生 #PF（RIP/CR3
    // 呈 0x77 毒填充腐坏，2026-09-23 实机实证、QEMU 从不复现）——诊断带与
    // 生产引导解耦（与 win_probe 快进同判据同理由）。
    if varix::proc::ring3::install() {
        varix::kinfo!("ring3: syscall MSRs + TSS.RSP0 installed");
        if probes_full {
            varix::proc::ring3::run_demo();
        } else {
            varix::kinfo!("ring3: quick mode — demo band skipped, straight into ushell");
            varix::proc::ring3::spawn_ushell_now();
        }
    } else {
        varix::kwarn!("ring3: install unavailable — demo skipped");
    }
    halt()
}

/// HUD block under the banner: progress bar (F014) + boot message.
fn draw_hud(surf: &varix::fb::Surface, y: i64) {
    let w = surf.width() as i64;
    let inset = w / 12;
    let bar_w = w - inset * 2 - 56; // leave room for the % text
    let completed = TIMELINE.finished_stages();
    varix::progress::draw(surf, inset, y + 16, bar_w, completed, TOTAL_STAGES);
    // Boot message line under the bar.
    let msg = "BOOT SEQUENCE COMPLETE";
    let mw = varix::font::text_width_scaled(msg, 1);
    varix::font::draw_text_scaled(
        surf,
        (w - mw) / 2,
        y + 44,
        msg,
        varix::banner::TAGLINE,
        1,
    );
}

/// Serial-only tail when no framebuffer exists (degraded but honest boot).
fn finish_no_fb() -> ! {
    varix::selftest::run_boot_checks();
    varix::kerror!("boot degraded — no display");
    halt()
}

/// Park the CPU forever with interrupts disabled (`hlt` loop).
#[cfg(target_os = "none")]
fn halt() -> ! {
    // `cli` once, then park in `hlt` forever. Expressed as raw instructions:
    // the legacy `core::arch::x86_64::_hlt` wrappers are not available for the
    // `x86_64-unknown-none` target.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        loop {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Host build (never executed — the bin is only linked for the kernel target).
#[cfg(not(target_os = "none"))]
fn halt() -> ! {
    loop {}
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Best-effort: emit to serial + ring, then park.
    if let Some(c) = varix::console::installed_ref() {
        c.set_colors(
            varix::console::Ink::White,
            varix::console::Ink::Red,
        );
        c.write_str("\nKERNEL PANIC\n");
        c.write_str(core::str::from_utf8(
            &format_panic(info),
        ).unwrap_or(""));
    }
    halt()
}

/// PanicInfo → bytes without fmt machinery on the stack-heavy path.
fn format_panic(info: &core::panic::PanicInfo<'_>) -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut n = 0usize;
    let push = |bytes: &[u8], out: &mut [u8; 256], n: &mut usize| {
        for &b in bytes {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    push(info.location().map(|l| l.file()).unwrap_or("?").as_bytes(), &mut out, &mut n);
    push(b":", &mut out, &mut n);
    // Location line as decimal.
    if let Some(l) = info.location() {
        let mut num = [0u8; 10];
        let mut w = 0usize;
        let mut v = l.line();
        if v == 0 {
            num[0] = b'0';
            w = 1;
        } else {
            while v > 0 && w < num.len() {
                num[w] = b'0' + (v % 10) as u8;
                v /= 10;
                w += 1;
            }
        }
        while w > 0 {
            w -= 1;
            push(&[num[w]], &mut out, &mut n);
        }
    }
    push(b"\n", &mut out, &mut n);
    out
}
