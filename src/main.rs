mod minecraft;
mod env;
mod util;

use std::sync::Arc;
use std::process::ExitStatus;
use std::time::Duration;
use iced::{Align, Application, Button, Clipboard, Column, Command, Container, Element, Length, PickList, Settings, Space, Subscription, Text, TextInput};
use iced::{button, executor, pick_list, text_input, time, window};
use async_std::sync::Mutex;
use async_std::task;

use minecraft::{MinecraftVersionList, MinecraftVersion, launch_minecraft_version};
use env::Environment;

fn main() -> iced::Result {
    // Launch the GUI
    let settings = Settings {
        window: window::Settings {
            size: (320, 440),
            min_size: Some((320, 230)),
            icon: Some(window::Icon::from_rgba(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon.raw")).to_vec(), 128, 128).unwrap()),
            ..window::Settings::default()
        },
        ..Settings::default()
    };
    GUI::run(settings)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSelection {
    Latest(String),
    LatestSnapshot(String),
    Version(MinecraftVersion),
}

impl VersionSelection {
    fn make_list(version_list: &MinecraftVersionList) -> Vec<VersionSelection> {
        let mut list = vec![VersionSelection::Latest(version_list.latest.release.clone()),
                            VersionSelection::LatestSnapshot(version_list.latest.snapshot.clone())];
        for v in version_list.versions.iter() {
            list.push(VersionSelection::Version(v.clone()));
        }
        return list;
    }
}

impl std::fmt::Display for VersionSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{0}", match self {
            VersionSelection::Latest(id) => format!("Latest Release ({0})", id),
            VersionSelection::LatestSnapshot(id) => format!("Latest Snapshot ({0})", id),
            VersionSelection::Version(v) => format!("{0} {1}", v.version_type, v.id),
        })
    }
}

pub struct GUI {
    launcher_path: String,
    versions: MinecraftVersionList,
    selected_version: VersionSelection,
    env: Arc<Mutex<Environment>>,
    last_exit_status: Option<ExitStatus>,
    username: String,

    check_env: bool,

    launch_button_state: button::State,
    version_dropdown_state: pick_list::State<VersionSelection>,
    username_input_state: text_input::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchPressed,
    VersionSelected(VersionSelection),
    UsernameChanged(String),
    CheckEnv,
    MinecraftExited(ExitStatus),
}

impl Application for GUI {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Message>) {
        let minecraft_path = ".";
        let mut env = Environment::new();
        env.set("game_directory", minecraft_path);
        env.set("launcher_name", "Minelaunch");
        env.set("launcher_version", env!("CARGO_PKG_VERSION"));
        env.set("auth_player_name", "");
        env.set("auth_uuid", ""); // TODO: Allow logging in
        env.set("auth_access_token", "");
        env.set("user_type", "offline"); // mojang for Mojang, msa for Microsoft

        // Get list of Minecraft versions
        let minecraft_versions_response = task::block_on(reqwest::get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")).unwrap();
        let minecraft_versions_text = task::block_on(minecraft_versions_response.text()).unwrap();
        let minecraft_versions: MinecraftVersionList = serde_json::from_str(&minecraft_versions_text).unwrap();

        let state = Self {
            launcher_path: minecraft_path.to_string(),
            selected_version: VersionSelection::Latest(minecraft_versions.latest.release.clone()),
            versions: minecraft_versions,
            env: Arc::new(Mutex::new(env)),
            last_exit_status: None,
            username: String::from(""),

            check_env: false,

            launch_button_state: button::State::default(),
            version_dropdown_state: pick_list::State::default(),
            username_input_state: text_input::State::default(),
        };
        return (state, Command::none());
    }

    fn title(&self) -> String {
        return String::from("Minelaunch");
    }

    fn view(&mut self) -> Element<Message> {
        let mut content = Column::new()
            .align_items(Align::Center)
            .push(Space::with_height(Length::Units(10)))
            .push(Text::new("Minelaunch"))
            .push(Text::new(format!("Version {0}", env!("CARGO_PKG_VERSION"))))
            .push(Space::with_height(Length::Units(10)))
            .push(
                PickList::new(&mut self.version_dropdown_state, VersionSelection::make_list(&self.versions),
                              Some(self.selected_version.clone()), Message::VersionSelected)
            ).push(Space::with_height(Length::Units(10)))
            .push(Text::new("Username:"))
            .push(
                TextInput::new(&mut self.username_input_state, "Enter your username...", &self.username, Message::UsernameChanged)
                .padding(5)
                .width(Length::Units(286))
            ).push(Space::with_height(Length::FillPortion(1)));

        if self.last_exit_status.is_some() {
            content = content.push(Text::new(format!("Minecraft exited with {0}", self.last_exit_status.unwrap())));
        }

        content = content.push(Space::with_height(Length::FillPortion(1)))
            .push(
                Button::new(&mut self.launch_button_state, Text::new("Launch"))
                    .on_press(Message::LaunchPressed)
            ).push(Space::with_height(Length::Units(10)));

        return Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into();
    }

    fn update(&mut self, message: Message, _clipboard: &mut Clipboard) -> Command<Message> {
        match message {
            Message::LaunchPressed => {
                self.last_exit_status = None;

                let mut version = self.versions.versions.get(0).unwrap();
                match &self.selected_version {
                    VersionSelection::Latest(id) => {
                        for v in self.versions.versions.iter() {
                            if v.id == *id {
                                version = v;
                                break;
                            }
                        }
                    },
                    VersionSelection::LatestSnapshot(id) => {
                        for v in self.versions.versions.iter() {
                            if v.id == *id {
                                version = v;
                                break;
                            }
                        }
                    },
                    VersionSelection::Version(v) => { version = &v; },
                };

                return Command::perform(launch_minecraft_version(self.launcher_path.clone(), version.clone(), self.env.clone()),
                                        move |s| { Message::MinecraftExited(s) });
            },
            Message::VersionSelected(version) => {
                self.selected_version = version;
            },
            Message::UsernameChanged(username) => {
                self.username = username;
                self.check_env = true;
            }
            Message::CheckEnv => {
                let env = self.env.try_lock();
                if env.is_some() {
                    let mut env = env.unwrap();
                    env.set("auth_player_name", &self.username);
                    self.check_env = false;
                }
            }
            Message::MinecraftExited(status) => {
                self.last_exit_status = Some(status);
            }
        }
        return Command::none();
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.check_env {
            return time::every(Duration::from_millis(10)).map(|_| { Message::CheckEnv });
        }
        return Subscription::none();
    }
}
