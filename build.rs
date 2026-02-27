fn main() {
    // 当 locales 目录下任何文件变化时，触发重新编译
    println!("cargo:rerun-if-changed=locales/");
}
