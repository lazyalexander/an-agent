//! A minimal payload contract: the strict subset of JSON Schema we honor.
//! Unknown keys are refused at config admission — a schema we half-read
//! is worse than none. Extra payload keys pass by default, as in JSON
//! Schema; a closed shape says `additionalProperties = false`.

use std::collections::BTreeMap;

/// A checked contract, parsed once at config admission.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    type_: Option<Type>,
    required: Vec<String>,
    properties: BTreeMap<String, Schema>,
    items: Option<Box<Schema>>,
    additional_properties: Option<bool>,
    enum_values: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Object,
    Array,
    String,
    Number,
    Integer,
    Boolean,
    Null,
}

impl Schema {
    pub fn from_toml(value: toml::Value) -> Result<Self, String> {
        let table = value.as_table().ok_or("schema must be a table")?.clone();
        let mut schema = Schema {
            type_: None,
            required: Vec::new(),
            properties: BTreeMap::new(),
            items: None,
            additional_properties: None,
            enum_values: None,
        };
        for (key, value) in table {
            match key.as_str() {
                "type" => {
                    let name = value.as_str().ok_or("type must be a string")?;
                    schema.type_ = Some(match name {
                        "object" => Type::Object,
                        "array" => Type::Array,
                        "string" => Type::String,
                        "number" => Type::Number,
                        "integer" => Type::Integer,
                        "boolean" => Type::Boolean,
                        "null" => Type::Null,
                        other => return Err(format!("unknown type: {other}")),
                    });
                }
                "required" => {
                    let items = value.as_array().ok_or("required must be an array")?;
                    schema.required = items
                        .iter()
                        .map(|item| {
                            item.as_str()
                                .map(str::to_string)
                                .ok_or_else(|| "required entries must be strings".to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                }
                "properties" => {
                    let props = value.as_table().ok_or("properties must be a table")?;
                    schema.properties = props
                        .iter()
                        .map(|(name, sub)| {
                            Schema::from_toml(sub.clone()).map(|parsed| (name.clone(), parsed))
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()?;
                }
                "items" => {
                    schema.items = Some(Box::new(Schema::from_toml(value)?));
                }
                "additionalProperties" => {
                    schema.additional_properties = Some(
                        value
                            .as_bool()
                            .ok_or("additionalProperties must be a boolean")?,
                    );
                }
                "enum" => {
                    let items = value.as_array().ok_or("enum must be an array")?;
                    schema.enum_values = Some(
                        items
                            .iter()
                            .map(|item| {
                                serde_json::to_value(item)
                                    .map_err(|err| format!("unsupported enum value: {err}"))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                other => return Err(format!("unknown schema key: {other}")),
            }
        }
        Ok(schema)
    }

    /// Check one payload. The error names the offending path.
    pub fn check(&self, value: &serde_json::Value) -> Result<(), String> {
        self.check_at(value, "$")
    }

    fn check_at(&self, value: &serde_json::Value, path: &str) -> Result<(), String> {
        if let Some(type_) = self.type_ {
            let ok = match type_ {
                Type::Object => value.is_object(),
                Type::Array => value.is_array(),
                Type::String => value.is_string(),
                Type::Number => value.is_number(),
                Type::Integer => value.is_i64() || value.is_u64(),
                Type::Boolean => value.is_boolean(),
                Type::Null => value.is_null(),
            };
            if !ok {
                return Err(format!("{path}: expected {type_:?}"));
            }
        }
        if let Some(allowed) = &self.enum_values
            && !allowed.contains(value)
        {
            return Err(format!("{path}: not one of the enum values"));
        }
        if let Some(object) = value.as_object() {
            for key in &self.required {
                if !object.contains_key(key) {
                    return Err(format!("{path}: missing required key {key}"));
                }
            }
            for (key, sub) in &self.properties {
                if let Some(inner) = object.get(key) {
                    sub.check_at(inner, &format!("{path}.{key}"))?;
                }
            }
            if self.additional_properties == Some(false) {
                for key in object.keys() {
                    if !self.properties.contains_key(key) {
                        return Err(format!("{path}: unexpected key {key}"));
                    }
                }
            }
        }
        if let (Some(items), Some(array)) = (&self.items, value.as_array()) {
            for (index, inner) in array.iter().enumerate() {
                items.check_at(inner, &format!("{path}[{index}]"))?;
            }
        }
        Ok(())
    }
}
