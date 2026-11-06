//! Typed value/call IR and C++ lowering. Semantic analysis never chooses scalar
//! storage: the backend recognizes trusted standard-library operations only.
mod cpp;
mod intrinsics;
pub(crate) mod ir;
