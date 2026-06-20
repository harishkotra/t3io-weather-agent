import "dotenv/config";
import {
  T3nClient,
  TenantClient,
  setEnvironment,
  loadWasmComponent,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  getNodeUrl,
  getScriptVersion,
} from "@terminal3/t3n-sdk";
import { readFile } from "fs/promises";

// ── Config (set these in .env) ──────────────────────────────────────────
const T3N_API_KEY = (process.env.T3N_API_KEY ?? "").trim();
const OWM_API_KEY = (process.env.OWM_API_KEY ?? "").trim();
const CONTRACT_TAIL = "weather-agent";
const CONTRACT_VERSION = "0.1.2";
const WASM_PATH = "target/wasm32-wasip2/release/z_tenant_weather.wasm";

async function main() {
  if (!T3N_API_KEY) throw new Error("T3N_API_KEY is not set in .env");
  if (!OWM_API_KEY) throw new Error("OWM_API_KEY is not set in .env");

  // ── 1. Authenticate ──────────────────────────────────────────────────
  setEnvironment("testnet");

  const wasmComponent = await loadWasmComponent();
  const address = eth_get_address(T3N_API_KEY);

  const t3n = new T3nClient({
    wasmComponent,
    handlers: {
      EthSign: metamask_sign(address, undefined, T3N_API_KEY),
    },
  });

  await t3n.handshake();
  const did = await t3n.authenticate(createEthAuthInput(address));
  const tenantDid = did.value;

  const tenant = new TenantClient({ t3n, baseUrl: getNodeUrl(), tenantDid });
  console.log("authenticated as", tenantDid);

  const tenantId = tenantDid.slice("did:t3n:".length);
  const scriptName = `z:${tenantId}:${CONTRACT_TAIL}`;

  // ── 2. Register contract (skip if already at this version) ────────────
  let contractId;
  try {
    const wasmBytes = await readFile(WASM_PATH);
    const result = await tenant.contracts.register({
      tail: CONTRACT_TAIL,
      version: CONTRACT_VERSION,
      wasm: wasmBytes,
    });
    contractId = result.contract_id as number;
    console.log(`  ✓ register     ${scriptName} (contract id ${contractId})`);
  } catch (e) {
    if (
      e instanceof Error &&
      e.message.includes("not higher than current version")
    ) {
      console.log("  ~ register     already deployed, skipping");
    } else {
      throw e;
    }
  }

  // ── 3. Create secrets map + seed API key ──────────────────────────────
  if (contractId) {
    try {
      await tenant.maps.create({
        tail: "secrets",
        visibility: "private",
        writers: { only: [contractId] },
        readers: { only: [contractId] },
      });
    } catch (e) {
      if (e instanceof Error && e.message.includes("map already exists")) {
        await tenant.maps.update("secrets", {
          writers: { only: [contractId] },
          readers: { only: [contractId] },
        });
      } else {
        throw e;
      }
    }

    await tenant.executeControl("map-entry-set", {
      map_name: tenant.canonicalName("secrets"),
      key: "weather_api_key",
      value: OWM_API_KEY,
    });
    console.log("  ✓ seed         weather_api_key → z:<tid>:secrets");
    console.log("  ✓ egress       api.openweathermap.org");
  } else {
    console.log("  ~ secrets      already configured");
    console.log("  ~ egress       already authorized");
  }

  // ── 4. Self-grant HTTP egress ────────────────────────────────────────
  if (contractId) {
    const userContractVersion = await getScriptVersion(
      getNodeUrl(),
      "tee:user/contracts",
    );
    await t3n.execute({
      script_name: "tee:user/contracts",
      script_version: userContractVersion,
      function_name: "agent-auth-update",
      input: {
        agents: [
          {
            agentDid: tenantDid,
            scripts: [
              {
                scriptName: scriptName,
                versionReq: CONTRACT_VERSION,
                functions: ["get-weather"],
                allowedHosts: ["api.openweathermap.org"],
              },
            ],
          },
        ],
      },
    });
  }

  // ── 5. Invoke (always runs) ──────────────────────────────────────────
  const scriptVersion = await getScriptVersion(getNodeUrl(), scriptName);

  const result = await t3n.executeAndDecode({
    script_name: scriptName,
    script_version: scriptVersion,
    function_name: "get-weather",
    input: { city: "Tokyo" },
  });

  console.log("\nweather result:", JSON.stringify(result, null, 2));
}

main().catch((err) => {
  console.error("deploy failed:", err);
  process.exit(1);
});
