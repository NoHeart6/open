# Third-party dependencies

OpenCommander uses Rust crates listed in `apps/windows-agent/Cargo.lock`, Go modules listed in `apps/gateway/go.mod` and npm dependencies listed in `apps/mcp-server/package-lock.json`. Each dependency retains its own license and copyright notices.

The Windows package includes dependency license files under `licenses/`. `scripts/package.ps1` collects the LICENSE, LICENCE, COPYING, COPYRIGHT and NOTICE files from the locked Cargo package inventory, installed Rust toolchain and Go module inventory. Its directories include each dependency name and version. The server's npm dependencies are deployed separately and are not included in the Windows ZIP.

Screenshot encoding uses the `image` crate, `png`, `zune-core` and `zune-jpeg`. Native Windows bindings use the `windows` and `windows-sys` crates. The original notices are distributed in the corresponding dependency directories.
