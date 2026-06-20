#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

extern crate alloc;

pub const CONTRACT_VERSION: &str = "0.1.1";

wit_bindgen::generate!({
    world: "tenant-weather",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

mod weather;

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_weather::contracts::Guest for Component {
    fn get_weather(
        req: exports::z::tenant_weather::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("get-weather: missing input")?;
        weather::get_weather(&input)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;

    #[test]
    fn contract_version_is_semver() {
        let parts: Vec<&str> = CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "each part must be a number");
        }
    }

    #[test]
    fn contract_version_is_v0_1_1() {
        assert_eq!(CONTRACT_VERSION, "0.1.1");
    }
}
