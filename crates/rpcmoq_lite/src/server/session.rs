use dashmap::DashMap;
use std::fmt;
use std::sync::Arc;

use crate::error::RpcServerError;

/// A composite key for session tracking: (client_id, grpc_path).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub client_id: String,
    pub grpc_path: String,
}

impl SessionKey {
    pub fn new(client_id: impl Into<String>, grpc_path: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            grpc_path: grpc_path.into(),
        }
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.client_id, self.grpc_path)
    }
}

/// Tracks active RPC sessions.
///
/// A session is identified by (client_id, grpc_path). Only one active session
/// is allowed per key at a time. When a session is created, a guard is returned
/// that automatically removes the session when dropped.
#[derive(Debug)]
pub struct SessionMap {
    sessions: DashMap<SessionKey, (), ahash::RandomState>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::default(),
        }
    }

    /// Try to create a new session. Returns a guard that removes the session on drop.
    ///
    /// Returns an error if a session already exists for this key.
    pub fn try_create(self: &Arc<Self>, key: SessionKey) -> Result<SessionGuard, RpcServerError> {
        use dashmap::mapref::entry::Entry;

        match self.sessions.entry(key.clone()) {
            Entry::Occupied(_) => Err(RpcServerError::SessionAlreadyActive {
                client_id: key.client_id,
                grpc_path: key.grpc_path,
            }),
            Entry::Vacant(slot) => {
                slot.insert(());
                Ok(SessionGuard {
                    key,
                    map: Arc::clone(self),
                })
            }
        }
    }

    /// Check if a session exists for the given key.
    pub fn contains(&self, key: &SessionKey) -> bool {
        self.sessions.contains_key(key)
    }

    /// Get the number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Check if there are no active sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Remove a session directly (used internally by SessionGuard).
    fn remove(&self, key: &SessionKey) {
        self.sessions.remove(key);
    }
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new()
    }
}

/// A guard that holds an active session. When dropped, the session is removed.
pub struct SessionGuard {
    key: SessionKey,
    map: Arc<SessionMap>,
}

impl SessionGuard {
    /// Get the session key.
    pub fn key(&self) -> &SessionKey {
        &self.key
    }

    /// Get the client ID.
    pub fn client_id(&self) -> &str {
        &self.key.client_id
    }

    /// Get the gRPC path.
    pub fn grpc_path(&self) -> &str {
        &self.key.grpc_path
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.map.remove(&self.key);
    }
}

impl fmt::Debug for SessionGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionGuard")
            .field("key", &self.key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_session() {
        let map = Arc::new(SessionMap::new());
        let key = SessionKey::new("drone-1", "drone.EchoService/Echo");

        let guard = map.try_create(key.clone()).unwrap();
        assert!(map.contains(&key));
        assert_eq!(map.len(), 1);

        drop(guard);
        assert!(!map.contains(&key));
        assert!(map.is_empty());
    }

    #[test]
    fn test_duplicate_session_rejected() {
        let map = Arc::new(SessionMap::new());
        let key = SessionKey::new("drone-1", "drone.EchoService/Echo");

        let _guard = map.try_create(key.clone()).unwrap();

        let result = map.try_create(key);
        assert!(matches!(
            result,
            Err(RpcServerError::SessionAlreadyActive { .. })
        ));
    }

    #[test]
    fn test_different_clients_same_rpc() {
        let map = Arc::new(SessionMap::new());
        let key1 = SessionKey::new("drone-1", "drone.EchoService/Echo");
        let key2 = SessionKey::new("drone-2", "drone.EchoService/Echo");

        let _guard1 = map.try_create(key1).unwrap();
        let _guard2 = map.try_create(key2).unwrap();

        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_same_client_different_rpcs() {
        let map = Arc::new(SessionMap::new());
        let key1 = SessionKey::new("drone-1", "drone.EchoService/Echo");
        let key2 = SessionKey::new("drone-1", "drone.CommandService/Execute");

        let _guard1 = map.try_create(key1).unwrap();
        let _guard2 = map.try_create(key2).unwrap();

        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_reconnect_after_drop() {
        let map = Arc::new(SessionMap::new());
        let key = SessionKey::new("drone-1", "drone.EchoService/Echo");

        {
            let _guard = map.try_create(key.clone()).unwrap();
        }

        // Should be able to reconnect after guard is dropped
        let _guard = map.try_create(key).unwrap();
        assert_eq!(map.len(), 1);
    }
}
