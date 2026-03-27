//! LanceDB embedded vector store backend.
//!
//! Implements [`MemoryEmbeddingRepo`] and [`MemoryFactRepo`] using LanceDB
//! with Apache Arrow for data exchange.

mod memory;
mod memory_fact;

pub use memory::*;
pub use memory_fact::*;
