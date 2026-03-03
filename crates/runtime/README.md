# walrus-runtime

Agent registry and hook orchestration for Walrus.

`Runtime<H: Hook>` stores agents and provides take/put semantics for
caller-driven execution. The `Hook` trait abstracts the backend (model
provider, tool dispatch, prompt enrichment, event observation).

## License

GPL-3.0
