//! Transport-neutral Judge Node worker components.

use openoj_application::{
    ApplicationError, Decision, StageContext, StageExecution, StageExecutor, evaluate,
};
use openoj_domain::{
    Capability, EvaluationRequest, EvaluationResult, ExecutorKind, NodeId, ResourceUsage, Score,
    StageKind, Verdict,
};

/// Executes one canonical Evaluation without depending on transport or storage.
pub trait JudgeExecutor {
    /// Executes a complete canonical request.
    ///
    /// # Errors
    ///
    /// Returns a typed evaluation error for unsupported capabilities or invalid output.
    fn execute(
        &mut self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError>;
}

/// Deterministic P0-C development executor that never launches processes or reads task files.
pub struct DevelopmentMockExecutor {
    node_id: NodeId,
}

impl DevelopmentMockExecutor {
    #[must_use]
    pub const fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }
}

impl JudgeExecutor for DevelopmentMockExecutor {
    fn execute(
        &mut self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError> {
        evaluate(request, self)
    }
}

impl StageExecutor for DevelopmentMockExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::DevelopmentMock
    }

    fn production_eligible(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<NodeId> {
        Some(self.node_id.clone())
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
        let decision = match Score::new(1, 1) {
            Ok(score) => Decision::new(Verdict::Accepted, score).ok(),
            Err(_) => None,
        };
        StageExecution::Succeeded {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            decision: (context.stage() == StageKind::Check)
                .then_some(decision)
                .flatten(),
        }
    }
}
