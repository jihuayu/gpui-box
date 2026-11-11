//! Screen-owned, account-partitioned cache. Applications choose their executor/provider.
use gpui_system::{CancelReason, CancellationToken, SystemError};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u64);

pub struct QueryRequest {
    id: RequestId,
    owner: Weak<()>,
    cancellation: CancellationToken,
}
impl QueryRequest {
    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}
impl Drop for QueryRequest {
    fn drop(&mut self) {
        self.cancellation.cancel(CancelReason::Caller);
    }
}

pub struct QueryState<T> {
    pub value: Option<T>,
    pub error: Option<SystemError>,
    revision: u64,
    active: Option<(RequestId, CancellationToken)>,
}
impl<T> Default for QueryState<T> {
    fn default() -> Self {
        Self {
            value: None,
            error: None,
            revision: 0,
            active: None,
        }
    }
}
impl<T> QueryState<T> {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn pending(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|(_, token)| token.reason().is_none())
    }
    fn cancel(&mut self) {
        if let Some((_, token)) = self.active.take() {
            token.cancel(CancelReason::OwnerDisposed);
        }
    }
}

/// One client per app/model. Account changes discard all cached values and in-flight work.
pub struct QueryClient<K: Ord, T> {
    account: Option<String>,
    entries: BTreeMap<K, QueryState<T>>,
    next_id: u64,
    epoch: u64,
    owner: Rc<()>,
}
impl<K: Ord, T> Default for QueryClient<K, T> {
    fn default() -> Self {
        Self {
            account: None,
            entries: BTreeMap::new(),
            next_id: 0,
            epoch: 0,
            owner: Rc::new(()),
        }
    }
}
impl<K: Ord + Clone, T> QueryClient<K, T> {
    pub fn set_account(&mut self, account: Option<String>) {
        if self.account == account {
            return;
        }
        self.clear();
        self.account = account;
    }
    pub fn clear(&mut self) {
        for entry in self.entries.values_mut() {
            entry.cancel();
        }
        self.entries.clear();
        self.epoch += 1;
    }
    pub fn state(&self, key: &K) -> Option<&QueryState<T>> {
        self.entries.get(key)
    }
    pub fn start(&mut self, key: K) -> QueryRequest {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("query request counter exhausted");
        let id = RequestId(self.next_id);
        let entry = self.entries.entry(key).or_default();
        entry.cancel();
        let cancellation = CancellationToken::default();
        entry.active = Some((id, cancellation.clone()));
        entry.error = None;
        QueryRequest {
            id,
            owner: Rc::downgrade(&self.owner),
            cancellation,
        }
    }
    pub fn complete(
        &mut self,
        key: &K,
        request: &QueryRequest,
        result: Result<T, SystemError>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        if !request.owner.ptr_eq(&Rc::downgrade(&self.owner))
            || request.cancellation.reason().is_some()
            || entry.active.as_ref().map(|a| a.0) != Some(request.id)
        {
            return false;
        }
        entry.active = None;
        entry.revision += 1;
        match result {
            Ok(value) => {
                entry.value = Some(value);
                entry.error = None;
            }
            Err(error) => entry.error = Some(error),
        }
        true
    }
    pub fn invalidate(&mut self, key: &K) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.cancel();
            entry.revision += 1;
        }
    }
    /// Commit a mutation only if its base revision is current. Refetch on conflict.
    pub fn mutation(&self, key: K) -> Mutation<K> {
        Mutation {
            revision: self.entries.get(&key).map_or(0, |entry| entry.revision),
            key,
            epoch: self.epoch,
            owner: Rc::downgrade(&self.owner),
        }
    }
    pub fn reconcile(&mut self, mutation: &Mutation<K>, value: T) -> bool {
        if mutation.epoch != self.epoch || !mutation.owner.ptr_eq(&Rc::downgrade(&self.owner)) {
            return false;
        }
        let entry = self.entries.entry(mutation.key.clone()).or_default();
        if entry.revision != mutation.revision {
            return false;
        }
        entry.cancel();
        entry.value = Some(value);
        entry.error = None;
        entry.revision += 1;
        true
    }
}
impl<K: Ord, T> Drop for QueryClient<K, T> {
    fn drop(&mut self) {
        for entry in self.entries.values_mut() {
            entry.cancel();
        }
    }
}

/// A mutation captures account, client and data revision. Conflicts require refetch.
pub struct Mutation<K> {
    key: K,
    revision: u64,
    epoch: u64,
    owner: Weak<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_system::ErrorCode;

    #[test]
    fn cancellation_stale_duplicates_and_refresh_errors_preserve_authority() {
        let mut client = QueryClient::default();
        let first = client.start("items");
        let second = client.start("items");
        assert_eq!(
            first.cancellation().reason(),
            Some(CancelReason::OwnerDisposed)
        );
        assert!(!client.complete(&"items", &first, Ok(1)));
        assert!(client.complete(&"items", &second, Ok(2)));
        assert!(!client.complete(&"items", &second, Ok(3)));
        let refresh = client.start("items");
        assert!(client.complete(
            &"items",
            &refresh,
            Err(SystemError::new(ErrorCode::Unavailable, "offline"))
        ));
        assert_eq!(client.state(&"items").unwrap().value, Some(2));
        assert!(client.state(&"items").unwrap().error.is_some());
        let abandoned = client.start("items");
        let token = abandoned.cancellation();
        drop(abandoned);
        assert_eq!(token.reason(), Some(CancelReason::Caller));
        client.invalidate(&"items");
        assert!(!client.state(&"items").unwrap().pending());
    }

    #[test]
    fn logout_and_mutation_conflicts_reject_obsolete_data() {
        let mut client = QueryClient::default();
        client.set_account(Some("alice".into()));
        let read = client.start("items");
        let mutation = client.mutation("items");
        assert!(client.reconcile(&mutation, 4));
        assert!(!client.complete(&"items", &read, Ok(1)));
        assert!(!client.reconcile(&mutation, 5));
        let obsolete = client.start("items");
        client.set_account(None);
        assert!(client.state(&"items").is_none());
        client.set_account(Some("bob".into()));
        assert!(!client.reconcile(&mutation, 5));
        let current = client.start("items");
        assert!(!client.complete(&"items", &obsolete, Ok(8)));
        assert!(client.complete(&"items", &current, Ok(9)));
        let active = client.start("items").cancellation();
        drop(client);
        assert!(active.reason().is_some());
    }
}
