use pretty_assertions::assert_eq;
use tokio::sync::Semaphore;

use super::ConcurrentRequestLimit;
use super::RequestDispatchMode;

/// Public limits reject values that cannot safely enable semaphore-backed concurrency.
#[test]
fn concurrent_request_limit_rejects_invalid_values() {
    assert_eq!(
        ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 0),
        None
    );
    assert_eq!(
        ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 1),
        None
    );
    assert_eq!(
        ConcurrentRequestLimit::new(Semaphore::MAX_PERMITS.saturating_add(1)),
        None
    );
    assert_eq!(
        ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 2).map(ConcurrentRequestLimit::get),
        Some(2)
    );
}

/// CLI parsing keeps one request inline and bounds larger positive concurrency limits.
#[test]
fn request_dispatch_mode_parses_bounded_concurrency() {
    assert!(matches!("1".parse(), Ok(RequestDispatchMode::Inline)));
    assert!("0".parse::<RequestDispatchMode>().is_err());

    let oversized_limit = Semaphore::MAX_PERMITS.saturating_add(1).to_string();
    let mode = oversized_limit
        .parse::<RequestDispatchMode>()
        .expect("parse oversized concurrent request limit");
    let RequestDispatchMode::Concurrent {
        max_concurrent_requests,
    } = mode
    else {
        panic!("expected concurrent request dispatch");
    };
    assert_eq!(max_concurrent_requests.get(), Semaphore::MAX_PERMITS);
}
