use log::{error, info};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::time::Instant;

struct ExtractedFields {
    name: String,
    tumonline_room_nr: Option<i32>,
    r#type: String,
    type_common_name: String,
    lat: f32,
    lon: f32,
}
struct StorableValue;

impl StorableValue {
    fn from(value: Value) -> Option<(String, ExtractedFields)> {
        let obj = value.as_object()?;
        Some((
            obj.get("id")?.as_str()?.to_string(),
            ExtractedFields {
                name: obj.get("name")?.as_str()?.to_string(),
                tumonline_room_nr: Some(1),
                r#type: obj.get("type")?.as_str()?.to_string(),
                type_common_name: obj.get("type_common_name")?.as_str()?.to_string(),
                lat: 1.0,
                lon: 1.0,
            },
        ))
    }
}

fn delocalise(value: Value, language: &'static str) -> Value {
    match value {
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|value| delocalise(value, language))
                .collect(),
        ),
        Value::Object(obj) => {
            if obj.contains_key("de") || obj.contains_key("en") {
                obj.get(language)
                    .cloned()
                    .unwrap_or(Value::String("".to_string()))
            } else {
                Value::Object(
                    obj.into_iter()
                        .map(|(key, value)| (key, delocalise(value, language)))
                        .filter(|(key, _)| key != "de" && key != "en")
                        .collect(),
                )
            }
        }
        a => a,
    }
}

struct DelocalisedValues {
    key: String,
    de: Value,
    en: Value,
}

impl From<(String, Value)> for DelocalisedValues {
    fn from((key, value): (String, Value)) -> Self {
        Self {
            de: delocalise(value.clone(), "de"),
            en: delocalise(value.clone(), "en"),
            key,
        }
    }
}

impl DelocalisedValues {
    async fn store(
        self,
        pool: &SqlitePool,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        let key = self.key.clone(); // has to be here due to livetimes somehow
        if let Some((data, fields)) = StorableValue::from(self.de) {
            sqlx::query!(
                r#"INSERT INTO de(key,data,name,tumonline_room_nr,type,type_common_name,lat,lon)
            VALUES (?,?,?,?,?,?,?,?)"#,
                key,
                data,
                fields.name,
                fields.tumonline_room_nr,
                fields.r#type,
                fields.type_common_name,
                fields.lat,
                fields.lon,
            )
            .execute(pool)
            .await?;
        } else {
            error!("failed to store de for {key}");
            return Err(sqlx::Error::Protocol(format!(
                "failed to store de for {key}"
            )));
        }
        if let Some((data, fields)) = StorableValue::from(self.en) {
            sqlx::query!(
                r#"INSERT INTO en(key,data,name,tumonline_room_nr,type,type_common_name,lat,lon)
            VALUES (?,?,?,?,?,?,?,?)"#,
                self.key,
                data,
                fields.name,
                fields.tumonline_room_nr,
                fields.r#type,
                fields.type_common_name,
                fields.lat,
                fields.lon,
            )
            .execute(pool)
            .await
        } else {
            error!("failed to store de for {key}");
            Err(sqlx::Error::Protocol(format!(
                "failed to store de for {key}"
            )))
        }
    }
}
pub(crate) async fn load_all_to_db(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let cdn_url = std::env::var("CDN_URL").unwrap_or_else(|_| "https://nav.tum.de/cdn".to_string());
    let raw_tasks = reqwest::get(format!("{cdn_url}/api_data.json"))
        .await?
        .json::<HashMap<String, Value>>()
        .await?;
    let start = Instant::now();
    for task in raw_tasks.into_iter().map(DelocalisedValues::from) {
        task.store(pool).await?;
    }
    info!("loaded data in {elapsed:?}", elapsed = start.elapsed());

    Ok(())
}
