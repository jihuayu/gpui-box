//! Android API 35 runtime permissions on the process-owned main Looper.
use gpui_permissions::*;
use gpui_system::*;
use jni::objects::JValue;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const KINDS: [PermissionKind; 5] = [
    PermissionKind::Notifications,
    PermissionKind::Camera,
    PermissionKind::Microphone,
    PermissionKind::LocationWhenInUse,
    PermissionKind::Contacts,
];

struct Pending {
    id: RequestId,
    completion: PermissionPromptCompletion,
}

thread_local! {
    static SERVICE: PermissionService = PermissionService::new(Rc::new(AndroidPermissionProvider));
    static OWNER: UiOwner = UiOwner::new(SurfaceId(1));
    static ATTACHED: Cell<bool> = const { Cell::new(false) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    static REVISION: Cell<u64> = const { Cell::new(0) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

/// The same service is installed into GPUI's SystemServices before app launch.
pub fn permission_service() -> PermissionService {
    SERVICE.with(Clone::clone)
}

/// Obtain a host-owned context for the current attachment. Native presentation
/// additionally checks Activity resumed/focus state immediately before handoff.
/// Retain entity scopes for shorter application-owned lifetimes when needed.
pub fn permission_context() -> Result<OperationContext, SystemError> {
    if !ATTACHED.with(Cell::get) {
        return Err(native_error(-5));
    }
    Ok(OperationContext {
        ui: Some(OWNER.with(|owner| owner.context(None))?),
        ..Default::default()
    })
}

pub(super) fn attach(generation: u64) {
    if GENERATION.with(|value| value.replace(generation)) != generation {
        invalidate();
    }
    ATTACHED.with(|value| value.set(true));
}

pub(super) fn detach(generation: u64) {
    if GENERATION.with(Cell::get) == generation {
        ATTACHED.with(|value| value.set(false));
        invalidate();
    }
}

fn invalidate() {
    let _ = OWNER.with(UiOwner::advance_generation);
    let pending = PENDING.with(|slot| slot.borrow_mut().take());
    if let Some(pending) = pending {
        pending.completion.fail(native_error(-5));
    }
}

fn native_error(code: i32) -> SystemError {
    match code {
        -1 => SystemError::new(
            ErrorCode::Unsupported,
            "Android permission operation is unsupported",
        ),
        -2 => invalid_configuration(
            "Declare this permission and its purpose in the service configuration and Android manifest",
        ),
        -4 => SystemError::new(ErrorCode::Busy, "Android already owns a permission prompt"),
        -5 => SystemError::new(
            ErrorCode::ContextExpired,
            "Android permission UI owner is not foreground or has expired",
        ),
        _ => SystemError::new(
            ErrorCode::Unavailable,
            "Android permission host is unavailable",
        ),
    }
}

fn kind_id(kind: PermissionKind) -> Result<i32, SystemError> {
    KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .map(|index| index as i32)
        .ok_or_else(|| native_error(-1))
}

fn call(method: &str, signature: &str, values: &[JValue]) -> Result<i32, SystemError> {
    super::bridge::permission_call(method, signature, values).map_err(|_| native_error(-3))
}

/// Native implementation. Photos use resource-scoped picker grants, while
/// biometrics and special access need separate operation-specific providers.
pub struct AndroidPermissionProvider;

impl PermissionProvider for AndroidPermissionProvider {
    fn capability(&self, operation: PermissionOperation) -> Capability<PermissionOperation> {
        let supported = match operation {
            PermissionOperation::Query(kind) | PermissionOperation::Request(kind) => {
                kind_id(kind).is_ok()
            }
            PermissionOperation::OpenSettings(target) => {
                !matches!(target, SettingsTarget::SpecialAccess(_))
            }
        };
        if !supported {
            return unsupported_capability(operation);
        }
        let observation = operation.kind().map(|kind| self.query(kind));
        let error = observation
            .as_ref()
            .and_then(|result| result.as_ref().err());
        Capability {
            operation,
            support: Support::Native,
            availability: if error.is_some_and(|error| error.code == ErrorCode::Unavailable) {
                Availability::Unavailable
            } else {
                Availability::Available
            },
            configuration: if error
                .is_some_and(|error| error.code == ErrorCode::InvalidConfiguration)
            {
                Configuration::Required
            } else {
                Configuration::Ready
            },
            permission: observation.and_then(Result::ok).map_or(
                if operation.kind().is_some() {
                    PermissionState::Unknown
                } else {
                    PermissionState::NotRequired
                },
                |snapshot| snapshot.status.permission_state(),
            ),
            evidence: Evidence::Compiled,
            constraints: vec![
                Constraint::MinimumOsVersion("35"),
                Constraint::ForegroundOwner,
            ],
            observed_revision: REVISION.with(Cell::get),
        }
    }

    fn query(&self, kind: PermissionKind) -> Result<PermissionSnapshot, SystemError> {
        let value = call("permissionStatus", "(I)I", &[JValue::Int(kind_id(kind)?)])?;
        let status = match value {
            0 => PermissionStatus::NotDetermined,
            1 => PermissionStatus::Granted,
            2 => PermissionStatus::Denied {
                can_request_again: true,
            },
            3 => PermissionStatus::Denied {
                can_request_again: false,
            },
            4 => PermissionStatus::Limited,
            5 => PermissionStatus::Restricted,
            _ => return Err(native_error(value)),
        };
        let revision = REVISION.with(|revision| {
            let value = revision.get().saturating_add(1);
            revision.set(value);
            value
        });
        Ok(PermissionSnapshot::new(kind, status, revision))
    }

    fn request_permission(
        &self,
        prompt: PermissionPrompt,
        completion: PermissionPromptCompletion,
    ) -> Result<(), SystemError> {
        let kind = kind_id(prompt.kind)?;
        // A cancelled application request cannot cancel Android's visible system
        // dialog. Keep the native lane until its callback or attachment invalidation.
        if PENDING.with(|slot| slot.borrow().is_some()) {
            return Err(native_error(-4));
        }
        PENDING.with(|slot| {
            slot.replace(Some(Pending {
                id: prompt.request,
                completion,
            }))
        });
        let result = call(
            "requestPermission",
            "(IJ)I",
            &[JValue::Int(kind), JValue::Long(prompt.request.0 as i64)],
        );
        match result {
            Ok(1) => Ok(()),
            Ok(code) => {
                PENDING.with(|slot| slot.borrow_mut().take());
                Err(native_error(code))
            }
            Err(error) => {
                PENDING.with(|slot| slot.borrow_mut().take());
                Err(error)
            }
        }
    }

    fn open_settings(&self, target: SettingsTarget) -> Result<SettingsHandoff, SystemError> {
        let target = match target {
            SettingsTarget::AppDetails => 0,
            SettingsTarget::NotificationSettings => 1,
            SettingsTarget::SpecialAccess(_) => return Err(native_error(-1)),
        };
        match call("permissionSettings", "(I)I", &[JValue::Int(target)])? {
            1 => Ok(SettingsHandoff::Opened),
            0 => Ok(SettingsHandoff::Unavailable),
            code => Err(native_error(code)),
        }
    }

    fn tracked_kinds(&self) -> Vec<PermissionKind> {
        KINDS
            .into_iter()
            .filter(|kind| self.query(*kind).is_ok())
            .collect()
    }
}

pub(super) fn diagnostics() -> serde_json::Value {
    serde_json::Value::Array(PermissionKind::ALL.into_iter().map(|kind| {
        match AndroidPermissionProvider.query(kind) {
            Ok(snapshot) => serde_json::json!({"kind": kind.label(), "status": format!("{:?}", snapshot.status)}),
            Err(error) => serde_json::json!({"kind": kind.label(), "error": format!("{:?}", error.code), "recovery": format!("{:?}", error.recovery)}),
        }
    }).collect())
}

/// Called after Android's native result with its original unique request ID.
/// Status is freshly queried. Never trust callback grant arrays as authorization.
pub fn permission_pending(id: u64) -> bool {
    let active = permission_service().active_prompt();
    PENDING.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|pending| pending.id.0 == id && active == Some(pending.completion.kind()))
    })
}

/// Resolve one native prompt after releasing the host's prompt lane.
pub fn permission_result(id: u64, error: i32) -> bool {
    let pending = PENDING.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|pending| pending.id.0 == id) {
            slot.take()
        } else {
            None
        }
    });
    let Some(pending) = pending else { return false };
    let accepted = match if error < 0 {
        Err(native_error(error))
    } else {
        AndroidPermissionProvider.query(pending.completion.kind())
    } {
        Ok(snapshot) => pending.completion.complete(snapshot),
        Err(error) => pending.completion.fail(error),
    };
    super::bridge::post(0);
    accepted
}
