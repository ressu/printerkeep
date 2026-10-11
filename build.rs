// Gives the build target to the program, so `printerkeep install` knows the name of its release asset.
fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET");
    println!("cargo:rustc-env=PRINTERKEEP_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
