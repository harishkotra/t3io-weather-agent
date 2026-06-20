# Weather Agent — TEE-Powered Confidential AI Contract

A **privacy-first weather agent** built on [Terminal 3 (T3N)](https://docs.terminal3.io). A Rust TEE contract runs inside an Intel TDX hardware enclave, reads an API key from a sealed key-value store, calls OpenWeatherMap, and returns the weather — **without the API key ever leaving the enclave**.

```json
{
  "city": "Tokyo",
  "temperature": 23.18,
  "feels_like": 23.85,
  "humidity": 88,
  "description": "moderate rain",
  "wind_speed": 4.78
}
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  User CLI (tsx)                                             │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  scripts/deploy-and-invoke.ts                        │   │
│  │  - Authenticate via T3N SDK                          │   │
│  │  - Register WASM contract                            │   │
│  │  - Seed API key into sealed map                      │   │
│  │  - Self-grant HTTP egress                            │   │
│  │  - Invoke contract, decode response                  │   │
│  └──────────┬───────────────────────────────────────────┘   │
└─────────────┼───────────────────────────────────────────────┘
              │ encrypted session (ML-KEM)
              ▼
┌─────────────────────────────────────────────────────────────┐
│  T3N Node                                                    │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Dispatch → TDX Enclave                              │   │
│  │  ┌──────────────────────────────────────────────┐    │   │
│  │  │  Weather Contract (Rust → WASM)              │    │   │
│  │  │                                              │    │   │
│  │  │  1. kv_store::get("weather_api_key") ────────┼────┼───│──→ Sealed Map
│  │  │  2. http_iface::call(GET /data/2.5/weather) ─┼────┼───│──→ api.openweathermap.org
│  │  │  3. return { temperature, humidity, ... }    │    │   │
│  │  └──────────────────────────────────────────────┘    │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Key Properties

- **API key never leaves the TEE** — seeded via control plane `map-entry-set`, read via `kv_store::get()` inside the enclave
- **Outbound HTTP is authorized per-call** — the contract can only dial hosts the user signed off on (`api.openweathermap.org`)
- **Code is auditable** — WASM component, open source, deterministic
- **Deploy script is idempotent** — re-running skips setup, zero token waste

---

## Technologies

| Layer | Technology |
|---|---|
| **TEE Contract** | Rust → `wasm32-wasip2` (WASI Preview 2 component) |
| **Host Interfaces** | `kv-store`, `http`, `logging`, `tenant-context` (declared in WIT) |
| **Code Generation** | `wit-bindgen` 0.49 — generates Guest/Host bindings from WIT |
| **Serialization** | `serde` + `serde_json` — `no_std` + `alloc` |
| **SDK** | `@terminal3/t3n-sdk` 3.9.0 — TypeScript client for T3N |
| **Network** | T3N testnet (public sandbox with test tokens) |
| **External API** | OpenWeatherMap (api.openweathermap.org) |
| **Deploy Runner** | `tsx` 4.22.4 — execute TypeScript scripts directly |

---

## Project Structure

```
weather-agent/
├── .cargo/config.toml           # wasm32-wasip2 default build target
├── Cargo.toml                   # Rust deps: wit-bindgen, serde, serde_json, hex
├── package.json                 # TS deps: @terminal3/t3n-sdk, dotenv, tsx
├── wit/
│   ├── world.wit                # Contract WIT interface
│   └── deps/
│       ├── host-interfaces-2.1.0/package.wit   # http, kv-store, logging
│       └── host-tenant-1.0.0/package.wit       # tenant-context
├── src/
│   ├── lib.rs                   # wit-bindgen entry, Guest dispatch
│   └── weather.rs               # OpenWeatherMap logic, key retrieval
└── scripts/
    └── deploy-and-invoke.ts     # Full lifecycle: auth → register → setup → invoke
```

---

## Getting Started

### Prerequisites

- Node.js >= 18
- Rust toolchain with `wasm32-wasip2` target (`rustup target add wasm32-wasip2`)
- A T3N API key from [terminal3.io/claim-page](https://terminal3.io/claim-page)
- An OpenWeatherMap API key from [openweathermap.org/api](https://openweathermap.org/api)

### Setup

```bash
git clone <repo>
cd weather-agent

# Install JS dependencies
npm install

# Build the WASM contract
cargo build --release
```

### Configure

Create `.env` in the project root:

```env
T3N_API_KEY=your_ethereum_private_key_from_terminal3
OWM_API_KEY=your_openweathermap_api_key
```

### Run

```bash
npm run deploy
```

First run: registers the contract, creates the sealed secrets map, seeds the API key, authorizes HTTP egress, then invokes.

Subsequent runs: skip setup (contract + map + auth already exist), just invoke. Zero token waste.

### Tests

```bash
cargo test --target aarch64-apple-darwin --lib
```

---

## How It Works

### 1. WIT Interface (`wit/world.wit`)

Declares what the contract imports from the host (`kv-store`, `http`, `logging`, `tenant-context`) and exports to callers (`get-weather`).

```wit
world tenant-weather {
    import host:tenant/tenant-context@1.0.0;
    import host:interfaces/logging@2.1.0;
    import host:interfaces/kv-store@2.1.0;
    import host:interfaces/http@2.1.0;
    export contracts;
}
```

### 2. Rust Contract (`src/weather.rs`)

Inside the TEE, the contract:
1. Reads the tenant DID to construct the secrets map name
2. Calls `kv_store::get()` to retrieve the API key
3. Makes an HTTP GET to OpenWeatherMap
4. Parses the JSON response and returns structured weather data

```rust
fn get_api_key() -> Result<String, String> {
    let tid = tenant_context::tenant_did();
    let map_name = format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, b"weather_api_key")?
        .ok_or("weather_api_key not found")?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
```

### 3. Deploy Script (`scripts/deploy-and-invoke.ts`)

The TypeScript orchestrator handles the full lifecycle:

```typescript
// Register WASM component
const { contract_id } = await tenant.contracts.register({
  tail: "weather-agent", version: "0.1.0", wasm: wasmBytes,
});

// Create TEE-sealed KV map (only this contract can read/write)
await tenant.maps.create({
  tail: "secrets", visibility: "private",
  writers: { only: [contract_id] },
  readers: { only: [contract_id] },
});

// Seed API key (control-plane bypasses map ACL)
await tenant.executeControl("map-entry-set", {
  map_name: tenant.canonicalName("secrets"),
  key: "weather_api_key", value: OWM_API_KEY,
});

// Self-grant outbound HTTP
await t3n.execute({
  script_name: "tee:user/contracts",
  function_name: "agent-auth-update",
  input: { agents: [{ agentDid: tenantDid, scripts: [{
    scriptName, functions: ["get-weather"],
    allowedHosts: ["api.openweathermap.org"],
  }]}]},
});

// Invoke
const result = await t3n.executeAndDecode({
  script_name: scriptName, function_name: "get-weather",
  input: { city: "Tokyo" },
});
```

---

## Contributing / Ideas for Extension

This weather agent is a minimal demo — the same pattern can be extended to build far more powerful confidential agents. Here are ideas:

### New Features to Add

| Feature | Description | Files to Touch |
|---|---|---|
| **Multi-city forecast** | Accept an array of cities, batch-call OpenWeatherMap | `src/weather.rs`, `wit/world.wit` |
| **Cached responses** | Store recent results in a second KV map to save tokens on repeated queries | `src/weather.rs` |
| **Geolocation from IP** | Auto-detect the caller's city via ip-api.com inside the TEE | `src/weather.rs`, allow more hosts |
| **Weather alerts** | Check NWS/NOAA alert feeds for severe weather warnings | `src/weather.rs`, new WIT function |
| **Historical comparison** | Fetch and compare today vs yesterday vs same-day-last-year | `src/weather.rs` |
| **PII-safe booking agent** | Combine weather with travel booking — passenger PII never enters the TEE memory (use `http-with-placeholders`) | New contract functions, new WIT imports |
| **Slack/Discord bot** | Invoke the contract from a chat slash command | New `scripts/slash-bot.ts` |
| **Cron-triggered reports** | Use T3N webhook scheduling to run daily weather reports | `scripts/deploy-and-invoke.ts` |
| **Multi-provider failover** | Fall back to WeatherAPI.com or VisualCrossing if OpenWeatherMap is down | `src/weather.rs` |

### How to Contribute

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Implement your changes
4. Update tests in `src/weather.rs`
5. Run `cargo build --release && cargo test --lib`
6. Submit a pull request

---

## Live Demo Output

```
$ npm run deploy

authenticated as did:t3n:2c9d71730c17e69e394afbffb973d1a6444f4423
registered z:2c9d71730c17e69e394afbffb973d1a6444f4423:weather-agent as contract id 260
seeded weather_api_key into z:<tid>:secrets
authorized outbound HTTP to api.openweathermap.org

weather result: {
  "city": "Tokyo",
  "temperature": 23.18,
  "feels_like": 23.85,
  "humidity": 88,
  "description": "moderate rain",
  "wind_speed": 4.78
}
```

---

## License

MIT
