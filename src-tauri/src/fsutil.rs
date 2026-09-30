//! fsutil — 原子写工具。
//!
//! 关键状态文件（登记表 / 标签 / 剪贴板历史 / 回收策略 / 视图布局等）历史上
//! 直接 `fs::write`：进程在写入途中崩溃或断电会留下截断文件，下次启动状态
//! 全损。`atomic_write` 先写同目录临时文件（带进程号防并发碰撞）→ `sync_all`
//! 落盘 → `rename` 覆盖目标（Windows 上 Rust 的 rename 内部走
//! MOVEFILE_REPLACE_EXISTING，同为原子替换）。任一时刻目标文件要么是旧完整
//! 内容、要么是新完整内容，不存在中间态。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 原子替换写入：tmp(带 pid) → fsync → rename 覆盖。
/// rename 失败（个别杀软/索引器短暂锁文件）时退避：删目标再改名。
pub fn atomic_write<P: AsRef<Path>, B: AsRef<[u8]>>(path: P, bytes: B) -> io::Result<()> {
    let path = path.as_ref();
    let bytes = bytes.as_ref();
    let tmp = tmp_sibling(path);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows 上极少见的锁竞争退避：目标被短暂占用时先移除再改名。
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path)
        }
    }
}

/// 同目录临时文件名：`<name>.<pid>-<seq>.tmp`——同进程内每次调用唯一
/// （多线程并发写同一目标不碰撞），跨进程靠 pid 区分。崩溃残留的
/// 极少量 .tmp 不会被任何读取方引用，属无害垃圾。
fn tmp_sibling(path: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "state".into());
    name.push_str(&format!(".{}-{seq}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("variable-fsutil-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("state.json");
        atomic_write(&p, b"{\"v\":1}").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"{\"v\":1}");
        atomic_write(&p, b"{\"v\":2,\"longer\":true}").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"{\"v\":2,\"longer\":true}");
        // 无残留临时文件
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留 .tmp 文件");
        let _ = fs::remove_dir_all(&dir);
    }
}
