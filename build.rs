// This runs BEFORE src/main.rs is compiled. You should not need to touch it.
//
// libibverbs is a C library, so Rust needs a description of its functions and
// structs. The tool `bindgen` reads the C header <infiniband/verbs.h> and
// writes that description into a file called bindings.rs, which main.rs loads.
//
// One complication: some verbs (ibv_post_send, ibv_poll_cq, ...) are
// "static inline" in C, meaning they exist only inside the header and not
// inside libibverbs.so, so Rust has nothing to call. `wrap_static_fns` makes
// bindgen write a tiny C file with a normal, callable copy of each of them;
// `cc` then compiles that file into our program.
use std::{env, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let header = format!("{}/wrapper.h", env::var("CARGO_MANIFEST_DIR").unwrap());

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rustc-link-lib=ibverbs"); // use the system's libibverbs.so

    bindgen::Builder::default()
        .header(&header)
        .allowlist_function("_*ibv_.*") // only the verbs API, nothing else
        .allowlist_type("ibv_.*")
        .allowlist_var("IBV_.*")
        .prepend_enum_name(false) // constants keep their C names, e.g. IBV_QPS_INIT
        .wrap_static_fns(true)
        .wrap_static_fns_path(out.join("static_wrappers"))
        .generate()
        .expect("bindgen could not read infiniband/verbs.h")
        .write_to_file(out.join("bindings.rs"))
        .expect("could not write bindings.rs");

    cc::Build::new()
        .file(out.join("static_wrappers.c"))
        .compile("static_wrappers");
}
