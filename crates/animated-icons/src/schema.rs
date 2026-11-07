//! Shared serde schema for the declarative animated-icon source data.

use serde::{Deserialize, Deserializer};

use super::Prop;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IconSource {
    pub(crate) easings: Vec<Option<[f32; 4]>>,
    pub(crate) packs: Vec<Pack>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pack {
    pub(crate) id: String,
    pub(crate) dur: f32,
    pub(crate) parts: Vec<Part>,
    pub(crate) segs: Vec<Seg>,
    pub(crate) holes: Vec<Hole>,
    pub(crate) svg: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Part {
    pub(crate) name: String,
    pub(crate) ox: f32,
    pub(crate) oy: f32,
    pub(crate) len: f32,
    pub(crate) w: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Seg {
    pub(crate) part: u8,
    pub(crate) prop: Prop,
    pub(crate) t0: f32,
    pub(crate) dur: f32,
    pub(crate) keys: Vec<f32>,
    pub(crate) times: Vec<f32>,
    pub(crate) eases: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hole {
    pub(crate) part: u8,
    pub(crate) width: bool,
}

impl<'de> Deserialize<'de> for Prop {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        match name.as_str() {
            "X" => Ok(Self::X),
            "Y" => Ok(Self::Y),
            "Scale" => Ok(Self::Scale),
            "ScaleX" => Ok(Self::ScaleX),
            "ScaleY" => Ok(Self::ScaleY),
            "Rotate" => Ok(Self::Rotate),
            "SkewX" => Ok(Self::SkewX),
            "SkewY" => Ok(Self::SkewY),
            "Opacity" => Ok(Self::Opacity),
            "PathLen" => Ok(Self::PathLen),
            "PathOff" => Ok(Self::PathOff),
            "StrokeW" => Ok(Self::StrokeW),
            _ => Err(serde::de::Error::custom(format!(
                "unknown animated-icon property {name:?}"
            ))),
        }
    }
}
