use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use schemars::schema::{InstanceType, Schema, SchemaObject};
use schemars::{gen::SchemaGenerator, JsonSchema};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use ulid::Ulid;

#[derive(Error, Debug, Clone, Eq, PartialEq)]
#[error("invalid typed ulid: {0}")]
pub struct IdParseError(pub String);

pub struct TypedUlid<Scope> {
    inner: Ulid,
    _marker: PhantomData<Scope>,
}

impl<Scope> TypedUlid<Scope> {
    pub const fn from_u128(value: u128) -> Self {
        Self {
            inner: Ulid(value),
            _marker: PhantomData,
        }
    }

    pub fn new() -> Self {
        Self {
            inner: Ulid::new(),
            _marker: PhantomData,
        }
    }

    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        Ulid::from_string(s)
            .map(|inner| Self {
                inner,
                _marker: PhantomData,
            })
            .map_err(|error| IdParseError(error.to_string()))
    }

    pub fn as_bytes(&self) -> [u8; 16] {
        self.inner.0.to_be_bytes()
    }

    pub fn timestamp_ms(&self) -> u64 {
        (self.inner.0 >> 80) as u64
    }
}

impl<Scope> Default for TypedUlid<Scope> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Scope> Copy for TypedUlid<Scope> {}

impl<Scope> Clone for TypedUlid<Scope> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Scope> fmt::Debug for TypedUlid<Scope> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypedUlid").field(&self.to_string()).finish()
    }
}

impl<Scope> fmt::Display for TypedUlid<Scope> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl<Scope> FromStr for TypedUlid<Scope> {
    type Err = IdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl<Scope> PartialEq for TypedUlid<Scope> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<Scope> Eq for TypedUlid<Scope> {}

impl<Scope> Hash for TypedUlid<Scope> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}

impl<Scope> Serialize for TypedUlid<Scope> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, Scope> Deserialize<'de> for TypedUlid<Scope> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

impl<Scope> JsonSchema for TypedUlid<Scope> {
    fn schema_name() -> String {
        "TypedUlid".to_owned()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            format: Some("ulid".to_owned()),
            ..SchemaObject::default()
        })
    }
}

macro_rules! define_scopes {
    ($($scope:ident => $alias:ident),+ $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
            pub struct $scope;
            pub type $alias = TypedUlid<$scope>;
        )+
    };
}

define_scopes! {
    SessionScope => SessionId,
    RunScope => RunId,
    MessageScope => MessageId,
    ToolUseScope => ToolUseId,
    SubagentScope => SubagentId,
    TeamScope => TeamId,
    AgentScope => AgentId,
    TenantScope => TenantId,
    RequestScope => RequestId,
    DecisionIdScope => DecisionId,
    WorkspaceScope => WorkspaceId,
    MemoryScope => MemoryId,
    SnapshotScope => SnapshotId,
    BlobScope => BlobId,
    TransactionScope => TransactionId,
    CorrelationScope => CorrelationId,
    CausationScope => CausationId,
    EventScope => EventId,
    DeltaScope => DeltaId,
    BreakpointScope => BreakpointId,
    SteeringScope => SteeringId,
}

impl TenantId {
    pub const SINGLE: Self = Self::from_u128(1);
    pub const SHARED: Self = Self::from_u128(2);
}

#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct JournalOffset(pub u64);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ConfigHash(pub [u8; 32]);

/// ```compile_fail
/// use harness_contracts::{RunId, SessionId};
/// let session_id = SessionId::new();
/// let run_id: RunId = session_id;
/// ```
pub fn typed_ulid_compile_fail_anchor() {}
