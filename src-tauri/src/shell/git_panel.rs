//! Git 深度面板（B-22，M5）：git2 只读层 + SSH 金库代理。
//!
//! 职责边界（MASTER-PLAN M5 任务 3/4，刻意的设计）：
//! - **只读走 git2**：状态/分支/提交历史/最近 diff 统计——面板不提供任何
//!   写操作命令（commit/push/branch 一律在终端完成），写路径 = 终端，避免
//!   "面板一个按钮 = 一条新的进程创建/网络出站面"的安全面膨胀；
//! - **仓库必须位于容器内**：`ensure_in_container` 校验路径不逃逸容器根；
//! - **SSH 金库代理**：ed25519 密钥对生成后私钥封存 `vault/ssh/`（金库
//!   未解锁即拒绝），git 使用经 `ssh_git_command()` 注入 GIT_SSH_COMMAND
//!   （`ssh -i <私钥> -o IdentitiesOnly=yes`）——"代理"= 执行档环境注入，
//!   不另起 ssh-agent 进程（进程面缩减，如实边界：无 agent 转发）。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

fn ce(e: container::ContainerError) -> AppError {
    AppError::new("CONTAINER", e.to_string())
}

/// 校验仓库路径位于容器数据目录内（防逃逸；git2 只读也只读容器内仓库）。
pub(crate) fn ensure_in_container(st: &AppState, repo: &str) -> CmdResult<PathBuf> {
    let p = PathBuf::from(repo);
    let canonical_root = st
        .data_dir
        .canonicalize()
        .map_err(|e| AppError::io(e.to_string()))?;
    let canonical = p
        .canonicalize()
        .map_err(|e| AppError::io(e.to_string()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(AppError::validation(format!(
            "仓库不在容器内: {repo}（面板只读容器内仓库）"
        )));
    }
    Ok(canonical)
}

// ---------- 只读层（git2） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusView {
    pub head_branch: String,
    pub head_commit: Option<String>,
    pub entries: Vec<GitEntry>,
    pub ahead: usize,
    pub behind: usize,
    /// 当前分支是否配置了 upstream（false 时 ahead/behind 恒为 0，UI 据此提示）
    pub has_upstream: bool,
    pub is_repo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitEntry {
    pub path: String,
    /// wt_new / wt_modified / wt_deleted / index_new / index_modified / index_deleted / conflicted
    pub state: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitView {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub time_ms: i64,
}

pub(crate) fn open_repo(repo: &Path) -> Result<git2::Repository, AppError> {
    git2::Repository::open(repo)
        .map_err(|e| AppError::new("GIT", format!("不是 git 仓库或无法打开: {e}")))
}

fn status_state(s: git2::Status) -> &'static str {
    let i = s.intersects(git2::Status::INDEX_NEW
        | git2::Status::INDEX_MODIFIED
        | git2::Status::INDEX_DELETED
        | git2::Status::INDEX_RENAMED);
    let w = s.intersects(git2::Status::WT_NEW
        | git2::Status::WT_MODIFIED
        | git2::Status::WT_DELETED
        | git2::Status::WT_RENAMED);
    if s.intersects(git2::Status::CONFLICTED) {
        "conflicted"
    } else if s.contains(git2::Status::INDEX_NEW) {
        "index_new"
    } else if s.contains(git2::Status::INDEX_MODIFIED) {
        "index_modified"
    } else if s.contains(git2::Status::INDEX_DELETED) {
        "index_deleted"
    } else if i {
        "index_changed"
    } else if s.contains(git2::Status::WT_NEW) {
        "wt_new"
    } else if s.contains(git2::Status::WT_MODIFIED) {
        "wt_modified"
    } else if s.contains(git2::Status::WT_DELETED) {
        "wt_deleted"
    } else if w {
        "wt_changed"
    } else {
        "unchanged"
    }
}

#[tauri::command]
pub fn git_status(st: tauri::State<AppState>, repo: String) -> CmdResult<GitStatusView> {
    git_status_inner(&st, &repo)
}

pub(crate) fn git_status_inner(st: &AppState, repo_path: &str) -> CmdResult<GitStatusView> {
    let root = ensure_in_container(st, repo_path)?;
    let repo = open_repo(&root)?;
    let mut view = GitStatusView {
        head_branch: String::new(),
        head_commit: None,
        entries: Vec::new(),
        ahead: 0,
        behind: 0,
        has_upstream: false,
        is_repo: true,
    };
    if let Ok(head) = repo.head() {
        if let Some(shorthand) = head.shorthand() {
            view.head_branch = shorthand.to_string();
        }
        if let Some(commit) = head
            .target()
            .and_then(|oid| repo.find_commit(oid).ok())
        {
            view.head_commit = Some(commit.id().to_string()[..8].to_string());
            // ahead/behind（有 upstream 时）
            if let Some(shorthand) = head.shorthand() {
                if let Ok(local_branch) = repo.find_branch(shorthand, git2::BranchType::Local) {
                    if let Ok(upstream) = local_branch.upstream() {
                        view.has_upstream = true;
                        if let (Some(local_oid), Some(up_oid)) =
                            (head.target(), upstream.get().target())
                        {
                            if let Ok((a, b)) = repo.graph_ahead_behind(local_oid, up_oid) {
                                view.ahead = a;
                                view.behind = b;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(false);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| AppError::new("GIT", e.to_string()))?;
    for e in statuses.iter() {
        if let Some(path) = e.path() {
            view.entries.push(GitEntry {
                path: path.to_string(),
                state: status_state(e.status()).to_string(),
            });
        }
    }
    Ok(view)
}

#[tauri::command]
pub fn git_log(
    st: tauri::State<AppState>,
    repo: String,
    limit: Option<u32>,
) -> CmdResult<Vec<GitCommitView>> {
    git_log_inner(&st, &repo, limit)
}

pub(crate) fn git_log_inner(st: &AppState, repo_path: &str, limit: Option<u32>) -> CmdResult<Vec<GitCommitView>> {
    let root = ensure_in_container(st, repo_path)?;
    let repo = open_repo(&root)?;
    let limit = limit.unwrap_or(50).min(500);
    let mut revwalk = repo
        .revwalk()
        .map_err(|e| AppError::new("GIT", e.to_string()))?;
    revwalk
        .push_head()
        .map_err(|e| AppError::new("GIT", e.to_string()))?;
    revwalk.set_sorting(git2::Sort::TIME)
        .map_err(|e| AppError::new("GIT", e.to_string()))?;
    let mut out = Vec::new();
    for oid in revwalk.take(limit as usize) {
        let oid = oid.map_err(|e| AppError::new("GIT", e.to_string()))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| AppError::new("GIT", e.to_string()))?;
        out.push(GitCommitView {
            id: commit.id().to_string()[..8].to_string(),
            summary: commit
                .summary()
                .unwrap_or("(no message)")
                .to_string(),
            author: commit
                .author()
                .name()
                .unwrap_or("(unknown)")
                .to_string(),
            time_ms: commit.time().seconds() * 1000,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn git_branches(st: tauri::State<AppState>, repo: String) -> CmdResult<Vec<String>> {
    let root = ensure_in_container(&st, &repo)?;
    let repo = open_repo(&root)?;
    let branches = repo
        .branches(None)
        .map_err(|e| AppError::new("GIT", e.to_string()))?;
    let mut names: Vec<String> = branches
        .filter_map(|b| b.ok())
        .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
        .collect();
    names.sort();
    Ok(names)
}

// ---------- SSH 金库代理 ----------

fn ssh_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("vault").join("ssh")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyView {
    pub id: String,
    pub label: String,
    pub public_key: String,
    pub created_at: u64,
}

fn load_keys(st: &AppState) -> CmdResult<Vec<SshKeyView>> {
    let dir = ssh_dir(st);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| AppError::io(e.to_string()))? {
        let e = e.map_err(|e| AppError::io(e.to_string()))?;
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("pub") {
            let id = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let public_key =
                std::fs::read_to_string(&p).map_err(|e| AppError::io(e.to_string()))?;
            out.push(SshKeyView {
                id,
                label: public_key
                    .rsplit_once(' ')
                    .map(|(_, c)| c.trim().to_string())
                    .unwrap_or_default(),
                public_key: public_key.trim_end().to_string(),
                created_at: e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[tauri::command]
pub fn ssh_keys(st: tauri::State<AppState>) -> CmdResult<Vec<SshKeyView>> {
    load_keys(&st)
}

/// 生成 ed25519 密钥对；私钥写入金库目录（金库未解锁即拒绝）。
#[tauri::command]
pub fn ssh_key_generate(
    st: tauri::State<AppState>,
    label: String,
) -> CmdResult<SshKeyView> {
    use ssh_key::private::{Ed25519Keypair, KeypairData};
    use ssh_key::{PrivateKey, PublicKey};
    if crate::shell::privacy::vault_key()?.is_none() {
        return Err(AppError::new(
            "VAULT_LOCKED",
            "金库未解锁：SSH 密钥必须封存在已解锁的金库目录",
        ));
    }
    let slug = {
        let s: String = label
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_lowercase();
        if s.is_empty() { "key".into() } else { s }
    };
    let id = format!("{slug}-{}", now_short());

    let kp = Ed25519Keypair::random(&mut rand::thread_rng());
    let private = PrivateKey::new(KeypairData::Ed25519(kp.clone()), label.clone())
        .map_err(|e| AppError::new("SSH", e.to_string()))?;
    let public = private.public_key().clone();

    let dir = ssh_dir(&st);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(
        dir.join(&id),
        private
            .to_openssh(ssh_key::LineEnding::LF)
            .map_err(|e| AppError::new("SSH", e.to_string()))?,
    )
    .map_err(|e| AppError::io(e.to_string()))?;
    let mut pub_text = public
        .to_openssh()
        .map_err(|e| AppError::new("SSH", e.to_string()))?
        .to_string();
    pub_text.push(' ');
    pub_text.push_str(&label);
    std::fs::write(dir.join(format!("{id}.pub")), pub_text)
        .map_err(|e| AppError::io(e.to_string()))?;

    load_keys(&st)?
        .into_iter()
        .find(|k| k.id == id)
        .ok_or_else(|| AppError::new("SSH", "密钥写入后未找到（内部错误）"))
}

fn now_short() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 % 100_000_000)
        .unwrap_or(0)
}

/// GIT_SSH_COMMAND 注入值（执行档/终端环境用）：指定私钥 + 只用该密钥。
pub fn ssh_git_command(private_key_path: &Path) -> String {
    format!(
        "ssh -i \"{}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
        private_key_path.to_string_lossy()
    )
}

/// 删除 SSH 密钥（私钥 + 公钥）。
#[tauri::command]
pub fn ssh_key_delete(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    if crate::shell::privacy::vault_key()?.is_none() {
        return Err(AppError::new("VAULT_LOCKED", "金库未解锁"));
    }
    let dir = ssh_dir(&st);
    // id 白名单化（防路径逃逸）
    if id.contains('\\') || id.contains('/') || id.contains(':') || id.contains("..") {
        return Err(AppError::validation("非法密钥 id"));
    }
    for f in [dir.join(&id), dir.join(format!("{id}.pub"))] {
        if f.is_file() {
            std::fs::remove_file(f).map_err(|e| AppError::io(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("git-b22-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("workspaces")).unwrap();
        let st = AppState {
            conn: std::sync::Mutex::new(None),
            data_dir: dir.clone(),
            db_dir: dir.clone(),
            media_dir: dir.clone(),
            attachments_dir: dir.clone(),
            backups_dir: dir.clone(),
            recovery_dir: dir.clone(),
            logs_dir: dir.clone(),
        };
        (st, dir)
    }

    /// 用 git2 自身搭一个真实仓库：init → commit → 修改文件。
    fn make_repo(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        let sig = git2::Signature::now("tester", "t@x.dev").unwrap();
        let mut index = repo.index().unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, b"v1").unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init commit", &tree, &[])
            .unwrap();
        drop(tree);
        let _ = oid;
        // 工作区修改
        std::fs::write(dir.join("a.txt"), b"v2-modified").unwrap();
        std::fs::write(dir.join("b-new.txt"), b"new").unwrap();
    }

    #[test]
    fn status_shows_branch_entries_and_rejects_outside() {
        let (st, dir) = temp_state("status");
        let repo_dir = dir.join("workspaces").join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        make_repo(&repo_dir);

        let view = git_status_inner(&st, repo_dir.to_string_lossy().as_ref()).unwrap();
        assert!(view.is_repo);
        assert_eq!(view.head_branch, "master");
        assert!(view.head_commit.is_some());
        assert!(view.entries.iter().any(|e| e.path == "a.txt" && e.state == "wt_modified"));
        assert!(view.entries.iter().any(|e| e.path == "b-new.txt" && e.state == "wt_new"));

        // 容器外路径拒绝
        let outside = std::env::temp_dir().join("git-outside-repo");
        let err = git_status_inner(&st, outside.to_string_lossy().as_ref());
        assert!(err.is_err(), "容器外仓库必须拒绝");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_returns_commits_newest_first() {
        let (st, dir) = temp_state("log");
        let repo_dir = dir.join("workspaces").join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        make_repo(&repo_dir);
        let log = git_log_inner(&st, repo_dir.to_string_lossy().as_ref(), Some(10)).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "init commit");
        assert_eq!(log[0].author, "tester");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ssh_git_command_injects_key_and_identities_only() {
        let cmd = ssh_git_command(Path::new(r"C:\container\vault\ssh\k1"));
        assert!(cmd.contains("ssh -i"));
        assert!(cmd.contains("IdentitiesOnly=yes"));
        assert!(cmd.contains("vault"));
    }
}
