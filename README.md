# synchronoise

fun synchronization primitives for your fun synchronization needs

[Documentation](https://docs.rs/synchronoise)

This is a collection of synchronization facilities that aren't part of the standard library that I
wanted to make sure were available for the Rust community. Right now it's just a port of
CountdownEvent and EventWaitHandle (AutoResetEvent/ManualResetEvent) from .NET, but it could be
expanded in the future.

To add this crate to your project, add the following line to your Cargo.toml:

```toml
[dependencies]
synchronoise = "0.2.0"
```

...and the following to your crate root:

```rust
extern crate synchronoise;
```

# License

synchronoise is licensed under either the MIT License or the Apache License version 2.0, at your
option. See the files `LICENSE-MIT` and `LICENSE-APACHE` for details.

<!-- vim: set tw=100 expandtab: -->
