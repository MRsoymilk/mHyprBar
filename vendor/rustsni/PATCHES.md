# Local rustsni patch

Base: `rustsni` 0.2.2 from crates.io.

mHyprBar keeps the upstream API and protocol behavior, but bounds synchronous D-Bus waits so a
non-responsive StatusNotifierItem cannot freeze the single-threaded calloop event loop:

- item property refresh (`Properties.GetAll`): 300 ms
- watcher/menu synchronous replies: 500 ms

The patch is intentionally limited to timeout selection in `src/item.rs`, `src/watcher.rs`,
`src/menu.rs`, and `src/host.rs`.
