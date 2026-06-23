This package directory exists only to support PyPI/`uvx` distribution.

- The product source of truth is the Rust CLI in the repository root.
- Keep Python code here limited to thin packaging and entrypoint glue.
- Do not add feature logic that only works when running from this subdirectory.
- Local development and testing should target the Rust binary and the top-level repo workflows.
