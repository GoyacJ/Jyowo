mod chat_codec;
mod client;
mod completions_codec;
mod continuation;
mod dialect;
mod error;
mod request;
mod responses_codec;
mod streaming;

#[allow(unused_imports)]
pub(crate) use chat_codec::chat_messages_for_request;
#[allow(unused_imports)]
pub(crate) use chat_codec::deepseek_chat_prefix_requested;
pub(crate) use client::OpenAiProtocolClient;
#[allow(unused_imports)]
pub(crate) use client::OpenAiProtocolProviderExt;
#[allow(unused_imports)]
pub(crate) use dialect::OpenAiChatDialect;
