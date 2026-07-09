mod chat_codec;
mod client;
mod continuation;
mod dialect;
mod error;
mod request;
mod responses_codec;
mod streaming;

pub(crate) use chat_codec::chat_messages_for_request;
pub(crate) use client::{OpenAiProtocolClient, OpenAiProtocolProviderExt};
pub(crate) use dialect::OpenAiChatDialect;
