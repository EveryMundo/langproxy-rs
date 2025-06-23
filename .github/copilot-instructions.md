<!-- Copyright (c) 2025 PROS Inc. -->
<!-- All rights reserved. -->

# Rust CloudFlare Workers Performance Best Practices

## Project Context
This repository contains Rust code for CloudFlare Workers, a serverless execution environment. Performance is critical in this environment as execution time directly impacts costs and user experience.

## Coding Standards

### Performance-Focused Rust Best Practices

1. **Avoid Unnecessary Allocations**
   - Use references (`&str`) instead of owned types (`String`) when possible
   - Consider using `Cow<'a, str>` for conditionally owned data
   - Reuse allocations when processing data in loops
   - Use stack allocation for small, fixed-size data with arrays instead of Vec

2. **Zero-Cost Abstractions**
   - Prefer iterators over explicit loops when appropriate
   - Use iterator combinators (`map`, `filter`, etc.) which often optimize better
   - Leverage Rust's type system to enforce constraints at compile time

3. **Minimize Runtime Overhead**
   - Use `#[inline]` for small, frequently called functions
   - Consider `#[cold]` for error handling paths
   - Avoid deep recursion in favor of iteration
   - Use const generics and const functions where applicable

4. **Memory Management**
   - Use custom allocators for specialized memory patterns
   - Consider arena allocation for groups of objects with the same lifetime
   - Avoid unnecessary `Clone` operations
   - Use `Box<T>` for large stack objects

5. **Asynchronous Programming**
   - Use `async`/`await` efficiently
   - Avoid blocking operations in async contexts
   - Consider using `FuturesUnordered` for parallel task execution
   - Minimize `.await` points to reduce context switching

6. **Optimized Data Structures**
   - Choose appropriate collections (`Vec`, `HashMap`, etc.) based on access patterns
   - Use `SmallVec` or similar for collections that are usually small
   - Consider specialized data structures for specific use cases
   - Use `BTreeMap` instead of `HashMap` for ordered keys or when memory overhead matters

7. **Error Handling**
   - Use `Result<T, E>` with appropriate error types
   - Avoid panicking in production code paths
   - Use `?` operator for concise error propagation
   - Consider using `anyhow` for application code and `thiserror` for libraries

8. **Compilation and Build Optimizations**
   - Use appropriate optimization levels (`opt-level`)
   - Consider Link-Time Optimization (LTO) for release builds
   - Use conditional compilation with `cfg` attributes to avoid unnecessary code
   - Leverage `cargo-bloat` to identify large dependencies

9. **CloudFlare Workers Specific**
   - Minimize cold starts by keeping initialization code minimal
   - Cache expensive computations where appropriate
   - Use appropriate storage options (KV, Durable Objects) based on access patterns
   - Be mindful of CPU time limits in the Workers environment

10. **Testing and Benchmarking**
    - Write benchmarks for performance-critical code paths
    - Use criterion.rs for reliable benchmarking
    - Profile code regularly to identify bottlenecks
    - Test with realistic workloads

## Code Examples

```rust
// Bad: Unnecessary allocation
fn process_data(input: String) -> String {
    input.to_uppercase()
}

// Good: Avoid allocation with references
fn process_data(input: &str) -> String {
    input.to_uppercase()
}

// Better: Return a Cow to avoid allocation when possible
use std::borrow::Cow;
fn process_data<'a>(input: &'a str) -> Cow<'a, str> {
    if input.chars().any(|c| c.is_lowercase()) {
        Cow::Owned(input.to_uppercase())
    } else {
        Cow::Borrowed(input)
    }
}
```

## Preferred Libraries
- `serde` for serialization/deserialization
- `rayon` for parallel processing
- `tokio` for async runtime
- `futures` for async utilities
- `bytes` for efficient byte buffer manipulation
- `smallvec` for stack-based small vectors
