use crate::abi::message::Sender;
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Target {
    Group(i64),
    Private(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Message,
    Notice,
    Request,
}

pub trait MessageType {
    fn get_target(&self) -> Target;
    fn get_type(&self) -> Type;
    fn get_text(&self) -> String;
    fn get_sender(&self) -> Sender;
}

#[derive(Deserialize, Debug)]
#[serde(tag = "post_type", rename_all = "snake_case")]
pub enum Event {
    Message(Box<message::Message>),
    Notice(notice::Notice),
    Request(request::Request),
    MetaEvent(meta::MetaEvent),
    MessageSent(Box<message_sent::MessageSent>),
}

pub mod message {
    use crate::abi::message::{
        MessageReceive, SenderGroup, SenderPrivate,
        sender::{Role, SenderRole},
    };

    use super::*;

    #[derive(Deserialize, Debug, Serialize, Clone)]
    #[serde(tag = "message_type", rename_all = "snake_case")]
    pub enum Message {
        Private(Private),
        Group(Group),
    }

    impl MessageType for Message {
        fn get_target(&self) -> Target {
            match self {
                Message::Private(private) => Target::Private(private.user_id),
                Message::Group(group) => Target::Group(group.group_id),
            }
        }

        fn get_type(&self) -> Type {
            Type::Message
        }

        fn get_text(&self) -> String {
            match self {
                Message::Private(private) => private.message.get_text(),
                Message::Group(group) => group.message.get_text(),
            }
        }
        fn get_sender(&self) -> Sender {
            match self {
                Message::Group(p) => Sender {
                    nickname: p.sender.nickname.clone(),
                    user_id: p.sender.user_id,
                    card: p.sender.card.clone(),
                    role: match p.sender.role {
                        Some(Role::Admin) => Some(SenderRole::GroupAdmin),
                        Some(Role::Owner) => Some(SenderRole::GroupOwner),
                        Some(Role::Member) => Some(SenderRole::GroupMember),
                        None => None,
                    },
                },
                Message::Private(p) => Sender {
                    nickname: p.sender.nickname.clone(),
                    user_id: p.sender.user_id,
                    card: p.sender.card.clone(),
                    role: Some(SenderRole::Friend),
                },
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypePrivate {
        Friend,
        Group,
        Other,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Private {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypePrivate,
        pub message_id: i32,
        pub user_id: i64,
        pub raw_message: String,
        pub font: i32,
        pub sender: SenderPrivate,
        /// 此字段已被 `serde` 忽略。
        #[serde(skip_serializing)]
        pub message: MessageReceive,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypeGroup {
        Normal,
        Anonymous,
        Notice,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Anonymous {
        pub id: i64,
        pub name: String,
        pub flag: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Group {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypeGroup,
        pub message_id: i32,
        pub group_id: i64,
        pub user_id: i64,
        pub anonymous: Option<Anonymous>,
        pub raw_message: String,
        pub font: i32,
        pub sender: SenderGroup,
        /// 此字段已被 `serde` 忽略。
        #[serde(skip_serializing)]
        pub message: MessageReceive,
    }
}

pub mod notice {
    use crate::abi::message::file::File;

    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "notice_type", rename_all = "snake_case")]
    pub enum Notice {
        GroupUpload(GroupUpload),
        GroupAdmin(GroupAdmin),
        GroupDecrease(GroupDecrease),
        GroupIncrease(GroupIncrease),
        GroupBan(GroupBan),
        FriendAdd(FriendAdd),
        GroupRecall(GroupRecall),
        FriendRecall(FriendRecall),
        GroupMsgEmojiLike(GroupMsgEmojiLike),
        Notify(Notify),
        GroupCard(GroupCard),
    }

    impl MessageType for Notice {
        fn get_target(&self) -> Target {
            match self {
                Notice::GroupUpload(n) => Target::Group(n.group_id),
                Notice::GroupAdmin(n) => Target::Group(n.group_id),
                Notice::GroupDecrease(n) => Target::Group(n.group_id),
                Notice::GroupIncrease(n) => Target::Group(n.group_id),
                Notice::GroupBan(n) => Target::Group(n.group_id),
                Notice::FriendAdd(n) => Target::Private(n.user_id),
                Notice::GroupRecall(n) => Target::Group(n.group_id),
                Notice::FriendRecall(n) => Target::Private(n.user_id),
                Notice::Notify(notify) => match notify {
                    Notify::Poke(poke) => Target::Group(poke.group_id),
                    Notify::LuckyKing(lucky_king) => Target::Group(lucky_king.group_id),
                    Notify::Honor(honor) => Target::Group(honor.group_id),
                    Notify::Title(title) => Target::Group(title.group_id),
                },
                Notice::GroupMsgEmojiLike(g) => Target::Group(g.group_id),
                Notice::GroupCard(g) => Target::Group(g.group_id),
            }
        }

        fn get_type(&self) -> Type {
            Type::Notice
        }

        fn get_text(&self) -> String {
            String::new()
        }

        fn get_sender(&self) -> Sender {
            match self {
                Notice::GroupUpload(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupAdmin(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupDecrease(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupIncrease(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupBan(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::FriendAdd(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupRecall(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::FriendRecall(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Notice::Notify(notify) => match notify {
                    Notify::Poke(poke) => Sender {
                        nickname: None,
                        user_id: Some(poke.user_id),
                        card: None,
                        role: None,
                    },
                    Notify::LuckyKing(lucky_king) => Sender {
                        nickname: None,
                        user_id: Some(lucky_king.user_id),
                        card: None,
                        role: None,
                    },
                    Notify::Honor(honor) => Sender {
                        nickname: None,
                        user_id: Some(honor.user_id),
                        card: None,
                        role: None,
                    },
                    Notify::Title(title) => Sender {
                        nickname: None,
                        user_id: Some(title.user_id),
                        card: None,
                        role: None,
                    },
                },
                Notice::GroupMsgEmojiLike(g) => Sender {
                    nickname: None,
                    user_id: Some(g.user_id),
                    card: None,
                    role: None,
                },
                Notice::GroupCard(g) => Sender {
                    nickname: None,
                    user_id: Some(g.user_id),
                    card: None,
                    role: None,
                },
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupUpload {
        pub time: i64,
        pub self_id: i64,
        pub group_id: i64,
        pub user_id: i64,
        pub file: File,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypeGroupAdmin {
        Set,
        Unset,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupAdmin {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypeGroupAdmin,
        pub group_id: i64,
        pub user_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypeGroupDecrease {
        Leave,
        Kick,
        KickMe,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupDecrease {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypeGroupDecrease,
        pub group_id: i64,
        pub operator_id: i64,
        pub user_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypeGroupIncrease {
        Approve,
        Invite,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupIncrease {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypeGroupIncrease,
        pub group_id: i64,
        pub operator_id: i64,
        pub user_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubTypeGroupBan {
        Ban,
        LiftBan,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupBan {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubTypeGroupBan,
        pub group_id: i64,
        pub operator_id: i64,
        pub user_id: i64,
        pub duration: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct FriendAdd {
        pub time: i64,
        pub self_id: i64,
        pub user_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupRecall {
        pub time: i64,
        pub self_id: i64,
        pub group_id: i64,
        pub user_id: i64,
        pub operator_id: i64,
        pub message_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct FriendRecall {
        pub time: i64,
        pub self_id: i64,
        pub user_id: i64,
        pub message_id: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupMsgEmojiLikeItem {
        pub emoji_id: String,
        pub count: i64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupMsgEmojiLike {
        pub time: i64,
        pub self_id: i64,
        pub group_id: i64,
        pub user_id: i64,
        pub message_id: i64,
        pub likes: Vec<GroupMsgEmojiLikeItem>,
        pub is_add: bool,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "sub_type", rename_all = "snake_case")]
    pub enum Notify {
        Poke(notify::Poke),
        LuckyKing(notify::LuckyKing),
        Honor(notify::Honor),
        Title(notify::Title),
    }

    mod notify {
        use super::*;

        #[derive(Serialize, Deserialize, Debug)]
        pub struct Poke {
            pub time: i64,
            pub self_id: i64,
            pub group_id: i64,
            pub user_id: i64,
            pub target_id: i64,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct LuckyKing {
            pub time: i64,
            pub self_id: i64,
            pub group_id: i64,
            pub user_id: i64,
            pub target_id: i64,
        }

        #[derive(Serialize, Deserialize, Debug)]
        #[serde(rename_all = "snake_case")]
        pub enum HonorType {
            Talkative,
            Performer,
            Emotion,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct Honor {
            pub time: i64,
            pub self_id: i64,
            pub group_id: i64,
            pub honor_type: HonorType,
            pub user_id: i64,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct Title {
            pub time: i64,
            pub self_id: i64,
            pub group_id: i64,
            pub user_id: i64,
            pub title: String,
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct GroupCard {
        pub time: i64,
        pub self_id: i64,
        pub group_id: i64,
        pub user_id: i64,
        pub card_new: String,
        pub card_old: String,
    }
}

pub mod request {
    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "request_type", rename_all = "snake_case")]
    pub enum Request {
        Friend(Friend),
        Group(Group),
    }

    impl MessageType for Request {
        fn get_target(&self) -> Target {
            match self {
                Request::Friend(n) => Target::Private(n.user_id),
                Request::Group(n) => Target::Group(n.group_id),
            }
        }

        fn get_type(&self) -> Type {
            Type::Request
        }

        fn get_text(&self) -> String {
            String::new()
        }

        fn get_sender(&self) -> Sender {
            match self {
                Request::Friend(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
                Request::Group(n) => Sender {
                    nickname: None,
                    user_id: Some(n.user_id),
                    card: None,
                    role: None,
                },
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Friend {
        pub time: i64,
        pub self_id: i64,
        pub user_id: i64,
        pub comment: String,
        pub flag: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubType {
        Add,
        Invite,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Group {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubType,
        pub group_id: i64,
        pub user_id: i64,
        pub comment: String,
        pub flag: String,
    }
}

pub mod meta {
    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case", tag = "meta_event_type")]
    pub enum MetaEvent {
        Lifecycle(Lifecycle),
        Heartbeat(Heartbeat),
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubType {
        Enable,
        Disable,
        Connect,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Lifecycle {
        pub time: i64,
        pub self_id: i64,
        pub sub_type: SubType,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Status {
        online: bool,
        good: bool,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Heartbeat {
        pub time: i64,
        pub self_id: i64,
        pub status: Status,
        pub interval: i64,
    }
}

pub mod message_sent {
    use crate::abi::message::{MessageReceive, SenderGroup, SenderPrivate};

    use super::*;

    #[derive(Deserialize, Debug)]
    #[serde(tag = "message_type", rename_all = "snake_case")]
    pub enum MessageSent {
        Group(Group),
        Private(Private),
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum SubType {
        Friend,
        Group,
        Normal,
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum MessageFormat {
        Array,
        String,
    }

    #[derive(Deserialize, Debug)]
    pub struct Group {
        pub real_seq: Option<i64>,
        pub temp_source: Option<i64>,
        pub message_sent_type: Option<String>,
        pub target_id: Option<i64>,
        pub self_id: Option<i64>,
        pub time: i64,
        pub message_id: i64,
        pub message_seq: i64,
        pub real_id: i64,
        pub user_id: i64,
        pub group_id: Option<i64>,
        pub group_name: Option<String>,
        pub sub_type: SubType,
        pub sender: SenderGroup,
        pub message: MessageReceive,
        pub message_format: MessageFormat,
        pub raw_message: String,
        pub font: i64,
    }

    #[derive(Deserialize, Debug)]
    pub struct Private {
        pub real_seq: Option<i64>,
        pub temp_source: Option<i64>,
        pub message_sent_type: Option<String>,
        pub target_id: Option<i64>,
        pub self_id: Option<i64>,
        pub time: i64,
        pub message_id: i64,
        pub message_seq: i64,
        pub real_id: i64,
        pub user_id: i64,
        pub group_id: Option<i64>,
        pub group_name: Option<String>,
        pub sub_type: SubType,
        pub sender: SenderPrivate,
        pub message: MessageReceive,
        pub message_format: MessageFormat,
        pub raw_message: String,
        pub font: i64,
    }
}
