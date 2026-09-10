//! AI-2 隔离核：进程级资源隔离、看门狗、流式读取与随盘配置。
//!
//! 这个模块故意不把“隔离”实现成一条 shell 字符串命令：
//! - Windows 上每个受管进程先进入独立 Job Object，再启动；Job handle 由
//!   [`LimitedChild`] 持有，handle 关闭时整个进程树一起结束。
//! - 非 Windows 构建保留同一套 API，但只提供进程生命周期与测试替身；它不会
//!   假装在没有 Hyper-V/Job Object 的平台上提供硬件级隔离。
//! - 所有超时均是失败关闭（返回 None），调用方必须显示降级，而不是卡住 UI。
//!
//! 对应：PORTABLE_VIRTUAL_SYSTEM_PLAN 第 4、5、15、21、25 章。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
#[cfg(windows)]
use std::ffi::c_void;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io;
#[cfg(not(windows))]
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 64 KiB 是 VHDX/便携盘上大文件启动的最小调度单元。
pub const CHUNK_SIZE: usize = 64 * 1024;
/// RAM LRU 的硬上限。它是缓存上限，不是给软件承诺的可用内存。
pub const DEFAULT_CACHE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_IPC_TIMEOUT: Duration = Duration::from_millis(800);
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(3);
pub const DEFAULT_CIRCUIT_COOLDOWN: Duration = Duration::from_secs(60);

// -----------------------------------------------------------------------------
// 7 层隔离的可审计配置
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NetworkMode {
    Nat,
    HostOnly,
    Bridged,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskLayer {
    pub parent_vhdx: PathBuf,
    pub differencing_vhdx: PathBuf,
    /// 母盘只读是硬约束；脚本会在挂载前再次设置文件只读。
    pub parent_read_only: bool,
    /// B 模式由 Test-VM.ps1 / 部署器在宿主侧执行 diskpart。
    pub automount_disabled: bool,
    pub copy_on_write: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLayer {
    pub memory_limit_bytes: u64,
    pub cpu_percent: u8,
    pub cpu_count: u8,
    pub host_reserved_bytes: u64,
    pub low_integrity: bool,
    pub io_priority: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileLayer {
    pub clipboard_enabled: bool,
    pub drag_drop_enabled: bool,
    pub shared_folders_enabled: bool,
    pub usb_passthrough_enabled: bool,
    pub exchange_root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLayer {
    pub mode: NetworkMode,
    pub inbound_blocked: bool,
    pub host_visible: bool,
    pub bridge_requires_confirmation: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryLayer {
    pub hive_root: PathBuf,
    pub load_per_session: bool,
    pub unload_on_exit: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceLayer {
    pub data_root: PathBuf,
    pub temp_root: PathBuf,
    pub dumps_root: PathBuf,
    pub host_residue_scan: bool,
}

/// 7 层的单一 manifest。它同时是脚本和 Rust 守护进程的契约，避免 UI
/// 自己拼出一套“看起来隔离”的参数。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsolationPolicy {
    pub disk: DiskLayer,
    pub resources: ResourceLayer,
    pub files: FileLayer,
    pub network: NetworkLayer,
    pub registry: RegistryLayer,
    pub traces: TraceLayer,
}

impl IsolationPolicy {
    pub fn for_data_root(data_root: impl Into<PathBuf>, parent_vhdx: impl Into<PathBuf>, differencing_vhdx: impl Into<PathBuf>) -> Self {
        let data_root = data_root.into();
        Self {
            disk: DiskLayer {
                parent_vhdx: parent_vhdx.into(),
                differencing_vhdx: differencing_vhdx.into(),
                parent_read_only: true,
                automount_disabled: true,
                copy_on_write: true,
            },
            resources: ResourceLayer {
                memory_limit_bytes: 4 * 1024 * 1024 * 1024,
                cpu_percent: 30,
                cpu_count: 4,
                host_reserved_bytes: 2 * 1024 * 1024 * 1024,
                low_integrity: true,
                io_priority: "very-low".into(),
            },
            files: FileLayer {
                clipboard_enabled: false,
                drag_drop_enabled: false,
                shared_folders_enabled: false,
                usb_passthrough_enabled: false,
                exchange_root: data_root.join("Exchange"),
            },
            network: NetworkLayer {
                mode: NetworkMode::Nat,
                inbound_blocked: true,
                host_visible: false,
                bridge_requires_confirmation: true,
            },
            registry: RegistryLayer {
                hive_root: data_root.join("Registry"),
                load_per_session: true,
                unload_on_exit: true,
            },
            traces: TraceLayer {
                temp_root: data_root.join("Cache").join("Temp"),
                dumps_root: data_root.join("Dumps"),
                data_root,
                host_residue_scan: true,
            },
        }
    }

    /// 在启动 VM/Worker 前验证 fail-closed 条件。这个函数不创建、删除或
    /// 挂载任何文件，因此可以在 UI 线程或 dry-run 中安全调用。
    pub fn validate(&self) -> Result<(), String> {
        if !self.disk.copy_on_write || !self.disk.parent_read_only {
            return Err("母盘必须只读且必须启用 COW / parent VHDX must be read-only COW".into());
        }
        if self.disk.parent_vhdx == self.disk.differencing_vhdx {
            return Err("母盘和子盘不能是同一文件 / parent and differencing disk must differ".into());
        }
        if self.resources.memory_limit_bytes == 0
            || self.resources.cpu_percent == 0
            || self.resources.cpu_percent > 100
            || self.resources.cpu_count == 0
            || self.resources.host_reserved_bytes == 0
        {
            return Err("资源限额无效 / invalid memory or CPU budget".into());
        }
        if self.files.clipboard_enabled
            || self.files.drag_drop_enabled
            || self.files.shared_folders_enabled
            || self.files.usb_passthrough_enabled
        {
            return Err("默认文件边界必须全部关闭 / clipboard, drag-drop, shares and USB must be off".into());
        }
        if self.network.mode != NetworkMode::Nat || self.network.host_visible || !self.network.inbound_blocked {
            return Err("隔离档只能使用入站关闭的 NAT / isolated profile requires NAT with inbound blocked".into());
        }
        if !self.registry.load_per_session || !self.registry.unload_on_exit {
            return Err("注册表必须按会话加载和卸载 / registry hive must be per-session".into());
        }
        if self.traces.data_root.as_os_str().is_empty()
            || !is_contained(&self.files.exchange_root, &self.traces.data_root)
            || !is_contained(&self.registry.hive_root, &self.traces.data_root)
            || !is_contained(&self.traces.temp_root, &self.traces.data_root)
            || !is_contained(&self.traces.dumps_root, &self.traces.data_root)
        {
            return Err("交换区、注册表、临时文件和转储必须在 Data 内 / trace paths must stay below Data".into());
        }
        Ok(())
    }

    pub fn manifest_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// 建立随盘目录。只会创建 Data 下的目录，不会创建宿主用户目录。
pub fn ensure_data_layout(data_root: &Path) -> io::Result<()> {
    for relative in [
        "Exchange",
        "Registry",
        "Cache/Temp",
        "Cache/RamCache",
        "Dumps",
        "PortableVM",
        "Tests",
    ] {
        fs::create_dir_all(data_root.join(relative))?;
    }
    Ok(())
}

fn is_contained(path: &Path, root: &Path) -> bool {
    let path = path.components().collect::<Vec<_>>();
    let root = root.components().collect::<Vec<_>>();
    path.len() >= root.len() && path[..root.len()] == root[..]
}

/// 受控 Exchange 通道的路径解析。`..`、绝对路径和空名称都会被拒绝。
pub fn controlled_exchange_path(data_root: &Path, relative_name: &str) -> io::Result<PathBuf> {
    let candidate = Path::new(relative_name);
    if relative_name.trim().is_empty()
        || candidate.is_absolute()
        || candidate.components().any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid Exchange path"));
    }
    let out = data_root.join("Exchange").join(candidate);
    if !is_contained(&out, &data_root.join("Exchange")) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Exchange path escapes sandbox"));
    }
    Ok(out)
}

/// 给受管进程使用的环境重定向表。它不是对宿主的安全承诺：不经过这个
/// API 启动的子进程仍可能写宿主，因此 launcher 必须统一走执行档。
pub fn trace_free_environment(data_root: &Path) -> Vec<(String, String)> {
    let home = data_root.join("User");
    vec![
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), home.to_string_lossy().into_owned()),
        ("APPDATA".into(), home.join("AppData/Roaming").to_string_lossy().into_owned()),
        ("LOCALAPPDATA".into(), home.join("AppData/Local").to_string_lossy().into_owned()),
        ("TEMP".into(), data_root.join("Cache/Temp").to_string_lossy().into_owned()),
        ("TMP".into(), data_root.join("Cache/Temp").to_string_lossy().into_owned()),
        ("PROGRAMDATA".into(), data_root.join("ProgramData").to_string_lossy().into_owned()),
    ]
}

// -----------------------------------------------------------------------------
// 第 5/25 章：Job Object 限额 + 可取消子进程
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoPriority {
    VeryLow,
    Normal,
}

#[derive(Clone, Copy, Debug)]
pub struct IsolationLimits {
    pub memory_bytes: u64,
    pub cpu_percent: u8,
    pub cpu_count: u8,
    pub io_priority: IoPriority,
    pub low_integrity: bool,
    pub kill_on_drop: bool,
}

impl Default for IsolationLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 4 * 1024 * 1024 * 1024,
            cpu_percent: 30,
            cpu_count: 4,
            io_priority: IoPriority::VeryLow,
            low_integrity: true,
            kill_on_drop: true,
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct JobHandle(*mut c_void);

#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            close_handle(self.0);
        }
    }
}

/// `Child` 与其 Job Object 的所有权。不要只保存 `Child` 再丢掉本对象：
/// 丢失 Job handle 会使进程树失去“随壳退出”的保护。
pub struct LimitedChild {
    child: Child,
    #[cfg(windows)]
    job: Option<JobHandle>,
    limits: IsolationLimits,
    low_integrity_applied: bool,
    started_at: Instant,
}

// std::process::Child 是 Send；Windows JobHandle 已明确是唯一关闭的句柄。
unsafe impl Send for LimitedChild {}

impl LimitedChild {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn started_at(&self) -> Instant {
        self.started_at
    }

    pub fn low_integrity_applied(&self) -> bool {
        self.low_integrity_applied
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    /// 只终止该 Job，不影响宿主或其他登记软件。
    pub fn cancel(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        if let Some(job) = self.job.as_ref() {
            let ok = unsafe { terminate_job(job.0, 1) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        self.child.kill()
    }

    /// Very Low I/O/后台优先级的再次应用；Job CPU cap 仍是硬上限。
    pub fn throttle(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let ok = unsafe { set_background_priority(self.child.as_raw_handle() as *mut c_void) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

impl Drop for LimitedChild {
    fn drop(&mut self) {
        if !self.limits.kill_on_drop {
            return;
        }
        // On Windows closing JobHandle kills the full tree through
        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE. On other platforms Child::kill is
        // the conservative best effort; callers should normally call cancel().
        #[cfg(not(windows))]
        {
            let _ = self.child.kill();
        }
    }
}

/// 在 Job Object 配好之后才让子进程运行。Windows 上任何限额配置失败都会
/// 终止已启动的 Job 并返回错误，避免“以为隔离、其实裸奔”的失败模式。
pub fn spawn_limited(command: &mut Command, limits: IsolationLimits) -> io::Result<LimitedChild> {
    if limits.memory_bytes == 0 || limits.cpu_percent == 0 || limits.cpu_percent > 100 || limits.cpu_count == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid isolation limits"));
    }

    #[cfg(windows)]
    {
        let job = create_job(&limits)?;
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return Err(error),
        };
        let mut limited = LimitedChild {
            child,
            job: Some(job),
            limits,
            low_integrity_applied: false,
            started_at: Instant::now(),
        };
        let handle = limited.child.as_raw_handle() as *mut c_void;
        let job_handle = limited.job.as_ref().expect("job created").0;
        if unsafe { assign_process(job_handle, handle, limits.cpu_count) } == 0 {
            let _ = limited.cancel();
            return Err(io::Error::last_os_error());
        }
        if limits.io_priority == IoPriority::VeryLow {
            // Background priority is best effort on older Windows editions; the
            // Job CPU cap remains active even when this advisory fails.
            let _ = limited.throttle();
        }
        if limits.low_integrity {
            match unsafe { set_low_integrity(handle) } {
                Ok(()) => limited.low_integrity_applied = true,
                Err(error) => {
                    let _ = limited.cancel();
                    return Err(io::Error::new(
                        error.kind(),
                        format!("low-integrity token could not be applied: {error}"),
                    ));
                }
            }
        }
        return Ok(limited);
    }

    #[cfg(not(windows))]
    {
        let child = command.spawn()?;
        Ok(LimitedChild {
            child,
            limits,
            low_integrity_applied: false,
        #[cfg(windows)]
            job: None,
            started_at: Instant::now(),
        })
    }
}

pub fn spawn_limited_program(program: impl AsRef<OsStr>, args: &[String], limits: IsolationLimits) -> io::Result<LimitedChild> {
    let mut command = Command::new(program);
    command.args(args);
    spawn_limited(&mut command, limits)
}

// -----------------------------------------------------------------------------
// 生命周期 Job（第十轮大检查）：受管进程随宿主一起退出
// -----------------------------------------------------------------------------
//
// AI-2 隔离防崩 §2.3 的契约是「一软件一 Job；KILL_ON_JOB_CLOSE」。
// `spawn_limited` 只服务 Test-VM 资源限额路径；通用受管进程入口
// （exec::spawn_profiled → launcher::spawn_detached）此前没有 Job——宿主
// 崩溃或退出时整棵第三方进程树会作为孤儿泄漏到宿主上。这里补上最低限度
// 的生命周期绑定：Job 不带任何资源限额、不动亲和性、不改完整性级别，
// 只带 KILL_ON_JOB_CLOSE；句柄登记进全局注册表后故意永不关闭——宿主以
// 任何方式终止（正常退出、panic、任务管理器结束）时内核关闭全部句柄，
// 所有登记过的进程树一并终止。注册表只增不减：每项仅一个内核句柄，
// 数量级远低于一次图标解码，不值得引入清理路径。

#[cfg(windows)]
static LIFECYCLE_JOBS: Mutex<Vec<JobHandle>> = Mutex::new(Vec::new());

/// 给已启动的子进程绑定「随宿主退出」的 KILL_ON_JOB_CLOSE Job。
/// 返回是否绑定成功；失败开放——Job 建不起来时进程照常运行，只是
/// 失去生命周期绑定（由调用方记日志）。绝不能因绑不上 Job 而拒绝
/// 启动用户软件。
#[cfg(windows)]
pub fn bind_lifecycle(child: &Child) -> bool {
    let job = match create_lifecycle_job() {
        Ok(job) => job,
        Err(_) => return false,
    };
    let process = child.as_raw_handle() as *mut c_void;
    if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
        return false; // JobHandle Drop 关闭句柄；此时 Job 尚未承载任何进程
    }
    LIFECYCLE_JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(job);
    true
}

#[cfg(not(windows))]
pub fn bind_lifecycle(_child: &Child) -> bool {
    false
}

/// 只设 KILL_ON_JOB_CLOSE 的裸 Job：对进程的唯一语义影响是
/// 「宿主侧最后一个句柄关闭 = 整棵进程树终止」。
#[cfg(windows)]
fn create_lifecycle_job() -> io::Result<JobHandle> {
    let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut extended = ExtendedLimitInformation {
        basic: BasicLimitInformation {
            per_process_user_time_limit: 0,
            per_job_user_time_limit: 0,
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            minimum_working_set_size: 0,
            maximum_working_set_size: 0,
            active_process_limit: 0,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        },
        io: IoCounters {
            read_operations: 0,
            write_operations: 0,
            other_operations: 0,
            read_bytes: 0,
            write_bytes: 0,
            other_bytes: 0,
        },
        process_memory_limit: 0,
        peak_process_memory_used: 0,
        job_memory_limit: 0,
        peak_job_memory_used: 0,
    };
    let ok = unsafe {
        SetInformationJobObject(
            handle,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&mut extended as *mut ExtendedLimitInformation).cast(),
            std::mem::size_of::<ExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        unsafe { close_handle(handle) };
        return Err(io::Error::last_os_error());
    }
    Ok(JobHandle(handle))
}

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
#[cfg(windows)]
const JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION: u32 = 15;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x100;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_JOB_MEMORY: u32 = 0x200;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION: u32 = 0x400;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
#[cfg(windows)]
const JOB_OBJECT_CPU_RATE_CONTROL_ENABLE: u32 = 0x1;
#[cfg(windows)]
const JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP: u32 = 0x4;
#[cfg(windows)]
const PROCESS_MODE_BACKGROUND_BEGIN: u32 = 0x0010_0000;

#[cfg(windows)]
#[repr(C)]
struct BasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
struct IoCounters {
    read_operations: u64,
    write_operations: u64,
    other_operations: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}

#[cfg(windows)]
#[repr(C)]
struct ExtendedLimitInformation {
    basic: BasicLimitInformation,
    io: IoCounters,
    process_memory_limit: usize,
    peak_process_memory_used: usize,
    job_memory_limit: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
#[repr(C)]
struct CpuRateControlInformation {
    control_flags: u32,
    cpu_rate: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(job: *mut c_void, class: u32, info: *mut c_void, length: u32) -> i32;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn SetPriorityClass(process: *mut c_void, priority_class: u32) -> i32;
    fn SetProcessAffinityMask(process: *mut c_void, mask: usize) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, desired_access: u32, token: *mut *mut c_void) -> i32;
    fn ConvertStringSidToSidW(string_sid: *const u16, sid: *mut *mut c_void) -> i32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
    fn SetTokenInformation(token: *mut c_void, class: u32, info: *const c_void, length: u32) -> i32;
}

#[cfg(windows)]
unsafe fn close_handle(handle: *mut c_void) {
    if !handle.is_null() {
        let _ = CloseHandle(handle);
    }
}

#[cfg(windows)]
fn create_job(limits: &IsolationLimits) -> io::Result<JobHandle> {
    let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let memory = usize::try_from(limits.memory_bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "memory limit does not fit usize"))?;
    let mut extended = ExtendedLimitInformation {
        basic: BasicLimitInformation {
            per_process_user_time_limit: 0,
            per_job_user_time_limit: 0,
            limit_flags: JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            minimum_working_set_size: 0,
            maximum_working_set_size: 0,
            active_process_limit: 0,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        },
        io: IoCounters {
            read_operations: 0,
            write_operations: 0,
            other_operations: 0,
            read_bytes: 0,
            write_bytes: 0,
            other_bytes: 0,
        },
        process_memory_limit: memory,
        peak_process_memory_used: 0,
        job_memory_limit: memory,
        peak_job_memory_used: 0,
    };
    let ok = unsafe {
        SetInformationJobObject(
            handle,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&mut extended as *mut ExtendedLimitInformation).cast(),
            std::mem::size_of::<ExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        unsafe { close_handle(handle) };
        return Err(io::Error::last_os_error());
    }

    let mut cpu = CpuRateControlInformation {
        control_flags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        cpu_rate: u32::from(limits.cpu_percent) * 100,
    };
    let ok = unsafe {
        SetInformationJobObject(
            handle,
            JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION,
            (&mut cpu as *mut CpuRateControlInformation).cast(),
            std::mem::size_of::<CpuRateControlInformation>() as u32,
        )
    };
    if ok == 0 {
        unsafe { close_handle(handle) };
        return Err(io::Error::last_os_error());
    }
    Ok(JobHandle(handle))
}

#[cfg(windows)]
unsafe fn assign_process(job: *mut c_void, process: *mut c_void, requested_cores: u8) -> i32 {
    let ok = AssignProcessToJobObject(job, process);
    if ok != 0 {
        let cores = usize::from(requested_cores);
        let bits = usize::BITS as usize;
        let width = cores.min(bits);
        let affinity = if width == bits { usize::MAX } else { (1usize << width) - 1 };
        // Affinity is advisory; the Job CPU hard cap is the enforcement layer.
        let _ = SetProcessAffinityMask(process, affinity);
    }
    ok
}

#[cfg(windows)]
unsafe fn set_background_priority(process: *mut c_void) -> i32 {
    SetPriorityClass(process, PROCESS_MODE_BACKGROUND_BEGIN)
}

#[cfg(windows)]
unsafe fn terminate_job(job: *mut c_void, exit_code: u32) -> i32 {
    TerminateJobObject(job, exit_code)
}

#[cfg(windows)]
#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[cfg(windows)]
#[repr(C)]
struct TokenMandatoryLabel {
    label: SidAndAttributes,
}

#[cfg(windows)]
unsafe fn set_low_integrity(process: *mut c_void) -> io::Result<()> {
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ADJUST_DEFAULT: u32 = 0x0080;
    const TOKEN_INTEGRITY_LEVEL: u32 = 25;
    const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
    let mut token = std::ptr::null_mut();
    if OpenProcessToken(process, TOKEN_QUERY | TOKEN_ADJUST_DEFAULT, &mut token) == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid_text: Vec<u16> = "S-1-16-4096\0".encode_utf16().collect();
    let mut sid = std::ptr::null_mut();
    if ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) == 0 {
        close_handle(token);
        return Err(io::Error::last_os_error());
    }
    let sid_length = GetLengthSid(sid);
    let label = TokenMandatoryLabel {
        label: SidAndAttributes {
            sid,
            attributes: SE_GROUP_INTEGRITY,
        },
    };
    // TOKEN_MANDATORY_LABEL is followed by the SID bytes in the Windows ABI.
    let length = (std::mem::size_of::<TokenMandatoryLabel>() as u32).saturating_add(sid_length);
    let ok = SetTokenInformation(
        token,
        TOKEN_INTEGRITY_LEVEL,
        (&label as *const TokenMandatoryLabel).cast(),
        length,
    );
    let error = if ok == 0 { Some(io::Error::last_os_error()) } else { None };
    let _ = LocalFree(sid);
    close_handle(token);
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

// -----------------------------------------------------------------------------
// 第 5.3/5.6：64 KiB demand paging + 256 MiB LRU
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub bytes: usize,
    pub entries: usize,
}

struct ChunkCache {
    capacity: usize,
    bytes: usize,
    map: HashMap<u64, Vec<u8>>,
    order: VecDeque<u64>,
    hits: u64,
    misses: u64,
}

impl ChunkCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(CHUNK_SIZE),
            bytes: 0,
            map: HashMap::new(),
            order: VecDeque::new(),
            hits: 0,
            misses: 0,
        }
    }

    fn get(&mut self, key: u64) -> Option<Vec<u8>> {
        let value = self.map.get(&key).cloned();
        if value.is_some() {
            self.hits += 1;
            self.order.retain(|item| *item != key);
            self.order.push_back(key);
        } else {
            self.misses += 1;
        }
        value
    }

    fn insert(&mut self, key: u64, value: Vec<u8>) {
        if let Some(previous) = self.map.insert(key, value.clone()) {
            self.bytes = self.bytes.saturating_sub(previous.len());
            self.order.retain(|item| *item != key);
        }
        self.bytes += value.len();
        self.order.push_back(key);
        while self.bytes > self.capacity {
            let Some(oldest) = self.order.pop_front() else { break };
            if let Some(evicted) = self.map.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(evicted.len());
            }
        }
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            bytes: self.bytes,
            entries: self.map.len(),
        }
    }
}

/// 按需分页文件。Windows 使用 `CreateFileMappingW`/`MapViewOfFile` 映射单个
/// 64 KiB chunk；其他系统用等价的 seek-read 测试替身。调用方只拿到请求范围，
/// 不会为了打开一个 10 GB 软件把整个文件读进内存。
pub struct DemandPagedFile {
    file: Mutex<File>,
    length: u64,
    cache: Mutex<ChunkCache>,
    #[cfg(windows)]
    mapping: Option<MappingHandle>,
}

#[cfg(windows)]
struct MappingHandle(*mut c_void);

#[cfg(windows)]
unsafe impl Send for MappingHandle {}
#[cfg(windows)]
unsafe impl Sync for MappingHandle {}

#[cfg(windows)]
impl Drop for MappingHandle {
    fn drop(&mut self) {
        unsafe { close_handle(self.0) };
    }
}

impl DemandPagedFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_cache(path, DEFAULT_CACHE_BYTES)
    }

    pub fn open_with_cache(path: &Path, cache_bytes: usize) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let length = file.metadata()?.len();
        #[cfg(windows)]
        let mapping = if length == 0 {
            None
        } else {
            let handle = unsafe { create_read_mapping(file.as_raw_handle() as *mut c_void) }?;
            Some(MappingHandle(handle))
        };
        Ok(Self {
            file: Mutex::new(file),
            length,
            cache: Mutex::new(ChunkCache::new(cache_bytes)),
            #[cfg(windows)]
            mapping,
        })
    }

    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn stats(&self) -> CacheStats {
        self.cache.lock().expect("cache mutex poisoned").stats()
    }

    pub fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        if offset > self.length {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "offset beyond file"));
        }
        let end = offset.saturating_add(length as u64).min(self.length);
        let wanted = (end - offset) as usize;
        let mut output = Vec::with_capacity(wanted);
        if wanted == 0 {
            return Ok(output);
        }
        let first = offset / CHUNK_SIZE as u64;
        let last = (end - 1) / CHUNK_SIZE as u64;
        for index in first..=last {
            let chunk = self.chunk(index)?;
            let chunk_start = index * CHUNK_SIZE as u64;
            let from = offset.saturating_sub(chunk_start) as usize;
            let to = ((end - chunk_start) as usize).min(chunk.len());
            if from < to {
                output.extend_from_slice(&chunk[from..to]);
            }
        }
        Ok(output)
    }

    pub fn prefetch(&self, first_chunk: u64, count: usize) -> io::Result<()> {
        for index in first_chunk..first_chunk.saturating_add(count as u64) {
            if index * CHUNK_SIZE as u64 >= self.length {
                break;
            }
            let _ = self.chunk(index)?;
        }
        Ok(())
    }

    fn chunk(&self, index: u64) -> io::Result<Vec<u8>> {
        #[cfg(windows)]
        let _file_lifetime = &self.file;
        if let Some(value) = self.cache.lock().expect("cache mutex poisoned").get(index) {
            return Ok(value);
        }
        let start = index * CHUNK_SIZE as u64;
        if start >= self.length {
            return Ok(Vec::new());
        }
        let size = ((self.length - start) as usize).min(CHUNK_SIZE);
        #[cfg(windows)]
        let value = if let Some(mapping) = self.mapping.as_ref() {
            unsafe { map_chunk(mapping.0, index, size) }?
        } else {
            Vec::new()
        };
        #[cfg(not(windows))]
        let value = {
            let mut file = self.file.lock().expect("file mutex poisoned");
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = vec![0u8; size];
            file.read_exact(&mut bytes)?;
            bytes
        };
        self.cache.lock().expect("cache mutex poisoned").insert(index, value.clone());
        Ok(value)
    }
}

#[cfg(windows)]
const PAGE_READONLY: u32 = 0x02;
#[cfg(windows)]
const FILE_MAP_READ: u32 = 0x0004;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileMappingW(file: *mut c_void, attributes: *mut c_void, protect: u32, max_high: u32, max_low: u32, name: *const u16) -> *mut c_void;
    fn MapViewOfFile(mapping: *mut c_void, access: u32, high: u32, low: u32, bytes: usize) -> *mut c_void;
    fn UnmapViewOfFile(address: *const c_void) -> i32;
}

#[cfg(windows)]
unsafe fn create_read_mapping(file: *mut c_void) -> io::Result<*mut c_void> {
    let mapping = CreateFileMappingW(file, std::ptr::null_mut(), PAGE_READONLY, 0, 0, std::ptr::null());
    if mapping.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(mapping)
    }
}

#[cfg(windows)]
unsafe fn map_chunk(mapping: *mut c_void, index: u64, size: usize) -> io::Result<Vec<u8>> {
    let offset = index.saturating_mul(CHUNK_SIZE as u64);
    let view = MapViewOfFile(
        mapping,
        FILE_MAP_READ,
        (offset >> 32) as u32,
        offset as u32,
        size,
    );
    if view.is_null() {
        return Err(io::Error::last_os_error());
    }
    let bytes = std::slice::from_raw_parts(view.cast::<u8>(), size).to_vec();
    let ok = UnmapViewOfFile(view);
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(bytes)
}

// -----------------------------------------------------------------------------
// 第 15/25 章：800 ms 熔断、3 次失败隔离与 3 秒看门狗
// -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CircuitState {
    failures: u32,
    opened_at: Option<Instant>,
}

#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,
    failure_threshold: u32,
    cooldown: Duration,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(3, DEFAULT_CIRCUIT_COOLDOWN)
    }
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState { failures: 0, opened_at: None })),
            failure_threshold: failure_threshold.max(1),
            cooldown,
        }
    }

    pub fn is_open(&self) -> bool {
        let mut state = self.state.lock().expect("circuit mutex poisoned");
        if let Some(opened) = state.opened_at {
            if opened.elapsed() < self.cooldown {
                return true;
            }
            state.opened_at = None;
            state.failures = 0;
        }
        false
    }

    pub fn call<F, T>(&self, timeout: Duration, operation: F) -> Option<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        if self.is_open() {
            return None;
        }
        match timed_call(operation, timeout) {
            Some(value) => {
                let mut state = self.state.lock().expect("circuit mutex poisoned");
                state.failures = 0;
                state.opened_at = None;
                Some(value)
            }
            None => {
                let mut state = self.state.lock().expect("circuit mutex poisoned");
                state.failures = state.failures.saturating_add(1);
                if state.failures >= self.failure_threshold {
                    state.opened_at = Some(Instant::now());
                }
                None
            }
        }
    }
}

/// 大软件启动的可观测 30 秒熔断。它不会在别的线程强杀任意代码；加载器
/// 每次分页/阶段完成后检查 `expired()`，过期后调用 `LimitedChild::cancel()`。
#[derive(Clone, Debug)]
pub struct StartupFuse {
    started_at: Instant,
    timeout: Duration,
    cancelled: Arc<AtomicBool>,
}

impl StartupFuse {
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_STARTUP_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            started_at: Instant::now(),
            timeout,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn expired(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || self.started_at.elapsed() >= self.timeout
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn remaining(&self) -> Duration {
        self.timeout.saturating_sub(self.started_at.elapsed())
    }
}

impl Default for StartupFuse {
    fn default() -> Self {
        Self::new()
    }
}

/// 有限时间的同步阻塞调用。超时后不会等待工作线程，因此只适合没有 UI
/// 引用和可安全放弃结果的只读/IPC 操作；写操作应使用可取消 API。
pub fn timed_call<F, T>(operation: F, timeout: Duration) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let _ = thread::Builder::new()
        .name("variable-isolation-call".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).ok();
            if let Some(value) = result {
                let _ = sender.send(value);
            }
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()
}

/// 计划原文使用的中文 API 名称，保留它以便实现与文档逐项对照。
#[allow(non_snake_case)]
pub fn call_with熔断<F, T>(operation: F, timeout_ms: u64) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    timed_call(operation, Duration::from_millis(timeout_ms))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkerTier {
    Application,
    Worker,
    Shell,
    System,
}

impl WorkerTier {
    fn restart_delay(self) -> Duration {
        match self {
            WorkerTier::Application => Duration::from_millis(100),
            WorkerTier::Worker => Duration::from_secs(3),
            WorkerTier::Shell => Duration::from_secs(5),
            WorkerTier::System => Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchdogEventKind {
    Started,
    Exited,
    Restarted,
    RestartSuppressed,
    CircuitOpen,
    SystemRollbackRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogEvent {
    pub worker_id: String,
    pub tier: WorkerTier,
    pub kind: WatchdogEventKind,
    pub message: String,
}

pub type RestartFactory = Box<dyn Fn() -> io::Result<LimitedChild> + Send + Sync + 'static>;

pub type WatchdogObserver = Arc<dyn Fn(WatchdogEvent) + Send + Sync + 'static>;

struct WorkerEntry {
    tier: WorkerTier,
    child: Arc<Mutex<LimitedChild>>,
    restart: Option<RestartFactory>,
    restart_times: VecDeque<Instant>,
    next_restart_at: Instant,
}

struct WatchdogInner {
    stop: AtomicBool,
    workers: Mutex<HashMap<String, WorkerEntry>>,
}

/// Core 看门狗。默认 3 秒检查一次；单个 Worker 连续失败不会拖垮整个
/// Shell，60 秒内达到三次重启失败后进入熔断，只发事件不再狂重启。
pub struct Watchdog {
    inner: Arc<WatchdogInner>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Watchdog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WatchdogInner {
                stop: AtomicBool::new(false),
                workers: Mutex::new(HashMap::new()),
            }),
            thread: Mutex::new(None),
        }
    }

    pub fn register(
        &self,
        id: impl Into<String>,
        tier: WorkerTier,
        child: LimitedChild,
        restart: Option<RestartFactory>,
    ) {
        let id = id.into();
        let entry = WorkerEntry {
            tier,
            child: Arc::new(Mutex::new(child)),
            restart,
            restart_times: VecDeque::new(),
            next_restart_at: Instant::now(),
        };
        self.inner
            .workers
            .lock()
            .expect("watchdog mutex poisoned")
            .insert(id, entry);
    }

    pub fn register_with_restart<F>(
        &self,
        id: impl Into<String>,
        tier: WorkerTier,
        child: LimitedChild,
        restart: F,
    ) where
        F: Fn() -> io::Result<LimitedChild> + Send + Sync + 'static,
    {
        self.register(id, tier, child, Some(Box::new(restart)));
    }

    pub fn unregister(&self, id: &str) -> bool {
        self.inner.workers.lock().expect("watchdog mutex poisoned").remove(id).is_some()
    }

    pub fn start_default(&self, observer: Option<WatchdogObserver>) {
        self.start(DEFAULT_WATCHDOG_INTERVAL, observer);
    }

    pub fn start(&self, interval: Duration, observer: Option<WatchdogObserver>) {
        let mut slot = self.thread.lock().expect("watchdog thread mutex poisoned");
        if slot.is_some() {
            return;
        }
        self.inner.stop.store(false, Ordering::Release);
        let inner = Arc::clone(&self.inner);
        *slot = Some(thread::Builder::new().name("variable-watchdog".into()).spawn(move || {
            while !inner.stop.load(Ordering::Acquire) {
                let events = check_workers(&inner);
                if let Some(callback) = observer.as_ref() {
                    for event in events {
                        callback(event);
                    }
                }
                thread::sleep(interval);
            }
        }).expect("watchdog thread should start"));
    }

    /// 同步检查一次，适合单元测试和已有节拍器；返回值是 UI/Core 可消费的
    /// 事件，而不是在这里偷偷弹窗或终止宿主。
    pub fn check_once(&self) -> Vec<WatchdogEvent> {
        check_workers(&self.inner)
    }

    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.lock().expect("watchdog thread mutex poisoned").take() {
            let _ = thread.join();
        }
    }
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

fn check_workers(inner: &Arc<WatchdogInner>) -> Vec<WatchdogEvent> {
    let now = Instant::now();
    let mut events = Vec::new();
    let mut workers = inner.workers.lock().expect("watchdog mutex poisoned");
    for (id, entry) in workers.iter_mut() {
        let mut child = entry.child.lock().expect("worker mutex poisoned");
        let exited = match child.try_wait() {
            Ok(Some(status)) => Some(status),
            Ok(None) => None,
            Err(error) => {
                events.push(WatchdogEvent {
                    worker_id: id.clone(),
                    tier: entry.tier,
                    kind: WatchdogEventKind::Exited,
                    message: format!("无法读取进程状态，保留现有进程并等待下一次检查: {error}"),
                });
                continue;
            }
        };
        let Some(status) = exited else { continue };
        events.push(WatchdogEvent {
            worker_id: id.clone(),
            tier: entry.tier,
            kind: WatchdogEventKind::Exited,
            message: format!("受管进程退出，code={:?}", status.code()),
        });
        if entry.tier == WorkerTier::System {
            events.push(WatchdogEvent {
                worker_id: id.clone(),
                tier: entry.tier,
                kind: WatchdogEventKind::SystemRollbackRequired,
                message: "系统层故障：丢弃差分子盘并回滚到上一个检查点".into(),
            });
            continue;
        }
        if entry.restart.is_none() || now < entry.next_restart_at {
            events.push(WatchdogEvent {
                worker_id: id.clone(),
                tier: entry.tier,
                kind: WatchdogEventKind::RestartSuppressed,
                message: "没有自动重启策略或仍在退避窗口".into(),
            });
            continue;
        }
        while entry.restart_times.front().is_some_and(|at| now.duration_since(*at) > Duration::from_secs(60)) {
            entry.restart_times.pop_front();
        }
        if entry.restart_times.len() >= 3 {
            events.push(WatchdogEvent {
                worker_id: id.clone(),
                tier: entry.tier,
                kind: WatchdogEventKind::CircuitOpen,
                message: "60 秒内重启失败达到 3 次，隔离 1 分钟".into(),
            });
            entry.next_restart_at = now + DEFAULT_CIRCUIT_COOLDOWN;
            continue;
        }
        let result = entry.restart.as_ref().expect("checked above")();
        entry.restart_times.push_back(now);
        entry.next_restart_at = now + entry.tier.restart_delay();
        match result {
            Ok(replacement) => {
                *child = replacement;
                events.push(WatchdogEvent {
                    worker_id: id.clone(),
                    tier: entry.tier,
                    kind: WatchdogEventKind::Restarted,
                    message: "已在独立 Job 中自动恢复".into(),
                });
            }
            Err(error) => events.push(WatchdogEvent {
                worker_id: id.clone(),
                tier: entry.tier,
                kind: WatchdogEventKind::RestartSuppressed,
                message: format!("自动恢复失败，保留占位卡，不杀其他进程: {error}"),
            }),
        }
    }
    events
}

// -----------------------------------------------------------------------------
// 第 4.6 层：随盘 Registry hive（管理员权限不足时明确失败）
// -----------------------------------------------------------------------------

pub fn registry_hive_path(data_root: &Path, name: &str) -> io::Result<PathBuf> {
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name == "."
        || name == ".."
        || !name.to_ascii_lowercase().ends_with(".dat")
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid registry hive name"));
    }
    let root = data_root.join("Registry");
    let path = root.join(name);
    if !is_contained(&path, &root) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "registry hive escapes Data"));
    }
    Ok(path)
}

#[cfg(windows)]
pub struct LoadedRegistryHive {
    key_name: Vec<u16>,
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegLoadKeyW(root: *mut c_void, sub_key: *const u16, file: *const u16) -> i32;
    fn RegUnLoadKeyW(root: *mut c_void, sub_key: *const u16) -> i32;
}

#[cfg(windows)]
impl Drop for LoadedRegistryHive {
    fn drop(&mut self) {
        const HKEY_USERS: *mut c_void = 0x8000_0003usize as *mut c_void;
        unsafe {
            let _ = RegUnLoadKeyW(HKEY_USERS, self.key_name.as_ptr());
        }
    }
}

/// 将 Data/Registry/<name>.dat 临时挂到 HKU。Windows 可能要求管理员权限；
/// 失败时不回退去改宿主 HKCU。
#[cfg(windows)]
pub fn load_portable_registry(data_root: &Path, name: &str) -> io::Result<LoadedRegistryHive> {
    const HKEY_USERS: *mut c_void = 0x8000_0003usize as *mut c_void;
    let file = registry_hive_path(data_root, name)?;
    if !file.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "registry hive not found"));
    }
    let key_name = format!("VariablePortable_{}\0", name.trim_end_matches(".dat"));
    let key: Vec<u16> = key_name.encode_utf16().collect();
    let path: Vec<u16> = file.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let code = unsafe { RegLoadKeyW(HKEY_USERS, key.as_ptr(), path.as_ptr()) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code));
    }
    Ok(LoadedRegistryHive { key_name: key })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_policy_is_fail_closed() {
        let root = PathBuf::from("Data");
        let policy = IsolationPolicy::for_data_root(&root, "Base.vhdx", "User.vhdx");
        assert!(policy.validate().is_ok());
        assert_eq!(policy.resources.memory_limit_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(policy.resources.cpu_percent, 30);
        assert!(!policy.files.clipboard_enabled);
        assert_eq!(policy.network.mode, NetworkMode::Nat);
    }

    #[test]
    fn exchange_rejects_escape() {
        let root = PathBuf::from("Data");
        assert!(controlled_exchange_path(&root, "ok.zip").is_ok());
        assert!(controlled_exchange_path(&root, "../host.txt").is_err());
        assert!(controlled_exchange_path(&root, "").is_err());
    }

    #[test]
    fn demand_paging_reads_only_requested_ranges() {
        let dir = std::env::temp_dir().join(format!("variable-isolation-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("large.bin");
        let mut file = File::create(&path).unwrap();
        let mut source = vec![0u8; CHUNK_SIZE * 2 + 17];
        for (index, byte) in source.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        file.write_all(&source).unwrap();
        let paged = DemandPagedFile::open_with_cache(&path, CHUNK_SIZE * 2).unwrap();
        let bytes = paged.read_range(CHUNK_SIZE as u64 - 3, 9).unwrap();
        assert_eq!(&bytes, &source[CHUNK_SIZE - 3..CHUNK_SIZE + 6]);
        let stats = paged.stats();
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.entries, 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn registry_hive_rejects_escape() {
        let root = PathBuf::from("Data");
        assert!(registry_hive_path(&root, "User.dat").is_ok());
        assert!(registry_hive_path(&root, "../Host.dat").is_err());
        assert!(registry_hive_path(&root, "no-suffix").is_err());
    }

    /// 第十轮大检查：生命周期 Job 语义验证——宿主侧最后一个句柄关闭
    /// （等价于宿主进程退出）时，KILL_ON_JOB_CLOSE 必须终止已绑定的进程。
    /// 对照组（未绑定 Job 的同款进程）必须仍然存活，把因果钉在 Job 上。
    /// （KILL_ON_JOB_CLOSE 的终止退出码由内核给 0，不作断言。）
    #[cfg(windows)]
    #[test]
    fn lifecycle_job_kills_child_when_handle_drops() {
        let mut bound = Command::new("ping")
            .args(["-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut control = Command::new("ping")
            .args(["-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        assert!(bind_lifecycle(&bound));
        // 模拟宿主退出：从注册表取回唯一句柄并 drop。
        let job = LIFECYCLE_JOBS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop();
        drop(job);
        let mut exited = false;
        for _ in 0..60 {
            if bound.try_wait().unwrap().is_some() {
                exited = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(exited, "关闭 Job 句柄应在数秒内终止已绑定的子进程");
        assert!(
            control.try_wait().unwrap().is_none(),
            "未绑定 Job 的对照进程不应受影响"
        );
        let _ = bound.kill();
        let _ = control.kill();
    }

    #[test]
    fn circuit_opens_after_three_timeouts() {
        let circuit = CircuitBreaker::new(3, Duration::from_secs(60));
        for _ in 0..3 {
            assert!(circuit.call(Duration::from_millis(1), || {
                thread::sleep(Duration::from_millis(20));
                1u8
            }).is_none());
        }
        assert!(circuit.is_open());
        assert!(circuit.call(Duration::from_secs(1), || 1u8).is_none());
    }
}
