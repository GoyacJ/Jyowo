mod chat_codec;
mod client;
mod continuation;
mod dialect;
mod error;
mod request;
mod responses_codec;
mod streaming;

pub(crate) use chat_codec::chat_messages_for_request;
pub(crate) use client::OpenAiProtocolClient;
#[allow(unused_imports)]
pub(crate) use client::OpenAiProtocolProviderExt;
pub(crate) use dialect::OpenAiChatDialect;
