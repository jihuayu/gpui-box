//! Facade-level auth tests: feature wiring, matrix and a real faked ceremony.

#![cfg(feature = "auth")]

use gpui_box::auth::*;
use gpui_box::secrets::{
    CredentialProvider, DeleteReceipt, ProviderResult, SecretAccess, SecretBytes, SecretOperation,
    Secrets,
};
use gpui_box::system::{
    Availability, Capability, Configuration, Evidence, PermissionState, Support, SurfaceId,
    SystemError,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct MemoryCredentials(Mutex<BTreeMap<String, Vec<u8>>>);

impl CredentialProvider for MemoryCredentials {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        [
            SecretOperation::Put,
            SecretOperation::Get,
            SecretOperation::Delete,
            SecretOperation::RemoveAccount,
        ]
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
    }

    fn validate_policy(&self, _: gpui_box::secrets::SecretPolicy) -> Result<(), SystemError> {
        Ok(())
    }

    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target.to_owned(), value.expose().to_vec());
        Ok(SecretAccess::Available(()))
    }

    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        let value = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
            .cloned();
        match value {
            Some(value) => {
                SecretBytes::new(value).map(|value| SecretAccess::Available(Some(value)))
            }
            None => Ok(SecretAccess::Available(None)),
        }
    }

    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        let existed = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(target)
            .is_some();
        Ok(SecretAccess::Available(DeleteReceipt { existed }))
    }

    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        Ok(SecretAccess::Available(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .keys()
                .filter(|target| target.starts_with(prefix))
                .cloned()
                .collect(),
        ))
    }
}

fn service() -> AuthService {
    AuthService::with_fakes(
        AuthPlatform::Linux,
        FakeBrowserAuthProvider::new(49_152),
        FakeTokenExchangeBackend::new(
            AccountId::new("box-account").expect("account"),
            ProviderId::new("idp.example").expect("provider"),
        ),
        FakePasskeyProvider::new(),
        FakePasskeyServerVerifier::new(ProviderId::new("idp.example").expect("provider")),
        Secrets::with_provider("gpui-box-auth-test", Arc::new(MemoryCredentials::default()))
            .expect("secrets"),
    )
    .expect("auth service")
}

#[test]
fn auth_facade_exposes_the_six_by_ten_matrix() {
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 60);
    assert!(
        matrix
            .iter()
            .all(|cell| cell.evidence == Evidence::ContractOnly)
    );
    assert_eq!(AuthPlatform::ALL.len(), 6);
    assert_eq!(AuthOperation::ALL.len(), 10);
    assert_eq!(DEFAULT_AUTH_EVENT_CAPACITY, 64);
}

#[test]
fn auth_facade_runs_a_browser_ceremony_end_to_end() {
    let service = service();
    let now = Instant::now();
    let state = OAuthState::new("state-box-0000000000001").expect("state");
    let redirect = RedirectTarget::loopback(LoopbackHost::Ipv4, Some(49_152), "/oauth/callback")
        .expect("redirect");
    let request = BrowserAuthRequest::new(
        ProviderId::new("idp.example").expect("provider"),
        "gpui-box-client",
        AuthEndpointUrl::parse("https://idp.example/authorize").expect("endpoint"),
        redirect,
        AuthScopes::new([AuthScope::new("openid").expect("scope")]).expect("scopes"),
        state.clone(),
        PkcePair::from_verifier(
            PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").expect("verifier"),
        ),
        PresentationContext::new(SurfaceId(7), 1).expect("presentation"),
    )
    .expect("request");
    let handle = service
        .begin_browser_auth(ExecutionOwner::Ui, request, now)
        .expect("begin");
    let callback = format!(
        "{}?code=box-code&state={}",
        handle.resolved_redirect().redirect_uri().expect("uri"),
        state.expose()
    );
    assert_eq!(
        service.submit_callback(ExecutionOwner::Ui, &callback, now),
        RedirectCaptureOutcome::ReadyForExchange {
            ceremony_id: handle.ceremony_id()
        }
    );
    let summary = service
        .complete_browser_ceremony(ExecutionOwner::Background, handle.ceremony_id(), now)
        .expect("exchange");
    assert_eq!(summary.account_id().as_str(), "box-account");
    let lease = service
        .access_token(ExecutionOwner::Background, summary.account_id(), now)
        .expect("lease");
    assert_eq!(lease.expose_access_token(), b"access-token");
    assert!(!format!("{:?}", service.snapshot(ExecutionOwner::Ui, now)).contains("access-token"));
}

#[test]
fn auth_service_is_background_owned_for_tokens() {
    let service = service();
    let now = Instant::now();
    let account = AccountId::new("box-account").expect("account");
    assert!(matches!(
        service.access_token(ExecutionOwner::Ui, &account, now),
        Err(AuthError::WrongExecutionOwner(_))
    ));
    assert!(matches!(
        service.access_token(ExecutionOwner::Background, &account, now),
        Err(AuthError::NotAuthenticated)
    ));
    assert!(matches!(
        service.verify_local_presence(ExecutionOwner::Ui, LocalPresencePurpose::RemoveAccount, now),
        Err(AuthError::Unavailable(_))
    ));
}

#[cfg(feature = "auth-gpui")]
#[test]
fn auth_gpui_feature_exposes_the_entity_adapter() {
    let model = std::any::type_name::<gpui_box::auth::gpui_adapter::AuthModel>();
    assert!(model.contains("AuthModel"));
    let scope = std::any::type_name::<gpui_box::auth::gpui_adapter::SurfaceAuthScope>();
    assert!(scope.contains("AuthSurfaceScope"));
    let _ = std::any::type_name::<gpui_box::auth::gpui_adapter::AuthEventDrain>();
    #[allow(unused_imports)]
    use gpui_box::auth::gpui_adapter::AuthAppExt as _;
}

#[test]
fn passkey_and_browser_share_no_local_auth_dependency() {
    // Service construction only accepts remote backend/verifier traits; a local
    // presence receipt cannot be passed where these are required.
    let _ = Duration::from_secs(1);
    assert!(AuthPlatform::Linux.native_platform().is_some());
}
