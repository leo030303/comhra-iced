use std::fs;
use std::os::linux::fs::MetadataExt;
use std::path::PathBuf;

use arboard::Clipboard;
use iced::widget::svg::Handle;
use iced::widget::{
    button, column, container, markdown, row, scrollable, text, text_input, Row, Space, Svg,
    Tooltip,
};
use iced::{Center, Element, Length, Subscription, Task, Theme};
use iced_aw::Spinner;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::{ChatMessage, MessageRole};
use ollama_rs::models::LocalModel;
use ollama_rs::Ollama;

pub fn main() -> iced::Result {
    iced::application("Comhrá", App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .run_with(App::new)
}

#[derive(Default)]
struct App {
    ollama: Ollama,
    prompt: String,
    current_model: Option<LocalModel>,
    current_conversation: Option<PathBuf>,
    chats_list: Vec<(ChatMessage, Vec<markdown::Item>)>,
    models_list: Vec<LocalModel>,
    conversations_list: Vec<PathBuf>,
    show_sidebar: bool,
    is_generating: bool,
}

#[derive(Debug, Clone)]
enum Message {
    SetModelsList(Vec<LocalModel>),
    SetConversationsList(Vec<PathBuf>),
    SetConversationFile(Option<PathBuf>),
    SetModel(Option<LocalModel>),
    ToggleSidebar,
    LinkClicked(markdown::Url),
    CopyChat(String),
    UpdatePrompt(String),
    SubmitPrompt,
    SaveConversation,
    LoadConversation,
    HandleStreamResponse(String),
    NewChat,
    NewChatButtonPressed,
    LoadConversationList,
    ToggleIsGenerating,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let ollama = Ollama::default();
        (
            Self {
                ollama: ollama.clone(),
                prompt: String::new(),
                models_list: vec![],
                conversations_list: vec![],
                show_sidebar: true,
                current_model: None,
                current_conversation: None,
                chats_list: vec![],
                is_generating: false,
            },
            Task::batch([
                Task::perform(
                    async move { ollama.list_local_models().await.unwrap() },
                    Message::SetModelsList,
                ),
                Task::done(Message::LoadConversationList),
            ]),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SetModelsList(models_list) => self.models_list = models_list,
            Message::SetConversationsList(conversations_list) => {
                self.conversations_list = conversations_list
            }
            Message::SetConversationFile(conversation) => {
                self.current_conversation = conversation.clone();
                if conversation.is_some() {
                    return Task::done(Message::LoadConversation);
                }
            }
            Message::SetModel(model) => self.current_model = model,
            Message::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Message::LinkClicked(url) => {
                println!("The following url was clicked: {url}");
            }
            Message::CopyChat(s) => Clipboard::new().unwrap().set_text(s).unwrap(),
            Message::UpdatePrompt(s) => self.prompt = s,
            Message::SubmitPrompt => {
                let mut reload_conversation_list = false;
                if self.current_conversation.is_none() {
                    let mut conversation_file =
                        dirs::config_dir().expect("Couldn't find config dir");
                    conversation_file.push("github.com.leo030303.comhra/");
                    conversation_file.push("conversations/");
                    let mut filename = match self.prompt.split_at_checked(40) {
                        Some((title, _)) => title.to_string(),
                        None => self.prompt.clone(),
                    };
                    filename.push_str(".json");
                    conversation_file.push(filename);
                    self.current_conversation = Some(conversation_file);
                    reload_conversation_list = true;
                };
                let markdown_items = markdown::parse(&self.prompt).collect();
                self.chats_list.push((
                    ChatMessage {
                        role: MessageRole::User,
                        content: self.prompt.clone(),
                        images: None,
                    },
                    markdown_items,
                ));
                self.chats_list.push((
                    ChatMessage {
                        role: MessageRole::Assistant,
                        content: String::new(),
                        images: None,
                    },
                    vec![],
                ));
                let conversation: Vec<ChatMessage> = self
                    .chats_list
                    .iter()
                    .map(|(chat_message, _markdown_items)| chat_message.clone())
                    .collect();
                let chat_request =
                    ChatMessageRequest::new(self.current_model.clone().unwrap().name, conversation);
                let ollama = self.ollama.clone();
                self.prompt = String::new();
                return Task::done(Message::ToggleIsGenerating)
                    .chain(
                        Task::future(async move {
                            ollama.send_chat_messages_stream(chat_request).await
                        })
                        .and_then(move |stream| {
                            Task::run(stream, |stream_responses| {
                                let parsed_response =
                                    stream_responses.unwrap().message.unwrap().content;
                                Message::HandleStreamResponse(parsed_response)
                            })
                            .chain(Task::done(Message::SaveConversation))
                            .chain({
                                if reload_conversation_list {
                                    Task::done(Message::LoadConversationList)
                                } else {
                                    Task::none()
                                }
                            })
                        }),
                    )
                    .chain(Task::done(Message::ToggleIsGenerating));
            }
            Message::SaveConversation => {
                if let Some(current_conversation) = self.current_conversation.as_ref() {
                    fs::write(
                        current_conversation,
                        serde_json::to_string(
                            &self
                                .chats_list
                                .iter()
                                .map(|(chat_message, _markdown_items)| chat_message.clone())
                                .collect::<Vec<ChatMessage>>(),
                        )
                        .unwrap(),
                    )
                    .unwrap()
                }
            }
            Message::LoadConversation => {
                if let Ok(conversation_json) =
                    fs::read_to_string(self.current_conversation.as_ref().unwrap())
                {
                    let conversation: Vec<ChatMessage> =
                        serde_json::from_str(&conversation_json).unwrap_or_default();
                    self.chats_list = conversation
                        .into_iter()
                        .map(|chat_message| {
                            let markdown_items = markdown::parse(&chat_message.content)
                                .collect::<Vec<markdown::Item>>();
                            (chat_message, markdown_items)
                        })
                        .collect();
                };
            }
            Message::HandleStreamResponse(next_chunk) => {
                let (chat_message, markdown_vec) = self.chats_list.last_mut().unwrap();
                chat_message.content.push_str(&next_chunk);
                let markdown_items =
                    markdown::parse(&chat_message.content).collect::<Vec<markdown::Item>>();
                markdown_vec.clear();
                markdown_vec.extend(markdown_items);
            }
            Message::NewChat => {
                self.current_conversation = None;
                self.chats_list = vec![];
            }
            Message::NewChatButtonPressed => {
                return Task::done(Message::SaveConversation).chain(Task::done(Message::NewChat))
            }
            Message::LoadConversationList => {
                return Task::perform(
                    async {
                        let mut config_dir = dirs::config_dir().expect("Couldn't find config dir");
                        config_dir.push("github.com.leo030303.comhra/");
                        config_dir.push("conversations/");
                        if !config_dir.exists() {
                            fs::create_dir_all(&config_dir).expect("Error making the config dir");
                        };
                        let mut conversations_list: Vec<PathBuf> = fs::read_dir(config_dir)
                            .unwrap()
                            .map(|read_dir| read_dir.unwrap().path())
                            .collect();
                        conversations_list.sort_unstable_by(|a, b| {
                            a.metadata()
                                .unwrap()
                                .st_mtime()
                                .partial_cmp(&b.metadata().unwrap().st_mtime())
                                .unwrap()
                                .reverse()
                        });
                        conversations_list
                    },
                    Message::SetConversationsList,
                );
            }
            Message::ToggleIsGenerating => self.is_generating = !self.is_generating,
        };
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn view(&self) -> Element<Message> {
        row![if self.current_model.is_none() {
            column![scrollable(
                column(self.models_list.iter().map(|model| {
                    button(
                        text(&model.name)
                            .width(Length::Fixed(250.0))
                            .align_x(Center)
                            .size(20),
                    )
                    .on_press(Message::SetModel(Some(model.clone())))
                    .into()
                }))
                .spacing(10)
            )]
            .padding(30)
            .align_x(Center)
            .width(Length::Fill)
        } else {
            column![
                row![
                    Tooltip::new(
                        button(Svg::new(Handle::from_memory(include_bytes!(
                            "../icons/toggle-sidebar.svg"
                        ))))
                        .height(Length::Fill)
                        .on_press(Message::ToggleSidebar)
                        .style(if self.show_sidebar {
                            button::secondary
                        } else {
                            button::primary
                        })
                        .width(Length::Fixed(50.0)),
                        "Toggle Sidebar",
                        iced::widget::tooltip::Position::Bottom
                    ),
                    Tooltip::new(
                        button(Svg::new(Handle::from_memory(include_bytes!(
                            "../icons/add.svg"
                        ))))
                        .height(Length::Fill)
                        .on_press(Message::NewChatButtonPressed)
                        .width(Length::Fixed(50.0)),
                        "New Chat",
                        iced::widget::tooltip::Position::Bottom
                    ),
                    button(text("Select Model").width(Length::Fill).align_x(Center))
                        .on_press(Message::SetModel(None))
                        .height(Length::Fill)
                        .width(Length::Fixed(170.0)),
                    text(
                        self.current_model
                            .clone()
                            .map(|model| model.name)
                            .unwrap_or_default()
                    )
                    .width(Length::Fill)
                    .align_x(Center)
                    .size(24),
                ]
                .height(Length::Fixed(30.0)),
                row![
                    if self.show_sidebar {
                        container(column![
                            text("Conversations")
                                .width(Length::Fill)
                                .align_x(Center)
                                .size(24),
                            scrollable(
                                column(self.conversations_list.iter().map(|conversation_path| {
                                    button(
                                        text(
                                            conversation_path
                                                .file_stem()
                                                .unwrap_or_default()
                                                .to_str()
                                                .unwrap_or_default(),
                                        )
                                        .width(Length::Fill)
                                        .align_x(Center),
                                    )
                                    .width(Length::Fill)
                                    .on_press(Message::SetConversationFile(Some(
                                        conversation_path.clone(),
                                    )))
                                    .into()
                                }))
                                .spacing(5)
                            )
                        ])
                        .style(container::bordered_box)
                        .height(Length::Fill)
                        .width(Length::FillPortion(1))
                    } else {
                        container(column![])
                    },
                    column![
                        scrollable(column(self.chats_list.iter().map(
                            |(chat_message, markdown_items)| {
                                column![
                                    {
                                        let chat_message_title_row = Row::new().spacing(10);
                                        let title_text: Element<Message> =
                                            text(match chat_message.role {
                                                MessageRole::User => "User",
                                                MessageRole::Assistant => "Assistant",
                                                MessageRole::System => "System",
                                            })
                                            .size(20)
                                            .into();
                                        let spacer = Space::with_width(Length::Fill);
                                        let copy_button: Element<Message> = Tooltip::new(
                                            button(
                                                Svg::new(Handle::from_memory(include_bytes!(
                                                    "../icons/copy.svg"
                                                )))
                                                .height(Length::Fixed(20.0)),
                                            )
                                            .on_press(Message::CopyChat(
                                                chat_message.content.clone(),
                                            ))
                                            .width(Length::Fixed(50.0)),
                                            "Copy",
                                            iced::widget::tooltip::Position::Bottom,
                                        )
                                        .into();
                                        if let MessageRole::User = chat_message.role {
                                            chat_message_title_row
                                                .push(title_text)
                                                .push(copy_button)
                                                .push(spacer)
                                        } else {
                                            chat_message_title_row
                                                .push(spacer)
                                                .push(copy_button)
                                                .push(title_text)
                                        }
                                    },
                                    markdown::view(
                                        markdown_items,
                                        markdown::Settings::default(),
                                        markdown::Style::from_palette(
                                            Theme::TokyoNightStorm.palette()
                                        ),
                                    )
                                    .map(Message::LinkClicked),
                                ]
                                .padding(20)
                                .into()
                            }
                        )))
                        .height(Length::Fill),
                        row![
                            text_input("Enter your chat", &self.prompt)
                                .on_input(Message::UpdatePrompt)
                                .on_submit(Message::SubmitPrompt),
                            if self.is_generating {
                                column![Spinner::new()].width(30.0)
                            } else {
                                column![].width(30.0)
                            }
                        ]
                        .padding(10)
                    ]
                    .width(Length::FillPortion(2))
                    .padding(10)
                ]
            ]
        }]
        .into()
    }

    fn theme(&self) -> Theme {
        Theme::TokyoNightStorm
    }
}
