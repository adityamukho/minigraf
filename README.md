# minigraf
A tiny, portable GraphQL engine. **W.I.P.**

## Purpose
This project was started to (in order):
- Learn Rust,
- Learn how to write a parser,
- Learn GraphQL,
- Possibly create a borderline useful tool in the process.

## Scope
Minigraf will be designed to run in multiple environments, including:
- As a standalone binary,
- As a library,
- As a WebAssembly module (for browsers).

## Anti-Scope
Minigraf will **NOT** be designed to be (for now):
- Distributed,
- Fault-tolerant,
- ACID-compliant.

## Features
Minigraf will support multiple backends to store its data, including:
- In-memory,
- IndexedDB (browser only),
- SQLite,
- One of more embedded KV stores (such as LevelDB or RocksDB).
