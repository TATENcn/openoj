use openoj_judge_protocol::{
    CLIENT_PROTOCOL_VERSION, FILE_DESCRIPTOR_SET, MAX_CAPABILITIES, ProtocolContractError,
    validate_canonical_request, validate_capabilities, validate_protocol_version,
};

#[test]
fn accepts_the_exact_v0alpha1_protocol_version() {
    assert_eq!(validate_protocol_version(CLIENT_PROTOCOL_VERSION), Ok(()));
}

#[test]
fn rejects_an_unknown_protocol_version() {
    assert_eq!(
        validate_protocol_version("openoj.judge.control/v0alpha2"),
        Err(ProtocolContractError::UnsupportedVersion)
    );
}

#[test]
fn rejects_duplicate_capabilities() {
    let capabilities = vec!["algorithmic".to_owned(), "algorithmic".to_owned()];

    assert_eq!(
        validate_capabilities(&capabilities),
        Err(ProtocolContractError::InvalidCapabilities)
    );
}

#[test]
fn rejects_more_than_the_capability_limit() {
    let capabilities = (0..=MAX_CAPABILITIES)
        .map(|index| format!("capability-{index}"))
        .collect::<Vec<_>>();

    assert_eq!(
        validate_capabilities(&capabilities),
        Err(ProtocolContractError::InvalidCapabilities)
    );
}

#[test]
fn embeds_a_descriptor_for_the_v0alpha1_service_contract() {
    assert!(
        FILE_DESCRIPTOR_SET
            .windows(b"JudgeControl".len())
            .any(|window| window == b"JudgeControl")
    );
}

#[test]
fn rejects_a_malformed_canonical_request_before_dispatch() {
    assert_eq!(
        validate_canonical_request(b"not-json"),
        Err(ProtocolContractError::InvalidCanonicalPayload)
    );
}

#[test]
fn rejects_a_canonical_request_larger_than_its_transport_budget() {
    let oversized = vec![b'x'; 262_145];

    assert_eq!(
        validate_canonical_request(&oversized),
        Err(ProtocolContractError::MessageTooLarge)
    );
}
