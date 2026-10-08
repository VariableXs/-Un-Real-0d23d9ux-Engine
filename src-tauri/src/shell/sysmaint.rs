//! L3 shell — sysmaint.rs（F-6 系统维护与自更新）
//! - 计划备份：none/daily/weekly + 小时点；内部定时器（直跑档跨会话补偿：
//!   启动时发现错过的任务 → 立即补跑一次并标记 lastSource="catchup"）。
//!   边界：VM 档的 VM 内计划任务由 D 路登记；此处统一为 Variable 内部定时器。
//! - 引擎自更新：本地更新包（<dataDir>/updates/）manifest SHA-256 校验 →
//!   备份当前 runtime/engine（保留 N=2 份历史）→ 应用 → 失败自动回滚。
//!   边界：增量包下载（白名单域 + 传输签名）走下载器；本模块只做校验/应用/回滚。
//! - 数据自检扩展：备份可恢复性抽检（PRAGMA quick_check）、容器索引一致性抽检
//!   （快照抽样存在性）、媒体孤儿引用（media 目录文件未登记 DB）。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn civil_stamp(ms: i64) -> String {
    let secs = (ms / 1000).max(0) as i64;
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{ss:02}", h = tod / 3600, mi = (tod % 3600) / 60, ss = tod % 60)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    // 协同修复（AI-20）：标准 Hinnant 算法 doy = doe - (365*yoe + yoe/4 - yoe/100)，
    // 原实现多出的 "+ doe/1460" 使 1970-01-01 被算成 1969-10-01（civil_stamp_formats 既有测试红）
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------- 计划备份

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSchedule {
    /// none | daily | weekly
    pub freq: String,
    /// 每日触发的小时（0-23）。
    pub hour: u8,
    pub last_run_ms: i64,
    /// manual | scheduled | catchup
    pub last_source: String,
    /// 启动补偿：错过计划任务时置 true（前端补问提示）。
    pub missed: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct ScheduleCfg {
    freq: String,
    hour: u8,
    last_run_ms: i64,
    last_source: String,
}

fn schedule_path(st: &AppState) -> PathBuf {
    st.data_dir.join("config").join("backup-schedule.json")
}

fn load_schedule(st: &AppState) -> ScheduleCfg {
    let raw = fs::read_to_string(schedule_path(st)).unwrap_or_default();
    serde_json::from_str::<ScheduleCfg>(&raw).unwrap_or(ScheduleCfg {
        freq: "none".into(),
        hour: 3,
        last_run_ms: 0,
        last_source: String::new(),
    })
}

fn save_schedule(st: &AppState, cfg: &ScheduleCfg) -> CmdResult<()> {
    let p = schedule_path(st);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| AppError::io(e.to_string()))?;
    fs::write(&p, json).map_err(|e| AppError::io(e.to_string()))
}

fn interval_ms(freq: &str) -> i64 {
    match freq {
        "daily" => 86_400_000,
        "weekly" => 7 * 86_400_000,
        _ => 0,
    }
}

#[tauri::command(async)]
pub fn backup_schedule_get(st: tauri::State<AppState>) -> CmdResult<BackupSchedule> {
    let cfg = load_schedule(st.inner());
    let iv = interval_ms(&cfg.freq);
    Ok(BackupSchedule {
        freq: cfg.freq.clone(),
        hour: cfg.hour,
        last_run_ms: cfg.last_run_ms,
        last_source: cfg.last_source.clone(),
        missed: iv > 0 && cfg.last_run_ms > 0 && now_ms() - cfg.last_run_ms > iv + 3_600_000,
    })
}

#[tauri::command(async)]
pub fn backup_schedule_set(st: tauri::State<AppState>, freq: String, hour: u8) -> CmdResult<BackupSchedule> {
    if !["none", "daily", "weekly"].contains(&freq.as_str()) {
        return Err(AppError::validation("freq 仅支持 none/daily/weekly"));
    }
    if hour > 23 {
        return Err(AppError::validation("hour 需在 0-23"));
    }
    let mut cfg = load_schedule(st.inner());
    cfg.freq = freq;
    cfg.hour = hour;
    save_schedule(st.inner(), &cfg)?;
    backup_schedule_get(st)
}

/// 备份核心：WAL checkpoint → 复制 db → 登记 backups 表（复用现有 schema）。
fn run_backup(st: &AppState, source: &str) -> CmdResult<(String, i64)> {
    st.with_conn(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(AppError::from)?;
        Ok(())
    })?;
    let src = st.db_dir.join("variable.db");
    if !src.exists() {
        return Err(AppError::not_found("数据库文件不存在"));
    }
    let name = format!("variable-backup-{}-{}.db", civil_stamp(now_ms()), source);
    let dest = st.backups_dir.join(&name);
    fs::copy(&src, &dest).map_err(|e| AppError::io(format!("备份失败: {e}")))?;
    let meta = fs::metadata(&dest).map_err(AppError::from)?;
    let (checksum, _) = crate::media::checksum_file_public(&dest);
    let id = crate::db::gen_id();
    let ts = now_ms();
    st.with_conn(|conn| {
        conn.execute(
            "INSERT INTO backups(id,file_name,source,size,checksum,status,created_at) VALUES(?1,?2,?3,?4,?5,'ok',?6)",
            rusqlite::params![id, name, source, meta.len() as i64, checksum, ts],
        )
        .map_err(AppError::from)?;
        Ok(())
    })?;
    Ok((name, ts))
}

/// 手动「立即备份」（设置 → 数据 → 计划备份）。
#[tauri::command(async)]
pub fn backup_run_now(st: tauri::State<AppState>) -> CmdResult<String> {
    let (name, ts) = run_backup(st.inner(), "manual")?;
    let mut cfg = load_schedule(st.inner());
    cfg.last_run_ms = ts;
    cfg.last_source = "manual".into();
    let _ = save_schedule(st.inner(), &cfg);
    Ok(name)
}

/// 定时器判定：到点即跑（小时匹配 or 周期兜底），补跑标记 catchup。
fn tick(st: &AppState) -> Option<&'static str> {
    let cfg = load_schedule(st);
    let iv = interval_ms(&cfg.freq);
    if iv == 0 {
        return None;
    }
    let now = now_ms();
    let due = if cfg.last_run_ms <= 0 {
        true
    } else {
        now - cfg.last_run_ms >= iv
    };
    if !due {
        return None;
    }
    let source: &'static str = if cfg.last_run_ms > 0 && now - cfg.last_run_ms > iv + 3_600_000 {
        "catchup"
    } else {
        "scheduled"
    };
    match run_backup(st, source) {
        Ok((_, ts)) => {
            let mut cfg2 = cfg;
            cfg2.last_run_ms = ts;
            cfg2.last_source = source.to_string();
            let _ = save_schedule(st, &cfg2);
            Some(source)
        }
        Err(_) => None,
    }
}

static SCHED_STARTED: AtomicBool = AtomicBool::new(false);

/// 启动定时器（lib.rs setup 调用一次；15s 首查实现启动补偿，此后 10min 轮询）。
pub fn start_scheduler(app: tauri::AppHandle) {
    if SCHED_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            let st = app.state::<AppState>();
            let _ = tick(&st);
            tick_dep_audit(&st); // M-85 周任务（内部自判 7 天周期）
            drop(st);
            std::thread::sleep(Duration::from_secs(600));
        }
    });
}

// ---------------------------------------------------------------- M-85 依赖审计周任务

/// 周期 7 天；安装档无 node/开发工具链时如实记 unavailable（不重试轰炸，按周期推进）。
const DEP_AUDIT_INTERVAL_MS: i64 = 7 * 86_400_000;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DepAuditState {
    pub last_run_ms: i64,
    pub last_ok: bool,
    pub summary: String,
    pub due: bool,
}

fn dep_audit_cfg_path(st: &AppState) -> PathBuf {
    st.data_dir.join("config").join("dep-audit.json")
}

fn load_dep_audit(st: &AppState) -> DepAuditState {
    let raw = fs::read_to_string(dep_audit_cfg_path(st)).unwrap_or_default();
    serde_json::from_str::<DepAuditState>(&raw).unwrap_or_default()
}

fn save_dep_audit(st: &AppState, s: &DepAuditState) -> CmdResult<()> {
    let p = dep_audit_cfg_path(st);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| AppError::io(e.to_string()))?;
    fs::write(&p, json).map_err(|e| AppError::io(e.to_string()))
}

fn dep_audit_due(last_run_ms: i64, now: i64) -> bool {
    last_run_ms <= 0 || now - last_run_ms >= DEP_AUDIT_INTERVAL_MS
}

#[tauri::command(async)]
pub fn dep_audit_status(st: tauri::State<AppState>) -> CmdResult<DepAuditState> {
    let mut s = load_dep_audit(st.inner());
    s.due = dep_audit_due(s.last_run_ms, now_ms());
    Ok(s)
}

/// 定位开发仓的 tools/dep-audit.cjs（沿 exe 祖先目录寻找；安装档不存在 → None）。
fn find_dep_audit_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).find_map(|anc| {
        let cand = anc.join("tools").join("dep-audit.cjs");
        cand.is_file().then_some(cand)
    })
}

/// 周任务主体：到点拉起 node tools/dep-audit.cjs（报告归档 docs/selfcheck/，明细 JSON 落 dataDir）。
/// 红线：只审计、不自动升级——任何依赖升级必须人工审阅后另行提交（M-84 PR 声明 + 人工复核）。
fn tick_dep_audit(st: &AppState) {
    let now = now_ms();
    let mut s = load_dep_audit(st);
    if !dep_audit_due(s.last_run_ms, now) {
        return;
    }
    let detail = st.data_dir.join("config").join("dep-audit-detail.json");
    let (ok, summary) = match find_dep_audit_script() {
        Some(script) => {
            let mut cmd = std::process::Command::new("node");
            cmd.arg(&script).arg("--json-out").arg(&detail);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            match cmd.output() {
                Ok(o) if o.status.success() => (true, "ok".into()),
                Ok(o) => (false, format!("script exit {:?}", o.status.code())),
                Err(e) => (false, format!("node unavailable: {e}")),
            }
        }
        None => (false, "unavailable: 安装档无开发工具链（审计属开发档门禁）".into()),
    };
    s.last_run_ms = now;
    s.last_ok = ok;
    s.summary = summary;
    let _ = save_dep_audit(st, &s);
}

// ---------------------------------------------------------------- 自更新

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCandidate {
    pub version: String,
    pub files: usize,
    /// manifest 中声明的最小可升级版本（低于则拒绝）。
    pub min_version: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    min_version: Option<String>,
    #[serde(default)]
    files: Vec<UpdateFile>,
}

#[derive(Clone, Serialize, Deserialize)]
struct UpdateFile {
    /// 相对 runtime/engine 的目标路径（禁 .. / 绝对路径 / 盘符）。
    path: String,
    sha256: String,
}

fn updates_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("updates")
}

fn engine_dir(st: &AppState) -> PathBuf {
    // 引擎运行时目录：<exeDir>/runtime/engine；不可用时退回容器内。
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("runtime").join("engine")))
        .unwrap_or_else(|| st.data_dir.join("runtime").join("engine"))
}

fn sha256_file(p: &Path) -> CmdResult<String> {
    let mut f = fs::File::open(p).map_err(|e| AppError::io(format!("打开失败: {e}")))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65_536];
    loop {
        let n = f.read(&mut buf).map_err(|e| AppError::io(e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

fn safe_rel(rel: &str) -> CmdResult<()> {
    let p = Path::new(rel);
    // Windows 下 "/abs" 不是 is_absolute（无盘符前缀），需显式拦截根相对路径
    let absolute = p.is_absolute()
        || rel.starts_with('/')
        || rel.starts_with('\\')
        || rel.contains(':')
        || rel.contains("..");
    if absolute || rel.is_empty() || p.file_name().is_none() {
        return Err(AppError::validation(format!("非法更新路径: {rel}")));
    }
    Ok(())
}

fn read_manifest(st: &AppState) -> CmdResult<(UpdateManifest, PathBuf)> {
    let mpath = updates_dir(st).join("manifest.json");
    let raw = fs::read_to_string(&mpath)
        .map_err(|_| AppError::not_found("未发现更新包（<数据目录>/updates/manifest.json）"))?;
    let m: UpdateManifest = serde_json::from_str(&raw)
        .map_err(|e| AppError::validation(format!("manifest.json 解析失败: {e}")))?;
    Ok((m, updates_dir(st).join("payload")))
}

/// 扫描本地更新包（不改动文件，仅校验 SHA-256）。
#[tauri::command(async)]
pub fn update_scan(st: tauri::State<AppState>) -> CmdResult<Option<UpdateCandidate>> {
    let (m, payload) = match read_manifest(st.inner()) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    for f in &m.files {
        safe_rel(&f.path)?;
        let p = payload.join(&f.path);
        if !p.is_file() {
            return Err(AppError::validation(format!("更新包缺文件: {}", f.path)));
        }
        let got = sha256_file(&p)?;
        if !got.eq_ignore_ascii_case(&f.sha256) {
            return Err(AppError::validation(format!("SHA-256 不匹配: {}", f.path)));
        }
    }
    Ok(Some(UpdateCandidate {
        version: m.version,
        files: m.files.len(),
        min_version: m.min_version,
    }))
}

/// 应用更新：校验 → 备份当前（保留 N=2）→ 替换 → 失败回滚。
#[tauri::command(async)]
pub fn update_apply(st: tauri::State<AppState>) -> CmdResult<String> {
    let (m, payload) = read_manifest(st.inner())?;
    if let Some(minv) = &m.min_version {
        let cur = env!("CARGO_PKG_VERSION");
        if cur < minv.as_str() {
            return Err(AppError::validation(format!(
                "当前引擎 {cur} 低于该更新要求的最低版本 {minv}，拒绝升级"
            )));
        }
    }
    // 1) 全量校验（先验后动）
    for f in &m.files {
        safe_rel(&f.path)?;
        let p = payload.join(&f.path);
        let got = sha256_file(&p)?;
        if !got.eq_ignore_ascii_case(&f.sha256) {
            return Err(AppError::validation(format!("SHA-256 不匹配: {}", f.path)));
        }
    }
    // 2) 备份将被替换的现有文件 → history/<stamp>/
    let engine = engine_dir(st.inner());
    let stamp = civil_stamp(now_ms());
    let hist = updates_dir(st.inner()).join("history").join(&stamp);
    for f in &m.files {
        let cur = engine.join(&f.path);
        if cur.is_file() {
            let bk = hist.join(&f.path);
            if let Some(parent) = bk.parent() {
                fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
            }
            fs::copy(&cur, &bk).map_err(|e| AppError::io(format!("备份失败: {e}")))?;
        }
    }
    prune_history(&updates_dir(st.inner()).join("history"), 2);
    // 3) 应用（失败 → 从 history 回滚已替换文件）
    let mut applied: Vec<String> = Vec::new();
    for f in &m.files {
        let cur = engine.join(&f.path);
        if let Some(parent) = cur.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                rollback(&engine, &hist, &applied);
                return Err(AppError::io(format!("创建目录失败，已回滚: {e}")));
            }
        }
        match fs::copy(payload.join(&f.path), &cur) {
            Ok(_) => applied.push(f.path.clone()),
            Err(e) => {
                rollback(&engine, &hist, &applied);
                return Err(AppError::io(format!("写入失败，已回滚上一版本: {e}")));
            }
        }
    }
    let _ = fs::write(
        updates_dir(st.inner()).join("last-applied.json"),
        serde_json::json!({ "version": m.version, "stamp": stamp, "files": applied.len() }).to_string(),
    );
    // 应用成功后移除 manifest（避免重复应用）。
    let _ = fs::remove_file(updates_dir(st.inner()).join("manifest.json"));
    Ok(format!("已更新到 {}（{} 个文件，历史版本 {}）", m.version, applied.len(), stamp))
}

fn rollback(engine: &Path, hist: &Path, applied: &[String]) {
    for rel in applied {
        let bk = hist.join(rel);
        if bk.is_file() {
            let _ = fs::copy(&bk, engine.join(rel));
        }
    }
}

fn prune_history(hist_root: &Path, keep: usize) {
    let mut dirs: Vec<(PathBuf, SystemTime)> = fs::read_dir(hist_root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    let mt = e.metadata().ok()?.modified().ok()?;
                    Some((e.path(), mt))
                })
                .collect()
        })
        .unwrap_or_default();
    if dirs.len() <= keep {
        return;
    }
    dirs.sort_by_key(|(_, t)| *t);
    let excess = dirs.len() - keep;
    for (p, _) in dirs.into_iter().take(excess) {
        let _ = fs::remove_dir_all(p);
    }
}

// ---------------------------------------------------------------- 自检扩展

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintainFinding {
    pub id: String,
    /// ok | warn | info
    pub level: String,
    pub message: String,
}

/// 数据自检扩展（F-6.2）：备份可恢复性抽检 + 容器索引一致性抽检 + 媒体孤儿引用。
#[tauri::command(async)]
pub fn maintain_selfcheck(st: tauri::State<AppState>) -> CmdResult<Vec<MaintainFinding>> {
    let mut out: Vec<MaintainFinding> = Vec::new();

    // 1) 备份可恢复性抽检：最新备份 quick_check（只读，不演练写回）
    let newest: Option<(String, i64)> = st.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT file_name,created_at FROM backups WHERE status='ok' ORDER BY created_at DESC LIMIT 1")
            .map_err(AppError::from)?;
        let mut rows = stmt.query([]).map_err(AppError::from)?;
        match rows.next().map_err(AppError::from)? {
            Some(r) => Ok(Some((r.get(0)?, r.get(1)?))),
            None => Ok(None),
        }
    })?;
    match newest {
        None => out.push(MaintainFinding {
            id: "backup-drill".into(),
            level: "info".into(),
            message: "尚无备份记录；建议开启计划备份".into(),
        }),
        Some((name, ts)) => {
            let p = st.backups_dir.join(&name);
            if !p.is_file() {
                out.push(MaintainFinding {
                    id: "backup-drill".into(),
                    level: "warn".into(),
                    message: format!("最新备份文件缺失: {name}"),
                });
            } else {
                let check = rusqlite::Connection::open_with_flags(
                    &p,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .and_then(|c| c.query_row("PRAGMA quick_check;", [], |r| r.get::<_, String>(0)));
                match check {
                    Ok(s) if s == "ok" => out.push(MaintainFinding {
                        id: "backup-drill".into(),
                        level: "ok".into(),
                        message: format!("最新备份可恢复性抽检通过（{name}，{}）", civil_stamp(ts)),
                    }),
                    Ok(s) => out.push(MaintainFinding {
                        id: "backup-drill".into(),
                        level: "warn".into(),
                        message: format!("备份完整性异常（{s}）: {name}"),
                    }),
                    Err(e) => out.push(MaintainFinding {
                        id: "backup-drill".into(),
                        level: "warn".into(),
                        message: format!("备份校验失败: {e}"),
                    }),
                }
            }
        }
    }

    // 2) 容器索引一致性抽检：快照抽样 50 条，检查路径仍存在
    let sample = crate::shell::fsindex::sample_entries(50);
    if sample.is_empty() {
        out.push(MaintainFinding {
            id: "index-consistency".into(),
            level: "info".into(),
            message: "索引尚未构建，跳过一致性抽检".into(),
        });
    } else {
        let missing: Vec<&str> = sample
            .iter()
            .filter(|e| !e.is_dir && !Path::new(&e.path).exists())
            .map(|e| e.path.as_str())
            .take(3)
            .collect();
        if missing.is_empty() {
            out.push(MaintainFinding {
                id: "index-consistency".into(),
                level: "ok".into(),
                message: format!("索引一致性抽检通过（抽样 {} 条）", sample.len()),
            });
        } else {
            out.push(MaintainFinding {
                id: "index-consistency".into(),
                level: "warn".into(),
                message: format!("索引发现失效条目（示例 {} 条，等待下轮重建收敛）", missing.len()),
            });
        }
    }

    // 3) 媒体孤儿引用：media 目录文件未被 DB 登记（copied=1 的登记对照）
    let orphans = media_orphans(st.inner());
    match orphans {
        Ok(n) if n == 0 => out.push(MaintainFinding {
            id: "media-orphans".into(),
            level: "ok".into(),
            message: "媒体目录无孤儿文件".into(),
        }),
        Ok(n) => out.push(MaintainFinding {
            id: "media-orphans".into(),
            level: "warn".into(),
            message: format!("发现 {n} 个未登记媒体文件（建议清理，不动 DB 记录）"),
        }),
        Err(e) => out.push(MaintainFinding {
            id: "media-orphans".into(),
            level: "info".into(),
            message: format!("媒体抽检跳过: {e}"),
        }),
    }

    Ok(out)
}

fn media_orphans(st: &AppState) -> Result<usize, String> {
    let mut registered: std::collections::HashSet<String> = std::collections::HashSet::new();
    let q: CmdResult<()> = st.with_conn(|conn| {
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT file_name FROM media WHERE copied=1")
                .map_err(AppError::from)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(AppError::from)?;
            rows.flatten().collect()
        };
        registered.extend(names);
        Ok(())
    });
    q.map_err(|e| e.to_string())?;
    let mut n = 0usize;
    let rd = fs::read_dir(&st.media_dir).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if !registered.contains(&name) {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_blocks_traversal() {
        assert!(safe_rel("bin/app.exe").is_ok());
        assert!(safe_rel("a/b/c.dll").is_ok());
        assert!(safe_rel("../escape").is_err());
        assert!(safe_rel("C:/win").is_err());
        assert!(safe_rel("").is_err());
        assert!(safe_rel("/abs").is_err());
    }

    #[test]
    fn schedule_interval_sane() {
        assert_eq!(interval_ms("none"), 0);
        assert_eq!(interval_ms("daily"), 86_400_000);
        assert_eq!(interval_ms("weekly"), 604_800_000);
    }

    #[test]
    fn civil_stamp_formats() {
        assert_eq!(civil_stamp(0), "19700101-000000");
    }

    #[test]
    fn dep_audit_due_cycle() {
        assert!(dep_audit_due(0, 1_000)); // 从未跑过 → 立即到期
        assert!(!dep_audit_due(1_000, 1_000 + DEP_AUDIT_INTERVAL_MS - 1));
        assert!(dep_audit_due(1_000, 1_000 + DEP_AUDIT_INTERVAL_MS)); // 满 7 天到期
    }
}
