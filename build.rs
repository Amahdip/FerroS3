fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("freebsd") {
        // FreeBSD 11.x still needs pthread_setname_np compatibility.
        cc::Build::new()
            .file("src/freebsd11_shim.c")
            .compile("freebsd11_shim");
    }
}
