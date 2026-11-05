use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/icon.ico")
        .canonicalize()
        .unwrap();
    let source = out.join("icon.rc");
    fs::write(
        &source,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let resource = out.join("icon.res");
    let compiler = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let kits =
            PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap()).join("Windows Kits/10/bin");
        let mut candidates: Vec<_> = fs::read_dir(kits)
            .expect("Install the Windows SDK or set RC")
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("x64/rc.exe"))
            .filter(|path| path.is_file())
            .collect();
        candidates.sort();
        candidates
            .pop()
            .expect("Windows SDK rc.exe not found; set RC")
    });
    assert!(Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(source)
        .status()
        .expect("Run Windows resource compiler")
        .success());
    println!(
        "cargo:rustc-link-arg-bin=CameraBridge={}",
        resource.display()
    );
}
