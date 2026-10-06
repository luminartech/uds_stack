//! The sealing trait. Private module, public trait: a bound a caller can read but not
//! implement, which is what keeps ``UDSS_LLR_0011``'s "no caller-supplied trait
//! implementation" true for every trait this crate puts in a public bound.
pub trait Sealed {}
