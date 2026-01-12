//! Private sealed trait module for the AWS Durable Execution SDK.
//!
//! This module contains the `Sealed` trait used to prevent external implementations
//! of certain SDK traits. The sealed trait pattern ensures that only types defined
//! within this crate can implement sealed traits, allowing the SDK maintainers to
//! evolve the API without breaking external code.
//!
//! # Sealed Trait Pattern
//!
//! The sealed trait pattern works by requiring a private supertrait that external
//! crates cannot implement. Since this module is private (`pub(crate)`), external
//! crates cannot access the `Sealed` trait and therefore cannot implement any
//! trait that has `Sealed` as a supertrait.
//!
//! # Usage
//!
//! To seal a trait, add `private::Sealed` as a supertrait:
//!
//! ```rust,ignore
//! use crate::sealed::Sealed;
//!
//! pub trait MyTrait: Sealed {
//!     // trait methods
//! }
//! ```
//!
//! Then implement `Sealed` for all types that should implement `MyTrait`:
//!
//! ```rust,ignore
//! impl Sealed for MyType {}
//! ```
//!
//! # Requirements
//!
//! - 3.4: WHEN a trait is sealed, THE SDK SHALL use a private `Sealed` supertrait in a private module

/// A marker trait used to seal other traits.
///
/// This trait is intentionally empty and serves only to prevent external
/// implementations of traits that use it as a supertrait.
///
/// # Note
///
/// This trait is `pub(crate)` which means it can only be implemented
/// within this crate. External crates cannot implement this trait,
/// and therefore cannot implement any trait that requires `Sealed`.
pub(crate) trait Sealed {}
