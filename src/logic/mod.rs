mod classroom;
mod download;
mod echo;
mod helper;
mod llm;
mod login;
mod test;
mod timetable;

use crate::abi::logic_import::*;

pub trait BuildHelp {
    const HELP_MSG: &'static str;
}

register_handler_with_help!(
    command = [
        echo::EchoHandler,
        login::LoginHandler,
        login::LogoutHandler,
        download::DownloadHandler,
        test::TestHandler,
        test::GetTestHandler,
        test::TestAnsHandler,
        classroom::ClassHandler,
        classroom::GetClassHandler,
        timetable::TimetableHandler,
    ],
    other = [llm::LlmMessageHandler, llm::LlmNoticeHandler,]
);
