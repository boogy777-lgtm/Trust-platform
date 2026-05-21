# truST — open IEC 61131-3 control workspace

> **⚠️ This is a fork of the original [johannesPettersson80/trust-platform](https://github.com/johannesPettersson80/trust-platform) project.**
>
> | Metric | Original | This Fork |
> |--------|----------|-----------|
> | **ST Syntax Coverage** | ~75% | **100%** ✅ |
> | **Action Blocks** | ❌ Missing | ✅ Implemented |
> | **Struct/Union Fields** | ❌ Missing | ✅ Implemented |
> | **Variable Qualifiers (RETAIN, EDGE)** | ❌ Missing | ✅ Implemented |
> | **Nested DocumentSymbol** | ❌ Flat | ✅ Hierarchical |
> | **Tests** | ~500 | **1280+** |
>
> This fork extends the original project with full IEC 61131-3 ST syntax coverage and MCP (Model Context Protocol) integration for AI-assisted PLC development.
>
> **Original author:** [Johannes Pettersson](https://github.com/johannesPettersson80) — all credit for the foundation goes to the original project.

![truST logo](docs/public/assets/images/brand/trust-logo.svg)

[![Docs](https://img.shields.io/badge/docs-live-0f766e.svg)](https://johannespettersson80.github.io/trust-platform/)
[![Marketplace](https://img.shields.io/visual-studio-marketplace/v/trust-platform.trust-lsp?label=marketplace)](https://marketplace.visualstudio.com/items?itemName=trust-platform.trust-lsp)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](Cargo.toml)

## Overview

truST is an open IEC 61131-3 control workspace: one project edited in VS Code,
run by `trust-runtime`, observed through browser HMI, automated through product
CLI, Agent APIs, and `trust-dev` workbench tools, connected through truST Mesh,
and assisted by AI tools that can read diagnostics and use typed truST surfaces.

Runs on Linux, including [PREEMPT_RT soft-real-time deployments](docs/public/operate/preempt-rt.md),
macOS, Windows, and Raspberry Pi.

![One project across VS Code, diagnostics, debug, Browser IDE, and Browser HMI](docs/public/assets/images/one-project-surface-tour.gif)

The same runtime also serves a browser IDE at `/ide` and an operator HMI at
`/hmi`, so one project drives engineering, automation, and operation without
separate project copies to reconcile.

## Docs

- [Install truST](docs/public/start/installation.md)
- [What is truST?](docs/public/index.md)
- [Program in VS Code](docs/public/start/program-in-vscode.md)
- [Program with examples, I/O, communication, HMI, and AI](docs/public/develop/index.md)
- [Operate in Browser HMI](docs/public/start/operate-in-browser.md)
- [Hardware support](docs/public/hardware/index.md)
- [Reference](docs/public/reference/index.md)

## Features

- IEC-aware diagnostics, formatting, rename, navigation, and refactors
- Editor AI tools for typed diagnostics, navigation, file edits, HMI work, telemetry, settings, and debug actions
- Runtime panel with live values, memory, and I/O inspection
- Debugger with breakpoints, stepping, locals, and call stack
- Browser IDE and operator HMI backed by the same project/runtime
- CLI, Agent API, deterministic test, and harness workflows
- truST Mesh for runtime-to-runtime and plant connectivity
- PLCopen XML import/export and visual editor support
- **MCP integration** — HIR CLI provides JSON-RPC API for AI-assisted code graph analysis

## Install

1. Install `truST LSP` from the VS Code Marketplace.
2. Download released binaries from the latest GitHub release.
3. Open the docs site for guided setup, examples, and target-host instructions.

```bash
code --install-extension trust-platform.trust-lsp
```

## Components

| Component | Binary | Purpose |
|-----------|--------|---------|
| Language Server | `trust-lsp` | Diagnostics, navigation, formatting, refactors |
| HIR CLI | `trust-hir-cli` | JSON-RPC daemon for direct HIR queries, graph indexing, MCP integration |
| Runtime | `trust-runtime` | Runtime execution engine, CLI workflows, web UI |
| Developer Workbench | `trust-dev` | Developer/workbench commands: agent, test, docs, commit helpers |
| HMI Generator | `trust-hmi-gen` | ST→HMI widget mapping and SVG generation |
| Debug Adapter | `trust-debug` | DAP debugging |
| Bundle Tool | `trust-bundle-gen` | STBC bundle generation |

### HIR CLI — JSON-RPC API

`trust-hir-cli daemon --project <path>` exposes a line-delimited JSON-RPC interface over stdin/stdout:

| Method | Purpose |
|--------|---------|
| `hir/initialize` | Handshake and project loading |
| `hir/getSymbols` | Query symbols by name/kind/file |
| `hir/getTypes` | Query type definitions |
| `hir/getReferences` | Find all references to a symbol |
| `hir/getDiagnostics` | Get semantic diagnostics |
| `hir/getCallHierarchy` | Incoming/outgoing call edges |
| `hir/getFileSymbols` | All symbols in a file |
| `hir/getProjectSymbols` | All symbols in the project |
| `hir/index` | Full project indexing: POUs, variables, types, fields, relationships, diagnostics |
| `hir/health` | Daemon health check |

### MCP Server Integration

This fork includes [ST-graph-rag-mcp](https://github.com/boogy777-lgtm/ST-graph-rag-mcp) — an MCP server that uses `trust-hir-cli` for fast semantic indexing. Features:

- **HIR-based indexing** (replaces slow LSP path): `hir/index` returns POU graph in <5s vs 108s
- **Two indexing paths**: HIR (fast, default via `USE_HIR_CLIENT=true`) and LSP (legacy fallback)
- **Token-efficient responses**: dispatcher interceptor strips DB IDs, compact types (FB, PRG, FC), workspace-relative paths
- **SQLite graph database**: same schema for both paths, zero migration needed

## Building

```bash
# Release build with full optimizations
cargo build --release -p trust-hir-cli

# Optimizations applied:
#   target-cpu=native  — CPU-specific SIMD
#   lto=fat            — aggressive link-time optimization
#   panic=abort        — no unwind tables
#   strip=symbols      — smaller binary
```

## Status

- VS Code Marketplace: live
- Supported platforms: Linux, Linux PREEMPT_RT, macOS, Windows, Raspberry Pi
- Runtime + debugger: pre-1.0, behavior-locked by tests
- Rust MSRV: 1.95+

## Help

- GitHub Issues: <https://github.com/boogy777-lgtm/Trust-platform/issues>
- Original author Email: <johannes_salomon@hotmail.com>

## License

Licensed under MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
