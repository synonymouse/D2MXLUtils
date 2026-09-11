use super::*;

#[test]
fn limiter_allows_ten_requests_per_window() {
    let start = Instant::now();
    let mut limiter = RequestWindow::default();

    for _ in 0..RATE_LIMIT_COUNT {
        assert_eq!(limiter.check(start), Ok(()));
    }

    assert!(limiter.check(start).is_err());
}

#[test]
fn limiter_recovers_after_window() {
    let start = Instant::now();
    let mut limiter = RequestWindow::default();

    for _ in 0..RATE_LIMIT_COUNT {
        assert_eq!(limiter.check(start), Ok(()));
    }

    assert_eq!(limiter.check(start + RATE_LIMIT_WINDOW), Ok(()));
}
