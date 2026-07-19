use serde::{Deserialize, Serialize};

/// A durable, public user identity shared with contacts.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct UserPeerId(pub String);

/// A transport-specific device identity, backed by a separate Iroh key.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DeviceId(pub String);
