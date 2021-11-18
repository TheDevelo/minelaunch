mod minecraft;
mod env;
mod util;

use std::process::ExitStatus;
use std::time::Duration;
use iced::{Align, Application, Button, Clipboard, Column, Command, Container, Element, Length, PickList, Row, Settings, Space, Subscription, Text, TextInput};
use iced::{button, executor, pick_list, text_input, time, window};
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
enum VersionSelection {
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

struct ApplicationState {
    launcher_path: String,
    versions: MinecraftVersionList,
    env: Environment,
}

enum Tab {
    Launcher,
    Downloader,
}

struct GUI {
    state: ApplicationState,
    tab: Tab,
    launcher_tab: Launcher,
    downloader_tab: Downloader,

    launcher_button_state: button::State,
    downloader_button_state: button::State,
}

#[derive(Debug, Clone)]
enum Message {
    LauncherPressed,
    DownloaderPressed,
    LauncherMessage(LauncherMessage),
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

        let state = ApplicationState {
            launcher_path: minecraft_path.to_string(),
            versions: minecraft_versions,
            env: env,
        };

        let gui_state = Self {
            tab: Tab::Launcher,
            launcher_tab: Launcher::new(&state),
            downloader_tab: Downloader {},
            state: state,

            launcher_button_state: button::State::default(),
            downloader_button_state: button::State::default(),
        };
        return (gui_state, Command::none());
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
                Row::new()
                .push(
                    Button::new(&mut self.launcher_button_state, Text::new("Launcher"))
                        .on_press(Message::LauncherPressed)
                ).push(Space::with_width(Length::Units(20)))
                .push(
                    Button::new(&mut self.downloader_button_state, Text::new("Downloader"))
                        .on_press(Message::DownloaderPressed)
            )).push(Space::with_height(Length::Units(10)));

        match self.tab {
            Tab::Launcher => {
                content = content.push(self.launcher_tab.view(&self.state));
            }
            Tab::Downloader => {
            }
        }

        return Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into();
    }

    fn update(&mut self, message: Message, _clipboard: &mut Clipboard) -> Command<Message> {
        match message {
            Message::LauncherPressed => {
                self.tab = Tab::Launcher;
            },
            Message::DownloaderPressed => {
                self.tab = Tab::Downloader;
            },
            Message::LauncherMessage(launcher_msg) => {
                return self.launcher_tab.update(&mut self.state, launcher_msg);
            }
        }
        return Command::none();
    }

    fn subscription(&self) -> Subscription<Message> {
        return Subscription::none();
    }
}

#[derive(Debug, Clone)]
enum LauncherMessage {
    LaunchPressed,
    VersionSelected(VersionSelection),
    UsernameChanged(String),
    MinecraftExited(ExitStatus),
}

struct Launcher {
    selected_version: VersionSelection,
    last_exit_status: Option<ExitStatus>,
    username: String,

    launch_button_state: button::State,
    version_dropdown_state: pick_list::State<VersionSelection>,
    username_input_state: text_input::State,
}

impl Launcher {
    fn new(state: &ApplicationState) -> Self {
        Launcher {
            selected_version: VersionSelection::Latest(state.versions.latest.release.clone()),
            last_exit_status: None,
            username: String::from(""),

            launch_button_state: button::State::default(),
            version_dropdown_state: pick_list::State::default(),
            username_input_state: text_input::State::default(),
        }
    }

    fn view(&mut self, state: &ApplicationState) -> Element<Message> {
        let mut content = Column::new()
            .align_items(Align::Center)
            .push(
                PickList::new(&mut self.version_dropdown_state, VersionSelection::make_list(&state.versions), Some(self.selected_version.clone()),
                              move |v| { Message::LauncherMessage(LauncherMessage::VersionSelected(v)) })
            ).push(Space::with_height(Length::Units(10)))
            .push(Text::new("Username:"))
            .push(
                TextInput::new(&mut self.username_input_state, "Enter your username...", &self.username,
                               move |s| { Message::LauncherMessage(LauncherMessage::UsernameChanged(s)) })
                .padding(5)
                .width(Length::Units(286))
            ).push(Space::with_height(Length::FillPortion(1)));

        if self.last_exit_status.is_some() {
            content = content.push(Text::new(format!("Minecraft exited with {0}", self.last_exit_status.unwrap())));
        }

        content = content.push(Space::with_height(Length::FillPortion(1)))
            .push(
                Button::new(&mut self.launch_button_state, Text::new("Launch"))
                    .on_press(Message::LauncherMessage(LauncherMessage::LaunchPressed))
            ).push(Space::with_height(Length::Units(10)));

        return content.into();
    }

    fn update(&mut self, state: &mut ApplicationState, message: LauncherMessage) -> Command<Message> {
        match message {
            LauncherMessage::LaunchPressed => {
                self.last_exit_status = None;

                let mut version = state.versions.versions.get(0).unwrap();
                match &self.selected_version {
                    VersionSelection::Latest(id) => {
                        for v in state.versions.versions.iter() {
                            if v.id == *id {
                                version = v;
                                break;
                            }
                        }
                    },
                    VersionSelection::LatestSnapshot(id) => {
                        for v in state.versions.versions.iter() {
                            if v.id == *id {
                                version = v;
                                break;
                            }
                        }
                    },
                    VersionSelection::Version(v) => { version = &v; },
                };

                return Command::perform(launch_minecraft_version(state.launcher_path.clone(), version.clone(), Box::new(state.env.clone())),
                                        move |s| { Message::LauncherMessage(LauncherMessage::MinecraftExited(s)) });
            },
            LauncherMessage::VersionSelected(version) => {
                self.selected_version = version;
            },
            LauncherMessage::UsernameChanged(username) => {
                self.username = username;
                state.env.set("auth_player_name", &self.username);
            }
            LauncherMessage::MinecraftExited(status) => {
                self.last_exit_status = Some(status);
            }
        }
        return Command::none();
    }
}

struct Downloader {
}
