#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorPayload {
    pub code: &'static str,
    pub message: String,
}

pub(crate) fn invalid_payload(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message,
    }
}

pub(crate) fn runtime_unavailable(message: &str) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_UNAVAILABLE",
        message: message.to_owned(),
    }
}

pub(crate) fn runtime_init_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_INIT_FAILED",
        message,
    }
}

pub(crate) fn runtime_operation_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message,
    }
}

pub(crate) fn not_found(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "NOT_FOUND",
        message,
    }
}
