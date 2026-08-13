#![allow(clippy::uninlined_format_args)]

extern crate bindgen;

use cmake::Config;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap();
    // Link C++ standard library
    if let Some(cpp_stdlib) = get_cpp_link_stdlib(&target) {
        println!("cargo:rustc-link-lib=dylib={}", cpp_stdlib);
    }
    // Link macOS Accelerate framework for matrix calculations
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=framework=Accelerate");
        #[cfg(feature = "coreml")]
        {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=CoreML");
        }
        #[cfg(feature = "metal")]
        {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=Metal");
            println!("cargo:rustc-link-lib=framework=MetalKit");
        }
    }

    #[cfg(feature = "coreml")]
    println!("cargo:rustc-link-lib=static=whisper.coreml");

    #[cfg(feature = "openblas")]
    {
        if let Ok(openblas_path) = env::var("OPENBLAS_PATH") {
            println!(
                "cargo::rustc-link-search={}",
                PathBuf::from(openblas_path).join("lib").display()
            );
        }
        if cfg!(windows) {
            println!("cargo:rustc-link-lib=libopenblas");
        } else {
            println!("cargo:rustc-link-lib=openblas");
        }
    }
    #[cfg(feature = "cuda")]
    {
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublasLt");
        println!("cargo:rustc-link-lib=cuda");
        cfg_if::cfg_if! {
            if #[cfg(target_os = "windows")] {
                let cuda_path = PathBuf::from(env::var("CUDA_PATH").unwrap()).join("lib/x64");
                println!("cargo:rustc-link-search={}", cuda_path.display());
            } else {
                println!("cargo:rustc-link-lib=culibos");
                println!("cargo:rustc-link-search=/usr/local/cuda/lib64");
                println!("cargo:rustc-link-search=/usr/local/cuda/lib64/stubs");
                println!("cargo:rustc-link-search=/opt/cuda/lib64");
                println!("cargo:rustc-link-search=/opt/cuda/lib64/stubs");
            }
        }
    }
    // cuda feature ではなく自動検出で CUDA を有効にした場合も同じライブラリをリンクする
    if cfg!(not(feature = "cuda")) && cfg!(target_os = "windows") && cuda_toolkit_available() {
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublasLt");
        println!("cargo:rustc-link-lib=cuda");
        let cuda_path = PathBuf::from(env::var("CUDA_PATH").unwrap()).join("lib/x64");
        println!("cargo:rustc-link-search={}", cuda_path.display());
    }
    #[cfg(feature = "hipblas")]
    {
        println!("cargo:rustc-link-lib=hipblas");
        println!("cargo:rustc-link-lib=rocblas");
        println!("cargo:rustc-link-lib=amdhip64");

        cfg_if::cfg_if! {
            if #[cfg(target_os = "windows")] {
                panic!("Due to a problem with the last revision of the ROCm 5.7 library, it is not possible to compile the library for the windows environment.\nSee https://github.com/ggerganov/whisper.cpp/issues/2202 for more details.")
            } else {
                println!("cargo:rerun-if-env-changed=HIP_PATH");

                let hip_path = match env::var("HIP_PATH") {
                    Ok(path) =>PathBuf::from(path),
                    Err(_) => PathBuf::from("/opt/rocm"),
                };
                let hip_lib_path = hip_path.join("lib");

                println!("cargo:rustc-link-search={}",hip_lib_path.display());
            }
        }
    }

    #[cfg(feature = "openmp")]
    {
        if target.contains("gnu") {
            println!("cargo:rustc-link-lib=gomp");
        } else if target.contains("apple") {
            println!("cargo:rustc-link-lib=omp");
            println!("cargo:rustc-link-search=/opt/homebrew/opt/libomp/lib");
        }
    }

    println!("cargo:rerun-if-changed=wrapper.h");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let whisper_root = out.join("whisper.cpp/");

    if !whisper_root.join("CMakeLists.txt").exists() {
        if whisper_root.exists() {
            std::fs::remove_dir_all(&whisper_root).unwrap();
        }
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let local_src = manifest_dir.join("whisper.cpp");
        let src = if local_src.exists() {
            local_src
        } else {
            // Patched crate: find whisper.cpp in the original registry source
            let registry_src = manifest_dir
                .parent().unwrap() // patches/
                .parent().unwrap() // workspace root
                .join("target/release/build");
            // Fall back to downloading via the original crate's bundled source
            // by looking in the cargo registry cache
            let home = PathBuf::from(env::var("CARGO_HOME").unwrap_or_else(|_| {
                let h = env::var("HOME").or_else(|_| env::var("USERPROFILE")).unwrap();
                format!("{}/.cargo", h)
            }));
            let mut found = None;
            if let Ok(entries) = std::fs::read_dir(home.join("registry/src")) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("whisper-rs-sys-0.13.1/whisper.cpp");
                    if candidate.exists() {
                        found = Some(candidate);
                        break;
                    }
                }
            }
            found.expect("Could not find whisper.cpp sources in cargo registry")
        };
        fs_extra::dir::copy(&src, &out, &Default::default()).unwrap_or_else(|e| {
            panic!(
                "Failed to copy whisper sources into {}: {}",
                whisper_root.display(),
                e
            )
        });
    }

    if env::var("WHISPER_DONT_GENERATE_BINDINGS").is_ok() || target.contains("msvc") {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let _: u64 = std::fs::copy(manifest_dir.join("src/bindings.rs"), out.join("bindings.rs"))
            .expect("Failed to copy bindings.rs");
    } else {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let mut bindings = bindgen::Builder::default()
            .header(manifest_dir.join("wrapper.h").to_str().unwrap());

        #[cfg(feature = "metal")]
        {
            bindings = bindings.header("whisper.cpp/ggml/include/ggml-metal.h");
        }
        #[cfg(feature = "vulkan")]
        {
            bindings = bindings
                .header("whisper.cpp/ggml/include/ggml-vulkan.h")
                .clang_arg("-DGGML_USE_VULKAN=1");
        }

        if target.contains("msvc") {
            if let Ok(include) = env::var("INCLUDE") {
                for path in include.split(';').filter(|p| !p.is_empty()) {
                    bindings = bindings.clang_arg(format!("-isystem{}", path));
                }
            }
        }

        let bindings = bindings
            .clang_arg(format!("-I{}", whisper_root.display()))
            .clang_arg(format!("-I{}", whisper_root.join("include").display()))
            .clang_arg(format!("-I{}", whisper_root.join("ggml/include").display()))
            .layout_tests(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate();

        match bindings {
            Ok(b) => {
                let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
                b.write_to_file(out_path.join("bindings.rs"))
                    .expect("Couldn't write bindings!");
            }
            Err(e) => {
                println!("cargo:warning=Unable to generate bindings: {}", e);
                println!("cargo:warning=Using bundled bindings.rs, which may be out of date");
                // copy src/bindings.rs to OUT_DIR
                std::fs::copy("src/bindings.rs", out.join("bindings.rs"))
                    .expect("Unable to copy bindings.rs");
            }
        }
    };

    // stop if we're on docs.rs
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let mut config = Config::new(&whisper_root);

    config
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_ALL_WARNINGS", "OFF")
        .define("WHISPER_ALL_WARNINGS_3RD_PARTY", "OFF")
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .very_verbose(true)
        .pic(true);

    if cfg!(target_os = "windows") {
        config.cxxflag("/utf-8");
        // sherpa-rs-sys(static) と Rust の +crt-static が /MT なので whisper 側も揃える。
        // 混在すると LNK2038 RuntimeLibrary mismatch でリンクできない。
        config.define("CMAKE_POLICY_DEFAULT_CMP0091", "NEW");
        config.define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded");
        println!("cargo:rustc-link-lib=advapi32");
    }

    if cfg!(feature = "coreml") {
        config.define("WHISPER_COREML", "ON");
        config.define("WHISPER_COREML_ALLOW_FALLBACK", "1");
    }

    if cfg!(feature = "cuda") {
        config.define("GGML_CUDA", "ON");
        config.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
        config.define("CMAKE_CUDA_FLAGS", "-Xcompiler=-fPIC");
    } else if cfg!(target_os = "windows") && cuda_toolkit_available() {
        println!("cargo:warning=CUDA Toolkit detected; building whisper with the CUDA backend");
        config.define("GGML_CUDA", "ON");
        // -allow-unsupported-compiler: CUDA Toolkit の対応表より新しい MSVC が入っていることが多く、nvcc が弾くため
        // /Zc:preprocessor: CUDA 13 系の CCCL ヘッダが準拠プリプロセッサを要求する
        config.define(
            "CMAKE_CUDA_FLAGS",
            "-allow-unsupported-compiler -Xcompiler=/Zc:preprocessor",
        );
    }

    if cfg!(feature = "hipblas") {
        config.define("GGML_HIP", "ON");
        config.define("CMAKE_C_COMPILER", "hipcc");
        config.define("CMAKE_CXX_COMPILER", "hipcc");
        println!("cargo:rerun-if-env-changed=AMDGPU_TARGETS");
        if let Ok(gpu_targets) = env::var("AMDGPU_TARGETS") {
            config.define("AMDGPU_TARGETS", gpu_targets);
        }
    }

    if cfg!(feature = "vulkan") {
        config.define("GGML_VULKAN", "ON");
        if cfg!(windows) {
            println!("cargo:rerun-if-env-changed=VULKAN_SDK");
            println!("cargo:rustc-link-lib=vulkan-1");
            let vulkan_path = match env::var("VULKAN_SDK") {
                Ok(path) => PathBuf::from(path),
                Err(_) => panic!(
                    "Please install Vulkan SDK and ensure that VULKAN_SDK env variable is set"
                ),
            };
            let vulkan_lib_path = vulkan_path.join("Lib");
            println!("cargo:rustc-link-search={}", vulkan_lib_path.display());
        } else if cfg!(target_os = "macos") {
            println!("cargo:rerun-if-env-changed=VULKAN_SDK");
            println!("cargo:rustc-link-lib=vulkan");
            let vulkan_path = match env::var("VULKAN_SDK") {
                Ok(path) => PathBuf::from(path),
                Err(_) => panic!(
                    "Please install Vulkan SDK and ensure that VULKAN_SDK env variable is set"
                ),
            };
            let vulkan_lib_path = vulkan_path.join("lib");
            println!("cargo:rustc-link-search={}", vulkan_lib_path.display());
        } else {
            println!("cargo:rustc-link-lib=vulkan");
        }
    }

    if cfg!(feature = "openblas") {
        config.define("GGML_BLAS", "ON");
        config.define("GGML_BLAS_VENDOR", "OpenBLAS");
        if env::var("BLAS_INCLUDE_DIRS").is_err() {
            panic!("BLAS_INCLUDE_DIRS environment variable must be set when using OpenBLAS");
        }
        config.define("BLAS_INCLUDE_DIRS", env::var("BLAS_INCLUDE_DIRS").unwrap());
        println!("cargo:rerun-if-env-changed=BLAS_INCLUDE_DIRS");
    }

    if cfg!(feature = "metal") {
        config.define("GGML_METAL", "ON");
        config.define("GGML_METAL_NDEBUG", "ON");
        config.define("GGML_METAL_EMBED_LIBRARY", "ON");
    } else {
        // Metal is enabled by default, so we need to explicitly disable it
        config.define("GGML_METAL", "OFF");
    }

    if cfg!(debug_assertions) || cfg!(feature = "force-debug") {
        // debug builds are too slow to even remotely be usable,
        // so we build with optimizations even in debug mode
        config.define("CMAKE_BUILD_TYPE", "RelWithDebInfo");
        config.cxxflag("-DWHISPER_DEBUG");
    }

    // Allow passing any WHISPER or CMAKE compile flags
    for (key, value) in env::vars() {
        let is_whisper_flag =
            key.starts_with("WHISPER_") && key != "WHISPER_DONT_GENERATE_BINDINGS";
        let is_cmake_flag = key.starts_with("CMAKE_");
        if is_whisper_flag || is_cmake_flag {
            config.define(&key, &value);
        }
    }

    if cfg!(not(feature = "openmp")) {
        config.define("GGML_OPENMP", "OFF");
    }

    if cfg!(feature = "intel-sycl") {
        config.define("BUILD_SHARED_LIBS", "ON");
        config.define("GGML_SYCL", "ON");
        config.define("GGML_SYCL_TARGET", "INTEL");
        config.define("CMAKE_C_COMPILER", "icx");
        config.define("CMAKE_CXX_COMPILER", "icpx");
    }

    // Stale CMakeCache.txt causes cmake to re-configure when the compiler path
    // changes, and the re-configure loses -D flags (WHISPER_BUILD_EXAMPLES=OFF
    // reverts to ON, then add_subdirectory(examples) fails on the missing dir).
    let _ = std::fs::remove_file(out.join("build/CMakeCache.txt"));

    let destination = config.build();

    add_link_search_path(&out.join("build")).unwrap();

    println!("cargo:rustc-link-search=native={}", destination.display());
    if cfg!(feature = "intel-sycl") {
        println!("cargo:rustc-link-lib=whisper");
        println!("cargo:rustc-link-lib=ggml");
        println!("cargo:rustc-link-lib=ggml-base");
        println!("cargo:rustc-link-lib=ggml-cpu");
    } else {
        println!("cargo:rustc-link-lib=static=whisper");
        println!("cargo:rustc-link-lib=static=ggml");
        println!("cargo:rustc-link-lib=static=ggml-base");
        println!("cargo:rustc-link-lib=static=ggml-cpu");
    }
    if cfg!(target_os = "macos") || cfg!(feature = "openblas") {
        println!("cargo:rustc-link-lib=static=ggml-blas");
    }
    if cfg!(feature = "vulkan") {
        if cfg!(feature = "intel-sycl") {
            println!("cargo:rustc-link-lib=ggml-vulkan");
        } else {
            println!("cargo:rustc-link-lib=static=ggml-vulkan");
        }
    }

    if cfg!(feature = "hipblas") {
        println!("cargo:rustc-link-lib=static=ggml-hip");
    }

    if cfg!(feature = "metal") {
        println!("cargo:rustc-link-lib=static=ggml-metal");
    }

    if cfg!(feature = "cuda")
        || (cfg!(target_os = "windows") && cuda_toolkit_available())
    {
        println!("cargo:rustc-link-lib=static=ggml-cuda");
    }

    if cfg!(feature = "openblas") {
        println!("cargo:rustc-link-lib=static=ggml-blas");
    }

    if cfg!(feature = "intel-sycl") {
        println!("cargo:rustc-link-lib=ggml-sycl");
    }

    println!(
        "cargo:WHISPER_CPP_VERSION={}",
        get_whisper_cpp_version(&whisper_root)
            .expect("Failed to read whisper.cpp CMake config")
            .expect("Could not find whisper.cpp version declaration"),
    );

    // for whatever reason this file is generated during build and triggers cargo complaining
    _ = std::fs::remove_file("bindings/javascript/package.json");
}

/// CUDA Toolkit が使える状態なら GPU バックエンドを自動で有効にする。
/// CPU専用のバイナリが必要なとき（NVIDIA以外の環境へ配布するビルドなど）は
/// FENNEC_NO_CUDA=1 で抑止する。
///
/// 検出しただけでビルドが壊れないよう、実際にコンパイルが通る条件が揃ったときだけ true を返す。
fn cuda_toolkit_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        println!("cargo:rerun-if-env-changed=FENNEC_NO_CUDA");
        println!("cargo:rerun-if-env-changed=CUDA_PATH");
        if env::var_os("FENNEC_NO_CUDA").is_some() {
            return false;
        }
        let Some(root) = env::var_os("CUDA_PATH").map(PathBuf::from) else {
            return false;
        };
        if !root.join("bin").join("nvcc.exe").exists() {
            return false;
        }
        // MSVC 14.4x の STL は CUDA 12.4 以降でないと yvals_core.h の static_assert (STL1002) で落ちる
        if !cuda_version_at_least(&root, 12, 4) {
            println!("cargo:warning=CUDA Toolkit is older than 12.4; building whisper for CPU");
            return false;
        }
        // CUDA より後に Visual Studio を入れると MSBuild 統合が無く "No CUDA toolset found" になる
        if !vs_cuda_integration_installed() {
            println!(
                "cargo:warning=CUDA Visual Studio integration not found; building whisper for CPU"
            );
            return false;
        }
        true
    })
}

/// `.../CUDA/v12.6` のようなディレクトリ名からバージョンを読む
fn cuda_version_at_least(root: &PathBuf, major: u32, minor: u32) -> bool {
    let Some(name) = root.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some((v_major, v_minor)) = name.trim_start_matches('v').split_once('.') else {
        return false;
    };
    match (v_major.parse::<u32>(), v_minor.parse::<u32>()) {
        (Ok(a), Ok(b)) => (a, b) >= (major, minor),
        _ => false,
    }
}

fn vs_cuda_integration_installed() -> bool {
    let roots = [
        env::var_os("ProgramFiles"),
        env::var_os("ProgramFiles(x86)"),
    ];
    for root in roots.into_iter().flatten() {
        let vs = PathBuf::from(root).join("Microsoft Visual Studio").join("2022");
        let Ok(editions) = std::fs::read_dir(&vs) else {
            continue;
        };
        for edition in editions.flatten() {
            let customizations = edition
                .path()
                .join("MSBuild/Microsoft/VC/v170/BuildCustomizations");
            let Ok(entries) = std::fs::read_dir(customizations) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("CUDA ") && name.ends_with(".props") {
                    return true;
                }
            }
        }
    }
    false
}

// From https://github.com/alexcrichton/cc-rs/blob/fba7feded71ee4f63cfe885673ead6d7b4f2f454/src/lib.rs#L2462
fn get_cpp_link_stdlib(target: &str) -> Option<&'static str> {
    if target.contains("msvc") {
        None
    } else if target.contains("apple") || target.contains("freebsd") || target.contains("openbsd") {
        Some("c++")
    } else if target.contains("android") {
        Some("c++_shared")
    } else {
        Some("stdc++")
    }
}

fn add_link_search_path(dir: &std::path::Path) -> std::io::Result<()> {
    if dir.is_dir() {
        println!("cargo:rustc-link-search={}", dir.display());
        for entry in std::fs::read_dir(dir)? {
            add_link_search_path(&entry?.path())?;
        }
    }
    Ok(())
}

fn get_whisper_cpp_version(whisper_root: &std::path::Path) -> std::io::Result<Option<String>> {
    let cmake_lists = BufReader::new(File::open(whisper_root.join("CMakeLists.txt"))?);

    for line in cmake_lists.lines() {
        let line = line?;

        if let Some(suffix) = line.strip_prefix(r#"project("whisper.cpp" VERSION "#) {
            let whisper_cpp_version = suffix.trim_end_matches(')');
            return Ok(Some(whisper_cpp_version.into()));
        }
    }

    Ok(None)
}
