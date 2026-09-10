fn main() {
    // 前端是编译期嵌入的（tauri.conf.json 的 frontendDist），但 tauri-build 只为
    // tauri.conf.json / capabilities 发 rerun-if-changed —— 只改 frontend/ 再 cargo build
    // 不会重新嵌入，改了不生效还找不到原因。这里把整个前端目录纳入构建依赖。
    println!("cargo:rerun-if-changed=frontend");
    tauri_build::build()
}
