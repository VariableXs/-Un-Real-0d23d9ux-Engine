// VarixAutoPilot2 · Tauri 构建脚本
//
// 只做一件事：让 Cargo 知道 tauri.conf.json 在哪。
// 复杂逻辑不要放这里 —— build.rs 每��重新编译，放多了会拖慢构建。

fn main() {
    tauri_build::build()
}
