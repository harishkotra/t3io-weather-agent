#[derive(serde::Deserialize)]
pub struct GetWeatherReq {
    pub city: String,
}

#[derive(serde::Serialize)]
pub struct WeatherResp {
    pub city: String,
    pub temperature: f64,
    pub feels_like: f64,
    pub humidity: u32,
    pub description: String,
    pub wind_speed: f64,
}

const OWM_BASE: &str = "https://api.openweathermap.org";

pub fn get_weather(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: GetWeatherReq =
        serde_json::from_slice(input).map_err(|e| alloc::format!("get-weather: bad input: {e}"))?;

    if req.city.trim().is_empty() {
        return Err("get-weather: city must not be empty".to_string());
    }

    #[cfg(target_arch = "wasm32")]
    {
        get_weather_wasm(req)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("get_weather is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http as http_iface, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn get_weather_wasm(req: GetWeatherReq) -> Result<Vec<u8>, String> {
    let api_key = get_api_key()?;

    let url = alloc::format!(
        "{OWM_BASE}/data/2.5/weather?q={}&appid={}&units=metric",
        req.city,
        api_key,
    );

    let _ = logging::info(&alloc::format!(
        "calling OpenWeatherMap for city: {}",
        req.city
    ));

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url,
        headers: None,
        payload: None,
    })
    .map_err(|e| alloc::format!("openweathermap call: {e}"))?;

    if resp.code != 200 {
        let body = alloc::string::String::from_utf8_lossy(&resp.payload);
        return Err(alloc::format!(
            "OpenWeatherMap returned HTTP {}: {body}",
            resp.code
        ));
    }

    let data: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| alloc::format!("parse response: {e}"))?;

    let weather = WeatherResp {
        city: req.city,
        temperature: data["main"]["temp"].as_f64().unwrap_or(0.0),
        feels_like: data["main"]["feels_like"].as_f64().unwrap_or(0.0),
        humidity: data["main"]["humidity"].as_u64().unwrap_or(0) as u32,
        description: data["weather"][0]["description"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        wind_speed: data["wind"]["speed"].as_f64().unwrap_or(0.0),
    };

    let _ = logging::info(&alloc::format!(
        "weather for {}: {:.1}°C, {}",
        weather.city,
        weather.temperature,
        weather.description,
    ));

    serde_json::to_vec(&weather).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn get_api_key() -> Result<alloc::string::String, alloc::string::String> {
    let tid = tenant_context::tenant_did();
    let map_name = alloc::format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, b"weather_api_key")
        .map_err(|e| alloc::format!("kv read: {e}"))?
        .ok_or("weather_api_key not found in z:<tid>:secrets — populate it via the tenant SDK before use")?;
    alloc::string::String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_weather_non_wasm_returns_err() {
        let input = serde_json::to_vec(&json!({ "city": "Tokyo" })).unwrap();
        let result = get_weather(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn get_weather_bad_input_returns_err() {
        let result = get_weather(b"not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }

    #[test]
    fn get_weather_empty_city_returns_err() {
        let input = serde_json::to_vec(&json!({ "city": "" })).unwrap();
        let result = get_weather(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("city must not be empty"));
    }
}
