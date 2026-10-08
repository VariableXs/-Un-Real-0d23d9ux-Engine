//! 验证忙闲保护：正在生成时必须拒绝，且**一个字都不写进输入框**。
//!
//! 这是安全闸门，不是功能。宁可拒发，也不能打断对方正在跑的活。
//! 干跑模式不受此限（干跑的意义就是忙碌时也能预演）。
use varix_autopilot_lib::cdp::Cdp;
use varix_autopilot_lib::engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cdp = Cdp::connect("http://127.0.0.1:9222").await?;
    println!("== 忙闲状态 ==");
    let b = engine::busy_state(&cdp).await?;
    println!("   sending = {}  label = {:?}", b.sending, b.label);

    // 读填入前的编辑器字数（用于证明"没动过"）
    let before = cdp
        .eval("(()=>{const e=document.querySelector('div[data-slate-editor=\"true\"][contenteditable=\"true\"]');return e?(e.innerText||'').trim().length:-1})()")
        .await?;
    let before_n = before.as_i64().unwrap_or(-1);
    println!("\n== 填入前编辑器字数 ==");
    println!("   {before_n}");

    let probe = "忙闲保护实测探针文本 XYZ";
    println!("\n== 干跑填入（忙碌时也允许，用于验证能否写入）==");
    match engine::run_flow(&cdp, probe, true, false).await {
        Ok(f) => println!("   dry_run 成功：写入 {} 字符 · {}", f.chars, f.evidence),
        Err(engine::EngineError::Busy(m)) => println!("   忙时拒绝：{m}"),
        Err(engine::EngineError::Other(e)) => println!("   错误：{e:#}"),
    }

    let after = cdp
        .eval("(()=>{const e=document.querySelector('div[data-slate-editor=\"true\"][contenteditable=\"true\"]');return e?(e.innerText||'').trim().length:-1})()")
        .await?;
    let after_n = after.as_i64().unwrap_or(-1);
    println!("\n== 填入后编辑器字数 ==");
    println!("   {after_n}   （比填入前多 {} 字符）", after_n - before_n);

    println!("\n== 真发（忙碌时应被拒）==");
    match engine::run_flow(&cdp, probe, false, false).await {
        Ok(f) => println!("   返回 ok={} evidence={}", f.ok, f.evidence),
        Err(engine::EngineError::Busy(m)) => println!("   ★ 忙时拒绝（正确）★\n   {m}"),
        Err(engine::EngineError::Other(e)) => println!("   错误：{e:#}"),
    }
    Ok(())
}
