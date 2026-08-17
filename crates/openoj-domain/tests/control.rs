use openoj_domain::{
    AttemptState, DomainError, EvaluationState, LeaseDuration, LeaseToken, MAX_LEASE_DURATION_MS,
    MAX_UNIX_MILLIS, UnixMillis,
};

#[test]
fn terminal_evaluation_state_cannot_transition() {
    assert_eq!(
        EvaluationState::Completed.transition_to(EvaluationState::Queued),
        Err(DomainError::InvalidTransition {
            entity: "evaluation",
            from: "completed",
            to: "queued",
        })
    );
}

#[test]
fn attempt_expiry_is_one_way() {
    assert_eq!(
        AttemptState::Leased.transition_to(AttemptState::Expired),
        Ok(AttemptState::Expired)
    );
    assert!(
        AttemptState::Expired
            .transition_to(AttemptState::Queued)
            .is_err()
    );
}

#[test]
fn lease_expiry_is_checked_and_bounded() -> Result<(), DomainError> {
    let now = UnixMillis::new(MAX_UNIX_MILLIS)?;
    let duration = LeaseDuration::new(1)?;
    assert!(matches!(
        now.checked_add(duration),
        Err(DomainError::OutOfRange {
            field: "lease_expires_at_ms",
            minimum: 0,
            maximum: MAX_UNIX_MILLIS,
            ..
        })
    ));
    assert!(LeaseDuration::new(MAX_LEASE_DURATION_MS + 1).is_err());
    Ok(())
}

#[test]
fn lease_token_uses_a_distinct_bounded_type() {
    assert!(LeaseToken::parse("lease_01").is_ok());
    assert!(LeaseToken::parse("").is_err());
    assert!(LeaseToken::parse("x".repeat(65)).is_err());
}
