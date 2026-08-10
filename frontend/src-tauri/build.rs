#[path = "build/ffmpeg.rs"]
mod ffmpeg;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/ffmpeg.rs");

    // Ensure Silero VAD v6 ONNX model is present
    ensure_vad_model();

    // GPU Acceleration Detection and Build Guidance
    detect_and_report_gpu_capabilities();

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=Foundation");

        // Let the enhanced_macos crate handle its own Swift compilation
        // The swift-rs crate build will be handled in the enhanced_macos crate's build.rs
    }

    // Download and bundle FFmpeg binary at build-time
    ffmpeg::ensure_ffmpeg_binary();

    tauri_build::build()
}

/// Detects GPU acceleration capabilities and provides build guidance
fn detect_and_report_gpu_capabilities() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:warning=🚀 Building Meetily for: {}", target_os);

    match target_os.as_str() {
        "macos" => {
            println!("cargo:warning=✅ macOS: Metal GPU acceleration ENABLED by default");
            #[cfg(feature = "coreml")]
            println!("cargo:warning=✅ CoreML acceleration ENABLED");
        }
        "windows" => {
            if cfg!(feature = "cuda") {
                println!("cargo:warning=✅ Windows: CUDA GPU acceleration ENABLED");
            } else if cfg!(feature = "vulkan") {
                println!("cargo:warning=✅ Windows: Vulkan GPU acceleration ENABLED");
            } else if cfg!(feature = "openblas") {
                println!("cargo:warning=✅ Windows: OpenBLAS CPU optimization ENABLED");
            } else {
                println!("cargo:warning=⚠️  Windows: Using CPU-only mode (no GPU or BLAS acceleration)");
                println!("cargo:warning=💡 For NVIDIA GPU: cargo build --release --features cuda");
                println!("cargo:warning=💡 For AMD/Intel GPU: cargo build --release --features vulkan");
                println!("cargo:warning=💡 For CPU optimization: cargo build --release --features openblas");

                // Try to detect NVIDIA GPU
                if which::which("nvidia-smi").is_ok() {
                    println!("cargo:warning=🎯 NVIDIA GPU detected! Consider rebuilding with --features cuda");
                }
            }
        }
        "linux" => {
            if cfg!(feature = "cuda") {
                println!("cargo:warning=✅ Linux: CUDA GPU acceleration ENABLED");
            } else if cfg!(feature = "vulkan") {
                println!("cargo:warning=✅ Linux: Vulkan GPU acceleration ENABLED");
            } else if cfg!(feature = "hipblas") {
                println!("cargo:warning=✅ Linux: AMD ROCm (HIP) acceleration ENABLED");
            } else if cfg!(feature = "openblas") {
                println!("cargo:warning=✅ Linux: OpenBLAS CPU optimization ENABLED");
            } else {
                println!("cargo:warning=⚠️  Linux: Using CPU-only mode (no GPU or BLAS acceleration)");
                println!("cargo:warning=💡 For NVIDIA GPU: cargo build --release --features cuda");
                println!("cargo:warning=💡 For AMD GPU: cargo build --release --features hipblas");
                println!("cargo:warning=💡 For other GPUs: cargo build --release --features vulkan");
                println!("cargo:warning=💡 For CPU optimization: cargo build --release --features openblas");

                // Try to detect NVIDIA GPU
                if which::which("nvidia-smi").is_ok() {
                    println!("cargo:warning=🎯 NVIDIA GPU detected! Consider rebuilding with --features cuda");
                }

                // Try to detect AMD GPU
                if which::which("rocm-smi").is_ok() {
                    println!("cargo:warning=🎯 AMD GPU detected! Consider rebuilding with --features hipblas");
                }
            }
        }
        _ => {
            println!("cargo:warning=ℹ️  Unknown platform: {}", target_os);
        }
    }

    // Performance guidance
    if !cfg!(feature = "cuda") && !cfg!(feature = "vulkan") && !cfg!(feature = "hipblas") && !cfg!(feature = "openblas") && target_os != "macos" {
        println!("cargo:warning=📊 Performance: CPU-only builds are significantly slower than GPU/BLAS builds");
        println!("cargo:warning=📚 See README.md for GPU/BLAS setup instructions");
    }
}

/// Ensure the Silero VAD v6 ONNX model file exists.
/// Extracts from the silero-vad Python package via `uv`, or downloads from GitHub.
fn ensure_vad_model() {
    let out_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let model_dir = std::path::Path::new(&out_dir).join("models");
    let model_path = model_dir.join("silero_vad_v6.onnx");

    if model_path.exists() {
        let size = std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0);
        println!("cargo:warning=✅ VAD model found: {} ({} bytes)", model_path.display(), size);
        return;
    }

    println!("cargo:warning=⬇️ VAD model not found, attempting download...");

    // Try extracting via uv from silero-vad Python package
    if which::which("uv").is_ok() {
        println!("cargo:warning=🔍 uv found, extracting model from silero-vad==6.2...");
        std::fs::create_dir_all(&model_dir).ok();

        let script = format!(
            r#"import importlib.resources as r, shutil, pathlib
src = r.files('silero_vad.data').joinpath('silero_vad.onnx')
dst = pathlib.Path(r'{model}')
dst.parent.mkdir(parents=True, exist_ok=True)
shutil.copy2(str(src), str(dst))
print(f"Model extracted to {{dst}}")
"#,
            model = model_path.display()
        );

        let tmp_script = std::env::temp_dir().join("extract_vad_model.py");
        std::fs::write(&tmp_script, &script).ok();

        let status = std::process::Command::new("uv")
            .args(["run", "--with", "silero-vad==6.2", "--python", "3.12", "python", &tmp_script.to_string_lossy()])
            .status();

        match status {
            Ok(s) if s.success() && model_path.exists() => {
                let size = std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0);
                println!("cargo:warning=✅ VAD model extracted via uv: {} bytes", size);
                std::fs::remove_file(&tmp_script).ok();
                return;
            }
            _ => {
                println!("cargo:warning=⚠️ uv extraction failed, trying direct download...");
                std::fs::remove_file(&tmp_script).ok();
            }
        }
    } else {
        println!("cargo:warning=⚠️ uv not found, trying direct download...");
    }

    // Fallback: download from GitHub releases
    let url = "https://github.com/snakers4/silero-vad/releases/download/v6.0/silero_vad.onnx";
    println!("cargo:warning=⬇️ Downloading VAD model from {}...", url);

    match reqwest::blocking::get(url) {
        Ok(resp) if resp.status().is_success() => {
            std::fs::create_dir_all(&model_dir).ok();
            let bytes = resp.bytes().unwrap_or_default();
            if !bytes.is_empty() {
                std::fs::write(&model_path, &bytes).ok();
                let size = bytes.len();
                println!("cargo:warning=✅ VAD model downloaded: {} bytes", size);
                return;
            }
        }
        _ => {}
    }

    // Final fallback: extract from silero_rs git checkout if still present
    let old_model = std::path::Path::new(&out_dir)
        .join("..")
        .join("..")
        .join("..")
        .join(".cargo")
        .join("git")
        .join("checkouts")
        .join("silero-rs-16a8cd672fe824c4")
        .join("26a6460")
        .join("models")
        .join("silero_vad.onnx");

    if old_model.exists() {
        println!("cargo:warning=⚠️ Copying old VAD model as placeholder (v4, not v6 - quality will be lower)");
        std::fs::create_dir_all(&model_dir).ok();
        std::fs::copy(&old_model, &model_path).ok();
        return;
    }

    println!("cargo:warning=❌ VAD model download failed!");
    println!("cargo:warning=💡 Run: uv run --with silero-vad==6.2 --python 3.12 python -c \"import importlib.resources as r, shutil; src = r.files('silero_vad.data').joinpath('silero_vad.onnx'); shutil.copy2(str(src), '{}')\"",
             model_path.display());
}
