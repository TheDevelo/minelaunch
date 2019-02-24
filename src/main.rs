extern crate reqwest;
extern crate tempfile;
extern crate flate2;
extern crate tar;

use flate2::read::GzDecoder;
use tar::Archive;
use reqwest::header;
use std::path::Path;

fn main() {
    let minecraft_path = ".";

    // Check for java installation for the current platform
    if !Path::new(&format!("{0}/runtime/{1}-{2}/", minecraft_path, get_os(), get_arch())).exists() {
        println!("Java installation not found");
        download_java(minecraft_path);
    }
}

fn download_java(save_path: &str) {
    // Download Java runtime
    println!("Downloading Java for {0}-{1}", get_os(), get_arch());
    let client = reqwest::Client::builder()
        // Need custom redirect policy so that I can change the host header in between
        // Oracle gives a fit if I dont set HOST to exactly the same as the redirect url
        .redirect(reqwest::RedirectPolicy::custom(|attempt| {
            if attempt.previous().first().unwrap().host_str().unwrap().starts_with("edelivery") {
                attempt.stop()
            }
            else {
                attempt.follow()
            }
        }))
        .build()
        .unwrap();
    let java_url = format!("https://edelivery.oracle.com/otn-pub/java/jdk/8u201-b09/42970487e3af4f5aa5bca3f542482c60/jre-8u201-{0}-{1}.tar.gz", get_os_java(), get_arch_java());
    let mut response = client.get(&java_url)
        // Need to spoof host & cookies for java to download
        .header(header::COOKIE, "oraclelicense=accept-securebackup-cookie")
        .header(header::HOST, "edelivery.oracle.com")
        .send()
        .unwrap();
    // Manually requests the redirect but with a different HOST
    response = client.get(response.headers().get(header::LOCATION).unwrap().to_str().unwrap())
        .header(header::HOST, "download.oracle.com")
        .send()
        .unwrap();

    // Extract Java runtime to "{save-path}/runtime/{os}-{arch}/"
    println!("Extracting Java");
    let mut archive = Archive::new(GzDecoder::new(response));
    let extract_path = format!("{0}/runtime/{1}-{2}/", save_path, get_os(), get_arch());
    archive.unpack(extract_path).unwrap();
    println!("Java extracted to runtime/{0}-{1}/", get_os(), get_arch());
}

fn get_os() -> &'static str {
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

fn get_arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        panic!("unsupported architecture!");
    }
}

// Special get_os and get_arch wrapper functions that fit the java naming convention
fn get_os_java() -> &'static str {
    let os = get_os();
    if os == "macos" {
        "macosx"
    }
    else {
        os
    }
}

fn get_arch_java() -> &'static str {
    let arch = get_arch();
    if arch == "x86" {
        "i586"
    }
    else {
        arch
    }
}
