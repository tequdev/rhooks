//! Compiles the vendored upstream guard checker (`docs/DESIGN.md` §6.5)
//! into `rshooks-build`. The only C++ source is `cpp/guard_shim.cpp`; the
//! rest is byte-identical upstream `xahaud` headers under
//! `vendor/xahaud/` (see `vendor/xahaud/VENDOR.md`), compiled unmodified
//! with `-DGUARD_CHECKER_BUILD` (upstream's own supported standalone mode).

fn main() {
    let vendor_dir = "vendor/xahaud";

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(vendor_dir)
        .define("GUARD_CHECKER_BUILD", None)
        .warnings(false)
        .file("cpp/guard_shim.cpp")
        .compile("rhooks_guard_shim");

    println!("cargo:rerun-if-changed=cpp/guard_shim.cpp");
    println!("cargo:rerun-if-changed={vendor_dir}/Guard.h");
    println!("cargo:rerun-if-changed={vendor_dir}/Enum.h");
    println!("cargo:rerun-if-changed={vendor_dir}/hook_api.macro");
}
