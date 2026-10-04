//! 端到端实测：直连真实 WorkBuddy 跑一次三源采集。
//!
//! 用法：cargo run --example live_probe
//! 不依赖 GUI，只验Rust 侧能否读出对话 / 模型 / 工作目录。

use varix_autopilot_lib::cdp::Cdp;
use varix_autopilot_lib::collect::{self, CwdCache};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("== 连接 CDP ==");
    let cdp = Cdp::connect("http://127.0.0.1:9222").await?;
    println!("   已连接");

    println!("\n== 采集快照 ==");
    let mut cache = CwdCache::new();
    let t0 = std::time::Instant::now();
    let idx = cache.refresh();
    println!("   cwd 索引: {} 个 sessionId -> cwd", idx.by_sid.len());
    let snap = collect::probe_with_index(&cdp, idx).await?;
    println!("   耗时 {} ms", t0.elapsed().as_millis());

    println!("\n== 结果 ==");
    println!("   版本      : {}", snap.version);
    println!("   当前对话  : {}", snap.conversation_title);
    println!("   当前模型  : {}", snap.current_model);
    println!("   发送中    : {}", snap.sending);
    println!("   编辑器字符: {}", snap.editor_chars);
    println!("   编辑器可见: {}", snap.editor_visible);
    println!("   会话总数  : {}", snap.session_count);
    println!("   对话数    : {}", snap.convs.len());

    println!("\n== 对话列表 ==");
    for c in &snap.convs {
        let mark = if c.selected { "*" } else { " " };
        println!("{} [{}] {}", mark, c.index, c.title);
        println!("      id={}  时间={}", c.conv_id, c.rel_time);
        if c.model.is_empty() {
            println!("      模型: (侧栏不显示)");
        } else {
            println!("      模型: {}", c.model);
        }
        if c.cwd.is_empty() {
            println!("      目录: ({})", c.cwd_confidence);
        } else {
            let short: String = c.cwd.chars().rev().take(46).collect::<Vec<_>>().into_iter().rev().collect();
            println!("      目录: ...{}", short);
        }
    }

    println!("\n== 工作目录分布 (top6) ==");
    for h in &snap.cwd_histogram {
        let short: String = h.cwd.chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
        println!("   {:>3} 个  ...{}", h.sessions, short);
    }
    Ok(())
}
