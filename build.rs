fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("freebsd") {
        cc::Build::new()
            .file("src/freebsd11_shim.c")
            .compile("freebsd11_shim");
    }
}
