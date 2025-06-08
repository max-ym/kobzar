//! Kobzar is a relational NoSQL database. It is designed to be fast, and reliable, keeping in
//! mind new trends in hardware and software. Since SQL invention, there was a substantial
//! developments in programing languages and harder challenges in data processing.
//! SQL is a great tool, but it also somewhat limits the way we work with data.
//! For example, SQL does not allow for efficient expression of ADT (Algebraic Data Types) and
//! does not allow for efficient expression of nested data structures.
//! Kobzar is written in Rust, and it uses Tokio for asynchronous processing.  
//! Query language is based on Rust and is valid Rust code, which means that you can use
//! Rust's powerful type system and borrow checker to write safe and efficient queries.
//! 
//! # Architecture
//! 
//! Kobzar is designed to be modular and extensible. It consists of several components:
//! 
//! - **Storage**: The storage layer is responsible for storing data on disk. We use
//! [marble] as a storage engine, which is a high-performance, key-value store.
//! Since it is quite low level, we have to implement caching and buffering on top of it.
//! 
//! - **Query Engine**: The query engine is responsible for executing queries on the data.
//! For efficiency, we allow to transmit queries as bytecode, which is then compiled
//! and executed by the query engine. Query engine is also responsible for
//! optimizing storage layout and indexing, and sometimes automatically infering indexes, where
//! applicable.
//! 
//! - **Network**: The network layer is responsible for handling incoming requests and
//! sending responses.
//! 
//! ## Data Model
//! 
//! Kobzar's architecture is built around a flexible data model that supports complex data structures:
//! 
//! - **Algebraic Data Types (ADTs)**: Unlike traditional SQL databases, Kobzar natively supports 
//!   sum types (enums) and product types (structs), allowing for more expressive data modeling.
//!   These types are encoded efficiently in the storage layer and can be directly queried.
//! 
//! - **Nested Data Structures**: Data can be deeply nested without performance penalties.
//!   The query engine optimizes access patterns for nested structures through path-based indexing
//!   and lazy materialization of complex objects.
//! 
//! - **Graph Model**: Kobzar implements a native graph model with first-class support for nodes,
//!   edges, and properties. Graph traversals are optimized through specialized indexes and
//!   caching strategies.
//! 
//! ## Implementation Details
//! 
//! - **Type System**: The type system is implemented as a layer above the storage engine,
//!   translating between Rust types and the binary representation in the storage layer.
//!   It also allows for types to have related metadata types that enable efficient querying
//!   for specific properties. E.g. for [String] type, we can have a metadata type
//!   that stores string hash for fast lookups.
//! 
//! - **Planner**: Queries are analyzed to determine optimal access patterns for complex
//!   data structures, with the planner accounting for nested data access costs.
//!   Query planner also can inform storage engine for changes in storage layout,
//!   such as denormalization or materialized views, to optimize for specific query patterns.
//!   It as well can suggest indexes to be created, or even create them automatically
//!   if the query is frequently used, when such feature is enabled. it also devices whether
//!   to store some BLOB data (like [String]) inline, and whether to store some metadata, like
//!   string hash, to speed up lookups.
//! 
//! - **Indexing Strategy**: Kobzar uses specialized indexes for different data structures:
//!   path indexes for nested data, graph indexes for relationship traversal, and 
//!   type-aware indexes for ADTs that optimize for variant-specific queries.
//! 
//! # Features
//! 
//! Kobzar supports the following features:
//! 
//! - **ACID transactions**: Kobzar supports ACID transactions, which means that it can
//!   guarantee that a series of operations will either all succeed or all fail.
//! 
//! - **JIT compilation**: Kobzar uses JIT compilation to compile received bytecode
//!   into machine code, which allows for fast execution of queries.

#![allow(dead_code)] // while we are still developing the code, we will have some dead code

use std::sync::OnceLock;

use tracing::info;

pub mod cfg;

/// The query engine is responsible for executing queries on the data, defining indexes,
/// calculating layouts, and optimizing storage.
pub mod q_engine;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

pub fn main() {
    let config = cfg::init();
    cfg::init_log(&config);
    info!("Starting with configuration: {config:?}");
    CFG.set(config)
        .expect("should be settable since we hadn't yet set the global config");

    tokio::runtime::Builder::new_multi_thread()
        .thread_name("worker")
        .worker_threads(cfg().file.worker_threads as _)
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime")
        .block_on(async_main());
}

pub static CFG: OnceLock<cfg::Cfg> = OnceLock::new();

fn cfg() -> &'static cfg::Cfg {
    CFG.get().expect("should be initialized by this point")
}

pub async fn async_main() {}
