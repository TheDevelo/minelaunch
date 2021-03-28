use std::sync::Arc;
use std::process::ExitStatus;
use std::time::Duration;
use iced::{Align, Application, Button, Column, Command, Container, Element, Length, PickList, Space, Subscription, Text, TextInput};
use iced::{button, executor, pick_list, text_input, time};
use async_std::sync::Mutex;

use super::{MinecraftVersionList, MinecraftVersion};
use super::env::Environment;

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
    type Flags = (String, MinecraftVersionList, Environment);

    fn new(flags: Self::Flags) -> (Self, Command<Message>) {
        let state = Self {
            launcher_path: flags.0,
            selected_version: VersionSelection::Latest(flags.1.latest.release.clone()),
            versions: flags.1,
            env: Arc::new(Mutex::new(flags.2)),
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

    fn update(&mut self, message: Message) -> Command<Message> {
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

                return Command::perform(super::launch_minecraft_version(self.launcher_path.clone(), version.clone(), self.env.clone()),
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
