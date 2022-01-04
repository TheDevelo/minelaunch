use flate2::read::GzDecoder;
use tar::Archive;
use zip::read::ZipArchive;
use tempfile::{tempfile, tempdir, TempDir};
use walkdir::WalkDir;
use std::path::Path;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::collections::BTreeMap;
use std::process::ExitStatus;
use std::sync::Arc;
use serde::Deserialize;
use async_std::process::Command;
use async_std::sync::Mutex;
use bytes::Buf;
use futures::stream::{self, StreamExt};

use crate::env::Environment;
use crate::util::*;

// TODO: Move all these types to their own file where it won't clutter everything
// Types for version list JSON
#[derive(Deserialize)]
pub struct MinecraftLatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MinecraftVersion {
    pub id: String,
    #[serde(rename="type")]
    pub version_type: String,
    url: String,
    time: String,
    #[serde(rename="releaseTime")]
    release_time: String,
}

#[derive(Deserialize)]
pub struct MinecraftVersionList {
    pub latest: MinecraftLatestVersions,
    pub versions: Vec<MinecraftVersion>,
}

// Types for version spec JSON
#[derive(Deserialize)]
#[serde(untagged)]
enum SingleOrVec<T> {
    Single(T),
    Vector(Vec<T>),
}

#[derive(Deserialize)]
struct DynamicArgument {
    rules: Vec<Rule>,
    value: SingleOrVec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Argument {
    Static(String),
    Dynamic(DynamicArgument),
}

#[derive(Deserialize)]
struct VersionArguments {
    game: Vec<Argument>,
    jvm: Vec<Argument>,
}

#[derive(Deserialize)]
struct VersionAssets {
    id: String,
    sha1: String,
    size: u64,
    #[serde(rename="totalSize")]
    total_size: u64,
    url: String
}

#[derive(Deserialize)]
struct Download {
    path: Option<String>,
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Deserialize)]
struct VersionDownloads {
    client: Download,
    // Server doesn't exist for versions before 1.2.5
    server: Option<Download>,
    // Deobfuscation mappings don't exist for versions before 1.14.4
    client_mappings: Option<Download>,
    server_mappings: Option<Download>,
}

#[derive(Deserialize)]
struct LibraryDownloads {
    // Apparently in older versions some libraries might not have an artifact
    artifact: Option<Download>,
    // This doesn't have a fully specified layout because a classifier can be called anything
    classifiers: Option<BTreeMap<String, Download>>,
}

#[derive(Deserialize)]
struct LibraryNatives {
    linux: Option<String>,
    osx: Option<String>,
    windows: Option<String>,
}

#[derive(Deserialize)]
struct LibraryExtractOptions {
    exclude: Vec<String>,
}

#[derive(Deserialize)]
struct RuleOS {
    name: Option<String>,
    version: Option<String>,
    arch: Option<String>,
}

#[derive(Deserialize)]
struct Rule {
    action: String,
    os: Option<RuleOS>,
    features: Option<BTreeMap<String, bool>>,
}

#[derive(Deserialize)]
struct Library {
    downloads: LibraryDownloads,
    name: String,
    natives: Option<LibraryNatives>,
    extract: Option<LibraryExtractOptions>,
    rules: Option<Vec<Rule>>,
}

#[derive(Deserialize)]
struct JavaVersion {
    component: String,
    #[serde(rename="majorVersion")]
    major_version: u8,
}

// TODO: Properly fill out the entire spec struct
#[derive(Deserialize)]
struct VersionSpec {
    arguments: Option<VersionArguments>,
    #[serde(rename="assetIndex")]
    asset_index: VersionAssets,
    assets: String,
    downloads: VersionDownloads,
    id: String,
    #[serde(rename="javaVersion")]
    java_version: Option<JavaVersion>,
    libraries: Vec<Library>,
    #[serde(rename="mainClass")]
    main_class: String,
    #[serde(rename="minecraftArguments")]
    minecraft_arguments: Option<String>,
    #[serde(rename="minimumLauncherVersion")]
    minimum_launcher_version: u8,
    #[serde(rename="type")]
    version_type: String,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

#[derive(Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, AssetObject>,
    #[serde(rename="virtual")]
    virtual_assets: Option<bool>,
    map_to_resources: Option<bool>,
}


async fn download_java(save_path: &str, version: u8) {
    // Download Java runtime
    // Need to download JRE for Java 8, JDK for Java 16+ and then jlink
    println!("Downloading Java {0} for {1}-{2}", version, get_os(), get_arch());
    let java_url;
    if version == 8 {
        java_url = format!("https://api.adoptium.net/v3/binary/latest/8/ga/{0}/{1}/jre/hotspot/normal/eclipse", get_os_java(), get_arch_java());
    }
    else {
        java_url = format!("https://api.adoptium.net/v3/binary/latest/{0}/ga/{1}/{2}/jdk/hotspot/normal/eclipse", version, get_os_java(), get_arch_java());
    }
    let response = reqwest::get(&java_url).await.unwrap();

    // Extract Java runtime to tempdir
    println!("Extracting Java");
    let extract_dir = tempdir().unwrap();
    if get_os() == "windows" {
        let mut temp_file = tempfile().unwrap();
        temp_file.write_all(&response.bytes().await.unwrap()).unwrap();
        let mut archive = ZipArchive::new(temp_file).unwrap();
        archive.extract(extract_dir.path()).unwrap();
    }
    else {
        let mut archive = Archive::new(GzDecoder::new(response.bytes().await.unwrap().reader()));
        archive.unpack(extract_dir.path()).unwrap();
    }
    let version_folder = fs::read_dir(&extract_dir).unwrap().next().unwrap().unwrap().path();

    // Move/Make JRE to "{save-path}/runtime/java{version}-{os}-{arch}/"
    let runtime_dir = format!("{0}/runtime/java{1}-{2}-{3}/", save_path, version, get_os(), get_arch());
    // Create runtime folder if it doesn't exist
    if !Path::new(&format!("{0}/runtime/", save_path)).exists() {
        fs::create_dir_all(&format!("{0}/runtime/", save_path)).unwrap();
    }
    // Need to move JRE for Java 8
    if version == 8 {
        println!("Moving JRE to runtime folder");
        if get_os() == "windows" {
            // fs::rename doesn't work across drive letters, so I manually copy every file to move the folder
            // Don't need to worry about deleting the files because they're in a tempdir that gets automatically removed
            fs::create_dir(&runtime_dir).unwrap();
            for entry in WalkDir::new(&version_folder).min_depth(1) {
                let entry = entry.unwrap();
                let unprefixed_entry = entry.path().strip_prefix(&version_folder).unwrap();
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
            fs::rename(version_folder.join("Contents/Home"), &runtime_dir).unwrap();
            fs::rename(version_folder.join("Contents/MacOS/libjli.dylib"), Path::new(&runtime_dir).join("bin/libjli.dylib")).unwrap();
        }
        else if get_os() == "linux" {
            fs::rename(version_folder, &runtime_dir).unwrap();
        }
    }
    // Need to jlink the JDK to create the JRE for Java 16+
    else {
        println!("Creating JRE using jlink");
        let jlink_path;
        if get_os() == "macos" {
            // Mac OS X has a weird JRE file structure compared to Windows/Linux
            jlink_path = version_folder.join("Contents/Home/bin/jlink")
        }
        else {
            jlink_path = version_folder.join("bin/jlink")
        }
        let mut jlink_process = Command::new(jlink_path);
        jlink_process.args(vec!["--add-modules", "ALL-MODULE-PATH", "--output", &runtime_dir,
                                "--strip-debug", "--no-man-pages", "--no-header-files", "--compress=2"]);
        let status = jlink_process.status().await.unwrap();
        println!("jlink exited with {0}", status);
    }

    println!("Java extracted to runtime/java{0}-{1}-{2}/", version, get_os(), get_arch());
}

pub async fn launch_minecraft_version(minecraft_path: String, version: MinecraftVersion, env: Box<Environment>) -> ExitStatus {
    let mut env = *env;

    // Get the version spec for the specified version
    // Downloads minecraft if that version doesn't exist
    let version_spec = get_version_spec(&minecraft_path, &version).await;

    env.set("version_name", &version_spec.id);
    env.set("version_type", &version_spec.version_type);
    let assets_root = format!("{0}/assets/", minecraft_path);
    env.set("assets_root", &assets_root);
    env.set("assets_index_name", &version_spec.assets);
    let game_assets = format!("{0}/assets/virtual/{1}/", minecraft_path, &version_spec.assets);
    env.set("game_assets", &game_assets);

    let java_version;
    if let Some(v) = &version_spec.java_version {
        java_version = v.major_version;
    }
    else {
        java_version = 8;
    }

    // Check for java installation for the current platform
    if !Path::new(&format!("{0}/runtime/java{1}-{2}-{3}/", minecraft_path, java_version, get_os(), get_arch())).exists() {
        println!("Java installation not found");
        download_java(&minecraft_path, java_version).await;
    }

    // Check for necessary libraries
    check_minecraft_libraries(&minecraft_path, &version_spec).await;

    // Check for necessary assets
    check_minecraft_assets(&minecraft_path, &version_spec).await;

    // Construct Launch Arguments
    let natives_dir = tempdir().unwrap();
    let launch_args = construct_launch_args(&minecraft_path, &version_spec, &mut env, &natives_dir);

    // Run Minecraft
    println!("Launching Minecraft {0}", version.id);
    let mut java_process = Command::new(format!("{0}/runtime/java{1}-{2}-{3}/bin/java", minecraft_path, java_version, get_os(), get_arch()));
    java_process.args(launch_args);
    let status = java_process.status().await.unwrap();
    println!("Minecraft exited with {0}", status);
    return status;
}

async fn get_version_spec(minecraft_path: &str, version: &MinecraftVersion) -> VersionSpec {
    // Check if the minecraft version is actually downloaded
    let spec_path = format!("{0}/versions/{1}/{1}.json", minecraft_path, version.id);
    if Path::new(&spec_path).exists() {
        // TODO: Check sha1 of the spec file
        let mut spec_file = File::open(spec_path).unwrap();
        let mut spec_json = String::new();
        spec_file.read_to_string(&mut spec_json).unwrap();
        let spec: VersionSpec = serde_json::from_str(&spec_json).unwrap();

        // Check if the Minecraft jar is damaged
        let jar_path = format!("{0}/versions/{1}/{1}.jar", minecraft_path, version.id);
        let jar_path = Path::new(&jar_path);
        if !check_file(jar_path, &spec.downloads.client.sha1, spec.downloads.client.size) {
            println!("Minecraft {0} jar damaged, downloading", version.id);
            download_minecraft_jar(minecraft_path, &spec).await;
            println!("Minecraft {0} jar downloaded", version.id);
        }

        return spec;
    }
    else {
        println!("Minecraft {0} spec not found", version.id);
        return download_minecraft_version(minecraft_path, version).await;
    }
}

async fn download_minecraft_version(minecraft_path: &str, version: &MinecraftVersion) -> VersionSpec {
    // Create version folder if it doesn't exist
    if !Path::new(&format!("{0}/versions/{1}/", minecraft_path, version.id)).exists() {
        fs::create_dir_all(&format!("{0}/versions/{1}", minecraft_path, version.id)).unwrap();
    }

    // Download Minecraft version spec
    println!("Downloading Minecraft version spec");
    let version_spec_response = reqwest::get(&version.url).await.unwrap();
    let version_spec_path = format!("{0}/versions/{1}/{1}.json", minecraft_path, version.id);
    let mut version_spec_file = File::create(&version_spec_path).unwrap();
    // Copy text to string first so that I can use it again
    let version_spec_json = version_spec_response.text().await.unwrap();
    version_spec_file.write_all(version_spec_json.as_bytes()).unwrap();

    // Deserialize version spec
    let version_spec: VersionSpec = serde_json::from_str(&version_spec_json).unwrap();

    // Download Minecraft jar
    println!("Downloading Minecraft {0} jar", version.id);
    download_minecraft_jar(minecraft_path, &version_spec).await;
    println!("Minecraft {0} jar downloaded", version.id);

    // Pass on the version spec
    return version_spec;
}

async fn download_minecraft_jar(minecraft_path: &str, version: &VersionSpec) {
    let minecraft_jar_response = reqwest::get(&version.downloads.client.url).await.unwrap();
    let minecraft_jar_path = format!("{0}/versions/{1}/{1}.jar", minecraft_path, version.id);
    let mut minecraft_jar_file = File::create(&minecraft_jar_path).unwrap();
    minecraft_jar_file.write_all(&minecraft_jar_response.bytes().await.unwrap()).unwrap();
}

async fn check_minecraft_libraries(minecraft_path: &str, version: &VersionSpec) {
    let mut downloaders_vec = Vec::new();
    for library in version.libraries.iter() {
        // Check if library rules are satisfied and skip if not
        if library.rules.is_some() && !spec_rules_satisfied(library.rules.as_ref().unwrap()) {
            continue;
        }

        // Check if the library has a general jar
        if library.downloads.artifact.is_some() {
            // Check if the library has been downloaded
            // Uses successive shadowing to please the borrow checker, plus it shows the successive building of the path
            // Need as_ref before unwrapping the option so as to not consume it
            let download_artifact = library.downloads.artifact.as_ref().unwrap();
            let jar_path = download_artifact.path.as_ref().unwrap();
            let jar_path_str = format!("{0}/libraries/{1}", minecraft_path, jar_path);
            let jar_path = Path::new(&jar_path_str);
            if check_file(jar_path, &download_artifact.sha1, download_artifact.size) {
                println!("Library {0} already exists", library.name);
            }
            else {
                println!("Library {0} not found or damaged, downloading", library.name);

                // Create folders just to make sure
                fs::create_dir_all(jar_path.parent().unwrap()).unwrap();

                // Download the jar
                downloaders_vec.push(download_to_file(jar_path_str, download_artifact.url.clone(), format!("Library {0}", library.name)));
            }
        }

        // Get name of the native's classifier wrappen in an option, returns None if no native
        let classifier_name = library.natives.as_ref().and_then(|n| {
            match get_os() {
                "windows" => n.windows.as_ref(),
                "macos" => n.osx.as_ref(),
                "linux" => n.linux.as_ref(),
                _ => None,
            }
        });

        if classifier_name.is_some() {
            // Check if the native has been downloaded
            // TODO: Classifier name could have variable substitution inside, ie "windows-${arch}", get that working
            let native_classifier = library.downloads.classifiers.as_ref().unwrap().get(classifier_name.unwrap()).unwrap();
            let jar_path = native_classifier.path.as_ref().unwrap();
            let jar_path_str = format!("{0}/libraries/{1}", minecraft_path, jar_path);
            let jar_path = Path::new(&jar_path_str);
            if check_file(jar_path, &native_classifier.sha1, native_classifier.size) {
                println!("Native for {0} already exists", library.name);
            }
            else {
                println!("Native for {0} not found or damaged, downloading", library.name);

                // Create folders just to make sure
                fs::create_dir_all(jar_path.parent().unwrap()).unwrap();

                // Download the jar
                downloaders_vec.push(download_to_file(jar_path_str, native_classifier.url.clone(), format!("Native for {0}", library.name)));
            }
        }
    }

    // Poll all downloaders until all downloads are finished
    // Maximum of 25 downloads at a time since too many downloads causes a panic
    let mut downloaders = stream::iter(downloaders_vec).map(|func| async { func.await }).buffer_unordered(25);
    while let Some(id) = downloaders.next().await {
        println!("{0} downloaded", id);
    }
    println!("All libraries checked and downloaded");
}

async fn check_minecraft_assets(minecraft_path: &str, version: &VersionSpec) {
    let index_path = format!("{0}/assets/indexes/{1}.json", minecraft_path, version.assets);
    let index_path = Path::new(&index_path);
    let mut index_json = String::new();

    // Check if the asset index is downloaded
    if check_file(index_path, &version.asset_index.sha1, version.asset_index.size) {
        // Open asset index if downloaded
        let mut index_file = File::open(index_path).unwrap();
        index_file.read_to_string(&mut index_json).unwrap();
    }
    else {
        println!("Asset Index {0} not found or damaged, downloading", version.assets);

        // Create folders just to make sure
        fs::create_dir_all(index_path.parent().unwrap()).unwrap();

        // Download the asset index
        let index_response = reqwest::get(&version.asset_index.url).await.unwrap();
        let mut index_file = File::create(index_path).unwrap();
        index_json = index_response.text().await.unwrap();
        index_file.write_all(index_json.as_bytes()).unwrap();
    }

    // Deserialize asset index
    let asset_index: AssetIndex = serde_json::from_str(&index_json).unwrap();

    // Check and download all assets
    let mut downloaders_vec = Vec::new();
    for (asset_name, asset_object) in &asset_index.objects {
        let asset_path_str = format!("{0}/assets/objects/{1}/{2}", minecraft_path, &asset_object.hash[..2], asset_object.hash);
        let asset_path = Path::new(&asset_path_str);

        if check_file(asset_path, &asset_object.hash, asset_object.size) {
            println!("Asset {0} already exists", asset_name);
        }
        else {
            println!("Asset {0} not found or damaged, downloading", asset_name);

            // Create folders just to make sure
            fs::create_dir_all(asset_path.parent().unwrap()).unwrap();

            // Download the asset
            let asset_url = format!("http://resources.download.minecraft.net/{0}/{1}", &asset_object.hash[..2], asset_object.hash);
            downloaders_vec.push(download_to_file(asset_path_str, asset_url, asset_name.to_string()));
        }
    }

    // Poll all downloaders until all downloads are finished
    // Maximum of 25 downloads at a time since too many downloads causes a panic
    let mut downloaders = stream::iter(downloaders_vec).map(|func| async { func.await }).buffer_unordered(25);
    while let Some(id) = downloaders.next().await {
        println!("Asset {0} downloaded", id);
    }

    // Copy assets to appropriate directories if needed
    for (asset_name, asset_object) in &asset_index.objects {
        let asset_path = format!("{0}/assets/objects/{1}/{2}", minecraft_path, &asset_object.hash[..2], asset_object.hash);
        let asset_path = Path::new(&asset_path);

        // Copy to either virtual or resources for older versions
        if asset_index.virtual_assets == Some(true) {
            let virtual_path = format!("{0}/assets/virtual/{1}/{2}", minecraft_path, version.assets, asset_name);
            let virtual_path = Path::new(&virtual_path);

            if check_file(virtual_path, &asset_object.hash, asset_object.size) {
                println!("Virtual asset {0} already exists", asset_name);
            }
            else {
                println!("Virtual asset {0} not found or damaged, copying", asset_name);

                // Create folders just to make sure
                fs::create_dir_all(virtual_path.parent().unwrap()).unwrap();

                // Copy the asset
                fs::copy(&asset_path, &virtual_path).unwrap();
            }
        }

        if asset_index.map_to_resources == Some(true) {
            let resource_path = format!("{0}/resources/{1}", minecraft_path, asset_name);
            let resource_path = Path::new(&resource_path);

            if check_file(resource_path, &asset_object.hash, asset_object.size) {
                println!("Resource asset {0} already exists", asset_name);
            }
            else {
                println!("Resource asset {0} not found or damaged, copying", asset_name);

                // Create folders just to make sure
                fs::create_dir_all(resource_path.parent().unwrap()).unwrap();

                // Copy the asset
                fs::copy(&asset_path, &resource_path).unwrap();
            }
        }
    }
    println!("All assets checked and downloaded");
}

fn construct_launch_args(minecraft_path: &str, version: &VersionSpec, env: &mut Environment, natives_dir: &TempDir) -> Vec<String> {
    // Construct classpath and natives directory
    // TODO: Move classpath construction to library
    let mut classpath = String::new();
    for library in version.libraries.iter() {
        // Check if library rules are satisfied and skip if not
        if library.rules.is_some() && !spec_rules_satisfied(library.rules.as_ref().unwrap()) {
            continue;
        }

        // Check if the library has a general jar
        if library.downloads.artifact.is_some() {
            // Uses successive shadowing to please the borrow checker, plus it shows the successive building of the path
            // Need as_ref before unwrapping the option so as to not consume it
            let download_artifact = library.downloads.artifact.as_ref().unwrap();
            let jar_path = download_artifact.path.as_ref().unwrap();
            let jar_path = format!("{0}/libraries/{1}", minecraft_path, jar_path);

            // Add to the classpath
            classpath += &jar_path;
            if get_os() == "windows" {
                classpath += ";";
            }
            else {
                classpath += ":";
            }
        }

        // Get name of the native's classifier wrappen in an option, returns None if no native
        let classifier_name = library.natives.as_ref().and_then(|n| {
            match get_os() {
                "windows" => n.windows.as_ref(),
                "macos" => n.osx.as_ref(),
                "linux" => n.linux.as_ref(),
                _ => None,
            }
        });

        if classifier_name.is_some() {
            // Check if the native has been downloaded
            // TODO: Classifier name could have variable substitution inside, ie "windows-${arch}", get that working
            let native_classifier = library.downloads.classifiers.as_ref().unwrap().get(classifier_name.unwrap()).unwrap();
            let jar_path = native_classifier.path.as_ref().unwrap();
            let jar_path = format!("{0}/libraries/{1}", minecraft_path, jar_path);
            let jar_path = Path::new(&jar_path);

            // Extract into the natives directory
            let natives_jar = File::open(jar_path).unwrap();
            let mut archive = ZipArchive::new(natives_jar).unwrap();
            archive.extract(natives_dir.path()).unwrap();
            println!("Extracted native for {0}", library.name);
        }
    }
    let jar_path = format!("{0}/versions/{1}/{1}.jar", minecraft_path, version.id);
    classpath += &jar_path; // Don't forget to add the Minecraft jar itself
    env.set("classpath", &classpath);
    env.set("natives_directory", natives_dir.path().to_str().unwrap());

    // Construct the launch arguments
    let mut launch_args = Vec::<String>::new();
    if version.arguments.is_some() {
        for arg in version.arguments.as_ref().unwrap().jvm.iter() {
            match arg {
                Argument::Static(arg_str) => launch_args.push(arg_str.to_string()),
                Argument::Dynamic(dynamic_arg) => {
                    if spec_rules_satisfied(&dynamic_arg.rules) {
                        match &dynamic_arg.value {
                            SingleOrVec::Single(dynamic_arg_value) => launch_args.push(dynamic_arg_value.to_string()),
                            SingleOrVec::Vector(dynamic_arg_vec) => {
                                for dynamic_arg_value in dynamic_arg_vec.iter() {
                                    launch_args.push(dynamic_arg_value.to_string());
                                }
                            },
                        }
                    }
                },
            }
        }
        launch_args.push(version.main_class.clone());
        for arg in version.arguments.as_ref().unwrap().game.iter() {
            match arg {
                Argument::Static(arg_str) => launch_args.push(arg_str.to_string()),
                Argument::Dynamic(dynamic_arg) => {
                    if spec_rules_satisfied(&dynamic_arg.rules) {
                        match &dynamic_arg.value {
                            SingleOrVec::Single(dynamic_arg_value) => launch_args.push(dynamic_arg_value.to_string()),
                            SingleOrVec::Vector(dynamic_arg_vec) => {
                                for dynamic_arg_value in dynamic_arg_vec.iter() {
                                    launch_args.push(dynamic_arg_value.to_string());
                                }
                            },
                        }
                    }
                },
            }
        }
    }
    else {
        // Hardcoded JVM arguments, since they're not specified in the version spec
        if get_os() == "windows" {
            launch_args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".to_string());
            // TODO: Do -Dos.name=Windows 10 and -Dos.version=10.0 if Windows 10
        }
        if get_os() == "macos" {
            launch_args.push("-XstartOnFirstThread".to_string());
        }
        if get_arch() == "x86" {
            launch_args.push("-Xss1M".to_string());
        }
        launch_args.push("-Djava.library.path=${natives_directory}".to_string());
        launch_args.push("-Dminecraft.launcher.brand=${launcher_name}".to_string());
        launch_args.push("-Dminecraft.launcher.version=${launcher_version}".to_string());
        launch_args.push(format!("-Dminecraft.client.jar={0}", jar_path).to_string());
        launch_args.push("-cp".to_string());
        launch_args.push("${classpath}".to_string());
        launch_args.push(version.main_class.clone());
        let mut minecraft_args: Vec<String> = version.minecraft_arguments.as_ref().unwrap().split(" ").map(|s| s.to_string()).collect();
        launch_args.append(&mut minecraft_args);
    }

    // Replace ${config} variables with the values
    for arg in launch_args.iter_mut() {
        *arg = env.resolve(arg);
    }

    return launch_args;
}

fn spec_rules_satisfied(rules: &Vec<Rule>) -> bool {
    for rule in rules {
        // Define whether to return on a match or mismatch
        let allow_match = match rule.action.as_str() {
            "allow" => true,
            "disallow" => false,
            _ => panic!("Unknown rule action"),
        };

        // Check if os is matched
        if rule.os.is_some() {
            // TODO: support version matching, no clue how to get version currently
            let os_ok = match rule.os.as_ref().unwrap().name.as_ref() {
                Some(s) if s == get_os_minecraft() => true,
                Some(_) => false,
                _ => true,
            };
            let arch_ok = match rule.os.as_ref().unwrap().arch.as_ref() {
                Some(s) if s == get_arch() => true,
                Some(_) => false,
                _ => true,
            };

            if os_ok && arch_ok && !allow_match {
                return false;
            }
            if !(os_ok && arch_ok) && allow_match {
                return false;
            }
        }

        if rule.features.is_some() {
            // TODO: Implement real features checking
            // For now, ignore since the only features are has_custom_resolution and is_demo_user,
            // and we don't want either to trip.
            return false;
        }
    }
    return true;
}
