use std::error::Error;

use openoj_application::{JudgeClaim, LeasePolicy, NodePolicy, NodePolicyError};
use openoj_domain::{Capability, ClaimOperationId, LeaseDuration, LeaseToken, NodeId, UnixMillis};

fn capability(value: &str) -> Result<Capability, Box<dyn Error>> {
    Ok(Capability::parse(value)?)
}

fn node(value: &str) -> Result<NodeId, Box<dyn Error>> {
    Ok(NodeId::parse(value)?)
}

#[test]
fn lease_policy_uses_only_its_server_configured_renew_schedule() -> Result<(), Box<dyn Error>> {
    let lease_duration = LeaseDuration::new(30_000)?;
    let renew_after = LeaseDuration::new(10_000)?;

    let policy = LeasePolicy::new(lease_duration, renew_after)?;

    assert_eq!(policy.lease_duration(), lease_duration);
    assert_eq!(policy.renew_after(), renew_after);
    Ok(())
}

#[test]
fn lease_policy_rejects_a_renew_schedule_past_half_of_the_lease() -> Result<(), Box<dyn Error>> {
    let lease_duration = LeaseDuration::new(30_000)?;
    let renew_after = LeaseDuration::new(15_001)?;

    assert_eq!(
        LeasePolicy::new(lease_duration, renew_after),
        Err(NodePolicyError::InvalidRenewSchedule)
    );
    Ok(())
}

#[test]
fn node_policy_denies_a_declared_capability_missing_from_the_allowlist()
-> Result<(), Box<dyn Error>> {
    let node_id = node("judge-node-01")?;
    let policy = NodePolicy::new([(node_id.clone(), vec![capability("algorithm.batch")?])])?;

    assert_eq!(
        policy.authorize(
            &node_id,
            &[capability("algorithm.batch")?, capability("network")?]
        ),
        Err(NodePolicyError::CapabilityDenied)
    );
    Ok(())
}

#[test]
fn node_policy_defaults_to_deny_for_an_unlisted_node() -> Result<(), Box<dyn Error>> {
    let policy = NodePolicy::new([(node("judge-node-01")?, vec![capability("algorithm.batch")?])])?;

    assert_eq!(
        policy.authorize(&node("judge-node-02")?, &[capability("algorithm.batch")?]),
        Err(NodePolicyError::IdentityDenied)
    );
    Ok(())
}

#[test]
fn node_policy_rejects_duplicate_configured_capabilities() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        NodePolicy::new([(
            node("judge-node-01")?,
            vec![
                capability("algorithm.batch")?,
                capability("algorithm.batch")?
            ],
        )]),
        Err(NodePolicyError::InvalidCapabilities)
    );
    Ok(())
}

#[test]
fn node_policy_rejects_an_empty_declared_capability_set() -> Result<(), Box<dyn Error>> {
    let node_id = node("judge-node-01")?;
    let policy = NodePolicy::new([(node_id.clone(), vec![capability("algorithm.batch")?])])?;

    assert_eq!(
        policy.authorize(&node_id, &[]),
        Err(NodePolicyError::CapabilityDenied)
    );
    Ok(())
}

#[test]
fn judge_claim_carries_only_server_selected_lease_inputs() -> Result<(), Box<dyn Error>> {
    let claim = JudgeClaim {
        node_id: node("judge-node-01")?,
        declared_capabilities: vec![capability("algorithm.batch")?],
        operation_id: ClaimOperationId::parse("claim_01")?,
        lease_token: LeaseToken::parse("lease_01")?,
        now: UnixMillis::new(1_000)?,
        lease_policy: LeasePolicy::new(LeaseDuration::new(30_000)?, LeaseDuration::new(10_000)?)?,
    };

    assert_eq!(claim.lease_policy.lease_duration().value(), 30_000);
    assert_eq!(claim.lease_policy.renew_after().value(), 10_000);
    Ok(())
}
