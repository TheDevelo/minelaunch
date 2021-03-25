use std::path::Path;
use sha1::Sha1;
use std::fs::File;
use std::io::Read;

pub fn check_file(file_path: &Path, sha1: &str, size: u64) -> bool {
    // Check if the file actually exists first
    if !file_path.exists() {
        return false;
    }

    // Check if the size matches
    let mut file = File::open(file_path).unwrap();
    if file.metadata().unwrap().len() != size {
        return false;
    }

    // Check if sha1 hash matches
    let mut file_content = Vec::new();
    file.read_to_end(&mut file_content).unwrap();
    if Sha1::from(file_content).hexdigest() != sha1 {
        return false;
    }

    return true;
}

pub fn get_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        panic!("unsupported operating system!");
    }
}

pub fn get_arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        panic!("unsupported architecture!");
    }
}

// Special get_os and get_arch wrapper functions that fit the java naming convention
pub fn get_os_java() -> &'static str {
    let os = get_os();
    if os == "macos" {
        "mac"
    }
    else {
        os
    }
}

pub fn get_arch_java() -> &'static str {
    let arch = get_arch();
    if arch == "x86" {
        "x32"
    }
    else {
        arch
    }
}

// Special get_os and get_arch wrapper functions that fit the minecraft naming convention
// I don't think get_arch needs a wrapper, but I haven't seen x64 specified anywhere in Minecraft
pub fn get_os_minecraft() -> &'static str {
    let os = get_os();
    if os == "macos" {
        "osx"
    }
    else {
        os
    }
}
