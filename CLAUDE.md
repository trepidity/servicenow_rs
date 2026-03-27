# servicenow_rs

Rust library for the ServiceNow REST API.

## Rules of Conduct

1. **Never commit credentials** — No passwords, tokens, or secrets in code, tests, or config files. Use env vars or a gitignored `servicenow.toml`.
2. **Schema definitions are source of truth** — All table/field/relationship metadata lives in `definitions/base/*.json`. Custom schemas go in overlay files, never modify base definitions for customer-specific changes.
3. **Maintain the layered config precedence** — Builder methods > Environment variables > Config file > Defaults. Never break this order.
4. **All public API changes require tests** — No untested public methods or types.
5. **Async-first** — Core operations are async on Tokio. Sync wrappers are optional, not primary.

## Best Practices

### Rust Patterns
- Builder pattern for all complex object construction (client, queries)
- `thiserror` for error types — every error variant is actionable
- `Arc<dyn Trait>` for runtime polymorphism (auth, transport)
- Feature flags for optional capabilities
- `tracing` for structured logging, never `println!`
- Clippy-clean: `cargo clippy -- -D warnings`

### API Design
- Method chaining returns `Self` for builders
- Terminal operations (`execute`, `get`, `create`, etc.) consume the builder
- All IO operations return `Result<T>`
- Partial failures return data + errors, never silently drop either

### Testing
- Unit tests in each module (`#[cfg(test)] mod tests`)
- Integration tests use `wiremock` to mock ServiceNow responses
- Schema tests validate the bundled JSON definitions parse correctly

## Security Rules

1. **Credential handling** — Credentials are only stored in memory. BasicAuth encodes at construction time. No logging of auth headers or tokens (even at trace level).
2. **HTTPS enforced** — URL normalization always produces `https://`. The library should refuse `http://` in production (allow override for testing only).
3. **Input sanitization** — Query values are encoded before being sent. No raw string interpolation into URLs or query strings.
4. **No credential persistence** — The library never writes credentials to disk. Config file support reads only.
5. **Dependency audit** — Run `cargo audit` regularly. Pin major versions of security-sensitive deps (reqwest, base64).
6. **Session cookies** — When session reuse is enabled, cookies are managed by reqwest's cookie store, not manually. The library doesn't expose raw cookies.

## Project Structure

```
src/
  lib.rs          — Module declarations, prelude
  client.rs       — ServiceNowClient + ClientBuilder
  config.rs       — TOML config, env vars, URL normalization
  error.rs        — Error types
  auth/           — Authentication strategies
  transport/      — HTTP client, retry, rate limiting, response parsing
  schema/         — Schema definitions, loader, registry
  query/          — Query builder, filters, batching, strategy
  model/          — Record, FieldValue, QueryResult
  api/            — API-specific constants/helpers
definitions/
  base/           — Bundled schema definitions per release
```

## Running

```bash
cargo check          # Type check
cargo test           # Run all tests
cargo clippy         # Lint
cargo doc --open     # Generate docs
```
