//! Core-owned hosted-context review state.
//!
//! No requesting client receives a ticket, handle, or approval field. The core
//! retains the exact outbound scope, asks the desktop session only to launch the
//! fixed broker, and waits for a decision posted through the broker's dedicated
//! setgid capability socket. No review id or authority crosses the user-owned
//! bridge: the fixed broker atomically claims the sole pending review bound to
//! its kernel-authenticated desktop UID over the protected socket. The bridge
//! can delay or suppress launch, but cannot forge a
//! retry or post a decision. Same-UID accessibility automation remains outside
//! this boundary and is not claimed to be prevented here.

use std::{
    collections::BTreeMap,
    sync::{Condvar, Mutex, OnceLock, TryLockError},
    time::{Duration, Instant},
};

use axum::{extract::Extension, http::StatusCode, Json};
use rand::{rngs::OsRng, RngCore as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const LEASE_ID_BYTES: usize = 32;
const LEASE_ID_HEX_CHARS: usize = LEASE_ID_BYTES * 2;
const REVIEW_TTL: Duration = Duration::from_secs(310);
const REVIEW_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_PENDING_REVIEWS: usize = 8;
const MAX_SHORT_FIELD_CHARS: usize = 512;
const MAX_EXACT_CONTENT_CHARS: usize = 48 * 1024;

#[derive(Clone, Serialize)]
pub(crate) struct HostedContextReview {
    pub(crate) destination: String,
    pub(crate) action: String,
    pub(crate) context: String,
    pub(crate) content_label: String,
    pub(crate) exact_content: String,
    pub(crate) consequence: String,
}

impl HostedContextReview {
    fn valid(&self) -> bool {
        valid_short_field(&self.destination)
            && valid_short_field(&self.action)
            && valid_short_field(&self.context)
            && valid_short_field(&self.content_label)
            && valid_short_field(&self.consequence)
            && !self.exact_content.trim().is_empty()
            && self.exact_content.chars().count() <= MAX_EXACT_CONTENT_CHARS
    }
}

fn valid_short_field(value: &str) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= MAX_SHORT_FIELD_CHARS
        && !value.chars().any(|character| character == '\0')
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HostedContextScope {
    origin: String,
    action_id: String,
    request_digest: [u8; 32],
    outbound_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewState {
    Pending,
    Presented,
    Approved,
    Cancelled,
}

struct StoredReview {
    intended_user_id: u32,
    scope: HostedContextScope,
    review: HostedContextReview,
    state: ReviewState,
    expires_at: Instant,
}

struct ReviewStore {
    entries: Mutex<BTreeMap<String, StoredReview>>,
    changed: Condvar,
}

fn store() -> &'static ReviewStore {
    static STORE: OnceLock<ReviewStore> = OnceLock::new();
    STORE.get_or_init(|| ReviewStore {
        entries: Mutex::new(BTreeMap::new()),
        changed: Condvar::new(),
    })
}

fn flow_serial() -> &'static Mutex<()> {
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| Mutex::new(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostedConsentResult {
    Approved,
    Cancelled,
    Unavailable,
    TimedOut,
}

fn digest(domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
    digest.finalize().into()
}

fn request_digest(value: &[u8]) -> [u8; 32] {
    digest(b"goblins-hosted-consent-request-v1\0", value)
}

fn outbound_digest(value: &[u8]) -> [u8; 32] {
    digest(b"goblins-hosted-consent-outbound-v1\0", value)
}

fn valid_lease_id(lease_id: &str) -> bool {
    lease_id.len() == LEASE_ID_HEX_CHARS
        && lease_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn random_lease_id() -> String {
    let mut bytes = [0_u8; LEASE_ID_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let mut lease_id = String::with_capacity(LEASE_ID_HEX_CHARS);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        lease_id.push(char::from(HEX[usize::from(byte >> 4)]));
        lease_id.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    lease_id
}

fn prune(entries: &mut BTreeMap<String, StoredReview>, now: Instant) {
    entries.retain(|_, entry| entry.expires_at > now);
}

fn make_room_for_review(entries: &mut BTreeMap<String, StoredReview>) {
    while entries.len() >= MAX_PENDING_REVIEWS {
        let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.expires_at)
            .map(|(lease_id, _)| lease_id.clone())
        else {
            break;
        };
        entries.remove(&oldest);
    }
}

fn insert_review(
    intended_user_id: u32,
    scope: HostedContextScope,
    review: HostedContextReview,
    now: Instant,
) -> String {
    let mut entries = store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut entries, now);
    make_room_for_review(&mut entries);
    loop {
        let lease_id = random_lease_id();
        if !entries.contains_key(&lease_id) {
            entries.insert(
                lease_id.clone(),
                StoredReview {
                    intended_user_id,
                    scope: scope.clone(),
                    review: review.clone(),
                    state: ReviewState::Pending,
                    expires_at: now + REVIEW_TTL,
                },
            );
            return lease_id;
        }
    }
}

fn remove_review(lease_id: &str) {
    store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(lease_id);
}

pub(crate) fn request_hosted_context_consent(
    client: crate::control_plane::RequestClient,
    action_id: &str,
    request_binding: &[u8],
    outbound_binding: &[u8],
    review: HostedContextReview,
) -> HostedConsentResult {
    if action_id.trim().is_empty() || !review.valid() {
        return HostedConsentResult::Unavailable;
    }

    // A single trusted review surface is visible at a time. This lock is held
    // through the core-owned wait, so even a compromised requester cannot flood
    // overlapping dialogs through concurrent capability requests.
    let _serial = match flow_serial().try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return HostedConsentResult::Unavailable,
    };
    let scope = HostedContextScope {
        origin: client.id().to_string(),
        action_id: action_id.to_string(),
        request_digest: request_digest(request_binding),
        outbound_digest: outbound_digest(outbound_binding),
    };
    let lease_id = insert_review(client.user_id(), scope.clone(), review, Instant::now());
    if !matches!(
        crate::session_bridge::launch_hosted_consent_broker(),
        crate::session_bridge::SessionBridgeResult::Success(ref marker) if marker == "launched"
    ) {
        remove_review(&lease_id);
        return HostedConsentResult::Unavailable;
    }

    wait_for_decision_without_blocking_runtime(&lease_id, &scope, REVIEW_WAIT_TIMEOUT)
}

/// Yield the Tokio worker before entering the synchronous condition-variable
/// wait. Keeping this invariant local to the consent primitive prevents an
/// async caller from starving the broker's claim and decision routes.
fn wait_for_decision_without_blocking_runtime(
    lease_id: &str,
    expected_scope: &HostedContextScope,
    timeout: Duration,
) -> HostedConsentResult {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| wait_for_decision(lease_id, expected_scope, timeout))
        }
        Ok(_) => {
            // `block_in_place` panics on a current-thread runtime, while a
            // direct condition-variable wait would starve its broker routes.
            // Production handlers use the bounded blocking pool; any future
            // direct current-thread caller fails closed and leaves no review.
            remove_review(lease_id);
            HostedConsentResult::Unavailable
        }
        Err(_) => wait_for_decision(lease_id, expected_scope, timeout),
    }
}

fn wait_for_decision(
    lease_id: &str,
    expected_scope: &HostedContextScope,
    timeout: Duration,
) -> HostedConsentResult {
    let deadline = Instant::now() + timeout;
    let mut entries = store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        let result = match entries.get(lease_id) {
            Some(entry) if entry.scope != *expected_scope => Some(HostedConsentResult::Unavailable),
            Some(entry) if entry.expires_at <= Instant::now() => {
                Some(HostedConsentResult::TimedOut)
            }
            Some(entry) if entry.state == ReviewState::Approved => {
                Some(HostedConsentResult::Approved)
            }
            Some(entry) if entry.state == ReviewState::Cancelled => {
                Some(HostedConsentResult::Cancelled)
            }
            Some(_) => None,
            None => Some(HostedConsentResult::Unavailable),
        };
        if let Some(result) = result {
            entries.remove(lease_id);
            return result;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            entries.remove(lease_id);
            return HostedConsentResult::TimedOut;
        };
        let (next_entries, wait) = store()
            .changed
            .wait_timeout(entries, remaining)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries = next_entries;
        if wait.timed_out() {
            entries.remove(lease_id);
            return HostedConsentResult::TimedOut;
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerReviewRequest {}

#[derive(Serialize)]
pub(crate) struct BrokerReviewResponse {
    ok: bool,
    text: &'static str,
    lease_id: Option<String>,
    review: Option<HostedContextReview>,
}

pub(crate) async fn broker_fetch_review(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(BrokerReviewRequest {}): Json<BrokerReviewRequest>,
) -> (StatusCode, Json<BrokerReviewResponse>) {
    let Some((lease_id, review)) = claim_for_broker(client.user_id(), Instant::now()) else {
        return (
            StatusCode::GONE,
            Json(BrokerReviewResponse {
                ok: false,
                text: "This hosted-context review is unavailable or expired.",
                lease_id: None,
                review: None,
            }),
        );
    };
    (
        StatusCode::OK,
        Json(BrokerReviewResponse {
            ok: true,
            text: "Review the exact hosted context.",
            lease_id: Some(lease_id),
            review: Some(review),
        }),
    )
}

fn claim_for_broker(broker_user_id: u32, now: Instant) -> Option<(String, HostedContextReview)> {
    let mut entries = store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut entries, now);
    let mut pending = entries
        .iter()
        .filter(|(_, entry)| {
            entry.state == ReviewState::Pending
                && entry.expires_at > now
                && entry.intended_user_id == broker_user_id
        })
        .map(|(lease_id, _)| lease_id.clone());
    let lease_id = pending.next()?;
    if pending.next().is_some() {
        return None;
    }
    let entry = entries.get_mut(&lease_id)?;
    entry.state = ReviewState::Presented;
    Some((lease_id, entry.review.clone()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerDecisionRequest {
    lease_id: String,
    decision: String,
}

#[derive(Serialize)]
pub(crate) struct BrokerDecisionResponse {
    ok: bool,
    text: &'static str,
}

pub(crate) async fn broker_submit_decision(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(request): Json<BrokerDecisionRequest>,
) -> (StatusCode, Json<BrokerDecisionResponse>) {
    let (state, allow_pending) = match request.decision.as_str() {
        "approve" => (ReviewState::Approved, false),
        "cancel" => (ReviewState::Cancelled, false),
        "abort" => (ReviewState::Cancelled, true),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(BrokerDecisionResponse {
                    ok: false,
                    text: "The hosted-context decision is invalid.",
                }),
            )
        }
    };
    if !decide_from_broker(
        &request.lease_id,
        client.user_id(),
        state,
        allow_pending,
        Instant::now(),
    ) {
        return (
            StatusCode::GONE,
            Json(BrokerDecisionResponse {
                ok: false,
                text: "This hosted-context review is unavailable or already decided.",
            }),
        );
    }
    (
        StatusCode::OK,
        Json(BrokerDecisionResponse {
            ok: true,
            text: if state == ReviewState::Approved {
                "Approved once."
            } else {
                "Cancelled without sharing."
            },
        }),
    )
}

fn decide_from_broker(
    lease_id: &str,
    broker_user_id: u32,
    state: ReviewState,
    allow_pending_cancel: bool,
    now: Instant,
) -> bool {
    if !valid_lease_id(lease_id) || !matches!(state, ReviewState::Approved | ReviewState::Cancelled)
    {
        return false;
    }
    let mut entries = store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut entries, now);
    let Some(entry) = entries.get_mut(lease_id) else {
        return false;
    };
    let state_allows_decision = entry.state == ReviewState::Presented
        || (allow_pending_cancel
            && state == ReviewState::Cancelled
            && entry.state == ReviewState::Pending);
    if entry.intended_user_id != broker_user_id || !state_allows_decision || entry.expires_at <= now
    {
        return false;
    }
    entry.state = state;
    store().changed.notify_all();
    true
}

#[cfg(test)]
mod tests {
    use super::{
        broker_fetch_review, broker_submit_decision, claim_for_broker, decide_from_broker,
        flow_serial, insert_review, request_digest, request_hosted_context_consent, store,
        wait_for_decision, wait_for_decision_without_blocking_runtime, BrokerDecisionRequest,
        BrokerReviewRequest, HostedConsentResult, HostedContextReview, HostedContextScope,
        ReviewState,
    };
    use std::{
        sync::{Mutex, MutexGuard, OnceLock},
        thread,
        time::Duration,
        time::Instant,
    };

    const USER_A: u32 = 1_000;
    const USER_B: u32 = 1_001;

    struct StoreTestGuard {
        _serial: MutexGuard<'static, ()>,
    }

    impl Drop for StoreTestGuard {
        fn drop(&mut self) {
            store()
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
            store().changed.notify_all();
        }
    }

    fn isolate_store() -> StoreTestGuard {
        static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
        let serial = SERIAL
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        StoreTestGuard { _serial: serial }
    }

    fn review() -> HostedContextReview {
        HostedContextReview {
            destination: "your OpenAI account through Codex".to_string(),
            action: "Ask Goblins AI about selected text".to_string(),
            context: "the exact selected text".to_string(),
            content_label: "Exact content leaving this device".to_string(),
            exact_content: "the retained outbound prompt".to_string(),
            consequence: "This exact content will leave this device once.".to_string(),
        }
    }

    fn scope() -> HostedContextScope {
        HostedContextScope {
            origin: "launcher".to_string(),
            action_id: "ai.selected-text-context".to_string(),
            request_digest: request_digest(b"request"),
            outbound_digest: super::outbound_digest(b"outbound"),
        }
    }

    #[test]
    fn only_presented_broker_review_can_decide_once() {
        let _isolation = isolate_store();
        let id = insert_review(USER_A, scope(), review(), Instant::now());
        assert!(!decide_from_broker(
            &id,
            USER_A,
            ReviewState::Approved,
            false,
            Instant::now()
        ));
        let (claimed_id, fetched) =
            claim_for_broker(USER_A, Instant::now()).expect("broker review");
        assert_eq!(claimed_id, id);
        assert_eq!(fetched.exact_content, "the retained outbound prompt");
        assert!(claim_for_broker(USER_A, Instant::now()).is_none());
        assert!(decide_from_broker(
            &id,
            USER_A,
            ReviewState::Approved,
            false,
            Instant::now()
        ));
        assert!(!decide_from_broker(
            &id,
            USER_A,
            ReviewState::Cancelled,
            false,
            Instant::now()
        ));
        super::remove_review(&id);
    }

    #[test]
    fn waiter_requires_matching_internal_scope_and_consumes_approval() {
        let _isolation = isolate_store();
        let expected = scope();
        let id = insert_review(USER_A, expected.clone(), review(), Instant::now());
        assert_eq!(
            claim_for_broker(USER_A, Instant::now()).map(|claim| claim.0),
            Some(id.clone())
        );
        let decision_id = id.clone();
        thread::spawn(move || {
            assert!(decide_from_broker(
                &decision_id,
                USER_A,
                ReviewState::Approved,
                false,
                Instant::now()
            ));
        })
        .join()
        .expect("decision thread");
        assert_eq!(
            wait_for_decision(&id, &expected, Duration::from_secs(1)),
            HostedConsentResult::Approved
        );
        assert!(!store()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&id));

        let id = insert_review(USER_A, expected.clone(), review(), Instant::now());
        let mut wrong = expected;
        wrong.origin = "settings".to_string();
        assert_eq!(
            wait_for_decision(&id, &wrong, Duration::from_millis(1)),
            HostedConsentResult::Unavailable
        );
    }

    #[test]
    fn cancelled_and_timed_out_reviews_never_approve() {
        let _isolation = isolate_store();
        let expected = scope();
        let id = insert_review(USER_A, expected.clone(), review(), Instant::now());
        assert_eq!(
            claim_for_broker(USER_A, Instant::now()).map(|claim| claim.0),
            Some(id.clone())
        );
        assert!(decide_from_broker(
            &id,
            USER_A,
            ReviewState::Cancelled,
            false,
            Instant::now()
        ));
        assert_eq!(
            wait_for_decision(&id, &expected, Duration::from_secs(1)),
            HostedConsentResult::Cancelled
        );

        let id = insert_review(USER_A, expected.clone(), review(), Instant::now());
        assert_eq!(
            wait_for_decision(&id, &expected, Duration::from_millis(1)),
            HostedConsentResult::TimedOut
        );
    }

    #[test]
    fn pending_review_can_only_be_aborted_and_never_approved() {
        let _isolation = isolate_store();
        let id = insert_review(USER_A, scope(), review(), Instant::now());
        assert!(!decide_from_broker(
            &id,
            USER_A,
            ReviewState::Approved,
            true,
            Instant::now()
        ));
        assert!(decide_from_broker(
            &id,
            USER_A,
            ReviewState::Cancelled,
            true,
            Instant::now()
        ));
        super::remove_review(&id);
    }

    #[test]
    fn broker_claim_is_atomic_and_ambiguous_pending_state_fails_closed() {
        let _isolation = isolate_store();
        let first = insert_review(USER_A, scope(), review(), Instant::now());
        let mut second_scope = scope();
        second_scope.action_id = "ai.file-context".to_string();
        let second = insert_review(USER_A, second_scope, review(), Instant::now());
        assert!(claim_for_broker(USER_A, Instant::now()).is_none());
        super::remove_review(&first);
        let claim =
            claim_for_broker(USER_A, Instant::now()).expect("one unambiguous pending review");
        assert_eq!(claim.0, second);
        assert!(claim_for_broker(USER_A, Instant::now()).is_none());
        super::remove_review(&second);
    }

    #[test]
    fn broker_cannot_claim_or_decide_another_users_review() {
        let _isolation = isolate_store();
        let lease_id = insert_review(USER_A, scope(), review(), Instant::now());

        assert!(claim_for_broker(USER_B, Instant::now()).is_none());
        assert!(!decide_from_broker(
            &lease_id,
            USER_B,
            ReviewState::Cancelled,
            true,
            Instant::now(),
        ));
        assert_eq!(
            store()
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&lease_id)
                .map(|entry| entry.state),
            Some(ReviewState::Pending)
        );

        assert_eq!(
            claim_for_broker(USER_A, Instant::now()).map(|claim| claim.0),
            Some(lease_id.clone())
        );
        assert!(!decide_from_broker(
            &lease_id,
            USER_B,
            ReviewState::Approved,
            false,
            Instant::now(),
        ));
        assert!(decide_from_broker(
            &lease_id,
            USER_A,
            ReviewState::Cancelled,
            false,
            Instant::now(),
        ));
        super::remove_review(&lease_id);
    }

    #[test]
    fn cross_user_broker_routes_return_gone_and_preserve_intended_review() {
        let _isolation = isolate_store();
        let lease_id = insert_review(USER_A, scope(), review(), Instant::now());
        let user_a = crate::control_plane::RequestClient::for_test(
            crate::control_plane::ClientKind::ConsentBroker,
            USER_A,
        );
        let user_b = crate::control_plane::RequestClient::for_test(
            crate::control_plane::ClientKind::ConsentBroker,
            USER_B,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("Tokio runtime");

        let (status, _) = runtime.block_on(broker_fetch_review(
            axum::extract::Extension(user_b),
            axum::Json(BrokerReviewRequest {}),
        ));
        assert_eq!(status, axum::http::StatusCode::GONE);
        let (status, _) = runtime.block_on(broker_submit_decision(
            axum::extract::Extension(user_b),
            axum::Json(BrokerDecisionRequest {
                lease_id: lease_id.clone(),
                decision: "abort".to_string(),
            }),
        ));
        assert_eq!(status, axum::http::StatusCode::GONE);
        assert_eq!(
            store()
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&lease_id)
                .map(|entry| entry.state),
            Some(ReviewState::Pending)
        );

        let (status, _) = runtime.block_on(broker_fetch_review(
            axum::extract::Extension(user_a),
            axum::Json(BrokerReviewRequest {}),
        ));
        assert_eq!(status, axum::http::StatusCode::OK);
        let (status, _) = runtime.block_on(broker_submit_decision(
            axum::extract::Extension(user_a),
            axum::Json(BrokerDecisionRequest {
                lease_id,
                decision: "cancel".to_string(),
            }),
        ));
        assert_eq!(status, axum::http::StatusCode::OK);
    }

    #[test]
    fn broker_routes_progress_while_hosted_request_waits() {
        let _isolation = isolate_store();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_time()
            .build()
            .expect("one-worker Tokio runtime");
        runtime.block_on(async {
            let expected = scope();
            let lease_id = insert_review(USER_A, expected.clone(), review(), Instant::now());
            let waiter_lease = lease_id.clone();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let waiter = tokio::spawn(async move {
                let _ = entered_tx.send(());
                wait_for_decision_without_blocking_runtime(
                    &waiter_lease,
                    &expected,
                    Duration::from_secs(1),
                )
            });

            entered_rx.await.expect("waiter entered");
            let decision = tokio::time::timeout(Duration::from_millis(250), async {
                let (claimed, _) =
                    claim_for_broker(USER_A, Instant::now()).expect("broker claim progressed");
                assert_eq!(claimed, lease_id);
                assert!(decide_from_broker(
                    &claimed,
                    USER_A,
                    ReviewState::Approved,
                    false,
                    Instant::now(),
                ));
            })
            .await;
            assert!(decision.is_ok(), "broker decision route was starved");
            assert_eq!(
                waiter.await.expect("waiter joined"),
                HostedConsentResult::Approved
            );
        });
    }

    #[test]
    fn bounded_blocking_hosted_wait_does_not_nested_block_in_place() {
        let _isolation = isolate_store();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_time()
            .build()
            .expect("one-worker Tokio runtime");
        runtime.block_on(async {
            let expected = scope();
            let lease_id = insert_review(USER_A, expected.clone(), review(), Instant::now());
            let waiter_lease = lease_id.clone();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let waiter = tokio::task::spawn_blocking(move || {
                let _ = entered_tx.send(());
                wait_for_decision_without_blocking_runtime(
                    &waiter_lease,
                    &expected,
                    Duration::from_secs(1),
                )
            });

            entered_rx.await.expect("blocking waiter entered");
            let (claimed, _) =
                claim_for_broker(USER_A, Instant::now()).expect("broker claim progressed");
            assert_eq!(claimed, lease_id);
            assert!(decide_from_broker(
                &claimed,
                USER_A,
                ReviewState::Approved,
                false,
                Instant::now(),
            ));
            assert_eq!(
                waiter.await.expect("blocking waiter joined without panic"),
                HostedConsentResult::Approved
            );
        });
    }

    #[test]
    fn current_thread_runtime_fails_closed_without_panic_or_residual_review() {
        let _isolation = isolate_store();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("current-thread Tokio runtime");
        let expected = scope();
        let lease_id = insert_review(USER_A, expected.clone(), review(), Instant::now());
        let result = runtime.block_on(async {
            wait_for_decision_without_blocking_runtime(
                &lease_id,
                &expected,
                Duration::from_millis(10),
            )
        });
        assert_eq!(result, HostedConsentResult::Unavailable);
        assert!(!store()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&lease_id));
    }

    #[test]
    fn second_review_fails_fast_without_queueing_or_launching() {
        let _isolation = isolate_store();
        let _active = flow_serial()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = store()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        let started = Instant::now();
        assert_eq!(
            request_hosted_context_consent(
                crate::control_plane::RequestClient::for_test(
                    crate::control_plane::ClientKind::Launcher,
                    USER_A,
                ),
                "ai.runtime",
                b"request",
                b"outbound",
                review(),
            ),
            HostedConsentResult::Unavailable
        );
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(
            store()
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            before
        );
    }
}
