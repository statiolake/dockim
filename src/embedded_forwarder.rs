pub const LINUX_AMD64: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/dockim-forwarder-linux-amd64"));
pub const LINUX_ARM64: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/dockim-forwarder-linux-arm64"));

pub fn for_machine(machine: &str) -> Option<(&'static str, &'static [u8])> {
    match machine.trim() {
        "x86_64" | "amd64" => Some(("amd64", LINUX_AMD64)),
        "aarch64" | "arm64" => Some(("arm64", LINUX_ARM64)),
        _ => None,
    }
}
