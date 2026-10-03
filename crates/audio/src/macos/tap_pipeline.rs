//! The system tap's sample path is the shared [`crate::pipeline::Pipeline`]
//! (moved out of here in TUR-87 so the microphone uses it too). What stays
//! is TUR-84's regression, which drives it with the tap's own rate state.

pub(crate) use crate::pipeline::Pipeline as TapPipeline;

#[cfg(test)]
mod rate_regression;
