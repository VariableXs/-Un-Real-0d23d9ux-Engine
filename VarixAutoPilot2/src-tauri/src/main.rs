// VarixAutoPilot2 · 可执行入口
// Windows 发布版不弹控制台窗口（#![windows_subsystem]），
// 但保留 debug 断言与 stderr 便于排查。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    varix_autopilot_lib::run()
}
