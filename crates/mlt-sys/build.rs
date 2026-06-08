fn main() {
    let mlt = pkg_config::Config::new()
        .atleast_version("7.0")
        .probe("mlt-framework-7")
        .expect("MLT framework 7 not found. Install with: brew install mlt");

    // Build clang include args from pkg-config
    let mut clang_args: Vec<String> = Vec::new();
    for path in &mlt.include_paths {
        clang_args.push(format!("-I{}", path.to_string_lossy()));
    }

    // Generate bindings for the key MLT types we use
    // Note: pkg-config include path is .../include/mlt-7, so headers are relative to that
    let bindings = bindgen::Builder::default()
        .header_contents("mlt_wrapper.h", r#"
            #include <framework/mlt_factory.h>
            #include <framework/mlt_profile.h>
            #include <framework/mlt_producer.h>
            #include <framework/mlt_consumer.h>
            #include <framework/mlt_playlist.h>
            #include <framework/mlt_tractor.h>
            #include <framework/mlt_frame.h>
            #include <framework/mlt_filter.h>
            #include <framework/mlt_transition.h>
            #include <framework/mlt_properties.h>
            #include <framework/mlt_service.h>
            #include <framework/mlt_field.h>
        "#)
        .clang_args(&clang_args)
        .allowlist_type("mlt_.*")
        .allowlist_function("mlt_.*")
        .allowlist_var("mlt_.*")
        .blocklist_type("FP_NAN|FP_INFINITE|FP_ZERO|FP_SUBNORMAL|FP_NORMAL")
        .generate_comments(false)
        .layout_tests(false)
        .derive_debug(false)
        .derive_default(true)
        .derive_partialeq(true)
        .generate()
        .expect("Failed to generate MLT bindings");

    // Write bindings
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::PathBuf::from(out_dir).join("bindings.rs");
    bindings
        .write_to_file(&out_path)
        .expect("Failed to write MLT bindings");

    // Emit linker flags
    for lib in &mlt.libs {
        println!("cargo:rustc-link-lib={}", lib);
    }
    for path in &mlt.link_paths {
        println!("cargo:rustc-link-search=native={}", path.to_string_lossy());
    }
}
