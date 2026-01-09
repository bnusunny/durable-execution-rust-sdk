//! Tests for the state module.

mod checkpoint_result_tests;
mod replay_status_tests;
mod execution_state_tests;
mod checkpoint_queue_tests;
mod batch_ordering_tests;

#[cfg(test)]
mod checkpoint_batching_property_tests;

#[cfg(test)]
mod orphan_prevention_property_tests;

#[cfg(test)]
mod execution_operation_tests;
