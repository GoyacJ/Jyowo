mod chat_codec;
mod client;
mod continuation;
mod dialect;
mod error;
mod request;
mod responses_codec;
mod streaming;

pub(crate) use client::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
pub(crate) use dialect::OpenAiChatDialect;
