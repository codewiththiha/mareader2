//! A checkpoint's tensors, widened to f32 under their upstream names.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};

use crate::{Error, Result};

/// Every tensor of a GGUF or safetensors checkpoint, by name.
pub(crate) struct Weights {
    all: HashMap<String, Tensor>,
}

impl Weights {
    /// Reads `path`, GGUF by its extension, else safetensors.
    pub(crate) fn read(path: &Path) -> Result<Self> {
        let bad = |e: &dyn std::fmt::Display| Error::Load(format!("{}: {e}", path.display()));
        let gguf = path.extension().is_some_and(|e| e == "gguf");
        let raw = match gguf {
            true => read_gguf(path).map_err(|e| bad(&e))?,
            false => candle_core::safetensors::load(path, &Device::Cpu).map_err(|e| bad(&e))?,
        };
        let mut all = HashMap::with_capacity(raw.len());
        for (name, t) in raw {
            all.insert(name, t.to_dtype(DType::F32)?);
        }
        Ok(Self { all })
    }

    /// The safetensors file in memory, as a voice is.
    pub(crate) fn from_safetensors(bytes: &[u8]) -> Result<Self> {
        let raw = candle_core::safetensors::load_buffer(bytes, &Device::Cpu)
            .map_err(|e| Error::Load(e.to_string()))?;
        let mut all = HashMap::with_capacity(raw.len());
        for (name, t) in raw {
            all.insert(name, t.to_dtype(DType::F32)?);
        }
        Ok(Self { all })
    }

    /// Tensor `name`, checked to have `dims`.
    pub(crate) fn get(&self, name: &str, dims: &[usize]) -> Result<Tensor> {
        let t = (self.all.get(name)).ok_or_else(|| Error::Load(format!("no tensor {name}")))?;
        match t.dims() == dims {
            true => Ok(t.clone()),
            false => Err(Error::Load(format!(
                "{name} is {:?}, not {dims:?}",
                t.dims()
            ))),
        }
    }

    /// Tensor `name`, whatever its shape.
    pub(crate) fn any(&self, name: &str) -> Result<Tensor> {
        let t = self
            .all
            .get(name)
            .ok_or_else(|| Error::Load(format!("no tensor {name}")))?;
        Ok(t.clone())
    }

    /// Tensor `name` as a flat list, checked to have `dims`.
    pub(crate) fn vec(&self, name: &str, dims: &[usize]) -> Result<Vec<f32>> {
        Ok(self.get(name, dims)?.flatten_all()?.to_vec1()?)
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.all.contains_key(name)
    }
}

/// A GGUF file's tensors, dequantised.
fn read_gguf(path: &Path) -> candle_core::Result<HashMap<String, Tensor>> {
    let mut file = BufReader::new(File::open(path)?);
    let content = gguf_file::Content::read(&mut file)?;
    let mut all = HashMap::with_capacity(content.tensor_infos.len());
    for name in content.tensor_infos.keys() {
        let t = content.tensor(&mut file, name, &Device::Cpu)?;
        all.insert(name.clone(), t.dequantize(&Device::Cpu)?);
    }
    Ok(all)
}
