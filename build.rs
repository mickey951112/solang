use std::process::Command;

fn main() {
    lalrpop::Configuration::new()
        .generate_in_source_tree()
        .emit_rerun_directives(true)
        .process()
        .unwrap();

    // compile our linker
    let cxxflags = Command::new("llvm-config")
        .args(&["--cxxflags"])
        .output()
        .unwrap();

    let cxxflags = String::from_utf8(cxxflags.stdout).unwrap();

    let mut build = cc::Build::new();

    build.file("src/linker/linker.cpp").cpp(true);

    if !cfg!(target_os = "windows") {
        build.flag("-Wno-unused-parameter");
    }

    for flag in cxxflags.split_whitespace() {
        build.flag(flag);
    }

    build.compile("liblinker.a");

    // add the llvm linker
    let libdir = Command::new("llvm-config")
        .args(&["--libdir"])
        .output()
        .unwrap();
    let libdir = String::from_utf8(libdir.stdout).unwrap();

    println!("cargo:libdir={}", libdir);
    for lib in &["lldELF", "lldDriver", "lldCore", "lldCommon", "lldWasm"] {
        println!("cargo:rustc-link-lib=static={}", lib);
    }

    // And all the symbols were not using, needed by Windows and debug builds
    for lib in &["lldReaderWriter", "lldMachO", "lldYAML"] {
        println!("cargo:rustc-link-lib=static={}", lib);
    }

    // note: add error checking yourself.
    let output = Command::new("git")
        .args(&["describe", "--tags"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
