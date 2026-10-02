use std::env;
use std::path::Path;

use aya_build::{Package, Toolchain};
use ebpf as _;

const TOOLCHAIN: &str = "ASURA_EBPF_TOOLCHAIN";

fn main() -> aya_build::Result<()> {
    println!("cargo:rerun-if-env-changed={TOOLCHAIN}");

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ebpf")
        .canonicalize()?;

    let package = Package {
        name: "ebpf",
        root_dir: root.to_str().unwrap_or_default(),
        ..Default::default()
    };

    let pinned = env::var(TOOLCHAIN).ok();
    let toolchain = pinned
        .as_deref()
        .map_or(Toolchain::Nightly, Toolchain::Custom);

    aya_build::build_ebpf([package], toolchain)
}
