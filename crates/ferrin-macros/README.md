# ferrin-macros

Procedural macros for Ferrin: `#[ferrin::tool]` turns a function with one
owned input parameter (and an optional `ToolContext`) that returns
`Result<O, ToolError>` into a constructor of a `ferrin::tool::Tool`; the doc
comment becomes the tool description. Reference parameters and explicit
lifetimes are rejected at expansion time.

Use it through the `ferrin` facade (feature `macros`, on by default); the
generated code refers to `::ferrin::tool::*`. This crate has no stable API of
its own. Compile-fail cases live in `crates/ferrin/tests/ui/`.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/01-architecture/06-tool-system.md` §1.1.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
