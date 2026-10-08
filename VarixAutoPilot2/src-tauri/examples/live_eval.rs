//! 验证 Runtime.evaluate 是否真能执行。
//! 逐层缩小：先测 1+1，再测 document.title，再测选择器。
use varix_autopilot_lib::cdp::Cdp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cdp = Cdp::connect("http://127.0.0.1:9222").await?;
    println!("connected");

    for (name, expr) in [
        ("1+1", "1+1"),
        ("location.href", "location.href"),
        ("document.title", "document.title"),
        ("readyState", "document.readyState"),
        ("body存在", "!!document.body"),
        ("div总数", "document.querySelectorAll('div').length"),
        ("conversation-item", "document.querySelectorAll('div.conversation-item').length"),
        ("任意class样本", "Array.from(document.querySelectorAll('div')).slice(0,5).map(e=>String(e.className)).join('|')"),
        ("slate编辑器", "document.querySelectorAll('div[data-slate-editor]').length"),
        ("发送键", "document.querySelectorAll('button.cr-send-button').length"),
    ] {
        let t = std::time::Instant::now();
        match cdp.eval(expr).await {
            Ok(v) => {
                let s = v.to_string();
                let shown = if s.len() > 110 { format!("{}…", &s[..110]) } else { s };
                println!("{:<18} OK  {:>5}ms  {}", name, t.elapsed().as_millis(), shown);
            }
            Err(e) => println!("{:<18} ERR {:>5}ms  {e}", name, t.elapsed().as_millis()),
        }
    }
    Ok(())
}
