use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("vendor").join("lame");
    let lib_src = vendor_dir.join("libmp3lame");
    let inc_dir = vendor_dir.join("include");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    let c_files = [
        "bitstream.c",
        "encoder.c",
        "fft.c",
        "gain_analysis.c",
        "id3tag.c",
        "lame.c",
        "newmdct.c",
        "presets.c",
        "psymodel.c",
        "quantize.c",
        "quantize_pvt.c",
        "reservoir.c",
        "set_get.c",
        "tables.c",
        "takehiro.c",
        "util.c",
        "vbrquantize.c",
        "VbrTag.c",
        "version.c",
    ];

    println!("cargo:rerun-if-changed=vendor/lame");
    println!("cargo:rerun-if-changed=lame.def");

    let prebuilt_dist = manifest_dir.join("..").join("..").join("target").join("lame-dist");
    if target.contains("windows")
        && prebuilt_dist.join("mp3lame.lib").exists()
        && prebuilt_dist.join("libmp3lame.dll").exists()
    {
        let _ = std::fs::copy(prebuilt_dist.join("mp3lame.lib"), out_dir.join("mp3lame.lib"));
        let _ = std::fs::copy(prebuilt_dist.join("libmp3lame.dll"), out_dir.join("libmp3lame.dll"));
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=dylib=mp3lame");
        return;
    }

    let mut build = cc::Build::new();
    build.include(&lib_src);
    build.include(&inc_dir);
    build.define("HAVE_CONFIG_H", None);
    build.warnings(false);
    build.pic(true);

    for file in &c_files {
        build.file(lib_src.join(file));
    }

    if target.contains("windows") {
        if target.contains("msvc") {
            // For MSVC target: compile objects, then run link.exe to generate libmp3lame.dll and mp3lame.lib
            let objs = build.compile_intermediates();
            let def_file = manifest_dir.join("lame.def");
            let dll_out = out_dir.join("libmp3lame.dll");
            let lib_out = out_dir.join("mp3lame.lib");

            let mut link_cmd = Command::new("link.exe");
            link_cmd.arg("/DLL")
                .arg(format!("/DEF:{}", def_file.display()))
                .arg(format!("/OUT:{}", dll_out.display()))
                .arg(format!("/IMPLIB:{}", lib_out.display()))
                .arg("/NOLOGO");

            for obj in &objs {
                link_cmd.arg(obj);
            }

            let status = link_cmd.status().expect("Failed to execute link.exe for libmp3lame.dll");
            if !status.success() {
                panic!("link.exe failed to create libmp3lame.dll");
            }

            // Also copy libmp3lame.dll to CARGO_TARGET_DIR or project root if possible
            println!("cargo:rustc-link-search=native={}", out_dir.display());
            println!("cargo:rustc-link-lib=dylib=mp3lame");
        } else {
            // MinGW / gnu target on Windows
            let objs = build.compile_intermediates();
            let dll_out = out_dir.join("libmp3lame.dll");
            let implib_out = out_dir.join("libmp3lame.dll.a");
            let def_file = manifest_dir.join("lame.def");

            let mut cmd = Command::new("gcc");
            cmd.arg("-shared")
                .arg("-o").arg(&dll_out)
                .arg(format!("-Wl,--out-implib,{}", implib_out.display()))
                .arg(&def_file);

            for obj in &objs {
                cmd.arg(obj);
            }

            let status = cmd.status().expect("Failed to execute gcc for libmp3lame.dll");
            if !status.success() {
                panic!("gcc failed to create libmp3lame.dll");
            }

            println!("cargo:rustc-link-search=native={}", out_dir.display());
            println!("cargo:rustc-link-lib=dylib=mp3lame");
        }
    } else {
        // Unix / Linux: build libmp3lame.so for dev and tests
        let objs = build.compile_intermediates();
        let so_out = out_dir.join("libmp3lame.so");

        let compiler = build.get_compiler();
        let mut cmd = compiler.to_command();
        cmd.arg("-shared")
            .arg("-o").arg(&so_out);

        for obj in &objs {
            cmd.arg(obj);
        }
        cmd.arg("-lm");

        let status = cmd.status().expect("Failed to link libmp3lame.so");
        if !status.success() {
            panic!("Linker failed to create libmp3lame.so");
        }

        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=dylib=mp3lame");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", out_dir.display());
    }
}
