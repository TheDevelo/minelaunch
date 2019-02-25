extern crate reqwest;
extern crate tempfile;
extern crate flate2;
extern crate tar;
extern crate walkdir;
extern crate serde;
extern crate serde_json;

use flate2::read::GzDecoder;
use tar::Archive;
use tempfile::tempdir;
use reqwest::header;
use walkdir::WalkDir;
use std::path::Path;
use std::{fs, io};
use std::io::Write;
use serde::{Deserialize, Serialize};

// TODO: Move all these types to their own file where it won't clutter everything
// Types for version list JSON
#[derive(Serialize, Deserialize)]
struct MinecraftLatestVersions {
    release: String,
    snapshot: String,
}

#[derive(Serialize, Deserialize)]
struct MinecraftVersion {
    id: String,
    #[serde(rename="type")]
    version_type: String,
    url: String,
    time: String,
    #[serde(rename="releaseTime")]
    release_time: String,
}

#[derive(Serialize, Deserialize)]
struct MinecraftVersionList {
    latest: MinecraftLatestVersions,
    versions: Vec<MinecraftVersion>,
}

// Types for version spec JSON
// Honestly these types are pretty complex and I don't need them right now so I'll leave them blank
#[derive(Serialize, Deserialize)]
struct DynamicArgument {
}

#[derive(Serialize, Deserialize)]
enum Argument {
    Static(String),
    Dynamic(DynamicArgument),
}

#[derive(Serialize, Deserialize)]
struct VersionArguments {
    game: Vec<Argument>,
    jvm: Vec<Argument>,
}

#[derive(Serialize, Deserialize)]
struct VersionAssets {
}

#[derive(Serialize, Deserialize)]
struct Download {
    sha1: String,
    size: u32,
    url: String,
}

#[derive(Serialize, Deserialize)]
struct VersionDownloads {
    client: Download,
    // Server doesn't exist for versions before 1.2.5
    server: Option<Download>,
}

#[derive(Serialize, Deserialize)]
struct Library {
}

// TODO: Properly fill out the entire spec struct
#[derive(Serialize, Deserialize)]
struct VersionSpec {
    // Commented out because I have no clue how to make this work right now and it's not necessary
    // arguments: VersionArguments,
    #[serde(rename="assetIndex")]
    asset_index: VersionAssets,
    assets: String,
    downloads: VersionDownloads,
    id: String,
    libraries: Vec<Library>,
    #[serde(rename="mainClass")]
    main_class: String,
    #[serde(rename="minimumLauncherVersion")]
    minimum_launcher_version: u8,
}

fn main() {
    let minecraft_path = ".";

    // Check for java installation for the current platform
    if !Path::new(&format!("{0}/runtime/{1}-{2}/", minecraft_path, get_os(), get_arch())).exists() {
        println!("Java installation not found");
        download_java(minecraft_path);
    }

    // Get list of Minecraft versions
    let mut minecraft_versions_response = reqwest::get("https://launchermeta.mojang.com/mc/game/version_manifest.json").unwrap();
    let minecraft_versions: MinecraftVersionList = serde_json::from_str(&minecraft_versions_response.text().unwrap()).unwrap();

    // Display list of versions for user to pick
    println!("Available versions:");
    let version_list = minecraft_versions.versions.iter().filter(|v| v.version_type == "release").collect::<Vec<&MinecraftVersion>>();
    for (n, version) in version_list.iter().enumerate() {
        println!("{0}. {1}", n+1, version.id);
    }
    print!("Which version would you like to play (use the number in front): ");
    let mut version_select_str = String::new();
    let mut version_num = 0;
    while version_num == 0 {
        io::stdout().flush().unwrap();
        version_select_str.clear();
        io::stdin().read_line(&mut version_select_str).unwrap();
        match version_select_str.trim().parse::<usize>() {
            Ok(n) if n > 0 && n <= version_list.len() => version_num = n,
            Ok(_) => print!("That's not a listed version. Try again: "),
            Err(_) => print!("That's not a number. Try again: "),
        }
    }

    // Check for whether selected version is installed
    let version = version_list[version_num - 1];
    if !Path::new(&format!("{0}/versions/{1}/{1}.json", minecraft_path, version.id)).exists() {
        println!("Minecraft {0} not found", version.id);
        download_minecraft_version(minecraft_path, version);
    }

    // Check for necessary libraries
    // TODO later

    // Check for necessary assets
    // TODO later

    // Launch Minecraft
    // TODO later
    println!("Launching Minecraft {0}", version.id);
    launch_minecraft_version(minecraft_path, version);
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

    // Extract Java runtime to tempdir
    println!("Extracting Java");
    let mut archive = Archive::new(GzDecoder::new(response));
    let extract_dir = tempdir().unwrap();
    archive.unpack(extract_dir.path()).unwrap();

    // Move JRE directory to "{save-path}/runtime/{os}-{arch}/"
    let runtime_dir = format!("{0}/runtime/{1}-{2}/", save_path, get_os(), get_arch());
    // Create runtime folder if it doesn't exist
    if !Path::new(&format!("{0}/runtime/", save_path)).exists() {
        fs::create_dir_all(&format!("{0}/runtime/", save_path)).unwrap();
    }
    if get_os() == "windows" {
        // fs::rename doesn't work across drive letters, so I manually copy every file to move the folder
        // Don't need to worry about deleting the files because they're in a tempdir that gets automatically removed
        fs::create_dir(&runtime_dir).unwrap();
        for entry in WalkDir::new(extract_dir.path().join("jre1.8.0_201")).min_depth(1) {
            let entry = entry.unwrap();
            let unprefixed_entry = entry.path().strip_prefix(extract_dir.path().join("jre1.8.0_201")).unwrap();
            if entry.path().is_dir() {
                fs::create_dir(Path::new(&runtime_dir).join(unprefixed_entry)).unwrap();
            }
            else if entry.path().is_file() {
                fs::copy(entry.path(), Path::new(&runtime_dir).join(unprefixed_entry)).unwrap();
            }
        }
    }
    else if get_os() == "macos" {
        // Mac OS X has a weird JRE file structure compared to Windows/Linux
        fs::rename(extract_dir.path().join("jre1.8.0_201.jre/Contents/Home"), runtime_dir).unwrap();
    }
    else if get_os() == "linux" {
        fs::rename(extract_dir.path().join("jre1.8.0_201"), runtime_dir).unwrap();
    }
    println!("Java extracted to runtime/{0}-{1}/", get_os(), get_arch());
}

fn download_minecraft_version(minecraft_path: &str, version: &MinecraftVersion) {
    // Create version folder if it doesn't exist
    if !Path::new(&format!("{0}/versions/{1}/", minecraft_path, version.id)).exists() {
        fs::create_dir_all(&format!("{0}/versions/{1}", minecraft_path, version.id)).unwrap();
    }

    // Download Minecraft version spec
    println!("Downloading Minecraft version spec");
    let mut version_spec_response = reqwest::get(&version.url).unwrap();
    let version_spec_path = format!("{0}/versions/{1}/{1}.json", minecraft_path, version.id);
    let mut version_spec_file = fs::File::create(&version_spec_path).unwrap();
    // Copy text to string first so that I can use it again
    let version_spec_json = version_spec_response.text().unwrap();
    version_spec_file.write_all(version_spec_json.as_bytes()).unwrap();

    // Deserialize version spec
    let version_spec: VersionSpec = serde_json::from_str(&version_spec_json).unwrap();

    // Download Minecraft jar
    println!("Downloading Minecraft {0} jar", version.id);
    let mut minecraft_jar_response = reqwest::get(&version_spec.downloads.client.url).unwrap();
    let minecraft_jar_path = format!("{0}/versions/{1}/{1}.jar", minecraft_path, version.id);
    let mut minecraft_jar_file = fs::File::create(&minecraft_jar_path).unwrap();
    io::copy(&mut minecraft_jar_response, &mut minecraft_jar_file).unwrap();

    println!("Minecraft {0} downloaded", version.id);
}

fn launch_minecraft_version(minecraft_path: &str, version: &MinecraftVersion) {
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
