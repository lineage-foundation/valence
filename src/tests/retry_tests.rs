use crate::error::ValenceError;
use crate::utils::retry_with_backoff;
use futures::lock::Mutex;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_retry_success_after_failures() {
    let call_count = Arc::new(Mutex::new(0));
    let call_count_clone = call_count.clone();

    let result = retry_with_backoff(
        "Test Operation",
        move || {
            let count = call_count_clone.clone();
            async move {
                let mut c = count.lock().await;
                *c += 1;
                if *c < 3 {
                    Err(ValenceError::Database("Transient error".to_string()))
                } else {
                    Ok("Success")
                }
            }
        },
        5,
        Duration::from_millis(1),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Success");
    assert_eq!(*call_count.lock().await, 3);
}

#[tokio::test]
async fn test_retry_failure_after_max_retries() {
    let call_count = Arc::new(Mutex::new(0));
    let call_count_clone = call_count.clone();

    // Use explicit return type to fix inference for Err-only closure
    let result = retry_with_backoff(
        "Test Operation",
        move || {
            let count = call_count_clone.clone();
            async move {
                let mut c = count.lock().await;
                *c += 1;
                let res: Result<(), ValenceError> =
                    Err(ValenceError::Database("Permanent error".to_string()));
                res
            }
        },
        3,
        Duration::from_millis(1),
    )
    .await;

    assert!(result.is_err());
    match result {
        Err(ValenceError::RetryLimitExceeded(msg)) => {
            assert!(msg.contains("Failed Test Operation after 3 retries"));
        }
        _ => panic!("Expected RetryLimitExceeded error, got {:?}", result),
    }
    assert_eq!(*call_count.lock().await, 3);
}
