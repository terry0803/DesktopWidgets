// Links the icons and the program's name for Windows, made by resources/make.py.
fn main() {
    println!("cargo:rerun-if-changed=resources/app.o");
    let resources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("app.o");
    println!("cargo:rustc-link-arg-bins={}", resources.display());
}
