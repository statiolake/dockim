use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=forwarder/main.go");
    println!("cargo:rerun-if-env-changed=DOCKIM_GO");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source = manifest_dir.join("forwarder").join("main.go");
    let go = env::var_os("DOCKIM_GO").unwrap_or_else(|| "go".into());

    let targets = [("amd64", "amd64"), ("arm64", "arm64")];

    for (name, goarch) in targets {
        let output = out_dir.join(format!("dockim-forwarder-linux-{name}"));
        let mut command = Command::new(&go);
        command
            .current_dir(&manifest_dir)
            .env("GOOS", "linux")
            .env("GOARCH", goarch)
            .env("CGO_ENABLED", "0")
            .args(["build", "-trimpath", "-ldflags=-s -w", "-o"])
            .arg(&output)
            .arg(&source);

        let status = command.status().unwrap_or_else(|error| {
            panic!(
                "failed to run Go compiler {:?}: {error}; install Go or set DOCKIM_GO",
                go
            )
        });
        if !status.success() {
            panic!("Go compiler failed while building Linux {name} port forwarder");
        }
    }

    // Make the generated artifacts visible in build diagnostics without copying them into the
    // source tree. The Rust module below includes the files directly from OUT_DIR.
    for name in ["amd64", "arm64"] {
        let output = out_dir.join(format!("dockim-forwarder-linux-{name}"));
        let metadata = fs::metadata(&output).unwrap_or_else(|error| {
            panic!("Go compiler did not produce {}: {error}", output.display())
        });
        assert!(
            metadata.len() > 0,
            "Go compiler produced an empty forwarder"
        );
    }
}
